# Changelog — orchestrator

All notable changes to this crate are documented here.
Format: [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), versioning: [SemVer](https://semver.org/).

---

## 2.1.0 — 2026-09-06 — canale shell: registro, gate, turni con finestra di output, `/ping` (piano 2a, Task 2-8, 10)

Consuma i tipi additivi di `protocol` 2.1.0 (Task 1): una sessione `lare-shell` (spec
`Docs/i18n/ita/superpowers/specs/2026-09-04-lare-terminal-2-design.md` §3/§4, ADR-018) può ora
mandare `Command`/`/…` all'orchestratore esattamente come una connessione `ui`, con output
instradato alla finestra Markdown invece che nel terminale. Nessuna connessione `ui`/Telegram
esistente cambia comportamento — vedi `IMPLEMENTATION.md` §"Canale shell" per i dettagli modulo
per modulo. Riassunto per task:

- **Task 2 — `connections.rs`** (registro delle connessioni vive): sink `ui` (l'ultima
  connessione `role: Ui` senza canale vince, come il sink dei plugin), sessioni shell per
  `session_id`, ping `ui` pendenti (`UiPing{id}` → `oneshot` risolto da `UiPong`). Puro stato,
  nessun I/O — `Registry::shared()` per `main()`/test.
- **Task 3 — `local_confirm::ShellConfirmer`** + `agent::display_invocation`: gate di conferma
  per la shell, per composizione sopra `LocalUiConfirmer` (stessa meccanica `ToolConfirmRequest`
  + `PendingConfirms`, politica diversa: `should_gate` di default del trait, quindi TUTTO tranne
  `show_markdown`, `run_in_session`/`open_target` inclusi — a differenza della UI locale, che li
  lascia autonomi, spec §4.3/§8). `display_invocation("run_in_session", …)` aggiunge la nota
  `(interattivo)` al prompt `[Y/n]` quando l'input ha `interactive: true` (spec §4.5): l'utente
  deve sapere PRIMA di accettare che l'output non verrà catturato.
- **Task 4 — `shell_session.rs`**: la shell dell'utente come `ToolClient`. `ShellSessionState`
  (per connessione: cwd di sessione, mappa `exec_id → oneshot` delle esecuzioni pendenti, canale
  d'uscita, `McpToolClient` per i tool non-shell) e `ShellSessionToolClient` (per turno: sa il
  `turn_id` da scrivere in ogni `ExecInShell`). `run_in_session` diventa un round-trip
  `ExecInShell{exec_id}` → `ExecResult{exec_id}` sulla stessa connessione WS, mai più una shell
  posseduta dal processo. La cwd è per sessione (D17), non tocca `cwd_state` globale v1.
- **Task 5 — `surface.rs`** (router di superficie) + fix `5f863d7`: `route_shell_turn` consuma i
  `ServerMsg` di UN turno da un canale interno e li smista — `Chunk` bufferizzati in memoria,
  consegnati una volta sola a `ui` come `OutputWindowContent` a `Done`/`Error`; tutto il resto
  segue `ServerMsg::surface()` (`Origin` → shell, `Ui` → sink). **Una sola terminazione per
  turno** verso la shell: un secondo `Done`/`Error` per lo stesso `turn_id` viene scartato (log
  `debug`) — contratto che la host (piano 2b) potrà assumere. Senza `ui.exe` connesso il turno
  finisce comunque, con `NO_UI_ACK` (`"→ ui.exe non connesso: output non mostrato"`) al posto
  della riga di conferma.
- **Task 6 — `shell_slash.rs`** (pre-router) + `/help` 2.0: `classify_shell_input` classifica
  OGNI riga `/…` arrivata dalla shell (nessun I/O) in `Nl`/`SyntaxError`/`OpenUiLocal`/`Reset`/
  `Ping`/`Backend`/`Discard` — `/ai "testo"`/`/ "testo"` richiedono le virgolette (D8, altrimenti
  `AI_SYNTAX_ERROR`); `/config`/`/library`/`/aichat` e i canali esterni user-facing (`/nmap`,
  `/markets`, `/pyping`) diventano `OpenUiLocal{name}`; il resto ricade su
  `core::KNOWN_BACKEND_SLASHES` (iniettato dal chiamante, non duplicato) o viene scartato.
  `core.rs` guadagna `KNOWN_BACKEND_SLASHES`/`WINDOW_SLASHES` (quest'ultima: `/help`/`/show`, il
  cui esito È già una finestra — niente finestra di output col segnaposto, spec §3.2 eccezione)
  e un `HELP_MARKDOWN` riscritto per la 2.0 (niente più tasti/hotkey F2, comandi aggiornati).
  Fix `6bd58e8`: testo di `/ping` in `/help`, header dei commenti spostati, doc più esplicita su
  `KNOWN_BACKEND_SLASHES` (il test prova solo lista→dispatch, non il contrario — allineamento è
  responsabilità di revisione).
- **Task 7 — `ping.rs`** (built-in `/ping`, spec §3.1): una riga per strato in una tabella
  Markdown — `lare-shell` (dalla `Hello.version` + `session_id`), `orchestrator` (uptime da
  `PROCESS_START`), `plugin-ping` (sonda usa-e-getta Init→Ready, `plugins::host::probe`, mai
  l'istanza eager già viva), `ui.exe` (`UiPing`/`UiPong`, `UI_PING_TIMEOUT` 2 s). Ogni strato
  assente degrada a una riga `--`/`non trovato`/`non connesso`; il comando finisce comunque con
  `Done`. `PluginHost::find(plugin_id)` (nuovo) risolve manifest + storage dir per la sonda senza
  toccare l'istanza eager.
- **Task 8 — `ws.rs`** (cablaggio): `HelloInfo` sostituisce la tupla `(token, channel)` di v1 con
  tutti i campi della `Hello` 2.1.0. Una connessione `role: Shell` registra una
  `ShellSessionState`, non riceve mai il sink `ui`/AI Chat (`SetServerTx`), e ogni `Command` va a
  `shell_turn::run_shell_command` (spawnato, non blocca il loop). `ClientMsg::ExecResult` risolve
  l'`exec_id` pendente sulla sessione; `ClientMsg::UiPong` risolve il ping pendente nel registro.
  Alla disconnessione: `abort_all()` sblocca ogni `run_in_session` in attesa, la sessione esce dal
  registro, il sink `ui` si azzera SOLO se è ancora il nostro (`clear_ui_sink_if`, guardia contro
  una `ui` più recente che l'ha già sostituito). Nuovo modulo `shell_turn.rs`: intermediario fra
  `ws.rs` e {`shell_slash`, `surface`, `core::handle_command`} — classifica, apre il turno (che
  apre subito la finestra di output), poi esegue `/ai`/backend/`/ping` con `ShellConfirmer` +
  `ShellSessionToolClient`; `web_search` effettivo = richiesta del turno **oppure**
  `config.json.web_search_enabled` (la shell non ha una propria casella).
- **Task 10 — `tests/ws_integration.rs`** (5 scenari e2e) + `scripts/dev/shell-client.mjs`: slash
  ignoto → `Done` muto senza `Cwd` iniziale; `/ai` senza virgolette → `Error{routing_error}`;
  `/reset`/`OpenUiLocal` raggiungono il sink `ui`; un turno `/ai` completo fa il giro intero
  (finestra aperta su `ui` col titolo dal testo → gate `[Y/n]` → `ExecInShell`/`ExecResult` →
  contenuto sulla finestra UNA volta → riga di conferma + `Done` sulla shell, senza `Chunk` di
  testo AI); `/ping` riporta `ui` raggiunto e il plugin mancante. Il client di sviluppo imita
  `lare-shell` (Node ≥ 22, `WebSocket` globale, nessuna dipendenza): `Hello{role:"shell"}`, un
  `Command`, poi risponde a `tool_confirm_request` (stdin `[Y/n]`) ed `exec_in_shell` (pwsh,
  `capture` sceglie stdio ereditata o catturata) come farebbe la host vera. Fix `c3325a4`: niente
  marcatore di cwd sulla console quando `capture:false` (bug di review — il marcatore usato per
  `capture:true` finiva stampato anche su un comando interattivo con stdio ereditata); `--selftest`
  verifica `execInShell` in locale, senza orchestratore.

## 2.0.2 — 2026-09-05 — fix wave della review finale (piano 1)

Solo pulizia dopo la review whole-branch, nessun cambio di comportamento visibile:

- Plugin spawnati con `--config-dir <path>` (prima solo orchestrator/mcp-server/ui
  lo ricevevano) — spec 2.0 §6.1, nessuna eccezione. `spawn_plugin` cambia firma.
- Assolutizzazione di `--config-dir` spostata in `startup-config` (beneficia anche
  `ui`/`mcp-server`); il blocco locale ridondante in `main.rs` è stato rimosso.
- Rimossi: `ws::LISTEN_ADDR` (costante morta) e i commenti che citavano ancora la
  porta fissa 7331; un residuo `set_var("LARE_PYTOOLS_DIR", …)` in
  `ws_integration.rs` (test già verde senza, D6 lo rende inerte da tempo).
- `token_store::resolve_token` usa `startup_config::TOKEN_FILE_NAME`.
- `external_channel::tests::test_rt()`: fix di una tempdir leak (`.keep()` la
  rendeva permanente su disco a ogni chiamata, 15 volte per test run) — ora
  ritorna `(RuntimeConfig, TempDir)`, ripulita al drop.

## 2.0.1 — 2026-09-05 — `--config-dir`, `RuntimeConfig`, figli con argomento, log su file, nessuna env var

Il crate `startup-config` è stato riscritto (Task 2 del piano "fondamenta") con l'API 2.0:
niente più `load_from_dir`/`resolve`/`default_local_dir`/`Paths::local_dir`. L'orchestrator non
compilava più contro la nuova API (13 letture di `env::var("LARE_*")` + 3 `LOCALAPPDATA` sparse
nel crate). Migrazione completa a `--config-dir` (decisione D6 dello spec 2.0):

- **`RuntimeConfig`** (nuovo modulo `runtime_config.rs`): "context object" immutabile
  (`config_dir` + `StartupConfig`), risolto UNA volta in `main()` — `config_dir` è SEMPRE
  assoluto (risolto prima del `set_current_dir(home)` che segue, altrimenti un `--config-dir`
  relativo risolverebbe diversamente a seconda di quando viene riletto). Metodi derivati
  (`plugins_dir()`, `mcp_server_exe()`, `mcp_nmap_exe()`, `pytools_dir()`, `log_dir()`,
  `network_json_path()`) incapsulano `StartupConfig::resolve_path` invece di ripetere la stessa
  catena di join in ogni punto di chiamata.
- **Token WS** (`token_store.rs`): riscritto senza `LARE_TOKEN` — vive SOLO in `<config_dir>/token`.
- **`llms_config::resolve_path`**/**`telegram::settings::resolve_path`**: da 3 parametri
  (env/file/default) a un solo `config_dir: &Path` — sempre `<config_dir>/llms.json` e
  `<config_dir>/telegramsettings.json`, nessuna precedenza env/`startup.json` residua.
  `llms_config::build_adapter` guadagna un parametro `config_dir` (passato a `LlmAdapter`, che
  ora lo tiene come campo per `memory_file_path`/`needs_ai_name_prompt_at`).
- **Figli con `--config-dir`**: `McpToolClient::resolve(config_dir, cfg) -> Self` (non più
  fallibile: legge solo `startup.json.paths.mcp_server`, mai `LARE_MCP_SERVER`/`current_exe()`),
  `NmapToolClient::resolve(config_dir, cfg)`, `PythonMcpToolClient::resolve(config_dir, cfg,
  domain_id, script_relpath, tool_specs, call_timeout_secs)` — ciascuno passa `--config-dir
  <dir>` allo spawn del figlio (6 punti di spawn duplicati in `tool_client.rs`, uno per
  `nmap_tool_client.rs`/`python_mcp_tool_client.rs`). Rimosso il caso speciale
  `LARE_PLUGINS_DIR` (v1: nessun trim/empty-check, un'incoerenza deliberata mai più necessaria
  con un solo risolutore, `rt.plugins_dir()`).
- **`EXTERNAL_TOOL_CHANNELS`**: le factory dei canali (nmap/python-ping/financial-markets)
  cambiano firma da `fn() -> anyhow::Result<Arc<dyn ToolClient>>` a `fn(&RuntimeConfig,
  &Arc<dyn ToolClient>) -> anyhow::Result<Arc<dyn ToolClient>>` — `resolve_channel_tools` (e
  `ws::serve`/`handle_connection`, che ora ricevono `rt: Arc<RuntimeConfig>`) inoltrano il
  contesto invece di lasciare che ogni canale lo ri-derivi da solo.
- **`memory_file_path`**: UNA sola copia (`ai_adapter.rs`, ora `pub fn memory_file_path(config_dir:
  &Path, label_base: &str)`), riusata da `aichat/service.rs::append_memory_note` — eliminata la
  duplicazione deliberata della v1 (due resolver indipendenti che avrebbero potuto divergere).
  `agent::dispatch_tool` (wrapper a 3 parametri che ri-derivava `network.json` da solo) è stato
  eliminato: `agent::dispatch_tool_at` riceve sempre il path esplicitamente dal chiamante
  (`McpToolClient`/`CwdTrackingToolClient`, che tengono `config_dir` come campo).
- **Log su file**: sempre `<log.dir>/orchestrator.log` (default `Configuration/logs`, rotazione
  giornaliera via `tracing-appender`), più stderr solo con `--console-log` — un orchestrator in
  autostart non sporca (né si blocca su) un terminale che non ha.
- **Nessuna eccezione**: `ANTHROPIC_API_KEY`/`OPENROUTER_API_KEY` restano variabili d'ambiente
  (chiavi dei provider AI, fuori scope D6, come gli `api_keys` di `llms.json`).

Gate di verifica: `grep -rn 'env::var("LARE_' src` e `grep -rn "local_dir" src` vuoti (solo
commenti storici per il secondo, dove citano la v1). `LOCALAPPDATA`/`APPDATA` restano SOLO in
`search/paths_config.rs::detect_cloud_impl` (rilevamento della cartella di sync Dropbox
dell'utente — funzionalità di ricerca file, indipendente dalla configurazione 2.0, fuori scope D6).

### Fix round 1 (review Task 4) — log su file panic-free

L'inizializzazione del log su file (bullet sopra) usava `tracing_appender::rolling::daily(...)`,
che internamente fa `.expect(...)` e va in **panic** se la cartella di log non è creabile/scrivibile
(permessi negati, percorso occupato da un file, ...) — l'unico punto rimasto in questo task che
poteva ancora far crashare l'orchestrator all'avvio invece di ripiegare in modo morbido, un
problema serio per un demone pensato per girare inosservato in autostart. Estratta in un nuovo
modulo **`logging.rs`**: `open_log_file(log_dir) -> Result<(NonBlocking, WorkerGuard), String>`
(pura, mai panic, usa `RollingFileAppender::builder()...build()` che ritorna `Result` invece di
`rolling::daily()` che fa `.expect()`) + `init_logging(log_dir, level, console) -> LoggingGuard`,
che su errore stampa il motivo con `eprintln!` e ripiega su un subscriber di sola console
(SEMPRE attivo in quel caso, non solo con `--console-log` — un demone senza alcun log è peggio
di uno rumoroso). Due nuovi test coprono il fix, incluso quello di regressione (cartella di log
il cui genitore è un file regolare → `Err`, non panic).

## 2.0.0 — 2026-09-05 — fork da v1 0.41.21

Copia del crate dalla v1 (`mauriziolobello/lare-terminal`) nel repo 2.0. Nessuna modifica
funzionale in questa voce; le modifiche del piano 1 seguono nelle voci successive.

- Fixture dei test OpenRouter spostate in `tests/fixtures/` (nella v1 vivevano in `Docs/superpowers/fixtures/`, fuori dal crate).

## [0.41.21] — 2026-08-26 — `/markets`: quarto screener `citadel`

Nuovo screener registrato nel canale `financial-markets`: `citadel`
("Citadel" — analisi tecnica quant-style, primo screener NON fondamentale
del registry: doppio regime momentum/mean-reversion deciso dal trend
recente di ciascun titolo, percentili calcolati separatamente per regime
poi uniti in un pool ordinato, tetto — non quota — di 2 titoli per
settore/industry con backfill, nessuna esclusione). Nuovo
modulo `technical_cache.py` (cache storica OHLC, freschezza 1 giorno di
borsa, separato da `fundamentals_cache.json` — entry shape incompatibile).
Tutta la logica nuova vive in Python
(`scripts/pytools/financial-markets/screeners/citadel.py` +
`technical_cache.py`, moduli indipendenti dagli altri screener per
design): l'unico tocco in questo crate è una riga nell'elenco statico
id/titolo della `description` di `run_screener` (`external_channel.rs`).
Citadel legge `fundamentals_cache.json` (lo stesso `cache_path` che ogni
screener riceve) in SOLA LETTURA per il campo `industry` — non lo scrive
mai, la propria cache OHLC vive in un file sibling derivato.

---

## [0.41.20] — 2026-08-17 — `/markets`: terzo screener `jensen-huang`

Nuovo screener registrato nel canale `financial-markets`: `jensen-huang`
("Jensen Huang" — reverse-engineering dei pick del CEO Nvidia: fornitori
infrastruttura AI selezionati per industry, score a percentili su crescita
ricavi / PSG / pullback 52wk / target upside, bonus +10 per i ticker della
lista curata NVIDIA_LINKED, nessuna esclusione — top assoluto ripetibile,
nessun enrichment post-selezione). Tutta la logica nuova vive in Python
(`scripts/pytools/financial-markets/screeners/jensen_huang.py`, modulo
indipendente dagli altri screener per design): l'unico tocco in questo
crate è una riga nell'elenco statico id/titolo della `description` di
`run_screener` (`external_channel.rs`).

`ScreeningSnapshot` (shared, `scripts/pytools/financial-markets/data_sources/`)
esteso con 1 campo nullable (`industry`) — additivo, nessun impatto sugli
screener esistenti; le entry di cache pre-estensione non qualificano finché
il refresh graduale (7gg) non le rinnova.

---

## [0.41.19] — 2026-08-14 — Fix review finale whole-branch: canale WS "config-market-data-test" non registrato

Piano `2026-08-14-financial-markets-ibkr-data-source`, fix wave post-review
(review-fix-wave-brief.md, Fix A — CRITICO). `/code-review high` sull'intero
branch (16 commit) aveva trovato che il bottone "Test connessione" del tab
`/config` Dati Mercato (Task 12) non aveva MAI funzionato: `config-dialog.js`
apre una `LareWsClient` con `channel: "config-market-data-test"`, ma
`ws.rs::resolve_channel_tools` rifiutava QUALUNQUE `channel` non presente nel
registro `EXTERNAL_TOOL_CHANNELS` con `ServerMsg::Error` + chiusura del
socket — l'handshake falliva PRIMA che l'arm `ClientMsg::TestMarketDataSource`
(già corretto, Task 11) potesse mai essere raggiunto.

### Fixed

- **`external_channel.rs` — nuova voce `"config-market-data-test"` in
  `EXTERNAL_TOOL_CHANNELS`** (5ª voce del registro, dopo `"financial-markets"`):
  mirror ESATTO di `"library-expand"` — nessun tool AI custom
  (`EmptyToolClient`, riusato, non duplicato), nessun innesco da slash
  cursore (`slash_trigger` valorizzato ma inerte, come da convenzione del
  campo), nessun `system_prompt_override` (`TestMarketDataSource` è un
  `ClientMsg` diretto gestito in `ws.rs` indipendentemente dal canale, mai
  un tool AI). Con questa voce l'handshake supera `resolve_channel_tools`
  con `Ok(...)` e l'arm già esistente in `ws.rs` viene finalmente raggiunto.

### Tests

- `production_registry_has_the_config_market_data_test_channel` — registro
  passa da 4 a 5 voci, indice `[4]` è `"config-market-data-test"`.
- `config_market_data_test_channel_resolves_ok_not_err` — mirror positivo di
  `unregistered_channel_id_still_errs_against_the_real_registry`: prima del
  fix `resolve_channel_tools(Some("config-market-data-test"), ...)`
  ritornava `Err`, ora `Ok`.
- `config_market_data_test_tool_client_has_no_custom_tools` — mirror di
  `library_expand_tool_client_has_no_custom_tools`.
- I tre assert preesistenti `EXTERNAL_TOOL_CHANNELS.len() == 4` (voci
  `library-expand`/`python-ping`/`financial-markets`) aggiornati a `5` —
  nessuna regressione, solo l'indice di lunghezza cambiato dall'aggiunta.

`cargo test -p orchestrator`: verde (vedi `review-fix-wave-report.md` per i
numeri completi). Nessun impatto su `protocol`/`mcp-server`.

---

## [0.41.18] — 2026-08-14 — Task 11: handler `TestMarketDataSource` + check "fonte dati mercato" all'avvio

Piano `2026-08-14-financial-markets-ibkr-data-source`, Task 11/13. Consuma
`ClientMsg::TestMarketDataSource`/`ServerMsg::MarketDataSourceTestResult`
(Task 10, `protocol`) e il tool MCP `test_market_data_source` (Task 9,
`scripts/pytools/financial-markets/server.py`).

### Added

- **`ws::test_market_data_source_now() -> (bool, String)`** (nuova funzione
  `pub`, `ws.rs`) — costruisce un `PythonMcpToolClient` SCOPED (vive solo per
  la durata della chiamata, mai condiviso), dispaccia
  `test_market_data_source`, e parsa difensivamente la risposta JSON
  `{"ok": bool, "message": str}` del tool Python. Mai un panic: venv assente,
  timeout di rete (30s), o JSON malformato diventano tutti `(false,
  <messaggio leggibile>)`. Un solo punto condiviso dai due chiamanti sotto,
  per non duplicare la stessa costruzione client + parsing in due file.
  Nota architetturale: questo client è indipendente dal client MCP
  "normale" del canale `financial-markets` (quello usato dall'AI in
  `/markets`, per-connessione dentro `external_channel.rs`) — nessuno stato
  condiviso nuovo nel resto dell'orchestrator.
- **`ws.rs` — arm `ClientMsg::TestMarketDataSource`**: spawna un task
  (non blocca il read-loop della connessione, il test di rete può
  richiedere fino a 30s) che chiama `test_market_data_source_now()` e
  risponde con `ServerMsg::MarketDataSourceTestResult { id, ok, message }`.
  Azione manuale, innescata da `/config` (non fa parte del loop tool-use AI).
- **`main.rs` — check informativo "fonte dati mercato" all'avvio**: spawnato
  (non blocca `ws::serve`/l'apertura del listener) subito dopo il blocco AI
  provider — logga `INFO "Fonte dati mercato: connessa"` o `WARN "Fonte dati
  mercato: non raggiungibile (<messaggio>)"`. Stessa filosofia "non
  bloccante, solo informativo" del log del provider AI: TWS/Gateway spento
  non deve mai impedire l'avvio dell'orchestrator.
- **`telegram/channel.rs` — arm no-op `ServerMsg::MarketDataSourceTestResult`**
  in `format_response` (match esaustivo su `ServerMsg`): necessario per
  compilare dopo l'aggiunta della variante in Task 10 (gap non coperto da
  quel task, che toccava solo `protocol`) — superficie UI-only (`/config`),
  Telegram non ha un equivalente, no-op permanente come `ToolConfirmRequest`.

### Tests

- **`tests/ws_integration.rs` —
  `test_market_data_source_echoes_id_and_reports_unreachable_when_venv_missing`**:
  copre la wiring dell'arm (round-trip dell'`id`, `ServerMsg` corretto, read-loop
  reattivo dopo — Ping/Pong ricevuto subito dopo) senza I/O reale
  (`LARE_PYTOOLS_DIR` punta a una tempdir vuota → `resolve()` fallisce in modo
  sincrono e deterministico). Verificato RED (arm temporaneamente disabilitato,
  il test si blocca in attesa di una risposta mai inviata — fallimento per il
  motivo giusto) → GREEN (arm ripristinato). Scritto DOPO il codice
  dell'implementazione (non prima): la review ha notato che lo step 2 (wiring
  del protocollo) è deterministicamente testabile nell'harness esistente, a
  differenza dello step 4 del brief (I/O reale su IB Gateway, quello sì senza
  test automatico).

### Note

- **Deroga SOLID annotata** (`ws.rs`, doc-comment su
  `test_market_data_source_now`): il doc-comment di modulo in cima al file
  dichiara che `ws.rs` non dovrebbe contenere "logica di tool" — questa
  funzione la contiene. Scelta consapevole: lo scope dichiarato del Task 11
  è limitato a `ws.rs` + `main.rs`, e un client scoped per-chiamata (non
  condiviso altrove) non ha oggi una casa più naturale senza aggiungere un
  terzo file. Se il test cresce in futuro (più fonti, retry, cache), va
  estratto in un modulo dedicato.
- **Fix orfano di processo** (segnalato in review): senza uno `shutdown()`
  esplicito sul client scoped, il processo Python spawnato da `dispatch()`
  restava orfano indefinitamente a ogni chiamata (stessa classe di bug già
  corretta una volta per nmap). `test_market_data_source_now()` ora chiama
  `client.shutdown().await` subito dopo `dispatch()`, prima di interpretare
  la risposta — verificato: nessun `python.exe` residuo dopo la chiamata.
- **Verifica dal vivo**: vedi `task-11-report.md` per il dettaglio completo.
  In sintesi: venv Python di questo worktree riparato localmente (mai
  committato, cartella gitignored) per un mismatch di versione del
  pacchetto `mcp` (problema di ambiente pre-esistente, non introdotto da
  questo task). Con venv sano, un round-trip reale contro IB Gateway
  (porta 4001, raggiungibile) arriva fino a `IbkrDataSource._connect()`
  (Task 4/8) e fallisce lì con `"This event loop is already running"` —
  fatto osservato, riportato al supervisore per competenza (fuori dal
  file-scope di questo task); l'interpretazione della causa resta nel
  report, non qui.

## [0.41.17] — 2026-08-13 — Fix review finale whole-branch: nickname/badge `is_ai`

Ultima fix wave prima del merge su `main` del piano
`Docs/superpowers/plans/2026-08-13-aichat-display-names.md` (9 task, 14 commit).
Una review finale whole-branch (modello più capace, vede l'intero branch invece
di un singolo task) ha trovato 5 Important + 2 Minor sull'INTERAZIONE fra task,
invisibili a ogni review task-scoped precedente.

### Fixed

- **`agent.rs` — `set_ai_display_name` scriveva `network.json` con un pattern
  PERICOLOSO** (Important #1): faceva un round-trip COMPLETO attraverso il tipo
  tipizzato `AiChatConfig` (`load_or_generate` → muta il campo → `save`). Due
  problemi reali: (1) `network.json` è identità di RETE condivisa da più
  feature (il nome del file lo dichiara) — il round-trip cancellava
  silenziosamente qualunque chiave ignota scritta da un'altra feature; (2) su
  un file CORROTTO, `load_or_generate` rigenerava in silenzio i default
  (incluso `enabled: false`, che spegne l'intero servizio AI Chat) e li
  scriveva su disco — poi il tool aggiungeva il nome sopra quei default e
  ritornava successo, mascherando uno spegnimento silenzioso. Nuova
  `merge_ai_display_name`: legge il file grezzo come `serde_json::Value`,
  `insert` SOLO la chiave `ai_display_name`, preserva ogni altra chiave — stesso
  principio del gemello lato `ui`, `aichat_settings::merge_aichat_settings`. Un
  file davvero assente/vuoto riparte da `AiChatConfig::default()` serializzato
  (non da un oggetto `{}` vuoto: gli altri campi della struct — `enabled`/
  `label_base`/`chat_port` — non hanno `#[serde(default)]`, un file con la sola
  chiave `ai_display_name` romperebbe la prossima lettura tipizzata altrove).
  Un file NON vuoto che fallisce il parse è ora un errore esplicito (nessuna
  scrittura), a differenza del gemello `ui` (che riparte sempre da `{}` su
  qualunque fallimento) — scelta deliberatamente più prudente qui, perché
  questo tool è invocato dall'AI stessa, non da un umano davanti a `/config`.
  4 nuovi test (preserva chiavi ignote, rifiuta file corrotto senza toccarlo,
  semina dai default su file assente, il messaggio di successo avvisa del
  riavvio richiesto).
- **`network.json` risolto in DUE modi diversi nello stesso binario**
  (Important #4): `main.rs` legge ANCHE `startup.json` per risolvere
  `local_dir`; `agent.rs`/`ai_adapter.rs` avevano ciascuno un resolver privato
  quasi identico che si fermava a env > default, MAI `startup.json`. Su una
  macchina con `startup.json{local_dir:...}` impostato senza la env var
  `LARE_LOCAL_DIR`, il tool `set_ai_display_name` avrebbe scritto un
  `network.json` che `needs_ai_name_prompt`/`AiChatService` non avrebbero mai
  letto — un nickname "impostato con successo" che non compare mai, in
  silenzio. Nuove `aichat::config::resolve_network_json_path()` (impura, legge
  `exe_dir()`/`startup.json`/env) e `resolve_network_json_path_in` (pura,
  testabile, presa in prestito dallo stesso principio DI di
  `memory_file_path`/`memory_file_path_with_base`) — UNICO punto di
  risoluzione, condiviso da `agent::dispatch_tool` e
  `ai_adapter::needs_ai_name_prompt`. Rimossi i due resolver privati
  (`agent::default_network_json_path`, `ai_adapter::network_json_path`). 4
  nuovi test su `resolve_network_json_path_in` (precedenza env > startup.json >
  default).

### Changed

- **Messaggio di successo di `set_ai_display_name`** avvisa esplicitamente del
  riavvio richiesto: `"Nome impostato: {nome} (visibile in AI Chat dopo il
  riavvio dell'orchestrator)."` (Minor) — prima diceva solo `"Nome impostato:
  {nome}."`, lasciando intendere un effetto immediato che non c'è
  (`AiChatService` legge `ai_display_name` una sola volta all'avvio).
- **22 riferimenti a un path doc INESISTENTE** (`Docs/superpowers/sdd/
  2026-08-13-aichat-display-names/`, mai esistito — `Docs/superpowers/` ha solo
  `fixtures/`, `plans/`, `specs/`) corretti in tutto il repo (Important #5):
  `Docs/superpowers/plans/2026-08-13-aichat-display-names.md` dove si parla di
  task/esecuzione, `Docs/superpowers/specs/2026-08-13-aichat-display-names-design.md`
  dove si parla di design/motivazione. Un caso in `ai_adapter.rs` puntava a
  `task-8-brief.md` (vive solo in `.superpowers/`, gitignored, mai su GitHub) —
  corretto verso il piano.
- **`IMPLEMENTATION.md`**: la frase "stesso pattern già in uso da ogni altro
  consumer di `AiChatConfig` in questo crate (`main.rs`, `/config` backend Task
  5)" era FALSA — il backend `/config` del Task 5 non è in questo crate (è
  `ui`) e usava il pattern OPPOSTO (Value-based) proprio per evitare la perdita
  di dati che questa release corregge (Important #1). Corretta insieme alle
  altre sezioni rese stale dai due fix sopra (Important #2).
- **`Docs/HANDOFF.md`** aggiornato nello stesso commit — non lo era mai stato
  in nessuno dei 14 commit precedenti del piano (Important #3, viola la
  convenzione di progetto "si aggiorna nello stesso commit di ogni
  release/slice").

### Test

`cargo test` (protocol/mcp-server/orchestrator + i crate plugin in
`default-members`) — tutti verdi, nessuna regressione. `cargo clippy --all-targets
-p orchestrator` — stessi warning pre-esistenti di prima di questa release
(nessuno introdotto da questo fix).

## [0.41.16] — 2026-08-13 — Nudge one-shot per l'auto-assegnazione del nome AI (`ai_adapter.rs`)

Ottavo task del piano `Docs/superpowers/plans/2026-08-13-aichat-display-names.md`,
chiude il cerchio aperto dai Task 1 (`AiChatConfig::ai_display_name`) e 7
(tool `set_ai_display_name`, `agent.rs`): fa sì che l'AI SAPPIA di dover
chiamare quel tool, con un'istruzione one-shot nel proprio system prompt
quando serve.

**`needs_ai_name_prompt()`/`needs_ai_name_prompt_at(path)`** (nuove, private):
true quando AI Chat è attivo (`AiChatConfig::enabled`) ma l'AI non ha ancora
un nome (`ai_display_name.is_none()`). Stesso pattern DI di
`memory_file_path`/`memory_file_path_with_base`: la versione parametrizzata
sul path è quella testata, la versione reale (`network_json_path()` — env
`LARE_LOCAL_DIR` > `startup_config::default_local_dir()`, via
`startup_config::resolve`, stesso limite di `memory_file_path`: non legge
`startup.json`, fuori scope per questo piano) è un thin wrapper. Lettura
fresca a ogni iterazione del loop — nessuno stato in memoria da tenere
sincronizzato col file: dopo che `set_ai_display_name` scrive il nome,
questa funzione ricomincia a restituire `false` dal turno successivo.

**`AI_NAME_REQUEST_ADDENDUM`** (nuova costante di modulo): istruisce l'AI a
chiedere all'utente (o scegliersi da sola) un nome PRIMA di rispondere alla
richiesta corrente, poi a persisterlo col tool `set_ai_display_name`. Non
definita in `agent.rs` accanto a `WEB_SEARCH_ADDENDUM` (deliberato — quella
costante è usata da `agent::system_prompt`, che questo task NON tocca:
`TurnOptions`/`agent::system_prompt`/`LlmAdapter::respond` restano a firma
invariata, per non rompere ~50 siti di costruzione `TurnOptions` come
struct-literal esaustivo in `agent.rs`/`ai_adapter.rs`).

**Gap trovato e corretto nello stesso task (stesso genere del
`display_invocation` mancante scoperto dal Task 7)**: applicare l'addendum
SEMPRE (come nello pseudocodice originale del piano) avrebbe raggiunto anche
i canali tool esterni (es. mcp-nmap — `TurnOptions::system_prompt_override:
Some(...)`), il cui `ToolClient::tool_defs()` NON include
`set_ai_display_name` — l'AI si sarebbe vista istruita a chiamare un tool
che quel canale non possiede, e avrebbe rotto per costruzione l'uguaglianza
esatta asserita da un test esistente
(`channel_tool_client_restricts_tools_and_uses_own_system_prompt`,
`assert_eq!(req.system, "SEI IL CANALE FIXTURE...")`) su qualunque macchina
con `network.json` reale in stato "AI Chat attivo, nessun nome". Corretto
con una funzione di guardia dedicata, **`apply_ai_name_addendum(system:
String, has_override: bool, needs_prompt: bool) -> String`**: applica
l'addendum solo se `has_override` è `false` (nessun `system_prompt_override`
di canale — cursore/Telegram, mai i canali tool esterni). Estratta come
funzione PURA (nessun I/O) apposta per poterla testare senza toccare
env/filesystem reali — stesso principio DI di `needs_ai_name_prompt_at`,
diverso solo nell'assenza di percorso da parametrizzare (qui bastano due
booleani già calcolati dal chiamante).

**Wiring in `LlmAdapter::respond()`**: `let system = agent::system_prompt(opts);`
diventa `let system = apply_ai_name_addendum(agent::system_prompt(opts),
opts.system_prompt_override.is_some(), needs_ai_name_prompt());`, dentro il
loop (una lettura per iterazione) — stesso principio già in uso nella stessa
funzione per `self.backend.supports_web_search()` ("controlla qualcosa di
fresco proprio qui, prima di costruire il prompt").

**Test nuovi** (7, tutti in `ai_adapter.rs`): 4 su `needs_ai_name_prompt_at`
(true quando attivo+senza nome, false con nome/disattivo/file assente, come
da piano) + 3 su `apply_ai_name_addendum` (skip con override, skip quando
non serve, append quando serve e nessun override — questi ultimi 3 non
previsti dal testo letterale del piano, aggiunti per il gap sopra).

**Verifica della domanda del brief, con dato reale (non assunto)**:
`needs_ai_name_prompt()` (versione vera, non parametrizzata) NON risolve
dalla cwd di lancio dei test — risolve `LARE_LOCAL_DIR` (env) →
`%LOCALAPPDATA%\dev.lare.terminal\network.json`, esattamente come
`ai_adapter::memory_file_path`. Controllato direttamente (lettura, nessuna
scrittura) sulla macchina che ha eseguito questa suite:
`%LOCALAPPDATA%\dev.lare.terminal\network.json` esiste già, con
`enabled: true` e `ai_display_name` assente — quindi `needs_ai_name_prompt()`
ha restituito **`true`** durante l'intera run di 890 test, non `false`. La
suite è verde con l'addendum EFFETTIVAMENTE applicato a ogni test
`respond()` con `TurnOptions::default()` (nessun override) — verifica più
forte di quanto pianificato, non più debole: `channel_tool_client_restricts_
tools_and_uses_own_system_prompt` (l'unico test con `system_prompt_override:
Some(...)`, quello a rischio reale per il gap sopra) è passato perché la
guardia lo ha protetto DAVVERO in quello stato, non solo sulla carta. I due
test citati dal piano come a rischio
(`chat_system_prompt_without_memory_still_instructs_on_marker`,
`system_prompt_mentions_save_routine_rules`) restano comunque non toccati
per un motivo strutturale indipendente dallo stato di `network.json`:
chiamano le funzioni pure `chat_system_prompt`/`agent::system_prompt`
DIRETTAMENTE, mai attraverso `respond()`. Su una macchina diversa (o CI)
dove quel file fosse assente, `needs_ai_name_prompt()` tornerebbe `false`
per fail-safe (`std::fs::read` fallisce) — la suite resterebbe comunque
verde, ma senza esercitare dal vivo il ramo `true`.

## [0.41.15] — 2026-08-13 — Tool `set_ai_display_name` (`agent.rs`)

Settimo task del piano `Docs/superpowers/plans/2026-08-13-aichat-display-names.md`:
un nuovo tool locale, dato all'AI in ogni turno cursore (stesso set di
`run_in_session`/`open_target`/`show_markdown`), che le permette di
scrivere il PROPRIO nickname (`AiChatConfig::ai_display_name`, Task 1) in
`network.json`. Non passa da `mcp-server` (come `show_markdown`, tool
"orchestrator-native" — ADR-013): scrive il file direttamente, gated dal
meccanismo di conferma ESISTENTE (ADR-007) perché è un tool ad effetto
come qualunque altro — nessuna modifica al gate.

**`dispatch_tool` diventa un thin wrapper**: la funzione pubblica (3
parametri — `tools`, `name`, `input`) risolve ora `network.json` (env
`LARE_LOCAL_DIR` > `%LOCALAPPDATA%\dev.lare.terminal\`, via
`startup_config::resolve`/`default_local_dir`, già dipendenza del crate
dalla fase `startup-config-phase1`) e delega alla nuova `dispatch_tool_at`
(4 parametri, path esplicito) — stesso principio DI già in uso per
`ai_adapter::memory_file_path`/`memory_file_path_with_base`. TUTTI gli arm
esistenti (`run_in_session`, `open_target`, `search_routines`,
`run_routine`, `get_routine_content`, `save_routine`, più il catch-all
`other`) sono stati SPOSTATI invariati dentro `dispatch_tool_at`, senza
riscriverli: nessun comportamento cambiato per i tool già in produzione.
La firma pubblica di `dispatch_tool` resta identica — nessun chiamante
esistente (`tool_client.rs`, `cwd_tracking.rs`) tocco.

**Validazione dell'arm nuovo**: nome vuoto (dopo `.trim()`) → errore,
`network.json` non toccato; nome oltre 48 caratteri → errore. Altrimenti
`load_or_generate` (non `_with_migration`: `main.rs` esegue già la
migrazione `aichat.json`→`network.json` una volta all'avvio, prima che
l'AI possa ricevere un turno — quando questo arm gira il file esiste già)
+ `ai_display_name = Some(trimmed)` + `save`.

**`display_invocation` ha ora un arm dedicato per `set_ai_display_name`**
(`"imposta nome AI: {nome}"`): senza, sarebbe caduto sul catch-all
generico `[tool set_ai_display_name]`, usato SEMPRE come chunk di
trasparenza nel pannello (`ai_adapter.rs`, indipendentemente dal fatto che
il tool sia effettivamente gateizzato o meno) — l'utente avrebbe visto
un'etichetta senza il nome scelto, anche nel banner di conferma su
Telegram (dove tutti i tool tranne `show_markdown` sono gateizzati).

**Nota restart-required**: `AiChatService` legge `ai_display_name` UNA
SOLA volta all'avvio (v0.41.14, stesso pattern di `ai_participates`), non
"live". Questo tool scrive `network.json` a runtime: il nickname appare
in AI Chat solo dopo il prossimo riavvio dell'orchestrator — debito
accettato preesistente (`aichat-config-restart-policy`), non introdotto
né risolto da questo task.

**Effetto collaterale meccanico atteso** (NON un cambio di comportamento
di alcun arm esistente): `tool_defs()` cresce da 7 a 8 tool, quindi ogni
test che asserisce un conteggio HARDCODED del numero di tool esposti
all'AI si è rotto e va aggiornato di conseguenza (+1, o +1 sul totale con
web tool) — 8 siti in 4 file: `agent.rs` (`tools_for_default_has_eight_custom`,
rinominato da `_seven_` 7→8, `tools_for_no_windows_excludes_show_markdown`
6→7, `tools_for_web_search_adds_two_server_tools` 9→10),
`ai_adapter.rs` (`claude_text_only_end_turn_returns_text` 7→8,
`nowin_mode_excludes_show_markdown_tool` 6→7,
`web_search_mode_includes_server_tools` 9→10), `core.rs`
(`nowin_excludes_show_markdown_through_handle_command`, DUE assert nello
stesso test: `/nowin` 6→7 e "NL normale" 7→8), `tool_client.rs`
(`default_tool_defs_matches_historical_five` 7→8 — nome del test già
stale prima di questa modifica, non rinominato: fuori scope).

**Test**: `cargo test -p orchestrator` — 883 passed, 0 failed, 8 ignored
(baseline PRIMA di questo task: 879 passed; +4 nuovi test:
`tool_defs_includes_set_ai_display_name`,
`dispatch_set_ai_display_name_writes_network_json`,
`dispatch_set_ai_display_name_rejects_empty_name`,
`display_invocation_set_ai_display_name_shows_name`. Gli 8 fallimenti
intermedi visti durante lo sviluppo erano test PREESISTENTI con conteggio
hardcoded diventato stale, non test nuovi — corretti, non aggiunti).

## [0.41.14] — 2026-08-13 — `AiChatService` risolve `is_ai`/`display_name` in `publish_say`

Terzo task del piano `Docs/superpowers/plans/2026-08-13-aichat-display-names.md`:
il "collante" che fa sì che i due campi additivi introdotti nei Task 1/2
(`AiChatConfig::display_name`/`ai_display_name`, `wire::ChatLine`/
`ChatMsg::Say`/`protocol::ServerMsg::AiChatMessage::display_name`/`is_ai`)
vengano davvero CALCOLATI e stampati, invece di restare `None`/`false`
fissi.

**Due campi nuovi su `AiChatService`** (`display_name: Option<String>`,
`ai_display_name: Option<String>`): i nickname di QUESTA macchina, letti da
`network.json` (Task 1) e passati UNA volta all'avvio — NON "live", stesso
pattern di `ai_participates`/`ai_autoparticipate` già esistenti. `new()`
guadagna due parametri finali (`me, ai_adapter, ai_participates,
ai_autoparticipate, notes, display_name, ai_display_name`).

**`publish_say` (messaggi ORIGINATI da questa macchina — umano o AI
locale) CALCOLA**: `is_ai = from_label.ends_with("-ai")`, `display_name =
self.ai_display_name.clone()` se `is_ai`, altrimenti
`self.display_name.clone()`. Calcolato una sola volta, propagato invariato
a `ChatLine` (storico locale), `ChatMsg::Say` (rete) e
`ServerMsg::AiChatMessage` (eco UI) — un solo punto di verità per
messaggio.

**L'arm `ServiceEvent::PeerMsg { msg: ChatMsg::Say, .. }` (messaggi
RICEVUTI da un peer remoto) fa SOLO PASS-THROUGH**: il pattern cattura ora
`display_name`/`is_ai` (prima ignorati con `..`) e li ricopia
(`display_name.clone()`, `*is_ai`) in `ChatLine`/`AiChatMessage` senza
MAI ricalcolarli. Semantica deliberatamente OPPOSTA a `publish_say`: il
peer remoto ha già stampato questi campi con la propria `publish_say` (i
SUOI nickname configurati) — ricalcolarli qui da `from_label.ends_with(...)`
darebbe lo stesso `is_ai` (deterministico dalla label) ma un
`display_name` SBAGLIATO (risolveremmo il nickname configurato su QUESTA
macchina per un'etichetta che non è la nostra). Il relay server→altri
client (`server_relay`, via `msg.clone()`) porta comunque a bordo i due
campi automaticamente: nessuna modifica necessaria lì.

**Costruttori di comodo aggiornati** (`#[cfg(test)]`): i 3 esistenti
(`new_for_test`, `new_for_test_with_participation`,
`new_for_test_with_autoparticipate`) passano `None, None` come ultimi due
argomenti — comportamento invariato per tutta la suite esistente. Nuovo 4°
costruttore `new_for_test_with_display_names(me, display_name,
ai_display_name)` per i test che esercitano la risoluzione del nickname.
Aggiornati anche 3 call-site diretti a `AiChatService::new(...)` (non
tramite i costruttori di comodo) nello stesso file e 5 nei test di
integrazione (`tests/aichat_relay_loopback.rs`,
`tests/aichat_share_loopback.rs`) — nessuno di questi era coperto dal
`#[cfg(test)]` del lib crate, quindi la firma vecchia rompeva
`cargo build --tests -p orchestrator` finché non aggiornati.

**`main.rs`**: il call-site in produzione passa `None, None` per ora — il
wiring vero da `cfg.display_name`/`cfg.ai_display_name` (già letti in
`AiChatConfig`, Task 1) è compito del Task 4, deliberatamente non
anticipato qui per restare nel perimetro di questo slice.

**Test** (`aichat::service::tests`, 4 nuovi, RED→GREEN):
- `publish_say_stamps_is_ai_false_and_human_display_name_for_local_human_message`
  — `HumanSay` locale produce `is_ai=false` e `display_name` dal nickname
  umano configurato.
- `publish_say_stamps_is_ai_true_and_ai_display_name_for_local_ai_message`
  — `AiReply` locale produce `is_ai=true` e `display_name` dal nickname AI
  configurato (non da quello umano).
- `peer_msg_say_passes_through_received_display_name_and_is_ai_without_recomputing`
  — un `PeerMsg::Say` RICEVUTO con `display_name` del MITTENTE ("Gianni")
  deve riportare "Gianni" anche se il RICEVENTE ha nickname locali diversi
  configurati ("Maurizio"/"Aria"). Discrimina davvero pass-through vs
  ricalcolo: falsificato a mano (swap temporaneo a
  `self.display_name.clone()`/`self.ai_display_name.clone()`, verificato
  che il test fallisce con "Maurizio" invece di "Gianni", poi revertito) —
  è il RED che l'implementazione originale (senza questo test) non aveva.
- `peer_msg_say_passes_through_none_display_name_without_inheriting_local_ai_nickname`
  — un `PeerMsg::Say` con `display_name: None`/`is_ai: true` (mittente
  senza `ai_display_name` configurato) non deve ereditare il nostro
  `ai_display_name` locale.

`cargo test -p orchestrator`: 879 passed, 0 failed, 8 ignored (875 nella
baseline pre-Task-3 + questi 4 test nuovi).
`cargo clippy --all-targets`: nessun warning nuovo (i 7 preesistenti sono
in file non toccati da questo task — `config.rs`, `ws.rs` — o su righe di
test preesistenti in `service.rs` non modificate qui).

## [0.41.13] — 2026-08-13 — `wire::ChatLine`/`ChatMsg::Say`: `display_name`/`is_ai` additivi

Gemello, nel protocollo peer↔peer (`aichat/wire.rs`), del Task 2 già fatto su
`protocol` (Contratto A, WS orchestrator↔UI): due campi additivi
`#[serde(default)]` — `display_name: Option<String>` e `is_ai: bool` — su
`wire::ChatLine` e `ChatMsg::Say`. `impl From<wire::ChatLine> for
protocol::ChatLine` ora copia entrambi (prima ignorava silenziosamente
qualunque campo extra, essendo una conversione manuale campo-per-campo).

Nessun consumatore ancora: questo task NON risolve i valori (chi sono, come
si chiamano) — è puro trasporto. La risoluzione arriva con Task 3
(`AiChatService`, stesso piano) che valorizzerà i due campi al posto dei
`None`/`false` placeholder introdotti qui.

**Cascata meccanica**: i due campi nuovi non hanno `#[serde(default)]`
alcun effetto sui literal Rust diretti (quell'attributo vale solo per la
deserializzazione JSON) — ogni sito nel crate che costruiva `ChatMsg::Say`/
`wire::ChatLine` con un literal esaustivo ha smesso di compilare finché non
gli si aggiungeva `display_name: None, is_ai: false`; ogni sito che
*decostruiva* con un pattern esaustivo (`match`/`matches!`) ha richiesto
`..` per ignorare i due campi nuovi. Trovati tutti via `cargo build --tests
-p orchestrator` (non a occhio): 39 siti totali in
`aichat/service.rs` (33 costruzioni + 6 pattern), 1 in `aichat/transport.rs`,
1 in `aichat/channel.rs`, e 2 nei test di integrazione
(`tests/aichat_loopback.rs`, `tests/aichat_relay_loopback.rs`). Nessuna
logica cambiata — solo il campo aggiuntivo o l'ellissi di pattern.

Secondo task del piano `Docs/superpowers/plans/2026-08-13-aichat-display-names.md`.

## [0.41.12] — 2026-08-13 — `AiChatConfig`: campi `display_name`/`ai_display_name` (nickname)

Due campi opzionali additivi su `AiChatConfig` (`aichat/config.rs`):
`display_name: Option<String>` (nickname dell'umano, es. "Maurizio") e
`ai_display_name: Option<String>` (nickname che l'AI si è scelta/le è stato
assegnato, es. "Aria"), entrambi `#[serde(default)]` → `None` — un
`network.json` scritto prima che questi campi esistessero deserializza
senza errore, e `None` è il fallback esplicito al comportamento odierno
(bare `label_base` mostrato in AI Chat). `impl Default` aggiorna i due
campi finali dell'inizializzatore letterale (`display_name: None,
ai_display_name: None`).

Primo task (indipendente) del piano
`Docs/superpowers/plans/2026-08-13-aichat-display-names.md`: i due campi sono
solo dati, non ancora letti da nessun consumatore — task successivi (non
in questa release) li useranno per mostrare il nickname in AI Chat
(`main.rs`, `AiChatService`, tool `set_ai_display_name`).

Nota di scope: il bump di `Cargo.toml` a questa versione è avvenuto in un
commit di follow-up dello stesso task (il brief limitava lo scope
iniziale a questo file + CHANGELOG/IMPLEMENTATION.md), non nel commit che
ha introdotto i due campi.

## [0.41.11] — 2026-08-12 — `main.rs`: consolidamento startup.json (fase 1)

Le 5 copie quasi identiche della catena `LARE_LOCAL_DIR` →
`%LOCALAPPDATA%\dev.lare.terminal\` → `.lare-data\` (token, telegram,
search, plugin host, aichat) sono ridotte a UNA risoluzione (`local_dir`,
subito dopo il log delle versioni all'avvio), passata già pronta a tutti
e 5 i siti. `exe_dir()`/`startup.json` risolti una sola volta insieme
(`startup_cfg`). Rimosse anche le 2 risoluzioni locali temporanee
introdotte da 0.41.9/0.41.10 (blocco AI adapter, blocco Telegram) — ora
usano lo stesso binding condiviso. `llms_config::resolve_path`/
`telegram::settings::resolve_path` ricevono `local_dir`/`exe_dir` + il
campo del file corrispondente invece di rileggere l'ambiente per conto
proprio. `LARE_PLUGINS_DIR` preserva il proprio comportamento senza trim
(unico campo con questa eccezione, deliberata — vedi spec §3), ma ora può
anche essere impostato da `startup.json` quando l'env non è presente
(prima impossibile: nessuna fonte file esisteva per questo campo).

Effetto collaterale positivo verificato: prima di questo consolidamento,
un `startup.json` malformato veniva loggato DUE volte all'avvio (blocco
AI adapter + blocco Telegram, ciascuno rileggeva il file per conto
proprio). Con `startup_cfg` risolto una sola volta a inizio `main()`, il
warning compare ora una sola volta — confermato con un test manuale
(`startup.json` con contenuto non valido accanto a un binario compilato:
un solo `WARN ... JSON malformato ... — uso i default` in stderr).

Chiude la fase 1 di `Docs/superpowers/specs/2026-08-12-startup-config-design.md`.
`ui` resta invariata (fase 2, design a parte — Tauri managed state).

## [0.41.10] — 2026-08-12 — `telegram::settings::resolve_path`: fix ancoraggio cwd→exe_dir + `startup.json`

`resolve_path` risolveva `telegramsettings.json` (in assenza di override)
contro la cwd di LANCIO (`std::env::current_dir()`), non la cartella
dell'eseguibile — per un futuro servizio Windows lanciato da SCM la cwd di
default è spesso `System32`, non la cartella di installazione. Ancorato
ora a `exe_dir()` (stesso meccanismo di `McpToolClient::resolve()`), mai
la cwd. Guadagna anche un livello di precedenza (`telegram_settings` di
`startup.json`) fra `LARE_TELEGRAM_SETTINGS` e il default. Normalizzato lo
stesso bug minore di 0.41.9: l'override env non faceva `.trim()` — ora
trim sempre via `startup_config::resolve`.

`main.rs:219-222` (unico chiamante) aggiornato nello stesso commit con una
risoluzione locale di `exe_dir`/`startup_cfg` — temporanea, sostituita dal
binding condiviso in Task 5. `launch_dir` (l'ancoraggio sbagliato, cwd di
lancio) è eliminata insieme al suo unico uso.

## [0.41.9] — 2026-08-12 — `llms_config::resolve_path` da `startup.json` (fase 1)

Stesso pattern di `mcp-server` 0.7.1: `resolve_path` guadagna un livello
di precedenza (`llms_config` di `startup.json`) fra `LARE_LLMS_CONFIG` e
il default, e non ri-deriva più `%LOCALAPPDATA%\dev.lare.terminal\` per
conto proprio (`default_path` eliminata) — riceve `local_dir` già risolto.
Normalizzato un bug minore preesistente: l'override env non faceva
`.trim()` sull'input (solo `!p.is_empty()`) — ora trim sempre, via
`startup_config::resolve`, coerente con `default_path` che il trim lo
faceva già. Nessun cambio di comportamento osservabile per chi non ha mai
usato spazi nella env var.

## [0.41.8] — 2026-08-11 — `/help` aggiornato con i comandi mancanti

`HELP_MARKDOWN` (`core.rs`) era rimasto fermo a v0.10.0: mancavano 4 comandi
core aggiunti da allora — `/aichat` (UI-local, app.js) e i tre canali tool
esterni digitabili da cursore (`external-channels.js`/
`EXTERNAL_TOOL_CHANNELS`): `/markets`, `/nmap`, `/pyping`. Aggiunta una
sezione "Strumenti esterni" per i tre canali (finestra dedicata, non
gestiti da `handle_slash`). Esclusi deliberatamente: i comandi dei plugin
(`/calc`, `/lc`, `/crypto`, ...) — scoperti a runtime dal manifest plugin,
non fanno parte di questa costante statica — e `/library-expand` (nessun
trigger da cursore per design, si apre solo dal pulsante "Espandi").

Test esistente `slash_help_produces_open_window_and_done` esteso con 4
nuove asserzioni (una per comando) invece di un nuovo test — stessa
funzione già verificava il contenuto reale dell'help.

## [0.41.7] — 2026-08-11 — `/markets`: secondo screener `goldman-sachs`

Nuovo screener registrato nel canale `financial-markets`: `goldman-sachs`
("Goldman Sachs" — report equity research stile analista senior, top
titoli (default 25), quota minima 30% Technology/Communication Services, bonus
punteggio per prezzo sotto 80-100$, nessuna esclusione — top assoluto
ripetibile). Tutta la logica nuova vive in Python
(`scripts/pytools/financial-markets/screeners/goldman_sachs.py`, modulo
indipendente da `consumer_usage.py` per design), come previsto
dall'infrastruttura di registry (0.41.4-0.41.6): l'unico tocco in questo
crate è una riga nell'elenco statico id/titolo della `description` di
`run_screener` (`external_channel.rs`).

`ScreeningSnapshot` (shared, `scripts/pytools/financial-markets/data_sources/`)
esteso con 6 campi nullable (`pe_ratio`, `debt_to_equity`, `dividend_yield`,
`payout_ratio`, `target_high_price`, `target_low_price`) — additivo, nessun
impatto su `consumer-usage`.

## [0.41.6] — 2026-08-11 — `/markets`: `screen_stocks` → `list_screeners`+`run_screener`

Il canale `financial-markets` espone ora 5 tool invece di 4: `screen_stocks`
(l'unico screener cablato) sostituito da `list_screeners` (elenca gli
screener disponibili, apre il picker) + `run_screener(screener_id)`
(dispatcher generico — il registry vero vive interamente in Python,
`scripts/pytools/financial-markets/screeners/`). Il primo screener migrato
è `consumer-usage` (comportamento identico a `screen_stocks` di prima).
Aggiungere lo screener #2 non tocca più questo file a parte una riga
nell'elenco statico id/titolo nella `description` di `run_screener` (il
registry Python resta l'unica fonte di verità sulla logica).

3 test esistenti aggiornati (trovati dal compilatore, non dalla suite —
riferivano `screen_stocks_title`/il nome fisso `"screen_stocks"`, entrambi
rimossi): `financial_markets_system_prompt_names_all_four_tools` →
`..._all_five_tools`, `financial_markets_channel_exposes_four_tools` →
`..._five_tools`, `screen_stocks_title_is_fixed` →
`run_screener_title_fallback_reads_screener_id_from_input`.

Vedi `Docs/superpowers/specs/2026-08-11-markets-screener-registry-design.md`.

## [0.41.5] — 2026-08-11 — `list_screeners` → `ServerMsg::OpenScreenerPicker`

Nuova eccezione nominata in `ai_adapter.rs` (stesso pattern di `show_markdown`,
ADR-013): quando il tool `list_screeners` ritorna JSON `{"summary","items"}`
valido, l'adapter emette `ServerMsg::OpenScreenerPicker{items}` e usa
`summary` (testo breve) come `ToolResult` per l'AI invece del JSON grezzo.
JSON malformato → fallback invariato (nessun crash, nessuna finestra, testo
grezzo torna all'AI). Rispetta `opts.allow_windows` (`/nowin`) come
`show_markdown`/`report`. Vedi
`Docs/superpowers/specs/2026-08-11-markets-screener-registry-design.md`.

## [0.41.4] — 2026-08-11 — `PythonReportJson.title` (precedenza sul fallback Rust)

Campo additivo `title: Option<String>` nel contratto JSON `{summary,
report_markdown, channel_summary, title?, title_suffix?}` che un tool Python
può seguire. Quando presente, `split_report` lo usa come titolo BASE della
finestra al posto di `report_title(input)` (che resta fallback per
`stock_report`/`list_stocks`, invariati). Prerequisito per `run_screener`
(dispatcher generico multi-screener, prossima voce di questa serie): un
dispatcher generico non può avere una title-fn Rust per-screener, il
registry Python è l'unica fonte che conosce il titolo giusto.

Effetto collaterale trovato dal compilatore: `telegram/channel.rs::format_response`
fa match esaustivo su `ServerMsg` — la nuova variante `OpenScreenerPicker`
(protocol 0.15.2, commit precedente) ha richiesto un braccio no-op, stesso
trattamento delle altre finestre UI-only (`OpenPluginWindow`,
`RoutineSavePreview`, …): Telegram non ha una superficie di selezione a
lista.

Vedi `Docs/superpowers/specs/2026-08-11-markets-screener-registry-design.md`.

## [0.41.3] — 2026-08-07 — heartbeat: fix dalla review finale whole-branch

Fix dalla review finale whole-branch pre-merge della feature heartbeat (0.41.2), 4 finding.

**Il finding importante — `on_heartbeat` mancava il caso `input_json_delta`.** Lo spec (§0/§1)
dichiara l'obiettivo come propagare lo STESSO segnale di liveness che `idle_timeout` già ha
("resetta su qualsiasi byte"). Lo shippato in 0.41.2 faceva scattare `on_heartbeat` solo su
`Some("ping")` — più stretto dell'obiettivo dichiarato. Concretamente: quando l'AI streamma un
tool-call lungo (es. il contenuto di `show_markdown`, verso cui il system prompt di progetto
indirizza le risposte lunghe/formattate), il contenuto arriva come eventi `input_json_delta`
(`messages_client.rs`, ramo `Some("input_json_delta")`) — attività reale e frequente sul wire
che prima non faceva scattare né `on_text` né `on_heartbeat`. Questa è la ricostruzione più
plausibile dell'incidente originale (184s di silenzio) che ha motivato l'intera feature. Fix:
`on_heartbeat()` aggiunta come prima istruzione nel ramo `Some("input_json_delta")` di
`SseAccumulator::handle_event`. NON è un nuovo timer gemello scollegato dal traffico reale
(esplicitamente fuori scope) — `input_json_delta` è un evento reale osservato sul wire, prova
diretta che il modello sta producendo input di un tool. TDD: nuovo test
`input_json_delta_events_trigger_on_heartbeat_without_any_ping` (RED prima del fix — 0
heartbeat su 3 delta senza ping — GREEN dopo).

**3 finding minori:** `Cargo.lock` risincronizzato (`ui` era rimasto a `0.46.1` mentre
`Cargo.toml` era già a `0.46.2` — Task 3 di 0.41.2/ui-0.46.2 aveva perso il sync); doc-comment
di `ChatBackend::send_turn` (`chat_backend.rs`) aggiornata per menzionare il parametro
`on_heartbeat` (prima documentava solo `on_text`); voce di `Docs/HANDOFF.md` corretta (date
per-task sbagliate — tutti e 3 i commit della feature sono in realtà dello stesso giorno,
2026-08-07 — e riferimento a `app.js:641-645` corretto in `app.js:640-645`, la riga reale del
`case "heartbeat":`).

## [0.41.2] — 2026-08-07 — heartbeat sul turno AI (propaga il ping SSE)

Incidente reale: una richiesta di documento lungo veniva annullata dal watchdog frontend (184s
di silenzio percepito) mentre il turno stava ancora lavorando — il fix era già in produzione,
solo scollegato dal watchdog. `HttpMessagesClient` osserva già correttamente i byte grezzi dello
stream SSE (`idle_timeout`, tollera i `ping` di keep-alive di Anthropic), ma `handle_event`
scartava l'evento `ping` invece di propagarlo. Nuovo callback `on_heartbeat` (gemello di
`on_text`, nessun payload) filtrato attraverso `MessagesClient::create_streaming` →
`ChatBackend::send_turn` → `LlmAdapter::respond`, che emette `ServerMsg::Heartbeat{id}`
(`protocol` 0.15.1) sullo stesso canale WS già in uso. Nessuna nuova soglia, nessun timer gemello
scollegato dal traffico reale — vedi
`Docs/superpowers/specs/2026-08-07-ai-turn-heartbeat-watchdog-design.md`.

Effetto collaterale mechanico: `telegram/channel.rs::format_response` fa `match` esaustivo su
`ServerMsg` — aggiunta la variante `Heartbeat` come no-op permanente (Telegram non ha un
concetto di watchdog/timer di silenzio), stesso trattamento delle altre varianti UI-only già
presenti in quel match.

## [0.41.1] — 2026-08-05 — Review finale save_routine: messaggistica del confirmer + comment cleanup

Fix dalla review finale whole-branch pre-merge di save_routine (Fase 2), findings I3/M2.

**I3 — il chunk "anteprima routine aperta" era fuorviante su Telegram.** `ai_adapter.rs`
emetteva incondizionatamente `"📄 anteprima routine '<name>' aperta — rivedila nella
finestra"` prima di dispatchare a QUALUNQUE confirmer attivo — vero solo per la UI locale
(che apre davvero `routine-preview.html`), fuorviante su Telegram (nessuna finestra, round-
trip a bottoni via il default di `confirm_routine_save`). Spostato in
`LocalUiConfirmer::confirm_routine_save` (`local_confirm.rs`): ogni confirmer è ora
responsabile della propria messaggistica channel-appropriate. `LocalUiConfirmer` guadagna
un campo `command_id: String` (impostato da `ws.rs` all'`id` del comando/turno corrente) —
serve al `ServerMsg::Chunk` per associarsi alla bolla di comando giusta, esattamente come
ogni altro Chunk dello stesso turno in `respond()`. `LocalUiConfirmer::new` ha quindi un
nuovo 4° parametro (rottura di firma interna, `pub(crate)`, nessun impatto esterno). Tre
test in `local_confirm.rs` aggiornati (si aspettavano `RoutineSavePreview` come primo
messaggio sul canale, ora è il Chunk).

**M2 — comment cleanup.** Rimosso un paragrafo di commento duplicato/contraddittorio in
`ai_adapter.rs` (l'eccezione `save_routine` era descritta due volte, la copia più vecchia
non la menzionava).

## 0.41.0

- `save_routine`/`get_routine_content` (Fase 2 del repository di routine —
  Docs/superpowers/specs/2026-08-05-save-routine-design.md): due nuovi tool AI, wired
  in `tool_defs`/`dispatch_tool`/`display_invocation`/`SYSTEM_PROMPT`. `save_routine`
  ha una superficie di conferma DEDICATA (`ToolConfirmer::confirm_routine_save`,
  default-bodied — solo `LocalUiConfirmer` la sovrascrive per aprire una finestra
  invece del banner Sì/No) ed è escluso dal banner batched: un turno con
  `save_routine` + un altro tool gateizzato produce due conferme indipendenti.
  `ToolClient` guadagna gli stessi due metodi come default-bodied (rifiuto
  "non disponibile su questo canale") — override reali su `McpToolClient`,
  `FakeToolClient` (canned), `CwdTrackingToolClient` (delega a `inner`); ogni altro
  canale (fixture/nmap/python) li eredita senza modifiche. `save_routine` aggiunto a
  `SENSITIVE_TOOLS`.

## [0.40.75] — 2026-08-04 — Trasparenza `run_routine`: mostra la description della routine

La label di trasparenza/conferma per `run_routine` (gateizzato, `local_confirm.rs`) mostrava solo
`routine: <name>`, senza dire all'utente cosa fa la routine prima di doverla approvare. Ora, per
questo tool soltanto, la label diventa `Uso la routine disponibile <name> (<description>)` — la
description è recuperata dall'indice via `search_routines` (match esatto case-insensitive sul
nome, non il primo risultato per sottostringa). Se il lookup non trova un match esatto, ricade sul
comportamento di sempre (`agent::display_invocation`). Nuova funzione privata
`ai_adapter::build_label`, usata sia per il banner di conferma sia per il Chunk post-esecuzione;
`format_invocation` (override canale esterno) vince sempre, invariato. Nessun cambio di wire
format/protocollo.

## [0.40.74] — 2026-08-04 — `LARE_LOCAL_DIR`: override generale della cartella dati Local

Nuova variabile d'ambiente `LARE_LOCAL_DIR`: un override a percorso completo che sostituisce
`%LOCALAPPDATA%\dev.lare.terminal\` ovunque il crate risolve quella base — pensata per casi come
un profilo condiviso fra macchine sotto una radice comune (es. `C:\Lare Terminal\local-data\`)
invece della cartella utente Windows. Semantica: path completo usato verbatim (i `.join(...)`
esistenti — `token`, `network.json`, `plugins/`, `llms.json`, `memory-*.md`, ecc. — restano
invariati sopra qualunque base risolta); non impostata → comportamento identico a oggi.

Onorata in 8 punti, tutti con la stessa catena di precedenza (override specifico del sito, se
esiste → `LARE_LOCAL_DIR` → `LOCALAPPDATA` → fallback `.lare-data`):
- **`main.rs`**, 5 siti inline: token (`token_store`), stato Telegram, config di ricerca
  (`search-paths.json`), plugin host/storage, note/aichat inbox. Nessun helper condiviso — stesso
  pattern già duplicato ~13 volte nel crate prima di questo piano, deliberatamente non
  refactored. Nuovo log di avvio sul sito del token (il primo risolto):
  `tracing::info!("Local data dir: {path}")`.
- **`llms_config.rs::default_path`** (root di `llms.json`).
- **`ai_adapter.rs::memory_file_path_with_base`** e **`aichat/service.rs::memory_file_path_with_base`**
  (duplicato indipendente dello stesso schema di file di memoria persistente per-AI, non
  consolidato in un helper comune — stessa scelta di design delle altre duplicazioni sopra).
  Entrambi guadagnano il parametro `local_dir_override: Option<String>`, testabile senza mutare
  l'ambiente reale del processo.

4 nuovi test (2 in `llms_config.rs`, 1 in `ai_adapter.rs`, 1 in `aichat/service.rs`), pattern
`*_local_dir_override_wins_over_local_appdata` (+ `*_ignores_empty_local_dir_override` dove
presente). I 5 siti inline di `main.rs` non hanno test dedicati — stesso limite dei blocchi che
sostituiscono (codice dentro `main()`, non estratto in funzioni testabili).

## [0.40.73] — 2026-08-03 — test coverage per display_invocation + correzioni doc dalla review finale

Aggiunta copertura test mancante per i due arm `run_routine`/`search_routines` di
`agent::display_invocation` (il testo che finisce nel banner di conferma per i tool
gateizzati — rilevante per la sicurezza, non solo cosmetico). Correzioni di
accuratezza documentale trovate nella seconda review whole-branch: `HANDOFF.md`
allineato alle versioni reali + menzione esplicita che il wiring rende ora
raggiungibili i due tool; un'etichetta "NON nominato dal piano" errata in
`IMPLEMENTATION.md`; un code-span markdown spezzato su due righe in
`local_confirm.rs`; il conteggio "3 tool storici" in `tool_client.rs` non
aggiornato a 5 dal Task 5.

## [0.40.72] — 2026-08-03 — search_routines/run_routine raggiungibili dall'AI (wiring mancante)

Critical trovato nella review finale whole-branch del piano `routines-repository`: i Task 1-4
avevano costruito `search_routines`/`run_routine` come tool MCP funzionanti in `mcp-server`
(v0.6.0) e gateizzato `run_routine` in `SENSITIVE_TOOLS` (0.40.71 sopra), ma **nessun task
toccava `agent.rs`/`tool_client.rs`**: `agent::tool_defs()` esponeva ancora solo i 3 tool
storici, `agent::dispatch_tool` rifiutava qualunque nome diverso da `run_in_session`/
`open_target` come "sconosciuto", e nessun `ToolClient` sapeva chiamarli — i due tool erano
del tutto irraggiungibili dal loop AI.

**`tool_client.rs`**: trait `ToolClient` guadagna due metodi (`search_routines(query)`,
`run_routine(name, args)`) e i tipi risultato `SearchRoutinesResult`/`RunRoutineResult`
(quest'ultimo mirror esatto di `RunRoutineOutput` lato mcp-server, incluso `cwd`).
`McpToolClient` li implementa con lo stesso pattern lazy-connect duplicato verbatim di
`run_in_session`/`open_target` (duplicazione intenzionale, stessa convenzione già in uso —
non refactored). `FakeToolClient` ritorna esiti canned; `FixtureChannelToolClient` li rifiuta
come "non disponibile su questo canale".

**`agent.rs`**: due nuove `ToolDef` in `tool_defs()`, due arm in `dispatch_tool`, due arm in
`display_invocation` (vincolante per il gate — senza un'etichetta dedicata il banner di
conferma di `run_routine` ricadrebbe sul fallback generico `[tool run_routine]`, senza dire
quale routine). `SYSTEM_PROMPT` aggiornato: elenca i due nuovi tool e istruisce l'AI a
preferire `search_routines`+`run_routine` a riscrivere un comando da zero quando pertinente.

**Gap del piano trovato IN CORSO, non solo nella review**: il trait `ToolClient` essendo un
metodo `async fn` obbligatorio (nessun default — vincolo di object-safety, vedi doc-comment
del trait), OGNI implementor del workspace doveva guadagnare i due nuovi metodi per
compilare, non solo i 3 nominati dal piano di dettaglio (`FakeToolClient`/
`FixtureChannelToolClient`/`McpToolClient`). Altri 7 implementor coinvolti:
- **`cwd_tracking.rs::CwdTrackingToolClient`** — CRITICO, non cosmetico: è il decorator che
  `main.rs` avvolge attorno al vero `McpToolClient` in produzione (`main.rs:201`). Uno stub
  di rifiuto qui avrebbe riprodotto ESATTAMENTE il bug che questo task risolve, un livello
  più sopra. Delega a `self.inner` per entrambi i metodi; `run_routine` aggiorna anche
  `cwd_state` se `result.cwd` non è vuoto — una routine gira nella STESSA shell persistente
  di `run_in_session` (`mcp-server`'s `run_routine` delega a `Session::run`, `cwd` è
  popolato dal risultato reale, non hardcoded), quindi uno script con `Set-Location` deve
  aggiornare lo stato condiviso esattamente come un comando raw, altrimenti `/find` e la UI
  leggerebbero una cwd stantia dopo l'esecuzione di una routine. Nuovo test
  `dispatch_run_routine_updates_cwd_state` (mirror di `dispatch_run_in_session_updates_cwd_state`).
- **`external_channel.rs::EmptyToolClient`**, **`nmap_tool_client.rs::NmapToolClient`**,
  **`python_mcp_tool_client.rs::PythonMcpToolClient`** — stub "non disponibile su questo
  canale", stesso pattern isolamento-canale già in uso per `run_in_session`/`open_target`
  su questi 3 client. Difesa in profondità: `tool_defs()` di questi canali non include mai
  `search_routines`/`run_routine`, quindi il modello non li vede nel menu — lo stub copre
  solo il caso di un nome allucinato fuori menu, non è lui il vero gate di isolamento.
- **`ai_adapter.rs`** (3 `ToolClient` di test locali a singole funzioni di test,
  `ReportProducingToolClient`/`DeferredReportToolClient`/`CancelingDeferredReportToolClient`)
  — stessi stub "non disponibile" dei loro `run_in_session`/`open_target` esistenti.

**Conteggi tool aggiornati** (base storica 3 → 5): `agent.rs` (`tools_for_default_has_five_custom`,
`tools_for_no_windows_excludes_show_markdown` 2→4, `tools_for_web_search_adds_two_server_tools`
5→7), `tool_client.rs` (`default_tool_defs_matches_historical_five`), `ai_adapter.rs` (linea
797: 3→5; più due asserzioni non nominate dal piano di dettaglio ma matematicamente le stesse
conseguenze del conteggio base 3→5: `nowin_mode_excludes_show_markdown_tool` 2→4,
`web_search_mode_includes_server_tools` 5→7), `core.rs` (linea 1437: 3→5; più
`/nowin`-nella-stessa-funzione 2→4, non nominata dal piano). Nuovo test
`fixture_channel_dispatch_rejects_run_routine_as_unknown` (`tool_client.rs`).

`crates/mcp-server`: due correzioni di accuratezza documentale (claim di streaming inesistente
rimossa da CHANGELOG + doc-comment di `run_routine` — `RunRoutineParams` non ha
`progress_token`), guard `resolve_root` allineato a `.trim().is_empty()` (coerente con
`search`/`build_invocation`), numerazione "Four-level" → "Five-level" in IMPLEMENTATION.md.

`cargo build` (intero workspace): pulita. `cargo test -p mcp-server -p orchestrator`: verde,
nessuna regressione fuori dai conteggi sopra. `cargo clippy -p orchestrator --all-targets`:
nessun nuovo warning nei file toccati (i pochi warning residui sono preesistenti, in
`aichat/config.rs`/`aichat/service.rs`, fuori scope).

## [0.40.71] — 2026-08-03 — run_routine sempre gateizzato anche dal canale locale

`SENSITIVE_TOOLS` (`local_confirm.rs`) guadagna una seconda voce reale: `run_routine`
(mcp-server 0.6.0). A differenza di `run_in_session` (autonomo in locale — il suo testo
completo è già il chunk di trasparenza), una routine si invoca per nome: il corpo non è
ri-mostrato a ogni esecuzione, stesso principio di rischio già alla base della lista (finora
solo i cinque tool attivi `nmap_*`). `search_routines` resta autonomo (sola lettura).

## [0.40.70] — 2026-07-31 — fix `render_body`: segmento vuoto non genera più intestazione penzolante

Trovato in review (non ancora osservato dal vivo) subito dopo 0.40.69: quel
fix rende raggiungibile per la prima volta `NoteEditRequested { text: "" }`
(l'utente cancella deliberatamente il proprio segmento nel dialog "Modifica"
ora che parte precompilato). `merge_note` non rimuove mai un `Segment` dal
vettore (serve a preservare il `seq` per il merge CRDT), quindi una nota a
2+ macchine con un segmento svuotato aveva ancora `segments.len() >= 2` →
`render_body` mostrava comunque l'intestazione `— machine —` di quella
macchina, senza nulla sotto. Fix: `render_body` esclude i segmenti a testo
vuoto sia dal conteggio "2+ macchine → intestazioni" sia dal rendering — un
segmento vuoto conta come "non ha contribuito", non "ha contribuito con
niente". 2 nuovi test: `render_body_empty_segment_excluded_no_dangling_header`,
`render_body_all_segments_empty_is_empty_string`.

## [0.40.69] — 2026-07-31 — fix smoke test dal vivo: `my_segment_text` in `to_note_view`

Primo finding del primo smoke test multi-macchina reale del Blocco note:
"Modifica" mostrava il titolo ma mai il testo. `to_note_view` (protocol
`NoteView::my_segment_text`, v. changelog protocol 0.14.9) diventa un metodo
(`&self` invece di funzione associata) per poter isolare il segmento di
QUESTA macchina via `self.me.label_base` — prima non aveva modo di sapere
quale fosse "questa macchina" fra i segmenti della nota. Aggiornati tutti i
7 call site. Due nuovi test: `note_view_my_segment_text_reflects_own_segment_on_create`
e `note_view_my_segment_text_empty_when_this_machine_never_contributed`
(quest'ultimo verifica che il corpo fuso resti visibile in `body` anche
senza contributo proprio — `my_segment_text` e `body` non vanno confusi).

## [0.40.68] — 2026-07-31 — fix di review finale: id nota seedato, merge no-op silenzioso

Due correzioni trovate nella review dell'INTERO branch "Blocco note" (gap di
integrazione fra task, invisibili alle review dei singoli task).

**FIX 1 (Critical, perdita di dati)** — `AiChatService::new` inizializzava
`next_note_id: 0` incondizionatamente, anche ricevendo uno store già popolato
da `notes.json`. Dopo un riavvio, la prima nota creata riusava l'id
`"<mio-ip>:0"` di una nota esistente: `upsert_merged` FONDE su id noto invece
di inserire, quindi il testo nuovo veniva scartato (il vecchio segmento aveva
`seq` maggiore), il titolo sovrascritto via LWW e — se la vecchia era un
tombstone — la nota "nuova" nasceva già cancellata. Tutto silenziosamente, e
il risultato corrotto veniva pure ribroadcastato a ogni altra macchina. Il
contatore è ora SEEDATO dal massimo suffisso già presente fra gli id con il
proprio prefisso IP, +1 (0 se non ce ne sono).

**FIX 5 (Important, traffico e I/O inutili)** — gli arm remoti
(`ChatMsg::NoteUpdated`, il ciclo `push` di `NotesDigestReply`, e
`NotesData`) emettevano `ToUi(NoteUpserted)` + `SaveNotes` + fan-out per OGNI
nota in arrivo, anche quando il merge non cambiava nulla — contro il design
§5.2 punto 4 ("ri-broadcast solo se CAMBIATA"), con il rischio di rimbalzi che
si auto-alimentano fra peer. Nuovo helper condiviso `apply_incoming_note`
(snapshot prima del merge, confronto `PartialEq`, effetti solo se diverso);
`broadcast_note_to_links` resta INTATTA e incondizionata per i quattro arm
locali, dove il broadcast è sempre dovuto.

Test: 2 nuovi (`next_note_id_is_seeded_from_the_loaded_store_so_new_notes_never_collide`,
`peer_msg_note_updated_identical_to_stored_produces_no_effects`, entrambi RED
prima del fix) + 1 di caratterizzazione in `notes/mod.rs`
(`merge_same_machine_same_seq_same_text_is_lossless_and_order_independent`),
che chiude il ramo di tie-break `seq` uguale mai esercitato — precondizione che
proprio il FIX 1 rende davvero vera.

---

## [0.40.67] — 2026-07-31 — `ws.rs` instrada i ClientMsg di Blocco note (dispatch)

Quattro nuovi arm in `handle_client_msg`, stessa forma esatta dei cinque
`ClientMsg::Share*` già esistenti — nessuna logica nuova, solo `h.send(...)`
verso l'inbox `AiChatService`.

---

## [0.40.66] — 2026-07-31 — replay `NotesSnapshot` su ogni SetServerTx (Blocco note)

Ultimo pezzo lato `AiChatService`: ogni volta che la UI locale si (ri)connette,
riceve subito lo snapshot completo delle note — stesso principio già in uso
per `AiChatReachablePeers`/roster/storico. Con questo, il backend di "Blocco
note" è funzionalmente completo; restano da collegare `ws.rs` (Task 14),
l'avvio in `main.rs` (Task 15) e il frontend (Task 16-19).

---

## [0.40.65] — 2026-07-31 — `ChatMsg::NotesData` in arrivo, merge fulfillment (Blocco note)

Ultimo tassello del giro di riconciliazione: fulfillment di un `want` proprio,
merge di ogni nota ricevuta, push alla UI. Nessun fan-out qui (di proposito —
v. commento nel codice): eviterebbe un doppio invio, il mittente gestisce già
il proprio fan-out.

---

## [0.40.64] — 2026-07-31 — `ChatMsg::NotesDigestReply` in arrivo (Blocco note)

Soddisfa `want` con `NotesData`, fonde ogni nota in `push` via `merge_note`, e
ribroadcasta ai già-collegati (esclusa la fonte) — chiude il gap esplicito
del design (§5.2 punto 4): una riconciliazione porta i cambiamenti anche a
chi era già in rete, non solo al nuovo arrivato.

---

## [0.40.63] — 2026-07-29 — `ChatMsg::NotesDigest` in arrivo risponde con want/push (Blocco note)

Riusa `notes::digest::reconcile` (già testato in isolamento, Task 3) — nessuna
logica di confronto duplicata qui, solo il collegamento all'attore.

---

## [0.40.62] — 2026-07-29 — timer di riconciliazione note dopo link stabilito (Blocco note)

`Effect::StartNotesReconcileTimer` (gemello di `StartVoteTimeout`, stesso
pattern "timer come Effect") agganciato ai due punti REALI di "link
stabilito" in `run()` — non agli altri punti che ri-emettono
`AiChatReachablePeers` (replay UI su riconnessione, `Discovered`, `PeerGone`,
`Effect::Disconnect`: nessuno di questi è un nuovo link appena stabilito,
quindi non c'è nulla da riconciliare). Debounce fisso 2s, guardia di
generazione contro timer stantii dopo una riconnessione.

---

## [0.40.61] — 2026-07-29 — `ChatMsg::NoteUpdated` in arrivo: merge + fan-out (Blocco note)

Un `NoteUpdated` in arrivo si fonde nello store locale via `merge_note`,
aggiorna la UI locale, e viene ri-mandato a ogni altro link (mai al mittente)
— il fan-out che porta un aggiornamento arrivato da un client a TUTTI gli
altri client quando chi lo riceve è il server eletto.

---

## [0.40.60] — 2026-07-29 — `AiChatService` gestisce creazione/modifica/cancellazione locale (Blocco note)

`AiChatService` guadagna un `NotesStore` interno. I quattro `ServiceEvent` locali
(create/edit/edit-title/delete) applicano subito in memoria via `merge_note`,
emettono `Effect::SaveNotes` (I/O reale spostato in `perform`, `handle_event`
resta puro) e mandano `ChatMsg::NoteUpdated` a ogni peer attualmente linkato —
stessa via sia da client (verso il server) sia da server (già fan-out). `main.rs`
aggiornato per il nuovo 5° parametro di `AiChatService::new` (carica `notes.json`
all'avvio) — anticipa quello che era pianificato come Task 15, per non lasciare
il crate non compilabile fra un task e l'altro (lezione dai Task 5/6).

Fix di review: `Effect::SaveNotes` ora è un VERO gemello di `Effect::PersistMemory`
— `perform` clona `self.notes` (sincrono, economico) e spawna un task che scrive
`notes.json` sotto un lock dedicato (`notes_write_lock`), invece di bloccare
l'intero loop dell'attore (keepalive/elezione/relay) con uno `std::fs::write`
sincrono inline. `NotesStore` guadagna `#[derive(Clone)]` per permetterlo.

---

## [0.40.59] — 2026-07-29 — Placeholder `ClientMsg`/`ServerMsg` arms per Blocco note (Task 6)

Quattro placeholder no-op arm in `ws.rs` (match exhaustiveness su `ClientMsg`)
e due in `telegram/channel.rs` (match exhaustiveness su `ServerMsg`) per
`NoteCreate`/`NoteEdit`/`NoteEditTitle`/`NoteDelete` e
`NotesSnapshot`/`NoteUpserted` — rimpiazzate con real dispatch in Task 14.

---

## [0.40.58] — 2026-07-30 — `ChatMsg` peer↔peer per Blocco note (additiva)

Quattro nuove varianti (`NotesDigest`/`NotesDigestReply`/`NotesData`/
`NoteUpdated`) sul wire peer↔peer esistente (Contratto N) — nessuna modifica
alle varianti esistenti. Ancora non collegate a `AiChatService` (Task 7+).

---

## [0.40.57] — 2026-07-30 — `NotesStore` (persistenza `notes.json`)

Store delle note di proprietà dell'orchestrator, stesso pattern JSON-pretty di
`network.json`. `upsert_merged` fonde sempre via `merge_note` invece di
sovrascrivere — nessun percorso di scrittura bypassa il merge CRDT.

---

## [0.40.56] — 2026-07-30 — `render_body`/`NoteDigest`/`reconcile` (Blocco note)

Logica pura di riconciliazione: `digest_of` produce un'impronta senza testo,
`reconcile` confronta il proprio archivio coi digest del peer e decide
want/push. Copre esplicitamente il caso gemello di quello descritto
dall'utente in fase di brainstorm — una nota cancellata mentre offline, che
il peer crede ancora viva — con lo stesso scambio `(id, digest)`, nessun caso
speciale aggiuntivo.

---

## [0.40.55] — 2026-07-29 — modello `Note`/`Segment` + `merge_note` (Blocco note)

Nuovo modulo `notes`: merge CRDT per il "Blocco note" — unione di segmenti per
macchina (corpo), last-write-wins su orologio di parete (titolo), tombstone
monotono (cancellazione). Tre proprietà di merge (idempotenza, commutatività,
associatività) verificate esplicitamente nei test — garantiscono la
convergenza della rete indipendentemente dall'ordine di arrivo dei messaggi.
Design: `Docs/superpowers/specs/2026-07-29-library-notes-design.md` §3-4.

---

## [0.40.54] — 2026-07-29 — `aichat.json` rinominato `network.json` (migrazione automatica)

Il file di config della rete peer non si chiama più `aichat.json`: nome generico,
non deve sottintendere una sola funzionalità (prerequisito per "Blocco note",
design `Docs/superpowers/specs/2026-07-29-library-notes-design.md`). Migrazione
automatica e non distruttiva: se `network.json` manca ma `aichat.json` esiste,
viene letto e ricopiato — il vecchio file resta sul disco.

---

## [0.40.53] — 2026-07-29 — fix: chiudere la finestra AI Chat non deve azzerare il canale WS condiviso

**Causa radice reale** della bustina che non si accendeva mai per i messaggi
(0.45.30/0.45.31 non l'avevano risolto — il fix 0.45.31, emit-await su
`closeWindow()`, era un miglioramento genuino ma non era la causa).
Confermata con log incrociati JS+Rust dal vivo su 2 macchine, non ipotizzata:
`ClientMsg::AiChatClosed` (mandato quando l'utente chiude la finestra chat)
faceva `h.send(ServiceEvent::UiClosed)`, che azzera `self.server_tx` — il
canale WS **condiviso** usato da OGNI `Effect::ToUi` (messaggi, roster,
raggiungibilità per Condividi, gate 1/2 di ammissione, la bustina stessa,
tutto). Chiudere SOLO la finestra chat (una webview secondaria) veniva
trattato come se l'INTERA UI (`ui.exe`) si fosse disconnessa: per tutto il
tempo in cui la finestra chat restava chiusa, ogni evento AI Chat andava
perso in silenzio — nessun errore, nessun log visibile — finché la
finestra non veniva riaperta (`AiChatOpen` → `SetServerTx` ripristina il
canale).

**Fix:** `ClientMsg::AiChatClosed` non manda più `UiClosed` — diventa un
no-op esplicito e documentato. Il vero teardown della connessione WS (nel
blocco di disconnessione reale, non toccato) resta l'UNICO punto legittimo
che azzera `server_tx`. Lo stato "la finestra chat è aperta?" lo tiene già,
correttamente, il frontend (`aiChatGate` in `aichat-push.js`) — al backend
basta sapere se la connessione WS è viva, non quale sotto-finestra è aperta.

**Nessun test automatico** — stesso limite già accettato per questa classe
di codice in `ws.rs` (dispatch dentro la gestione connessione, richiede un
vero harness WS; vedi il commento su `connection_owns_plugin_sink` nello
stesso file per il precedente). Verificato dal vivo con log diagnostici
temporanei (rimossi in questo stesso commit) che hanno mostrato
`server_tx.is_some()=false` per l'intera durata della chat chiusa,
incluso il messaggio "hello" e le risposte AI — mai arrivati alla UI.
**Riverifica dal vivo richiesta** per confermare che il fix risolva
davvero (la diagnosi è certa, il fix non ancora testato dal vivo).

## [0.40.52] — 2026-07-28 — fix: gate 2 di ammissione (server) ora sopravvive alla finestra chiusa

Bug reale scoperto dal vivo usando Library "Condividi" (2 macchine): il server
non vedeva MAI la richiesta di ammissione di un candidato se la propria
finestra AI Chat era chiusa quando la richiesta arrivava — l'ammissione si
risolveva solo al timeout di 60s ("silenzio = sì"), mai per un voto
consapevole. Causa: `SetServerTx` ri-mostra il gate 1 (self come candidato,
`AiChatJoinPrompt`/`AiChatPending`) alla riapertura finestra, ma non il gate 2
(self come SERVER in attesa di votare, `AiChatAdmissionRequest`) — gap
esplicitamente lasciato aperto in un commento del 2026-07-04 ("da valutare in
una slice successiva"), mai chiuso finora. Fix: `SetServerTx` ora ri-emette
`AiChatAdmissionRequest` per ogni voto ancora aperto in `self.pending_votes`
su cui `self.me` non ha ancora votato (nessun nuovo `ServerMsg` — la variante
esiste già).

Test: `cargo test -p orchestrator --lib` — 786 passati (784 preesistenti + 2
nuovi: replay quando il voto è ancora aperto, NESSUN replay quando self ha
già votato ma il turno resta aperto in attesa di un altro presente),
nessuna regressione.

## [0.40.51] — 2026-07-28 — fix: Library "Condividi" non richiede più l'ammissione chat

Root cause (bug utente, verificato leggendo il codice): il dialog
"Condividi" leggeva `last_known_roster` (richiede il gate umano "vuoi
entrare?" alla stanza AI Chat), ma `request_share` non ha MAI consultato
quel roster — risolve da `self.peers ∩ self.links` (discovery UDP +
link TCP, entrambi automatici, nessun consenso umano). Nuovo
`reachable_peer_labels()` + `ServerMsg::AiChatReachablePeers` (protocol
0.14.7), emesso a ogni cambio di `peers`/`links` (5 siti: `Discovered`,
`PeerGone`, connessione in uscita riuscita, connessione in entrata
accettata, `Effect::Disconnect`) e ri-emesso incondizionatamente su
`SetServerTx`. Design:
`Docs/superpowers/specs/2026-07-28-library-share-reachable-peers-design.md`.

Aggiornato anche un match esaustivo pre-esistente su `ServerMsg` in
`telegram/channel.rs::format_response` (nuova variante additiva → nuovo
arm, no-op come le altre varianti AI Chat) e un test pre-esistente
(`set_server_tx_event_stores_sender_and_emits_only_self_label`, rinominato
`..._plus_empty_reachable_peers`) la cui asserzione di uguaglianza esatta
non era più vera col nuovo comportamento incondizionato.

Test: `cargo test -p orchestrator --lib` — 784 passati (777 preesistenti +
7 nuovi: helper puro ×3, emissione handle_event ×2, emissione perform ×1,
replay SetServerTx ×1), nessuna regressione. I due siti di cablaggio
dentro il loop async `run()` (nuovo link in entrata/uscita) non hanno test
automatico — richiederebbero un socket TCP reale, stesso limite degli
altri pochi test `#[ignore]` del modulo — verificati dal vivo.

## [0.40.50] — 2026-07-27 — feat: `title_suffix` opzionale nel contratto report Python (timestamp nel titolo finestra)

Richiesta post-verifica dal vivo di `screen_stocks`: data/ora anche nel
titolo della FINESTRA (= nome del salvataggio in Library — senza, due
screening salvati sarebbero indistinguibili), non solo nell'H1 del
documento. Il titolo finestra nasce dalla title_fn Rust del registro, che
non ha un orologio (e aggiungere chrono per un titolo sarebbe
sproporzionato) — mentre il tool Python il timestamp ce l'ha già
(`server.py` lo usa per l'H1 dallo 0.40.49... in realtà dal commit Python
f531ca1, stessa giornata).

**`PythonReportJson.title_suffix: Option<String>`** (`#[serde(default)]`,
`python_mcp_tool_client.rs`): se presente nel JSON del tool, `split_report`
lo APPENDE al risultato della title_fn — il titolo BASE resta del registro
Rust (autoritativo sul naming), il tool contribuisce solo la parte dinamica
che solo lui conosce. Assente (`stock_report`/`list_stocks`/qualunque tool
esistente): titolo invariato, retrocompatibilità totale. Lato Python,
`server.py::screen_stocks` manda `"title_suffix": " — GG/MM/AAAA HH:MM"`
con lo STESSO timestamp usato per l'H1 del documento (calcolato una volta:
documento e finestra non possono divergere a cavallo del minuto).

2 test nuovi in `split_report_tests` (suffisso appeso; assenza → titolo
invariato). `cargo test -p orchestrator --lib`: 777 passed, 0 failed.
`pytest` financial-markets: 125 passed.

## [0.40.49] — 2026-07-27 — feat: `screen_stocks` registrato nel canale `financial-markets`, timeout 120→180

Task 6/6 (ultimo) del piano `screen_stocks` (Docs/superpowers/plans/
2026-07-27-financial-markets-screen-stocks.md). Lato Python (Task 1-5, già
mergiati) `screen_stocks(top: int = 25) -> str` è già cablato in `server.py`
— questo task è solo il lato Rust che lo espone all'AI del canale.

Quarto `PythonToolSpec` nella voce `"financial-markets"` di
`EXTERNAL_TOOL_CHANNELS`, dopo `list_stocks`. `screen_stocks` usa
`defer_report_to_turn_end: true` (come `stock_report`, non come
`list_stocks`): il documento include la sezione '## Giudizio uso di massa'
scritta dall'AI — la finestra deve attendere la fine del turno per fondere
quel testo in coda, non può apparire subito al dispatch come un documento
puramente deterministico. Nuova `screen_stocks_title(_input)` — a
differenza di `stock_report_title`/`list_stocks_title` (titolo derivato da
un parametro della chiamata), qui il titolo è FISSO
(`"Financial Markets — Screening potenziale"`): `top` è solo un dettaglio
di presentazione, non identifica la finestra come `ticker`/`country`
fanno per gli altri due. La firma resta comunque `fn(&serde_json::Value) ->
String` per uniformità col resto del registro (parametro ignorato con `_`).

`call_timeout_secs` del canale alzato **120 → 180**: `screen_stocks` fetcha
fino a 60 ticker via `yfinance` (~1-2s l'uno) in una singola chiamata tool
— 120s era al limite. Il timeout è per-canale (non per-tool), quindi si
applica anche a `search_ticker`/`stock_report`/`list_stocks`, innocuo per
tutti e tre (nessuno si avvicina a quella soglia).

`FINANCIAL_MARKETS_SYSTEM_PROMPT` aggiornato per nominare i quattro tool
(era "tre strumenti", ora "quattro") e istruire l'AI: dopo `screen_stocks`
la risposta è ESCLUSIVAMENTE la sezione '## Giudizio uso di massa' — una
riga per titolo della selezione con archetipo d'uso (consumer diretto /
prodotto fisico / B2B / incorporato), verdetto Sì/No/Parziale sull'uso di
massa reale, motivazione breve, segnalazione esplicita dei falsi positivi
B2B, chiusa da un avviso che il giudizio è qualitativo e non una
raccomandazione operativa.

**File**: `crates/orchestrator/src/external_channel.rs`.

**Test**: sostituito `financial_markets_system_prompt_names_all_three_tools`
con `financial_markets_system_prompt_names_all_four_tools` (verifica anche
`screen_stocks` nel prompt); rinominato
`financial_markets_channel_exposes_three_tools` →
`financial_markets_channel_exposes_four_tools` (stesso `match`
venv-tolerante, conteggio `4` + presenza di `screen_stocks`); nuovo
`screen_stocks_title_is_fixed` (titolo identico con e senza `top`
nell'input).

**Verifica**: RED confermato (`cargo test -p orchestrator --lib
financial_markets` — errore di compilazione, `screen_stocks_title` non
trovata) prima dell'implementazione. GREEN dopo: `cargo build -p
orchestrator` pulito; `cargo test -p orchestrator --lib` — 775 passed, 0
failed, 7 ignored, **con `LARE_PYTOOLS_DIR` impostata O NO** (verificato dal
vivo in entrambi i casi; con la var impostata e il venv reale presente, il
ramo `Ok` di `financial_markets_channel_exposes_four_tools` è
effettivamente esercitato — `tool_defs().len() == 4` verificato per davvero,
non solo tollerato). `cargo clippy -p orchestrator --all-targets`: 0
warning in `external_channel.rs` (i warning residui nel crate sono
pre-esistenti, fuori scope, in `aichat/service.rs`). `cargo fmt --check`:
stessa non-conformità pre-esistente del file (righe lunghe con campi
inline) già segnalata in 0.40.47/0.40.48, non una regressione introdotta
qui.

Verifica dal vivo (manuale, non automatizzata) rimane il passo finale del
piano — vedi §"After all tasks: live verification" nel task brief e
`Docs/HANDOFF.md`.

## [0.40.48] — 2026-07-24 — fix: `BRK-B` (non `BRK.B`), normalizzazione `country`, test sort rinforzato

Trovati nella review finale whole-branch del piano `list_stocks` (dopo i 4
task), non in un task specifico.

**1 (pytools) — `SP500_TICKERS` conteneva `"BRK.B"` (punto), la cache reale
usa `"BRK-B"` (trattino).** `tickers_us.json` è sourced verbatim dal
`company_tickers.json` di SEC, che spella OGNI azione multi-classe con un
trattino (`BRK-B`, `BF-B`, `MOG-A`, mai un punto) — la Task 2 aveva scritto
`"BRK.B"` nella lista curata, e il proprio spot-check (Task 2 Step 5)
verificava `indices_for("BRK.B")` (la spelling SBAGLIATA, quella del set
stesso) invece della spelling reale della cache — un guard che testava il
lato sbagliato del join, mai in grado di scoprire il bug. Risultato:
Berkshire Hathaway Class B, una delle prime 10 aziende dell'S&P 500,
mostrava sempre `-` nella colonna Indice. Corretto in `index_membership.py`
(`"BRK.B"` → `"BRK-B"`), più un nuovo test che verifica ESPLICITAMENTE che
la spelling con il punto NON faccia match (`indices_for("BRK.B") == "-"`) —
così una futura regressione a quella spelling verrebbe scoperta subito,
invece di ripetere lo stesso errore di guardia.

**2 (pytools + orchestrator) — `country` non normalizzato per la
visualizzazione.** `is_country_supported` valida case-insensitive
(`country.upper() in SUPPORTED_COUNTRIES`), ma il valore grezzo passava
non normalizzato nella colonna Paese di ogni riga (`stock_list.py::
build_rows`) e nel titolo del documento/finestra (`server.py`'s
`full_markdown`, `external_channel.rs::list_stocks_title`) — un
`list_stocks(country="usa")` avrebbe mostrato "usa" ovunque invece di
"USA". Corretto in tutti e tre i punti.

**3 (pytools) — `test_build_rows_sorts_alphabetically_by_name` non
distingueva un ordinamento per nome da uno per ticker.** I due ticker di
prova (`ZZZ`/"Zeta Corp", `AAA`/"Alpha Corp") avevano ordine-ticker e
ordine-nome coincidenti — un regresso a "ordina per ticker" sarebbe passato
comunque. Sostituiti con `AAA`/"Zeta Corp" e `ZZZ`/"Alpha Corp" (ordini
deliberatamente divergenti), che ora fallirebbero davvero su quel regresso.

**Verifica**: `pytest` (venv `financial-markets`): 80 passed, 0 failed.
`cargo test -p orchestrator --lib`: 774 passed, 0 failed, 7 ignored.

## [0.40.47] — 2026-07-24 — feat: `list_stocks` registrato nel canale `financial-markets`

Task 4/4 (ultimo) del piano `list_stocks`. Aggiunge il terzo `PythonToolSpec`
alla voce `"financial-markets"` in `EXTERNAL_TOOL_CHANNELS`, accanto a
`search_ticker` e `stock_report` (invariati). Lato Python (Task 2/3, già
mergiati) `list_stocks(country: str = "USA") -> str` è già cablato in
`server.py` — questo task è solo il lato Rust che lo espone all'AI.

`list_stocks` usa `defer_report_to_turn_end: false` (a differenza di
`stock_report: true`): il suo documento — una tabella Nome/Asset/Paese/
Indice — è interamente deterministico, nessuna narrativa AI da attendere,
quindi la finestra Markdown si apre SUBITO al dispatch, come `nmap`. Nuova
`list_stocks_title(input)` deriva il titolo da `input.country` (default
`"USA"` se assente) — stesso schema di `stock_report_title` (titolo
dall'INPUT della chiamata, mai dalla risposta). Su un paese non supportato
`list_stocks` ritorna una stringa semplice (non JSON): `split_report()`
(Task 1) la instrada già correttamente nel percorso "non-JSON" esistente —
nessun nuovo codice Rust richiesto per quel fallback.

`FINANCIAL_MARKETS_SYSTEM_PROMPT` aggiornato per nominare i tre tool e
istruire l'AI a NON ripetere la tabella nella propria risposta dopo
`list_stocks` (solo una conferma breve del conteggio) — parallelo al
comportamento già esistente per `stock_report`.

**File**: `crates/orchestrator/src/external_channel.rs`.

**Test**: sostituito `financial_markets_system_prompt_names_both_tools` con
`financial_markets_system_prompt_names_all_three_tools` (verifica anche
`list_stocks` nel prompt) e aggiunto
`financial_markets_channel_exposes_three_tools` (`resolve_channel_tools`
sul canale reale + `tool_defs().len() == 3` quando il venv esiste).

**Fix di review same-day (dettagli in 0.40.48 sopra)**: la prima versione
di questo test faceva `.unwrap()` sul risultato — panic garantito su
`cargo test -p orchestrator --lib` su qualunque macchina senza il venv
`scripts/pytools/financial-markets` creato a mano, in contraddizione con il
precedente già esistente nello stesso file
(`financial_markets_channel_tool_client_factory_does_not_panic`, che
tollera `Ok`/`Err` per lo stesso identico motivo). Corretto a un `match`
che tollera `Err` (nessuna asserzione possibile senza client reale, ma mai
un panic) e verifica `tool_defs().len() == 3` solo nel ramo `Ok`. `cargo
test -p orchestrator --lib`: 774 passed, 0 failed, 7 ignored, **con
`LARE_PYTOOLS_DIR` impostata O NO** — verificato dal vivo in entrambi i
casi. `cargo build -p orchestrator` pulito. `cargo clippy -p orchestrator
--all-targets`: 0 warning nel file toccato (i warning residui nel crate
sono pre-esistenti, fuori scope, in `ws.rs`/`aichat/service.rs`).

## [0.40.46] — 2026-07-24 — feat: `PythonToolSpec.defer_report_to_turn_end` — apertura finestra configurabile per-tool

Task 1/4 del piano `list_stocks`. Prima di questo cambio, `split_report()`
hardcodava `defer_to_turn_end: true` incondizionatamente per QUALSIASI tool
con `report_title` — corretto finché `stock_report` era l'unico tool del
genere (il suo documento include narrativa AI, deve attendere il testo
finale del turno), ma `list_stocks` (Task 4) produce un documento
interamente deterministico e deve aprire la finestra SUBITO al dispatch,
come nmap — non c'era modo di esprimere la differenza per-tool.

Aggiunto `PythonToolSpec.defer_report_to_turn_end: bool` — letto da
`split_report()` SOLO quando `report_title` è `Some` (ignorato altrimenti,
nessun report in nessun caso). `split_report()` ora prende questo bool come
parametro esplicito invece di hardcodare `true`; il chiamante in
`dispatch()` lo passa da `spec.defer_report_to_turn_end`. Aggiornati i 3
`PythonToolSpec` literal di produzione in `external_channel.rs`
(`pyping`/`search_ticker` → `false`, nessuno dei due produce un report
comunque essendo `report_title: None`; `stock_report` → `true`, preserva il
comportamento esistente) e i 3 `#[cfg(test)]` literal `#[ignore]`
real-network in `python_mcp_tool_client.rs` (stesso schema). Nessun cambio
di comportamento osservabile per i tool esistenti — `stock_report` continua
a posporre l'apertura, `pyping`/`search_ticker` continuano a non aprire mai
una finestra (nessun `report_title`).

**File**: `crates/orchestrator/src/python_mcp_tool_client.rs`,
`crates/orchestrator/src/external_channel.rs`.

**Test**: `cargo test -p orchestrator --lib` — 771 passed, 0 failed, 7
ignored (774 totali con `split_report_tests`, di cui 1 nuovo:
`defer_to_turn_end_false_produces_immediate_open_report`, il primo caso a
esercitare `defer_to_turn_end: false` su un report prodotto). `cargo build
-p orchestrator` pulito. `cargo clippy -p orchestrator --all-targets`: 0
warning nei 2 file toccati (i warning esistenti nel crate sono
pre-esistenti, in `ai_adapter.rs`/`ws.rs`/`aichat/service.rs`, non toccati
da questo task).

## [0.40.45] — 2026-07-24 — chore: `pytools/` → `scripts/pytools/`, financial-markets segue la migrazione già in corso su `main`

Al momento del merge di questo branch, `main` aveva già in corso (non ancora
committata) una migrazione `pytools/` → `scripts/pytools/` — `python-ping`
era già stato spostato, `financial-markets` (esistente solo su questo
branch) no. Spostato `pytools/financial-markets/` → `scripts/pytools/
financial-markets/` (`git mv`, storia preservata) per restare coerenti con
la nuova convenzione. Aggiornati: `.gitignore` (pattern venv/__pycache__/
tickers_us.json), i 3 percorsi `repo_root.join("pytools")` nei test
`#[ignore]` real-* (ora `repo_root.join("scripts").join("pytools")`),
`CLAUDE.md` (riga `LARE_PYTOOLS_DIR` + riferimento a `scripts/pytools/
README.md`), e ogni riferimento testuale a `pytools/README.md`/`pytools/
financial-markets/` dentro `python_mcp_tool_client.rs` (messaggi d'errore,
doc comment, motivazioni `#[ignore]`). **Non toccato** (fuori scope,
segnalato all'utente): `deploy_binary_only.ps1` copia ancora da `pytools\`
— va aggiornato separatamente per restare corretto con questa migrazione.
Il fallback runtime `parent.join("pytools")` in `resolve()` (usato quando
`LARE_PYTOOLS_DIR` non è impostata, tipicamente in un binario deployato) è
**rimasto invariato di proposito**: è il nome della cartella accanto
all'eseguibile a destinazione, indipendente da dove vive il codice
sorgente nel repo. Verificato dal vivo dopo lo spostamento: entrambi i
test `#[ignore]` `real_search_ticker_multi_match_roundtrip_via_venv` e
`real_stock_report_roundtrip_via_venv` passano contro il venv rilocato +
rete reale.

## [0.40.44] — 2026-07-23 — feat: la finestra `stock_report` porta le sezioni 5-6 dell'AI; pannello ridotto a un riassunto numerico

Seconda correzione dal vivo dopo 0.40.43 (la prima ha introdotto la
finestra dedicata, ma con solo le 4 sezioni fattuali): l'utente ha
osservato che il pannello del canale mostrava per intero la risposta
testuale dell'AI (narrativa di trend + ipotesi di investimento) — troppo
lunga — e che quelle stesse due sezioni non finivano MAI nel documento
salvabile, solo nel pannello effimero. Richiesta: la finestra deve portare
TUTTE e 6 le sezioni; il pannello deve mostrare solo un riassunto numerico
(≤5 righe), mai la prosa dell'AI.

**Il problema architetturale**: al momento del `dispatch()` del tool
(quando prima si apriva la finestra), il testo dell'AI per le sezioni 5-6
non esiste ancora — arriva in un turno successivo, dopo che l'AI ha letto
il riassunto fattuale. L'apertura non può più essere immediata per questo
tool specifico.

**`ChannelReport::defer_to_turn_end: bool`** (nuovo campo, `tool_client.rs`)
— `false` per nmap (comportamento storico, invariato: il documento è già
completo al dispatch), `true` per `stock_report` (`split_report()` in
`python_mcp_tool_client.rs` lo imposta sempre). **`DispatchOutcome.
channel_summary: Option<String>`** (nuovo campo) — un riassunto breve,
deterministico, mostrato nel pannello SUBITO dopo il dispatch,
indipendentemente da cosa scriverà poi l'AI. Il contratto JSON Python
(`PythonReportJson`) guadagna il terzo campo `channel_summary`.

**`ai_adapter.rs`'s `respond()`** (il loop di tool-use condiviso da OGNI
canale — cambiato con cautela, dietro il flag `defer_to_turn_end`): quando
un report è `defer_to_turn_end: true`, la finestra non si apre più subito
— viene tenuta in sospeso (`pending_report`, un `std::sync::Mutex` locale
alla chiamata: `RefCell` non basta, `on_text` deve restare `Send`) e
`on_text` smette di inviare i delta di quel turno al pannello (li scarta:
il testo finale si legge da `turn.blocks`, non serve bufferizzarlo due
volte). Alla risposta finale del turno (nessun altro tool_use), il testo
dell'AI viene fuso in coda al documento fattuale e la finestra si apre
SOLO allora — sempre PRIMA del `Done` del turno, su ogni via d'uscita
tranne l'annullamento utente (Stop): lì il report in sospeso viene
scartato senza aprire nulla, per rispettare la scelta di annullare. Un
errore del backend/un rifiuto DOPO che il tool ha già prodotto un
documento reale non lo fa sparire (il documento fattuale si apre comunque,
senza testo AI) — regressione rispetto al comportamento immediato di
prima, altrimenti. 3 nuovi test in `ai_adapter.rs` coprono il percorso
d'oro (riassunto nel pannello, testo AI fuso nella finestra, mai nel
pannello, finestra prima del Done), l'errore-dopo-il-tool, e
l'annullamento-prima-del-testo-finale (deterministico: il fixture di test
cancella il token DENTRO `dispatch()`, non da un task separato — nessuna
corsa).

`FINANCIAL_MARKETS_SYSTEM_PROMPT` riscritto: la risposta dell'AI è ora
Markdown vero (intestazioni `##`, come nel resto del documento — continua
il documento, non è più testo semplice per un pannello) e non ha più il
vincolo "poche righe" (quel vincolo si applica ora al riassunto
deterministico, generato da `report.py`, non alla risposta dell'AI).

**Lato Python** (`pytools/financial-markets/`, refinement richiesto nella
stessa sessione, insieme al punto sopra):
- `number_format.py` (nuovo — rinominato da un primo tentativo `numbers.py`
  che oscurava il modulo stdlib omonimo, rompendo `decimal`/pytest a
  runtime) — `format_number`/`format_abbreviated`, formato it-IT (punto
  migliaia, virgola decimali), scala K/M/B/T dinamica. Bug segnalato
  dall'utente: `Market cap: 4786462654464` illeggibile, e l'esempio di
  scala che aveva proposto ("4786,46B") era esso stesso sbagliato — il
  valore è ~4,79 **trilioni**, non miliardi; la scala va sempre scelta
  dinamicamente in base alla grandezza, mai fissata a priori.
- `option_chain.py` (nuovo) — tabella option chain vera: accoppia call/put
  per strike, ritaglia a 10 strike sopra e 10 sotto quello più vicino al
  prezzo attuale (non più le prime 3 scadenze intere). Colonne, ordine
  confermato dall'utente: Call IV/Bid/Ask/Ultimo | Strike | Put
  IV/Bid/Ask/Ultimo.
- `charts.py` — stile scuro (`_dark_style()`, mplfinance) allineato al
  tema dell'app (`crates/ui/frontend/config.css`: sfondo
  `rgba(20,24,36,...)`, testo `#e8eefc`) invece dello sfondo bianco di
  default. Verificato con un test che legge un pixel d'angolo del PNG
  risultante (richiede Pillow, già presente nel venv).
- `report.py` — `build_report` ritorna ora `(full_markdown,
  channel_summary)` invece di solo la stringa; `build_channel_summary`
  deriva le sue righe dagli stessi numeri delle sezioni Fondamentale/
  Comparative (mai un dato "nuovo"). Comparative riscritta con prosa
  esplicativa (cosa significano market cap/P/E/variazione) e numeri
  formattati con `number_format`.
- `data_sources/__init__.py` — `Fundamentals.current_price` (nuovo, serve
  sia al Fondamentale sia a centrare l'option chain sull'ATM);
  `OptionContract` guadagna `bid`/`ask`/`last_price`.
- `data_sources/yfinance_source.py` — `fetch_options` ora prende SOLO la
  scadenza più vicina (`expiries[0]`, non le prime 3) con bid/ask/last
  price; `fetch_fundamentals` popola `current_price`.

## [0.40.43] — 2026-07-23 — feat: `stock_report` apre una finestra Markdown dedicata (report deterministico)

Corretto in brainstorming dal vivo dopo il primo smoke test manuale con la
UI: il canale `/markets` mostrava l'intero report (fondamentale + 5 grafici
+ option chain + comparative) come testo semplice nel proprio pannello,
illeggibile per un documento con immagini/tabelle — coerenza con il resto
del programma (nmap apre una finestra Markdown dedicata + Salva in
Library) richiedeva lo stesso meccanismo qui.

`PythonMcpToolClient` generalizzato una terza volta: ogni tool iniettato
(`PythonToolSpec`) porta ora un `report_title: Option<fn(&Value) -> String>`
opzionale — se presente E la risposta del tool è un JSON `{"summary":...,
"report_markdown":...}`, `dispatch()` (via la nuova `split_report()`, pura
e testata senza spawnare nulla) separa `output=summary` (all'AI, per
ragionare) da `report=Some(ChannelReport{title, markdown: report_
markdown})` (finestra deterministica + Salva in Library — stesso
meccanismo di `nmap_tool_client.rs::call_scan_tool`, mai una scelta
dell'AI). Un tool senza `report_title` (`pyping`, `search_ticker`) o la
cui risposta non è quel JSON (qualunque tool preesistente) resta testo
semplice — nessun cambio di comportamento per loro.

`stock_report` (Python) ora ritorna quel JSON: `report_markdown` è il
documento completo (con i grafici), `summary` è lo stesso testo senza le
immagini (`report.strip_chart_images`, nuovo) — l'AI ragiona sui dati
fattuali per scrivere le sezioni 5-6, ma non li ripete mai (il documento
si apre da solo). `FINANCIAL_MARKETS_SYSTEM_PROMPT` riscritto di
conseguenza: la risposta dell'AI nel canale è ESCLUSIVAMENTE le sezioni
5-6, testo semplice senza intestazioni Markdown (il pannello del canale
non renderizza Markdown).

## [0.40.42] — 2026-07-23 — fix: blocchi MCP multipli troncati + crash su dati reali (financial-markets)

Due bug trovati dal vivo interrogando il canale `financial-markets` appena
costruito (0.40.41) con dati reali (SEC.gov, Yahoo Finance), prima dello
smoke test manuale con la UI — nessun test a fixture li aveva esercitati.

**1 (orchestrator) — `PythonMcpToolClient` non troncava più blocchi di
contenuto MCP.** `dispatch()` prendeva solo il PRIMO blocco di testo di un
`CallToolResult` (`find_map`). FastMCP serializza un `list[dict]` ritornato
da un tool Python come UN blocco per elemento — `search_ticker` con più
corrispondenze (es. "Novo Nordisk" → NVO + NONOF) faceva arrivare all'AI
solo la prima, vanificando silenziosamente l'istruzione del system prompt
"se risultati multipli ambigui, chiedi all'utente quale". Estratta
`join_text_content()` (pura, testata anche senza spawnare un processo reale
grazie a `CallToolResult::success()`) che concatena TUTTI i blocchi con
`\n`; per un tool a singolo blocco (`pyping`, `stock_report`) il
comportamento è bit-per-bit identico a prima.

**2 (pytools/financial-markets) — `charts.py` crashava l'intero report su
dati OHLC intraday reali.** `bars_to_dataframe` chiamava
`pd.to_datetime(df["date"])` senza `utc=True`: le barre H1/H4 reali (mai
esercitate dai test, che usano solo `FakeDataSource`) arrivano da yfinance
con offset UTC diversi tra loro quando la finestra di 60gg attraversa il
cambio ora legale — pandas rifiuta di costruire un `DatetimeIndex` con
offset misti (`"Mixed timezones detected"`), facendo fallire l'intero
`stock_report`, non solo la sezione grafici (violava il contratto "mai
un'eccezione" della spec §4). Fix in due passi: `utc=True` risolve
l'ambiguità di parsing, poi `.dt.tz_convert("America/New_York")` riporta i
timestamp all'ora locale del mercato USA — senza questo secondo passo i
grafici H1/H4 mostrerebbero l'ora UTC invece dell'apertura/chiusura reale,
un errore silenzioso ma visibile sull'asse del grafico usato per l'analisi
tecnica di breve termine (spec §6). Verificato dal vivo che le barre
D/W/M/H4/H1 reali sono già espresse in America/New_York da yfinance, quindi
il giro andata-ritorno per UTC è un no-op per i timeframe non-intraday.

Due nuovi test `#[ignore]` (spawn reale contro il venv di
`financial-markets`) fissano entrambe le regressioni a livello di
integrazione vera: `real_search_ticker_multi_match_roundtrip_via_venv`
(NVO+NONOF entrambi presenti) e `real_stock_report_roundtrip_via_venv`
(report completo su AAPL non crasha).

## [0.40.41] — 2026-07-22 — feat: canale financial-markets — `/markets`

Primo tool Python reale del dominio `financial-markets` (sub-progetto 2 di
pytools, spec `Docs/superpowers/specs/2026-07-22-financial-markets-stock-
report-design.md`): due tool MCP, `search_ticker` (risolve nome societario→
ticker via cache SEC) e `stock_report` (report fattuale: fondamentale,
grafici 5 timeframe, option chain, comparative). Nuova voce in
`EXTERNAL_TOOL_CHANNELS`, riusa `PythonMcpToolClient` generalizzato in
0.40.40 — zero altre modifiche all'infrastruttura condivisa.

## [0.40.40] — 2026-07-22 — refactor: `PythonMcpToolClient` supporta più tool per canale

`tool_defs()`/`dispatch()` erano hardcoded sul literal `"pyping"` nonostante
il modulo si dichiarasse generico — gap emerso progettando `financial-
markets` (due tool: `search_ticker`+`stock_report`). `resolve()` ora riceve
`tool_defs: Vec<ToolDef>` e `call_timeout_secs: u64` dal chiamante (la voce
di registro del canale, come già fa `nmap_tool_client.rs` per i propri sette
tool). Zero cambio di comportamento per `python-ping`.

## [0.40.39] — 2026-07-22 — fix: `LARE_PYTOOLS_DIR` è la radice `pytools/`, `domain_id` sempre unito

Fix di review sull'intero branch pytools-infra (3 finding, `python_mcp_tool_client.rs`).

**Fix 1 (il motivo di questa release) — `resolve()` era asimmetrico.** Senza
la variabile d'ambiente, la radice era `<exe_dir>/pytools/<domain_id>/`; CON
la variabile impostata, il suo valore veniva preso COME la cartella
dell'ambito stessa, senza unire `domain_id` una seconda volta. Due
conseguenze reali: (1) il ramo di default non risolve mai — nulla copia
`pytools/` accanto all'eseguibile compilato (niente `build.rs`, nessuna
risorsa Tauri) — quindi la variabile era obbligatoria de-facto pur essendo
documentata come override opzionale; (2) `LARE_PYTOOLS_DIR` era di fatto
cablata sull'unico canale esistente (`python-ping`) mentre il client è
deliberatamente generico — il prossimo dominio (dati finanziari, la ragione
dichiarata di questa infrastruttura) avrebbe richiesto una seconda variabile
o avrebbe silenziosamente risolto nella cartella di `python-ping`.

Ora `<root>` significa SEMPRE la radice `pytools/`, in entrambi i rami, e
`domain_id` viene unito sopra incondizionatamente: variabile impostata →
`<valore>/<domain_id>/…`; variabile assente → `<exe_dir>/pytools/<domain_id>/…`.
Nessun cambiamento sotto quel punto (interprete/script invariati). RED
genuino confermato aggiornando prima i 3 test esistenti di `resolve()` (e il
test `#[ignore]` `real_pyping_roundtrip_via_venv`) a puntare la variabile
alla radice invece che alla cartella dell'ambito — 2 dei 3 test falliscono
contro il codice vecchio, poi tornano verdi dopo il fix di produzione.

Aggiornati anche `pytools/README.md` (una formula sola, simmetrica, invece
della sezione che documentava l'asimmetria — introdotta in b169a65 per
chiarire il comportamento vecchio, ora superata dal fix — + nota esplicita
che nel workflow di sviluppo normale `LARE_PYTOOLS_DIR` va sempre impostata,
con l'one-liner PowerShell) e `CLAUDE.md` (riga `LARE_PYTOOLS_DIR` nella
tabella delle variabili d'ambiente, da "override opzionale" a "radice
richiesta in dev").

**Fix 2 — documentazione dell'invariante di concorrenza, portata dal
sibling.** `NmapToolClient` porta un doc comment che spiega perché i suoi
due `Mutex` (`peer`, `child_pid`) sono race-free e perché il lock non resta
mai attivo durante la chiamata al tool; `PythonMcpToolClient` aveva la
stessa struttura e lo stesso invariante ma nessun commento. Portato
(adattato: qui niente tracking multi-tool — un solo tool, `pyping` — e kill
semplice invece di kill-tree, coerente con quanto già detto nel modulo doc
in testa al file). Aggiunti anche doc comment per campo sui 5 campi dello
struct, e i brevi commenti "perché" su `reset_session`/`run_in_session`/
`open_target` che il sibling nmap ha (il punto: questo canale non ha
strutturalmente una via shell). Nessun cambio di comportamento.

**Fix 3 — `shutdown_kills_the_connected_process` testava solo metà
dell'invariante.** Asseriva solo che il PID fosse stato ucciso, non che
`peer`/`child_pid` fossero stati azzerati — che è l'invariante reale
"niente doppio kill" garantito da `Option::take()`. Aggiunte le due
asserzioni mancanti, mirror di `NmapToolClient::close_connection_kills_
the_stored_pid_and_clears_state`.

Verifica: `cargo test -p orchestrator` invariato a 757 lib passed/0
failed/5 ignored; `cargo clippy -p orchestrator --all-targets` invariato ai
4 warning pre-esistenti (nessuno nuovo).

## [0.40.38] — 2026-07-21 — Canale `"python-ping"` registrato in `EXTERNAL_TOOL_CHANNELS` (Task 2/4)

Terza voce del registro `EXTERNAL_TOOL_CHANNELS` (`external_channel.rs`), dopo
`"nmap"` e `"library-expand"`: `id: "python-ping"`, `slash_trigger: "/pyping"`,
`window_title: "Lare — Python ping"`. La sua `tool_client` factory chiama
`PythonMcpToolClient::resolve("python-ping", "server.py", "LARE_PYTOOLS_DIR")`
(Task 1, 0.40.37) — a differenza di `NmapToolClient::resolve()`/
`EmptyToolClient`, questa factory PUÒ legittimamente fallire (`Err`) finché il
venv Python non esiste ancora sulla macchina (arriva nel Task 4): non è un
bug, è lo stato atteso oggi.

Nuovo `PYTHON_PING_SYSTEM_PROMPT`: dice all'AI di questo canale che ha
ESATTAMENTE un tool, `pyping`, e che il suo compito è chiamarlo con il
messaggio dell'utente e riportarne l'eco, senza commenti aggiuntivi.

3 nuovi test in `external_channel.rs` (`production_registry_has_the_
python_ping_channel`, `python_ping_system_prompt_constant_is_set`,
`python_ping_channel_tool_client_factory_does_not_panic`) + aggiornato
`production_registry_has_the_library_expand_channel` (registro ora a 3 voci,
non più 2). Il test sul system prompt legge il campo statico
`system_prompt_override` direttamente, MAI tramite `resolve_channel_tools`
(che invocherebbe la factory fallibile) — coerente con la nota sopra. Il test
sulla factory tollera sia `Ok` che `Err`, verifica solo l'assenza di panic.

Zero modifiche a `ws.rs`/protocollo — `resolve_channel_tools` era già
generico su qualunque voce del registro (dimostrato per la seconda volta di
fila, dopo `"library-expand"` in 0.40.35).

## [0.40.37] — 2026-07-21 — `PythonMcpToolClient` — infrastruttura tool esterni Python (Task 1/4)

Nuovo `crates/orchestrator/src/python_mcp_tool_client.rs`, mirror strutturale di
`NmapToolClient` ma per un server MCP scritto in **Python** invece di un binario
nativo Rust (Docs/superpowers/specs/2026-07-21-pytools-infrastructure-design.md).
`PythonMcpToolClient` è GENERICO: riceve `domain_id`/`script_relpath`/
`env_override` da chi lo costruisce, non conosce alcun dominio specifico.

`resolve(domain_id, script_relpath, env_override)`: risolve la radice
(`env_override` se impostata, altrimenti `pytools/<domain_id>/` sibling di
`current_exe()`) e l'interprete Python del venv (`venv/Scripts/python.exe` su
Windows, `venv/bin/python3` su Unix). Verifica che interprete E script esistano
PRIMA di spawnare — mancante → `Err` leggibile che nomina l'ambito e il file
mancante, mai un panic. Nessun provisioning automatico del venv (scelta di
design: l'utente lo crea a mano).

`ensure_connected()`/`close_connection()` mirror-ano esattamente l'omonimo di
`NmapToolClient`: spawn lazy + handshake MCP via `rmcp::transport::
TokioChildProcess` al primo uso, peer riusato alle chiamate successive; il
processo Python viene ucciso per PID (`ProcessKiller`, seam testabile via
`FakeProcessKiller`) su timeout o su `shutdown()` (teardown della connessione
WS) — stesso motivo del fix nmap 0.40.30/0.40.31 (altrimenti resterebbe orfano
indefinitamente). A differenza di `NmapToolClient::ProcessTreeKiller`, qui
niente kill-tree (`/T`): lo script Python non spawna figli elevati via UAC come
`nmap_os_detect`.

`impl ToolClient`: `run_in_session`/`open_target`/`reset_session` sono stub
("non disponibile su questo canale") — chiude strutturalmente le vie
`Route::Os`/`Route::Slash` per questo canale, come ogni altro `ToolClient` di
canale esterno. `tool_defs()` espone un solo tool, `pyping` (`message: string`,
tool di prova — mirror di `plugin-ping` per il sistema plugin). `dispatch()`
instrada `pyping` al server Python via `peer.call_tool(...)` sotto un timeout
di 60s (`PYTHON_TOOL_CALL_TIMEOUT_SECS`, più basso dei 900s di nmap: oggi copre
solo un'eco istantanea, non generalizzato oltre finché non esiste un caso
reale che lo richieda — YAGNI); un nome fuori menu è rifiutato come "tool
sconosciuto sul canale python-ping".

9 unit test (path-building del resolver + i 6 comportamenti storici di
`ToolClient`) + 1 test `#[ignore]` di round-trip reale contro
`pytools/python-ping/venv` (non ancora creato — Task 3).

**Non ancora collegato a `EXTERNAL_TOOL_CHANNELS`** (`external_channel.rs`, che
resta invariato) — la registrazione del canale `"python-ping"` è il prossimo
task (2/4).

## [0.40.36] — 2026-07-20 — fix: `Done{exit_code: Some(1)}` sui fallimenti del turno AI (data-loss "Espandi")

**Critical** trovato nella review finale whole-branch: `LlmAdapter::respond` segnalava
OGNI esito — successo E fallimento — con `ServerMsg::Done{exit_code: None}`. Tre
percorsi di errore emettevano un Chunk placeholder e poi lo stesso `Done` "pulito":
errore backend/API (`Err(e)` da `send_turn`), rifiuto dell'AI (`TurnStop::Refused`),
risposta degenere (né testo né tool_use). Il consumatore `crates/ui/frontend/
window.js`'s `runExpand()` (canale `library-expand`, v0.40.35) tratta qualunque
buffer non vuoto arrivato con `done` come successo e chiama `archive_update` —
sovrascrivendo silenziosamente il documento Library dell'utente col testo
d'errore dell'orchestrator (il placeholder stesso, non vuoto, superava il
controllo esistente `isEmptyExpandResult`).

Fix (valore-only, nessun cambio di schema del protocollo): i tre percorsi ora
emettono `Done{exit_code: Some(1)}` tramite una nuova closure `fail()`, mirror
di `done()` — precedente già in questa funzione (la cancellazione emette
`Done{exit_code: Some(130)}` per lo stesso motivo). Il ramo `MaxTokens` e il
fallthrough di completamento normale restano `exit_code: None` (contengono
testo vero e salvabile). 3 test esistenti (`error_path_returns_visible_error`,
`refusal_path_is_clear`, `empty_assistant_turn_not_pushed_keeps_history_valid`)
estesi con l'assert sul nuovo `exit_code`.

Minor deferiti dalle review dei Task 2/3: rename
`production_registry_has_exactly_the_nmap_channel` →
`production_registry_starts_with_nmap_channel` (`external_channel.rs`, il nome
non rifletteva più il body dopo l'aggiunta del canale `library-expand`); merge
di due `use crate::tool_client::...` separati da un import estraneo; doc del
campo `slash_trigger` arricchita per menzionare l'inerzia di un canale senza
innesco da cursore (es. `"library-expand"`).

---

## [0.40.35] — 2026-07-20 — feat: canale `library-expand` — turno AI isolato per espandere un documento Library

Seconda voce in `EXTERNAL_TOOL_CHANNELS` (dopo `/nmap`) — nessuna modifica al
protocollo, a `ws.rs`, o al router: il meccanismo canale esterno era già
generico su qualunque voce di registro. `EmptyToolClient` (nessun tool
custom, mirror strutturale di `NmapToolClient` ma senza stato/processo) +
`LIBRARY_EXPAND_SYSTEM_PROMPT` (fisso — il documento e la richiesta viaggiano
nel messaggio utente del turno, non nel system prompt, che è `&'static str`
e non può contenerli). L'AI di questo canale ha SOLO `web_search`/
`web_fetch` se il toggle utente è attivo (aggiunti da `agent::tools_for`
indipendentemente dal `ToolClient`) — niente shell, niente apertura app,
niente `show_markdown`. Nessuno slash trigger collegato in UI: la finestra
si apre solo dal pulsante "Espandi" di un documento già aperto (Task 4).
6 test nuovi. Design: `Docs/superpowers/specs/2026-07-20-library-expand-
design.md`.

---

## [0.40.34] — 2026-07-20 — fix: system prompt prometteva `web_search` anche a backend che non lo eseguono

Bug osservato dal vivo dall'utente: toggle "Ricerca web" ON + DeepSeek via
OpenRouter → l'AI invoca `[tool web_search]`, riceve `tool sconosciuto:
web_search`, e fabbrica una scusa plausibile ("non ho accesso diretto a
strumenti di ricerca web"). Causa: `agent::system_prompt` appende
l'addendum web (`WEB_SEARCH_ADDENDUM`) e `agent::tools_for` aggiunge i
`ToolSpec::Server` in base al SOLO toggle utente, senza sapere se il
backend attivo può davvero eseguire il tool server-side Anthropic.
`OpenRouterBackend::to_or_tools` scarta già `ToolSpec::Server` in
traduzione (comportamento corretto, documentato) — ma il system prompt
non lo sapeva, e continuava a promettere la capacità.

- **`ChatBackend::supports_web_search() -> bool`** (`chat_backend.rs`), nuovo
  metodo di default (`true`, comportamento Claude reale invariato).
  `OpenRouterBackend` lo sovrascrive a `false` (`openrouter_backend.rs`) —
  stessa collocazione logica di `to_or_tools`, capacità del backend, non del
  chiamante.
- **`LlmAdapter::respond`** (`ai_adapter.rs`) spegne `opts.web_search` PRIMA
  di costruire system prompt e tool set se `!self.backend.supports_web_search()`
  — un solo punto di gate, un solo flag guida sia il prompt sia il tool set
  (nessuna nuova granularità: `ClaudeBackend`/`web_fetch_supported`, un caso
  già esistente e diverso, non toccato).
- 2 test nuovi (RED-first): `openrouter_backend_does_not_support_web_search`,
  `web_search_mode_is_neutered_when_backend_does_not_support_it` (riproduce
  lo scenario esatto del bug: tool set E system prompt entrambi puliti).
  739 test lib verdi (+2), clippy/fmt invariati sui file toccati (drift
  pre-esistente in `ai_adapter.rs`/`openrouter_backend.rs`, non introdotto
  da questa release — vedi nota release 0.34.0).

---

## [0.40.33] — 2026-07-19 — fix: finestre plugin aperte di propria iniziativa non ricevevano più UiEvent

Bug di piattaforma trovato pianificando il plugin crittografia (Task 1 del
piano `plugin-crypto`, prerequisito — non tocca ancora il plugin stesso):
`PluginHost.windows` (registro `window_id → plugin_id`) era popolato SOLO
dentro `activate()`, cioè solo per la PRIMA finestra di un plugin, quella
aperta da un comando slash. Un plugin che apre una SECONDA finestra di
propria iniziativa (es. una dialog parametri spostabile, il requisito che
il plugin crittografia avrà più avanti nello stesso piano) inviava
`ShowWindow` regolarmente — il pump task lo traduceva in
`OpenPluginWindow` e la finestra si apriva sul client — ma il suo
`window_id` non finiva mai in `windows`. Ogni `UiEvent` successivo
generato da quella finestra veniva scartato in silenzio da
`route_ui_event` (`windows.get(&window_id) == None` → no-op), perché
quel branch conosce solo i window_id passati da `activate`. Nessun
errore, nessun log: la finestra sembrava reattiva all'apertura e poi
smetteva di rispondere.

- **Fix alla radice, nel pump task, non in `route_ui_event`**: il loop di
  `spawn_and_handshake` ora registra `windows.insert(window_id, plugin_id)`
  per OGNI `PluginToHost::ShowWindow` che osserva, non solo quello inviato
  durante l'handshake di `activate`. Un blind insert è corretto anche per
  il window_id già registrato da `activate` (stesso plugin_id, overwrite
  idempotente) — non serve un controllo "se assente".
- `PluginHost.windows` cambia tipo: da `HashMap<u64, String>` a
  `Arc<tokio::sync::Mutex<HashMap<u64, String>>>`. Necessario perché il
  pump task gira come `tokio::spawn` separato, FUORI dal lock esterno
  `Arc<Mutex<PluginHost>>` (stesso motivo per cui `writers`/`reader` sono
  già tenuti separati — vedi module doc di `host.rs`): senza un proprio
  lock interno, il pump non avrebbe modo di scrivere in `windows` senza
  rischiare un deadlock reader-vs-sender.
- `pub fn forget_window` diventa `pub async fn forget_window` (ora prende
  il lock interno). Unico chiamante, `ws.rs:365`
  (`ClientMsg::PluginWindowClosed`), aggiornato con `.await`.
- **Capacità di piattaforma riusabile**, non specifica del plugin
  crittografia: qualunque plugin futuro che apra più di una finestra di
  propria iniziativa (non tramite un comando slash) ne beneficia
  automaticamente.
- Test RED→GREEN: nuovo
  `plugins::host::tests::pump_registers_window_for_unsolicited_show_window`
  — un plugin lazy che, dopo `Ready`, emette anche
  `ShowWindow{window_id:42,...}` (mai passato da `activate`, che ha già
  assegnato `window_id:1`); il discriminante è
  `host.route_ui_event(42, "apply", ...)` che deve raggiungere il plugin
  (pre-fix: no-op silenzioso, verificato RED con `cargo test -p
  orchestrator pump_registers_window_for_unsolicited_show_window`).
  `cargo test -p orchestrator --lib plugins::host`: 11 passed. `cargo test
  -p orchestrator`: 737 lib + 16 ws_integration + 2 doctest, tutti verdi,
  nessuna regressione dal cambio di tipo di `windows`/dalla firma async di
  `forget_window`. `cargo build`: pulito.

## [0.40.32] — 2026-07-19 — feat: log di avvio mostra le versioni dei componenti sorella

Richiesta utente (verbatim): "nel log di orchestrator.exe, visualizza, a
seguire il versioning di orchestrator stesso, anche il versioning degli
altri componenti, protocol, mcp-server, eccetera, ad esclusione di ui.exe
che lo mostra di suo." La riga di avvio (`Lare Terminal orchestrator v{}
starting`) mostrava solo la propria versione; ora elenca anche
`protocol`, `plugin-protocol`, `mcp-server`, `mcp-nmap` — non `ui`, che è
un eseguibile lanciato separatamente e logga già la propria versione per
conto suo.

- Nuovo `build.rs`: legge la riga `version = "..."` dalla sezione
  `[package]` del `Cargo.toml` di ciascuno dei 4 crate sorella (semplice
  scan testuale — nessuna dipendenza `toml` in tutto il workspace, e non
  ne vale la pena per quattro letture one-shot) ed espone ognuna come env
  var a compile-time via `cargo:rustc-env=...`. `main.rs` le legge con
  `env!(...)`.
- `mcp-server` e `mcp-nmap` **non** diventano dipendenze Cargo di
  orchestrator — sarebbe la violazione di un confine architetturale
  deliberato: l'orchestrator li lancia come processi figli separati via
  stdio, non li linka mai. Le loro versioni si leggono dal `Cargo.toml`
  al build-time dell'orchestrator stesso, con lo stesso meccanismo usato
  per `protocol`/`plugin-protocol` (che invece SONO dipendenze reali via
  path) — un solo meccanismo uniforme, non due diversi per lo stesso tipo
  di informazione.
- Perché non hardcodare/rispecchiare a mano le stringhe di versione:
  `mcp-server` e `mcp-nmap` hanno già oggi esattamente questo failure
  mode — la loro riga di log di avvio hardcoda una versione letterale
  (es. `"...mcp-nmap v0.8.2 starting..."`) che va tenuta sincronizzata a
  mano a ogni bump. Leggendo ogni `Cargo.toml` al build time
  dell'orchestrator, il suo log non può mai andare fuori sincrono da
  quanto effettivamente presente nel workspace, indipendentemente da
  quanto sia stale la stringa hardcoded di un altro crate.
- `main.rs`: la riga di log diventa
  `"Lare Terminal orchestrator v{} starting (protocol v{} · plugin-protocol v{} · mcp-server v{} · mcp-nmap v{})"`.
- Nessun test automatico aggiunto: `env!(...)` è una macro a compile-time
  — se `build.rs` non impostasse una delle env var, l'intero crate non
  compilerebbe affatto, garanzia più forte di qualunque test a runtime.
  Verificato con `cargo build -p orchestrator` (successo) e run reale del
  binario (`cargo run -p orchestrator`), confermando la riga di log con
  le 4 versioni correnti (`protocol v0.14.6 · plugin-protocol v0.2.1 ·
  mcp-server v0.5.0 · mcp-nmap v0.8.2`) — tutte diverse tra loro, nessuna
  `0.0.0`/vuota/panic. `cargo test -p orchestrator` invariato (736 lib +
  16 ws_integration + 2 doctest, tutti verdi).

## [0.40.31] — 2026-07-19 — fix: `mcp-nmap.exe` orfano permanente alla chiusura NORMALE della finestra `/nmap`

Secondo trigger della stessa leak documentata in 0.40.30 (debito aperto
esplicitamente in quella voce), trovato dalla stessa review indipendente e
confermato leggendo per intero `handle_connection` (`ws.rs`), non per
ipotesi: chiudere la finestra `/nmap` normalmente — dopo uno scan riuscito,
o anche senza mai lanciarne uno — lasciava `mcp-nmap.exe` orfano per
sempre, esattamente come il timeout non gestito di 0.40.30. Nessun
`Drop`/hook di teardown esisteva su `NmapToolClient`, né sul trait
`ToolClient` in generale: il task `tokio::spawn(async move {
running.waiting().await.ok(); })` dentro `ensure_connected` tiene vivo il
processo indipendentemente dal ciclo di vita del client che lo ha creato.
Verificato leggendo l'intera `handle_connection`: dopo che il binding
`tools` viene risolto da `resolve_channel_tools` (~riga 199), l'UNICO
percorso che raggiunge la fine della funzione (quindi il teardown) è
l'uscita naturale dal loop `while let Some(client_msg) = in_rx.recv()
.await` quando la connessione WS si chiude — ogni altro `return` nella
funzione avviene PRIMA di quel binding. Un solo punto di aggancio serviva,
ed è l'unico che serve: nessun'altra via di chiusura resta scoperta.

- Nuovo metodo `ToolClient::shutdown(&self)` con **default no-op**
  (`tool_client.rs`): chiamato una volta quando la connessione a cui
  appartiene il `ToolClient` va in chiusura. Default no-op perché ogni
  `ToolClient` storico (cursore/Telegram via `McpToolClient`) non possiede
  alcuna risorsa da rilasciare — il processo `mcp-server` è un singleton a
  vita di workspace, gestito altrove, non per-connessione. Zero modifiche
  richieste a `McpToolClient`/`FakeToolClient`/`FixtureChannelToolClient`.
- `NmapToolClient::abandon_connection_after_timeout` **rinominato** in
  `close_connection` (`nmap_tool_client.rs`): non è più specifico del
  timeout, essendo ora chiamato da tre punti (i due rami `Err(_elapsed)`
  di `call_scan_tool`/`call_info_tool`, invariati nella logica, più il
  nuovo `shutdown()`). Corpo invariato: azzera il peer in cache e uccide
  il processo tracciato via `killer.kill_tree(pid)` se presente.
- `impl ToolClient for NmapToolClient` guadagna `shutdown()`, che delega a
  `close_connection()` — riusa esattamente la stessa logica di kill del
  fix 0.40.30, nessuna duplicazione.
- `ws.rs`: `tools.shutdown().await` aggiunto nel blocco di chiusura di
  `handle_connection`, subito dopo il drain di `commands`/`searches` e
  prima della notifica `ServiceEvent::UiClosed` ad AI Chat. Sicuro per
  cursore/Telegram (dove `tools` è il `McpToolClient` condiviso): eredita
  il default no-op, quindi non tocca mai il singleton `mcp-server`.
- Test: le 2 tabelle esistenti (`abandon_connection_after_timeout_kills_
  the_stored_pid_and_clears_state`,
  `abandon_connection_after_timeout_does_not_kill_when_never_connected`)
  rinominate in `close_connection_*` (stesso corpo, solo il nome del
  metodo chiamato cambia). Nuovo test
  `shutdown_kills_the_connected_process` (`nmap_tool_client.rs`): prova
  che `ToolClient::shutdown()` — l'unico metodo che `ws.rs` chiama
  davvero — instrada fino al kill, non solo che il metodo inerente
  funziona in isolamento. Nuovo test `default_shutdown_is_a_true_noop`
  (`tool_client.rs`): `FakeToolClient` non sovrascrive `shutdown()`, prova
  che il default esiste, è chiamabile, e non fa nulla di osservabile —
  guardia contro un futuro refactor che renda `shutdown` un metodo
  required (romperebbe ogni `ToolClient` storico).
- TDD genuino: rename dei test + nuovi test scritti PRIMA
  dell'implementazione, RED verificato come errore di compilazione
  `E0599: no method named 'close_connection'/'shutdown' found` (non un
  typo — il metodo davvero non esisteva ancora), non un fallimento a
  runtime. `cargo test -p orchestrator --lib nmap_tool_client`: 10/10
  (7 preesistenti + 1 rinominato ×2 + 1 nuovo — vedi dettaglio sopra).
  `cargo test -p orchestrator --lib tool_client`: 39/39 (38 preesistenti +
  1 nuovo). `cargo test -p orchestrator --lib`: 736 passati (734 di
  0.40.30 + 2 nuovi), 0 falliti, 4 ignorati (invariato — e2e reali
  preesistenti). `cargo test -p orchestrator` (suite intera): 16/16
  `ws_integration`, 1/1 doc-test, tutto verde. `cargo build` (intero
  workspace): pulito. `cargo clippy -p orchestrator --all-targets`: stessi
  4 warning pre-esistenti di 0.40.30 (`ai_adapter.rs:331`, `ws.rs:586`,
  `aichat/service.rs:5604`/`7958`), zero warning nuovi. `cargo fmt -p
  orchestrator --check`: stesso conteggio di diff pre-esistenti nei 3 file
  toccati rispetto alla versione precedente (13 in `nmap_tool_client.rs`,
  8 in `tool_client.rs`, 14 in `ws.rs` — verificato per confronto diretto
  via `git stash`), zero drift nuovo introdotto da questo task.

Nessun file sotto `crates/mcp-nmap/` toccato. Nessun processo reale
(nmap/mcp-nmap) eseguito durante questo task: validazione interamente via
`FakeProcessTreeKiller`/`FakeToolClient`, deterministica.

**Addendum post-review (stesso commit) — debito residuo trovato, NON
chiuso qui:** la review indipendente ha verificato (non ipotizzato — un
grep mirato in `ai_adapter.rs`) che il loop di dispatch dei tool_use
controlla la cancellazione una sola volta per iterazione ESTERNA del turno
AI (`ai_adapter.rs:331`), non tra un `tools.dispatch()` e il successivo
all'interno dello stesso batch di tool_use (il loop interno che parte da
`ai_adapter.rs:432` non ha nessun controllo del genere). Conseguenza: se un
singolo turno AI richiede PIÙ tool_use in batch (es. `local_network_info`
seguito da `nmap_quick_scan`, esplicitamente incoraggiato dal system prompt
di questo canale) e la connessione WS cade esattamente tra due dispatch di
quel batch, il primo dispatch può già aver innescato `close_connection`
(via il proprio timeout, o l'ordine event-loop), azzerando peer/child_pid;
il dispatch SUCCESSIVO nello stesso batch trova `peer == None`, fa
ripartire `ensure_connected` e spawna un `mcp-nmap.exe` nuovo — se QUELLA
chiamata completa con successo (il caso comune), nulla la uccide più:
`shutdown()` per questa connessione è già stato chiamato una volta e non
verrà richiamato. Molto più stretto del bug pre-0.40.31 (richiede un turno
con PIÙ tool_use in batch E una caduta della connessione esattamente a
metà batch), ma reale — non chiuso in questo commit. Deliberatamente non
toccato `ai_adapter.rs` per risolverlo qui: quel loop serve OGNI canale
(cursore, Telegram, `/nmap`), un cambio lì per un leak specifico di un
canale ha un raggio d'azione troppo ampio per questo task — richiede una
decisione a parte, non presa qui.

## [0.40.30] — 2026-07-19 — fix: `mcp-nmap.exe` orfano permanente al timeout di `NmapToolClient`

Bug confermato via riproduzione dal vivo (non ipotizzato): abbassata
temporaneamente `NMAP_CALL_TIMEOUT_SECS` a 10s, eseguito un vero
`nmap_vuln_scan` (che normalmente completa in ~71s grazie al fix
`--script-timeout` di mcp-nmap 0.8.2) attraverso `NmapToolClient::dispatch`
direttamente, poi verificato via `Get-Process`/`tasklist` sia subito dopo il
timeout sia 90s dopo. Esito: `nmap.exe` termina da solo (limitato da
`--script-timeout`) e sparisce entro i 90s — non è il problema di questo
fix. `mcp-nmap.exe` invece resta presente e **invariato** (PID stabile) 90s
dopo il timeout: il vecchio codice, al timeout, azzerava solo l'handle
`Peer` in cache (`*self.peer.lock().await = None`) senza mai toccare il
processo figlio reale — `mcp-nmap.exe`, un server MCP stdio, resta vivo per
design in attesa del prossimo messaggio, che non arriva mai. Ogni singolo
timeout lasciava un `mcp-nmap.exe` in più a vivere per sempre: una leak
permanente e illimitata, visibile solo dal Task Manager.

- Nuovo trait `ProcessTreeKiller` (seam DI, stesso pattern di `NmapProcess`
  di `mcp-nmap`) con implementazione reale `RealProcessTreeKiller` (
  `taskkill /F /T /PID` su Windows) e fake di test (`FakeProcessTreeKiller`).
  Il `/T` (tree-kill) è difesa in profondità: Windows non uccide a cascata i
  processi figli quando muore il padre, quindi cattura anche il raro caso in
  cui `nmap.exe` (figlio di mcp-nmap, non dell'orchestrator) sia ancora vivo
  al momento del timeout (es. un hang nella fase di port-scan, non coperta
  da `--script-timeout`).
- `NmapToolClient` guadagna due campi: `child_pid` (il PID di
  `mcp-nmap.exe` catturato in `ensure_connected` subito dopo lo spawn) e
  `killer: Arc<dyn ProcessTreeKiller>`.
- Nuovo helper `abandon_connection_after_timeout()`: azzera il peer in
  cache E uccide il processo abbandonato tramite `killer.kill_tree(pid)`.
  Usato nei due rami `Err(_elapsed)` (timeout) di `call_scan_tool` e
  `call_info_tool`, al posto del vecchio `*self.peer.lock().await = None;`
  inline. I rami `Err(e)` generici (errore di trasporto/protocollo, non
  timeout) restano **invariati**: su un errore generico il processo si è
  quasi certamente già chiuso da solo, quindi non c'è nulla da uccidere.
- Valutata e scartata l'alternativa di rendere asincrono `NmapProcess::run`
  in `mcp-nmap` per propagare un timeout interno: cambio molto più ampio e
  rischioso (si propaga a `RealNmapProcess` e ~15 fake di test in
  `scan.rs`) per un problema che riguarda l'orchestrator che non uccide il
  proprio figlio diretto. `mcp-nmap.exe` è un figlio a un solo salto
  dell'*orchestrator* (non di mcp-nmap stesso): ucciderlo direttamente da
  qui è il fix minimo per la leak confermata. `mcp-nmap` resta a 0.8.2,
  invariato.
- 2 nuovi test unitari (`abandon_connection_after_timeout_kills_the_stored_
  pid_and_clears_state`, `abandon_connection_after_timeout_does_not_kill_
  when_never_connected`) con `FakeProcessTreeKiller`, nessun processo reale
  coinvolto. `cargo test -p orchestrator --lib nmap_tool_client`: 9/9
  (7 preesistenti + 2 nuovi). `cargo test -p orchestrator`: tutto verde.
  `cargo build` (workspace): pulito. `cargo clippy -p orchestrator
  --all-targets`: stessi 4 warning pre-esistenti (`ai_adapter.rs:331`,
  `ws.rs:586`, `aichat/service.rs:5604`/`7958`), nessuno nuovo.

**Addendum post-review (stesso commit):** una review indipendente ha
verificato la domanda di concorrenza più rischiosa di questo fix — se una
call in timeout potesse uccidere il processo che un'ALTRA call concorrente
sta ancora legittimamente usando — e l'ha esclusa con prove dirette (non
per assunzione): ogni connessione WS ottiene un `NmapToolClient` proprio
(`external_channel.rs`, factory chiamata una sola volta per connessione,
zero stato condiviso tra connessioni), e dentro una connessione `ws.rs`
tiene il lock di `ConversationHistory` per l'intera `handle_command(...)
.await`, tool call incluse — una seconda `dispatch()` non può iniziare
finché la prima non è tornata. Questo invariante è ora documentato
esplicitamente sul tipo `NmapToolClient` (non era scritto da nessuna
parte prima, solo vero per un effetto collaterale del locking di
`ws.rs`). Considerato ma scartato l'accorpamento di `peer`/`child_pid` in
un unico `Mutex` (avrebbe reso l'atomicità esplicita anziché implicita):
`rmcp::Peer` non ha un costruttore utilizzabile nei test, quindi
l'accorpamento avrebbe reso impossibile testare in isolamento il
comportamento di kill-by-pid senza una connessione MCP reale — bocciato,
non ne valeva il costo. Aggiunta anche una nota sul trait
`ProcessTreeKiller`: `/T` (tree-kill) non copre in modo affidabile
`nmap_os_detect`'s `nmap.exe` elevato (rischia di essere ri-parentato
sotto il servizio AppInfo da `ShellExecuteExW`/UAC, fuori dall'albero
processi che `taskkill /T` percorre) — il fix primario (uccidere
`mcp-nmap.exe`, la leak confermata) resta valido anche per
`nmap_os_detect`, solo il bonus "cattura anche nmap.exe" non si estende
fino al caso elevato.

**Trovato ma NON risolto in questo fix — debito aperto:** la stessa
review ha scoperto che una chiusura NORMALE della sessione `/nmap` (uno
scan completato con successo, poi la finestra chiusa) lascia lo stesso
`mcp-nmap.exe` orfano per sempre — nessun `Drop`/hook di teardown esiste
su `NmapToolClient`, il task che tiene vivo il processo
(`tokio::spawn(async move { running.waiting()... })` in `ensure_connected`)
è scollegato dal ciclo di vita del client stesso. Non è una regressione di
questo commit (il vecchio codice aveva la stessa lacuna) ed è
probabilmente il trigger PIÙ comune (ogni sessione riuscita, non solo
quelle che superano i 900s). Richiede lo stesso meccanismo
`child_pid`/`kill_tree`, agganciato però al teardown della connessione
WS (`ws.rs`) invece che al ramo di timeout — task separato.

## [0.40.29] — 2026-07-18 — feat: wiring dei 3 nuovi tool scan nmap (versione, host discovery, vuln), gate esteso

Consuma `mcp-nmap` 0.8.0 (piano `2026-07-18-nmap-scan-variants.md`, Task 2): il
canale `/nmap` ora espone all'AI i 3 nuovi tool scan aggiunti in mcp-nmap
(`nmap_version_scan`, `nmap_host_discovery`, `nmap_vuln_scan`), oltre ai 4
esistenti.

- `NmapToolClient::tool_defs()`: da 4 a 7 `ToolDef`. I 3 nuovi tool sono
  strutturalmente identici a `nmap_quick_scan`/`nmap_os_detect` (stesso
  shape JSON `{summary, report_markdown, is_error}`), quindi instradano
  attraverso il metodo `call_scan_tool` GIÀ esistente — nessun nuovo
  metodo/struct lato client, solo 3 nuovi rami nel `match` di `dispatch()`.
- `local_confirm::SENSITIVE_TOOLS`: da 2 a 5 voci. Tutti e 3 i nuovi tool
  sono gateizzati dietro il banner di conferma locale — stesso principio già
  applicato a `nmap_quick_scan`/`nmap_os_detect` (qualunque tool che esegue
  attivamente uno scan su un target scelto dall'AI richiede conferma
  esplicita prima di partire), non una policy nuova. Corretto anche il
  doc-comment del modulo, che citava ancora "i due tool" del canale nmap.
- `external_channel::format_nmap_invocation` (copia duplicata dell'omonima
  funzione in mcp-nmap, stessa motivazione di sempre — niente dipendenza
  cross-crate per 8 righe): 3 nuovi rami per i 3 nuovi tool.
- `NMAP_SYSTEM_PROMPT`: riscritto per descrivere tutti e 7 i tool
  (prima ne descriveva 4). Dice esplicitamente che `nmap_vuln_scan` esegue
  SOLO la categoria NSE `vuln` integrata in nmap — mai uno script
  personalizzato — così l'AI non lascia intendere all'utente di poter
  eseguire NSE arbitrario. Mantenuti invariati: l'istruzione di chiamare
  `local_network_info` prima di uno scan quando manca il target, e i vincoli
  "niente shell/file/URL/finestre Markdown, non ripetere il report".

## [0.40.28] — 2026-07-18 — fix: canale esterno rubava il sink condiviso di PluginHost

Trovato dal vivo: dopo aver aperto `/nmap`, i plugin (`/lc`/`/calc`/`/counter`)
iniziavano a fallire in modo intermittente — a volte "exit 0" senza che la
finestra si aprisse mai. Causa: `handle_connection` (`ws.rs`) chiamava
incondizionatamente `plugin_host.set_server_tx(out_tx)` per QUALSIASI
connessione. `PluginHost` è un unico `Arc<Mutex<>>` per-processo, condiviso da
ogni connessione WS — prima del canale esterno esisteva sempre e solo UNA
connessione (il cursore), quindi il bug era invisibile. `/nmap` apre una
SECONDA connessione (`Hello{channel:"nmap"}`): la sua `set_server_tx` rubava
il sink al cursore principale. Un plugin attivato per la prima volta dopo
quel momento catturava (nel proprio pump task, spawn-time) il sink sbagliato
— il suo `ShowWindow` finiva sulla connessione `/nmap`, priva di un case per
quel messaggio (scartato silenziosamente). Fix: nuova `connection_owns_plugin_sink(channel: Option<&str>) -> bool`
(pura, `channel.is_none()`) — `set_server_tx` ora scatta solo per la
connessione del cursore principale. 2 nuovi test unitari. Copertura
end-to-end non automatizzabile: il dispatch dei plugin lazy in
`handle_connection` usa `spawn_plugin` reale, non iniettabile nei test di
`tests/ws_integration.rs` — verificata per ispezione.

**Nota per il prossimo giro:** `ServiceEvent::SetServerTx` di AI Chat (riga
successiva nello stesso blocco) ha la STESSA forma — una connessione di
canale probabilmente ruba anche quel sink. Non toccato qui: gatearlo alla
cieca potrebbe rompere AI Chat, va verificato prima.

## [0.40.27] — 2026-07-18 — `NmapToolClient`: `local_network_info` + `traceroute`

(Rinumerato da 0.40.26 a 0.40.27 al merge: 0.40.26 nel frattempo assegnato
a un fix indipendente, si veda subito sotto — nessuna sovrapposizione di
codice fra i due, solo la versione andava rinumerata per evitare la
collisione.)

`tool_defs()` passa da 2 a 4 tool; `dispatch()` instrada i 2 nuovi via un
nuovo `call_info_tool` (stessa macchina di lazy-connect/timeout di
`call_scan_tool`, ma deserializza `NmapInfoOutcomeJson{output, is_error}` —
niente `report`, questi 2 tool mandano tutto l'output al modello). Nessuna
voce nuova in `SENSITIVE_TOOLS`: entrambi sono read-only/locali, nessun
banner di conferma. `NMAP_SYSTEM_PROMPT`/`format_nmap_invocation`
aggiornati per i 4 tool. Consuma `mcp-nmap` 0.7.1.

## [0.40.26] — 2026-07-18 — rimosso il salvataggio automatico in Library dei report di canale

`ai_adapter.rs::respond()` non emette più `ServerMsg::SaveToLibrary` quando
un `DispatchOutcome.report` è presente — apre ancora la finestra
(`ServerMsg::OpenWindow`, deterministico, `allow_windows`-gated com'era) ma
il salvataggio in Library resta a scelta dell'utente, tramite il pulsante
"Salva" già esistente in ogni finestra Markdown. Trovato dal vivo: uno scan
nmap salvato una volta produceva 2 voci identiche in Library, stessa ora e
minuto — un salvataggio automatico + un click sul pulsante manuale. Vedi
`protocol` 0.14.6 per la rimozione della variante `ServerMsg::SaveToLibrary`
stessa.

## [0.40.25] — 2026-07-16 — Canale `/nmap` registrato — `EXTERNAL_TOOL_CHANNELS` non è più vuoto

`EXTERNAL_TOOL_CHANNELS` guadagna la sua prima voce reale: `id: "nmap"`,
`tool_client: NmapToolClient::resolve`, `format_invocation` (dichiara
esplicitamente "richiede privilegi elevati" per `nmap_os_detect`),
`system_prompt_override` (l'AI del canale sa di avere SOLO i 2 tool nmap).
`SENSITIVE_TOOLS` guadagna `nmap_quick_scan`/`nmap_os_detect` — primi tool
mai gateizzati dal gate di conferma locale. Aggiorna 3 test preesistenti (non
2, come nel piano originale — un terzo era nascosto in `router.rs`, stessa
famiglia di problema) il cui nome/assunzione ("nmap"/"/nmap" come esempio di
canale/trigger sconosciuto, o `SENSITIVE_TOOLS` sempre vuoto) non erano più
veri: `external_channel::empty_production_registry_never_matches_anything` →
`unregistered_channel_id_still_errs_against_the_real_registry`,
`router::external_channel_command_empty_registry_always_none` →
`external_channel_command_unregistered_trigger_against_real_registry_is_none`,
`local_confirm::should_gate_is_false_for_any_tool_while_sensitive_tools_is_empty`
→ `should_gate_is_true_for_sensitive_tools_false_for_others`. Anche 3 commenti
non-test (in `ws.rs` x2, `ai_adapter.rs` x1) che affermavano ancora
"SENSITIVE_TOOLS vuoto"/"nessun tool gateizzato" sono stati corretti (solo
commenti, nessun cambio di logica).

## [0.40.24] — 2026-07-16 — `NmapToolClient`

Primo `ToolClient` reale per un canale esterno. Mirror di `McpToolClient`
(spawn lazy del sidecar `mcp-nmap`, `LARE_MCP_NMAP` per l'override del path)
ma: nessuno streaming, timeout di chiamata 900s (supera i 600s interni di
`mcp-nmap::elevate::SCAN_TIMEOUT` + i 180s del gate di conferma). `tool_defs()`
espone SOLO `nmap_quick_scan`/`nmap_os_detect`; `dispatch()` instrada a
entrambi via il sidecar e popola `DispatchOutcome.report` (titolo + Markdown
completo) quando lo scan riesce. `run_in_session`/`open_target`/
`reset_session` ritornano "non disponibile su questo canale" — chiude
strutturalmente le vie `Route::Os`/`Route::Slash` per questo canale, mirror
esatto di `FixtureChannelToolClient`. Non ancora registrato in
`EXTERNAL_TOOL_CHANNELS` (prossimo task).

## [0.40.23] — 2026-07-16 — `DispatchOutcome`/`ChannelReport`: un canale può produrre un report

`ToolClient::dispatch()` ritorna ora `DispatchOutcome { output, is_error, report:
Option<ChannelReport> }` invece di `(String, bool)`. `report`, quando `Some`,
fa sì che `LlmAdapter::respond` apra una finestra Markdown E salvi una copia
in Library (`ServerMsg::OpenWindow` + `ServerMsg::SaveToLibrary`, 0.14.5) —
DETERMINISTICAMENTE, mai una scelta dell'AI (che vede solo `output`).
`FakeToolClient`/`McpToolClient`/`CwdTrackingToolClient`/`FixtureChannelToolClient`
impostano tutti `report: None` — zero comportamento cambiato per cursore/
Telegram/i test di isolamento esistenti. Primo consumatore reale:
`NmapToolClient` (prossimo task).

## [0.40.22] — 2026-07-16 — Prova end-to-end dell'isolamento per-canale

4 nuovi test provano i 3 assi di isolamento descritti in
`Docs/superpowers/specs/2026-07-16-tool-isolation-design.md` usando
`FixtureChannelToolClient` (0.40.20) come `ToolClient` di un canale
ipotetico, SENZA toccare `EXTERNAL_TOOL_CHANNELS` (resta vuoto): (1)
`channel_tool_client_blocks_os_route_shell_execution` +
`channel_tool_client_blocks_open_target_slash` (`core.rs`) — `Route::Os`/
`Route::Slash` restano bloccate da un `ToolClient` di canale, senza che
`core.rs` sappia nulla di canali; (2)
`channel_tool_client_restricts_tools_and_uses_own_system_prompt`
(`ai_adapter.rs`) — il backend riceve SOLO i tool del canale e il system
prompt del canale, mai il default; (3)
`channel_tool_client_rejects_off_menu_tool_use` (`ai_adapter.rs`) — un
tool_use per un nome fuori menu (qui `run_in_session`, mai offerto) è
rifiutato dal dispatch del canale, non eseguito. Nessun codice di
produzione cambia in questo task — solo test. Chiude il piano
`Docs/superpowers/plans/2026-07-16-tool-isolation.md`; il prossimo
consumatore reale è il server MCP `nmap` (spec separato), che ora ha un
meccanismo provato su cui costruire, non solo assunto.

## [0.40.21] — 2026-07-16 — `TurnOptions.system_prompt_override` + `respond()` usa `tools.tool_defs()`/`dispatch()`

`TurnOptions` guadagna `system_prompt_override: Option<&'static str>`, filato
da `ExternalToolChannel` (nuovo campo) attraverso `resolve_channel_tools`
(ora una 3-upla, era una coppia) fino a `core::handle_command` (nuovo
parametro dopo `format_invocation`) — stesso identico pattern del campo
`format_invocation` (0.40.18). `agent::system_prompt` lo usa per sostituire
INTERAMENTE il prompt di default quando presente. `agent::tools_for` diventa
pura (accetta `defs: Vec<ToolDef>` come parametro, non chiama più
`agent::tool_defs()` internamente); `LlmAdapter::respond` ora chiama
`tools.tool_defs()`/`tools.dispatch()` (i metodi del trait aggiunti in
0.40.20) al posto delle funzioni libere `agent::tool_defs()`/
`agent::dispatch_tool()`. `EXTERNAL_TOOL_CHANNELS` resta vuoto: **nessun
comportamento visibile cambia** per cursore/Telegram — `FakeToolClient`/
`McpToolClient` ereditano ancora i default del trait. `telegram/channel.rs`
aggiorna anch'esso la sua chiamata diretta a `handle_command` (`None` — stesso
trattamento di `format_invocation`). Design:
`Docs/superpowers/specs/2026-07-16-tool-isolation-design.md`.

## [0.40.20] — 2026-07-16 — `ToolClient::tool_defs()`/`dispatch()` (required, non default)

`ToolClient` guadagna `tool_defs() -> Vec<ToolDef>` (metodo con default:
`agent::tool_defs()`, i 3 storici) e `dispatch(name, input) -> (String, bool)`
(**metodo required**, non default — un default trait method non può castare
`&Self` a `&dyn ToolClient` per chiamare la funzione libera
`agent::dispatch_tool`, un vincolo reale di object-safety). Ognuno dei tre
`ToolClient` esistenti (`FakeToolClient`, `McpToolClient`,
`CwdTrackingToolClient`) fornisce quindi il proprio corpo per `dispatch`, che
delega a `agent::dispatch_tool(self as &dyn ToolClient, name, input)` —
re-entrando tramite `self`, MAI tramite un eventuale `inner` — cursore/Telegram
invariati byte-per-byte. Un `ToolClient` di canale esterno sovrascrive
`tool_defs`/`dispatch` per esporre e dispacciare SOLO i propri tool. Aggiunta
`FixtureChannelToolClient` (test double): 2 tool fittizi, e le 3 chiamate
storiche (`run_in_session`/`open_target`/`reset_session`) ritornano "non
disponibile su questo canale" invece di delegare a una shell/target reale —
dimostra come un `ToolClient` di canale chiude anche le vie
`Route::Os`/`Route::Slash` di `core.rs` senza che `core.rs` sappia nulla di
canali. Non ancora agganciato: `agent::tools_for`/`agent::dispatch_tool`/
`LlmAdapter::respond` continuano a usare le funzioni libere (Task 2). Design:
`Docs/superpowers/specs/2026-07-16-tool-isolation-design.md`.

**Fix di review (stesso commit del fix, non un bump di versione separato):**
la prima versione di questo cambiamento aveva un bug critico, confermato
indipendentemente da due review: `CwdTrackingToolClient::dispatch` delegava a
`self.inner.dispatch(name, input)` invece di re-entrare tramite `self` come il
resto del pattern sopra. Per `name == "run_in_session"`, questo re-entrava
`agent::dispatch_tool` con `self.inner` come receiver — chiamando
`run_in_session` **direttamente sul client interno**, bypassando totalmente
l'override di `CwdTrackingToolClient::run_in_session` (quello che aggiorna
`cwd_state`). Non era un bug live al momento del commit (nessun sito di
produzione chiama ancora `ToolClient::dispatch`; `ai_adapter.rs` chiama la
funzione libera `agent::dispatch_tool(tools, ...)` direttamente), ma lo
sarebbe stato silenziosamente non appena un task futuro collegasse il loop
tool-use dell'AI a `tools.dispatch(...)`: i comandi shell guidati dall'AI
avrebbero smesso di aggiornare il cwd tracciato, mentre quelli diretti (route
Os, che chiamano `.run_in_session()` direttamente) avrebbero continuato a
funzionare — un bug subdolo, difficile da notare in seguito. Corretto
dispacciando su `self`: `crate::agent::dispatch_tool(self as &dyn ToolClient,
name, input).await`, che re-entra con QUESTO decorator come receiver, così
`run_in_session` ricade sull'override che aggiorna `cwd_state`. Nuovo test di
regressione `dispatch_run_in_session_updates_cwd_state` (RED confermato
contro il codice buggy, poi GREEN dopo il fix).

## [0.40.19] — 2026-07-16 — `router::external_channel_command` (pura, non ancora agganciata)

Mirror esatto di `plugin_command` sul registro `ExternalToolChannel`. Chiude
la metà "router.rs" dello spec §6; la metà "wiring in `ws.rs`" resta deferita
— aprire un canale esterno è un'azione frontend (Task 7 del piano), non un
dispatch backend come per i plugin. `EXTERNAL_TOOL_CHANNELS` vuoto: la
funzione ritorna sempre `None` in produzione oggi.

## [0.40.18] — 2026-07-16 — Wiring `ws.rs` + `TurnOptions.format_invocation`

`ws.rs`: `parse_hello` ora ritorna `(token, channel)`; l'handshake risolve
`(tools, format_invocation)` per-connessione via `external_channel::resolve_channel_tools`
(0.40.16), shadowando localmente il `ToolClient` condiviso — `channel: None`
(cursore/Telegram) invariato, canale sconosciuto → `ServerMsg::Error` esplicito
e chiusura connessione (mai un panic). `TurnOptions` guadagna il campo
`format_invocation: Option<fn(&str,&Value)->String>`, che `LlmAdapter::respond`
usa al posto di `agent::display_invocation` nei suoi due punti di trasparenza
(banner di conferma + Chunk) quando `Some`. `EXTERNAL_TOOL_CHANNELS` resta
vuoto: **nessun comportamento visibile cambia**. Chiude l'infrastruttura del
canale tool esterno; il prossimo consumatore reale sarà il server MCP `nmap`
(spec separato). Design: `Docs/superpowers/specs/2026-07-16-external-tool-channel-design.md`.
`telegram/channel.rs`: aggiornata anch'essa la sua chiamata diretta a
`handle_command` (`format_invocation: None` — il canale Telegram non
partecipa alla risoluzione canale, fallback su `agent::display_invocation`
invariato) — sito di produzione non coperto dallo spec originale, individuato
in fase di build.

## [0.40.16] — 2026-07-16 — `external_channel.rs`: registro canali tool esterni (vuoto)

Nuovo modulo `external_channel.rs`: `ExternalToolChannel` (id, slash_trigger,
window_title, tool_client factory, format_invocation opzionale) +
`EXTERNAL_TOOL_CHANNELS` (**vuoto**) + `resolve_channel_tools` (pura,
testata con `FakeToolClient` distinti). Base per la connessione WS scoped
del canale tool esterno (design `Docs/superpowers/specs/2026-07-16-external-tool-channel-design.md`);
non ancora agganciato a `ws.rs` (prossima release). Nessun comportamento
visibile cambia — stesso principio già usato per `SENSITIVE_TOOLS`.

## [0.40.15] — 2026-07-15 — Gate locale per-tool: wiring in `ws.rs`

`ws.rs` tiene un `PendingConfirms` per connessione (accanto ai registri esistenti
`searches`/`commands`); nuovo arm `ClientMsg::ToolConfirmResponse` lo risolve (id
sconosciuto → no-op silenzioso, stesso principio di `CancelCommand` su id già
terminato). Il task spawnato per ogni comando normale costruisce un
`LocalUiConfirmer` (timeout 180s) e lo passa a `core::handle_command` al posto del
precedente `None` — con `SENSITIVE_TOOLS` ancora vuoto (`local_confirm.rs`,
v0.40.14) nessun tool viene gateizzato: **nessun comportamento visibile cambia**.
Chiude l'infrastruttura del gate locale per-tool; il prossimo consumatore reale
sarà il server MCP `nmap` (spec separato). Design:
`Docs/superpowers/specs/2026-07-15-local-tool-confirm-gate-design.md`.

Fix aggiuntivo trovato in review di Task 3: `LocalUiConfirmer::confirm` lasciava
un'entry orfana in `PendingConfirms` sui percorsi di timeout e di send fallito
(solo una `resolve()` andata a buon fine rimuoveva la entry). Ora entrambi i
percorsi chiamano `pending.resolve(&id, false)` prima di ritornare — no-op
idempotente se già risolta altrove. Nuovo test di regressione
`confirm_timeout_removes_orphaned_entry_from_registry`.

---

## [0.40.14] — 2026-07-15 — `local_confirm`: `PendingConfirms` + `LocalUiConfirmer`

Nuovo modulo `local_confirm.rs`: `PendingConfirms` (registro condiviso
`id → oneshot::Sender<bool>`, cloneabile) + `LocalUiConfirmer` (implementa
`ToolConfirmer`, round-trip via `ServerMsg::ToolConfirmRequest` +
`ClientMsg::ToolConfirmResponse`, timeout configurabile, id opaco non derivato
dal comando — stessa garanzia già testata per `TelegramConfirmer`).
`SENSITIVE_TOOLS` vuoto: `should_gate` ritorna sempre `false`, nessun consumatore
ancora (arriva col wiring in `ws.rs`, prossima versione). 8 test nuovi. Design:
`Docs/superpowers/specs/2026-07-15-local-tool-confirm-gate-design.md`.

---

## [0.40.13] — 2026-07-15 — `ToolConfirmer::should_gate`: gate per-tool invece che per-canale

`ToolConfirmer` guadagna `should_gate(&self, tool_name: &str) -> bool` (default:
`tool_name != "show_markdown"`, preserva il comportamento storico — `TelegramConfirmer`
non lo sovrascrive). Il filtro `gated_labels` in `ai_adapter.rs` usa ora `should_gate`
invece del confronto fisso `name != "show_markdown"`. Prerequisito per un confirmer
locale che gateizzi solo tool esplicitamente marcati sensibili (Task 3), lasciando
`run_in_session`/`open_target` autonomi. Nessuna regressione: i test esistenti del
gate Telegram (`AllowAll`/`DenyAll`/`CountingDeny`, che ereditano il default)
restano verdi invariati. Design:
`Docs/superpowers/specs/2026-07-15-local-tool-confirm-gate-design.md`.

**Fix post-review (stesso commit logico, commit `d85d827`):** il verdetto `approved`
del turno era ancora applicato in blocco a TUTTI i tool non-`show_markdown`
nell'esecuzione (non solo a quelli che `should_gate` marca) — inerte oggi
(`SENSITIVE_TOOLS` vuoto) ma avrebbe bloccato `run_in_session` in un turno misto
con un tool sensibile negato, una volta popolato `SENSITIVE_TOOLS`. Corretto:
il blocco in esecuzione ora consulta `should_gate` anch'esso. Test di
regressione `deny_blocks_only_the_gated_tool_in_a_mixed_turn` (RED-first).

---

## [0.40.12] — 2026-07-12 — plugin host: propaga la dimensione finestra dal manifest

Parte dello slice cross-crate "plugin-lc UX + window protocol". Il pump task del plugin host
ora inoltra la dimensione iniziale dichiarata nel manifest (`plugin_protocol::WindowSize`) fino
al `ServerMsg::OpenPluginWindow`.

### Changed
- `plugins::transport::plugin_msg_to_server` guadagna un secondo parametro `window: Option<plugin_protocol::WindowSize>`. Per `ShowWindow` popola i nuovi `width`/`height` di `OpenPluginWindow`; per `UpdateWindow`/`CloseWindow` il parametro è IGNORATO (la dimensione conta solo alla creazione della finestra).
- `plugins::host::spawn_and_handshake`: il pump task cattura `p.manifest.window` (Copy) e lo passa a `plugin_msg_to_server` a ogni messaggio.

### Tests
- Nuovo `plugin_msg_to_server_threads_window_size_only_on_show` (transport): la dimensione arriva a `OpenPluginWindow` solo per `ShowWindow`, non trapela in Update/Close.
- Nuovo `pump_forwards_manifest_window_size` (host): prova end-to-end che un manifest con `window: Some(960×620)` produce un `OpenPluginWindow` con quei valori esatti.
- Test esistenti (`plugin_msg_to_server_maps_correctly`, `pump_forwards_show_window_to_server_tx`) aggiornati per i nuovi campi (`width: None, height: None` nel caso senza dimensione).

## [0.40.11] — 2026-07-10 — fix: recupero JSON per escape orfani in `arguments` OpenRouter

Bug reale segnalato dall'utente: con un modello OpenAI-compatibile via OpenRouter
(osservato con DeepSeek), un turno AI lungo che chiama `show_markdown` a volte falliva con
`[errore AI] decodifica: arguments di 'show_markdown' non è JSON valido: invalid escape at
line 1 column N` — l'INTERO turno veniva scartato (storia troncata, l'utente non riceveva
nulla), pur essendo la risposta del modello altrimenti completa e valida.

**Causa**: `OrFunctionCall::arguments` (`openrouter_backend.rs`) è una stringa JSON che il
MODELLO stesso serializza (a differenza di Claude, dove il tool-use arriva già come oggetto
strutturato dall'API — vedi `claude_backend.rs`, non toccato da questa fix). Il modello a
volte scrive dentro quella stringa un escape Markdown letterale come `\*` (valido in
Markdown) senza raddoppiare il backslash come richiederebbe JSON grezzo (`\\*`) —
`serde_json` rifiuta `\*` con "invalid escape" perché `*` non è uno dei 9 caratteri di
escape validi JSON.

**Fix**: nuova funzione pura `repair_invalid_json_escapes` (OpenRouter-only,
`openrouter_backend.rs`) — scansiona `arguments` carattere per carattere tracciando se si è
dentro una stringa JSON, e quando trova un `\` seguito da un carattere NON tra i 9 escape
validi (`"` `\` `/` `b` `f` `n` `r` `t` `u`), raddoppia il backslash orfano. `from_or_response`
ora: 1) prova il parsing diretto (percorso veloce, nessuna riparazione nel caso comune); 2)
se fallisce, prova la riparazione e ri-parsa; 3) se la riparazione riesce, logga un
`tracing::warn!` e usa il valore riparato; 4) se la riparazione NON basta (JSON rotto oltre
un backslash orfano — troncato, struttura sbagliata...), logga gli `arguments` GREZZI
ORIGINALI (troncati a 2000 char, per non inondare i log) via `tracing::warn!` — prima di
questa fix quella stringa andava persa per sempre al primo errore, rendendo impossibile
diagnosticare future malformazioni diverse — e restituisce lo STESSO `BackendError::Decode`
di prima, costruito dall'errore di parsing ORIGINALE (non da quello del tentativo di
riparazione).

**Scope deliberatamente limitato**: la funzione ripara SOLO la classe "invalid escape"
(backslash + carattere non valido). NON tocca backslash seguiti da un carattere di escape
valido (es. `C:\temp` scritto senza raddoppio produce `\t` = TAB, non backslash letterale) —
quel caso è una corruzione SILENZIOSA diversa (non un errore di decodifica), fuori scope
qui, lasciata per una fix futura se emergerà come problema reale. La funzione non inventa
mai caratteri mancanti (es. una virgoletta di chiusura per una stringa troncata): un
documento genuinamente rotto resta rotto e produce lo stesso errore di prima.

Test aggiunti in `openrouter_backend.rs`: 5 unit test su `repair_invalid_json_escapes`
(`repair_leaves_already_valid_json_untouched`, `repair_leaves_all_valid_escapes_untouched` —
i 9 escape validi non toccati, guardia anti-over-fix — `repair_doubles_stray_backslash_before_invalid_escape_char`,
`repair_does_not_desync_on_escaped_quote` — guardia sul tracking dentro/fuori-stringa —
`repair_does_not_invent_a_closing_quote`) + 1 test di integrazione
(`from_or_response_recovers_from_stray_markdown_escape_in_arguments`, RED confermato contro
il codice pre-fix con lo stesso tipo di errore riportato dall'utente). Il test di
regressione esistente `from_or_response_malformed_arguments_is_a_decode_error` (JSON
genuinamente rotto, non un problema di escape) resta invariato e verde.

## [0.40.10] — 2026-07-10 — fix: `folder:` si estrae dopo `in:`, non prima

Fix di una regressione trovata nella final review del branch `feat/find-folder-scope`
(non individuata dai 5 task originali, ciascuno rivisto singolarmente). `parse_find_input`
(`src/search/query.rs`) estraeva `folder:from-here` dal testo GREZZO dell'input PRIMA di
estrarre `in:"<frase>"` — ma `extract_folder_directive` cerca `folder:` come sottostringa
ovunque nella stringa, quindi una frase di ricerca contenuto che contiene letteralmente il
testo `folder:` (plausibile su JSON/log/codice) veniva scambiata per la direttiva:
- `/find in:"vedi folder: qui"` → errore spurio (`folder: valore non riconosciuto ""`).
- `/find in:"see the folder:from-here now"` → frase di contenuto corrotta a metà
  (`folder:from-here` espunto silenziosamente) E `from_here` attivato per errore.

**Fix**: ordine di estrazione invertito — `in:"<frase>"` viene estratta PRIMA, `folder:` DOPO
sul testo restante, cosicché una frase tra virgolette non venga mai scansionata da
`extract_folder_directive`. Deviazione intenzionale dall'ordine letterale descritto in
`Docs/superpowers/specs/2026-07-10-find-folder-scope-design.md` §3 (approvata dall'utente):
nessun comportamento osservabile documentato cambia, solo l'ordine interno di estrazione.
Aggiunti 2 test di regressione in `query.rs` (`in_phrase_containing_folder_colon_is_content_not_directive`,
`in_phrase_with_from_here_substring_is_not_corrupted`); tutti i test `folder_*`/`in:`
pre-esistenti restano verdi con i loro valori attesi originali (invariati).

Rinforzato anche `find_folder_invalid_value_errors_before_opening_search_window` in
`tests/ws_integration.rs`: il test leggeva solo `Error`+`Done` e non verificava mai
l'assenza di un successivo `SearchOpen` — la mutation testing della final review ha
dimostrato che rimuovere il `continue` del gate in `ws.rs` lasciava il test verde. Ora un
terzo `recv` con timeout di 200ms conferma che nessun messaggio ulteriore (in particolare
nessun `SearchOpen`) arriva dopo il `Done` di errore.

## [0.40.9] — 2026-07-10 — `/find folder:from-here` — restringere la ricerca alla sola cwd

`/find` guadagna una terza direttiva **indipendente e componibile** con le altre due: `/find
[<query>] [in:"<frase>"] [folder:from-here]`. `folder:from-here` restringe la ricerca al **solo**
root Cwd (+ sottocartelle, stesso `max_depth`/`exclude`/fan-out di sempre) — Standard/Cloud/
External non vengono nemmeno **costruiti** come candidati (non semplicemente deprioritizzati o
filtrati dopo). Vale sia per il nome sia per il contenuto (`in:`): è un filtro sui ROOT, ortogonale
a cosa si cerca dentro ai file trovati. Spec:
`Docs/superpowers/specs/2026-07-10-find-folder-scope-design.md`.

### Sintassi e semantica

- `folder:<parola>` è un token SENZA virgolette (a differenza di `in:"<frase>"`), delimitato da
  whitespace. La chiave `folder:` è riconosciuta solo in minuscolo esatto; il valore `from-here` è
  anch'esso case-sensitive (`folder:From-Here` non è `from-here`).
- `folder:<valore-diverso-da-from-here>` ⇒ **errore esplicito**, nessuna ricerca eseguita — a
  differenza di `in:"..."` con virgoletta di chiusura mancante, che degrada silenziosamente a
  query-sul-nome invariata. `folder:` è una chiave riconosciuta: un valore sbagliato su una chiave
  riconosciuta è un errore utente da segnalare, non da inghiottire. `Folder:from-here` (chiave con
  case sbagliata) resta invece testo letterale nella query sul nome, nessun errore — stesso
  trattamento di un `in:` scritto senza virgolette.
- Il gate di validazione vive in `ws.rs`, PRIMA di aprire la finestra di ricerca: nessun
  `SearchOpen` viene mai emesso per un `folder:` invalido. Stesso pattern/stessa posizione del
  controllo esistente `query.is_empty()`. `search::mod::launch` chiama la stessa funzione di
  parsing internamente (pura, nessuna I/O) e in caso di `Err` (che non dovrebbe mai accadere, dato
  che `ws.rs` ha già validato) esce silenziosamente senza emettere nulla — `ws.rs` resta l'unico
  punto che segnala l'errore, `launch` resta difensivo ma non duplica la segnalazione.
- Il fallback `"*"` (query-sul-nome assente ⇒ "qualunque nome", non zero risultati) — già esistente
  per `in:` da solo — è ora **generalizzato**: scatta anche per `folder:from-here` da solo (decisione
  utente in brainstorming).

### Architettura

- **`search/query.rs`**: `parse_find_input` cambia firma da `(Option<String>, String)` a
  `Result<ParsedFind, String>` (`ParsedFind { content, from_here, name_query }`) — breaking change
  interna, entrambi i chiamanti (`ws.rs`, `search::mod::launch`) aggiornati nello stesso lavoro.
- **`search/roots.rs`**: `resolve_roots` guadagna un 4° parametro `only_cwd: bool` — quando `true`,
  lo Step 1 salta del tutto la costruzione dei candidati Standard/Cloud/External (non li filtra
  dopo averli costruiti); tutto il resto (canonicalizzazione, dedup, fan-out, `prune`) resta
  identico e si applica al solo root Cwd.
- **`search/mod.rs`**: `launch` inverte l'ordine — il parsing (`parse_find_input`) ora precede
  `resolve_roots`, perché `only_cwd` (da `parsed.from_here`) serve a `resolve_roots` e non esiste
  finché il parsing non è avvenuto.
- **`ws.rs`**: nuovo check subito dopo `query.is_empty()` — chiama `parse_find_input`, su `Err`
  risponde `Error{RoutingError}` + `Done{exit_code: Some(1)}` e non spawna `launch`.

### Fuori scope (annotato, non implementato qui)

Altri valori di `folder:` oltre `from-here` (il parser è strutturato per accoglierne altri senza
riscrittura, nessuno richiesto ora); una sintassi per restringere a un path arbitrario (non solo
cwd); interazione con `search-paths.json` (puro filtro a runtime sui root, il file di config non
cambia); gestione `folder:` lato Telegram (`/find` resta no-op sul canale, invariato).

**11 test nuovi** (7 in query.rs, 2 in roots.rs, 1 in mod.rs, 1 in ws_integration.rs), **692 test
verdi nel crate** (675 lib + 2 main.rs + 14 ws_integration + 1 doc-test; 4 ignored invariati —
`cargo test -p orchestrator 2>&1 | grep "test result"`), whole-workspace `cargo test` verde;
`cargo clippy --all-targets`: 4 warning pre-esistenti invariati (0 nuovi — `ai_adapter.rs`/
`ws.rs`/`aichat/service.rs`, mai toccati da questa feature).

## [0.40.8] — 2026-07-10 — Ricerca nei contenuti dei file (`/find in:"<frase>"`)

`/find` (v1, solo-nome) guadagna un secondo filtro **indipendente e componibile**: `/find
[<query>] [in:"<frase>"]` cerca anche DENTRO il contenuto dei file, in qualunque ordine con la
query sul nome (`/find *.pdf in:"totale"` ≡ `/find in:"totale" *.pdf`). `/find` senza `in:` resta
**esattamente invariato** — stesso comportamento v1, zero apertura file, zero regressioni (nessun
test v1 esistente modificato). Spec:
`Docs/superpowers/specs/2026-07-10-ricerca-contenuti-design.md`.

### Sintassi e semantica

- `in:"<frase>"` — frase esatta **letterale** (non token), case-insensitive, cercata riga per
  riga: un match che attraversa due righe non scatta. Virgoletta di chiusura mancante (es.
  `in:"TODO` senza chiusura) ⇒ `in:` NON riconosciuto: l'intero input passa invariato al matcher
  sul nome (stesso trattamento di oggi per una query che contiene i due punti).
- Un hit di contenuto mostra path + numero riga + testo della riga (troncato a 200 caratteri); se
  un file ha più righe che matchano, se ne mostra solo la **prima**.

### Due fasi (evita di aprire binari o rallentare l'albero)

Nuovo file di config espandibile **`search-content.json`** (stesso pattern di
`search-paths.json`: auto-generato al 1° avvio con default sensati, poi editabile a mano — vive
accanto in `%LOCALAPPDATA%\dev.lare.terminal\`): `text_extensions`/`binary_extensions` (liste),
`max_file_size_kb` (default 5120), `max_unknown_scan` (default 5000), `version`. Ogni file
incontrato dal walk (dopo il match sul nome, se presente) è classificato per estensione:

- **Text** → **Fase A**: aperto e cercato subito, durante il walk normale — hit live come oggi.
- **Binary** → mai aperto, saltato sempre. **`.pdf` è qui fin da subito** (fuori scope, vedi
  sotto): i PDF reali sono tipicamente compressi/codificati internamente, leggerne i byte grezzi
  quasi mai trova il testo.
- **Unknown** (né l'uno né l'altro, incluso nessuna estensione) → accodato durante la Fase A
  (solo path+nome, zero I/O). **Fase B**, dopo che tutti i walker della Fase A sono esauriti: uno
  sniff dei primi byte di ogni file in coda (assenza di byte null ⇒ sembra testo); se passa lo
  sniff, viene cercato come i file Text. Cap `max_unknown_scan`: oltre quel numero di file in
  coda, la Fase B si ferma e il risultato segnala troncamento (stesso concetto di `result_cap`,
  dimensione diversa).

### Il bug critico trovato in review (Fase A/Fase B originali erano un deadlock permanente)

Il design di riferimento del piano drenava `hit_rx` (risultati Fase A) **fino a esaurimento**
PRIMA di iniziare a drenare `unknown_rx` (coda Fase B) — sequenziale, non concorrente. `unknown_tx`
è un canale **bounded** (capacità 256) e `max_unknown_scan` di default è 5000: su un albero con
più di 256 file "Unknown", il walker che trova il file #257 si blocca per davvero dentro
`unknown_tx.blocking_send` (nessuno sta ancora drenando quel canale) — non ritorna mai da
`walk_root`, quindi non droppa mai il proprio `hit_tx`, quindi `hit_rx.recv()` non vede mai "tutti
i sender spariti", quindi la Fase A non finisce **mai**, quindi la Fase B non parte **mai**, quindi
nessuno drena mai il canale che sta bloccando il walker. Deadlock permanente, silenzioso (nessun
panic, nessun errore — la ricerca resta appesa per sempre). Non un errore dell'implementatore: un
difetto del design originale stesso, trovato in code review prima del merge.

**Fix** (`SearchEngine::run`): un unico loop `tokio::select!` drena **entrambi** i canali
**concorrentemente** — gli hit di Fase A vengono ancora inoltrati subito come `SearchHit`; i
candidati Unknown vengono solo **accumulati** in un `Vec` (zero I/O, serve solo a svuotare il
canale così nessun walker resta mai bloccato). Il lavoro vero (sniff+cerca, Fase B) parte **dopo**
che il loop esce — il che succede solo quando **entrambi** i canali risultano chiusi, cioè ogni
walker è genuinamente terminato. La review ha anche trovato un secondo bug nella prima bozza del
fix: al raggiungimento di `result_cap` va interrotto il drain di **entrambi** i canali (non solo
`hit_rx`) — altrimenti un walker ancora attivo può bloccarsi per sempre su `hit_tx.blocking_send`
(anch'esso bounded a 256), la stessa classe di deadlock spostata sull'altro canale. Nuovo test di
regressione `content_search_does_not_deadlock_on_many_unknown_files` (300 file Unknown, oltre la
capacità storica del canale; wrappato in `tokio::time::timeout` per fallire in pochi secondi
invece di appendere la test suite per sempre in caso di regressione futura).

### Fase B ora onora la pausa (trovato nella review finale sull'intero branch)

La Fase B controllava `cancel.is_cancelled()` ma non chiamava mai `gate.wait_while_paused(...)` —
`PauseGate` era riusato solo dai walker di Fase A. Deviazione dalla spec §5 ("stesso `id`/
`CancellationToken`/`PauseGate` riusati") e bug utente reale: il pulsante ⏹ resta visibile per
tutta la Fase B (si nasconde solo su `search:done`), quindi mettere in pausa durante una Fase B
lunga (fino a `max_unknown_scan` = 5000 file) cambiava l'etichetta di stato ma non fermava nulla —
gli hit continuavano ad arrivare. Fix: il check di pausa entra nella stessa chiusura
`spawn_blocking` per-file di Fase B, PRIMA del check di cancellazione (stesso ordine di
`walk_root`: un resume durante una pausa deve comunque lasciar uscire un cancel arrivato nel
frattempo). Nuovo test `phase_b_respects_pause_gate` — usa un corpus di soli file Unknown (Fase A
non emette mai un hit diretto per quelli) così il *primo* `SearchHit` è prova inequivocabile che la
Fase B è partita: mette in pausa esattamente lì, senza indovinare un timing.

### Architettura

- **`protocol` 0.14.1**: `ServerMsg::SearchHit` guadagna `line`/`snippet` opzionali (additivo).
- **`orchestrator/src/search/`**: `query::parse_find_input` (estrae `in:"..."` dall'input grezzo,
  ovunque compaia); nuovo `content.rs` (`ContentConfig`, `classify_extension`, `looks_like_text`/
  `sniff_file`, `ContentMatcher::find_first_match`); `walk.rs` diventa content-aware (`Text` aperto
  e cercato subito, `Unknown` accodato su `ContentSearch::unknown_tx`, `Binary` mai aperto);
  `mod.rs` (`SearchEngine::run`, Fase A/B come sopra; `SearchContext`/`launch` fanno lo split
  `in:"..."` e costruiscono `ContentMatcher`/`ContentConfig` quando presente).
- **`ws.rs`** — nessun cambio strutturale: `/find` resta un solo comando end-to-end (più lungo in
  presenza di `in:`, ma stesso ciclo di vita/cancellazione/pausa).

### Fuori scope (annotato, non implementato qui)

Regex nel contenuto (`in:` resta solo frase letterale); ricerca dentro PDF (aggancio futuro
esplicito quando esisterà un estrattore PDF→Markdown, memoria `open-doc-to-markdown-button`);
jump-to-line all'apertura di un hit; "mostra nella cartella" per un hit di contenuto; gestione
`/find` lato Telegram (già no-op, resta no-op); toggle per disattivare la Fase B (sempre attiva
quando c'è `in:`, YAGNI).

**38 test nuovi** (622 → 660 lib), **676 test verdi nel crate** (660 lib + 2 main.rs + 13
ws_integration + 1 doc-test; 4 ignored invariati), clippy invariato (4 warning pre-esistenti, 0
nuovi — vedi `ai_adapter.rs`/`ws.rs`/`aichat/service.rs`, mai toccati da questa feature).

## [0.40.7] — 2026-07-10 — `/help`: sintassi completa di `/find`

Anticipa la documentazione della sintassi `/find` (nome + `in:"<frase>"` per contenuto) prima
dell'implementazione — brainstorm della ricerca-contenuti approvato dall'utente, spec in arrivo
(`Docs/superpowers/specs/2026-07-10-ricerca-contenuti-design.md`). `in:"..."` non è ancora
funzionante a questo commit: la voce in `/help` descrive la sintassi target, non un comportamento
già implementato — nota per non confondere un futuro riferimento a questo CHANGELOG. Nessun test
nuovo (stesso motivo del fix 0.40.6: contenuto Markdown statico). 631 test verdi, clippy
invariato.

## [0.40.6] — 2026-07-09 — fix: `/find` mancante in `HELP_MARKDOWN`

Segnalato dall'utente: `/help` elencava `open`/`web`/`show`/`nowin`/`library`/`reset` ma non
`/find` — dimenticanza, non un bug di dispatch (`/find` funziona, gestito in `ws.rs` non in
`core::handle_slash`, per questo facile da perdere aggiornando `HELP_MARKDOWN` in `core.rs`).
Aggiunta una riga (`/find <query>` — ricerca file dal vivo). Nessun test nuovo (contenuto Markdown
statico, il test esistente `slash_help_produces_open_window_and_done` non asserisce il testo
letterale). 631 test verdi, clippy invariato.

## [0.40.5] — 2026-07-09 — llms.json: `base_url` custom anche per provider `"openrouter"`

Seguito del fix 0.40.4: risolto l'HTTP 400, ma `deepseek-direct` (shim Anthropic-compatibile di
DeepSeek) si è rivelato immaturo su un fronte diverso — leaka a volte le proprie tool-call native
("DSML", `<|DSML|tool_calls>...`) come testo grezzo invece di tradurle in blocchi `tool_use`
Anthropic strutturati (bug non-deterministico, confermato riscontrato anche da altri progetti
agentic contro lo stesso modello via endpoint diversi — non un bug lato Lare). DeepSeek espone
anche un endpoint OpenAI-compatibile DIRETTO (`https://api.deepseek.com/chat/completions`, stessa
auth `Authorization: Bearer`) — stesso identico wire già parlato da `OpenRouterBackend`
(`to_or_tools`/`to_or_messages`, validati contro una fixture reale), solo un host diverso da
`openrouter.ai`. `HttpOpenRouterClient::with_base_url` esisteva già (usato nei test); mancava solo
la plumbing in `build_adapter` per onorare `ProviderConfig::base_url` anche sul ramo
`"openrouter"` (prima ignorato lì, hardcoded su `HttpOpenRouterClient::new`). Nessuna logica
DeepSeek-specifica nel codice: funziona per qualunque endpoint OpenAI-compatibile con un
`base_url` diverso. 1 test nuovo, 631 verdi, clippy invariato. Editing manuale di `llms.json` —
nuova entry `deepseek-openai-direct` (`provider: "openrouter"`, `base_url:
"https://api.deepseek.com"`, `model: "deepseek-v4-pro"`), impostata attiva al posto di
`deepseek-direct`.

## [0.40.4] — 2026-07-09 — fix: DeepSeek 400 su `web_fetch`, tutte le richieste NL fallivano

Bug reale osservato dal vivo dopo la 0.40.3: con `deepseek-direct` attivo, i comandi verso il SO
funzionavano ma OGNI comando in linguaggio naturale falliva con `HTTP 400: unknown variant
web_fetch_20260209, expected web_search_20250305 or web_search_20260209`. Causa: la ricerca web è
abilitata di default (`Config::default().web_search_enabled`), quindi `agent::tools_for` aggiunge
SEMPRE entrambi i tool server-side Anthropic (`web_search` + `web_fetch`) a ogni turno; lo shim
Anthropic-compatibile di DeepSeek (raggiunto via `base_url` custom, 0.40.3) implementa solo i
tipi `web_search_*`, non conosce affatto `web_fetch` — reject immediato dell'intera richiesta,
prima ancora che il modello veda il prompt. Root cause: `ClaudeBackend` presumeva superficie
Anthropic COMPLETA per qualunque `provider: "anthropic"`, ma un `base_url` custom è garanzia solo
di compatibilità di WIRE, non di feature-parity. Fix: nuovo campo `ProviderConfig::
web_fetch_supported` (default `true`, invariato per Anthropic reale); `ClaudeBackend::
with_web_fetch_supported(false)` scarta `web_fetch` dalla richiesta effettiva, lasciando
`web_search` intatto — stesso punto/stessa filosofia di `OpenRouterBackend::to_or_tools`, che già
scarta i `ToolSpec::Server` senza equivalente OpenAI. Editing manuale di `llms.json` per attivarlo
sull'entry `deepseek-direct` (nessun campo UI in questa slice). 4 test nuovi (2 in
`claude_backend`, 2 in `llms_config`), 630 verdi, clippy invariato.

## [0.40.3] — 2026-07-09 — llms.json: `base_url` custom per provider `"anthropic"`

Permette di puntare un provider `"anthropic"` a un endpoint Anthropic-COMPATIBILE diverso da
`api.anthropic.com` — caso concreto: DeepSeek espone lo stesso wire `/v1/messages` (header
`x-api-key`/`anthropic-version`, stesso body) sotto un proprio host, quindi si può usare la sua
API key DIRETTAMENTE, senza passare da OpenRouter. Nessun nuovo backend serve: `ClaudeBackend`/
`HttpMessagesClient` erano già parametrizzati su un `base_url` arbitrario
(`HttpMessagesClient::with_base_url`); mancava solo il campo in `ProviderConfig` e la plumbing
in `build_adapter`. Nuovo campo `base_url: Option<String>` (default assente = comportamento
invariato, `api.anthropic.com` reale) — ignorato per `provider: "openrouter"` (host fisso).
Editing manuale di `llms.json` per ora (nessun campo nella UI `/config` → LLM in questa slice).
3 test nuovi, 622 verdi, clippy invariato.

## [0.40.2] — 2026-07-08 — AI Chat: SILENCE tollerante a punteggiatura finale

Bug osservato dal vivo (schermata utente): il sentinel di silenzio dell'auto-partecipazione
(`SILENCE`) veniva riconosciuto solo per match esatto case-insensitive. Un modello chat-tuned ha
risposto `"SILENCE."` (con punto finale) invece della parola nuda — il match falliva, il testo
`"SILENCE."` veniva trattato come un contributo genuino e pubblicato in chat, rumore visibile che
innescava a sua volta ulteriori cicli di auto-partecipazione sulle altre macchine (che reagivano
a quel messaggio come a un contributo vero). Fix: `trim_end_matches(['.', '!', '?'])` toglie SOLO
UNA eventuale punteggiatura finale prima del confronto — mai un match di sottostringa, quindi
"silenzioso" non diventa mai "silence" per nessun trimming (verificato con test dedicato). Nota
di scope: questo fix chiude solo la coda rumorosa (SILENCE malriconosciuto pubblicato come
messaggio vero); non tocca la cascata AI↔AI spontanea in sé (un'altra macchina che risponde a un
`@ai` altrui appena eseguito), comportamento intenzionale della chat "partecipativa" che l'utente
sta deliberatamente studiando dal vivo — resta fuori scope per una futura sessione di design.

## [0.40.1] — 2026-07-08 — AI Chat: sopprimi auto-partecipazione su invocazione altrui

Bug osservato dal vivo: un umano invoca esplicitamente `@ai` (bare, riguarda solo la propria
macchina) — con `ai_autoparticipate` attivo altrove, ANCHE l'AI di macchine non targetate
rispondeva, duplicando la risposta. Causa: il gate anti-double-fire (Slice 2b) sopprimeva
l'auto-partecipazione solo quando l'invocazione produceva un `InvokeLocalAi` PER la macchina che
valutava — se l'invocazione targetava un'ALTRA macchina (`@ai` bare da remoto, o
`@<altro-label>-ai` da chiunque), il gate non scattava. Fix: il gate ora guarda se il messaggio
ERA un'invocazione esplicita di qualunque tipo, non chi essa targetasse — coerente con quanto il
system prompt di auto-partecipazione già dichiara ("nessuno ti ha invocato esplicitamente").
Inverte un test che documentava il comportamento precedente come scelto (non era un residuo, era
una decisione rivista con l'utente). Spec:
`Docs/superpowers/specs/2026-07-08-aichat-autoparticipate-explicit-invocation-design.md`.

## [0.40.0] — 2026-07-08 — AI Chat: memoria persistente per-macchina

Prima volta che un'AI partecipante di AI Chat ha una memoria che sopravvive fra sessioni di chat
diverse. Un file `%LOCALAPPDATA%\dev.lare.terminal\memory-{label_base}.md` (per macchina, mai
condiviso) viene letto ad ogni turno e incluso nel system prompt; l'AI può scegliere di
accodarci una nuova nota scrivendo una riga `MEMORIA: <nota>` nella propria risposta — SEMPRE una
scelta dell'AI, mai un'estrazione deterministica di una richiesta umana, e SEMPRE visibile nel
messaggio pubblicato in chat (il companion vede cosa la propria AI ha scelto di ricordare).
Nessun limite di crescita del file in questa slice (YAGNI). Spec:
`Docs/superpowers/specs/2026-07-07-aichat-persistent-memory-design.md`.

### Added

- `ai_adapter.rs`: `memory_file_path`/`memory_file_path_with_base`, `memory_block` — `LlmAdapter`
  legge il proprio file di memoria da sé (deriva `label_base` da `my_ai_label`, già ricevuto —
  nessuna nuova firma di trait dopo il costo già pagato in Slice 1).
- `aichat/service.rs`: `extract_memoria_marker` (pura), `Effect::PersistMemory`, campo
  `memory_write_lock: Arc<Mutex<()>>` (contro il read-modify-write concorrente quando
  `chat_reply`/`chat_autoparticipate` sono in volo insieme — non sono mutuamente esclusi).

## [0.39.2] — 2026-07-07 — AI Chat: fix mirroring d'identita' (ruoli veri + system prompt calcolato)

Bug osservato dal vivo (chat multi-provider, due macchine con provider diversi attivi): un
peer si e' auto-identificato come il provider/la macchina dell'ALTRO peer, ripetutamente. Causa
in due parti: (1) `CHAT_SYSTEM_PROMPT`/`AUTOPARTICIPATE_SYSTEM_PROMPT` erano `const` fisse senza
alcuna identita' del chiamante; (2) la history veniva appiattita in un solo turno `user` di
testo piatto, senza distinguere le proprie righe passate da quelle altrui (nessun ruolo
`assistant`). Fix: system prompt calcolato (label+provider, sempre fresco) + history a ruoli
veri (`build_ai_history`, sostituisce `format_transcript`) con coalescenza dei turni consecutivi
dello stesso ruolo (l'API rifiuta `[user, user]` con HTTP 400). Nessun file di memoria
persistente in questa slice (pianificato separatamente).

### Changed

- `AiAdapter::chat_reply`/`chat_autoparticipate`: firma cambiata, prendono `my_ai_label: &str` +
  `history: &[Message]` invece di `transcript: &str`. Impatta `StubAdapter`/`LlmAdapter`, i
  fake di test in `aichat/service.rs` e in `telegram/channel.rs` (solo firma, nessun cambio di
  comportamento nei fake — quest'ultimo era stato mancato dal censimento iniziale del piano,
  trovato via grep esaustivo prima di questo commit).
- `Effect::InvokeLocalAi`/`Effect::AutoParticipate`: il campo `transcript: String` diventa
  `history: Vec<Message>` + nuovo campo `my_ai_label: String`.
- `format_transcript` (Slice 1a) rimossa, sostituita da `build_ai_history`.

## [0.39.1] — 2026-07-07 — `llms.json` si sposta in `%LOCALAPPDATA%` (Slice 5)

**Correzione della Slice 4**, non ancora osservata da nessun utente reale (stesso giorno):
`llms_config::resolve_path` non risolve più `llms/llms.json` contro la cartella di lancio
dell'orchestrator, ma contro `%LOCALAPPDATA%\dev.lare.terminal\llms.json` (fallback
`.lare-data\llms.json`) — stessa forma di `aichat.json`/`search-paths.json`. Motivo: la Slice 5
aggiunge un tab `/config` (crate `ui`, processo Tauri separato) che deve scrivere nello stesso
file che l'orchestrator legge al proprio avvio — i due processi non condividono necessariamente
una cartella di lancio, ma condividono sempre lo stesso `%LOCALAPPDATA%`.

### Changed

- `llms_config::resolve_path(env_override: Option<String>) -> PathBuf` — **rimosso** il parametro
  `launch_dir: &Path`. Se avevi già creato a mano un `llms/llms.json` nella cartella di lancio
  (Slice 4), spostalo in `%LOCALAPPDATA%\dev.lare.terminal\llms.json`.
- `.gitignore`: rimossa l'entry `llms/` (non più applicabile — il file non vive più nell'albero
  del repo).

### Added

- `llms_config::default_path` (privata): risoluzione del path di default, parametrizzata su
  `local_appdata: Option<String>` per essere testabile senza mutare variabili d'ambiente reali.
  4 test (override esplicito vince, default con `LOCALAPPDATA` impostata, fallback `.lare-data`
  quando assente/vuota).

## [0.39.0] — 2026-07-07 — `llms.json` wired in `main.rs` (Slice 4 completa)

**Prima volta che OpenRouter è selezionabile in produzione.** `main.rs` legge `llms/llms.json`
(opzionale) all'avvio: se presente e valido, usa il provider indicato da `active` (Anthropic
diretto o OpenRouter/DeepSeek); altrimenti (file assente, illeggibile, o non valido) usa
`default_ai_adapter()` — ESATTAMENTE il branch `ANTHROPIC_API_KEY`/`StubAdapter` di sempre,
estratto invariato in una funzione. Retrocompatibilità bit-per-bit garantita per costruzione.

### Added

- `main.rs`: wiring di `llms_config::{resolve_path, load, build_adapter}` nel blocco di selezione
  AI adapter. `.gitignore`: nuova entry `llms/` (il JSON porta le key inline).

### Changed

- Il branch odierno (`ANTHROPIC_API_KEY`/`StubAdapter`) è ora la funzione `default_ai_adapter()`
  — stessa logica, stessi messaggi di log, nessuna riga di comportamento cambiata.

## [0.38.9] — 2026-07-07 — `llms_config`: schema e validazione di `llms/llms.json` (Slice 4)

Primo passo della Slice 4 (`Docs/superpowers/specs/2026-07-06-openrouter-tooluse-adapter-design.md`
§6, §8): il modulo che legge, valida, e assembla l'`AiAdapter` a partire da un `llms/llms.json`
opzionale — un solo provider attivo per macchina, Anthropic diretto o OpenRouter. Puramente
additivo — `main.rs` non lo chiama ancora (Task 2).

### Added

- `llms_config.rs`: `LlmsConfig`/`ProviderConfig` (schema), `resolve_path`/`load` (mirror di
  `telegram::settings`), `find_active`/`resolve_api_key` (validazione, MAI logga una key reale),
  `build_adapter` (assembla `LlmAdapter`+`ClaudeBackend` o `LlmAdapter`+`OpenRouterBackend` in
  base al campo `provider`). 14 test.

## [0.38.8] — 2026-07-07 — e2e reale contro DeepSeek (Slice 3 completa)

### Added

- Test `#[ignore]` `real_e2e_tool_use_roundtrip_against_deepseek`: valida il round-trip di
  tool-use COMPLETO contro OpenRouter/DeepSeek reale (`OPENROUTER_API_KEY`) — il modello chiama
  `run_in_session`, riceve un risultato fabbricato, risponde in testo usandolo. Non eseguito di
  default (richiede rete/chiave vere), verificato manualmente prima del commit.

## [0.38.7] — 2026-07-07 — `HttpOpenRouterClient`: trasporto reale per OpenRouter (Slice 3)

Primo passo della Slice 3 (`Docs/superpowers/specs/2026-07-06-openrouter-tooluse-adapter-design.md`
§7 punti 4-6, §8): `HttpOpenRouterClient` — un `OpenRouterClient` che parla davvero con
`https://openrouter.ai/api/v1/chat/completions` via `reqwest`. Puramente additivo — nessun wiring
in `main.rs`, `OpenRouterBackend` resta non selezionabile in produzione.

### Added

- `openrouter_backend::HttpOpenRouterClient`: `new`/`with_base_url`/`with_attribution`
  (header opzionali `HTTP-Referer`/`X-Title`, raccomandati da OpenRouter ma mai richiesti),
  `impl OpenRouterClient`. Timeout singolo (120s, non-streaming — molto più semplice del doppio
  idle/totale di `HttpMessagesClient`, qui non c'è SSE). 5 nuovi test contro un server TCP locale
  (nessuna nuova dipendenza di mocking): auth header, mapping errore HTTP, mapping errore di rete,
  header di attribuzione presenti/assenti.

## [0.38.6] — 2026-07-06 — `OpenRouterBackend` assemblato (Slice 2 completa)

Secondo `ChatBackend` completo (dopo `ClaudeBackend`, Slice 1): `OpenRouterBackend` assembla
`to_or_messages`/`to_or_tools`/`from_or_response` dietro `Arc<dyn OpenRouterClient>`. **Ancora non
wired in `main.rs`** — nessun modo di selezionarlo in produzione (arriva in Slice 3/4: trasporto
HTTP reale + `llms.json`). Il test capstone rigioca l'INTERO round-trip a 2 turni della fixture
reale DeepSeek/OpenRouter attraverso `OpenRouterBackend`, verificando che la richiesta del turno 2
abbia esattamente la forma che la fixture ha dimostrato essere accettata dal provider.

### Added

- `openrouter_backend::OpenRouterBackend` (struct + `impl ChatBackend`). V1 non-streaming: `on_text`
  chiamata una sola volta con tutto il testo, prima di ritornare. 5 nuovi test (incl. il round-trip
  end-to-end sulla fixture reale).

## [0.38.5] — 2026-07-06 — `OpenRouterClient` (seam) + `FakeOpenRouterClient` (Slice 2)

### Added

- `openrouter_backend::OpenRouterClient` (trait) + `OrError` (mirror di `MessagesError`) +
  `FakeOpenRouterClient` (test double, stesso pattern di `FakeMessagesClient`). Nessun trasporto
  HTTP reale ancora — arriva in Slice 3 (`HttpOpenRouterClient`).

## [0.38.4] — 2026-07-06 — `from_or_response`: traduzione pura risposta → BackendTurn (Slice 2)

### Added

- `openrouter_backend::from_or_response`: `finish_reason` → `TurnStop` (`length`→`MaxTokens`,
  `content_filter`→`Refused{category:None}`, altro→`Normal`); `tool_calls[].function.arguments`
  (stringa JSON) → `Block::ToolUse.input`, parsing fallito → `BackendError::Decode` esplicito
  (mai un drop silenzioso). 6 nuovi test.

## [0.38.3] — 2026-07-06 — `to_or_tools`: traduzione pura definizioni tool (Slice 2)

### Added

- `openrouter_backend::to_or_tools`: `ToolSpec::Custom` → `OrTool` (`parameters` = lo stesso
  `input_schema` JSON Schema, nessuna conversione); `ToolSpec::Server` (tool server-side
  Anthropic, es. `web_search`) scartato con `tracing::warn!`, non tradotto (nessun equivalente
  OpenRouter). 3 nuovi test.

## [0.38.2] — 2026-07-06 — `to_or_messages`: traduzione pura storia → OpenAI-compatibile (Slice 2)

### Added

- `openrouter_backend::to_or_messages`: traduce `&[Message]` (+ system prompt opzionale) nella
  forma OpenAI-compatibile. Il caso `Block::ToolResult` multiplo nello stesso turno si SROTOLA in
  N messaggi `role:"tool"` separati (OpenAI non ha un turno multi-risultato come Anthropic);
  `is_error` si ripiega in `content` (nessun campo OpenAI dedicato). 5 nuovi test.

## [0.38.1] — 2026-07-06 — Tipi wire OpenAI-compatibili per `OpenRouterBackend` (Slice 2)

Primo passo della Slice 2 (`Docs/superpowers/specs/2026-07-06-openrouter-tooluse-adapter-design.md`):
i tipi wire (`OrMessage`/`OrToolCall`/`OrFunctionCall`/`OrTool`/`OrFunctionDef`/`OrRequest`/
`OrResponse`/`OrChoice`) per un secondo `ChatBackend` (OpenRouter/DeepSeek), validati contro una
risposta reale catturata (`Docs/superpowers/fixtures/2026-07-06-deepseek-openrouter-real-roundtrip/`),
non solo dedotti dalla documentazione. Puramente additivo — nessun backend ancora assemblato.

### Added

- `openrouter_backend.rs`: tipi `Or*` per l'API OpenAI-compatibile (`chat/completions`). 4 test
  che confrontano serializzazione/deserializzazione byte-per-byte (via `serde_json::Value`) con
  la fixture reale, incluso il caso `tool_calls` assente (non `null`) e `arguments` come stringa
  JSON.

### Fixed

- Deviazione dal piano originale scoperta dal test contro la fixture reale (RED→GREEN, non
  un'assunzione): `OrMessage::content` NON usa `#[serde(skip_serializing_if = "Option::is_none")]`
  (a differenza di `tool_calls`/`tool_call_id`) — la fixture reale (`turn2-request.json`) mostra
  `"content": null` ESPLICITO nel messaggio `assistant` con `tool_calls`, non il campo assente. Il
  piano originale annotava questo campo con `skip_serializing_if` per tutti e tre gli `Option`
  allo stesso modo; il test `or_request_turn2_serializes_exactly_like_the_captured_fixture` fallisce
  se lo si reintroduce.

## [0.38.0] — 2026-07-06 — `ClaudeAdapter` → `LlmAdapter` (backend-agnostico)

Il loop di tool-use (gate di conferma, dispatch dei tool, cap iterazioni, cancellazione, igiene
storia) non è più cablato su Anthropic: `LlmAdapter` lo guida chiamando `ChatBackend::send_turn`,
e `ClaudeBackend` (0.37.3) è la prima implementazione. **Zero cambio di comportamento**: stesso
wire Anthropic sul filo, stesso fallback `ANTHROPIC_API_KEY`/`StubAdapter` in `main.rs`. Prepara
il terreno per un secondo provider (OpenRouter, slice successiva) senza duplicare il loop.
Precisazione: `chat_reply`/`chat_autoparticipate` ora instradano anche loro su
`ChatBackend::send_turn` (`stream: true` sul trasporto interno), mentre prima usavano una
chiamata non-streaming diretta — testo restituito identico, `respond()` era già streaming
prima e dopo.

### Changed

- `ai_adapter.rs`: `ClaudeAdapter` rimosso, sostituito da `LlmAdapter { backend: Arc<dyn
  ChatBackend>, provider_name: String }` — un solo tipo adapter, parametrizzato per
  composizione invece che per tipo concreto (Liskov: qualunque `ChatBackend` valido produce un
  `LlmAdapter` valido).
- `main.rs`: la selezione dell'AI adapter costruisce `ClaudeBackend` + `LlmAdapter` invece di
  `ClaudeAdapter` direttamente — stesso branch `ANTHROPIC_API_KEY`/`LARE_AI_MODEL`, comportamento
  bit-per-bit invariato.
- Tutti i test di loop-mechanics di `ai_adapter.rs` (gate, dispatch, cap, cancellazione, igiene
  storia) migrati da `FakeMessagesClient`/`ClaudeAdapter` a `FakeChatBackend`/`LlmAdapter` —
  stesse asserzioni, stesso comportamento verificato, zero test rimossi o indeboliti (incluso
  `web_search_mode_includes_server_tools`, guardia di regressione per la scelta `&[ToolSpec]`
  invece di `&[ToolDef]` nel trait `ChatBackend`).

## [0.37.3] — 2026-07-06 — `ClaudeBackend`: prima implementazione di `ChatBackend`

### Added

- `claude_backend.rs`: `ClaudeBackend`, un `ChatBackend` quasi-passthrough sopra
  `Arc<dyn MessagesClient>` — `Block`/`ToolSpec` sono già la forma wire Anthropic, quindi qui
  non c'è vera traduzione, solo l'assemblaggio della richiesta e la mappatura di `stop_reason`
  nel vocabolario neutro `TurnStop`. Puramente additivo: `ClaudeAdapter` resta wired in
  `main.rs`, `ClaudeBackend` non è ancora usato da nessuno.

## [0.37.2] — 2026-07-06 — Nuovo seam `ChatBackend` (fondamenta per multi-LLM, ADR-014)

Primo passo del meccanismo di tool-use cross-provider (design in
`Docs/superpowers/specs/2026-07-06-openrouter-tooluse-adapter-design.md`): il trait
`ChatBackend` isola "come parlare con un provider LLM" dal loop di tool-use. Puramente
additivo — nessun codice esistente tocca ancora questo trait.

### Added

- `chat_backend.rs`: trait `ChatBackend` (`send_turn`), `BackendTurn`/`TurnStop`/`BackendError`
  (vocabolario neutro, non le stringhe `stop_reason`/`finish_reason` di un provider specifico),
  `FakeChatBackend` (test double, stesso pattern di `FakeMessagesClient`).

## [0.37.1] — 2026-07-06 — Fix: il destinatario impone il consenso prima del contenuto (Slice 2a)

Il review finale del branch (pre-merge) ha trovato che `PendingShare::AwaitingDecision` non
distingueva "offerta non ancora decisa" da "accettata, in attesa del contenuto": `share_consent`
non transizionava mai l'entry sull'accettazione, e `handle_share_data` accettava `ChatMsg::ShareData`
per qualunque entry `AwaitingDecision`. Un mittente non-conforme o buggato (già ammesso nella stanza)
poteva quindi mandare `ShareData` subito dopo `ShareOffer`, prima che l'umano decidesse, e il file
veniva scritto in Library senza consenso — viola spec §8 ("niente contenuto prima del consenso").

### Fixed

- `PendingShare` guadagna un terzo stadio, `AwaitingContent` — simmetrico allo stadio già
  esistente lato mittente (`OutgoingShareStage::AwaitingContent`). `share_consent` transiziona
  ESPLICITAMENTE `AwaitingDecision` → `AwaitingContent` sull'accettazione (prima non modificava
  l'entry). `handle_share_data` ora richiede `AwaitingContent`: un `ShareData` prematuro (entry
  ancora `AwaitingDecision`) è un no-op silenzioso, non un'accettazione implicita.
- `handle_share_data` rivalida la dimensione reale del contenuto contro `MAX_SHARE_SIZE_BYTES`
  (mancava lato destinatario — il mittente la fa già in `handle_share_content`): sopra il cap,
  rimuove l'entry e rifiuta attivamente via `auto_reject_share` invece di scrivere in Library.
- `ServiceEvent::ShareExpiryTimeout` e il replay in `SetServerTx` coprono il nuovo stadio
  `AwaitingContent` (stesso trattamento degli altri due: notifica il mittente su scadenza,
  nessun re-invio del banner di consenso già superato sul replay).
- Corretti due commenti di documentazione non aggiornati trovati dal review (`ShareDocumentRequested`
  menzionava ancora `rel_path` come "non usato"; `handle_share_content` riportava "documento non
  più disponibile" per un fallimento di dimensione, non di disponibilità).

## [0.37.0] — 2026-07-06 — Library "Share with": trasferimento contenuto (Slice 2a)

Il documento accettato arriva DAVVERO nella Library del destinatario. `ChatMsg::ShareData`
(peer↔peer, contenuto vero). `PendingShare` da struct a enum a due stadi
(`AwaitingDecision`/`AwaitingUiWrite`, spec §9.1). `OutgoingShare` guadagna `rel_path` e uno stadio
(`AwaitingAccept`/`AwaitingContent`) — l'accettazione non risolve più subito l'esito verso l'UI del
mittente insieme alla rimozione dell'entry: ora chiede il contenuto vero alla UI
(`ServerMsg::ShareContentRequest`) e lo inoltra al destinatario solo dopo averlo ricevuto. Fallimento
del recupero contenuto (documento cancellato/spostato) riportato SUBITO al mittente, non dopo la
scadenza di 24h. Nessun nuovo messaggio wire per i fallimenti lato destinatario (scrittura fallita,
contenuto mai arrivato) — restano silenziosi, scadono naturalmente. Spec:
`Docs/superpowers/specs/2026-07-06-library-share-slice2a-content-transfer-design.md`.

## [0.36.0] — 2026-07-05 — Library "Share with": UI (Slice 1a-ui)

Collega il backend di Slice 1a (0.35.0) alla UI: pulsante "Condividi" in Library,
banner di consenso in AI Chat, riga di esito nel pannello cursore. Spec:
`Docs/superpowers/specs/2026-07-05-library-share-ui-slice1a-design.md`.

### Added

- `aichat::service::OutgoingShare { target_label, doc_name }` sostituisce il
  precedente `String` grezzo come valore di `outgoing_shares` — `doc_name` serve
  solo a comporre un messaggio di esito leggibile, nessuna decisione ne dipende.
- `ServerMsg::ShareResult` ora riporta `doc_name` (protocol 0.13.0).

---

## [0.35.0] — 2026-07-05 — Library "Share with": consenso + scadenza 24h (Slice 1a, backend)

Prima slice della feature "Share with" (`Docs/superpowers/specs/2026-07-02-library-share-with-design.md`):
round-trip completo di offerta/consenso/scadenza tra due macchine via AI Chat, **nessun
trasferimento di contenuto ancora** (arriva in Slice 2) e **nessuna UI ancora** (arriva in una
slice successiva — questa è la sola parte backend, provata via test + un loopback TCP reale).

### Added

- `aichat::wire::ChatMsg::ShareOffer/Accept/Reject/Expired` (peer↔peer).
- `aichat::relay::route_to_label` — routing unicast per `label_base`, gemello di
  `fan_out_targets` (broadcast).
- `aichat::service::AiChatService`: `pending_shares`/`outgoing_shares`, timer-effect
  `StartShareExpiryTimer`/`ShareExpiryTimeout` (24h, unico orologio per "mai risposto" — spec
  §9.1), cap di 1000 trasferimenti pendenti (auto-rifiuto oltre soglia).
- `SetServerTx` ri-emette `ShareRequest` per ogni offerta ancora pendente alla riconnessione
  della UI — chiuso lo stesso gap di replay già risolto per il gate di ammissione (0.34.2), questa
  volta prima ancora che la feature avesse una UI.
- Dispatch `ws.rs`: `ClientMsg::ShareDocument`/`ShareConsent`.

### Fixed

- `request_share` verifica ora anche la raggiungibilità (link TCP attivo), non solo la scoperta
  del peer — evita un'offerta "persa nel nulla" verso un peer scoperto ma non direttamente
  collegato (rilevante in topologie a 3+ macchine dove solo il server ha link diretti con tutti i
  client; il relay lato server per client→client resta debito per una slice futura, spec §4.1).
  Emerso dalla review finale della slice (2026-07-05), prima del merge — corretto nella stessa
  versione 0.35.0 anziché aprirne una nuova.

**Debiti annotati (fuori scope qui, spec §6/§9.1):** flag `auto_answer_share_requests` (Slice 4);
`ChatMsg::ShareData`/scrittura su disco (Slice 2); `ShareTarget::All` (Slice 3, oggi risponde
subito `Failed`).

## [0.34.2] — 2026-07-04 — AI Chat: fix "tutto fermo" — `SetServerTx` ri-emette il gate di ammissione perso

Bug reale (non solo diagnostica), riprodotto dal vivo su 2 macchine (`rumpleteazer`/`skimble`):
il connect verso il server eletto parte da solo all'avvio dell'orchestrator (scoperta UDP →
`decide_and_connect`), quasi sempre **prima** che l'umano apra la finestra AI Chat (webview
separato, aperto a parte). `begin_join_gate`/`request_admission` emettevano il loro `ToUi`
nell'istante del connect: con `server_tx` ancora `None` in quel momento, l'effetto veniva
scartato in silenzio — e nessun replay esisteva (gap documentato nel commento di `SetServerTx`
fin dalla 0.33.0, mai chiuso). Risultato: il nuovo arrivato restava bloccato in
`Deciding`/`Pending` a tempo indeterminato, senza alcun prompt visibile né modo di recuperarlo
aprendo la finestra più tardi — confermato dal vivo anche aprendo la finestra 8s **dopo**
l'elezione.

### Fixed

- `aichat::service::SetServerTx` ora ri-emette il gate corrispondente se `self_admission` è
  `Deciding` (`AiChatJoinPrompt`) o `Pending` (`AiChatPending`) — stesso principio già usato per
  il replay di roster/storico. `Rejected` resta deliberatamente non ri-emesso: la variante non
  porta un timestamp (scelta esplicita per tenere `handle_event` pura, vedi doc-comment di
  `SelfAdmission`), quindi non c'è un `retry_after_secs` accurato da ricostruire — invariato.
  3 nuovi test TDD (RED-first): due positivi (Deciding/Pending) + un negativo (NotJoining non
  ri-emette nulla). 506 lib test verdi, clippy pulito sul modulo `aichat` (2 warning clippy
  restanti sono drift preesistente su file mai toccati — `ai_adapter.rs`/`ws.rs`, già presenti
  sul commit base 01852f8).

**Resta fuori scope** (documentato in-code): il gate 2 di un PRESENTE che deve ancora votare
(`self.pending_votes`, lato server) non viene ri-emesso alla riapertura della finestra — stesso
gap, lato opposto della stanza, da valutare in una slice successiva.

## [0.34.1] — 2026-07-04 — AI Chat: diagnostica keepalive — distingue le 3 cause di `PeerGone`

Nessun cambio di comportamento: solo strumentazione. Emerso investigando un `PeerGone` falso
riportato dal vivo (2 macchine, `rumpleteazer`/`skimble`) — il reader task del link TCP logga
tutti e tre i percorsi che portano a `PeerGone` (timeout keepalive, EOF pulito, errore I/O) a
`tracing::debug!`, invisibile a filtro di default (`INFO`, `main.rs`): impossibile stabilire a
posteriori quale dei tre fosse scattato senza `RUST_LOG=debug` durante l'incidente.

### Changed

- `aichat::service` (reader task, `spawn_peer_tasks`): il ramo timeout (`DEAD_THRESHOLD` scaduto
  senza alcun byte) e il ramo errore I/O passano da `debug!` a `warn!` (esiti anomali); il ramo
  EOF pulito (prima senza alcun log) guadagna un `info!` (esito normale di uno shutdown). Tutti e
  tre ora visibili a filtro default, con testo che nomina esplicitamente `PEER-GONE per <causa>`
  — un futuro "peer sparito" spurio in chat sarà diagnosticabile dal log senza dover riprodurre
  con `RUST_LOG=debug`.

## [0.34.0] — 2026-07-03 — AI Chat: fix di integrità del consenso (review 2026-07-03, fix #1/#2/#3/#5/#7)

Corregge cinque difetti di **integrità del consenso** trovati nella review dell'ammissione a
stanza (0.33.0). Tutti guidati da test RED-first (8 nuovi test nel modulo `aichat::service`).

### Fixed

- **#1 — race del timer di voto stantio.** `ServiceEvent::VoteTimeout`/`Effect::StartVoteTimeout`
  ora portano una **generazione** monotona (`PendingVote::generation`, contatore `next_vote_gen`):
  il timer di un turno già risolto, se scade in ritardo dopo che il candidato ha ri-chiesto, non
  combacia più con la generazione del voto corrente e viene ignorato (prima ri-risolveva —
  admit — il turno NUOVO, scavalcando la finestra di veto).
- **#2 — veto da un non-presente.** `record_vote` ora verifica `voter ∈ pv.present` prima di
  qualunque risoluzione: un peer connesso ma non invitato a votare (secondo candidato, o peer
  già rifiutato ancora linkato) non può più vietare l'ammissione altrui.
- **#3 — admit fantasma su `PeerGone` del candidato.** `PeerGone` ora rimuove anche
  `pending_votes[id]` quando `id` è il candidato del voto: prima il turno sopravviveva e
  `ready_after_peer_gone`/`VoteTimeout` ammetteva un peer ormai sparito (membro fantasma del
  roster).
- **#5 — cooldown di re-request lato server.** Un candidato rifiutato entra in `cooling_down`
  (nuovo `Effect::StartCooldownTimer` → `ServiceEvent::CooldownExpired`, sempre via timer-Effect
  per tenere `handle_event` puro): il server ignora i re-request finché il cooldown non scade —
  prima il cooldown viveva solo nella UI ed era scavalcabile via wire o riaprendo la finestra.
- **#7 — banner del gate 2 stantio.** Alla risoluzione (`resolve_admit`/`resolve_reject`) e alla
  sparizione del candidato, il server manda `AiChatAdmissionResolved`/`ChatMsg::AdmissionResolved`
  a tutti i presenti, che tolgono il banner. Nuovo bridge lato presente-client
  (`ChatMsg::AdmissionResolved` → `ToUi`).

### Note

Differiti a un follow-up (per scelta di scope): #4 (relay-guard lato destinatari — un peer
connesso non-ammesso riceve ancora i messaggi di stanza) e #6 (liveness dell'elezione — un peer
UDP-annunciato ma TCP-irraggiungibile con IP più basso vince l'elezione all'infinito).

## [0.33.0] — 2026-07-03 — AI Chat: ammissione alla stanza (2 gate, AND con veto + timeout=Sì)

Chiude il **Debito #1** ("consenso lato ingresso") lasciato dall'hardening AI Chat: prima
di questa slice l'accept-loop ammetteva peer non consentiti; il consenso esistente era
**pairwise/asimmetrico** e perdeva il saluto del nuovo arrivato (parlava prima che l'altro
lo ammettesse) oltre a mostrare testo fuorviante. Design:
`Docs/superpowers/specs/2026-07-03-aichat-admission-consent-design.md`; piano:
`Docs/superpowers/plans/2026-07-03-aichat-admission.md`.

### Added

- **`src/aichat/wire.rs`** — 5 nuove varianti `ChatMsg` (peer↔peer, Contratto N):
  `RequestAdmission { label }` (client→server, dopo il gate 1 locale), `AdmissionVoteRequest
  { candidate }` (server→ogni presente ammesso), `AdmissionVote { candidate, accept }`
  (presente→server, `accept:false` = veto immediato), `Admitted { label }` (server→tutti,
  segue `Roster` aggiornato), `AdmissionRejected { label }` (server→solo il candidato).
  Additive, wire `snake_case`.
- **`src/aichat/service.rs`** — il macchinario dell'ammissione:
  - `SelfAdmission` (enum: `NotJoining`/`Deciding`/`Pending`/`Admitted`/`Rejected`) — stato
    del **nuovo arrivato** rispetto al proprio tentativo di ingresso.
  - `admitted: HashSet<PeerId>` — i client GIÀ ammessi nella stanza (base di calcolo dei
    "presenti" per un nuovo voto, e superficie del relay-guard, sotto). Distinto da
    `connected` (link TCP vivo ma non ancora votato ammesso).
  - `pending_votes: HashMap<PeerId, PendingVote>` — il turno di voto in corso per candidato
    (etichetta, presenti invitati, voti raccolti finora).
  - `start_admission_vote(candidate, label)` (SOLO server): calcola i presenti da
    `self.admitted` (non `connected` — un peer solo connesso non ha titolo per votare),
    registra il `PendingVote`, manda `AdmissionVoteRequest` a ogni presente e avvia
    `Effect::StartVoteTimeout { candidate, secs: ADMISSION_VOTE_TIMEOUT_SECS }` (**60s**).
  - `record_vote(candidate_label, voter, accept)`: un `accept:false` risolve subito
    `resolve_reject` (veto, basta un solo no); un `accept:true` che completa **tutti** i
    presenti risolve `resolve_admit`; un voto per un'etichetta senza `PendingVote`
    corrispondente è stale e viene ignorato silenziosamente.
  - `resolve_admit(candidate, pv)`: inserisce il candidato in `admitted`, gli manda lo
    storico (`ChatMsg::History`, riusato invariato — la history-dump esistente è
    **preservata all'ammissione**), broadcast `Admitted` + `Roster` aggiornato a tutti.
  - `resolve_reject(candidate, pv)`: manda `AdmissionRejected` solo al candidato (i
    presenti non ricevono conferma esplicita — solo il non-aggiornamento del roster).
  - `ServiceEvent::VoteTimeout { candidate }`: silenzio-oltre-timeout **conta come sì** —
    se il `PendingVote` esiste ancora al timeout (nessun veto arrivato), risolve
    `resolve_admit` come se tutti i presenti avessero votato sì; se già risolto (veto
    arrivato prima), no-op (guard anti-doppia-risoluzione).
  - `begin_join_gate()` / `request_admission(server_id)`: lato **nuovo arrivato**. Al
    connect al server eletto, `begin_join_gate` mostra il gate 1 (`AiChatJoinPrompt` coi
    presenti da `self.peers`) e passa a `Deciding` — **sostituisce** il vecchio invio
    immediato di `ChatMsg::Join`. Su `AiChatJoinDecision{accept:true}`, `request_admission`
    manda `ChatMsg::RequestAdmission` e passa a `Pending` (la UI blocca l'input — **previene
    il saluto perso**, il bug principale del vecchio consenso). Su `Admitted`/
    `AdmissionRejected` per me: `self_admission` passa a `Admitted`/`Rejected`.
  - **Elezione da tutti i peer scoperti**: i "presenti" per il gate 1/2 (`present_labels()`)
    sono ora calcolati da `self.peers` (popolata da `Discovered`), non solo da `connected` —
    coerente con l'estensione già fatta per l'elezione late-joiner (0.25.7).
  - **`PeerGone` pulisce `admitted`**: la sparizione di un peer lo rimuove da `self.admitted`
    **prima** di ogni altra pulizia — evita un voto-fantasma (un presente sparito a metà voto
    non blocca la risoluzione, che ricade su chi resta) — e ri-valuta `pending_votes` in corso
    per ammettere/rifiutare subito se il peer sparito era l'ultimo voto mancante.
  - **Relay-guard** (Task 8b): il server **ignora** un `ChatMsg::Say` da un mittente non in
    `self.admitted` — un peer in stato `pending` (connesso ma non ancora votato) non può
    iniettare messaggi nella stanza. Nota di debito: il controllo copre solo il **mittente**;
    i destinatari del relay non sono ristretti ad `admitted` (valutato e scartato per questa
    slice — un client connesso ma non admitted riceve comunque il relay, non lo genera).
  - Costruttori/getter `#[cfg(test)]`: `mark_admitted_for_test`, `admitted_contains_for_test`,
    `self_admission_for_test`/`set_self_admission_for_test`, `pending_votes_len_for_test`.
- **`src/ws.rs`** — dispatch dei 3 nuovi `ClientMsg` (`AiChatJoinDecision`,
  `AiChatAdmissionVote`, `AiChatRequestAdmission`) verso l'attore `AiChatService`.

### Removed

- Il vecchio flusso di consenso **pairwise/asimmetrico** (`AiChatJoinRequest`/
  `AiChatJoinConsent`, invio diretto di `ChatMsg::Join` senza voto) non è più il percorso
  primario di ingresso — le varianti restano nel wire per retro-compatibilità additiva ma
  non sono più emesse dal flusso di ammissione. **Fix del Debito #1.**

### Test

495 test lib totali (nuovi test dedicati all'ammissione: avvio voto dai soli `admitted`,
veto immediato, timeout=sì, voto stale ignorato, history-dump all'ammissione, `PeerGone`
durante un voto in corso, relay-guard su non-ammessi, gate 1/2 lato nuovo arrivato,
transizioni `SelfAdmission` complete) + 2 integration loopback (`aichat_relay_loopback.rs`,
`--ignored`).

### Verifica

```
cargo build -p orchestrator          → pulito
cargo test -p orchestrator           → 495 passed (lib)
cargo test -p orchestrator --test aichat_relay_loopback -- --ignored → 2 passed
```

### Debito residuo (per il supervisore)

- Destinatari del relay **non** ristretti ad `admitted` (solo il mittente è guardato) —
  annotato in-code in `service.rs`, non affrontato in questa slice.
- Delega del voto di ammissione all'AI (flag "Let AI answer") — follow-up esplicito
  (Slice C del piano), non implementato qui.

**UI da accettare dal vivo** (multi-macchina): vedi `Docs/TESTING-e2e.md` §H.

---

## [0.32.1] — 2026-07-02 — AI Chat 2b: no double-fire (invocazione sopprime auto sullo stesso messaggio)

Piccolo raffinamento di Slice 2 (0.32.0). Con un'invocazione esplicita (`@ai`/`@all`/
`@<mio-label>-ai`) E `ai_autoparticipate` ON, un singolo messaggio produceva SIA
`Effect::InvokeLocalAi` (risposta all'invocazione, Slice 1a/1b) SIA
`Effect::AutoParticipate` (auto-giudizio, Slice 2) — due turni AI per un solo messaggio
umano (bounded dal cap esistente, ma costo doppio e ridondante).

### Fixed

- **`src/aichat/service.rs`** — negli arm `ServiceEvent::HumanSay` e
  `ServiceEvent::PeerMsg { msg: ChatMsg::Say, .. }`, `maybe_autoparticipate` viene ora
  chiamato SOLO se il messaggio corrente non ha GIÀ prodotto un `Effect::InvokeLocalAi`
  (check `effects.iter().any(|e| matches!(e, Effect::InvokeLocalAi { .. }))` subito prima
  della chiamata). La chiamata viene saltata del tutto (non solo il suo esito scartato):
  `maybe_autoparticipate` ha il side-effect di marcare `autoparticipate_inflight = true`
  quando scatta, e non ha senso marcarlo per un giudizio mai richiesto. Invarianti
  preservate: un messaggio umano NORMALE (senza `@`) con auto ON continua ad
  auto-partecipare (nessuna invocazione → niente da sopprimere); un `@<altro-label>-ai`
  che non mi targeta non produce `InvokeLocalAi` PER ME, quindi l'auto-partecipazione può
  ancora scattare (comportamento scelto: non sto rispondendo a quell'invocazione, potrei
  avere comunque qualcosa da aggiungere). Le 5 guardie di `maybe_autoparticipate` restano
  invariate.

## [0.32.0] — 2026-07-02 — AI Chat Slice 2 (auto-partecipazione: giudizio rilevanza + cap turni)

Estende Slice 1a/1b (0.30.0/0.31.0): l'AI può ora intervenire su messaggi NORMALI della
stanza (nessuna invocazione umana), giudicando da sola la rilevanza. Design:
`Docs/superpowers/specs/2026-07-02-aichat-ai-participants-design.md` §10. Rimuove la
loop-safety-per-costruzione delle Slice 1a/1b: qui il controllo del loop è cap sui turni
AI consecutivi + guardie esplicite. **Costruita e verificata SOLO con `StubAdapter`**
(silenzio deterministico) — nessuna chiamata API reale in questa slice; l'attivazione con
`ai_autoparticipate = true` e API reale resta al supervisore, DOPO l'e2e live di 1a/1b.

### Added

- **`src/ai_adapter.rs`** — nuovo metodo `AiAdapter::chat_autoparticipate(&self,
  transcript: &str, cancel: Option<CancellationToken>) -> Option<String>` (default sul
  trait: ritorna `None` incondizionatamente — è ciò che rende `StubAdapter`
  deterministicamente silenzioso SENZA bisogno di un override esplicito, e protegge
  qualunque altro `impl AiAdapter` esistente — es. i fake di test in `aichat/service.rs`
  — dalla rottura di build). `ClaudeAdapter` sovrascrive con l'implementazione reale: una
  `messages.create()` text-only (stesso pattern di `chat_reply`, nessun tool nel payload),
  con un nuovo system prompt dedicato `AUTOPARTICIPATE_SYSTEM_PROMPT` (diverso da
  `CHAT_SYSTEM_PROMPT`: qui l'AI non risponde a un'invocazione, deve giudicare da sola se
  intervenire, con un'opzione esplicita di silenzio). Se il testo di risposta (trimmed) è
  vuoto o uguale a `"SILENCE"` (case-insensitive) → `None`; altrimenti `Some(text)`. Un
  errore di rete/API mappa a `None` (mai una riga di chat "fantasma" per un fallimento
  che nessuno ha richiesto — diverso da `chat_reply`, dove l'errore è visibile perché
  l'umano ha chiesto esplicitamente).

- **`src/aichat/config.rs`** — nuovo campo `pub ai_autoparticipate: bool` su
  `AiChatConfig`, `#[serde(default)]` (bool → `false`, tollera i config esistenti
  scritti prima che il campo esistesse). Default `false` in `impl Default for
  AiChatConfig` — a DIFFERENZA di `ai_participates` (default `true`): l'auto-
  partecipazione è più aggressiva/costosa, opt-in esplicito.

- **`src/aichat/service.rs`** — il cuore della slice (design §10.3):
  - Nuova const `MAX_CONSECUTIVE_AI_TURNS: usize = 4` — il cap sui turni AI
    consecutivi, il "controllo del loop" di questa slice.
  - Nuovi campi su `AiChatService`: `ai_autoparticipate: bool` (dal config, nuovo 4°
    parametro di `new(me, ai_adapter, ai_participates, ai_autoparticipate)`),
    `consecutive_ai_turns: usize` (init 0), `last_speaker_label: Option<String>` (init
    `None`), `autoparticipate_inflight: bool` (init `false`).
  - Nuovo helper privato `note_room_message(&mut self, from_label: &str)`: azzera il
    contatore su un mittente `-human`, lo incrementa su `-ai`; aggiorna sempre
    `last_speaker_label`. Choke-point unico: chiamato da `publish_say` (copre i MIEI
    messaggi — `HumanSay`, `AiReply`, `AutoParticipateDone{Some}`, tutti passano da lì)
    e, separatamente, dall'arm `PeerMsg::Say` (messaggi di un ALTRO peer, che hanno il
    proprio `history.push` indipendente) — mai due volte per la stessa riga di storico.
  - Nuovo helper privato `maybe_autoparticipate(&mut self, triggering_from_label: &str)
    -> Vec<Effect>`: valuta le 5 guardie del design (flag ON, non i miei messaggi, non
    ho parlato io per ultimo, cap non raggiunto, nessun giudizio già in volo); se tutte
    passano marca `autoparticipate_inflight = true` e ritorna
    `[Effect::AutoParticipate { transcript }]` (transcript da `format_transcript`,
    funzione pura invariata).
  - Nuovo `Effect::AutoParticipate { transcript: String }` e nuovo `ServiceEvent::
    AutoParticipateDone { reply: Option<String> }`.
  - `perform` per `Effect::AutoParticipate`: spawna un task shutdown-aware che chiama
    `ai_adapter.chat_autoparticipate(&transcript, Some(shutdown))` e manda SEMPRE
    `AutoParticipateDone { reply }` sull'inbox — **anche per `None`** (differenza
    deliberata da `Effect::InvokeLocalAi`, che scarta una risposta vuota in `perform`
    stesso): qui il silenzio è l'esito PIÙ comune, non un caso degenere, e
    `autoparticipate_inflight` deve azzerarsi per ogni esito, altrimenti un singolo
    silenzio bloccherebbe l'auto-partecipazione per sempre.
  - `handle_event` per `ServiceEvent::AutoParticipateDone`: azzera
    `autoparticipate_inflight` INCONDIZIONATAMENTE, poi — solo se `Some(text)` con
    `text.trim()` non vuoto — pubblica come `Say` da `"<label_base>-ai"` via
    `publish_say` (stesso percorso di `AiReply`).
  - Wiring nei punti di ingresso messaggio: arm `HumanSay` chiama
    `maybe_autoparticipate(&mio_label_human)` dopo `publish_say` + l'eventuale
    `InvokeLocalAi` di 1a/1b; arm `PeerMsg::Say` chiama `note_room_message` accanto al
    proprio `history.push` e poi `maybe_autoparticipate(from_label)` — **senza** il
    filtro "-human" usato per l'invocazione Slice 1b: un `-ai` di un ALTRO peer PUÒ
    innescare la mia auto-partecipazione (è la conversazione AI↔AI che questa slice
    abilita), bounded dal cap; arm `AiReply` invariato (nessuna chiamata a
    `maybe_autoparticipate` — la guardia 2 la escluderebbe comunque, l'omissione evita
    solo una valutazione ridondante).
  - Nuovi costruttori/helper di test: `new_for_test_with_autoparticipate(me,
    ai_autoparticipate)`; `new_for_test`/`new_for_test_with_participation` aggiornati al
    nuovo 4° parametro di `new` (default `false`); `#[cfg(test)]`
    `consecutive_ai_turns_for_test`/`set_consecutive_ai_turns_for_test`,
    `autoparticipate_inflight_for_test`/`set_autoparticipate_inflight_for_test`.

- **`src/main.rs`** — legge `cfg.ai_autoparticipate` e lo passa come 4° argomento a
  `AiChatService::new`.

### Changed

- **`crates/orchestrator/tests/aichat_relay_loopback.rs`** — i 3 call-site di
  `AiChatService::new(...)` aggiornati al nuovo 4° parametro (`false` — questi test
  coprono il relay TCP/keepalive, non l'auto-partecipazione).

### Nota — osservazione non risolta (da valutare dal supervisore)

Con `@ai`/`@all` (invocazione esplicita, Slice 1a/1b) E `ai_autoparticipate` entrambi
attivi sulla stessa macchina, un singolo messaggio umano può produrre SIA
`Effect::InvokeLocalAi` SIA `Effect::AutoParticipate` — due turni AI per un solo
messaggio. Non è una rottura della loop-safety (resta bounded dal cap), ma è un costo
doppio non scontato; il design (§10) non lo esclude esplicitamente, e le istruzioni
ricevute per questa slice non prevedevano un guard specifico — implementato fedele al
brief, nessuna guardia extra aggiunta di iniziativa. Segnalato esplicitamente qui e nel
report finale.

### Test

19 nuovi test in `aichat/service.rs` (contatore, le 5 guardie di `maybe_autoparticipate`
una per una — incluso il confine esatto del cap, `MAX-1` vs `MAX` —, reset dopo un
`-human`, `AutoParticipateDone{Some/None/blank}`, `perform` che manda SEMPRE l'evento
anche per `None`, integrazione `HumanSay`/`PeerMsg::Say`); 3 nuovi test in
`ai_adapter.rs` (`stub_autoparticipate_is_silent`,
`claude_autoparticipate_silence_maps_to_none`,
`claude_autoparticipate_text_maps_to_some`); 2 nuovi test in `aichat/config.rs`
(default `false`, migrazione tollerante). Vedi `IMPLEMENTATION.md` per i dettagli.

### Verifica

```
cargo test -p orchestrator          → 470 passed (lib) + 13 (ws_integration) + 2 (doc-tests)
cargo test -p orchestrator --test aichat_relay_loopback -- --ignored → 2 passed
cargo clippy -p orchestrator --all-targets → 2 warning PRE-ESISTENTI (invariati, non toccati da questa slice)
cargo build -p orchestrator          → pulito
```

---

## [0.31.0] — 2026-07-02 — AI Chat Slice 1b (@all / @<label>-ai + ai_participates)

Estende Slice 1a (0.30.0): l'invocazione dell'AI non è più limitata alla propria
macchina. Design: `Docs/superpowers/specs/2026-07-02-aichat-ai-participants-design.md`
§9. **Nessun cambio a `protocol`/`wire.rs`/`mcp-server`**: la propagazione riusa
integralmente `ChatMsg::Say` già esistente (§9.3 del design, RACCOMANDAZIONE seguita
alla lettera — YAGNI su un `ChatMsg` dedicato).

### Added

- **`src/aichat/service.rs`** — nuove forme di invocazione + gate sul flag
  "la mia AI partecipa":
  - `extract_ai_invocation(text) -> Option<Invocation>` (cambio di tipo di ritorno
    da `Option<String>`): due nuovi tipi PURI, `InvokeTarget { Own, Label(String),
    All }` e `Invocation { target, request }`. Riconosce, oltre a `@ai` (Own,
    invariato), anche `@all` (All, tutte le AI presenti) e `@<label>-ai` (Label,
    l'AI di una macchina specifica per `label_base` — anche remota). Il confine di
    parola è ora garantito per costruzione dalla tokenizzazione sul primo spazio
    bianco (il "marcatore" è il primo token), senza un controllo separato: `"@aiuto
    qualcosa"` produce il token `"@aiuto"`, che non combacia con nessuna delle tre
    forme → `None`, comportamento invariato rispetto a 1a. Case-insensitive SOLO sul
    marcatore (`to_ascii_lowercase`, che preserva la lunghezza in byte — nessun
    rischio di spezzare un confine UTF-8 nell'estrazione del label a case originale);
    il confronto del `label` estratto con `self.me.label_base` resta case-sensitive
    (stessa convenzione già usata per `peer_label` in `ServiceEvent::Consent`).
  - Nuovo campo `ai_participates: bool` su `AiChatService`; `new(me, ai_adapter,
    ai_participates)` guadagna il terzo parametro. `new_for_test(me)` invariato nella
    firma (default `true`); nuovo `#[cfg(test)] new_for_test_with_participation(me,
    ai_participates)` per i test che esercitano il flag OFF.
  - Arm `ServiceEvent::HumanSay` (rifattorizzato): dopo `publish_say`, se
    `extract_ai_invocation` rileva un'invocazione che mi riguarda (`Own`/`All`
    sempre, `Label(l)` solo se `l == self.me.label_base`) emette
    `Effect::InvokeLocalAi` — **sempre**, nessun gate sul flag: il proprio umano ha
    scritto nella propria finestra, è già il consenso.
  - Arm `ServiceEvent::PeerMsg { msg: ChatMsg::Say { from_label, text }, .. }`
    (esteso, relay/forward esistenti INVARIATI — l'invocazione è rilevata ACCANTO,
    non al posto): SE `from_label` finisce in `"-human"` (**GUARDIA LOOP,
    load-bearing** — vedi sotto), rileva l'invocazione e, se il target sono io
    (`All`, oppure `Label(l)` con `l == self.me.label_base`; `Own` è IGNORATO — è lo
    shorthand "propria AI" del MITTENTE, non un'invocazione verso di me) **e**
    `self.ai_participates == true`, emette `Effect::InvokeLocalAi` (stesso `Effect`
    di 1a, riusato senza modifiche).
  - **Guardia loop (load-bearing)**: la rilevazione su `PeerMsg::Say` gira SOLO se
    `from_label.ends_with("-human")`. Una risposta di un'AI (`-ai`) relayata — anche
    se il suo testo contenesse letteralmente `"@all"` — non produce MAI
    `InvokeLocalAi`: un'AI non può innescarne un'altra. Resta loop-safe per
    costruzione anche cross-macchina (ogni turno AI è iniziato da un umano), stesso
    principio di 1a esteso al caso remoto. Test dedicato:
    `peer_say_from_ai_never_invokes_even_with_at_all_in_text`.

- **`src/aichat/config.rs`** — nuovo campo `pub ai_participates: bool` su
  `AiChatConfig`, `#[serde(default = "default_true")]` (tollera i config esistenti
  scritti prima che il campo esistesse — deserializzazione non fallisce, assume
  `true`). `Default` aggiornato (`ai_participates: true` — partecipativo di
  default, opt-out facile dal tab `/config`, design §9.4).

- **`src/main.rs`** — legge `cfg.ai_participates` e lo passa al terzo parametro di
  `AiChatService::new`.

### Changed

- **`crates/orchestrator/tests/aichat_relay_loopback.rs`** — i tre costruttori
  `AiChatService::new(...)` aggiornati al nuovo terzo parametro (`true` — nessuno di
  questi test esercita il flag OFF, sono test di relay TCP/keepalive preesistenti).

### Test

- `crates/orchestrator/src/aichat/service.rs`: `extract_ai_invocation_detects_all`,
  `extract_ai_invocation_detects_label`, `extract_ai_invocation_rejects_empty_label`
  (+ le 4 esistenti di 1a adattate al nuovo tipo `Invocation`);
  `human_say_at_all_invokes_local_ai_regardless_of_flag`,
  `human_say_at_own_label_ai_invokes_local_ai`,
  `human_say_at_other_label_ai_does_not_invoke_local_ai`;
  `peer_say_from_human_at_all_invokes_when_participates_on`,
  `peer_say_from_human_at_all_does_not_invoke_when_participates_off`,
  `peer_say_from_ai_never_invokes_even_with_at_all_in_text` (guardia loop),
  `peer_say_from_human_at_ai_own_is_ignored`,
  `peer_say_from_human_at_my_label_ai_invokes_when_participates_on`,
  `peer_say_from_human_at_other_label_ai_does_not_invoke`.
- `crates/orchestrator/src/aichat/config.rs`: `default_ai_participates_is_true`,
  `missing_field_deserializes_as_true`.
- `cargo test -p orchestrator --test aichat_relay_loopback -- --ignored`: 2/2
  invariati (nessuna regressione sul relay TCP/keepalive esistente).

---

## [0.30.0] — 2026-07-02 — AI Chat: AI participant Slice 1a (@ai)

### Added

- **`src/ai_adapter.rs`** — nuovo metodo `AiAdapter::chat_reply(transcript, request,
  cancel) -> String`: risposta **text-only** (nessun tool, nessuna finestra) per la
  stanza AI Chat, vedi
  `Docs/superpowers/specs/2026-07-02-aichat-ai-participants-design.md` §4. È il confine
  di sicurezza della Slice 1a: l'AI "commenta" la conversazione, non "agisce" su di essa.
  - `StubAdapter::chat_reply` — risposta deterministica `"[stub AI] commento su: {request}"`
    (fallback senza API key + doppio usato dai test del servizio AI Chat).
  - `ClaudeAdapter::chat_reply` — UNA sola chiamata Messages API NON-streaming, con
    `tools: Vec::new()` (nessun tool nel payload) e un nuovo system prompt
    `CHAT_SYSTEM_PROMPT` da "partecipante chat" (diverso da `agent::SYSTEM_PROMPT`, che
    è orientato ai comandi shell del cursore). Un solo turno `user` = transcript +
    richiesta; niente storia stateful (a differenza di `respond`, ogni invocazione `@ai`
    riparte da zero — il "contesto" è il transcript passato in ingresso).
  - `ProbeAdapter` in `telegram/channel.rs` (adapter di test già esistente, usato SOLO
    per verificare il wiring del confirmer) ha ricevuto un'implementazione minima di
    `chat_reply` per continuare a soddisfare il trait.

- **`src/aichat/service.rs`** — l'AI locale come partecipante della stanza, invocata con
  il prefisso `@ai` in un messaggio umano (case-insensitive, seguito da spazio o fine
  stringa). Loop-safe per costruzione: l'AI parla SOLO su invocazione umana esplicita,
  mai in risposta a un proprio turno o a un messaggio normale.
  - Nuovo campo `ai_adapter: Arc<dyn AiAdapter>` su `AiChatService`; `new(me,
    ai_adapter)` (nuovo secondo parametro). Nuovo `#[cfg(test)] new_for_test(me)`
    (usa `StubAdapter`) per non dover toccare i ~44 costruttori dei test esistenti a
    mano — restano tutti sullo `StubAdapter`, comportamento invariato.
  - Nuovo `Effect::InvokeLocalAi { request, transcript }`: emesso da `handle_event`
    quando un `HumanSay` è un'invocazione. `perform` lo esegue spawnando un task
    (stesso pattern non-bloccante di `Effect::ConnectTo`) che chiama
    `ai_adapter.chat_reply(&transcript, &request, Some(shutdown))` e re-inietta il
    risultato come `ServiceEvent::AiReply { text }` sull'inbox dell'attore — SALVO se il
    testo (una volta tolti gli spazi) è vuoto: una `chat_reply` vuota (es. cancellazione
    upfront, o risposta degenere) non deve diventare una riga di chat fantasma.
  - Nuovo `ServiceEvent::AiReply { text }`: trattato ESATTAMENTE come un `Say` da
    `"<label_base>-ai"` — storico, eco alla propria UI, e relay in rete secondo il ruolo
    (Server → broadcast; Client → invia al server; Undecided → nessun inoltro). Questo
    arm NON controlla `@ai` nel testo: nessuna ri-invocazione possibile da qui (loop-safety).
  - Nuovo metodo privato `publish_say(from_label, text) -> Vec<Effect>`: fattorizza la
    logica di pubblicazione (storico + eco UI + inoltro per ruolo) già presente
    nell'arm `HumanSay`, ora riusata anche da `AiReply` (SOLID/DRY — prima duplicava
    interamente lo stesso corpo).
  - Nuove funzioni pure (module-level, nessun `self`): `extract_ai_invocation(text) ->
    Option<String>` (rileva il prefisso `@ai`, default `"commenta la discussione"` se
    la richiesta è vuota) e `format_transcript(&[ChatLine]) -> String` ("label: testo"
    per riga).

- **`crates/orchestrator/tests/aichat_relay_loopback.rs`** — i tre costruttori
  `AiChatService::new(...)` (integration test, fuori dal `cfg(test)` del lib crate,
  quindi senza accesso a `new_for_test`) aggiornati al nuovo secondo parametro
  (`Arc::new(StubAdapter)`).

Nessun cambio a `protocol`/`ui`/`mcp-server`: Slice 1a è backend-only — la risposta
dell'AI è un normale `AiChatMessage` con label `-ai`, già mostrato dalla UI esistente
(il suffisso `-ai` era già anticipato in `wire.rs`).

`cargo test -p orchestrator`: 432 test lib (418 passed su 0.29.0 + 14 nuovi: 2 in
`ai_adapter.rs` [`stub_chat_reply_mentions_request`, `claude_chat_reply_returns_text_only`
— quest'ultimo rinforzato in review con asserzioni su `system`/contenuto del turno user]
+ 12 in `aichat/service.rs` [4 `extract_ai_invocation_*`, 2 `format_transcript_*`,
`at_ai_invocation_emits_invoke_local_ai`, `plain_message_does_not_invoke_ai`,
`ai_reply_posts_say_from_ai_label`, `ai_reply_does_not_reinvoke`,
`perform_invoke_local_ai_sends_ai_reply_back_to_inbox`,
`perform_invoke_local_ai_skips_empty_reply`]), 0 failed, 3 ignored (invariato). `cargo
test -p orchestrator --test aichat_relay_loopback -- --ignored`: 2/2. `cargo clippy -p
orchestrator --all-targets`: 2 warning preesistenti (`ai_adapter.rs` — `map_or`/
`is_some_and` nel loop di `respond`, NON nel nuovo `chat_reply` che già usa
`is_some_and`; `ws.rs:491`), nessuno nuovo. `cargo build -p orchestrator` pulito.
`cargo fmt` deliberatamente NON eseguito su tutto il crate (drift preesistente).

---

## [0.29.0] — 2026-07-02 — AI Chat hardening: register-before-spawn (chiude la race residua di 0.27.0)

### Fixed

- **`src/aichat/service.rs`** — Slice C2: race residua lasciata da Slice C (0.27.0,
  debiti #4+#3). Sul path CONNECT, il task di connessione spawnato per
  `Effect::ConnectTo` chiamava `spawn_peer_tasks` SU SE STESSO (dentro il task
  spawnato), prima di mandare `ConnectOutcome::Success` all'attore. Il reader appena
  avviato poteva quindi emettere un `ServiceEvent::PeerGone(id, gen)` PRIMA che
  l'attore avesse scritto `self.link_gen[id] = gen` (che avveniva solo dopo, quando
  l'attore processava il `Success`) — la guardia di generazione lo vedeva come STALE
  e lo scartava; la registrazione arrivava comunque dopo, creando un **link fantasma**
  (in `connected`/`links`, ma col reader già morto — mai risanato: il guard
  anti-duplicato di `ConnectTo` impedisce una riconnessione finché il peer risulta
  ancora "connesso"). Stesso pattern, latente, sul path ACCEPT (l'accept loop spawnava
  `spawn_peer_tasks` prima di notificare l'attore).
  - Fix strutturale: `spawn_peer_tasks` si sposta DENTRO l'attore (con `&mut self`),
    su ENTRAMBI i path, e viene chiamata SEMPRE dopo aver scritto `link_gen`/
    `connected`. Nuovi metodi privati `register_link`/`register_connect_success`/
    `register_connect_failure` (decomposti da quello che era `apply_connect_outcome`,
    ora rimosso) fanno SOLO la mutazione di stato — nessun I/O, testabili senza un
    `TcpStream`.
  - `ConnectOutcome::Success` ora porta il `TcpStream` grezzo (`stream: TcpStream`)
    invece di un `writer_tx` già pronto: il task di connessione NON spawna più nulla,
    si limita al connect con timeout esplicito (5s, invariato).
  - Il canale interno `new_link_tx`/`new_link_rx` (accept → attore) passa da
    `(PeerId, UnboundedSender<ChatMsg>, u64)` a `(PeerId, TcpStream)`: l'accept loop
    manda il socket grezzo, non alloca più una generazione né spawna nulla.
  - Conseguenza (semplificazione): `link_gen_counter: Arc<AtomicU64>` (contatore
    CONDIVISO, necessario finché l'allocazione avveniva in contesti spawnati senza
    `&mut self`) sostituito da `next_gen: u64` — l'allocazione è ora TUTTA lato attore,
    un `Arc` condiviso non serve più.
  - `handle_event(PeerGone)` (guardia di generazione, Slice C) invariata — ora è
    semplicemente impossibile che un `Success`/una registrazione avvenga dopo un
    `PeerGone` stale, perché `link_gen` è scritto prima dello spawn su entrambi i path.
  - Nessun cambio a `protocol`/`ui`/`mcp-server`. Solo `src/aichat/service.rs` toccato.

`cargo test -p orchestrator`: 421 test lib (418 passed + 3 ignored, `+1` rispetto a
0.28.0 [417 passed + 3 ignored]: `connect_success_registers_link_and_sends_join`
riscritto in due test [`connect_success_registers_state_before_any_spawn` e
`register_connect_success_makes_subsequent_peer_gone_non_stale`], nessun test
rimosso), 0 failed. `cargo test -p orchestrator --test aichat_relay_loopback --
--ignored`: 2/2. `cargo clippy -p orchestrator --all-targets`: 2 warning preesistenti
(`ai_adapter.rs:173`, `ws.rs:491`), nessuno nuovo. `cargo fmt` deliberatamente NON
eseguito (drift preesistente sull'intero crate).

---

## [0.28.0] — 2026-07-02 — AI Chat hardening: discovery full-duplex (#6)

### Fixed

- **`src/aichat/discovery.rs`** — debito #6 ("discovery full-duplex"): il trait
  `Discoverer::next` prendeva `&mut self` (per il buffer di ricezione riusabile di
  `UdpDiscoverer`), mentre `announce` prende `&self` — le due chiamate non potevano
  coesistere in un `tokio::select!` sulla stessa variabile (aliasing `&mut`/`&` vietato
  dal borrow checker). Il loop di scoperta in `main.rs` era quindi SEQUENZIALE
  (announce → finestra di ascolto da 500ms → ri-annuncio ogni ~7s), senza ascolto
  continuo mentre si annuncia.
  - Fix: `Discoverer::next(&self)` — firma cambiata (breaking per gli implementori del
    trait, coerente col resto del piano di hardening).
  - `FakeDiscoverer`: `incoming` passa da `VecDeque<..>` a
    `Mutex<VecDeque<..>>` (mutabilità interna) — `next(&self)` prende il lock e fa
    `pop_front` senza bisogno di un prestito esclusivo.
  - `UdpDiscoverer`: rimosso il campo `buf: Vec<u8>` riusabile; `next(&self)` alloca un
    buffer LOCALE (`[0u8; 2048]`) ad ogni chiamata — costo trascurabile, i datagrammi di
    scoperta arrivano ogni pochi secondi, non è un percorso hot-path. `announce`
    invariata.
- **`src/main.rs`** — il loop del task di scoperta UDP riscritto come `tokio::select!`
  full-duplex su TRE rami: un `tokio::time::interval` (~5s, con il primo tick
  immediato) per l'annuncio periodico; l'ascolto continuo `disc.next()` che inietta
  `ServiceEvent::Discovered` nell'inbox del servizio; il nuovo ramo
  `shutdown_disc.cancelled() => break` che estende il teardown del debito #2
  (Slice B) anche a questo task, che prima girava per sempre senza un `break`
  raggiungibile. `disc` non richiede più `mut` (entrambi i metodi sono `&self`).

Nessun cambio a `protocol`/`ui`/`mcp-server`. `service.rs` non toccato (in lavorazione
separata).

---

## [0.27.0] — 2026-07-02 — AI Chat hardening: link robustness (#4 generation + #3 non-blocking connect)

### Fixed

- **`src/aichat/service.rs`** — debito #4 ("race di riconnessione"): `PeerGone(PeerId)`
  rimuoveva il link SOLO per `PeerId`. Un `PeerGone` "vecchio" (dal reader di un link già
  morto) processato DOPO una riconnessione dello stesso IP rimuoveva il link FRESCO —
  il peer riconnesso restava senza writer.
  - Fix: `ServiceEvent::PeerGone(PeerId, u64)` — il secondo campo è la GENERAZIONE del
    link che è morto. Nuovo stato: `link_gen_counter: Arc<AtomicU64>` (contatore
    monotono CONDIVISO, stesso pattern già accettato per `believed_leader` — serve
    perché i link nascono in contesti spawnati senza `&mut self`) e
    `link_gen: HashMap<PeerId, u64>` (generazione del link ATTUALMENTE vivo per peer,
    SOLO attore). `handle_event(PeerGone(id, gen))` confronta `gen` con
    `self.link_gen[id]` PRIMA di qualunque altra cosa: se non combaciano, no-op (stale).
  - `spawn_peer_tasks` riceve un nuovo parametro `gen: u64` e lo riporta indietro nel
    `PeerGone` che il reader invia alla propria uscita (EOF/errore/timeout
    keepalive/shutdown).
  - Ogni punto che registra un link vivo ora scrive anche `link_gen`: il ramo accept
    (`new_link_tx` esteso a `(PeerId, UnboundedSender<ChatMsg>, u64)`) e — dopo il fix
    del debito #3 sotto — il ramo `ConnectOutcome::Success`.
- **`src/aichat/service.rs`** — debito #3 ("head-of-line blocking"): `Effect::ConnectTo`
  faceva `TcpStream::connect(addr).await` **inline** dentro `perform`, cioè dentro il
  task dell'attore: un peer irraggiungibile bloccava l'INTERO attore per il timeout del
  sistema operativo (~21s su Windows) — niente ping, niente altri eventi processati.
  - Fix: il connect si sposta in un task **spawnato** (mai `.await`-ato da `perform`),
    con un `tokio::time::timeout(5s, ..)` esplicito (non ci affidiamo più al timeout
    OS). Il task manda il proprio esito su un nuovo canale interno
    `connect_res_tx: UnboundedSender<ConnectOutcome>` (`Success { info, writer_tx, gen }`
    o `Failure { id }` — non è un `ServiceEvent`, stesso trattamento di `new_link_tx`).
  - Nuovo stato SOLO attore: `connecting: HashSet<PeerId>` — guard anti-duplicato:
    `Effect::ConnectTo` per un peer già `connected` o già `connecting` è un no-op
    sincrono (il guard avviene PRIMA dello spawn, quindi osservabile senza I/O reale).
  - Nuovo metodo `apply_connect_outcome(&mut self, ConnectOutcome)`, estratto (non
    `#[cfg(test)]`, logica di produzione reale) per essere testabile iniettando un esito
    sintetico senza aprire un socket: `Success` registra `links`/`connected`/`link_gen`,
    libera `connecting`, invia il `Join` di presentazione (prima inviato inline);
    `Failure` libera solo `connecting` (retry-on-failure preservato).
  - Nuovo (quinto) ramo del `select!` in `run`: `connect_res_rx.recv()` →
    `apply_connect_outcome`.
- Test aggiunti (`service.rs`): `peer_gone_matching_generation_removes_link`,
  `stale_peer_gone_does_not_remove_fresh_link` (debito #4);
  `connect_dispatch_marks_connecting_and_dedups`,
  `connect_success_registers_link_and_sends_join`,
  `connect_failure_clears_connecting_for_retry` (debito #3). Nuovi helper `#[cfg(test)]`:
  `link_gen_for_test`/`set_link_gen_for_test`/`connected_contains_for_test`/
  `links_contains_for_test`/`connecting_contains_for_test`/`connecting_len_for_test`/
  `mark_connecting_for_test`; `mark_connected_for_test` esteso per assegnare anche una
  generazione (dallo stesso contatore condiviso) e un link fittizio in `self.links`.
  Tutti i test preesistenti che costruivano `PeerGone(id)` sono stati migrati alla
  nuova forma a due campi, recuperando la generazione via `link_gen_for_test` dove lo
  scenario passava da `mark_connected_for_test`, o impostandola esplicitamente con
  `set_link_gen_for_test` dove lo scenario connetteva il peer solo via `Consent`
  (mai realmente "connesso" nel senso di `self.connected`).

Slice C del piano di hardening AI Chat (design:
`Docs/superpowers/specs/2026-07-02-aichat-hardening-debts-design.md`, §"Slice C").
`cargo test -p orchestrator`: 419 test lib (416 passed + 3 ignored, +5 rispetto a
0.26.0), 0 failed. `cargo test -p orchestrator --test aichat_relay_loopback -- --ignored`:
2/2 (relay + keepalive reggono al restructuring di connect/reader). `cargo clippy -p
orchestrator --all-targets`: 2 warning preesistenti (`ai_adapter.rs:173`, `ws.rs:491`),
nessuno nuovo. `cargo fmt` deliberatamente NON eseguito (drift preesistente sull'intero
crate). Nessuna modifica a `protocol`/`ui`/`mcp-server`.

---

## [0.26.0] — 2026-07-02 — AI Chat hardening: teardown (shutdown token)

### Fixed

- **`src/aichat/service.rs`** — debito #2 ("teardown reale"): `run` possedeva sia
  `inbox_tx` sia `new_link_tx`, quindi c'era SEMPRE almeno un sender vivo sull'inbox →
  `inbox_rx.recv()`/`new_link_rx.recv()` non tornavano mai `None` e i `break` esistenti
  erano irraggiungibili. Il loop dell'attore girava per sempre e i suoi task figli
  (accept loop, reader/writer per-peer da `spawn_peer_tasks`) sopravvivevano anche a un
  eventuale abort del task `run` — nessun modo pulito di fermare il canale.
- Fix: `run` accetta ora un quarto parametro
  `shutdown: tokio_util::sync::CancellationToken` (lo stesso tipo già usato da `ws::serve`
  e dal comando "q"+invio in `main.rs`). Quarto ramo del `select!`:
  `_ = shutdown.cancelled() => break` — l'UNICO `break` del loop realmente raggiungibile.
  Il token è propagato "verso il basso" a tutta la gerarchia di task, invece di un
  `JoinSet` posseduto da `run` (che raggiungerebbe solo i task spawnati direttamente
  lì — l'accept loop, ma NON i reader/writer per-peer che l'accept loop stesso spawna,
  "figli dei figli"; vedi doc-comment su `run` e §"Slice B" del design doc):
  - `perform` riceve `shutdown: &CancellationToken` e lo clona verso ogni effetto che
    spawna task (`ConnectTo` → `spawn_peer_tasks`; `StartListener` → accept loop).
  - L'accept loop avvolge `listener.accept()` in un `select!` con
    `shutdown_acc.cancelled()`; all'uscita invia `ServiceEvent::ListenerStopped` (fix
    Slice A) SOLO se non stiamo uscendo per shutdown (`!shutdown_acc.is_cancelled()`) —
    altrimenti ri-triggererebbe un bind proprio mentre il processo si sta spegnendo.
  - `spawn_peer_tasks` riceve `shutdown: CancellationToken` e ne clona una copia per il
    writer e una per il reader (due task Tokio distinti): entrambi i loop hanno un ramo
    `select!` su `shutdown.cancelled()` accanto alla logica esistente (mpsc `recv()` per
    il writer; `tokio::time::timeout(DEAD_THRESHOLD, read_line(..))` per il reader,
    comportamento keepalive invariato).
- **`src/main.rs`** — il `CancellationToken` di shutdown globale è ora creato PRIMA del
  blocco "Canale AI Chat" (era creato dopo, subito prima di `ws::serve`) così un clone
  può essere passato a `service.run(inbox_rx, inbox_tx.clone(), shutdown.clone())`. Lo
  stesso token resta condiviso con `ws::serve` e col comando "q"+invio: uno shutdown
  globale ora spegne ordinatamente anche il canale AI Chat.
- Test aggiunto (`#[tokio::test]` in `service.rs`): `run_exits_on_cancellation` — spawna
  `run`, cancella il token, verifica che il `JoinHandle` completi entro 2s (senza fix
  resterebbe pendente indefinitamente).
- `tests/aichat_relay_loopback.rs` — i due call-site esistenti di `.run(...)` aggiornati
  alla nuova firma (token mai cancellato: questi test coprono relay/keepalive, non il
  teardown).

Slice B del piano di hardening AI Chat (design:
`Docs/superpowers/specs/2026-07-02-aichat-hardening-debts-design.md`, §"Slice B" —
aggiornato per riflettere l'uso di `CancellationToken` invece di `JoinSet`, come
richiesto dal supervisore: l'accept loop spawna a sua volta i task reader/writer
per-peer, "figli dei figli" che un `JoinSet` posseduto da `run` non raggiungerebbe).
Nessuna modifica a `protocol`/`ui`/`mcp-server`.

---

## [0.25.9] — 2026-07-02 — AI Chat hardening: listener reset

### Fixed

- **`src/aichat/service.rs`** — debito #5 ("listening non si resetta"): l'accept loop
  TCP spawnato in `perform` per `Effect::StartListener` usciva dal proprio `loop` (errore
  di `listener.accept()`, o `new_link_tx.send(...)` fallito) senza segnalare nulla
  all'attore. `self.listening` restava `true` per sempre, e il guard di idempotenza in
  `perform` (`if self.listening { return }`) impediva qualunque futuro `StartListener` →
  il server smetteva di accettare connessioni **in silenzio**, senza modo di riprendersi.
  Fix: nuovo `ServiceEvent::ListenerStopped` (evento puro, nessun dato), inviato
  dall'accept loop appena il `loop` termina per qualunque motivo. `handle_event` resetta
  `self.listening = false` e richiama subito `decide_and_connect()`: se siamo ancora
  `Role::Server`, questo ri-emette `Effect::StartListener(port)`, che ora supera il guard
  e ri-binda — ripristino immediato invece di aspettare il prossimo `Discovered` UDP
  (~5-7s in LAN reale).
- Test aggiunti (`#[cfg(test)] mod tests` in `service.rs`): `listener_stopped_resets_listening_flag`,
  `listener_stopped_when_server_reemits_start_listener`. Nuovi helper di test
  `listening_for_test`/`set_listening_for_test` per osservare/impostare il flag senza I/O reale.

Slice A del piano di hardening AI Chat (design:
`Docs/superpowers/specs/2026-07-02-aichat-hardening-debts-design.md`, §"Slice A").
Nessuna modifica a `protocol`/`ui`/`mcp-server`.

---

## [0.25.8] — 2026-07-02 — AI Chat: keepalive (rilevazione disconnessioni improvvise)

### Added

- **`src/aichat/wire.rs`** — `ChatMsg::Ping {}` (battito cardiaco, nessun payload) e
  `ChatMsg::PeerLost { label }` (server→client, annuncio di sparizione), additivi.
- **`src/aichat/service.rs`** — `ServiceEvent::Tick` (nuovo, timer da `run()` ogni
  `PING_INTERVAL` = 5s) → un `Ping` per ogni peer in `self.connected` (pura, testata).
  `spawn_peer_tasks` (task READER): `read_line` avvolto in
  `tokio::time::timeout(DEAD_THRESHOLD, ...)` (15s) — silenzio oltre soglia (nessuna riga,
  Ping incluso) → stesso trattamento di EOF/errore (`ServiceEvent::PeerGone`). Un solo
  Ping mancato non basta (il timeout si resetta ad ogni riga ricevuta): serve silenzio
  continuo per l'intera soglia, ~3 cicli di ping mancati.
- **`src/aichat/service.rs`** — arm `PeerGone` esteso: cattura `label`/`era_server`/
  `was_my_server` PRIMA delle rimozioni esistenti (incluso l'azzeramento di
  `reported_leader` del fix Bug 2, invariato). Se il peer sparito era un client del
  server locale: `AiChatChannel::on_server_leave` (nuovo, wiring di `Room::leave()` MAI
  chiamato prima) + broadcast `Roster` aggiornato e `ChatMsg::PeerLost` a tutti i client
  rimasti + `ToUi(AiChatPeerLost)` alla propria UI. Se il peer sparito era il proprio
  server: solo `ToUi(AiChatPeerLost)` locale (nessun altro link). Evento **live-only**:
  non entra mai in `history`/`AiChatHistory`.
- **`src/aichat/channel.rs`** — `AiChatChannel::on_server_leave` (mirror di
  `on_server_join`, finalmente invoca `Room::leave()`).

### Fixed

- Bug preesistente scoperto in questa slice: `Room::leave()` (scritto e testato da
  tempo) non era mai chiamato da nessuna parte — anche una disconnessione **pulita** non
  toglieva mai l'etichetta dai presenti né ribroadcastava il roster ai client rimasti.

Spec: `Docs/superpowers/specs/2026-07-02-aichat-keepalive-design.md`. Nessuna modifica a
`election.rs`/`decide_role`/`reported_leader` — il keepalive fornisce solo un trigger più
affidabile per `PeerGone`, riusando integralmente la rielezione esistente (0.25.7).

---

## [0.25.7] — 2026-07-02 — AI Chat: fix Bug 2 — il terzo peer si autoelegge server (late-joiner election)

### Fixed

- **Causa radice**: `election::elect(members, current)` è sticky solo LOCALMENTE — `current`
  viene dal ruolo già deciso in precedenza dallo stesso nodo. Un processo appena avviato parte
  sempre con `current = None`, quindi la sua prima elezione è un calcolo grezzo
  `members.lowest()` sul roster visibile in quel momento, senza sapere che gli altri membri
  hanno GIÀ un server stabilito da un'elezione precedente più piccola. Confermato dal vivo su
  3 macchine (rumpleteazer, skimble, quaxo): rumpleteazer↔skimble si eleggono correttamente
  (rumpleteazer, IP più basso tra i due), ma quaxo — entrato 3 minuti dopo con l'IP più basso di
  TUTTI — si autoeleggeva server invece di diventare client di rumpleteazer, restando isolato in
  una stella-di-uno. Riportato "Bug 2" in `Docs/HANDOFF.md` fin da 0.25.1.
- **`src/aichat/election.rs`** — `elect()` guadagna un terzo parametro
  `reported: Option<PeerId>`: il leader che un peer scoperto ha annunciato di credere valido.
  Se `current` non sticka nulla: `reported` presente E già nel roster consentito → vince
  (adozione del leader esistente, niente ricalcolo dal proprio IP); `reported` presente ma
  ASSENTE dal roster (consenso umano non ancora dato) → `Undecided`, mai autoelezione;
  `reported: None` → comportamento founding invariato (`members.lowest()`). Tutte le chiamate
  esistenti passano `reported: None` (comportamento bit-per-bit invariato). 3 nuovi test TDD.
- **`src/aichat/channel.rs`** — `decide_role()` guadagna `reported: Option<PeerId>`, passato
  tale e quale a `elect()`. Nuovo test che riproduce lo scenario esatto: roster a 3, `me` con
  l'IP più basso di tutti ma `reported` punta a un membro già presente → `Role::Client`, mai
  `Role::Server`.
- **`src/aichat/wire.rs`** — `Announce` porta un nuovo campo `leader: Option<PeerId>`
  (`#[serde(default)]`, additivo e retro-compatibile con annunci da un binario non ancora
  aggiornato): il leader che il mittente crede attualmente valido. 2 nuovi test round-trip
  (`Some`/`None`).
- **`src/aichat/discovery.rs`** — trait `Discoverer` esteso:
  `announce(&self, leader: Option<PeerId>)` e `next(&mut self) -> Option<(PeerInfo,
  Option<PeerId>)>`. `FakeDiscoverer` aggiornato (coda di tuple, registra l'ultimo `leader`
  passato ad `announce` via `last_announced_leader()` per assertion nei test).
- **`src/aichat/net.rs`** — `UdpDiscoverer::announce`/`next` allineati alla nuova firma:
  `announce` include `leader` nel datagramma serializzato; `next` estrae anche `ann.leader`.
- **`src/aichat/service.rs`** — `AiChatService` guadagna il campo `reported_leader:
  Option<PeerId>`. `ServiceEvent::Discovered` cambia forma in `Discovered(PeerInfo,
  Option<PeerId>)`; l'arm aggiorna `reported_leader` SOLO quando riceve `Some(_)` (un annuncio
  con `leader: None` — peer ancora indeciso — non cancella un'informazione più utile già
  ricevuta da un altro peer). `decide_and_connect()` passa `self.reported_leader` a
  `decide_role`. L'arm `PeerGone(id)` azzera `reported_leader` se coincide con `id` — punto più
  delicato dello spec: SENZA questo, dopo la sparizione del vero leader la rielezione tra i
  superstiti resterebbe bloccata `Undecided` invece di ricadere su `members.lowest()`
  (regressione verificata con un test dedicato, RED dimostrato disabilitando temporaneamente
  la pulizia). Nuovo campo `believed_leader: Arc<Mutex<Option<PeerId>>>` +
  `believed_leader_handle()`: scritto in `decide_and_connect()` nello stesso punto in cui si
  calcola `current_leader` per il log di elezione, letto dal task di scoperta UDP in `main.rs`.
  8 nuovi test TDD, tra cui un end-to-end (tre `AiChatService`, `handle_event` diretto, nessun
  socket) che riproduce l'intero scenario del log — rumpleteazer+skimble eletti, poi quaxo con
  IP più basso deve risultare `Client(rumpleteazer)` — e la variante sull'ordine dei consensi
  (umano consente prima skimble, poi rumpleteazer: quaxo passa per uno stato intermedio
  `Undecided`, mai autoelezione).
- **`src/main.rs`** — plumbing I/O: `service.believed_leader_handle()` clonato PRIMA che `run`
  consumi `service` (`run` prende `self` per valore), passato alla closure del task discovery;
  ad ogni `announce()` legge l'handle e lo inoltra a `disc.announce(leader)`; `Discovered(peer,
  leader)` ricevuto da `disc.next()` inoltrato così com'è nell'inbox. Nota tecnica: il valore
  letto dall'`Arc<Mutex<..>>` va estratto in una variabile PRIMA dell'`.await` — un
  `std::sync::MutexGuard` non è `Send`, quindi non può restare "vivo" nel future attraverso un
  punto di sospensione, altrimenti `tokio::spawn` rifiuta il future.

Design pre-approvato dall'utente: `Docs/superpowers/specs/2026-07-02-aichat-late-joiner-election-design.md`.
Tutti i 396 test lib passano (72 in `aichat`, +13 rispetto a 0.25.6); clippy invariato (2
warning preesistenti in `ai_adapter.rs`/`ws.rs`, nessuno nuovo).

---

## [0.25.6] — 2026-07-01 — Uscita pulita interattiva ("Q" + invio)

### Added

- **`src/ws.rs`** — `serve()` accetta un nuovo parametro `shutdown:
  tokio_util::sync::CancellationToken`; il loop di accept usa `tokio::select!` tra
  `listener.accept()` e `shutdown.cancelled()`, tornando `Ok(())` quando cancellato
  invece di girare per sempre. Nuovo test d'integrazione
  `serve_returns_when_shutdown_token_is_cancelled` (verifica che `serve()` torni entro
  2s dopo la cancellazione). I 3 call site di test esistenti passano
  `CancellationToken::new()` (mai cancellato → comportamento invariato).
- **`src/main.rs`** — `is_quit_command(line)` (pura, 2 test) riconosce "q"/"quit"
  case-insensitive. Un task `spawn_blocking` legge stdin in un loop e cancella il
  token quando arriva il comando, così `serve()` torna e la sequenza di shutdown già
  esistente (`plugin_host.shutdown().await`, kill_on_drop dei plugin) parte come se
  `serve()` fosse terminato per un errore — nessuna nuova logica di cleanup necessaria:
  il processo `mcp-server` esce da solo su EOF dello stdin (chiuso dall'OS all'uscita
  del processo, verificato leggendo `mcp-server/src/main.rs`: `service.waiting()`
  risolve alla chiusura della pipe). Prima di questo, l'unico modo di fermare il
  processo era Ctrl+C (interruzione brusca).

Quando i processi gireranno come servizi, l'uscita pulita sarà innescata dallo stop
del servizio, non da stdin — questo è un affordance esplicitamente temporaneo per
l'uso interattivo.

---

## [0.25.5] — 2026-07-01 — AI Chat: `AiChatSelf` — titolo finestra con la propria etichetta

### Added

- **`src/aichat/service.rs`** — `SetServerTx` emette incondizionatamente
  `Effect::ToUi(AiChatSelf { label: "{label_base}-human" })`, così la finestra-chat sa
  sempre "chi sono io" senza doverlo indovinare dal roster — anche prima che ci sia
  qualsiasi roster/storico da mostrare. Test esistente
  `set_server_tx_event_stores_sender_and_emits_no_effect` rinominato e aggiornato
  (`..._emits_only_self_label`): il comportamento "nessun effetto" non è più vero, ora è
  "solo `AiChatSelf`".
- **`src/telegram/channel.rs`** — arm `ServerMsg::AiChatSelf { .. }` aggiunto al gruppo
  no-op AI Chat (ripple edit, stesso commit di `protocol`).

---

## [0.25.4] — 2026-07-01 — Fix timeout risposte AI lunghe ("error decoding response body")

### Fixed

- **`src/messages_client.rs`** — `HttpMessagesClient` sostituisce il timeout totale unico
  (120s, connect+send+lettura completa del body) con due meccanismi distinti:
  - **`DEFAULT_TOTAL_TIMEOUT` (900s)** — backstop generoso sul client `reqwest`, per
    hang veramente patologici.
  - **`DEFAULT_IDLE_TIMEOUT` (90s)** — timeout di INATTIVITÀ applicato per-chunk dentro
    `create_streaming`: rileva stalli reali in fretta, ma non scatta durante pause
    silenziose legittime (es. Anthropic che esegue `web_search`/`web_fetch` server-side
    fra un delta di testo e l'altro).
  - Causa radice: risposte lunghe (ricerca web + generazione esaustiva) superavano i
    120s totali → reqwest classificava il timeout-durante-lettura-body anche come
    `is_decode()==true`, e il codice catturava solo `e.to_string()` → messaggio opaco
    `"[errore AI] rete: error decoding response body"`, indistinguibile da una vera
    corruzione di rete. Confermato empiricamente (server TCP locale + client
    `.timeout()` corto): `is_timeout()==true`, `is_decode()==true`, `Display` = "error
    decoding response body".
  - Nuovo helper `describe_network_error` antepone `"timeout: "` quando
    `e.is_timeout()` — i futuri timeout sul backstop totale restano diagnosticabili.
  - Costruttore `HttpMessagesClient::with_timeouts` (privato, solo test) per iniettare
    timeout brevi senza aspettare minuti reali. 2 nuovi test TDD.

Vedi memoria di sessione: root cause confermata dall'utente su richiesta esplicita di
verifica di un bug osservato due volte in produzione.

---

## [0.25.3] — 2026-07-01 — AI Chat: fix guard storico su dump più corto (review finale)

### Fixed

- **`src/aichat/service.rs`** — l'arm `ChatMsg::History` (ricezione lato client) ora sostituisce
  lo storico locale SOLO se non siamo il server E il dump ricevuto non è più corto del nostro
  storico locale. Senza questo guard, un client con un messaggio proprio non ancora relayato dal
  vecchio server (morto prima del relay) lo perdeva ricevendo il dump — più corto — dal nuovo
  server appena eletto. Trovato nella review finale whole-feature (Opus) di 0.25.2. Non è un
  merge vero (richiederebbe identità/sequenza per messaggio, fuori scope — task keepalive);
  è una difesa minima contro un dump più povero dello stato locale. 2 nuovi test TDD + 1
  esistente rafforzato per discriminare sostituzione da concatenazione.

---

## [0.25.2] — 2026-07-01 — AI Chat: storico messaggi persistente + log elezione server

### Added

- **`src/aichat/wire.rs` + `src/aichat/service.rs`** — `AiChatService.history: Vec<ChatLine>`:
  storico ordinato dei messaggi (`Say`), popolato incondizionatamente dal ruolo su ogni messaggio
  visto (locale o da peer). `SetServerTx` ri-emette `AiChatHistory` se non vuoto (stesso pattern di
  `last_known_roster`, fix 0.25.1). Nuova variante wire `ChatMsg::History`: il server la manda al
  peer che manda `Join` — solo a lui, non broadcast; il client che la riceve sostituisce il proprio
  storico. Chi viene eletto server dopo una sparizione eredita lo storico "per osmosi" (l'ha già
  visto passare come client), nessun handoff esplicito. 8 nuovi test TDD.
- **`src/aichat/service.rs`** — `AiChatService.last_logged_leader: Option<PeerId>` +
  `label_for`: `decide_and_connect` logga (`tracing::info!`) l'elezione iniziale e ogni cambio
  effettivo di server eletto, su **ogni** orchestrator della stanza (ognuno calcola la propria
  vista). Nessun log ripetuto sui `Discovered` periodici che riconfermano lo stesso leader.
  3 nuovi test TDD.

Vedi `Docs/superpowers/specs/2026-07-01-aichat-history-persistence-design.md`.

---

## [0.25.1] — 2026-07-01 — AI Chat: fix riapertura finestra (presenti vuoti + no ricezione)

### Fixed

- **`src/aichat/service.rs`** — Bug "presenti vuoti / nessuna ricezione dopo riapertura finestra":
  - Aggiunto campo `last_known_roster: Vec<String>` ad `AiChatService`. Viene aggiornato
    ogni volta che il roster cambia (server: `PeerMsg::Join`; client: `PeerMsg::Roster`).
  - `SetServerTx` ora ri-emette `Effect::ToUi(AiChatRoster { last_known_roster })` se il cache
    non è vuoto — ripristina i "presenti" al nuovo webview senza aspettare un nuovo evento di rete.
  - 2 nuovi test TDD: `set_server_tx_replays_last_roster_on_ui_reopen` (scenario server),
    `set_server_tx_replays_roster_received_as_client` (scenario client).

- **`src/ws.rs`** — Aggiunto arm `ClientMsg::AiChatOpen {}` → `ServiceEvent::SetServerTx(out_tx.clone())`.
  Senza questo arm, riaprire la finestra-chat (stessa connessione WS) non ri-registrava il sink
  e il servizio non poteva più pushare messaggi alla UI.

---

## [0.25.0] — 2026-06-28 — AI Chat Slice 1a-ui-A (orchestrazione backend)

> Un solo minor copre l'**intera** slice 1a-ui-A (Tasks 2-10); i moduli si accumulano sotto questa voce.

### Fixed

- **`src/aichat/net.rs` + `src/aichat/service.rs`** — log di scoperta peer ridotto al primo annuncio. `aichat udp: PEER ricevuto …` abbassato da `info!` a `debug!` (non spamma più lo stderr a ogni annuncio UDP, ~7s). In compenso `service.rs` emette `info!("aichat: peer scoperto <ip> (label=…)")` **una sola volta** alla prima `Discovered` di un peer (confronto `!peers.contains_key` prima dell'insert). Una riscoperta dopo `PeerGone` ri-logga una volta sola — comportamento voluto.

### Added

- **`src/aichat/config.rs`** — `AiChatConfig { enabled: bool, label_base: String, chat_port: u16 }` con `Default` (`enabled=false, label_base="lare", chat_port=40100`). `load_or_generate(path)` carica da JSON o genera il default e lo scrive su disco (template editabile dall'utente). `save(path, cfg)` scrive JSON indentato. 2 test TDD.

- **`src/aichat/service.rs`** — `AiChatService`: attore principale del canale AI Chat (Tasks 3-8):
  - `ServiceEvent` enum: `SetServerTx`, `UiClosed`, `Discovered`, `Consent`, `PeerMsg`, `PeerGone`, `HumanSay`.
  - `Effect` enum: `ToUi`, `SendToPeer`, `ConnectTo`, `StartListener`, `Disconnect`.
  - `AiChatService::new(me: PeerInfo)` — costruisce l'attore (no I/O).
  - `handle_event` (pura) — gestisce ogni variante di `ServiceEvent` e produce `Vec<Effect>`:
    - `SetServerTx` → registra il sink WS per i `ServerMsg`.
    - `UiClosed` → cancella il sink (idempotente).
    - `Discovered(info)` → decide il ruolo via `channel.decide_role`; se `Server` → `StartListener`; se `Client` → `ConnectTo`; gestisce gate di consenso (`pending`/`refused`).
    - `Consent { accept }` → se accettato → `ConnectTo` o `StartListener` secondo ruolo; se rifiutato → aggiunge a `refused`.
    - `PeerMsg { from, msg: Say }` → `ToUi(AiChatMessage)` + relay server-side; `Roster` → `ToUi(AiChatRoster)`; `Join` lato server → `on_server_join` + broadcast roster a tutti i client.
    - `PeerGone` → `Disconnect` + rimozione da `pending`/`refused`.
    - `HumanSay` → eco `ToUi(AiChatMessage)` + `SendToPeer` a tutti i client (server) o al server (client).
  - `run(inbox_rx, inbox_tx)` — loop attore: `tokio::select!` su `inbox_rx` + `new_link_rx`; per ogni evento `handle_event` → `for eff { perform(eff) }`; per ogni `(PeerId, tx)` da `new_link_rx` → aggiorna `links`/`connected`.
  - `perform` — esegue gli effetti I/O: `ToUi` → `server_tx.send`; `SendToPeer` → `links[id].send`; `ConnectTo` → `TcpStream::connect` + `spawn_peer_tasks` + `ChatMsg::Join`; `StartListener` → `bind_listener` + accept loop idempotente (flag `listening`); `Disconnect` → rimuove il link.
  - `spawn_peer_tasks` — divide `TcpStream` in writer task (mpsc→righe JSON su `WriteHalf`) e reader task (`BufReader<ReadHalf>`→`PeerMsg`/`PeerGone` re-iniettati nell'inbox).
  - Campi aggiuntivi rispetto allo scheletro iniziale: `links: HashMap<PeerId, UnboundedSender<ChatMsg>>`, `listening: bool`.
  - 10 test TDD nel modulo.

- **`tests/aichat_relay_loopback.rs`** — integration test `#[ignore]` su loopback TCP (Task 8): due `AiChatService` (server=`127.0.0.1:40191`, client=`127.0.0.2`); sequenza `SetServerTx`→`Discovered`→`Consent` (server binds)→`Discovered`+`Consent` (client connects+Join)→`HumanSay`→assert `AiChatMessage` su `server_ui_rx`. Run: `cargo test -p orchestrator --test aichat_relay_loopback -- --ignored`.

- **`src/ws.rs`** — wiring AI Chat per connessione (Task 9):
  - `serve` / `handle_connection` ricevono `Option<UnboundedSender<ServiceEvent>>` come parametro finale.
  - Al connect: `ServiceEvent::SetServerTx(out_tx.clone())` inviato all'inbox (il servizio può pushare `ServerMsg` alla UI).
  - `ClientMsg::AiChatSend { text }` → `ServiceEvent::HumanSay(text)`.
  - `ClientMsg::AiChatJoinConsent { peer_label, accept }` → `ServiceEvent::Consent { peer_label, accept }`.
  - `ClientMsg::AiChatClosed` → `ServiceEvent::UiClosed`.
  - Al disconnect WS: `ServiceEvent::UiClosed` (idempotente).

- **`src/main.rs`** — spawn condizionato canale AI Chat (Task 10):
  - Risolve `aichat.json` in app-data (`%LOCALAPPDATA%\dev.lare.terminal\` o `.lare-data/`), `load_or_generate`.
  - `detect_local_ipv4()` — helper puro (UDP connect trick): `UdpSocket::bind("0.0.0.0:0")` → `connect("8.8.8.8:80")` → `local_addr().ip()`. Nessun pacchetto inviato; il kernel sceglie l'interfaccia. Fallback: `127.0.0.1` con warning.
  - Se `cfg.enabled`: costruisce `PeerInfo` con IP LAN, crea `UnboundedChannel<ServiceEvent>`, spawna `AiChatService::run` (ownership esclusiva), spawna task discovery UDP.
  - Task discovery: loop sequenziale `announce()` → `timeout(7s, disc.next())` → `Discovered`. Forma sequenziale per evitare conflitto borrow `&self`/`&mut self` che `tokio::select!` sullo stesso `UdpDiscoverer` causerebbe. Full-duplex differito a 1a-ui-B.
  - Passa `Some(inbox_tx)` o `None` a `ws::serve`.

### Changed

- **`src/aichat/service.rs` — `handle_event(PeerMsg { Join })`** (Task 7): ramo no-op sostituito da handler server-side con guard `role() == Role::Server`. Il server chiama `channel.on_server_join(label)` (aggiorna `Room`), emette `ToUi(AiChatRoster)` per la propria UI e `SendToPeer(peer, roster_msg.clone())` per ogni client. `Join` non-server e `Leave` restano no-op. Aggiunto test TDD `server_join_updates_room_and_broadcasts_roster` (10 test totali nel modulo).
- **`src/aichat/service.rs` — `handle_event(PeerGone)`** (Task 7 fix minore): il ramo rimuove anche da `pending` e `refused`; un peer disconnesso durante la dialog di consenso non resta bloccato indefinitamente.

### Debito tecnico lasciato

- **DRY su wire JSON-per-riga**: `spawn_peer_tasks` riscrive la logica di `TcpPeerLink` inline (necessario per dividere il `TcpStream`). Refactoring futuro: estrarre `send_json_line`/`recv_json_line` come funzioni libere riusabili.

---

## [0.24.0] — 2026-06-28 — aichat Slice 1a-net (core di rete del canale AI Chat)

> Un solo minor (0.23.0 → 0.24.0) copre l'**intera** slice 1a-net; i moduli si accumulano sotto questa voce.

### Added

- **`src/aichat/mod.rs`** — nuovo modulo canale AI Chat (Slice 1a-net).
- **`src/aichat/peer.rs`** — tipi base peer puri (nessun I/O):
  - `PeerId(pub Ipv4Addr)` — newtype con `Ord` numerico (BTreeMap friendly, serde trasparente).
  - `PeerInfo { id, label_base, chat_port }` — informazioni annunciate dal peer.
  - `Roster` — insieme dei membri (`BTreeMap<PeerId, PeerInfo>`); API `new/upsert/remove/contains/ids/lowest/get/len/is_empty`; `lowest()` = IP più basso = peer eletto come server.
- **`src/aichat/election.rs`** — funzione pura `pub fn elect(members, current) -> Option<PeerId>`, **sticky**: se il server corrente è ancora nel roster resta (niente prelazione da IP più basso), altrimenti si elegge l'IP più basso.
- **`src/aichat/relay.rs`** — funzione pura `pub fn fan_out_targets(from, me, members) -> Vec<PeerId>`: relay a stella. Esclude dai destinatari l'autore (`from`) e il server stesso (`me`); con 2 peer → 0 target; se il server parla (`from == me`) — tutti i peer tranne sé stesso ricevono. 3 test TDD.
- **`src/aichat/wire.rs`** — wire protocol peer↔peer ("Contratto N") e scoperta UDP:
  - `ChatMsg` — enum tagged (`#[serde(tag = "type", rename_all = "snake_case")]`) con varianti `Join { label }`, `Say { from_label, text }`, `Roster { participants: Vec<String> }`, `Leave { label }`. Ogni messaggio è una riga JSON su TCP.
  - `Announce { v, id, label_base, chat_port }` — datagramma UDP broadcast; `id: PeerId` si serializza come stringa IP (serde trasparente via newtype). 3 test TDD (round-trip + tag `"say"` verificato, round-trip `Join`/`Leave`/`Roster`, round-trip `Announce` con IP come stringa).
- **`src/aichat/discovery.rs`** — `PeerTable`: parte PURA della scoperta peer (nessun I/O):
  - `PeerTable::new(ttl_ms)` — crea la tabella con il TTL in ms.
  - `observe(info, now_ms)` — registra/rinnova un peer (sovrascrive il timestamp se già presente, impedendo la scadenza di peer attivi).
  - `expire(now_ms) -> Vec<PeerId>` — rimuove i peer il cui ultimo avvistamento è oltre il TTL; restituisce gli id decaduti (il chiamante può rielezionare il server).
  - `roster() -> Roster` — snapshot del roster corrente (solo peer vivi).
  - Clock iniettato (`now_ms: u64`) → test deterministici senza I/O. 3 test TDD.
  - **Seam**: `pub trait Discoverer: Send { async fn announce(&self) -> std::io::Result<()>; async fn next(&mut self) -> Option<PeerInfo>; }` — seam async per la scoperta UDP (real impl in net.rs).
  - `FakeDiscoverer { incoming: VecDeque<PeerInfo>, announces: AtomicUsize }` — fake in-memory: restituisce annunci prefissati via `VecDeque`, conta `announce()` via `AtomicUsize`. 2 nuovi test TDD (5 totali nel modulo).
- **`src/aichat/transport.rs`** — seam per UNA connessione peer bidirezionale:
  - `SentChat = Arc<Mutex<Vec<ChatMsg>>>` — tipo condiviso per ispezionare i messaggi inviati dal test.
  - `pub trait PeerLink: Send { async fn send(&mut self, ChatMsg) -> io::Result<()>; async fn recv(&mut self) -> io::Result<Option<ChatMsg>>; }` — astrae il link TCP; `Ok(None)` segnala EOF.
  - `FakePeerLink { sent: SentChat, incoming: VecDeque<ChatMsg> }` — fake in-memory: `send()` accoda nel log condiviso, `recv()` rigioca la coda prefissata (EOF quando esaurita). 1 test TDD.
- **`src/aichat/room.rs`** — `Room`: lista dei partecipanti per ETICHETTA:
  - `Room::new()` — stanza vuota (BTreeSet interno, ordine stabile).
  - `join(label: String) -> ChatMsg` — aggiunge idempotente + ritorna snapshot `ChatMsg::Roster`.
  - `leave(label: &str) -> ChatMsg` — rimuove + ritorna snapshot aggiornato.
  - `roster_msg() -> ChatMsg` — snapshot corrente come `ChatMsg::Roster { participants }`.
  - `participants() -> Vec<String>` — copia ordinata dei presenti.
  - 3 test TDD: `join_adds_and_returns_roster_msg`, `join_is_idempotent_and_sorted`, `leave_removes_and_returns_updated_roster`.
- **`src/aichat/channel.rs`** — `AiChatChannel`: unità integrativa — logica pura senza I/O:
  - `Role` — enum `Undecided | Server | Client(PeerId)` (Copy, Debug, PartialEq, Eq).
  - `AiChatChannel::new(me: PeerInfo)` — crea il canale; ruolo iniziale `Undecided`.
  - `role() -> Role` — legge il ruolo corrente.
  - `decide_role(&mut self, members: &Roster) -> Role` — ricalcola il ruolo in modo **sticky** via `elect`.
  - `on_server_join(label: String) -> (ChatMsg, Vec<String>)` — da server: aggiunge etichetta al `Room`; ritorna snapshot `Roster` + lista presenti.
  - `server_relay(from, msg, members) -> Vec<(PeerId, ChatMsg)>` — da server: usa `fan_out_targets`; ritorna coppie `(target, msg.clone())`.
  - 5 test TDD.
- **`src/aichat/net.rs`** — I/O reale di rete (chiude la Slice 1a-net):
  - `TcpPeerLink` — impl reale di `PeerLink` su TCP: JSON-per-riga (`send` = serializza+`\n`+flush, `recv` = `read_line`+deserializza+`trim_end`); `from_stream(TcpStream)` divide il socket con `tokio::io::split`.
  - `UdpDiscoverer` — impl reale di `Discoverer` via UDP broadcast: bind su `0.0.0.0:disc_port`, `SO_BROADCAST`; annunci come JSON `Announce`; filtra i propri annunci (`ann.id == me.id`).
  - `bind_listener(port: u16) -> io::Result<TcpListener>` — helper: bind TCP su porta effimera (0) o fissa.
  - `connect_peer(addr: SocketAddr) -> io::Result<TcpPeerLink>` — helper: connette TCP e avvolge nel link.
  - Aggiunta feature `"net"` a `tokio` in `[dependencies]`.
  - Integration test `#[ignore]` su loopback (`tests/aichat_loopback.rs`): client+server su `127.0.0.1:PORT` effimera si scambiano un `ChatMsg::Say`; verificato GREEN. Nota Windows: `local_addr()` restituisce `0.0.0.0:PORT` → il test usa `127.0.0.1:PORT` esplicitamente (Winsock non instrada `0.0.0.0` al loopback come Linux/POSIX).
  - Totale aichat: 30 test TDD (lib) + 1 integration test `#[ignore]`.

### Fixed (post-review)

- **`src/aichat/channel.rs` — `decide_role`**: il metodo ora include `self.me` nel roster prima dell'elezione. `UdpDiscoverer` filtra i propri annunci, quindi il `Roster` prodotto da `PeerTable` non contiene mai `me`; senza questa inclusione, il nodo con IP più basso si dichiarava `Client` invece di `Server`. Aggiunto test TDD di composizione `lowest_ip_elects_self_even_when_roster_excludes_self` (31 test totali nel modulo aichat).
- **`src/aichat/net.rs` — `UdpDiscoverer::next`**: gli errori transitori di `recv_from` (es. `WSAECONNRESET` su Windows dopo un `send_to` a peer irraggiungibile) ora vengono loggati a `debug` e il loop continua invece di restituire `None` (che segnala chiusura permanente della sorgente).
- **`src/aichat/net.rs` — commenti**: commento `flush` in `TcpPeerLink::send` riscritto correttamente (`WriteHalf<TcpStream>` è senza buffer in userspace; `flush` è no-op; Nagle è controllato da `TCP_NODELAY`/`set_nodelay`); typo `per-datagamma` → `per-datagramma` nel commento del campo `buf`.

---

## [0.23.0] — 2026-06-27 — plugin Slice 1: routing comandi + lazy-spawn + window registry + pump + wiring WS + e2e; +robustezza I1/I2

### Added

- **`src/router.rs`: `plugin_command(input, cmds) -> Option<&str>`** — controlla se l'input
  corrisponde a uno slash command registrato da un plugin (`/counter`, `/calc`, …).
  Chiamata in `ws.rs` PRIMA del normale routing OS/NL/slash.
- **`src/plugins/host.rs`: `PluginHost::activate(id, make) -> Option<u64>`** — lazy-spawn
  al primo comando slash: se il plugin non è in `writers` viene spawnato (spawn_and_handshake),
  poi `Activate{window_id, args:Null}` viene inviato; `windows[wid]=id` registra la finestra.
  Ritorna `Some(window_id)` o `None` su errore.
- **`src/plugins/host.rs`: `PluginHost::route_ui_event(wid, element_id, value)`** — inoltra
  `HostToPlugin::UiEvent` al writer del plugin che possiede `window_id`.
- **`src/plugins/host.rs`: `PluginHost::forget_window(wid)`** — rimuove la registrazione di
  una finestra dopo la chiusura (cleanup).
- **`src/plugins/host.rs`: `PluginHost::set_server_tx(tx)`** — inietta il canale WS nel host;
  i pump task avviati DOPO questa chiamata catturano `Some(tx)` e forwardano i ServerMsg.
- **`src/plugins/transport.rs`: split writer/reader** — `PluginWriter`/`PluginReader` trait +
  `ChildPluginWriter`/`ChildPluginReader` impl (process I/O reale); `FakeWriter`/`FakeReader`
  (test); `plugin_msg_to_server` (bridge PluginToHost→ServerMsg pura); `fake_transport` seam.
- **`src/plugins/host.rs`: pump task** — `tokio::spawn` dentro `spawn_and_handshake` che
  possiede il `reader`; traduce `PluginToHost → ServerMsg` via `plugin_msg_to_server` e
  invia su `server_tx`. `Ready`/`Log` → None (scartati silenziosamente).
- **`src/ws.rs`**: plugin command dispatch + wiring pump→WS:
  - `serve` / `handle_connection` ricevono `Arc<Mutex<PluginHost>>` e
    `Arc<Vec<(String, String)>>` (plugin_commands).
  - `plugin_host.set_server_tx(out_tx.clone())` dopo la creazione del canale persistente.
  - `ClientMsg::PluginUiEvent` → `route_ui_event`.
  - `ClientMsg::PluginWindowClosed` → `forget_window`.
  - `ClientMsg::Command` con input matching `plugin_command` → `tokio::spawn` che chiama
    `activate` con factory `spawn_plugin`; emette `Done{0}` o `Error{RoutingError}`.
- **`crates/orchestrator/tests/plugin_window_e2e.rs`** — test `#[ignore]` end-to-end (Task 7):
  round-trip reale `start(lazy) → set_server_tx → activate → OpenPluginWindow(Count: 0) →
  route_ui_event(inc) → UpdatePluginWindow(Count: 1) → shutdown` con il binario `counter`.
  Run: `cargo test -p orchestrator --test plugin_window_e2e -- --ignored`.

### Fixed (robustezza — I1 + I2)

- **I1a — rispawn dopo crash in `activate`** (`host.rs`): quando `send(Activate)` ritorna `Err`
  (broken pipe = plugin crashato durante l'attivazione), il writer viene rimosso da `writers`.
  Test: `activate_dead_writer_is_removed_so_respawn_is_possible` (RED→GREEN verificato;
  discriminante: `running_count() == 0` post-fix vs `== 1` pre-fix).
- **I1b — rispawn dopo crash in `route_ui_event`** (`host.rs`): quando `send(UiEvent)` ritorna
  `Err`, il writer viene rimosso da `writers` in modo analogo a I1a.
  Test: `route_ui_event_removes_dead_writer_so_respawn_is_possible` (RED→GREEN verificato;
  same discriminante).
- **I2 — timeout Ready-gate** (`host.rs`): `spawn_and_handshake` avvolge `reader.recv()` in
  `tokio::time::timeout(5s)`. Un plugin che non invia `Ready` non congela più l'host.
  Test con `#[tokio::test(start_paused = true)]` + `pending()` fake: il timeout scatta
  virtualmente (0ms reali); test deterministico e istantaneo.

---

## [0.22.0] — 2026-06-26 — plugin host Fase 0

### Security / Hardening

- Discovery rifiuta `id` non validi (anti path-traversal/abs-path): solo ASCII alfanumerici, `_`, `-` ammessi.

### Added

- **Plugin host Fase 0** wired into `main.rs`: `discover()` scansiona
  `%LOCALAPPDATA%\dev.lare.terminal\plugins\` (override: `LARE_PLUGINS_DIR`);
  `PluginHost::start` eager-spawna i plugin trovati, invia `Init`, attende `Ready`;
  `plugin_host.shutdown().await` invivia `Deinit` a tutti prima di uscire.
  Se la cartella `plugins/` è assente `discover` restituisce un vettore vuoto →
  l'avvio non viene bloccato.
- **`crates/orchestrator/tests/plugin_e2e.rs`** — test `#[ignore]` end-to-end che
  esercita la catena reale `Init → Ready → Deinit` con il binario `plugin-ping`
  compilato. Run: `cargo test -p orchestrator --test plugin_e2e -- --ignored`.
- **`crates/plugin-ping`** — primo plugin di riferimento (CHANGELOG + IMPLEMENTATION).

---

## [0.21.0] — 2026-06-25 — streaming OS output via `notifications/progress` (real-time chunks)

### Added — real-time streaming of OS command output lines

Each line produced by an OS command is now forwarded to the UI as a
`ServerMsg::Chunk` **as it arrives**, instead of being buffered until the
command completes.  AI tool-use calls are unaffected (no streaming, full
result returned to the model as before).

#### `tool_client.rs` — `ToolClient` trait + `McpToolClient` + `FakeToolClient`

- **`ToolClient::run_in_session` signature extended** (minor breaking change
  within the crate — all callers are internal):
  ```rust
  async fn run_in_session(
      &self,
      command: &str,
      progress_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
  ) -> CommandResult;
  ```
- **`LareClientHandler`** — new `Clone` struct that wraps
  `rmcp::handler::client::progress::ProgressDispatcher` and implements
  `rmcp::ClientHandler::on_progress` by delegating to the dispatcher.
  This is the missing link: `McpToolClient` previously used `()` as the MCP
  client handler, which silently discards all `notifications/progress`
  notifications.  `LareClientHandler` routes them to `ProgressDispatcher`.
- **`McpToolClient::run_in_session`** — streaming path (when `progress_tx`
  is `Some`):
  1. Generates a unique token string: `format!("lare-{:x}", rand::random::<u32>())`.
  2. Subscribes the dispatcher to that token → `ProgressSubscriber` (a `Stream`).
  3. Spawns a routing task: polls `subscriber.next().await` (via `futures::StreamExt`)
     and forwards each `ProgressNotificationParam.message` to `progress_tx`.
  4. Injects `progress_token` into the MCP tool call args (`serde_json::Value`).
  5. Awaits the routing task after `peer.call_tool()` returns.
  Non-streaming path (`None`): calls `peer.call_tool()` without a token and
  without injecting the extra field (backward compatible).
- **`FakeToolClient::run_in_session`** — when `progress_tx` is `Some`,
  forwards each line of `self.stdout` via the channel before returning
  (so unit tests of `core.rs` work through the streaming path).
- **All existing test call sites** updated: `run_in_session(cmd)` →
  `run_in_session(cmd, None)`.
- New `futures = "0.3"` dependency in `Cargo.toml` (for `StreamExt`).

#### `core.rs` — `handle_os` rewritten with dual-channel streaming

```rust
// Two channels:
// 1. progress_tx/rx  — raw lines from the tool (UnboundedSender<String>)
// 2. chunks_tx/rx    — collected ServerMsg::Chunk objects (assembled from lines)
let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
let (chunks_tx, mut chunks_rx)     = tokio::sync::mpsc::unbounded_channel::<ServerMsg>();

let chunk_task = tokio::spawn(async move {
    while let Some(line) = progress_rx.recv().await {
        let _ = chunks_tx.send(ServerMsg::Chunk { id: id_owned.clone(), content: line });
    }
});

let result = tools.run_in_session(command, Some(progress_tx)).await;
chunk_task.await.ok();
```

After the tasks complete, `handle_os` drains `chunks_rx` into the output
vector.  A **fallback path** handles the edge case where streaming produced
no chunks but `result.stdout` is non-empty (e.g. a tool client that does
not support streaming): emits stdout as a single `Chunk`.  stderr is
appended as a separate `Chunk` when non-empty.  `Done` is always appended last.

#### `cwd_tracking.rs` — decorator signature updated

`CwdTrackingToolClient::run_in_session` updated to match the new trait:
`progress_tx` is forwarded transparently to `self.inner.run_in_session`.

#### `agent.rs` — AI tool-use path unchanged

`dispatch_tool` calls `tools.run_in_session(command, None)`: no streaming
for AI tool-use (the model needs the full result as a string).

#### Cargo.toml

- `futures = "0.3"` added (required by `StreamExt` for `ProgressSubscriber` polling).

#### TDD (RED → GREEN)

| Test | Crate | RED reason | Assertion |
|------|-------|-----------|-----------|
| `streaming_progress_tx_receives_lines` | `mcp-server` | E0061 wrong arity | `Some(tx)` → lines arrive on channel |
| `streaming_none_progress_tx_no_change` | `mcp-server` | same | `None` → no send; output unchanged |
| `fake_run_in_session_with_progress_tx_forwards_lines` | `orchestrator` | E0061 trait mismatch | `FakeToolClient` + `Some(tx)` → lines forwarded |
| `fake_run_in_session_none_progress_tx_no_change` | `orchestrator` | same | `None` → unchanged |

- `cargo test -p mcp-server --lib` → **41 passed**, 0 failed.
- `cargo test -p orchestrator --lib` → **301 passed**, 0 failed, 3 ignored.
- `cargo clippy -p mcp-server -p orchestrator` → 2 pre-existing warnings, no new warnings/errors.

---

## [0.20.0] — 2026-06-25 — stop button + watchdog: CancellationToken su AI + timeout tool 90s

### Added

- **Stop button (`CancelCommand`):** `ClientMsg::CancelCommand { id }` (già nel protocollo)
  è ora gestito in `ws.rs`. Quando arriva, cancella il `CancellationToken` associato al
  comando `id` in corso (se non già terminato).
- **`cancel: Option<CancellationToken>` nei contratti interni:**
  - `AiAdapter::respond` — parametro penultimo (prima di `tx`). `StubAdapter` lo ignora
    (`_cancel`). `ClaudeAdapter` lo controlla all'inizio di ogni iterazione del loop AI:
    se cancellato, scarta lo scambio corrente dalla storia (igiene alternanza ruoli),
    emette `Chunk("⛔ Operazione annullata.")` + `Done{exit_code: Some(130)}` e termina.
  - `handle_command` + `handle_nl` — parametro passato trasparentemente a `ai.respond`.
  - `telegram/channel.rs` call site: `cancel: None` (Telegram non ha stop button per ora).
- **Reader task in `ws.rs`:** `source` (stream WS) viene spostato in un task separato che
  deserializza i frame e li invia su `in_rx: UnboundedChannel<ClientMsg>`. Il loop principale
  legge da `in_rx` — non blocca mai sull'I/O WS.
- **Comandi spawnati in `ws.rs`:** ogni `Command` non-find viene eseguito in un `tokio::spawn`
  separato (history avvolta in `Arc<Mutex<...>>`), così il loop principale può ricevere
  `CancelCommand` mentre il comando è ancora in corso.
- **Watchdog tool 90s:** `McpToolClient::run_in_session` avvolge `peer.call_tool` con
  `tokio::time::timeout(90s)`. Scaduto il timeout, azzera il peer (la connessione MCP viene
  ristabilita alla prossima chiamata) e restituisce un `CommandResult` con un messaggio
  leggibile ("⚠ Timeout…") e `exit_code: -1`.
- `tokio = { features = [..., "time"] }` aggiunto a `Cargo.toml` (richiesto dal timeout).

### Tests (TDD — RED → GREEN)

- `ai_adapter::tests::claude_cancel_before_first_iteration_skips_api_call`: token già
  cancellato → 0 richieste API, `Done{exit_code:130}`, storia pulita.
- `ai_adapter::tests::claude_no_cancel_token_behaves_normally`: `cancel=None` → comportamento
  invariato (1 richiesta, `Done{exit_code:None}`).
- `ai_adapter::tests::respond_cancel_before_first_iteration_emits_done`: `StubAdapter` accetta
  `Some(token)` senza panica.
- `core::tests::handle_command_cancel_param_accepted_without_panic`: parametro `Some(token)`
  propagato correttamente attraverso `handle_command → handle_nl → ai.respond`.
- `tool_client::tests::timeout_message_contains_expected_content`: verifica la struttura del
  messaggio di timeout (contiene "90", "Timeout", "nmap").
- **Test count:** 283 → 299 lib + 11 integration = **310 passed**, 0 failed, 3 ignored.

---

## [0.19.0] — 2026-06-24 — search: `launch` rilegge `PathsConfig` da disco a ogni `/find` (config live)

### Changed — `search/mod.rs` — `SearchContext.cfg` → `cfg_path`; `launch` rilegge da disco

- `SearchContext.cfg: Arc<PathsConfig>` rinominato in `cfg_path: Arc<PathBuf>`: il contesto
  non porta più una copia della config, ma solo il path di `search-paths.json`.
- `launch` chiama `PathsConfig::load_or_generate(&ctx.cfg_path, ...)` a ogni ricerca:
  le modifiche a `search-paths.json` (es. `result_cap` via `/config`) valgono dalla
  ricerca successiva, **senza riavviare l'orchestrator** (config live).
- `main.rs`: continua a chiamare `load_or_generate` all'avvio (genera/migra il file),
  ma il valore restituito non viene tenuto — costruisce `SearchContext { cfg_path, … }`.
- `tests/ws_integration.rs`: helper `make_test_cfg_path()` scrive un config JSON con
  radici vuote in un path univoco (process-id + contatore atomico), evitando che
  `OsPathProvider` aggiunga directory di sistema che saturerebbero `result_cap` nei test.

**Test RED → GREEN:**
- RED: `launch_reloads_result_cap_from_disk` scritto prima dell'implementazione;
  `cargo test -p orchestrator launch_reloads` → `E0560: struct 'SearchContext' has no field
  named 'cfg_path'`. RED genuino.
- GREEN: campo rinominato + `launch` aggiornato; `cargo test -p orchestrator` →
  283 lib + 11 integration = **294 passed**, 0 failed, 3 ignored.
- Clippy: `cargo clippy -p orchestrator --all-targets -- -D warnings` → zero warning.

---

## [0.18.2] — 2026-06-24 — config: result_cap default 1000 → 2000 (editabile, migrato)

### Changed — `search/paths_config.rs`

- `result_cap` (numero massimo di risultati di `/find` prima del troncamento) era già
  un campo **editabile e persistente** in `search-paths.json`; il **default** sale da
  1000 a **2000**.
- Migrazione config `v1 → v2`: i config esistenti ancora al vecchio default (1000)
  salgono automaticamente a 2000 al prossimo avvio; i valori **personalizzati** dall'utente
  restano intatti (`PREVIOUS_DEFAULT_RESULT_CAP` + `CURRENT_CONFIG_VERSION = 2`).

## [0.18.1] — 2026-06-24 — fix: telegramsettings.json risolto dalla cartella di lancio

### Fixed — `main.rs` + `telegram/settings.rs`

All'avvio l'orchestrator fa `set_current_dir(home)` (la shell persistente parte da `~`).
Il path di default di `telegramsettings.json` era **relativo**, quindi dopo il `cd` veniva
cercato nella home invece che dove si lancia l'orchestrator → "canale Telegram disattivo
(telegramsettings.json assente)" anche col file presente nella cartella di progetto.

- `telegram::settings::resolve_path(env_override, launch_dir) -> PathBuf` (nuovo, puro,
  3 test): `LARE_TELEGRAM_SETTINGS` non vuoto vince; altrimenti `telegramsettings.json`
  nella **cartella di lancio**.
- `main.rs`: cattura la `launch_dir` (`current_dir()`) **prima** del `set_current_dir(home)`
  e la passa a `resolve_path`. Override env invariato.

## [0.18.0] — 2026-06-24 — /find performance da radice disco: esclusione sistema + fan-out

### Added — `search/paths_config.rs` — `PathProvider::system_excludes` + migrazione `version`-based

Le cartelle di sistema (pesanti, raramente cercate) sono ora escluse come **dato** in
`search-paths.json`, non come logica hardcoded nel motore di ricerca:

- `PathProvider::system_excludes(&self) -> Vec<String>` — nuovo metodo del trait con default
  `Vec::new()` (retrocompatibile; i test fakes non devono fare override).
- `OsPathProvider` implementa `system_excludes` tramite `system_excludes_impl()` per-OS:
  - **Windows:** `Windows`, `Program Files`, `Program Files (x86)`, `ProgramData`,
    `$Recycle.Bin`, `System Volume Information`, `$WinREAgent`, `Recovery`,
    `PerfLogs`, `AppData`.
  - **Non-Windows:** `vec![]` (pronto per porting futuro).
- `pub const CURRENT_CONFIG_VERSION: u32 = 1` — costante di versione schema config.
- `PathsConfig.version: u32` — nuovo campo (`#[serde(default)]`) che traccia la versione
  del config su disco.
- `default_exclude()` ridotta alla base cross-platform: `["node_modules", ".git"]`.
  La voce morta `AppData/Local/Temp` (era un path-like, non un nome componente) è rimossa.
- **Migrazione `version`-based:** al caricamento di un config esistente con `version < 1`,
  `PathsConfig::migrate(provider)` viene invocato:
  1. Rimuove le voci con `/` o `\` nel nome (path-like, non funzionanti come filtro per nome
     componente; es. `AppData/Local/Temp` rimosso).
  2. Unisce i `system_excludes` del provider corrente (dedup preservando l'ordine).
  3. Imposta `version = CURRENT_CONFIG_VERSION` e riscrive il file.
  - **Idempotenza:** se il config è già a `CURRENT_CONFIG_VERSION`, nessuna migrazione.
  - **Rispetto delle rimozioni utente:** la migrazione avviene solo una volta (version check);
    se l'utente rimuove manualmente una voce da un config già migrato, non viene reintrodotta.
- **Generazione fresh:** `load_or_generate` su file assente/corrotto usa `default_exclude()`
  + `provider.system_excludes()` come valore iniziale di `exclude`, impostando `version = 1`.

6 nuovi test TDD (RED -> GREEN genuino):
- `fresh_config_merges_system_excludes_and_sets_version`
- `migrates_old_config_dropping_dead_entries_and_merging_system`
- `migration_is_idempotent_and_respects_user_removals`
(piu i test aggiornati di `roots.rs` e `mod.rs` per il literal `PathsConfig`).

### Changed — `search/roots.rs` — `Root.max_depth` + fan-out di un livello in `resolve_roots`

**`Root.max_depth: usize`** — nuovo campo (pubblico) su `Root`:
- Ogni root porta il proprio budget di profondita; i root standard ricevono `cfg.max_depth`.
- `walk_root` e il `PauseGate`/pausa/resume del #7 restano invariati nella logica del loop:
  l'unico cambiamento e che `walk_root` usa `root.max_depth` anziche un parametro separato.

**Fan-out di un livello in `resolve_roots`** — i root grandi vengono parallelizzati:
Ogni root sopravvissuto (dopo canonicalizzazione e dedup) viene espanso nelle sue
sottocartelle immediate che non sono escluse e non sono gia root propri. Ogni figlio
diventa un sotto-root con `max_depth = parent.max_depth - 1` e la stessa `source` del
padre. I figli vengono aggiunti al set di prune del padre, cosi `walk_root` del padre
li salta (nessun double-scan).

Dettagli:
- I file (non directory) e i symlink/reparse point non diventano mai sotto-root.
- Cartelle il cui nome e in `cfg.exclude` vengono saltate anche qui (coerente col walker).
- Path gia root (es. cwd dentro un cloud) non vengono duplicati.
- Se `read_dir` fallisce (permessi, path non accessibile), il root resta non espanso
  (comportamento identico a prima: cammina tutto in un singolo task).
- Il fan-out e al massimo di un livello: i figli NON vengono ri-espansi (non ricorsivo).

Effetto combinato con `system_excludes`: da `C:\` il fan-out espande `C:\` nei figli di
primo livello; quelli esclusi (`Windows`, `Program Files`, ecc.) non diventano root propri
e non vengono camminati. I root utente (`Users`, ecc.) diventano task paralleli.

7 nuovi test TDD (RED -> GREEN genuino):
- `roots_carry_cfg_max_depth` (max_depth per-root)
- `fanout_expands_one_level_with_decremented_depth`
- `fanout_skips_excluded_children`
- `fanout_does_not_duplicate_existing_root`
(piu aggiornamento dei literal `Root` nei test esistenti di `walk.rs` e `mod.rs`).

### Changed — `search/walk.rs` — `walk_root` usa `root.max_depth` (parametro rimosso)

`walk_root` perde il parametro `max_depth: usize` (ora viene letto da `root.max_depth`).
Firma aggiornata (7 -> 6 parametri). Nessun cambio di comportamento.

#### TDD cycle

RED e GREEN documentati nei task 1, 2, 3 di questo branch. Esito finale:
`cargo test -p orchestrator` -> verde (tutti i test passano, inclusi i nuovi).
`cargo build -p orchestrator` -> pulito, versione 0.18.0.

---

## [0.17.0] — 2026-06-24 — /find pausa/resume — PauseGate + walk pausabile + ws

### Added — `search/pause.rs` — `PauseGate`

Nuovo modulo `search::pause` con `pub struct PauseGate { paused: Mutex<bool>, cvar: Condvar }`:

- `PauseGate::new() -> Arc<Self>` — crea un gate non in pausa.
- `pause(&self)` — imposta il flag a `true`.
- `resume(&self)` — imposta il flag a `false` e chiama `cvar.notify_all()`.
- `is_paused(&self) -> bool` — legge il flag.
- `wait_while_paused(&self, cancel: &CancellationToken)` — parcheggia il thread chiamante
  mentre `paused && !cancel.is_cancelled()`; usa `wait_timeout(100ms)` per notare il cancel
  durante la pausa senza bruciare CPU. Corretto per thread `spawn_blocking`.

Esportato via `pub use pause::PauseGate` da `search/mod.rs`.

3 test TDD (RED scritto prima dell'implementazione):
- `pause_and_is_paused` — `new` → non paused; `pause` → paused; `resume` → non paused.
- `wait_parks_until_resume` — thread si parcheggia durante la pausa; `AtomicBool` non viene
  settato finché `resume()` non è chiamato dal thread principale.
- `wait_released_by_cancel` — `cancel.cancel()` libera il thread anche senza `resume()`.

### Changed — `search/walk.rs` — `walk_root` riceve `gate: Arc<PauseGate>`

`walk_root` guadagna un settimo parametro `gate: Arc<PauseGate>`. Nel loop di iterazione,
**prima** del check `cancel.is_cancelled()`, viene chiamato `gate.wait_while_paused(&cancel)`.
In questo modo, mentre il gate è in pausa, il walker blocking si ferma tra un'entry e la
successiva senza mai emettere `SearchDone`.

I test esistenti aggiornati: `collect_hit_names` e `cancellation_stops_walk` creano un
`PauseGate::new()` non in pausa (comportamento trasparente per i test esistenti).

### Changed — `search/mod.rs` — `SearchEngine::run` e `launch` ricevono `gate`

- `SearchEngine::run` guadagna `gate: Arc<PauseGate>` e lo clona in ogni task spawn per root.
- `launch` guadagna `gate: Arc<PauseGate>` e lo propaga a `run`.
- Test `emits_open_hits_done_with_sources` e `caps_results_and_marks_truncated` aggiornati
  con `PauseGate::new()` non in pausa.

### Changed — `ws.rs` — registro con pausa + handle Pause/Resume

- Registro ricerche: `HashMap<String, CancellationToken>` → `HashMap<String, (CancellationToken, Arc<PauseGate>)>`.
- Al lancio di `/find`: crea `PauseGate::new()`, lo inserisce nel registro, lo passa a `search::launch`.
- Nuovi arm nel match `ClientMsg`:
  - `PauseSearch { id }` → `gate.pause()` (no-op se id sconosciuto).
  - `ResumeSearch { id }` → `gate.resume()` (no-op se id sconosciuto).
- `CancelSearch { id }` → `token.cancel()` invariato (il `wait_while_paused` nota il cancel al prossimo poll).
- Cancel-all alla chiusura: destruttura `(token, _gate)` e chiama `token.cancel()`.

### Added — test d'integrazione `pause_resume_search_loop_stays_responsive`

In `tests/ws_integration.rs`: lanciata una `/find`, un `PauseSearch` non blocca il loop
(Ping→Pong durante la pausa), un `ResumeSearch` poi un `CancelSearch` chiudono pulito, e un
secondo Ping ottiene Pong confermando che la connessione è viva.

#### TDD cycle (RED → GREEN genuino)

**RED (PauseGate):** `pause.rs` scritto con soli i test (nessuna implementazione).
`cargo test -p orchestrator pause` → `E0432: unresolved import super::PauseGate` +
`E0004: non-exhaustive patterns` in `ws.rs`. RED confermato su 2 errori separati.

**GREEN:** implementazione `PauseGate` + `walk.rs` + `mod.rs` + `ws.rs` aggiornati insieme
(accoppiati: `PauseGate` attraversa l'intera catena).
`cargo test -p orchestrator` → **270 lib + 11 integration = 281 passed, 0 failed, 3 ignored**.
`cargo clippy -p orchestrator --all-targets -- -D warnings` → zero warning.

---

## [0.16.0] — 2026-06-24 — /find — modalità regex re:

### Added — `Matcher::Regex` in `search/query.rs`

Terza modalità di match per `/find`: una query che inizia con `re:` viene interpretata
come regex (case-insensitive, sul nome file) compilata via `regex::RegexBuilder`.

- Nuovo variante `Matcher::Regex(regex::Regex)` nell'enum `Matcher`.
- In `parse_query`, il check `re:` è inserito **prima** del check glob (`*`/`?`),
  così `re:foo*bar` è una regex e non un glob.
- Regex invalida (es. `re:[unclosed`) → `Matcher::Tokens(Vec::new())` → nessun match.
- `is_match`: braccio `Matcher::Regex(re) => re.is_match(filename)` — la
  case-insensitivity è già nel flag del regex compilato, nessuna manipolazione di stringa.
- Nuova dipendenza: `regex = "1"`.

Documentazione aggiornata: `Docs/16-ricerca-file.md` — riga sulla sintassi `re:<pattern>`.

#### TDD cycle (RED → GREEN genuino)

1. **RED** — 4 test aggiunti prima dell'implementazione: `regex_anchored`,
   `regex_unanchored_case_insensitive`, `regex_alternation`, `regex_invalid_matches_nothing`.
   `cargo test -p orchestrator query` → 3 FAILED (`regex_anchored`,
   `regex_unanchored_case_insensitive`, `regex_alternation`); `regex_invalid_matches_nothing`
   passava già (il prefix `re:` era trattato come token-AND e non matchava "anything" — il
   test verifica il comportamento atteso e rimarrà verde nell'implementazione).
2. **GREEN** — variante + branch implementati. `cargo test -p orchestrator` →
   267 lib + 10 integration = **277 passed**, 0 failed, 3 ignored.

4 nuovi test in `search::query::tests`:
- `regex_anchored` — `re:^report-\d{4}\.pdf$` matcha solo il formato esatto.
- `regex_unanchored_case_insensitive` — `re:language` matcha `MyLanguage.pdf` (case fold).
- `regex_alternation` — `re:(foo|bar)` matcha `FOObar.x` e `xBARy`, non `baz`.
- `regex_invalid_matches_nothing` — `re:[unclosed` → nessun match su qualsiasi stringa.

---

## [0.15.0] — 2026-06-23 — router: bare drive-letter X: routes to shell

### Changed — `router::classify_auto` (Rule 3)

Added `is_drive_change` pure helper and Rule 3 in `classify_auto`: a bare drive-letter input
(`c:`, `P:\`) is now routed to `Os` instead of `Nl`, enabling PowerShell drive switching without
requiring the `$` prefix or the `cd` keyword.

New rule in the module doc-comment table: `bare drive-letter → Os`.

2 new tests in `router.rs`:
- `auto_bare_drive_letter_routes_to_os` — `c:`, `C:\`, `p:`, `Z:` all route to `Os`.
- `auto_non_bare_drive_stays_nl` — `c:foo`, `cc:`, `:`, `c:\Users` correctly stay `Nl`.

---

## [0.14.0] — 2026-06-23 — cwd reale tracciata (init home + tracking + /find + ServerMsg::Cwd + /open relativo)

### Added — `CommandResult.cwd` (Step 1)

`CommandResult` (in `tool_client.rs`) guadagna il campo `pub cwd: String`.
- `FakeToolClient` guadagna un nuovo costruttore `with_cwd(stdout, cwd)` che imposta il cwd
  riportato dal fake. Tutti gli altri costruttori impostano `cwd = ""`.
- `McpToolClient::run_in_session` parsa il campo `"cwd"` dal JSON di `mcp-server`
  (`#[serde(default)]` → `""` se assente, per compatibilità con mcp-server < 0.4.0).
- Tutte le path di errore (`CommandResult` nelle funzioni di spawn/handshake) inizializzano `cwd = ""`.

3 nuovi test in `tool_client.rs`:
- `command_result_has_cwd_field` — il campo esiste e vale `""` di default.
- `fake_with_cwd_returns_cwd_in_result` — `FakeToolClient::with_cwd` propaga il valore.
- `mcp_output_cwd_parsed_from_json` — parsing con e senza `"cwd"` nel JSON.

### Added — `CwdTrackingToolClient` (Step 3)

Nuovo modulo `cwd_tracking.rs`: `CwdTrackingToolClient { inner, cwd_state }` implementa
`ToolClient` come decorator (pattern Decorator, SOLID OCP).

- Delega TUTTI i metodi a `inner`.
- Dopo ogni `run_in_session`, se `result.cwd` è non-vuota, aggiorna
  `*cwd_state.lock().await = result.cwd`.
- `open_target` e `reset_session` delegano senza toccare `cwd_state`.
- Copre sia il path OS diretto (ws.rs) sia il path AI (`agent::dispatch_tool`) con un unico gancio.

6 nuovi test in `cwd_tracking.rs`:
- `tracking_updates_cwd_state_when_cwd_non_empty` — cwd aggiornato dopo run.
- `tracking_does_not_overwrite_cwd_when_result_cwd_is_empty` — invariato se `cwd = ""`.
- `tracking_propagates_result_stdout` — il risultato è trasparente.
- `tracking_reflects_most_recent_cwd` — sequenza di comandi aggiorna progressivamente.
- `tracking_is_dyn_compatible` — `Arc<dyn ToolClient>` funziona.
- `open_target_is_delegated` — delegato correttamente.

### Added — Init home + `cwd_state` in `main.rs` (Step 2)

All'avvio:
1. `if let Some(h) = dirs::home_dir() { let _ = std::env::set_current_dir(&h); }`
   — sposta il processo (e il figlio `mcp-server`) nella home dell'utente.
   Best-effort: se `home_dir()` restituisce `None`, il processo resta nella cwd di lancio.
2. `cwd_state = Arc::new(Mutex::new(current_dir()))` — inizializza lo stato condiviso.
3. `tools` è avvolto in `CwdTrackingToolClient::new(raw_tools, cwd_state.clone())`.

### Changed — `ws::serve` + `ws::handle_connection` (Steps 4 e 5)

`ws::serve` guadagna il parametro `cwd_state: Arc<Mutex<String>>` (passato a ogni connessione).

**Emit `ServerMsg::Cwd` iniziale (Step 5):**
Dopo `ServerInfo`, se `cwd_state` non è vuota, emette `ServerMsg::Cwd { path }` direttamente
sul sink (prima di spostarlo nel writer task). Traccia il valore in `last_cwd` (locale,
per-connessione; no Mutex).

**`/find` usa la cwd tracciata (Step 4):**
Sostituisce `std::env::current_dir()` con la lettura di `cwd_state`. Se il `Command`
client include `cwd`, quella ha priorità (retrocompatibilità).

**Emit `ServerMsg::Cwd` dopo ogni comando (Step 5):**
Dopo che `handle_command` ritorna, legge `cwd_state`; se il valore è diverso da `last_cwd`,
emette `ServerMsg::Cwd { path }` su `out_tx` e aggiorna `last_cwd`.

**Fix `telegram/channel.rs`:**
`format_response` aggiunge `ServerMsg::Cwd { .. }` nel ramo infrastruttura (non visibile Telegram).

3 nuovi test d'integrazione in `ws_integration.rs`:
- `connection_emits_initial_cwd` — dopo ServerInfo arriva `Cwd` con la cwd iniziale.
- `command_that_changes_cwd_emits_new_cwd` — dopo un comando che cambia cwd, il server emette `Cwd`.
- `find_uses_tracked_cwd` — `/find` senza `cwd` nel Command usa la cwd tracciata.

### TDD cycle (RED -> GREEN genuino)

**Step 1 (tool_client.rs):**
- RED: `cargo test --lib tool_client` -- E0609 (no field `cwd`) + E0599 (no method `with_cwd`) -- 2 FAILED.
- GREEN: campo aggiunto + costruttore + parsing -- 10 passed.

**Step 3 (cwd_tracking.rs):**
- Il modulo non era dichiarato in `lib.rs` -- compilazione fallisce (RED per simboli mancanti).
- GREEN immediato dopo aggiunta del modulo e dell'implementazione -- 6 passed.

**Steps 4+5 (ws.rs / ws_integration.rs):**
- RED: `cargo test --test ws_integration` -- E0061 (6 argomenti, ne attesi 5) -- compilazione fallisce.
- GREEN: firma aggiornata + logica -- 10 passed (era 7).

**Clippy (T3):** `cargo clippy -p orchestrator --all-targets -- -D warnings` -- zero warning.

**Totale test orchestrator (T3):** 256 lib + 10 integration = 266 passed, 0 failed, 3 ignored.

### Added — `/open` relativo risolto contro la cwd (Task 4)

Nuova funzione pura `resolve_open_target(target: &str, cwd: &str) -> String` in `core.rs`:
- Se `target` inizia con `http://` o `https://` → invariato (URL).
- Se `target` è un path assoluto (`Path::is_absolute`) → invariato.
- Altrimenti (path relativo) → `Path::new(cwd).join(target)` come stringa.

La funzione viene chiamata nel ramo `/open` di `handle_slash` PRIMA di passare il target
a `tools.open_target` (la classificazione URL/folder/file resta in `mcp-server`; qui
si normalizza solo relativo→assoluto a monte). La funzione riceve la cwd dal parametro
`cwd: Option<&str>` di `handle_command` (già esistente, rinominato da `_cwd`).

**Wiring in `ws.rs`:** prima della chiamata a `handle_command`, si calcola
`effective_cwd`: `Command.cwd` del client se non vuoto (retrocompatibilità), altrimenti
`cwd_state.lock().await` (la cwd tracciata della shell). Il valore è passato come
`Some(&effective_cwd)` a `handle_command`, che lo propaga a `handle_slash`.

5 nuovi test in `core::tests` (pure unit, `#[test]`, no async):
- `resolve_open_target_url_https_is_unchanged` — `https://...` → invariato.
- `resolve_open_target_url_http_is_unchanged` — `http://...` → invariato.
- `resolve_open_target_absolute_windows_path_is_unchanged` (`#[cfg(windows)]`) — path assoluto Windows → invariato.
- `resolve_open_target_relative_resolved_against_cwd_windows` (`#[cfg(windows)]`) — `fattura.pdf` + cwd → path assoluto.
- `resolve_open_target_sub_path_relative_resolved_against_cwd_windows` (`#[cfg(windows)]`) — `sub\x.txt` + cwd → path assoluto.

**TDD cycle (Task 4):**
- RED: test aggiunti con riferimento a `resolve_open_target` non esistente.
  `cargo test -p orchestrator --lib core` → E0425 (cannot find function `resolve_open_target`) — 5 errori di compilazione. RED confermato.
- GREEN: funzione implementata + wiring `handle_slash`/`handle_command`/`ws.rs`.
  `cargo test -p orchestrator` → 261 lib + 10 integration = 271 passed, 0 failed, 3 ignored.
- Clippy: `cargo clippy -p orchestrator --all-targets -- -D warnings` → zero warning.

**Totale test orchestrator (T4):** 261 lib + 10 integration = 271 passed, 0 failed, 3 ignored.

---

## [0.13.0] — 2026-06-22 — ricerca file (/find + motore; find_files = follow-up)

### Changed — gate in blocco: una conferma per N tool dello stesso turno (Task 9)

`ClaudeAdapter::respond` ora raccoglie le label di TUTTI i tool soggetti al gate
(`run_in_session`/`open_target`) proposti nello stesso turno e chiede **una sola**
conferma (label unite multi-riga), applicando il verdetto a tutti — così aprire più
file insieme chiede una domanda sola, non N. `show_markdown` non passa dal gate.
Firma di `ToolConfirmer::confirm(&str)` invariata (le label sono unite con `\n`):
nessun impatto su `TelegramConfirmer` né sui test del gate 0.12.0. Nuovo test
`confirmer_batches_multiple_tools_in_one_turn`.

### Added — `ws`: canale d'uscita persistente + launch `/find` + `CancelSearch` (Task 7)

Refactor del transport per supportare la ricerca live e la sua cancellazione:
- **Canale d'uscita persistente**: un solo task scrittore possiede il `sink`; tutto
  passa da `out_tx`. Il read-loop resta libero di leggere il frame successivo (es.
  `CancelSearch`) mentre una ricerca spawnata invia i risultati in streaming. (Prima: `tokio::join!` per
  comando, sequenziale.) ServerInfo dell'handshake è scritto diretto sul sink prima
  di spostarlo nel task scrittore.
- **`/find <query>`** è gestito nel transport (`ws`), non in `core`: ricerca SPAWNATA
  (non blocca il loop), con id proprio (`sid`) e `CancellationToken` nel registro
  `id→token` di proprietà del solo read-loop (nessun Mutex). `CancelSearch{id}` →
  `token.cancel()`. Alla chiusura: cancel-all delle ricerche ancora in corso.
- Comandi non-`/find` restano inline (sequenziali → `history` safe), ora emettono su
  `out_tx`. Stop-on-send-error preservato (client caduto → scrittore finito → send fallisce).
- `search::SearchContext { engine, cfg, provider }` (Clone) + `search::launch(...)`
  (helper sottile: `resolve_roots` + `engine.run`, tiene `ws` privo di logica di ricerca).
  Costruito in `main.rs` da `search-paths.json` (app-data) + `OsPathProvider`.
- cwd ricerca = `Command.cwd` ?? `current_dir()` (NB: non è la cwd reale della shell
  persistente in mcp-server — best-effort per v1).

Test d'integrazione (`tests/ws_integration.rs`): `/find` invia in streaming `SearchOpen→Hit→Done`;
il loop resta responsivo (Ping→Pong durante la ricerca); `CancelSearch` non blocca la
connessione. Nessuna regressione sul flusso comandi esistente (7/7 verdi).

### Added — `SearchEngine::run` (Task 6, ricerca file — fase 1)

Nuovo modulo pubblico in `search/mod.rs` che implementa `SearchEngine`, l'orchestratore
che connette query, roots, walk e messaggi di protocollo:

- `struct SearchEngine { pub max_concurrency: usize }` — cfg-only, nessuno stato tra chiamate.
  `impl Default` con `max_concurrency = 8`.
- `async fn run(id, title, query, roots, cfg, tx, cancel)`:
  1. Emette `SearchOpen { id, title }` su `tx`; se la send fallisce ritorna (receiver sparito).
  2. Compila la query: `Arc::new(parse_query(query))` e `Arc::new(cfg.exclude.clone())`.
  3. Crea `mpsc::channel::<Hit>(256)` (bounded, backpressure) e `Arc<Semaphore::new(max_concurrency)`.
  4. Per ogni root: `tokio::spawn` un task che `sem.acquire_owned().await` (gating), poi
     `walk_root(...)`. Dopo il loop `drop(hit_tx)` — chiude il canale quando tutti i task finiscono.
  5. Drain loop: `while let Some(hit) = hit_rx.recv().await` → `SearchHit`; se `count >= result_cap`
     → `cancel.cancel()`, `truncated = true`, break.
  6. Emette `SearchDone { id, count, truncated }`.

**TDD cycle:**
- **RED** — `run` con body vuoto `{}`. Entrambi i test falliscono con `index out of bounds`
  (msgs vec vuoto) o `subtract with overflow` (msgs.len()-1 su empty). `cargo test` → 2 FAILED.
- **GREEN** — implementazione completa. `cargo test` → 2 passed, 246 totali.
- **Clippy** — `#[allow(clippy::too_many_arguments)]` su `run` (8 parametri). Zero warning.

2 test aggiunti (246 totali, era 244):
- `emits_open_hits_done_with_sources`: 2 root (1 Cwd + 1 Cloud), 3 file totali;
  prima msg SearchOpen, 3 SearchHit contati per source (non per ordine), ultima SearchDone{count:3, truncated:false}.
- `caps_results_and_marks_truncated`: 1 root con 5 file, `result_cap=2`;
  ≤2 SearchHit, SearchDone{count:2, truncated:true}.

### Added — `search::walk` modulo (Task 5, ricerca file — fase 1)

Nuovo modulo `search::walk` che implementa la camminata async di un singolo root
di ricerca, con filtering efficiente via `walkdir::WalkDir::filter_entry`:

- `struct Hit { path: String, source: SearchSource }`: un singolo risultato.
  `path` è il percorso assoluto del file con il prefisso verbatim Windows rimosso.
- `fn normalize_path_string(p: &Path) -> String`: funzione pura che rimuove i
  prefissi verbatim Windows (`\\?\UNC\server\...` → `\\server\...`;
  `\\?\C:\...` → `C:\...`). Controllata prima UNC, poi bare verbatim (perché UNC
  inizia con lo stesso prefisso). Cross-platform (manipolazione stringa).
- `async fn walk_root(root, matcher, max_depth, exclude, tx, cancel)`:
  - Gira in `tokio::task::spawn_blocking` per non bloccare il runtime async.
  - `WalkDir::new(&root.path).max_depth(max_depth).into_iter()`.
  - `filter_entry` applica tre filtri in ordine:
    1. **prune** — se la directory corrisponde a un path in `root.prune`, non
       scende (sotto-alberi camminati come root propri, evita double-scanning).
    2. **exclude** — se il nome del componente è in `exclude` (es. `node_modules`,
       `.git`), salta l'entry (file o dir).
    3. WalkDir's `max_depth` (terzo gate implicito).
  - Per ogni file che supera i filtri: se `matcher.is_match(file_name)` →
    `Hit { path: normalize_path_string(entry.path()), source: root.source.clone() }`
    inviato via `tx.blocking_send`. Se `Err` (receiver chiuso) → stop.
  - Cancellazione cooperativa: `cancel.is_cancelled()` controllato a ogni iterazione
    del loop; se attivo → break immediato.
  - Entry non leggibili (errori IO) skippate silenziosamente (no panic).

Nuove dipendenze: `walkdir = "2"`, `tokio-util = { version = "0.7", features = ["rt"] }`.

**TDD cycle (RED → GREEN genuino):**
1. **RED** — file scritto con stub (`normalize_path_string` ritorna path invariato;
   `walk_root` body vuoto). `cargo test -p orchestrator --lib search::walk` →
   4 FAILED: `normalize_strips_verbatim_prefix`, `normalize_strips_verbatim_unc_prefix`,
   `finds_matching_names_respecting_depth_and_exclude`, `prune_skips_subtree`.
   `cancellation_stops_walk` passava già sullo stub (proprietà: count < 50, stub → 0).
2. **GREEN** — implementazione reale. Tutti e 6 i test passano.
3. **Clippy** — un warning (`while let Some(_)` → `.is_some()`) rimosso; clippy pulito.

6 test aggiunti (244 totali, era 238):
- `normalize_strips_verbatim_prefix`: `\\?\C:\x\y` → `C:\x\y`.
- `normalize_strips_verbatim_unc_prefix`: `\\?\UNC\server\share` → `\\server\share`.
- `normalize_leaves_normal_path_unchanged`: path normale invariato.
- `finds_matching_names_respecting_depth_and_exclude`: albero con a.pdf (depth=1),
  sub/b.pdf (depth=2), node_modules/c.pdf (escluso), x/y/z.pdf (depth=3 > max_depth=2);
  solo a.pdf e b.pdf vengono emessi.
- `prune_skips_subtree`: proj/ in `root.prune` → inside.pdf non emesso; outside.pdf sì.
- `cancellation_stops_walk`: cancel prima del walk → count < 50, funzione ritorna.

### Added — `search::roots` modulo (Task 4, ricerca file — fase 1)

Nuovo modulo `search::roots` che risolve i punti di partenza della ricerca in ordine
di priorità (Cwd → Standard → Cloud → External), con canonicalizzazione, dedup degli
esatti, e calcolo dei `prune` set per i root annidati:

- `struct Root { path: PathBuf, source: SearchSource, prune: Vec<PathBuf> }`:
  rappresenta un root di ricerca pronto per la camminata. `prune` elenca i path
  canonicalizzati degli ALTRI root che sono sotto-cartelle strette di questo root:
  il walker di questo root deve saltare questi sotto-alberi (saranno camminati come
  root propri alla loro priorità). Questo evita il double-scanning preservando
  le priorità (la cwd resta il suo root anche se è dentro una cartella cloud).
- `fn resolve_roots(cwd, provider, cfg) -> Vec<Root>`: funzione pura (solo I/O su disco
  per `canonicalize`). Algoritmo:
  1. Costruisce i candidati in ordine: `[(cwd, Cwd)]` ++ `cfg.expanded_standard()` →
     Standard ++ `cfg.expanded_cloud()` → Cloud ++ `cfg.expanded_external(provider)` →
     External.
  2. Canonicalizza ogni path (`std::fs::canonicalize`, best-effort). Path inesistenti → scartati.
  3. Scarta i duplicati esatti tenendo il PRIMO (priorità più alta).
  4. Per ogni root R sopravvissuto, `prune` = path degli altri root che sono discendenti
     stretti di R (`x.starts_with(R) && x != R`, confronto component-wise su path
     canonicalizzati).
  5. Ritorna `Vec<Root>` nell'ordine di priorità.

Nessuna dipendenza nuova (solo `std::fs::canonicalize` + `std::collections::HashSet`).

**Decisione su canonicalize/Windows `\\?\`:** i path sono tenuti nella forma verbatim
(`\\?\C:\...`) restituita da `canonicalize`. I test confrontano sempre contro
`std::fs::canonicalize(...)` per consistenza. `Path::starts_with` confronta per
componente (non per stringa), quindi funziona correttamente con il prefisso verbatim.

4 test aggiunti (TDD RED → GREEN):
- `order_is_cwd_standard_cloud_external`: 4 dir disgiunti → 4 root nell'ordine atteso,
  tutti con `prune` vuoti.
- `cwd_inside_cloud_keeps_both_and_prunes`: cwd = `base/cloud/proj`, cloud = `base/cloud`
  → entrambi presenti; cloud.prune contiene il path canonicalizzato della cwd; cwd viene
  prima.
- `exact_duplicate_dropped`: cwd == un path standard → un solo Root con source Cwd.
- `nonexistent_path_skipped`: path standard inesistente → non compare.

### Added — `search::paths_config` modulo (Task 3, ricerca file — fase 1)

Nuovo modulo `search::paths_config` che gestisce la configurazione JSON dei percorsi
di ricerca (`search-paths.json`), auto-generata al primo avvio ed editabile dall'utente:

- `trait PathProvider: Send + Sync`: astrazione OS-specifica per il rilevamento di
  cartelle (standard, cloud, drive esterni). Iniettabile nei test via `struct Fake` locale
  (privata al modulo; nessun tipo `FakePathProvider` pubblico in questo task).
- `struct OsPathProvider`: implementazione reale via crate `dirs` (Documents, Downloads,
  Desktop); rilevamento cloud (Windows: Dropbox da `%APPDATA%\Dropbox\info.json`,
  OneDrive da `%OneDrive%`); enumerazione drive esterni (Windows: lettere A:-Z: ≠ C:).
  Tutto isolato con `#[cfg(windows)]` / `#[cfg(not(windows))]`. Non-Windows restituisce
  `vec![]` per cloud e drive (pronto per futuro porting).
- `enum ExternalMode { Auto, List(Vec<String>) }`: serde custom — la stringa `"auto"`
  mapppata su `Auto`, un array JSON su `List(...)`.
- `struct PathsConfig`: config serde con `#[serde(default = ...)]` su ogni campo
  (JSON parziale/editato a mano non causa errori di parse).
- `PathsConfig::load_or_generate(path, provider)`: carica se il file esiste ed è JSON
  valido; altrimenti (assente o corrotto) genera dai default del provider, salva, e
  restituisce. Pattern identico a `telegram::auth::TelegramState::load`.
- `PathsConfig::expanded_standard/cloud/external`: espandono `~` (`dirs::home_dir()`)
  e variabili `%VAR%`/`$VAR` (best-effort, no-panic se non risolvibili).

Dipendenza aggiunta: `dirs = "5"` nella sezione Search di `Cargo.toml`.
11 test aggiunti (generazione, reload, corrotto→default, serde ExternalMode, espansione
tilde, cloud/external expanded).

### Added — `search::query` modulo (Task 2, ricerca file — fase 1)

Nuovo modulo `search::query` (puro, zero dipendenze interne) che implementa il
Matcher per query di ricerca su nome file:
- `parse_query(q: &str) -> Matcher`: se la query contiene `*` o `?`, interpreta come
  glob (case-insensitive sul solo nome file); altrimenti come token-AND (tutte le
  parole, in qualsiasi ordine, case-insensitive, senza ordine).
- `enum Matcher { Glob(GlobMatcher), Tokens(Vec<String>) }`: enum che racchiude
  entrambe le modalità.
- `Matcher::is_match(&self, filename: &str) -> bool`: test del nome file (puro).

Usato dai task successivi (`search::walk`, `SearchEngine::run`) per filtrare i
risultati della ricerca. Dipendenza: `globset = "0.4"`.

---

## [0.12.0] — 2026-06-22 — gate conferma sui tool dell'AI

Da Telegram, ogni tool che l'AI propone da linguaggio naturale (`run_in_session`,
`open_target`) richiede conferma `[Esegui]/[Annulla]` prima dell'esecuzione. La UI
locale resta autonoma (nessun gate). Chiude la "nota di rischio v1" di ADR-007.

### Fixed — feedback sul tap a sessione scaduta (UX, emerso dall'e2e)

`telegram::channel::dispatch_callback`: un tap su un bottone "vecchio" con sessione
scaduta ora risponde **"Sessione scaduta. Usa /login <codice_totp>."** invece di
restare in silenzio (coerente con `dispatch_message`). Le chat non autorizzate
(`Denied`/`NeedPairing`) restano in silenzio (anti-enumerazione). Il comando NON
viene comunque eseguito. Test `dispatch_callback_expired_session_replies_need_login`.

### Added — `telegram::confirm::TelegramConfirmer` + wiring nel canale (Task 3)

`TelegramConfirmer` implementa `ToolConfirmer`: ogni tool che l'AI propone da
Telegram (`run_in_session`/`open_target`) richiede una conferma in-loop via
inline keyboard. Tutti i campi sono prestiti (`&dyn TelegramClient`, `&AtomicI64`,
`&Authenticator`) + un clock iniettabile `Box<dyn Fn() -> u64 + Send + Sync>`;
`Send + Sync` per soddisfare il supertrait di `ToolConfirmer`.

`confirm()`:
1. verifica la sessione (`authorize`) PRIMA di chiedere (scaduta → nega + avviso);
2. id **opaco** a 128 bit (non derivato dal comando) + `confirm_via_poll`;
3. **ri-verifica** la sessione AL TAP (può essere scaduta durante l'attesa).

`telegram::channel`:
- `offset: i64` → `AtomicI64` (condiviso col confirmer; `Ordering::Relaxed`).
- Il loop `run` legge l'offset **fresh** a ogni iterazione e salta gli update già
  consumati dal confirmer (evita il doppio-trattamento di un messaggio intercorrente).
- `run_command_buffered` costruisce un `TelegramConfirmer` con prestiti disgiunti
  da `&mut self.history` e passa `Some(&confirmer)` a `handle_command`.

`FakeTelegramClient` guadagna una modalità opt-in `set_auto_confirm(bool)` (test):
estrae l'id random dal bottone ricevuto e accoda il tap corrispondente.

Test di sicurezza:
- `confirmer_uses_opaque_id_not_derived_from_command` — id a 32 hex, variabile, senza frammenti del comando; il prompt mostra il comando.
- `confirmer_session_expired_at_tap_denies` — sessione valida all'inizio, scaduta al tap → nega anche con tap "Esegui".
- `confirmer_timeout_denies_and_returns` — nessun tap → nega dopo `max_polls`, senza bloccarsi.
- `confirmer_deny_skips_open_target` (in `ai_adapter`) — `open_target` negato non viene eseguito.
- `run_command_reaches_confirmer_via_channel_wiring` (in `channel`) — prova che il wiring passa `Some(&confirmer)`, non `None`.

### Changed — `telegram::confirm` — `Cell<i64>` → `AtomicI64` (Task 2 correzione Send)

Firma aggiornata di `confirm_via_poll`:

```rust
pub async fn confirm_via_poll(
    client: &dyn TelegramClient,
    chat_id: i64,
    command: &str,
    id: &str,
    offset: &AtomicI64,   // era &Cell<i64>
    max_polls: u32,
) -> bool
```

`Cell<i64>` era `!Send`; `AtomicI64` rende la future `Send`, necessario perché
`respond` gira in task spawnati da `ws.rs`. Ordering::Relaxed sufficiente
(nessuna contesa concorrente). I 5 test di `confirm.rs` aggiornati.

### Added — `ai_adapter::ToolConfirmer` + gate in `ClaudeAdapter::respond` (Task 2)

Nuovo trait pubblico:

```rust
#[async_trait]
pub trait ToolConfirmer: Send + Sync {
    async fn confirm(&self, command: &str) -> bool;
}
```

`Send + Sync` obbligatori: senza i supertrait, `Option<&dyn ToolConfirmer>` sarebbe
`!Send` e `ws.rs` (che spawna task) non compilerebbe.

Firma di `AiAdapter::respond` aggiornata (trait, StubAdapter, ClaudeAdapter):

```rust
#[allow(clippy::too_many_arguments)]
async fn respond(
    &self,
    id: &str,
    input: &str,
    history: &mut ConversationHistory,
    tools: &dyn ToolClient,
    opts: agent::TurnOptions,
    confirmer: Option<&dyn ToolConfirmer>,  // penultimo
    tx: UnboundedSender<ServerMsg>,
);
```

Gate nel ramo `else` di `ClaudeAdapter::respond` (solo `run_in_session`/`open_target`;
`show_markdown` non passa dal gate): `None` = esecuzione autonoma; `Some(c)` → `c.confirm(&label).await`;
se negato, emette chunk `"{label} — annullato"` e `ToolResult { is_error: true }`.

`handle_command` e `handle_nl` in `core.rs` guadagnano `confirmer: Option<&dyn ToolConfirmer>`
come penultimo parametro e lo propagano fino a `respond`. Tutti i call site nei test
aggiornati con `None`. `ws.rs` e `telegram/channel.rs` passano `None`
(il canale Telegram riceverà `Some(&TelegramConfirmer)` in Task 3).

Nuovi test in `ai_adapter.rs`:
- `confirmer_deny_skips_tool_execution` — `DenyAll` → tool_result "annullato", tool non eseguito.
- `confirmer_allow_executes_tool` — `AllowAll` → tool_result contiene l'output reale.
- `no_confirmer_executes_autonomously` — `None` → tool eseguito normalmente.

### Added — `telegram::confirm` — round-trip poll↔conferma (Task 1 de-risk)

Nuovo modulo `telegram::confirm` con la funzione pura (Task 1; firma aggiornata
come sopra per la correzione Send di Task 2).

Manda i bottoni `[Esegui|ok:{id}]` / `[Annulla|no:{id}]` e attende il tap
tramite polling single-threaded (niente spawn, niente Arc/Mutex).

Comportamenti coperti da test:
- tap `ok:{id}` dalla chat giusta → `true`; `answer_callback` chiamato; offset avanzato.
- tap `no:{id}` → `false`.
- nessun update dopo `max_polls` cicli → `false` (timeout).
- messaggio intercorrente → invia "Conferma in sospeso: usa i bottoni [Esegui] / [Annulla]." e prosegue.
- tap `ok:{id}` da chat diversa → `answer_callback` chiamato comunque ma ignorato → timeout `false`.

---

## [0.11.4] — 2026-06-22

### Fixed
- `telegram::auth::init`: l'URI/QR di setup TOTP ora viene mostrato a ogni avvio
  **finché la chat non è appaiata** (non più solo alla primissima generazione del
  secret). Così, se al primo avvio si è perso il QR, basta riavviare — senza dover
  cancellare `telegram-state.json`. Dopo il pairing il secret non viene più mostrato.
  Test `init_reuses_secret_and_shows_uri_until_paired`, `init_returns_none_when_already_paired`.

---

## [0.11.3] — 2026-06-22

### Added — QR code a terminale per il setup TOTP (primo avvio Telegram)

Al primo avvio del canale Telegram, `main.rs` stampa su stderr l'URI `otpauth://`
come **QR scansionabile** da Google Authenticator, seguito dall'URI testuale come
fallback per inserimento manuale.

#### Nuovo modulo `telegram::qr`

- `pub fn render_terminal_qr(data: &str) -> Result<String, String>` — funzione pura:
  usa `qr2term::generate_qr_string` che imposta explicit color pairs via `crossterm`
  (bianco su nero / nero su bianco); scansionabile su terminali a sfondo scuro.
  Errori mappati a `String` per composizione semplice.

#### Dipendenza aggiunta

- `qr2term = "0.3"` (risolve a 0.3.3) — MPL-2.0. Dipendenze transitive aggiunte:
  `qrcode`, `crossterm`, `crossterm_winapi` (Windows).

#### Modifica `main.rs` (blocco primo avvio TOTP)

Ordine stampa su stderr (invariato: solo stderr, mai `tracing`):

1. `"[Telegram] Scansiona questo QR con Google Authenticator:"` — titolo.
2. Output di `render_terminal_qr(&uri)`: QR a caratteri ANSI (`eprint!`).
   Se `render_terminal_qr` ritorna `Err`, stampa `"(QR non disponibile: …)"` e
   continua — l'avvio non viene mai bloccato da un fallimento QR.
3. `"[Telegram] Oppure inserisci la chiave manualmente / usa questo URI:"` + URI.
4. `"[Telegram] Conserva l'URI in modo sicuro. Non verrà mostrato di nuovo."`.

Sicurezza: il QR/URI contengono il TOTP secret — solo `eprintln!`/`eprint!`, mai
`tracing::info!` o log persistenti. Invariato rispetto a v0.11.x.

#### TDD cycle (RED → GREEN genuino)

1. **RED** — `telegram/qr.rs` creato con solo il blocco `#[cfg(test)]`
   (riferimento a `super::render_terminal_qr` non esistente) + `pub mod qr` in
   `telegram/mod.rs`.
   `cargo test -p orchestrator --lib telegram::qr` → `E0432: unresolved import
   super::render_terminal_qr` — RED confermato sul simbolo mancante.
2. **GREEN** — aggiunta `render_terminal_qr` (corpo: `qr2term::generate_qr_string`
   + `map_err`). `cargo test -p orchestrator --lib` → 204 passed, 0 failed, 3 ignored.
3. **Clippy** `--lib -D warnings` → nessun warning.

Nuovi test (2 unit in `telegram::qr`; lib: 200 → 204 — le altre 2 provengono da `telegram::auth::tests::totp_matches_rfc6238_sha1_known_vector` e `telegram::settings::tests::token_key_case_insensitive_via_alias` già introdotte in 0.11.1/0.11.2):

| Test | Asserzione chiave |
|------|-------------------|
| `render_valid_otpauth_uri_returns_ok_non_empty` | URI otpauth esempio → `Ok` con stringa non vuota |
| `render_overlong_input_returns_err_not_panic`   | 3000 byte → nessun panic (Err o Ok accettato) |

---

## [0.11.2] — 2026-06-22

### Fixed
- `telegram::settings`: il campo del token in `telegramsettings.json` ora è
  tollerante sul case — accetta `token`, `Token`, `TOKEN` (`#[serde(alias)]`).
  Prima un file con `"Token"` (T maiuscola) dava "invalid data" e disattivava il
  canale. Test `token_key_case_insensitive_via_alias`.

---

## [0.11.1] — 2026-06-22

### Fixed — l'AI ora usa davvero la ricerca web quando è attiva

Con la ricerca web abilitata, il modello ignorava `web_search`/`web_fetch` e
ripiegava su `open_target` (apriva il browser) rispondendo dalla propria
conoscenza ("…basato sulla mia conoscenza, potrebbe non essere aggiornato").
Causa: il `SYSTEM_PROMPT` elencava solo 3 strumenti e non menzionava i tool web.

- `agent.rs`: `SYSTEM_PROMPT` non dice più "tre strumenti" ("i seguenti strumenti");
  nuovo `system_prompt(opts: TurnOptions) -> String` che, se `opts.web_search`,
  appende un addendum: "hai web_search/web_fetch, USALI per RISPONDERE alle domande
  su info aggiornate (prezzi/meteo/notizie) citando la fonte, non limitarti ad
  aprire il browser".
- `ai_adapter.rs`: `respond` usa `agent::system_prompt(opts)` al posto della
  costante. Test `system_prompt_mentions_web_tools_only_when_enabled`.

---

## [0.11.0] — 2026-06-22

### Added — Canale Telegram completo (Tasks 1-5, ADR-007)

Implementazione completa del canale Telegram di Lare Terminal, comprensiva di
settings, client, auth (TOTP + pairing), gate sicurezza, polling loop e wiring
in `main.rs`. Riferimento: `Docs/superpowers/plans/2026-06-22-telegram.md`.

#### `telegram::settings` (Task 1)

- `pub struct TelegramSettings { pub token: String }` — no `Debug` intenzionale (evita stampe accidentali del token).
- `pub fn load(path: &Path) -> std::io::Result<Option<TelegramSettings>>`:
  - `Ok(None)` → file assente (canale disattivato, comportamento normale).
  - `Ok(Some(_))` → file presente, JSON valido `{ "token": "<non-vuoto>" }`.
  - `Err(_)` → file presente ma illeggibile, JSON malformato, campo `token` mancante, o token vuoto.
  - Il token non compare mai nei messaggi di errore.

#### `telegram::client` (Task 1)

Tipi minimi Bot API (solo i campi usati, con `#[serde(default)]` dove utile):
- `Update`, `Message`, `CallbackQuery`, `InlineButton`, `ApiResult<T>` (privato).
- `chat_id` estratto da strutture wire intermedie via `Deserialize` custom.

`#[async_trait] pub trait TelegramClient: Send + Sync` con:
`get_updates`, `send_message`, `send_buttons`, `answer_callback`.

`HttpTelegramClient` (no `Debug` — `base` contiene il token):
- `get_updates` con timeout reqwest = `timeout_s + 10s` (evita taglio del long-poll).
- `without_url()` su ogni `reqwest::Error` (token mai nell'errore).

`FakeTelegramClient` (`#[cfg(test)]`) con coda aggiornamenti e registri.

#### `telegram::auth` (Task 2)

- `TelegramState` (Serialize/Deserialize/Default — no Debug): `totp_secret_base32`, `paired_chat_id`.
  `load` + `save` con gestione file assente/corrotto.
- `Authenticator` con `init`, `authorize`, `try_pair`, `try_login`, `new_pairing_code`, `paired_chat_id`.
- TOTP RFC 6238 via `totp-rs` v5; skew=1 (±30s). Secret in `%LOCALAPPDATA%\dev.lare.terminal\telegram-state.json`.
- Sessioni in-memory (non persistite). Rate-limit 5 fail → lockout 1 min.
- `init` ritorna `Some(otpauth_uri)` solo al primo avvio (secret nuovo). URI mai ri-loggato.

#### `telegram::gate` (Task 3, ADR-007)

- `needs_confirmation(input, kind) -> bool`: richiede conferma per `Route::Os` e `/open`/`/web`.
- `PendingGates` con `insert`, `take` (single-use), `purge_expired`.
- `GATE_TTL_MS = 120_000` (2 minuti).

#### `telegram::channel` (Task 4)

- `TelegramChannel` con loop di polling long-poll (skip backlog, backoff esponenziale).
- `dispatch`: `/pair` → `/login` → `authorize` → `needs_confirmation` (bottoni inline) → `run_command_buffered`.
- `format_response`: Chunk + Done → testo; OpenWindow → `"📄 titolo\n\ncorpo"`; Error → `"[errore] ..."`.
- `split_at_chars`: split char-safe (non byte) per il limite Telegram 4096.
- Silenzio su chat non appaiata e non autorizzata (no information leakage).

#### Wiring `main.rs` + `telegram::run_channel` (Task 5)

- `pub async fn run_channel(client, auth, state_path, ai, tools)` in `telegram/mod.rs`.
- `main.rs`: risolve `telegramsettings.json` (env `LARE_TELEGRAM_SETTINGS` o cwd);
  `Ok(None)` → info disattivo; `Err(e)` → warn (no token); `Ok(Some)` → avvio.
- App-data dir: `%LOCALAPPDATA%\dev.lare.terminal\` (Windows) / `.lare-data/` (fallback).
- `Authenticator::init`: URI TOTP su stderr solo al primo avvio.
- Codice `/pair` su stderr solo se chat non appaiata.
- `tokio::spawn(telegram::run_channel(...))` in parallelo a `ws::serve(...).await`.
- Token Telegram mai nei log.

#### Fix debito `purge_expired` (Task 5)

Nel loop `run` di `TelegramChannel`, `self.gates.purge_expired(now_ms)` ora viene
chiamato ad ogni update ricevuto — i pending gates scaduti vengono rimossi prima
di ogni `dispatch`, evitando l'accumulo indefinito in memoria.

#### ADR-007 — implementato (v0.11.0)

Gate 2FA completo: pairing TOTP + conferma per comandi OS e `/open`/`/web`.
Dettagli in `Docs/06-decisions.md` ADR-007.

### TDD cycle

**Deviazione TDD nota (Tasks 1-4):** implementazione e test co-sviluppati nella
stessa sessione senza un ciclo RED di asserzione comportamentale genuino per ogni
singolo comportamento. Gli errori di compilazione iniziali hanno servito come RED
parziale. Il wiring di `main.rs` (Task 5) e `run_channel` sono I/O/infiniti-loop,
non unit-testabili; la verifica è `cargo build -p orchestrator` + `cargo test` verde.

**GREEN:** `cargo test -p orchestrator` → **200 passed, 0 failed, 3 ignored** (200 unit + 4 integration).
**Clippy:** `cargo clippy -p orchestrator --all-targets -- -D warnings` → nessun warning.

---

## [Storico Task 4 — incluso in 0.11.0] — Task 4 canale Telegram: channel (polling loop + dispatch + mapping)

### Added — `telegram::channel`

Nuovo sottomodulo `src/telegram/channel.rs` (Task 4 del piano `Docs/superpowers/plans/2026-06-22-telegram.md`).

**`pub struct TelegramChannel`** — stato del canale:
- `client: Arc<dyn TelegramClient>` — client HTTP (o Fake nei test); `Arc` per compatibilità `'static` con `tokio::spawn` (Task 5).
- `auth: Authenticator` — pairing/TOTP/sessioni (Tasks 1-2).
- `gates: PendingGates` — pending di conferma (Task 3).
- `history: ConversationHistory` — storia NL per-canale.
- `ai: Arc<dyn AiAdapter>` — adapter AI (`Arc` per `'static`).
- `tools: Arc<dyn ToolClient>` — tool client MCP (`Arc` per `'static`).
- `offset: i64` — offset corrente per `getUpdates`.
- `state_path: PathBuf` — path del file di stato (pairing persistito).

**`pub fn format_response(msgs: &[ServerMsg]) -> String`** — funzione pura:
- `Chunk { content }` → accumulato in buffer.
- `OpenWindow { title, content, .. }` → `"📄 {title}\n\n{content}"`.
- `Done { .. }` → emette il buffer Chunk accumulato.
- `Error { message, .. }` → `"[errore] {message}"`.
- `ServerInfo | Pong` → ignorati (messaggi infrastruttura WS).
- Segmenti separati da `"\n"`. Se nessun contenuto → `""`.

**`fn split_at_chars(text: &str, max_chars: usize) -> Vec<String>`** — split char-safe (emoji = 1 char, non 4 byte) per il limite Telegram 4096.

**`async fn run_command_buffered`** — integrazione con `handle_command`:
1. Crea canale mpsc unbounded.
2. `handle_command(...)` con `web_search=false` (v1).
3. Drain con `try_recv` (tx già droppato al termine).
4. `format_response` + fallback `"(nessun output)"` se vuoto.
5. Split ≤ 4096 char e `send_message` per ogni chunk.

**`async fn dispatch`** — smista un `Update`:
- Callback query: `answer_callback` subito (togliere spinner); verifica `authorize`; parse `data` come `"ok:<id>"` o `"no:<id>"`; gate `take` → esegue o invia "Richiesta scaduta".
- Messaggio: ordine fisso = `/pair` → `/login` → `authorize` → `needs_confirmation` (→ bottoni) → `run_command_buffered`.
- `/pair` e `/login` gestiti PRIMA di `authorize` (altrimenti il login è impossibile).
- Silenzio su chat non appaiata e chat non autorizzata (non rivela l'esistenza del bot).

**`async fn run`** — loop polling long-poll:
- Skip backlog all'avvio (offset = max(update_id)+1).
- Loop: `get_updates(offset, 25)` → aggiorna offset → dispatcha.
- Backoff esponenziale su `Err` (1s → 2s → 4s → max 32s).
- `now_ms` da `SystemTime` (solo in `run`; `dispatch` riceve ms iniettato).

**`pub use channel::TelegramChannel`** esportato da `telegram/mod.rs`.

### TDD cycle (RED → GREEN)

**Deviazione TDD nota:** implementazione e test scritti nello stesso `Write` (stessa sessione, senza un ciclo RED-verde separato per ogni singolo comportamento). Non è stato osservato un RED di asserzione comportamentale genuino prima dell'implementazione. Il primo `cargo build` ha prodotto due errori di compilazione nel codice di produzione — non nel codice dei test e non per simboli mancanti:
1. `E0382`: `CommandKind` non è `Copy` — risolto con `.clone()` in `dispatch_message`.
2. `E0004`: `ServerMsg::ServerInfo` e `ServerMsg::Pong` non coperti nel match di `format_response` — risolto con ramo wildcard `=> {}`.

Il ciclo RED genuino più significativo del modulo è stato il test `dispatch_os_command_from_authorized_sends_confirmation_buttons` che verifica sia la presenza dei bottoni sia l'assenza di esecuzione immediata (property del gate). Aggiunto dopo la review dell'advisor.

**GREEN:** `cargo test -p orchestrator` → **200 passed, 0 failed, 3 ignored** (da 177 a 200 = 23 nuovi test).
**Clippy:** `cargo clippy -p orchestrator --all-targets -- -D warnings` → nessun warning.

### Test suite — `telegram::channel`

23 nuovi test (unit e async tokio):

| Test | Asserzione chiave |
|------|------------------|
| `format_response_empty_slice_returns_empty_string` | `[] → ""` |
| `format_response_single_chunk_and_done` | Chunk+Done → contenuto |
| `format_response_two_chunks_concatenated` | 2 Chunk consecutivi → uniti |
| `format_response_open_window_uses_emoji_prefix` | OpenWindow → `"📄 titolo\n\ncorpo"` |
| `format_response_error_has_prefix` | Error → `"[errore] ..."` |
| `format_response_done_only_returns_empty` | Done solo → `""` |
| `split_at_chars_short_string_is_single_chunk` | ≤4096 → 1 chunk |
| `split_at_chars_splits_correctly` | 4 char, limit 2 → 2 chunk |
| `split_at_chars_respects_char_boundaries` | emoji = 1 char |
| `split_at_chars_empty_string_returns_empty_vec` | `""` → `vec![]` |
| `dispatch_unpaired_chat_sends_nothing_and_does_not_exec` | silenzio totale su chat non appaiata |
| `dispatch_pair_correct_code_confirms` | `/pair <code>` → "Pairing completato" |
| `dispatch_login_correct_totp_opens_session` | `/login <totp>` → "Sessione aperta" |
| `dispatch_nl_command_from_authorized_chat_executes` | NL → stub AI risponde |
| `dispatch_os_command_from_authorized_sends_confirmation_buttons` | `dir` → `send_buttons` con 2 bottoni |
| `dispatch_callback_ok_executes_command` | `"ok:<id>"` → esegue il comando |
| `dispatch_callback_ok_repeated_returns_expired` | secondo `"ok:<id>"` → "scaduta/non valida" |
| `dispatch_callback_no_sends_cancelled_and_removes_gate` | `"no:<id>"` → "Annullato" + gate rimosso |
| `dispatch_expired_session_sends_need_login` | sessione scaduta → "Sessione scaduta/login" |
| `dispatch_wrong_chat_sends_nothing` | chat non autorizzata → silenzio |
| `dispatch_callback_from_wrong_chat_only_answers_callback` | callback non auth → solo `answer_callback` |
| `dispatch_pair_no_code_unpaired_sends_instructions` | `/pair` senza codice, non appaiato → istruzioni |
| `run_command_buffered_empty_output_sends_fallback` | output vuoto → `"(nessun output)"` |

### Note sicurezza

- Token mai nei log/errori (`HttpTelegramClient` usa `without_url()`).
- TOTP secret mai loggato (né in `dispatch` né in `run`).
- `chat_id` + sessione verificati su OGNI update E callback.
- Gate id opaco: `rand::random::<u128>()` formattato come 32-char hex (≤ 64 byte callback data).
- `web_search=false` hardcoded in v1 (nessun toggle Telegram): commento inline nel codice.
- `/pair` e `/login` prima di `authorize` nel branch messaggi.
- Silenzio su chat non appaiata e non autorizzata.

---

## [Storico Task 3 — incluso in 0.11.0] — Task 3 canale Telegram: gate (predicato + pending store)

### Added — `telegram::gate`

Nuovo sottomodulo `src/telegram/gate.rs` (Task 3 del piano `Docs/superpowers/plans/2026-06-22-telegram.md`).

**Funzione pura `needs_confirmation(input: &str, command_type: CommandKind) -> bool`:**
- Riusa `crate::router::classify` come unica fonte di verità sulla route.
- `Route::Os` → `true` (qualsiasi comando shell tocca il sistema).
- `Route::Slash` la cui prima parola (lowercase, after `strip_prefix('/')`) è `"open"` o `"web"` → `true`
  (`/open` lancia file/app/URL via `open_target`; `/web` apre il browser — azioni che toccano il sistema da remoto).
- Tutti gli altri casi (`Route::Nl`, slash non soggetti al gate) → `false`.
- Estrazione prima parola: `input.trim().strip_prefix('/').unwrap_or("").split_whitespace().next().unwrap_or("").to_lowercase()`
  — trim coerente con `classify`, gestisce `"/"` bare senza parole, case-insensitive.

**`pub const GATE_TTL_MS: u64 = 120_000`** — 2 minuti di vita per un pending gate.

**`pub struct PendingGates`** (incapsula `HashMap<String, Pending>`):
- `#[derive(Default)]` + `pub fn new() -> Self` (no `clippy::new_without_default`).
- `pub fn insert(&mut self, id: &str, input: &str, kind: CommandKind, now_ms: u64)`:
  registra il pending con `expiry_ms = now_ms + GATE_TTL_MS`. L'id opaco è fornito dal chiamante (non generato qui).
- `pub fn take(&mut self, id: &str, now_ms: u64) -> Option<(String, CommandKind)>`:
  rimuove sempre l'entry (single-use); ritorna `Some((input, kind))` solo se `now_ms < expiry_ms`, altrimenti `None`.
- `pub fn purge_expired(&mut self, now_ms: u64)`: `retain` su `now_ms < expiry_ms` — garbage collection periodica.

**Struct privata `Pending { input: String, kind: CommandKind, expiry_ms: u64 }`** (non esposta al pub).

**Sicurezza:**
- `now_ms` iniettato ovunque — nessun clock reale in questo modulo.
- `take` è single-use: l'entry viene rimossa sia su uso valido che su scadenza.
- L'id non deriva mai dal contenuto del comando (non forgiabile).

### TDD cycle (RED → GREEN genuino)

Il file è stato creato con il solo blocco `#[cfg(test)]` (referenze a simboli inesistenti), poi
registrato in `telegram/mod.rs`. Prima esecuzione di `cargo test -p orchestrator`:

```
error[E0432]: unresolved imports `super::needs_confirmation`, `super::PendingGates`
error[E0432]: unresolved import `super::GATE_TTL_MS`  (×4)
```

**RED confermato (5 errori di compilazione su simboli mancanti).** Solo dopo aver visto il RED
è stato scritto il codice di produzione.

**GREEN:** `cargo test -p orchestrator` → 177 passed, 0 failed, 3 ignored.
**Clippy:** `cargo clippy -p orchestrator --all-targets -- -D warnings` → nessun warning.

Nuovi test (22 unit in `telegram::gate`; lib ora 177 passed + 3 ignored):

| Modulo | Test | Asserzione chiave |
|---|---|---|
| `telegram::gate` | `os_dir_needs_confirmation` | `dir` (shell token) → `Route::Os` → true |
| `telegram::gate` | `os_dollar_rm_needs_confirmation` | `$ rm x` (`$`-prefix) → `Route::Os` → true |
| `telegram::gate` | `explicit_os_kind_needs_confirmation` | `CommandKind::Os` → true |
| `telegram::gate` | `slash_open_needs_confirmation` | `/open foo` → true |
| `telegram::gate` | `slash_open_uppercase_needs_confirmation` | `/OPEN foo` (case-insensitive) → true |
| `telegram::gate` | `slash_web_needs_confirmation` | `/web gatti` → true |
| `telegram::gate` | `slash_open_with_leading_whitespace_needs_confirmation` | `  /open x` → true |
| `telegram::gate` | `slash_show_does_not_need_confirmation` | `/show ...` → false |
| `telegram::gate` | `slash_reset_does_not_need_confirmation` | `/reset` → false |
| `telegram::gate` | `slash_help_does_not_need_confirmation` | `/help` → false |
| `telegram::gate` | `slash_nowin_does_not_need_confirmation` | `/nowin ...` → false |
| `telegram::gate` | `slash_config_does_not_need_confirmation` | `/config` → false |
| `telegram::gate` | `slash_library_does_not_need_confirmation` | `/library` → false |
| `telegram::gate` | `nl_sentence_does_not_need_confirmation` | frase NL → false |
| `telegram::gate` | `explicit_nl_kind_does_not_need_confirmation` | `CommandKind::Nl` → false |
| `telegram::gate` | `insert_then_take_returns_input_and_consumes` | take → Some, seconda take → None (single-use) |
| `telegram::gate` | `take_unknown_id_returns_none` | id inesistente → None |
| `telegram::gate` | `take_expired_returns_none_and_removes_entry` | past expiry → None + rimosso |
| `telegram::gate` | `take_exactly_at_expiry_boundary_is_expired` | `now_ms == expiry_ms` → None (< non <=) |
| `telegram::gate` | `take_one_ms_before_expiry_succeeds` | `now_ms = expiry_ms - 1` → Some |
| `telegram::gate` | `purge_expired_removes_expired_keeps_valid` | scaduto rimosso, valido intatto |
| `telegram::gate` | `purge_expired_empty_store_does_not_panic` | nessun panic su store vuoto |

---

## [Storico Task 2 — incluso in 0.11.0] — Task 2 canale Telegram: auth (pairing + TOTP + sessioni + rate-limit)

### Added — `telegram::auth` + dipendenza `totp-rs`

Nuovo sottomodulo `src/telegram/auth.rs` (Task 2 del piano `Docs/superpowers/plans/2026-06-22-telegram.md`).

**Dipendenza aggiunta:** `totp-rs = { version = "5", features = ["gen_secret", "otpauth"] }` (v5.7.1).
API usata: `TOTP::new(alg, digits, skew, step, secret_bytes, issuer, account_name)` + `get_url()` +
`generate(secs_u64)` + `check(code, secs_u64)` + `Secret::generate_secret()` + `Secret::Encoded(b32).to_bytes()`.

**`TelegramState`** (Serialize/Deserialize/Default — no Debug intenzionale):
- `totp_secret_base32: Option<String>` — secret TOTP base32; generato una volta.
- `paired_chat_id: Option<i64>` — chat_id Telegram autorizzato.
- `load(path: &Path) -> Self` — file assente/corrotto → default (robustezza al riavvio).
- `save(&self, path: &Path) -> Result<(), String>` — crea dir parent; no secret nell'errore.

**`AuthResult`** (Debug, Clone, PartialEq):
`Ok | NeedPairing | NeedLogin | RateLimited | Denied`

**`Authenticator`** — stato runtime (sessioni in-memory, rate-limit, pairing code):
- `from_state(state: &TelegramState) -> Result<Self, String>` — seam testabile con secret noto.
- `init(state_path: &Path) -> (Self, Option<String>)` — carica o genera secret TOTP;
  prima chiamata → `Some(otpauth_uri)` (da mostrare una volta); seconda → `None`.
- `new_pairing_code(&mut self, now_ms: u64) -> String` — 6 cifre random, TTL = now_ms + PAIRING_TTL_MS.
- `authorize(&self, chat_id: i64, now_ms: u64) -> AuthResult` — 4 rami ordinati:
  `None→NeedPairing`, `wrong_id→Denied`, `no_session→NeedLogin`, `active→Ok`.
- `try_pair(&mut self, chat_id, code, now_ms, state_path) -> AuthResult` — lockout check;
  code valido → appaia + persisti; code errato → incrementa pair_fails, lockout dopo MAX_FAILS.
- `try_login(&mut self, chat_id, totp, now_ms) -> AuthResult` — only paired chat; lockout check;
  TOTP verifica con skew=1 (±30s); ok → session_expiry = now_ms + SESSION_TTL_MS.

**Costanti:**
- `MAX_FAILS = 5` — tentativi prima del lockout.
- `LOCKOUT_MS = 60_000` — durata lockout (1 minuto).
- `SESSION_TTL_MS = 1_800_000` — sessione (30 minuti).
- `PAIRING_TTL_MS = 600_000` — scadenza codice pairing (10 minuti).

**Decisione lockout (spec corretta):** i primi MAX_FAILS tentativi falliti ritornano tutti `Denied`
(l'ultimo dei 5 arma `lockout_until` ma ritorna ancora `Denied`). Solo il tentativo MAX_FAILS+1
(bloccato da `is_locked_out`) ritorna `RateLimited`. Scaduto il lockout, il contatore viene
azzerato → nuova finestra di MAX_FAILS tentativi. Questa semantica è "hai ancora 5 tentativi prima
di essere bloccato" — più user-friendly e allineata alla spec del piano Telegram.

**Decisione ms↔secs:** `totp.check(code, now_ms / 1000)` e `totp.generate(now_ms / 1000)`;
le costanti di scadenza sono tutte in ms. Documentato nel codice per evitare regressioni.

### Fix post-review (advisor)

Due problemi corretti dopo revisione del secondo advisor:

1. **Off-by-one rate-limit** (semantica errata): i test `try_login_rate_limit_after_max_fails` e
   `try_pair_rate_limit_after_max_fails` usavano `for i in 1..MAX_FAILS` (4 iterazioni) → il 5°
   tornava `RateLimited`. Spec: 5 Denied poi il 6° RateLimited. Corretto: loop `1..=MAX_FAILS`
   + step separato per il MAX_FAILS+1-esimo tentativo. Implementazione aggiornata di conseguenza
   (`record_fail_login`/`record_fail_pair` ora ritornano sempre `Denied`; il lockout viene armato
   all'ultimo fail ma `RateLimited` viene prodotto solo da `is_locked_out`).

2. **Panic su secret invalido in `init`** (robustezza): se `totp_secret_base32` era presente ma
   non decodificabile (corruzione disco, scrittura parziale, modifica manuale), `init` panizzava
   con `.expect(...)`. Fix: rileva `Secret::Encoded(b32).to_bytes().is_err()` e rigenera il
   secret come se fosse `None`. Test aggiunto: `init_with_invalid_secret_regenerates_instead_of_panicking`.

### Note TDD

Ciclo RED → GREEN rispettato. Il primo `cargo test` ha fallito per `SecretSize(80)`:
il secret test `JBSWY3DPEHPK3PXP` (16 char = 10 byte) è sotto il minimo di totp-rs (128 bit = 16 byte).
Corretto con `JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP` (32 char = 20 byte). Secondo run: tutti verdi.

Deviazione TDD nota: implementazione e test scritti insieme (non test-first puro), per gli stessi
motivi del Task 1. Il test `init_with_invalid_secret_regenerates_instead_of_panicking` è l'unico
scritto esattamente in ordine RED→GREEN (behavior gap identificato da review).

Nuovi test (34 unit; era 128 → ora 154 — inclusi quelli di Task 1 di cui 3 ignored):

| Modulo | Test | Asserzione chiave |
|---|---|---|
| `telegram::auth` | `telegram_state_load_save_round_trip` | round-trip serializzazione |
| `telegram::auth` | `telegram_state_missing_file_returns_default` | Default su file assente |
| `telegram::auth` | `telegram_state_corrupted_file_returns_default` | Default su file corrotto |
| `telegram::auth` | `telegram_state_save_creates_parent_dir` | mkdir_all su save |
| `telegram::auth` | `init_generates_secret_and_returns_uri_on_first_call` | Some(otpauth://) prima init |
| `telegram::auth` | `init_with_invalid_secret_regenerates_instead_of_panicking` | secret corrotto → rigenera (no panic) |
| `telegram::auth` | `init_reuses_existing_secret_and_returns_none` | None seconda init, stesso secret |
| `telegram::auth` | `init_preserves_paired_chat_id_across_restart` | pair → restart → chat_id ok |
| `telegram::auth` | `authorize_no_paired_chat_returns_need_pairing` | ramo NeedPairing |
| `telegram::auth` | `authorize_wrong_chat_id_returns_denied` | ramo Denied |
| `telegram::auth` | `authorize_paired_no_session_returns_need_login` | ramo NeedLogin |
| `telegram::auth` | `authorize_paired_with_active_session_returns_ok` | ramo Ok |
| `telegram::auth` | `authorize_expired_session_returns_need_login` | sessione scaduta → NeedLogin |
| `telegram::auth` | `try_login_correct_totp_returns_ok_and_sets_session` | TOTP corretto → Ok + sessione |
| `telegram::auth` | `try_login_wrong_totp_returns_denied_no_session` | TOTP errato → Denied |
| `telegram::auth` | `try_login_totp_plus_one_step_is_accepted` | skew=1: codice +30s accettato |
| `telegram::auth` | `try_login_totp_minus_one_step_is_accepted` | skew=1: codice -30s accettato |
| `telegram::auth` | `try_login_totp_minus_two_steps_is_rejected` | skew=1: codice -60s rifiutato |
| `telegram::auth` | `try_login_wrong_chat_id_returns_denied` | chat non appaiata → Denied |
| `telegram::auth` | `try_login_rate_limit_after_max_fails` | 5 falliti → tutti Denied; 6° → RateLimited; 7° durante lockout → RateLimited |
| `telegram::auth` | `try_login_lockout_expires_and_allows_retry` | dopo LOCKOUT_MS → Denied (non RateLimited) |
| `telegram::auth` | `try_login_succeeds_after_lockout_expires` | login ok dopo scadenza lockout |
| `telegram::auth` | `try_pair_correct_code_pairs_and_persists` | codice ok → Ok + persistenza |
| `telegram::auth` | `try_pair_code_is_single_use` | secondo uso → Denied |
| `telegram::auth` | `try_pair_wrong_code_returns_denied` | codice sbagliato → Denied |
| `telegram::auth` | `try_pair_expired_code_returns_denied` | codice scaduto → Denied |
| `telegram::auth` | `try_pair_without_pairing_code_returns_denied` | no codice → Denied |
| `telegram::auth` | `try_pair_rate_limit_after_max_fails` | 5 falliti pairing → tutti Denied; 6° → RateLimited |
| `telegram::auth` | `try_pair_during_lockout_blocks_even_correct_code` | lockout blocca anche codice corretto |
| `telegram::auth` | `try_pair_lockout_expires_and_allows_retry` | dopo lockout → nuovo pair riuscito |
| `telegram::auth` | `full_auth_flow_pair_login_session_expiry` | flusso completo: pair→login→ok→scadenza |
| `telegram::auth` | `unpaired_chat_cannot_login` | nessun chat appaiato → Denied su try_login |
| `telegram::auth` | `only_paired_chat_can_login` | solo chat 42 (non 99) può fare login |
| `telegram::auth` | `telegram_state_can_be_instantiated_and_does_not_need_debug` | no Debug needed |

**Sicurezza verificata:**
- `TelegramState` no `Debug` (secret TOTP non compare nei log per errore).
- `now_ms` iniettato ovunque — nessuna chiamata a clock reale nel codice testato.
- Sessioni solo in-memory (non in `TelegramState`).
- `try_login` non scrive su disco.
- Secret mai loggato (non c'è `eprintln!`/`tracing::debug!` con il secret; URI mostrato solo da `init`).

---

## [Storico Task 1 — incluso in 0.11.0] — Task 1 canale Telegram: settings + client

### Added — `telegram::settings` + `telegram::client` (Bot API seam)

Nuovo modulo `src/telegram/` con i primi due sottomoduli (Task 1 del piano
`Docs/superpowers/plans/2026-06-22-telegram.md`).

**`telegram::settings`**

- `pub struct TelegramSettings { pub token: String }` — no `Debug` intenzionale (evita stampe accidentali del token).
- `pub fn load(path: &Path) -> std::io::Result<Option<TelegramSettings>>`:
  - `Ok(None)` → file assente (canale disattivato, comportamento normale).
  - `Ok(Some(_))` → file presente, JSON valido `{ "token": "<non-vuoto>" }`.
  - `Err(_)` → file presente ma illeggibile, JSON malformato, campo `token` mancante, o token vuoto.
  - Il token non compare mai nei messaggi di errore.

**`telegram::client`**

Tipi minimi Bot API (solo i campi usati, con `#[serde(default)]` dove utile):
- `Update { update_id: i64, message: Option<Message>, callback_query: Option<CallbackQuery> }`
- `Message { chat_id: i64, text: Option<String> }` — `chat_id` estratto da `message.chat.id` via structs wire intermedie + `impl serde::Deserialize<'de>` custom.
- `CallbackQuery { id: String, chat_id: i64, data: Option<String> }` — `chat_id` da `callback_query.message.chat.id`.
- `InlineButton { text: String, callback_data: String }` — solo `Serialize`.
- `ApiResult<T>` (privato) per l'envelope `{"ok":true,"result":[...]}`.

`#[async_trait] pub trait TelegramClient: Send + Sync` con:
- `get_updates(offset: i64, timeout_s: u32) -> Result<Vec<Update>, String>`
- `send_message(chat_id: i64, text: &str) -> Result<(), String>`
- `send_buttons(chat_id: i64, text: &str, buttons: &[InlineButton]) -> Result<(), String>`
- `answer_callback(callback_id: &str) -> Result<(), String>`

`HttpTelegramClient` (no `Debug` — `base` contiene il token):
- `new(token: &str)` → base URL `https://api.telegram.org/bot{token}`.
- `get_updates` → GET `.../getUpdates?offset=...&timeout=...` con timeout reqwest per-request = `timeout_s + 10s` (evita il taglio del long-poll con timeout fisso lato client).
- `send_message` / `send_buttons` → POST `.../sendMessage` (con `reply_markup.inline_keyboard: [[...]]` per i bottoni).
- `answer_callback` → POST `.../answerCallbackQuery` con `callback_query_id`.
- **Sicurezza token:** `reqwest::Error::without_url()` applicato prima di `.to_string()` su ogni errore di rete (l'URL include il token); lo status HTTP è incluso senza il corpo completo; la stringa `self.base` non compare mai negli errori.

`FakeTelegramClient` (`#[cfg(test)]`):
- Coda `Mutex<VecDeque<Vec<Update>>>` per le risposte di `get_updates` (poi `vec![]`).
- Registra `send_message` / `send_buttons` / `answer_callback` in `Mutex<Vec<...>>`.
- Helper: `push_updates`, `recorded_messages`, `recorded_buttons`, `recorded_callbacks`.

### Note sul ciclo TDD

Test e implementazione sono stati co-sviluppati nella stessa sessione. L'unico
fallimento osservato è stato un errore di compilazione nel codice dei test (non nel
codice di produzione):

```
error[E0277]: `TelegramSettings` doesn't implement `std::fmt::Debug`
```

I test usavano `unwrap_err()` che richiede `T: Debug`. Corretti prima di procedere
con `result.err().unwrap()` (nessun bound Debug su T). Questo è un errore nel codice
dei test, non un RED comportamentale: non è stato osservato un test verde su funzione
assente seguita da implementazione che lo fa passare.

**Eccezione parziale:** il test `send_buttons_body_has_correct_reply_markup_shape`
è stato successivamente convertito per chiamare direttamente la funzione pura
`build_send_buttons_body` estratta dalla produzione. Questa modifica è stata
fatta dopo la review e dà un ciclo RED genuino per quella funzione specifica.

Nuovi test (14 unit; era 106 → ora 120):

| Modulo | Test | Asserzione chiave |
|---|---|---|
| `telegram::settings` | `valid_file_returns_some_token` | Ok(Some(token)) |
| `telegram::settings` | `missing_file_returns_none` | Ok(None) |
| `telegram::settings` | `malformed_json_returns_err` | Err |
| `telegram::settings` | `missing_token_field_returns_err` | Err |
| `telegram::settings` | `empty_token_returns_err` | Err + messaggio "token vuoto" |
| `telegram::settings` | `error_message_does_not_contain_file_content` | msg non contiene contenuto raw |
| `telegram::client` | `get_updates_message_deserializes_chat_id_from_nested_chat` | chat_id==42 da message.chat.id |
| `telegram::client` | `get_updates_callback_query_deserializes_chat_id_from_nested_message_chat` | chat_id==99 da callback_query.message.chat.id |
| `telegram::client` | `update_with_neither_message_nor_callback_deserializes_ok` | campi opzionali None |
| `telegram::client` | `send_buttons_body_has_correct_reply_markup_shape` | inline_keyboard[[btn1,btn2]] |
| `telegram::client` | `fake_get_updates_returns_queued_batch_then_empty` | coda + vec![] |
| `telegram::client` | `fake_send_message_records_call` | recorded (chat_id, text) |
| `telegram::client` | `fake_send_buttons_records_call` | recorded (chat_id, text, buttons) |
| `telegram::client` | `fake_answer_callback_records_callback_id` | recorded callback_id |

**Nota sicurezza (per ispezione, non test automatico):** la proprietà "il token non
appare negli errori di `HttpTelegramClient`" è garantita da `e.without_url()` su
ogni `reqwest::Error` e dall'assenza del campo `base` nei messaggi di errore formattati.
Non è coperta da test automatici (richiederebbe un server HTTP finto che ritorna
errori, fuori scope Task 1).

**Nessuna dipendenza nuova aggiunta** (`totp-rs` non serve in Task 1; arriva al Task 2).

---

## [0.10.0] — 2026-06-21

### Added — Comando `/help` (finestra sistema con lista comandi)

- **`HELP_MARKDOWN` constant** — Markdown help text listing all slash commands,
  shell usage, and keyboard shortcuts. Kept as a `const &str` in `core.rs`
  (co-located with `handle_slash`) to make it easy to update when new commands
  are added. No separate module: the content is small and tightly coupled to
  `handle_slash`'s dispatch table.

- **`handle_slash` ramo `"help"`**:
  ```
  "/help" → [OpenWindow{kind: Help, title: "Lare — Comandi", content: HELP_MARKDOWN}, Done{exit_code: Some(0)}]
  ```
  Same two-message pattern as `/show` (established in orchestrator 0.8.0):
  every Command that triggers `commandStarted` in the UI MUST terminate with
  `Done` or `Error`; omitting `Done` leaves the spinner running until disconnect.

  **Deviation from spec**: `Docs/13-window-archive-help.md` says "un solo
  OpenWindow". This was not implemented as specified. `/help` uses the same
  `[OpenWindow, Done{0}]` pattern as `/show` because: (a) the UI wires `/help`
  through `handleSlashCommand` → `commandStarted` → spinner; (b) `commandEnded`
  only fires on `done` or `error`; (c) shipping "un solo OpenWindow" would
  reintroduce the stuck-spinner regression fixed in 0.8.0. The supervisor
  should update the spec document to reflect the correct two-message pattern.

- Dependency on `protocol` bumped implicitly via path: `WindowKind::Help` is now
  used (requires `protocol >= 0.4.0`).

### TDD cycle (RED → GREEN)

**RED** (runtime assertion failure before `"help"` arm existed):
```
thread 'core::tests::slash_help_produces_open_window_and_done' panicked:
assertion `left == right` failed:
expected [OpenWindow{Help}, Done{0}],
got [Error { id: "help-1", code: RoutingError, message: "comando slash sconosciuto: /help" }]
```

**GREEN** after adding `HELP_MARKDOWN` constant and `"help"` arm in `handle_slash`.

New test (106 unit tests; was 105):

| Test | Key assertion |
|------|---------------|
| `slash_help_produces_open_window_and_done` | `[OpenWindow{Help, title∋"Comandi", content∋"/config"}, Done{0}]` |

---

## [0.9.0] - 2026-06-21

### Added
- **Comando `/web <query>`** — apre il browser di default su una ricerca Google esterna.
  `handle_slash` ramo `"web"`: percorso vuoto → `Error{RoutingError}`; altrimenti
  `open_target("https://www.google.com/search?q=<encoded>")` → `Chunk + Done{0/1}`.
  Helper puro `percent_encode_query(s: &str) -> String` (inline, nessuna dipendenza esterna):
  segue RFC 3986 unreserved + percent-encoding %XX UTF-8 per tutti gli altri byte.

- **Ricerca interna AI via tool server-side gated da `TurnOptions.web_search`** —
  `handle_command` accetta ora `web_search: bool` (prima di `tx`); il flag viene propagato
  nei `TurnOptions` costruiti per ogni turno:
  - Intercetto `/nowin` → `TurnOptions { allow_windows: false, web_search }`.
  - Ramo `Route::Nl` → `TurnOptions { allow_windows: true, web_search }`.
  `ws.rs` destructura `web_search` dal `ClientMsg::Command` (non più `_`) e lo passa a
  `handle_command`. Quando `web_search=true`, `tools_for(opts)` aggiunge i tool
  server-side `web_search` e `web_fetch` alla richiesta (`Vec<ToolSpec>` = custom + server).

- **`MessagesRequest.tools` ora `Vec<ToolSpec>`** (custom + server-side) — `ToolSpec`
  è un enum untagged (`Custom(ToolDef)` | `Server(ServerTool)`); `ToolSpec::name() -> &str`.

- **Igiene storia — scarto turni assistant vuoti** — il push del turno assistant salta i
  turni con `blocks.is_empty()` (tipico di `pause_turn` con ricerca web attiva):
  evita content vuoto → HTTP 400 alla chiamata successiva.

### Changed
- **Firma `handle_command`**: aggiunto parametro `web_search: bool` prima di `tx`.
- **`agent::tools_for(opts: TurnOptions) -> Vec<ToolSpec>`** (rimpiazza `tool_defs_for`):
  filtra `show_markdown` se `!allow_windows` e aggiunge tool server-side se `web_search`.
- **`AiAdapter::respond`**: parametro `allow_windows: bool` → `opts: TurnOptions`.

---

## [0.8.0] - 2026-06-21

### Added
- **Comando `/nowin <prompt>`** — forza risposta testo-semplice rimuovendo `show_markdown`
  per quel solo turno (storia della sessione non inquinata).
  Intercettato in `handle_command` prima di `router::classify`; chiama `handle_nl` con
  il prompt pulito e `allow_windows=false`, che passa `tools_for(TurnOptions { allow_windows: false, web_search: false })`
  all'AI (esclude `show_markdown` dalla lista tool). La storia salva solo il prompt originale.
  Input vuoto (`/nowin` senza testo) emette un Chunk suggerimento + `Done{exit_code: Some(1)}`.
  Il comando è case-insensitive (`/NoWin`, `/NOWIN`, ecc.).
  **Loop gate (bugfix):** il branch `show_markdown` nel loop di `ClaudeAdapter::respond`
  è ora condizionato su `opts.allow_windows`. Se il modello emette comunque `show_markdown`
  durante una sessione `/nowin` (es. rievocato dalla storia di un turno precedente normale),
  il loop **non apre alcuna finestra** e restituisce un `ToolResult { is_error: true }`
  che spinge il modello a rispondere come testo semplice. La rimozione del tool da
  `tools_for(TurnOptions { allow_windows: false, .. })` resta utile come primo livello di
  suggerimento; il gate nel loop è la garanzia definitiva anche contro i rievochi dalla storia.
- **Streaming SSE (Fase 2 / Slice 4)** — il testo dell'AI appare token-by-token.
  - Seam evoluto: `AiAdapter::respond` / `core::handle_command` emettono i
    `ServerMsg` su un `tokio::sync::mpsc::UnboundedSender<ServerMsg>` invece di
    ritornare un `Vec`. `ws.rs` fa girare produttore (core) e consumatore (drain
    → WS sink) concorrenti via `tokio::join!`; stop-on-send-error con `rx.close()`.
  - `MessagesClient::create_streaming(req, on_text)` con impl di default
    (fallback non-streaming) e override `HttpMessagesClient` che legge l'SSE
    Anthropic (`bytes_stream`) con byte-framing corretto via `SseAccumulator`
    (parser puro, unit-testato: framing a confini arbitrari, char multibyte
    spezzati, accumulo `input_json_delta` per `tool_use`, `refusal`).
  - `reqwest`: aggiunta la feature `stream`.

### Fixed
- **`/show` ora emette `Done{exit_code: Some(0)}`** dopo `OpenWindow` (conformità
  protocollo "ogni Command termina con Done/Error"; evita spinner UI bloccato).
  Il ramo `/show` vuoto/whitespace → `Error{RoutingError}` rimane invariato.

### Notes
- Backend-only: `renderer.appendChunk` accoda già allo stesso `id`, quindi i
  delta si renderizzano progressivamente senza modifiche al frontend.

---

## [0.7.1] — 2026-06-21

### Fixed — BUG-004: trasparenza solo-comando (niente output nel pannello principale)

**Sintomo (osservato dal vivo):** quando l'AI leggeva un file con `run_in_session` (es.
`Get-Content x`) e poi lo mostrava con `show_markdown`, il contenuto compariva due volte:
come output nel `Chunk` di trasparenza (finestra principale) **e** nella finestra Markdown.

**Causa.** `ai_adapter.rs` — branch `else` del loop (tool non-`show_markdown`): il `Chunk`
di trasparenza emesso era `"{label}\n{output_troncato}"`, quindi l'output del comando
finiva direttamente nel pannello principale.

**Fix:**

- **`ai_adapter.rs`** — `ClaudeAdapter::respond`, branch `else` (dispatch_tool):
  il `Chunk` di trasparenza ora contiene **solo il comando** (`display_invocation`), non
  l'output.  Il `ToolResult` con l'output troncato è invariato: l'AI riceve comunque tutto
  l'output e decide dove presentarlo (finestra Markdown / testo conciso).

- **`agent.rs`** — `SYSTEM_PROMPT`: aggiunta guida esplicita:
  *"Per mostrare il contenuto di un file o un output lungo usa show_markdown e non ripetere
  lo stesso contenuto anche come testo."*

- **Test aggiornato** (`ai_adapter::tests::loop_executes_tool_and_feeds_result_back`,
  assertion (c)): verifica che il `Chunk` di trasparenza contenga il comando (`"$ ls"`)
  ma **non** l'output (`"file.txt"`). RED→GREEN:
  - RED: il vecchio codice produceva `Chunk("$ ls\nfile.txt")` → l'assertion
    `!any(contains("file.txt"))` falliva con *"BUG-004: l'output del comando non deve
    comparire nei Chunk del pannello"*.
  - GREEN: dopo la fix, `msgs = [Chunk("$ ls"), Chunk("Ci sono 3 file."), Done]` —
    `"file.txt"` vive solo nel `ToolResult` nella storia, mai in un `ServerMsg`.

---

## [0.7.0] — 2026-06-21

### Added — Fase 2 / Slice 3: tool `show_markdown` (orchestrator-native) → OpenWindow

- **`agent::tool_defs()`** += terzo tool `show_markdown` — input `{ content: string (req), title?: string }`. Descrizione: mostra Markdown ricco (spiegazioni/codice/tabelle) in una finestra dedicata.

- **`agent::markdown_window(input: &Value) -> (String, String)`** — nuovo helper puro (char-safe):
  - `content` = `input.content`;
  - `title` = `input.title` (se non vuoto, ≤60 char) → prima riga non vuota di `content` (char-safe, ≤60) → fallback `"Lare — Output"`.
  - Stessa logica del `/show` in `core.rs` (ADR-013).

- **`agent::SYSTEM_PROMPT`** aggiornato: "due strumenti" → "tre strumenti" (aggiunto `show_markdown` con descrizione del caso d'uso).

- **Branch `show_markdown` nel loop `ClaudeAdapter::respond`** (passo 9, prima di `dispatch_tool`):
  - Deriva `(title, content)` con `agent::markdown_window`.
  - Emette `ServerMsg::OpenWindow { title, kind: WindowKind::Markdown, content }`.
  - Emette `Chunk` di traccia (`"📄 finestra aperta: {title}"`).
  - Costruisce `Block::ToolResult { ack: "Finestra Markdown aperta.", is_error: false }` e lo rimanda all'AI — ogni `tool_use` ha il suo `tool_result`.
  - Importa `protocol::WindowKind` in `ai_adapter.rs`.

### Test coverage (TDD — 82 test + 2 ignored)

- 4 test nuovi in `agent::tests`:
  - `markdown_window_uses_explicit_title` — titolo da `input.title`.
  - `markdown_window_derives_title_from_first_line` — titolo dalla prima riga di `content`.
  - `markdown_window_falls_back_to_default_title` — fallback `"Lare — Output"` su content vuoto.
  - `markdown_window_truncates_title_to_60_chars` — troncamento ≤60 char char-safe.
- 1 test nuovo in `ai_adapter::tests`:
  - `loop_show_markdown_emits_open_window_and_acks` — verifica: (a) `OpenWindow{Markdown}` con titolo/contenuto corretti; (b) seconda richiesta porta `ToolResult` ack con id `"tu_md"`; (c) `Chunk` di traccia presente.
- Test aggiornato: `claude_text_only_end_turn_returns_text` — `req.tools.len()` aggiornato da 2 a 3.

**RED → GREEN:**
- Task 1: compile-RED (`E0425: cannot find function markdown_window`) su 4 test. Green dopo implementazione `markdown_window`.
- Task 2: runtime-RED (`panicked: atteso un OpenWindow`) — il loop cadeva in `dispatch_tool` → "tool sconosciuto: show_markdown". Green dopo aggiunta branch `if name == "show_markdown"`.

---

## [0.6.0] — 2026-06-21

### Added — Fase 2 / Slice 2: tool-use stateful (no gate)

- **Nuovo modulo `agent`** — funzioni pure per il loop di tool-use:
  - Costanti: `MAX_ITERATIONS=8`, `TRUNCATE_HEAD=6144`, `TRUNCATE_TAIL=2048`.
  - `SYSTEM_PROMPT` — prompt di sistema per l'assistente terminale Windows/PowerShell.
  - `tool_defs() -> Vec<ToolDef>` — definizioni JSON Schema dei tool `run_in_session` + `open_target`.
  - `truncate_for_model(s: &str) -> String` — troncamento char-safe (testa+coda+marcatore `[…troncato N byte…]`).
  - `display_invocation(name, input) -> String` — etichetta leggibile per il `Chunk` di trasparenza.
  - `dispatch_tool(tools, name, input) -> (String, bool)` — esegue `run_in_session`/`open_target` e ritorna `(output, is_error)`.

- **Tipi wire cresciuti in `messages_client`** — sostituisce `ContentBlock`/`String` con tipi strutturati:
  - `Block` enum (tagged serde `type`): `Text`, `ToolUse`, `ToolResult`, `Unknown` (`#[serde(other)]` — scarta `thinking` e blocchi futuri).
  - `Message { role: String, content: Vec<Block> }` con costruttori `user_text(input)` / `with_blocks(role, blocks)`.
  - `ConversationHistory` — storia per-connessione (owned da `ws.rs`); `new/push/to_vec/len/is_empty/truncate`.
  - `ToolDef { name, description, input_schema: Value }` — definizione tool senza schema obbligato.
  - `MessagesRequest` — aggiunta `system: String`, `tools: Vec<ToolDef>`, `thinking: Option<Thinking>` con `skip_serializing_if` (non invia campi vuoti/None: niente `budget_tokens`/`temperature`).
  - `MessagesResponse::assistant_blocks()` — filtra solo `Text` e `ToolUse` per il turno `assistant` in storia.
  - `FakeMessagesClient` (test) — ora registra **tutte** le richieste; `sequence(vec)` (una per chiamata) / `repeating(resp)` (infinita, per test del cap); metodi `ok/err/recorded/nth_request/request_count`.

- **Seam `AiAdapter::respond`** — sostituisce `complete(&str) -> String`:
  - Firma: `async fn respond(&self, id, input, &mut ConversationHistory, &dyn ToolClient) -> Vec<ServerMsg>`.
  - `StubAdapter::respond` — emette `[Chunk("[stub AI] ricevuto: …"), Done(None)]` (comportamento identico a `complete`, test WS invariati).
  - `ClaudeAdapter::respond` — loop tool-use:
    1. Aggiunge turno `user` alla storia.
    2. Invia `MessagesRequest` con system prompt + tool defs + storia (niente `thinking`).
    3. Gestisce errori HTTP e `stop_reason=="refusal"`.
    4. Salva turno `assistant` (solo `text`/`tool_use`, scarta `Unknown`).
    5. Emette testo AI come `Chunk` se presente.
    6. Raccoglie `ToolUse`, dispatcha ciascuno, emette `Chunk` di trasparenza (label + output troncato), costruisce `ToolResult`.
    7. Aggiunge turno `user` con i `ToolResult`.
    8. Ripete; se nessun tool → emette `Done`; se cap raggiunto → `Chunk("[…limite…]") + Done`.

- **Wiring `core.rs`** — `handle_command` aggiunge `history: &mut ConversationHistory` (before `ai`); `handle_nl` delega direttamente a `ai.respond(id, input, history, tools)`.

- **Wiring `ws.rs`** — `handle_connection` crea `let mut history = ConversationHistory::new()` dopo `ServerInfo` e la passa a ogni chiamata `handle_command` (storia per-connessione, stateful).

- **Igiene storia (alternanza ruoli)** — `respond` prende uno snapshot di `history.len()` all'inizio e **tronca allo snapshot** sui path non-puliti (errore HTTP, `refusal`, cap iterazioni): uno scambio fallito non resta in storia, così la chiamata successiva non produce mai turni `user` consecutivi (che l'API può rifiutare con 400). Bug intercettato in review e corretto con test di regressione `history_stays_clean_after_error` (RED→GREEN).

### Test coverage (TDD — 76 test, 2 ignored)

- 7 test in `agent`: troncamento (passthrough, head+tail+marker, char-safe su UTF-8 multi-byte), dispatch `run_in_session` (stdout, exit nonzero), dispatch `open_target`, dispatch tool sconosciuto.
- 6 test in `messages_client`: `one_shot`, serializzazione wire (verifica assenza campi proibiti), `ToolUse` wire, `ToolResult` wire, deserializzazione con `Unknown` (thinking scartato), concatenazione testo, display errore.
- 9 test in `ai_adapter`: stub respond, Claude text-only, **loop canonico** (`loop_executes_tool_and_feeds_result_back` — verifica 2 richieste, tool_result corretto nella 2a, Chunk di trasparenza, Chunk di testo finale), stateful seconda chiamata, cap iterazioni (8 richieste exact), error path, refusal path, provider, **igiene storia dopo errore** (`history_stays_clean_after_error`).
- 4 test integrazione WS invariati (usano `StubAdapter::respond`).

**RED cross-task:** dopo Tasks 1-3 il crate non compilava (errore E0599 in `core.rs`: `ai.complete` non esiste); risolto al Task 4 (unico errore, nessuno nei nuovi moduli). Questo è il RED atteso documentato nel piano.

---

## [0.5.0] — 2026-06-20

### Added — Fase 2 / Slice 1: ClaudeAdapter (solo testo)

- **Nuovo modulo `messages_client`** — seam DIP per la chiamata HTTP alla Anthropic Messages API:
  - Tipi serde: `MessagesRequest`, `Message`, `Thinking`, `MessagesResponse`, `ContentBlock`, `StopDetails`, `MessagesError`.
  - `MessagesRequest::one_shot(model, max_tokens, input)` — costruisce una richiesta one-shot con `thinking:{type:"adaptive"}` e **senza** `budget_tokens`/`temperature`/`top_p`/`top_k` (che causano 400).
  - `MessagesResponse::text()` — concatena i blocchi `type=="text"`, ignora quelli `thinking`.
  - `trait MessagesClient` — seam asincrono e `Send+Sync`; `ClaudeAdapter` dipende da `Arc<dyn MessagesClient>`, non da `reqwest`.
  - `HttpMessagesClient::new(api_key)` / `with_base_url(api_key, base_url)` — impl reale con `reqwest` 0.12 + rustls-tls, timeout 120s, header `x-api-key`/`anthropic-version: 2023-06-01`.
  - `FakeMessagesClient` (solo `#[cfg(test)]`) — registra la richiesta inviata e ritorna una risposta scriptata; usato da `ai_adapter` per testare il loop senza rete.

- **`ClaudeAdapter`** (in `ai_adapter.rs`) — seconda impl di `AiAdapter`, nessuna modifica a `core`/`ws`/`router`/`tool_client` (OCP):
  - Costruttore: `ClaudeAdapter::new(messages: Arc<dyn MessagesClient>, model: String, max_tokens: u32)`.
  - Gestione `stop_reason`:
    - `end_turn` → testo concatenato dai blocchi `text`.
    - `max_tokens` → testo parziale + `"\n[…risposta troncata: max_tokens]"`.
    - `refusal` → `"[AI: richiesta rifiutata — <categoria>]"` (con categoria da `stop_details`).
    - Errore HTTP/rete → `"[errore AI] <causa>"`.

- **`AiAdapter::provider()` (metodo di default)** — aggiunto al trait; default `"unknown"`.
  - `StubAdapter` override: `"stub"`.
  - `ClaudeAdapter` override: nome del modello (`"claude-sonnet-4-6"` per default).

- **Wiring `main.rs`** — selezione adapter in base all'env:
  - `ANTHROPIC_API_KEY` presente e non vuota → `ClaudeAdapter` con `HttpMessagesClient`.
  - Altrimenti → warning + `StubAdapter` (fallback; `cargo test` continua a funzionare senza chiave).
  - `LARE_AI_MODEL` (opzionale, default `"claude-sonnet-4-6"`) — override modello.

- **`ws.rs`**: `ServerInfo.ai_provider` non è più hard-coded `"stub"` ma riflette `ai.provider()`.

- **Dipendenza nuova**: `reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }` (nessun OpenSSL di sistema — coerente con ADR-009).

### Test coverage (TDD)

- 4 test unitari in `messages_client`: forma richiesta, serializzazione wire, concatenazione blocchi text, display errori.
- 1 test di integrazione reale `#[ignore]` (`real_messages_api_returns_text`) — richiede `ANTHROPIC_API_KEY`; eseguito dal supervisore per l'e2e.
- 5 test unitari in `ai_adapter`: happy path, error path, refusal, max_tokens, dyn compatibility + `provider()`.

**RED → GREEN (Task 3):** i test di `ClaudeAdapter` scritti prima dell'impl producevano 5 errori di compilazione `cannot find type ClaudeAdapter`; tutti verdi dopo l'implementazione.

---

## [0.4.0] — 2026-06-20

### Added — Superficie 3: `/show` slash command (ADR-013)

- **`handle_slash` ramo `show`:**
  - Pattern: `/show <markdown>` → `vec![ServerMsg::OpenWindow{ title, kind: WindowKind::Markdown, content }]`.
  - `content` = everything after `/show ` (whitespace-trimmed, internal newlines preserved).
  - `title` = first non-empty line of `content` after `str::trim`, truncated to ≤60 **chars**
    (uses `chars().take(60)`, not byte slice — safe for UTF-8 / Italian accents).
    Fallback: `"Lare — Output"` if all lines are empty.
  - Empty `/show` (content empty or whitespace-only after trim): `Error{RoutingError,
    "nessun contenuto da mostrare: usa /show <markdown>"}`.
    Rationale: an empty `/show` is a usage error, not a valid empty window.
  - **No `Chunk`/`Done`** emitted: `OpenWindow` is self-contained.
    The WS layer (`ws.rs`) already sends any `ServerMsg` in the returned `Vec` —
    no changes required to the transport layer.

- Import: `use protocol::WindowKind` added to `core.rs`.

- Module doc comment updated to v0.4.0 with the new flow diagram entry for `/show`.

### Design decisions documented

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Empty `/show` | `Error{RoutingError}` | Usage error; an empty window serves no purpose |
| `/show` response | `OpenWindow` only (no `Done`) | Message is self-contained; no streaming correlation needed |
| Title truncation | `chars().take(60)` | Byte-slicing UTF-8 at offset 60 can panic on multi-byte chars |

### TDD cycle (RED → GREEN)

**RED (genuine test failures before implementation):**
Four tests asserting `OpenWindow` received `Error{RoutingError, "comando slash sconosciuto: /show"}`:
```
thread 'core::tests::slash_show_markdown_produces_open_window' panicked:
expected OpenWindow{Markdown}, got [Error { id: "show-1", code: RoutingError, message: "comando slash sconosciuto: /show" }]
(3 more failures for title/truncation/empty-first-line tests)
```

**GREEN** after adding `"show"` arm to `handle_slash`.

New tests (all 56 pass, 6 new):

| Test | Key assertion |
|------|---------------|
| `slash_show_markdown_produces_open_window` | `[OpenWindow{Markdown}]`; content verbatim |
| `slash_show_title_from_first_line` | title contains first-line text |
| `slash_show_long_first_line_truncated_to_60_chars` | title ≤ 60 chars |
| `slash_show_default_title_when_content_empty` | `Error{RoutingError}` |
| `slash_show_whitespace_only_content_is_error` | `Error{RoutingError}` |
| `slash_show_default_title_when_first_line_empty` | title non-empty (fallback or body line) |

---

## [0.3.0] — 2026-06-20

### Added — Superficie 2: slash routing + `/open` + `/reset` (ADR-012)

- **`Route::Slash`** added to `router::Route` enum.
  `classify()` now checks for `/`-prefix (after trim) **before** any
  `command_type` matching — highest priority, mirrors `$` for OS commands.

- **`ToolClient::open_target(target: &str) -> OpenResult`** added to trait:
  - `FakeToolClient`: returns `ok=true` for most targets; `ok=false` when
    the target contains `"nonexistent"` (enables both branches in unit tests).
  - `McpToolClient`: calls `open_target` tool on `mcp-server`, parses
    `{ ok, message }` JSON (mirrors `run_in_session` pattern).
  - `OpenResult { ok: bool, message: String }` mirrors `mcp-server`'s
    `open_target::OpenResult` for the JSON round-trip.

- **`core::handle_slash`** new async function:
  - Parses the slash command name from the input (after stripping `/` + trim).
  - `open <target>` → `tools.open_target(target)` →
    `Chunk(message) + Done{exit_code: Some(0/1)}`.
  - `reset` → `tools.reset_session()` → `Chunk("sessione riavviata") + Done{Some(0)}`.
  - Unknown → `Error{code: RoutingError, message: "comando slash sconosciuto: /<cmd>"}`.

- **Error convention documented (v0.3.0):**
  - `Done{exit_code: Some(1)}` for "dispatched but target invalid" (`/open` not found).
  - `Error{RoutingError}` for "could not dispatch" (unknown slash command).
  This matches the existing Os branch convention (core.rs lines 34-42).

### TDD cycle (RED → GREEN) — router + core

**RED (genuine compile-time failure):**
`Route::Slash` was added to the enum and the slash tests were written *before*
adding the slash arm to the `match` in `core::handle_command`.  Running
`cargo test -p orchestrator router` produced:

```
error[E0004]: non-exhaustive patterns: `Route::Slash` not covered
  --> crates\orchestrator\src\core.rs:78:11
   |
78 |     match route {
   |           ^^^^^ pattern `Route::Slash` not covered
```

Tests could not run (stronger than assertion failure).

**GREEN** after adding `handle_slash` and the `router::Route::Slash` match arm.

New tests (all GREEN):

| Module | Test | Key assertion |
|--------|------|---------------|
| `router` | `slash_open_routes_to_slash` | `/open …` → `Slash` |
| `router` | `slash_reset_routes_to_slash` | `/reset` → `Slash` |
| `router` | `slash_unknown_routes_to_slash` | `/sconosciuto` → `Slash` |
| `router` | `slash_with_leading_whitespace_routes_to_slash` | `  /reset` → `Slash` |
| `router` | `slash_overrides_explicit_os_and_nl` | `/open x` with Os/Nl → `Slash` |
| `router` | `non_slash_inputs_unaffected_by_slash_rule` | `dir`, `ls`, NL, `$` unaffected |
| `core` | `slash_open_found_produces_chunk_and_done_zero` | `ok=true` → exit 0 |
| `core` | `slash_open_not_found_produces_chunk_and_done_one` | `ok=false` → exit 1 |
| `core` | `slash_reset_produces_chunk_and_done_zero` | "sessione riavviata" + Done{0} |
| `core` | `slash_unknown_produces_routing_error` | `Error{RoutingError}` |
| `core` | `slash_config_produces_routing_error_from_backend` | unknown at backend → Error |
| `core` | `slash_with_whitespace_prefix_routes_to_slash` | `  /reset` → Chunk+Done |
| `tool_client` | `fake_open_target_success_returns_ok_true` | ok=true for normal target |
| `tool_client` | `fake_open_target_nonexistent_returns_ok_false` | ok=false for "nonexistent" |
| `tool_client` | `fake_open_target_is_dyn_compatible` | dyn dispatch works |

### Notes

- `/reset` via this backend slash route **closes the TODO** in `tool_client.rs`
  that noted "UI → orchestrator wiring is deferred to a follow-up task" (ADR-010).
- The `TODO` comment has been removed from `ToolClient::reset_session`.

---

## [0.2.0] — 2026-06-20

### Changed

- **`ToolClient` trait renamed method: `run_os_command` → `run_in_session` (ADR-011).**
  - Signature: `async fn run_in_session(&self, command: &str) -> CommandResult`.
  - The `cwd: Option<&str>` parameter is **removed**: `cwd` is now session state
    managed by the shell itself (the user runs `cd` inside the session).
  - `FakeToolClient` updated accordingly (4 unit tests updated).
  - `McpToolClient` updated to call `run_in_session` tool on `mcp-server` v0.2.0.

- **`McpToolClient` is now a persistent connection (critical for ADR-011):**
  - Previous behavior: spawn a new `mcp-server` process per call → session died
    on every command, making `cd`/env persistence impossible.
  - New behavior: `mcp-server` spawned once on the first `run_in_session` call;
    the `rmcp::Peer` handle stored in `Arc<Mutex<Option<Peer>>>` and reused.
  - The `RunningService` is kept alive via a background `tokio::spawn` task
    that holds it until the orchestrator process exits.
  - On tool call failure: the peer is cleared and the next call reconnects.
  - End-to-end persistence test added: `mcp_tool_client_run_in_session` (`#[ignore]`)
    verifies `cd <tmpdir>` → `pwd` → output contains tmpdir (proof of session continuity).

- **`core::handle_command`:** now calls `tools.run_in_session(command)` instead of
  `tools.run_os_command(command, cwd)`.  The `cwd` parameter is accepted in the
  public signature (for WS-layer compatibility) but is no longer forwarded.
  All 14 core unit tests updated and passing.

- **`core::tests::cwd_is_forwarded_to_tools`** renamed to
  `cwd_param_is_accepted_without_panic` to reflect that cwd is session state,
  not a per-call forwarded parameter.

### Added

- `tempfile = "3"` dev-dependency (for the ignored e2e persistence test).

## [0.1.0] — 2026-06-20

### Added

- **`router` module** — pure `classify(input, CommandKind) -> Route` function.
  Routes `Os`/`Nl` hints directly; applies `$`-prefix + shell-token heuristic
  in `Auto` mode.  34 unit tests cover explicit kinds, prefix detection,
  Windows/Unix/dev-tool tokens, case-insensitivity, and NL sentences.

- **`AiAdapter` trait** — async, dyn-safe (via `async-trait`), returns `String`.
  Fase 1 impl: `StubAdapter` — echoes `"[stub AI] ricevuto: {input}"`.
  3 unit tests: expected prefix, empty input, dyn compatibility.

- **`ToolClient` trait** — async, dyn-safe, `run_os_command(command, cwd) -> CommandResult`.
  Two impls:
  - `FakeToolClient` — in-memory, deterministic, no processes.
    Constructors: `success(stdout)`, `failure(stderr, code)`, `with_output(...)`.
  - `McpToolClient` — real rmcp 1.7.0 client; spawns `mcp-server` as child
    process via `TokioChildProcess`, calls `run_os_command` tool, parses JSON.
    Binary path: `LARE_MCP_SERVER` env var → sibling of current exe.
  4 unit tests on `FakeToolClient`.
  1 `#[ignore]`'d integration test (`mcp_tool_client_echo`) that exercises the full rmcp path end-to-end (passes when `LARE_MCP_SERVER` points to the built binary).

- **`core::handle_command`** — transport-agnostic nucleus.
  `Os` branch: strips `$`-prefix, calls `tools.run_os_command`, emits
  `Chunk{stdout}` (if non-empty) + `Chunk{stderr}` (if non-empty) + `Done{exit_code}`.
  `Nl` branch: calls `ai.complete`, emits `Chunk{reply}` + `Done{None}`.
  Fully tested with `FakeToolClient` + `StubAdapter`: 14 unit tests covering
  stdout-only, stderr-only, both, no-output, failure, `$`-prefix routing, NL
  routing, `cwd` forwarding, and `strip_dollar_prefix` corner cases.

- **`ws` module** — WebSocket server on `127.0.0.1:7331` (localhost only).
  Handshake: first message must be `Hello{token}`; wrong token → close immediately.
  Correct token → `ServerInfo`. Then dispatches `Command` to core, echoes `Ping`→`Pong`.

- **`main.rs`** — daemon entry point.
  Token: `LARE_TOKEN` env var or random 32-char alphanumeric logged to stderr.
  Fase 1 wiring: `StubAdapter` + `McpToolClient`.

- **Integration tests** (`tests/ws_integration.rs`) — 4 tests using `FakeToolClient`:
  correct token → `ServerInfo` + `Chunk` + `Done`;
  wrong token → connection closed;
  `Ping` → `Pong`;
  NL command → stub `Chunk` + `Done{None}`.

- **Workspace**: `orchestrator` added to `Cargo.toml` members.

- `CHANGELOG.md` and `IMPLEMENTATION.md`.
