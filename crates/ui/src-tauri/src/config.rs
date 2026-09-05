// config.rs — Persistent configuration for Lare Terminal.
//
// Responsibility (SRP): owns the Config struct, its defaults, and JSON
// persistence.  Does NOT touch Tauri APIs (those are in main.rs).
//
// File location at runtime: `<config_dir>/config.json` (2.0: `config_dir` is
// `--config-dir` or `<exe_dir>/Configuration`, resolved once via
// `ConfigDirState` — see main.rs's `config_file_path`).
// The full path is built by main.rs and then handed to `load_from` / `save_to`
// here as an injectable &Path — keeping this module testable without a
// running Tauri instance.
//
// Task 7 (piano 1, 2026-09-05): l'overlay F2 è sparito — con lui ogni campo
// che pilotava SOLO il suo aspetto/comportamento (tasto d'attivazione,
// colore/font/dimensione del testo, posizione della finestra, stile
// dell'indicatore di attività, minuti di inattività, e i due enum che li
// tipavano). Restano `web_search_enabled` (letto da ogni finestra che invia
// comandi AI) e `window_alpha` (trasparenza condivisa da tutte le finestre).
// Nessun `#[serde(deny_unknown_fields)]` su questo struct: le chiavi rimosse
// in un `config.json` v1 preesistente vengono ignorate silenziosamente in
// lettura — vedi `legacy_v1_config_json_with_extra_fields_still_loads`.

use serde::{Deserialize, Serialize};
use std::path::Path;

// ---------------------------------------------------------------------------
// Config struct
// ---------------------------------------------------------------------------

/// Persisted user configuration.
///
/// All fields have documented defaults matching `07-ux-and-config.md`.
/// `Default` yields those same values; round-trip through JSON preserves them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    /// Abilita la ricerca web interna dell'AI (tool server-side).
    /// Default: `true`.
    #[serde(default = "default_true")]
    pub web_search_enabled: bool,

    /// Alpha di trasparenza condiviso da ogni sfondo "pannello" di ogni finestra
    /// dell'app (Library, /config, AI Chat, tutti i plugin). Intervallo
    /// [0.0, 1.0]. Default 0.87 (valore storico allineato in 185f304).
    #[serde(default = "default_window_alpha")]
    pub window_alpha: f64,
}

/// Helper per `#[serde(default = "default_true")]`.
fn default_true() -> bool {
    true
}

/// Helper per `#[serde(default = "default_window_alpha")]`.
fn default_window_alpha() -> f64 {
    0.87
}

impl Default for Config {
    fn default() -> Self {
        Self {
            web_search_enabled: true,
            window_alpha: 0.87,
        }
    }
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

/// Load config from `path`.
///
/// Behaviour (infallible to the caller):
/// - File not found  → `Config::default()` (silently).
/// - JSON parse error → `Config::default()` + warning logged to stderr.
/// - File readable and valid → the deserialized config.
///
/// `path` is injectable so this function is fully testable without Tauri.
pub fn load_from(path: &Path) -> Config {
    match std::fs::read_to_string(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Expected on first run — not an error.
            Config::default()
        }
        Err(e) => {
            // Unexpected IO error (permissions, etc.) — default + warning.
            eprintln!("[config] Could not read {:?}: {e} — using defaults", path);
            Config::default()
        }
        Ok(text) => match serde_json::from_str::<Config>(&text) {
            Ok(mut cfg) => {
                // window_alpha è editabile a mano nel JSON — clampiamo qui in
                // lettura così ogni consumatore (frontend, plugin) riceve
                // sempre un valore già dentro [0.0, 1.0], senza doverlo
                // ri-validare ovunque venga letto.
                cfg.window_alpha = cfg.window_alpha.clamp(0.0, 1.0);
                cfg
            }
            Err(e) => {
                eprintln!(
                    "[config] Corrupt config at {:?}: {e} — using defaults",
                    path
                );
                Config::default()
            }
        },
    }
}

/// Persist `config` to `path` as pretty-printed JSON.
///
/// Creates the parent directory if it does not exist.
/// Returns an error string on any IO or serialisation failure.
pub fn save_to(config: &Config, path: &Path) -> Result<(), String> {
    // Create parent dirs if absent.
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create config dir {:?}: {e}", parent))?;
    }

    let json = serde_json::to_string_pretty(config)
        .map_err(|e| format!("cannot serialise config: {e}"))?;

    std::fs::write(path, json).map_err(|e| format!("cannot write {:?}: {e}", path))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests (TDD: written before implementation)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use tempfile::NamedTempFile;

    // ── Round-trip save → load ────────────────────────────────────────────

    #[test]
    fn round_trip_default_config() {
        let tmp = NamedTempFile::new().expect("tempfile");
        let path = tmp.path();

        let original = Config::default();
        save_to(&original, path).expect("save_to should succeed");

        let loaded = load_from(path);
        assert_eq!(original, loaded, "round-trip must preserve all fields");
    }

    #[test]
    fn round_trip_custom_config() {
        let tmp = NamedTempFile::new().expect("tempfile");
        let path = tmp.path();

        let original = Config {
            web_search_enabled: false,
            window_alpha: 0.6,
        };
        save_to(&original, path).expect("save_to should succeed");

        let loaded = load_from(path);
        assert_eq!(original, loaded, "round-trip must preserve custom fields");
    }

    // ── Missing file → default ────────────────────────────────────────────

    #[test]
    fn missing_file_returns_default() {
        // Use a path that certainly does not exist.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("no_such_config.json");

        let cfg = load_from(&path);
        assert_eq!(cfg, Config::default(), "missing file must yield defaults");
    }

    // ── Corrupt JSON → default ────────────────────────────────────────────

    #[test]
    fn corrupt_json_returns_default() {
        let mut tmp = NamedTempFile::new().expect("tempfile");
        tmp.write_all(b"{ this is: not valid json }")
            .expect("write");

        let cfg = load_from(tmp.path());
        assert_eq!(cfg, Config::default(), "corrupt JSON must yield defaults");
    }

    // ── web_search_enabled default and round-trip ─────────────────────────

    #[test]
    fn default_web_search_enabled_is_true() {
        assert!(Config::default().web_search_enabled);
    }

    #[test]
    fn missing_web_search_field_defaults_to_true() {
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(br##"{"window_alpha":0.5}"##).unwrap();
        let cfg = load_from(tmp.path());
        assert!(
            cfg.web_search_enabled,
            "campo assente deve defaultare a true"
        );
    }

    // ── window_alpha default, round-trip, clamp ───────────────────────────

    #[test]
    fn default_window_alpha_is_0_87() {
        assert_eq!(Config::default().window_alpha, 0.87);
    }

    #[test]
    fn missing_window_alpha_field_defaults_to_0_87() {
        let mut tmp = NamedTempFile::new().expect("tempfile");
        tmp.write_all(br##"{"web_search_enabled":true}"##)
            .expect("write");
        let cfg = load_from(tmp.path());
        assert_eq!(
            cfg.window_alpha, 0.87,
            "campo assente deve defaultare a 0.87"
        );
    }

    #[test]
    fn window_alpha_below_zero_clamps_to_zero_on_load() {
        let mut tmp = NamedTempFile::new().expect("tempfile");
        tmp.write_all(br##"{"web_search_enabled":true,"window_alpha":-0.5}"##)
            .expect("write");
        let cfg = load_from(tmp.path());
        assert_eq!(
            cfg.window_alpha, 0.0,
            "un valore negativo scritto a mano deve clampare a 0.0 in lettura"
        );
    }

    #[test]
    fn window_alpha_above_one_clamps_to_one_on_load() {
        let mut tmp = NamedTempFile::new().expect("tempfile");
        tmp.write_all(br##"{"web_search_enabled":true,"window_alpha":1.5}"##)
            .expect("write");
        let cfg = load_from(tmp.path());
        assert_eq!(
            cfg.window_alpha, 1.0,
            "un valore > 1 scritto a mano deve clampare a 1.0 in lettura"
        );
    }

    // ── Compatibilità con un config.json v1 (Task 7) ───────────────────────

    /// Un `config.json` scritto da v1 (o da un `ui.exe` di una versione
    /// precedente al Task 7) porta ancora campi dell'overlay che non
    /// esistono più in questo `Config`. serde deve ignorarli silenziosamente
    /// (comportamento di default, nessun `#[serde(deny_unknown_fields)]` su
    /// questo struct) invece di far fallire il parse e far perdere
    /// `window_alpha`/`web_search_enabled` già personalizzati dall'utente.
    #[test]
    fn legacy_v1_config_json_with_extra_fields_still_loads() {
        // r##"…"## (doppio hash), non r#"…"# (singolo): il valore `"#FFF"`
        // contiene la sequenza `"#`, che con un solo hash chiuderebbe la raw
        // string subito dopo `cursor_color":` — stesso motivo per cui gli
        // altri test qui sopra usano `br##"…"##` per i loro JSON con colori.
        let json = r##"{"action_key":"F4","cursor_color":"#FFF","window_alpha":0.5,"web_search_enabled":false}"##;
        let cfg: Config = serde_json::from_str(json).unwrap(); // i campi ignoti vengono ignorati
        assert_eq!(cfg.window_alpha, 0.5);
        assert!(!cfg.web_search_enabled);
    }
}
