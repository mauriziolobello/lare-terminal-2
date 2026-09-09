// external-channel-window.js — runtime della finestra generica "canale
// esterno" (Docs/superpowers/specs/2026-07-16-external-tool-channel-design.md §4).
//
// PRIMA finestra secondaria che apre la propria connessione WS (Hello{channel})
// invece di parlare col backend solo via IPC Tauri come config/library/plugin-*.
// `window.__LARE_EXT_CHANNEL_ID__`/`window.__LARE_EXT_CHANNEL_TITLE__` sono
// iniettati da Rust (initialization_script, vedi open_external_channel_window
// in main.rs) prima che questo modulo carichi.

import { LareRenderer } from "./renderer.js";
import { LareWsClient } from "./ws-client.js";
import { fetchI18n, applyI18n, t } from "./i18n.mjs";

const { invoke } = window.__TAURI__.core;
const tauriEvent = window.__TAURI__.event;
const { getCurrentWindow } = window.__TAURI__.window;

const channelId = window.__LARE_EXT_CHANNEL_ID__;
const channelTitle = window.__LARE_EXT_CHANNEL_TITLE__;

const outputEl = document.getElementById("output-area");
const statusEl = document.getElementById("status");
const inputEl = document.getElementById("input-box");
const closeBtn = document.getElementById("close-btn");
const titlebarLabelEl = document.getElementById("titlebar-label");
const activityEl = document.getElementById("activity-indicator");

// `decorations(false)`: nessuna titlebar nativa del SO — l'unico titolo
// visibile all'utente è questo span, hardcoded "Canale esterno" in HTML per
// avere un fallback ragionevole prima che questo script carichi. Lo
// sostituiamo subito col titolo reale del canale (es. "Lare — nmap").
if (titlebarLabelEl && channelTitle) {
  titlebarLabelEl.textContent = channelTitle;
}

const renderer = new LareRenderer(outputEl, statusEl);

let client = null;
let currentLanguage = "it";

// ── Activity indicator ──────────────────────────────────────────────────
// Un tool su questo canale (es. una scansione nmap) può impiegare fino a
// 900s (vedi NMAP_CALL_TIMEOUT_SECS in nmap_tool_client.rs) prima di
// rispondere: senza un indicatore visivo la finestra sembra bloccata.
// Versione ridotta dello spinner "status" del cursore v1 (overlay F2,
// rimosso in 2.0 — host.js non ne ha uno) — un solo stile fisso,
// nessun watchdog (il timeout vero è lato server, questa finestra non ha
// un pulsante di stop da mostrare/nascondere).
const SPINNER_FRAMES = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const SPINNER_INTERVAL_MS = 90;
const inFlight = new Set();
let spinnerTimer = null;
let frameIndex = 0;

function tickSpinner() {
  const frame = SPINNER_FRAMES[frameIndex % SPINNER_FRAMES.length];
  frameIndex++;
  if (activityEl) activityEl.textContent = t("ext_channel.processing", { frame });
}

function commandStarted(id) {
  inFlight.add(id);
  if (spinnerTimer === null) {
    frameIndex = 0;
    tickSpinner();
    spinnerTimer = setInterval(tickSpinner, SPINNER_INTERVAL_MS);
  }
}

function commandEnded(id) {
  inFlight.delete(id);
  if (inFlight.size === 0 && spinnerTimer !== null) {
    clearInterval(spinnerTimer);
    spinnerTimer = null;
    if (activityEl) activityEl.textContent = "";
  }
}

function handleServerMsg(msg) {
  switch (msg.type) {
    case "server_info":
      console.info("[external-channel-window]", channelId, "server_info:", msg.version, msg.ai_provider);
      break;
    case "chunk":
      renderer.appendChunk(msg.id, msg.content);
      break;
    case "done":
      renderer.markDone(msg.id, msg.exit_code ?? null);
      commandEnded(msg.id);
      break;
    case "error":
      renderer.markError(msg.id, msg.code, msg.message);
      commandEnded(msg.id);
      break;
    case "tool_confirm_request":
      renderer.confirmBanner(msg.id, msg.commands, (accept) => {
        if (client) client.sendToolConfirmResponse(msg.id, accept);
      });
      break;
    case "pong":
      break;
    case "cwd":
      // Ogni connessione (anche di canale) riceve la cwd tracciata all'handshake
      // e a ogni comando che la cambia; questa finestra non ha una riga cwd da
      // aggiornare (nessun editing di shell qui), quindi no-op deliberato.
      break;
    case "open_window":
      // Apertura finestra deterministica (mai una scelta dell'AI). Il
      // salvataggio in Library resta a scelta dell'utente tramite il
      // pulsante "Salva" già presente in ogni finestra Markdown (window.js)
      // — niente save_to_library automatico qui, produceva doppioni in
      // Library quando l'utente cliccava anche quel pulsante.
      invoke("open_markdown_window", { title: msg.title, content: msg.content, kind: msg.kind, sourceFile: "" }).catch((e) => {
        console.error("[external-channel-window] open_markdown_window error:", e);
      });
      break;
    case "open_screener_picker":
      // Apertura picker deterministica (mai una scelta dell'utente sulla
      // FORMA — solo su QUALE screener, dentro il picker). Docs/superpowers/
      // specs/2026-08-11-markets-screener-registry-design.md.
      invoke("open_screener_picker_window", { itemsJson: JSON.stringify(msg.items) }).catch((e) => {
        console.error("[external-channel-window] open_screener_picker_window error:", e);
      });
      break;
    default:
      console.warn("[external-channel-window] unhandled ServerMsg type:", msg.type);
  }
}

async function init() {
  try {
    const cfg = await invoke("get_config");
    if (cfg && typeof cfg.window_alpha === "number") {
      document.documentElement.style.setProperty("--window-alpha", cfg.window_alpha);
    }
    if (cfg?.language) {
      currentLanguage = cfg.language;
    }
    await fetchI18n(invoke, cfg?.language || "it");
    applyI18n(document);
  } catch (e) {
    console.warn("[external-channel-window] startup i18n/config failed:", e);
  }

  const token = (await invoke("get_lare_token")) ?? "";
  // Porta WS da startup.json (2.0), letta una volta all'apertura della
  // finestra — stesso comando usato da host.js.
  const url = (await invoke("get_ws_endpoint")) ?? "";
  client = new LareWsClient({
    url,
    token,
    channel: channelId,
    lang: currentLanguage,
    onStatus: (s) => renderer.setStatus(s),
    onMessage: handleServerMsg,
  });
  client.connect();
}

tauriEvent.listen("config:saved", async (ev) => {
  const alpha = ev.payload?.window_alpha;
  if (typeof alpha === "number") {
    document.documentElement.style.setProperty("--window-alpha", alpha);
  }
  const lang = ev.payload?.language;
  if (lang) {
    currentLanguage = lang;
    if (client) client.setLanguage(lang);
    await fetchI18n(invoke, lang);
    applyI18n(document);
  }
});

// Riusata sia dall'Enter sull'input-box sia dal listener "screener:picked"
// (picker) — stessa semantica: come se l'utente avesse scritto `text` a
// mano e premuto Invio.
function submitCommand(text) {
  if (!text || !client) return;
  renderer.echoCommand(text);
  const id = crypto.randomUUID();
  commandStarted(id);
  client.sendCommand(text, id, false, currentLanguage);
}

inputEl.addEventListener("keydown", (ev) => {
  if (ev.key !== "Enter") return;
  const text = inputEl.value.trim();
  submitCommand(text);
  inputEl.value = "";
});

// Back-channel dal picker (screener-picker.js): evento GLOBALE (questo
// codebase non usa mai emitTo — sempre broadcast + filtro lato client,
// stesso pattern di plugin-window.js), filtrato per la PROPRIA label così
// una selezione fatta per un'altra finestra /markets (se mai più di una
// fosse aperta) non arriva qui.
tauriEvent.listen("screener:picked", (event) => {
  const myLabel = getCurrentWindow()?.label;
  if (event.payload.opener_label !== myLabel) return;
  submitCommand(`Esegui lo screener ${event.payload.title}`);
});

closeBtn.addEventListener("click", () => invoke("close_self"));

init();
