//! # telegram::settings
//!
//! Caricamento di `telegramsettings.json` (token del bot Telegram).
//!
//! Il file è opzionale: se assente il canale Telegram è disattivato.
//! Se presente deve contenere `{ "token": "<non-vuoto>" }`.
//!
//! **Sicurezza:** il token non viene mai loggato né incluso nei messaggi di
//! errore. `TelegramSettings` non implementa `Debug` (evita stampe accidentali).

use std::path::{Path, PathBuf};

/// Configurazione del canale Telegram.
///
/// Non implementa `Debug` intenzionalmente: il campo `token` non deve mai
/// comparire nei log.
pub struct TelegramSettings {
    pub token: String,
}

/// Schema JSON di `telegramsettings.json`.
///
/// Usato solo internamente per la deserializzazione; non esposto.
#[derive(serde::Deserialize)]
struct TelegramSettingsFile {
    /// Tollerante sul case del campo: accetta `token`, `Token`, `TOKEN`.
    #[serde(alias = "Token", alias = "TOKEN")]
    token: String,
}

/// Risolve il path di `telegramsettings.json`: SEMPRE
/// `<config_dir>/telegramsettings.json`, nessuna variabile d'ambiente né
/// campo `startup.json` dedicato (D6, 2.0 — la v1 aveva 3 livelli di
/// precedenza qui, `LARE_TELEGRAM_SETTINGS` compreso). `config_dir` è la
/// stessa cartella risolta una volta in `main()`.
pub fn resolve_path(config_dir: &Path) -> PathBuf {
    config_dir.join("telegramsettings.json")
}

/// Carica `telegramsettings.json` dal path indicato.
///
/// - `Ok(None)` — file non trovato: canale disattivato (comportamento normale).
/// - `Ok(Some(s))` — file trovato e valido.
/// - `Err(_)` — file trovato ma illeggibile, JSON malformato, o token vuoto.
///
/// **Sicurezza:** il token non viene mai incluso nei messaggi di errore.
pub fn load(path: &Path) -> std::io::Result<Option<TelegramSettings>> {
    match std::fs::read_to_string(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // File assente → canale disattivato, nessun errore.
            Ok(None)
        }
        Err(e) => {
            // File presente ma non leggibile (es. permessi).
            Err(e)
        }
        Ok(content) => {
            // File letto: deserializza.
            let parsed: TelegramSettingsFile = serde_json::from_str(&content).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "telegramsettings.json: JSON malformato o campo 'token' mancante",
                )
            })?;

            if parsed.token.is_empty() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "telegramsettings.json: token vuoto",
                ));
            }

            Ok(Some(TelegramSettings {
                token: parsed.token,
            }))
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests (TDD — RED scritto prima dell'implementazione)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    // ── resolve_path ──────────────────────────────────────────────────────────

    /// Sempre `<config_dir>/telegramsettings.json`, nessun override.
    #[test]
    fn resolve_path_is_config_dir_join_telegramsettings_json() {
        let p = resolve_path(Path::new("/proj/Configuration"));
        assert_eq!(
            p,
            Path::new("/proj/Configuration").join("telegramsettings.json")
        );
    }

    /// File valido con token → Ok(Some(token))
    #[test]
    fn valid_file_returns_some_token() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, r#"{{"token":"bot123:abc"}}"#).unwrap();
        let result = load(f.path()).unwrap();
        let settings = result.expect("atteso Some");
        assert_eq!(settings.token, "bot123:abc");
    }

    /// Chiave `Token`/`TOKEN` (case diverso) → accettata via alias serde.
    #[test]
    fn token_key_case_insensitive_via_alias() {
        for key in ["Token", "TOKEN"] {
            let mut f = NamedTempFile::new().unwrap();
            write!(f, r#"{{"{key}":"bot123:abc"}}"#).unwrap();
            let settings = load(f.path())
                .unwrap()
                .expect("atteso Some per chiave {key}");
            assert_eq!(
                settings.token, "bot123:abc",
                "chiave {key} deve essere accettata"
            );
        }
    }

    /// File assente → Ok(None)
    #[test]
    fn missing_file_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("telegramsettings.json");
        let result = load(&missing).unwrap();
        assert!(result.is_none(), "file assente deve restituire None");
    }

    /// JSON malformato → Err
    #[test]
    fn malformed_json_returns_err() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "not json at all").unwrap();
        let result = load(f.path());
        assert!(result.is_err(), "JSON malformato deve restituire Err");
    }

    /// JSON valido ma campo 'token' mancante → Err
    #[test]
    fn missing_token_field_returns_err() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, r#"{{"other_field":"value"}}"#).unwrap();
        let result = load(f.path());
        assert!(result.is_err(), "campo token mancante deve restituire Err");
    }

    /// Token vuoto → Err
    #[test]
    fn empty_token_returns_err() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, r#"{{"token":""}}"#).unwrap();
        let result = load(f.path());
        assert!(result.is_err(), "token vuoto deve restituire Err");
        // Il messaggio di errore non deve contenere il token (qui vuoto, ma
        // il principio vale anche per token non-vuoti).
        // Nota: `unwrap_err()` richiede `T: Debug`; usiamo `err().unwrap()`.
        let msg = result.err().unwrap().to_string();
        assert!(
            msg.contains("token vuoto"),
            "messaggio di errore atteso; trovato: {msg}"
        );
    }

    /// Il messaggio di errore per JSON malformato non deve rivelare il contenuto
    /// del file (che potrebbe includere accidentalmente un token parziale).
    #[test]
    fn error_message_does_not_contain_file_content() {
        let mut f = NamedTempFile::new().unwrap();
        // Contenuto "pericoloso" che include una stringa simile a un token
        write!(f, r#"{{broken json with token_like 123:abc}}"#).unwrap();
        // Usiamo `err().unwrap()` per evitare il bound `T: Debug` su `unwrap_err()`.
        let err = load(f.path()).err().unwrap();
        let msg = err.to_string();
        // Il messaggio non deve ripetere il contenuto grezzo del file
        assert!(
            !msg.contains("broken json"),
            "il messaggio di errore non deve includere il contenuto del file: {msg}"
        );
    }
}
