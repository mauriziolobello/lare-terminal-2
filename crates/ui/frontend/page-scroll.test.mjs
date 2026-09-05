import { test } from "node:test";
import assert from "node:assert/strict";
import { pageDelta } from "./page-scroll.js";

test("una pagina meno l'overlap di default", () => {
  assert.equal(pageDelta(600), 576);          // 600 - 24
});
test("minimo 40 px quando il pannello è piccolo", () => {
  assert.equal(pageDelta(50), 40);            // max(40, 26)
  assert.equal(pageDelta(0), 40);
});
test("overlap personalizzato", () => {
  assert.equal(pageDelta(600, 0), 600);
});
test("input non finito → minimo", () => {
  assert.equal(pageDelta(NaN), 40);
  assert.equal(pageDelta(undefined), 40);
});
