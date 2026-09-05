import { test } from "node:test";
import assert from "node:assert/strict";
import {
  parentDir,
  wordBounds,
  isArgumentPosition,
  splitPathToken,
  replaceRange,
  buildCompletionText,
} from "./path-utils.js";

test("Windows: file in sottocartella", () => {
  assert.equal(parentDir("C:\\a\\b\\file.pdf"), "C:\\a\\b");
});
test("Windows: file nella radice del drive", () => {
  assert.equal(parentDir("C:\\file.txt"), "C:\\");
});
test("POSIX: file in sottocartella", () => {
  assert.equal(parentDir("/a/b/file.txt"), "/a/b");
});
test("POSIX: file nella root", () => {
  assert.equal(parentDir("/file"), "/");
});
test("nessun separatore → invariato", () => {
  assert.equal(parentDir("file.txt"), "file.txt");
});
test("stringa vuota → invariato", () => {
  assert.equal(parentDir(""), "");
});

// ── wordBounds ──────────────────────────────────────────────────────────

test("wordBounds: caret alla fine di una parola singola", () => {
  assert.deepEqual(wordBounds("dir", 3), { start: 0, end: 3 });
});
test("wordBounds: caret dopo uno spazio, parola vuota", () => {
  assert.deepEqual(wordBounds("cd ", 3), { start: 3, end: 3 });
});
test("wordBounds: seconda parola parziale", () => {
  assert.deepEqual(wordBounds("type C:\\Users\\Doc", 18), { start: 5, end: 18 });
});
test("wordBounds: caret nel mezzo del testo (non a fine parola)", () => {
  // "cd sub|dir" con caret a 6 (dopo "sub") — solo "sub" è la parola fino al caret.
  assert.deepEqual(wordBounds("cd subdir", 6), { start: 3, end: 6 });
});
test("wordBounds: stringa vuota", () => {
  assert.deepEqual(wordBounds("", 0), { start: 0, end: 0 });
});

// ── isArgumentPosition ──────────────────────────────────────────────────

test("isArgumentPosition: prima parola della riga → false", () => {
  assert.equal(isArgumentPosition("dir", 0), false);
});
test("isArgumentPosition: prima parola con spazi iniziali → false", () => {
  assert.equal(isArgumentPosition("   dir", 3), false);
});
test("isArgumentPosition: seconda parola → true", () => {
  assert.equal(isArgumentPosition("cd sub", 3), true);
});
test("isArgumentPosition: terza parola (più argomenti) → true", () => {
  assert.equal(isArgumentPosition("copy a.txt b.txt", 11), true);
});

// ── splitPathToken ──────────────────────────────────────────────────────

test("splitPathToken: nessun separatore → dirPart vuoto", () => {
  assert.deepEqual(splitPathToken("Doc"), { dirPart: "", prefix: "Doc", quoteChar: null });
});
test("splitPathToken: Windows, sottocartella parziale", () => {
  assert.deepEqual(splitPathToken("C:\\Users\\Maurizio\\Doc"), {
    dirPart: "C:\\Users\\Maurizio\\",
    prefix: "Doc",
    quoteChar: null,
  });
});
test("splitPathToken: separatore finale, prefix vuoto (rientra in una directory)", () => {
  assert.deepEqual(splitPathToken("sub\\"), { dirPart: "sub\\", prefix: "", quoteChar: null });
});
test("splitPathToken: separatore POSIX", () => {
  assert.deepEqual(splitPathToken("a/b/fi"), { dirPart: "a/b/", prefix: "fi", quoteChar: null });
});
test("splitPathToken: virgolette doppie iniziali → quoteChar estratto, non nel prefix", () => {
  // Il caso segnalato dal vivo: 'cd "My' — prima del fix, prefix era '"My'
  // (con la virgoletta), che non trova mai corrispondenze reali.
  assert.deepEqual(splitPathToken('"My'), { dirPart: "", prefix: "My", quoteChar: '"' });
});
test("splitPathToken: virgolette singole iniziali", () => {
  assert.deepEqual(splitPathToken("'My"), { dirPart: "", prefix: "My", quoteChar: "'" });
});
test("splitPathToken: virgolette iniziali con sottocartella parziale", () => {
  assert.deepEqual(splitPathToken('"C:\\Users\\Maurizio\\Doc'), {
    dirPart: "C:\\Users\\Maurizio\\",
    prefix: "Doc",
    quoteChar: '"',
  });
});
// ── buildCompletionText ──────────────────────────────────────────────────

test("buildCompletionText: nessun dirPart, nessuna virgoletta", () => {
  assert.equal(buildCompletionText("", "Documents\\", null), "Documents\\");
});
test("buildCompletionText: dirPart ri-anteposto (bug trovato in review, 2026-07-19)", () => {
  // `list_path_completions` torna solo il basename ("File.txt"), mai dirPart
  // ("Sub\\") — buildCompletionText deve rimetterlo, altrimenti "Sub\\"
  // sparisce dalla riga: 'cd Sub\Fi<Tab>' diventava 'cd File.txt' invece di
  // 'cd Sub\File.txt'.
  assert.equal(buildCompletionText("Sub\\", "File.txt", null), "Sub\\File.txt");
});
test("buildCompletionText: virgolette avvolgono dirPart+candidato insieme", () => {
  assert.equal(buildCompletionText("", "My Kindle Content\\", '"'), '"My Kindle Content\\"');
});
test("buildCompletionText: dirPart + virgolette insieme", () => {
  assert.equal(
    buildCompletionText("Sub Folder\\", "My File.txt", '"'),
    '"Sub Folder\\My File.txt"'
  );
});

// ── replaceRange ──────────────────────────────────────────────────────────

test("replaceRange: sostituisce il prefix con il candidato completo", () => {
  assert.deepEqual(replaceRange("cd Doc", 3, 6, "Documents\\"), {
    text: "cd Documents\\",
    caret: 13,
  });
});
test("replaceRange: preserva testo dopo il range sostituito", () => {
  assert.deepEqual(replaceRange("cd Doc extra", 3, 6, "Documents\\"), {
    text: "cd Documents\\ extra",
    caret: 13,
  });
});
test("replaceRange: ciclo successivo sostituisce il candidato precedente", () => {
  // Prima sostituzione: "Doc" → "Documents\" (range 3..13).
  // Seconda (Tab successivo): sostituisce quel range con un altro candidato.
  assert.deepEqual(replaceRange("cd Documents\\", 3, 13, "Downloads\\"), {
    text: "cd Downloads\\",
    caret: 13,
  });
});
