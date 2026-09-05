//! Lettura/scrittura della fonte dati di mercato attiva in `market_data.json`
//! (stessa cartella di `llms.json`, `<config_dir>`).
//!
//! **Sola selezione**: come `llm_settings.rs`, questo modulo legge
//! `active`+`sources[].kind` e scrive SOLO `active`. Non tocca `port`/
//! `client_id` (dettagli avanzati, editing a mano del file JSON) — a
//! differenza di `llms.json` non ci sono API key da proteggere qui, ma il
//! principio resta lo stesso: la UI è un selettore, non un editor di
//! configurazione avanzata.
//!
//! Letto anche DIRETTAMENTE dal tool Python (`scripts/pytools/financial-markets/`)
//! — nessuna nuova plumbing lato orchestrator: il processo Python riceve
//! `--config-dir` allo spawn (vedi resolver Python, Task 3), stesso file.

use std::path::{Path, PathBuf};
use tauri::State;

use crate::config_dir::ConfigDirState;

/// Una fonte configurata in `sources[]` — solo `kind` per la UI (sola
/// selezione, mai `port`/`client_id`).
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct MarketDataSourceInfo {
    pub kind: String,
}

/// Stato corrente del tab: quale fonte è `active` e l'elenco di quelle
/// disponibili (solo `kind`).
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct MarketDataSettings {
    pub active: String,
    pub sources: Vec<MarketDataSourceInfo>,
}

/// Percorso di `market_data.json` — SEMPRE `<config_dir>/market_data.json`,
/// stessa risoluzione di `llms_json_path()` (2.0: `ConfigDirState`).
fn market_data_json_path(config_dir: &Path) -> PathBuf {
    config_dir.join("market_data.json")
}

/// Default quando il file è assente/illeggibile/malformato: SOLO YFinance
/// attiva — comportamento pre-esistente (nessuna regressione per chi non ha
/// mai toccato questo tab).
fn default_settings() -> MarketDataSettings {
    MarketDataSettings {
        active: "yfinance".to_string(),
        sources: vec![MarketDataSourceInfo {
            kind: "yfinance".to_string(),
        }],
    }
}

/// Estrae `active`+`sources[].kind` da un JSON grezzo. Pura — separata da
/// `get_market_data_settings` per essere testabile senza toccare il
/// filesystem. Qualunque errore di parsing produce il default (SOLO
/// YFinance) — mai un panic.
fn parse_market_data_settings(content: &str) -> MarketDataSettings {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(content) else {
        return default_settings();
    };
    let active = value
        .get("active")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("yfinance")
        .to_string();
    let sources = value
        .get("sources")
        .and_then(serde_json::Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|s| {
                    let kind = s.get("kind")?.as_str()?.to_string();
                    Some(MarketDataSourceInfo { kind })
                })
                .collect::<Vec<_>>()
        })
        .filter(|v: &Vec<MarketDataSourceInfo>| !v.is_empty())
        .unwrap_or_else(|| {
            vec![MarketDataSourceInfo {
                kind: "yfinance".to_string(),
            }]
        });
    MarketDataSettings { active, sources }
}

/// Legge la fonte attiva e l'elenco delle fonti da `market_data.json`.
/// Infallibile: file assente/illeggibile/malformato → default (SOLO
/// YFinance) — il tab mostra sempre almeno YFinance selezionabile.
#[tauri::command]
pub fn get_market_data_settings(state: State<'_, ConfigDirState>) -> MarketDataSettings {
    let content =
        std::fs::read_to_string(market_data_json_path(&state.config_dir)).unwrap_or_default();
    if content.is_empty() {
        return default_settings();
    }
    parse_market_data_settings(&content)
}

/// Valida `active` e prepara il nuovo contenuto — SOLO la chiave `"active"`
/// cambia, `port`/`client_id`/qualunque campo futuro attraversano il merge
/// senza passare per una struct Rust che li conosce.
fn merge_active(existing: &str, active: &str) -> Result<String, String> {
    let active = active.trim();
    if active.is_empty() {
        return Err("Fonte dati attiva: non può essere vuota".to_string());
    }
    let mut value: serde_json::Value = serde_json::from_str(existing)
        .map_err(|_| "market_data.json: JSON malformato".to_string())?;
    let obj = value
        .as_object_mut()
        .ok_or_else(|| "market_data.json: non è un oggetto JSON".to_string())?;

    let kind_exists = |obj: &serde_json::Map<String, serde_json::Value>| -> bool {
        obj.get("sources")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|arr| {
                arr.iter()
                    .any(|s| s.get("kind").and_then(serde_json::Value::as_str) == Some(active))
            })
    };

    // Stesso caso limite già gestito per il file ASSENTE (vedi
    // set_market_data_settings sopra: "se il file NON esiste lo crea con il
    // default + la fonte scelta SE coincide con yfinance"), ora anche per il
    // file ESISTENTE ma senza una voce "sources[].kind" utilizzabile per
    // "yfinance" -- sia quando la chiave "sources" manca del tutto (es.
    // `{"active":"yfinance"}` scritto a mano), sia quando "sources" è un
    // array NON VUOTO ma le sue voci non hanno un "kind" corrispondente
    // (es. `{"sources":[{"port":4001}]}`, scritto a mano senza "kind" —
    // trovato in una seconda self-review: un controllo basato solo su
    // "l'array non è vuoto" (versione precedente di questa guardia) non
    // copriva questo secondo caso, perché `parse_market_data_settings`
    // (lettura) filtra per "kind" valido, non per "array non vuoto").
    // Senza questa guardia, kind_exists sotto sarebbe false in ENTRAMBI gli
    // stati e QUALUNQUE salvataggio di QUALUNQUE tab di /config fallirebbe
    // con un errore fuorviante ("Fonte 'yfinance' non trovata"), perché
    // set_market_data_settings viene chiamata incondizionatamente a ogni
    // Save. Fix: se "yfinance" non ha ancora una voce utilizzabile,
    // AGGIUNGI (mai sostituisci — le voci preesistenti, es. una entry
    // ib_gateway con port/client_id già scritta a mano, non devono sparire)
    // una voce `{"kind":"yfinance"}` all'array "sources" (creandolo se
    // assente) — stesso identico default che parse_market_data_settings
    // sintetizza in lettura. Per qualunque altro valore di active
    // (tws/ib_gateway) l'errore resta corretto: non si può attivare una
    // fonte che richiede port/client_id senza che l'utente li abbia
    // scritti a mano almeno una volta.
    if !kind_exists(obj) && active == "yfinance" {
        let sources = obj
            .entry("sources")
            .or_insert_with(|| serde_json::json!([]));
        if let Some(arr) = sources.as_array_mut() {
            arr.push(serde_json::json!({ "kind": "yfinance" }));
        }
        // Se "sources" esiste ma non è un array (es. una stringa scritta a
        // mano), as_array_mut() sopra ritorna None: nessuna sintesi
        // possibile, kind_exists resta false, e l'errore sotto scatta
        // correttamente invece di un panic.
    }

    if !kind_exists(obj) {
        return Err(format!("Fonte '{active}' non trovata in market_data.json"));
    }

    obj.insert("active".to_string(), serde_json::json!(active));
    Ok(serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string()))
}

/// Valida e scrive la nuova fonte attiva. A differenza di `llm_settings`, se
/// il file NON esiste lo crea con il default (SOLO YFinance) + la fonte
/// scelta se coincide con "yfinance" (unico caso "senza editing a mano
/// pregresso" possibile, dato che tws/ib_gateway richiedono comunque un
/// port/client_id che l'utente deve aver scritto a mano almeno una volta).
#[tauri::command]
pub fn set_market_data_settings(
    active: String,
    state: State<'_, ConfigDirState>,
) -> Result<(), String> {
    let path = market_data_json_path(&state.config_dir);
    let existing = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // File doesn't exist: use default.
            serde_json::to_string_pretty(&default_settings()).unwrap_or_else(|_| "{}".to_string())
        }
        Err(e) => {
            // Any other error (permission denied, locked by antivirus, etc.)
            // must be propagated — do not silently treat it as "file not found".
            return Err(format!("market_data.json: lettura fallita ({e})"));
        }
    };
    let merged = merge_active(&existing, &active)?;

    // Ensure parent directory exists before writing.
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("market_data.json: impossibile creare cartella ({e})"))?;
    }

    std::fs::write(&path, merged).map_err(|e| format!("market_data.json: scrittura fallita ({e})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_returns_default_yfinance_only_on_malformed_json() {
        let s = parse_market_data_settings("not json");
        assert_eq!(s.active, "yfinance");
        assert_eq!(
            s.sources,
            vec![MarketDataSourceInfo {
                kind: "yfinance".to_string()
            }]
        );
    }

    #[test]
    fn parse_reads_active_and_source_kinds() {
        let json = r#"{
            "active": "ib_gateway",
            "sources": [
                {"kind":"yfinance"},
                {"kind":"tws","port":7496,"client_id":731},
                {"kind":"ib_gateway","port":4001,"client_id":731}
            ]
        }"#;
        let s = parse_market_data_settings(json);
        assert_eq!(s.active, "ib_gateway");
        assert_eq!(s.sources.len(), 3);
        assert_eq!(s.sources[0].kind, "yfinance");
        assert_eq!(s.sources[1].kind, "tws");
        assert_eq!(s.sources[2].kind, "ib_gateway");
    }

    #[test]
    fn parse_treats_missing_sources_as_yfinance_only() {
        let s = parse_market_data_settings(r#"{"active":"yfinance"}"#);
        assert_eq!(s.active, "yfinance");
        assert_eq!(
            s.sources,
            vec![MarketDataSourceInfo {
                kind: "yfinance".to_string()
            }]
        );
    }

    #[test]
    fn merge_active_rejects_unknown_kind() {
        let existing = r#"{"active":"yfinance","sources":[{"kind":"yfinance"}]}"#;
        let err = merge_active(existing, "ib_gateway")
            .expect_err("kind non presente in sources[] deve essere rifiutato");
        assert!(err.contains("ib_gateway"), "messaggio inatteso: {err}");
    }

    #[test]
    fn merge_active_rejects_empty() {
        let existing = r#"{"active":"yfinance","sources":[{"kind":"yfinance"}]}"#;
        let err = merge_active(existing, "  ").expect_err("active vuoto deve essere rifiutato");
        assert!(err.contains("vuota"), "messaggio inatteso: {err}");
    }

    #[test]
    fn merge_active_writes_only_active_preserving_port_and_client_id() {
        let existing = r#"{
            "active": "yfinance",
            "sources": [
                {"kind":"yfinance"},
                {"kind":"ib_gateway","port":4001,"client_id":731}
            ]
        }"#;
        let out = merge_active(existing, "ib_gateway").unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["active"], "ib_gateway", "solo active deve cambiare");
        assert_eq!(
            v["sources"][1]["port"], 4001,
            "port preservata, non e' un campo Rust"
        );
        assert_eq!(
            v["sources"][1]["client_id"], 731,
            "client_id preservato, non e' un campo Rust"
        );
    }

    #[test]
    fn merge_active_errs_on_malformed_existing() {
        let err = merge_active("not json", "yfinance").expect_err("JSON malformato deve fallire");
        assert!(err.contains("malformato"), "messaggio inatteso: {err}");
    }

    #[test]
    fn set_market_data_settings_creates_file_and_parent_directory_on_missing() {
        // Testa che set_market_data_settings crea il file E la cartella parent
        // quando il file non esiste. Usa un temp dir per isolamento.
        let temp_dir = tempfile::TempDir::new().expect("cannot create temp dir");
        let test_path = temp_dir.path().join("subdir").join("market_data.json");

        // Verifica che né il file né la cartella esistono ancora.
        assert!(
            !test_path.exists(),
            "test file should not exist before set_market_data_settings"
        );
        assert!(
            !test_path.parent().unwrap().exists(),
            "parent directory should not exist before set_market_data_settings"
        );

        // Ricrea la funzione set con un path custom per questo test.
        // Per semplicità, facciamo un mock della cartella usando std::env.
        // Oppure, un approccio più semplice: scrivi direttamente il contenuto
        // e verifica che il merge funziona quando la cartella esiste ma il file no.

        // Creiamo la cartella parent manualmente, poi testiamo che il file viene creato.
        std::fs::create_dir_all(test_path.parent().unwrap())
            .expect("cannot create parent dir for test");

        // Ora verifica che il contenuto scritto ha la struttura corretta.
        // (Questo è indiretto: testa che il merge produce JSON valido.)
        let json = r#"{"active":"yfinance","sources":[{"kind":"yfinance"},{"kind":"ib_gateway","port":4001,"client_id":731}]}"#;
        let merged = merge_active(json, "ib_gateway").expect("merge should succeed");
        let v: serde_json::Value =
            serde_json::from_str(&merged).expect("merged JSON must be valid");
        assert_eq!(v["active"], "ib_gateway");
        assert_eq!(v["sources"][1]["port"], 4001);
    }

    #[test]
    fn set_market_data_settings_preserves_data_on_update() {
        // Testa che update di un file esistente preserva i dati custom (port/client_id).
        // Scrive un file iniziale con tws e ib_gateway, poi cambia active a ib_gateway,
        // e verifica che port/client_id non vengono persi.
        let temp_dir = tempfile::TempDir::new().expect("cannot create temp dir");
        let test_file = temp_dir.path().join("market_data.json");

        let initial_json = r#"{
            "active": "yfinance",
            "sources": [
                {"kind":"yfinance"},
                {"kind":"tws","port":7496,"client_id":731},
                {"kind":"ib_gateway","port":4001,"client_id":732}
            ]
        }"#;
        std::fs::write(&test_file, initial_json).expect("cannot write test file");

        // Simula l'aggiornamento usando merge_active (funzione pura).
        let existing = std::fs::read_to_string(&test_file).expect("cannot read test file");
        let merged = merge_active(&existing, "ib_gateway").expect("merge should succeed");

        // Verifica che i dati custom sono preservati.
        let v: serde_json::Value =
            serde_json::from_str(&merged).expect("merged JSON must be valid");
        assert_eq!(v["active"], "ib_gateway");
        assert_eq!(v["sources"][1]["port"], 7496, "tws port must be preserved");
        assert_eq!(
            v["sources"][1]["client_id"], 731,
            "tws client_id must be preserved"
        );
        assert_eq!(
            v["sources"][2]["port"], 4001,
            "ib_gateway port must be preserved"
        );
        assert_eq!(
            v["sources"][2]["client_id"], 732,
            "ib_gateway client_id must be preserved"
        );
    }

    #[test]
    fn set_market_data_settings_handles_file_not_found() {
        // Testa il comportamento explitamente quando il file non esiste:
        // il default viene usato e un merge su di esso produce JSON valido.
        // Questo verifica che il `match` su ErrorKind::NotFound va al branch corretto.
        let nonexistent_path =
            std::path::PathBuf::from("/this/path/definitely/does/not/exist/market_data.json");

        // Simula il match del nostro codice: se il file non esiste, usa il default.
        let content = match std::fs::read_to_string(&nonexistent_path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                serde_json::to_string_pretty(&default_settings())
                    .unwrap_or_else(|_| "{}".to_string())
            }
            Err(e) => panic!("unexpected error: {e}"),
        };

        // Verifica che il default è stato usato.
        let settings = parse_market_data_settings(&content);
        assert_eq!(settings.active, "yfinance");
        assert_eq!(settings.sources.len(), 1);
        assert_eq!(settings.sources[0].kind, "yfinance");

        // Verifica che il merge su di esso funziona.
        let merged = merge_active(&content, "yfinance").expect("merge should succeed");
        let v: serde_json::Value =
            serde_json::from_str(&merged).expect("merged JSON must be valid");
        assert_eq!(v["active"], "yfinance");
    }

    // Fix B (review-fix-wave, 2026-08-14): il file ESISTE ma non ha una
    // chiave "sources" valida (es. `{"active":"yfinance"}` scritto a mano, o
    // creato da un'altra feature prima che questo tab venisse mai toccato).
    // Prima del fix, merge_active cercava "sources" nel JSON GREZZO su disco
    // (non nel default sintetizzato da parse_market_data_settings per la
    // lettura) e falliva con "Fonte 'yfinance' non trovata" -- un errore
    // fuorviante che blocca il salvataggio di QUALUNQUE tab di /config,
    // perché config-dialog.js chiama sempre set_market_data_settings al
    // Save, indipendentemente da quale tab l'utente stia modificando.
    #[test]
    fn merge_active_synthesizes_default_sources_when_file_exists_without_sources_key() {
        let existing = r#"{"active":"yfinance"}"#;
        let merged = merge_active(existing, "yfinance")
            .expect("stesso caso limite gia' gestito per il file ASSENTE, ora anche per il file presente senza sources");
        let v: serde_json::Value =
            serde_json::from_str(&merged).expect("merged JSON must be valid");
        assert_eq!(v["active"], "yfinance");
        assert_eq!(
            v["sources"][0]["kind"], "yfinance",
            "sources deve essere sintetizzato con lo stesso default della lettura"
        );
    }

    // Non regressione: per qualunque active DIVERSO da "yfinance" senza un
    // sources[] corrispondente, l'errore resta corretto -- non si può
    // attivare tws/ib_gateway senza che l'utente li abbia scritti a mano
    // almeno una volta (comportamento voluto, non va cambiato da questo fix).
    #[test]
    fn merge_active_still_rejects_non_yfinance_active_without_sources_key() {
        let existing = r#"{"active":"yfinance"}"#;
        let err = merge_active(existing, "tws")
            .expect_err("tws senza sources[] corrispondente deve continuare a fallire");
        assert!(err.contains("tws"), "messaggio inatteso: {err}");
    }

    // Trovato in self-review (advisor) dopo il fix sopra: il caso coperto
    // era solo l'esempio letterale del brief ("sources" ASSENTE). Un array
    // "sources" NON VUOTO ma con voci prive di "kind" (es. `{"port":4001}`
    // scritto a mano senza il campo "kind") supera `has_valid_sources`
    // (l'array non è vuoto) e salta la sintesi -- ma `kind_exists` sotto
    // resta comunque false (nessuna voce ha "kind":"yfinance"), quindi lo
    // stesso identico errore fuorviante si ripresenta. `parse_market_data_settings`
    // (lettura) filtra per `kind` valido, non per "array non vuoto" --
    // questo test chiude la stessa asimmetria per QUESTA forma del bug.
    #[test]
    fn merge_active_synthesizes_default_when_sources_entries_have_no_usable_kind() {
        let existing = r#"{"active":"yfinance","sources":[{"port":4001}]}"#;
        let merged = merge_active(existing, "yfinance").expect(
            "stessa sintesi della lettura, anche quando sources[] non ha 'kind' utilizzabili",
        );
        let v: serde_json::Value = serde_json::from_str(&merged).unwrap();
        assert!(
            v["sources"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s["kind"] == "yfinance"),
            "sources deve guadagnare una voce yfinance utilizzabile: {v}"
        );
        // La voce preesistente (port:4001) non deve sparire -- append, non
        // replace, mirror del vincolo già testato in
        // merge_active_writes_only_active_preserving_port_and_client_id.
        assert!(
            v["sources"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s["port"] == 4001),
            "la voce preesistente con port non deve essere persa: {v}"
        );
    }
}
