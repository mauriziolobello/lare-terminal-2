//! Vista di sola lettura sui plugin installati, per il tab "Plugins" della
//! finestra Library.
//!
//! A differenza di `orchestrator::plugins::discovery::discover` (che valida
//! l'`id` per sicurezza e cerca il binario associato prima di eseguirlo),
//! questo modulo NON esegue nulla: si limita a mostrare cosa c'è su disco.
//! Per questo è deliberatamente più tollerante — un manifest malformato non
//! viene scartato, viene mostrato "com'è" (fallback sul nome della cartella)
//! perché per un pannello diagnostico "qui c'è un plugin.json rotto nella
//! cartella X" è più utile di un elenco che tace il problema.
//!
//! Editing della configurazione, nuova finestra dedicata, cifratura: fuori
//! scope per questa prima slice (vedi `IMPLEMENTATION.md`).

use std::path::Path;
use tauri::State;

use crate::config_dir::ConfigDirState;

/// Una entry della lista Plugin: id/nome (per la riga) + il contenuto grezzo
/// del manifest (per la vista dettaglio). Sola lettura — nessuna validazione
/// di sicurezza (id-safety, binario presente) perché questa vista non esegue
/// nulla, si limita a mostrare cosa c'è su disco.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct PluginListEntry {
    pub id: String,
    pub name: String,
    pub manifest_json: String,
}

/// Scandisce `dir` e ritorna una entry per ogni sottocartella che contiene un
/// `plugin.json` leggibile. Nucleo puro/testabile (prende `dir` come
/// parametro, esattamente come `discovery::discover`) — nessuna dipendenza
/// da `ConfigDirState::plugins_dir()` così i test possono puntare a una tempdir.
///
/// Tolleranza (a differenza di `discovery::discover`, che scarta i manifest
/// invalidi): se il JSON non parsa, o `id`/`name` mancano o non sono
/// stringhe, la entry viene comunque prodotta usando il nome della cartella
/// come fallback per ENTRAMBI i campi — non vogliamo nascondere un plugin
/// rotto, vogliamo mostrarlo.
pub fn scan_plugins(dir: &Path) -> Vec<PluginListEntry> {
    let mut out = Vec::new();

    // `read_dir` fallisce se `dir` non esiste o non è leggibile: nessun
    // plugin installato è una condizione normale, non un errore da propagare.
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };

    for entry in entries.flatten() {
        let plugin_dir = entry.path();

        // Solo sottocartelle: file sciolti in plugins/ non sono plugin.
        if !plugin_dir.is_dir() {
            continue;
        }

        // Nome della cartella, usato come fallback e per il caso "manifest
        // assente" (che salta silenziosamente, mirror di discovery.rs).
        let folder_name = plugin_dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        let manifest_path = plugin_dir.join("plugin.json");
        let Ok(raw) = std::fs::read_to_string(&manifest_path) else {
            // Nessun plugin.json in questa sottocartella: non è una dir
            // plugin, saltala in silenzio (stessa tolleranza di discovery.rs).
            continue;
        };

        // Prova a interpretare il file come JSON. Se riesce, estrae id/name
        // (con fallback sul nome cartella se mancano o non sono stringhe) e
        // pretty-print del manifest per la vista dettaglio. Se il parsing
        // fallisce, mostra comunque l'entry con il testo grezzo e il nome
        // cartella per id/name — un plugin.json rotto non deve sparire.
        let (id, name, manifest_json) = match serde_json::from_str::<serde_json::Value>(&raw) {
            Ok(value) => {
                let id = value
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| folder_name.clone());
                let name = value
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| folder_name.clone());
                let pretty = serde_json::to_string_pretty(&value).unwrap_or_else(|_| raw.clone());
                (id, name, pretty)
            }
            Err(_) => (folder_name.clone(), folder_name.clone(), raw),
        };

        out.push(PluginListEntry {
            id,
            name,
            manifest_json,
        });
    }

    out
}

/// Comando Tauri invocato dal frontend (`library.js::loadPluginsTab`).
/// Sottile: nessuna logica qui, solo la composizione risoluzione-path +
/// scansione — la logica vera è in `scan_plugins`, testata sotto. La
/// directory viene da `ConfigDirState::plugins_dir()` (`startup.json.paths.plugins_dir`,
/// relativa alla radice del deploy) — 2.0: niente più caso speciale
/// `LARE_PLUGINS_DIR`, un solo risolutore condiviso con l'orchestrator.
#[tauri::command]
pub fn list_plugins(state: State<'_, ConfigDirState>) -> Vec<PluginListEntry> {
    scan_plugins(&state.plugins_dir())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Crea `<root>/<id>/plugin.json` con il contenuto dato.
    fn make_plugin(root: &Path, id: &str, manifest_json: &str) {
        let dir = root.join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("plugin.json"), manifest_json).unwrap();
    }

    #[test]
    fn scans_valid_plugin_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        make_plugin(
            tmp.path(),
            "ping",
            r#"{"id":"ping","name":"Ping Plugin","version":"1.0.0"}"#,
        );

        let found = scan_plugins(tmp.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "ping");
        assert_eq!(found[0].name, "Ping Plugin");
        assert!(found[0].manifest_json.contains("Ping Plugin"));
    }

    #[test]
    fn skips_subfolder_without_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("empty")).unwrap();

        assert_eq!(scan_plugins(tmp.path()).len(), 0);
    }

    #[test]
    fn malformed_manifest_falls_back_to_folder_name() {
        let tmp = tempfile::tempdir().unwrap();
        make_plugin(tmp.path(), "broken", "{ not json");

        let found = scan_plugins(tmp.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "broken");
        assert_eq!(found[0].name, "broken");
        assert_eq!(found[0].manifest_json, "{ not json");
    }

    #[test]
    fn manifest_missing_id_or_name_falls_back_to_folder_name() {
        let tmp = tempfile::tempdir().unwrap();
        make_plugin(tmp.path(), "nofields", r#"{"version":"1.0.0"}"#);

        let found = scan_plugins(tmp.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "nofields");
        assert_eq!(found[0].name, "nofields");
    }

    #[test]
    fn missing_plugins_dir_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(scan_plugins(&tmp.path().join("nope")).len(), 0);
    }

    #[test]
    fn multiple_plugins_all_listed() {
        let tmp = tempfile::tempdir().unwrap();
        make_plugin(tmp.path(), "alpha", r#"{"id":"alpha","name":"Alpha"}"#);
        make_plugin(tmp.path(), "beta", r#"{"id":"beta","name":"Beta"}"#);

        let found = scan_plugins(tmp.path());
        assert_eq!(found.len(), 2);
        assert!(found.iter().any(|p| p.id == "alpha"));
        assert!(found.iter().any(|p| p.id == "beta"));
    }
}
