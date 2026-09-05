// plugin-runtime.test.mjs — unit tests for the pure helpers in plugin-runtime.mjs.
//
// Run with: node --test crates/ui/frontend/plugin-runtime.test.mjs
//
// These tests use the node:test runner (built-in since Node 18) and synthetic
// DOM stubs — no browser, no bundler, no dependencies.
//
// What we test here: `eventFromTarget` (pure function, easy to unit-test
// without a browser). `sanitizeAndRender` is tested only conceptually via a
// fake DOMPurify — its real behaviour is covered by GUI acceptance.

import { test } from "node:test";
import assert from "node:assert/strict";

// The module under test (must be importable without a browser).
import { eventFromTarget, eventFromKey, eventFromDblTarget, sanitizeAndRender, restoreActiveField } from "./plugin-runtime.mjs";

// ---------------------------------------------------------------------------
// eventFromTarget — extracts { element_id, value } from a click target.
//
// The function climbs to the nearest ancestor that carries [data-evt], then
// reads the element_id from `dataset.evt` and a value only for INPUT/SELECT.
// ---------------------------------------------------------------------------

test("reads data-evt from the clicked element itself", () => {
  // Synthetic "button" element that has data-evt directly on it.
  const btn = {
    dataset: { evt: "inc" },
    closest: (selector) => selector === "[data-evt]" ? btn : null,
    tagName: "BUTTON",
  };
  assert.deepEqual(eventFromTarget(btn), { element_id: "inc", value: null });
});

test("climbs to an ancestor that carries data-evt", () => {
  // Simulate a <span> inside a <button data-evt="inc">.
  // The span itself has NO dataset.evt; closest() returns the parent button.
  const btn = {
    dataset: { evt: "dec" },
    tagName: "BUTTON",
  };
  // closest() is called on the span — returns the parent button.
  const span = {
    dataset: {},
    closest: (selector) => selector === "[data-evt]" ? btn : null,
    tagName: "SPAN",
  };
  assert.deepEqual(eventFromTarget(span), { element_id: "dec", value: null });
});

test("INPUT element includes its value in the result", () => {
  const inp = {
    dataset: { evt: "quantity" },
    closest: (selector) => selector === "[data-evt]" ? inp : null,
    tagName: "INPUT",
    value: "42",
  };
  assert.deepEqual(eventFromTarget(inp), { element_id: "quantity", value: "42" });
});

test("SELECT element includes its selected value", () => {
  const sel = {
    dataset: { evt: "color" },
    closest: (selector) => selector === "[data-evt]" ? sel : null,
    tagName: "SELECT",
    value: "blue",
  };
  assert.deepEqual(eventFromTarget(sel), { element_id: "color", value: "blue" });
});

test("TEXTAREA element includes its value in the result", () => {
  const ta = {
    dataset: { evt: "plaintext" },
    closest: (selector) => selector === "[data-evt]" ? ta : null,
    tagName: "TEXTAREA",
    value: "riga uno\nriga due",
  };
  assert.deepEqual(eventFromTarget(ta), { element_id: "plaintext", value: "riga uno\nriga due" });
});

test("element with no data-evt ancestor returns null", () => {
  const x = {
    dataset: {},
    closest: () => null,
    tagName: "DIV",
  };
  assert.equal(eventFromTarget(x), null);
});

test("target without a closest() method returns null gracefully", () => {
  // Defensive: if target has no closest (e.g. SVG or non-element), return null.
  const svg = { dataset: {}, tagName: "SVG" };
  assert.equal(eventFromTarget(svg), null);
});

// ---------------------------------------------------------------------------
// eventFromKey — mappa un tasto fisico (KeyboardEvent.key) sul pulsante plugin
// che deve attivare, così tastiera e mouse condividono lo stesso percorso evento.
// ---------------------------------------------------------------------------

// Lista pulsanti tipica della calcolatrice (sottoinsieme sufficiente ai test).
const CALC_BUTTONS = [
  { dataKey: "7", dataEvt: "d7" },
  { dataKey: ". ,", dataEvt: "dot" },
  { dataKey: "* x", dataEvt: "op_mul" },
  { dataKey: "= Enter", dataEvt: "eq" },
  { dataKey: "Backspace", dataEvt: "back" },
  { dataKey: "Escape Delete", dataEvt: "clear" },
];

test("eventFromKey: cifra → data-evt corrispondente", () => {
  assert.deepEqual(eventFromKey("7", CALC_BUTTONS), { element_id: "d7", value: null });
});

test("eventFromKey: alias multipli (Enter e = → eq)", () => {
  assert.deepEqual(eventFromKey("Enter", CALC_BUTTONS), { element_id: "eq", value: null });
  assert.deepEqual(eventFromKey("=", CALC_BUTTONS), { element_id: "eq", value: null });
});

test("eventFromKey: alias x e * → op_mul; , e . → dot", () => {
  assert.equal(eventFromKey("x", CALC_BUTTONS).element_id, "op_mul");
  assert.equal(eventFromKey("*", CALC_BUTTONS).element_id, "op_mul");
  assert.equal(eventFromKey(",", CALC_BUTTONS).element_id, "dot");
});

test("eventFromKey: tasti speciali per nome", () => {
  assert.equal(eventFromKey("Backspace", CALC_BUTTONS).element_id, "back");
  assert.equal(eventFromKey("Escape", CALC_BUTTONS).element_id, "clear");
});

test("eventFromKey: tasto non mappato → null", () => {
  assert.equal(eventFromKey("q", CALC_BUTTONS), null);
  assert.equal(eventFromKey("F1", CALC_BUTTONS), null);
});

test("eventFromKey: input degenere → null (niente crash)", () => {
  assert.equal(eventFromKey("", CALC_BUTTONS), null);
  assert.equal(eventFromKey(null, CALC_BUTTONS), null);
  assert.equal(eventFromKey("7", null), null);
  assert.equal(eventFromKey("7", []), null);
});

// ---------------------------------------------------------------------------
// eventFromDblTarget — extracts { element_id, value:null } from a DOUBLE-click
// target, reading [data-dblevt] (parallel to [data-evt] for single clicks).
//
// This mirrors eventFromTarget's delegation algorithm but is a SEPARATE, opt-in
// attribute: an element wired only for single-click (data-evt) must NOT respond
// to a double-click — the two attributes are independent by design.
// ---------------------------------------------------------------------------

test("eventFromDblTarget: reads data-dblevt from the double-clicked element itself", () => {
  // Synthetic row element carrying data-dblevt directly on it.
  const row = {
    dataset: { dblevt: "key:Enter" },
    closest: (selector) => selector === "[data-dblevt]" ? row : null,
    tagName: "DIV",
  };
  assert.deepEqual(eventFromDblTarget(row), { element_id: "key:Enter", value: null });
});

test("eventFromDblTarget: climbs to an ancestor that carries data-dblevt", () => {
  // Simulate a <span> inside a <div data-dblevt="key:Enter"> — delegation.
  const row = {
    dataset: { dblevt: "key:Enter" },
    tagName: "DIV",
  };
  const span = {
    dataset: {},
    closest: (selector) => selector === "[data-dblevt]" ? row : null,
    tagName: "SPAN",
  };
  assert.deepEqual(eventFromDblTarget(span), { element_id: "key:Enter", value: null });
});

test("eventFromDblTarget: element with data-evt but NO data-dblevt returns null", () => {
  // A button wired only for single-click must NOT fire on double-click.
  // closest("[data-dblevt]") returns null because only data-evt is present.
  const btn = {
    dataset: { evt: "inc" },     // single-click wiring only
    closest: (selector) => selector === "[data-dblevt]" ? null : btn,
    tagName: "BUTTON",
  };
  assert.equal(eventFromDblTarget(btn), null);
});

test("eventFromDblTarget: element with neither attribute returns null", () => {
  const x = {
    dataset: {},
    closest: () => null,
    tagName: "DIV",
  };
  assert.equal(eventFromDblTarget(x), null);
});

test("eventFromDblTarget: non-Element target (no closest) returns null gracefully", () => {
  const textNode = { dataset: {}, tagName: "#text" };
  assert.equal(eventFromDblTarget(textNode), null);
});

// ---------------------------------------------------------------------------
// sanitizeAndRender — replaces container.innerHTML with the sanitized HTML.
//
// The real DOMPurify is a browser bundle, so (following this function's own
// dependency-injection design) we pass a FAKE DOMPurify that models the ONE
// behaviour that matters here: it keeps only attributes that are either in a
// baseline allowlist OR explicitly added via `opts.ADD_ATTR`. This mirrors the
// vendored bundle, where `autofocus`/`data-evt`/`data-key` are NOT in the
// default allowlist and would be stripped unless listed in ADD_ATTR.
// ---------------------------------------------------------------------------

/**
 * Fake DOMPurify stub. `notAllowedByDefault` is the set of attribute names this
 * "build" does NOT keep by default (like `autofocus`/`data-*` in the real
 * vendored bundle): sanitize() strips each such attribute UNLESS it appears in
 * `opts.ADD_ATTR`. Operates on the controlled test markup only (not arbitrary
 * HTML), so a small regex is sufficient and deterministic.
 */
function fakeDOMPurify(notAllowedByDefault) {
  return {
    sanitize(html, opts = {}) {
      const added = new Set(opts.ADD_ATTR || []);
      let out = html;
      for (const attr of notAllowedByDefault) {
        if (added.has(attr)) continue; // preserved because it's in ADD_ATTR
        // Remove both the boolean form (` autofocus`) and the valued form
        // (` data-evt="..."`). Leading \s keeps us from mangling other names.
        const re = new RegExp(`\\s${attr}(="[^"]*")?`, "g");
        out = out.replace(re, "");
      }
      return out;
    },
  };
}

test("sanitizeAndRender: keeps autofocus on the element (via ADD_ATTR)", () => {
  // The MkDir input relies on the native `autofocus` attribute to get focus the
  // FIRST time the dialog appears. Since this DOMPurify build drops `autofocus`
  // by default (confirmed by direct inspection of the vendored bundle),
  // sanitizeAndRender MUST list it in ADD_ATTR or the field never auto-focuses.
  const container = { innerHTML: "" };
  const html = `<input class="mc-mkdir-input" autofocus data-evt="dialog:mkdir:input">`;
  // The fake drops both `autofocus` and `data-evt` unless ADD_ATTR keeps them.
  sanitizeAndRender(container, html, fakeDOMPurify(["autofocus", "data-evt"]));
  assert.match(container.innerHTML, /autofocus/,
    "autofocus deve sopravvivere alla sanitizzazione (deve essere in ADD_ATTR)");
  // Sanity: data-evt (already in ADD_ATTR before this round) is still kept too.
  assert.match(container.innerHTML, /data-evt="dialog:mkdir:input"/,
    "data-evt deve restare preservato (regressione)");
});

// ---------------------------------------------------------------------------
// restoreActiveField — dopo un re-render (sostituzione integrale del DOM via
// sanitizeAndRender), ritrova l'elemento che aveva il focus e gli ripristina
// focus + VALORE LIVE + posizione del cursore.
//
// Bug che questa funzione risolve (osservato dal vivo nel pannello "Testo in
// chiaro" del plugin crittografia): ogni tasto premuto fa un round-trip
// completo attraverso il sidecar plugin, e l'HTML che torna riflette sempre
// lo stato di UN TASTO PRIMA rispetto a quanto l'utente ha già digitato
// localmente. Senza questa funzione, `render()` ritrovava il campo e gli
// ripristinava SOLO focus+selectionRange, lasciando il `.value` a quello
// "in ritardo" arrivato nell'HTML fresco — ogni carattere appariva per un
// istante e spariva subito, sovrascritto dal round-trip del tasto precedente.
//
// La funzione è volutamente PURA (nessun accesso a window/document/CSS):
// riceve un elemento radice "nuovo" (già sostituito da sanitizeAndRender) e
// vi cerca l'elemento corrispondente tramite `newRootElement.querySelector`,
// che nei test è un fake — non il DOM reale del browser.
// ---------------------------------------------------------------------------

/**
 * Costruisce un fake "root" con un solo elemento figlio che risponde a
 * querySelector per un selettore `[data-evt="<evt>"]` esatto — sufficiente
 * per testare restoreActiveField senza un DOM reale (stesso stile delle
 * altre fake in questo file: oggetti semplici con i soli metodi/campi usati
 * dal codice sotto test).
 */
function fakeRootWithElement(evt, el) {
  return {
    querySelector(selector) {
      return selector === `[data-evt="${evt}"]` ? el : null;
    },
  };
}

/** Costruisce un elemento INPUT/TEXTAREA fake che traccia value/focus/selection. */
function fakeField(tagName, initialValue, calls) {
  let _value = initialValue;
  const el = {
    tagName,
    get value() { return _value; },
    set value(v) { calls.push(["value", v]); _value = v; },
    focus() { calls.push(["focus"]); },
    setSelectionRange(start, end) { calls.push(["setSelectionRange", start, end]); },
  };
  return el;
}

test("restoreActiveField: scrive il valore LIVE catturato, non quello nel nuovo DOM", () => {
  const calls = [];
  // Il nuovo elemento arriva dal render col valore "in ritardo di un tasto"
  // (quello che il round-trip col plugin ha ritornato).
  const next = fakeField("TEXTAREA", "ciao mond", calls);
  const newRoot = fakeRootWithElement("field:plaintext", next);

  restoreActiveField({
    activeDatasetEvt: "field:plaintext",
    activeValue: "ciao mondo",   // quello che l'utente ha DAVVERO digitato
    activeStart: 10,
    activeEnd: 10,
    newRootElement: newRoot,
  });

  assert.equal(next.value, "ciao mondo",
    "il valore live digitato dall'utente deve sopravvivere al re-render, non quello in ritardo");
});

test("restoreActiveField: ripristina focus e selectionRange oltre al valore", () => {
  const calls = [];
  const next = fakeField("INPUT", "abc", calls);
  const newRoot = fakeRootWithElement("field:key", next);

  restoreActiveField({
    activeDatasetEvt: "field:key",
    activeValue: "abcd",
    activeStart: 4,
    activeEnd: 4,
    newRootElement: newRoot,
  });

  assert.deepEqual(
    calls.filter((c) => c[0] === "setSelectionRange"),
    [["setSelectionRange", 4, 4]],
  );
  assert.deepEqual(calls.filter((c) => c[0] === "focus"), [["focus"]]);
});

test("restoreActiveField: scrive .value PRIMA di setSelectionRange (l'ordine conta)", () => {
  // Scrivere .value su un <input>/<textarea> reale può normalizzare o troncare
  // una selectionRange impostata in precedenza — quindi l'ordine delle
  // operazioni non è un dettaglio, è parte del contratto della funzione.
  const calls = [];
  const next = fakeField("TEXTAREA", "old", calls);
  const newRoot = fakeRootWithElement("field:plaintext", next);

  restoreActiveField({
    activeDatasetEvt: "field:plaintext",
    activeValue: "new value",
    activeStart: 9,
    activeEnd: 9,
    newRootElement: newRoot,
  });

  const valueIdx = calls.findIndex((c) => c[0] === "value");
  const selectionIdx = calls.findIndex((c) => c[0] === "setSelectionRange");
  assert.ok(valueIdx !== -1 && selectionIdx !== -1, "entrambe le operazioni devono avvenire");
  assert.ok(valueIdx < selectionIdx,
    "il valore deve essere scritto PRIMA di setSelectionRange, altrimenti la selection può essere troncata/normalizzata");
});

test("restoreActiveField: preserved nullo → no-op, nessun crash", () => {
  assert.doesNotThrow(() => restoreActiveField(null));
  assert.doesNotThrow(() => restoreActiveField(undefined));
});

test("restoreActiveField: nessun elemento corrispondente nel nuovo DOM → no-op", () => {
  const newRoot = { querySelector: () => null };
  assert.doesNotThrow(() => restoreActiveField({
    activeDatasetEvt: "field:plaintext",
    activeValue: "qualcosa",
    activeStart: 0,
    activeEnd: 0,
    newRootElement: newRoot,
  }));
});

test("restoreActiveField: activeDatasetEvt mancante → no-op (niente da ritrovare)", () => {
  let queried = false;
  const newRoot = { querySelector: () => { queried = true; return null; } };
  restoreActiveField({
    activeDatasetEvt: undefined,
    activeValue: "qualcosa",
    activeStart: 0,
    activeEnd: 0,
    newRootElement: newRoot,
  });
  assert.equal(queried, false, "senza un evt noto non ha senso nemmeno interrogare il DOM");
});

test("restoreActiveField: elemento trovato ma non è INPUT/TEXTAREA → no-op difensivo", () => {
  // Un DIV che per qualche motivo condivide lo stesso data-evt non deve mai
  // ricevere .value/focus/setSelectionRange (non li possiede nemmeno).
  const div = { tagName: "DIV" };
  const newRoot = fakeRootWithElement("field:plaintext", div);
  assert.doesNotThrow(() => restoreActiveField({
    activeDatasetEvt: "field:plaintext",
    activeValue: "qualcosa",
    activeStart: 0,
    activeEnd: 0,
    newRootElement: newRoot,
  }));
});

test("restoreActiveField: un data-evt con virgolette non rompe il selettore (escaping difensivo)", () => {
  const calls = [];
  const next = fakeField("INPUT", "x", calls);
  // Il selettore costruito internamente deve escapare la virgoletta, così
  // `newRoot.querySelector` riceve un selettore sintatticamente valido.
  const newRoot = {
    querySelector(selector) {
      assert.equal(selector, `[data-evt="foo\\"bar"]`);
      return next;
    },
  };
  restoreActiveField({
    activeDatasetEvt: 'foo"bar',
    activeValue: "y",
    activeStart: 1,
    activeEnd: 1,
    newRootElement: newRoot,
  });
  assert.equal(next.value, "y");
});
