//! # routines — repository di script PowerShell riusabili
//!
//! Modulo puro: nessuna dipendenza da MCP/rmcp. Possiede l'indice JSON su
//! disco (`index.json`), la ricerca al suo interno, e la costruzione della
//! riga di invocazione per `Session::run`. `main.rs` resta thin glue: chiama
//! queste funzioni e basta, esattamente come fa oggi con `session.rs`.
//!
//! Root di default (2.0, decisione D6 dello spec): `Configuration/routines`,
//! percorso relativo dentro `startup.json` (`paths.routines_dir`), risolto
//! rispetto alla RADICE DEL DEPLOY tramite `startup_config::StartupConfig::
//! resolve_path` (vedi `crates/startup-config`). Nessuna variabile
//! d'ambiente: nella v1 `LARE_ROUTINES_DIR`/`LARE_LOCAL_DIR` erano lette qui
//! e in altri punti, e potevano divergere in silenzio — nella 2.0 l'unica
//! fonte è `--config-dir`/`<exe_dir>\Configuration\` + `startup.json`.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RoutineEntry {
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub category: String,
    /// Path relativo alla root `routines/` (es. `"list-big-files.ps1"`).
    pub path: String,
    /// Data di creazione, formato `YYYY-MM-DD`.
    pub created: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct RoutineIndex {
    pub scripts: Vec<RoutineEntry>,
}

/// Cartella delle routine: `paths.routines_dir` di startup.json, relativa alla
/// radice del deploy (default `Configuration/routines`). Nessuna env var.
pub fn resolve_root(config_dir: &Path, cfg: &startup_config::StartupConfig) -> PathBuf {
    startup_config::StartupConfig::resolve_path(config_dir, &cfg.paths.routines_dir)
}

/// Carica `index.json` dalla root indicata.
///
/// File assente O JSON malformato → indice vuoto, MAI un errore: è il caso
/// normale al primo avvio (nessuna routine ancora salvata), e una routine
/// non deve mai bloccare l'AI per un indice corrotto — al peggio la ricerca
/// non trova nulla.
pub fn load_index(root: &Path) -> RoutineIndex {
    let index_path = root.join("index.json");
    match std::fs::read_to_string(&index_path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => RoutineIndex::default(),
    }
}

/// Errori di `save_entry`. `Io` porta il messaggio già formattato (non
/// `std::io::Error` diretto): `SaveError` deve restare `PartialEq`-comparabile
/// nei test, e `std::io::Error` non lo è.
#[derive(Debug, PartialEq)]
pub enum SaveError {
    /// Il nome finale (nuovo o dopo un `replace`) è già usato da un'altra
    /// routine nell'indice.
    NameCollision,
    /// Il nome non rispetta il charset sicuro (vedi `validate_name`) — porta
    /// il nome rifiutato per il messaggio d'errore.
    InvalidName(String),
    /// `replace: Some(old_name)` ma `old_name` non esiste nell'indice.
    ReplaceTargetNotFound(String),
    /// Scrittura file o indice fallita (permessi, disco pieno, ecc.).
    Io(String),
}

/// Valida `name` contro un charset sicuro: solo lettere minuscole ASCII,
/// cifre, trattino. `name` diventa parte di un path filesystem reale
/// (`<name>.ps1`, vedi `save_entry`) e finisce dentro `build_invocation` —
/// questa validazione non è forma, è la difesa strutturale contro
/// path-traversal/injection: niente `/`, `\`, `..` (il `.` non è nel charset
/// consentito, quindi ".." è escluso a monte, non serve un caso ad-hoc), niente
/// spazi (romperebbero l'invocazione PowerShell non quotata).
pub fn validate_name(name: &str) -> Result<(), SaveError> {
    let ok = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if ok {
        Ok(())
    } else {
        Err(SaveError::InvalidName(name.to_string()))
    }
}

/// Converte secondi Unix in `YYYY-MM-DD` (UTC). Nessuna dipendenza `chrono`
/// (assente dal workspace, coerente con la linea di progetto contro
/// dipendenze aggiuntive quando evitabili): algoritmo civile di Howard
/// Hinnant (`civil_from_days`, dominio pubblico, correttezza nota inclusi
/// anni bisestili) su `secs / 86400` giorni dall'epoca.
pub fn format_date_from_unix_secs(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

pub fn find_by_name<'a>(idx: &'a RoutineIndex, name: &str) -> Option<&'a RoutineEntry> {
    idx.scripts.iter().find(|e| e.name == name)
}

/// Legge nome+metadati+corpo di una routine esistente, per confronto lato AI
/// prima di un `save_routine` (design doc §3: "search-first" esteso alla
/// scrittura). `None` se il nome non è nell'indice, o se il file su disco è
/// assente/illeggibile (indice disallineato dal filesystem — stesso
/// trattamento "mai un errore" di `load_index`).
pub fn get_content(root: &Path, idx: &RoutineIndex, name: &str) -> Option<(RoutineEntry, String)> {
    let entry = find_by_name(idx, name)?.clone();
    let content = std::fs::read_to_string(root.join(&entry.path)).ok()?;
    Some((entry, content))
}

/// Cerca per nome/descrizione/tag, case-insensitive. `query` assente o vuota
/// (dopo trim) → tutte le entry (list+search unificati, vedi design doc §4).
///
/// La query è tokenizzata per spazi: un'entry combacia se ALMENO UNA parola
/// (non tutte, OR non AND) è sottostringa di nome, descrizione o un tag —
/// prima era match sull'intera query come UNA sottostringa contigua, che
/// falliva su query multi-parola naturali dell'AI (es. "file grandi
/// dimensione" non è sottostringa di "Elenca i file più grandi di N MB..."
/// anche se il tema è lo stesso): trovato in un test dal vivo 2026-08-05,
/// l'AI cercava la routine, non la trovava, e riscriveva il comando da zero
/// — il fix del system prompt (verifica search_routines prima) da solo non
/// bastava se poi la ricerca stessa non trova nulla.
pub fn search<'a>(idx: &'a RoutineIndex, query: Option<&str>) -> Vec<&'a RoutineEntry> {
    let q = match query {
        Some(q) if !q.trim().is_empty() => q.to_lowercase(),
        _ => return idx.scripts.iter().collect(),
    };
    let tokens: Vec<&str> = q.split_whitespace().collect();
    idx.scripts
        .iter()
        .filter(|e| {
            let name = e.name.to_lowercase();
            let description = e.description.to_lowercase();
            tokens.iter().any(|t| {
                name.contains(t)
                    || description.contains(t)
                    || e.tags.iter().any(|tag| tag.to_lowercase().contains(t))
            })
        })
        .collect()
}

/// Salva una routine nuova, o aggiorna/rinomina una esistente (`replace`).
///
/// `replace: None` → `name` deve essere libero nell'indice, altrimenti
/// [`SaveError::NameCollision`]. `replace: Some(old_name)` → `old_name` deve
/// esistere ([`SaveError::ReplaceTargetNotFound`] altrimenti); il `name`
/// finale (che può coincidere con `old_name` — update in place — o essere
/// diverso — rename) deve essere libero CONTRO L'INDICE MENO `old_name`
/// (permette il rename verso un nome libero senza scontrarsi con se stesso).
///
/// Il path su disco (`<name>.ps1`) è derivato QUI dal `name` già validato —
/// il chiamante non lo passa mai: nessun modo di aggirare `validate_name`
/// fornendo un path che non corrisponde al nome (vedi doc-comment di modulo
/// e design doc §4, "path-traversal/injection").
///
/// Scrittura file PRIMA di toccare l'indice: se fallisce, l'indice resta
/// intatto (nessuno stato a metà). Il vecchio file viene rimosso SOLO dopo
/// che il nuovo è scritto con successo, e solo se il path è cambiato
/// (rename) — un update in-place sovrascrive lo stesso file, rimuoverlo
/// dopo cancellerebbe il contenuto appena scritto.
#[allow(clippy::too_many_arguments)]
pub fn save_entry(
    root: &Path,
    name: &str,
    description: &str,
    tags: Vec<String>,
    category: &str,
    content: &str,
    created: &str,
    replace: Option<&str>,
) -> Result<(), SaveError> {
    validate_name(name)?;

    let mut idx = load_index(root);

    // Se `replace` è impostato, rimuovi la vecchia entry PRIMA del controllo
    // di collisione: permette in-place update (stesso nome) e rename verso
    // un nome libero, senza che la entry vecchia colluda con se stessa.
    let mut removed_old_path: Option<String> = None;
    if let Some(old_name) = replace {
        let pos = idx
            .scripts
            .iter()
            .position(|e| e.name == old_name)
            .ok_or_else(|| SaveError::ReplaceTargetNotFound(old_name.to_string()))?;
        removed_old_path = Some(idx.scripts.remove(pos).path);
    }

    if find_by_name(&idx, name).is_some() {
        return Err(SaveError::NameCollision);
    }

    std::fs::create_dir_all(root).map_err(|e| SaveError::Io(e.to_string()))?;

    let path = format!("{name}.ps1");
    std::fs::write(root.join(&path), content).map_err(|e| SaveError::Io(e.to_string()))?;

    if let Some(old_path) = removed_old_path {
        if old_path != path {
            // Best-effort: il nuovo file è già scritto con successo sopra,
            // un fallimento qui (permessi, file già assente) non deve far
            // fallire l'intero salvataggio.
            let _ = std::fs::remove_file(root.join(&old_path));
        }
    }

    idx.scripts.push(RoutineEntry {
        name: name.to_string(),
        description: description.to_string(),
        tags,
        category: category.to_string(),
        path,
        created: created.to_string(),
    });

    let json = serde_json::to_string_pretty(&idx).map_err(|e| SaveError::Io(e.to_string()))?;
    std::fs::write(root.join("index.json"), json).map_err(|e| SaveError::Io(e.to_string()))?;

    Ok(())
}

/// Costruisce la riga di invocazione PowerShell per una routine, pronta per
/// `Session::run`. Nessun path assoluto di default iniettato oltre allo
/// script stesso: è la routine a dover default-are ai propri parametri
/// "cartella di partenza" sulla cwd corrente (es. `-Path = "."`), non questo
/// tool — l'esecuzione parte sempre dalla cwd della shell persistente
/// (regola esplicita utente, vedi design doc §4).
///
/// `args`, se presente e non vuota (dopo trim), è appesa VERBATIM: la shell
/// stessa tokenizza correttamente virgolette/escape. Non va mai ri-splittata
/// a mano qui (bug del prototipo — vedi `build_invocation_passes_args_verbatim_not_resplit`).
pub fn build_invocation(entry: &RoutineEntry, root: &Path, args: Option<&str>) -> String {
    let script_path = root.join(&entry.path);
    // PowerShell escape per apice singolo dentro una stringa single-quoted: si
    // raddoppia (`''`). Senza questo, un profilo utente con un apice nel path
    // (es. `C:\Users\O'Brien\...`) rompe l'invocazione; più seriamente, quando
    // `entry.path` potrà provenire da un index.json scritto dall'AI (futuro
    // `save_routine`), un path non escaped sarebbe un vettore di command
    // injection oltre il banner di conferma (che mostra il NOME della routine,
    // non il path risolto).
    let script_path_str = script_path.to_string_lossy().replace('\'', "''");
    match args {
        Some(a) if !a.trim().is_empty() => format!("& '{script_path_str}' {a}"),
        _ => format!("& '{script_path_str}'"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── resolve_root ─────────────────────────────────────────────────────

    #[test]
    fn resolve_root_uses_startup_routines_dir_relative_to_deploy_root() {
        let cfg = startup_config::StartupConfig::default(); // routines_dir = "Configuration/routines"
        let root = resolve_root(std::path::Path::new("C:/Lare/Configuration"), &cfg);
        assert_eq!(
            root,
            std::path::Path::new("C:/Lare").join("Configuration/routines")
        );
    }
    #[test]
    fn resolve_root_absolute_routines_dir_is_kept() {
        let mut cfg = startup_config::StartupConfig::default();
        cfg.paths.routines_dir = "D:/routines".into();
        let root = resolve_root(std::path::Path::new("C:/Lare/Configuration"), &cfg);
        assert_eq!(root, std::path::PathBuf::from("D:/routines"));
    }

    // ── load_index ───────────────────────────────────────────────────────

    #[test]
    fn load_index_missing_file_returns_empty_not_error() {
        let dir = tempfile::tempdir().unwrap();
        let idx = load_index(dir.path());
        assert_eq!(idx, RoutineIndex::default());
    }

    #[test]
    fn load_index_reads_valid_json() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("index.json"),
            r#"{"scripts":[{"name":"list-big-files","description":"Elenca i file più grandi di N MB","tags":["files","disk"],"category":"files","path":"list-big-files.ps1","created":"2026-08-03"}]}"#,
        )
        .unwrap();
        let idx = load_index(dir.path());
        assert_eq!(idx.scripts.len(), 1);
        assert_eq!(idx.scripts[0].name, "list-big-files");
    }

    #[test]
    fn load_index_malformed_json_returns_empty_not_panic() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("index.json"), "not json at all").unwrap();
        let idx = load_index(dir.path());
        assert_eq!(idx, RoutineIndex::default());
    }

    // ── find_by_name ─────────────────────────────────────────────────────

    fn sample_index() -> RoutineIndex {
        RoutineIndex {
            scripts: vec![
                RoutineEntry {
                    name: "list-big-files".into(),
                    description: "Elenca i file più grandi di N MB in una cartella".into(),
                    tags: vec!["files".into(), "disk".into(), "size".into()],
                    category: "files".into(),
                    path: "list-big-files.ps1".into(),
                    created: "2026-08-03".into(),
                },
                RoutineEntry {
                    name: "ping-hosts".into(),
                    description: "Ping su una lista di host".into(),
                    tags: vec!["network".into()],
                    category: "network".into(),
                    path: "ping-hosts.ps1".into(),
                    created: "2026-08-03".into(),
                },
            ],
        }
    }

    #[test]
    fn find_by_name_found() {
        let idx = sample_index();
        let found = find_by_name(&idx, "ping-hosts").unwrap();
        assert_eq!(found.category, "network");
    }

    #[test]
    fn find_by_name_not_found() {
        let idx = sample_index();
        assert!(find_by_name(&idx, "does-not-exist").is_none());
    }

    // ── search ───────────────────────────────────────────────────────────

    #[test]
    fn search_none_query_returns_all() {
        let idx = sample_index();
        assert_eq!(search(&idx, None).len(), 2);
    }

    #[test]
    fn search_empty_query_returns_all() {
        let idx = sample_index();
        assert_eq!(search(&idx, Some("")).len(), 2);
    }

    #[test]
    fn search_matches_name_case_insensitive() {
        let idx = sample_index();
        let results = search(&idx, Some("PING"));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "ping-hosts");
    }

    #[test]
    fn search_matches_description() {
        let idx = sample_index();
        let results = search(&idx, Some("più grandi"));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "list-big-files");
    }

    #[test]
    fn search_matches_tag() {
        let idx = sample_index();
        let results = search(&idx, Some("disk"));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "list-big-files");
    }

    #[test]
    fn search_no_match_returns_empty() {
        let idx = sample_index();
        assert!(search(&idx, Some("nonexistent-xyz")).is_empty());
    }

    #[test]
    fn search_multi_word_query_matches_via_any_token() {
        let idx = sample_index();
        // "dimensione" non appare da nessuna parte (né in "size" né in "N MB"),
        // ma "file"/"grandi" sì — con OR-per-token deve comunque trovarla.
        let results = search(&idx, Some("file grandi dimensione"));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "list-big-files");
    }

    #[test]
    fn search_multi_word_query_still_empty_when_no_token_matches() {
        let idx = sample_index();
        assert!(search(&idx, Some("stampante ufficio remoto")).is_empty());
    }

    // ── build_invocation ─────────────────────────────────────────────────

    #[test]
    fn build_invocation_escapes_single_quote_in_path() {
        let entry = RoutineEntry {
            name: "test".into(),
            description: "d".into(),
            tags: vec![],
            category: "custom".into(),
            path: "o'brien.ps1".into(),
            created: "2026-08-03".into(),
        };
        let root = PathBuf::from(r"C:\Users\O'Brien\routines");
        let cmd = build_invocation(&entry, &root, None);
        assert_eq!(cmd, r"& 'C:\Users\O''Brien\routines\o''brien.ps1'");
    }

    #[test]
    fn build_invocation_no_args() {
        let entry = &sample_index().scripts[0];
        let root = PathBuf::from(r"C:\fake\routines");
        let cmd = build_invocation(entry, &root, None);
        assert_eq!(cmd, r"& 'C:\fake\routines\list-big-files.ps1'");
    }

    #[test]
    fn build_invocation_empty_args_treated_as_none() {
        let entry = &sample_index().scripts[0];
        let root = PathBuf::from(r"C:\fake\routines");
        let cmd = build_invocation(entry, &root, Some("   "));
        assert_eq!(cmd, r"& 'C:\fake\routines\list-big-files.ps1'");
    }

    #[test]
    fn build_invocation_passes_args_verbatim_not_resplit() {
        // Bug del prototipo da non riprodurre: `.Split(" ")` rompe sui path
        // fra virgolette. La stringa args va passata intatta: è la shell,
        // non Rust, a tokenizzare correttamente virgolette/escape.
        let entry = &sample_index().scripts[0];
        let root = PathBuf::from(r"C:\fake\routines");
        let cmd = build_invocation(
            entry,
            &root,
            Some(r#"-Path "C:\Program Files" -MinSizeMB 50"#),
        );
        assert_eq!(
            cmd,
            r#"& 'C:\fake\routines\list-big-files.ps1' -Path "C:\Program Files" -MinSizeMB 50"#
        );
    }

    // ── validate_name ────────────────────────────────────────────────────────

    #[test]
    fn validate_name_accepts_lowercase_digits_dashes() {
        assert!(validate_name("list-big-files-v2").is_ok());
    }

    #[test]
    fn validate_name_rejects_empty() {
        assert!(matches!(validate_name(""), Err(SaveError::InvalidName(_))));
    }

    #[test]
    fn validate_name_rejects_uppercase() {
        assert!(matches!(
            validate_name("List-Big-Files"),
            Err(SaveError::InvalidName(_))
        ));
    }

    #[test]
    fn validate_name_rejects_slash_and_backslash() {
        assert!(matches!(
            validate_name("a/b"),
            Err(SaveError::InvalidName(_))
        ));
        assert!(matches!(
            validate_name("a\\b"),
            Err(SaveError::InvalidName(_))
        ));
    }

    #[test]
    fn validate_name_rejects_dot_dot_and_spaces() {
        // Il charset non contiene '.' o ' ': ".." e i nomi con spazi sono
        // strutturalmente esclusi, non serve un caso ad-hoc per path traversal.
        assert!(matches!(
            validate_name(".."),
            Err(SaveError::InvalidName(_))
        ));
        assert!(matches!(
            validate_name("a b"),
            Err(SaveError::InvalidName(_))
        ));
    }

    // ── format_date_from_unix_secs ──────────────────────────────────────

    #[test]
    fn format_date_epoch_is_1970_01_01() {
        assert_eq!(format_date_from_unix_secs(0), "1970-01-01");
    }

    #[test]
    fn format_date_known_millennium_reference() {
        // 946684800 = 2000-01-01T00:00:00Z, un timestamp di riferimento notissimo.
        assert_eq!(format_date_from_unix_secs(946_684_800), "2000-01-01");
        // Un secondo prima: ancora l'ultimo giorno del millennio precedente.
        assert_eq!(format_date_from_unix_secs(946_684_799), "1999-12-31");
    }

    // ── save_entry ───────────────────────────────────────────────────────

    #[test]
    fn save_entry_new_routine_writes_file_and_index() {
        let dir = tempfile::tempdir().unwrap();
        let result = save_entry(
            dir.path(),
            "new-routine",
            "Fa qualcosa",
            vec!["tag1".to_string()],
            "custom",
            "Write-Host 'ciao'",
            "2026-08-05",
            None,
        );
        assert!(result.is_ok(), "{result:?}");

        let script = std::fs::read_to_string(dir.path().join("new-routine.ps1")).unwrap();
        assert_eq!(script, "Write-Host 'ciao'");

        let idx = load_index(dir.path());
        assert_eq!(idx.scripts.len(), 1);
        assert_eq!(idx.scripts[0].name, "new-routine");
        assert_eq!(idx.scripts[0].path, "new-routine.ps1");
        assert_eq!(idx.scripts[0].created, "2026-08-05");
    }

    #[test]
    fn save_entry_creates_root_dir_if_missing() {
        // Root non ancora creata (primo save assoluto) — deve funzionare, non
        // fallire con "directory non trovata".
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("routines");
        assert!(!root.exists());
        let result = save_entry(&root, "n", "d", vec![], "c", "content", "2026-08-05", None);
        assert!(result.is_ok(), "{result:?}");
        assert!(root.join("n.ps1").exists());
    }

    #[test]
    fn save_entry_rejects_invalid_name_before_touching_disk() {
        let dir = tempfile::tempdir().unwrap();
        let result = save_entry(
            dir.path(),
            "Not Valid!",
            "d",
            vec![],
            "c",
            "content",
            "2026-08-05",
            None,
        );
        assert!(matches!(result, Err(SaveError::InvalidName(_))));
        assert!(
            load_index(dir.path()).scripts.is_empty(),
            "nessuna entry deve essere stata scritta"
        );
    }

    #[test]
    fn save_entry_collision_without_replace_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        save_entry(
            dir.path(),
            "existing",
            "d",
            vec![],
            "c",
            "content",
            "2026-08-05",
            None,
        )
        .unwrap();
        let result = save_entry(
            dir.path(),
            "existing",
            "d2",
            vec![],
            "c",
            "content2",
            "2026-08-05",
            None,
        );
        assert_eq!(result, Err(SaveError::NameCollision));
        // La entry originale non deve essere stata toccata.
        let idx = load_index(dir.path());
        assert_eq!(idx.scripts.len(), 1);
        assert_eq!(idx.scripts[0].description, "d");
    }

    #[test]
    fn save_entry_replace_in_place_same_name_overwrites_content() {
        let dir = tempfile::tempdir().unwrap();
        save_entry(
            dir.path(),
            "r",
            "old desc",
            vec![],
            "c",
            "old content",
            "2026-08-01",
            None,
        )
        .unwrap();

        let result = save_entry(
            dir.path(),
            "r",
            "new desc",
            vec!["t".to_string()],
            "c2",
            "new content",
            "2026-08-05",
            Some("r"),
        );
        assert!(result.is_ok(), "{result:?}");

        let idx = load_index(dir.path());
        assert_eq!(
            idx.scripts.len(),
            1,
            "nessuna entry duplicata dopo l'update in-place"
        );
        assert_eq!(idx.scripts[0].description, "new desc");
        assert_eq!(idx.scripts[0].created, "2026-08-05");
        let script = std::fs::read_to_string(dir.path().join("r.ps1")).unwrap();
        assert_eq!(script, "new content");
    }

    #[test]
    fn save_entry_replace_with_rename_removes_old_file_and_entry() {
        let dir = tempfile::tempdir().unwrap();
        save_entry(
            dir.path(),
            "old-name",
            "d",
            vec![],
            "c",
            "content",
            "2026-08-01",
            None,
        )
        .unwrap();

        let result = save_entry(
            dir.path(),
            "new-name",
            "d2",
            vec![],
            "c",
            "content2",
            "2026-08-05",
            Some("old-name"),
        );
        assert!(result.is_ok(), "{result:?}");

        assert!(
            !dir.path().join("old-name.ps1").exists(),
            "il vecchio file deve essere rimosso"
        );
        assert!(dir.path().join("new-name.ps1").exists());

        let idx = load_index(dir.path());
        assert_eq!(idx.scripts.len(), 1);
        assert_eq!(idx.scripts[0].name, "new-name");
        assert!(idx.scripts.iter().all(|e| e.name != "old-name"));
    }

    #[test]
    fn save_entry_replace_target_not_found_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let result = save_entry(
            dir.path(),
            "new-name",
            "d",
            vec![],
            "c",
            "content",
            "2026-08-05",
            Some("does-not-exist"),
        );
        assert_eq!(
            result,
            Err(SaveError::ReplaceTargetNotFound(
                "does-not-exist".to_string()
            ))
        );
        assert!(load_index(dir.path()).scripts.is_empty());
    }

    #[test]
    fn save_entry_replace_rename_colliding_with_third_entry_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        save_entry(dir.path(), "a", "d", vec![], "c", "ca", "2026-08-01", None).unwrap();
        save_entry(dir.path(), "b", "d", vec![], "c", "cb", "2026-08-01", None).unwrap();

        // Rinominare "a" in "b" (già esistente, non è quella che stiamo
        // sostituendo) deve fallire.
        let result = save_entry(
            dir.path(),
            "b",
            "d2",
            vec![],
            "c",
            "cb2",
            "2026-08-05",
            Some("a"),
        );
        assert_eq!(result, Err(SaveError::NameCollision));

        // Nessuno dei due file/entry originali deve essere stato toccato.
        assert!(dir.path().join("a.ps1").exists());
        let idx = load_index(dir.path());
        assert_eq!(idx.scripts.len(), 2);
    }

    // ── get_content ──────────────────────────────────────────────────────

    #[test]
    fn get_content_found_returns_entry_and_body() {
        let dir = tempfile::tempdir().unwrap();
        save_entry(
            dir.path(),
            "r",
            "d",
            vec!["t".to_string()],
            "c",
            "Write-Host x",
            "2026-08-05",
            None,
        )
        .unwrap();
        let idx = load_index(dir.path());

        let result = get_content(dir.path(), &idx, "r");
        let (entry, content) = result.expect("atteso Some");
        assert_eq!(entry.name, "r");
        assert_eq!(content, "Write-Host x");
    }

    #[test]
    fn get_content_not_found_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let idx = load_index(dir.path());
        assert!(get_content(dir.path(), &idx, "does-not-exist").is_none());
    }
}
