// plugin-runtime.mjs — pure helpers for the generic plugin window.
//
// This module has NO browser globals (no `document`, no `window`, no Tauri).
// That makes every export here unit-testable with node:test and a few plain
// objects — the tests substitute the browser elements with stubs.
//
// Three exports:
//   eventFromTarget(target) → { element_id, value } | null
//   eventFromKey(key, buttons) → { element_id, value: null } | null
//   sanitizeAndRender(container, html, DOMPurify)  → void
//
// Dependency-injection note (OOP/SOLID, Open/Closed):
//   `sanitizeAndRender` accepts DOMPurify as a parameter instead of importing
//   it directly.  This keeps the function pure and decouples it from the
//   vendor bundle — the caller passes the loaded DOMPurify, so both the
//   browser-based usage and any future server-side renderer can work.

// ---------------------------------------------------------------------------
// eventFromTarget
//
// Called inside a delegated click listener on the plugin window's root.
//
// Algorithm:
//   1. Find the nearest ancestor (inclusive) that carries `[data-evt]`.
//      This is the "semantic element" the user meant to click; inner <span>/<img>
//      children that lack data-evt are transparent to the event system.
//   2. Read `element_id` from that element's `dataset.evt`.
//   3. Read `value` only for INPUT, SELECT, and TEXTAREA (they carry
//      user-supplied text). All other element types (BUTTON, DIV, …)
//      send `value: null`.
//
// Returns null if:
//   - `closest` is not a function (target is not a real Element), OR
//   - no ancestor with [data-evt] exists (the click was on bare decorative area).
//
// @param {EventTarget} target  The `event.target` from the delegated listener.
// @returns {{ element_id: string, value: string|null } | null}
// ---------------------------------------------------------------------------
export function eventFromTarget(target) {
  // Guard: if `closest` is missing (e.g. an SVG textNode or a synthetic stub
  // without the method), bail out immediately.
  if (typeof target?.closest !== "function") {
    return null;
  }

  // Walk up to the nearest element that declares a semantic event name.
  const el = target.closest("[data-evt]");
  if (!el) {
    // The click landed on a part of the DOM that has no associated event.
    return null;
  }

  const element_id = el.dataset.evt;

  // INPUT, SELECT, and TEXTAREA elements carry meaningful user-supplied
  // text. BUTTON, A, SPAN, DIV, etc. are triggers — their "value" is
  // irrelevant. TEXTAREA added 2026-07-19 (plugin-crypto's two text
  // panels are multi-line — an <input> would work mechanically but wrap
  // poorly for longer ciphertext/plaintext).
  const isValueBearing = el.tagName === "INPUT" || el.tagName === "SELECT" || el.tagName === "TEXTAREA";
  const value = isValueBearing ? el.value : null;

  return { element_id, value };
}

// ---------------------------------------------------------------------------
// eventFromDblTarget
//
// Called inside a delegated DOUBLE-click listener on the plugin window's root.
// It is the double-click twin of `eventFromTarget`, but reads `[data-dblevt]`
// instead of `[data-evt]`.
//
// Why a separate attribute (not reuse data-evt)?
//   Single-click and double-click are DISTINCT interactions.  Reusing data-evt
//   would make every clickable element also fire on double-click, which is
//   almost never what a plugin wants.  With a dedicated `data-dblevt`, a plugin
//   opts IN per element: an element carrying only `data-evt` is deaf to
//   double-clicks, exactly as an element with no data-evt is deaf to clicks.
//
// Algorithm (mirrors eventFromTarget):
//   1. Find the nearest ancestor (inclusive) carrying `[data-dblevt]`.
//   2. Read `element_id` from that element's `dataset.dblevt`.
//   3. Value is always null — double-click targets in this codebase are rows /
//      buttons, never INPUT/SELECT, so there is no user-supplied text to carry.
//
// Returns null if:
//   - `closest` is not a function (target is not a real Element), OR
//   - no ancestor with [data-dblevt] exists.
//
// @param {EventTarget} target  The `event.target` from the delegated listener.
// @returns {{ element_id: string, value: null } | null}
// ---------------------------------------------------------------------------
export function eventFromDblTarget(target) {
  // Guard: if `closest` is missing (non-Element target), bail out immediately.
  if (typeof target?.closest !== "function") {
    return null;
  }

  // Walk up to the nearest element that declares a double-click event name.
  const el = target.closest("[data-dblevt]");
  if (!el) {
    // The double-click landed on an element with no double-click wiring.
    return null;
  }

  const element_id = el.dataset.dblevt;
  return { element_id, value: null };
}

// ---------------------------------------------------------------------------
// eventFromKey
//
// Mappa un tasto fisico (KeyboardEvent.key) sul pulsante plugin che deve
// attivare, così tastiera e mouse condividono lo stesso percorso evento.
//
// `buttons` è una lista di { dataKey, dataEvt } estratta dagli elementi
// [data-key] del DOM:  dataKey è una lista di KeyboardEvent.key separati da
// spazio (es. "= Enter"), dataEvt è il data-evt da emettere.
//
// Ritorna { element_id: dataEvt, value: null } per il PRIMO pulsante i cui
// token di dataKey includono `key`; altrimenti null (tasto non mappato →
// il chiamante lascia l'evento al browser).
//
// Puro: nessun accesso al DOM → testabile con node:test e oggetti semplici.
//
// @param {string} key       KeyboardEvent.key dell'evento keydown.
// @param {Array<{dataKey:string, dataEvt:string}>} buttons
// @returns {{ element_id: string, value: null } | null}
// ---------------------------------------------------------------------------
export function eventFromKey(key, buttons) {
  // Guard: chiave vuota/assente o lista non valida → nessun match.
  if (key == null || key === "" || !Array.isArray(buttons)) {
    return null;
  }
  for (const b of buttons) {
    if (!b || typeof b.dataKey !== "string" || typeof b.dataEvt !== "string") {
      continue;
    }
    // I token sono separati da spazio; `filter` scarta eventuali spazi doppi.
    const tokens = b.dataKey.split(" ").filter((t) => t.length > 0);
    if (tokens.includes(key)) {
      return { element_id: b.dataEvt, value: null };
    }
  }
  return null;
}

// ---------------------------------------------------------------------------
// sanitizeAndRender
//
// Replaces the content of `container` with the sanitized version of `html`.
//
// DOMPurify strips dangerous attributes (onclick, onerror, …) while keeping
// structural classes and our custom `data-evt` attribute (passed via ADD_ATTR).
// The class attribute is kept by default; `data-*` attributes are NOT kept by
// default — hence ADD_ATTR.  `autofocus` is likewise NOT in this DOMPurify
// build's default attribute allowlist (confirmed by direct inspection of the
// vendored `vendor/purify.min.js`: the literal string "autofocus" does not
// appear in it), so it too must be listed in ADD_ATTR or it is silently
// stripped — that is what gives a freshly-rendered dialog input (e.g. Lare
// Commander's MkDir field) its initial focus.
//
// Full-replace strategy (not a patch): plugins always send the complete new
// HTML for a window; there is no diffing.  This keeps the implementation
// simple and the protocol additive (ADR-007 pattern: no mutations, just updates).
//
// @param {HTMLElement} container   The DOM element whose innerHTML is replaced.
// @param {string}      html        Raw HTML from the plugin.
// @param {object}      DOMPurify   The loaded DOMPurify instance (injected).
// ---------------------------------------------------------------------------
export function sanitizeAndRender(container, html, DOMPurify) {
  // DOMPurify.sanitize() returns a safe HTML string.
  // ADD_ATTR keeps `data-evt`, `data-key` AND `autofocus` (all stripped by
  // default by this build's allowlist).
  //   data-evt   → nome dell'evento plugin emesso al click.
  //   data-key   → lista di KeyboardEvent.key che attivano il pulsante (input tastiera).
  //   autofocus  → dà il focus iniziale a un input al primo render (es. campo
  //                MkDir di Lare Commander); non nell'allowlist di default di
  //                questo bundle DOMPurify → va aggiunto qui esplicitamente.
  // FORCE_BODY ensures fragments like "<button>…</button>" are wrapped correctly.
  const clean = DOMPurify.sanitize(html, {
    ADD_ATTR: ["data-evt", "data-key", "autofocus"],
    FORCE_BODY: true,
  });

  // Replace the entire content atomically — no partial update.
  container.innerHTML = clean;
}

// ---------------------------------------------------------------------------
// escapeForAttrSelector (privata, non esportata)
//
// Sfugge una stringa perché sia inseribile in tutta sicurezza dentro il
// valore TRA VIRGOLETTE di un selettore CSS ad attributo, es. `[data-evt="…"]`.
//
// Il browser espone `CSS.escape()` per questo scopo, ma è pensato per
// produrre IDENTIFICATORI CSS validi (escapa molti più caratteri del
// necessario, es. `:`), e soprattutto NON esiste come global in Node — questo
// modulo deve restare privo di globali browser per restare testabile con
// `node:test` senza jsdom (vedi il commento di testa del file). Dentro un
// valore già tra virgolette, gli unici due caratteri davvero pericolosi sono
// la virgoletta stessa (chiuderebbe la stringa in anticipo) e il backslash
// (l'escape character): li raddoppiamo con un backslash davanti.
//
// @param {string} value
// @returns {string}
// ---------------------------------------------------------------------------
function escapeForAttrSelector(value) {
  return String(value).replace(/[\\"]/g, (ch) => `\\${ch}`);
}

// ---------------------------------------------------------------------------
// restoreActiveField
//
// Dopo un re-render (sanitizeAndRender sostituisce SEMPRE l'intero DOM, mai
// un patch parziale — vedi il commento sopra sanitizeAndRender), l'elemento
// che aveva il focus è stato distrutto e ricreato da zero. Questa funzione
// ritrova il "nuovo" elemento equivalente (stessa identità semantica: stesso
// `data-evt`) dentro `newRootElement` e gli ripristina TRE cose che
// andrebbero altrimenti perse:
//
//   1. il VALORE — non quello arrivato nell'HTML fresco del plugin (che, in
//      un round-trip tasto-per-tasto, riflette sempre lo stato di UN TASTO
//      PRIMA rispetto a quanto l'utente ha già digitato localmente), ma il
//      valore LIVE catturato da `active.value` un istante prima del render.
//      Bug che questo risolve: senza questo passo, ogni carattere digitato
//      appariva per un istante e spariva subito, sovrascritto dal valore "in
//      ritardo" del tasto precedente (osservato dal vivo nel pannello "Testo
//      in chiaro" del plugin crittografia).
//   2. il FOCUS.
//   3. la SELECTION RANGE (posizione del cursore).
//
// ORDINE DELLE OPERAZIONI — non è un dettaglio: il valore va scritto PRIMA
// di chiamare setSelectionRange. Scrivere `.value` su un <input>/<textarea>
// reale può normalizzare o troncare una selectionRange impostata in
// precedenza (il browser non garantisce che sopravviva a un cambio di
// value), quindi impostarla prima sarebbe silenziosamente vanificato.
//
// Pura per costruzione: non tocca window/document/CSS.escape — riceve tutto
// ciò che le serve come parametri, incluso l'elemento radice in cui cercare
// (già sostituito dal chiamante tramite sanitizeAndRender). Questo la rende
// testabile con node:test e semplici oggetti fake, esattamente come le altre
// funzioni di questo modulo.
//
// @param {{
//   activeDatasetEvt: string,    // data-evt dell'elemento CHE AVEVA il focus (identità semantica, sopravvive al render)
//   activeValue: string,         // il valore LIVE catturato prima del render
//   activeStart: number,         // selectionStart catturato prima del render
//   activeEnd: number,           // selectionEnd catturato prima del render
//   newRootElement: { querySelector: function },  // radice DOVE cercare il nuovo elemento
// } | null | undefined} preserved
// @returns {void}
// ---------------------------------------------------------------------------
export function restoreActiveField(preserved) {
  // Nessun campo era a fuoco prima del render, o non abbiamo un'identità
  // semantica (data-evt) per ritrovarlo → niente da ripristinare.
  if (!preserved || !preserved.activeDatasetEvt) {
    return;
  }

  const { activeDatasetEvt, activeValue, activeStart, activeEnd, newRootElement } = preserved;

  if (!newRootElement || typeof newRootElement.querySelector !== "function") {
    return;
  }

  const selector = `[data-evt="${escapeForAttrSelector(activeDatasetEvt)}"]`;
  const next = newRootElement.querySelector(selector);

  // L'elemento potrebbe non esserci più (il plugin ha smesso di renderlo),
  // oppure — difensivamente — potrebbe non essere un campo di testo (stesso
  // data-evt riusato su un tipo diverso di elemento non dovrebbe accadere,
  // ma non è questa funzione a doverlo assumere).
  if (!next || (next.tagName !== "INPUT" && next.tagName !== "TEXTAREA")) {
    return;
  }

  // 1) VALORE — sempre PRIMA della selection range (vedi commento sopra).
  if (typeof activeValue === "string") {
    next.value = activeValue;
  }

  // 2) FOCUS.
  if (typeof next.focus === "function") {
    next.focus();
  }

  // 3) SELECTION RANGE — per ultima, dopo che .value è già stato scritto.
  if (typeof next.setSelectionRange === "function") {
    next.setSelectionRange(activeStart, activeEnd);
  }
}
