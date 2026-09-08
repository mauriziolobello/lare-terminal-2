// crates/ui/src-tauri/src/launcher.rs
//! Self-heal all'avvio di `ui.exe` (spec §6.4): se il WS dell'orchestratore
//! non risponde e `autostart.orchestrator` è attivo in `startup.json`,
//! avvialo e ritenta per una finestra di tempo. Mirror Rust di
//! `shell/lare-shell/src/LareShell/Shell/Launcher.cs` (stessa struttura,
//! stessi nomi dove sensato) — un motore, due implementazioni dello stesso
//! ruolo (host C#, qui `ui.exe`), non eredità fra loro.

use std::path::Path;
use std::time::{Duration, Instant};

use startup_config::spawn_detached;

pub const CONNECT_WINDOW: Duration = Duration::from_secs(5);
pub const RETRY_INTERVAL: Duration = Duration::from_millis(250);

/// Se `connect()` fallisce e `autostart` è attivo, avvia `orchestrator_exe
/// --config-dir <config_dir>` (staccato) e ritenta `connect()` ogni `retry`
/// fino a `window`. `connect()`: `None` = riuscito, altrimenti il motivo
/// (stessa forma di `Func<string?>` in `Launcher.cs::EnsureConnected`).
/// Ritorna `true` se, alla fine, `connect()` ha avuto successo.
pub fn ensure_orchestrator(
    connect: &dyn Fn() -> Option<String>,
    autostart: bool,
    orchestrator_exe: &Path,
    config_dir: &Path,
    window: Duration,
    retry: Duration,
) -> bool {
    let Some(reason) = connect() else {
        return true;
    };

    if !autostart {
        tracing::warn!(
            "[ui] orchestratore non raggiungibile ({reason}); autostart disattivo in startup.json"
        );
        return false;
    }
    if !orchestrator_exe.exists() {
        tracing::warn!(
            "[ui] orchestratore non raggiungibile ({reason}) e {orchestrator_exe:?} non esiste"
        );
        return false;
    }

    tracing::warn!("[ui] orchestratore non raggiungibile ({reason}): avvio {orchestrator_exe:?}");
    let args = vec![
        "--config-dir".to_string(),
        config_dir.to_string_lossy().into_owned(),
    ];
    if let Err(e) = spawn_detached(orchestrator_exe, &args) {
        tracing::warn!("[ui] avvio orchestratore fallito: {e}");
        return false;
    }

    let deadline = Instant::now() + window;
    while Instant::now() < deadline {
        std::thread::sleep(retry);
        if connect().is_none() {
            // Esito positivo del self-heal, non un avviso — a differenza
            // degli altri messaggi di questa funzione (tutti percorsi di
            // fallimento), questo è "ha funzionato".
            tracing::info!("[ui] orchestratore avviato e connesso");
            return true;
        }
    }

    tracing::warn!("[ui] orchestratore non raggiungibile dopo {window:?}: i comandi non funzioneranno finché non risponde");
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::path::Path;
    use std::time::Duration;

    #[test]
    fn ensure_orchestrator_non_avvia_nulla_se_la_connessione_riesce_subito() {
        let calls = RefCell::new(0);
        let connect = || {
            *calls.borrow_mut() += 1;
            None
        };
        let ok = ensure_orchestrator(
            &connect,
            true,
            Path::new("non-esiste.exe"),
            Path::new("."),
            Duration::from_millis(1),
            Duration::from_millis(1),
        );
        assert!(ok);
        assert_eq!(*calls.borrow(), 1); // un solo tentativo, nessun avvio.
    }

    #[test]
    fn ensure_orchestrator_rifiuta_se_autostart_disattivo() {
        let connect = || Some("timeout".to_string());
        let ok = ensure_orchestrator(
            &connect,
            false,
            Path::new("non-esiste.exe"),
            Path::new("."),
            Duration::from_millis(1),
            Duration::from_millis(1),
        );
        assert!(!ok);
    }

    #[test]
    fn ensure_orchestrator_rifiuta_se_leseguibile_non_esiste() {
        let connect = || Some("timeout".to_string());
        let ok = ensure_orchestrator(
            &connect,
            true,
            Path::new("sicuramente-non-esiste.exe"),
            Path::new("."),
            Duration::from_millis(1),
            Duration::from_millis(1),
        );
        assert!(!ok);
    }
}
