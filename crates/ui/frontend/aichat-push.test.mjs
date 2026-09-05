import { test } from "node:test";
import assert from "node:assert/strict";
import { createAiChatPushGate, isNotifiableDrop } from "./aichat-push.js";

test("push senza richiesta d'apertura scarta l'evento (niente auto-apertura all'avvio)", () => {
  const gate = createAiChatPushGate();
  const result = gate.push("aichat:self", { label: "skimble-human" });
  assert.deepEqual(result, { action: "drop" });
});

test("requestOpen poi push bufferizza (apertura in corso, finestra non ancora pronta)", () => {
  const gate = createAiChatPushGate();
  gate.requestOpen();
  const result = gate.push("aichat:self", { label: "skimble-human" });
  assert.deepEqual(result, { action: "buffer" });
});

test("più push durante l'apertura si accumulano nell'ordine d'arrivo", () => {
  const gate = createAiChatPushGate();
  gate.requestOpen();
  gate.push("aichat:self", { label: "skimble-human" });
  gate.push("aichat:roster", { participants: ["skimble-human"] });
  const replay = gate.ready();
  assert.deepEqual(replay, [
    { event: "aichat:self", payload: { label: "skimble-human" } },
    { event: "aichat:roster", payload: { participants: ["skimble-human"] } },
  ]);
});

test("ready() passa live e svuota il buffer; push successivi vanno emessi subito", () => {
  const gate = createAiChatPushGate();
  gate.requestOpen();
  gate.push("aichat:self", { label: "skimble-human" });
  gate.ready();
  assert.equal(gate.isLive(), true);
  const result = gate.push("aichat:msg", { from_label: "skimble-human", text: "ciao" });
  assert.deepEqual(result, { action: "emit" });
});

test("closed() azzera lo stato: un push successivo torna a scartare", () => {
  const gate = createAiChatPushGate();
  gate.requestOpen();
  gate.push("aichat:self", { label: "skimble-human" });
  gate.ready();
  gate.closed();
  assert.equal(gate.isLive(), false);
  const result = gate.push("aichat:roster", { participants: [] });
  assert.deepEqual(result, { action: "drop" });
});

test("closed() durante l'apertura (finestra chiusa prima di 'ready') scarta il buffer accumulato", () => {
  const gate = createAiChatPushGate();
  gate.requestOpen();
  gate.push("aichat:self", { label: "skimble-human" });
  gate.closed();
  gate.requestOpen();
  const replay = gate.ready();
  assert.deepEqual(replay, [], "il buffer della apertura precedente non deve sopravvivere alla chiusura");
});

test("isNotifiableDrop è vero per un messaggio scartato", () => {
  assert.equal(isNotifiableDrop("aichat:msg", "drop"), true);
});

test("isNotifiableDrop è vero per una richiesta di condivisione scartata", () => {
  assert.equal(isNotifiableDrop("aichat:share-request", "drop"), true);
});

test("isNotifiableDrop è vero per il gate 1 (candidato) scartato", () => {
  assert.equal(isNotifiableDrop("aichat:join-prompt", "drop"), true);
});

test("isNotifiableDrop è vero per il gate 2 (server) scartato", () => {
  assert.equal(isNotifiableDrop("aichat:admission-request", "drop"), true);
});

test("isNotifiableDrop è falso per un evento di solo stato scartato (roster)", () => {
  assert.equal(isNotifiableDrop("aichat:roster", "drop"), false);
});

test("isNotifiableDrop è falso se action è 'buffer' anche per un evento notificabile", () => {
  assert.equal(isNotifiableDrop("aichat:msg", "buffer"), false);
});

test("isNotifiableDrop è falso per aichat:join-request (evento morto, mai emesso dal backend)", () => {
  assert.equal(isNotifiableDrop("aichat:join-request", "drop"), false);
});
