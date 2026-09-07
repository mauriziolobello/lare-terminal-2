import { test } from "node:test";
import assert from "node:assert/strict";
import { base64ToUint8Array } from "./base64.mjs";

test("decodifica base64 in byte grezzi", () => {
  const encoded = Buffer.from("hello").toString("base64");
  const bytes = base64ToUint8Array(encoded);
  assert.deepEqual(Array.from(bytes), [104, 101, 108, 108, 111]);
});

test("stringa vuota decodifica in array vuoto", () => {
  assert.equal(base64ToUint8Array("").length, 0);
});
