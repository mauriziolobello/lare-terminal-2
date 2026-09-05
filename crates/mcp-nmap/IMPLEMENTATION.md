# Implementation — `mcp-nmap` v0.8.2

## Scope

`mcp-nmap` is a **complete, working MCP sidecar server**, exposing seven
tools over stdio: five structured nmap scan tools sharing one process
pipeline (`nmap_quick_scan` unelevated `-sT`, `nmap_os_detect` elevated
`-O` via Windows UAC, `nmap_version_scan` unelevated `-sV`,
`nmap_host_discovery` unelevated `-sn`, `nmap_vuln_scan` unelevated
`--script vuln`) plus two OS-diagnostic tools (`local_network_info`,
`traceroute`) added after the first live smoke test showed the AI had no
way to discover its own IP/subnet to propose a scan target. The three
version/discovery/vuln scan tools were added in v0.8.0 after a live
`/nmap` session where the user asked the channel's AI what else nmap could
do and picked exactly 3 of the 7 options it listed. See the design spec at
`Docs/superpowers/specs/2026-07-16-mcp-nmap-design.md`, the original
implementation plan at
`Docs/superpowers/plans/2026-07-16-mcp-nmap-sidecar.md`, and the
scan-variants plan at
`Docs/superpowers/plans/2026-07-18-nmap-scan-variants.md`.

Six modules are implemented (current version `0.8.2`):

1. **`src/report.rs`** — pure XML parsing + data model (`ScanReport`,
   `HostReport`, `PortReport`, `ScriptResult`, `ParseError`). Zero
   dependency on MCP/rmcp. Since v0.8.1, also captures NSE script results
   (`<script>`/`<hostscript>`) — see that section below.
2. **`src/markdown.rs`** — `ScanReport` → full Markdown report + brief
   LLM-facing summary + confirm-banner invocation label (now covering all 7
   tool names).
3. **`src/scan.rs`** — process invocation: PATH check, tempfile handling,
   target validation (`validate_target`, `pub(crate)` since v0.7.0 so
   `network_info.rs` can reuse it), and the orchestration
   (`run_quick_scan`/`run_os_detect`/`run_version_scan`/
   `run_host_discovery`/`run_vuln_scan`) tying parsing + formatting together
   into one `ScanOutcome`. Four of those five share one private
   `run_scan_with_flags` pipeline behind the mockable `NmapProcess` trait
   (`run_os_detect` is the one exception — see its own section below). Since
   v0.8.2, `run_vuln_scan` also bounds each NSE script's execution time
   (`--script-timeout 60s`) — see "The 2026-07-18 hang fix" below.
4. **`src/elevate.rs`** — Windows-only `ShellExecuteExW` elevation
   (`run_elevated`), isolated so the rest of the crate stays
   platform-agnostic (no impact on the macOS/Linux gemellaggio requirement —
   elevation there is future, out of scope per design spec §9).
5. **`src/network_info.rs`** (new, v0.7.0) — OS-builtin network diagnostics
   (`local_network_info`, `traceroute`), returning `NetworkInfoOutcome
   { output, is_error }` — see its own section below for why this shape is
   deliberately different from `ScanOutcome`.
6. **`src/main.rs`** — thin MCP stdio glue: `#[tool_router(server_handler)]`
   exposing all 7 tools, `RealNmapProcess` (the real `NmapProcess`
   implementation, spawning `nmap.exe`/`nmap`), and `main()` serving over
   stdio — mirrors `crates/mcp-server/src/main.rs`'s pattern exactly.

Not yet done (deliberately out of this crate's scope): wiring into the
orchestrator (`NmapToolClient`, confirm-gate registration, channel/frontend
plumbing) — see "Not yet implemented" at the end of this document.

---

## `src/report.rs` — pure XML parsing + data model

**Single responsibility:** turn nmap's `-oX` XML text into a plain,
serialization-agnostic data structure. No I/O, no process spawning, no MCP
types — testable entirely with string fixtures.

```rust
pub struct ScriptResult {         // added v0.8.1 — see "NSE script capture" below
    pub id: String,
    pub output: String,
}

pub struct PortReport {
    pub port: u16,
    pub protocol: String,
    pub state: String,          // "open" / "closed" / "filtered", verbatim from nmap
    pub service: Option<String>,
    pub scripts: Vec<ScriptResult>, // added v0.8.1 — <port><script ...> results
}

pub struct HostReport {
    pub address: String,
    pub up: bool,                // <status state="up|down"> — false is NOT an error
    pub ports: Vec<PortReport>,
    pub os_matches: Vec<String>, // <osmatch name="..."> entries, most-accurate first
    pub host_scripts: Vec<ScriptResult>, // added v0.8.1 — <hostscript><script ...> results
}

pub struct ScanReport {
    pub target: String,          // extracted from <nmaprun args="...">, last word
    pub os_detection: bool,      // passed by caller, NOT read from the XML
    pub hosts: Vec<HostReport>,
}

pub enum ParseError {
    MalformedXml(String),        // carries nmap's raw parse-failure reason
}

pub fn parse_nmap_xml(xml: &str, os_detection: bool) -> Result<ScanReport, ParseError>
```

### Parsing approach

Uses `roxmltree::Document` (read-only DOM parser, not serde-derive) because
the `-oX` schema is attribute-heavy and nested
(`<host><status/><address/><ports><port><state/><service/></port></ports><os><osmatch/></os></host>`)
— manual DOM traversal with `.children().find(...)`/`.filter(...)` on tag
name is more direct than fighting serde's XML mapping conventions for this
shape.

Traversal walks `doc.root_element()` (the `<nmaprun>` element) for `args` (to
derive `target`) and iterates its `<host>` children. For each host:
- `up` — `<status state="...">`'s `state` attribute equals `"up"`.
- `address` — `<address addr="...">`'s `addr` attribute.
- `ports` — iterates `<ports><port>` children; each port's `portid` attribute
  parses to `u16` (defaults to `0` on parse failure — nmap always emits a
  valid number here in practice, so this is a defensive fallback, not an
  expected path); `state` comes from the nested `<state state="...">`;
  `service` from the optional nested `<service name="...">`.
- `os_matches` — iterates `<os><osmatch>` children (absent entirely for
  non-`-O` scans, or present-but-filterable-to-empty when nmap made no
  confident OS match), collecting `name` attributes in document order (nmap
  emits its own matches most-accurate-first, so no re-sorting is needed).
- `ports[].scripts` / `host_scripts` (added v0.8.1) — see "NSE script
  capture" below.

### NSE script capture (`ScriptResult`, added v0.8.1)

**The bug this fixes:** `nmap_vuln_scan`'s report window was byte-identical
to `nmap_quick_scan`'s for the same target — nmap's `--script vuln` results
live in `<script>` (per-port, nested inside `<port>`) and `<hostscript>`
(host-level, e.g. SMB checks not tied to one port) XML elements, and this
parser had no code path that read either at all before this fix.

`fn parse_scripts(parent: roxmltree::Node) -> Vec<ScriptResult>` is a
private helper, shared between both call sites: for each direct `<script>`
child of `parent`, it reads `id` and `output` attributes (missing → empty
string, matching this parser's existing style for `protocol`/`state`) into
a `ScriptResult`. Called once per `<port>` (building `PortReport::scripts`)
and once per host's `<hostscript>` element if present (building
`HostReport::host_scripts`, defaulting to `vec![]` when no `<hostscript>`
element exists at all — most scans don't run scripts).

**Only the `output` attribute is captured, deliberately — not the text
child.** Real nmap XML shape (captured live against a home router):

```xml
<hostscript>
  <script id="smb-vuln-ms10-054" output="false">false</script>
  <script id="samba-vuln-cve-2012-1182" output="Could not negotiate a connection:SMB: Failed to receive bytes: EOF">false</script>
</hostscript>
```

Some host-level `<script>` elements carry BOTH an `output` attribute AND a
text child, and the two are not always the same string — `output` is
nmap's own flattened, canonical result text; the text child is not. This
parser reads only `output`. Nested `<table>`/`<elem>` structured children
some NSE scripts also emit are out of scope (a correct first fix does not
need them) — a deliberate scope fence, not an oversight.

### Why `up: false` is not treated as a parse error

Design spec §7 explicitly calls out "zero host up" as a legitimate scan
result (the target simply didn't respond), distinct from a parse failure
(malformed/unexpected XML shape) or an nmap process failure (Task 3's
concern). `parse_nmap_xml` returns `Ok(ScanReport { hosts: [HostReport { up:
false, ports: vec![], .. }], .. })` for a down host — never an `Err`. Callers
in later tasks (`markdown.rs`, the eventual MCP tool response) must branch on
`host.up`, not on `Result::is_err()`, to decide how to report "no response."

### `ParseError` — diagnostics, not just a boolean

`ParseError::MalformedXml(String)` carries `roxmltree`'s own parse failure
message (`Document::parse(xml).map_err(|e| e.to_string())`), not a generic
"parse failed" string. This lets the future MCP tool response distinguish
"nmap itself crashed / produced no XML" (Task 3's concern — a missing or
empty file) from "nmap produced XML but this parser doesn't understand its
shape" (a bug in `report.rs` — should be rare, since nmap's `-oX` schema is
stable, but worth being able to tell apart per design spec §7).

### Fixtures (`src/fixtures/*.xml`)

Three hand-written, realistic `-oX` fragments, loaded via
`include_str!("fixtures/...")` in `report.rs`'s test module (path is relative
to `report.rs`, hence fixtures live under `src/fixtures/`, not the crate
root):

- `host_up_open_port.xml` — one host up, one open TCP port with a `service`,
  one closed port with no `service` element at all (proves `Option::None`
  path, not an empty string).
- `host_down.xml` — host down, **no `<ports>` element present** (proves the
  parser doesn't assume `<ports>` always exists).
- `os_detect_with_matches.xml` — `-O` scan, one `filtered` port, two
  `<osmatch>` entries (proves ordering is preserved, not re-sorted).
- `host_with_vuln_scripts.xml` (added v0.8.1) — one open port with two
  per-port `<script>` results, one open port with none (proves an empty
  `scripts` vec, not a missing field), and a `<hostscript>` block with two
  host-level results, one of which has both an `output` attribute AND a
  differing text child (proves the parser reads `output`, not the text
  child — see "NSE script capture" above). **Provenance matters here more
  than usual**: this crate was already burned once by a hand-guessed
  fixture that silently diverged from real nmap output
  (`real_nmap_output_with_dtd.xml`, v0.6.0 — three Task-1 fixtures without
  a DOCTYPE line meant `parse_nmap_xml` rejected every real scan on every
  machine, undetected until a live smoke test). This fixture's shape is
  lifted directly from a real `nmap -sT --script vuln` capture against a
  live router, not invented — do not "clean up" its attribute names or
  nesting without capturing real nmap output again first.

---

## Crate scaffold

### `Cargo.toml`

- `[[bin]] name = "mcp-nmap" path = "src/main.rs"` — same pattern as
  `mcp-server`: the crate has both a `lib` target (pure core, unit-tested in
  isolation) and a `bin` target (thin MCP glue).
- `roxmltree = "0.21"` — the only dependency actually used by any code in
  this task.
- The remaining dependencies (`tempfile`, `rmcp = "=1.7.0"`, `tokio`, `serde`/
  `serde_json`, `anyhow`, `tracing`/`tracing-subscriber`, and the
  `cfg(windows)`-gated `windows = "0.62"`) are declared now per the design
  spec's anticipated Task 2-5 needs, but are **not yet imported or used by
  any code** — `cargo build`/`cargo test` pull them in as unused-but-declared
  dependencies (no dead-code warnings result, since nothing references them).

### `src/lib.rs`

Exposes `pub mod report;` only. The module-level doc comment documents the
full five-module plan (see above) so a reader opening this file mid-plan
understands where the crate is headed, not just its current state.

### `src/main.rs`

Placeholder binary — prints an error to stderr and exits `1`. Exists solely
so the `[[bin]]` target in `Cargo.toml` resolves during `cargo build`/`cargo
test` for Tasks 1-4; real MCP stdio glue (mirroring `mcp-server/src/main.rs`)
lands in Task 5.

### Workspace registration

`Cargo.toml` (workspace root): `"crates/mcp-nmap"` added to both `members`
and `default-members`, positioned right after `"crates/mcp-server"` — so
`cargo build`/`cargo test` without `-p` (which the whole backend workflow
relies on) now include this crate by default, same as `mcp-server` and
`orchestrator`.

---

## Verification (this task)

- `cargo test -p mcp-nmap` → `test result: ok. 5 passed; 0 failed`, no
  warnings.
- `cargo build` (whole workspace) → clean, confirms the workspace
  registration doesn't break any other default-member crate.
- `cargo clippy -p mcp-nmap --all-targets` → no warnings.
- `cargo fmt -p mcp-nmap --check` → clean (one `cargo fmt -p mcp-nmap` run
  applied to reformat a handful of long lines from the design brief into
  rustfmt's canonical multi-line style; purely cosmetic, tests re-verified
  green afterward).

---

## `src/markdown.rs` — Markdown formatting + invocation labels (Task 2)

**Single responsibility:** turn `ScanReport` into human-readable Markdown
(full report for the Markdown window / Library save) and brief text for the
LLM's `tool_result` (same context-awareness philosophy as the orchestrator's
`agent::truncate_for_model`, but achieved here by keeping the function's scope
narrow — no orchestrator dependency, no truncation logic needed).

### Three functions, all pure (no I/O, no process spawning)

```rust
pub fn format_report_markdown(report: &ScanReport) -> String
// → "# Scansione nmap — {target}\n\n## Host {address}\n..." per-host sections,
//   ports with services, OS matches if os_detection=true.
//   **Deterministic** (byte-for-byte consistent) so snapshot tests are meaningful.
//   **Never sent to the LLM** — reserved for the Markdown window/Library.

pub fn format_summary(report: &ScanReport) -> String
// → "nmap: {target} — {up_count} host attivo/i, {open_count} porta/e aperta/e."
//   Omits per-port service detail (that's in the full report).
//   **For the LLM's tool_result** — keeps the model's context small per call.

pub fn format_nmap_invocation(tool_name: &str, args: &serde_json::Value) -> String
// → "nmap quick scan → {target}" or
//   "nmap OS detect (richiede privilegi elevati) → {target}" or
//   "[tool {unknown}]"
// **For the confirm banner label** (design spec §5).
// nmap_os_detect explicitly mentions elevation because Windows' UAC dialog
// never shows the target to the user — this is the only place they see it.
```

### Markdown structure

**Full report:**
- Title with scan target.
- Per-host section (one `## Host` block per host):
  - Status (up/down).
  - Ports list (only if host is up): `- {port}/{proto} — {state} ({service})`.
  - OS section (only if `os_detection=true`): matches in nmap's order
    (most-accurate first).
- Empty hosts case: "Nessun host nel risultato."
- Down hosts: no ports section (design spec §7 — a down host is a legitimate
  result, not an error, so we don't list ports for a host that didn't respond).

**Brief summary (for the LLM):**
- Host count (active only, ignoring `up: false` hosts).
- Open port count (across all active hosts, not per-host).
- No service names (kept out of the LLM summary intentionally).
- "nessun host attivo trovato." if all hosts are down.

### Test coverage

9 deterministic tests (all passing), written RED-first:

| Test | Assertion |
|------|-----------|
| `markdown_report_includes_target_host_and_ports` | full report has target, host IP, port numbers, protocols, states, service names |
| `markdown_report_includes_os_matches_when_os_detection_true` | OS section present only when `os_detection=true` and includes matches |
| `markdown_report_down_host_has_no_ports_section` | down host doesn't show ports list (a down host is a legitimate result, not an error) |
| `markdown_report_empty_hosts_says_so` | empty `hosts` vector → "Nessun host nel risultato." (not treated as an error) |
| `summary_counts_up_hosts_and_open_ports` | summary counts active hosts only + counts "open" ports |
| `summary_zero_hosts_says_none_active_not_an_error` | all hosts down → "nessun host attivo trovato." (distinct from parse/process error) |
| `invocation_label_quick_scan` | `nmap_quick_scan` → "nmap quick scan → {target}" |
| `invocation_label_os_detect_mentions_elevation` | `nmap_os_detect` → mentions elevation requirement + target |
| `invocation_label_unknown_tool_falls_back` | unknown tool name → "[tool {name}]" (graceful fallback) |

### Dependencies

- Uses `crate::report::ScanReport` (Task 1's data model).
- Requires `serde_json::Value` for `format_nmap_invocation` args (already
  declared in `Cargo.toml` for later tasks).
- No I/O, no `tokio`, no `rmcp` — pure string formatting.

---

## `src/scan.rs` — unelevated `nmap_quick_scan` invocation (Task 3)

**Single responsibility:** orchestrate one unelevated quick scan — check
`nmap` availability, reserve a tempfile for `-oX` output, invoke `nmap`
through a mockable seam, parse the result, and format it into the shape the
MCP tool response (Task 5) and the orchestrator's `NmapToolClient` (companion
integration plan) both expect. No MCP dependency; only `report`/`markdown`
(this crate) plus `tempfile`/`std::process`.

```rust
pub struct ScanOutcome {
    pub summary: String,          // for the LLM tool_result
    pub report_markdown: String,  // for the Markdown window/Library — never sent to the LLM
    pub is_error: bool,
}

pub trait NmapProcess {
    fn run(&self, args: &[&str], xml_path: &Path) -> Result<ExitStatus, String>;
}

pub fn nmap_on_path() -> bool
pub fn run_quick_scan(target: &str, process: &dyn NmapProcess, nmap_available: bool) -> ScanOutcome
```

### `NmapProcess` — the seam that keeps this testable

`run_quick_scan` never touches `std::process::Command` directly; it calls
`process.run(&args, &temp_path)` through `&dyn NmapProcess`. Tests implement
the trait with a `FakeNmapProcess` that writes canned XML to `xml_path` and
returns a canned `ExitStatus` — no real `nmap` process is spawned by any test
in this crate. The real implementation (`RealNmapProcess`, spawning
`std::process::Command::new("nmap")`) is added in Task 5's `main.rs`; Task
4's elevated `-O` path implements the same trait via
`elevate::run_elevated`, so `run_quick_scan`'s call site does not need to
change when the elevated variant lands.

Synthesizing a real `ExitStatus` in tests: `std::process::ExitStatus` has no
public constructor in stable `std`, so `FakeNmapProcess::run` spawns a
trivial real subprocess (`cmd /C exit 0`/`1` on Windows, `true`/`false` on
Unix) purely to obtain a genuine `ExitStatus` value — this is a test-only
detail, `run_quick_scan` itself never spawns anything beyond what `process`
does.

### `nmap_on_path()` — explicit PATH check, not a caught spawn error

Scans every directory in the `PATH` env var for `nmap.exe` (Windows) /
`nmap` (Unix) via `std::env::split_paths(...).any(|dir| dir.join(exe).is_file())`.
Exists so the "nmap not installed" case produces a specific, readable error
message instead of relying on the OS's process-spawn failure (design spec
§7 — the raw OS error is less legible to the user).

**Not called by `run_quick_scan` itself.** `run_quick_scan` takes
`nmap_available: bool` as an explicit parameter instead of calling
`nmap_on_path()` internally — this is deliberate, not an oversight: it keeps
`run_quick_scan`'s unit tests fully deterministic regardless of whether the
machine actually running the test suite happens to have `nmap` installed.
Every test in this crate passes a literal `true`/`false`. `nmap_on_path()`'s
real PATH scan is only ever invoked from `main.rs` (Task 5), which passes its
result straight into `run_quick_scan`/`run_os_detect`'s `nmap_available`
argument.

### `run_quick_scan` — flow

1. `nmap_available == false` → short-circuits immediately to
   `ScanOutcome::error(...)`, **before** `process.run(...)` is ever called
   (proven by a test whose fake process panics if invoked).
2. Reserves an output path via `tempfile::NamedTempFile::new()?.into_temp_path()`
   — `into_temp_path()` closes the Rust-side file handle before nmap writes
   to it, avoiding a file-lock conflict on Windows; the returned `TempPath`
   still deletes the file on drop once we're done reading it.
3. Builds `args = ["-sT", "-oX", <path>, target]` — `-sT` (TCP connect scan)
   is pinned explicitly, never left to nmap's privilege-based default
   (design spec §3: an unelevated `-sS` would silently fall back and produce
   different results without telling the caller).
4. Calls `process.run(&args, &temp_path)`. A `Result::Err` (couldn't spawn)
   or a non-success `ExitStatus` both become a readable `ScanOutcome::error`
   (the latter includes the raw exit code for diagnostics).
5. Reads the XML file back with `std::fs::read_to_string`, then
   `parse_nmap_xml(&xml, false)` (Task 1) — `false` because `nmap_quick_scan`
   never uses `-O`. Parse failure also becomes a readable error (reusing
   `ParseError`'s `Display`, which already says "XML nmap malformato").
6. On success, builds `ScanOutcome` from `format_summary` +
   `format_report_markdown` (Task 2) over the same parsed `ScanReport` — both
   fields come from one XML read/parse, not two.
7. The `TempPath` drops at the end of the function scope, deleting the XML
   file only after it has been fully read — no manual cleanup needed.

### `ScanOutcome::error` — error outcomes carry no report

The private `ScanOutcome::error(message)` helper always sets
`report_markdown` to an empty string and `is_error` to `true`. Design spec
§7: a tool error (PATH missing, spawn failure, malformed XML, non-zero exit)
is not a scan report — there is nothing meaningful to show in a Markdown
window for "nmap non trovato sul PATH", so the field is left empty rather
than populated with placeholder text.

### Test coverage

4 deterministic tests (all passing), written RED-first — the whole module
initially only contained the test block referencing `NmapProcess`/
`ScanOutcome`/`run_quick_scan`, which didn't exist yet (confirmed RED: 6
compile errors, all `E0405`/`E0425` naming the missing symbols, not a typo).

| Test | Assertion |
|------|-----------|
| `quick_scan_success_produces_summary_and_report` | fake process reports success + writes valid XML → `is_error: false`, summary mentions the target, report Markdown mentions the open port |
| `quick_scan_nonzero_exit_is_a_readable_error` | fake process reports a non-zero exit → `is_error: true`, summary mentions "exit code", `report_markdown` is empty |
| `quick_scan_malformed_xml_is_a_readable_error_not_a_panic` | fake process writes invalid XML with a success exit → `is_error: true`, summary reuses `ParseError`'s "XML nmap malformato" message, no panic |
| `quick_scan_nmap_not_available_is_a_readable_error_before_any_process_call` | `nmap_available: false` with a fake process that panics if `run()` is ever called → still returns a readable error, proving the short-circuit happens before any process interaction |

### Dependencies

- Uses `crate::report::parse_nmap_xml` (Task 1) and
  `crate::markdown::{format_summary, format_report_markdown}` (Task 2).
- `tempfile` (already a normal, non-dev dependency in `Cargo.toml` since Task
  1 — used here for the first time).
- `serde`/`serde_json` derive `Serialize`/`Deserialize` on `ScanOutcome`
  (already declared in `Cargo.toml`) — this is the shape Task 5's MCP tool
  response serializes and the orchestrator's `NmapToolClient` (companion
  integration plan) deserializes.
- No `rmcp`, no `tokio`, no direct `std::process::Command` call in this
  module — that lives in the real `NmapProcess` implementation, added in
  Task 5's `main.rs`.

---

## `src/elevate.rs` — Windows elevation for `nmap_os_detect` (Task 4)

**Single responsibility:** wrap `ShellExecuteExW`'s UAC "runas" elevation
behind a safe function, isolated in its own module so the rest of the crate
(`report`, `markdown`, `scan`'s `run_quick_scan`) stays platform-agnostic and
testable without any Windows-specific mocking — no other module has an
`unsafe` block or a `#[cfg(windows)]` gate anywhere in this crate.

```rust
pub enum ElevationError { UserCancelled, Timeout, Other(String) }
impl std::fmt::Display for ElevationError { .. }

pub const SCAN_TIMEOUT: Duration = Duration::from_secs(600);

fn map_shell_execute_error(raw_code: i32) -> ElevationError

#[cfg(windows)]
pub fn run_elevated(exe: &str, args: &[&str], timeout: Duration) -> Result<i32, ElevationError>

#[cfg(not(windows))]
pub fn run_elevated(_exe: &str, _args: &[&str], _timeout: Duration) -> Result<i32, ElevationError>
```

### Why this is NOT unit-tested end-to-end

Real elevation requires an interactive UAC prompt — there is no way to fake
`ShellExecuteExW` returning a genuine elevated process handle without
actually triggering Windows' consent UI. Only `map_shell_execute_error`
(a pure `i32 -> ElevationError` mapping) and `ElevationError`'s `Display`
impl are unit-tested. A manual end-to-end test (real scan, real UAC prompt)
is required before merge, per this crate's companion integration plan.

### `run_elevated` (Windows) — flow

1. Builds three `HSTRING`s (`verb = "runas"`, `file = exe`, `params =
   args.join(" ")`) and a `SHELLEXECUTEINFOW` referencing them via
   `PCWSTR::from_raw(h.as_ptr())` — see the HSTRING/PCWSTR verification
   section below for why this construction is correct.
2. `fMask = SEE_MASK_NOCLOSEPROCESS` — tells Windows to hand back a process
   handle (`info.hProcess`) instead of closing it immediately after
   `ShellExecuteExW` returns, so the handle can be waited on afterward.
3. `nShow = 0` (`SW_HIDE`) — the elevated `nmap.exe` gets no visible console
   window.
4. `ShellExecuteExW(&mut info)` — a Windows `Err` here (including
   `ERROR_CANCELLED` from a declined UAC prompt) is mapped through
   `map_shell_execute_error(e.code().0)`.
5. `WaitForSingleObject(hProcess, timeout.as_millis())` — `WAIT_TIMEOUT` →
   `TerminateProcess(hProcess, 1)` (force-kill the elevated child if it's
   still alive — see the post-review fix below), then `CloseHandle`, then
   `ElevationError::Timeout`; any other non-`WAIT_OBJECT_0` result →
   `ElevationError::Other` (after `CloseHandle`, no `TerminateProcess` — the
   process already signalled in that branch).
6. `GetExitCodeProcess(hProcess, &mut exit_code)` → the child's real exit
   code on success. `CloseHandle` runs on every exit path from step 5
   onward — no leaked handles regardless of which branch returns. On the
   `WAIT_TIMEOUT` path specifically, the elevated process itself is also
   force-terminated (not just handle-closed) — see "Elevated process leak
   on timeout" below.
7. Every raw Win32 call (`ShellExecuteExW`, `WaitForSingleObject`,
   `GetExitCodeProcess`, `TerminateProcess`, `CloseHandle`) is wrapped in its
   own `unsafe` block with a `// SAFETY:` comment explaining why the call is
   sound at that point (valid, fully-initialised struct; handle still
   valid/not yet closed; process has signalled). The public `run_elevated`
   function itself has no `unsafe` in its signature — callers never touch
   the raw
   APIs.

### `HSTRING`/`PCWSTR` verification (Task 4 Step 1)

The task brief flagged one FFI detail as NOT independently verified during
planning (unlike `ShellExecuteExW`'s signature, `SHELLEXECUTEINFOW`'s
fields, `WaitForSingleObject`, `GetExitCodeProcess`, and
`SEE_MASK_NOCLOSEPROCESS`'s value, all confirmed against
microsoft.github.io/windows-docs-rs during planning): how to construct a
`PCWSTR` from a Rust `&str`.

Verified by running `cargo doc -p mcp-nmap` (full doc generation — *not*
`--no-deps`, so the `windows`/`windows-strings` 0.62.2 dependencies'
documentation was generated into `target/doc/` alongside this crate's own)
and reading the actually-generated source HTML
(`target/doc/src/windows_strings/hstring.rs.html`,
`target/doc/src/windows_strings/pcwstr.rs.html`) rather than relying on
memory of the crate's API:

- `impl From<&str> for HSTRING` — real, exact match to the brief
  (`hstring.rs:129-133`): `fn from(value: &str) -> Self { unsafe {
  Self::from_wide_iter(value.encode_utf16(), value.len()) } }`.
- `PCWSTR::from_raw(ptr: *const u16) -> Self` — real, exact match
  (`pcwstr.rs:10-12`), a `pub const fn`.
- **Nuance:** `HSTRING` has no *inherent* `as_ptr` method. It implements
  `Deref<Target = [u16]>` (`hstring.rs:65-78`), so `verb.as_ptr()` resolves
  via Rust's normal method-call auto-deref to the slice's own
  `as_ptr(&self) -> *const u16`. This is confirmed to be the intended usage,
  not an accidental side effect: the crate's own `Deref` impl keeps the
  empty-string case pointing at a static null-terminated `[0u16; 1]`
  specifically "so that if `as_ptr` is called on the slice that the
  resulting pointer will still refer to a null-terminated string"
  (`hstring.rs:72-75`).

**Conclusion: the brief's exact code pattern (`HSTRING::from(s)` +
`PCWSTR::from_raw(h.as_ptr())`) is correct and was used as-is** — no
deviation was needed for this specific API detail. (Two *other*, unrelated
compile errors were found and fixed — see "Real compile errors" below —
neither is the HSTRING/PCWSTR pattern.)

### Real compile errors found by `cargo build -p mcp-nmap` (unrelated to HSTRING/PCWSTR)

The brief's Step 3 anticipated `cargo build -p mcp-nmap` as the place a
HSTRING/PCWSTR mismatch would surface — none did, but two *other* real
compile errors did, both `windows`-crate feature/module-path issues not
covered by either "verified" or "unverified" list in the brief:

1. **`SHELLEXECUTEINFOW`/`ShellExecuteExW` gated behind
   `Win32_System_Registry`.** `SHELLEXECUTEINFOW` carries an `hkeyClass:
   HKEY` field (unused by this code — always left at its `Default`-derived
   zero value via `..Default::default()`), and windows-rs 0.62.2 gates the
   entire struct (and the function that takes it) behind that feature.
   Fixed by adding `"Win32_System_Registry"` to `Cargo.toml`'s
   `cfg(windows)` `windows` dependency features.
2. **`WAIT_OBJECT_0`/`WAIT_TIMEOUT` live in `Win32::Foundation`, not
   `Win32::System::Threading`.** These are `WAIT_EVENT` constants (the
   return type of `WaitForSingleObject`), defined alongside `WAIT_EVENT`
   itself in `Foundation`, not the `Threading` module the brief's `use`
   statement imported them from. Fixed by moving the import.

### `build_parameters` — argument quoting (post-implementation review fix)

Found by an `advisor()` review call after the initial implementation went
GREEN, not by any automated check or the brief itself:

**The bug:** `run_elevated`'s original code (matching the design brief's
literal text) built `lpParameters` via `HSTRING::from(args.join(" "))`.
`ShellExecuteExW` passes `lpParameters` to the launched executable as one
command-line string; nmap's own C runtime re-splits it on whitespace
(roughly `CommandLineToArgvW` rules). This crate's `-oX` output path comes
from `tempfile::NamedTempFile`, which lives under `std::env::temp_dir()` —
`C:\Users\<username>\AppData\Local\Temp\...`. On any machine whose Windows
username contains a space (`C:\Users\John Smith\...`), the naive join
silently splits that path into two argv entries once nmap re-parses
`lpParameters`, truncating the `-oX` path — `nmap_os_detect` would then
deterministically fail (later, at `read_to_string`, with "impossibile
leggere l'output XML di nmap") on every such machine, for every target, not
just an edge case. No test exercised this — the crate's one `run_os_detect`
test only covers the `nmap_available == false` guard — and this project's
own dev machine (`C:\Users\Maurizio`) has no space in its username, so even
the planned manual end-to-end test would not have caught it.

A related concern shares the same root cause: `target` is supplied by the
tool caller (ultimately the AI, via the confirm-gated tool call) and was
being concatenated into the command line with no escaping at all — a
target containing a literal `"` could unbalance the whole `lpParameters`
quoting and smuggle extra argv entries past their intended boundary
(argument injection), independent of the whitespace bug.

**The fix:**

```rust
fn build_parameters(args: &[&str]) -> Result<String, ElevationError>
```

Pure, platform-agnostic (no `cfg(windows)`, no FFI) — defined and tested
outside `windows_impl`, alongside `map_shell_execute_error`. For each
argument:
- If it contains a literal `"`, returns `Err(ElevationError::Other(...))`
  immediately — rejected outright rather than escaped. Escaping quotes
  correctly requires exactly matching the target executable's own
  unescaping algorithm (Windows' documented argv-quoting rules are more
  subtle than a simple backslash-escape, particularly around backslashes
  immediately preceding a quote); getting that subtly wrong here would
  trade one injection bug for a different one. None of this crate's own
  arguments (fixed flags, a tempfile path, a scan target) has any
  legitimate reason to contain a quote character, so rejecting is both
  simpler and safer than attempting to escape.
- Else if it contains any whitespace character, wraps it in `"..."` so it
  survives as a single argv entry after nmap's own re-split.
- Otherwise, passes it through unchanged.

`run_elevated` now calls `build_parameters(args)?` (the `?` relies on
`ElevationError`'s blanket `From<ElevationError> for ElevationError`) in
place of the original `args.join(" ")`, before building any `HSTRING`.

3 tests, RED-first (`cargo test -p mcp-nmap` confirmed `E0425: cannot find
function build_parameters` for all three before the function existed):
- `build_parameters_quotes_an_argument_containing_whitespace` — a
  spaced-path argument (simulating the real bug) gets wrapped in quotes;
  the other, whitespace-free arguments in the same call are left alone.
- `build_parameters_leaves_whitespace_free_arguments_unquoted` — confirms
  no spurious quoting when every argument is already safe.
- `build_parameters_rejects_an_argument_containing_a_quote_character` — a
  target-shaped string containing `"` returns `Err`, proving it never
  reaches `ShellExecuteExW`.

### Elevated process leak on timeout (post-review fix, v0.6.0)

Found by the final whole-branch review of the completed 5-task plan, not by
any automated check: the `WAIT_TIMEOUT` branch of `run_elevated` called only
`CloseHandle(hprocess)` before returning `ElevationError::Timeout`.
`CloseHandle` releases *this process's own reference* to the handle — it
does **not** signal or stop the process it refers to. The design spec §4
requires "processo elevato terminato se ancora vivo" (elevated process
terminated if still alive) on timeout; without a real termination call, an
elevated `nmap.exe` that hangs past `SCAN_TIMEOUT` (600s) keeps running
indefinitely, and — since it may still hold the `-oX` tempfile open — can
also cause `run_os_detect`'s `TempPath` cleanup (`scan.rs`) to silently fail
to delete that file, compounding a process leak with a tempfile leak.

**The fix:** `TerminateProcess(hprocess, 1)` is called immediately before
`CloseHandle` in the `WAIT_TIMEOUT` branch only (the other two exit paths —
success and the "unexpected wait result" branch — don't need it: the process
has already signalled by the time either of those runs, so there's nothing
left to terminate). `mcp-nmap` itself runs at medium integrity (never
elevated) while the target process runs at high integrity (elevated via
UAC); a medium-integrity process can normally terminate its own child even
when that child is elevated, since the child's token descends from the
`ShellExecuteExW` call this process itself made. If `TerminateProcess` still
fails for some other reason (e.g. access denied), the error is deliberately
ignored — the function already returns `ElevationError::Timeout` regardless,
and a failure to force-terminate must not be allowed to mask the timeout
with a different, less informative error.

**Not unit-tested**, same category as the rest of `run_elevated`'s Windows
path (see "Why this is NOT unit-tested end-to-end" above): exercising this
branch for real requires an actual hung elevated process, which no
automated test in this crate can safely trigger. Verified instead by
`cargo build -p mcp-nmap` compiling cleanly with the new FFI call
type-checked, plus `cargo test -p mcp-nmap` (same 38 tests, none added) and
`cargo clippy -p mcp-nmap --all-targets` (clean).

### `src/scan.rs` — `run_os_detect`

```rust
pub fn run_os_detect(target: &str, nmap_available: bool) -> ScanOutcome
```

Same flow as `run_quick_scan` (Task 3) — PATH check → tempfile for `-oX` →
invoke → read → parse → format — except:
- Pins `-O` instead of `-sT`.
- Calls `crate::elevate::run_elevated(exe, &args, crate::elevate::SCAN_TIMEOUT)`
  **directly**, not through the `NmapProcess` trait. This is deliberate:
  elevation cannot be meaningfully faked the way `run_quick_scan`'s
  `FakeNmapProcess` fakes a plain process spawn — a fake `run_elevated`
  would either always claim success (testing nothing about elevation) or
  need to simulate a UAC prompt (which doesn't exist to simulate). Routing
  it through `NmapProcess` anyway would misleadingly suggest it's tested the
  same way `run_quick_scan` is.
- `parse_nmap_xml(&xml, true)` — `true` because `nmap_os_detect` always
  requests `-O`.

**What IS tested:** the `nmap_available == false` short-circuit — this
branch is fully deterministic (no elevation, no process spawn) and returns
before `run_elevated` could ever be called, exactly mirroring
`run_quick_scan`'s equivalent guard test.
(`os_detect_nmap_not_available_is_a_readable_error_before_elevation`).

**What is NOT unit-tested:** everything past that guard — invoking a real
`run_elevated` would trigger an actual UAC prompt. Covered by a manual
end-to-end test before merge (companion integration plan's final task), not
by this crate's automated suite.

### Corrected stale comments from Task 3

Task 3's `scan.rs` module doc and `NmapProcess` trait doc both said
`os_detect`'s elevated path would "implement the same trait via
`elevate::run_elevated`" — written before this task's actual design (direct
call, not through the trait) was finalized. Both doc comments, plus
`lib.rs`'s module-list entry for `scan`, are corrected in this task to
describe what `run_os_detect` actually does.

## `src/scan.rs` — `validate_target` (security fix, v0.5.0)

**Single responsibility:** reject any scan target whose *content* would let
nmap's own argument parser (or, on the elevated `run_os_detect` path,
`ShellExecuteExW`'s consuming process) treat it as something other than a
single positional target. Fixes a Critical security-review finding: neither
`run_quick_scan` nor `run_os_detect` validated `target`'s content at all
before this fix — a target starting with `-` (e.g. `"--script=vuln"`) is
read by nmap as a FLAG, not a positional target, completely independent of
`elevate::build_parameters`'s quoting fix (0.4.0), which only controls how
Windows tokenizes `lpParameters` into argv — it says nothing about what
nmap does with a single clean argv token once received.

```rust
fn validate_target(target: &str) -> Result<(), String>
```

Called as the **very first statement** in both `run_quick_scan` and
`run_os_detect`, before the `nmap_available` check, before any tempfile is
created, before any process is invoked.

### Rules (in check order)

1. Empty target → rejected.
2. Target starting with `-` → rejected. This is the actual exploit vector.
3. Target containing any whitespace → rejected. This tool takes exactly one
   target per call; nmap's own space-separated multi-target syntax is out of
   scope (same "one target per call" principle already established
   elsewhere in this crate).
4. Defense in depth: any character outside `[A-Za-z0-9] . : / * - _` →
   rejected.

Deliberately **permissive** of legitimate nmap target syntax — IPv4, IPv4
CIDR (`192.168.1.0/24`), IPv4 range (`192.168.1.1-254`), octet wildcard
(`192.168.1.*`), hostname (including hyphenated ones, e.g.
`my-host.example.com`), IPv6 — all of which can use `.`/`:`/`/`/`-`/`_`/`*`
anywhere except as the first character. Not a full nmap target-grammar
parser; the two checks that matter most for the actual exploit (leading `-`,
embedded whitespace) plus a whitelist as a backstop.

On failure, returns `ScanOutcome::error(message)` — same helper, same
error-shape convention as every other failure mode in this file (PATH
missing, spawn failure, malformed XML): `report_markdown` empty, `is_error:
true`, a readable Italian message.

### Test coverage

9 unit tests for `validate_target` in isolation (normal IP, hyphenated
hostname, CIDR, IPv4 range, octet wildcard all `Ok`; empty string, leading
`-`, embedded whitespace, and disallowed characters `;`/`` ` ``/`$` all
`Err`), plus 2 integration tests proving the gate actually blocks the two
call sites before they do anything dangerous:

- `quick_scan_invalid_target_is_a_readable_error_before_any_process_call` —
  mirrors the existing `quick_scan_nmap_not_available_...` test's pattern: a
  `NmapProcess` fake that panics if `run()` is ever called proves
  `"--script=vuln"` never reaches process invocation.
- `os_detect_invalid_target_is_a_readable_error` — `run_os_detect` has no
  injectable process (it calls `elevate::run_elevated` directly — see the
  module doc), so this cannot prove "elevation was never attempted" the way
  the quick-scan test proves "process.run was never called". It proves the
  narrower but still load-bearing claim: the validation gate itself rejects
  `"--script=vuln"` with a readable error. Deliberately passes
  `nmap_available: true` (not `false`) so this test cannot be satisfied by
  the unrelated PATH-guard short-circuit — a regression that removed the
  `validate_target` call would make this test fail, not pass vacuously.

### TDD deviation, explicitly flagged (SOLID/debt note)

Every other test in this fix followed strict RED-then-GREEN with an
observed failing run. `os_detect_invalid_target_is_a_readable_error` is the
one exception: observing a genuine RED for it would have meant running
`cargo test` against a `run_os_detect` that had `validate_target` wired into
`run_quick_scan` but **not yet** into `run_os_detect`, with
`nmap_available: true` and a malicious-looking target — which would have
called `elevate::run_elevated` for real, triggering an actual
`ShellExecuteExW(lpVerb="runas")` UAC round-trip on the real Windows dev
machine running the test suite. This crate's own existing test
(`os_detect_nmap_not_available_is_a_readable_error_before_elevation`)
already documents avoiding exactly this class of risk as the reason it uses
`nmap_available: false` instead of exercising the elevated path for real.
Given that established precedent, `validate_target` was wired into
`run_os_detect` in the same edit as `run_quick_scan` — before the
integration test for `run_os_detect` was written or run — so `cargo test`
was never executed against an unsafe intermediate state. The test itself is
still real and deterministic (no mocked-away result); only the *order* of
"write test, observe it fail" was skipped for this one case, for a safety
reason, not a convenience one.

## `src/main.rs` — MCP stdio glue (Task 5)

**Single responsibility:** wire `scan`/`report`/`markdown`'s pure core to
the MCP protocol over stdio — no business logic of its own. Mirrors
`crates/mcp-server/src/main.rs`'s structure exactly (parameter DTOs,
`#[tool_router(server_handler)]`, `serve(stdio())`, tracing to stderr).

```rust
struct RealNmapProcess; // impl NmapProcess — spawns std::process::Command::new("nmap"/"nmap.exe")

#[derive(Deserialize, schemars::JsonSchema)] struct NmapQuickScanParams { target: String }
#[derive(Deserialize, schemars::JsonSchema)] struct NmapOsDetectParams { target: String }

#[derive(Clone)] struct NmapServer;

#[tool_router(server_handler)]
impl NmapServer {
    async fn nmap_quick_scan(&self, Parameters(NmapQuickScanParams { target }): ...) -> String
    async fn nmap_os_detect(&self, Parameters(NmapOsDetectParams { target }): ...) -> String
}
```

Both tool methods call straight into `scan::run_quick_scan`/`run_os_detect`,
passing `scan::nmap_on_path()` as the `nmap_available` argument — this is
the crate's first and only real caller of `nmap_on_path()`, matching the
design documented in `scan.rs` since Task 3. The `ScanOutcome` result is
`serde_json::to_string`'d into a bare `String` — success and error are both
encoded as JSON *data* (`is_error: bool`), never surfaced as an
MCP-protocol-level tool error, same convention as `mcp-server`'s tools.

**Signature check before writing this file:** the task brief was written
before Task 4's `validate_target` security fix landed, which added a third
parameter (`nmap_available: bool`) to both `run_quick_scan`/`run_os_detect`.
Before writing any call site, `scan.rs`'s actual current signatures were
read directly (not assumed from the brief) — the brief's own literal code
sample had, by the time this task ran, already been updated to the correct
3-argument form, so no adjustment was needed. `cargo build -p mcp-nmap`
compiled clean on the first attempt.

**No dedicated unit tests for this file** (by design, matching
`mcp-server/src/main.rs`'s own precedent) — its MCP wiring is exercised by
the companion integration plan's `NmapToolClient` tests against the real
spawned binary, not by this crate's `cargo test`.

### Two bugs found live by this task's own mandated manual smoke test

The task brief requires a manual smoke check (pipe an `initialize` request
into the built binary) before committing. Going one step further —since
`nmap` is actually installed on this machine— and invoking a real
`nmap_quick_scan` against `127.0.0.1` surfaced two real, previously-hidden
bugs. Neither Tasks 1-4's automated tests could have caught either: all of
them fake the `nmap` process (`FakeNmapProcess` in `scan.rs`'s tests) or use
hand-written synthetic XML fixtures (`report.rs`'s Task 1 fixtures) — Task 5
is the first point in this plan where a *real* `nmap` process is ever
actually spawned and its *real* XML output ever actually parsed.

**Bug 1 (`main.rs`, this task's own file) — nmap's console output corrupted
the MCP stdio wire.** `std::process::Command`'s default stdio behaviour is
to *inherit* the parent's stdin/stdout/stderr. nmap always writes its
human-readable report to its own stdout/stderr in addition to the `-oX` XML
file this crate reads — and since that inherited stdout is the exact same
stream `rmcp`'s stdio transport uses for the JSON-RPC wire, a real scan's
console output (banner, port table, "Nmap done...") interleaved directly
into the protocol stream. Confirmed live: a `tools/call` produced ~20 lines
of nmap's raw text on stdout with the actual JSON-RPC response nowhere to
be found in the captured output.

**Fix:** `RealNmapProcess::run` now sets `.stdout(Stdio::null())
.stderr(Stdio::null())` on the spawned `Command`. The `-oX <path>` XML file
this crate actually reads is unaffected (it's a separate, explicit
argument, not stdout).

**Bug 2 (`src/report.rs`, Task 1) — `parse_nmap_xml` rejected every real
nmap scan as malformed.** Real `nmap -oX` output always opens with
`<!DOCTYPE nmaprun>` (confirmed by capturing a live `-oX` file from this
machine's `nmap 7.95`), plus an `<?xml-stylesheet ...?>` PI and an
entity-bearing XML comment. `roxmltree::Document::parse` rejects **any**
DTD declaration by default (`ParsingOptions::allow_dtd` defaults to
`false`, a defense against XML entity-expansion attacks) — and none of
Task 1's three hand-written fixtures included a DOCTYPE line, so this path
was never exercised before. Every real scan on every machine failed with
`ParseError::MalformedXml("XML with DTD detected")` — the crate has been
non-functional against real nmap output since Task 1, undetected until this
task's mandated smoke check.

**Fix:** `parse_nmap_xml` now calls `Document::parse_with_options(xml,
ParsingOptions { allow_dtd: true, ..Default::default() })` instead of
`Document::parse(xml)`. Safe here because nmap's DOCTYPE has no internal or
external subset (bare `<!DOCTYPE nmaprun>` token) — nothing for an
entity-expansion attack to exploit even with `allow_dtd: true`.

- New fixture `src/fixtures/real_nmap_output_with_dtd.xml` — derived
  directly from a live `-oX` capture against `127.0.0.1` on this machine
  (trimmed to 3 ports for size, but preserving the real DOCTYPE,
  `xml-stylesheet` PI, entity-bearing comment, `<hostnames>` block, and
  `<extraports>`/`<extrareasons>` structure).
- New test `parses_real_nmap_output_including_doctype_and_stylesheet_pi` —
  RED-first: failed against the unmodified `Document::parse` call with
  `MalformedXml("XML with DTD detected")` (confirmed the exact right
  failure reason, not a compile error), then GREEN after the
  `parse_with_options`/`allow_dtd` fix.

### Observed, not fixed — dropped response is a smoke-test artifact

A third symptom surfaced during triage: piping a single-shot request with
stdin closing immediately after made the `tools/call` response vanish
entirely (`rmcp`'s stderr trace showed `input stream terminated` almost
immediately, then `timed out draining in-flight responses` ~5s later, right
around when the real ~5.3s scan finished). Re-running the *identical,
unmodified* code with stdin held open past the scan's duration delivered
the response correctly — proving this is a race between stdin-closing and
`rmcp`'s in-flight-response drain timeout, not a thread-pool-blocking bug
from `RealNmapProcess`'s synchronous `Command::status()` call inside an
`async fn`. In production, the orchestrator's `NmapToolClient` (companion
integration plan) holds the child's stdin open for the sidecar's entire
process lifetime (same persistent-process pattern as `mcp-server`'s
session), so this race never triggers there. An `advisor()` review call
confirmed a `spawn_blocking`-based "fix" would not have changed the
observed outcome (it doesn't shrink nmap's own runtime) and was correctly
left unimplemented — out of scope for a task meant to stay thin MCP glue.

### Verification

- `cargo test -p mcp-nmap` → `test result: ok. 38 passed; 0 failed` (37
  pre-existing + 1 new DTD-parsing test), no warnings.
- `cargo build` (whole workspace) → clean.
- `cargo clippy -p mcp-nmap --all-targets` → no warnings.
- `cargo fmt -p mcp-nmap --check` → `main.rs` and the touched line in
  `report.rs` clean; remaining `markdown.rs` drift is pre-existing
  (documented in this crate's own CHANGELOG since 0.3.0).
- Live manual smoke tests against the built binary, real `nmap 7.95`
  installed on this machine:
  1. Bare `initialize`, single-shot pipe (brief's literal Step 2 check) →
     valid JSON-RPC result, clean exit 0.
  2. `tools/list` → both tools present with correct schemas.
  3. `tools/call` → `nmap_quick_scan` against `127.0.0.1`, stdin held open
     → clean stdout (only the two expected JSON-RPC lines), `is_error:
     false`, correct 15-open-port report and summary.

## `network_info.rs` — `local_network_info`/`traceroute` (0.7.0)

Modulo nuovo, mirror strutturale di `elevate.rs` (Windows-only via
`#[cfg(windows)]`/`#[cfg(not(windows))]`, nessuna astrazione mockabile tipo
`NmapProcess` — YAGNI, un solo chiamante). `run_and_capture` è l'unico punto
che tocca `std::process::Command` in questo modulo. `local_network_info`
esegue 4 comandi fissi in sequenza (`ipconfig /all`, `arp -a`, `route print`,
`netstat -rn`), concatena l'output in Markdown con una sezione per comando;
`is_error` è `true` SOLO se tutti e 4 falliscono (un fallimento parziale
lascia comunque dati utili). `traceroute` valida il target con
`scan::validate_target` (bumpata da privata a `pub(crate)` in questo task —
stessa whitelist/stesso motivo dei tool di scan: mai passare argv non
validato a un processo esterno) prima di invocare `tracert -d -w 1000`.

**Differenza deliberata da `ScanOutcome`**: `NetworkInfoOutcome{output,
is_error}` non ha `report_markdown`. I tool di scan nascondono il dettaglio
al modello (solo `summary`, il report va in finestra+Library per l'umano) —
qui è l'opposto: l'AI deve leggere l'IP/subnet/hop per decidere il prossimo
target di scan, quindi tutto l'output va nel tool_result che il modello vede.

**Nota nota (0.7.1):** `run_and_capture` usa `String::from_utf8_lossy`
diretto, quindi riproduce il mojibake OEM già documentato in
`Docs/KNOWN-ISSUES.md` ("Codepage — output dei comandi NATIVI") sulle
etichette accentate (`Sì` → `S�`). Non corretto qui: IP/subnet/gateway/MAC —
i dati ASCII che l'AI legge per scegliere un target — non ne risentono. Fix
vera è app-wide (tocca anche `mcp-server/src/session.rs`), fuori scope.

```rust
pub struct NetworkInfoOutcome { pub output: String, pub is_error: bool }

fn run_and_capture(exe: &str, args: &[&str]) -> Result<String, String>

#[cfg(windows)]
pub fn local_network_info() -> NetworkInfoOutcome
#[cfg(windows)]
pub fn traceroute(target: &str) -> NetworkInfoOutcome

#[cfg(not(windows))]
pub fn local_network_info() -> NetworkInfoOutcome  // stub leggibile
#[cfg(not(windows))]
pub fn traceroute(_target: &str) -> NetworkInfoOutcome  // stub leggibile
```

### `local_network_info` — flow

Itera `COMMANDS: [(&str, &[&str]); 4] = [("ipconfig", &["/all"]), ("arp",
&["-a"]), ("route", &["print"]), ("netstat", &["-rn"])]` in quest'ordine
(dal più generale — indirizzi/interfacce — al più specifico — routing),
concatenando l'output di ciascuno in una sezione Markdown (`## {exe} {args}`
+ blocco fenced). Un comando che fallisce (es. binario assente su una build
Windows atipica) produce una riga `_Errore: ..._` invece di interrompere
l'intero report — `is_error` diventa `true` solo se **nessuno** dei 4 è
riuscito, non al primo fallimento.

### `traceroute` — flow

1. `validate_target(target)` — stesso gate di sicurezza usato da
   `run_quick_scan`/`run_os_detect` (argv flag-smuggling defense — un target
   che inizia con `-` verrebbe letto da `tracert` come flag, non come
   bersaglio posizionale). Su fallimento, ritorna
   `NetworkInfoOutcome { output: <messaggio d'errore>, is_error: true }`
   **prima** di invocare `tracert`.
2. `tracert -d -w 1000 <target>` — `-d` disabilita la risoluzione hostname
   (più veloce, evita attese DNS per hop filtrati); `-w 1000` imposta un
   timeout di 1000ms per hop, molto più basso del default di `tracert`
   (spesso alcuni secondi per hop) — senza questo, un traceroute con molti
   hop filtrati (comune dietro NAT/firewall) potrebbe richiedere minuti.

### Perché niente seam `NmapProcess` qui

`NmapProcess` modella "un processo che scrive un file `-oX` e ritorna un
exit status" — i comandi di questo modulo (`ipconfig`/`arp`/`route`/
`netstat`/`tracert`) non scrivono alcun file, ritornano l'output
direttamente su stdout. Riusare quel trait qui avrebbe richiesto piegarne la
semantica per un caso che non calza, oppure introdurre una seconda
astrazione mockabile per un solo chiamante (`run_and_capture`) — YAGNI. I
test in questo modulo non mockano `std::process::Command`: verificano solo i
percorsi deterministici che non richiedono di spawnare processi reali (JSON
round-trip, gate di validazione del target).

### Windows-only in v1

Stesso trattamento di `elevate.rs`: `ipconfig`/`arp`/`route`/`netstat`/
`tracert` sono nomi di comando specifici di Windows (gli equivalenti
macOS/Linux sarebbero `ifconfig`/`arp`/`netstat -rn`/`traceroute` — binari
diversi, fuori scope). `#[cfg(not(windows))]` ritorna uno stub leggibile
("...non ancora supportate/o su questa piattaforma") per entrambe le
funzioni, mantenendo l'API pubblica del crate identica multi-piattaforma —
nessun impatto sul requisito di gemellaggio macOS/Linux, dato che non c'è
ancora comportamento reale da "gemellare".

### Test coverage

3 test unitari eseguibili su questa macchina Windows (più un 4° gated
`#[cfg(not(windows))]`, che non compila/gira qui):

| Test | Assertion |
|------|-----------|
| `network_info_outcome_json_roundtrips` | `NetworkInfoOutcome` sopravvive a un round-trip serde JSON |
| `traceroute_rejects_invalid_target_before_spawning_anything` | un target tipo `--script=vuln` (che inizia con `-`) viene rigettato dal gate `validate_target` prima di invocare `tracert` |
| `traceroute_rejects_target_with_whitespace` | un target con spazi (`"192.168.1.10 extra"`) viene rigettato |
| `non_windows_stubs_are_readable_errors_not_panics` (`#[cfg(not(windows))]`) | entrambi gli stub non-Windows ritornano un messaggio leggibile menzionante "piattaforma", non un panic |

### TDD (RED → GREEN)

`network_info.rs` è stato scritto in due passaggi, stesso pattern usato per
`report.rs`/`scan.rs` in Task 1/3: primo passaggio solo il modulo
`#[cfg(test)] mod tests` (agganciato in `lib.rs` — necessario perché il file
venisse anche solo considerato dal compilatore), riferendo
`NetworkInfoOutcome`/`traceroute`/`local_network_info` non ancora esistenti.
`cargo test -p mcp-nmap` confermato RED: 4 errori di compilazione
(`E0422`/`E0425`), tutti che nominano i simboli mancanti. Secondo passaggio:
aggiunta l'implementazione completa (struct, `run_and_capture`,
`windows_impl`, stub non-Windows) più il bump di `scan::validate_target` a
`pub(crate)` (necessario perché il modulo compilasse); GREEN — 41 test
totali (38 preesistenti + 3 nuovi eseguiti su Windows).

### Dependencies

- `crate::scan::validate_target` (Task 1's security gate, bumpato a
  `pub(crate)` in questo task — unico consumatore esterno al modulo
  `scan`).
- `serde`/`serde_json` (già dichiarati in `Cargo.toml`) per
  `Serialize`/`Deserialize` su `NetworkInfoOutcome`.
- Nessuna nuova dipendenza esterna — solo `std::process::Command`.

## Aggiornamento `format_nmap_invocation` (0.7.0)

`markdown.rs`'s `format_nmap_invocation` guadagna 2 nuovi arm:
`"local_network_info"` → etichetta fissa `"informazioni di rete locali"`
(nessun target da mostrare — a differenza degli altri 3 tool, questo non ha
parametri); `"traceroute"` → `"traceroute → {target}"`, stesso pattern degli
altri tool che accettano un target. L'arm di fallback `other => "[tool
{other}]"` resta invariato — testato dallo stesso
`invocation_label_unknown_tool_falls_back` preesistente (nome tool
`"nope"`, non toccato da questi 2 nuovi arm).

TDD: i 2 nuovi test (`invocation_label_local_network_info_has_no_target`,
`invocation_label_traceroute_shows_target`) sono stati scritti per primi.
`cargo test -p mcp-nmap invocation_label` confermato RED per il motivo
giusto: entrambi falliscono un `assert_eq!` (non un errore di compilazione)
perché `format_nmap_invocation` cadeva ancora nell'arm di fallback per
questi due nomi non ancora riconosciuti (`"[tool local_network_info]"` /
`"[tool traceroute]"` invece dell'etichetta attesa). Poi aggiunti i 2 nuovi
arm; GREEN — 43 test totali.

## `main.rs` — 2 nuovi tool MCP (0.7.0)

`TracerouteParams { target: String }` — stesso pattern di
`NmapQuickScanParams`/`NmapOsDetectParams` (`#[derive(Debug, Deserialize,
schemars::JsonSchema)]`). `local_network_info` non ha una struct parametri
propria (nessun parametro — il tool `#[tool]` corrispondente non accetta
`Parameters<...>` nella firma).

```rust
#[tool(description = "... Usa questo PRIMA di uno scan per determinare il proprio IP/subnet quando l'utente non lo specifica. Ritorna { output, is_error }.")]
async fn local_network_info(&self) -> String

#[tool(description = "Traccia il percorso di rete (tracert) verso un host. Ritorna { output, is_error }.")]
async fn traceroute(&self, Parameters(TracerouteParams { target }): Parameters<TracerouteParams>) -> String
```

Entrambi chiamano direttamente in `network_info::local_network_info()`/
`network_info::traceroute(&target)` e serializzano `NetworkInfoOutcome` in
una stringa JSON bare — stessa convenzione dei tool di scan esistenti
(successo/errore codificati come dati via `is_error`, mai come errore MCP a
livello di protocollo).

**Verificato via smoke test manuale** (non un test automatico — stesso
principio già stabilito per `main.rs` in Task 5): `initialize` +
`tools/list` pipati in `target/debug/mcp-nmap.exe` mostrano tutti e 4 i
tool con schema corretto — `local_network_info` con `inputSchema:
{properties: {}, type: "object"}` (nessun parametro), `traceroute` con
`target: {type: "string"}` in `required`.

## `src/scan.rs` — `run_scan_with_flags` refactor + 3 new scan tools (0.8.0)

**Single responsibility unchanged, one new layer of sharing.** Before this
task, `run_quick_scan` was the only unelevated scan function and owned its
entire pipeline body directly (validate target → PATH guard → tempfile for
`-oX` → `process.run(...)` → exit-code guard → read + parse the XML →
build `ScanOutcome`). Adding `nmap_version_scan`/`nmap_host_discovery`/
`nmap_vuln_scan` — three more unelevated tools that differ from
`nmap_quick_scan` *only* in which flag(s) they pass to `nmap` — made that
pipeline worth extracting rather than copy-pasting a fourth time.

```rust
fn run_scan_with_flags(
    target: &str,
    process: &dyn NmapProcess,
    nmap_available: bool,
    extra_args: &[&str],
) -> ScanOutcome

pub fn run_quick_scan(target: &str, process: &dyn NmapProcess, nmap_available: bool) -> ScanOutcome
pub fn run_version_scan(target: &str, process: &dyn NmapProcess, nmap_available: bool) -> ScanOutcome
pub fn run_host_discovery(target: &str, process: &dyn NmapProcess, nmap_available: bool) -> ScanOutcome
pub fn run_vuln_scan(target: &str, process: &dyn NmapProcess, nmap_available: bool) -> ScanOutcome
```

`run_scan_with_flags` is `run_quick_scan`'s original body, unchanged except
that the previously-hardcoded `let args = ["-sT", "-oX", &xml_path_str,
target];` line becomes `let mut args: Vec<&str> = extra_args.to_vec();
args.push("-oX"); args.push(&xml_path_str); args.push(target);` — the flags
that used to be the one fixed `"-sT"` are now whatever the caller passes as
`extra_args`. `run_quick_scan` itself shrinks to a one-line wrapper:
`run_scan_with_flags(target, process, nmap_available, &["-sT"])`. The three
new functions are equally thin wrappers, differing from `run_quick_scan`
only in the slice passed as `extra_args`:

| Function | `extra_args` | nmap behaviour |
|---|---|---|
| `run_quick_scan` | `&["-sT"]` | TCP connect scan (unchanged from before this task) |
| `run_version_scan` | `&["-sV"]` | probes open ports for service/version info |
| `run_host_discovery` | `&["-sn"]` | ping-style sweep, no port scan at all |
| `run_vuln_scan` | `&["--script", "vuln"]` | nmap's own curated `vuln` NSE script category |

None of the four is elevated — all still go through the `NmapProcess`
trait, same as `run_quick_scan` always has. `run_os_detect` (`-O`,
elevated via `crate::elevate::run_elevated`) is untouched by this task and
does **not** call `run_scan_with_flags` — it has its own direct pipeline,
for the reasons already documented in its own section above (no meaningful
mock exists for a real UAC round-trip).

### Why the refactor came with no dedicated test of its own

`run_scan_with_flags` is `private` (not `pub`, not `pub(crate)`) — nothing
outside `run_quick_scan`/`run_version_scan`/`run_host_discovery`/
`run_vuln_scan` calls it directly, so there is nothing to unit-test in
isolation that isn't already covered by testing its four callers. The
refactor step itself was verified purely by regression: all 16
pre-existing `scan::tests` (`quick_scan_*`, `validate_target_*`,
`os_detect_*`) were re-run immediately after extracting the helper, before
writing a single new test, and stayed green with the identical assertions
— proof the extraction didn't change `run_quick_scan`'s observable
behaviour at all.

### `nmap_vuln_scan`'s hardcoded script category — a security boundary, not a placeholder

`run_vuln_scan`'s body is exactly `run_scan_with_flags(target, process,
nmap_available, &["--script", "vuln"])` — there is no parameter, no
config value, and no code path anywhere in this crate that could cause a
different string to reach that position in `args`. This mirrors
`validate_target`'s own rationale (0.5.0): accepting an AI/caller-supplied
script name here would reopen the same class of concern that fix already
closes for targets — running an attacker- or model-influenced NSE script
via argv smuggling. The test
`vuln_scan_always_uses_the_fixed_vuln_category_never_a_custom_script`
makes this an executable guarantee rather than just a doc comment: a
`CapturingProcess` fake records the literal `args` slice
`run_vuln_scan` hands to `NmapProcess::run` and asserts it always starts
with `["--script", "vuln"]`, regardless of `target`.

### Test coverage (7 new tests)

Two per new function (mirroring `run_quick_scan`'s own existing test
pairs) plus the one security-specific test above:

| Test | Assertion |
|------|-----------|
| `version_scan_uses_sv_flag_and_produces_summary` | success path with a fake process → `is_error: false`, summary/report mention the target/port |
| `version_scan_invalid_target_is_a_readable_error_before_any_process_call` | an argv-smuggling target is rejected before `process.run` is ever reached |
| `host_discovery_uses_sn_flag_and_produces_summary` | success path → `is_error: false`, summary mentions "host attivo" |
| `host_discovery_nmap_not_available_is_a_readable_error_before_any_process_call` | `nmap_available: false` short-circuits before `process.run` |
| `vuln_scan_uses_script_vuln_and_produces_summary` | success path → `is_error: false`, summary mentions the target |
| `vuln_scan_invalid_target_is_a_readable_error_before_any_process_call` | an argv-smuggling target is rejected before `process.run` is ever reached |
| `vuln_scan_always_uses_the_fixed_vuln_category_never_a_custom_script` | captures the real `args` passed to `NmapProcess::run`, asserts they are always exactly `["--script", "vuln", ...]` |

### `src/markdown.rs` — 3 new invocation-label arms

`format_nmap_invocation` gains `"nmap_version_scan"` → `"nmap version scan
(-sV) → {target}"`, `"nmap_host_discovery"` → `"nmap host discovery (-sn)
→ {target}"`, `"nmap_vuln_scan"` → `"nmap scansione vulnerabilità
(--script vuln) → {target}"`. The vuln-scan label deliberately spells out
`--script vuln` in the confirm-banner text itself — the same "make the
scary part visible before it runs" precedent `nmap_os_detect`'s "richiede
privilegi elevati" label already set — so a user confirming the action
never has to wonder whether an arbitrary script might run.

### `src/main.rs` — 3 new tool methods + param DTOs

`NmapVersionScanParams`/`NmapHostDiscoveryParams`/`NmapVulnScanParams`,
each a single-field `{ target: String }` DTO, same shape and derive list
as the existing `NmapQuickScanParams`/`NmapOsDetectParams`. The three new
`#[tool]` methods (registered between `nmap_os_detect` and
`local_network_info`) each call their matching `scan::run_*` function with
`&RealNmapProcess` and `scan::nmap_on_path()`, then
`serde_json::to_string` the resulting `ScanOutcome` — identical pattern to
`nmap_quick_scan`'s existing method, no new serialization logic. The
startup log line (`tracing::info!("Lare Terminal mcp-nmap v0.8.0
starting...")`) was bumped alongside the version, per this project's own
established precedent (documented in `network_info.rs`'s 0.7.0 changelog
entry) that this string always tracks the crate version.

**Verified via manual smoke test** (same as every prior task in this
crate — not part of the automated suite): `initialize` + `tools/list`
piped into `target/debug/mcp-nmap.exe` list all 7 tools
(`nmap_quick_scan`, `nmap_os_detect`, `nmap_version_scan`,
`nmap_host_discovery`, `nmap_vuln_scan`, `local_network_info`,
`traceroute`), each new scan tool's schema showing `target: {type:
"string"}` in `required`, consistent with the existing scan tools' schemas.

### Not touched by this task

- `run_os_detect`/`elevate.rs` — untouched, still the only elevated path,
  still calling `elevate::run_elevated` directly rather than through
  `NmapProcess`/`run_scan_with_flags`.
- The orchestrator side (`NmapToolClient::tool_defs()`/`dispatch()`,
  `SENSITIVE_TOOLS` gating, `NMAP_SYSTEM_PROMPT`) — deferred to Task 2 of
  the scan-variants plan; this crate's `ScanOutcome{summary,
  report_markdown, is_error}` JSON shape is unchanged, so the existing
  `NmapScanOutcomeJson`/`call_scan_tool` on the orchestrator side need no
  new struct, only new call sites.

## `src/markdown.rs` — NSE script output surfaced in both formatters (0.8.1)

**The bug this closes:** `nmap_vuln_scan`'s report window was byte-identical
to `nmap_quick_scan`'s for the same target. Root cause lived entirely in
`report.rs` (see "NSE script capture" above) — `ScanReport` never carried
script data at all, so neither formatter in this file was structurally
capable of showing it, no matter what nmap actually found.

### `format_report_markdown` — two new per-host blocks

Inserted after the existing ports block, before the OS-detection block:

```rust
let ports_with_scripts: Vec<&crate::report::PortReport> = host
    .ports.iter().filter(|p| !p.scripts.is_empty()).collect();
if !ports_with_scripts.is_empty() {
    // "**Risultati script per porta:**" — one fenced block per port,
    // "{port}/{protocol}:" heading, then "{id}: {output}" per script.
}
if !host.host_scripts.is_empty() {
    // "**Risultati script a livello host:**" — one fenced block,
    // "{id}: {output}" per script.
}
```

Both blocks are conditional on non-empty data — a scan that never ran a
script (every tool except `nmap_vuln_scan`) renders no script section at
all, verified by `markdown_report_without_scripts_has_no_script_sections`.

### `format_summary` — script *count*, deliberately not a verdict

**This is the crux of the original bug, not a cosmetic addition.**
`format_summary` is the ONLY channel the AI model itself ever sees in the
tool_result — the user's original bug report included the model
*narrating* "trovate potenziali vulnerabilità" based on nothing but a port
count, because `format_summary` never told it whether any script had
actually run. Fixing only `format_report_markdown` (the human-visible
Markdown window) would have left that half of the bug alive; the model
would still have had zero real signal to reason from.

The fix sums script results across all up hosts
(`host_scripts.len() + Σ port.scripts.len()`) and, only when that count is
`> 0`, appends `", {N} risultato/i script NSE"` to the existing sentence —
otherwise the wording is byte-for-byte identical to before this fix
(verified by `summary_without_scripts_matches_existing_wording_exactly`, a
dedicated regression guard). **Deliberately a count, not a
vulnerable/not-vulnerable classification**: nmap's own NSE scripts format
their `output` text far too inconsistently (see the `smb-vuln-ms10-054`
example above — a script's own "not vulnerable" result is literally the
string `"false"`) for any generic parsing here to be reliable. The model
gets a real, if coarse, signal and is pointed at the Markdown report for
the actual detail — same spirit as the pre-existing open-port-count
sentence, not a new design idiom.

### Test coverage (5 new tests, `markdown.rs`)

| Test | Assertion |
|------|-----------|
| `markdown_report_includes_per_port_script_output` | full report contains "Risultati script per porta", the script id, and its `output` text |
| `markdown_report_includes_host_level_script_output` | full report contains "Risultati script a livello host" and the host-script id |
| `markdown_report_without_scripts_has_no_script_sections` | a plain report (no scripts) contains neither script heading |
| `summary_without_scripts_matches_existing_wording_exactly` | regression guard — a plain report's summary is byte-identical to the pre-0.8.1 wording |
| `summary_with_scripts_reports_script_count_not_a_vulnerable_verdict` | summary mentions "risultato/i script NSE" and the correct count (2), and does NOT contain "vulnerabile" |

### Every existing `PortReport`/`HostReport` literal fixed for the new fields

Two new fields (`PortReport::scripts`, `HostReport::host_scripts`) broke
every existing hand-built literal in this file: `sample_report`'s two
`PortReport` literals plus its `HostReport` literal (gained `scripts:
vec![]`/`host_scripts: vec![]`), and
`summary_zero_hosts_says_none_active_not_an_error`'s inline `HostReport`
literal. `scan.rs`'s test module builds no `PortReport`/`HostReport` by
hand (only through `parse_nmap_xml` over fixture XML), so it needed no
changes — confirmed by a clean `cargo build -p mcp-nmap --tests` after the
`report.rs`/`markdown.rs` fixes.

### Verification

- `cargo test -p mcp-nmap` → `test result: ok. 60 passed; 0 failed` (53
  pre-existing + 7 new: 2 in `report.rs`, 5 in `markdown.rs`), no warnings.
- `cargo build` (whole workspace, default-members) → clean — confirms
  `orchestrator` needs no changes (`ScanOutcome{summary, report_markdown,
  is_error}`'s JSON shape is unchanged; both fields remain plain `String`s).
- `cargo clippy -p mcp-nmap --all-targets` → no warnings.
- `cargo fmt -p mcp-nmap --check` → `report.rs` (fully owned by this task)
  clean. In `markdown.rs`, every line this task touched was hand-formatted
  to rustfmt's canonical style and cross-checked against `git diff` to
  confirm no line remaining in `cargo fmt --check`'s output belonged to
  this task; the residual drift (7 diffs, all on lines pre-existing before
  this task) is the same drift already documented as out-of-scope since
  the 0.3.0 changelog entry.

### `Cargo.toml` + `src/main.rs` — version bump

`Cargo.toml`'s `version` bumped `0.8.0` → `0.8.1` (bug fix, no new tool/API
surface). `main.rs`'s startup log line (`tracing::info!("Lare Terminal
mcp-nmap v0.8.1 starting...")`) bumped alongside it — not explicitly called
out by this task's brief, but kept consistent with this crate's own
established precedent (this string has tracked the crate version since its
first commit; see the 0.7.0/0.8.0 changelog entries) rather than left
stale.

## `src/scan.rs` — bound `nmap_vuln_scan`'s per-script timeout (0.8.2)

### The hang symptom

A real `nmap_vuln_scan` run against a real router (`192.168.178.1`) hit the
orchestrator's 900s outer timeout (`NMAP_CALL_TIMEOUT_SECS`,
`crates/orchestrator/src/nmap_tool_client.rs`) with **no report window
opened at all** — total loss of every port and every script result nmap had
already collected, because nmap never got to write its `-oX` file.

### Live reproduction methodology

Re-ran the exact same scan against the same router with `--stats-every 15s`
added (nmap's own periodic progress reporting) to observe the stall instead
of guessing at it. The trace showed NSE completion reaching **99.76% within
45 seconds**, then flatlining there for **5+ minutes straight with zero
further progress** — one specific script in the `vuln` NSE category hangs
indefinitely against this router. The most likely cause: a script probing
the SIP (5060) or UPnP/wsdapi (5357) port, both known to silently drop
unexpected probes on consumer routers rather than resetting or erroring,
which leaves an NSE script with no timeout of its own waiting forever.
Nothing in `run_vuln_scan`'s invocation bounded any individual script's
execution time — the only thing that ever stopped a hang was the
orchestrator's *outer* 900s timeout, which kills the orchestrator's wait on
the child process, not the nmap process itself, and discards 100% of the
scan data because nmap never reaches the point of writing `-oX`.

### The fix

`run_vuln_scan`'s `extra_args` gained nmap's own `--script-timeout 60s`
flag, alongside the existing `--script vuln`:

```rust
pub fn run_vuln_scan(target: &str, process: &dyn NmapProcess, nmap_available: bool) -> ScanOutcome {
    run_scan_with_flags(
        target,
        process,
        nmap_available,
        &["--script", "vuln", "--script-timeout", "60s"],
    )
}
```

`--script-timeout` bounds each *individual* NSE script's execution time —
when one script exceeds it, nmap abandons just that script and moves on,
still writing a complete `-oX` file with every port and every other
script's completed result intact. This is a real, verified nmap flag (not a
hand-guessed name): confirmed via `nmap --help` listing `--host-timeout` in
the same option group, and via `nmap --script-timeout 5s -sn 127.0.0.1`
running without an "unrecognized option" error. 60s was chosen generously
above what any legitimate script took in the reproduction run (the slowest
completed script finished well under 10s) — a sane per-script upper bound,
not a value tuned to the specific failure.

No other scan function is affected: `run_quick_scan`/`run_version_scan`/
`run_host_discovery` never pass `--script` at all, and `run_os_detect`
(`-O`, elevated) has its own separate pipeline that doesn't go through
`run_scan_with_flags`.

### Verification (re-ran the exact hanging scenario)

With `--script-timeout 60s` added, the identical scan against the identical
router completed cleanly in **71.44 seconds** — full report, all 9 ports,
all other script results intact — instead of hanging past 900s and
producing nothing.

### New test

`vuln_scan_always_bounds_script_execution_time` (`src/scan.rs`, right after
`vuln_scan_always_uses_the_fixed_vuln_category_never_a_custom_script`) —
defines its own local `CapturingProcess` fake (the same pattern the
existing test in that position already uses; two `#[test]` functions can't
share a struct scoped inside another test function's body, which is why
this file already has two separate `PanicsIfCalled` definitions for the
same reason). Captures the real `args` slice `run_vuln_scan` hands to
`NmapProcess::run` and asserts `["--script-timeout", "60s"]` always appears
as a contiguous pair, regardless of `target`.

RED-first: with the fix not yet applied, `cargo test -p mcp-nmap
vuln_scan_always_bounds_script_execution_time` failed the `assert!` (not a
compile error) with `expected --script-timeout 60s in args, got:
["--script", "vuln", "-oX", ..., "192.168.1.10"]` — the flag was genuinely
missing. Then `"--script-timeout", "60s"` was added to `extra_args`; GREEN.

### Verification (build/test)

- `cargo test -p mcp-nmap` → `test result: ok. 61 passed; 0 failed` (60
  pre-existing + 1 new), no warnings.
- `cargo build` (whole workspace, default-members) → clean — confirms
  `orchestrator` needs no changes (`ScanOutcome`'s JSON shape is unchanged;
  this fix only changes the argv passed to nmap, not the output shape).
- `cargo clippy -p mcp-nmap --all-targets` → no warnings.
- `cargo fmt -p mcp-nmap --check` → `scan.rs` (the only code file this fix
  touches) clean after hand-wrapping the new test's `assert!` call to match
  rustfmt's expected line-wrap. Residual `markdown.rs` drift (5 diffs, all
  on lines pre-existing before this fix) remains **pre-existing**, already
  documented as out-of-scope since the 0.3.0 changelog entry.

### `Cargo.toml` + `src/main.rs` — version bump

`Cargo.toml`'s `version` bumped `0.8.1` → `0.8.2` (bug fix, no new tool/API
surface). `main.rs`'s startup log line (`tracing::info!("Lare Terminal
mcp-nmap v0.8.2 starting...")`) bumped alongside it, per this crate's own
established precedent (this string has tracked the crate version since its
first commit; see the 0.7.0/0.8.0/0.8.1 changelog entries).

## Not yet implemented (future work — outside this crate)

- Wiring into the orchestrator (`NmapToolClient`, `DispatchOutcome`/report
  side-channel, channel + `SENSITIVE_TOOLS` registration) and the frontend
  (confirm-banner CSS, `external-channel-window.js` report handling,
  channel registry entries) — companion integration plan,
  `Docs/superpowers/plans/2026-07-16-mcp-nmap-integration.md`.
- A real interactive UAC end-to-end test of `run_elevated`/`run_os_detect`
  — requires a live scan against a real target with a real elevation
  prompt, not something an automated `cargo test` run (or this task's
  stdio-only smoke test, which only exercised `nmap_quick_scan`) can cover.
