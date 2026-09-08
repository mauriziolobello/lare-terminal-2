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

/// Rileva il codepage di output attivo per decodificare correttamente l'output
/// dei comandi diagnostici nativi di Windows.
///
/// Tenta prima di leggere il codepage associato alla console del processo tramite `GetConsoleOutputCP()`.
///
/// **Punto critico (processo senza finestra di console)**:
/// Quando `mcp-nmap.exe` viene avviato dall'orchestratore con flag `CREATE_NO_WINDOW`,
/// non esiste alcuna console allocata e `GetConsoleOutputCP()` restituisce `0`.
/// In questo caso, la funzione ripiega su `GetOEMCP()`, che interroga il codepage OEM
/// di default configurato a livello di sistema operativo Windows (es. CP850 in Europa occidentale/Italia,
/// CP437 per installazioni US), garantendo una decodifica corretta anche in produzione.
#[cfg(windows)]
fn active_console_output_codepage() -> u32 {
    // SAFETY: GetConsoleOutputCP è una chiamata Win32 pura di interrogazione,
    // senza puntatori o parametri da validare.
    let cp = unsafe { windows::Win32::System::Console::GetConsoleOutputCP() };
    if cp != 0 {
        return cp;
    }
    // SAFETY: GetOEMCP è una chiamata Win32 pura che ritorna il codepage OEM di sistema.
    unsafe { windows::Win32::Globalization::GetOEMCP() }
}

/// Decodifica una sequenza di byte emessa da un processo nativo Win32 usando il
/// codepage specificato (tipicamente OEM, es. CP850 su Windows italiano o CP437 su Windows US).
///
/// Se il `codepage` (rappresentato come `u32` coerentemente con i tipi di ritorno Win32)
/// è convertibile in `u16` ed è presente nella mappa delle tabelle OEM (`oem_cp::code_table::DECODING_TABLE_CP_MAP`),
/// la sequenza di byte viene decodificata tramite la tabella corrispondente (`decode_string_lossy`,
/// che mappa eventuali byte non definiti nel carattere di sostituzione U+FFFD).
///
/// Se il codepage non è presente (es. numero sconosciuto/non OEM) o eccede `u16::MAX`,
/// la funzione ripiega fedelmente su `String::from_utf8_lossy(bytes)` (lo stesso comportamento
/// storico pre-fix), garantendo che i dati ASCII (IP, MAC, gateway, subnet) rimangano leggibili
/// e che non si verifichino mai panic o errori fatali.
pub(crate) fn decode_oem(bytes: &[u8], codepage: u32) -> String {
    // La tabella `oem_cp::code_table::DECODING_TABLE_CP_MAP` è indicizzata per chiave `u16`,
    // mentre le API Win32 ritornano `u32`. Convertiamo esplicitamente con `try_from` gestendo
    // l'eventuale overflow senza troncare silenziosamente con un cast cieco `as u16`.
    if let Ok(cp16) = u16::try_from(codepage) {
        if let Some(table) = oem_cp::code_table::DECODING_TABLE_CP_MAP.get(&cp16) {
            return table.decode_string_lossy(bytes);
        }
    }
    // Fallback contrattuale non-negoziabile: se il codepage non è noto alle tabelle OEM,
    // si ripiega esattamente su `String::from_utf8_lossy`.
    String::from_utf8_lossy(bytes).into_owned()
}

/// Esegue `exe args...` e cattura stdout (+ stderr se non vuoto, in coda, marcato).
/// Non usa `NmapProcess` (quel trait modella "un processo che scrive un file -oX e
/// ritorna un exit status" — questi comandi non scrivono file, ritornano output diretto):
/// mockare qui aggiungerebbe un'astrazione per un solo chiamante, YAGNI.
///
/// I comandi diagnostici di rete nativi di Windows (`ipconfig`, `arp`, `route`, `netstat`, `tracert`)
/// emettono byte nel codepage OEM della console (CP850 in Italia, CP437 su Windows US, ecc.),
/// non in UTF-8. Su Windows la decodifica dei flussi stdout/stderr interroga il codepage reale
/// (`active_console_output_codepage()`) e lo decodifica con `decode_oem`, risolvendo il problema
/// del mojibake sulle etichette accentate (es. "Sì" invece di "S").
/// Su piattaforme non-Windows viene mantenuto il fallback standard su UTF-8 lossy.
fn run_and_capture(exe: &str, args: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new(exe)
        .args(args)
        .output()
        .map_err(|e| format!("esecuzione di {exe} fallita: {e}"))?;

    #[cfg(windows)]
    let decode = |bytes: &[u8]| -> String {
        let cp = active_console_output_codepage();
        decode_oem(bytes, cp)
    };

    #[cfg(not(windows))]
    let decode = |bytes: &[u8]| -> String { String::from_utf8_lossy(bytes).into_owned() };

    let mut text = decode(&output.stdout);
    if !output.stderr.is_empty() {
        text.push_str("\n[stderr]\n");
        text.push_str(&decode(&output.stderr));
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

    #[test]
    fn decode_oem_decodifica_correttamente_accentate_cp850() {
        // "Sì" in CP850 (Europa occidentale) = byte [0x53, 0x8D]
        assert_eq!(decode_oem(&[0x53, 0x8D], 850), "Sì");

        // "è" da sola, CP850 = byte [0x8A]
        assert_eq!(decode_oem(&[0x8A], 850), "è");

        // "città" per intero, CP850 = byte [0x63, 0x69, 0x74, 0x74, 0x85]
        assert_eq!(decode_oem(&[0x63, 0x69, 0x74, 0x74, 0x85], 850), "città");
    }

    #[test]
    fn decode_oem_codepage_sconosciuta_ripiega_su_utf8_lossy() {
        // Codepage sconosciuta (numero inventato, non in nessuna tabella OEM reale):
        // il CONTRATTO impone di ripiegare sullo STESSO comportamento di oggi
        // (String::from_utf8_lossy), non deve mai panicare né inventare un codepage di default.
        // Fallback per [0x53, 0x8D] è "S\u{FFFD}".
        assert_eq!(decode_oem(&[0x53, 0x8D], 999999), "S\u{FFFD}");
    }

    #[test]
    fn decode_oem_preserva_invariati_i_dati_ascii_puri() {
        // I dati di rete (IP, subnet, gateway, MAC) sono ASCII puro e devono passare
        // invariati per qualunque codepage.
        assert_eq!(decode_oem(b"192.168.1.1", 850), "192.168.1.1");
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
