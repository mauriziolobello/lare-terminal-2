// routine-preview.js — finestra di anteprima dedicata per save_routine
// (Fase 2 — Docs/superpowers/specs/2026-08-05-save-routine-design.md §5).
//
// Flusso:
//   1. Al load, legge (title, content=JSON, kind="routine_preview") da
//      WindowContentStore via take_window_content (stesso meccanismo di
//      window.js/plugin-window.js — content porta un JSON string, non HTML
//      o Markdown, per questo tipo di finestra).
//   2. Rende i campi con textContent (MAI innerHTML): lo script arriva
//      dall'AI, textContent è intrinsecamente sicuro, nessuna libreria di
//      sanitizzazione necessaria per questa finestra.
//   3. Salva/Annulla emettono "routine-preview:decision" sul bus eventi
//      globale di Tauri — host.js lo inoltra come la STESSA
//      ClientMsg::ToolConfirmResponse che il banner Sì/No del cursore manda
//      già oggi (vedi setupRoutinePreviewEvents in host.js).
//   4. Chiudere la finestra senza cliccare un pulsante (✕, Esc, Alt-F4) è
//      trattato come Annulla — emette decision(false) prima di chiudersi.

const invoke = window.__TAURI__?.core?.invoke;
const tauriEvent = window.__TAURI__?.event;

let routineId = null;
let decided = false; // evita una doppia emissione (click poi anche il close handler)

async function emitDecision(accept) {
  if (decided || routineId === null) return;
  decided = true;
  // `await` PRIMA di chiudere: close_self tronca il canale IPC di questa
  // webview, quindi un emit fire-and-forget avviato subito prima della
  // chiusura rischia di perdere la corsa e non arrivare mai ad host.js —
  // stesso bug osservato dal vivo su AI Chat (2026-07-29, v. closeWindow in
  // aichat-window.js) e già corretto con lo stesso pattern in
  // note-window.js (saveBtn handler). Qui la posta in gioco è più alta:
  // senza l'await, un Salva "perso" fa scadere la conferma lato orchestrator
  // dopo 180s e la routine NON viene salvata pur avendo l'utente cliccato
  // Salva.
  await tauriEvent?.emit("routine-preview:decision", { id: routineId, accept });
}

function closeSelf() {
  invoke?.("close_self").catch((e) => console.error("[routine-preview] close_self error:", e));
}

function render(data) {
  document.getElementById("meta-name").textContent = data.name;
  document.getElementById("meta-description").textContent = data.description;
  document.getElementById("meta-category").textContent = data.category;
  document.getElementById("meta-tags").textContent = (data.tags || []).join(", ");
  document.getElementById("script").textContent = data.script;

  const banner = document.getElementById("replace-banner");
  if (data.replace) {
    banner.textContent = `Sostituisce/aggiorna: ${data.replace}`;
    banner.style.display = "block";
  }
}

(async () => {
  if (!invoke) return;
  try {
    const stored = await invoke("take_window_content");
    if (!stored || stored.kind !== "routine_preview") return;
    const data = JSON.parse(stored.content);
    routineId = data.id;
    render(data);
  } catch (e) {
    console.error("[routine-preview] take_window_content error:", e);
  }
})();

document.getElementById("save-action").addEventListener("click", async () => {
  await emitDecision(true);
  closeSelf();
});

document.getElementById("cancel-action").addEventListener("click", async () => {
  await emitDecision(false);
  closeSelf();
});

document.getElementById("close-btn").addEventListener("click", async () => {
  await emitDecision(false);
  closeSelf();
});

document.addEventListener("keydown", async (e) => {
  if (e.key === "Escape") {
    e.preventDefault();
    await emitDecision(false);
    closeSelf();
  }
});

// Backstop per chiusura "per altre vie" (Alt-F4, task manager, chiusura da
// barra) che bypassano sia il pulsante ✕ sia Escape: senza questo, la
// conferma pendente lato orchestrator scade dopo 180s prima di essere
// trattata come Annullato. NON await qui: `beforeunload` non garantisce che
// una promise completi prima che la pagina scarichi — best-effort fire, come
// fa `aichat-window.js` per il suo stesso backstop ("aichat:closed" emesso
// senza await nel listener beforeunload).
window.addEventListener("beforeunload", () => {
  emitDecision(false);
});

// ── Trasparenza configurabile (--window-alpha) ─────────────────────────────
// Stesso boilerplate di ogni altra finestra secondaria (aichat-window.js,
// note-window.js, plugin-window.js): applica all'avvio e ad ogni salvataggio
// di /config. `routine-preview.html` referenzia già `--window-alpha` nel suo
// `:root` (`--bg: rgba(15,15,22, var(--window-alpha, 0.87))`) ma prima di
// questo fix nulla lo impostava mai — la finestra restava bloccata al
// default 0.87, ignorando lo slider di trasparenza dell'utente.
(async () => {
  if (!invoke) return;
  try {
    const cfg = await invoke("get_config");
    if (cfg && typeof cfg.window_alpha === "number") {
      document.documentElement.style.setProperty("--window-alpha", cfg.window_alpha);
    }
  } catch (e) {
    console.warn("[routine-preview] get_config on startup failed:", e);
  }
})();

tauriEvent?.listen("config:saved", (ev) => {
  const alpha = ev.payload?.window_alpha;
  if (typeof alpha === "number") {
    document.documentElement.style.setProperty("--window-alpha", alpha);
  }
});
