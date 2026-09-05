//! Config del canale AI Chat: file JSON in app-data (gemello di search-paths.json).
//! Sorgente di verità per enabled/label_base/chat_port; riletta all'avvio del servizio.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Configurazione del canale AI Chat.
///
/// Viene serializzata come JSON in `<config_dir>/network.json` (2.0, D6 —
/// `config_dir` risolto una volta in `main()`, nessuna variabile
/// d'ambiente). Un campo `enabled: false` basta a tenere tutto il servizio
/// dormiente senza rimuovere il file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiChatConfig {
    /// Se false, il servizio non parte (default: spento).
    pub enabled: bool,
    /// Parte comune dell'etichetta di questa macchina (es. "skimble").
    /// L'etichetta completa di un peer è `label_base@hostname`.
    pub label_base: String,
    /// Porta TCP della chat (e porta UDP di scoperta — spazi di porta distinti).
    pub chat_port: u16,
    /// AI Chat Slice 1b — flag "la mia AI partecipa" (design §9.4:
    /// `Docs/superpowers/specs/2026-07-02-aichat-ai-participants-design.md`).
    ///
    /// Gata SOLO le invocazioni REMOTE (`@all`/`@<mio-label>-ai` da un umano
    /// di un'altra macchina): l'invocazione della propria macchina agisce
    /// sempre (consenso implicito, l'umano ha scritto nella PROPRIA finestra).
    /// `#[serde(default = "default_true")]` tollera i config esistenti scritti
    /// PRIMA che questo campo esistesse — senza, deserializzare un
    /// `aichat.json` più vecchio fallirebbe con "missing field".
    #[serde(default = "default_true")]
    pub ai_participates: bool,
    /// AI Chat Slice 2 — flag "auto-partecipazione" (design §10.1:
    /// `Docs/superpowers/specs/2026-07-02-aichat-ai-participants-design.md`).
    ///
    /// A differenza di `ai_participates` (che risponde a un'invocazione ESPLICITA,
    /// `@all`/`@<mio-label>-ai`), questo flag fa intervenire l'AI SPONTANEAMENTE sui
    /// messaggi normali della stanza, giudicando da sola la rilevanza
    /// (`AiAdapter::chat_autoparticipate`). È più aggressivo/costoso (rimuove la
    /// loop-safety-per-costruzione delle Slice 1a/1b — il controllo passa al cap sui
    /// turni consecutivi in `aichat/service.rs`): **opt-in esplicito, default
    /// `false`**. `#[serde(default)]` (bool → `false`) tollera i config esistenti
    /// scritti prima che questo campo esistesse.
    #[serde(default)]
    pub ai_autoparticipate: bool,
    /// Nickname dell'umano che usa questa macchina (es. "Maurizio"), mostrato in
    /// AI Chat al posto della bare label tecnica. `None` → fallback al
    /// comportamento odierno (bare `label_base` mostrato come oggi).
    /// `#[serde(default)]` tollera un `network.json` scritto prima che questo
    /// campo esistesse.
    #[serde(default)]
    pub display_name: Option<String>,
    /// Nickname che l'AI si è scelta (o si è fatta assegnare dall'utente) per
    /// questa macchina (es. "Aria"), indipendente da `display_name`. `None` →
    /// l'AI non ha ancora un nome — vedi `ai_adapter::needs_ai_name_prompt`
    /// (Task 8) e il tool `set_ai_display_name` (Task 7) che lo valorizza.
    #[serde(default)]
    pub ai_display_name: Option<String>,
}

/// Helper per `#[serde(default = "...")]`: serde richiede una FUNZIONE (non
/// un'espressione inline) per il default di un campo — `true` da sola non è
/// una sintassi valida lì. Module-level perché `default_true` deve avere un
/// nome risolvibile dall'attributo, non un closure locale.
fn default_true() -> bool {
    true
}

/// I valori di partenza sicuri: servizio spento, etichetta "lare", porta 40100,
/// AI partecipativa. L'utente cambia `enabled = true` nel file JSON per
/// attivare la funzione.
impl Default for AiChatConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            label_base: "lare".to_string(),
            chat_port: 40100,
            ai_participates: true,
            ai_autoparticipate: false,
            display_name: None,
            ai_display_name: None,
        }
    }
}

/// Carica il config dal file indicato.
///
/// Se il file è assente oppure corrotto, genera il default, lo scrive su disco
/// (così l'utente ha un template da modificare), e lo restituisce.
/// Non propaga errori di lettura: un config mancante è un caso normale
/// (primo avvio), non un errore fatale.
pub fn load_or_generate(path: &Path) -> AiChatConfig {
    // Tentiamo la lettura; un fallimento (file assente, permessi, JSON malformato)
    // è gestito silenziosamente: si cade sul default.
    if let Ok(bytes) = std::fs::read(path) {
        if let Ok(cfg) = serde_json::from_slice::<AiChatConfig>(&bytes) {
            return cfg;
        }
    }
    // Default → scriviamo il template in modo che l'utente sappia cosa configurare.
    let cfg = AiChatConfig::default();
    let _ = save(path, &cfg); // ignoriamo l'errore di scrittura (es. path non esiste ancora)
    cfg
}

/// Scrive il config come JSON indentato (leggibile a mano dall'utente).
pub fn save(path: &Path, cfg: &AiChatConfig) -> std::io::Result<()> {
    // `to_vec_pretty` produce un JSON multi-riga comodo da editare manualmente.
    let json = serde_json::to_vec_pretty(cfg).map_err(std::io::Error::other)?;
    std::fs::write(path, json)
}

/// Carica `new_path` (`network.json`). Se manca ma `legacy_path` (`aichat.json`,
/// nome storico legato ad AI Chat) esiste, migra: legge il vecchio, scrive il
/// nuovo — SENZA cancellare il vecchio (migrazione non distruttiva, v. design
/// `Docs/superpowers/specs/2026-07-29-library-notes-design.md` §9). Se
/// `new_path` esiste già, `legacy_path` viene ignorato del tutto.
pub fn load_or_generate_with_migration(new_path: &Path, legacy_path: &Path) -> AiChatConfig {
    if new_path.exists() {
        return load_or_generate(new_path);
    }
    if let Ok(bytes) = std::fs::read(legacy_path) {
        if let Ok(cfg) = serde_json::from_slice::<AiChatConfig>(&bytes) {
            let _ = save(new_path, &cfg);
            return cfg;
        }
    }
    load_or_generate(new_path)
}

// `resolve_network_json_path`/`resolve_network_json_path_in` (v1) sono state
// RIMOSSE in Task 4 (2.0, D6): risolvevano `network.json` da sole via env
// var/`startup.json` (`LARE_LOCAL_DIR`, `startup_config::resolve`, API oggi
// non più esistenti). Il path di `network.json` è ormai SEMPRE
// `<config_dir>/network.json` (vedi `RuntimeConfig::network_json_path` in
// `runtime_config.rs`) — i chiamanti reali (`agent::dispatch_tool_at`,
// `ai_adapter::needs_ai_name_prompt_at`) lo ricevono già risolto da chi li
// invoca (`McpToolClient`/`CwdTrackingToolClient`/`LlmAdapter`, che tengono
// `config_dir` come campo), mai ri-derivato qui.

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn default_is_disabled_with_port_40100() {
        let c = AiChatConfig::default();
        assert!(!c.enabled);
        assert_eq!(c.chat_port, 40100);
    }

    #[test]
    fn load_or_generate_creates_then_reads_back() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("aichat.json");
        let c1 = load_or_generate(&p); // genera il default
        assert!(p.exists());
        let mut c2 = c1.clone();
        c2.enabled = true;
        c2.label_base = "skimble".into();
        save(&p, &c2).unwrap();
        let c3 = load_or_generate(&p); // rilegge i valori salvati
        assert!(c3.enabled);
        assert_eq!(c3.label_base, "skimble");
    }

    // -------------------------------------------------------------------------
    // AI Chat Slice 1b — flag "la mia AI partecipa" (design §9.4):
    // Docs/superpowers/specs/2026-07-02-aichat-ai-participants-design.md
    // -------------------------------------------------------------------------

    /// Il default è partecipativo (`true`) — coerente con "spazio a tutte le
    /// voci" (design §9.4); opt-out facile dal tab `/config`.
    #[test]
    fn default_ai_participates_is_true() {
        let c = AiChatConfig::default();
        assert!(c.ai_participates);
    }

    /// Un config JSON scritto PRIMA che il campo esistesse (nessuna chiave
    /// `ai_participates`) deve deserializzare con `ai_participates = true`
    /// (`serde(default)`) — tolleranza alla migrazione, nessun file esistente
    /// si rompe con questa slice.
    #[test]
    fn missing_field_deserializes_as_true() {
        let json = r#"{"enabled":true,"label_base":"skimble","chat_port":40100}"#;
        let c: AiChatConfig = serde_json::from_str(json).expect("deserializzazione fallita");
        assert!(c.ai_participates, "un config senza il campo deve assumere true");
    }

    // -------------------------------------------------------------------------
    // AI Chat Slice 2 — flag "auto-partecipazione" (design §10.1):
    // Docs/superpowers/specs/2026-07-02-aichat-ai-participants-design.md
    // -------------------------------------------------------------------------

    /// A differenza di `ai_participates` (default `true`), l'auto-partecipazione è
    /// più aggressiva/costosa (rimuove la loop-safety-per-costruzione): default
    /// `false`, opt-in esplicito dal tab `/config`.
    #[test]
    fn default_ai_autoparticipate_is_false() {
        let c = AiChatConfig::default();
        assert!(!c.ai_autoparticipate);
    }

    /// Un config JSON scritto PRIMA che il campo esistesse deve deserializzare con
    /// `ai_autoparticipate = false` (`serde(default)`, bool → `false`) — nessun file
    /// esistente si rompe con questa slice, e il default resta prudente (silenzio).
    #[test]
    fn missing_ai_autoparticipate_field_deserializes_as_false() {
        let json = r#"{"enabled":true,"label_base":"skimble","chat_port":40100,"ai_participates":true}"#;
        let c: AiChatConfig = serde_json::from_str(json).expect("deserializzazione fallita");
        assert!(!c.ai_autoparticipate, "un config senza il campo deve assumere false");
    }

    // -------------------------------------------------------------------------
    // Migrazione aichat.json → network.json (non distruttiva)
    // -------------------------------------------------------------------------

    #[test]
    fn migration_reads_legacy_when_new_absent_and_writes_new_file() {
        let dir = TempDir::new().unwrap();
        let legacy = dir.path().join("aichat.json");
        let new = dir.path().join("network.json");
        let mut cfg = AiChatConfig::default();
        cfg.enabled = true;
        cfg.label_base = "skimble".into();
        save(&legacy, &cfg).unwrap();

        let loaded = load_or_generate_with_migration(&new, &legacy);

        assert!(loaded.enabled);
        assert_eq!(loaded.label_base, "skimble");
        assert!(new.exists(), "la migrazione deve scrivere network.json");
        assert!(legacy.exists(), "il file legacy NON va cancellato (non distruttiva)");
    }

    #[test]
    fn migration_ignores_legacy_when_new_already_present() {
        let dir = TempDir::new().unwrap();
        let legacy = dir.path().join("aichat.json");
        let new = dir.path().join("network.json");
        let mut legacy_cfg = AiChatConfig::default();
        legacy_cfg.label_base = "legacy-name".into();
        save(&legacy, &legacy_cfg).unwrap();
        let mut new_cfg = AiChatConfig::default();
        new_cfg.label_base = "current-name".into();
        save(&new, &new_cfg).unwrap();

        let loaded = load_or_generate_with_migration(&new, &legacy);

        assert_eq!(loaded.label_base, "current-name", "network.json esistente vince, legacy ignorato");
    }

    #[test]
    fn migration_generates_default_when_neither_file_exists() {
        let dir = TempDir::new().unwrap();
        let legacy = dir.path().join("aichat.json");
        let new = dir.path().join("network.json");

        let loaded = load_or_generate_with_migration(&new, &legacy);

        assert!(!loaded.enabled, "default pulito: disabilitato");
        assert!(new.exists());
        assert!(!legacy.exists(), "nessun file legacy non deve comparire dal nulla");
    }

    // -------------------------------------------------------------------------
    // Nickname umano + AI (display_name / ai_display_name) — campi additivi
    // per mostrare un nickname al posto della bare label tecnica in AI Chat.
    // Vedi Docs/superpowers/specs/2026-08-13-aichat-display-names-design.md.
    // -------------------------------------------------------------------------

    /// Round-trip completo: i due nuovi campi sopravvivono a save+load.
    #[test]
    fn load_or_generate_round_trips_display_names() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("network.json");
        let mut c = AiChatConfig::default();
        assert_eq!(c.display_name, None, "default: nessun nickname umano");
        assert_eq!(c.ai_display_name, None, "default: nessun nickname AI");
        c.display_name = Some("Maurizio".to_string());
        c.ai_display_name = Some("Aria".to_string());
        save(&p, &c).unwrap();
        let c2 = load_or_generate(&p);
        assert_eq!(c2.display_name, Some("Maurizio".to_string()));
        assert_eq!(c2.ai_display_name, Some("Aria".to_string()));
    }

    /// Un `network.json` scritto PRIMA che questi campi esistessero (nessuna
    /// delle due chiavi presente) deve deserializzare con entrambi a `None`
    /// (`#[serde(default)]`), non fallire con "missing field".
    #[test]
    fn missing_display_name_fields_read_as_none() {
        let json = r#"{"enabled":true,"label_base":"skimble","chat_port":40100}"#;
        let cfg: AiChatConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.display_name, None);
        assert_eq!(cfg.ai_display_name, None);
    }

    // `resolve_network_json_path`/`resolve_network_json_path_in` e i loro
    // test sono stati RIMOSSI in Task 4 (2.0, D6) — vedi il commento al posto
    // delle due funzioni sopra. La risoluzione del path di `network.json` è
    // ormai coperta da `runtime_config::RuntimeConfig::network_json_path`
    // (test in `runtime_config.rs`).
}
