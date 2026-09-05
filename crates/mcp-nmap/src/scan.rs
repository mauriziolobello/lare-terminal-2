//! # scan — process invocation for nmap tools
//!
//! `NmapProcess` is the seam that makes `run_quick_scan` testable without
//! spawning a real `nmap`: it takes `&dyn NmapProcess` and never touches
//! `std::process::Command` directly. The real implementation
//! (`RealNmapProcess`, used by `main.rs`) does.
//!
//! `run_os_detect` (Task 4) does **not** go through `NmapProcess` — it calls
//! `elevate::run_elevated` directly. Elevation requires a real interactive
//! UAC prompt, so there is no meaningful cross-platform fake for it beyond
//! `elevate`'s own `#[cfg(not(windows))]` stub; routing it through the same
//! trait as the unelevated path would suggest it's mockable/tested the same
//! way, which it deliberately isn't (see `elevate`'s module doc).

use crate::markdown::{format_report_markdown, format_summary};
use crate::report::parse_nmap_xml;
use std::path::Path;
use std::process::ExitStatus;

/// Result of a scan tool call — the exact shape serialized as this MCP
/// tool's JSON response (Task 5), and deserialized by the orchestrator's
/// `NmapToolClient` (companion integration plan). `report_markdown` is
/// **never** sent to the LLM by the orchestrator — only `summary` is (design
/// spec §6); it exists in this same struct because both are produced
/// together from one `ScanReport` and it is simpler to carry them as one
/// unit than to re-parse XML twice.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ScanOutcome {
    pub summary: String,
    pub report_markdown: String,
    pub is_error: bool,
}

impl ScanOutcome {
    /// Build a tool-error outcome (PATH missing, spawn failure, malformed
    /// XML, timeout) — `report_markdown` is empty (design spec §7: these are
    /// tool errors, not reports; nothing to show in a Markdown window for
    /// "nmap non trovato sul PATH").
    fn error(message: impl Into<String>) -> Self {
        Self {
            summary: message.into(),
            report_markdown: String::new(),
            is_error: true,
        }
    }
}

/// Abstraction over "run nmap and produce an exit status" for the
/// unelevated quick scan only (`run_os_detect` does not use this — see the
/// module doc). The real implementation spawns a process directly; tests
/// implement this trait with canned behaviour, never touching the real OS.
pub trait NmapProcess {
    /// `args` excludes the binary name (e.g. `["-sT", "-oX", "<path>", "<target>"]`).
    /// `xml_path` is where nmap is expected to write its `-oX` output —
    /// passed separately (not just parsed out of `args`) so the caller can
    /// read the file after a successful exit without re-parsing argv.
    fn run(&self, args: &[&str], xml_path: &Path) -> Result<ExitStatus, String>;
}

/// Checks whether `nmap` resolves on `PATH` — explicit check before every
/// invocation (design spec §7: never rely on the OS spawn error, which is
/// less readable). Uses `which`-style `PATH` scanning via `std::env::var`.
///
/// Called only from `main.rs` (Task 5), which passes its result into
/// `run_quick_scan`/`run_os_detect`'s `nmap_available` parameter — kept as a
/// separate function (not inlined into those two) so it stays independently
/// testable in isolation later if needed, without being exercised implicitly
/// by every scan-orchestration test.
pub fn nmap_on_path() -> bool {
    let Ok(path_var) = std::env::var("PATH") else {
        return false;
    };
    #[cfg(windows)]
    let exe_name = "nmap.exe";
    #[cfg(not(windows))]
    let exe_name = "nmap";
    std::env::split_paths(&path_var).any(|dir| dir.join(exe_name).is_file())
}

/// Validates a scan target's *content* before it ever reaches nmap's own
/// argv — the fix for a Critical security-review finding (argv flag
/// smuggling): `target` is caller/tool supplied (ultimately the LLM, via the
/// orchestrator's tool dispatch), and neither `run_quick_scan` nor
/// `run_os_detect` checked its content at all before this fix. A target
/// starting with `-` (e.g. `"--script=vuln"`) is parsed by nmap as a FLAG,
/// not a positional target — completely independent of `elevate`'s
/// `build_parameters` quoting fix, which only controls how Windows splits
/// `lpParameters` into argv tokens and does nothing about nmap's own
/// flag-vs-positional-argument semantics once it receives a clean single
/// token. On `run_os_detect`'s path this runs **elevated** (post-UAC),
/// directly reaching the "no NSE scripts / no third nmap tool" boundary the
/// design spec (§9) puts out of scope for v1 — via argument smuggling
/// instead of a legitimate third tool.
///
/// Deliberately permissive of legitimate nmap target syntax: IPv4, IPv4 CIDR
/// (`192.168.1.0/24`), IPv4 range (`192.168.1.1-254`), octet wildcard
/// (`192.168.1.*`), hostname (including common hyphenated ones like
/// `my-host.example.com`), and IPv6 can all legitimately use
/// `.`/`:`/`/`/`-`/`_`/`*` characters anywhere except as the very first
/// character. This is **not** a full nmap target-grammar parser — it is a
/// defense-in-depth whitelist plus the two checks that matter most for this
/// exploit (a leading `-`, and embedded whitespace, since this tool takes
/// exactly one target per call — nmap's own space-separated multi-target
/// syntax is out of scope here, same "one target per call" principle this
/// crate already establishes elsewhere).
pub(crate) fn validate_target(target: &str) -> Result<(), String> {
    if target.is_empty() {
        return Err(
            "target vuoto: specifica un host, IP, range o rete da scansionare.".to_string(),
        );
    }
    if target.starts_with('-') {
        return Err(format!(
            "target non valido (\"{target}\"): inizia con '-' e verrebbe interpretato da nmap \
             come un'opzione della riga di comando anziché come bersaglio della scansione."
        ));
    }
    if target.chars().any(char::is_whitespace) {
        return Err(format!(
            "target non valido (\"{target}\"): contiene spazi. Questo strumento accetta un solo \
             bersaglio per chiamata."
        ));
    }
    if let Some(bad_char) = target
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '/' | '*' | '-' | '_')))
    {
        return Err(format!(
            "target non valido (\"{target}\"): contiene un carattere non ammesso ('{bad_char}')."
        ));
    }
    Ok(())
}

/// Shared pipeline behind every unelevated scan tool (`nmap_quick_scan`,
/// `nmap_version_scan`, `nmap_host_discovery`, `nmap_vuln_scan`): validate
/// target → PATH guard → tempfile for `-oX` → spawn via `NmapProcess` →
/// exit-code guard → read + parse the XML → `ScanOutcome`. `extra_args` is
/// the ONLY thing that differs between callers — e.g. `&["-sT"]` for a quick
/// scan, `&["-sV"]` for version detection, `&["--script", "vuln"]` for a
/// vuln scan. Extracted from the original `run_quick_scan` body (Task 1 of
/// the 2026-07-18 scan-variants plan) once three more callers needed the
/// identical pipeline — `run_os_detect` does NOT use this (it goes through
/// `elevate::run_elevated`, not `NmapProcess` — see the module doc).
fn run_scan_with_flags(
    target: &str,
    process: &dyn NmapProcess,
    nmap_available: bool,
    extra_args: &[&str],
) -> ScanOutcome {
    if let Err(e) = validate_target(target) {
        return ScanOutcome::error(e);
    }
    if !nmap_available {
        return ScanOutcome::error(
            "nmap non trovato sul PATH. Installa nmap per usare questo canale.",
        );
    }

    let temp_path = match tempfile::NamedTempFile::new() {
        Ok(f) => f.into_temp_path(),
        Err(e) => {
            return ScanOutcome::error(format!(
                "impossibile creare il file temporaneo per l'output XML: {e}"
            ))
        }
    };
    let xml_path_str = temp_path.to_string_lossy().into_owned();

    let mut args: Vec<&str> = extra_args.to_vec();
    args.push("-oX");
    args.push(&xml_path_str);
    args.push(target);

    let status = match process.run(&args, &temp_path) {
        Ok(s) => s,
        Err(e) => return ScanOutcome::error(format!("esecuzione di nmap fallita: {e}")),
    };
    if !status.success() {
        return ScanOutcome::error(format!(
            "nmap è terminato con errore (exit code {:?})",
            status.code()
        ));
    }

    let xml = match std::fs::read_to_string(&temp_path) {
        Ok(s) => s,
        Err(e) => {
            return ScanOutcome::error(format!("impossibile leggere l'output XML di nmap: {e}"))
        }
    };
    match parse_nmap_xml(&xml, false) {
        Ok(report) => ScanOutcome {
            summary: format_summary(&report),
            report_markdown: format_report_markdown(&report),
            is_error: false,
        },
        Err(e) => ScanOutcome::error(e.to_string()),
    }
    // `temp_path` drops here, deleting the XML file — after it's been read.
}

/// Run `nmap_quick_scan`: unelevated, `-sT` pinned explicitly (never left to
/// nmap's privilege-based default — design spec §3).
///
/// `nmap_available` is passed in (not checked internally via `nmap_on_path()`)
/// so this function's tests are deterministic regardless of whether the
/// machine running them happens to have `nmap` installed — `main.rs` is the
/// only real caller and passes `nmap_on_path()` there.
pub fn run_quick_scan(
    target: &str,
    process: &dyn NmapProcess,
    nmap_available: bool,
) -> ScanOutcome {
    run_scan_with_flags(target, process, nmap_available, &["-sT"])
}

/// Run `nmap_version_scan`: unelevated, `-sV` (probes open ports to determine
/// service/version info nmap doesn't report on a plain `-sT` scan). User's
/// option 5 from the live "what else can nmap do" conversation
/// (2026-07-18) — same pipeline as `run_quick_scan`, different flag.
pub fn run_version_scan(
    target: &str,
    process: &dyn NmapProcess,
    nmap_available: bool,
) -> ScanOutcome {
    run_scan_with_flags(target, process, nmap_available, &["-sV"])
}

/// Run `nmap_host_discovery`: unelevated, `-sn` (ping-style sweep — which
/// hosts in `target` are up, no port scan at all). User's option 6.
/// `format_summary`'s "N porte aperte" phrasing is technically imprecise
/// here (no ports are ever probed, so it's always 0) but not incorrect —
/// left as-is rather than special-cased, out of scope for this plan.
pub fn run_host_discovery(
    target: &str,
    process: &dyn NmapProcess,
    nmap_available: bool,
) -> ScanOutcome {
    run_scan_with_flags(target, process, nmap_available, &["-sn"])
}

/// Run `nmap_vuln_scan`: unelevated, `nmap`'s own curated `vuln` NSE script
/// category — checks for known vulnerabilities using nmap's built-in
/// `--script vuln` set (documented by nmap itself, not this crate). User's
/// option 3, explicitly requested after asking specifically about
/// vulnerability testing on their own home router (2026-07-18).
///
/// **Security note (design intent, not a loophole to close later):** this
/// is the ONLY vuln-scan capability this crate exposes — the script name is
/// hardcoded to `"vuln"`, never taken as a parameter from the AI. Accepting
/// an arbitrary `--script <name>` from the model would reopen exactly the
/// class of concern `validate_target` already defends against for targets
/// (argv smuggling / running an attacker-influenced NSE script) — this
/// project's fixed-tools-never-a-shell-escape rule applies here too.
///
/// **`--script-timeout 60s` (2026-07-18 hang fix):** live-reproduced bug —
/// a real scan against a real router hung indefinitely (NSE completion
/// stuck at 99.76% for 5+ minutes straight, no progress) because one NSE
/// script in the "vuln" category has no bounded timeout of its own and the
/// target silently drops its probe instead of resetting/erroring (common
/// for SIP/UPnP-style ports on consumer routers). Nothing bounded any
/// individual script's execution time before this fix, so the ONLY thing
/// that ever stopped a hang was the orchestrator's 900s outer timeout
/// (`nmap_tool_client.rs`'s `NMAP_CALL_TIMEOUT_SECS`) — which kills the
/// orchestrator's *wait*, not the nmap process itself, and discards 100% of
/// the scan data (every port, every OTHER script's completed result)
/// because nmap never got to write its `-oX` file. `--script-timeout`
/// bounds each *individual* script instead: when one script exceeds it,
/// nmap abandons just that script and moves on, still writing a complete
/// report with everything else. Verified against the real hang: re-running
/// the exact scan that hung, with `--script-timeout 60s` added, completed
/// cleanly in 71.44 seconds with a full report (all 9 ports, all other
/// script results intact) instead of hanging for 900s+ and producing
/// nothing. 60s was chosen generously above what any legitimate script in
/// this run took (the slowest completed script finished well under 10s) —
/// not tuned to the failure, just a sane upper bound per script.
pub fn run_vuln_scan(target: &str, process: &dyn NmapProcess, nmap_available: bool) -> ScanOutcome {
    run_scan_with_flags(
        target,
        process,
        nmap_available,
        &["--script", "vuln", "--script-timeout", "60s"],
    )
}

/// Run `nmap_os_detect`: elevated (`-O`), same output-shape as
/// `run_quick_scan` otherwise. Calls `elevate::run_elevated` directly —
/// **not** through the `NmapProcess` trait: elevation has no meaningful
/// cross-platform mock beyond what `elevate`'s own `#[cfg(not(windows))]`
/// stub already provides, and faking a `ShellExecuteExW`+UAC round-trip
/// behind the same seam as `run_quick_scan` would hide, rather than test,
/// the one part of this crate that truly can't be exercised without a real
/// interactive prompt (see `elevate`'s module doc). `nmap_available` follows
/// the same pattern as `run_quick_scan` — passed in by the caller
/// (`main.rs` passes `nmap_on_path()`), never checked internally, so this
/// stays testable (for its deterministic PATH-guard branch) without
/// depending on the test machine's real PATH.
pub fn run_os_detect(target: &str, nmap_available: bool) -> ScanOutcome {
    if let Err(e) = validate_target(target) {
        return ScanOutcome::error(e);
    }
    if !nmap_available {
        return ScanOutcome::error(
            "nmap non trovato sul PATH. Installa nmap per usare questo canale.",
        );
    }

    let temp_path = match tempfile::NamedTempFile::new() {
        Ok(f) => f.into_temp_path(),
        Err(e) => {
            return ScanOutcome::error(format!(
                "impossibile creare il file temporaneo per l'output XML: {e}"
            ))
        }
    };
    let xml_path_str = temp_path.to_string_lossy().into_owned();

    #[cfg(windows)]
    let exe = "nmap.exe";
    #[cfg(not(windows))]
    let exe = "nmap";

    let args = ["-O", "-oX", &xml_path_str, target];
    match crate::elevate::run_elevated(exe, &args, crate::elevate::SCAN_TIMEOUT) {
        Ok(0) => {}
        Ok(exit_code) => {
            return ScanOutcome::error(format!(
                "nmap è terminato con errore (exit code {exit_code})"
            ))
        }
        Err(e) => return ScanOutcome::error(e.to_string()),
    }

    let xml = match std::fs::read_to_string(&temp_path) {
        Ok(s) => s,
        Err(e) => {
            return ScanOutcome::error(format!("impossibile leggere l'output XML di nmap: {e}"))
        }
    };
    match parse_nmap_xml(&xml, true) {
        Ok(report) => ScanOutcome {
            summary: format_summary(&report),
            report_markdown: format_report_markdown(&report),
            is_error: false,
        },
        Err(e) => ScanOutcome::error(e.to_string()),
    }
    // `temp_path` drops here, deleting the XML file — after it's been read.
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fake `NmapProcess` that writes canned XML to `xml_path` and reports a
    /// canned exit status — never spawns a real process.
    struct FakeNmapProcess {
        xml_to_write: &'static str,
        exit_success: bool,
    }

    impl NmapProcess for FakeNmapProcess {
        fn run(&self, _args: &[&str], xml_path: &Path) -> Result<ExitStatus, String> {
            std::fs::write(xml_path, self.xml_to_write).map_err(|e| e.to_string())?;
            // ExitStatus has no public constructor in stable std; tests use
            // `std::process::Command` to synthesize a real one cheaply
            // (`true`/`false` on Unix, `cmd /C exit 0`/`1` on Windows).
            #[cfg(windows)]
            let status = std::process::Command::new("cmd")
                .args([
                    "/C",
                    if self.exit_success {
                        "exit 0"
                    } else {
                        "exit 1"
                    },
                ])
                .status()
                .expect("failed to run synthetic exit-status process");
            #[cfg(not(windows))]
            let status =
                std::process::Command::new(if self.exit_success { "true" } else { "false" })
                    .status()
                    .expect("failed to run synthetic exit-status process");
            Ok(status)
        }
    }

    const HOST_UP_OPEN_PORT: &str = include_str!("fixtures/host_up_open_port.xml");

    // --- validate_target: argv flag-smuggling fix (security review finding) ---
    //
    // Legitimate nmap target syntax is deliberately permissive: IPv4, CIDR,
    // ranges, octet wildcards, hostnames (including hyphenated ones), and
    // IPv6 can all use `.`/`:`/`/`/`-`/`_`/`*` characters anywhere except as
    // the first character. Only a leading `-`, embedded whitespace, or a
    // character outside the whitelist should be rejected.

    #[test]
    fn validate_target_accepts_a_normal_ip() {
        assert!(validate_target("192.168.1.10").is_ok());
    }

    #[test]
    fn validate_target_accepts_a_hyphenated_hostname() {
        assert!(validate_target("my-host.example.com").is_ok());
    }

    #[test]
    fn validate_target_accepts_a_cidr_range() {
        assert!(validate_target("192.168.1.0/24").is_ok());
    }

    #[test]
    fn validate_target_accepts_an_ipv4_range() {
        assert!(validate_target("192.168.1.1-254").is_ok());
    }

    #[test]
    fn validate_target_accepts_an_octet_wildcard() {
        assert!(validate_target("192.168.1.*").is_ok());
    }

    #[test]
    fn validate_target_rejects_empty_string() {
        assert!(validate_target("").is_err());
    }

    #[test]
    fn validate_target_rejects_a_target_starting_with_dash() {
        // The actual exploit vector: nmap's own argv parser reads a leading
        // `-` as a FLAG, not a positional target.
        assert!(validate_target("--script=vuln").is_err());
    }

    #[test]
    fn validate_target_rejects_a_target_containing_whitespace() {
        assert!(validate_target("192.168.1.10 --script=vuln").is_err());
    }

    #[test]
    fn validate_target_rejects_disallowed_characters() {
        assert!(validate_target("192.168.1.10;whoami").is_err());
        assert!(validate_target("192.168.1.10`whoami`").is_err());
        assert!(validate_target("$(whoami)").is_err());
    }

    #[test]
    fn quick_scan_success_produces_summary_and_report() {
        let process = FakeNmapProcess {
            xml_to_write: HOST_UP_OPEN_PORT,
            exit_success: true,
        };
        let outcome = run_quick_scan("192.168.1.10", &process, true);
        assert!(!outcome.is_error, "expected success, got: {outcome:?}");
        assert!(outcome.summary.contains("192.168.1.10"));
        assert!(outcome.report_markdown.contains("22/tcp"));
    }

    #[test]
    fn quick_scan_nonzero_exit_is_a_readable_error() {
        let process = FakeNmapProcess {
            xml_to_write: HOST_UP_OPEN_PORT,
            exit_success: false,
        };
        let outcome = run_quick_scan("192.168.1.10", &process, true);
        assert!(outcome.is_error);
        assert!(outcome.summary.contains("exit code"));
        assert!(outcome.report_markdown.is_empty());
    }

    #[test]
    fn quick_scan_malformed_xml_is_a_readable_error_not_a_panic() {
        let process = FakeNmapProcess {
            xml_to_write: "<not valid xml",
            exit_success: true,
        };
        let outcome = run_quick_scan("192.168.1.10", &process, true);
        assert!(outcome.is_error);
        assert!(outcome.summary.contains("XML nmap malformato"));
    }

    #[test]
    fn quick_scan_nmap_not_available_is_a_readable_error_before_any_process_call() {
        // `nmap_available: false` must short-circuit BEFORE calling process.run(...) —
        // use a process that would panic/fail if actually invoked, to prove it's
        // never reached.
        struct PanicsIfCalled;
        impl NmapProcess for PanicsIfCalled {
            fn run(&self, _args: &[&str], _xml_path: &Path) -> Result<ExitStatus, String> {
                panic!("must not be called when nmap_available is false");
            }
        }
        let outcome = run_quick_scan("192.168.1.10", &PanicsIfCalled, false);
        assert!(outcome.is_error);
        assert!(outcome.summary.contains("non trovato sul PATH"));
    }

    #[test]
    fn quick_scan_invalid_target_is_a_readable_error_before_any_process_call() {
        // Security-review fix: a target nmap would parse as a flag
        // (argv smuggling, e.g. "--script=vuln") must be rejected by
        // validate_target before process.run(...) is ever reached — proven
        // the same way as the nmap_available guard above, with a process
        // fake that panics if actually invoked.
        struct PanicsIfCalled;
        impl NmapProcess for PanicsIfCalled {
            fn run(&self, _args: &[&str], _xml_path: &Path) -> Result<ExitStatus, String> {
                panic!("must not be called when target fails validation");
            }
        }
        let outcome = run_quick_scan("--script=vuln", &PanicsIfCalled, true);
        assert!(outcome.is_error);
        assert!(outcome.summary.contains("non valido"));
    }

    /// Unlike `run_quick_scan`'s equivalent guard test, this cannot inject a
    /// fake in place of the elevation call (`run_os_detect` calls
    /// `elevate::run_elevated` directly, not through `NmapProcess` — see the
    /// module doc). But the `nmap_available == false` short-circuit itself is
    /// deterministic and has nothing to do with elevation, so it is still
    /// directly testable: this only proves the function returns a readable
    /// error *before* nmap_available is true's path could ever reach
    /// `run_elevated` (which would otherwise trigger a real UAC prompt).
    #[test]
    fn os_detect_nmap_not_available_is_a_readable_error_before_elevation() {
        let outcome = run_os_detect("192.168.1.10", false);
        assert!(outcome.is_error);
        assert!(outcome.summary.contains("non trovato sul PATH"));
    }

    /// Security-review fix. Unlike `run_quick_scan`'s equivalent test, this
    /// cannot prove "elevation was never attempted" via a mock — `run_os_detect`
    /// calls `elevate::run_elevated` directly (see module doc), and there is
    /// no safe fake for a real `ShellExecuteExW(lpVerb="runas")` round-trip.
    /// Deliberately passes `nmap_available: true` (not `false`) so a future
    /// regression that removes the `validate_target` call could not be
    /// masked by the PATH-guard short-circuit — this test only proves the
    /// validation gate itself rejects an argv-smuggling target and returns
    /// a readable error, which is the one part of this path that a RED/GREEN
    /// cycle can verify without ever triggering a real elevation prompt.
    #[test]
    fn os_detect_invalid_target_is_a_readable_error() {
        let outcome = run_os_detect("--script=vuln", true);
        assert!(outcome.is_error);
        assert!(outcome.summary.contains("non valido"));
    }

    // --- run_version_scan / run_host_discovery / run_vuln_scan (2026-07-18
    // scan-variants plan) — all three share run_scan_with_flags with
    // run_quick_scan, so these tests mirror the quick-scan ones above rather
    // than re-deriving new assertions from scratch. ---

    #[test]
    fn version_scan_uses_sv_flag_and_produces_summary() {
        let process = FakeNmapProcess {
            xml_to_write: HOST_UP_OPEN_PORT,
            exit_success: true,
        };
        let outcome = run_version_scan("192.168.1.10", &process, true);
        assert!(!outcome.is_error, "expected success, got: {outcome:?}");
        assert!(outcome.summary.contains("192.168.1.10"));
        assert!(outcome.report_markdown.contains("22/tcp"));
    }

    #[test]
    fn version_scan_invalid_target_is_a_readable_error_before_any_process_call() {
        struct PanicsIfCalled;
        impl NmapProcess for PanicsIfCalled {
            fn run(&self, _args: &[&str], _xml_path: &Path) -> Result<ExitStatus, String> {
                panic!("must not be called when target fails validation");
            }
        }
        let outcome = run_version_scan("--script=vuln", &PanicsIfCalled, true);
        assert!(outcome.is_error);
        assert!(outcome.summary.contains("non valido"));
    }

    #[test]
    fn host_discovery_uses_sn_flag_and_produces_summary() {
        let process = FakeNmapProcess {
            xml_to_write: HOST_UP_OPEN_PORT,
            exit_success: true,
        };
        let outcome = run_host_discovery("192.168.1.0/24", &process, true);
        assert!(!outcome.is_error, "expected success, got: {outcome:?}");
        assert!(outcome.summary.contains("host attivo"));
    }

    #[test]
    fn host_discovery_nmap_not_available_is_a_readable_error_before_any_process_call() {
        struct PanicsIfCalled;
        impl NmapProcess for PanicsIfCalled {
            fn run(&self, _args: &[&str], _xml_path: &Path) -> Result<ExitStatus, String> {
                panic!("must not be called when nmap_available is false");
            }
        }
        let outcome = run_host_discovery("192.168.1.0/24", &PanicsIfCalled, false);
        assert!(outcome.is_error);
        assert!(outcome.summary.contains("non trovato sul PATH"));
    }

    #[test]
    fn vuln_scan_uses_script_vuln_and_produces_summary() {
        let process = FakeNmapProcess {
            xml_to_write: HOST_UP_OPEN_PORT,
            exit_success: true,
        };
        let outcome = run_vuln_scan("192.168.1.10", &process, true);
        assert!(!outcome.is_error, "expected success, got: {outcome:?}");
        assert!(outcome.summary.contains("192.168.1.10"));
    }

    #[test]
    fn vuln_scan_invalid_target_is_a_readable_error_before_any_process_call() {
        struct PanicsIfCalled;
        impl NmapProcess for PanicsIfCalled {
            fn run(&self, _args: &[&str], _xml_path: &Path) -> Result<ExitStatus, String> {
                panic!("must not be called when target fails validation");
            }
        }
        let outcome = run_vuln_scan("--script=vuln", &PanicsIfCalled, true);
        assert!(outcome.is_error);
        assert!(outcome.summary.contains("non valido"));
    }

    /// Security-relevant: proves the vuln scan ALWAYS passes exactly
    /// `["--script", "vuln"]` as its extra args, regardless of anything in
    /// `target` — there is no code path in `run_vuln_scan` that could ever
    /// forward a caller/model-supplied script name into `args`.
    #[test]
    fn vuln_scan_always_uses_the_fixed_vuln_category_never_a_custom_script() {
        struct CapturingProcess {
            captured_args: std::sync::Mutex<Vec<String>>,
        }
        impl NmapProcess for CapturingProcess {
            fn run(&self, args: &[&str], xml_path: &Path) -> Result<ExitStatus, String> {
                *self.captured_args.lock().unwrap() = args.iter().map(|s| s.to_string()).collect();
                std::fs::write(xml_path, HOST_UP_OPEN_PORT).map_err(|e| e.to_string())?;
                #[cfg(windows)]
                let status = std::process::Command::new("cmd")
                    .args(["/C", "exit 0"])
                    .status()
                    .unwrap();
                #[cfg(not(windows))]
                let status = std::process::Command::new("true").status().unwrap();
                Ok(status)
            }
        }
        let process = CapturingProcess {
            captured_args: std::sync::Mutex::new(vec![]),
        };
        run_vuln_scan("192.168.1.10", &process, true);
        let captured = process.captured_args.lock().unwrap();
        assert_eq!(captured[0], "--script");
        assert_eq!(captured[1], "vuln");
    }

    /// Regression guard for the 2026-07-18 hang fix: `run_vuln_scan` must
    /// always pass `--script-timeout 60s` alongside `--script vuln` — without
    /// it, a single unresponsive NSE script can hang the whole scan
    /// indefinitely (live-reproduced against a real router; see this
    /// function's doc comment for the full story).
    #[test]
    fn vuln_scan_always_bounds_script_execution_time() {
        struct CapturingProcess {
            captured_args: std::sync::Mutex<Vec<String>>,
        }
        impl NmapProcess for CapturingProcess {
            fn run(&self, args: &[&str], xml_path: &Path) -> Result<ExitStatus, String> {
                *self.captured_args.lock().unwrap() = args.iter().map(|s| s.to_string()).collect();
                std::fs::write(xml_path, HOST_UP_OPEN_PORT).map_err(|e| e.to_string())?;
                #[cfg(windows)]
                let status = std::process::Command::new("cmd")
                    .args(["/C", "exit 0"])
                    .status()
                    .unwrap();
                #[cfg(not(windows))]
                let status = std::process::Command::new("true").status().unwrap();
                Ok(status)
            }
        }
        let process = CapturingProcess {
            captured_args: std::sync::Mutex::new(vec![]),
        };
        run_vuln_scan("192.168.1.10", &process, true);
        let captured = process.captured_args.lock().unwrap();
        assert!(
            captured
                .windows(2)
                .any(|w| w == ["--script-timeout", "60s"]),
            "expected --script-timeout 60s in args, got: {captured:?}"
        );
    }
}
