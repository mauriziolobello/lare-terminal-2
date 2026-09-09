// Cablaggio della finestra terminale (piano 3): xterm.js ↔ pty (Rust,
// portable-pty/ConPTY) ↔ lare-shell.exe. Adattato dallo spike 2
// (spikes/lare-terminal-window/frontend/app.js) alle convenzioni del
// prodotto: session id dal backend (get_terminal_session), OSC/base64/
// debounce/segnalini nei moduli puri testati da soli (Task 1),
// ActivityIndicator dal WS orchestratore via host.js (Task 5).

import { base64ToUint8Array } from "./base64.mjs";
import { parseLareOsc } from "./osc-lare.mjs";
import { createIndicatorState, onIntercept, onActivity } from "./indicators.mjs";
import { createDebouncer } from "./fit-debounce.mjs";
import { fetchI18n, applyI18n, t } from "./i18n.mjs";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

// Palette Campbell (Windows Terminal, profilo PowerShell) — stessa dello spike 2.
const campbellTheme = {
  background: "#0C0C0C", foreground: "#CCCCCC", cursor: "#FFFFFF",
  black: "#0C0C0C", red: "#C50F1F", green: "#13A10E", yellow: "#C19C00",
  blue: "#0037DA", magenta: "#881798", cyan: "#3A96DD", white: "#CCCCCC",
  brightBlack: "#767676", brightRed: "#E74856", brightGreen: "#16C60C",
  brightYellow: "#F9F1A5", brightBlue: "#3B78FF", brightMagenta: "#B4009E",
  brightCyan: "#61D6D6", brightWhite: "#F2F2F2",
};

const term = new Terminal({
  cursorBlink: true,
  scrollback: 5000,
  theme: campbellTheme,
  fontFamily: "Cascadia Mono, Consolas, monospace",
  fontSize: 14,
  allowProposedApi: true,
});
const fitAddon = new FitAddon.FitAddon();
term.loadAddon(fitAddon);
term.open(document.getElementById("terminal"));
fitAddon.fit();
term.focus();

let indicatorState = createIndicatorState();
const lastCommandEl = document.getElementById("last-command");
const dotEl = document.getElementById("activity-dot");
let dotOffTimer = null;

function renderIndicators() {
  const cmd = indicatorState.lastCommand ?? "—";
  lastCommandEl.textContent = t("terminal.last_command", { command: cmd });
  dotEl.classList.toggle("on", indicatorState.aiBusy);
}

// OSC 9001 (canale diretto host→emulatore, spec §4.7): accende il pallino
// per 2s indipendentemente da ActivityIndicator, che segue l'intero turno
// /ai (Task 5), non il singolo comando intercettato.
term.parser.registerOscHandler(9001, (data) => {
  const parsed = parseLareOsc(data);
  if (parsed) {
    indicatorState = onIntercept(indicatorState, parsed.line);
    renderIndicators();
    clearTimeout(dotOffTimer);
    dotOffTimer = setTimeout(() => dotEl.classList.remove("on"), 2000);
  }
  return true;
});

let ownSessionId = null;

// ActivityIndicator (Task 5): segue l'intero turno /ai, non solo l'OSC.
listen("terminal:activity", (event) => {
  if (!ownSessionId) return;
  indicatorState = onActivity(indicatorState, event.payload.session_id, ownSessionId, event.payload.on);
  renderIndicators();
});

const clockEl = document.getElementById("clock");
function tickClock() {
  const now = new Date();
  const pad = (n) => String(n).padStart(2, "0");
  clockEl.textContent = `${pad(now.getHours())}:${pad(now.getMinutes())}:${pad(now.getSeconds())}`;
}
tickClock();
setInterval(tickClock, 1000);

// Pulsanti /help /library /aichat /config: scrivono direttamente nella pty
// (come se l'utente li avesse digitati) — nessun comando Tauri dedicato.
document.querySelectorAll(".slash-btn").forEach((btn) => {
  btn.addEventListener("click", () => {
    invoke("pty_write", { data: btn.dataset.cmd + "\r" }).catch((err) => console.error("pty_write:", err));
    term.focus();
  });
});

const restartBanner = document.getElementById("restart-banner");
const restartMessage = document.getElementById("restart-message");
const restartBtn = document.getElementById("restart-btn");

async function spawnShell() {
  restartBanner.hidden = true;
  try {
    await invoke("pty_spawn", { sessionId: ownSessionId, cols: term.cols, rows: term.rows });
  } catch (err) {
    term.write("\r\n\x1b[31m" + t("terminal.pty_spawn_failed", { error: String(err) }) + "\x1b[0m\r\n");
  }
}

restartBtn.addEventListener("click", () => {
  term.reset();
  spawnShell();
});

async function main() {
  try {
    const cfg = await invoke("get_config");
    await fetchI18n(invoke, cfg?.language);
  } catch (e) {
    console.warn("[terminal] get_config failed:", e);
    await fetchI18n(invoke);
  }
  applyI18n(document);

  ownSessionId = await invoke("get_terminal_session");
  document.getElementById("session-label").textContent = t("terminal.session_label", { session: ownSessionId });

  await listen("pty-out", (event) => term.write(base64ToUint8Array(event.payload)));
  await listen("pty-exit", (event) => {
    term.write("\r\n\x1b[31m" + t("terminal.shell_terminated", { code: event.payload }) + "\x1b[0m\r\n");
    restartMessage.textContent = t("terminal.restart_message", { code: event.payload });
    restartBanner.hidden = false;
  });

  listen("config:saved", async (ev) => {
    if (ev.payload?.language) {
      await fetchI18n(invoke, ev.payload.language);
      applyI18n(document);
      renderIndicators();
      if (ownSessionId) {
        document.getElementById("session-label").textContent = t("terminal.session_label", { session: ownSessionId });
      }
    }
  });

  await spawnShell();

  term.onData((data) => {
    invoke("pty_write", { data }).catch((err) => console.error("pty_write:", err));
  });

  const debounced = createDebouncer(50);
  const resizeObserver = new ResizeObserver(() => {
    debounced(() => {
      fitAddon.fit();
      invoke("pty_resize", { cols: term.cols, rows: term.rows }).catch((err) => console.error("pty_resize:", err));
    });
  });
  resizeObserver.observe(document.getElementById("terminal"));
}

main();
