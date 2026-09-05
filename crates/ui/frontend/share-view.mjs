/**
 * share-view.mjs — Modulo puro per la UI della feature "Share with" (Library →
 * AI Chat). Nessuna dipendenza da DOM, Tauri o I/O — testabile con `node:test`.
 *
 * Riusato da tre finestre diverse (library.js, host.js, aichat-window.js) per
 * evitare di duplicare la logica di formattazione in ciascuna.
 */

// ---------------------------------------------------------------------------
// shareTargetList(participants) → string[]
//
// Converte il roster grezzo di AI Chat (etichette con suffisso "-human"/"-ai",
// una per ogni peer/AI collegato) nell'elenco di MACCHINE distinte per il
// picker "Condividi" — Share non distingue chi risponde per una macchina
// (spec Slice 1a §1), quindi "skimble-human" e "skimble-ai" contano come
// un'unica destinazione "skimble".
//
// Etichette che non terminano con "-human" o "-ai" sono escluse (difesa in
// profondità: un'etichetta malformata non deve interrompere il rendering).
//
// Ordinamento alfabetico per un elenco stabile/prevedibile in UI.
// ---------------------------------------------------------------------------
export function shareTargetList(participants) {
  const machines = new Set();
  for (const label of participants) {
    if (label.endsWith("-human")) {
      machines.add(label.slice(0, -"-human".length));
    } else if (label.endsWith("-ai")) {
      machines.add(label.slice(0, -"-ai".length));
    }
  }
  return Array.from(machines).sort();
}

// ---------------------------------------------------------------------------
// formatShareSize(bytes) → string
//
// Formatta una dimensione in byte per la UI umana. Il cap di Share è 512 KB
// (MAX_SHARE_SIZE_BYTES lato backend), quindi non serve gestire MB.
//
//   < 1024        → "N B"        (es. "500 B")
//   >= 1024       → "N,N KB"     (una cifra decimale, virgola italiana)
// ---------------------------------------------------------------------------
export function formatShareSize(bytes) {
  if (bytes < 1024) return `${bytes} B`;
  const kb = bytes / 1024;
  return `${kb.toFixed(1).replace(".", ",")} KB`;
}

// ---------------------------------------------------------------------------
// shareConsentPrompt(fromLabel, docName, sizeBytes) → string
//
// Testo del banner di consenso mostrato al DESTINATARIO di un'offerta
// (ServerMsg::ShareRequest). fromLabel è già il label_base nudo (la macchina,
// non "-human"/"-ai" — vedi ChatMsg::ShareOffer::from_label lato backend).
// ---------------------------------------------------------------------------
export function shareConsentPrompt(fromLabel, docName, sizeBytes) {
  return `"${fromLabel}" vuole condividere "${docName}" (${formatShareSize(sizeBytes)}) — accetti?`;
}

// ---------------------------------------------------------------------------
// shareResultLine(docName, targetLabel, outcome) → string
//
// Riga di esito mostrata nel pannello del cursore principale al MITTENTE.
// `outcome` è `msg.outcome` così come arriva dal JSON di ServerMsg::ShareResult
// — a differenza di ServerMsg/ClientMsg (tag "type" esplicito, enum
// internamente taggato), `ShareOutcome` NON ha `#[serde(tag = "type")]`, solo
// `rename_all = "snake_case")]` → rappresentazione di default di serde,
// "esternamente taggata" (verificato empiricamente con
// `serde_json::to_string`, non assunto):
//   Accepted        → la stringa nuda "accepted"
//   Rejected        → la stringa nuda "rejected"
//   Failed{reason}  → {"failed": {"reason": "..."}}
// ---------------------------------------------------------------------------
export function shareResultLine(docName, targetLabel, outcome) {
  const prefix = `📤 Condivisione di "${docName}" con ${targetLabel}: `;
  if (outcome === "accepted") return prefix + "accettata";
  if (outcome === "rejected") return prefix + "rifiutata";
  if (outcome && typeof outcome === "object" && "failed" in outcome) {
    return prefix + `fallita (${outcome.failed.reason})`;
  }
  return prefix + "esito sconosciuto";
}

// ---------------------------------------------------------------------------
// shareReceivedLine(title, fromLabel) → string
//
// Riga di feedback mostrata nel pannello del cursore principale al
// DESTINATARIO, quando un documento condiviso è stato scritto con successo
// nella propria Library (ServerMsg::ShareIncomingData → archive_save riuscito).
// Simmetrica a shareResultLine (che informa il MITTENTE), ma solo per il caso
// di successo — un fallimento lato destinatario resta silenzioso (design §4:
// l'entry scade naturalmente dopo 24h, nessun messaggio wire dedicato).
// ---------------------------------------------------------------------------
export function shareReceivedLine(title, fromLabel) {
  return `📥 Ricevuto "${title}" da ${fromLabel}, salvato in Library`;
}
