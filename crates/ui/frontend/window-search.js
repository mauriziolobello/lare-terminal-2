// window-search.js — finestra di ricerca LIVE per Lare Terminal.
//
// Responsabilità (SRP): all'apertura legge (title, sid) da `take_window_content`
// (lo stesso store delle finestre Markdown; per la ricerca il campo `content`
// porta il `sid`). Poi ascolta gli eventi Tauri globali emessi dalla finestra
// principale e filtrati per `sid`:
//   - "search:hit"  { sid, path, source }  → appende una riga nel gruppo della sorgente
//   - "search:done" { sid, count, truncated } → aggiorna lo stato (✅ N)
// Click su una riga → emette "search:open-path" { path } (la finestra principale
// invia `/open <path>` via WebSocket). Chiusura (×/Esc) → emette "search:cancel"
// { sid } (la principale invia CancelSearch) e poi chiude la finestra.
// Il pulsante ⏹ emette "search:pause" { sid } (la principale invia PauseSearch);
// il pulsante ▶ emette "search:resume" { sid } (la principale invia ResumeSearch).
//
// Sicurezza: i path arrivano come testo e vengono inseriti SOLO via textContent
// (mai innerHTML) → nessuna injection di markup.

import { statusLabel } from "./search-status.js";
import { parentDir } from "./path-utils.js";

const tauri = window.__TAURI__;
const invoke = tauri?.core?.invoke;
const tauriEvent = tauri?.event;

async function invokeCmd(cmd, args) {
  if (!invoke) return undefined;
  return invoke(cmd, args);
}

// ── DOM ──────────────────────────────────────────────────────────────────────
const titlebarLabelEl = document.getElementById("titlebar-label");
const statusEl        = document.getElementById("status");
const closeBtnEl      = document.getElementById("close-btn");
const stopBtnEl       = document.getElementById("stop-btn");
const resumeBtnEl     = document.getElementById("resume-btn");
const saveBtnEl       = document.getElementById("save-btn");
const resultsEl       = document.getElementById("results");
const emptyEl         = document.getElementById("empty");

// ── Stato ────────────────────────────────────────────────────────────────────
let mySid   = null;
let stopped = false;
let paused  = false;
let done    = false;
/** Running hit count, updated incrementally via addHit(); used by statusLabel in paused state. */
let hitCount = 0;
const groups = new Map(); // source → { listEl, countEl, count }

// Ordine e label dei gruppi per sorgente (wire value → etichetta).
const SOURCE_ORDER = ["cwd", "standard", "cloud", "external"];
const SOURCE_LABEL = {
  cwd: "Cartella corrente",
  standard: "Standard",
  cloud: "Cloud",
  external: "Unità esterne",
};

// ── Chiusura: annulla la ricerca, poi chiudi la finestra ──────────────────────
async function closeWindow() {
  if (mySid && tauriEvent?.emit) {
    try { await tauriEvent.emit("search:cancel", { sid: mySid }); } catch (_) {}
  }
  await invokeCmd("close_self").catch(() => {});
}
closeBtnEl.addEventListener("click", closeWindow);
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") { e.preventDefault(); closeWindow(); }
});

// ── Pause: mette in pausa la ricerca, lascia la finestra aperta ──────────────
async function pauseSearch() {
  if (paused || stopped || done) return; // idempotente
  paused = true;
  if (mySid && tauriEvent?.emit) {
    try { await tauriEvent.emit("search:pause", { sid: mySid }); } catch (_) {}
  }
  stopBtnEl.classList.add("hidden");
  resumeBtnEl.classList.remove("hidden");
  statusEl.textContent = statusLabel({ done: false, paused: true, count: hitCount });
}
stopBtnEl.addEventListener("click", pauseSearch);

// ── Resume: riprende la ricerca dalla pausa ────────────────────────────────────
async function resumeSearch() {
  if (!paused || stopped || done) return; // idempotente
  paused = false;
  if (mySid && tauriEvent?.emit) {
    try { await tauriEvent.emit("search:resume", { sid: mySid }); } catch (_) {}
  }
  resumeBtnEl.classList.add("hidden");
  stopBtnEl.classList.remove("hidden");
  statusEl.textContent = statusLabel({ done: false, paused: false });
}
resumeBtnEl.addEventListener("click", resumeSearch);

// ── Gruppi per sorgente (inseriti nell'ordine di priorità) ────────────────────
function getGroup(source) {
  let g = groups.get(source);
  if (g) return g;

  const header = document.createElement("div");
  header.className = "group-header";
  const countEl = document.createElement("span");
  const labelText = SOURCE_LABEL[source] || source;
  header.textContent = `▸ ${labelText} (`;
  header.appendChild(countEl);
  header.appendChild(document.createTextNode(")"));
  countEl.textContent = "0";

  const listEl = document.createElement("div");

  // Inserisci header+list nella posizione giusta secondo SOURCE_ORDER, così
  // l'ordine visivo resta cwd → standard → cloud → external a prescindere
  // dall'ordine d'arrivo degli hit.
  const myIdx = SOURCE_ORDER.indexOf(source);
  let beforeNode = null;
  for (const [otherSource, otherG] of groups) {
    if (SOURCE_ORDER.indexOf(otherSource) > myIdx) {
      beforeNode = otherG.headerEl;
      break;
    }
  }
  resultsEl.insertBefore(header, beforeNode);
  resultsEl.insertBefore(listEl, beforeNode);

  g = { headerEl: header, listEl, countEl, count: 0 };
  groups.set(source, g);
  return g;
}

function addHit(source, path, line, snippet) {
  if (emptyEl && emptyEl.parentNode) emptyEl.remove();

  hitCount += 1;
  const g = getGroup(source);
  const row = document.createElement("div");
  row.className = "result-row";
  row.title = path;

  // Contenitore verticale: path (+ eventuale riga/snippet sotto).
  const main = document.createElement("div");
  main.className = "row-main";

  const pathLabel = document.createElement("span");
  pathLabel.className = "row-path";
  pathLabel.textContent = path;
  main.appendChild(pathLabel);

  // Snippet di contenuto (solo per un hit di ricerca CONTENUTO — line/snippet
  // presenti). Testo via textContent — mai innerHTML (stessa cautela dei path).
  if (typeof line === "number" && typeof snippet === "string") {
    const snippetLabel = document.createElement("div");
    snippetLabel.className = "row-snippet";
    snippetLabel.textContent = `${line}: ${snippet}`;
    main.appendChild(snippetLabel);
  }

  row.appendChild(main);

  // Folder button: opens the parent directory via the same search:open-path event.
  const folderBtn = document.createElement("button");
  folderBtn.className = "row-folder-btn";
  folderBtn.title = "Apri cartella";
  folderBtn.setAttribute("aria-label", "Apri cartella");
  folderBtn.textContent = "\u{1F4C2}"; // 📂
  folderBtn.addEventListener("click", (e) => {
    e.stopPropagation(); // do not trigger the row's open-file click
    openPath(parentDir(path));
  });
  row.appendChild(folderBtn);

  // Row click opens the file itself.
  row.addEventListener("click", () => openPath(path));
  g.listEl.appendChild(row);

  g.count += 1;
  g.countEl.textContent = String(g.count);
}

async function openPath(path) {
  if (tauriEvent?.emit) {
    try { await tauriEvent.emit("search:open-path", { path }); } catch (e) {
      console.error("[search] emit open-path error:", e);
    }
  }
}

// ── Save button (live mode only) ──────────────────────────────────────────────
//
// Raccoglie gli hit correnti da `groups` (path via textContent — no injection),
// la query dal titolo, e invoca `save_find`. Idempotente: una volta salvato il
// pulsante resta disabilitato per la sessione (stesso pattern di window.js).
function wireSaveButton(query) {
  saveBtnEl.classList.remove("hidden");
  saveBtnEl.addEventListener("click", async () => {
    if (saveBtnEl.disabled) return; // guard idempotente
    saveBtnEl.disabled = true;

    // Colleziona gli hit dallo stato `groups`: per ogni riga nel listEl del gruppo
    // il path è nel primo span (.row-path) via textContent.
    const hits = [];
    for (const [source, g] of groups) {
      for (const row of g.listEl.children) {
        const pathEl = row.querySelector(".row-path");
        if (pathEl) hits.push({ path: pathEl.textContent, source });
      }
    }

    try {
      await invokeCmd("save_find", { query, hits });
      // Successo: feedback stabile; il pulsante non viene riabilitato (idempotente).
      saveBtnEl.textContent = "✓ Salvato";
    } catch (e) {
      console.error("[search] save_find failed:", e);
      saveBtnEl.disabled = false;
      const original = saveBtnEl.textContent;
      saveBtnEl.textContent = "✗";
      setTimeout(() => { saveBtnEl.textContent = original; }, 1500);
    }
  });
}

// ── Bootstrap ─────────────────────────────────────────────────────────────────
async function bootstrap() {
  if (!invoke || !tauriEvent) {
    if (statusEl) statusEl.textContent = "errore: Tauri non disponibile";
    return;
  }

  try {
    const cfg = await invokeCmd("get_config");
    if (cfg && typeof cfg.window_alpha === "number") {
      document.documentElement.style.setProperty("--window-alpha", cfg.window_alpha);
    }
  } catch (e) {
    console.warn("[window-search] get_config on startup failed:", e);
  }

  let data;
  try {
    // Stesso store delle finestre Markdown; per la ricerca: content = sid, kind = "search".
    // In modalità "saved", content = "saved:" + JSON (ArchiveFind).
    data = await invokeCmd("take_window_content");
  } catch (e) {
    if (statusEl) statusEl.textContent = `errore: ${e}`;
    return;
  }
  if (!data) {
    if (statusEl) statusEl.textContent = "errore: nessun contesto di ricerca";
    return;
  }

  const title = data.title || "Lare — Ricerca";
  titlebarLabelEl.textContent = title;
  document.title = title;

  // Aggiorna live la trasparenza se l'utente cambia lo slider in /config
  // mentre questa finestra resta aperta. Registrato QUI, prima dello split
  // saved/live, così vale in ENTRAMBE le modalità — stesso pattern di
  // window.js (che lo registra a livello di modulo, fuori da ogni branch).
  tauriEvent.listen("config:saved", (ev) => {
    const alpha = ev.payload?.window_alpha;
    if (typeof alpha === "number") {
      document.documentElement.style.setProperty("--window-alpha", alpha);
    }
  });

  // ── Modalità "saved" (replay) ───────────────────────────────────────────
  // content inizia con "saved:" → i risultati vengono da un ArchiveFind salvato.
  // In questa modalità: NO listener live, NO 💾, NO ⏹.
  if (typeof data.content === "string" && data.content.startsWith("saved:")) {
    let archive;
    try {
      archive = JSON.parse(data.content.slice("saved:".length));
    } catch (e) {
      statusEl.textContent = `errore: JSON non valido (${e})`;
      return;
    }

    // Nasconde i controlli live (stop, resume e save non hanno senso in replay).
    stopBtnEl.classList.add("hidden");
    resumeBtnEl.classList.add("hidden");
    saveBtnEl.classList.add("hidden");

    // Replay degli hit nella stessa struttura visiva della modalità live.
    const hits = Array.isArray(archive.hits) ? archive.hits : [];
    for (const hit of hits) {
      if (hit && typeof hit.source === "string" && typeof hit.path === "string") {
        addHit(hit.source, hit.path, hit.line, hit.snippet);
      }
    }

    // Stato finale "salvati": usa la label standard + indicatore esplicito.
    const n = hits.length;
    statusEl.textContent = `✅ ${n} risultati (salvati)`;
    return; // fine — nessun listener live
  }

  // ── Modalità "live" (comportamento originale) ───────────────────────────
  mySid = data.content; // il sid è trasportato nel campo content

  // Mostra il pulsante 💾 e lo connette al query estratto dal titolo.
  // La query è il titolo stesso (il titolo della finestra è "Lare — Find: <query>").
  // Per sicurezza passiamo il titolo grezzo; `save_find` lo riceve come stringa
  // opaca e non esegue comandi — nessun rischio injection lato Tauri.
  wireSaveButton(title);

  // Ascolta gli hit/done filtrando per il NOSTRO sid (eventi globali).
  tauriEvent.listen("search:hit", (e) => {
    const p = e.payload || {};
    if (p.sid !== mySid) return;
    addHit(p.source, p.path, p.line, p.snippet);
  });
  tauriEvent.listen("search:done", (e) => {
    const p = e.payload || {};
    if (p.sid !== mySid) return;
    done = true;
    // Hide both action buttons: search is over, no more pause/resume.
    stopBtnEl.classList.add("hidden");
    resumeBtnEl.classList.add("hidden");
    statusEl.textContent = statusLabel({
      done: true, stopped, count: p.count, truncated: p.truncated,
    });
    if (p.count === 0 && emptyEl && emptyEl.parentNode) {
      emptyEl.textContent = stopped ? "Ricerca interrotta." : "Nessun file trovato.";
    }
  });

  // Buffer-and-replay: segnala al main che i listener sono attivi. Il main
  // rigioca gli hit/done accumulati prima che questa finestra fosse pronta.
  // (Solo modalità live: la modalità "saved" è già tornata prima con `return`.)
  if (tauriEvent?.emit) {
    tauriEvent.emit("search:subscribe", { sid: mySid }).catch(() => {});
  }
}

bootstrap();
