# Python Tools (pytools)

Certain external channels in Lare Terminal rely, for part of their functionality, on Python scripts
independent of the rest of the system — not Rust modules, not plugins, but a separate domain with its
own dependencies and execution environment. This document explains what they are, how they are
organized, and how to introduce a new domain. For a general overview of the project, see
[`00-opening.md`](./00-opening.md); for the three tiers (channels → orchestrator → tool server)
within which pytools reside, see [`01-architecture.md`](./01-architecture.md).

## What they are, and why they exist in a Rust/C# project

The rest of the backend is Rust, the shell is C#, and the UI is Tauri — pytools is an explicit
exception to this homogeneity. It exists because certain capabilities possess mature Python
ecosystems that would be unreasonable to reinvent from scratch: financial data analysis (`yfinance`,
`mplfinance`, `pandas`, an Interactive Brokers client) for the `financial-markets` domain, and a
dedicated library for the TR-064 protocol of FRITZ!Box routers (`fritzconnection`) for `fritzbox`.
Rewriting these libraries in Rust purely for language uniformity would represent immense effort for
zero benefit — the same logic applied elsewhere in the project behind keeping the shell host in C#
rather than rebuilding from a low-level binding to `Microsoft.PowerShell.SDK` (see
[`01-architecture.md`](./01-architecture.md), "Technology choices").

A pytool is not a bypass around the three-tier architecture: it always remains behind a tool server,
in one way or another (detailed below) — never as a channel speaking directly to the AI or executing
arbitrary code. The AI continues to see a fixed set of tools defined by name, description, and
parameter schema; whether the implementation behind a tool is native Rust or a Python subprocess is
an internal detail hidden from upper layers.

## Two patterns for connecting Python to the system

Two distinct patterns exist today, selected case-by-case based on what each domain genuinely
requires. There are no rigid rules for this choice, but the criterion emerging from existing cases
is straightforward: state that must persist between invocations (or multiple tools under a single
umbrella) calls for the first pattern; a single stateless query integrating neatly into an existing
Rust channel calls for the second.

**Persistent MCP server.** The orchestrator itself spawns the Python process as an MCP server and
communicates with it directly via stdio, using the same protocol (via the `rmcp` library) spoken to
any other tool server. The Rust type handling this is `PythonMcpToolClient`
(`crates/orchestrator/src/python_mcp_tool_client.rs`), explicitly documented in code as a "mirror" of
the client used for native Rust MCP servers — following the same lazy-spawn-and-reuse pattern: the
process is created upon the first request on that channel, stays alive, and is reused for subsequent
calls on the same connection (one client per WebSocket connection), being explicitly terminated on
the Rust side when no longer needed. This is the pattern used by `python-ping` and
`financial-markets`: each possesses its own `server.py` that instantiates a FastMCP object, declares
one or more tools via `@mcp.tool()`, and listens on stdio with `mcp.run(transport="stdio")`.

**One-shot script.** Here the orchestrator does not speak to Python directly: an existing Rust tool
server spawns the Python script as a subprocess for ONE of the tools it exposes, waits for it to
print a single JSON line `{"output": "...", "is_error": bool}` to stdout, and lets it terminate — no
persistent process, no MCP handshake with the script itself. This is the pattern used by `fritzbox`:
the `/netsec` channel (`crates/mcp-nmap`) spawns `fritzbox_status.py` using
`python -X utf8 fritzbox_status.py --config-dir <dir>` (the `-X utf8` flag avoids all OEM encoding
quirks on Windows, unlike the native Win32 commands used by other tools in the same channel), awaits
completion with a dedicated timeout (30 seconds, independent of the channel's general timeout), and
parses the resulting JSON line. A failure — router unreachable, invalid credentials — is always an
**outcome** reported back to the AI as structured data (`is_error: true`, with a readable message),
never an unhandled process crash: the Python script always exits with code 0 whenever it produces an
answer, whatever that answer may be.

## Current domains

A "domain" (or scope) is a subdirectory of `scripts/pytools/` featuring its own virtual environment
— which does not necessarily map 1:1 to a "channel": `python-ping` and `financial-markets` are each
both a pytools domain and a standalone channel (with their own slash trigger and dedicated system
prompt); `fritzbox` is a pytools domain serving a single tool inside an existing Rust channel
(`/netsec`), not a separate channel.

| Domain | Pattern | Used by | What it does |
|---|---|---|---|
| `python-ping` | persistent | `/pyping` channel | Test domain: a single tool, `pyping`, returning a deterministic echo. Isolates mechanism bugs (venv, spawn, MCP handshake) from domain bugs before real Python tooling arrives — zero dependencies beyond `mcp`. |
| `financial-markets` | persistent | `/markets` channel | US stock reporting: fundamentals, charts across five time horizons, option chains, peer comparisons, a filterable tabular list by country, and a screener with a registry of multiple variants (`list_screeners`/`run_screener`). Data sources (yfinance, Interactive Brokers, a mock source for testing) are interchangeable behind a common abstraction. |
| `fritzbox` | one-shot | `fritzbox_status` tool of the `/netsec` channel | Home router diagnostics via TR-064: recent event log entries; public IP and WAN connection status; known local network devices. Openly stated in code: this is not intrusion detection — the FRITZ!Box log records failed logins and VPN attempts, not packets dropped by the firewall; the tool reports what the log contains without over-interpreting it. |

## Practical conventions

- **One virtual environment per domain, never shared across different domains.** Multiple scripts
  belonging to the *same* domain share the same venv (`financial-markets` has several, all under a
  single `venv/`) — avoiding duplication of heavy dependencies such as `pandas` for each script.
- **The venv must never be committed.** `.gitignore` excludes it globally (`**/venv/`,
  `**/__pycache__/`), rather than via a pytools-specific rule. What is committed is everything
  defining the domain: `.py` scripts, `requirements.txt`, and a domain `README.md` when present.
  Runtime-generated files next to `server.py` (caches such as `tickers_us.json`,
  `fundamentals_cache.json`, `discoveries.json` for `financial-markets`) are not source assets.
- **Venv provisioning is intentionally manual**, not an oversight: no mechanism automatically
  creates or updates a venv. Creating or recreating one is always:
  ```powershell
  cd scripts/pytools/<domain-name>
  python -m venv venv
  venv\Scripts\Activate.ps1
  pip install -r requirements.txt
  ```
- **A missing venv never causes crashes**: both the persistent pattern (during channel resolution)
  and the one-shot pattern verify interpreter existence before spawning, returning a clear,
  actionable message directing the user to create the virtual environment rather than an opaque
  system error.

## Configuration: `--config-dir`, as everywhere else

The same single rule described in [`01-architecture.md`](./01-architecture.md) ("Configuration: one
single rule, always") applies here: no project-specific environment variables. Every Python server
or script receives an explicit `--config-dir <dir>` argument from its caller — the orchestrator for
persistent patterns, the Rust crate owning the channel for one-shot patterns — upon spawn,
regardless of whether that domain actually requires it: `python-ping` receives and ignores it
(FastMCP does not inspect `sys.argv`, so extra arguments never trigger "unrecognized arguments"
errors). A domain reading configuration does so via explicit argument parsing
(`config_dir.resolve()` in `financial-markets`, an equivalent helper in `fritzbox_status.py`) — never
via `os.environ.get(...)`.

The first version of the project read this directory from the `LARE_LOCAL_DIR` environment variable
(inherited from the parent process, falling back to `%LOCALAPPDATA%`): a third independent reading
point alongside Rust and the UI, which could quietly drift out of sync. Version 2.0 removes this
variable for the same reason it eliminates all `LARE_*` variables throughout the system.

Typical per-domain config files: `market_data.json` for `financial-markets`, `fritzbox.json` for
`fritzbox` (a template is available in `fritzbox.example.json` inside `Configuration`). One slight
non-uniformity between the two, reported transparently: if the flag is omitted entirely — an edge
case occurring only when launching a script manually during development, never in normal Rust usage
— `financial-markets` falls back to a default path (`<deploy root>/Configuration`), whereas
`fritzbox_status.py` raises an explicit error rather than guessing a path.

## Where the pytools root lives

The path resolution formula is identical and shared by both patterns (`PythonMcpToolClient::resolve`
for persistent, matching logic in `crates/mcp-nmap/src/main.rs` for one-shot): the root (`<root>`) is
`startup.json.paths.pytools_dir` (defaulting to `"pytools"`); if it is an absolute path, it is used
as-is; otherwise, it resolves relative to the **deployment root** — the folder containing
`Configuration/`, which is the parent of the folder passed via `--config-dir`. The `domain_id` is
always joined onto this root:

- interpreter: `<root>/<domain_id>/venv/Scripts/python.exe` (Windows) or
  `<root>/<domain_id>/venv/bin/python3` (Unix)
- script: `<root>/<domain_id>/<script_relpath>`

In v1, this path was also read from a dedicated environment variable (`LARE_PYTOOLS_DIR`) —
eliminated in 2.0 alongside all others.

In normal development workflows, the orchestrator starts with an explicit `--config-dir` pointing to
`Test Run\Configuration`: the deployment root is therefore `Test Run\`, and the default
`pytools_dir: "pytools"` resolves to `Test Run\pytools\` — **not** `scripts/pytools/` in the
repository. There are two legitimate ways to align this:

1. Copy `scripts/pytools/` into `Test Run/pytools/` (the deploy script already does this, explicitly
   excluding `venv/`, `__pycache__/`, `.pytest_cache/`, and generated cache files) and create each
   domain's venv directly inside `Test Run\pytools\<domain>\` — copying never transports a venv; it
   must always be initialized there.
2. Point `paths.pytools_dir` in `Test Run\Configuration\startup.json` to an absolute path pointing
   directly to repository source files, working directly there without copying:
   ```json
   { "paths": { "pytools_dir": "C:/.../Lare Terminal 2.0/scripts/pytools" } }
   ```

## Adding a new domain, step by step

**Persistent pattern** (the common case for domains with multiple tools, or state maintained across
invocations):

1. Create `scripts/pytools/<domain-name>/`, containing `requirements.txt` (at least `mcp<2.0.0`) and
   a `server.py` that sets up `FastMCP`, declares one or more `@mcp.tool()` handlers, and concludes
   with `mcp.run(transport="stdio")`.
2. Create the venv and install dependencies (see above).
3. Register the domain as an entry in `EXTERNAL_TOOL_CHANNELS`
   (`crates/orchestrator/src/external_channel.rs`): specifying `id`, `slash_trigger`,
   `window_title`, a dedicated system prompt, and a `tool_client` factory invoking
   `PythonMcpToolClient::resolve(config_dir, startup, "<id>", "server.py", vec![...])` with a
   `PythonToolSpec` for each exposed tool. This registration is the sole way to make the domain
   accessible as a **channel** with its own slash trigger; a Rust caller can also construct an ad-hoc
   `PythonMcpToolClient` for internal needs without touching the registry (as with the
   `test_market_data_source` diagnostic tool in `financial-markets`, invoked directly via a "Test
   Connection" button in the configuration window, rather than as a channel) — but in either case,
   explicit Rust code must declare the tool: there is no auto-discovery.

An easily overlooked nuance: **a tool is declared twice, with different roles**. In Python, the
`@mcp.tool()` decorator and its docstring constitute the implementation — what the tool actually
does. In Rust, the `def: ToolDef { name, description, input_schema }` field inside each
`PythonToolSpec` is what the AI **sees** — name, description, and parameter schemas are hand-written
on the Rust side, and only tools enumerated in the `vec![...]` passed to `resolve()` for a given
channel are accessible to that channel. `financial-markets/server.py` declares six `@mcp.tool()`
handlers; the `/markets` channel registers five — the sixth, `test_market_data_source`, is excluded
(see above). Rust therefore remains authoritative over which capability subset is exposed and under
what description, regardless of what the Python code defines.

If a tool needs to render its output into a Markdown window (rather than plain text in the chat),
`server.py` can satisfy an opt-in JSON contract — `{"summary", "report_markdown", "channel_summary"[,
"title", "title_suffix"]}` — which the Rust side interprets to construct the window; a tool
returning a simple string (`pyping`) remains plain chat text, which is entirely normal.

**One-shot pattern** (for a single stateless capability attached to an existing Rust channel):

1. Create `scripts/pytools/<domain-name>/` containing the `.py` script and `requirements.txt`.
2. The script accepts `--config-dir` (even if unused, for uniformity with other pytools), performs
   its task, and **always** prints a JSON line `{"output": "...", "is_error": bool}` to stdout
   before exiting with code 0 — error states must be reported as readable data, never as an unhandled
   exception.
3. In the Rust crate owning the target channel (e.g. `mcp-nmap` for `/netsec`), implement spawning
   logic: resolve interpreter and script using the formula above, spawn the subprocess with a
   dedicated timeout, parse the resulting JSON line, and surface the outcome as an ordinary tool for
   that channel.

## Known limitations, openly declared

- No automatic discovery: creating the directory and venv is not enough to make a domain accessible
  — an explicit modification on the Rust side is always required (a registry entry for a persistent
  channel, spawn logic for one-shot patterns, or an ad-hoc client as with
  `test_market_data_source`).
- Behavior in the complete absence of `--config-dir` is not identical across domains (see above) —
  a difference relevant only when manually executing Python scripts, never in normal Rust usage.
- Today only three domains exist; the decision boundary between the two patterns reflects observed
  architectural needs rather than an a priori codified rule.

## Further reading

- [`00-opening.md`](./00-opening.md) — what Lare Terminal is and why it exists.
- [`01-architecture.md`](./01-architecture.md) — the three tiers and the configuration rule
  applying here as well.
- [`03-status-and-implementation.md`](./03-status-and-implementation.md) — overall picture of what
  is implemented today, including pytools.
- [`06-channels.md`](./06-channels.md) — channels utilizing these tools (`/netsec`, `/markets`).
