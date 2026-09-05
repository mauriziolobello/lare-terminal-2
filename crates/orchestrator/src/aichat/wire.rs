//! Wire protocol peer↔peer ("Contratto N"), JSON-per-riga su TCP, e l'`Announce` UDP.
//! Versionato e additivo (come `protocol`). Tag `"type"` in snake_case.
//!
//! # Contratto N — regole additive
//! - Ogni messaggio è una riga JSON (`\n`-terminated) sulla connessione TCP.
//! - Nuove varianti si aggiungono; quelle esistenti non cambiano struttura.
//! - Il campo `"type"` (tag) identifica la variante; usa `snake_case`.
//! - L'`Announce` viaggia via UDP broadcast e non ha tag: è un datagramma autonomo.

use crate::aichat::peer::PeerId;
use crate::notes::{digest::NoteDigest, Note};
use serde::{Deserialize, Serialize};

/// Una riga di storico: chi l'ha scritta e cosa. Riusata sia nel dump di storico peer↔peer
/// (`ChatMsg::History`, Task 5) sia come elemento dello storico locale in
/// `AiChatService::history` (vedi `service.rs`, Task 3). Distinta dall'omonima
/// `protocol::ChatLine` usata sul WS orchestrator↔UI — stessa forma, stessa scelta già in uso
/// per `ChatMsg::Roster`/`ServerMsg::AiChatRoster` (due tipi identici in crate distinti,
/// nessuna dipendenza incrociata: `protocol` non dipende da `orchestrator`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatLine {
    pub from_label: String,
    pub text: String,
    /// Nickname risolto dal mittente al momento dell'invio (umano o AI, in base
    /// a chi ha scritto). `None` → il frontend mostra `from_label` come oggi
    /// (fallback, comportamento invariato per peer/storico vecchi). Additivo:
    /// gemello di `protocol::ChatLine::display_name` — stessa semantica.
    #[serde(default)]
    pub display_name: Option<String>,
    /// True se il mittente è l'AI (non un umano) — pilota badge/icona nel
    /// frontend. `false` di default: un peer vecchio che non manda questo
    /// campo appare come umano. Gemello di `protocol::ChatLine::is_ai`.
    #[serde(default)]
    pub is_ai: bool,
}

/// Conversione verso il tipo omonimo di `protocol`, usata al confine WS orchestrator↔UI
/// (`Effect::ToUi(ServerMsg::AiChatHistory{..})` in `service.rs`, Task 4/5).
impl From<ChatLine> for protocol::ChatLine {
    fn from(line: ChatLine) -> Self {
        protocol::ChatLine {
            from_label: line.from_label,
            text: line.text,
            display_name: line.display_name,
            is_ai: line.is_ai,
        }
    }
}

/// Messaggi scambiati tra peer sulla connessione TCP (una riga JSON ciascuno).
///
/// Il tag `"type"` in `snake_case` identifica la variante:
/// `"join"`, `"say"`, `"roster"`, `"leave"`, `"history"`, `"ping"`, `"peer_lost"`,
/// `"request_admission"`, `"admission_vote_request"`, `"admission_vote"`, `"admitted"`,
/// `"admission_rejected"`, `"admission_resolved"`,
/// `"share_offer"`, `"share_accept"`, `"share_data"`, `"share_reject"`, `"share_expired"`,
/// `"notes_digest"`, `"notes_digest_reply"`, `"notes_data"`, `"note_updated"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatMsg {
    /// Un partecipante entra nella stanza (dopo il consenso umano).
    Join { label: String },
    /// Un messaggio di chat. L'attribuzione (`from_label`) viaggia NEL messaggio:
    /// il ricevente non la inventa mai.
    Say {
        from_label: String,
        text: String,
        /// Vedi doc-comment su `ChatLine::display_name` — stessa semantica.
        #[serde(default)]
        display_name: Option<String>,
        /// Vedi doc-comment su `ChatLine::is_ai` — stessa semantica.
        #[serde(default)]
        is_ai: bool,
    },
    /// Il server pubblica la lista autorevole dei presenti.
    Roster { participants: Vec<String> },
    /// Un partecipante esce (superficie chiusa).
    Leave { label: String },
    /// Dump dello storico completo, mandato dal SERVER a un peer che ha appena mandato
    /// `Join` (nuovo arrivo o riconnessione/riavvio). Non è un broadcast: va solo al peer
    /// appena entrato, che sostituisce il proprio storico locale (vedi `service.rs`).
    History { entries: Vec<ChatLine> },
    /// Battito cardiaco (keepalive). Nessun payload: il solo fatto di arrivare resetta il
    /// timeout di silenzio sul reader del mittente (vedi `service.rs::spawn_peer_tasks`).
    /// Non triggera nessuna azione applicativa oltre al reset del timeout.
    Ping {},
    /// (Solo server→client) Un client è sparito (keepalive scaduto o disconnessione pulita).
    /// Broadcast a tutti i client rimasti connessi — MAI mandato al peer sparito stesso.
    PeerLost { label: String },

    // ── Library "Share with" (Slice 1a — additive) ────────────────────────────
    // Vedi Docs/superpowers/specs/2026-07-02-library-share-with-design.md §4.
    // `from_label`/`doc_name` sono `label_base` NUDI e nomi-file, mai un percorso: il
    // ricevente non deve mai fidarsi ciecamente di `doc_name` per costruire un path
    // (verrà sanificato quando si costruirà un percorso su disco — Slice 2).
    /// (mittente→destinatario) Offerta di condivisione — SOLO metadati, nessun
    /// contenuto. `share_id` correla Offer→Accept/Reject/Expired.
    ShareOffer { share_id: String, from_label: String, doc_name: String, size_bytes: u64 },
    /// (destinatario→mittente) Accetta — il mittente potrà mandare il contenuto
    /// (`ChatMsg::ShareData` sotto).
    ShareAccept { share_id: String, from_label: String },
    /// (mittente→destinatario, Slice 2a) Il contenuto vero, mandato SOLO dopo un
    /// `ShareAccept` per lo stesso `share_id`. `title`+`content` invece di un blob:
    /// riusa la stessa forma di `ArchiveDoc` (crate `ui`, `archive.rs`) — il destinatario
    /// lo scrive con l'equivalente di `archive::save`.
    ShareData { share_id: String, title: String, content: String },
    /// (destinatario→mittente) Rifiuta — nessun effetto, il mittente non manda nulla.
    /// Usato sia per un rifiuto esplicito sia per l'auto-rifiuto al cap di trasferimenti
    /// pendenti (§8 dello spec) — nessun campo `reason`.
    ShareReject { share_id: String, from_label: String },
    /// (destinatario→mittente) Il trasferimento è scaduto (24h da `ShareOffer` senza che
    /// il documento risultasse consegnato) — vedi spec §9.1.
    ShareExpired { share_id: String },

    // ── Ammissione alla stanza (voto coordinato dal server) ───────────────────
    // Sostituisce il vecchio consenso pairwise (`Join` immediato dopo un `Consent`
    // locale): ora il SERVER ELETTO coordina un voto tra i presenti prima di far
    // entrare un nuovo arrivato. Vedi
    // Docs/superpowers/specs/2026-07-03-aichat-admission-consent-design.md.
    /// (client→server) Il nuovo arrivato chiede di entrare, dopo il gate 1 (consenso proprio,
    /// gestito lato UI: "vuoi entrare con [presenti]?"). `label` è l'etichetta del candidato
    /// (es. "cand-human"), non un `PeerId`: il server la userà come chiave umana-leggibile
    /// per gli inviti di voto (`AdmissionVoteRequest`) e per il broadcast finale.
    RequestAdmission { label: String },
    /// (server→client presente) "Ammetti questo candidato? sì/no" (gate 2). Il server manda
    /// questo a OGNI presente già ammesso nella stanza quando riceve un `RequestAdmission`.
    AdmissionVoteRequest { candidate: String },
    /// (client presente→server) Il voto di un presente sul candidato. `accept: false` è un
    /// VETO immediato (basta un solo no per rifiutare); `accept: true` conta come un sì fra
    /// i necessari per l'ammissione (AND logico fra tutti i presenti, silenzio-oltre-timeout
    /// conta come sì — vedi `service.rs`, Task 5).
    AdmissionVote { candidate: String, accept: bool },
    /// (server→tutti) Il candidato è stato ammesso (segue un `Roster` aggiornato che include
    /// la sua etichetta). Mandato sia al candidato stesso (per sbloccare la sua UI) sia in
    /// broadcast ai presenti (per la notifica "X è entrato").
    Admitted { label: String },
    /// (server→candidato) Ingresso rifiutato: un presente ha votato no (veto). Mandato SOLO
    /// al candidato — i presenti non ricevono conferma esplicita del rifiuto (solo il non-
    /// aggiornamento del roster). Il candidato può ri-chiedere dopo un cooldown (lato UI).
    AdmissionRejected { label: String },
    /// (server→presente, FIX #7 — review 2026-07-03) Il voto per `candidate` si è
    /// CONCLUSO: il presente remoto deve togliere il banner del gate 2 per quel
    /// candidato. Gemello "di chiusura" di `AdmissionVoteRequest` (che lo mostrava).
    /// Mandato SOLO ai presenti remoti (l'umano-server aggiorna la propria UI via
    /// `ServerMsg::AiChatAdmissionResolved` diretto, non da un ChatMsg di rete),
    /// mai al candidato (che riceve `Admitted`/`AdmissionRejected`).
    AdmissionResolved { candidate: String },

    // ── Library "Blocco note" (additiva) ──────────────────────────────────
    // Design: Docs/superpowers/specs/2026-07-29-library-notes-design.md §5-6.
    // Scambiato SOLO fra il link appena stabilito e chi lo riceve — mai un
    // broadcast a tutti (a differenza di `Say`/`PeerLost`).
    /// Impronte di tutte le note note al mittente, senza testo (id → digest).
    NotesDigest { entries: Vec<(String, NoteDigest)> },
    /// Risposta al digest ricevuto: `want` = id di cui serve il contenuto
    /// pieno; `push` = note intere che il mittente crede mancanti o più
    /// vecchie dal lato ricevente.
    NotesDigestReply { want: Vec<String>, push: Vec<Note> },
    /// Fulfillment di un `want` da una `NotesDigestReply` precedente.
    NotesData { notes: Vec<Note> },
    /// Steady-state (creazione/modifica/cancellazione con link vivo) E
    /// fan-out post-riconciliazione verso i client già collegati.
    NoteUpdated { note: Note },
}

/// Datagramma di scoperta UDP: "ci sono, sono questo peer, ascolto su questa porta".
///
/// Inviato periodicamente in broadcast sulla LAN (porta fissa `DISCOVERY_PORT`).
/// Ogni macchina che lo riceve aggiorna il proprio `Roster` e calcola chi è il server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Announce {
    /// Versione del protocollo di scoperta (per evoluzione additiva).
    pub v: u32,
    /// Identità del peer mittente: il suo IPv4 serializzato come stringa.
    pub id: PeerId,
    /// Nome base del peer (es. `"skimble"`) — il canale aggiunge il suffisso `-human` o `-ai`.
    pub label_base: String,
    /// Porta TCP su cui il peer ascolta le connessioni di chat.
    pub chat_port: u16,
    /// Il leader che il mittente crede attualmente valido (`None` se ancora `Undecided`).
    /// Nuovo campo (Bug 2, late-joiner): permette a un peer appena avviato di adottare
    /// un server già eletto altrove invece di ricalcolare l'elezione da zero sul solo
    /// IP più basso — vedi `election::elect` e `AiChatService::reported_leader`.
    /// `#[serde(default)]`: additivo, tollera annunci da un binario non ancora aggiornato
    /// (assenza del campo → `None`, non un errore di parsing).
    #[serde(default)]
    pub leader: Option<PeerId>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn chatmsg_say_roundtrip_and_tag() {
        let m = ChatMsg::Say {
            from_label: "skimble-human".into(),
            text: "ciao".into(),
            display_name: None,
            is_ai: false,
        };
        let j = serde_json::to_string(&m).unwrap();
        assert!(j.contains(r#""type":"say""#), "tag errato: {j}");
        assert_eq!(serde_json::from_str::<ChatMsg>(&j).unwrap(), m);
    }

    // ── TDD RED: display_name/is_ai additivi su wire::ChatLine/ChatMsg::Say ──
    // Scritti PRIMA di aggiungere i due campi nuovi. Prima dell'aggiunta producono
    // errori di compilazione (campi inesistenti) — quell'errore È la fase RED.
    // Vedi Docs/superpowers/plans/2026-08-13-aichat-display-names.md, Task 2.

    #[test]
    fn wire_say_defaults_display_name_none_and_is_ai_false_when_absent() {
        let json = r#"{"type":"say","from_label":"skimble-human","text":"ciao"}"#;
        let msg: ChatMsg = serde_json::from_str(json).unwrap();
        match msg {
            ChatMsg::Say { display_name, is_ai, .. } => {
                assert_eq!(display_name, None);
                assert!(!is_ai);
            }
            other => panic!("atteso Say, trovato {other:?}"),
        }
    }

    #[test]
    fn wire_chat_line_into_protocol_chat_line_carries_display_name_and_is_ai() {
        let wire_line = ChatLine {
            from_label: "skimble-ai".into(),
            text: "ciao".into(),
            display_name: Some("Aria".into()),
            is_ai: true,
        };
        let converted: protocol::ChatLine = wire_line.into();
        assert_eq!(converted.display_name, Some("Aria".into()));
        assert!(converted.is_ai);
    }

    #[test]
    fn chatmsg_join_leave_roster_roundtrip() {
        let msgs = [
            ChatMsg::Join { label: "macavity-human".into() },
            ChatMsg::Leave { label: "macavity-human".into() },
            ChatMsg::Roster { participants: vec!["a".into(), "b".into()] },
        ];
        for m in msgs {
            let j = serde_json::to_string(&m).unwrap();
            assert_eq!(serde_json::from_str::<ChatMsg>(&j).unwrap(), m);
        }
    }

    #[test]
    fn chatline_converts_into_protocol_chatline() {
        let line = ChatLine {
            from_label: "skimble-human".into(),
            text: "ciao".into(),
            display_name: None,
            is_ai: false,
        };
        let proto: protocol::ChatLine = line.into();
        assert_eq!(proto.from_label, "skimble-human");
        assert_eq!(proto.text, "ciao");
    }

    #[test]
    fn chatmsg_history_roundtrip_and_tag() {
        let m = ChatMsg::History {
            entries: vec![
                ChatLine {
                    from_label: "skimble-human".into(),
                    text: "ciao".into(),
                    display_name: None,
                    is_ai: false,
                },
                ChatLine {
                    from_label: "quaxo-human".into(),
                    text: "ehi".into(),
                    display_name: None,
                    is_ai: false,
                },
            ],
        };
        let j = serde_json::to_string(&m).unwrap();
        assert!(j.contains(r#""type":"history""#), "tag errato: {j}");
        assert_eq!(serde_json::from_str::<ChatMsg>(&j).unwrap(), m);
    }

    #[test]
    fn announce_roundtrip_serializes_id_as_ip_string() {
        let a = Announce {
            v: 1,
            id: PeerId(Ipv4Addr::new(192, 168, 1, 12)),
            label_base: "skimble".into(),
            chat_port: 40100,
            leader: None,
        };
        let j = serde_json::to_string(&a).unwrap();
        // PeerId (newtype su Ipv4Addr) → stringa "192.168.1.12".
        assert!(j.contains(r#""id":"192.168.1.12""#), "id non è stringa IP: {j}");
        assert_eq!(serde_json::from_str::<Announce>(&j).unwrap(), a);
    }

    // -------------------------------------------------------------------------
    // Bug 2 (late-joiner): `Announce.leader` — vedi
    // Docs/superpowers/specs/2026-07-02-aichat-late-joiner-election-design.md §3.5
    // -------------------------------------------------------------------------

    #[test]
    fn announce_roundtrip_with_leader_some() {
        let a = Announce {
            v: 1,
            id: PeerId(Ipv4Addr::new(192, 168, 1, 26)),
            label_base: "rumpleteazer".into(),
            chat_port: 40100,
            leader: Some(PeerId(Ipv4Addr::new(192, 168, 1, 26))),
        };
        let j = serde_json::to_string(&a).unwrap();
        assert!(j.contains(r#""leader":"192.168.1.26""#), "leader non serializzato come stringa IP: {j}");
        assert_eq!(serde_json::from_str::<Announce>(&j).unwrap(), a);
    }

    #[test]
    fn announce_without_leader_field_defaults_to_none() {
        // Retro-compatibilità: un datagramma da un binario non ancora aggiornato non
        // include affatto il campo `leader`. `#[serde(default)]` deve farlo decodificare
        // comunque, con `leader: None`.
        let json_from_old_binary = r#"{"v":1,"id":"192.168.1.35","label_base":"skimble","chat_port":40100}"#;
        let a: Announce = serde_json::from_str(json_from_old_binary).unwrap();
        assert_eq!(a.leader, None);
    }

    #[test]
    fn chatmsg_ping_roundtrip_and_tag() {
        let m = ChatMsg::Ping {};
        let j = serde_json::to_string(&m).unwrap();
        assert!(j.contains(r#""type":"ping""#), "tag errato: {j}");
        assert_eq!(serde_json::from_str::<ChatMsg>(&j).unwrap(), m);
    }

    #[test]
    fn chatmsg_peer_lost_roundtrip_and_tag() {
        let m = ChatMsg::PeerLost { label: "quaxo-human".into() };
        let j = serde_json::to_string(&m).unwrap();
        assert!(j.contains(r#""type":"peer_lost""#), "tag errato: {j}");
        assert!(j.contains(r#""label":"quaxo-human""#), "{j}");
        assert_eq!(serde_json::from_str::<ChatMsg>(&j).unwrap(), m);
    }

    // ── TDD RED: Library "Share with" — messaggi peer↔peer (Slice 1a) ───────────
    // Scritto PRIMA di aggiungere le 4 varianti a `ChatMsg`. Prima dell'aggiunta produce
    // `E0599: no variant named ... found for enum `ChatMsg`` — quell'errore di compilazione
    // È la fase RED. Vedi
    // Docs/superpowers/plans/2026-07-05-library-share-slice1-consent-net.md, Task 2.
    #[test]
    fn share_msgs_roundtrip_and_tags() {
        let cases = [
            (
                ChatMsg::ShareOffer {
                    share_id: "192.168.1.10:1".into(),
                    from_label: "rumpleteazer".into(),
                    doc_name: "notes.md".into(),
                    size_bytes: 1234,
                },
                r#""type":"share_offer""#,
            ),
            (
                ChatMsg::ShareAccept { share_id: "192.168.1.10:1".into(), from_label: "skimble".into() },
                r#""type":"share_accept""#,
            ),
            (
                ChatMsg::ShareReject { share_id: "192.168.1.10:1".into(), from_label: "skimble".into() },
                r#""type":"share_reject""#,
            ),
            (
                ChatMsg::ShareData {
                    share_id: "192.168.1.10:1".into(),
                    title: "Note".into(),
                    content: "corpo del documento".into(),
                },
                r#""type":"share_data""#,
            ),
            (
                ChatMsg::ShareExpired { share_id: "192.168.1.10:1".into() },
                r#""type":"share_expired""#,
            ),
        ];
        for (m, expected_tag) in cases {
            let j = serde_json::to_string(&m).unwrap();
            assert!(j.contains(expected_tag), "tag errato: {j}");
            assert_eq!(serde_json::from_str::<ChatMsg>(&j).unwrap(), m);
        }
    }

    // ── TDD RED: varianti di ammissione alla stanza (voto peer↔peer) ─────────
    // Scritto PRIMA di aggiungere le 5 varianti a `ChatMsg`. Prima dell'aggiunta
    // produce errori di compilazione `E0599: no variant named ... found for enum
    // `ChatMsg`` — quell'errore di compilazione È la fase RED.
    // Vedi Docs/superpowers/plans/2026-07-03-aichat-admission.md, Task 1.
    #[test]
    fn admission_msgs_roundtrip_and_tags() {
        // Ogni tupla: (messaggio, tag snake_case atteso sul wire).
        let cases = [
            (
                ChatMsg::RequestAdmission { label: "cand-human".into() },
                r#""type":"request_admission""#,
            ),
            (
                ChatMsg::AdmissionVoteRequest { candidate: "cand-human".into() },
                r#""type":"admission_vote_request""#,
            ),
            (
                ChatMsg::AdmissionVote { candidate: "cand-human".into(), accept: true },
                r#""type":"admission_vote""#,
            ),
            (
                ChatMsg::Admitted { label: "cand-human".into() },
                r#""type":"admitted""#,
            ),
            (
                ChatMsg::AdmissionRejected { label: "cand-human".into() },
                r#""type":"admission_rejected""#,
            ),
            (
                ChatMsg::AdmissionResolved { candidate: "cand-human".into() },
                r#""type":"admission_resolved""#,
            ),
        ];
        for (m, expected_tag) in cases {
            let j = serde_json::to_string(&m).unwrap();
            assert!(j.contains(expected_tag), "tag errato: {j}");
            assert_eq!(serde_json::from_str::<ChatMsg>(&j).unwrap(), m);
        }
    }

    #[test]
    fn chatmsg_notes_digest_roundtrip_and_tag() {
        use crate::notes::digest::NoteDigest;
        let m = ChatMsg::NotesDigest {
            entries: vec![(
                "192.168.1.10:0".into(),
                NoteDigest {
                    deleted: false,
                    title_touched: ("skimble".into(), 100),
                    segment_keys: vec![("skimble".into(), 1)],
                },
            )],
        };
        let j = serde_json::to_string(&m).unwrap();
        assert!(j.contains(r#""type":"notes_digest""#), "tag errato: {j}");
        assert_eq!(serde_json::from_str::<ChatMsg>(&j).unwrap(), m);
    }

    #[test]
    fn chatmsg_notes_digest_reply_roundtrip_and_tag() {
        use crate::notes::Note;
        let pushed = Note {
            id: "192.168.1.10:0".into(),
            title: "t".into(),
            title_touched: ("skimble".into(), 0),
            segments: vec![],
            created_by: "skimble".into(),
            created_at_ms: 0,
            deleted: false,
        };
        let m = ChatMsg::NotesDigestReply { want: vec!["192.168.1.11:2".into()], push: vec![pushed] };
        let j = serde_json::to_string(&m).unwrap();
        assert!(j.contains(r#""type":"notes_digest_reply""#), "tag errato: {j}");
        assert_eq!(serde_json::from_str::<ChatMsg>(&j).unwrap(), m);
    }

    #[test]
    fn chatmsg_notes_data_and_note_updated_roundtrip_and_tag() {
        use crate::notes::Note;
        let n = Note {
            id: "192.168.1.10:0".into(),
            title: "t".into(),
            title_touched: ("skimble".into(), 0),
            segments: vec![],
            created_by: "skimble".into(),
            created_at_ms: 0,
            deleted: false,
        };
        let data = ChatMsg::NotesData { notes: vec![n.clone()] };
        let j = serde_json::to_string(&data).unwrap();
        assert!(j.contains(r#""type":"notes_data""#), "tag errato: {j}");
        assert_eq!(serde_json::from_str::<ChatMsg>(&j).unwrap(), data);

        let updated = ChatMsg::NoteUpdated { note: n };
        let j2 = serde_json::to_string(&updated).unwrap();
        assert!(j2.contains(r#""type":"note_updated""#), "tag errato: {j2}");
        assert_eq!(serde_json::from_str::<ChatMsg>(&j2).unwrap(), updated);
    }
}
