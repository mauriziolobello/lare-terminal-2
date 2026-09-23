# BUILD — Compiling Lare Terminal 2.0

Compilation and testing only. To populate an executable directory (`Test Run\` or elsewhere), see
[`DEPLOY.md`](./DEPLOY.md); to launch the program once built and deployed, see [`RUN.md`](./RUN.md).

The project uses two independent toolchains, compiled separately:

- **Rust** (Cargo workspace): `orchestrator`, `mcp-server`, `mcp-nmap`, `ui`, plugins (`plugin-ping`,
  `plugin-calc`, …), `startup-config`, `protocol`.
- **.NET** (separate solution, not in the Cargo workspace): `shell/lare-shell/` — the custom C# host
  for the PowerShell engine (ADR-015).

## All in one go — `build.ps1`

To compile everything needed for a functional deployment (Rust + C# host) without memorizing
individual commands:

```powershell
.\build.ps1                                    # debug: default-members + ui + lare-shell
.\build.ps1 -BuildConfig release               # same as above, in release mode
.\build.ps1 -IncludePlugins                    # includes plugin-ping/plugin-calc
.\build.ps1 -SkipUi                            # skips ui (fast iteration on backend only)
.\build.ps1 -SkipShell                         # skips C# host (~1 min saved)
.\build.ps1 -CleanUi                           # + cargo clean -p ui first (frontend gotcha, see below)
```

This does not populate `Test Run\`: afterwards, execute `.\deploy_test_run.ps1` (with matching
`-BuildConfig`) — see [`DEPLOY.md`](./DEPLOY.md). The remainder of this page details what
`build.ps1` executes under the hood, command by command, for developers wanting to compile isolated
components during development.

## Rust — debug (development cycle)

From the repository root:

```powershell
cargo build                                   # only "default-members": orchestrator, mcp-server,
                                               # mcp-nmap, startup-config, protocol, plugins — NOT ui
cargo build -p ui                             # Tauri UI (~400 external crates, slow on first compile)
cargo build -p plugin-ping -p plugin-calc     # plugins used by Test Run\ (see DEPLOY.md)
```

`cargo build` without `-p` deliberately excludes `ui` (too slow for rapid backend iteration) — if
you need the UI as well, compile it separately with `-p ui`.

Output: `target\debug\`.

## Rust — release (for a production deploy)

Same commands with `--release`:

```powershell
cargo build --release
cargo build --release -p ui
cargo build --release -p plugin-ping -p plugin-calc
```

Output: `target\release\`. Only needed when preparing an actual deployment (see
[`DEPLOY.md`](./DEPLOY.md)) — for everyday code development, debug builds are sufficient and much
faster.

## .NET — `lare-shell` (custom C# host for the PowerShell engine)

Separate solution: `shell/lare-shell/LareShell.sln` (`src/LareShell` executable,
`tests/LareShell.Tests` xUnit tests). Implementation details: `shell/lare-shell/IMPLEMENTATION.md`.

```powershell
dotnet build shell/lare-shell/LareShell.sln
```

Debug output: `shell/lare-shell/src/LareShell/bin/Debug/net10.0/win-x64/` — **not**
`bin\Debug\net10.0\` as in a standard .NET project lacking an explicit RID. Both `LareShell.csproj`
and `LareShell.Tests.csproj` declare `<RuntimeIdentifier>win-x64</RuntimeIdentifier>` (ADR-019 item 1):
without an explicit RID, `$PSHOME` would resolve inside `runtimes\win\lib\net10.0\` rather than the
executable directory, causing `powershell.config.json` next to the binary (execution policy) to be
ignored.

**`lare-shell` has no useful "release" build directly from source**: deployments always use a
framework-dependent *publish* (see [`DEPLOY.md`](./DEPLOY.md)) — `dotnet build`/`dotnet run` serve
solely for developing and testing the host, not for staging a deployment.

Running from source (without publishing) — always requires an explicit `--config-dir`, as no
useful default exists next to `bin\Debug\...\`:

```powershell
dotnet run --project shell/lare-shell/src/LareShell -- --config-dir "Test Run\Configuration"
```

## Testing

```powershell
cargo test                                    # backend crate tests (default-members)
cargo test -p ui                              # ui crate tests (not a default-member)
cargo test -p orchestrator <substring>        # a single test (or filtered subset by name)
cargo test -p orchestrator -- --ignored       # live integration tests (external APIs, binaries, venv)
node --test crates/ui/frontend/*.test.mjs     # pure JS frontend tests (242 tests, no webview)
cargo clippy --all-targets                    # linter across all workspace targets
cargo fmt --check                             # formatting check (v1 is not fmt-clean: known diffs)

dotnet test shell/lare-shell/LareShell.sln                                        # 118 xUnit tests
dotnet test shell/lare-shell/LareShell.sln --filter "FullyQualifiedName~Executor"  # single test file
```

**Python (`scripts/pytools/`)**: each domain (`financial-markets/`, `python-ping/`) has a local,
manually created virtual environment that is never committed:

```powershell
cd scripts/pytools/financial-markets
python -m venv venv
venv\Scripts\Activate.ps1
pip install -r requirements.txt pytest
pytest -q                                     # from domain folder, or:
pytest scripts/pytools/financial-markets -q   # from repo root with venv active
```

`python-ping/` lacks its own venv (no domain dependencies): for manual smoke testing, one can reuse
the Python interpreter from the `financial-markets` venv.

## Development utility: speaking the shell channel without a real console

`scripts/dev/shell-client.mjs` mimics what `lare-shell.exe` does on the shell channel from the client
side (`Hello{role:"shell"}`, issues a `Command`, then responds to orchestrator messages) — convenient
for verifying orchestrator and `ui.exe` **without** building or publishing the C# host, or from a
terminal with redirected stdin (e.g. Claude Code) where an interactive `[Y/n]` gate cannot operate.

```powershell
# With orchestrator and ui.exe already running (see RUN.md):
node scripts/dev/shell-client.mjs -- '/ping'          # window on ui.exe displaying table
node scripts/dev/shell-client.mjs -- '/nonesiste'     # prints "[done exit_code=0]" (discarded slash)
node scripts/dev/shell-client.mjs -- '/ai hi'       # [error routing_error]: /ai "testo" (virgolette obbligatorie) [literal Italian error string]
node scripts/dev/shell-client.mjs -- '/config'        # opens /config dialog on ui.exe
node scripts/dev/shell-client.mjs -- '/ai "list 3 largest files here"'   # [Y/n] → runs → window
```

`--config-dir`/`--session` accept the same default as other binaries (`Test Run\Configuration`, a PID-derived ID):
`node scripts/dev/shell-client.mjs --config-dir "Test Run/Configuration" --session s1 -- '/ping'`.
`--selftest` validates `exec_in_shell` locally without an orchestrator.

**Gotcha (Git Bash/MSYS)**: inside Git Bash, MSYS rewrites arguments resembling Unix absolute paths
— `/ping` turns into `C:/Program Files/Git/ping` — before Node receives it. Run the client from
**PowerShell** (as above), or disable argument conversion for that command:
`MSYS_NO_PATHCONV=1 node scripts/dev/shell-client.mjs -- '/ping'`.

## Gotchas (Windows, applicable to all build commands above)

- **`Access is denied (os error 5)` during build** = a binary is still executing. Terminate it
  before recompiling — `.\stop_lare.ps1` (default: only processes inside `Test Run\`) locates and
  kills orchestrator/ui/lare-shell/mcp-server/mcp-nmap/plugins, removing the need to remember process
  names manually:
  ```powershell
  .\stop_lare.ps1                # only processes running from Test Run\ (standard case)
  .\stop_lare.ps1 -Release        # same process names ANYWHERE ELSE on the filesystem (never inside
                                  # Test Run\) — an actual deployment, not the dev one
  ```
  For ping.exe/calc.exe (names colliding with the Windows Calculator and Windows's own network/ping
  utility), matching is always
  performed against the full path (`...\plugins\<id>\<id>.exe`), never on bare executable name — an
  unrelated system `calc.exe` is never touched. Manual equivalent if preferred:
  ```powershell
  taskkill /F /IM orchestrator.exe /IM ui.exe /IM lare-shell.exe
  ```
  (in Git Bash, forward slashes must be doubled: `taskkill //F //IM orchestrator.exe //IM ui.exe //IM lare-shell.exe`).
- **Always restart the orchestrator following a backend build** — it does not auto-reload.
- **Tauri (`ui`) does not recompile after modifications only to the frontend** (`crates/ui/frontend/`):
  `generate_context!` embeds `frontendDist` at compile time; `build.rs` includes a `rerun-if-changed`
  on the frontend folder covering most cases, but if `cargo build -p ui`/`cargo run -p ui` seems to
  ignore frontend changes, force recompilation:
  ```powershell
  cargo clean -p ui
  ```
- **A running `lare-shell.exe` causes `dotnet build` to fail** with the same "Access is denied" error
  — resolve using the same `taskkill` command above.
