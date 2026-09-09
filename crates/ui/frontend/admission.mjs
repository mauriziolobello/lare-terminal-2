// admission.mjs — logica PURA del flusso di ammissione in AI Chat (gate 1:
// "vuoi entrare in chat?", e cooldown del pulsante di re-request dopo un
// rifiuto). Niente DOM, niente Tauri → testabile con `node:test` senza
// dipendenze. Il DOM/routing vive in aichat-window.js + host.js (Task 10).
import { t } from "./i18n.mjs";

/**
 * Testo del gate 1 mostrato a chi sta per entrare in una chat già popolata.
 * `present` è l'elenco delle etichette già presenti nella stanza (es.
 * ["skimble-human", "quaxo-human"]).
 *
 * Segue la congiunzione italiana standard per liste: virgole fra tutti gli
 * elementi tranne l'ultimo, che è introdotto da " e ".
 *   []            → "Vuoi entrare in chat?"
 *   ["A"]         → "Vuoi entrare in chat con A?"
 *   ["A","B"]     → "Vuoi entrare in chat con A e B?"
 *   ["A","B","C"] → "Vuoi entrare in chat con A, B e C?"
 */
export function joinPromptText(present) {
  if (!present || present.length === 0) {
    return t("aichat.join_prompt_empty");
  }
  // Tutti gli elementi tranne l'ultimo vanno uniti con ", "; l'ultimo è
  // agganciato con " e " (es. ["A","B","C"] → "A, B" + " e " + "C").
  const allButLast = present.slice(0, -1);
  const last = present[present.length - 1];
  const andWord = t("aichat.and_conjunction");
  const joined = allButLast.length > 0 ? `${allButLast.join(", ")}${andWord}${last}` : last;
  return t("aichat.join_prompt_with_peers", { peers: joined });
}

/**
 * Stato del pulsante "Chiedi di entrare" (re-request), dato l'orologio
 * corrente (`nowMs`) e la scadenza del cooldown imposto dal server dopo un
 * rifiuto (`retryAtMs`), entrambi in millisecondi epoch.
 *
 * - `retryAtMs` assente (null/undefined) → nessun cooldown attivo: pulsante
 *   subito abilitato.
 * - `nowMs >= retryAtMs` → il cooldown è scaduto: pulsante abilitato.
 * - `nowMs < retryAtMs` → cooldown ancora attivo: pulsante disabilitato,
 *   con i secondi rimanenti arrotondati per eccesso (Math.ceil) così il
 *   countdown mostrato all'utente non tocca mai "0" prima che sia
 *   davvero scaduto.
 */
export function rerequestState(nowMs, retryAtMs) {
  if (retryAtMs == null || nowMs >= retryAtMs) {
    return { enabled: true, secondsLeft: 0 };
  }
  const secondsLeft = Math.ceil((retryAtMs - nowMs) / 1000);
  return { enabled: false, secondsLeft };
}

/**
 * (FIX #7 — review 2026-07-03) Toglie un candidato risolto dalla coda del gate 2.
 *
 * Il server manda `ai_chat_admission_resolved` quando un voto si conclude
 * (ammesso/rifiutato altrove, o candidato sparito): il presente deve togliere
 * quel candidato dalla propria coda, altrimenti il banner "ammetti X?" resta
 * appeso e il click non ha più effetto (il server scarta il voto come stale).
 *
 * Pura: non tocca il DOM, ritorna una NUOVA coda (non muta l'input). Rimuove
 * TUTTE le occorrenze del candidato — così una notifica ripetuta è idempotente e
 * un eventuale duplicato accidentale in coda sparisce comunque.
 */
export function removeResolvedCandidate(queue, candidate) {
  return queue.filter((c) => c !== candidate);
}
