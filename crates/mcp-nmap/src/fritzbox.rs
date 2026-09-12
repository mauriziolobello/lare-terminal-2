//! # fritzbox — stato del router FRITZ!Box via script Python (fritzconnection)
//!
//! Stesso principio isolativo di `network_info.rs`: un tool fisso, un solo
//! shell-out one-shot, nessuna shell arbitraria. A differenza dei comandi nativi
//! Win32 di `network_info.rs`, qui il sotto-processo è uno script Python
//! (`scripts/pytools/fritzbox/fritzbox_status.py`) che emette UTF-8 direttamente
//! (invocato con `-X utf8`) — nessuna decodifica OEM necessaria.

use crate::network_info::NetworkInfoOutcome;
use std::path::Path;

#[derive(serde::Deserialize)]
struct FritzScriptJson {
    output: String,
    is_error: bool,
}

/// Interpreta l'esito grezzo del sotto-processo Python. Pura, senza I/O —
/// testabile passando stringhe dirette, senza spawnare nulla (mirror di
/// `network_info::decode_oem`, testato allo stesso modo).
pub(crate) fn parse_script_output(
    stdout: &str,
    stderr: &str,
    exit_success: bool,
) -> NetworkInfoOutcome {
    if !exit_success {
        return NetworkInfoOutcome {
            output: format!("script fritzbox terminato con errore.\nstderr:\n{stderr}"),
            is_error: true,
        };
    }
    match serde_json::from_str::<FritzScriptJson>(stdout.trim()) {
        Ok(v) => NetworkInfoOutcome {
            output: v.output,
            is_error: v.is_error,
        },
        Err(e) => NetworkInfoOutcome {
            output: format!(
                "output dello script fritzbox non interpretabile ({e}); output grezzo:\n{stdout}"
            ),
            is_error: true,
        },
    }
}

/// Shell-out one-shot: `<python_path> -X utf8 <script_path> --config-dir <config_dir>`.
/// Verifica prima che python/script esistano (messaggio leggibile, stesso stile di
/// `PythonMcpToolClient::resolve` — vedi crates/orchestrator/src/python_mcp_tool_client.rs)
/// invece di lasciare che lo spawn fallisca con un errore OS opaco.
pub async fn fritzbox_status(
    python_path: &Path,
    script_path: &Path,
    config_dir: &Path,
) -> NetworkInfoOutcome {
    if !python_path.exists() {
        return NetworkInfoOutcome {
            output: format!(
                "venv Python non trovato per \"fritzbox\": {} — crea il virtual environment \
                 (vedi scripts/pytools/fritzbox/README.md)",
                python_path.display()
            ),
            is_error: true,
        };
    }
    if !script_path.exists() {
        return NetworkInfoOutcome {
            output: format!("script fritzbox non trovato: {}", script_path.display()),
            is_error: true,
        };
    }

    let mut cmd = tokio::process::Command::new(python_path);
    cmd.arg("-X")
        .arg("utf8")
        .arg(script_path)
        .arg("--config-dir")
        .arg(config_dir);
    cmd.stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW — stesso motivo di NmapToolClient
    }

    // Timeout dedicato: un router irraggiungibile non deve far attendere il
    // timeout esterno da 900s del canale (NMAP_CALL_TIMEOUT_SECS) — 30s bastano
    // ampiamente per una chiamata TR-064 locale.
    let spawn_and_wait = async {
        let child = cmd.output();
        child.await
    };
    match tokio::time::timeout(std::time::Duration::from_secs(30), spawn_and_wait).await {
        Ok(Ok(out)) => parse_script_output(
            &String::from_utf8_lossy(&out.stdout),
            &String::from_utf8_lossy(&out.stderr),
            out.status.success(),
        ),
        Ok(Err(e)) => NetworkInfoOutcome {
            output: format!("esecuzione dello script fritzbox fallita: {e}"),
            is_error: true,
        },
        Err(_) => NetworkInfoOutcome {
            output: "timeout (30s) in attesa dello script fritzbox — router irraggiungibile?"
                .to_string(),
            is_error: true,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_script_output_passes_through_valid_json() {
        let out = parse_script_output(r#"{"output":"tutto ok","is_error":false}"#, "", true);
        assert_eq!(out.output, "tutto ok");
        assert!(!out.is_error);
    }

    #[test]
    fn parse_script_output_reports_error_flag_from_script() {
        let out = parse_script_output(
            r#"{"output":"router irraggiungibile","is_error":true}"#,
            "",
            true,
        );
        assert!(out.is_error);
        assert_eq!(out.output, "router irraggiungibile");
    }

    #[test]
    fn parse_script_output_handles_garbage_stdout() {
        let out = parse_script_output("questo non e' JSON", "", true);
        assert!(out.is_error);
        assert!(out.output.contains("non interpretabile"));
        assert!(out.output.contains("questo non e' JSON"));
    }

    #[test]
    fn parse_script_output_surfaces_nonzero_exit() {
        let out = parse_script_output("", "Traceback...", false);
        assert!(out.is_error);
        assert!(out.output.contains("Traceback"));
    }

    #[tokio::test]
    async fn fritzbox_status_errs_readably_when_venv_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let out = fritzbox_status(
            &tmp.path().join("venv/Scripts/python.exe"),
            &tmp.path().join("fritzbox_status.py"),
            tmp.path(),
        )
        .await;
        assert!(out.is_error);
        assert!(out.output.contains("venv"));
    }

    #[tokio::test]
    async fn fritzbox_status_errs_readably_when_script_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let fake_python = tmp.path().join("python.exe");
        std::fs::write(&fake_python, b"").unwrap();
        let out = fritzbox_status(&fake_python, &tmp.path().join("missing.py"), tmp.path()).await;
        assert!(out.is_error);
        assert!(out.output.contains("non trovato"));
    }
}
