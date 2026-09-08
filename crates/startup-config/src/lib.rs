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

pub mod logging;

pub const CONFIG_DIR_FLAG: &str = "--config-dir";
pub const DEFAULT_CONFIG_DIR_NAME: &str = "Configuration";
pub const STARTUP_FILE_NAME: &str = "startup.json";
/// Nome del file token (`<config_dir>/token`) — condiviso da orchestrator
/// (che lo genera) e ui (che lo legge soltanto): prima di questa costante
/// ciascuno dei due crate scriveva la stringa `"token"` per conto proprio,
/// due copie della stessa "magic string" da tenere sincronizzate a mano.
pub const TOKEN_FILE_NAME: &str = "token";

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

/// Rende `p` assoluto rispetto a `cwd` se è relativo; se `p` è già assoluto
/// lo ritorna invariato. Funzione pura (nessun I/O, nessuna lettura della
/// cwd reale) così è testabile senza dover spostare la cwd del processo di
/// test — usata da `config_dir_from_process` qui sotto, che le passa la cwd
/// vera al momento della chiamata.
pub fn absolutize(p: PathBuf, cwd: &Path) -> PathBuf {
    if p.is_absolute() {
        p
    } else {
        cwd.join(p)
    }
}

/// Comodità per i `main`: argv reali + cartella dell'eseguibile reale,
/// SEMPRE assolutizzata rispetto alla cwd del processo al momento della
/// chiamata. Se `current_exe()` fallisce (caso rarissimo) cade sulla cwd.
///
/// Perché l'assolutizzazione vive QUI e non nel singolo `main()` di un
/// binario (come faceva prima solo `orchestrator/src/main.rs`): l'unico
/// caso in cui `--config-dir` relativo è ambiguo è quando la cwd del
/// processo cambia DOPO l'avvio (l'orchestrator fa `set_current_dir(home)`
/// per dare al cursore un cwd iniziale sensato) — un `--config-dir`
/// relativo letto una seconda volta dopo quel cambio risolverebbe contro
/// `home`, non contro la cartella di lancio: due risultati diversi per lo
/// stesso flag. Prima questo fix viveva solo nell'orchestrator: `ui` e
/// `mcp-server`, che chiamano questa stessa funzione, non ne beneficiavano.
/// Risolvendo in assoluto qui, alla fonte condivisa, tutti i binari sono al
/// sicuro senza doverselo ricordare ciascuno per conto proprio. In caso di
/// errore nel leggere la cwd (rarissimo) si lascia il path relativo
/// invariato: meglio funzionante-finché-la-cwd-non-cambia che un errore
/// fatale all'avvio per un dettaglio secondario.
pub fn config_dir_from_process() -> PathBuf {
    let args: Vec<String> = std::env::args().collect();
    let exe = exe_dir().unwrap_or_else(|_| PathBuf::from("."));
    let dir = resolve_config_dir(parse_config_dir(&args), &exe);
    match std::env::current_dir() {
        Ok(cwd) => absolutize(dir, &cwd),
        Err(_) => dir,
    }
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

/// Avvia `exe` con `args`, staccato dal processo corrente (spec §6.4):
/// nessuna console ereditata, un Ctrl+C nel padre non lo abbatte, e le tre
/// stdio (stdin/stdout/stderr) sono chiuse esplicitamente (`Stdio::null()`)
/// invece di essere lasciate ereditare qualcosa di indefinito dal padre — è
/// un'azione esplicita di questa funzione, non solo un effetto collaterale
/// di `DETACHED_PROCESS`. Un solo posto che sa COME staccare un processo su
/// Windows — riusato sia da `ui.exe` (self-heal dell'orchestratore,
/// `launcher::ensure_orchestrator`) sia dall'orchestratore stesso (autostart
/// di `ui.exe`, piano 3 Task 6), invece di duplicare la logica nei due crate.
#[cfg(windows)]
pub fn spawn_detached(exe: &Path, args: &[String]) -> std::io::Result<std::process::Child> {
    use std::os::windows::process::CommandExt;
    // DETACHED_PROCESS (0x8) | CREATE_NEW_PROCESS_GROUP (0x200): equivalente
    // Windows di `setsid` — nessuna console ereditata, gruppo di processi
    // proprio (un Ctrl+C nella console del padre non raggiunge il figlio).
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    std::process::Command::new(exe)
        .args(args)
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
}

/// Non verificato fuori Windows (il prodotto oggi lo è, ADR-019): spawn
/// semplice, senza distacco — meglio di un errore di compilazione su altre
/// piattaforme di sviluppo.
#[cfg(not(windows))]
pub fn spawn_detached(exe: &Path, args: &[String]) -> std::io::Result<std::process::Child> {
    std::process::Command::new(exe).args(args).spawn()
}

/// Apre (creando la cartella se serve) un file di log in append per lo
/// stderr di un processo figlio console-subsystem spawnato senza console
/// (vedi `CREATE_NO_WINDOW` nei chiamanti) — altrimenti quello stderr non
/// andrebbe da nessuna parte di osservabile. Best-effort: se il file non si
/// apre (permessi, disco pieno, ...) ritorna `Stdio::null()` — perdere il
/// log dello stderr di un tool non deve MAI impedire al tool di partire.
pub fn child_stderr_log_sink(log_dir: &Path, file_name: &str) -> std::process::Stdio {
    if std::fs::create_dir_all(log_dir).is_err() {
        return std::process::Stdio::null();
    }
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join(file_name))
    {
        Ok(f) => std::process::Stdio::from(f),
        Err(_) => std::process::Stdio::null(),
    }
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
    #[test]
    fn deploy_root_of_config_dir_directly_under_a_drive_root() {
        // Caso limite: `Configuration\` è direttamente sotto la radice
        // dell'unità (nessuna cartella intermedia) — `Path::parent()` di
        // `C:/Configuration` è `C:/`, non `C:` senza slash: il deploy root
        // resta un path valido da usare con `.join(...)`.
        assert_eq!(
            deploy_root(Path::new("C:/Configuration")),
            PathBuf::from("C:/")
        );
    }

    // ── absolutize ───────────────────────────────────────────────────────
    // Funzione pura estratta da `config_dir_from_process` (fix wave finale,
    // review): un `--config-dir` relativo letto PRIMA che l'orchestrator
    // faccia `set_current_dir(home)` risolverebbe diversamente se qualcosa
    // lo rileggesse dopo quel cambio di cwd — assolutizzarlo qui, alla
    // fonte condivisa da ogni binario (orchestrator/ui/mcp-server), evita il
    // bug per tutti senza che ciascun `main()` debba ricordarselo da solo.
    #[test]
    fn absolutize_relative_path_is_joined_to_cwd() {
        assert_eq!(
            absolutize(PathBuf::from("Configuration"), Path::new("C:/work")),
            PathBuf::from("C:/work/Configuration")
        );
    }
    #[test]
    fn absolutize_absolute_path_is_left_unchanged() {
        assert_eq!(
            absolutize(PathBuf::from("D:/cfg"), Path::new("C:/work")),
            PathBuf::from("D:/cfg")
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
    #[test]
    fn resolve_path_rooted_without_drive_on_windows_drops_the_middle_of_the_base() {
        // Debito noto #2 (HANDOFF): `/x` non ha una lettera di unità, quindi
        // `Path::is_absolute()` su Windows ritorna `false` (richiede un
        // prefisso tipo `C:`) — `resolve_path` lo tratta come relativo e lo
        // passa a `deploy_root(config_dir).join(p)`. Ma `PathBuf::join` con
        // un path "rooted" (`has_root()==true`, inizia con `\`/`/`) SOSTITUISCE
        // la parte radice della base mantenendone solo il prefisso (l'unità):
        // `C:/Lare`.join("/x") == `C:/x`, NON `C:/Lare/x` — la cartella
        // intermedia sparisce silenziosamente. Comportamento di Windows, non
        // un bug di questo crate: qui lo fissiamo con un test perché nessuno
        // se lo scordi. Su Unix `/x` è genuinamente assoluto: nessuna sorpresa.
        let p = StartupConfig::resolve_path(Path::new("C:/Lare/Configuration"), "/x");
        if cfg!(windows) {
            assert_eq!(p, PathBuf::from("C:/x"));
        } else {
            assert_eq!(p, PathBuf::from("/x"));
        }
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

    // ── spawn_detached ───────────────────────────────────────────────────
    #[test]
    fn spawn_detached_avvia_un_processo_reale() {
        let mut child =
            spawn_detached(Path::new("cmd"), &["/c".into(), "exit".into(), "0".into()]).unwrap();
        let status = child.wait().unwrap();
        assert!(status.success());
    }
}
