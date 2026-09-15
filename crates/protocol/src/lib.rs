//! # protocol
//!
//! Shared message types for the Lare Terminal WebSocket protocol.
//!
//! Defines [`ClientMsg`] (UI/channel → orchestrator) and [`ServerMsg`]
//! (orchestrator → UI/channel) together with their supporting enums.
//!
//! ## Wire format
//! Both enums use `#[serde(tag = "type", rename_all = "snake_case")]`,
//! producing a `"type"` discriminator field in every JSON object.
//! Supporting enums (`InputMode`, `CommandKind`, `ErrCode`) are also
//! `snake_case` on the wire.
//!
//! ## Design notes (SOLID)
//! This crate owns **only the contract**: no network I/O, no business logic.
//! Every consumer (UI, orchestrator, tests) depends on this interface crate.

use serde::{Deserialize, Serialize};

// ── Supporting enums ──────────────────────────────────────────────────────

/// Origine di un risultato di ricerca (ordine di priorità di scansione).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchSource {
    Cwd,
    Standard,
    Cloud,
    External,
}

/// How the user provided the input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputMode {
    Keyboard,
    Voice,
}

/// Routing hint from the client.
/// `Auto` means the orchestrator decides (server-side routing).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandKind {
    /// Server decides the route (OS or NL).
    Auto,
    /// Force OS command path.
    Os,
    /// Force natural-language path.
    Nl,
}

/// Error category codes sent by the orchestrator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrCode {
    OsError,
    AiError,
    RoutingError,
}

/// Discriminates the kind of content to display in a custom window.
///
/// Wire format: `snake_case` (e.g. `Markdown` → `"markdown"`, `Help` → `"help"`).
/// Designed for extension: future variants (`Table`, `Html`, …) are additive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowKind {
    /// The content is a Markdown string; the UI must parse and sanitise it.
    Markdown,
    /// System help window: lists slash commands and keyboard shortcuts.
    /// The UI should apply a distinct visual style (e.g. `body.help` CSS class)
    /// to mark this as an "official" system-generated window.
    Help,
    /// Finestra di ricerca: viva e interattiva (righe in append, click per aprire).
    Search,
}

/// Ruolo di una connessione WS (spec §4.1). Deciso dal client nella `Hello`.
///
/// - `Ui`: `ui.exe` (pagina host nascosta, finestre) — è il default per
///   compatibilità: un client v1 non manda il campo e resta valido.
/// - `Shell`: una sessione `lare-shell` (host PowerShell). Il suo `ToolClient`
///   è la shell dell'utente stesso (`ExecInShell`/`ExecResult`), la sua cwd è
///   per-connessione, e l'output dei comandi slash va in una finestra su `ui`.
///
/// Wire: `"ui"` | `"shell"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    #[default]
    Ui,
    Shell,
}

/// Superficie di destinazione di un `ServerMsg` emesso durante un turno
/// originato da una connessione **shell** (spec §3.2/§4). Per una connessione
/// `ui` non cambia nulla: tutto torna alla connessione stessa, come in v1.
///
/// - `Origin`: torna alla connessione che ha mandato il `Command` (la shell):
///   avanzamento del turno, gate, esecuzioni, risposte a richieste puntuali.
/// - `Ui`: apre o aggiorna finestre, o alimenta un relay (AI Chat, Library,
///   Share): va al sink `ui` unico della macchina.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    Origin,
    Ui,
}

/// Voce nella lista screener mostrata dal picker (canale `financial-markets`).
/// `id` è lo `screener_id` da passare al tool `run_screener`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScreenerListItem {
    pub id: String,
    pub title: String,
    pub description: String,
}

/// Destinazione di una condivisione Library — una macchina specifica o tutte quelle
/// attualmente collegate. Vedi `Docs/superpowers/specs/2026-07-02-library-share-with-design.md` §5.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShareTarget {
    One { label_base: String },
    All,
}

/// Vista "dumb" di una nota per la UI: il corpo arriva già renderizzato
/// (segmenti per macchina già uniti, intestazioni già inserite se 2+) — il
/// frontend non vede mai `Segment` grezzi. V. design §7.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteView {
    pub id: String,
    pub title: String,
    pub body: String,
    /// Il solo segmento di QUESTA macchina (stringa vuota se non ha ancora
    /// contribuito). Serve a precompilare la finestra "Modifica" col testo
    /// che l'utente sta davvero per sovrascrivere — senza, la casella di
    /// modifica appariva vuota anche su una nota con contenuto, trovato nello
    /// smoke test dal vivo del 2026-07-31.
    pub my_segment_text: String,
    pub created_by: String,
    pub created_at_ms: u64,
    pub deleted: bool,
}

/// Esito di un trasferimento Share, riportato al MITTENTE (`ServerMsg::ShareResult`).
/// `Accepted` riporta il consenso del destinatario, non la scrittura su disco effettiva
/// (che può avvenire più tardi — vedi spec §9.1, §5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShareOutcome {
    Accepted,
    Rejected,
    Failed { reason: String },
}

/// Una riga di storico chat: chi l'ha scritta e cosa. Usata da `ServerMsg::AiChatHistory`
/// per il dump in blocco dello storico (riapertura finestra o catch-up di un nuovo peer).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatLine {
    pub from_label: String,
    pub text: String,
    /// Nickname risolto dal mittente al momento dell'invio (umano o AI, in base
    /// a chi ha scritto). `None` → il frontend mostra `from_label` come oggi
    /// (fallback, comportamento invariato per peer/storico vecchi).
    #[serde(default)]
    pub display_name: Option<String>,
    /// True se il mittente è l'AI (non un umano) — pilota badge/icona nel
    /// frontend. `false` di default: un peer vecchio che non manda questo
    /// campo appare come umano (comportamento storico, nessuna regressione
    /// visibile: prima d'ora non esisteva alcun badge).
    #[serde(default)]
    pub is_ai: bool,
}

// ── ClientMsg ─────────────────────────────────────────────────────────────

/// Messages from the client (UI or channel) to the orchestrator.
///
/// Wire format: `{"type":"<variant>", ...fields }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    /// Handshake. MUST be the first message after connecting.
    Hello {
        token: String,
        /// Canale esterno richiesto da questa connessione (es. `"nmap"`).
        /// `None` = comportamento di sempre (cursore/Telegram, ToolClient di
        /// default). Vedi `Docs/superpowers/specs/2026-07-16-external-tool-channel-design.md`.
        #[serde(default)]
        channel: Option<String>,
        /// Ruolo della connessione (2.0, spec §4.1). Assente → `Ui`.
        #[serde(default)]
        role: Role,
        /// Id della sessione shell (generato dalla host, o passato da `ui.exe`
        /// con `--session`): lega la connessione alla finestra terminale.
        /// Solo per `role: Shell`; `ui` lo lascia assente.
        #[serde(default)]
        session_id: Option<String>,
        /// cwd iniziale della sessione shell (`$PWD` del runspace all'avvio).
        #[serde(default)]
        cwd: Option<String>,
        /// Versione del client (es. `lare-shell 2.0.0`), mostrata da `/ping`.
        #[serde(default)]
        version: Option<String>,
    },

    /// Execute a command. `id` must be unique per session for correlation.
    Command {
        id: String,
        input: String,
        input_mode: InputMode,
        command_type: CommandKind,
        /// Working directory. `null` means the orchestrator uses its default.
        cwd: Option<String>,
        /// Se true, il client autorizza la ricerca web interna dell'AI per
        /// questo comando. Campo additivo: i client che non lo inviano → false.
        #[serde(default)]
        web_search: bool,
        /// Lingua della risposta AI richiesta dal client (es. "it", "en").
        /// Campo additivo: i client che non lo inviano → stringa vuota "" (default).
        #[serde(default)]
        lang: String,
    },

    /// Keep-alive ping; `ts` is a client-side Unix timestamp (ms or s).
    Ping { ts: u64 },

    /// Annulla la ricerca `id` in corso (es. l'utente ha chiuso la finestra).
    CancelSearch { id: String },

    /// Pauses the running search identified by `id`.
    /// The walkers suspend at the next checkpoint; the search state is preserved.
    PauseSearch { id: String },

    /// Resumes a previously paused search identified by `id`.
    /// The walkers continue from where they stopped.
    ResumeSearch { id: String },

    /// Cancels the in-progress command `id` (AI or OS).
    /// No-op if the command has already completed.
    CancelCommand { id: String },

    // ── Plugin window messages (Contract A, Slice 1 — additive) ─────────────
    //
    // These two variants form the client→server half of the plugin-window
    // round-trip.  They are sent by the UI's plugin-window JavaScript (Task 6)
    // and dispatched by the orchestrator's WS handler (Task 5).
    //
    // Wire names (snake_case via `rename_all`):
    //   PluginUiEvent     → "plugin_ui_event"
    //   PluginWindowClosed → "plugin_window_closed"
    /// Carries a UI interaction event from a plugin window to the orchestrator.
    ///
    /// The UI sends this when the user interacts with an element that carries a
    /// `data-evt` attribute inside a plugin window.  The orchestrator forwards
    /// it as `HostToPlugin::UiEvent` to the appropriate plugin process.
    ///
    /// `value` is `Some(...)` for input/select elements, `None` for buttons.
    ///
    /// Wire format: `{"type":"plugin_ui_event","window_id":N,"element_id":"...","value":null}`
    PluginUiEvent {
        /// Identifies which plugin window originated the event.
        window_id: u64,
        /// The `data-evt` attribute value of the interacted element.
        element_id: String,
        /// Current value for input/select elements; `null` for buttons.
        value: Option<String>,
    },

    /// Notifies the orchestrator that the user closed a plugin window.
    ///
    /// The UI sends this when the WebviewWindow for a plugin is about to be
    /// destroyed (e.g. the user clicked the close button).  The orchestrator
    /// removes the window from its registry and may optionally send a
    /// `HostToPlugin::Deinit` to the plugin process.
    ///
    /// Wire format: `{"type":"plugin_window_closed","window_id":N}`
    PluginWindowClosed {
        /// Identifies the plugin window that was closed.
        window_id: u64,
    },

    // ── AI Chat variants (Contract A, Slice 1a-ui — additive) ────────────────
    //
    // Queste varianti compongono la metà client→server del canale AI Chat
    // (Slice 1a-ui).  Sono puramente additive: nessuna variante esistente cambia.
    //
    // Wire names (snake_case via `rename_all`):
    //   AiChatSend             → "ai_chat_send"
    //   AiChatJoinConsent      → "ai_chat_join_consent"
    //   AiChatClosed           → "ai_chat_closed"
    //   AiChatOpen             → "ai_chat_open"
    //   AiChatJoinDecision     → "ai_chat_join_decision"      (ammissione, vedi sotto)
    //   AiChatAdmissionVote    → "ai_chat_admission_vote"     (ammissione, vedi sotto)
    //   AiChatRequestAdmission → "ai_chat_request_admission" (ammissione, vedi sotto)
    /// L'umano invia un messaggio nella stanza AI Chat.
    AiChatSend { text: String },
    /// Risposta al consenso d'ingresso per uno specifico peer.
    AiChatJoinConsent { peer_label: String, accept: bool },
    /// La superficie chat è stata chiusa: l'umano esce dalla stanza.
    AiChatClosed {},
    /// La finestra-chat è stata (ri)aperta: richiede al servizio di ri-registrare
    /// il sink WS e ri-inviare l'ultimo roster noto al nuovo webview.
    ///
    /// Inviato da app.js ogni volta che `aichat:ready` emette (nuova webview aperta).
    /// Necessario perché, dopo `AiChatClosed`, il `server_tx` viene azzerato;
    /// la riapertura della finestra non crea una nuova connessione WS (stessa connessione),
    /// quindi l'unico modo per re-registrare il sink è questo messaggio esplicito.
    AiChatOpen {},

    // ── AI Chat — ammissione alla stanza (additive) ──────────────────────────
    //
    // Sostituisce il vecchio consenso pairwise (`AiChatJoinConsent` sopra) con
    // un'ammissione a voto coordinata dal server eletto: gate 1 (il nuovo arrivato
    // decide se provare a entrare) + gate 2 (i presenti votano se ammetterlo).
    // Vedi Docs/superpowers/plans/2026-07-03-aichat-admission.md.
    //
    // Wire names (snake_case via `rename_all`):
    //   AiChatJoinDecision     → "ai_chat_join_decision"
    //   AiChatAdmissionVote    → "ai_chat_admission_vote"
    //   AiChatRequestAdmission → "ai_chat_request_admission"
    /// Risposta al gate 1 (`ServerMsg::AiChatJoinPrompt`): l'umano nuovo arrivato ha
    /// deciso se provare a entrare nella stanza con i presenti mostrati. `accept: false`
    /// annulla il tentativo (nessuna connessione al server); `accept: true` fa scattare
    /// la richiesta di ammissione (`ChatMsg::RequestAdmission` verso il server eletto).
    AiChatJoinDecision { accept: bool },
    /// Risposta al gate 2 (`ServerMsg::AiChatAdmissionRequest`): un presente ha votato
    /// se ammettere `candidate`. `accept: false` è un veto immediato (basta un solo no);
    /// gli altri presenti che non rispondono entro il timeout contano come sì.
    AiChatAdmissionVote { candidate: String, accept: bool },
    /// Pulsante "chiedi di entrare" dopo un rifiuto (`ServerMsg::AiChatRejected`):
    /// re-invia la richiesta di ammissione. La UI applica già un cooldown lato client
    /// prima di abilitare il pulsante; l'orchestrator non deve fidarsi ciecamente del
    /// timing del client, ma questa slice non introduce un rate-limit server-side.
    AiChatRequestAdmission {},

    // ── Library "Share with" (Slice 1a — additive) ───────────────────────────
    //
    // Round-trip di consenso per condividere un documento Library con un'altra
    // macchina. Il trasferimento vero del contenuto (Slice 2a) usa un secondo
    // giro di richiesta: `ServerMsg::ShareContentRequest` → questa risposta.
    //
    // Wire names (snake_case via `rename_all`):
    //   ShareDocument       → "share_document"
    //   ShareConsent        → "share_consent"
    //   ShareContent        → "share_content"
    //   ShareContentFailed  → "share_content_failed"
    //   ShareWritten        → "share_written"
    /// Trigger dalla UI: condividi un documento Library. `doc_name`/`size_bytes` sono
    /// già risolti dalla UI (che ha accesso al filesystem della Library via `archive::open`)
    /// — l'orchestrator non tocca mai `library/documents/` (confine SRP). `rel_path`
    /// resta nel messaggio: serve a costruire `ServerMsg::ShareContentRequest` quando
    /// arriva un `ShareAccept` (Slice 2a).
    ShareDocument { rel_path: String, doc_name: String, size_bytes: u64, target: ShareTarget },
    /// Risposta dell'umano (o, in una slice futura, dell'AI locale) al banner di
    /// consenso mostrato da `ServerMsg::ShareRequest`.
    ShareConsent { share_id: String, accept: bool },
    /// (Slice 2a) La UI ha letto con successo il documento richiesto da
    /// `ServerMsg::ShareContentRequest` (via `archive_open`).
    ShareContent { share_id: String, title: String, content: String },
    /// (Slice 2a) La UI non è riuscita a leggere il documento richiesto (cancellato,
    /// spostato, permessi negati). `reason` è sempre la frase fissa
    /// `"documento non più disponibile"` — non l'errore tecnico grezzo di `archive_open`
    /// (design §2: l'esito pratico per il mittente è lo stesso in tutti i casi).
    ShareContentFailed { share_id: String, reason: String },
    /// (Slice 2a) La UI del DESTINATARIO conferma di aver scritto su disco un documento
    /// ricevuto (via `archive_save`) — l'orchestrator lo rimuove dalla coda di
    /// trasferimenti pendenti. Idempotente: un ack per uno `share_id` già rimosso è un
    /// no-op silenzioso.
    ShareWritten { share_id: String },

    // ── Gate di conferma locale per tool sensibili (additiva) ───────────────
    //
    // Estende ADR-007 (Docs/06-decisions.md) oltre il solo canale Telegram: un
    // tool marcato "sensibile" richiede conferma esplicita dell'utente anche
    // dalla UI locale. Design:
    // Docs/superpowers/specs/2026-07-15-local-tool-confirm-gate-design.md.
    //
    // Wire name: ToolConfirmResponse → "tool_confirm_response"
    /// Risposta dell'utente a un `ServerMsg::ToolConfirmRequest`. `id` è l'id
    /// opaco ricevuto nella richiesta; `accept: true` = esegui, `false` = annulla.
    ToolConfirmResponse { id: String, accept: bool },

    // ── Library "Blocco note" (additiva) ──────────────────────────────────
    // Wire names: NoteCreate → "note_create", NoteEdit → "note_edit",
    // NoteEditTitle → "note_edit_title", NoteDelete → "note_delete".
    // Design: Docs/superpowers/specs/2026-07-29-library-notes-design.md §7.
    NoteCreate { title: String, text: String },
    /// Sostituisce SOLO il segmento di QUESTA macchina — mai l'intera nota.
    NoteEdit { id: String, text: String },
    /// Last-write-wins separato dal corpo (v. design §3).
    NoteEditTitle { id: String, title: String },
    /// Imposta il tombstone — non rimuove la nota dallo store.
    NoteDelete { id: String },

    // ── Market data source test (additiva) ────────────────────────────
    // Wire name: TestMarketDataSource → "test_market_data_source"
    // Design: Docs/superpowers/specs/2026-08-14-financial-markets-ibkr-data-source/task-10.
    /// Richiede un test di raggiungibilità per la fonte dati mercato
    /// ATTUALMENTE selezionata (letta da market_data.json lato Python).
    /// `id` correla la risposta (`ServerMsg::MarketDataSourceTestResult`),
    /// stesso pattern di `Command`/`Done`.
    TestMarketDataSource { id: String },

    // ── Canale shell (2.0, spec §4.1) — additivi ─────────────────────────
    // Wire names: ExecResult → "exec_result", UiPong → "ui_pong".

    /// Esito di un `ServerMsg::ExecInShell`: la host ha eseguito `command`
    /// nel runspace dell'utente. `output` è vuoto con `capture: false`
    /// (§4.5); `cwd` è `$PWD` dopo il comando (aggiorna la cwd per sessione).
    ExecResult { turn_id: String, exec_id: String, exit_code: i32, output: String, cwd: String },

    /// Risposta di `ui.exe` a `ServerMsg::UiPing` (built-in `/ping`, §3.1).
    UiPong { id: String, version: String },
}

// ── ServerMsg ─────────────────────────────────────────────────────────────

/// Messages from the orchestrator to the client.
///
/// Wire format: `{"type":"<variant>", ...fields }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    /// Sent immediately after a successful `Hello` handshake.
    ServerInfo {
        version: String,
        ai_provider: String,
        capabilities: Vec<String>,
    },

    /// One streaming chunk of output for command `id`.
    Chunk { id: String, content: String },

    /// Terminal message for command `id`.
    /// `exit_code` is `null` when the command was handled by the AI path.
    Done { id: String, exit_code: Option<i32> },

    /// Error for command `id`.
    Error {
        id: String,
        code: ErrCode,
        message: String,
    },

    /// Keep-alive pong; echoes the client `ts`.
    Pong { ts: u64 },

    /// Instructs the UI to open a new custom window displaying `content`.
    ///
    /// Emitted by the orchestrator in response to `/show <markdown>` (Fase 1)
    /// or by the AI adapter in Fase 2.  The UI must create a `WebviewWindow`,
    /// load `window.html`, and render the sanitised content there.
    ///
    /// Wire format: `{"type":"open_window","title":"...","kind":"markdown","content":"..."}`
    ///
    /// Note: unlike `Chunk`/`Done`/`Error`, this message carries NO `id`.
    /// It is self-contained: one message opens one window.
    OpenWindow {
        /// Window title bar text.  Derived from the first non-empty line of
        /// the content (truncated to ~60 chars) or the default `"Lare — Output"`.
        title: String,
        /// Content type — determines how the UI renders `content`.
        kind: WindowKind,
        /// Raw content string.  For `Markdown`, the UI must parse and sanitise
        /// before inserting into the DOM.
        content: String,
    },

    /// Apre la finestra di selezione screener (canale `financial-markets`).
    /// Emesso quando il tool `list_screeners` ritorna con successo — vedi
    /// `Docs/superpowers/specs/2026-08-11-markets-screener-registry-design.md`.
    ///
    /// Wire format: `{"type":"open_screener_picker","items":[...]}`
    OpenScreenerPicker { items: Vec<ScreenerListItem> },

    /// Apre una finestra di ricerca live (window kind `Search`), identificata da `id`.
    SearchOpen { id: String, title: String },

    /// Un risultato della ricerca `id`: percorso completo + sorgente.
    ///
    /// `line`/`snippet` sono presenti solo per un hit di ricerca CONTENUTO
    /// (`/find in:"..."`) — assenti (e OMESSI dal wire) per un hit solo-nome,
    /// così il formato resta invariato per client che non conoscono questi campi.
    SearchHit {
        id: String,
        path: String,
        source: SearchSource,
        /// Numero di riga (1-based) del match nel contenuto. `None` per un hit
        /// solo-nome. `skip_serializing_if` lo fa sparire del tutto dal JSON
        /// quando `None`, invece di serializzarlo come `"line":null` — così un
        /// client vecchio, che non si aspetta questo campo, vede esattamente
        /// lo stesso wire di prima (Contratto A: le aggiunte sono additive).
        #[serde(skip_serializing_if = "Option::is_none")]
        line: Option<u32>,
        /// Frammento di testo attorno al match, per l'anteprima nella finestra
        /// di ricerca. Stessa logica di omissione di `line`.
        #[serde(skip_serializing_if = "Option::is_none")]
        snippet: Option<String>,
    },

    /// Fine della ricerca `id`: totale risultati; `truncated` se ha raggiunto il cap.
    SearchDone {
        id: String,
        count: usize,
        truncated: bool,
    },

    /// Current working directory of the persistent shell session.
    ///
    /// Emitted by the orchestrator:
    /// - once at connection time (initial cwd, after `Hello` handshake);
    /// - again whenever the cwd changes after a command execution.
    ///
    /// Wire format: `{"type":"cwd","path":"<absolute-path>"}`
    Cwd { path: String },

    /// Battito di vita per il comando `id` — nessun contenuto, solo "il turno è
    /// ancora attivo". Origina da un evento SSE `ping` (keep-alive nativo di
    /// Anthropic) propagato da `messages_client.rs`/`ai_adapter.rs` (orchestrator).
    /// Il frontend lo usa per resettare il timer di silenzio del watchdog senza
    /// stampare nulla nel pannello — vedi
    /// Docs/superpowers/specs/2026-08-07-ai-turn-heartbeat-watchdog-design.md.
    Heartbeat { id: String },

    // ── Plugin window messages (Contract A, Slice 1 — additive) ─────────────
    //
    // These three variants form the server→client half of the plugin-window
    // round-trip.  They are emitted by the orchestrator's plugin pump (Task 5)
    // and consumed by the UI's plugin-window handler (Task 6).
    //
    // Design note (SOLID / OCP): all three are purely additive; no existing
    // variant changes.  The `window_id` is a stable u64 token that correlates
    // the open/update/close lifecycle for a single plugin-window instance.
    //
    // Wire names (snake_case via `rename_all`):
    //   OpenPluginWindow   → "open_plugin_window"
    //   UpdatePluginWindow → "update_plugin_window"
    //   ClosePluginWindow  → "close_plugin_window"
    /// Instructs the UI to open a new plugin window and render `html` inside it.
    ///
    /// The orchestrator emits this when a plugin sends `PluginToHost::ShowWindow`.
    /// `window_id` is unique within the session and must be echoed back in any
    /// subsequent `PluginUiEvent` or `PluginWindowClosed` from the UI.
    ///
    /// Wire format (senza dimensione): `{"type":"open_plugin_window","window_id":N,"title":"...","html":"..."}`
    /// Wire format (con dimensione):   `{...,"width":960.0,"height":620.0}`
    ///
    /// `width`/`height` sono OPZIONALI e ADDITIVI: se il plugin non dichiara una
    /// dimensione nel suo `plugin.json`, `skip_serializing_if` li omette del tutto
    /// dal JSON — un client vecchio che non li legge continua a usare il default
    /// generico (480×360). Questo preserva la compatibilità con calc/ping/counter.
    OpenPluginWindow {
        /// Unique identifier for this plugin-window instance.
        window_id: u64,
        /// Title bar text for the window.
        title: String,
        /// Full HTML body to render inside the window (sanitised by the UI with DOMPurify).
        html: String,
        /// Dimensione iniziale richiesta dal plugin (dal suo `plugin.json`), se
        /// dichiarata — vedi `plugin_protocol::WindowSize`. Assente -> l'host usa
        /// il default generico. Cambio ADDITIVO: i client esistenti che non
        /// leggono questi due campi continuano a funzionare (default 480×360).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        width: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        height: Option<f64>,
    },

    /// Instructs the UI to replace the content of an existing plugin window.
    ///
    /// The orchestrator emits this when a plugin sends `PluginToHost::UpdateWindow`.
    /// The update is full-HTML (no element-level patch): the UI replaces the entire
    /// content area with the new `html`.
    ///
    /// Wire format: `{"type":"update_plugin_window","window_id":N,"html":"..."}`
    UpdatePluginWindow {
        /// Identifies the plugin window to update (matches a prior `OpenPluginWindow.window_id`).
        window_id: u64,
        /// New HTML content to replace the entire window body.
        html: String,
    },

    /// Instructs the UI to close a plugin window.
    ///
    /// The orchestrator emits this when a plugin sends `PluginToHost::CloseWindow`.
    ///
    /// Wire format: `{"type":"close_plugin_window","window_id":N}`
    ClosePluginWindow {
        /// Identifies the plugin window to close.
        window_id: u64,
    },

    // ── AI Chat variants (Contract A, Slice 1a-ui — additive) ────────────────
    //
    // Queste tre varianti compongono la metà server→client del canale AI Chat
    // (Slice 1a-ui).  Sono puramente additive: nessuna variante esistente cambia.
    //
    // Wire names (snake_case via `rename_all`):
    //   AiChatMessage          → "ai_chat_message"
    //   AiChatRoster           → "ai_chat_roster"
    //   AiChatJoinRequest      → "ai_chat_join_request"
    //   AiChatHistory          → "ai_chat_history"
    //   AiChatSelf             → "ai_chat_self"
    //   AiChatPeerLost         → "ai_chat_peer_lost"
    //   AiChatReachablePeers   → "ai_chat_reachable_peers"
    //   AiChatJoinPrompt       → "ai_chat_join_prompt"       (ammissione, vedi sotto)
    //   AiChatAdmissionRequest → "ai_chat_admission_request" (ammissione, vedi sotto)
    //   AiChatPending          → "ai_chat_pending"           (ammissione, vedi sotto)
    //   AiChatAdmitted         → "ai_chat_admitted"          (ammissione, vedi sotto)
    //   AiChatRejected         → "ai_chat_rejected"          (ammissione, vedi sotto)
    /// Un messaggio della stanza AI Chat (Slice 1a-ui). `from_label` è l'etichetta
    /// attribuita dal mittente (es. "skimble-human"); il ricevente non la inventa.
    AiChatMessage {
        from_label: String,
        text: String,
        /// Vedi doc-comment su `ChatLine::display_name` — stessa semantica.
        #[serde(default)]
        display_name: Option<String>,
        /// Vedi doc-comment su `ChatLine::is_ai` — stessa semantica.
        #[serde(default)]
        is_ai: bool,
    },
    /// Lista autorevole dei partecipanti presenti nella stanza.
    AiChatRoster { participants: Vec<String> },
    /// Un peer NUOVO è comparso in LAN: l'UI deve chiedere il consenso all'umano
    /// prima che questa macchina entri nella stanza con lui.
    AiChatJoinRequest { peer_label: String },
    /// Storico dei messaggi in blocco: alla riapertura della finestra-chat (se non vuoto)
    /// o quando un peer manda un dump di catch-up dopo il proprio `Join`. Il frontend
    /// svuota il trascritto e lo ri-renderizza per intero (niente merge/dedup).
    AiChatHistory { entries: Vec<ChatLine> },
    /// La nostra etichetta umana in questa stanza (es. "skimble-human") — mandata quando
    /// la UI si (ri)connette al canale, così la finestra-chat sa mostrare "chi sono io"
    /// (titolo finestra) senza doverlo indovinare dal roster.
    AiChatSelf { label: String },
    /// Un peer è sparito dalla stanza AI Chat (rilevato via keepalive o disconnessione
    /// pulita). Usato in ENTRAMBE le direzioni: un client sparito (annunciato dal server,
    /// relay di `ChatMsg::PeerLost`) o il proprio server sparito (rilevato in locale dal
    /// client). Evento LIVE: non entra mai in `AiChatHistory`, nessun replay a riapertura
    /// finestra.
    AiChatPeerLost { label: String },
    /// Etichette (`"<base>-human"`) dei peer scoperti via UDP E con un link TCP
    /// vivo — indipendente dall'ammissione alla stanza AI Chat (`AiChatRoster`).
    /// Consumato SOLO dal dialog "Condividi" della Library: rispecchia
    /// esattamente cosa `request_share` (aichat/service.rs) accetterebbe in
    /// questo istante. Vedi Docs/superpowers/specs/
    /// 2026-07-28-library-share-reachable-peers-design.md.
    AiChatReachablePeers { labels: Vec<String> },

    // ── Library "Blocco note" (additiva) ──────────────────────────────────
    // Wire names: NotesSnapshot → "notes_snapshot", NoteUpserted → "note_upserted".
    /// Replay completo su ogni nuova connessione WS (stesso pattern di
    /// `AiChatReachablePeers`/storico/roster — v. design §7).
    NotesSnapshot { notes: Vec<NoteView> },
    /// Push live: creazione, modifica locale, o cambiamento arrivato dalla rete.
    NoteUpserted { note: NoteView },

    // ── Library "Share with" (Slice 1a — additive) ───────────────────────────
    //
    // Wire names (snake_case via `rename_all`):
    //   ShareRequest         → "share_request"
    //   ShareResult          → "share_result"
    //   ShareContentRequest  → "share_content_request"
    //   ShareIncomingData    → "share_incoming_data"
    /// Richiesta di consenso al DESTINATARIO: "vuoi ricevere `doc_name` da `from_label`?"
    /// `from_label` è il `label_base` NUDO del mittente (la macchina, non un umano/AI —
    /// Share non distingue chi risponde). Mostrata dalla finestra AI Chat (slice futura).
    ShareRequest { share_id: String, from_label: String, doc_name: String, size_bytes: u64 },
    /// Esito per il MITTENTE — una per ogni destinatario (rilevante da Slice 3 in poi,
    /// quando "Share with all" ne produce N indipendenti).
    ShareResult { share_id: String, target_label: String, doc_name: String, outcome: ShareOutcome },
    /// (Slice 2a, MITTENTE) L'orchestrator ha bisogno del contenuto di un documento già
    /// offerto e accettato dal destinatario — la UI lo legge con `archive_open` e
    /// risponde con `ClientMsg::ShareContent` o `ClientMsg::ShareContentFailed`.
    ShareContentRequest { share_id: String, rel_path: String },
    /// (Slice 2a, DESTINATARIO) Un documento accettato è pronto per essere scritto su
    /// disco — la UI lo persiste con `archive_save` e risponde con
    /// `ClientMsg::ShareWritten`. Emesso subito se la UI è già connessa quando arriva
    /// `ChatMsg::ShareData`, oppure in replay su `SetServerTx` per ogni trasferimento
    /// ancora in coda (stesso principio di `AiChatRoster`/`AiChatHistory`/`ShareRequest`).
    ShareIncomingData { share_id: String, from_label: String, title: String, content: String },

    // ── AI Chat — ammissione alla stanza (additive) ──────────────────────────
    //
    // Cinque varianti server→client per il modello di ammissione a voto (sostituisce
    // il vecchio `AiChatJoinRequest`/consenso pairwise sopra — quella variante resta
    // per retro-compatibilità additiva ma non è più il percorso primario). Vedi
    // Docs/superpowers/plans/2026-07-03-aichat-admission.md.
    //
    // Wire names (snake_case via `rename_all`):
    //   AiChatJoinPrompt        → "ai_chat_join_prompt"
    //   AiChatAdmissionRequest  → "ai_chat_admission_request"
    //   AiChatPending           → "ai_chat_pending"
    //   AiChatAdmitted          → "ai_chat_admitted"
    //   AiChatRejected          → "ai_chat_rejected"
    //   AiChatAdmissionResolved → "ai_chat_admission_resolved"
    /// Gate 1, al NUOVO ARRIVATO: "vuoi entrare in chat con [present]?". Emesso quando
    /// questa macchina si connette al server eletto, PRIMA di mandare qualunque richiesta
    /// di ammissione — l'umano deve acconsentire esplicitamente (risposta:
    /// `ClientMsg::AiChatJoinDecision`).
    AiChatJoinPrompt { present: Vec<String> },
    /// Gate 2, a un PRESENTE già ammesso: "ammetti `candidate`? sì/no". Il server lo
    /// manda a ogni presente quando un nuovo arrivato chiede di entrare (risposta:
    /// `ClientMsg::AiChatAdmissionVote`).
    AiChatAdmissionRequest { candidate: String },
    /// Stato di attesa del NUOVO ARRIVATO dopo aver accettato il gate 1: il voto dei
    /// presenti è in corso. La UI deve bloccare l'input finché non arriva `AiChatAdmitted`
    /// o `AiChatRejected`.
    AiChatPending { present: Vec<String> },
    /// Il nuovo arrivato è stato ammesso: il voto dei presenti non ha avuto veti (sì
    /// espliciti o silenzio-oltre-timeout). La UI sblocca l'input.
    AiChatAdmitted {},
    /// Il nuovo arrivato è stato rifiutato: almeno un presente ha votato no (veto).
    /// `retry_after_secs` è il cooldown minimo prima che la UI riabiliti il pulsante
    /// "chiedi di entrare" (`ClientMsg::AiChatRequestAdmission`).
    AiChatRejected { retry_after_secs: u32 },
    /// (FIX #7 — review 2026-07-03) A un PRESENTE: il voto di ammissione per
    /// `candidate` si è CONCLUSO (ammesso, rifiutato, o candidato sparito). La UI
    /// deve togliere il banner del gate 2 per quel candidato — senza questo, dopo
    /// che il turno è deciso altrove (veto di un altro presente, timeout, o uscita
    /// del candidato) il banner "ammetti X?" restava appeso e il click non aveva
    /// più effetto (il server scarta il voto come stale). Gemello "di chiusura" di
    /// `AiChatAdmissionRequest`.
    AiChatAdmissionResolved { candidate: String },

    // ── Gate di conferma locale per tool sensibili (additiva) ───────────────
    //
    // Wire name: ToolConfirmRequest → "tool_confirm_request"
    /// Chiede all'utente di confermare l'esecuzione di uno o più tool AI marcati
    /// "sensibili" nello stesso turno (stesso principio di batch-in-un-turno già
    /// usato dal gate Telegram — `commands` può contenere più righe). `id` è
    /// opaco (128 bit random), NON derivato dal comando: la risposta
    /// (`ClientMsg::ToolConfirmResponse`) lo echoa per correlazione.
    ToolConfirmRequest { id: String, commands: String },

    // ── save_routine — finestra di anteprima dedicata (Fase 2, additiva) ────
    //
    // Docs/superpowers/specs/2026-08-05-save-routine-design.md §5. Estende il
    // gate di conferma locale con una SECONDA superficie: a differenza di
    // `ToolConfirmRequest` (banner Sì/No col comando come stringa libera),
    // questo messaggio porta i campi STRUTTURATI di una proposta di
    // `save_routine` — la UI apre una finestra dedicata invece del banner nel
    // cursore. La risposta è la STESSA `ClientMsg::ToolConfirmResponse` (id
    // opaco, `accept`), riusata as-is: i pulsanti Salva/Annulla della
    // finestra mandano lo stesso messaggio che oggi manda il cursore.
    //
    // Wire name: RoutineSavePreview → "routine_save_preview"
    /// Chiede conferma per il salvataggio di una routine (`save_routine`),
    /// mostrando il corpo completo dello script in una finestra dedicata.
    /// `id` è opaco (128 bit random), come `ToolConfirmRequest::id` — la
    /// risposta (`ClientMsg::ToolConfirmResponse`) lo echoa per correlazione.
    /// `replace`: `Some(<nome vecchio>)` se questo salvataggio sostituisce/
    /// aggiorna una routine esistente (rinominata o meno) — `None` per una
    /// routine nuova e distinta.
    RoutineSavePreview {
        id: String,
        name: String,
        description: String,
        tags: Vec<String>,
        category: String,
        script: String,
        replace: Option<String>,
    },

    // ── Market data source test (additiva) ────────────────────────────
    // Wire name: MarketDataSourceTestResult → "market_data_source_test_result"
    // Design: Docs/superpowers/specs/2026-08-14-financial-markets-ibkr-data-source/task-10.
    /// Risposta a `ClientMsg::TestMarketDataSource`.
    /// `id` correla la richiesta (stesso pattern di `Command`/`Done`).
    /// `ok`: true se la connessione alla fonte dati è riuscita, false altrimenti.
    /// `message`: feedback umano-leggibile (es. "Connesso a IBKR", "Timeout", ecc).
    MarketDataSourceTestResult { id: String, ok: bool, message: String },

    // ── Canale shell (2.0, spec §3.2/§4.1) — additivi ────────────────────
    // Wire names: exec_in_shell, open_output_window, output_window_content,
    // open_ui_local, ui_ping, activity_indicator.

    /// Esegui `command` nella shell dell'utente (solo verso `role: Shell`,
    /// SEMPRE dopo un `ToolConfirmRequest` accettato — spec §8). `capture`
    /// (§4.5): `true` = output catturato e restituito in `ExecResult.output`;
    /// `false` = console attaccata (programmi interattivi), output vuoto.
    ExecInShell { turn_id: String, exec_id: String, command: String, capture: bool },

    /// Apre su `ui` la finestra Markdown di output di un comando slash
    /// originato dalla shell, con un segnaposto ("in corso…").
    OpenOutputWindow { window_id: String, title: String },

    /// Sostituisce il contenuto della finestra `window_id` (a `Done`/`Error`).
    OutputWindowContent { window_id: String, markdown: String },

    /// Chiede a `ui` di aprire (o portare in primo piano, D15) una finestra
    /// locale: `"config"`, `"library"`, `"aichat"`, oppure l'id di un canale
    /// esterno (`"nmap"`, `"financial-markets"`, `"python-ping"`).
    OpenUiLocal { name: String },

    /// Richiesta di vita a `ui.exe` (built-in `/ping`); risposta `UiPong`.
    UiPing { id: String },

    /// Segnalino di stato per la finestra terminale della sessione
    /// (`kind`: `"ai_busy"`); consumato dal piano 3, già emesso qui.
    ActivityIndicator { session_id: String, kind: String, on: bool },

    /// Segnala che il turno "proprietario" della finestra `window_id` (una
    /// finestra `show_markdown`, vedi `ai_adapter.rs`) è concluso (successo,
    /// errore o cancellazione) — la finestra lo usa per sapere se può chiudersi
    /// liberamente o deve ancora mostrare il gate di conferma (Parte B) e il
    /// badge "ricerca in corso" (Parte C). Mandato SOLO se quel turno ha
    /// aperto una finestra `show_markdown` — un turno che non l'ha mai
    /// chiamato non genera questo messaggio (nessuna finestra a cui riferirsi).
    MarkdownWindowTurnEnded { window_id: String },
}

impl ServerMsg {
    /// Superficie di destinazione durante un turno originato da una shell
    /// (vedi [`Surface`]). Il `match` è esaustivo **senza wildcard** di
    /// proposito: chi aggiunge una variante deve decidere dove va, e il
    /// compilatore glielo ricorda.
    pub fn surface(&self) -> Surface {
        use ServerMsg::*;
        match self {
            // Avanzamento del turno e risposte puntuali: alla connessione origine.
            ServerInfo { .. } | Chunk { .. } | Done { .. } | Error { .. } | Pong { .. }
            | Cwd { .. } | Heartbeat { .. } | ToolConfirmRequest { .. }
            | RoutineSavePreview { .. } | MarketDataSourceTestResult { .. }
            | ExecInShell { .. } => Surface::Origin,
            // Finestre e relay: al sink `ui` della macchina.
            OpenWindow { .. } | OpenScreenerPicker { .. } | SearchOpen { .. }
            | SearchHit { .. } | SearchDone { .. } | OpenPluginWindow { .. }
            | UpdatePluginWindow { .. } | ClosePluginWindow { .. }
            | AiChatMessage { .. } | AiChatRoster { .. } | AiChatJoinRequest { .. }
            | AiChatHistory { .. } | AiChatSelf { .. } | AiChatPeerLost { .. }
            | AiChatReachablePeers { .. } | NotesSnapshot { .. } | NoteUpserted { .. }
            | ShareRequest { .. } | ShareResult { .. } | ShareContentRequest { .. }
            | ShareIncomingData { .. } | AiChatJoinPrompt { .. }
            | AiChatAdmissionRequest { .. } | AiChatPending { .. } | AiChatAdmitted { .. }
            | AiChatRejected { .. } | AiChatAdmissionResolved { .. }
            | OpenOutputWindow { .. } | OutputWindowContent { .. } | OpenUiLocal { .. }
            | UiPing { .. } | ActivityIndicator { .. } | MarkdownWindowTurnEnded { .. } => Surface::Ui,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── ClientMsg round-trips ──────────────────────────────────────────────

    #[test]
    fn client_hello_roundtrip() {
        let msg = ClientMsg::Hello {
            token: "secret".to_string(),
            channel: None,
            role: Role::Ui,
            session_id: None,
            cwd: None,
            version: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: ClientMsg = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn client_command_roundtrip() {
        let msg = ClientMsg::Command {
            id: "abc-123".to_string(),
            input: "dir".to_string(),
            input_mode: InputMode::Keyboard,
            command_type: CommandKind::Auto,
            cwd: None,
            web_search: false,
            lang: String::new(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: ClientMsg = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn client_command_with_cwd_roundtrip() {
        let msg = ClientMsg::Command {
            id: "xyz".to_string(),
            input: "ls".to_string(),
            input_mode: InputMode::Voice,
            command_type: CommandKind::Nl,
            cwd: Some("/home/user".to_string()),
            web_search: false,
            lang: String::new(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: ClientMsg = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn client_ping_roundtrip() {
        let msg = ClientMsg::Ping { ts: 1_700_000_000 };
        let json = serde_json::to_string(&msg).unwrap();
        let back: ClientMsg = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    // ── ServerMsg round-trips ─────────────────────────────────────────────

    #[test]
    fn server_info_roundtrip() {
        let msg = ServerMsg::ServerInfo {
            version: "0.1.0".to_string(),
            ai_provider: "claude".to_string(),
            capabilities: vec!["os_command".to_string()],
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: ServerMsg = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn server_chunk_roundtrip() {
        let msg = ServerMsg::Chunk {
            id: "cmd-1".to_string(),
            content: "hello\n".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: ServerMsg = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn server_open_screener_picker_roundtrip() {
        let msg = ServerMsg::OpenScreenerPicker {
            items: vec![
                ScreenerListItem {
                    id: "consumer-usage".to_string(),
                    title: "Uso Consumer".to_string(),
                    description: "Screening 'potenziale inespresso' — aziende consumer USA.".to_string(),
                },
            ],
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"open_screener_picker\""));
        let back: ServerMsg = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn server_open_screener_picker_empty_items_roundtrip() {
        let msg = ServerMsg::OpenScreenerPicker { items: vec![] };
        let json = serde_json::to_string(&msg).unwrap();
        let back: ServerMsg = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn server_done_roundtrip() {
        let msg = ServerMsg::Done {
            id: "cmd-1".to_string(),
            exit_code: Some(0),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: ServerMsg = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn server_done_no_exit_code_roundtrip() {
        let msg = ServerMsg::Done {
            id: "cmd-1".to_string(),
            exit_code: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: ServerMsg = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn server_error_roundtrip() {
        let msg = ServerMsg::Error {
            id: "cmd-2".to_string(),
            code: ErrCode::AiError,
            message: "timeout".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: ServerMsg = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn server_pong_roundtrip() {
        let msg = ServerMsg::Pong { ts: 42 };
        let json = serde_json::to_string(&msg).unwrap();
        let back: ServerMsg = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    // ── Wire format assertions (JSON shape on the wire) ───────────────────

    #[test]
    fn command_wire_format() {
        // The discriminator must be "type":"command" (snake_case of variant name).
        // Fields must be snake_case. cwd:None must appear as "cwd":null (not omitted).
        let msg = ClientMsg::Command {
            id: "i1".to_string(),
            input: "ls".to_string(),
            input_mode: InputMode::Keyboard,
            command_type: CommandKind::Auto,
            cwd: None,
            web_search: false,
            lang: String::new(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(
            json.contains(r#""type":"command""#),
            "missing type:command in: {json}"
        );
        assert!(
            json.contains(r#""input_mode":"keyboard""#),
            "missing input_mode:keyboard in: {json}"
        );
        assert!(
            json.contains(r#""command_type":"auto""#),
            "missing command_type:auto in: {json}"
        );
        assert!(
            json.contains(r#""cwd":null"#),
            "missing cwd:null in: {json}"
        );
    }

    #[test]
    fn hello_wire_format() {
        let msg = ClientMsg::Hello {
            token: "tok".to_string(),
            channel: None,
            role: Role::Ui,
            session_id: None,
            cwd: None,
            version: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(
            json.contains(r#""type":"hello""#),
            "missing type:hello in: {json}"
        );
        assert!(
            json.contains(r#""token":"tok""#),
            "missing token in: {json}"
        );
    }

    #[test]
    fn chunk_wire_format() {
        let msg = ServerMsg::Chunk {
            id: "x".to_string(),
            content: "y".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(
            json.contains(r#""type":"chunk""#),
            "missing type:chunk in: {json}"
        );
    }

    #[test]
    fn done_wire_format() {
        let msg = ServerMsg::Done {
            id: "x".to_string(),
            exit_code: Some(1),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(
            json.contains(r#""type":"done""#),
            "missing type:done in: {json}"
        );
    }

    #[test]
    fn server_info_wire_format() {
        let msg = ServerMsg::ServerInfo {
            version: "0.1.0".to_string(),
            ai_provider: "claude".to_string(),
            capabilities: vec![],
        };
        let json = serde_json::to_string(&msg).unwrap();
        // PascalCase variant ServerInfo → snake_case tag "server_info"
        assert!(
            json.contains(r#""type":"server_info""#),
            "missing type:server_info in: {json}"
        );
    }

    // ── Hand-written JSON → ClientMsg (blinda il contratto verso client non-Rust) ──

    #[test]
    fn deserialize_handwritten_command_json() {
        // Simulates what `wscat` would send.
        let raw = r#"{
            "type": "command",
            "id": "abc",
            "input": "dir",
            "input_mode": "keyboard",
            "command_type": "auto",
            "cwd": null
        }"#;
        let msg: ClientMsg = serde_json::from_str(raw).unwrap();
        assert_eq!(
            msg,
            ClientMsg::Command {
                id: "abc".to_string(),
                input: "dir".to_string(),
                input_mode: InputMode::Keyboard,
                command_type: CommandKind::Auto,
                cwd: None,
                web_search: false,
                lang: String::new(),
            }
        );
    }

    #[test]
    fn deserialize_handwritten_hello_json() {
        let raw = r#"{"type":"hello","token":"my-secret-token"}"#;
        let msg: ClientMsg = serde_json::from_str(raw).unwrap();
        assert_eq!(
            msg,
            ClientMsg::Hello {
                token: "my-secret-token".to_string(),
                channel: None,
                role: Role::Ui,
                session_id: None,
                cwd: None,
                version: None,
            }
        );
    }

    #[test]
    fn hello_channel_defaults_none_when_absent() {
        // Un client che non invia `channel` (tutto il codice esistente) → None,
        // stesso principio già usato per `web_search` su Command.
        let raw = r#"{"type":"hello","token":"my-secret-token"}"#;
        let msg: ClientMsg = serde_json::from_str(raw).unwrap();
        match msg {
            ClientMsg::Hello { channel, .. } => assert_eq!(channel, None),
            other => panic!("atteso Hello, trovato {other:?}"),
        }
    }

    #[test]
    fn hello_channel_roundtrips_when_present() {
        let msg = ClientMsg::Hello {
            token: "secret".to_string(),
            channel: Some("nmap".to_string()),
            role: Role::Ui,
            session_id: None,
            cwd: None,
            version: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""channel":"nmap""#), "missing channel in: {json}");
        let back: ClientMsg = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn deserialize_handwritten_ping_json() {
        let raw = r#"{"type":"ping","ts":1234567890}"#;
        let msg: ClientMsg = serde_json::from_str(raw).unwrap();
        assert_eq!(msg, ClientMsg::Ping { ts: 1_234_567_890 });
    }

    #[test]
    fn deserialize_handwritten_command_with_cwd() {
        let raw = r#"{
            "type": "command",
            "id": "x1",
            "input": "ls -la",
            "input_mode": "voice",
            "command_type": "nl",
            "cwd": "/tmp"
        }"#;
        let msg: ClientMsg = serde_json::from_str(raw).unwrap();
        assert_eq!(
            msg,
            ClientMsg::Command {
                id: "x1".to_string(),
                input: "ls -la".to_string(),
                input_mode: InputMode::Voice,
                command_type: CommandKind::Nl,
                cwd: Some("/tmp".to_string()),
                web_search: false,
                lang: String::new(),
            }
        );
    }

    #[test]
    fn deserialize_handwritten_command_os_kind() {
        let raw = r#"{
            "type": "command",
            "id": "os1",
            "input": "$ pwd",
            "input_mode": "keyboard",
            "command_type": "os",
            "cwd": null
        }"#;
        let msg: ClientMsg = serde_json::from_str(raw).unwrap();
        assert_eq!(
            msg,
            ClientMsg::Command {
                id: "os1".to_string(),
                input: "$ pwd".to_string(),
                input_mode: InputMode::Keyboard,
                command_type: CommandKind::Os,
                cwd: None,
                web_search: false,
                lang: String::new(),
            }
        );
    }

    // ── TDD RED (written before OpenWindow/WindowKind exist) ─────────────────
    // These tests will fail to compile until ServerMsg::OpenWindow and
    // WindowKind are added below — the compiler error IS the RED phase.
    // Precedent: same E0004/undefined-variant pattern as orchestrator 0.3.0.

    #[test]
    fn open_window_markdown_roundtrip() {
        // Round-trip serialisation: OpenWindow survives serde.
        let msg = ServerMsg::OpenWindow {
            title: "Test Title".to_string(),
            kind: WindowKind::Markdown,
            content: "# Hello\n- item".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: ServerMsg = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn open_window_wire_format_type_discriminator() {
        // The discriminator MUST be "type":"open_window" (snake_case of OpenWindow).
        let msg = ServerMsg::OpenWindow {
            title: "Lare — Output".to_string(),
            kind: WindowKind::Markdown,
            content: "some content".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(
            json.contains(r#""type":"open_window""#),
            r#"missing "type":"open_window" in: {json}"#
        );
    }

    #[test]
    fn open_window_wire_format_kind_markdown() {
        // The kind field MUST be "kind":"markdown" (snake_case of Markdown).
        let msg = ServerMsg::OpenWindow {
            title: "T".to_string(),
            kind: WindowKind::Markdown,
            content: "c".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(
            json.contains(r#""kind":"markdown""#),
            r#"missing "kind":"markdown" in: {json}"#
        );
    }

    #[test]
    fn open_window_wire_format_full_shape() {
        // Hand-verify the complete wire shape expected by the UI:
        // {"type":"open_window","title":"...","kind":"markdown","content":"..."}
        let msg = ServerMsg::OpenWindow {
            title: "My Title".to_string(),
            kind: WindowKind::Markdown,
            content: "# Heading".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(
            json.contains(r#""type":"open_window""#),
            "missing type: {json}"
        );
        assert!(
            json.contains(r#""title":"My Title""#),
            "missing title: {json}"
        );
        assert!(
            json.contains(r#""kind":"markdown""#),
            "missing kind: {json}"
        );
        assert!(
            json.contains("\"content\":\"# Heading\""),
            "missing content: {json}"
        );
    }

    #[test]
    fn deserialize_handwritten_open_window_json() {
        // Simulates what the orchestrator sends over WS.
        // The UI's JSON.parse will produce an object with these exact keys.
        let raw = "{\"type\":\"open_window\",\"title\":\"Ciao Mondo\",\"kind\":\"markdown\",\"content\":\"# Ciao\\n- a\\n- b\"}";
        let msg: ServerMsg = serde_json::from_str(raw).unwrap();
        assert_eq!(
            msg,
            ServerMsg::OpenWindow {
                title: "Ciao Mondo".to_string(),
                kind: WindowKind::Markdown,
                content: "# Ciao\n- a\n- b".to_string(),
            }
        );
    }

    // ── TDD: WindowKind::Help (0.4.0) ────────────────────────────────────────
    // These tests were written BEFORE adding the `Help` variant to `WindowKind`.
    // Pre-addition they produce:
    //   error[E0599]: no variant or associated item named `Help` found for enum
    //   `WindowKind` in the current scope
    // That compile error IS the RED phase.  Adding `Help` is the GREEN phase.

    #[test]
    fn window_kind_help_roundtrip() {
        // WindowKind::Help survives a serde round-trip.
        let kind = WindowKind::Help;
        let json = serde_json::to_string(&kind).unwrap();
        let back: WindowKind = serde_json::from_str(&json).unwrap();
        assert_eq!(kind, back);
    }

    #[test]
    fn window_kind_help_wire_is_help_string() {
        // The wire value MUST be `"help"` (snake_case of `Help`).
        let kind = WindowKind::Help;
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(
            json, r#""help""#,
            "WindowKind::Help must serialise to \"help\""
        );
    }

    #[test]
    fn open_window_help_wire_format() {
        // Full ServerMsg::OpenWindow with kind:Help → the JSON contains "kind":"help".
        let msg = ServerMsg::OpenWindow {
            title: "Lare \u{2014} Comandi".to_string(),
            kind: WindowKind::Help,
            content: "# Help\n- /help".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(
            json.contains(r#""kind":"help""#),
            r#"missing "kind":"help" in: {json}"#
        );
        assert!(
            json.contains(r#""type":"open_window""#),
            r#"missing "type":"open_window" in: {json}"#
        );
    }

    // ── TDD RED: web_search field on Command ─────────────────────────────────
    // These tests are written BEFORE the web_search field is added to
    // ClientMsg::Command. They will fail at compile time (missing field) or at
    // runtime until the field and its serde(default) attribute are in place.

    #[test]
    fn command_web_search_defaults_false_when_absent() {
        // Un client che non invia `web_search` → false (serde default).
        let raw = r#"{"type":"command","id":"a","input":"x","input_mode":"keyboard","command_type":"auto","cwd":null}"#;
        let msg: ClientMsg = serde_json::from_str(raw).unwrap();
        match msg {
            ClientMsg::Command { web_search, .. } => assert!(!web_search),
            other => panic!("atteso Command, trovato {other:?}"),
        }
    }

    #[test]
    fn command_web_search_true_roundtrip() {
        let msg = ClientMsg::Command {
            id: "a".to_string(),
            input: "cerca".to_string(),
            input_mode: InputMode::Keyboard,
            command_type: CommandKind::Nl,
            cwd: None,
            web_search: true,
            lang: String::new(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(
            json.contains(r#""web_search":true"#),
            "manca web_search:true in {json}"
        );
        let back: ClientMsg = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    // ── lang field on Command (i18n Parte 3) ──────────────────────────────────

    #[test]
    fn command_lang_defaults_empty_when_absent() {
        // Un client che non invia `lang` → stringa vuota "" (serde default).
        let raw = r#"{"type":"command","id":"a","input":"x","input_mode":"keyboard","command_type":"auto","cwd":null}"#;
        let msg: ClientMsg = serde_json::from_str(raw).unwrap();
        match msg {
            ClientMsg::Command { lang, .. } => assert_eq!(lang, ""),
            other => panic!("atteso Command, trovato {other:?}"),
        }
    }

    #[test]
    fn command_lang_roundtrip() {
        let msg = ClientMsg::Command {
            id: "a".to_string(),
            input: "cerca".to_string(),
            input_mode: InputMode::Keyboard,
            command_type: CommandKind::Nl,
            cwd: None,
            web_search: false,
            lang: "en".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(
            json.contains(r#""lang":"en""#),
            "manca lang:\"en\" in {json}"
        );
        let back: ClientMsg = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    // ── TDD RED: Search messages (SearchOpen/Hit/Done, CancelSearch, WindowKind::Search) ──

    #[test]
    fn search_open_roundtrip_and_wire() {
        let m = ServerMsg::SearchOpen {
            id: "s1".into(),
            title: "/find *.pdf".into(),
        };
        let j = serde_json::to_string(&m).unwrap();
        assert!(j.contains(r#""type":"search_open""#), "{j}");
        assert_eq!(serde_json::from_str::<ServerMsg>(&j).unwrap(), m);
    }

    #[test]
    fn search_hit_source_is_snake_case() {
        let m = ServerMsg::SearchHit {
            id: "s1".into(),
            path: "P:\\a.pdf".into(),
            source: SearchSource::Cwd,
            line: None,
            snippet: None,
        };
        let j = serde_json::to_string(&m).unwrap();
        assert!(
            j.contains(r#""type":"search_hit""#) && j.contains(r#""source":"cwd""#),
            "{j}"
        );
        assert_eq!(serde_json::from_str::<ServerMsg>(&j).unwrap(), m);
    }

    // ── TDD RED: SearchHit guadagna line/snippet opzionali (ricerca contenuto) ──
    //
    // `line`/`snippet` valorizzano un hit di ricerca CONTENUTO (`/find in:"..."`,
    // task successivi di questo piano). Un hit solo-nome (ricerca corrente) li
    // lascia `None`. Sono decorati con `skip_serializing_if = "Option::is_none"`
    // così, quando assenti, NON compaiono affatto nel JSON — un client vecchio
    // che non conosce questi campi vede lo stesso identico wire di prima
    // (compatibilità additiva, Contratto A).

    #[test]
    fn search_hit_with_line_and_snippet_roundtrip() {
        let m = ServerMsg::SearchHit {
            id: "s1".into(),
            path: "P:\\a.txt".into(),
            source: SearchSource::Cwd,
            line: Some(42),
            snippet: Some("il totale è 1234".into()),
        };
        let j = serde_json::to_string(&m).unwrap();
        assert!(
            j.contains(r#""line":42"#) && j.contains(r#""snippet":"il totale è 1234""#),
            "{j}"
        );
        assert_eq!(serde_json::from_str::<ServerMsg>(&j).unwrap(), m);
    }

    #[test]
    fn search_hit_without_line_omits_it_from_wire() {
        let m = ServerMsg::SearchHit {
            id: "s1".into(),
            path: "P:\\a.pdf".into(),
            source: SearchSource::Cwd,
            line: None,
            snippet: None,
        };
        let j = serde_json::to_string(&m).unwrap();
        assert!(
            !j.contains("line") && !j.contains("snippet"),
            "wire invariato per hit solo-nome: {j}"
        );
        assert_eq!(serde_json::from_str::<ServerMsg>(&j).unwrap(), m);
    }

    #[test]
    fn search_done_roundtrip() {
        let m = ServerMsg::SearchDone {
            id: "s1".into(),
            count: 23,
            truncated: false,
        };
        let j = serde_json::to_string(&m).unwrap();
        assert!(j.contains(r#""type":"search_done""#), "{j}");
        assert_eq!(serde_json::from_str::<ServerMsg>(&j).unwrap(), m);
    }

    #[test]
    fn cancel_search_roundtrip() {
        let m = ClientMsg::CancelSearch { id: "s1".into() };
        let j = serde_json::to_string(&m).unwrap();
        assert!(j.contains(r#""type":"cancel_search""#), "{j}");
        assert_eq!(serde_json::from_str::<ClientMsg>(&j).unwrap(), m);
    }

    #[test]
    fn window_kind_search_wire() {
        assert_eq!(
            serde_json::to_string(&WindowKind::Search).unwrap(),
            r#""search""#
        );
    }

    // ── TDD RED: ServerMsg::Cwd (0.6.0) ─────────────────────────────────────
    // Written BEFORE adding the `Cwd` variant to `ServerMsg`.
    // Pre-addition this produces:
    //   error[E0599]: no variant or associated item named `Cwd` found for enum `ServerMsg`
    // That compile error IS the RED phase. Adding `Cwd { path: String }` is the GREEN phase.

    #[test]
    fn server_cwd_roundtrip() {
        // Round-trip: ServerMsg::Cwd survives serde serialisation/deserialisation.
        let msg = ServerMsg::Cwd {
            path: "C:\\Users\\x".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: ServerMsg = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn server_cwd_wire_format() {
        // The discriminator MUST be "type":"cwd" (snake_case of `Cwd`).
        // The payload MUST contain "path":"C:\\Users\\x".
        let msg = ServerMsg::Cwd {
            path: "C:\\Users\\x".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(
            json.contains(r#""type":"cwd""#),
            r#"missing "type":"cwd" in: {json}"#
        );
        assert!(
            json.contains(r#""path":"C:\\Users\\x""#),
            r#"missing "path" in: {json}"#
        );
    }

    #[test]
    fn server_cwd_deserialize_handwritten() {
        // Simulates what the orchestrator sends over WS.
        let raw = r#"{"type":"cwd","path":"C:\\Users\\x"}"#;
        let msg: ServerMsg = serde_json::from_str(raw).unwrap();
        assert_eq!(
            msg,
            ServerMsg::Cwd {
                path: "C:\\Users\\x".to_string(),
            }
        );
    }

    // ── TDD RED: ServerMsg::Heartbeat (0.15.1) ──────────────────────────────
    // Written BEFORE adding the `Heartbeat` variant to `ServerMsg`.
    // Pre-addition this produces:
    //   error[E0599]: no variant or associated item named `Heartbeat` found for enum `ServerMsg`
    // That compile error IS the RED phase. Adding `Heartbeat { id: String }` is the GREEN phase.

    #[test]
    fn server_heartbeat_roundtrip() {
        // Round-trip: ServerMsg::Heartbeat survives serde serialisation/deserialisation.
        let msg = ServerMsg::Heartbeat { id: "cmd-1".to_string() };
        let json = serde_json::to_string(&msg).unwrap();
        let back: ServerMsg = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn server_heartbeat_wire_format() {
        // The discriminator MUST be "type":"heartbeat" (snake_case of `Heartbeat`).
        let msg = ServerMsg::Heartbeat { id: "cmd-1".to_string() };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(
            json.contains(r#""type":"heartbeat""#),
            r#"missing "type":"heartbeat" in: {json}"#
        );
        assert!(
            json.contains(r#""id":"cmd-1""#),
            r#"missing "id" in: {json}"#
        );
    }

    // ── TDD RED: PauseSearch / ResumeSearch (0.7.0) ──────────────────────────
    // Written BEFORE adding the variants to `ClientMsg`.
    // Pre-addition these produce:
    //   error[E0599]: no variant or associated item named `PauseSearch` found
    // That compile error IS the RED phase. Adding the variants is the GREEN phase.

    #[test]
    fn pause_search_roundtrip() {
        // Round-trip: ClientMsg::PauseSearch survives serde serialisation/deserialisation.
        let msg = ClientMsg::PauseSearch {
            id: "s1".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: ClientMsg = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn pause_search_wire_format() {
        // The discriminator MUST be "type":"pause_search" (snake_case of `PauseSearch`).
        // The payload MUST contain "id":"s1".
        let msg = ClientMsg::PauseSearch {
            id: "s1".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(
            json.contains(r#""type":"pause_search""#),
            r#"missing "type":"pause_search" in: {json}"#
        );
        assert!(
            json.contains(r#""id":"s1""#),
            r#"missing "id":"s1" in: {json}"#
        );
    }

    #[test]
    fn pause_search_deserialize_handwritten() {
        // Simulates what the UI sends over WS to pause a running search.
        let raw = r#"{"type":"pause_search","id":"s1"}"#;
        let msg: ClientMsg = serde_json::from_str(raw).unwrap();
        assert_eq!(
            msg,
            ClientMsg::PauseSearch {
                id: "s1".to_string()
            }
        );
    }

    #[test]
    fn resume_search_roundtrip() {
        // Round-trip: ClientMsg::ResumeSearch survives serde serialisation/deserialisation.
        let msg = ClientMsg::ResumeSearch {
            id: "s1".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: ClientMsg = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn resume_search_wire_format() {
        // The discriminator MUST be "type":"resume_search" (snake_case of `ResumeSearch`).
        // The payload MUST contain "id":"s1".
        let msg = ClientMsg::ResumeSearch {
            id: "s1".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(
            json.contains(r#""type":"resume_search""#),
            r#"missing "type":"resume_search" in: {json}"#
        );
        assert!(
            json.contains(r#""id":"s1""#),
            r#"missing "id":"s1" in: {json}"#
        );
    }

    #[test]
    fn resume_search_deserialize_handwritten() {
        // Simulates what the UI sends over WS to resume a paused search.
        let raw = r#"{"type":"resume_search","id":"s1"}"#;
        let msg: ClientMsg = serde_json::from_str(raw).unwrap();
        assert_eq!(
            msg,
            ClientMsg::ResumeSearch {
                id: "s1".to_string()
            }
        );
    }

    // ── TDD RED: plugin window variants (0.8.0) ──────────────────────────────
    // Written BEFORE adding the plugin window variants to ServerMsg / ClientMsg.
    // Pre-addition these produce:
    //   error[E0599]: no variant named `OpenPluginWindow` found for enum `ServerMsg`
    //   error[E0599]: no variant named `PluginUiEvent` found for enum `ClientMsg`
    // Those compile errors ARE the RED phase.  Adding the 3 ServerMsg + 2 ClientMsg
    // variants is the GREEN phase.

    #[test]
    fn open_plugin_window_roundtrip_and_tag() {
        // Round-trip: ServerMsg::OpenPluginWindow survives serde.
        // The discriminator MUST be "open_plugin_window" (snake_case of OpenPluginWindow).
        let m = ServerMsg::OpenPluginWindow {
            window_id: 3,
            title: "Counter".into(),
            html: "<i>x</i>".into(),
            // Nessuna dimensione dichiarata: il caso comune (calc/ping/counter).
            width: None,
            height: None,
        };
        let s = serde_json::to_string(&m).unwrap();
        assert!(
            s.contains("\"type\":\"open_plugin_window\""),
            "missing type:open_plugin_window in: {s}"
        );
        // Retro-compatibilità: senza dimensione dichiarata i campi width/height
        // NON compaiono nel wire (grazie a skip_serializing_if). Un client vecchio
        // vede esattamente lo stesso JSON di prima → default 480×360 lato UI.
        assert!(!s.contains("width"),  "width non deve comparire quando None: {s}");
        assert!(!s.contains("height"), "height non deve comparire quando None: {s}");
        assert_eq!(serde_json::from_str::<ServerMsg>(&s).unwrap(), m);
    }

    #[test]
    fn open_plugin_window_carries_optional_size() {
        // Con una dimensione dichiarata dal plugin, width/height DEVONO comparire
        // nel wire (numeri f64) e sopravvivere al round-trip.
        let m = ServerMsg::OpenPluginWindow {
            window_id: 5,
            title: "Lare Commander".into(),
            html: "<div/>".into(),
            width: Some(960.0),
            height: Some(620.0),
        };
        let s = serde_json::to_string(&m).unwrap();
        assert!(s.contains("\"width\":960.0"),  "width mancante nel wire: {s}");
        assert!(s.contains("\"height\":620.0"), "height mancante nel wire: {s}");
        assert_eq!(serde_json::from_str::<ServerMsg>(&s).unwrap(), m);
    }

    #[test]
    fn plugin_ui_event_roundtrip() {
        // Round-trip: ClientMsg::PluginUiEvent survives serde.
        // The discriminator MUST be "plugin_ui_event" (snake_case of PluginUiEvent).
        // `value: None` must deserialise back to None.
        let m = ClientMsg::PluginUiEvent {
            window_id: 3,
            element_id: "inc".into(),
            value: None,
        };
        let s = serde_json::to_string(&m).unwrap();
        assert!(
            s.contains("\"type\":\"plugin_ui_event\""),
            "missing type:plugin_ui_event in: {s}"
        );
        assert_eq!(serde_json::from_str::<ClientMsg>(&s).unwrap(), m);
    }

    // ── TDD RED: AI Chat variants (0.9.0) ────────────────────────────────────
    // Questi test sono scritti PRIMA di aggiungere le varianti AI Chat a ServerMsg
    // e ClientMsg. Prima dell'aggiunta producono:
    //   error[E0599]: no variant named `AiChatMessage` found for enum `ServerMsg`
    //   error[E0599]: no variant named `AiChatSend` found for enum `ClientMsg`
    //   ...
    // Quell'errore di compilazione è la fase RED del ciclo TDD.
    // La fase GREEN è l'aggiunta delle 6 varianti agli enum.

    #[test]
    fn aichat_message_roundtrip_and_tag() {
        let m = ServerMsg::AiChatMessage {
            from_label: "skimble-human".into(),
            text: "ciao".into(),
            display_name: None,
            is_ai: false,
        };
        let j = serde_json::to_string(&m).unwrap();
        assert!(j.contains(r#""type":"ai_chat_message""#), "{j}");
        assert_eq!(serde_json::from_str::<ServerMsg>(&j).unwrap(), m);
    }

    // ── TDD RED: display_name/is_ai additivi su ChatLine/AiChatMessage ───────
    // Scritti PRIMA di aggiungere i due campi nuovi. Prima dell'aggiunta producono
    // errori di compilazione (campi inesistenti) — quell'errore È la fase RED.
    // Vedi Docs/superpowers/plans/2026-08-13-aichat-display-names.md, Task 2.

    #[test]
    fn chat_line_defaults_display_name_none_and_is_ai_false_when_absent() {
        // Storico scritto/ricevuto da un peer che non conosce ancora i campi nuovi.
        let json = r#"{"from_label":"skimble-human","text":"ciao"}"#;
        let line: ChatLine = serde_json::from_str(json).unwrap();
        assert_eq!(line.display_name, None);
        assert!(!line.is_ai);
    }

    #[test]
    fn ai_chat_message_round_trips_display_name_and_is_ai() {
        let m = ServerMsg::AiChatMessage {
            from_label: "skimble-ai".into(),
            text: "ciao".into(),
            display_name: Some("Aria".into()),
            is_ai: true,
        };
        let json = serde_json::to_string(&m).unwrap();
        let back: ServerMsg = serde_json::from_str(&json).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn aichat_roster_and_join_request_roundtrip() {
        let r = ServerMsg::AiChatRoster { participants: vec!["a".into(), "b".into()] };
        let jr = ServerMsg::AiChatJoinRequest { peer_label: "macavity".into() };
        for m in [r, jr] {
            assert_eq!(serde_json::from_str::<ServerMsg>(&serde_json::to_string(&m).unwrap()).unwrap(), m);
        }
    }

    #[test]
    fn aichat_history_roundtrip_and_tag() {
        let m = ServerMsg::AiChatHistory {
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
        assert!(j.contains(r#""type":"ai_chat_history""#), "{j}");
        assert!(j.contains(r#""from_label":"skimble-human""#), "{j}");
        assert_eq!(serde_json::from_str::<ServerMsg>(&j).unwrap(), m);
    }

    #[test]
    fn aichat_self_roundtrip_and_tag() {
        let m = ServerMsg::AiChatSelf { label: "skimble-human".into() };
        let j = serde_json::to_string(&m).unwrap();
        assert!(j.contains(r#""type":"ai_chat_self""#), "{j}");
        assert!(j.contains(r#""label":"skimble-human""#), "{j}");
        assert_eq!(serde_json::from_str::<ServerMsg>(&j).unwrap(), m);
    }

    #[test]
    fn aichat_peer_lost_roundtrip_and_tag() {
        let m = ServerMsg::AiChatPeerLost { label: "quaxo-human".into() };
        let j = serde_json::to_string(&m).unwrap();
        assert!(j.contains(r#""type":"ai_chat_peer_lost""#), "{j}");
        assert!(j.contains(r#""label":"quaxo-human""#), "{j}");
        assert_eq!(serde_json::from_str::<ServerMsg>(&j).unwrap(), m);
    }

    #[test]
    fn aichat_client_messages_roundtrip_and_tags() {
        let send = ClientMsg::AiChatSend { text: "ciao".into() };
        let consent = ClientMsg::AiChatJoinConsent { peer_label: "macavity".into(), accept: true };
        let closed = ClientMsg::AiChatClosed {};
        let js = serde_json::to_string(&send).unwrap();
        assert!(js.contains(r#""type":"ai_chat_send""#), "{js}");
        for m in [send, consent, closed] {
            assert_eq!(serde_json::from_str::<ClientMsg>(&serde_json::to_string(&m).unwrap()).unwrap(), m);
        }
    }

    /// `AiChatReachablePeers`: nuovo segnale per Library "Condividi" (spec
    /// 2026-07-28) — round-trip serde + tag wire corretto.
    #[test]
    fn ai_chat_reachable_peers_roundtrips_and_tag() {
        let m = ServerMsg::AiChatReachablePeers {
            labels: vec!["skimble-human".into(), "rumpleteazer-human".into()],
        };
        let j = serde_json::to_string(&m).unwrap();
        assert!(j.contains(r#""type":"ai_chat_reachable_peers""#), "tag errato: {j}");
        assert_eq!(serde_json::from_str::<ServerMsg>(&j).unwrap(), m);
    }

    // ── TDD RED: ammissione alla stanza AI Chat (gate 1/2, pending, admitted/rejected) ──
    // Scritti PRIMA di aggiungere le nuove varianti a `ServerMsg`/`ClientMsg`. Prima
    // dell'aggiunta producono errori di compilazione `E0599: no variant named ... found`
    // — quell'errore di compilazione È la fase RED.
    // Vedi Docs/superpowers/plans/2026-07-03-aichat-admission.md, Task 2.
    #[test]
    fn admission_ws_msgs_roundtrip_and_tags() {
        // ServerMsg (orch→UI): gate 1/2 + stati pending/admitted/rejected.
        let server_cases = [
            (
                ServerMsg::AiChatJoinPrompt { present: vec!["a".into(), "c".into()] },
                r#""type":"ai_chat_join_prompt""#,
            ),
            (
                ServerMsg::AiChatAdmissionRequest { candidate: "cand-human".into() },
                r#""type":"ai_chat_admission_request""#,
            ),
            (
                ServerMsg::AiChatPending { present: vec!["a".into()] },
                r#""type":"ai_chat_pending""#,
            ),
            (ServerMsg::AiChatAdmitted {}, r#""type":"ai_chat_admitted""#),
            (
                ServerMsg::AiChatRejected { retry_after_secs: 30 },
                r#""type":"ai_chat_rejected""#,
            ),
            // FIX #7 (review 2026-07-03): notifica ai presenti che un voto si è
            // concluso, così la loro UI toglie il banner del gate 2.
            (
                ServerMsg::AiChatAdmissionResolved { candidate: "cand-human".into() },
                r#""type":"ai_chat_admission_resolved""#,
            ),
        ];
        for (m, expected_tag) in server_cases {
            let j = serde_json::to_string(&m).unwrap();
            assert!(j.contains(expected_tag), "tag errato: {j}");
            assert_eq!(serde_json::from_str::<ServerMsg>(&j).unwrap(), m);
        }

        // ClientMsg (UI→orch): risposte al gate 1/2 + re-request.
        let client_cases = [
            (
                ClientMsg::AiChatJoinDecision { accept: true },
                r#""type":"ai_chat_join_decision""#,
            ),
            (
                ClientMsg::AiChatAdmissionVote { candidate: "cand-human".into(), accept: false },
                r#""type":"ai_chat_admission_vote""#,
            ),
            (
                ClientMsg::AiChatRequestAdmission {},
                r#""type":"ai_chat_request_admission""#,
            ),
        ];
        for (m, expected_tag) in client_cases {
            let j = serde_json::to_string(&m).unwrap();
            assert!(j.contains(expected_tag), "tag errato: {j}");
            assert_eq!(serde_json::from_str::<ClientMsg>(&j).unwrap(), m);
        }
    }

    // ── TDD RED: Library "Share with" — consenso + esito (Slice 1a) ─────────────
    // Scritto PRIMA di aggiungere le nuove varianti/enum. Prima dell'aggiunta produce
    // `E0599: no variant named ... found` — quell'errore di compilazione È la fase RED.
    // Vedi Docs/superpowers/plans/2026-07-05-library-share-slice1-consent-net.md, Task 1.
    #[test]
    fn share_ws_msgs_roundtrip_and_tags() {
        let client_cases = [
            (
                ClientMsg::ShareDocument {
                    rel_path: "notes.md".into(),
                    doc_name: "notes.md".into(),
                    size_bytes: 1234,
                    target: ShareTarget::One { label_base: "skimble".into() },
                },
                r#""type":"share_document""#,
            ),
            (
                ClientMsg::ShareDocument {
                    rel_path: "notes.md".into(),
                    doc_name: "notes.md".into(),
                    size_bytes: 1234,
                    target: ShareTarget::All,
                },
                r#""type":"share_document""#,
            ),
            (
                ClientMsg::ShareConsent { share_id: "192.168.1.10:1".into(), accept: true },
                r#""type":"share_consent""#,
            ),
            (
                ClientMsg::ShareContent {
                    share_id: "192.168.1.10:1".into(),
                    title: "Note".into(),
                    content: "corpo del documento".into(),
                },
                r#""type":"share_content""#,
            ),
            (
                ClientMsg::ShareContentFailed {
                    share_id: "192.168.1.10:1".into(),
                    reason: "documento non più disponibile".into(),
                },
                r#""type":"share_content_failed""#,
            ),
            (
                ClientMsg::ShareWritten { share_id: "192.168.1.10:1".into() },
                r#""type":"share_written""#,
            ),
        ];
        for (m, expected_tag) in client_cases {
            let j = serde_json::to_string(&m).unwrap();
            assert!(j.contains(expected_tag), "tag errato: {j}");
            assert_eq!(serde_json::from_str::<ClientMsg>(&j).unwrap(), m);
        }

        let server_cases = [
            (
                ServerMsg::ShareRequest {
                    share_id: "192.168.1.10:1".into(),
                    from_label: "rumpleteazer".into(),
                    doc_name: "notes.md".into(),
                    size_bytes: 1234,
                },
                r#""type":"share_request""#,
            ),
            (
                ServerMsg::ShareResult {
                    share_id: "192.168.1.10:1".into(),
                    target_label: "skimble".into(),
                    doc_name: "notes.md".into(),
                    outcome: ShareOutcome::Accepted,
                },
                r#""type":"share_result""#,
            ),
            (
                ServerMsg::ShareResult {
                    share_id: "192.168.1.10:1".into(),
                    target_label: "skimble".into(),
                    doc_name: "notes.md".into(),
                    outcome: ShareOutcome::Failed { reason: "scaduto".into() },
                },
                r#""type":"share_result""#,
            ),
            (
                ServerMsg::ShareContentRequest {
                    share_id: "192.168.1.10:1".into(),
                    rel_path: "notes.md".into(),
                },
                r#""type":"share_content_request""#,
            ),
            (
                ServerMsg::ShareIncomingData {
                    share_id: "192.168.1.10:1".into(),
                    from_label: "rumpleteazer".into(),
                    title: "Note".into(),
                    content: "corpo del documento".into(),
                },
                r#""type":"share_incoming_data""#,
            ),
        ];
        for (m, expected_tag) in server_cases {
            let j = serde_json::to_string(&m).unwrap();
            assert!(j.contains(expected_tag), "tag errato: {j}");
            assert_eq!(serde_json::from_str::<ServerMsg>(&j).unwrap(), m);
        }
    }

    // ── TDD RED: Gate di conferma locale per tool sensibili (ToolConfirmRequest/ToolConfirmResponse) ──
    // Scritti PRIMA di aggiungere le nuove varianti a `ServerMsg`/`ClientMsg`. Prima
    // dell'aggiunta producono errori di compilazione `E0599: no variant named ... found`
    // — quell'errore di compilazione È la fase RED.
    // Vedi Docs/superpowers/specs/2026-07-15-local-tool-confirm-gate-design.md.

    #[test]
    fn tool_confirm_request_round_trips() {
        let msg = ServerMsg::ToolConfirmRequest {
            id: "abc123".to_string(),
            commands: "$ nmap_quick_scan 10.0.0.5".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"tool_confirm_request\""), "wire type errato: {json}");
        let back: ServerMsg = serde_json::from_str(&json).unwrap();
        assert_eq!(back, msg);
    }

    #[test]
    fn tool_confirm_response_round_trips() {
        let msg = ClientMsg::ToolConfirmResponse {
            id: "abc123".to_string(),
            accept: true,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"tool_confirm_response\""), "wire type errato: {json}");
        let back: ClientMsg = serde_json::from_str(&json).unwrap();
        assert_eq!(back, msg);
    }

    // ── TDD RED: save_routine preview window (Fase 2) — Docs/superpowers/specs/
    // 2026-08-05-save-routine-design.md §5 ──────────────────────────────────

    #[test]
    fn routine_save_preview_roundtrip() {
        let msg = ServerMsg::RoutineSavePreview {
            id: "abc123".to_string(),
            name: "list-big-files".to_string(),
            description: "Elenca i file grandi".to_string(),
            tags: vec!["files".to_string(), "disk".to_string()],
            category: "files".to_string(),
            script: "Get-ChildItem -Path . -Recurse".to_string(),
            replace: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: ServerMsg = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn routine_save_preview_wire_discriminator_is_snake_case() {
        let msg = ServerMsg::RoutineSavePreview {
            id: "x".to_string(),
            name: "n".to_string(),
            description: "d".to_string(),
            tags: vec![],
            category: "c".to_string(),
            script: "s".to_string(),
            replace: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"routine_save_preview""#), "got: {json}");
    }

    #[test]
    fn routine_save_preview_replace_field_present_when_some() {
        let msg = ServerMsg::RoutineSavePreview {
            id: "x".to_string(),
            name: "n".to_string(),
            description: "d".to_string(),
            tags: vec![],
            category: "c".to_string(),
            script: "s".to_string(),
            replace: Some("old-name".to_string()),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""replace":"old-name""#), "got: {json}");
    }

    // ── TDD RED: Library "Blocco note" (NoteView, ClientMsg/ServerMsg variants) ──
    // Scritti PRIMA di aggiungere le nuove varianti. Prima dell'aggiunta producono
    // errori di compilazione `E0599: no variant named ... found` — quell'errore È la fase RED.
    // Vedi Docs/superpowers/specs/2026-07-29-library-notes-design.md §7.

    #[test]
    fn client_msg_note_variants_roundtrip_and_tag() {
        let msgs_and_tags: Vec<(ClientMsg, &str)> = vec![
            (ClientMsg::NoteCreate { title: "Idea".into(), text: "corpo".into() }, "note_create"),
            (ClientMsg::NoteEdit { id: "192.168.1.10:0".into(), text: "nuovo corpo".into() }, "note_edit"),
            (ClientMsg::NoteEditTitle { id: "192.168.1.10:0".into(), title: "Titolo nuovo".into() }, "note_edit_title"),
            (ClientMsg::NoteDelete { id: "192.168.1.10:0".into() }, "note_delete"),
        ];
        for (m, tag) in msgs_and_tags {
            let j = serde_json::to_string(&m).unwrap();
            assert!(j.contains(&format!(r#""type":"{tag}""#)), "tag errato per {tag}: {j}");
            assert_eq!(serde_json::from_str::<ClientMsg>(&j).unwrap(), m);
        }
    }

    #[test]
    fn server_msg_notes_snapshot_and_upserted_roundtrip_and_tag() {
        let view = NoteView {
            id: "192.168.1.10:0".into(),
            title: "Idea".into(),
            body: "corpo".into(),
            my_segment_text: "corpo".into(),
            created_by: "skimble".into(),
            created_at_ms: 0,
            deleted: false,
        };
        let snapshot = ServerMsg::NotesSnapshot { notes: vec![view.clone()] };
        let j = serde_json::to_string(&snapshot).unwrap();
        assert!(j.contains(r#""type":"notes_snapshot""#), "tag errato: {j}");
        assert_eq!(serde_json::from_str::<ServerMsg>(&j).unwrap(), snapshot);

        let upserted = ServerMsg::NoteUpserted { note: view };
        let j2 = serde_json::to_string(&upserted).unwrap();
        assert!(j2.contains(r#""type":"note_upserted""#), "tag errato: {j2}");
        assert_eq!(serde_json::from_str::<ServerMsg>(&j2).unwrap(), upserted);
    }

    // ── TDD RED: Market data source test (TestMarketDataSource/MarketDataSourceTestResult) ──
    // Scritti PRIMA di aggiungere le nuove varianti a `ClientMsg`/`ServerMsg`. Prima
    // dell'aggiunta producono errori di compilazione `E0599: no variant named ... found`
    // — quell'errore di compilazione È la fase RED.
    // Vedi Docs/superpowers/specs/2026-08-14-financial-markets-ibkr-data-source/task-10.

    #[test]
    fn test_market_data_source_client_msg_round_trips() {
        let msg = ClientMsg::TestMarketDataSource { id: "abc123".to_string() };
        let json = serde_json::to_string(&msg).unwrap();
        let back: ClientMsg = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn market_data_source_test_result_server_msg_round_trips() {
        let msg = ServerMsg::MarketDataSourceTestResult {
            id: "abc123".to_string(),
            ok: true,
            message: "connesso".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: ServerMsg = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    // ── Piano 2a: ruolo della connessione e messaggi host↔orchestratore (spec §4.1) ──

    /// Un client v1 manda `Hello` senza `role`: deve restare valido e valere `ui`.
    #[test]
    fn hello_without_role_defaults_to_ui() {
        let msg: ClientMsg = serde_json::from_str(r#"{"type":"hello","token":"t"}"#).unwrap();
        match msg {
            ClientMsg::Hello { token, channel, role, session_id, cwd, version } => {
                assert_eq!(token, "t");
                assert_eq!(channel, None);
                assert_eq!(role, Role::Ui);
                assert_eq!(session_id, None);
                assert_eq!(cwd, None);
                assert_eq!(version, None);
            }
            other => panic!("atteso Hello, ricevuto {other:?}"),
        }
    }

    #[test]
    fn hello_shell_round_trips_all_new_fields() {
        let msg = ClientMsg::Hello {
            token: "t".into(),
            channel: None,
            role: Role::Shell,
            session_id: Some("a1b2".into()),
            cwd: Some("C:\\Users\\x".into()),
            version: Some("2.0.0".into()),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""role":"shell""#), "wire: {json}");
        let back: ClientMsg = serde_json::from_str(&json).unwrap();
        assert_eq!(back, msg);
    }

    #[test]
    fn exec_in_shell_and_exec_result_wire_names() {
        let s = ServerMsg::ExecInShell {
            turn_id: "t1".into(), exec_id: "e1".into(), command: "dir".into(), capture: true,
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.starts_with(r#"{"type":"exec_in_shell""#), "wire: {json}");
        let c: ClientMsg = serde_json::from_str(
            r#"{"type":"exec_result","turn_id":"t1","exec_id":"e1","exit_code":0,"output":"x","cwd":"C:\\"}"#,
        ).unwrap();
        assert_eq!(c, ClientMsg::ExecResult {
            turn_id: "t1".into(), exec_id: "e1".into(), exit_code: 0, output: "x".into(), cwd: "C:\\".into(),
        });
    }

    #[test]
    fn output_window_ui_local_ping_indicator_wire_names() {
        let cases = vec![
            (ServerMsg::OpenOutputWindow { window_id: "w".into(), title: "T".into() }, "open_output_window"),
            (ServerMsg::OutputWindowContent { window_id: "w".into(), markdown: "# x".into() }, "output_window_content"),
            (ServerMsg::OpenUiLocal { name: "config".into() }, "open_ui_local"),
            (ServerMsg::UiPing { id: "p".into() }, "ui_ping"),
            (ServerMsg::ActivityIndicator { session_id: "s".into(), kind: "ai_busy".into(), on: true }, "activity_indicator"),
            (ServerMsg::MarkdownWindowTurnEnded { window_id: "w".into() }, "markdown_window_turn_ended"),
        ];
        for (msg, wire) in cases {
            let json = serde_json::to_string(&msg).unwrap();
            assert!(json.contains(&format!(r#""type":"{wire}""#)), "wire: {json}");
            let back: ServerMsg = serde_json::from_str(&json).unwrap();
            assert_eq!(back, msg);
        }
        let pong: ClientMsg = serde_json::from_str(r#"{"type":"ui_pong","id":"p","version":"2.1.0"}"#).unwrap();
        assert_eq!(pong, ClientMsg::UiPong { id: "p".into(), version: "2.1.0".into() });
    }

    /// `surface()`: ciò che torna alla connessione che ha emesso il comando
    /// (`Origin`) contro ciò che apre/aggiorna finestre su `ui.exe` (`Ui`).
    /// Il `match` in `surface()` è esaustivo senza wildcard: una variante
    /// nuova senza riga nella tabella NON compila — questo test copre solo un
    /// campione rappresentativo per superficie.
    #[test]
    fn surface_origin_for_turn_messages_and_ui_for_window_messages() {
        let id = || "c1".to_string();
        let origin = vec![
            ServerMsg::Chunk { id: id(), content: "x".into() },
            ServerMsg::Done { id: id(), exit_code: None },
            ServerMsg::Error { id: id(), code: ErrCode::RoutingError, message: "m".into() },
            ServerMsg::Pong { ts: 1 },
            ServerMsg::Cwd { path: "C:\\".into() },
            ServerMsg::Heartbeat { id: id() },
            ServerMsg::ToolConfirmRequest { id: id(), commands: "$ dir".into() },
            ServerMsg::ExecInShell { turn_id: id(), exec_id: "e".into(), command: "dir".into(), capture: true },
            ServerMsg::MarketDataSourceTestResult { id: id(), ok: true, message: String::new() },
        ];
        for m in origin {
            assert_eq!(m.surface(), Surface::Origin, "{m:?}");
        }
        let ui = vec![
            ServerMsg::OpenWindow { title: "t".into(), kind: WindowKind::Markdown, content: "c".into() },
            ServerMsg::SearchOpen { id: id(), title: "t".into() },
            ServerMsg::OpenPluginWindow { window_id: 1, title: "t".into(), html: "<p/>".into(), width: None, height: None },
            ServerMsg::AiChatRoster { participants: vec![] },
            ServerMsg::NotesSnapshot { notes: vec![] },
            ServerMsg::OpenOutputWindow { window_id: "w".into(), title: "T".into() },
            ServerMsg::OutputWindowContent { window_id: "w".into(), markdown: "m".into() },
            ServerMsg::OpenUiLocal { name: "library".into() },
            ServerMsg::UiPing { id: id() },
            ServerMsg::ActivityIndicator { session_id: "s".into(), kind: "ai_busy".into(), on: false },
            ServerMsg::MarkdownWindowTurnEnded { window_id: id() },
        ];
        for m in ui {
            assert_eq!(m.surface(), Surface::Ui, "{m:?}");
        }
    }
}
