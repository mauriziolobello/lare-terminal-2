//! Lettura/scrittura dei parametri di rete condivisi (enabled, label_base,
//! chat_port) in network.json (di proprietà dell'orchestrator, in `<config_dir>`).
//! Mirror di `search_settings.rs`: stesso pattern (merge preserva gli altri
//! campi, stessa cartella — `ConfigDirState`, 2.0).
//!
//! **Nome del file** — era `aichat.json` fino al piano "Blocco note": il file
//! porta l'identità di rete condivisa da PIÙ funzionalità (AI Chat *e* Blocco
//! note), non solo AI Chat, quindi è stato rinominato `network.json` (design
//! `Docs/superpowers/specs/2026-07-29-library-notes-design.md` §9). Qui
//! rispecchiamo la stessa migrazione non distruttiva già implementata lato
//! orchestrator in `aichat::config::load_or_generate_with_migration`:
//! - in LETTURA: `network.json` se esiste, altrimenti fallback al vecchio
//!   `aichat.json` (una macchina può aprire /config prima di aver mai avviato
//!   l'orchestrator aggiornato — senza il fallback vedrebbe i default al posto
//!   delle sue impostazioni);
//! - in SCRITTURA: SEMPRE e SOLO `network.json` (mai più `aichat.json`),
//!   coerente con "network.json vince, nessun merge" della migrazione.
//!
//! A differenza dei parametri di ricerca, questi NON sono "live": il servizio
//! AI Chat legge network.json solo all'avvio dell'orchestrator — la UI lo
//! segnala con una nota nel tab "AI Chat" di /config.

use std::path::{Path, PathBuf};
use tauri::State;

use crate::config_dir::ConfigDirState;

/// I 3 campi di network.json editabili da /config.
///
/// Mirror "sul filo" di `orchestrator::aichat::config::AiChatConfig`: stessi
/// nomi/tipi di campo, ma è un tipo Rust separato — la UI e l'orchestrator
/// sono processi/crate distinti che comunicano SOLO tramite lo stesso file
/// JSON su disco, non tramite un tipo condiviso (niente dipendenza `ui` →
/// `orchestrator`, che romperebbe la separazione dei crate).
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct AiChatSettings {
    pub enabled: bool,
    pub label_base: String,
    pub chat_port: u16,
    /// AI Chat Slice 1b — flag "la mia AI partecipa" (mirror di
    /// `orchestrator::aichat::config::AiChatConfig::ai_participates`). Gata
    /// SOLO le invocazioni REMOTE (`@all`/`@<mio-label>-ai` da un umano di
    /// un'altra macchina): la propria invocazione agisce sempre.
    pub ai_participates: bool,
    /// AI Chat Slice 2 — flag "auto-partecipazione" (mirror di
    /// `orchestrator::aichat::config::AiChatConfig::ai_autoparticipate`). A
    /// differenza di `ai_participates` (risponde a un'invocazione ESPLICITA),
    /// fa intervenire l'AI SPONTANEAMENTE sui messaggi normali della stanza,
    /// giudicando da sola la rilevanza. Opt-in esplicito: default `false`
    /// (diverso da `ai_participates`, che di default è `true`).
    pub ai_autoparticipate: bool,
    /// Nickname dell'umano (mirror di `AiChatConfig::display_name` lato
    /// orchestrator). `None` → nessun nickname configurato.
    pub display_name: Option<String>,
    /// Nickname dell'AI (mirror di `AiChatConfig::ai_display_name`). Normalmente
    /// popolato dal tool `set_ai_display_name` (Task 7) al posto che da qui, ma
    /// resta editabile a mano in ogni momento — le due strade scrivono sullo
    /// stesso campo, nessun conflitto.
    pub ai_display_name: Option<String>,
}

/// Imposta enabled/label_base/chat_port in `existing` (JSON object), preservando
/// ogni altro campo presente. Se `existing` non è un object JSON valido (file
/// assente, corrotto, o non un object) riparte da `{}`. Output pretty (leggibile
/// a mano, come per search-paths.json).
pub fn merge_aichat_settings(existing: &str, s: &AiChatSettings) -> String {
    // `serde_json::from_str` fallisce (input vuoto/malformato) → `.ok()` lo
    // trasforma in `None`; `.filter(is_object)` scarta anche un JSON valido ma
    // non-object (es. `[1,2,3]`). In entrambi i casi ripartiamo da un object vuoto.
    let mut value: serde_json::Value = serde_json::from_str(existing)
        .ok()
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    // `.expect()` è sicuro qui: il ramo sopra garantisce sempre un Value::Object.
    let obj = value.as_object_mut().expect("object garantito sopra");
    obj.insert("enabled".to_string(), serde_json::json!(s.enabled));
    obj.insert("label_base".to_string(), serde_json::json!(s.label_base));
    obj.insert("chat_port".to_string(), serde_json::json!(s.chat_port));
    obj.insert(
        "ai_participates".to_string(),
        serde_json::json!(s.ai_participates),
    );
    obj.insert(
        "ai_autoparticipate".to_string(),
        serde_json::json!(s.ai_autoparticipate),
    );
    // A differenza dei campi sopra (sempre presenti), i due nickname sono
    // opzionali: `remove` invece di `insert(json!(null))` — un nickname
    // cancellato dall'utente deve sparire dal file, non restare come chiave
    // `null`. Più pulito non scriverla affatto.
    match &s.display_name {
        Some(v) => {
            obj.insert("display_name".to_string(), serde_json::json!(v));
        }
        None => {
            obj.remove("display_name");
        }
    }
    match &s.ai_display_name {
        Some(v) => {
            obj.insert("ai_display_name".to_string(), serde_json::json!(v));
        }
        None => {
            obj.remove("ai_display_name");
        }
    }
    serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string())
}

/// Percorso di network.json — il file corrente, l'UNICO su cui si SCRIVE.
/// SEMPRE `<config_dir>/network.json` (2.0: `ConfigDirState`, stessa cartella
/// dell'orchestrator — niente più variabili d'ambiente).
fn network_json_path(config_dir: &Path) -> PathBuf {
    config_dir.join("network.json")
}

/// Percorso del vecchio aichat.json — nome storico, letto SOLO in fallback
/// quando `network.json` non esiste ancora (macchina che non ha mai avviato
/// l'orchestrator aggiornato, che è chi esegue la migrazione vera). Non viene
/// mai scritto né cancellato: la migrazione è non distruttiva da entrambi i
/// lati (v. `aichat::config::load_or_generate_with_migration` lato orchestrator).
fn legacy_aichat_json_path(config_dir: &Path) -> PathBuf {
    config_dir.join("aichat.json")
}

/// Contenuto grezzo del file di config corrente, con fallback al nome legacy.
///
/// Precedenza identica a quella dell'orchestrator: si guarda l'ESISTENZA di
/// `network.json` (non "la lettura è riuscita") — se c'è, è la sorgente di
/// verità anche se corrotto, esattamente come `load_or_generate_with_migration`
/// che su `new_path.exists()` non guarda più il legacy. Stringa vuota se non
/// esiste nessuno dei due: i chiamanti la trattano già come "riparti dai
/// default"/"riparti da `{}`".
///
/// Fa I/O, quindi non è testabile come funzione pura (a differenza di
/// `merge_aichat_settings`/`validate`): resta un helper condiviso da
/// `get_aichat_settings` e `set_aichat_settings` per non duplicare la regola di
/// precedenza in due punti — un solo posto che decide "da dove si legge".
fn read_existing_settings_json(config_dir: &Path) -> String {
    let current = network_json_path(config_dir);
    if current.exists() {
        return std::fs::read_to_string(current).unwrap_or_default();
    }
    std::fs::read_to_string(legacy_aichat_json_path(config_dir)).unwrap_or_default()
}

/// Valida i campi prima di scrivere su disco. Funzione pura (nessun I/O): può
/// essere testata in isolamento senza toccare il filesystem — è il "seam" che
/// rende `set_aichat_settings` testabile senza dover mockare tauri::command.
///
/// - `label_base`: dopo `trim()`, non vuota, lunghezza 1..=32, solo caratteri
///   `[A-Za-z0-9_-]`. Motivo: sul wire e nell'UI diventa `"<label_base>-human"`
///   (vedi `orchestrator/src/aichat/service.rs`) — spazi/`@`/`.` romperebbero
///   quel formato o confonderebbero l'etichetta con un hostname/email.
/// - `chat_port`: 1024..=65535 — le porte sotto 1024 sono "well-known"/privilegiate
///   (su molti sistemi richiedono permessi elevati per il bind).
fn validate(s: &AiChatSettings) -> Result<(), String> {
    let trimmed = s.label_base.trim();
    if trimmed.is_empty() {
        return Err("Etichetta: non può essere vuota".to_string());
    }
    if trimmed.chars().count() > 32 {
        return Err("Etichetta: massimo 32 caratteri".to_string());
    }
    let charset_ok = trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if !charset_ok {
        return Err(
            "Etichetta: solo lettere, cifre, '_' e '-' (niente spazi o simboli)".to_string(),
        );
    }
    if !(1024..=65535).contains(&s.chat_port) {
        return Err("Porta: deve essere un intero tra 1024 e 65535".to_string());
    }
    validate_nickname(&s.display_name, "Nickname")?;
    validate_nickname(&s.ai_display_name, "Nickname AI")?;
    Ok(())
}

/// Valida un nickname libero (display_name o ai_display_name): trim, non
/// vuoto SE presente (un campo `None` è sempre valido — "nessun nickname"),
/// lunghezza 1..=48, nessuna restrizione di charset (testo libero: spazi,
/// accenti, qualunque carattere stampabile) — a differenza di `label_base`.
/// `field_label` compare nel messaggio d'errore ("Nickname"/"Nickname AI") per
/// distinguere quale dei due campi ha fallito la validazione.
fn validate_nickname(value: &Option<String>, field_label: &str) -> Result<(), String> {
    // `let ... else` (Rust 2021+): se `value` è `None` usciamo subito con `Ok`
    // — nessun nickname configurato non è un errore, è lo stato di default.
    let Some(raw) = value else {
        return Ok(());
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(format!(
            "{field_label}: non può essere vuoto (ometti il campo invece di lasciarlo vuoto)"
        ));
    }
    if trimmed.chars().count() > 48 {
        return Err(format!("{field_label}: massimo 48 caratteri"));
    }
    Ok(())
}

/// Legge i parametri correnti di AI Chat da network.json (con fallback al
/// vecchio aichat.json se il primo non esiste ancora — v. il commento di
/// modulo e `read_existing_settings_json`).
///
/// Infallibile (come `get_search_settings`): se il file è assente/corrotto o
/// un campo manca, restituisce il default corrispondente (`enabled=false`,
/// `label_base="lare"`, `chat_port=40100` — gli stessi di
/// `AiChatConfig::default()` lato orchestrator) invece di propagare un errore.
/// Il tab /config deve sempre potersi aprire, anche al primissimo avvio.
#[tauri::command]
pub fn get_aichat_settings(state: State<'_, ConfigDirState>) -> AiChatSettings {
    let content = read_existing_settings_json(&state.config_dir);
    let value: serde_json::Value =
        serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}));
    let enabled = value
        .get("enabled")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let label_base = value
        .get("label_base")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| "lare".to_string());
    let chat_port = value
        .get("chat_port")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(40100) as u16;
    // Default `true` — coerente con `AiChatConfig::default()` lato orchestrator
    // (design §9.4: "partecipativo" di default, opt-out facile dal tab).
    // Copre sia il file assente/corrotto (ramo sopra) sia un `aichat.json`
    // scritto PRIMA che questo campo esistesse (Slice 1a, chiave assente).
    let ai_participates = value
        .get("ai_participates")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);
    // Default `false` — a differenza di `ai_participates`, l'auto-partecipazione è
    // opt-in esplicito (design §10.1: più aggressiva/costosa, rimuove la
    // loop-safety-per-costruzione delle Slice 1a/1b). Copre sia il file
    // assente/corrotto sia un `aichat.json` scritto prima che il campo esistesse.
    let ai_autoparticipate = value
        .get("ai_autoparticipate")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    // Nickname umano + AI: `None` è lo stato di default (nessuna chiave nel
    // JSON, file assente/corrotto, o un `network.json` scritto prima che
    // questi due campi esistessero) — stesso pattern infallibile degli altri
    // campi, ma qui il default è `Option::None` invece di un valore concreto.
    let display_name = value
        .get("display_name")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let ai_display_name = value
        .get("ai_display_name")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    AiChatSettings {
        enabled,
        label_base,
        chat_port,
        ai_participates,
        ai_autoparticipate,
        display_name,
        ai_display_name,
    }
}

/// Valida e scrive i parametri di AI Chat su network.json.
///
/// La scrittura va SEMPRE su `network.json`, mai più sul vecchio `aichat.json`
/// — è il file che l'orchestrator legge davvero (`network.json` vince sempre
/// sul legacy nella sua migrazione). I campi preesistenti da preservare nel
/// merge si leggono invece con la stessa precedenza della lettura
/// (`read_existing_settings_json`): così, la prima volta che si salva da una
/// macchina non ancora migrata, gli altri campi del vecchio file (es.
/// `peer_ttl_secs`) vengono travasati nel nuovo invece di andare persi.
///
/// La label viene "trimmata" prima di validare/salvare (coerente col messaggio
/// di `validate`, che valuta la versione trimmata). Nota importante per
/// l'utente: a differenza di `set_search_settings`, questi valori NON sono
/// letti live dall'orchestrator — hanno effetto solo al prossimo riavvio (il
/// tab lo segnala in UI).
#[tauri::command]
pub fn set_aichat_settings(
    settings: AiChatSettings,
    state: State<'_, ConfigDirState>,
) -> Result<(), String> {
    let settings = AiChatSettings {
        label_base: settings.label_base.trim().to_string(),
        // Stesso trattamento di `label_base`: trimma se presente, e converte
        // una stringa vuota-dopo-trim in `None` — così l'utente può
        // "cancellare" il nickname scrivendo solo spazi nel campo, senza
        // dover distinguere a mano fra "cancella" e "lascia invariato".
        display_name: settings
            .display_name
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        ai_display_name: settings
            .ai_display_name
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        ..settings
    };
    validate(&settings)?;

    let path = network_json_path(&state.config_dir);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let existing = read_existing_settings_json(&state.config_dir);
    let merged = merge_aichat_settings(&existing, &settings);
    std::fs::write(&path, merged).map_err(|e| format!("network.json: scrittura fallita ({e})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_preserves_other_fields() {
        let existing =
            r#"{"peer_ttl_secs":30,"enabled":false,"label_base":"lare","chat_port":40100}"#;
        let out = merge_aichat_settings(
            existing,
            &AiChatSettings {
                enabled: true,
                label_base: "skimble".to_string(),
                chat_port: 41000,
                ai_participates: true,
                ai_autoparticipate: false,
                display_name: None,
                ai_display_name: None,
            },
        );
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["enabled"], true);
        assert_eq!(v["label_base"], "skimble");
        assert_eq!(v["chat_port"], 41000);
        assert_eq!(v["peer_ttl_secs"], 30);
    }

    #[test]
    fn merge_from_empty_or_invalid_yields_object_with_fields() {
        for bad in ["", "not json", "[1,2,3]"] {
            let out = merge_aichat_settings(
                bad,
                &AiChatSettings {
                    enabled: false,
                    label_base: "lare".to_string(),
                    chat_port: 40100,
                    ai_participates: true,
                    ai_autoparticipate: false,
                    display_name: None,
                    ai_display_name: None,
                },
            );
            let v: serde_json::Value = serde_json::from_str(&out).unwrap();
            assert!(v.is_object(), "input {bad:?} deve dare un object");
            assert_eq!(v["enabled"], false);
            assert_eq!(v["label_base"], "lare");
            assert_eq!(v["chat_port"], 40100);
        }
    }

    #[test]
    fn validate_rejects_empty_label() {
        let s = AiChatSettings {
            enabled: false,
            label_base: "   ".to_string(),
            chat_port: 40100,
            ai_participates: true,
            ai_autoparticipate: false,
            display_name: None,
            ai_display_name: None,
        };
        let err = validate(&s).expect_err("label vuota (dopo trim) deve essere rifiutata");
        assert!(err.contains("Etichetta"), "messaggio inatteso: {err}");
    }

    #[test]
    fn validate_rejects_bad_charset() {
        for bad_label in ["la re", "a@b", "lare.local"] {
            let s = AiChatSettings {
                enabled: false,
                label_base: bad_label.to_string(),
                chat_port: 40100,
                ai_participates: true,
                ai_autoparticipate: false,
                display_name: None,
                ai_display_name: None,
            };
            assert!(validate(&s).is_err(), "{bad_label:?} deve essere rifiutato");
        }
    }

    #[test]
    fn validate_rejects_privileged_port() {
        let s = AiChatSettings {
            enabled: false,
            label_base: "lare".to_string(),
            chat_port: 80,
            ai_participates: true,
            ai_autoparticipate: false,
            display_name: None,
            ai_display_name: None,
        };
        let err = validate(&s).expect_err("porta privilegiata deve essere rifiutata");
        assert!(err.contains("Porta"), "messaggio inatteso: {err}");
    }

    #[test]
    fn validate_accepts_defaults() {
        let s1 = AiChatSettings {
            enabled: true,
            label_base: "lare".to_string(),
            chat_port: 40100,
            ai_participates: true,
            ai_autoparticipate: false,
            display_name: None,
            ai_display_name: None,
        };
        assert!(validate(&s1).is_ok());
        let s2 = AiChatSettings {
            enabled: false,
            label_base: "lare".to_string(),
            chat_port: 40100,
            ai_participates: false,
            ai_autoparticipate: true,
            display_name: None,
            ai_display_name: None,
        };
        assert!(validate(&s2).is_ok());
    }

    // -------------------------------------------------------------------------
    // AI Chat Slice 1b — flag "la mia AI partecipa" (design §9.4):
    // Docs/superpowers/specs/2026-07-02-aichat-ai-participants-design.md
    // -------------------------------------------------------------------------

    /// `get_aichat_settings` legge `ai_participates` dal JSON quando presente,
    /// e usa il default `true` quando assente — stesso pattern infallibile
    /// già usato per `enabled`/`label_base`/`chat_port` sopra.
    #[test]
    fn get_settings_reads_ai_participates_with_true_default() {
        // Nessun file: infallibile, default true (mirror del comportamento
        // "sempre apribile" già garantito per gli altri 3 campi).
        // (Verificato indirettamente da `merge_round_trip_preserves_ai_participates`
        //  sotto: qui verifichiamo solo che il default via `unwrap_or(true)`
        //  sia coerente con `AiChatConfig::default()` lato orchestrator.)
        let value: serde_json::Value = serde_json::json!({});
        let ai_participates = value
            .get("ai_participates")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true);
        assert!(ai_participates, "default assente deve leggersi come true");
    }

    /// `merge_aichat_settings` scrive `ai_participates` nel JSON — round-trip
    /// completo: merge poi rilettura ritrovano lo stesso valore, e gli altri
    /// campi restano intatti (stesso principio di `merge_preserves_other_fields`).
    #[test]
    fn merge_round_trip_preserves_ai_participates() {
        let existing =
            r#"{"enabled":true,"label_base":"skimble","chat_port":40100,"ai_participates":true}"#;
        let out = merge_aichat_settings(
            existing,
            &AiChatSettings {
                enabled: true,
                label_base: "skimble".to_string(),
                chat_port: 40100,
                ai_participates: false,
                ai_autoparticipate: false,
                display_name: None,
                ai_display_name: None,
            },
        );
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            v["ai_participates"], false,
            "merge deve scrivere il nuovo valore"
        );
        // Gli altri 3 campi restano intatti.
        assert_eq!(v["enabled"], true);
        assert_eq!(v["label_base"], "skimble");
        assert_eq!(v["chat_port"], 40100);
    }

    // -------------------------------------------------------------------------
    // AI Chat Slice 2 — flag "auto-partecipazione" (design §10.1):
    // Docs/superpowers/specs/2026-07-02-aichat-ai-participants-design.md
    // -------------------------------------------------------------------------

    /// `get_aichat_settings` legge `ai_autoparticipate` dal JSON quando presente,
    /// e usa il default `false` quando assente — a DIFFERENZA di `ai_participates`
    /// (default `true`): l'auto-partecipazione è opt-in esplicito.
    #[test]
    fn get_settings_reads_ai_autoparticipate_with_false_default() {
        // Nessun file: infallibile, default false (mirror di
        // `AiChatConfig::default()` lato orchestrator — vedi
        // `default_ai_autoparticipate_is_false` in aichat/config.rs).
        let value: serde_json::Value = serde_json::json!({});
        let ai_autoparticipate = value
            .get("ai_autoparticipate")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        assert!(
            !ai_autoparticipate,
            "default assente deve leggersi come false"
        );
    }

    /// Un `aichat.json` scritto PRIMA che il campo esistesse (nessuna chiave
    /// `ai_autoparticipate`, come i file di Slice 1a/1b) deve leggersi come
    /// `false` — tolleranza alla migrazione, nessun file esistente si rompe.
    #[test]
    fn missing_ai_autoparticipate_field_reads_as_false() {
        let value: serde_json::Value = serde_json::json!({
            "enabled": true,
            "label_base": "skimble",
            "chat_port": 40100,
            "ai_participates": true
        });
        let ai_autoparticipate = value
            .get("ai_autoparticipate")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        assert!(
            !ai_autoparticipate,
            "config senza il campo deve assumere false"
        );
    }

    /// `merge_aichat_settings` scrive `ai_autoparticipate` nel JSON — round-trip
    /// completo, stesso principio di `merge_round_trip_preserves_ai_participates`.
    #[test]
    fn merge_round_trip_preserves_ai_autoparticipate() {
        let existing = r#"{"enabled":true,"label_base":"skimble","chat_port":40100,"ai_participates":true,"ai_autoparticipate":false}"#;
        let out = merge_aichat_settings(
            existing,
            &AiChatSettings {
                enabled: true,
                label_base: "skimble".to_string(),
                chat_port: 40100,
                ai_participates: true,
                ai_autoparticipate: true,
                display_name: None,
                ai_display_name: None,
            },
        );
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            v["ai_autoparticipate"], true,
            "merge deve scrivere il nuovo valore"
        );
        // Gli altri campi restano intatti.
        assert_eq!(v["enabled"], true);
        assert_eq!(v["label_base"], "skimble");
        assert_eq!(v["chat_port"], 40100);
        assert_eq!(v["ai_participates"], true);
    }

    // -------------------------------------------------------------------------
    // Nickname umano + AI (display_name / ai_display_name) — mirror lato `ui` dei
    // campi additivi già aggiunti a `AiChatConfig` lato orchestrator (Task 1-4).
    // Vedi Docs/superpowers/specs/2026-08-13-aichat-display-names-design.md.
    // -------------------------------------------------------------------------

    /// `merge_aichat_settings` scrive entrambi i nickname nel JSON — round-trip
    /// completo, stesso principio di `merge_round_trip_preserves_ai_participates`.
    #[test]
    fn merge_round_trip_preserves_display_names() {
        let existing = r#"{"enabled":true,"label_base":"skimble","chat_port":40100,"ai_participates":true,"ai_autoparticipate":false}"#;
        let out = merge_aichat_settings(
            existing,
            &AiChatSettings {
                enabled: true,
                label_base: "skimble".to_string(),
                chat_port: 40100,
                ai_participates: true,
                ai_autoparticipate: false,
                display_name: Some("Maurizio".to_string()),
                ai_display_name: Some("Aria".to_string()),
            },
        );
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["display_name"], "Maurizio");
        assert_eq!(v["ai_display_name"], "Aria");
    }

    /// `get_aichat_settings` legge i due nickname come `None` quando la chiave è
    /// assente — stesso pattern infallibile degli altri campi (nessun nickname
    /// configurato è uno stato normale, non un errore).
    #[test]
    fn get_settings_reads_display_names_with_none_default() {
        let value: serde_json::Value = serde_json::json!({});
        let display_name = value
            .get("display_name")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
        let ai_display_name = value
            .get("ai_display_name")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
        assert_eq!(display_name, None);
        assert_eq!(ai_display_name, None);
    }

    /// Un nickname oltre 48 caratteri viene rifiutato — limite di lunghezza
    /// analogo a `label_base` (32), ma più permissivo perché il nickname è testo
    /// libero mostrato in UI, non un identificatore tecnico sul wire.
    #[test]
    fn validate_rejects_display_name_too_long() {
        let s = AiChatSettings {
            enabled: false,
            label_base: "lare".to_string(),
            chat_port: 40100,
            ai_participates: true,
            ai_autoparticipate: false,
            display_name: Some("x".repeat(49)),
            ai_display_name: None,
        };
        let err = validate(&s).expect_err("nickname oltre 48 caratteri deve essere rifiutato");
        assert!(err.contains("Nickname"), "messaggio inatteso: {err}");
    }

    /// A differenza di `label_base` (solo `[A-Za-z0-9_-]`), il nickname è testo
    /// libero: spazi e accenti sono validi, perché non finisce mai sul wire come
    /// identificatore tecnico — è solo un'etichetta mostrata in UI.
    #[test]
    fn validate_accepts_display_name_with_spaces_and_accents() {
        let s = AiChatSettings {
            enabled: false,
            label_base: "lare".to_string(),
            chat_port: 40100,
            ai_participates: true,
            ai_autoparticipate: false,
            display_name: Some("Maria José".to_string()),
            ai_display_name: Some("Àlex".to_string()),
        };
        assert!(
            validate(&s).is_ok(),
            "nickname libero (spazi/accenti) deve essere accettato, a differenza di label_base"
        );
    }

    /// `config-dialog.js` (Task 6) manda oggi sempre `display_name`/
    /// `ai_display_name` nel payload di `set_aichat_settings`, ma questo test
    /// resta come rete di sicurezza per QUALUNQUE payload che ne sia privo
    /// (es. un frontend più vecchio non ancora aggiornato, o un payload
    /// costruito a mano) — verifica che l'assenza delle due chiavi NON rompa
    /// la deserializzazione: `serde_derive` tratta i campi `Option<T>` in modo
    /// speciale — una chiave assente diventa `None` automaticamente, GIÀ senza
    /// bisogno di `#[serde(default)]` (che serve invece per gli altri tipi,
    /// es. `bool`/`String`, dove una chiave mancante farebbe fallire con
    /// "missing field"). Un tale payload deserializza con successo — ma con
    /// entrambi i nickname a `None` (vedi il commento su
    /// `merge_aichat_settings` sopra per la conseguenza: un nickname già
    /// presente in `network.json` verrebbe cancellato, non un errore di
    /// deserializzazione).
    #[test]
    fn deserializes_payload_without_display_name_keys() {
        let json = r#"{"enabled":true,"label_base":"lare","chat_port":40100,"ai_participates":true,"ai_autoparticipate":false}"#;
        let s: AiChatSettings = serde_json::from_str(json)
            .expect("un payload senza le chiavi nickname deve comunque deserializzare");
        assert_eq!(s.display_name, None);
        assert_eq!(s.ai_display_name, None);
    }
}
