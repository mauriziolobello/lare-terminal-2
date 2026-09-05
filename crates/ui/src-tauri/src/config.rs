// config.rs — Persistent configuration for Lare Terminal.
//
// Responsibility (SRP): owns the Config struct, its defaults, JSON persistence,
// and action-key validation.  Does NOT touch Tauri APIs (those are in main.rs).
//
// File location at runtime: `<config_dir>/config.json` (2.0: `config_dir` is
// `--config-dir` or `<exe_dir>/Configuration`, resolved once via
// `ConfigDirState` — see main.rs's `config_file_path`).
// The full path is built by main.rs and then handed to `load_from` / `save_to`
// here as an injectable &Path — keeping this module testable without a
// running Tauri instance.

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::str::FromStr;

// Re-export Shortcut so main.rs can parse validated strings.
pub use tauri_plugin_global_shortcut::Shortcut;

// ---------------------------------------------------------------------------
// Position enum
// ---------------------------------------------------------------------------

/// Where the overlay window appears when summoned by the action key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Position {
    /// Centered on the monitor that holds the cursor (default).
    #[default]
    Center,
    /// Horizontally centered, near the bottom of the monitor.
    BottomCenter,
    /// Near the current mouse cursor position.
    NearMouse,
}

// ---------------------------------------------------------------------------
// ActivityIndicator enum
// ---------------------------------------------------------------------------

/// Stile dell'indicatore di attività mostrato durante l'elaborazione.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ActivityIndicator {
    /// Spinner accanto al titolo (default).
    #[default]
    Title,
    /// Spinner + "elaboro…" accanto al badge di stato.
    Status,
    /// Il prompt pulsa.
    Prompt,
}

// ---------------------------------------------------------------------------
// Config struct
// ---------------------------------------------------------------------------

/// Persisted user configuration.
///
/// All fields have documented defaults matching `07-ux-and-config.md`.
/// `Default` yields those same values; round-trip through JSON preserves them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    /// Tauri accelerator string for the global toggle hotkey.
    /// Default: `"F2"`.
    pub action_key: String,

    /// Text colour as a CSS hex string.
    /// Default: `"#FFFFFF"` (white).
    pub cursor_color: String,

    /// Font family name for the input / output area.
    /// Default: `"Consolas"`.
    pub cursor_font: String,

    /// Font size in pixels.
    /// Default: `11`.
    pub cursor_size: u32,

    /// Where the overlay appears when summoned.
    /// Default: `Position::Center`.
    pub position: Position,

    /// Stile dell'indicatore di attività.
    /// Default: `ActivityIndicator::Title`.
    #[serde(default)]
    pub activity_indicator: ActivityIndicator,

    /// Abilita la ricerca web interna dell'AI (tool server-side).
    /// Default: `true`.
    #[serde(default = "default_true")]
    pub web_search_enabled: bool,

    /// Minuti di inattività prima che appaia l'animazione del pulcino idle.
    /// `0` disabilita l'animazione. Default: `5`.
    #[serde(default = "default_idle_duck_minutes")]
    pub idle_duck_minutes: u32,

    /// Alpha di trasparenza condiviso da ogni sfondo "pannello" di ogni finestra
    /// dell'app (cursore, Library, /config, AI Chat, tutti i plugin). Intervallo
    /// [0.0, 1.0]. Default 0.87 (valore storico allineato in 185f304).
    #[serde(default = "default_window_alpha")]
    pub window_alpha: f64,
}

/// Helper per `#[serde(default = "default_true")]`.
fn default_true() -> bool {
    true
}

/// Helper per `#[serde(default = "default_idle_duck_minutes")]`.
fn default_idle_duck_minutes() -> u32 {
    5
}

/// Helper per `#[serde(default = "default_window_alpha")]`.
fn default_window_alpha() -> f64 {
    0.87
}

impl Default for Config {
    fn default() -> Self {
        Self {
            action_key: "F2".to_string(),
            cursor_color: "#FFFFFF".to_string(),
            cursor_font: "Consolas".to_string(),
            cursor_size: 11,
            position: Position::Center,
            activity_indicator: ActivityIndicator::Title,
            web_search_enabled: true,
            idle_duck_minutes: 5,
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
// Validation
// ---------------------------------------------------------------------------

/// Validate that `key` is parseable as a Tauri/global-hotkey accelerator.
///
/// Delegates to `Shortcut::from_str` (via `global_hotkey` crate) — the same
/// parser used by `app.global_shortcut().register(...)`.
///
/// Returns `Ok(())` on success or `Err(human-readable message)` on failure.
pub fn validate_action_key(key: &str) -> Result<(), String> {
    Shortcut::from_str(key).map(|_| ()).map_err(|e| {
        format!(
            "invalid accelerator {:?}: {e}  \
             (examples: \"F2\", \"Ctrl+Alt+T\", \"Alt+F4\")",
            key
        )
    })
}

// ---------------------------------------------------------------------------
// Tests (TDD: written before implementation)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use tempfile::NamedTempFile;

    // ── Default values ────────────────────────────────────────────────────

    #[test]
    fn default_action_key_is_f2() {
        assert_eq!(Config::default().action_key, "F2");
    }

    #[test]
    fn default_cursor_color_is_white() {
        assert_eq!(Config::default().cursor_color, "#FFFFFF");
    }

    #[test]
    fn default_cursor_font_is_consolas() {
        assert_eq!(Config::default().cursor_font, "Consolas");
    }

    #[test]
    fn default_cursor_size_is_11() {
        assert_eq!(Config::default().cursor_size, 11);
    }

    #[test]
    fn default_position_is_center() {
        assert_eq!(Config::default().position, Position::Center);
    }

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
            action_key: "F5".to_string(),
            cursor_color: "#00FF00".to_string(),
            cursor_font: "Courier New".to_string(),
            cursor_size: 14,
            position: Position::BottomCenter,
            activity_indicator: ActivityIndicator::Status,
            web_search_enabled: false,
            idle_duck_minutes: 10,
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

    // ── validate_action_key ───────────────────────────────────────────────

    #[test]
    fn validate_f2_is_ok() {
        assert!(
            validate_action_key("F2").is_ok(),
            "\"F2\" must be a valid accelerator"
        );
    }

    #[test]
    fn validate_f5_is_ok() {
        assert!(
            validate_action_key("F5").is_ok(),
            "\"F5\" must be a valid accelerator"
        );
    }

    #[test]
    fn validate_ctrl_alt_t_is_ok() {
        // On Windows/Linux Ctrl+Alt+T is a valid combination.
        assert!(
            validate_action_key("Ctrl+Alt+T").is_ok(),
            "\"Ctrl+Alt+T\" must be a valid accelerator"
        );
    }

    #[test]
    fn validate_invalid_key_returns_err() {
        assert!(
            validate_action_key("NotAKey###").is_err(),
            "\"NotAKey###\" must be rejected"
        );
    }

    #[test]
    fn validate_empty_string_returns_err() {
        assert!(
            validate_action_key("").is_err(),
            "empty string must be rejected"
        );
    }

    // ── web_search_enabled default and round-trip ─────────────────────────

    #[test]
    fn default_web_search_enabled_is_true() {
        assert!(Config::default().web_search_enabled);
    }

    #[test]
    fn missing_web_search_field_defaults_to_true() {
        let mut tmp = NamedTempFile::new().unwrap();
        // JSON senza web_search_enabled ma con action_key distintivo "F7".
        // Usiamo br##"..."## perché "#FFFFFF" contiene #" che terminerebbe un raw literal a singolo hash.
        tmp.write_all(br##"{"action_key":"F7","cursor_color":"#FFFFFF","cursor_font":"Consolas","cursor_size":11,"position":"center","activity_indicator":"title"}"##).unwrap();
        let cfg = load_from(tmp.path());
        // Se #[serde(default = "default_true")] funziona, il parse riesce e action_key è preservato.
        assert_eq!(cfg.action_key, "F7",
            "parse deve riuscire — action_key deve essere F7 (non fallback a F2)");
        assert!(cfg.web_search_enabled, "campo assente deve defaultare a true");
    }

    // ── ActivityIndicator default and round-trip ──────────────────────────

    #[test]
    fn default_activity_indicator_is_title() {
        assert_eq!(
            Config::default().activity_indicator,
            ActivityIndicator::Title,
            "default activity_indicator must be Title"
        );
    }

    /// Verify that `#[serde(default)]` is in place: a JSON config that omits
    /// `activity_indicator` but has a distinctive `action_key` (not "F2") must
    /// deserialize successfully with `activity_indicator == Title` AND preserve
    /// the custom `action_key`.  If `#[serde(default)]` were missing, serde
    /// would return a parse error and `load_from` would fall back to
    /// `Config::default()` — which would give `action_key == "F2"`, causing
    /// the second assertion to fail.
    #[test]
    fn default_idle_duck_minutes_is_5() {
        assert_eq!(Config::default().idle_duck_minutes, 5);
    }

    #[test]
    fn missing_idle_duck_minutes_defaults_to_5() {
        let mut tmp = NamedTempFile::new().expect("tempfile");
        // JSON senza idle_duck_minutes ma con action_key distintivo "F9".
        tmp.write_all(
            br##"{"action_key":"F9","cursor_color":"#FFFFFF","cursor_font":"Consolas","cursor_size":11,"position":"center","activity_indicator":"title","web_search_enabled":true}"##,
        )
        .expect("write");
        let cfg = load_from(tmp.path());
        assert_eq!(cfg.action_key, "F9",
            "parse deve riuscire — action_key F9 deve essere preservato");
        assert_eq!(cfg.idle_duck_minutes, 5,
            "campo assente deve defaultare a 5");
    }

    #[test]
    fn missing_activity_indicator_field_defaults_to_title() {
        let mut tmp = NamedTempFile::new().expect("tempfile");
        // JSON with a distinctive action_key but NO activity_indicator field.
        // Use a two-hash raw literal so the #RRGGBB colour doesn't end the string.
        tmp.write_all(
            br##"{
  "action_key": "F7",
  "cursor_color": "#00FF00",
  "cursor_font": "Consolas",
  "cursor_size": 12,
  "position": "center"
}"##,
        )
        .expect("write");

        let cfg = load_from(tmp.path());
        // If #[serde(default)] works, the parse succeeds and action_key is preserved.
        assert_eq!(
            cfg.action_key, "F7",
            "parse must succeed — action_key must be preserved (not fallen back to F2)"
        );
        assert_eq!(
            cfg.activity_indicator,
            ActivityIndicator::Title,
            "missing activity_indicator field must default to Title"
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
        // JSON senza window_alpha ma con action_key distintivo "F11".
        tmp.write_all(
            br##"{"action_key":"F11","cursor_color":"#FFFFFF","cursor_font":"Consolas","cursor_size":11,"position":"center","activity_indicator":"title","web_search_enabled":true,"idle_duck_minutes":5}"##,
        )
        .expect("write");
        let cfg = load_from(tmp.path());
        assert_eq!(cfg.action_key, "F11",
            "parse deve riuscire — action_key F11 deve essere preservato");
        assert_eq!(cfg.window_alpha, 0.87,
            "campo assente deve defaultare a 0.87");
    }

    #[test]
    fn window_alpha_below_zero_clamps_to_zero_on_load() {
        let mut tmp = NamedTempFile::new().expect("tempfile");
        tmp.write_all(
            br##"{"action_key":"F2","cursor_color":"#FFFFFF","cursor_font":"Consolas","cursor_size":11,"position":"center","activity_indicator":"title","web_search_enabled":true,"idle_duck_minutes":5,"window_alpha":-0.5}"##,
        )
        .expect("write");
        let cfg = load_from(tmp.path());
        assert_eq!(cfg.window_alpha, 0.0,
            "un valore negativo scritto a mano deve clampare a 0.0 in lettura");
    }

    #[test]
    fn window_alpha_above_one_clamps_to_one_on_load() {
        let mut tmp = NamedTempFile::new().expect("tempfile");
        tmp.write_all(
            br##"{"action_key":"F2","cursor_color":"#FFFFFF","cursor_font":"Consolas","cursor_size":11,"position":"center","activity_indicator":"title","web_search_enabled":true,"idle_duck_minutes":5,"window_alpha":1.5}"##,
        )
        .expect("write");
        let cfg = load_from(tmp.path());
        assert_eq!(cfg.window_alpha, 1.0,
            "un valore > 1 scritto a mano deve clampare a 1.0 in lettura");
    }
}
