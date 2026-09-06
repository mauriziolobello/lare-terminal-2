# Changelog — `protocol`

All notable changes to this crate are documented here.
Format: [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).
Versioning: `major.minor.update` (SemVer).

## 2.1.0 — 2026-09-06 — ruolo della connessione + canale shell (piano 2a, Task 1)

Fondamenta del canale "shell" (spec `Docs/i18n/ita/superpowers/specs/2026-09-04-lare-terminal-2-design.md`
§3.2/§4.1): la connessione WS dichiara ora un **ruolo** (`ui` o `shell`), e l'orchestratore
può instradare un `ServerMsg` verso l'origine del turno o verso il sink `ui.exe` a seconda
del messaggio.

- **`Role { Ui, Shell }`** (nuovo enum di supporto) — wire `"ui"`/`"shell"`. `Default = Ui`:
  un client v1 che non manda il campo resta valido.
- **`ClientMsg::Hello`** guadagna quattro campi additivi (tutti `#[serde(default)]`, nessuna
  regressione sui client esistenti): `role: Role`, `session_id: Option<String>`,
  `cwd: Option<String>`, `version: Option<String>`.
- **`ClientMsg::ExecResult { turn_id, exec_id, exit_code, output, cwd }`** (wire:
  `"exec_result"`) — esito di un `ExecInShell` eseguito da `lare-shell` nel runspace
  dell'utente.
- **`ClientMsg::UiPong { id, version }`** (wire: `"ui_pong"`) — risposta di `ui.exe` a
  `ServerMsg::UiPing` (built-in `/ping`).
- **`ServerMsg::ExecInShell { turn_id, exec_id, command, capture }`** (wire:
  `"exec_in_shell"`) — chiede alla shell dell'utente di eseguire `command`; `capture`
  distingue output catturato da console attaccata (programmi interattivi).
- **`ServerMsg::OpenOutputWindow { window_id, title }`** (wire: `"open_output_window"`) —
  apre su `ui` la finestra Markdown di output di un comando slash originato dalla shell.
- **`ServerMsg::OutputWindowContent { window_id, markdown }`** (wire:
  `"output_window_content"`) — sostituisce il contenuto di quella finestra.
- **`ServerMsg::OpenUiLocal { name }`** (wire: `"open_ui_local"`) — chiede a `ui` di aprire
  (o portare in primo piano) una finestra locale (`"config"`, `"library"`, `"aichat"`, o
  l'id di un canale esterno).
- **`ServerMsg::UiPing { id }`** (wire: `"ui_ping"`) — richiesta di vita a `ui.exe`
  (built-in `/ping`); risposta `UiPong`.
- **`ServerMsg::ActivityIndicator { session_id, kind, on }`** (wire:
  `"activity_indicator"`) — segnalino di stato per la finestra terminale della sessione
  (consumato dal piano 3, già emesso qui).
- **`Surface { Origin, Ui }`** (nuovo enum, non serializzato) + **`ServerMsg::surface(&self)
  -> Surface`** — decide, per ogni variante `ServerMsg`, se torna alla connessione origine
  del turno (`Origin`) o va al sink `ui` della macchina (`Ui`). Il `match` è esaustivo
  **senza wildcard** di proposito: una variante nuova senza riga nella tabella non compila.

Tutte le aggiunte sono additive (Contratto A): nessuna variante esistente cambia forma sul
wire. Test: `hello_without_role_defaults_to_ui`, `hello_shell_round_trips_all_new_fields`,
`exec_in_shell_and_exec_result_wire_names`, `output_window_ui_local_ping_indicator_wire_names`,
`surface_origin_for_turn_messages_and_ui_for_window_messages`.

## 2.0.0 — 2026-09-05 — fork da v1 0.15.4

Copia del crate dalla v1 (`mauriziolobello/lare-terminal`) nel repo 2.0. Nessuna modifica
funzionale in questa voce; le modifiche del piano 1 seguono nelle voci successive.

## [0.15.4] — 2026-08-14 — Market data source test (`TestMarketDataSource`/`MarketDataSourceTestResult`)

Due varianti additive (Contratto A) per il test di raggiungibilità di una fonte dati mercato:

- **`ClientMsg::TestMarketDataSource { id: String }`** (wire: `"test_market_data_source"`) —
  richiede un test della connessione alla fonte dati mercato ATTUALMENTE selezionata (letta da
  `market_data.json` lato Python). `id` correla la risposta (`ServerMsg::MarketDataSourceTestResult`),
  stesso pattern di `Command`/`Done`.

- **`ServerMsg::MarketDataSourceTestResult { id, ok, message }`** (wire: `"market_data_source_test_result"`) —
  risposta a `TestMarketDataSource`. `ok: true` se la connessione è riuscita, `false` altrimenti.
  `message` riporta feedback umano-leggibile (es. "Connesso a IBKR", "Timeout", ecc).

Varianti puramente additive: nessuna variante esistente cambia. Test: `test_market_data_source_client_msg_round_trips`,
`market_data_source_test_result_server_msg_round_trips`. Design:
`Docs/superpowers/specs/2026-08-14-financial-markets-ibkr-data-source/task-10.md`.

## [0.15.3] — 2026-08-13 — `ChatLine`/`AiChatMessage`: `display_name`/`is_ai` additivi

Due campi nuovi, entrambi `#[serde(default)]` (nessuna regressione su peer/storico
vecchi che non li mandano):

- `display_name: Option<String>` — nickname risolto dal mittente al momento
  dell'invio (umano o AI). `None` → il frontend mostra `from_label` come oggi
  (fallback invariato).
- `is_ai: bool` — `true` se il mittente è l'AI, non un umano. Pilota badge/icona
  nel frontend AI Chat. `false` di default: comportamento storico (nessun badge
  esisteva prima).

Aggiunti a `ChatLine` (usata da `ServerMsg::AiChatHistory`) e a
`ServerMsg::AiChatMessage`. Task 2 del piano
`Docs/superpowers/plans/2026-08-13-aichat-display-names.md`. Nessun consumatore
ancora in questo crate — la risoluzione dei valori arriva con Task 3
(`orchestrator::aichat::service`) e la resa visiva col Task 9 (frontend).

## [0.15.2] — 2026-08-11 — `ServerMsg::OpenScreenerPicker`

Nuovo struct `ScreenerListItem { id, title, description }` + variante additiva
`ServerMsg::OpenScreenerPicker { items: Vec<ScreenerListItem> }` (wire
`{"type":"open_screener_picker","items":[...]}`). Apre la finestra di
selezione screener del canale `financial-markets` quando l'AI chiama
`list_screeners` senza che l'utente abbia nominato uno screener specifico.
Vedi `Docs/superpowers/specs/2026-08-11-markets-screener-registry-design.md`.

## [0.15.1] — 2026-08-07 — `ServerMsg::Heartbeat`

Nuova variante additiva `Heartbeat { id: String }` (wire `{"type":"heartbeat","id":"..."}`).
Battito di vita per un comando in-flight, scollegato dal contenuto — origina da un evento SSE
`ping` osservato da `messages_client.rs` (orchestrator), propagato fino al watchdog frontend per
distinguere "silenzio perché il turno lavora ancora" da "silenzio perché è bloccato". Vedi
`Docs/superpowers/specs/2026-08-07-ai-turn-heartbeat-watchdog-design.md`.

## [0.15.0] — 2026-08-05 — `ServerMsg::RoutineSavePreview` (additiva)

Nuovo `ServerMsg::RoutineSavePreview` (additivo — Contratto A) — apre una finestra
di anteprima dedicata per `save_routine` (Fase 2 del repository di routine), invece
del banner Sì/No generico di `ToolConfirmRequest`. Risposta invariata:
`ClientMsg::ToolConfirmResponse`. Vedi `Docs/superpowers/specs/2026-08-05-save-routine-design.md` §5.

## [0.14.9] — 2026-07-31 — `NoteView::my_segment_text` (additiva)

Nuovo campo: il segmento di QUESTA sola macchina (stringa vuota se non ha
ancora contribuito), distinto da `body` (corpo fuso di tutte le macchine).
Root cause di un bug trovato nel primo smoke test dal vivo multi-macchina
del Blocco note: la finestra "Modifica" non aveva modo di sapere cosa
precompilare nel box di testo — restava vuota anche su note con contenuto
proprio. Design §10 lo prevedeva fin dall'inizio; una limitazione temporanea
della shape di `NoteView` lo aveva rimandato.

## [0.14.8] — 2026-07-29 — `NoteView` + `ClientMsg`/`ServerMsg` per Blocco note (additiva)

Nuovi messaggi WS orchestrator↔UI locale per la scheda Library "Note":
`NoteCreate`/`NoteEdit`/`NoteEditTitle`/`NoteDelete` (client→orchestrator),
`NotesSnapshot`/`NoteUpserted` (orchestrator→client). `NoteView` è la forma
"dumb" per il frontend — corpo già renderizzato, niente segmenti grezzi.
Design: `Docs/superpowers/specs/2026-07-29-library-notes-design.md` §7.

## [0.14.7] — 2026-07-28 — `ServerMsg::AiChatReachablePeers` (additiva)

Nuovo segnale per Library "Condividi": etichette dei peer scoperti via UDP
E con un link TCP vivo, indipendente dall'ammissione alla stanza AI Chat
(`AiChatRoster`). Root cause: `request_share` (orchestrator, aichat/
service.rs) non ha mai richiesto l'ammissione — risolve da `peers ∩ links`
— ma il dialog "Condividi" leggeva `AiChatRoster`, un sottoinsieme troppo
stretto (richiede il gate umano "vuoi entrare?"). Design:
`Docs/superpowers/specs/2026-07-28-library-share-reachable-peers-design.md`.

## [0.14.6] — 2026-07-18 — rimossa `ServerMsg::SaveToLibrary`

Rollback del salvataggio automatico in Library introdotto in 0.14.5: primo
uso dal vivo (report di uno scan nmap) ha prodotto un doppione in Library —
il pulsante "Salva" preesistente in ogni finestra Markdown (`window.js`) e
questo salvataggio automatico scattavano entrambi sullo stesso contenuto.
Decisione esplicita dell'utente: il salvataggio resta scelta dell'utente
(pulsante esistente), niente automatismo — non serve una seconda via.
Variante mai stata nella storia di una release rilasciata al di fuori di
questo ciclo di sviluppo, rimozione pulita anziché deprecazione additiva.

## [0.14.5] — 2026-07-16 — `ServerMsg::SaveToLibrary`

Nuova variante `{title, content}` — istruisce la UI a salvare un documento in
Library senza il protocollo di consenso/ack di `ShareIncomingData` (nessun
peer coinvolto). Primo consumatore: il report di uno scan nmap
(`Docs/superpowers/specs/2026-07-16-mcp-nmap-design.md` §6).

## [0.14.4] — 2026-07-16 — `ClientMsg::Hello` guadagna `channel` (canale tool esterno)

Aggiunta additiva (Contratto A): `Hello { token, channel: Option<String> }`.
`#[serde(default)]` garantisce che un client che non manda `channel` (tutto il
codice esistente) deserializzi a `None` — zero comportamento cambiato per le
connessioni di oggi. Prerequisito per il canale tool esterno (base comune),
consumato per la prima volta da `orchestrator` in questa stessa release.
Design: `Docs/superpowers/specs/2026-07-16-external-tool-channel-design.md`.

## [0.14.3] — 2026-07-15 — Gate di conferma locale per tool sensibili (`ToolConfirmRequest`/`ToolConfirmResponse`)

Due nuove varianti additive: `ServerMsg::ToolConfirmRequest { id, commands }` e
`ClientMsg::ToolConfirmResponse { id, accept }`. Estendono ADR-007 (gate di conferma
per i tool AI) oltre il solo canale Telegram — la UI locale potrà gateizzare tool
specifici marcati "sensibili" (nessuno ancora in questa versione). Design:
`Docs/superpowers/specs/2026-07-15-local-tool-confirm-gate-design.md`.

## [0.14.2] — 2026-07-12 — `OpenPluginWindow` guadagna `width`/`height` opzionali (dimensione finestra plugin)

Aggiunta additiva (Contratto A) per permettere a un plugin di dichiarare la dimensione
iniziale della sua finestra (dal `plugin.json`, via `plugin_protocol::WindowSize`).

### Added

- `ServerMsg::OpenPluginWindow.width: Option<f64>` e `.height: Option<f64>`, entrambi `#[serde(default, skip_serializing_if = "Option::is_none")]`.

Quando il plugin non dichiara una dimensione (`None`), i due campi spariscono del tutto dal
wire JSON: un client vecchio vede esattamente `{"type":"open_plugin_window","window_id":N,"title":"...","html":"..."}` come prima e ricade sul default 480×360. Nessuna rottura del Contratto A.
2 test: `open_plugin_window_carries_optional_size` (round-trip con dimensione + `"width":960.0`/`"height":620.0` nel wire) e l'esistente `open_plugin_window_roundtrip_and_tag` esteso per asserire che `width`/`height` NON compaiono quando `None`.

## [0.14.1] — 2026-07-10 — `SearchHit` guadagna `line`/`snippet` (ricerca nei contenuti)

Aggiunta additiva per la ricerca nel **contenuto** dei file (`/find in:"<frase>"`, orchestrator
0.40.4): `ServerMsg::SearchHit` guadagna due campi opzionali, entrambi
`#[serde(skip_serializing_if = "Option::is_none")]`.

### Added

- `SearchHit.line: Option<u32>` — numero di riga (1-based) del match nel contenuto.
- `SearchHit.snippet: Option<String>` — testo della riga che matcha (troncato a 200 caratteri).

Entrambi `None` per un hit solo-nome (comportamento `/find` v1, invariato): con
`skip_serializing_if`, quando assenti spariscono del tutto dal JSON invece di serializzarsi come
`"line":null` — un client vecchio, che non conosce questi campi, vede esattamente lo stesso wire
di prima. Nessuna rottura del Contratto A. 2 nuovi test: `search_hit_with_line_and_snippet_roundtrip`
(round-trip con entrambi i campi valorizzati) e `search_hit_without_line_omits_it_from_wire`
(assenza ⇒ i campi non compaiono affatto nel JSON). Spec:
`Docs/superpowers/specs/2026-07-10-ricerca-contenuti-design.md`.

## [0.14.0] — 2026-07-06 — Trasferimento contenuto Share (Slice 2a)

Nuove varianti additive per il round-trip di richiesta-contenuto: `ServerMsg::ShareContentRequest`,
`ServerMsg::ShareIncomingData`, `ClientMsg::ShareContent`, `ClientMsg::ShareContentFailed`,
`ClientMsg::ShareWritten`. L'orchestrator non ha accesso al filesystem della Library (confine SRP)
— deve chiedere il contenuto alla UI dopo un `ChatMsg::ShareAccept`, e la UI del destinatario deve
confermare la scrittura su disco. Spec:
`Docs/superpowers/specs/2026-07-06-library-share-slice2a-content-transfer-design.md`.

## [0.13.0] — 2026-07-05 — `doc_name` in ServerMsg::ShareResult

Aggiunta additiva per la UI di Slice 1a (`Docs/superpowers/specs/2026-07-05-library-share-ui-slice1a-design.md`):
senza il nome del documento, un mittente con più condivisioni in volo verso la
stessa macchina non potrebbe distinguere a quale si riferisce un esito.

### Added

- `ServerMsg::ShareResult` guadagna il campo `doc_name: String`.

---

## [0.12.0] — 2026-07-05 — Library "Share with": wire types per il consenso (Slice 1a)

Nuovi tipi per la Slice 1a di `Docs/superpowers/specs/2026-07-02-library-share-with-design.md`:
`ShareTarget`/`ShareOutcome` (enum di supporto), `ClientMsg::ShareDocument`/`ShareConsent`,
`ServerMsg::ShareRequest`/`ShareResult`. Nessun trasferimento di contenuto ancora (arriva in
Slice 2 via `ChatMsg` peer↔peer, fuori da questo crate). Puramente additivo.

### Added

- `ShareTarget::{One{label_base}, All}`, `ShareOutcome::{Accepted, Rejected, Failed{reason}}`.
- `ClientMsg::ShareDocument{rel_path, doc_name, size_bytes, target}` — `doc_name`/`size_bytes`
  sono risolti lato UI (l'orchestrator non ha accesso al filesystem della Library).
- `ClientMsg::ShareConsent{share_id, accept}`.
- `ServerMsg::ShareRequest{share_id, from_label, doc_name, size_bytes}` —
  `from_label` è il `label_base` nudo (la macchina, non un umano/AI).
- `ServerMsg::ShareResult{share_id, target_label, outcome}`.

---

## [0.11.0] — 2026-07-03 — AI Chat: notifica di chiusura del voto di ammissione (fix review #7)

### Added

- **`ServerMsg::AiChatAdmissionResolved { candidate }`** (wire `ai_chat_admission_resolved`):
  il server dice a un PRESENTE che il voto di ammissione per `candidate` si è concluso
  (ammesso, rifiutato, o candidato sparito), così la UI toglie il banner del gate 2. Gemello
  "di chiusura" di `AiChatAdmissionRequest`.

Additiva (Contract A rispettato): nessuna variante/campo esistente rimosso o modificato — i
peer/UI più vecchi continuano a deserializzare senza problemi. Chiude il finding **#7** della
review 2026-07-03 (banner del gate 2 che restava appeso quando il voto si risolveva altrove:
veto di un altro presente, timeout, o uscita del candidato → click sul banner no-op silenzioso).

## [0.10.0] — 2026-07-03 — AI Chat: ammissione alla stanza (2 gate, sostituisce il consenso pairwise)

### Added

Nuove varianti additive per il modello di ammissione a voto che sostituisce il vecchio
consenso pairwise/asimmetrico (`AiChatJoinRequest`/`AiChatJoinConsent`, restano per
retro-compatibilità ma non sono più il percorso primario). Chiude il **Debito #1**
(consenso lato ingresso) segnalato nell'hardening AI Chat. Design:
`Docs/superpowers/specs/2026-07-03-aichat-admission-consent-design.md`; piano:
`Docs/superpowers/plans/2026-07-03-aichat-admission.md`.

#### `ServerMsg` additions

| Variant | Wire type | Fields | Purpose |
|---------|-----------|--------|---------|
| `AiChatJoinPrompt` | `"ai_chat_join_prompt"` | `present: Vec<String>` | Gate 1, al **nuovo arrivato**: "vuoi entrare in chat con [present]?", prima di qualunque richiesta di ammissione |
| `AiChatAdmissionRequest` | `"ai_chat_admission_request"` | `candidate: String` | Gate 2, a un **presente** già ammesso: "ammetti `candidate`? sì/no" |
| `AiChatPending` | `"ai_chat_pending"` | `present: Vec<String>` | Stato di attesa del nuovo arrivato dopo il gate 1: il voto è in corso, la UI **blocca l'input** |
| `AiChatAdmitted` | `"ai_chat_admitted"` | _(none)_ | Il nuovo arrivato è stato ammesso (nessun veto, o silenzio-oltre-timeout) |
| `AiChatRejected` | `"ai_chat_rejected"` | `retry_after_secs: u32` | Il nuovo arrivato è stato rifiutato (almeno un veto); cooldown minimo prima di poter ri-chiedere |

#### `ClientMsg` additions

| Variant | Wire type | Fields | Purpose |
|---------|-----------|--------|---------|
| `AiChatJoinDecision` | `"ai_chat_join_decision"` | `accept: bool` | Risposta al gate 1: `false` annulla il tentativo, `true` fa scattare la richiesta di ammissione |
| `AiChatAdmissionVote` | `"ai_chat_admission_vote"` | `candidate: String`, `accept: bool` | Risposta al gate 2: `false` è un veto immediato (basta un solo no) |
| `AiChatRequestAdmission` | `"ai_chat_request_admission"` | _(none)_ | Pulsante "chiedi di entrare" dopo un rifiuto: re-invia la richiesta |

Tutte additive: nessuna variante esistente cambia forma. Round-trip serde + wire-format
assertions per tutte e 8 le nuove varianti in `src/lib.rs`.

---

## [0.9.4] — 2026-07-02 — AI Chat: `AiChatPeerLost` (annuncio keepalive)

### Added

- **`ServerMsg::AiChatPeerLost { label: String }`** (wire: `"ai_chat_peer_lost"`) — un peer è
  sparito dalla stanza AI Chat (keepalive scaduto o disconnessione pulita). Usato in entrambe
  le direzioni: un client sparito (annunciato dal server) o il proprio server sparito
  (rilevato in locale dal client). Evento **live**: non entra mai in `AiChatHistory`. Variante
  puramente additiva. Vedi `Docs/superpowers/specs/2026-07-02-aichat-keepalive-design.md`.

### Ripple edits

- `crates/orchestrator/src/telegram/channel.rs:156-161` — match esaustivo su `ServerMsg`:
  aggiunto `AiChatPeerLost { .. }` al gruppo no-op AI Chat (superficie locale, Telegram non la
  rappresenta) — stesso trattamento permanente delle altre varianti AiChat lì, non un bridge
  temporaneo.

---

## [0.9.3] — 2026-07-01 — AI Chat: `AiChatSelf` (etichetta propria per il titolo finestra)

### Added

- **`ServerMsg::AiChatSelf { label: String }`** (wire: `"ai_chat_self"`) — la propria
  etichetta umana nella stanza AI Chat (es. `"skimble-human"`), mandata quando la UI si
  (ri)connette al canale. La finestra-chat la usa per il titolo (sapere "questa è la mia
  finestra" senza doverlo indovinare dal roster). Variante puramente additiva.

---

## [0.9.2] — 2026-07-01 — AI Chat: `AiChatHistory` (storico messaggi persistente)

### Added

- **`ChatLine { from_label: String, text: String }`** (nuova struct) — una riga di storico chat.
- **`ServerMsg::AiChatHistory { entries: Vec<ChatLine> }`** (wire: `"ai_chat_history"`) — dump in
  blocco dello storico messaggi: emesso alla riapertura della finestra-chat (se lo storico non è
  vuoto) e quando un peer si (ri)unisce alla stanza. Variante puramente additiva. Vedi
  `Docs/superpowers/specs/2026-07-01-aichat-history-persistence-design.md`.

### Ripple edits

- `crates/orchestrator/src/telegram/channel.rs:156` — match esaustivo su `ServerMsg`: aggiunto
  `AiChatHistory { .. }` al gruppo no-op AI Chat (superficie locale, Telegram non la rappresenta).

---

## [0.9.1] — 2026-07-01 — AI Chat: `AiChatOpen` per riapertura finestra

### Added

- **`ClientMsg::AiChatOpen {}`** (wire: `"ai_chat_open"`) — inviato dalla UI ogni volta che
  la finestra-chat viene (ri)aperta. Permette all'orchestrator di ri-registrare il sink WS
  (`SetServerTx`) e ri-emettere il roster noto al nuovo webview. Senza questo messaggio,
  dopo la chiusura della finestra il `server_tx` rimaneva `None` e la riapertura non riceveva
  più messaggi né vedeva i "presenti". Variante puramente additiva: client che non la inviano
  continuano a funzionare (comportamento pre-fix preservato al primo `SetServerTx`).

---

## [0.9.0] — 2026-06-28 — AI Chat (Slice 1a-ui): 6 varianti additive

### Added — AI Chat variants (Contract A, Slice 1a-ui)

Sei nuove varianti (3 `ServerMsg` + 3 `ClientMsg`) per il canale AI Chat peer-to-peer
tra istanze Lare (Slice 1a-ui, vedi `Docs/21-plugin-ai-to-ai.md`).
Tutte additive: nessuna variante esistente cambia.

#### `ServerMsg` additions

| Variant | Wire type | Fields | Purpose |
|---------|-----------|--------|---------|
| `AiChatMessage` | `"ai_chat_message"` | `from_label: String`, `text: String` | Un messaggio nella stanza AI Chat; `from_label` è l'etichetta del mittente |
| `AiChatRoster` | `"ai_chat_roster"` | `participants: Vec<String>` | Lista autorevole dei partecipanti presenti nella stanza |
| `AiChatJoinRequest` | `"ai_chat_join_request"` | `peer_label: String` | Un nuovo peer è comparso in LAN: l'UI deve chiedere il consenso all'umano |

#### `ClientMsg` additions

| Variant | Wire type | Fields | Purpose |
|---------|-----------|--------|---------|
| `AiChatSend` | `"ai_chat_send"` | `text: String` | L'umano invia un messaggio nella stanza AI Chat |
| `AiChatJoinConsent` | `"ai_chat_join_consent"` | `peer_label: String`, `accept: bool` | Risposta al consenso d'ingresso per uno specifico peer |
| `AiChatClosed` | `"ai_chat_closed"` | _(none)_ | La superficie chat è stata chiusa: l'umano esce dalla stanza |

### TDD cycle (RED → GREEN)

**RED** (errori di compilazione prima di aggiungere le varianti):
```
error[E0599]: no variant named `AiChatMessage` found for enum `ServerMsg`
error[E0599]: no variant named `AiChatRoster` found for enum `ServerMsg`
error[E0599]: no variant named `AiChatJoinRequest` found for enum `ServerMsg`
error[E0599]: no variant named `AiChatSend` found for enum `ClientMsg`
error[E0599]: no variant named `AiChatJoinConsent` found for enum `ClientMsg`
(+1 — compile failed)
```

**GREEN** dopo l'aggiunta delle 3 varianti `ServerMsg` + 3 varianti `ClientMsg`.

Nuovi test (49 totali; 3 aggiunti):

| Test | Asserzione chiave |
|------|-------------------|
| `aichat_message_roundtrip_and_tag` | `ServerMsg::AiChatMessage` sopravvive serde; JSON contiene `"type":"ai_chat_message"` |
| `aichat_roster_and_join_request_roundtrip` | `AiChatRoster` e `AiChatJoinRequest` sopravvivono serde |
| `aichat_client_messages_roundtrip_and_tags` | Tutti e 3 i `ClientMsg` AI Chat sopravvivono serde; wire contiene `"type":"ai_chat_send"` |

## [0.8.0] — 2026-06-27

### Added — plugin window variants (Contract A, Slice 1 — additive)

Three new `ServerMsg` variants (orchestrator → UI) and two new `ClientMsg`
variants (UI → orchestrator) for the plugin-window round-trip.  All additive:
no existing variant changes.

#### `ServerMsg` additions

| Variant | Wire type | Fields | Purpose |
|---------|-----------|--------|---------|
| `OpenPluginWindow` | `"open_plugin_window"` | `window_id: u64`, `title: String`, `html: String` | Instructs the UI to open a new plugin window with the given HTML content |
| `UpdatePluginWindow` | `"update_plugin_window"` | `window_id: u64`, `html: String` | Full-HTML replacement of an existing plugin window's content |
| `ClosePluginWindow` | `"close_plugin_window"` | `window_id: u64` | Instructs the UI to close the plugin window |

#### `ClientMsg` additions

| Variant | Wire type | Fields | Purpose |
|---------|-----------|--------|---------|
| `PluginUiEvent` | `"plugin_ui_event"` | `window_id: u64`, `element_id: String`, `value: Option<String>` | UI interaction event from a plugin window (data-evt element clicked or changed) |
| `PluginWindowClosed` | `"plugin_window_closed"` | `window_id: u64` | Notifies the orchestrator that the user closed a plugin window |

#### Ripple edits (exhaustive matches in orchestrator)

Adding the new variants required explicit no-op arms in two exhaustive `match`
blocks:
- `crates/orchestrator/src/ws.rs:223` — `match client_msg`: added arms for
  `PluginUiEvent { .. }` and `PluginWindowClosed { .. }` (no-op; wired in Task 5).
- `crates/orchestrator/src/telegram/channel.rs:117` — `match msg`: added arm for
  `OpenPluginWindow { .. } | UpdatePluginWindow { .. } | ClosePluginWindow { .. }`
  (no-op; Telegram has no plugin-window surface).

### TDD cycle (RED → GREEN)

**RED** (compile errors before adding variants):
```
error[E0599]: no variant named `OpenPluginWindow` found for enum `ServerMsg`
  --> crates\protocol\src\lib.rs:846:28
error[E0599]: no variant named `PluginUiEvent` found for enum `ClientMsg`
  --> crates\protocol\src\lib.rs:864:28
(2 errors — compile failed)
```

**GREEN** after adding the 3 `ServerMsg` + 2 `ClientMsg` variants.

New tests (46 total; 2 added):

| Test | Key assertion |
|------|---------------|
| `open_plugin_window_roundtrip_and_tag` | `ServerMsg::OpenPluginWindow` survives serde; JSON contains `"type":"open_plugin_window"` |
| `plugin_ui_event_roundtrip` | `ClientMsg::PluginUiEvent` survives serde; JSON contains `"type":"plugin_ui_event"` |

## [0.7.0] — 2026-06-24

### Added — `ClientMsg::PauseSearch` / `ResumeSearch` (search pause/resume)

- **`ClientMsg::PauseSearch { id: String }`** (new additive variant):
  - Wire: `{"type":"pause_search","id":"<search-id>"}` (`snake_case` discriminator).
  - Semantics: instructs the orchestrator to pause the running search identified
    by `id`. The walkers suspend at the next cooperative checkpoint; search state
    is preserved so the search can be resumed.

- **`ClientMsg::ResumeSearch { id: String }`** (new additive variant):
  - Wire: `{"type":"resume_search","id":"<search-id>"}` (`snake_case` discriminator).
  - Semantics: instructs the orchestrator to resume a previously paused search
    identified by `id`. The walkers continue from where they stopped.

Both variants are additive; the overall enum uses
`#[serde(tag = "type", rename_all = "snake_case")]` and pattern matches in
`ws.rs` (orchestrator) will require updating to handle the new arms.

### TDD cycle (RED → GREEN)

**RED** (compile errors before adding variants):
```
error[E0599]: no variant named `PauseSearch` found for enum `ClientMsg`  (3 occurrences)
error[E0599]: no variant named `ResumeSearch` found for enum `ClientMsg` (3 occurrences)
(6 errors — compile failed)
```

**GREEN** after adding `PauseSearch { id: String }` and `ResumeSearch { id: String }`
to `ClientMsg`.

New tests (44 total; 6 added):

| Test | Key assertion |
|------|---------------|
| `pause_search_roundtrip` | `ClientMsg::PauseSearch` survives serde round-trip |
| `pause_search_wire_format` | JSON contains `"type":"pause_search"` and `"id":"s1"` |
| `pause_search_deserialize_handwritten` | hand-written JSON → correct variant |
| `resume_search_roundtrip` | `ClientMsg::ResumeSearch` survives serde round-trip |
| `resume_search_wire_format` | JSON contains `"type":"resume_search"` and `"id":"s1"` |
| `resume_search_deserialize_handwritten` | hand-written JSON → correct variant |

## [0.6.0] — 2026-06-23

### Added — `ServerMsg::Cwd` (cwd tracking, Approach B)

- **`ServerMsg::Cwd { path: String }`** (new additive variant):
  - Wire: `{"type":"cwd","path":"<absolute-path>"}` (`snake_case` discriminator via `serde(tag = "type", rename_all = "snake_case")`).
  - Semantics: carries the current working directory of the orchestrator's
    persistent shell session. Emitted once at connection time (initial cwd) and
    again whenever the cwd changes after a command execution.
  - Additive: no existing `match` over `ServerMsg` outside `protocol` is
    exhaustive; the compiler will catch any non-exhaustive `match` at compile time.

### TDD cycle (RED → GREEN)

**RED** (compile errors before adding `Cwd`):
```
error[E0599]: no variant named `Cwd` found for enum `ServerMsg`
  --> crates\protocol\src\lib.rs:702:30  (server_cwd_roundtrip)
error[E0599]: no variant named `Cwd` found for enum `ServerMsg`
  --> crates\protocol\src\lib.rs:714:30  (server_cwd_wire_format)
error[E0599]: no variant named `Cwd` found for enum `ServerMsg`
  --> crates\protocol\src\lib.rs:735:24  (server_cwd_deserialize_handwritten)
(3 errors — compile failed)
```

**GREEN** after adding `Cwd { path: String }` to `ServerMsg`.

New tests (38 total; 3 added):

| Test | Key assertion |
|------|---------------|
| `server_cwd_roundtrip` | `ServerMsg::Cwd` survives serde round-trip |
| `server_cwd_wire_format` | JSON contains `"type":"cwd"` and `"path":"C:\\Users\\x"` |
| `server_cwd_deserialize_handwritten` | hand-written `{"type":"cwd","path":"..."}` → correct variant |

## [0.5.0] — 2026-06-22

### Added — messaggi per la ricerca file

- `ServerMsg::SearchOpen { id, title }`, `SearchHit { id, path, source }`,
  `SearchDone { id, count, truncated }`.
- `SearchSource { Cwd | Standard | Cloud | External }` (wire `snake_case`).
- `ClientMsg::CancelSearch { id }` (annulla una ricerca in corso).
- `WindowKind::Search` (finestra di ricerca live).

## [0.4.0] — 2026-06-21

### Added — `/help` feature: `WindowKind::Help` variant (ADR-013 additive extension)

- **`WindowKind::Help`** (new variant, additive):
  - Wire: `"help"` (`snake_case` of `Help`; existing `Markdown` → `"markdown"` unchanged).
  - Semantics: system help window. The UI applies a distinct visual style
    (`body.help` CSS class) to distinguish it from regular Markdown output windows.
  - Enum designed for extension: adding `Help` is a pure addition — no existing
    `match` outside `protocol` is exhaustive (uses `..` or is closed), so no
    downstream breakage. The compiler would catch any non-exhaustive `match` at
    compile time.

### TDD cycle (RED → GREEN)

**RED** (compile error before adding `Help`):
```
error[E0599]: no variant or associated function named `Help` found for enum `WindowKind`
  --> crates/protocol/src/lib.rs:553:34
   |
553|         let kind = WindowKind::Help;
   |                                ^^^^ variant not found for this enum
(3 occurrences — 3 test functions reference WindowKind::Help)
```

**GREEN** after adding `Help` to the `WindowKind` enum.

New tests (30 total; 3 added):

| Test | Key assertion |
|------|---------------|
| `window_kind_help_roundtrip` | `WindowKind::Help` survives serde round-trip |
| `window_kind_help_wire_is_help_string` | serialises to `"help"` (snake_case) |
| `open_window_help_wire_format` | `ServerMsg::OpenWindow{Help}` JSON contains `"kind":"help"` and `"type":"open_window"` |

## [0.3.0] — 2026-06-21

### Added — Web search: campo `web_search` su `Command` (ADR-014)

- **`ClientMsg::Command.web_search: bool`** (nuovo campo additivo):
  - Wire: `"web_search": true/false`.
  - Decorato con `#[serde(default)]`: i client che non lo inviano (es. `test-ws.mjs`,
    client legacy) ottengono `false` senza errori di deserializzazione.
  - Semantica: `true` autorizza la ricerca web interna dell'AI per quel turno;
    il cablaggio avviene nell'orchestrator (Task 3). Per ora il campo viene
    destructurato come `web_search: _` in `ws.rs` (ignorato — intentional).

### TDD cycle (RED → GREEN)

**RED** (compile error prima dell'aggiunta del campo):
```
error[E0026]: variant `ClientMsg::Command` does not have a field named `web_search`
error[E0559]: variant `ClientMsg::Command` has no field named `web_search`
```

**GREEN** dopo l'aggiunta di `#[serde(default)] pub web_search: bool`.

Literal `ClientMsg::Command { … }` aggiornati nei test esistenti (6 occorrenze):
`client_command_roundtrip`, `client_command_with_cwd_roundtrip`, `command_wire_format`,
`deserialize_handwritten_command_json`, `deserialize_handwritten_command_with_cwd`,
`deserialize_handwritten_command_os_kind` — tutti con `web_search: false,`.

Nuovi test (27 totali; 2 aggiunti):

| Test | Key assertion |
|------|---------------|
| `command_web_search_defaults_false_when_absent` | JSON senza `web_search` → `false` (serde default) |
| `command_web_search_true_roundtrip` | round-trip con `web_search:true`; wire contiene `"web_search":true` |

## [0.2.0] — 2026-06-20

### Added — Superficie 3: custom windows (ADR-013)

- **`WindowKind` enum** (new): `Markdown` variant; `#[serde(rename_all = "snake_case")]`
  so `Markdown` → `"markdown"` on the wire.  Designed for extension (Table, Html, …).

- **`ServerMsg::OpenWindow { title, kind, content }`** (new additive variant):
  - Wire: `{"type":"open_window","title":"...","kind":"markdown","content":"..."}`.
  - Carries no `id`; self-contained (one message → one window).
  - `title` is populated by the orchestrator from the first non-empty content line
    (truncated to ≤60 chars) or defaults to `"Lare — Output"`.
  - `kind: WindowKind` determines how the UI renders `content`.

### TDD cycle (RED → GREEN)

**RED (genuine compile errors before adding types):**
```
error[E0599]: no variant named `OpenWindow` found for enum `ServerMsg`
error[E0433]: cannot find type `WindowKind` in this scope
```
Five new tests could not compile (stronger than assertion failure).

**GREEN** after adding `WindowKind` and `ServerMsg::OpenWindow`.

New tests (all 25 pass, 5 new):

| Test | Key assertion |
|------|---------------|
| `open_window_markdown_roundtrip` | round-trip serde preserves all fields |
| `open_window_wire_format_type_discriminator` | `"type":"open_window"` |
| `open_window_wire_format_kind_markdown` | `"kind":"markdown"` |
| `open_window_wire_format_full_shape` | all four fields present in JSON |
| `deserialize_handwritten_open_window_json` | hand-written JSON → correct variant |

## [0.1.0] — 2026-06-20

### Added
- `ClientMsg` enum: `Hello`, `Command`, `Ping` variants.
- `ServerMsg` enum: `ServerInfo`, `Chunk`, `Done`, `Error`, `Pong` variants.
- `InputMode` enum: `Keyboard`, `Voice`.
- `CommandKind` enum: `Auto`, `Os`, `Nl`.
- `ErrCode` enum: `OsError`, `AiError`, `RoutingError`.
- Full serde support with `tag = "type"` discriminator and `snake_case` wire names.
- 20 inline unit tests: round-trips for every variant + wire-format assertions +
  hand-written JSON deserialization (non-Rust client contract).
- Workspace `Cargo.toml` (virtual, `resolver = "2"`).
