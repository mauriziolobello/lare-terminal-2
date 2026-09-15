# Implementation — `protocol` v2.1.0

v0.15.3: `ChatLine`/`ServerMsg::AiChatMessage` guadagnano due campi additivi
`#[serde(default)]`: `display_name: Option<String>` (nickname risolto dal
mittente al momento dell'invio, `None` = fallback a `from_label` come
oggi) e `is_ai: bool` (`true` se il mittente è l'AI, pilota badge/icona nel
frontend, `false` di default). Un peer/storico vecchio che non manda questi
campi deserializza comunque — nessuna regressione, nessun badge esisteva
prima d'ora. Task 2 (indipendente dal Task 1) del piano
`Docs/superpowers/plans/2026-08-13-aichat-display-names.md`; il gemello sul
protocollo peer↔peer è `orchestrator::aichat::wire::{ChatLine, ChatMsg::Say}`
(stessa release, `orchestrator` 0.41.13). Nessun consumatore ancora in
questo crate — la risoluzione dei valori arriva col Task 3
(`orchestrator::aichat::service`), la resa visiva col Task 9 (frontend).

v0.15.2: nuovo struct `ScreenerListItem { id, title, description }` + `ServerMsg::OpenScreenerPicker
{ items: Vec<ScreenerListItem> }` — wire tag `"open_screener_picker"`. Prerequisito Contratto A per il
registry screener multipli di `/markets` (`Docs/superpowers/specs/2026-08-11-markets-screener-registry-
design.md`). `ScreenerListItem` è la vista "dumb" (id/titolo/descrizione) di uno screener registrato
lato Python — mai la logica di selezione, che resta interamente server-side. Additiva.

v0.15.1: aggiunta `ServerMsg::Heartbeat { id: String }` — wire tag `"heartbeat"`. Battito di
vita per un comando in-flight, scollegato dal contenuto; origina da un evento SSE `ping`
(keep-alive nativo di Anthropic) osservato da `messages_client.rs` (orchestrator), propagato
fino al watchdog frontend per distinguere "silenzio perché il turno lavora ancora" da "silenzio
perché è bloccato". Additiva. Design: `Docs/superpowers/specs/2026-08-07-ai-turn-heartbeat-watchdog-design.md`.

v0.15.0: aggiunta `ServerMsg::RoutineSavePreview { id, name, description, tags, category, script, replace }` —
wire tag `"routine_save_preview"`. Finestra di anteprima dedicata per il salvataggio di routine
(Fase 2 del repository di routine), con i campi STRUTTURATI della proposta. La risposta è la stessa
`ClientMsg::ToolConfirmResponse` (id opaco, accept), riusata as-is. Design: `Docs/superpowers/specs/
2026-08-05-save-routine-design.md` §5. Additiva.

v0.14.9: `NoteView` guadagna `my_segment_text: String` — il segmento di QUESTA
sola macchina (vuoto se non ha ancora contribuito), distinto da `body` (corpo
fuso di tutte le macchine). Serve a precompilare la finestra "Modifica" col
testo che l'utente sta davvero per sovrascrivere. Additiva.

v0.14.8: aggiunta struct `NoteView { id, title, body, created_by, created_at_ms, deleted }`
(forma "dumb" di una nota per la UI, corpo già renderizzato, senza segmenti grezzi)
+ quattro varianti `ClientMsg` (`NoteCreate`, `NoteEdit`, `NoteEditTitle`, `NoteDelete`)
+ due varianti `ServerMsg` (`NotesSnapshot`, `NoteUpserted`) — tutte additive. Wire tags:
`"note_create"`, `"note_edit"`, `"note_edit_title"`, `"note_delete"` (client→orchestrator),
`"notes_snapshot"`, `"note_upserted"` (orchestrator→client). Design: `Docs/superpowers/specs/
2026-07-29-library-notes-design.md` §7.

v0.14.7: aggiunta `ServerMsg::AiChatReachablePeers { labels: Vec<String> }` —
wire tag `"ai_chat_reachable_peers"`. Fix bug Library "Condividi" (mostrava
"Nessuna macchina connessa" nonostante discovery+elezione AI Chat riuscite):
il dialog leggeva `AiChatRoster` (richiede ammissione umana alla stanza),
ma il backend che esegue la condivisione (`request_share`) non l'ha mai
consultato — risolve da `peers ∩ links` (discovery UDP + link TCP, entrambi
automatici). Design: `Docs/superpowers/specs/
2026-07-28-library-share-reachable-peers-design.md`.

v0.14.3: aggiunte `ServerMsg::ToolConfirmRequest { id, commands }` e
`ClientMsg::ToolConfirmResponse { id, accept }` — round-trip di conferma per un
gate di conferma locale per-tool (estende ADR-007 oltre Telegram). Nessun
consumatore reale in questa versione (`SENSITIVE_TOOLS` vuoto lato orchestrator).

v0.12.0: aggiunti `ShareTarget` e `ShareOutcome` enum di supporto, più `ClientMsg::ShareDocument`
e `ClientMsg::ShareConsent`, più `ServerMsg::ShareRequest` e `ServerMsg::ShareResult` — round-trip
di consenso per Library "Share with" (Slice 1a). Nessun trasferimento di contenuto (Slice 2).
Wire format: `ShareTarget::{One{label_base}, All}`, `ShareOutcome::{Accepted, Rejected, Failed}`;
type discriminator snake_case via `rename_all`.

v0.9.4: aggiunta `ServerMsg::AiChatPeerLost { label: String }` — un peer è sparito dalla
stanza AI Chat (keepalive scaduto o disconnessione pulita), in entrambe le direzioni (client
o proprio server). Evento live, mai in `AiChatHistory`. Ripple: `orchestrator`'s
`telegram/channel.rs` (match esaustivo su `ServerMsg`) trattato come no-op permanente, come
le altre varianti AiChat.

v0.9.3: aggiunta `ServerMsg::AiChatSelf { label: String }` — la propria etichetta umana
nella stanza AI Chat, per il titolo della finestra-chat.

v0.9.2: aggiunta `ChatLine { from_label, text }` + `ServerMsg::AiChatHistory { entries: Vec<ChatLine> }`
— dump in blocco dello storico messaggi AI Chat (riapertura finestra / catch-up al Join).

v0.9.1: aggiunta `ClientMsg::AiChatOpen {}` (wire `"ai_chat_open"`) — segnala riapertura
finestra-chat; additiva rispetto alle 6 varianti AI Chat introdotte in v0.9.0.

v0.9.0 (precedente): vedi CHANGELOG per le 6 varianti AI Chat (Slice 1a-ui).

> Note: versioni 0.3.0–0.8.0 in CHANGELOG.

## Ruolo della connessione e superficie (2.1.0, piano 2a Task 1)

Piano 2a aggiunge un canale "shell" (una sessione `lare-shell`, host PowerShell in
`shell/lare-shell/`) accanto al canale "ui" (`ui.exe`) già esistente. Le due connessioni
condividono lo stesso WS e lo stesso `ClientMsg`/`ServerMsg`, ma differiscono per **ruolo**
e per **dove va la risposta**:

- **`Role { Ui, Shell }`** — dichiarato dal client nella `Hello` (`role: Role`, default
  `Ui` per compatibilità: un client v1 non manda il campo). Una connessione `Shell` porta
  con sé una `session_id` (lega la connessione alla finestra terminale) e una `cwd`
  iniziale (il `$PWD` del runspace all'avvio) — entrambe assenti per `Ui`.
- **`Surface { Origin, Ui }`**, esposto da **`ServerMsg::surface(&self) -> Surface`** —
  per una connessione `ui` non cambia nulla (tutto torna alla connessione stessa, come in
  v1). Per un turno originato da una connessione **shell**, `surface()` distingue:
  - `Origin`: torna alla connessione shell che ha mandato il `Command` — avanzamento del
    turno (`Chunk`/`Done`/`Error`/`Pong`/`Cwd`/`Heartbeat`), il gate di conferma
    (`ToolConfirmRequest`), l'esecuzione nel suo stesso runspace (`ExecInShell`), risposte
    puntuali (`MarketDataSourceTestResult`).
  - `Ui`: apre/aggiorna finestre o alimenta un relay (AI Chat, Library, Share, plugin,
    Notes) — va sempre al sink `ui` unico della macchina, mai alla shell che ha originato
    il turno. Include le tre novità di questo task pensate apposta per l'output dei
    comandi slash lanciati da una shell (`OpenOutputWindow`, `OutputWindowContent`,
    `OpenUiLocal`) più `UiPing`/`ActivityIndicator`.

  Il `match` dentro `surface()` è **esaustivo senza wildcard di proposito**: chi aggiunge
  una variante `ServerMsg` deve decidere esplicitamente `Origin` o `Ui`, e il compilatore
  glielo impone (niente `_ => ...` che nasconderebbe la scelta).

- **Esecuzione nella shell dell'utente**: `ServerMsg::ExecInShell { turn_id, exec_id,
  command, capture }` (verso `role: Shell`, sempre dopo un `ToolConfirmRequest` accettato)
  e la sua risposta `ClientMsg::ExecResult { turn_id, exec_id, exit_code, output, cwd }` —
  `capture: true` cattura l'output in `ExecResult.output`; `capture: false` lascia la
  console attaccata (programmi interattivi), `output` vuoto. `cwd` nella risposta aggiorna
  la cwd *per sessione* (non più una cwd unica di processo come in v1).

- **`/ping` per `ui.exe`**: `ServerMsg::UiPing { id }` / `ClientMsg::UiPong { id, version }`
  — simmetrico al `Ping`/`Pong` già esistente, ma verso `ui` invece che verso il client
  generico; `version` compare nella riga `lare-shell` di `/ping`. `Hello.version` è stato
  aggiunto qui (Task 1) prima che lo spec §4.1 lo documentasse — l'emendamento (Task 11 del
  piano 2a) ha allineato lo spec al codice, non il contrario.

Consumo reale a fine piano 2a (Task 2-8, vedi `orchestrator/IMPLEMENTATION.md` §"Canale
shell"): il registro delle connessioni (`connections.rs`) instrada per `Role`, `surface.rs`
usa `ServerMsg::surface()` per dividere `Origin`/`Ui`, `shell_session.rs` correla
`ExecInShell`/`ExecResult` per `exec_id`. `orchestrator::telegram::channel.rs` non cambia:
Telegram non ha un ruolo `Shell`, quindi le nuove varianti restano arm no-op lì, per design
(non un debito).

## Contratti per la host (piano 2b)

Il protocollo del canale shell (sopra) lascia alla host C# (`lare-shell`, ancora da scrivere,
piano 2b) quattro vincoli impliciti che il lato orchestratore già assume — trovati/consolidati
durante la review finale del piano 2a. Non sono campi o messaggi nuovi: sono comportamenti che
la host DEVE rispettare perché il codice lato orchestratore (`ws.rs`/`shell_turn.rs`/`surface.rs`/
`shell_session.rs`) è scritto assumendoli.

- **(a) Un solo turno AI alla volta per connessione.** La `history` conversazionale è dietro un
  `tokio::sync::Mutex` tenuto per l'INTERA durata del turno, gate `[Y/n]` incluso (fino a 180 s):
  se la host manda un secondo `Command` (`/ai`/backend) mentre il primo è ancora in corso sulla
  stessa connessione, la sua finestra di output si apre subito (`start_turn` lo fa prima di
  qualunque `.await` sulla history) ma il turno resta bloccato in attesa del lock — nessun errore,
  nessun messaggio di scusa, semplicemente in coda. La host non deve mandare un secondo `Command`
  prima di aver ricevuto `Done`/`Error` del precedente sulla stessa sessione.
- **(b) `Command.id` deve essere UNICO per connessione.** L'`id` fa da chiave sia per il cancel
  token (`ws.rs`, mappa `commands: HashMap<String, CancellationToken>`) sia per `window_id`/
  `turn_id` (`shell_turn::start_turn`, `ShellSessionToolClient::for_turn`). Un `id` duplicato
  mentre il primo turno con quell'`id` è ancora vivo sovrascrive silenziosamente la entry
  `commands` (un `CancelCommand` successivo annullerebbe solo il SECONDO turno) e riusa la stessa
  finestra di output (stesso `window_id`) invece di aprirne una nuova.
- **(c) Il turno finisce al PRIMO `Done`/`Error`.** `surface::route_shell_turn` consegna una sola
  terminazione per turno alla shell: un secondo `Done`/`Error` con lo stesso `id`/`turn_id` viene
  scartato (log `debug`, non un errore visibile). La host deve considerare il turno chiuso al
  primo dei due che arriva e ignorare ogni messaggio successivo per quell'`id` — l'orchestratore
  non ne manderà comunque un secondo per il tramite normale, ma un client difensivo non deve
  fare affidamento sul contrario.
- **(d) `ExecResult.turn_id` deve corrispondere al `turn_id` ricevuto nell'`ExecInShell` a cui si
  risponde** (fix M8, review finale). `ShellSessionState::resolve_exec` valida il `turn_id`
  memorizzato per quell'`exec_id`: un `turn_id` diverso viene scartato con un `tracing::warn!`
  e — punto importante — l'esecuzione pendente NON viene rimossa (una risposta sbagliata non deve
  poter consumare l'attesa di un turno diverso). La host deve sempre echeggiare il `turn_id`
  ricevuto nell'`ExecInShell` corrispondente, mai un `turn_id` proprio o ricostruito.

## Scope (SRP)

This crate owns **only the contract types** for the Lare Terminal WebSocket
protocol (Contratto A in `Docs/03-protocol.md`). It contains:

- message enums (`ClientMsg`, `ServerMsg`)
- supporting enums (`InputMode`, `CommandKind`, `ErrCode`, `WindowKind`, `SearchSource`)
- their serde serialisation/deserialisation implementations
- unit tests (inline `#[cfg(test)] mod tests`)

It contains **no networking, no runtime, no business logic**. All consumers
(UI, orchestrator, tests, integration helpers) depend on this crate as a
library — they never bypass the types.

## Types

### `ClientMsg` (client → orchestrator)

| Variant | Fields | Purpose |
|---------|--------|---------|
| `Hello` | `token: String`, `channel: Option<String>`, `role: Role`, `session_id: Option<String>`, `cwd: Option<String>`, `version: Option<String>` | Handshake; must be first message per session. `channel: Some(id)` requests a scoped external-tool-channel connection (default `None` = today's behaviour). Gli ultimi quattro campi sono additivi `#[serde(default)]` (v2.1.0): `role` dichiara `ui`/`shell` (default `ui`), `session_id`/`cwd` solo per `shell`, `version` è la versione del client (usata da `/ping`) |
| `ExecResult` | `turn_id: String`, `exec_id: String`, `exit_code: i32`, `output: String`, `cwd: String` | Esito di un `ExecInShell` eseguito dalla shell dell'utente; `cwd` aggiorna la cwd della sessione (v2.1.0) |
| `UiPong` | `id: String`, `version: String` | Risposta di `ui.exe` a `ServerMsg::UiPing` (built-in `/ping`) (v2.1.0) |
| `Command` | `id`, `input`, `input_mode`, `command_type`, `cwd`, `web_search` | Execute a command |
| `Ping` | `ts: u64` | Keep-alive; `ts` is a client-supplied timestamp |
| `CancelSearch` | `id: String` | Cancel an in-progress file search |
| `PauseSearch` | `id: String` | Pause a running search (walkers suspend at next checkpoint) |
| `ResumeSearch` | `id: String` | Resume a previously paused search |
| `CancelCommand` | `id: String` | Cancel an in-progress AI/OS command |
| `PluginUiEvent` | `window_id: u64`, `element_id: String`, `value: Option<String>` | UI interaction event from a plugin window (v0.8.0) |
| `PluginWindowClosed` | `window_id: u64` | Notifies orchestrator that user closed a plugin window (v0.8.0) |
| `AiChatSend` | `text: String` | Umano invia un messaggio nella stanza AI Chat (v0.9.0) |
| `AiChatJoinConsent` | `peer_label: String`, `accept: bool` | Risposta al consenso d'ingresso peer (v0.9.0) |
| `AiChatClosed` | _(none)_ | La finestra-chat è stata chiusa (v0.9.0) |
| `AiChatOpen` | _(none)_ | La finestra-chat è stata (ri)aperta; ripristina `server_tx` e roster (v0.9.1) |
| `ShareDocument` | `rel_path: String`, `doc_name: String`, `size_bytes: u64`, `target: ShareTarget` | UI trigger: condividi un documento Library; metadati risolti lato UI (v0.12.0) |
| `ShareConsent` | `share_id: String`, `accept: bool` | Risposta umana al banner di consenso mostrato da `ServerMsg::ShareRequest` (v0.12.0) |

### `ServerMsg` (orchestrator → client)

| Variant | Fields | Purpose |
|---------|--------|---------|
| `ServerInfo` | `version`, `ai_provider`, `capabilities` | Sent after successful handshake |
| `Chunk` | `id`, `content` | One streaming output chunk for command `id` |
| `Done` | `id`, `exit_code: Option<i32>` | Terminal signal; `exit_code` is `null` on AI path |
| `Error` | `id`, `code`, `message` | Error for command `id` |
| `Pong` | `ts: u64` | Keep-alive reply |
| `OpenWindow` | `title`, `kind: WindowKind`, `content` | Open a new UI window (Markdown/Help) |
| `SearchOpen` | `id`, `title` | Opens a live search window |
| `SearchHit` | `id`, `path`, `source: SearchSource`, `line: Option<u32>`, `snippet: Option<String>` | One file result from a running search; `line`/`snippet` present only for a content-search hit (v0.14.1) |
| `SearchDone` | `id`, `count`, `truncated` | Signals end of search |
| `Cwd` | `path: String` | Current working directory of the persistent shell session |
| `Heartbeat` | `id: String` | Battito di vita per il comando `id` — nessun contenuto, solo "il turno è ancora attivo"; origina da un evento SSE `ping` (keep-alive nativo di Anthropic) propagato da `messages_client.rs`/`ai_adapter.rs` (orchestrator); il frontend lo usa per resettare il timer di silenzio del watchdog (v0.15.1) |
| `OpenPluginWindow` | `window_id: u64`, `title: String`, `html: String`, `width: Option<f64>`, `height: Option<f64>` | Open a new plugin window (v0.8.0); `width`/`height` = dimensione iniziale dichiarata dal plugin, `skip_serializing_if=None` ⇒ omessi dal wire quando assenti, l'UI ricade sul default 480×360 (v0.14.2) |
| `UpdatePluginWindow` | `window_id: u64`, `html: String` | Full-HTML update of an existing plugin window (v0.8.0) |
| `ClosePluginWindow` | `window_id: u64` | Close a plugin window (v0.8.0) |
| `AiChatMessage` | `from_label: String`, `text: String`, `display_name: Option<String>`, `is_ai: bool` | Messaggio nella stanza AI Chat (v0.9.0); ultimi due campi additivi `#[serde(default)]` (v0.15.3) |
| `AiChatRoster` | `participants: Vec<String>` | Lista autorevole dei partecipanti presenti (v0.9.0) |
| `AiChatJoinRequest` | `peer_label: String` | Nuovo peer in LAN: l'UI chiede consenso (v0.9.0) |
| `AiChatHistory` | `entries: Vec<ChatLine>` | Storico messaggi in blocco (replay/riapertura, catch-up al Join) (v0.9.2); `ChatLine` guadagna `display_name: Option<String>`/`is_ai: bool` additivi (v0.15.3) |
| `AiChatSelf` | `label: String` | La propria etichetta umana (per il titolo finestra) (v0.9.3) |
| `AiChatPeerLost` | `label: String` | Un peer è sparito dalla stanza (keepalive/disconnessione); evento live, mai in history (v0.9.4) |
| `ShareRequest` | `share_id: String`, `from_label: String`, `doc_name: String`, `size_bytes: u64` | Richiesta di consenso a destinatario: vuoi ricevere questo doc? (v0.12.0) |
| `ShareResult` | `share_id: String`, `target_label: String`, `outcome: ShareOutcome` | Esito per mittente (una per ogni destinatario); rilevante da Slice 3+ (v0.12.0) |
| `ToolConfirmRequest` | `id: String`, `commands: String` | Gate di conferma locale per tool sensibili — comando/e in stringa libera, banner Sì/No (v0.14.3) |
| `RoutineSavePreview` | `id: String`, `name: String`, `description: String`, `tags: Vec<String>`, `category: String`, `script: String`, `replace: Option<String>` | Finestra di anteprima dedicata per `save_routine`, con campi STRUTTURATI; risposta è `ClientMsg::ToolConfirmResponse` riusata (v0.15.0) |
| `ExecInShell` | `turn_id: String`, `exec_id: String`, `command: String`, `capture: bool` | Chiede alla shell dell'utente di eseguire `command`; `capture` distingue output catturato da console attaccata (v2.1.0) |
| `OpenOutputWindow` | `window_id: String`, `title: String` | Apre su `ui` la finestra Markdown di output di un comando slash originato dalla shell, col segnaposto (v2.1.0) |
| `OutputWindowContent` | `window_id: String`, `markdown: String` | Sostituisce il contenuto di quella finestra, a `Done`/`Error` (v2.1.0) |
| `OpenUiLocal` | `name: String` | Chiede a `ui` di aprire (o portare in primo piano) una finestra locale (`"config"`, `"library"`, `"aichat"`, o l'id di un canale esterno) (v2.1.0) |
| `UiPing` | `id: String` | Richiesta di vita a `ui.exe` (built-in `/ping`); risposta `UiPong` (v2.1.0) |
| `ActivityIndicator` | `session_id: String`, `kind: String`, `on: bool` | Segnalino di stato per la finestra terminale della sessione; emesso da questa versione, consumato dal piano 3 (v2.1.0) |
| `MarkdownWindowTurnEnded` | `window_id: String` | Segnala che il turno proprietario di una finestra `show_markdown` è concluso (successo/errore/cancellazione); la finestra lo usa per il gate di chiusura e il badge di stato (v2.2.0) |

### `Role` e `Surface` (2.1.0)

| Enum | Variants | Wire values |
|------|----------|-------------|
| `Role` | `Ui`, `Shell` | `"ui"`, `"shell"` — `Default = Ui` |

`Surface { Origin, Ui }` non è serializzato (nessuna presenza sul wire): è il tipo di ritorno di
`ServerMsg::surface(&self) -> Surface`, che decide per ogni variante se il messaggio torna alla
connessione che ha originato il turno (`Origin`) o va al sink `ui` della macchina (`Ui`). Vedi la
sezione "Ruolo della connessione e superficie" sopra.

### Supporting enums

| Enum | Variants | Wire values |
|------|----------|-------------|
| `InputMode` | `Keyboard`, `Voice` | `"keyboard"`, `"voice"` |
| `CommandKind` | `Auto`, `Os`, `Nl` | `"auto"`, `"os"`, `"nl"` |
| `ErrCode` | `OsError`, `AiError`, `RoutingError` | `"os_error"`, `"ai_error"`, `"routing_error"` |
| `WindowKind` | `Markdown`, `Help`, `Search` | `"markdown"`, `"help"`, `"search"` |
| `SearchSource` | `Cwd`, `Standard`, `Cloud`, `External` | `"cwd"`, `"standard"`, `"cloud"`, `"external"` |
| `ShareTarget` | `One{label_base}`, `All` | `"one"`, `"all"` (v0.12.0) |
| `ShareOutcome` | `Accepted`, `Rejected`, `Failed{reason}` | `"accepted"`, `"rejected"`, `"failed"` (v0.12.0) |

## Wire format and why

Both message enums use:

```rust
#[serde(tag = "type", rename_all = "snake_case")]
```

This produces an **internally tagged JSON** object where every message carries
a `"type"` discriminator as a regular field — the format most WebSocket clients
(wscat, browser JS, Python scripts) produce and expect naturally.

Example (`Command` with `cwd: None`):

```json
{
  "type": "command",
  "id": "abc-123",
  "input": "dir",
  "input_mode": "keyboard",
  "command_type": "auto",
  "cwd": null
}
```

Key choices:
- `cwd: null` is **always present** (no `skip_serializing_if`). The contract
  test asserts this; omitting `cwd` on absent values would silently break
  non-Rust clients that distinguish `null` from missing field.
- `rename_all = "snake_case"` on supporting enums maps `Keyboard` → `"keyboard"`,
  `OsError` → `"os_error"`, etc.

## SOLID notes

- **SRP**: crate has one reason to change — the wire contract changes.
- **OCP/DIP**: consumers depend on types, not on impl; swap serde backend without
  touching consumers.
- **ISP**: no fat interfaces; each type carries only its own fields.
- Composition model: no inheritance, no macros beyond `#[derive]`.

## How to test

```sh
# Full test suite (runs on every `cargo test` in the workspace too)
cargo test -p protocol

# Linting
cargo clippy -p protocol -- -D warnings

# Formatting check
cargo fmt -p protocol -- --check
```

The 20 tests cover:
1. Round-trip for every `ClientMsg` variant (4 tests).
2. Round-trip for every `ServerMsg` variant (6 tests, including `Done` with
   and without `exit_code`).
3. Wire-format assertions for key variants (`command`, `hello`, `chunk`, `done`,
   `server_info`) — these fail at runtime if serde attributes are wrong.
4. Deserialization of hand-written JSON strings (5 tests) — simulates wscat or
   any non-Rust client; locks the contract for clients outside this codebase.

## Dependencies

| Crate | Role | Scope |
|-------|------|-------|
| `serde` + feature `derive` | Serialisation | Runtime |
| `serde_json` | JSON encoding/decoding in tests | Dev only |

## `ServerMsg::SaveToLibrary` — rimossa (0.14.6)

Introdotta in 0.14.5 per il salvataggio automatico dei report di canale
(nmap), rimossa un ciclo di sviluppo dopo: il primo uso dal vivo ha
prodotto un doppione in Library, perché il pulsante "Salva" preesistente
in ogni finestra Markdown salvava di nuovo lo stesso contenuto. Il
salvataggio resta scelta dell'utente tramite quel pulsante — vedi
`orchestrator` 0.40.26 e `ui` 0.45.10.
