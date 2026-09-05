/**
 * note-view.mjs — Modulo puro per la scheda Library "Note" (Blocco note).
 * Nessuna dipendenza da DOM, Tauri o I/O — testabile con `node:test`.
 * Design: Docs/superpowers/specs/2026-07-29-library-notes-design.md §10.
 */

// ---------------------------------------------------------------------------
// visibleNotesSorted(notes) → NoteView[]
//
// Filtra le note cancellate (l'orchestrator le tiene per il protocollo di
// riconciliazione — la UI non le mostra mai) e ordina per data di creazione
// decrescente (più recente prima).
// ---------------------------------------------------------------------------
export function visibleNotesSorted(notes) {
  return notes
    .filter((n) => !n.deleted)
    .slice()
    .sort((a, b) => b.created_at_ms - a.created_at_ms);
}

// ---------------------------------------------------------------------------
// isTitleValid(title) → bool
//
// Cap 200 caratteri (design §9), non vuoto (dopo trim).
// ---------------------------------------------------------------------------
export function isTitleValid(title) {
  const trimmed = title.trim();
  return trimmed.length > 0 && title.length <= 200;
}

// ---------------------------------------------------------------------------
// isBodyValid(text) → bool
//
// Cap 256 KB (design §9) — misurato in BYTE UTF-8, non caratteri JS (un
// carattere multi-byte pesa di più lato Rust/serde che lo riceverà).
// ---------------------------------------------------------------------------
const MAX_NOTE_BODY_BYTES = 256 * 1024;

export function isBodyValid(text) {
  return new TextEncoder().encode(text).length <= MAX_NOTE_BODY_BYTES;
}

// ---------------------------------------------------------------------------
// formatNoteMeta(createdBy, createdAtMs) → string
//
// Riga "macchina, data/ora" mostrata sotto il titolo di ogni nota nella lista.
// ---------------------------------------------------------------------------
export function formatNoteMeta(createdBy, createdAtMs) {
  const d = new Date(createdAtMs);
  return `${createdBy} — ${d.toLocaleString()}`;
}

// ---------------------------------------------------------------------------
// noteEditMessages({title, text, originalTitle, originalBodyText})
//   → { editText?, editTitle? }
//
// Decide quali messaggi mandare al salvataggio di "Modifica nota". Ogni
// campo viaggia SOLO se davvero cambiato rispetto al valore catturato
// all'apertura del dialog (design §10: due messaggi indipendenti sullo
// stesso NoteId, non un unico update incondizionato).
//
// Confronto grezzo, nessun trim: uno spazio aggiunto in coda è comunque una
// modifica reale (sia per il titolo sia per il corpo) e va propagata.
//
// `originalBodyText` è il segmento di QUESTA macchina precompilato nel
// dialog (`NoteView.my_segment_text`) — NON `body` (il corpo fuso di tutte
// le macchine, mai scritto dal dialog). Confrontare contro vuoto invece che
// contro l'originale sarebbe un bug: un utente che cancella deliberatamente
// tutto il testo per svuotare il proprio segmento vedrebbe il salvataggio
// ignorato in silenzio (bug reale, trovato nello smoke test dal vivo del
// 2026-07-31 dopo che il dialog ha iniziato a precompilare col testo vero).
// ---------------------------------------------------------------------------
export function noteEditMessages({ title, text, originalTitle, originalBodyText }) {
  const messages = {};
  if (text !== originalBodyText) {
    messages.editText = text;
  }
  if (title !== originalTitle) {
    messages.editTitle = title;
  }
  return messages;
}
