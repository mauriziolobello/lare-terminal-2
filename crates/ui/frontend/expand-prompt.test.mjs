import { test } from "node:test";
import assert from "node:assert/strict";
import { buildExpandPrompt, isConnectionFailureStatus, isEmptyExpandResult, isAiTurnFailure } from "./expand-prompt.mjs";

test("buildExpandPrompt embeds document and request with clear delimiters", () => {
  const prompt = buildExpandPrompt("# Sumeri\n\nPopolo antico.", "aggiungi i riferimenti bibliografici");
  assert.ok(prompt.includes("# Sumeri\n\nPopolo antico."), "deve contenere il documento intatto");
  assert.ok(prompt.includes("aggiungi i riferimenti bibliografici"), "deve contenere la richiesta");
  assert.ok(prompt.indexOf("# Sumeri") < prompt.indexOf("aggiungi i riferimenti"), "il documento precede la richiesta");
});

test("buildExpandPrompt handles empty document (edge case, should not happen in practice)", () => {
  const prompt = buildExpandPrompt("", "scrivi qualcosa sui Sumeri");
  assert.ok(prompt.includes("scrivi qualcosa sui Sumeri"));
});

test("isConnectionFailureStatus treats error as failure when not yet settled", () => {
  assert.equal(isConnectionFailureStatus("error", false), true);
});

test("isConnectionFailureStatus treats disconnected as failure when not yet settled", () => {
  assert.equal(isConnectionFailureStatus("disconnected", false), true);
});

test("isConnectionFailureStatus ignores error/disconnected once already settled", () => {
  assert.equal(isConnectionFailureStatus("error", true), false);
  assert.equal(isConnectionFailureStatus("disconnected", true), false);
});

test("isConnectionFailureStatus ignores connecting/connected regardless of settled", () => {
  assert.equal(isConnectionFailureStatus("connecting", false), false);
  assert.equal(isConnectionFailureStatus("connected", false), false);
});

test("isEmptyExpandResult rejects empty string", () => {
  assert.equal(isEmptyExpandResult(""), true);
});

test("isEmptyExpandResult rejects whitespace-only content", () => {
  assert.equal(isEmptyExpandResult("   \n\t  "), true);
});

test("isEmptyExpandResult accepts real content", () => {
  assert.equal(isEmptyExpandResult("# Sumeri\n\nTesto vero."), false);
});

test("isAiTurnFailure treats null exit_code as success (no failure)", () => {
  assert.equal(isAiTurnFailure(null), false);
});

test("isAiTurnFailure treats undefined exit_code as success (no failure)", () => {
  assert.equal(isAiTurnFailure(undefined), false);
});

test("isAiTurnFailure treats exit_code 1 as a failure", () => {
  assert.equal(isAiTurnFailure(1), true);
});

test("isAiTurnFailure treats exit_code 130 (cancellation) as a failure", () => {
  assert.equal(isAiTurnFailure(130), true);
});
