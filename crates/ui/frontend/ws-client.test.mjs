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

import { test, mock } from "node:test";
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

// Sostituto avanzato di WebSocket per testare il ciclo di vita (open, close,
// error, message) e il retry/backoff esponenziale senza toccare la rete.
class MockEventWebSocket {
  static instances = [];

  constructor(url) {
    this.url = url;
    this.readyState = MockEventWebSocket.CONNECTING;
    this.listeners = new Map();
    this.sent = [];
    MockEventWebSocket.instances.push(this);
  }

  addEventListener(event, fn) {
    if (!this.listeners.has(event)) {
      this.listeners.set(event, []);
    }
    this.listeners.get(event).push(fn);
  }

  emit(event, ev = {}) {
    if (event === "open") {
      this.readyState = MockEventWebSocket.OPEN;
    } else if (event === "close") {
      this.readyState = MockEventWebSocket.CLOSED;
    }
    const handlers = this.listeners.get(event) || [];
    for (const h of handlers) {
      h(ev);
    }
  }

  send(data) {
    this.sent.push(data);
  }

  close() {
    this.readyState = MockEventWebSocket.CLOSED;
    this.emit("close");
  }
}
MockEventWebSocket.CONNECTING = 0;
MockEventWebSocket.OPEN = 1;
MockEventWebSocket.CLOSING = 2;
MockEventWebSocket.CLOSED = 3;

test("con maxRetries: 2, 1 apertura fallita + 2 retry falliti emettono failed e fermano i timer", () => {
  mock.timers.enable({ apis: ["setTimeout"] });
  const origWs = globalThis.WebSocket;
  globalThis.WebSocket = MockEventWebSocket;
  MockEventWebSocket.instances = [];

  try {
    const statuses = [];
    const client = new LareWsClient({
      url: "ws://127.0.0.1:7331",
      token: "t",
      maxRetries: 2,
      onStatus: (s) => statuses.push(s),
    });

    // 1. Connessione iniziale
    client.connect();
    assert.equal(MockEventWebSocket.instances.length, 1);
    const ws1 = MockEventWebSocket.instances[0];
    // Fallimento iniziale
    ws1.emit("error");
    ws1.emit("close");

    // Retry 1 (attesa 1000ms)
    mock.timers.tick(999);
    assert.equal(MockEventWebSocket.instances.length, 1); // non ancora scattato
    mock.timers.tick(1);
    assert.equal(MockEventWebSocket.instances.length, 2); // scattato retry 1
    const ws2 = MockEventWebSocket.instances[1];
    ws2.emit("error");
    ws2.emit("close");

    // Retry 2 (attesa 2000ms)
    mock.timers.tick(1999);
    assert.equal(MockEventWebSocket.instances.length, 2);
    mock.timers.tick(1);
    assert.equal(MockEventWebSocket.instances.length, 3); // scattato retry 2
    const ws3 = MockEventWebSocket.instances[2];
    ws3.emit("error");
    ws3.emit("close");

    // Dopo 2 retry falliti con maxRetries: 2, deve emettere "failed" come stato terminale
    const failedCount = statuses.filter((s) => s === "failed").length;
    assert.equal(failedCount, 1, "deve emettere 'failed' esattamente una volta");
    assert.equal(statuses[statuses.length - 1], "failed", "l'ultimo stato deve essere 'failed'");

    // Nessun ulteriore timer deve essere schedulato: avanzando il tempo non si creano altri socket
    mock.timers.tick(60000);
    assert.equal(MockEventWebSocket.instances.length, 3, "nessun socket aggiuntivo creato");
    assert.equal(statuses[statuses.length - 1], "failed", "lo stato resta 'failed'");
  } finally {
    globalThis.WebSocket = origWs;
    mock.timers.reset();
  }
});

test("senza maxRetries (default / host.js), il client continua a ritentare all'infinito e non emette mai failed", () => {
  mock.timers.enable({ apis: ["setTimeout"] });
  const origWs = globalThis.WebSocket;
  globalThis.WebSocket = MockEventWebSocket;
  MockEventWebSocket.instances = [];

  try {
    const statuses = [];
    const client = new LareWsClient({
      url: "ws://127.0.0.1:7331",
      token: "t",
      onStatus: (s) => statuses.push(s),
    });

    client.connect();
    // Simula 10 fallimenti consecutivi
    for (let i = 0; i < 10; i++) {
      const currentWs = MockEventWebSocket.instances[MockEventWebSocket.instances.length - 1];
      currentWs.emit("error");
      currentWs.emit("close");
      mock.timers.tick(10000); // tempo sufficiente a far scattare qualsiasi backoff (max 8s)
    }

    assert.equal(MockEventWebSocket.instances.length, 11, "1 iniziale + 10 retry");
    assert.equal(statuses.includes("failed"), false, "non deve mai emettere 'failed'");

    client.disconnect();
  } finally {
    globalThis.WebSocket = origWs;
    mock.timers.reset();
  }
});

test("server_info dopo un fallimento precedente resetta _retryMs e _retryCount", () => {
  mock.timers.enable({ apis: ["setTimeout"] });
  const origWs = globalThis.WebSocket;
  globalThis.WebSocket = MockEventWebSocket;
  MockEventWebSocket.instances = [];

  try {
    const statuses = [];
    const client = new LareWsClient({
      url: "ws://127.0.0.1:7331",
      token: "t",
      maxRetries: 6,
      onStatus: (s) => statuses.push(s),
    });

    // 1. Primo tentativo: fallisce
    client.connect();
    const ws1 = MockEventWebSocket.instances[0];
    ws1.emit("close");

    // Retry 1: attesa 1000ms
    mock.timers.tick(1000);
    const ws2 = MockEventWebSocket.instances[1];
    ws2.emit("close");

    // Retry 2: attesa 2000ms
    mock.timers.tick(2000);
    const ws3 = MockEventWebSocket.instances[2];

    // Questa volta l'handshake ha successo: open + server_info
    ws3.emit("open");
    ws3.emit("message", {
      data: JSON.stringify({ type: "server_info", version: "2.0.0" }),
    });

    assert.equal(statuses[statuses.length - 1], "connected");

    // Ora la connessione cade (nuovo ciclo di disconnessione)
    ws3.emit("close");

    // Il nuovo tentativo deve partire dal backoff iniziale (1000ms), non da 4000ms!
    mock.timers.tick(999);
    assert.equal(MockEventWebSocket.instances.length, 3, "non deve ancora essere scattato a 999ms");
    mock.timers.tick(1);
    assert.equal(MockEventWebSocket.instances.length, 4, "deve scattare a 1000ms (backoff resettato)");

    client.disconnect();
  } finally {
    globalThis.WebSocket = origWs;
    mock.timers.reset();
  }
});

test("apertura TCP riuscita ma server chiude subito: il backoff raddoppia e non resta fisso a 1s", () => {
  mock.timers.enable({ apis: ["setTimeout"] });
  const origWs = globalThis.WebSocket;
  globalThis.WebSocket = MockEventWebSocket;
  MockEventWebSocket.instances = [];

  try {
    const client = new LareWsClient({
      url: "ws://127.0.0.1:7331",
      token: "t",
    });

    // Connessione iniziale: TCP open ma il server chiude subito senza server_info
    client.connect();
    const ws1 = MockEventWebSocket.instances[0];
    ws1.emit("open");
    ws1.emit("close");

    // Retry 1 atteso dopo 1000ms
    mock.timers.tick(999);
    assert.equal(MockEventWebSocket.instances.length, 1);
    mock.timers.tick(1);
    assert.equal(MockEventWebSocket.instances.length, 2);

    // Retry 1: TCP open ma server chiude subito
    const ws2 = MockEventWebSocket.instances[1];
    ws2.emit("open");
    ws2.emit("close");

    // Se il bug di reset su open fosse ancora presente, il timer sarebbe ancora a 1000ms.
    // Con la correzione, il backoff deve essere cresciuto a 2000ms!
    mock.timers.tick(1000);
    assert.equal(MockEventWebSocket.instances.length, 2, "a 1000ms non deve ancora scattare se raddoppiato a 2000ms");
    mock.timers.tick(1000);
    assert.equal(MockEventWebSocket.instances.length, 3, "a 2000ms scatta il retry 2");

    // Retry 2: TCP open e chiusura immediata
    const ws3 = MockEventWebSocket.instances[2];
    ws3.emit("open");
    ws3.emit("close");

    // Ora il backoff deve essere cresciuto a 4000ms
    mock.timers.tick(2000);
    assert.equal(MockEventWebSocket.instances.length, 3, "a 2000ms non deve scattare");
    mock.timers.tick(2000);
    assert.equal(MockEventWebSocket.instances.length, 4, "a 4000ms scatta il retry 3");

    client.disconnect();
  } finally {
    globalThis.WebSocket = origWs;
    mock.timers.reset();
  }
});
