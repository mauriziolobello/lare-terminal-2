# Changelog — `mcp-server`

All notable changes to this crate are documented here.
Format: [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).
Versioning: `major.minor.update` (SemVer).

## 2.0.1 — 2026-09-05 — `--config-dir` al posto di `LARE_LOCAL_DIR`/`LARE_ROUTINES_DIR`

Il crate `startup-config` è stato riscritto (Task 2 del piano "fondamenta") con l'API 2.0:
niente più `load_from_dir`/`resolve`/`default_local_dir`, né il campo `local_dir` su
`StartupConfig` — l'API v1 non esiste più, e questo crate non compilava più contro di essa.
Migrazione a riga di comando: `main()` non legge più `LARE_LOCAL_DIR`/`LARE_ROUTINES_DIR`
dall'ambiente (decisione D6 dello spec 2.0: un'unica fonte della cartella di configurazione
per ogni binario Lare, niente env var che nella v1 divergevano in silenzio fra i binari).
La cartella di configurazione è ora `--config-dir <path>` se passato sulla riga di comando,
altrimenti `<cartella dell'eseguibile>\Configuration\` (`startup_config::config_dir_from_process`).
`routines::resolve_root` cambia firma: da `(env_value, file_value, local_dir) -> PathBuf` a
`(config_dir: &Path, cfg: &StartupConfig) -> PathBuf` — legge `cfg.paths.routines_dir`
(default `Configuration/routines`) e lo risolve rispetto alla radice del deploy via
`StartupConfig::resolve_path` (assoluto → invariato; relativo → `deploy_root(config_dir)/valore`).
I 4 vecchi test di `resolve_root` (precedenza env/file/default, env vuoto ignorato) sono
sostituiti da 2 nuovi test sulla nuova firma (`resolve_root_uses_startup_routines_dir_relative_to_deploy_root`,
`resolve_root_absolute_routines_dir_is_kept`). La riga di log d'avvio non hardcoda più la
versione (era rimasta ferma a `v0.7.1` anche dopo il fork 2.0.0): ora usa
`env!("CARGO_PKG_VERSION")`, quindi non richiede più un aggiornamento manuale a ogni bump.

## 2.0.0 — 2026-09-05 — fork da v1 0.7.1

Copia del crate dalla v1 (`mauriziolobello/lare-terminal`) nel repo 2.0. Nessuna modifica
funzionale in questa voce; le modifiche del piano 1 seguono nelle voci successive.

## [0.7.1] — 2026-08-12 — `routines_root` da `startup.json` (fase 1)

`routines::resolve_root` guadagna un livello di precedenza fra
`LARE_ROUTINES_DIR` e il default: il campo `routines_dir` di
`startup.json` (nuovo file, accanto a `mcp-server.exe` — vedi
`Docs/superpowers/specs/2026-08-12-startup-config-design.md`). `local_dir`
(prima ri-derivato internamente da `default_root`, ora eliminato) è
risolto una volta in `main()` via il nuovo crate `startup-config` e passato
già pronto. Nessun cambio di comportamento per chi non ha mai avuto
`startup.json`: file assente → `Ok(None)` → stessa catena di sempre. Un
`startup.json` presente ma illeggibile/malformato ora emette un
`tracing::warn!` invece di cadere ai default in silenzio (onora il
contratto di `startup_config::load_from_dir`, che documenta esplicitamente
"il chiamante logga e cade al default").

## 0.7.0

- Nuovi tool MCP `get_routine_content`/`save_routine` (Fase 2 del repository di routine
  — Docs/superpowers/specs/2026-08-05-save-routine-design.md): `get_routine_content`
  legge corpo+metadati di una routine per nome (sola lettura); `save_routine` salva una
  routine nuova o aggiorna/rinomina una esistente (`replace`). Nuove funzioni pure in
  `routines.rs`: `validate_name` (charset sicuro, difesa path-traversal), `save_entry`,
  `get_content`, `format_date_from_unix_secs` (nessuna dipendenza chrono). 19 nuovi test.

## [0.6.3] — 2026-08-05 — `search_routines`: match per parola (OR), non più sull'intera query come sottostringa unica

`routines::search` cercava l'intera query come UNA sottostringa contigua di nome/descrizione/tag —
falliva su query multi-parola naturali (l'AI cercava "file grandi dimensione", non trovava nulla
in "Elenca i file più grandi di N MB..." perché quella frase esatta non c'è, e riscriveva il
comando da zero invece di usare `run_routine`; trovato in un test dal vivo 2026-08-05). Ora la
query è tokenizzata per spazi: match se ALMENO UNA parola (OR, non tutte) è sottostringa di nome,
descrizione o un tag. 2 nuovi test (`search_multi_word_query_matches_via_any_token`,
`..._still_empty_when_no_token_matches`); i 6 test `search_*` preesistenti restano verdi invariati
(erano tutti già query mono-parola o sottostringhe contigue esistenti).

## [0.6.2] — 2026-08-04 — `LARE_LOCAL_DIR`: override generale della base dati Local per `routines/`

Nuova variabile d'ambiente `LARE_LOCAL_DIR`: `routines::default_root` (usata da `resolve_root`
quando `LARE_ROUTINES_DIR` non è impostata) la onora ora come override a percorso completo che
sostituisce l'intera base `%LOCALAPPDATA%\dev.lare.terminal\` — non solo la sottocartella
`routines\` come fa `LARE_ROUTINES_DIR`. Pensata per casi come un profilo condiviso fra macchine
sotto una radice comune (es. `C:\Lare Terminal\local-data\`) invece della cartella utente
Windows. Semantica: path completo usato verbatim, nessun `dev.lare.terminal` unito sopra; non
impostata → comportamento invariato.

Catena di precedenza in `resolve_root` (più specifico prima): 1) `LARE_ROUTINES_DIR` (override di
questo solo path) 2) `LARE_LOCAL_DIR` (override dell'intera base) 3) `%LOCALAPPDATA%\dev.lare.terminal\`
(fallback `.lare-data` se `LOCALAPPDATA` non è impostata). `default_root` guadagna il parametro
`local_dir_override: Option<String>`, testabile senza mutare l'ambiente reale del processo (stesso
principio già in uso per `local_appdata`). 2 nuovi test:
`default_root_local_dir_override_wins_over_local_appdata`,
`default_root_ignores_empty_local_dir_override`.

## [0.6.1] — 2026-08-03 — fix sicurezza: escape apici singoli in build_invocation

`build_invocation()` (`routines.rs`) interpolava il path assoluto della routine in un
letterale PowerShell single-quoted senza escaping. Un profilo utente Windows con un
apice nel nome (`C:\Users\O'Brien\...`) produceva un'invocazione rotta; più seriamente,
quando `save_routine` (fase 2, non ancora costruita) permetterà a `entry.path` di
provenire da un `index.json` scritto dall'AI invece che curato a mano, un path non
escaped sarebbe un vettore di command injection oltre il banner di conferma (che mostra
il NOME della routine, non il path risolto). Fix: raddoppia ogni apice singolo
(`replace('\'', "''")`, escape standard PowerShell per stringhe single-quoted) prima
di interpolare. Nuovo test `build_invocation_escapes_single_quote_in_path`.

Corretti anche: due riferimenti residui a "v0.5.0" (`src/main.rs:1` doc-comment,
`:371` log di avvio — il secondo in particolare rendeva inutile lo smoke test
post-merge, che verifica la presenza di un binario NUOVO controllando la versione
loggata); intestazione "Three-level architecture" → "Five-level" in `lib.rs` (il
modulo `routines.rs` era già stato aggiunto alla lista numerata in una versione
precedente senza aggiornare il titolo).

## [0.6.0] — 2026-08-03 — repository di routine PowerShell (search_routines, run_routine)

Due nuovi tool MCP: `search_routines(query?)` (ricerca sola-lettura per nome/descrizione/tag
nel repository di routine salvate) e `run_routine(name, args?)` (esegue una routine già
presente nell'indice, delegando alla stessa shell persistente di `run_in_session` — stessa
cwd, nessun nuovo meccanismo di esecuzione — non streamma: RunRoutineParams non ha progress_token,
l'output arriva tutto insieme a fine esecuzione).

Nuovo modulo puro `routines.rs`: indice JSON su disco (`index.json` + `.ps1` per routine,
storage piatto, nessuna sottocartella per categoria), ricerca case-insensitive su
name/description/tag, costruzione dell'invocazione (`args` passato verbatim alla shell — MAI
ri-splittato a mano, bug noto del prototipo PowerShell-puro che questo tool sostituisce).
Root dati: `%LOCALAPPDATA%\dev.lare.terminal\routines\` (override `LARE_ROUTINES_DIR`),
NON `%APPDATA%\...\library\` (quella cartella resta di proprietà esclusiva del crate `ui`).

Nasce per sostituire un prototipo PowerShell-puro (`lare-lib.ps1`) costruito da una sessione
AI di Lare tramite `run_in_session` senza alcuna modifica al codice — mai agganciato al loop
tool-use reale. Design completo: `Docs/superpowers/specs/2026-08-03-routines-repository-design.md`.

18 nuovi test in `routines.rs` (path resolution, indice, ricerca, costruzione invocazione).

Deferito esplicitamente (vedi design doc §7): `save_routine` (l'AI genera e salva una nuova
routine da una richiesta NL) e l'esecuzione con privilegi di Amministratore.

## [0.5.0] — 2026-06-25

### Added — streaming output via `notifications/progress` (rmcp 1.7.0)

**Feature:** each output line of a `run_in_session` command can be streamed
to the caller in real time as a `notifications/progress` MCP notification,
instead of being buffered and returned only when the command finishes.

#### `session.rs` — streaming channel parameter

- **`Session::run`** signature extended:
  ```rust
  pub async fn run(
      &self,
      command: &str,
      progress_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
  ) -> CommandOutput
  ```
  When `progress_tx` is `Some`, every non-marker stdout line is sent to the
  channel **before** being pushed to `stdout_lines`.  When `None`, behaviour
  is unchanged (backward compatible).
- **`run_command_in_shell`** and **`read_lines_lossy_until_marker`** updated
  to accept and forward `progress_tx: Option<&UnboundedSender<String>>`.
- **All existing call sites** updated: old `session.run("cmd")` →
  `session.run("cmd", None)`; old `read_lines_lossy_until_marker(&mut r, m)` →
  `read_lines_lossy_until_marker(&mut r, m, None)`.

#### `main.rs` — `run_in_session` MCP handler rewritten

- **`RunInSessionParams`** gains `#[serde(default)] progress_token: Option<String>`.
- **`run_in_session` handler** now injects `Peer<RoleServer>` (via rmcp's
  `FromContextPart` impl on the server peer) in addition to `Parameters<...>`:
  ```rust
  async fn run_in_session(
      &self,
      peer: Peer<RoleServer>,
      Parameters(RunInSessionParams { command, progress_token }): Parameters<RunInSessionParams>,
  ) -> String
  ```
- **Streaming path** (when `progress_token` is `Some`):
  1. Creates `(drain_tx, drain_rx): UnboundedChannel<String>`.
  2. Builds `ProgressToken(NumberOrString::String(token_str.into()))`.
  3. Spawns a drain task: reads lines from `drain_rx`, calls
     `peer.notify_progress(ProgressNotificationParam::new(token, progress).with_message(line))`
     per line; `progress` is a monotonically increasing `f64` counter.
     Send errors (client disconnected) log at `debug` and stop the drain
     without aborting the command.
  4. Calls `session.run(&command, Some(drain_tx))`.
  5. Awaits the drain task so all in-flight notifications are sent before
     the final JSON result is returned.
- **Non-streaming path** (when `progress_token` is `None`): unchanged —
  `session.run(&command, None)` and returns the full JSON at once.
- New imports: `rmcp::{Peer, RoleServer}`,
  `rmcp::model::{NumberOrString, ProgressNotificationParam, ProgressToken}`.

#### TDD (RED → GREEN)

Two new unit tests written before the production changes:

| Test | RED reason | Assertion |
|------|-----------|-----------|
| `streaming_progress_tx_receives_lines` | `read_lines_lossy_until_marker` had 2 params (E0061) | given a marker-terminated buffer with 2 lines + `Some(tx)`, both lines arrive on the channel |
| `streaming_none_progress_tx_no_change` | same compilation error | given `None`, no channel send; output lines unchanged |

All 41 tests pass (0 failed).

---

## [0.4.0] — 2026-06-23

### Added — two-line marker protocol: `run_in_session` reports the shell cwd

**Feature:** after every command the shell emits a second marker line carrying its
current working directory.  `CommandOutput` and `SessionOutput` now include `cwd`.

#### `session.rs` — marker protocol changes

- **`write_command`** now emits **two** marker lines after each command:
  1. Exit-code line (unchanged, emitted first to capture `$?`/`$LASTEXITCODE`
     before any navigation side-effect):
     - Windows: `Write-Output ("{marker}" + $(if ($?) { 0 } else { ... }))`
     - Unix: `printf '{marker}%s\n' "$?"`
  2. cwd line (new, terminates the read loop):
     - Windows: `Write-Output ("{marker}D" + (Get-Location).Path)`
     - Unix: `printf '{marker}D%s\n' "$(pwd)"`
- **`read_lines_lossy_until_marker`** updated to the two-line protocol:
  - A `{marker}<digits>` line captures the exit code and **does not terminate**
    the loop (cwd line is still to come).
  - A `{marker}D<path>` line terminates the loop; `path` is the cwd.
  - Return type widened from `(Vec<String>, i32, bool)` to
    `(Vec<String>, i32, String, bool)` (added `cwd: String`).
  - EOF before the cwd line → `shell_died = true`, `cwd = ""`.
- **`CommandOutput`** — new public field `cwd: String`.
- **`read_until_marker`** propagates `cwd` from the inner helper into
  `CommandOutput`.

#### `main.rs` — `run_in_session` output

- **`SessionOutput`** — new field `cwd: String`; serialised as `"cwd"` in the
  JSON response of `run_in_session`.

#### TDD (RED → GREEN)

Two new pure-reader unit tests written before any production change:

| Test | Assertion |
|------|-----------|
| `two_line_marker_parses_exit_code_and_cwd` | given `"<out>\n{marker}0\n{marker}D/home/u\n"`, returns `exit_code=0`, `cwd="/home/u"`, `shell_died=false`, 1 output line |
| `two_line_marker_eof_before_cwd_returns_empty_cwd` | given `"output\n{marker}0\n"` (no cwd line), `shell_died=true`, `cwd=""`, exit code captured |

Extended existing integration test:

- `cd_then_pwd_shows_new_dir`: after `cd <tmp>`, a no-op command's `result.cwd`
  (lowercased) contains the temp-dir name — proves the cwd travels through the
  marker protocol end-to-end.

Existing tests updated to the new 4-tuple return / `cwd` field:

- `non_utf8_output_handled_lossy`: input extended with a cwd line; destructuring
  updated; extra assertion `cwd == "/home/u"`.

All 39 tests pass (0 failed).

## [0.3.0] — 2026-06-20

### Added — Superficie 2: `/open` (ADR-012)

- **`open_target` module (`src/open_target.rs`):**
  - `TargetKind` enum: `Url` / `Folder` / `File` / `NotFound`.
  - `classify_target(target: &str) -> TargetKind` — pure function (touches FS,
    no GUI side-effect).  Classification order: `http(s)://` prefix first
    (before any FS stat), then `is_dir()`, then `is_file()`, then `NotFound`.
    Other URL schemes (`ftp:`, `mailto:`, `file:`) → `NotFound` (ADR-012).
  - `open_target(target: &str) -> OpenResult` — classifies then calls
    `opener::open(target)` (or returns `ok:false` without launching anything
    for `NotFound`).
  - `OpenResult { ok: bool, message: String }` — serialisable via serde.
- **`open_target` MCP tool in `main.rs`:**
  - Input: `{ target: String }` via `Parameters<OpenTargetParams>`.
  - Output: JSON `{ ok: bool, message: String }`.
  - Tool description documents the classification rules.

- **Dependency: `opener = "0.8.5"`** — cross-platform native opener
  (ShellExecute on Windows, `open` on macOS, `xdg-open` on Linux).
  No shell-string interpolation → no injection risk.

### TDD cycle (RED → GREEN) — `open_target` module

Tests were written **before** `classify_target` and `open_target` were
implemented.  The module compiled (no syntax errors in the tests themselves)
but all assertion-based tests would fail with the wrong values.

| Test | Key assertion |
|------|---------------|
| `classify_http_url` | `http://` → `Url` |
| `classify_https_url` | `https://…` → `Url` |
| `classify_url_case_insensitive` | `HTTPS://` → `Url` |
| `classify_existing_folder` | temp dir → `Folder` |
| `classify_existing_file` | temp file → `File` |
| `classify_nonexistent_path_is_not_found` | invented path → `NotFound` |
| `classify_invented_path_is_not_found` | Windows path → `NotFound` |
| `classify_other_scheme_is_not_found` | `ftp:`, `mailto:`, `file:` → `NotFound` |
| `open_target_not_found_returns_ok_false_no_side_effect` | no side-effect, `ok=false` |
| `open_target_not_found_message_contains_target` | target in message |

All 9 tests pass GREEN after implementation.

## [0.2.1] — 2026-06-20

### Fixed

- **BUG-001 — non-UTF-8 output no longer kills the session (`session.rs`):**
  `AsyncBufReadExt::read_line(&mut String)` requires valid UTF-8 and returned
  `Err(InvalidData)` on the first byte ≥ 0x80 (accented character, box-drawing
  glyph, etc.) — treated as a dead shell.  Replaced with
  `read_until(b'\n', &mut Vec<u8>)` + `String::from_utf8_lossy` **in both**
  the stdout loop (`read_lines_lossy_until_marker`) and the stderr drain task
  (`spawn_shell`).  Invalid bytes are now replaced with U+FFFD instead of
  causing a fatal error.
- **UTF-8 console fidelity (Windows):** `spawn_shell` now writes
  `[Console]::OutputEncoding = [System.Text.Encoding]::UTF8` and
  `$OutputEncoding = [System.Text.Encoding]::UTF8` to PowerShell stdin
  immediately after spawning (before any user command).  These assignments are
  silent (no stdout output), so they do not interfere with the marker protocol.
  With this in place, accented characters emitted by commands are encoded in
  UTF-8 and decoded correctly, rather than appearing as U+FFFD replacement
  characters.  `chcp 65001` was evaluated but rejected: it emits "Active code
  page: 65001\n" on non-TTY pipes, corrupting marker detection.
- `spawn_shell` changed from `fn` to `async fn` to allow `.await` on the init
  write; its single caller (`Session::run`) was already async.

### Added (tests — RED → GREEN)

- `non_utf8_output_old_impl_returns_err` — **RED proof**: a local
  `read_line`-based helper receives `b"out\xFFput\nMARKER0\n"` and returns
  `Err("stream did not contain valid UTF-8")`, confirming the bug.
- `non_utf8_output_handled_lossy` — **GREEN** (unit, pure): `read_lines_lossy_until_marker`
  receives the same byte sequence, returns no error, `shell_died = false`,
  `exit_code = 0`, and the output line contains U+FFFD where `\xFF` was.
  Genuine RED also demonstrated by temporarily reverting the helper to `read_line`:
  the test fails with `shell_died=true` (the `Err` branch sets it), then passes
  with `read_until`.
- `accented_char_decoded_correctly` — **integration fidelity**: spawns a real
  PowerShell session, runs `Write-Output ([char]0xE9)`, asserts the output
  contains `é` (U+00E9) with no replacement characters.  Proves that the
  `[Console]::OutputEncoding` injection delivers correct UTF-8 output end-to-end
  and does not produce spurious output corrupting the first command.

## [0.2.0] — 2026-06-20

### Added

- **`session` module — persistent shell session (ADR-011, Superficie 1):**
  - `Session` struct: owns a long-lived shell process (PowerShell on Windows,
    `sh` on Unix) with stdin/stdout/stderr in pipes.
  - `Session::new()`: lazy construction (shell not spawned until first command).
  - `Session::run(command) -> CommandOutput`: writes command to shell stdin,
    writes a marker line, reads stdout until the marker, extracts exit code.
  - `Session::reset()`: kills the current shell; next `run` spawns a fresh one.
  - `CommandOutput { stdout, stderr, exit_code }`: stderr always empty (stderr
    stays on the server's own stderr pipe for log visibility in Fase 1).
  - **Exit-code formula (PowerShell):**
    `$(if ($?) { 0 } else { if ($LASTEXITCODE) { $LASTEXITCODE } else { 1 } })`
    — `$?` is authoritative (True iff last command succeeded), avoiding the
    `$LASTEXITCODE` stickiness trap (stale code from prior external command).
  - **Marker:** per-session nonce (`__LARE_{hex_nanos}__`).
  - Auto-respawn: if the shell dies (write error or EOF on stdout), the dead
    handle is cleared and the next `run` spawns a fresh process.
  - stderr drained by a dedicated `tokio::spawn` task that owns `ChildStderr`
    and forwards lines to an `mpsc::unbounded_channel`.  stdout is read in a
    plain `loop` with no `select!` (cancel-safe).  This prevents both the
    pipe-buffer deadlock AND the silent data-loss that `select!`-over-`read_line`
    causes when the stderr branch cancels a mid-line stdout read.
  - `unbounded_channel` (not bounded) for the stderr drain task so that `send`
    is synchronous and never blocks — a bounded channel would deadlock the drain
    task when full (>512 lines), filling the OS pipe buffer and wedging the child.
  - 13 unit tests (4 pure marker-parsing + 9 integration with real shell):
    - `marker_parses_zero`, `marker_parses_nonzero`, `marker_parses_negative_one`,
      `non_marker_line_returns_none` — pure, no I/O.
    - `echo_produces_output_and_exit_zero` — stdout + exit 0.
    - `cd_then_pwd_shows_new_dir` — **persistence proof**.
    - `failing_command_returns_nonzero_exit_code` — exit code 3 via `cmd /c exit 3`.
    - `nonexistent_command_returns_nonzero_exit_code` — cmdlet failure → exit 1.
    - `reset_spawns_fresh_shell` — cwd is reset after `Session::reset()`.
    - `respawn_after_shell_exits` — RED→GREEN: auto-respawn after natural death.
    - `large_stderr_does_not_deadlock` — RED→GREEN: 100 KB blob within 10 s.
    - `many_stderr_lines_do_not_deadlock` — 5 000 stderr lines; validates that
      unbounded channel does not deadlock even above bounded capacity threshold.
    - `interleaved_stdout_stderr_does_not_corrupt_marker` — RED→GREEN: 200
      interleaved stdout+stderr lines, all present (cancel-safety proof); also
      asserts stderr content appears in combined output.

### Changed

- **`main.rs` rewritten:** `OsCommandServer` (one tool: `run_os_command`) replaced
  by `SessionServer` (two tools: `run_in_session`, `reset_session`).
  - `run_in_session(command)`: delegates to `Session::run`; returns
    `{ stdout, stderr, exit_code }` JSON.
  - `reset_session()`: delegates to `Session::reset`; returns `{"reset":true}`.
  - Session shared via `Arc<Mutex<Session>>` across clones (required by rmcp).

### Dependencies

- Added `process`, `io-util`, `sync` features to `tokio` (required by `session.rs`).

## [0.1.1] — 2026-06-20

### Security

- **cwd hardening — Fase 1 (anti-UNC/NTLM):** `run_command` now validates the
  `cwd` argument via a new pure function `validate_cwd` before passing it to
  `Command::current_dir`.  On Windows, a UNC path (`\\attacker\share`) as `cwd`
  causes the shell to authenticate against the remote SMB server, leaking the
  process owner's NTLM hash.  Validation rules (applied in security-critical
  order):
  1. Reject paths starting with `\\` or `//` (UNC / device / verbatim) — checked
     **first** because `Path::is_dir()` on a UNC path itself triggers SMB auth.
  2. Reject non-absolute paths (prevents relative directory traversal).
  3. Reject paths that are not an existing local directory.
  If validation fails, the command is **not executed**; `exit_code = -1` and a
  descriptive message are returned in `stderr`.
  `cwd: None` is unchanged (no validation, inherits process cwd).
  **TODO (Fase 2):** root-allowlist enforcement.

## [0.1.0] — 2026-06-20

### Added
- `system` module: pure, testable OS command runner with no MCP/tokio dependency.
  - `CommandOutput` struct: `stdout: String`, `stderr: String`, `exit_code: i32`.
  - `run_command(command, cwd?) -> CommandOutput`: executes via `cmd /C` (Windows)
    or `sh -c` (Unix); exit code convention `-1` for signal-terminated processes.
  - 4 inline unit tests (RED→GREEN→REFACTOR cycle):
    - stdout capture (`echo hello` → contains "hello", exit 0)
    - non-zero exit code (`exit 3` → exit_code == 3)
    - stderr capture (`echo oops 1>&2` → stderr non-empty)
    - cwd respected (temp dir → output contains unique dir name)
- `main.rs` MCP stdio glue (`OsCommandServer`):
  - Exposes the `run_os_command` tool via `rmcp` 1.7.0 on stdio transport.
  - Input: `{ command: string, cwd?: string }` (matches Contratto B).
  - Output: JSON string `{ stdout, stderr, exit_code }`.
  - All logs routed to stderr; stdout reserved for MCP JSON-RPC wire.
- Added `crates/mcp-server` to workspace `Cargo.toml` members.
- `CHANGELOG.md` and `IMPLEMENTATION.md`.
