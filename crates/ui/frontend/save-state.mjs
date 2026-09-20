// save-state.mjs — logica pura (senza DOM, senza Tauri) per decidere se il
// click su "Salva" deve creare un nuovo documento in Library (primo salvataggio
// di questa finestra) o sovrascrivere quello già creato in precedenza (click
// successivi) — vedi window.js, wiring di #save-btn. Testabile con node:test.
//
// Perché un modulo a parte: window.js non ha copertura di test a livello DOM
// (debito noto del progetto — nessun jsdom/browser in questa suite). La
// decisione "quale comando Tauri chiamare" non dipende però dal DOM: dipende
// solo da "esiste già un filename salvato per QUESTA finestra?". Estrarla qui
// la rende testabile con node:test puro, come già fatto per table-sort.mjs e
// ui-local.mjs in questa stessa cartella.

/**
 * @param {string|null} savedFile — filename già restituito da un `archive_save`
 *   precedente in questa stessa finestra, o `null`/stringa vuota se non si è
 *   ancora salvato (una stringa vuota è trattata come "non salvato": un path
 *   vuoto non è un filename valido da passare ad `archive_update`).
 * @param {string} title
 * @param {string} content — il contenuto ATTUALE (più recente) da salvare.
 * @returns {{cmd: "archive_save"|"archive_update", args: object}}
 */
export function planSave(savedFile, title, content) {
  if (savedFile) {
    return { cmd: "archive_update", args: { file: savedFile, title, content } };
  }
  return { cmd: "archive_save", args: { title, content } };
}
