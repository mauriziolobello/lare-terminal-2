// search-status.js — logica pura della label di stato per la finestra /find.
//
// SRP: dato lo stato corrente della ricerca, ritorna la stringa mostrata in
// #status. Nessun DOM, nessun I/O → testabile con node:test (come line-editor.js).

/**
 * @param {{ done?: boolean, stopped?: boolean, paused?: boolean, count?: number, truncated?: boolean }} [state]
 * @returns {string}
 */
export function statusLabel({ done = false, stopped = false, paused = false, count = 0, truncated = false } = {}) {
  const suffix = truncated ? " (troncati)" : "";
  // Priority: stopped > paused > done > default (active).
  if (stopped) {
    return done ? `⏹ interrotta — ${count} risultati${suffix}` : "⏹ interrotta…";
  }
  if (paused && !done) {
    return `⏸ in pausa — ${count} risultati`;
  }
  return done ? `✅ ${count} risultati${suffix}` : "⏳ ricerca…";
}
