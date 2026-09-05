//! config.rs — Configurazione persistente di Lare Commander.
//!
//! I path dei due pannelli vengono salvati in `config.json` **dentro la
//! directory di storage privata del plugin** (fornita dall'host nell'`Init`,
//! es. `%LOCALAPPDATA%\dev.lare.terminal\plugin-storage\lc\`). Questo file è di
//! proprietà ESCLUSIVA del plugin — nulla nell'orchestrator lo legge o scrive —
//! quindi contiene solo i due campi `left_path`/`right_path` e non serve alcun
//! merge con altri campi.
//!
//! **Politica di persistenza (attraversa i riavvii):**
//! - Ad ogni **navigazione** (Enter/Backspace) i path correnti vengono salvati
//!   (vedi `state::save_config`) — è il sostituto funzionale del "salva alla
//!   chiusura", che non esiste come segnale verso il processo plugin.
//! - All'`Activate` i path salvati vengono riletti (`load`).
//! - Nessun reset all'`Init`: lo stato sopravvive al riavvio dell'orchestratore.
//!
//! Se lo storage non è raggiungibile, tutte le operazioni falliscono
//! silenziosamente — il plugin funziona comunque con i valori di default (home).

use std::path::{Path, PathBuf};
use serde_json::Value;

/// Nome del file di configurazione dentro `storage_dir`.
const CONFIG_FILE: &str = "config.json";

/// Configurazione persistente del plugin (path dei due pannelli).
#[derive(Debug, Clone)]
pub struct LcConfig {
    pub left_path:  PathBuf,
    pub right_path: PathBuf,
}

impl LcConfig {
    /// Restituisce la home directory dell'utente corrente.
    /// Priorità: `%USERPROFILE%` (Windows) → `$HOME` (Unix) → radice del filesystem.
    pub fn home() -> PathBuf {
        std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                if cfg!(windows) {
                    PathBuf::from("C:\\")
                } else {
                    PathBuf::from("/")
                }
            })
    }

    /// Legge `left_path` e `right_path` da `config.json` nella directory `storage_dir`.
    ///
    /// - Se il file non esiste o non è JSON valido → entrambi i path sulla home.
    /// - Se un path letto non esiste sul filesystem → quel path rimpiazzato con la home.
    /// - In caso di errore I/O → entrambi i path sulla home (fail silenzioso).
    pub fn load(storage_dir: &Path) -> Self {
        let home = Self::home();
        let json_path = storage_dir.join(CONFIG_FILE);

        let content = match std::fs::read_to_string(&json_path) {
            Ok(c) => c,
            Err(_) => return Self { left_path: home.clone(), right_path: home },
        };

        let v: Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(_) => return Self { left_path: home.clone(), right_path: home },
        };

        let parse_path = |key: &str| -> PathBuf {
            v.get(key)
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
                .filter(|p| p.exists())
                .unwrap_or_else(|| home.clone())
        };

        Self {
            left_path:  parse_path("left_path"),
            right_path: parse_path("right_path"),
        }
    }

    /// Salva `left_path` e `right_path` in `storage_dir/config.json`.
    ///
    /// Fallisce silenziosamente in caso di errore I/O o di serializzazione: la
    /// navigazione nel plugin non deve mai rompersi perché lo storage è ostile.
    pub fn save(&self, storage_dir: &Path) {
        let _ = self.try_save(storage_dir);
    }

    fn try_save(&self, storage_dir: &Path) -> std::io::Result<()> {
        // Guardia: uno `storage_dir` vuoto (nessuna dir nota, es. stato di test o
        // `Init` mai ricevuto) risolverebbe in un `config.json` RELATIVO scritto
        // nella cwd corrente — inquinamento indesiderato. Non salviamo nulla.
        if storage_dir.as_os_str().is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "storage_dir non impostata: salvataggio saltato",
            ));
        }
        // L'host non garantisce che storage_dir esista già: creala se manca.
        std::fs::create_dir_all(storage_dir)?;
        let json_path = storage_dir.join(CONFIG_FILE);

        // File di proprietà esclusiva del plugin → serializziamo direttamente i
        // due soli campi, senza leggere/merge-are alcunché.
        let v: Value = serde_json::json!({
            "left_path":  self.left_path.to_string_lossy(),
            "right_path": self.right_path.to_string_lossy(),
        });

        let serialized = serde_json::to_string_pretty(&v)
            .map_err(std::io::Error::other)?;
        std::fs::write(&json_path, serialized)
    }
}

// ─── Test ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Crea una directory temporanea pulita per ogni test.
    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lc_config_test_{tag}"));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn load_returns_home_when_file_missing() {
        let dir = temp_dir("missing");
        let cfg = LcConfig::load(&dir);
        let home = LcConfig::home();
        assert_eq!(cfg.left_path,  home, "senza config.json il path sinistro deve essere la home");
        assert_eq!(cfg.right_path, home, "senza config.json il path destro deve essere la home");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_returns_home_when_saved_paths_dont_exist() {
        let dir = temp_dir("invalid");
        let json = serde_json::json!({
            "left_path":  "C:\\NonExistentPath_XyzAbc_123",
            "right_path": "C:\\AlsoNonExistent_456789"
        });
        fs::write(dir.join("config.json"), json.to_string()).unwrap();

        let cfg = LcConfig::load(&dir);
        let home = LcConfig::home();
        assert_eq!(cfg.left_path,  home, "path salvato inesistente → home");
        assert_eq!(cfg.right_path, home, "path salvato inesistente → home");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_and_reload_roundtrips() {
        let dir = temp_dir("roundtrip");
        let tmp = std::env::temp_dir(); // esiste sempre

        LcConfig { left_path: tmp.clone(), right_path: tmp.clone() }.save(&dir);

        let loaded = LcConfig::load(&dir);
        assert_eq!(loaded.left_path,  tmp, "il path sinistro deve essere il valore salvato");
        assert_eq!(loaded.right_path, tmp, "il path destro deve essere il valore salvato");
        let _ = fs::remove_dir_all(&dir);
    }

    /// L'host NON garantisce che `storage_dir` esista già → `save` deve crearla.
    #[test]
    fn save_creates_storage_dir_if_missing() {
        let parent  = temp_dir("createdir");
        let storage = parent.join("not_yet_created"); // NON esiste ancora
        assert!(!storage.exists(), "precondizione: storage_dir non deve esistere");
        let tmp = std::env::temp_dir();

        LcConfig { left_path: tmp.clone(), right_path: tmp }.save(&storage);

        assert!(storage.join("config.json").exists(),
            "save deve creare storage_dir e scrivervi config.json");
        let _ = fs::remove_dir_all(&parent);
    }

    /// Guardia contro l'inquinamento: uno `storage_dir` vuoto (stato di test o
    /// `Init` mai ricevuto) NON deve scrivere un `config.json` relativo nella cwd.
    #[test]
    fn save_skips_when_storage_dir_is_empty() {
        let cfg = LcConfig {
            left_path:  PathBuf::from("/x"),
            right_path: PathBuf::from("/y"),
        };
        let err = cfg.try_save(Path::new("")).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound,
            "storage_dir vuota → nessuna scrittura");
    }

    /// `config.json` è di proprietà esclusiva del plugin: niente merge di altri
    /// campi, solo `left_path` e `right_path`.
    #[test]
    fn save_writes_only_left_and_right() {
        let dir = temp_dir("onlyfields");
        let tmp = std::env::temp_dir();
        LcConfig { left_path: tmp.clone(), right_path: tmp }.save(&dir);

        let content = fs::read_to_string(dir.join("config.json")).unwrap();
        let v: Value = serde_json::from_str(&content).unwrap();
        let obj = v.as_object().expect("config.json deve essere un oggetto JSON");
        assert!(obj.contains_key("left_path"),  "deve contenere left_path");
        assert!(obj.contains_key("right_path"), "deve contenere right_path");
        assert_eq!(obj.len(), 2,
            "config.json deve contenere SOLO left_path e right_path");
        let _ = fs::remove_dir_all(&dir);
    }
}
