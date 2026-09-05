//! Risoluzione della cartella di configurazione per ui.exe (2.0): UNA sola
//! funzione al posto dei sei risolutori duplicati della v1 (main.rs,
//! search_settings, plugins_view, market_data_settings, llm_settings,
//! aichat_settings), che leggevano ciascuno per conto proprio le stesse
//! variabili d'ambiente. Ora: `--config-dir` oppure `<exe_dir>/Configuration`,
//! identico all'orchestratore (stesso crate `startup-config`).

use startup_config::StartupConfig;
use std::path::{Path, PathBuf};

/// Stato gestito da Tauri (`app.manage`): la cartella risolta e lo
/// `startup.json` letto una volta all'avvio.
pub struct ConfigDirState {
    pub config_dir: PathBuf,
    pub startup: StartupConfig,
}

impl ConfigDirState {
    /// Da argv reali + cartella dell'eseguibile. `warn` va loggato dal chiamante.
    pub fn from_process() -> (Self, Option<String>) {
        let config_dir = startup_config::config_dir_from_process();
        let (startup, warn) = StartupConfig::load(&config_dir);
        (
            Self {
                config_dir,
                startup,
            },
            warn,
        )
    }
    pub fn ws_endpoint(&self) -> String {
        format!("ws://127.0.0.1:{}", self.startup.ws_port)
    }
    pub fn plugins_dir(&self) -> PathBuf {
        StartupConfig::resolve_path(&self.config_dir, &self.startup.paths.plugins_dir)
    }
}

pub fn token_path(config_dir: &Path) -> PathBuf {
    config_dir.join(startup_config::TOKEN_FILE_NAME)
}
pub fn config_file_path(config_dir: &Path) -> PathBuf {
    config_dir.join("config.json")
}
pub fn library_dir_path(config_dir: &Path) -> PathBuf {
    config_dir.join("library")
}
pub fn find_dir_path(config_dir: &Path) -> PathBuf {
    library_dir_path(config_dir).join("find")
}

/// Legge il token dal file; stringa vuota se assente/vuoto (il frontend
/// mostra lo stato "errore" come in v1). A differenza dell'orchestrator
/// (`token_store::resolve_token`), la ui NON genera mai il token: è solo
/// lettrice, chi lo crea al primo avvio è l'orchestrator.
pub fn read_token(config_dir: &Path) -> String {
    std::fs::read_to_string(token_path(config_dir))
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paths_are_all_under_config_dir() {
        let d = Path::new("C:/Lare/Configuration");
        assert_eq!(token_path(d), d.join("token"));
        assert_eq!(config_file_path(d), d.join("config.json"));
        assert_eq!(library_dir_path(d), d.join("library"));
        assert_eq!(find_dir_path(d), d.join("library").join("find"));
    }
    #[test]
    fn read_token_trims_and_defaults_to_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_token(dir.path()), "");
        std::fs::write(dir.path().join("token"), " t0k3n \n").unwrap();
        assert_eq!(read_token(dir.path()), "t0k3n");
    }
    #[test]
    fn ws_endpoint_and_plugins_dir_come_from_startup() {
        // `..Default::default()` invece di un `mut` + riassegnazione del
        // campo (clippy::field_reassign_with_default) — stesso valore del
        // brief, solo idiomatico.
        let startup = StartupConfig {
            ws_port: 7400,
            ..StartupConfig::default()
        };
        let st = ConfigDirState {
            config_dir: PathBuf::from("C:/Lare/Configuration"),
            startup,
        };
        assert_eq!(st.ws_endpoint(), "ws://127.0.0.1:7400");
        assert_eq!(st.plugins_dir(), Path::new("C:/Lare").join("plugins"));
    }
}
