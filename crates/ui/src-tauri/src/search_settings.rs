//! Lettura/scrittura dei parametri di ricerca (result_cap, max_depth) in
//! search-paths.json (di proprietà dell'orchestrator, in `<config_dir>`).
//! Il merge PRESERVA tutti gli altri campi del file.

use std::path::{Path, PathBuf};
use tauri::State;

use crate::config_dir::ConfigDirState;

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug)]
pub struct SearchSettings {
    pub result_cap: usize,
    pub max_depth: usize,
}

/// Imposta result_cap e max_depth in `existing` (JSON object), preservando gli
/// altri campi. Se `existing` non è un object valido → riparte da `{}`.
pub fn merge_search_settings(existing: &str, s: &SearchSettings) -> String {
    let mut value: serde_json::Value = serde_json::from_str(existing)
        .ok()
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    let obj = value.as_object_mut().expect("object garantito sopra");
    obj.insert("result_cap".to_string(), serde_json::json!(s.result_cap));
    obj.insert("max_depth".to_string(), serde_json::json!(s.max_depth));
    serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string())
}

/// Percorso di search-paths.json — SEMPRE `<config_dir>/search-paths.json`
/// (2.0: un solo risolutore, `ConfigDirState`, al posto della catena di
/// variabili d'ambiente con fallback che v1 duplicava in ognuno dei 5
/// moduli settings).
fn search_paths_json_path(config_dir: &Path) -> PathBuf {
    config_dir.join("search-paths.json")
}

#[tauri::command]
pub fn get_search_settings(state: State<'_, ConfigDirState>) -> SearchSettings {
    let content =
        std::fs::read_to_string(search_paths_json_path(&state.config_dir)).unwrap_or_default();
    let value: serde_json::Value =
        serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}));
    let result_cap = value
        .get("result_cap")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(2000) as usize;
    let max_depth = value
        .get("max_depth")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(8) as usize;
    SearchSettings {
        result_cap,
        max_depth,
    }
}

#[tauri::command]
pub fn set_search_settings(
    settings: SearchSettings,
    state: State<'_, ConfigDirState>,
) -> Result<(), String> {
    if !(1..=100_000).contains(&settings.result_cap) {
        return Err("Risultati max: deve essere un intero tra 1 e 100000".to_string());
    }
    if !(1..=64).contains(&settings.max_depth) {
        return Err("Profondità max: deve essere un intero tra 1 e 64".to_string());
    }
    let path = search_paths_json_path(&state.config_dir);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let merged = merge_search_settings(&existing, &settings);
    std::fs::write(&path, merged).map_err(|e| format!("search-paths.json: scrittura fallita ({e})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_preserves_other_fields() {
        let existing =
            r#"{"standard":["x"],"exclude":["a"],"version":2,"result_cap":1000,"max_depth":8}"#;
        let out = merge_search_settings(
            existing,
            &SearchSettings {
                result_cap: 2500,
                max_depth: 10,
            },
        );
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["result_cap"], 2500);
        assert_eq!(v["max_depth"], 10);
        assert_eq!(v["version"], 2);
        assert_eq!(v["standard"][0], "x");
        assert_eq!(v["exclude"][0], "a");
    }

    #[test]
    fn merge_from_empty_or_invalid_yields_object_with_fields() {
        for bad in ["", "not json", "[1,2,3]"] {
            let out = merge_search_settings(
                bad,
                &SearchSettings {
                    result_cap: 2000,
                    max_depth: 8,
                },
            );
            let v: serde_json::Value = serde_json::from_str(&out).unwrap();
            assert!(v.is_object(), "input {bad:?} deve dare un object");
            assert_eq!(v["result_cap"], 2000);
            assert_eq!(v["max_depth"], 8);
        }
    }
}
