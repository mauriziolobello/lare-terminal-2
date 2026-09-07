import { test } from "node:test";
import assert from "node:assert/strict";
import { createDebouncer } from "./fit-debounce.mjs";

test("richiama fn una sola volta se invocato più volte entro il delay", () => {
  const scheduled = [];
  const fakeSetTimeout = (fn, ms) => {
    scheduled.push({ fn, ms, cancelled: false });
    return scheduled.length - 1;
  };
  const fakeClearTimeout = (handle) => {
    scheduled[handle].cancelled = true;
  };
  const debounced = createDebouncer(50, fakeSetTimeout, fakeClearTimeout);

  let calls = 0;
  debounced(() => calls++);
  debounced(() => calls++); // deve cancellare la schedulazione precedente

  const alive = scheduled.filter((s) => !s.cancelled);
  assert.equal(alive.length, 1, "una sola schedulazione viva");
  assert.equal(alive[0].ms, 50);
  alive[0].fn();
  assert.equal(calls, 1);
});
