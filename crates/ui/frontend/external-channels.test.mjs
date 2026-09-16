import { test } from "node:test";
import assert from "node:assert/strict";
import { findExternalChannelBySlash, EXTERNAL_TOOL_CHANNELS } from "./external-channels.js";

test("finds a registered channel by exact trimmed slash trigger", () => {
  const registry = [{ id: "nmap", slashTrigger: "/nmap", windowTitle: "Nmap" }];
  assert.deepEqual(findExternalChannelBySlash("/nmap", registry), registry[0]);
  assert.deepEqual(findExternalChannelBySlash("  /nmap  ", registry), registry[0]);
});

test("returns null when no channel claims the input", () => {
  const registry = [{ id: "nmap", slashTrigger: "/nmap", windowTitle: "Nmap" }];
  assert.equal(findExternalChannelBySlash("/altro", registry), null);
});

// Le 3 verifiche sul registro DI PRODUZIONE sostituiscono il vecchio test
// "production registry is empty (no real channel yet)": da Task 5 il registro
// non è più vuoto (primo canale reale, nmap/netsec), quindi quell'asserzione è
// diventata falsa per costruzione — questi 3 test ne sono la copertura
// aggiornata, non un'aggiunta indipendente.
// Rinominato da "has exactly the nmap entry": da Task 3 (python-ping) il registro
// non ha più esattamente una voce — stesso principio già applicato lato Rust per
// production_registry_starts_with_netsec_channel (verifica solo l'indice 0, non la
// lunghezza esatta a lungo termine). id/slashTrigger aggiornati "nmap" → "netsec"
// col rename del canale (2026-09-12, compito netsec-rename-fritzbox.md) — questi
// 2 test erano rimasti scoperti dal rename (nessuno aveva rieseguito l'intera
// suite JS da allora), trovato e corretto il 2026-09-16.
test("EXTERNAL_TOOL_CHANNELS has the netsec entry", () => {
  assert.equal(EXTERNAL_TOOL_CHANNELS.length, 3);
  assert.equal(EXTERNAL_TOOL_CHANNELS[0].id, "netsec");
  assert.equal(EXTERNAL_TOOL_CHANNELS[0].slashTrigger, "/netsec");
});

test("findExternalChannelBySlash matches /netsec", () => {
  const found = findExternalChannelBySlash("/netsec", EXTERNAL_TOOL_CHANNELS);
  assert.equal(found?.id, "netsec");
});

test("EXTERNAL_TOOL_CHANNELS has the python-ping entry", () => {
  const found = findExternalChannelBySlash("/pyping", EXTERNAL_TOOL_CHANNELS);
  assert.equal(found?.id, "python-ping");
});

// Task 11 (2026-07-22): primo tool Python reale del dominio financial-markets
// — mirror dell'aggiunta lato Rust in external_channel.rs (EXTERNAL_TOOL_CHANNELS[3]).
test("EXTERNAL_TOOL_CHANNELS has the financial-markets entry", () => {
  const found = findExternalChannelBySlash("/markets", EXTERNAL_TOOL_CHANNELS);
  assert.equal(found?.id, "financial-markets");
});

test("findExternalChannelBySlash returns null for an unrelated slash command", () => {
  assert.equal(findExternalChannelBySlash("/config", EXTERNAL_TOOL_CHANNELS), null);
});
