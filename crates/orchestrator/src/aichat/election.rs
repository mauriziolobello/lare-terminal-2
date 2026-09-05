//! Elezione del server: deterministica e **sticky**.
//! Regola: vince l'IP più basso. Sticky: chi è già server resta server finché è
//! presente (niente prelazione da un IP più basso che arriva dopo); si rielegge solo
//! se il server attuale sparisce. Motivo: in una stella, cambiare hub = rifare tutti i
//! link → costoso; lo sticky minimizza il churn.

use crate::aichat::peer::{PeerId, Roster};

/// Elegge il server.
/// - `current`: il server creduto attualmente da QUESTO nodo (sticky, invariato).
///   Se `current` è ancora un membro → resta (sticky, niente prelazione).
/// - `reported`: il leader che un peer scoperto ha annunciato di credere valido
///   (nuovo — vedi Bug 2, late-joiner). Serve a far sì che un nodo appena avviato
///   (che parte sempre con `current = None`) non ricalcoli l'elezione da zero
///   quando il gruppo ha già un server stabilito altrove:
///   - se `reported` è già nel nostro roster (consentito) → lo adottiamo, non
///     vince il solo IP più basso;
///   - se `reported` è noto ma non ancora nel nostro roster (consenso umano non
///     ancora dato) → NON ci autoeleggiamo: restiamo `Undecided` (`None`) finché
///     non lo raggiungiamo o finché il segnale sparisce (vedi `PeerGone` in
///     `service.rs`);
///   - se `reported` è `None` → nessun leader noto da nessuna parte, elezione
///     fondativa invariata (`members.lowest()`).
pub fn elect(members: &Roster, current: Option<PeerId>, reported: Option<PeerId>) -> Option<PeerId> {
    match current {
        Some(c) if members.contains(c) => Some(c),
        _ => match reported {
            Some(r) if members.contains(r) => Some(r),
            Some(_) => None,
            None => members.lowest(),
        },
    }
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
    fn elects_lowest_ip_when_no_current() {
        assert_eq!(elect(&roster(&[50, 10, 200]), None, None), Some(pid(10)));
    }

    #[test]
    fn single_member_elects_itself() {
        assert_eq!(elect(&roster(&[42]), None, None), Some(pid(42)));
    }

    #[test]
    fn empty_roster_has_no_leader() {
        assert_eq!(elect(&roster(&[]), None, None), None);
    }

    #[test]
    fn sticky_current_is_not_preempted_by_lower_ip() {
        // current = .50 (era il più basso alla formazione); ora entra .10 (più basso).
        // Sticky: NON deve spodestare .50.
        let members = roster(&[50, 10]);
        assert_eq!(elect(&members, Some(pid(50)), None), Some(pid(50)));
    }

    #[test]
    fn re_elects_lowest_when_current_disappeared() {
        // current = .10 ma non è più nel roster → si rielegge il più basso presente.
        let members = roster(&[50, 200]);
        assert_eq!(elect(&members, Some(pid(10)), None), Some(pid(50)));
    }

    // -------------------------------------------------------------------------
    // Bug 2 (late-joiner): terzo parametro `reported` — vedi
    // Docs/superpowers/specs/2026-07-02-aichat-late-joiner-election-design.md §3.1
    // -------------------------------------------------------------------------

    #[test]
    fn reported_leader_present_in_roster_wins_over_lowest_ip() {
        // `me` (chiamante) vedrebbe .10 come IP più basso, ma un peer scoperto ha
        // riportato che il leader creduto è .200 — e .200 È nel nostro roster
        // (consentito). Il leader riportato vince: NON dobbiamo autoeleggerci/rieleggere
        // in base al solo IP più basso.
        let members = roster(&[10, 50, 200]);
        assert_eq!(elect(&members, None, Some(pid(200))), Some(pid(200)));
    }

    #[test]
    fn reported_leader_absent_from_roster_yields_undecided() {
        // Un peer scoperto riporta un leader (.200) che NON è ancora nel nostro
        // roster (consenso umano non ancora dato) → non ricadiamo su `lowest()`,
        // restiamo in attesa (`None` = Undecided).
        let members = roster(&[10, 50]);
        assert_eq!(elect(&members, None, Some(pid(200))), None);
    }

    #[test]
    fn no_reported_leader_behaves_like_founding_election() {
        // `reported: None` → comportamento identico a prima del fix (elezione
        // fondativa sul roster visibile, IP più basso vince).
        assert_eq!(elect(&roster(&[50, 10, 200]), None, None), Some(pid(10)));
    }
}
