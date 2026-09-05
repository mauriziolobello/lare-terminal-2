//! Integration test REALE (#[ignore]) del relay TCP su loopback, senza scoperta UDP.
//!
//! Verifica il giro end-to-end:
//!   client.HumanSay("ciao da client")
//!   → client TCP → 127.0.0.1:PORT
//!   → server reader task → ServiceEvent::PeerMsg
//!   → server handle_event → Effect::ToUi(AiChatMessage)
//!   → server_tx → asserzione
//!
//! NON copre la scoperta UDP (quella è validazione LIVE in 1a-ui-B).
//! Eseguire: `cargo test -p orchestrator --test aichat_relay_loopback -- --ignored`
//!
//! ## Nota sul loopback PeerId mismatch
//!
//! Su loopback, `peer_addr().ip()` restituisce SEMPRE `127.0.0.1`, anche per connessioni
//! "provenienti" da un client con PeerId `127.0.0.2`. Il server quindi registra il link
//! sotto `PeerId(127.0.0.1)` — coincide con il proprio `me.id`. Questo non rompe il test
//! perché (a) `ToUi(AiChatMessage)` viene emesso incondizionatamente dal ramo `Say`, e
//! (b) `roster_snapshot()` non trova info per `PeerId(127.0.0.1)` in `self.peers` (lì è
//! registrato solo `PeerId(127.0.0.2)` via Discovered) → roster vuoto → 0 relay target.
//! Su LAN reale ogni macchina ha IP distinto, il mismatch non accade.

use orchestrator::ai_adapter::StubAdapter;
use orchestrator::aichat::peer::{PeerId, PeerInfo};
use orchestrator::aichat::service::{AiChatService, ServiceEvent};
use orchestrator::notes::store::NotesStore;
use protocol::ServerMsg;
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

/// Porta TCP fissa usata dal listener del server nel test di loopback.
///
/// Porta alta (> 49152) per ridurre la probabilità di conflitti con altri servizi.
/// Per test `#[ignore]` su I/O reale è accettabile una porta fissa (documentato nel brief).
const PORT: u16 = 40191;

#[tokio::test]
#[ignore = "I/O reale loopback; eseguire con --ignored"]
async fn client_human_say_reaches_server_ui_over_tcp() {
    // -----------------------------------------------------------------
    // Identità dei due peer per questo test di loopback.
    //
    // Regola di elezione: IP più basso = server.
    // 127.0.0.1 < 127.0.0.2 → server = .1, client = .2.
    //
    // `chat_port` del server = PORT: `decide_and_connect` emetterà StartListener(PORT)
    //   → il server binderà su PORT.
    // `chat_port` del client = 0: non fa da server, la porta è irrilevante.
    // -----------------------------------------------------------------
    let server_id = PeerId(Ipv4Addr::new(127, 0, 0, 1));
    let client_id = PeerId(Ipv4Addr::new(127, 0, 0, 2));

    let server_info = PeerInfo { id: server_id, label_base: "server".into(), chat_port: PORT };
    let client_info = PeerInfo { id: client_id, label_base: "client".into(), chat_port: 0 };

    // -----------------------------------------------------------------
    // Crea i due attori: un `AiChatService` per macchina.
    // -----------------------------------------------------------------
    // `StubAdapter`: questi test coprono il relay TCP, non l'AI Chat Slice 1a
    // (invocazione "@ai") — nessuna chiamata HTTP reale attesa qui.
    let server_svc = AiChatService::new(server_info.clone(), Arc::new(StubAdapter), true, false, NotesStore::empty_in_memory(), None, None, std::path::PathBuf::from("/test-config"));
    let client_svc = AiChatService::new(client_info.clone(), Arc::new(StubAdapter), true, false, NotesStore::empty_in_memory(), None, None, std::path::PathBuf::from("/test-config"));

    // Canali inbox separati per i due attori (ogni attore è indipendente).
    let (server_inbox_tx, server_inbox_rx) = mpsc::unbounded_channel::<ServiceEvent>();
    let (client_inbox_tx, client_inbox_rx) = mpsc::unbounded_channel::<ServiceEvent>();

    // Canale "UI" del server: qui leggeremo i `ServerMsg` prodotti dall'attore server.
    // È il canale che in produzione è collegato alla finestra AI Chat di Tauri.
    let (server_ui_tx, mut server_ui_rx) = mpsc::unbounded_channel::<ServerMsg>();

    // Avvia i due attori in background (ognuno possiede il proprio inbox_rx).
    // Token di shutdown: mai cancellato in questo test (nessuna asserzione su
    // teardown qui — quello è coperto da `run_exits_on_cancellation` in service.rs).
    let shutdown = tokio_util::sync::CancellationToken::new();
    tokio::spawn(server_svc.run(server_inbox_rx, server_inbox_tx.clone(), shutdown.clone()));
    tokio::spawn(client_svc.run(client_inbox_rx, client_inbox_tx.clone(), shutdown.clone()));

    // -----------------------------------------------------------------
    // Setup del SERVER
    //
    // 1. Connetti la "UI" al server: tutti i ServerMsg prodotti da ora in poi
    //    arriveranno su server_ui_rx.
    // 2. Inietta il peer "client" come discovered: Task 8 (rimozione del vecchio
    //    consenso pairwise) — `Discovered` da sola aggiorna il roster dell'elezione
    //    e chiama `decide_and_connect()` → StartListener(PORT) (IP .1 più basso di
    //    .2 → server). Il perform StartListener binda su 0.0.0.0:PORT e avvia
    //    l'accept loop. Non c'è più un secondo evento "Consent" da mandare.
    // -----------------------------------------------------------------
    server_inbox_tx
        .send(ServiceEvent::SetServerTx(server_ui_tx))
        .expect("send SetServerTx al server");
    server_inbox_tx
        .send(ServiceEvent::Discovered(client_info.clone(), None))
        .expect("send Discovered al server");

    // Attendi che il listener sia bindato prima che il client tenti di connettersi.
    // 100ms è più che sufficiente per un bind loopback su Windows.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // -----------------------------------------------------------------
    // Setup del CLIENT
    //
    // 3. Inietta il peer "server" come discovered: `Discovered` da sola chiama
    //    decide_and_connect() → ConnectTo(server_info) (IP .2 non è il più basso →
    //    client). Il perform ConnectTo si connette a 127.0.0.1:PORT e avvia i task
    //    reader/writer. Task 8: NON invia più `Join` — il connect riuscito fa
    //    scattare `begin_join_gate()` (mostra il gate 1 alla propria UI, qui
    //    assente → effetto silenziosamente scartato) e il client resta in stato
    //    `Deciding` per tutta la durata di questo test (nessuno risponde al gate 1
    //    perché non c'è una UI client che lo faccia). Questo NON impedisce il
    //    relay di `Say` sotto: il gate di ammissione non è ancora applicato al
    //    relay (`TODO(admission-8b)` in `service.rs`), quindi il messaggio arriva
    //    comunque al server anche senza che il client sia mai stato "ammesso".
    // -----------------------------------------------------------------
    client_inbox_tx
        .send(ServiceEvent::Discovered(server_info.clone(), None))
        .expect("send Discovered al client");

    // Attendi la connessione TCP (connect loopback + avvio task reader/writer).
    tokio::time::sleep(Duration::from_millis(300)).await;

    // -----------------------------------------------------------------
    // Azione: il client "umano" invia un messaggio nella stanza.
    //
    // Effetti attesi lato client:
    //   - ToUi(AiChatMessage { "client-human", "ciao da client" }) → scartato (no server_tx)
    //   - SendToPeer(PeerId(127.0.0.1), Say { ... }) → scritto sul socket TCP
    //
    // Effetti attesi lato server (dopo che il reader legge la riga JSON):
    //   - handle_event(PeerMsg { from: ..., msg: Say { "client-human", ... } })
    //     → ToUi(AiChatMessage { "client-human", "ciao da client" }) ← asserzione
    //     → server_relay → 0 target (loopback: roster vuoto) → nessun SendToPeer
    // -----------------------------------------------------------------
    client_inbox_tx
        .send(ServiceEvent::HumanSay("ciao da client".into()))
        .expect("send HumanSay al client");

    // -----------------------------------------------------------------
    // Asserzione: drena server_ui_rx fino a trovare AiChatMessage.
    //
    // Task 8: la sequenza "Join → AiChatRoster" prima di AiChatMessage non è più
    // valida — `Discovered` non emette più `AiChatJoinRequest`, e il client non
    // manda mai `RequestAdmission` in questo test (nessuna UI client risponde al
    // gate 1, vedi il commento sul setup del CLIENT sopra), quindi il server non
    // avvia mai un voto e non emette mai `AiChatRoster` qui. Prima di AiChatMessage
    // il server_ui_rx riceve solo:
    //   1. AiChatSelf { label: "server-human" }  (incondizionato, da SetServerTx)
    //   2. AiChatMessage { from_label: "client-human", text: "ciao da client" } ← target
    //
    // Il loop `continue` salta 1 (e qualunque altro messaggio non previsto — resta
    // robusto anche se un task futuro reintroducesse un effetto intermedio) e
    // torna per 2. Il `timeout` evita che il test rimanga appeso indefinitamente
    // in caso di regressione.
    // -----------------------------------------------------------------
    let got = loop {
        match tokio::time::timeout(Duration::from_secs(2), server_ui_rx.recv()).await {
            // Trovato! Estrai i campi e termina il loop.
            Ok(Some(ServerMsg::AiChatMessage { from_label, text, .. })) => {
                break (from_label, text);
            }
            // Un altro tipo di messaggio (AiChatSelf, ecc.): skippa.
            Ok(Some(_other)) => {
                continue;
            }
            // Il canale si è chiuso: l'attore server è terminato inaspettatamente.
            Ok(None) => {
                panic!("server_ui channel chiuso prima di ricevere AiChatMessage");
            }
            // Timeout: il messaggio non è mai arrivato entro 2 secondi.
            Err(_timeout) => {
                panic!("timeout 2s: AiChatMessage mai arrivato al server via TCP loopback");
            }
        }
    };

    assert_eq!(
        got.0, "client-human",
        "from_label errata (atteso 'client-human', ricevuto {:?})",
        got.0
    );
    assert_eq!(
        got.1, "ciao da client",
        "testo errato (atteso 'ciao da client', ricevuto {:?})",
        got.1
    );
}

/// Porta TCP fissa per questo test — distinta da `PORT` sopra per evitare conflitti se
/// entrambi i test `--ignored` girano nello stesso processo `cargo test`.
const GHOST_PORT: u16 = 40192;

/// Verifica Task 7+8 (keepalive): un peer che smette di rispondere SENZA chiudere il
/// socket (simulando un freeze — crash, sospensione, cavo staccato: niente FIN TCP) viene
/// dichiarato sparito entro la soglia di silenzio (`DEAD_THRESHOLD` = 15s), non "mai" come
/// prima di questa slice.
///
/// ## Perché una connessione TCP grezza invece di un secondo `AiChatService`
///
/// Un secondo `AiChatService` reale manderebbe i suoi Ping regolari via il proprio timer
/// `Tick` (Task 7) — non si potrebbe farlo "restare in silenzio" senza hook di test
/// invasivi. Una connessione grezza, tenuta viva ma mai scritta dopo il primo `Join`,
/// riproduce esattamente "socket aperto ma applicazione congelata".
///
/// ## Perché l'id del server è 10.0.0.1 e non un 127.x
///
/// Due vincoli in tensione: (a) su loopback, `peer_addr().ip()` in accept() restituisce
/// SEMPRE `127.0.0.1` per qualunque connessione in ingresso (stesso quirk documentato nel
/// test sopra) — quindi il "ghost" DEVE essere registrato sotto `PeerId(127.0.0.1)` via
/// `Discovered`, per coincidere con l'id che l'accept loop gli assegnerà davvero (serve a
/// `PeerGone`, che cerca l'etichetta in `self.peers` per costruire l'annuncio, Task 6).
/// (b) L'elezione vince per IP numericamente più basso: se il server avesse un'identità
/// nominale `127.0.0.x` con `x > 1`, il ghost (`127.0.0.1`) vincerebbe l'elezione al posto
/// del server, che non chiamerebbe mai `StartListener` — la connessione TCP grezza sotto
/// fallirebbe (nessuno in ascolto). Usiamo quindi `10.0.0.1` come identità nominale del
/// server: numericamente più basso di `127.0.0.1` (primo ottetto 10 < 127), così il server
/// si autoelegge correttamente. Non influisce sul bind reale, che è sempre su
/// `0.0.0.0:porta` (`bind_listener`), indipendentemente da `me.id`.
#[tokio::test]
#[ignore = "I/O reale, attesa ~20s; eseguire con --ignored"]
async fn silent_peer_is_declared_gone_after_dead_threshold() {
    let server_id = PeerId(Ipv4Addr::new(10, 0, 0, 1));
    let ghost_id = PeerId(Ipv4Addr::new(127, 0, 0, 1));

    let server_info = PeerInfo { id: server_id, label_base: "server".into(), chat_port: GHOST_PORT };
    let ghost_info = PeerInfo { id: ghost_id, label_base: "ghost".into(), chat_port: 0 };

    let server_svc = AiChatService::new(server_info.clone(), Arc::new(StubAdapter), true, false, NotesStore::empty_in_memory(), None, None, std::path::PathBuf::from("/test-config"));
    let (server_inbox_tx, server_inbox_rx) = mpsc::unbounded_channel::<ServiceEvent>();
    let (server_ui_tx, mut server_ui_rx) = mpsc::unbounded_channel::<ServerMsg>();
    // Token di shutdown mai cancellato: questo test verifica il timeout keepalive,
    // non il teardown (coperto separatamente da `run_exits_on_cancellation`).
    let shutdown = tokio_util::sync::CancellationToken::new();
    tokio::spawn(server_svc.run(server_inbox_rx, server_inbox_tx.clone(), shutdown));

    server_inbox_tx.send(ServiceEvent::SetServerTx(server_ui_tx)).expect("SetServerTx");
    // Task 8: `Discovered` da sola basta — aggiorna `self.peers` (da cui `PeerGone`
    // legge l'etichetta per l'annuncio) e chiama `decide_and_connect()` →
    // StartListener. Non c'è più un secondo evento "Consent" da mandare.
    server_inbox_tx
        .send(ServiceEvent::Discovered(ghost_info.clone(), None))
        .expect("Discovered ghost");

    // Attendi il bind del listener prima di connettersi.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Connessione TCP grezza: scrive una riga JSON valida (un `ChatMsg::Join` — Task 8:
    // dead code applicativo lato server, vedi `handle_event`, ma resta un messaggio
    // wire valido: qui serve solo a produrre un primo scritto sul socket prima del
    // silenzio), poi resta MUTA (nessun altro scritto) — mai un FIN, quindi mai un EOF
    // "pulito": solo il timeout keepalive può rilevare la sparizione.
    let mut ghost_stream = tokio::net::TcpStream::connect(("127.0.0.1", GHOST_PORT))
        .await
        .expect("connect ghost al server");
    ghost_stream
        .write_all(b"{\"type\":\"join\",\"label\":\"ghost-human\"}\n")
        .await
        .expect("scrittura Join grezza");

    // Attendi oltre DEAD_THRESHOLD (15s): il reader del server deve accorgersi del
    // silenzio e il server deve annunciare la sparizione alla propria UI.
    let got_label = loop {
        match tokio::time::timeout(Duration::from_secs(20), server_ui_rx.recv()).await {
            Ok(Some(ServerMsg::AiChatPeerLost { label })) => break label,
            Ok(Some(_other)) => continue, // AiChatSelf, ecc.: skippa
            Ok(None) => panic!("server_ui channel chiuso prima di AiChatPeerLost"),
            Err(_timeout) => {
                panic!("timeout 20s: AiChatPeerLost mai arrivato — silenzio non rilevato")
            }
        }
    };

    assert_eq!(got_label, "ghost-human");

    // Tiene viva la connessione fino a qui: un drop anticipato manderebbe un FIN reale,
    // confondendo il test con il path EOF invece del path keepalive.
    drop(ghost_stream);
}
