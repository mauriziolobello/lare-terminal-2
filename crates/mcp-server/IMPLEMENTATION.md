# Implementation — `mcp-server` v0.7.1

## `routines_root` da `startup.json` (v0.7.1)

`routines::resolve_root` non ri-deriva più `%LOCALAPPDATA%\dev.lare.terminal\`
per conto proprio (funzione `default_root` eliminata) — riceve `local_dir`
già risolto da `main()`. Nuova firma:
`resolve_root(env_value: Option<&str>, file_value: Option<&str>, local_dir: &Path) -> PathBuf`,
thin wrapper su `startup_config::resolve(env_value, file_value, || local_dir.join("routines"))`.

`SessionServer::new` prende ora `routines_root: PathBuf` già risolto invece
di rileggere `LARE_ROUTINES_DIR` internamente — `main()` fa la risoluzione
una sola volta (`exe_dir()` → `load_from_dir()` → `resolve()` per
`local_dir` → `resolve_root()` per la root finale) e la passa giù.

I 6 test `resolve_root_explicit_override_wins`/`default_root_*` sono stati
sostituiti da 4 nuovi test sulla nuova firma a 3 argomenti — copertura
equivalente su ciò che questo crate può testare senza toccare l'ambiente
reale (env vince, file vince su default, default = `local_dir.join
("routines")`, env vuoto ignorato). La logica `%LOCALAPPDATA%`/`.lare-data`
non è più testata QUI: si è spostata in `startup-config::default_local_dir`
(crate `startup-config`, v0.1.0), che però — come `exe_dir()` — è
intenzionalmente SENZA unit test diretti (wrapper sottile sul confine col
sistema operativo, stesso pattern già in uso nel progetto per
`McpToolClient::resolve()`/`NmapToolClient::resolve()`; vedi
`startup-config/IMPLEMENTATION.md`). `resolve()` (il dispatcher a 3 livelli
usato da `resolve_root`) resta invece pienamente testato in
`startup-config` stesso.

`main()` non scarta più in silenzio un `startup.json` illeggibile o JSON
malformato: `load_from_dir` restituisce `Err`, e `main()` lo logga
(`tracing::warn!`) prima di cadere ai default — nessun bug-fantasma "il
file c'è ma non succede niente".

## Scope

This crate implements Contratto B (`Docs/03-protocol.md`): an MCP server on
**stdio transport** that exposes seven tools:
- `run_in_session` / `reset_session` — persistent shell session management.
- `open_target` — native app opener (ADR-012, Superficie 2).
- `search_routines` / `run_routine` / `get_routine_content` / `save_routine` — read-only search + execution + read/write of saved
  PowerShell routines (v0.6.0–0.7.0, see `routines.rs` below). `run_routine`
  delegates to the same persistent shell as `run_in_session` — no new
  execution mechanism. `get_routine_content` reads full script bodies (read-only,
  companion to `save_routine`). `save_routine` writes new routines or updates
  existing ones (replace mode with rename support) — writes are gated at the
  orchestrator layer before dispatch.

It is the only crate in the workspace that executes OS commands or opens
native applications.  It has no WebSocket logic (that lives in the orchestrator).

---

## Five-level architecture (SRP)

### 1. `src/system.rs` — legacy pure core (kept for reference)

One-shot command runner (`cmd /C` / `sh -c`) from v0.1.x.  No longer exposed
as an MCP tool; retained for testing utility.

### 2. `src/session.rs` — persistent shell session

**Single responsibility:** own a long-lived shell process and expose
`run(command, progress_tx) -> CommandOutput`.

```rust
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub cwd: String,   // shell cwd after command; "" if shell died before cwd marker
}

impl Session {
    pub fn new() -> Self { ... }           // lazy — no shell spawned yet
    pub async fn run(
        &self,
        command: &str,
        progress_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
    ) -> CommandOutput { ... }
    pub async fn reset(&self) { ... }      // kill + drop; next run() respawns
}
```

**Streaming (v0.5.0):** when `progress_tx` is `Some`, every non-marker stdout
line is sent to the channel immediately as it is read from the shell pipe —
before being pushed to `stdout_lines`.  When `None`, behaviour is identical to
v0.4.0 (backward compatible).  The channel is `tokio::sync::mpsc::UnboundedSender`
so the send is synchronous and never blocks the read loop.

**What it does NOT do:** no MCP, no rmcp, no serde.  Only `tokio::process` +
`tokio::io`.

### 3. `src/open_target.rs` — native app opener (ADR-012)

**Single responsibility:** classify a target string and open it with the OS
default application.

```rust
pub enum TargetKind { Url, Folder, File, NotFound }
pub struct OpenResult { pub ok: bool, pub message: String }

pub fn classify_target(target: &str) -> TargetKind { ... }
pub fn open_target(target: &str) -> OpenResult { ... }
```

Classification order (URL check before FS stat):
1. `http://` / `https://` prefix → `Url`
2. `is_dir()` → `Folder`
3. `is_file()` → `File`
4. otherwise → `NotFound`

Other URL schemes (`ftp:`, `mailto:`, `file:`) → `NotFound` (ADR-012).
`opener::open()` is called only for `Url`/`Folder`/`File` — `NotFound` returns
immediately with `ok=false`, no side-effect.

**What it does NOT do:** no shell, no MCP, no async.

### 4. `src/routines.rs` (v0.6.0)

Repository di script PowerShell riusabili — indice JSON su disco, ricerca, costruzione
invocazione. Nessuna dipendenza da MCP/rmcp (testabile in isolamento con `tempdir`).

| Funzione | Comportamento |
|---|---|
| `resolve_root(env_override)` | Catena di precedenza: `LARE_ROUTINES_DIR` (override di questo solo path) → `LARE_LOCAL_DIR` (override dell'intera base `dev.lare.terminal`, v0.6.2, path completo verbatim) → `%LOCALAPPDATA%\dev.lare.terminal\routines`, fallback `.lare-data\routines` |
| `load_index(root)` | Legge `index.json`; file assente O malformato → indice vuoto, mai errore |
| `find_by_name(idx, name)` | Match esatto sul nome |
| `search(idx, query)` | `query` assente/vuota → tutte le entry; altrimenti tokenizzata per spazi, match OR-per-parola su name/description/tags (v0.6.3 — prima l'intera query doveva essere UNA sottostringa contigua, falliva su query multi-parola naturali) |
| `build_invocation(entry, root, args)` | `& '<path assoluto>' <args verbatim>` — nessun path di default iniettato, `args` mai ri-splittato |

Storage piatto: `routines/index.json` + `routines/<name>.ps1`, nessuna sottocartella per
categoria (`category` è solo un campo dell'indice).

**Tool MCP** (`main.rs`): `search_routines(query?)` sola lettura, mai gateizzato;
`run_routine(name, args?)` delega a `Session::run` (stessa shell persistente di
`run_in_session`), sempre gateizzato anche in locale (`orchestrator`, `SENSITIVE_TOOLS`).

### 5. `src/main.rs` — thin MCP stdio glue

**Single responsibility:** wire `session` and `open_target` modules to the MCP protocol.

Exposes seven tools via `#[tool_router]` + `rmcp` 1.7.0:
- `run_in_session(command, [progress_token])` → JSON `{ stdout, stderr, exit_code, cwd }`
- `reset_session()` → JSON `{"reset":true}`
- `open_target(target: String)` → JSON `{ ok: bool, message: String }` (ADR-012)
- `search_routines(query?)` → JSON `{ results: [{ name, description, category, tags }, ...] }` (v0.6.0, see `routines.rs` above)
- `run_routine(name, args?)` → JSON `{ ok, message, stdout, stderr, exit_code, cwd }` (v0.6.0)
- `get_routine_content(name)` → JSON `{ found, name, description, category, tags, content, created }` (v0.7.0, read-only companion to `save_routine` — the AI reads full script bodies before deciding to reuse/update/create distinct)
- `save_routine(name, description, tags, category, content, [replace])` → JSON `{ ok, message }` (v0.7.0, delegates to `routines::save_entry` — writes are already gated at orchestrator layer by `ToolConfirmer::confirm_routine_save` before dispatch)

#### Streaming in `run_in_session` (v0.5.0)

The handler injects `Peer<RoleServer>` via rmcp's `FromContextPart` impl
(no manual wiring needed — the framework extracts it from the handler context).

```rust
async fn run_in_session(
    &self,
    peer: Peer<RoleServer>,
    Parameters(RunInSessionParams { command, progress_token }): Parameters<RunInSessionParams>,
) -> String
```

`RunInSessionParams.progress_token: Option<String>` — when `Some`:

1. Creates `(drain_tx, drain_rx): UnboundedChannel<String>`.
2. Builds `ProgressToken(NumberOrString::String(token_str.into()))`.
3. Spawns drain task: reads lines from `drain_rx`; calls
   `peer.notify_progress(ProgressNotificationParam::new(token, progress).with_message(line))`
   per line (`progress` is a monotonically increasing `f64`).
   Send errors (client disconnected) log at `debug` and stop the drain.
4. Calls `session.run(&command, Some(drain_tx))` — lines flow into the
   drain task as they arrive from the shell pipe.
5. Awaits drain task — ensures all in-flight notifications are flushed
   before the final JSON result is returned.

When `progress_token` is `None` (default `#[serde(default)]`), the non-streaming
path calls `session.run(&command, None)` and returns the full JSON at once
(backward compatible).

---

## Shell invocation

| Platform | Command |
|----------|---------|
| Windows  | `powershell -NoLogo -NoProfile -NonInteractive -Command -` |
| Unix     | `sh` |

`-NoLogo -NoProfile -NonInteractive` suppress the banner, user profile, and
interactive prompts.  `-Command -` means "read commands from stdin".  With
stdin pipe-redirected (non-TTY), PowerShell does NOT emit `PS>` prompts to stdout.

---

## Marker protocol (two-line, v0.4.0)

After every user command, the shell emits **two** marker lines on stdout:

**Line 1 — exit-code line (emitted first, unchanged):**
```powershell
# PowerShell
Write-Output ("{marker}" + $(if ($?) { 0 } else { if ($LASTEXITCODE) { $LASTEXITCODE } else { 1 } }))
```
```sh
# Unix sh
printf '{marker}%s\n' "$?"
```

**Line 2 — cwd line (new, terminates the read loop):**
```powershell
# PowerShell
Write-Output ("{marker}D" + (Get-Location).Path)
```
```sh
# Unix sh
printf '{marker}D%s\n' "$(pwd)"
```

The reader accumulates output lines until it sees the cwd line (`{marker}D<path>`).
A `{marker}<digits>` line captures the exit code but does **not** terminate
the loop (the cwd line is still to come).  EOF before the cwd line means the
shell died: `shell_died = true`, `cwd = ""`.

The exit-code line is emitted **first** so that `$?`/`$LASTEXITCODE` are
captured before `Get-Location`/`pwd` can touch them (those commands always
succeed, which would overwrite `$?`).

### Exit-code formula (PowerShell — important)

`$?` is authoritative: it is `True` iff the last command (native cmdlet or
external program) succeeded.  We check `$?` first, not `$LASTEXITCODE`.

Why: `$LASTEXITCODE` is set **only** by external (native) programs and is
**sticky** — it keeps its value across subsequent PowerShell-native commands.
A naive `$LASTEXITCODE`-first formula would report a stale nonzero code from
a prior external command for every subsequent successful `echo` or `cd`.

| Scenario | `$?` | `$LASTEXITCODE` | Effective exit code |
|----------|------|-----------------|---------------------|
| Successful cmdlet | True | (stale) | **0** |
| Failed cmdlet (CommandNotFound) | False | null | **1** |
| External cmd exit 0 | True | 0 | **0** |
| External cmd exit 3 | False | 3 | **3** |
| Successful cmdlet after external exit 3 | True | 3 (stale) | **0** ✓ |

---

## Non-UTF-8 output handling (BUG-001 fix, v0.2.1)

`AsyncBufReadExt::read_line(&mut String)` requires valid UTF-8 and returned
`Err(InvalidData)` at the first byte ≥ 0x80.  On Windows, PowerShell emits
output in the console code page (OEM/ANSI, e.g. CP850/CP1252) unless the
session is explicitly configured otherwise.

**Fix:** both the stdout loop and the stderr drain task now use
`read_until(b'\n', &mut Vec<u8>)` + `String::from_utf8_lossy`.  Invalid bytes
are replaced with U+FFFD (Unicode replacement character) instead of causing a
fatal error.  The marker is pure ASCII, so `strip_prefix` on the lossy string
continues to work correctly.

**UTF-8 fidelity (Windows):** `spawn_shell` now writes two PowerShell
assignments to stdin before the first user command:

```powershell
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$OutputEncoding = [System.Text.Encoding]::UTF8
```

These assignments are silent (no stdout output) and ensure accented characters
are emitted in UTF-8, decoded correctly (not as U+FFFD).  `chcp 65001` was
considered but rejected: it prints "Active code page: 65001\n" on non-TTY
pipes, which corrupts the marker detection loop.

`spawn_shell` is now `async fn` (was `fn`) to allow `.await` on the init write.

---

## stderr handling

stderr is captured on a separate pipe and drained by a **dedicated background
`tokio::spawn` task** that owns `ChildStderr` exclusively and forwards lines
(one per `read_until`) to an `mpsc::unbounded_channel`.  `read_until_marker`
reads stdout in a plain `loop` (no `select!`) and collects whatever stderr
lines have arrived after the marker is found via a `try_recv` drain (preceded
by a single `tokio::task::yield_now()` to let the drain task flush in-flight
lines).

This design is correct for three reasons:

1. **Deadlock prevention (OS pipe buffer)**: the drain task runs continuously,
   so the OS stderr pipe buffer never fills.  A command emitting 100 KB+ to
   stderr (even a single blob, even thousands of lines) completes normally.

2. **Deadlock prevention (channel overflow)**: the channel is **unbounded** so
   the drain task's `send` is synchronous and never blocks.  A bounded channel
   would fill at 512 lines, block the drain task, fill the OS pipe buffer, and
   wedge the child — exactly the original deadlock with a higher threshold.

3. **Cancel safety**: `AsyncBufReadExt::read_line` is explicitly *not*
   cancel-safe.  Using it in a `tokio::select!` over two pipes causes partially-
   read stdout lines to be silently lost when the stderr branch wins — corrupting
   command output.  The dedicated-task approach avoids this: stdout is read in
   a plain sequential loop, and the drain task owns stderr exclusively.

Combined newline-terminated stderr lines are appended to stdout output and
returned in `CommandOutput.stdout`.  `CommandOutput.stderr` is always empty.

**Capture accuracy note**: `read_until` only completes on `\n`, so a single
continuous blob with no newlines (e.g. `'x' * 100000`) produces no channel
sends — it is drained from the OS pipe buffer but not captured in the output.
For newline-terminated stderr (the common case), content is captured.

**Interleave order**: stderr lines are collected after the marker, so they
appear after stdout rather than interleaved by arrival time.  Acceptable for a
non-TTY session (Fase 1 limitation; TODO Fase 2: per-line interleaving).

---

## Auto-respawn

If the shell dies naturally (e.g. the user types `exit`), `run_command_in_shell`
detects the death via a write error (broken pipe) or EOF on stdout.  It returns
`shell_died = true`, and `Session::run` sets `*guard = None` so the next call
spawns a fresh process.  The previous session state (cwd, env) is lost — this
is a documented Fase 1 limitation.

`is_alive` was **removed**: the old version only checked `guard.is_some()` and
could not detect a dead process.  Death detection now happens via I/O error
signals inside `run_command_in_shell`.

---

## Session serialisation

All commands run **sequentially** (one at a time) because `Session::run` holds
`Arc<Mutex<Option<ShellHandles>>>` for the duration of each command.  This
matches the shell's natural constraint: a persistent shell can only run one
command at a time.

---

## MCP server in main.rs

`SessionServer` holds `Arc<Mutex<Session>>` so it can be `Clone`-d (required by
rmcp's `ServerHandler` trait) while keeping a single live `Session`.

```
run_in_session(command) → Session::run(command) → write to shell stdin → read until marker
reset_session()         → Session::reset()      → *guard = None
```

---

## Known limitations (Fase 1)

- **No PTY**: no colours, no ANSI codes, no terminal-dependent programs (vim, REPL).
- **Stdin-reading commands**: any command that reads stdin (e.g. `Read-Host`,
  bare `python`, `more`) or leaves an unclosed `{`/quote will swallow the marker
  line and hang the session.
- **`exit N` kills the session** (it terminates the shell process).  The session
  auto-respawns, but the nonzero code is lost.  Use `cmd /c exit N` (Windows)
  or `sh -c 'exit N'` (Unix) to get an exit code without killing the session.
- **stderr interleave order**: stderr may arrive out of order relative to stdout.

---

## Test coverage

| Test | Type | Key behavior |
|------|------|-------------|
| `marker_parses_zero` | unit (pure) | marker parsing, exit 0 |
| `marker_parses_nonzero` | unit (pure) | marker parsing, exit 3 |
| `marker_parses_negative_one` | unit (pure) | marker parsing, exit -1 |
| `non_marker_line_returns_none` | unit (pure) | non-marker lines ignored |
| `two_line_marker_parses_exit_code_and_cwd` | unit (pure) | **two-line protocol**: exit+cwd parsed; 1 output line |
| `two_line_marker_eof_before_cwd_returns_empty_cwd` | unit (pure) | **EOF before cwd**: `shell_died=true`, `cwd=""` |
| `non_utf8_output_old_impl_returns_err` | unit (pure) | **RED proof**: `read_line` fails on `\xFF` with "invalid UTF-8" |
| `non_utf8_output_handled_lossy` | unit (pure) | **GREEN (BUG-001)**: `read_until`+lossy handles `\xFF`; cwd extracted |
| `accented_char_decoded_correctly` | integration | **UTF-8 fidelity**: `é` (U+00E9) decoded correctly; no U+FFFD |
| `echo_produces_output_and_exit_zero` | integration | stdout + exit 0 |
| `cd_then_pwd_shows_new_dir` | integration | **persistence proof + cwd in result**: `result.cwd` contains tmp dir name |
| `failing_command_returns_nonzero_exit_code` | integration | external exit 3 |
| `nonexistent_command_returns_nonzero_exit_code` | integration | cmdlet failure |
| `reset_spawns_fresh_shell` | integration | reset clears cwd |
| `respawn_after_shell_exits` | integration | **auto-respawn after natural death** |
| `large_stderr_does_not_deadlock` | integration | **stderr OS-pipe deadlock prevented** |
| `many_stderr_lines_do_not_deadlock` | integration | **channel-overflow deadlock prevented (unbounded)** |
| `interleaved_stdout_stderr_does_not_corrupt_marker` | integration | **cancel-safe read; all 200 stdout + some stderr captured** |
| `streaming_progress_tx_receives_lines` | unit (pure) | **streaming (v0.5.0)**: `Some(tx)` → both non-marker lines sent to channel |
| `streaming_none_progress_tx_no_change` | unit (pure) | **streaming (v0.5.0)**: `None` → no channel send; output unchanged |

RED → GREEN demonstrated genuinely for:
- `respawn_after_shell_exits`: failed with "pipe is closing (os error 232)" before fix
- `large_stderr_does_not_deadlock`: timed out after 10 s before fix
- `interleaved_stdout_stderr_does_not_corrupt_marker`: stdout lines silently dropped when `select!`-over-`read_line` cancelled mid-line
- `many_stderr_lines_do_not_deadlock`: bounded mpsc(512) blocks drain task at line 513 → deadlock; fixed by `unbounded_channel`
- `non_utf8_output_old_impl_returns_err` / `non_utf8_output_handled_lossy` (BUG-001):
  `read_line` returns `Err("stream did not contain valid UTF-8")` on `b"\xFF"` (RED);
  `read_until`+`from_utf8_lossy` handles it as U+FFFD without error (GREEN).
  Genuine RED also confirmed by temporarily reverting `read_lines_lossy_until_marker`
  to `read_line`: `non_utf8_output_handled_lossy` panics with `shell_died=true`.
- `accented_char_decoded_correctly`: verifies the `OutputEncoding` init injection
  causes `Write-Output ([char]0xE9)` to produce `é` (U+00E9) without U+FFFD

---

## Dependencies

| Crate | Role | Scope |
|-------|------|-------|
| `rmcp` 1.7.0 | MCP SDK: server, stdio transport, macros | Runtime |
| `tokio` 1.x | Async runtime + process + io-util + sync | Runtime |
| `serde` + `serde_json` | DTO serialisation in the glue layer | Runtime |
| `schemars` (via `rmcp::schemars` 1.2.1) | JsonSchema for `Parameters<T>` | Runtime (re-export) |
| `opener` 0.8.5 | Cross-platform native app opener (ShellExecute/open/xdg-open) | Runtime |
| `anyhow` | Error handling in `main` | Runtime |
| `tracing` + `tracing-subscriber` | Structured logging to stderr | Runtime |
| `tempfile` 3 | Auto-cleaned temp dirs in session + open_target tests | Dev only |

---

## How to test

```sh
# All unit + integration tests (includes real shell tests)
cargo test -p mcp-server

# Linting (zero warnings)
cargo clippy -p mcp-server --all-targets -- -D warnings

# Format check
cargo fmt --all -- --check
```

---

## How to run (stdio)

```sh
# Launch the server — speaks MCP JSON-RPC on stdin/stdout
cargo run -p mcp-server

# Or after building:
.\target\debug\mcp-server.exe   # Windows
./target/debug/mcp-server       # Unix

# Log verbosity (goes to stderr):
$env:RUST_LOG = "debug"; cargo run -p mcp-server
```

The orchestrator spawns this binary as a child process and communicates over
its stdin/stdout (standard MCP stdio transport, per ADR-001).  The connection
is persistent: the orchestrator reuses the same process for the entire session.
