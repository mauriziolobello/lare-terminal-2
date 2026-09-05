// expand-prompt.mjs — composizione pura del testo del turno per il canale
// "library-expand" (Docs/superpowers/specs/2026-07-20-library-expand-design.md).
//
// Il documento attuale e la richiesta dell'utente NON possono stare nel
// system prompt del canale (fisso, &'static str lato Rust) — viaggiano nel
// testo del messaggio utente di quel turno, esattamente come qualunque
// richiesta NL normale. Questa funzione è pura (nessuna dipendenza da
// Tauri/WS) per essere testabile in isolamento.

/**
 * @param {string} currentContent - Il Markdown del documento così com'è ora.
 * @param {string} request        - La richiesta di ampliamento dell'utente.
 * @returns {string} Il testo completo da mandare come Command.input.
 */
export function buildExpandPrompt(currentContent, request) {
  return `Documento attuale (Markdown):\n---\n${currentContent}\n---\n\nRichiesta di espansione: ${request}`;
}

/**
 * Decide whether a LareWsClient status change should be treated as a
 * terminal failure for an in-flight "espandi" request. LareWsClient
 * retries indefinitely on drop (no cap) — correct for the long-lived
 * cursor connection, wrong for this one-shot request: without this check,
 * a lost connection would either strand the UI forever, or a later
 * automatic reconnect would silently re-send the same prompt as a fresh
 * AI turn.
 *
 * @param {string} status - LareWsClient onStatus value.
 * @param {boolean} alreadySettled - whether this request already reached
 *   a terminal outcome (done/error/a prior status-triggered failure).
 * @returns {boolean} true if this status should end the request as a failure.
 */
export function isConnectionFailureStatus(status, alreadySettled) {
  if (alreadySettled) return false;
  return status === "error" || status === "disconnected";
}

/**
 * An AI turn that completes with zero (or whitespace-only) content is a
 * degenerate response, not a real answer — writing it to disk would
 * silently wipe the document (no backup exists by design). Treat it as a
 * failure instead of a successful empty overwrite.
 *
 * @param {string} content - the accumulated AI response text.
 * @returns {boolean} true if the response should be rejected as empty.
 */
export function isEmptyExpandResult(content) {
  return content.trim().length === 0;
}

/**
 * A `Done` message whose `exit_code` is non-null signals an AI-turn failure
 * (backend/API error, refusal, or degenerate response — see `ai_adapter.rs`'s
 * `fail()` closure), NOT a successful completion. `exit_code: null` is what a
 * clean AI text response emits (existing convention already used elsewhere
 * in this codebase, e.g. `renderer.js`'s `exitCode === null` check for the
 * cursor UI). Any non-null value (`1` for AI failures, `130` for a
 * user-cancelled turn) means "do not trust the buffer as real content".
 *
 * @param {number|null|undefined} exitCode - ServerMsg::Done's exit_code field.
 * @returns {boolean} true if this Done represents a failed AI turn.
 */
export function isAiTurnFailure(exitCode) {
  return exitCode !== null && exitCode !== undefined;
}
