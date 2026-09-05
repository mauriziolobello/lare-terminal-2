//! # session — persistent shell session
//!
//! Maintains a single long-lived shell process (PowerShell on Windows, sh on Unix)
//! with stdin/stdout/stderr in pipe, using a marker-based protocol to delimit
//! command output and capture exit codes.
//!
//! ## Architecture (SRP)
//!
//! This module owns the shell subprocess and exposes one method: `run`.
//! It has **no dependency** on MCP, rmcp, serde, or any protocol type.
//! The MCP glue in `main.rs` calls `Session::run` and serialises the result.
//!
//! ## Marker protocol
//!
//! After writing the user command to stdin, we write **two** marker lines to
//! stdout.  The reader accumulates command output until the second (cwd) line
//! is found:
//!
//! 1. `{marker}<exit_code>` — exit-code line (emitted first, before `pwd`/
//!    `Get-Location` can affect `$?`).
//! 2. `{marker}D<cwd>` — cwd line; the reader terminates on this line and
//!    extracts the current working directory from the suffix after `{marker}D`.
//!
//! ### Exit-code formula (PowerShell)
//!
//! ```powershell
//! Write-Output ("{marker}" + $(if ($?) { 0 } else { if ($LASTEXITCODE) { $LASTEXITCODE } else { 1 } }))
//! ```
//!
//! Rationale: `$?` is True iff the last command (native *or* cmdlet) succeeded.
//! If it is False we prefer `$LASTEXITCODE` (set by external/native commands)
//! over the fallback `1` (used for cmdlet/PowerShell failures which do not set
//! `$LASTEXITCODE`).  This avoids the **stickiness trap**: `$LASTEXITCODE` from
//! a prior external command does NOT leak into a subsequent succeeding cmdlet.
//!
//! ### Unix (sh)
//!
//! ```sh
//! printf '<marker>%s\n' "$?"
//! ```
//!
//! `$?` in POSIX sh is always the exit code of the last command (both built-in
//! and external), so no special formula is needed.
//!
//! ## stderr handling
//!
//! stderr is captured on a **separate pipe** and read concurrently with stdout
//! via two tokio tasks draining into the same `mpsc` channel.  This avoids
//! pipe-buffer deadlock (blocked stdout reader + full stderr buffer).  The marker
//! appears only on stdout; the reader task signals the end-of-command boundary.
//!
//! Combined output (stdout + stderr interleaved in arrival order) is returned in
//! `CommandOutput.stdout`; `CommandOutput.stderr` is always empty.
//!
//! Trade-off: interleave ordering is best-effort — lines from the two streams
//! may arrive reordered if the OS schedules the two reader threads differently.
//! This is acceptable for a non-TTY session.
//!
//! ## Shell invocation
//!
//! | Platform | Command |
//! |----------|---------|
//! | Windows  | `powershell -NoLogo -NoProfile -NonInteractive -Command -` |
//! | Unix     | `sh` |
//!
//! `-NoLogo -NoProfile -NonInteractive` suppresses the banner, user profile,
//! and interactive prompts.  `-Command -` means "read commands from stdin".
//! With stdin redirected (non-TTY), PowerShell does not emit `PS>` prompts.
//!
//! ## Limitations (Fase 1)
//!
//! - No PTY: no colours, no terminal-dependent programs (vim, REPL), no ANSI.
//! - A user command that itself reads stdin, or leaves an unclosed `{`/quote,
//!   will swallow the marker line and hang.  Documented limitation.
//! - `exit N` kills the session (it kills the shell process). The session
//!   auto-respawns on the next command.  Users who want a nonzero exit code
//!   from a test should use `cmd /c exit N` (Windows) or `sh -c 'exit N'` (Unix).

use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::{mpsc, Mutex};

/// Output captured from a completed session command.
#[derive(Debug, Clone, PartialEq)]
pub struct CommandOutput {
    /// Combined stdout + stderr output.
    /// (stderr lines are appended after stdout; `stderr` field is always empty.)
    pub stdout: String,
    /// Always empty — stderr is merged into `stdout`.
    pub stderr: String,
    /// Process exit code as reported by the shell marker.
    /// `-1` on spawn failure or session error.
    pub exit_code: i32,
    /// Current working directory of the shell after the command completed.
    /// Extracted from the two-line marker protocol (`{marker}D<path>`).
    /// Empty string if the shell died before emitting the cwd marker line.
    pub cwd: String,
}

// ── Internal state ─────────────────────────────────────────────────────────────

/// The live shell handles (stdin writer + stdout reader + stderr drain channel).
///
/// stderr is owned by a dedicated `tokio::spawn` task that reads it in a tight
/// loop and sends lines to `stderr_rx`.  This prevents pipe-buffer deadlock
/// without requiring `select!` over `read_line` (which is not cancel-safe).
struct ShellHandles {
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    /// Receives stderr lines from the background drain task.
    /// Unbounded so the drain task's `send` never blocks — bounded send would
    /// deadlock when the channel fills faster than the receiver drains it.
    stderr_rx: mpsc::UnboundedReceiver<String>,
    /// We keep the `Child` alive so the process isn't killed on drop.
    _child: Child,
}

/// A persistent shell session.
///
/// One session = one shell process, kept alive across `run` calls.
/// Serialises commands via an internal `Mutex` (one at a time).
/// Auto-respawns if the shell process dies (write error or EOF on read).
pub struct Session {
    /// Per-session marker nonce — unique across runs of the same process.
    marker: String,
    /// The live shell handles (None = not yet spawned or dead, lazily spawned).
    inner: Arc<Mutex<Option<ShellHandles>>>,
}

impl Session {
    /// Create a new session.  The shell is **not** spawned yet (lazy).
    pub fn new() -> Self {
        // Use a fixed-format nonce from the process start time.
        let nonce = format!(
            "{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos()
        );
        let marker = format!("__LARE_{nonce}__");
        Self {
            marker,
            inner: Arc::new(Mutex::new(None)),
        }
    }

    /// Run a command in the persistent session.
    ///
    /// Spawns the shell on first call (or after a crash/exit).
    /// Sends the command, reads output until the marker, returns the result.
    /// If the shell dies during the command, the dead handle is cleared so
    /// the next `run` call auto-respawns.
    ///
    /// `progress_tx`: when `Some`, each output line is forwarded to this channel
    /// before being accumulated in the final result.  When `None`, behaviour is
    /// identical to the previous single-argument API.
    pub async fn run(
        &self,
        command: &str,
        progress_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
    ) -> CommandOutput {
        let mut guard = self.inner.lock().await;

        // Ensure the shell is alive; spawn/respawn if needed.
        if guard.is_none() {
            match spawn_shell().await {
                Ok(handles) => {
                    *guard = Some(handles);
                }
                Err(_e) => {
                    return CommandOutput {
                        stdout: String::new(),
                        stderr: String::new(),
                        exit_code: -1,
                        cwd: String::new(),
                    };
                }
            }
        }

        let handles = guard.as_mut().expect("just spawned");
        // `shell_died` is true when we hit a write error or EOF on stdout.
        // Clear the guard so the next call respawns.
        let (output, shell_died) =
            run_command_in_shell(handles, command, &self.marker, progress_tx.as_ref()).await;
        if shell_died {
            *guard = None;
        }
        output
    }

    /// Kill and drop the current shell.  The next `run` call will respawn.
    pub async fn reset(&self) {
        let mut guard = self.inner.lock().await;
        *guard = None;
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────────

/// Spawn a shell child process and write the UTF-8 encoding setup.
///
/// stderr is immediately handed off to a background `tokio::spawn` task that
/// reads it in a tight loop and forwards lines to `stderr_rx` via an `mpsc`
/// channel.  This prevents pipe-buffer deadlock: the child can always write to
/// stderr regardless of whether the parent is currently reading stdout.
///
/// The drain task terminates when the child's stderr pipe closes (EOF), which
/// happens when the child process exits.  If the `ShellHandles` are dropped
/// (session reset/respawn), `kill_on_drop` terminates the child and the pipe
/// closes, causing the task to exit cleanly.
///
/// ## UTF-8 encoding setup (Windows)
///
/// PowerShell inherits the console code page (OEM/ANSI, e.g. CP850/CP1252)
/// from the process environment.  Without explicit setup, accented characters
/// emitted by commands are encoded in the OEM code page and would appear as
/// U+FFFD replacement characters even after the lossy-read fix.
///
/// We write two assignments to stdin immediately after spawning:
/// - `[Console]::OutputEncoding = [System.Text.Encoding]::UTF8`
/// - `$OutputEncoding = [System.Text.Encoding]::UTF8`
///
/// PowerShell property assignments are **silent** (no stdout output), so they
/// cannot interfere with the marker protocol or produce spurious output.
///
/// `chcp 65001` was considered but avoided: it targets the console window
/// (irrelevant in a non-TTY pipe context) and emits "Active code page: 65001\n"
/// which would corrupt the first command's output.
async fn spawn_shell() -> Result<ShellHandles, std::io::Error> {
    use tokio::process::Command;

    #[cfg(windows)]
    let mut cmd = {
        let mut c = Command::new("powershell");
        c.args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", "-"]);
        c
    };
    #[cfg(unix)]
    let mut cmd = Command::new("sh");

    cmd.stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    let mut child = cmd.spawn()?;
    // `mut` is required on Windows where we call stdin.write_all() below;
    // on Unix that cfg-block is absent so the compiler warns. Suppress it.
    #[allow(unused_mut)]
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| std::io::Error::other("failed to get stdin"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("failed to get stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| std::io::Error::other("failed to get stderr"))?;

    // Write UTF-8 encoding setup to PowerShell stdin before any user command.
    // On Unix, sh uses the locale (UTF-8 by default on modern systems); no setup needed.
    #[cfg(windows)]
    {
        stdin
            .write_all(
                b"[Console]::OutputEncoding = [System.Text.Encoding]::UTF8\n\
                  $OutputEncoding = [System.Text.Encoding]::UTF8\n",
            )
            .await?;
    }

    // Spawn the stderr drain task.  It owns `ChildStderr` exclusively, so
    // no `'static` bound conflict, and no `select!` cancel-safety issue.
    //
    // IMPORTANT: use `unbounded_channel` so that `send` is synchronous and
    // never blocks.  A bounded channel would cause the drain task to block
    // when full, filling the OS pipe buffer, blocking the child process on
    // its stderr writes, preventing it from emitting the stdout marker →
    // deadlock.  Memory is bounded by the command's stderr volume, same as
    // stdout is already accepted.
    let (stderr_tx, stderr_rx) = mpsc::unbounded_channel::<String>();
    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr);
        loop {
            let mut buf: Vec<u8> = Vec::new();
            match reader.read_until(b'\n', &mut buf).await {
                Ok(0) | Err(_) => break, // EOF or error → child died, exit task
                Ok(_) => {
                    // Convert bytes → String lossy, same as stdout (BUG-001).
                    // `send` on unbounded channel is infallible (returns Err
                    // only when receiver dropped).  Ignore: the receiver may
                    // be dropped on session reset while command runs.
                    let line = String::from_utf8_lossy(&buf).into_owned();
                    let _ = stderr_tx.send(line);
                }
            }
        }
    });

    Ok(ShellHandles {
        stdin,
        stdout: BufReader::new(stdout),
        stderr_rx,
        _child: child,
    })
}

/// Send a command + marker line to the shell, read output until marker.
///
/// `progress_tx`: when `Some`, each output line is forwarded to the channel
/// before being accumulated (see `read_lines_lossy_until_marker`).
///
/// Returns `(output, shell_died)`.  `shell_died` is `true` when a write error
/// or stdout EOF is encountered, signalling that the caller should clear the
/// shell handle so the next `run` auto-respawns.
async fn run_command_in_shell(
    handles: &mut ShellHandles,
    command: &str,
    marker: &str,
    progress_tx: Option<&tokio::sync::mpsc::UnboundedSender<String>>,
) -> (CommandOutput, bool) {
    // Write command + marker extraction lines to stdin.
    let write_result = write_command(&mut handles.stdin, command, marker).await;
    if let Err(e) = write_result {
        // Pipe closed → shell is dead.
        return (
            CommandOutput {
                stdout: format!("session write error: {e}"),
                stderr: String::new(),
                exit_code: -1,
                cwd: String::new(),
            },
            true, // shell_died
        );
    }

    // Read stdout in a plain loop until the marker is found.
    // stderr is drained concurrently by the background task spawned in
    // `spawn_shell`; we collect whatever arrived after the marker.
    read_until_marker(&mut handles.stdout, &mut handles.stderr_rx, marker, progress_tx).await
}

/// Write the command + marker-extraction line to the shell's stdin.
async fn write_command(
    stdin: &mut ChildStdin,
    command: &str,
    marker: &str,
) -> Result<(), std::io::Error> {
    stdin.write_all(command.as_bytes()).await?;
    stdin.write_all(b"\n").await?;

    #[cfg(windows)]
    {
        // Exit-code line (FIRST — captures $?/$LASTEXITCODE before Get-Location runs).
        // $? is True iff last command succeeded (native cmdlet or external).
        // If False, prefer $LASTEXITCODE (external cmd), else 1 (cmdlet fail).
        // See module doc for the stickiness-trap rationale.
        let exit_marker_cmd = format!(
            "Write-Output (\"{marker}\" + $(if ($?) {{ 0 }} else {{ if ($LASTEXITCODE) {{ $LASTEXITCODE }} else {{ 1 }} }}))\n"
        );
        stdin.write_all(exit_marker_cmd.as_bytes()).await?;
        // cwd line (SECOND — Get-Location does not affect $?/$LASTEXITCODE).
        let cwd_marker_cmd =
            format!("Write-Output (\"{marker}D\" + (Get-Location).Path)\n");
        stdin.write_all(cwd_marker_cmd.as_bytes()).await?;
    }
    #[cfg(unix)]
    {
        // Exit-code line (FIRST — captures $? before pwd runs).
        let exit_marker_cmd = format!("printf '{marker}%s\\n' \"$?\"\n");
        stdin.write_all(exit_marker_cmd.as_bytes()).await?;
        // cwd line (SECOND).
        let cwd_marker_cmd = format!("printf '{marker}D%s\\n' \"$(pwd)\"\n");
        stdin.write_all(cwd_marker_cmd.as_bytes()).await?;
    }

    stdin.flush().await?;
    Ok(())
}

/// Read lines (byte-by-byte, lossy UTF-8) from `reader` using the two-line marker protocol.
///
/// This helper is the testable core of the stdout-reading loop.  It accepts
/// any `impl AsyncBufRead + Unpin` so that unit tests can pass an in-memory
/// `&[u8]` cursor instead of a real `ChildStdout`.
///
/// ## Two-line marker protocol
///
/// After each command the shell emits two marker lines on stdout:
/// 1. `{marker}<digits>\n`  — exit-code line (first, so `$?` is captured before
///    `Get-Location`/`pwd` can touch it).
/// 2. `{marker}D<path>\n`   — cwd line (terminates the read loop).
///
/// The reader accumulates output lines until it sees the cwd line.
/// A `{marker}<digits>` line is NOT treated as EOF — it captures the exit code
/// and the loop continues waiting for the cwd line.
/// EOF or I/O error before the cwd line → `shell_died = true`, `cwd = ""`.
///
/// ## Streaming
///
/// When `progress_tx` is `Some(tx)`, each non-marker output line is forwarded
/// to the channel via `tx.send(line.clone())` BEFORE being pushed to
/// `stdout_lines`.  This lets callers observe output in real time rather than
/// waiting for the full command to complete.
///
/// The channel is unbounded and `send` is synchronous — failures (receiver
/// dropped) are silently ignored so the session never blocks or fails due to a
/// dropped subscriber.
///
/// ## Why `read_until` instead of `read_line`
///
/// `AsyncBufReadExt::read_line` requires the stream to be valid UTF-8; it
/// returns `Err(InvalidData)` at the first byte ≥ 0x80.  On Windows,
/// PowerShell emits output in the console code page (OEM/ANSI, e.g. CP850
/// or CP1252) unless explicitly configured otherwise.  Any accented character
/// or box-drawing character in command output caused the session to die.
///
/// `read_until(b'\n', &mut Vec<u8>)` accumulates raw bytes up to `\n`, then
/// `String::from_utf8_lossy` converts them — replacing invalid sequences with
/// the Unicode replacement character (U+FFFD) instead of returning an error.
/// The marker is pure ASCII, so `strip_prefix` on the lossy string works
/// identically to before.
///
/// ## Returns
///
/// `(stdout_lines, exit_code, cwd, shell_died)`
/// - `stdout_lines`: output lines before the marker lines, each with trailing `\n`.
/// - `exit_code`: parsed from the exit-code marker; `-1` on parse failure or death.
/// - `cwd`: path extracted from the cwd marker; `""` if shell died before it.
/// - `shell_died`: `true` on EOF or I/O error before the cwd marker was found.
async fn read_lines_lossy_until_marker<R>(
    reader: &mut R,
    marker: &str,
    progress_tx: Option<&mpsc::UnboundedSender<String>>,
) -> (Vec<String>, i32, String, bool)
where
    R: tokio::io::AsyncBufRead + Unpin,
{
    let mut stdout_lines: Vec<String> = Vec::new();
    let mut exit_code: i32 = -1;
    let mut cwd = String::new();
    let mut shell_died = false;

    // The cwd marker prefix is `{marker}D`.
    let cwd_marker = format!("{marker}D");

    loop {
        let mut buf: Vec<u8> = Vec::new();
        match reader.read_until(b'\n', &mut buf).await {
            Ok(0) => {
                // EOF before the cwd marker: shell died.
                shell_died = true;
                break;
            }
            Ok(_) => {
                // Convert bytes → String, replacing invalid UTF-8 sequences
                // with U+FFFD (lossy).  This prevents crashes on non-UTF-8
                // console output (BUG-001).
                let line = String::from_utf8_lossy(&buf).into_owned();
                let trimmed = line.trim_end_matches(['\r', '\n']);

                if let Some(path_part) = trimmed.strip_prefix(&cwd_marker) {
                    // cwd line: this is the end-of-command terminator.
                    cwd = path_part.to_owned();
                    break;
                } else if let Some(code_part) = trimmed.strip_prefix(marker) {
                    // Exit-code line: capture exit code and continue reading
                    // (the cwd line is still to come).
                    match code_part.trim().parse::<i32>() {
                        Ok(n) => exit_code = n,
                        Err(_) => {
                            tracing::warn!(
                                "session: failed to parse exit code from marker: {:?}",
                                code_part
                            );
                            exit_code = -1;
                        }
                    }
                    // Do NOT break here — wait for the cwd line.
                } else {
                    // Non-marker output line: forward to streaming channel if present.
                    // `send` on an unbounded channel is infallible except when receiver
                    // dropped; ignore that case so the session never blocks.
                    if let Some(tx) = progress_tx {
                        let _ = tx.send(line.clone());
                    }
                    stdout_lines.push(line);
                }
            }
            Err(e) => {
                tracing::error!("session: stdout read error: {e}");
                shell_died = true;
                break;
            }
        }
    }

    (stdout_lines, exit_code, cwd, shell_died)
}

/// Read stdout lines until the marker is found.
///
/// `progress_tx`: when `Some`, each non-marker line is forwarded to the
/// channel before being accumulated (see `read_lines_lossy_until_marker`).
///
/// stderr lines are collected from `stderr_rx` (the background drain task
/// channel) using a brief non-blocking drain after the marker is found.
///
/// # Cancel safety
///
/// `read_until` is used in a plain `loop` with no `select!`, so there is no
/// cancel-safety hazard: each iteration completes a full line or hits EOF.
///
/// # Returns
///
/// `(output, shell_died)` — `shell_died` is `true` on EOF or I/O error.
async fn read_until_marker(
    stdout: &mut BufReader<ChildStdout>,
    stderr_rx: &mut mpsc::UnboundedReceiver<String>,
    marker: &str,
    progress_tx: Option<&mpsc::UnboundedSender<String>>,
) -> (CommandOutput, bool) {
    let (stdout_lines, exit_code, cwd, shell_died) =
        read_lines_lossy_until_marker(stdout, marker, progress_tx).await;

    // Collect any stderr lines that arrived during stdout reading.
    // `try_recv` is non-blocking: drain what's already in the channel buffer
    // without waiting for new data.  A brief yield lets the drain task flush
    // any lines that were in flight when the marker was found.
    tokio::task::yield_now().await;
    let mut stderr_lines: Vec<String> = Vec::new();
    while let Ok(line) = stderr_rx.try_recv() {
        stderr_lines.push(line);
    }

    // Combine stdout + stderr.  Stderr appended after stdout; interleave
    // order is best-effort for a non-TTY session.
    let mut combined = stdout_lines.join("");
    if !stderr_lines.is_empty() {
        combined.push_str(&stderr_lines.join(""));
    }

    (
        CommandOutput {
            stdout: combined,
            stderr: String::new(),
            exit_code,
            cwd,
        },
        shell_died,
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests — TDD: RED then GREEN
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Marker parsing: pure logic, no I/O ───────────────────────────────────

    /// Parse a marker line and extract the exit code.
    /// This mirrors the logic in `read_until_marker` for isolated testing.
    fn parse_marker_line(line: &str, marker: &str) -> Option<i32> {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        trimmed.strip_prefix(marker)?.trim().parse::<i32>().ok()
    }

    #[test]
    fn marker_parses_zero() {
        let marker = "__LARE_test__";
        assert_eq!(parse_marker_line("__LARE_test__0\n", marker), Some(0));
    }

    #[test]
    fn marker_parses_nonzero() {
        let marker = "__LARE_test__";
        assert_eq!(parse_marker_line("__LARE_test__3\r\n", marker), Some(3));
    }

    #[test]
    fn marker_parses_negative_one() {
        let marker = "__LARE_test__";
        assert_eq!(parse_marker_line("__LARE_test__-1\n", marker), Some(-1));
    }

    #[test]
    fn non_marker_line_returns_none() {
        let marker = "__LARE_test__";
        assert_eq!(parse_marker_line("just output\n", marker), None);
        assert_eq!(parse_marker_line("", marker), None);
        assert_eq!(parse_marker_line("__LARE_other__0\n", marker), None);
    }

    // ── BUG-001: non-UTF-8 output handled lossy (no session death) ─────────────

    /// Helper that demonstrates the RED case: using `read_line` on a stream
    /// that contains non-UTF-8 bytes returns `Err(InvalidData)`.
    ///
    /// This function mirrors the OLD behaviour of `read_lines_lossy_until_marker`
    /// (before BUG-001 fix) and is kept here **only to document the failure mode**.
    /// It is NOT called from production code.
    async fn read_line_based_old_impl(bytes: &[u8], marker: &str) -> Result<Vec<String>, String> {
        use tokio::io::AsyncBufReadExt as _;
        let mut reader = tokio::io::BufReader::new(bytes);
        let mut lines: Vec<String> = Vec::new();
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line).await {
                Ok(0) => break,
                Ok(_) => {
                    let trimmed = line.trim_end_matches(['\r', '\n']);
                    if trimmed.strip_prefix(marker).is_some() {
                        break;
                    }
                    lines.push(line);
                }
                Err(e) => return Err(e.to_string()), // ← BUG: "stream did not contain valid UTF-8"
            }
        }
        Ok(lines)
    }

    /// RED proof — `read_line` fails on non-UTF-8 bytes.
    ///
    /// This test verifies that the OLD `read_line`-based implementation (now
    /// only in `read_line_based_old_impl` above) returns an error when the
    /// input contains the byte `0xFF` (invalid UTF-8).  This is the exact
    /// symptom of BUG-001: `stream did not contain valid UTF-8`.
    #[tokio::test]
    async fn non_utf8_output_old_impl_returns_err() {
        // b"out\xFFput\n" — the 0xFF byte is invalid UTF-8.
        // b"__LARE_test__0\n" — marker line that would terminate the loop.
        let marker = "__LARE_test__";
        let input: &[u8] = b"out\xFFput\n__LARE_test__0\n";

        let result = read_line_based_old_impl(input, marker).await;
        assert!(
            result.is_err(),
            "RED: expected read_line to fail on non-UTF-8 input, but it returned Ok; \
             this means the test environment is unexpectedly lenient — RED proof is invalid"
        );
        let err = result.unwrap_err();
        assert!(
            err.contains("UTF-8") || err.contains("utf-8") || err.contains("invalid"),
            "RED: expected UTF-8 error, got: {err:?}"
        );
    }

    /// GREEN — `read_lines_lossy_until_marker` handles non-UTF-8 bytes without error.
    ///
    /// This is BUG-001 fixed: the helper uses `read_until` + `from_utf8_lossy`
    /// instead of `read_line`, so invalid bytes are replaced with U+FFFD (the
    /// Unicode replacement character) and the session continues normally.
    ///
    /// Input stream: one non-UTF-8 line followed by the two-line marker.
    /// Expected:     no error, output contains the lossy-decoded line, exit_code = 0.
    #[tokio::test]
    async fn non_utf8_output_handled_lossy() {
        let marker = "__LARE_test__";
        // b"out\xFFput\n" — the 0xFF byte (invalid UTF-8) must NOT kill the session.
        // Two-line marker: exit-code line first, then cwd line as terminator.
        let input: &[u8] =
            b"out\xFFput\n__LARE_test__0\n__LARE_test__D/home/u\n";
        let mut reader = tokio::io::BufReader::new(input);

        let (lines, exit_code, cwd, shell_died) =
            read_lines_lossy_until_marker(&mut reader, marker, None).await;

        assert!(
            !shell_died,
            "GREEN: shell_died must be false; got shell_died=true (stream treated as dead)"
        );
        assert_eq!(
            exit_code, 0,
            "GREEN: exit_code must be 0 (from marker); got {exit_code}"
        );
        assert_eq!(cwd, "/home/u", "GREEN: cwd must be '/home/u'");
        assert_eq!(
            lines.len(),
            1,
            "GREEN: expected exactly 1 output line; got {lines:?}"
        );
        // The 0xFF byte must be replaced with the Unicode replacement character.
        assert!(
            lines[0].contains('\u{FFFD}'),
            "GREEN: expected U+FFFD replacement char in lossy output; got: {:?}",
            lines[0]
        );
        assert!(
            lines[0].contains("out") && lines[0].contains("put"),
            "GREEN: surrounding ASCII bytes must be preserved; got: {:?}",
            lines[0]
        );
    }

    // ── Session: integration tests (require real shell) ───────────────────────

    /// echo command produces the expected output with exit code 0.
    #[tokio::test]
    async fn echo_produces_output_and_exit_zero() {
        let session = Session::new();

        #[cfg(windows)]
        let result = session.run("Write-Output 'hello-lare'", None).await;
        #[cfg(unix)]
        let result = session.run("echo 'hello-lare'", None).await;

        assert!(
            result.stdout.contains("hello-lare"),
            "expected 'hello-lare' in stdout, got: {:?}",
            result.stdout
        );
        assert_eq!(
            result.exit_code, 0,
            "expected exit_code 0, got: {}",
            result.exit_code
        );
    }

    // ── Two-line marker: pure parser tests (RED → GREEN) ────────────────────────

    /// RED → GREEN: `read_lines_lossy_until_marker` with the new two-line marker format.
    ///
    /// Buffer format:
    ///   "<output line>\n"
    ///   "{marker}0\n"        ← exit-code line (first marker line)
    ///   "{marker}D/home/u\n" ← cwd line (second marker line, terminates the loop)
    ///
    /// Expected: lines = ["<out>\n"], exit_code = 0, cwd = "/home/u", shell_died = false.
    #[tokio::test]
    async fn two_line_marker_parses_exit_code_and_cwd() {
        let marker = "__LARE_test__";
        // exit-code line first, then cwd line — cwd line is the terminator.
        let input: &[u8] = b"<out>\n__LARE_test__0\n__LARE_test__D/home/u\n";
        let mut reader = tokio::io::BufReader::new(input);

        let (lines, exit_code, cwd, shell_died) =
            read_lines_lossy_until_marker(&mut reader, marker, None).await;

        assert!(!shell_died, "shell_died must be false");
        assert_eq!(exit_code, 0, "exit_code must be 0");
        assert_eq!(cwd, "/home/u", "cwd must be '/home/u'");
        assert_eq!(lines.len(), 1, "must have exactly 1 output line");
        assert_eq!(lines[0], "<out>\n");
    }

    /// When the shell dies (EOF) before the cwd marker, cwd is "".
    #[tokio::test]
    async fn two_line_marker_eof_before_cwd_returns_empty_cwd() {
        let marker = "__LARE_test__";
        // exit-code marker found, then EOF (no cwd line) → shell_died = true, cwd = "".
        let input: &[u8] = b"output\n__LARE_test__0\n";
        let mut reader = tokio::io::BufReader::new(input);

        let (lines, exit_code, cwd, shell_died) =
            read_lines_lossy_until_marker(&mut reader, marker, None).await;

        assert!(shell_died, "shell_died must be true when EOF before cwd line");
        assert_eq!(exit_code, 0, "exit_code captured before EOF");
        assert_eq!(cwd, "", "cwd must be empty string on EOF");
        assert_eq!(lines.len(), 1);
    }

    /// THE persistence test: cd to a temp dir, then read cwd — must reflect the change.
    #[tokio::test]
    async fn cd_then_pwd_shows_new_dir() {
        let session = Session::new();
        let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
        let tmp_str = tmp.path().to_str().expect("non-UTF-8 temp path");

        // Unique name of the temp dir for assertion (lowercase for case tolerance).
        let unique_name = tmp
            .path()
            .file_name()
            .expect("temp dir has no file_name")
            .to_string_lossy()
            .to_lowercase();

        // Step 1: cd to temp dir.
        #[cfg(windows)]
        let cd_cmd = format!("cd \"{}\"", tmp_str);
        #[cfg(unix)]
        let cd_cmd = format!("cd '{}'", tmp_str);

        let cd_result = session.run(&cd_cmd, None).await;
        assert_eq!(
            cd_result.exit_code, 0,
            "cd to temp dir failed with exit_code {}: {:?}",
            cd_result.exit_code, cd_result.stdout
        );

        // Step 2: run a no-op to capture cwd via the marker — MUST show the temp dir.
        // (We use a no-op echo so we don't need a separate pwd command;
        //  the marker protocol always reports cwd after every command.)
        #[cfg(windows)]
        let noop_cmd = "Write-Output ''";
        #[cfg(unix)]
        let noop_cmd = "true";

        let noop_result = session.run(noop_cmd, None).await;
        assert!(
            noop_result.cwd.to_lowercase().contains(&unique_name),
            "expected result.cwd to contain {:?} after cd, got: {:?}",
            unique_name,
            noop_result.cwd
        );
        assert_eq!(
            noop_result.exit_code, 0,
            "noop returned non-zero exit code: {}",
            noop_result.exit_code
        );

        // Step 3 (compat): also verify stdout via an explicit pwd command still works.
        #[cfg(windows)]
        let pwd_cmd = "(Get-Location).Path";
        #[cfg(unix)]
        let pwd_cmd = "pwd";

        let pwd_result = session.run(pwd_cmd, None).await;
        assert!(
            pwd_result.stdout.to_lowercase().contains(&unique_name),
            "expected pwd output to contain {:?}, got: {:?}",
            unique_name,
            pwd_result.stdout
        );
        assert_eq!(
            pwd_result.exit_code, 0,
            "pwd returned non-zero exit code: {}",
            pwd_result.exit_code
        );
    }

    /// A failing external command (exit code 3) must return exit_code 3.
    /// Note: `exit 3` kills the session; we use `cmd /c exit 3` (Windows)
    /// or `sh -c 'exit 3'` (Unix) to get the exit code without killing the session.
    #[tokio::test]
    async fn failing_command_returns_nonzero_exit_code() {
        let session = Session::new();

        #[cfg(windows)]
        let result = session.run("cmd /c exit 3", None).await;
        #[cfg(unix)]
        let result = session.run("sh -c 'exit 3'", None).await;

        assert_eq!(
            result.exit_code, 3,
            "expected exit_code 3, got: {}; stdout: {:?}",
            result.exit_code, result.stdout
        );
    }

    /// A cmdlet/command that does not exist returns a non-zero exit code.
    #[tokio::test]
    async fn nonexistent_command_returns_nonzero_exit_code() {
        let session = Session::new();

        #[cfg(windows)]
        let result = session.run("Get-LareCommandThatDoesNotExist 2>&1", None).await;
        #[cfg(unix)]
        let result = session
            .run("lare_command_that_does_not_exist 2>&1", None)
            .await;

        assert_ne!(
            result.exit_code, 0,
            "expected non-zero exit_code, got 0; stdout: {:?}",
            result.stdout
        );
    }

    // ── Streaming: progress_tx ───────────────────────────────────────────────

    /// RED → GREEN: `read_lines_lossy_until_marker` sends non-marker lines to
    /// `progress_tx` when `Some(tx)` is provided.
    ///
    /// Input: two non-marker lines + two-line marker protocol.
    /// Expected: both non-marker lines arrive on the channel, in order.
    #[tokio::test]
    async fn streaming_progress_tx_receives_lines() {
        let marker = "__LARE_test__";
        // Two non-marker lines, then exit-code line, then cwd line.
        let input: &[u8] =
            b"line-one\nline-two\n__LARE_test__0\n__LARE_test__D/tmp\n";
        let mut reader = tokio::io::BufReader::new(input);

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();

        let (lines, exit_code, cwd, shell_died) =
            read_lines_lossy_until_marker(&mut reader, marker, Some(&tx)).await;

        // Functional result is correct.
        assert!(!shell_died);
        assert_eq!(exit_code, 0);
        assert_eq!(cwd, "/tmp");
        assert_eq!(lines.len(), 2);

        // Channel received both lines.
        drop(tx); // close so try_recv can drain fully
        let mut received: Vec<String> = Vec::new();
        while let Ok(line) = rx.try_recv() {
            received.push(line);
        }
        assert_eq!(received.len(), 2, "expected 2 lines on channel, got: {received:?}");
        assert!(received[0].contains("line-one"), "first line: {:?}", received[0]);
        assert!(received[1].contains("line-two"), "second line: {:?}", received[1]);
    }

    /// When `progress_tx` is `None`, behaviour is identical to before (no-op).
    #[tokio::test]
    async fn streaming_none_progress_tx_no_change() {
        let marker = "__LARE_test__";
        let input: &[u8] = b"out\n__LARE_test__0\n__LARE_test__D/x\n";
        let mut reader = tokio::io::BufReader::new(input);

        let (lines, exit_code, _cwd, shell_died) =
            read_lines_lossy_until_marker(&mut reader, marker, None).await;

        assert!(!shell_died);
        assert_eq!(exit_code, 0);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("out"));
    }

    // ── Blocker regression tests (RED → GREEN) ───────────────────────────────

    /// Blocker 2 — auto-respawn on natural shell death.
    ///
    /// `exit` kills the PowerShell session process.  The NEXT `run` call must
    /// succeed (auto-respawn), not hang or return -1 forever.
    ///
    /// Without the fix: `is_alive` only checks `guard.is_some()`, so the dead
    /// handle stays `Some(dead)` and every subsequent read hits EOF → exit_code -1.
    #[tokio::test]
    async fn respawn_after_shell_exits() {
        let session = Session::new();

        // First command — establish the session.
        #[cfg(windows)]
        session.run("Write-Output 'before-exit'", None).await;
        #[cfg(unix)]
        session.run("echo 'before-exit'", None).await;

        // Kill the shell process by running `exit`.
        // We swallow the result — it will be -1 (EOF), that's expected.
        session.run("exit", None).await;

        // The NEXT call must auto-respawn and succeed.
        #[cfg(windows)]
        let result = session.run("Write-Output 'after-respawn'", None).await;
        #[cfg(unix)]
        let result = session.run("echo 'after-respawn'", None).await;

        assert!(
            result.stdout.contains("after-respawn"),
            "expected 'after-respawn' in stdout after auto-respawn, got: {:?}",
            result.stdout
        );
        assert_eq!(
            result.exit_code, 0,
            "expected exit_code 0 after auto-respawn, got: {}",
            result.exit_code
        );
    }

    /// Blocker 1 — large stderr does not deadlock the session.
    ///
    /// If stderr is piped but never drained, a command that emits > ~64 KB
    /// to stderr will fill the OS pipe buffer and block indefinitely.
    ///
    /// We write 100 000 characters to stderr and assert the command completes
    /// within a timeout.  Without the fix this test times out.
    #[tokio::test]
    async fn large_stderr_does_not_deadlock() {
        use tokio::time::{timeout, Duration};

        let session = Session::new();

        // Write 100 000 characters to stderr.
        // PowerShell: [Console]::Error.Write('x' * 100000)
        // Unix sh:    python3 -c "import sys; sys.stderr.write('x'*100000)"
        //            (or: dd if=/dev/zero bs=100000 count=1 status=none >&2)
        #[cfg(windows)]
        let command = "[Console]::Error.Write('x' * 100000)";
        #[cfg(unix)]
        let command = "dd if=/dev/zero bs=100000 count=1 status=none >&2";

        let result = timeout(Duration::from_secs(10), session.run(command, None))
            .await
            .expect("large stderr output deadlocked the session (timed out after 10 s)");

        // The command itself succeeds (exit 0).
        assert_eq!(
            result.exit_code, 0,
            "expected exit_code 0 after large-stderr command, got: {}",
            result.exit_code
        );
    }

    /// Blocker C — stderr channel overflow does not deadlock the session.
    ///
    /// If the mpsc channel used to forward stderr lines is bounded and fills up
    /// before the command completes, the drain task blocks on `send().await`,
    /// the OS stderr pipe fills, and the child blocks — wedging the session.
    ///
    /// This test emits 5 000 stderr-only lines (no stdout) and asserts the
    /// command completes within a timeout.  With a bounded channel this hangs;
    /// with `unbounded_channel` it completes immediately.
    #[tokio::test]
    async fn many_stderr_lines_do_not_deadlock() {
        use tokio::time::{timeout, Duration};

        let session = Session::new();

        // Emit 5 000 stderr lines (no stdout).  With a bounded channel of 512,
        // the drain task blocks at line 513 → deadlock.
        #[cfg(windows)]
        let command = "1..5000 | ForEach-Object { [Console]::Error.WriteLine(\"e$_\") }";
        #[cfg(unix)]
        let command = "for i in $(seq 1 5000); do echo \"e$i\" >&2; done";

        let result = timeout(Duration::from_secs(15), session.run(command, None))
            .await
            .expect("5000 stderr lines deadlocked the session (timed out after 15 s)");

        assert_eq!(
            result.exit_code, 0,
            "expected exit_code 0 after 5000-stderr-line command, got: {}",
            result.exit_code
        );
    }

    /// Blocker A — interleaved stdout + stderr does not corrupt the marker.
    ///
    /// If stdout `read_line` is cancelled mid-line (when stderr branch wins a
    /// `select!`), partially-consumed bytes are lost from the BufReader internal
    /// buffer.  If those bytes were part of the marker, the marker is never
    /// matched and the loop hangs with the session mutex held.
    ///
    /// This test emits 200 lines of stdout interleaved with 200 lines of stderr
    /// and asserts all stdout lines are present and exit_code is 0.
    #[tokio::test]
    async fn interleaved_stdout_stderr_does_not_corrupt_marker() {
        use tokio::time::{timeout, Duration};

        let session = Session::new();

        // Emit 200 stdout lines interleaved with 200 stderr lines.
        // On Windows: write to both streams in a tight loop.
        #[cfg(windows)]
        let command = r#"
1..200 | ForEach-Object {
    Write-Output "out$_"
    [Console]::Error.WriteLine("err$_")
}
"#;
        #[cfg(unix)]
        let command = r#"
for i in $(seq 1 200); do
    echo "out$i"
    echo "err$i" >&2
done
"#;

        let result = timeout(Duration::from_secs(15), session.run(command, None))
            .await
            .expect("interleaved stdout/stderr deadlocked the session (timed out after 15 s)");

        assert_eq!(
            result.exit_code, 0,
            "expected exit_code 0, got: {}; stdout: {:?}",
            result.exit_code, result.stdout
        );
        // All 200 stdout lines must be present.
        for i in 1..=200 {
            let expected = format!("out{i}");
            assert!(
                result.stdout.contains(&expected),
                "stdout missing line {:?}; full stdout: {:?}",
                expected,
                result.stdout
            );
        }
        // At least some stderr lines must appear (best-effort: the drain task
        // uses a single yield_now + try_recv so not all 200 are guaranteed, but
        // if stderr content is captured at all they should appear).
        // This assertion validates the "combined output" doc claim.
        assert!(
            result.stdout.contains("err"),
            "expected some stderr lines (errN) to appear in combined output, got: {:?}",
            result.stdout
        );
    }

    /// UTF-8 fidelity — accented characters emitted by the shell are decoded
    /// correctly (not as U+FFFD) when the session sets OutputEncoding to UTF-8.
    ///
    /// This test exercises the `[Console]::OutputEncoding` injection in
    /// `spawn_shell` end-to-end: without it, PowerShell would emit `é`
    /// (U+00E9) in the OEM code page (e.g. CP1252 byte 0xE9), which after
    /// lossy conversion would appear as U+FFFD instead of `é`.
    ///
    /// Also verifies that the two silent setup assignments do NOT produce any
    /// spurious output that would corrupt the first command's marker read.
    #[tokio::test]
    async fn accented_char_decoded_correctly() {
        let session = Session::new();

        // Write-Output emits the character é (U+00E9).
        // [char]0xE9 in PowerShell is Unicode code point 0xE9 = é.
        #[cfg(windows)]
        let result = session.run("Write-Output ([char]0xE9)", None).await;
        // On Unix the shell already runs in UTF-8; print the char directly.
        #[cfg(unix)]
        let result = session.run("printf '\\xC3\\xA9\\n'", None).await; // UTF-8 encoding of U+00E9

        assert_eq!(
            result.exit_code, 0,
            "expected exit_code 0; got {}; stdout: {:?}",
            result.exit_code, result.stdout
        );
        assert!(
            result.stdout.contains('\u{E9}'),
            "expected 'é' (U+00E9) in stdout — OutputEncoding may not be UTF-8; \
             got: {:?} (if all U+FFFD, the init injection may have failed)",
            result.stdout
        );
        // Confirm no replacement characters (would indicate OEM encoding leak).
        assert!(
            !result.stdout.contains('\u{FFFD}'),
            "found U+FFFD in stdout — accented char not decoded as UTF-8; \
             stdout: {:?}",
            result.stdout
        );
    }

    /// After reset(), the session spawns a fresh shell (new cwd).
    #[tokio::test]
    async fn reset_spawns_fresh_shell() {
        let session = Session::new();
        let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
        let tmp_str = tmp.path().to_str().expect("non-UTF-8 temp path");

        // cd to temp dir.
        #[cfg(windows)]
        let cd_cmd = format!("cd \"{}\"", tmp_str);
        #[cfg(unix)]
        let cd_cmd = format!("cd '{}'", tmp_str);
        session.run(&cd_cmd, None).await;

        // Reset the session.
        session.reset().await;

        // After reset, cwd should be the default (not the temp dir).
        #[cfg(windows)]
        let pwd_cmd = "(Get-Location).Path";
        #[cfg(unix)]
        let pwd_cmd = "pwd";

        let unique_name = tmp
            .path()
            .file_name()
            .expect("no file_name")
            .to_string_lossy()
            .to_lowercase();

        let pwd_result = session.run(pwd_cmd, None).await;
        assert!(
            !pwd_result.stdout.to_lowercase().contains(&unique_name),
            "expected pwd to NOT contain temp dir after reset, got: {:?}",
            pwd_result.stdout
        );
    }
}
