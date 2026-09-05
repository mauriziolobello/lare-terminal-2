// crates/orchestrator/src/ping.rs
//! # ping — il built-in `/ping` (spec §3.1): una riga per strato
//!
//! Misura ciò che si può misurare davvero: l'orchestratore (uptime),
//! plugin-ping (una sonda Init→Ready usa-e-getta, `plugins::host::probe`),
//! `ui.exe` (`UiPing`→`UiPong` con timeout). La riga di `lare-shell` viene
//! dalla `Hello` (versione dichiarata + `session_id`). Ogni strato assente
//! degrada a una riga `--`; il comando finisce comunque con `Done`.
//!
//! Le sonde arrivano già risolte (`Option<Result<…>>`): il chiamante
//! (`ws.rs`) le esegue con le sue dipendenze, questo modulo formatta. Così è
//! testabile senza plugin host né registro.

use std::time::Duration;

/// Tempo massimo di attesa dell'`UiPong` (usato da `ws.rs`).
pub const UI_PING_TIMEOUT: Duration = Duration::from_secs(2);

/// `5s`, `1m01s`, `1h12m`, `1d01h`: due unità al massimo.
pub fn format_uptime(d: Duration) -> String {
    let s = d.as_secs();
    let (days, hours, mins, secs) = (s / 86_400, (s % 86_400) / 3_600, (s % 3_600) / 60, s % 60);
    if days > 0 {
        format!("{days}d{hours:02}h")
    } else if hours > 0 {
        format!("{hours}h{mins:02}m")
    } else if mins > 0 {
        format!("{mins}m{secs:02}s")
    } else {
        format!("{secs}s")
    }
}

/// Una riga della tabella: `(strato, versione, stato, dettaglio)`.
pub struct LayerReport {
    pub layer: &'static str,
    pub version: String,
    pub status: &'static str,
    pub detail: String,
}

fn probed(
    layer: &'static str,
    probe: Option<Result<(String, Duration), String>>,
    missing: &'static str,
    ok_detail: &dyn Fn(Duration) -> String,
) -> LayerReport {
    match probe {
        Some(Ok((version, elapsed))) => LayerReport {
            layer,
            version,
            status: "ok",
            detail: ok_detail(elapsed),
        },
        Some(Err(reason)) => LayerReport {
            layer,
            version: "--".into(),
            status: "errore",
            detail: reason,
        },
        None => LayerReport {
            layer,
            version: "--".into(),
            status: missing,
            detail: String::new(),
        },
    }
}

/// Markdown del report (spec §3.1). `plugin`/`ui`: `None` = strato non
/// presente (plugin non scoperto / `ui.exe` non connesso); `Some(Err)` =
/// presente ma in errore (con motivo); `Some(Ok((versione, tempo)))`.
pub fn run_ping(
    session_id: &str,
    shell_version: Option<&str>,
    plugin: Option<Result<(String, Duration), String>>,
    ui: Option<Result<(String, Duration), String>>,
) -> String {
    let rows = [
        LayerReport {
            layer: "lare-shell",
            version: shell_version.unwrap_or("--").to_string(),
            status: "ok",
            detail: format!("sessione {session_id}"),
        },
        LayerReport {
            layer: "orchestrator",
            version: env!("CARGO_PKG_VERSION").to_string(),
            status: "ok",
            detail: format!("uptime {}", format_uptime(crate::PROCESS_START.elapsed())),
        },
        probed("plugin-ping", plugin, "non trovato", &|e| {
            format!("round-trip {} ms", e.as_millis())
        }),
        probed("ui.exe", ui, "non connesso", &|e| {
            format!("{} ms", e.as_millis())
        }),
    ];
    let mut md = String::from("| strato | versione | stato | dettaglio |\n|---|---|---|---|\n");
    for r in rows {
        md.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            r.layer, r.version, r.status, r.detail
        ));
    }
    md
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uptime_is_compact_and_human() {
        assert_eq!(format_uptime(Duration::from_secs(5)), "5s");
        assert_eq!(format_uptime(Duration::from_secs(61)), "1m01s");
        assert_eq!(format_uptime(Duration::from_secs(4320)), "1h12m");
        assert_eq!(format_uptime(Duration::from_secs(90000)), "1d01h");
    }

    #[test]
    fn report_has_one_row_per_layer_in_order() {
        let md = run_ping(
            "a1b2",
            Some("2.0.0"),
            Some(Ok(("1.0.0".into(), Duration::from_millis(8)))),
            Some(Ok(("2.1.0".into(), Duration::from_millis(12)))),
        );
        let rows: Vec<&str> = md
            .lines()
            .filter(|l| l.starts_with("| ") && !l.starts_with("| strato"))
            .collect();
        assert_eq!(rows.len(), 4, "{md}");
        assert!(
            rows[0].starts_with("| lare-shell | 2.0.0 | ok | sessione a1b2"),
            "{}",
            rows[0]
        );
        assert!(rows[1].starts_with("| orchestrator | "), "{}", rows[1]);
        assert!(
            rows[1].contains(env!("CARGO_PKG_VERSION")) && rows[1].contains("uptime "),
            "{}",
            rows[1]
        );
        assert!(
            rows[2].starts_with("| plugin-ping | 1.0.0 | ok | round-trip 8 ms"),
            "{}",
            rows[2]
        );
        assert!(
            rows[3].starts_with("| ui.exe | 2.1.0 | ok | 12 ms"),
            "{}",
            rows[3]
        );
    }

    #[test]
    fn missing_layers_degrade_to_dashes_and_errors_are_shown() {
        let md = run_ping("s", None, Some(Err("spawn fallito: x".into())), None);
        assert!(md.contains("| lare-shell | -- | ok | sessione s"), "{md}");
        assert!(
            md.contains("| plugin-ping | -- | errore | spawn fallito: x"),
            "{md}"
        );
        assert!(md.contains("| ui.exe | -- | non connesso |"), "{md}");
        let md2 = run_ping("s", None, None, None);
        assert!(md2.contains("| plugin-ping | -- | non trovato |"), "{md2}");
    }
}
