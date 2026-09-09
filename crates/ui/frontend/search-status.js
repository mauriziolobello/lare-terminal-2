// search-status.js — logica pura della label di stato per la finestra /find.
//
// SRP: dato lo stato corrente della ricerca, ritorna la stringa mostrata in
// #status. Nessun DOM, nessun I/O → testabile con node:test.

import { t } from "./i18n.mjs";

/**
 * @param {{ done?: boolean, stopped?: boolean, paused?: boolean, count?: number, truncated?: boolean }} [state]
 * @returns {string}
 */
export function statusLabel({ done = false, stopped = false, paused = false, count = 0, truncated = false } = {}) {
  // Priority: stopped > paused > done > default (active).
  if (stopped) {
    if (done) {
      return truncated
        ? t("search.status_interrupted_count_truncated", { count })
        : t("search.status_interrupted_count", { count });
    }
    return t("search.status_interrupted");
  }
  if (paused && !done) {
    return t("search.status_paused_count", { count });
  }
  if (done) {
    return truncated
      ? t("search.status_done_count_truncated", { count })
      : t("search.status_done_count", { count });
  }
  return t("search.status_searching");
}
