//! # markdown — `ScanReport` → human-readable Markdown + LLM summary
//!
//! Two audiences, two functions (design spec §6): `format_report_markdown`
//! produces the FULL report for the deterministic Markdown-window/Library
//! save (never sent to the model); `format_summary` produces a few lines for
//! the LLM's tool_result (kept brief, same spirit as `agent::truncate_for_model`
//! in the orchestrator — this crate has no dependency on the orchestrator, so
//! it keeps its own summary short by construction instead).

use crate::report::ScanReport;

/// Full Markdown report: one section per host, ports as a list, OS matches
/// if present. Deterministic (byte-for-byte for a given `ScanReport`) so
/// snapshot-style assertions are meaningful in tests.
pub fn format_report_markdown(report: &ScanReport) -> String {
    let mut out = format!("# Scansione nmap — {}\n\n", report.target);
    if report.hosts.is_empty() {
        out.push_str("Nessun host nel risultato.\n");
        return out;
    }
    for host in &report.hosts {
        out.push_str(&format!("## Host {}\n\n", host.address));
        out.push_str(&format!("**Stato:** {}\n\n", if host.up { "up" } else { "down" }));
        if !host.up {
            continue;
        }
        if !host.ports.is_empty() {
            out.push_str("**Porte:**\n\n");
            for port in &host.ports {
                let service = port.service.as_deref().unwrap_or("sconosciuto");
                out.push_str(&format!(
                    "- {}/{} — {} ({service})\n",
                    port.port, port.protocol, port.state
                ));
            }
            out.push('\n');
        }
        // NSE script output (e.g. `nmap_vuln_scan`'s `--script vuln`) — the
        // fix for the bug where a vuln-scan report window was byte-identical
        // to a quick-scan report window for the same target, because nothing
        // in this function ever read `PortReport::scripts`/
        // `HostReport::host_scripts` at all.
        let ports_with_scripts: Vec<&crate::report::PortReport> = host
            .ports
            .iter()
            .filter(|p| !p.scripts.is_empty())
            .collect();
        if !ports_with_scripts.is_empty() {
            out.push_str("**Risultati script per porta:**\n\n");
            for port in &ports_with_scripts {
                out.push_str(&format!("- {}/{}:\n\n```\n", port.port, port.protocol));
                for script in &port.scripts {
                    out.push_str(&format!("{}: {}\n", script.id, script.output));
                }
                out.push_str("```\n\n");
            }
        }
        if !host.host_scripts.is_empty() {
            out.push_str("**Risultati script a livello host:**\n\n```\n");
            for script in &host.host_scripts {
                out.push_str(&format!("{}: {}\n", script.id, script.output));
            }
            out.push_str("```\n\n");
        }
        if report.os_detection {
            if host.os_matches.is_empty() {
                out.push_str("**Rilevamento OS:** nessuna corrispondenza sicura.\n\n");
            } else {
                out.push_str("**Rilevamento OS (più probabile prima):**\n\n");
                for m in &host.os_matches {
                    out.push_str(&format!("- {m}\n"));
                }
                out.push('\n');
            }
        }
    }
    out
}

/// Brief summary for the LLM's tool_result — host count, up/down, open-port
/// count, and (when any NSE script ran, e.g. `nmap_vuln_scan`) a script
/// result count. Never includes the per-port service detail or the raw
/// script output itself (that's in the full report only) — keeps the
/// model's context small per tool call.
///
/// **Why a count, not a verdict:** this function deliberately does NOT try
/// to classify script results as "vulnerable"/"not vulnerable" — nmap's own
/// NSE scripts format their `output` text too inconsistently for that to be
/// reliable. Before this fix, `format_summary` told the model nothing at
/// all about whether any script ran, so the model was narrating
/// "vulnerabilità trovate" based on nothing but a port count; now it gets a
/// real (if coarse) signal and points at the Markdown report for detail,
/// same spirit as the existing port-count sentence.
pub fn format_summary(report: &ScanReport) -> String {
    let up_hosts: Vec<&crate::report::HostReport> = report.hosts.iter().filter(|h| h.up).collect();
    if up_hosts.is_empty() {
        return format!("nmap: {} — nessun host attivo trovato.", report.target);
    }
    let open_ports: usize = up_hosts
        .iter()
        .flat_map(|h| h.ports.iter())
        .filter(|p| p.state == "open")
        .count();
    let script_count: usize = up_hosts
        .iter()
        .map(|h| h.host_scripts.len() + h.ports.iter().map(|p| p.scripts.len()).sum::<usize>())
        .sum();
    if script_count > 0 {
        format!(
            "nmap: {} — {} host attivo/i, {} porta/e aperta/e, {} risultato/i script NSE. Report completo nella finestra Markdown.",
            report.target,
            up_hosts.len(),
            open_ports,
            script_count
        )
    } else {
        format!(
            "nmap: {} — {} host attivo/i, {} porta/e aperta/e. Report completo nella finestra Markdown.",
            report.target,
            up_hosts.len(),
            open_ports
        )
    }
}

/// Etichetta di trasparenza/banner-di-conferma per un tool_use nmap (design
/// spec §5). `nmap_os_detect` dichiara ESPLICITAMENTE "richiede privilegi
/// elevati" — la dialog UAC di Windows non mostra il target, quindi questo è
/// l'unico punto dove l'utente lo vede prima dell'elevazione.
pub fn format_nmap_invocation(tool_name: &str, args: &serde_json::Value) -> String {
    let target = args.get("target").and_then(|v| v.as_str()).unwrap_or("?");
    match tool_name {
        "nmap_quick_scan" => format!("nmap quick scan → {target}"),
        "nmap_os_detect" => format!("nmap OS detect (richiede privilegi elevati) → {target}"),
        "nmap_version_scan" => format!("nmap version scan (-sV) → {target}"),
        "nmap_host_discovery" => format!("nmap host discovery (-sn) → {target}"),
        "nmap_vuln_scan" => format!("nmap scansione vulnerabilità (--script vuln) → {target}"),
        "local_network_info" => "informazioni di rete locali".to_string(),
        "traceroute" => format!("traceroute → {target}"),
        other => format!("[tool {other}]"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{HostReport, PortReport};

    fn sample_report(os_detection: bool) -> ScanReport {
        ScanReport {
            target: "192.168.1.10".to_string(),
            os_detection,
            hosts: vec![HostReport {
                address: "192.168.1.10".to_string(),
                up: true,
                ports: vec![
                    PortReport {
                        port: 22,
                        protocol: "tcp".to_string(),
                        state: "open".to_string(),
                        service: Some("ssh".to_string()),
                        scripts: vec![],
                    },
                    PortReport {
                        port: 80,
                        protocol: "tcp".to_string(),
                        state: "closed".to_string(),
                        service: None,
                        scripts: vec![],
                    },
                ],
                os_matches: if os_detection {
                    vec!["Linux 5.X".to_string()]
                } else {
                    vec![]
                },
                host_scripts: vec![],
            }],
        }
    }

    /// A report shaped like `nmap_vuln_scan`'s real output: one port-level
    /// script result AND one host-level script result — used to prove
    /// `format_report_markdown`/`format_summary` are script-aware (the fix
    /// for the bug where a vuln-scan report window was byte-identical to a
    /// quick-scan report window for the same target).
    fn sample_report_with_scripts() -> ScanReport {
        use crate::report::ScriptResult;
        ScanReport {
            target: "192.168.178.1".to_string(),
            os_detection: false,
            hosts: vec![HostReport {
                address: "192.168.178.1".to_string(),
                up: true,
                ports: vec![PortReport {
                    port: 80,
                    protocol: "tcp".to_string(),
                    state: "open".to_string(),
                    service: Some("http".to_string()),
                    scripts: vec![ScriptResult {
                        id: "http-csrf".to_string(),
                        output: "Couldn't find any CSRF vulnerabilities.".to_string(),
                    }],
                }],
                os_matches: vec![],
                host_scripts: vec![ScriptResult {
                    id: "smb-vuln-ms10-054".to_string(),
                    output: "false".to_string(),
                }],
            }],
        }
    }

    #[test]
    fn markdown_report_includes_target_host_and_ports() {
        let md = format_report_markdown(&sample_report(false));
        assert!(md.contains("192.168.1.10"));
        assert!(md.contains("22/tcp"));
        assert!(md.contains("ssh"));
        assert!(md.contains("80/tcp"));
        assert!(!md.contains("Rilevamento OS"), "quick scan non deve menzionare l'OS");
    }

    #[test]
    fn markdown_report_includes_os_matches_when_os_detection_true() {
        let md = format_report_markdown(&sample_report(true));
        assert!(md.contains("Rilevamento OS"));
        assert!(md.contains("Linux 5.X"));
    }

    #[test]
    fn markdown_report_down_host_has_no_ports_section() {
        let mut report = sample_report(false);
        report.hosts[0].up = false;
        report.hosts[0].ports.clear();
        let md = format_report_markdown(&report);
        assert!(md.contains("down"));
        assert!(!md.contains("Porte:"));
    }

    #[test]
    fn markdown_report_empty_hosts_says_so() {
        let report = ScanReport { target: "10.0.0.1".to_string(), os_detection: false, hosts: vec![] };
        let md = format_report_markdown(&report);
        assert!(md.contains("Nessun host"));
    }

    #[test]
    fn markdown_report_includes_per_port_script_output() {
        let md = format_report_markdown(&sample_report_with_scripts());
        assert!(md.contains("Risultati script per porta"));
        assert!(md.contains("http-csrf"));
        assert!(md.contains("Couldn't find any CSRF vulnerabilities."));
    }

    #[test]
    fn markdown_report_includes_host_level_script_output() {
        let md = format_report_markdown(&sample_report_with_scripts());
        assert!(md.contains("Risultati script a livello host"));
        assert!(md.contains("smb-vuln-ms10-054"));
    }

    #[test]
    fn markdown_report_without_scripts_has_no_script_sections() {
        let md = format_report_markdown(&sample_report(false));
        assert!(!md.contains("Risultati script"));
    }

    #[test]
    fn summary_without_scripts_matches_existing_wording_exactly() {
        // Regression guard: a plain quick_scan/version_scan/host_discovery
        // summary (no scripts ran) must be byte-identical to today's wording —
        // only vuln-scan-shaped reports (scripts present) get the new clause.
        let summary = format_summary(&sample_report(false));
        assert_eq!(
            summary,
            "nmap: 192.168.1.10 — 1 host attivo/i, 1 porta/e aperta/e. Report completo nella finestra Markdown."
        );
    }

    #[test]
    fn summary_with_scripts_reports_script_count_not_a_vulnerable_verdict() {
        let summary = format_summary(&sample_report_with_scripts());
        assert!(summary.contains("risultato/i script NSE"));
        assert!(
            summary.contains('2'),
            "1 port script + 1 host script = 2: {summary}"
        );
        // Must NOT claim to have classified anything as vulnerable/not —
        // format_summary cannot reliably parse arbitrary NSE script output.
        assert!(!summary.to_lowercase().contains("vulnerabile"));
    }

    #[test]
    fn summary_counts_up_hosts_and_open_ports() {
        let summary = format_summary(&sample_report(false));
        assert!(summary.contains('1'), "1 host attivo: {summary}");
        assert!(summary.contains("192.168.1.10"));
    }

    #[test]
    fn summary_zero_hosts_says_none_active_not_an_error() {
        let report = ScanReport {
            target: "10.0.0.99".to_string(),
            os_detection: false,
            hosts: vec![HostReport {
                address: "10.0.0.99".to_string(),
                up: false,
                ports: vec![],
                os_matches: vec![],
                host_scripts: vec![],
            }],
        };
        let summary = format_summary(&report);
        assert!(summary.contains("nessun host attivo"));
    }

    #[test]
    fn invocation_label_quick_scan() {
        let label = format_nmap_invocation("nmap_quick_scan", &serde_json::json!({"target": "10.0.0.5"}));
        assert_eq!(label, "nmap quick scan → 10.0.0.5");
    }

    #[test]
    fn invocation_label_os_detect_mentions_elevation() {
        let label = format_nmap_invocation("nmap_os_detect", &serde_json::json!({"target": "10.0.0.5"}));
        assert!(label.contains("richiede privilegi elevati"));
        assert!(label.contains("10.0.0.5"));
    }

    #[test]
    fn invocation_label_unknown_tool_falls_back() {
        let label = format_nmap_invocation("nope", &serde_json::json!({}));
        assert_eq!(label, "[tool nope]");
    }

    #[test]
    fn invocation_label_local_network_info_has_no_target() {
        let label = format_nmap_invocation("local_network_info", &serde_json::json!({}));
        assert_eq!(label, "informazioni di rete locali");
    }

    #[test]
    fn invocation_label_traceroute_shows_target() {
        let label = format_nmap_invocation("traceroute", &serde_json::json!({"target": "8.8.8.8"}));
        assert_eq!(label, "traceroute → 8.8.8.8");
    }

    #[test]
    fn invocation_label_version_scan_shows_target() {
        let label = format_nmap_invocation(
            "nmap_version_scan",
            &serde_json::json!({"target": "10.0.0.5"}),
        );
        assert_eq!(label, "nmap version scan (-sV) → 10.0.0.5");
    }

    #[test]
    fn invocation_label_host_discovery_shows_target() {
        let label = format_nmap_invocation(
            "nmap_host_discovery",
            &serde_json::json!({"target": "192.168.1.0/24"}),
        );
        assert_eq!(label, "nmap host discovery (-sn) → 192.168.1.0/24");
    }

    #[test]
    fn invocation_label_vuln_scan_names_the_script_category() {
        let label = format_nmap_invocation(
            "nmap_vuln_scan",
            &serde_json::json!({"target": "192.168.178.1"}),
        );
        assert_eq!(
            label,
            "nmap scansione vulnerabilità (--script vuln) → 192.168.178.1"
        );
    }
}
