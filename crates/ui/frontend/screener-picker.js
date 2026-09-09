// screener-picker.js — runtime della finestra picker screener (Tauri
// webview separata, label "screener-picker-<ts>-<ctr>"). Riceve gli item
// via take_window_content (stesso meccanismo generico di window.js/
// plugin-window.js — main.rs: open_screener_picker_window), rimanda la
// scelta alla finestra /markets che l'ha aperta via evento Tauri GLOBALE
// filtrato per label — questo codebase non usa MAI emitTo (window-targeted
// emit), sempre broadcast + filtro lato client, stesso pattern di
// plugin-window.js (filtro su window_id).
//
// Back-channel (questa finestra → finestra /markets):
//   tauriEvent.emit("screener:picked", { opener_label, id, title })
// external-channel-window.js lo riceve e lo filtra per opener_label ===
// la propria label.

import { createPickerState, moveSelection, selectedItem } from "./screener-picker-list.mjs";
import { fetchI18n, applyI18n, t } from "./i18n.mjs";

const { invoke } = window.__TAURI__.core;
const tauriEvent = window.__TAURI__.event;

const listArea = document.getElementById("list-area");
const closeBtn = document.getElementById("close-btn");

let state = createPickerState([]);
let openerLabel = "";

function render() {
  listArea.innerHTML = "";
  if (state.items.length === 0) {
    const empty = document.createElement("div");
    empty.id = "empty-state";
    empty.textContent = t("screener_picker.empty");
    listArea.appendChild(empty);
    return;
  }
  state.items.forEach((item, i) => {
    const row = document.createElement("div");
    row.className = "screener-item" + (i === state.index ? " selected" : "");
    row.dataset.index = String(i);
    const title = document.createElement("div");
    title.className = "item-title";
    title.textContent = item.title;
    const desc = document.createElement("div");
    desc.className = "item-description";
    desc.textContent = item.description;
    row.appendChild(title);
    row.appendChild(desc);
    // Convenzione Windows classica (richiesta esplicita utente dopo live
    // test 2026-08-11): 1 click SELEZIONA soltanto (come l'hover già
    // faceva), doppio click CONFERMA (come Invio). Prima il click singolo
    // confermava subito — comportamento diverso da hover/Invio e causa di
    // un report utente ("click e doppio click non fanno nulla, solo Invio
    // conferma"): con un solo screener in lista la riga risultava già
    // selezionata di default, quindi un click "che seleziona soltanto"
    // sembrava non fare nulla — ipotesi più probabile della causa del
    // report rispetto a un problema di consegna degli eventi DOM.
    row.addEventListener("click", () => {
      state = { items: state.items, index: i };
      render();
    });
    row.addEventListener("dblclick", () => {
      state = { items: state.items, index: i };
      confirmSelection();
    });
    row.addEventListener("mouseenter", () => {
      state = { items: state.items, index: i };
      render();
    });
    listArea.appendChild(row);
  });
}

function confirmSelection() {
  const item = selectedItem(state);
  if (!item) return; // stato vuoto: Invio/click non fanno nulla
  tauriEvent.emit("screener:picked", { opener_label: openerLabel, id: item.id, title: item.title });
  invoke("close_self").catch((e) => console.error("[screener-picker] close_self error:", e));
}

document.addEventListener("keydown", (ev) => {
  if (ev.key === "ArrowDown") {
    state = moveSelection(state, 1);
    render();
  } else if (ev.key === "ArrowUp") {
    state = moveSelection(state, -1);
    render();
  } else if (ev.key === "Enter") {
    confirmSelection();
  } else if (ev.key === "Escape") {
    // Nessun evento emesso: la conversazione /markets resta invariata,
    // come da requisito esplicito.
    invoke("close_self").catch((e) => console.error("[screener-picker] close_self error:", e));
  }
});

closeBtn.addEventListener("click", () => invoke("close_self"));

async function init() {
  try {
    const cfg = await invoke("get_config");
    if (cfg && typeof cfg.window_alpha === "number") {
      document.documentElement.style.setProperty("--window-alpha", cfg.window_alpha);
    }
    await fetchI18n(invoke, cfg?.language || "it");
    applyI18n(document);
  } catch (e) {
    console.warn("[screener-picker] startup i18n/config failed:", e);
  }

  const data = await invoke("take_window_content");
  if (!data) {
    console.error("[screener-picker] take_window_content returned null");
    return;
  }
  openerLabel = data.source_file; // riuso deliberato dello slot, vedi main.rs
  let items = [];
  try {
    items = JSON.parse(data.content);
  } catch (e) {
    console.error("[screener-picker] items JSON non valido:", e);
  }
  state = createPickerState(items);
  render();
}

tauriEvent.listen("config:saved", async (ev) => {
  const alpha = ev.payload?.window_alpha;
  if (typeof alpha === "number") {
    document.documentElement.style.setProperty("--window-alpha", alpha);
  }
  const lang = ev.payload?.language;
  if (lang) {
    await fetchI18n(invoke, lang);
    applyI18n(document);
    render();
  }
});

init();
