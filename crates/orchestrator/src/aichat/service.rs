//! `AiChatService`: l'ATTORE del canale AI Chat. Un solo task possiede lo stato;
//! ws.rs e i task I/O (discovery/reader) gli mandano `ServiceEvent`. La logica è in
//! `handle_event` (PURA: muta lo stato e ritorna `Effect`, non fa I/O). Il loop `run`
//! (Task 8) esegue gli effetti sui socket reali via i driver `net`.
//!
//! ## Pattern attore (actor pattern)
//!
//! Un *attore* è un'entità che:
//! 1. **Possiede** il proprio stato in modo esclusivo (nessun altro può modificarlo
//!    direttamente — in Rust questo è garantito dall'ownership: un solo task detiene
//!    il `&mut AiChatService`).
//! 2. **Riceve messaggi** (`ServiceEvent`) da tutte le sorgenti esterne
//!    (ws.rs, task discovery UDP, reader per-peer TCP).
//! 3. **Risponde con effetti** (`Vec<Effect>`) che descrivono le azioni da compiere
//!    sull'I/O reale — ma NON le esegue: il loop `run` (Task 8) le esegue.
//!
//! Questo separa nettamente la **logica pura** (testabile senza socket) dall'**I/O**
//! (che vive nel loop esterno). È il principio del "ritorna le azioni, non eseguirle",
//! già usato in `AiChatChannel` e nel relay.

use crate::ai_adapter::AiAdapter;
use crate::aichat::channel::{AiChatChannel, Role};
use crate::aichat::peer::{PeerId, PeerInfo};
use crate::aichat::wire::{ChatLine, ChatMsg};
use crate::messages_client::{Block, Message};
use crate::notes::{digest::render_body, store::NotesStore, Note, Segment};
use protocol::ServerMsg;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

/// Intervallo del battito cardiaco keepalive: ogni peer connesso manda un `Ping` con
/// questa cadenza (in entrambe le direzioni, stesso meccanismo per server e client).
const PING_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);
/// Soglia di silenzio oltre la quale un peer è dichiarato sparito: il reader task
/// (`spawn_peer_tasks`) tratta l'assenza di QUALUNQUE riga (non solo `Ping`) per questo
/// tempo come morte del link — stesso trattamento di un EOF/errore reale.
const DEAD_THRESHOLD: std::time::Duration = std::time::Duration::from_secs(15);

/// Orologio di parete in millisecondi (Task 7, Blocco note). Seam analogo a
/// `PeerTable::observe(info, now_ms)` in `discovery.rs`: `handle_event` non legge
/// mai `SystemTime::now()` direttamente, passa da qui (o, nei test, dal campo
/// `wallclock_ms_for_test` quando serve un valore deterministico) — così la
/// logica pura resta testabile senza dipendere dall'orologio reale.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// AI Chat Slice 2 — cap sui turni AI consecutivi (design §10.3:
/// `Docs/superpowers/specs/2026-07-02-aichat-ai-participants-design.md`).
///
/// È il "controllo del loop" della Slice 2: a differenza di 1a/1b (loop-safe per
/// COSTRUZIONE — solo un umano può innescare un turno AI), la Slice 2 fa parlare l'AI
/// SPONTANEAMENTE, quindi un burst AI↔AI cross-macchina è possibile in linea di
/// principio. Questo cap lo rende BOUNDED: ogni macchina smette di auto-partecipare
/// dopo `MAX_CONSECUTIVE_AI_TURNS` turni `-ai` consecutivi (visti localmente, senza
/// che nel mezzo sia passato un messaggio `-human` che azzera il contatore — vedi
/// `note_room_message`), quindi un burst coinvolge al più `n_macchine ×
/// MAX_CONSECUTIVE_AI_TURNS` turni AI prima che TUTTE le macchine tacciano. Un
/// qualunque messaggio umano riapre la partecipazione (reset del contatore).
const MAX_CONSECUTIVE_AI_TURNS: usize = 4;

/// Ammissione alla stanza (Task 4 —
/// `Docs/superpowers/plans/2026-07-03-aichat-admission.md`): quanto tempo (in
/// secondi) un presente ha per rispondere al gate 2 ("ammetti *candidate*?")
/// prima che il SERVER risolva il voto da solo. Un non-votante allo scadere
/// del timer conta come "sì" (silenzio = sì — Task 5, `resolve_if_ready`):
/// nessun voto resta bloccato per sempre solo perché un presente non ha
/// risposto (es. la sua macchina è spenta). `u64` (non `Duration`) perché
/// `Effect::StartVoteTimeout` lo porta così: `perform` (Task 7) lo userà con
/// `tokio::time::sleep(Duration::from_secs(secs))`.
const ADMISSION_VOTE_TIMEOUT_SECS: u64 = 60;

/// FIX #5 (review 2026-07-03): quanti secondi un candidato appena RIFIUTATO deve
/// aspettare prima che il SERVER accetti un suo nuovo `RequestAdmission`. Prima
/// del fix il cooldown viveva solo nella UI (`retryAtMs` in `admission.mjs`) e un
/// re-request via wire — o dopo una chiusura/riapertura della finestra che azzera
/// quello stato — lo scavalcava, riaprendo un voto e ri-promptando ogni presente a
/// ogni tentativo. Come per il timeout del voto, il cooldown è realizzato con un
/// `Effect::StartCooldownTimer` (I/O nel loop `perform`) così che `handle_event`
/// resti PURA — nessun `Instant::now()` nell'attore. 30s = stesso valore già
/// mostrato dalla UI (`ServerMsg::AiChatRejected { retry_after_secs: 30 }`).
const ADMISSION_REREQUEST_COOLDOWN_SECS: u64 = 30;

/// Timeout end-to-end di un trasferimento Share (spec §9.1): 24 ore dalla `ShareOffer`,
/// unico orologio per "mai risposto" e (in Slice 2) "accettato ma mai scritto su disco".
const SHARE_EXPIRY_SECS: u64 = 24 * 60 * 60;

/// Cap sui trasferimenti Share pendenti contemporaneamente su questa macchina (spec §8) —
/// protegge la memoria da un mittente che bombarda offerte mai risposte.
const MAX_PENDING_SHARES: usize = 1000;

/// Dimensione massima di un documento condivisibile (spec §1, §3.3, §8). L'utente ha
/// già anticipato che vorrà raddoppiarlo in futuro — bump = una riga qui, niente UI da
/// toccare (non è un campo di `/config`, deliberatamente).
const MAX_SHARE_SIZE_BYTES: u64 = 512 * 1024;

/// Debounce fisso dopo che un link si stabilizza, prima di scambiare digest —
/// non un timer libero: guardia contro il flapping durante l'assestamento
/// dell'elezione (design §5.2, §9).
const RECONCILE_DEBOUNCE_SECS: u64 = 2;

/// Eventi che arrivano all'attore da ogni sorgente (ws.rs, task discovery, reader per-peer).
///
/// Ogni evento rappresenta qualcosa che è *già accaduto* nel mondo esterno;
/// `handle_event` decide come reagire (aggiornando lo stato e producendo effetti).
///
/// In OOP classico potremmo chiamarli "messaggi" dell'attore o "comandi" di un Command Pattern.
#[derive(Debug)]
pub enum ServiceEvent {
    /// Una nuova connessione UI WS: il suo sink per i `ServerMsg`.
    /// Viene inviato da ws.rs ogni volta che la finestra AI Chat viene aperta.
    SetServerTx(UnboundedSender<ServerMsg>),
    /// La superficie chat è stata chiusa (umano esce / finestra chiusa).
    /// Viene inviato da ws.rs quando il canale WS si chiude.
    UiClosed,
    /// Un peer si è annunciato (dal task discovery UDP).
    /// Porta le sue informazioni pubbliche (id, etichetta, porta TCP) più il leader
    /// che QUEL peer ha riportato di credere valido (`Announce.leader`, `None` se il
    /// mittente è ancora `Undecided`). Il secondo campo è la base del fix Bug 2
    /// (late-joiner): senza di esso un nodo appena avviato non ha modo di sapere che
    /// il gruppo ha già un server eletto altrove — vedi `reported_leader` più sotto.
    Discovered(PeerInfo, Option<PeerId>),
    /// Un messaggio è arrivato da un peer connesso (dal reader per-peer).
    /// `from` identifica il peer mittente; `msg` è il payload wire.
    PeerMsg { from: PeerId, msg: ChatMsg },
    /// Un link verso un peer si è chiuso (reader per-peer ha ricevuto EOF, errore,
    /// timeout keepalive, o shutdown — vedi `spawn_peer_tasks`).
    ///
    /// Il secondo campo è la GENERAZIONE del link che è morto (Debito #4, Slice C:
    /// "race di riconnessione"). `spawn_peer_tasks` la riceve alla creazione del link
    /// e la riporta indietro qui quando il reader esce. `handle_event` confronta questa
    /// generazione con quella ATTUALMENTE registrata per il peer (`self.link_gen`): se
    /// non combaciano, questo evento si riferisce a un link già rimpiazzato da una
    /// riconnessione più recente — no-op. Senza questo, un `PeerGone` "vecchio"
    /// (dal reader di un link già morto) processato DOPO una riconnessione dello stesso
    /// IP rimuoverebbe il link FRESCO.
    PeerGone(PeerId, u64),
    /// L'umano locale invia un messaggio nella stanza (dal WS, tasto Invio).
    HumanSay(String),
    /// Timer periodico keepalive (ogni `PING_INTERVAL`, da `run()`): manda un `Ping` a
    /// ogni peer connesso. Non porta dati — è un semplice trigger temporale.
    Tick,
    /// L'accept loop TCP (spawnato in `perform` per `Effect::StartListener`) è terminato,
    /// per qualunque motivo (errore di `accept()`, oppure il canale verso l'attore si è
    /// chiuso). Non porta dati — è un trigger di dominio: "l'attore deve poter ri-bindare".
    ///
    /// Senza questo evento, il flag `self.listening` resterebbe bloccato a `true` per
    /// sempre: il guard di idempotenza in `perform` (`if self.listening { return }`)
    /// impedirebbe qualunque futuro `StartListener`, e il server smetterebbe di accettare
    /// connessioni in silenzio, senza modo di riprendersi (debito #5).
    ListenerStopped,
    /// AI Chat Slice 1a: il task spawnato da `perform` per `Effect::InvokeLocalAi` ha
    /// ottenuto il testo dalla propria AI locale (`ai_adapter.chat_reply(...)`). Porta
    /// SOLO il testo: `handle_event` lo tratta come un `Say` da `"<label_base>-ai"`
    /// (storico + eco UI + relay se server — vedi `publish_say`). Non c'è mai un
    /// `InvokeLocalAi` innescato a partire da questo evento: l'AI parla solo su
    /// invocazione umana esplicita (loop-safe per costruzione).
    AiReply { text: String },
    /// AI Chat Slice 2 (auto-partecipazione — design §10.3): il task spawnato da
    /// `perform` per `Effect::AutoParticipate` ha ottenuto l'ESITO del giudizio di
    /// rilevanza da `ai_adapter.chat_autoparticipate(...)`. `None` = l'AI ha scelto il
    /// silenzio (NON un errore — è un esito normale, il più frequente); `Some(text)` =
    /// contributo spontaneo, pubblicato come un `Say` da `"<label_base>-ai"` (stesso
    /// percorso di `AiReply`).
    ///
    /// A differenza di `Effect::InvokeLocalAi`/`AiReply` (dove `perform` scarta già una
    /// risposta vuota, perché lì il costo di un "silenzio" è basso e raro), QUI
    /// `perform` invia SEMPRE questo evento, anche per `None`: `handle_event` deve
    /// poter azzerare `autoparticipate_inflight` incondizionatamente, altrimenti un
    /// silenzio dell'AI lascerebbe il flag bloccato a `true` per sempre — nessuna
    /// futura auto-partecipazione scatterebbe più (vedi il doc-comment del campo
    /// `autoparticipate_inflight`).
    AutoParticipateDone { reply: Option<String> },

    // -------------------------------------------------------------------------
    // Ammissione alla stanza (scaffolding — Task 3). Arrivano da ws.rs (i primi
    // tre, risposte dell'umano ai due gate + il pulsante di re-request) o dal
    // timer del voto (l'ultimo). Nessuna logica ancora: gli arm in
    // `handle_event` sono no-op (`Vec::new()`) fino ai Task 4-7.
    // -------------------------------------------------------------------------
    /// (Nuovo arrivato) risposta dell'umano al gate 1 ("vuoi entrare con
    /// [presenti]?"). `true` → manderemo `RequestAdmission` ed entreremo in
    /// stato `Pending`; `false` → ci disconnettiamo (non volevamo entrare).
    JoinDecision { accept: bool },
    /// (Presente) voto dell'umano al gate 2 ("ammetti *candidate_label*?"),
    /// da inoltrare al server come `ChatMsg::AdmissionVote`.
    AdmissionVoteUi { candidate_label: String, accept: bool },
    /// (Nuovo arrivato, dopo un rifiuto) l'umano preme "chiedi di entrare"
    /// per ritentare. Il cooldown di 30s è già stato rispettato dalla UI
    /// (il pulsante resta disabilitato finché non scade — vedi il
    /// doc-comment di `SelfAdmission::Rejected`).
    RequestAdmissionUi,
    /// (Server) il timer di voto per `candidate` è scaduto senza che tutti i
    /// presenti abbiano votato. Nel design finale (Task 5/7), i non-votanti
    /// contano come "sì" (timeout = ammesso), salvo un veto già arrivato.
    VoteTimeout { candidate: PeerId, generation: u64 },
    /// (Server, FIX #5) il cooldown di re-request per `candidate` è scaduto: il
    /// candidato esce da `cooling_down` e può di nuovo chiedere l'ammissione.
    /// Inviato dal task del `Effect::StartCooldownTimer` (in `perform`), stesso
    /// pattern "timer come Effect" di `VoteTimeout` — così `handle_event` non
    /// legge mai l'orologio.
    CooldownExpired { candidate: PeerId },
    /// Timer di `Effect::StartShareExpiryTimer` scaduto (24h dall'Offer, spec §9.1).
    /// No-op se `share_id` è già stato risolto (rimosso da `pending_shares`).
    ShareExpiryTimeout { share_id: String },
    /// Trigger dalla UI (`ClientMsg::ShareDocument`): condividi un documento. `doc_name`/
    /// `size_bytes`/`rel_path` sono già risolti dalla UI — `rel_path` viene registrato in
    /// `outgoing_shares` (Slice 2a) per costruire `ServerMsg::ShareContentRequest` quando
    /// arriverà `ChatMsg::ShareAccept`.
    ShareDocumentRequested {
        rel_path: String,
        doc_name: String,
        size_bytes: u64,
        target: protocol::ShareTarget,
    },
    /// Risposta dell'umano (`ClientMsg::ShareConsent`) al banner mostrato da un
    /// `ShareOffer` in arrivo.
    ShareConsentUi { share_id: String, accept: bool },
    /// (Slice 2a, destinatario) `ClientMsg::ShareWritten` — la UI ha scritto su disco
    /// il documento ricevuto. Rimuove l'entry da `pending_shares`. Idempotente.
    ShareWrittenUi { share_id: String },
    /// (Slice 2a, mittente) `ClientMsg::ShareContent` — la UI ha letto con successo il
    /// documento richiesto da `ServerMsg::ShareContentRequest`.
    ShareContentUi { share_id: String, title: String, content: String },
    /// (Slice 2a, mittente) `ClientMsg::ShareContentFailed` — la UI non è riuscita a
    /// leggere il documento richiesto.
    ShareContentFailedUi { share_id: String, reason: String },

    // ── Library "Blocco note" (additiva) ──────────────────────────────────
    // Design: Docs/superpowers/specs/2026-07-29-library-notes-design.md §5.1.
    /// `ClientMsg::NoteCreate` dalla UI locale.
    NoteCreateRequested { title: String, text: String },
    /// `ClientMsg::NoteEdit`: sostituisce SOLO il segmento di QUESTA macchina.
    NoteEditRequested { id: String, text: String },
    /// `ClientMsg::NoteEditTitle`: last-write-wins, separato dal corpo.
    NoteEditTitleRequested { id: String, title: String },
    /// `ClientMsg::NoteDelete`: imposta il tombstone.
    NoteDeleteRequested { id: String },
    /// Timer di `Effect::StartNotesReconcileTimer` scaduto. `generation` è
    /// quella del link al momento in cui il timer è partito — se non combacia
    /// più con `self.link_gen[peer_id]` il link è stato rimpiazzato/chiuso nel
    /// frattempo: no-op (stesso principio-guardia di `VoteTimeout`).
    NotesReconcileDue { peer_id: PeerId, generation: u64 },
}

/// Azioni che il loop `run` deve eseguire (I/O). `handle_event` le *ritorna*, non le esegue.
///
/// In OOP potremmo chiamarle "comandi di output" o "side effect description".
/// Ogni variante descrive COSA fare; il loop decide COME (ha accesso ai socket reali).
///
/// `PartialEq` permette di asserire sugli effetti nei test senza I/O reale.
/// Nota: `Eq` non è derivabile perché `ToUi(ServerMsg)` porta un tipo che ha solo
/// `PartialEq` (nessuna relazione d'equivalenza totale su `f64` nei campi di `ServerMsg`).
#[derive(Debug, PartialEq)]
pub enum Effect {
    /// Spingi un `ServerMsg` alla UI connessa (se c'è).
    /// Il loop legge `server_tx` dallo stato e chiama `.send()`.
    ToUi(ServerMsg),
    /// Invia un `ChatMsg` a uno specifico peer connesso.
    /// Il loop cerca il link per `PeerId` nella propria mappa e scrive sul socket.
    SendToPeer(PeerId, ChatMsg),
    /// (Client) connettiti al server eletto al suo indirizzo TCP.
    /// Il loop crea il `TcpStream` e avvia il reader/writer per-peer.
    ConnectTo(PeerInfo),
    /// (Server) assicura il listener TCP sulla porta data.
    /// Il loop avvia `TcpListener::bind` se non ancora attivo.
    StartListener(u16),
    /// Chiudi il link verso un peer.
    /// Il loop trova il `PeerLink` e lo cancella / chiude il socket.
    Disconnect(PeerId),
    /// AI Chat Slice 1a: invoca la propria AI locale text-only. `perform` spawna un task
    /// che chiama `self.ai_adapter.chat_reply(&my_ai_label, &history, &request, ...)` e
    /// re-inietta il risultato come `ServiceEvent::AiReply { text }` sull'inbox —
    /// esattamente lo stesso pattern "descrivi l'azione, esegui nel loop I/O" degli altri
    /// effetti (es. `ConnectTo`). `request`/`history`/`my_ai_label` sono già pronti
    /// (costruiti PURAMENTE in `handle_event` da `extract_ai_invocation`/
    /// `build_ai_history`): `perform` è un semplice esecutore, non decide nulla.
    InvokeLocalAi { request: String, history: Vec<Message>, my_ai_label: String },
    /// AI Chat Slice 2 (auto-partecipazione — design §10.3): chiedi alla propria AI
    /// locale se ha qualcosa da aggiungere ALLA CONVERSAZIONE (nessun umano ha
    /// invocato nulla — `history` è l'unico input, a differenza di `InvokeLocalAi` che
    /// porta anche una `request` esplicita). `perform` spawna un task che chiama
    /// `self.ai_adapter.chat_autoparticipate(&my_ai_label, &history, ...)` e re-inietta
    /// l'esito come `ServiceEvent::AutoParticipateDone { reply }` — stesso pattern
    /// "descrivi l'azione, esegui nel loop I/O" di `InvokeLocalAi`.
    AutoParticipate { history: Vec<Message>, my_ai_label: String },
    /// Slice 2 (memoria persistente — vedi
    /// `Docs/superpowers/specs/2026-07-07-aichat-persistent-memory-design.md` §3): accoda
    /// `note` a `memory-{label_base}.md` (crea cartella+file se assenti). Rilevato PURAMENTE
    /// in `handle_event` da `extract_memoria_marker` sul testo che la PROPRIA AI ha appena
    /// restituito. `perform` fa I/O reale (mutex-guardato, vedi `memory_write_lock`) e non
    /// decide nulla — stesso pattern di `InvokeLocalAi`/`AutoParticipate`.
    PersistMemory { label_base: String, note: String },
    /// (Server) avvia il timer del voto di ammissione per `candidate`: `perform`
    /// spawna un task che aspetta `secs` secondi e poi manda
    /// `ServiceEvent::VoteTimeout { candidate, generation }` sull'inbox (stesso
    /// pattern "descrivi l'azione, esegui nel loop I/O" di `ConnectTo`/
    /// `InvokeLocalAi`). `generation` (FIX #1) identifica il turno: un timer che
    /// scade in ritardo, di un turno già risolto, la porta con sé e viene scartato
    /// da `handle_event` se non combacia con quella del voto ATTUALE.
    StartVoteTimeout { candidate: PeerId, generation: u64, secs: u64 },
    /// (Server, FIX #5) avvia il timer di cooldown del re-request per `candidate`:
    /// `perform` spawna un task che dorme `secs` secondi e poi manda
    /// `ServiceEvent::CooldownExpired { candidate }` sull'inbox — gemello di
    /// `StartVoteTimeout`. Emesso da `resolve_reject` quando un candidato viene
    /// rifiutato.
    StartCooldownTimer { candidate: PeerId, secs: u64 },
    /// Gemello di `StartVoteTimeout`/`StartCooldownTimer` per il timeout Share (spec
    /// §9.1): dopo `secs` secondi re-inietta `ServiceEvent::ShareExpiryTimeout`.
    StartShareExpiryTimer { share_id: String, secs: u64 },
    /// Gemello di `StartVoteTimeout`: dopo `secs` secondi re-inietta
    /// `ServiceEvent::NotesReconcileDue { peer_id, generation }`.
    StartNotesReconcileTimer { peer_id: PeerId, generation: u64, secs: u64 },
    /// Persisti `notes.json` con lo stato ATTUALE di `self.notes` (clonato da
    /// `perform` al momento dell'esecuzione — l'effetto non porta payload,
    /// `handle_event` resta senza I/O reale). Vero gemello di `PersistMemory`:
    /// `perform` clona `self.notes` (sincrono, economico) e spawna un task che fa
    /// il write reale su disco sotto `notes_write_lock`, così un `std::fs::write`
    /// lento non blocca il loop principale dell'attore (keepalive/elezione/relay).
    SaveNotes,
}

/// Esito asincrono di un tentativo di connessione (Debito #3, Slice C).
///
/// NON è un `ServiceEvent`: come il canale `new_link_tx`, è un dettaglio I/O interno del
/// loop `run` — non un evento di dominio che `handle_event` deve conoscere. Il task di
/// connessione spawnato da `perform` per `Effect::ConnectTo` (vedi sotto) manda uno di
/// questi due esiti sul canale `connect_res_tx`; il ramo dedicato del `select!` in `run`
/// applica l'esito allo stato tramite `register_connect_success`/`register_connect_failure`
/// e — solo per `Success` — spawna il reader/writer.
///
/// ## Perché non bloccare l'attore sul connect
///
/// Prima di questo fix, `Effect::ConnectTo` faceva `TcpStream::connect(addr).await`
/// **dentro** `perform`, cioè dentro il task dell'attore stesso: un peer irraggiungibile
/// blocca l'intero attore per il timeout del sistema operativo (~21s su Windows) — niente
/// ping, niente altri eventi, l'intero canale AI Chat sembra "congelato". Spostando il
/// connect in un task spawnato (con un timeout esplicito di 5s, non quello OS), l'attore
/// resta reattivo: continua a processare `Tick`/`PeerMsg`/altri `Discovered` mentre un
/// connect lento è ancora in corso in background.
///
/// ## Slice C2 — "register-before-spawn" (fix della race residua)
///
/// A differenza della versione 0.27.0, il task di connessione (spawnato in `perform` per
/// `Effect::ConnectTo`) NON chiama più `spawn_peer_tasks` su se stesso: manda indietro il
/// `TcpStream` GREZZO, non ancora "smontato" in reader/writer. Lo spawn avviene ORA
/// nell'attore (5° ramo di `select!` in `run`), SUBITO DOPO aver scritto
/// `link_gen`/`connected` via `register_connect_success`. Prima di questo fix, il reader
/// (spawnato dentro il task di connessione stesso) poteva emettere un
/// `ServiceEvent::PeerGone` PRIMA che l'attore avesse la possibilità di registrare lo
/// stato — la guardia di generazione lo vedeva come stale (la mappa non aveva ancora la
/// voce) e lo scartava, mentre la registrazione arrivava comunque dopo, creando un "link
/// fantasma" (registrato ma il cui reader era già morto). Vedi il doc-comment di
/// `register_link` per il dettaglio dell'invariante che chiude la race.
#[derive(Debug)]
enum ConnectOutcome {
    /// Connessione TCP riuscita: il `TcpStream` grezzo. Il chiamante (5° ramo di `run`)
    /// deve registrare lo stato PRIMA di spawnare `spawn_peer_tasks` su questo stream —
    /// vedi `register_connect_success`.
    Success {
        info: PeerInfo,
        stream: TcpStream,
        gen: u64,
    },
    /// Connessione fallita: errore di `TcpStream::connect` oppure timeout (5s) scaduto.
    /// L'attore deve solo sbloccare `connecting`, così un futuro `Discovered`/
    /// `decide_and_connect` per lo stesso peer possa ritentare.
    Failure { id: PeerId },
}

// =============================================================================
// Ammissione alla stanza (`Docs/superpowers/plans/2026-07-03-aichat-admission.md`).
// Sostituisce il vecchio consenso pairwise `pending`/`consented`/`refused` (Task 8
// li ha rimossi da `AiChatService`, vedi l'arm `Discovered` in `handle_event`) con
// un'ammissione a VOTO coordinata dal server: gate 1 (il nuovo arrivato accetta di
// entrare, Task 6), gate 2 (i presenti votano sull'ingresso, Task 4/5), silenzio-
// oltre-timeout = sì (Task 7).
// =============================================================================

/// Stato di QUESTA macchina come "nuovo arrivato" che sta entrando in una
/// stanza già popolata da altri peer.
///
/// In OOP potremmo vederlo come lo stato di una piccola macchina a stati
/// finiti (state machine): `NotJoining → Pending → Admitted` (via voto sì)
/// oppure `NotJoining → Pending → Rejected` (via veto), con un possibile
/// re-request che riporta `Rejected → Pending`.
///
/// **Nessun timestamp nella variante `Rejected`** (raffinamento del
/// supervisore rispetto alla bozza iniziale del piano, che portava
/// `retry_at`): per tenere `handle_event` PURA — niente `Instant::now()`
/// dentro l'attore, vedi il doc-comment del pattern attore in cima al file —
/// il cooldown di 30s del re-request è responsabilità della UI (Task 9/10,
/// `admission.mjs`), non di questo stato. Quando l'attore riceve un
/// re-request mentre è in `Rejected`, si fida che la UI abbia già rispettato
/// il cooldown (il pulsante resta disabilitato lato frontend finché non è
/// scaduto).
///
/// `pub` (non solo module-private) per lo stesso motivo di `Role`
/// (`aichat::channel::Role`, esposto da `role_for_test`): il getter di test
/// `self_admission_for_test` lo ritorna per riferimento, e un tipo di
/// ritorno di un metodo `pub` non può essere meno visibile del metodo
/// stesso (altrimenti `private_interfaces` lint). Non è comunque parte di
/// un'API pensata per l'uso esterno al crate: resta un dettaglio interno
/// dell'attore, solo "tecnicamente" raggiungibile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelfAdmission {
    /// Non stiamo entrando in nessuna stanza altrui: o siamo già dentro
    /// (siamo il server, oppure un client già ammesso), o siamo semplicemente
    /// soli/indecisi (nessun server ancora scoperto). Stato iniziale di ogni
    /// servizio appena creato.
    NotJoining,
    /// (Task 6) Ci siamo appena connessi al server eletto e gli abbiamo
    /// mostrato il gate 1 alla nostra UI ("vuoi entrare con [presenti]?"),
    /// ma l'umano non ha ancora risposto. Sostituisce, in questo punto della
    /// state machine, il vecchio invio immediato di `ChatMsg::Join`: ora
    /// aspettiamo `ServiceEvent::JoinDecision` prima di presentarci al
    /// server con `RequestAdmission` (vedi `begin_join_gate`).
    Deciding,
    /// Abbiamo mandato `ChatMsg::RequestAdmission` al server e siamo in
    /// attesa dell'esito del voto dei presenti. La UI deve bloccare l'input
    /// della chat in questo stato (Task 6/10) — previene il "saluto perso":
    /// scrivere prima di essere ammessi non avrebbe nessun presente pronto
    /// ad ascoltare.
    Pending,
    /// Il voto ci ha ammessi: possiamo scrivere nella stanza normalmente.
    Admitted,
    /// Il voto ci ha rifiutati (almeno un presente ha votato no — veto).
    /// Un re-request è permesso da qui (torna a `Pending`), soggetto al
    /// cooldown enforced lato UI (vedi il doc-comment sopra).
    Rejected,
}

/// (Solo lato SERVER) Un voto di ammissione attualmente in corso per UN
/// candidato — il "gate 2" del design (chiedere ai presenti "ammettiamo
/// questo nuovo arrivato?").
///
/// Vive esclusivamente nella mappa `AiChatService::pending_votes`, indicizzata
/// per `PeerId` del candidato. Ciclo di vita completo: creato da
/// `start_admission_vote` (Task 4), aggiornato da `record_vote`, rimosso alla
/// risoluzione admit/reject (`resolve_admit`/`resolve_reject`, Task 5).
#[derive(Debug, Clone)]
struct PendingVote {
    /// Etichetta del candidato (`"<base>-human"`), da mostrare all'umano nel
    /// gate 2 ("Ammetti <label>?") e da includere nei messaggi di rete
    /// (`ChatMsg::Admitted`/`AdmissionRejected` portano l'etichetta testuale,
    /// non il `PeerId`, che resta un dettaglio interno di questo processo).
    label: String,
    /// I peer PRESENTI al momento in cui il voto è iniziato — cioè chi deve
    /// votare. Congelato alla creazione (Task 4): un peer che si connette
    /// DOPO l'inizio del voto non fa parte di questo turno (non deve né
    /// votare né bloccare la risoluzione).
    present: HashSet<PeerId>,
    /// Voti raccolti finora: `PeerId` del votante → `accept`. Un voto
    /// `false` (veto) risolve immediatamente il turno in reject (Task 5);
    /// l'ASSENZA di un voto in questa mappa non significa "no" — significa
    /// solo "non ancora votato": è il timeout (non questa struttura) a
    /// decidere come trattare i non-votanti ("silenzio = sì").
    votes: HashMap<PeerId, bool>,
    /// FIX #1 (review 2026-07-03): generazione monotona di QUESTO turno, assegnata
    /// da `start_admission_vote` (dal contatore `AiChatService::next_vote_gen`).
    /// Identifica univocamente l'istanza del voto: il `Effect::StartVoteTimeout` la
    /// porta con sé e la re-inietta nel `ServiceEvent::VoteTimeout`, che la
    /// confronta con la generazione del voto ATTUALE. Se un candidato è rifiutato e
    /// poi ri-chiede, il nuovo turno ha una generazione diversa: il timer del turno
    /// vecchio, se scade in ritardo, non combacia più e viene ignorato (niente
    /// risoluzione "fantasma" del turno nuovo).
    generation: u64,
}

/// Un trasferimento Share in arrivo, non ancora concluso — TRE stadi (spec §9.1 + fix
/// di integrità del consenso, review finale del branch Slice 2a): `AwaitingDecision`
/// (in attesa della mia decisione — NON ancora accettata), `AwaitingContent` (accettata:
/// `share_consent` ha mandato `ShareAccept`, in attesa che il contenuto vero arrivi con
/// `ChatMsg::ShareData` — un `ShareData` che arriva mentre l'entry è ancora
/// `AwaitingDecision` è un no-op, non un'accettazione implicita: niente contenuto prima
/// del consenso esplicito, spec §8), e `AwaitingUiWrite` (il contenuto è arrivato, in
/// attesa che la UI lo scriva su disco). Un unico timer di scadenza (24h dall'Offer)
/// copre TUTTI e tre gli stadi.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingShare {
    AwaitingDecision { from_label: String, doc_name: String, size_bytes: u64 },
    AwaitingContent { from_label: String, doc_name: String, size_bytes: u64 },
    AwaitingUiWrite { from_label: String, title: String, content: String },
}

/// Stadio di un `OutgoingShare` (Slice 2a). A differenza di `PendingShare`, i due
/// stadi qui portano ESATTAMENTE gli stessi dati — un enum a due varianti duplicherebbe
/// gli stessi campi senza guadagno di type-safety, quindi `OutgoingShare` resta uno
/// struct con questo campo `stage` invece di diventare a sua volta un enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutgoingShareStage {
    /// Offerta mandata, in attesa di Accept/Reject/Expired dal destinatario.
    AwaitingAccept,
    /// Il destinatario ha accettato — in attesa che la UI fornisca il contenuto vero.
    AwaitingContent,
}

/// Un trasferimento Share INIZIATO da questa macchina, in attesa dell'esito dal
/// destinatario. `doc_name` serve SOLO a comporre un messaggio di esito leggibile
/// per l'umano (il backend non lo usa per nessuna decisione) — senza questo campo,
/// due condivisioni in volo verso la stessa macchina produrrebbero esiti indistinguibili.
/// `rel_path` (Slice 2a) serve a costruire `ServerMsg::ShareContentRequest` quando lo
/// stadio passa ad `AwaitingContent`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingShare {
    pub target_label: String,
    pub doc_name: String,
    pub rel_path: String,
    pub stage: OutgoingShareStage,
}

/// Stato dell'attore (posseduto da un solo task).
///
/// I campi sono privati: solo `handle_event` li muta.
/// In OOP sarebbe una classe con stato incapsulato e metodi che mutano lo stato.
/// Qui in Rust l'incapsulamento è garantito dal modulo: solo il codice in `service.rs`
/// può accedere direttamente ai campi.
///
pub struct AiChatService {
    /// La nostra identità: non cambia dopo la creazione.
    me: PeerInfo,
    /// Il canale di orchestrazione (decide ruolo server/client, gestisce la stanza).
    channel: AiChatChannel,
    /// Sink verso la UI connessa (per `Effect::ToUi`).
    /// `None` se nessuna UI è aperta; `Some` dopo `SetServerTx`.
    /// È un `UnboundedSender`: mandare un messaggio non blocca mai,
    /// ma il canale si chiude se il ricevitore (`_rx` / loop UI) viene droppato.
    server_tx: Option<UnboundedSender<ServerMsg>>,
    /// Peer attualmente connessi (link TCP vivo).
    connected: HashSet<PeerId>,
    /// Mappa di tutti i peer noti: `PeerId → PeerInfo`.
    ///
    /// Popolata da `Discovered` — TUTTI i peer scoperti via UDP, senza filtro di
    /// consenso (Task 8, ammissione alla stanza: rimpiazza il vecchio
    /// `consented`/`pending`/`refused` pairwise). Usata da `decide_and_connect` per
    /// costruire il `Roster` dell'elezione, e nei test da `mark_connected_for_test`
    /// per simulare connessioni dirette.
    peers: HashMap<PeerId, PeerInfo>,
    /// Mappa dei link TCP attivi verso i peer: `PeerId → sender del writer task`.
    ///
    /// Ogni voce corrisponde a una connessione TCP viva verso un peer. Il `UnboundedSender<ChatMsg>`
    /// è l'ingresso del *writer task* per quel peer: inviare un `ChatMsg` sul sender lo fa
    /// scrivere (come riga JSON) sul socket TCP. Quando il sender viene droppato (es. in
    /// `Disconnect`), il writer task termina perché il suo `UnboundedReceiver` chiude.
    links: HashMap<PeerId, UnboundedSender<ChatMsg>>,
    /// Flag di idempotenza per `Effect::StartListener`.
    ///
    /// `true` = il TcpListener è già stato bindato e l'accept loop è in esecuzione.
    /// `handle_event` emette `StartListener` ogni volta che decide_and_connect lo richiede
    /// (è pura, non sa se il listener è già attivo); `perform` usa questo flag per evitare
    /// di bindare due volte sulla stessa porta (che fallirebbe con "address already in use").
    listening: bool,
    /// Ultimo roster noto, aggiornato ogni volta che il roster cambia.
    ///
    /// Viene ri-emesso quando una nuova UI si connette (`SetServerTx`): se la finestra-chat
    /// viene chiusa e riaperta (stessa connessione WS), il nuovo webview parte senza
    /// "presenti" perché non ha ricevuto gli eventi precedenti. Questo campo funge da
    /// *snapshot del read model*: ripristina immediatamente i "presenti" alla riapertura.
    /// Aggiornato da:
    ///   - `PeerMsg::Join` (server: processa il join e ottiene il roster aggiornato)
    ///   - `PeerMsg::Roster` (client: riceve il roster dal server)
    last_known_roster: Vec<String>,
    /// Storico ordinato dei messaggi (`Say`) visti finora da questo peer — sia scritti
    /// localmente (`HumanSay`) sia ricevuti da un peer (`PeerMsg::Say`). Aggiornato
    /// incondizionatamente dal ruolo: ogni peer che "vede" un messaggio lo registra, così
    /// chi viene eletto server eredita lo storico completo senza handoff esplicito (vedi
    /// `Docs/superpowers/specs/2026-07-01-aichat-history-persistence-design.md` §3/§5).
    /// Ri-emesso alla UI su `SetServerTx` e spedito in blocco a un peer che manda `Join`
    /// (solo lato server).
    history: Vec<ChatLine>,
    /// Ultimo leader (server eletto) di cui abbiamo loggato l'elezione/il cambio.
    /// `None` finché non è ancora stata calcolata una prima elezione. Usato da
    /// `decide_and_connect` per loggare **una volta sola per cambio effettivo** — non ad
    /// ogni `Discovered` periodico che riconferma lo stesso leader (vedi §6 dello spec
    /// storico). Gira identico su ogni peer: il log compare su TUTTI gli orchestrator
    /// della stanza, non solo su chi diventa server.
    last_logged_leader: Option<PeerId>,
    /// Ultimo leader che un peer scoperto ha annunciato di credere valido (Bug 2,
    /// late-joiner — vedi
    /// `Docs/superpowers/specs/2026-07-02-aichat-late-joiner-election-design.md` §3.3).
    /// `None` finché nessun annuncio ricevuto lo riporta. Aggiornato SOLO quando un
    /// annuncio riporta `Some(_)` (un annuncio con `leader: None` — peer ancora
    /// indeciso — non deve cancellare un'informazione più utile già ricevuta da un
    /// altro peer, vedi l'arm `Discovered`). Ripulito su `PeerGone` se il peer sparito
    /// era proprio il leader riportato (altrimenti, dopo la sparizione del vero
    /// leader, la rielezione tra i superstiti resterebbe bloccata su `Undecided`
    /// invece di ricadere su `members.lowest()` — regressione da evitare, vedi §3.4).
    /// Passato a `channel.decide_role` da `decide_and_connect`.
    reported_leader: Option<PeerId>,
    /// Handle condiviso, letto (in sola lettura) dal task di scoperta UDP in `main.rs`
    /// per includere il leader ATTUALMENTE creduto da noi nei propri annunci
    /// (`Discoverer::announce(leader)`). Scritto qui in `decide_and_connect`, nello
    /// stesso punto in cui si calcola `current_leader` per il log di elezione — vedi
    /// §3.6 dello spec late-joiner. `Arc<Mutex<..>>` perché il task di scoperta gira
    /// in uno `spawn` separato dall'attore (comunicano solo via l'inbox in entrata);
    /// questo è l'UNICO stato condiviso mutabile del canale AI Chat, deliberatamente
    /// isolato qui invece di generalizzare il pattern attore.
    believed_leader: Arc<Mutex<Option<PeerId>>>,

    // ---------------------------------------------------------------------------
    // Debito #4 (Slice C) — generation token dei link, per risolvere la race di
    // riconnessione (`PeerGone` stale che rimuove un link fresco).
    // ---------------------------------------------------------------------------
    /// Contatore monotono di generazione dei link — SOLO attore.
    ///
    /// Ogni volta che un link viene registrato (accept in ingresso, connect in uscita
    /// riuscito, o gli helper di test) riceve una generazione UNICA da questo contatore
    /// (`let gen = self.next_gen; self.next_gen += 1;`).
    ///
    /// ### Slice C2 — perché non è più un `Arc<AtomicU64>` condiviso
    ///
    /// Fino alla 0.28.0 era un `Arc<AtomicU64>` (stesso pattern di `believed_leader`
    /// sopra), perché l'accept loop e il task di connessione — entrambi SPAWNATI, senza
    /// `&mut self` — allocavano la propria generazione autonomamente e la usavano
    /// SUBITO per chiamare `spawn_peer_tasks` su se stessi, prima che l'attore avesse
    /// scritto `self.link_gen`. Questo apriva la race residua che Slice C2 chiude: un
    /// `PeerGone` emesso dal reader appena spawnato poteva arrivare all'attore PRIMA
    /// della registrazione dello stato (vedi `register_link`). Ora l'allocazione E la
    /// scrittura in `self.link_gen` avvengono ENTRAMBE nell'attore (con `&mut self`),
    /// SEMPRE prima di `spawn_peer_tasks` — un `Arc` condiviso non serve più: un campo
    /// `u64` di proprietà esclusiva dell'attore basta.
    next_gen: u64,
    /// Generazione del link ATTUALMENTE vivo per ogni peer connesso.
    ///
    /// A differenza del contatore sopra, questa mappa è privata all'attore: solo
    /// `handle_event`/`register_link`/`register_connect_success` la leggono e scrivono.
    /// Confrontata con la generazione portata da `ServiceEvent::PeerGone` per distinguere
    /// un evento genuino (combacia) da uno stale (si riferisce a un link già sostituito).
    link_gen: HashMap<PeerId, u64>,
    /// Peer per cui un task di connessione (Debito #3, Slice C) è attualmente in volo.
    ///
    /// Guard anti-duplicato per `Effect::ConnectTo`: un peer già `connecting` (o già
    /// `connected`) non avvia un secondo tentativo — senza questo, un `Discovered`
    /// periodico che arriva mentre un connect lento è ancora in corso aprirebbe
    /// connessioni concorrenti verso lo stesso peer.
    connecting: HashSet<PeerId>,
    /// La "propria" AI locale (AI Chat Slice 1a — vedi
    /// `Docs/superpowers/specs/2026-07-02-aichat-ai-participants-design.md`): lo stesso
    /// `Arc<dyn AiAdapter>` già costruito in `main.rs` per il cursore (StubAdapter senza
    /// API key, LlmAdapter con chiave). Usata SOLO da `perform` per
    /// `Effect::InvokeLocalAi` — mai chiamata da `handle_event` (che resta puro, niente
    /// I/O: vedi il doc-comment in cima al file). `Arc` perché il task spawnato in
    /// `perform` per gestire l'effetto vive indipendentemente da `self`.
    ai_adapter: Arc<dyn AiAdapter>,
    /// AI Chat Slice 1b — flag "la mia AI partecipa" (design §9.4, dal campo
    /// `AiChatConfig::ai_participates`, letto UNA volta all'avvio in `main.rs`
    /// e passato qui: NON è "live", coerente con gli altri campi di
    /// `aichat.json`). Gata SOLO le invocazioni `@all`/`@<mio-label>-ai`
    /// provenienti da un UMANO REMOTO (`PeerMsg::Say` con `from_label` che
    /// finisce in `-human`) — l'invocazione della propria macchina (arm
    /// `HumanSay`) non lo consulta mai: scrivere nella propria finestra È già
    /// il consenso. Preserva l'autonomia di ogni macchina: nessuna macchina fa
    /// girare l'AI di un'altra senza il proprio consenso esplicito.
    ai_participates: bool,
    /// AI Chat Slice 2 — flag "auto-partecipazione" (design §10.1, dal campo
    /// `AiChatConfig::ai_autoparticipate`, letto UNA volta all'avvio, NON "live" —
    /// stesso pattern di `ai_participates`). A differenza di `ai_participates`
    /// (risponde a un'invocazione ESPLICITA), questo flag fa parlare l'AI
    /// SPONTANEAMENTE sui messaggi normali della stanza (`maybe_autoparticipate`).
    /// Default `false` (opt-in): è la prima guardia — con questo flag `false` nessuno
    /// degli altri campi qui sotto entra mai in gioco.
    ai_autoparticipate: bool,
    /// Contatore di turni `-ai` CONSECUTIVI visti in questa stanza (il "cap" —
    /// design §10.3). Aggiornato da `note_room_message`: azzerato da un messaggio
    /// `-human`, incrementato da un messaggio `-ai` (mio o di un altro peer — il
    /// contatore è per-STANZA, non "quante volte HO parlato io"). Confrontato con
    /// `MAX_CONSECUTIVE_AI_TURNS` in `maybe_autoparticipate`: è ciò che rende il burst
    /// AI↔AI BOUNDED anche se ogni macchina lo traccia in modo indipendente/locale
    /// (nessuna sincronizzazione esplicita del contatore tra i peer — un piccolo
    /// drift tra le macchine è tollerato, il bound regge comunque, vedi il
    /// doc-comment di `MAX_CONSECUTIVE_AI_TURNS`).
    consecutive_ai_turns: usize,
    /// Etichetta (`from_label`) dell'ULTIMO messaggio visto in questa stanza,
    /// aggiornata da `note_room_message`. Usata dalla guardia "non due volte di
    /// fila"/"non rispondo a me stesso" in `maybe_autoparticipate`: se l'ultimo a
    /// parlare sono stato IO (la mia stessa `-ai`), non mi auto-invito a
    /// contribuire di nuovo subito dopo. `None` finché non è ancora passato alcun
    /// messaggio (stato iniziale del servizio).
    last_speaker_label: Option<String>,
    /// `true` se un giudizio di auto-partecipazione (`Effect::AutoParticipate`) è
    /// ATTUALMENTE in volo (task spawnato, risposta non ancora tornata). Guardia
    /// anti-concorrenza/anti-costo (design §10.3): finché `true`, `maybe_autoparticipate`
    /// non innesca un secondo giudizio — un solo turno AI "in giudizio" per volta per
    /// macchina, indipendentemente da quanti messaggi arrivano nel frattempo. Azzerato
    /// SEMPRE da `ServiceEvent::AutoParticipateDone` (sia `Some` sia `None` — vedi il
    /// doc-comment di quella variante: senza questo azzeramento incondizionato un
    /// singolo "silenzio" dell'AI bloccherebbe l'auto-partecipazione per sempre).
    autoparticipate_inflight: bool,

    /// Slice 2 (memoria persistente): lock contro la scrittura concorrente su
    /// `memory-{label_base}.md` — `chat_reply` (invocazione esplicita) e
    /// `chat_autoparticipate` NON sono mutuamente esclusi (solo autoparticipate-vs-
    /// autoparticipate lo è, via `autoparticipate_inflight`), quindi due eventi "MEMORIA:"
    /// quasi simultanei potrebbero altrimenti fare un read-modify-write non atomico e
    /// perdere una delle due note. Un solo lock per macchina (un file per label_base).
    /// `std::sync::Mutex`, non `tokio::sync::Mutex`: la sezione critica (leggi-accoda-scrivi)
    /// è I/O sincrono (`std::fs`), nessun `.await` mentre il lock è tenuto — stesso principio
    /// già in uso per `believed_leader` in questo stesso struct.
    memory_write_lock: Arc<Mutex<()>>,

    // -------------------------------------------------------------------------
    // Ammissione alla stanza (Task 3-8, vedi il blocco di tipi sopra
    // `AiChatService`): sostituisce il vecchio consenso pairwise
    // `pending`/`consented`/`refused` (rimosso in Task 8).
    // -------------------------------------------------------------------------
    /// Il nostro stato come "nuovo arrivato" — letto/scritto in produzione da
    /// `begin_join_gate`/`request_admission`/gli arm `JoinDecision`,
    /// `RequestAdmissionUi` e `PeerMsg{Admitted|AdmissionRejected}` (Task 6).
    self_admission: SelfAdmission,
    /// (Solo server) i voti di ammissione in corso, per candidato (Task 4/5).
    pending_votes: HashMap<PeerId, PendingVote>,
    /// (Solo server) i `PeerId` dei client GIÀ AMMESSI alla stanza — cioè quelli
    /// che hanno superato il voto di ammissione (Task 5, `resolve_if_ready` →
    /// admit). NON include: il candidato che sta chiedendo di entrare in
    /// questo momento (non è ancora ammesso, ovviamente); il proprio umano
    /// server (che non è un "client" — vota a parte, direttamente via UI, non
    /// tramite un link TCP); un client `connected` ma non ancora ammesso (un
    /// altro voto ancora in corso, o mai partito).
    ///
    /// Raffinamento del supervisore rispetto alla bozza iniziale del piano
    /// (che usava `self.connected` come base dei "presenti" da consultare):
    /// `connected` include ANCHE il candidato appena arrivato (il suo link
    /// TCP è già up quando manda `RequestAdmission`) e client non ancora
    /// ammessi — nessuno dei due ha titolo per votare. `admitted` è la base
    /// corretta per calcolare i "presenti" quando parte un nuovo voto (vedi
    /// `start_admission_vote`).
    admitted: HashSet<PeerId>,
    /// Trasferimenti Share INIZIATI da questa macchina, non ancora risolti: `share_id` →
    /// `OutgoingShare { target_label, doc_name, rel_path, stage }`. Due stadi (Slice 2a,
    /// spec §9.1): `AwaitingAccept` (in attesa di Accept/Reject/Expired dal destinatario)
    /// e `AwaitingContent` (accettata, in attesa che la UI fornisca il contenuto vero).
    /// Rimosso solo su Reject/Expired/fallimento contenuto, oppure sull'invio riuscito
    /// del contenuto (`handle_share_content`) — NON sulla sola accettazione.
    outgoing_shares: HashMap<String, OutgoingShare>,
    /// Trasferimenti Share RICEVUTI, in attesa di una decisione (mia o, in una slice
    /// futura, dell'AI locale) — chiave `share_id`. Cap `MAX_PENDING_SHARES` (spec §8).
    pending_shares: HashMap<String, PendingShare>,
    /// Contatore monotono per generare `share_id` univoci (`"{ip}:{n}"`) senza
    /// randomness — stesso principio di `next_gen` per le generazioni di link.
    /// `handle_event` resta pura: nessun uso di `rand`/UUID qui.
    next_share_id: u64,
    /// (Solo server, FIX #5 — review 2026-07-03) i candidati attualmente in
    /// COOLDOWN di re-request: rifiutati da poco, un loro nuovo `RequestAdmission`
    /// va ignorato finché il cooldown non scade (`ServiceEvent::CooldownExpired`,
    /// dal timer di `Effect::StartCooldownTimer`). Sostituisce la fiducia cieca
    /// nella UI: un peer che manda il wire direttamente non può più riaprire voti
    /// a raffica. Un candidato ci entra in `resolve_reject`, esce in
    /// `CooldownExpired` (o in `PeerGone`, per igiene: un peer sparito non deve
    /// lasciare una voce residua).
    cooling_down: HashSet<PeerId>,
    /// (Solo server, FIX #1 — review 2026-07-03) contatore monotono delle
    /// generazioni di voto: incrementato a ogni `start_admission_vote`, il valore
    /// assegnato finisce nel `PendingVote::generation` e nel timer del voto. Rende
    /// univoco ogni turno così che un `VoteTimeout` in ritardo (di un turno ormai
    /// risolto) non risolva un turno successivo per lo stesso candidato. Basta un
    /// `u64` monotono: nessun bisogno di riuso, un turno alla volta in pratica.
    next_vote_gen: u64,

    // -------------------------------------------------------------------------
    // Library "Blocco note" (Task 7 — design §5.1/§8).
    // -------------------------------------------------------------------------
    /// Archivio delle note, di proprietà dell'orchestrator (sopravvive a UI
    /// chiusa/riavvio): stesso pattern di `history` per la chat. `handle_event`
    /// muta questo campo IN MEMORIA (nessun I/O reale — vedi il doc-comment del
    /// pattern attore in cima al file); la scrittura vera su `notes.json` avviene
    /// in `perform`, innescata da `Effect::SaveNotes`.
    notes: NotesStore,
    /// Contatore monotono per generare `note_id` univoci (`"{ip}:{n}"`), stesso
    /// schema di `next_share_id` — nessuna dipendenza `uuid` nel codebase.
    ///
    /// A DIFFERENZA di `next_share_id` (che può ripartire da 0 a ogni avvio: uno
    /// `share_id` vive solo per la durata di un trasferimento), questo contatore
    /// è SEEDATO in `new()` dal massimo contatore già presente nello store
    /// caricato da disco — altrimenti la prima nota creata dopo un riavvio
    /// collide con una nota esistente e `upsert_merged` la FONDE invece di
    /// crearla (perdita di dati silenziosa: v. il commento in `new()`).
    next_note_id: u64,
    /// Orologio iniettabile per i test (Task 7): `None` in produzione (si usa
    /// sempre `now_ms()` reale); un test che ha bisogno di un timestamp
    /// deterministico lo imposta con il setter `#[cfg(test)]`
    /// `set_wallclock_ms_for_test` (usato dal test del tie-break LWW sul
    /// titolo). Stesso principio del "clock iniettato" di `PeerTable::observe`.
    wallclock_ms_for_test: Option<u64>,
    /// Lock contro la scrittura concorrente su `notes.json` (fix di review,
    /// Task 7) — gemello di `memory_write_lock` sopra, stesso motivo: due
    /// `Effect::SaveNotes` ravvicinati spawnano due task che altrimenti
    /// potrebbero interlacciare la scrittura del file. `std::sync::Mutex`
    /// (non `tokio::sync::Mutex`): la sezione critica è I/O sincrono
    /// (`std::fs::write` dentro `NotesStore::save`), nessun `.await` mentre il
    /// lock è tenuto.
    notes_write_lock: Arc<Mutex<()>>,

    // -------------------------------------------------------------------------
    // Task 3 (display names): nickname umano/AI di QUESTA macchina, letti da
    // `network.json` in `main.rs` e passati qui UNA volta all'avvio (NON "live",
    // stesso pattern di `ai_participates`/`ai_autoparticipate` sopra). Usati SOLO
    // da `publish_say` per stampare `ChatLine::display_name`/`ChatMsg::Say::display_name`
    // sui MIEI messaggi — mai per i messaggi di un peer remoto (quelli arrivano già
    // stampati, vedi l'arm `ServiceEvent::PeerMsg { msg: ChatMsg::Say, .. }`, che fa
    // solo pass-through).
    // -------------------------------------------------------------------------
    /// Nickname dell'umano di questa macchina (network.json, Task 1) — usato da
    /// `publish_say` per popolare `ChatLine::display_name`/`ChatMsg::Say::display_name`
    /// quando `from_label` finisce in "-human". `None` → nessun nickname configurato,
    /// il frontend mostra `from_label` come oggi.
    display_name: Option<String>,
    /// Nickname dell'AI di questa macchina (network.json, Task 1) — stesso ruolo di
    /// `display_name` ma per i messaggi con `from_label` che finisce in "-ai".
    ai_display_name: Option<String>,
    /// Cartella di configurazione (2.0, D6) — risolta una volta in `main()`
    /// (`RuntimeConfig::config_dir`) e portata qui a costruzione: usata SOLO
    /// da `Effect::PersistMemory` (memoria persistente `MEMORIA:`, vedi
    /// `append_memory_note`/`memory_file_path` sotto), mai ri-derivata da
    /// questo servizio con una propria lettura di env/`startup.json`.
    config_dir: std::path::PathBuf,
}

impl AiChatService {
    /// Crea un nuovo servizio per il peer `me`, con la propria AI locale (`ai_adapter`)
    /// usata per rispondere alle invocazioni `@ai`/`@all`/`@<label>-ai` nella stanza
    /// (Slice 1a/1b), `ai_participates` (Slice 1b, design §9.4) che gata SOLO le
    /// invocazioni remote (`@all`/`@<mio-label>-ai` da un umano di un'altra macchina —
    /// vedi il doc-comment del campo `ai_participates`), e `ai_autoparticipate`
    /// (Slice 2, design §10.1) che abilita l'intervento SPONTANEO dell'AI sui
    /// messaggi normali della stanza (vedi il doc-comment del campo omonimo), e
    /// `notes` (Task 7, Library "Blocco note") — l'archivio già caricato da disco
    /// (o `NotesStore::empty_in_memory()` nei test): questo costruttore non fa
    /// I/O da solo, si limita a immagazzinare quello che il chiamante gli passa
    /// già pronto (`main.rs` chiama `NotesStore::load_or_generate` PRIMA di
    /// costruire il servizio). `display_name`/`ai_display_name` (Task 3, display
    /// names) sono i nickname di questa macchina letti da `network.json` — vedi
    /// i doc-comment dei campi omonimi.
    ///
    /// Come `new()` in OOP: alloca e inizializza lo stato; non fa I/O.
    // 8 parametri (era 7 prima di `config_dir`, Task 4): stesso compromesso
    // già accettato in `ws::serve`/`handle_connection` — un builder/struct-
    // literal di config sarebbe un refactor più ampio, fuori scope qui.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        me: PeerInfo,
        ai_adapter: Arc<dyn AiAdapter>,
        ai_participates: bool,
        ai_autoparticipate: bool,
        notes: NotesStore,
        display_name: Option<String>,
        ai_display_name: Option<String>,
        config_dir: std::path::PathBuf,
    ) -> Self {
        let channel = AiChatChannel::new(me.clone());

        // FIX 1 (review finale di branch): il contatore degli id nota NON può
        // ripartire da 0 quando lo store arriva già popolato da disco.
        //
        // `main.rs` carica `notes.json` PRIMA di costruire il servizio; se dopo un
        // riavvio ripartissimo da 0, la prima nota creata riuserebbe l'id
        // `"<mio-ip>:0"` di una nota già esistente. `upsert_merged` non inserisce
        // su id già noto: FONDE — e il merge scarterebbe il testo nuovo (il vecchio
        // segmento ha `seq` maggiore), sovrascriverebbe il titolo via LWW, e se la
        // vecchia era un tombstone la nota "nuova" nascerebbe già cancellata. Tutto
        // in silenzio, e per giunta ribroadcastato a ogni altra macchina.
        //
        // Ricaviamo quindi il punto di partenza dall'archivio stesso: massimo
        // contatore fra gli id CHE PORTANO IL MIO PREFISSO, +1. Gli id delle altre
        // macchine non ci riguardano (prefisso diverso → spazio di nomi diverso).
        // Nota: il prefisso è l'IP (`me.id.0`), NON `label_base` — è lo schema
        // scelto in `next_note_id`/`fresh_share_id`. Se l'IP cambia (DHCP), il
        // prefisso cambia con lui e il contatore riparte da 0 su uno spazio di
        // nomi vergine: nessuna collisione possibile, per costruzione.
        let my_id_prefix = format!("{}:", me.id.0);
        let next_note_id = notes
            .all()
            .iter()
            .filter_map(|n| n.id.strip_prefix(&my_id_prefix))
            .filter_map(|counter| counter.parse::<u64>().ok())
            .max()
            .map_or(0, |max| max + 1);

        Self {
            me,
            channel,
            server_tx: None,
            connected: HashSet::new(),
            peers: HashMap::new(),
            links: HashMap::new(),
            listening: false,
            last_known_roster: Vec::new(),
            history: Vec::new(),
            last_logged_leader: None,
            reported_leader: None,
            believed_leader: Arc::new(Mutex::new(None)),
            next_gen: 0,
            link_gen: HashMap::new(),
            connecting: HashSet::new(),
            ai_adapter,
            ai_participates,
            ai_autoparticipate,
            consecutive_ai_turns: 0,
            last_speaker_label: None,
            autoparticipate_inflight: false,
            memory_write_lock: Arc::new(Mutex::new(())),
            self_admission: SelfAdmission::NotJoining,
            pending_votes: HashMap::new(),
            admitted: HashSet::new(),
            outgoing_shares: HashMap::new(),
            pending_shares: HashMap::new(),
            next_share_id: 0,
            cooling_down: HashSet::new(),
            next_vote_gen: 0,
            notes,
            next_note_id, // seedato sopra dallo store già caricato (FIX 1), MAI 0 fisso
            wallclock_ms_for_test: None,
            notes_write_lock: Arc::new(Mutex::new(())),
            display_name,
            ai_display_name,
            config_dir,
        }
    }

    /// Costruttore di comodo per i test PURI di `handle_event`/`perform` che non
    /// riguardano `chat_reply`/`chat_autoparticipate`: usa lo `StubAdapter` (risposta
    /// deterministica, nessuna chiamata HTTP), `ai_participates = true`
    /// (comportamento "partecipativo" atteso dalla maggioranza dei test — solo i
    /// pochi test di Slice 1b che esercitano il flag OFF usano
    /// `new_for_test_with_participation` sotto) e `ai_autoparticipate = false`
    /// (Slice 2 è opt-in: i test che la esercitano usano
    /// `new_for_test_with_autoparticipate` sotto). Evita di dover passare
    /// `Arc<dyn AiAdapter>`/i flag espliciti in decine di call-site che non testano
    /// quel comportamento specifico.
    #[cfg(test)]
    pub fn new_for_test(me: PeerInfo) -> Self {
        Self::new(me, Arc::new(crate::ai_adapter::StubAdapter), true, false, NotesStore::empty_in_memory(), None, None, "unused".into())
    }

    /// Come `new_for_test`, ma con `ai_participates` esplicito — usato dai test di AI
    /// Chat Slice 1b che verificano il gate sul flag (invocazione remota `@all`/
    /// `@<mio-label>-ai` con `ai_participates = false`). `ai_autoparticipate` resta
    /// `false` (non è oggetto di questi test).
    #[cfg(test)]
    pub fn new_for_test_with_participation(me: PeerInfo, ai_participates: bool) -> Self {
        Self::new(me, Arc::new(crate::ai_adapter::StubAdapter), ai_participates, false, NotesStore::empty_in_memory(), None, None, "unused".into())
    }

    /// Come `new_for_test`, ma con `ai_autoparticipate` esplicito — usato dai test di
    /// AI Chat Slice 2 (trigger/cap/guardie della auto-partecipazione).
    /// `ai_participates` resta `true` (non è oggetto di questi test).
    #[cfg(test)]
    pub fn new_for_test_with_autoparticipate(me: PeerInfo, ai_autoparticipate: bool) -> Self {
        Self::new(me, Arc::new(crate::ai_adapter::StubAdapter), true, ai_autoparticipate, NotesStore::empty_in_memory(), None, None, "unused".into())
    }

    /// Come `new_for_test`, ma con `display_name`/`ai_display_name` espliciti —
    /// usato dai test che verificano la risoluzione del nickname in `publish_say`
    /// (Task 3, feature display-names). `ai_participates`/`ai_autoparticipate`
    /// restano ai default di `new_for_test` (non oggetto di questi test).
    #[cfg(test)]
    pub fn new_for_test_with_display_names(
        me: PeerInfo,
        display_name: Option<String>,
        ai_display_name: Option<String>,
    ) -> Self {
        Self::new(
            me,
            Arc::new(crate::ai_adapter::StubAdapter),
            true,
            false,
            NotesStore::empty_in_memory(),
            display_name,
            ai_display_name,
            "unused".into(),
        )
    }

    /// Restituisce un clone dell'`Arc` interno che porta il leader ATTUALMENTE
    /// creduto da questo nodo. Letto dal task di scoperta UDP (`main.rs`) a ogni
    /// `announce()`, così l'annuncio in uscita porta sempre il valore aggiornato
    /// senza che il task debba avere accesso diretto allo stato dell'attore (che
    /// vive esclusivamente in questo `run` loop — vedi il pattern attore in cima
    /// al file).
    pub fn believed_leader_handle(&self) -> Arc<Mutex<Option<PeerId>>> {
        Arc::clone(&self.believed_leader)
    }

    /// Ritorna `true` se c'è una UI connessa (il `server_tx` è vivo).
    pub fn has_ui(&self) -> bool {
        self.server_tx.is_some()
    }

    /// Processa un evento e ritorna gli effetti da eseguire. **PURA** (nessun I/O).
    ///
    /// "Pura" in questo contesto significa: niente socket, niente `await`, niente
    /// operazioni su file — solo mutazioni di `self` e costruzione di `Vec<Effect>`.
    /// Questo la rende testabile in isolamento totale, senza infrastruttura di rete.
    ///
    /// Analogia OOP: è come un metodo di un Command Processor che trasforma lo stato
    /// e produce una lista di comandi da eseguire (Command Pattern + Event Sourcing).
    pub fn handle_event(&mut self, ev: ServiceEvent) -> Vec<Effect> {
        match ev {
            // --- Gestione connessione UI ---

            // Una nuova UI si è connessa: memorizziamo il suo sink.
            // Non produciamo effetti: la UI riceverà aggiornamenti al prossimo evento
            // rilevante (es. un peer che si annuncia, o un messaggio in arrivo).
            ServiceEvent::SetServerTx(tx) => {
                self.server_tx = Some(tx);
                // La finestra-chat deve sempre sapere "chi sono io" (titolo finestra) —
                // incondizionato, a differenza di roster/storico sotto, che si
                // ri-emettono solo se c'è qualcosa da mostrare.
                let mut effects: Vec<Effect> = vec![Effect::ToUi(ServerMsg::AiChatSelf {
                    label: format!("{}-human", self.me.label_base),
                })];
                // Task 8 (ammissione alla stanza): qui c'era la ri-emissione delle
                // richieste di CONSENSO pairwise ancora pendenti (`self.pending`) — rimossa
                // insieme a quel flusso (vedi l'arm `Discovered`).
                //
                // Bug fix (2026-07-04, verificato dal vivo su 2 macchine): il connect verso
                // il server eletto parte da solo all'avvio dell'orchestrator, quasi sempre
                // PRIMA che l'umano apra la finestra chat — `begin_join_gate`/
                // `request_admission` emettevano il loro `ToUi` mentre `server_tx` era
                // ancora `None` (scartato in silenzio da `perform`), lasciando il nuovo
                // arrivato bloccato in `Deciding`/`Pending` SENZA alcun prompt visibile e
                // SENZA modo di recuperarlo aprendo la finestra più tardi (nessun replay
                // esisteva). Stesso principio del replay di roster/storico sotto: se siamo
                // a metà del nostro STESSO gate 1/2, ri-mostralo. Resta un gap NON coperto
                // (fuori scope qui): il gate 2 di un PRESENTE che deve ancora votare
                // (`self.pending_votes`, lato server) non viene ri-emesso alla riapertura —
                // da valutare in una slice successiva.
                match self.self_admission {
                    SelfAdmission::Deciding => {
                        effects.push(Effect::ToUi(ServerMsg::AiChatJoinPrompt {
                            present: self.known_peer_labels(),
                        }));
                    }
                    SelfAdmission::Pending => {
                        effects.push(Effect::ToUi(ServerMsg::AiChatPending {
                            present: self.known_peer_labels(),
                        }));
                    }
                    SelfAdmission::NotJoining | SelfAdmission::Admitted | SelfAdmission::Rejected => {}
                }
                // Gap chiuso (2026-07-28, bug scoperto dal vivo via Library "Condividi"):
                // il gate 1 sopra ri-mostra self come CANDIDATO in attesa; qui ri-mostriamo
                // self come SERVER che deve ancora votare — per ogni voto ancora aperto
                // (`self.pending_votes`) su cui self.me non ha ancora espresso il proprio
                // voto. Senza questo, una richiesta di ammissione arrivata a finestra
                // chiusa non veniva MAI vista dal server (si risolveva solo al timeout
                // "silenzio = sì", mai per un voto consapevole).
                for pv in self.pending_votes.values() {
                    if !pv.votes.contains_key(&self.me.id) {
                        effects.push(Effect::ToUi(ServerMsg::AiChatAdmissionRequest {
                            candidate: pv.label.clone(),
                        }));
                    }
                }
                //
                // Re-invia l'ultimo roster noto se la UI si sta riconnettendo (finestra riaperta).
                // Senza questo, un webview aperto dopo la chiusura della finestra-chat partirebbe
                // con "presenti" vuoti pur essendo il canale TCP ancora attivo.
                // Con `last_known_roster` vuoto (primo avvio o nessun Join ancora) non produce
                // effetti aggiuntivi: comportamento invariato rispetto alla versione precedente.
                if !self.last_known_roster.is_empty() {
                    effects.push(Effect::ToUi(ServerMsg::AiChatRoster {
                        participants: self.last_known_roster.clone(),
                    }));
                }
                // Library "Condividi" (spec 2026-07-28): a differenza del roster
                // sopra, questo va ri-emesso SEMPRE, anche a lista vuota — la UI
                // deve poter distinguere "vuota confermata" da "non ancora arrivata".
                effects.push(Effect::ToUi(ServerMsg::AiChatReachablePeers {
                    labels: self.reachable_peer_labels(),
                }));
                // Blocco note (design §7): replay incondizionato, stesso principio di
                // AiChatReachablePeers — la UI deve poter distinguere "vuoto confermato"
                // da "non ancora arrivato".
                effects.push(Effect::ToUi(ServerMsg::NotesSnapshot {
                    notes: self.notes.all().iter().map(|n| self.to_note_view(n)).collect(),
                }));
                // Stesso principio per lo storico messaggi: se non vuoto, ri-emettilo in blocco
                // così il trascritto si ricostruisce subito alla riapertura della finestra.
                if !self.history.is_empty() {
                    effects.push(Effect::ToUi(ServerMsg::AiChatHistory {
                        entries: self.history.iter().cloned().map(Into::into).collect(),
                    }));
                }
                // Bug fix (2026-07-05, spec §9.1): ri-emette il banner di consenso Share
                // per ogni offerta ancora in attesa di una mia decisione — stesso
                // principio del replay di roster/storico/gate di ammissione sopra. Senza
                // questo, un'offerta arrivata a finestra chiusa resterebbe invisibile per
                // sempre anche riaprendo la finestra più tardi (fino alla scadenza 24h).
                // Slice 2a: `pending_shares` ora ha TRE stadi — replay diverso per
                // ciascuno, stesso principio di roster/storico/gate di ammissione sopra.
                for (share_id, ps) in &self.pending_shares {
                    match ps {
                        PendingShare::AwaitingDecision { from_label, doc_name, size_bytes } => {
                            effects.push(Effect::ToUi(ServerMsg::ShareRequest {
                                share_id: share_id.clone(),
                                from_label: from_label.clone(),
                                doc_name: doc_name.clone(),
                                size_bytes: *size_bytes,
                            }));
                        }
                        // Già decisa (accettata), in attesa del contenuto vero — niente da
                        // rimostrare, il banner di consenso non deve ricomparire.
                        PendingShare::AwaitingContent { .. } => {}
                        PendingShare::AwaitingUiWrite { from_label, title, content } => {
                            effects.push(Effect::ToUi(ServerMsg::ShareIncomingData {
                                share_id: share_id.clone(),
                                from_label: from_label.clone(),
                                title: title.clone(),
                                content: content.clone(),
                            }));
                        }
                    }
                }
                // Slice 2a: replay di ShareContentRequest per ogni outgoing_shares
                // ancora AwaitingContent — stesso principio del replay sopra.
                for (share_id, os) in &self.outgoing_shares {
                    if os.stage == OutgoingShareStage::AwaitingContent {
                        effects.push(Effect::ToUi(ServerMsg::ShareContentRequest {
                            share_id: share_id.clone(),
                            rel_path: os.rel_path.clone(),
                        }));
                    }
                }
                effects
            }

            // La UI si è chiusa: droppiamo il sink. Il canale `mpsc` si chiude
            // automaticamente quando non ci sono più sender o receiver.
            ServiceEvent::UiClosed => {
                self.server_tx = None;
                Vec::new()
            }

            // --- Task 8 (ammissione alla stanza): Discovered — solo scoperta ---
            //
            // Un peer si è annunciato via UDP. FINO al Task 8 questo evento apriva anche
            // un gate di CONSENSO pairwise/asimmetrico (ogni macchina chiedeva al proprio
            // umano "ammetti X?", indipendentemente dalle altre — vedi
            // `Docs/superpowers/specs/2026-07-03-aichat-admission-consent-design.md` §1
            // per i problemi che questo causava: "saluto perso", testo fuorviante). Quel
            // gate è ora SOSTITUITO dal voto di ammissione coordinato dal server (Task
            // 4/5) + il gate 1 del nuovo arrivato (Task 6, `begin_join_gate`): entrambi
            // scattano DOPO la connessione TCP, non prima dell'elezione.
            //
            // `Discovered` fa quindi solo due cose: aggiorna la mappa dei peer noti
            // (`self.peers`, usata da `decide_and_connect` per l'elezione — vedi il suo
            // doc-comment) e rieletti/riconnetti. Niente più `pending`/`consented`/
            // `refused`: chiunque sia scoperto sulla LAN partecipa al calcolo di chi è
            // il server, senza bisogno di un consenso preventivo per-peer.
            ServiceEvent::Discovered(peer, reported) => {
                // Bug 2 (late-joiner): registra il leader che QUEL peer ha riportato di
                // credere valido — PRIMA di ricalcolare l'elezione, perché anche un peer
                // appena scoperto porta un'informazione utile. Aggiorna SOLO se
                // `Some(_)`: un annuncio con `leader: None` (peer ancora indeciso) non
                // deve cancellare un'informazione più utile già ricevuta da un altro peer.
                if let Some(leader) = reported {
                    self.reported_leader = Some(leader);
                }

                // Log "una sola volta" alla prima scoperta: gli annunci UDP arrivano ogni
                // ~7s, quindi loggare a ogni `Discovered` allagherebbe lo stderr. Confrontiamo
                // PRIMA di inserire: `is_new` è vero solo finché il peer non è nella mappa.
                // (Dopo un `PeerGone` il peer esce da `peers`, quindi una riscoperta genuina
                //  ri-logga una volta — comportamento voluto.)
                let is_new = !self.peers.contains_key(&peer.id);
                // Aggiorna (o inserisce) le informazioni del peer nella mappa locale.
                // `peer.clone()` perché `peer.id` e `peer.label_base` ci servono ancora.
                self.peers.insert(peer.id, peer.clone());
                if is_new {
                    tracing::info!(
                        "aichat: peer scoperto {} (label={})",
                        peer.id.0, peer.label_base
                    );
                }

                // Rieletti SEMPRE: `decide_and_connect` ora legge `self.peers` (appena
                // aggiornata sopra), non un set di "consentiti" — vedi il suo doc-comment.
                let mut effects = self.decide_and_connect();
                // Library "Condividi" (spec 2026-07-28): ri-emesso SEMPRE, anche se
                // ancora vuoto — copre la race discovery-vs-link (un link può
                // stabilirsi prima che il relativo Discovered arrivi).
                effects.push(Effect::ToUi(ServerMsg::AiChatReachablePeers {
                    labels: self.reachable_peer_labels(),
                }));
                effects
            }

            // --- Task 6: PeerGone — un peer si è disconnesso ---
            //
            // Il reader TCP per quel peer ha ricevuto EOF, errore, O il timeout di
            // silenzio keepalive è scaduto (Task 8) — stesso evento, stesso trattamento.
            // Puliamo lo stato, annunciamo la sparizione, e rieletti il cluster.
            ServiceEvent::PeerGone(id, gen) => {
                // Debito #4 (Slice C): guardia di generazione — PRIMA di qualunque altra
                // cosa. Confrontiamo la generazione morta (`gen`, portata dall'evento) con
                // quella ATTUALMENTE registrata per questo peer (`self.link_gen`). Se non
                // combaciano, questo evento si riferisce a un link già rimpiazzato da una
                // riconnessione più recente (race: il reader del link vecchio ha impiegato
                // più tempo ad accorgersi della propria morte di quanto ce ne sia voluto
                // per aprire il nuovo link) — no-op totale, nessuna mutazione di stato.
                if self.link_gen.get(&id) != Some(&gen) {
                    tracing::debug!(
                        "aichat: PeerGone stale ignorato per {:?} (gen evento={}, gen attuale={:?})",
                        id, gen, self.link_gen.get(&id)
                    );
                    return Vec::new();
                }

                // Cattura PRIMA di qualunque rimozione: serve per l'annuncio sotto, e
                // "ero server"/"era il mio server" hanno senso solo rispetto al ruolo
                // PRE-rielezione (mutuamente esclusivi: un nodo è o server o client, mai
                // entrambi — al più uno dei due rami di annuncio sotto scatta).
                let label = self.peers.get(&id).map(|i| format!("{}-human", i.label_base));
                let era_server = self.channel.role() == Role::Server;
                let was_my_server = matches!(self.channel.role(), Role::Client(server) if server == id);

                // Rimuovi da TUTTI i set di stato che riguardano questo peer.
                // Task 8: `pending`/`consented`/`refused` (il vecchio consenso pairwise)
                // non esistono più — rimossi insieme a quel flusso (vedi l'arm `Discovered`).
                self.connected.remove(&id);
                self.peers.remove(&id);
                // Rimuovi anche il LINK (writer task): senza, il task scrittore e la metà
                // di socket restano orfani — leak per ogni peer che non si riconnette.
                // Debito #4 (Slice C, RISOLTO): la race "un PeerGone vecchio rimuove il
                // link fresco dopo una riconnessione" è impedita dal guard di generazione
                // in cima a questo arm — se siamo arrivati fin qui, `gen` combacia con
                // `self.link_gen[id]`, quindi questo È il link corrente. Ripuliamo anche
                // `link_gen`: la prossima connessione verso questo peer partirà da una
                // mappa senza voce residua per il suo id.
                self.links.remove(&id);
                self.link_gen.remove(&id);
                // Bug 2 (late-joiner) §3.4: se il peer sparito era proprio il leader
                // riportato, azzera `reported_leader`. SENZA questo, `elect()` prenderebbe
                // il ramo "riportato ma fuori roster" → `Undecided` invece di ricadere su
                // `members.lowest()` tra i superstiti → la rielezione dopo sparizione del
                // server (oggi funzionante) si bloccherebbe. Test di regressione esplicito
                // in `peer_gone_clears_reported_leader_and_reelects_lowest_among_survivors`.
                if self.reported_leader == Some(id) {
                    self.reported_leader = None;
                }

                // Task 8b (robustezza dell'ammissione): un peer AMMESSO che sparisce non
                // deve restare un "votante fantasma". Senza questa pulizia, `id` continua a
                // contare come "presente" in `self.admitted` (letto da `start_admission_vote`
                // per un FUTURO voto) e come voto/presente ATTESO in un voto GIÀ in corso
                // (`self.pending_votes`) — un turno che aspettava solo il suo voto resterebbe
                // bloccato fino al `VoteTimeout` invece di risolversi subito.
                self.admitted.remove(&id);
                for pv in self.pending_votes.values_mut() {
                    // `id` esce sia dai "presenti attesi" (non deve più votare) sia da un suo
                    // eventuale voto già registrato (irrilevante ormai: non è più un presente).
                    pv.present.remove(&id);
                    pv.votes.remove(&id);
                }
                // FIX #3 (review 2026-07-03): se `id` è ESSO STESSO il candidato di un
                // voto in corso, quel voto non ha più oggetto — il candidato se n'è
                // andato. Va rimosso QUI, altrimenti sopravvive: poiché un candidato non
                // è mai tra i propri `present`, `ready_after_peer_gone` (sotto) lo
                // vedrebbe "pronto" (tutti i presenti rimasti hanno votato sì) e lo
                // `resolve_admit`-erebbe come membro fantasma; e comunque il suo
                // `VoteTimeout` residuo finirebbe per ammetterlo. La rimozione è un no-op
                // per un `id` che NON è candidato di alcun voto (es. un presente sparito:
                // il suo turno è keyed sul candidato, non su di lui — resta gestito da
                // `ready_after_peer_gone`).
                let dropped_candidate_vote = self.pending_votes.remove(&id);
                // FIX #5 (igiene): un candidato in cooldown che sparisce non deve
                // lasciare una voce residua in `cooling_down` (bloccherebbe un futuro
                // peer che riusasse lo stesso IP finché non arriva un `CooldownExpired`
                // per un timer ormai orfano).
                self.cooling_down.remove(&id);
                // Dopo la rimozione sopra, un turno potrebbe essere ORA pronto per la
                // risoluzione (tutti i presenti RIMANENTI hanno già votato sì) — non aspettare
                // il timeout in quel caso. Raccogliamo PRIMA i candidati pronti in sola
                // lettura: non possiamo iterare `&mut self.pending_votes` (o anche solo
                // tenerne un prestito) e contemporaneamente chiamare `self.resolve_admit`
                // (che vuole `&mut self` per intero) — è il classico conflitto di borrow
                // checker quando si vuole "leggere una collezione e poi mutare il suo
                // contenitore in base a quanto letto". La soluzione: prima leggi e produci un
                // `Vec` indipendente (nessun prestito residuo), POI muta.
                let ready_after_peer_gone: Vec<PeerId> = self
                    .pending_votes
                    .iter()
                    .filter(|(_, pv)| {
                        !pv.present.is_empty()
                            && pv.present.iter().all(|p| pv.votes.get(p) == Some(&true))
                    })
                    .map(|(&candidate, _)| candidate)
                    .collect();

                // Keepalive §5: annuncio "peer sparito" — live-only, non entra in `history`.
                // Se `label` è `None` (caso anomalo: il peer sparito non era in `self.peers`)
                // nessuno dei due rami produce effetti di annuncio — niente panico/unwrap.
                let mut effects = Vec::new();
                // FIX #7 (review 2026-07-03): se abbiamo appena droppato il voto DI cui
                // il peer sparito era candidato (vedi `dropped_candidate_vote` sopra),
                // avvisa i suoi presenti che il gate 2 è chiuso — altrimenti il loro
                // banner "ammetti X?" resterebbe appeso per un candidato che non c'è più.
                if let Some(pv) = &dropped_candidate_vote {
                    effects.extend(self.notify_vote_resolved(pv));
                }
                if let Some(label) = label {
                    if era_server {
                        // Un client è sparito: wiring di Room::leave() (mai chiamato prima
                        // di questa slice) + broadcast Roster aggiornato e PeerLost a TUTTI
                        // i client rimasti (self.connected è già ripulito dell'id sparito).
                        let (roster_msg, participants) = self.channel.on_server_leave(&label);
                        // Fix review finale whole-slice (Opus, Important): la UI LOCALE del
                        // server deve vedere lo stesso roster aggiornato che riceve chi resta
                        // connesso — altrimenti il "presenti:" mostrato nella finestra-chat del
                        // server continua a elencare il peer sparito indefinitamente, e una
                        // eventuale riapertura della finestra (`SetServerTx`) ri-emetterebbe un
                        // `last_known_roster` stantio (mai aggiornato qui). Specchia esattamente
                        // il ramo `ChatMsg::Join` poco sopra in questo stesso file.
                        self.last_known_roster = participants.clone();
                        effects.push(Effect::ToUi(ServerMsg::AiChatRoster { participants }));
                        for &peer in &self.connected {
                            effects.push(Effect::SendToPeer(peer, roster_msg.clone()));
                            effects.push(Effect::SendToPeer(peer, ChatMsg::PeerLost { label: label.clone() }));
                        }
                        effects.push(Effect::ToUi(ServerMsg::AiChatPeerLost { label }));
                    } else if was_my_server {
                        // Il mio server è sparito: nessun altro link a cui inoltrare, solo
                        // notifica locale. decide_and_connect() (sotto) avvia già la
                        // rielezione/riconnessione secondo la logica sticky+reported esistente.
                        effects.push(Effect::ToUi(ServerMsg::AiChatPeerLost { label }));
                    }
                }

                // Rieletti: se il server è andato, un altro peer (o me) prende il ruolo.
                effects.extend(self.decide_and_connect());

                // Task 8b: risolvi ORA i voti diventati pronti in seguito alla rimozione di
                // `id` da `present`/`votes` più sopra, invece di lasciarli bloccati fino al
                // `VoteTimeout`. `ready_after_peer_gone` è vuoto nel caso comune (nessun voto
                // in corso, o un voto ancora in attesa di altri sì) — il loop non fa nulla.
                for candidate in ready_after_peer_gone {
                    let pv = self
                        .pending_votes
                        .remove(&candidate)
                        .expect("candidato raccolto da pending_votes poche righe sopra");
                    effects.extend(self.resolve_admit(candidate, pv));
                }

                // Library "Condividi" (spec 2026-07-28): il peer sparito non deve più
                // comparire come raggiungibile — peers/links sono già stati ripuliti
                // sopra in questo stesso arm.
                effects.push(Effect::ToUi(ServerMsg::AiChatReachablePeers {
                    labels: self.reachable_peer_labels(),
                }));

                effects
            }

            // --- Task 5: PeerMsg — un messaggio arriva da un peer connesso ---
            //
            // Due ruoli, due comportamenti:
            // - SERVER: fa relay agli altri client (n-2, escludendo autore e se stesso)
            //   E forward alla propria UI.
            // - CLIENT: forward soltanto alla propria UI (è compito del server fare relay).
            //
            // `ChatMsg::Roster` viaggia SERVER→CLIENT quando il roster cambia.
            // `ChatMsg::Join`/`Leave` sono gestiti lato server in Task 7; qui no-op minimo.
            ServiceEvent::PeerMsg { from, msg } => {
                let mut effects = Vec::new();
                // Task 6: usata dai guard `Admitted`/`AdmissionRejected` sotto per
                // riconoscere se un messaggio del server riguarda NOI (siamo il
                // candidato) o un altro peer (ignorato — vedi i due arm dedicati).
                let my_candidate_label = format!("{}-human", self.me.label_base);
                match &msg {
                    ChatMsg::Say { from_label, text, display_name, is_ai } => {
                        // Task 8b (relay-guard su `admitted`, chiude il Debito #1 lasciato dal
                        // Task 8 — vedi il vecchio `TODO(admission-8b)` sostituito da questo
                        // commento): un peer CONNESSO ma NON ANCORA AMMESSO (voto in corso, o
                        // mai iniziato) non deve poter iniettare testo nella stanza. Se il
                        // mittente non è ammesso, ignoriamo il messaggio per intero — nessun
                        // append allo storico, nessun `ToUi`, nessun relay: usciamo SUBITO,
                        // PRIMA di qualunque mutazione di stato qui sotto (`effects` è ancora
                        // vuoto in questo punto, quindi `return effects` equivale a `Vec::new()`).
                        //
                        // Il controllo è ristretto al ruolo SERVER: `self.admitted` è "solo
                        // server" per costruzione (vedi il suo doc-comment) — un CLIENT non lo
                        // popola mai, quindi un guard incondizionato bloccherebbe ogni Say che
                        // il client riceve dal proprio server (mai "ammesso" in una mappa che il
                        // client non tiene). Lato client il gate di ammissione resta quello che
                        // già esiste (`self_admission`), non questo.
                        //
                        // `from == self.me.id` è sempre fidato: è il solo modo in cui un PeerMsg
                        // può arrivare con l'identità del server stesso, e accade SOLO in test su
                        // loopback (127.0.0.1 aliasing — vedi la nota in
                        // `tests/aichat_relay_loopback.rs`); su una LAN reale un peer remoto non
                        // condivide mai l'IP locale del server, quindi il caso non si presenta.
                        if self.channel.role() == Role::Server
                            && from != self.me.id
                            && !self.admitted.contains(&from)
                        {
                            tracing::debug!(
                                "aichat: Say ignorato da {:?} ({from_label}): non ancora ammesso alla stanza",
                                from
                            );
                            return effects;
                        }

                        // Ogni peer che vede un Say lo registra nel proprio storico locale —
                        // è ciò che permette a chi viene eletto server dopo di ereditare lo
                        // storico "per osmosi" (nessun handoff esplicito necessario).
                        //
                        // `display_name`/`is_ai` (Task 3, display names): PASS-THROUGH, MAI
                        // ricalcolati — questo è un messaggio di un ALTRO peer, che li ha già
                        // stampati con la propria `publish_say` (la SUA `self.display_name`/
                        // `self.ai_display_name`, non la nostra). Ricalcolare qui da
                        // `from_label.ends_with("-ai")` darebbe comunque lo stesso `is_ai`
                        // (deterministico dalla label), ma `display_name` sarebbe SBAGLIATO:
                        // risolveremmo il nickname CONFIGURATO SU QUESTA MACCHINA per
                        // un'etichetta che non è la nostra.
                        self.history.push(ChatLine {
                            from_label: from_label.clone(),
                            text: text.clone(),
                            display_name: display_name.clone(),
                            is_ai: *is_ai,
                        });
                        // Slice 2 (design §10.3): questo È il messaggio "gemello" del
                        // choke-point in `publish_say` — un messaggio da un ALTRO peer entra
                        // nello storico QUI, non in `publish_say`. Aggiorniamo contatore/
                        // last_speaker qui, esattamente una volta, per restare coerenti col
                        // resto del file (mai due volte per la stessa riga di storico).
                        self.note_room_message(from_label);
                        // (1) Forward alla propria UI: l'utente locale vede il messaggio nel cursore.
                        // Stesso pass-through di sopra: `display_name`/`is_ai` sono quelli già
                        // stampati dal peer d'origine, non ricalcolati qui.
                        effects.push(Effect::ToUi(ServerMsg::AiChatMessage {
                            from_label: from_label.clone(),
                            text: text.clone(),
                            display_name: display_name.clone(),
                            is_ai: *is_ai,
                        }));
                        // (2) Solo il server fa relay: invia agli altri client (n-2).
                        // `n-2` = tutti i connessi meno l'autore del messaggio (`from`)
                        // e meno il server stesso (`me`) — già gestito da `fan_out_targets` in `relay.rs`.
                        //
                        // Task 8b (RISOLTO, era `TODO(admission-8b)`): il mittente è già
                        // garantito AMMESSO a questo punto — il guard in cima a questo arm ha
                        // già fatto `return` per qualunque `from` non in `self.admitted`. I
                        // DESTINATARI di `roster_snapshot()` restano invece basati su
                        // `self.connected` (non ristretti ad `admitted`): un peer connesso ma
                        // non ancora ammesso può quindi ancora RICEVERE un relay altrui, pur non
                        // potendo MAI essere lui stesso mittente di un Say accettato. Restringere
                        // anche i destinatari a `self.admitted` è stato valutato ma scartato per
                        // questa slice (nessuna regressione nota, ma nessun test lo richiede
                        // esplicitamente) — vedi il report del Task 8b per i dettagli.
                        if self.channel.role() == Role::Server {
                            let roster = self.roster_snapshot();
                            for (peer, m) in self.channel.server_relay(from, msg.clone(), &roster) {
                                effects.push(Effect::SendToPeer(peer, m));
                            }
                        }

                        // --- AI Chat Slice 1b: invocazione REMOTA (@all / @<label>-ai) ---
                        //
                        // GUARDIA LOOP (load-bearing): rileva un'invocazione SOLO se il
                        // messaggio arriva da un UMANO (`from_label` finisce in "-human").
                        // Una risposta di un'AI (`-ai`) relayata NON deve MAI essere
                        // trattata come invocazione — altrimenti un'AI potrebbe innescarne
                        // un'altra, aprendo un loop cross-macchina. Gira sia da SERVER sia
                        // da CLIENT (entrambi "vedono" passare questo Say, indipendentemente
                        // dal relay sopra), indipendentemente dal blocco (2): quello decide
                        // SE inoltrare il messaggio ad altri peer, questo decide SE la mia
                        // AI locale deve rispondere.
                        // Cattura PRIMA se questo messaggio è un'invocazione esplicita di
                        // QUALUNQUE tipo — serve sotto per sopprimere l'auto-
                        // partecipazione indipendentemente da CHI l'invocazione targeti
                        // (fix 2026-07-08: prima si guardava solo se targetava ME).
                        let invocation = if from_label.ends_with("-human") {
                            extract_ai_invocation(text)
                        } else {
                            None
                        };
                        let was_explicit_invocation = invocation.is_some();

                        if let Some(invocation) = invocation {
                            let targets_me = match &invocation.target {
                                // "@ai" è lo shorthand "la MIA AI" del MITTENTE — da
                                // remoto non riguarda me (se il mittente voleva la MIA
                                // AI doveva usare "@all" o il mio label esplicito).
                                InvokeTarget::Own => false,
                                InvokeTarget::All => true,
                                InvokeTarget::Label(label) => label == &self.me.label_base,
                            };
                            // Un umano REMOTO mi ha invocato: rispondo SOLO se il flag
                            // locale lo consente — autonomia della macchina (design
                            // §3/§9.4). L'invocazione della PROPRIA macchina (arm
                            // `HumanSay`, sotto) non passa MAI da qui: bypassa il flag.
                            if targets_me && self.ai_participates {
                                let my_ai_label = format!("{}-ai", self.me.label_base);
                                let history = build_ai_history(&self.history, &my_ai_label);
                                effects.push(Effect::InvokeLocalAi {
                                    request: invocation.request,
                                    history,
                                    my_ai_label,
                                });
                            }
                        }

                        // --- AI Chat Slice 2: auto-partecipazione (design §10.3) ---
                        //
                        // A differenza del blocco Slice 1b sopra (gated su "-human"
                        // perché SOLO un umano può invocare esplicitamente), qui NON
                        // filtriamo su `from_label`: un messaggio `-ai` di un ALTRO peer
                        // PUÒ far scattare la mia auto-partecipazione (è precisamente la
                        // conversazione AI↔AI spontanea che questa slice abilita) — le
                        // guardie 2/3/4/5 di `maybe_autoparticipate` (mai sui MIEI
                        // messaggi, cap sui turni, un giudizio alla volta) bastano da
                        // sole a tenerla bounded, senza bisogno di questo filtro.
                        //
                        // --- AI Chat Slice 2b/2c: sopprimi su QUALSIASI invocazione esplicita ---
                        //
                        // AGGIORNATO (2026-07-08, bug osservato dal vivo — vedi
                        // Docs/superpowers/specs/2026-07-08-aichat-autoparticipate-explicit-invocation-design.md):
                        // il gate non guarda più "ho GIÀ prodotto un InvokeLocalAi per ME"
                        // (`already_invoked`, rimosso), ma "il messaggio ERA
                        // un'invocazione esplicita, chiunque essa targetasse"
                        // (`was_explicit_invocation`, catturato sopra). Un'invocazione
                        // esplicita ha già un destinatario chiaro — l'auto-partecipazione
                        // esiste per i messaggi che NESSUNO ha esplicitamente rivolto a
                        // qualcuno, coerente con quanto il system prompt di
                        // `chat_autoparticipate` già dichiara ("NESSUNO ti ha invocato
                        // esplicitamente") — prima poteva essere falso senza che l'AI lo
                        // sapesse.
                        if !was_explicit_invocation {
                            effects.extend(self.maybe_autoparticipate(from_label));
                        }
                    }
                    ChatMsg::Roster { participants } => {
                        // Un client riceve il roster autorevole dal server.
                        // Lo inoltra alla UI perché questa aggiorni l'elenco dei partecipanti.
                        // Caching: memorizziamo il roster per ri-emetterlo se la UI si riapre.
                        self.last_known_roster = participants.clone();
                        effects.push(Effect::ToUi(ServerMsg::AiChatRoster {
                            participants: participants.clone(),
                        }));
                    }
                    // Catch-up: un peer (nuovo arrivo, o riavvio di questo stesso
                    // orchestrator) riceve lo storico completo dal server. Sostituisce SOLO
                    // se: (a) non siamo noi il server (un server non dovrebbe mai ricevere
                    // History — solo lui lo manda) e (b) il dump non è più corto del nostro
                    // storico locale. Il guard (b) evita di perdere un messaggio nostro non
                    // ancora relayato dal vecchio server prima che sparisse (scenario: proprio
                    // la sparizione del server che questa feature deve sopravvivere). NON è un
                    // merge vero (richiederebbe identità/sequenza per messaggio, fuori scope —
                    // vedi task keepalive); è solo una difesa minima contro un dump più povero
                    // del nostro stato locale. Trovato in review finale whole-feature (Opus).
                    ChatMsg::History { entries }
                        if self.channel.role() != Role::Server && entries.len() >= self.history.len() =>
                    {
                        self.history = entries.clone();
                        if !self.history.is_empty() {
                            effects.push(Effect::ToUi(ServerMsg::AiChatHistory {
                                entries: self.history.iter().cloned().map(Into::into).collect(),
                            }));
                        }
                    }
                    // Storico ricevuto ma scartato: o siamo il server (non dovremmo mai
                    // riceverlo), o il dump è più corto/uguale del nostro storico locale
                    // (stale — potremmo avere un messaggio nostro non ancora relayato dal
                    // vecchio server). In entrambi i casi non sovrascriviamo nulla.
                    ChatMsg::History { .. } => {}
                    // Il server ci ha detto che un altro client è sparito — solo notifica
                    // locale, nessun relay ulteriore (il client non ha altri link).
                    ChatMsg::PeerLost { label } => {
                        effects.push(Effect::ToUi(ServerMsg::AiChatPeerLost { label: label.clone() }));
                    }
                    // No-op applicativo: la sola ricezione ha già resettato il timeout di
                    // silenzio sul reader (vedi spawn_peer_tasks, Task 8). Nessuna azione qui.
                    ChatMsg::Ping {} => {}
                    // Task 8 (ammissione alla stanza): `Join`/`Leave` sono ora DEAD CODE
                    // applicativo — nessun peer li manda più. `Join` è sostituito dal
                    // flusso `RequestAdmission`/voto/`Admitted` (Task 4-6, sotto); `Leave`
                    // non è mai stato inviato da nessun mittente in questo protocollo (la
                    // sparizione di un peer è rilevata da `PeerGone`, non da un messaggio
                    // esplicito). Le due varianti restano nell'enum `ChatMsg` (Contratto N è
                    // additivo, vedi `wire.rs`) ma sono qui solo per esaustività del `match`.
                    ChatMsg::Join { .. } | ChatMsg::Leave { .. } => {}
                    // --- Task 4: RequestAdmission lato SERVER → avvia il voto ---
                    //
                    // Il candidato (`from`) ha appena mandato `ChatMsg::RequestAdmission`,
                    // dopo aver superato il proprio gate 1 (Task 6: l'umano ha accettato
                    // "vuoi entrare con [presenti]?"). Solo il SERVER ELETTO coordina il
                    // voto — un client che riceve questo messaggio (caso anomalo: il
                    // candidato dovrebbe connettersi SOLO al server) lo ignora, senza
                    // avviare nulla (guard `if`, non un `match`-guard: evita un secondo
                    // arm di fallback per il ramo "non siamo server").
                    ChatMsg::RequestAdmission { label } => {
                        if self.channel.role() == Role::Server {
                            // FIX #5 (review 2026-07-03): rispetta il cooldown lato SERVER.
                            // Un candidato rifiutato di recente resta in `cooling_down`
                            // finché il suo timer non scade: ignorare il re-request qui
                            // impedisce a un peer che manda il wire direttamente (o che ha
                            // chiuso/riaperto la finestra, azzerando il cooldown UI) di
                            // riaprire il voto a raffica e ri-promptare tutti i presenti.
                            if self.cooling_down.contains(&from) {
                                tracing::debug!(
                                    "aichat: RequestAdmission da {:?} in cooldown — ignorato",
                                    from
                                );
                            } else {
                                effects.extend(self.start_admission_vote(from, label.clone()));
                            }
                        } else {
                            tracing::debug!(
                                "aichat: RequestAdmission ricevuto da {:?} ma non siamo server",
                                from
                            );
                        }
                    }
                    // --- Task 5: AdmissionVote lato SERVER → registra il voto (o veto) ---
                    //
                    // Un presente ha risposto al gate 2 ("ammetti `candidate`?"). Solo il
                    // server tiene `pending_votes`: `record_vote` cerca il turno per
                    // ETICHETTA (il wire porta `candidate` come `String`, non `PeerId` —
                    // vedi il doc-comment di `record_vote`) e decide se il turno si chiude
                    // subito (veto, o "tutti hanno detto sì") o resta in attesa di altri voti.
                    // Un client che riceve questo messaggio per errore (non dovrebbe mai
                    // accadere: solo il server è destinatario di un `AdmissionVote`) lo
                    // processerebbe comunque qui — ma `pending_votes` è sempre vuoto per un
                    // client (nessun voto vi è mai stato registrato), quindi `record_vote`
                    // ritorna `Vec::new()` senza effetti collaterali indesiderati.
                    ChatMsg::AdmissionVote { candidate, accept } => {
                        effects.extend(self.record_vote(candidate, from, *accept));
                    }
                    // --- Task 6b: AdmissionVoteRequest lato PRESENTE-CLIENT → gate 2 ---
                    //
                    // `AdmissionVoteRequest` è il gate 2 per un PRESENTE già ammesso
                    // ("ammetti X?"), non per il candidato stesso. Il server manda questo
                    // messaggio SOLO ai presenti REMOTI (`start_admission_vote`: mai a se
                    // stesso, il proprio umano-server riceve il gate 2 via `ToUi`
                    // diretto, non da un `ChatMsg` di rete). Un presente-client che lo
                    // riceve deve quindi mostrare lo stesso gate 2 alla propria UI —
                    // esattamente come il server mostra il proprio. Nessun guard di
                    // ruolo qui (a differenza di `RequestAdmission`/`AdmissionVote`
                    // sopra, che sono SOLO-server): mostrare il gate 2 alla UI è innocuo
                    // anche nell'improbabile caso in cui arrivasse in un altro stato —
                    // sarà l'umano a decidere se e come rispondere.
                    //
                    // Prima di questo fix l'arm era no-op: il presente remoto non vedeva
                    // mai la richiesta di voto, e un suo eventuale voto (vedi il fix
                    // gemello in `ServiceEvent::AdmissionVoteUi` sotto) sarebbe comunque
                    // andato perso. Risultato: solo l'umano-server poteva votare/vietare,
                    // violando il modello AND-con-veto del Task 5.
                    ChatMsg::AdmissionVoteRequest { candidate } => {
                        effects.push(Effect::ToUi(ServerMsg::AiChatAdmissionRequest {
                            candidate: candidate.clone(),
                        }));
                    }
                    // --- FIX #7 (review 2026-07-03): AdmissionResolved lato PRESENTE-CLIENT ---
                    //
                    // Gemello "di chiusura" di `AdmissionVoteRequest`: il server ci dice
                    // che il voto per `candidate` è concluso (ammesso/rifiutato/candidato
                    // sparito), così togliamo il banner del gate 2. Come per
                    // `AdmissionVoteRequest`, nessun guard di ruolo: ribaltare la notifica
                    // nella UI è innocuo qualunque sia il nostro stato.
                    ChatMsg::AdmissionResolved { candidate } => {
                        effects.push(Effect::ToUi(ServerMsg::AiChatAdmissionResolved {
                            candidate: candidate.clone(),
                        }));
                    }

                    // --- Task 6: Admitted/AdmissionRejected lato NUOVO ARRIVATO ---
                    //
                    // Il server li manda SOLO al candidato (`SendToPeer(candidate, ..)` in
                    // `resolve_admit`/`resolve_reject`, mai in broadcast) — ma il guard
                    // `label == la nostra etichetta` resta comunque la difesa corretta:
                    // se un giorno `Admitted` venisse anche notificato ai presenti già
                    // ammessi (vedi il doc-comment di `ChatMsg::Admitted` in wire.rs), un
                    // `Admitted` per l'etichetta di un ALTRO candidato non deve MAI
                    // toccare il nostro `self_admission`.
                    ChatMsg::Admitted { label } if label == &my_candidate_label => {
                        self.self_admission = SelfAdmission::Admitted;
                        effects.push(Effect::ToUi(ServerMsg::AiChatAdmitted {}));
                    }
                    // Notifica dell'ammissione di un ALTRO candidato: non ci riguarda
                    // direttamente — il `Roster` aggiornato che il server manda subito
                    // dopo (vedi `resolve_admit`) è ciò che aggiorna la UI con la sua
                    // presenza, non serve reagire qui.
                    ChatMsg::Admitted { .. } => {}
                    ChatMsg::AdmissionRejected { label } if label == &my_candidate_label => {
                        self.self_admission = SelfAdmission::Rejected;
                        effects.push(Effect::ToUi(ServerMsg::AiChatRejected { retry_after_secs: 30 }));
                    }
                    // Un rifiuto per un ALTRO candidato (caso anomalo: `resolve_reject`
                    // manda questo messaggio solo al candidato stesso) — ignorato.
                    ChatMsg::AdmissionRejected { .. } => {}
                    // ── Task 8/9: Share message handling (placeholders per Slice 1a) ──
                    ChatMsg::ShareOffer { share_id, from_label, doc_name, size_bytes } => {
                        effects.extend(self.handle_share_offer(
                            share_id.clone(),
                            from_label.clone(),
                            doc_name.clone(),
                            *size_bytes,
                        ));
                    }
                    ChatMsg::ShareAccept { share_id, .. } => {
                        effects.extend(self.handle_share_accepted(share_id.clone()));
                    }
                    ChatMsg::ShareReject { share_id, .. } => {
                        effects.extend(self.resolve_outgoing_share(share_id, protocol::ShareOutcome::Rejected));
                    }
                    ChatMsg::ShareExpired { share_id } => {
                        effects.extend(self.resolve_outgoing_share(
                            share_id,
                            protocol::ShareOutcome::Failed {
                                reason: "il destinatario non ha completato la ricezione entro 24h".into(),
                            },
                        ));
                    }
                    ChatMsg::ShareData { share_id, title, content } => {
                        effects.extend(self.handle_share_data(share_id.clone(), title.clone(), content.clone()));
                    }
                    // ── Task 10: Library "Blocco note" incoming NotesDigest → reply ────
                    ChatMsg::NotesDigest { entries } => {
                        let (want, push) = crate::notes::digest::reconcile(self.notes.all(), entries);
                        if !want.is_empty() || !push.is_empty() {
                            effects.push(Effect::SendToPeer(from, ChatMsg::NotesDigestReply { want, push }));
                        }
                    }
                    ChatMsg::NotesDigestReply { want, push } => {
                        // Fulfili `want`: invia indietro al peer le note che ha richiesto.
                        if !want.is_empty() {
                            let notes: Vec<Note> = want.iter()
                                .filter_map(|id| self.notes.get(id).cloned())
                                .collect();
                            if !notes.is_empty() {
                                effects.push(Effect::SendToPeer(from, ChatMsg::NotesData { notes }));
                            }
                        }
                        // Elabora `push`: mergia ogni nota ricevuta e fanetela agli altri
                        // peer connessi (escludendo il mittente).
                        //
                        // Design §5.2 punto 4: una nota arrivata via riconciliazione deve
                        // raggiungere anche gli altri peer già collegati in questa stanza,
                        // non solo il peer che ce l'ha mandata — MA solo se il merge ha
                        // davvero cambiato qualcosa (FIX 5, gate dentro
                        // `apply_incoming_note`). Nota: la risposta al `want` qui sopra
                        // resta FUORI dal gate — non è il risultato di un merge, è il
                        // contenuto che il peer ci ha esplicitamente chiesto.
                        for incoming in push {
                            effects.extend(self.apply_incoming_note(incoming.clone(), from, true));
                        }
                    }
                    // ── Task 12: incoming NotesData — fulfillment di want proprio ────────────
                    //
                    // Questo braccio gestisce il fulfilment di un `want` CHE NOI STESSI abbiamo
                    // richiesto (via NotesDigestReply): il peer ci manda indietro le note che gli
                    // abbiamo chiesto. A differenza di `NoteUpdated` (che ha sempre bisogno di
                    // fan-out per raggiungere gli altri peer) e di `NotesDigestReply::push`
                    // (che fanetizza il suo payload agli altri peer), QUI il mittente ha GIÀ
                    // effettuato il suo fan-out (se necessario) attraverso i propri
                    // `NoteUpdated`/`NotesDigestReply` — nessuno di quei messaggi è passato da
                    // questo braccio. Un fan-out aggiuntivo qui causerebbe un DOPPIO INVIO della
                    // stessa nota via due percorsi diversi, violando il principio di "consegnare
                    // una sola volta per aggiornamento" (spec §5.2).
                    //
                    // `fan_out: false` per questo motivo; il resto della coda (merge,
                    // gate "solo se cambiata" del FIX 5, UI, salvataggio) è identico
                    // agli altri due arm remoti, quindi condiviso in
                    // `apply_incoming_note`.
                    ChatMsg::NotesData { notes } => {
                        for incoming in notes {
                            effects.extend(self.apply_incoming_note(incoming.clone(), from, false));
                        }
                    }
                    ChatMsg::NoteUpdated { note } => {
                        // Fan-out verso gli altri link (design §5.1/§5.2 punto 4): un client
                        // con un solo link (verso il server) non ha "altri" a cui inoltrare —
                        // questo no-oppa naturalmente per i client, e fa da vero fan-out solo
                        // quando chi riceve è il server (più link registrati). Gatato da
                        // `apply_incoming_note` sul "è davvero cambiata?" (FIX 5).
                        effects.extend(self.apply_incoming_note(note.clone(), from, true));
                    }
                }
                effects
            }

            // --- Task 4: HumanSay — eco locale + broadcast secondo il ruolo ---
            // --- AI Chat Slice 1a/1b: rilevazione dell'invocazione "@ai"/"@all"/
            // --- "@<label>-ai" -----------------------------------------------

            // L'umano locale ha scritto un messaggio. La pubblicazione nella stanza
            // (storico + eco UI + inoltro in rete secondo il ruolo) è delegata a
            // `publish_say` (riusata anche da `AiReply` sotto — SOLID/DRY: prima
            // duplicava il corpo di questo arm quasi identico).
            //
            // IN PIÙ: se il messaggio è un'INVOCAZIONE esplicita dell'AI e mi
            // riguarda (vedi `targets_me` sotto), emettiamo anche un
            // `Effect::InvokeLocalAi` — uno dei due punti di ingresso che fanno
            // parlare l'AI locale (l'altro è `PeerMsg::Say` da un umano remoto,
            // sotto — SOLO quello è gated dal flag `ai_participates`: il MIO umano
            // ha già dato il consenso scrivendo qui). Un messaggio "normale"
            // (conversazione umano↔umano) non la tocca mai: è quanto rende il
            // modello loop-safe per costruzione (Slice 1a/1b, vedi il design doc §2/§9).
            ServiceEvent::HumanSay(text) => {
                let label = format!("{}-human", self.me.label_base);
                // `publish_say` aggiorna GIÀ `self.history` (append della riga umana) E
                // chiama `note_room_message` (choke-point, vedi il doc-comment su
                // `publish_say`): il transcript costruito subito dopo la include, così
                // l'AI "vede" anche la propria invocazione nel contesto passato a
                // `chat_reply`. `label.clone()`: serve di nuovo sotto per
                // `maybe_autoparticipate` (Slice 2), dopo che `publish_say` la consuma.
                let mut effects = self.publish_say(label.clone(), text.clone());

                let invocation = extract_ai_invocation(&text);
                let was_explicit_invocation = invocation.is_some();

                if let Some(invocation) = invocation {
                    // Il MIO umano ha scritto: se l'invocazione mi riguarda
                    // (Own/All sono sempre "me stesso" dal punto di vista del
                    // mittente; Label(l) solo se l == il mio label_base) la mia
                    // AI risponde SEMPRE — nessun gate sul flag `ai_participates`,
                    // che riguarda SOLO le richieste in arrivo da un umano REMOTO
                    // (vedi l'arm `PeerMsg::Say` sopra).
                    let targets_me = match &invocation.target {
                        InvokeTarget::Own | InvokeTarget::All => true,
                        InvokeTarget::Label(label) => label == &self.me.label_base,
                    };
                    if targets_me {
                        let my_ai_label = format!("{}-ai", self.me.label_base);
                        let history = build_ai_history(&self.history, &my_ai_label);
                        effects.push(Effect::InvokeLocalAi {
                            request: invocation.request,
                            history,
                            my_ai_label,
                        });
                    }
                }

                // AI Chat Slice 2 (design §10.3): oltre all'eventuale invocazione
                // esplicita sopra, il MIO messaggio può ANCHE far scattare
                // un'auto-partecipazione spontanea (se `ai_autoparticipate` è ON —
                // le altre guardie sono valutate dentro `maybe_autoparticipate`).
                //
                // AGGIORNATO (2026-07-08, bug osservato dal vivo — vedi
                // Docs/superpowers/specs/2026-07-08-aichat-autoparticipate-explicit-invocation-design.md):
                // il gate non guarda più "ho GIÀ prodotto un InvokeLocalAi" ma "il
                // messaggio ERA un'invocazione esplicita" (`was_explicit_invocation`,
                // catturato sopra — anche se non mi targetava, es. il MIO umano scrive
                // `@<altro-label>-ai`: quell'invocazione ha già un destinatario chiaro,
                // non è "nessuno", quindi la mia auto-partecipazione non deve scattare).
                // Un messaggio NORMALE (senza @) continua a poter innescare
                // l'auto-partecipazione come prima (invariato — vedi
                // `human_say_plain_with_auto_on_still_autoparticipates`).
                if !was_explicit_invocation {
                    effects.extend(self.maybe_autoparticipate(&label));
                }

                effects
            }

            // AI Chat Slice 1a: la risposta della propria AI locale (testo ottenuto da
            // `perform` per `Effect::InvokeLocalAi`, tramite `ai_adapter.chat_reply`).
            // Trattata ESATTAMENTE come un `Say` da "<label_base>-ai" — stessa
            // pubblicazione di `HumanSay`, cambia solo l'etichetta del mittente.
            //
            // Nota loop-safety: questo arm NON chiama `extract_ai_invocation` — anche se
            // il testo dell'AI contenesse "@ai" (es. lo cita testualmente), non verrebbe
            // MAI ri-innescato un `InvokeLocalAi` da qui. Solo un umano può invocare l'AI.
            //
            // Slice 2: questo arm NON chiama `maybe_autoparticipate` — `publish_say`
            // registra comunque il messaggio (`note_room_message`, contatore
            // incrementato, `last_speaker_label` = la MIA `-ai`), ma non c'è bisogno di
            // valutare il trigger: anche se lo facessimo, la guardia 2 di
            // `maybe_autoparticipate` ("il messaggio non è dal mio -ai") lo escluderebbe
            // comunque. Ometterlo qui evita solo una valutazione ridondante.
            ServiceEvent::AiReply { text } => {
                let label = format!("{}-ai", self.me.label_base);
                let memory_note = extract_memoria_marker(&text);
                let mut effects = self.publish_say(label, text);
                if let Some(note) = memory_note {
                    effects.push(Effect::PersistMemory { label_base: self.me.label_base.clone(), note });
                }
                effects
            }

            // AI Chat Slice 2 (design §10.3): il task spawnato da `perform` per
            // `Effect::AutoParticipate` ha ottenuto l'esito del giudizio di rilevanza
            // da `ai_adapter.chat_autoparticipate`. Due responsabilità, in quest'ordine:
            // 1. Azzera SEMPRE `autoparticipate_inflight` — incondizionatamente, PRIMA
            //    di guardare `reply`: un `None` (silenzio, l'esito più comune) non deve
            //    MAI lasciare il flag bloccato a `true` (vedi il doc-comment del campo).
            // 2. Se l'AI ha contribuito con un testo non vuoto, pubblicalo come un `Say`
            //    da "<label_base>-ai" — ESATTAMENTE il percorso di `AiReply` sopra
            //    (stesso `publish_say`, stessa etichetta `-ai`). Il guard su
            //    `!text.trim().is_empty()` rispecchia `perform_invoke_local_ai_skips_empty_reply`
            //    (Slice 1a): un testo vuoto non deve diventare una riga di chat fantasma.
            ServiceEvent::AutoParticipateDone { reply } => {
                self.autoparticipate_inflight = false;
                match reply {
                    Some(text) if !text.trim().is_empty() => {
                        let label = format!("{}-ai", self.me.label_base);
                        let memory_note = extract_memoria_marker(&text);
                        let mut effects = self.publish_say(label, text);
                        if let Some(note) = memory_note {
                            effects.push(Effect::PersistMemory { label_base: self.me.label_base.clone(), note });
                        }
                        effects
                    }
                    _ => Vec::new(),
                }
            }

            // --- Keepalive: Tick — manda un Ping ad ogni peer connesso ---
            //
            // Innescato da `run()` ogni `PING_INTERVAL` (5s). Stesso codice sia da server
            // (N client) sia da client (1 server): `connected` è già generico. Un nodo
            // Undecided/isolato (`connected` vuoto) produce zero effetti — no-op innocuo.
            ServiceEvent::Tick => {
                self.connected
                    .iter()
                    .map(|&peer| Effect::SendToPeer(peer, ChatMsg::Ping {}))
                    .collect()
            }

            // Debito #5: l'accept loop TCP è terminato (errore o canale chiuso).
            // Resettiamo il flag di idempotenza: senza questo, il guard in `perform`
            // (`if self.listening { return }`) bloccherebbe per sempre un nuovo bind.
            //
            // Ri-chiamiamo subito `decide_and_connect()` invece di aspettare il prossimo
            // `Discovered` (che in LAN arriva ogni ~5-7s dal discoverer UDP): se siamo
            // ancora `Role::Server`, questo ri-emette immediatamente `StartListener`, che
            // ora supera il guard (appena resettato) e ri-binda il socket — ripristino
            // del servizio senza attese. Se non siamo più server (rielezione nel
            // frattempo), `decide_and_connect` emette l'effetto giusto per il nuovo ruolo
            // (o nessuno, se `Undecided`).
            ServiceEvent::ListenerStopped => {
                self.listening = false;
                self.decide_and_connect()
            }

            // --- Task 6: JoinDecision — risposta dell'umano al gate 1 ---
            //
            // Serve SEMPRE il server a cui siamo connessi (per `Disconnect`/
            // `SendToPeer`): lo ricaviamo dal ruolo. Se non siamo `Role::Client`
            // (caso anomalo: il gate 1 è mostrato solo dopo un connect riuscito
            // come client, vedi `begin_join_gate`) ignoriamo l'evento — non c'è
            // nessun server a cui rivolgersi.
            ServiceEvent::JoinDecision { accept } => {
                let server_id = match self.channel.role() {
                    Role::Client(id) => id,
                    other => {
                        tracing::debug!(
                            "aichat: JoinDecision ricevuta ma il ruolo non è Client (era {other:?}) — ignorata"
                        );
                        return Vec::new();
                    }
                };
                if !accept {
                    // L'umano non vuole entrare: il link verso il server non ci
                    // serve più. Torniamo allo stato iniziale — un futuro
                    // `Discovered` potrebbe ripresentare il gate 1 da capo.
                    self.self_admission = SelfAdmission::NotJoining;
                    return vec![Effect::Disconnect(server_id)];
                }
                // Sì: significativo SOLO se il gate 1 è ancora aperto
                // (`Deciding`, mostrato da `begin_join_gate`). Una `JoinDecision`
                // duplicata/stale (es. arrivata di nuovo dopo che il voto è già
                // partito, o dopo un `Admitted`/`AdmissionRejected`) non deve
                // ri-chiedere l'ammissione né riportarci in `Pending` da uno
                // stato più avanzato della state machine.
                if self.self_admission != SelfAdmission::Deciding {
                    tracing::debug!(
                        "aichat: JoinDecision{{accept:true}} ricevuta ma self_admission non è Deciding (era {:?}) — ignorata",
                        self.self_admission
                    );
                    return Vec::new();
                }
                self.request_admission(server_id)
            }

            // --- Task 5: ShareDocumentRequested — la UI vuole condividere un documento ---
            ServiceEvent::ShareDocumentRequested { rel_path, doc_name, size_bytes, target } => {
                self.request_share(rel_path, doc_name, size_bytes, target)
            }

            // --- Task 8: ShareConsentUi — l'umano decide su un'offerta pendente ---
            ServiceEvent::ShareConsentUi { share_id, accept } => self.share_consent(share_id, accept),
            // --- Slice 2a: ShareWrittenUi — la UI ha scritto su disco il documento ---
            ServiceEvent::ShareWrittenUi { share_id } => {
                self.pending_shares.remove(&share_id);
                Vec::new()
            }
            // --- Slice 2a: ShareContentUi/ShareContentFailedUi — la UI del MITTENTE
            // risponde a ShareContentRequest ---
            ServiceEvent::ShareContentUi { share_id, title, content } => {
                self.handle_share_content(share_id, title, content)
            }
            ServiceEvent::ShareContentFailedUi { share_id, reason } => {
                self.resolve_outgoing_share(&share_id, protocol::ShareOutcome::Failed { reason })
            }

            // --- Task 7: Blocco note — creazione/modifica/cancellazione locale ---
            // Design: Docs/superpowers/specs/2026-07-29-library-notes-design.md §5.1.
            ServiceEvent::NoteCreateRequested { title, text } => {
                let id = self.next_note_id();
                let now = self.wallclock_ms_for_test.unwrap_or_else(now_ms);
                let note = Note {
                    id,
                    title,
                    title_touched: (self.me.label_base.clone(), now),
                    segments: vec![Segment {
                        machine: self.me.label_base.clone(),
                        seq: 1,
                        text,
                        edited_at_ms: now,
                    }],
                    created_by: self.me.label_base.clone(),
                    created_at_ms: now,
                    deleted: false,
                };
                let merged = self.notes.upsert_merged(note);
                let mut effects = vec![
                    Effect::ToUi(ServerMsg::NoteUpserted { note: self.to_note_view(&merged) }),
                    Effect::SaveNotes,
                ];
                effects.extend(self.broadcast_note_to_links(&merged));
                effects
            }
            ServiceEvent::NoteEditRequested { id, text } => {
                let Some(existing) = self.notes.get(&id) else { return Vec::new() };
                let now = self.wallclock_ms_for_test.unwrap_or_else(now_ms);
                let my_seq = existing
                    .segments
                    .iter()
                    .find(|s| s.machine == self.me.label_base)
                    .map(|s| s.seq + 1)
                    .unwrap_or(1);
                let mut delta = existing.clone();
                delta.segments = vec![Segment {
                    machine: self.me.label_base.clone(),
                    seq: my_seq,
                    text,
                    edited_at_ms: now,
                }];
                let merged = self.notes.upsert_merged(delta);
                let mut effects = vec![
                    Effect::ToUi(ServerMsg::NoteUpserted { note: self.to_note_view(&merged) }),
                    Effect::SaveNotes,
                ];
                effects.extend(self.broadcast_note_to_links(&merged));
                effects
            }
            ServiceEvent::NoteEditTitleRequested { id, title } => {
                let Some(existing) = self.notes.get(&id) else { return Vec::new() };
                let now = self.wallclock_ms_for_test.unwrap_or_else(now_ms);
                let mut delta = existing.clone();
                delta.segments = Vec::new(); // nessun segmento nuovo, solo titolo
                delta.title = title;
                delta.title_touched = (self.me.label_base.clone(), now);
                let merged = self.notes.upsert_merged(delta);
                let mut effects = vec![
                    Effect::ToUi(ServerMsg::NoteUpserted { note: self.to_note_view(&merged) }),
                    Effect::SaveNotes,
                ];
                effects.extend(self.broadcast_note_to_links(&merged));
                effects
            }
            ServiceEvent::NoteDeleteRequested { id } => {
                let Some(existing) = self.notes.get(&id) else { return Vec::new() };
                let mut delta = existing.clone();
                delta.segments = Vec::new();
                delta.deleted = true;
                let merged = self.notes.upsert_merged(delta);
                let mut effects = vec![
                    Effect::ToUi(ServerMsg::NoteUpserted { note: self.to_note_view(&merged) }),
                    Effect::SaveNotes,
                ];
                effects.extend(self.broadcast_note_to_links(&merged));
                effects
            }

            // --- Task 9: timer di riconciliazione (Effect::StartNotesReconcileTimer) ---
            // Design: Docs/superpowers/specs/2026-07-29-library-notes-design.md §5.2, §9.
            ServiceEvent::NotesReconcileDue { peer_id, generation } => {
                if self.link_gen.get(&peer_id) != Some(&generation) {
                    return Vec::new(); // timer stantio, link già rimpiazzato/chiuso
                }
                let entries = self
                    .notes
                    .all()
                    .iter()
                    .map(|n| (n.id.clone(), crate::notes::digest::digest_of(n)))
                    .collect();
                vec![Effect::SendToPeer(peer_id, ChatMsg::NotesDigest { entries })]
            }

            // --- Task 6: RequestAdmissionUi — pulsante "chiedi di entrare" ---
            //
            // Solo significativo da `Rejected` (dopo un rifiuto): il cooldown di
            // 30s è già stato rispettato dalla UI (il pulsante resta disabilitato
            // finché non scade, vedi il doc-comment di `SelfAdmission::Rejected`).
            // In ogni altro stato (es. doppio click, evento duplicato) è un no-op:
            // non ha senso ri-chiedere l'ammissione se non siamo stati rifiutati.
            ServiceEvent::RequestAdmissionUi => {
                if self.self_admission != SelfAdmission::Rejected {
                    return Vec::new();
                }
                let server_id = match self.channel.role() {
                    Role::Client(id) => id,
                    other => {
                        tracing::debug!(
                            "aichat: RequestAdmissionUi ricevuta ma il ruolo non è Client (era {other:?}) — ignorata"
                        );
                        return Vec::new();
                    }
                };
                self.request_admission(server_id)
            }

            // Task 5 + Task 6b: la risposta del proprio umano al gate 2 ("ammetti
            // *candidate_label*?"). QUESTO evento arriva identico sia sull'umano-
            // SERVER (che coordina il voto) sia su un umano-PRESENTE che è un
            // CLIENT (che ha appena visto il gate 2 mostrato dal fix gemello in
            // `ChatMsg::AdmissionVoteRequest` sopra) — la UI non distingue i due
            // casi, quindi `handle_event` deve farlo qui in base al proprio ruolo:
            //
            // - `Role::Server`: il proprio umano è un presente come un altro agli
            //   occhi di `record_vote` (Task 5 INVARIATO) — usiamo `self.me.id`
            //   come "voter", esattamente il `PeerId` che `start_admission_vote`
            //   ha inserito in `present` per rappresentare il proprio umano.
            // - `Role::Client(server_id)`: NON abbiamo `pending_votes` (è solo
            //   stato server) — il voto deve viaggiare in rete fino al
            //   coordinatore, come `ChatMsg::AdmissionVote`. Prima di questo fix
            //   questo ramo chiamava sempre `record_vote`: su un client
            //   `pending_votes` è sempre vuoto, quindi il voto veniva scartato in
            //   silenzio (bug — vedi il doc-comment del Task 6b più sopra).
            // - `Role::Undecided`: caso anomalo (il gate 2 è mostrato solo dopo
            //   un ruolo deciso), nessun destinatario sensato per il voto — no-op.
            ServiceEvent::AdmissionVoteUi { candidate_label, accept } => match self.channel.role() {
                Role::Server => self.record_vote(&candidate_label, self.me.id, accept),
                Role::Client(server_id) => vec![Effect::SendToPeer(
                    server_id,
                    ChatMsg::AdmissionVote { candidate: candidate_label, accept },
                )],
                Role::Undecided => {
                    tracing::debug!(
                        "aichat: AdmissionVoteUi ricevuta ma il ruolo è Undecided — voto scartato"
                    );
                    Vec::new()
                }
            },

            // Task 5: il timer del voto per `candidate` è scaduto senza che tutti
            // i presenti avessero votato. "Silenzio = sì": se il turno è ANCORA
            // aperto (nessun veto lo ha già risolto in reject nel frattempo — in
            // quel caso `pending_votes` non contiene più `candidate` e questo arm
            // è un no-op, vedi il guard `contains_key`), i non-votanti contano
            // come sì e ammettiamo direttamente, senza richiamare `record_vote`
            // (che si aspetta un voto vero e proprio, non l'assenza di uno).
            ServiceEvent::VoteTimeout { candidate, generation } => {
                // FIX #1 (review 2026-07-03): risolvi SOLO se il voto ancora in corso
                // per questo candidato è ESATTAMENTE quello a cui apparteneva il timer
                // (stessa generazione). Un timer in ritardo di un turno già risolto —
                // il candidato è stato rifiutato e ha ri-chiesto — porta una
                // generazione vecchia: non combacia, no-op. Il vecchio guard
                // `contains_key` da solo risolveva erroneamente il turno NUOVO.
                match self.pending_votes.get(&candidate) {
                    Some(pv) if pv.generation == generation => {
                        let pv = self
                            .pending_votes
                            .remove(&candidate)
                            .expect("get(candidate) appena sopra è Some");
                        self.resolve_admit(candidate, pv)
                    }
                    _ => Vec::new(),
                }
            }
            // (Server, FIX #5) il cooldown di re-request per `candidate` è scaduto:
            // esce da `cooling_down` e potrà di nuovo chiedere l'ammissione. No-op
            // per un candidato non in cooldown (idempotente).
            ServiceEvent::CooldownExpired { candidate } => {
                self.cooling_down.remove(&candidate);
                Vec::new()
            }
            // (Destinatario) Il timer di scadenza (24h, avviato da `handle_share_offer`)
            // è scattato. Se l'entry è ancora in `pending_shares` — `AwaitingDecision`
            // (mai risposto), `AwaitingContent` (accettata ma il contenuto non è ancora
            // arrivato), o `AwaitingUiWrite` (contenuto arrivato ma mai scritto su disco)
            // — la rimuoviamo e avvertiamo il mittente originale con `ShareExpired`, che
            // farà risolvere `outgoing_shares` via `resolve_outgoing_share` sopra con
            // `ShareOutcome::Failed`. Se invece `share_id` non è (più) in
            // `pending_shares` (l'umano ha già RIFIUTATO prima che il timer scadesse —
            // un timer stantio, esattamente come `VoteTimeout` per un voto già chiuso —
            // oppure la UI ha già confermato la scrittura con `ShareWritten`), no-op:
            // non c'è nulla da notificare.
            ServiceEvent::ShareExpiryTimeout { share_id } => {
                let Some(ps) = self.pending_shares.remove(&share_id) else {
                    return Vec::new();
                };
                let from_label = match &ps {
                    PendingShare::AwaitingDecision { from_label, .. } => from_label,
                    PendingShare::AwaitingContent { from_label, .. } => from_label,
                    PendingShare::AwaitingUiWrite { from_label, .. } => from_label,
                };
                match crate::aichat::relay::route_to_label(from_label, &self.peers) {
                    Some(sender_id) => {
                        vec![Effect::SendToPeer(sender_id, ChatMsg::ShareExpired { share_id })]
                    }
                    None => {
                        // Il mittente originale non è più raggiungibile — best-effort,
                        // nessuno a cui notificare. Non un errore.
                        Vec::new()
                    }
                }
            }
        }
    }

    /// Pubblica un messaggio nella stanza da `from_label`: lo aggiunge allo storico
    /// locale, lo mostra nella propria UI, e lo inoltra in rete secondo il ruolo
    /// corrente (Server → broadcast a tutti i client connessi; Client → invia solo al
    /// server, che farà il relay agli altri; Undecided → nessun peer noto, niente
    /// inoltro). PURA come `handle_event` (nessun I/O: solo mutazione di `self.history`
    /// e costruzione di `Vec<Effect>`).
    ///
    /// Riusata da TRE chiamanti che differiscono SOLO nel mittente (SOLID/DRY —
    /// evita di duplicare questa logica in tre arm quasi identici di `handle_event`):
    /// - `ServiceEvent::HumanSay` → `from_label = "<base>-human"`;
    /// - `ServiceEvent::AiReply` (AI Chat Slice 1a) → `from_label = "<base>-ai"`;
    /// - `ServiceEvent::AutoParticipateDone { reply: Some(_) }` (AI Chat Slice 2) →
    ///   `from_label = "<base>-ai"` (stesso percorso -ai di `AiReply`, cambia solo
    ///   COME il testo è stato ottenuto: giudizio spontaneo invece di invocazione).
    ///
    /// ## Choke-point del conteggio turni (Slice 2, design §10.3)
    ///
    /// Questo è l'UNICO punto in cui i messaggi PROPRI (miei, sia `-human` sia `-ai`)
    /// entrano nello storico — per questo è anche l'UNICO punto in cui chiamiamo
    /// `note_room_message` per loro. Il messaggio "gemello" che arriva da un ALTRO
    /// peer (`ServiceEvent::PeerMsg { msg: ChatMsg::Say, .. }`) NON passa da qui (ha
    /// il proprio `self.history.push` nell'arm `PeerMsg`) — lì `note_room_message`
    /// viene chiamato esplicitamente, UNA volta, accanto a quel push. Risultato: ogni
    /// riga che entra in `self.history` innesca `note_room_message` esattamente una
    /// volta, da esattamente uno dei due punti — mai zero, mai due.
    fn publish_say(&mut self, from_label: String, text: String) -> Vec<Effect> {
        let mut effects = Vec::new();

        // `is_ai`/`display_name` (Task 2, campi additivi): calcolati QUI, una sola
        // volta, dalla stessa `from_label` che il resto della funzione già usa per
        // instradare — riusa `ends_with("-ai")`, lo stesso pattern già in uso altrove
        // in questo file (vedi `note_room_message` sotto) per non introdurre una
        // seconda fonte di verità. Nessun sito esistente che fa
        // `from_label.ends_with(...)` per routing/gating cambia: questo è un campo
        // cosmetico calcolato in parallelo. `publish_say` è chiamata SOLO per
        // messaggi ORIGINATI da questa macchina (vedi il doc-comment sopra sui tre
        // chiamanti) — per questo qui è corretto CALCOLARE, mentre l'arm
        // `PeerMsg::Say` (messaggio di un ALTRO peer) deve invece solo INOLTRARE i
        // valori già stampati dalla `publish_say` di quel peer.
        let is_ai = from_label.ends_with("-ai");
        let display_name = if is_ai { self.ai_display_name.clone() } else { self.display_name.clone() };

        // Registra nello storico locale PRIMA di costruire il messaggio di rete
        // (stesso principio del branch PeerMsg::Say: ogni peer che vede un messaggio,
        // anche il proprio, lo registra).
        self.history.push(ChatLine {
            from_label: from_label.clone(),
            text: text.clone(),
            display_name: display_name.clone(),
            is_ai,
        });
        // Slice 2: aggiorna contatore/last_speaker per QUESTO messaggio — vedi il
        // doc-comment sopra sul choke-point.
        self.note_room_message(&from_label);

        // Costruiamo il `ChatMsg::Say` che verrà inviato in rete ai peer.
        // `text.clone()` perché `text` viene anche usato nell'eco locale sotto.
        let say = ChatMsg::Say {
            from_label: from_label.clone(),
            text: text.clone(),
            display_name: display_name.clone(),
            is_ai,
        };

        // (1) Eco alla propria UI: `Effect::ToUi` — il loop eseguirà `.send()` sul tx.
        // Il messaggio appare nella finestra chat locale prima ancora di andare in rete.
        effects.push(Effect::ToUi(ServerMsg::AiChatMessage {
            from_label,
            text,
            display_name,
            is_ai,
        }));

        // (2) Inoltro in rete secondo il ruolo corrente.
        // `self.channel.role()` è `Copy`, nessun clone necessario.
        match self.channel.role() {
            Role::Server => {
                // Il server fa da hub: invia una copia del messaggio a ogni client.
                // `say.clone()` perché ogni iterazione produce un `Effect` indipendente.
                for &peer in &self.connected {
                    effects.push(Effect::SendToPeer(peer, say.clone()));
                }
            }
            Role::Client(server) => {
                // Il client invia solo al server (che poi farà il relay agli altri client).
                // `say` viene mosso (non clonato): è l'unico destinatario.
                effects.push(Effect::SendToPeer(server, say));
            }
            Role::Undecided => {
                // Nessun peer noto: siamo soli, niente inoltro.
            }
        }

        effects
    }

    // =========================================================================
    // AI Chat Slice 2 — auto-partecipazione (giudizio di rilevanza + cap turni).
    // Design: Docs/superpowers/specs/2026-07-02-aichat-ai-participants-design.md §10.
    // =========================================================================

    /// Registra un messaggio APPENA ENTRATO nella conversazione (`self.history`):
    /// aggiorna il contatore di turni AI consecutivi e l'ultimo mittente visto.
    ///
    /// Chiamato ESATTAMENTE una volta per riga di storico, da due punti (mai
    /// entrambi per lo stesso messaggio — vedi il doc-comment "choke-point" su
    /// `publish_say`):
    /// - `publish_say` (i MIEI messaggi: `HumanSay`, `AiReply`,
    ///   `AutoParticipateDone{Some}` — tutti passano da lì);
    /// - l'arm `PeerMsg::Say` (i messaggi di un ALTRO peer, relayati).
    ///
    /// Logica (design §10.3): un mittente `-human` azzera il contatore (un umano ha
    /// ripreso la parola: la "conversazione AI" corrente, se c'era, è chiusa); un
    /// mittente `-ai` lo incrementa (un altro turno AI, mio o altrui, si aggiunge al
    /// burst corrente). Un `from_label` che non finisce né in `-human` né in `-ai`
    /// (non dovrebbe accadere nel protocollo attuale) non tocca il contatore — solo
    /// `last_speaker_label` viene comunque aggiornato, perché riflette
    /// incondizionatamente "chi ha parlato per ultimo".
    fn note_room_message(&mut self, from_label: &str) {
        if from_label.ends_with("-human") {
            self.consecutive_ai_turns = 0;
        } else if from_label.ends_with("-ai") {
            self.consecutive_ai_turns += 1;
        }
        self.last_speaker_label = Some(from_label.to_string());
    }

    /// Valuta se la mia AI locale deve essere invitata a giudicare la rilevanza del
    /// messaggio appena registrato (design §10.3) e, se sì, prepara l'effetto.
    ///
    /// **Precondizione**: va chiamata DOPO `note_room_message` per lo STESSO
    /// messaggio (`triggering_from_label` deve coincidere con l'ultimo
    /// `from_label` passato a `note_room_message`) — altrimenti le guardie 3/4
    /// (last-speaker/cap) giudicherebbero uno stato non ancora aggiornato per il
    /// messaggio corrente. Tutti i chiamanti in `handle_event` rispettano questo
    /// ordine (vedi gli arm `HumanSay`/`PeerMsg::Say`).
    ///
    /// Scatta SOLO se TUTTE le guardie passano:
    /// 1. `ai_autoparticipate` è attivo (opt-in, design §10.1);
    /// 2. il messaggio non è la mia stessa `-ai` (non giudico i miei contributi);
    /// 3. l'ultimo a parlare non ero io (`last_speaker_label != Some(mia -ai)`) —
    ///    non mi auto-invito a parlare due volte di fila;
    /// 4. il cap sui turni AI consecutivi non è stato raggiunto
    ///    (`consecutive_ai_turns < MAX_CONSECUTIVE_AI_TURNS`) — è il "controllo del
    ///    loop" della slice, vedi il doc-comment di `MAX_CONSECUTIVE_AI_TURNS`;
    /// 5. nessun giudizio è già in volo (`!autoparticipate_inflight`) — un solo
    ///    giudizio alla volta per macchina.
    ///
    /// Se scatta, marca `autoparticipate_inflight = true` (guardia 5 per le
    /// prossime chiamate, finché `AutoParticipateDone` non la azzera) e ritorna
    /// `[Effect::AutoParticipate { history, my_ai_label }]`; altrimenti `Vec::new()` —
    /// PURA come `handle_event` (nessun I/O, la history è costruita qui da
    /// `build_ai_history`, funzione pura).
    fn maybe_autoparticipate(&mut self, triggering_from_label: &str) -> Vec<Effect> {
        let my_ai_label = format!("{}-ai", self.me.label_base);

        let should_trigger = self.ai_autoparticipate
            && triggering_from_label != my_ai_label
            && self.last_speaker_label.as_deref() != Some(my_ai_label.as_str())
            && self.consecutive_ai_turns < MAX_CONSECUTIVE_AI_TURNS
            && !self.autoparticipate_inflight;

        if !should_trigger {
            return Vec::new();
        }

        self.autoparticipate_inflight = true;
        vec![Effect::AutoParticipate {
            history: build_ai_history(&self.history, &my_ai_label),
            my_ai_label,
        }]
    }

    /// Costruisce uno snapshot del `Roster` dei peer attualmente connessi.
    ///
    /// Itera su `self.connected` (gli id dei peer con link TCP vivo) e — per ciascuno —
    /// recupera la `PeerInfo` dalla mappa `self.peers`. Il roster risultante non include
    /// `me` stesso: questo è corretto per `server_relay`, che esclude autore e server via
    /// `fan_out_targets` (il server è già escluso dal suo stesso metodo, indipendentemente
    /// dalla sua presenza nel roster).
    ///
    /// Analogia OOP: è un metodo "factory" privato che produce un value object immutabile
    /// da usare come parametro di query — nessuna mutazione dello stato.
    fn roster_snapshot(&self) -> crate::aichat::peer::Roster {
        let mut r = crate::aichat::peer::Roster::new();
        for id in self.connected.iter() {
            // `peers.get` ritorna `Option<&PeerInfo>`; se il peer è in `connected` ma non
            // in `peers` (caso anomalo), lo saltiamo silenziosamente.
            if let Some(info) = self.peers.get(id) {
                r.upsert(info.clone());
            }
        }
        r
    }

    /// (SERVER) Avvia il voto di ammissione per un candidato che ha appena mandato
    /// `ChatMsg::RequestAdmission` (Task 4 —
    /// `Docs/superpowers/plans/2026-07-03-aichat-admission.md`).
    ///
    /// I "presenti" che devono votare sono i client GIÀ AMMESSI (`self.admitted`)
    /// **più il proprio umano-server** (`self.me.id`): il server non è un
    /// semplice coordinatore passivo, è anche un votante — il suo umano vede il
    /// gate 2 nella propria UI (l'`Effect::ToUi(AiChatAdmissionRequest)` qui
    /// sotto) e il suo voto arriva più tardi come `ServiceEvent::AdmissionVoteUi`
    /// (Task 5, `record_vote`). Se non lo includessimo in `present`, il turno
    /// potrebbe chiudersi (admit) SENZA che l'umano-server abbia mai potuto dire
    /// la sua. MAI `self.connected` per i client remoti: un client connesso ma
    /// non ancora ammesso (un voto altrui ancora in corso, o mai partito) non ha
    /// titolo per votare — vedi il doc-comment del campo `admitted`. Il filtro su
    /// `candidate` è una difesa esplicita (il candidato non dovrebbe MAI comparire
    /// in `admitted` prima di essere ammesso, ma escluderlo qui rende l'invariante
    /// innegoziabile anche se qualcos'altro andasse storto altrove).
    ///
    /// PURA come `handle_event` (nessun I/O): registra il voto in `pending_votes`
    /// e ritorna gli `Effect` che il loop `run` dovrà eseguire — una richiesta di
    /// voto (`ChatMsg::AdmissionVoteRequest`) per ciascun presente REMOTO (mai a
    /// se stessi: non si manda un messaggio di rete al proprio stesso processo),
    /// il gate 2 alla propria UI (il voto del proprio umano-server passa da qui,
    /// non da un `SendToPeer`), e l'avvio del timer del voto (silenzio-oltre-
    /// timeout = sì, Task 5/7).
    fn start_admission_vote(&mut self, candidate: PeerId, label: String) -> Vec<Effect> {
        let mut present: HashSet<PeerId> =
            self.admitted.iter().filter(|id| **id != candidate).copied().collect();
        // Il proprio umano-server è sempre un presente: vota anche lui (vedi il
        // doc-comment sopra).
        present.insert(self.me.id);

        let mut effects = Vec::with_capacity(present.len() + 2);
        for &peer in &present {
            // Non mandiamo un `ChatMsg` a noi stessi: il nostro voto lo raccogliamo
            // tramite `ToUi` + `ServiceEvent::AdmissionVoteUi`, non tramite rete.
            if peer == self.me.id {
                continue;
            }
            effects.push(Effect::SendToPeer(
                peer,
                ChatMsg::AdmissionVoteRequest { candidate: label.clone() },
            ));
        }
        // Il gate 2 arriva anche al proprio umano-server: il SUO voto è
        // raccolto come un presente in più (Task 5, `ServiceEvent::AdmissionVoteUi`).
        effects.push(Effect::ToUi(ServerMsg::AiChatAdmissionRequest { candidate: label.clone() }));
        // FIX #1 (review 2026-07-03): assegna a QUESTO turno una generazione
        // monotona univoca. Il timer la porta con sé; alla scadenza `VoteTimeout`
        // la confronta con la generazione del voto ancora in corso — se il
        // candidato nel frattempo è stato rifiutato e ha ri-chiesto, il turno
        // corrente ha una generazione diversa e il timer vecchio è ignorato.
        let generation = self.next_vote_gen;
        self.next_vote_gen += 1;
        // Se nessuno risponde entro `ADMISSION_VOTE_TIMEOUT_SECS`, il silenzio
        // conta come sì (Task 5): nessun voto resta bloccato per sempre.
        effects.push(Effect::StartVoteTimeout {
            candidate,
            generation,
            secs: ADMISSION_VOTE_TIMEOUT_SECS,
        });

        self.pending_votes
            .insert(candidate, PendingVote { label, present, votes: HashMap::new(), generation });

        effects
    }

    /// (SERVER) Registra il voto di `voter` per il candidato identificato dalla
    /// sua ETICHETTA `candidate_label` (non il suo `PeerId`: il wire —
    /// `ChatMsg::AdmissionVote` — porta l'etichetta testuale, non un `PeerId`
    /// interno, perché è quello che un presente remoto e la UI dell'umano-server
    /// conoscono entrambi — vedi il doc-comment di `ChatMsg::RequestAdmission`).
    ///
    /// Risolve subito il turno se:
    /// - `accept == false` → **VETO immediato** (basta un solo no: AND logico fra
    ///   tutti i presenti) → `resolve_reject`.
    /// - `accept == true` e ORA **tutti** i presenti hanno votato sì → `resolve_admit`.
    /// - altrimenti (mancano ancora dei sì) → nessun effetto, il turno resta aperto.
    ///
    /// Un voto per un'etichetta SENZA `PendingVote` corrispondente è STALE (il
    /// turno è già stato risolto altrove — un veto precedente, o un timeout) e
    /// viene ignorato silenziosamente: in un sistema distribuito un voto può
    /// sempre arrivare dopo che la decisione è già stata presa, non è un errore.
    fn record_vote(&mut self, candidate_label: &str, voter: PeerId, accept: bool) -> Vec<Effect> {
        // Il wire porta l'ETICHETTA, non il `PeerId`: non abbiamo un secondo
        // indice label→PeerId (i voti sono rari — un turno alla volta, in
        // pratica), quindi una scansione lineare su `pending_votes`
        // (tipicamente 0-1 elementi) è più che sufficiente.
        let candidate_id = match self
            .pending_votes
            .iter()
            .find(|(_, pv)| pv.label == candidate_label)
            .map(|(&id, _)| id)
        {
            Some(id) => id,
            None => {
                tracing::debug!(
                    "aichat: voto per '{candidate_label}' ricevuto ma nessun voto in corso (stale?)"
                );
                return Vec::new();
            }
        };

        // FIX #2 (review 2026-07-03): solo un peer PRESENTE (uno di quelli
        // invitati a votare quando il turno è partito) può influenzare l'esito.
        // Senza questo controllo, un `AdmissionVote{accept:false}` da un peer
        // connesso ma NON presente (un secondo candidato, o un peer già rifiutato
        // ancora linkato) chiamava `resolve_reject` e vietava l'ammissione altrui.
        // Un `sì` da un estraneo era già inerte (`all_yes` scorre solo `present`),
        // quindi il guard qui rende la verifica simmetrica per entrambi i voti.
        if !self
            .pending_votes
            .get(&candidate_id)
            .map(|pv| pv.present.contains(&voter))
            .unwrap_or(false)
        {
            tracing::debug!(
                "aichat: voto per '{candidate_label}' da {voter:?} ignorato: non è tra i presenti del turno"
            );
            return Vec::new();
        }

        if !accept {
            // Veto: un solo no basta per chiudere il turno in rifiuto — rimuoviamo
            // subito `pending_votes` (nessun altro voto verrà più raccolto per
            // questo candidato).
            let pv = self.pending_votes.remove(&candidate_id).expect("candidate_id trovato sopra");
            return self.resolve_reject(candidate_id, pv);
        }

        // Sì: registriamo il voto PRIMA di controllare se il turno è completo —
        // altrimenti l'ultimo "sì" atteso non conterebbe mai come "tutti hanno
        // votato". Il blocco `{ ... }` delimita il prestito mutabile (`pv`) a
        // queste due righe: finisce prima del successivo `self.pending_votes.remove`,
        // evitando qualunque conflitto di borrow con `self.pending_votes` sotto.
        let all_yes = {
            let pv = self.pending_votes.get_mut(&candidate_id).expect("candidate_id trovato sopra");
            pv.votes.insert(voter, true);
            pv.present.iter().all(|id| pv.votes.get(id) == Some(&true))
        };

        if all_yes {
            let pv = self.pending_votes.remove(&candidate_id).expect("candidate_id trovato sopra");
            self.resolve_admit(candidate_id, pv)
        } else {
            // Attendiamo ancora altri voti sì: nessun effetto da produrre ora.
            Vec::new()
        }
    }

    /// (SERVER) Il voto per `candidate` si è risolto in AMMISSIONE — nessun veto
    /// arrivato, e tutti i presenti hanno detto sì (esplicitamente, o per
    /// timeout: silenzio = sì, Task 5/7).
    ///
    /// Aggiunge il candidato al `Room` (`on_server_join`, che aggiorna anche lo
    /// snapshot autorevole dei partecipanti) e notifica:
    /// - il candidato stesso riceve `Admitted` (si sblocca — Task 6) **e** il
    ///   `Roster` aggiornato (che ora lo include: gli dà la lista presenti);
    /// - ogni ALTRO client già ammesso riceve solo il `Roster` aggiornato (vede
    ///   il nuovo membro comparire — non gli serve un `Admitted`, non è lui il
    ///   candidato).
    ///
    /// `roster_msg` (già una `ChatMsg::Roster` pronta, ritornata da
    /// `on_server_join`) è lo stesso pattern già usato dall'arm `ChatMsg::Join`
    /// più sopra in questo file: costruito UNA volta, clonato per ogni
    /// destinatario (ogni `Effect` deve restare indipendente).
    ///
    /// Task 8 (rimozione del vecchio consenso pairwise): il vecchio arm
    /// `ChatMsg::Join` (ora dead code, nessuno lo manda più) mandava anche il
    /// dump di storico (`ChatMsg::History`) al peer appena entrato, così la
    /// sua finestra-chat si ricostruiva col trascritto completo invece di
    /// partire "muta". Il nuovo flusso a voto deve preservare lo stesso
    /// comportamento: lo facciamo qui, SOLO al candidato appena ammesso (non
    /// un broadcast — gli altri presenti hanno già visto ogni riga passare
    /// dal vivo via il relay a stella, §3 dello spec storico).
    fn resolve_admit(&mut self, candidate: PeerId, pv: PendingVote) -> Vec<Effect> {
        // FIX #7 (review 2026-07-03): avvisa i presenti che il turno è chiuso PRIMA
        // di mutare lo stato (borrow immutabile di `pv`/`self`, termina qui).
        let mut effects = self.notify_vote_resolved(&pv);

        self.admitted.insert(candidate);

        let (roster_msg, participants) = self.channel.on_server_join(pv.label.clone());
        self.last_known_roster = participants.clone();

        effects.reserve(self.admitted.len() + 3);
        effects.push(Effect::SendToPeer(candidate, ChatMsg::Admitted { label: pv.label.clone() }));
        effects.push(Effect::SendToPeer(candidate, ChatMsg::History { entries: self.history.clone() }));
        // `self.admitted` include GIÀ il candidato appena inserito sopra: riceve
        // sia `Admitted` (appena sopra) sia questo `Roster` (che ora lo contiene).
        for &c in &self.admitted {
            effects.push(Effect::SendToPeer(c, roster_msg.clone()));
        }
        effects.push(Effect::ToUi(ServerMsg::AiChatRoster { participants }));

        effects
    }

    /// (SERVER) Il voto per `candidate` si è risolto in RIFIUTO (veto: almeno un
    /// presente ha votato no). Il candidato NON entra in `admitted`/nel roster —
    /// riceve solo la notifica di rifiuto, così la sua UI può proporre un
    /// re-request dopo il cooldown (Task 6, `SelfAdmission::Rejected`).
    fn resolve_reject(&mut self, candidate: PeerId, pv: PendingVote) -> Vec<Effect> {
        // FIX #5 (review 2026-07-03): il candidato rifiutato entra in cooldown lato
        // SERVER — un suo re-request prima della scadenza sarà ignorato (vedi l'arm
        // `ChatMsg::RequestAdmission`). Il timer è un Effect (come `StartVoteTimeout`)
        // per non leggere l'orologio dentro l'attore: `CooldownExpired` lo toglierà
        // da `cooling_down`.
        self.cooling_down.insert(candidate);
        // FIX #7: avvisa i presenti (banner del gate 2) che il turno è chiuso —
        // costruito prima di consumare `pv.label` nell'AdmissionRejected sotto.
        let mut effects = self.notify_vote_resolved(&pv);
        effects.push(Effect::SendToPeer(candidate, ChatMsg::AdmissionRejected { label: pv.label }));
        effects.push(Effect::StartCooldownTimer { candidate, secs: ADMISSION_REREQUEST_COOLDOWN_SECS });
        effects
    }

    /// (SERVER, FIX #7 — review 2026-07-03) Costruisce le notifiche di CHIUSURA del
    /// turno `pv` per tutti i suoi presenti (quelli che hanno ricevuto il gate 2),
    /// così le loro UI tolgono il banner "ammetti X?". Il proprio umano-server
    /// (`self.me.id`, sempre presente) riceve un `ToUi(AiChatAdmissionResolved)`; i
    /// presenti REMOTI un `ChatMsg::AdmissionResolved` (che il loro `handle_event`
    /// ribalta in `ToUi` — vedi l'arm gemello di `AdmissionVoteRequest`). Il
    /// candidato NON è tra i `present` (è escluso in `start_admission_vote`): riceve
    /// la propria notifica (`Admitted`/`AdmissionRejected`) altrove, non questa.
    /// PURA: nessuna mutazione di `self`, solo lettura di `pv.present`/`pv.label`.
    fn notify_vote_resolved(&self, pv: &PendingVote) -> Vec<Effect> {
        let mut effects = Vec::with_capacity(pv.present.len());
        for &peer in &pv.present {
            if peer == self.me.id {
                effects.push(Effect::ToUi(ServerMsg::AiChatAdmissionResolved {
                    candidate: pv.label.clone(),
                }));
            } else {
                effects.push(Effect::SendToPeer(
                    peer,
                    ChatMsg::AdmissionResolved { candidate: pv.label.clone() },
                ));
            }
        }
        effects
    }

    // -------------------------------------------------------------------------
    // Ammissione alla stanza — Task 6: il NUOVO ARRIVATO (client). Le due
    // funzioni sotto sono PURE (nessun I/O, solo mutazione di `self` +
    // `Vec<Effect>` in ritorno) — stesso stile del resto del file: `perform`
    // (o, per `begin_join_gate`, il ramo `ConnectOutcome::Success` di `run`)
    // esegue gli effetti, questa logica decide solo COSA fare.
    // -------------------------------------------------------------------------

    /// (Nuovo arrivato) Mostra il gate 1 alla UI ("vuoi entrare con [presenti]?")
    /// e passa in `SelfAdmission::Deciding`, in attesa della risposta
    /// dell'umano (`ServiceEvent::JoinDecision`).
    ///
    /// Chiamata da `run()` subito dopo un connect riuscito verso il server
    /// eletto (ramo `ConnectOutcome::Success` del `select!`), **al posto** del
    /// vecchio invio immediato di `ChatMsg::Join` — quel `Join` partiva prima
    /// che esistesse un qualunque gate: ora il nuovo arrivato deve prima
    /// superare il gate 1 (qui) e poi il gate 2 (voto dei presenti, Task 4/5)
    /// prima di essere ammesso davvero.
    ///
    /// Guardia `NotJoining`: se abbiamo già in corso (o concluso) un
    /// tentativo di ingresso — per esempio una riconnessione TCP dopo un link
    /// caduto mentre eravamo già `Pending`/`Admitted` — non ripresentiamo il
    /// gate 1 da capo. `handle_event` non chiama mai questa funzione
    /// direttamente: vive fuori dal match di `ServiceEvent` perché non
    /// risponde a un evento del dominio applicativo, ma a un dettaglio del
    /// loop I/O (il connect è appena riuscito) — esattamente come
    /// `register_connect_success` qui sotto non è un arm di `handle_event`.
    fn begin_join_gate(&mut self) -> Vec<Effect> {
        if self.self_admission != SelfAdmission::NotJoining {
            return Vec::new();
        }
        self.self_admission = SelfAdmission::Deciding;
        vec![Effect::ToUi(ServerMsg::AiChatJoinPrompt { present: self.known_peer_labels() })]
    }

    /// (Nuovo arrivato) Manda `ChatMsg::RequestAdmission` al server e passa in
    /// `Pending`, avvisando la UI che siamo in attesa del voto dei presenti
    /// (gate 2, Task 4/5). Fattorizzata perché lo stesso passo si ripete in
    /// due punti del Task 6: quando l'umano accetta il gate 1
    /// (`ServiceEvent::JoinDecision{accept:true}`) e quando ri-chiede
    /// l'ingresso dopo un rifiuto (`ServiceEvent::RequestAdmissionUi`, con il
    /// cooldown di 30s già rispettato dalla UI).
    fn request_admission(&mut self, server_id: PeerId) -> Vec<Effect> {
        self.self_admission = SelfAdmission::Pending;
        let label = format!("{}-human", self.me.label_base);
        vec![
            Effect::SendToPeer(server_id, ChatMsg::RequestAdmission { label }),
            Effect::ToUi(ServerMsg::AiChatPending { present: self.known_peer_labels() }),
        ]
    }

    /// (Destinatario) Un `ShareOffer` è arrivato. Due cap-check prima di registrare
    /// qualunque cosa (§8): dimensione, poi conteggio pendenti — entrambi passano da
    /// `auto_reject_share` (stesso esito, motivo di log diverso).
    fn handle_share_offer(
        &mut self,
        share_id: String,
        from_label: String,
        doc_name: String,
        size_bytes: u64,
    ) -> Vec<Effect> {
        if size_bytes > MAX_SHARE_SIZE_BYTES {
            // Non fidarsi del mittente (Task 5 controlla anche lì, ma un mittente
            // compromesso o con un bug potrebbe mandare comunque un size_bytes falso).
            return self.auto_reject_share(
                share_id,
                &from_label,
                &format!(
                    "'{doc_name}' da {from_label}: {size_bytes} byte sopra il cap di {MAX_SHARE_SIZE_BYTES}"
                ),
            );
        }

        if self.pending_shares.len() >= MAX_PENDING_SHARES {
            return self.auto_reject_share(
                share_id,
                &from_label,
                &format!(
                    "'{doc_name}' da {from_label}: cap di {MAX_PENDING_SHARES} trasferimenti pendenti raggiunto"
                ),
            );
        }

        self.pending_shares.insert(
            share_id.clone(),
            PendingShare::AwaitingDecision {
                from_label: from_label.clone(),
                doc_name: doc_name.clone(),
                size_bytes,
            },
        );

        let mut effects = vec![Effect::StartShareExpiryTimer {
            share_id: share_id.clone(),
            secs: SHARE_EXPIRY_SECS,
        }];
        if self.has_ui() {
            effects.push(Effect::ToUi(ServerMsg::ShareRequest {
                share_id,
                from_label,
                doc_name,
                size_bytes,
            }));
        }
        effects
    }

    /// (Destinatario, Slice 2a) Il contenuto vero è arrivato per un'offerta che deve
    /// essere `AwaitingContent` — cioè GIÀ ACCETTATA dall'umano (`share_consent` l'ha
    /// transizionata). Fix di integrità del consenso (review finale Slice 2a): un
    /// `ShareData` che arriva mentre l'entry è ancora `AwaitingDecision` (mai decisa) è
    /// un no-op silenzioso, esattamente come uno `share_id` sconosciuto — non
    /// un'accettazione implicita. Niente contenuto prima del consenso esplicito (spec
    /// §8). Rivalida anche la dimensione reale contro `MAX_SHARE_SIZE_BYTES` (l'offerta
    /// iniziale dichiarava una dimensione, ma non ci fidiamo ciecamente del mittente —
    /// stessa filosofia già applicata lato mittente in `handle_share_content`): se
    /// supera il cap, rimuove l'entry e rifiuta attivamente (stesso trattamento
    /// dell'offerta iniziale sovradimensionata, via `auto_reject_share`). Altrimenti
    /// transiziona ad `AwaitingUiWrite`; se la UI è connessa, la scrittura può iniziare
    /// subito (`ShareIncomingData`), altrimenti resta in coda per il replay al prossimo
    /// `SetServerTx`.
    fn handle_share_data(&mut self, share_id: String, title: String, content: String) -> Vec<Effect> {
        let from_label = match self.pending_shares.get(&share_id) {
            Some(PendingShare::AwaitingContent { from_label, .. }) => from_label.clone(),
            _ => {
                tracing::debug!(
                    "aichat: ShareData per '{share_id}' ricevuto ma nessuna offerta AwaitingContent corrispondente (prematuro, stale o anomalo?)"
                );
                return Vec::new();
            }
        };
        if content.len() as u64 > MAX_SHARE_SIZE_BYTES {
            self.pending_shares.remove(&share_id);
            return self.auto_reject_share(
                share_id,
                &from_label,
                &format!(
                    "contenuto ricevuto sopra il cap di {MAX_SHARE_SIZE_BYTES} byte (dichiarato più piccolo nell'offerta)"
                ),
            );
        }
        self.pending_shares.insert(
            share_id.clone(),
            PendingShare::AwaitingUiWrite {
                from_label: from_label.clone(),
                title: title.clone(),
                content: content.clone(),
            },
        );
        if self.has_ui() {
            vec![Effect::ToUi(ServerMsg::ShareIncomingData { share_id, from_label, title, content })]
        } else {
            Vec::new()
        }
    }

    /// Auto-rifiuta un `ShareOffer` prima ancora di registrarlo (cap di dimensione o di
    /// conteggio, §8) — logga `reason` e manda `ShareReject` al mittente se risolvibile
    /// per `from_label`. Condiviso dai due guard di `handle_share_offer` sopra: stesso
    /// esito di rete, motivo di log diverso.
    fn auto_reject_share(&self, share_id: String, from_label: &str, reason: &str) -> Vec<Effect> {
        tracing::warn!("aichat: ShareOffer auto-rifiutata: {reason}");
        match crate::aichat::relay::route_to_label(from_label, &self.peers) {
            Some(sender_id) => vec![Effect::SendToPeer(
                sender_id,
                ChatMsg::ShareReject { share_id, from_label: self.me.label_base.clone() },
            )],
            None => Vec::new(),
        }
    }

    /// (Mittente) Avvia una condivisione verso un target risolto per `label_base`.
    /// `rel_path` viene registrato in `outgoing_shares` (Slice 2a): servirà a costruire
    /// `ServerMsg::ShareContentRequest` quando arriverà un `ChatMsg::ShareAccept`.
    fn request_share(
        &mut self,
        rel_path: String,
        doc_name: String,
        size_bytes: u64,
        target: protocol::ShareTarget,
    ) -> Vec<Effect> {
        let target_label = match target {
            protocol::ShareTarget::One { label_base } => label_base,
            protocol::ShareTarget::All => {
                // Slice 3 implementerà il fan-out. Per ora: esito immediato, nessun invio.
                let share_id = self.fresh_share_id();
                return vec![Effect::ToUi(ServerMsg::ShareResult {
                    share_id,
                    target_label: String::new(),
                    doc_name,
                    outcome: protocol::ShareOutcome::Failed {
                        reason: "\"Share with all\" non è ancora disponibile".into(),
                    },
                })];
            }
        };

        if size_bytes > MAX_SHARE_SIZE_BYTES {
            // Verifica QUI, non solo lato UI (che in questa slice non esiste ancora):
            // `size_bytes` arriva da fuori (test oggi, UI domani) e non va mai fidato
            // ciecamente — stessa filosofia del cap sui trasferimenti pendenti sotto.
            let share_id = self.fresh_share_id();
            return vec![Effect::ToUi(ServerMsg::ShareResult {
                share_id,
                target_label,
                doc_name,
                outcome: protocol::ShareOutcome::Failed {
                    reason: format!(
                        "documento troppo grande: {size_bytes} byte, massimo {MAX_SHARE_SIZE_BYTES}"
                    ),
                },
            })];
        }

        let share_id = self.fresh_share_id();
        // Review finale (2026-07-05): `route_to_label` risolve dalla mera *scoperta* UDP
        // (`self.peers`), non dalla raggiungibilità TCP. In una topologia a stella con 3+
        // macchine un CLIENT può aver scoperto un altro CLIENT senza avere un link diretto
        // con lui (solo il server eletto ha link con tutti). Il `.filter` sotto esclude
        // esattamente questo caso PRIMA di emettere `SendToPeer`: l'esecutore generico di
        // quell'effetto in `perform` non fa fallback né notifica se non trova un link —
        // scarterebbe il messaggio in silenzio, lasciando un'entry orfana in
        // `outgoing_shares` e nessun feedback alla UI di chi ha chiesto la condivisione.
        let reachable = crate::aichat::relay::route_to_label(&target_label, &self.peers)
            .filter(|target_id| self.links.contains_key(target_id));
        match reachable {
            Some(target_id) => {
                self.outgoing_shares.insert(
                    share_id.clone(),
                    OutgoingShare {
                        target_label: target_label.clone(),
                        doc_name: doc_name.clone(),
                        rel_path,
                        stage: OutgoingShareStage::AwaitingAccept,
                    },
                );
                vec![Effect::SendToPeer(
                    target_id,
                    ChatMsg::ShareOffer {
                        share_id,
                        from_label: self.me.label_base.clone(),
                        doc_name,
                        size_bytes,
                    },
                )]
            }
            None => vec![Effect::ToUi(ServerMsg::ShareResult {
                share_id,
                target_label,
                doc_name,
                outcome: protocol::ShareOutcome::Failed {
                    reason: "macchina non trovata o non connessa".into(),
                },
            })],
        }
    }

    /// (Mittente, Slice 2a) Il destinatario ha accettato — riporta `Accepted` alla UI
    /// (il mittente vede "accettata" come sempre, invariato rispetto a prima di questa
    /// slice) e transiziona `outgoing_shares` ad `AwaitingContent`: serve ora il
    /// contenuto vero, che solo la UI può leggere (confine SRP). No-op se `share_id`
    /// non è (più) in `outgoing_shares` (stale — già scaduta o già risolta).
    fn handle_share_accepted(&mut self, share_id: String) -> Vec<Effect> {
        let Some(entry) = self.outgoing_shares.get_mut(&share_id) else {
            tracing::debug!(
                "aichat: ShareAccept per '{share_id}' ricevuto ma nessuna condivisione in uscita corrispondente (stale?)"
            );
            return Vec::new();
        };
        entry.stage = OutgoingShareStage::AwaitingContent;
        let target_label = entry.target_label.clone();
        let doc_name = entry.doc_name.clone();
        let rel_path = entry.rel_path.clone();

        let mut effects = vec![Effect::ToUi(ServerMsg::ShareResult {
            share_id: share_id.clone(),
            target_label,
            doc_name,
            outcome: protocol::ShareOutcome::Accepted,
        })];
        if self.has_ui() {
            effects.push(Effect::ToUi(ServerMsg::ShareContentRequest { share_id, rel_path }));
        }
        effects
    }

    /// (Mittente, Slice 2a) La UI ha letto con successo il documento richiesto —
    /// rivalida la dimensione (può essere cresciuta dall'offerta iniziale: difesa in
    /// profondità, stessa filosofia di `request_share`), poi manda `ChatMsg::ShareData`
    /// al peer risolto FRESCO (non un `PeerId` cacheato dall'Accept — potrebbe essersi
    /// riconnesso nel frattempo), applicando lo stesso filtro di raggiungibilità TCP di
    /// `request_share` (topologia a stella). Se la dimensione supera il cap, stesso
    /// trattamento di `ShareContentFailed` — nessun `ShareResult` aggiuntivo se il peer
    /// non è più raggiungibile (il mittente ha già visto "accettata", spec §3).
    fn handle_share_content(&mut self, share_id: String, title: String, content: String) -> Vec<Effect> {
        let Some(entry) = self.outgoing_shares.get(&share_id) else {
            tracing::debug!(
                "aichat: ShareContent per '{share_id}' ricevuto ma nessuna condivisione in uscita corrispondente (stale?)"
            );
            return Vec::new();
        };
        if entry.stage != OutgoingShareStage::AwaitingContent {
            tracing::debug!(
                "aichat: ShareContent per '{share_id}' ricevuto ma l'entry non è AwaitingContent (stale?)"
            );
            return Vec::new();
        }
        if content.len() as u64 > MAX_SHARE_SIZE_BYTES {
            return self.resolve_outgoing_share(
                &share_id,
                protocol::ShareOutcome::Failed {
                    reason: format!(
                        "documento troppo grande: {} byte, massimo {MAX_SHARE_SIZE_BYTES}",
                        content.len()
                    ),
                },
            );
        }
        let target_label = entry.target_label.clone();
        self.outgoing_shares.remove(&share_id);
        let reachable = crate::aichat::relay::route_to_label(&target_label, &self.peers)
            .filter(|target_id| self.links.contains_key(target_id));
        match reachable {
            Some(target_id) => {
                vec![Effect::SendToPeer(target_id, ChatMsg::ShareData { share_id, title, content })]
            }
            None => Vec::new(),
        }
    }

    /// (Destinatario) L'umano ha deciso su un'offerta pendente. No-op se `share_id`
    /// non è (più) in `pending_shares` come `AwaitingDecision` (stale: già scaduta, già
    /// decisa, o mai esistita — `AwaitingContent`/`AwaitingUiWrite` non dovrebbero mai
    /// ricevere un nuovo `ShareConsent`: la UI non mostra più il banner dopo la
    /// transizione).
    ///
    /// Fix di integrità del consenso (review finale Slice 2a): sull'accettazione
    /// l'entry transiziona esplicitamente ad `AwaitingContent` — NON resta
    /// `AwaitingDecision` invariata. Questo è ciò che impedisce a un `ChatMsg::ShareData`
    /// prematuro (mandato prima che l'umano decida) di essere trattato come se fosse
    /// stato accettato: `handle_share_data` richiede `AwaitingContent`, quindi un
    /// `ShareData` che arriva mentre l'entry è ancora `AwaitingDecision` è ignorato
    /// (spec §8, "niente contenuto prima del consenso"). Solo il rifiuto rimuove subito.
    fn share_consent(&mut self, share_id: String, accept: bool) -> Vec<Effect> {
        let (from_label, doc_name, size_bytes) = match self.pending_shares.get(&share_id) {
            Some(PendingShare::AwaitingDecision { from_label, doc_name, size_bytes }) => {
                (from_label.clone(), doc_name.clone(), *size_bytes)
            }
            _ => {
                tracing::debug!("aichat: ShareConsent per '{share_id}' ricevuto ma nessuna offerta AwaitingDecision pendente (stale?)");
                return Vec::new();
            }
        };
        let msg = if accept {
            self.pending_shares.insert(
                share_id.clone(),
                PendingShare::AwaitingContent { from_label: from_label.clone(), doc_name, size_bytes },
            );
            ChatMsg::ShareAccept { share_id, from_label: self.me.label_base.clone() }
        } else {
            self.pending_shares.remove(&share_id);
            ChatMsg::ShareReject { share_id, from_label: self.me.label_base.clone() }
        };
        match crate::aichat::relay::route_to_label(&from_label, &self.peers) {
            Some(sender_id) => vec![Effect::SendToPeer(sender_id, msg)],
            None => {
                // Il mittente originale non è (più) raggiungibile — la nostra decisione
                // non ha nessuno a cui essere comunicata. Non un errore: coerente col
                // trattamento "best-effort" già riservato altrove a un peer sparito.
                Vec::new()
            }
        }
    }

    /// (Mittente) Un `share_id` in `outgoing_shares` si è risolto — riporta l'esito alla
    /// UI e rimuove l'entry. Condiviso da `ShareReject`/`ShareExpired`,
    /// `ServiceEvent::ShareContentFailedUi` (la UI non è riuscita a leggere il documento
    /// richiesto), e dal ramo "oltre il cap" di `handle_share_content` (Slice 2a).
    /// `ChatMsg::ShareAccept` NON passa più da qui (Slice 2a): usa `handle_share_accepted`,
    /// che riporta `Accepted` ma transiziona ad `AwaitingContent` invece di rimuovere.
    fn resolve_outgoing_share(&mut self, share_id: &str, outcome: protocol::ShareOutcome) -> Vec<Effect> {
        match self.outgoing_shares.remove(share_id) {
            Some(OutgoingShare { target_label, doc_name, .. }) => vec![Effect::ToUi(ServerMsg::ShareResult {
                share_id: share_id.to_string(),
                target_label,
                doc_name,
                outcome,
            })],
            None => {
                tracing::debug!("aichat: esito Share per '{share_id}' ricevuto ma nessuna condivisione in uscita corrispondente (stale?)");
                Vec::new()
            }
        }
    }

    /// Genera il prossimo `share_id` univoco: `"{ip-locale}:{contatore}"`. Prefissato
    /// dal proprio IP per restare univoco anche fra macchine diverse (ogni macchina
    /// genera `share_id` solo per le condivisioni che INIZIA — mai per quelle in
    /// arrivo, che portano già il `share_id` scelto dal mittente).
    fn fresh_share_id(&mut self) -> String {
        let id = format!("{}:{}", self.me.id.0, self.next_share_id);
        self.next_share_id += 1;
        id
    }

    /// Etichette (`"<base>-human"`) di tutti i peer attualmente noti
    /// (`self.peers`, popolata da `Discovered`) — la lista dei "presenti" da
    /// mostrare al nuovo arrivato nel gate 1/nello stato di attesa. Estratta
    /// perché ricalcolata in tre punti (`begin_join_gate`/`request_admission`
    /// via i due chiamanti sopra): un solo posto che decide COSA conta come
    /// "presente" agli occhi del candidato.
    fn known_peer_labels(&self) -> Vec<String> {
        self.peers.values().map(|p| format!("{}-human", p.label_base)).collect()
    }

    /// Etichette (`"<base>-human"`) dei peer scoperti E con un link TCP vivo
    /// (`self.peers ∩ self.links`) — esattamente il filtro già usato da
    /// `request_share`/`request_share` (sezione Share più sotto in questo
    /// file). A differenza di `known_peer_labels()` (usato dal gate 1 chat,
    /// che non filtra su `links`), questa lista non deve MAI contenere una
    /// macchina che poi fallirebbe una condivisione con "non trovata o non
    /// connessa" — rispecchia lo stato REALE di raggiungibilità, non la
    /// semplice scoperta UDP. Nessuna esclusione di `self.me`: `self.peers`
    /// non contiene mai la propria identità per costruzione (la scoperta UDP
    /// non annuncia se stessi a se stessi).
    fn reachable_peer_labels(&self) -> Vec<String> {
        self.peers.values()
            .filter(|p| self.links.contains_key(&p.id))
            .map(|p| format!("{}-human", p.label_base))
            .collect()
    }

    /// Genera il prossimo `note_id` univoco: stesso schema di `fresh_share_id`
    /// (design §3 — nessuna dipendenza `uuid` nel codebase).
    fn next_note_id(&mut self) -> String {
        let id = format!("{}:{}", self.me.id.0, self.next_note_id);
        self.next_note_id += 1;
        id
    }

    /// `Note` interno → `NoteView` per la UI (corpo già renderizzato).
    ///
    /// `&self` (non più funzione associata): serve `self.me.label_base` per
    /// isolare il segmento di QUESTA macchina — senza, la finestra "Modifica"
    /// non aveva modo di sapere cosa precompilare (trovato nello smoke test
    /// dal vivo del 2026-07-31, v. design §10 sul precompilamento previsto).
    fn to_note_view(&self, note: &Note) -> protocol::NoteView {
        let my_segment_text = note
            .segments
            .iter()
            .find(|s| s.machine == self.me.label_base)
            .map(|s| s.text.clone())
            .unwrap_or_default();
        protocol::NoteView {
            id: note.id.clone(),
            title: note.title.clone(),
            body: render_body(note),
            my_segment_text,
            created_by: note.created_by.clone(),
            created_at_ms: note.created_at_ms,
            deleted: note.deleted,
        }
    }

    /// Manda `ChatMsg::NoteUpdated { note }` a OGNI peer attualmente linkato
    /// (design §5.1): se siamo client, è l'unico link (verso il server); se
    /// siamo server, questo È già il fan-out verso tutti i client.
    ///
    /// Usata SOLO dai quattro arm LOCALI (`NoteCreate/Edit/EditTitle/Delete
    /// Requested`), dove il broadcast è sempre dovuto: è l'utente di QUESTA
    /// macchina che ha appena agito, e le altre macchine devono saperlo anche
    /// quando lo stato locale non cambia (es. ri-cancellare una nota già
    /// tombstoned qui ma ancora viva altrove). Il percorso REMOTO ha invece il
    /// suo gemello gatato: `apply_incoming_note` qui sotto.
    fn broadcast_note_to_links(&self, note: &Note) -> Vec<Effect> {
        self.links.keys().map(|id| Effect::SendToPeer(*id, ChatMsg::NoteUpdated { note: note.clone() })).collect()
    }

    /// FIX 5 (review finale di branch) — coda comune di OGNI nota che arriva
    /// dalla RETE: merge nello store, poi (e SOLO se qualcosa è davvero
    /// cambiato) push alla UI, salvataggio su disco e fan-out agli altri link.
    ///
    /// Il gate "solo se cambiata" è il design §5.2 punto 4: si ri-broadcasta una
    /// nota solo quando "è CAMBIATA rispetto a quanto aveva prima". Senza il
    /// gate, ricevere due volte la stessa identica nota (ritrasmissione, giro di
    /// riconciliazione periodica, rimbalzo da un terzo peer) rimbalzava di nuovo
    /// in rete — traffico che si auto-alimenta — oltre a un `SaveNotes` e a un
    /// ri-render della Library inutili.
    ///
    /// Il confronto è il `PartialEq` derivato di `Note` (uguaglianza strutturale
    /// campo per campo): `before == None` significa "nota mai vista prima",
    /// quindi genuinamente nuova e sempre da propagare.
    ///
    /// Parametri:
    /// - `from`: il mittente, ESCLUSO dal fan-out (mai rimandare indietro a chi
    ///   ce l'ha appena data).
    /// - `fan_out`: `false` per `ChatMsg::NotesData`, dove il mittente ha già
    ///   fatto il proprio fan-out (v. il commento esteso in quell'arm: un
    ///   secondo fan-out qui produrrebbe un doppio invio via due percorsi).
    ///
    /// Analogia OOP: è un *template method* del percorso "nota in arrivo" — i
    /// tre arm remoti condividono la stessa coda e variano solo su `fan_out`.
    fn apply_incoming_note(&mut self, incoming: Note, from: PeerId, fan_out: bool) -> Vec<Effect> {
        let before = self.notes.get(&incoming.id).cloned();
        let merged = self.notes.upsert_merged(incoming);
        if before.as_ref() == Some(&merged) {
            return Vec::new(); // merge no-op: nulla da mostrare, salvare o propagare
        }
        let mut effects = vec![
            Effect::ToUi(ServerMsg::NoteUpserted { note: self.to_note_view(&merged) }),
            Effect::SaveNotes,
        ];
        if fan_out {
            for peer_id in self.links.keys() {
                if *peer_id != from {
                    effects.push(Effect::SendToPeer(*peer_id, ChatMsg::NoteUpdated { note: merged.clone() }));
                }
            }
        }
        effects
    }

    /// Etichetta leggibile per un `PeerId` noto: la propria (`self.me`) o quella di un peer
    /// scoperto (`self.peers`). Usato solo per i log di elezione (§6 dello spec storico);
    /// fallback sul solo IP se per qualche motivo non abbiamo ancora l'info del peer.
    fn label_for(&self, id: PeerId) -> String {
        if id == self.me.id {
            format!("{} ({})", self.me.label_base, self.me.id.0)
        } else {
            match self.peers.get(&id) {
                Some(info) => format!("{} ({})", info.label_base, info.id.0),
                None => format!("{}", id.0),
            }
        }
    }

    /// Calcola il ruolo del cluster (Server / Client / Undecided) e ritorna gli effetti
    /// di connessione da eseguire.
    ///
    /// ## Come funziona
    ///
    /// 1. Costruisce un `Roster` con `me` + **tutti** i peer scoperti via UDP
    ///    (`self.peers`, popolata da `Discovered`). Task 8 (ammissione alla stanza,
    ///    `Docs/superpowers/plans/2026-07-03-aichat-admission.md`): l'elezione non
    ///    dipende più da un consenso pairwise pre-connessione — chiunque sia
    ///    scoperto sulla LAN partecipa al calcolo di chi è il server. Il vero gate
    ///    che decide CHI entra davvero nella stanza è il voto di ammissione (Task
    ///    4/5), applicato DOPO la connessione TCP, non prima dell'elezione.
    /// 2. Chiama `channel.decide_role(&roster)` che aggiorna il ruolo interno (sticky:
    ///    il server corrente non viene prelato se è ancora nel roster).
    /// 3. Traduce il ruolo in effetti:
    ///    - `Server` → `StartListener(me.chat_port)`: il loop deve aprire il TcpListener.
    ///    - `Client(server)` → `ConnectTo(info_server)`: il loop deve aprire il TcpStream.
    ///    - `Undecided` → nessun effetto (roster con meno di 2 membri o vuoto).
    ///
    /// ## Idempotenza
    ///
    /// Questa funzione ritorna l'effetto **ogni volta che viene chiamata**, anche se il
    /// listener/connessione è già attivo. La vera de-duplicazione (non ri-bindare se già
    /// in ascolto, non ri-connettersi se già connessi) è compito del loop `run` (Task 8):
    /// consulterà `connected` e un flag `listening: bool` per decidere se l'effetto è
    /// effettivamente necessario. Questo mantiene `handle_event` pura (nessun stato I/O).
    fn decide_and_connect(&mut self) -> Vec<Effect> {
        // Costruisce il roster: me + TUTTI i peer scoperti (vedi il doc-comment
        // sopra). `me` è sempre incluso: l'elezione funziona solo se considera
        // tutti i nodi, incluso noi stessi (il discoverer UDP filtra i propri
        // annunci, quindi i peer ricevuti non ci contengono).
        let mut roster = crate::aichat::peer::Roster::new();
        roster.upsert(self.me.clone());
        for info in self.peers.values() {
            roster.upsert(info.clone());
        }

        // Ricalcola il ruolo (aggiorna `channel.role` internamente). `self.reported_leader`
        // (Bug 2, late-joiner) impedisce l'autoelezione quando un peer scoperto ha già
        // riportato un leader esistente altrove — vedi `election::elect`.
        let role = self.channel.decide_role(&roster, self.reported_leader);

        // Log elezione/cambio server — una volta sola per cambio effettivo. Gira su OGNI
        // peer (ognuno calcola la propria vista con `elect()`), quindi compare nel log di
        // tutti gli orchestrator della stanza (§6 dello spec storico).
        let current_leader = match role {
            Role::Server => Some(self.me.id),
            Role::Client(server) => Some(server),
            Role::Undecided => None,
        };

        // §3.6 dello spec late-joiner: pubblica il leader attuale sull'handle condiviso,
        // letto dal task di scoperta UDP a ogni `announce()`. Non è I/O reale (nessun
        // socket/file/await): stesso livello di "purezza pragmatica" già tollerato per
        // `tracing::info!` qui sotto, nella stessa funzione.
        *self.believed_leader.lock().unwrap() = current_leader;

        if let Some(leader) = current_leader {
            if self.last_logged_leader != Some(leader) {
                match self.last_logged_leader {
                    None => tracing::info!("aichat: server eletto: {}", self.label_for(leader)),
                    Some(old) => tracing::info!(
                        "aichat: server {} non più raggiungibile — nuovo server eletto: {}",
                        self.label_for(old),
                        self.label_for(leader)
                    ),
                }
                self.last_logged_leader = Some(leader);
            }
        }

        match role {
            // Sono il server: assicura il TcpListener attivo E registra la PROPRIA etichetta
            // umana nel Room. Il server è anch'esso un partecipante: i client si registrano col
            // loro `Join`, il server no → senza questo, il proprio "<base>-human" mancherebbe dal
            // roster broadcastato. `on_server_join` è idempotente; lo snapshot viene diffuso al
            // prossimo Join di un client.
            Role::Server => {
                let _ = self
                    .channel
                    .on_server_join(format!("{}-human", self.me.label_base));
                vec![Effect::StartListener(self.me.chat_port)]
            }

            // Sono un client: il loop deve connettersi al server eletto.
            Role::Client(server) => {
                // Recupera le info del server per avere l'indirizzo TCP.
                // Se per qualche ragione non abbiamo l'info (anomalia), nessun effetto.
                match self.peers.get(&server) {
                    Some(info) => vec![Effect::ConnectTo(info.clone())],
                    None => Vec::new(),
                }
            }

            // Roster con meno di 2 nodi (solo me): nessuna connessione da fare.
            Role::Undecided => Vec::new(),
        }
    }

    // =========================================================================
    // Task 8 — loop attore + esecuzione effetti (I/O reale)
    // =========================================================================

    /// Il loop principale dell'attore. Consuma `self` per garantire l'ownership esclusiva.
    ///
    /// Riceve tre argomenti:
    /// - `inbox_rx`: il ricevitore del canale pubblico (ws.rs, discovery, reader per-peer).
    /// - `inbox_tx`: il sender clone, passato ai task I/O così possono re-iniettare eventi
    ///   (`PeerMsg`, `PeerGone`) nell'inbox senza condividere stato.
    /// - `shutdown`: token di cancellazione condiviso col resto del processo (stesso token
    ///   usato da `ws::serve` e dal comando "q"+invio in `main.rs`). Debito #2 ("teardown
    ///   reale", Slice B): senza questo, l'attore non aveva NESSUN modo pulito di fermarsi —
    ///   vedi il commento sul ramo 4 del `select!` sotto.
    ///
    /// ## Pattern: select! su cinque sorgenti
    ///
    /// Il loop usa `tokio::select!` per ascoltare contemporaneamente:
    /// 1. `inbox_rx` — eventi pubblici (ws.rs, discovery, reader per-peer).
    /// 2. `new_link_rx` — socket grezzi appena accettati dall'accept loop (Slice C2:
    ///    QUESTO ramo registra lo stato via `register_link` E spawna
    ///    `spawn_peer_tasks` — l'accept loop stesso non fa più né l'uno né l'altro).
    /// 3. `ping_timer` — il timer keepalive: ogni `PING_INTERVAL` inietta un
    ///    `ServiceEvent::Tick`, che fa emettere un `Ping` a ogni peer connesso.
    /// 4. `connect_res_rx` — esiti dei task di connessione non bloccanti (Debito #3,
    ///    Slice C). Slice C2: su `Success` questo ramo registra lo stato via
    ///    `register_connect_success` E spawna `spawn_peer_tasks` (stesso principio del
    ///    ramo 2 — il task di connessione stesso non fa più né l'uno né l'altro); su
    ///    `Failure` chiama `register_connect_failure`.
    /// 5. `shutdown.cancelled()` — segnale di spegnimento globale (vedi sotto).
    ///
    /// Questo evita un `Arc<Mutex<links>>` condiviso tra il loop e l'accept loop:
    /// l'accept loop non modifica `self.links` direttamente, ma manda un messaggio
    /// al loop principale via `new_link_tx`.
    ///
    /// ## Perché un `CancellationToken` e non un `JoinSet`
    ///
    /// L'accept loop (spawnato in `perform` per `Effect::StartListener`) spawna a sua
    /// volta i task reader/writer per-peer (`spawn_peer_tasks`) — sono "figli dei figli"
    /// rispetto a `run`. Un `tokio::task::JoinSet` posseduto da `run` raggiungerebbe SOLO
    /// i task spawnati direttamente qui dentro (l'accept loop): `abort_all()` non
    /// propagherebbe alla gerarchia sottostante. Un `CancellationToken` clonato "verso il
    /// basso" (`run` → accept loop → `spawn_peer_tasks` → reader/writer) risolve questo
    /// naturalmente: ogni livello osserva la stessa cancellazione, qualunque sia la sua
    /// profondità nell'albero dei task.
    pub async fn run(
        mut self,
        mut inbox_rx: UnboundedReceiver<ServiceEvent>,
        inbox_tx: UnboundedSender<ServiceEvent>,
        shutdown: tokio_util::sync::CancellationToken,
    ) {
        // Canale interno: l'accept loop notifica il loop run di ogni nuova connessione.
        // Tipo: (PeerId del peer appena accettato, il `TcpStream` GREZZO).
        // Non è in `ServiceEvent` per non inquinare il protocollo pubblico con dettagli I/O.
        //
        // Slice C2 (fix "register-before-spawn"): prima di questa slice il tipo portava
        // già il writer_tx E la generazione (`(PeerId, UnboundedSender<ChatMsg>, u64)`),
        // perché l'accept loop chiamava `spawn_peer_tasks` su se stesso PRIMA di notificare
        // l'attore. Questo è esattamente ciò che apriva la race: il reader poteva morire ed
        // emettere un `PeerGone` prima che l'attore scrivesse `link_gen`. Ora l'accept loop
        // manda SOLO il socket grezzo: è l'attore (2° ramo di `select!` sotto) ad allocare
        // la generazione, scrivere `link_gen`/`connected`, e SOLO DOPO spawnare il reader/
        // writer — vedi `register_link`.
        let (new_link_tx, mut new_link_rx) =
            tokio::sync::mpsc::unbounded_channel::<(PeerId, TcpStream)>();

        // Canale interno: i task di connessione (spawnati in `perform` per
        // `Effect::ConnectTo`, Debito #3) notificano il loro esito senza mai bloccare
        // l'attore su un `TcpStream::connect(..).await`. Vedi `ConnectOutcome`.
        let (connect_res_tx, mut connect_res_rx) =
            tokio::sync::mpsc::unbounded_channel::<ConnectOutcome>();

        // Timer keepalive: ogni PING_INTERVAL, il terzo ramo del select! sotto inietta
        // ServiceEvent::Tick nello stesso loop degli altri eventi — nessun task separato,
        // nessuno stato condiviso aggiuntivo (a differenza di `believed_leader`, che
        // ATTRAVERSA due task distinti — qui il timer vive già dentro `run`).
        let mut ping_timer = tokio::time::interval(PING_INTERVAL);

        loop {
            tokio::select! {
                // Ramo 1: eventi pubblici dall'inbox.
                maybe_ev = inbox_rx.recv() => {
                    let Some(ev) = maybe_ev else {
                        // NB: `run` POSSIEDE `inbox_tx` (e `new_link_tx` qui sotto), quindi
                        // esiste sempre almeno un sender vivo → `recv()` non ritorna mai `None`
                        // e questo `break` resta di fatto IRRAGGIUNGIBILE anche dopo il fix del
                        // debito #2: l'unico `break` realmente raggiungibile è il ramo 4 qui
                        // sotto (`shutdown.cancelled()`).
                        break;
                    };
                    // `handle_event` è PURA (nessun I/O); ritorna la lista degli effetti.
                    let effects = self.handle_event(ev);
                    // Esegui ogni effetto nell'ordine in cui è stato prodotto.
                    for eff in effects {
                        self.perform(eff, &inbox_tx, &new_link_tx, &connect_res_tx, &shutdown).await;
                    }
                }

                // Ramo 2: un nuovo socket è stato accettato dall'accept loop.
                // Stesso discorso del ramo 1: `new_link_tx` è posseduto da `run` (tramite
                // il clone passato a `perform`), quindi anche qui `None` è irraggiungibile
                // finché il loop non esce dal ramo 5.
                //
                // Slice C2 (fix "register-before-spawn"): a differenza di prima, QUI
                // avviene sia l'allocazione della generazione sia lo spawn di
                // `spawn_peer_tasks` — nell'attore, con `&mut self`. `register_link` scrive
                // `link_gen`/`connected` PRIMA della riga di spawn: non c'è alcun `.await`
                // fra le due, quindi non esiste una finestra in cui il reader appena
                // spawnato possa emettere un `PeerGone` per una generazione non ancora
                // registrata — la race che questa slice chiude. Vedi il doc-comment di
                // `register_link` per il dettaglio.
                maybe_link = new_link_rx.recv() => {
                    let Some((peer_id, stream)) = maybe_link else { break };
                    let gen = self.next_gen;
                    self.next_gen += 1;
                    self.register_link(peer_id, gen);
                    let writer_tx = spawn_peer_tasks(stream, peer_id, inbox_tx.clone(), gen, shutdown.clone());
                    self.links.insert(peer_id, writer_tx);
                    // Library "Condividi" (spec 2026-07-28): nuovo link stabilito,
                    // notifica il nuovo snapshot di raggiungibilità.
                    self.perform(
                        Effect::ToUi(ServerMsg::AiChatReachablePeers { labels: self.reachable_peer_labels() }),
                        &inbox_tx, &new_link_tx, &connect_res_tx, &shutdown,
                    ).await;
                    // Blocco note (design §5.2): link appena stabilito → programma lo scambio
                    // di digest dopo il debounce fisso.
                    self.perform(
                        Effect::StartNotesReconcileTimer { peer_id, generation: gen, secs: RECONCILE_DEBOUNCE_SECS },
                        &inbox_tx, &new_link_tx, &connect_res_tx, &shutdown,
                    ).await;
                }

                // Ramo 3: timer keepalive. Ad ogni tick, genera un ServiceEvent::Tick
                // esattamente come se fosse arrivato dall'inbox — stesso trattamento
                // (handle_event puro → esegui gli effetti).
                _ = ping_timer.tick() => {
                    let effects = self.handle_event(ServiceEvent::Tick);
                    for eff in effects {
                        self.perform(eff, &inbox_tx, &new_link_tx, &connect_res_tx, &shutdown).await;
                    }
                }

                // Ramo 4 (debito #3, Slice C): esito di un task di connessione non
                // bloccante. Stesso discorso di irraggiungibilità di `None` degli altri
                // rami: `connect_res_tx` è posseduto da `run` (tramite il clone passato a
                // `perform`).
                //
                // Slice C2 (fix "register-before-spawn"): `Success` ora porta il
                // `TcpStream` grezzo (non più un `writer_tx` già pronto). Esattamente come
                // nel ramo 2, la registrazione di stato (`register_connect_success`, che
                // scrive `link_gen`/`connected` e libera `connecting`) avviene PRIMA di
                // `spawn_peer_tasks` — la stessa logica di `register_connect_success` è
                // testata direttamente nei test `connect_success_*`/`register_connect_*`,
                // senza bisogno di un vero socket (vedi quei test).
                maybe_outcome = connect_res_rx.recv() => {
                    let Some(outcome) = maybe_outcome else { break };
                    match outcome {
                        ConnectOutcome::Success { info, stream, gen } => {
                            let id = info.id;
                            self.register_connect_success(id, gen);
                            let writer_tx = spawn_peer_tasks(stream, id, inbox_tx.clone(), gen, shutdown.clone());
                            self.links.insert(id, writer_tx);
                            // Library "Condividi" (spec 2026-07-28): nuovo link stabilito,
                            // notifica il nuovo snapshot di raggiungibilità.
                            self.perform(
                                Effect::ToUi(ServerMsg::AiChatReachablePeers { labels: self.reachable_peer_labels() }),
                                &inbox_tx, &new_link_tx, &connect_res_tx, &shutdown,
                            ).await;
                            // Blocco note (design §5.2): link appena stabilito → programma lo scambio
                            // di digest dopo il debounce fisso.
                            self.perform(
                                Effect::StartNotesReconcileTimer { peer_id: id, generation: gen, secs: RECONCILE_DEBOUNCE_SECS },
                                &inbox_tx, &new_link_tx, &connect_res_tx, &shutdown,
                            ).await;
                            // Task 6: al posto del vecchio `ChatMsg::Join` (inviato subito,
                            // senza alcun gate) presentiamo ora il gate 1 alla nostra UI —
                            // `begin_join_gate` decide se mostrarlo (guardia `NotJoining`) e
                            // ritorna l'unico effetto da eseguire (`ToUi(AiChatJoinPrompt)`).
                            // Il `Join` non esiste più: il candidato si presenta al server
                            // con `ChatMsg::RequestAdmission` solo DOPO che l'umano ha
                            // accettato il gate 1 (arm `ServiceEvent::JoinDecision`, in
                            // `handle_event`).
                            let effects = self.begin_join_gate();
                            for eff in effects {
                                self.perform(eff, &inbox_tx, &new_link_tx, &connect_res_tx, &shutdown).await;
                            }
                        }
                        ConnectOutcome::Failure { id } => {
                            self.register_connect_failure(id);
                        }
                    }
                }

                // Ramo 5 (debito #2, Slice B): segnale di shutdown globale.
                // Questo È l'UNICO `break` realmente raggiungibile del loop: chiude
                // l'attore su richiesta esplicita (comando "q"+invio in main.rs, o
                // qualunque altro spegnimento ordinato che condivida lo stesso token).
                // `shutdown` è stato passato per riferimento a `perform` sopra, che a sua
                // volta lo clona verso i task figli (accept loop, task di connessione,
                // reader/writer per-peer): uscendo da qui il loop dell'attore termina, e
                // la stessa cancellazione raggiunge — asincronamente — anche l'intera
                // gerarchia di task spawnati.
                _ = shutdown.cancelled() => {
                    break;
                }
            }
        }
    }

    /// Esegue un singolo `Effect` producendo I/O reale (socket, spawn di task).
    ///
    /// È il "driver" degli effetti: mentre `handle_event` è pura (testabile),
    /// `perform` ha accesso ai socket e ai task asincroni di Tokio.
    ///
    /// `inbox_tx` e `new_link_tx` sono passati ai task I/O spawned qui dentro,
    /// così possono re-iniettare eventi nell'attore senza condividere stato.
    ///
    /// `connect_res_tx` (Debito #3, Slice C) è il canale interno su cui il task di
    /// connessione spawnato per `Effect::ConnectTo` manda il proprio esito
    /// (`ConnectOutcome::Success`/`Failure`) — vedi il doc-comment su `ConnectOutcome`.
    ///
    /// `shutdown` (debito #2, Slice B) è il token di cancellazione condiviso. Slice C2:
    /// né il task di connessione né l'accept loop chiamano più `spawn_peer_tasks` (quindi
    /// non hanno più bisogno di un proprio clone del token per quello) — lo spawn, e il
    /// relativo clone di `shutdown`, avvengono ora nell'attore (rami 2 e 4 di `run`).
    async fn perform(
        &mut self,
        eff: Effect,
        inbox_tx: &UnboundedSender<ServiceEvent>,
        new_link_tx: &UnboundedSender<(PeerId, TcpStream)>,
        connect_res_tx: &UnboundedSender<ConnectOutcome>,
        shutdown: &tokio_util::sync::CancellationToken,
    ) {
        match eff {
            // Spingi un messaggio alla UI connessa (se presente).
            // Se la UI è chiusa (server_tx == None), il messaggio viene silenziosamente ignorato.
            Effect::ToUi(sm) => {
                if let Some(tx) = &self.server_tx {
                    // `send` su un `UnboundedSender` non blocca mai; se il ricevitore è chiuso,
                    // l'errore viene ignorato (la UI non è più disponibile, non è un errore grave).
                    let _ = tx.send(sm);
                }
            }

            // Invia un `ChatMsg` a un peer via il suo writer task (mpsc → TCP).
            Effect::SendToPeer(peer_id, msg) => {
                if let Some(tx) = self.links.get(&peer_id) {
                    let _ = tx.send(msg);
                } else {
                    // Il peer non ha un link attivo: log e ignora (può accadere se il peer
                    // è appena disconnesso mentre un effetto era in coda).
                    tracing::debug!("aichat: SendToPeer a peer senza link: {:?}", peer_id);
                }
            }

            // (Client) Apri una connessione TCP verso il server eletto.
            //
            // Guard di de-dup: `decide_and_connect` (PURA) non sa se il link è già vivo, né
            // se un connect è già in volo, e viene chiamata ad ogni `Discovered` (ogni ~5s
            // dal discoverer UDP). Senza questo guard, il client aprirebbe un nuovo
            // `TcpStream` ad ogni annuncio del server, sovrascrivendo il link attivo o
            // avviando connect concorrenti verso lo stesso peer.
            //
            // Debito #3 (Slice C, "head-of-line blocking"): il connect vero e proprio NON
            // avviene più qui, inline (`TcpStream::connect(..).await` dentro `perform`
            // bloccherebbe l'INTERO attore per il timeout OS — ~21s su Windows — se il peer
            // è irraggiungibile). Questo arm fa SOLO la parte sincrona: verifica i guard,
            // marca il peer come `connecting`, e SPAWNA un task indipendente che farà il
            // connect vero (con un timeout esplicito di 5s) e riporterà l'esito su
            // `connect_res_tx` — vedi `ConnectOutcome`. `perform` ritorna subito, senza mai
            // attendere il socket: l'attore resta reattivo.
            //
            // Slice C2: il task spawnato sotto NON chiama più `spawn_peer_tasks` su se
            // stesso — manda indietro il `TcpStream` grezzo. Lo spawn avviene nel 5° ramo
            // di `select!` in `run`, DOPO la registrazione dello stato (fix
            // "register-before-spawn", vedi il doc-comment di `ConnectOutcome`).
            //
            // Usa `info.id.0` (l'`Ipv4Addr` estratto dal PeerId) come indirizzo di destinazione:
            // questo è corretto e sicuro su Windows (non usiamo `0.0.0.0` come destinazione).
            Effect::ConnectTo(info) => {
                // Guard: già connessi, o un connect verso questo peer è già in volo.
                if self.connected.contains(&info.id) || self.connecting.contains(&info.id) {
                    return;
                }
                self.connecting.insert(info.id);
                // Genera ORA la generazione del link (debito #4): non importa che sia
                // allocata qui (prima del connect) o nel ramo di successo — quello che
                // conta per il fix Slice C2 è che la SCRITTURA in `self.link_gen` avvenga
                // nell'attore, prima dello spawn (vedi `register_connect_success`).
                let gen = self.next_gen;
                self.next_gen += 1;

                // Unico canale clonato per il task di connessione, che vive
                // indipendentemente da `perform` (spawnato, MAI `.await`-ato qui: questo è
                // precisamente ciò che rende il connect non bloccante per l'attore). Non
                // servono più cloni di `inbox_tx`/`shutdown`: questo task non spawna più
                // nulla, si limita a riportare l'esito del connect.
                let connect_res_c = connect_res_tx.clone();

                tokio::spawn(async move {
                    let addr = std::net::SocketAddr::from((info.id.0, info.chat_port));
                    // Timeout ESPLICITO di 5s: non ci affidiamo al timeout del sistema
                    // operativo (fino a ~21s su Windows per un host irraggiungibile) — un
                    // peer lento/muto non deve tenere il peer "connecting" più del dovuto.
                    match tokio::time::timeout(std::time::Duration::from_secs(5), TcpStream::connect(addr)).await {
                        Ok(Ok(stream)) => {
                            let _ = connect_res_c.send(ConnectOutcome::Success { info, stream, gen });
                        }
                        Ok(Err(e)) => {
                            tracing::warn!("aichat: ConnectTo {:?} fallita: {}", addr, e);
                            let _ = connect_res_c.send(ConnectOutcome::Failure { id: info.id });
                        }
                        Err(_elapsed) => {
                            tracing::warn!("aichat: ConnectTo {:?} timeout (5s)", addr);
                            let _ = connect_res_c.send(ConnectOutcome::Failure { id: info.id });
                        }
                    }
                });
            }

            // (Server) Assicura che il listener TCP sia in ascolto sulla porta indicata.
            //
            // Idempotente grazie al flag `listening`: se è già `true`, non ri-bindiamo.
            // `handle_event` emette `StartListener` ogni volta che ricalcola il ruolo;
            // senza questo guard, il secondo bind fallirebbe con "Address already in use".
            Effect::StartListener(port) => {
                if self.listening {
                    // Già in ascolto: il guard previene il doppio bind.
                    return;
                }
                match crate::aichat::net::bind_listener(port).await {
                    Ok(listener) => {
                        self.listening = true;
                        // Clona i canali per il task accept (ha vita propria, indipendente dal loop).
                        let inbox_tx_acc = inbox_tx.clone();
                        let new_link_tx_acc = new_link_tx.clone();
                        // Clona il token di shutdown per il task accept: usato SOLO per il
                        // proprio ramo `select!` sotto (e per il check su `ListenerStopped`
                        // in fondo) — debito #2, Slice B. Slice C2: l'accept loop non spawna
                        // più `spawn_peer_tasks` (quindi non deve più propagargli un clone
                        // del token): lo fa l'attore nel 2° ramo di `select!` in `run`, DOPO
                        // aver registrato lo stato — vedi `register_link`.
                        let shutdown_acc = shutdown.clone();
                        // Avvia l'accept loop: gira per tutta la vita del servizio (o finché
                        // non arriva la cancellazione).
                        tokio::spawn(async move {
                            loop {
                                tokio::select! {
                                    // Spegnimento richiesto: esci SENZA passare dal ramo Err
                                    // sotto (che invierebbe ListenerStopped e — in teoria —
                                    // farebbe ripartire un nuovo bind, l'opposto di quello che
                                    // vogliamo mentre il processo si sta fermando).
                                    _ = shutdown_acc.cancelled() => break,
                                    res = listener.accept() => {
                                        match res {
                                            Ok((stream, peer_addr)) => {
                                                // Ricaviamo l'IP del peer per costruire il PeerId.
                                                // NOTA loopback: su loopback `peer_addr.ip()` restituisce
                                                // sempre `127.0.0.1`, anche per connessioni da "client
                                                // simulati" con PeerId diverso. Questo è un limite del test
                                                // di loopback e non si verifica su LAN reale (dove ogni
                                                // macchina ha un IP distinto). Vedere task-8-report.md.
                                                let peer_ip = match peer_addr.ip() {
                                                    std::net::IpAddr::V4(v4) => v4,
                                                    std::net::IpAddr::V6(_) => {
                                                        tracing::warn!(
                                                            "aichat: connessione IPv6 ignorata: {}", peer_addr
                                                        );
                                                        continue;
                                                    }
                                                };
                                                let peer_id = PeerId(peer_ip);
                                                // Slice C2 (fix "register-before-spawn"): l'accept
                                                // loop NON spawna più `spawn_peer_tasks` né alloca la
                                                // generazione qui — manda il `TcpStream` GREZZO
                                                // all'attore (2° ramo di `select!` in `run`), che
                                                // scriverà `link_gen`/`connected` PRIMA di spawnare il
                                                // reader/writer. Senza questo, il reader poteva morire
                                                // ed emettere un `PeerGone` prima ancora che l'attore
                                                // sapesse che il link esisteva — la race che questa
                                                // slice chiude (vedi doc-comment di `register_link`).
                                                // Se il loop `run` è già terminato, `send` fallisce →
                                                // termina l'accept loop.
                                                if new_link_tx_acc.send((peer_id, stream)).is_err() {
                                                    break;
                                                }
                                            }
                                            Err(e) => {
                                                tracing::debug!("aichat: accept loop errore: {}", e);
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                            // Il `loop` è uscito per uno di tre motivi: errore di `accept()`,
                            // `new_link_tx_acc.send(...)` fallito perché il loop `run` è già
                            // morto, oppure cancellazione esplicita (`shutdown_acc`).
                            //
                            // Segnaliamo `ListenerStopped` SOLO nei primi due casi: è il fix
                            // del debito #5 (senza, `self.listening` resterebbe bloccato a
                            // `true` per sempre — vedi doc-comment di `ServiceEvent::
                            // ListenerStopped`). Nel terzo caso (shutdown) NON lo inviamo:
                            // l'attore sta già uscendo dal proprio loop (ramo 4 di `run`),
                            // quindi `handle_event(ListenerStopped)` non verrebbe mai eseguito
                            // comunque — e se per una race venisse letto PRIMA del ramo di
                            // shutdown, ri-emetterebbe `StartListener` (un nuovo bind) proprio
                            // mentre il processo si sta spegnendo: l'opposto del comportamento
                            // voluto. Se il loop `run` è già terminato, questo `send` fallisce
                            // comunque innocuamente — ignoriamo l'errore con `let _ =`.
                            if !shutdown_acc.is_cancelled() {
                                let _ = inbox_tx_acc.send(ServiceEvent::ListenerStopped);
                            }
                        });
                    }
                    Err(e) => {
                        tracing::warn!(
                            "aichat: StartListener porta {} fallita: {}",
                            port, e
                        );
                    }
                }
            }

            // Chiudi il link verso un peer: droppa il sender del writer task.
            //
            // Droppare il `UnboundedSender` chiude il canale mpsc:
            // il writer task si accorge che `recv()` restituisce `None` e termina.
            // Il lato lettura (reader task) termina per EOF o errore al prossimo `read_line`.
            Effect::Disconnect(peer_id) => {
                self.links.remove(&peer_id);
                self.connected.remove(&peer_id);
                // Il link è appena caduto: notifica la UI del nuovo snapshot di
                // raggiungibilità (per Library "Condividi") — calcolato DOPO le
                // rimozioni sopra, quindi già senza `peer_id`.
                if let Some(tx) = &self.server_tx {
                    let _ = tx.send(ServerMsg::AiChatReachablePeers {
                        labels: self.reachable_peer_labels(),
                    });
                }
            }

            // AI Chat Slice 1a: invoca la propria AI locale (text-only, nessun tool —
            // confine di sicurezza in `AiAdapter::chat_reply`). Spawnato come task
            // indipendente per lo STESSO motivo di `Effect::ConnectTo`: una chiamata
            // HTTP (anche non lentissima) non deve bloccare l'intero attore — `perform`
            // ritorna subito, l'attore resta reattivo a `Tick`/`PeerMsg`/altri eventi
            // nel frattempo. Il task comunica il risultato SOLO tramite `ServiceEvent`
            // (mai `&mut self` catturato): stesso pattern "nessuno stato condiviso
            // mutabile nuovo" del resto del file.
            Effect::InvokeLocalAi { request, history, my_ai_label } => {
                let ai = Arc::clone(&self.ai_adapter);
                let inbox = inbox_tx.clone();
                // Il token di shutdown copre anche questo task: se il processo si sta
                // spegnendo mentre la chiamata AI è in volo, `chat_reply` può abbreviarla
                // (rispetto "leggero" — vedi il doc-comment del metodo in `ai_adapter.rs`).
                let shutdown_ai = shutdown.clone();
                tokio::spawn(async move {
                    let text = ai.chat_reply(&my_ai_label, &history, &request, Some(shutdown_ai)).await;
                    // Guardia: una risposta vuota (es. `chat_reply` che abbrevia per
                    // cancellazione upfront, o una risposta degenere) non deve diventare
                    // una riga di chat fantasma — `publish_say` la pubblicherebbe comunque
                    // (storico + eco UI + relay) senza alcun valore per l'umano.
                    if text.trim().is_empty() {
                        return;
                    }
                    // Se l'attore è già terminato (canale chiuso), `send` fallisce
                    // innocuamente: non c'è più nessuno che possa pubblicare la risposta.
                    let _ = inbox.send(ServiceEvent::AiReply { text });
                });
            }

            // AI Chat Slice 2 (design §10.3): chiedi alla propria AI locale un giudizio
            // di rilevanza (nessuna `request` esplicita: nessun umano ha invocato nulla,
            // a differenza di `InvokeLocalAi` sopra). Stesso pattern "task spawnato,
            // comunica SOLO via ServiceEvent" del resto del file.
            //
            // ATTENZIONE (differenza deliberata da `InvokeLocalAi` sopra): qui `perform`
            // manda `AutoParticipateDone` SEMPRE, anche quando `chat_autoparticipate`
            // ritorna `None` (silenzio — l'esito più comune) o un testo vuoto. NON
            // replichiamo la guardia "if text.trim().is_empty() { return; }" di
            // `InvokeLocalAi`: qui il `None`/testo-vuoto è un ESITO NORMALE del
            // giudizio (non un caso degenere raro), e `autoparticipate_inflight` deve
            // essere azzerato per OGNI esito — se `perform` scartasse l'evento in
            // silenzio come fa per `InvokeLocalAi`, il flag resterebbe bloccato a
            // `true` per sempre dopo il primo silenzio, e l'auto-partecipazione non
            // scatterebbe mai più (vedi il doc-comment del campo `autoparticipate_inflight`
            // e dell'arm `ServiceEvent::AutoParticipateDone`, che è dove vive la logica
            // "pubblica solo se Some non vuoto" — qui in `perform` non decidiamo nulla).
            Effect::AutoParticipate { history, my_ai_label } => {
                let ai = Arc::clone(&self.ai_adapter);
                let inbox = inbox_tx.clone();
                let shutdown_ai = shutdown.clone();
                tokio::spawn(async move {
                    let reply = ai.chat_autoparticipate(&my_ai_label, &history, Some(shutdown_ai)).await;
                    let _ = inbox.send(ServiceEvent::AutoParticipateDone { reply });
                });
            }

            // Slice 2 (memoria persistente): scrittura reale, mutex-guardata (vedi il
            // doc-comment di `memory_write_lock`). Spawnato come task indipendente per lo
            // stesso motivo degli altri effetti I/O di questo blocco: non blocca l'attore.
            Effect::PersistMemory { label_base, note } => {
                let lock = Arc::clone(&self.memory_write_lock);
                // `self.config_dir` (2.0, D6): clonato PRIMA dello spawn (il
                // task `async move` non può prendere in prestito `self`) —
                // niente ri-derivazione da env/`startup.json` qui.
                let config_dir = self.config_dir.clone();
                tokio::spawn(async move {
                    let _guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    if let Err(e) = append_memory_note(&config_dir, &label_base, &note) {
                        tracing::warn!(
                            "aichat: impossibile salvare la memoria di {label_base}: {e}"
                        );
                    }
                });
            }

            // Ammissione alla stanza (Task 7): il timer del voto. `handle_event`
            // (Task 5) ha già DECISO la logica di risoluzione — "silenzio dopo
            // `secs` secondi = sì, ammetti" — nell'arm `ServiceEvent::VoteTimeout`.
            // Qui in `perform` viviamo nel layer I/O: il nostro unico compito è
            // *schedulare* quella decisione, cioè aspettare `secs` secondi e poi
            // re-iniettare l'evento sull'inbox. Stesso pattern "task spawnato,
            // comunica SOLO tramite ServiceEvent, mai `&mut self` catturato" di
            // `Effect::ConnectTo`/`Effect::InvokeLocalAi` sopra.
            //
            // `tokio::select!` con il token di shutdown copre il caso in cui il
            // processo si stia spegnendo mentre il timer è ancora in corsa: in
            // quel caso NON inviamo l'evento (non c'è più un attore `run` che lo
            // consumerebbe). Se invece il voto viene risolto PRIMA che il timer
            // scada (un veto arriva e rimuove la entry da `pending_votes`), il
            // task continua comunque a dormire fino in fondo: quando infine manda
            // `VoteTimeout`, l'arm in `handle_event` lo trova già risolto (niente
            // `PendingVote` per quel candidato) e lo tratta come no-op — vedi il
            // test `timeout_after_a_no_is_noop`. Nessuna cancellazione esplicita
            // del timer è quindi necessaria: il guard "idempotente" a valle basta.
            Effect::StartVoteTimeout { candidate, generation, secs } => {
                let inbox = inbox_tx.clone();
                let shutdown_vote = shutdown.clone();
                tokio::spawn(async move {
                    tokio::select! {
                        _ = shutdown_vote.cancelled() => {}
                        _ = tokio::time::sleep(std::time::Duration::from_secs(secs)) => {
                            // FIX #1: re-inietta la generazione del turno così che
                            // `handle_event` possa scartare un timer ormai stantio.
                            let _ = inbox.send(ServiceEvent::VoteTimeout { candidate, generation });
                        }
                    }
                });
            }
            // (FIX #5) Gemello di `StartVoteTimeout`: dorme `secs` secondi e poi
            // sblocca il candidato dal cooldown con `CooldownExpired`. Come sopra, un
            // `shutdown` interrompe il sonno senza inviare l'evento (l'attore sta
            // uscendo: nessuno leggerebbe più `CooldownExpired`).
            Effect::StartCooldownTimer { candidate, secs } => {
                let inbox = inbox_tx.clone();
                let shutdown_cd = shutdown.clone();
                tokio::spawn(async move {
                    tokio::select! {
                        _ = shutdown_cd.cancelled() => {}
                        _ = tokio::time::sleep(std::time::Duration::from_secs(secs)) => {
                            let _ = inbox.send(ServiceEvent::CooldownExpired { candidate });
                        }
                    }
                });
            }
            // Terzo gemello di `StartVoteTimeout`/`StartCooldownTimer`, stesso principio:
            // dorme `secs` secondi, poi re-inietta l'evento — `shutdown` interrompe il
            // sonno senza inviarlo se l'attore sta uscendo.
            Effect::StartShareExpiryTimer { share_id, secs } => {
                let inbox = inbox_tx.clone();
                let shutdown_share = shutdown.clone();
                tokio::spawn(async move {
                    tokio::select! {
                        _ = shutdown_share.cancelled() => {}
                        _ = tokio::time::sleep(std::time::Duration::from_secs(secs)) => {
                            let _ = inbox.send(ServiceEvent::ShareExpiryTimeout { share_id });
                        }
                    }
                });
            }
            // Task 9 (Blocco note): gemello di `StartVoteTimeout` — dorme `secs`
            // secondi (debounce fisso `RECONCILE_DEBOUNCE_SECS`) e poi re-inietta
            // `NotesReconcileDue { peer_id, generation }`. `generation` è la stessa
            // guardia già vista sopra: se il link è stato rimpiazzato o chiuso nel
            // frattempo, `handle_event` scarta il timer come stantio invece di
            // mandare un digest a un link che non è più quello per cui il timer era
            // partito. `shutdown` interrompe il sonno senza inviare l'evento se
            // l'attore sta uscendo, come per gli altri due timer gemelli sopra.
            Effect::StartNotesReconcileTimer { peer_id, generation, secs } => {
                let inbox = inbox_tx.clone();
                let shutdown_reconcile = shutdown.clone();
                tokio::spawn(async move {
                    tokio::select! {
                        _ = shutdown_reconcile.cancelled() => {}
                        _ = tokio::time::sleep(std::time::Duration::from_secs(secs)) => {
                            let _ = inbox.send(ServiceEvent::NotesReconcileDue { peer_id, generation });
                        }
                    }
                });
            }

            // Task 7 (Blocco note), FIX DI REVIEW: gemello vero di `PersistMemory` sopra
            // (non solo a parole, come nella prima versione di questo arm). `perform`
            // gira INLINE dentro il `select!` principale di `run` — lo stesso loop che
            // gestisce keepalive/elezione/relay per QUESTO attore — quindi uno
            // `std::fs::write` sincrono qui dentro bloccherebbe l'intero attore per la
            // durata dello scrivi-su-disco (antivirus/indicizzazione su Windows possono
            // renderlo lento in modo realistico). La sezione sincrona resta minuscola:
            // clonare `self.notes` (un `Vec<Note>` di scala personale, economico) — il
            // vero I/O (`snapshot.save()`) si sposta in un task spawnato, esattamente
            // come `PersistMemory` fa per `memory-{label_base}.md`. Il lock
            // (`notes_write_lock`, gemello di `memory_write_lock`) previene che due
            // `SaveNotes` ravvicinati interlaccino la scrittura dello stesso file.
            Effect::SaveNotes => {
                let lock = Arc::clone(&self.notes_write_lock);
                let snapshot = self.notes.clone();
                tokio::spawn(async move {
                    let _guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    if let Err(e) = snapshot.save() {
                        tracing::warn!("aichat: impossibile salvare notes.json: {e}");
                    }
                });
            }
        }
    }

    // =========================================================================
    // Slice C2 — "register-before-spawn": registrazione di stato dei link, SEMPRE
    // eseguita nell'attore (con `&mut self`) PRIMA di qualunque `spawn_peer_tasks`.
    //
    // Prima di questa slice, un unico metodo `apply_connect_outcome(&mut self,
    // ConnectOutcome)` faceva TUTTO (registrazione + spawn + invio del Join). Qui è
    // decomposto in due parti con responsabilità diverse (SRP):
    // - questi metodi: SOLO mutazione di stato, nessun I/O — testabili passando un
    //   `PeerId`/`u64` sintetici, senza fabbricare un `TcpStream` o un `writer_tx`;
    // - il chiamante (i rami 2 e 4 del `select!` in `run`, che hanno accesso al vero
    //   `TcpStream` e a `inbox_tx`/`shutdown`) chiama uno di questi metodi e SOLO DOPO
    //   spawna `spawn_peer_tasks` — mai il contrario.
    //
    // Questo ordine è precisamente il fix della race residua lasciata da Slice C
    // (0.27.0): lì `spawn_peer_tasks` veniva chiamata DENTRO il task spawnato (di
    // connessione, o l'accept loop), prima che l'attore potesse scrivere `link_gen`. Il
    // reader appena avviato poteva quindi emettere un `PeerGone(id, gen)` che la
    // guardia di generazione in `handle_event` vedeva come STALE (la mappa non aveva
    // ancora quella voce) — la registrazione arrivava poi comunque, creando un link
    // "fantasma" il cui reader era già morto. Vedi
    // `Docs/superpowers/specs/2026-07-02-aichat-hardening-debts-design.md` §"Slice C2".
    // =========================================================================

    /// Registra `link_gen`/`connected` per un link (in ingresso O in uscita) — SENZA
    /// alcun I/O. È l'unico punto che scrive in queste due mappe: sia il ramo ACCEPT
    /// (2° ramo di `select!`) sia `register_connect_success` (sotto, per il ramo
    /// CONNECT) lo chiamano PRIMA di spawnare il reader/writer.
    ///
    /// ## L'invariante che chiude la race
    ///
    /// Il chiamante esegue sempre, in ordine, senza alcun `.await` fra i due passi:
    /// 1. `self.register_link(id, gen)` (o `register_connect_success`, che lo include);
    /// 2. `spawn_peer_tasks(...)`.
    ///
    /// `tokio::spawn` non cede mai il controllo al task appena creato: la funzione
    /// ritorna subito un `JoinHandle`, il task figlio verrà schedulato ad un certo
    /// punto SUCCESSIVO. Poiché il passo 1 è sincrono e avviene PRIMA della riga di
    /// spawn (nello stesso `&mut self`, senza alcuna sospensione asincrona in mezzo),
    /// `self.link_gen[id]` esiste GIÀ nel momento in cui il reader può, nella peggiore
    /// delle ipotesi, iniziare ed eseguire. Un `ServiceEvent::PeerGone(id, gen)`
    /// generato da quel reader — per quanto rapidamente il link muoia — troverà quindi
    /// SEMPRE la generazione corrispondente già scritta: mai stale per questo motivo.
    fn register_link(&mut self, id: PeerId, gen: u64) {
        self.link_gen.insert(id, gen);
        self.connected.insert(id);
    }

    /// Registra un link in USCITA riuscito (`ConnectOutcome::Success`): come
    /// `register_link`, più la liberazione di `connecting` (il guard anti-duplicato di
    /// `Effect::ConnectTo` — la connessione non è più "in volo", è stabilita).
    ///
    /// Non tocca `self.links` (il `writer_tx` non esiste ancora: nasce da
    /// `spawn_peer_tasks`, chiamata dal chiamante SUBITO DOPO) né invia il `Join` — quello
    /// resta responsabilità del ramo 4 di `select!` in `run`, che ha il `writer_tx`
    /// restituito da `spawn_peer_tasks`.
    fn register_connect_success(&mut self, id: PeerId, gen: u64) {
        self.register_link(id, gen);
        self.connecting.remove(&id);
    }

    /// Un tentativo di connessione in uscita è fallito (errore o timeout,
    /// `ConnectOutcome::Failure`): l'unica mutazione necessaria è liberare `connecting`,
    /// così un futuro `Discovered`/`decide_and_connect` per lo stesso peer possa
    /// ritentare (comportamento di retry-on-failure preesistente, invariato da Slice C).
    fn register_connect_failure(&mut self, id: PeerId) {
        self.connecting.remove(&id);
    }
}

/// Avvia i task reader e writer per un peer appena connesso o accettato.
///
/// ## Perché una funzione libera (non metodo)?
///
/// `run`/`perform` hanno già un `&mut self`, e `tokio::spawn` richiede `'static`:
/// i task non possono tenere un riferimento a `self`. Passare i canali clonati (inbox_tx,
/// new_link_tx) è sufficiente per re-iniettare eventi nell'attore senza condividere stato.
///
/// ## Wire protocol (JSON-per-riga)
///
/// Ogni `ChatMsg` è serializzato come JSON + `\n` (una riga). Il reader legge una riga
/// alla volta con `BufReader::read_line`. Questo è il formato definito in `wire.rs` e
/// implementato in `TcpPeerLink`; qui lo reimplementiamo inline per poter dividere il
/// `TcpStream` in due task indipendenti (reader e writer), cosa non possibile con la
/// struct `TcpPeerLink` (che tiene insieme le due metà).
///
/// ## Debito DRY
///
/// La logica JSON-per-riga duplica quella di `TcpPeerLink` (`net.rs`). La refactoring
/// corretta sarebbe estrarre `send_json_line` e `recv_json_line` in funzioni libere usabili
/// sia da `TcpPeerLink` sia da questi task. Rinviato — vedere `task-8-report.md`.
///
/// ## Generazione del link (debito #4, Slice C)
///
/// `gen` identifica UNIVOCAMENTE questa istanza di link (assegnata dal chiamante via
/// `self.next_gen` — vedi il doc-comment del campo su `AiChatService`). Il reader la
/// riporta indietro nel `ServiceEvent::PeerGone` che invia alla propria uscita:
/// `handle_event` la confronta con la generazione ATTUALE registrata per `peer_id` per
/// scartare un `PeerGone` stale (di un link già rimpiazzato da una riconnessione più
/// recente) invece di rimuovere per errore il link fresco.
///
/// Slice C2: questa funzione viene chiamata ESCLUSIVAMENTE dall'attore (`run`, con
/// `&mut self`), SEMPRE dopo `register_link`/`register_connect_success` — mai più dentro
/// un task spawnato "a monte" (l'accept loop o il task di connessione), che è ciò che
/// causava la race residua chiusa da questa slice — vedi il doc-comment di
/// `register_link`.
///
/// ## Shutdown (debito #2, Slice B)
///
/// `shutdown` è il token di cancellazione dell'intero processo (passato "verso il basso"
/// da `run` → `perform` → qui). Il reader e il writer sono due task Tokio distinti, quindi
/// ciascuno riceve il proprio **clone** del token (`Clone` su `CancellationToken` condivide
/// lo stato interno — cancellare un clone cancella tutti gli altri): entrambi i task
/// smettono di lavorare non appena la cancellazione viene richiesta, senza aspettare
/// un EOF/errore/timeout che potrebbe non arrivare mai (es. un peer muto ma con
/// connessione TCP ancora aperta).
fn spawn_peer_tasks(
    stream: TcpStream,
    peer_id: PeerId,
    inbox_tx: UnboundedSender<ServiceEvent>,
    gen: u64,
    shutdown: tokio_util::sync::CancellationToken,
) -> UnboundedSender<ChatMsg> {
    // Divide il `TcpStream` in metà lettura e scrittura indipendenti.
    // `tokio::io::split` è la versione per i tipi che implementano `AsyncRead + AsyncWrite`;
    // le due metà possono vivere in task distinti senza condividere un `Mutex`.
    let (read_half, mut write_half) = tokio::io::split(stream);

    // Canale mpsc per il writer task: l'attore (e gli altri task) mandano `ChatMsg` qui,
    // e il writer task li scrive sul socket TCP come righe JSON.
    let (writer_tx, mut writer_rx) = tokio::sync::mpsc::unbounded_channel::<ChatMsg>();

    // Un clone per il writer, uno per il reader: sono due task separati, ognuno tiene
    // la propria "vista" sullo stesso token condiviso (vedi doc-comment sopra).
    let shutdown_writer = shutdown.clone();
    let shutdown_reader = shutdown;

    // --- Task WRITER: mpsc → TCP (una riga JSON per messaggio) ---
    //
    // Gira finché il sender del canale mpsc è vivo; termina quando il canale si chiude
    // (il sender è stato droppato, es. per `Effect::Disconnect`) O quando arriva lo
    // shutdown globale.
    tokio::spawn(async move {
        loop {
            tokio::select! {
                // Spegnimento globale: esci subito, senza attendere altri messaggi in coda.
                _ = shutdown_writer.cancelled() => break,
                maybe_msg = writer_rx.recv() => {
                    let Some(msg) = maybe_msg else { break };
                    // Serializza il ChatMsg come JSON.
                    let mut line = match serde_json::to_string(&msg) {
                        Ok(s) => s,
                        Err(e) => {
                            tracing::warn!(
                                "aichat: writer task: serializzazione fallita (peer {:?}): {}",
                                peer_id, e
                            );
                            continue; // salta questo messaggio, non termina
                        }
                    };
                    line.push('\n'); // delimitatore riga (protocollo wire)
                    if let Err(e) = write_half.write_all(line.as_bytes()).await {
                        tracing::debug!(
                            "aichat: writer task: errore scrittura (peer {:?}): {}",
                            peer_id, e
                        );
                        break; // errore fatale sul socket → termina il task
                    }
                }
            }
        }
        // Droppare `write_half` chiude il lato scrittura del TCP:
        // il peer vede EOF sul suo lato lettura (FIN nel TCP).
    });

    // --- Task READER: TCP → inbox (una riga JSON → ServiceEvent) ---
    //
    // Legge righe JSON dal socket e le re-inietta nell'inbox dell'attore come `PeerMsg`.
    // Termina su EOF (il peer ha chiuso), errore irreversibile del socket, timeout di
    // silenzio keepalive (`DEAD_THRESHOLD` senza NESSUN traffico, `Ping` incluso), o
    // shutdown globale — i quattro percorsi convergono tutti sullo stesso
    // `ServiceEvent::PeerGone` in fondo al task (innocuo se `run` sta già uscendo per
    // lo stesso shutdown: l'inbox potrebbe già non avere più nessuno in ascolto, e
    // `send` su un canale chiuso fallisce silenziosamente).
    tokio::spawn(async move {
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();
        loop {
            line.clear();
            tokio::select! {
                // Spegnimento globale: esci subito. Una riga parzialmente letta viene
                // scartata — corretto qui, perché stiamo comunque per abbandonare il link.
                _ = shutdown_reader.cancelled() => break,
                // `tokio::time::timeout` restituisce `Result<T, Elapsed>`: qui T è a sua volta
                // il `std::io::Result<usize>` di `read_line`, quindi il match ha due livelli.
                // Livello esterno: `Err` = è scaduto il timeout (nessun byte in DEAD_THRESHOLD).
                // Livello interno (`Ok(..)`): l'esito reale della lettura, invariato rispetto
                // a prima (EOF / riga valida / errore I/O).
                result = tokio::time::timeout(DEAD_THRESHOLD, reader.read_line(&mut line)) => {
                    match result {
                        // Nessuna riga arrivata entro DEAD_THRESHOLD (15s senza NESSUN traffico,
                        // Ping incluso): stesso trattamento di un EOF/errore reale — il peer è
                        // considerato sparito. Questo È il "retest" descritto nel design: finché
                        // arriva almeno un Ping ogni ~5s il timeout non scade mai; un blip isolato
                        // non basta (serve silenzio continuo per l'intera soglia).
                        Err(_elapsed) => {
                            // A `warn!` (non `debug!`, invisibile a filtro default INFO):
                            // questo è l'UNICO dei tre percorsi verso `PeerGone` che indica
                            // un vero "nessun byte sul filo" — non un EOF pulito né un errore
                            // I/O esplicito. Prima di questa slice era indistinguibile dagli
                            // altri due senza `RUST_LOG=debug`, rendendo impossibile capire a
                            // posteriori se un "peer sparito" osservato in chat fosse un vero
                            // silenzio keepalive o altro.
                            tracing::warn!(
                                "aichat: reader task: PEER-GONE per timeout keepalive — nessun byte ricevuto entro {:?} (peer {:?})",
                                DEAD_THRESHOLD, peer_id
                            );
                            break;
                        }
                        Ok(Ok(0)) => {
                            // EOF: il peer ha chiuso la connessione in modo pulito. `info!`
                            // (non `warn!`: è l'esito normale di uno shutdown/riavvio), ma
                            // comunque visibile a filtro default — per lo stesso motivo di
                            // distinguibilità del ramo timeout sopra.
                            tracing::info!(
                                "aichat: reader task: PEER-GONE per EOF pulito (peer {:?})",
                                peer_id
                            );
                            break;
                        }
                        Ok(Ok(_)) => {
                            // Decodifica la riga JSON in `ChatMsg`.
                            // `trim_end` rimuove `\n` (e `\r\n` su Windows) prima del parse.
                            match serde_json::from_str::<ChatMsg>(line.trim_end()) {
                                Ok(msg) => {
                                    // Re-inietta nell'inbox dell'attore. Se il canale è chiuso
                                    // (il loop run è terminato), ignoriamo silenziosamente.
                                    let _ = inbox_tx.send(ServiceEvent::PeerMsg { from: peer_id, msg });
                                }
                                Err(e) => {
                                    tracing::debug!(
                                        "aichat: reader task: JSON malformato (peer {:?}): {}",
                                        peer_id, e
                                    );
                                    // Riga malformata: non critica, continuiamo a leggere.
                                }
                            }
                        }
                        Ok(Err(e)) => {
                            // Anomalo quanto il timeout sopra: `warn!` per lo stesso motivo.
                            tracing::warn!(
                                "aichat: reader task: PEER-GONE per errore I/O (peer {:?}): {}",
                                peer_id, e
                            );
                            break; // errore I/O fatale sul socket
                        }
                    }
                }
            }
        }
        // Notifica l'attore: questo peer non è più raggiungibile.
        // L'attore rimuoverà il peer da `connected` e rielegerà il cluster — ma SOLO se
        // `gen` combacia ancora con la generazione registrata (debito #4): questo reader
        // potrebbe essere quello di un link già rimpiazzato da una riconnessione.
        let _ = inbox_tx.send(ServiceEvent::PeerGone(peer_id, gen));
    });

    // Restituisce il sender del writer task: l'attore lo memorizza in `self.links[peer_id]`.
    writer_tx
}

/// Helper di test accessibili solo in build `#[cfg(test)]`.
///
/// Implementano il "back door" che permette ai test di costruire uno stato valido
/// *senza* passare dal reader/writer TCP reale (nessun socket, niente I/O). Task 8:
/// il vecchio consenso pairwise (`ServiceEvent::Consent`) è stato rimosso —
/// `Discovered` da sola basta a portare un peer nel roster dell'elezione.
///
/// In OOP sarebbe un "test builder" o "test fixture setup method" sul tipo.
#[cfg(test)]
impl AiChatService {
    /// Espone il ruolo corrente: utile nei test per asserire il risultato dell'elezione
    /// dopo un `Discovered` senza dover accedere direttamente a `channel`.
    ///
    /// Analogia OOP: un "protected getter" usato solo nelle test-fixture.
    pub fn role_for_test(&self) -> Role {
        self.channel.role()
    }

    /// Inietta un orologio deterministico (Task 7, Blocco note): dopo questa chiamata,
    /// `now_ms()` reale non viene più consultato dagli arm `NoteCreateRequested`/
    /// `NoteEditRequested`/`NoteEditTitleRequested` — usano SEMPRE `ms`. Serve ai test
    /// che devono osservare l'esito del tie-break LWW su `title_touched` (altrimenti
    /// non deterministico: due chiamate ravvicinate nello stesso millisecondo
    /// produrrebbero lo stesso timestamp).
    pub fn set_wallclock_ms_for_test(&mut self, ms: u64) {
        self.wallclock_ms_for_test = Some(ms);
    }

    /// Simula un peer che diventa connesso senza passare da un vero accept/connect TCP.
    ///
    /// Inserisce una `PeerInfo` sintetica (la label non impatta l'elezione, che è
    /// basata solo sull'ordine numerico degli IPv4), aggiunge `id` a `connected`,
    /// e ricalcola il ruolo tenendo conto di `me` + tutti i peer connessi.
    ///
    /// L'elezione è **sticky**: se eravamo già server e il server corrente è ancora
    /// nel roster, il ruolo non cambia (nessuna prelazione da IP più basso).
    ///
    /// Debito #4 (Slice C): assegna anche una GENERAZIONE al link simulato, prelevandola
    /// dallo stesso contatore (`self.next_gen`) che userebbe l'attore in un vero
    /// `Effect::ConnectTo`/accept. Chiamare questo helper una seconda volta per lo STESSO
    /// `id` simula quindi anche una "riconnessione": la seconda chiamata assegna una
    /// generazione più recente, sostituendo quella registrata dalla prima (esattamente
    /// come accadrebbe con un link TCP reale sostituito da uno nuovo). Recupera la
    /// generazione assegnata con `link_gen_for_test`.
    ///
    /// Registra anche un `links[id]` FINTO (un `UnboundedSender<ChatMsg>` il cui
    /// ricevitore viene scartato subito): serve solo perché `links_contains_for_test`
    /// possa osservare "il link fresco NON è stato rimosso" nei test del debito #4 — non
    /// consegna mai davvero un messaggio (nessuno legge dall'altra parte), ma la voce
    /// nella mappa `links` è indistinguibile da un link reale ai fini della presenza/
    /// assenza che questi test verificano.
    pub fn mark_connected_for_test(&mut self, id: PeerId) {
        use crate::aichat::peer::Roster;

        // Inseriamo una PeerInfo sintetica: l'etichetta è irrilevante per l'elezione
        // (che dipende solo dall'IPv4 numerico), quindi un format descrittivo va bene.
        let synthetic_info = PeerInfo {
            id,
            label_base: format!("peer-{}", id.0),
            chat_port: 40100,
        };
        self.peers.insert(id, synthetic_info);
        self.connected.insert(id);
        let gen = self.next_gen;
        self.next_gen += 1;
        self.link_gen.insert(id, gen);
        let (fake_writer_tx, _unused_rx) = tokio::sync::mpsc::unbounded_channel::<ChatMsg>();
        self.links.insert(id, fake_writer_tx);

        // Ricalcola il ruolo: costruiamo un Roster con `me` + tutti i peer connessi.
        // `decide_role` in `AiChatChannel` aggiorna `self.channel.role` (sticky).
        let mut roster = Roster::new();
        roster.upsert(self.me.clone());
        for &pid in &self.connected {
            // `HashMap::get` vuole `&K` (reference alla chiave), non `K`.
            if let Some(info) = self.peers.get(&pid) {
                roster.upsert(info.clone());
            }
        }
        // Passa `self.reported_leader`, non `None` hardcoded: questo backdoor di test
        // deve restare coerente con `decide_and_connect` (stesso stato, stesso calcolo).
        // Nessun test esistente imposta `reported_leader` prima di questa chiamata, quindi
        // il comportamento per i test attuali è invariato (`None`).
        self.channel.decide_role(&roster, self.reported_leader);
    }

    /// Espone lo storico dei messaggi: utile nei test per asserire gli append e i replay
    /// senza dover intercettare gli `Effect::ToUi`/`SendToPeer` prodotti.
    pub fn history_for_test(&self) -> &[ChatLine] {
        &self.history
    }

    /// Espone l'ultimo leader loggato: utile nei test per verificare che il log di
    /// elezione si aggiorni solo quando il leader cambia davvero.
    pub fn last_logged_leader_for_test(&self) -> Option<PeerId> {
        self.last_logged_leader
    }

    /// Espone `reported_leader`: utile nei test per verificare che `Discovered`
    /// aggiorni/preservi correttamente il leader riportato (Bug 2, late-joiner).
    pub fn reported_leader_for_test(&self) -> Option<PeerId> {
        self.reported_leader
    }

    /// Espone il flag `listening`: utile nei test del debito #5 ("listening non si
    /// resetta") per verificare che `ListenerStopped` lo riporti a `false`.
    pub fn listening_for_test(&self) -> bool {
        self.listening
    }

    /// Imposta direttamente `listening`, bypassando `perform` (che farebbe I/O reale,
    /// cioè un vero `TcpListener::bind`). Serve per allestire lo stato "ero in ascolto"
    /// in un test PURO di `handle_event`, senza toccare la rete.
    pub fn set_listening_for_test(&mut self, v: bool) {
        self.listening = v;
    }

    // -------------------------------------------------------------------------
    // Debito #4 (Slice C): helper per i test sulla generazione dei link.
    // -------------------------------------------------------------------------

    /// Espone la generazione ATTUALMENTE registrata per un peer (`None` se non ha mai
    /// avuto un link, o se il link è già stato rimosso). Usato dai test per recuperare
    /// la generazione assegnata da `mark_connected_for_test` invece di indovinare un
    /// numero arbitrario (il contatore condiviso è cumulativo entro un intero test).
    pub fn link_gen_for_test(&self, id: PeerId) -> Option<u64> {
        self.link_gen.get(&id).copied()
    }

    /// Imposta direttamente la generazione di un peer, SENZA passare da
    /// `mark_connected_for_test` (che avrebbe l'effetto collaterale di sovrascrivere la
    /// `PeerInfo` con una sintetica — vedi le note sparse nei test più sotto che
    /// ri-annunciano il peer reale dopo `mark_connected_for_test` per questo motivo).
    /// Serve ai test che costruiscono lo stato SOLO tramite `Discovered` (mai
    /// "connessi" nel senso di `self.connected`) ma devono comunque far combaciare la
    /// generazione di un `PeerGone` di test con quella registrata.
    pub fn set_link_gen_for_test(&mut self, id: PeerId, gen: u64) {
        self.link_gen.insert(id, gen);
    }

    /// Espone se un peer è attualmente in `self.connected` — utile per verificare che un
    /// `PeerGone` stale (generazione non combaciante) NON abbia toccato un link fresco.
    pub fn connected_contains_for_test(&self, id: PeerId) -> bool {
        self.connected.contains(&id)
    }

    /// Espone se un peer ha ancora un link registrato in `self.links` — complementare a
    /// `connected_contains_for_test`: un `PeerGone` genuino deve ripulire ENTRAMBI.
    pub fn links_contains_for_test(&self, id: PeerId) -> bool {
        self.links.contains_key(&id)
    }

    // -------------------------------------------------------------------------
    // Debito #3 (Slice C): helper per i test del connect non bloccante.
    // -------------------------------------------------------------------------

    /// Espone se un peer è attualmente in `self.connecting` (un task di connessione è in
    /// volo per lui). Usato per verificare la guardia di de-dup di `Effect::ConnectTo`.
    pub fn connecting_contains_for_test(&self, id: PeerId) -> bool {
        self.connecting.contains(&id)
    }

    /// Espone quanti peer sono attualmente in `self.connecting` — usato per verificare
    /// che una seconda `ConnectTo` verso lo stesso peer non produca una voce duplicata
    /// (un `HashSet` non duplicherebbe comunque, ma la lunghezza rende l'asserzione
    /// esplicita invece di dipendere implicitamente dalla struttura dati sottostante).
    pub fn connecting_len_for_test(&self) -> usize {
        self.connecting.len()
    }

    /// Inserisce direttamente un peer in `self.connecting`, bypassando `perform` (che
    /// farebbe I/O reale). Serve per allestire lo stato "un connect è in volo" prima di
    /// testare `register_connect_success`/`register_connect_failure` in isolamento,
    /// senza toccare la rete.
    pub fn mark_connecting_for_test(&mut self, id: PeerId) {
        self.connecting.insert(id);
    }

    // -------------------------------------------------------------------------
    // AI Chat Slice 2 — helper per i test di auto-partecipazione (design §10.4).
    // -------------------------------------------------------------------------

    /// Espone il contatore di turni AI consecutivi — usato dai test per verificare
    /// che `note_room_message` lo aggiorni correttamente (reset su `-human`,
    /// incremento su `-ai`).
    pub fn consecutive_ai_turns_for_test(&self) -> usize {
        self.consecutive_ai_turns
    }

    /// Imposta direttamente il contatore, bypassando una sequenza di
    /// `note_room_message`/`handle_event` — serve ad allestire rapidamente lo stato
    /// "cap raggiunto" nei test che esercitano la guardia 4 di `maybe_autoparticipate`.
    pub fn set_consecutive_ai_turns_for_test(&mut self, n: usize) {
        self.consecutive_ai_turns = n;
    }

    /// Espone se un giudizio di auto-partecipazione è attualmente marcato "in volo" —
    /// usato dai test per verificare che `AutoParticipateDone` lo azzeri sempre
    /// (sia per `Some` sia per `None`).
    pub fn autoparticipate_inflight_for_test(&self) -> bool {
        self.autoparticipate_inflight
    }

    /// Imposta direttamente `autoparticipate_inflight`, bypassando `perform` (che
    /// farebbe I/O reale). Serve ad allestire lo stato "un giudizio è già in corso"
    /// nei test che esercitano la guardia 5 di `maybe_autoparticipate`.
    pub fn set_autoparticipate_inflight_for_test(&mut self, v: bool) {
        self.autoparticipate_inflight = v;
    }

    // -------------------------------------------------------------------------
    // Ammissione alla stanza — helper per i test (Task 3: solo i default;
    // i Task 4-7 aggiungeranno getter/setter per esercitare `record_vote`/
    // `resolve_if_ready`/il flusso del nuovo arrivato).
    // -------------------------------------------------------------------------

    /// Espone lo stato di ammissione di QUESTA macchina — usato dal test che
    /// verifica il default (`NotJoining` alla creazione).
    pub fn self_admission_for_test(&self) -> &SelfAdmission {
        &self.self_admission
    }

    /// Impone direttamente `self_admission`, bypassando la sequenza di eventi
    /// che normalmente ce la farebbe raggiungere (`begin_join_gate` →
    /// `JoinDecision` → ...). Serve ai test del Task 6 che vogliono partire
    /// da uno stato intermedio della state machine (es. `Deciding` per
    /// testare la risposta al gate 1, o `Rejected` per testare il
    /// re-request) senza rieseguire tutti i passaggi precedenti.
    pub fn set_self_admission_for_test(&mut self, v: SelfAdmission) {
        self.self_admission = v;
    }

    /// Espone quanti voti di ammissione sono attualmente in corso (lato
    /// server) — usato dal test che verifica il default (vuoto alla
    /// creazione).
    pub fn pending_votes_len_for_test(&self) -> usize {
        self.pending_votes.len()
    }

    /// Simula che il peer `id` sia già stato AMMESSO alla stanza (Task 4),
    /// senza dover eseguire l'intero flusso di voto (Task 5). Da usare
    /// INSIEME a `mark_connected_for_test` quando un test vuole un peer
    /// "presente" che DEVE contare come votante — a differenza di un peer
    /// solo connesso (link TCP vivo ma non ancora ammesso, che NON deve
    /// votare — vedi il doc-comment del campo `admitted`).
    pub fn mark_admitted_for_test(&mut self, id: PeerId) {
        self.admitted.insert(id);
    }

    /// Espone i "presenti" congelati per un voto di ammissione in corso (il
    /// campo `present` di `PendingVote`, indicizzato per candidato) — usato
    /// dai test per verificare che `start_admission_vote` li abbia calcolati
    /// da `self.admitted` (non da `self.connected`). `None` se nessun voto è
    /// attualmente registrato per quel candidato.
    pub fn pending_vote_present_for_test(&self, candidate: PeerId) -> Option<Vec<PeerId>> {
        self.pending_votes.get(&candidate).map(|pv| pv.present.iter().copied().collect())
    }

    /// (FIX #1) Espone la generazione del voto attualmente in corso per `candidate`
    /// (`None` se non ce n'è uno): usato dai test per costruire un `VoteTimeout`
    /// con la generazione giusta — o volutamente stantia.
    pub fn pending_vote_generation_for_test(&self, candidate: PeerId) -> Option<u64> {
        self.pending_votes.get(&candidate).map(|pv| pv.generation)
    }

    /// Espone `outgoing_shares` (share_id → `OutgoingShare { target_label, doc_name,
    /// rel_path, stage }`) — usato dai test per verificare che `request_share` abbia
    /// registrato il trasferimento avviato prima di mandare `ChatMsg::ShareOffer`, e
    /// (Slice 2a) che lo stadio transizioni correttamente su Accept/invio contenuto.
    pub fn outgoing_shares_for_test(&self) -> &HashMap<String, OutgoingShare> {
        &self.outgoing_shares
    }

    /// Espone `pending_shares` (share_id → `PendingShare`) — usato dai test per
    /// verificare che `handle_share_offer` abbia registrato (o rifiutato) il
    /// trasferimento in arrivo.
    pub fn pending_shares_for_test(&self) -> &HashMap<String, PendingShare> {
        &self.pending_shares
    }

    /// Riempie `pending_shares` fino a `MAX_PENDING_SHARES` con entry sintetiche — usato
    /// solo dal test del cap (`peer_share_offer_auto_rejects_when_cap_reached`).
    pub fn fill_pending_shares_to_cap_for_test(&mut self) {
        for i in 0..MAX_PENDING_SHARES {
            self.pending_shares.insert(
                format!("filler:{i}"),
                PendingShare::AwaitingDecision {
                    from_label: "filler".into(),
                    doc_name: "x.md".into(),
                    size_bytes: 1,
                },
            );
        }
    }

    /// Espone se `id` è tra i client GIÀ AMMESSI (Task 5, `resolve_admit`) — usato
    /// dai test per verificare l'esito di un voto (ammesso dopo tutti i sì / non
    /// ammesso dopo un veto).
    pub fn admitted_contains_for_test(&self, id: PeerId) -> bool {
        self.admitted.contains(&id)
    }

    /// Espone l'ultimo roster noto (`self.last_known_roster`) — usato dai test di
    /// ammissione per verificare che `resolve_admit` lo abbia aggiornato con
    /// l'etichetta del candidato appena ammesso.
    pub fn last_known_roster_for_test(&self) -> &[String] {
        &self.last_known_roster
    }
}

// =============================================================================
// AI Chat Slice 1a/1b — funzioni PURE (nessun `self`, testabili in isolamento
// totale). Vedi Docs/superpowers/specs/2026-07-02-aichat-ai-participants-design.md
// §2/§4 (1a) e §9 (1b: @all / @<label>-ai + il flag ai_participates).
// =============================================================================

/// A CHI è rivolta un'invocazione AI rilevata in un messaggio umano (Slice 1b).
///
/// In OOP potremmo chiamarlo il "destinatario" del comando: `handle_event`
/// (nel chiamante) confronta questo valore con la propria identità
/// (`self.me.label_base`) per decidere se la PROPRIA AI deve rispondere.
#[derive(Debug, Clone, PartialEq)]
enum InvokeTarget {
    /// `@ai` — la propria AI DEL MITTENTE del messaggio (shorthand). Rilevato
    /// su qualunque messaggio, ma ha senso di "invocazione verso di me" SOLO
    /// quando il mittente sono io stesso (arm `HumanSay`) — vedi il
    /// doc-comment dell'arm `PeerMsg::Say` per il perché va ignorato da remoto.
    Own,
    /// `@<label>-ai` — l'AI di una macchina SPECIFICA (per `label_base`),
    /// eventualmente remota. Il confronto col proprio `label_base` è
    /// case-sensitive (stessa convenzione già usata per `peer_label` in
    /// `ServiceEvent::AdmissionVoteUi`).
    Label(String),
    /// `@all` — TUTTE le AI presenti nella stanza.
    All,
}

/// Un'invocazione AI rilevata in un messaggio: il destinatario (`target`) e la
/// richiesta da passare come prompt (`request`, mai vuota — vedi
/// `extract_ai_invocation`).
#[derive(Debug, Clone, PartialEq)]
struct Invocation {
    target: InvokeTarget,
    request: String,
}

/// Rileva un'invocazione esplicita dell'AI (propria, di un'altra macchina, o di
/// tutte) nel messaggio di un umano.
///
/// Riconosce tre forme (dopo aver ignorato spazi iniziali), tutte
/// case-insensitive SOLO sul marcatore (il testo del `label` in `@<label>-ai`
/// mantiene il case originale — il confronto con `label_base` è case-sensitive,
/// vedi `InvokeTarget::Label`):
/// - `@ai` → [`InvokeTarget::Own`] (Slice 1a, invariato).
/// - `@all` → [`InvokeTarget::All`] (Slice 1b).
/// - `@<label>-ai` → [`InvokeTarget::Label`] (Slice 1b).
///
/// Il messaggio è tokenizzato sul primo spazio bianco: tutto ciò che precede è
/// il "marcatore" (`@ai`/`@all`/`@<label>-ai`), tutto ciò che segue (trimmato)
/// è la richiesta. Questo tokenizzare-per-spazio è ciò che garantisce IL
/// CONFINE DI PAROLA senza bisogno di un controllo esplicito separato: "@aiuto"
/// è un marcatore di un solo token ("@aiuto") che non combacia con nessuna
/// delle tre forme riconosciute, quindi `None` — nessun falso positivo su
/// parole che iniziano per "ai".
///
/// Se la richiesta (il testo dopo il marcatore) è vuota, usa un default
/// leggibile invece di mandare un prompt vuoto all'AI. Ritorna `None` per
/// qualunque messaggio che non è uno dei tre marcatori riconosciuti — è questo
/// `None`, insieme alla GUARDIA LOOP sul mittente (`-human` vs `-ai`, vedi
/// l'arm `PeerMsg::Say`), che rende il modello loop-safe per costruzione: una
/// conversazione umano↔umano "normale" non fa mai parlare un'AI, e una
/// risposta di un'AI non può mai essere scambiata per un'invocazione.
fn extract_ai_invocation(text: &str) -> Option<Invocation> {
    let trimmed = text.trim_start();
    if !trimmed.starts_with('@') {
        return None;
    }

    // Il "marcatore" è tutto ciò che precede il primo spazio bianco (o l'intera
    // stringa, se non c'è spazio — caso "@ai"/"@all" da soli).
    let token_end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
    let token = &trimmed[..token_end];
    let request_text = trimmed[token_end..].trim();
    let request = if request_text.is_empty() {
        "commenta la discussione".to_string()
    } else {
        request_text.to_string()
    };

    // Confronto case-insensitive SOLO sul marcatore. `to_ascii_lowercase`
    // trasforma solo i byte ASCII A-Z (mai i byte UTF-8 multi-byte di un
    // eventuale label non-ASCII): preserva ESATTAMENTE la lunghezza in byte
    // di `token`, quindi gli indici calcolati su `lower` sotto (via
    // `strip_prefix`/`strip_suffix`, che a loro volta non spezzano mai un
    // confine di carattere) restano validi anche per estrarre il label dal
    // `token` ORIGINALE (case preservato per il confronto con `label_base`).
    let lower = token.to_ascii_lowercase();
    let target = if lower == "@ai" {
        InvokeTarget::Own
    } else if lower == "@all" {
        InvokeTarget::All
    } else if let Some(label_lower) = lower.strip_prefix('@').and_then(|s| s.strip_suffix("-ai")) {
        if label_lower.is_empty() {
            // "@-ai": nessun label_base è vuoto (vietato da `validate` lato UI)
            // — non è un'invocazione valida, non un falso positivo su un caso
            // limite non gestito.
            return None;
        }
        // Ricalcola il label dal token ORIGINALE (case preservato): stessa
        // porzione di byte di `label_lower`, sicura per il motivo spiegato
        // sopra (`to_ascii_lowercase` preserva la lunghezza in byte).
        let original_label = &token[1..token.len() - 3];
        InvokeTarget::Label(original_label.to_string())
    } else {
        return None;
    };

    Some(Invocation { target, request })
}

/// Converte lo storico della stanza in turni a ruoli veri per il prompt dell'AI: righe
/// proprie (`from_label == my_ai_label`) diventano turni `assistant` (testo nudo, il
/// ruolo stesso è il marcatore d'identità), tutte le altre diventano turni `user`
/// (prefissate `"from_label: testo"` — serve ANCORA distinguere fra voci diverse tra
/// gli "altri"). Righe consecutive dello STESSO ruolo sono coalescute in un turno solo
/// (unite da `\n`, ciascuna riga ancora prefissata dentro il turno `user`): l'API rifiuta
/// `[user, user]` consecutivi con HTTP 400 (stessa igiene di `ai_adapter.rs`, vedi
/// `history_stays_clean_after_error`). Sostituisce `format_transcript` (Slice 1a) — fix
/// del mirroring d'identità, vedi
/// `Docs/superpowers/specs/2026-07-07-aichat-identity-anchor-design.md` §3. Funzione PURA
/// (nessun accesso a `self`) — riusabile e testabile senza costruire un intero
/// `AiChatService`. Il cap di lunghezza per sessioni molto lunghe resta una domanda
/// aperta del design originale (§8 punto 3): non affrontato in questa slice.
fn build_ai_history(history: &[ChatLine], my_ai_label: &str) -> Vec<Message> {
    let mut turns: Vec<Message> = Vec::new();
    for line in history {
        let is_own = line.from_label == my_ai_label;
        let role = if is_own { "assistant" } else { "user" };
        let text = if is_own {
            line.text.clone()
        } else {
            format!("{}: {}", line.from_label, line.text)
        };
        match turns.last_mut() {
            Some(last) if last.role == role => {
                if let Some(Block::Text { text: last_text }) = last.content.last_mut() {
                    last_text.push('\n');
                    last_text.push_str(&text);
                }
            }
            _ => turns.push(Message::with_blocks(role, vec![Block::Text { text }])),
        }
    }
    turns
}

/// Cerca la PRIMA riga che inizia con `"MEMORIA: "` (dopo trim, case-sensitive) nel testo che
/// la PROPRIA AI ha appena restituito — MAI applicata alle righe altrui nella history/
/// transcript (chiamata solo sul testo di ritorno di `chat_reply`/`chat_autoparticipate`, vedi
/// gli arm `ServiceEvent::AiReply`/`AutoParticipateDone`). Tutto ciò che segue il prefisso, fino
/// a fine riga, è la nota — `None` se il marker non c'è o la nota (dopo trim) è vuota: non
/// salvare note vuote. Funzione PURA. Vedi
/// `Docs/superpowers/specs/2026-07-07-aichat-persistent-memory-design.md` §3.
fn extract_memoria_marker(text: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.trim().strip_prefix("MEMORIA: "))
        .map(str::trim)
        .filter(|note| !note.is_empty())
        .map(str::to_string)
}

/// Accoda `note` (una nuova riga) al file indicato da `path`, creando cartella+file se
/// assenti. Letta-modificata-scritta per intero (non un append a livello di filesystem):
/// file piccolo, operazione rara, semplicità sopra micro-ottimizzazione. Separata da
/// `append_memory_note` per essere testabile su un path arbitrario (es. una cartella
/// temporanea) senza toccare la vera `config_dir`.
fn append_note_at(path: &std::path::Path, note: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let updated = if existing.is_empty() {
        format!("{note}\n")
    } else {
        format!("{existing}{note}\n")
    };
    std::fs::write(path, updated)
}

/// Accoda `note` a `memory-{label_base}.md` in `config_dir`. Il chiamante
/// (`perform`) tiene `memory_write_lock` per la durata di questa chiamata.
/// Riusa `ai_adapter::memory_file_path` (2.0, Task 4) — UNA sola copia nel
/// crate, invece della duplicazione deliberata della v1 (che qui risolveva
/// `LARE_LOCAL_DIR`/`LOCALAPPDATA` per conto proprio).
fn append_memory_note(config_dir: &std::path::Path, label_base: &str, note: &str) -> std::io::Result<()> {
    append_note_at(&crate::ai_adapter::memory_file_path(config_dir, label_base), note)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::net::Ipv4Addr;

    /// Crea un `PeerInfo` di test con l'ultimo ottetto `d` e l'etichetta base `base`.
    fn info(d: u8, base: &str) -> PeerInfo {
        PeerInfo { id: PeerId(Ipv4Addr::new(192, 168, 1, d)), label_base: base.into(), chat_port: 40100 }
    }

    /// `reachable_peer_labels`: vuoto quando non c'è nessun peer scoperto.
    #[test]
    fn reachable_peer_labels_empty_when_no_peers() {
        let s = AiChatService::new_for_test(info(10, "m10"));
        assert!(s.reachable_peer_labels().is_empty());
    }

    /// Un peer scoperto ma SENZA link TCP vivo non è raggiungibile — copre
    /// esattamente il caso del bug originale (discovery+elezione avvenuti,
    /// nessun link ancora stabilito).
    #[test]
    fn reachable_peer_labels_excludes_discovered_peer_without_link() {
        let mut s = AiChatService::new_for_test(info(10, "m10"));
        s.handle_event(ServiceEvent::Discovered(info(20, "m20"), None));
        // `Discovered` da solo NON stabilisce un link: nessun mark_connected_for_test.
        assert!(s.reachable_peer_labels().is_empty(),
            "senza link il peer non deve essere raggiungibile: {:?}", s.reachable_peer_labels());
    }

    /// Un peer scoperto E con link vivo (`mark_connected_for_test` inserisce
    /// entrambi) È raggiungibile, con suffisso `-human`.
    #[test]
    fn reachable_peer_labels_includes_discovered_and_linked_peer() {
        let mut s = AiChatService::new_for_test(info(10, "m10"));
        let id20 = PeerId(Ipv4Addr::new(192, 168, 1, 20));
        s.mark_connected_for_test(id20);
        let labels = s.reachable_peer_labels();
        assert_eq!(labels, vec![format!("peer-{}-human", id20.0)]);
    }

    /// `Discovered` deve SEMPRE emettere `AiChatReachablePeers` (anche se vuoto)
    /// — copre la race discovery-vs-link: un link può stabilirsi PRIMA che il
    /// relativo `Discovered` arrivi (UDP e TCP sono meccanismi indipendenti);
    /// senza questa emissione quel caso resterebbe silenzioso finché non
    /// scattasse un altro evento scorrelato.
    #[test]
    fn discovered_always_emits_reachable_peers_snapshot() {
        let mut s = AiChatService::new_for_test(info(10, "m10"));
        let effects = s.handle_event(ServiceEvent::Discovered(info(20, "m20"), None));
        let labels = effects.iter().find_map(|e| match e {
            Effect::ToUi(ServerMsg::AiChatReachablePeers { labels }) => Some(labels.clone()),
            _ => None,
        });
        assert_eq!(labels, Some(Vec::<String>::new()),
            "Discovered deve emettere AiChatReachablePeers anche vuoto: {effects:?}");
    }

    /// `PeerGone` deve ri-emettere `AiChatReachablePeers` SENZA il peer sparito.
    #[test]
    fn peer_gone_removes_peer_from_reachable_peers() {
        let mut s = AiChatService::new_for_test(info(10, "m10"));
        let id20 = PeerId(Ipv4Addr::new(192, 168, 1, 20));
        let id30 = PeerId(Ipv4Addr::new(192, 168, 1, 30));
        s.mark_connected_for_test(id20);
        s.mark_connected_for_test(id30);
        let gen20 = s.link_gen_for_test(id20).expect("gen di id20 dopo mark_connected_for_test");

        let effects = s.handle_event(ServiceEvent::PeerGone(id20, gen20));
        let labels = effects.iter().find_map(|e| match e {
            Effect::ToUi(ServerMsg::AiChatReachablePeers { labels }) => Some(labels.clone()),
            _ => None,
        }).expect("PeerGone deve emettere AiChatReachablePeers");

        assert!(!labels.contains(&format!("peer-{}-human", id20.0)), "id20 doveva sparire: {labels:?}");
        assert!(labels.contains(&format!("peer-{}-human", id30.0)), "id30 doveva restare: {labels:?}");
    }

    /// `Effect::Disconnect` (eseguito in `perform`, non in `handle_event`) deve
    /// notificare la UI col nuovo snapshot di raggiungibilità DOPO aver
    /// rimosso il link — non prima. Scenario reale: l'umano rifiuta il gate 1
    /// chat (`JoinDecision{accept:false}`), che produce `Effect::Disconnect`;
    /// una volta eseguito, quel peer non deve più comparire come raggiungibile.
    #[tokio::test]
    async fn disconnect_effect_notifies_reachable_peers_without_the_disconnected_peer() {
        let mut s = AiChatService::new_for_test(info(10, "m10"));
        let id20 = PeerId(Ipv4Addr::new(192, 168, 1, 20));
        s.mark_connected_for_test(id20);

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ServerMsg>();
        s.handle_event(ServiceEvent::SetServerTx(tx));

        let (inbox_tx, _inbox_rx) = tokio::sync::mpsc::unbounded_channel();
        let (new_link_tx, _new_link_rx) = tokio::sync::mpsc::unbounded_channel();
        let (connect_res_tx, _connect_res_rx) = tokio::sync::mpsc::unbounded_channel();
        let shutdown = tokio_util::sync::CancellationToken::new();

        s.perform(Effect::Disconnect(id20), &inbox_tx, &new_link_tx, &connect_res_tx, &shutdown).await;

        let msg = rx.try_recv().expect("perform(Disconnect) deve mandare AiChatReachablePeers");
        match msg {
            ServerMsg::AiChatReachablePeers { labels } => {
                assert!(labels.is_empty(), "id20 doveva sparire dai raggiungibili: {labels:?}");
            }
            other => panic!("atteso AiChatReachablePeers, trovato {other:?}"),
        }
    }

    /// A differenza del roster (dietro `if !last_known_roster.is_empty()`),
    /// `AiChatReachablePeers` va ri-emesso su OGNI `SetServerTx`, ANCHE quando
    /// la lista è vuota — la UI deve poter distinguere "vuota confermata" da
    /// "non ancora arrivata".
    #[test]
    fn set_server_tx_always_emits_reachable_peers_even_when_empty() {
        let mut s = AiChatService::new_for_test(info(10, "m10"));
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<ServerMsg>();
        let effects = s.handle_event(ServiceEvent::SetServerTx(tx));
        let labels = effects.iter().find_map(|e| match e {
            Effect::ToUi(ServerMsg::AiChatReachablePeers { labels }) => Some(labels.clone()),
            _ => None,
        });
        assert_eq!(labels, Some(Vec::<String>::new()),
            "SetServerTx deve emettere AiChatReachablePeers anche a vuoto: {effects:?}");
    }

    /// Blocco note (design §7): a differenza del roster (dietro `if !last_known_roster.is_empty()`),
    /// `NotesSnapshot` va ri-emesso su OGNI `SetServerTx`, ANCHE quando la lista è
    /// vuota — la UI deve poter distinguere "vuota confermata" da "non ancora arrivata".
    /// Stesso principio già in uso per `AiChatReachablePeers`.
    #[test]
    fn set_server_tx_replays_notes_snapshot_unconditionally_even_when_empty() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let effects = s.handle_event(ServiceEvent::SetServerTx(tx));

        assert!(
            effects.iter().any(|e| matches!(e, Effect::ToUi(ServerMsg::NotesSnapshot { notes }) if notes.is_empty())),
            "deve emettere NotesSnapshot anche vuoto, stesso principio di AiChatReachablePeers: {effects:?}"
        );
    }

    /// Verifica che `SetServerTx` includa un snapshot di note già create in precedenza.
    #[test]
    fn set_server_tx_replays_notes_snapshot_with_existing_notes() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        s.handle_event(ServiceEvent::NoteCreateRequested { title: "A".into(), text: "x".into() });
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

        let effects = s.handle_event(ServiceEvent::SetServerTx(tx));

        let snapshot = effects.iter().find_map(|e| match e {
            Effect::ToUi(ServerMsg::NotesSnapshot { notes }) => Some(notes.clone()),
            _ => None,
        }).expect("deve emettere NotesSnapshot");
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].title, "A");
    }

    /// Helper di test: restituisce il ruolo corrente del servizio.
    /// Evita di accedere direttamente al campo `channel` (privato fuori dalla struct).
    fn s_role(s: &AiChatService) -> Role {
        s.role_for_test()
    }

    /// Verifica che `SetServerTx` memorizzi il sender ed emetta SOLO `AiChatSelf`
    /// (la nostra etichetta) quando non c'è nient'altro da ri-emettere.
    ///
    /// Questo testa il ramo più semplice dell'attore: la UI si connette e l'attore
    /// registra il suo canale di uscita. `AiChatSelf` è incondizionato (la finestra deve
    /// sempre sapere "chi sono io" per il titolo); roster/storico/consensi pendenti
    /// restano condizionati come prima — l'UI li riceverà quando ci sarà qualcosa.
    #[test]
    fn set_server_tx_event_stores_sender_and_emits_self_label_plus_empty_reachable_peers() {
        // Rinominato (era "...emits_only_self_label"): da spec 2026-07-28,
        // AiChatReachablePeers è ora ri-emesso INCONDIZIONATAMENTE su ogni
        // SetServerTx (a differenza del roster/storico, dietro `is_empty()`) —
        // "solo AiChatSelf" non è più vero nemmeno nel caso base.
        // A partire da Task 13, anche NotesSnapshot è ri-emesso incondizionatamente.
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<ServerMsg>();
        let effects = s.handle_event(ServiceEvent::SetServerTx(tx));
        assert_eq!(
            effects,
            vec![
                Effect::ToUi(ServerMsg::AiChatSelf { label: "skimble-human".into() }),
                Effect::ToUi(ServerMsg::AiChatReachablePeers { labels: Vec::new() }),
                Effect::ToUi(ServerMsg::NotesSnapshot { notes: Vec::new() }),
            ],
            "SetServerTx deve emettere AiChatSelf + AiChatReachablePeers + NotesSnapshot (tutti vuoti) quando non c'è altro da ri-emettere"
        );
        assert!(s.has_ui(), "dopo SetServerTx il servizio ha una UI connessa");
    }

    /// Verifica che `UiClosed` rimuova il sender (la UI non è più raggiungibile).
    ///
    /// Dopo `SetServerTx` + `UiClosed`, `has_ui()` deve tornare `false`: l'attore
    /// sa che non c'è UI connessa e non proverà a mandarle messaggi (evita panic
    /// su sender chiuso).
    #[test]
    fn ui_closed_event_drops_sender() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<ServerMsg>();
        s.handle_event(ServiceEvent::SetServerTx(tx));
        s.handle_event(ServiceEvent::UiClosed);
        assert!(!s.has_ui());
    }

    // -------------------------------------------------------------------------
    // Ammissione alla stanza (Task 3, scaffolding) — RED confermato: prima di
    // implementare `SelfAdmission`/i getter `_for_test`/il campo
    // `pending_votes`, `cargo test` falliva con 3 errori di compilazione
    // (E0599 su entrambi i metodi, E0433 sul tipo `SelfAdmission` inesistente).
    // -------------------------------------------------------------------------

    /// Un `AiChatService` appena creato non sta entrando in nessuna stanza
    /// (`SelfAdmission::NotJoining`) e non ha alcun voto di ammissione in
    /// corso (`pending_votes` vuoto) — i valori di default dello scaffolding.
    #[test]
    fn service_has_admission_state_defaults() {
        let s = AiChatService::new_for_test(info(10, "skimble"));
        assert_eq!(s.self_admission_for_test(), &SelfAdmission::NotJoining);
        assert_eq!(s.pending_votes_len_for_test(), 0);
    }

    // -------------------------------------------------------------------------
    // Ammissione alla stanza — Task 4: il SERVER avvia il voto su
    // `ChatMsg::RequestAdmission` (piano `2026-07-03-aichat-admission.md`).
    // -------------------------------------------------------------------------

    /// Scenario: me=.10 è SERVER (IP più basso). P=.20 è connesso E AMMESSO
    /// (un client che ha già superato il proprio voto). Q=.40 è connesso ma
    /// NON ammesso (un voto per lui è ancora in corso, o non è mai partito):
    /// non deve MAI comparire tra i "presenti" di un voto altrui — è
    /// esattamente il bug che il campo `admitted` (invece di `connected`)
    /// previene. C=.30 è il candidato che manda `RequestAdmission`.
    ///
    /// Il server deve: registrare un `PendingVote` per C con `present = {P, me}`
    /// — il proprio umano-server è SEMPRE un presente, vota anche lui (correzione
    /// del supervisore al Task 4: vedi il doc-comment di `start_admission_vote`)
    /// — (Q escluso); mandare `AdmissionVoteRequest` a P (non a Q, non a se
    /// stessi); mandare il gate 2 anche alla propria UI (il voto del proprio
    /// umano-server passa da `ToUi`, non da un `SendToPeer`); avviare il timer
    /// del voto.
    #[test]
    fn server_starts_vote_on_request_admission() {
        let me = info(10, "skimble");
        let me_id = me.id;
        let mut s = AiChatService::new_for_test(me);
        let p = PeerId(Ipv4Addr::new(192, 168, 1, 20));
        let q = PeerId(Ipv4Addr::new(192, 168, 1, 40));
        let c = PeerId(Ipv4Addr::new(192, 168, 1, 30));

        s.mark_connected_for_test(p);
        s.mark_connected_for_test(q);
        s.mark_admitted_for_test(p); // solo P è AMMESSO — Q resta solo "connesso"
        assert_eq!(s_role(&s), Role::Server, "IP .10 è il più basso: siamo server");

        let effects = s.handle_event(ServiceEvent::PeerMsg {
            from: c,
            msg: ChatMsg::RequestAdmission { label: "cand-human".into() },
        });

        assert_eq!(s.pending_votes_len_for_test(), 1, "un voto deve essere registrato per C");
        let present = s
            .pending_vote_present_for_test(c)
            .expect("deve esserci un PendingVote per il candidato C");
        assert_eq!(
            present.len(),
            2,
            "P (ammesso) e il proprio umano-server devono essere tra i presenti, non Q (solo connesso): {present:?}"
        );
        assert!(present.contains(&p), "P (ammesso) deve essere tra i presenti");
        assert!(present.contains(&me_id), "il proprio umano-server deve essere tra i presenti (vota anche lui)");
        assert!(!present.contains(&q), "Q (non ammesso) NON deve essere tra i presenti");

        assert!(
            effects.contains(&Effect::SendToPeer(
                p,
                ChatMsg::AdmissionVoteRequest { candidate: "cand-human".into() }
            )),
            "il presente P deve ricevere la richiesta di voto (gate 2): {effects:?}"
        );
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::SendToPeer(id, _) if *id == q)),
            "Q non è presente: non deve ricevere alcun effetto: {effects:?}"
        );
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::SendToPeer(id, _) if *id == me_id)),
            "non ci mandiamo un ChatMsg da soli: il nostro voto passa dal ToUi, non da SendToPeer: {effects:?}"
        );
        assert!(
            effects.contains(&Effect::ToUi(ServerMsg::AiChatAdmissionRequest {
                candidate: "cand-human".into()
            })),
            "il gate 2 deve arrivare anche al proprio umano-server: {effects:?}"
        );
        assert!(
            effects.contains(&Effect::StartVoteTimeout {
                candidate: c,
                // FIX #1: primo (e unico) voto di un servizio appena creato →
                // generazione 0 (il contatore `next_vote_gen` parte da 0).
                generation: 0,
                secs: ADMISSION_VOTE_TIMEOUT_SECS
            }),
            "il timer del voto deve partire: {effects:?}"
        );
    }

    /// Un peer che NON è il server ignora `RequestAdmission`: solo il server
    /// eletto coordina il voto. Scenario: me=.20 (IP più alto di P=.10 → CLIENT).
    #[test]
    fn non_server_ignores_request_admission() {
        let mut s = AiChatService::new_for_test(info(20, "skimble"));
        let server_id = PeerId(Ipv4Addr::new(192, 168, 1, 10));
        s.mark_connected_for_test(server_id);
        assert_eq!(s_role(&s), Role::Client(server_id), "IP .20 non è il più basso: siamo client");

        let c = PeerId(Ipv4Addr::new(192, 168, 1, 30));
        let effects = s.handle_event(ServiceEvent::PeerMsg {
            from: c,
            msg: ChatMsg::RequestAdmission { label: "cand-human".into() },
        });

        assert!(effects.is_empty(), "un non-server non deve produrre effetti: {effects:?}");
        assert_eq!(s.pending_votes_len_for_test(), 0, "un non-server non avvia nessun voto");
    }

    // -------------------------------------------------------------------------
    // Ammissione alla stanza — Task 5: raccolta voti, veto, admit/reject, timeout
    // (piano `2026-07-03-aichat-admission.md`). Scenario comune a tutti e quattro
    // i test: me=.10 è SERVER, P=.20 è connesso E AMMESSO (l'unico presente
    // remoto), C=.30 è il candidato che ha appena mandato `RequestAdmission` —
    // quindi `present = {P, me}` (vedi `server_starts_vote_on_request_admission`
    // sopra).
    // -------------------------------------------------------------------------

    /// Tutti i presenti votano sì (P via `PeerMsg`, l'umano-server via
    /// `AdmissionVoteUi`): il candidato viene ammesso solo DOPO l'ultimo voto —
    /// il voto di P da solo non basta (manca ancora l'umano-server), verifica
    /// esplicita che previene la regressione "dimentica un presente e ammette
    /// comunque".
    #[test]
    fn vote_all_yes_admits() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let p = PeerId(Ipv4Addr::new(192, 168, 1, 20));
        let c = PeerId(Ipv4Addr::new(192, 168, 1, 30));

        s.mark_connected_for_test(p);
        s.mark_admitted_for_test(p);
        s.handle_event(ServiceEvent::PeerMsg {
            from: c,
            msg: ChatMsg::RequestAdmission { label: "cand-human".into() },
        });

        // P vota sì: manca ancora il voto dell'umano-server, il turno resta aperto.
        let effects_after_p = s.handle_event(ServiceEvent::PeerMsg {
            from: p,
            msg: ChatMsg::AdmissionVote { candidate: "cand-human".into(), accept: true },
        });
        assert!(
            effects_after_p.is_empty(),
            "un solo sì su due presenti non deve chiudere il turno: {effects_after_p:?}"
        );
        assert_eq!(s.pending_votes_len_for_test(), 1, "il voto resta aperto finché non votano tutti");
        assert!(!s.admitted_contains_for_test(c), "non ancora ammesso: manca un voto");

        // L'umano-server vota sì: ORA tutti i presenti hanno detto sì → admit.
        let effects = s.handle_event(ServiceEvent::AdmissionVoteUi {
            candidate_label: "cand-human".into(),
            accept: true,
        });

        assert!(s.admitted_contains_for_test(c), "C deve essere ammesso dopo che tutti hanno votato sì");
        assert_eq!(s.pending_votes_len_for_test(), 0, "il voto deve essere rimosso dopo la risoluzione");
        assert!(
            effects.contains(&Effect::SendToPeer(c, ChatMsg::Admitted { label: "cand-human".into() })),
            "il candidato deve ricevere Admitted: {effects:?}"
        );
        assert!(
            effects.iter().any(|e| matches!(
                e,
                Effect::SendToPeer(_, ChatMsg::Roster { participants })
                    if participants.contains(&"cand-human".to_string())
            )),
            "il roster diffuso deve includere il candidato appena ammesso: {effects:?}"
        );
        assert!(
            s.last_known_roster_for_test().contains(&"cand-human".to_string()),
            "last_known_roster deve includere il candidato: {:?}",
            s.last_known_roster_for_test()
        );
    }

    /// Task 8 (`Docs/superpowers/plans/2026-07-03-aichat-admission.md`): preserva
    /// il comportamento del vecchio `ChatMsg::Join` (che dumpava lo storico
    /// completo al peer appena entrato) anche nel nuovo flusso di ammissione a
    /// voto. Senza questo, un candidato appena ammesso vedrebbe una stanza
    /// "muta": nessun messaggio precedente alla sua ammissione, perché
    /// `resolve_admit` aggiorna solo il `Roster` — mai lo storico.
    ///
    /// Scenario: c'è già un messaggio nello storico PRIMA che il candidato C
    /// chieda di entrare; dopo che il voto lo ammette, gli effetti devono
    /// includere un `ChatMsg::History` con esattamente quello storico.
    #[test]
    fn resolve_admit_sends_history_dump_to_candidate() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let p = PeerId(Ipv4Addr::new(192, 168, 1, 20));
        let c = PeerId(Ipv4Addr::new(192, 168, 1, 30));

        s.mark_connected_for_test(p);
        s.mark_admitted_for_test(p);

        // Storico preesistente, scritto PRIMA che il candidato si presenti.
        s.handle_event(ServiceEvent::HumanSay("ciao a tutti".into()));
        let expected_history = s.history_for_test().to_vec();

        s.handle_event(ServiceEvent::PeerMsg {
            from: c,
            msg: ChatMsg::RequestAdmission { label: "cand-human".into() },
        });
        s.handle_event(ServiceEvent::PeerMsg {
            from: p,
            msg: ChatMsg::AdmissionVote { candidate: "cand-human".into(), accept: true },
        });
        // Ultimo voto atteso (l'umano-server): risolve il turno in admit.
        let effects = s.handle_event(ServiceEvent::AdmissionVoteUi {
            candidate_label: "cand-human".into(),
            accept: true,
        });

        assert!(
            effects.contains(&Effect::SendToPeer(
                c,
                ChatMsg::History { entries: expected_history.clone() }
            )),
            "il candidato appena ammesso deve ricevere il dump storico: {effects:?}"
        );
        // Consolida anche l'invariante del vecchio test (ora rimosso)
        // `server_join_sends_history_dump_only_to_new_peer`: il dump NON va
        // ri-broadcastato a P (già presente prima dell'ammissione di C) — P ha
        // già visto ogni riga passare dal vivo via il relay a stella.
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::SendToPeer(peer, ChatMsg::History { .. }) if *peer == p)),
            "un presente già ammesso NON deve ricevere il dump storico: {effects:?}"
        );
    }

    /// Un solo voto no è un VETO: basta un presente contrario a rifiutare il
    /// candidato, anche se nessun altro ha ancora votato.
    #[test]
    fn single_no_vetoes() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let p = PeerId(Ipv4Addr::new(192, 168, 1, 20));
        let c = PeerId(Ipv4Addr::new(192, 168, 1, 30));

        s.mark_connected_for_test(p);
        s.mark_admitted_for_test(p);
        s.handle_event(ServiceEvent::PeerMsg {
            from: c,
            msg: ChatMsg::RequestAdmission { label: "cand-human".into() },
        });

        let effects = s.handle_event(ServiceEvent::PeerMsg {
            from: p,
            msg: ChatMsg::AdmissionVote { candidate: "cand-human".into(), accept: false },
        });

        assert!(!s.admitted_contains_for_test(c), "un veto non deve ammettere il candidato");
        assert_eq!(s.pending_votes_len_for_test(), 0, "il voto deve essere rimosso dopo il veto");
        assert!(
            effects.contains(&Effect::SendToPeer(
                c,
                ChatMsg::AdmissionRejected { label: "cand-human".into() }
            )),
            "il candidato deve ricevere AdmissionRejected: {effects:?}"
        );
    }

    /// FIX #2 (review 2026-07-03): un VETO (`accept:false`) da un peer che NON è
    /// tra i `present` del voto NON deve risolvere il turno. Prima del fix,
    /// `record_vote` invocava `resolve_reject` senza verificare l'appartenenza a
    /// `present`: un qualunque peer connesso ma non presente (un secondo
    /// candidato, o un peer già rifiutato ancora linkato) poteva vietare
    /// l'ammissione altrui. Asimmetria che il fix chiude: un `sì` da un estraneo
    /// era già inerte (`all_yes` scorre solo `present`), ma un `no` sfuggiva al
    /// controllo. Scenario: me=.10 server, P=.20 ammesso (present={P,me}), C=.30
    /// candidato; D=.40 è connesso ma NON ammesso → non è tra i presenti.
    #[test]
    fn veto_from_non_present_peer_is_ignored() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let p = PeerId(Ipv4Addr::new(192, 168, 1, 20));
        let c = PeerId(Ipv4Addr::new(192, 168, 1, 30));
        let d = PeerId(Ipv4Addr::new(192, 168, 1, 40));

        s.mark_connected_for_test(p);
        s.mark_admitted_for_test(p);
        s.mark_connected_for_test(d); // connesso ma NON ammesso → non presente
        s.handle_event(ServiceEvent::PeerMsg {
            from: c,
            msg: ChatMsg::RequestAdmission { label: "cand-human".into() },
        });
        assert_eq!(s.pending_votes_len_for_test(), 1, "precondizione: voto avviato per C");

        // D (non presente) prova a vietare: deve essere ignorato del tutto.
        let effects = s.handle_event(ServiceEvent::PeerMsg {
            from: d,
            msg: ChatMsg::AdmissionVote { candidate: "cand-human".into(), accept: false },
        });

        assert!(effects.is_empty(), "il veto di un non-presente non deve produrre effetti: {effects:?}");
        assert_eq!(
            s.pending_votes_len_for_test(),
            1,
            "il voto deve restare aperto: un non-presente non può vietarlo"
        );
        assert!(!s.admitted_contains_for_test(c), "C non ammesso, ma nemmeno rifiutato da un estraneo");
    }

    /// FIX #5 (review 2026-07-03): dopo un rifiuto, il SERVER impone il cooldown
    /// di re-request — non si fida solo della UI. Prima del fix il cooldown di 30s
    /// viveva solo nel frontend (`retryAtMs`): un peer che mandava `RequestAdmission`
    /// via wire — o dopo aver chiuso/riaperto la finestra, che azzera `retryAtMs` —
    /// riapriva un voto a ogni tentativo, ri-promptando ogni presente. Il server ora
    /// mette il candidato rifiutato in cooldown (`StartCooldownTimer`, sempre via
    /// Effect per tenere `handle_event` PURA) e ignora i re-request finché non
    /// arriva `CooldownExpired`.
    #[test]
    fn rejected_candidate_rerequest_is_blocked_until_cooldown_expires() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let p = PeerId(Ipv4Addr::new(192, 168, 1, 20));
        let c = PeerId(Ipv4Addr::new(192, 168, 1, 30));

        s.mark_connected_for_test(p);
        s.mark_admitted_for_test(p);

        // C chiede, P veta → rifiuto: deve partire il timer di cooldown.
        s.handle_event(ServiceEvent::PeerMsg {
            from: c,
            msg: ChatMsg::RequestAdmission { label: "cand-human".into() },
        });
        let reject_effects = s.handle_event(ServiceEvent::PeerMsg {
            from: p,
            msg: ChatMsg::AdmissionVote { candidate: "cand-human".into(), accept: false },
        });
        assert!(
            reject_effects.contains(&Effect::StartCooldownTimer {
                candidate: c,
                secs: ADMISSION_REREQUEST_COOLDOWN_SECS
            }),
            "il rifiuto deve avviare il timer di cooldown del re-request: {reject_effects:?}"
        );

        // Re-request IMMEDIATO: ignorato, nessun nuovo voto.
        let too_soon = s.handle_event(ServiceEvent::PeerMsg {
            from: c,
            msg: ChatMsg::RequestAdmission { label: "cand-human".into() },
        });
        assert!(too_soon.is_empty(), "un re-request in cooldown non deve produrre effetti: {too_soon:?}");
        assert_eq!(s.pending_votes_len_for_test(), 0, "nessun nuovo voto durante il cooldown");

        // Scade il cooldown: ORA il re-request riapre il voto.
        s.handle_event(ServiceEvent::CooldownExpired { candidate: c });
        let after = s.handle_event(ServiceEvent::PeerMsg {
            from: c,
            msg: ChatMsg::RequestAdmission { label: "cand-human".into() },
        });
        assert_eq!(s.pending_votes_len_for_test(), 1, "dopo la scadenza il voto riparte");
        assert!(
            after.iter().any(|e| matches!(
                e,
                Effect::SendToPeer(peer, ChatMsg::AdmissionVoteRequest { .. }) if *peer == p
            )),
            "il re-request post-cooldown deve ri-promptare il presente P: {after:?}"
        );
    }

    /// Nessun voto arriva prima dello scadere del timer: "silenzio = sì" —
    /// il candidato viene ammesso comunque, perché nessun veto lo ha fermato.
    #[test]
    fn timeout_counts_missing_as_yes() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let p = PeerId(Ipv4Addr::new(192, 168, 1, 20));
        let c = PeerId(Ipv4Addr::new(192, 168, 1, 30));

        s.mark_connected_for_test(p);
        s.mark_admitted_for_test(p);
        s.handle_event(ServiceEvent::PeerMsg {
            from: c,
            msg: ChatMsg::RequestAdmission { label: "cand-human".into() },
        });

        // Nessun voto arriva: il timer scade e il silenzio conta come sì.
        let gen = s.pending_vote_generation_for_test(c).expect("voto in corso per C");
        let effects = s.handle_event(ServiceEvent::VoteTimeout { candidate: c, generation: gen });

        assert!(s.admitted_contains_for_test(c), "il timeout senza veto deve ammettere (silenzio = sì)");
        assert_eq!(s.pending_votes_len_for_test(), 0, "il voto deve essere rimosso dopo la risoluzione");
        assert!(
            effects.contains(&Effect::SendToPeer(c, ChatMsg::Admitted { label: "cand-human".into() })),
            "il candidato deve ricevere Admitted: {effects:?}"
        );
        assert!(
            effects.iter().any(|e| matches!(e, Effect::SendToPeer(_, ChatMsg::Roster { .. }))),
            "il roster aggiornato deve essere diffuso: {effects:?}"
        );
    }

    /// FIX #1 (review 2026-07-03): un `VoteTimeout` "stantio" — il timer di un
    /// voto GIÀ risolto — non deve risolvere un voto SUCCESSIVO per lo stesso
    /// candidato. Prima del fix `VoteTimeout` era indicizzato solo per `PeerId` e
    /// il timer non veniva mai cancellato: dopo un veto (che rimuove il voto), il
    /// cooldown, e un nuovo `RequestAdmission` (che apre un altro voto per lo
    /// stesso candidato), il vecchio timer scadeva e — trovando UNA entry per quel
    /// candidato — la risolveva in admit, scavalcando la finestra di veto del
    /// secondo turno. Il fix marca ogni voto con una GENERAZIONE monotona: il
    /// timeout la porta con sé ed è ignorato se non combacia con quella del voto
    /// attualmente in corso.
    #[test]
    fn stale_vote_timeout_does_not_resolve_a_newer_vote() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let p = PeerId(Ipv4Addr::new(192, 168, 1, 20));
        let c = PeerId(Ipv4Addr::new(192, 168, 1, 30));

        s.mark_connected_for_test(p);
        s.mark_admitted_for_test(p);

        // Voto 1: C chiede; ne leggiamo la generazione.
        s.handle_event(ServiceEvent::PeerMsg {
            from: c,
            msg: ChatMsg::RequestAdmission { label: "cand-human".into() },
        });
        let gen1 = s.pending_vote_generation_for_test(c).expect("voto 1 in corso");

        // P veta: il voto 1 è risolto (reject) e C entra in cooldown (FIX #5).
        s.handle_event(ServiceEvent::PeerMsg {
            from: p,
            msg: ChatMsg::AdmissionVote { candidate: "cand-human".into(), accept: false },
        });
        assert_eq!(s.pending_votes_len_for_test(), 0, "il veto ha risolto il voto 1");

        // Scade il cooldown, poi C ri-chiede: voto 2, nuova generazione.
        s.handle_event(ServiceEvent::CooldownExpired { candidate: c });
        s.handle_event(ServiceEvent::PeerMsg {
            from: c,
            msg: ChatMsg::RequestAdmission { label: "cand-human".into() },
        });
        let gen2 = s.pending_vote_generation_for_test(c).expect("voto 2 in corso");
        assert_ne!(gen1, gen2, "il secondo voto deve avere una generazione diversa dal primo");

        // Il timer STANTIO del voto 1 (porta gen1) scade ORA: deve essere un no-op.
        let stale = s.handle_event(ServiceEvent::VoteTimeout { candidate: c, generation: gen1 });
        assert!(stale.is_empty(), "un VoteTimeout stantio non deve produrre effetti: {stale:?}");
        assert!(!s.admitted_contains_for_test(c), "il voto 2 non deve essere risolto dal timer del voto 1");
        assert_eq!(s.pending_votes_len_for_test(), 1, "il voto 2 deve restare aperto");

        // Il timer CORRENTE (gen2) risolve invece regolarmente (silenzio = sì).
        s.handle_event(ServiceEvent::VoteTimeout { candidate: c, generation: gen2 });
        assert!(
            s.admitted_contains_for_test(c),
            "il timer del voto 2 (generazione corrente) deve risolvere"
        );
    }

    /// Un timeout che arriva DOPO che un veto ha già risolto il turno (reject) è
    /// un no-op: `pending_votes` non contiene più il candidato (rimosso dal
    /// veto), quindi il guard in `VoteTimeout` impedisce una doppia risoluzione
    /// che altrimenti ri-ammetterebbe un candidato già rifiutato.
    #[test]
    fn timeout_after_a_no_is_noop() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let p = PeerId(Ipv4Addr::new(192, 168, 1, 20));
        let c = PeerId(Ipv4Addr::new(192, 168, 1, 30));

        s.mark_connected_for_test(p);
        s.mark_admitted_for_test(p);
        s.handle_event(ServiceEvent::PeerMsg {
            from: c,
            msg: ChatMsg::RequestAdmission { label: "cand-human".into() },
        });
        s.handle_event(ServiceEvent::PeerMsg {
            from: p,
            msg: ChatMsg::AdmissionVote { candidate: "cand-human".into(), accept: false },
        });
        assert_eq!(s.pending_votes_len_for_test(), 0, "il veto deve aver già risolto il turno");

        // Il voto è già stato risolto dal veto: `pending_votes` non contiene più C,
        // quindi la generazione passata è irrilevante (il match su `get(candidate)`
        // fallisce comunque, prima ancora del confronto di generazione).
        let effects = s.handle_event(ServiceEvent::VoteTimeout { candidate: c, generation: 0 });

        assert!(effects.is_empty(), "un timeout su un voto già risolto non deve produrre effetti: {effects:?}");
        assert!(!s.admitted_contains_for_test(c), "il candidato deve restare rifiutato, non ri-ammesso");
    }

    /// Un `AdmissionVote` per un'etichetta SENZA `PendingVote` corrispondente è
    /// STALE (il turno per quel nome non è mai partito, o è già stato risolto):
    /// `record_vote` deve ignorarlo silenziosamente, senza toccare `pending_votes`
    /// né produrre effetti — normale in un sistema distribuito dove un voto può
    /// arrivare in ritardo o riferirsi a un candidato ormai deciso.
    #[test]
    fn vote_for_unknown_candidate_is_ignored() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let p = PeerId(Ipv4Addr::new(192, 168, 1, 20));

        // Nessun voto in corso: `pending_votes` è vuoto fin dall'inizio.
        assert_eq!(s.pending_votes_len_for_test(), 0);

        let effects = s.handle_event(ServiceEvent::PeerMsg {
            from: p,
            msg: ChatMsg::AdmissionVote { candidate: "nessuno-in-corso-human".into(), accept: true },
        });

        assert!(effects.is_empty(), "un voto stale non deve produrre effetti: {effects:?}");
        assert_eq!(s.pending_votes_len_for_test(), 0, "un voto stale non deve creare un PendingVote");
    }

    /// FIX #7 (review 2026-07-03): quando un voto si RISOLVE (qui tutti sì →
    /// admit), ogni PRESENTE che aveva ricevuto il gate 2 va avvisato che il turno
    /// è chiuso, così la sua UI toglie il banner "ammetti X?". L'umano-server
    /// (sempre presente) via `ToUi(AiChatAdmissionResolved)`; i presenti remoti via
    /// `ChatMsg::AdmissionResolved`. Il candidato non è tra i presenti (riceve
    /// `Admitted`, non questa notifica).
    #[test]
    fn vote_admit_notifies_presents_that_the_turn_is_resolved() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let p = PeerId(Ipv4Addr::new(192, 168, 1, 20));
        let c = PeerId(Ipv4Addr::new(192, 168, 1, 30));

        s.mark_connected_for_test(p);
        s.mark_admitted_for_test(p);
        s.handle_event(ServiceEvent::PeerMsg {
            from: c,
            msg: ChatMsg::RequestAdmission { label: "cand-human".into() },
        });
        s.handle_event(ServiceEvent::PeerMsg {
            from: p,
            msg: ChatMsg::AdmissionVote { candidate: "cand-human".into(), accept: true },
        });
        // Ultimo voto (umano-server): risolve in admit.
        let effects = s.handle_event(ServiceEvent::AdmissionVoteUi {
            candidate_label: "cand-human".into(),
            accept: true,
        });

        assert!(
            effects.contains(&Effect::ToUi(ServerMsg::AiChatAdmissionResolved {
                candidate: "cand-human".into()
            })),
            "l'umano-server deve vedere chiudersi il proprio banner del gate 2: {effects:?}"
        );
        assert!(
            effects.contains(&Effect::SendToPeer(
                p,
                ChatMsg::AdmissionResolved { candidate: "cand-human".into() }
            )),
            "il presente remoto P deve ricevere la notifica di chiusura del voto: {effects:?}"
        );
    }

    /// FIX #7: anche un RIFIUTO (veto) chiude il turno per i presenti — stessa
    /// notifica di `AiChatAdmissionResolved`. Verifica in particolare che il
    /// banner del server (che qui NON ha ancora votato) si chiuda comunque.
    #[test]
    fn vote_reject_notifies_presents_that_the_turn_is_resolved() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let p = PeerId(Ipv4Addr::new(192, 168, 1, 20));
        let c = PeerId(Ipv4Addr::new(192, 168, 1, 30));

        s.mark_connected_for_test(p);
        s.mark_admitted_for_test(p);
        s.handle_event(ServiceEvent::PeerMsg {
            from: c,
            msg: ChatMsg::RequestAdmission { label: "cand-human".into() },
        });
        // P veta → reject immediato (il server-umano non ha ancora votato).
        let effects = s.handle_event(ServiceEvent::PeerMsg {
            from: p,
            msg: ChatMsg::AdmissionVote { candidate: "cand-human".into(), accept: false },
        });

        assert!(
            effects.contains(&Effect::ToUi(ServerMsg::AiChatAdmissionResolved {
                candidate: "cand-human".into()
            })),
            "il gate 2 del server deve chiudersi anche su veto: {effects:?}"
        );
        assert!(
            effects.contains(&Effect::SendToPeer(
                p,
                ChatMsg::AdmissionResolved { candidate: "cand-human".into() }
            )),
            "il presente remoto deve sapere della chiusura del voto: {effects:?}"
        );
    }

    /// FIX #7: un presente-CLIENT che riceve `ChatMsg::AdmissionResolved` dal
    /// server deve ribaltarlo in `ToUi(AiChatAdmissionResolved)` per togliere il
    /// proprio banner del gate 2 — gemello del bridge di `AdmissionVoteRequest`.
    #[test]
    fn client_bridges_admission_resolved_to_ui() {
        let mut s = AiChatService::new_for_test(info(20, "skimble")); // .20 → client di .10
        let server_id = PeerId(Ipv4Addr::new(192, 168, 1, 10));
        s.mark_connected_for_test(server_id);

        let effects = s.handle_event(ServiceEvent::PeerMsg {
            from: server_id,
            msg: ChatMsg::AdmissionResolved { candidate: "cand-human".into() },
        });

        assert!(
            effects.contains(&Effect::ToUi(ServerMsg::AiChatAdmissionResolved {
                candidate: "cand-human".into()
            })),
            "il presente-client deve ribaltare AdmissionResolved in ToUi: {effects:?}"
        );
    }

    // -------------------------------------------------------------------------
    // Ammissione alla stanza — Task 6: il NUOVO ARRIVATO (client) — gate 1 →
    // pending → admitted/rejected (piano `2026-07-03-aichat-admission.md`).
    // Scenario comune a tutti questi test: me=.30 (IP più alto) → CLIENT del
    // server sintetico .10, allestito con `mark_connected_for_test` (stesso
    // pattern di `non_server_ignores_request_admission` sopra).
    // -------------------------------------------------------------------------

    /// Dal gate 1 mostrato (`Deciding`), l'umano accetta di entrare:
    /// `self_admission` passa a `Pending` e partono sia la richiesta al
    /// server (`ChatMsg::RequestAdmission`) sia l'avviso "in attesa" alla UI.
    #[test]
    fn join_decision_yes_sends_request_and_pending() {
        let mut s = AiChatService::new_for_test(info(30, "macavity"));
        let server_id = PeerId(Ipv4Addr::new(192, 168, 1, 10));
        s.mark_connected_for_test(server_id);
        assert_eq!(s_role(&s), Role::Client(server_id), "IP .30 non è il più basso: siamo client");
        s.set_self_admission_for_test(SelfAdmission::Deciding);

        let effects = s.handle_event(ServiceEvent::JoinDecision { accept: true });

        assert_eq!(s.self_admission_for_test(), &SelfAdmission::Pending);
        assert!(
            effects.contains(&Effect::SendToPeer(
                server_id,
                ChatMsg::RequestAdmission { label: "macavity-human".into() }
            )),
            "deve chiedere l'ammissione al server: {effects:?}"
        );
        assert!(
            effects.iter().any(|e| matches!(e, Effect::ToUi(ServerMsg::AiChatPending { .. }))),
            "deve avvisare la UI dello stato di attesa: {effects:?}"
        );
    }

    /// L'umano rifiuta il gate 1 ("non voglio entrare"): ci disconnettiamo dal
    /// server e torniamo a `NotJoining` — nessuna richiesta di ammissione parte mai.
    #[test]
    fn join_decision_no_disconnects() {
        let mut s = AiChatService::new_for_test(info(30, "macavity"));
        let server_id = PeerId(Ipv4Addr::new(192, 168, 1, 10));
        s.mark_connected_for_test(server_id);
        s.set_self_admission_for_test(SelfAdmission::Deciding);

        let effects = s.handle_event(ServiceEvent::JoinDecision { accept: false });

        assert_eq!(s.self_admission_for_test(), &SelfAdmission::NotJoining);
        assert_eq!(effects, vec![Effect::Disconnect(server_id)]);
    }

    /// `ChatMsg::Admitted` per la NOSTRA etichetta sblocca lo stato: passiamo
    /// ad `Admitted` e la UI riceve `AiChatAdmitted` (l'input si sblocca).
    #[test]
    fn admitted_for_me_sets_admitted() {
        let mut s = AiChatService::new_for_test(info(30, "macavity"));
        let server_id = PeerId(Ipv4Addr::new(192, 168, 1, 10));
        s.mark_connected_for_test(server_id);
        s.set_self_admission_for_test(SelfAdmission::Pending);

        let effects = s.handle_event(ServiceEvent::PeerMsg {
            from: server_id,
            msg: ChatMsg::Admitted { label: "macavity-human".into() },
        });

        assert_eq!(s.self_admission_for_test(), &SelfAdmission::Admitted);
        assert!(
            effects.contains(&Effect::ToUi(ServerMsg::AiChatAdmitted {})),
            "deve sbloccare la UI: {effects:?}"
        );
    }

    /// `ChatMsg::Admitted` per l'etichetta di un ALTRO candidato non deve
    /// toccare il nostro stato: non è la notifica del NOSTRO ingresso, è
    /// qualcun altro che entra (il `Roster` che segue aggiorna la sua presenza
    /// nella UI, non serve reagire qui).
    #[test]
    fn admitted_for_other_does_not_change_self() {
        let mut s = AiChatService::new_for_test(info(30, "macavity"));
        let server_id = PeerId(Ipv4Addr::new(192, 168, 1, 10));
        s.mark_connected_for_test(server_id);
        s.set_self_admission_for_test(SelfAdmission::Pending);

        s.handle_event(ServiceEvent::PeerMsg {
            from: server_id,
            msg: ChatMsg::Admitted { label: "altro-human".into() },
        });

        assert_eq!(
            s.self_admission_for_test(),
            &SelfAdmission::Pending,
            "il nostro stato non deve cambiare per l'ammissione di un altro candidato"
        );
    }

    /// `ChatMsg::AdmissionRejected` per la nostra etichetta: passiamo a
    /// `Rejected` e la UI riceve il countdown per il pulsante di re-request (30s).
    #[test]
    fn rejected_for_me_sets_rejected() {
        let mut s = AiChatService::new_for_test(info(30, "macavity"));
        let server_id = PeerId(Ipv4Addr::new(192, 168, 1, 10));
        s.mark_connected_for_test(server_id);
        s.set_self_admission_for_test(SelfAdmission::Pending);

        let effects = s.handle_event(ServiceEvent::PeerMsg {
            from: server_id,
            msg: ChatMsg::AdmissionRejected { label: "macavity-human".into() },
        });

        assert_eq!(s.self_admission_for_test(), &SelfAdmission::Rejected);
        assert!(
            effects.contains(&Effect::ToUi(ServerMsg::AiChatRejected { retry_after_secs: 30 })),
            "deve avvisare la UI del rifiuto con il cooldown: {effects:?}"
        );
    }

    /// Dopo un rifiuto, il pulsante "chiedi di entrare" (il cooldown di 30s è
    /// già stato rispettato dalla UI, che tiene il pulsante disabilitato fino
    /// allo scadere) ci riporta a `Pending`, ripetendo la richiesta al server.
    #[test]
    fn rerequest_from_rejected_goes_pending() {
        let mut s = AiChatService::new_for_test(info(30, "macavity"));
        let server_id = PeerId(Ipv4Addr::new(192, 168, 1, 10));
        s.mark_connected_for_test(server_id);
        s.set_self_admission_for_test(SelfAdmission::Rejected);

        let effects = s.handle_event(ServiceEvent::RequestAdmissionUi);

        assert_eq!(s.self_admission_for_test(), &SelfAdmission::Pending);
        assert!(
            effects.contains(&Effect::SendToPeer(
                server_id,
                ChatMsg::RequestAdmission { label: "macavity-human".into() }
            )),
            "deve ri-mandare la richiesta di ammissione: {effects:?}"
        );
    }

    /// Al connect riuscito verso il server, `run()` (il loop I/O, Task 8) invoca
    /// `begin_join_gate` al posto del vecchio invio immediato di `ChatMsg::Join`
    /// (vedi il ramo `ConnectOutcome::Success` in `run`). Qui testiamo
    /// direttamente questa logica PURA: mostra il gate 1 alla UI e passa in
    /// `Deciding`, senza mandare alcun `Join`.
    #[test]
    fn client_connect_emits_join_prompt_not_join() {
        let mut s = AiChatService::new_for_test(info(30, "macavity"));
        let server_id = PeerId(Ipv4Addr::new(192, 168, 1, 10));
        s.mark_connected_for_test(server_id);
        assert_eq!(
            s.self_admission_for_test(),
            &SelfAdmission::NotJoining,
            "prima del connect: stato iniziale di ogni servizio"
        );

        let effects = s.begin_join_gate();

        assert_eq!(s.self_admission_for_test(), &SelfAdmission::Deciding);
        assert!(
            effects.iter().any(|e| matches!(e, Effect::ToUi(ServerMsg::AiChatJoinPrompt { .. }))),
            "deve mostrare il gate 1 alla UI: {effects:?}"
        );
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::SendToPeer(_, ChatMsg::Join { .. }))),
            "NON deve più mandare il vecchio Join: {effects:?}"
        );
    }

    // -------------------------------------------------------------------------
    // Ammissione alla stanza — Task 6b (gap chiuso): il voto di un PRESENTE
    // che è un CLIENT (non l'umano-server). Prima di questo fix: (a)
    // `ChatMsg::AdmissionVoteRequest` era un no-op sul presente-client, il suo
    // umano non vedeva mai il gate 2; (b) `ServiceEvent::AdmissionVoteUi`
    // chiamava sempre `record_vote` (logica SOLO server) — su un client
    // `pending_votes` è sempre vuoto, quindi il voto veniva silenziosamente
    // perso. Risultato: solo l'umano-server poteva votare/vietare, violando
    // il modello AND-con-veto (Task 5). Vedi il piano
    // `2026-07-03-aichat-admission.md`.
    // -------------------------------------------------------------------------

    /// Un presente-CLIENT riceve `ChatMsg::AdmissionVoteRequest` dal server: deve
    /// mostrare il gate 2 alla propria UI (`ToUi(AiChatAdmissionRequest)`), esattamente
    /// come il server mostra il gate 2 alla propria UI in `start_admission_vote`. Prima
    /// del fix questo arm era no-op (vedi il TODO che sostituisce): il presente remoto
    /// non vedeva mai la richiesta di voto.
    #[test]
    fn client_receives_vote_request_forwards_to_ui() {
        let mut s = AiChatService::new_for_test(info(20, "skimble"));
        let server_id = PeerId(Ipv4Addr::new(192, 168, 1, 10));
        s.mark_connected_for_test(server_id);
        assert_eq!(s_role(&s), Role::Client(server_id), "IP .20 non è il più basso: siamo client");

        let effects = s.handle_event(ServiceEvent::PeerMsg {
            from: server_id,
            msg: ChatMsg::AdmissionVoteRequest { candidate: "cand-human".into() },
        });

        assert!(
            effects.contains(&Effect::ToUi(ServerMsg::AiChatAdmissionRequest {
                candidate: "cand-human".into()
            })),
            "il gate 2 deve arrivare alla UI del presente-client: {effects:?}"
        );
    }

    /// Il voto dell'umano-PRESENTE (via `AdmissionVoteUi`, lo stesso evento che
    /// userebbe l'umano-server) deve essere INOLTRATO al server coordinatore come
    /// `ChatMsg::AdmissionVote` — un presente-client non tiene `pending_votes` (è
    /// solo stato server, popolato da `start_admission_vote`), quindi non c'è nulla
    /// da "tallyare" localmente: il voto deve viaggiare in rete. Prima del fix,
    /// `AdmissionVoteUi` chiamava sempre `record_vote`, che su un client non trovava
    /// mai un `PendingVote` corrispondente e il voto veniva perso in silenzio.
    #[test]
    fn client_admission_vote_ui_forwards_to_server() {
        let mut s = AiChatService::new_for_test(info(20, "skimble"));
        let server_id = PeerId(Ipv4Addr::new(192, 168, 1, 10));
        s.mark_connected_for_test(server_id);
        assert_eq!(s_role(&s), Role::Client(server_id), "IP .20 non è il più basso: siamo client");

        let effects = s.handle_event(ServiceEvent::AdmissionVoteUi {
            candidate_label: "cand-human".into(),
            accept: true,
        });

        assert!(
            effects.contains(&Effect::SendToPeer(
                server_id,
                ChatMsg::AdmissionVote { candidate: "cand-human".into(), accept: true }
            )),
            "il voto del presente-client deve essere inoltrato al server: {effects:?}"
        );
        assert_eq!(
            s.pending_votes_len_for_test(),
            0,
            "un client non tiene pending_votes: il voto passa dalla rete, non da un tally locale"
        );
    }

    /// Comportamento Task 5 INVARIATO: da SERVER, `AdmissionVoteUi` deve continuare
    /// a chiamare `record_vote` (tally LOCALE) — mai un `SendToPeer(_, AdmissionVote)`,
    /// che sarebbe un messaggio di rete mandato al proprio stesso processo (non ha
    /// senso: il server non è client di se stesso). Scenario identico a
    /// `vote_all_yes_admits` sopra: dopo il voto di P (via `PeerMsg`, rete) e quello
    /// dell'umano-server (via `AdmissionVoteUi`, locale), il candidato è ammesso
    /// esattamente come prima di questo task — la ristrutturazione in `Role::Server`/
    /// `Role::Client`/`Role::Undecided` non deve cambiare questo ramo.
    #[test]
    fn server_admission_vote_ui_still_tallies_locally() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let p = PeerId(Ipv4Addr::new(192, 168, 1, 20));
        let c = PeerId(Ipv4Addr::new(192, 168, 1, 30));

        s.mark_connected_for_test(p);
        s.mark_admitted_for_test(p);
        assert_eq!(s_role(&s), Role::Server, "IP .10 è il più basso: siamo server");
        s.handle_event(ServiceEvent::PeerMsg {
            from: c,
            msg: ChatMsg::RequestAdmission { label: "cand-human".into() },
        });
        s.handle_event(ServiceEvent::PeerMsg {
            from: p,
            msg: ChatMsg::AdmissionVote { candidate: "cand-human".into(), accept: true },
        });

        // Ultimo voto atteso: quello dell'umano-server, via AdmissionVoteUi.
        let effects = s.handle_event(ServiceEvent::AdmissionVoteUi {
            candidate_label: "cand-human".into(),
            accept: true,
        });

        assert!(
            !effects.iter().any(|e| matches!(e, Effect::SendToPeer(_, ChatMsg::AdmissionVote { .. }))),
            "il voto dell'umano-server non deve MAI essere inoltrato via rete: {effects:?}"
        );
        assert!(s.admitted_contains_for_test(c), "il tally locale deve ancora ammettere dopo tutti i sì");
        assert_eq!(s.pending_votes_len_for_test(), 0, "il voto risolto deve essere rimosso da pending_votes");
    }

    // -------------------------------------------------------------------------
    // Scenario di Test per Task 4: HumanSay
    // -------------------------------------------------------------------------

    /// Costruisce uno scenario: me = .10 (IP più basso → Server), peer .20 e .30 connessi.
    ///
    /// Task 8: `Discovered` da sola basta a registrare il peer in `self.peers`
    /// (usato dall'elezione, vedi `decide_and_connect`) — il vecchio `Consent`
    /// (rimosso) non serve più per arrivare a questo stato.
    fn server_with_two_clients() -> AiChatService {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        for d in [20u8, 30] {
            s.handle_event(ServiceEvent::Discovered(info(d, &format!("m{d}")), None));
            // Simula il link TCP stabilito: inserisce il peer in `connected` e ri-elegge.
            s.mark_connected_for_test(PeerId(Ipv4Addr::new(192, 168, 1, d)));
        }
        s
    }

    /// Verifica che da SERVER `HumanSay` produca:
    /// 1. un `Effect::ToUi(AiChatMessage)` con etichetta `"{me.label_base}-human"`;
    /// 2. un `Effect::SendToPeer` per ciascun client connesso (qui 2).
    #[test]
    fn human_say_as_server_echoes_to_ui_and_sends_to_all_clients() {
        let mut s = server_with_two_clients();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<ServerMsg>();
        s.handle_event(ServiceEvent::SetServerTx(tx));
        let effects = s.handle_event(ServiceEvent::HumanSay("ciao".into()));

        // (1) Eco alla propria UI: l'umano vede subito il suo messaggio nel cursore.
        assert!(
            effects.iter().any(|e| matches!(e,
                Effect::ToUi(ServerMsg::AiChatMessage { from_label, text, .. })
                    if from_label == "skimble-human" && text == "ciao"
            )),
            "eco UI mancante: {effects:?}"
        );

        // (2) Inoltro: il server invia a TUTTI i client connessi (.20 e .30 → 2 Send).
        let sends = effects
            .iter()
            .filter(|e| matches!(e, Effect::SendToPeer(_, ChatMsg::Say { .. })))
            .count();
        assert_eq!(sends, 2, "il server dovrebbe inviare a entrambi i client: {effects:?}");
    }

    /// Verifica che da CLIENT `HumanSay` produca:
    /// 1. un `Effect::ToUi(AiChatMessage)` con eco locale;
    /// 2. esattamente un `Effect::SendToPeer` — solo verso il server (.10), non broadcast.
    #[test]
    fn human_say_as_client_sends_only_to_server() {
        // me = .30 (IP più alto) → Client del server .10 (IP più basso).
        let mut s = AiChatService::new_for_test(info(30, "macavity"));
        s.handle_event(ServiceEvent::Discovered(info(10, "skimble"), None));
        s.mark_connected_for_test(PeerId(Ipv4Addr::new(192, 168, 1, 10)));

        let effects = s.handle_event(ServiceEvent::HumanSay("ehi".into()));

        // Il client invia SOLO al server, non fa broadcast.
        let sends: Vec<_> = effects
            .iter()
            .filter(|e| matches!(e, Effect::SendToPeer(_, _)))
            .collect();
        assert_eq!(sends.len(), 1, "il client dovrebbe inviare solo al server: {effects:?}");
    }

    // -------------------------------------------------------------------------
    // AI Chat — Slice 1a: la propria AI come partecipante ("@ai", vedi
    // Docs/superpowers/specs/2026-07-02-aichat-ai-participants-design.md).
    //
    // TDD RED: prima di implementare `extract_ai_invocation`/`format_transcript`
    // (funzioni PURE, private) e le varianti `ServiceEvent::AiReply`/
    // `Effect::InvokeLocalAi`, questi test producono un errore di compilazione
    // (funzioni/varianti inesistenti) — quel compile error È la fase RED, stesso
    // pattern già usato per il `CancellationToken` in `ai_adapter.rs`.
    // -------------------------------------------------------------------------

    /// `extract_ai_invocation` pura: un messaggio senza prefisso `@ai` NON è
    /// un'invocazione — nessuna richiesta estratta.
    #[test]
    fn extract_ai_invocation_returns_none_for_plain_message() {
        assert_eq!(extract_ai_invocation("ciao a tutti"), None);
    }

    /// Il prefisso `@ai` è riconosciuto case-insensitive, e gli spazi bordo
    /// (sia prima del prefisso sia dopo) vengono rimossi dalla richiesta estratta.
    #[test]
    fn extract_ai_invocation_detects_prefix_case_insensitive() {
        assert_eq!(
            extract_ai_invocation("@AI commenta X"),
            Some(Invocation { target: InvokeTarget::Own, request: "commenta X".to_string() })
        );
        assert_eq!(
            extract_ai_invocation("  @ai   spazi bordo  "),
            Some(Invocation { target: InvokeTarget::Own, request: "spazi bordo".to_string() })
        );
    }

    /// `@ai` da solo (senza richiesta) usa un default leggibile invece di un
    /// prompt vuoto.
    #[test]
    fn extract_ai_invocation_alone_defaults_to_generic_request() {
        assert_eq!(
            extract_ai_invocation("@ai"),
            Some(Invocation {
                target: InvokeTarget::Own,
                request: "commenta la discussione".to_string(),
            })
        );
    }

    /// `@aiuto` non è `@ai` seguito da spazio/fine stringa: NON è un'invocazione
    /// (evita falsi positivi su parole che iniziano per "ai").
    #[test]
    fn extract_ai_invocation_requires_word_boundary() {
        assert_eq!(extract_ai_invocation("@aiuto qualcosa"), None);
    }

    // -------------------------------------------------------------------------
    // AI Chat — Slice 1b: nuove forme di invocazione `@all` / `@<label>-ai`
    // (vedi Docs/superpowers/specs/2026-07-02-aichat-ai-participants-design.md
    // §9.1). `extract_ai_invocation` resta PURA — nessun cambio al motivo per
    // cui è testabile senza costruire un `AiChatService`.
    // -------------------------------------------------------------------------

    /// `@all` (case-insensitive, come `@ai`) invoca TUTTE le AI presenti.
    #[test]
    fn extract_ai_invocation_detects_all() {
        assert_eq!(
            extract_ai_invocation("@all commenta X"),
            Some(Invocation { target: InvokeTarget::All, request: "commenta X".to_string() })
        );
        assert_eq!(
            extract_ai_invocation("@ALL"),
            Some(Invocation {
                target: InvokeTarget::All,
                request: "commenta la discussione".to_string(),
            })
        );
    }

    /// `@<label>-ai` (case-insensitive sul marcatore, label estratto tra `@` e
    /// `-ai`) invoca l'AI di una macchina SPECIFICA — anche remota.
    #[test]
    fn extract_ai_invocation_detects_label() {
        assert_eq!(
            extract_ai_invocation("@skimble-ai commenta X"),
            Some(Invocation {
                target: InvokeTarget::Label("skimble".to_string()),
                request: "commenta X".to_string(),
            })
        );
        assert_eq!(
            extract_ai_invocation("@Skimble-AI"),
            Some(Invocation {
                target: InvokeTarget::Label("Skimble".to_string()),
                request: "commenta la discussione".to_string(),
            })
        );
    }

    /// `@-ai` (label vuoto) NON è un'invocazione valida — nessuna macchina ha
    /// un `label_base` vuoto (vietato da `validate` in `aichat_settings.rs`).
    #[test]
    fn extract_ai_invocation_rejects_empty_label() {
        assert_eq!(extract_ai_invocation("@-ai commenta X"), None);
    }

    /// `build_ai_history` pura: una riga di un altro partecipante → un turno `user`
    /// prefissato "label: testo".
    #[test]
    fn build_ai_history_maps_other_lines_to_user_turns() {
        let history = vec![
            ChatLine { from_label: "skimble-human".into(), text: "ciao".into(), display_name: None, is_ai: false },
        ];
        let msgs = build_ai_history(&history, "rumpleteazer-ai");
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, "user");
        assert!(matches!(&msgs[0].content[0], Block::Text { text } if text == "skimble-human: ciao"));
    }

    /// Una riga PROPRIA (from_label == my_ai_label) → turno `assistant`, testo nudo
    /// (nessun prefisso label: il ruolo stesso è il marcatore d'identità). Questo È il
    /// fix strutturale della slice: senza ruolo `assistant`, le proprie righe passate
    /// sono indistinguibili da quelle altrui.
    #[test]
    fn build_ai_history_maps_own_line_to_assistant_turn_no_prefix() {
        let history = vec![
            ChatLine { from_label: "rumpleteazer-ai".into(), text: "Confermato: sono Claude.".into(), display_name: None, is_ai: false },
        ];
        let msgs = build_ai_history(&history, "rumpleteazer-ai");
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, "assistant");
        assert!(matches!(&msgs[0].content[0], Block::Text { text } if text == "Confermato: sono Claude."));
    }

    /// Righe consecutive dello STESSO ruolo (es. due righe altrui di fila, da autori
    /// diversi) vengono coalescute in UN turno solo — l'API rifiuta `[user, user]`
    /// consecutivi con HTTP 400 (stessa igiene già garantita per il cursore, vedi
    /// `ai_adapter.rs::history_stays_clean_after_error`).
    #[test]
    fn build_ai_history_coalesces_consecutive_same_role_lines() {
        let history = vec![
            ChatLine { from_label: "skimble-human".into(), text: "ciao a tutti".into(), display_name: None, is_ai: false },
            ChatLine { from_label: "skimble-ai".into(), text: "sono DeepSeek, opero su skimble.".into(), display_name: None, is_ai: false },
        ];
        let msgs = build_ai_history(&history, "rumpleteazer-ai");
        assert_eq!(msgs.len(), 1, "entrambe le righe sono 'user' dal punto di vista di rumpleteazer-ai: {msgs:?}");
        assert_eq!(msgs[0].role, "user");
        let text = match &msgs[0].content[0] {
            Block::Text { text } => text.clone(),
            other => panic!("atteso Block::Text: {other:?}"),
        };
        assert!(text.contains("skimble-human: ciao a tutti"));
        assert!(text.contains("skimble-ai: sono DeepSeek, opero su skimble."));
    }

    /// Una riga propria seguita da una riga altrui produce DUE turni distinti
    /// (assistant, poi user) — la coalescenza vale solo fra ruoli UGUALI consecutivi.
    #[test]
    fn build_ai_history_does_not_coalesce_across_different_roles() {
        let history = vec![
            ChatLine { from_label: "rumpleteazer-ai".into(), text: "Confermato: sono Claude.".into(), display_name: None, is_ai: false },
            ChatLine { from_label: "skimble-ai".into(), text: "Confermato: sono DeepSeek.".into(), display_name: None, is_ai: false },
        ];
        let msgs = build_ai_history(&history, "rumpleteazer-ai");
        assert_eq!(msgs.len(), 2, "ruoli diversi, niente coalescenza: {msgs:?}");
        assert_eq!(msgs[0].role, "assistant");
        assert_eq!(msgs[1].role, "user");
    }

    /// Storico vuoto → history vuota (nessun panico, nessun turno fantasma).
    #[test]
    fn build_ai_history_empty_history_is_empty_vec() {
        assert!(build_ai_history(&[], "rumpleteazer-ai").is_empty());
    }

    /// Invariante di sicurezza (documentata anche nel piano/spec): quando lo scenario
    /// reale che ha innescato il bug si ripresenta (un'altra AI si auto-identifica in
    /// modo errato/come provider diverso nell'ultima riga), quella riga arriva come
    /// turno `user` — MAI `assistant` — perché non porta il proprio `my_ai_label`.
    #[test]
    fn build_ai_history_never_labels_other_ai_lines_as_assistant() {
        let history = vec![
            ChatLine { from_label: "skimble-ai".into(), text: "Sono io DeepSeek, che gira su skimble.".into(), display_name: None, is_ai: false },
        ];
        let msgs = build_ai_history(&history, "rumpleteazer-ai");
        assert_eq!(msgs[0].role, "user", "la riga di un'altra AI non deve mai diventare assistant");
    }

    /// `HumanSay("@ai ...")` è un'INVOCAZIONE: oltre agli effetti normali del
    /// messaggio umano (eco UI + inoltro rete, invariati), produce IN PIÙ un
    /// `Effect::InvokeLocalAi` con la richiesta (senza il prefisso `@ai`) e il
    /// transcript della stanza — che a questo punto include GIÀ il messaggio di
    /// invocazione appena pubblicato (`publish_say` viene eseguito prima).
    #[test]
    fn at_ai_invocation_emits_invoke_local_ai() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let effects = s.handle_event(ServiceEvent::HumanSay("@ai commenta X".into()));

        // Gli effetti "normali" di HumanSay restano presenti (eco UI).
        assert!(
            effects.iter().any(|e| matches!(e,
                Effect::ToUi(ServerMsg::AiChatMessage { from_label, text, .. })
                    if from_label == "skimble-human" && text == "@ai commenta X"
            )),
            "l'invocazione resta comunque parte della conversazione (eco UI): {effects:?}"
        );

        let invoke = effects.iter().find_map(|e| match e {
            Effect::InvokeLocalAi { request, history, my_ai_label } => {
                Some((request.clone(), history.clone(), my_ai_label.clone()))
            }
            _ => None,
        });
        let (request, history, my_ai_label) =
            invoke.unwrap_or_else(|| panic!("atteso Effect::InvokeLocalAi: {effects:?}"));
        assert_eq!(request, "commenta X");
        assert_eq!(my_ai_label, "skimble-ai");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].role, "user");
        assert!(matches!(&history[0].content[0],
            Block::Text { text } if text == "skimble-human: @ai commenta X"));
    }

    /// Un messaggio normale (senza `@ai`) NON deve MAI produrre `InvokeLocalAi` —
    /// è il principio "loop-safe per costruzione": l'AI parla SOLO su invocazione
    /// umana esplicita (Slice 1a).
    #[test]
    fn plain_message_does_not_invoke_ai() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let effects = s.handle_event(ServiceEvent::HumanSay("ciao a tutti".into()));
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::InvokeLocalAi { .. })),
            "messaggio normale non deve invocare l'AI: {effects:?}"
        );
    }

    /// `ServiceEvent::AiReply` è trattato ESATTAMENTE come un `Say` da
    /// `"<label_base>-ai"`: storico locale, eco alla propria UI, e — da server —
    /// relay agli altri client connessi (stessa identica logica di `HumanSay`,
    /// riusata tramite `publish_say`).
    #[test]
    fn ai_reply_posts_say_from_ai_label() {
        let mut s = server_with_two_clients(); // me = skimble (.10), server; .20/.30 connessi
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<ServerMsg>();
        s.handle_event(ServiceEvent::SetServerTx(tx));

        let effects = s.handle_event(ServiceEvent::AiReply { text: "ecco".into() });

        assert!(
            effects.iter().any(|e| matches!(e,
                Effect::ToUi(ServerMsg::AiChatMessage { from_label, text, .. })
                    if from_label == "skimble-ai" && text == "ecco"
            )),
            "atteso ToUi(AiChatMessage) da skimble-ai: {effects:?}"
        );
        assert!(
            effects.iter().any(|e| matches!(e,
                Effect::SendToPeer(_, ChatMsg::Say { from_label, text, .. })
                    if from_label == "skimble-ai" && text == "ecco"
            )),
            "atteso relay SendToPeer(Say) da skimble-ai: {effects:?}"
        );
        assert!(
            s.history_for_test()
                .iter()
                .any(|l| l.from_label == "skimble-ai" && l.text == "ecco"),
            "storico deve contenere la riga -ai: {:?}",
            s.history_for_test()
        );
    }

    /// `AiReply` non deve MAI ri-innescare l'AI — l'arm non chiama
    /// `extract_ai_invocation`, quindi anche un testo che contiene `@ai` (caso di
    /// scuola: l'AI cita testualmente l'invocazione) non produce un secondo
    /// `InvokeLocalAi`. È ciò che rende il modello loop-safe per costruzione.
    #[test]
    fn ai_reply_does_not_reinvoke() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let effects = s.handle_event(ServiceEvent::AiReply {
            text: "@ai richiama ancora?".into(),
        });
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::InvokeLocalAi { .. })),
            "AiReply non deve mai produrre InvokeLocalAi: {effects:?}"
        );
    }

    /// Slice 2 (memoria persistente): un marker `MEMORIA: ` nel testo che la PROPRIA
    /// AI ha appena restituito produce, IN PIÙ rispetto al normale `Say` pubblicato,
    /// un `Effect::PersistMemory` con la nota estratta e il `label_base` della
    /// macchina — mai al posto della pubblicazione, sempre in aggiunta.
    #[test]
    fn ai_reply_with_marker_also_produces_persist_memory_effect() {
        let mut s = AiChatService::new_for_test(info(10, "rumpleteazer"));
        let effects = s.handle_event(ServiceEvent::AiReply {
            text: "Certo.\nMEMORIA: il companion preferisce risposte brevi.".to_string(),
        });
        let persisted = effects.iter().find_map(|e| match e {
            Effect::PersistMemory { label_base, note } => Some((label_base.clone(), note.clone())),
            _ => None,
        });
        assert_eq!(
            persisted,
            Some(("rumpleteazer".to_string(), "il companion preferisce risposte brevi.".to_string()))
        );
    }

    /// Senza marker, nessun `Effect::PersistMemory` — il caso comune (la maggior
    /// parte delle risposte dell'AI non toccano la memoria persistente).
    #[test]
    fn ai_reply_without_marker_produces_no_persist_memory_effect() {
        let mut s = AiChatService::new_for_test(info(10, "rumpleteazer"));
        let effects = s.handle_event(ServiceEvent::AiReply { text: "Solo un commento.".to_string() });
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::PersistMemory { .. })),
            "nessun marker, nessun effetto di persistenza: {effects:?}"
        );
    }

    /// `perform(Effect::InvokeLocalAi)` spawna un task che chiama
    /// `ai_adapter.chat_reply(...)` e re-inietta il risultato come
    /// `ServiceEvent::AiReply` sull'inbox — il "layer I/O" che collega l'effetto
    /// puro alla vera chiamata AI (qui lo `StubAdapter` di `new_for_test`, nessuna
    /// rete). Verifica il collegamento end-to-end perform → inbox.
    #[tokio::test]
    async fn perform_invoke_local_ai_sends_ai_reply_back_to_inbox() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let (inbox_tx, mut inbox_rx) = tokio::sync::mpsc::unbounded_channel::<ServiceEvent>();
        let (new_link_tx, _new_link_rx) = tokio::sync::mpsc::unbounded_channel();
        let (connect_res_tx, _connect_res_rx) = tokio::sync::mpsc::unbounded_channel();
        let shutdown = tokio_util::sync::CancellationToken::new();

        s.perform(
            Effect::InvokeLocalAi {
                request: "commenta".into(),
                history: vec![Message::user_text("skimble-human: @ai commenta")],
                my_ai_label: "skimble-ai".into(),
            },
            &inbox_tx,
            &new_link_tx,
            &connect_res_tx,
            &shutdown,
        )
        .await;

        let ev = inbox_rx
            .recv()
            .await
            .expect("atteso un ServiceEvent::AiReply re-iniettato sull'inbox");
        match ev {
            ServiceEvent::AiReply { text } => {
                assert!(text.contains("commenta"), "testo inatteso: {text}")
            }
            other => panic!("atteso AiReply, trovato {other:?}"),
        }
    }

    /// `perform(Effect::StartVoteTimeout)` spawna un task che aspetta `secs`
    /// secondi e poi re-inietta `ServiceEvent::VoteTimeout { candidate }`
    /// sull'inbox — il "layer I/O" che collega l'effetto puro (deciso in
    /// `handle_event` quando arriva una `RequestAdmission`, Task 5) al vero
    /// timer del sistema operativo (`tokio::time::sleep`). Verifica il
    /// collegamento end-to-end perform → sleep → inbox.
    ///
    /// `secs: 0` rende il test deterministico: lo sleep scade "subito" (al
    /// prossimo giro di poll del runtime tokio), quindi non c'è alcuna corsa
    /// con un timeout di test scelto a caso — il timeout di 2s qui sotto è
    /// solo una rete di sicurezza per non bloccare la test suite se qualcosa
    /// si rompe (l'evento non arriva mai).
    #[tokio::test]
    async fn start_vote_timeout_injects_vote_timeout_event() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let (inbox_tx, mut inbox_rx) = tokio::sync::mpsc::unbounded_channel::<ServiceEvent>();
        let (new_link_tx, _new_link_rx) = tokio::sync::mpsc::unbounded_channel();
        let (connect_res_tx, _connect_res_rx) = tokio::sync::mpsc::unbounded_channel();
        let shutdown = tokio_util::sync::CancellationToken::new();
        let candidate = PeerId(Ipv4Addr::new(192, 168, 1, 30));

        s.perform(
            // FIX #1: `perform` deve preservare la generazione re-iniettandola
            // nell'evento — verifica esplicita sotto (`got_gen == 42`).
            Effect::StartVoteTimeout { candidate, generation: 42, secs: 0 },
            &inbox_tx,
            &new_link_tx,
            &connect_res_tx,
            &shutdown,
        )
        .await;

        let ev = tokio::time::timeout(std::time::Duration::from_secs(2), inbox_rx.recv())
            .await
            .expect("il timer non ha re-iniettato VoteTimeout entro 2s")
            .expect("l'inbox si è chiusa prima di ricevere VoteTimeout");

        match ev {
            ServiceEvent::VoteTimeout { candidate: got, generation: got_gen } => {
                assert_eq!(got, candidate, "candidato inatteso: {got:?}");
                assert_eq!(got_gen, 42, "la generazione deve essere preservata dal timer");
            }
            other => panic!("atteso VoteTimeout, trovato {other:?}"),
        }
    }

    /// (FIX #5) Gemello di `start_vote_timeout_injects_vote_timeout_event`:
    /// `perform(Effect::StartCooldownTimer)` deve spawnare un task che, allo scadere
    /// del sonno, re-inietta `ServiceEvent::CooldownExpired { candidate }`
    /// sull'inbox — il "layer I/O" che collega il cooldown puro (deciso in
    /// `resolve_reject`) al vero timer del sistema operativo. `secs: 0` rende il
    /// test deterministico (lo sleep scade al prossimo giro di poll del runtime).
    #[tokio::test]
    async fn start_cooldown_timer_injects_cooldown_expired_event() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let (inbox_tx, mut inbox_rx) = tokio::sync::mpsc::unbounded_channel::<ServiceEvent>();
        let (new_link_tx, _new_link_rx) = tokio::sync::mpsc::unbounded_channel();
        let (connect_res_tx, _connect_res_rx) = tokio::sync::mpsc::unbounded_channel();
        let shutdown = tokio_util::sync::CancellationToken::new();
        let candidate = PeerId(Ipv4Addr::new(192, 168, 1, 30));

        s.perform(
            Effect::StartCooldownTimer { candidate, secs: 0 },
            &inbox_tx,
            &new_link_tx,
            &connect_res_tx,
            &shutdown,
        )
        .await;

        let ev = tokio::time::timeout(std::time::Duration::from_secs(2), inbox_rx.recv())
            .await
            .expect("il timer non ha re-iniettato CooldownExpired entro 2s")
            .expect("l'inbox si è chiusa prima di ricevere CooldownExpired");

        match ev {
            ServiceEvent::CooldownExpired { candidate: got } => {
                assert_eq!(got, candidate, "candidato inatteso: {got:?}")
            }
            other => panic!("atteso CooldownExpired, trovato {other:?}"),
        }
    }

    /// Gemello di `start_vote_timeout_injects_vote_timeout_event`/
    /// `start_cooldown_timer_injects_cooldown_expired_event` per il nuovo timer di
    /// scadenza Share (spec §9.1). `secs: 0` rende il test deterministico.
    #[tokio::test]
    async fn start_share_expiry_timer_injects_share_expiry_timeout_event() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let (inbox_tx, mut inbox_rx) = tokio::sync::mpsc::unbounded_channel::<ServiceEvent>();
        let (new_link_tx, _new_link_rx) = tokio::sync::mpsc::unbounded_channel();
        let (connect_res_tx, _connect_res_rx) = tokio::sync::mpsc::unbounded_channel();
        let shutdown = tokio_util::sync::CancellationToken::new();

        s.perform(
            Effect::StartShareExpiryTimer { share_id: "192.168.1.10:1".into(), secs: 0 },
            &inbox_tx,
            &new_link_tx,
            &connect_res_tx,
            &shutdown,
        )
        .await;

        let ev = tokio::time::timeout(std::time::Duration::from_secs(2), inbox_rx.recv())
            .await
            .expect("il timer non ha re-iniettato ShareExpiryTimeout entro 2s")
            .expect("l'inbox si è chiusa prima di ricevere ShareExpiryTimeout");

        match ev {
            ServiceEvent::ShareExpiryTimeout { share_id } => {
                assert_eq!(share_id, "192.168.1.10:1");
            }
            other => panic!("atteso ShareExpiryTimeout, trovato {other:?}"),
        }
    }

    /// Task 9: gemello di `start_vote_timeout_injects_vote_timeout_event`/
    /// `start_cooldown_timer_injects_cooldown_expired_event`/
    /// `start_share_expiry_timer_injects_share_expiry_timeout_event` per il nuovo
    /// timer di riconciliazione note (design §5.2, §9). Verifica il collegamento
    /// end-to-end `perform` → `tokio::time::sleep` → inbox: `secs: 0` rende il
    /// test deterministico (lo sleep scade al prossimo giro di poll del runtime).
    #[tokio::test]
    async fn start_notes_reconcile_timer_injects_notes_reconcile_due_event() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let (inbox_tx, mut inbox_rx) = tokio::sync::mpsc::unbounded_channel::<ServiceEvent>();
        let (new_link_tx, _new_link_rx) = tokio::sync::mpsc::unbounded_channel();
        let (connect_res_tx, _connect_res_rx) = tokio::sync::mpsc::unbounded_channel();
        let shutdown = tokio_util::sync::CancellationToken::new();
        let peer = PeerId(Ipv4Addr::new(192, 168, 1, 20));

        s.perform(
            Effect::StartNotesReconcileTimer { peer_id: peer, generation: 7, secs: 0 },
            &inbox_tx,
            &new_link_tx,
            &connect_res_tx,
            &shutdown,
        )
        .await;

        let ev = tokio::time::timeout(std::time::Duration::from_secs(2), inbox_rx.recv())
            .await
            .expect("il timer non ha re-iniettato NotesReconcileDue entro 2s")
            .expect("l'inbox si è chiusa prima di ricevere NotesReconcileDue");

        match ev {
            ServiceEvent::NotesReconcileDue { peer_id: got, generation: got_gen } => {
                assert_eq!(got, peer, "peer inatteso: {got:?}");
                assert_eq!(got_gen, 7, "la generazione deve essere preservata dal timer");
            }
            other => panic!("atteso NotesReconcileDue, trovato {other:?}"),
        }
    }

    /// `ShareDocumentRequested` con un target conosciuto deve: generare un `share_id`,
    /// registrarlo in `outgoing_shares`, e mandare `ChatMsg::ShareOffer` al peer risolto —
    /// `from_label` nel wire message è il `label_base` NUDO del mittente (non "-human").
    #[test]
    fn share_document_requested_sends_offer_to_known_target() {
        let mut s = AiChatService::new_for_test(info(10, "rumpleteazer"));
        s.handle_event(ServiceEvent::Discovered(info(20, "skimble"), None));
        // Review 2026-07-05: dopo il fix di `request_share`, un target "conosciuto" deve
        // anche essere RAGGIUNGIBILE (in `self.links`), non solo scoperto. `mark_connected_
        // for_test` lo simula, ma sovrascrive la `PeerInfo` con una sintetica ("peer-<ip>")
        // — il secondo `Discovered` ripristina la label reale "skimble" senza toccare il
        // link appena registrato (stesso pattern già usato altrove in questo file).
        s.mark_connected_for_test(PeerId(Ipv4Addr::new(192, 168, 1, 20)));
        s.handle_event(ServiceEvent::Discovered(info(20, "skimble"), None));

        let effects = s.handle_event(ServiceEvent::ShareDocumentRequested {
            rel_path: "notes.md".into(),
            doc_name: "notes.md".into(),
            size_bytes: 1234,
            target: protocol::ShareTarget::One { label_base: "skimble".into() },
        });

        let sent = effects.iter().find_map(|e| match e {
            Effect::SendToPeer(peer, ChatMsg::ShareOffer { share_id, from_label, doc_name, size_bytes })
                if *peer == PeerId(Ipv4Addr::new(192, 168, 1, 20)) =>
            {
                Some((share_id.clone(), from_label.clone(), doc_name.clone(), *size_bytes))
            }
            _ => None,
        });
        let (share_id, from_label, doc_name, size_bytes) =
            sent.expect(&format!("deve mandare ShareOffer a skimble: {effects:?}"));
        assert_eq!(from_label, "rumpleteazer", "from_label deve essere il label_base nudo, non '-human'");
        assert_eq!(doc_name, "notes.md");
        assert_eq!(size_bytes, 1234);
        assert_eq!(
            s.outgoing_shares_for_test().get(&share_id).cloned(),
            Some(OutgoingShare {
                target_label: "skimble".to_string(),
                doc_name: "notes.md".to_string(),
                rel_path: "notes.md".to_string(),
                stage: OutgoingShareStage::AwaitingAccept,
            })
        );
    }

    /// Target sconosciuto (non scoperto, o coincide con se stessi — vedi il test dedicato
    /// in `relay.rs`): nessun `SendToPeer`, esito immediato `Failed` per la propria UI.
    #[test]
    fn share_document_requested_fails_immediately_for_unknown_target() {
        let mut s = AiChatService::new_for_test(info(10, "rumpleteazer"));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ServerMsg>();
        s.handle_event(ServiceEvent::SetServerTx(tx));

        let effects = s.handle_event(ServiceEvent::ShareDocumentRequested {
            rel_path: "notes.md".into(),
            doc_name: "notes.md".into(),
            size_bytes: 1234,
            target: protocol::ShareTarget::One { label_base: "nobody".into() },
        });

        assert!(
            !effects.iter().any(|e| matches!(e, Effect::SendToPeer(_, ChatMsg::ShareOffer { .. }))),
            "non deve mandare nessuna Offer per un target sconosciuto: {effects:?}"
        );
        assert!(
            effects.iter().any(|e| matches!(
                e,
                Effect::ToUi(ServerMsg::ShareResult { outcome: protocol::ShareOutcome::Failed { .. }, .. })
            )),
            "deve riportare Failed subito: {effects:?}"
        );
        drop(rx.try_recv()); // silenzia l'unused warning su `rx` se il test non lo consuma altrove
    }

    /// `ShareTarget::All` non è ancora implementato in questa slice (Slice 3) — esito
    /// immediato `Failed` con un motivo leggibile, non un panic/match mancante.
    #[test]
    fn share_document_requested_with_target_all_fails_not_yet_implemented() {
        let mut s = AiChatService::new_for_test(info(10, "rumpleteazer"));
        let effects = s.handle_event(ServiceEvent::ShareDocumentRequested {
            rel_path: "notes.md".into(),
            doc_name: "notes.md".into(),
            size_bytes: 1234,
            target: protocol::ShareTarget::All,
        });
        assert!(effects.iter().any(|e| matches!(
            e,
            Effect::ToUi(ServerMsg::ShareResult { outcome: protocol::ShareOutcome::Failed { .. }, .. })
        )));
    }

    /// Cap di dimensione (spec §8, §3.3): un documento sopra `MAX_SHARE_SIZE_BYTES` non
    /// viene mai offerto — esito `Failed` immediato, nessun `SendToPeer`. Verificato QUI
    /// lato mittente anche se la vera fonte di verità (leggere il file) è la UI di una
    /// slice futura: `ServiceEvent::ShareDocumentRequested` può ricevere qualunque
    /// `size_bytes` (test-costruito o, più avanti, da una UI con un bug) e non deve
    /// fidarsi ciecamente — stessa filosofia "verifica ad ogni confine" già in uso nel
    /// resto del file (es. il cap sui trasferimenti pendenti).
    #[test]
    fn share_document_requested_over_size_cap_fails_immediately() {
        let mut s = AiChatService::new_for_test(info(10, "rumpleteazer"));
        s.handle_event(ServiceEvent::Discovered(info(20, "skimble"), None));

        let effects = s.handle_event(ServiceEvent::ShareDocumentRequested {
            rel_path: "big.md".into(),
            doc_name: "big.md".into(),
            size_bytes: MAX_SHARE_SIZE_BYTES + 1,
            target: protocol::ShareTarget::One { label_base: "skimble".into() },
        });

        assert!(
            !effects.iter().any(|e| matches!(e, Effect::SendToPeer(_, ChatMsg::ShareOffer { .. }))),
            "non deve mandare un'Offer sopra il cap: {effects:?}"
        );
        assert!(effects.iter().any(|e| matches!(
            e,
            Effect::ToUi(ServerMsg::ShareResult { outcome: protocol::ShareOutcome::Failed { .. }, .. })
        )));
    }

    /// Review finale (2026-07-05), bug Important: `route_to_label` risolve un target
    /// dalla mera *scoperta* UDP (`self.peers`), non dalla raggiungibilità TCP
    /// (`self.links`). In una topologia a stella con 3+ macchine un CLIENT può aver
    /// scoperto un altro CLIENT senza avere alcun link diretto con lui (solo il server
    /// eletto ha link con tutti). Qui il target è SOLO scoperto (niente
    /// `mark_connected_for_test`, che popolerebbe anche `links`): `request_share` deve
    /// accorgersene PRIMA di emettere `SendToPeer` — altrimenti l'esecutore generico di
    /// `Effect::SendToPeer` in `perform` scarterebbe il messaggio in silenzio (nessun
    /// link trovato) lasciando un'entry orfana in `outgoing_shares` e nessun feedback
    /// alla UI.
    #[test]
    fn share_document_requested_fails_when_target_discovered_but_not_linked() {
        let mut s = AiChatService::new_for_test(info(10, "rumpleteazer"));
        // Solo scoperto (Discovered), MAI `mark_connected_for_test`: è esattamente il
        // caso che il fix deve intercettare — niente link registrato per questo peer.
        s.handle_event(ServiceEvent::Discovered(info(20, "skimble"), None));

        let effects = s.handle_event(ServiceEvent::ShareDocumentRequested {
            rel_path: "notes.md".into(),
            doc_name: "notes.md".into(),
            size_bytes: 1234,
            target: protocol::ShareTarget::One { label_base: "skimble".into() },
        });

        let share_id = match effects.as_slice() {
            [Effect::ToUi(ServerMsg::ShareResult { share_id, target_label, outcome, .. })] => {
                assert_eq!(target_label, "skimble");
                match outcome {
                    protocol::ShareOutcome::Failed { reason } => {
                        assert_eq!(reason, "macchina non trovata o non connessa");
                    }
                    other => panic!("atteso Failed, trovato {other:?}"),
                }
                share_id.clone()
            }
            other => panic!("atteso esattamente un Effect::ToUi(ShareResult Failed): {other:?}"),
        };
        assert!(
            !s.outgoing_shares_for_test().contains_key(&share_id),
            "non deve restare un'entry orfana in outgoing_shares: {:?}",
            s.outgoing_shares_for_test()
        );
    }

    /// Un `ShareOffer` in arrivo: registra `pending_shares`, avvia il timer di scadenza,
    /// e mostra il banner alla UI SE connessa.
    #[test]
    fn peer_share_offer_registers_pending_and_shows_request_when_ui_connected() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ServerMsg>();
        s.handle_event(ServiceEvent::SetServerTx(tx));

        let from = PeerId(Ipv4Addr::new(192, 168, 1, 26));
        let effects = s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::ShareOffer {
                share_id: "192.168.1.26:0".into(),
                from_label: "rumpleteazer".into(),
                doc_name: "notes.md".into(),
                size_bytes: 1234,
            },
        });

        assert!(
            effects.iter().any(|e| matches!(e, Effect::StartShareExpiryTimer { share_id, secs }
                if share_id == "192.168.1.26:0" && *secs == SHARE_EXPIRY_SECS)),
            "deve avviare il timer di scadenza 24h: {effects:?}"
        );
        assert!(
            effects.iter().any(|e| matches!(e, Effect::ToUi(ServerMsg::ShareRequest { share_id, from_label, doc_name, size_bytes })
                if share_id == "192.168.1.26:0" && from_label == "rumpleteazer" && doc_name == "notes.md" && *size_bytes == 1234)),
            "deve mostrare il banner alla UI connessa: {effects:?}"
        );
        assert!(s.pending_shares_for_test().contains_key("192.168.1.26:0"));
        drop(rx.try_recv());
    }

    /// Stessa offerta, ma UI NON connessa: nessun `ToUi`, ma l'entry resta in
    /// `pending_shares` per il replay futuro (Task 7).
    #[test]
    fn peer_share_offer_registers_pending_without_ui_no_banner_yet() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let from = PeerId(Ipv4Addr::new(192, 168, 1, 26));
        let effects = s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::ShareOffer {
                share_id: "192.168.1.26:0".into(),
                from_label: "rumpleteazer".into(),
                doc_name: "notes.md".into(),
                size_bytes: 1234,
            },
        });
        assert!(!effects.iter().any(|e| matches!(e, Effect::ToUi(ServerMsg::ShareRequest { .. }))));
        assert!(s.pending_shares_for_test().contains_key("192.168.1.26:0"));
    }

    /// Cap raggiunto (spec §8): la nuova offerta è auto-rifiutata, nessuna entry aggiunta,
    /// nessun timer avviato.
    #[test]
    fn peer_share_offer_auto_rejects_when_cap_reached() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        s.fill_pending_shares_to_cap_for_test();
        // Registra il mittente in `self.peers` (come farebbe una `Discovered` UDP reale
        // arrivata prima dell'Offer): `auto_reject_share` risolve la destinazione del
        // `ShareReject` per `from_label` via `route_to_label`, non per `PeerId` di
        // trasporto — senza questa entry non saprebbe a chi rispondere (deviazione
        // rispetto al brief originale del Task 6, che ometteva questo setup — vedi report).
        s.handle_event(ServiceEvent::Discovered(info(26, "rumpleteazer"), None));
        let from = PeerId(Ipv4Addr::new(192, 168, 1, 26));
        let effects = s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::ShareOffer {
                share_id: "192.168.1.26:99".into(),
                from_label: "rumpleteazer".into(),
                doc_name: "notes.md".into(),
                size_bytes: 1234,
            },
        });
        assert!(
            effects.iter().any(|e| matches!(e, Effect::SendToPeer(p, ChatMsg::ShareReject { share_id, .. })
                if *p == from && share_id == "192.168.1.26:99")),
            "deve auto-rifiutare al cap: {effects:?}"
        );
        assert!(!s.pending_shares_for_test().contains_key("192.168.1.26:99"));
    }

    /// Cap di dimensione (spec §8): un'offerta che dichiara `size_bytes` sopra
    /// `MAX_SHARE_SIZE_BYTES` è auto-rifiutata dal DESTINATARIO — un mittente compromesso
    /// o buggato potrebbe mentire, quindi il controllo lato mittente (Task 5) da solo non
    /// basta. Nessuna entry in `pending_shares`, nessun timer, nessun banner.
    #[test]
    fn peer_share_offer_over_size_cap_auto_rejects() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        // Vedi il commento gemello in `peer_share_offer_auto_rejects_when_cap_reached`.
        s.handle_event(ServiceEvent::Discovered(info(26, "rumpleteazer"), None));
        let from = PeerId(Ipv4Addr::new(192, 168, 1, 26));
        let effects = s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::ShareOffer {
                share_id: "192.168.1.26:0".into(),
                from_label: "rumpleteazer".into(),
                doc_name: "big.md".into(),
                size_bytes: MAX_SHARE_SIZE_BYTES + 1,
            },
        });
        assert!(
            effects.iter().any(|e| matches!(e, Effect::SendToPeer(p, ChatMsg::ShareReject { share_id, .. })
                if *p == from && share_id == "192.168.1.26:0")),
            "deve auto-rifiutare sopra il cap dimensione: {effects:?}"
        );
        assert!(!effects.iter().any(|e| matches!(e, Effect::StartShareExpiryTimer { .. })));
        assert!(!s.pending_shares_for_test().contains_key("192.168.1.26:0"));
    }

    /// Accetto un'offerta pendente: manda `ShareAccept` al mittente originale (risolto
    /// per `from_label` via `route_to_label`). Fix di integrità del consenso (review
    /// finale Slice 2a): l'entry transiziona esplicitamente ad `AwaitingContent`, in
    /// attesa che il contenuto vero arrivi con `ChatMsg::ShareData` — non resta più
    /// `AwaitingDecision` invariata.
    #[test]
    fn share_consent_accept_sends_share_accept_and_transitions_to_awaiting_content() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        s.handle_event(ServiceEvent::Discovered(info(26, "rumpleteazer"), None));
        let from = PeerId(Ipv4Addr::new(192, 168, 1, 26));
        s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::ShareOffer {
                share_id: "192.168.1.26:0".into(),
                from_label: "rumpleteazer".into(),
                doc_name: "notes.md".into(),
                size_bytes: 1234,
            },
        });

        let effects = s.handle_event(ServiceEvent::ShareConsentUi {
            share_id: "192.168.1.26:0".into(),
            accept: true,
        });

        assert!(
            effects.iter().any(|e| matches!(e, Effect::SendToPeer(p, ChatMsg::ShareAccept { share_id, from_label })
                if *p == from && share_id == "192.168.1.26:0" && from_label == "skimble")),
            "deve mandare ShareAccept al mittente: {effects:?}"
        );
        assert_eq!(
            s.pending_shares_for_test().get("192.168.1.26:0"),
            Some(&PendingShare::AwaitingContent {
                from_label: "rumpleteazer".into(),
                doc_name: "notes.md".into(),
                size_bytes: 1234,
            }),
            "l'entry deve transizionare ad AwaitingContent, non restare AwaitingDecision"
        );
    }

    /// Rifiuto un'offerta pendente: manda `ShareReject`, rimuove da `pending_shares`.
    #[test]
    fn share_consent_reject_sends_share_reject_and_clears_pending() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        s.handle_event(ServiceEvent::Discovered(info(26, "rumpleteazer"), None));
        let from = PeerId(Ipv4Addr::new(192, 168, 1, 26));
        s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::ShareOffer {
                share_id: "192.168.1.26:0".into(),
                from_label: "rumpleteazer".into(),
                doc_name: "notes.md".into(),
                size_bytes: 1234,
            },
        });

        let effects = s.handle_event(ServiceEvent::ShareConsentUi {
            share_id: "192.168.1.26:0".into(),
            accept: false,
        });

        assert!(effects.iter().any(|e| matches!(e, Effect::SendToPeer(p, ChatMsg::ShareReject { share_id, .. })
            if *p == from && share_id == "192.168.1.26:0")));
        assert!(!s.pending_shares_for_test().contains_key("192.168.1.26:0"));
    }

    /// Il mittente originale di un'offerta pendente non è (più) risolvibile in
    /// `self.peers` (es. si è disconnesso prima che l'umano decidesse) — a differenza
    /// del test sopra, qui NON chiamiamo `ServiceEvent::Discovered` per "rumpleteazer":
    /// `route_to_label` in `share_consent` ritorna quindi `None`. Nessun `Effect::SendToPeer`
    /// (best-effort, coerente con `auto_reject_share`). Fix di integrità del consenso
    /// (review finale Slice 2a): l'entry transiziona comunque ad `AwaitingContent` —
    /// l'irraggiungibilità del mittente non cambia il fatto che l'accettazione registra
    /// sempre la mia decisione localmente prima di aspettare `ShareData`.
    #[test]
    fn share_consent_accept_with_unresolvable_sender_transitions_to_awaiting_content_no_effect() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        // NIENTE `Discovered`: "rumpleteazer" non è in `self.peers` quando arriva la
        // decisione — solo `pending_shares` la registra (via `handle_share_offer`, che
        // non consulta affatto `self.peers`).
        let from = PeerId(Ipv4Addr::new(192, 168, 1, 26));
        s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::ShareOffer {
                share_id: "192.168.1.26:0".into(),
                from_label: "rumpleteazer".into(),
                doc_name: "notes.md".into(),
                size_bytes: 1234,
            },
        });
        assert!(
            s.pending_shares_for_test().contains_key("192.168.1.26:0"),
            "precondizione: l'offerta deve essere pendente prima del consenso"
        );

        let effects = s.handle_event(ServiceEvent::ShareConsentUi {
            share_id: "192.168.1.26:0".into(),
            accept: true,
        });

        assert!(
            effects.is_empty(),
            "mittente non risolvibile: nessun SendToPeer, best-effort: {effects:?}"
        );
        assert_eq!(
            s.pending_shares_for_test().get("192.168.1.26:0"),
            Some(&PendingShare::AwaitingContent {
                from_label: "rumpleteazer".into(),
                doc_name: "notes.md".into(),
                size_bytes: 1234,
            }),
            "l'entry transiziona ad AwaitingContent comunque — l'irraggiungibilità del mittente non c'entra"
        );
    }

    /// Un `ShareConsentUi` per uno `share_id` non (più) pendente (già scaduto, già deciso,
    /// mai esistito) è un no-op — stesso trattamento difensivo del voto di ammissione stale.
    #[test]
    fn share_consent_for_unknown_share_id_is_noop() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let effects = s.handle_event(ServiceEvent::ShareConsentUi { share_id: "ghost".into(), accept: true });
        assert!(effects.is_empty(), "un consenso per uno share_id stale non deve produrre effetti: {effects:?}");
    }

    /// (Mittente) Ricevo `ShareAccept`: riporto `ShareResult::Accepted` alla mia UI.
    /// Slice 2a: l'entry NON viene più rimossa — transiziona ad `AwaitingContent`, in
    /// attesa che la UI fornisca il contenuto vero (`ChatMsg::ShareData` non è ancora
    /// stato mandato a questo punto).
    #[test]
    fn peer_share_accept_reports_accepted_result_and_keeps_entry_awaiting_content() {
        let mut s = AiChatService::new_for_test(info(10, "rumpleteazer"));
        s.handle_event(ServiceEvent::Discovered(info(20, "skimble"), None));
        // Review 2026-07-05: dopo il fix di `request_share`, un target "conosciuto" deve
        // anche essere RAGGIUNGIBILE (in `self.links`), non solo scoperto. `mark_connected_
        // for_test` lo simula, ma sovrascrive la `PeerInfo` con una sintetica ("peer-<ip>")
        // — il secondo `Discovered` ripristina la label reale "skimble" senza toccare il
        // link appena registrato (stesso pattern già usato altrove in questo file).
        s.mark_connected_for_test(PeerId(Ipv4Addr::new(192, 168, 1, 20)));
        s.handle_event(ServiceEvent::Discovered(info(20, "skimble"), None));
        let effects = s.handle_event(ServiceEvent::ShareDocumentRequested {
            rel_path: "notes.md".into(),
            doc_name: "notes.md".into(),
            size_bytes: 1234,
            target: protocol::ShareTarget::One { label_base: "skimble".into() },
        });
        let share_id = effects.iter().find_map(|e| match e {
            Effect::SendToPeer(_, ChatMsg::ShareOffer { share_id, .. }) => Some(share_id.clone()),
            _ => None,
        }).expect("deve aver mandato un'Offer");

        let from = PeerId(Ipv4Addr::new(192, 168, 1, 20));
        let result_effects = s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::ShareAccept { share_id: share_id.clone(), from_label: "skimble".into() },
        });

        assert!(result_effects.iter().any(|e| matches!(e,
            Effect::ToUi(ServerMsg::ShareResult { share_id: sid, target_label, doc_name, outcome: protocol::ShareOutcome::Accepted })
            if *sid == share_id && target_label == "skimble" && doc_name == "notes.md"
        )), "{result_effects:?}");
        assert_eq!(
            s.outgoing_shares_for_test().get(&share_id).map(|o| &o.stage),
            Some(&OutgoingShareStage::AwaitingContent),
            "l'entry deve restare in attesa del contenuto vero, non essere rimossa"
        );
    }

    /// Stesso scenario con `ShareReject`.
    #[test]
    fn peer_share_reject_reports_rejected_result() {
        let mut s = AiChatService::new_for_test(info(10, "rumpleteazer"));
        s.handle_event(ServiceEvent::Discovered(info(20, "skimble"), None));
        // Review 2026-07-05: dopo il fix di `request_share`, un target "conosciuto" deve
        // anche essere RAGGIUNGIBILE (in `self.links`), non solo scoperto. `mark_connected_
        // for_test` lo simula, ma sovrascrive la `PeerInfo` con una sintetica ("peer-<ip>")
        // — il secondo `Discovered` ripristina la label reale "skimble" senza toccare il
        // link appena registrato (stesso pattern già usato altrove in questo file).
        s.mark_connected_for_test(PeerId(Ipv4Addr::new(192, 168, 1, 20)));
        s.handle_event(ServiceEvent::Discovered(info(20, "skimble"), None));
        let effects = s.handle_event(ServiceEvent::ShareDocumentRequested {
            rel_path: "notes.md".into(),
            doc_name: "notes.md".into(),
            size_bytes: 1234,
            target: protocol::ShareTarget::One { label_base: "skimble".into() },
        });
        let share_id = effects.iter().find_map(|e| match e {
            Effect::SendToPeer(_, ChatMsg::ShareOffer { share_id, .. }) => Some(share_id.clone()),
            _ => None,
        }).unwrap();

        let from = PeerId(Ipv4Addr::new(192, 168, 1, 20));
        let result_effects = s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::ShareReject { share_id: share_id.clone(), from_label: "skimble".into() },
        });

        assert!(result_effects.iter().any(|e| matches!(e,
            Effect::ToUi(ServerMsg::ShareResult { outcome: protocol::ShareOutcome::Rejected, .. })
        )), "{result_effects:?}");
        assert!(!s.outgoing_shares_for_test().contains_key(&share_id));
    }

    /// (Mittente) Ricevo `ShareExpired`: riporto `ShareResult::Failed` alla mia UI.
    #[test]
    fn peer_share_expired_reports_failed_result() {
        let mut s = AiChatService::new_for_test(info(10, "rumpleteazer"));
        s.handle_event(ServiceEvent::Discovered(info(20, "skimble"), None));
        // Review 2026-07-05: dopo il fix di `request_share`, un target "conosciuto" deve
        // anche essere RAGGIUNGIBILE (in `self.links`), non solo scoperto. `mark_connected_
        // for_test` lo simula, ma sovrascrive la `PeerInfo` con una sintetica ("peer-<ip>")
        // — il secondo `Discovered` ripristina la label reale "skimble" senza toccare il
        // link appena registrato (stesso pattern già usato altrove in questo file).
        s.mark_connected_for_test(PeerId(Ipv4Addr::new(192, 168, 1, 20)));
        s.handle_event(ServiceEvent::Discovered(info(20, "skimble"), None));
        let effects = s.handle_event(ServiceEvent::ShareDocumentRequested {
            rel_path: "notes.md".into(),
            doc_name: "notes.md".into(),
            size_bytes: 1234,
            target: protocol::ShareTarget::One { label_base: "skimble".into() },
        });
        let share_id = effects.iter().find_map(|e| match e {
            Effect::SendToPeer(_, ChatMsg::ShareOffer { share_id, .. }) => Some(share_id.clone()),
            _ => None,
        }).unwrap();

        let from = PeerId(Ipv4Addr::new(192, 168, 1, 20));
        let result_effects = s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::ShareExpired { share_id: share_id.clone() },
        });

        assert!(result_effects.iter().any(|e| matches!(e,
            Effect::ToUi(ServerMsg::ShareResult { outcome: protocol::ShareOutcome::Failed { .. }, .. })
        )), "{result_effects:?}");
        assert!(!s.outgoing_shares_for_test().contains_key(&share_id));
    }

    /// Un `ChatMsg::ShareAccept` transiziona `outgoing_shares` ad `AwaitingContent`,
    /// riporta SEMPRE `Accepted` alla UI (invariato rispetto a prima di questa slice) e,
    /// se la UI è connessa, chiede subito il contenuto (spec Slice 2a §3).
    #[test]
    fn share_accept_transitions_to_awaiting_content_and_requests_content() {
        let mut s = AiChatService::new_for_test(info(10, "rumpleteazer"));
        s.handle_event(ServiceEvent::Discovered(info(20, "skimble"), None));
        s.mark_connected_for_test(PeerId(Ipv4Addr::new(192, 168, 1, 20)));
        s.handle_event(ServiceEvent::Discovered(info(20, "skimble"), None));
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<ServerMsg>();
        s.handle_event(ServiceEvent::SetServerTx(tx));

        let effects = s.handle_event(ServiceEvent::ShareDocumentRequested {
            rel_path: "notes.md".into(),
            doc_name: "notes.md".into(),
            size_bytes: 100,
            target: protocol::ShareTarget::One { label_base: "skimble".into() },
        });
        let share_id = effects.iter().find_map(|e| match e {
            Effect::SendToPeer(_, ChatMsg::ShareOffer { share_id, .. }) => Some(share_id.clone()),
            _ => None,
        }).expect("deve aver mandato un'Offer");

        let from = PeerId(Ipv4Addr::new(192, 168, 1, 20));
        let effects = s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::ShareAccept { share_id: share_id.clone(), from_label: "skimble".into() },
        });

        assert!(
            effects.iter().any(|e| matches!(
                e,
                Effect::ToUi(ServerMsg::ShareResult { outcome: protocol::ShareOutcome::Accepted, .. })
            )),
            "atteso ShareResult{{Accepted}}: {effects:?}"
        );
        assert!(
            effects.iter().any(|e| matches!(
                e,
                Effect::ToUi(ServerMsg::ShareContentRequest { share_id: sid, rel_path })
                    if *sid == share_id && rel_path == "notes.md"
            )),
            "atteso ShareContentRequest: {effects:?}"
        );
        assert_eq!(
            s.outgoing_shares_for_test().get(&share_id).map(|o| &o.stage),
            Some(&OutgoingShareStage::AwaitingContent)
        );
    }

    /// Stesso `ShareAccept`, ma senza UI connessa: transizione avviene comunque
    /// (e `Accepted` viene comunque riportato — l'`Effect::ToUi` resta nella lista anche
    /// se nessuno lo consumerà finché la UI non si connette, stesso trattamento già
    /// riservato altrove a `Effect::ToUi` con UI assente), nessuna `ShareContentRequest`
    /// immediata.
    #[test]
    fn share_accept_without_ui_does_not_request_content_immediately() {
        let mut s = AiChatService::new_for_test(info(10, "rumpleteazer"));
        s.handle_event(ServiceEvent::Discovered(info(20, "skimble"), None));
        s.mark_connected_for_test(PeerId(Ipv4Addr::new(192, 168, 1, 20)));
        s.handle_event(ServiceEvent::Discovered(info(20, "skimble"), None));

        let effects = s.handle_event(ServiceEvent::ShareDocumentRequested {
            rel_path: "notes.md".into(),
            doc_name: "notes.md".into(),
            size_bytes: 100,
            target: protocol::ShareTarget::One { label_base: "skimble".into() },
        });
        let share_id = effects.iter().find_map(|e| match e {
            Effect::SendToPeer(_, ChatMsg::ShareOffer { share_id, .. }) => Some(share_id.clone()),
            _ => None,
        }).expect("deve aver mandato un'Offer");

        let from = PeerId(Ipv4Addr::new(192, 168, 1, 20));
        let effects = s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::ShareAccept { share_id: share_id.clone(), from_label: "skimble".into() },
        });

        assert!(
            !effects.iter().any(|e| matches!(e, Effect::ToUi(ServerMsg::ShareContentRequest { .. }))),
            "senza UI connessa non deve chiedere il contenuto subito: {effects:?}"
        );
        assert_eq!(
            s.outgoing_shares_for_test().get(&share_id).map(|o| &o.stage),
            Some(&OutgoingShareStage::AwaitingContent)
        );
    }

    /// Riapertura finestra con una condivisione in uscita ancora `AwaitingContent`:
    /// `SetServerTx` deve ri-emettere `ShareContentRequest` — stesso principio del replay
    /// di `ShareRequest` per `pending_shares` (vedi `set_server_tx_replays_share_request_for_pending_offer`).
    /// Copre lo stesso gap di replay di quel test, ma sul lato MITTENTE: senza questo, una
    /// condivisione accettata mentre la finestra era chiusa non chiederebbe MAI il
    /// contenuto, nemmeno riaprendo la finestra più tardi.
    #[test]
    fn set_server_tx_replays_share_content_request_for_awaiting_content() {
        let mut s = AiChatService::new_for_test(info(10, "rumpleteazer"));
        s.handle_event(ServiceEvent::Discovered(info(20, "skimble"), None));
        s.mark_connected_for_test(PeerId(Ipv4Addr::new(192, 168, 1, 20)));
        s.handle_event(ServiceEvent::Discovered(info(20, "skimble"), None));

        // Nessuna UI connessa: la condivisione viene accettata a finestra chiusa,
        // esattamente come in `share_accept_without_ui_does_not_request_content_immediately`.
        let effects = s.handle_event(ServiceEvent::ShareDocumentRequested {
            rel_path: "notes.md".into(),
            doc_name: "notes.md".into(),
            size_bytes: 100,
            target: protocol::ShareTarget::One { label_base: "skimble".into() },
        });
        let share_id = effects.iter().find_map(|e| match e {
            Effect::SendToPeer(_, ChatMsg::ShareOffer { share_id, .. }) => Some(share_id.clone()),
            _ => None,
        }).expect("deve aver mandato un'Offer");

        let from = PeerId(Ipv4Addr::new(192, 168, 1, 20));
        s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::ShareAccept { share_id: share_id.clone(), from_label: "skimble".into() },
        });

        // La finestra si apre ORA, dopo l'accettazione: il replay deve chiedere il
        // contenuto che l'`Effect::ToUi` immediato non ha potuto consegnare a nessuno.
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<ServerMsg>();
        let effects = s.handle_event(ServiceEvent::SetServerTx(tx));

        assert!(
            effects.iter().any(|e| matches!(
                e,
                Effect::ToUi(ServerMsg::ShareContentRequest { share_id: sid, rel_path })
                    if *sid == share_id && rel_path == "notes.md"
            )),
            "SetServerTx deve ri-emettere ShareContentRequest per una condivisione ancora AwaitingContent: {effects:?}"
        );
    }

    /// `ClientMsg::ShareContent` (via `ServiceEvent`) manda `ChatMsg::ShareData` al peer
    /// risolto FRESCO e rimuove l'entry — nessun secondo `ShareResult` (il mittente ha già
    /// visto "accettata").
    #[test]
    fn share_content_sends_share_data_and_removes_entry() {
        let mut s = AiChatService::new_for_test(info(10, "rumpleteazer"));
        s.handle_event(ServiceEvent::Discovered(info(20, "skimble"), None));
        s.mark_connected_for_test(PeerId(Ipv4Addr::new(192, 168, 1, 20)));
        s.handle_event(ServiceEvent::Discovered(info(20, "skimble"), None));

        let effects = s.handle_event(ServiceEvent::ShareDocumentRequested {
            rel_path: "notes.md".into(),
            doc_name: "notes.md".into(),
            size_bytes: 100,
            target: protocol::ShareTarget::One { label_base: "skimble".into() },
        });
        let share_id = effects.iter().find_map(|e| match e {
            Effect::SendToPeer(_, ChatMsg::ShareOffer { share_id, .. }) => Some(share_id.clone()),
            _ => None,
        }).expect("deve aver mandato un'Offer");

        let from = PeerId(Ipv4Addr::new(192, 168, 1, 20));
        s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::ShareAccept { share_id: share_id.clone(), from_label: "skimble".into() },
        });

        let effects = s.handle_event(ServiceEvent::ShareContentUi {
            share_id: share_id.clone(),
            title: "Note".into(),
            content: "corpo".into(),
        });

        assert!(
            effects.iter().any(|e| matches!(
                e,
                Effect::SendToPeer(_, ChatMsg::ShareData { share_id: sid, title, content })
                    if *sid == share_id && title == "Note" && content == "corpo"
            )),
            "atteso SendToPeer ShareData: {effects:?}"
        );
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::ToUi(ServerMsg::ShareResult { .. }))),
            "nessun secondo ShareResult per il successo del contenuto: {effects:?}"
        );
        assert!(!s.outgoing_shares_for_test().contains_key(&share_id));
    }

    /// `ClientMsg::ShareContentFailed` riporta subito `Failed` e rimuove l'entry — non
    /// aspetta la scadenza di 24h (deciso nel brainstorm, spec §2).
    #[test]
    fn share_content_failed_reports_failed_immediately() {
        let mut s = AiChatService::new_for_test(info(10, "rumpleteazer"));
        s.handle_event(ServiceEvent::Discovered(info(20, "skimble"), None));
        s.mark_connected_for_test(PeerId(Ipv4Addr::new(192, 168, 1, 20)));
        s.handle_event(ServiceEvent::Discovered(info(20, "skimble"), None));

        let effects = s.handle_event(ServiceEvent::ShareDocumentRequested {
            rel_path: "notes.md".into(),
            doc_name: "notes.md".into(),
            size_bytes: 100,
            target: protocol::ShareTarget::One { label_base: "skimble".into() },
        });
        let share_id = effects.iter().find_map(|e| match e {
            Effect::SendToPeer(_, ChatMsg::ShareOffer { share_id, .. }) => Some(share_id.clone()),
            _ => None,
        }).expect("deve aver mandato un'Offer");

        let from = PeerId(Ipv4Addr::new(192, 168, 1, 20));
        s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::ShareAccept { share_id: share_id.clone(), from_label: "skimble".into() },
        });

        let effects = s.handle_event(ServiceEvent::ShareContentFailedUi {
            share_id: share_id.clone(),
            reason: "documento non più disponibile".into(),
        });

        assert!(
            effects.iter().any(|e| matches!(
                e,
                Effect::ToUi(ServerMsg::ShareResult {
                    outcome: protocol::ShareOutcome::Failed { reason },
                    ..
                }) if reason == "documento non più disponibile"
            )),
            "atteso ShareResult{{Failed}}: {effects:?}"
        );
        assert!(!s.outgoing_shares_for_test().contains_key(&share_id));
    }

    // ── Task 7: Blocco note — creazione/modifica/cancellazione locale ──────────
    // Design: Docs/superpowers/specs/2026-07-29-library-notes-design.md §5.1.

    /// `NoteCreateRequested` deve applicare subito in memoria (via `merge_note`),
    /// spingere una `NoteUpserted` alla UI e chiedere il salvataggio su disco.
    #[test]
    fn note_create_requested_upserts_locally_pushes_to_ui_and_saves() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let effects = s.handle_event(ServiceEvent::NoteCreateRequested {
            title: "Idea".into(),
            text: "corpo".into(),
        });

        assert!(effects.iter().any(|e| matches!(e, Effect::SaveNotes)));
        let view = effects.iter().find_map(|e| match e {
            Effect::ToUi(ServerMsg::NoteUpserted { note }) => Some(note.clone()),
            _ => None,
        }).expect("deve emettere NoteUpserted");
        assert_eq!(view.title, "Idea");
        assert_eq!(view.body, "corpo");
        assert_eq!(view.created_by, "skimble");
        assert!(!view.deleted);
    }

    /// `my_segment_text` deve riportare il segmento di QUESTA macchina, non il
    /// corpo intero fuso — precompila la finestra "Modifica" (trovato mancante
    /// nello smoke test dal vivo del 2026-07-31: la casella appariva vuota
    /// anche su una nota con contenuto).
    #[test]
    fn note_view_my_segment_text_reflects_own_segment_on_create() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let effects = s.handle_event(ServiceEvent::NoteCreateRequested {
            title: "Idea".into(),
            text: "corpo mio".into(),
        });
        let view = effects.iter().find_map(|e| match e {
            Effect::ToUi(ServerMsg::NoteUpserted { note }) => Some(note.clone()),
            _ => None,
        }).unwrap();
        assert_eq!(view.my_segment_text, "corpo mio");
    }

    /// Una nota ricevuta da un peer, a cui questa macchina non ha ancora
    /// contribuito, deve avere `my_segment_text` vuoto — non deve mai
    /// inventare testo, né confonderlo col corpo fuso (`view.body`), che
    /// invece riporta anche il contributo altrui.
    #[test]
    fn note_view_my_segment_text_empty_when_this_machine_never_contributed() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let from = PeerId(std::net::Ipv4Addr::new(192, 168, 1, 20));
        let incoming = Note {
            id: "192.168.1.20:0".into(),
            title: "Remota".into(),
            title_touched: ("rumpleteazer".into(), 100),
            segments: vec![Segment { machine: "rumpleteazer".into(), seq: 1, text: "corpo altrui".into(), edited_at_ms: 100 }],
            created_by: "rumpleteazer".into(),
            created_at_ms: 100,
            deleted: false,
        };
        let effects = s.handle_event(ServiceEvent::PeerMsg { from, msg: ChatMsg::NoteUpdated { note: incoming } });
        let view = effects.iter().find_map(|e| match e {
            Effect::ToUi(ServerMsg::NoteUpserted { note }) => Some(note.clone()),
            _ => None,
        }).unwrap();
        assert_eq!(view.my_segment_text, "");
        assert_eq!(view.body, "corpo altrui", "il corpo fuso resta visibile anche senza contributo proprio");
    }

    /// `NoteEditRequested` sostituisce SOLO il segmento di questa macchina — con un
    /// solo segmento presente, il corpo renderizzato è il nuovo testo, non
    /// un'accumulazione dei due (regressione diretta del bug "contatore singolo +
    /// concatenazione" di `merge_note`, design §4).
    #[test]
    fn note_edit_requested_replaces_own_segment_not_whole_note() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let create_effects = s.handle_event(ServiceEvent::NoteCreateRequested {
            title: "Idea".into(),
            text: "v1".into(),
        });
        let id = create_effects.iter().find_map(|e| match e {
            Effect::ToUi(ServerMsg::NoteUpserted { note }) => Some(note.id.clone()),
            _ => None,
        }).unwrap();

        let edit_effects = s.handle_event(ServiceEvent::NoteEditRequested { id: id.clone(), text: "v2".into() });
        let view = edit_effects.iter().find_map(|e| match e {
            Effect::ToUi(ServerMsg::NoteUpserted { note }) => Some(note.clone()),
            _ => None,
        }).unwrap();
        assert_eq!(view.body, "v2", "un solo segmento (la nostra macchina): sostituito, non accumulato");
    }

    /// `NoteDeleteRequested` imposta il tombstone (`deleted: true`), mai rimosso da
    /// `merge_note` — vedi `merge_tombstone_never_resets_to_false` in `notes/mod.rs`.
    #[test]
    fn note_delete_requested_sets_tombstone() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let create_effects = s.handle_event(ServiceEvent::NoteCreateRequested { title: "X".into(), text: "y".into() });
        let id = create_effects.iter().find_map(|e| match e {
            Effect::ToUi(ServerMsg::NoteUpserted { note }) => Some(note.id.clone()),
            _ => None,
        }).unwrap();

        let del_effects = s.handle_event(ServiceEvent::NoteDeleteRequested { id: id.clone() });
        let view = del_effects.iter().find_map(|e| match e {
            Effect::ToUi(ServerMsg::NoteUpserted { note }) => Some(note.clone()),
            _ => None,
        }).unwrap();
        assert!(view.deleted);
    }

    /// `NoteEditTitleRequested` cambia SOLO il titolo (last-write-wins su
    /// `title_touched`) senza mai perdere il corpo — la delta passata a
    /// `upsert_merged` ha `segments: Vec::new()`, quindi il corpo sopravvive
    /// SOLO se `merge_note` preserva i segmenti del lato che non li porta
    /// (proprietà verificata a parte per `merge_note` in `notes/mod.rs`, qui
    /// verifichiamo che l'arm la sfrutti correttamente). Orologio iniettato
    /// (`set_wallclock_ms_for_test`) per rendere deterministico il tie-break
    /// LWW: senza un secondo timestamp maggiore del primo, il nuovo titolo
    /// perderebbe sempre il confronto `b.title_touched.1 > a.title_touched.1`.
    #[test]
    fn note_edit_title_requested_changes_title_without_losing_the_body() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        s.set_wallclock_ms_for_test(1_000);
        let create_effects = s.handle_event(ServiceEvent::NoteCreateRequested {
            title: "Idea".into(),
            text: "corpo".into(),
        });
        let id = create_effects.iter().find_map(|e| match e {
            Effect::ToUi(ServerMsg::NoteUpserted { note }) => Some(note.id.clone()),
            _ => None,
        }).unwrap();

        s.set_wallclock_ms_for_test(2_000); // strettamente maggiore: il nuovo titolo vince il LWW
        let edit_effects = s.handle_event(ServiceEvent::NoteEditTitleRequested { id: id.clone(), title: "Nuovo".into() });
        let view = edit_effects.iter().find_map(|e| match e {
            Effect::ToUi(ServerMsg::NoteUpserted { note }) => Some(note.clone()),
            _ => None,
        }).unwrap();
        assert_eq!(view.title, "Nuovo");
        assert_eq!(view.body, "corpo", "il titolo cambia, il corpo NON deve sparire");
    }

    /// Ogni nota creata localmente va mandata a OGNI peer attualmente linkato
    /// (`broadcast_note_to_links`) — se siamo client è l'unico link (verso il
    /// server), se siamo server è già il fan-out verso tutti i client.
    #[test]
    fn note_create_requested_sends_to_every_currently_linked_peer() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        s.handle_event(ServiceEvent::Discovered(info(20, "rumpleteazer"), None));
        s.mark_connected_for_test(PeerId(Ipv4Addr::new(192, 168, 1, 20)));

        let effects = s.handle_event(ServiceEvent::NoteCreateRequested { title: "X".into(), text: "y".into() });
        assert!(effects.iter().any(|e| matches!(e, Effect::SendToPeer(_, ChatMsg::NoteUpdated { .. }))));
    }

    /// FIX 1 (review finale di branch) — `next_note_id` va SEEDATO dallo store già
    /// caricato da disco, non azzerato a 0.
    ///
    /// Scenario reale: `main.rs` carica `notes.json` (che contiene già `"<mio-ip>:0"`
    /// da prima del riavvio) e passa lo store a `AiChatService::new`. Con
    /// `next_note_id: 0` fisso, la PRIMA nota creata dopo il riavvio riusa l'id
    /// `"<mio-ip>:0"`: `upsert_merged` non inserisce, FONDE con la vecchia — il
    /// testo nuovo viene scartato (il vecchio segmento ha `seq` ≥) e il titolo
    /// sovrascritto via LWW. Perdita di dati silenziosa, per giunta ribroadcastata
    /// a tutte le altre macchine.
    ///
    /// Le due asserzioni forti sono `all().len() == 2` (la nota nuova ESISTE come
    /// entità separata) e la sopravvivenza del suo testo — l'id diverso da solo
    /// sarebbe una proxy debole.
    #[test]
    fn next_note_id_is_seeded_from_the_loaded_store_so_new_notes_never_collide() {
        let me = info(10, "skimble");
        // Store "caricato da disco": contiene già una nota creata da QUESTA macchina
        // prima del riavvio, con l'id `"192.168.1.10:0"`.
        let mut store = NotesStore::empty_in_memory();
        store.upsert_merged(Note {
            id: "192.168.1.10:0".into(),
            title: "Vecchia".into(),
            title_touched: ("skimble".into(), 500),
            segments: vec![Segment {
                machine: "skimble".into(),
                seq: 7, // seq ALTO: un segmento nuovo con seq 1 perderebbe il merge
                text: "testo vecchio".into(),
                edited_at_ms: 500,
            }],
            created_by: "skimble".into(),
            created_at_ms: 500,
            deleted: false,
        });
        let mut s = AiChatService::new(
            me,
            Arc::new(crate::ai_adapter::StubAdapter),
            true,
            false,
            store,
            None,
            None,
            "unused".into(),
        );

        let effects = s.handle_event(ServiceEvent::NoteCreateRequested {
            title: "Nuova".into(),
            text: "testo nuovo".into(),
        });

        let view = effects
            .iter()
            .find_map(|e| match e {
                Effect::ToUi(ServerMsg::NoteUpserted { note }) => Some(note.clone()),
                _ => None,
            })
            .expect("deve emettere NoteUpserted");

        assert_ne!(view.id, "192.168.1.10:0", "l'id NON deve collidere con quello già in archivio");
        assert_eq!(
            s.notes.all().len(),
            2,
            "la nota nuova deve essere una SECONDA entità, non una fusione con la vecchia"
        );
        assert_eq!(view.title, "Nuova");
        assert_eq!(view.body, "testo nuovo", "il testo appena scritto non deve essere scartato dal merge");
        // La nota preesistente resta intatta (nessuna sovrascrittura del titolo via LWW).
        assert_eq!(s.notes.get("192.168.1.10:0").expect("la vecchia deve restare").title, "Vecchia");
    }

    /// Rivalidazione dimensione al momento dell'invio: un contenuto oltre il cap ha lo
    /// stesso trattamento di `ShareContentFailed`, anche se l'offerta iniziale era sotto
    /// il cap (il documento può essere cresciuto nel frattempo).
    #[test]
    fn share_content_over_cap_at_send_time_fails_like_content_failed() {
        let mut s = AiChatService::new_for_test(info(10, "rumpleteazer"));
        s.handle_event(ServiceEvent::Discovered(info(20, "skimble"), None));
        s.mark_connected_for_test(PeerId(Ipv4Addr::new(192, 168, 1, 20)));
        s.handle_event(ServiceEvent::Discovered(info(20, "skimble"), None));

        let effects = s.handle_event(ServiceEvent::ShareDocumentRequested {
            rel_path: "notes.md".into(),
            doc_name: "notes.md".into(),
            size_bytes: 100,
            target: protocol::ShareTarget::One { label_base: "skimble".into() },
        });
        let share_id = effects.iter().find_map(|e| match e {
            Effect::SendToPeer(_, ChatMsg::ShareOffer { share_id, .. }) => Some(share_id.clone()),
            _ => None,
        }).expect("deve aver mandato un'Offer");

        let from = PeerId(Ipv4Addr::new(192, 168, 1, 20));
        s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::ShareAccept { share_id: share_id.clone(), from_label: "skimble".into() },
        });

        let oversized = "x".repeat(600 * 1024); // oltre i 512 KB del cap
        let effects = s.handle_event(ServiceEvent::ShareContentUi {
            share_id: share_id.clone(),
            title: "Note".into(),
            content: oversized,
        });

        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::ToUi(ServerMsg::ShareResult { outcome: protocol::ShareOutcome::Failed { .. }, .. }))),
            "atteso ShareResult{{Failed}} per dimensione oltre cap: {effects:?}"
        );
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::SendToPeer(_, ChatMsg::ShareData { .. }))),
            "non deve mandare ShareData se oltre il cap: {effects:?}"
        );
        assert!(!s.outgoing_shares_for_test().contains_key(&share_id));
    }

    /// Un `ChatMsg::ShareData` per un'offerta GIÀ ACCETTATA (`AwaitingContent`) transiziona
    /// l'entry ad `AwaitingUiWrite` ed emette `ShareIncomingData` se la UI è connessa (spec
    /// Slice 2a §3).
    #[test]
    fn peer_share_data_after_accept_transitions_to_awaiting_ui_write_and_notifies_ui() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let from = PeerId(Ipv4Addr::new(192, 168, 1, 26));
        s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::ShareOffer {
                share_id: "192.168.1.26:0".into(),
                from_label: "rumpleteazer".into(),
                doc_name: "notes.md".into(),
                size_bytes: 100,
            },
        });
        s.handle_event(ServiceEvent::ShareConsentUi { share_id: "192.168.1.26:0".into(), accept: true });
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<ServerMsg>();
        s.handle_event(ServiceEvent::SetServerTx(tx));

        let effects = s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::ShareData {
                share_id: "192.168.1.26:0".into(),
                title: "Note".into(),
                content: "corpo".into(),
            },
        });

        assert!(
            effects.iter().any(|e| matches!(
                e,
                Effect::ToUi(ServerMsg::ShareIncomingData { share_id, from_label, title, content })
                    if share_id == "192.168.1.26:0" && from_label == "rumpleteazer"
                        && title == "Note" && content == "corpo"
            )),
            "atteso ShareIncomingData, trovato: {effects:?}"
        );
        assert_eq!(
            s.pending_shares_for_test().get("192.168.1.26:0"),
            Some(&PendingShare::AwaitingUiWrite {
                from_label: "rumpleteazer".into(),
                title: "Note".into(),
                content: "corpo".into(),
            })
        );
    }

    /// FIX DI SICUREZZA (review finale Slice 2a): un `ChatMsg::ShareData` che arriva
    /// PRIMA che l'umano abbia accettato (l'entry è ancora `AwaitingDecision`, non
    /// `AwaitingContent`) è un no-op — non deve scrivere nulla, non deve transizionare,
    /// non deve emettere `ShareIncomingData`. Prima di questo fix, un mittente
    /// buggato/non-conforme poteva mandare `ShareData` subito dopo `ShareOffer`, senza
    /// mai aspettare `ShareAccept`, e il contenuto veniva comunque scritto in Library
    /// (spec §8 violato: "niente contenuto prima del consenso").
    #[test]
    fn peer_share_data_before_accept_is_noop() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let from = PeerId(Ipv4Addr::new(192, 168, 1, 26));
        s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::ShareOffer {
                share_id: "192.168.1.26:0".into(),
                from_label: "rumpleteazer".into(),
                doc_name: "notes.md".into(),
                size_bytes: 100,
            },
        });
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<ServerMsg>();
        s.handle_event(ServiceEvent::SetServerTx(tx));

        // NESSUN ShareConsentUi qui — il contenuto arriva prima di qualsiasi decisione.
        let effects = s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::ShareData {
                share_id: "192.168.1.26:0".into(),
                title: "Note".into(),
                content: "corpo".into(),
            },
        });

        assert!(
            effects.is_empty(),
            "ShareData prima dell'accettazione non deve emettere nulla: {effects:?}"
        );
        assert_eq!(
            s.pending_shares_for_test().get("192.168.1.26:0"),
            Some(&PendingShare::AwaitingDecision {
                from_label: "rumpleteazer".into(),
                doc_name: "notes.md".into(),
                size_bytes: 100,
            }),
            "l'entry deve restare AwaitingDecision, invariata — nessuna transizione implicita"
        );
    }

    /// Rivalidazione dimensione al momento della ricezione (spec §8, "l'annuncio
    /// potrebbe mentire"): un `ChatMsg::ShareData` il cui contenuto reale supera il cap,
    /// arrivato per un'entry legittimamente `AwaitingContent`, viene rifiutato
    /// attivamente (stesso trattamento dell'offerta iniziale sovradimensionata) invece
    /// di essere scritto in Library.
    #[test]
    fn peer_share_data_over_cap_rejects_and_removes_entry() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        s.handle_event(ServiceEvent::Discovered(info(26, "rumpleteazer"), None));
        let from = PeerId(Ipv4Addr::new(192, 168, 1, 26));
        s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::ShareOffer {
                share_id: "192.168.1.26:0".into(),
                from_label: "rumpleteazer".into(),
                doc_name: "notes.md".into(),
                size_bytes: 100,
            },
        });
        s.handle_event(ServiceEvent::ShareConsentUi { share_id: "192.168.1.26:0".into(), accept: true });

        let oversized = "x".repeat(600 * 1024); // oltre i 512 KB del cap
        let effects = s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::ShareData {
                share_id: "192.168.1.26:0".into(),
                title: "Note".into(),
                content: oversized,
            },
        });

        assert!(
            effects.iter().any(|e| matches!(e, Effect::SendToPeer(p, ChatMsg::ShareReject { share_id, .. })
                if *p == from && share_id == "192.168.1.26:0")),
            "deve rifiutare attivamente sopra il cap: {effects:?}"
        );
        assert!(!s.pending_shares_for_test().contains_key("192.168.1.26:0"));
    }

    /// Stesso `ShareData` (dopo l'accettazione), ma senza UI connessa: transizione
    /// avviene comunque, nessun `ShareIncomingData` immediato — il replay avverrà al
    /// prossimo `SetServerTx`.
    #[test]
    fn peer_share_data_after_accept_without_ui_transitions_silently() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let from = PeerId(Ipv4Addr::new(192, 168, 1, 26));
        s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::ShareOffer {
                share_id: "192.168.1.26:0".into(),
                from_label: "rumpleteazer".into(),
                doc_name: "notes.md".into(),
                size_bytes: 100,
            },
        });
        s.handle_event(ServiceEvent::ShareConsentUi { share_id: "192.168.1.26:0".into(), accept: true });

        let effects = s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::ShareData {
                share_id: "192.168.1.26:0".into(),
                title: "Note".into(),
                content: "corpo".into(),
            },
        });

        assert!(effects.is_empty(), "senza UI connessa non deve emettere nulla subito: {effects:?}");
        assert_eq!(
            s.pending_shares_for_test().get("192.168.1.26:0"),
            Some(&PendingShare::AwaitingUiWrite {
                from_label: "rumpleteazer".into(),
                title: "Note".into(),
                content: "corpo".into(),
            })
        );
    }

    /// `ShareData` per uno `share_id` sconosciuto (mai offerto, o già risolto) è un no-op
    /// difensivo — non ci fidiamo ciecamente di un mittente che manda dati senza offerta.
    #[test]
    fn peer_share_data_for_unknown_share_id_is_noop() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let from = PeerId(Ipv4Addr::new(192, 168, 1, 26));
        let effects = s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::ShareData {
                share_id: "ghost".into(),
                title: "Note".into(),
                content: "corpo".into(),
            },
        });
        assert!(effects.is_empty());
        assert!(!s.pending_shares_for_test().contains_key("ghost"));
    }

    /// `ClientMsg::ShareWritten` (via `ServiceEvent::ShareWrittenUi`) rimuove l'entry —
    /// idempotente: un secondo ack per lo stesso `share_id` è un no-op silenzioso.
    #[test]
    fn share_written_ui_removes_pending_entry_idempotently() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let from = PeerId(Ipv4Addr::new(192, 168, 1, 26));
        s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::ShareOffer {
                share_id: "192.168.1.26:0".into(),
                from_label: "rumpleteazer".into(),
                doc_name: "notes.md".into(),
                size_bytes: 100,
            },
        });
        s.handle_event(ServiceEvent::ShareConsentUi { share_id: "192.168.1.26:0".into(), accept: true });
        s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::ShareData {
                share_id: "192.168.1.26:0".into(),
                title: "Note".into(),
                content: "corpo".into(),
            },
        });

        let effects = s.handle_event(ServiceEvent::ShareWrittenUi { share_id: "192.168.1.26:0".into() });
        assert!(effects.is_empty());
        assert!(!s.pending_shares_for_test().contains_key("192.168.1.26:0"));

        // Secondo ack, stesso share_id: no-op, nessun panic.
        let effects2 = s.handle_event(ServiceEvent::ShareWrittenUi { share_id: "192.168.1.26:0".into() });
        assert!(effects2.is_empty());
    }

    /// Il timer di scadenza scatta mentre l'entry è in `AwaitingUiWrite` (il contenuto è
    /// arrivato ma la UI non l'ha mai scritto su disco, es. finestra chiusa e mai
    /// riaperta entro 24h) — stesso trattamento di quando scatta su `AwaitingDecision`:
    /// rimuove l'entry, notifica il mittente con `ShareExpired`. Nuovo scenario reso
    /// possibile dal secondo stadio introdotto in questo task (Step 9).
    #[test]
    fn share_expiry_timeout_notifies_sender_when_awaiting_ui_write() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        s.handle_event(ServiceEvent::Discovered(info(26, "rumpleteazer"), None));
        let from = PeerId(Ipv4Addr::new(192, 168, 1, 26));
        s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::ShareOffer {
                share_id: "192.168.1.26:0".into(),
                from_label: "rumpleteazer".into(),
                doc_name: "notes.md".into(),
                size_bytes: 100,
            },
        });
        s.handle_event(ServiceEvent::ShareConsentUi { share_id: "192.168.1.26:0".into(), accept: true });
        s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::ShareData {
                share_id: "192.168.1.26:0".into(),
                title: "Note".into(),
                content: "corpo".into(),
            },
        });
        assert_eq!(
            s.pending_shares_for_test().get("192.168.1.26:0"),
            Some(&PendingShare::AwaitingUiWrite {
                from_label: "rumpleteazer".into(),
                title: "Note".into(),
                content: "corpo".into(),
            }),
            "precondizione: l'entry deve essere AwaitingUiWrite prima della scadenza"
        );

        let effects = s.handle_event(ServiceEvent::ShareExpiryTimeout { share_id: "192.168.1.26:0".into() });

        assert!(effects.iter().any(|e| matches!(e, Effect::SendToPeer(p, ChatMsg::ShareExpired { share_id })
            if *p == from && share_id == "192.168.1.26:0")), "{effects:?}");
        assert!(!s.pending_shares_for_test().contains_key("192.168.1.26:0"));
    }

    /// Il timer di scadenza scatta mentre l'entry è in `AwaitingContent` (accettata, ma
    /// il contenuto vero non è mai arrivato) — stesso trattamento degli altri due stadi:
    /// rimuove l'entry, notifica il mittente con `ShareExpired`. Terzo scenario del trio
    /// (`AwaitingDecision`/`AwaitingContent`/`AwaitingUiWrite`) introdotto dal fix di
    /// integrità del consenso.
    #[test]
    fn share_expiry_timeout_notifies_sender_when_awaiting_content() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        s.handle_event(ServiceEvent::Discovered(info(26, "rumpleteazer"), None));
        let from = PeerId(Ipv4Addr::new(192, 168, 1, 26));
        s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::ShareOffer {
                share_id: "192.168.1.26:0".into(),
                from_label: "rumpleteazer".into(),
                doc_name: "notes.md".into(),
                size_bytes: 100,
            },
        });
        s.handle_event(ServiceEvent::ShareConsentUi { share_id: "192.168.1.26:0".into(), accept: true });
        assert_eq!(
            s.pending_shares_for_test().get("192.168.1.26:0"),
            Some(&PendingShare::AwaitingContent {
                from_label: "rumpleteazer".into(),
                doc_name: "notes.md".into(),
                size_bytes: 100,
            }),
            "precondizione: l'entry deve essere AwaitingContent prima della scadenza"
        );

        let effects = s.handle_event(ServiceEvent::ShareExpiryTimeout { share_id: "192.168.1.26:0".into() });

        assert!(effects.iter().any(|e| matches!(e, Effect::SendToPeer(p, ChatMsg::ShareExpired { share_id })
            if *p == from && share_id == "192.168.1.26:0")), "{effects:?}");
        assert!(!s.pending_shares_for_test().contains_key("192.168.1.26:0"));
    }

    /// (Destinatario) Il timer di scadenza scatta mentre l'offerta è ANCORA pendente
    /// (mai risposta) — `AwaitingDecision`: rimuove `pending_shares`, notifica il
    /// mittente con `ShareExpired`. Il caso "già accettata ma contenuto non arrivato"
    /// ha ora un proprio stadio e un proprio test
    /// (`share_expiry_timeout_notifies_sender_when_awaiting_content`, fix di integrità
    /// del consenso, review finale Slice 2a).
    #[test]
    fn share_expiry_timeout_notifies_sender_when_still_pending() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        s.handle_event(ServiceEvent::Discovered(info(26, "rumpleteazer"), None));
        let from = PeerId(Ipv4Addr::new(192, 168, 1, 26));
        s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::ShareOffer {
                share_id: "192.168.1.26:0".into(),
                from_label: "rumpleteazer".into(),
                doc_name: "notes.md".into(),
                size_bytes: 1234,
            },
        });

        let effects = s.handle_event(ServiceEvent::ShareExpiryTimeout { share_id: "192.168.1.26:0".into() });

        assert!(effects.iter().any(|e| matches!(e, Effect::SendToPeer(p, ChatMsg::ShareExpired { share_id })
            if *p == from && share_id == "192.168.1.26:0")), "{effects:?}");
        assert!(!s.pending_shares_for_test().contains_key("192.168.1.26:0"));
    }

    /// Il timer scatta ma l'offerta è GIÀ stata rifiutata prima dello scadere: no-op —
    /// stesso principio di `VoteTimeout` stantio. Il caso "già accettata" non rientra
    /// più qui dal fix di integrità del consenso (review finale Slice 2a): accettare
    /// transiziona esplicitamente ad `AwaitingContent`, uno stadio distinto con un
    /// proprio test (`share_expiry_timeout_notifies_sender_when_awaiting_content`), non
    /// più indistinguibile da un'offerta mai decisa.
    #[test]
    fn share_expiry_timeout_is_noop_when_already_rejected() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        s.handle_event(ServiceEvent::Discovered(info(26, "rumpleteazer"), None));
        let from = PeerId(Ipv4Addr::new(192, 168, 1, 26));
        s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::ShareOffer {
                share_id: "192.168.1.26:0".into(),
                from_label: "rumpleteazer".into(),
                doc_name: "notes.md".into(),
                size_bytes: 1234,
            },
        });
        s.handle_event(ServiceEvent::ShareConsentUi { share_id: "192.168.1.26:0".into(), accept: false });

        let effects = s.handle_event(ServiceEvent::ShareExpiryTimeout { share_id: "192.168.1.26:0".into() });
        assert!(effects.is_empty(), "un timeout stantio non deve produrre effetti: {effects:?}");
    }

    /// Adapter di test che ritorna sempre una `chat_reply` vuota (simula il ramo
    /// "cancellazione upfront" di `LlmAdapter::chat_reply`, o una risposta degenere).
    /// `respond` non è esercitato da questi test: implementazione minima (no-op).
    struct EmptyReplyAdapter;

    #[async_trait::async_trait]
    impl AiAdapter for EmptyReplyAdapter {
        #[allow(clippy::too_many_arguments)]
        async fn respond(
            &self,
            _id: &str,
            _input: &str,
            _history: &mut crate::messages_client::ConversationHistory,
            _tools: &dyn crate::tool_client::ToolClient,
            _opts: crate::agent::TurnOptions,
            _confirmer: Option<&dyn crate::ai_adapter::ToolConfirmer>,
            _cancel: Option<tokio_util::sync::CancellationToken>,
            _tx: tokio::sync::mpsc::UnboundedSender<ServerMsg>,
        ) {
        }

        async fn chat_reply(
            &self,
            _my_ai_label: &str,
            _history: &[crate::messages_client::Message],
            _request: &str,
            _cancel: Option<tokio_util::sync::CancellationToken>,
        ) -> String {
            String::new()
        }
    }

    /// Se `chat_reply` ritorna una stringa vuota, `perform` NON deve re-iniettare un
    /// `AiReply` vuoto: pubblicarlo produrrebbe una riga di chat fantasma (storico + eco
    /// UI + relay, tramite `publish_say`) senza alcun valore per l'umano. Verifichiamo
    /// che l'inbox si esaurisca SENZA alcun evento: droppiamo il nostro `inbox_tx` e
    /// attendiamo `recv() == None`, che si verifica solo quando anche il clone del task
    /// spawnato è stato droppato SENZA aver mandato nulla.
    #[tokio::test]
    async fn perform_invoke_local_ai_skips_empty_reply() {
        let mut s = AiChatService::new(
            info(10, "skimble"),
            Arc::new(EmptyReplyAdapter),
            true,
            false,
            NotesStore::empty_in_memory(),
            None,
            None,
            "unused".into(),
        );
        let (inbox_tx, mut inbox_rx) = tokio::sync::mpsc::unbounded_channel::<ServiceEvent>();
        let (new_link_tx, _new_link_rx) = tokio::sync::mpsc::unbounded_channel();
        let (connect_res_tx, _connect_res_rx) = tokio::sync::mpsc::unbounded_channel();
        let shutdown = tokio_util::sync::CancellationToken::new();

        s.perform(
            Effect::InvokeLocalAi {
                request: "x".into(),
                history: Vec::new(),
                my_ai_label: "skimble-ai".into(),
            },
            &inbox_tx,
            &new_link_tx,
            &connect_res_tx,
            &shutdown,
        )
        .await;

        drop(inbox_tx); // il nostro handle: resta solo il clone del task spawnato
        assert!(
            inbox_rx.recv().await.is_none(),
            "nessun AiReply atteso per una chat_reply vuota"
        );
    }

    // -------------------------------------------------------------------------
    // Scenario di Test per Task 5: PeerMsg (relay server + forward alla UI)
    // -------------------------------------------------------------------------

    /// Verifica che il SERVER, ricevendo un `ChatMsg::Say` da un peer,
    /// (1) faccia forward alla propria UI come `Effect::ToUi(AiChatMessage)`,
    /// (2) inoltri il messaggio agli altri client connessi (n-2 = solo .30, non .20 autore).
    ///
    /// Scenario: me=.10 (server), client .20 invia "ciao".
    /// Atteso: UI riceve AiChatMessage; .30 riceve il relay; .20 (autore) NON riceve.
    #[test]
    fn server_relays_peer_say_to_other_clients_and_forwards_to_ui() {
        let mut s = server_with_two_clients(); // me=.10 server, client .20/.30
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<ServerMsg>();
        s.handle_event(ServiceEvent::SetServerTx(tx));
        let from = PeerId(std::net::Ipv4Addr::new(192, 168, 1, 20));
        // Task 8b: il relay-guard ignora un Say da un mittente non ammesso — questo
        // test verifica il relay "normale" (n-2), quindi il mittente DEVE essere
        // ammesso (il caso "non ammesso" ha un test dedicato,
        // `server_ignores_say_from_non_admitted`).
        s.mark_admitted_for_test(from);
        let msg = ChatMsg::Say { from_label: "m20-human".into(), text: "ciao".into(), display_name: None, is_ai: false };
        let effects = s.handle_event(ServiceEvent::PeerMsg { from, msg });
        // forward alla UI
        assert!(
            effects.iter().any(|e| matches!(e,
                Effect::ToUi(ServerMsg::AiChatMessage { from_label, .. }) if from_label == "m20-human")),
            "forward alla UI mancante: {effects:?}"
        );
        // relay all'ALTRO client (.30), non a .20 (l'autore)
        let targets: Vec<_> = effects
            .iter()
            .filter_map(|e| match e { Effect::SendToPeer(p, _) => Some(*p), _ => None })
            .collect();
        assert_eq!(
            targets,
            vec![PeerId(std::net::Ipv4Addr::new(192, 168, 1, 30))],
            "relay a destinatari errati: {effects:?}"
        );
    }

    // -------------------------------------------------------------------------
    // Task 3 (display names): `publish_say` calcola is_ai/display_name per i
    // messaggi ORIGINATI da questa macchina (umano o AI locale) — vedi
    // Docs/superpowers/plans/2026-08-13-aichat-display-names.md, Task 3.
    // -------------------------------------------------------------------------

    /// Un `HumanSay` locale deve produrre `is_ai = false` e `display_name` risolto
    /// dal nickname umano configurato (`self.display_name`), non `None`.
    #[test]
    fn publish_say_stamps_is_ai_false_and_human_display_name_for_local_human_message() {
        let me = PeerInfo { id: PeerId(std::net::Ipv4Addr::new(192, 168, 1, 10)), label_base: "skimble".into(), chat_port: 40100 };
        let mut s = AiChatService::new_for_test_with_display_names(me, Some("Maurizio".to_string()), None);
        let effects = s.handle_event(ServiceEvent::HumanSay("ciao".into()));
        let to_ui = effects.iter().find_map(|e| match e {
            Effect::ToUi(ServerMsg::AiChatMessage { from_label, display_name, is_ai, .. }) => {
                Some((from_label.clone(), display_name.clone(), *is_ai))
            }
            _ => None,
        }).expect("atteso un Effect::ToUi(AiChatMessage)");
        assert_eq!(to_ui.0, "skimble-human");
        assert_eq!(to_ui.1, Some("Maurizio".to_string()));
        assert!(!to_ui.2);
    }

    /// Un `AiReply` locale (la MIA AI) deve produrre `is_ai = true` e `display_name`
    /// risolto dal nickname AI configurato (`self.ai_display_name`), non dal nickname
    /// umano.
    #[test]
    fn publish_say_stamps_is_ai_true_and_ai_display_name_for_local_ai_message() {
        let me = PeerInfo { id: PeerId(std::net::Ipv4Addr::new(192, 168, 1, 10)), label_base: "skimble".into(), chat_port: 40100 };
        let mut s = AiChatService::new_for_test_with_display_names(me, Some("Maurizio".to_string()), Some("Aria".to_string()));
        // `ServiceEvent::AiReply { text }` è il percorso "i miei messaggi AI" che
        // `publish_say` normalizza a `from_label = "<base>-ai"` (confermato dal suo
        // doc-comment sopra `publish_say`, e dall'arm `ServiceEvent::AiReply`).
        let effects = s.handle_event(ServiceEvent::AiReply { text: "ciao".into() });
        let to_ui = effects.iter().find_map(|e| match e {
            Effect::ToUi(ServerMsg::AiChatMessage { from_label, display_name, is_ai, .. }) => {
                Some((from_label.clone(), display_name.clone(), *is_ai))
            }
            _ => None,
        }).expect("atteso un Effect::ToUi(AiChatMessage)");
        assert_eq!(to_ui.0, "skimble-ai");
        assert_eq!(to_ui.1, Some("Aria".to_string()));
        assert!(to_ui.2);
    }

    /// L'arm `PeerMsg::Say` deve fare SOLO pass-through di `display_name`/`is_ai` —
    /// MAI ricalcolarli dalla propria `self.display_name`/`self.ai_display_name`.
    ///
    /// Discrimina davvero i due comportamenti: il RICEVENTE ha i propri nickname
    /// configurati ("Maurizio"/"Aria"), ma il messaggio arriva già stampato con il
    /// nickname del MITTENTE ("Gianni") — un nickname DIVERSO. Se l'arm ricalcolasse
    /// (bug: `from_label.ends_with("-ai")` per `is_ai` darebbe lo stesso risultato per
    /// caso, ma `display_name` risolverebbe al nickname LOCALE "Maurizio" invece di
    /// riportare "Gianni"), questo test lo scoprirebbe; con `new_for_test` (nessun
    /// nickname locale configurato) i due comportamenti sarebbero indistinguibili.
    ///
    /// Il servizio resta `Role::Undecided` (nessun peer marcato connesso/ammesso):
    /// il relay-guard su `admitted` si applica solo da `Role::Server`, quindi non
    /// serve `mark_admitted_for_test` per questo test.
    #[test]
    fn peer_msg_say_passes_through_received_display_name_and_is_ai_without_recomputing() {
        let me = info(10, "skimble");
        let mut s = AiChatService::new_for_test_with_display_names(me, Some("Maurizio".to_string()), Some("Aria".to_string()));
        let from = PeerId(std::net::Ipv4Addr::new(192, 168, 1, 20));
        let msg = ChatMsg::Say {
            from_label: "quaxo-human".into(),
            text: "ciao".into(),
            display_name: Some("Gianni".to_string()), // nickname del MITTENTE, non del ricevente
            is_ai: false,
        };
        let effects = s.handle_event(ServiceEvent::PeerMsg { from, msg });
        let to_ui = effects.iter().find_map(|e| match e {
            Effect::ToUi(ServerMsg::AiChatMessage { from_label, display_name, is_ai, .. }) => {
                Some((from_label.clone(), display_name.clone(), *is_ai))
            }
            _ => None,
        }).expect("atteso un Effect::ToUi(AiChatMessage)");
        assert_eq!(to_ui.0, "quaxo-human");
        assert_eq!(
            to_ui.1,
            Some("Gianni".to_string()),
            "deve riportare il nickname del MITTENTE (già nel messaggio), non risolvere il nostro"
        );
        assert!(!to_ui.2);
    }

    /// Gemello del test sopra sul caso `is_ai=true`/`display_name: None`: un peer che
    /// non ha ancora un `ai_display_name` configurato manda `display_name: None` — il
    /// ricevente (che HA un `ai_display_name` locale) deve riportare `None`, non
    /// ereditare il proprio nickname AI per un'etichetta che non è la sua.
    #[test]
    fn peer_msg_say_passes_through_none_display_name_without_inheriting_local_ai_nickname() {
        let me = info(10, "skimble");
        let mut s = AiChatService::new_for_test_with_display_names(me, None, Some("Aria".to_string()));
        let from = PeerId(std::net::Ipv4Addr::new(192, 168, 1, 20));
        let msg = ChatMsg::Say {
            from_label: "quaxo-ai".into(),
            text: "ciao".into(),
            display_name: None, // il mittente non ha (ancora) un nickname AI configurato
            is_ai: true,
        };
        let effects = s.handle_event(ServiceEvent::PeerMsg { from, msg });
        let to_ui = effects.iter().find_map(|e| match e {
            Effect::ToUi(ServerMsg::AiChatMessage { from_label, display_name, is_ai, .. }) => {
                Some((from_label.clone(), display_name.clone(), *is_ai))
            }
            _ => None,
        }).expect("atteso un Effect::ToUi(AiChatMessage)");
        assert_eq!(to_ui.0, "quaxo-ai");
        assert_eq!(
            to_ui.1, None,
            "non deve ereditare il nostro ai_display_name (\"Aria\") per l'etichetta di un altro peer"
        );
        assert!(to_ui.2);
    }

    /// Task 8b (chiude il Debito #1 lasciato dal Task 8, `TODO(admission-8b)`):
    /// un peer CONNESSO ma NON ANCORA AMMESSO (voto in corso, o mai iniziato) non
    /// deve poter iniettare testo nella stanza. Il server deve ignorare il suo
    /// `Say`: nessun relay agli altri peer, nessun `ToUi` verso la propria UI,
    /// nessun append allo storico.
    ///
    /// Scenario: me=.10 (server), .20/.30 solo CONNESSI (mai passati dal voto di
    /// ammissione — `server_with_two_clients()` usa `mark_connected_for_test`, non
    /// `mark_admitted_for_test`). Poi, come regressione, ammettiamo .20 e rimandiamo
    /// LO STESSO `Say`: stavolta deve passare normalmente (il guard non deve rompere
    /// il caso legittimo).
    #[test]
    fn server_ignores_say_from_non_admitted() {
        let mut s = server_with_two_clients(); // me=.10 server; .20/.30 connessi, NON ammessi
        let from = PeerId(std::net::Ipv4Addr::new(192, 168, 1, 20));
        let msg = ChatMsg::Say { from_label: "m20-human".into(), text: "ciao".into(), display_name: None, is_ai: false };

        let effects = s.handle_event(ServiceEvent::PeerMsg { from, msg: msg.clone() });

        assert!(
            effects.is_empty(),
            "un Say da un peer non ammesso non deve produrre alcun effetto: {effects:?}"
        );
        assert!(
            s.history_for_test().is_empty(),
            "lo storico non deve registrare un Say da un mittente non ammesso: {:?}",
            s.history_for_test()
        );

        // Regressione: una volta ammesso, lo STESSO Say deve passare normalmente.
        s.mark_admitted_for_test(from);
        let effects2 = s.handle_event(ServiceEvent::PeerMsg { from, msg });
        assert!(
            effects2.iter().any(|e| matches!(e,
                Effect::ToUi(ServerMsg::AiChatMessage { from_label, .. }) if from_label == "m20-human")),
            "dopo l'ammissione lo stesso Say deve essere inoltrato alla UI: {effects2:?}"
        );
        assert_eq!(
            s.history_for_test().len(),
            1,
            "lo storico deve registrare il Say una volta il mittente ammesso: {:?}",
            s.history_for_test()
        );
    }

    /// Verifica che il CLIENT, ricevendo un `ChatMsg::Say` da un peer,
    /// faccia solo il forward alla propria UI (NON deve fare relay — quello è compito del server).
    ///
    /// Scenario: me=.30 (client del server .10), riceve "ehi" da .10.
    /// Atteso: UI riceve AiChatMessage; NESSUN SendToPeer.
    #[test]
    fn client_forwards_peer_say_to_ui_only() {
        let mut s = AiChatService::new_for_test(info(30, "macavity"));
        s.handle_event(ServiceEvent::Discovered(info(10, "skimble"), None));
        s.mark_connected_for_test(PeerId(std::net::Ipv4Addr::new(192, 168, 1, 10)));
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<ServerMsg>();
        s.handle_event(ServiceEvent::SetServerTx(tx));
        let from = PeerId(std::net::Ipv4Addr::new(192, 168, 1, 10));
        let msg = ChatMsg::Say { from_label: "skimble-human".into(), text: "ehi".into(), display_name: None, is_ai: false };
        let effects = s.handle_event(ServiceEvent::PeerMsg { from, msg });
        assert!(
            effects.iter().any(|e| matches!(e, Effect::ToUi(ServerMsg::AiChatMessage { .. }))),
            "forward alla UI mancante: {effects:?}"
        );
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::SendToPeer(_, _))),
            "il client non deve fare relay: {effects:?}"
        );
    }

    // -------------------------------------------------------------------------
    // AI Chat — Slice 1b: invocazione remota (@all / @<label>-ai) + flag
    // "ai_participates" (vedi
    // Docs/superpowers/specs/2026-07-02-aichat-ai-participants-design.md §9).
    //
    // Riusa `extract_ai_invocation`/`Invocation` (estesi sopra) e
    // `Effect::InvokeLocalAi` (invariato, Slice 1a). Due punti di rilevazione:
    // - HumanSay (il MIO umano): Own/All/Label(mio) invocano SEMPRE — nessun
    //   gate sul flag, l'atto di scrivere nella propria finestra è già il
    //   consenso.
    // - PeerMsg::Say da un umano REMOTO (`from_label` termina in "-human" —
    //   GUARDIA LOOP): All/Label(mio) invocano SOLO se `ai_participates`.
    // -------------------------------------------------------------------------

    /// `@all` nella PROPRIA finestra (HumanSay) invoca SEMPRE la propria AI,
    /// indipendentemente dal flag `ai_participates` — il proprio umano ha
    /// chiesto, quindi il consenso è implicito (design §9.4).
    #[test]
    fn human_say_at_all_invokes_local_ai_regardless_of_flag() {
        let mut s = AiChatService::new_for_test_with_participation(info(10, "skimble"), false);
        let effects = s.handle_event(ServiceEvent::HumanSay("@all commenta X".into()));
        assert!(
            effects.iter().any(|e| matches!(e, Effect::InvokeLocalAi { .. })),
            "@all dal proprio umano deve invocare SEMPRE, flag OFF incluso: {effects:?}"
        );
    }

    /// `@<mio-label>-ai` nella PROPRIA finestra invoca la propria AI (il mio
    /// umano mi ha targetato esplicitamente).
    #[test]
    fn human_say_at_own_label_ai_invokes_local_ai() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let effects = s.handle_event(ServiceEvent::HumanSay("@skimble-ai commenta X".into()));
        assert!(
            effects.iter().any(|e| matches!(e, Effect::InvokeLocalAi { .. })),
            "@<mio-label>-ai deve invocare la mia AI: {effects:?}"
        );
    }

    /// `@<altro-label>-ai` nella PROPRIA finestra NON invoca la mia AI — il
    /// mio umano ha targetato una macchina diversa; il `Say` relayato la
    /// raggiungerà (la macchina di destinazione reagirà dal ramo `PeerMsg`).
    #[test]
    fn human_say_at_other_label_ai_does_not_invoke_local_ai() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let effects = s.handle_event(ServiceEvent::HumanSay("@quaxo-ai commenta X".into()));
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::InvokeLocalAi { .. })),
            "@<altro-label>-ai non deve invocare la mia AI: {effects:?}"
        );
    }

    /// Un `PeerMsg::Say` da un umano REMOTO (`-human`) con `@all` invoca la
    /// mia AI SOLO se `ai_participates == true`.
    #[test]
    fn peer_say_from_human_at_all_invokes_when_participates_on() {
        let mut s = AiChatService::new_for_test(info(10, "skimble")); // participates=true default
        let from = PeerId(std::net::Ipv4Addr::new(192, 168, 1, 20));
        let msg = ChatMsg::Say { from_label: "quaxo-human".into(), text: "@all commenta X".into(), display_name: None, is_ai: false };
        let effects = s.handle_event(ServiceEvent::PeerMsg { from, msg });
        assert!(
            effects.iter().any(|e| matches!(e, Effect::InvokeLocalAi { .. })),
            "@all da umano remoto con flag ON deve invocare: {effects:?}"
        );
    }

    /// Stesso scenario, ma con `ai_participates == false`: nessuna
    /// invocazione — l'autonomia della macchina prevale sulla richiesta remota.
    #[test]
    fn peer_say_from_human_at_all_does_not_invoke_when_participates_off() {
        let mut s = AiChatService::new_for_test_with_participation(info(10, "skimble"), false);
        let from = PeerId(std::net::Ipv4Addr::new(192, 168, 1, 20));
        let msg = ChatMsg::Say { from_label: "quaxo-human".into(), text: "@all commenta X".into(), display_name: None, is_ai: false };
        let effects = s.handle_event(ServiceEvent::PeerMsg { from, msg });
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::InvokeLocalAi { .. })),
            "@all da umano remoto con flag OFF NON deve invocare: {effects:?}"
        );
    }

    /// GUARDIA LOOP (load-bearing): un `PeerMsg::Say` da un'AI (`-ai`) — anche
    /// se il testo contiene letteralmente `@all` — NON deve MAI invocare la
    /// mia AI. Senza questa guardia un'AI potrebbe innescarne un'altra,
    /// aprendo un loop cross-macchina (il design §9.2 la chiama esplicitamente
    /// "load-bearing").
    #[test]
    fn peer_say_from_ai_never_invokes_even_with_at_all_in_text() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let from = PeerId(std::net::Ipv4Addr::new(192, 168, 1, 20));
        let msg = ChatMsg::Say { from_label: "quaxo-ai".into(), text: "@all commenta X".into(), display_name: None, is_ai: false };
        let effects = s.handle_event(ServiceEvent::PeerMsg { from, msg });
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::InvokeLocalAi { .. })),
            "una risposta -ai relayata non deve MAI invocare (guardia loop): {effects:?}"
        );
    }

    /// Un `PeerMsg::Say` da un umano remoto con `@ai` (shorthand `Own`) è
    /// IGNORATO: non riguarda me — è lo shorthand "la MIA AI" del MITTENTE,
    /// non un'invocazione verso la mia macchina.
    #[test]
    fn peer_say_from_human_at_ai_own_is_ignored() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let from = PeerId(std::net::Ipv4Addr::new(192, 168, 1, 20));
        let msg = ChatMsg::Say { from_label: "quaxo-human".into(), text: "@ai commenta X".into(), display_name: None, is_ai: false };
        let effects = s.handle_event(ServiceEvent::PeerMsg { from, msg });
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::InvokeLocalAi { .. })),
            "@ai (Own) da umano remoto non riguarda me: {effects:?}"
        );
    }

    /// Un `PeerMsg::Say` da un umano remoto con `@<mio-label>-ai` invoca la
    /// mia AI se il flag è ON.
    #[test]
    fn peer_say_from_human_at_my_label_ai_invokes_when_participates_on() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let from = PeerId(std::net::Ipv4Addr::new(192, 168, 1, 20));
        let msg = ChatMsg::Say { from_label: "quaxo-human".into(), text: "@skimble-ai commenta X".into(), display_name: None, is_ai: false };
        let effects = s.handle_event(ServiceEvent::PeerMsg { from, msg });
        assert!(
            effects.iter().any(|e| matches!(e, Effect::InvokeLocalAi { .. })),
            "@<mio-label>-ai da umano remoto con flag ON deve invocare: {effects:?}"
        );
    }

    /// Un `PeerMsg::Say` da un umano remoto con `@<altro-label>-ai` (non il
    /// mio) NON invoca la mia AI — non sono il target dell'invocazione.
    #[test]
    fn peer_say_from_human_at_other_label_ai_does_not_invoke() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let from = PeerId(std::net::Ipv4Addr::new(192, 168, 1, 20));
        let msg = ChatMsg::Say { from_label: "quaxo-human".into(), text: "@rumpleteazer-ai commenta X".into(), display_name: None, is_ai: false };
        let effects = s.handle_event(ServiceEvent::PeerMsg { from, msg });
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::InvokeLocalAi { .. })),
            "@<altro-label>-ai non deve invocare la mia AI: {effects:?}"
        );
    }

    // -------------------------------------------------------------------------
    // AI Chat — Slice 2: auto-partecipazione (giudizio di rilevanza + cap turni).
    // Design: Docs/superpowers/specs/2026-07-02-aichat-ai-participants-design.md §10.
    //
    // Tutti i test qui usano lo `StubAdapter` (via `new_for_test_with_autoparticipate`):
    // `chat_autoparticipate` sullo Stub ritorna SEMPRE `None` (silenzio deterministico,
    // vedi `stub_autoparticipate_is_silent` in `ai_adapter.rs`) — questa slice va
    // costruita e verificata SENZA alcuna chiamata API reale (vedi il brief del task).
    // -------------------------------------------------------------------------

    /// Contatore (design §10.3, testato direttamente sul metodo privato — stesso
    /// stile già usato per `extract_ai_invocation`/`format_transcript` in questo
    /// file): un mittente `-human` azzera, un mittente `-ai` incrementa.
    #[test]
    fn note_room_message_counts_ai_turns_and_resets_on_human() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        s.note_room_message("x-human");
        assert_eq!(s.consecutive_ai_turns_for_test(), 0);
        s.note_room_message("y-ai");
        assert_eq!(s.consecutive_ai_turns_for_test(), 1);
        s.note_room_message("z-ai");
        assert_eq!(s.consecutive_ai_turns_for_test(), 2);
    }

    /// `maybe_autoparticipate` produce `Effect::AutoParticipate` quando
    /// `ai_autoparticipate` è ON e nessun'altra guardia lo blocca (stato iniziale
    /// pulito: nessun turno pregresso, nessun giudizio in volo).
    #[test]
    fn maybe_autoparticipate_triggers_when_flag_on() {
        let mut s = AiChatService::new_for_test_with_autoparticipate(info(10, "skimble"), true);
        let effects = s.maybe_autoparticipate("quaxo-human");
        assert!(
            matches!(effects.as_slice(), [Effect::AutoParticipate { .. }]),
            "atteso un singolo Effect::AutoParticipate: {effects:?}"
        );
    }

    /// Guardia 1 (design §10.3): con `ai_autoparticipate` OFF (default), nessun
    /// trigger — qualunque sia lo stato delle altre guardie.
    #[test]
    fn maybe_autoparticipate_does_not_trigger_when_flag_off() {
        let mut s = AiChatService::new_for_test(info(10, "skimble")); // autoparticipate=false
        let effects = s.maybe_autoparticipate("quaxo-human");
        assert!(effects.is_empty(), "flag OFF: nessun trigger atteso: {effects:?}");
    }

    /// Guardia 2 (design §10.3): un messaggio dalla MIA stessa `-ai` non fa mai
    /// scattare un giudizio — non valuto la rilevanza dei miei stessi contributi.
    #[test]
    fn maybe_autoparticipate_never_triggers_from_own_ai_message() {
        let mut s = AiChatService::new_for_test_with_autoparticipate(info(10, "skimble"), true);
        let effects = s.maybe_autoparticipate("skimble-ai"); // il mio label_base è "skimble"
        assert!(effects.is_empty(), "il mio -ai non deve mai auto-invitarsi: {effects:?}");
    }

    /// Guardia 3 (design §10.3): se l'ultimo a parlare nella stanza ero io (la mia
    /// `-ai`), non mi invito a contribuire di nuovo — anche se il trigger arriva
    /// "nominalmente" da un altro mittente (caso limite testato direttamente sul
    /// guard, per isolarlo dalla guardia 2 sopra).
    #[test]
    fn maybe_autoparticipate_skips_when_last_speaker_was_own_ai() {
        let mut s = AiChatService::new_for_test_with_autoparticipate(info(10, "skimble"), true);
        s.note_room_message("skimble-ai"); // imposta last_speaker_label = "skimble-ai"
        let effects = s.maybe_autoparticipate("quaxo-human");
        assert!(
            effects.is_empty(),
            "non parlo due volte di fila (ultimo a parlare ero io): {effects:?}"
        );
    }

    /// Guardia 4 — il CAP (design §10.3, il "controllo del loop" della slice): con
    /// `consecutive_ai_turns == MAX_CONSECUTIVE_AI_TURNS`, il trigger è soppresso.
    /// Confine ESATTO: la condizione è `<`, quindi il cap deve scattare già al
    /// valore uguale al massimo (non solo "oltre").
    #[test]
    fn maybe_autoparticipate_suppressed_at_cap() {
        let mut s = AiChatService::new_for_test_with_autoparticipate(info(10, "skimble"), true);
        s.set_consecutive_ai_turns_for_test(MAX_CONSECUTIVE_AI_TURNS);
        let effects = s.maybe_autoparticipate("quaxo-human");
        assert!(effects.is_empty(), "cap raggiunto: trigger deve essere soppresso: {effects:?}");
    }

    /// Un turno IN PIÙ sotto il cap (`MAX - 1`) deve ancora scattare — verifica che
    /// il confine non sia "off by one" nella direzione opposta.
    #[test]
    fn maybe_autoparticipate_still_triggers_just_below_cap() {
        let mut s = AiChatService::new_for_test_with_autoparticipate(info(10, "skimble"), true);
        s.set_consecutive_ai_turns_for_test(MAX_CONSECUTIVE_AI_TURNS - 1);
        let effects = s.maybe_autoparticipate("quaxo-human");
        assert!(
            matches!(effects.as_slice(), [Effect::AutoParticipate { .. }]),
            "un turno sotto il cap deve ancora scattare: {effects:?}"
        );
    }

    /// Dopo il cap, un messaggio `-human` azzera il contatore (`note_room_message`)
    /// e l'auto-partecipazione riscatta — il cap non è un blocco permanente, solo
    /// per-burst (design §10.3: "un qualunque messaggio umano riapre la
    /// partecipazione").
    #[test]
    fn maybe_autoparticipate_reopens_after_human_message_resets_cap() {
        let mut s = AiChatService::new_for_test_with_autoparticipate(info(10, "skimble"), true);
        s.set_consecutive_ai_turns_for_test(MAX_CONSECUTIVE_AI_TURNS);
        s.note_room_message("quaxo-human"); // reset a 0
        let effects = s.maybe_autoparticipate("quaxo-human");
        assert!(
            matches!(effects.as_slice(), [Effect::AutoParticipate { .. }]),
            "un -human deve riaprire la partecipazione dopo il cap: {effects:?}"
        );
    }

    /// Guardia 5 (design §10.3): con un giudizio già in volo, nessun secondo
    /// trigger — un solo giudizio alla volta per macchina.
    #[test]
    fn maybe_autoparticipate_skips_when_already_inflight() {
        let mut s = AiChatService::new_for_test_with_autoparticipate(info(10, "skimble"), true);
        s.set_autoparticipate_inflight_for_test(true);
        let effects = s.maybe_autoparticipate("quaxo-human");
        assert!(effects.is_empty(), "un giudizio già in volo deve sopprimere: {effects:?}");
    }

    /// Un trigger riuscito marca `autoparticipate_inflight = true` — è la guardia 5
    /// che protegge le chiamate SUCCESSIVE finché `AutoParticipateDone` non arriva.
    #[test]
    fn maybe_autoparticipate_marks_inflight_on_trigger() {
        let mut s = AiChatService::new_for_test_with_autoparticipate(info(10, "skimble"), true);
        assert!(!s.autoparticipate_inflight_for_test());
        s.maybe_autoparticipate("quaxo-human");
        assert!(
            s.autoparticipate_inflight_for_test(),
            "un trigger riuscito deve marcare inflight=true"
        );
    }

    /// Integrazione end-to-end (pura): un `HumanSay` normale (nessun `@ai`/`@all`)
    /// con `ai_autoparticipate` ON produce `Effect::AutoParticipate` — la Slice 2
    /// fa esattamente ciò che la 1a/1b non fa mai (reagire a un messaggio NORMALE).
    #[test]
    fn human_say_triggers_autoparticipate_when_flag_on() {
        let mut s = AiChatService::new_for_test_with_autoparticipate(info(10, "skimble"), true);
        let effects = s.handle_event(ServiceEvent::HumanSay("che tempo fa oggi?".into()));
        assert!(
            effects.iter().any(|e| matches!(e, Effect::AutoParticipate { .. })),
            "HumanSay con flag ON deve produrre AutoParticipate: {effects:?}"
        );
    }

    /// Stesso scenario con `ai_autoparticipate` OFF (default di `new_for_test`):
    /// nessun `Effect::AutoParticipate`.
    #[test]
    fn human_say_does_not_trigger_autoparticipate_when_flag_off() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let effects = s.handle_event(ServiceEvent::HumanSay("che tempo fa oggi?".into()));
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::AutoParticipate { .. })),
            "HumanSay con flag OFF non deve produrre AutoParticipate: {effects:?}"
        );
    }

    /// `AutoParticipateDone { reply: Some(text) }` pubblica il testo come `Say` da
    /// `"<label_base>-ai"` (stesso percorso di `AiReply`) E azzera
    /// `autoparticipate_inflight`.
    #[test]
    fn autoparticipate_done_some_publishes_ai_reply_and_clears_inflight() {
        let mut s = AiChatService::new_for_test_with_autoparticipate(info(10, "skimble"), true);
        s.set_autoparticipate_inflight_for_test(true);

        let effects = s.handle_event(ServiceEvent::AutoParticipateDone {
            reply: Some("occhio a quel comando".into()),
        });

        assert!(
            !s.autoparticipate_inflight_for_test(),
            "AutoParticipateDone deve sempre azzerare inflight"
        );
        assert!(
            effects.iter().any(|e| matches!(e,
                Effect::ToUi(ServerMsg::AiChatMessage { from_label, text, .. })
                    if from_label == "skimble-ai" && text == "occhio a quel comando"
            )),
            "atteso ToUi(AiChatMessage) da skimble-ai: {effects:?}"
        );
        assert!(
            s.history_for_test()
                .iter()
                .any(|l| l.from_label == "skimble-ai" && l.text == "occhio a quel comando"),
            "storico deve contenere il contributo spontaneo: {:?}",
            s.history_for_test()
        );
    }

    /// `AutoParticipateDone { reply: None }` (il caso PIÙ comune — l'AI sceglie il
    /// silenzio) azzera `autoparticipate_inflight` SENZA pubblicare alcun messaggio.
    /// Questo è il punto critico segnalato in review: se `perform` scartasse
    /// silenziosamente l'evento invece di mandarlo sempre, `inflight` resterebbe
    /// bloccato a `true` per sempre dopo il primo silenzio.
    #[test]
    fn autoparticipate_done_none_clears_inflight_without_publishing() {
        let mut s = AiChatService::new_for_test_with_autoparticipate(info(10, "skimble"), true);
        s.set_autoparticipate_inflight_for_test(true);

        let effects = s.handle_event(ServiceEvent::AutoParticipateDone { reply: None });

        assert!(!s.autoparticipate_inflight_for_test(), "None deve comunque azzerare inflight");
        assert!(effects.is_empty(), "nessun messaggio atteso per un silenzio: {effects:?}");
    }

    /// Un testo tutto spazi (`"   "`) è trattato come un silenzio (stessa guardia già
    /// usata da `perform_invoke_local_ai_skips_empty_reply` in Slice 1a): niente
    /// riga di chat fantasma, ma `inflight` si azzera comunque.
    #[test]
    fn autoparticipate_done_blank_text_clears_inflight_without_publishing() {
        let mut s = AiChatService::new_for_test_with_autoparticipate(info(10, "skimble"), true);
        s.set_autoparticipate_inflight_for_test(true);

        let effects = s.handle_event(ServiceEvent::AutoParticipateDone {
            reply: Some("   ".into()),
        });

        assert!(!s.autoparticipate_inflight_for_test());
        assert!(effects.is_empty(), "testo vuoto/spazi: nessun messaggio atteso: {effects:?}");
    }

    /// Slice 2 (memoria persistente): stesso marker `MEMORIA: `, stesso effetto in
    /// più, ma sul percorso dell'auto-partecipazione (nessun umano ha invocato
    /// nulla) — la scelta di ricordare resta comunque sempre dell'AI stessa.
    #[test]
    fn autoparticipate_done_with_marker_also_produces_persist_memory_effect() {
        let mut s = AiChatService::new_for_test(info(10, "rumpleteazer"));
        let effects = s.handle_event(ServiceEvent::AutoParticipateDone {
            reply: Some("Occhio.\nMEMORIA: attenzione ai comandi distruttivi.".to_string()),
        });
        assert!(
            effects.iter().any(|e| matches!(e, Effect::PersistMemory { note, .. } if note == "attenzione ai comandi distruttivi.")),
            "atteso Effect::PersistMemory: {effects:?}"
        );
    }

    /// `perform(Effect::AutoParticipate)` con lo `StubAdapter` (via
    /// `new_for_test_with_autoparticipate`) chiama `chat_autoparticipate` (che sullo
    /// Stub ritorna sempre `None`) e re-inietta l'esito come
    /// `ServiceEvent::AutoParticipateDone { reply: None }` — verifica il collegamento
    /// end-to-end `perform` → inbox, e soprattutto che l'evento arrivi SEMPRE (non
    /// solo per un `Some`, a differenza di `Effect::InvokeLocalAi`).
    #[tokio::test]
    async fn perform_autoparticipate_sends_done_event_even_for_stub_none() {
        let mut s = AiChatService::new_for_test_with_autoparticipate(info(10, "skimble"), true);
        let (inbox_tx, mut inbox_rx) = tokio::sync::mpsc::unbounded_channel::<ServiceEvent>();
        let (new_link_tx, _new_link_rx) = tokio::sync::mpsc::unbounded_channel();
        let (connect_res_tx, _connect_res_rx) = tokio::sync::mpsc::unbounded_channel();
        let shutdown = tokio_util::sync::CancellationToken::new();

        s.perform(
            Effect::AutoParticipate {
                history: vec![Message::user_text("skimble-human: ciao a tutti")],
                my_ai_label: "skimble-ai".into(),
            },
            &inbox_tx,
            &new_link_tx,
            &connect_res_tx,
            &shutdown,
        )
        .await;

        let ev = inbox_rx
            .recv()
            .await
            .expect("atteso un ServiceEvent::AutoParticipateDone re-iniettato sull'inbox");
        match ev {
            ServiceEvent::AutoParticipateDone { reply } => {
                assert_eq!(reply, None, "lo Stub deve tacere: {reply:?}")
            }
            other => panic!("atteso AutoParticipateDone, trovato {other:?}"),
        }
    }

    /// Adapter di test che ritorna un `Some(..)` fisso da `chat_autoparticipate` —
    /// simmetrico a `EmptyReplyAdapter` sopra (Slice 1a), qui per il ramo "l'AI ha
    /// davvero qualcosa da dire". `respond`/`chat_reply` non sono esercitati:
    /// implementazione minima (no-op/stringa vuota), come `EmptyReplyAdapter`.
    struct AutoParticipateRespondsAdapter(String);

    #[async_trait::async_trait]
    impl AiAdapter for AutoParticipateRespondsAdapter {
        #[allow(clippy::too_many_arguments)]
        async fn respond(
            &self,
            _id: &str,
            _input: &str,
            _history: &mut crate::messages_client::ConversationHistory,
            _tools: &dyn crate::tool_client::ToolClient,
            _opts: crate::agent::TurnOptions,
            _confirmer: Option<&dyn crate::ai_adapter::ToolConfirmer>,
            _cancel: Option<tokio_util::sync::CancellationToken>,
            _tx: tokio::sync::mpsc::UnboundedSender<ServerMsg>,
        ) {
        }

        async fn chat_reply(
            &self,
            _my_ai_label: &str,
            _history: &[crate::messages_client::Message],
            _request: &str,
            _cancel: Option<tokio_util::sync::CancellationToken>,
        ) -> String {
            String::new()
        }

        async fn chat_autoparticipate(
            &self,
            _my_ai_label: &str,
            _history: &[crate::messages_client::Message],
            _cancel: Option<tokio_util::sync::CancellationToken>,
        ) -> Option<String> {
            Some(self.0.clone())
        }
    }

    /// `perform(Effect::AutoParticipate)` con un adapter che RISPONDE (non tace)
    /// re-inietta `AutoParticipateDone { reply: Some(text) }` — il testo passa
    /// intatto dall'adapter all'evento, pronto per essere pubblicato da
    /// `handle_event` (verificato separatamente da
    /// `autoparticipate_done_some_publishes_ai_reply_and_clears_inflight`).
    #[tokio::test]
    async fn perform_autoparticipate_sends_done_event_with_some_text() {
        let mut s = AiChatService::new(
            info(10, "skimble"),
            Arc::new(AutoParticipateRespondsAdapter("buona osservazione".into())),
            true,
            true,
            NotesStore::empty_in_memory(),
            None,
            None,
            "unused".into(),
        );
        let (inbox_tx, mut inbox_rx) = tokio::sync::mpsc::unbounded_channel::<ServiceEvent>();
        let (new_link_tx, _new_link_rx) = tokio::sync::mpsc::unbounded_channel();
        let (connect_res_tx, _connect_res_rx) = tokio::sync::mpsc::unbounded_channel();
        let shutdown = tokio_util::sync::CancellationToken::new();

        s.perform(
            Effect::AutoParticipate {
                history: vec![Message::user_text("skimble-human: che ne pensate?")],
                my_ai_label: "skimble-ai".into(),
            },
            &inbox_tx,
            &new_link_tx,
            &connect_res_tx,
            &shutdown,
        )
        .await;

        let ev = inbox_rx.recv().await.expect("atteso AutoParticipateDone");
        match ev {
            ServiceEvent::AutoParticipateDone { reply } => {
                assert_eq!(reply, Some("buona osservazione".to_string()))
            }
            other => panic!("atteso AutoParticipateDone, trovato {other:?}"),
        }
    }

    /// Un `PeerMsg::Say` da un'AI di un ALTRO peer (`quaxo-ai`) PUÒ far scattare la
    /// mia auto-partecipazione: a differenza della guardia loop di Slice 1b
    /// (`from_label.ends_with("-human")`, che riguarda SOLO l'invocazione esplicita),
    /// qui un messaggio `-ai` altrui è un contributo legittimo della conversazione
    /// AI↔AI che la Slice 2 abilita — bounded dal cap, non dal filtro sul mittente.
    #[test]
    fn peer_say_from_other_ai_can_trigger_my_autoparticipate() {
        let mut s = AiChatService::new_for_test_with_autoparticipate(info(10, "skimble"), true);
        let from = PeerId(std::net::Ipv4Addr::new(192, 168, 1, 20));
        let msg = ChatMsg::Say { from_label: "quaxo-ai".into(), text: "credo che X sia rilevante".into(), display_name: None, is_ai: false };
        let effects = s.handle_event(ServiceEvent::PeerMsg { from, msg });
        assert!(
            effects.iter().any(|e| matches!(e, Effect::AutoParticipate { .. })),
            "un -ai altrui deve poter innescare la mia auto-partecipazione: {effects:?}"
        );
    }

    /// GUARDIA LOOP condivisa con Slice 1b: un `PeerMsg::Say` dalla MIA stessa `-ai`
    /// (rara — accadrebbe solo per un loopback anomalo) non innesca mai
    /// l'auto-partecipazione (guardia 2 di `maybe_autoparticipate`).
    #[test]
    fn peer_say_from_own_ai_label_never_triggers_autoparticipate() {
        let mut s = AiChatService::new_for_test_with_autoparticipate(info(10, "skimble"), true);
        let from = PeerId(std::net::Ipv4Addr::new(192, 168, 1, 20));
        let msg = ChatMsg::Say { from_label: "skimble-ai".into(), text: "eco di me stesso".into(), display_name: None, is_ai: false };
        let effects = s.handle_event(ServiceEvent::PeerMsg { from, msg });
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::AutoParticipate { .. })),
            "il mio -ai relayato non deve mai auto-invitarsi: {effects:?}"
        );
    }

    // -------------------------------------------------------------------------
    // AI Chat — Slice 2b/2c: sopprimi il "double-fire" e le invocazioni altrui.
    //
    // Problema originale (Slice 2b): con un'invocazione esplicita (@ai/@all/
    // @<mio-label>-ai) E `ai_autoparticipate` ON, un singolo messaggio produceva
    // SIA `Effect::InvokeLocalAi` (risposta all'invocazione, Slice 1a/1b) SIA
    // `Effect::AutoParticipate` (auto-giudizio, Slice 2) — due turni AI per un
    // solo messaggio umano (costo doppio, ridondante, anche se bounded dal cap).
    //
    // AGGIORNATO (Slice 2c, 2026-07-08, bug osservato dal vivo — vedi
    // Docs/superpowers/specs/2026-07-08-aichat-autoparticipate-explicit-invocation-design.md):
    // il gate NON guarda più "il messaggio ha GIÀ prodotto un `InvokeLocalAi` PER
    // ME" (`already_invoked`, rimosso) — guarda invece "il messaggio ERA
    // un'invocazione esplicita, chiunque essa targetasse" (`was_explicit_invocation`).
    // Un'invocazione esplicita ha SEMPRE un destinatario chiaro, anche quando
    // quel destinatario non sono io: `maybe_autoparticipate` non scatta MAI su un
    // messaggio del genere, non solo su quelli che mi invocavano direttamente.
    // -------------------------------------------------------------------------

    /// `HumanSay("@ai ...")` con `ai_autoparticipate` ON: l'invocazione esplicita
    /// produce `InvokeLocalAi` come sempre, ma l'auto-partecipazione sullo STESSO
    /// messaggio deve essere soppressa.
    #[test]
    fn human_say_at_ai_with_auto_on_does_not_double_fire() {
        let mut s = AiChatService::new_for_test_with_autoparticipate(info(10, "skimble"), true);
        let effects = s.handle_event(ServiceEvent::HumanSay("@ai fai qualcosa".into()));
        assert!(
            effects.iter().any(|e| matches!(e, Effect::InvokeLocalAi { .. })),
            "@ai deve comunque invocare la mia AI: {effects:?}"
        );
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::AutoParticipate { .. })),
            "l'invocazione esplicita deve sopprimere l'auto-partecipazione sullo stesso messaggio: {effects:?}"
        );
    }

    /// Invariante preservata: un messaggio umano NORMALE (senza @) con
    /// `ai_autoparticipate` ON continua a produrre `AutoParticipate` — nessuna
    /// invocazione esplicita in questo messaggio, quindi niente da sopprimere.
    #[test]
    fn human_say_plain_with_auto_on_still_autoparticipates() {
        let mut s = AiChatService::new_for_test_with_autoparticipate(info(10, "skimble"), true);
        let effects = s.handle_event(ServiceEvent::HumanSay("ciao a tutti".into()));
        assert!(
            effects.iter().any(|e| matches!(e, Effect::AutoParticipate { .. })),
            "un messaggio normale deve ancora poter auto-partecipare: {effects:?}"
        );
    }

    /// Stesso fix lato `PeerMsg::Say`: un umano remoto (`X-human`) che scrive
    /// `@all`, con `ai_participates` E `ai_autoparticipate` entrambi ON, produce
    /// `InvokeLocalAi` ma NON `AutoParticipate` sullo stesso messaggio.
    #[test]
    fn peer_say_at_all_with_auto_on_does_not_double_fire() {
        let mut s = AiChatService::new_for_test_with_autoparticipate(info(10, "skimble"), true);
        let from = PeerId(std::net::Ipv4Addr::new(192, 168, 1, 20));
        let msg = ChatMsg::Say { from_label: "quaxo-human".into(), text: "@all commenta X".into(), display_name: None, is_ai: false };
        let effects = s.handle_event(ServiceEvent::PeerMsg { from, msg });
        assert!(
            effects.iter().any(|e| matches!(e, Effect::InvokeLocalAi { .. })),
            "@all da umano remoto deve comunque invocare: {effects:?}"
        );
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::AutoParticipate { .. })),
            "l'invocazione remota deve sopprimere l'auto-partecipazione sullo stesso messaggio: {effects:?}"
        );
    }

    /// AGGIORNATO (2026-07-08, bug osservato dal vivo): `@<altro-label>-ai` (non il mio,
    /// da un umano remoto) non produce `InvokeLocalAi` PER ME (non sono il target), MA è
    /// comunque un'invocazione esplicita — qualcuno ha invocato QUALCUN ALTRO, non
    /// "nessuno". L'auto-partecipazione NON deve scattare: il system prompt dedicato
    /// dichiara "NESSUNO ti ha invocato esplicitamente", falso in questo caso.
    #[test]
    fn peer_say_at_other_label_with_auto_on_does_not_autoparticipate() {
        let mut s = AiChatService::new_for_test_with_autoparticipate(info(10, "skimble"), true);
        let from = PeerId(std::net::Ipv4Addr::new(192, 168, 1, 20));
        let msg = ChatMsg::Say { from_label: "quaxo-human".into(), text: "@rumpleteazer-ai commenta X".into(), display_name: None, is_ai: false };
        let effects = s.handle_event(ServiceEvent::PeerMsg { from, msg });
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::InvokeLocalAi { .. })),
            "@<altro-label>-ai non deve invocare la mia AI: {effects:?}"
        );
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::AutoParticipate { .. })),
            "un'invocazione esplicita verso un'altra macchina deve sopprimere la mia auto-partecipazione: {effects:?}"
        );
    }

    /// Scenario ESATTO osservato dal vivo (2026-07-08): un umano REMOTO scrive `@ai`
    /// bare (`InvokeTarget::Own`, sempre "la MIA AI" dal punto di vista del mittente —
    /// mai la mia). Con `ai_autoparticipate` ON, la mia AI rispondeva comunque tramite
    /// auto-partecipazione, duplicando la risposta del peer effettivamente invocato.
    #[test]
    fn peer_say_bare_at_ai_with_auto_on_does_not_autoparticipate() {
        let mut s = AiChatService::new_for_test_with_autoparticipate(info(10, "rumpleteazer"), true);
        let from = PeerId(std::net::Ipv4Addr::new(192, 168, 1, 20));
        let msg = ChatMsg::Say { from_label: "skimble-human".into(), text: "@ai elenchi 4 verbi inglesi irregolari?".into(), display_name: None, is_ai: false };
        let effects = s.handle_event(ServiceEvent::PeerMsg { from, msg });
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::InvokeLocalAi { .. })),
            "@ai bare da remoto non deve mai invocare un'altra macchina: {effects:?}"
        );
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::AutoParticipate { .. })),
            "un @ai bare, per quanto rivolto ad un'altra macchina, resta un'invocazione esplicita: {effects:?}"
        );
    }

    // -------------------------------------------------------------------------
    // Task 8: la sezione "Scenario di Test per Task 6: Discovered + Consent +
    // PeerGone" che stava qui (`first_discovery_of_new_peer_asks_consent`) testava
    // ESATTAMENTE l'invariante opposta a quella voluta ora: verificava che
    // `Discovered` NON producesse alcun effetto di connessione prima del consenso
    // dell'umano ("consent-before-accept"). Quel gate pre-connessione è stato
    // rimosso (vedi l'arm `Discovered` in `handle_event`): ora la connessione
    // TCP parte SUBITO alla scoperta — il gate reale si è spostato DOPO la
    // connessione, a livello applicativo (gate 1 del nuovo arrivato + voto di
    // ammissione dei presenti, Task 4-6). L'invariante opposta e corretta ("un
    // `Discovered` produce SUBITO gli effetti di connessione giusti") è già
    // coperta da `discovered_lowest_ip_becomes_server_and_starts_listener` e
    // `discovered_higher_ip_becomes_client_and_connects_to_server` (sopra).
    // -------------------------------------------------------------------------

    // -------------------------------------------------------------------------
    // Bug 2 (late-joiner): `reported_leader` — vedi
    // Docs/superpowers/specs/2026-07-02-aichat-late-joiner-election-design.md §3.3
    // -------------------------------------------------------------------------

    /// `Discovered` con `leader: Some(x)` deve aggiornare `reported_leader`.
    #[test]
    fn discovered_with_leader_some_updates_reported_leader() {
        let mut s = AiChatService::new_for_test(info(10, "quaxo"));
        assert_eq!(s.reported_leader_for_test(), None);
        s.handle_event(ServiceEvent::Discovered(info(20, "rumpleteazer"), Some(info(20, "rumpleteazer").id)));
        assert_eq!(s.reported_leader_for_test(), Some(info(20, "rumpleteazer").id));
    }

    /// `Discovered` con `leader: None` NON deve cancellare un `reported_leader` già
    /// valorizzato da un annuncio precedente (un peer ancora `Undecided` non deve
    /// "azzerare" un'informazione più utile ricevuta da un altro peer).
    #[test]
    fn discovered_with_leader_none_does_not_clear_existing_reported_leader() {
        let mut s = AiChatService::new_for_test(info(10, "quaxo"));
        s.handle_event(ServiceEvent::Discovered(info(20, "rumpleteazer"), Some(info(20, "rumpleteazer").id)));
        assert_eq!(s.reported_leader_for_test(), Some(info(20, "rumpleteazer").id));

        // Un secondo peer si annuncia ma non riporta ancora un leader (Undecided).
        s.handle_event(ServiceEvent::Discovered(info(30, "skimble"), None));
        assert_eq!(
            s.reported_leader_for_test(),
            Some(info(20, "rumpleteazer").id),
            "un annuncio con leader:None non deve cancellare il reported_leader esistente"
        );
    }

    // -------------------------------------------------------------------------
    // Keepalive: `ServiceEvent::Tick` → un `Ping` per ogni peer connesso.
    // Vedi Docs/superpowers/specs/2026-07-02-aichat-keepalive-design.md §4.
    // -------------------------------------------------------------------------

    #[test]
    fn tick_sends_ping_to_every_connected_peer() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        s.mark_connected_for_test(PeerId(Ipv4Addr::new(192, 168, 1, 20)));
        s.mark_connected_for_test(PeerId(Ipv4Addr::new(192, 168, 1, 30)));

        let effects = s.handle_event(ServiceEvent::Tick);

        assert_eq!(effects.len(), 2, "un Ping per peer connesso: {:?}", effects);
        assert!(effects.iter().any(|e| matches!(e,
            Effect::SendToPeer(p, ChatMsg::Ping {}) if *p == PeerId(Ipv4Addr::new(192, 168, 1, 20)))));
        assert!(effects.iter().any(|e| matches!(e,
            Effect::SendToPeer(p, ChatMsg::Ping {}) if *p == PeerId(Ipv4Addr::new(192, 168, 1, 30)))));
    }

    #[test]
    fn tick_with_no_connected_peers_emits_nothing() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let effects = s.handle_event(ServiceEvent::Tick);
        assert_eq!(effects, Vec::new());
    }

    /// Verifica che la scoperta di un peer con IP più basso diventi Server e avvii il listener.
    ///
    /// Regola di elezione: l'IP più basso del roster (me + TUTTI i peer scoperti — Task 8,
    /// non più solo i "consentiti") è eletto server. me=.10 (IP più basso), peer=.20 →
    /// dopo `Discovered`, me deve essere Server e l'effetto `StartListener(40100)` deve
    /// essere prodotto. (Il vecchio `Consent` che seguiva `Discovered` è stato rimosso:
    /// `Discovered` da sola ora rielegge — vedi il suo doc-comment in `handle_event`.)
    #[test]
    fn discovered_lowest_ip_becomes_server_and_starts_listener() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let effects = s.handle_event(ServiceEvent::Discovered(info(20, "macavity"), None));
        assert_eq!(s_role(&s), Role::Server, "me=.10 dovrebbe essere Server: ruolo={:?}", s_role(&s));
        assert!(
            effects.iter().any(|e| matches!(e, Effect::StartListener(40100))),
            "StartListener mancante: {effects:?}"
        );
    }

    /// Verifica che la scoperta di un peer con IP più basso del proprio faccia diventare
    /// Client e si connetta al server.
    ///
    /// me=.30 (IP più alto), peer=.10 (IP più basso) → dopo `Discovered`, me è Client
    /// e l'effetto `ConnectTo(peer con label "skimble")` deve essere prodotto.
    #[test]
    fn discovered_higher_ip_becomes_client_and_connects_to_server() {
        let mut s = AiChatService::new_for_test(info(30, "macavity"));
        let effects = s.handle_event(ServiceEvent::Discovered(info(10, "skimble"), None));
        assert!(
            matches!(s_role(&s), Role::Client(_)),
            "me=.30 dovrebbe essere Client: ruolo={:?}", s_role(&s)
        );
        assert!(
            effects.iter().any(|e| matches!(e, Effect::ConnectTo(p) if p.label_base == "skimble")),
            "ConnectTo mancante o label errata: {effects:?}"
        );
    }

    // -------------------------------------------------------------------------
    // Debito #5: "listening non si resetta" (accept loop morto → server sordo).
    //
    // L'accept loop TCP spawnato in `perform` per `Effect::StartListener` può uscire
    // dal proprio `loop` (errore di `accept()`, o canale verso l'attore chiuso) senza
    // che l'attore lo sappia mai: `self.listening` resta `true` per sempre, e il guard
    // di idempotenza in `perform` (`if self.listening { return }`) blocca qualunque
    // futuro `StartListener`. Questi test verificano SOLO la parte pura (`handle_event`):
    // il nuovo evento `ListenerStopped` deve resettare il flag e, se siamo ancora
    // `Role::Server`, far ripartire subito la rielezione (che ri-emette `StartListener`).
    // -------------------------------------------------------------------------

    /// `ListenerStopped` deve riportare `listening` a `false`, qualunque fosse lo stato
    /// prima (qui lo forziamo a `true` con l'helper di test, senza fare I/O reale).
    #[test]
    fn listener_stopped_resets_listening_flag() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        s.set_listening_for_test(true);

        s.handle_event(ServiceEvent::ListenerStopped);

        assert!(
            !s.listening_for_test(),
            "listening deve tornare false dopo ListenerStopped"
        );
    }

    /// Se siamo ancora `Role::Server` quando l'accept loop muore, `ListenerStopped`
    /// deve far ripartire SUBITO il ri-bind (non aspettare il prossimo `Discovered`,
    /// che nella LAN reale arriva ogni ~5-7s): `handle_event` chiama `decide_and_connect`,
    /// che — vedendo `listening` di nuovo `false` — ri-emette `Effect::StartListener`,
    /// il quale ora supera il guard di idempotenza in `perform` e ri-binda davvero.
    #[test]
    fn listener_stopped_when_server_reemits_start_listener() {
        // me=.20, peer .30 (IP più alto) consentito e connesso → me resta il "lowest"
        // del roster → Server (elezione sticky, stessa logica di `server_with_two_clients`).
        let mut s = AiChatService::new_for_test(info(20, "skimble"));
        s.handle_event(ServiceEvent::Discovered(info(30, "macavity"), None));
        s.mark_connected_for_test(PeerId(Ipv4Addr::new(192, 168, 1, 30)));
        assert_eq!(s_role(&s), Role::Server, "me=.20 dovrebbe essere Server: ruolo={:?}", s_role(&s));

        s.set_listening_for_test(true);
        let effects = s.handle_event(ServiceEvent::ListenerStopped);

        assert!(
            !s.listening_for_test(),
            "listening deve tornare false anche quando siamo Server"
        );
        assert!(
            effects.iter().any(|e| matches!(e, Effect::StartListener(40100))),
            "StartListener mancante dopo ListenerStopped da Server: {effects:?}"
        );
    }

    /// Il SERVER deve comparire nel proprio roster: dopo essere diventato server
    /// (`decide_and_connect`) e AMMESSO un client (Task 8: voto di ammissione, non più
    /// il vecchio `ChatMsg::Join` diretto), il roster broadcastato contiene SIA la
    /// propria "<base>-human" SIA quella del client.
    #[test]
    fn server_roster_includes_own_human_label() {
        let mut s = AiChatService::new_for_test(info(10, "m10")); // me=.10 → server (IP più basso)
        s.handle_event(ServiceEvent::Discovered(info(20, "m20"), None));
        s.mark_connected_for_test(PeerId(std::net::Ipv4Addr::new(192, 168, 1, 20)));
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<ServerMsg>();
        s.handle_event(ServiceEvent::SetServerTx(tx));
        let from = PeerId(std::net::Ipv4Addr::new(192, 168, 1, 20));
        // Task 8: il candidato chiede l'ammissione e il voto si risolve per timeout
        // (silenzio = sì, nessun altro presente da consultare oltre l'umano-server,
        // che qui non vota esplicitamente — vedi `resolve_if_ready`/`VoteTimeout`).
        s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::RequestAdmission { label: "m20-human".into() },
        });
        let gen = s.pending_vote_generation_for_test(from).expect("voto in corso");
        let effects = s.handle_event(ServiceEvent::VoteTimeout { candidate: from, generation: gen });
        let roster = effects
            .iter()
            .find_map(|e| match e {
                Effect::ToUi(ServerMsg::AiChatRoster { participants }) => Some(participants.clone()),
                _ => None,
            })
            .expect("atteso ToUi(AiChatRoster)");
        assert!(roster.contains(&"m10-human".to_string()), "manca self nel roster: {roster:?}");
        assert!(roster.contains(&"m20-human".to_string()), "manca client nel roster: {roster:?}");
    }

    // -------------------------------------------------------------------------
    // Task 8 (rimozione del vecchio consenso pairwise): la sezione "Scenario di Test
    // per Task 7: Join lato server → aggiorna Room + broadcast Roster" che stava qui
    // (`server_join_updates_room_and_broadcasts_roster`) testava l'arm SERVER di
    // `ChatMsg::Join`, ora dead code (nessun peer lo manda più — vedi l'arm
    // `PeerMsg` in `handle_event`, sostituito da `RequestAdmission`/voto). Rimossa:
    // la stessa invariante (Room aggiornato + broadcast Roster dopo un ingresso) è
    // già coperta dai test di Task 5 (`vote_all_yes_admits`,
    // `timeout_counts_missing_as_yes`) e da `server_roster_includes_own_human_label`
    // appena sopra, ora convertito al nuovo meccanismo.
    // -------------------------------------------------------------------------

    // -------------------------------------------------------------------------
    // Scenario di Test: storico messaggi — append su Say (locale e da peer)
    // -------------------------------------------------------------------------

    /// `HumanSay` deve appendere allo storico locale, indipendentemente dal ruolo
    /// (qui il servizio è ancora `Undecided`: nessun peer consentito).
    #[test]
    fn human_say_appends_to_history() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        s.handle_event(ServiceEvent::HumanSay("ciao".into()));
        assert_eq!(
            s.history_for_test(),
            &[ChatLine { from_label: "skimble-human".into(), text: "ciao".into(), display_name: None, is_ai: false }]
        );
    }

    /// Un `Say` ricevuto da un peer connesso E AMMESSO deve appendere allo storico
    /// locale. Task 8b: il relay-guard ignora un Say da un mittente non ammesso — il
    /// caso "non ammesso, storico invariato" ha un test dedicato,
    /// `server_ignores_say_from_non_admitted`; qui verifichiamo il caso legittimo.
    #[test]
    fn peer_say_appends_to_history() {
        let mut s = server_with_two_clients(); // me=.10 server, .20/.30 connessi
        let from = PeerId(Ipv4Addr::new(192, 168, 1, 20));
        s.mark_admitted_for_test(from);
        s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::Say { from_label: "m20-human".into(), text: "ehi".into(), display_name: None, is_ai: false },
        });
        assert_eq!(
            s.history_for_test(),
            &[ChatLine { from_label: "m20-human".into(), text: "ehi".into(), display_name: None, is_ai: false }]
        );
    }

    // -------------------------------------------------------------------------
    // Bug fix: reopen UI — SetServerTx ri-emette il roster noto
    // -------------------------------------------------------------------------

    /// Verifica che `SetServerTx` ri-emetta il roster noto quando la UI viene
    /// riaperta dopo una chiusura (`UiClosed`).
    ///
    /// Caso d'uso: l'utente chiude la finestra-chat e la riapre senza riavviare
    /// l'orchestrator. Il nuovo webview non ha mai ricevuto il `AiChatRoster`
    /// precedente, quindi i "presenti" appaiono vuoti. Il servizio DEVE ri-inviare
    /// l'ultimo roster memorizzato (`last_known_roster`) al momento del `SetServerTx`.
    ///
    /// Scenario: me=.10 (server), .20 si connette e viene AMMESSO (Task 8: voto,
    /// non più il vecchio `ChatMsg::Join` diretto).
    /// Poi la UI si chiude (`UiClosed`) e si riapre (`SetServerTx`).
    /// Il secondo `SetServerTx` DEVE emettere `ToUi(AiChatRoster { ["m10-human", "m20-human"] })`.
    #[test]
    fn set_server_tx_replays_last_roster_on_ui_reopen() {
        // Costruisci scenario: server con un client connesso.
        let mut s = AiChatService::new_for_test(info(10, "m10"));
        s.handle_event(ServiceEvent::Discovered(info(20, "m20"), None));
        s.mark_connected_for_test(PeerId(Ipv4Addr::new(192, 168, 1, 20)));

        // Prima connessione UI: nessun roster ancora nel cache → effetti vuoti (come prima).
        let (tx1, _rx1) = tokio::sync::mpsc::unbounded_channel::<ServerMsg>();
        let effects1 = s.handle_event(ServiceEvent::SetServerTx(tx1));
        assert!(
            !effects1.iter().any(|e| matches!(e, Effect::ToUi(ServerMsg::AiChatRoster { .. }))),
            "primo SetServerTx non deve emettere roster (cache vuota): {effects1:?}"
        );

        // .20 chiede l'ammissione e viene ammesso (silenzio = sì, via timeout) → il
        // server aggiorna il Room e il cache del roster (`resolve_admit`).
        let from = PeerId(Ipv4Addr::new(192, 168, 1, 20));
        s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::RequestAdmission { label: "m20-human".into() },
        });
        let gen = s.pending_vote_generation_for_test(from).expect("voto in corso");
        s.handle_event(ServiceEvent::VoteTimeout { candidate: from, generation: gen });

        // UI si chiude.
        s.handle_event(ServiceEvent::UiClosed);

        // UI si riapre: SetServerTx DEVE ri-emettere il roster noto.
        let (tx2, _rx2) = tokio::sync::mpsc::unbounded_channel::<ServerMsg>();
        let effects2 = s.handle_event(ServiceEvent::SetServerTx(tx2));
        let roster = effects2.iter().find_map(|e| match e {
            Effect::ToUi(ServerMsg::AiChatRoster { participants }) => Some(participants.clone()),
            _ => None,
        });
        let participants = roster.expect("secondo SetServerTx deve emettere AiChatRoster");
        assert!(participants.contains(&"m10-human".to_string()), "manca m10-human: {participants:?}");
        assert!(participants.contains(&"m20-human".to_string()), "manca m20-human: {participants:?}");
    }

    /// Verifica che il CLIENT ri-ottenga il roster dalla cache dopo una riapertura UI.
    ///
    /// Un client riceve il roster tramite `ChatMsg::Roster` dal server.
    /// Se la UI viene chiusa e riaperta, il secondo `SetServerTx` deve ri-emettere
    /// il roster memorizzato (proveniente dall'ultimo `PeerMsg::Roster`).
    #[test]
    fn set_server_tx_replays_roster_received_as_client() {
        // me=.30 (client del server .10)
        let mut s = AiChatService::new_for_test(info(30, "m30"));
        s.handle_event(ServiceEvent::Discovered(info(10, "m10"), None));
        s.mark_connected_for_test(PeerId(Ipv4Addr::new(192, 168, 1, 10)));

        let (tx1, _rx1) = tokio::sync::mpsc::unbounded_channel::<ServerMsg>();
        s.handle_event(ServiceEvent::SetServerTx(tx1));

        // Il server invia un Roster a questo client.
        let from = PeerId(Ipv4Addr::new(192, 168, 1, 10));
        s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::Roster { participants: vec!["m10-human".into(), "m30-human".into()] },
        });

        // UI si chiude e si riapre.
        s.handle_event(ServiceEvent::UiClosed);
        let (tx2, _rx2) = tokio::sync::mpsc::unbounded_channel::<ServerMsg>();
        let effects = s.handle_event(ServiceEvent::SetServerTx(tx2));

        let roster = effects.iter().find_map(|e| match e {
            Effect::ToUi(ServerMsg::AiChatRoster { participants }) => Some(participants.clone()),
            _ => None,
        });
        let participants = roster.expect("SetServerTx deve ri-emettere il roster del client");
        assert!(participants.contains(&"m10-human".to_string()), "manca m10-human: {participants:?}");
        assert!(participants.contains(&"m30-human".to_string()), "manca m30-human: {participants:?}");
    }

    // -------------------------------------------------------------------------
    // Scenario di Test: replay storico su SetServerTx (riapertura finestra)
    // -------------------------------------------------------------------------

    /// Riapertura finestra con storico non vuoto: `SetServerTx` deve ri-emettere
    /// `AiChatHistory` con tutti i messaggi visti finora.
    #[test]
    fn set_server_tx_replays_history_on_ui_reopen() {
        let mut s = AiChatService::new_for_test(info(10, "m10"));
        s.handle_event(ServiceEvent::HumanSay("primo messaggio".into()));

        s.handle_event(ServiceEvent::UiClosed);
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<ServerMsg>();
        let effects = s.handle_event(ServiceEvent::SetServerTx(tx));

        let entries = effects.iter().find_map(|e| match e {
            Effect::ToUi(ServerMsg::AiChatHistory { entries }) => Some(entries.clone()),
            _ => None,
        }).expect("SetServerTx deve ri-emettere lo storico");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text, "primo messaggio");
        assert_eq!(entries[0].from_label, "m10-human");
    }

    /// Storico vuoto (primo avvio, nessun messaggio ancora): `SetServerTx` non deve
    /// emettere `AiChatHistory` — comportamento invariato rispetto a prima di questo task.
    #[test]
    fn set_server_tx_no_history_effect_when_empty() {
        let mut s = AiChatService::new_for_test(info(10, "m10"));
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<ServerMsg>();
        let effects = s.handle_event(ServiceEvent::SetServerTx(tx));
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::ToUi(ServerMsg::AiChatHistory { .. }))),
            "non deve emettere AiChatHistory se lo storico è vuoto: {effects:?}"
        );
    }

    // -------------------------------------------------------------------------
    // Bug fix (2026-07-04): SetServerTx ri-emette il gate di ammissione perso
    // -------------------------------------------------------------------------
    //
    // Scenario dal vivo (2 macchine): il connect verso il server eletto parte da
    // solo all'avvio dell'orchestrator (`decide_and_connect`, innescato dalla
    // scoperta UDP) — quasi sempre PRIMA che l'umano apra la finestra AI Chat,
    // che è un webview separato aperto a parte. `begin_join_gate`/`request_admission`
    // emettono il loro `ToUi` nell'istante del connect: se `server_tx` è ancora
    // `None` in quel momento, l'`Effect::ToUi` viene scartato in silenzio
    // (`perform`, arm `Effect::ToUi`) — e PRIMA di questo fix nessun replay
    // esisteva, lasciando il nuovo arrivato bloccato in `Deciding`/`Pending` per
    // sempre, senza alcun prompt visibile né modo di recuperarlo aprendo la
    // finestra più tardi (verificato dal vivo: riaprire la finestra 8s DOPO
    // l'elezione non mostra comunque nulla).

    /// `SetServerTx` mentre `self_admission == Deciding` deve ri-emettere il gate 1
    /// (`AiChatJoinPrompt`) — stesso principio del replay di roster/storico sopra.
    #[test]
    fn set_server_tx_replays_join_prompt_when_deciding() {
        // me=.30, ha scoperto il server .10 ma la finestra chat non era ancora
        // aperta quando `begin_join_gate` ha marcato Deciding (fuori dal `handle_event`
        // testato qui — usiamo il setter di test per riprodurre lo stato).
        let mut s = AiChatService::new_for_test(info(30, "m30"));
        s.handle_event(ServiceEvent::Discovered(info(10, "m10"), None));
        s.set_self_admission_for_test(SelfAdmission::Deciding);

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<ServerMsg>();
        let effects = s.handle_event(ServiceEvent::SetServerTx(tx));

        let present = effects
            .iter()
            .find_map(|e| match e {
                Effect::ToUi(ServerMsg::AiChatJoinPrompt { present }) => Some(present.clone()),
                _ => None,
            })
            .expect(&format!(
                "SetServerTx deve ri-emettere AiChatJoinPrompt se self_admission è Deciding: {effects:?}"
            ));
        assert!(present.contains(&"m10-human".to_string()), "manca m10-human tra i presenti: {present:?}");
    }

    /// `SetServerTx` mentre `self_admission == Pending` deve ri-emettere l'attesa
    /// del gate 2 (`AiChatPending`) — stesso principio del test sopra.
    #[test]
    fn set_server_tx_replays_pending_when_pending() {
        let mut s = AiChatService::new_for_test(info(30, "m30"));
        s.handle_event(ServiceEvent::Discovered(info(10, "m10"), None));
        s.set_self_admission_for_test(SelfAdmission::Pending);

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<ServerMsg>();
        let effects = s.handle_event(ServiceEvent::SetServerTx(tx));

        assert!(
            effects.iter().any(|e| matches!(e, Effect::ToUi(ServerMsg::AiChatPending { .. }))),
            "SetServerTx deve ri-emettere AiChatPending se self_admission è Pending: {effects:?}"
        );
    }

    /// Bug reale (2026-07-28, scoperto dal vivo usando Library "Condividi"): a
    /// differenza del gate 1 (self come candidato, testato sopra), il gate 2
    /// (self come SERVER che deve votare su un altro) non veniva MAI ri-mostrato
    /// se la finestra chat del server era chiusa quando la richiesta di
    /// ammissione arrivava — gap esplicitamente documentato nel commento di
    /// `SetServerTx` ("da valutare in una slice successiva") e mai chiuso.
    /// Conseguenza dal vivo: il server non vedeva alcuna richiesta di ingresso,
    /// l'ammissione si risolveva solo al timeout di 60s (silenzio = sì), non per
    /// un voto consapevole.
    #[test]
    fn set_server_tx_replays_pending_admission_vote_on_ui_reopen() {
        let mut s = AiChatService::new_for_test(info(10, "m10"));
        s.handle_event(ServiceEvent::Discovered(info(20, "m20"), None));
        s.mark_connected_for_test(PeerId(Ipv4Addr::new(192, 168, 1, 20)));

        // La richiesta di ammissione arriva mentre la UI del server non si è mai
        // connessa (in produzione: server_tx ancora None, il ToUi generato qui
        // viene scartato in silenzio da `perform`).
        let from = PeerId(Ipv4Addr::new(192, 168, 1, 20));
        s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::RequestAdmission { label: "m20-human".into() },
        });

        // La UI del server si connette: SetServerTx DEVE ri-emettere il gate 2
        // per il voto ancora aperto.
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<ServerMsg>();
        let effects = s.handle_event(ServiceEvent::SetServerTx(tx));
        let candidate = effects.iter().find_map(|e| match e {
            Effect::ToUi(ServerMsg::AiChatAdmissionRequest { candidate }) => Some(candidate.clone()),
            _ => None,
        });
        assert_eq!(candidate, Some("m20-human".to_string()),
            "SetServerTx deve ri-emettere AiChatAdmissionRequest per il voto ancora aperto: {effects:?}");
    }

    /// Se self (server) ha GIÀ votato ma il turno resta aperto (un altro
    /// presente non ha ancora votato), il gate 2 non va ri-mostrato — self ha
    /// già risposto, ri-proporre la stessa domanda sarebbe confuso/ridondante.
    #[test]
    fn set_server_tx_does_not_replay_admission_vote_already_cast_by_self() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let p = PeerId(Ipv4Addr::new(192, 168, 1, 20));
        let c = PeerId(Ipv4Addr::new(192, 168, 1, 30));
        s.mark_connected_for_test(p);
        s.mark_admitted_for_test(p);

        s.handle_event(ServiceEvent::PeerMsg {
            from: c,
            msg: ChatMsg::RequestAdmission { label: "cand-human".into() },
        });
        // Il server (self) vota subito — p (presente) non ha ancora votato,
        // quindi il turno resta aperto (non c'è ancora unanimità).
        s.handle_event(ServiceEvent::AdmissionVoteUi {
            candidate_label: "cand-human".into(),
            accept: true,
        });
        assert!(s.pending_votes_len_for_test() > 0, "il voto di p manca ancora: il turno deve restare aperto");

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<ServerMsg>();
        let effects = s.handle_event(ServiceEvent::SetServerTx(tx));
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::ToUi(ServerMsg::AiChatAdmissionRequest { .. }))),
            "self ha già votato: non deve ri-mostrare il gate 2 per lo stesso candidato: {effects:?}"
        );
    }

    /// Riapertura finestra con un'offerta ancora in attesa di decisione: `SetServerTx`
    /// deve ri-mostrare `ShareRequest` — stesso principio del replay di roster/storico/
    /// gate di ammissione. Regressione diretta del gap trovato nel self-review del
    /// brainstorm (spec §9.1): senza questo, un'offerta arrivata a finestra chiusa non
    /// verrebbe MAI mostrata, nemmeno riaprendo la finestra più tardi.
    #[test]
    fn set_server_tx_replays_share_request_for_pending_offer() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let from = PeerId(Ipv4Addr::new(192, 168, 1, 26));
        s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::ShareOffer {
                share_id: "192.168.1.26:0".into(),
                from_label: "rumpleteazer".into(),
                doc_name: "notes.md".into(),
                size_bytes: 1234,
            },
        });

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<ServerMsg>();
        let effects = s.handle_event(ServiceEvent::SetServerTx(tx));

        assert!(
            effects.iter().any(|e| matches!(e, Effect::ToUi(ServerMsg::ShareRequest { share_id, .. })
                if share_id == "192.168.1.26:0")),
            "SetServerTx deve ri-emettere ShareRequest per un'offerta ancora pendente: {effects:?}"
        );
    }

    /// Negativo: `self_admission == NotJoining` (caso comune: siamo il server, o
    /// nessun peer ancora scoperto) non deve produrre nessuno dei due replay sopra
    /// — comportamento invariato rispetto a prima di questo fix.
    #[test]
    fn set_server_tx_no_admission_gate_replay_when_not_joining() {
        let mut s = AiChatService::new_for_test(info(10, "m10"));
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<ServerMsg>();
        let effects = s.handle_event(ServiceEvent::SetServerTx(tx));
        assert!(
            !effects.iter().any(|e| matches!(
                e,
                Effect::ToUi(ServerMsg::AiChatJoinPrompt { .. }) | Effect::ToUi(ServerMsg::AiChatPending { .. })
            )),
            "NotJoining non deve ri-emettere alcun gate di ammissione: {effects:?}"
        );
    }

    // -------------------------------------------------------------------------
    // Task 8: la sezione "dump storico al Join (server→nuovo peer)" che stava qui
    // (`server_join_sends_history_dump_only_to_new_peer`) testava l'arm SERVER di
    // `ChatMsg::Join`, ora dead code (rimosso — vedi l'arm `PeerMsg` in
    // `handle_event`). La stessa invariante (dump SOLO al peer appena ammesso, non
    // ri-broadcastato agli altri presenti) è ora coperta da
    // `resolve_admit_sends_history_dump_to_candidate` (Task 8, sopra), che verifica
    // esplicitamente entrambe le metà: il candidato riceve `History`, un presente
    // già ammesso no.
    // -------------------------------------------------------------------------

    /// Il CLIENT, ricevendo `ChatMsg::History` dal server, sostituisce il proprio storico
    /// locale (non lo accoda) e — se ha una UI connessa — la aggiorna subito con `AiChatHistory`.
    ///
    /// Parte con UN messaggio locale preesistente (più corto del dump, quindi il guard passa)
    /// per discriminare "sostituisci" da "accoda": se il codice facesse `.extend()` invece di
    /// `=`, il risultato avrebbe 3 elementi (1 preesistente + 2 del dump) invece di 2.
    #[test]
    fn client_receiving_history_replaces_local_and_pushes_to_ui() {
        let mut s = AiChatService::new_for_test(info(30, "m30"));
        s.handle_event(ServiceEvent::Discovered(info(10, "m10"), None));
        s.mark_connected_for_test(PeerId(Ipv4Addr::new(192, 168, 1, 10)));

        // Storico locale preesistente (stale), più corto del dump in arrivo.
        s.handle_event(ServiceEvent::HumanSay("messaggio vecchio".into()));

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<ServerMsg>();
        s.handle_event(ServiceEvent::SetServerTx(tx));

        let from = PeerId(Ipv4Addr::new(192, 168, 1, 10));
        let dump = vec![
            ChatLine { from_label: "m10-human".into(), text: "ciao".into(), display_name: None, is_ai: false },
            ChatLine { from_label: "m30-human".into(), text: "ehi".into(), display_name: None, is_ai: false },
        ];
        let effects = s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::History { entries: dump.clone() },
        });

        assert_eq!(
            s.history_for_test(),
            dump.as_slice(),
            "lo storico locale deve essere SOSTITUITO dal dump, non accodato"
        );

        let pushed = effects.iter().find_map(|e| match e {
            Effect::ToUi(ServerMsg::AiChatHistory { entries }) => Some(entries.clone()),
            _ => None,
        }).expect("deve pushare AiChatHistory alla UI");
        assert_eq!(pushed.len(), 2);
        assert_eq!(pushed[0].text, "ciao");
        assert_eq!(pushed[1].text, "ehi");
    }

    /// `Ping` è un battito cardiaco puro: la sola ricezione ha già resettato il timeout di
    /// silenzio nel reader (`spawn_peer_tasks`, Task 8) — questo match non deve produrre
    /// NESSUN effetto applicativo (niente da mandare alla UI, niente da rilanciare ai peer).
    #[test]
    fn peer_msg_ping_is_a_no_op() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let from = PeerId(Ipv4Addr::new(192, 168, 1, 20));
        let effects = s.handle_event(ServiceEvent::PeerMsg { from, msg: ChatMsg::Ping {} });
        assert_eq!(effects, Vec::new(), "Ping non deve produrre nessun effetto applicativo");
    }

    /// Il CLIENT, ricevendo `ChatMsg::PeerLost` dal proprio server (un altro client è sparito),
    /// deve solo inoltrare la notifica alla propria UI locale (`AiChatPeerLost`) — nessun
    /// ulteriore relay: il client non ha altri link verso cui rilanciare.
    #[test]
    fn peer_msg_peer_lost_forwards_to_local_ui() {
        let mut s = AiChatService::new_for_test(info(30, "m30"));
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<ServerMsg>();
        s.handle_event(ServiceEvent::SetServerTx(tx));

        let from = PeerId(Ipv4Addr::new(192, 168, 1, 10)); // il proprio server
        let effects = s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::PeerLost { label: "quaxo-human".into() },
        });

        assert_eq!(
            effects,
            vec![Effect::ToUi(ServerMsg::AiChatPeerLost { label: "quaxo-human".into() })]
        );
    }

    /// Se il dump ricevuto è PIÙ CORTO dello storico locale, NON deve sovrascriverlo — il
    /// nostro storico locale potrebbe contenere un messaggio nostro non ancora relayato dal
    /// vecchio server prima che sparisse (lo scenario esatto che questa feature deve
    /// sopravvivere). Regression test per il finding Important della review finale.
    #[test]
    fn client_keeps_longer_local_history_when_dump_is_shorter() {
        let mut s = AiChatService::new_for_test(info(30, "m30"));
        s.handle_event(ServiceEvent::Discovered(info(10, "m10"), None));
        s.mark_connected_for_test(PeerId(Ipv4Addr::new(192, 168, 1, 10)));

        // Due messaggi locali, incluso uno "nostro" non ancora relayato dal vecchio server.
        s.handle_event(ServiceEvent::HumanSay("primo".into()));
        s.handle_event(ServiceEvent::HumanSay("secondo non ancora relayato".into()));
        let local_before = s.history_for_test().to_vec();
        assert_eq!(local_before.len(), 2);

        // Il nuovo server manda un dump più corto (non ha visto il secondo messaggio).
        let from = PeerId(Ipv4Addr::new(192, 168, 1, 10));
        let shorter_dump = vec![ChatLine { from_label: "m10-human".into(), text: "ciao".into(), display_name: None, is_ai: false }];
        let effects = s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::History { entries: shorter_dump },
        });

        assert_eq!(
            s.history_for_test(),
            local_before.as_slice(),
            "un dump più corto non deve sovrascrivere lo storico locale più lungo"
        );
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::ToUi(ServerMsg::AiChatHistory { .. }))),
            "nessun aggiornamento UI se il dump viene scartato: {effects:?}"
        );
    }

    /// Un SERVER non dovrebbe mai ricevere `ChatMsg::History` (solo lui lo manda), ma se
    /// capitasse (anomalia), deve ignorarlo — non sovrascrivere il proprio storico
    /// autorevole con quello di un client.
    ///
    /// Il dump in arrivo ha la STESSA lunghezza dello storico locale (1) ma contenuto
    /// diverso: così il guard sulla lunghezza (`entries.len() >= self.history.len()`) da
    /// solo passerebbe, e a rigettare è SOLO la clausola di ruolo — isola la clausola
    /// invece di farla coincidere con un dump più corto (che la nasconderebbe).
    #[test]
    fn server_ignores_history_message_from_a_client() {
        let mut s = server_with_two_clients(); // me=.10 server, .20/.30 connessi
        s.handle_event(ServiceEvent::HumanSay("autorevole".into()));
        let local_before = s.history_for_test().to_vec();
        assert_eq!(local_before.len(), 1);

        let from = PeerId(Ipv4Addr::new(192, 168, 1, 20));
        let same_len_but_different = vec![ChatLine { from_label: "m20-human".into(), text: "diverso".into(), display_name: None, is_ai: false }];
        let effects = s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::History { entries: same_len_but_different },
        });

        assert_eq!(s.history_for_test(), local_before.as_slice(), "il server ignora History in arrivo");
        assert!(effects.is_empty(), "nessun effetto per un History anomalo lato server: {effects:?}");
    }

    // -------------------------------------------------------------------------
    // Scenario di Test: log elezione/cambio server
    // -------------------------------------------------------------------------

    /// Prima elezione (nessun leader precedente): `last_logged_leader` passa da `None` a
    /// `Some(leader)`. me=.10 è l'IP più basso → leader = se stesso.
    #[test]
    fn first_election_sets_last_logged_leader() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        assert_eq!(s.last_logged_leader_for_test(), None);
        s.handle_event(ServiceEvent::Discovered(info(20, "macavity"), None));
        assert_eq!(
            s.last_logged_leader_for_test(),
            Some(PeerId(Ipv4Addr::new(192, 168, 1, 10)))
        );
    }

    /// Sparizione del leader corrente (`PeerGone`): `last_logged_leader` deve aggiornarsi al
    /// nuovo leader eletto. me=.20, server iniziale .10 (IP più basso); .10 sparisce → il
    /// nuovo (unico) leader è me stesso.
    #[test]
    fn leader_change_on_peer_gone_updates_last_logged_leader() {
        let mut s = AiChatService::new_for_test(info(20, "m20"));
        s.handle_event(ServiceEvent::Discovered(info(10, "m10"), None));
        let server10 = PeerId(Ipv4Addr::new(192, 168, 1, 10));
        assert_eq!(s.last_logged_leader_for_test(), Some(server10));

        // Debito #4: questo scenario non passa da `mark_connected_for_test` (il peer è
        // solo scoperto, mai "connesso" nel senso di `self.connected`), quindi
        // registriamo esplicitamente una generazione da far combaciare col PeerGone sotto.
        s.set_link_gen_for_test(server10, 1);
        s.handle_event(ServiceEvent::PeerGone(server10, 1));
        assert_eq!(
            s.last_logged_leader_for_test(),
            Some(PeerId(Ipv4Addr::new(192, 168, 1, 20))),
            "dopo la sparizione del leader, deve essere rieletto me stesso"
        );
    }

    /// Un ri-annuncio dello stesso peer (come i cicli UDP periodici ogni ~7s) non deve
    /// cambiare `last_logged_leader` — il leader è lo stesso di prima.
    #[test]
    fn repeated_discovered_same_leader_does_not_change_last_logged_leader() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        s.handle_event(ServiceEvent::Discovered(info(20, "macavity"), None));
        let first = s.last_logged_leader_for_test();
        s.handle_event(ServiceEvent::Discovered(info(20, "macavity"), None));
        assert_eq!(s.last_logged_leader_for_test(), first, "il leader non cambia, resta lo stesso");
    }

    // -------------------------------------------------------------------------
    // Bug 2 (late-joiner) §3.4: `PeerGone` deve azzerare `reported_leader` quando il
    // peer sparito era proprio il leader riportato — altrimenti la rielezione tra i
    // superstiti resta bloccata `Undecided` invece di ricadere su `members.lowest()`.
    // Punto più delicato dello spec: regressione esplicita.
    // -------------------------------------------------------------------------

    #[test]
    fn peer_gone_clears_reported_leader_and_reelects_lowest_among_survivors() {
        // me = quaxo (.24). rumpleteazer (.26) è il leader riportato (e consentito):
        // quaxo diventa Client(rumpleteazer). skimble (.35) si unisce dopo, riportando
        // anch'esso rumpleteazer come leader — nessun cambio (sticky).
        let mut s = AiChatService::new_for_test(info(24, "quaxo"));
        let rumpleteazer = info(26, "rumpleteazer");
        let skimble = info(35, "skimble");

        s.handle_event(ServiceEvent::Discovered(rumpleteazer.clone(), Some(rumpleteazer.id)));
        assert_eq!(s_role(&s), Role::Client(rumpleteazer.id));
        assert_eq!(s.reported_leader_for_test(), Some(rumpleteazer.id));

        s.handle_event(ServiceEvent::Discovered(skimble.clone(), Some(rumpleteazer.id)));
        assert_eq!(s_role(&s), Role::Client(rumpleteazer.id), "sticky: resta Client di rumpleteazer");

        // rumpleteazer sparisce (PeerGone): `reported_leader` deve azzerarsi, e la
        // rielezione tra i superstiti {quaxo=.24, skimble=.35} deve ricadere su
        // `members.lowest()` → quaxo (IP più basso) diventa Server. SENZA il fix,
        // `reported_leader` resterebbe "leftover" su rumpleteazer (ormai fuori
        // roster) e `elect()` produrrebbe `Undecided` invece di rieleggere.
        // Debito #4: nessun `mark_connected_for_test` in questo scenario (solo
        // Discovered) → registriamo una generazione esplicita da far combaciare.
        s.set_link_gen_for_test(rumpleteazer.id, 1);
        s.handle_event(ServiceEvent::PeerGone(rumpleteazer.id, 1));
        assert_eq!(
            s.reported_leader_for_test(), None,
            "reported_leader deve azzerarsi quando il peer sparito coincide con esso"
        );
        assert_eq!(
            s_role(&s), Role::Server,
            "i superstiti devono rieleggere il più basso (quaxo), non restare Undecided: ruolo={:?}",
            s_role(&s)
        );
    }

    // -------------------------------------------------------------------------
    // Keepalive §5: `PeerGone` annuncia la sparizione + cablaggio di `Room::leave()`.
    // -------------------------------------------------------------------------

    #[test]
    fn peer_gone_from_server_broadcasts_roster_and_peer_lost_to_remaining_clients() {
        // me=10 (server). Due client REALMENTE ammessi (Room popolato via il voto di
        // ammissione — Task 8: `RequestAdmission` + `VoteTimeout`, non più il vecchio
        // `ChatMsg::Join` diretto) così on_server_leave ha qualcosa da togliere.
        //
        // NOTA su `mark_connected_for_test`: sovrascrive `self.peers[id]` con una
        // `PeerInfo` SINTETICA (label `"peer-<ip>"`, backdoor di test per simulare un link
        // connesso senza socket reali — vedi il suo doc-comment). Il capture dell'etichetta
        // in `PeerGone` legge PROPRIO `self.peers[id].label_base` — senza ri-annunciare
        // (`Discovered` di nuovo) DOPO `mark_connected_for_test`, l'etichetta catturata
        // sarebbe quella sintetica, non "m20"/"m30", e `on_server_leave` cercherebbe nel
        // Room un'etichetta che non vi si trova mai (il voto sotto ammette "m20-human"/
        // "m30-human") — `Room::leave` diventerebbe un no-op silenzioso. Il secondo
        // `Discovered` ripristina la `PeerInfo` reale (`Discovered` la sovrascrive
        // incondizionatamente, in cima al suo arm, prima di qualunque branching).
        let mut s = AiChatService::new_for_test(info(10, "m10"));
        let peer20 = PeerId(Ipv4Addr::new(192, 168, 1, 20));
        let peer30 = PeerId(Ipv4Addr::new(192, 168, 1, 30));

        s.handle_event(ServiceEvent::Discovered(info(20, "m20"), None));
        s.mark_connected_for_test(peer20);
        s.handle_event(ServiceEvent::Discovered(info(20, "m20"), None)); // ripristina label reale
        s.handle_event(ServiceEvent::PeerMsg {
            from: peer20,
            msg: ChatMsg::RequestAdmission { label: "m20-human".into() },
        });
        let g20 = s.pending_vote_generation_for_test(peer20).expect("voto in corso per peer20");
        s.handle_event(ServiceEvent::VoteTimeout { candidate: peer20, generation: g20 }); // silenzio = sì → admit

        s.handle_event(ServiceEvent::Discovered(info(30, "m30"), None));
        s.mark_connected_for_test(peer30);
        s.handle_event(ServiceEvent::Discovered(info(30, "m30"), None)); // ripristina label reale
        s.handle_event(ServiceEvent::PeerMsg {
            from: peer30,
            msg: ChatMsg::RequestAdmission { label: "m30-human".into() },
        });
        let g30 = s.pending_vote_generation_for_test(peer30).expect("voto in corso per peer30");
        s.handle_event(ServiceEvent::VoteTimeout { candidate: peer30, generation: g30 }); // silenzio = sì → admit

        assert_eq!(s_role(&s), Role::Server);

        // Debito #4: recupera la generazione assegnata da `mark_connected_for_test(peer20)`
        // sopra — il successivo `Discovered` di ripristino label NON tocca `link_gen`.
        let gen20 = s.link_gen_for_test(peer20).expect("mark_connected_for_test deve assegnare una generazione");
        let effects = s.handle_event(ServiceEvent::PeerGone(peer20, gen20));

        // NOTA (deviazione dal brief, verificata): il roster atteso include ANCHE
        // "m10-human" (la propria etichetta). `decide_and_connect` (Role::Server) chiama
        // `on_server_join(self)` a OGNI Discovered mentre il ruolo resta Server
        // (idempotente, comportamento preesistente e già testato altrove — vedi
        // l'assert su "m10-human" a riga ~1628 di questo stesso file). Il letterale del
        // brief (`vec!["m30-human"]`) non teneva conto di questo; escludere "m20-human"
        // resta l'unica cosa che conta qui.
        assert!(
            effects.iter().any(|e| matches!(e,
                Effect::SendToPeer(p, ChatMsg::Roster { participants })
                    if *p == peer30 && participants == &vec!["m10-human".to_string(), "m30-human".to_string()])),
            "il Roster broadcastato al superstite deve escludere m20-human: {:?}", effects
        );
        assert!(
            effects.iter().any(|e| matches!(e,
                Effect::SendToPeer(p, ChatMsg::PeerLost { label })
                    if *p == peer30 && label == "m20-human")),
            "PeerLost deve arrivare al client rimasto: {:?}", effects
        );
        assert!(
            effects.contains(&Effect::ToUi(ServerMsg::AiChatPeerLost { label: "m20-human".into() })),
            "la propria UI deve ricevere l'annuncio: {:?}", effects
        );
    }

    // Review finale whole-slice (Opus), Important: quando un client sparisce e il nodo
    // locale è il server, `on_server_leave` ritorna il roster aggiornato ma il ramo
    // `PeerGone`/`era_server` scartava `_participants` — `self.last_known_roster` non
    // veniva toccato e nessun `Effect::ToUi(AiChatRoster)` veniva emesso per la UI
    // locale del server. Conseguenza osservabile: il "presenti:" nella finestra-chat
    // del server continuava a elencare il peer sparito; se il server chiudeva/riapriva
    // la finestra, `SetServerTx` ri-emetteva il `last_known_roster` STANTIO (ancora col
    // fantasma), perché non era mai stato aggiornato. Confronta col ramo `resolve_admit`
    // (Task 8, ex `ChatMsg::Join`), che AGGIORNA `last_known_roster` e emette
    // `ToUi(AiChatRoster)` — questo test verifica che il ramo `leave` faccia la stessa cosa.
    #[test]
    fn peer_gone_from_server_refreshes_own_roster_for_reopened_window() {
        // Stesso setup del test precedente: me=10 (server), due client REALMENTE
        // ammessi (m20, m30, via il voto di ammissione — Task 8), cosicché
        // on_server_leave abbia un roster vero da cui togliere il peer sparito.
        let mut s = AiChatService::new_for_test(info(10, "m10"));
        let peer20 = PeerId(Ipv4Addr::new(192, 168, 1, 20));
        let peer30 = PeerId(Ipv4Addr::new(192, 168, 1, 30));

        s.handle_event(ServiceEvent::Discovered(info(20, "m20"), None));
        s.mark_connected_for_test(peer20);
        s.handle_event(ServiceEvent::Discovered(info(20, "m20"), None)); // ripristina label reale
        s.handle_event(ServiceEvent::PeerMsg {
            from: peer20,
            msg: ChatMsg::RequestAdmission { label: "m20-human".into() },
        });
        let g20 = s.pending_vote_generation_for_test(peer20).expect("voto in corso per peer20");
        s.handle_event(ServiceEvent::VoteTimeout { candidate: peer20, generation: g20 }); // silenzio = sì → admit

        s.handle_event(ServiceEvent::Discovered(info(30, "m30"), None));
        s.mark_connected_for_test(peer30);
        s.handle_event(ServiceEvent::Discovered(info(30, "m30"), None)); // ripristina label reale
        s.handle_event(ServiceEvent::PeerMsg {
            from: peer30,
            msg: ChatMsg::RequestAdmission { label: "m30-human".into() },
        });
        let g30 = s.pending_vote_generation_for_test(peer30).expect("voto in corso per peer30");
        s.handle_event(ServiceEvent::VoteTimeout { candidate: peer30, generation: g30 }); // silenzio = sì → admit

        assert_eq!(s_role(&s), Role::Server);

        // Debito #4: generazione assegnata da `mark_connected_for_test(peer20)` sopra.
        let gen20 = s.link_gen_for_test(peer20).expect("mark_connected_for_test deve assegnare una generazione");
        let effects = s.handle_event(ServiceEvent::PeerGone(peer20, gen20));

        // La UI LOCALE del server deve ricevere il roster aggiornato (senza m20-human),
        // esattamente come i client superstiti lo ricevono via SendToPeer/Roster.
        assert!(
            effects.contains(&Effect::ToUi(ServerMsg::AiChatRoster {
                participants: vec!["m10-human".to_string(), "m30-human".to_string()],
            })),
            "la UI locale del server deve ricevere il roster aggiornato dopo un leave: {:?}", effects
        );
    }

    #[test]
    fn peer_gone_from_client_when_server_lost_emits_local_notice_only() {
        // me=30 (client di 10). Nessun altro link → nessun SendToPeer, solo ToUi locale.
        let mut s = AiChatService::new_for_test(info(30, "m30"));
        let server_id = PeerId(Ipv4Addr::new(192, 168, 1, 10));
        s.handle_event(ServiceEvent::Discovered(info(10, "server"), None));
        s.mark_connected_for_test(server_id);
        // Ripristina la label reale sovrascritta da `mark_connected_for_test` (vedi nota
        // dettagliata nel test precedente) — necessario perché PeerGone la legge da
        // `self.peers[id].label_base` per costruire l'annuncio.
        s.handle_event(ServiceEvent::Discovered(info(10, "server"), None));
        assert_eq!(s_role(&s), Role::Client(server_id));

        // Debito #4: generazione assegnata da `mark_connected_for_test(server_id)` sopra.
        let gen = s.link_gen_for_test(server_id).expect("mark_connected_for_test deve assegnare una generazione");
        let effects = s.handle_event(ServiceEvent::PeerGone(server_id, gen));

        assert!(
            !effects.iter().any(|e| matches!(e, Effect::SendToPeer(..))),
            "un client non ha altri link a cui inoltrare: {:?}", effects
        );
        assert!(
            effects.contains(&Effect::ToUi(ServerMsg::AiChatPeerLost { label: "server-human".into() })),
            "notifica locale attesa: {:?}", effects
        );
    }

    #[test]
    fn peer_gone_combines_reported_leader_cleanup_with_peer_lost_notice() {
        // Riusa lo scenario del fix Bug 2 (quaxo Client di rumpleteazer): la sparizione
        // del server deve SIA azzerare reported_leader (0.25.7, invariato) SIA produrre
        // l'annuncio locale (nuovo qui) — le due cose convivono nello stesso arm.
        let mut s = AiChatService::new_for_test(info(24, "quaxo"));
        let rumpleteazer = info(26, "rumpleteazer");
        s.handle_event(ServiceEvent::Discovered(rumpleteazer.clone(), Some(rumpleteazer.id)));
        assert_eq!(s.reported_leader_for_test(), Some(rumpleteazer.id));

        // Debito #4: nessun `mark_connected_for_test` in questo scenario — generazione
        // esplicita da far combaciare col PeerGone sotto.
        s.set_link_gen_for_test(rumpleteazer.id, 1);
        let effects = s.handle_event(ServiceEvent::PeerGone(rumpleteazer.id, 1));

        assert_eq!(s.reported_leader_for_test(), None, "invariato dal fix Bug 2");
        assert!(
            effects.contains(&Effect::ToUi(ServerMsg::AiChatPeerLost { label: "rumpleteazer-human".into() })),
            "nuovo annuncio atteso accanto alla pulizia di reported_leader: {:?}", effects
        );
    }

    // -------------------------------------------------------------------------
    // Task 8b: robustezza dell'ammissione — `PeerGone` deve ripulire `admitted`
    // (RED prima del GREEN, vedi `Docs/superpowers/plans/2026-07-03-aichat-admission.md`
    // e il `TODO(admission-8b)` lasciato dal Task 8).
    //
    // Problema: prima di questo fix, l'arm `PeerGone` ripuliva `connected`/`peers`/
    // `links`/`link_gen` ma NON `self.admitted` — un peer ammesso e poi disconnesso
    // restava un "votante fantasma": continuava a contare come "presente" in un
    // futuro `start_admission_vote` (che legge `self.admitted`), pur non avendo più
    // alcun link TCP da cui ricevere un voto reale.
    // -------------------------------------------------------------------------

    /// Un peer AMMESSO che sparisce (`PeerGone`) deve uscire da `self.admitted` —
    /// altrimenti un voto di ammissione futuro lo considererebbe ancora un
    /// "presente" che deve votare, bloccando il turno fino al timeout invece che
    /// per una decisione reale.
    #[test]
    fn peer_gone_removes_from_admitted() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let p = PeerId(Ipv4Addr::new(192, 168, 1, 20));
        s.mark_connected_for_test(p);
        s.mark_admitted_for_test(p);
        assert!(s.admitted_contains_for_test(p), "precondizione: p deve partire ammesso");

        let gen = s.link_gen_for_test(p).expect("mark_connected_for_test deve assegnare una generazione");
        s.handle_event(ServiceEvent::PeerGone(p, gen));

        assert!(
            !s.admitted_contains_for_test(p),
            "un peer sparito non deve restare un votante fantasma in `admitted`"
        );
    }

    /// Scenario: un voto di ammissione è in corso per il candidato `C`, coi
    /// presenti congelati `{P, me}`. `me` (l'umano-server) ha già votato sì — resta
    /// solo P a dover votare. Se P sparisce (`PeerGone`) PRIMA di votare, non deve
    /// restare un votante richiesto per sempre: il turno, con `me` come UNICO
    /// presente rimasto (che ha già detto sì), deve risolversi SUBITO in
    /// ammissione — non restare bloccato fino al `VoteTimeout`.
    #[test]
    fn peer_gone_during_vote_removes_required_voter_and_may_resolve() {
        let mut s = AiChatService::new_for_test(info(10, "skimble")); // me=.10, server
        let p = PeerId(Ipv4Addr::new(192, 168, 1, 20));
        let c = PeerId(Ipv4Addr::new(192, 168, 1, 30));

        s.mark_connected_for_test(p);
        s.mark_admitted_for_test(p); // P è un presente AMMESSO — deve votare

        // Il candidato C manda RequestAdmission: parte il voto, presenti = {P, me}.
        s.handle_event(ServiceEvent::PeerMsg {
            from: c,
            msg: ChatMsg::RequestAdmission { label: "cand-human".into() },
        });
        assert_eq!(s.pending_votes_len_for_test(), 1, "precondizione: voto avviato");

        // "me" vota sì: manca ancora P, il turno resta aperto.
        let effects_after_me = s.handle_event(ServiceEvent::AdmissionVoteUi {
            candidate_label: "cand-human".into(),
            accept: true,
        });
        assert!(
            effects_after_me.is_empty(),
            "un solo sì su due presenti non deve chiudere il turno: {effects_after_me:?}"
        );
        assert!(!s.admitted_contains_for_test(c), "precondizione: C non ancora ammesso");

        // P sparisce PRIMA di votare — usa la generazione assegnata da
        // `mark_connected_for_test` per non innescare il guard "PeerGone stale".
        let gen_p = s.link_gen_for_test(p).expect("mark_connected_for_test deve assegnare una generazione");
        let effects = s.handle_event(ServiceEvent::PeerGone(p, gen_p));

        // P non deve restare un votante richiesto: il voto per C deve essersi
        // risolto (rimosso da `pending_votes`) subito dopo la sua rimozione da
        // `present`, dato che l'unico presente rimasto (me) aveva già votato sì.
        assert_eq!(
            s.pending_vote_present_for_test(c),
            None,
            "il voto per C deve risolversi (non restare aperto) non appena P — l'unico mancante — sparisce"
        );
        assert!(
            s.admitted_contains_for_test(c),
            "C deve risultare ammesso: l'unico presente rimasto (me) aveva già votato sì"
        );
        assert!(
            effects.iter().any(|e| matches!(e,
                Effect::SendToPeer(cand, ChatMsg::Admitted { label })
                    if *cand == c && label == "cand-human")),
            "gli effetti del PeerGone devono includere la risoluzione del voto (Admitted a C): {effects:?}"
        );
    }

    /// FIX #3 (review 2026-07-03): se il peer che sparisce è ESSO STESSO il
    /// candidato di un voto in corso, il voto non ha più oggetto e va rimosso.
    /// Prima del fix, `PeerGone` ripuliva `id` dai `present`/`votes` degli ALTRI
    /// voti e da `admitted`, ma non rimuoveva `pending_votes[id]` (il voto di CUI
    /// `id` è candidato): poiché un candidato non è mai tra i propri `present`,
    /// `ready_after_peer_gone` vedeva "tutti i presenti hanno votato sì" e
    /// ammetteva un peer ORMAI SPARITO — un membro fantasma del roster.
    /// Scenario: me=.10 server, P=.20 ammesso, C=.30 candidato; P vota sì (manca
    /// solo il voto del server-umano), poi C sparisce: il voto deve sparire con
    /// lui, senza admit fantasma.
    #[test]
    fn peer_gone_of_candidate_drops_its_vote_without_phantom_admit() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let p = PeerId(Ipv4Addr::new(192, 168, 1, 20));
        let c = PeerId(Ipv4Addr::new(192, 168, 1, 30));

        s.mark_connected_for_test(p);
        s.mark_admitted_for_test(p);
        // C si connette e chiede di entrare: parte il voto, present={P,me}.
        s.mark_connected_for_test(c);
        s.handle_event(ServiceEvent::PeerMsg {
            from: c,
            msg: ChatMsg::RequestAdmission { label: "cand-human".into() },
        });
        // P vota sì: manca solo il voto del server-umano → turno ancora aperto.
        s.handle_event(ServiceEvent::PeerMsg {
            from: p,
            msg: ChatMsg::AdmissionVote { candidate: "cand-human".into(), accept: true },
        });
        assert_eq!(s.pending_votes_len_for_test(), 1, "precondizione: voto ancora aperto");

        // C sparisce PRIMA che il turno si chiuda (generazione reale del link).
        let gen = s.link_gen_for_test(c).expect("mark_connected_for_test deve assegnare una generazione");
        let effects = s.handle_event(ServiceEvent::PeerGone(c, gen));

        assert_eq!(
            s.pending_votes_len_for_test(),
            0,
            "il voto del candidato sparito deve essere rimosso"
        );
        // FIX #7: i presenti del turno (present = {P, me}) devono sapere che il gate 2
        // è chiuso, così tolgono il banner "ammetti cand-human?" per un candidato che
        // non c'è più (chiude il caso "candidato uscito" della stale-banner di F).
        assert!(
            effects.contains(&Effect::ToUi(ServerMsg::AiChatAdmissionResolved {
                candidate: "cand-human".into()
            })),
            "l'umano-server deve vedere chiudersi il gate 2 del candidato sparito: {effects:?}"
        );
        assert!(
            effects.contains(&Effect::SendToPeer(
                p,
                ChatMsg::AdmissionResolved { candidate: "cand-human".into() }
            )),
            "il presente remoto P deve ricevere la chiusura del voto del candidato sparito: {effects:?}"
        );
        assert!(
            !s.admitted_contains_for_test(c),
            "un candidato sparito non deve essere ammesso come membro fantasma"
        );
        assert!(
            !s.last_known_roster_for_test().contains(&"cand-human".to_string()),
            "il roster non deve contenere il candidato sparito: {:?}",
            s.last_known_roster_for_test()
        );
    }

    // -------------------------------------------------------------------------
    // Debito #4 (Slice C): generation token — race di riconnessione.
    //
    // Un `PeerGone` "vecchio" (dal reader di un link già morto) processato DOPO una
    // riconnessione dello stesso IP non deve rimuovere il link FRESCO. Vedi il design
    // in `Docs/superpowers/specs/2026-07-02-aichat-hardening-debts-design.md` §"Slice C".
    // -------------------------------------------------------------------------

    /// Caso "normale" (nessuna race in corso): un `PeerGone` la cui generazione COMBACIA
    /// con quella registrata deve rimuovere il link da `connected`/`links`/`link_gen`,
    /// esattamente come faceva l'arm prima di questo debito.
    #[test]
    fn peer_gone_matching_generation_removes_link() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let peer_id = PeerId(Ipv4Addr::new(192, 168, 1, 20));
        s.mark_connected_for_test(peer_id);
        let gen = s.link_gen_for_test(peer_id)
            .expect("mark_connected_for_test deve assegnare una generazione");

        s.handle_event(ServiceEvent::PeerGone(peer_id, gen));

        assert!(!s.connected_contains_for_test(peer_id), "il peer deve uscire da connected");
        assert!(!s.links_contains_for_test(peer_id), "il link deve essere rimosso");
        assert_eq!(s.link_gen_for_test(peer_id), None, "link_gen deve essere ripulito");
    }

    /// Il cuore del fix: un `PeerGone` STALE (porta la generazione di un link già
    /// rimpiazzato da una riconnessione più recente) deve essere un no-op totale — il
    /// link FRESCO (e tutto lo stato ad esso associato) deve restare intatto.
    ///
    /// Simuliamo la riconnessione chiamando `mark_connected_for_test` DUE VOLTE per lo
    /// stesso `id`: la seconda chiamata preleva una nuova generazione dallo stesso
    /// contatore condiviso che userebbe un vero link TCP, sostituendo quella della prima
    /// — esattamente lo scenario che il fix deve gestire.
    #[test]
    fn stale_peer_gone_does_not_remove_fresh_link() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let peer_id = PeerId(Ipv4Addr::new(192, 168, 1, 20));

        // Prima "connessione": generazione N.
        s.mark_connected_for_test(peer_id);
        let stale_gen = s.link_gen_for_test(peer_id).expect("prima generazione assegnata");

        // "Riconnessione" dello stesso IP: generazione N+k, più recente, sostituisce la
        // prima nella mappa `link_gen` — come farebbe un vero link TCP nuovo.
        s.mark_connected_for_test(peer_id);
        let fresh_gen = s.link_gen_for_test(peer_id).expect("seconda generazione assegnata");
        assert_ne!(stale_gen, fresh_gen, "le due generazioni devono essere diverse");

        // Il PeerGone "vecchio" (reader del link ormai sostituito) arriva DOPO: la sua
        // generazione non combacia più con quella corrente → deve essere ignorato.
        let effects = s.handle_event(ServiceEvent::PeerGone(peer_id, stale_gen));

        assert_eq!(effects, Vec::new(), "un PeerGone stale non deve produrre alcun effetto");
        assert!(s.connected_contains_for_test(peer_id), "il link fresco deve restare connesso");
        assert!(s.links_contains_for_test(peer_id), "il link fresco non deve essere rimosso");
        assert_eq!(
            s.link_gen_for_test(peer_id),
            Some(fresh_gen),
            "link_gen deve continuare a puntare alla generazione fresca"
        );
    }

    // -------------------------------------------------------------------------
    // Debito #3 (Slice C): connect non bloccante (head-of-line blocking).
    //
    // `Effect::ConnectTo` non deve più fare `TcpStream::connect(..).await` INLINE dentro
    // `perform` (il task dell'attore): un peer irraggiungibile bloccherebbe l'intero
    // attore per il timeout OS (~21s su Windows). Il connect vero e proprio si sposta in
    // un task spawnato; `perform` fa SOLO la parte sincrona (guard + marcatura
    // `connecting` + spawn) e ritorna subito. Questi test verificano quella parte
    // sincrona — mai un vero socket — più la logica di registrazione dell'esito
    // (`register_connect_success`/`register_connect_failure`, Slice C2), estratta apposta
    // per essere testabile passando un `PeerId`/`u64` sintetici, senza aspettare un vero
    // connect né fabbricare un `TcpStream`.
    // -------------------------------------------------------------------------

    /// Due `Effect::ConnectTo` di fila per LO STESSO peer: il primo marca il peer come
    /// "in connessione" (`connecting`); il secondo, trovandolo già lì, deve essere un
    /// no-op — niente panico, nessuna voce duplicata. La guardia è SINCRONA (avviene
    /// prima di qualunque `.await` sul socket), quindi osservabile subito dopo
    /// `perform(...).await`, senza dipendere dall'esito reale di un connect.
    #[tokio::test]
    async fn connect_dispatch_marks_connecting_and_dedups() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        // TEST-NET-1 (192.0.2.0/24, RFC 5737): riservato alla documentazione, mai
        // instradato — il task di connect spawnato in background non completerà mai
        // durante questo test (e non ci interessa che lo faccia: la guardia che testiamo
        // è tutta PRIMA dello spawn).
        let peer = PeerInfo {
            id: PeerId(Ipv4Addr::new(192, 0, 2, 1)),
            label_base: "documentation-net".into(),
            chat_port: 40100,
        };

        let (inbox_tx, _inbox_rx) = tokio::sync::mpsc::unbounded_channel::<ServiceEvent>();
        let (new_link_tx, _new_link_rx) =
            tokio::sync::mpsc::unbounded_channel::<(PeerId, TcpStream)>();
        let (connect_res_tx, _connect_res_rx) =
            tokio::sync::mpsc::unbounded_channel::<ConnectOutcome>();
        let shutdown = tokio_util::sync::CancellationToken::new();

        assert!(!s.connecting_contains_for_test(peer.id), "connecting deve partire vuoto");

        s.perform(Effect::ConnectTo(peer.clone()), &inbox_tx, &new_link_tx, &connect_res_tx, &shutdown).await;
        assert!(
            s.connecting_contains_for_test(peer.id),
            "connecting deve contenere il peer dopo la prima ConnectTo"
        );

        // Seconda ConnectTo per LO STESSO peer, ancora "in volo": no-op per la guardia.
        s.perform(Effect::ConnectTo(peer.clone()), &inbox_tx, &new_link_tx, &connect_res_tx, &shutdown).await;
        assert_eq!(
            s.connecting_len_for_test(), 1,
            "un solo peer in connecting, la seconda ConnectTo non deve duplicare"
        );
    }

    // -------------------------------------------------------------------------
    // Slice C2: race residua "register-before-spawn" (chiude il debito lasciato da
    // Slice C/0.27.0). Prima di questo fix, sia il path CONNECT sia il path ACCEPT
    // chiamavano `spawn_peer_tasks` DENTRO il task spawnato, PRIMA che l'attore avesse
    // scritto `self.link_gen`/`self.connected` — il reader appena avviato poteva quindi
    // emettere un `PeerGone(id, gen)` che la guardia di generazione vedeva come STALE
    // (la mappa non aveva ancora quella voce), mentre la registrazione arrivava DOPO e
    // inseriva comunque un link il cui reader era già morto (un "link fantasma" che il
    // guard anti-duplicato di `ConnectTo` impedisce poi di far risanare).
    //
    // Il fix è strutturale: la registrazione di stato (`register_connect_success`/
    // `register_link`) ora avviene SEMPRE nell'attore (con `&mut self`), SEMPRE prima
    // di chiamare `spawn_peer_tasks` — mai il contrario. Questi test verificano quella
    // registrazione in isolamento (senza alcun `TcpStream`, perché non serve: la
    // registrazione è pura mutazione di stato) e l'invariante d'ordine che ne deriva.
    // -------------------------------------------------------------------------

    /// `register_connect_success` deve registrare `link_gen`/`connected` e liberare
    /// `connecting` — SENZA bisogno di un `TcpStream`/writer reali, perché a questo
    /// punto (prima dello spawn) non esistono ancora: è compito del CHIAMANTE (il 5°
    /// ramo di `select!` in `run`) chiamare `spawn_peer_tasks` SUBITO DOPO e solo allora
    /// registrare `links`/inviare il `Join`.
    #[test]
    fn connect_success_registers_state_before_any_spawn() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let peer = info(20, "macavity");
        s.mark_connecting_for_test(peer.id);

        s.register_connect_success(peer.id, 7);

        assert!(s.connected_contains_for_test(peer.id), "il peer deve risultare connesso");
        assert_eq!(s.link_gen_for_test(peer.id), Some(7), "la generazione va registrata tale e quale");
        assert!(!s.connecting_contains_for_test(peer.id), "connecting deve liberarsi a connessione riuscita");
    }

    /// Il cuore del fix: dimostra che la scrittura di `link_gen` avviene PRIMA che
    /// qualunque `PeerGone` per quella generazione possa arrivare — cioè che un
    /// `PeerGone(id, gen)` successivo alla registrazione combacia SEMPRE (non è mai
    /// scartato come stale). Prima del fix questo non era garantito: la scrittura
    /// avveniva DOPO lo spawn (dentro il task/accept loop), quindi esisteva una finestra
    /// in cui il reader poteva già essere partito e morto senza che `link_gen` esistesse.
    /// Qui non serve un vero socket: basta chiamare `register_connect_success` e POI
    /// simulare l'evento che il reader avrebbe emesso — la guardia di generazione in
    /// `handle_event(PeerGone)` lo tratta come genuino (rimuove lo stato), non come stale.
    #[test]
    fn register_connect_success_makes_subsequent_peer_gone_non_stale() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let peer_id = PeerId(Ipv4Addr::new(192, 168, 1, 20));
        s.mark_connecting_for_test(peer_id);

        s.register_connect_success(peer_id, 3);
        assert_eq!(s.link_gen_for_test(peer_id), Some(3), "invariante: registrato PRIMA di qualunque spawn");
        assert!(s.connected_contains_for_test(peer_id));

        // Simula il PeerGone che il reader (spawnato SUBITO DOPO da chi chiama questo
        // metodo in produzione) emetterebbe se il link morisse immediatamente. Se fosse
        // stale (bug pre-fix) `handle_event` lo scarterebbe come no-op e connected/
        // link_gen resterebbero intonsi — invece qui devono essere ripuliti, a riprova
        // che la generazione combacia.
        s.handle_event(ServiceEvent::PeerGone(peer_id, 3));

        assert!(!s.connected_contains_for_test(peer_id), "il PeerGone deve essere trattato come genuino");
        assert_eq!(s.link_gen_for_test(peer_id), None, "link_gen ripulito: non era stale");
    }

    /// `register_connect_failure` deve SOLO liberare `connecting` — senza toccare
    /// `connected`/`links`/`link_gen` — così un futuro `Discovered`/`decide_and_connect`
    /// può ritentare la connessione verso lo stesso peer.
    #[test]
    fn connect_failure_clears_connecting_for_retry() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let peer_id = PeerId(Ipv4Addr::new(192, 168, 1, 20));
        s.mark_connecting_for_test(peer_id);
        assert!(s.connecting_contains_for_test(peer_id));

        s.register_connect_failure(peer_id);

        assert!(
            !s.connecting_contains_for_test(peer_id),
            "connecting deve liberarsi dopo un fallimento, per permettere un retry"
        );
        assert!(!s.connected_contains_for_test(peer_id), "un fallimento non deve mai connettere il peer");
    }

    // -------------------------------------------------------------------------
    // Bug 2 (late-joiner) §5-§6: test end-to-end che riproduce lo scenario esatto
    // del log a tre macchine (rumpleteazer, skimble, quaxo) — nessun socket, tre
    // `AiChatService` con `handle_event` chiamato direttamente, simulando lo scambio
    // di annunci UDP passando esplicitamente il `leader` che ciascun nodo starebbe
    // annunciando nel mondo reale (calcolato dal proprio ruolo corrente).
    // -------------------------------------------------------------------------

    /// Riproduce ESATTAMENTE lo scenario del log (§0 dello spec): rumpleteazer e
    /// skimble si eleggono per primi (rumpleteazer, IP più basso tra i due, diventa
    /// server); quaxo arriva DOPO con l'IP più basso di TUTTI (.24) ma — grazie al
    /// leader riportato dagli annunci di rumpleteazer/skimble — deve risultare
    /// `Client(rumpleteazer)`, MAI `Server`.
    #[test]
    fn late_joiner_with_lowest_ip_adopts_existing_leader_instead_of_self_electing() {
        let mut rumpleteazer = AiChatService::new_for_test(info(26, "rumpleteazer"));
        let mut skimble = AiChatService::new_for_test(info(35, "skimble"));
        let mut quaxo = AiChatService::new_for_test(info(24, "quaxo"));

        // --- Fondazione: rumpleteazer + skimble si scoprono e si eleggono ---
        // rumpleteazer scopre skimble: skimble non ha ancora deciso nulla (appena
        // avviato) → il suo annuncio riporterebbe leader:None. Task 8: `Discovered`
        // da sola rielegge (niente più `Consent` a valle).
        rumpleteazer.handle_event(ServiceEvent::Discovered(info(35, "skimble"), None));
        assert_eq!(s_role(&rumpleteazer), Role::Server, "rumpleteazer (.26 < .35) eletto server");

        // skimble scopre rumpleteazer: rumpleteazer ora crede di essere Server, quindi
        // il SUO annuncio riporterebbe leader:Some(rumpleteazer).
        skimble.handle_event(ServiceEvent::Discovered(info(26, "rumpleteazer"), Some(info(26, "rumpleteazer").id)));
        assert_eq!(s_role(&skimble), Role::Client(info(26, "rumpleteazer").id));

        // --- Arrivo tardivo: quaxo (IP più basso di TUTTI) ---
        // quaxo scopre rumpleteazer (annuncia leader:Some(rumpleteazer), essendo Server)
        // e skimble (annuncia anch'esso leader:Some(rumpleteazer), essendo suo Client).
        quaxo.handle_event(ServiceEvent::Discovered(info(26, "rumpleteazer"), Some(info(26, "rumpleteazer").id)));
        quaxo.handle_event(ServiceEvent::Discovered(info(35, "skimble"), Some(info(26, "rumpleteazer").id)));

        assert_eq!(
            s_role(&quaxo), Role::Client(info(26, "rumpleteazer").id),
            "BUG 2: quaxo (.24, IP più basso di tutti) deve adottare il server già eletto \
             (rumpleteazer), mai autoeleggersi: ruolo={:?}", s_role(&quaxo)
        );
    }

    /// Variante §4 dello spec, RISCRITTA per Task 8: la versione originale di questo
    /// test simulava il ritardo "leader riportato ma non ancora membro del roster"
    /// facendo dare all'umano il CONSENSO a skimble PRIMA che a rumpleteazer
    /// (`ServiceEvent::Consent`, ora rimosso — l'elezione non dipende più da un
    /// consenso pre-connessione, vedi `decide_and_connect`). Quella premessa non è
    /// più riproducibile: un peer `Discovered` entra SUBITO nel roster dell'elezione.
    ///
    /// L'invariante protetto resta però identico e ancora valido: un leader
    /// RIPORTATO (`reported_leader`, da un annuncio altrui) ma non ancora un
    /// MEMBRO del nostro roster non deve mai far ricadere l'elezione su
    /// `members.lowest()` (che eleggerebbe quaxo, sbagliato) — deve restare
    /// `Undecided` finché il leader riportato non viene scoperto anche da noi.
    /// Qui il ritardo si ottiene ritardando la SCOPERTA stessa (non il consenso):
    /// quaxo scopre PRIMA skimble (che riporta rumpleteazer come leader) e SOLO
    /// DOPO rumpleteazer.
    #[test]
    fn late_joiner_stays_undecided_until_reported_leader_is_discovered() {
        let mut quaxo = AiChatService::new_for_test(info(24, "quaxo"));
        let rumpleteazer = info(26, "rumpleteazer");
        let skimble = info(35, "skimble");

        // quaxo scopre SOLO skimble: roster locale = {quaxo, skimble}. skimble
        // riporta rumpleteazer come leader creduto → `reported_leader` si valorizza
        // subito, ma rumpleteazer NON è ancora un membro del roster di quaxo.
        quaxo.handle_event(ServiceEvent::Discovered(skimble.clone(), Some(rumpleteazer.id)));
        assert_eq!(quaxo.reported_leader_for_test(), Some(rumpleteazer.id));
        assert_eq!(
            s_role(&quaxo), Role::Undecided,
            "senza rumpleteazer nel roster, quaxo deve restare Undecided, mai autoeleggersi"
        );

        // Poi quaxo scopre anche rumpleteazer: ora è un membro → Client(rumpleteazer).
        quaxo.handle_event(ServiceEvent::Discovered(rumpleteazer.clone(), Some(rumpleteazer.id)));
        assert_eq!(s_role(&quaxo), Role::Client(rumpleteazer.id));
    }

    // -------------------------------------------------------------------------
    // §3.6 dello spec: `believed_leader_handle()` — l'handle condiviso letto dal task
    // di scoperta UDP in `main.rs` per includere il leader attuale nei propri annunci.
    // -------------------------------------------------------------------------

    /// L'handle deve riflettere il leader attualmente creduto da questo nodo, sempre
    /// aggiornato allo stesso punto in cui `decide_and_connect` calcola `current_leader`
    /// per il log di elezione. Prima di qualunque elezione: `None`.
    #[test]
    fn believed_leader_handle_reflects_current_leader_after_election() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let handle = s.believed_leader_handle();
        assert_eq!(*handle.lock().unwrap(), None, "nessuna elezione ancora avvenuta");

        s.handle_event(ServiceEvent::Discovered(info(20, "macavity"), None));

        assert_eq!(
            *handle.lock().unwrap(), Some(info(10, "skimble").id),
            "l'handle deve riflettere il leader eletto (me stesso, IP più basso)"
        );
    }

    // -------------------------------------------------------------------------
    // Debito #2 (Slice B): teardown reale — `run` deve uscire dal proprio loop
    // quando lo `shutdown` (CancellationToken) viene cancellato dall'esterno.
    //
    // Prima del fix, `run` possedeva sia `inbox_tx` sia `new_link_tx`: c'era quindi
    // SEMPRE almeno un sender vivo sull'inbox, quindi `inbox_rx.recv()` non tornava
    // mai `None` e il `break` esistente era irraggiungibile — il task `run` girava
    // per sempre, senza alcun modo pulito di fermarlo dall'esterno.
    // -------------------------------------------------------------------------

    /// Senza cancellazione, `run` non termina mai: questo test verifica che UN
    /// `token.cancel()` esterno faccia uscire il loop entro una soglia breve.
    ///
    /// `tokio::time::timeout` è la controparte async di un "aspetta al massimo N,
    /// altrimenti considera fallito": se `run` non rispondesse al token (comportamento
    /// pre-fix), il `JoinHandle` non completerebbe mai e il timeout scadrebbe →
    /// `timeout(..).await` ritornerebbe `Err(Elapsed)`, non `Ok(..)` → test fallito.
    #[tokio::test]
    async fn run_exits_on_cancellation() {
        let svc = AiChatService::new_for_test(info(10, "skimble"));
        let (inbox_tx, inbox_rx) = tokio::sync::mpsc::unbounded_channel::<ServiceEvent>();
        let token = tokio_util::sync::CancellationToken::new();

        // `run` consuma `self`: lo spawniamo come task indipendente, esattamente
        // come fa `main.rs` in produzione (`tokio::spawn(service.run(...))`).
        let handle = tokio::spawn(svc.run(inbox_rx, inbox_tx.clone(), token.clone()));

        // Segnala lo spegnimento: nessun evento applicativo, solo il token.
        token.cancel();

        // Se `run` reagisce al token, il task termina ben entro 2s (in pratica
        // quasi subito: `select!` si risveglia sul ramo `shutdown.cancelled()`).
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), handle).await;
        assert!(
            result.is_ok(),
            "run() non è terminato entro il timeout dopo la cancellazione — \
             il ramo di shutdown nel select! non è (ancora) cablato"
        );
    }

    #[test]
    fn extract_memoria_marker_returns_none_without_marker() {
        assert_eq!(extract_memoria_marker("Ciao, come va?"), None);
    }

    #[test]
    fn extract_memoria_marker_extracts_text_after_prefix() {
        assert_eq!(
            extract_memoria_marker("Va bene.\nMEMORIA: il companion preferisce risposte brevi."),
            Some("il companion preferisce risposte brevi.".to_string())
        );
    }

    #[test]
    fn extract_memoria_marker_takes_only_the_first_occurrence() {
        let text = "MEMORIA: prima nota\nMEMORIA: seconda nota";
        assert_eq!(extract_memoria_marker(text), Some("prima nota".to_string()));
    }

    #[test]
    fn extract_memoria_marker_returns_none_for_empty_note() {
        assert_eq!(extract_memoria_marker("MEMORIA:    "), None);
    }

    /// `append_memory_note` (2.0, Task 4): compone `ai_adapter::memory_file_path`
    /// (già testata in `ai_adapter.rs`) + `append_note_at` (già testata sotto)
    /// — verifica solo che la composizione scriva DAVVERO nel file atteso,
    /// niente più test duplicati sulla risoluzione del path (rimossi con la
    /// `LARE_LOCAL_DIR`/`LOCALAPPDATA` che risolvevano qui prima del fix D6).
    #[test]
    fn append_memory_note_writes_to_config_dir_memory_file() {
        let dir = tempfile::tempdir().unwrap();
        append_memory_note(dir.path(), "rumpleteazer", "prima nota").unwrap();
        let content = std::fs::read_to_string(dir.path().join("memory-rumpleteazer.md")).unwrap();
        assert_eq!(content, "prima nota\n");
    }

    #[test]
    fn append_note_at_creates_directory_and_file_when_absent() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("nested").join("memory-test.md");
        append_note_at(&path, "prima nota").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "prima nota\n");
    }

    #[test]
    fn append_note_at_appends_without_overwriting_existing_content() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("memory-test.md");
        append_note_at(&path, "prima nota").unwrap();
        append_note_at(&path, "seconda nota").unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "prima nota\nseconda nota\n"
        );
    }

    #[test]
    fn peer_msg_note_updated_merges_and_pushes_to_ui() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let from = PeerId(std::net::Ipv4Addr::new(192, 168, 1, 20));
        let incoming = Note {
            id: "192.168.1.20:0".into(),
            title: "Remota".into(),
            title_touched: ("rumpleteazer".into(), 100),
            segments: vec![Segment { machine: "rumpleteazer".into(), seq: 1, text: "corpo".into(), edited_at_ms: 100 }],
            created_by: "rumpleteazer".into(),
            created_at_ms: 100,
            deleted: false,
        };
        let effects = s.handle_event(ServiceEvent::PeerMsg { from, msg: ChatMsg::NoteUpdated { note: incoming } });

        assert!(effects.iter().any(|e| matches!(e, Effect::SaveNotes)));
        let view = effects.iter().find_map(|e| match e {
            Effect::ToUi(ServerMsg::NoteUpserted { note }) => Some(note.clone()),
            _ => None,
        }).expect("deve emettere NoteUpserted");
        assert_eq!(view.title, "Remota");
    }

    #[test]
    fn peer_msg_note_updated_fans_out_to_other_links_excluding_sender() {
        let mut s = AiChatService::new_for_test(info(10, "skimble")); // io sono il server in questo scenario
        let sender = PeerId(std::net::Ipv4Addr::new(192, 168, 1, 20));
        let other = PeerId(std::net::Ipv4Addr::new(192, 168, 1, 30));
        s.handle_event(ServiceEvent::Discovered(info(20, "rumpleteazer"), None));
        s.handle_event(ServiceEvent::Discovered(info(30, "mungojerrie"), None));
        s.mark_connected_for_test(sender);
        s.mark_connected_for_test(other);

        let incoming = Note {
            id: "192.168.1.20:0".into(),
            title: "T".into(),
            title_touched: ("rumpleteazer".into(), 100),
            segments: vec![],
            created_by: "rumpleteazer".into(),
            created_at_ms: 100,
            deleted: false,
        };
        let effects = s.handle_event(ServiceEvent::PeerMsg { from: sender, msg: ChatMsg::NoteUpdated { note: incoming } });

        assert!(
            effects.iter().any(|e| matches!(e, Effect::SendToPeer(p, ChatMsg::NoteUpdated { .. }) if *p == other)),
            "deve rimbalzare a `other`: {effects:?}"
        );
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::SendToPeer(p, ChatMsg::NoteUpdated { .. }) if *p == sender)),
            "NON deve rimandare al mittente: {effects:?}"
        );
    }

    /// FIX 5 (review finale di branch) — un merge che NON cambia nulla non deve
    /// produrre effetti.
    ///
    /// Design §5.2 punto 4: si ri-broadcasta solo quando la nota "è CAMBIATA
    /// rispetto a quanto aveva prima". Ricevere due volte la stessa identica nota
    /// (retrasmissione, riconciliazione periodica, rimbalzo di un altro peer)
    /// deve essere un vero no-op: nessun `SendToPeer` (che rimbalzerebbe in rete
    /// all'infinito fra i peer), nessun `SaveNotes` (I/O inutile), nessun `ToUi`
    /// (ri-render inutile della Library).
    #[test]
    fn peer_msg_note_updated_identical_to_stored_produces_no_effects() {
        let mut s = AiChatService::new_for_test(info(10, "skimble")); // io sono il server
        let sender = PeerId(std::net::Ipv4Addr::new(192, 168, 1, 20));
        let other = PeerId(std::net::Ipv4Addr::new(192, 168, 1, 30));
        s.handle_event(ServiceEvent::Discovered(info(20, "rumpleteazer"), None));
        s.handle_event(ServiceEvent::Discovered(info(30, "mungojerrie"), None));
        s.mark_connected_for_test(sender);
        s.mark_connected_for_test(other);

        let incoming = Note {
            id: "192.168.1.20:0".into(),
            title: "T".into(),
            title_touched: ("rumpleteazer".into(), 100),
            segments: vec![Segment {
                machine: "rumpleteazer".into(),
                seq: 1,
                text: "corpo".into(),
                edited_at_ms: 100,
            }],
            created_by: "rumpleteazer".into(),
            created_at_ms: 100,
            deleted: false,
        };

        // Primo arrivo: genuinamente nuova → effetti attesi (UI + salvataggio + fan-out).
        let first = s.handle_event(ServiceEvent::PeerMsg {
            from: sender,
            msg: ChatMsg::NoteUpdated { note: incoming.clone() },
        });
        assert!(!first.is_empty(), "il primo arrivo NON è un no-op: {first:?}");

        // Secondo arrivo, byte per byte identico: il merge non cambia nulla.
        let second = s.handle_event(ServiceEvent::PeerMsg {
            from: sender,
            msg: ChatMsg::NoteUpdated { note: incoming },
        });

        assert!(
            second.is_empty(),
            "merge no-op: nessun ToUi/SaveNotes/SendToPeer (fan-out incluso verso {other:?}): {second:?}"
        );
    }

    // --- Task 9: timer di debounce di riconciliazione agganciato ai link ---
    // Design: Docs/superpowers/specs/2026-07-29-library-notes-design.md §5.2, §9.
    //
    // Nota: `mark_connected_for_test` (a differenza di quanto assumerebbe una firma
    // a 2 argomenti) alloca la generazione da sé, dal contatore condiviso `next_gen`
    // — esattamente come farebbe un vero link. Per fissare deterministicamente la
    // generazione "corrente" al valore atteso dal test, sovrascriviamo con
    // `set_link_gen_for_test` subito dopo (stesso pattern già usato altrove in questo
    // file, es. `PeerGone`/`VoteTimeout`, per costruire scenari di generazione stale).

    #[test]
    fn notes_reconcile_due_sends_digest_when_generation_matches() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let peer = PeerId(std::net::Ipv4Addr::new(192, 168, 1, 20));
        s.mark_connected_for_test(peer);
        s.set_link_gen_for_test(peer, 7); // generazione 7 registrata

        let effects = s.handle_event(ServiceEvent::NotesReconcileDue { peer_id: peer, generation: 7 });

        assert!(effects.iter().any(|e| matches!(e, Effect::SendToPeer(p, ChatMsg::NotesDigest { .. }) if *p == peer)));
    }

    #[test]
    fn notes_reconcile_due_is_noop_for_stale_generation() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let peer = PeerId(std::net::Ipv4Addr::new(192, 168, 1, 20));
        s.mark_connected_for_test(peer);
        s.set_link_gen_for_test(peer, 7); // generazione ATTUALE è 7

        // Un timer di una generazione precedente (5) è stantio: il link è stato
        // rimpiazzato — stesso principio del guard già usato da VoteTimeout.
        let effects = s.handle_event(ServiceEvent::NotesReconcileDue { peer_id: peer, generation: 5 });

        assert!(effects.is_empty(), "timer stantio: nessun effetto");
    }

    #[test]
    fn digest_entries_reflect_all_known_notes() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        s.handle_event(ServiceEvent::NoteCreateRequested { title: "A".into(), text: "x".into() });
        let peer = PeerId(std::net::Ipv4Addr::new(192, 168, 1, 20));
        s.mark_connected_for_test(peer);
        let gen = s.link_gen_for_test(peer).expect("mark_connected_for_test deve assegnare una generazione");

        let effects = s.handle_event(ServiceEvent::NotesReconcileDue { peer_id: peer, generation: gen });

        let entries = effects.iter().find_map(|e| match e {
            Effect::SendToPeer(_, ChatMsg::NotesDigest { entries }) => Some(entries.clone()),
            _ => None,
        }).expect("deve mandare un digest");
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn peer_msg_notes_digest_replies_with_want_and_push() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        s.handle_event(ServiceEvent::NoteCreateRequested { title: "Mine".into(), text: "x".into() });
        let from = PeerId(std::net::Ipv4Addr::new(192, 168, 1, 20));

        // Il peer manda un digest che menziona una nota che noi non abbiamo.
        let their_digest = crate::notes::digest::NoteDigest {
            deleted: false,
            title_touched: ("rumpleteazer".into(), 0),
            segment_keys: vec![("rumpleteazer".into(), 1)],
        };
        let effects = s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::NotesDigest { entries: vec![("192.168.1.20:0".into(), their_digest)] },
        });

        let (want, push) = effects.iter().find_map(|e| match e {
            Effect::SendToPeer(p, ChatMsg::NotesDigestReply { want, push }) if *p == from => Some((want.clone(), push.clone())),
            _ => None,
        }).expect("deve rispondere con NotesDigestReply");
        assert_eq!(want, vec!["192.168.1.20:0".to_string()], "la nota del peer non la abbiamo: la vogliamo");
        assert_eq!(push.len(), 1, "la nostra 'Mine' non è nel loro digest: gliela mandiamo");
    }

    #[test]
    fn peer_msg_notes_digest_reply_fulfills_want_with_notes_data() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        s.handle_event(ServiceEvent::NoteCreateRequested { title: "Mine".into(), text: "x".into() });
        let mine_id = s.notes.all()[0].id.clone();
        let from = PeerId(std::net::Ipv4Addr::new(192, 168, 1, 20));

        let effects = s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::NotesDigestReply { want: vec![mine_id.clone()], push: vec![] },
        });

        let notes = effects.iter().find_map(|e| match e {
            Effect::SendToPeer(p, ChatMsg::NotesData { notes }) if *p == from => Some(notes.clone()),
            _ => None,
        }).expect("deve mandare NotesData per soddisfare want");
        assert_eq!(notes[0].id, mine_id);
    }

    #[test]
    fn peer_msg_notes_digest_reply_merges_pushed_notes_and_fans_out() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let from = PeerId(std::net::Ipv4Addr::new(192, 168, 1, 20));
        let other = PeerId(std::net::Ipv4Addr::new(192, 168, 1, 30));
        s.handle_event(ServiceEvent::Discovered(info(20, "rumpleteazer"), None));
        s.handle_event(ServiceEvent::Discovered(info(30, "mungojerrie"), None));
        s.mark_connected_for_test(from);
        s.mark_connected_for_test(other);

        let pushed = Note {
            id: "192.168.1.20:0".into(),
            title: "Loro".into(),
            title_touched: ("rumpleteazer".into(), 0),
            segments: vec![],
            created_by: "rumpleteazer".into(),
            created_at_ms: 0,
            deleted: false,
        };
        let effects = s.handle_event(ServiceEvent::PeerMsg {
            from,
            msg: ChatMsg::NotesDigestReply { want: vec![], push: vec![pushed] },
        });

        assert!(effects.iter().any(|e| matches!(e, Effect::SaveNotes)));
        assert!(effects.iter().any(|e| matches!(e, Effect::ToUi(ServerMsg::NoteUpserted { .. }))));
        assert!(
            effects.iter().any(|e| matches!(e, Effect::SendToPeer(p, ChatMsg::NoteUpdated { .. }) if *p == other)),
            "design §5.2 punto 4: una nota fusa via riconciliazione va ribroadcastata agli altri già collegati"
        );
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::SendToPeer(p, ChatMsg::NoteUpdated { .. }) if *p == from)),
            "MAI rimandarla a chi ce l'ha appena mandata"
        );
    }

    #[test]
    fn peer_msg_notes_data_merges_each_note_and_pushes_to_ui() {
        let mut s = AiChatService::new_for_test(info(10, "skimble"));
        let from = PeerId(std::net::Ipv4Addr::new(192, 168, 1, 20));
        let a = Note {
            id: "192.168.1.20:0".into(), title: "A".into(), title_touched: ("rumpleteazer".into(), 0),
            segments: vec![], created_by: "rumpleteazer".into(), created_at_ms: 0, deleted: false,
        };
        let b = Note {
            id: "192.168.1.20:1".into(), title: "B".into(), title_touched: ("rumpleteazer".into(), 0),
            segments: vec![], created_by: "rumpleteazer".into(), created_at_ms: 0, deleted: false,
        };

        let effects = s.handle_event(ServiceEvent::PeerMsg { from, msg: ChatMsg::NotesData { notes: vec![a, b] } });

        let upserted: Vec<_> = effects.iter().filter_map(|e| match e {
            Effect::ToUi(ServerMsg::NoteUpserted { note }) => Some(note.id.clone()),
            _ => None,
        }).collect();
        assert_eq!(upserted.len(), 2);
        assert!(effects.iter().any(|e| matches!(e, Effect::SaveNotes)));
    }
}
