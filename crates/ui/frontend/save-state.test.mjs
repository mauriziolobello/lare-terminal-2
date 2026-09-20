import { test } from "node:test";
import assert from "node:assert/strict";
import { planSave } from "./save-state.mjs";

test("planSave chiama archive_save quando la finestra non ha ancora un file salvato", () => {
  assert.deepEqual(planSave(null, "Titolo", "testo"), {
    cmd: "archive_save",
    args: { title: "Titolo", content: "testo" },
  });
});

test("planSave chiama archive_update con lo stesso filename una volta che esiste già", () => {
  assert.deepEqual(planSave("titolo-123.md", "Titolo", "testo aggiornato"), {
    cmd: "archive_update",
    args: { file: "titolo-123.md", title: "Titolo", content: "testo aggiornato" },
  });
});

test("planSave torna ad archive_save se il filename memorizzato viene azzerato (stringa vuota)", () => {
  // Difesa esplicita: "" e' falsy in JS quanto null — un filename vuoto non
  // deve mai essere passato ad archive_update (Rust rifiuterebbe comunque un
  // path vuoto, ma il contratto di planSave lo tratta come "non salvato").
  assert.deepEqual(planSave("", "Titolo", "testo"), {
    cmd: "archive_save",
    args: { title: "Titolo", content: "testo" },
  });
});
