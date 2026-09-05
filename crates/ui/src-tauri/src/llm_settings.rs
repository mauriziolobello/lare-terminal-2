//! Lettura/scrittura del provider LLM attivo in `llms.json` (di proprietà
//! dell'orchestrator, in `<config_dir>` — stesso path che
//! `orchestrator::llms_config::resolve_path` risolve, vedi
//! `Docs/superpowers/specs/2026-07-07-llms-config-ui-tab-design.md`).
//!
//! **Sola selezione**: questo modulo legge `active`+`providers[].{name,model}` e
//! scrive SOLO `active`. Non aggiunge/rimuove provider, non tocca `model`/
//! `api_key_ref`/`max_tokens` di un provider, e soprattutto **non legge né scrive
//! mai `api_keys`** — i tipi Rust qui sotto non hanno nemmeno quel campo: non è
//! omesso per disciplina, non esiste nello schema. Aggiungere/rimuovere provider o
//! editare le API key resta editing a mano del file JSON.

use std::path::{Path, PathBuf};
use tauri::State;

use crate::config_dir::ConfigDirState;

/// Un provider, per la sola visualizzazione nel tab (mai una API key).
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct LlmProviderInfo {
    pub name: String,
    pub model: String,
}

/// Stato corrente del tab: quale provider è `active` e l'elenco di quelli
/// disponibili (nome+modello soltanto).
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct LlmSettings {
    pub active: String,
    pub providers: Vec<LlmProviderInfo>,
}

/// Percorso di `llms.json` — SEMPRE `<config_dir>/llms.json`, stesso path che
/// `orchestrator::llms_config::resolve_path` risolve (2.0: entrambi i processi
/// usano lo stesso crate `startup-config`, nessuna divergenza possibile).
fn llms_json_path(config_dir: &Path) -> PathBuf {
    config_dir.join("llms.json")
}

/// Estrae `active`+`providers[].{name,model}` da un JSON grezzo. Pura (nessun
/// I/O) — separata da `get_llm_settings` per essere testabile senza toccare il
/// filesystem. Qualunque errore di parsing, o campo mancante/malformato,
/// produce il default vuoto — mai un panic: il dialog deve sempre potersi
/// aprire, anche col file assente o corrotto.
fn parse_llm_settings(content: &str) -> LlmSettings {
    let empty = || LlmSettings {
        active: String::new(),
        providers: Vec::new(),
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(content) else {
        return empty();
    };
    let active = value
        .get("active")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string();
    let providers = value
        .get("providers")
        .and_then(serde_json::Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|p| {
                    let name = p.get("name")?.as_str()?.to_string();
                    let model = p.get("model")?.as_str()?.to_string();
                    Some(LlmProviderInfo { name, model })
                })
                .collect()
        })
        .unwrap_or_default();
    LlmSettings { active, providers }
}

/// Legge il provider attivo e l'elenco dei provider da `llms.json`.
///
/// Infallibile (come `get_aichat_settings`): file assente/illeggibile/malformato
/// → `{active: "", providers: []}` — il tab mostra la nota informativa invece
/// della lista (vedi `config-dialog.js::_buildLlmTab`, Task 3).
#[tauri::command]
pub fn get_llm_settings(state: State<'_, ConfigDirState>) -> LlmSettings {
    let content = std::fs::read_to_string(llms_json_path(&state.config_dir)).unwrap_or_default();
    parse_llm_settings(&content)
}

/// Valida `active` e prepara il nuovo contenuto di `llms.json` — SOLO la chiave
/// `"active"` cambia, tutto il resto (`providers[]`, `api_keys`, qualunque
/// campo futuro) attraversa il merge senza mai passare per una struct Rust che
/// lo conosce. Pura (nessun I/O) — separata da `set_llm_settings` per essere
/// testabile senza toccare il filesystem.
fn merge_active(existing: &str, active: &str) -> Result<String, String> {
    let active = active.trim();
    if active.is_empty() {
        return Err("Provider attivo: non può essere vuoto".to_string());
    }
    let mut value: serde_json::Value =
        serde_json::from_str(existing).map_err(|_| "llms.json: JSON malformato".to_string())?;
    let obj = value
        .as_object_mut()
        .ok_or_else(|| "llms.json: non è un oggetto JSON".to_string())?;

    let provider_exists = obj
        .get("providers")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|arr| {
            arr.iter()
                .any(|p| p.get("name").and_then(serde_json::Value::as_str) == Some(active))
        });
    if !provider_exists {
        return Err(format!("Provider '{active}' non trovato in llms.json"));
    }

    obj.insert("active".to_string(), serde_json::json!(active));
    Ok(serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string()))
}

/// Valida e scrive il nuovo provider attivo su `llms.json`.
///
/// A differenza di `set_aichat_settings`, qui NON c'è un caso "file assente →
/// crea con default": non ha senso creare un `llms.json` vuoto scelto dalla UI
/// se l'utente non ha ancora configurato nessun provider a mano (nessuna API
/// key da mettere) — fallisce esplicitamente se il file non esiste o non
/// contiene il provider scelto.
#[tauri::command]
pub fn set_llm_settings(active: String, state: State<'_, ConfigDirState>) -> Result<(), String> {
    let path = llms_json_path(&state.config_dir);
    let existing =
        std::fs::read_to_string(&path).map_err(|e| format!("llms.json: lettura fallita ({e})"))?;
    let merged = merge_active(&existing, &active)?;
    std::fs::write(&path, merged).map_err(|e| format!("llms.json: scrittura fallita ({e})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_returns_empty_on_malformed_json() {
        let s = parse_llm_settings("not json");
        assert_eq!(s.active, "");
        assert!(s.providers.is_empty());
    }

    #[test]
    fn parse_reads_active_and_providers_ignoring_api_keys() {
        let json = r#"{
            "active": "claude-direct",
            "providers": [
                {"name":"deepseek-openrouter","provider":"openrouter","model":"deepseek/deepseek-chat","api_key_ref":"openrouter"},
                {"name":"claude-direct","provider":"anthropic","model":"claude-sonnet-4-6","api_key_ref":"anthropic"}
            ],
            "api_keys": {"openrouter":"sk-or-real-secret","anthropic":"sk-ant-real-secret"}
        }"#;
        let s = parse_llm_settings(json);
        assert_eq!(s.active, "claude-direct");
        assert_eq!(s.providers.len(), 2);
        assert_eq!(s.providers[0].name, "deepseek-openrouter");
        assert_eq!(s.providers[0].model, "deepseek/deepseek-chat");
        assert_eq!(s.providers[1].name, "claude-direct");
        assert_eq!(s.providers[1].model, "claude-sonnet-4-6");
    }

    #[test]
    fn parse_treats_missing_providers_as_empty_list() {
        let s = parse_llm_settings(r#"{"active":"x"}"#);
        assert_eq!(s.active, "x");
        assert!(s.providers.is_empty());
    }

    #[test]
    fn merge_active_rejects_empty() {
        let err = merge_active(r#"{"active":"a","providers":[{"name":"a"}]}"#, "  ")
            .expect_err("active vuoto (dopo trim) deve essere rifiutato");
        assert!(err.contains("vuoto"), "messaggio inatteso: {err}");
    }

    #[test]
    fn merge_active_rejects_unknown_provider() {
        let existing = r#"{"active":"a","providers":[{"name":"a"}]}"#;
        let err = merge_active(existing, "does-not-exist")
            .expect_err("provider inesistente deve essere rifiutato");
        assert!(err.contains("does-not-exist"), "messaggio inatteso: {err}");
    }

    #[test]
    fn merge_active_errs_on_malformed_existing() {
        let err = merge_active("not json", "a").expect_err("JSON malformato deve fallire");
        assert!(err.contains("malformato"), "messaggio inatteso: {err}");
    }

    #[test]
    fn merge_active_writes_only_active_preserving_everything_else() {
        let existing = r#"{
            "active": "claude-direct",
            "providers": [
                {"name":"deepseek-openrouter","provider":"openrouter","model":"deepseek/deepseek-chat","api_key_ref":"openrouter","max_tokens":null},
                {"name":"claude-direct","provider":"anthropic","model":"claude-sonnet-4-6","api_key_ref":"anthropic"}
            ],
            "api_keys": {"openrouter":"sk-or-real-secret","anthropic":"sk-ant-real-secret"}
        }"#;
        let out = merge_active(existing, "deepseek-openrouter").unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            v["active"], "deepseek-openrouter",
            "solo active deve cambiare"
        );
        assert_eq!(
            v["api_keys"]["openrouter"], "sk-or-real-secret",
            "api_keys devono restare intatte — questo e' il test di sicurezza centrale"
        );
        assert_eq!(
            v["api_keys"]["anthropic"], "sk-ant-real-secret",
            "api_keys intatte"
        );
        assert_eq!(
            v["providers"][0]["api_key_ref"], "openrouter",
            "provider fields intatti"
        );
        assert_eq!(
            v["providers"][1]["model"], "claude-sonnet-4-6",
            "provider fields intatti"
        );
    }
}
