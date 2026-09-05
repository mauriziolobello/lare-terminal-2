import { test } from "node:test";
import assert from "node:assert/strict";
import {
  visibleNotesSorted,
  isTitleValid,
  isBodyValid,
  formatNoteMeta,
  noteEditMessages,
} from "./note-view.mjs";

test("visibleNotesSorted esclude le note cancellate", () => {
  const notes = [
    { id: "1", title: "A", deleted: false, created_at_ms: 100 },
    { id: "2", title: "B", deleted: true, created_at_ms: 200 },
  ];
  assert.deepEqual(visibleNotesSorted(notes).map((n) => n.id), ["1"]);
});

test("visibleNotesSorted ordina per data di creazione, più recente prima", () => {
  const notes = [
    { id: "old", title: "A", deleted: false, created_at_ms: 100 },
    { id: "new", title: "B", deleted: false, created_at_ms: 200 },
  ];
  assert.deepEqual(visibleNotesSorted(notes).map((n) => n.id), ["new", "old"]);
});

test("visibleNotesSorted su array vuoto restituisce array vuoto", () => {
  assert.deepEqual(visibleNotesSorted([]), []);
});

test("isTitleValid accetta titoli fino a 200 caratteri", () => {
  assert.equal(isTitleValid("a".repeat(200)), true);
  assert.equal(isTitleValid("a".repeat(201)), false);
});

test("isTitleValid rifiuta titolo vuoto", () => {
  assert.equal(isTitleValid(""), false);
  assert.equal(isTitleValid("   "), false);
});

test("formatNoteMeta compone macchina + data leggibile", () => {
  // 2026-07-29T00:00:00.000Z = 1785283200000 ms — usiamo un valore fisso e
  // verifichiamo solo che la macchina compaia; il formato data dipende dal
  // fuso orario di chi esegue il test, non lo blocchiamo su una stringa esatta.
  const line = formatNoteMeta("skimble", 1785283200000);
  assert.match(line, /skimble/);
});

test("isBodyValid rifiuta corpo oltre 256 KB, accetta fino al limite", () => {
  const maxBytes = 256 * 1024;
  assert.equal(isBodyValid("a".repeat(maxBytes)), true);
  assert.equal(isBodyValid("a".repeat(maxBytes + 1)), false);
});

// ---------------------------------------------------------------------------
// noteEditMessages — decide quali messaggi WS mandare al salvataggio di
// "Modifica nota". Estratto dall'handler `noteSaveBtn` (library.js) perché
// era l'unica logica pura del dialog nota rimasta non testata: un fix reale
// (smoke test dal vivo 2026-07-31) l'aveva appena toccata — confronto contro
// "vuoto" corretto in confronto contro il valore originale precompilato,
// altrimenti cancellare deliberatamente il proprio testo veniva ignorato in
// silenzio al salvataggio.
// ---------------------------------------------------------------------------

test("noteEditMessages: nulla cambiato → nessun messaggio", () => {
  const msgs = noteEditMessages({
    id: "n1",
    title: "Titolo",
    text: "corpo",
    originalTitle: "Titolo",
    originalBodyText: "corpo",
  });
  assert.deepEqual(msgs, {});
});

test("noteEditMessages: corpo svuotato deliberatamente → editText vuoto inviato", () => {
  const msgs = noteEditMessages({
    id: "n1",
    title: "Titolo",
    text: "",
    originalTitle: "Titolo",
    originalBodyText: "corpo",
  });
  assert.deepEqual(msgs, { editText: "" });
});

test("noteEditMessages: solo titolo cambiato → solo editTitle", () => {
  const msgs = noteEditMessages({
    id: "n1",
    title: "Nuovo titolo",
    text: "corpo",
    originalTitle: "Titolo",
    originalBodyText: "corpo",
  });
  assert.deepEqual(msgs, { editTitle: "Nuovo titolo" });
});

test("noteEditMessages: entrambi cambiati → entrambi i messaggi", () => {
  const msgs = noteEditMessages({
    id: "n1",
    title: "Nuovo titolo",
    text: "nuovo corpo",
    originalTitle: "Titolo",
    originalBodyText: "corpo",
  });
  assert.deepEqual(msgs, { editText: "nuovo corpo", editTitle: "Nuovo titolo" });
});

test("noteEditMessages: confronto grezzo, nessun trim (spazio finale è modifica reale)", () => {
  const msgs = noteEditMessages({
    id: "n1",
    title: "Titolo ",
    text: "corpo",
    originalTitle: "Titolo",
    originalBodyText: "corpo",
  });
  assert.deepEqual(msgs, { editTitle: "Titolo " });
});
