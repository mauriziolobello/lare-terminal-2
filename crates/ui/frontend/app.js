// app.js — Application wiring layer for Lare Terminal.
//
// Responsibility (SRP): connects the WebSocket client to the renderer,
// the keyboard/DOM event handlers, and CSS vars.
// This file knows about all modules but none of them know about each other.
//
// Separation:
//   ws-client.js      — WebSocket lifecycle + wire protocol
//   renderer.js       — DOM output rendering
//   config-dialog.js  — /config dialog (used by config-window.js, not here)
//   line-editor.js    — pure line-editor state machine (text + caret)
//   app.js            — event wiring, input handling, command dispatch (here)

import { LareWsClient } from "./ws-client.js";
import { LareRenderer }  from "./renderer.js";
import * as lineEditor      from "./line-editor.js";
import { formatCwd }        from "./cwd-format.js";
import { pageDelta }        from "./page-scroll.js";
import { createSearchBuffers } from "./search-buffer.js";
import { buildDiagnosisMarkdown } from "./connection-diagnosis.js";
import { duckStep }               from "./idle-duck.js";
import { shareResultLine, shareReceivedLine } from "./share-view.mjs";
import { createAiChatPushGate, isNotifiableDrop } from "./aichat-push.js";
import { EXTERNAL_TOOL_CHANNELS, findExternalChannelBySlash } from "./external-channels.js";
import * as pathUtils from "./path-utils.js";

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
// DOM references
// ---------------------------------------------------------------------------
const hiddenInput      = document.getElementById("hidden-input");
const typedTextEl      = document.getElementById("typed-text");
const outputArea       = document.getElementById("output-area");
const statusBadge      = document.getElementById("status-badge");
const aiChatNotifyEl   = document.getElementById("aichat-notify");
const activityTitleEl  = document.getElementById("activity-title");
const activityStatusEl = document.getElementById("activity-status");
const promptEl         = document.querySelector(".prompt");
const cwdLineEl        = document.getElementById("cwd-line");
const stopBtnEl        = document.getElementById("stop-btn");

// ---------------------------------------------------------------------------
// Home directory — fetched once at bootstrap via Tauri `home_dir` command.
// Used by formatCwd() to substitute the ~ alias.  Falls back to "" so that
// formatCwd() skips the substitution gracefully when Tauri is not available.
// ---------------------------------------------------------------------------
let homeDir = "";

// ---------------------------------------------------------------------------
// Current working directory — the RAW path (unlike `cwdLineEl`'s textContent,
// which is `formatCwd`'s abbreviated display form). Updated on every
// ServerMsg::Cwd. Used to resolve relative paths for Tab-completion
// (`list_path_completions` needs a real filesystem path, not "~\Doc").
// ---------------------------------------------------------------------------
let currentCwd = "";

// ---------------------------------------------------------------------------
// Tab-completion cycle state (2026-07-19) — `null` when no completion is in
// progress. While active: `{ rangeStart, rangeEnd, candidates, quoteChar,
// dirPart, index, text }` — `quoteChar` (`"`, `'`, or `null`) and `dirPart`
// are carried across cycles so every candidate is rebuilt via
// `pathUtils.buildCompletionText` the same way each time (re-prepending the
// typed subdirectory, re-applying the user's own leading quote if any).
// Cleared on any keydown OTHER than Tab/Shift+Tab (see the keydown handler)
// so a completion session only survives across consecutive Tab presses,
// mirroring PowerShell's own Tab-cycling behavior.
// ---------------------------------------------------------------------------
let tabCompletion = null;

// ---------------------------------------------------------------------------
// Activity indicator controller
//
// Tracks in-flight commands (by id) and drives three UI styles:
//   "title"  — braille spinner next to the "Lare Terminal" label.
//   "status" — braille spinner + "elaboro…" to the left of the status badge.
//   "prompt" — the › prompt pulsates via CSS animation.
//
// Only ONE setInterval is running at any time (startActivity is idempotent).
// ---------------------------------------------------------------------------

/** Braille spinner frames (~90 ms per frame). */
const SPINNER_FRAMES = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const SPINNER_INTERVAL_MS = 90;

/** Set of command ids currently in-flight (sent but not yet done/error). */
const inFlight = new Set();

// ── Watchdog: rileva blocchi AI e avvisa / annulla automaticamente ────────────
// lastChunkAt: timestamp dell'ultimo chunk ricevuto per ogni comando in volo.
// Il watchdog confronta (now - lastChunkAt) con le soglie di warning e cancel.
const _cmdStallData = new Map(); // id → { lastChunkAt: number }
let _watchdogSuffix = "";        // testo aggiuntivo mostrato dallo spinner dopo 30s
const WATCHDOG_WARN_SEC   = 30;  // mostra il contatore nel'activity indicator
const WATCHDOG_CANCEL_SEC = 180; // auto-cancel dopo 3 minuti senza risposta

/** Current activity style ("title" | "status" | "prompt"). */
let activityStyle = "title";

/** Running setInterval handle, or null if idle. */
let _spinnerTimer = null;

/** Current frame index into SPINNER_FRAMES. */
let _frameIndex = 0;

/**
 * Apply (or switch) the activity style.
 *
 * Cleans up the previous style's DOM artefacts and re-starts the spinner if
 * there is currently activity in progress.  Safe to call at any time.
 *
 * @param {string} style  "title" | "status" | "prompt"
 */
function setActivityStyle(style) {
  activityStyle = style || "title";
  // stopActivity() clears BOTH the timer AND the DOM.  We cannot call
  // _clearActivityUI() alone here: if a spinner timer is already running and
  // we switch to "prompt", _clearActivityUI leaves _spinnerTimer non-null, so
  // startActivity() below would hit the idempotency guard and bail out before
  // adding the .busy class.  stopActivity() is the correct one-stop reset.
  stopActivity();
  // If a command is still in flight, restart immediately with the new style.
  if (inFlight.size > 0) {
    startActivity();
  }
}

/**
 * Called when a command is dispatched to the orchestrator.
 *
 * @param {string} id  The command UUID (same id that will appear in done/error).
 */
function commandStarted(id) {
  inFlight.add(id);
  _cmdStallData.set(id, { lastChunkAt: Date.now() });
  startActivity();
  if (stopBtnEl) stopBtnEl.classList.remove("hidden");
}

/** Reset del timer di stall per il comando `id` — chiamato ad ogni chunk ricevuto. */
function commandChunkReceived(id) {
  const d = _cmdStallData.get(id);
  if (d) d.lastChunkAt = Date.now();
  _watchdogSuffix = "";
}

/**
 * Called when a done or error message arrives from the orchestrator.
 *
 * @param {string} id  The command UUID from the server message.
 */
function commandEnded(id) {
  inFlight.delete(id);
  _cmdStallData.delete(id);
  if (inFlight.size === 0) {
    stopActivity();
    _watchdogSuffix = "";
    if (stopBtnEl) stopBtnEl.classList.add("hidden");
  }
}

/**
 * Start the activity animation.
 *
 * Idempotent: if the spinner is already running, returns without starting a
 * second timer (prevents double-speed flicker and timer leaks on concurrent
 * commands).
 */
function startActivity() {
  if (_spinnerTimer !== null) return; // already running — idempotent

  if (activityStyle === "prompt") {
    // CSS animation on the prompt element; no timer needed.
    if (promptEl) promptEl.classList.add("busy");
    return;
  }

  // Braille spinner: tick every SPINNER_INTERVAL_MS ms.
  _frameIndex = 0;
  _tickSpinner(); // render first frame immediately (no initial delay)
  _spinnerTimer = setInterval(_tickSpinner, SPINNER_INTERVAL_MS);
}

/**
 * Stop the activity animation and clear all UI artefacts.
 */
function stopActivity() {
  if (_spinnerTimer !== null) {
    clearInterval(_spinnerTimer);
    _spinnerTimer = null;
  }
  _clearActivityUI();
}

/**
 * Advance the spinner by one frame.  Called by the setInterval handler.
 */
function _tickSpinner() {
  const frame = SPINNER_FRAMES[_frameIndex % SPINNER_FRAMES.length];
  _frameIndex++;

  if (activityStyle === "title") {
    if (activityTitleEl)  activityTitleEl.textContent = frame;
    // In "title" mode activityStatusEl è normalmente vuoto → usato dal watchdog.
    if (activityStatusEl) activityStatusEl.textContent = _watchdogSuffix ? _watchdogSuffix.trimStart() : "";
  } else if (activityStyle === "status") {
    if (activityStatusEl) activityStatusEl.textContent = `${frame} elaboro…${_watchdogSuffix}`;
  }
}

/**
 * Clear all activity UI state without touching the timer.
 *
 * Removes the CSS class from the prompt and empties the spinner elements.
 */
function _clearActivityUI() {
  if (promptEl)         promptEl.classList.remove("busy");
  if (activityTitleEl)  activityTitleEl.textContent = "";
  if (activityStatusEl) activityStatusEl.textContent = "";
}

// ---------------------------------------------------------------------------
// Watchdog interval — controlla il silenzio ogni 5 secondi.
//
// Tre stati:
//   < 30s   → silenzio normale (nessun indicatore)
//   30–180s → aggiunge il contatore al'activity indicator ("· 45s ↻")
//   ≥ 180s  → auto-cancel + chunk di avviso nell'output del comando
// ---------------------------------------------------------------------------
setInterval(() => {
  if (_cmdStallData.size === 0) {
    if (_watchdogSuffix) { _watchdogSuffix = ""; }
    return;
  }
  const now = Date.now();
  let maxSilenceSec = 0;
  for (const d of _cmdStallData.values()) {
    const sec = Math.round((now - d.lastChunkAt) / 1000);
    if (sec > maxSilenceSec) maxSilenceSec = sec;
  }
  if (maxSilenceSec >= WATCHDOG_CANCEL_SEC) {
    for (const id of [...inFlight]) {
      if (client) client.cancelCommand(id);
      renderer.appendChunk(
        id,
        `\n⏱ Watchdog: nessuna risposta per ${maxSilenceSec}s — operazione annullata automaticamente.\n`
      );
      commandEnded(id);
    }
  } else if (maxSilenceSec >= WATCHDOG_WARN_SEC) {
    const min = Math.floor(maxSilenceSec / 60);
    const sec = maxSilenceSec % 60;
    _watchdogSuffix = min > 0
      ? ` · ${min}m${sec > 0 ? sec + "s" : ""} ↻`
      : ` · ${maxSilenceSec}s ↻`;
    // In "prompt" mode _tickSpinner non gira — aggiorna activityStatusEl direttamente.
    if (activityStyle === "prompt" && activityStatusEl) {
      activityStatusEl.textContent = _watchdogSuffix.trimStart();
    }
  } else {
    _watchdogSuffix = "";
  }
}, 5000);

// ---------------------------------------------------------------------------
// Stop button — cancella subito tutti i comandi in volo (ottimista lato client).
// Il server riceve CancelCommand e annulla il loop AI.
// ---------------------------------------------------------------------------
if (stopBtnEl) {
  stopBtnEl.addEventListener("click", () => {
    for (const id of [...inFlight]) {
      if (client) client.cancelCommand(id);
      commandEnded(id);
    }
  });
}

// ---------------------------------------------------------------------------
// Renderer (pure DOM, no network)
// ---------------------------------------------------------------------------
const renderer = new LareRenderer(outputArea, statusBadge);

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
// Finestra↔pannello: il pannello RIEMPIE la finestra + auto-grow all'output.
// ---------------------------------------------------------------------------
// Il `.overlay-panel` riempie la finestra (index.html: body `display:flex`,
// pannello `flex:1`) → ridimensionare la finestra (bordo blu, resizable:true)
// ridimensiona il pannello; l'`#output-area` (`flex:1`) scrolla. Niente bande
// trasparenti che intercettano i click. Centratura su F2: resta in Rust.

// Dimensione compatta di default (px logici/CSS), usata da resetWindow (Ctrl+R).
const WINDOW_DEFAULT = { width: 1064, height: 140 };

// API finestra dal global Tauri (withGlobalTauri:true → window.__TAURI__ esiste
// nella WebView2). currentWindow è null in test/browser puro → tutto si guarda.
const tauriWindowApi = window.__TAURI__?.window;
const currentWindow  = tauriWindowApi?.getCurrentWindow?.() ?? null;

// ── Auto-grow: la finestra CRESCE da sola per mostrare il nuovo output ──────
// (fino a ~70% dell'altezza schermo), poi l'output scrolla. SOLO crescita: non
// rimpicciolisce mai da sola → l'history resta visibile e un resize manuale è
// rispettato. Per tornare compatti: Ctrl+R (resetWindow).
//
// Trigger: MutationObserver sul CONTENUTO dell'#output-area (NON ResizeObserver).
// setSize cambia la SIZE, non il contenuto → non ri-triggera l'observer = niente
// loop. Debounce per non saltellare durante lo streaming token-by-token.
(function setupAutoGrow() {
  if (!currentWindow || !tauriWindowApi?.LogicalSize || typeof MutationObserver === "undefined") return;

  const MAX_SCREEN_FRAC = 0.7; // tetto: 70% dell'altezza schermo disponibile

  function growToFit() {
    const hidden = outputArea.scrollHeight - outputArea.clientHeight; // output non visibile (px)
    if (hidden <= 1) return; // tutto già visibile → niente da fare
    const avail = window.screen?.availHeight || 1080;
    // Tetto: 70% dello schermo MA senza sforare il bordo basso — la finestra
    // cresce verso il basso (top-left ancorato), quindi il max utile è lo spazio
    // sotto il suo top (window.screenY). Mai sotto la default compatta.
    const topY = Number.isFinite(window.screenY) ? window.screenY : 0;
    const maxInner = Math.max(
      WINDOW_DEFAULT.height,
      Math.min(Math.floor(avail * MAX_SCREEN_FRAC), Math.floor(avail - topY - 16))
    );
    const target = Math.min(window.innerHeight + hidden, maxInner);
    if (target <= window.innerHeight + 1) return; // già al tetto
    // LogicalSize = px CSS, coerente con innerWidth/Height. La larghezza resta.
    currentWindow.setSize(new tauriWindowApi.LogicalSize(window.innerWidth, target)).catch(() => {});
  }

  let timer;
  const mo = new MutationObserver(() => {
    clearTimeout(timer);
    timer = setTimeout(growToFit, 80);
  });
  mo.observe(outputArea, { childList: true, subtree: true, characterData: true });
})();

// ── Reset finestra (Ctrl+R): torna alla dimensione compatta di default e
// ri-centra. La centratura è di Rust: `show_overlay` applica la posizione
// configurata (Center/BottomCenter/NearMouse). Utile dopo che l'auto-grow o un
// resize manuale ha allargato la finestra.
async function resetWindow() {
  if (currentWindow && tauriWindowApi?.LogicalSize) {
    try {
      await currentWindow.setSize(new tauriWindowApi.LogicalSize(WINDOW_DEFAULT.width, WINDOW_DEFAULT.height));
    } catch (e) {
      console.error("[reset-window] setSize:", e);
    }
  }
  invokeCmd("show_overlay").catch(console.error); // ri-centra (size già compatta)
}

// ---------------------------------------------------------------------------
// Clipboard (Tauri v2 clipboardManager — available when withGlobalTauri: true)
// ---------------------------------------------------------------------------
// window.__TAURI__.clipboardManager is provided by tauri-plugin-clipboard-manager
// when withGlobalTauri: true (tauri.conf.json).  Wrapped defensively: errors do
// not crash the terminal, they are logged to the console.
const clipboard = window.__TAURI__?.clipboardManager;

async function clipboardWrite(text) {
  try {
    if (clipboard?.writeText) await clipboard.writeText(text);
  } catch (e) {
    console.warn("[app] clipboard write error:", e);
  }
}

async function clipboardRead() {
  try {
    if (clipboard?.readText) return await clipboard.readText();
  } catch (e) {
    console.warn("[app] clipboard read error:", e);
  }
  return "";
}

// ---------------------------------------------------------------------------
// CSS variable application (cursor appearance from config)
// ---------------------------------------------------------------------------

/**
 * Apply appearance fields from a Config object to CSS custom properties on
 * :root.  The stylesheet references these vars so changes take effect
 * immediately without a reload.
 *
 * @param {{ cursor_color: string, cursor_font: string, cursor_size: number, window_alpha: number }} cfg
 */
function applyAppearance(cfg) {
  const root = document.documentElement;
  if (cfg.cursor_color) root.style.setProperty("--cursor-color", cfg.cursor_color);
  if (cfg.cursor_font)  root.style.setProperty("--cursor-font",  cfg.cursor_font);
  if (cfg.cursor_size)  root.style.setProperty("--cursor-size",  `${cfg.cursor_size}px`);
  // Trasparenza della finestra principale (0.0-1.0): alimenta --bg in index.html
  // (var derivata: rgba(15, 15, 22, var(--window-alpha, 0.87))).
  if (typeof cfg.window_alpha === "number") {
    root.style.setProperty("--window-alpha", cfg.window_alpha);
  }
  // Aggiorna il timeout del pulcino e riavvia il countdown con il nuovo valore.
  _idleDuckMins = typeof cfg.idle_duck_minutes === "number" ? cfg.idle_duck_minutes : 5;
  resetIdleTimer();
}

// ---------------------------------------------------------------------------
// Web search state
//
// Tracks whether the user has enabled server-side AI web search (from /config).
// Default true — matches Config::default().web_search_enabled in config.rs.
// Updated at bootstrap (from get_config) and on every successful /config save.
// Passed as the third arg to client.sendCommand() so the orchestrator knows
// whether to include web_search / web_fetch tools in the AI request.
// ---------------------------------------------------------------------------
let webSearchEnabled = true;

// ---------------------------------------------------------------------------
// Config window back-channel
//
// La finestra /config (config.html + config-window.js) è una Tauri webview
// separata. Quando l'utente salva, config-window.js emette l'evento Tauri
// `config:saved` con il payload della config salvata. Lo ascoltiamo in
// setupConfigWindowEvents() (chiamata da bootstrap) e riapplichiamo qui.
// ---------------------------------------------------------------------------

/**
 * Riapplica al pannello principale una config appena salvata.
 * Chiamata dall'evento config:saved emesso dalla finestra /config.
 *
 * @param {{ cursor_color?: string, cursor_font?: string, cursor_size?: number,
 *            activity_indicator?: string, web_search_enabled?: boolean }} savedCfg
 */
function applySavedConfig(savedCfg) {
  applyAppearance(savedCfg);
  setActivityStyle(savedCfg.activity_indicator);
  webSearchEnabled = savedCfg.web_search_enabled !== false;
}

// ---------------------------------------------------------------------------
// Token retrieval from Rust
// ---------------------------------------------------------------------------
async function getToken() {
  try {
    return await invokeCmd("get_lare_token") ?? "";
  } catch (e) {
    console.error("[app] get_lare_token error:", e);
    return "";
  }
}

// ---------------------------------------------------------------------------
// Idle duck animation
//
// Dopo `idleDuckMinutes` minuti senza input, 🐤 appare nel pannello e cammina
// avanti e indietro usando requestAnimationFrame + la funzione pura duckStep.
// Qualsiasi keydown o click nel pannello nasconde il pulcino e azzera il timer.
// ---------------------------------------------------------------------------
const duckEl = document.getElementById("idle-duck");

let _idleTimer     = null;   // setTimeout handle
let _duckRaf       = null;   // requestAnimationFrame handle
let _duckState     = { x: 0, dir: 1 };
let _duckLastTs    = null;
let _idleDuckMins  = 5;      // aggiornato da applyConfig() al caricamento

/** Avvia (o riavvia) il countdown per l'apparizione del pulcino. */
function resetIdleTimer() {
  if (_idleTimer !== null) { clearTimeout(_idleTimer); _idleTimer = null; }
  _stopDuck();
  if (!_idleDuckMins || _idleDuckMins <= 0) return; // 0 = disabilitato
  _idleTimer = setTimeout(_showDuck, _idleDuckMins * 60 * 1000);
}

function _showDuck() {
  _idleTimer = null;
  if (!duckEl) return;
  _duckState  = { x: 0, dir: 1 };
  _duckLastTs = null;
  duckEl.classList.add("walking");
  duckEl.classList.remove("going-right");
  _duckRaf = requestAnimationFrame(_animateDuck);
}

function _stopDuck() {
  if (_duckRaf !== null) { cancelAnimationFrame(_duckRaf); _duckRaf = null; }
  if (duckEl) duckEl.classList.remove("walking", "going-right");
}

function _animateDuck(ts) {
  if (_duckLastTs === null) _duckLastTs = ts;
  const dt = ts - _duckLastTs;
  _duckLastTs = ts;

  // parentElement è .input-row — usa la sua larghezza come bounds
  const rowW = duckEl.parentElement?.clientWidth ?? 1060;
  const duckW = duckEl.offsetWidth || 22;
  const maxX  = rowW - duckW;

  _duckState = duckStep(_duckState, dt, maxX);
  duckEl.style.left = `${_duckState.x}px`;
  // 🐤 guarda a sinistra di default: flippa quando va a destra
  duckEl.classList.toggle("going-right", _duckState.dir === 1);

  _duckRaf = requestAnimationFrame(_animateDuck);
}

// ---------------------------------------------------------------------------
// Connection auto-diagnosis
//
// After DIAGNOSIS_DELAY_MS of persistent error/disconnected status, or
// immediately when LARE_TOKEN is missing, we run `diagnose_connection` (a Tauri
// command that checks the env var + TCP port 7331) and show the result in a
// Markdown window so the user gets actionable guidance.
//
// Guard: _diagnosisShown prevents duplicate windows in the same "down session";
// it resets on "connected" so a future disconnect can trigger again.
// ---------------------------------------------------------------------------
const DIAGNOSIS_DELAY_MS = 8_000;
let _diagnosisTimer   = null;
let _diagnosisShown   = false;

async function runDiagnosis() {
  if (_diagnosisShown) return;
  _diagnosisShown = true;

  let result;
  try {
    result = await invokeCmd("diagnose_connection");
  } catch (e) {
    console.error("[app] diagnose_connection error:", e);
    return;
  }

  const content = buildDiagnosisMarkdown(result.token_set, result.port_open);
  await openMarkdownWindow("Diagnosi connessione", content, "help");
}

function scheduleDiagnosis() {
  if (_diagnosisShown || _diagnosisTimer !== null) return;
  _diagnosisTimer = setTimeout(() => {
    _diagnosisTimer = null;
    runDiagnosis();
  }, DIAGNOSIS_DELAY_MS);
}

function cancelDiagnosis() {
  if (_diagnosisTimer !== null) {
    clearTimeout(_diagnosisTimer);
    _diagnosisTimer = null;
  }
  _diagnosisShown = false;
}

// ---------------------------------------------------------------------------
// WebSocket client
// ---------------------------------------------------------------------------
let client = null;

async function initClient() {
  if (!tauriInvoke) {
    console.warn("[app] Tauri IPC not available — running outside Tauri?");
    renderer.setStatus("error");
    return;
  }

  const token = await getToken();
  if (!token) {
    renderer.setStatus("error");
    console.error(
      "[app] LARE_TOKEN is not set.  Start the UI with " +
      "LARE_TOKEN=<token> set in the environment."
    );
    runDiagnosis(); // immediate — no point waiting, the token is clearly missing
    return;
  }

  client = new LareWsClient({
    token,
    onStatus: (s) => {
      renderer.setStatus(s);
      if (s === "disconnected" || s === "error") {
        // Clear any in-flight state to avoid a stuck spinner.
        inFlight.clear();
        _cmdStallData.clear();
        _watchdogSuffix = "";
        if (stopBtnEl) stopBtnEl.classList.add("hidden");
        stopActivity();
        // Schedule a diagnostic window after a short delay, so the user gets
        // guidance if the orchestrator stays unreachable.
        scheduleDiagnosis();
      }
      if (s === "connected") {
        // Connection recovered — cancel any pending or shown diagnosis.
        cancelDiagnosis();
      }
    },
    onMessage: handleServerMsg,
  });
  client.connect();
}

// ---------------------------------------------------------------------------
// ServerMsg dispatch
// ---------------------------------------------------------------------------
function handleServerMsg(msg) {
  switch (msg.type) {
    case "server_info":
      console.info("[app] server_info:", msg.version, msg.ai_provider);
      break;
    case "chunk":
      commandChunkReceived(msg.id);
      renderer.appendChunk(msg.id, msg.content);
      break;
    case "heartbeat":
      // Nessun contenuto, nessun render: resetta solo il timer di silenzio
      // del watchdog (stessa funzione usata da "chunk") — vedi
      // Docs/superpowers/specs/2026-08-07-ai-turn-heartbeat-watchdog-design.md.
      commandChunkReceived(msg.id);
      break;
    case "done":
      renderer.markDone(msg.id, msg.exit_code ?? null);
      commandEnded(msg.id);
      break;
    case "error":
      renderer.markError(msg.id, msg.code, msg.message);
      commandEnded(msg.id);
      break;
    case "pong":
      break;

    // ── Gate locale per-tool (Docs/superpowers/specs/2026-07-15-local-tool-confirm-gate-design.md) ──
    // Un tool AI marcato "sensibile" (nessuno ancora in questo spec) chiede
    // conferma esplicita prima di eseguire. Il banner si disabilita dopo un click.
    case "tool_confirm_request":
      renderer.confirmBanner(msg.id, msg.commands, (accept) => {
        if (client) client.sendToolConfirmResponse(msg.id, accept);
      });
      break;

    // ── ADR-013: Superficie 3 — custom windows ─────────────────────────────
    // Received from orchestrator in response to `/show <markdown>` or `/help`.
    // Delegates to Rust `open_markdown_window` which creates a WebviewWindow,
    // stores (title, content, kind) in managed state, and lets window.html read
    // it via `take_window_content`.
    // `msg.kind` is the WindowKind wire value ("markdown" or "help"); it is
    // forwarded to Rust so window.js can apply CSS for special window types.
    case "open_window":
      openMarkdownWindow(msg.title, msg.content, msg.kind);
      break;

    // ── Ricerca file: finestra dedicata live (Task 11) ─────────────────────
    // SearchOpen apre la finestra; Hit/Done le sono inoltrati via eventi Tauri
    // globali (la finestra filtra per sid). Il Command /find che ha avviato la
    // ricerca riceve il SUO Done a parte (chiude lo spinner del comando).
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

    // ── Plugin windows (Slice 1) ───────────────────────────────────────────
    // The orchestrator sends these three variants to drive the generic plugin
    // window UI.  All three delegate to Tauri via Rust command or global event.
    //
    // open_plugin_window   → Rust creates a new WebviewWindow (plugin-window.html)
    //                        and stores (title, html, window_id) in managed state.
    //                        plugin-window.js reads it via take_window_content.
    //
    // update_plugin_window → Global Tauri event "plugin:update" broadcast to all
    //                        windows; the target window filters by window_id.
    //
    // close_plugin_window  → Global Tauri event "plugin:close" broadcast; the
    //                        target window closes itself.
    case "open_plugin_window":
      // width/height sono opzionali (assenti per calc/ping/counter → undefined).
      openPluginWindow(msg.window_id, msg.title, msg.html, msg.width, msg.height);
      break;
    // ── save_routine — finestra di anteprima dedicata (Fase 2) ─────────────
    // Docs/superpowers/specs/2026-08-05-save-routine-design.md §5. A
    // differenza di tool_confirm_request (banner Sì/No nel cursore), questo
    // apre una finestra dedicata; la risposta è la STESSA
    // sendToolConfirmResponse (vedi setupRoutinePreviewEvents più sotto).
    case "routine_save_preview":
      openRoutinePreviewWindow(
        msg.id, msg.name, msg.description, msg.tags, msg.category, msg.script, msg.replace ?? null
      );
      break;
    case "update_plugin_window":
      // window_id must be sent as a JSON number matching the Rust u64.
      emitToPlugin("plugin:update", { window_id: msg.window_id, html: msg.html });
      break;
    case "close_plugin_window":
      emitToPlugin("plugin:close", { window_id: msg.window_id });
      break;
    case "ai_chat_message":
      // display_name/is_ai (Task 9): campi additivi arrivati dal wire (protocol
      // ChatLine/AiChatMessage) — vanno inoltrati, altrimenti addLine() in
      // aichat-window.js non ha modo di risolvere il nickname/badge per i messaggi
      // LIVE (lo storico, via ai_chat_history più sotto, passa l'intera entry e
      // non ha questo problema).
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
    case "share_request":
      pushAiChat("aichat:share-request", {
        share_id: msg.share_id,
        from_label: msg.from_label,
        doc_name: msg.doc_name,
        size_bytes: msg.size_bytes,
      });
      break;
    case "share_result":
      renderer.systemMessage(shareResultLine(msg.doc_name, msg.target_label, msg.outcome));
      break;
    // Slice 2a: l'orchestrator (MITTENTE) ha bisogno del contenuto vero di un
    // documento già accettato — lo leggiamo con lo stesso comando Tauri già usato
    // da Library per calcolare size_bytes (archive_open). Se fallisce (file
    // cancellato/spostato/permessi), il motivo tecnico non interessa alla UI:
    // mandiamo sempre la stessa frase fissa (vedi share-slice2a design §2).
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
    // finestre Markdown (archive_save). Se fallisce (disco pieno, permessi),
    // nessun ack e nessuna riga: l'entry lato orchestrator scade naturalmente
    // dopo 24h (design §4, silenzio verso il destinatario).
    case "share_incoming_data":
      invokeCmd("archive_save", { title: msg.title, content: msg.content })
        .then(() => {
          if (client) client.sendShareWritten(msg.share_id);
          renderer.systemMessage(shareReceivedLine(msg.title, msg.from_label));
        })
        .catch((e) => {
          console.error("[app] archive_save failed per documento ricevuto via Share:", e);
        });
      break;
    case "ai_chat_peer_lost":
      pushAiChat("aichat:peer_lost", { label: msg.label });
      break;
    // ── AI Chat — ammissione alla stanza (Task 10) ──────────────────────
    // Vedi Docs/superpowers/plans/2026-07-03-aichat-admission.md. Stesso
    // pattern buffer-and-replay di pushAiChat usato sopra per msg/roster/ecc.
    case "ai_chat_join_prompt":
      pushAiChat("aichat:join-prompt", { present: msg.present });
      break;
    case "ai_chat_admission_request":
      pushAiChat("aichat:admission-request", { candidate: msg.candidate });
      break;
    // FIX #7 (review 2026-07-03): un voto di ammissione si è concluso → la
    // finestra chat toglie il candidato dalla coda del gate 2 (banner "ammetti X?").
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

    // ── ServerMsg::Cwd — update the cwd display line above the prompt ─────
    // Received whenever the shell's working directory changes (on connect and
    // after every command).  The path is displayed via textContent (never
    // innerHTML) so no XSS risk even on adversarial path strings.
    case "cwd":
      currentCwd = msg.path;
      if (cwdLineEl) {
        cwdLineEl.textContent = formatCwd(msg.path, homeDir);
        cwdLineEl.classList.remove("hidden");
      }
      break;

    default:
      console.warn("[app] Unknown ServerMsg type:", msg.type);
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
    await invokeCmd("open_markdown_window", { title, content, kind, sourceFile: "" });
  } catch (e) {
    console.error("[app] open_markdown_window error:", e);
  }
}

// ---------------------------------------------------------------------------
// Ricerca file (Task 11/SR4) — finestra dedicata live
//
// Flusso:
//   - search_open → openSearchWindow(sid, title): Rust crea la WebviewWindow.
//   - search_hit/done → emitToSearch: evento globale Tauri; la finestra (che
//     conosce il proprio sid) filtra e rende.
//   - La finestra emette a sua volta:
//       "search:open-path" { path } → inviamo `/open <path>` al backend.
//       "search:cancel"    { sid }  → inviamo CancelSearch al backend.
//       "search:pause"     { sid }  → inviamo PauseSearch al backend.
//       "search:resume"    { sid }  → inviamo ResumeSearch al backend.
//     (vedi setupSearchEvents, registrato al bootstrap.)
// ---------------------------------------------------------------------------

/** Apre la finestra di ricerca live delegando a Rust. */
async function openSearchWindow(sid, title) {
  try {
    await invokeCmd("open_search_window", { title: title || "Ricerca", sid });
  } catch (e) {
    console.error("[app] open_search_window error:", e);
  }
}

/** Inoltra un evento alla finestra di ricerca (emit globale; la finestra filtra per sid). */
function emitToSearch(event, payload) {
  if (tauriEvent?.emit) {
    tauriEvent
      .emit(event, payload)
      .catch((e) => console.error(`[app] emit ${event} error:`, e));
  }
}

/**
 * Registra i listener per gli eventi emessi DALLA finestra di ricerca verso il main.
 * Idempotente nei fatti: chiamato una volta al bootstrap.
 */
function setupSearchEvents() {
  if (!tauriEvent?.listen) return;
  tauriEvent.listen("search:open-path", (e) => {
    const path = e?.payload?.path;
    if (path) handleSlashCommand(`/open ${path}`); // riusa il path slash → open_target
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
//
// Flow (inbound, orchestrator → UI):
//   open_plugin_window   → Rust creates WebviewWindow (plugin-window.html).
//   update_plugin_window → broadcast Tauri event "plugin:update"; target filters.
//   close_plugin_window  → broadcast Tauri event "plugin:close"; target closes.
//
// Flow (outbound, plugin window → orchestrator):
//   "plugin:ui-event"      → client.sendPluginUiEvent(window_id, element_id, value)
//   "plugin:window-closed" → client.sendPluginWindowClosed(window_id)
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
    // (title/html sono parole singole, invariate.)
    //
    // width/height sono OPZIONALI: per calc/ping/counter arrivano come `undefined`.
    // L'IPC di Tauri serializza l'oggetto args in JSON e le proprietà `undefined`
    // vengono omesse → lato Rust i parametri `Option<f64>` risultano assenti = None
    // → il comando ricade sul default 480×360. Nessun impatto sui plugin esistenti.
    await invokeCmd("open_plugin_window", { windowId: window_id, title, html, width, height });
  } catch (e) {
    console.error("[app] open_plugin_window error:", e);
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
    console.error("[app] open_routine_preview error:", e);
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
      .catch((e) => console.error(`[app] emitToPlugin ${event} error:`, e));
  }
}

// ── AI Chat (Slice 1a-ui-B) ────────────────────────────────────────────────
// La finestra-chat è una webview Tauri che NON apre una WS propria: parla con
// QUESTA connessione (app.js) via eventi Tauri, come le finestre plugin.
// Stato apertura/buffer della finestra-chat: vedi aichat-push.js. L'apertura è
// SOLO esplicita (comando `/aichat`, `aiChatGate.requestOpen()`) — un evento
// ai_chat_* che arriva senza che la finestra sia stata richiesta viene scartato,
// non apre più da solo la finestra (comportamento pre-esistente, indesiderato
// in uso normale: la chat si apriva ad ogni avvio di ui.exe).
const aiChatGate = createAiChatPushGate();

// Indicatore "attività AI Chat non vista" (spec 2026-07-28): in-memory,
// azzerato a ogni avvio di ui.exe — stesso trattamento del resto del gate.
let hasUnseenAiChatActivity = false;

function updateAiChatNotifyIndicator() {
  if (!aiChatNotifyEl) return;
  aiChatNotifyEl.classList.toggle("hidden", !hasUnseenAiChatActivity);
}

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
// Serve a escludere se stessi dall'elenco "Condividi con…" in Library — il
// roster AI Chat include SEMPRE anche la propria etichetta (confermato dal
// test server_roster_includes_own_human_label lato backend), e selezionare
// se stessi come destinatario fallisce sempre con un messaggio fuorviante
// ("macchina non trovata o non connessa" — in realtà è solo "sei tu").
let mySelfLabel = null;

// Ponte verso la finestra Library — stesso schema di emitToPlugin/emitToAiChat
// (emit globale Tauri; ogni finestra ascolta solo gli eventi che le competono).
function emitToLibrary(event, payload) {
  if (tauriEvent?.emit) {
    tauriEvent
      .emit(event, payload)
      .catch((e) => console.error(`[app] emitToLibrary ${event} error:`, e));
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
    console.error("[app] open_aichat_window error:", e);
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
    console.error("[app] open_note_window error:", e);
  }
}

/**
 * Listener per gli eventi emessi DALLA finestra nota verso il main.
 *
 * "note-window:save" — l'utente ha cliccato "Salva".
 *   `noteId: null` → creazione: payload `{title, text}`.
 *   `noteId` presente → modifica: payload `{editText?, editTitle?}` — solo i
 *   campi DAVVERO cambiati (decisione già presa in note-window.js via
 *   `noteEditMessages`, note-view.mjs). Un campo assente non genera alcun
 *   messaggio verso l'orchestrator.
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
      .catch((e) => console.error(`[app] emitToAiChat ${event} error:`, e));
  }
}

// Spinge un evento verso la finestra-chat. Se è già pronta, va diretto; se
// l'apertura è in corso (richiesta con `/aichat`), bufferizza (buffer-and-replay:
// evita di perdere il primo messaggio prima che la webview separata abbia
// registrato i suoi listener); altrimenti scarta — niente auto-apertura.
function pushAiChat(event, payload) {
  const { action } = aiChatGate.push(event, payload);
  if (action === "emit") {
    emitToAiChat(event, payload);
    return;
  }
  if (isNotifiableDrop(event, action)) {
    hasUnseenAiChatActivity = true;
    updateAiChatNotifyIndicator();
  }
}

// Listener per gli eventi emessi DALLA finestra-chat verso il main.
function setupAiChatEvents() {
  if (!tauriEvent?.listen) return;
  // La finestra è pronta → ri-registra il sink WS, rigioca il buffer, passa in live.
  // `sendAiChatOpen` comunica all'orchestrator che il webview è tornato vivo:
  // l'orchestrator ri-mette il `server_tx` e ri-emette il roster noto così i
  // "presenti" appaiono subito senza aspettare un nuovo messaggio di rete.
  tauriEvent.listen("aichat:ready", () => {
    if (client) client.sendAiChatOpen();
    for (const b of aiChatGate.ready()) emitToAiChat(b.event, b.payload);
    hasUnseenAiChatActivity = false;
    updateAiChatNotifyIndicator();
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
 * Called once at bootstrap; idempotent in practice.
 *
 * "plugin:ui-event"      — user clicked a [data-evt] element inside a plugin window.
 *   Payload: { window_id: number, element_id: string, value: string|null }
 *   → forwarded to orchestrator as ClientMsg::PluginUiEvent over WebSocket.
 *
 * "plugin:window-closed" — user clicked ✕ or the window was closed by the OS.
 *   Payload: { window_id: number }
 *   → forwarded as ClientMsg::PluginWindowClosed so the plugin can clean up.
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
 * ClientMsg::ToolConfirmResponse the cursor's Sì/No banner already sends
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
 * Registra il listener per l'evento back-channel della finestra /config.
 *
 * Quando l'utente salva da config.html, config-window.js emette `config:saved`
 * con il payload della config. Lo riceviamo qui e riapplichiamo aspetto e flag
 * al pannello principale — senza richiedere un reload.
 */
function setupConfigWindowEvents() {
  if (!tauriEvent?.listen) return;
  // La finestra /config notifica il salvataggio → riapplichiamo l'aspetto qui.
  tauriEvent.listen("config:saved", (ev) => applySavedConfig(ev.payload));
}

/**
 * Registra i listener per gli eventi emessi DALLA finestra library verso il main.
 *
 * `library:open-folder { path }` — emesso da library.js quando l'utente clicca
 * il pulsante "Apri cartella archivio" (📂).  Il main invia `/open <path>` al
 * backend che apre la cartella nel file manager del SO (stesso flusso di
 * `search:open-path`).
 *
 * Security: `path` proviene da Rust (`library_dir` command), non da input utente.
 * Passato a `handleSlashCommand` via template literal → textContent nel renderer
 * (mai innerHTML).
 */
function setupLibraryEvents() {
  if (!tauriEvent?.listen) return;
  tauriEvent.listen("library:open-folder", (e) => {
    const path = e?.payload?.path;
    if (path) handleSlashCommand(`/open ${path}`); // riusa il path slash → open_target
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
  // `payload.note` è `null`/assente per "Nuova nota". La creazione/modifica
  // vera e propria arriva poi da "note-window:save" (v. setupNoteWindowEvents)
  // quando l'utente clicca "Salva" nella nuova finestra separata — non da qui.
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
// Input: focus management
// ---------------------------------------------------------------------------
function grabFocus() {
  hiddenInput.focus();
}

grabFocus();
document.addEventListener("click", () => {
  resetIdleTimer(); // click nel pannello = attività
  grabFocus();
});
window.addEventListener("focus", grabFocus);

// ---------------------------------------------------------------------------
// Comandi cliccabili nella riga hint (sotto il prompt).
//
// I `.hint-cmd` (es. /config, /library, /help) sono link: un click esegue lo
// STESSO comando del digitato, riusando `handleSlashCommand` (zero logica nuova
// — /config apre il dialog, /library la libreria, gli altri vanno al backend).
// Listener delegato sulla riga `.hint`. Il global click handler riporta poi il
// focus all'hidden input, così l'utente può continuare a digitare.
// ---------------------------------------------------------------------------
const hintEl = document.querySelector(".hint");
if (hintEl) {
  hintEl.addEventListener("click", (e) => {
    const cmdEl = e.target.closest(".hint-cmd");
    if (cmdEl && cmdEl.dataset.cmd) handleSlashCommand(cmdEl.dataset.cmd);
  });
}

// ---------------------------------------------------------------------------
// Line-editor state
//
// A single { text, caret } object, managed by the pure lineEditor functions.
// Persists across hide/show (Esc does NOT clear it — only Ctrl+L and Enter do).
// ---------------------------------------------------------------------------
let editorState = { text: "", caret: 0 };

// ---------------------------------------------------------------------------
// Render: block cursor at caret
//
// The input row is rendered as three pieces:
//   [text before caret] [cursor block] [text after caret]
//
// The cursor block:
//   - Contains the char AT the caret if text.length > caret, otherwise empty.
//   - Styled with inverse colours (bg = cursor colour, fg = panel bg).
//   - The blink animation lives on the .cursor span (kept from original CSS).
//   - `white-space: pre` is set on all spans so spaces are preserved.
//
// We build spans with createElement/textContent — never innerHTML — so
// characters like '<' and '&' cannot inject markup.
// `white-space: pre-wrap` is set on all text spans so spaces are preserved
// and long lines wrap at the panel boundary instead of clipping.
// ---------------------------------------------------------------------------
function renderEditor() {
  const { text, caret } = editorState;

  // Build the three text pieces.
  const before  = text.slice(0, caret);
  const atCaret = text.slice(caret, caret + 1); // "" when caret === text.length
  const after   = text.slice(caret + 1);

  // Clear current children.
  typedTextEl.textContent = "";

  // Before-caret text span.
  if (before.length > 0) {
    const spanBefore = document.createElement("span");
    spanBefore.style.whiteSpace = "pre-wrap";
    spanBefore.textContent = before;
    typedTextEl.appendChild(spanBefore);
  }

  // Cursor block: the `.cursor` CSS class drives the blink-block animation
  // (inverse video ON ↔ OFF via @keyframes blink-block in index.html).
  // Do NOT set background/color inline here — CSS animations override inline
  // styles and the keyframes own those properties to keep the glyph visible
  // during both phases.
  const spanCursor = document.createElement("span");
  spanCursor.className = "cursor";
  // Char at caret (space if at end so the block has a visible minimum width).
  spanCursor.textContent = atCaret.length > 0 ? atCaret : " ";
  // At end-of-line (empty char): ensure the empty-space block has minimum width.
  if (atCaret.length === 0) {
    spanCursor.style.minWidth = "0.6em";
  }
  typedTextEl.appendChild(spanCursor);

  // After-caret text span.
  if (after.length > 0) {
    const spanAfter = document.createElement("span");
    spanAfter.style.whiteSpace = "pre-wrap";
    spanAfter.textContent = after;
    typedTextEl.appendChild(spanAfter);
  }
}

// Initialise the render (empty state = block at position 0).
renderEditor();

// ---------------------------------------------------------------------------
// Input: keyboard handler
// ---------------------------------------------------------------------------
hiddenInput.addEventListener("keydown", (e) => {
  resetIdleTimer(); // qualsiasi tasto nasconde il pulcino e azzera il countdown

  // Any key other than Tab/Shift+Tab ends an in-progress completion cycle —
  // must run before every other branch below, unconditionally, so no future
  // branch can forget to clear it. A physical Shift+Tab press fires as TWO
  // separate keydown events (Shift down, then Tab down with shiftKey=true)
  // — the bare "Shift" keydown must also be exempted, or it wipes the cycle
  // a moment before the "Tab" keydown that was meant to continue it,
  // breaking reverse cycling on every discrete Shift+Tab tap (found in
  // review, 2026-07-19).
  if (e.key !== "Tab" && e.key !== "Shift") {
    tabCompletion = null;
  }

  // ── Esc: hide overlay (text PRESERVED — only Ctrl+L wipes it) ──────────
  if (e.key === "Escape") {
    e.preventDefault();
    invokeCmd("hide_overlay").catch(console.error);
    return;
  }

  // ── Ctrl+L: explicit clear of input AND output ──────────────────────────
  if (e.key === "l" && e.ctrlKey) {
    e.preventDefault();
    editorState = lineEditor.clear(editorState);
    renderer.clear();
    renderEditor();
    return;
  }

  // ── Ctrl+T: clear (come Ctrl+L) E ripristina la finestra compatta + ri-centra ──
  // Reset completo: svuota input+output e riporta la finestra alla dimensione
  // di default. Ctrl+T (non Ctrl+R, riservato dalla WebView2 al "Refresh").
  if (e.key === "t" && e.ctrlKey) {
    e.preventDefault();
    editorState = lineEditor.clear(editorState);
    renderer.clear();
    renderEditor();
    resetWindow();
    return;
  }

  // ── Ctrl+C: copy current line to clipboard ──────────────────────────────
  if (e.key === "c" && e.ctrlKey) {
    e.preventDefault();
    clipboardWrite(editorState.text);
    return;
  }

  // ── Ctrl+V: paste from clipboard at caret ──────────────────────────────
  if (e.key === "v" && e.ctrlKey) {
    e.preventDefault();
    clipboardRead().then((text) => {
      if (text) {
        // Single-line input: collapse newlines/tabs to spaces so a multiline
        // paste doesn't break the one-row layout.
        const oneLine = text.replace(/[\r\n\t]+/g, " ");
        editorState = lineEditor.insert(editorState, oneLine);
        renderEditor();
      }
    });
    return;
  }

  // ── Tab / Shift+Tab: filename/path completion (argument position only) ──
  // Mirrors PowerShell's own Tab-completion (the shell this app drives via
  // mcp-server's persistent session, not bash): repeated Tab cycles FORWARD
  // through every matching candidate one at a time; Shift+Tab cycles
  // BACKWARD. Only completes ARGUMENT words (see `isArgumentPosition`) — the
  // command word itself (position 0) is left untouched, out of scope.
  if (e.key === "Tab") {
    e.preventDefault();

    // Continue an active cycle: same text/caret as right after our own last
    // insertion (guards against anything unexpected mutating editorState
    // between Tab presses without going through the reset above).
    if (
      tabCompletion &&
      tabCompletion.text === editorState.text &&
      tabCompletion.rangeEnd === editorState.caret
    ) {
      const { candidates, quoteChar, dirPart } = tabCompletion;
      const direction = e.shiftKey ? -1 : 1;
      const nextIndex = (tabCompletion.index + direction + candidates.length) % candidates.length;
      const candidate = candidates[nextIndex];
      const wrapped = pathUtils.buildCompletionText(dirPart, candidate, quoteChar);
      const result = pathUtils.replaceRange(
        editorState.text,
        tabCompletion.rangeStart,
        tabCompletion.rangeEnd,
        wrapped
      );
      editorState = result;
      tabCompletion = {
        rangeStart: tabCompletion.rangeStart,
        rangeEnd: tabCompletion.rangeStart + wrapped.length,
        candidates,
        quoteChar,
        dirPart,
        index: nextIndex,
        text: result.text,
      };
      renderEditor();
      return;
    }

    // Fresh completion: find the word at the caret, bail out silently if
    // it's the command word (position 0) rather than an argument.
    const { start } = pathUtils.wordBounds(editorState.text, editorState.caret);
    if (!pathUtils.isArgumentPosition(editorState.text, start)) {
      return;
    }
    const caretAtRequest = editorState.caret;
    const textAtRequest = editorState.text;
    const token = editorState.text.slice(start, caretAtRequest);
    const { dirPart, prefix, quoteChar } = pathUtils.splitPathToken(token);
    invokeCmd("list_path_completions", { cwd: currentCwd, dirPart, prefix })
      .then((candidates) => {
        if (!candidates || candidates.length === 0) return;
        // Bail if the user kept typing while this lookup was in flight.
        if (editorState.caret !== caretAtRequest || editorState.text !== textAtRequest) return;
        const candidate = candidates[0];
        const wrapped = pathUtils.buildCompletionText(dirPart, candidate, quoteChar);
        const result = pathUtils.replaceRange(editorState.text, start, caretAtRequest, wrapped);
        editorState = result;
        tabCompletion = {
          rangeStart: start,
          rangeEnd: start + wrapped.length,
          candidates,
          quoteChar,
          dirPart,
          index: 0,
          text: result.text,
        };
        renderEditor();
      })
      .catch(console.error);
    return;
  }

  // ── PageUp/PageDown: scroll the previous output (scrollback) ──────────
  // ↑/↓ are intentionally NOT used here — they are reserved for command history.
  if (e.key === "PageUp" || e.key === "PageDown") {
    e.preventDefault();
    const delta = pageDelta(outputArea.clientHeight);
    outputArea.scrollBy({ top: e.key === "PageUp" ? -delta : delta });
    return;
  }

  // ── Arrow keys: move caret ──────────────────────────────────────────────
  if (e.key === "ArrowLeft") {
    e.preventDefault();
    editorState = lineEditor.left(editorState);
    renderEditor();
    return;
  }

  if (e.key === "ArrowRight") {
    e.preventDefault();
    editorState = lineEditor.right(editorState);
    renderEditor();
    return;
  }

  // ── Home / End: jump to line start / end ───────────────────────────────
  if (e.key === "Home") {
    e.preventDefault();
    editorState = lineEditor.home(editorState);
    renderEditor();
    return;
  }

  if (e.key === "End") {
    e.preventDefault();
    editorState = lineEditor.end(editorState);
    renderEditor();
    return;
  }

  // ── Backspace ───────────────────────────────────────────────────────────
  if (e.key === "Backspace") {
    e.preventDefault();
    editorState = lineEditor.backspace(editorState);
    renderEditor();
    return;
  }

  // ── Delete (forward delete) ─────────────────────────────────────────────
  if (e.key === "Delete") {
    e.preventDefault();
    editorState = lineEditor.deleteForward(editorState);
    renderEditor();
    return;
  }

  // ── Enter: send command or dispatch slash ───────────────────────────────
  if (e.key === "Enter") {
    e.preventDefault();
    const input = editorState.text.trim();
    editorState = lineEditor.clear(editorState);
    renderEditor();

    if (!input) return;

    // Echo the just-submitted input immediately into the output area, so the
    // user sees what they typed (and that it was received) before any response
    // — the input line itself is cleared on Enter.
    renderer.echoCommand(input);

    // Slash commands: intercepted client-side, NOT sent to backend.
    if (input.startsWith("/")) {
      handleSlashCommand(input);
      return;
    }

    // Regular command: send to orchestrator via WebSocket.
    if (!client || !client.isConnected()) {
      const errId = uuidv4();
      renderer.appendChunk(errId, "[not connected — waiting for orchestrator]\n");
      renderer.markError(errId, "routing_error", "WebSocket not connected");
      return;
    }

    // Generate the id before sending so commandStarted and sendCommand share it.
    const cmdId = uuidv4();
    commandStarted(cmdId);
    client.sendCommand(input, cmdId, webSearchEnabled);
    return;
  }

  // ── Printable characters: insert at caret ──────────────────────────────
  if (e.key.length === 1 && !e.ctrlKey && !e.altKey && !e.metaKey) {
    editorState = lineEditor.insert(editorState, e.key);
    renderEditor();
  }
});

// ---------------------------------------------------------------------------
// Slash command dispatch (ADR-012)
// ---------------------------------------------------------------------------

/**
 * Handle slash commands.
 *
 * Slash commands are split into two categories (ADR-012):
 *   - UI-local:  `/config` → handled here (opens the configuration dialog).
 *   - Backend:   every other `/…` → forwarded to the orchestrator as a normal
 *                Command.  The backend's Route::Slash dispatch handles `/open`,
 *                `/reset`, and unknown slashes (which return an Error message).
 *
 * This removes the "sconosciuto" inline error: the backend is now responsible
 * for unknown slash responses, keeping the UI dumb and the routing logic
 * server-side.
 *
 * @param {string} input - Full input starting with '/'.
 */
function handleSlashCommand(input) {
  const cmd = input.split(/\s+/)[0].toLowerCase(); // "/config extra" → "/config"

  // UI-local: /config opens the dedicated config window (config.html) without
  // a backend round-trip. Rust handles the singleton pattern (focus if already open).
  if (cmd === "/config") {
    invokeCmd("open_config_window").catch((e) => {
      console.error("[app] open_config_window error:", e);
    });
    return;
  }

  // UI-local: /library opens the archive browser window without a backend round-trip.
  // Same pattern as /config: invoke a Rust command, no WebSocket send.
  if (cmd === "/library") {
    invokeCmd("open_library_window").catch((e) => {
      console.error("[app] open_library_window error:", e);
    });
    return;
  }

  // UI-local: /aichat apre la finestra-chat AI Chat (singleton; Rust focalizza se già
  // aperta). Unico punto d'ingresso: `requestOpen()` fa sì che gli eventi ai_chat_*
  // ri-emessi dal backend (AiChatOpen → SetServerTx) vengano bufferizzati invece
  // che scartati finché la finestra non è pronta.
  if (cmd === "/aichat") {
    aiChatGate.requestOpen();
    openAiChatWindow();
    return;
  }

  // UI-local: canale tool esterno (es. /nmap) — apre una finestra dedicata con
  // la propria connessione WS (Hello{channel}), non quella condivisa del cursore.
  // EXTERNAL_TOOL_CHANNELS ha ora la voce "nmap" (registrata nell'integrazione
  // mcp-nmap): questo branch è live in produzione per /nmap.
  const extChannel = findExternalChannelBySlash(cmd, EXTERNAL_TOOL_CHANNELS);
  if (extChannel) {
    invokeCmd("open_external_channel_window", {
      channelId: extChannel.id,
      windowTitle: extChannel.windowTitle,
    }).catch((e) => {
      console.error("[app] open_external_channel_window error:", e);
    });
    return;
  }

  // Backend slash: forward to orchestrator as a normal Command.
  // The not-connected guard is duplicated here so the user gets feedback
  // even before the WS is established.
  if (!client || !client.isConnected()) {
    const errId = uuidv4();
    renderer.appendChunk(errId, "[not connected — waiting for orchestrator]\n");
    renderer.markError(errId, "routing_error", "WebSocket not connected");
    return;
  }

  // Generate the id before sending so commandStarted and sendCommand share it.
  const slashId = uuidv4();
  commandStarted(slashId);
  client.sendCommand(input, slashId, webSearchEnabled);
}

// ---------------------------------------------------------------------------
// Bootstrap: load config → fetch home dir → apply appearance → connect WS
// ---------------------------------------------------------------------------
async function bootstrap() {
  // Apply appearance from persisted config (before WS, so no flash).
  if (tauriInvoke) {
    try {
      const cfg = await invokeCmd("get_config");
      if (cfg) {
        applyAppearance(cfg);
        // Apply activity indicator style from saved config (default: "title").
        setActivityStyle(cfg.activity_indicator);
        // Initialise web search flag from persisted config (default: true).
        webSearchEnabled = cfg.web_search_enabled !== false;
      }
    } catch (e) {
      console.warn("[app] get_config on startup failed:", e);
    }

    // Fetch the home directory once so formatCwd() can substitute ~.
    // Falls back to "" (no substitution) if the command is unavailable.
    try {
      homeDir = (await invokeCmd("home_dir")) ?? "";
    } catch (e) {
      console.warn("[app] home_dir on startup failed:", e);
      homeDir = "";
    }
  }

  // Then connect WS.
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
}

bootstrap();
