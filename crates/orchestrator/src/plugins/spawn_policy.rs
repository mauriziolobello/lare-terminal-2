//! Spawn policy: dato un set di `Triggers` dichiarati nel manifest, decide se il plugin
//! deve essere spawnato subito all'avvio dell'orchestrator (Eager) oppure solo al primo
//! comando che lo coinvolge (Lazy).
//!
//! # Regola
//! | Triggers            | Policy |
//! |---------------------|--------|
//! | `interval` presente | Eager  |  deve ricevere `OnTimer` -> deve essere vivo in anticipo
//! | solo `command`      | Lazy   |  si spawna al 1° `Activate` (Fase 1)
//! | nessun trigger      | Eager  |  degenere (es. plugin-ping di Fase 0) -> eager per semplicità
//!
//! Questa funzione è PURA (niente I/O, niente stato globale) — la rende banalmente testabile
//! con semplici `assert_eq!` senza mock.
use plugin_protocol::Triggers;

/// Politica di spawn per un plugin.
///
/// `Eager` = il plugin viene spawnato immediatamente all'avvio dell'orchestrator e rimane
/// in vita fino allo shutdown. `Lazy` = il plugin viene spawnato al primo `Activate`
/// (Fase 1: non ancora implementato).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnPolicy {
    /// Spawn all'avvio: necessario per plugin con `interval` (timer) o senza trigger noti.
    Eager,
    /// Spawn al primo comando: adatto a plugin command-only (es. /calc), risparmia risorse.
    Lazy,
}

/// Deriva la `SpawnPolicy` dai trigger dichiarati nel manifest.
///
/// La funzione è intenzionalmente piccola e pura: non legge env né config.
/// La logica di scheduling reale (timer, comandi) vive nel `PluginHost` (Task 5).
pub fn spawn_policy(t: &Triggers) -> SpawnPolicy {
    if t.interval.is_some() {
        // Il plugin ha un timer: deve essere già vivo quando l'orchestrator scatta OnTimer.
        SpawnPolicy::Eager
    } else if t.command.is_some() {
        // Solo comando, niente timer: si può rimandare al primo utilizzo.
        SpawnPolicy::Lazy
    } else {
        // Nessun trigger (degenere): spawn eager per semplicità (es. plugin ping di Fase 0).
        SpawnPolicy::Eager
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plugin_protocol::Triggers;

    /// Helper: costruisce un `Triggers` dai valori opzionali senza chiamare il costruttore
    /// ripetutamente — rende le righe di test molto più compatte.
    fn t(cmd: Option<&str>, int: Option<&str>) -> Triggers {
        Triggers {
            command: cmd.map(String::from),
            interval: int.map(String::from),
        }
    }

    #[test]
    fn interval_is_eager() {
        assert_eq!(spawn_policy(&t(None, Some("5m"))), SpawnPolicy::Eager);
    }

    #[test]
    fn command_only_is_lazy() {
        assert_eq!(spawn_policy(&t(Some("/calc"), None)), SpawnPolicy::Lazy);
    }

    #[test]
    fn both_is_eager() {
        // command + interval -> eager: il plugin deve essere vivo per ricevere OnTimer.
        assert_eq!(spawn_policy(&t(Some("/agenda"), Some("1m"))), SpawnPolicy::Eager);
    }

    #[test]
    fn none_is_eager() {
        // degenere (nessun trigger, es. ping) -> eager
        assert_eq!(spawn_policy(&t(None, None)), SpawnPolicy::Eager);
    }
}
