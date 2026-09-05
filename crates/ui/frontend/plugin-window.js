// plugin-window.js — runtime for the generic plugin window (Tauri webview).
//
// This module runs INSIDE the plugin webview (plugin-window.html), not in the
// main cursor window.  It:
//
//   1. Calls `take_window_content` at load time to receive:
//        { title, content: <initial HTML>, kind: "<window_id>" }
//      from the managed WindowContentStore (same mechanism as window-search.js).
//
//   2. Renders the initial HTML with DOMPurify (vendored, already loaded
//      synchronously by plugin-window.html before this module runs).
//
//   3. Listens for delegated click events on #plugin-root and converts them
//      to PluginUiEvent messages emitted via the Tauri global event bus →
//      app.js picks them up and forwards them over WebSocket.
//
//   4. Listens for incoming Tauri events:
//        "plugin:update"  → re-render with new HTML
//        "plugin:close"   → close this window
//      Both are global (broadcast to all windows); we filter by myWindowId.
//
//   5. Handles the ✕ button: notifies the orchestrator that the user closed
//      the window manually, then closes the webview.
//
// Back-channel (this window → orchestrator):
//   tauriEvent.emit("plugin:ui-event",      { window_id, element_id, value })
//   tauriEvent.emit("plugin:window-closed", { window_id })
// Both events are received by app.js and forwarded over WebSocket.

// ── Tauri v2 globals ───────────────────────────────────────────────────────
// Tauri v2 with `withGlobalTauri: true` exposes __TAURI__ on window.
const { invoke }  = window.__TAURI__.core;
const tauriEvent  = window.__TAURI__.event;

import { eventFromTarget, eventFromDblTarget, eventFromKey, sanitizeAndRender, restoreActiveField } from "./plugin-runtime.mjs";

// ── DOM refs ───────────────────────────────────────────────────────────────
const titlebarLabel = document.getElementById("titlebar-label");
const closeBtn      = document.getElementById("close-btn");
const pluginRoot    = document.getElementById("plugin-root");

// ── State ──────────────────────────────────────────────────────────────────
// window_id is a u64 on the Rust side; we keep it as a JS number.
// The advisor flagged that losing the numeric type would silently break both
// the WS deserialization (Rust u64 rejects a JSON string) and the event filter.
let myWindowId = null;   // number — set once take_window_content returns

// High-water dell'altezza del display calcolatrice (px), per-webview.
// DEVE vivere qui (non sull'elemento): ogni render re-inietta l'HTML, ricreando
// il `.lare-display` e perdendo il suo `style.minHeight`. Tenendolo in modulo, lo
// ri-applichiamo dopo ogni render → l'altezza è davvero monotòna. Reset = riapertura
// /calc (nuova webview ⇒ modulo ricaricato ⇒ torna a 0 ⇒ pavimento CSS 3 righe).
let calcDisplayHighWaterPx = 0;

// ── Helpers ────────────────────────────────────────────────────────────────

/**
 * Invoke a Tauri command via the IPC bridge.
 * Thin wrapper to keep call sites clean (same pattern as app.js).
 *
 * @param {string} cmd   - Tauri command name (snake_case).
 * @param {object} [args] - Optional arguments object.
 */
function invokeCmd(cmd, args) {
  return invoke(cmd, args ?? {});
}

/**
 * Render `html` into #plugin-root using the already-loaded DOMPurify.
 * DOMPurify is loaded synchronously by <script src="vendor/purify.min.js">
 * before this module, so window.DOMPurify is available here.
 *
 * @param {string} html - Raw HTML from the plugin.
 */
function render(html) {
  // Preserva focus + VALORE LIVE + posizione del cursore su un
  // <input>/<textarea> che l'utente sta editando: sanitizeAndRender
  // rimpiazza SEMPRE l'intero DOM (nessun diffing — vedi il commento su
  // sanitizeAndRender in plugin-runtime.mjs), quindi un input attivo
  // verrebbe distrutto e ricreato ad ogni ridigitazione.
  //
  // Cattura solo (focus+selection, non ancora il valore) risolveva un bug
  // precedente ("il campo perde il focus mentre scrivo"). Il VALORE resta
  // catturato qui perché è l'unico punto in cui possiamo leggere
  // `active.value` PRIMA che sanitizeAndRender lo distrugga: è lo stato
  // REALMENTE digitato dall'utente in questo istante, che è sempre più
  // aggiornato dell'HTML che arriva dal plugin (ogni tasto fa un round-trip
  // completo attraverso il sidecar — l'HTML fresco riflette sempre lo stato
  // di UN TASTO PRIMA). Senza questa cattura, il valore fresco (in ritardo)
  // sovrascriveva sistematicamente quanto già digitato: ogni carattere
  // appariva per un istante e spariva subito — bug osservato dal vivo nel
  // pannello "Testo in chiaro" del plugin crittografia.
  const active = document.activeElement;
  let preserved = null;
  if (active && pluginRoot.contains(active) &&
      (active.tagName === "INPUT" || active.tagName === "TEXTAREA")) {
    preserved = {
      activeDatasetEvt: active.dataset.evt,
      activeValue: active.value,
      activeStart: active.selectionStart,
      activeEnd: active.selectionEnd,
    };
  }

  sanitizeAndRender(pluginRoot, html, window.DOMPurify);

  // Ritrova il NUOVO elemento con lo stesso data-evt (identità semantica,
  // non la reference DOM che è stata distrutta) e gli ripristina
  // valore+focus+cursore. Logica pura (niente window/document/CSS.escape)
  // estratta in plugin-runtime.mjs così è testabile con node:test senza
  // un browser — vedi restoreActiveField per i dettagli, incluso il perché
  // l'ordine valore→selectionRange conta.
  restoreActiveField({ ...preserved, newRootElement: pluginRoot });

  // Porta in vista l'elemento marcato come selezionato (convenzione ARIA
  // generica, non specifica di un plugin): serve quando la selezione si
  // sposta via tastiera oltre l'area visibile di un contenitore con scroll
  // nativo (es. le righe di Lare Commander, ora che non tronca più la
  // lista lato server — vedi Parte D).
  pluginRoot.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: "nearest", inline: "nearest" });
}

/**
 * Emit a Tauri global event to notify app.js that the user interacted with
 * a [data-evt] element.  app.js listens for "plugin:ui-event" and forwards
 * the payload over WebSocket as ClientMsg::PluginUiEvent.
 *
 * @param {string}      element_id - Value of the clicked element's data-evt.
 * @param {string|null} value      - Filled value (INPUT/SELECT) or null.
 */
function emitUiEvent(element_id, value) {
  tauriEvent.emit("plugin:ui-event", {
    window_id: myWindowId,   // must be a JS number — Rust u64
    element_id,
    value,
  });
}

/**
 * Emit a Tauri global event to notify app.js that this window was closed
 * (either by the orchestrator or by the user clicking ✕).
 * app.js listens for "plugin:window-closed" and sends PluginWindowClosed to
 * the orchestrator so it can clean up state.
 */
function emitWindowClosed() {
  tauriEvent.emit("plugin:window-closed", { window_id: myWindowId });
}

/** Close this webview.  Mirrors window-search.js's use of "close_self". */
function closeSelf() {
  invokeCmd("close_self");
}

/**
 * Display grow-only della calcolatrice (solo finestre con `.lare-calc`).
 *
 * Tiene il high-water dell'altezza del display in `calcDisplayHighWaterPx` (modulo,
 * per-webview) e lo ri-applica come `min-height` ad ogni render. È necessario il
 * modulo perché ogni render re-inietta l'HTML: il `.lare-display` è un elemento NUOVO
 * ogni volta e perderebbe uno `style.minHeight` inline. Misuriamo `scrollHeight` del
 * display appena reso (= max(contenuto, pavimento CSS 3 righe)), aggiorniamo il massimo
 * e lo riapplichiamo → il display non rimpicciolisce mai entro la sessione della finestra
 * → l'auto-fit (invariato) non rimpicciolisce la finestra → niente "saltellio". Reset =
 * riapertura /calc (nuova webview ⇒ modulo ricaricato ⇒ high-water a 0 ⇒ pavimento 3 righe).
 *
 * Guard: agisce solo se esiste un `.lare-calc .lare-display` → counter/ping e gli
 * altri plugin non sono toccati. Va chiamata PRIMA di autoFitHeight() così che la
 * finestra misuri il display già cresciuto.
 */
function growCalcDisplay() {
  const display = pluginRoot.querySelector(".lare-calc .lare-display");
  if (!display) return;
  // L'elemento è appena stato (ri)creato dal render → parte dal pavimento CSS (3 righe).
  // `scrollHeight` = max(contenuto, pavimento). Accumuliamo il massimo storico in modulo
  // e lo ri-applichiamo: il display non rimpicciolisce mai entro la sessione della finestra.
  calcDisplayHighWaterPx = Math.max(calcDisplayHighWaterPx, display.scrollHeight);
  display.style.minHeight = calcDisplayHighWaterPx + "px";
}

/**
 * Resize this window's HEIGHT to fit its content.
 * Called after EVERY render (initial in init() AND each plugin:update) so that
 * content that grows — e.g. the calculator's display switching from a single
 * line to a stacked 2D fraction — is never clipped.  Only the height is
 * adjusted; the width is left at the user's current value, so a manual
 * horizontal resize (e.g. to fit a long expression) is preserved and we never
 * "fight" the user.  When the content height is unchanged, resize_self to the
 * same height is a visual no-op.
 *
 * Why the flex-collapse trick?
 *   #plugin-root is `flex:1` (stretches to fill the available window height).
 *   When flex:1 is active, `scrollHeight` reports the container height, not the
 *   natural content height — useless for fitting.  Temporarily switching to
 *   `flex: 0 0 auto` collapses the element to its minimum size, making the
 *   browser recompute layout synchronously so that `scrollHeight` returns the
 *   real content height.  We then restore the original flex value.
 *
 * The final height = content natural height + titlebar height + 4 px border
 * buffer, clamped to [120 px, 85 % of the screen height].
 */
function autoFitHeight() {
  // Opt-out: un plugin che vuole gestire la propria altezza (es. un file
  // manager che l'utente deve poter ridimensionare liberamente) marca il
  // proprio elemento radice con [data-no-autofit]. Senza questo, ogni
  // re-render richiama resize_self sull'altezza "naturale" del contenuto,
  // vanificando qualunque resize verticale manuale dell'utente. È una
  // convenzione di sola presentazione (sniff dell'HTML reso, come
  // growCalcDisplay col marker .lare-calc) — nessun campo di manifest/protocollo.
  if (pluginRoot.querySelector("[data-no-autofit]")) return;

  requestAnimationFrame(() => {
    const titlebarEl = document.getElementById("titlebar");

    const prevFlex = pluginRoot.style.flex;
    pluginRoot.style.flex = "0 0 auto";
    const contentNatural = pluginRoot.scrollHeight; // forza reflow sync → altezza reale
    pluginRoot.style.flex = prevFlex;

    const naturalHeight =
      contentNatural +
      (titlebarEl ? titlebarEl.offsetHeight : 0) +
      4; // bordo body 1.5px top/bottom (3px) + 1px buffer

    const MIN_HEIGHT = 120;
    const MAX_HEIGHT = Math.round(window.screen.availHeight * 0.85);
    const clampedHeight = Math.max(MIN_HEIGHT, Math.min(naturalHeight, MAX_HEIGHT));

    const currentWidth = window.outerWidth || 480;

    invokeCmd("resize_self", { width: currentWidth, height: clampedHeight }).catch(() => {});
  });
}

// ── Event wiring ───────────────────────────────────────────────────────────

/**
 * Delegated click handler for the entire plugin content area.
 * Any click on an element (or its ancestor) that carries [data-evt]
 * is converted to a PluginUiEvent and forwarded to the orchestrator.
 *
 * Delegation avoids attaching per-element listeners to dynamically
 * injected HTML — one listener survives across HTML updates.
 */
pluginRoot.addEventListener("click", (ev) => {
  if (myWindowId === null) return;  // not yet initialised

  const result = eventFromTarget(ev.target);
  if (!result) return;              // click on non-interactive area

  emitUiEvent(result.element_id, result.value);
});

/**
 * Delegated dblclick handler — mirrors the click handler above, but reads
 * [data-dblevt] instead of [data-evt]. Opt-in per element: a plugin that
 * never renders data-dblevt simply never receives double-click events.
 *
 * Note: a double-click is preceded by a native single `click`, so a row that
 * carries BOTH data-evt (select) and data-dblevt (enter) will first select
 * itself, then this handler fires the double-click action — exactly the desired
 * "select + open" behaviour.
 */
pluginRoot.addEventListener("dblclick", (ev) => {
  if (myWindowId === null) return;  // not yet initialised

  const result = eventFromDblTarget(ev.target);
  if (!result) return;              // double-click on non-interactive area

  emitUiEvent(result.element_id, result.value);
});

/**
 * Delegated 'input' handler — forwards live text-field edits as the user
 * types. Reuses eventFromTarget (already reads .value for INPUT/SELECT).
 * Without this, a plugin-rendered <input data-evt="..."> never sends
 * anything to the plugin as the user types — state.rs's own handler for
 * this element_id existed but was unreachable (confirmed via a diagnostic
 * e2e that drove the real plugin process directly, bypassing this file).
 */
pluginRoot.addEventListener("input", (ev) => {
  if (myWindowId === null) return;
  const result = eventFromTarget(ev.target);
  if (!result) return;
  emitUiEvent(result.element_id, result.value);
});

/** Close button: notify orchestrator, then close the window. */
closeBtn.addEventListener("click", () => {
  if (myWindowId !== null) {
    emitWindowClosed();
  }
  closeSelf();
});

/**
 * Input da tastiera fisica: mappa un keydown sul pulsante con `data-key`
 * corrispondente e ne scatena il `data-evt` — stessa via del click.
 *
 * I pulsanti vengono riletti dal DOM ad ogni keydown (l'HTML è re-iniettato
 * ad ogni update; querySelectorAll è economico per una manciata di tasti).
 * `eventFromKey` (puro) fa il matching. Su match facciamo preventDefault per
 * sopprimere effetti del browser (es. "/" quick-find, Backspace navigazione).
 */
window.addEventListener("keydown", (ev) => {
  if (myWindowId === null) return;  // non ancora inizializzato

  // Un campo di testo attivo (es. il nome della nuova directory in Lare
  // Commander) possiede la tastiera per intero: Backspace/Delete/frecce/Home/
  // End devono editare il testo, MAI essere dirottati su un [data-key] (es.
  // "key:Backspace" = "sali di livello" nel pannello file). Senza questa
  // guardia, correggere un refuso con Backspace chiudeva silenziosamente il
  // dialog MkDir e scartava quanto digitato — trovato in review dal vivo.
  const active = document.activeElement;
  if (active && pluginRoot.contains(active) &&
      (active.tagName === "INPUT" || active.tagName === "TEXTAREA")) {
    return;
  }

  // Lascia passare le scorciatoie di sistema/browser (Ctrl/Cmd/Alt): es. Ctrl+0,
  // Ctrl++ (zoom), Ctrl+R (reload) non devono diventare input della calcolatrice.
  // NB: `shiftKey` NON è escluso — serve a digitare `* ( ) = /` (combo con Shift
  // su molti layout, incluso l'italiano); nessun simbolo della calcolatrice usa AltGr.
  if (ev.ctrlKey || ev.metaKey || ev.altKey) return;

  const buttons = Array.from(pluginRoot.querySelectorAll("[data-key]")).map((el) => ({
    dataKey: el.getAttribute("data-key") || "",
    dataEvt: el.getAttribute("data-evt") || "",
  }));

  const result = eventFromKey(ev.key, buttons);
  if (!result) return;              // tasto non mappato → lascia fare al browser

  ev.preventDefault();
  emitUiEvent(result.element_id, result.value);
});

// ── Incoming Tauri events ──────────────────────────────────────────────────

/**
 * "plugin:update" — re-render the window with new HTML.
 * Payload: { window_id: number, html: string }
 *
 * Global broadcast — filter by myWindowId so only the intended window reacts.
 */
tauriEvent.listen("plugin:update", (ev) => {
  const p = ev.payload;
  // Strict equality: both sides are JS numbers (p.window_id from JSON number,
  // myWindowId set via Number() below).  No type coercion — intentional.
  if (p.window_id !== myWindowId) return;
  render(p.html);
  growCalcDisplay();  // calcolatrice: display grow-only (high-water), PRIMA del fit
  autoFitHeight();   // ri-adatta l'altezza: il contenuto può crescere (es. frazione 2D)
});

/**
 * "plugin:close" — the orchestrator is telling this window to close.
 * Payload: { window_id: number }
 *
 * We do NOT emit "plugin:window-closed" back here: the orchestrator already
 * knows it triggered the close, so a round-trip event would be redundant
 * and could cause double-cleanup.
 */
tauriEvent.listen("plugin:close", (ev) => {
  const p = ev.payload;
  if (p.window_id !== myWindowId) return;
  closeSelf();
});

/**
 * "config:saved" — trasparenza configurabile (--window-alpha).
 * Broadcast globale Tauri emesso da config-window.js al salvataggio di
 * /config: raggiunge questa finestra anche se è già aperta, senza bisogno
 * di riaprirla (stesso meccanismo confermato in Task 4 su Library).
 */
tauriEvent.listen("config:saved", (ev) => {
  const alpha = ev.payload?.window_alpha;
  if (typeof alpha === "number") {
    document.documentElement.style.setProperty("--window-alpha", alpha);
  }
});

// ── Bootstrap ──────────────────────────────────────────────────────────────

/**
 * Called once at load time.  Fetches the initial content from the host
 * (stored in WindowContentStore by `open_plugin_window`) and renders it.
 *
 * The store entry is keyed by the Tauri window label and is consumed
 * exactly once (take = remove after read), preventing stale data.
 *
 * Returned shape: { title: string, content: string (HTML), kind: string (window_id) }
 */
async function init() {
  let data;
  try {
    data = await invokeCmd("take_window_content");
  } catch (err) {
    console.error("[plugin-window] take_window_content failed:", err);
    pluginRoot.textContent = "Error: could not load plugin content.";
    return;
  }

  if (!data) {
    console.error("[plugin-window] take_window_content returned null");
    pluginRoot.textContent = "Error: plugin content not found.";
    return;
  }

  // `kind` holds the window_id as a decimal string (stored as String in Rust).
  // Convert to a JS number now so all comparisons use strict numeric equality.
  myWindowId = Number(data.kind);
  if (!Number.isFinite(myWindowId)) {
    console.error("[plugin-window] Invalid window_id in kind:", data.kind);
    return;
  }

  // Update the titlebar label with the plugin-supplied title.
  if (data.title) {
    titlebarLabel.textContent = data.title;
    document.title = data.title;
  }

  // Trasparenza (--window-alpha): applicata prima del render così il primo
  // paint è già corretto (nessun flash al valore di default).
  try {
    const cfg = await invokeCmd("get_config");
    if (cfg && typeof cfg.window_alpha === "number") {
      document.documentElement.style.setProperty("--window-alpha", cfg.window_alpha);
    }
  } catch (e) {
    console.warn("[plugin-window] get_config on startup failed:", e);
  }

  // Render the initial HTML (may be empty string if the plugin hasn't
  // pushed content yet; the next "plugin:update" event will populate it).
  if (data.content) {
    render(data.content);
    growCalcDisplay();  // calcolatrice: display grow-only (high-water), PRIMA del fit
    autoFitHeight();   // adatta l'altezza al contenuto iniziale (poi anche su ogni update)
  }
}

init();
