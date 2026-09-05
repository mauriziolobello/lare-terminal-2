// config-window.js — runtime della finestra dedicata /config.
// Ospita la stessa ConfigDialog usata prima nel pannello principale, ma in una
// finestra a sé. Al salvataggio emette "config:saved" (back-channel Tauri
// globale: nel cursore v1 il pannello principale lo ascoltava e riapplicava
// l'aspetto — host.js registra lo stesso listener ma non fa nulla, non ha
// un aspetto da riapplicare: v. setupConfigWindowEvents in host.js). Annulla/
// Esc/× e il post-save chiudono la finestra via close_self.
const { invoke } = window.__TAURI__.core;
const tauriEvent = window.__TAURI__.event;

import { ConfigDialog } from "./config-dialog.js";

const root     = document.getElementById("config-root");
const closeBtn = document.getElementById("close-btn");

function closeSelf() { invoke("close_self"); }

const dlg = new ConfigDialog(
  root,
  invoke,
  (savedCfg) => {
    // Back-channel: notifica il pannello principale di riapplicare l'aspetto.
    tauriEvent.emit("config:saved", savedCfg);
  },
  closeSelf,  // onClose → chiudi la finestra (Cancel/Esc/dopo-save)
);

// ── Trasparenza configurabile (--window-alpha) ─────────────────────────────
// Questa finestra è l'EMITTER di "config:saved" (vedi callback onSaved sopra) e
// si chiude subito dopo ogni salvataggio, quindi non le serve un listener per
// l'evento: basta applicare il valore corrente una volta, all'avvio.
(async () => {
  try {
    const cfg = await invoke("get_config");
    if (cfg && typeof cfg.window_alpha === "number") {
      document.documentElement.style.setProperty("--window-alpha", cfg.window_alpha);
    }
  } catch (e) {
    console.warn("[config-window] get_config on startup failed:", e);
  }
})();

closeBtn.addEventListener("click", () => closeSelf());

dlg.open();
