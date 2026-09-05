// screener-picker-list.mjs — logica PURA di selezione per la finestra
// picker screener (crates/ui/frontend/screener-picker.js). Zero dipendenze
// Tauri/DOM, testato con node:test — convenzione di progetto (CLAUDE.md:
// "Aggiungendo logica nel frontend, estraila in un modulo puro").
//
// Stato: { items: Array<{id,title,description}>, index: number }.
// index === -1 significa "nessuna selezione possibile" (items vuoto) — il
// chiamante (screener-picker.js) lo usa per mostrare lo stato vuoto
// (Docs/superpowers/specs/2026-08-11-markets-screener-registry-design.md §8).

/**
 * @param {Array<{id:string,title:string,description:string}>} items
 * @returns {{items: Array, index: number}}
 */
export function createPickerState(items) {
  return { items, index: items.length > 0 ? 0 : -1 };
}

/**
 * Sposta la selezione di `delta` posizioni (±1), con wraparound. No-op se
 * `items` è vuoto (index resta -1).
 * @param {{items: Array, index: number}} state
 * @param {number} delta
 * @returns {{items: Array, index: number}}
 */
export function moveSelection(state, delta) {
  const n = state.items.length;
  if (n === 0) return state;
  const next = ((state.index + delta) % n + n) % n; // wraparound anche per delta negativo
  return { items: state.items, index: next };
}

/**
 * @param {{items: Array, index: number}} state
 * @returns {{id:string,title:string,description:string}|null}
 */
export function selectedItem(state) {
  if (state.index < 0) return null;
  return state.items[state.index];
}
