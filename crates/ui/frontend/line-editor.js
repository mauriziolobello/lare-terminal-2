// line-editor.js — Pure line-editor state machine for Lare Terminal.
//
// State: { text: string, caret: number }
//   - `text`  is the current line content
//   - `caret` is the insertion point (0..text.length), inclusive on both ends
//
// All exported functions are PURE: they return a new state object and never
// mutate the argument.  This makes them trivially unit-testable with node:test
// and composable without side effects.
//
// Clipboard operations (Ctrl+C/V) are handled in app.js (async/browser API),
// not here — this module has zero DOM/async dependencies.

/**
 * Insert `str` at the caret, advancing the caret to end of insertion.
 *
 * @param {{ text: string, caret: number }} state
 * @param {string} str
 * @returns {{ text: string, caret: number }}
 */
export function insert(state, str) {
  const { text, caret } = state;
  return {
    text: text.slice(0, caret) + str + text.slice(caret),
    caret: caret + str.length,
  };
}

/**
 * Delete the character immediately before the caret (no-op if caret === 0).
 *
 * @param {{ text: string, caret: number }} state
 * @returns {{ text: string, caret: number }}
 */
export function backspace(state) {
  const { text, caret } = state;
  if (caret === 0) return state;
  return {
    text: text.slice(0, caret - 1) + text.slice(caret),
    caret: caret - 1,
  };
}

/**
 * Delete the character at the caret position (no-op if caret === text.length).
 *
 * @param {{ text: string, caret: number }} state
 * @returns {{ text: string, caret: number }}
 */
export function deleteForward(state) {
  const { text, caret } = state;
  if (caret === text.length) return state;
  return {
    text: text.slice(0, caret) + text.slice(caret + 1),
    caret,
  };
}

/**
 * Move the caret one position to the left (clamp to 0).
 *
 * @param {{ text: string, caret: number }} state
 * @returns {{ text: string, caret: number }}
 */
export function left(state) {
  const { text, caret } = state;
  return { text, caret: Math.max(0, caret - 1) };
}

/**
 * Move the caret one position to the right (clamp to text.length).
 *
 * @param {{ text: string, caret: number }} state
 * @returns {{ text: string, caret: number }}
 */
export function right(state) {
  const { text, caret } = state;
  return { text, caret: Math.min(text.length, caret + 1) };
}

/**
 * Move the caret to the beginning of the line (position 0).
 *
 * @param {{ text: string, caret: number }} state
 * @returns {{ text: string, caret: number }}
 */
export function home(state) {
  return { text: state.text, caret: 0 };
}

/**
 * Move the caret to the end of the line (position text.length).
 *
 * @param {{ text: string, caret: number }} state
 * @returns {{ text: string, caret: number }}
 */
export function end(state) {
  return { text: state.text, caret: state.text.length };
}

/**
 * Clear the line: empty text, caret at 0.
 *
 * @param {{ text: string, caret: number }} _state
 * @returns {{ text: string, caret: number }}
 */
export function clear(_state) {
  return { text: "", caret: 0 };
}
