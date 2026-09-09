// ws-client.test.mjs — test di wiring puro per LareWsClient.
//
// Nota TDD: questo test è stato scritto DOPO il metodo che verifica
// (sendTestMarketDataSource), non prima — deviazione dall'ordine RED→GREEN
// richiesto dal progetto. Motivo: il codice segue un pattern esistente
// (runExpand/library-expand in window.js) privo di test dedicati per lo
// stesso motivo — wiring DOM/WS con I/O di rete reale — e non esisteva
// alcun ws-client.test.mjs da cui partire in RED. Aggiunto qui a
// posteriori su richiesta esplicita di revisione, non come precedente per
// futuri sviluppi: il ciclo corretto resta RED prima del codice.
//
// Isola SOLO la serializzazione del messaggio sul wire (nessun vero
// WebSocket): un FakeWebSocket minimale intercetta send() e ci permette
// di ispezionare il payload JSON esatto, mirror del contratto in
// crates/protocol/src/lib.rs (ClientMsg::TestMarketDataSource).

import { test } from "node:test";
import assert from "node:assert/strict";
import { LareWsClient } from "./ws-client.js";

// Sostituto minimale di WebSocket: nessuna connessione di rete reale,
// registra solo ciò che viene inviato tramite send().
class FakeWebSocket {
  constructor() {
    this.readyState = FakeWebSocket.OPEN;
    this.sent = [];
  }
  addEventListener() {}
  send(data) {
    this.sent.push(data);
  }
  close() {}
}
FakeWebSocket.OPEN = 1;

test("sendTestMarketDataSource invia ClientMsg::TestMarketDataSource sul wire", () => {
  const client = new LareWsClient({
    url: "ws://127.0.0.1:7331",
    token: "t",
    channel: "config-market-data-test",
  });
  // Inietta direttamente un socket "aperto" fake, bypassando l'evento
  // asincrono "open" di connect() — qui ci interessa solo _send/_isOpen.
  client._ws = new FakeWebSocket();

  const ok = client.sendTestMarketDataSource("req-123");

  assert.equal(ok, true);
  assert.equal(client._ws.sent.length, 1);
  assert.deepEqual(JSON.parse(client._ws.sent[0]), {
    type: "test_market_data_source",
    id: "req-123",
  });
});

test("sendTestMarketDataSource ritorna false se il socket non è aperto", () => {
  const client = new LareWsClient({
    url: "ws://127.0.0.1:7331",
    token: "t",
    channel: "config-market-data-test",
  });
  // Nessun connect() chiamato: _ws resta null, quindi _isOpen() è false.
  const ok = client.sendTestMarketDataSource("req-456");
  assert.equal(ok, false);
});

test("sendCommand invia ClientMsg::Command con lang default vuoto", () => {
  const client = new LareWsClient({
    url: "ws://127.0.0.1:7331",
    token: "t",
  });
  client._ws = new FakeWebSocket();

  const ok = client.sendCommand("dir", "cmd-1");

  assert.equal(ok, true);
  assert.equal(client._ws.sent.length, 1);
  assert.deepEqual(JSON.parse(client._ws.sent[0]), {
    type: "command",
    id: "cmd-1",
    input: "dir",
    input_mode: "keyboard",
    command_type: "auto",
    cwd: null,
    web_search: false,
    lang: "",
  });
});

test("sendCommand usa lang configurato nel constructor", () => {
  const client = new LareWsClient({
    url: "ws://127.0.0.1:7331",
    token: "t",
    lang: "en",
  });
  client._ws = new FakeWebSocket();

  const ok = client.sendCommand("dir", "cmd-2");

  assert.equal(ok, true);
  assert.equal(client._ws.sent.length, 1);
  assert.deepEqual(JSON.parse(client._ws.sent[0]), {
    type: "command",
    id: "cmd-2",
    input: "dir",
    input_mode: "keyboard",
    command_type: "auto",
    cwd: null,
    web_search: false,
    lang: "en",
  });
});

test("sendCommand rispetta setLanguage e argomento esplicito", () => {
  const client = new LareWsClient({
    url: "ws://127.0.0.1:7331",
    token: "t",
    lang: "it",
  });
  client._ws = new FakeWebSocket();

  client.setLanguage("en");
  client.sendCommand("echo hi", "cmd-3", true);

  assert.equal(client._ws.sent.length, 1);
  assert.deepEqual(JSON.parse(client._ws.sent[0]), {
    type: "command",
    id: "cmd-3",
    input: "echo hi",
    input_mode: "keyboard",
    command_type: "auto",
    cwd: null,
    web_search: true,
    lang: "en",
  });

  // Argomento esplicito vince su client._lang
  client.sendCommand("echo ciao", "cmd-4", false, "it");
  assert.equal(client._ws.sent.length, 2);
  assert.deepEqual(JSON.parse(client._ws.sent[1]), {
    type: "command",
    id: "cmd-4",
    input: "echo ciao",
    input_mode: "keyboard",
    command_type: "auto",
    cwd: null,
    web_search: false,
    lang: "it",
  });
});

test("sendCommand ritorna false se il socket non è aperto", () => {
  const client = new LareWsClient({
    url: "ws://127.0.0.1:7331",
    token: "t",
  });
  const ok = client.sendCommand("dir", "cmd-closed");
  assert.equal(ok, false);
});
