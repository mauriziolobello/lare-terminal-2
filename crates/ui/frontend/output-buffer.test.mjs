import { test } from "node:test";
import assert from "node:assert/strict";
import { createOutputBuffers } from "./output-buffer.mjs";

test("content prima del subscribe viene bufferizzato e rigiocato una sola volta al subscribe", () => {
  const b = createOutputBuffers();
  b.open("w1");
  assert.deepEqual(b.content("w1", "# ciao"), { emit: false });
  assert.equal(b.subscribe("w1"), "# ciao");
  // Il replay è one-shot: un secondo subscribe sullo stesso id non ritrova
  // più il contenuto (l'entry è stata svuotata dal primo subscribe).
  assert.equal(b.subscribe("w1"), null);
});

test("content dopo il subscribe si emette subito (emit:true) e non viene rigiocato di nuovo", () => {
  const b = createOutputBuffers();
  b.open("w1");
  assert.equal(b.subscribe("w1"), null);
  assert.deepEqual(b.content("w1", "# risultato"), { emit: true });
  // Dopo un emit diretto l'entry è stata eliminata: un subscribe successivo
  // (id ora ignoto) si limita a marcare "subscribed" e non trova nulla.
  assert.equal(b.subscribe("w1"), null);
});

test("close elimina il contenuto bufferizzato: un subscribe successivo non lo trova più", () => {
  const b = createOutputBuffers();
  b.open("w1");
  b.content("w1", "# perso");
  b.close("w1");
  assert.equal(b.subscribe("w1"), null);
});

test("due finestre non si interferiscono", () => {
  const b = createOutputBuffers();
  b.open("w1");
  b.open("w2");
  assert.deepEqual(b.content("w1", "A"), { emit: false });
  assert.deepEqual(b.content("w2", "B"), { emit: false });
  assert.equal(b.subscribe("w2"), "B");
  assert.equal(b.subscribe("w1"), "A");
});

test("subscribe due volte restituisce il contenuto una sola volta", () => {
  const b = createOutputBuffers();
  b.open("w1");
  b.content("w1", "# una volta sola");
  assert.equal(b.subscribe("w1"), "# una volta sola");
  assert.equal(b.subscribe("w1"), null);
});

test("content su un id ignoto (open perso) si bufferizza comunque, non-subscribed di default", () => {
  const b = createOutputBuffers();
  assert.deepEqual(b.content("ghost", "# tardivo"), { emit: false });
  assert.equal(b.subscribe("ghost"), "# tardivo");
});

test("subscribe su un id ignoto marca subscribed e ritorna null", () => {
  const b = createOutputBuffers();
  assert.equal(b.subscribe("ghost"), null);
  // Da questo punto l'id è "subscribed": un content successivo va emesso subito.
  assert.deepEqual(b.content("ghost", "# live"), { emit: true });
});

test("content persiste dopo la sottoscrizione: più aggiornamenti, tutti emessi", () => {
  const b = createOutputBuffers();
  b.open("w1");
  assert.strictEqual(b.subscribe("w1"), null); // niente ancora bufferizzato
  assert.deepStrictEqual(b.content("w1", "primo"), { emit: true });
  assert.deepStrictEqual(b.content("w1", "secondo"), { emit: true }); // PRIMA del fix A.1: { emit: false }
  assert.deepStrictEqual(b.content("w1", "terzo"), { emit: true });
});
