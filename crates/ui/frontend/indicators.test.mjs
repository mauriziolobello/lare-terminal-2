import { test } from "node:test";
import assert from "node:assert/strict";
import { createIndicatorState, onIntercept, onActivity } from "./indicators.mjs";

test("stato iniziale: nessun comando, AI non al lavoro", () => {
  assert.deepEqual(createIndicatorState(), { lastCommand: null, aiBusy: false });
});

test("onIntercept aggiorna lastCommand, lascia aiBusy invariato", () => {
  const s = onIntercept({ lastCommand: null, aiBusy: true }, "/help");
  assert.deepEqual(s, { lastCommand: "/help", aiBusy: true });
});

test("onActivity con la propria sessione aggiorna aiBusy", () => {
  const s = onActivity({ lastCommand: null, aiBusy: false }, "abc123", "abc123", true);
  assert.equal(s.aiBusy, true);
});

test("onActivity di un'altra sessione è ignorato", () => {
  const before = { lastCommand: null, aiBusy: false };
  const after = onActivity(before, "altra-sessione", "abc123", true);
  assert.deepEqual(after, before);
});
