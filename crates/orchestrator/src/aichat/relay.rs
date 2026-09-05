//! Relay a stella: quando il SERVER riceve un messaggio dal peer `from`, lo inoltra a
//! tutti gli altri tranne `from` (l'autore, che già lo conosce) e `me` (il server, che
//! l'ha appena ricevuto). Con 2 peer → 0 target (entrambi già sanno).

use crate::aichat::peer::{PeerId, PeerInfo, Roster};
use std::collections::HashMap;

/// Trova il `PeerId` del peer il cui `label_base` combacia con `label_base` — usato per
/// instradare un messaggio verso UNA macchina specifica per nome (Library "Share with",
/// vedi `Docs/superpowers/specs/2026-07-02-library-share-with-design.md` §4.1), a
/// differenza di `fan_out_targets` (broadcast a tutti tranne autore+server).
///
/// `peers` non contiene mai la macchina locale (`self.peers` nel chiamante reale è
/// popolato solo da `Discovered`, mai dalla propria identità) — quindi un `label_base`
/// che combacia con se stessi risulta in `None` esattamente come un `label_base`
/// sconosciuto, senza bisogno di un controllo esplicito qui o nel chiamante.
pub fn route_to_label(label_base: &str, peers: &HashMap<PeerId, PeerInfo>) -> Option<PeerId> {
    peers.values().find(|p| p.label_base == label_base).map(|p| p.id)
}

/// Destinatari dell'inoltro di un messaggio ricevuto dal server.
/// Esclude l'autore (`from`) e il server stesso (`me`).
///
/// Topologia a stella: il server è il centro, ogni messaggio ricevuto
/// va ritrasmesso a tutti i rami tranne quello da cui è arrivato.
/// Se `from == me` (il server parla), nessuna esclusione "autore"
/// aggiuntiva: tutti i peer diversi dal server stesso ricevono.
pub fn fan_out_targets(from: PeerId, me: PeerId, members: &Roster) -> Vec<PeerId> {
    members.ids().filter(|&id| id != from && id != me).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aichat::peer::PeerInfo;
    use std::net::Ipv4Addr;

    fn pid(d: u8) -> PeerId {
        PeerId(Ipv4Addr::new(192, 168, 1, d))
    }
    fn roster(ds: &[u8]) -> Roster {
        let mut r = Roster::new();
        for &d in ds {
            r.upsert(PeerInfo { id: pid(d), label_base: "x".into(), chat_port: 40100 });
        }
        r
    }

    #[test]
    fn two_peers_no_rebound() {
        // server = .10 (me), client = .20 (from). Nessun terzo → nessun inoltro.
        let targets = fan_out_targets(pid(20), pid(10), &roster(&[10, 20]));
        assert!(targets.is_empty());
    }

    #[test]
    fn three_peers_message_from_client_goes_to_the_other_client() {
        // server = .10 (me); from = .20; resta solo .30 come destinatario.
        let targets = fan_out_targets(pid(20), pid(10), &roster(&[10, 20, 30]));
        assert_eq!(targets, vec![pid(30)]);
    }

    #[test]
    fn message_from_server_goes_to_all_others() {
        // Il server stesso parla (from == me): inoltra a tutti i client (n-1).
        let mut targets = fan_out_targets(pid(10), pid(10), &roster(&[10, 20, 30]));
        targets.sort();
        assert_eq!(targets, vec![pid(20), pid(30)]);
    }

    // === route_to_label tests ===

    use std::collections::HashMap;

    fn peers_map(entries: &[(u8, &str)]) -> HashMap<PeerId, PeerInfo> {
        entries
            .iter()
            .map(|&(d, base)| {
                let id = pid(d);
                (id, PeerInfo { id, label_base: base.into(), chat_port: 40100 })
            })
            .collect()
    }

    #[test]
    fn route_to_label_finds_matching_peer() {
        let peers = peers_map(&[(20, "skimble"), (30, "quaxo")]);
        assert_eq!(route_to_label("quaxo", &peers), Some(pid(30)));
    }

    #[test]
    fn route_to_label_none_for_unknown_label() {
        let peers = peers_map(&[(20, "skimble")]);
        assert_eq!(route_to_label("nobody", &peers), None);
    }

    #[test]
    fn route_to_label_none_for_own_label_since_self_is_never_in_peers() {
        // `self.peers` (nel chiamante reale) non contiene mai la macchina locale — questo
        // test lo simula con una mappa che semplicemente non include "rumpleteazer": un
        // tentativo di condividere con se stessi si comporta come "macchina non trovata",
        // senza bisogno di un controllo dedicato nel chiamante.
        let peers = peers_map(&[(20, "skimble")]);
        assert_eq!(route_to_label("rumpleteazer", &peers), None);
    }
}
