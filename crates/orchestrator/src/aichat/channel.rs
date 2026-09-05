//! `AiChatChannel`: orchestratore del canale. Decide il proprio ruolo (server/client)
//! con `elect`, e — da server — gestisce il Room e il relay. I metodi ritornano le
//! AZIONI da compiere (quali ChatMsg, a quali peer): l'I/O reale sui PeerLink è in Task 10.
//! Così la logica di stanza si testa in isolamento, come per il canale Telegram.

use crate::aichat::election::elect;
use crate::aichat::peer::{PeerId, PeerInfo, Roster};
use crate::aichat::relay::fan_out_targets;
use crate::aichat::room::Room;
use crate::aichat::wire::ChatMsg;

/// Ruolo del peer nella stanza.
///
/// In Rust gli enum possono contenere dati (simile alle "sealed class" di Kotlin o alle
/// "discriminated union" di F#). Qui `Client` trasporta l'id del server corrente.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Non ancora deciso (nessun membro noto).
    Undecided,
    /// Io sono il server (relay hub).
    Server,
    /// Sono un client; il server è il peer indicato.
    Client(PeerId),
}

/// Orchestratore del canale (lato logica, senza I/O).
///
/// Incapsula tre preoccupazioni (SRP per campo):
/// - `me`: identità propria (immutabile dopo la creazione).
/// - `role`: ruolo sticky nel cluster.
/// - `room`: lista dei partecipanti per etichetta (usata solo da server).
///
/// Il pattern "ritorna le azioni, non eseguirle" separa la logica pura dall'I/O
/// (i `PeerLink` veri arrivano in Task 10). Rende il tutto testabile senza socket.
pub struct AiChatChannel {
    me: PeerInfo,
    role: Role,
    /// Roster dei partecipanti per etichetta — popolato solo quando sono server.
    room: Room,
}

impl AiChatChannel {
    /// Crea un nuovo canale per il peer `me`, inizialmente senza ruolo.
    pub fn new(me: PeerInfo) -> Self {
        Self { me, role: Role::Undecided, room: Room::new() }
    }

    /// Restituisce il ruolo corrente (Copy → nessun clone necessario).
    pub fn role(&self) -> Role {
        self.role
    }

    /// Ricalcola il ruolo dato il roster di peer corrente, in modo **sticky**:
    /// se ero già server/client di un peer ancora presente, non cambio.
    ///
    /// La sticky-ness vive in `elect()`: passa il server corrente come `current`,
    /// e `elect` lo conserva se è ancora nel roster (niente prelazione da IP più basso).
    ///
    /// `reported`: il leader che un peer scoperto ha annunciato di credere valido
    /// (vedi Bug 2 late-joiner, `election::elect`). Passato tale e quale a `elect`:
    /// se non `None` e già dentro `members`, vince sul solo IP più basso; se `None`,
    /// comportamento invariato (elezione fondativa).
    pub fn decide_role(&mut self, members: &Roster, reported: Option<PeerId>) -> Role {
        // Il canale È esso stesso un membro della stanza. La sorgente di scoperta
        // (UdpDiscoverer) filtra i PROPRI annunci, quindi `members` può non contenere
        // `me`; lo includiamo qui così l'elezione considera l'intera membership e un
        // nodo con IP più basso può eleggere se stesso come server.
        // `upsert` è idempotente: se `me` è già in `members` (roster costruito a mano
        // nei test) non ha effetto → i test esistenti restano verdi.
        let mut full = members.clone();
        full.upsert(self.me.clone());

        // Il "current" passato a `elect` è l'attuale server creduto (me se Server,
        // il peer puntato se Client, None se Undecided).
        let current = match self.role {
            Role::Server => Some(self.me.id),
            Role::Client(server) => Some(server),
            Role::Undecided => None,
        };
        // `elect` ritorna il leader eletto (o None se roster vuoto — impossibile ora
        // che `full` contiene almeno `me`, ma gestiamo il caso per completezza).
        // Mappiamo il risultato in un `Role`.
        self.role = match elect(&full, current, reported) {
            Some(leader) if leader == self.me.id => Role::Server,
            Some(leader) => Role::Client(leader),
            None => Role::Undecided,
        };
        self.role
    }

    /// Da SERVER: un partecipante entra. Aggiorna il Room e ritorna
    /// (il `ChatMsg::Roster` da broadcastare, la lista presenti).
    ///
    /// Il chiamante (Task 10) userà il `ChatMsg` per inviarlo a tutti i peer.
    pub fn on_server_join(&mut self, label: String) -> (ChatMsg, Vec<String>) {
        // `room.join` è idempotente e ritorna già lo snapshot come ChatMsg::Roster.
        let msg = self.room.join(label);
        (msg, self.room.participants())
    }

    /// Da SERVER: un partecipante esce (keepalive scaduto o disconnessione pulita).
    /// Aggiorna il Room e ritorna (il `ChatMsg::Roster` aggiornato da broadcastare, la
    /// lista presenti). Mirror di `on_server_join`; idempotente su un'etichetta assente
    /// (come `Room::leave`, che con `BTreeSet::remove` su chiave assente è un no-op).
    pub fn on_server_leave(&mut self, label: &str) -> (ChatMsg, Vec<String>) {
        let msg = self.room.leave(label);
        (msg, self.room.participants())
    }

    /// Da SERVER: inoltra `msg` ricevuto da `from` ai destinatari `n-2`.
    /// Ritorna le coppie (peer destinatario, messaggio da inviargli).
    ///
    /// `fan_out_targets` esclude l'autore (`from`) e il server (`me`):
    /// con 3 peer {.10=server, .20, .30}, messaggio da .20 → solo .30 riceve.
    pub fn server_relay(&self, from: PeerId, msg: ChatMsg, members: &Roster) -> Vec<(PeerId, ChatMsg)> {
        fan_out_targets(from, self.me.id, members)
            .into_iter()
            // `msg.clone()` perché ogni destinatario riceve una copia indipendente.
            .map(|target| (target, msg.clone()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn info(d: u8) -> PeerInfo {
        PeerInfo { id: PeerId(Ipv4Addr::new(192, 168, 1, d)), label_base: format!("m{d}"), chat_port: 40100 }
    }
    fn roster(ds: &[u8]) -> Roster {
        let mut r = Roster::new();
        for &d in ds {
            r.upsert(info(d));
        }
        r
    }

    #[test]
    fn lowest_ip_decides_server_role_for_itself() {
        let mut ch = AiChatChannel::new(info(10));
        assert_eq!(ch.decide_role(&roster(&[10, 20, 30]), None), Role::Server);
        assert_eq!(ch.role(), Role::Server);
    }

    #[test]
    fn higher_ip_decides_client_role_pointing_at_lowest() {
        let mut ch = AiChatChannel::new(info(30));
        assert_eq!(ch.decide_role(&roster(&[10, 20, 30]), None), Role::Client(info(10).id));
    }

    #[test]
    fn role_is_sticky_across_a_later_lower_ip() {
        let mut ch = AiChatChannel::new(info(20));
        // formazione: {20,30} → 20 è il più basso → server.
        assert_eq!(ch.decide_role(&roster(&[20, 30]), None), Role::Server);
        // poi entra 10 (più basso): sticky → 20 resta server.
        assert_eq!(ch.decide_role(&roster(&[10, 20, 30]), None), Role::Server);
    }

    #[test]
    fn server_join_updates_room_and_returns_roster_broadcast() {
        let mut ch = AiChatChannel::new(info(10));
        ch.decide_role(&roster(&[10]), None);
        let (msg, present) = ch.on_server_join("skimble-human".into());
        assert_eq!(msg, ChatMsg::Roster { participants: vec!["skimble-human".into()] });
        assert_eq!(present, vec!["skimble-human".to_string()]);
    }

    #[test]
    fn server_leave_updates_room_and_returns_roster_broadcast() {
        let mut ch = AiChatChannel::new(info(10));
        ch.decide_role(&roster(&[10]), None);
        ch.on_server_join("skimble-human".into());
        ch.on_server_join("quaxo-human".into());

        let (msg, present) = ch.on_server_leave("skimble-human");

        assert_eq!(msg, ChatMsg::Roster { participants: vec!["quaxo-human".into()] });
        assert_eq!(present, vec!["quaxo-human".to_string()]);
    }

    #[test]
    fn server_leave_on_absent_label_is_idempotent() {
        let mut ch = AiChatChannel::new(info(10));
        ch.decide_role(&roster(&[10]), None);
        ch.on_server_join("skimble-human".into());

        let (msg, present) = ch.on_server_leave("mai-entrato-human");

        assert_eq!(msg, ChatMsg::Roster { participants: vec!["skimble-human".into()] });
        assert_eq!(present, vec!["skimble-human".to_string()]);
    }

    #[test]
    fn server_relay_forwards_say_to_n_minus_2() {
        let mut ch = AiChatChannel::new(info(10)); // server = me
        ch.decide_role(&roster(&[10, 20, 30]), None);
        let say = ChatMsg::Say { from_label: "m20-human".into(), text: "ciao".into(), display_name: None, is_ai: false };
        let out = ch.server_relay(info(20).id, say.clone(), &roster(&[10, 20, 30]));
        // da .20, escluso server (.10) e autore (.20) → solo .30.
        assert_eq!(out, vec![(info(30).id, say)]);
    }

    #[test]
    fn lowest_ip_elects_self_even_when_roster_excludes_self() {
        // Simula la sorgente reale: PeerTable popolata SOLO dagli altri peer
        // (UdpDiscoverer filtra i propri annunci). me = .10 (IP più basso).
        // Prima del fix, decide_role non includeva `me` nell'elezione → il
        // peer più basso tra gli altri (.20) veniva eletto, e .10 si dichiarava
        // Client invece di Server.
        use crate::aichat::discovery::PeerTable;
        let mut table = PeerTable::new(1000);
        table.observe(info(20), 0);
        table.observe(info(30), 0);
        let roster = table.roster(); // NON contiene .10 (me): simula UdpDiscoverer
        let mut ch = AiChatChannel::new(info(10));
        assert_eq!(ch.decide_role(&roster, None), Role::Server);
    }

    // -------------------------------------------------------------------------
    // Bug 2 (late-joiner): `reported` impedisce l'autoelezione di un terzo peer
    // che si unisce dopo che un server è già stato eletto altrove. Riproduce lo
    // scenario esatto del log (quaxo, IP più basso di tutti, entra dopo che
    // rumpleteazer↔skimble si sono già eletti) — vedi §3.2 dello spec.
    // -------------------------------------------------------------------------

    #[test]
    fn reported_leader_prevents_self_election_even_with_lowest_ip() {
        // roster a 3: me = .10 (IP più basso di TUTTI — vincerebbe una founding
        // election classica), ma un peer scoperto ha riportato che il leader
        // creduto è .20 (già presente nel roster, cioè già consentito da noi).
        // Deve risultare Client(.20), MAI Server.
        let mut ch = AiChatChannel::new(info(10));
        let members = roster(&[10, 20, 30]);
        assert_eq!(ch.decide_role(&members, Some(info(20).id)), Role::Client(info(20).id));
        assert_ne!(ch.role(), Role::Server, "non deve mai autoeleggersi con un leader già riportato");
    }
}
