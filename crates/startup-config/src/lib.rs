//! # startup-config — configurazione da file accanto agli eseguibili
//!
//! `startup.json` vive nella STESSA cartella dell'eseguibile che lo legge
//! (`orchestrator.exe`, `mcp-server.exe` — non `ui.exe`, fase 2). Aggiunge
//! un livello di precedenza fra la env var e il default hardcoded, per due
//! motivi (vedi Docs/superpowers/specs/2026-08-12-startup-config-design.md):
//! pulizia (~10 env var oggi lette in punti diversi, stessa catena di
//! fallback copiata quasi identica in 7 punti) e requisito del futuro
//! servizio Windows (SCM non ha un modo nativo di impostare env var
//! per-servizio come fa uno script `.ps1`).
//!
//! Crate infrastrutturale puro: nessuna logica di dominio, nessuna
//! dipendenza da tokio/tauri/rmcp.

use serde::Deserialize;
use std::io;
use std::path::{Path, PathBuf};

/// Schema di `startup.json`. Tutti i campi opzionali: un campo assente O
/// il file stesso assente (`Ok(None)` da `load_from_dir`) significano
/// "usa il default di sempre", MAI un errore.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct StartupConfig {
    #[serde(default)]
    pub local_dir: Option<String>,
    #[serde(default)]
    pub roaming_dir: Option<String>,
    #[serde(default)]
    pub plugins_dir: Option<String>,
    #[serde(default)]
    pub routines_dir: Option<String>,
    #[serde(default)]
    pub telegram_settings: Option<String>,
    #[serde(default)]
    pub llms_config: Option<String>,
}

/// Cartella dell'eseguibile in esecuzione — stesso ancoraggio già usato da
/// `McpToolClient::resolve()`/`NmapToolClient::resolve()` per trovare i
/// binari sibling. Funziona identico da CLI, da servizio Windows (SCM), da
/// Task Scheduler: `current_exe()` riflette sempre il path del binario
/// realmente lanciato, mai la cwd (che PUÒ differire — es. `System32` per
/// un servizio senza working directory esplicita).
pub fn exe_dir() -> io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    exe.parent().map(Path::to_path_buf).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "current_exe() non ha una cartella padre",
        )
    })
}

/// Carica `startup.json` dalla cartella indicata (parametro esplicito, non
/// chiama `exe_dir()` internamente — testabile con qualunque tempdir).
///
/// - `Ok(None)` — file assente: caso normale, nessun `startup.json`
///   installato o workflow di sviluppo (`cargo run`, nessun file accanto a
///   `target/debug/`).
/// - `Ok(Some(cfg))` — file trovato e JSON valido.
/// - `Err(_)` — file presente ma illeggibile o JSON malformato: il
///   chiamante logga e cade al default, MAI un panic.
pub fn load_from_dir(dir: &Path) -> Result<Option<StartupConfig>, String> {
    let path = dir.join("startup.json");
    match std::fs::read_to_string(&path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("startup.json: errore di lettura ({e})")),
        Ok(content) => serde_json::from_str(&content)
            .map(Some)
            .map_err(|e| format!("startup.json: JSON malformato ({e})")),
    }
}

/// Precedenza a 3 livelli: env var (se impostata, non vuota dopo
/// `.trim()`) > campo del file (se presente, non vuoto dopo `.trim()`) >
/// default fornito dal chiamante (closure, valutata SOLO se serve — evita
/// di costruire un `PathBuf` di default quando non verrà usato). Il trim
/// serve solo al controllo di vuoto: il valore restituito è quello
/// originale, non trimmato — stessa convenzione già in uso in
/// `llms_config`/`routines.rs` oggi.
pub fn resolve(
    env_value: Option<&str>,
    file_value: Option<&str>,
    default: impl FnOnce() -> PathBuf,
) -> PathBuf {
    if let Some(v) = env_value {
        if !v.trim().is_empty() {
            return PathBuf::from(v);
        }
    }
    if let Some(v) = file_value {
        if !v.trim().is_empty() {
            return PathBuf::from(v);
        }
    }
    default()
}

/// Default di `local_dir` quando né la env var né `startup.json` lo
/// specificano: `%LOCALAPPDATA%\dev.lare.terminal\` (fallback
/// `.lare-data\` se `LOCALAPPDATA` non è impostata — sviluppo/non-Windows).
/// Stessa identica catena duplicata oggi in ~7 punti diversi — l'UNICA
/// copia resta qui: ogni binario la passa come `default` a `resolve()` per
/// il campo `local_dir`. Legge l'ambiente direttamente (a differenza di
/// `resolve()`, che è pura): stessa categoria di `exe_dir()`, un wrapper
/// sottile sul confine col sistema operativo — vedi nota di test più sotto.
pub fn default_local_dir() -> PathBuf {
    match std::env::var("LOCALAPPDATA") {
        Ok(local) if !local.is_empty() => PathBuf::from(local).join("dev.lare.terminal"),
        _ => PathBuf::from(".lare-data"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // ── resolve ──────────────────────────────────────────────────────────

    #[test]
    fn resolve_env_value_wins() {
        let p = resolve(Some("D:/from-env"), Some("D:/from-file"), || {
            PathBuf::from("D:/default")
        });
        assert_eq!(p, PathBuf::from("D:/from-env"));
    }

    #[test]
    fn resolve_whitespace_only_env_falls_through_to_file() {
        let p = resolve(Some("   "), Some("D:/from-file"), || PathBuf::from("D:/default"));
        assert_eq!(p, PathBuf::from("D:/from-file"));
    }

    #[test]
    fn resolve_file_value_wins_when_env_absent() {
        let p = resolve(None, Some("D:/from-file"), || PathBuf::from("D:/default"));
        assert_eq!(p, PathBuf::from("D:/from-file"));
    }

    #[test]
    fn resolve_whitespace_only_file_falls_through_to_default() {
        let p = resolve(None, Some("   "), || PathBuf::from("D:/default"));
        assert_eq!(p, PathBuf::from("D:/default"));
    }

    #[test]
    fn resolve_uses_default_when_both_absent() {
        let p = resolve(None, None, || PathBuf::from("D:/default"));
        assert_eq!(p, PathBuf::from("D:/default"));
    }

    #[test]
    fn resolve_default_closure_not_evaluated_when_env_wins() {
        // Se la closure venisse comunque chiamata, il panic fallisce il test:
        // dimostra che `default` è pigro (evaluato solo quando serve).
        let p = resolve(Some("D:/from-env"), None, || -> PathBuf {
            panic!("default non deve essere valutato quando env_value vince")
        });
        assert_eq!(p, PathBuf::from("D:/from-env"));
    }

    // ── load_from_dir ────────────────────────────────────────────────────

    #[test]
    fn load_from_dir_missing_file_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let result = load_from_dir(dir.path());
        assert!(matches!(result, Ok(None)));
    }

    #[test]
    fn load_from_dir_valid_json_returns_some_with_fields() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = std::fs::File::create(dir.path().join("startup.json")).unwrap();
        write!(
            f,
            r#"{{"local_dir": "C:/Lare Terminal/Local", "llms_config": "C:/Lare Terminal/Local/llms.json"}}"#
        )
        .unwrap();
        let cfg = load_from_dir(dir.path()).unwrap().expect("atteso Some");
        assert_eq!(cfg.local_dir.as_deref(), Some("C:/Lare Terminal/Local"));
        assert_eq!(
            cfg.llms_config.as_deref(),
            Some("C:/Lare Terminal/Local/llms.json")
        );
        assert_eq!(cfg.roaming_dir, None);
    }

    #[test]
    fn load_from_dir_empty_object_returns_some_with_all_none() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = std::fs::File::create(dir.path().join("startup.json")).unwrap();
        write!(f, "{{}}").unwrap();
        let cfg = load_from_dir(dir.path()).unwrap().expect("atteso Some");
        assert_eq!(cfg.local_dir, None);
        assert_eq!(cfg.roaming_dir, None);
        assert_eq!(cfg.plugins_dir, None);
        assert_eq!(cfg.routines_dir, None);
        assert_eq!(cfg.telegram_settings, None);
        assert_eq!(cfg.llms_config, None);
    }

    #[test]
    fn load_from_dir_malformed_json_returns_err() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = std::fs::File::create(dir.path().join("startup.json")).unwrap();
        write!(f, "not json at all").unwrap();
        let result = load_from_dir(dir.path());
        assert!(result.is_err());
    }
}
