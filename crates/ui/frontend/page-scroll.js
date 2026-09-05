// page-scroll.js — pure helper for keyboard page-scroll of a scroll area. No DOM/IO.

/**
 * Pixel da scorrere per un passo PageUp/PageDown: una "pagina" meno un piccolo
 * overlap di contesto, con un minimo sensato.
 * @param {number} clientHeight  altezza visibile del contenitore (px)
 * @param {number} [overlap=24]  px mantenuti visibili attraverso il salto
 * @returns {number} px positivi per pagina
 */
export function pageDelta(clientHeight, overlap = 24) {
  const h = Number.isFinite(clientHeight) ? clientHeight : 0;
  return Math.max(40, h - overlap);
}
