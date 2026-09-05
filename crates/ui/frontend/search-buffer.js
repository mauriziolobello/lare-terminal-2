// search-buffer.js — buffer-and-replay puro per i risultati di ricerca live.
//
// La finestra di ricerca è una webview separata che si apre in modo asincrono;
// gli eventi Tauri non sono bufferizzati, quindi gli hit/done emessi prima che
// la finestra sia in ascolto andrebbero persi. Qui accumuliamo gli hit per
// `sid` finché la finestra non si iscrive (`search:subscribe`), poi li
// rigiochiamo. Logica PURA (niente Tauri/DOM) → testabile con `node:test`.

/**
 * Crea un gestore di buffer per-ricerca.
 */
export function createSearchBuffers() {
  /** @type {Map<string, { hits: Array<{path:string, source:string}>, done: {count:number, truncated:boolean}|null, live: boolean }>} */
  const map = new Map();

  return {
    /** Registra un nuovo buffer (non-live) per `sid`. */
    open(sid) {
      map.set(sid, { hits: [], done: null, live: false });
    },

    /**
     * Registra un hit. Buffer esistente e non-live → accoda, `{emit:false}`.
     * Buffer live o assente → `{emit:true}` (va emesso direttamente).
     */
    hit(sid, h) {
      const b = map.get(sid);
      if (!b || b.live) return { emit: true };
      b.hits.push(h);
      return { emit: false };
    },

    /** Come `hit`, ma per il messaggio finale `done`. */
    done(sid, d) {
      const b = map.get(sid);
      if (!b || b.live) return { emit: true };
      b.done = d;
      return { emit: false };
    },

    /**
     * Iscrive la finestra: marca il buffer live e ritorna il replay accumulato
     * (gli hit nell'ordine d'arrivo + il `done` se già ricevuto). Svuota gli
     * accumulatori interni (il reload finestra a metà ricerca è fuori scope).
     */
    subscribe(sid) {
      const b = map.get(sid);
      if (!b) return { hits: [], done: null };
      b.live = true;
      const replay = { hits: b.hits, done: b.done };
      b.hits = [];
      b.done = null;
      return replay;
    },

    /** Elimina il buffer per `sid`. */
    close(sid) {
      map.delete(sid);
    },

    /** True se esiste un buffer per `sid`. */
    has(sid) {
      return map.has(sid);
    },
  };
}
