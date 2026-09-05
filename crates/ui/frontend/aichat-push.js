// aichat-push.js — decide se un evento ai_chat_* dal backend va emesso subito
// alla finestra-chat, bufferizzato in attesa che si apra, o scartato.
//
// Storicamente OGNI evento (anche solo "sono io" all'avvio, ServerMsg::AiChatSelf
// che l'orchestrator manda incondizionato ad ogni nuova connessione WS — vedi
// SetServerTx in aichat/service.rs) apriva da solo la finestra-chat: comodo in
// fase di test iniziale (verificare il collegamento tra macchine), ma indesiderato
// in uso normale — la chat si apriva subito ad ogni avvio di ui.exe anche senza
// che l'utente la chiedesse. Ora l'apertura è SOLO esplicita (comando `/aichat`,
// via `requestOpen()`); un evento che arriva mentre la finestra non è né aperta
// né richiesta viene scartato — non c'è nessuno ad ascoltarlo, e non dobbiamo
// aprire la finestra al posto dell'utente. Logica PURA (niente Tauri/DOM) →
// testabile con `node:test`.

// Eventi ai_chat_* "degni di notifica" quando scartati a finestra chiusa —
// tutti gli altri che passano davvero da push() (roster, self, storico,
// pending/admitted/rejected, peer-lost) sono sincronizzazione di stato
// passiva, non richiedono attenzione. "aichat:join-request" è escluso
// deliberatamente: verificato che non è mai emesso dal backend (residuo
// del vecchio flusso di consenso pairwise pre-Task 8, sostituito da
// "aichat:join-prompt"). "aichat:reachable-peers" non compare qui perché
// non passa nemmeno da push()/aiChatGate — il suo case in app.js inoltra
// direttamente a emitToLibrary, un canale separato per la finestra Library.
const NOTIFIABLE_EVENTS = new Set([
  "aichat:msg",
  "aichat:share-request",
  "aichat:join-prompt",
  "aichat:admission-request",
]);

/**
 * Un evento ai_chat_* con la sua `action` (il risultato di `push()`) merita
 * di accendere l'indicatore "attività non vista"? Vero SOLO per action ===
 * "drop" (scartato per sempre finché non si riapre la finestra) su uno dei
 * 4 eventi notificabili — "buffer" arriva comunque a momenti (apertura già
 * in corso), non è una notifica mancata.
 */
export function isNotifiableDrop(event, action) {
  return action === "drop" && NOTIFIABLE_EVENTS.has(event);
}

export function createAiChatPushGate() {
  let live = false;
  let opening = false;
  let buffer = [];

  return {
    /**
     * Un evento ai_chat_* è arrivato dal backend. Ritorna l'azione da compiere:
     * - "emit"   → finestra pronta, inoltra subito
     * - "buffer" → apertura in corso (richiesta con `requestOpen()`), accoda
     * - "drop"   → nessuno ha chiesto la finestra, scarta l'evento
     */
    push(event, payload) {
      if (live) return { action: "emit" };
      if (opening) {
        buffer.push({ event, payload });
        return { action: "buffer" };
      }
      return { action: "drop" };
    },

    /** L'utente ha chiesto esplicitamente l'apertura (es. comando `/aichat`). */
    requestOpen() {
      opening = true;
    },

    /** La finestra ha registrato i suoi listener: passa live e ritorna il replay accumulato. */
    ready() {
      live = true;
      const replay = buffer;
      buffer = [];
      return replay;
    },

    /** La finestra è stata chiusa: azzera tutto lo stato. */
    closed() {
      live = false;
      opening = false;
      buffer = [];
    },

    isLive() {
      return live;
    },
  };
}
