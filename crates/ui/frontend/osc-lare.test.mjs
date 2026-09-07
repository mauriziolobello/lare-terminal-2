import { test } from "node:test";
import assert from "node:assert/strict";
import { parseLareOsc } from "./osc-lare.mjs";

test("riconosce un payload lare;intercept;<riga>", () => {
  assert.deepEqual(parseLareOsc("lare;intercept;/help"), { kind: "intercept", line: "/help" });
});

test("la riga può contenere punti e virgola: solo i primi due separano", () => {
  assert.deepEqual(parseLareOsc("lare;intercept;/ai \"a;b;c\""), {
    kind: "intercept",
    line: '/ai "a;b;c"',
  });
});

test("payload senza il prefisso lare;intercept; è null", () => {
  assert.equal(parseLareOsc("altro;9001;x"), null);
  assert.equal(parseLareOsc("lare;altro-evento;x"), null);
  assert.equal(parseLareOsc(""), null);
});
