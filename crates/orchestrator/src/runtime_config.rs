//! # runtime_config — `RuntimeConfig`, il "context object" immutabile
//!
//! Tutto ciò che il resto dell'orchestrator deve sapere della configurazione
//! è risolto UNA volta in `main()` (config dir + `startup.json`) e passato da
//! lì in poi come `Arc<RuntimeConfig>` — mai ri-derivato altrove (niente più
//! `LARE_*`/`LOCALAPPDATA`/`APPDATA` sparsi nel crate, decisione
//! D6 dello spec 2.0).
//!
//! Analogia OOP: `RuntimeConfig` è un "context object" immutabile, come un
//! `HttpContext`/`ApplicationContext` in altri stack — costruito una volta
//! all'avvio, passato per riferimento condiviso (`Arc`) a chiunque ne abbia
//! bisogno, mai mutato dopo la costruzione. I metodi `plugins_dir()`,
//! `mcp_server_exe()`, ecc. sono "getter derivati": non aggiungono stato,
//! incapsulano solo la stessa regola di risoluzione (`StartupConfig::
//! resolve_path`, relativo alla radice del deploy) che altrimenti andrebbe
//! ripetuta ad ogni punto di chiamata.
//!
//! `config_dir` qui dentro è SEMPRE assoluto — non lo garantisce più questo
//! `main()` (fix wave finale, review) ma `startup_config::
//! config_dir_from_process()`, chiamata da `main` per costruire `config_dir`:
//! un path relativo salvato prima del cambio di cwd del processo
//! (`set_current_dir(home)`, più sotto in `main`) risolverebbe in modo
//! diverso a seconda di QUANDO lo si usa — bug latente evitato risolvendolo
//! in assoluto una volta sola, alla fonte condivisa da ogni binario.

use std::path::PathBuf;
#[cfg(test)]
use std::path::Path;

use startup_config::StartupConfig;

/// Tutto ciò che il resto dell'orchestratore deve sapere della configurazione,
/// risolto UNA volta in `main`. Analogia OOP: un "context object" immutabile
/// passato per riferimento (`Arc`) a chi ne ha bisogno.
pub struct RuntimeConfig {
    /// Cartella di configurazione (`--config-dir` o default), SEMPRE assoluta.
    pub config_dir: PathBuf,
    /// Contenuto di `startup.json` (o i suoi default).
    pub startup: StartupConfig,
}

impl RuntimeConfig {
    /// Risolve un valore di `paths`/`log.dir` rispetto alla radice del deploy
    /// (assoluto → invariato). Vedi `StartupConfig::resolve_path`.
    pub fn path(&self, value: &str) -> PathBuf {
        StartupConfig::resolve_path(&self.config_dir, value)
    }

    /// Percorso di `network.json` (config AI Chat/Blocco note), sempre in
    /// `config_dir` — nessuna precedenza env/startup.json separata (D6): un
    /// solo file, un solo posto.
    pub fn network_json_path(&self) -> PathBuf {
        self.config_dir.join("network.json")
    }

    pub fn plugins_dir(&self) -> PathBuf {
        self.path(&self.startup.paths.plugins_dir)
    }
    pub fn pytools_dir(&self) -> PathBuf {
        self.path(&self.startup.paths.pytools_dir)
    }
    pub fn mcp_server_exe(&self) -> PathBuf {
        self.path(&self.startup.paths.mcp_server)
    }
    pub fn mcp_nmap_exe(&self) -> PathBuf {
        self.path(&self.startup.paths.mcp_nmap)
    }
    pub fn log_dir(&self) -> PathBuf {
        self.path(&self.startup.log.dir)
    }

    /// Costruisce un `RuntimeConfig` di comodo per i test: `startup.json`
    /// default + `config_dir` passata (tipicamente una tempdir). Evita di
    /// dover ripetere `RuntimeConfig { config_dir: .., startup: StartupConfig
    /// ::default() }` in decine di test in giro per il crate.
    #[cfg(test)]
    pub fn for_test(config_dir: &Path) -> Self {
        Self {
            config_dir: config_dir.to_path_buf(),
            startup: StartupConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn plugins_dir_resolves_relative_to_deploy_root() {
        let dir = tempdir().unwrap();
        let config_dir = dir.path().join("Configuration");
        let rt = RuntimeConfig::for_test(&config_dir);
        // default: paths.plugins_dir = "plugins", relativo alla radice del
        // deploy = padre di config_dir.
        assert_eq!(rt.plugins_dir(), dir.path().join("plugins"));
    }

    #[test]
    fn mcp_server_exe_resolves_relative_to_deploy_root() {
        let dir = tempdir().unwrap();
        let config_dir = dir.path().join("Configuration");
        let rt = RuntimeConfig::for_test(&config_dir);
        assert_eq!(rt.mcp_server_exe(), dir.path().join("mcp-server.exe"));
    }

    #[test]
    fn log_dir_default_is_configuration_logs_under_config_dir() {
        let dir = tempdir().unwrap();
        let config_dir = dir.path().join("Configuration");
        let rt = RuntimeConfig::for_test(&config_dir);
        // default: log.dir = "Configuration/logs", relativo alla radice del
        // deploy (padre di config_dir) — risolve dentro la STESSA
        // config_dir (config_dir è già ".../Configuration").
        assert_eq!(rt.log_dir(), config_dir.join("logs"));
    }

    #[test]
    fn network_json_path_is_always_in_config_dir_directly() {
        let dir = tempdir().unwrap();
        let config_dir = dir.path().join("Configuration");
        let rt = RuntimeConfig::for_test(&config_dir);
        assert_eq!(rt.network_json_path(), config_dir.join("network.json"));
    }
}
