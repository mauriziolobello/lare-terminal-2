import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { initI18n } from "./i18n.mjs";
import {
  messageLine,
  rosterText,
  consentPrompt,
  isSelf,
  historyEntries,
  chatWindowTitle,
  peerLostText,
} from "./aichat-view.mjs";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const itDict = JSON.parse(
  fs.readFileSync(path.resolve(__dirname, "../../../Test Run/Configuration/i18n/it.json"), "utf8")
);
initI18n(itDict);

test("messageLine passa label e text", () => {
  assert.deepEqual(messageLine({ from_label: "skimble-human", text: "ciao" }), {
    label: "skimble-human",
    text: "ciao",
    is_ai: false,
  });
});

test("messageLine usa display_name quando presente, affiancato all'etichetta macchina", () => {
  const line = messageLine({ from_label: "skimble-ai", text: "ciao", display_name: "Aria", is_ai: true });
  assert.deepStrictEqual(line, { label: "Aria / skimble", text: "ciao", is_ai: true });
});

test("messageLine con display_name su umano affianca comunque l'etichetta macchina", () => {
  const line = messageLine({ from_label: "skimble-human", text: "ciao", display_name: "Maurizio", is_ai: false });
  assert.deepStrictEqual(line, { label: "Maurizio / skimble", text: "ciao", is_ai: false });
});

test("messageLine disambigua lo stesso nickname su due macchine diverse", () => {
  const a = messageLine({ from_label: "rumpleteazer-human", text: "ciao", display_name: "Maurizio" });
  const b = messageLine({ from_label: "skimble-human", text: "ciao", display_name: "Maurizio" });
  assert.notEqual(a.label, b.label);
  assert.equal(a.label, "Maurizio / rumpleteazer");
  assert.equal(b.label, "Maurizio / skimble");
});

test("messageLine ricade su from_label quando display_name è assente", () => {
  const line = messageLine({ from_label: "skimble-human", text: "ciao", display_name: null, is_ai: false });
  assert.deepStrictEqual(line, { label: "skimble-human", text: "ciao", is_ai: false });
});

test("messageLine tratta is_ai assente come false (compat peer vecchi)", () => {
  const line = messageLine({ from_label: "skimble-human", text: "ciao" });
  assert.deepStrictEqual(line, { label: "skimble-human", text: "ciao", is_ai: false });
});

test("rosterText vuoto → trattino", () => {
  assert.equal(rosterText([]), "presenti: —");
  assert.equal(rosterText(undefined), "presenti: —");
});

test("rosterText unisce con ·", () => {
  assert.equal(rosterText(["a", "b", "c"]), "presenti: a · b · c");
});

test("consentPrompt include il peer", () => {
  assert.match(consentPrompt("macavity"), /macavity/);
});

test("isSelf true se label inizia con la mia base", () => {
  assert.equal(isSelf("skimble-human", "skimble"), true);
  assert.equal(isSelf("macavity-human", "skimble"), false);
});

test("historyEntries assente → array vuoto", () => {
  assert.deepEqual(historyEntries(undefined), []);
  assert.deepEqual(historyEntries(null), []);
});

test("historyEntries preserva l'array e l'ordine (forma grezza, non messageLine)", () => {
  const entries = [
    { from_label: "skimble-human", text: "ciao" },
    { from_label: "quaxo-human", text: "ehi" },
  ];
  assert.deepEqual(historyEntries(entries), entries);
});

test("chatWindowTitle con etichetta nota", () => {
  assert.equal(chatWindowTitle("skimble-human"), "AICHAT - skimble-human");
});

test("chatWindowTitle senza etichetta (non ancora arrivata da AiChatSelf) → solo AICHAT", () => {
  assert.equal(chatWindowTitle(null), "AICHAT");
  assert.equal(chatWindowTitle(undefined), "AICHAT");
  assert.equal(chatWindowTitle(""), "AICHAT");
});

test("peerLostText formats a departure notice", () => {
  assert.equal(peerLostText("quaxo-human"), "quaxo-human risulta scollegato");
});

test("peerLostText handles a missing label gracefully", () => {
  assert.equal(peerLostText(undefined), "un peer risulta scollegato");
});
