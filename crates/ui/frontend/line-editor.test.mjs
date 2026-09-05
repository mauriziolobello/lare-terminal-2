// line-editor.test.mjs — TDD tests for the pure lineEditor module.
//
// Run with:  node --test crates/ui/frontend/line-editor.test.mjs
//
// Uses only node:test and node:assert — zero external dependencies.

import { test } from "node:test";
import assert from "node:assert/strict";

import {
  insert,
  backspace,
  deleteForward,
  left,
  right,
  home,
  end,
  clear,
} from "./line-editor.js";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** Shorthand: create state with given text; caret defaults to end of text. */
function s(text, caret = text.length) {
  return { text, caret };
}

// ---------------------------------------------------------------------------
// insert
// ---------------------------------------------------------------------------

test("insert: appends at end (caret at end)", () => {
  const result = insert(s("hello"), "!");
  assert.deepEqual(result, { text: "hello!", caret: 6 });
});

test("insert: prepends at start (caret 0)", () => {
  const result = insert(s("hello", 0), "!");
  assert.deepEqual(result, { text: "!hello", caret: 1 });
});

test("insert: inserts in the middle", () => {
  // "helo" with caret at 3 → insert "l" → "hello", caret 4
  const result = insert(s("helo", 3), "l");
  assert.deepEqual(result, { text: "hello", caret: 4 });
});

test("insert: multi-char (paste scenario)", () => {
  // "AB" caret 1 → paste "XY" → "AXYB", caret 3
  const result = insert(s("AB", 1), "XY");
  assert.deepEqual(result, { text: "AXYB", caret: 3 });
});

test("insert: into empty state", () => {
  const result = insert(s(""), "a");
  assert.deepEqual(result, { text: "a", caret: 1 });
});

test("insert: does not mutate input state", () => {
  const before = s("hello");
  insert(before, "!");
  assert.deepEqual(before, { text: "hello", caret: 5 });
});

// ---------------------------------------------------------------------------
// backspace
// ---------------------------------------------------------------------------

test("backspace: removes char before caret", () => {
  // "hello" caret 5 → "hell", caret 4
  const result = backspace(s("hello", 5));
  assert.deepEqual(result, { text: "hell", caret: 4 });
});

test("backspace: mid-line removes correct char", () => {
  // "hello" caret 2 → "hllo", caret 1
  const result = backspace(s("hello", 2));
  assert.deepEqual(result, { text: "hllo", caret: 1 });
});

test("backspace: no-op when caret is 0", () => {
  const result = backspace(s("hello", 0));
  assert.deepEqual(result, { text: "hello", caret: 0 });
});

test("backspace: no-op on empty string", () => {
  const result = backspace(s("", 0));
  assert.deepEqual(result, { text: "", caret: 0 });
});

// ---------------------------------------------------------------------------
// deleteForward
// ---------------------------------------------------------------------------

test("deleteForward: removes char at caret", () => {
  // "hello" caret 1 → removes 'e' → "hllo", caret stays 1
  const result = deleteForward(s("hello", 1));
  assert.deepEqual(result, { text: "hllo", caret: 1 });
});

test("deleteForward: at end of string is no-op", () => {
  const result = deleteForward(s("hello", 5));
  assert.deepEqual(result, { text: "hello", caret: 5 });
});

test("deleteForward: on empty string is no-op", () => {
  const result = deleteForward(s("", 0));
  assert.deepEqual(result, { text: "", caret: 0 });
});

test("deleteForward: at caret 0 removes first char", () => {
  const result = deleteForward(s("hello", 0));
  assert.deepEqual(result, { text: "ello", caret: 0 });
});

// ---------------------------------------------------------------------------
// left / right
// ---------------------------------------------------------------------------

test("left: moves caret left by 1", () => {
  const result = left(s("hello", 3));
  assert.deepEqual(result, { text: "hello", caret: 2 });
});

test("left: clamps to 0 (no underflow)", () => {
  const result = left(s("hello", 0));
  assert.deepEqual(result, { text: "hello", caret: 0 });
});

test("right: moves caret right by 1", () => {
  const result = right(s("hello", 2));
  assert.deepEqual(result, { text: "hello", caret: 3 });
});

test("right: clamps to text.length (no overflow)", () => {
  const result = right(s("hello", 5));
  assert.deepEqual(result, { text: "hello", caret: 5 });
});

// ---------------------------------------------------------------------------
// home / end
// ---------------------------------------------------------------------------

test("home: moves caret to 0", () => {
  const result = home(s("hello", 3));
  assert.deepEqual(result, { text: "hello", caret: 0 });
});

test("home: already at 0 is no-op", () => {
  const result = home(s("hello", 0));
  assert.deepEqual(result, { text: "hello", caret: 0 });
});

test("end: moves caret to text.length", () => {
  const result = end(s("hello", 2));
  assert.deepEqual(result, { text: "hello", caret: 5 });
});

test("end: already at end is no-op", () => {
  const result = end(s("hello", 5));
  assert.deepEqual(result, { text: "hello", caret: 5 });
});

// ---------------------------------------------------------------------------
// clear
// ---------------------------------------------------------------------------

test("clear: empties text and resets caret", () => {
  const result = clear(s("hello", 3));
  assert.deepEqual(result, { text: "", caret: 0 });
});

test("clear: on already-empty state is no-op", () => {
  const result = clear(s("", 0));
  assert.deepEqual(result, { text: "", caret: 0 });
});
