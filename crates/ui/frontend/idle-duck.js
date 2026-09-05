// idle-duck.js — logica pura per l'animazione dell'anatra idle.
//
// SRP: calcola la nuova posizione dell'anatra dato lo stato corrente e il
// tempo trascorso. Nessun DOM, nessun timer, nessuna dipendenza → testabile
// con node:test (pattern identico a search-status.js, page-scroll.js, ecc.).

/** Velocità di cammino in pixel al secondo. */
export const DUCK_SPEED_PX_PER_S = 30;

/**
 * Calcola la posizione dell'anatra dopo `dtMs` millisecondi.
 *
 * Il pulcino rimbalza tra x=0 e x=maxX senza overshoot: se il passo
 * lo porterebbe oltre il bordo, viene fermato esattamente al bordo e
 * la direzione viene invertita.
 *
 * @param {{ x: number, dir: 1|-1 }} state  Stato corrente.
 * @param {number} dtMs   Millisecondi trascorsi dall'ultimo frame.
 * @param {number} maxX   Posizione x massima raggiungibile (pannello - larghezza pulcino).
 * @returns {{ x: number, dir: 1|-1 }}
 */
export function duckStep(state, dtMs, maxX) {
  let { x, dir } = state;
  x += dir * DUCK_SPEED_PX_PER_S * (dtMs / 1000);

  if (x >= maxX) {
    return { x: maxX, dir: -1 };
  }
  if (x <= 0) {
    return { x: 0, dir: 1 };
  }
  return { x, dir };
}
