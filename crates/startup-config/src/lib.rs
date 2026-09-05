//! # startup-config — cartella di configurazione e `startup.json` (2.0)
//!
//! Regola unica per OGNI binario Lare (orchestrator, mcp-server, ui, plugin,
//! server Python): la cartella di configurazione è `--config-dir <path>` se
//! passato sulla riga di comando, altrimenti `<cartella dell'eseguibile>\Configuration\`.
//! Nessuna variabile d'ambiente viene letta (decisione D6 dello spec 2.0):
//! nella v1 tre lettori indipendenti (orchestrator, ui, Python) della stessa
//! env var divergevano in silenzio.
//!
//! Dentro la cartella vive `startup.json`, i cui percorsi relativi sono
//! risolti rispetto alla RADICE DEL DEPLOY = cartella padre di `Configuration\`
//! (non alla cwd, non alla cartella dell'eseguibile: `lare-shell.exe` vive in
//! `shell\`, i binari Rust nella radice — un'unica base evita due risultati).
//!
//! Analogia OOP: `StartupConfig` è un "value object" immutabile con default;
//! le funzioni libere sono metodi statici di una classe di utilità.

use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

pub const CONFIG_DIR_FLAG: &str = "--config-dir";
pub const DEFAULT_CONFIG_DIR_NAME: &str = "Configuration";
pub const STARTUP_FILE_NAME: &str = "startup.json";

/// Estrae `--config-dir <path>` oppure `--config-dir=<path>` da `args`
/// (argv completo, `args[0]` = eseguibile). Ignora ogni altro argomento.
/// Flag presente ma senza valore → `None` (il chiamante userà il default).
pub fn parse_config_dir(args: &[String]) -> Option<PathBuf> {
    let mut iter = args.iter();
    while let Some(a) = iter.next() {
        if a == CONFIG_DIR_FLAG {
            return iter
                .next()
                .filter(|v| !v.trim().is_empty())
                .map(PathBuf::from);
        }
        if let Some(v) = a.strip_prefix(&format!("{CONFIG_DIR_FLAG}=")) {
            if !v.trim().is_empty() {
                return Some(PathBuf::from(v));
            }
        }
    }
    None
}

/// `true` se `flag` compare in `args` (flag booleani come `--console-log`).
pub fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|a| a == flag)
}

/// Flag esplicito > `<exe_dir>/Configuration`. `exe_dir` è un parametro (non
/// letto qui) così il test non dipende dalla posizione del binario di test.
pub fn resolve_config_dir(flag: Option<PathBuf>, exe_dir: &Path) -> PathBuf {
    flag.unwrap_or_else(|| exe_dir.join(DEFAULT_CONFIG_DIR_NAME))
}

/// Comodità per i `main`: argv reali + cartella dell'eseguibile reale.
/// Se `current_exe()` fallisce (caso rarissimo) cade sulla cwd.
pub fn config_dir_from_process() -> PathBuf {
    let args: Vec<String> = std::env::args().collect();
    let exe = exe_dir().unwrap_or_else(|_| PathBuf::from("."));
    resolve_config_dir(parse_config_dir(&args), &exe)
}

/// Cartella dell'eseguibile in esecuzione (invariata dalla v1): `current_exe()`
/// riflette il binario davvero lanciato, mai la cwd.
pub fn exe_dir() -> io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    exe.parent().map(Path::to_path_buf).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "current_exe() non ha una cartella padre",
        )
    })
}

/// Radice del deploy = cartella padre di `config_dir`. Se `config_dir` non ha
/// padre (es. `/`), è la radice stessa.
pub fn deploy_root(config_dir: &Path) -> PathBuf {
    config_dir
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| config_dir.to_path_buf())
}

/// Schema di `startup.json` (spec §6.3). Ogni campo ha un default: file
/// assente = tutti i default; campo assente = default di quel campo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StartupConfig {
    pub ws_port: u16,
    pub paths: Paths,
    pub ai_model: String,
    pub autostart: Autostart,
    pub log: LogConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Paths {
    pub shell: String,
    pub mcp_server: String,
    pub mcp_nmap: String,
    pub plugins_dir: String,
    pub pytools_dir: String,
    pub routines_dir: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Autostart {
    pub orchestrator: bool,
    pub ui: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LogConfig {
    pub level: String,
    pub dir: String,
}

impl Default for StartupConfig {
    fn default() -> Self {
        Self {
            ws_port: 7331,
            paths: Paths::default(),
            ai_model: "claude-sonnet-4-6".into(),
            autostart: Autostart::default(),
            log: LogConfig::default(),
        }
    }
}
impl Default for Paths {
    fn default() -> Self {
        Self {
            shell: "shell/lare-shell.exe".into(),
            mcp_server: "mcp-server.exe".into(),
            mcp_nmap: "mcp-nmap.exe".into(),
            plugins_dir: "plugins".into(),
            pytools_dir: "pytools".into(),
            routines_dir: "Configuration/routines".into(),
        }
    }
}
impl Default for Autostart {
    fn default() -> Self {
        Self {
            orchestrator: true,
            ui: true,
        }
    }
}
impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: "info".into(),
            dir: "Configuration/logs".into(),
        }
    }
}

impl StartupConfig {
    /// Legge `<config_dir>/startup.json`. Ritorna sempre una configurazione
    /// utilizzabile: file assente → default, nessun avviso; file presente ma
    /// illeggibile/malformato → default + avviso (il chiamante lo logga: un
    /// `startup.json` con una virgola di troppo non deve fallire in silenzio).
    pub fn load(config_dir: &Path) -> (StartupConfig, Option<String>) {
        let path = config_dir.join(STARTUP_FILE_NAME);
        match std::fs::read_to_string(&path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => (StartupConfig::default(), None),
            Err(e) => (
                StartupConfig::default(),
                Some(format!(
                    "{}: errore di lettura ({e}) — uso i default",
                    path.display()
                )),
            ),
            Ok(content) => match serde_json::from_str::<StartupConfig>(&content) {
                Ok(cfg) => (cfg, None),
                Err(e) => (
                    StartupConfig::default(),
                    Some(format!(
                        "{}: JSON malformato ({e}) — uso i default",
                        path.display()
                    )),
                ),
            },
        }
    }

    /// Risolve un valore di `paths`: assoluto → com'è; relativo → rispetto
    /// alla radice del deploy (`deploy_root(config_dir)`).
    pub fn resolve_path(config_dir: &Path, value: &str) -> PathBuf {
        let p = Path::new(value);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            deploy_root(config_dir).join(p)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    // ── parse_config_dir ─────────────────────────────────────────────────
    #[test]
    fn parse_flag_with_separate_value() {
        assert_eq!(
            parse_config_dir(&args(&["ui.exe", "--config-dir", "D:/cfg"])),
            Some(PathBuf::from("D:/cfg"))
        );
    }
    #[test]
    fn parse_flag_with_equals() {
        assert_eq!(
            parse_config_dir(&args(&["x", "--config-dir=D:/cfg"])),
            Some(PathBuf::from("D:/cfg"))
        );
    }
    #[test]
    fn parse_absent_flag_is_none() {
        assert_eq!(parse_config_dir(&args(&["x", "--open", "config"])), None);
    }
    #[test]
    fn parse_flag_without_value_is_none() {
        assert_eq!(parse_config_dir(&args(&["x", "--config-dir"])), None);
    }

    // ── resolve_config_dir ───────────────────────────────────────────────
    #[test]
    fn resolve_flag_wins() {
        let p = resolve_config_dir(Some(PathBuf::from("D:/cfg")), Path::new("C:/app"));
        assert_eq!(p, PathBuf::from("D:/cfg"));
    }
    #[test]
    fn resolve_default_is_configuration_next_to_exe() {
        let p = resolve_config_dir(None, Path::new("C:/Lare"));
        assert_eq!(p, Path::new("C:/Lare").join("Configuration"));
    }

    // ── deploy_root ──────────────────────────────────────────────────────
    #[test]
    fn deploy_root_is_parent_of_config_dir() {
        assert_eq!(
            deploy_root(Path::new("C:/Lare/Configuration")),
            PathBuf::from("C:/Lare")
        );
    }

    // ── StartupConfig::load ──────────────────────────────────────────────
    #[test]
    fn load_absent_file_gives_defaults_without_warning() {
        let dir = tempfile::tempdir().unwrap();
        let (cfg, warn) = StartupConfig::load(dir.path());
        assert_eq!(cfg, StartupConfig::default());
        assert!(warn.is_none());
        assert_eq!(cfg.ws_port, 7331);
        assert_eq!(cfg.paths.mcp_server, "mcp-server.exe");
        assert_eq!(cfg.paths.routines_dir, "Configuration/routines");
        assert_eq!(cfg.ai_model, "claude-sonnet-4-6");
        assert!(cfg.autostart.orchestrator && cfg.autostart.ui);
        assert_eq!(cfg.log.dir, "Configuration/logs");
        assert_eq!(cfg.log.level, "info");
    }
    #[test]
    fn load_partial_file_fills_missing_with_defaults() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("startup.json"),
            r#"{ "ws_port": 8000, "paths": { "plugins_dir": "plug" } }"#,
        )
        .unwrap();
        let (cfg, warn) = StartupConfig::load(dir.path());
        assert!(warn.is_none());
        assert_eq!(cfg.ws_port, 8000);
        assert_eq!(cfg.paths.plugins_dir, "plug");
        assert_eq!(cfg.paths.mcp_server, "mcp-server.exe"); // default conservato
    }
    #[test]
    fn load_malformed_file_gives_defaults_with_warning() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("startup.json"), "{ not json").unwrap();
        let (cfg, warn) = StartupConfig::load(dir.path());
        assert_eq!(cfg, StartupConfig::default());
        assert!(warn.unwrap().contains("startup.json"));
    }

    // ── resolve_path ─────────────────────────────────────────────────────
    #[test]
    fn resolve_path_relative_is_against_deploy_root() {
        let p =
            StartupConfig::resolve_path(Path::new("C:/Lare/Configuration"), "shell/lare-shell.exe");
        assert_eq!(p, Path::new("C:/Lare").join("shell/lare-shell.exe"));
    }
    #[test]
    fn resolve_path_absolute_is_kept() {
        let p = StartupConfig::resolve_path(Path::new("C:/Lare/Configuration"), "D:/tools/x.exe");
        assert_eq!(p, PathBuf::from("D:/tools/x.exe"));
    }
    #[test]
    fn resolve_path_with_nested_config_dir() {
        let p = StartupConfig::resolve_path(Path::new("C:/a/b/Configuration"), "plugins");
        assert_eq!(p, Path::new("C:/a/b").join("plugins"));
    }

    // ── has_flag ─────────────────────────────────────────────────────────
    #[test]
    fn has_flag_detects_console_log() {
        assert!(has_flag(
            &args(&["orchestrator.exe", "--console-log"]),
            "--console-log"
        ));
        assert!(!has_flag(&args(&["orchestrator.exe"]), "--console-log"));
    }
}
