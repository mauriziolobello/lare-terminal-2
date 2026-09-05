//! # llms_config — selezione del provider AI attivo (`llms/llms.json`)
//!
//! Il file è OPZIONALE: se assente, illeggibile o non valido, l'orchestrator usa
//! il comportamento pre-Slice-4 (branch `ANTHROPIC_API_KEY`/`StubAdapter` in
//! `main.rs`) — retrocompatibilità DURA, requisito della spec
//! (`Docs/superpowers/specs/2026-07-06-openrouter-tooluse-adapter-design.md` §6).
//! Un solo provider attivo per macchina serve sia il cursore sia AI Chat
//! (decisione utente, vedi memoria `multi-llm-openrouter`).
//!
//! Vive in `<config_dir>/llms.json` (2.0: `config_dir` è `--config-dir` o
//! `<exe_dir>/Configuration` di default — vedi `startup_config` — nessuna
//! variabile d'ambiente, D6. `ui.exe` risolve lo stesso `config_dir` con la
//! stessa regola, quindi legge sempre lo stesso file senza condividere una
//! cartella di lancio).
//!
//! **Sicurezza:** le API key in `api_keys` non vengono MAI loggate né incluse
//! nei messaggi di errore — stessa cautela già presa in `telegram::settings`
//! per il token del bot.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::ai_adapter::{AiAdapter, LlmAdapter};
use crate::claude_backend::ClaudeBackend;
use crate::messages_client::HttpMessagesClient;
use crate::openrouter_backend::{HttpOpenRouterClient, OpenRouterBackend};

/// Schema di `llms/llms.json`.
///
/// Non implementa `Debug` intenzionalmente: porta `api_keys` (vedi sotto), quindi un
/// eventuale `{:?}` accidentale in un log non deve poter stampare le key reali — stessa
/// cautela già presa in `telegram::settings::TelegramSettings` per il token del bot (il
/// modulo che questo mirror-a: "il campo `token` non deve mai comparire nei log").
#[derive(Clone, PartialEq, serde::Deserialize)]
pub struct LlmsConfig {
    pub active: String,
    pub providers: Vec<ProviderConfig>,
    pub api_keys: HashMap<String, String>,
}

/// Un provider configurato in `providers[]`.
///
/// Non implementa `Debug` per lo stesso motivo di `LlmsConfig`: pur non portando essa
/// stessa una key, viaggia sempre insieme a `LlmsConfig` nelle firme pubbliche del modulo
/// (es. `find_active`) — ometterlo qui evita la tentazione di derivarlo "per comodità" più
/// avanti su un tipo che finirebbe comunque annidato in una struttura con segreti.
#[derive(Clone, PartialEq, serde::Deserialize)]
pub struct ProviderConfig {
    pub name: String,
    /// `"anthropic"` o `"openrouter"` — qualunque altro valore è un errore a
    /// `build_adapter` (nessuna probing statica di altri provider: solo questi
    /// due hanno un `ChatBackend` oggi, vedi Slice 1-3).
    pub provider: String,
    pub model: String,
    /// Chiave in `api_keys` che porta la API key reale di questo provider.
    pub api_key_ref: String,
    /// Anthropic lo richiede sempre (default `DEFAULT_ANTHROPIC_MAX_TOKENS` se
    /// assente); OpenRouter lo tratta come opzionale, esattamente come
    /// `OpenRouterBackend::new` (Slice 2).
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// Endpoint HTTP alternativo — assente (default) usa l'host reale del provider
    /// (`api.anthropic.com` per `"anthropic"`, `openrouter.ai` per `"openrouter"`).
    /// Permette di puntare un provider a un endpoint COMPATIBILE con lo stesso wire ma
    /// esposto da un host diverso — usato per DeepSeek in due modi alternativi: `provider:
    /// "anthropic"` + `base_url: "https://api.deepseek.com/anthropic"` (shim
    /// Anthropic-compatibile, vedi `web_fetch_supported` sotto — versione immatura,
    /// leaka a volte tool-call native come testo grezzo, bug osservato dal vivo), oppure
    /// `provider: "openrouter"` + `base_url: "https://api.deepseek.com"` (endpoint
    /// OpenAI-compatibile DIRETTO di DeepSeek, senza passare da OpenRouter — stesso wire
    /// già parlato da `OpenRouterBackend`, solo un host diverso; niente di
    /// DeepSeek-specifico nel codice, è generico per QUALUNQUE endpoint OpenAI-compatibile).
    #[serde(default)]
    pub base_url: Option<String>,
    /// `false` se questo endpoint Anthropic-COMPATIBILE non implementa il tool
    /// server-side `web_fetch` (solo `web_search`). Default `true` (Anthropic reale
    /// supporta entrambi). Serve per DeepSeek (`base_url` custom): il loro shim
    /// risponde HTTP 400 "unknown variant web_fetch_20260209" se lo si include — la
    /// ricerca web è attiva di default, quindi senza questo flag OGNI comando in
    /// linguaggio naturale falliva (bug reale osservato in produzione). Ignorato per
    /// `provider: "openrouter"` (scarta comunque tutti i `ToolSpec::Server`, vedi
    /// `openrouter_backend::to_or_tools`).
    #[serde(default = "default_web_fetch_supported")]
    pub web_fetch_supported: bool,
}

/// Default di `ProviderConfig::web_fetch_supported` quando il campo è assente in
/// config: `true`, coerente con Anthropic reale (nessuna rottura per chi non ha
/// ancora questo campo nel proprio `llms.json`).
fn default_web_fetch_supported() -> bool {
    true
}

/// Default `max_tokens` per Anthropic quando il campo è assente in config —
/// stesso valore hard-coded già usato nel branch di fallback (`main.rs`,
/// `default_ai_adapter`, Task 2).
const DEFAULT_ANTHROPIC_MAX_TOKENS: u32 = 16000;

/// Default `base_url` per Anthropic quando il campo è assente in config — stesso valore
/// hard-coded in `HttpMessagesClient::new` (`messages_client.rs`).
const DEFAULT_ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";

/// Risolve il `base_url` effettivo per un provider `"anthropic"`: quello custom se
/// presente in config, altrimenti l'host Anthropic reale.
fn resolve_anthropic_base_url(provider: &ProviderConfig) -> String {
    provider
        .base_url
        .clone()
        .unwrap_or_else(|| DEFAULT_ANTHROPIC_BASE_URL.to_string())
}

/// Risolve il path di `llms/llms.json`: SEMPRE `<config_dir>/llms.json`,
/// nessuna variabile d'ambiente né campo `startup.json` dedicato (D6, 2.0
/// — la v1 aveva 3 livelli di precedenza qui, `LARE_LLMS_CONFIG` compreso).
/// `config_dir` è la stessa cartella risolta una volta in `main()` da
/// `startup_config::config_dir_from_process()`.
pub fn resolve_path(config_dir: &Path) -> PathBuf {
    config_dir.join("llms.json")
}

/// Carica `llms/llms.json` dal path indicato.
///
/// - `Ok(None)` — file assente: nessun errore, è il caso normale finché
///   l'utente non ha ancora configurato un provider alternativo.
/// - `Ok(Some(cfg))` — file trovato e JSON valido (la VALIDAZIONE semantica,
///   es. provider attivo esistente, è responsabilità di `build_adapter`).
/// - `Err(_)` — file presente ma illeggibile o JSON malformato.
pub fn load(path: &Path) -> std::io::Result<Option<LlmsConfig>> {
    match std::fs::read_to_string(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
        Ok(content) => {
            let parsed: LlmsConfig = serde_json::from_str(&content).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "llms.json: JSON malformato o schema non valido",
                )
            })?;
            Ok(Some(parsed))
        }
    }
}

/// Trova il provider indicato da `cfg.active` fra `cfg.providers`.
pub fn find_active(cfg: &LlmsConfig) -> Result<&ProviderConfig, String> {
    cfg.providers
        .iter()
        .find(|p| p.name == cfg.active)
        .ok_or_else(|| {
            format!(
                "provider attivo '{}' non trovato in providers[]",
                cfg.active
            )
        })
}

/// Risolve la API key del provider (via `api_key_ref`). Il valore non compare
/// MAI nel messaggio di errore restituito — solo il nome del riferimento
/// mancante.
pub fn resolve_api_key(cfg: &LlmsConfig, provider: &ProviderConfig) -> Result<String, String> {
    cfg.api_keys
        .get(&provider.api_key_ref)
        .cloned()
        .ok_or_else(|| {
            format!(
                "api_key_ref '{}' non trovato in api_keys",
                provider.api_key_ref
            )
        })
}

/// Costruisce l'`AiAdapter` per il provider attivo di `cfg`. `Err` copre
/// qualunque config semanticamente invalida (provider attivo assente, key
/// mancante, campo `provider` sconosciuto) — il chiamante (`main.rs`, Task 2)
/// tratta QUALUNQUE `Err` come "usa il comportamento pre-Slice-4", mai come
/// un crash.
///
/// `config_dir` viene passato a `LlmAdapter` (2.0, Task 4): serve a risolvere
/// `memory-{label_base}.md` (`ai_adapter::memory_file_path`) quando questo
/// adapter viene usato anche per il canale AI Chat — nessuna variabile
/// d'ambiente, D6.
pub fn build_adapter(cfg: &LlmsConfig, config_dir: &Path) -> Result<Arc<dyn AiAdapter>, String> {
    let provider = find_active(cfg)?;
    let key = resolve_api_key(cfg, provider)?;
    match provider.provider.as_str() {
        "anthropic" => {
            let max_tokens = provider.max_tokens.unwrap_or(DEFAULT_ANTHROPIC_MAX_TOKENS);
            let base_url = resolve_anthropic_base_url(provider);
            let http = Arc::new(HttpMessagesClient::with_base_url(key, base_url));
            let backend = Arc::new(
                ClaudeBackend::new(http, provider.model.clone(), max_tokens)
                    .with_web_fetch_supported(provider.web_fetch_supported),
            );
            Ok(Arc::new(LlmAdapter::new(
                backend,
                provider.name.clone(),
                config_dir.to_path_buf(),
            )))
        }
        "openrouter" => {
            // `HttpOpenRouterClient::new` incapsula già il default (`openrouter.ai`) — a
            // differenza del ramo `"anthropic"` sopra non serve una costante/funzione
            // `resolve_*` duplicata qui, basta delegare quando `base_url` è assente.
            let http = match &provider.base_url {
                Some(url) => Arc::new(HttpOpenRouterClient::with_base_url(key, url.clone())),
                None => Arc::new(HttpOpenRouterClient::new(key)),
            };
            let backend = Arc::new(OpenRouterBackend::new(
                http,
                provider.model.clone(),
                provider.max_tokens,
            ));
            Ok(Arc::new(LlmAdapter::new(
                backend,
                provider.name.clone(),
                config_dir.to_path_buf(),
            )))
        }
        other => Err(format!(
            "provider '{other}' sconosciuto (atteso 'anthropic' o 'openrouter')"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    fn sample_config(active: &str) -> String {
        format!(
            r#"{{
                "active": "{active}",
                "providers": [
                    {{"name": "deepseek-openrouter", "provider": "openrouter",
                      "model": "deepseek/deepseek-chat", "api_key_ref": "openrouter"}},
                    {{"name": "claude-direct", "provider": "anthropic",
                      "model": "claude-sonnet-4-6", "api_key_ref": "anthropic"}}
                ],
                "api_keys": {{"openrouter": "sk-or-fake", "anthropic": "sk-ant-fake"}}
            }}"#
        )
    }

    // ── resolve_path ─────────────────────────────────────────────────────

    #[test]
    fn resolve_path_is_config_dir_join_llms_json() {
        let p = resolve_path(PathBuf::from("/config").as_path());
        assert_eq!(p, PathBuf::from("/config").join("llms.json"));
    }

    // ── load ──────────────────────────────────────────────────────────────────

    #[test]
    fn missing_file_returns_ok_none() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("llms.json");
        let result = load(&missing).unwrap();
        assert!(result.is_none(), "file assente deve restituire Ok(None)");
    }

    #[test]
    fn valid_file_returns_ok_some() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "{}", sample_config("claude-direct")).unwrap();
        let cfg = load(f.path()).unwrap().expect("atteso Some");
        assert_eq!(cfg.active, "claude-direct");
        assert_eq!(cfg.providers.len(), 2);
    }

    #[test]
    fn malformed_json_returns_err() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "not json at all").unwrap();
        assert!(load(f.path()).is_err());
    }

    // ── find_active / resolve_api_key ───────────────────────────────────────

    #[test]
    fn find_active_locates_the_named_provider() {
        let cfg: LlmsConfig = serde_json::from_str(&sample_config("deepseek-openrouter")).unwrap();
        let p = find_active(&cfg).unwrap();
        assert_eq!(p.name, "deepseek-openrouter");
        assert_eq!(p.provider, "openrouter");
    }

    #[test]
    fn find_active_errs_when_active_name_not_in_providers() {
        let cfg: LlmsConfig = serde_json::from_str(&sample_config("does-not-exist")).unwrap();
        // `.err().unwrap()` invece di `.unwrap_err()`: `ProviderConfig` non implementa
        // `Debug` di proposito (vedi commento sul tipo) — stesso idiom usato sotto per
        // `build_adapter_errs_on_unknown_provider_kind`.
        let err = find_active(&cfg).err().unwrap();
        assert!(err.contains("does-not-exist"));
    }

    #[test]
    fn resolve_api_key_returns_the_referenced_key() {
        let cfg: LlmsConfig = serde_json::from_str(&sample_config("claude-direct")).unwrap();
        let provider = find_active(&cfg).unwrap();
        let key = resolve_api_key(&cfg, provider).unwrap();
        assert_eq!(key, "sk-ant-fake");
    }

    #[test]
    fn resolve_api_key_errs_and_does_not_leak_when_ref_missing() {
        let mut cfg: LlmsConfig = serde_json::from_str(&sample_config("claude-direct")).unwrap();
        cfg.providers[1].api_key_ref = "missing-ref".to_string();
        let provider = find_active(&cfg).unwrap();
        let err = resolve_api_key(&cfg, provider).unwrap_err();
        assert!(err.contains("missing-ref"));
        assert!(
            !err.contains("sk-ant-fake"),
            "l'errore non deve MAI contenere una key reale"
        );
    }

    // ── web_fetch_supported ──────────────────────────────────────────────────

    #[test]
    fn web_fetch_supported_defaults_to_true_when_absent() {
        let cfg: LlmsConfig = serde_json::from_str(&sample_config("claude-direct")).unwrap();
        let provider = find_active(&cfg).unwrap();
        assert!(provider.web_fetch_supported);
    }

    #[test]
    fn web_fetch_supported_reads_explicit_false() {
        let json = r#"{
            "active": "deepseek-direct",
            "providers": [
                {"name": "deepseek-direct", "provider": "anthropic",
                 "model": "deepseek-v4-pro", "api_key_ref": "deepseek",
                 "base_url": "https://api.deepseek.com/anthropic",
                 "web_fetch_supported": false}
            ],
            "api_keys": {"deepseek": "sk-fake"}
        }"#;
        let cfg: LlmsConfig = serde_json::from_str(json).unwrap();
        let provider = find_active(&cfg).unwrap();
        assert!(!provider.web_fetch_supported);
    }

    // ── resolve_anthropic_base_url ──────────────────────────────────────────

    #[test]
    fn resolve_anthropic_base_url_defaults_to_anthropic_when_absent() {
        let cfg: LlmsConfig = serde_json::from_str(&sample_config("claude-direct")).unwrap();
        let provider = find_active(&cfg).unwrap();
        assert_eq!(
            resolve_anthropic_base_url(provider),
            "https://api.anthropic.com"
        );
    }

    #[test]
    fn resolve_anthropic_base_url_uses_custom_value_when_present() {
        let mut cfg: LlmsConfig = serde_json::from_str(&sample_config("claude-direct")).unwrap();
        cfg.providers[1].base_url = Some("https://api.deepseek.com/anthropic".to_string());
        let provider = find_active(&cfg).unwrap();
        assert_eq!(
            resolve_anthropic_base_url(provider),
            "https://api.deepseek.com/anthropic"
        );
    }

    // ── build_adapter ─────────────────────────────────────────────────────────

    #[test]
    fn build_adapter_selects_openrouter_backend_by_provider_name() {
        let cfg: LlmsConfig = serde_json::from_str(&sample_config("deepseek-openrouter")).unwrap();
        let adapter = build_adapter(&cfg, Path::new("/test-config")).unwrap();
        assert_eq!(adapter.provider(), "deepseek-openrouter");
    }

    #[test]
    fn build_adapter_selects_claude_backend_by_provider_name() {
        let cfg: LlmsConfig = serde_json::from_str(&sample_config("claude-direct")).unwrap();
        let adapter = build_adapter(&cfg, Path::new("/test-config")).unwrap();
        assert_eq!(adapter.provider(), "claude-direct");
    }

    /// Un provider `"anthropic"` con `base_url` custom (es. l'endpoint Anthropic-compatibile
    /// di DeepSeek, non passando da OpenRouter) costruisce comunque l'adapter senza errori —
    /// la plumbing di `base_url` non rompe il percorso esistente.
    #[test]
    fn build_adapter_succeeds_with_custom_anthropic_base_url() {
        let mut cfg: LlmsConfig = serde_json::from_str(&sample_config("claude-direct")).unwrap();
        cfg.providers[1].base_url = Some("https://api.deepseek.com/anthropic".to_string());
        let adapter = build_adapter(&cfg, Path::new("/test-config")).unwrap();
        assert_eq!(adapter.provider(), "claude-direct");
    }

    /// Un provider `"openrouter"` con `base_url` custom (es. l'endpoint OpenAI-compatibile
    /// DIRETTO di DeepSeek, `https://api.deepseek.com`, bypassando OpenRouter) costruisce
    /// comunque l'adapter senza errori — stesso principio del test gemello per `"anthropic"`
    /// sopra: la plumbing di `base_url` non rompe il percorso esistente (default assente =
    /// `openrouter.ai`, invariato).
    #[test]
    fn build_adapter_succeeds_with_custom_openrouter_base_url() {
        let mut cfg: LlmsConfig =
            serde_json::from_str(&sample_config("deepseek-openrouter")).unwrap();
        cfg.providers[0].base_url = Some("https://api.deepseek.com".to_string());
        let adapter = build_adapter(&cfg, Path::new("/test-config")).unwrap();
        assert_eq!(adapter.provider(), "deepseek-openrouter");
    }

    #[test]
    fn build_adapter_errs_on_unknown_provider_kind() {
        let mut cfg: LlmsConfig = serde_json::from_str(&sample_config("claude-direct")).unwrap();
        cfg.providers[1].provider = "gemini-direct".to_string();
        // `unwrap_err()` richiede `T: Debug` sul tipo `Ok` (qui `Arc<dyn AiAdapter>`, che non
        // implementa `Debug`) — usiamo `.err().unwrap()` invece, stesso idiom già adottato in
        // `telegram::settings` per lo stesso identico vincolo del compilatore.
        let err = build_adapter(&cfg, Path::new("/test-config"))
            .err()
            .unwrap();
        assert!(err.contains("gemini-direct"));
    }

    #[test]
    fn build_adapter_errs_when_active_provider_missing() {
        let cfg: LlmsConfig = serde_json::from_str(&sample_config("nonexistent")).unwrap();
        assert!(build_adapter(&cfg, Path::new("/test-config")).is_err());
    }
}
