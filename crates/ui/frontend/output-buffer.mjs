// output-buffer.mjs — buffer-and-replay PERSISTENTE per il contenuto delle
// finestre di output del canale shell (2.0, spec §3.2, D14).
// Fix 2026-09-15 (show_markdown update-in-place): la sottoscrizione ora è
// persistente invece che one-shot. Una finestra che riceve più chiamate
// `content()` per lo stesso `window_id` (es. show_markdown chiamato più volte
// nello stesso turno AI) le riceve tutte — l'entry resta con `subscribed:true`
// finché `close()` non la rimuove esplicitamente.
//
// Perché serve (bug trovato in review, task 9 fix round 1): la finestra di
// output è una webview separata che si apre in modo asincrono (open_output_window
// crea la finestra, poi window.js carica ed esegue il proprio bootstrap prima
// di potersi mettere in ascolto). L'evento Tauri globale `output:content`, a
// differenza di una coda, NON è bufferizzato dal runtime: se l'orchestratore
// risponde prima che la finestra abbia registrato il listener (un comando
// slash veloce, es. "/open x"), quel contenuto va perso per sempre e la
// finestra resta bloccata sul segnaposto "_in corso…_" — nessuna superficie
// se ne accorge.
//
// Soluzione: stesso pattern di search-buffer.js/createSearchBuffers() —
// accumuliamo il contenuto per `windowId` finché la finestra non si iscrive
// esplicitamente (`output:subscribe`), poi lo rigiochiamo. Da ora in poi la
// sottoscrizione persiste: ogni `content()` successivo viene emesso subito
// (finestra già in ascolto) finché `close()` non rimuove l'entry.
// Logica PURA (niente Tauri/DOM) → testabile con `node:test`.

/**
 * Crea un gestore di buffer per-finestra-di-output.
 *
 * Stato per id: `{ markdown: string|null, subscribed: boolean }`.
 *   - `markdown === null` e non-subscribed → aperta, contenuto non ancora
 *     arrivato (stato dopo `open()`).
 *   - `markdown` valorizzato e non-subscribed → contenuto arrivato PRIMA che
 *     la finestra si iscrivesse: bufferizzato, in attesa di replay.
 *   - `subscribed === true` → la finestra è in ascolto: il contenuto (quando
 *     arriva) va emesso subito, non bufferizzato.
 */
export function createOutputBuffers() {
  /** @type {Map<string, { markdown: string|null, subscribed: boolean }>} */
  const map = new Map();

  return {
    /** Registra un nuovo buffer (non ancora sottoscritto) per `windowId`. */
    open(windowId) {
      map.set(windowId, { markdown: null, subscribed: false });
    },

/**
      * Contenuto arrivato per `windowId`.
      *   - Finestra già sottoscritta → `{emit:true}`. L'entry RESTA (non viene
      *     più eliminata — la finestra può ricevere più aggiornamenti nello
      *     stesso turno, es. show_markdown chiamato più volte).
      *   - Finestra non sottoscritta (o `open()` mai arrivato — difensivo: se
      *     il messaggio `open_output_window` si fosse perso, non vogliamo
      *     perdere ANCHE il contenuto) → bufferizzato, `{emit:false}`.
      */
     content(windowId, markdown) {
       const b = map.get(windowId);
       if (b && b.subscribed) {
         return { emit: true };
       }
       map.set(windowId, { markdown, subscribed: false });
       return { emit: false };
     },

     /**
      * La finestra si iscrive: marca `windowId` come sottoscritto. Se il
      * contenuto era già arrivato (bufferizzato), lo ritorna e imposta
      * l'entry come `subscribed: true, markdown: null` (pronta a ricevere
      * aggiornamenti successivi) invece di cancellarla — la sottoscrizione
      * ora è persistente fino a `close()`.
      */
     subscribe(windowId) {
       const b = map.get(windowId);
       if (b && b.markdown !== null) {
         const md = b.markdown;
         map.set(windowId, { markdown: null, subscribed: true });
         return md;
       }
       map.set(windowId, { markdown: null, subscribed: true });
       return null;
     },

    /** Elimina il buffer per `windowId` (finestra chiusa). */
    close(windowId) {
      map.delete(windowId);
    },
  };
}
