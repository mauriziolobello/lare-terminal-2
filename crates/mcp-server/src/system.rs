//! # system — pure OS command runner
//!
//! Executes a command string via the platform shell and captures output.
//!
//! ## SRP boundary
//! This module has **one responsibility**: run a command and return its output.
//! It has **zero dependencies** on rmcp, tokio, serde, or any MCP type.
//! All callers (tests, MCP glue) depend on this module — not the reverse.
//!
//! ## Shell dispatch (compile-time)
//! - Windows: `cmd /C <command>`
//! - Unix:    `sh -c <command>`
//!
//! This matches the shell the user would invoke interactively, so paths,
//! environment variables, and built-ins (like `echo`, `dir`, `cd`) work as
//! expected on each platform.
//!
//! ## Exit code convention
//! - Normal termination: the process exit code (e.g. 0 = success, non-zero = error).
//! - Signal-terminated (Unix only, no exit code available): **-1**.
//!   Documented here; callers should treat -1 as "terminated abnormally".
//!
//! ## Security (Fase 1 — cwd hardening, anti-UNC/NTLM)
//! `run_command` validates the `cwd` argument before passing it to the OS.
//! The validation rules (applied **in order**) are:
//!
//! 1. **Reject UNC / device / verbatim paths** — any path whose trimmed form
//!    starts with `\\` or `//`.  This is checked *first* because on Windows
//!    a UNC path such as `\\attacker\share` passed to `Command::current_dir`
//!    causes the shell to authenticate to the remote SMB server, leaking the
//!    process owner's NTLM hash.  Crucially, `Path::is_dir()` on a UNC path
//!    *itself* touches the network, so the UNC check must precede the fs check.
//! 2. **Reject non-absolute paths** — prevents directory traversal relative to
//!    an unpredictable process cwd.
//! 3. **Reject paths that are not an existing directory** — prevents commands
//!    from being silently run in the wrong directory on spawn failure.
//!
//! This is **Fase 1** defence (syntactic + local-fs guards).
//! **TODO (Fase 2):** implement a root-allowlist and a confirmation step for
//! dangerous commands before this server is exposed to untrusted input.

use std::path::Path;
use std::process::Command;

/// Output captured from a completed OS command.
#[derive(Debug, Clone, PartialEq)]
pub struct CommandOutput {
    /// Everything written to stdout by the command.
    pub stdout: String,
    /// Everything written to stderr by the command.
    pub stderr: String,
    /// Process exit code.
    /// Convention: `-1` when the process was terminated by a signal
    /// (Unix only) and no exit code is available.
    pub exit_code: i32,
}

/// Validates a working-directory path before it is passed to the OS.
///
/// # Security (Fase 1 — anti-UNC/NTLM)
/// Rules are applied **in this exact order** for security reasons:
///
/// 1. **Reject UNC / device / verbatim paths** — any path (after trim) that
///    starts with `\\` or `//`.  This check is first because `Path::is_dir()`
///    on a UNC path touches the network and would itself leak NTLM credentials.
/// 2. **Reject non-absolute paths** — relative paths are context-dependent and
///    allow directory traversal via the caller's cwd.
/// 3. **Reject non-existent or non-directory paths** — ensures the shell
///    actually lands in the intended place and does not silently inherit cwd.
///
/// Returns `Ok(())` if the path passes all checks, or `Err(message)` with a
/// distinct, human-readable reason for each rejection case.
///
/// **TODO (Fase 2):** enforce a root-allowlist so only pre-approved subtrees
/// are reachable, even via absolute local paths.
fn validate_cwd(cwd: &str) -> Result<(), String> {
    let trimmed = cwd.trim();

    // ── Rule 1: reject UNC / device / verbatim (check BEFORE is_dir!) ────────
    // A UNC path starts with \\ or //. Windows device paths (\\.\, \\?\) are
    // also covered. This must come first: Path::is_dir() on a UNC path causes
    // SMB negotiation and leaks the NTLM hash before we even see the result.
    if trimmed.starts_with(r"\\") || trimmed.starts_with("//") {
        return Err(format!(
            "cwd rejected: UNC / device / verbatim paths are not allowed: {cwd:?}"
        ));
    }

    // ── Rule 2: reject non-absolute paths ────────────────────────────────────
    // Relative paths are resolved against the calling process's cwd, which is
    // unpredictable and enables directory-traversal attacks.
    if !Path::new(trimmed).is_absolute() {
        return Err(format!("cwd rejected: path must be absolute: {cwd:?}"));
    }

    // ── Rule 3: reject paths that are not an existing directory ──────────────
    // This check is safe only after Rule 1 has confirmed the path is not UNC.
    if !Path::new(trimmed).is_dir() {
        return Err(format!(
            "cwd rejected: path does not exist or is not a directory: {cwd:?}"
        ));
    }

    Ok(())
}

/// Runs `command` through the platform shell, optionally inside `cwd`.
///
/// # Platform shell
/// - **Windows:** `cmd /C <command>`
/// - **Unix:**    `sh -c <command>`
///
/// # Arguments
/// - `command` — the command string to pass to the shell.
/// - `cwd`     — optional working directory; `None` means inherit the current
///   directory of the calling process.  When `Some`, the path is validated by
///   [`validate_cwd`] (Fase 1 anti-UNC/NTLM guard) before being passed to the
///   OS.  If validation fails the command is **not executed** and the error
///   message is returned in `stderr` with `exit_code = -1`.
///
/// # Return value
/// Always returns a [`CommandOutput`]; never panics. If spawning the child
/// process fails (e.g. shell not found), the error message is placed in
/// `stderr` and `exit_code` is set to `-1`.
///
/// # Exit code convention
/// - Normal exit: the process's own exit code.
/// - Signal-terminated (Unix): `-1` (no exit code available).
/// - Spawn failure or cwd validation failure: `-1`.
pub fn run_command(command: &str, cwd: Option<&str>) -> CommandOutput {
    // Select the platform shell at compile time (SRP: one decision point).
    #[cfg(windows)]
    let (shell, flag) = ("cmd", "/C");
    #[cfg(unix)]
    let (shell, flag) = ("sh", "-c");

    let mut cmd = Command::new(shell);
    cmd.arg(flag).arg(command);

    // Apply cwd when supplied; otherwise the child inherits the parent's cwd.
    if let Some(dir) = cwd {
        // Fase 1 security: validate before touching the OS with this path.
        if let Err(msg) = validate_cwd(dir) {
            return CommandOutput {
                stdout: String::new(),
                stderr: msg,
                exit_code: -1,
            };
        }
        // Use the same (trimmed) value that validate_cwd checked, so validation
        // and the value handed to the OS can never diverge.
        cmd.current_dir(dir.trim());
    }

    // `.output()` captures stdout + stderr fully and closes the child's stdin.
    // This is intentional: the child must NOT inherit our stdin, which on the
    // MCP wire path carries the JSON-RPC framing.
    match cmd.output() {
        Ok(output) => {
            // `output.status.code()` returns None only when the process was
            // terminated by a signal (Unix). We document -1 as the convention.
            let exit_code = output.status.code().unwrap_or(-1);
            CommandOutput {
                stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                exit_code,
            }
        }
        Err(e) => {
            // Spawn failure (shell not found, permission denied, …).
            // We surface the error in stderr so callers can report it, rather
            // than panicking, keeping the server robust.
            CommandOutput {
                stdout: String::new(),
                stderr: format!("Failed to spawn shell: {e}"),
                exit_code: -1,
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── 1. stdout capture ────────────────────────────────────────────────────

    /// `echo hello` must produce stdout containing "hello" and exit 0.
    ///
    /// Note: `cmd /C echo hello` on Windows appends `\r\n`; we use
    /// `contains("hello")` to be tolerant of line-ending differences.
    #[test]
    fn stdout_echo_hello() {
        let out = run_command("echo hello", None);
        assert!(
            out.stdout.contains("hello"),
            "expected stdout to contain 'hello', got: {:?}",
            out.stdout
        );
        assert_eq!(out.exit_code, 0, "expected exit code 0");
    }

    // ── 2. non-zero exit code ────────────────────────────────────────────────

    /// `cmd /C exit 3` must exit with code 3.
    ///
    /// We use an explicit `exit N` rather than a nonexistent command so the
    /// exit code is deterministic across environments.
    #[cfg(windows)]
    #[test]
    fn nonzero_exit_code_windows() {
        let out = run_command("exit 3", None);
        assert_eq!(
            out.exit_code, 3,
            "expected exit code 3, got: {}",
            out.exit_code
        );
    }

    #[cfg(unix)]
    #[test]
    fn nonzero_exit_code_unix() {
        let out = run_command("exit 3", None);
        assert_eq!(
            out.exit_code, 3,
            "expected exit code 3, got: {}",
            out.exit_code
        );
    }

    // ── 3. stderr capture ────────────────────────────────────────────────────

    /// A command that writes to stderr must produce a non-empty `stderr` field.
    ///
    /// On Windows: `echo oops 1>&2` redirects echo output to stderr.
    /// On Unix:    `echo oops >&2` does the same via sh.
    #[cfg(windows)]
    #[test]
    fn stderr_capture_windows() {
        let out = run_command("echo oops 1>&2", None);
        assert!(
            !out.stderr.is_empty(),
            "expected non-empty stderr, got empty string; stdout was: {:?}",
            out.stdout
        );
    }

    #[cfg(unix)]
    #[test]
    fn stderr_capture_unix() {
        let out = run_command("echo oops >&2", None);
        assert!(
            !out.stderr.is_empty(),
            "expected non-empty stderr, got empty string"
        );
    }

    // ── 4. cwd respected ─────────────────────────────────────────────────────

    /// A unique subdirectory is created in the system temp dir, used as `cwd`.
    /// The command prints the current directory; its output must contain the
    /// unique directory name (case-insensitive, to tolerate Windows path
    /// canonicalisation differences like short vs. long names).
    #[test]
    fn cwd_is_respected() {
        // tempfile::TempDir creates a randomly-named dir and removes it on drop.
        let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
        let tmp_path = tmp.path();
        // Grab the directory name component (the random part) for assertion.
        let unique_name = tmp_path
            .file_name()
            .expect("temp dir has no file_name")
            .to_string_lossy()
            .to_lowercase();

        // Command that prints the current directory, per shell:
        // - Windows (cmd):  `cd`  (with no args, prints the cwd)
        // - Unix (sh):      `pwd` (cmd's `cd` with no args would go to $HOME and print nothing)
        #[cfg(windows)]
        let print_cwd = "cd";
        #[cfg(unix)]
        let print_cwd = "pwd";
        let out = run_command(
            print_cwd,
            Some(tmp_path.to_str().expect("non-UTF-8 temp path")),
        );

        assert!(
            out.stdout.to_lowercase().contains(&unique_name),
            "expected stdout to contain the temp dir name '{unique_name}', \
             got: {:?}",
            out.stdout
        );
        assert_eq!(out.exit_code, 0, "expected exit code 0 for 'cd'");
    }

    // ── 5. validate_cwd — security unit tests ────────────────────────────────
    //
    // These tests operate purely on path strings (syntactic + local-fs checks).
    // No network traffic is triggered because UNC paths are rejected before
    // any filesystem call is made.

    /// UNC path with backslashes must be rejected with the UNC-specific message.
    /// This also covers Windows verbatim/device paths (\\?\, \\.\).
    #[test]
    fn validate_cwd_rejects_unc_backslash() {
        let result = validate_cwd(r"\\evil\share");
        assert!(result.is_err(), "expected Err for UNC backslash path");
        let msg = result.unwrap_err();
        assert!(
            msg.contains("UNC"),
            "expected 'UNC' in error message, got: {msg:?}"
        );
    }

    /// UNC path with forward slashes must be rejected with the UNC-specific message.
    #[test]
    fn validate_cwd_rejects_unc_forward_slash() {
        let result = validate_cwd("//evil/share");
        assert!(result.is_err(), "expected Err for UNC forward-slash path");
        let msg = result.unwrap_err();
        assert!(
            msg.contains("UNC"),
            "expected 'UNC' in error message, got: {msg:?}"
        );
    }

    /// Windows verbatim path (\\?\) must be rejected with the UNC-specific message.
    /// This test proves the UNC check fires FIRST — the path `C:\Windows` is a
    /// real directory, so only the prefix guard stops it from returning Ok.
    #[test]
    fn validate_cwd_rejects_verbatim_device_path() {
        let result = validate_cwd(r"\\?\C:\Windows");
        assert!(result.is_err(), "expected Err for verbatim device path");
        let msg = result.unwrap_err();
        assert!(
            msg.contains("UNC"),
            "expected 'UNC' in error message (proves UNC check fired first), got: {msg:?}"
        );
    }

    /// A relative path must be rejected with the "absolute" error message.
    #[test]
    fn validate_cwd_rejects_relative_path() {
        let result = validate_cwd("some/rel/dir");
        assert!(result.is_err(), "expected Err for relative path");
        let msg = result.unwrap_err();
        assert!(
            msg.contains("absolute"),
            "expected 'absolute' in error message, got: {msg:?}"
        );
    }

    /// An absolute path that does not exist on disk must be rejected.
    /// Gated to Windows because `C:\...` is not absolute on Unix, and we don't
    /// want it to hit the "not absolute" branch instead of "not exists".
    #[cfg(windows)]
    #[test]
    fn validate_cwd_rejects_nonexistent_path() {
        let result = validate_cwd(r"C:\__nope_lare_test__\__nope__");
        assert!(result.is_err(), "expected Err for nonexistent path");
        let msg = result.unwrap_err();
        assert!(
            msg.contains("not exist") || msg.contains("not a directory"),
            "expected 'not exist' or 'not a directory' in error message, got: {msg:?}"
        );
    }

    /// A real temporary directory must be accepted (returns Ok).
    #[test]
    fn validate_cwd_accepts_real_temp_dir() {
        let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
        let path_str = tmp.path().to_str().expect("non-UTF-8 temp path");
        let result = validate_cwd(path_str);
        assert!(
            result.is_ok(),
            "expected Ok for real existing temp dir, got: {:?}",
            result
        );
    }

    // ── 6. end-to-end: run_command with UNC cwd → blocked ────────────────────

    /// `run_command` with a UNC cwd must NOT execute the command:
    /// - exit_code == -1  (validation failure sentinel)
    /// - stderr non-empty (contains the rejection reason)
    /// - stdout empty     (proves the command body never ran)
    #[test]
    fn run_command_unc_cwd_blocked() {
        let out = run_command("echo x", Some(r"\\evil\share"));
        assert_eq!(
            out.exit_code, -1,
            "expected exit_code -1 for UNC-blocked command, got: {}",
            out.exit_code
        );
        assert!(
            !out.stderr.is_empty(),
            "expected non-empty stderr for UNC-blocked command"
        );
        assert!(
            out.stdout.is_empty(),
            "expected empty stdout (command must not have executed), got: {:?}",
            out.stdout
        );
    }
}
