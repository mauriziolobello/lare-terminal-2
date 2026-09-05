//! Integration test REALE (#[ignore]) della consegna di un'offerta Share su loopback.
//! Stesso harness di `aichat_relay_loopback.rs` (due `AiChatService` reali, TCP vero su
//! 127.0.0.1) — vedi quel file per la nota completa sul "loopback PeerId mismatch"
//! (l'accept loop vede SEMPRE `127.0.0.1` come IP del peer in ingresso, qualunque sia
//! la vera identità nominale del chiamante che si è connesso).
//!
//! ## Perché questo test copre SOLO la consegna dell'offerta, non il round-trip intero
//!
//! Il piano originale (Task 11) prevedeva un round-trip completo: A offre un documento
//! a B, B accetta, A vede l'esito. Tentativo dal vivo, diagnosi confermata due volte
//! (una volta dal log `DEBUG aichat: SendToPeer a peer senza link: PeerId(127.0.0.2)`,
//! una volta in review con l'advisor): su loopback Windows, il ramo server→client di
//! `Effect::SendToPeer` (necessario per "il destinatario dell'offerta risponde
//! all'offerente") **non è testabile** con due `AiChatService` reali, per una ragione
//! strutturale — non un typo risolvibile scegliendo IP diversi:
//!
//! - Il CLIENT apre la connessione in uscita (`ConnectTo`): il suo `self.links` viene
//!   registrato con l'id VERO del server (quello passato a `ConnectTo`) — affidabile,
//!   nessun coinvolgimento dell'accept loop. `route_to_label`+`SendToPeer` funzionano
//!   SEMPRE in direzione client→server.
//! - Il SERVER accetta la connessione: il suo `self.links` viene registrato con l'id
//!   che l'accept loop OSSERVA — su loopback Windows, sempre `127.0.0.1`,
//!   indipendentemente dalla vera identità nominale del client che si è connesso
//!   (comportamento OS, non applicativo). Perché il server possa raggiungere quel
//!   link con `route_to_label`, l'id nominale del client dovrebbe combaciare con
//!   `127.0.0.1` — ma quell'indirizzo deve restare libero per il SERVER (il client lo
//!   deve poter comporre con `ConnectTo`, che usa l'id nominale come indirizzo di
//!   destinazione reale), quindi le due esigenze si escludono a vicenda. Nessuna
//!   combinazione di due `PeerId` di loopback distinti risolve questo — l'unica vera
//!   soluzione è una LAN reale, dove `peer_addr().ip()` riporta l'IP vero e distinto
//!   di ciascuna macchina (nessun mismatch).
//!
//! **Cosa prova comunque questo test**: la superficie di rete NUOVA introdotta da
//! questa slice — serializzazione/deserializzazione di `ChatMsg::ShareOffer` su un
//! vero socket TCP, il reader task che la decodifica, `ServiceEvent::PeerMsg` che la
//! consegna a `handle_share_offer` — non coperta dai test puri di `handle_event`
//! (quelli iniettano l'evento direttamente, saltando la rete). La direzione
//! client→server è esattamente quella che NON soffre del mismatch, quindi è l'unica
//! metà del round-trip verificabile con integrità qui. Le altre metà (accetta/rifiuta,
//! risultato al mittente, scadenza) restano coperte dai test `handle_event` di Task
//! 5/6/8/9 (già verdi) e andranno confermate dal vivo multi-macchina (vedi
//! `Docs/TESTING-e2e.md`), dove il mismatch non esiste.
//!
//! Eseguire: `cargo test -p orchestrator --test aichat_share_loopback -- --ignored`

use orchestrator::ai_adapter::StubAdapter;
use orchestrator::aichat::peer::{PeerId, PeerInfo};
use orchestrator::aichat::service::{AiChatService, ServiceEvent};
use orchestrator::notes::store::NotesStore;
use protocol::{ServerMsg, ShareTarget};
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

/// Porta TCP fissa per questo test — distinta da quelle usate in
/// `aichat_relay_loopback.rs` per evitare conflitti se più test `--ignored` girano
/// nello stesso processo `cargo test`.
const PORT: u16 = 40193;

/// Consegna di un'offerta Share dal CLIENT al SERVER su un vero socket TCP loopback —
/// la sola metà del round-trip che il mismatch di loopback (vedi commento di modulo)
/// non rompe. Il CLIENT (B) manda l'offerta; il SERVER (A) deve riceverla e mostrare
/// `ServerMsg::ShareRequest` sulla propria UI con i metadati corretti.
#[tokio::test]
#[ignore = "I/O reale loopback; eseguire con --ignored"]
async fn client_delivers_share_offer_to_server_over_real_tcp() {
    let a_id = PeerId(Ipv4Addr::new(127, 0, 0, 1));
    let b_id = PeerId(Ipv4Addr::new(127, 0, 0, 2));
    let a_info = PeerInfo {
        id: a_id,
        label_base: "a".into(),
        chat_port: PORT,
    };
    let b_info = PeerInfo {
        id: b_id,
        label_base: "b".into(),
        chat_port: 0,
    };

    let a_svc = AiChatService::new(a_info.clone(), Arc::new(StubAdapter), true, false, NotesStore::empty_in_memory(), None, None, std::path::PathBuf::from("/test-config"));
    let b_svc = AiChatService::new(b_info.clone(), Arc::new(StubAdapter), true, false, NotesStore::empty_in_memory(), None, None, std::path::PathBuf::from("/test-config"));

    let (a_inbox_tx, a_inbox_rx) = mpsc::unbounded_channel::<ServiceEvent>();
    let (b_inbox_tx, b_inbox_rx) = mpsc::unbounded_channel::<ServiceEvent>();
    let (a_ui_tx, mut a_ui_rx) = mpsc::unbounded_channel::<ServerMsg>();
    let (b_ui_tx, _b_ui_rx) = mpsc::unbounded_channel::<ServerMsg>();

    let shutdown = tokio_util::sync::CancellationToken::new();
    tokio::spawn(a_svc.run(a_inbox_rx, a_inbox_tx.clone(), shutdown.clone()));
    tokio::spawn(b_svc.run(b_inbox_rx, b_inbox_tx.clone(), shutdown.clone()));

    // A: IP .1 più basso di .2 → si autoelegge server. SetServerTx PRIMA di Discovered
    // così il canale UI è già pronto per qualunque effetto immediato.
    a_inbox_tx
        .send(ServiceEvent::SetServerTx(a_ui_tx))
        .expect("SetServerTx ad A");
    a_inbox_tx
        .send(ServiceEvent::Discovered(b_info.clone(), None))
        .expect("Discovered B su A");

    // Attendi il bind del listener (StartListener innescato da decide_and_connect)
    // prima che B tenti la connessione.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // B: IP .2 non è il più basso → client, si connette ad A.
    b_inbox_tx
        .send(ServiceEvent::SetServerTx(b_ui_tx))
        .expect("SetServerTx a B");
    b_inbox_tx
        .send(ServiceEvent::Discovered(a_info.clone(), None))
        .expect("Discovered A su B");

    // Attendi la connessione TCP (connect loopback + spawn task reader/writer).
    tokio::time::sleep(Duration::from_millis(300)).await;

    // B (client) offre "notes.md" ad A (target risolto per label_base "a") — direzione
    // client→server, quella che il mismatch di loopback non rompe (vedi commento di
    // modulo): il `self.links` di B è registrato con l'id VERO di A (quello passato a
    // `ConnectTo`), non con un id osservato dall'accept loop.
    b_inbox_tx
        .send(ServiceEvent::ShareDocumentRequested {
            rel_path: "notes.md".into(),
            doc_name: "notes.md".into(),
            size_bytes: 10,
            target: ShareTarget::One {
                label_base: "a".into(),
            },
        })
        .expect("send ShareDocumentRequested a B");

    // A riceve ShareRequest sul proprio canale UI — drena finché non lo trova (salta
    // AiChatSelf, stesso pattern di aichat_relay_loopback.rs).
    let (share_id, doc_name, size_bytes) = loop {
        match tokio::time::timeout(Duration::from_secs(2), a_ui_rx.recv()).await {
            Ok(Some(ServerMsg::ShareRequest {
                share_id,
                doc_name,
                size_bytes,
                ..
            })) => {
                break (share_id, doc_name, size_bytes);
            }
            Ok(Some(_other)) => continue,
            Ok(None) => panic!("a_ui channel chiuso prima di ShareRequest"),
            Err(_timeout) => panic!("timeout 2s: ShareRequest mai arrivato ad A via TCP loopback"),
        }
    };

    assert!(!share_id.is_empty(), "share_id non deve essere vuoto");
    assert_eq!(doc_name, "notes.md");
    assert_eq!(size_bytes, 10);
}
