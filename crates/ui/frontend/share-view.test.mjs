import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { initI18n } from "./i18n.mjs";
import {
  shareTargetList,
  formatShareSize,
  shareConsentPrompt,
  shareResultLine,
  shareReceivedLine,
} from "./share-view.mjs";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const itDict = JSON.parse(
  fs.readFileSync(path.resolve(__dirname, "../../../Test Run/Configuration/i18n/it.json"), "utf8")
);
initI18n(itDict);

test("shareTargetList spoglia il suffisso -human/-ai e deduplica per macchina", () => {
  const participants = ["skimble-human", "skimble-ai", "rumpleteazer-human"];
  assert.deepEqual(shareTargetList(participants), ["rumpleteazer", "skimble"]);
});

test("shareTargetList su roster vuoto restituisce array vuoto", () => {
  assert.deepEqual(shareTargetList([]), []);
});

test("shareTargetList ignora etichette senza suffisso -human/-ai riconosciuto", () => {
  // Difesa in profondità: un'etichetta malformata non deve far crashare la UI,
  // solo essere esclusa (non è "una macchina" nel senso atteso da questa feature).
  assert.deepEqual(shareTargetList(["skimble-human", "malformed"]), ["skimble"]);
});

test("formatShareSize sotto 1024 byte mostra i byte esatti", () => {
  assert.equal(formatShareSize(0), "0 B");
  assert.equal(formatShareSize(500), "500 B");
  assert.equal(formatShareSize(1023), "1023 B");
});

test("formatShareSize da 1024 byte in su mostra KB con una cifra decimale", () => {
  assert.equal(formatShareSize(1024), "1,0 KB");
  assert.equal(formatShareSize(524288), "512,0 KB"); // cap di riferimento (512 * 1024)
  assert.equal(formatShareSize(1536), "1,5 KB");
});

test("shareConsentPrompt compone il testo del banner di consenso", () => {
  assert.equal(
    shareConsentPrompt("rumpleteazer", "notes.md", 1536),
    '"rumpleteazer" vuole condividere "notes.md" (1,5 KB) — accetti?'
  );
});

test("shareResultLine per esito Accepted (stringa nuda sul wire)", () => {
  assert.equal(
    shareResultLine("notes.md", "skimble", "accepted"),
    '📤 Condivisione di "notes.md" con skimble: accettata'
  );
});

test("shareResultLine per esito Rejected (stringa nuda sul wire)", () => {
  assert.equal(
    shareResultLine("notes.md", "skimble", "rejected"),
    '📤 Condivisione di "notes.md" con skimble: rifiutata'
  );
});

test("shareResultLine per esito Failed include il motivo (oggetto {failed:{reason}} sul wire)", () => {
  assert.equal(
    shareResultLine("notes.md", "skimble", { failed: { reason: "documento troppo grande" } }),
    '📤 Condivisione di "notes.md" con skimble: fallita (documento troppo grande)'
  );
});

test("shareReceivedLine compone la riga di feedback per il destinatario", () => {
  assert.equal(
    shareReceivedLine("Note", "rumpleteazer"),
    '📥 Ricevuto "Note" da rumpleteazer, salvato in Library'
  );
});

test("shareReceivedLine con titolo contenente virgolette non le sfugge (testContent, non HTML)", () => {
  // Nessun escaping necessario: questa stringa finisce sempre in pre.textContent
  // (renderer.js), mai in innerHTML — vedi Global Constraints del piano Slice 1a-ui.
  assert.equal(
    shareReceivedLine('Il mio "documento"', "skimble"),
    '📥 Ricevuto "Il mio "documento"" da skimble, salvato in Library'
  );
});
