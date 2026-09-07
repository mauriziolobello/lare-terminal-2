// Debounce puro (50 ms, confermato dallo spike 2) per il fit del terminale
// sul resize della finestra. `setTimeoutFn`/`clearTimeoutFn` sono un seam:
// nel prodotto sono i globali `setTimeout`/`clearTimeout`, nei test un fake
// che non aspetta tempo reale.
export function createDebouncer(delayMs, setTimeoutFn = setTimeout, clearTimeoutFn = clearTimeout) {
  let handle = null;
  return function debounced(fn) {
    if (handle !== null) clearTimeoutFn(handle);
    handle = setTimeoutFn(fn, delayMs);
  };
}
