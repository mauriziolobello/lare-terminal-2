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

/// Risolve il path di `telegramsettings.json`. Precedenza a 3 livelli via
/// `startup_config::resolve`: `LARE_TELEGRAM_SETTINGS` (env, se impostata
/// e non vuota dopo trim) > campo `telegram_settings` di `startup.json`
/// (se presente) > `<exe_dir>/telegramsettings.json`.
///
/// `exe_dir` (cartella dell'eseguibile, via `startup_config::exe_dir()`)
/// sostituisce la precedente `launch_dir` (`std::env::current_dir()`,
/// catturata prima di qualunque `cd`): un servizio Windows lanciato da SCM
/// ha spesso una cwd di default (`C:\Windows\System32\`) diversa dalla
/// cartella di installazione — bug latente reale, mai osservato finora
/// perché si lancia sempre da `.ps1` con la cwd già corretta, ma un
/// ancoraggio sbagliato per lo scenario servizio (vedi spec §2).
pub fn resolve_path(
    env_override: Option<&str>,
    file_value: Option<&str>,
    exe_dir: &Path,
) -> PathBuf {
    startup_config::resolve(env_override, file_value, || {
        exe_dir.join("telegramsettings.json")
    })
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
            let parsed: TelegramSettingsFile =
                serde_json::from_str(&content).map_err(|_| {
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
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    // ── resolve_path ──────────────────────────────────────────────────────────

    /// Nessun override → il file è cercato nella cartella dell'eseguibile.
    #[test]
    fn resolve_uses_exe_dir_by_default() {
        let p = resolve_path(None, None, Path::new("/proj"));
        assert_eq!(p, Path::new("/proj").join("telegramsettings.json"));
    }

    /// Override env vuoto o solo spazi → fallback (come override assente).
    #[test]
    fn resolve_blank_env_override_falls_back() {
        let p = resolve_path(Some("   "), None, Path::new("/proj"));
        assert_eq!(p, Path::new("/proj").join("telegramsettings.json"));
    }

    /// Override env non vuoto → vince, usato così com'è.
    #[test]
    fn resolve_env_override_wins() {
        let p = resolve_path(Some("D:/custom/tg.json"), None, Path::new("/proj"));
        assert_eq!(p, PathBuf::from("D:/custom/tg.json"));
    }

    /// Campo di startup.json → vince quando l'env non è impostata.
    #[test]
    fn resolve_file_value_wins_when_env_absent() {
        let p = resolve_path(
            None,
            Some("D:/from-startup-json/tg.json"),
            Path::new("/proj"),
        );
        assert_eq!(p, PathBuf::from("D:/from-startup-json/tg.json"));
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
            let settings = load(f.path()).unwrap().expect("atteso Some per chiave {key}");
            assert_eq!(settings.token, "bot123:abc", "chiave {key} deve essere accettata");
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
