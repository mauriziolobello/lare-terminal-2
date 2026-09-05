//! `Room`: la lista dei partecipanti per ETICHETTA (es. "skimble-human"). Distinta dal
//! `Roster` dei peer (che è per indirizzo IP). Il server la possiede e ne pubblica lo
//! snapshot come `ChatMsg::Roster`. Pura e ordinata (BTreeSet) per output deterministico.

use crate::aichat::wire::ChatMsg;
use std::collections::BTreeSet;

/// Lista dei partecipanti presenti, per etichetta. `BTreeSet` → ordine stabile.
#[derive(Debug, Default)]
pub struct Room {
    participants: BTreeSet<String>,
}

impl Room {
    pub fn new() -> Self {
        Self::default()
    }
    /// Aggiunge un partecipante (idempotente) e ritorna lo snapshot del roster da pubblicare.
    pub fn join(&mut self, label: String) -> ChatMsg {
        self.participants.insert(label);
        self.roster_msg()
    }
    /// Rimuove un partecipante e ritorna lo snapshot aggiornato.
    pub fn leave(&mut self, label: &str) -> ChatMsg {
        self.participants.remove(label);
        self.roster_msg()
    }
    /// Restituisce una copia ordinata dei presenti (stabile: alfabetico via BTreeSet).
    pub fn participants(&self) -> Vec<String> {
        self.participants.iter().cloned().collect()
    }
    /// Snapshot corrente come `ChatMsg::Roster { participants }`.
    pub fn roster_msg(&self) -> ChatMsg {
        ChatMsg::Roster { participants: self.participants() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_adds_and_returns_roster_msg() {
        let mut room = Room::new();
        let msg = room.join("skimble-human".into());
        assert_eq!(msg, ChatMsg::Roster { participants: vec!["skimble-human".into()] });
    }

    #[test]
    fn join_is_idempotent_and_sorted() {
        let mut room = Room::new();
        room.join("b-human".into());
        room.join("a-human".into());
        room.join("b-human".into()); // duplicato → ignorato
        assert_eq!(room.participants(), vec!["a-human".to_string(), "b-human".to_string()]);
    }

    #[test]
    fn leave_removes_and_returns_updated_roster() {
        let mut room = Room::new();
        room.join("a".into());
        room.join("b".into());
        let msg = room.leave("a");
        assert_eq!(msg, ChatMsg::Roster { participants: vec!["b".into()] });
    }
}
