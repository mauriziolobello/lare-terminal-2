import { test } from "node:test";
import assert from "node:assert/strict";
import { createSearchBuffers } from "./search-buffer.js";

test("hit prima del subscribe viene bufferizzato (emit:false) e poi rigiocato", () => {
  const b = createSearchBuffers();
  b.open("s1");
  assert.deepEqual(b.hit("s1", { path: "a.pdf", source: "cwd" }), { emit: false });
  assert.deepEqual(b.hit("s1", { path: "b.pdf", source: "cloud" }), { emit: false });
  const r = b.subscribe("s1");
  assert.deepEqual(r.hits, [
    { path: "a.pdf", source: "cwd" },
    { path: "b.pdf", source: "cloud" },
  ]);
  assert.equal(r.done, null);
});

test("done prima del subscribe è incluso nel replay", () => {
  const b = createSearchBuffers();
  b.open("s1");
  b.hit("s1", { path: "a.pdf", source: "cwd" });
  assert.deepEqual(b.done("s1", { count: 1, truncated: false }), { emit: false });
  const r = b.subscribe("s1");
  assert.equal(r.hits.length, 1);
  assert.deepEqual(r.done, { count: 1, truncated: false });
});

test("dopo il subscribe (live) hit e done si emettono diretti (emit:true)", () => {
  const b = createSearchBuffers();
  b.open("s1");
  b.subscribe("s1");
  assert.deepEqual(b.hit("s1", { path: "c.pdf", source: "cwd" }), { emit: true });
  assert.deepEqual(b.done("s1", { count: 1, truncated: false }), { emit: true });
});

test("hit/done senza open → emit diretto (difensivo)", () => {
  const b = createSearchBuffers();
  assert.deepEqual(b.hit("nope", { path: "x", source: "cwd" }), { emit: true });
  assert.deepEqual(b.done("nope", { count: 0, truncated: false }), { emit: true });
});

test("close elimina il buffer", () => {
  const b = createSearchBuffers();
  b.open("s1");
  assert.equal(b.has("s1"), true);
  b.close("s1");
  assert.equal(b.has("s1"), false);
});

test("subscribe senza buffer → replay vuoto", () => {
  const b = createSearchBuffers();
  assert.deepEqual(b.subscribe("ghost"), { hits: [], done: null });
});
