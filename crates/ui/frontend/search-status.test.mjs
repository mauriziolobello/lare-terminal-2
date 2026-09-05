import { test } from "node:test";
import assert from "node:assert/strict";
import { statusLabel } from "./search-status.js";

test("attiva (non done, non stopped) → spinner", () => {
  assert.equal(statusLabel({ done: false, stopped: false }), "⏳ ricerca…");
});

test("default senza argomenti → spinner", () => {
  assert.equal(statusLabel(), "⏳ ricerca…");
});

test("done, non stopped → completata con conteggio", () => {
  assert.equal(statusLabel({ done: true, count: 5 }), "✅ 5 risultati");
});

test("done, non stopped, troncati → suffisso", () => {
  assert.equal(
    statusLabel({ done: true, count: 1000, truncated: true }),
    "✅ 1000 risultati (troncati)",
  );
});

test("stopped ma non ancora done → interrotta in corso", () => {
  assert.equal(statusLabel({ done: false, stopped: true }), "⏹ interrotta…");
});

test("stopped e done → interrotta con conteggio parziale", () => {
  assert.equal(
    statusLabel({ done: true, stopped: true, count: 3 }),
    "⏹ interrotta — 3 risultati",
  );
});

test("stopped e done con troncati", () => {
  assert.equal(
    statusLabel({ done: true, stopped: true, count: 1000, truncated: true }),
    "⏹ interrotta — 1000 risultati (troncati)",
  );
});

// ── paused state ──────────────────────────────────────────────────────────────

test("paused, non done → in pausa con conteggio", () => {
  assert.equal(
    statusLabel({ paused: true, done: false, count: 7 }),
    "⏸ in pausa — 7 risultati",
  );
});

test("paused, non done, count zero → in pausa con conteggio zero", () => {
  assert.equal(
    statusLabel({ paused: true, done: false, count: 0 }),
    "⏸ in pausa — 0 risultati",
  );
});

test("paused e done → done vince su paused (ricerca completata dopo resume)", () => {
  assert.equal(
    statusLabel({ paused: true, done: true, count: 5 }),
    "✅ 5 risultati",
  );
});

test("paused ma stopped vince su paused (cancel dopo pausa)", () => {
  assert.equal(
    statusLabel({ paused: true, stopped: true, done: false, count: 2 }),
    "⏹ interrotta…",
  );
});
