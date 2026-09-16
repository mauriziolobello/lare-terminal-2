// window.js — Markdown window init for Lare Terminal.
//
// Responsibility (SRP): on load, retrieve (title, content) from Rust managed
// state via `take_window_content`, then render the sanitised Markdown into the
// DOM.  Also wires the chromeless close button (#close-btn) and Esc key to
// `close_self` (Rust command) so the decoration-free window can be closed.
//
// Security contract (ADR-013):
//   - Content MUST pass through DOMPurify before any innerHTML assignment.
//   - DOMPurify is loaded as a local vendored file (vendor/purify.min.js).
//   - marked is loaded as a local vendored file (vendor/marked.min.js).
//   - No CDN at runtime; the project is offline-safe.
//
// Render pipeline:
//   rawMarkdown → marked.parse() → DOMPurify.sanitize() → innerHTML (safe)
//
// Vendored library versions (pinned, no CDN):
//   - marked    v15.0.12   (vendor/marked.min.js)
//   - DOMPurify v3.2.5     (vendor/purify.min.js)

import { LareWsClient } from "./ws-client.js";
import { buildExpandPrompt, isConnectionFailureStatus, isEmptyExpandResult, isAiTurnFailure } from "./expand-prompt.mjs";
import { sortedOrder } from "./table-sort.mjs";
import { outputWindowIdFromLabel } from "./ui-local.mjs";
import { fetchI18n, applyI18n, t } from "./i18n.mjs";

// ---------------------------------------------------------------------------
// Tauri IPC reference
// ---------------------------------------------------------------------------
const tauriInvoke = window.__TAURI__?.core?.invoke;
const tauriEvent = window.__TAURI__?.event;

async function invokeCmd(cmd, args) {
  if (!tauriInvoke) return undefined;
  return tauriInvoke(cmd, args);
}

// ── Finestra di output del canale shell (2.0, spec §3.2/D14) ─────────────────
// Calcolato SUBITO (non dentro bootstrap): serve sia a closeWindow() qui sotto
// (definita ed agganciata prima che bootstrap() giri) sia al wiring
// output:content/output:subscribe più avanti. `null` per ogni finestra
// Markdown normale (label "md-*"/"help") — solo le finestre "output-<id>"
// hanno un myOutputId non nullo.
const myOutputId = outputWindowIdFromLabel(window.__TAURI__?.window?.getCurrentWindow?.()?.label);

// ── Gate di chiusura per finestre show_markdown (update-in-place 2026-09-15) ──
// Una finestra il cui myOutputId termina per "-md" è stata aperta da
// show_markdown dentro un turno AI (contratto con ai_adapter.rs: window_id =
// "{id}-md"). Il turno è attivo finché non arriva "markdown:turn-ended".
// Se l'utente prova a chiudere mentre il turno è attivo, un modale chiede
// conferma; "Sì" cancella il turno (CancelCommand) e chiude.
const turnId = myOutputId?.endsWith("-md") ? myOutputId.slice(0, -3) : null;
let turnActive = turnId !== null;
const currentWindow = window.__TAURI__?.window?.getCurrentWindow?.();

if (turnId && tauriEvent?.listen) {
  tauriEvent.listen("markdown:turn-ended", (event) => {
    if (event.payload?.window_id === myOutputId) {
      turnActive = false;
      if (turnBadgeEl) turnBadgeEl.textContent = "✓ completato";
    }
  });
}

// Bug trovato dal vivo (2026-09-16, Maurizio, con DevTools): `onCloseRequested`
// del SDK Tauri, quando la callback NON chiama `event.preventDefault()`,
// tenta DA SOLO `this.destroy()` come azione di default — e quel comando
// richiede il permesso `core:window:allow-destroy`, mai concesso a questa
// finestra (`markdown-window.json`: solo `core:default` +
// `allow-start-dragging`). Risultato: `Uncaught (in promise) window.destroy
// not allowed`, la finestra non si chiude MAI quando il turno è concluso
// (`turnActive === false`) — funzionava "a volte" solo per timing: `listen()`
// è asincrono, un click abbastanza rapido dopo l'apertura batteva la
// registrazione del listener stesso.
//
// Primo fix tentato (`b139bb1`) — SBAGLIATO, riportava un loop infinito:
// prevenire SEMPRE la chiusura e richiamare `closeWindow()` noi stessi nel
// ramo "turno concluso". Ma `closeWindow()` invoca `close_self` (Rust) →
// `webview.close()` → questo RI-SCATENA `onCloseRequested` lato JS (un
// listener e' registrato) → che richiamava di nuovo `closeWindow()` →
// ciclo che non termina mai (confermato con un harness Node ad-hoc che
// simula il rimbalzo Rust→JS: 10+ giri prima che la guardia anti-loop del
// test intervenisse). Root cause del loop: NON esiste modo di "chiudere
// noi stessi" senza ripassare da questo stesso evento — l'unica via
// d'uscita e' lasciare che il SDK proceda con la sua azione di default.
//
// Fix corretto: preveniamo SOLO se dobbiamo davvero intercettare (turno
// attivo → mostra il modale, la chiusura NON deve procedere). Se il turno
// è concluso, puliamo il buffer (`output:closed`) e poi NON chiamiamo
// `preventDefault()`: lasciamo che il SDK esegua `destroy()` da solo — ora
// permesso da `core:window:allow-destroy` in `markdown-window.json`. Questo
// copre anche Alt+F4 (arriva come lo stesso evento) senza alcun secondo giro.
if (turnId && currentWindow?.onCloseRequested) {
  currentWindow.onCloseRequested(async (event) => {
    if (turnActive) {
      event.preventDefault();
      showCloseConfirmModal();
      return;
    }
    if (myOutputId && tauriEvent?.emit) {
      try { await tauriEvent.emit("output:closed", { window_id: myOutputId }); } catch (_) {}
    }
    // NESSUN preventDefault qui: il default (destroy) chiude la finestra.
  });
}

let currentLanguage = "it";

// ---------------------------------------------------------------------------
// DOM references
// ---------------------------------------------------------------------------
const titlebarLabelEl = document.getElementById("titlebar-label");
const closeBtnEl      = document.getElementById("close-btn");
const saveBtnEl       = document.getElementById("save-btn");
const contentEl       = document.getElementById("content");
const expandInputEl  = document.getElementById("expand-input");
const expandBtnEl    = document.getElementById("expand-btn");
const expandStatusEl = document.getElementById("expand-status");
const turnStatusEl      = document.getElementById("turn-status");
const turnBadgeEl        = document.getElementById("turn-progress-badge");
const turnUpdatedEl      = document.getElementById("turn-updated-time");

// Mostra la barra di progresso SOLO per finestre show_markdown. Va DOPO le
// dichiarazioni `const` sopra (non prima, come nella prima stesura di questo
// blocco): un modulo ES applica la temporal dead zone a `let`/`const` — un
// accesso a `turnStatusEl` prima della sua dichiarazione testuale, anche se
// eseguito subito dopo nell'ordine del file, lancia `ReferenceError` e
// interrompe l'esecuzione di TUTTO il resto del modulo (bootstrap mai
// eseguito, bottone × mai agganciato, il gate di chiusura della Parte B
// incluso) — bug trovato dal supervisore in fase di riverifica, mai
// osservato dai 13 test JS del branch perché nessuno di essi carica
// window.js con un vero DOM (jsdom/browser), solo i moduli .mjs puri.
if (turnId && turnStatusEl) {
  turnStatusEl.classList.add("visible");
}

// ---------------------------------------------------------------------------
// Close helpers
//
// `close_self` is a Rust command that calls webview.close() on the calling
// window.  Called by both the × button and the Esc key.
// Error is ignored: if IPC is unavailable the window will still close via
// Alt-F4 or other OS mechanism.
//
// Fix round 1 (D14): per una finestra di output, emette PRIMA `output:closed`
// (stesso schema del `search:cancel` di window-search.js) così host.js libera
// il buffer — un contenuto che arrivasse dopo la chiusura non deve restare in
// memoria per sempre.
// ---------------------------------------------------------------------------
async function closeWindow() {
  if (myOutputId && tauriEvent?.emit) {
    try { await tauriEvent.emit("output:closed", { window_id: myOutputId }); } catch (_) {}
  }
  await invokeCmd("close_self").catch(() => {});
}

async function requestClose() {
  if (turnId && turnActive) {
    showCloseConfirmModal();
    return;
  }
  await closeWindow();
}

function showCloseConfirmModal() {
  document.getElementById("close-confirm-modal").hidden = false;
}

// × button — data-tauri-drag-region is on the parent #titlebar, NOT on this
// button, so the click is never swallowed by the drag handler.
closeBtnEl.addEventListener("click", requestClose);

// Esc key anywhere in the window closes it.
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") {
    e.preventDefault();
    requestClose();
  }
});

// ---------------------------------------------------------------------------
// Markdown render + sanitize
//
// Security: DOMPurify.sanitize() removes all script tags, event handlers,
// and other dangerous content before the HTML is inserted.
// This is the ONLY place where innerHTML is assigned; every assignment here
// goes through sanitize() first.
// ---------------------------------------------------------------------------
function renderMarkdown(md) {
  // marked.parse() converts Markdown to HTML.
  const rawHtml = window.marked.parse(md);

  // DOMPurify.sanitize() removes script tags, onerror=, javascript: URIs, etc.
  // This is the mandatory sanitization step per ADR-013.
  const safeHtml = window.DOMPurify.sanitize(rawHtml);

  // Only assign sanitized HTML — never assign rawHtml directly.
  contentEl.innerHTML = safeHtml;
}

// ---------------------------------------------------------------------------
// Sortable tables — delegated click listener.
//
// ADR-013: DOMPurify rimuove ogni attributo on* (nessun onclick inline
// possibile) — l'interattività passa da qui, non dal Markdown generato.
//
// Installato una sola volta su #content stesso (mai sostituito da
// un'assegnazione innerHTML) — sopravvive a ogni renderMarkdown()
// successivo, incluso il re-render di "Espandi". Generico: qualunque
// tabella la cui sorgente Markdown emette lo stesso markup (vedi
// screening.py::_table) eredita l'ordinamento senza altro codice qui.
// ---------------------------------------------------------------------------
contentEl.addEventListener("click", (e) => {
  const arrow = e.target.closest(".sort-arrow");
  if (!arrow) return;
  const th = arrow.closest("th");
  const table = arrow.closest("table");
  if (!th || !table) return;
  sortTableByColumn(table, th.cellIndex, arrow.dataset.dir, arrow.dataset.type);
});

function sortTableByColumn(table, colIndex, dir, type) {
  const tbody = table.tBodies[0];
  if (!tbody) return;
  const rows = Array.from(tbody.rows);
  const keys = rows.map((row) => {
    const raw = row.cells[colIndex]?.dataset.sortValue;
    return raw === undefined ? null : raw;
  });
  const order = sortedOrder(keys, dir, type);
  order.forEach((originalIndex, newIndex) => {
    const row = rows[originalIndex];
    // tbody.appendChild su un nodo già nel DOM lo SPOSTA in coda
    // (comportamento nativo di Node.appendChild) — appendere in sequenza
    // secondo `order` riproduce esattamente quell'ordine come ordine
    // finale dei figli del tbody.
    tbody.appendChild(row);
    const posCell = row.cells[0];
    if (posCell?.hasAttribute("data-pos")) posCell.textContent = String(newIndex + 1);
  });
}

// ---------------------------------------------------------------------------
// Bootstrap: get window label from URL query param, fetch content, render.
//
// The Rust `open_markdown_window` command appended `?label=<label>` to the
// WebviewUrl so window.js can identify itself and call take_window_content.
// ---------------------------------------------------------------------------
async function bootstrap() {
  if (!tauriInvoke) {
    contentEl.classList.replace("loading", "error");
    contentEl.textContent = t("md_window.error_ipc");
    return;
  }

  let webSearchEnabled = true; // default, mirror di Config::default().web_search_enabled
  try {
    const cfg = await invokeCmd("get_config");
    if (cfg && typeof cfg.window_alpha === "number") {
      document.documentElement.style.setProperty("--window-alpha", cfg.window_alpha);
    }
    if (cfg && typeof cfg.web_search_enabled === "boolean") {
      webSearchEnabled = cfg.web_search_enabled;
    }
    if (cfg?.language) {
      currentLanguage = cfg.language;
    }
    await fetchI18n(invokeCmd, cfg?.language);
  } catch (e) {
    console.warn("[window] get_config on startup failed:", e);
    await fetchI18n(invokeCmd);
  }
  applyI18n(document);

  let data;
  try {
    // `take_window_content` derives THIS window's label server-side (from the
    // calling WebviewWindow) and returns { title, content }, removing the entry
    // (one-shot read prevents memory leaks). No ?label= query param needed.
    data = await invokeCmd("take_window_content");
  } catch (e) {
    contentEl.classList.replace("loading", "error");
    contentEl.textContent = t("md_window.error_retrieving", { error: e });
    return;
  }

  if (!data) {
    contentEl.classList.replace("loading", "error");
    contentEl.textContent = t("md_window.no_content");
    return;
  }

  // Update the title bar label and document title.
  // Only the label span is updated — the close button in #titlebar is untouched.
  const title = data.title || t("md_window.titlebar");
  titlebarLabelEl.textContent = title;
  document.title = title;

  // Remove the loading class before rendering.
  contentEl.classList.remove("loading");

  // Apply special window styles based on `kind`.
  // "help" → add `body.help` CSS class so the window is visually distinct
  // (thicker/differently coloured border — see window.html CSS).
  // Other kinds (or missing kind) leave body unstyled (normal Markdown window).
  if (data.kind === "help") {
    document.body.classList.add("help");
  } else if (data.kind === "archived") {
    document.body.classList.add("archived");
    if (data.source_file) {
      document.body.classList.add("expandable");
    }
  }

  // Wire the save button now that data is available.
  // The closure captures `data.title` and `data.content` (the original Markdown,
  // NOT the rendered HTML — we archive the source, not the sanitized output).
  // We keep a reference to the original text rather than reading innerHTML to
  // ensure we save what was received, not what DOMPurify may have stripped.
  const saveTitle   = data.title || t("common.untitled");
  // `let`, non `const`: per una finestra di output del canale shell (2.0),
  // il contenuto arriva DOPO l'apertura via evento `output:content` — il
  // salvataggio in Library deve archiviare quel contenuto aggiornato, non
  // il segnaposto "_in corso…_" con cui la finestra si apre (vedi sotto).
  let saveContent = data.content || "";

  saveBtnEl.addEventListener("click", async () => {
    // Guard: disable immediately to prevent duplicate saves from rapid clicks
    // or clicks that arrive while archive_save is in flight.
    // This is idempotent for the session: once saved, the button stays disabled.
    if (saveBtnEl.disabled) return;
    saveBtnEl.disabled = true;

    try {
      await invokeCmd("archive_save", { title: saveTitle, content: saveContent });
      // Success: stable "saved" state — button stays disabled, text updated.
      // The user cannot click again until the window is reopened (new session).
      saveBtnEl.textContent = t("common.saved");
    } catch (e) {
      // Failure: re-enable the button so the user can retry.
      console.error("[archive] archive_save failed:", e);
      saveBtnEl.disabled = false;
      // Transient error indicator, then restore original label.
      const originalText = saveBtnEl.textContent;
      saveBtnEl.textContent = "✗";
      setTimeout(() => {
        saveBtnEl.textContent = originalText;
      }, 1500);
    }
  });

  // ── Espandi (solo se data.source_file è presente — vedi CSS body.expandable) ──
  // Ogni click apre una connessione WS EFFIMERA (Hello{channel:"library-expand"}),
  // manda UN comando, riceve lo streaming, chiude — nessuna history che
  // persiste tra un click e l'altro (ogni volta si rimanda l'intero documento
  // aggiornato, riusare una connessione accumulerebbe versioni vecchie).
  let currentContent = data.content || "";
  const sourceFile = data.source_file || "";
  const docTitle = data.title || t("common.untitled");

  // ── Finestra di output del canale shell (2.0, spec §3.2/D14) ──────────────
  // Solo le finestre `output-<id>` ricevono aggiornamenti dopo l'apertura
  // (myOutputId, calcolato a livello di modulo — vedi sopra). Il testo
  // salvabile in Library è quello aggiornato, non il segnaposto.
  //
  // Fix round 1 (buffer-and-replay, D14): l'evento Tauri globale NON è
  // bufferizzato dal runtime — se host.js avesse già ricevuto/emesso
  // `output_window_content` PRIMA che questo listener fosse registrato, il
  // contenuto sarebbe perso per sempre. Soluzione: ci mettiamo in ascolto
  // PRIMA di annunciarci (await sul listen, poi emit `output:subscribe`).
  // host.js (vedi output-buffer.mjs/setupOutputEvents) bufferizza il
  // contenuto arrivato in anticipo e, alla ricezione di `output:subscribe`,
  // lo rigioca come lo STESSO evento `output:content` — un evento live e un
  // replay arrivano quindi allo stesso handler qui sotto, senza duplicare
  // logica.
  if (myOutputId && tauriEvent?.listen) {
    const applyOutputContent = (markdown) => {
      renderMarkdown(markdown || "");
      saveContent = markdown || "";
      currentContent = markdown || "";
      // Aggiorna l'orario dell'ultimo contenuto (badge di progresso Parte C).
      if (turnUpdatedEl) {
        turnUpdatedEl.textContent = "aggiornato alle " + new Date().toLocaleTimeString();
      }
    };
    try {
      await tauriEvent.listen("output:content", (ev) => {
        const p = ev.payload || {};
        if (p.window_id !== myOutputId) return;
        applyOutputContent(p.markdown);
      });
      await tauriEvent.emit("output:subscribe", { window_id: myOutputId });
    } catch (e) {
      console.error("[window] output:content/subscribe wiring error:", e);
    }
  }

  expandInputEl.addEventListener("input", () => {
    expandBtnEl.disabled = expandInputEl.value.trim().length === 0;
  });

  expandInputEl.addEventListener("keydown", (ev) => {
    if (ev.key === "Enter" && !expandBtnEl.disabled) {
      ev.preventDefault();
      runExpand();
    }
  });

  expandBtnEl.addEventListener("click", runExpand);

  async function runExpand() {
    const request = expandInputEl.value.trim();
    if (!request || !sourceFile) return;

    expandBtnEl.disabled = true;
    expandInputEl.disabled = true;
    expandStatusEl.textContent = t("md_window.expanding");

    const token = (await invokeCmd("get_lare_token")) ?? "";
    // Porta WS da startup.json (2.0), stesso comando usato da host.js.
    const url = (await invokeCmd("get_ws_endpoint")) ?? "";
    const requestId = crypto.randomUUID();
    let buffer = "";
    let settled = false;

    const client = new LareWsClient({
      url,
      token,
      channel: "library-expand",
      lang: currentLanguage,
      // LareWsClient retries indefinitely on drop (exponential backoff, no
      // cap) — fine for the main cursor connection, wrong here: this is a
      // one-shot request, and without this handler a lost connection would
      // either strand the UI on "espando…" forever, or a later successful
      // reconnect would silently re-send the SAME prompt as a fresh AI turn
      // (a new `server_info` re-triggers the `sendCommand` below). Guard
      // with `settled` so this never double-fires against `done`/`error`.
      onStatus: (status) => {
        if (!isConnectionFailureStatus(status, settled)) return;
        settled = true;
        failExpand(t("md_window.connection_lost"));
        client.disconnect();
      },
      onMessage: (msg) => {
        if (msg.id !== requestId && msg.type !== "server_info") return;
        if (msg.type === "server_info") {
          client.sendCommand(buildExpandPrompt(currentContent, request), requestId, webSearchEnabled, currentLanguage);
          return;
        }
        if (msg.type === "chunk") {
          buffer += msg.content;
        } else if (msg.type === "done") {
          settled = true;
          if (isAiTurnFailure(msg.exit_code)) {
            const detail = buffer.trim() || t("md_window.unknown_error");
            failExpand(t("md_window.ai_failed", { detail }));
          } else if (isEmptyExpandResult(buffer)) {
            failExpand(t("md_window.ai_empty"));
          } else {
            finishExpand(buffer);
          }
          client.disconnect();
        } else if (msg.type === "error") {
          settled = true;
          failExpand(msg.message || t("md_window.unknown_error"));
          client.disconnect();
        }
      },
    });
    client.connect();
  }

  async function finishExpand(newContent) {
    try {
      await invokeCmd("archive_update", { file: sourceFile, title: docTitle, content: newContent });
    } catch (e) {
      failExpand(t("md_window.save_failed", { error: e }));
      return;
    }
    currentContent = newContent;
    renderMarkdown(newContent);
    expandStatusEl.textContent = t("md_window.expanded");
    expandInputEl.value = "";
    expandInputEl.disabled = false;
    expandBtnEl.disabled = true; // richiede nuovo testo per riattivarsi
    setTimeout(() => { expandStatusEl.textContent = ""; }, 3000);
  }

  function failExpand(message) {
    console.error("[window] expand failed:", message);
    expandStatusEl.textContent = `✗ ${message}`;
    expandInputEl.disabled = false;
    expandBtnEl.disabled = expandInputEl.value.trim().length === 0;
    // Messaggio più lungo di quello di successo (può includere il testo
    // d'errore reale dell'orchestrator, es. "[errore AI] ..."): 3s bastano
    // per "✓ Espanso" ma sono troppo poco per leggerlo (riscontro utente,
    // smoke test dal vivo 2026-07-20).
    setTimeout(() => { expandStatusEl.textContent = ""; }, 8000);
  }

  // Render the sanitised Markdown.
  renderMarkdown(data.content);

  // Auto-height: resize the window to fit the content after the browser
  // has performed layout (requestAnimationFrame guarantees post-layout measure).
  //
  // Why we temporarily collapse #content's flex growth:
  //   - body is height:100% + overflow:hidden with flex-direction:column.
  //   - #content is flex:1, so the browser STRETCHES it to fill the window —
  //     its clientHeight ≈ (window height − titlebar), regardless of content size.
  //   - scrollHeight is defined as max(padding-box height, content height), so for
  //     short content scrollHeight ≈ clientHeight ≈ 600 — useless for auto-sizing.
  //   - Fix: temporarily set flex:"0 0 auto" so the element collapses to its natural
  //     content height; read scrollHeight; then restore flex so the layout is correct
  //     after the resize.
  requestAnimationFrame(() => {
    const titlebarEl = document.getElementById("titlebar");

    // Collapse flex growth to read the true content height.
    const prevFlex = contentEl.style.flex;
    contentEl.style.flex = "0 0 auto";
    const contentNatural = contentEl.scrollHeight; // reading forces sync reflow; now = real content height
    contentEl.style.flex = prevFlex; // restore so flex:1 refills after resize

    const naturalHeight =
      contentNatural +
      (titlebarEl ? titlebarEl.offsetHeight : 0) +
      4; // body's 1.5px top/bottom border (3px) + 1px anti-scrollbar buffer

    const MIN_HEIGHT = 120;
    const MAX_HEIGHT = Math.round(window.screen.availHeight * 0.85);
    const clampedHeight = Math.max(MIN_HEIGHT, Math.min(naturalHeight, MAX_HEIGHT));

    // Keep current width; only adjust height.
    const currentWidth = window.outerWidth || 800;

    invokeCmd("resize_self", { width: currentWidth, height: clampedHeight }).catch(() => {});
  });
}

// ── Modale conferma chiusura (show_markdown update-in-place) ──────────────
document.getElementById("close-confirm-yes").addEventListener("click", async () => {
  document.getElementById("close-confirm-modal").hidden = true;
  turnActive = false;
  if (tauriEvent?.emit) {
    try { await tauriEvent.emit("markdown:cancel-turn", { turn_id: turnId }); } catch (_) {}
  }
  await closeWindow();
});
document.getElementById("close-confirm-no").addEventListener("click", () => {
  document.getElementById("close-confirm-modal").hidden = true;
});

bootstrap();

tauriEvent?.listen("config:saved", async (ev) => {
  const alpha = ev.payload?.window_alpha;
  if (typeof alpha === "number") {
    document.documentElement.style.setProperty("--window-alpha", alpha);
  }
  if (ev.payload?.language) {
    currentLanguage = ev.payload.language;
    await fetchI18n(invokeCmd, ev.payload.language);
    applyI18n(document);
  }
});
