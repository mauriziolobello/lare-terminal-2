// aichat-view.mjs — logica PURA della finestra-chat AI Chat (Slice 1a-ui-B).
// Niente DOM, niente Tauri → testabile con `node:test`. Il DOM vive in aichat-window.js.
import { t } from "./i18n.mjs";

/** Estrae l'etichetta macchina da `from_label` ("skimble-human"/"skimble-ai" → "skimble").
 *  `from_label` è sempre `<label_base>-human` o `<label_base>-ai` sul wire (vedi
 *  `channel.rs`) — qui basta togliere il suffisso finale. Se il formato non
 *  combacia (peer malformato/storico anomalo), ritorna la stringa intera così com'è:
 *  meglio un'etichetta macchina "sporca" che perderla del tutto. */
function machineLabel(from_label) {
  if (typeof from_label !== "string") return from_label;
  return from_label.replace(/-(human|ai)$/, "");
}

/** Normalizza un messaggio della stanza in { label, text, is_ai } per il render.
 *  `display_name` (se presente) sostituisce `from_label` come testo mostrato, ma
 *  affiancato all'etichetta macchina ("Clio / skimble", "Maurizio / rumpleteazer"):
 *  un nickname da solo non basta a distinguere due macchine con lo stesso nome
 *  scelto (stesso umano collegato da due PC, o due AI con lo stesso nickname) —
 *  feedback utente 2026-08-13 dopo il primo smoke test dal vivo. Senza
 *  `display_name`, fallback a `from_label` grezzo (già disambiguante da solo) per
 *  compatibilità con storico/peer che non lo mandano ancora. `is_ai` pilota il
 *  badge (mai concatenato al nome: un nickname scelto dall'AI è un nome proprio a
 *  tutti gli effetti, non va reso ambiguo). */
export function messageLine({ from_label, text, display_name, is_ai }) {
  const label = display_name ? `${display_name} / ${machineLabel(from_label)}` : from_label;
  return { label, text, is_ai: Boolean(is_ai) };
}

/** Riga del roster: "presenti: a · b · c" (vuoto → trattino). */
export function rosterText(participants) {
  if (!participants || participants.length === 0) return t("aichat.roster_empty");
  return t("aichat.roster_present", { participants: participants.join(" · ") });
}

/** Testo del banner di consenso per un peer comparso in rete. */
export function consentPrompt(peer_label) {
  return t("aichat.consent_prompt", { peer: peer_label });
}

/** True se l'etichetta è di QUESTA macchina (per stilare diversamente i propri messaggi). */
export function isSelf(label, myBase) {
  return typeof label === "string" && label.startsWith(`${myBase}-`);
}

/** Normalizza il payload delle entries di storico: assente/vuoto → array vuoto.
 *  Restituisce le entries GREZZE ({from_label,text,display_name,is_ai} — la forma
 *  di `ChatLine` sul wire), non il formato di `messageLine`: `addLine` (in
 *  aichat-window.js) chiama già `messageLine` internamente. */
export function historyEntries(entries) {
  return entries || [];
}

/** Titolo della finestra chat: "AICHAT - <etichetta>" quando conosciamo la nostra
 *  etichetta (arrivata via AiChatSelf); solo "AICHAT" come fallback prima che arrivi. */
export function chatWindowTitle(selfLabel) {
  return selfLabel ? `AICHAT - ${selfLabel}` : "AICHAT";
}

/** Testo dell'avviso live "peer scollegato" (keepalive scaduto o disconnessione). Evento
 *  transitorio: non entra mai nello storico persistito. "risulta scollegato" (non "è
 *  sparito"): stessa causa tecnica, ma "sparito" suona allarmante/gergale per un umano
 *  che legge la chat — feedback utente 2026-07-05. */
export function peerLostText(label) {
  return label ? t("aichat.peer_lost_named", { label }) : t("aichat.peer_lost_unnamed");
}
