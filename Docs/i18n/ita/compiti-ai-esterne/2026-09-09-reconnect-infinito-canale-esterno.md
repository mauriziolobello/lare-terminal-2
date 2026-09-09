# Compito per AI esterna — reconnect infinito su un canale esterno rotto in modo permanente

> **Prima cosa**: leggi per intero `Docs/i18n/ita/BRIEFING-AI-ESTERNE.md` alla radice del
> repository, poi questo file per intero, PRIMA di scrivere codice.

## Cosa è successo

Issue tracciata in `Docs/i18n/ita/KNOWN-ISSUES.md`, voce `[APERTO] Reconnect infinito su un
canale esterno permanentemente rotto` — debito architetturale ereditato dalla v1, mai risolto.
Root cause verificata leggendo il codice (non assunta):

`crates/ui/frontend/ws-client.js`, classe `LareWsClient`, metodo `_scheduleRetry()` (righe
510-519): il backoff esponenziale è **capped** a `RETRY_MAX_MS` (8000ms) ma **non ha mai un
numero massimo di tentativi** — se il socket continua a chiudersi, il client ritenta all'infinito,
per sempre, senza mai smettere né segnalare uno stato terminale.

**Quando il bug si manifesta davvero (verificato lato server):** `crates/orchestrator/src/ws.rs`,
righe 199-224 — se `hello.channel` è un id sconosciuto, o la costruzione del `ToolClient` del
canale fallisce (es. `venv` Python mancante per `/pyping`, config assente per `/markets`), il
server manda `ServerMsg::Error` e poi **chiude la connessione** (`sink.close()`). Questo è un
close reale, non un errore applicativo dentro una connessione che resta aperta — quindi
`ws-client.js` lo vede come un normale `close` event e riparte con `_scheduleRetry()`. Un canale
esterno con backend rotto in modo permanente (non un guasto transitorio) fa ripetere questo ciclo
per sempre.

## Un secondo bug, più sottile, trovato durante l'indagine — in scope

`_openSocket()` (righe 460-472): il backoff (`_retryMs`) viene resettato a `RETRY_INITIAL_MS`
sull'evento `"open"` del WebSocket (riga 471) — **livello TCP**, non sull'accettazione effettiva
dell'handshake applicativo (`server_info` ricevuto, gestito più sotto nello stesso file, righe
483-486). Conseguenza: un server che accetta la connessione TCP ma poi la chiude subito dopo aver
letto `hello` (esattamente il caso di `ws.rs` sopra — `sink.close()` avviene DOPO che il TCP
handshake è già completo) fa sì che `_retryMs` si resetti a `RETRY_INITIAL_MS` ad ogni giro, quindi
il client ritenta a **intervallo costante di ~1s per sempre**, invece di crescere fino al cap di
8s come la documentazione del backoff lascia intendere. Sintomo peggiore di quello oggi descritto
in KNOWN-ISSUES.md.

## I 4 consumer di `LareWsClient` — comportamento attuale verificato

- **`host.js`** (riga 128): `onStatus: (s) => console.info("[host] ws status:", s)` — nessun
  feedback visibile, nessuna auto-mitigazione. Questa è la connessione principale (canale
  cursore, `channel: undefined`).
- **`external-channel-window.js`** (righe 148-157): apre la finestra usata da `/nmap`, `/markets`,
  `/pyping` (i canali esterni con processo/config, vedi `crates/orchestrator/src/external_channel.rs`
  righe 240, 276, ~300). `onStatus: (s) => renderer.setStatus(s)` → `renderer.js::setStatus()`
  (riga 209) mostra un badge di stato tradotto (`ext_channel.status_{connecting,connected,
  disconnected,error}`) ma **non ha uno stato terminale**: il badge cicla `connecting` →
  `disconnected` → `connecting` → … all'infinito. Questo è il sintomo reale che l'utente vede.
- **`window.js`** (righe ~335-375, feature "library-expand") e **`config-dialog.js`**
  (`_runMarketDataTest`, righe ~570-625, test-connessione del pannello "Dati Mercato"): **NON**
  sono rotti in pratica oggi — entrambi usano già `isConnectionFailureStatus(status,
  alreadySettled)` (da `crates/ui/frontend/expand-prompt.mjs`, righe 33-45) per trattare il primo
  segnale `"error"`/`"disconnected"` come fatale per la loro richiesta one-shot e chiamano subito
  `client.disconnect()`. `window.js` ha già un commento esplicito nel codice che cita il retry
  infinito di `ws-client.js` come motivo di questa guardia.

## Vincolo architetturale — LEGGERE PRIMA DI PROGETTARE LA FIX

**`host.js` deve continuare a ritentare all'infinito.** Non è un'omissione da correggere: se
l'orchestratore si riavvia (build, crash, self-heal), la connessione cursore principale deve
riconnettersi quando torna disponibile, che sia dopo 31 secondi o 3 minuti. `host.js::onStatus`
oggi fa solo `console.info` — con un limite di tentativi globale, questa connessione smetterebbe
di ritentare in silenzio e l'utente si troverebbe con la UI morta finché non riavvia `ui.exe` a
mano. Sarebbe peggio del bug che stai risolvendo.

**Per questo il limite di tentativi deve essere un'opzione per-istanza, mai un default globale.**
Vedi design sotto.

## Design della fix

### 1. `LareWsClient` — nuovo parametro opzionale `maxRetries`

Nel costruttore (`crates/ui/frontend/ws-client.js`, righe 43-69), aggiungi `maxRetries` alla
destrutturazione delle opzioni:

- `undefined`/`null` (default, nessun valore passato) → comportamento **invariato**: retry
  all'infinito. `host.js` non passa questo parametro e non va toccato.
- Un numero `N` → dopo N tentativi di retry falliti consecutivi, il client smette e emette uno
  stato terminale (vedi punto 3) invece di schedulare un altro `setTimeout`.

Aggiungi un contatore di stato, es. `this._retryCount = 0`, accanto a `this._retryMs`.

**Semantica esatta di `maxRetries` (da rispettare, non da reinterpretare)**: è il numero di
tentativi di RIconnessione dopo il primo fallimento — il primo `_openSocket()` chiamato da
`connect()` non conta. Con `maxRetries: 6` e i valori attuali di `RETRY_INITIAL_MS`/`RETRY_MAX_MS`
la sequenza di attese è 1s, 2s, 4s, 8s, 8s, 8s (~31s totali) prima di arrendersi — verifica questo
calcolo scrivendo un test che lo dimostra, non fidarti a occhio.

### 2. Punto di reset del backoff e del contatore — sposta da `"open"` a `server_info`

Corregge anche il secondo bug (sopra). Rimuovi `this._retryMs = RETRY_INITIAL_MS;` dalla riga 471
(dentro il listener `"open"`) e spostalo (insieme al reset di `this._retryCount = 0`) dentro il
branch `if (msg.type === "server_info")` del listener `"message"` (righe 483-486) — quello è il
punto in cui l'handshake applicativo è davvero accettato, non la semplice apertura TCP.

`connect()` (righe 82-85) deve **anche lui** azzerare `_retryMs` e `_retryCount` all'inizio, prima
di chiamare `_openSocket()` — così una riconnessione manuale (es. riapertura della finestra) parte
sempre con un budget di tentativi fresco, indipendentemente da quanti ne erano già stati consumati
prima.

### 3. Nuovo stato terminale `"failed"`

Quando `_scheduleRetry()` (righe 510-519) viene chiamato e `maxRetries` è impostato e
`this._retryCount >= maxRetries`: **non** schedulare un altro `setTimeout`, **non** incrementare
più nulla — emetti `this._onStatus("failed")` una sola volta e fermati lì. Nessun ulteriore
tentativo automatico finché non arriva una nuova chiamata a `connect()`.

`disconnect()` (righe 88-98) già cancella correttamente un `_retryTimer` pendente (`clearTimeout`)
— verificato leggendo il metodo per intero, non serve toccarlo.

### 4. `external-channel-window.js` — unico consumer che passa `maxRetries`

Nella costruzione di `LareWsClient` (righe 148-157), aggiungi `maxRetries: 6` all'oggetto opzioni.
Nessun altro file passa questo parametro — `host.js`, `window.js`, `config-dialog.js` restano
com'è (i due one-shot già si disconnettono da soli al primo errore, non arrivano mai a consumare
un `maxRetries`; `host.js` deve restare a retry infinito, vedi vincolo sopra).

### 5. `renderer.js::setStatus()` — render dello stato `"failed"`

Righe 209-217: aggiungi una riga al `labels`:
```js
failed: t("ext_channel.status_failed"),
```
Stessa struttura delle 4 righe esistenti. Il badge deve mostrare questo testo e **restare fermo
lì** (nessun altro cambio di stato arriva finché l'utente non chiude/riapre la finestra, che
costruisce un nuovo `LareWsClient` da zero).

### 6. Traduzioni — nuova chiave `ext_channel.status_failed`

Aggiungi la chiave in tutti e 3 i file lingua, stesso registro delle chiavi
`ext_channel.status_disconnected`/`ext_channel.status_error` già presenti:
- `Test Run/Configuration/i18n/it.json`
- `Test Run/Configuration/i18n/en.json`
- `Test Run/Configuration/i18n/es.json`

**Importante — non è opzionale**: `crates/ui/frontend/i18n-parity.test.mjs` (righe 136-153) verifica
in ENTRAMBE le direzioni — ogni chiave usata nel codice (`t("...")`) deve esistere in tutti i
dizionari, E ogni chiave nei dizionari deve essere effettivamente usata nel codice. Se aggiungi la
chiave in `renderer.js` ma non in tutti e 3 i JSON (o viceversa), il test fallisce.

## Test — TDD, RED prima del codice

`crates/ui/frontend/ws-client.test.mjs` esiste già ma il suo `FakeWebSocket` (righe 23-34) è
minimale: intercetta solo `send()`, non simula `"open"`/`"close"`/`"error"` né dà controllo sui
timer di `setTimeout`. **Per testare `maxRetries` devi estendere quel fake** (o scriverne uno
nuovo nello stesso file) in modo che possa:
- disparare `"open"` e poi `"close"` a comando (per simulare un server che accetta e poi chiude,
  come fa `ws.rs` su canale rotto);
- non disparare mai `"open"` (per simulare un server irraggiungibile).

Per i timer, usa `node:test`'s `mock.timers` (disponibile da Node 20+, verifica la versione di
Node nel tuo ambiente con `node --version` prima di usarlo) per far avanzare il tempo senza
attese reali — non introdurre `setTimeout` reali nei test, sarebbero lenti e fragili.

Test minimi richiesti (scrivi il RED prima dell'implementazione):
1. Con `maxRetries: 2`: 1 apertura fallita + 2 retry falliti → `onStatus` riceve `"failed"`
   esattamente una volta come ultimo stato, nessun timer resta pendente dopo (verifica che un
   ulteriore avanzamento del tempo simulato non produca altri tentativi).
2. Senza `maxRetries` (comportamento di `host.js`): dopo N fallimenti arbitrariamente alti (es.
   10), `onStatus` non riceve mai `"failed"` — continua a ritentare.
3. `server_info` ricevuto dopo un fallimento precedente resetta `_retryMs`/`_retryCount`: verifica
   che dopo un ciclo fallimento→successo→nuovo fallimento, il primo retry del secondo ciclo usi di
   nuovo `RETRY_INITIAL_MS`, non un valore già cresciuto dal ciclo precedente.
4. Apertura TCP riuscita ma il server chiude subito dopo (mai arrivato `server_info`): il backoff
   deve CRESCERE ad ogni tentativo (1s, 2s, 4s, 8s, 8s…), non restare fisso a 1s — questo è il
   test che dimostra la correzione del secondo bug (punto 2 del design).

## Documentazione e versioni

- `crates/ui`: bump patch (`ws-client.js`, `external-channel-window.js`, `renderer.js` toccati).
  `CHANGELOG.md` + `IMPLEMENTATION.md` — descrivi sia il fix del retry infinito sia il fix del
  punto di reset del backoff, sono due bug distinti anche se risolti nello stesso commit.
- `Docs/i18n/ita/HANDOFF.md`: una riga in FATTO.
- `Docs/i18n/ita/KNOWN-ISSUES.md`: sposta la voce da `[APERTO]` a `[RISOLTO]`, aggiungi una nota
  che la connessione principale (`host.js`, canale cursore) mantiene deliberatamente il retry
  infinito — non è stata "dimenticata", è un vincolo architetturale (vedi sezione sopra in questo
  compito, citala o riassumila).

## Verifica finale

```powershell
node --test crates/ui/frontend/*.test.mjs
cargo build -p orchestrator   # invariato, verifica solo che non hai toccato nulla lato Rust
cargo clippy -p orchestrator --all-targets
```

Non c'è codice Rust da modificare in questo compito — se `cargo build`/`clippy` mostrano diff
rispetto a prima del tuo lavoro, qualcosa è andato storto.

Se il tuo ambiente ha un display: apri `/nmap` (o un'altra voce di `EXTERNAL_TOOL_CHANNELS`),
osserva il badge di stato. Riprodurre dal vivo un canale "permanentemente rotto" richiede
manomettere temporaneamente il backend (es. rinominare temporaneamente il `venv` di un tool
Python) — se lo fai, ripristina tutto prima di committare. Se non puoi verificare dal vivo,
scrivilo nel report — il supervisore la fa lui prima del merge.

## Cosa NON fare

- Non introdurre `maxRetries` come default globale in `LareWsClient` — deve restare `undefined`
  finché non lo passa esplicitamente chi costruisce il client. `host.js` NON deve essere toccato.
- Non aggiungere `maxRetries` a `window.js` o `config-dialog.js` — hanno già la loro guardia
  (`isConnectionFailureStatus`), che resta fuori scope: non toccare `expand-prompt.mjs`.
- Non introdurre un pulsante "Riprova" manuale nell'interfaccia — fuori scope, non richiesto da
  KNOWN-ISSUES.md. Se hai un'opinione in merito, scrivila nel report, non implementarla.
- Non toccare `crates/orchestrator/src/ws.rs` o `crates/orchestrator/src/external_channel.rs` —
  il comportamento server-side (chiudere la connessione su canale sconosciuto/rotto) è corretto
  così com'è ed è fuori scope; questo compito è solo lato client.
- Non toccare `crates/protocol` — nessun campo nuovo sul wire, `maxRetries` è puro stato locale
  del client JS.
