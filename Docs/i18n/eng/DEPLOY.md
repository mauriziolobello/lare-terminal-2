# DEPLOY — Preparing an Executable Directory for Lare Terminal 2.0

How to populate `Test Run\` (or a copy elsewhere) with binaries and configuration **after**
compiling ([`BUILD.md`](./BUILD.md)). Does not contain launch instructions — those are in
[`RUN.md`](./RUN.md).

`Test Run\` in the repository **already is** a deployment: population scripts, manifests, and
directory structures are committed; binaries, DLLs, secrets, and runtime data are not (gitignored).
For everyday development, nothing else is needed — `Test Run\` itself is the folder from which to run
the program (see [`RUN.md`](./RUN.md)). This page also guides anyone wishing to copy the application
to another machine or directory.

## Prerequisites on the target machine

- **Windows 10/11 x64**.
- **WebView2 Runtime** (typically pre-installed on updated Windows 10/11; required by Tauri to host
  windows).
- **pwsh 7.6+ installed** (`C:\Program Files\PowerShell\7` or in PATH): `lare-shell` loads PSReadLine
  from real pwsh modules, which the `Microsoft.PowerShell.SDK` NuGet package does not bundle
  (`PwshLocator`, ADR-019 item 2) — without pwsh, line editing falls back to basic `Console.ReadLine`,
  though the shell remains usable.
- **.NET 10 runtime x64**: `lare-shell` is published *framework-dependent* (publish
  `-r win-x64 --self-contained false`, ~100 MB instead of ~200 MB self-contained). Measured live:
  `Test Run\shell\` (`lare-shell.exe` plus PowerShell engine DLLs, including `System.Management.
  Automation.dll`) weighs **41.4 MB**. Without the runtime, the process will not start (missing
  framework error, not a Lare crash).
- A Python virtual environment per domain in `pytools\<domain>\venv\` **created manually** on the
  target machine (never deployed: see below) — required only if utilizing tools depending on that
  domain (e.g. `/markets` → `financial-markets`).

## Populating (or re-populating) `Test Run\`

```powershell
.\deploy_test_run.ps1                          # from target\debug\ (debug build, see BUILD.md)
.\deploy_test_run.ps1 -IncludePlugins           # as above, plus ping.exe/calc.exe in plugins\<id>\
.\deploy_test_run.ps1 -BuildConfig release      # from target\release\ instead of target\debug\
.\deploy_test_run.ps1 -SkipShell                # skips lare-shell publishing (~1 minute) —
                                                 # useful when only recompiling Rust
```

The script **never touches** `Test Run\Configuration\` (tokens, generated configs, logs — gitignored,
persisting across deployments) and always publishes `lare-shell` to `Test Run\shell\` via
`dotnet publish -c Release -r win-x64 --self-contained false` (the host has no useful "debug" build
for deployment — see [`BUILD.md`](./BUILD.md)), unless `-SkipShell` is specified.

It concludes with `$LASTEXITCODE` non-zero even on success (known technical debt: `robocopy` returns
1 for "files copied"): this is not an error signal, check the console output
(`Test Run pronta: ...` on the final line).

**Quick deployment sanity check** (without typing or running the orchestrator):

```powershell
& ".\Test Run\shell\lare-shell.exe" --selftest
```

(`Test Run` contains a space: PowerShell requires the call operator `&` when the command is a quoted
expression — quotes enclose **only the path**, arguments remain outside. `.\Test Run\...` without
quotes splits on the space; `".\Test Run\... --selftest"` with the argument inside quotes becomes a
single string that PowerShell simply echoes rather than executing.)

Prints a `[OK]`/`[FAIL]` line per verification check (configuration folder, `startup.json`, pwsh
discovered, …), exiting with 0 or 1.

## Deployment layout

Paths in `startup.json` are relative to the deployment root (the folder containing `Configuration\`),
rather than repository locations: `Test Run\` (or a copy in any folder) functions without modification.

```
<deploy>\
├── orchestrator.exe  mcp-server.exe  mcp-nmap.exe  ui.exe
├── shell\                                            ← lare-shell.exe + PowerShell engine DLLs
│                                                        + powershell.config.json (execution policy,
│                                                        ADR-019); framework-dependent win-x64 publish
├── install-wt-profile.ps1  uninstall-wt-profile.ps1  ← installs/removes the Windows Terminal
│                                                        "Lare Terminal" profile (Mode B —
│                                                        see RUN.md, executed once)
├── debug_orchestrator.ps1                           ← orchestrator with live console logging, for
│                                                        debugging only (self-heal handles it otherwise —
│                                                        see RUN.md)
├── Configuration\
│   ├── startup.json                                 ← template, contains no secrets
│   ├── README.md  README.it.md  *.example.json
│   ├── token                                         ← generated upon first launch (secret)
│   ├── llms.json                                     ← optional, manual (secret: api_keys)
│   └── logs\                                         ← generated upon first launch
├── plugins\ping\{plugin.json, ping.exe}
├── plugins\calc\{plugin.json, calc.exe}
└── pytools\<domain>\{server.py, requirements.txt, venv\}  ← venv created manually, never deployed
```

## Contents of `Configuration\` — what is a template, what is a secret

| File | Committed (template)? | Secret? | Created by |
|---|---|---|---|
| `startup.json` | Yes | No | template in repo, matches hard-coded defaults |
| `README.md`, `README.it.md` | Yes | No | template in repo (English, Italian) |
| `*.example.json` | Yes | No — `<your-...>` placeholders only | template in repo, to copy without `.example` |
| `token` | No (gitignored) | **Yes** — WS token (256-bit) | `orchestrator`, on first run |
| `llms.json` | No (gitignored) | **Yes** — contains provider `api_keys` | manually, only if using a non-default provider |
| `telegramsettings.json` | No | **Yes** — bot token | manually, only to activate Telegram |
| `telegram-state.json` | No | **Yes** — TOTP secret + chat ID | `orchestrator`, when Telegram is active |
| `config.json` | No | No (local) | `ui.exe`, on first settings save |
| `network.json`, `search-paths.json`, `search-content.json`, `market_data.json` | No | No, but machine-specific | `orchestrator`/`ui.exe`, at runtime |
| `notes.json`, `memory-<label>.md` | No | No | `orchestrator`, at runtime |
| `logs\`, `library\`, `plugin-storage\<id>\` | No | No | respective applications, at runtime |

Full details (with exact lifecycle timing: first run vs every run vs first save) in
`Test Run/Configuration/README.md` (Italian version: `README.it.md` in the same folder), which is
the exact template used across all deployments.

**AI provider credentials.** `ANTHROPIC_API_KEY` (and `OPENROUTER_API_KEY`) remain environment
variables — an explicit design decision outside the scope of D6 (which governs the *location* of Lare
configuration, not provider secrets): set them on the target machine, or create
`Configuration\llms.json` manually to select a non-default provider/model (`claude-sonnet-4-6` via
direct Claude, falling back to `StubAdapter` if neither is available). If neither is configured, the
orchestrator logs `ANTHROPIC_API_KEY non impostata — uso StubAdapter` and responds via a mock
adapter rather than calling Claude.

## Copying the deployment elsewhere

Copy the entire directory (`Test Run\`, or an equivalent staged deployment) to the target: any path
is valid, as relative paths in `startup.json` resolve from the new root. After copying:

- If requiring a non-default AI provider, create `Configuration\llms.json` **manually on the new
  machine** (never copy it from another deployment: it contains API keys).
- If using a `pytools` domain (e.g. `/markets`), create the virtual environment manually inside
  `pytools\<domain>\` on the target machine (see [`BUILD.md`](./BUILD.md) §Python) — venvs are never
  copied by deployment scripts.

To launch the program once the deployment is ready (here or after copying): **[`RUN.md`](./RUN.md)**.
