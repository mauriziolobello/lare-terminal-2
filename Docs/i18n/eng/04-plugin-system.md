# Plugin system

This document explains Lare Terminal's plugin system: what it is, how a plugin lives within the
program, and how to write a new plugin. For a description of the project as a whole, see
[`00-opening.md`](./00-opening.md); for the overall architectural picture,
[`01-architecture.md`](./01-architecture.md); for what currently exists plugin by plugin,
[`03-status-and-implementation.md`](./03-status-and-implementation.md).

## What a plugin is

A plugin is a **separate process** (a standalone executable, not a library loaded inside the
orchestrator) communicating with the orchestrator via its own protocol: line-by-line JSON over
stdin/stdout — internally referred to as "Contract P", defined entirely in the `plugin-protocol`
crate. When a plugin has something to display, it opens a **dedicated Tauri window**, distinct from
the terminal and every other application window.

This is intentionally the simplest design that could possibly work: no dynamic in-process plugin
loading (no `dlopen`/shared DLLs, no WASM), and no internal runtime APIs exposed. A plugin is
effectively a second program, launched and monitored by the orchestrator, communicating via messages.

**Why a separate process rather than an in-process library.** The rationale is fault isolation. A
plugin that panics, deadlocks, or becomes unresponsive does not take down the orchestrator or other
plugins — it remains an isolated process whose failure is observed as such and does not propagate:
the initial handshake (`Init` → `Ready`) has a 5-second timeout, after which the orchestrator gives
up and marks the plugin unavailable; a failed write to an already active plugin (broken pipe,
terminated process) drops the write channel, so that the next command requesting it restarts it from
scratch instead of continuing to knock on a dead process; upon orchestrator shutdown, every plugin
process is terminated (`kill_on_drop`), never left orphaned. None of these guarantees would be
possible at comparable cost with code loaded into the orchestrator process itself.

It is equally important to state clearly what this isolation **is not**: it is not a security
sandbox. A plugin runs as a native process with the same privileges as the user who launched the
program — there is no boundary preventing it from reading files or accessing the network if its code
were to do so. The only effectively contained element is the HTML that a plugin produces for its
window: it passes through host-side DOMPurify before rendering (the same precaution described in
[`01-architecture.md`](./01-architecture.md) for the AI Markdown windows), ensuring that dangerous
scripts or attributes within plugin HTML cannot cross that boundary. The only plugin in this
repository not intended for real-world use declares this explicitly in its code: **`crypto`
implements "textbook" RSA without OAEP padding and without constant-time arithmetic — an educational
tool, not suitable for encrypting anything of real value.**

## On the filesystem: `plugins/<id>/`

Each plugin lives in its own subdirectory inside `plugins/`, requiring only two files:

```
plugins/
  <id>/
    plugin.json      -- static manifest
    <id>(.exe)        -- plugin binary (.exe on Windows, no extension elsewhere)
```

**Discovery**, executed by the orchestrator on every launch, scans this directory: it reads
`plugin.json`, and if the manifest is valid, looks by convention — without needing declaration
elsewhere — for a binary sharing the same name as the `id` within the same folder. If the manifest
is missing, malformed, or the binary does not exist, that subdirectory is skipped with a warning on
stderr: never a fatal error; a broken plugin does not prevent others from starting.

The manifest is minimal:

```json
{
  "name": "Calcolatrice",
  "id": "calc",
  "version": "1.0.0",
  "protocol_version": 1,
  "triggers": { "command": "/calc" },
  "window": { "width": 960.0, "height": 620.0 }
}
```

| Field | Mandatory | Meaning |
|---|---|---|
| `name` | yes | Display name (default window title). |
| `id` | yes | Stable identifier: folder name, binary name, routing key. |
| `version` | yes | *Plugin* version as a product — independent of the Rust crate version implementing it. |
| `protocol_version` | yes | Protocol version spoken by the plugin, for future negotiation. |
| `triggers.command` | no | Slash command triggering the plugin (e.g. `/calc`). Absent by default. |
| `triggers.interval` | no | Timer interval (e.g. `"5m"`) — declarable today, not yet executed (see below). |
| `window` | no | Initial window dimensions. Absent → host applies generic default (480×360). |

Unrecognized fields in the JSON are silently ignored (forward extensibility); missing optional
fields assume harmless defaults — a manifest without `triggers` equates to `triggers: {}`, one
without `window` equates to "no size preference".

**The `id` is restricted for security**, not merely convention: it must consist exclusively of
ASCII letters/digits, `_`, and `-`. The reason is that binary paths are constructed by joining the
plugins directory with the `id` read from the manifest, and two path-resolution behaviors make this
perilous if unvalidated: an `id` containing `..` traverses outside the plugins directory (standard
relative path resolution), while an absolute path or drive letter (e.g. `C:/Windows/calc`) causes
path concatenation to **discard the prefix** and return only the second path — this is standard
behavior for `Path::join` in Rust, and elsewhere. In either case, the discovered "plugin binary"
could be an arbitrary system executable. Discovery discards the entire plugin before even checking
for the binary if `id` contains characters outside that safe set.

## Lifecycle

A plugin declaring `triggers.interval` — or no triggers at all — is started **eagerly**, immediately
upon orchestrator launch. A plugin declaring **only** `triggers.command` is started **lazily**, upon
first use: saving a process for every plugin never invoked during that session.

| `triggers` | Policy |
|---|---|
| `interval` present | Eager — must already be running to receive future `OnTimer` |
| `command` only | Lazy — starts upon the first slash command requesting it |
| neither | Eager (degenerate case, for simplicity) |

Whether eager or lazy, startup always follows the same sequence:

1. The orchestrator spawns the process (`Command::new(bin_path)`, with the same `--config-dir`
   argument received by every binary in the project — see the "single rule" of configuration in
   [`01-architecture.md`](./01-architecture.md). No plugin currently reads that argument, but the
   convention holds uniformly, without exceptions for plugins, in anticipation of the day one might
   need it).
2. It sends `Init { protocol_version, config, storage_dir }` — always the very first message.
3. It awaits **`Ready`** as the first response, within **5 seconds**. Any other outcome — a
   different message, EOF, timeout — and the plugin does not enter service: the orchestrator logs
   this to stderr and proceeds with others.
4. From here on, the plugin is "active": the orchestrator maintains a write channel to it and a
   dedicated task reads its output in a loop, translating each message toward the UI.

When the declared slash command arrives — one of those lines starting with `/`, the *power commands*
introduced in [`00-opening.md`](./00-opening.md), matched by exact trimmed string equality rather
than prefix — the orchestrator sends `Activate { window_id, args }`: `window_id` is a monotonically
increasing counter that is never reused; `args` currently always carries `null` (slash commands do
not yet pass arguments to plugins). The plugin responds with `ShowWindow`, and from that moment
onward, every user interaction in the window (clicks, typing) arrives as a `UiEvent`, to which the
plugin responds with `UpdateWindow` — a full replacement of the window's HTML, rather than a partial
patch (rationale detailed below).

Two details are essential when authoring a plugin:

- **Closing the window does not terminate the plugin.** When the user clicks the window's ✕, the
  host unregisters that window (its `window_id` is no longer routed), but the plugin process remains
  alive with all its state intact — reopening it (same slash command) can display state remembered
  from the previous session if intended (`lc` does this explicitly, saving both panel paths to its
  storage upon every navigation).
- **`Deinit` arrives only on orchestrator shutdown**, never upon closing a single window. It is the
  only "clean" shutdown signal a plugin receives; a plugin needing reliable persistence cannot rely
  solely on that moment, and should persist state on every significant change — again, the approach
  taken by `lc`, which does not assume a guaranteed `Deinit` for every individual session.
- A plugin **can open more than one window on its own initiative**, not just the one provided by
  `Activate`, by generating its own `window_id`s (typically with an offset that avoids colliding
  with host-assigned IDs). `crypto` does this for its cipher parameter dialog: a genuine second
  Tauri window, independent of the main window and movable separately.

## The protocol

The protocol — two tagged JSON enums shared via the `plugin-protocol` crate — is deliberately
compact and additive: each new message type is appended at the end without breaking existing plugins
unaware of it. Every message is a single JSON line featuring a `"type"` field denoting the variant.

**Host → plugin:**

| Message | When | Shape |
|---|---|---|
| `Init` | Always first, following spawn | `{"type":"Init","protocol_version":1,"config":null,"storage_dir":"…/Configuration/plugin-storage/calc"}` |
| `Activate` | Recognized slash command | `{"type":"Activate","window_id":7,"args":null}` |
| `UiEvent` | Click or input in window | `{"type":"UiEvent","window_id":7,"element_id":"inc","value":null}` |
| `Deinit` | Orchestrator shutdown | `{"type":"Deinit"}` |

**Plugin → host:**

| Message | When | Shape |
|---|---|---|
| `Ready` | Mandatory reply to `Init` | `{"type":"Ready","name":"calc","protocol_version":1}` |
| `ShowWindow` | In reply to `Activate` (or unsolicited) | `{"type":"ShowWindow","window_id":7,"title":"Calcolatrice","html":"<div>…</div>"}` |
| `UpdateWindow` | In reply to a `UiEvent` | `{"type":"UpdateWindow","window_id":7,"html":"<div>…</div>"}` |
| `CloseWindow` | When the plugin has nothing left to show | `{"type":"CloseWindow","window_id":7}` |
| `Log` | Freeform diagnostics | `{"type":"Log","level":"info","msg":"…"}` (received but not yet exposed in UI) |

`UiEvent.element_id` corresponds to the `data-evt` attribute of the clicked HTML element (see the
next section); `value` is `Some(...)` for an `<input>`/`<textarea>`/`<select>` (the current field
value), and `None` for a button. `ShowWindow`/`UpdateWindow` always carry the **complete** window
HTML — "full-HTML replace", not a patch tree — because it is the simplest strategy to implement
correctly in a plugin and the most predictable to reason about: the author of
`render_window(&state) -> String` never has to calculate diffs against the previous render, only what
must be present now.

## The window: what a plugin can contain

A plugin window is not an arbitrary webview: it is a shared container governed by precise rules,
designed so a plugin does not have to reinvent standard patterns.

- **Shared CSS.** `plugin-catalog.css`, automatically injected into every plugin window, defines
  common utility classes (`lare-window`, `lare-label`, `lare-button`, `lare-input`…) consistent
  with the dark theme and configurable transparency of the entire application (`--window-alpha`, the
  same variable controlling every other window). Using them is optional, but ignoring them incurs
  real costs: `lc` originally introduced its own CSS color conventions (fully opaque hex codes
  instead of the translucent `rgba()` values used throughout the rest of the application), and had
  to be realigned in a subsequent pass, rule by rule.
- **Events via `data-*` attributes, not plugin JavaScript.** A plugin provides no JavaScript of its
  own: it interacts with the user by declaring `data-evt="something"` on the HTML elements it
  generates. The host runtime intercepts clicks (`data-evt`), double clicks (`data-dblevt`, useful
  for "select and enter" in lists), and physical keys mapped to buttons (`data-key`, evaluated on
  every `keydown`), translating all of them into the uniform `UiEvent` sent to the plugin. An
  `<input>`, `<textarea>`, or `<select>` bearing `data-evt` is also "value-bearing": every keypress
  inside it immediately sends a `UiEvent` with the current value — enabling plugins to react to live
  typing rather than solely explicit submits.
- **Two-tier sanitization.** HTML produced by a plugin passes through DOMPurify on the host side
  before rendering (no `<script>`, no inline handlers) — but a prudent plugin also sanitizes user
  text on its own side, at string level, before injecting it into HTML (all plugins in this
  repository do so via concise manual escaping of `&`/`<`/`>`/`"`). These are two independent layers;
  neither replaces the other.
- **Dimensions and auto-fitting.** By default, the window automatically fits the height of its
  content following each render (convenient for plugins that grow or shrink, such as the calculator
  switching between a single line and a 2D fraction). A plugin preferring a height freely chosen by
  the user — a file manager with scrolling panels, for example — marks its root element with the
  `data-no-autofit` attribute to opt out of that behavior.
- **Focus and active field values survive `UpdateWindow`**, even though "full-HTML replace"
  recreates every DOM element from scratch: the host captures what the user is typing an instant
  before replacing the HTML and restores it onto the matching new element. Without this precaution,
  typing inside a plugin text field would be nearly impossible (characters appearing and
  disappearing one by one) — a real issue observed during live testing of `crypto` and resolved once
  at platform level, rather than plugin by plugin.

## Current plugins

Five plugins currently reside in this repository. This is not an exhaustive list — it provides a
foundation illustrating what a plugin can achieve, from the simplest to the most feature-rich.

- **`ping`** — the minimal reference plugin: responds `Ready` to `Init` and nothing more, never
  opening a window. It exists to validate the discovery → spawn → handshake chain, and also acts as
  the probe used by the `/ping` diagnostic command (built into the orchestrator, distinct from the
  plugin: it measures `Init → Ready` latency of a disposable instance and reports it alongside the
  health of other system layers).
- **`counter`** — the simplest plugin **with** a window: a counter and a "+1" button. Serves as an
  educational reference for the `Activate → ShowWindow`, `UiEvent → UpdateWindow` cycle.
- **`calc`** — a complete calculator: arithmetic, scientific functions (trigonometry, logarithms,
  powers, roots, factorials), and a **programmer mode** with numeric bases (decimal, hexadecimal,
  octal, binary), configurable bit width, and bitwise operators. It is functionally the most rich
  plugin, illustrating how much internal state and logic a plugin can maintain while remaining an
  ordinary sidecar process.
- **`lc`** ("Lare Commander") — a dual-pane file manager in the style of Midnight Commander: copy,
  move, create directories, delete, compare directories, and diff two files. It leverages its
  `storage_dir` to preserve the last opened path in each panel across restarts — the most concrete
  example of per-plugin persistence in the repository.
- **`crypto`** — classical ciphers (Caesar, Vigenère) and RSA, designed as an educational tool: as
  noted above, its RSA implementation intentionally lacks the protections required for real-world
  security.

## Writing a new plugin

The protocol itself imposes no specific programming language: it is line-by-line JSON over
stdin/stdout, readable and writable by any runtime capable of standard I/O. In practice, today,
**every plugin in the repository is written in Rust** and shares the `plugin-protocol` crate for
message types — the verified path with tests, working examples, and a natural home in the Cargo
workspace. A plugin in another language is architecturally possible (discovery simply searches for a
native executable named `<id>` or `<id>.exe` inside the plugin directory — without inspecting how it
was built), but remains an untested path in this project: taking it for granted without empirical
testing would be optimism rather than a verified fact.

The remainder of this section uses **`ping`** as a reference — the smallest plugin in existence,
whose entire useful logic spans only a few lines.

### 1. The crate

```
crates/plugin-<id>/
  Cargo.toml         -- [[bin]] name = "<id>"; dependency on plugin-protocol (+ serde_json)
  plugin.json          -- manifest (see above)
  src/main.rs           -- I/O loop + logic
  IMPLEMENTATION.md     -- (repository convention) technical details of the plugin
```

The new crate must be added to both `members` and `default-members` in the workspace `Cargo.toml` at
the root of the repository — otherwise `cargo build`/`cargo test` without `-p` will ignore it.

### 2. Logic: a pure function, not a loop full of I/O

Every plugin in this repository separates testable I/O-free logic from the loop reading stdin and
writing stdout. For `ping`, the entire logic is:

```rust
fn handle(msg: HostToPlugin) -> Vec<PluginToHost> {
    match msg {
        HostToPlugin::Init { .. } => {
            vec![PluginToHost::Ready { name: "ping".into(), protocol_version: 1 }]
        }
        HostToPlugin::Deinit {} => Vec::new(),
        HostToPlugin::Activate { .. } | HostToPlugin::UiEvent { .. } => Vec::new(),
    }
}
```

`handle` takes an incoming message and returns zero or more responses — no state, no I/O, easily
tested with a simple `assert_eq!`. A stateful plugin (`counter`, `calc`, …) passes `&mut PluginState`
as the first argument, but the structure remains identical: pure input, pure output.

The loop wrapping `handle` is always the same handful of lines: it reads a line from stdin,
deserializes it into `HostToPlugin`, invokes `handle`, writes each response as a JSON line to stdout
with an explicit `flush`, and exits the loop — terminating the process — after processing `Deinit`.
No asynchronous runtime is required: a stdio plugin is single-task by nature; a blocking loop over
`stdin.lock().lines()` is entirely sufficient.

### 3. The manifest

```json
{ "name": "<Display Name>", "id": "<id>", "version": "1.0.0", "protocol_version": 1, "triggers": {} }
```

`triggers: {}` (or omitted) makes the plugin eager — convenient during development to see it start
immediately without needing a slash command. A plugin meant to be invoked on demand declares
`triggers.command` instead.

### 4. Compile, deploy, register

```powershell
cargo build -p <id>                       # target\debug\<id>.exe
```

For the orchestrator to discover it, the compiled binary and manifest must be placed together under
`plugins/<id>/` in the deployment directory (`Test Run\` during development — see
[`BUILD.md`](./BUILD.md) and [`DEPLOY.md`](./DEPLOY.md)). The `deploy_test_run.ps1 -IncludePlugins`
script automates this copy, but maintains an explicit list of known IDs (`ping`, `calc`, `counter`,
`crypto`, `lc`) — a new plugin must be added to that list, or copied manually, until the script is
extended.

There is no hot-reloading: **the orchestrator must be restarted** after adding or recompiling a
plugin, exactly as with any other backend modification.

### 5. Verify discovery

The "Plugins" tab in the Library window lists every folder under `plugins/` containing a readable
`plugin.json` — including malformed ones, displayed "as-is" rather than hidden, precisely because it
acts as a diagnostic panel: if the plugin does not appear there, the issue precedes execution (wrong
folder, invalid JSON). If it appears there but fails to start when invoked, the issue lies in the
handshake or the binary itself.

### 6. Testing

- **Unit tests on `handle`** (or its stateful counterpart): the style across all existing plugins,
  involving no actual processes — `cargo test -p <id>`.
- **Manual verification via stdin**, bypassing the orchestrator:
  ```powershell
  echo '{"type":"Init","protocol_version":1,"config":null,"storage_dir":"C:/tmp"}' | .\target\debug\<id>.exe
  ```
  expecting a `{"type":"Ready",...}` line on stdout.
- **Real end-to-end testing**, with actual discovery and hosting (`orchestrator/tests/plugin_e2e.rs`
  provides the example for `ping`): compiles the binary, sets up a temporary `plugins/<id>/`, has
  `PluginHost` discover and launch it, verifying `Init → Ready → Deinit`. Excluded from routine test
  runs (requires pre-compiled binary) — invoked explicitly with:
  `cargo test -p orchestrator --test plugin_e2e -- --ignored`.

## Known technical debt and declared limitations

Technical honesty, avoiding overpromising beyond what the code currently delivers:

- **`triggers.interval` is declarable but not executed.** The manifest accepts an interval and
  spawning policy handles it correctly (eager), but no `OnTimer` loop exists yet to trigger it — it
  is a planned protocol message type, not yet implemented.
- **`Init.config` is always `null`.** The channel for passing user configuration to a plugin exists
  in the message, but no production path currently populates it.
- **`Activate.args` is always `null`.** A slash command does not yet pass parameters to the plugin it
  activates — only the invocation signal itself.
- **`Log` reaches the host but is not visible anywhere.** The message is received and discarded: no
  dedicated panel or file exists to display it yet.
- **`storage_dir` is not created by the host.** A plugin wishing to write to it must call
  `create_dir_all` itself — the host guarantees only the path string, not its existence.
- **No real `protocol_version` negotiation.** The field exists in the manifest and messages, ready
  for future use, but currently the only existing version is `1`, and the host does not yet reject a
  plugin declaring a different version.

## Further reading

- [`03-status-and-implementation.md`](./03-status-and-implementation.md) — what part of this system
  is in active use today alongside the rest of the application.
- `crates/plugin-protocol/IMPLEMENTATION.md` and each `crates/plugin-<id>/IMPLEMENTATION.md` —
  current technical details, updated with every change to individual plugins.
