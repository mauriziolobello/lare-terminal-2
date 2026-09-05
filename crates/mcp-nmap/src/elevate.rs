//! # elevate — Windows elevation for `nmap_os_detect` (design spec §4)
//!
//! `ShellExecuteExW` with `lpVerb = "runas"` triggers Windows' UAC prompt;
//! `fMask = SEE_MASK_NOCLOSEPROCESS` asks Windows to hand back a process
//! handle (`hProcess`) instead of closing it immediately, so we can
//! `WaitForSingleObject` on it and later `GetExitCodeProcess`.
//!
//! **Not unit-testable**: real elevation requires an interactive UAC prompt.
//! This module is isolated so the rest of the crate (Tasks 1-3, 5) stays
//! platform-agnostic and testable without any Windows-specific mocking. Only
//! the ERROR MAPPING (Windows error codes → readable `ElevationError`
//! variants) is tested here, as pure functions taking a raw error code.
//! A manual end-to-end test (real scan, real UAC prompt) is required before
//! merge — see this crate's companion integration plan's final task.

use std::time::Duration;

/// Readable elevation failure — never a panic, never a raw Windows error
/// code surfaced to the tool caller directly (design spec §4).
#[derive(Debug, Clone, PartialEq)]
pub enum ElevationError {
    /// User dismissed/declined the UAC prompt. Windows reports this as
    /// `ERROR_CANCELLED` (1223) from `ShellExecuteExW`.
    UserCancelled,
    /// The scan exceeded its timeout — distinct from the confirm-gate's own
    /// 180s (design spec §4: that covers waiting for the user's confirm
    /// click, not the scan's own duration; a `/24` scan can run well past
    /// 180s). Fixed at 600s (10 minutes) here — generous enough for a
    /// `/24` `-O` scan without hanging indefinitely on a truly stuck process.
    Timeout,
    Other(String),
}

impl std::fmt::Display for ElevationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ElevationError::UserCancelled => write!(f, "Elevazione rifiutata dall'utente"),
            ElevationError::Timeout => write!(
                f,
                "Timeout: lo scan elevato ha superato il limite di tempo consentito"
            ),
            ElevationError::Other(reason) => write!(f, "Elevazione fallita: {reason}"),
        }
    }
}

/// Timeout for an elevated scan (`nmap_os_detect`'s child process), distinct
/// from the 180s confirm-gate timeout (design spec §4).
pub const SCAN_TIMEOUT: Duration = Duration::from_secs(600);

/// Maps a raw Windows error code (from `GetLastError()`-style reporting, as
/// surfaced by `windows::core::Error::code()` after a failed `ShellExecuteExW`
/// call) to a readable `ElevationError`. Pure function — testable without
/// touching the real Windows API.
///
/// `1223` is `ERROR_CANCELLED` — the UAC-decline case (design spec §4).
fn map_shell_execute_error(raw_code: i32) -> ElevationError {
    const ERROR_CANCELLED: i32 = 1223;
    match raw_code {
        ERROR_CANCELLED => ElevationError::UserCancelled,
        other => ElevationError::Other(format!("codice errore Windows {other}")),
    }
}

/// Builds `ShellExecuteExW`'s `lpParameters` string from a raw argv-shaped
/// slice. Windows passes `lpParameters` to the launched executable as a
/// single command-line string, not an argv array — the executable's own
/// C runtime re-splits it on whitespace (roughly `CommandLineToArgvW`
/// rules). Pure, platform-agnostic function — no Windows API touched here,
/// so it is fully unit-tested without any FFI.
///
/// Any argument containing whitespace is wrapped in `"..."` so it survives
/// as one argv entry. This matters concretely for this crate: `tempfile`
/// reserves its `-oX` output path under `std::env::temp_dir()`, which is
/// `C:\Users\<username>\...` — on any machine whose Windows username
/// contains a space, an unquoted path would silently split into multiple
/// argv entries once nmap re-parses `lpParameters`, truncating the `-oX`
/// path and making the scan fail deterministically regardless of target.
///
/// Arguments containing a literal `"` are rejected (`Err`) rather than
/// escaped: naively escaping quotes here could itself become an argument-
/// injection vector if this crate's escaping doesn't exactly match the
/// target executable's own unescaping rules. None of this crate's own
/// arguments (flags, the tempfile path, a scan target) has a legitimate
/// reason to contain a quote character.
fn build_parameters(args: &[&str]) -> Result<String, ElevationError> {
    let mut parts = Vec::with_capacity(args.len());
    for arg in args {
        if arg.contains('"') {
            return Err(ElevationError::Other(
                "argomento non consentito: contiene un carattere virgolette (\")".to_string(),
            ));
        }
        if arg.chars().any(char::is_whitespace) {
            parts.push(format!("\"{arg}\""));
        } else {
            parts.push((*arg).to_string());
        }
    }
    Ok(parts.join(" "))
}

#[cfg(windows)]
mod windows_impl {
    // `SCAN_TIMEOUT` is not used inside this module — `run_elevated` takes
    // `timeout` as an explicit parameter; the constant is only referenced by
    // `scan.rs`'s call site (`crate::elevate::SCAN_TIMEOUT`).
    use super::{build_parameters, map_shell_execute_error, ElevationError};
    use windows::core::{HSTRING, PCWSTR};
    // `WAIT_OBJECT_0`/`WAIT_TIMEOUT` live in `Win32::Foundation` (they're
    // `WAIT_EVENT` constants, not part of the Threading module itself) —
    // discovered as a real `E0432` unresolved-import error during Task 4's
    // `cargo build -p mcp-nmap` (see this crate's IMPLEMENTATION.md).
    use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, TerminateProcess, WaitForSingleObject,
    };
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};

    /// Run `exe args...` elevated (UAC "runas"), wait up to `timeout`, and
    /// return the child's exit code on success.
    ///
    /// # Safety / FFI notes
    /// `ShellExecuteExW` and `WaitForSingleObject`/`GetExitCodeProcess` are
    /// `unsafe` (raw Win32 calls) — wrapped here behind a safe public
    /// function; no caller outside this module touches the raw APIs.
    ///
    /// # `HSTRING`/`PCWSTR` construction (verified against the actually-
    /// compiled `windows`/`windows-strings` 0.62.2 docs, Task 4 Step 1 — see
    /// this crate's IMPLEMENTATION.md for the exact findings):
    /// `HSTRING::from(&str)` is a real `impl From<&str> for HSTRING`.
    /// `HSTRING` has no *inherent* `as_ptr` — it derefs to `[u16]`
    /// (`impl Deref for HSTRING { type Target = [u16]; }`), so `.as_ptr()`
    /// resolves to the slice's `as_ptr(&self) -> *const u16` via Rust's
    /// normal method-call auto-deref; the crate's own `Deref` impl keeps the
    /// empty-string case null-terminated specifically so this works.
    /// `PCWSTR::from_raw(ptr: *const u16) -> Self` is a real, exact-match
    /// associated function.
    ///
    /// # Argument quoting (post-review fix)
    /// `args` is joined via `build_parameters` (not a naive `args.join(" ")`
    /// — the design brief's original text), which quotes any argument
    /// containing whitespace. Without this, a tempfile path under a
    /// Windows profile whose username contains a space (`C:\Users\John
    /// Smith\...`) would silently split into multiple argv entries once
    /// nmap re-parses `lpParameters`, corrupting the `-oX` output path.
    pub fn run_elevated(
        exe: &str,
        args: &[&str],
        timeout: std::time::Duration,
    ) -> Result<i32, ElevationError> {
        let params_str = build_parameters(args)?;

        let verb = HSTRING::from("runas");
        let file = HSTRING::from(exe);
        let params = HSTRING::from(params_str);

        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS,
            lpVerb: PCWSTR::from_raw(verb.as_ptr()),
            lpFile: PCWSTR::from_raw(file.as_ptr()),
            lpParameters: PCWSTR::from_raw(params.as_ptr()),
            nShow: 0, // SW_HIDE — no visible console window for the elevated nmap.exe
            ..Default::default()
        };

        // SAFETY: `info` is a valid, fully-initialised SHELLEXECUTEINFOW; the
        // HSTRINGs it borrows from (`verb`/`file`/`params`) all outlive this
        // call (they are not dropped until this function returns).
        let result = unsafe { ShellExecuteExW(&mut info) };
        if let Err(e) = result {
            return Err(map_shell_execute_error(e.code().0));
        }

        let hprocess: HANDLE = info.hProcess;
        if hprocess.is_invalid() {
            return Err(ElevationError::Other(
                "ShellExecuteExW non ha restituito un handle di processo valido".to_string(),
            ));
        }

        // SAFETY: `hprocess` is the valid handle just returned by ShellExecuteExW.
        let wait_result = unsafe { WaitForSingleObject(hprocess, timeout.as_millis() as u32) };
        if wait_result == WAIT_TIMEOUT {
            // Il processo elevato ha superato SCAN_TIMEOUT: termina forzatamente
            // prima di chiudere l'handle (design spec §4 — "processo elevato
            // terminato se ancora vivo"). Senza questo, nmap.exe elevato continua a
            // girare oltre il timeout, e può tenere aperto il file -oX impedendo la
            // pulizia del TempPath in run_os_detect.
            //
            // NOTA: mcp-nmap gira a integrità media (mai elevato); il processo
            // target gira a integrità alta (elevato via UAC). Un processo a
            // integrità media può normalmente terminare un proprio processo figlio
            // anche se quel figlio è elevato (il token del figlio discende da
            // ShellExecuteExW, che questo processo ha invocato) — ma se
            // TerminateProcess dovesse comunque fallire (accesso negato o altro),
            // l'errore viene ignorato qui: abbiamo già deciso di ritornare
            // ElevationError::Timeout in ogni caso, e un fallimento nel forzare la
            // terminazione non deve mascherare il timeout con un errore diverso.
            unsafe {
                let _ = TerminateProcess(hprocess, 1);
                let _ = CloseHandle(hprocess);
            }
            return Err(ElevationError::Timeout);
        }
        if wait_result != WAIT_OBJECT_0 {
            unsafe {
                let _ = CloseHandle(hprocess);
            }
            return Err(ElevationError::Other(format!(
                "WaitForSingleObject ha restituito un esito inatteso: {wait_result:?}"
            )));
        }

        let mut exit_code: u32 = 0;
        // SAFETY: `hprocess` is still valid (not yet closed) and the process
        // has signalled (WAIT_OBJECT_0 above), so its exit code is available.
        let exit_result = unsafe { GetExitCodeProcess(hprocess, &mut exit_code) };
        unsafe {
            let _ = CloseHandle(hprocess);
        }
        match exit_result {
            Ok(()) => Ok(exit_code as i32),
            Err(e) => Err(ElevationError::Other(format!(
                "GetExitCodeProcess fallita: {e}"
            ))),
        }
    }
}

#[cfg(windows)]
pub use windows_impl::run_elevated;

#[cfg(not(windows))]
pub fn run_elevated(_exe: &str, _args: &[&str], _timeout: Duration) -> Result<i32, ElevationError> {
    Err(ElevationError::Other(
        "elevazione non supportata su questa piattaforma (solo Windows in v1 — design spec §9)"
            .to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_cancelled_maps_to_user_cancelled_variant() {
        let err = map_shell_execute_error(1223);
        assert_eq!(err, ElevationError::UserCancelled);
        assert_eq!(err.to_string(), "Elevazione rifiutata dall'utente");
    }

    #[test]
    fn unknown_error_code_maps_to_other_with_code_in_message() {
        let err = map_shell_execute_error(5);
        assert!(matches!(err, ElevationError::Other(_)));
        assert!(err.to_string().contains('5'));
    }

    #[test]
    fn timeout_display_is_readable_and_distinct_from_confirm_gate() {
        assert_eq!(
            ElevationError::Timeout.to_string(),
            "Timeout: lo scan elevato ha superato il limite di tempo consentito"
        );
    }

    #[test]
    fn scan_timeout_constant_exceeds_the_180s_confirm_gate() {
        assert!(
            SCAN_TIMEOUT > Duration::from_secs(180),
            "design spec §4: lo scan timeout deve superare i 180s del gate di conferma"
        );
    }

    // --- `build_parameters` — post-review fix: a naive `args.join(" ")`
    // (the original code, matching the design brief's literal text) silently
    // corrupts any argument containing whitespace. Concretely: `tempfile`
    // reserves its `-oX` output path under `std::env::temp_dir()`, which on
    // any machine where the Windows username contains a space (e.g.
    // `C:\Users\John Smith\AppData\Local\Temp\...`) would split into two
    // argv entries once `ShellExecuteExW` (via the launched nmap.exe's own
    // command-line parser) re-splits `lpParameters` on whitespace — nmap
    // would then receive a truncated `-oX` path and the scan would
    // deterministically fail on every machine with a spaced username,
    // regardless of target. This was never reachable by any test in this
    // crate (the guard test only exercises `nmap_available == false`) and
    // would not show up in a manual e2e run on a dev machine whose username
    // has no space.

    #[test]
    fn build_parameters_quotes_an_argument_containing_whitespace() {
        let args = ["-O", "-oX", "C:\\Users\\John Smith\\a.xml", "192.168.1.10"];
        let params = build_parameters(&args).expect("no quote characters present");
        assert_eq!(
            params,
            "-O -oX \"C:\\Users\\John Smith\\a.xml\" 192.168.1.10"
        );
    }

    #[test]
    fn build_parameters_leaves_whitespace_free_arguments_unquoted() {
        let args = ["-O", "-oX", "C:\\Users\\Maurizio\\a.xml", "192.168.1.10"];
        let params = build_parameters(&args).expect("no quote characters present");
        assert_eq!(params, "-O -oX C:\\Users\\Maurizio\\a.xml 192.168.1.10");
    }

    #[test]
    fn build_parameters_rejects_an_argument_containing_a_quote_character() {
        // A `target` containing `"` could otherwise unbalance the quoting
        // of the whole `lpParameters` string, letting an attacker-influenced
        // target smuggle extra argv entries past its intended boundary
        // (argument injection) — rejected outright rather than escaped, to
        // avoid depending on this crate's escaping matching nmap's own
        // unescaping rules exactly.
        let args = ["-O", "-oX", "C:\\a.xml", "10.0.0.1\" & calc.exe \""];
        let result = build_parameters(&args);
        assert!(result.is_err());
    }
}
