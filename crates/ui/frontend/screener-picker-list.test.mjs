import { test } from "node:test";
import assert from "node:assert/strict";
import { createPickerState, moveSelection, selectedItem } from "./screener-picker-list.mjs";

const ITEMS = [
  { id: "a", title: "Screener A", description: "desc A" },
  { id: "b", title: "Screener B", description: "desc B" },
  { id: "c", title: "Screener C", description: "desc C" },
];

test("createPickerState starts with index 0 on non-empty items", () => {
  const state = createPickerState(ITEMS);
  assert.equal(state.index, 0);
  assert.deepEqual(state.items, ITEMS);
});

test("createPickerState on empty items has index -1 (nessuna selezione possibile)", () => {
  const state = createPickerState([]);
  assert.equal(state.index, -1);
});

test("moveSelection(+1) advances the index", () => {
  const state = moveSelection(createPickerState(ITEMS), 1);
  assert.equal(state.index, 1);
});

test("moveSelection(-1) at index 0 wraps to the last item", () => {
  const state = moveSelection(createPickerState(ITEMS), -1);
  assert.equal(state.index, 2);
});

test("moveSelection(+1) at the last item wraps to index 0", () => {
  let state = createPickerState(ITEMS);
  state = moveSelection(state, 1);
  state = moveSelection(state, 1);
  state = moveSelection(state, 1); // 0->1->2->0 (wrap)
  assert.equal(state.index, 0);
});

test("moveSelection on empty items is a no-op (stays at -1)", () => {
  const state = moveSelection(createPickerState([]), 1);
  assert.equal(state.index, -1);
});

test("selectedItem returns the item at the current index", () => {
  const state = moveSelection(createPickerState(ITEMS), 1);
  assert.deepEqual(selectedItem(state), ITEMS[1]);
});

test("selectedItem on empty items returns null", () => {
  assert.equal(selectedItem(createPickerState([])), null);
});
