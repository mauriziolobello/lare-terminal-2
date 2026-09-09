// admission.test.mjs — TDD tests for the pure admission module (gate 1 join
// prompt text + re-request cooldown state). Nessuna dipendenza da DOM/Tauri:
// solo node:test + node:assert, come gli altri moduli .mjs del frontend.
//
// Run with: node --test crates/ui/frontend/admission.test.mjs

import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { initI18n } from "./i18n.mjs";

import { joinPromptText, rerequestState, removeResolvedCandidate } from "./admission.mjs";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const itDict = JSON.parse(
  fs.readFileSync(path.resolve(__dirname, "../../../Test Run/Configuration/i18n/it.json"), "utf8")
);
initI18n(itDict);

// ---------------------------------------------------------------------------
// joinPromptText
// ---------------------------------------------------------------------------

test("joinPromptText: nessun presente → domanda generica", () => {
  assert.equal(joinPromptText([]), "Vuoi entrare in chat?");
});

test("joinPromptText: un presente → 'con A?'", () => {
  assert.equal(joinPromptText(["A"]), "Vuoi entrare in chat con A?");
});

test("joinPromptText: due presenti → congiunti con 'e'", () => {
  assert.equal(joinPromptText(["A", "B"]), "Vuoi entrare in chat con A e B?");
});

test("joinPromptText: tre presenti → virgole + 'e' prima dell'ultimo", () => {
  assert.equal(
    joinPromptText(["A", "B", "C"]),
    "Vuoi entrare in chat con A, B e C?"
  );
});

// ---------------------------------------------------------------------------
// rerequestState
// ---------------------------------------------------------------------------

test("rerequestState: retryAtMs assente → abilitato, nessun countdown", () => {
  assert.deepEqual(rerequestState(1000, null), { enabled: true, secondsLeft: 0 });
});

test("rerequestState: retryAtMs raggiunto esattamente → abilitato", () => {
  assert.deepEqual(rerequestState(1000, 1000), { enabled: true, secondsLeft: 0 });
});

test("rerequestState: cooldown attivo → disabilitato con secondi arrotondati per eccesso", () => {
  // 4000 - 1000 = 3000ms esatti → 3s
  assert.deepEqual(rerequestState(1000, 4000), { enabled: false, secondsLeft: 3 });
});

test("rerequestState: differenza non multipla di 1000 → ceil", () => {
  // 3500 - 1000 = 2500ms → ceil(2.5) = 3s
  assert.deepEqual(rerequestState(1000, 3500), { enabled: false, secondsLeft: 3 });
});

// ---------------------------------------------------------------------------
// removeResolvedCandidate (FIX #7 — review 2026-07-03): quando il server manda
// `ai_chat_admission_resolved`, il candidato va tolto dalla coda del gate 2, così
// il banner "ammetti X?" non resta appeso per un voto già deciso altrove.
// ---------------------------------------------------------------------------

test("removeResolvedCandidate: rimuove il candidato dalla coda", () => {
  assert.deepEqual(removeResolvedCandidate(["A", "B", "C"], "B"), ["A", "C"]);
});

test("removeResolvedCandidate: il candidato in testa (banner mostrato) esce", () => {
  assert.deepEqual(removeResolvedCandidate(["A", "B"], "A"), ["B"]);
});

test("removeResolvedCandidate: candidato assente → coda invariata (idempotente)", () => {
  assert.deepEqual(removeResolvedCandidate(["A", "B"], "Z"), ["A", "B"]);
});

test("removeResolvedCandidate: rimuove eventuali duplicati (notifica ripetuta innocua)", () => {
  assert.deepEqual(removeResolvedCandidate(["A", "B", "A"], "A"), ["B"]);
});

test("removeResolvedCandidate: coda vuota → resta vuota", () => {
  assert.deepEqual(removeResolvedCandidate([], "A"), []);
});
