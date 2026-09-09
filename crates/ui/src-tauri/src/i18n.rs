// i18n.rs — Internationalization dictionary loader for Lare Terminal.
//
// Responsibility (SRP): owns loading flat key-value dictionaries from JSON files
// and fallback resolution (lang -> it -> key). Does NOT touch Tauri APIs.
//
// File location at runtime: `<config_dir>/i18n/<lang>.json`
// Handed an injectable `&Path` so this module is fully testable without Tauri.

use std::collections::HashMap;
use std::path::Path;

/// Carica un dizionario piatto chiave->valore da un file JSON.
///
/// Infallibile per il chiamante:
/// - File assente  → dizionario vuoto (silenzioso, prima esecuzione o lingua non presente).
/// - Errore IO     → dizionario vuoto + log di avviso via `tracing::warn!`.
/// - JSON corrotto → dizionario vuoto + log di avviso via `tracing::warn!`.
/// - File valido   → dizionario `HashMap<String, String>`.
pub fn load_dict(path: &Path) -> HashMap<String, String> {
    match std::fs::read_to_string(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            HashMap::new()
        }
        Err(e) => {
            tracing::warn!("[i18n] Impossibile leggere {:?}: {e} — dizionario vuoto", path);
            HashMap::new()
        }
        Ok(text) => match serde_json::from_str::<HashMap<String, String>>(&text) {
            Ok(dict) => dict,
            Err(e) => {
                tracing::warn!("[i18n] JSON non valido in {:?}: {e} — dizionario vuoto", path);
                HashMap::new()
            }
        },
    }
}

/// Carica il dizionario unito per la lingua richiesta con fallback su "it".
///
/// Catena di fallback:
/// Se `lang` != "it", carica prima `it.json` come base e poi sovrascrive
/// con le chiavi presenti in `<lang>.json`.
pub fn load_merged_dict(i18n_dir: &Path, lang: &str) -> HashMap<String, String> {
    let mut base = load_dict(&i18n_dir.join("it.json"));
    if lang != "it" {
        let overlay = load_dict(&i18n_dir.join(format!("{lang}.json")));
        for (k, v) in overlay {
            base.insert(k, v);
        }
    }
    base
}

/// Traduce una chiave sincronicamente usando i dizionari presenti in `i18n_dir`.
///
/// Catena di fallback completa:
/// `<lang>.json` -> `it.json` -> `key` letterale.
pub fn t_sync(i18n_dir: &Path, lang: &str, key: &str) -> String {
    let dict = load_merged_dict(i18n_dir, lang);
    dict.get(key).cloned().unwrap_or_else(|| key.to_string())
}

// ---------------------------------------------------------------------------
// Tests (TDD: coprono assente, corrotto, valido, fallback chain)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use tempfile::NamedTempFile;

    // ── Missing file → empty dict ─────────────────────────────────────────

    #[test]
    fn missing_file_returns_empty_dict() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("non_existent.json");
        let dict = load_dict(&path);
        assert!(dict.is_empty(), "file assente deve restituire un dizionario vuoto");
    }

    // ── Corrupt JSON → empty dict ─────────────────────────────────────────

    #[test]
    fn corrupt_json_returns_empty_dict() {
        let mut tmp = NamedTempFile::new().expect("tempfile");
        tmp.write_all(b"{ this is invalid json: definitely not valid }")
            .expect("write");
        let dict = load_dict(tmp.path());
        assert!(dict.is_empty(), "JSON corrotto deve restituire un dizionario vuoto");
    }

    // ── Valid JSON → populated dict ───────────────────────────────────────

    #[test]
    fn valid_json_loads_successfully() {
        let mut tmp = NamedTempFile::new().expect("tempfile");
        tmp.write_all(br#"{"common.save":"Salva","config.title":"Configurazione"}"#)
            .expect("write");
        let dict = load_dict(tmp.path());
        assert_eq!(dict.len(), 2);
        assert_eq!(dict.get("common.save").unwrap(), "Salva");
        assert_eq!(dict.get("config.title").unwrap(), "Configurazione");
    }

    // ── Fallback chain: lang -> it -> key ─────────────────────────────────

    #[test]
    fn fallback_chain_en_to_it_to_key() {
        let dir = tempfile::tempdir().expect("tempdir");
        let it_path = dir.path().join("it.json");
        let en_path = dir.path().join("en.json");

        // it.json has both key1 and key2
        std::fs::write(&it_path, r#"{"key1":"Valore IT 1","key2":"Valore IT 2"}"#).unwrap();
        // en.json only has key1
        std::fs::write(&en_path, r#"{"key1":"Value EN 1"}"#).unwrap();

        // 1. Existing in EN -> returns EN value
        assert_eq!(t_sync(dir.path(), "en", "key1"), "Value EN 1");

        // 2. Missing in EN, but present in IT -> falls back to IT value
        assert_eq!(t_sync(dir.path(), "en", "key2"), "Valore IT 2");

        // 3. Missing in both EN and IT -> returns literal key
        assert_eq!(t_sync(dir.path(), "en", "key_unknown"), "key_unknown");

        // 4. IT requested -> returns IT value
        assert_eq!(t_sync(dir.path(), "it", "key1"), "Valore IT 1");
    }
}
