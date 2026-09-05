// host.js — Pagina host di Lare Terminal 2.0.
//
// L'overlay F2 (il "cursore" v1: editor a riga singola, output scrollabile,
// spinner di attività, pulcino idle, diagnosi automatica) è sparito — vedi
// spec §5. `ui.exe` resta comunque necessario per tre compiti che nessuna
// finestra "senza WS propria" può fare da sola:
//
//   1. Tiene l'UNICA connessione WebSocket "di default" (Hello SENZA
//      `channel`) verso l'orchestrator — quella su cui arrivano AiChat*,
//      Search*, le finestre Markdown/plugin e Share.
//   2. È il lanciatore di ogni finestra (`invokeCmd("open_*_window")` vive
//      tutto qui).
//   3. Fa da relay: riceve AiChat*/Search*/Share*/OpenPluginWindow dal wire e
//      li ri-emette come eventi Tauri globali verso la finestra AI Chat, la
//      finestra di ricerca (`/find`), Library e le finestre plugin.
//
// Questo file è quindi una PAGINA NASCOSTA (host.html, finestra Tauri
// invisibile, vedi tauri.conf.json) — nessun DOM utente, nessuna tastiera.
// Estratto da app.js (v1): stesse funzioni, stesso comportamento, tolto solo
// tutto ciò che leggeva/scriveva il DOM del cursore (v. CHANGELOG/IMPLEMENTATION
// per l'elenco completo di cosa è rimasto e cosa è stato tolto).
//
// Separazione (invariata da v1):
//   ws-client.js      — WebSocket lifecycle + wire protocol
//   host-dispatch.js  — classificazione pura dei ServerMsg (window/relay/ignored/deny)
//   search-buffer.js, aichat-push.js, share-view.mjs — logica di relay pura
//   host.js           — event wiring, lancio finestre, relay (qui)

import { LareWsClient } from "./ws-client.js";
import { createSearchBuffers } from "./search-buffer.js";
import { createAiChatPushGate } from "./aichat-push.js";
import { shareResultLine, shareReceivedLine } from "./share-view.mjs";
import { classifyServerMsg } from "./host-dispatch.mjs";
import { resolveUiLocal, markdownWindowLabel } from "./ui-local.mjs";
import { EXTERNAL_TOOL_CHANNELS } from "./external-channels.js";

// Buffer-and-replay dei risultati di ricerca: la finestra (webview separata)
// può aprirsi DOPO che il backend ha già inviato hit/done. Accumuliamo qui e
// rigiochiamo quando la finestra emette `search:subscribe`.
const searchBuffers = createSearchBuffers();

// ---------------------------------------------------------------------------
// UUID v4 — simple, no external dependency.
// ---------------------------------------------------------------------------
function uuidv4() {
  if (typeof crypto !== "undefined" && crypto.randomUUID) {
    return crypto.randomUUID();
  }
  return "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".replace(/[xy]/g, (c) => {
    const r = (Math.random() * 16) | 0;
    const v = c === "x" ? r : (r & 0x3) | 0x8;
    return v.toString(16);
  });
}

// ---------------------------------------------------------------------------
// Tauri IPC
// ---------------------------------------------------------------------------
const tauriInvoke = window.__TAURI__?.core?.invoke;
const tauriEvent  = window.__TAURI__?.event;

async function invokeCmd(cmd, args) {
  if (!tauriInvoke) return undefined;
  return tauriInvoke(cmd, args);
}

// ---------------------------------------------------------------------------
// Token retrieval from Rust
// ---------------------------------------------------------------------------
async function getToken() {
  try {
    return await invokeCmd("get_lare_token") ?? "";
  } catch (e) {
    console.error("[host] get_lare_token error:", e);
    return "";
  }
}

// ---------------------------------------------------------------------------
// WebSocket client — connessione di default (Hello senza `channel`)
// ---------------------------------------------------------------------------
let client = null;

// Versione di ui.exe (get_ui_version, comando Rust): serve solo per
// rispondere a `ui_ping` con `UiPong{id, version}` (built-in /ping) — non è
// usata per nessun'altra decisione lato JS. Valorizzata in bootstrap() PRIMA
// di initClient() così è già pronta al primo eventuale ui_ping in arrivo.
let uiVersion = "";

async function initClient() {
  if (!tauriInvoke) {
    console.warn("[host] Tauri IPC not available — running outside Tauri?");
    return;
  }

  const token = await getToken();
  if (!token) {
    console.error(
      "[host] Token assente: il file <config-dir>/token non esiste o è vuoto " +
      "(lo crea l'orchestrator al primo avvio)."
    );
    return;
  }

  // Porta WS da startup.json (via get_ws_endpoint) — non una costante
  // hardcoded in ws-client.js, vedi Task 6.
  const url = await invokeCmd("get_ws_endpoint");
  if (!url) {
    console.error("[host] get_ws_endpoint non ha restituito un URL.");
    return;
  }

  client = new LareWsClient({
    url,
    token,
    // Nessuna UI da aggiornare qui: niente più badge di stato, niente più
    // spinner. ws-client.js logga già da solo errori/retry di connessione
    // (console.error/console.info) — questo callback resta solo per
    // diagnostica manuale (DevTools) durante lo sviluppo.
    onStatus: (s) => console.info("[host] ws status:", s),
    onMessage: handleServerMsg,
  });
  client.connect();
}

// ---------------------------------------------------------------------------
// ServerMsg dispatch
//
// classifyServerMsg (host-dispatch.mjs) decide la categoria PRIMA di entrare
// nello switch: "deny" e "ignored" sono gestiti qui senza bisogno di un case
// per ogni tipo — lo switch sotto contiene SOLO i case dei tipi "window" e
// "relay" che hanno davvero qualcosa da fare (alcuni tipi "window"/"relay",
// es. open_screener_picker, non arrivano mai su questa connessione — vedi il
// `default` in fondo allo switch).
// ---------------------------------------------------------------------------
function handleServerMsg(msg) {
  const kind = classifyServerMsg(msg);

  // ── Gate locale per-tool (Docs/superpowers/specs/2026-07-15-local-tool-confirm-gate-design.md) ──
  // Un tool AI marcato "sensibile" chiedeva conferma esplicita con un banner
  // Sì/No nel cursore v1. Senza cursore nessuno può rispondere: la pagina
  // host nega sempre (default sicuro — l'orchestratore ha comunque un
  // timeout che nega da solo se nessuno risponde in tempo).
  if (kind === "deny") {
    if (client) client.sendToolConfirmResponse(msg.id, false);
    return;
  }

  // Messaggi che erano per il cursore v1 (output, watchdog, riga cwd, ecc.):
  // nessuna superficie li consuma più.
  if (kind === "ignored") return;

  switch (msg.type) {
    // ── ADR-013: Superficie 3 — custom windows ─────────────────────────────
    // Received from orchestrator in response to `/show <markdown>` or `/help`.
    // Delegates to Rust `open_markdown_window` which creates a WebviewWindow,
    // stores (title, content, kind) in managed state, and lets window.html read
    // it via `take_window_content`.
    case "open_window":
      openMarkdownWindow(msg.title, msg.content, msg.kind);
      break;

    // ── Ricerca file: finestra dedicata live (Task 11) ─────────────────────
    // SearchOpen apre la finestra; Hit/Done le sono inoltrati via eventi Tauri
    // globali (la finestra filtra per sid).
    case "search_open":
      searchBuffers.open(msg.id);
      openSearchWindow(msg.id, msg.title);
      break;
    case "search_hit":
      if (searchBuffers.hit(msg.id, { path: msg.path, source: msg.source, line: msg.line, snippet: msg.snippet }).emit) {
        emitToSearch("search:hit", { sid: msg.id, path: msg.path, source: msg.source, line: msg.line, snippet: msg.snippet });
      }
      break;
    case "search_done":
      if (searchBuffers.done(msg.id, { count: msg.count, truncated: msg.truncated }).emit) {
        emitToSearch("search:done", { sid: msg.id, count: msg.count, truncated: msg.truncated });
      }
      break;

    // ── Plugin windows (Slice 1) ────────────────────────────────────────────
    case "open_plugin_window":
      // width/height sono opzionali (assenti per calc/ping/counter → undefined).
      openPluginWindow(msg.window_id, msg.title, msg.html, msg.width, msg.height);
      break;
    case "update_plugin_window":
      // window_id must be sent as a JSON number matching the Rust u64.
      emitToPlugin("plugin:update", { window_id: msg.window_id, html: msg.html });
      break;
    case "close_plugin_window":
      emitToPlugin("plugin:close", { window_id: msg.window_id });
      break;

    // ── save_routine — finestra di anteprima dedicata (Fase 2) ──────────────
    // Docs/superpowers/specs/2026-08-05-save-routine-design.md §5. A
    // differenza di tool_confirm_request (banner Sì/No nel cursore), questo
    // apre una finestra dedicata; la risposta è la STESSA
    // sendToolConfirmResponse (vedi setupRoutinePreviewEvents più sotto).
    case "routine_save_preview":
      openRoutinePreviewWindow(
        msg.id, msg.name, msg.description, msg.tags, msg.category, msg.script, msg.replace ?? null
      );
      break;

    // ── Canale shell (2.0, spec §3.2/§4.1) ─────────────────────────────────
    // Finestra di output di un comando slash originato da una shell: si apre
    // subito col segnaposto, il contenuto arriva a Done via evento globale.
    case "open_output_window":
      invokeCmd("open_output_window", { windowId: msg.window_id, title: msg.title })
        .catch((e) => console.error("[host] open_output_window error:", e));
      break;
    case "output_window_content":
      // emitToPlugin è un emit globale generico (nome storico): la finestra
      // di output filtra per window_id come fanno le finestre plugin.
      emitToPlugin("output:content", { window_id: msg.window_id, markdown: msg.markdown });
      break;
    // Finestra locale chiesta da una shell (/config, /library, /aichat, canali esterni).
    case "open_ui_local": {
      const target = resolveUiLocal(msg.name, EXTERNAL_TOOL_CHANNELS);
      if (!target) { console.warn("[host] open_ui_local: nome ignoto", msg.name); break; }
      invokeCmd(target.cmd, target.args).catch((e) => console.error(`[host] ${target.cmd} error:`, e));
      break;
    }
    // Built-in /ping: rispondi con la versione di ui.exe.
    case "ui_ping":
      if (client) client.sendUiPong(msg.id, uiVersion);
      break;

    // ── AI Chat (Slice 1a-ui-B / Task 9/10) ─────────────────────────────────
    case "ai_chat_message":
      // display_name/is_ai (Task 9): campi additivi arrivati dal wire (protocol
      // ChatLine/AiChatMessage) — vanno inoltrati, altrimenti addLine() in
      // aichat-window.js non ha modo di risolvere il nickname/badge per i messaggi
      // LIVE.
      pushAiChat("aichat:msg", {
        from_label: msg.from_label,
        text: msg.text,
        display_name: msg.display_name,
        is_ai: msg.is_ai,
      });
      break;
    case "ai_chat_roster":
      lastKnownRoster = msg.participants;
      pushAiChat("aichat:roster", { participants: msg.participants });
      emitToLibrary("library:roster", { participants: libraryRosterParticipants() });
      break;
    case "ai_chat_reachable_peers":
      lastKnownReachablePeers = msg.labels;
      emitToLibrary("library:reachable-peers", { labels: msg.labels });
      break;
    case "ai_chat_history":
      pushAiChat("aichat:history", { entries: msg.entries });
      break;
    case "ai_chat_self":
      mySelfLabel = msg.label;
      pushAiChat("aichat:self", { label: msg.label });
      break;
    case "ai_chat_join_request":
      pushAiChat("aichat:join-request", { peer_label: msg.peer_label });
      break;
    case "ai_chat_peer_lost":
      pushAiChat("aichat:peer_lost", { label: msg.label });
      break;
    // ── AI Chat — ammissione alla stanza (Task 10) ──────────────────────
    case "ai_chat_join_prompt":
      pushAiChat("aichat:join-prompt", { present: msg.present });
      break;
    case "ai_chat_admission_request":
      pushAiChat("aichat:admission-request", { candidate: msg.candidate });
      break;
    case "ai_chat_admission_resolved":
      pushAiChat("aichat:admission-resolved", { candidate: msg.candidate });
      break;
    case "ai_chat_pending":
      pushAiChat("aichat:pending", { present: msg.present });
      break;
    case "ai_chat_admitted":
      pushAiChat("aichat:admitted", {});
      break;
    case "ai_chat_rejected":
      pushAiChat("aichat:rejected", { retry_after_secs: msg.retry_after_secs });
      break;

    // ── Blocco note (Task 16) ────────────────────────────────────────────
    case "notes_snapshot":
      lastKnownNotes = msg.notes;
      emitToLibrary("library:notes-snapshot", { notes: msg.notes });
      break;
    case "note_upserted":
      if (lastKnownNotes) {
        const idx = lastKnownNotes.findIndex((n) => n.id === msg.note.id);
        if (idx >= 0) lastKnownNotes[idx] = msg.note; else lastKnownNotes.push(msg.note);
      }
      emitToLibrary("library:note-upserted", { note: msg.note });
      break;

    // ── Share (Library → AI Chat) ───────────────────────────────────────
    case "share_request":
      pushAiChat("aichat:share-request", {
        share_id: msg.share_id,
        from_label: msg.from_label,
        doc_name: msg.doc_name,
        size_bytes: msg.size_bytes,
      });
      break;
    // share_result e share_incoming_data mostravano una riga di esito nel
    // pannello del cursore v1 (`renderer.systemMessage`, vedi share-view.mjs:
    // "Riga di esito mostrata nel pannello del cursore principale") — senza
    // cursore quella riga non ha più una superficie. La logghiamo in console
    // invece di perderla silenziosamente; NON la inoltriamo a nessuna
    // finestra (non lo faceva nemmeno v1 — sarebbe un comportamento nuovo,
    // non un'estrazione).
    case "share_result":
      console.info("[host]", shareResultLine(msg.doc_name, msg.target_label, msg.outcome));
      break;
    // Slice 2a: l'orchestrator (MITTENTE) ha bisogno del contenuto vero di un
    // documento già accettato — lo leggiamo con lo stesso comando Tauri già usato
    // da Library per calcolare size_bytes (archive_open).
    case "share_content_request":
      invokeCmd("archive_open", { file: msg.rel_path })
        .then((doc) => {
          if (client) client.sendShareContent(msg.share_id, doc.title, doc.content);
        })
        .catch(() => {
          if (client) client.sendShareContentFailed(msg.share_id, "documento non più disponibile");
        });
      break;
    // Slice 2a: un documento accettato è pronto per essere scritto su disco
    // (DESTINATARIO) — riusa lo stesso comando Tauri già usato per salvare le
    // finestre Markdown (archive_save). Il salvataggio e l'ack (sendShareWritten)
    // restano FUNZIONALI — solo la riga di conferma visiva è stata tolta (vedi
    // sopra, share_result).
    case "share_incoming_data":
      invokeCmd("archive_save", { title: msg.title, content: msg.content })
        .then(() => {
          if (client) client.sendShareWritten(msg.share_id);
          console.info("[host]", shareReceivedLine(msg.title, msg.from_label));
        })
        .catch((e) => {
          console.error("[host] archive_save failed per documento ricevuto via Share:", e);
        });
      break;

    default:
      // "window"/"relay" secondo host-dispatch.mjs ma senza case qui: es.
      // open_screener_picker arriva solo sul canale dedicato di /markets
      // (external-channel-window.js), mai su questa connessione di default.
      break;
  }
}

/**
 * Open a new Markdown window by delegating to Rust.
 *
 * The Rust command creates the WebviewWindow (label = "md-<ts>-<n>"),
 * stores (title, content, kind) in managed HashMap, then the new window calls
 * `take_window_content` at load time to retrieve and render it.
 *
 * @param {string} title          - Window title bar text.
 * @param {string} content        - Raw Markdown string.
 * @param {string} [kind="markdown"] - WindowKind wire value ("markdown" | "help").
 *                                    Defaults to "markdown" for backward compat
 *                                    with any caller that omits the argument.
 */
async function openMarkdownWindow(title, content, kind = "markdown") {
  try {
    // label: markdownWindowLabel(kind) è undefined/null per ogni kind tranne
    // "help" (D15) — Rust la riceve come Option<String>::None e genera una
    // label univoca come prima (comportamento invariato per le altre finestre).
    await invokeCmd("open_markdown_window", { title, content, kind, sourceFile: "", label: markdownWindowLabel(kind) });
  } catch (e) {
    console.error("[host] open_markdown_window error:", e);
  }
}

// ---------------------------------------------------------------------------
// Ricerca file (Task 11/SR4) — finestra dedicata live
// ---------------------------------------------------------------------------

/** Apre la finestra di ricerca live delegando a Rust. */
async function openSearchWindow(sid, title) {
  try {
    await invokeCmd("open_search_window", { title: title || "Ricerca", sid });
  } catch (e) {
    console.error("[host] open_search_window error:", e);
  }
}

/** Inoltra un evento alla finestra di ricerca (emit globale; la finestra filtra per sid). */
function emitToSearch(event, payload) {
  if (tauriEvent?.emit) {
    tauriEvent
      .emit(event, payload)
      .catch((e) => console.error(`[host] emit ${event} error:`, e));
  }
}

/**
 * Invia `/open <path>` all'orchestrator come comando normale.
 *
 * Nel cursore v1 questo passava per `handleSlashCommand`, che oltre a
 * inviare il comando pilotava lo spinner di attività e mostrava un errore
 * nel pannello se il WS non era connesso — entrambe superfici del cursore
 * che qui non esistono più. La pagina host invia solo la parte che conta
 * davvero (il comando sul WS); un WS non connesso finisce solo in console.
 * Unico chiamante di `handleSlashCommand` rimasto dopo l'estrazione (era
 * usato ESCLUSIVAMENTE per questo, da search:open-path e
 * library:open-folder — vedi setupSearchEvents/setupLibraryEvents sotto).
 */
function sendOpenCommand(path) {
  if (!client || !client.isConnected()) {
    console.warn("[host] impossibile eseguire /open: WebSocket non connesso");
    return;
  }
  client.sendCommand(`/open ${path}`, uuidv4());
}

/**
 * Registra i listener per gli eventi emessi DALLA finestra di ricerca verso il main.
 * Idempotente nei fatti: chiamato una volta al bootstrap.
 */
function setupSearchEvents() {
  if (!tauriEvent?.listen) return;
  tauriEvent.listen("search:open-path", (e) => {
    const path = e?.payload?.path;
    if (path) sendOpenCommand(path);
  });
  tauriEvent.listen("search:cancel", (e) => {
    const sid = e?.payload?.sid;
    if (!sid) return;
    if (client) client.cancelSearch(sid);
    searchBuffers.close(sid);
  });
  // La finestra di ricerca, appena pronta, emette `search:subscribe`: rigioca
  // dal buffer gli hit/done arrivati prima che fosse in ascolto, poi va live.
  tauriEvent.listen("search:subscribe", (e) => {
    const sid = e?.payload?.sid;
    if (!sid) return;
    const replay = searchBuffers.subscribe(sid);
    for (const h of replay.hits) {
      emitToSearch("search:hit", { sid, path: h.path, source: h.source, line: h.line, snippet: h.snippet });
    }
    if (replay.done) {
      emitToSearch("search:done", { sid, count: replay.done.count, truncated: replay.done.truncated });
    }
  });
  tauriEvent.listen("search:pause", (e) => {
    const sid = e?.payload?.sid;
    if (sid && client) client.pauseSearch(sid);
  });
  tauriEvent.listen("search:resume", (e) => {
    const sid = e?.payload?.sid;
    if (sid && client) client.resumeSearch(sid);
  });
}

// ---------------------------------------------------------------------------
// Plugin windows (Slice 1) — generic plugin webview
// ---------------------------------------------------------------------------

/**
 * Ask Rust to open a new plugin window (chromeless, always-on-top).
 * Mirrors openSearchWindow: delegates to a Rust async command.
 *
 * @param {number} window_id - Plugin window identifier (u64 from orchestrator).
 * @param {string} title     - Window title bar text.
 * @param {string} html      - Initial plugin HTML to render.
 * @param {number} [width]   - Larghezza iniziale richiesta dal plugin (opzionale).
 * @param {number} [height]  - Altezza iniziale richiesta dal plugin (opzionale).
 */
async function openPluginWindow(window_id, title, html, width, height) {
  try {
    // NB: Tauri v2 espone i parametri snake_case del comando Rust come camelCase
    // lato JS → il comando `open_plugin_window(window_id, …)` si aspetta `windowId`.
    await invokeCmd("open_plugin_window", { windowId: window_id, title, html, width, height });
  } catch (e) {
    console.error("[host] open_plugin_window error:", e);
  }
}

/**
 * Ask Rust to open the routine-save preview window.
 * Mirrors openPluginWindow: delegates to a Rust async command.
 */
async function openRoutinePreviewWindow(id, name, description, tags, category, script, replace) {
  try {
    await invokeCmd("open_routine_preview", { id, name, description, tags, category, script, replace });
  } catch (e) {
    console.error("[host] open_routine_preview error:", e);
  }
}

/**
 * Broadcast a Tauri global event toward one or more plugin windows.
 * The target window filters by its own window_id so only the intended window reacts.
 * Mirrors emitToSearch.
 *
 * @param {string} event   - Tauri event name (e.g. "plugin:update").
 * @param {object} payload - Event payload (must include window_id: number).
 */
function emitToPlugin(event, payload) {
  if (tauriEvent?.emit) {
    tauriEvent
      .emit(event, payload)
      .catch((e) => console.error(`[host] emitToPlugin ${event} error:`, e));
  }
}

// ── AI Chat (Slice 1a-ui-B) ────────────────────────────────────────────────
// La finestra-chat è una webview Tauri che NON apre una WS propria: parla con
// QUESTA connessione (host.js) via eventi Tauri, come le finestre plugin.
// Stato apertura/buffer della finestra-chat: vedi aichat-push.js. L'apertura è
// SOLO esplicita (comando `/aichat`, `aiChatGate.requestOpen()`) — in 2.0
// senza l'overlay quel comando lo emetterà l'orchestratore (plan 2); qui
// restano solo il gate e il relay.
const aiChatGate = createAiChatPushGate();

// Library "Share with" (Slice 1a-ui): ultimo roster noto, per rispondere a
// "library:request-roster" anche se Library si apre PRIMA che arrivi un
// AiChatRoster live. null = mai arrivato (AI Chat disabilitata o non ancora
// connessa a nessuno) — il picker "Condividi" mostrerà "nessuna macchina".
let lastKnownRoster = null;

// Library "Condividi" (fix 2026-07-28): a differenza di lastKnownRoster
// (ammissione alla stanza AI Chat), questa cache segue self.peers ∩
// self.links lato Rust — raggiungibilità di rete, non partecipazione alla
// chat. Canale Tauri separato da "library:roster"/"library:request-roster":
// non condivide stato con la finestra AI Chat.
let lastKnownReachablePeers = null; // null = mai arrivato

// Blocco note (Task 16): cache dell'ultimo snapshot ricevuto dal backend.
// null = mai arrivato; altrimenti array di NoteView.
let lastKnownNotes = null;

// Library "Share with" (M2, review finale 2026-07-06): l'etichetta COMPLETA di
// questa macchina (es. "rumpleteazer-human"), nota da ServerMsg::AiChatSelf.
// Serve a escludere se stessi dall'elenco "Condividi con…" in Library.
let mySelfLabel = null;

// Ponte verso la finestra Library — stesso schema di emitToPlugin/emitToAiChat
// (emit globale Tauri; ogni finestra ascolta solo gli eventi che le competono).
function emitToLibrary(event, payload) {
  if (tauriEvent?.emit) {
    tauriEvent
      .emit(event, payload)
      .catch((e) => console.error(`[host] emitToLibrary ${event} error:`, e));
  }
}

// Il roster inoltrato a Library non deve MAI includere l'etichetta di questa
// stessa macchina — vedi il commento su mySelfLabel sopra per il perché.
function libraryRosterParticipants() {
  return (lastKnownRoster ?? []).filter((label) => label !== mySelfLabel);
}

async function openAiChatWindow() {
  try {
    await invokeCmd("open_aichat_window");
  } catch (e) {
    console.error("[host] open_aichat_window error:", e);
  }
}

// Apre (o riporta in primo piano) la finestra "Nuova nota"/"Modifica nota"
// (v. main.rs open_note_window). `note` è `null` per "Nuova nota", oppure il
// NoteView completo per "Modifica" — solo `id`/`title`/`my_segment_text`
// servono qui, il resto (body fuso, deleted, ...) non ha senso per il form.
async function openNoteWindow(note) {
  try {
    await invokeCmd("open_note_window", {
      noteId: note?.id ?? null,
      title: note?.title ?? "",
      text: note?.my_segment_text ?? "",
    });
  } catch (e) {
    console.error("[host] open_note_window error:", e);
  }
}

/**
 * Listener per gli eventi emessi DALLA finestra nota verso il main.
 *
 * "note-window:save" — l'utente ha cliccato "Salva".
 *   `noteId: null` → creazione: payload `{title, text}`.
 *   `noteId` presente → modifica: payload `{editText?, editTitle?}` — solo i
 *   campi DAVVERO cambiati.
 */
function setupNoteWindowEvents() {
  if (!tauriEvent?.listen) return;
  tauriEvent.listen("note-window:save", (e) => {
    const { noteId, title, text, editText, editTitle } = e.payload ?? {};
    if (!client) return;
    if (!noteId) {
      client.sendNoteCreate(title, text);
      return;
    }
    if (editText !== undefined) client.sendNoteEdit(noteId, editText);
    if (editTitle !== undefined) client.sendNoteEditTitle(noteId, editTitle);
  });
}

function emitToAiChat(event, payload) {
  if (tauriEvent?.emit) {
    tauriEvent
      .emit(event, payload)
      .catch((e) => console.error(`[host] emitToAiChat ${event} error:`, e));
  }
}

// Spinge un evento verso la finestra-chat. Se è già pronta, va diretto; se
// l'apertura è in corso (richiesta con `/aichat`), bufferizza (buffer-and-replay:
// evita di perdere il primo messaggio prima che la webview separata abbia
// registrato i suoi listener); altrimenti scarta — niente auto-apertura.
function pushAiChat(event, payload) {
  const { action } = aiChatGate.push(event, payload);
  if (action === "emit") emitToAiChat(event, payload);
  // "buffer" → in coda, rigiocato quando la finestra chat è pronta (vedi
  // "aichat:ready" sotto). "drop" → nessuno ha chiesto la finestra: il
  // cursore v1 accendeva qui un'icona "attività non vista"
  // (updateAiChatNotifyIndicator) — quell'icona viveva nel DOM del cursore,
  // sparito con lui. Senza un consumatore, tenere solo lo stato scritto e
  // mai letto sarebbe dead code: rimosso (vedi IMPLEMENTATION.md).
}

// Listener per gli eventi emessi DALLA finestra-chat verso il main.
function setupAiChatEvents() {
  if (!tauriEvent?.listen) return;
  // La finestra è pronta → ri-registra il sink WS, rigioca il buffer, passa in live.
  tauriEvent.listen("aichat:ready", () => {
    if (client) client.sendAiChatOpen();
    for (const b of aiChatGate.ready()) emitToAiChat(b.event, b.payload);
  });
  tauriEvent.listen("aichat:send", (e) => {
    const t = e?.payload?.text;
    if (t && client) client.sendAiChatSend(t);
  });
  tauriEvent.listen("aichat:consent", (e) => {
    const p = e?.payload;
    if (p && client) client.sendAiChatJoinConsent(p.peer_label, !!p.accept);
  });
  tauriEvent.listen("aichat:share-consent", (e) => {
    const p = e?.payload;
    if (p && client) client.sendShareConsent(p.share_id, !!p.accept);
  });
  // ── Ammissione alla stanza (Task 10): risposta ai due gate + re-request ──
  tauriEvent.listen("aichat:join-decision", (e) => {
    const accept = !!e?.payload?.accept;
    if (client) client.sendAiChatJoinDecision(accept);
  });
  tauriEvent.listen("aichat:admission-vote", (e) => {
    const p = e?.payload;
    if (p && client) client.sendAiChatAdmissionVote(p.candidate, !!p.accept);
  });
  tauriEvent.listen("aichat:request-admission", () => {
    if (client) client.sendAiChatRequestAdmission();
  });
  tauriEvent.listen("aichat:closed", () => {
    aiChatGate.closed();
    if (client) client.sendAiChatClosed();
  });
}

/**
 * Register listeners for events emitted BY plugin windows back to the main window.
 *
 * "plugin:ui-event"      — user clicked a [data-evt] element inside a plugin window.
 * "plugin:window-closed" — user clicked ✕ or the window was closed by the OS.
 */
function setupPluginEvents() {
  if (!tauriEvent?.listen) return;

  tauriEvent.listen("plugin:ui-event", (e) => {
    const p = e?.payload;
    // Guard: window_id must be a finite number (Rust u64 cannot be string).
    if (!p || !Number.isFinite(p.window_id)) return;
    if (client) client.sendPluginUiEvent(p.window_id, p.element_id, p.value ?? null);
  });

  tauriEvent.listen("plugin:window-closed", (e) => {
    const p = e?.payload;
    if (!p || !Number.isFinite(p.window_id)) return;
    if (client) client.sendPluginWindowClosed(p.window_id);
  });
}

/**
 * "routine-preview:decision" — the routine preview window's Save/Annulla
 * buttons emit this Tauri global event; forward it as the SAME
 * ClientMsg::ToolConfirmResponse the cursor's Sì/No banner already sent
 * (client.sendToolConfirmResponse) — no new WS message type.
 * Payload: { id: string, accept: boolean }
 */
function setupRoutinePreviewEvents() {
  if (!tauriEvent?.listen) return;

  tauriEvent.listen("routine-preview:decision", (e) => {
    const p = e?.payload;
    if (!p || typeof p.id !== "string") return;
    if (client) client.sendToolConfirmResponse(p.id, !!p.accept);
  });
}

/**
 * Listener per l'evento back-channel della finestra /config.
 *
 * Nel cursore v1, quando l'utente salvava da config.html, `config-window.js`
 * emetteva `config:saved` e il main riapplicava aspetto/flag SENZA reload
 * (`applySavedConfig`, sparita con il cursore). La pagina host non ha aspetto
 * da riapplicare — ogni finestra legge la propria configurazione da sé via
 * `get_config`. Il listener resta registrato (documenta il contratto
 * dell'evento per le finestre che lo emettono) ma non fa nulla.
 */
function setupConfigWindowEvents() {
  if (!tauriEvent?.listen) return;
  tauriEvent.listen("config:saved", () => {});
}

/**
 * Registra i listener per gli eventi emessi DALLA finestra library verso il main.
 */
function setupLibraryEvents() {
  if (!tauriEvent?.listen) return;
  tauriEvent.listen("library:open-folder", (e) => {
    const path = e?.payload?.path;
    if (path) sendOpenCommand(path);
  });

  // Library "Share with": la finestra Library chiede il roster corrente
  // all'apertura (invece di bufferizzare push come per AI Chat — il roster
  // cambia raramente, un pull all'apertura è sufficiente e più semplice).
  tauriEvent.listen("library:request-roster", () => {
    emitToLibrary("library:roster", { participants: libraryRosterParticipants() });
  });

  tauriEvent.listen("library:request-reachable-peers", () => {
    emitToLibrary("library:reachable-peers", { labels: lastKnownReachablePeers ?? [] });
  });

  // Library "Share with": invio della richiesta di condivisione.
  tauriEvent.listen("library:share-document", (e) => {
    const { rel_path, doc_name, size_bytes, target_label } = e.payload ?? {};
    if (client) client.sendShareDocument(rel_path, doc_name, size_bytes, target_label);
  });

  // Blocco note (Task 16): richiesta dello snapshot corrente all'apertura della finestra Library.
  tauriEvent.listen("library:request-notes", () => {
    emitToLibrary("library:notes-snapshot", { notes: lastKnownNotes ?? [] });
  });

  // Blocco note: richiesta di apertura della finestra "Nuova nota"/"Modifica
  // nota" (dalla finestra Library — pulsante "Nuova" o ✏️ su una nota).
  tauriEvent.listen("library:note-window-open", (e) => {
    openNoteWindow(e.payload?.note ?? null);
  });

  // Blocco note: cancellazione di una nota (dalla finestra Library).
  tauriEvent.listen("library:note-delete", (e) => {
    const { id } = e.payload ?? {};
    if (client) client.sendNoteDelete(id);
  });
}

// ---------------------------------------------------------------------------
// Bootstrap: connetti il WS di default, registra i listener di relay.
//
// A differenza di app.js (v1) NON c'è più: caricamento/applicazione della
// config (aspetto/attività/ricerca web — la pagina host non ha DOM da
// vestire), fetch di home_dir (serviva solo a formatCwd, sparita col
// cursore), focus/editor/idle-duck/diagnosi (tutto cursore).
// ---------------------------------------------------------------------------
async function bootstrap() {
  uiVersion = (await invokeCmd("get_ui_version")) ?? "";

  await initClient();

  // Listener per gli eventi della finestra di ricerca (click→/open, chiusura→cancel).
  setupSearchEvents();

  // Listener per l'evento della finestra library (pulsante "Apri cartella" → /open).
  setupLibraryEvents();

  // Listener per gli eventi back-channel delle finestre plugin.
  setupPluginEvents();
  setupRoutinePreviewEvents();

  // Listener config:saved — la finestra /config notifica il salvataggio.
  setupConfigWindowEvents();

  // Listener back-channel della finestra-chat AI Chat (send/consent/closed/ready).
  setupAiChatEvents();

  // Listener back-channel della finestra "Nuova nota"/"Modifica nota".
  setupNoteWindowEvents();

  // Flag di sviluppo TEMPORANEO (piano 1 → rimosso nel piano 3, vedi
  // main.rs::DevOpenRequest): senza overlay né canale shell nessuna
  // superficie può ancora chiedere l'apertura di /config o Library da sola
  // — `ui.exe --open config|library` lo fa al posto loro, per poter
  // verificare le finestre a mano durante lo sviluppo.
  const open = await invokeCmd("dev_open_request");
  if (open === "config") invokeCmd("open_config_window");
  if (open === "library") invokeCmd("open_library_window");
}

bootstrap();
