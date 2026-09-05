//! Tipi base dei peer: identità (`PeerId` = IPv4), info annunciate (`PeerInfo`)
//! e l'insieme dei membri presenti (`Roster`). Tutto puro, senza I/O.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::Ipv4Addr;

/// Identità di un peer sulla LAN: il suo indirizzo IPv4.
/// Univoco per macchina e con **ordine totale** numerico (deriva `Ord` da `Ipv4Addr`)
/// — è ciò che rende l'elezione deterministica senza scambiare messaggi.
/// `serde` serializza il newtype in modo trasparente come stringa IP ("192.168.1.12").
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PeerId(pub Ipv4Addr);

/// Informazioni che un peer annuncia di sé: chi è e su quale porta TCP ascolta la chat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerInfo {
    pub id: PeerId,
    pub label_base: String,
    pub chat_port: u16,
}

/// Insieme dei membri presenti, indicizzati per `PeerId`.
/// Usa `BTreeMap` perché tiene le chiavi **ordinate**: `lowest()` è semplicemente
/// il primo elemento, cioè l'IP più basso → il server eletto.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Roster {
    members: BTreeMap<PeerId, PeerInfo>,
}

impl Roster {
    pub fn new() -> Self {
        Self::default()
    }
    /// Inserisce o aggiorna un membro (idempotente sull'id).
    pub fn upsert(&mut self, info: PeerInfo) {
        self.members.insert(info.id, info);
    }
    pub fn remove(&mut self, id: PeerId) {
        self.members.remove(&id);
    }
    pub fn contains(&self, id: PeerId) -> bool {
        self.members.contains_key(&id)
    }
    /// Iteratore sugli id in ordine numerico crescente.
    pub fn ids(&self) -> impl Iterator<Item = PeerId> + '_ {
        self.members.keys().copied()
    }
    /// L'IP più basso presente (= il server eletto), o `None` se vuoto.
    pub fn lowest(&self) -> Option<PeerId> {
        self.members.keys().next().copied()
    }
    pub fn get(&self, id: PeerId) -> Option<&PeerInfo> {
        self.members.get(&id)
    }
    pub fn len(&self) -> usize {
        self.members.len()
    }
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pid(a: u8, b: u8, c: u8, d: u8) -> PeerId {
        PeerId(Ipv4Addr::new(a, b, c, d))
    }
    fn info(id: PeerId, base: &str) -> PeerInfo {
        PeerInfo { id, label_base: base.to_string(), chat_port: 40100 }
    }

    #[test]
    fn peerid_orders_by_numeric_ip() {
        // 192.168.1.12 < 192.168.1.200 come numero, non come stringa.
        assert!(pid(192, 168, 1, 12) < pid(192, 168, 1, 200));
    }

    #[test]
    fn roster_upsert_contains_and_len() {
        let mut r = Roster::new();
        assert!(r.is_empty());
        r.upsert(info(pid(192, 168, 1, 50), "skimble"));
        r.upsert(info(pid(192, 168, 1, 10), "macavity"));
        assert_eq!(r.len(), 2);
        assert!(r.contains(pid(192, 168, 1, 50)));
        assert!(!r.contains(pid(10, 0, 0, 1)));
    }

    #[test]
    fn roster_lowest_is_numerically_smallest_ip() {
        let mut r = Roster::new();
        r.upsert(info(pid(192, 168, 1, 50), "a"));
        r.upsert(info(pid(192, 168, 1, 10), "b"));
        r.upsert(info(pid(192, 168, 1, 200), "c"));
        assert_eq!(r.lowest(), Some(pid(192, 168, 1, 10)));
    }

    #[test]
    fn roster_remove_and_empty_lowest() {
        let mut r = Roster::new();
        r.upsert(info(pid(192, 168, 1, 10), "b"));
        r.remove(pid(192, 168, 1, 10));
        assert!(r.is_empty());
        assert_eq!(r.lowest(), None);
    }

    #[test]
    fn roster_get_returns_info() {
        let mut r = Roster::new();
        r.upsert(info(pid(192, 168, 1, 10), "b"));
        assert_eq!(r.get(pid(192, 168, 1, 10)).map(|i| i.label_base.as_str()), Some("b"));
    }
}
