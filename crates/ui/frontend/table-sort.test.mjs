import { test } from "node:test";
import assert from "node:assert/strict";
import { sortedOrder } from "./table-sort.mjs";

test("sortedOrder ascending numeric", () => {
  assert.deepEqual(sortedOrder(["30", "10", "20"], "asc", "num"), [1, 2, 0]);
});

test("sortedOrder descending numeric", () => {
  assert.deepEqual(sortedOrder(["30", "10", "20"], "desc", "num"), [0, 2, 1]);
});

test("sortedOrder ascending text uses locale compare, not lexicographic on raw strings", () => {
  assert.deepEqual(sortedOrder(["Zeta", "Alfa", "Beta"], "asc", "text"), [1, 2, 0]);
});

test("sortedOrder descending text", () => {
  assert.deepEqual(sortedOrder(["Zeta", "Alfa", "Beta"], "desc", "text"), [0, 2, 1]);
});

test("sortedOrder puts missing values (null) last on ascending", () => {
  assert.deepEqual(sortedOrder(["10", null, "5"], "asc", "num"), [2, 0, 1]);
});

test("sortedOrder puts missing values (null) last on descending too", () => {
  assert.deepEqual(sortedOrder(["10", null, "5"], "desc", "num"), [0, 2, 1]);
});

test("sortedOrder is stable on ties (preserves prior relative order)", () => {
  assert.deepEqual(sortedOrder(["10", "10", "5"], "asc", "num"), [2, 0, 1]);
});

test("sortedOrder handles an empty array", () => {
  assert.deepEqual(sortedOrder([], "asc", "num"), []);
});

test("sortedOrder handles all-missing values", () => {
  assert.deepEqual(sortedOrder([null, null], "asc", "num"), [0, 1]);
});
