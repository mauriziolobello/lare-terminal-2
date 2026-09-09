// note-window.js — finestra "Nuova nota"/"Modifica nota" del Blocco note
// (webview Tauri separato, label fissa "note-compose").
//
// Sostituisce il vecchio <dialog> HTML dentro library.html (Task 19): primo
// smoke test dal vivo multi-macchina (2026-07-31, finding #3) l'ha trovato
// inamovibile e ridimensionabile solo dall'angolo della textarea, essendo una
// finestra CHILD del webview Library. Una WebviewWindow indipendente risolve
// entrambi per costruzione — v. main.rs `open_note_window`.
//
// NON apre una WS propria: parla con l'orchestrator SOLO via eventi Tauri
// verso/da host.js (connessione WS primaria) — stesso disaccoppiamento delle
// finestre AI Chat/plugin. La decisione "cosa mandare al salvataggio" è
// delegata a `noteEditMessages` (note-view.mjs, testata con node:test):
// unica logica pura del vecchio dialog, isolata perché un fix reale
// (smoke test dal vivo) l'aveva appena toccata.
import { isTitleValid, isBodyValid, noteEditMessages } from "./note-view.mjs";
import { fetchI18n, applyI18n, t } from "./i18n.mjs";

const tauriEvent = window.__TAURI__?.event;
const invoke = window.__TAURI__?.core?.invoke;

const titlebarTextEl = document.getElementById("titlebar-text");
const titleInput = document.getElementById("note-title-input");
const textInput  = document.getElementById("note-text-input");
const errorEl    = document.getElementById("note-error");
const saveBtn    = document.getElementById("note-save");
const cancelBtn  = document.getElementById("note-cancel");

// Stato della nota attualmente caricata. `noteId === null` → "Nuova nota".
// `originalTitle`/`originalBodyText` sono i valori con cui il form è stato
// precompilato (o "" per una nota nuova) — servono a `noteEditMessages` per
// decidere cosa è DAVVERO cambiato al salvataggio.
let noteId = null;
let originalTitle = "";
let originalBodyText = "";

// loadNote({noteId, title, text}) — (ri)popola il form. Usata sia al primo
// avvio (via take_window_content, sotto) sia su una riapertura mentre la
// finestra è già viva (evento "note-window:load" — v. main.rs
// open_note_window: la finestra singleton non ripassa da take_window_content
// una seconda volta, quel canale è "one-shot" per design).
function loadNote({ noteId: id, title, text }) {
  noteId = id || null;
  originalTitle = title ?? "";
  originalBodyText = text ?? "";
  titlebarTextEl.textContent = noteId ? t("note.edit_title") : t("note.new_title");
  titleInput.value = originalTitle;
  textInput.value = originalBodyText;
  errorEl.style.display = "none";
  titleInput.focus();
}

tauriEvent?.listen("note-window:load", (e) => loadNote(e.payload ?? {}));

function emit(event, payload) {
  return tauriEvent?.emit(event, payload);
}

async function closeWindow() {
  invoke?.("close_self").catch(() => {});
}

saveBtn.addEventListener("click", async () => {
  const title = titleInput.value;
  const text = textInput.value;

  if (!isTitleValid(title)) {
    errorEl.textContent = t("note.error_title_invalid");
    errorEl.style.display = "block";
    return;
  }
  if (!isBodyValid(text)) {
    errorEl.textContent = t("note.error_body_too_long");
    errorEl.style.display = "block";
    return;
  }

  // `await emit` PRIMA di chiudere: close_self tronca il canale IPC di
  // questa webview, quindi un emit avviato subito prima della chiusura
  // rischia di non arrivare mai (stesso ordine di aichat-window.js
  // closeWindow — bug osservato dal vivo su AI Chat, 2026-07-29).
  if (noteId) {
    const { editText, editTitle } = noteEditMessages({
      title,
      text,
      originalTitle,
      originalBodyText,
    });
    // Né l'uno né l'altro cambiato → comunque emesso: host.js non manda nulla
    // all'orchestrator se entrambi i campi sono `undefined` (v. listener
    // "note-window:save" in host.js), "Salva" si comporta come "Chiudi".
    await emit("note-window:save", { noteId, editText, editTitle });
  } else {
    await emit("note-window:save", { noteId: null, title, text });
  }
  await closeWindow();
});

cancelBtn.addEventListener("click", closeWindow);
document.getElementById("close-btn").addEventListener("click", closeWindow);

// Escape chiude la finestra. A differenza del vecchio <dialog> dentro
// library.html (dove Escape andava intercettato con stopPropagation per non
// chiudere l'intera finestra Library sotto di lui), qui non c'è alcuna
// finestra padre da proteggere: è il comportamento diretto e giusto.
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") closeWindow();
});

// ── Caricamento iniziale e i18n ───────────────────────────────────────────
(async () => {
  if (!invoke) return;
  try {
    const cfg = await invoke("get_config");
    if (cfg && typeof cfg.window_alpha === "number") {
      document.documentElement.style.setProperty("--window-alpha", cfg.window_alpha);
    }
    const lang = (cfg && cfg.language) || "it";
    await fetchI18n(invoke, lang);
  } catch (e) {
    console.warn("[note-window] get_config/i18n on startup failed:", e);
    await fetchI18n(invoke, "it");
  } finally {
    applyI18n();
  }

  try {
    const data = await invoke("take_window_content");
    if (data) {
      loadNote({ noteId: data.kind || null, title: data.title, text: data.content });
    }
  } catch (e) {
    console.error("[note-window] take_window_content error:", e);
  }
})();

tauriEvent?.listen("config:saved", (ev) => {
  const alpha = ev.payload?.window_alpha;
  if (typeof alpha === "number") {
    document.documentElement.style.setProperty("--window-alpha", alpha);
  }
});
