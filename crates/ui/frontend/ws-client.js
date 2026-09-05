// ws-client.js — WebSocket client for the Lare Terminal orchestrator.
//
// Responsibility (SRP): owns ONLY the WebSocket connection lifecycle and the
// wire protocol.  It does NOT touch the DOM.
//
// Wire format (from crates/protocol/src/lib.rs — serde snake_case):
//   ClientMsg::Hello       → {"type":"hello","token":"...","channel":null|"..."}
//   ClientMsg::Command     → {"type":"command","id":"...","input":"...",
//                             "input_mode":"keyboard","command_type":"auto",
//                             "cwd":null,"web_search":false}
//   ServerMsg::ServerInfo  → {"type":"server_info","version":"...","ai_provider":"...","capabilities":[...]}
//   ServerMsg::Chunk       → {"type":"chunk","id":"...","content":"..."}
//   ServerMsg::Done        → {"type":"done","id":"...","exit_code":...}
//   ServerMsg::Error       → {"type":"error","id":"...","code":"...","message":"..."}
//   ServerMsg::Cwd         → {"type":"cwd","path":"..."}
//   ServerMsg::Heartbeat   → {"type":"heartbeat","id":"..."}
//
// All server messages are forwarded to the onMessage callback unconditionally
// (the switch in app.js handles dispatch by type).

const WS_URL = "ws://127.0.0.1:7331";
const RETRY_INITIAL_MS = 1000;
const RETRY_MAX_MS = 8000;

/**
 * LareWsClient
 *
 * Usage:
 *   const client = new LareWsClient({ token, onStatus, onMessage });
 *   client.connect();
 *   client.sendCommand("dir", "cmd-uuid");
 *   client.disconnect();
 *
 * onStatus(status: "connecting"|"connected"|"disconnected"|"error")
 * onMessage(msg: object)  — parsed ServerMsg
 */
export class LareWsClient {
  /**
   * @param {{ token: string, onStatus: function, onMessage: function, channel?: string }} opts
   */
  constructor({ token, onStatus, onMessage, channel }) {
    /** @type {string} */
    this._token = token;
    /** @type {function} */
    this._onStatus = onStatus || (() => {});
    /** @type {function} */
    this._onMessage = onMessage || (() => {});
    /** @type {string|undefined} */
    this._channel = channel;

    /** @type {WebSocket|null} */
    this._ws = null;
    /** @type {boolean} */
    this._intentionalClose = false;
    /** @type {number} */
    this._retryMs = RETRY_INITIAL_MS;
    /** @type {number|null} */
    this._retryTimer = null;
  }

  // ── Public API ─────────────────────────────────────────────────────────

  /** Open the connection (with automatic retry on failure). */
  connect() {
    this._intentionalClose = false;
    this._openSocket();
  }

  /** Close the connection permanently (no retry). */
  disconnect() {
    this._intentionalClose = true;
    if (this._retryTimer !== null) {
      clearTimeout(this._retryTimer);
      this._retryTimer = null;
    }
    if (this._ws) {
      this._ws.close();
      this._ws = null;
    }
  }

  /**
   * Send a Command message.
   *
   * @param {string}  input     - The command text typed by the user.
   * @param {string}  id        - A UUID that correlates Chunk/Done/Error responses.
   * @param {boolean} [webSearch=false] - If true, the orchestrator is authorised to
   *                              run server-side web search tools for this turn.
   * @returns {boolean}    - false if the socket is not open.
   */
  sendCommand(input, id, webSearch = false) {
    if (!this._isOpen()) return false;
    /** @type {import('./ws-client.js').ClientCommandMsg} */
    const msg = {
      type: "command",
      id,
      input,
      input_mode: "keyboard",
      command_type: "auto",
      cwd: null,            // Known Slice A limit: each command starts a fresh shell.
      web_search: !!webSearch,
    };
    this._send(msg);
    return true;
  }

  /**
   * Richiede il test di raggiungibilità della fonte dati mercato
   * ATTUALMENTE SALVATA (ClientMsg::TestMarketDataSource) — non accetta
   * una sorgente a scelta, vedi task-12-brief.md.
   *
   * @param {string} id - UUID che correla ServerMsg::MarketDataSourceTestResult.
   * @returns {boolean} false se il socket non è aperto.
   */
  sendTestMarketDataSource(id) {
    if (!this._isOpen()) return false;
    this._send({ type: "test_market_data_source", id });
    return true;
  }

  /**
   * Annulla una ricerca in corso (ClientMsg::CancelSearch).
   *
   * @param {string} id - Il `sid` della ricerca (da SearchOpen).
   * @returns {boolean} false se il socket non è aperto.
   */
  cancelSearch(id) {
    if (!this._isOpen()) return false;
    this._send({ type: "cancel_search", id });
    return true;
  }

  /**
   * Mette in pausa una ricerca in corso (ClientMsg::PauseSearch).
   *
   * @param {string} id - Il `sid` della ricerca (da SearchOpen).
   * @returns {boolean} false se il socket non è aperto.
   */
  pauseSearch(id) {
    if (!this._isOpen()) return false;
    this._send({ type: "pause_search", id });
    return true;
  }

  /**
   * Riprende una ricerca in pausa (ClientMsg::ResumeSearch).
   *
   * @param {string} id - Il `sid` della ricerca (da SearchOpen).
   * @returns {boolean} false se il socket non è aperto.
   */
  resumeSearch(id) {
    if (!this._isOpen()) return false;
    this._send({ type: "resume_search", id });
    return true;
  }

  /**
   * Cancella un comando AI/OS in corso (ClientMsg::CancelCommand).
   *
   * @param {string} id - L'UUID del comando (stesso usato in sendCommand).
   * @returns {boolean} false se il socket non è aperto.
   */
  cancelCommand(id) {
    if (!this._isOpen()) return false;
    this._send({ type: "cancel_command", id });
    return true;
  }

  /**
   * Notify the orchestrator that the user interacted with a plugin window
   * element (ClientMsg::PluginUiEvent).
   *
   * The orchestrator routes this to the plugin handler which dispatches it
   * as a `UiEvent` into the plugin's state machine.
   *
   * @param {number}      window_id  - Plugin window id (u64 on the Rust side;
   *                                   must be a JS number, not a string).
   * @param {string}      element_id - Value of the clicked element's data-evt.
   * @param {string|null} value      - Input/select value, or null for buttons.
   * @returns {boolean} false if the socket is not open.
   */
  sendPluginUiEvent(window_id, element_id, value) {
    if (!this._isOpen()) return false;
    this._send({ type: "plugin_ui_event", window_id, element_id, value });
    return true;
  }

  /** AI Chat (Slice 1a-ui-B): l'umano invia un messaggio nella stanza. */
  sendAiChatSend(text) {
    if (!this._isOpen()) return false;
    this._send({ type: "ai_chat_send", text });
    return true;
  }

  /** AI Chat: risposta al consenso d'ingresso per un peer. */
  sendAiChatJoinConsent(peer_label, accept) {
    if (!this._isOpen()) return false;
    this._send({ type: "ai_chat_join_consent", peer_label, accept });
    return true;
  }

  /** AI Chat: la finestra-chat è stata chiusa (umano esce). */
  sendAiChatClosed() {
    if (!this._isOpen()) return false;
    this._send({ type: "ai_chat_closed" });
    return true;
  }

  /**
   * AI Chat — ammissione (risposta al gate 1): l'umano nuovo arrivato ha deciso
   * se provare a entrare nella stanza (ClientMsg::AiChatJoinDecision).
   *
   * @param {boolean} accept - true = prova a entrare (segue `RequestAdmission`
   *                           lato orchestratore); false = annulla il tentativo.
   * @returns {boolean} false se il socket non è aperto.
   */
  sendAiChatJoinDecision(accept) {
    if (!this._isOpen()) return false;
    this._send({ type: "ai_chat_join_decision", accept: !!accept });
    return true;
  }

  /**
   * AI Chat — ammissione (risposta al gate 2): un presente vota se ammettere
   * `candidate` (ClientMsg::AiChatAdmissionVote). Un solo "no" tra i presenti
   * è un veto immediato; il server tratta i non-votanti-entro-timeout come sì.
   *
   * @param {string}  candidate - Etichetta del candidato (es. "macavity-human").
   * @param {boolean} accept    - true = ammetti, false = rifiuta (veto).
   * @returns {boolean} false se il socket non è aperto.
   */
  sendAiChatAdmissionVote(candidate, accept) {
    if (!this._isOpen()) return false;
    this._send({ type: "ai_chat_admission_vote", candidate, accept: !!accept });
    return true;
  }

  /**
   * AI Chat — ammissione: pulsante "Chiedi di entrare" dopo un rifiuto
   * (ClientMsg::AiChatRequestAdmission). La UI applica già un cooldown lato
   * client (vedi admission.mjs/rerequestState) prima di abilitare il pulsante.
   *
   * @returns {boolean} false se il socket non è aperto.
   */
  sendAiChatRequestAdmission() {
    if (!this._isOpen()) return false;
    this._send({ type: "ai_chat_request_admission" });
    return true;
  }

  /**
   * AI Chat: la finestra-chat è stata (ri)aperta.
   *
   * Ri-registra il sink WS nell'orchestrator affinché i messaggi in arrivo
   * vengano di nuovo inoltrati alla UI, e ri-emette l'ultimo roster noto
   * (i "presenti" vengono ripristinati senza dover aspettare un nuovo messaggio).
   *
   * Va inviato ogni volta che `aichat:ready` emette (nuovo webview Tauri aperto).
   * @returns {boolean} false se il socket non è aperto.
   */
  sendAiChatOpen() {
    if (!this._isOpen()) return false;
    this._send({ type: "ai_chat_open" });
    return true;
  }

  /**
   * Library "Share with": richiede la condivisione di un documento con una
   * macchina (ClientMsg::ShareDocument). Solo target "one" in questa slice —
   * "Share with all" non ha ancora una UI (Slice 3).
   *
   * @param {string} rel_path    - Path relativo del documento nella Library.
   * @param {string} doc_name    - Nome visualizzato del documento.
   * @param {number} size_bytes  - Dimensione in byte (calcolata dal chiamante).
   * @param {string} target_label - label_base nudo della macchina destinataria.
   * @returns {boolean} false se il socket non è aperto.
   */
  sendShareDocument(rel_path, doc_name, size_bytes, target_label) {
    if (!this._isOpen()) return false;
    this._send({
      type: "share_document",
      rel_path,
      doc_name,
      size_bytes,
      target: { one: { label_base: target_label } },
    });
    return true;
  }

  /**
   * Risposta al banner di consenso Share (ClientMsg::ShareConsent).
   *
   * @param {string}  share_id
   * @param {boolean} accept
   * @returns {boolean} false se il socket non è aperto.
   */
  sendShareConsent(share_id, accept) {
    if (!this._isOpen()) return false;
    this._send({ type: "share_consent", share_id, accept: !!accept });
    return true;
  }

  /**
   * Risposta al banner di conferma per un tool AI sensibile (gate locale
   * per-tool, ClientMsg::ToolConfirmResponse).
   *
   * @param {string}  id
   * @param {boolean} accept
   * @returns {boolean} false se il socket non è aperto.
   */
  sendToolConfirmResponse(id, accept) {
    if (!this._isOpen()) return false;
    this._send({ type: "tool_confirm_response", id, accept: !!accept });
    return true;
  }

  /**
   * Blocco note (Task 16): crea una nuova nota.
   *
   * @param {string} title - Titolo della nota.
   * @param {string} text  - Testo della nota.
   * @returns {boolean} false se il socket non è aperto.
   */
  sendNoteCreate(title, text) {
    if (!this._isOpen()) return false;
    this._send({ type: "note_create", title, text });
    return true;
  }

  /**
   * Blocco note (Task 16): modifica il testo di una nota esistente.
   *
   * @param {string} id   - UUID della nota.
   * @param {string} text - Nuovo testo della nota.
   * @returns {boolean} false se il socket non è aperto.
   */
  sendNoteEdit(id, text) {
    if (!this._isOpen()) return false;
    this._send({ type: "note_edit", id, text });
    return true;
  }

  /**
   * Blocco note (Task 16): modifica il titolo di una nota esistente.
   *
   * @param {string} id    - UUID della nota.
   * @param {string} title - Nuovo titolo della nota.
   * @returns {boolean} false se il socket non è aperto.
   */
  sendNoteEditTitle(id, title) {
    if (!this._isOpen()) return false;
    this._send({ type: "note_edit_title", id, title });
    return true;
  }

  /**
   * Blocco note (Task 16): cancella una nota.
   *
   * @param {string} id - UUID della nota.
   * @returns {boolean} false se il socket non è aperto.
   */
  sendNoteDelete(id) {
    if (!this._isOpen()) return false;
    this._send({ type: "note_delete", id });
    return true;
  }

  /**
   * Slice 2a: la UI ha letto con successo il documento richiesto da
   * ServerMsg::ShareContentRequest (ClientMsg::ShareContent).
   *
   * @param {string} share_id
   * @param {string} title
   * @param {string} content
   * @returns {boolean} false se il socket non è aperto.
   */
  sendShareContent(share_id, title, content) {
    if (!this._isOpen()) return false;
    this._send({ type: "share_content", share_id, title, content });
    return true;
  }

  /**
   * Slice 2a: la UI non è riuscita a leggere il documento richiesto
   * (ClientMsg::ShareContentFailed). `reason` è sempre la frase fissa
   * "documento non più disponibile" — vedi app.js per il chiamante.
   *
   * @param {string} share_id
   * @param {string} reason
   * @returns {boolean} false se il socket non è aperto.
   */
  sendShareContentFailed(share_id, reason) {
    if (!this._isOpen()) return false;
    this._send({ type: "share_content_failed", share_id, reason });
    return true;
  }

  /**
   * Slice 2a: la UI (destinatario) conferma di aver scritto su disco un
   * documento ricevuto (ClientMsg::ShareWritten).
   *
   * @param {string} share_id
   * @returns {boolean} false se il socket non è aperto.
   */
  sendShareWritten(share_id) {
    if (!this._isOpen()) return false;
    this._send({ type: "share_written", share_id });
    return true;
  }

  /**
   * Notify the orchestrator that a plugin window was closed by the user
   * (ClientMsg::PluginWindowClosed).
   *
   * The plugin can use this to release resources, update state, or log the event.
   *
   * @param {number} window_id - Plugin window id (u64 on the Rust side).
   * @returns {boolean} false if the socket is not open.
   */
  sendPluginWindowClosed(window_id) {
    if (!this._isOpen()) return false;
    this._send({ type: "plugin_window_closed", window_id });
    return true;
  }

  /** Whether the socket is currently open and past the handshake. */
  isConnected() {
    return this._isOpen();
  }

  // ── Private helpers ────────────────────────────────────────────────────

  _openSocket() {
    this._onStatus("connecting");
    try {
      const ws = new WebSocket(WS_URL);
      this._ws = ws;

      ws.addEventListener("open", () => {
        // Handshake: Hello MUST be the first message. `channel` is omitted
        // (undefined) for the main cursor connection — JSON.stringify drops
        // undefined-valued keys, so the wire shape is byte-identical to today.
        this._send({ type: "hello", token: this._token, channel: this._channel });
        this._retryMs = RETRY_INITIAL_MS; // reset backoff on success
      });

      ws.addEventListener("message", (ev) => {
        let msg;
        try {
          msg = JSON.parse(ev.data);
        } catch {
          console.error("[ws-client] JSON parse error:", ev.data);
          return;
        }

        // ServerInfo signals that the handshake was accepted.
        if (msg.type === "server_info") {
          this._onStatus("connected");
        }

        this._onMessage(msg);
      });

      ws.addEventListener("close", () => {
        this._ws = null;
        if (!this._intentionalClose) {
          this._onStatus("disconnected");
          this._scheduleRetry();
        }
      });

      ws.addEventListener("error", () => {
        // The "close" event always follows "error"; retry logic lives there.
        this._onStatus("error");
      });
    } catch (err) {
      console.error("[ws-client] WebSocket constructor threw:", err);
      this._onStatus("error");
      this._scheduleRetry();
    }
  }

  _scheduleRetry() {
    if (this._intentionalClose) return;
    console.info(`[ws-client] Reconnecting in ${this._retryMs}ms…`);
    this._retryTimer = setTimeout(() => {
      this._retryTimer = null;
      this._openSocket();
    }, this._retryMs);
    // Exponential back-off capped at RETRY_MAX_MS.
    this._retryMs = Math.min(this._retryMs * 2, RETRY_MAX_MS);
  }

  _isOpen() {
    return this._ws !== null && this._ws.readyState === WebSocket.OPEN;
  }

  /** Serialize and send a ClientMsg object. */
  _send(obj) {
    if (!this._isOpen()) return;
    try {
      this._ws.send(JSON.stringify(obj));
    } catch (err) {
      console.error("[ws-client] send error:", err);
    }
  }
}
