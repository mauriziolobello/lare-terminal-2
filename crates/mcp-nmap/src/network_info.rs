//! # network_info — comandi diagnostici di rete built-in del SO
//!
//! A differenza di `scan.rs` (che invoca `nmap`, un binario esterno da
//! installare), questo modulo invoca comandi built-in del SO
//! (`ipconfig`/`arp`/`route`/`netstat`/`tracert` su Windows) — nessuna
//! dipendenza da nmap. Stesso principio di isolamento del resto del crate:
//! tool fissi, mai una shell arbitraria — l'AI non sceglie QUALI comandi
//! girano, solo QUANDO invocare questi due tool fissi.
//!
//! ## Perché niente `report_markdown` qui (a differenza di `ScanOutcome`)
//! Un report di scan (§6 del design spec) è per l'UMANO: il modello vede un
//! riassunto breve, il dettaglio va in una finestra + Library. Qui è
//! l'opposto: l'AI ha BISOGNO di leggere l'IP/subnet/gateway per decidere
//! quale target passare a `nmap_quick_scan` — nasconderglielo dietro un
//! summary vago vanificherebbe lo scopo stesso di questi due tool. Quindi
//! `output` va per intero al modello, niente finestra, niente Library.

use crate::scan::validate_target;

/// Esito di `local_network_info`/`traceroute` — forma JSON diversa da
/// `ScanOutcome` (niente `report_markdown`/`summary`): qui c'è solo
/// `output`, mandato per intero al modello (vedi doc-comment del modulo).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct NetworkInfoOutcome {
    pub output: String,
    pub is_error: bool,
}

/// Esegue `exe args...` e cattura stdout (+ stderr se non vuoto, in coda,
/// marcato). Non usa `NmapProcess` (quel trait modella "un processo che
/// scrive un file -oX e ritorna un exit status" — questi comandi non
/// scrivono file, ritornano output diretto): mockare qui aggiungerebbe
/// un'astrazione per un solo chiamante, YAGNI.
/// `String::from_utf8_lossy` qui riproduce il mojibake noto su etichette
/// accentate (`Sì` → `S�`) documentato in `Docs/KNOWN-ISSUES.md` ("Codepage
/// — output dei comandi NATIVI"): i comandi nativi emettono byte nel
/// codepage OEM (CP850/437 su Windows italiano), non UTF-8. Non risolto
/// qui deliberatamente: IP/subnet/gateway/MAC (i dati che servono
/// all'AI per scegliere un target) sono ASCII e non ne risentono — solo
/// le etichette italiane si corrompono. La fix vera (decodifica OEM o
/// `chcp` soppressa) è un problema app-wide che tocca anche
/// `mcp-server/src/session.rs`, fuori scope per questo tool.
fn run_and_capture(exe: &str, args: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new(exe)
        .args(args)
        .output()
        .map_err(|e| format!("esecuzione di {exe} fallita: {e}"))?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    if !output.stderr.is_empty() {
        text.push_str("\n[stderr]\n");
        text.push_str(&String::from_utf8_lossy(&output.stderr));
    }
    Ok(text)
}

#[cfg(windows)]
mod windows_impl {
    use super::{run_and_capture, validate_target, NetworkInfoOutcome};

    /// Le 4 fonti fisse di `local_network_info`, in quest'ordine (dal più
    /// generale — indirizzi e interfacce — al più specifico — routing).
    const COMMANDS: [(&str, &[&str]); 4] = [
        ("ipconfig", &["/all"]),
        ("arp", &["-a"]),
        ("route", &["print"]),
        ("netstat", &["-rn"]),
    ];

    pub fn local_network_info() -> NetworkInfoOutcome {
        let mut report = String::new();
        let mut any_succeeded = false;
        for (exe, args) in COMMANDS {
            report.push_str(&format!("## {exe} {}\n\n", args.join(" ")));
            match run_and_capture(exe, args) {
                Ok(text) => {
                    any_succeeded = true;
                    report.push_str("```\n");
                    report.push_str(text.trim());
                    report.push_str("\n```\n\n");
                }
                Err(e) => {
                    report.push_str(&format!("_Errore: {e}_\n\n"));
                }
            }
        }
        // is_error solo se TUTTI i comandi sono falliti — un fallimento
        // parziale (es. `route` assente su una build Windows atipica) lascia
        // comunque informazioni utili nel report, non è un errore di tool.
        NetworkInfoOutcome {
            output: report,
            is_error: !any_succeeded,
        }
    }

    pub fn traceroute(target: &str) -> NetworkInfoOutcome {
        if let Err(e) = validate_target(target) {
            return NetworkInfoOutcome {
                output: e,
                is_error: true,
            };
        }
        // -d: non risolvere hostname (più veloce, evita attese DNS per hop
        // filtrati). -w 1000: 1000ms di timeout per hop, invece del default
        // di tracert (spesso alcuni secondi per hop) — un traceroute con
        // molti hop filtrati altrimenti richiederebbe minuti.
        match run_and_capture("tracert", &["-d", "-w", "1000", target]) {
            Ok(text) => NetworkInfoOutcome {
                output: text,
                is_error: false,
            },
            Err(e) => NetworkInfoOutcome {
                output: e,
                is_error: true,
            },
        }
    }
}

#[cfg(windows)]
pub use windows_impl::{local_network_info, traceroute};

#[cfg(not(windows))]
pub fn local_network_info() -> NetworkInfoOutcome {
    NetworkInfoOutcome {
        output: "informazioni di rete locali non ancora supportate su questa piattaforma"
            .to_string(),
        is_error: true,
    }
}

#[cfg(not(windows))]
pub fn traceroute(_target: &str) -> NetworkInfoOutcome {
    NetworkInfoOutcome {
        output: "traceroute non ancora supportato su questa piattaforma".to_string(),
        is_error: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_info_outcome_json_roundtrips() {
        let outcome = NetworkInfoOutcome {
            output: "test".to_string(),
            is_error: false,
        };
        let json = serde_json::to_string(&outcome).unwrap();
        let back: NetworkInfoOutcome = serde_json::from_str(&json).unwrap();
        assert_eq!(back, outcome);
    }

    #[test]
    fn traceroute_rejects_invalid_target_before_spawning_anything() {
        // "--script=vuln" starts with '-': validate_target must reject it
        // before tracert is ever invoked — same principle as scan.rs's own
        // target-validation gate (argv flag-smuggling defense).
        let outcome = traceroute("--script=vuln");
        assert!(outcome.is_error);
        assert!(outcome.output.contains("target non valido") || outcome.output.contains("'-'"));
    }

    #[test]
    fn traceroute_rejects_target_with_whitespace() {
        let outcome = traceroute("192.168.1.10 extra");
        assert!(outcome.is_error);
    }

    #[cfg(not(windows))]
    #[test]
    fn non_windows_stubs_are_readable_errors_not_panics() {
        let info = local_network_info();
        assert!(info.is_error);
        assert!(info.output.contains("piattaforma"));
        let tr = traceroute("192.168.1.1");
        assert!(tr.is_error);
        assert!(tr.output.contains("piattaforma"));
    }
}
