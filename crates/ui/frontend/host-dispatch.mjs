// host-dispatch.mjs — Classificazione pura dei ServerMsg per la pagina host
// (nessun DOM, testabile con node:test).
//
// La pagina host (host.html/host.js) è l'unica proprietaria della connessione
// WS "di default" (Hello senza `channel`) in 2.0 — l'overlay F2 è sparito, e
// con lui ogni superficie che il cursore v1 offriva (output, editor, banner
// di conferma). Ogni ServerMsg che arriva su questa connessione ricade in una
// di quattro categorie:
//
//   "window":  apre o aggiorna una finestra (es. una finestra Markdown, la
//              ricerca live, una finestra plugin, l'anteprima di una routine),
//              apre una finestra locale chiesta da una shell, o risponde a un
//              ping di `ui`.
//   "relay":   va rigirato a una finestra già aperta (AI Chat, Library/note,
//              Share) via evento Tauri globale — host.js non lo rende, lo
//              inoltra soltanto.
//   "ignored": era per il cursore v1 (output, watchdog, riga cwd, ecc.), che
//              non esiste più — nessuna superficie lo consuma.
//   "deny":    richiesta di conferma tool (tool_confirm_request) arrivata
//              sulla connessione di default — senza cursore nessuno può
//              rispondere Sì/No, e la risposta sicura di default è NO
//              (l'orchestratore ha comunque un timeout che nega da solo).
const WINDOW = new Set([
  "open_window",
  "search_open",
  "search_hit",
  "search_done",
  "open_screener_picker",
  "open_plugin_window",
  "update_plugin_window",
  "close_plugin_window",
  "routine_save_preview",
  // canale shell (2.0, spec §3.2/§4.1)
  "open_output_window",
  "output_window_content",
  "open_ui_local",
  "ui_ping",
]);

const RELAY = new Set([
  "ai_chat_message",
  "activity_indicator",
  "ai_chat_roster",
  "ai_chat_reachable_peers",
  "ai_chat_history",
  "ai_chat_self",
  "ai_chat_join_request",
  "ai_chat_peer_lost",
  "ai_chat_join_prompt",
  "ai_chat_admission_request",
  "ai_chat_admission_resolved",
  "ai_chat_pending",
  "ai_chat_admitted",
  "ai_chat_rejected",
  "notes_snapshot",
  "note_upserted",
  "share_request",
  "share_result",
  "share_content_request",
  "share_incoming_data",
]);

export function classifyServerMsg(msg) {
  if (msg.type === "tool_confirm_request") return "deny";
  if (WINDOW.has(msg.type)) return "window";
  if (RELAY.has(msg.type)) return "relay";
  return "ignored";
}
