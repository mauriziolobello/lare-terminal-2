# Implementation — orchestrator v2.6.0

## Quinta e sesta lingua: tedesco (de) e francese (fr) per AI e /help (v2.6.0)

Stesso schema di "Terza lingua: spagnolo" sotto, applicato a due lingue in un solo passaggio:
1. **Direttiva AI (`crates/orchestrator/src/agent.rs`)**:
   - Costanti `RESPOND_GERMAN`/`RESPOND_FRENCH` aggiunte accanto a `RESPOND_ITALIAN`/`_ENGLISH`/`_SPANISH`.
   - `system_prompt` estende il `match` su `opts.lang.as_deref()` con `Some("de")`/`Some("fr")`.
2. **Finestra /help (`crates/orchestrator/src/core.rs`)**:
   - Costanti `HELP_TITLE_GERMAN`/`HELP_TITLE_FRENCH`; ramo `"help"` esteso con i due codici,
     invariato il fallback italiano per qualunque altro codice.
3. **Contenuti**: `Configuration/i18n/{de,fr}.json` (dizionario UI, 212 chiavi) e
   `Configuration/help/{de,fr}.md` (corpo di `/help`) — nessuna modifica di codice richiesta per
   questi, la catena di fallback in `help.rs` e il caricamento dinamico in `i18n.rs`/`i18n-parity.test.mjs`
   sono già generici rispetto al set di lingue.
4. **UI**: `crates/ui/frontend/config-dialog.js` — due nuove voci nel dropdown lingua ("Deutsch",
   "Français"), vedi `crates/ui/IMPLEMENTATION.md`.

## Terza lingua: spagnolo (es) per AI e /help (v2.2.6)

Nel piano `Docs/i18n/ita/compiti-ai-esterne/2026-09-09-i18n-help-esterno-e-spagnolo.md` (Parte B):
1. **Direttiva AI (`crates/orchestrator/src/agent.rs`)**:
   - Aggiunta costante `RESPOND_SPANISH = " Responde en español, de forma concisa."`.
   - `system_prompt` seleziona `RESPOND_SPANISH` quando `opts.lang.as_deref() == Some("es")`.
   - `system_prompt_override` preservato intatto per i canali specializzati.
2. **Finestra /help (`crates/orchestrator/src/core.rs`)**:
   - Costante `HELP_TITLE_SPANISH = "Lare \u{2014} Comandos"`.
   - Ramo `"help"` mappa `Some("es") => (HELP_TITLE_SPANISH, "es")`, delegando a `help::load_help_body`
     il caricamento di `Configuration/help/es.md`.

## Refactor: migrazione /help su file esterni (v2.2.5)

Nel piano `Docs/i18n/ita/compiti-ai-esterne/2026-09-09-i18n-help-esterno-e-spagnolo.md` (Parte A),
il corpo Markdown di `/help` è stato migrato da costanti statiche Rust a file esterni dedicati sotto
`<config_dir>/help/<lang>.md`. Questo allinea l'architettura dei testi di help a quella già adottata
per i dizionari UI (`<config_dir>/i18n/<lang>.json`), prevenendo la crescita incontrollata di `core.rs`.

### Componenti e flusso:
1. **Modulo `help` (`crates/orchestrator/src/help.rs`)**:
   - Fornisce `help_dir_path(config_dir: &Path) -> PathBuf` per comporre `<config_dir>/help`.
   - Fornisce `load_help_body(help_dir: &Path, lang: &str) -> String` che carica `<lang>.md` con
     catena di fallback: file della lingua richiesta -> `it.md` -> stringa di sicurezza minima
     (`"Aiuto non disponibile: file mancante."`).
2. **File Markdown esterni**:
   - `Test Run/Configuration/help/it.md`: contenuto italiano originale di `HELP_MARKDOWN_IT`.
   - `Test Run/Configuration/help/en.md`: contenuto inglese originale di `HELP_MARKDOWN_EN`.
3. **Propagazione di `config_dir`**:
   - Esteso `handle_slash` e `handle_command` con `config_dir: &Path`.
   - Aggiornati i chiamanti `ws.rs`, `shell_turn.rs` e `telegram::channel`.
   - I titoli della finestra restano costanti compatte in `core.rs` (`HELP_TITLE_IT` ed `HELP_TITLE_EN`).

## Fix: /help rispetta la lingua selezionata (v2.2.4)

Risolto il difetto riscontrato dal vivo in cui `/help` rimaneva in italiano con l'interfaccia in inglese (`Docs/i18n/ita/compiti-ai-esterne/2026-09-09-i18n-fix-help.md`).
Il testo dell'help è stato suddiviso nelle costanti statiche `HELP_MARKDOWN_IT` e `HELP_MARKDOWN_EN` in `crates/orchestrator/src/core.rs`.
La funzione `handle_slash` riceve ora il parametro `lang: Option<&str>`, inoltrato da `handle_command` (`lang.as_deref()`).
Nel ramo `"help"`, il titolo ("Lare — Commands" / "Lare — Comandi") e il markdown visualizzato vengono selezionati con lo stesso pattern di `agent::system_prompt`: `Some("en") => EN, _ => IT`.

## Lingua AI e cablaggio direttiva lang (v2.2.3, i18n Parte 3)

Nel piano `Docs/i18n/ita/compiti-ai-esterne/2026-09-08-i18n-programma.md` (Parte 3), l'orchestratore
adatta la lingua di risposta dell'assistente AI in base alla preferenza di sistema (`config.json.language`)
o al valore `lang` trasmesso dal client nel messaggio `ClientMsg::Command { lang, .. }` (protocollo v2.1.1).

### Architettura e invarianti

1. **`agent.rs` — Nessuna traduzione dell'intero `SYSTEM_PROMPT`**:
   Il prompt principale (`BASE_SYSTEM_PROMPT`) descrive le capacità del sistema e le convenzioni operative,
   e rimane inalterato per evitare costi di manutenzione e divergenze tra lingue.
   È stata isolata la frase finale direttiva in:
   - `RESPOND_ITALIAN = " Rispondi in italiano, in modo conciso."`
   - `RESPOND_ENGLISH = " Answer in English, concisely."`
   La funzione `system_prompt(opts: &TurnOptions)` valuta `opts.lang`: se `opts.lang.as_deref() == Some("en")`,
   appende `RESPOND_ENGLISH`, altrimenti appende `RESPOND_ITALIAN` (default conservativo per `Some("it")`,
   `None` o stringa vuota).

2. **Integrità di `system_prompt_override`**:
   I canali che utilizzano prompt dedicati (`Telegram`, `mcp-nmap`, `AI Chat`) specificano
   `TurnOptions { system_prompt_override: Some(...), .. }`. In tali canali, `system_prompt` restituisce
   l'override inalterato, garantendo che nessuna direttiva spuria venga iniettata in prompt già specializzati.

3. **Propagazione e fallback**:
   - `crates/orchestrator/src/shell_slash.rs`: funzione `read_language(&config_dir) -> Option<String>`
     estrae la preferenza `language` dal file `config.json` se presente e non vuoto.
   - `crates/orchestrator/src/shell_turn.rs`: in `run_shell_command` e `run_ai_turn`, il parametro `lang: String`
     ricevuto da `ClientMsg::Command` viene valutato: se non vuoto è usato direttamente come `Some(lang)`,
     altrimenti viene effettuato il fallback su `read_language(&deps.rt.config_dir)`.
   - `crates/orchestrator/src/ws.rs`: decodifica il campo `lang` da `ClientMsg::Command` e lo instrada
     a `run_shell_command` o a `handle_command` (con fallback su `read_language(&config_dir)`).

## Sink plugin aggiornabile alla riconnessione della UI (v2.2.2)

`PluginHost::server_tx` è uno slot `tokio::sync::watch::Sender<Option<UnboundedSender<ServerMsg>>>`.
I pump clonano lo slot condiviso; per ogni messaggio consultano `borrow()` e inviano sul
sender corrente, senza mantenere il borrow attraverso un await. `set_server_tx` usa
`send_replace`, che sostituisce il valore anche senza receiver watch: non occorre attendere
notifiche perché la lettura avviene all'arrivo dei messaggi plugin. Prima di una UI il valore
è None. Nessun buffering/replay: la modifica rende raggiungibili i plugin già vivi dopo
chiusura e riapertura di ui.exe. Prima ogni pump tratteneva il sender della prima connessione.

## Copertura e2e crypto (2026-09-08)

`tests/plugin_crypto_e2e.rs` usa manifest sorgente e binario compilato in una `TempDir`,
con `discover` → `PluginHost::start` → attivazione lazy e transport stdio reale.
Un decoratore del reader verifica `Ready{name:"crypto", protocol_version:1}` e inoltra
lo stesso messaggio all'host. Il canale server viene collegato prima dell'attivazione.
Le asserzioni verificano i valori nelle due textarea: Cesare XYZ→ABC, DEF→ABC;
la dialog cambia shift da 3 a 1 e Applica aggiorna principale (ABC→BCD) e dialog.
Gli eventi usano l'id ricevuto in ShowWindow, esercitando la registrazione delle finestre
secondarie nel pump. Dopo CloseWindow, la principale continua a decifrare con shift=1.
Ogni ricezione ha timeout di 2 secondi; chiusura finale con `shutdown`.
La parte finale simula la riconnessione: elimina la prima ricezione, registra un nuovo sink,
riattiva lo stesso plugin e verifica apertura e cifratura con il parametro conservato.
Questo scenario falliva per timeout prima del fix v2.2.2, come osservato dal vivo.

Esecuzione: `cargo build -p plugin-crypto`, poi
`cargo test -p orchestrator --test plugin_crypto_e2e -- --ignored`.
Il test non sostituisce la verifica grafica attraverso il deploy reale.

## Autostart di `ui.exe`: `RuntimeConfig::ui_exe()`, `ensure_ui_sink` (v2.2.0)

Piano `Docs/i18n/ita/superpowers/plans/2026-09-07-piano-3-finestra-terminale.md` Task 6, spec
§6.4/§9 (emendato nello stesso piano, Task 7), ADR-020. Chiude il lato mancante dell'autostart
reciproco: fino a questo task solo `ui.exe`/`lare-shell.exe` sapevano avviare l'orchestratore
(self-heal, piano 2b/3), mai il contrario.

`RuntimeConfig::ui_exe()` (`runtime_config.rs`): percorso di `ui.exe`, sempre
`startup_config::deploy_root(&self.config_dir).join("ui.exe")` — a differenza di
`mcp_server_exe()`/`mcp_nmap_exe()`, che leggono `paths.*` da `startup.json`, `ui.exe` vive sempre
alla radice del deploy, come `orchestrator.exe` stesso, mai spostabile.

`ensure_ui_sink` (`shell_turn.rs`, privata): prima verifica il sink `ui` già nel `Registry`
(`registry.lock().await.ui_sink()`) — se presente, ritorna subito, nessuna attesa. Se assente e
`autostart.ui` (da `startup.json`) è disattivato, ritorna `None` senza tentare nulla. Altrimenti,
se `ui.exe` esiste sul filesystem (funzione iniettata, non `Path::exists()` diretto — seam di
test), lo avvia (`startup_config::spawn_detached(&ui_exe, &["--config-dir", dir, "--no-terminal"])`
— sempre con `--no-terminal`, ADR-020: l'orchestratore avvia `ui.exe` per il solo ruolo host, mai
per aprire una finestra terminale interattiva) e ritenta ogni `UI_AUTOSTART_RETRY` (250ms) fino a
`UI_AUTOSTART_WINDOW` (10s). `start_turn` lo chiama al posto della lettura diretta del sink prima
di aprire un turno con finestra. Se il sink non compare comunque entro la finestra, il turno
prosegue con `NO_UI_ACK` (`surface.rs`) — nessun `Error` dedicato al mittente, stesso
comportamento consolidato dal piano 2a/2b quando `ui.exe` non è mai connesso.

**Stesso schema di firma di `launcher::ensure_orchestrator`** (`ui`, Task 3 dello stesso piano):
funzioni di controllo/spawn iniettate (`exists`/`spawn`), non chiamate dirette al filesystem/
`Command` — due implementazioni indipendenti dello stesso pattern in due crate diversi, non
codice condiviso. Testabile senza un `ui.exe` reale: i 4 test (`ensure_ui_sink_*`) passano
funzioni fake per `exists`/`spawn`; nessuna `config_dir` di test ha mai un `ui.exe` reale alla
radice del deploy risolta, quindi la suite non rallenta di 10s per caso.

## Canale shell: registro, gate, superficie, `/ping` (v2.1.0)

Piano `Docs/i18n/ita/superpowers/plans/2026-09-05-piano-2a-protocollo-shell.md` (Task 2-8, 10),
spec `Docs/i18n/ita/superpowers/specs/2026-09-04-lare-terminal-2-design.md` §3/§4, ADR-018. Una
sessione `lare-shell` (host C# del piano 2b — qui ancora impersonata dal client di sviluppo
`scripts/dev/shell-client.mjs`) si connette con `Hello{role:"shell", session_id, cwd, version}` e
manda `Command`/righe `/…` esattamente come farebbe una `ui`; l'orchestratore instrada l'output
verso la finestra Markdown di `ui.exe` invece che nel terminale. Sette moduli nuovi/estesi, in
ordine di dipendenza:

### `connections.rs` — registro delle connessioni vive (Task 2)

`Registry` (dietro `SharedRegistry = Arc<Mutex<Registry>>`, `Registry::shared()`) è l'UNICO posto
che sa "chi è connesso": il **sink `ui`** (la connessione `role: Ui` senza canale, l'ultima vince
— stessa regola di `PluginHost::set_server_tx`; `clear_ui_sink_if(&tx)` lo azzera solo se `tx` è
ANCORA il sink corrente, via `same_channel`, così una `ui` più recente non viene scalzata dalla
disconnessione di una vecchia), le **sessioni shell** per `session_id` (mappa verso il piano 3,
push di segnalini), e i **ping `ui` pendenti** (`register_ui_ping(id) -> oneshot::Receiver<String>`,
risolto da `resolve_ui_ping(id, version)` quando arriva `UiPong`; id ignoto/già risolto → `false`,
stesso principio di `PendingConfirms::resolve`). Puro stato + `tokio::sync`, nessun I/O: testabile
senza socket.

### `local_confirm::ShellConfirmer` — gate di conferma per la shell (Task 3)

Composizione sopra `LocalUiConfirmer` (non ereditarietà: uno struct con un campo `inner`), stessa
meccanica (`ToolConfirmRequest` sulla connessione + `PendingConfirms` + timeout che nega), politica
diversa: `should_gate`/`confirm_routine_save` restano il **default del trait** — quindi TUTTO tranne
`show_markdown` è gateizzato, `run_in_session`/`open_target` inclusi (spec §4.3/§8; la UI locale li
lascia invece autonomi). `ShellConfirmer::new(out_tx, pending, timeout, command_id)`. Ripple in
`agent::display_invocation`: `run_in_session` con `interactive: true` nell'input aggiunge la nota
`(interattivo)` al comando mostrato nel prompt `[Y/n]` (spec §4.5) — l'utente deve sapere PRIMA di
accettare che l'output non sarà catturato.

### `shell_session.rs` — la shell dell'utente come `ToolClient` (Task 4)

Due oggetti a vita diversa: **`ShellSessionState`** (per **connessione**: `session_id`, cwd di
sessione dietro `Mutex` — mai il `cwd_state` globale v1, D17 — mappa `exec_id → oneshot<ExecReply>`
delle esecuzioni pendenti, canale d'uscita, `McpToolClient` condiviso per i tool non-shell) con
`{new, session_id, cwd, set_cwd, exec, resolve_exec, abort_turn, abort_all}`; **`ShellSessionToolClient`**
(per **turno**, via `ShellSessionToolClient::for_turn(&state, turn_id)`: conosce il `turn_id` da
scrivere in ogni `ExecInShell`, implementa `ToolClient`, è ciò che `core::handle_command` riceve
come `tools`). `run_in_session` diventa un round-trip `ServerMsg::ExecInShell{turn_id, exec_id,
command, capture}` → la host esegue → `ClientMsg::ExecResult{exec_id, exit_code, output, cwd}`,
correlato da un `oneshot` per `exec_id` (`exec()` lo registra e attende, `resolve_exec()` lo
risolve dal reader loop di `ws.rs`). `abort_turn(turn_id)` sblocca (con esito "annullato") solo le
esecuzioni pendenti di QUEL turno — usato da `CancelCommand` e da Ctrl+C nella host (spec §4.4, la
pipeline si è già fermata lì, nessun `ExecResult` arriverà mai); `abort_all()` (disconnessione)
sblocca tutto.

### `surface.rs` — router di superficie per un turno shell (Task 5, fix `5f863d7`)

`route_shell_turn(rx, shell, ui, turn: ShellTurn)` consuma da un canale interno i `ServerMsg` che
`core::handle_command`/il built-in `/ping` producono per UN turno e li smista: i `Chunk` (testo AI
+ trasparenza dei tool) si accumulano in un buffer in memoria e vengono consegnati **una sola
volta**, a `Done`/`Error`, come `ServerMsg::OutputWindowContent` verso `ui` (`ShellTurn.output_window
= false` per `/help`/`/show` — `core::WINDOW_SLASHES` — niente finestra col segnaposto, altrimenti
se ne aprirebbero due); tutto il resto del traffico del turno segue `ServerMsg::surface()`: `Origin`
→ la connessione shell (gate, `ExecInShell`, heartbeat), `Ui` → il sink registrato. **Una sola
terminazione per turno** verso la shell: un secondo `Done`/`Error` per lo stesso `turn_id` dopo che
il primo è già stato inoltrato viene scartato con un log `debug` (guardia `terminal_sent`, separata
da `flushed` che governa solo il lato `ui`) — è il contratto che la host del piano 2b potrà
assumere ("il turno finisce al primo terminale"). Senza `ui.exe` connesso (`ui: None`) i messaggi
per `ui` vengono scartati con un log `warn` e la shell riceve `NO_UI_ACK`
(`"→ ui.exe non connesso: output non mostrato"`) al posto della riga di conferma — il turno
completa comunque, l'autostart di `ui.exe` è del piano 3. `output_window_title(input)` deriva il
titolo dal testo fra virgolette di `/ai`/`/ ` (max 60 caratteri) o da `Lare — /<comando>`.

### `shell_slash.rs` — pre-router degli slash della shell (Task 6) + `/help` 2.0

`classify_shell_input(input, is_known_backend)` è pura classificazione (nessun I/O, nessun
`await`): decide, per OGNI riga `/…` che la host manda (non ha un elenco proprio), se è `Nl(testo)`
(`/ai "…"`/`/ "…"`, virgolette obbligatorie — D8, altrimenti `SyntaxError(AI_SYNTAX_ERROR)`),
`OpenUiLocal(nome)` (`config`/`library`/`aichat` — `UI_LOCAL_SLASHES` — o l'id di un canale esterno
user-facing risolto da `shell_channel_table()`, derivata dal registro reale `EXTERNAL_TOOL_CHANNELS`
filtrato su `/nmap`/`/pyping`/`/markets`), `Reset`, `Ping`, `Backend` (predicato `is_known_backend`
iniettato dal chiamante — costruito da `core::KNOWN_BACKEND_SLASHES`, un'unica fonte), o
`Discard(nome)` (slash ignoto: `Done` muto + log `info`, `/find`/`/nowin` inclusi in questa
versione — debito, vedi `HANDOFF.md`). `read_web_search_enabled(config_dir)` legge
`config.json.web_search_enabled` (il file di `ui`): la shell non ha una propria casella, quindi
vale la scelta fatta in `/config`. Ripple in `core.rs`: `KNOWN_BACKEND_SLASHES = ["open", "web",
"show", "help"]` (l'unica fonte di "questo slash esiste" per il pre-router — allineamento con
`handle_slash` è responsabilità di revisione, provata solo in una direzione dal test
`known_backend_slashes_are_all_dispatched_by_handle_slash`) e `WINDOW_SLASHES = ["help", "show"]`
(sottoinsieme il cui esito È già una finestra, consumato da `surface.rs` sopra). `HELP_MARKDOWN`
riscritto per la 2.0: via i tasti/hotkey F2 della v1, dentro `/ping`, `/aichat`, `/calc`, i comandi
realmente supportati dalla shell.

### `ping.rs` — built-in `/ping` (Task 7, spec §3.1)

`run_ping(session_id, shell_version, plugin, ui)` formatta una tabella Markdown con una riga per
strato: `lare-shell` (dalla `Hello`), `orchestrator` (versione del crate + uptime da
`crate::PROCESS_START`, `LazyLock<Instant>` forzato in `main()` all'avvio), `plugin-ping` e
`ui.exe` arrivano già **risolti** (`Option<Result<(versione, tempo), motivo>>`: `None` = strato
assente, `Some(Err)` = presente ma in errore) — chi chiama (`shell_turn::run_ping_turn`) esegue le
sonde vere con le sue dipendenze, questo modulo SOLO formatta (testabile senza plugin host né
registro). `probe(plugin, storage_dir, make)` (in `plugins/host.rs`) è la sonda usa-e-getta:
spawna un'istanza NUOVA del plugin (mai l'eager già viva, che non va disturbata), misura
Init→Ready, manda `Deinit` best-effort e la lascia morire (`kill_on_drop`); `PluginHost::find(id)`
(nuovo) risolve manifest + storage dir senza toccare `writers`. Verso `ui.exe`:
`ServerMsg::UiPing{id}` sul sink del registro, `UI_PING_TIMEOUT` (2 s) su un `oneshot` da
`register_ui_ping`; timeout o sink assente → riga `ui.exe -- non connesso`. Il comando finisce
sempre con `Done`, qualunque strato sia assente.

### `ws.rs` — cablaggio (Task 8) + `shell_turn.rs` (nuovo)

`HelloInfo { token, channel, role, session_id, cwd, version }` sostituisce la tupla `(token,
channel)` di v1 (troppi campi per restare una tupla leggibile). Una connessione `role: Shell`:
non riceve MAI il sink `ui`/AI Chat (`SetServerTx` resta per `role: Ui`); registra una
`ShellSessionState` (Task 4) con la `cwd` iniziale della `Hello`; ogni `Command` va SPAWNATO a
`shell_turn::run_shell_command` (il loop della connessione resta libero per `CancelCommand`/
`ExecResult` nel frattempo); `ClientMsg::ExecResult` risolve l'`exec_id` pendente sulla sessione;
`ClientMsg::UiPong` risolve il ping pendente nel registro. Alla disconnessione:
`shell.abort_all()` sblocca ogni `run_in_session` in attesa (niente deadlock silenziosi),
`registry.unregister_shell(session_id)`, poi `registry.clear_ui_sink_if(&out_tx)` (no-op se questa
non era `role: Ui`, o se il sink è già stato preso da una connessione più recente). Una
connessione `ui` **non cambia**: stesso `Cwd` iniziale, stesso `SetServerTx`, stesso `UiClosed`
alla disconnessione — tutti gli `if hello.role == Role::Ui` intorno a questi punti sono guardie
additive, non un percorso nuovo.

Nuovo modulo **`shell_turn.rs`**: l'unica cosa che `ws.rs` fa per un `Command` shell è costruire
`ShellTurnDeps` (le collaborazioni esplicite: `ai`, `history`, `pending_confirms`, `registry`,
`plugin_host`, `rt`, `shell`, `out_tx`, `shell_version`) e spawnare `run_shell_command`. Dentro:
1. `shell_slash::classify_shell_input` decide il tipo di riga;
2. le risposte immediate (`SyntaxError`, `Discard`, `Reset`, `OpenUiLocal` — quest'ultima manda
   `OpenUiLocal{name}` al sink `ui` se connesso, altrimenti `NO_UI_ACK` + `Done{1}`) vanno
   direttamente sulla connessione shell, nessun turno aperto;
3. `/ai`/backend/`/ping` aprono un **turno** (`start_turn`: crea `ShellTurn` con
   `output_window = !is_window_slash(input)`, apre subito la finestra tramite
   `surface::route_shell_turn` spawnato su un canale interno) e alimentano quel canale con
   `core::handle_command` (via `run_ai_turn`, che costruisce `ShellSessionToolClient`/
   `ShellConfirmer` per il turno e calcola `web_search = richiesta del turno || config.json.
   web_search_enabled`) o col built-in `run_ping_turn` (Task 7).

### Cosa NON cambia

Le connessioni **`ui`** e **Telegram** hanno lo stesso comportamento di v1, non lo stesso codice
bit-per-bit: stesso `Cwd` iniziale, stesso `SetServerTx`/`UiClosed`, stesso dispatch v1 di
`Command` — ma `ws.rs` ha guadagnato guardie esplicite (`if hello.role == Role::Ui`) attorno a
questi punti, non un percorso nuovo. `orchestrator::telegram::channel.rs` non ha ricevuto nessun
cambio di **comportamento**: ha ricevuto solo i bracci no-op richiesti dal `match` esaustivo
(Task 1, `44e025a` — `ExecInShell`, `OpenOutputWindow`, `OutputWindowContent`, `OpenUiLocal`,
`UiPing`, `ActivityIndicator`), perché Telegram non ha una shell propria né finestre — stesso
trattamento permanente delle altre superfici UI-only già no-op lì (`OpenPluginWindow`,
`RoutineSavePreview`, …), come già documentato in `crates/protocol/IMPLEMENTATION.md`. Nessun
consumatore reale del gate _routine_ dedicato (`RoutineSavePreview`) è cambiato: `ShellConfirmer`
eredita lo stesso comportamento di default (appiattito nel testo `[Y/n]`) della UI locale per
quel caso, come documentato nel Task 3.

## Fix wave della review finale del piano 1 (v2.0.2)

Solo pulizia dopo la review whole-branch, nessun cambio di comportamento visibile:

- **`--config-dir` anche ai plugin** (`spawn_plugin`, `plugins/transport.rs`): la
  regola dello spec 2.0 §6.1 ("ogni binario Lare, senza eccezioni") ora vale anche
  per i sidecar plugin. Verificato: nessun plugin oggi fa parsing di argv, quindi
  l'argomento è inerte — ma la regola vale comunque, non solo quando servirà.
  `spawn_plugin` cambia firma (`bin_path, config_dir`); aggiornati i due chiamanti
  di produzione (`main.rs`, `ws.rs` — quest'ultimo ora clona `Arc<RuntimeConfig>`
  nel task `'static` che spawna i plugin lazy) e i 5 call site nei test e2e
  `#[ignore]` (config_dir fittizio, `tmp.path()`).
- **Assolutizzazione di `--config-dir` spostata in `startup-config`**: il blocco
  locale in `main.rs` che rendeva assoluto un `--config-dir` relativo è stato
  rimosso — lo fa ora `startup_config::config_dir_from_process()` per ogni
  binario (vedi CHANGELOG di quel crate, v2.0.2). L'invariante di
  `RuntimeConfig::config_dir` (SEMPRE assoluto) resta vera; il commento che la
  documenta ora punta al crate condiviso.
- **`ws::LISTEN_ADDR`** (costante morta, sostituita da tempo da `startup.json.
  ws_port`): rimossa; corretti i commenti in `ws.rs`/`lib.rs` che citavano ancora
  la porta fissa 7331.
- **`ws_integration.rs`**: rimosso un `std::env::set_var("LARE_PYTOOLS_DIR", …)`
  residuo e il commento che diceva che `resolve()` lo legge — non lo fa più da
  quando questo crate è passato a `RuntimeConfig` (D6); il test resta verde
  perché `test_rt()` usa già un `config_dir` fittizio la cui `pytools_dir`
  risolta non esiste su disco.
- **`token_store::resolve_token`**: usa `startup_config::TOKEN_FILE_NAME` invece
  della stringa letterale `"token"`.
- **`external_channel::tests::test_rt()`**: prima faceva `tempdir().unwrap().
  keep()`, che rende permanente la tempdir (disattiva la pulizia RAII) — ogni
  chiamata (15 nei test di questo modulo) lasciava una cartella orfana in
  `%TEMP%`. Ora ritorna `(RuntimeConfig, TempDir)`; i 15 chiamanti tengono vivo
  il `TempDir` per la durata del test (`let (rt, _tmp) = test_rt();`), che viene
  ripulito al drop come dovrebbe.

## Configurazione 2.0: RuntimeConfig, figli con --config-dir, log su file (v2.0.1)

Piano `2026-09-05-piano-1-fondamenta`, Task 4. Il crate `startup-config` è stato
riscritto (Task 2) con l'API 2.0 (`config_dir_from_process`, `StartupConfig::load`/
`resolve_path`, `CONFIG_DIR_FLAG`) — l'API v1 (`load_from_dir`/`resolve`/
`default_local_dir`, `Paths::local_dir`) non esiste più, e l'orchestrator (13 letture
di `env::var("LARE_*")` + 3 `LOCALAPPDATA` sparse nel crate) non compilava più.

### `RuntimeConfig` — un "context object" immutabile

Nuovo modulo `runtime_config.rs`:

```rust
pub struct RuntimeConfig {
    pub config_dir: PathBuf,  // SEMPRE assoluto
    pub startup: StartupConfig,
}
impl RuntimeConfig {
    pub fn path(&self, value: &str) -> PathBuf { StartupConfig::resolve_path(&self.config_dir, value) }
    pub fn network_json_path(&self) -> PathBuf { self.config_dir.join("network.json") }
    pub fn plugins_dir(&self) -> PathBuf { self.path(&self.startup.paths.plugins_dir) }
    pub fn pytools_dir(&self) -> PathBuf { self.path(&self.startup.paths.pytools_dir) }
    pub fn mcp_server_exe(&self) -> PathBuf { self.path(&self.startup.paths.mcp_server) }
    pub fn mcp_nmap_exe(&self) -> PathBuf { self.path(&self.startup.paths.mcp_nmap) }
    pub fn log_dir(&self) -> PathBuf { self.path(&self.startup.log.dir) }
}
```

Analogia OOP: come un `ApplicationContext`, costruito una volta in `main()` e
condiviso (`Arc`) da tutto il resto — sostituisce le 5+ copie quasi identiche della
stessa catena di fallback env→file→default che c'erano nella v1 per token,
`llms.json`, `telegramsettings.json`, search, plugin, aichat.

**`config_dir` è SEMPRE assolutizzato** in `main()`, prima di qualunque uso e
PRIMA del `set_current_dir(home)` che segue poco dopo (per dare al cursore un cwd
iniziale sensato): un `--config-dir` relativo letto una SECONDA volta dopo quel
cambio di cwd risolverebbe contro `home`, non contro la cartella di lancio — due
risultati diversi per lo stesso flag. La regola che ne segue, applicata ovunque in
questo task: **nessun punto del crate rilegge `--config-dir`/ricostruisce
`RuntimeConfig` dopo `main()`** — tutti i consumatori (vedi sotto) tengono
`config_dir`/`RuntimeConfig` come campo o parametro ricevuto, mai ri-derivato.

### Figli con `--config-dir` esplicito

- `McpToolClient::resolve(config_dir: &Path, cfg: &StartupConfig) -> Self` (non più
  fallibile — legge solo `startup.json.paths.mcp_server`, mai `LARE_MCP_SERVER`/
  `current_exe()`). Il campo `config_dir` è conservato e passato ad OGNI spawn del
  figlio (`c.arg(CONFIG_DIR_FLAG).arg(&self.config_dir)`) — la v1 duplicava il
  blocco di lazy-connect-e-spawn in **sei** punti diversi di `tool_client.rs`
  (`run_in_session` più cinque dei sette metodi `ToolClient`, ciascuno col proprio
  `if peer_guard.is_none() { ... }`), e tutti e sei dovevano ricevere l'argomento —
  un `grep -n "Command::new(&self.mcp_server_path)"` prima del fix mostrava sei
  righe, non una; il primo fix (solo `run_in_session`) faceva passare il test
  d'integrazione "run_in_session" ma falliva ancora quello "save_routine", proprio
  perché quel metodo spawna dal SUO blocco duplicato, non da quello di
  `run_in_session`.
  **2.2.1**: i sei blocchi duplicati (identici carattere per carattere tranne il
  primo, che aveva un commento in più) sono ora fattorizzati in un solo metodo
  privato, `McpToolClient::mcp_server_command(&self) -> tokio::process::Command` —
  ogni chiamante fa solo `let child_cmd = self.mcp_server_command();`. Lo stesso
  metodo aggiunge `CREATE_NO_WINDOW` (0x0800_0000, solo `#[cfg(windows)]`) e
  redirige `stderr` su file (`startup_config::child_stderr_log_sink`) invece di
  `inherit()` — vedi CHANGELOG 2.2.1 per il perché (console spuria quando
  l'orchestrator è staccato). Stesso trattamento in `nmap_tool_client.rs`
  (`ensure_connected`), `python_mcp_tool_client.rs` (`ensure_connected`) e
  `plugins/transport.rs` (`spawn_plugin`), più `CREATE_NO_WINDOW` sui due
  `taskkill` di `RealProcessTreeKiller`/`RealProcessKiller`.
- `NmapToolClient::resolve(config_dir, cfg)` — mirror esatto, un solo punto di spawn.
- `PythonMcpToolClient::resolve(config_dir, cfg, domain_id, script_relpath,
  tool_specs, call_timeout_secs)` — `pytools_root` da `cfg.paths.pytools_dir`
  (default `"pytools"`) risolto rispetto alla radice del deploy, mai da
  `env_override` (v1: una variabile diversa per dominio, es. `LARE_PYTOOLS_DIR`).
  Lo script riceve `--config-dir` come argomento aggiuntivo dopo il proprio path.
- **`LARE_PLUGINS_DIR` rimossa**: la v1 aveva un caso speciale (NESSUN trim/empty-
  check, per restare "consistente" con `plugins_view.rs` lato UI) prima di ricadere
  su `startup.json`/default. Un solo risolutore ora: `rt.plugins_dir()`.
- **Anche i plugin sidecar ricevono `--config-dir`** (`spawn_plugin`, fix wave
  della review finale, v2.0.2): la regola dello spec 2.0 §6.1 vale per OGNI
  binario Lare senza eccezioni — oggi nessun plugin fa parsing di argv, quindi
  l'argomento è inerte, ma la regola resta valida anche per i sidecar.

### `EXTERNAL_TOOL_CHANNELS` — factory con contesto esplicito

Le tre factory che costruiscono client reali (`nmap`, `python-ping`,
`financial-markets`) cambiano firma da `fn() -> anyhow::Result<Arc<dyn ToolClient>>`
a `fn(&RuntimeConfig, &Arc<dyn ToolClient>) -> anyhow::Result<Arc<dyn ToolClient>>`
(resta un puntatore a funzione semplice, non una closure — il registro è un array
`const`). Il secondo parametro (`default_tools`, il `ToolClient` condiviso del
cursore) non è usato da nessun canale odierno, ma è nella firma per un ipotetico
canale futuro che voglia avvolgerlo/delegargli invece di costruirne uno nuovo.
`resolve_channel_tools` guadagna un quarto parametro `rt: &RuntimeConfig`, e
`ws::serve`/`handle_connection` un `rt: Arc<RuntimeConfig>` — thread fino a lì da
`main()`, mai ri-derivato.

### `memory_file_path` — una sola copia

`ai_adapter.rs` porta ora l'UNICA implementazione: `pub fn memory_file_path
(config_dir: &Path, label_base: &str) -> PathBuf`. `aichat/service.rs` (che aveva
una copia quasi identica, con lo stesso commento "duplicazione deliberata" della v1)
la riusa in `append_memory_note`. `LlmAdapter` guadagna un campo `config_dir`
(passato al costruttore da `main.rs`/`llms_config::build_adapter`), usato da
`chat_reply`/`chat_autoparticipate` (memoria) e da `respond` (`needs_ai_name_prompt_at`
per il nudge one-shot "come ti chiami?").

`agent::dispatch_tool` — il wrapper a 3 parametri che ri-derivava da solo il path di
`network.json` (`aichat::config::resolve_network_json_path`, a sua volta basata
sull'API v1 rimossa) — è stato **eliminato**. `agent::dispatch_tool_at` (4°
parametro: `network_json_path: &Path`) è ora l'unica funzione: i chiamanti reali
(`McpToolClient`/`CwdTrackingToolClient`, che tengono `config_dir` come campo)
passano `self.config_dir.join("network.json")` esplicitamente — stesso principio
del `RuntimeConfig` sopra, applicato a un caso più piccolo.

### Log su file

`main()` inizializza il tracing con un layer su file (`tracing_appender::rolling::
daily`, cartella da `rt.log_dir()` — default `Configuration/logs`, nome file
`orchestrator.log`) SEMPRE attivo, più uno stderr layer SOLO con `--console-log`
(passato da `init_*.ps1` per il debug interattivo). Un orchestrator avviato in
autostart (nessun terminale) non deve bloccarsi/sporcare un handle di console che
non ha. Livello da `startup.json.log.level` (default `"info"`), non più
`RUST_LOG`/`EnvFilter` da env.

### Test — TDD, RED verificato

Nuovi test scritti PRIMA dell'implementazione (RED = errore di compilazione, non
solo assert falliti — le vecchie firme non esistevano più):
`tool_client::tests::mcp_server_path_comes_from_startup_paths`,
`nmap_tool_client::tests::mcp_nmap_path_comes_from_startup_paths`,
`python_mcp_tool_client::tests::python_client_paths_come_from_startup_pytools_dir`
(quest'ultimo richiede `{}`/`.display()` nei messaggi `bail!` di `resolve()` invece
di `{:?}` — il Debug di `PathBuf` su Windows raddoppia i backslash, corrompendo
l'asserzione sul path atteso dopo `.replace('\\', "/")`), più
`runtime_config::tests::*` (4 test sugli accessor derivati). Test di integrazione
con il binario `mcp-server` REALE (non un fake), riscritti per Task 4:
`tool_client::tests::mcp_tool_client_run_in_session` e
`mcp_tool_client_save_routine_then_get_routine_content_round_trips` — quest'ultimo
verifica che il figlio abbia DAVVERO ricevuto `--config-dir` controllando che
`index.json` sia finito dentro la `config_dir` di test (una tempdir chiamata
`Configuration`, cui il `routines_dir` di default risolve), non altrove o assente.

## Fix review finale whole-branch — canale WS "config-market-data-test" non registrato (v0.41.19)

Piano `2026-08-14-financial-markets-ibkr-data-source`, fix wave post-review
(`.superpowers/sdd/2026-08-14-financial-markets-ibkr-data-source/review-fix-wave-brief.md`,
Fix A). Il supervisore ha letto il codice reale (non fidandosi del solo
report `/code-review high`) e confermato: il bottone "Test connessione" del
tab `/config` Dati Mercato (Task 12, sezione sotto) non aveva MAI funzionato
in produzione.

### Causa — handshake WS falliva prima dell'arm già corretto

`config-dialog.js::_testMarketDataConnection` (Task 12) apre una
`LareWsClient` dedicata con `channel: "config-market-data-test"` (pattern
copiato da "library-expand", vedi Task 4 del piano library-expand). Il
pattern però FUNZIONA per "library-expand" solo perché quell'id è
effettivamente registrato in `EXTERNAL_TOOL_CHANNELS` — "config-market-data-test"
non lo era mai stato. `ws.rs::resolve_channel_tools` (righe ~199-216 prima
del fix) rifiuta QUALUNQUE `channel` non trovato nel registro con
`ServerMsg::Error` seguito dalla chiusura del socket — questo accade PRIMA
che l'arm `ClientMsg::TestMarketDataSource` (Task 11, v0.41.18, già corretto
e mai il problema) possa essere raggiunto. Risultato osservabile: ogni click
sul bottone falliva silenziosamente all'handshake, senza mai arrivare al
codice che la review textuale aveva giudicato corretto.

### Fix — nuova voce nel registro, mirror esatto di "library-expand"

```rust
ExternalToolChannel {
    id: "config-market-data-test",
    slash_trigger: "/config-market-data-test", // inerte, nessun innesco da cursore
    window_title: "Lare — Test fonte dati mercato",
    tool_client: || Ok(std::sync::Arc::new(EmptyToolClient) as std::sync::Arc<dyn ToolClient>),
    format_invocation: None,
    system_prompt_override: None,
},
```

`EmptyToolClient` riusato (già in scope, usato da `"library-expand"` poco
sopra nello stesso file) — non duplicato. `TestMarketDataSource` non è un
tool AI: è un `ClientMsg` diretto gestito in `ws.rs` indipendentemente dal
canale, quindi nessun `system_prompt_override` serve qui (a differenza di
`"nmap"`/`"python-ping"`/`"financial-markets"`, che hanno un'AI vera dietro).

### Test — TDD, RED verificato

Tre test nuovi scritti PRIMA della voce nel registro, lanciati e falliti per
il motivo giusto (`EXTERNAL_TOOL_CHANNELS.len()` era ancora 4,
`resolve_channel_tools` ritornava `Err`, l'indice `[4]` era out-of-bounds):

- `production_registry_has_the_config_market_data_test_channel`
- `config_market_data_test_channel_resolves_ok_not_err`
- `config_market_data_test_tool_client_has_no_custom_tools`

I tre assert preesistenti `EXTERNAL_TOOL_CHANNELS.len() == 4` (nei test delle
voci `library-expand`/`python-ping`/`financial-markets`) aggiornati a `5` —
manutenzione necessaria della stessa lunghezza del registro, nessun
comportamento diverso per quelle voci.

## Task 11 (IBKR data source): handler `TestMarketDataSource` + check di avvio (v0.41.18)

Task 11/13 del piano `2026-08-14-financial-markets-ibkr-data-source`. Collega
lato Rust il tool MCP Python `test_market_data_source` (Task 9) e i tipi wire
`ClientMsg::TestMarketDataSource`/`ServerMsg::MarketDataSourceTestResult`
(Task 10, crate `protocol`) — punto di consumo finale, nessuna nuova
interfaccia pubblica del "Contratto A" prodotta qui.

### Nota architetturale — perché uno scoped client, non uno condiviso

Lo spec originale del piano immaginava un client MCP `financial-markets`
condiviso/persistente nello stato dell'orchestrator. Non è così oggi: il
client di quel canale è costruito PER-CONNESSIONE dentro
`external_channel.rs`, solo quando la UI apre la finestra `/markets`. Il
comando qui (innescato da `/config`, un contesto diverso — l'utente non ha
necessariamente aperto `/markets`) usa quindi un `PythonMcpToolClient`
**scoped**: costruito, usato una volta, lasciato cadere. Costo di avvio
Python pagato una volta per click/avvio — accettabile per un'azione manuale
infrequente, nessuno stato nuovo da gestire nel resto dell'orchestrator.

### `ws::test_market_data_source_now() -> (bool, String)`

Funzione `pub` in fondo a `ws.rs` (sezione Helpers), condivisa dai due
chiamanti sotto — evita di duplicare la costruzione del client +
il parsing della risposta JSON in due file:

```rust
pub async fn test_market_data_source_now() -> (bool, String) {
    match PythonMcpToolClient::resolve(
        "financial-markets", "server.py",
        vec![PythonToolSpec { def: ToolDef { name: "test_market_data_source", .. }, report_title: None, defer_report_to_turn_end: false }],
        30, "LARE_PYTOOLS_DIR",
    ) {
        Ok(client) => {
            let outcome = client.dispatch("test_market_data_source", &serde_json::json!({})).await;
            client.shutdown().await; // vedi "Fix orfano di processo" sotto
            /* dispatch + parsing difensivo del JSON {"ok","message"} */
        }
        Err(e) => (false, format!("venv financial-markets non trovato: {e}")),
    }
}
```

Punto importante scoperto durante l'implementazione: `PythonMcpToolClient::
dispatch()` rifiuta con `"tool sconosciuto sul canale"` qualunque nome non
presente nel vettore `tool_specs` passato a `resolve()` — lo spec del task
suggeriva `vec![]` (vuoto), che avrebbe reso l'intero handler un fallimento
garantito. Corretto passando un `PythonToolSpec` minimo per
`test_market_data_source` (nessun `report_title`: questo test non apre mai
una finestra Markdown).

Ritorna sempre `(ok, message)`, mai un panic: venv assente, timeout (30s),
o JSON malformato diventano tutti `(false, <messaggio leggibile>)`.

#### Fix orfano di processo (trovato in review, prima del commit)

La prima versione chiamava `dispatch()` e basta. `PythonMcpToolClient` è
SCOPED (vive solo per questa funzione), ma `dispatch()`/`ensure_connected()`
spawnano comunque un processo Python reale e lo lasciano vivo finché non
arriva uno `shutdown()` esplicito — il proprio doc-comment di
`close_connection` lo dice chiaramente: "senza questo, il processo spawnato
resta orfano indefinitamente" (stessa classe di bug già corretta una volta
per nmap, vedi `nmap-orphan-process-timeout-fix`). Senza il fix, OGNI click
sul pulsante di test da `/config` — e ogni riavvio dell'orchestrator, che fa
lo stesso check all'avvio — avrebbe lasciato un `python.exe` orfano.
`client.shutdown().await` è sicuro anche quando `dispatch` non si è mai
connesso davvero (tool rifiutato prima di `ensure_connected`):
`close_connection_does_not_kill_when_never_connected` in
`python_mcp_tool_client.rs` copre esattamente questo caso no-op. Verificato
dal vivo: nessun `python.exe` residuo dopo una chiamata reale (vedi
"Verifica dal vivo" sotto).

### `ws.rs` — arm `ClientMsg::TestMarketDataSource`

```rust
ClientMsg::TestMarketDataSource { id } => {
    let out = out_tx.clone();
    tokio::spawn(async move {
        let (ok, message) = test_market_data_source_now().await;
        let _ = out.send(ServerMsg::MarketDataSourceTestResult { id, ok, message });
    });
}
```

**Spawnato**, non `.await`-ato inline nel match del read-loop principale
(a differenza dello snippet del brief, che faceva `sink.send(...).await`
inline): il test di rete può richiedere fino al timeout di 30s se
TWS/Gateway non risponde — inline, questo avrebbe bloccato l'intera
connessione WS (niente Ping/Pong, niente CancelCommand) per tutto quel
tempo. Stesso motivo per cui i comandi OS/NL sono già spawnati poco più giù
nello stesso file. `sink` inoltre non è più raggiungibile a quel punto del
codice (di proprietà del task `writer`) — l'invio usa il canale d'uscita
persistente `out_tx`, come ogni altro punto di questo file dopo
l'handshake.

### `main.rs` — check informativo all'avvio

Spawnato subito dopo il blocco che costruisce l'AI adapter (righe ~169-196),
prima della sezione "Tool client":

```rust
tokio::spawn(async move {
    let (ok, message) = ws::test_market_data_source_now().await;
    if ok {
        tracing::info!("Fonte dati mercato: connessa");
    } else {
        tracing::warn!("Fonte dati mercato: non raggiungibile ({message})");
    }
});
```

Non bloccante per lo stesso motivo di sopra — `main()` non aspetta questo
task prima di aprire il listener WS (`ws::serve`). Semplificato rispetto
allo snippet del brief (che distingueva `is_error`/JSON malformato/`ok:
false` con tre rami separati): dato che `test_market_data_source_now()`
unifica già ogni fallimento in `(false, message)`, qui bastano due rami.

### `telegram/channel.rs` — arm no-op aggiuntivo

`format_response` fa un match ESAUSTIVO su `ServerMsg` (nessun `_ =>`
catch-all). Il Task 10 (crate `protocol`, fuori scope) ha aggiunto la
variante `MarketDataSourceTestResult` senza toccare questo file — il
crate non compilava più (`E0004: non-exhaustive patterns`) finché non è
stato aggiunto qui un arm no-op, stesso trattamento di `ToolConfirmRequest`
(superficie UI-only, `/config`, nessun equivalente su Telegram).

### Test — `tests/ws_integration.rs`

`test_market_data_source_echoes_id_and_reports_unreachable_when_venv_missing`
copre la wiring dell'arm (Step 2 del brief) deterministicamente, senza I/O
reale: `LARE_PYTOOLS_DIR` punta a una tempdir vuota (`tempfile::tempdir()`),
quindi `resolve()` fallisce in modo sincrono e prevedibile, nessuno spawn
Python. Verifica: (1) l'`id` della richiesta torna invariato nella risposta
(punto classico di copia-incolla sbagliata); (2) `ok: false` con un messaggio
non vuoto; (3) un `Ping` inviato subito dopo riceve `Pong` — prova che il
read-loop resta reattivo, cioè che l'arm è davvero spawnato e non eseguito
inline (un'esecuzione inline avrebbe bloccato la connessione fino al timeout
di 30s del client scoped).

**Nota sull'ordine TDD**: questo test è stato scritto DOPO il codice
dell'arm (non prima) — la review di questo task ha segnalato che lo Step 2
(wiring del protocollo) è deterministicamente testabile nell'harness
esistente, a differenza dello Step 4 del brief (I/O reale su IB Gateway, che
resta senza test automatico per costruzione). Il ciclo RED→GREEN è comunque
stato eseguito per intero, solo dopo l'implementazione: l'arm è stato
temporaneamente disabilitato (sostituito con un no-op), il test lanciato — si
blocca in attesa di un `ServerMsg` mai inviato, fallimento per il motivo
giusto (RED) — poi l'arm è stato ripristinato e il test è tornato verde
(GREEN), 17/17 test del file passano.

### Deroga SOLID annotata

Il doc-comment di modulo in cima a `ws.rs` dichiara: "This module does NOT
contain routing, AI, or tool logic." `test_market_data_source_now`
contravviene a questo (costruisce un `PythonMcpToolClient`, lo dispaccia).
Deroga consapevole, non silenziosa: lo scope dichiarato del Task 11 è
limitato a `ws.rs` + `main.rs`; un client scoped per-singola-chiamata (non
condiviso altrove nell'orchestrator — vedi nota architetturale sopra) non
ha oggi una casa più naturale senza introdurre un terzo file fuori da quello
scope. Se il test cresce (più fonti dati, retry, cache), andrebbe estratto
in un modulo dedicato (es. `market_data_check.rs`).

### Verifica dal vivo (Step 4 del brief) — cosa è stato verificato davvero

Niente test automatico per questo step (I/O reale), come da brief — il test
in `ws_integration.rs` sopra copre solo la wiring (Step 2), non l'I/O reale.
La verifica end-to-end via `orchestrator.exe` reale (Step 4) non è stata
possibile fino in fondo: la porta `127.0.0.1:7331` era già occupata da
un'istanza orchestrator legittima e attiva in questa sessione (PID
verificato, connessione WS stabilita) — non disturbata. Dettaglio completo,
inclusi i comandi esatti usati, in `task-11-report.md`. In sintesi:

1. **Check di avvio non bloccante (`main.rs`)** — verificato con
   `orchestrator.exe` reale, `LARE_PYTOOLS_DIR` puntata a una cartella vuota:
   il `WARN "Fonte dati mercato: non raggiungibile (...)"` compare nel log
   subito dopo l'avvio, PRIMA degli altri passi di inizializzazione (cwd,
   Telegram, plugin host, AI Chat) che completano tutti normalmente dopo —
   confermato che il check non blocca l'avvio.
2. **Fonte non raggiungibile / venv assente (`ws::test_market_data_source_now`
   diretta)** → `(false, <messaggio leggibile>)`, mai un panic — confermato.
3. **Fonte raggiungibile (IB Gateway attivo su :4001, `ws::test_market_data_
   source_now` diretta, bypassando il bind di `ws::serve`)** → round-trip
   REALE attraverso il vero server MCP (spawn Python + handshake stdio +
   `call_tool` + parsing della risposta JSON) fino a `IbkrDataSource.
   _connect()` (Task 4/8) — confermato che la CATENA Rust è corretta
   end-to-end (nessun errore di wiring, parsing, o protocollo); confermato
   anche nessun `python.exe` residuo dopo la chiamata (fix orfano sopra).
   **Non verificato**: il ramo `INFO "connessa"` (`ok: true`) — vedi sotto.

**Due fatti osservati durante la verifica, ENTRAMBI fuori scope Task 11 (non
corretti qui, riportati al supervisore in `task-11-report.md` con
l'interpretazione — qui solo i fatti):**

- Il venv Python di questo worktree aveva `mcp==2.0.0` invece di `1.28.1`
  (rompeva `mcp.server.fastmcp`) — problema di ambiente pre-esistente, non
  introdotto da questo task. Riparato localmente (`pip install
  mcp==1.28.1`, mai committato — la cartella `venv/` è gitignored) solo per
  poter completare la verifica.
- Con un venv sano, il round-trip arriva davvero a `IbkrDataSource.
  _connect()`, che fallisce con `"This event loop is already running"`
  quando invocato attraverso il vero server MCP async (FastMCP) — impedisce
  di osservare il ramo `ok: true`/`INFO "connessa"` in questa sessione.

## Fix review finale whole-branch — nickname/badge `is_ai` (v0.41.17)

Ultima fix wave prima del merge su `main` del piano
`Docs/superpowers/plans/2026-08-13-aichat-display-names.md` (9 task, 14 commit) — 5
Important + 2 Minor trovati da una review finale whole-branch (interazioni FRA
task, invisibili a ogni review task-scoped precedente). Riassunto (dettaglio nel
report della fix wave, locale/gitignored:
`.superpowers/sdd/2026-08-13-aichat-display-names/final-review-fix-report.md`):

- **`agent.rs` — scrittura di `set_ai_display_name` ora Value-based, mai più un
  round-trip tramite `AiChatConfig`** (Important #1): nuova `merge_ai_display_name`,
  stesso principio di `aichat_settings::merge_aichat_settings` lato `ui`. Sostituisce
  di fatto quanto descritto nella sezione "Arm nuovo `set_ai_display_name`" più giù
  (Task 7, v0.41.15) — quella sezione descriveva il comportamento PRIMA di questo
  fix e resta solo come contesto storico del task originale.
- **`network.json` risolto in UN SOLO modo in tutto il binario** (Important #4):
  nuove `crate::aichat::config::resolve_network_json_path()`/
  `resolve_network_json_path_in` (legge ANCHE `startup.json`, come `main.rs`),
  condivise da `agent::dispatch_tool` e `ai_adapter::needs_ai_name_prompt` — sostituiscono
  i due resolver quasi identici che il Task 7 (v0.41.15) e il Task 8 (v0.41.16) avevano
  introdotto indipendentemente, NESSUNO dei due leggeva `startup.json`. La "Nota
  duplicazione accettata" nella sezione Task 8 più giù descriveva quella duplicazione
  come debito accettato — non più vera dopo questo fix, corretta in-place lì.
- **Messaggio di successo del tool** avvisa esplicitamente che il nickname compare
  in AI Chat solo dopo il riavvio dell'orchestrator (Minor).
- **22 riferimenti a un path doc INESISTENTE** (`Docs/superpowers/sdd/...`, mai
  esistito) corretti in tutto il repo verso `Docs/superpowers/plans/
  2026-08-13-aichat-display-names.md` (contesto task/esecuzione) o
  `Docs/superpowers/specs/2026-08-13-aichat-display-names-design.md` (contesto
  design/motivazione) — Important #5.
- `Docs/HANDOFF.md` aggiornato nello stesso commit di questa fix wave (Important #3 —
  non lo era mai stato in nessuno dei 14 commit precedenti del piano).

Le sezioni sotto (Task 7 v0.41.15, Task 8 v0.41.16) restano per il resto come
scritte a suo tempo — corrette SOLO nei punti resi falsi da questo fix (segnalati
esplicitamente dove accade), non riscritte per intero: descrivono correttamente
il resto del lavoro fatto in quei task.

## `ai_adapter.rs`: nudge one-shot per l'auto-assegnazione del nome AI (v0.41.16)

Ottavo task del piano `Docs/superpowers/plans/2026-08-13-aichat-display-names.md`.
Chiude il cerchio Task 1 (`AiChatConfig::ai_display_name`, campo) → Task 7
(`set_ai_display_name`, il tool che lo scrive) → Task 8 (questo: l'AI SA di
doverlo chiamare).

### `needs_ai_name_prompt()` / `needs_ai_name_prompt_at(path)`

**Aggiornato dal fix review finale (v0.41.17, vedi sezione in cima al file)**:
il corpo originale di questo task risolveva `network.json` con un resolver
PRIVATO di questo file (`network_json_path()`, sotto). Quel resolver è stato
rimosso: `needs_ai_name_prompt()` ora chiama direttamente la funzione
CONDIVISA `crate::aichat::config::resolve_network_json_path()` (stessa usata
da `agent::dispatch_tool`). Forma attuale:

```rust
fn needs_ai_name_prompt() -> bool {
    needs_ai_name_prompt_at(&crate::aichat::config::resolve_network_json_path())
}

fn needs_ai_name_prompt_at(path: &std::path::Path) -> bool {
    let Ok(bytes) = std::fs::read(path) else { return false };
    let Ok(cfg) = serde_json::from_slice::<crate::aichat::config::AiChatConfig>(&bytes) else {
        return false;
    };
    cfg.enabled && cfg.ai_display_name.is_none()
}
```

Stesso pattern DI di `memory_file_path`/`memory_file_path_with_base` (righe
~356-386 dello stesso file): la funzione vera è un thin wrapper attorno alla
versione parametrizzata sul path, che è quella coperta dai test (nessuna
`tempfile`/env nei test — solo path espliciti). `resolve_network_json_path()`
(in `aichat/config.rs`) risolve `LARE_LOCAL_DIR` (env) → campo `local_dir` di
`startup.json` (accanto all'eseguibile) → `startup_config::default_local_dir()`
— stessa precedenza a 3 livelli di `main.rs`, ANCHE `startup.json` (a
differenza della versione originale di questo task, che si fermava a env >
default: vedi il fix Important #4 in cima al file).

`true` significa: AI Chat è attivo (`AiChatConfig::enabled`) MA l'AI non ha
ancora un nome proprio (`ai_display_name.is_none()`). File assente,
illeggibile o non deserializzabile → `false` (fail-safe: nessun nudge se lo
stato è incerto, mai un falso positivo che spammerebbe l'istruzione a ogni
turno per un file corrotto).

**Nessuno stato in memoria**: la funzione legge `network.json` fresco a
OGNI iterazione del loop di `respond()` (non solo al primo turno) — stesso
principio già in uso nella stessa funzione per
`self.backend.supports_web_search()`. Conseguenza pratica: appena il tool
`set_ai_display_name` (Task 7) scrive un nome, questa funzione ricomincia a
restituire `false` dal turno SUCCESSIVO, senza bisogno di alcun canale di
notifica o cache da invalidare.

**Nota "duplicazione accettata" — RISOLTA dal fix v0.41.17, non più vera**:
questa sezione diceva che `agent.rs` aveva un `default_network_json_path()`
quasi identico (Task 7) e che i due resolver non erano condivisi
deliberatamente ("fuori scope", "debito minore accettato"). Il fix Important
#4 della review finale ha mostrato che quel debito non era solo estetico:
sulla combinazione reale "`startup.json{local_dir:...}` impostato SENZA la
env var `LARE_LOCAL_DIR`", i due resolver avrebbero risolto due path
DIVERSI in silenzio — il tool `set_ai_display_name` avrebbe scritto un
`network.json` che questa funzione non avrebbe mai letto. Entrambi i
resolver privati sono stati rimossi, sostituiti dall'unica
`crate::aichat::config::resolve_network_json_path()` condivisa.

### `AI_NAME_REQUEST_ADDENDUM`

Costante di modulo (NON in `agent.rs` accanto a `WEB_SEARCH_ADDENDUM`,
deliberatamente: quella costante è usata da `agent::system_prompt`, che
questo task non tocca). Istruisce l'AI, PRIMA di rispondere alla richiesta
dell'utente nel turno corrente, a: chiedere se l'utente ha già un nome che
usa per lei altrove; se sì, usare quello; se no, sceglierlo lei; in
entrambi i casi, chiamare `set_ai_display_name` col nome scelto.

### Gap trovato durante l'implementazione (stesso genere del Task 7)

Il piano (pseudocodice originale, Step 5 del brief) applicava l'addendum
incondizionatamente:

```rust
let mut system = agent::system_prompt(opts);
if needs_ai_name_prompt() {
    system.push_str(AI_NAME_REQUEST_ADDENDUM);
}
```

Questo raggiunge ANCHE i turni dei canali tool esterni (es. mcp-nmap —
`Docs/superpowers/specs/2026-07-16-tool-isolation-design.md`), riconoscibili
da `TurnOptions::system_prompt_override: Some(...)` (contro `None` per
cursore/Telegram — vedi il doc-comment su `TurnOptions` in `agent.rs`). Il
`ToolClient::tool_defs()` di QUEI canali (`NmapToolClient`,
`PythonMcpToolClient`, `FixtureChannelToolClient`, `EmptyToolClient`)
sovrascrive il default e NON include `set_ai_display_name` (verificato
leggendo tutti e 4 gli override in `nmap_tool_client.rs`,
`python_mcp_tool_client.rs`, `tool_client.rs`, `external_channel.rs`):
l'AI di quei canali si sarebbe vista istruita — in italiano, fuori
contesto — a chiedere un nickname all'"utente" e a chiamare un tool che
il proprio set non contiene.

Effetto concreto, non solo teorico: un test ESISTENTE,
`channel_tool_client_restricts_tools_and_uses_own_system_prompt`
(riga ~2296 circa dopo questo task), asserisce l'UGUAGLIANZA ESATTA fra il
system prompt inviato al backend e l'override del canale fixture:

```rust
assert_eq!(req.system, "SEI IL CANALE FIXTURE, hai solo fixture_tool_a/b");
```

Su qualunque macchina con un `network.json` reale in stato "AI Chat attivo,
nessun nome" (es. la macchina di sviluppo, dopo aver abilitato AI Chat senza
ancora aver fatto scegliere un nome all'AI), la versione incondizionata
avrebbe rotto quel test SILENZIOSAMENTE in quell'ambiente pur restando verde
in CI/sandbox pulita — esattamente il tipo di "assunzione non verificata su
un altro canale" che la review del Task 7 aveva trovato su
`display_invocation`.

**Fix**: guardia dedicata, funzione PURA testabile senza I/O:

```rust
fn apply_ai_name_addendum(system: String, has_override: bool, needs_prompt: bool) -> String {
    if has_override || !needs_prompt {
        system
    } else {
        format!("{system}{AI_NAME_REQUEST_ADDENDUM}")
    }
}
```

`has_override` rispecchia `opts.system_prompt_override.is_some()`: quando
vero, l'addendum non viene MAI applicato, indipendentemente da
`needs_prompt` — i canali tool esterni non lo vedono mai. La funzione non
fa I/O proprio per questo: separare "decidere se append" (puro, testabile
con due booleani in-memory) da "calcolare `needs_prompt`" (impuro, legge il
filesystem) permette di testare la guardia con RED→GREEN reale senza
toccare `LARE_LOCAL_DIR`/env globali nei test (rischio di interferenza fra
test paralleli nello stesso binario — il crate evita già questo pattern
altrove: `python_mcp_tool_client.rs`/`tool_client.rs` usano nomi di env var
DEDICATI e non condivisi quando devono mutare env nei test; `LARE_LOCAL_DIR`
è invece condiviso da più funzioni di produzione, quindi non se ne muta il
valore globale in un test unitario).

### Wiring in `LlmAdapter::respond()`

```rust
let system = apply_ai_name_addendum(
    agent::system_prompt(opts),
    opts.system_prompt_override.is_some(),
    needs_ai_name_prompt(),
);
let turn_tools = agent::tools_for(opts, tools.tool_defs());
```

Sostituisce `let system = agent::system_prompt(opts);`, dentro il loop
`for _ in 0..agent::MAX_ITERATIONS`, una lettura per iterazione — stesso
punto e stesso principio già commentato lì per
`self.backend.supports_web_search()`.

### Debito noto (non introdotto da questo task, ma reso più visibile)

- **Il nudge può ripetersi se l'utente nega la conferma**: `set_ai_display_name`
  è un tool ad effetto, gated dal meccanismo di conferma esistente (ADR-007 —
  vedi Task 7). Se l'utente nega quella conferma (o su Telegram non risponde
  in tempo), `network.json` non viene scritto: `needs_ai_name_prompt()`
  torna `true` anche al turno successivo, e l'addendum ricompare — la
  promessa che contiene ("dopo non te lo richiederò più") si rivela falsa in
  quel caso. Conseguenza diretta della scelta deliberata "stateless, nessuna
  cache" (vedi sopra) fatta dal piano, non un difetto introdotto qui — ma
  segnalato esplicitamente perché durante lo smoke test dal vivo un
  "continua a chiedermi il nome" dopo un rifiuto NON è un bug di questo
  task, è il comportamento atteso di un meccanismo stateless.
- **Restart-required (Task 7) ora più visibile**: `AiChatService` legge
  `ai_display_name` una sola volta all'avvio (v0.41.14), non "live" — vedi
  la nota gemella nell'`IMPLEMENTATION.md` del Task 7. Con questo nudge
  attivo, l'AI confermerà con sicurezza un nome appena scelto ("fatto, ora
  mi chiamo Aria") che però non compare in AI Chat finché l'orchestrator non
  riparte — più probabile da notare ora che l'AI lo propone spontaneamente,
  non solo quando l'utente chiama il tool a mano.

### Test

7 nuovi in `ai_adapter.rs::tests`, vicino a
`memory_file_path_local_dir_override_wins_over_local_appdata`:

- `needs_ai_name_prompt_true_when_enabled_and_no_ai_name`
- `needs_ai_name_prompt_false_when_ai_already_has_a_name`
- `needs_ai_name_prompt_false_when_aichat_disabled`
- `needs_ai_name_prompt_false_when_file_missing`
- `ai_name_addendum_skipped_when_channel_has_system_prompt_override` (gap sopra)
- `ai_name_addendum_skipped_when_not_needed`
- `ai_name_addendum_appended_when_needed_and_no_override`

### Verifica della domanda del brief — con dato reale, non assunto

Il brief chiede esplicitamente: "l'ambiente di test di default (nessun
`network.json` nella cwd di test) fa sì che `needs_ai_name_prompt()` (la
versione non parametrizzata) restituisca `false`?" — la premessa della
domanda è imprecisa e va corretta prima di rispondere: `network_json_path()`
non risolve dalla CWD di lancio dei test, risolve `LARE_LOCAL_DIR` (env) →
`%LOCALAPPDATA%\dev.lare.terminal\network.json` (identico a
`ai_adapter::memory_file_path`). "Nessun file nella cwd" è quindi ininfluente
di per sé: quello che conta è se quel path REALE ha un `network.json`.

Controllato con una lettura diretta (nessuna scrittura) sulla macchina che
ha eseguito questa suite:

```powershell
$p = "$env:LOCALAPPDATA\dev.lare.terminal\network.json"
Get-Content $p | ConvertFrom-Json | Select-Object enabled, ai_display_name
# → enabled: True, ai_display_name: (assente)
```

Il file esiste già (AI Chat è stato usato dal vivo su questa macchina, per
altre feature del progetto — vedi lo storico di `Docs/HANDOFF.md`) e nessuno
ha ancora chiamato `set_ai_display_name` (il tool è del Task 7, appena
scritto). Quindi `needs_ai_name_prompt()` ha restituito **`true`** durante
l'intera run di 890 test di questo task — non `false` come inizialmente
scritto in una prima versione di questa nota (corretta dopo la verifica
puntuale, prima di dichiarare il task concluso). Questo è un risultato PIÙ
forte di quello che il piano chiedeva di verificare, non più debole:

- Ogni test `respond()` con `TurnOptions::default()` (nessun
  `system_prompt_override`) in questa suite ha effettivamente ricevuto
  l'addendum nel system prompt inviato al backend — il ramo `true` di
  `apply_ai_name_addendum` è stato esercitato dal vivo, non solo in teoria.
- `channel_tool_client_restricts_tools_and_uses_own_system_prompt` — il
  test la cui uguaglianza esatta (`assert_eq!(req.system, "SEI IL CANALE
  FIXTURE...")`) il gap sopra avrebbe rotto — è passato perché la guardia
  lo ha protetto DAVVERO in questo stato, non ipoteticamente.

I due test citati esplicitamente dal piano come potenzialmente a rischio
restano comunque non toccati, ma per un motivo strutturale indipendente
dallo stato di `network.json`, non per assenza del file:

- `chat_system_prompt_without_memory_still_instructs_on_marker` (chiama
  `chat_system_prompt(...)` direttamente)
- `system_prompt_mentions_save_routine_rules`, in `agent.rs` (chiama
  `system_prompt(TurnOptions::default())` direttamente)

non passano MAI da `LlmAdapter::respond()` — chiamano le funzioni pure a
cui `needs_ai_name_prompt`/`apply_ai_name_addendum` sono estranee per
costruzione, indipendentemente da cosa contenga `network.json`.

**Su una macchina/CI dove quel file fosse assente** (es. un runner pulito),
`needs_ai_name_prompt()` tornerebbe `false` per fail-safe (`std::fs::read`
fallisce) — la suite resterebbe comunque verde, ma senza esercitare dal
vivo il ramo `true`: questo task lo ha verificato con evidenza diretta su
QUESTA macchina, non per costruzione logica soltanto.

## `agent.rs`: tool `set_ai_display_name` + `dispatch_tool`/`dispatch_tool_at` (v0.41.15)

Settimo task del piano `Docs/superpowers/plans/2026-08-13-aichat-display-names.md`.

**Refactor DI di `dispatch_tool`** (stesso principio di
`ai_adapter::memory_file_path`/`memory_file_path_with_base`): la funzione
pubblica esistente

```rust
pub async fn dispatch_tool(tools: &dyn ToolClient, name: &str, input: &serde_json::Value)
    -> crate::tool_client::DispatchOutcome
```

è un thin wrapper — risolve `network.json` e delega a:

```rust
pub async fn dispatch_tool_at(tools: &dyn ToolClient, name: &str, input: &serde_json::Value,
    network_json_path: &std::path::Path) -> crate::tool_client::DispatchOutcome
```

che contiene il match VERO. La firma pubblica di `dispatch_tool` (3
parametri) resta identica: nessun chiamante esistente
(`tool_client.rs::MpcToolClient::dispatch`/`FakeToolClient::dispatch`,
`cwd_tracking.rs::CwdTrackingToolClient::dispatch`) è stato toccato. Tutti
gli arm esistenti (`"run_in_session"`, `"open_target"`,
`"search_routines"`, `"run_routine"`, `"get_routine_content"`,
`"save_routine"`, il catch-all `other`) sono stati spostati
carattere-per-carattere dentro `dispatch_tool_at`, sopra l'arm nuovo —
nessun comportamento cambiato per i tool già in produzione. `use
crate::tool_client::DispatchOutcome;` è ora dichiarato una sola volta, in
cima a `dispatch_tool_at`.

**Aggiornato dal fix review finale (v0.41.17, vedi sezione in cima al
file)** — a questa versione (v0.41.15), `dispatch_tool` risolveva
`network.json` con un `default_network_json_path()` privato di questo file
(env `LARE_LOCAL_DIR` > `startup_config::default_local_dir()`, SENZA
leggere `startup.json`). Quella funzione è stata rimossa: `dispatch_tool`
chiama ora `crate::aichat::config::resolve_network_json_path()`, condivisa
con `ai_adapter::needs_ai_name_prompt` (Important #4 — prima del fix i due
avrebbero potuto risolvere due path diversi su una macchina con
`startup.json{local_dir:...}` senza la env var corrispondente).

**Arm `"set_ai_display_name"` — riscritto dal fix review finale (v0.41.17,
Important #1), sezione ORIGINALE storica sotto per riferimento**: a questa
versione (v0.41.15) l'arm faceva `crate::aichat::config::load_or_generate(path)`
(Task 1, NON `_with_migration`) → `cfg.ai_display_name = Some(trimmed)` →
`crate::aichat::config::save(path, &cfg)` — un round-trip COMPLETO tramite
il tipo tipizzato `AiChatConfig`, motivato all'epoca come "stesso pattern
già in uso da ogni altro consumer di `AiChatConfig` in questo crate
(`main.rs`, `/config` backend Task 5)". **Quella motivazione era FALSA**: il
backend `/config` del Task 5 non è in questo crate (è `crates/ui`) e usa il
pattern OPPOSTO — `aichat_settings::merge_aichat_settings` legge
`network.json` come `serde_json::Value` e fa `insert`/`remove` SOLO sulle
chiavi di propria competenza, proprio per NON perdere chiavi ignote o
azzerare il file su un round-trip tipizzato. Il round-trip di questa
versione aveva due problemi reali (dettaglio nel fix in cima al file):
cancellava silenziosamente chiavi ignote condivise da altre feature, e su
un file CORROTTO rigenerava in silenzio i default (`enabled: false`)
dietro un "successo". **Dopo il fix v0.41.17**, l'arm usa
`merge_ai_display_name` (Value-based, preserva ogni chiave, corrotto →
errore esplicito) — lo stesso pattern di `merge_aichat_settings`, ORA
davvero: la frase "stesso pattern già in uso da ogni altro consumer" è
diventata vera nel momento in cui il codice ha smesso di fare il
round-trip che la contraddiceva.

**Nota restart-required**: `AiChatService` legge `display_name`/
`ai_display_name` UNA SOLA volta all'avvio (v0.41.14 — costruttore,
stesso pattern di `ai_participates`/`ai_autoparticipate`), non "live".
Questo tool scrive `network.json` a runtime: a questa versione (v0.41.15)
`dispatch_tool_at` ritornava `"Nome impostato: {trimmed}."` — un successo
silenzioso sul fatto che il nickname NON è ancora visibile in AI Chat. Il
nickname compare solo dopo il prossimo riavvio dell'orchestrator. Debito
accettato preesistente (memoria di progetto `aichat-config-restart-policy`),
non introdotto né risolto da questo task — rilevante per lo smoke test dal
vivo: un turno "chiamati Aria" seguito da un messaggio AI Chat SENZA
riavvio mostrerà ancora il vecchio (o nessun) nickname, non è un bug di
questo task. **Fix Minor della review finale (v0.41.17)**: il messaggio di
successo ora lo dice esplicitamente — `"Nome impostato: {trimmed} (visibile
in AI Chat dopo il riavvio dell'orchestrator)."`.

**`tool_defs()`**: nuova `ToolDef` `set_ai_display_name` (system prompt
NON aggiornato in questo task — l'addendum che dice all'AI QUANDO
chiamarlo spontaneamente è Task 8, fuori scope qui: qui il tool è solo
disponibile e chiamabile). **`display_invocation` HA un arm dedicato**
per `set_ai_display_name` (`"imposta nome AI: {nome}"`): necessario
perché `build_label`
(`ai_adapter.rs::build_label`, riga ~59) ricade su
`agent::display_invocation` quando non c'è un `format_invocation`
override, e il risultato è usato SEMPRE come chunk di trasparenza
(`emit!(chunk(format!("{label}\n\n")))`, riga ~788) indipendentemente dal
fatto che il tool sia effettivamente gateizzato — senza l'arm, sia il
banner di conferma (Telegram, dove tutti i tool tranne `show_markdown`
sono gateizzati) sia il chunk del pannello locale avrebbero mostrato il
catch-all generico `[tool set_ai_display_name]`, senza il nome scelto
dall'AI. Trovato in review (non nel brief originale), aggiunto con RED→GREEN
(`display_invocation_set_ai_display_name_shows_name`).

**Gate di conferma**: nessuna modifica a `ai_adapter.rs`. `set_ai_display_name`
non riceve un trattamento speciale come `show_markdown` (che bypassa il
gate) — passa dal ramo `else` generico, quindi richiede la stessa conferma
di `run_in_session`/`open_target` per ADR-007, come specificato dal brief.
Localmente (UI cursore) `LocalUiConfirmer` gateizza SOLO i tool in
`SENSITIVE_TOOLS` (`local_confirm.rs`) e `set_ai_display_name` NON vi è
incluso — quindi in locale esegue senza conferma esplicita (coerente con
`run_in_session`/`open_target`, anch'essi assenti da quella lista); su
Telegram (`TelegramConfirmer`, gate su tutto tranne `show_markdown`)
richiede sempre conferma. Nessuna decisione presa qui su se
`set_ai_display_name` debba entrare in `SENSITIVE_TOOLS`: fuori scope,
il brief specifica solo "gated dal meccanismo ESISTENTE", non un nuovo
livello di sensibilità.

**Effetto collaterale meccanico**: `tool_defs()` passa da 7 a 8 elementi.
Ogni test in questo crate che asserisce un conteggio hardcoded del numero
di tool esposti all'AI (non la logica di alcun arm) è stato aggiornato di
+1 — 8 siti in 4 file (`agent.rs`, `ai_adapter.rs`, `core.rs`,
`tool_client.rs`), dettaglio nel `CHANGELOG.md` di questa versione. Un
test rinominato (`tools_for_default_has_seven_custom` →
`tools_for_default_has_eight_custom`, il nome era diventato letteralmente
sbagliato dopo l'incremento).

**Dipendenza**: `startup-config` era già una dipendenza di
`crates/orchestrator/Cargo.toml` (fase `startup-config-phase1`, per
`main.rs`/`llms_config.rs`/`telegram/settings.rs`) — nessuna riga aggiunta
a `Cargo.toml`, solo un nuovo sito d'uso (`startup_config::resolve`/
`startup_config::default_local_dir`, riferiti per path di crate diretto,
senza `use`, stesso stile di `main.rs`).

**Test**: 4 nuovi (`tool_defs_includes_set_ai_display_name`,
`dispatch_set_ai_display_name_writes_network_json`,
`dispatch_set_ai_display_name_rejects_empty_name`,
`display_invocation_set_ai_display_name_shows_name`), RED→GREEN
verificato per ciascuno; `cargo test -p orchestrator` — intera suite
verde (883 passed, 0 failed; baseline prima di questo task: 879 passed).

## `main.rs`: consolidamento startup.json (v0.41.11)

`local_dir`/`exe_dir`/`startup_cfg` risolti una volta all'inizio di
`main()` (dopo il log delle versioni, prima del blocco token) invece che
ri-derivati 5 volte — più le 2 risoluzioni locali temporanee introdotte da
0.41.9 (blocco AI adapter) e 0.41.10 (blocco Telegram), rimosse qui. Ogni
sito consumatore riceve il valore già pronto:

- token: `token_store::resolve_token(&local_dir)` (prima: `&app_data_dir`
  ricalcolato lì).
- llms.json: `llms_config::resolve_path(env, file, &local_dir)`.
- telegramsettings.json: `telegram::settings::resolve_path(env, file,
  exe_dir)` — `launch_dir` (cwd di lancio) già eliminata in v0.41.10. Il
  commento di flusso del blocco Telegram in `main.rs` è stato aggiornato
  per riflettere questo (precedenza a 3 livelli, ancoraggio a `exe_dir`,
  `local_dir` condiviso) — descriveva ancora il comportamento
  pre-v0.41.10 (cartella di lancio/cwd).
- search-paths.json/search-content.json: `local_dir.join(...)`.
- plugin host: `plugins_dir` preserva il proprio match locale senza trim
  su `LARE_PLUGINS_DIR` (comportamento invariato), ma ricade su
  `startup_config::resolve(None, startup_cfg.plugins_dir, || local_dir
  .join("plugins"))` invece che direttamente su `app_data_dir.join
  ("plugins")` quando l'env non è impostata — `startup.json` può ora
  specificarlo.
- aichat: `network.json`/`aichat.json` (migrazione) + `notes.json` via
  `local_dir.join(...)`. Il binding condiviso è chiamato `startup_cfg`
  (non `cfg`) proprio per lasciare libero il nome `cfg` già in uso da
  questo blocco (la config di rete caricata) — nessuna rinomina a cascata
  necessaria.

`main()` non ha una propria suite di test (glue code, come già `exe_dir()`
di `startup-config`) — verificato da build pulita + l'intera suite di
`orchestrator` (871 test, tutti verdi, inclusi quelli di
`llms_config`/`telegram::settings` da Task 3/4 ora effettivamente
esercitati anche dal binario) + un test manuale dell'avvio reale (binario
compilato lanciato con un `startup.json` malformato accanto: un solo
`WARN ... JSON malformato ... — uso i default` in stderr, non due — la
doppia lettura/doppio log di 0.41.9+0.41.10 è sparita) + smoke test dal
vivo (Task 7 del piano).

## `telegram::settings::resolve_path`: exe_dir + startup.json (v0.41.10)

Nuova firma: `resolve_path(env_override: Option<&str>, file_value: Option<&str>, exe_dir: &Path) -> PathBuf`
(prima: `(env_override: Option<String>, launch_dir: &Path)`). `launch_dir`
(`std::env::current_dir()`, catturato in `main.rs` PRIMA del blocco "Init
home") è eliminata: era l'unico consumatore oltre alla propria
dichiarazione (verificato). Sostituita da `exe_dir`
(`startup_config::exe_dir()`) — fix di un bug latente reale per lo
scenario servizio Windows (vedi CHANGELOG). Task 5 sostituisce la
risoluzione locale di `exe_dir`/`startup_cfg` introdotta qui col binding
condiviso di livello `main()`.

## `llms_config::resolve_path` da `startup.json` (v0.41.9)

Nuova firma: `resolve_path(env_value: Option<&str>, file_value: Option<&str>, local_dir: &Path) -> PathBuf`,
thin wrapper su `startup_config::resolve(env_value, file_value, || local_dir.join("llms.json"))`.
`default_path` (la ri-derivazione interna di `%LOCALAPPDATA%`/`.lare-data`)
è eliminata — quella logica vive ora SOLO in `startup_config::default_local_dir`.

Bug-fix minore incluso (non un comportamento da preservare, vedi spec §3):
l'override `LARE_LLMS_CONFIG` non faceva `.trim()` sull'input — ora lo fa,
via `startup_config::resolve`.

`main.rs:154` (unico chiamante) aggiornato nello stesso commit con una
risoluzione locale di `exe_dir`/`startup_cfg`/`local_dir` — temporanea,
duplicata anche nel blocco Telegram (Task 4) e negli altri 3 siti
(token/search/plugin/aichat, ancora sulla vecchia catena inline). Task 5
consolida tutto in un binding unico condiviso, rimuovendo queste
duplicazioni locali.

## Wiring di `get_routine_content`/`save_routine` (v0.41.0)

Fase 2 del repository di routine (Docs/superpowers/specs/2026-08-05-save-routine-design.md):
due nuovi tool AI esposti all'assistente, wired nei tre punti di dispatching in `agent.rs`
(`tool_defs`, `dispatch_tool`, `display_invocation`) e nel `SYSTEM_PROMPT` per guidare
l'AI a cercare prima di creare, e testare prima di salvare.

**`ToolConfirmer::confirm_routine_save`**: nuovo metodo di trait dedicato al gate di
`save_routine`, con una surface specializzata (a differenza del generico `confirm`
che è testo piatto). Default: appiattisce in testo e passa a `confirm()`, corretto per
`TelegramConfirmer` (nessun override). `LocalUiConfirmer` l'UNICO override (Task 7):
apre una finestra dedicata che mostra il corpo completo dello script prima di risolvere,
esclude `save_routine` dal banner batched (un turno con `save_routine` + altro tool
gateizzato = due conferme indipendenti, non raggruppate).

**`RoutineSaveRequest`**: struct interna (non protocoll) che incapsula i campi di
`save_routine` estratti dal JSON del tool_use, costruita da `build_routine_save_request`
e passata al confirmer. Tutti i 5 campi obbligatori + `replace` opzionale arrivano
dalla stessa chiamata AI, nessun ulteriore round-trip con `ToolClient`.

**Per-tool `approvals` map**: Task 8 ha esteso il gate da un flat `approved: bool`
a una mappa `HashMap<String, bool>` mantenuta fra i turni, permettendo a ciascun tool
gateizzato di essere indipendentemente confermato/rifiutato. `save_routine` usa questa
nuova architettura — ogni invocazione del tool ha la sua chiave nella mappa, identificata
dal `tool_use_id` che il modello assegna a quella singola chiamata (non da nome+parametri:
niente hashing, l'id del modello è già univoco per invocazione).

**`ToolClient` trait**: due nuovi metodi di default (`get_routine_content` e `save_routine`),
entrambi default-bodied con rifiuto "non disponibile su questo canale". Override reali su
`McpToolClient` (delega a mcp-server), `FakeToolClient` (canned: not found per
`get_routine_content`, ok:true per `save_routine`), `CwdTrackingToolClient` (delega a
`inner`). Tutti gli altri canali (fixture/nmap/python via loro traitstubs) li ereditano
identici — nessun nuovo wire format, protocoll già preparato da Task 4.

`save_routine` aggiunto a `SENSITIVE_TOOLS` in `local_confirm.rs` per abilitare il
gate (a differenza di `get_routine_content`, una sola lettura, non gateizzata).

## Review finale save_routine: chunk "anteprima aperta" spostato nel confirmer (v0.41.1)

Il `ServerMsg::Chunk` "📄 anteprima routine '\<name\>' aperta — rivedila nella finestra"
viveva incondizionato in `ai_adapter.rs`, emesso PRIMA di dispatchare a qualunque
confirmer attivo — corretto per la UI locale, fuorviante su Telegram (nessuna finestra,
solo un round-trip a bottoni via il default di `confirm_routine_save`). Spostato dentro
`LocalUiConfirmer::confirm_routine_save` (`local_confirm.rs`): ogni implementazione del
trait è ora responsabile della propria messaggistica channel-appropriate, invece di un
`if` esterno che ispeziona il tipo di confirmer. `LocalUiConfirmer` guadagna un campo
`command_id: String` (4° parametro di `new`, impostato da `ws.rs` all'`id` del
comando/turno corrente) — necessario perché il Chunk deve portare lo stesso `id` di ogni
altro Chunk del turno, e solo `ws.rs` lo conosce al momento della costruzione.

## Fix `render_body`: segmento vuoto non genera intestazione penzolante (v0.40.70)

Trovato in review (`advisor`) subito dopo 0.40.69, non ancora osservato dal
vivo: quel fix rende raggiungibile `NoteEditRequested { text: "" }` per la
prima volta. `merge_note` (`notes/mod.rs`) non rimuove mai un `Segment` dal
vettore — un segmento svuotato resta con `text: ""` e il suo `seq`, serve al
merge CRDT (un futuro re-invio dalla stessa macchina deve battere questo
`seq`, non ripartire da zero). Ma `render_body` (`notes/digest.rs`) contava
`segments.len()` grezzo per decidere "2+ macchine → intestazioni per
macchina": una nota a 2 macchine con un segmento svuotato restava a
`len() == 2`, quindi mostrava comunque `— machine —\n` per quella macchina,
senza nulla sotto.

```rust
pub fn render_body(note: &Note) -> String {
    let mut segments: Vec<_> = note.segments.iter().filter(|s| !s.text.is_empty()).collect();
    segments.sort_by(|a, b| a.machine.cmp(&b.machine));
    if segments.len() <= 1 {
        return segments.first().map(|s| s.text.clone()).unwrap_or_default();
    }
    segments.iter().map(|s| format!("— {} —\n{}", s.machine, s.text)).collect::<Vec<_>>().join("\n\n")
}
```

Un segmento a testo vuoto conta ora come "non ha contribuito" per il
rendering — il `Segment` resta nei dati per il CRDT, semplicemente non
produce output. Test: `render_body_empty_segment_excluded_no_dangling_header`,
`render_body_all_segments_empty_is_empty_string`.

## Fix primo smoke test dal vivo del Blocco note (v0.40.69)

Primo finding del primo smoke test multi-macchina reale: "Modifica" mostrava
il titolo ma mai il testo della nota. Causa: `NoteView` (protocol) non
esponeva alcun modo di isolare "il segmento di QUESTA macchina" — solo
`body`, il corpo fuso di tutte le macchine.

**Il fix.** `to_note_view` da funzione associata a metodo (`&self`), per
poter leggere `self.me.label_base` e isolare il proprio segmento:

```rust
fn to_note_view(&self, note: &Note) -> protocol::NoteView {
    let my_segment_text = note
        .segments
        .iter()
        .find(|s| s.machine == self.me.label_base)
        .map(|s| s.text.clone())
        .unwrap_or_default();
    protocol::NoteView {
        id: note.id.clone(),
        title: note.title.clone(),
        body: render_body(note),
        my_segment_text,
        created_by: note.created_by.clone(),
        created_at_ms: note.created_at_ms,
        deleted: note.deleted,
    }
}
```

Tutti i 7 call site aggiornati da `Self::to_note_view(...)`/
`.map(Self::to_note_view)` a `self.to_note_view(...)`/
`.map(|n| self.to_note_view(n))` (i vecchi snippet più sotto in questo file,
relativi a Task 6, restano come documentano lo stato di allora — non
riscritti).

**Test.** `note_view_my_segment_text_reflects_own_segment_on_create` (una
nota creata localmente porta il proprio testo in `my_segment_text`) e
`note_view_my_segment_text_empty_when_this_machine_never_contributed` (una
nota arrivata da un peer a cui questa macchina non ha ancora contribuito ha
`my_segment_text` vuoto, ma `body` mostra comunque il corpo fuso — i due
campi non vanno confusi).

Protocol 0.14.9 aggiunge il campo lato wire; UI 0.45.39 lo consuma per
precompilare il dialog "Modifica" — v. i rispettivi CHANGELOG.

## Fix di review finale del branch "Blocco note" (v0.40.68)

Due gap di INTEGRAZIONE fra task, invisibili alle review dei singoli task.
File toccati: `src/aichat/service.rs`, `src/notes/mod.rs` (solo test).

### FIX 1 — `next_note_id` seedato dallo store caricato (Critical, perdita di dati)

**Il bug.** `AiChatService::new` faceva `next_note_id: 0` incondizionatamente,
ma `main.rs` gli passa uno `NotesStore` **già caricato da `notes.json`** — che
dopo un riavvio contiene tipicamente `"<mio-ip>:0"`, `"<mio-ip>:1"`, … Quindi la
prima nota creata dopo ogni riavvio riusava un id esistente. E poiché la
creazione passa da `self.notes.upsert_merged(note)`, che su id già noto **fonde
invece di inserire**, la nota "nuova" spariva dentro la vecchia:

- il testo nuovo veniva **scartato** (`merge_note` tiene il segmento con `seq`
  maggiore, e quello vecchio ce l'aveva);
- il titolo della vecchia veniva **sovrascritto** via LWW;
- se la vecchia era un **tombstone**, la nota "nuova" nasceva `deleted: true` e
  non compariva mai.

Il tutto senza errori né log — e il risultato corrotto veniva poi
**ribroadcastato a ogni altra macchina** dal normale fan-out.

**Il fix.** In `new()`, prima di costruire lo struct, il contatore si ricava
dall'archivio stesso: massimo suffisso numerico fra gli id che portano il
proprio prefisso, `+1` (0 se non ce ne sono).

```rust
let my_id_prefix = format!("{}:", me.id.0);
let next_note_id = notes.all().iter()
    .filter_map(|n| n.id.strip_prefix(&my_id_prefix))
    .filter_map(|counter| counter.parse::<u64>().ok())
    .max()
    .map_or(0, |max| max + 1);
```

Il prefisso è l'**IP** (`me.id.0`), non `label_base` — è lo schema di
`next_note_id`/`fresh_share_id`. Gli id delle altre macchine hanno un prefisso
diverso, quindi vivono in uno spazio di nomi separato e non entrano nel calcolo.
Se l'IP cambia (DHCP), il prefisso cambia con lui e il contatore riparte da 0 su
uno spazio vergine: nessuna collisione, per costruzione.

**Test.** `next_note_id_is_seeded_from_the_loaded_store_so_new_notes_never_collide`
(RED prima del fix: `left: "192.168.1.10:0" == right: "192.168.1.10:0"`).
Asserisce non solo l'id diverso (proxy debole) ma anche `all().len() == 2` e la
sopravvivenza del testo nuovo — la vera regressione. In più, in `notes/mod.rs`,
un test di **caratterizzazione** (`merge_same_machine_same_seq_same_text_...`)
copre il ramo di tie-break `seq` uguale che nessun test toccava: è la
precondizione che il doc-comment di `merge_note` assume e che proprio questo fix
rende davvero vera — prima, due note DIVERSE potevano nascere con lo stesso id e
`seq = 1`, cioè stessa chiave `(machine, seq)` con testo diverso, uno stato in
cui il merge non è commutativo.

### FIX 5 — fan-out solo se la nota è davvero cambiata (Important)

**Il bug.** Gli arm che ricevono note dalla RETE (`ChatMsg::NoteUpdated`, il
ciclo `push` di `NotesDigestReply`, e `NotesData`) emettevano
`ToUi(NoteUpserted)` + `SaveNotes` + fan-out **per ogni nota in arrivo**, anche
quando il merge non cambiava nulla (nota già identica in archivio). Contro il
design §5.2 punto 4 ("ri-broadcast solo quando è CAMBIATA rispetto a quanto
aveva prima"), con rimbalzi fra peer che si auto-alimentano più I/O e ri-render
inutili.

**Il fix.** Nuovo helper `apply_incoming_note(incoming, from, fan_out)`, coda
comune dei tre arm remoti: snapshot `self.notes.get(&id).cloned()` PRIMA del
merge, confronto col risultato via il `PartialEq` derivato di `Note`, e ritorno
di `Vec::new()` se identici. `before == None` (nota mai vista) conta sempre come
"cambiata". `fan_out: false` per `NotesData`, dove il mittente ha già fatto il
proprio fan-out e un secondo invio qui produrrebbe un doppio percorso.

**Scelta deliberata:** `broadcast_note_to_links` NON è stata toccata e resta
incondizionata. La usano i quattro arm **locali**
(`NoteCreate/Edit/EditTitle/Delete Requested`), dove il broadcast è sempre
dovuto: è l'utente di questa macchina che ha agito, e le altre devono saperlo
anche quando lo stato locale non cambia — es. ri-cancellare una nota già
tombstoned qui ma ancora viva altrove, dove gatare avrebbe silenziosamente
rotto la propagazione del tombstone.

**Test.** `peer_msg_note_updated_identical_to_stored_produces_no_effects` (RED
prima del fix: il secondo arrivo produceva `ToUi` + `SaveNotes` +
`SendToPeer`). I test di fan-out esistenti restano verdi: usano note che il
servizio non ha ancora, quindi `before = None` → effetti attesi.

## `ws.rs` instrada i ClientMsg di Blocco note (v0.40.67 — Task 14)

**Contesto:** i quattro `ServiceEvent` di "Blocco note" (`NoteCreateRequested`,
`NoteEditRequested`, `NoteEditTitleRequested`, `NoteDeleteRequested`) sono stati
testati e implementati nel backend `AiChatService` (Task 7-13). Task 14 collega
il WS layer (`ws.rs`) al servizio mediante dispatch — quando il frontend locale
invia un `ClientMsg::NoteCreate` (o simile), il WS lo router come il matching
`ServiceEvent` all'inbox del servizio.

**Modifiche:** `crates/orchestrator/src/ws.rs`, funzione `handle_client_msg()`,
righe 668-674 (precedentemente placeholder Task 6):

```rust
ClientMsg::NoteCreate { title, text } => {
    if let Some(h) = &aichat {
        let _ = h.send(ServiceEvent::NoteCreateRequested { title, text });
    }
}
// (analogamente per NoteEdit, NoteEditTitle, NoteDelete)
```

**Forma esatta:** replica perfettamente i 5 `ClientMsg::Share*` arm già presenti
(linee ~637-659) — nessuna logica nuova, solo un `if let Some(h)` che invia
verso l'inbox del servizio. Test unitari non aggiunto (puro dispatch già coperto
dai test di integrazione WS e dalla logica applicativa in Task 7-13).

---

## Replay `NotesSnapshot` su ogni SetServerTx (v0.40.66 — Task 13: Blocco note)

**Contesto:** l'ultima modifica di pura logica lato `AiChatService` prima di
collegare il backend con il WS layer (`ws.rs`) e il frontend. Quando la UI
(ri)connette, `SetServerTx` già ri-emette diversi state snapshots: `AiChatSelf`
(etichetta), `AiChatReachablePeers` (peer raggiungibili), roster (partecipanti
al giro chat), storico messaggi, consensi di ammissione pendenti, e Share request.

**Modifiche:** `crates/orchestrator/src/aichat/service.rs`:

1. **Nuovi test (2):**
   - `set_server_tx_replays_notes_snapshot_unconditionally_even_when_empty()` —
     verifica che `SetServerTx` emetta un `NotesSnapshot` ANCHE quando vuoto,
     senza condizione di non-empty (stesso principio di `AiChatReachablePeers`).
   - `set_server_tx_replays_notes_snapshot_with_existing_notes()` — verifica
     che il snapshot contenga tutte le note locali presenti allo scopo della
     riconnessione.

2. **Implementazione:** arm `ServiceEvent::SetServerTx(tx)`, subito dopo la push
   di `AiChatReachablePeers` (righe ~981-983):
   ```rust
   effects.push(Effect::ToUi(ServerMsg::NotesSnapshot {
       notes: self.notes.all().iter().map(Self::to_note_view).collect(),
   }));
   ```
   Unconditional: emesso SEMPRE, col front-end che sa distinguere "snapshot
   vuoto arrivato" da "non ancora arrivato".

3. **Aggiornamento test esistente:** `set_server_tx_event_stores_sender_and_emits_self_label_plus_empty_reachable_peers()`
   — l'assertion sulla lista di effetti ora attende 3 elementi (non 2) perché
   il `NotesSnapshot` è stato aggiunto.

**Backend "Blocco note" completo** — restano: `ws.rs` routing (Task 14),
`main.rs` wiring (Task 15), e il frontend (Task 16-19).

---

## Incoming `ChatMsg::NotesData` — merge fulfillment (v0.40.65)

**Task 12 di "Blocco note":** ricezione del fulfillment di un `want` proprio —
il peer mittente ci rimanda le note che gli abbiamo richiesto via
`NotesDigestReply`. È l'ultimo tassello del giro di riconciliazione bidirezionale
(Task 9 → Task 10 → Task 11 → **Task 12**).

**Modifica:** `crates/orchestrator/src/aichat/service.rs`, arm `ChatMsg::NotesData { notes }`:

Per ogni nota ricevuta:
- Mergiamo nello store via `self.notes.upsert_merged(incoming.clone())`.
- Pushiamo `Effect::ToUi(ServerMsg::NoteUpserted { note: .. })` e
  `Effect::SaveNotes` (stessi due effetti di Task 8's `NoteUpdated` e Task 11's
  `NotesDigestReply::push`).
- **NESSUN fan-out qui** (deliberatamente diverso da `NoteUpdated` e
  `NotesDigestReply::push`): `NotesData` è fulfillment di un `want` CHE NOI
  ABBIAMO RICHIESTO — il mittente sa bene cosa ha mandato e, se ha bisogno di
  inviare un aggiornamento a terzi, lo fa ATTRAVERSO i suoi stessi
  `NoteUpdated`/`NotesDigestReply` quando è il suo turno. Un fan-out aggiuntivo
  qui causerebbe un DOPPIO INVIO della stessa nota, violando il principio di
  "consegnare una sola volta per aggiornamento" (Design §5.2).

**Test (1):** `peer_msg_notes_data_merges_each_note_and_pushes_to_ui` —
verifica che un `NotesData { notes: [a, b] }` ricevuto da un peer mergia
entrambe le note, emetta due `NoteUpserted` e almeno un `SaveNotes`, ma
nessun `SendToPeer` verso altri link.

---

## Incoming `ChatMsg::NotesDigest` → reply con want/push (v0.40.63)

**Task 10 di "Blocco note":** ricezione del digest da un peer, confronto col
nostro store locale, e risposta con le note che ci mancano (`want`) e quelle
che il peer non ha (`push`).

**Modifica:** `crates/orchestrator/src/aichat/service.rs`, arm `ChatMsg::NotesDigest { entries }`:

1. Chiama `crate::notes::digest::reconcile(self.notes.all(), entries)` (funzione
   pura, Task 3) che restituisce `(want, push)` — le note da richiedere al peer
   e quelle da mandargli.
2. Se almeno uno dei due vettori è non vuoto, manda un `ChatMsg::NotesDigestReply
   { want, push }` al peer mittente via `Effect::SendToPeer`.
3. Nessuna modifica dello stato locale durante la ricezione del digest: non è
   un'acquisizione di dati, solo uno scambio di impronte per coordinare il
   successivo trasferimento (che avviene in Task 11-12).

**Test (1):** `peer_msg_notes_digest_replies_with_want_and_push` — verifica che
quando riceviamo un digest che menziona note che non abbiamo E il peer manca
delle note che abbiamo, rispondiamo con un `NotesDigestReply` contenente sia
`want` che `push`.

---

## Incoming `ChatMsg::NotesDigestReply` — fulfill want, merge push, fan-out (v0.40.64)

**Task 11 di "Blocco note":** ricezione della RISPOSTA al nostro digest (mandato
in Task 9): il peer manda note che ci chiede (`push`) e ci elenca le note che
vuole da noi (`want`). Eseguiamo entrambe: fulfiliamo `want` rispondendo con
`NotesData`, mergiamo ogni nota in `push` nello store locale e la facciamo
arrivare agli altri peer già connessi.

**Modifica:** `crates/orchestrator/src/aichat/service.rs`, arm
`ChatMsg::NotesDigestReply { want, push }`:

1. **Fulfillment di `want`:** se `want` è non vuoto, raccogliamo i `Note`
   locali cui corrispondono gli id richiesti via `self.notes.get(id)`, e se
   ce ne sono, mandiamo un unico `ChatMsg::NotesData { notes }` al peer
   mittente via `Effect::SendToPeer`.
2. **Merge di `push`:** per ogni nota ricevuta:
   - Mergiamo nello store via `self.notes.upsert_merged(incoming.clone())`.
   - Pushiamo `Effect::ToUi(ServerMsg::NoteUpserted { note: .. })` e
     `Effect::SaveNotes` (stessi due effetti di Task 8's `NoteUpdated`).
   - **Fan-out:** iteriamo `self.links.keys()` e per ogni peer DIVERSO dal
     mittente (`from`), pushiamo `Effect::SendToPeer(*peer_id, ChatMsg::NoteUpdated
     { note: merged.clone() })` — design §5.2 punto 4, una riconciliazione
     porta i cambiamenti a chi era già in rete, non solo al peer che ci ha
     inviato la nota.

**Test (2):**
- `peer_msg_notes_digest_reply_fulfills_want_with_notes_data` — verifica che
  un `NotesDigestReply { want: [id], push: [] }` generi un `NotesData` con la
  nota locale corrispondente, inviato al peer mittente.
- `peer_msg_notes_digest_reply_merges_pushed_notes_and_fans_out` — verifica che
  un `NotesDigestReply { want: [], push: [note] }` mergia la nota, emetta
  `SaveNotes`, e la ribroadcasti a ogni altro peer collegato (mai al mittente).

---

## Timer di riconciliazione note dopo link stabilito (v0.40.62)

**Task 9 di "Blocco note":** il trigger che fa PARTIRE lo scambio di note fra
due macchine appena si stabilisce un link, senza bisogno di un'azione umana —
Design: `Docs/superpowers/specs/2026-07-29-library-notes-design.md` §5.2, §9.

**Modifica:** `crates/orchestrator/src/aichat/service.rs`, quattro punti:

1. **Costante** `RECONCILE_DEBOUNCE_SECS: u64 = 2` (accanto alle altre costanti
   di timeout come `PING_INTERVAL`, `ADMISSION_VOTE_TIMEOUT_SECS`): debounce
   fisso — non un timer libero — per non scambiare digest a raffica durante
   l'assestamento di un'elezione (più link che si stabiliscono/cadono in
   sequenza ravvicinata).
2. **`ServiceEvent::NotesReconcileDue { peer_id, generation }`**: timer scaduto.
3. **`Effect::StartNotesReconcileTimer { peer_id, generation, secs }`**: gemello
   ESATTO di `Effect::StartVoteTimeout` — stesso corpo in `perform` (spawn +
   `tokio::select!` fra `shutdown.cancelled()` e `tokio::time::sleep`, poi
   re-inietta l'evento sull'inbox). Come per `VoteTimeout`, `shutdown` evita di
   mandare l'evento se l'attore sta già uscendo.
4. **Arm `handle_event` per `NotesReconcileDue`**: guardia di generazione
   IDENTICA a `PeerGone`/`VoteTimeout` — se `self.link_gen.get(&peer_id) !=
   Some(&generation)` il link è stato rimpiazzato o chiuso da quando il timer è
   partito: no-op. Altrimenti costruisce `entries: Vec<(String, NoteDigest)>` da
   `self.notes.all()` via `crate::notes::digest::digest_of` e manda
   `Effect::SendToPeer(peer_id, ChatMsg::NotesDigest { entries })` — impronte di
   TUTTE le note note, mai testo (design §5.2 punto 2).

**Wiring in `run()`:** il timer è agganciato ai due (e SOLO due) punti REALI di
"link appena stabilito" — non ai (più numerosi) punti dove si ricalcola
`reachable_peer_labels`, che includono anche disconnessioni dove non c'è nulla
da riconciliare:
- Ramo 2 (`new_link_rx.recv()`, accept in ingresso): dopo
  `self.links.insert(peer_id, writer_tx)`, subito dopo il blocco
  `Effect::ToUi(AiChatReachablePeers { .. })` già esistente.
- Ramo 4 (`connect_res_rx.recv()`, `ConnectOutcome::Success`, connessione in
  uscita): stesso punto, variabile locale `id` invece di `peer_id`.

In entrambi i casi la `generation` passata all'`Effect` è lo stesso `gen`
appena assegnato da `register_link`/`register_connect_success` — è quella che
`handle_event` confronterà al risveglio del timer.

**Nota di scope:** solo l'INVIO del digest. La gestione della RISPOSTA
(`ChatMsg::NotesDigest` in arrivo sull'altro lato, `NotesDigestReply`,
`NotesData`) è Task 10-12, deliberatamente non toccata qui (YAGNI) — gli arm
di ricezione in `ChatMsg::NotesDigest { .. } => {}` (e gemelli) restano
placeholder no-op.

**Test (4):**
- `notes_reconcile_due_sends_digest_when_generation_matches`: generazione
  combacia → `Effect::SendToPeer(peer, NotesDigest {..})`.
- `notes_reconcile_due_is_noop_for_stale_generation`: generazione NON combacia
  (timer di un link già rimpiazzato) → nessun effetto.
- `digest_entries_reflect_all_known_notes`: una nota locale creata prima del
  timer → il digest inviato la contiene (`entries.len() == 1`). Usa
  `mark_connected_for_test(peer)` + `set_link_gen_for_test(peer, gen)` per
  fissare la generazione registrata al valore atteso (stesso pattern già usato
  altrove in questo file per scenari di generazione stale, es. `PeerGone`).
- `start_notes_reconcile_timer_injects_notes_reconcile_due_event`: test a
  livello `perform` (gemello di `start_vote_timeout_injects_vote_timeout_event`
  e affini) — verifica il collegamento end-to-end `perform` → `tokio::time::sleep`
  → re-iniezione sull'inbox, con `secs: 0` per determinismo.

**Verifiche:** 4 test nuovi verdi; `cargo test -p orchestrator aichat::service::`
195 test verdi (nessuna regressione nel loop `run()` condiviso — keepalive,
elezione, relay); full `cargo test -p orchestrator` 821 test verdi, 0 falliti.

---

## Ricezione di `ChatMsg::NoteUpdated` in arrivo (v0.40.61)

**Task 8 di "Blocco note":** gestione lato-ricevente di aggiornamenti alle note
provenienti da peer remoti durante una sessione AI Chat.

**Modifica:** `crates/orchestrator/src/aichat/service.rs`, arm `ChatMsg::NoteUpdated { .. }`
nella match-exhaustiveness di `ServiceEvent::PeerMsg { from, msg }` (linee ~1599).

**Logica implementata:**

1. **Merge locale**: ricevi `note: Note` dal messaggio, mergialo nel `self.notes`
   via `upsert_merged` (semantica CRDT consolidata da Task 2).
2. **Notifica UI**: emetti `Effect::ToUi(ServerMsg::NoteUpserted { note })` con
   la vista trasformata di `Self::to_note_view(&merged)` (Task 6).
3. **Persistenza**: emetti `Effect::SaveNotes` per far scrivere il `notes.json`
   aggiornato in `perform` (stesso pattern di `Effect::PersistMemory` per AI Chat).
4. **Fan-out ai peer**: per ogni link in `self.links` (ESCLUDENDO il mittente `from`),
   emetti `Effect::SendToPeer(*peer_id, ChatMsg::NoteUpdated { note: merged.clone() })`.
   - Lato **client** (1 link verso server): no-op di fatto (nessun "altro" peer).
   - Lato **server** (n link verso client): broadcast a TUTTI i client tranne chi l'ha
     mandato — è il meccanismo che sincronizza le note fra tutti i client.

**Test (2):**
- `peer_msg_note_updated_merges_and_pushes_to_ui`: verifica merge, `SaveNotes`,
  e `NoteUpserted` verso UI.
- `peer_msg_note_updated_fans_out_to_other_links_excluding_sender`: verifica
  fan-out verso altri peer, escludendo il mittente.

**Verifiche:** 2 test verdi; full test suite `cargo test -p orchestrator` ancora
OK (824 test, nessuna regressione).

---

## NotesStore — persistenza `notes.json` (v0.40.57)

**Nuovo modulo `crates/orchestrator/src/notes/store.rs`** (Slice 3 di "Blocco note"):
store in-memoria e persistente delle note, proprietario dell'orchestrator (sopravvive
a UI chiusa e riavvio orchestrator).

**Interfaccia pubblica:**

- **`NotesStore`** (struct): `path: PathBuf, notes: Vec<Note>`.
- **`load_or_generate(path: &Path) -> Self`**: carica da `path`; se assente o corrotto,
  parte da archivio vuoto (tollerante, come `aichat::config::load_or_generate`).
- **`empty_in_memory() -> Self`**: store puramente in-memoria (usato dai test unitari per
  evitare I/O reale).
- **`save(&self) -> io::Result<()>`**: serializza a JSON pretty in `path`; no-op silenzioso
  se `path` è vuoto (store in-memory).
- **`all(&self) -> &[Note]`**: accesso di lettura all'intero archivio.
- **`get(&self, id: &str) -> Option<&Note>`**: lookup per id.
- **`upsert_merged(&mut self, incoming: Note) -> Note`**: inserisce se id è nuovo; altrimenti
  fonde col merge CRDT via `merge_note(...)` e SOSTITUISCE l'entry (nessun bypass del merge).
  Ritorna il `Note` risultante per broadcast UI.

**Proprietà verificate dai test (5 test):**
- `load_or_generate_empty_when_file_absent`: file assente → archivio vuoto.
- `save_then_load_round_trips`: round-trip persistenza su file.
- `upsert_merged_inserts_new_id`: id nuovo → insert.
- `upsert_merged_merges_existing_id_instead_of_duplicating`: id esistente → merge CRDT,
  non duplicazione.
- `empty_in_memory_save_is_a_silent_noop`: save() su in-memory store è OK silenzioso
  (nessun file scritto).

**Verifiche:** tutti i 5 test passano; nessuna regressione nei 799 test dell'orchestrator.

---

## Digest (impronta senza testo) e riconciliazione per Blocco note (v0.40.56)

**Nuovo modulo `crates/orchestrator/src/notes/digest.rs`** (Slice 2 di "Blocco note"):
logica PURA di riconciliazione fra coppie di peer. Tre funzioni pubbliche:

1. **`NoteDigest`** (struct): impronta di una nota senza il testo dei segmenti.
   Campi: `deleted: bool`, `title_touched: (String, u64)`, `segment_keys: Vec<(String, u64)>`
   (coppie macchina/seq ordinati lessicograficamente). Serializzabile, usato per
   il confronto deterministico fra digest ricevuti e locali.

2. **`digest_of(note: &Note) -> NoteDigest`**: converte una nota completa nel suo
   digest — estrae il `deleted` flag, `title_touched` invariato, e proietta ogni
   segmento alla coppia `(machine, seq)`. Il vettore `segment_keys` risultante è
   ordinato per permettere il confronto O(n) e la serializzazione deterministica
   (nessun testo portato, solo "chi ha detto cosa e quando").

3. **`render_body(note: &Note) -> String`**: renderizza il corpo per la UI.
   - Un segmento → testo semplice, nessun header.
   - 2+ segmenti → ognuno preceduto da `— <machine> —`, ordinati per nome macchina
     (ordine già garantito da `merge_note`, ma riordiniamo qui per non dipendere
     dalla costruzione esterna del `Note`).

4. **`reconcile(mine: &[Note], their_digests: &[(String, NoteDigest)]) -> (Vec<String>, Vec<Note>)`**:
   confronta il nostro archivio (`mine`) con i digest ricevuti dal peer
   (`their_digests`) e decide lo scambio minimo:
   - **`want`** (Vec<String>): ID di note che il peer ha (per digest) e che noi
     mancano, o abbiamo con digest diverso — dobbiamo chiederne il contenuto pieno.
   - **`push`** (Vec<Note>): note NOSTRE che il peer non conosce, o conosce con
     digest diverso — le mandiamo intere per sincronizzare.
   - Nota nota da entrambi i lati con LO STESSO digest → non scambiata (già allineata).

**Proprietà verificate dai test (6 test):**
- `render_body_single_segment_has_no_header`: singolo segmento → testo nudo.
- `render_body_multiple_segments_has_headers_in_machine_order`: 2+ segmenti →
  header "— <machine> —" ordinati per machine lessicograficamente.
- `reconcile_wants_note_unknown_locally`: nota nel digest peer ma assente
  localmente → finisce in `want`.
- `reconcile_pushes_note_unknown_to_peer`: nota locale assente dal peer →
  finisce in `push`.
- `reconcile_covers_stale_tombstone_case` (chiave): la nostra copia è `deleted:true`,
  il digest del peer mostra ancora `deleted:false` — finisce in ENTRAMBI `want` e
  `push` (vogliamo il loro stato per confronto, e spingiamo il nostro tombstone
  perché lo marchi cancellato anche lui). Questo copre il caso gemello descritto
  dall'utente in fase di brainstorm: nota cancellata mentre offline.
- `reconcile_skips_notes_with_identical_digest`: stesso digest su entrambi i lati
  → nessuno scambio.

**Verifiche:** tutti i 6 test passano; nessuna regressione nei 799 test
complessivi di orchestrator (796 precedenti + 6 nuovi - 3 non affetti dal filtro).

**Design spec:** `Docs/superpowers/specs/2026-07-29-library-notes-design.md`
§5 (digest a confronto deterministico).

---

## Modello dati e merge CRDT per Blocco note (v0.40.55)

**Nuovo modulo `notes`** (Slice 1 di "Blocco note"): implementa il modello dati e
la logica CRDT pura per sincronizzazione delle note testuali in tempo reale su
più macchine via la rete AI Chat esistente.

**Strutture dati:**
- `Segment`: contributo di UNA macchina al corpo di una nota. Chiave `machine`,
  contatore locale monotono `seq` (non condiviso con le altre macchine),
  testo, timestamp di display (non usato per decisioni di merge).
- `Note`: titolo (last-write-wins su orologio di parete, tie-break su nome
  macchina), corpo (vettore di `Segment` ordinato per `machine`), metadata
  creazione, tombstone monotono `deleted`.

**Algoritmo di merge (`merge_note`):**
1. **Corpo:** unione di segmenti per `machine` — per ogni `machine`, se esiste
   in entrambi gli input, vince il segmento con `seq` più alto (candidato per
   aggiornamenti futuri: quello più recente in termini locali). **Proprietà
   verificata:** non genera duplicati dopo merge(merge(a,b), b) — nessun
   crescimento di segmenti.
2. **Titolo:** LWW sull'orologio di parete (`title_touched.1`); a parità,
   vince il nome macchina lessicograficamente più grande (tie-break
   deterministico).
3. **Tombstone:** `deleted |= deleted` — una volta `true`, resta `true` per
   sempre (proprietà monotona della cancellazione).

**Proprietà provate dai test (7 test):**
- Idempotenza: `merge(n, n) == n`
- Commutatività: `merge(a, b) == merge(b, a)`
- Associatività: `merge(merge(a, b), c) == merge(a, merge(b, c))`
- Risoluzione conflitti: seq-max per segmenti, LWW per titolo
- Assenza di duplicati: union senza crescita
- Tombstone irreversibile

**Verifiche:** tutti i test passano; nessuna regressione nei 793 test
complessivi di orchestrator.

**Design spec:** `Docs/superpowers/specs/2026-07-29-library-notes-design.md`
§3-4 (convergenza CRDT indipendente dall'ordine/molteplicità).

---

## Migration: `aichat.json` → `network.json` (v0.40.54)

Aggiunto `pub fn load_or_generate_with_migration(new_path, legacy_path)` in
`crates/orchestrator/src/aichat/config.rs`. La funzione implementa una migrazione
non distruttiva: se `new_path` (`network.json`) manca ma `legacy_path` 
(`aichat.json`, nome storico legato ad AI Chat) esiste, legge il file legacy,
lo scrive al nuovo percorso, e **mantiene il file vecchio su disco**. Se 
`new_path` esiste già, `legacy_path` viene ignorato. Se nessuno esiste,
genera un default e lo scrive a `new_path`.

Utilizzato in `crates/orchestrator/src/main.rs:434-438` per caricare la
configurazione della rete peer al boot, con routing automatico verso il
nuovo nome del file per i prossimi avvii.

Tre test unitari verificano: (1) lettura legacy + scrittura nuova quando
la nuova è assente; (2) ignoranza della legacy quando la nuova esiste;
(3) generazione del default quando nessuno esiste.

---

## Fix: chiudere la finestra AI Chat azzerava il canale WS condiviso (v0.40.53)

**Bug (root cause reale della bustina mai accesa per i messaggi):** dopo
0.45.30 (icona busta) e il tentativo 0.45.31 (emit-await su
`closeWindow()` — miglioramento genuino, ma non la causa), la busta
continuava a non accendersi mai per un `ai_chat_message` arrivato a
finestra chat chiusa, su entrambe le macchine, ripetutamente. Confermata
dall'utente ogni volta: chiusura REALE della finestra (×), non solo in
background.

**Investigazione:** log diagnostici temporanei aggiunti sia lato
`aichat-push.js`/`app.js` (JS) sia lato `aichat/service.rs`
(`tracing::info!` su `Effect::ToUi` e sulla ricezione di `ChatMsg::Say`),
poi rimossi in questo stesso commit dopo la diagnosi. Il log JS ha mostrato
che `ai_chat_message` non arrivava MAI a `handleServerMsg` mentre la chat
era chiusa — non uno scarto del gate (`aichat-push.js`), proprio nessun
arrivo. Il log Rust ha chiuso il caso: `server_tx.is_some()=false` per
TUTTA la finestra temporale tra la chiusura della chat e la sua
riapertura, incluso il messaggio "hello" e le due risposte AI — nessuno
di questi `Effect::ToUi` veniva mai inviato.

**Causa:** `ClientMsg::AiChatClosed` (`ws.rs`, mandato da `app.js` quando
l'evento Tauri `"aichat:closed"` arriva — cioè quando l'utente chiude la
finestra chat) faceva `h.send(ServiceEvent::UiClosed)`. `UiClosed`
(`aichat/service.rs`) fa `self.server_tx = None` — e `server_tx` è il
canale WS **condiviso**, usato da `perform()` per OGNI `Effect::ToUi`:
messaggi, roster, `AiChatReachablePeers` (Condividi), gate 1/2 di
ammissione, la bustina stessa. Chiudere la finestra chat (una webview
secondaria) veniva quindi trattato come se l'INTERA UI (`ui.exe`) si
fosse disconnessa — un'equivalenza sbagliata: la connessione WS reale
resta viva finché `ui.exe` gira, indipendentemente da quale sotto-finestra
Tauri è aperta in un dato momento.

**Fix:** l'arm `ClientMsg::AiChatClosed` non manda più `UiClosed` —
diventa un no-op esplicito e commentato (non ha più nessun altro scopo).
Il VERO teardown della connessione WS (nel blocco di disconnessione reale
del socket, invariato) resta l'unico punto legittimo che azzera
`server_tx`. Lo stato "la finestra chat è aperta?" è già tenuto
correttamente lato frontend (`aiChatGate`, `aichat-push.js`) — il backend
non ha bisogno di una propria copia di quello stato per decidere se può
scrivere sul canale WS.

**Perché nessun test automatico:** il dispatch `ClientMsg → ServiceEvent`
vive dentro la funzione di gestione connessione di `ws.rs`, non estraibile
in una funzione pura testabile senza un vero harness WS — stesso limite
già accettato in questo file per `connection_owns_plugin_sink` (la cui
copertura si ferma deliberatamente all'ispezione per lo stesso motivo,
vedi il commento lì). Verificato dal vivo con la strumentazione diagnostica
sopra (rimossa qui). **Riverifica dal vivo del fix stesso ancora da fare**
con l'utente — la diagnosi è certa (prova diretta nei log), il fix non
ancora ri-testato dopo la correzione.

## Gate 2 di ammissione: replay su riapertura finestra del server (v0.40.52)

**Bug scoperto dal vivo** (2 macchine, usando Library "Condividi" — v0.40.51
sopra): A entra in chat correttamente (gate 1, coperto da replay già
esistente), ma B (il server eletto) non vede MAI la richiesta di ammissione
di A se la propria finestra chat era chiusa quando `RequestAdmission` è
arrivato. Il voto si risolve solo al timeout di 60s
(`ADMISSION_VOTE_TIMEOUT_SECS`, "silenzio = sì"), mai per una decisione
consapevole del server.

**Root cause:** il blocco di replay in `ServiceEvent::SetServerTx` (righe
~835-900) ri-mostra il gate 1 (self come CANDIDATO: `AiChatJoinPrompt`/
`AiChatPending`, basato su `self.self_admission`) ma non il gate 2 (self
come SERVER che deve ancora votare su un altro: `AiChatAdmissionRequest`,
basato su `self.pending_votes`). Gap **esplicitamente documentato** in un
commento del 2026-07-04 ("Resta un gap NON coperto... da valutare in una
slice successiva") — mai chiuso, riscoperto oggi dall'uso reale.

**Fix:** dopo il blocco `match self.self_admission { ... }`, un nuovo loop:

```rust
for pv in self.pending_votes.values() {
    if !pv.votes.contains_key(&self.me.id) {
        effects.push(Effect::ToUi(ServerMsg::AiChatAdmissionRequest {
            candidate: pv.label.clone(),
        }));
    }
}
```

Il guard `!pv.votes.contains_key(&self.me.id)` evita di ri-mostrare un voto
che self ha GIÀ espresso ma che resta aperto in attesa di un ALTRO presente
(scenario con 3+ partecipanti) — ri-proporre la stessa domanda dopo che
self ha già risposto sarebbe confuso, non solo ridondante. Nessun nuovo
`ServerMsg`: `AiChatAdmissionRequest` esiste già (era solo mai ri-emesso).

**Testing:** due test in `mod tests` di `aichat/service.rs`:
1. `set_server_tx_replays_pending_admission_vote_on_ui_reopen` — richiesta
   arrivata a finestra mai connessa, `SetServerTx` successivo deve
   ri-emettere `AiChatAdmissionRequest` per il candidato.
2. `set_server_tx_does_not_replay_admission_vote_already_cast_by_self` —
   self vota (`AdmissionVoteUi`) ma il turno resta aperto (un altro
   presente, aggiunto via `mark_admitted_for_test`, non ha ancora votato):
   nessun replay.

786 test passati (784 preesistenti + questi 2), nessuna regressione.

## Library "Condividi": raggiungibilità, non ammissione chat (v0.40.51)

**Bug:** il dialog "Condividi" della Library mostrava "Nessuna macchina
connessa" nonostante il log confermasse discovery+elezione AI Chat
riuscite ("aichat: peer scoperto...", "aichat: server eletto...").

**Root cause:** il dialog leggeva `ServerMsg::AiChatRoster`
(`last_known_roster` lato Rust), che si aggiorna SOLO su un vero
`Join`/ammissione alla stanza chat (gate umano "vuoi entrare?"). Ma
`request_share` (la funzione che esegue davvero una condivisione,
`aichat/service.rs`) non ha MAI consultato quel roster: risolve il
destinatario da `self.peers` (scoperta UDP, `ServiceEvent::Discovered`,
incondizionata) filtrata da `self.links` (link TCP vivo, stabilito
automaticamente subito dopo l'elezione via `Effect::ConnectTo` — anche
questo senza alcun consenso umano, che riguarda solo la partecipazione
alla CHAT TESTUALE, non la connessione TCP sottostante già presente prima
che l'umano risponda). Il roster di ammissione era quindi un sottoinsieme
troppo stretto di "chi è davvero raggiungibile per una condivisione".

**Fix — nuovo segnale, non un cambiamento di `last_known_roster`:**

- `reachable_peer_labels(&self) -> Vec<String>` (privato,
  `aichat/service.rs`): `self.peers` filtrati da
  `self.links.contains_key`, suffisso `-human` (stesso formato di
  `known_peer_labels()`, usato dal gate 1 chat — che invece non filtra su
  `links`: quella lista è solo testo informativo per l'umano, questa deve
  rispecchiare ESATTAMENTE cosa `request_share` accetterebbe).
- `ServerMsg::AiChatReachablePeers { labels }` (protocol 0.14.7,
  additiva) — emesso in `Effect::ToUi` a ogni punto dove `peers`/`links`
  cambiano davvero (5 siti, individuati leggendo il codice, non
  ipotizzati):
  1. `ServiceEvent::Discovered` — copre la race discovery-vs-link (un
     link TCP può stabilirsi PRIMA che il relativo annuncio UDP venga
     processato, dato che sono meccanismi indipendenti).
  2. `ServiceEvent::PeerGone` — il peer sparito esce dalla lista.
  3. Connessione TCP in uscita riuscita (ramo 4 del loop `run()`, dopo
     `self.links.insert(id, writer_tx)`).
  4. Connessione TCP in entrata accettata (ramo 2 del loop `run()`, dopo
     `self.links.insert(peer_id, writer_tx)`).
  5. `Effect::Disconnect` (eseguito in `perform`, non in `handle_event` —
     scatta es. quando l'umano rifiuta il gate 1 chat): la notifica è
     dentro l'arm dell'effetto stesso, calcolata DOPO le rimozioni da
     `links`/`connected`.
  - Ri-emesso **incondizionatamente** su `SetServerTx` (a differenza del
    roster/storico, dietro `is_empty()`): anche una lista vuota è
    un'informazione valida per la UI (distingue "vuota confermata" da
    "non ancora arrivata").
- **Effetto collaterale necessario, non un extra:** la nuova variante ha
  rotto un match esaustivo pre-esistente su `ServerMsg` in
  `telegram/channel.rs::format_response` — aggiunta al gruppo no-op delle
  altre varianti AI Chat (il canale Telegram non rappresenta la
  raggiungibilità di rete). Ha anche invalidato l'asserzione di un test
  pre-esistente (`..._emits_only_self_label`, rinominato
  `..._plus_empty_reachable_peers`): non era più vero che `SetServerTx`
  emettesse SOLO `AiChatSelf` nel caso base, essendo ora incondizionato
  anche il nuovo segnale.
- **Frontend** (`crates/ui`, v0.45.29): canale Tauri parallelo e separato
  da quello del roster chat (`library:reachable-peers`/
  `library:request-reachable-peers`), `library.js::openShareDialog`
  cambia una riga (fonte dati), `shareTargetList` resta invariata.

**Testing:** `reachable_peer_labels()` testato in isolamento (vuoto,
peer-senza-link escluso, peer-con-link incluso); i 4 siti dentro
`handle_event`/`perform` testati via `Vec<Effect>`/canale finto
(`#[tokio::test]` per il sito async in `perform`); i 2 siti dentro il loop
`run()` (nuovo link in/out) **non hanno test automatico** — richiederebbero
un socket TCP reale — verificati con uno smoke test dal vivo a 2 macchine
(senza mai aprire `/aichat` su nessuna delle due).

Spec: `Docs/superpowers/specs/2026-07-28-library-share-reachable-peers-design.md`.
Piano: `Docs/superpowers/plans/2026-07-28-library-share-reachable-peers.md`.

## `stock_report` apre una finestra Markdown dedicata (v0.40.43)

**Il problema:** il primo smoke test manuale con la UI ha mostrato che
`/markets AAPL` funzionava (report generato correttamente) ma tutto il
Markdown — fondamentale, 5 grafici embedded, option chain, comparative —
arrivava come TESTO SEMPLICE nel pannello del canale (nessun rendering
Markdown lì), illeggibile. L'utente ha correttamente osservato che questo
è incoerente col resto del programma: nmap apre una finestra Markdown
dedicata con pulsante Salva; il canale stesso dovrebbe portare solo un
riassunto breve in testo semplice.

**Il fix:** `PythonMcpToolClient` generalizzato una terza volta (dopo
`tool_defs`/`call_timeout_secs` iniettabili e `join_text_content`). Ogni
tool ora è un `PythonToolSpec { def: ToolDef, report_title: Option<fn(&
serde_json::Value) -> String> }` invece di un semplice `ToolDef`.
`report_title`, se presente, deriva il titolo della finestra dall'INPUT
della chiamata (mai dalla risposta) — stesso schema di
`NmapToolClient::call_scan_tool`, che costruisce il titolo da `target`.

`dispatch()` passa il testo unito (già passato da `join_text_content`) a
`split_report(text, report_title, input)` (funzione pura, testata con
`CallToolResult` finti — nessuno spawn richiesto): se `report_title` è
`Some` E `text` è un JSON `{"summary": ..., "report_markdown": ...}`,
ritorna `output=summary` (all'AI) + `report=Some(ChannelReport{title,
markdown: report_markdown})` (finestra deterministica + Salva in
Library). Se il parsing fallisce (tool che non segue il contratto, es.
`pyping`) o non c'è `report_title` (es. `search_ticker` — nessuna
finestra, resta testo breve in chat), ritorna tutto il testo com'è,
nessun report: fallback sicuro, mai un errore per un tool che non ha
scelto questo contratto opt-in.

Lato Python: `stock_report` ora ritorna quel JSON — `report_markdown` è
il documento completo (`report.build_report`, con i grafici),
`summary` è lo stesso testo passato per `report.strip_chart_images`
(nuovo: sostituisce ogni `![...](data:image/png;base64,...)` con una
nota testuale via regex) — l'AI riceve dati fattuali completi per
ragionare (fondamentale, comparative, option chain) ma non megabyte di
immagini che non può comunque "leggere" utilmente in testo.

`FINANCIAL_MARKETS_SYSTEM_PROMPT` riscritto: prima chiedeva all'AI di
"aggiungere in coda al Markdown ricevuto" le sezioni 5-6 (narrativa +
ipotesi investimento), assumendo che l'AI ripetesse anche le sezioni 1-4
— causa diretta del bug. Ora, come nmap ("non serve che tu lo ripeta"),
l'AI non deve mai ripetere i dati fattuali (il documento si apre da solo);
la sua risposta nel canale è ESCLUSIVAMENTE le sezioni 5-6, testo semplice
senza intestazioni Markdown (il pannello del canale non renderizza
Markdown, solo testo puro).

`python-ping`'s `PythonToolSpec` porta `report_title: None` — nessun
cambio di comportamento.

## `PythonMcpToolClient::dispatch` univa solo il primo blocco di contenuto MCP (v0.40.42)

**Il problema:** `dispatch()` estraeva il testo del risultato con
`content.iter().find_map(|c| c.as_text()...)` — il PRIMO blocco di testo,
scartando gli altri. Innocuo per `pyping`/`stock_report` (un tool che
ritorna una singola stringa produce un solo blocco), ma `search_ticker`
ritorna `list[dict]`: FastMCP (lato Python) serializza ogni elemento della
lista come un blocco di contenuto MCP separato. Con più corrispondenze
(`search_ticker("Novo Nordisk")` → NVO + NONOF, verificato dal vivo), l'AI
riceveva SOLO il primo — l'istruzione del system prompt "chiedi
all'utente quale" non aveva mai una seconda opzione da mostrare. Non
scoperto da nessun test unitario (nessuno aveva ancora esercitato un tool
multi-risultato reale) — trovato interrogando il server MCP direttamente
via script Python, prima di arrivare allo smoke test manuale con la UI.

**Il fix:** estratta `join_text_content(result: &CallToolResult) -> String`
(funzione pura, fuori da `dispatch()`) che concatena TUTTI i blocchi con
`\n` invece di prenderne uno solo. Testabile senza spawnare un processo
reale: `rmcp::model::CallToolResult::success(vec![Content::text(...), ...])`
è un costruttore pubblico, quindi il test unitario costruisce un risultato
MCP finto con N blocchi e verifica la concatenazione direttamente. Per un
singolo blocco il comportamento è identico a prima (nessun separatore
spurio). Due nuovi test `#[ignore]` (spawn reale) fissano la regressione a
livello di integrazione vera: `real_search_ticker_multi_match_roundtrip_
via_venv` (verifica che NVO e NONOF compaiano entrambi) e `real_stock_
report_roundtrip_via_venv` (verifica che il report completo non crashi
più).

## `charts.py` crashava l'intero report su dati OHLC intraday reali (v0.40.42)

**Il problema:** `bars_to_dataframe` (`pytools/financial-markets/charts.py`)
chiamava `pd.to_datetime(df["date"])` senza `utc=True`. yfinance ritorna le
barre H1/H4 con offset UTC diverso tra loro quando la finestra di 60gg
richiesta attraversa un cambio ora legale (es. `-04:00` EDT prima,
`-05:00` EST dopo) — pandas rifiuta di costruire un `DatetimeIndex` con
offset misti nella stessa colonna (`ValueError: Mixed timezones detected`).
`FakeDataSource` (usata da tutti i test unitari) non esercita mai questo
caso: i suoi bar fittizi sono stringhe naive senza offset. Il crash
propagava da `bars_to_dataframe` → `candlestick_png_base64` →
`all_timeframe_charts` → `report.py::_charts_section` → `build_report`,
facendo fallire l'INTERO report (fondamentale, option chain, comparative
compresi) per un ticker qualunque i cui bar H1/H4 attraversassero quel
confine — violando il contratto della design spec §4 ("mai un'eccezione
che fa fallire l'intero report").

**Il fix, in due passi:** `pd.to_datetime(df["date"], utc=True)` risolve
l'ambiguità di parsing normalizzando tutto a UTC; `.dt.tz_convert(
"America/New_York")` riporta poi i timestamp all'ora locale del mercato
USA. Il secondo passo non è cosmetico: senza di esso ogni grafico H1/H4
mostrerebbe l'ora UTC (es. apertura mercato `09:30` locale renderizzata
come `13:30`/`14:30` secondo la stagione) invece dell'ora di
apertura/chiusura reale — un errore silenzioso ma visibile sull'asse del
grafico, proprio sul timeframe che la design spec (§6) usa come base
tecnica per l'ipotesi di investimento a breve termine. Verificato dal vivo
(`yf.Ticker("AAPL").history(...)`) che yfinance ritorna GIÀ barre D/W/M in
America/New_York a mezzanotte: per quei timeframe il giro andata-ritorno
per UTC è un no-op (stesso orario, stessa data) — il fix non introduce
alcuno spostamento di data sui timeframe non-intraday.

## Canale `financial-markets` — `/markets` (v0.40.41)

Voce in `EXTERNAL_TOOL_CHANNELS`: `PythonMcpToolClient::resolve("financial-
markets", "server.py", vec![ToolDef{search_ticker}, ToolDef{stock_report}],
120, "LARE_PYTOOLS_DIR")` — timeout a 120s (contro i 60s di `python-ping`):
`stock_report` fa più round-trip di rete (fondamentale + 5 timeframe OHLC +
option chain + peer + indice) in una sola chiamata. `FINANCIAL_MARKETS_
SYSTEM_PROMPT` istruisce l'AI a risolvere nomi societari via `search_ticker`
prima di chiamare `stock_report`, e a scrivere SEMPRE le sezioni 5-6
(narrativa di trend + ipotesi di investimento con punteggio -5/+5) in coda
al Markdown fattuale ricevuto dal tool — quelle due sezioni non sono mai
prodotte lato Python. Dettagli completi: `Docs/superpowers/specs/2026-07-22-
financial-markets-stock-report-design.md`.

## `PythonMcpToolClient` generico su più tool per canale (v0.40.40)

**Il problema:** `tool_defs()` ritornava sempre `vec![ToolDef{name:"pyping",...}]`
a prescindere dal `domain_id` passato a `resolve()` — un secondo dominio con
tool diversi avrebbe comunque visto "pyping" come proprio (unico) tool
esposto. `dispatch()` validava il nome con un literal `if name != "pyping"`.
Emerso progettando `financial-markets` (due tool: `search_ticker`+
`stock_report`), non da un bug report live.

**Il fix:** `resolve(domain_id, script_relpath, tool_defs: Vec<ToolDef>,
call_timeout_secs: u64, env_override)` — le schema/nomi e il timeout per
`call_tool` sono ora iniettati dal chiamante. `tool_defs()` ritorna il
vettore iniettato; `dispatch()` valida il nome contro quel vettore
(`self.tool_defs.iter().any(|t| t.name == name)`) invece del literal, poi
forwarda a `peer.call_tool()` con `name` dinamico (prima era hardcoded
anche lì). `python-ping` passa la propria unica `ToolDef` (comportamento
bit-per-bit invariato); `financial-markets` (v0.40.41) passa le sue due.

## `PluginHost.windows` — registrato anche dal pump task, non solo da `activate` (v0.40.33)

**Cosa mancava:** `windows: HashMap<u64, String>` (registro `window_id →
plugin_id`, usato da `route_ui_event` per trovare il writer destinatario
di un `UiEvent`) veniva scritto in un solo punto: `activate()`, subito
dopo l'invio riuscito di `Activate{window_id}`. Questo copre la PRIMA
finestra di un plugin — quella aperta da un comando slash, l'unico
percorso che passa da `activate`. Un plugin che apre finestre ULTERIORI
di propria iniziativa (nessuna richiesta esplicita dell'host, solo un
`PluginToHost::ShowWindow` che il plugin decide di mandare quando vuole —
il caso d'uso concreto: una dialog parametri spostabile, requisito del
plugin crittografia pianificato per un task successivo di questo stesso
piano) genera un `window_id` che il pump task traduce correttamente in
`ServerMsg::OpenPluginWindow` (la finestra si apre) ma che `activate` non
ha mai visto — non finisce mai in `windows`. Ogni `UiEvent` successivo per
quella finestra viene scartato in silenzio da `route_ui_event`
(`self.windows.get(&window_id) == None` → `return` immediato, nessun
log). Trovato per ispezione del codice (non da un bug report live), in
preparazione del piano `plugin-crypto`.

**Il fix — spostare la registrazione nel posto che vede TUTTI i
`ShowWindow`, non solo il primo:** il pump task (spawnato da
`spawn_and_handshake`, un `tokio::spawn` separato che possiede in
esclusiva il `reader` del plugin — vedi module doc in cima al file per il
motivo del design a due metà writer/reader) è l'UNICO punto del sistema
che osserva ogni `PluginToHost::ShowWindow` in arrivo dal plugin,
indipendentemente da chi l'ha causato (comando slash via `activate`, o
iniziativa autonoma del plugin). Il fix aggiunge, dentro il loop del
pump, PRIMA di tradurre il messaggio con `plugin_msg_to_server`:

```rust
if let PluginToHost::ShowWindow { window_id, .. } = &pm {
    windows.lock().await.insert(*window_id, pid.clone());
}
```

Un "blind insert" (nessun controllo "se assente") è corretto anche per il
window_id già registrato da `activate` per la prima finestra: stesso
`plugin_id`, overwrite idempotente — non c'è nessun caso in cui
`window_id` appartenga legittimamente a un plugin diverso da chi lo sta
inviando.

**Perché `windows` cambia tipo, non solo contenuto:** il pump gira come
task separato, FUORI dal lock `Arc<Mutex<PluginHost>>` che i chiamanti
(`ws.rs`) tengono per `route_ui_event`/`activate`/etc. Se `windows` fosse
rimasto un campo `HashMap` semplice dentro `PluginHost`, il pump non
avrebbe modo di scriverci senza acquisire lo stesso lock esterno — e
quel lock è tenuto per la durata di operazioni che possono bloccare a
loro volta (es. `writer.send(...).await`), esattamente lo stesso
deadlock reader-vs-sender che ha motivato la separazione `writers`
(host)/`reader` (pump task) nel Task 5. La soluzione è la stessa
categoria: un `Arc<tokio::sync::Mutex<HashMap<u64, String>>>` clonato
(clone di `Arc`, economico) al momento dello spawn del pump, cosicché
host e pump condividano lo stesso registro con un lock indipendente da
quello esterno su `PluginHost`. Tutti i punti che leggevano/scrivevano
`self.windows` sincrono (`activate`, `route_ui_event`, `shutdown`) sono
diventati `self.windows.lock().await....`.

**`forget_window` diventa `async`:** conseguenza diretta del nuovo tipo
(`.lock().await` non è disponibile in un contesto sync). Firma cambiata
da `pub fn forget_window(&mut self, window_id: u64)` a `pub async fn
forget_window(&mut self, window_id: u64)`. Unico chiamante nel codebase:
`ws.rs:365`, dentro `ClientMsg::PluginWindowClosed` — aggiornato a
`plugin_host.lock().await.forget_window(window_id).await;` (due
`.await` in sequenza: il primo sul `Mutex<PluginHost>` esterno per
ottenere `&mut PluginHost`, il secondo sul nuovo `Mutex` interno di
`windows`).

**Perché è una capacità di piattaforma, non un fix specifico del plugin
crittografia:** niente in questo cambiamento menziona crittografia,
Cesare, Vigenère o RSA — è un fix del meccanismo generico
"registrazione finestra plugin" in `PluginHost`. Qualunque plugin futuro
che apra più di una finestra di propria iniziativa (non tramite un
comando slash che passa da `activate`) ne beneficia automaticamente,
senza bisogno di alcuna modifica per-plugin.

**Test (`plugins::host::tests::pump_registers_window_for_unsolicited_show_window`):**
un plugin lazy (`crypto`, non spawnato da `start` — solo trigger
`command: Some("/crypto")`) attivato con `host.activate("crypto", ...)`.
Il `FakeReader` emette due messaggi in sequenza: `Ready` (consumato
dall'handshake) poi `ShowWindow{window_id:42,...}` — una SECONDA finestra
con un window_id che `activate` non ha mai assegnato (`activate` assegna
`window_id:1` per la finestra principale). Il test attende
`rx.recv()` (con timeout 1s) per verificare che il pump abbia già
processato `ShowWindow{42}` e lo abbia tradotto in
`OpenPluginWindow{window_id:42,...}`. Il discriminante vero è la riga
successiva: `host.route_ui_event(42, "apply", Some("3")).await` seguito
dall'assert che il log condiviso (`SentLog`) contenga
`HostToPlugin::UiEvent{window_id:42, element_id:"apply", ...}`. Pre-fix
questo assert falliva (verificato RED con `cargo test -p orchestrator
pump_registers_window_for_unsolicited_show_window`: fallisce esattamente
su questa riga, con messaggio "UiEvent per la finestra non richiesta da
activate deve raggiungere il plugin" — `route_ui_event(42, ...)` trovava
`self.windows.get(&42) == None` e faceva `return` immediato prima del
fix). Post-fix: verde.

**Verifica:** `cargo test -p orchestrator --lib plugins::host` → 11
passed (incluso il nuovo test). `cargo test -p orchestrator` → 737 lib +
16 ws_integration + 2 doctest, tutti verdi — nessuna regressione dal
cambio di tipo di `windows` o dalla firma `async` di `forget_window`.
`cargo build` (root, default-members) → pulito. `cargo clippy -p
orchestrator --all-targets` → 4 warning, tutti PRE-ESISTENTI in
`ai_adapter.rs` e `aichat/service.rs` (file non toccati da questo
cambiamento) — nessun nuovo warning introdotto.

---

## `build.rs` — versioni dei crate sorella nel log di avvio (v0.40.32)

Nuovo `crates/orchestrator/build.rs` (nessun `build.rs` esisteva prima in
questo crate). Legge il `Cargo.toml` di `protocol`, `plugin-protocol`,
`mcp-server`, `mcp-nmap` (path relativi `../<crate>/Cargo.toml`, risolti
rispetto alla working directory del build script, che è sempre la
directory del proprio crate — `crates/orchestrator/`) ed espone la
versione di ciascuno come env var compile-time (`PROTOCOL_VERSION`,
`PLUGIN_PROTOCOL_VERSION`, `MCP_SERVER_VERSION`, `MCP_NMAP_VERSION`) via
`cargo:rustc-env=...`. `main.rs` le consuma con `env!("...")` nella riga
di log di avvio.

**Parsing (`extract_package_version`):** uno scan testuale riga per riga,
non un parser TOML vero — questo workspace non ha alcuna dipendenza
`toml` e non ne vale la pena per quattro letture one-shot di una singola
riga. Il punto delicato è che un `Cargo.toml` contiene tipicamente PIÙ
occorrenze della chiave `version` — non solo quella del pacchetto stesso
in `[package]`, ma anche una per ogni dipendenza che fissa una versione
esplicita (es. `tokio = { version = "1", ... }` dentro `[dependencies]`).
Un match ingenuo su "prima riga che contiene `version =`" prenderebbe la
riga sbagliata in un `Cargo.toml` con `[dependencies]` prima di
`[package]`, o più in generale è fragile all'ordine delle sezioni. La
funzione traccia esplicitamente in quale sezione `[...]` si trova
(`in_package_section: bool`, aggiornato quando incontra una riga che
inizia con `[`) e accetta solo righe `version = "..."` mentre è dentro
`[package]`. Ogni sibling `Cargo.toml` del workspace ha `[package]` come
prima sezione con `version` come una delle prime righe, quindi lo scan si
ferma quasi subito con l'esito corretto.

**`cargo:rerun-if-changed`:** un `println!("cargo:rerun-if-changed={path}")`
per ciascun sibling `Cargo.toml`. Senza questa direttiva, `cargo` non ha
motivo di ri-eseguire `build.rs` quando il file letto (che non fa parte
dei sorgenti di orchestrator) cambia — un bump di versione in
`mcp-nmap/Cargo.toml`, ad esempio, non verrebbe rilevato dalla build
successiva di orchestrator finché qualcos'altro non forzasse comunque una
ricompilazione. Con la direttiva, cargo osserva quei 4 file e ri-esegue
lo script (rigenerando le env var) ogni volta che uno di essi cambia.

**Perché nessun test automatico:** questo è un meccanismo interamente a
compile-time. `env!("PROTOCOL_VERSION")` eccetera sono macro valutate dal
compilatore — se `build.rs` panica (es. path sbagliato, sezione
`[package]` assente) o semplicemente non setta una delle env var, l'intero
crate `orchestrator` non compila, punto. Non esiste un test a runtime che
possa dare una garanzia più forte di questo: un `cargo build -p
orchestrator` riuscito È la prova che il meccanismo funziona
correttamente per la configurazione attuale del workspace. La
verifica effettiva usata per questa versione: `cargo build -p
orchestrator` (successo), esecuzione reale del binario con log osservato
manualmente (`Lare Terminal orchestrator v0.40.32 starting (protocol
v0.14.6 · plugin-protocol v0.2.1 · mcp-server v0.5.0 · mcp-nmap v0.8.2)`
— 4 versioni distinte, nessuna a `0.0.0`/vuota, coerenti con i rispettivi
`Cargo.toml`), e `cargo test -p orchestrator` per confermare che nessun
altro comportamento è stato toccato (solo un nuovo file + gli argomenti
di una riga di log sono cambiati).

`mcp-server`/`mcp-nmap` restano deliberatamente FUORI dalle
`[dependencies]` di `orchestrator/Cargo.toml`: sono processi figli
lanciati via stdio (Contratto B), mai crate linkati. Leggerne la versione
via `build.rs` anziché via `Cargo.toml` `[dependencies]` mantiene questo
confine intatto pur ottenendo un numero di versione sempre accurato (letto
dalla fonte, non copiato a mano). `ui` è escluso di proposito: è un
eseguibile lanciato separatamente dall'utente e logga già la propria
versione per conto suo — includerlo qui non avrebbe senso (l'orchestrator
non lo lancia né lo linka).

---

## Gate locale per-tool — wiring in `ws.rs` (v0.40.15)

Ultimo tassello dell'infrastruttura (`protocol` 0.14.3 → `should_gate` 0.40.13 →
`local_confirm` 0.40.14 → questo commit): `ws.rs` costruisce un
`local_confirm::PendingConfirms` per connessione, lo risolve sull'arm
`ClientMsg::ToolConfirmResponse`, e passa un `LocalUiConfirmer` (invece del
precedente `confirmer: None` letterale) a `core::handle_command` per ogni comando
normale spawnato. Con `SENSITIVE_TOOLS` vuoto il comportamento resta identico a
prima — il seam esiste, pronto per il primo tool reale marcato sensibile.

Fix aggiuntivo (trovato in review di Task 3, applicato qui): `LocalUiConfirmer::
confirm` ora rimuove l'entry da `PendingConfirms` anche sui percorsi di timeout
e di send fallito (prima solo `resolve()` riuscita puliva la mappa) — evita
entry orfane quando il client non risponde o è già disconnesso. `id` viene
clonato prima del `send` (non più mosso) perché resta necessario nei due branch
di cleanup.

## `local_confirm` — `PendingConfirms` + `LocalUiConfirmer` (v0.40.14)

Round-trip di conferma sul canale WS locale già persistente (no polling, a
differenza di Telegram): `LocalUiConfirmer::confirm` genera un id opaco a 128
bit, lo registra in `PendingConfirms` (mappa condivisa `id → oneshot::Sender<bool>`
dietro `Arc<Mutex<...>>`, `Clone` economico), manda `ServerMsg::ToolConfirmRequest`,
e attende il `oneshot::Receiver` con un timeout iniettabile (in produzione 180s,
wiring in `ws.rs` — prossima versione). Il reader loop di `ws.rs` risolverà il
pending su `ClientMsg::ToolConfirmResponse` chiamando `PendingConfirms::resolve`.
`should_gate` consulta `SENSITIVE_TOOLS` (`&[&str]` vuoto per ora — nessun tool
reale marcato sensibile in questa versione).

---

## `ToolConfirmer::should_gate` — gate per-tool (v0.40.13)

Il gate di conferma (ADR-007) era per-canale: `confirmer: Option<&dyn ToolConfirmer>`
decideva "tutto o niente" (`Some` gateizza ogni tool tranne `show_markdown`, `None`
autonomo). Un gate locale per tool sensibili futuri (nmap e successivi) richiede
gateizzare SOLO quei tool, lasciando `run_in_session`/`open_target` autonomi come
oggi. `should_gate(tool_name) -> bool` (default `tool_name != "show_markdown"`)
sposta la decisione dal canale al tool: `TelegramConfirmer` eredita il default
(comportamento invariato); il futuro `LocalUiConfirmer` (Task 3 del piano) lo
sovrascrive con un elenco esplicito (`SENSITIVE_TOOLS`, vuoto per ora). Il filtro
`gated_labels` in `ai_adapter.rs::respond` consulta `confirmer.is_some_and(|c| c.should_gate(name))`
invece del confronto fisso. Design:
`Docs/superpowers/specs/2026-07-15-local-tool-confirm-gate-design.md`.

**Fix post-review (`d85d827`):** la selettività si fermava al PROMPT di conferma
(`gated_labels`) — il blocco vero e proprio in esecuzione (`if !approved { ... }`)
restava applicato a ogni tool non-`show_markdown`, non filtrato da `should_gate`.
Inerte con `SENSITIVE_TOOLS` vuoto (`approved` è sempre `true`), ma avrebbe
bloccato `run_in_session` in un turno che mischia un tool sensibile negato con
un tool non gateizzato. Corretto: il blocco in esecuzione ora richiede
`should_gate(name) && !approved`, non solo `!approved`.

---

Chiude il **Debito #1** ("consenso lato ingresso"): il vecchio consenso pairwise/asimmetrico
(`AiChatJoinRequest`/`AiChatJoinConsent`) è sostituito da un modello a **2 gate** con voto
coordinato dal **server eletto**. Design:
`Docs/superpowers/specs/2026-07-03-aichat-admission-consent-design.md`; piano:
`Docs/superpowers/plans/2026-07-03-aichat-admission.md`. File toccati: `src/aichat/wire.rs`,
`src/aichat/service.rs`, `src/ws.rs`, `tests/aichat_relay_loopback.rs` (call-site).

## Obiettivo

Il consenso esistente (uno-a-uno, asimmetrico) aveva due problemi: (1) il nuovo arrivato
poteva parlare (`Join` mandato subito dopo il consenso locale) **prima** che gli altri
peer avessero effettivamente acconsentito — il suo saluto arrivava e veniva letto come "già
dentro" mentre la conferma altrui era ancora in corso, con testo fuorviante tipo "A vuole
entrare" quando A era già presente; (2) l'accept-loop TCP ammetteva la CONNESSIONE di
qualunque peer scoperto, senza alcun gate d'ingresso reale — il consenso copriva solo il
lato USCITA (`ConnectTo`).

Il nuovo modello introduce due gate distinti e uno stato esplicito che **blocca l'input**
del nuovo arrivato finché il voto non si è risolto:

- **Gate 1** (nuovo arrivato → se stesso): "vuoi entrare in chat con [presenti]?" — decisione
  locale, prima di mandare qualunque richiesta al server.
- **Gate 2** (server → ogni presente ammesso): "ammetti `candidate`? sì/no" — voto coordinato,
  **AND logico con veto**: basta un solo no per rifiutare; il silenzio oltre un timeout di
  **60s** conta come sì (nessun presente può bloccare la stanza restando inerte).

## Design 1 — nuovi messaggi wire (`src/aichat/wire.rs`, Contratto N)

5 nuove varianti `ChatMsg` (peer↔peer, JSON-per-riga su TCP, tag `snake_case`), tutte
additive:

```rust
RequestAdmission { label: String }        // client→server: richiesta dopo il gate 1 locale
AdmissionVoteRequest { candidate: String } // server→ogni presente: "ammetti questo?"
AdmissionVote { candidate: String, accept: bool } // presente→server: il voto
Admitted { label: String }                 // server→tutti: ammesso, segue Roster aggiornato
AdmissionRejected { label: String }        // server→SOLO il candidato: rifiutato (veto)
```

`RequestAdmission.label` è l'**etichetta** del candidato (es. `"cand-human"`), non un
`PeerId`: il server la usa come chiave umano-leggibile per gli inviti di voto e il
broadcast finale — coerente con come `Roster`/`ChatMsg::Say` già trasportano etichette, non
id interni.

Sul confine WS orchestrator↔UI (`protocol`, Contratto A), 5 varianti `ServerMsg` +
3 `ClientMsg` speculari (vedi `protocol` 0.10.0): `AiChatJoinPrompt`/`AiChatAdmissionRequest`/
`AiChatPending`/`AiChatAdmitted`/`AiChatRejected` e `AiChatJoinDecision`/
`AiChatAdmissionVote`/`AiChatRequestAdmission`.

## Design 2 — stato: `SelfAdmission`, `admitted`, `pending_votes`

- **`SelfAdmission`** (enum): `NotJoining` → `Deciding` (gate 1 mostrato) → `Pending`
  (richiesta mandata, in attesa del voto) → `Admitted` | `Rejected`. È lo stato del
  **nuovo arrivato** rispetto al proprio tentativo — letto dalla UI per bloccare l'input
  durante `Pending` (il fix del bug del saluto perso).
- **`admitted: HashSet<PeerId>`** — i client GIÀ ammessi nella stanza. **Distinto** da
  `connected` (link TCP vivo, ma non ancora votato ammesso): un peer appena connesso non ha
  titolo per votare né per parlare finché non è in `admitted`. Questa distinzione è il cuore
  del fix: `start_admission_vote` calcola i "presenti" da `admitted`, non da `connected`.
- **`pending_votes: HashMap<PeerId, PendingVote>`** — il turno di voto in corso per
  candidato: `PendingVote { label, present: HashSet<PeerId>, votes: HashMap<PeerId, bool> }`.

## Design 3 — il cuore: `start_admission_vote` / `record_vote` / `resolve_admit` / `resolve_reject` (SOLO server)

- **`start_admission_vote(candidate, label)`**: chiamato quando il server riceve
  `ChatMsg::RequestAdmission`. Calcola `present` da `self.admitted` (esclude il candidato
  stesso), registra un `PendingVote`, manda `AdmissionVoteRequest` a ogni presente **remoto**
  e avvia `Effect::StartVoteTimeout { candidate, secs: ADMISSION_VOTE_TIMEOUT_SECS }`
  (**60s**, costante di modulo). Se `present` è vuoto (nessun altro presente — il candidato
  è il primo ad arrivare) risolve immediatamente `resolve_admit` (nessuno da consultare).
- **`record_vote(candidate_label, voter, accept)`**: cerca il `PendingVote` per l'etichetta
  (un voto per un'etichetta senza turno in corso è **stale**, ignorato silenziosamente — es.
  un voto arrivato dopo che il timeout ha già risolto). `accept == false` → **veto immediato**,
  risolve subito `resolve_reject` (basta un solo no, non serve aspettare gli altri).
  `accept == true` registrato: se ORA tutti i presenti attesi hanno votato sì, risolve
  `resolve_admit`; altrimenti resta in attesa degli altri voti o del timeout.
- **`resolve_admit(candidate, pv)`**: inserisce il candidato in `self.admitted`, gli manda
  lo storico (`ChatMsg::History` — riusato **invariato** da 0.25.2, la history-dump esistente
  è **preservata all'ammissione**, nessun handoff nuovo da costruire), poi broadcast
  `Admitted { label }` + `Roster` aggiornato a **tutti** i presenti (candidato incluso, che
  lo usa per sbloccare la UI).
- **`resolve_reject(candidate, pv)`**: manda `AdmissionRejected` **solo** al candidato — i
  presenti non ricevono conferma esplicita del rifiuto (solo assenza di aggiornamento
  roster). Rimuove il `PendingVote`.
- **`ServiceEvent::VoteTimeout { candidate }`**: **silenzio-oltre-timeout conta come sì**.
  Se il `PendingVote` esiste ancora al timeout (nessun veto ricevuto nel frattempo), risolve
  `resolve_admit` trattando ogni presente silenzioso come un sì implicito. Se il turno è già
  stato risolto (un veto è arrivato prima del timeout), l'evento trova `pending_votes` senza
  entry per quel candidato → no-op (guard anti-doppia-risoluzione, niente ammissione dopo un
  rifiuto già deciso).

## Design 4 — lato nuovo arrivato: `begin_join_gate` / `request_admission`

- **`begin_join_gate()`**: chiamato al posto del vecchio invio immediato di `ChatMsg::Join`,
  quando questa macchina si connette al server eletto (ramo `ConnectOutcome::Success` di
  `run`). Guardia: no-op se `self_admission != NotJoining` (evita doppio gate su eventi
  ripetuti). Emette `Effect::ToUi(AiChatJoinPrompt { present })` coi presenti calcolati da
  `self.peers` (non solo `connected` — **elezione/presenza ora sempre da tutti i peer
  scoperti**, stessa estensione già fatta per l'elezione late-joiner in 0.25.7) e passa a
  `Deciding`.
- **`request_admission(server_id)`**: chiamato sull'arm `ClientMsg::AiChatJoinDecision
  {accept:true}` (guard: solo se `self_admission == Deciding`). Manda
  `ChatMsg::RequestAdmission` al server e passa a `Pending` — da qui la UI **blocca
  l'input** (`Effect::ToUi(AiChatPending { present })`) fino a `Admitted`/`Rejected`. Questo
  è il fix diretto del bug principale: prima il saluto poteva precedere l'ammissione
  effettiva, ora è strutturalmente impossibile (l'input è bloccato lato service, non solo
  lato UI).
- Su `ChatMsg::Admitted { label }` per la propria etichetta: `self_admission = Admitted`.
  Su `ChatMsg::AdmissionRejected { label }` per la propria etichetta: `self_admission =
  Rejected` (la UI può ri-tentare via `AiChatRequestAdmission` dopo il cooldown lato
  client — vedi `ui` 0.40.0 — `request_admission` viene richiamato tale e quale).
- Un `AiChatJoinDecision{accept:false}` (l'umano rifiuta il gate 1) riporta
  `self_admission = NotJoining` — nessuna connessione/richiesta viene mai mandata.

## Design 5 — `PeerGone` pulisce `admitted` (evita voto-fantasma)

L'arm `PeerGone` rimuove l'id da `self.admitted` **prima** di ogni altra pulizia esistente
(keepalive 0.25.8, generation token 0.27.0). Motivazione: se il peer sparito era uno dei
`present` attesi in un `PendingVote` in corso, la sua sparizione non deve bloccare la
risoluzione all'infinito in attesa di un voto che non arriverà mai — il fix ri-valuta i
`pending_votes` in corso dopo la rimozione: se il peer sparito era l'**ultimo** voto
mancante (tutti gli altri hanno già detto sì), il turno si risolve subito
(`resolve_admit`), invece di restare bloccato fino al `VoteTimeout` da 60s.

## Design 6 — relay-guard (Task 8b, mittente non ammesso)

Il server **ignora** un `ChatMsg::Say` proveniente da un peer che non è in `self.admitted`
(controllo ristretto al ruolo `Server` — solo lì `admitted` ha significato di sorgente di
verità sui presenti reali). Un peer in stato `pending` (connesso via TCP ma non ancora
votato ammesso) non può iniettare messaggi nella stanza prima che il voto si risolva.

**Debito annotato in-code (non risolto in questa slice)**: il guard copre solo il
**mittente**. I **destinatari** del relay non sono ristretti ad `admitted` — un client
connesso ma non ancora ammesso riceve comunque eventuali `Say` relayati (non li genera, ma
li vede). Valutato e scartato per questa slice (superficie diversa, nessun bug di sicurezza
diretto: chi non è admitted non può comunque *parlare*).

## Costanti

- `ADMISSION_VOTE_TIMEOUT_SECS: u64 = 60` — timeout del voto lato server (silenzio = sì).
- Cooldown di 30s per il pulsante "Chiedi di entrare" dopo un rifiuto: **lato UI**
  (`admission.mjs::rerequestState`, `ui` 0.40.0), non nell'orchestrator — coerente col
  commento nel doc di `ClientMsg::AiChatRequestAdmission`: "l'orchestrator non deve fidarsi
  ciecamente del timing del client, ma questa slice non introduce un rate-limit
  server-side" (nessun rate-limit server-side aggiunto qui, deliberatamente).

## Test

495 test lib totali. Copertura dedicata all'ammissione (in `service.rs`): avvio voto dai
soli `admitted` (non `connected`), veto immediato, ammissione dopo tutti i sì, voto stale
ignorato, history-dump mandata all'ammissione, timeout=sì quando nessun veto arriva,
guard anti-doppia-risoluzione dopo un rifiuto già deciso, `PeerGone` durante un voto in
corso (rimozione da `admitted` + ri-valutazione), relay-guard su mittente non ammesso,
transizioni complete di `SelfAdmission` (`NotJoining→Deciding→Pending→Admitted/Rejected`,
incluso il rifiuto del gate 1 che torna a `NotJoining`), non-server ignora
`RequestAdmission` (guardia di ruolo). 2 integration loopback invariati (`aichat_relay_loopback.rs`, `--ignored`).

## Verifica

```
cargo build -p orchestrator          → pulito
cargo test -p orchestrator           → 495 passed (lib)
cargo test -p orchestrator --test aichat_relay_loopback -- --ignored → 2 passed
```

## Resta per l'accettazione live

e2e multi-macchina (checklist completa in `Docs/TESTING-e2e.md` §H): nuovo arrivato vede il
gate 1, i presenti vedono il gate 2, saluto non perso, veto→rifiuto+cooldown+re-request,
timeout→ammesso, sparizione a metà voto non blocca, relay-guard su un pending che prova a
parlare.

---

# Implementation — orchestrator v0.32.0 (AI Chat Slice 2 — auto-partecipazione: giudizio rilevanza + cap turni)

Slice 2 del design "AI Chat — le AI come partecipanti della stanza" (design:
`Docs/superpowers/specs/2026-07-02-aichat-ai-participants-design.md`, §10). Costruita
sopra Slice 1a/1b (0.30.0/0.31.0, sotto in questo stesso file). File toccati:
`src/ai_adapter.rs`, `src/aichat/config.rs`, `src/aichat/service.rs`, `src/main.rs`,
`tests/aichat_relay_loopback.rs` (aggiornamento call-site). **Nessun cambio a
`protocol`/`wire.rs`/`mcp-server`**: `AutoParticipateDone{Some}` riusa integralmente il
percorso `-ai`/`publish_say` già esistente (stesso `ChatMsg::Say` sul filo).

## Obiettivo

A differenza di 1a/1b (l'AI parla SOLO se invocata esplicitamente da un umano — modello
loop-safe **per costruzione**), la Slice 2 fa sì che l'AI possa intervenire
SPONTANEAMENTE sui messaggi normali della stanza, giudicando da sola se ha qualcosa di
genuinamente utile da aggiungere. Questo rimuove la garanzia strutturale di 1a/1b: qui il
controllo del loop diventa un problema di design esplicito, risolto con un cap sui turni
AI consecutivi + guardie sullo stato locale. Modalità **opt-in separata**
(`ai_autoparticipate`, default `false`) — distinta dal flag `ai_participates` di 1b (che
resta "rispondi alle invocazioni remote").

## Design 1 — `AiAdapter::chat_autoparticipate`: giudizio + eventuale risposta

Nuovo metodo sul trait, con **default `None`**:

```rust
async fn chat_autoparticipate(
    &self,
    _transcript: &str,
    _cancel: Option<CancellationToken>,
) -> Option<String> {
    None
}
```

La scelta del default (invece di renderlo un metodo obbligatorio come `chat_reply`) è
deliberata: rende `StubAdapter` deterministicamente silenzioso SENZA bisogno di un
override esplicito (nessuna chiamata HTTP, nessun costo, nessun non-determinismo nei
test — coerente col vincolo del brief "costruire e testare SOLO con `StubAdapter`"), e
protegge ogni altro `impl AiAdapter` esistente nel crate (i fake di test in
`aichat/service.rs`, es. `EmptyReplyAdapter`) dalla rottura di build che un metodo
obbligatorio avrebbe causato. `ClaudeAdapter` sovrascrive con l'implementazione reale:
una `messages.create()` text-only (stesso pattern non-streaming di `chat_reply`, un solo
turno `user` = SOLO il transcript, nessuna `request` separata — qui nessun umano ha
invocato nulla), con un nuovo system prompt dedicato `AUTOPARTICIPATE_SYSTEM_PROMPT`
(diverso da `CHAT_SYSTEM_PROMPT` di `chat_reply`: qui l'AI deve anche saper scegliere il
silenzio, con l'istruzione esplicita di rispondere "SILENCE" quando non ha nulla da
aggiungere). Se il testo (trimmed) è vuoto o `"SILENCE"` (case-insensitive via
`eq_ignore_ascii_case`) → `None`; altrimenti `Some(text)`. Un errore di rete/API mappa a
`None` (loggato con `tracing::warn!`, mai propagato come testo visibile — a differenza di
`chat_reply`, dove l'errore È visibile perché un umano ha chiesto esplicitamente e si
aspetta un riscontro).

## Design 2 — il cuore: `note_room_message` + `maybe_autoparticipate` (design §10.3)

Nuovi campi su `AiChatService`: `ai_autoparticipate: bool`, `consecutive_ai_turns:
usize` (init 0), `last_speaker_label: Option<String>` (init `None`),
`autoparticipate_inflight: bool` (init `false`). Nuova const `MAX_CONSECUTIVE_AI_TURNS:
usize = 4`.

**`note_room_message(&mut self, from_label: &str)`** (privato): aggiorna il contatore —
`-human` azzera, `-ai` incrementa; aggiorna sempre `last_speaker_label`. È il
**choke-point** del conteggio (vedi "il problema del doppio conteggio" sotto).

**`maybe_autoparticipate(&mut self, triggering_from_label: &str) -> Vec<Effect>`**
(privato, PURO): valuta 5 guardie, in ordine —
1. `ai_autoparticipate` è ON;
2. il messaggio non è la mia stessa `-ai` (`triggering_from_label != "<base>-ai"`);
3. l'ultimo a parlare non ero io (`last_speaker_label != Some("<base>-ai")`);
4. `consecutive_ai_turns < MAX_CONSECUTIVE_AI_TURNS` (il CAP — confine ESATTO: `<`, non
   `<=`, quindi il cap sopprime già al valore UGUALE al massimo, non solo "oltre");
5. nessun giudizio già in volo (`!autoparticipate_inflight`).

Se tutte passano: marca `autoparticipate_inflight = true` e ritorna
`[Effect::AutoParticipate { transcript }]` (transcript = `format_transcript(&self.history)`,
funzione pura invariata da 1a). Altrimenti `Vec::new()`.

**Precondizione d'ordine**: `maybe_autoparticipate` va chiamata DOPO `note_room_message`
per lo STESSO messaggio — le guardie 3/4 leggono stato che `note_room_message` ha appena
aggiornato per il messaggio corrente. Tutti i call-site in `handle_event` rispettano
questo ordine.

## Design 3 — il problema del doppio conteggio (e come è evitato)

Ogni riga che entra in `self.history` deve innescare `note_room_message` **esattamente
una volta**. Due famiglie di ingresso nello storico esistevano già prima di questa
slice:
1. I MIEI messaggi (`HumanSay`, `AiReply`, e il nuovo `AutoParticipateDone{Some}`) —
   tutti passano da `publish_say` (già il choke-point SOLID/DRY per l'append + l'eco UI +
   il relay). `note_room_message` è stato aggiunto DENTRO `publish_say`, subito dopo
   `self.history.push(...)` — un solo punto per tutti e tre i chiamanti.
2. I messaggi di un ALTRO peer (`ServiceEvent::PeerMsg { msg: ChatMsg::Say, .. }`) —
   questo arm ha il proprio `self.history.push(...)` INDIPENDENTE (non passa da
   `publish_say`, che pubblicherebbe di nuovo verso la rete un messaggio già ricevuto
   dalla rete). `note_room_message` è stato aggiunto qui, esplicitamente, subito dopo il
   suo `history.push`.

Questi due punti sono mutuamente esclusivi per costruzione (un messaggio o è "mio"
— arriva da `HumanSay`/`AiReply`/`AutoParticipateDone` — o è "di un peer" — arriva da
`PeerMsg::Say` — mai entrambi), quindi ogni riga di storico attiva `note_room_message`
da esattamente uno dei due punti: mai zero, mai due.

## Design 4 — wiring nei tre punti di ingresso messaggio

- **Arm `HumanSay`**: dopo `publish_say` (che ha già chiamato `note_room_message` per il
  MIO `-human`) e l'eventuale `Effect::InvokeLocalAi` di 1a/1b, chiama
  `maybe_autoparticipate(&mio_label_human)` e ne accoda gli effetti.
- **Arm `PeerMsg::Say`**: chiama `note_room_message(from_label)` subito dopo il proprio
  `history.push`; poi, ACCANTO al blocco di rilevazione Slice 1b (che resta gated su
  `from_label.ends_with("-human")` — guardia loop dell'INVOCAZIONE esplicita), chiama
  `maybe_autoparticipate(from_label)` **senza** quel filtro: un `-ai` di un ALTRO peer
  PUÒ far scattare la mia auto-partecipazione (è la conversazione AI↔AI spontanea che
  questa slice abilita), bounded dalle guardie 2-5 (in particolare il cap), non da un
  filtro sul mittente.
- **Arm `AiReply`**: INVARIATO — nessuna chiamata a `maybe_autoparticipate`. Anche se la
  aggiungessimo, la guardia 2 la escluderebbe sempre (è il MIO `-ai`); ometterla evita
  solo una valutazione ridondante.
- **Nuovo arm `AutoParticipateDone { reply }`**: azzera `autoparticipate_inflight`
  INCONDIZIONATAMENTE (prima di guardare `reply`), poi — solo se `Some(text)` con
  `text.trim()` non vuoto — pubblica come `Say` da `"<label_base>-ai"` via `publish_say`
  (stesso percorso di `AiReply`).

## Design 4b — Slice 2b (0.32.1): sopprimi il "double-fire"

Raffinamento successivo alla 0.32.0. Con un'invocazione esplicita (`@ai`/`@all`/
`@<mio-label>-ai`) E `ai_autoparticipate` ON, gli arm `HumanSay`/`PeerMsg::Say`
producevano SIA `Effect::InvokeLocalAi` SIA `Effect::AutoParticipate` per lo stesso
messaggio — due turni AI, costo doppio (bounded dal cap, ma ridondante). Fix (decisione
esplicita: l'invocazione sopprime l'auto sullo stesso messaggio): in ENTRAMBI gli arm,
subito prima della chiamata a `maybe_autoparticipate`, un check
`effects.iter().any(|e| matches!(e, Effect::InvokeLocalAi { .. }))` decide se chiamarla
affatto — non solo se scartarne l'esito, perché `maybe_autoparticipate` marca
`autoparticipate_inflight = true` come side-effect quando scatta, e non ha senso marcare
l'inflight per un giudizio mai richiesto.

Conseguenze per i tre casi rilevanti:
- Messaggio NORMALE (nessun `@`): nessun `InvokeLocalAi` prodotto → il check è `false` →
  `maybe_autoparticipate` viene chiamata come prima (invariato).
- `@ai`/`@all`/`@<mio-label>-ai` (mi targeta): `InvokeLocalAi` prodotto → il check è
  `true` → `maybe_autoparticipate` NON viene chiamata su questo messaggio.
- `@<altro-label>-ai` (non mi targeta, da `PeerMsg::Say`): nessun `InvokeLocalAi` PER ME
  → il check resta `false` → l'auto-partecipazione può ancora scattare (comportamento
  scelto: non sto rispondendo a quell'invocazione, potrei avere comunque qualcosa da
  aggiungere alla conversazione — non è un residuo del bug, è il design).

Le 5 guardie di `maybe_autoparticipate` (§Design 2) restano invariate: questo fix agisce
SOLO sul "se chiamarla", non sul suo interno.

## Design 5 — `perform`: perché manda SEMPRE l'evento (differenza da `InvokeLocalAi`)

`perform` per `Effect::AutoParticipate` spawna un task shutdown-aware che chiama
`ai_adapter.chat_autoparticipate(&transcript, Some(shutdown))` e manda **SEMPRE**
`ServiceEvent::AutoParticipateDone { reply }` sull'inbox — anche quando `reply` è `None`.
Questo è DELIBERATAMENTE diverso da `Effect::InvokeLocalAi`, dove `perform` scarta una
risposta vuota in silenzio (`if text.trim().is_empty() { return; }`) PRIMA di mandare
`AiReply`. Il motivo: in `InvokeLocalAi` un testo vuoto è un caso degenere raro (l'umano
ha chiesto, si aspettava una risposta); qui `None`/silenzio è l'ESITO PIÙ COMUNE del
giudizio di rilevanza. Se `perform` applicasse la stessa guardia "scarta e non mandare
nulla", `autoparticipate_inflight` (impostato a `true` da `maybe_autoparticipate`)
resterebbe bloccato per sempre dopo il PRIMO silenzio — nessuna futura
auto-partecipazione scatterebbe mai più (guardia 5 sempre falsa). Il "decidi se
pubblicare" resta interamente nell'arm `AutoParticipateDone` di `handle_event`
(dove vive anche l'azzeramento incondizionato di `inflight`); `perform` è un puro
esecutore che non decide nulla, come da principio già stabilito per `InvokeLocalAi`.

## Design 6 — flag `ai_autoparticipate`: opt-in, default `false`

Nuovo campo `ai_autoparticipate: bool` su `AiChatConfig` (`config.rs`,
`#[serde(default)]` — bool → `false`, un `aichat.json` scritto prima che il campo
esistesse deserializza comunque) e su `AiChatSettings` lato UI
(`crates/ui/src-tauri/src/aichat_settings.rs`, letto/scritto con lo stesso pattern
infallibile degli altri campi, `unwrap_or(false)`). `main.rs` lo legge da
`cfg.ai_autoparticipate` e lo passa come 4° parametro a `AiChatService::new`. Come gli
altri campi di `aichat.json`, NON è "live". **Default `false`** — a DIFFERENZA di
`ai_participates` (default `true`): l'auto-partecipazione è più aggressiva/costosa
(rimuove la loop-safety-per-costruzione), quindi opt-in esplicito.

## Costruttori di test

`AiChatService::new(me, ai_adapter, ai_participates, ai_autoparticipate)` guadagna un
quarto parametro. `new_for_test(me)`/`new_for_test_with_participation(me,
ai_participates)` (già esistenti) passano internamente `false` come quarto argomento
(comportamento invariato per i ~90 call-site esistenti). Nuovo `#[cfg(test)]
new_for_test_with_autoparticipate(me, ai_autoparticipate)` per i test che esercitano il
flag/le guardie. I 3 costruttori `AiChatService::new` fuori dal `cfg(test)` del lib crate
(`tests/aichat_relay_loopback.rs`) aggiornati esplicitamente al quarto parametro `false`.

## Come si è ottenuto il RED

Stesso principio già documentato per 1a/1b: un errore di **compilazione** per un
metodo/campo/variante non ancora esistente è la fase RED per i cambi strutturali. In
ordine:
1. `ai_adapter.rs`: 3 test (`stub_autoparticipate_is_silent`,
   `claude_autoparticipate_silence_maps_to_none`,
   `claude_autoparticipate_text_maps_to_some`) scritti PRIMA che il trait esponesse
   `chat_autoparticipate` → RED (`E0599`, "no method named `chat_autoparticipate`") →
   aggiunto il metodo (default `None` + override `ClaudeAdapter`) → GREEN.
2. `aichat/config.rs`: il campo `ai_autoparticipate` aggiunto alla struct PRIMA di
   aggiornare `impl Default` → RED (`E0063`, "missing field `ai_autoparticipate` in
   initializer") → aggiunto a `Default` → GREEN (con i 2 nuovi test
   `default_ai_autoparticipate_is_false`/`missing_ai_autoparticipate_field_deserializes_as_false`
   scritti prima, verificati falliti per lo stesso motivo).
3. `aichat/service.rs`: `Effect::AutoParticipate`/`ServiceEvent::AutoParticipateDone`
   aggiunti alle enum PRIMA di gestirli in `perform`/`handle_event` → RED (`E0004`,
   "non-exhaustive patterns") → aggiunti gli arm → GREEN. I 4 call-site di
   `AiChatService::new` aggiornati (RED `E0061`, "this function takes 4 arguments but 3
   were supplied" → GREEN). Le 19 asserzioni sulle 5 guardie di `maybe_autoparticipate` +
   il contatore + `AutoParticipateDone` + `perform` sono state scritte e verificate contro
   l'implementazione unita (le guardie sono state progettate insieme, come un'unica
   funzione pura con 5 condizioni — non 5 metodi separati — quindi la granularità naturale
   del RED è "il metodo non esiste ancora", poi GREEN su tutte le 5 guardie insieme,
   verificato caso per caso incluso il confine esatto del cap `MAX-1` vs `MAX`).

## Test

19 nuovi in `aichat/service.rs`:
- `note_room_message_counts_ai_turns_and_resets_on_human` — il contatore.
- `maybe_autoparticipate_triggers_when_flag_on` /
  `maybe_autoparticipate_does_not_trigger_when_flag_off` — guardia 1.
- `maybe_autoparticipate_never_triggers_from_own_ai_message` — guardia 2.
- `maybe_autoparticipate_skips_when_last_speaker_was_own_ai` — guardia 3.
- `maybe_autoparticipate_suppressed_at_cap` /
  `maybe_autoparticipate_still_triggers_just_below_cap` /
  `maybe_autoparticipate_reopens_after_human_message_resets_cap` — guardia 4 (il cap),
  confine esatto (`MAX` sopprime, `MAX-1` no) + riapertura dopo un `-human`.
- `maybe_autoparticipate_skips_when_already_inflight` /
  `maybe_autoparticipate_marks_inflight_on_trigger` — guardia 5.
- `human_say_triggers_autoparticipate_when_flag_on` /
  `human_say_does_not_trigger_autoparticipate_when_flag_off` — integrazione end-to-end
  pura via `handle_event(HumanSay(...))`.
- `autoparticipate_done_some_publishes_ai_reply_and_clears_inflight` /
  `autoparticipate_done_none_clears_inflight_without_publishing` /
  `autoparticipate_done_blank_text_clears_inflight_without_publishing` — l'arm
  `AutoParticipateDone`, inclusa la guardia "testo vuoto/spazi = silenzio".
- `perform_autoparticipate_sends_done_event_even_for_stub_none` — verifica il punto
  critico: `perform` manda l'evento ANCHE per `None` (con lo Stub).
- `perform_autoparticipate_sends_done_event_with_some_text` — idem con un adapter di
  test (`AutoParticipateRespondsAdapter`) che risponde davvero.
- `peer_say_from_other_ai_can_trigger_my_autoparticipate` /
  `peer_say_from_own_ai_label_never_triggers_autoparticipate` — il `PeerMsg::Say` da
  un'AI ALTRUI può innescare, dal mio `-ai` mai.

3 nuovi in `ai_adapter.rs` (vedi Design 1); 2 nuovi in `aichat/config.rs` (vedi Design 6).

### Slice 2b (0.32.1) — 4 nuovi test in `aichat/service.rs`

- `human_say_at_ai_with_auto_on_does_not_double_fire` — RED prima del fix (senza il
  check, `AutoParticipate` era presente accanto a `InvokeLocalAi`); GREEN dopo.
- `human_say_plain_with_auto_on_still_autoparticipates` — invariante: un messaggio
  normale continua ad auto-partecipare.
- `peer_say_at_all_with_auto_on_does_not_double_fire` — stesso fix, lato `PeerMsg::Say`
  (RED prima del fix, stesso motivo).
- `peer_say_at_other_label_with_auto_on_can_still_autoparticipate` — documenta il
  comportamento scelto per `@<altro-label>-ai`: nessun `InvokeLocalAi` per me,
  l'auto-partecipazione resta possibile (non era RED: già vero prima del fix, qui solo
  per fissare il comportamento come specifica leggibile).

## Verifica

```
cargo test -p orchestrator
  → lib: 470 passed, 0 failed, 3 ignored (baseline pre-slice: 470-19-3=448 in
    aichat::service + ai_adapter + aichat::config; verificato incrementale per modulo)
  → ws_integration: 13 passed
  → doc-tests: 1 passed, 1 ignored

cargo test -p orchestrator --test aichat_relay_loopback -- --ignored
  → 2 passed (client_human_say_reaches_server_ui_over_tcp,
    silent_peer_is_declared_gone_after_dead_threshold) — i costruttori aggiornati al
    quarto parametro non alterano il comportamento del relay TCP/keepalive reale.

cargo clippy -p orchestrator --all-targets
  → 2 warning PRE-ESISTENTI (ai_adapter.rs:241 map_or, ws.rs:491 explicit_auto_deref —
    verificato via `git stash`/clippy sul baseline: stessi 2 warning, stesse righe,
    nessun file toccato da questa slice contiene quelle righe se non per shift di
    numero riga) — nessun warning NUOVO introdotto.

cargo build -p orchestrator → pulito.
```

`cargo fmt` deliberatamente NON eseguito su tutto il crate (drift preesistente, come
1a/1b).

### Slice 2b (0.32.1) — verifica

```
cargo test -p orchestrator --lib
  → 474 passed, 0 failed, 3 ignored (470 + 4 nuovi test di questa slice).
  RED confermato prima del fix: human_say_at_ai_with_auto_on_does_not_double_fire e
  peer_say_at_all_with_auto_on_does_not_double_fire fallivano entrambi con lo stesso
  Vec<Effect> che conteneva sia InvokeLocalAi sia AutoParticipate.

cargo test -p orchestrator --test aichat_relay_loopback -- --ignored
  → 2 passed (invariato).

cargo clippy -p orchestrator --all-targets
  → stessi 2 warning pre-esistenti (ai_adapter.rs:241, ws.rs:491) — nessuno in
    service.rs, l'unico file toccato da questa slice — nessun warning NUOVO.

cargo build -p orchestrator → pulito.
```

## Osservazione non risolta (per il supervisore)

Con `@ai`/`@all` (invocazione esplicita) E `ai_autoparticipate` entrambi ON sulla stessa
macchina, un singolo messaggio umano PUÒ produrre sia `Effect::InvokeLocalAi` sia
`Effect::AutoParticipate` — due turni AI per un solo messaggio (bounded dal cap, non una
rottura della loop-safety, ma un costo doppio non ovvio). Il design §10 non lo esclude
esplicitamente e il brief di questa slice non prevedeva un guard dedicato: implementato
fedele alle istruzioni ricevute, nessuna guardia aggiunta di iniziativa. Segnalata anche
nel CHANGELOG e nel report al supervisore.

## Resta per l'accettazione live

Come 1a/1b: nessuna prova end-to-end con AI reale in questa slice (costruita e verificata
SOLO con `StubAdapter`, per vincolo esplicito del brief — l'auto-partecipazione amplifica
ogni bug in un loop potenzialmente costoso). Da eseguire dal supervisore SOLO DOPO l'e2e
live di 1a/1b (§9.7): 2 macchine, `ai_autoparticipate` ON su entrambe, un umano lancia un
argomento (senza `@`) → le AI intervengono spontaneamente; verificare che dopo ~cap turni
AI si fermino finché un umano non riparla; con flag OFF, nessun intervento spontaneo.
Tenere d'occhio il costo (chiamate API reali).

---

# Implementation — orchestrator v0.31.0 (AI Chat Slice 1b — @all / @<label>-ai + ai_participates)

Slice 1b del design "AI Chat — le AI come partecipanti della stanza" (design:
`Docs/superpowers/specs/2026-07-02-aichat-ai-participants-design.md`, §9 "dettaglio di
design build-ready"). Costruito sopra Slice 1a (0.30.0, sotto in questo stesso file).
File toccati: `src/aichat/service.rs`, `src/aichat/config.rs`, `src/main.rs`,
`tests/aichat_relay_loopback.rs`. **Nessun cambio a `protocol`/`wire.rs`/`mcp-server`**
(§9.3 del design: la propagazione riusa integralmente `ChatMsg::Say` già esistente).

## Obiettivo

Estendere l'invocazione dell'AI (Slice 1a: solo `@ai` = la propria AI) a due nuove
forme: `@all` (tutte le AI presenti nella stanza) e `@<label>-ai` (l'AI di UNA macchina
specifica, anche remota). Ogni macchina target risponde con la propria AI locale — MA
solo se un umano REMOTO la invoca **e** il flag locale `ai_participates` lo consente
(autonomia della macchina: il summoner propone, la macchina target decide). L'invocazione
della PROPRIA macchina agisce sempre (l'atto di scrivere nella propria finestra è già il
consenso).

## Design 1 — `extract_ai_invocation`: da `Option<String>` a `Option<Invocation>`

La funzione pura (module-level, nessun `self`) cambia tipo di ritorno per portare anche
il DESTINATARIO dell'invocazione, non solo la richiesta:

```rust
enum InvokeTarget { Own, Label(String), All }
struct Invocation { target: InvokeTarget, request: String }
```

Riconosce tre forme, tutte case-insensitive SOLO sul "marcatore" (il testo prima del
primo spazio bianco):
- `@ai` → `Own` (Slice 1a, invariato).
- `@all` → `All` (nuovo).
- `@<label>-ai` → `Label("<label>")`, con il `label` estratto a CASE ORIGINALE (nuovo).

**Come si ottiene il confine di parola senza un controllo separato.** La 1a aveva un
controllo esplicito ("dopo `@ai` deve esserci uno spazio o niente, altrimenti è
`@aiuto`"). La 1b lo ottiene per costruzione dalla tokenizzazione: `trimmed.find
(char::is_whitespace)` isola il primo token (il "marcatore"); tutto ciò che segue
(trimmato) è la richiesta. `"@aiuto qualcosa"` produce il token `"@aiuto"`, che non
combacia con nessuna delle tre forme riconosciute (non è `"@ai"`, non è `"@all"`, non
finisce in `"-ai"` dopo aver tolto la `@` iniziale) → `None` — stesso comportamento di
1a, ottenuto con meno codice.

**Byte-safety dell'estrazione del label a case originale.** Il confronto case-insensitive
usa `token.to_ascii_lowercase()` (trasforma SOLO i byte ASCII A-Z, mai i byte UTF-8
multi-byte di un eventuale label non-ASCII — preserva ESATTAMENTE la lunghezza in byte
del token originale). Il label a case ORIGINALE (per il confronto con `label_base`, che è
case-sensitive) è ricalcolato con `strip_prefix('@')`/`strip_suffix("-ai")` sulla versione
lowercased (sicuro, nessun panico su un confine di carattere) e poi riletto dal token
ORIGINALE usando GLI STESSI indici di byte — sicuro proprio perché la trasformazione
ASCII-only preserva la lunghezza.

`@-ai` (label vuoto) ritorna `None`: nessun `label_base` valido è vuoto (vietato da
`validate()` in `crates/ui/src-tauri/src/aichat_settings.rs`), quindi non è un
falso-positivo su un caso limite non gestito ma un rifiuto esplicito.

## Design 2 — due punti di rilevazione, un solo `Effect`

`Effect::InvokeLocalAi`/`ServiceEvent::AiReply` (Slice 1a) sono RIUSATI senza modifiche:
1b cambia solo COME/QUANDO vengono emessi.

- **`ServiceEvent::HumanSay` (il MIO umano)**: dopo `publish_say`, se
  `extract_ai_invocation` rileva un'invocazione che mi riguarda (`Own`/`All` sempre;
  `Label(l)` solo se `l == self.me.label_base`), emette `Effect::InvokeLocalAi` —
  **sempre**, nessun gate sul flag `ai_participates`. `Label(altro)` non emette nulla: il
  `Say` relayato raggiungerà la macchina giusta, che reagirà dal ramo `PeerMsg::Say` sotto.
- **`ServiceEvent::PeerMsg { msg: ChatMsg::Say { from_label, text }, .. }` (un umano
  REMOTO)**: il relay/forward esistenti restano INVARIATI (l'invocazione è rilevata
  ACCANTO, non al posto). SE `from_label.ends_with("-human")` (guardia loop, vedi sotto),
  rileva l'invocazione: `All` o `Label(l)` con `l == self.me.label_base` invocano la mia
  AI **solo se `self.ai_participates == true`**; `Own` è IGNORATO (è lo shorthand "propria
  AI" del MITTENTE, non un'invocazione verso di me); `Label(altro)` è ignorato (non sono
  il target).

## Design 3 — la guardia loop (load-bearing)

La rilevazione su `PeerMsg::Say` gira SOLO se `from_label` finisce in `"-human"`. Una
risposta di un'AI (`-ai`) relayata — anche se il suo testo contenesse letteralmente
`"@all"` (es. l'AI lo cita) — non produce MAI `InvokeLocalAi`: un'AI non può innescarne
un'altra. Questo estende alla topologia cross-macchina lo stesso principio "loop-safe per
costruzione" di 1a (dove bastava che l'arm `AiReply` non chiamasse mai
`extract_ai_invocation`): ora che l'invocazione può viaggiare in rete (`Say` relayato),
la guardia deve stare sul MITTENTE del messaggio, non solo sul tipo di evento locale. Test
dedicato (esplicitamente commentato come "load-bearing" nel codice):
`peer_say_from_ai_never_invokes_even_with_at_all_in_text`.

## Design 4 — il flag `ai_participates`

Nuovo campo `ai_participates: bool` su `AiChatConfig` (`config.rs`,
`#[serde(default = "default_true")]` — un `aichat.json` scritto PRIMA che il campo
esistesse deserializza comunque, assumendo `true`) e su `AiChatSettings` lato UI
(`crates/ui/src-tauri/src/aichat_settings.rs`, letto/scritto con lo stesso pattern
infallibile degli altri 3 campi). `main.rs` lo legge da `cfg.ai_participates` e lo passa
al terzo parametro di `AiChatService::new`. Come gli altri campi di `aichat.json`, NON è
"live": letto una volta all'avvio dell'orchestrator (il tab `/config` lo segnala già con
la nota esistente "Le modifiche all'AI Chat richiedono il riavvio dell'orchestrator").

Default `true` (partecipativo): coerente con "spazio a tutte le voci" (design §9.4);
opt-out facile dal tab `/config` (nuova checkbox "La mia AI partecipa...").

## Costruttori di test

`AiChatService::new(me, ai_adapter, ai_participates)` guadagna un terzo parametro.
`new_for_test(me)` (già esistente da 1a) resta con la stessa firma — ora internamente
passa `true` come terzo argomento a `new`, così i ~50 call-site esistenti nei test
restano invariati (comportamento di default "partecipativo", coerente col default di
`AiChatConfig`). Nuovo `#[cfg(test)] new_for_test_with_participation(me, ai_participates)`
per i (pochi) test che esercitano il flag OFF, evitando di dover passare
`Arc::new(StubAdapter)` esplicito in quei call-site. I 3 costruttori `AiChatService::new`
fuori dal `cfg(test)` del lib crate (`tests/aichat_relay_loopback.rs`, che non ha accesso
a `new_for_test`) sono stati aggiornati esplicitamente al terzo parametro `true`.

## Come si è ottenuto il RED

Stesso principio già documentato per 1a: un errore di **compilazione** per un tipo/
parametro non ancora esistente è la fase RED quando il cambio è strutturale. I nuovi test
(estensione di `extract_ai_invocation`, i due punti di rilevazione, la guardia loop, il
flag) sono stati scritti PRIMA usando `Invocation`/`InvokeTarget`/
`new_for_test_with_participation`/il terzo parametro di `new` — nessuno dei quali esisteva
ancora → `cargo test -p orchestrator --lib aichat::service` falliva con 16 errori di
compilazione (`E0422`/`E0433`/`E0599`). Implementati i due tipi, il nuovo corpo di
`extract_ai_invocation`, il campo/parametro `ai_participates`, i due arm estesi → GREEN
(118/118 test del modulo `aichat`). Stesso schema per `config.rs`: 2 test scritti PRIMA
(`default_ai_participates_is_true`, `missing_field_deserializes_as_true`) → RED
(`E0609`, campo inesistente) → aggiunto il campo con `#[serde(default = "default_true")]`
→ GREEN.

## Test

- `extract_ai_invocation_detects_all`, `extract_ai_invocation_detects_label`,
  `extract_ai_invocation_rejects_empty_label` — le 3 nuove forme, in isolamento totale
  (nessun `AiChatService` costruito). Le 4 esistenti di 1a adattate al nuovo tipo
  `Invocation` (comportamento invariato, solo il tipo di ritorno cambia).
- `human_say_at_all_invokes_local_ai_regardless_of_flag` — `@all` dal PROPRIO umano
  invoca SEMPRE, anche con `ai_participates = false` (costruito con
  `new_for_test_with_participation`).
- `human_say_at_own_label_ai_invokes_local_ai` / `human_say_at_other_label_ai_does_not_invoke_local_ai`
  — `@<mio-label>-ai` invoca, `@<altro-label>-ai` no (dal PROPRIO umano).
- `peer_say_from_human_at_all_invokes_when_participates_on` /
  `peer_say_from_human_at_all_does_not_invoke_when_participates_off` — `@all` da un
  `PeerMsg::Say` con `from_label` `-human`, gated dal flag.
- `peer_say_from_ai_never_invokes_even_with_at_all_in_text` — GUARDIA LOOP: stesso
  scenario ma `from_label` è `-ai` → mai `InvokeLocalAi`, indipendentemente dal contenuto
  del testo.
- `peer_say_from_human_at_ai_own_is_ignored` — `@ai` (shorthand Own) da un umano remoto
  non riguarda me.
- `peer_say_from_human_at_my_label_ai_invokes_when_participates_on` /
  `peer_say_from_human_at_other_label_ai_does_not_invoke` — `@<label>-ai` da remoto:
  invoca solo se il label combacia col mio.
- `default_ai_participates_is_true` / `missing_field_deserializes_as_true` — il default e
  la tolleranza di migrazione di `AiChatConfig::ai_participates`.

## Verifica

`cargo test -p orchestrator`: 446 test lib (432 su 0.30.0 + 14 nuovi: 12 in
`aichat/service.rs`, 2 in `aichat/config.rs` — verificato con `git stash`/`cargo test`
sul baseline pre-slice: 432 → 446), 0 failed, 3 ignored (invariato). I 4 test
esistenti di `extract_ai_invocation` (1a) adattati al nuovo tipo `Invocation` senza
aumentare il conteggio (stesso comportamento, tipo di ritorno diverso).
`cargo test -p orchestrator --test aichat_relay_loopback -- --ignored`: 2/2 (i costruttori
aggiornati al terzo parametro non alterano il comportamento del relay TCP reale su
loopback). `cargo clippy -p orchestrator --all-targets`: 2 warning preesistenti
(`ai_adapter.rs:207`, `ws.rs:491` — verificato via `git diff --stat` che questi file non
sono toccati da questa slice), nessuno nuovo. `cargo build -p orchestrator`: pulito.
`cargo fmt` deliberatamente NON eseguito su tutto il crate (drift preesistente, come 1a).

## Resta per l'accettazione live

Come 1a, nessuna prova end-to-end multi-macchina con AI reale in questa slice (fuori
portata dei test automatici, che usano `StubAdapter`). Serve: 2+ orchestrator con
`aichat.json` (`enabled: true`, `ai_participates` ON su almeno due) e
`ANTHROPIC_API_KEY`, `@all commenta X` da una macchina → ogni AI partecipante risponde
`<suo-label>-ai` nella stanza; con `ai_participates: false` su una macchina, quella tace
alla richiesta remota (ma risponde comunque se invocata dal proprio umano locale).

---

# Implementation — orchestrator v0.30.0 (AI Chat: AI participant Slice 1a — @ai)

Slice 1a del design "AI Chat — le AI come partecipanti della stanza" (design:
`Docs/superpowers/specs/2026-07-02-aichat-ai-participants-design.md`, §6 "Slice 1a").
File toccati: `src/ai_adapter.rs`, `src/aichat/service.rs`, `src/main.rs`,
`src/telegram/channel.rs` (solo l'adapter di test `ProbeAdapter`),
`tests/aichat_relay_loopback.rs`. **Nessun cambio a `protocol`/`ui`/`mcp-server`**:
Slice 1a è backend-only — la risposta dell'AI è un `AiChatMessage` con label `-ai`,
già mostrato dalla UI esistente (il suffisso era già anticipato in `wire.rs`).

## Obiettivo

Un umano nella propria finestra AI Chat scrive `@ai <richiesta>` → la propria AI
locale genera **SOLO testo** (niente tool, niente finestre, niente web) usando il
transcript della stanza come contesto, e la risposta viene pubblicata nella stanza
come `Say` da `"<label_base>-ai"` (relay agli altri peer se siamo server + mostrata
nella propria UI + aggiunta allo storico). **Loop-safe per costruzione**: l'AI parla
SOLO su invocazione umana esplicita — nessun evento del sistema può ri-innescarla.

## Design 1 — `chat_reply`: text-only, senza tool (confine di sicurezza)

`AiAdapter` guadagna un secondo metodo, `chat_reply(transcript, request, cancel) ->
String`, indipendente da `respond` (il loop di tool-use del cursore):

- **`StubAdapter::chat_reply`** — `format!("[stub AI] commento su: {request}")`:
  deterministico, usato come default fallback (senza API key) e come doppio nei test
  del servizio AI Chat (`AiChatService::new_for_test`).
- **`ClaudeAdapter::chat_reply`** — UNA sola chiamata `self.messages.create(req)`
  (NON-streaming: non c'è un pannello di trasparenza da alimentare token-by-token
  come nel cursore) con:
  - `tools: Vec::new()` — questo, da solo, è il confine di sicurezza: il payload
    inviato all'API non porta ALCUN tool, quindi il modello non può nemmeno provare a
    invocarne uno (verificato dal test `claude_chat_reply_returns_text_only`,
    `req.tools.is_empty()`).
  - `system: CHAT_SYSTEM_PROMPT` — un nuovo system prompt "da partecipante chat",
    DIVERSO da `agent::SYSTEM_PROMPT` (che nomina esplicitamente `run_in_session`/
    `open_target`/`show_markdown` e istruisce a preferirne l'uso — sarebbe fuorviante
    qui, dove quei tool non esistono nel payload).
  - `messages: vec![Message::user_text(&input)]` — un SOLO turno `user`, dove `input`
    è `"{transcript}\n\n{request}"` (o solo `request` se il transcript è vuoto — prima
    invocazione in una stanza senza storico). Nessuna storia stateful: a differenza di
    `respond` (che accumula `history` fra chiamate successive sulla stessa
    connessione), ogni invocazione `@ai` è indipendente — il "contesto" è
    interamente il transcript passato in ingresso, ricostruito da `format_transcript`.
  - `cancel: Option<CancellationToken>` rispettato "alla leggera": un controllo
    upfront (`is_some_and(|c| c.is_cancelled())` → ritorna stringa vuota senza
    chiamare l'API) — non c'è un loop di iterazioni da interrompere a metà come nel
    cursore, quindi un controllo più fine non è necessario per questa slice.
  - Errore HTTP → `format!("[errore AI] {e}")` (stesso pattern di `respond`): mai un
    `Result` che il chiamante deve gestire, sempre una stringa pubblicabile.

`ProbeAdapter` (test helper già esistente in `telegram/channel.rs`, usato SOLO per
verificare che `run_command_buffered` passi `Some(&confirmer)` a `respond`) ha
ricevuto un'implementazione minima (`String::new()`) per continuare a soddisfare il
trait — non esercitata da alcun test (il suo scopo resta il wiring del confirmer).

## Design 2 — l'attore: rilevazione `@ai`, `InvokeLocalAi`, `AiReply`

`AiChatService` resta **puro** in `handle_event` (nessun I/O — vedi il doc-comment
del pattern attore in cima al file): la chiamata AI vive interamente in `perform`
(il layer I/O), collegata da un nuovo `Effect`/`ServiceEvent`, esattamente come già
avviene per `Effect::ConnectTo` → task spawnato → `ConnectOutcome` → evento di ritorno.

- **`extract_ai_invocation(text: &str) -> Option<String>`** (funzione pura,
  module-level, nessun `self`): rileva il prefisso `@ai` (case-insensitive, dopo aver
  ignorato spazi iniziali) seguito da uno spazio o dalla fine della stringa — questo
  secondo controllo evita il falso positivo "@aiuto" (che NON deve invocare l'AI). Se
  la richiesta dopo il prefisso è vuota (l'umano ha scritto solo `"@ai"`), usa il
  default `"commenta la discussione"` invece di un prompt vuoto. Ritorna `None` per
  qualunque messaggio che non inizia con `@ai`: è questo `None` — mai bypassato da
  nessun altro percorso del codice — che rende il modello loop-safe per costruzione.
- **`format_transcript(history: &[ChatLine]) -> String`** (funzione pura): una riga
  `"from_label: testo"` per messaggio, in ordine cronologico, unite da `\n`. Cap di
  lunghezza per sessioni molto lunghe: NON affrontato in questa slice (domanda aperta
  §8 del design doc).
- **`ServiceEvent::HumanSay(text)`** (arm esistente, refattorizzato): la
  pubblicazione (storico + eco UI + inoltro secondo ruolo) è ora delegata al nuovo
  metodo privato `publish_say(from_label, text)`. IN PIÙ, se `extract_ai_invocation`
  riconosce un'invocazione, l'arm emette anche un `Effect::InvokeLocalAi { request,
  transcript }` — costruendo il transcript da `self.history` **dopo** aver chiamato
  `publish_say` (quindi il transcript passato all'AI include già il messaggio di
  invocazione stessa: l'AI "vede" la propria chiamata in contesto).
- **`Effect::InvokeLocalAi { request, transcript }`** (nuovo): `perform` lo esegue
  spawnando un task indipendente (stesso motivo di `Effect::ConnectTo`: una chiamata
  HTTP non deve bloccare l'intero attore, che deve restare reattivo a `Tick`/
  `PeerMsg`/altri eventi nel frattempo) che chiama
  `ai_adapter.chat_reply(&transcript, &request, Some(shutdown.clone())).await` e
  re-inietta il risultato come `ServiceEvent::AiReply { text }` sull'inbox tramite
  `inbox_tx.clone()` — nessuno stato condiviso mutabile nuovo, solo un canale. Guardia
  (aggiunta in review, `perform_invoke_local_ai_skips_empty_reply`): se `text.trim()`
  è vuoto (cancellazione upfront di `chat_reply`, o una risposta degenere), il task
  ritorna SENZA mandare `AiReply` — altrimenti `publish_say` pubblicherebbe una riga
  di chat fantasma (storico + eco UI + relay di una stringa vuota).
- **`ServiceEvent::AiReply { text }`** (nuovo): trattato ESATTAMENTE come un `Say` da
  `"<label_base>-ai"` — stessa chiamata a `publish_say`. **Punto cruciale di
  loop-safety**: questo arm NON chiama `extract_ai_invocation` — anche se il testo
  generato dall'AI contenesse letteralmente `"@ai qualcosa"` (es. lo cita), non
  scatterebbe MAI un secondo `InvokeLocalAi` da qui. L'unico punto d'ingresso che può
  far parlare l'AI è `HumanSay`.
- **`publish_say(&mut self, from_label: String, text: String) -> Vec<Effect>`**
  (nuovo metodo privato, SOLID/DRY): fattorizza la logica di pubblicazione
  (storico → `ChatLine`; eco `Effect::ToUi(AiChatMessage)`; inoltro per ruolo —
  Server: broadcast a tutti i connessi; Client: invia solo al server; Undecided:
  nessun inoltro) che PRIMA di questa slice viveva duplicata solo dentro l'arm
  `HumanSay`. Ora è l'UNICO punto che sa come "pubblicare un messaggio nella stanza",
  riusato da entrambi gli arm — un cambio futuro alla logica di relay (es. Slice 1b,
  l'invocazione remota) tocca un solo posto.
- **Campo `ai_adapter: Arc<dyn AiAdapter>`** su `AiChatService`; `new(me,
  ai_adapter)` guadagna il secondo parametro. `main.rs` passa `ai.clone()` — LO
  STESSO `Arc<dyn AiAdapter>` già costruito per il cursore (`StubAdapter` senza
  `ANTHROPIC_API_KEY`, `ClaudeAdapter` con chiave): la propria AI del canale AI Chat
  è quindi sempre coerente con quella del cursore, nessuna configurazione separata.

## Costruttori di test — `new_for_test`

Il nuovo parametro di `new` avrebbe richiesto toccare ~44 call-site nei test interni
del crate (`#[cfg(test)] mod tests` in `service.rs`). Invece: nuovo
`#[cfg(test)] pub fn new_for_test(me: PeerInfo) -> Self` che chiama `Self::new(me,
Arc::new(StubAdapter))` — i 44 call-site sono stati aggiornati meccanicamente
(`AiChatService::new(` → `AiChatService::new_for_test(`, sostituzione di stringa
letterale SOLO dentro il blocco `#[cfg(test)] mod tests`) senza cambiare il loro
comportamento (comportamento invariato: stesso `StubAdapter` usato implicitamente
prima che il campo esistesse). `new_for_test` è `#[cfg(test)]`, quindi NON visibile
ai 3 test di integrazione in `tests/aichat_relay_loopback.rs` (crate separato, linka
il lib normalmente, non in modalità test): lì i 3 costruttori sono stati aggiornati
esplicitamente a `AiChatService::new(info, Arc::new(StubAdapter))`.

## Come si è ottenuto il RED

Due fasi RED distinte, stesso principio già usato per il `CancellationToken` in
0.15.0 (vedi lo storico di `ai_adapter.rs`): un errore di **compilazione** per un
metodo/variante non ancora esistente È la fase RED quando il cambio è strutturale
(nuova forma dell'API), non isolabile in un singolo comportamento a runtime.

1. **`chat_reply`**: `stub_chat_reply_mentions_request` e
   `claude_chat_reply_returns_text_only` scritti PRIMA di aggiungere il metodo al
   trait `AiAdapter` → `cargo test -p orchestrator chat_reply` falliva con due
   `E0599` ("no method named `chat_reply`"). Aggiunto il metodo al trait (con le due
   implementazioni) → GREEN. Un terzo `E0046` inatteso ("not all trait items
   implemented") in `telegram/channel.rs` (il `ProbeAdapter` dimenticato) è stato
   scoperto dallo stesso `cargo build` e colmato subito dopo.
2. **`AiReply`/`InvokeLocalAi`/`publish_say`/le due funzioni pure**: prima un
   refactor meccanico e comportamentalmente neutro (cambiare la firma di `new`,
   aggiungere `new_for_test`, aggiornare i call-site — verificato GREEN, 420 test
   invariati) per NON confondere il refactor abilitante con la vera fase RED. Poi gli
   11 nuovi test (invocazione, `AiReply`, `publish_say` via i suoi due chiamanti,
   `extract_ai_invocation`, `format_transcript`, il collegamento `perform` → inbox)
   scritti in un colpo solo usando l'API non ancora esistente
   (`ServiceEvent::AiReply`, `Effect::InvokeLocalAi`, le due funzioni pure) →
   `cargo test aichat::service::tests` falliva con 14 errori di compilazione
   (`E0425`/`E0599`, funzioni/varianti non trovate) — quello È il RED. Implementate
   le due varianti enum, le due funzioni pure, il refactor di `HumanSay`/`AiReply` in
   `handle_event`, e l'arm `Effect::InvokeLocalAi` in `perform` → GREEN.
3. **Guardia risposta vuota** (in review, dopo GREEN dei punti 1-2):
   `perform_invoke_local_ai_skips_empty_reply` scritto per PRIMO con un
   `EmptyReplyAdapter` che ritorna sempre `""` — a codice invariato, il test falliva a
   runtime (assert, non compilazione: `recv()` restituiva `Some(AiReply{text:""})`
   invece di `None`) perché `perform` mandava comunque l'evento. RED genuino a runtime,
   stesso pattern dei test di dominio del resto del file. Aggiunta la guardia
   (`if text.trim().is_empty() { return; }`) → GREEN.

## Test

- `extract_ai_invocation_returns_none_for_plain_message` /
  `extract_ai_invocation_detects_prefix_case_insensitive` /
  `extract_ai_invocation_alone_defaults_to_generic_request` /
  `extract_ai_invocation_requires_word_boundary` — la funzione pura, in isolamento
  totale (nessun `AiChatService` costruito).
- `format_transcript_formats_label_colon_text_per_line` /
  `format_transcript_empty_history_is_empty_string` — idem.
- `at_ai_invocation_emits_invoke_local_ai` — `HumanSay("@ai commenta X")` produce
  SIA l'eco UI normale SIA un `Effect::InvokeLocalAi` con `request == "commenta X"` e
  `transcript` che include la riga appena pubblicata.
- `plain_message_does_not_invoke_ai` — `HumanSay("ciao a tutti")` non produce mai
  `InvokeLocalAi` (loop-safety, caso base).
- `ai_reply_posts_say_from_ai_label` — da server con due client connessi
  (`server_with_two_clients`, fixture esistente), `AiReply{text:"ecco"}` produce
  `ToUi(AiChatMessage{from_label:"skimble-ai", ..})`, almeno un
  `SendToPeer(_, Say{from_label:"skimble-ai", ..})`, e `self.history` contiene la
  riga `-ai`.
- `ai_reply_does_not_reinvoke` — anche con un testo che contiene letteralmente
  `"@ai richiama ancora?"`, l'arm `AiReply` non produce mai `InvokeLocalAi`
  (loop-safety, caso limite).
- `perform_invoke_local_ai_sends_ai_reply_back_to_inbox` — chiama
  `s.perform(Effect::InvokeLocalAi{..}, ...)` (stesso pattern degli altri test
  `perform` già presenti nel file, es. quelli di `ConnectTo`) e verifica che l'inbox
  riceva un `ServiceEvent::AiReply` col testo dello `StubAdapter` (collegamento
  end-to-end effetto → task → evento di ritorno).
- `perform_invoke_local_ai_skips_empty_reply` (aggiunto in review) — con un
  `EmptyReplyAdapter` di test (`chat_reply` → `String::new()`), verifica che l'inbox
  si esaurisca SENZA alcun evento: droppato il proprio `inbox_tx`, `recv()` ritorna
  `None` solo quando anche il clone del task spawnato è stato droppato senza aver
  mandato nulla — prova che la guardia impedisce la riga fantasma.

## Verifica

`cargo test -p orchestrator`: 432 test lib (418 passed su 0.29.0 + 14 nuovi: 2 in
`ai_adapter.rs`, 12 in `aichat/service.rs`), 0 failed, 3 ignored (invariato). `cargo
test -p orchestrator --test aichat_relay_loopback -- --ignored`: 2/2 (i tre
costruttori aggiornati al secondo parametro non hanno alterato il comportamento del
relay TCP reale su loopback). `cargo clippy -p orchestrator --all-targets`: 2 warning
preesistenti (`ai_adapter.rs` — `map_or`/`is_some_and` nel loop di `respond`,
verificato NON introdotto da questa slice via `git stash`; `ws.rs:491`), nessuno
nuovo. `cargo build -p orchestrator`: pulito (1 warning atteso e transitorio durante
lo sviluppo — `ai_adapter` mai letto — sparito una volta cablato `perform`). `cargo
fmt` deliberatamente NON eseguito su tutto il crate (drift preesistente).

## Hardening in review (advisor)

Due migliorie emerse in una review pre-consegna (`advisor`), applicate con TDD:
1. La guardia su risposta vuota in `Effect::InvokeLocalAi` (sopra) — senza,
   un'invocazione `@ai` con `chat_reply` vuota avrebbe pubblicato una riga di chat
   fantasma.
2. `claude_chat_reply_returns_text_only` rinforzato con due asserzioni: `!req.system.
   contains("run_in_session")` (un regresso che ricablasse per errore il system prompt
   del cursore sarebbe passato comunque, dato che i tool restano vuoti) e che il turno
   `user` porti SIA il transcript SIA la richiesta (non solo la richiesta da sola).

## Resta per l'accettazione live

Slice 1a non ha ancora una prova end-to-end con l'AI reale: serve avviare due
orchestrator con AI Chat abilitato (`aichat.json`, `enabled: true`) e
`ANTHROPIC_API_KEY` impostata, aprire la finestra AI Chat su almeno uno dei due,
scrivere `@ai <richiesta>` e osservare la risposta `-ai` comparire (nella propria UI
e, se il peer è connesso, anche in quella dell'altra macchina via relay). Non
verificato in questa slice (fuori portata dei test automatici, che usano
`StubAdapter`/`FakeMessagesClient`).

---

# Implementation — orchestrator v0.29.0 (AI Chat hardening: register-before-spawn — Slice C2)

Slice C2 del piano di hardening AI Chat (design:
`Docs/superpowers/specs/2026-07-02-aichat-hardening-debts-design.md`, §"Slice C2"). Chiude
una race residua lasciata da Slice C (0.27.0, debiti #4 generation token + #3 connect non
bloccante). File toccato: **solo** `src/aichat/service.rs` (nessun altro file di
`orchestrator`, nessun cambio a `protocol`/`ui`/`mcp-server`).

## Problema

Slice C (0.27.0) aveva introdotto la generazione dei link (`link_gen`) per distinguere un
`PeerGone` genuino da uno stale (di un link già rimpiazzato da una riconnessione). Ma
lasciava una race residua nel PUNTO in cui `link_gen` veniva scritto: sia sul path CONNECT
sia sul path ACCEPT, `spawn_peer_tasks` (che avvia il reader/writer) veniva chiamata DENTRO
il task spawnato (il task di connessione, o l'accept loop stesso) — cioè PRIMA che l'attore
avesse la possibilità di registrare lo stato (`self.link_gen.insert`/`self.connected.insert`,
che avveniva solo DOPO, quando l'attore processava l'evento di notifica).

Concretamente, sul path CONNECT:
1. Il task di connessione (spawnato per `Effect::ConnectTo`) fa `TcpStream::connect(..)`.
2. Su successo, chiama `spawn_peer_tasks(stream, ..., gen, ...)` — il reader parte SUBITO.
3. Manda `ConnectOutcome::Success { info, writer_tx, gen }` sul canale `connect_res_tx`.
4. L'attore (5° ramo di `select!` in `run`) riceve l'esito e SOLO ORA scrive
   `self.link_gen.insert(id, gen)`.

Se il link muore immediatamente dopo il passo 2 (es. il peer chiude subito, o un errore di
rete), il reader spawnato al passo 2 può emettere `ServiceEvent::PeerGone(id, gen)` ed essere
processato dall'attore PRIMA del passo 4. La guardia di generazione in
`handle_event(PeerGone)` (`self.link_gen.get(&id) != Some(&gen)`) vede questo evento come
STALE (la mappa `link_gen` non ha ancora quella voce — `None != Some(gen)`) e lo scarta come
no-op. Il `Success` del passo 3/4 arriva comunque, POCO DOPO, e registra il link normalmente
— risultato: un **link fantasma**, registrato in `connected`/`links`, ma il cui reader è già
morto. Nessun secondo `PeerGone` arriverà mai a ripulirlo (il reader è già uscito, ha già
emesso il suo unico evento). Il guard anti-duplicato di `Effect::ConnectTo`
(`self.connected.contains(&info.id)`) blocca poi qualunque tentativo di riconnessione futura
verso quel peer, perché lo vede ancora "connesso" — il canale resta rotto silenziosamente
finché l'orchestrator non viene riavviato.

Stesso pattern, LATENTE (mai osservato in pratica ma strutturalmente identico), sul path
ACCEPT: l'accept loop chiamava `spawn_peer_tasks` prima di notificare l'attore via
`new_link_tx`.

## Fix — la registrazione di stato si sposta DENTRO l'attore, PRIMA dello spawn

L'idea è semplice ma strutturale: **`spawn_peer_tasks` deve essere chiamata SOLO dopo che
`self.link_gen`/`self.connected` sono già stati scritti — mai prima.** Poiché la scrittura
di questi campi richiede `&mut self` (solo l'attore, in `run`, ce l'ha), lo spawn deve
spostarsi anch'esso dentro l'attore.

1. **`ConnectOutcome::Success`** ora porta il `TcpStream` GREZZO invece di un `writer_tx` già
   pronto: `Success { info: PeerInfo, stream: TcpStream, gen: u64 }`. Il task di connessione
   (in `perform`/`Effect::ConnectTo`) NON chiama più `spawn_peer_tasks`: fa SOLO
   `tokio::time::timeout(5s, TcpStream::connect(addr))` e manda l'esito (stream o
   `Failure { id }`) su `connect_res_tx`. La generazione (`gen`) continua ad essere allocata
   in `Effect::ConnectTo` PRIMA dello spawn del task (con `&mut self`, tramite `self.next_gen`)
   — non importa CHI alloca il numero, quello che conta per il fix è CHI e QUANDO scrive
   `self.link_gen`.

2. **Il canale interno accept→attore** (`new_link_tx`/`new_link_rx`) cambia tipo da
   `(PeerId, UnboundedSender<ChatMsg>, u64)` a `(PeerId, TcpStream)`: l'accept loop manda il
   socket grezzo, senza più allocare una generazione né chiamare `spawn_peer_tasks`.

3. **Tre nuovi metodi privati** su `AiChatService`, che sostituiscono il vecchio
   `apply_connect_outcome` (rimosso) — SOLO mutazione di stato, nessun I/O, chiamati
   dall'attore PRIMA dello spawn:
   - `register_link(&mut self, id, gen)` — scrive `link_gen[id] = gen` e `connected.insert(id)`.
     Usato da ENTRAMBI i path (accept diretto, e indirettamente da CONNECT tramite il punto
     sotto). Il doc-comment di questo metodo spiega in dettaglio l'invariante che chiude la
     race: chiamato SEMPRE prima di `spawn_peer_tasks`, senza alcun `.await` fra le due
     chiamate (uno `spawn` è "fire and forget", non cede il controllo), quindi non esiste
     ALCUNA finestra in cui il reader appena creato possa eseguire prima che
     `self.link_gen[id]` esista già.
   - `register_connect_success(&mut self, id, gen)` — `register_link` + libera `connecting`
     (il guard anti-duplicato del path CONNECT). Non tocca `links` né manda il `Join`: quelli
     restano responsabilità del chiamante, SUBITO DOPO aver chiamato `spawn_peer_tasks` (ha
     bisogno del `writer_tx` restituito).
   - `register_connect_failure(&mut self, id)` — libera solo `connecting` (comportamento di
     retry-on-failure preesistente, invariato).

4. **Ramo 2 del `select!` in `run`** (`new_link_rx.recv()`, notifiche dall'accept loop):
   ```rust
   maybe_link = new_link_rx.recv() => {
       let Some((peer_id, stream)) = maybe_link else { break };
       let gen = self.next_gen;
       self.next_gen += 1;
       self.register_link(peer_id, gen);                                   // PRIMA
       let writer_tx = spawn_peer_tasks(stream, peer_id, inbox_tx.clone(), gen, shutdown.clone());
       self.links.insert(peer_id, writer_tx);
   }
   ```
   L'allocazione della generazione si sposta QUI (era nell'accept loop, in un contesto senza
   `&mut self`): coerente col principio "tutto lato attore".

5. **Ramo 4 del `select!` in `run`** (`connect_res_rx.recv()`, esiti di connessione):
   ```rust
   maybe_outcome = connect_res_rx.recv() => {
       let Some(outcome) = maybe_outcome else { break };
       match outcome {
           ConnectOutcome::Success { info, stream, gen } => {
               let id = info.id;
               self.register_connect_success(id, gen);                     // PRIMA
               let writer_tx = spawn_peer_tasks(stream, id, inbox_tx.clone(), gen, shutdown.clone());
               self.links.insert(id, writer_tx.clone());
               let label = format!("{}-human", self.me.label_base);
               let _ = writer_tx.send(ChatMsg::Join { label });
           }
           ConnectOutcome::Failure { id } => { self.register_connect_failure(id); }
       }
   }
   ```
   Il `Join` di presentazione (invariato nel contenuto) si manda ORA dopo lo spawn — stesso
   punto logico di prima (quando l'attore sa che il link è pronto), solo riorganizzato.

## Conseguenza: `link_gen_counter: Arc<AtomicU64>` → `next_gen: u64`

Fino alla 0.28.0, `link_gen_counter` era un `Arc<AtomicU64>` CONDIVISO (stesso pattern di
`believed_leader`): serviva perché l'accept loop e il task di connessione — entrambi
SPAWNATI, senza `&mut self` — allocavano la propria generazione autonomamente con
`fetch_add(1, Ordering::Relaxed)`. Con questo fix, l'allocazione (oltre alla scrittura) è
TUTTA lato attore: un `Arc` condiviso non serve più. Sostituito da un campo `u64` di proprietà
esclusiva dell'attore (`next_gen`), incrementato con `let gen = self.next_gen; self.next_gen
+= 1;` nei tre punti che allocano una generazione (`Effect::ConnectTo` in `perform`, ramo 2 di
`run`, e l'helper di test `mark_connected_for_test`). Rimossi `use
std::sync::atomic::{AtomicU64, Ordering}` (non più referenziati nel file).

## Come si è ottenuto il RED

Cambiare la forma di `ConnectOutcome::Success` (da `writer_tx` a `stream`) e introdurre
`register_connect_success`/`register_connect_failure` (che non esistevano ancora) rompe la
compilazione di QUALUNQUE test che costruiva un `ConnectOutcome::Success` o chiamava
`apply_connect_outcome` — non un fallimento isolabile in un singolo comportamento, come già
osservato per il cambio di arità di `PeerGone` in 0.27.0. Il RED genuino qui è stato quindi
il fallimento di **compilazione** dei test aggiornati per PRIMI (nuova forma delle
chiamate, metodi non ancora esistenti in produzione): `cargo test -p orchestrator --lib`
falliva con `E0308` (tipo del canale `new_link_tx` non combaciante) e due `E0599` (metodo
non trovato) — la mancanza dell'API desiderata resa esplicita dal compilatore, poi colmata
implementando `register_link`/`register_connect_success`/`register_connect_failure` e i
cambi di tipo in `perform`/`run` fino a GREEN.

## Test

- `connect_success_registers_state_before_any_spawn` (riscritto da
  `connect_success_registers_link_and_sends_join`, 0.27.0): verifica che
  `register_connect_success` scriva `link_gen`/`connected` e liberi `connecting` — SENZA
  fabbricare un `TcpStream`/`writer_tx` (non servono: la registrazione è pura mutazione di
  stato, lo spawn+Join restano nel 4° ramo di `run`, coperti solo dal test di integrazione).
- `register_connect_success_makes_subsequent_peer_gone_non_stale` (NUOVO): documenta
  esplicitamente l'invariante che chiude la race. Chiama `register_connect_success(id, gen)`
  e POI simula l'evento che il reader avrebbe emesso se il link fosse morto immediatamente
  (`handle_event(PeerGone(id, gen))`), verificando che venga trattato come GENUINO (non
  stale): `connected`/`link_gen` vengono ripuliti, invece di restare intonsi come accadrebbe
  per un vero stale. Non serve un socket: la garanzia è che la SCRITTURA precede sempre
  QUALUNQUE possibile spawn, quindi qualunque `PeerGone` successivo per quella generazione
  combacia sempre.
- `connect_failure_clears_connecting_for_retry`: invariato nel comportamento verificato,
  aggiornato per chiamare `register_connect_failure` invece di `apply_connect_outcome`.
- `stale_peer_gone_does_not_remove_fresh_link` / `peer_gone_matching_generation_removes_link`
  (Slice C, 0.27.0): invariati, verdi senza modifiche — la guardia di generazione in
  `handle_event(PeerGone)` non cambia; questa slice ne rende semplicemente impossibile
  l'aggiramento strutturale.
- `connect_dispatch_marks_connecting_and_dedups`: aggiornato solo per il nuovo tipo del
  canale `new_link_tx` (`(PeerId, TcpStream)`); la guardia sincrona che verifica resta
  invariata.

`cargo test -p orchestrator`: 421 test lib (418 passed + 3 ignored, +1 rispetto a 0.28.0
[417 passed + 3 ignored] — un test riscritto in due), 0 failed. `cargo test -p orchestrator
--test aichat_relay_loopback -- --ignored`: 2/2 (relay, keepalive e connect via TCP reale su
loopback reggono con la nuova sequenza registra→spawna). `cargo clippy -p orchestrator
--all-targets`: 2 warning preesistenti (`ai_adapter.rs:173`, `ws.rs:491`), nessuno nuovo —
rimuovendo l'`Arc<AtomicU64>` sono stati tolti anche gli `use` non più referenziati
(`std::sync::atomic::{AtomicU64, Ordering}`). `cargo build -p orchestrator`: pulito. `cargo
fmt` deliberatamente NON eseguito (drift preesistente sull'intero crate).

---

# Implementation — orchestrator v0.28.0 (AI Chat hardening: discovery full-duplex — #6)

Slice D del piano di hardening AI Chat (design:
`Docs/superpowers/specs/2026-07-02-aichat-hardening-debts-design.md`, §"Slice D"). File
toccati: `src/aichat/discovery.rs`, `src/aichat/net.rs`, `src/main.rs` (nessun cambio a
`service.rs`, `protocol`, `ui`, `mcp-server`).

## Problema

`Discoverer::next(&mut self)` richiedeva `&mut self` (serviva al buffer di ricezione
riusabile di `UdpDiscoverer`, pensato per evitare un'allocazione per datagramma), mentre
`announce(&self)` richiede `&self`. Le due chiamate non potevano coesistere in un
`tokio::select!` sulla stessa variabile `disc`: Rust vieta un prestito `&mut` e uno `&`
simultanei sullo stesso valore. Il loop di scoperta in `main.rs` era quindi SEQUENZIALE
(un annuncio, poi `timeout(500ms, next())` ripetuto finché non erano trascorsi ~7s, poi un
nuovo annuncio) — niente ascolto continuo mentre si annuncia.

## Fix

1. **`discovery.rs`** — `trait Discoverer::next` passa da `&mut self` a `&self`. Il
   doc-comment del trait spiega il "perché" (vedi il nuovo paragrafo "`&self` su ENTRAMBI
   i metodi").
2. **`discovery.rs` — `FakeDiscoverer`** — `incoming` passa da `VecDeque<(PeerInfo,
   Option<PeerId>)>` a `Mutex<VecDeque<(PeerInfo, Option<PeerId>)>>` (mutabilità interna):
   `next(&self)` prende il lock, fa `pop_front`, rilascia — nessun `.await` tenuto col
   lock aperto. Il test `fake_discoverer_yields_prefixed_then_none` non richiede più `let
   mut d`.
3. **`net.rs` — `UdpDiscoverer`** — rimosso il campo `buf: Vec<u8>` dalla struct e dal
   costruttore. `next(&self)` alloca un buffer LOCALE (`let mut buf = [0u8; 2048];`) ad
   ogni chiamata: costo trascurabile (i datagrammi di scoperta arrivano ogni pochi
   secondi, non è un percorso hot-path) — la correttezza/full-duplex vale più della
   micro-ottimizzazione del buffer riusato. `announce` invariata (già `&self`,
   `tokio::net::UdpSocket::send_to`/`recv_from` supportano nativamente l'uso concorrente
   da `&self`, il socket non ha bisogno di sincronizzazione aggiuntiva).
4. **`main.rs` — loop di scoperta** (dentro il blocco "Canale AI Chat", task "Task
   scoperta UDP") — riscritto come `tokio::select!` full-duplex su TRE rami:
   - `announce_tick.tick()` — `tokio::time::interval(ANNOUNCE_INTERVAL)` con
     `ANNOUNCE_INTERVAL = 5s` (`const` locale al task). Il primo tick di
     `tokio::time::interval` scatta IMMEDIATAMENTE (comportamento di default), poi ogni
     5s — equivalente al "primo annuncio subito" del vecchio schema. Il leader creduto è
     estratto dal `Mutex` PRIMA dell'`.await` (stesso vincolo preesistente:
     `std::sync::MutexGuard` non è `Send`, non può restare vivo attraverso un punto di
     sospensione).
   - `disc.next()` — ascolto continuo; su `Some((peer, leader))` inietta
     `ServiceEvent::Discovered(peer, leader)` nell'inbox; su `None` (sorgente chiusa in
     modo permanente) esce dal loop con un log di warning.
   - `shutdown_disc.cancelled()` — nuovo ramo: estende il teardown del debito #2
     (Slice B, `CancellationToken` propagato "verso il basso") anche a questo task, che
     prima girava per sempre (nessun `break` raggiungibile). `shutdown_disc` è un clone
     del token globale creato prima del blocco AI Chat, catturato `move` nella closure del
     task.
   - `disc` non è più `let mut`: entrambi i metodi sono `&self`, il loop prende solo
     prestiti immutabili (mai esclusivi) ad ogni ramo — questo è ciò che rende possibile
     il `select!`.
   - Cancellation-safety del ramo `disc.next()`: quando `announce_tick` vince, il future
     `disc.next()` in corso viene droppato e ricreato al giro successivo. Non perde
     datagrammi — `recv_from` di `tokio::UdpSocket` è cancellation-safe (un future
     droppato mentre è in attesa non ha ancora estratto nulla dal buffer del kernel; un
     datagramma arrivato nel frattempo resta lì per la prossima `recv_from`); e quando
     `recv_from` ha già un datagramma pronto, `next()` lo decodifica e ritorna in modo
     sincrono, senza punti di sospensione fra la lettura e il `return`.

## Test

Il RED di questa slice è principalmente DI COMPILAZIONE (come per gli slice precedenti
che cambiano firme pubbliche): cambiare `Discoverer::next` da `&mut self` a `&self`
rompe immediatamente la compilazione di `FakeDiscoverer` e `UdpDiscoverer` (entrambi
implementano il vecchio `&mut self`) — verificato con `cargo build -p orchestrator`
(errore `E0053: method next has an incompatible type for trait` su entrambi gli
implementori) PRIMA di correggerli, uno alla volta, fino al verde.

Nuovo test deterministico in `discovery.rs`:
`announce_and_next_usable_concurrently_on_shared_ref` — costruisce un `&d` condiviso e
chiama `tokio::join!(d_ref.announce(None), d_ref.next())`: se `next` richiedesse ancora
`&mut self`, il modulo non compilerebbe (E0502, prestito mutabile mentre esiste un
prestito immutabile attivo). Il fatto che compili ED esegua è la prova che l'aliasing
`&mut`/`&` è sparito — è la stessa forma di concorrenza (due prestiti `&` vivi
attraverso punti di sospensione) che il `select!` di `main.rs` usa in produzione.

Non è stato aggiunto un `#[ignore]` integration test con due `UdpDiscoverer` reali in
loopback (alternativa suggerita dal design): due discoverer sullo stesso host o
collidono sul bind (stessa porta, nessun `SO_REUSEADDR` impostato) o, con porte diverse,
non si sentirebbero a vicenda (ognuno annuncia sulla PROPRIA porta broadcast); la
consegna di broadcast UDP sullo stesso host è inoltre nota per essere flaky
cross-platform. Lo stesso motivo per cui `tests/aichat_relay_loopback.rs` esercita
deliberatamente il relay "senza scoperta UDP". Il test deterministico sul `FakeDiscoverer`
sopra è l'opzione preferita indicata dal brief ("se fattibile in modo deterministico") ed
è quella scelta.

## Verifica

`cargo test -p orchestrator --lib`: 420 test lib (417 passed + 3 ignored, +1 rispetto a
0.27.0 — il nuovo test full-duplex), 0 failed. `cargo test -p orchestrator --test
aichat_relay_loopback -- --ignored`: 2/2 (invariato — quel file non usa `Discoverer`).
`cargo build -p orchestrator`: pulito, nessun warning (in particolare nessun
`unused_mut` residuo su `disc` in `main.rs`). `cargo clippy -p orchestrator
--all-targets`: 2 warning preesistenti (`ai_adapter.rs:173`, `ws.rs:491`), nessuno nuovo
introdotto da questa slice. `cargo fmt` deliberatamente NON eseguito sull'intero crate
(drift preesistente, vedi 0.25.9/0.26.0/0.27.0).

**Limite noto.** Il loop `select!` full-duplex in `main.rs` non ha copertura
automatizzata diretta (non è testabile in isolamento senza un vero socket UDP e un vero
timer — il task è spawnato dentro `main`, non è una funzione estratta testabile). La
correttezza è garantita per costruzione: (a) a livello di tipo, dal test
`announce_and_next_usable_concurrently_on_shared_ref`; (b) a livello di compilazione, dal
fatto che `main.rs` compila il `select!` sulle chiamate reali `UdpDiscoverer::announce`/
`next`. Un test end-to-end dal vivo (avviare l'orchestrator, osservare due istanze
scoprirsi mentre annunciano) resta un candidato per la verifica manuale del supervisore,
coerente col limite già documentato per Slice B (propagazione dello shutdown ai task
figli).

---

# Implementation — orchestrator v0.27.0 (AI Chat hardening: link robustness — #4 generation + #3 non-blocking connect)

Slice C del piano di hardening AI Chat (design:
`Docs/superpowers/specs/2026-07-02-aichat-hardening-debts-design.md`, §"Slice C"). Due
debiti, un solo slice coordinato perché condividono `spawn_peer_tasks`/`Effect::ConnectTo`/
`ServiceEvent::PeerGone`. File toccato: `src/aichat/service.rs` (nessun altro file di
`orchestrator`, e nessuna modifica a `protocol`/`ui`/`mcp-server`).

## Parte #4 — generation token (race di riconnessione)

**Problema.** `ServiceEvent::PeerGone(PeerId)` rimuoveva il link nella mappa `self.links`
identificando il peer SOLO per `PeerId`. Scenario di race: il reader task di un link A
(già morto — es. il peer si è disconnesso e riconnesso rapidamente) impiega più tempo ad
accorgersi della propria morte (timeout keepalive, 15s) di quanto ce ne voglia per aprire
un nuovo link B verso lo STESSO IP. Quando il `PeerGone` "vecchio" del link A viene
finalmente processato, `self.links.remove(&id)` rimuove il link B (fresco) — il peer
riconnesso resta senza writer, silenziosamente, senza errore visibile.

**Fix — generazione monotona per link.**

1. `ServiceEvent::PeerGone(PeerId, u64)` — il secondo campo è la GENERAZIONE del link che
   è morto (non "il peer che è morto": la distinzione è cruciale — lo stesso `PeerId` può
   avere più generazioni nel tempo, una per ogni link TCP che si apre verso di esso).
2. Nuovo stato su `AiChatService`:
   - `link_gen_counter: Arc<AtomicU64>` — contatore CONDIVISO e monotono (`init
     Arc::new(AtomicU64::new(0))` in `new()`). Deve essere condiviso (non un semplice
     `u64` di `self`) perché i link nascono in contesti SPAWNATI (l'accept loop, e — dopo
     la Parte #3 — il task di connessione) che non hanno `&mut self`: allocano la propria
     generazione con `fetch_add(1, Ordering::Relaxed)` senza passare dall'attore. Stesso
     pattern di stato condiviso già accettato per `believed_leader` (vedi il suo
     doc-comment preesistente).
   - `link_gen: HashMap<PeerId, u64>` — SOLO attore: la generazione del link ATTUALMENTE
     vivo per ogni peer connesso. Letta/scritta solo da `handle_event`/`perform`/
     `apply_connect_outcome`.
3. `spawn_peer_tasks(stream, peer_id, inbox_tx, gen: u64, shutdown)` — quarto parametro
   (prima di `shutdown`). Il reader task, quando esce (EOF/errore/timeout keepalive/
   shutdown), invia `ServiceEvent::PeerGone(peer_id, gen)` — la generazione che gli è
   stata assegnata alla creazione, non quella (eventualmente diversa) che il peer ha
   ORA.
4. `handle_event(PeerGone(id, gen))`: PRIMA di qualunque altra cosa,
   `if self.link_gen.get(&id) != Some(&gen) { return Vec::new(); }` (con
   `tracing::debug!` che spiega). Solo se la generazione combacia procede con TUTTA la
   logica preesistente (rimozione da `connected`/`peers`/`consented`/`pending`/`refused`/
   `links`, azzeramento `reported_leader`, annunci `PeerLost`/`Roster`, rielezione) — più
   `self.link_gen.remove(&id)`, aggiunto accanto a `self.links.remove(&id)`.
5. Ogni punto che registra un link vivo scrive anche `link_gen[id] = gen`:
   - Ramo accept (`Effect::StartListener`, dentro `perform`): il canale interno
     `new_link_tx` è esteso da `(PeerId, UnboundedSender<ChatMsg>)` a
     `(PeerId, UnboundedSender<ChatMsg>, u64)`. L'accept loop clona il contatore
     condiviso PRIMA dello spawn (`link_gen_counter_acc = self.link_gen_counter.clone()`),
     alloca `gen` per ogni connessione accettata, la passa a `spawn_peer_tasks` e la
     include nel `send((peer_id, writer_tx, gen))`. Il ramo 2 del `select!` in `run`
     inserisce `self.link_gen.insert(peer_id, gen)` insieme a `links`/`connected`.
   - Ramo connect riuscito (`ConnectOutcome::Success`, Parte #3 sotto):
     `apply_connect_outcome` fa lo stesso.

## Parte #3 — connect non bloccante (head-of-line blocking)

**Problema.** `Effect::ConnectTo` eseguiva `TcpStream::connect(addr).await` **inline**
dentro `perform` — cioè dentro il task dell'attore stesso (`run`). Un peer irraggiungibile
(es. spento, cavo di rete staccato, firewall che droppa i SYN in silenzio) blocca l'intero
attore per il timeout del sistema operativo: su Windows può arrivare a ~21s. Durante quel
tempo l'attore non processa NULLA — niente `Tick` (i ping keepalive si fermano), niente
`PeerMsg` dagli altri link già connessi, niente altri `Discovered`. Un solo peer lento
degrada l'intero canale AI Chat.

**Fix — connect spawnato, mai atteso da `perform`.**

1. Nuovo enum privato al modulo (non `ServiceEvent`, stesso trattamento del canale
   `new_link_tx` interno — dettaglio I/O, non evento di dominio):
   ```rust
   enum ConnectOutcome {
       Success { info: PeerInfo, writer_tx: UnboundedSender<ChatMsg>, gen: u64 },
       Failure { id: PeerId },
   }
   ```
2. Nuovo stato SOLO attore: `connecting: HashSet<PeerId>` — guard anti-duplicato: un
   `Discovered` periodico che arriva mentre un connect lento è ancora in corso non deve
   aprire un secondo tentativo verso lo stesso peer.
3. `perform`/`Effect::ConnectTo(info)` — riscritto per fare SOLO la parte sincrona:
   - Guard: `if self.connected.contains(&info.id) || self.connecting.contains(&info.id)
     { return; }`.
   - `self.connecting.insert(info.id)`.
   - Alloca `gen` dal contatore condiviso (Parte #4) — PRIMA dello spawn, così è
     disponibile sia per il task sia per `ConnectOutcome::Success`.
   - **Spawna** un task (`tokio::spawn`, MAI `.await`-ato da `perform`) che fa il connect
     vero con un `tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(addr))`
     ESPLICITO (non ci affidiamo più al timeout OS): su successo chiama
     `spawn_peer_tasks` e manda `ConnectOutcome::Success{ info, writer_tx, gen }` sul
     canale `connect_res_tx`; su errore o timeout manda `ConnectOutcome::Failure{ id }`.
   - `perform` ritorna SUBITO dopo lo spawn — l'attore non attende mai il socket.
4. `perform` guadagna un quinto parametro `connect_res_tx: &UnboundedSender<ConnectOutcome>`
   (per riferimento, come già `new_link_tx`), clonato verso il task spawnato.
5. Nuovo metodo `apply_connect_outcome(&mut self, outcome: ConnectOutcome)` — ESTRATTO
   dal ramo del `select!` (non inlineato) per due motivi: (a) SRP — separa "interpretare
   un esito di connessione" da "il compito del loop di ascoltare i canali"; (b)
   TESTABILITÀ — un test inietta un `ConnectOutcome` sintetico e verifica la mutazione di
   stato senza aprire un socket. Non è `#[cfg(test)]`: è logica di produzione reale,
   condivisa dal quinto ramo di `run` e dai test.
   - `Success { info, writer_tx, gen }`: registra `links`/`connected`/`link_gen`, libera
     `connecting`, invia `ChatMsg::Join { label: "{me}-human" }` (il Join che PRIMA di
     questo fix veniva inviato inline dentro `Effect::ConnectTo` — logicamente invariato,
     solo spostato al punto in cui l'attore sa che la connessione è riuscita).
   - `Failure { id }`: libera SOLO `connecting` — il prossimo `Discovered`/
     `decide_and_connect` per lo stesso peer ritenterà (comportamento di
     retry-on-failure preesistente, preservato).
6. `run`: nuovo canale interno `(connect_res_tx, connect_res_rx) =
   unbounded_channel::<ConnectOutcome>()`, quinto ramo del `select!`:
   `maybe_outcome = connect_res_rx.recv() => { ...; self.apply_connect_outcome(outcome); }`
   (renumerato: lo shutdown, prima quarto ramo, è ora il quinto).

## Come testare il connect senza un socket reale

`connect_dispatch_marks_connecting_and_dedups` (`#[tokio::test]`) chiama `perform`
direttamente con un `Effect::ConnectTo` verso un indirizzo in TEST-NET-1
(`192.0.2.0/24`, RFC 5737 — riservato alla documentazione, mai instradato) e verifica lo
stato di `connecting` SUBITO dopo `.await`: la guardia (`connected.contains` /
`connecting.contains`, poi `connecting.insert`) è tutta SINCRONA, prima dello
`tokio::spawn` — osservabile senza attendere l'esito del connect (che comunque, spawnato
su un indirizzo non instradato, non completerà mai durante il test; irrilevante, la
guardia non dipende da quell'esito). `connect_success_registers_link_and_sends_join` e
`connect_failure_clears_connecting_for_retry` chiamano `apply_connect_outcome`
direttamente con un `ConnectOutcome` costruito a mano (nessun `TcpStream`, un normale
`unbounded_channel::<ChatMsg>()` per il `writer_tx` fittizio) — logica pura, nessun I/O.

## Migrazione dei test esistenti (PeerGone a due campi)

Il cambio di arità di `ServiceEvent::PeerGone` tocca ogni test che lo costruiva — non è
un cambio isolabile in un singolo comportamento (il tipo deve compilare prima che
qualunque test possa girare), quindi il RED genuino per la Parte #4 è stato ottenuto in
due passi: (1) tutta l'infrastruttura (campo aggiunto, stato, helper, popolamento di
`link_gen` in ogni punto di registrazione) SENZA la guardia in cima all'arm, con tutti i
test migrati alla forma a due campi con generazioni COERENTI — a questo punto
`stale_peer_gone_does_not_remove_fresh_link` fallisce per il motivo giusto (lo stale
rimuove davvero il link fresco, zero guardia); poi (2) la guardia, che porta il test a
verde senza toccare nessun altro test. `peer_gone_matching_generation_removes_link` è
invece un test di regressione (verde fin dal passo 1): la sua unica porzione di RED
genuino è l'asserzione su `link_gen_for_test(id) == None` dopo la rimozione (il
`self.link_gen.remove(&id)` aggiunto nel passo 1).

Per far combaciare le generazioni nei test preesistenti, due percorsi:
- Se lo scenario passava da `mark_connected_for_test(id)` (che ora assegna anche una
  generazione dal contatore condiviso, oltre a registrare un `links[id]` fittizio — un
  `UnboundedSender<ChatMsg>` il cui ricevitore viene scartato, così
  `links_contains_for_test` può osservare presenza/assenza): recupera la generazione con
  `link_gen_for_test(id)` subito prima di costruire il `PeerGone`.
- Se lo scenario connetteva il peer SOLO tramite `Consent` (mai realmente "connesso" nel
  senso di `self.connected` — es. `leader_change_on_peer_gone_updates_last_logged_leader`,
  che verifica solo l'elezione, non lo stato del link): nuovo helper
  `set_link_gen_for_test(id, gen)`, che imposta `link_gen` SENZA gli altri effetti
  collaterali di `mark_connected_for_test` (che sovrascriverebbe la `PeerInfo` con una
  sintetica, richiedendo poi un `Discovered` di ripristino — pattern già presente altrove
  nel file per un motivo simile, vedi i commenti sparsi sui test `peer_gone_from_server_*`).

Non si è scelto di estendere `mark_connected_for_test` per coprire ANCHE questi casi,
perché l'helper avrebbe dovuto anche "connettere" un peer che nello scenario originale
non doveva esserlo (differenza semantica reale, non solo cosmetica) — `set_link_gen_for_test`
tiene la generazione ortogonale dagli altri effetti collaterali.

`cargo test -p orchestrator`: 419 test lib (416 passed + 3 ignored, +5 rispetto a
0.26.0 — i 5 nuovi test di questa slice), 0 failed. `cargo test -p orchestrator --test
aichat_relay_loopback -- --ignored`: 2/2 (il restructuring di connect/reader non rompe
relay né keepalive — la latenza aggiuntiva dell'hop extra
spawn→connect_res_tx→ramo 5→apply_connect_outcome è nell'ordine dei microsecondi su
loopback, ben dentro i margini di attesa già presenti nel test, 100ms/300ms). `cargo
clippy -p orchestrator --all-targets`: 2 warning preesistenti (`ai_adapter.rs:173`,
`ws.rs:491`), nessuno nuovo. `cargo fmt` deliberatamente NON eseguito (drift preesistente
sull'intero crate, vedi 0.25.9/0.26.0).

---

# Implementation — orchestrator v0.26.0 (AI Chat hardening: teardown — shutdown token)

Slice B del piano di hardening AI Chat (debito #2 "teardown reale", design:
`Docs/superpowers/specs/2026-07-02-aichat-hardening-debts-design.md`, §"Slice B" —
aggiornata rispetto alla bozza originale: `CancellationToken` propagato "verso il basso"
invece di un `JoinSet` posseduto da `run`, vedi "Perché non un JoinSet" sotto). File
toccati: `src/aichat/service.rs`, `src/main.rs`, `tests/aichat_relay_loopback.rs`.

**Problema.** `run` (l'attore) possiede sia `inbox_tx` sia `new_link_tx` (li clona per
passarli a `perform`/ai task figli, ma l'originale resta vivo dentro `run` stesso). Di
conseguenza esiste SEMPRE almeno un sender vivo su entrambi i canali → `inbox_rx.recv()`
e `new_link_rx.recv()` non tornano mai `None`: i due `break` nei rami 1 e 2 del
`select!` erano irraggiungibili. Il task `run` girava per sempre; i suoi task figli
(accept loop spawnato da `perform` per `Effect::StartListener`, e i reader/writer
per-peer spawnati da `spawn_peer_tasks` — sia dall'accept loop sia da `Effect::ConnectTo`)
non erano scoped al loop e sarebbero sopravvissuti anche a un ipotetico abort esterno di
`run`. Nessun modo pulito di fermare il canale AI Chat.

**Perché `CancellationToken` e non un `JoinSet`** (raffinamento rispetto alla bozza del
design doc, richiesto dal supervisore). L'accept loop spawna a sua volta i task
reader/writer per-peer: sono "figli dei figli" rispetto a `run`. Un
`tokio::task::JoinSet` posseduto da `run` raggiungerebbe SOLO i task spawnati
direttamente lì dentro (l'accept loop) — `abort_all()` non si propagherebbe alla
gerarchia sottostante (reader/writer). Un `CancellationToken` clonato ad ogni livello
(`run` → `perform` → accept loop / `ConnectTo` → `spawn_peer_tasks` → reader e writer)
risolve il problema strutturalmente: ogni task, a qualunque profondità nell'albero,
osserva la STESSA cancellazione tramite il proprio clone del token (uno `.cancel()` su
un clone qualsiasi è visibile a tutti i cloni — stato condiviso via `Arc` interno alla
libreria `tokio-util`).

**Fix — propagazione punto per punto.**

1. `run(mut self, inbox_rx, inbox_tx, shutdown: CancellationToken)` — quarto parametro.
   Quarto ramo del `select!`: `_ = shutdown.cancelled() => break`. È l'UNICO `break` del
   loop realmente raggiungibile (i rami 1/2 restano teoricamente vivi ma di fatto morti,
   come prima — la loro presenza documenta il caso limite, non è dead code da rimuovere:
   se in futuro `inbox_tx`/`new_link_tx` smettessero di essere posseduti da `run`, questi
   `break` tornerebbero raggiungibili senza altre modifiche).
2. `perform(&mut self, eff, inbox_tx, new_link_tx, shutdown: &CancellationToken)` — quarto
   parametro, passato per riferimento (nessun bisogno di possederlo, solo di clonarlo
   dove serve).
3. `Effect::ConnectTo`: `spawn_peer_tasks(stream, peer_id, inbox_tx.clone(),
   shutdown.clone())`.
4. `Effect::StartListener`: `shutdown_acc = shutdown.clone()` clonato PRIMA dello
   `tokio::spawn` dell'accept loop. Dentro, `listener.accept()` è avvolto in
   `tokio::select! { _ = shutdown_acc.cancelled() => break, res = listener.accept() =>
   {...} }`. Ogni link accettato riceve `shutdown_acc.clone()` in `spawn_peer_tasks`. Il
   fix Slice A (`ServiceEvent::ListenerStopped` inviato all'uscita del loop, per
   resettare `self.listening`) è preservato ma condizionato:
   `if !shutdown_acc.is_cancelled() { send(ListenerStopped) }` — se l'uscita è dovuta
   allo shutdown, NON emettiamo l'evento: l'attore sta già uscendo dal proprio loop (ramo
   4), e se per una race `ListenerStopped` venisse comunque processato PRIMA del ramo di
   shutdown, `decide_and_connect()` ri-emetterebbe `StartListener` — un nuovo bind proprio
   mentre il processo si sta spegnendo, l'opposto del comportamento voluto.
5. `spawn_peer_tasks(stream, peer_id, inbox_tx, shutdown: CancellationToken)` — quinto
   parametro (owned: viene clonato due volte internamente, una per il writer task e una
   per il reader task, perché sono due `tokio::spawn` distinti). Writer: il precedente
   `while let Some(msg) = writer_rx.recv().await` diventa un `loop { select! { _ =
   shutdown_writer.cancelled() => break, maybe_msg = writer_rx.recv() => match maybe_msg {
   Some(m) => {...}, None => break } } }`. Reader: il `match
   tokio::time::timeout(DEAD_THRESHOLD, reader.read_line(&mut line)).await { ... }`
   diventa un ramo dentro `select! { _ = shutdown_reader.cancelled() => break, result =
   tokio::time::timeout(..) => { match result { ... stesso comportamento di prima ... } }
   }` — comportamento keepalive INVARIATO (timeout → `PeerGone`, EOF → `PeerGone`, errore
   → `PeerGone`), aggiunto solo il ramo di cancellazione. In entrambi i task, se il
   `select!` esce per shutdown, il `send(PeerGone)` finale del reader resta nel codice ma
   è innocuo: se l'inbox è già chiusa (l'attore è già uscito) fallisce silenziosamente.
6. `src/main.rs`: il `CancellationToken` di shutdown globale (in precedenza creato subito
   prima di `ws::serve`, riga ~511) è ora creato PRIMA del blocco "Canale AI Chat" (riga
   ~339), con un commento che spiega il perché (`service.run(...)` ne ha bisogno). Lo
   STESSO token — non uno nuovo — resta usato per il comando "q"+invio e per
   `ws::serve(...)`: un solo evento di shutdown spegne ordinatamente sia il WS sia il
   canale AI Chat. La riga di spawn diventa
   `tokio::spawn(service.run(inbox_rx, inbox_tx.clone(), shutdown.clone()))`.

**Test (TDD, RED reale via timeout — non errore di compilazione).** Il cambio di firma di
`run` richiede necessariamente che il parametro `shutdown` esista già per far compilare
il test (altrimenti sarebbe un errore di compilazione, non un test che fallisce
"per il motivo giusto"). Il RED genuino è stato ottenuto in due passi: (1) aggiunto il
solo parametro `shutdown` a `run` — SENZA cablare il quarto ramo del `select!` — poi (2)
scritto `run_exits_on_cancellation` e osservato fallire per un vero motivo applicativo
(timeout dell'assert, non un errore di tipo):

```rust
#[tokio::test]
async fn run_exits_on_cancellation() {
    let svc = AiChatService::new(info(10, "skimble"));
    let (inbox_tx, inbox_rx) = tokio::sync::mpsc::unbounded_channel::<ServiceEvent>();
    let token = tokio_util::sync::CancellationToken::new();
    let handle = tokio::spawn(svc.run(inbox_rx, inbox_tx.clone(), token.clone()));
    token.cancel();
    let result = tokio::time::timeout(std::time::Duration::from_secs(2), handle).await;
    assert!(result.is_ok(), "run() non è terminato entro il timeout dopo la cancellazione...");
}
```

RED osservato: `panicked ... run() non è terminato entro il timeout dopo la
cancellazione` (timeout di 2s scaduto per davvero — non un errore di compilazione).
GREEN: dopo aver cablato il quarto ramo del `select!`, il test passa (il `JoinHandle`
completa quasi subito, il `select!` si risveglia sul ramo `shutdown.cancelled()`).

**Copertura NON automatizzata (limite noto, verificato per ispezione).** Il test sopra
esercita SOLO il ramo 4 di `run` — nessun peer, nessun listener spawnato nello scenario.
La propagazione verso i task figli (punti 3-5 sopra: `ConnectTo`, accept loop,
reader/writer per-peer) non ha un test dedicato in questa slice — verificata a mano
punto per punto durante l'implementazione (vedi checklist nel commit). Un test end-to-end
che verifichi "un peer/listener realmente termina dopo la cancellazione" richiederebbe
I/O reale (socket TCP) ed è un candidato per un futuro test `#[ignore]` in
`tests/aichat_relay_loopback.rs`, non incluso in questa slice.

`cargo test -p orchestrator`: 414 test lib (411 passed + 3 ignored, +1 rispetto a
0.25.9), 0 failed. `cargo clippy -p orchestrator --all-targets`: 2 warning preesistenti
(`ai_adapter.rs:173`, `ws.rs:491`), nessuno nuovo. `cargo fmt` deliberatamente NON
eseguito (stesso motivo di 0.25.9 — drift preesistente sull'intero crate, vedi sotto).

---

# Implementation — orchestrator v0.25.9 (AI Chat hardening: listener reset)

Slice A del piano di hardening AI Chat (debito #5 "listening non si resetta", design:
`Docs/superpowers/specs/2026-07-02-aichat-hardening-debts-design.md`, §"Slice A"). Fix
mirato, un solo file: `src/aichat/service.rs`.

**Problema.** L'accept loop TCP (spawnato in `perform` dentro `Effect::StartListener`)
gira in un `tokio::spawn` proprio, con vita indipendente dal loop `run` dell'attore.
Quando esce dal suo `loop { ... }` — per un errore di `listener.accept()`, oppure
perché `new_link_tx_acc.send(...)` fallisce (il loop `run` è già morto) — semplicemente
termina, senza mandare nessun segnale all'attore. Il campo `self.listening` (flag di
idempotenza che in `perform` previene il doppio bind: `if self.listening { return }`)
resta bloccato a `true` per sempre: nessun futuro `Effect::StartListener` può più
ri-bindare la porta, anche dopo una nuova rielezione a `Role::Server`. Il server smette
di accettare connessioni **in silenzio**, senza log d'errore visibile e senza modo di
riprendersi da solo.

**Fix.** Nuova variante `ServiceEvent::ListenerStopped` (evento puro del dominio,
nessun dato: "l'accept loop è terminato"). L'accept loop, subito dopo l'uscita dal
`loop` (qualunque sia il motivo), esegue
`let _ = inbox_tx_acc.send(ServiceEvent::ListenerStopped);` — riusa il clone di
`inbox_tx` già catturato dalla closure per l'invio di `PeerMsg`/il resto; se il loop
`run` è già terminato il `send` fallisce innocuamente (nessun panic, nessun log). L'arm
in `handle_event`:

```rust
ServiceEvent::ListenerStopped => {
    self.listening = false;
    self.decide_and_connect()
}
```

resetta il flag e richiama SUBITO `decide_and_connect()` (stessa funzione usata da
`Discovered`/`Consent`/`PeerGone`): se il ruolo corrente è ancora `Role::Server`, questo
ri-emette immediatamente `Effect::StartListener(me.chat_port)`, che ora supera il guard
di idempotenza (appena resettato) e ri-binda per davvero — ripristino nello stesso ciclo
dell'attore, invece di aspettare passivamente il prossimo `Discovered` UDP (~5-7s in LAN
reale, il discoverer non ha nessun trigger più rapido). Se nel frattempo il ruolo non è
più `Server` (rielezione), `decide_and_connect()` produce l'effetto giusto per il nuovo
ruolo (o nessuno, se `Undecided`) — nessun ramo speciale necessario, il fix riusa
integralmente la macchina a stati esistente.

**Test (TDD, RED reale).** Due nuovi helper `#[cfg(test)]`: `listening_for_test(&self)
-> bool` (getter) e `set_listening_for_test(&mut self, v: bool)` (setter diretto, evita
di dover passare da un vero `TcpListener::bind` in `perform` solo per allestire lo stato
"ero in ascolto" in un test puro di `handle_event`). Test:
- `listener_stopped_resets_listening_flag` — `listening=true` (via setter) →
  `ListenerStopped` → `listening_for_test() == false`.
- `listener_stopped_when_server_reemits_start_listener` — costruisce uno stato
  `Role::Server` (me=.20, peer .30 con IP più alto consentito e connesso via
  `mark_connected_for_test`, così `me` resta il "lowest" del roster), forza
  `listening=true`, invia `ListenerStopped`, verifica sia il reset del flag sia la
  presenza di `Effect::StartListener(40100)` tra gli effetti ritornati.

Il RED iniziale è stato un errore di compilazione (`E0004: non-exhaustive patterns:
ServiceEvent::ListenerStopped not covered`) subito dopo l'aggiunta della sola variante
all'enum, prima di scrivere l'arm in `handle_event`: legittimo per l'iron-law TDD (il
comportamento manca davvero, non è un typo) — il match esaustivo del compilatore Rust
fa da "test" a livello di tipo. GREEN: `cargo test -p orchestrator` → 410 test lib
passed (2 nuovi + 408 preesistenti), 0 failed, 3 ignored (invariato). Clippy pulito su
`service.rs` (i 2 warning preesistenti del crate sono in `ai_adapter.rs`/`ws.rs`, non
toccati da questa slice).

**Nota su `cargo fmt`.** `cargo fmt -p orchestrator --check` segnala drift preesistente
su TUTTO il crate (465 hunk su 38 file, incluso `service.rs` da solo: ~5100 righe di
diff se riformattato da capo) — nessun `rustfmt.toml`/pin di toolchain, quindi
formattazione di default mai applicata storicamente a questo codice. Deliberatamente
NON eseguito `cargo fmt` (né sull'intero file né sull'intero crate): avrebbe prodotto
un diff enorme e sganciato da questa slice, illeggibile in review. Il codice nuovo
rispecchia lo stile a mano già presente nel file (es. `assert_eq!` su riga singola con
più argomenti, come il pattern preesistente a `consent_accept_makes_lowest_ip_start_listener`).

---

# Implementation — orchestrator v0.25.8 (AI Chat: keepalive — rilevazione disconnessioni improvvise)

Prima di questa slice nessuna disconnessione brusca (crash, cavo staccato, sospensione del
PC) veniva rilevata: un peer spariva dalla rete ma restava "presente" per sempre agli occhi
degli altri, perché il roster si aggiornava solo su EOF/errore TCP — un socket morto senza
chiusura pulita non genera nessuno dei due da solo. Il fix aggiunge un battito cardiaco
applicativo sopra TCP: `run()` in `src/aichat/service.rs` porta un
`tokio::time::interval(PING_INTERVAL)` (5s, terzo ramo del `select!` esistente) che inietta
`ServiceEvent::Tick`; l'arm — pura, testata senza I/O — manda `ChatMsg::Ping {}` (nuova
variante in `src/aichat/wire.rs`, nessun payload) a ogni peer in `self.connected`, sia da
ruolo server (N client) sia da client (1 server), lo stesso codice per entrambi. Sul lato
ricezione, il task READER spawnato da `spawn_peer_tasks` avvolge ogni `read_line` in
`tokio::time::timeout(DEAD_THRESHOLD, ...)` (15s): il timeout si resetta a ogni riga
ricevuta (Ping compreso), quindi serve silenzio continuo per l'intera soglia — circa 3 cicli
di Ping mancati — prima che il peer sia dichiarato morto con lo stesso `ServiceEvent::PeerGone`
già usato per EOF/errore di rete. Nessuna nuova via di disconnessione: il keepalive alimenta
solo il trigger esistente, non lo duplica.

L'arm `PeerGone` cattura `label`/`era_server`/`was_my_server` PRIMA delle rimozioni già in
vigore (incluso l'azzeramento di `reported_leader` del fix Bug 2 di 0.25.7, invariato), poi
si dirama sul ruolo di CHI è sparito rispetto a me: `era_server` (il mio stesso ruolo è
`Role::Server`, quindi il peer sparito era un MIO client) porta a
`AiChatChannel::on_server_leave` — nuovo in `src/aichat/channel.rs`, mirror di
`on_server_join`, che toglie l'etichetta dalla `Room` e produce lo snapshot per il broadcast
`Roster` aggiornato + `ChatMsg::PeerLost { label }` a tutti i client rimasti (già ripuliti di
`self.connected`), oltre a `ToUi(AiChatPeerLost)` per la propria UI. `was_my_server` (il peer
sparito era il server di cui io sono client) produce solo `ToUi(AiChatPeerLost)` locale —
nessun altro link da notificare; `decide_and_connect()`, invariato, gestisce già la
rielezione/riconnessione a valle. È un evento **live-only**: non entra mai in
`history`/`AiChatHistory`, coerente con la persistenza dei soli messaggi di chat (0.25.2).

Lavorando su questo task è emerso un bug preesistente, non introdotto dal keepalive:
`Room::leave()` era scritto e testato da tempo ma non veniva mai invocato da nessuna parte —
anche una disconnessione pulita non toglieva mai l'etichetta dai presenti né ribroadcastava
il roster ai client rimasti. `on_server_leave` è quindi anche il fix di quel bug, non solo il
nuovo percorso innescato dal keepalive.

Spec: `Docs/superpowers/specs/2026-07-02-aichat-keepalive-design.md`. Nessuna modifica a
`election.rs`/`decide_role`/`reported_leader`: il keepalive fornisce solo un trigger più
affidabile per `PeerGone`, riusando integralmente la rielezione esistente introdotta in
0.25.7.

---

# Implementation — orchestrator v0.25.7 (AI Chat: fix Bug 2 — late-joiner election)

Fix del bug reale osservato dal vivo su 3 macchine: un terzo peer che si unisce a una stanza
AI Chat dove esiste già un server eletto si autoeleggeva server invece di diventare client,
perché la sticky-ness di `election::elect` protegge solo il ruolo GIÀ deciso localmente da un
nodo — un processo appena avviato parte sempre con `current = None`, quindi la sua prima
elezione era un calcolo grezzo `members.lowest()` sul roster visibile in quel momento, cieco al
fatto che il gruppo avesse già un server stabilito altrove. Design pre-approvato dall'utente:
`Docs/superpowers/specs/2026-07-02-aichat-late-joiner-election-design.md`.

Il fix introduce un terzo segnale, **`reported`**, propagato lungo tutta la catena
scoperta→elezione:

- **`src/aichat/election.rs`**: `elect(members, current, reported)` — terzo parametro. Se
  `current` non sticka nulla (roster nuovo o server sparito), prova `reported`: presente E già
  nel roster consentito → lo adotta (vince sul solo IP più basso); presente ma ASSENTE dal
  roster (consenso umano non ancora dato per quel peer) → `None` (`Undecided`, mai
  autoelezione, mai `lowest()`); `reported: None` → comportamento founding invariato. 3 nuovi
  test (adotta reported-in-roster; Undecided se reported-fuori-roster; founding invariato con
  `None`). Tutte le chiamate esistenti passano `reported: None` → comportamento bit-per-bit
  invariato per la sticky-ness fondativa (`role_is_sticky_across_a_later_lower_ip` ecc.).
- **`src/aichat/channel.rs`**: `decide_role(members, reported)` — passa `reported` tale e quale
  a `elect`. Nuovo test che riproduce lo scenario esatto del bug: roster `{rumpleteazer,
  skimble, quaxo}`, `me = quaxo` con l'IP più basso di tutti, `reported =
  Some(rumpleteazer)` → `Role::Client(rumpleteazer)`, mai `Role::Server`.
- **`src/aichat/wire.rs`**: `Announce` guadagna `leader: Option<PeerId>` (`#[serde(default)]` —
  additivo, un binario non ancora aggiornato che manda annunci senza questo campo decodifica
  comunque, con `leader: None`). Il leader che il MITTENTE crede attualmente valido, incluso
  nel proprio annuncio periodico UDP (~ogni 7s). 2 nuovi test round-trip (`Some`/`None` +
  retro-compatibilità sul JSON "vecchio").
- **`src/aichat/discovery.rs`**: trait `Discoverer` esteso — `announce(&self, leader:
  Option<PeerId>)` (il leader creduto DA NOI, da incorporare nell'annuncio in uscita);
  `next(&mut self) -> Option<(PeerInfo, Option<PeerId>)>` (ritorna anche il leader che il
  MITTENTE ha riportato). `FakeDiscoverer`: coda di `(PeerInfo, Option<PeerId>)`, nuovo campo
  `last_announced_leader: Mutex<Option<PeerId>>` con getter `last_announced_leader()` per
  assertion nei test su cosa è stato passato ad `announce()`. 1 nuovo test.
- **`src/aichat/net.rs`**: `UdpDiscoverer::announce` include `leader` nell'`Announce`
  serializzato; `next` estrae anche `ann.leader` dal datagramma decodificato e lo ritorna nella
  tupla. Nessun nuovo test proprio (coperto dai round-trip di `wire.rs` + dal contratto del
  trait già testato con `FakeDiscoverer`).
- **`src/aichat/service.rs`**: `AiChatService` guadagna due campi:
  - `reported_leader: Option<PeerId>` — ultimo leader che un peer scoperto ha riportato.
    `ServiceEvent::Discovered` cambia forma in `Discovered(PeerInfo, Option<PeerId>)`; l'arm
    aggiorna `reported_leader` SOLO su `Some(_)` (un annuncio `leader: None` da un peer ancora
    indeciso non cancella un'informazione più utile già ricevuta da un altro peer) — questo è
    ciò che chiude anche la race sull'ordine dei consensi (vedi test
    `late_joiner_stays_undecided_when_reported_leader_not_yet_consented`, riproduce §4 dello
    spec: consenso a un peer minore PRIMA del leader riportato → stato intermedio `Undecided`,
    mai autoelezione, converge su `Client` non appena il leader riportato entra nel roster
    consentito). `decide_and_connect()` passa `self.reported_leader` a `decide_role`. L'arm
    `PeerGone(id)` azzera `reported_leader` se coincide con `id` — punto più delicato dello
    spec: senza questo, dopo la sparizione del vero leader `elect()` prenderebbe il ramo
    "riportato ma fuori roster" → `Undecided` invece di ricadere su `members.lowest()` tra i
    superstiti, bloccando la rielezione che oggi funziona. Verificato con RED esplicito
    (disabilitata temporaneamente la pulizia, il test falliva restando `Undecided`) prima di
    reintrodurre il fix.
  - `believed_leader: Arc<Mutex<Option<PeerId>>>` + `believed_leader_handle()` (clone
    dell'`Arc`) — scritto in `decide_and_connect()` nello stesso punto in cui si calcola
    `current_leader` per il log di elezione (non I/O reale, stesso livello di "purezza
    pragmatica" già tollerato per `tracing::info!` nella stessa funzione); letto dal task di
    scoperta UDP in `main.rs` a ogni `announce()`.
  - 8 nuovi test: `discovered_with_leader_some_updates_reported_leader`,
    `discovered_with_leader_none_does_not_clear_existing_reported_leader`,
    `peer_gone_clears_reported_leader_and_reelects_lowest_among_survivors` (regressione),
    `late_joiner_with_lowest_ip_adopts_existing_leader_instead_of_self_electing` (e2e, tre
    `AiChatService`, `handle_event` diretto senza socket, riproduce l'intero scenario del log),
    `late_joiner_stays_undecided_when_reported_leader_not_yet_consented` (variante ordine
    consensi), `believed_leader_handle_reflects_current_leader_after_election`.
- **`src/main.rs`**: `service.believed_leader_handle()` clonato PRIMA che `run(inbox_rx,
  inbox_tx)` consumi `service` (prende `self` per valore), passato alla closure del task
  discovery. Ad ogni `announce()` (iniziale e a ogni ciclo di ~7s) il valore va estratto in una
  variabile locale PRIMA dell'`.await` — `std::sync::MutexGuard` non è `Send`, quindi tenerlo
  "vivo" attraverso un punto di sospensione rende il future non-`Send` e `tokio::spawn` lo
  rifiuta a compile-time (errore osservato e corretto durante l'implementazione).
  `Discovered(peer, leader)` ricevuto da `disc.next()` inoltrato così com'è nell'inbox.

Totale test: 396 lib (72 in `aichat`, +13 rispetto a 0.25.6) + 13 `ws_integration` + 2 `main`
(is_quit_command) + 5 integration file con 1 test `#[ignore]` ciascuno (esclusi dal run
standard) + 1 doc-test = tutti passed. Clippy invariato: 2 warning preesistenti
(`ai_adapter.rs`, `ws.rs`), nessuno nuovo introdotto da questa slice.

---

# Implementation — orchestrator v0.25.6 (uscita pulita interattiva "Q")

`ws::serve()` accetta un `CancellationToken` (`shutdown`); il loop di accept usa
`tokio::select!` fra `listener.accept()` e `shutdown.cancelled()` — cancellato, il loop
si ferma e `serve()` torna `Ok(())`. `main.rs` crea il token, spawna un task
`spawn_blocking` che legge stdin (bloccante — mai su un task async direttamente) e
chiama `is_quit_command(line)` (pura, "q"/"quit" case-insensitive, spazi ignorati) per
decidere quando cancellare. Non serve nessuna logica di cleanup aggiuntiva per
`mcp-server`: il processo esce da solo quando l'OS chiude la sua pipe stdin all'uscita
dell'orchestrator (confermato leggendo `mcp-server/src/main.rs`:
`service.waiting()` risolve alla EOF). La sequenza `plugin_host.shutdown().await`
già esistente in `main.rs` gira invariata dopo che `serve()` torna. Nuovo test
d'integrazione `serve_returns_when_shutdown_token_is_cancelled` in
`tests/ws_integration.rs`. Affordance esplicitamente temporanea per l'uso
interattivo — sostituita da uno shutdown-su-stop-servizio quando si passerà ai
servizi (Fase 5 autorun).

---

# Implementation — orchestrator v0.25.5 (AI Chat: AiChatSelf per titolo finestra)

`SetServerTx` in `src/aichat/service.rs` ora emette sempre
`Effect::ToUi(ServerMsg::AiChatSelf { label })` (etichetta = `"{label_base}-human"`),
oltre ai replay condizionati esistenti (pending join-request, roster, storico). La
finestra-chat usa questa etichetta per il titolo — "chi sono io" non dipende più da
inferirlo dal roster. Ripple edit su `telegram/channel.rs` (arm no-op). Test esistente
aggiornato per riflettere che `SetServerTx` non è più "senza effetti" nel caso base.

---

# Implementation — orchestrator v0.25.4 (fix timeout risposte AI lunghe)

**Bug fix** — risposte AI lunghe (ricerca web + generazione esaustiva) fallivano con
`"[errore AI] rete: error decoding response body"`. Causa: `HttpMessagesClient` usava un
unico `.timeout()` reqwest da 120s che copre connect+send+lettura COMPLETA del body — per
uno stream SSE, un turno che supera 120s (es. più chiamate server-side `web_search`/
`web_fetch` durante la generazione) veniva tagliato a metà, e reqwest classifica un
timeout-durante-body-read anche come `is_decode()==true` col `Display` "error decoding
response body" — testo indistinguibile da una vera corruzione, perché il codice catturava
solo `e.to_string()`.

Fix (`src/messages_client.rs`): due timeout distinti invece di uno solo.
`DEFAULT_TOTAL_TIMEOUT` (900s) resta sul client `reqwest` come backstop per hang
patologici; `DEFAULT_IDLE_TIMEOUT` (90s) è nuovo, applicato per-chunk dentro
`create_streaming` via `tokio::time::timeout(idle_timeout, stream.next())` — rileva
stalli reali in fretta senza scattare durante pause silenziose legittime lato server.
`HttpMessagesClient::with_timeouts` (privato) permette ai test di iniettare timeout
brevi. `describe_network_error` antepone `"timeout: "` quando `e.is_timeout()`, così un
eventuale timeout sul backstop totale resta diagnosticabile invece di mostrare solo il
testo "decoding" di reqwest. 2 test TDD (server TCP locale che simula stream lento):
idle-timeout produce un errore che parla esplicitamente di timeout; uno stream a
trickle costante (pause sempre sotto l'idle-timeout) completa con successo anche se la
durata totale supera quello che sarebbe stato il vecchio limite.

Root cause confermata empiricamente (non solo per ispezione del codice): riprodotto con
un server TCP locale + client `.timeout()` corto, osservato `is_timeout()==true` E
`is_decode()==true` con `Display` = "error decoding response body" — match esatto col
messaggio segnalato dall'utente.

---

# Implementation — orchestrator v0.25.3 (fix guard storico su dump più corto)

Fix da review finale whole-feature (Opus) su v0.25.2: l'arm `ChatMsg::History` ora richiede
`role() != Role::Server && entries.len() >= self.history.len()` prima di sostituire lo storico
locale — altrimenti un client con un messaggio proprio non ancora relayato dal vecchio server
(morto prima del relay) lo perdeva al primo dump — più corto — dal nuovo server. 2 test nuovi
(`client_keeps_longer_local_history_when_dump_is_shorter`,
`server_ignores_history_message_from_a_client`) + 1 esistente rafforzato (parte con storico
locale non vuoto, cosicché `.extend()` al posto di `=` farebbe fallire l'asserzione).

---

# Implementation — orchestrator v0.25.2 (AI Chat: storico persistente + log elezione)

**Storico messaggi persistente in-memory.** `src/aichat/wire.rs`: nuova `ChatLine { from_label,
text }` (wire-local, `From` verso `protocol::ChatLine`) + variante `ChatMsg::History { entries }`.
`src/aichat/service.rs`: nuovo campo `history: Vec<ChatLine>`, popolato su ogni `Say` visto (locale
o da peer) indipendentemente dal ruolo. `SetServerTx` ri-emette `AiChatHistory` se non vuoto (stesso
pattern di `last_known_roster`, v0.25.1). Il server manda `ChatMsg::History` (dump completo) SOLO al
peer che manda `Join` (non broadcast); il client lo riceve, sostituisce il proprio storico, aggiorna
la UI se connessa. Chi diventa server dopo una sparizione eredita lo storico "per osmosi" — l'ha già
visto passare come client via il relay a stella esistente, nessun handoff esplicito. 8 test TDD
nuovi.

**Log elezione/cambio server.** `AiChatService.last_logged_leader: Option<PeerId>` +
`label_for(id) -> String`. `decide_and_connect` logga (`tracing::info!`) la prima elezione e ogni
cambio effettivo, confrontando il leader calcolato con l'ultimo loggato — gira su ogni peer, il
log compare su tutti gli orchestrator della stanza. 3 test TDD (stato, non testo del log — stesso
approccio già in uso per "peer scoperto una sola volta").

Spec: `Docs/superpowers/specs/2026-07-01-aichat-history-persistence-design.md`.

---

# Implementation — orchestrator v0.25.1 (Slice 1a-ui-B: fix riapertura finestra AI Chat)

**Bug fix** — presenti vuoti e nessuna ricezione dopo chiusura/riapertura finestra-chat.
`src/aichat/service.rs`: aggiunto campo `last_known_roster: Vec<String>` ad `AiChatService`;
aggiornato in `PeerMsg::Join` (server) e `PeerMsg::Roster` (client); `SetServerTx` ora ri-emette
`ToUi(AiChatRoster)` se il cache non è vuoto. `src/ws.rs`: arm `ClientMsg::AiChatOpen {}` →
`ServiceEvent::SetServerTx(out_tx.clone())`. 2 nuovi test TDD in `service.rs`.

---

# Implementation — orchestrator v0.25.0 (Slice 1a-ui-A)

Lare Terminal daemon. Implements Contratto A (WebSocket server) and
Contratto B (MCP client toward `mcp-server`) with OS/NL routing in the middle.
v0.24.0: **aichat Slice 1a-net** (core di rete del canale AI Chat). Un solo minor (0.23.0 →
  0.24.0) copre l'intera slice; i moduli si accumulano qui. `src/aichat/mod.rs` dichiara i
  submodule. `src/aichat/peer.rs`: tipi puri — `PeerId(pub Ipv4Addr)` (newtype Ord numerico),
  `PeerInfo { id, label_base, chat_port }`, `Roster` (BTreeMap-backed, `lowest()` = IP più basso
  = server eletto). `src/aichat/election.rs`: `pub fn elect(members, current)` sticky (resta se
  `current` è ancora nel roster, altrimenti `members.lowest()`). `src/aichat/relay.rs`:
  `pub fn fan_out_targets(from, me, members)` — relay a stella: filtra il roster escludendo
  l'autore (`from`) e il server stesso (`me`); con 2 peer → 0 target; con 3+ peer → n-2 target
  (n-1 se il server stesso è `from`). `src/aichat/wire.rs`: "Contratto N" — `ChatMsg` enum
  tagged (`serde tag="type" snake_case`): `Join{label}`, `Say{from_label, text}`,
  `Roster{participants: Vec<String>}`, `Leave{label}`; `Announce{v, id: PeerId, label_base,
  chat_port}` datagramma UDP broadcast con `id` serializzato come stringa IP.
  `src/aichat/discovery.rs`: `PeerTable` — parte PURA della scoperta peer (nessun I/O);
  clock iniettato (`now_ms: u64`) → test deterministici. API: `new(ttl_ms)`, `observe(info,
  now_ms)` (upsert: sovrascrive il timestamp rinnova il peer), `expire(now_ms) -> Vec<PeerId>`
  (rimuove stale, restituisce gli id decaduti), `roster() -> Roster` (snapshot membri vivi).
  Task 6 (seam): `pub trait Discoverer: Send` con `async fn announce(&self) -> io::Result<()>` e
  `async fn next(&mut self) -> Option<PeerInfo>` — seam asincrono per la scoperta UDP; impl reale
  (`UdpDiscoverer`) Task 10. `FakeDiscoverer`: in-memory con `VecDeque<PeerInfo>` (coda prefissata)
  e `AtomicUsize` (contatore `announce()`); `announce_count()` legge il contatore senza `mut`.
  5 test TDD totali nel modulo (3 PeerTable + 2 FakeDiscoverer). Tutto puro, niente I/O.
  Task 7 (seam): `src/aichat/transport.rs`: `SentChat = Arc<Mutex<Vec<ChatMsg>>>` — alias
  per il log condiviso. `pub trait PeerLink: Send`: `async fn send(&mut self, ChatMsg) ->
  io::Result<()>` + `async fn recv(&mut self) -> io::Result<Option<ChatMsg>>` (`Ok(None)` = EOF).
  `FakePeerLink { sent: SentChat, incoming: VecDeque<ChatMsg> }`: `send()` appende al log,
  `recv()` restituisce dalla coda FIFO prefissata; EOF quando `VecDeque` è vuota. 1 test TDD.
  `TcpPeerLink` rimandato a Task 10.
  Task 8: `src/aichat/room.rs`: `Room` — lista dei partecipanti per ETICHETTA (distinta da
  `Roster` per IP: un peer può ospitare più etichette). `BTreeSet<String>` interno → ordine
  stabile. API: `new()`, `join(label: String) -> ChatMsg` (idempotente, ritorna
  `ChatMsg::Roster` snapshot), `leave(label: &str) -> ChatMsg` (idem), `roster_msg() ->
  ChatMsg`, `participants() -> Vec<String>`. Pura, nessun I/O. 3 test TDD.
  Task 9: `src/aichat/channel.rs`: `AiChatChannel` — unità integrativa (logica pura, no I/O).
  `Role { Undecided, Server, Client(PeerId) }` (Copy). `AiChatChannel { me: PeerInfo, role:
  Role, room: Room }`. `new(me)` → `Undecided`. `decide_role(&mut self, members)` sticky via
  `elect`: `Server→Some(me.id)`, `Client(s)→Some(s)`, `Undecided→None`; leader→`Role`. 
  `on_server_join(label) -> (ChatMsg, Vec<String>)`: delega a `room.join`, ritorna snapshot da
  broadcastare. `server_relay(from, msg, members) -> Vec<(PeerId, ChatMsg)>`: usa
  `fan_out_targets`; ritorna coppie `(target, msg.clone())`. L'I/O reale sui `PeerLink` è in
  Task 10. 5 test TDD. Totale aichat: 30 test (lib).
  Task 10: `src/aichat/net.rs`: driver I/O reali dietro i seam `PeerLink`/`Discoverer`.
  `TcpPeerLink { reader: BufReader<ReadHalf<TcpStream>>, writer: WriteHalf<TcpStream> }` —
  impl `PeerLink`: `send` serializza `ChatMsg` come riga JSON+`\n`+flush; `recv` legge riga,
  `trim_end`, deserializza (EOF→`Ok(None)`). `from_stream(TcpStream)` split via `tokio::io::split`.
  `UdpDiscoverer { socket: UdpSocket, me: PeerInfo, broadcast_addr, buf: Vec<u8> }` —
  impl `Discoverer`: `announce` serializza `Announce` e lo invia via `send_to(broadcast_addr)`;
  `next` riceve datagrammi in loop (ignora propri annunci `ann.id==me.id`, ignora JSON invalidi),
  ritorna `PeerInfo`. `new(me, disc_port)`: bind su `0.0.0.0:disc_port` + `set_broadcast(true)`.
  Helper: `bind_listener(port)` → `TcpListener::bind(0.0.0.0:port)`; `connect_peer(addr)` →
  `TcpStream::connect(addr)` + `from_stream`. Tokio feature `"net"` aggiunta a `[dependencies]`.
  Integration test `#[ignore]`: `tests/aichat_loopback.rs` — listener su porta effimera, client
  su `127.0.0.1:PORT` (fix Windows: `0.0.0.0` come destinazione non instrada al loopback su
  Winsock), scambio `ChatMsg::Say`, assert round-trip. 30 test TDD (lib) + 1 integration test.
  Moduli aichat: peer, election, relay, wire, discovery, transport, room, channel, net.
  Principio architetturale: "tubo stupido / intelligenza locale" — la rete trasporta testo+presenza;
  l'AI è locale a ogni macchina.
v0.25.0: **aichat Slice 1a-ui-A** — orchestrazione backend headless (Tasks 2-10).
  `src/aichat/config.rs`: `AiChatConfig { enabled, label_base, chat_port }` (default spento, porta 40100);
  `load_or_generate` + `save` (JSON indentato in app-data). 2 test TDD.
  `src/aichat/service.rs`: `AiChatService` — attore principale. `ServiceEvent` enum (SetServerTx, UiClosed,
  Discovered, Consent, PeerMsg, PeerGone, HumanSay). `Effect` enum (ToUi, SendToPeer, ConnectTo,
  StartListener, Disconnect). `handle_event(event) -> Vec<Effect>` pura: decide_role su Discovered,
  gate consenso (pending/refused), server-side Join (on_server_join + broadcast Roster), relay HumanSay
  per ruolo, eco locale. `run(inbox_rx, inbox_tx)`: loop `tokio::select!` su inbox + new_link_rx;
  `perform` esegue effetti I/O (ToUi→send, SendToPeer→links[id], ConnectTo→TcpStream+spawn_peer_tasks+Join,
  StartListener→bind_listener+accept-loop idempotente, Disconnect→drop link). `spawn_peer_tasks`:
  writer task (mpsc→JSON line su WriteHalf) + reader task (BufReader<ReadHalf>→PeerMsg/PeerGone in inbox).
  10 test TDD. Debito DRY: JSON-per-riga inline vs TcpPeerLink (futuro extract).
  `tests/aichat_relay_loopback.rs`: integration test `#[ignore]` su loopback TCP — due `AiChatService`
  (server=127.0.0.1:40191, client=127.0.0.2); SetServerTx→Discovered→Consent→Join→HumanSay→assert
  AiChatMessage su server_ui_rx. PeerId mismatch su loopback documentato (benigno).
  `src/ws.rs` (Task 9): `serve`/`handle_connection` + `Option<UnboundedSender<ServiceEvent>>`; al connect
  → SetServerTx; AiChatSend→HumanSay, AiChatJoinConsent→Consent, AiChatClosed→UiClosed; al disconnect →
  UiClosed (idempotente).
  `src/main.rs` (Task 10): spawn condizionato da `aichat.json`; `detect_local_ipv4()` (UDP connect trick,
  no pacchetti; fallback 127.0.0.1); PeerInfo con IP LAN; UnboundedChannel inbox; tokio::spawn service.run
  + task discovery (loop sequenziale announce→timeout(7s,next) — evita borrow conflict &self+&mut self);
  Some(inbox_tx)/None a ws::serve. (I fix `decide_role`/`UdpDiscoverer::next` appartengono a **1a-net 0.24.0**,
  non a questa slice — vedi v0.24.0.)
  Hardening differito a 1a-ui-B (review opus): consenso lato ingresso (accept ammette peer non consentiti),
  teardown reale, head-of-line blocking su connect inline, race di riconnessione (token di generazione link),
  reset `listening`, discovery un-annuncio-per-ciclo, auto-presenza server nel Room, registro server_tx per-connessione.
  Totale test: 365 lib + 12 integration + 1 doc = 378 passed; ignored: 3 lib + 1 doc + integration #[ignore] esclusi dal run standard.
  Clippy: 2 warning preesistenti (ai_adapter.rs + ws.rs), nessun nuovo warning.
v0.23.0: **plugin Slice 1** — routing comandi plugin in `router.rs` + `ws.rs`; lazy-spawn
  via `PluginHost::activate`; window registry (`windows: HashMap<u64, String>`);
  pump task (PluginToHost → ServerMsg, forwarding su `server_tx`); split transport
  (`PluginWriter`/`PluginReader` trait); `set_server_tx` wired in `ws.rs`; `ClientMsg::PluginUiEvent`
  → `route_ui_event`; `ClientMsg::PluginWindowClosed` → `forget_window`; e2e `plugin_window_e2e.rs`
  (round-trip reale con `counter`: Activate→ShowWindow→UiEvent→UpdateWindow).
v0.22.0: **plugin host Fase 0** — `PluginHost` wired in `main.rs` (discover→eager-spawn→Init→Ready→Deinit);
  `LARE_PLUGINS_DIR` override; e2e test con `plugin-ping`.
v0.21.0: **streaming OS output via `notifications/progress` (real-time chunks).**
  `ToolClient::run_in_session` gains `progress_tx: Option<UnboundedSender<String>>`.
  `LareClientHandler` wraps `ProgressDispatcher` and implements `ClientHandler::on_progress`.
  `McpToolClient::run_in_session`: when `Some(progress_tx)`, generates a token, subscribes
  dispatcher, spawns routing task (polls `ProgressSubscriber` via `StreamExt::next`) that
  forwards each notification `message` to `progress_tx`, injects `progress_token` in args.
  `FakeToolClient::run_in_session`: when `Some`, forwards stdout lines to channel.
  `handle_os` in `core.rs` rewritten: two channels (progress_tx/rx for raw lines, chunks_tx/rx
  for assembled `ServerMsg::Chunk`); chunk_task translates lines→Chunks concurrently;
  fallback path for non-streaming tools; stderr appended; Done always last.
  `CwdTrackingToolClient`: forwards `progress_tx` to inner client transparently.
  `agent.rs::dispatch_tool`: calls `run_in_session(command, None)` (AI path, no streaming).
  `futures = "0.3"` added to Cargo.toml (StreamExt). 4 new TDD tests; count: 301 lib, 3 ignored.
v0.20.0: **stop button + watchdog tool 90s.**
  `AiAdapter::respond` guadagna `cancel: Option<CancellationToken>` (penultimo parametro,
  prima di `tx`). `ClaudeAdapter::respond` controlla il token ALL'INIZIO di ogni iterazione
  del loop AI: se cancellato → `history.truncate(snapshot)` (igiene ruoli) + emette
  `Chunk("⛔ Operazione annullata.")` + `Done{exit_code: Some(130)}` → return.
  `handle_command` + `handle_nl` in `core.rs` guadagnano lo stesso parametro (passthrough).
  `ws.rs` ristrutturato: (1) reader task separato — `source` spostato in `tokio::spawn`,
  deserializza e invia su `in_rx: UnboundedChannel<ClientMsg>`; il loop principale legge da
  `in_rx` (nessun I/O WS bloccante); (2) comandi non-find SPAWNATI in `tokio::spawn` separato
  (history in `Arc<Mutex<...>>`); (3) `commands: HashMap<String, CancellationToken>` registra
  i comandi in corso; `CancelCommand{id}` fa `commands.remove(&id).cancel()`; cleanup
  all-drain a chiusura connessione + `reader.await`.
  `McpToolClient::run_in_session`: `tokio::time::timeout(90s)` su `peer.call_tool`; scaduto →
  `*peer_guard = None` (reset connessione) + `CommandResult{stdout: "⚠ Timeout…", exit_code:-1}`.
  `tokio` + feature `"time"` aggiunto a `Cargo.toml`. Nessuna dipendenza nuova (tokio-util
  era già presente). 5 nuovi test TDD; count totale: 299 lib + 11 integration.
v0.18.0: **search — esclusione cartelle di sistema + fan-out di un livello** (Task 1-3).
  `PathProvider::system_excludes() -> Vec<String>` (metodo con default `vec![]`);
  `OsPathProvider` implementa la lista Windows (`Windows`, `Program Files`, `ProgramData`,
  `AppData`, ...). `PathsConfig.version: u32` + `CURRENT_CONFIG_VERSION = 1`: migrazione
  automatica dei config esistenti (`version < 1`): rimuove voci path-like (con `/`/`\`),
  unisce i `system_excludes`, riscrive il file; idempotente; rispetta le rimozioni utente.
  `default_exclude()` ridotta a `["node_modules", ".git"]` (la voce morta `AppData/Local/Temp`
  rimossa). `Root.max_depth: usize`: ogni root porta il proprio budget di profondita; standard
  riceve `cfg.max_depth`. `resolve_roots` fa fan-out di un livello: figli non esclusi e non gia
  root diventano sotto-root con `max_depth - 1` e stessa `source`; aggiunti al `prune` del padre
  (nessun double-scan); `read_dir` fallisce silenziosamente. `walk_root` firma aggiornata
  (7 -> 6 param: `max_depth` rimosso, letto da `root.max_depth`). `PauseGate`/#7 invariati.
v0.14.0: **cwd reale tracciata** (Task 3). `CommandResult` guadagna `cwd: String`
  (parsato dal JSON di mcp-server 0.4.0; `""` se assente). `CwdTrackingToolClient`
  (decorator, `cwd_tracking.rs`) avvolge il tool client reale e aggiorna
  `Arc<Mutex<String>> cwd_state` dopo ogni `run_in_session` con cwd non-vuota.
  `main.rs`: `set_current_dir(home_dir())` best-effort + `cwd_state` init + wrapping.
  `ws::serve` guadagna `cwd_state` param; `handle_connection` emette `ServerMsg::Cwd`
  iniziale (dopo ServerInfo, se non vuota) e dopo ogni comando se la cwd è cambiata.
  `/find` usa `cwd_state` al posto di `current_dir()` quando il client non fornisce `cwd`.
  `telegram/channel.rs`: `format_response` gestisce `ServerMsg::Cwd` (ignorato, infrastruttura).
v0.13.0: **gate in blocco** (Task 9). `respond` raccoglie le label dei tool
  soggetti al gate del turno e fa UNA sola `confirmer.confirm(labels.join("\n"))`, verdetto
  applicato a tutti (N aperture → 1 conferma). Firma `confirm(&str)` invariata → nessun
  ripple su TelegramConfirmer/test 0.12.0. Test `confirmer_batches_multiple_tools_in_one_turn`.
v0.13.0: **ws refactor + /find + CancelSearch** (Task 7). Canale d'uscita
  persistente (un task scrittore possiede il `sink`; tutto passa da `out_tx`) → il
  read-loop resta responsivo durante una ricerca. `/find` gestito nel transport: ricerca
  SPAWNATA con id proprio + `CancellationToken` nel registro `id→token` (solo read-loop,
  no Mutex); `CancelSearch{id}`→cancel; cancel-all alla chiusura. Comandi non-/find inline
  (history safe). `search::SearchContext{engine,cfg,provider}` (Clone) + `search::launch`
  (resolve_roots+engine.run). `main.rs` costruisce il context da `search-paths.json`
  (app-data). cwd ricerca = Command.cwd ?? current_dir (best-effort, non la shell mcp).
  Test in `tests/ws_integration.rs` (streaming /find, responsività, cancel non-blocca).
v0.13.0: **SearchEngine::run** — orchestrazione multi-root mpsc + semaforo + cap (Task 6).
  `SearchEngine { max_concurrency: usize }` (Default=8). `run`: emette SearchOpen; `Arc<Matcher>` +
  `Arc<Vec<String>>`; `mpsc::channel::<Hit>(256)` + `Arc<Semaphore::new(max_concurrency)`;
  spawn per root con `acquire_owned()` (gating, `_permit` non `_`); `drop(hit_tx)` dopo spawn;
  drain `hit_rx.recv()` → SearchHit; se `count >= result_cap` → `cancel.cancel()`, truncated;
  emette SearchDone. `#[allow(clippy::too_many_arguments)]`. 2 test (TDD).
v0.18.0/v0.13.0: **search::walk** — camminata async di un root.
  `Hit { path: String, source: SearchSource }`. `normalize_path_string`: rimuove
  prefisso verbatim Windows (`\\?\UNC\` -> `\\`; `\\?\` -> niente). `walk_root`:
  `spawn_blocking` + `WalkDir::max_depth(root.max_depth)` + `filter_entry` (prune per path,
  exclude per nome componente); `matcher.is_match(filename)` -> `tx.blocking_send(hit)`;
  `gate.wait_while_paused(&cancel)` + `cancel.is_cancelled()` a ogni iterazione. Firma:
  `walk_root(root, matcher, exclude, tx, cancel, gate)` (6 param; `max_depth` rimosso in v0.18.0,
  `gate` aggiunto in v0.17.0). Dipendenze: `walkdir = "2"`, `tokio-util = "0.7"`. 6 test.
v0.18.0/v0.13.0: **search::roots** — `Root{path, source, prune, max_depth}` + `resolve_roots`.
  Priorita: Cwd -> Standard -> Cloud -> External. Canonicalizza, scarta path inesistenti,
  scarta duplicati esatti (vince il primo/piu prioritario), calcola `prune`.
  v0.18.0: fan-out di un livello — ogni root sopravvissuto viene espanso nei figli di primo
  livello non esclusi e non gia root; figli: `max_depth = parent.max_depth - 1`, stessa
  `source`, aggiunti al `prune` del padre. `read_dir` fallisce silenziosamente.
  Path tenuti nella forma verbatim (`\\?\`) su Windows. 4 + 4 test (TDD, tempfile).
v0.18.0/v0.13.0: **search::paths_config** — config JSON percorsi ricerca + `PathProvider` per-OS.
  `PathProvider` trait (Send+Sync) iniettato in test tramite `struct Fake` locale.
  `OsPathProvider`: `dirs` crate per standard dirs; Windows-only cloud (Dropbox info.json, %OneDrive%);
  Windows-only drive ext (lettere A-Z != C:); v0.18.0 aggiunge `system_excludes()` (lista Windows).
  `#[cfg(windows)]` / `#[cfg(not(windows))]`.
  `ExternalMode` serde custom: "auto" <-> Auto, array <-> List. `PathsConfig::load_or_generate`:
  carica oppure (assente/corrotto) rigenera+salva; v0.18.0: migra automaticamente se `version < 1`.
  Espansione ~ e %VAR%/$VAR best-effort. Dipendenza aggiunta: `dirs = "5"`. 11 + 3 test.
v0.13.0: **search::query** — parsing query glob/token-AND per ricerca file (Task 2).
v0.13.0: stub match per le nuove varianti protocol (SearchOpen/Hit/Done, CancelSearch) in ws.rs/channel.rs — placeholder per Task 7/11.
v0.12.0: **gate conferma in-loop sui tool dell'AI da Telegram** (ADR-007 fast-follow).
  `ToolConfirmer` trait + gate nel ramo `else` di `ClaudeAdapter::respond`
  (`run_in_session`/`open_target`; `None` = UI locale autonoma, invariata).
  `TelegramConfirmer` (in `telegram::confirm`) guida il polling single-thread sullo
  stesso `offset` (`AtomicI64`, ex `Cell`, per rendere `Send` la future di `respond`);
  id opaco, ri-verifica la sessione PRIMA e AL TAP. `telegram::channel`: `offset` →
  `AtomicI64` + skip-guard nel loop `run`; `run_command_buffered` costruisce il
  confirmer (prestiti disgiunti da `&mut history`) e passa `Some(&confirmer)`.
  Fix UX (da e2e): tap a sessione scaduta → "Sessione scaduta. Usa /login" (non più silenzio).
v0.11.4: fix `telegram::auth::init` (QR TOTP mostrato finché non appaiato).
v0.11.3: QR code a terminale al primo avvio Telegram (`telegram::qr` + wiring in `main.rs`).
v0.11.0: canale Telegram completo (ADR-007) — settings, client, auth (TOTP + pairing),
gate sicurezza, polling loop, wiring `main.rs` + `telegram::run_channel`.

v0.10.0: comando `/help` → `OpenWindow{kind:Help}` + `Done{0}` (finestra sistema con lista comandi).

Dipendenze Telegram: `totp-rs = { version = "5", features = ["gen_secret", "otpauth"] }`,
`qr2term = "0.3"` (QR a terminale), `rand = "0.8"` (già presente), `async-trait` (già presente).

---

## Plugin host (Fase 0 — v0.22.0)

Wired in `main.rs` tra il blocco `search` e `ws::serve`.

### Sequenza di avvio

```
LARE_PLUGINS_DIR  (env override)
       ↓ oppure
%LOCALAPPDATA%\dev.lare.terminal\plugins\
       ↓
discover(&plugins_dir)  →  Vec<DiscoveredPlugin>
       ↓
PluginHost::start(discovered, factory, &storage_root).await
  per ogni Eager:
    ChildPluginTransport::spawn(&bin_path)  →  stdout/stdin pipe
    send(Init { protocol_version, config: Null, storage_dir })
    recv() → Ready { name, protocol_version }  →  RunningPlugin
       ↓
ws::serve(...).await   ← il server WebSocket gira normalmente
       ↓
plugin_host.shutdown().await
  per ogni running:  send(Deinit {})
  clear running vec  →  kill_on_drop chiude i processi
```

### Struttura modulo (`src/plugins/`)

```
src/plugins/
├── mod.rs         — pub mod discovery | host | spawn_policy | transport
├── discovery.rs   — discover(dir) → Vec<DiscoveredPlugin>; parse plugin.json + localizza binario
├── spawn_policy.rs— spawn_policy(triggers) → Eager | Lazy (puro)
├── transport.rs   — PluginTransport trait (send/recv) + ChildPluginTransport + FakePluginTransport
└── host.rs        — PluginHost { running: Vec<RunningPlugin> }; start<F>() + shutdown()
```

**Dimensione finestra dal manifest (v0.40.12).** `plugin_msg_to_server(m, window: Option<plugin_protocol::WindowSize>)` popola i campi `width`/`height` di `ServerMsg::OpenPluginWindow` a partire dalla dimensione dichiarata nel manifest — solo per `ShowWindow` (per `UpdateWindow`/`CloseWindow` il parametro è ignorato). Il pump task in `host.rs::spawn_and_handshake` cattura `p.manifest.window` (Copy) e lo passa a ogni traduzione. `None` ⇒ l'UI usa il default 480×360.

### Env vars

| Var | Default | Ruolo |
|-----|---------|-------|
| `LARE_PLUGINS_DIR` | `%LOCALAPPDATA%\dev.lare.terminal\plugins\` | Override dir dei plugin (utile per test e2e) |

### Test e2e

`crates/orchestrator/tests/plugin_e2e.rs` — test `#[ignore]` che:
1. Localizza `target/debug/ping.exe` (richiede build preventiva).
2. Allestisce `plugins/ping/` in una tempdir (`plugin.json` + copia binario).
3. Chiama `discover` → `PluginHost::start` → `assert!(running.len() == 1)` → `shutdown`.

Run: `cargo build -p plugin-ping && cargo test -p orchestrator --test plugin_e2e -- --ignored`

---

## Architecture — layered (SOLID)

```
src/
├── lib.rs              — module declarations, crate-level doc
├── main.rs             — entry point: token, AI adapter, tool client, plugin host, ws::serve
├── agent.rs            — funzioni pure per il loop tool-use (system prompt, defs, truncate, dispatch)
├── router.rs           — pure fn classify(input, CommandKind) -> Route
├── ai_adapter.rs       — AiAdapter trait + StubAdapter + ClaudeAdapter (loop + streaming Slice 4)
├── messages_client.rs  — MessagesClient trait (create + create_streaming) + SseAccumulator + tipi Block/Message/History/ToolDef + HttpMessagesClient + FakeMessagesClient(test)
├── tool_client.rs      — ToolClient trait + FakeToolClient + McpToolClient + CommandResult (con cwd)
├── cwd_tracking.rs     — CwdTrackingToolClient (decorator: aggiorna cwd_state dopo run_in_session)
├── core.rs             — handle_command (transport-agnostic nucleus, emette su tx)
├── test_support.rs     — helper collect<F,Fut> per test (solo #[cfg(test)])
├── ws.rs               — WebSocket server (Contratto A transport, produttore/consumatore concorrenti)
├── plugins/            — sistema plugin (Fase 0)
│   ├── mod.rs          — pub mod discovery|host|spawn_policy|transport
│   ├── discovery.rs    — discover(dir) -> Vec<DiscoveredPlugin>; parse plugin.json + localizza binario
│   ├── spawn_policy.rs — spawn_policy(triggers) -> Eager | Lazy (puro)
│   ├── transport.rs    — PluginTransport trait (send/recv); ChildPluginTransport (stdio reale); FakePluginTransport (test)
│   └── host.rs         — PluginHost { running: Vec<RunningPlugin> }; start<F>() + shutdown()
├── search/
│   ├── mod.rs          — SearchEngine{max_concurrency} + run(id,title,query,roots,cfg,tx,cancel); pub mod query|paths_config|roots|walk
│   ├── query.rs        — parse_query(q: &str) -> Matcher; enum Matcher { Glob | Tokens }; is_match (puro)
│   ├── paths_config.rs — PathProvider trait; OsPathProvider; PathsConfig (load_or_generate, expanded_*); ExternalMode
│   ├── roots.rs        — Root{path, source, prune, max_depth}; resolve_roots(cwd, provider, cfg) -> Vec<Root> (fan-out 1 livello)
│   └── walk.rs         — Hit{path, source}; walk_root(root, matcher, exclude, tx, cancel, gate); normalize_path_string
└── telegram/
    ├── mod.rs          — pub mod auth|channel|client|gate|qr|settings; pub use TelegramChannel; pub async fn run_channel(...)
    ├── settings.rs     — TelegramSettings { token } + load(path) -> io::Result<Option<TelegramSettings>>
    ├── client.rs       — TelegramClient trait + HttpTelegramClient + tipi Bot API + FakeTelegramClient(cfg(test))
    ├── auth.rs         — TelegramState (persist) + AuthResult + Authenticator (pairing/TOTP/sessioni/rate-limit)
    ├── gate.rs         — needs_confirmation(input,kind)->bool + PendingGates (single-use, TTL 2min)
    ├── qr.rs           — render_terminal_qr(data: &str) -> Result<String, String> (QR a caratteri per stderr)
    ├── confirm.rs      — confirm_via_poll(..., &AtomicI64, max_polls)->bool + TelegramConfirmer (impl ToolConfirmer: gate in-loop dei tool AI)
    └── channel.rs      — TelegramChannel + format_response + dispatch + run + purge_expired per-update

tests/
├── ws_integration.rs  — WS integration tests (ephemeral port, FakeToolClient)
└── plugin_e2e.rs      — e2e #[ignore]: ping spawn reale Init→Ready→Deinit
```

### search::query (v0.13.0 — Task 2)

Modulo puro (zero dipendenze interne a crate, solo `globset` esterna) che implementa
il Matcher per query di ricerca su nome file. Separato perché usato da task successivi
senza dipendenze da core, ws, telegram.

**Interfaccia:**
- `parse_query(q: &str) -> Matcher`: la query è interprtata come
  - **Glob** (case-insensitive): se contiene `*` o `?` (ad es. `"*.pdf"`, `"report-202?.xlsx"`).
    Usa `globset::Glob::new(&q.to_lowercase()).compile_matcher()`.
  - **Token-AND** (case-insensitive, order-free): altrimenti la query è splittata per whitespace,
    tutti lowercased, e un filename corrisponde se contiene TUTTI i token
    (ad es. `"machine learning"` → `["machine", "learning"]` → match se entrambi in nome).
  - Query vuota → `Tokens(vec![])` che non corrisponde mai.
- `impl Matcher { pub fn is_match(&self, filename: &str) -> bool }`: test del nome file (puro, niente I/O).

**Dipendenza:** `globset = "0.4"` (aggiunta in Cargo.toml).

**Test:** 4 casi (globstar, glob question, token-AND, empty); tutti green.

### Key SOLID decisions

| Principle | Implementation |
|-----------|---------------|
| SRP | `agent` solo helpers puri; `core` solo handle; `ws` solo transport; `ai_adapter` solo cervello; `search::query` solo parsing Matcher |
| OCP | Nuovo AI backend = nuova impl `AiAdapter`; nuovo tool = nuova branch in `agent::dispatch_tool` |
| DIP | `core::handle_command` dipende da `&dyn AiAdapter` + `&dyn ToolClient` + `&mut ConversationHistory`, non dai concreti |
| ISP | `AiAdapter::respond` porta storia e tools per-chiamata; adapter resta stateless e `Arc`-condiviso |

---

## Modules

### `agent`

Funzioni pure/dispatch per `ClaudeAdapter::respond`. SRP: separate da `ai_adapter` per testabilità.

```rust
pub const MAX_ITERATIONS: usize = 8;
pub const TRUNCATE_HEAD: usize = 6144;
pub const TRUNCATE_TAIL: usize = 2048;
pub const SYSTEM_PROMPT: &str = "...";  // 3 tool: run_in_session, open_target, show_markdown

// Flag per-turno che modellano la richiesta all'AI.
pub struct TurnOptions { pub allow_windows: bool, pub web_search: bool }
impl Default for TurnOptions  // allow_windows=true, web_search=false

pub fn tool_defs() -> Vec<ToolDef>                    // 3 tool defs custom (Slice 3: += show_markdown)
pub fn tools_for(opts: TurnOptions) -> Vec<ToolSpec>  // custom (- show_markdown se !allow_windows) + server-side se web_search
pub fn markdown_window(input: &Value) -> (String, String)  // (title, content) char-safe ≤60
pub fn truncate_for_model(s: &str) -> String          // char-safe head+tail+marcatore
pub fn display_invocation(name: &str, input: &Value) -> String
pub async fn dispatch_tool(tools: &dyn ToolClient, name: &str, input: &Value) -> (String, bool)
```

`dispatch_tool` combina `stdout`+`stderr` con `\n`; output vuoto → `"(nessun output, exit_code=N)"`.
`truncate_for_model` usa `is_char_boundary` per non tagliare a metà un codepoint UTF-8 multi-byte.
`markdown_window` deriva il titolo: `input.title` (se non vuoto, ≤60 char) → prima riga non vuota di `content` (char-safe) → fallback `"Lare — Output"`.

### `router`

```rust
pub enum Route { Os, Nl, Slash }
pub fn classify(input: &str, command_type: CommandKind) -> Route
```

Rules (ADR-012 refinement di `Docs/03-protocol.md`):

| priority | condition | result |
|---|---|---|
| 1 | `input.trim()` starts with `/` | `Route::Slash` |
| 2 | `command_type = Os` | `Route::Os` |
| 2 | `command_type = Nl` | `Route::Nl` |
| 3 | `Auto` + starts with `$` | `Route::Os` |
| 3 | `Auto` + first token in `SHELL_TOKENS` | `Route::Os` |
| 3 | `Auto` otherwise | `Route::Nl` |

### `messages_client`

Seam DIP per la chiamata HTTP alla Anthropic Messages API.

```rust
#[async_trait]
pub trait MessagesClient: Send + Sync {
    async fn create(&self, req: MessagesRequest) -> Result<MessagesResponse, MessagesError>;

    /// Variante streaming (Slice 4): chiama `on_text` per ogni delta di testo,
    /// ritorna la `MessagesResponse` assemblata. Default: fallback non-streaming
    /// (chiama `create`, emette testo intero in un unico delta).
    /// `HttpMessagesClient` fa l'override con SSE reale.
    async fn create_streaming(
        &self,
        req: MessagesRequest,
        on_text: &mut (dyn FnMut(&str) + Send),
    ) -> Result<MessagesResponse, MessagesError>;
}

// Wire types
pub enum Block {
    Text { text: String },
    ToolUse { id, name, input: Value },
    ToolResult { tool_use_id, content, is_error: bool },
    #[serde(other)] Unknown,  // scarta thinking e blocchi futuri
}

pub struct Message { pub role: String, pub content: Vec<Block> }
pub struct ConversationHistory { messages: Vec<Message> }  // per-connessione, owned da ws.rs
pub struct ToolDef { pub name, pub description, pub input_schema: Value }

pub struct ServerTool { kind: String, name: String, max_uses: Option<u32> }  // tipo, nome, cap
#[serde(untagged)]
pub enum ToolSpec { Custom(ToolDef), Server(ServerTool) }  // untagged: wire invariata per custom
impl ToolSpec { pub fn name(&self) -> &str }

pub struct MessagesRequest {
    model, max_tokens,
    #[serde(skip_serializing_if = "String::is_empty")] system: String,
    #[serde(skip_serializing_if = "Vec::is_empty")] tools: Vec<ToolSpec>,  // custom + server-side
    messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")] thinking: Option<Thinking>,
}
```

`MessagesResponse::assistant_blocks()` filtra solo `Text` e `ToolUse` per il turno `assistant` in storia (scarta `Unknown`).

`HttpMessagesClient` usa `reqwest` 0.12 + rustls-tls + feature `stream`, timeout 120s. L'override `create_streaming` aggiunge `"stream": true` al body JSON e legge la risposta come `bytes_stream()` → `SseAccumulator`.

`SseAccumulator` (privato): parser puro con byte-framing corretto.
- `feed(&mut self, bytes: &[u8], on_text: &mut dyn FnMut(&str))`: accumula byte nel buffer interno, estrae eventi completi (separati da `"\n\n"`), chiama `on_text` per ogni `text_delta`, accumula `input_json_delta` per i blocchi `tool_use`.
- `finish(self) -> MessagesResponse`: assembla blocchi in `content: Vec<Block>` + `stop_reason` + `stop_details`.
- **Byte-framing:** i chunk di rete non sono un-evento-per-chunk; i byte vengono bufferizzati e gli eventi decodificati solo quando completi (un carattere multibyte spezzato a cavallo di due chunk resta nel buffer non decodificato).

`FakeMessagesClient` (test): `sequence(vec)` / `repeating(resp)` / `ok/err`; `recorded/nth_request/request_count`. Il default `create_streaming` (fallback non-streaming) emette il testo completo in un unico delta → i test esistenti restano invariati.

**`on_heartbeat` (v0.41.2, esteso v0.41.3)** — gemello di `on_text` ma senza payload: chiamato da
`SseAccumulator::handle_event` su DUE eventi SSE, non solo uno — evidenza reale di attività sul
wire in entrambi i casi:
- evento `ping` (keep-alive nativo di Anthropic, prima scartato in silenzio nel ramo catch-all);
- ogni evento `input_json_delta` (v0.41.3 — lo streaming dell'input di un tool-call, es. il
  contenuto di `show_markdown`). Un turno che streamma SOLO tool-input (zero testo, zero ping)
  è la ricostruzione più plausibile dell'incidente da 184s di silenzio che ha motivato l'intera
  feature: prima di questo fix quel traffico non faceva scattare né `on_text` né `on_heartbeat`.
  Test: `input_json_delta_events_trigger_on_heartbeat_without_any_ping`.

Firma identica end-to-end da `MessagesClient::create_streaming` a `ChatBackend::send_turn`
(`chat_backend.rs`) a `LlmAdapter::respond` (`ai_adapter.rs`), che lo trasforma in
`ServerMsg::Heartbeat{id}` sullo stesso canale WS di `on_text`/`Chunk`. `OpenRouterBackend`
accetta il parametro ma non lo chiama mai (REST non-streaming, una singola chiamata bloccante
senza eventi intermedi da riportare). Vedi
`Docs/superpowers/specs/2026-08-07-ai-turn-heartbeat-watchdog-design.md`.

### `ai_adapter`

```rust
/// Gate di conferma per i tool dell'AI (Task 2). Send + Sync obbligatori:
/// senza supertrait, Option<&dyn ToolConfirmer> sarebbe !Send e ws.rs non compilerebbe.
#[async_trait]
pub trait ToolConfirmer: Send + Sync {
    async fn confirm(&self, command: &str) -> bool;
}

#[async_trait]
pub trait AiAdapter: Send + Sync {
    #[allow(clippy::too_many_arguments)]
    async fn respond(
        &self, id: &str, input: &str,
        history: &mut ConversationHistory,
        tools: &dyn ToolClient,
        opts: agent::TurnOptions,               // allow_windows + web_search per-turno
        confirmer: Option<&dyn ToolConfirmer>,  // None = UI fidata; Some(c) = gate Telegram
        tx: UnboundedSender<ServerMsg>,         // posseduto, droppato a fine respond
    );
    fn provider(&self) -> String { "unknown".to_string() }
}

pub struct StubAdapter;   // respond = Chunk("[stub AI] ricevuto: {input}") + Done(None)
pub struct ClaudeAdapter { messages: Arc<dyn MessagesClient>, model: String, max_tokens: u32 }
```

`ClaudeAdapter::respond` — loop tool-use (max `MAX_ITERATIONS=8`) con streaming (Slice 4):
1. `let snapshot = history.len()`; poi `history.push(Message::user_text(input))`
2. Definisce closure `on_text = |delta: &str| { tx.send(Chunk{id, delta}) }` (cattura `tx`/`id` per riferimento condiviso)
3. Costruisce `MessagesRequest` con system + `tools_for(opts)` (custom ± show_markdown + server-side se web_search) + `history.to_vec()` + `thinking: None`
4. `messages.create_streaming(req, &mut on_text)` — ogni delta di testo viene inviato su `tx` via `on_text`; errore → `Chunk("[errore AI] …") + Done`
5. `stop_reason == "refusal"` → `Chunk("[AI: richiesta rifiutata — {cat}]") + Done`
6. Turno assistant: se `blocks.is_empty()` NON pushare (evita HTTP 400 su turno vuoto / `pause_turn`); altrimenti `history.push(Message::with_blocks("assistant", blocks))`
7. Testo già inviato in streaming delta-by-delta in passo 4 — non ri-emesso
8. Raccogli `Block::ToolUse` dalla risposta
9. Se nessun tool → `Done` (o `Chunk("[…troncata: max_tokens]") + Done`)
10. Per ogni tool (Slice 3 + Task 2):
    - Se `name == "show_markdown"` (orchestrator-native): `agent::markdown_window(&input)` → `emit!(OpenWindow{Markdown})` + `emit!(Chunk("📄 finestra aperta: {title}"))` + `ToolResult{ack}`. (Non passa dal gate.)
    - Altrimenti (run_in_session / open_target): **gate Task 2** — `confirmer.confirm(&label).await`:
      - `None` (UI locale) → esecuzione autonoma.
      - `Some(c)` + `true` → `dispatch_tool` → `emit!(Chunk("{label}"))` + `ToolResult{output_troncato}`.
      - `Some(c)` + `false` → `emit!(Chunk("{label} — annullato"))` + `ToolResult{is_error:true, "Comando annullato dall'utente."}` (tool non eseguito).
11. `history.push(Message::with_blocks("user", results))`; torna al passo 3
12. Cap raggiunto: `Chunk("[…limite di iterazioni raggiunto…]") + Done`

**Macros `emit!`:** invia un `ServerMsg` su `tx`; se il ricevitore (WS) è andato (`tx.send` ritorna `Err`), abbandona immediatamente (stop-on-send-error). `tx` è `UnboundedSender` → `send` prende `&self` → utilizzabile più volte senza consumarlo.

**Canale mpsc unbounded:** `tx` è posseduto da `respond`; viene droppato al termine della funzione, chiudendo il canale e segnalando la fine al consumatore in `ws.rs`.

**Igiene storia (alternanza ruoli):** sui path non-puliti (errore HTTP passo 4, `refusal` passo 5, cap passo 12) `history.truncate(snapshot)` ripristina la storia → uno scambio fallito non resta in `history`, così la chiamata successiva non produce mai turni `user` consecutivi (che l'API può rifiutare con 400). Su `end_turn` la storia termina con un turno `assistant`.

**`Done.exit_code` discrimina successo da fallimento (0.40.36, fix data-loss):** `done()` (`exit_code: None`) e `fail()` (`exit_code: Some(1)`) sono due closure gemelle vicine all'inizio di `respond`. `fail()` è usata SOLO nei tre percorsi che non hanno testo vero da mostrare come contenuto valido — errore backend/API (passo 4), rifiuto (passo 5), risposta degenere (passo 9, ramo `blocks.is_empty()`) — perché in tutti e tre i casi il Chunk emesso è un placeholder d'errore ("[errore AI] …", "[AI: richiesta rifiutata — …]", "[nessuna risposta dall'AI — riprova]"), non testo dell'AI. Il ramo `MaxTokens` (stesso punto del passo 9, ma `blocks` non vuoto) e il fallthrough di completamento normale restano `done()`: contengono testo reale, anche se troncato. Un consumatore che tratta "buffer non vuoto + `Done`" come sinonimo di successo (es. `window.js`'s canale `library-expand`, che scrive il buffer su disco) deve controllare `exit_code` PRIMA di fidarsi del contenuto — vedi `isAiTurnFailure` in `crates/ui/frontend/expand-prompt.mjs`. Precedente già presente in questa stessa funzione: la cancellazione (passo iniziale del loop) emette `Done{exit_code: Some(130)}` per lo stesso motivo.

### `tool_client`

```rust
pub struct CommandResult { pub stdout: String, pub stderr: String, pub exit_code: i32 }
pub struct OpenResult    { pub ok: bool, pub message: String }

#[async_trait]
pub trait ToolClient: Send + Sync {
    async fn run_in_session(&self, command: &str) -> CommandResult;
    async fn reset_session(&self);
    async fn open_target(&self, target: &str) -> OpenResult;
}
```

`McpToolClient` mantiene connessione persistente (`Arc<Mutex<Option<Peer>>>`). Prima chiamata → spawn `mcp-server`, MCP handshake, store peer. Successive → riuso (session state persiste).

### `telegram::settings`

```rust
// No `#[derive(Debug)]` — il token non deve mai comparire nei log.
pub struct TelegramSettings { pub token: String }

/// Ok(None) = file assente (canale disattivato)
/// Ok(Some(_)) = file valido
/// Err(_) = file illeggibile / JSON malformato / token vuoto
pub fn load(path: &Path) -> std::io::Result<Option<TelegramSettings>>
```

Formato file: `{ "token": "<non-vuoto>" }`. Campi extra ignorati.

Sicurezza: il token non compare nei messaggi `io::Error` prodotti da `load`.

### `telegram::client`

```rust
// Tipi Bot API (solo campi usati, #[serde(default)] dove utile)
pub struct Update      { pub update_id: i64, pub message: Option<Message>, pub callback_query: Option<CallbackQuery> }
pub struct Message     { pub chat_id: i64, pub text: Option<String> }        // da message.chat.id via struct wire
pub struct CallbackQuery { pub id: String, pub chat_id: i64, pub data: Option<String> } // da callback_query.message.chat.id
pub struct InlineButton  { pub text: String, pub callback_data: String }     // solo Serialize

#[async_trait]
pub trait TelegramClient: Send + Sync {
    async fn get_updates(&self, offset: i64, timeout_s: u32) -> Result<Vec<Update>, String>;
    async fn send_message(&self, chat_id: i64, text: &str) -> Result<(), String>;
    async fn send_buttons(&self, chat_id: i64, text: &str, buttons: &[InlineButton]) -> Result<(), String>;
    async fn answer_callback(&self, callback_id: &str) -> Result<(), String>;
}

// No `#[derive(Debug)]` — `base` contiene il token.
pub struct HttpTelegramClient { client: reqwest::Client, base: String }
impl HttpTelegramClient { pub fn new(token: &str) -> Self }
```

`HttpTelegramClient` dettagli chiave:
- `get_updates`: timeout reqwest per-request = `timeout_s + 10s` (la ragione: un timeout fisso lato client ≤ `timeout_s` taglierebbe la long-poll prima che Telegram risponda).
- Tutti gli errori reqwest usano `e.without_url()` prima di `.to_string()` (l'URL contiene il token).
- Lo status HTTP è incluso negli errori senza il corpo della risposta (che potrebbe contenere dati sensibili).
- `answer_callback` usa il campo wire `callback_query_id` (non confondere col nome del parametro Rust `callback_id`).
- `send_buttons` costruisce `reply_markup: { inline_keyboard: [[btn1, btn2, ...]] }` (una riga di bottoni).

`FakeTelegramClient` (`#[cfg(test)]`, `pub(crate)`):
- `push_updates(updates: Vec<Update>)` — accoda una batch per la prossima chiamata `get_updates`.
- `recorded_messages() -> Vec<(i64, String)>` — (chat_id, text).
- `recorded_buttons() -> Vec<(i64, String, Vec<InlineButton>)>` — (chat_id, text, buttons).
- `recorded_callbacks() -> Vec<String>` — callback_id ricevuti.

### `telegram::qr`

Rendering QR a caratteri per il terminale (v0.11.3).

```rust
/// Rende l'URI come QR a caratteri per il terminale. Err se il dato è troppo
/// grande per un QR o il rendering fallisce.
///
/// Sicurezza: l'output contiene il TOTP secret — stampare solo su stderr,
/// mai in tracing o log persistenti.
pub fn render_terminal_qr(data: &str) -> Result<String, String>
```

Implementazione: `qr2term::generate_qr_string(data).map_err(|e| ...)`.

`qr2term` usa `crossterm` per explicit color pairs (bianco su nero / nero su bianco)
via caratteri Unicode half-block (`▄`): scansionabile su terminali a sfondo scuro.

Usato da `main.rs` nel blocco primo avvio TOTP:
- `Ok(qr)` → `eprint!("{qr}")` (il QR include il trailing newline).
- `Err(e)` → `eprintln!("(QR non disponibile: {e})")` + continua normalmente.
  L'avvio del canale non viene mai bloccato da un fallimento QR.

### `telegram::channel`

Entry-point del canale Telegram (Task 4). Integra client, auth, gate, history, AI e tools.

```rust
pub struct TelegramChannel {
    client:     Arc<dyn TelegramClient>,   // Arc per 'static (tokio::spawn Task 5)
    auth:       Authenticator,
    gates:      PendingGates,
    history:    ConversationHistory,
    ai:         Arc<dyn AiAdapter>,        // Arc per 'static
    tools:      Arc<dyn ToolClient>,       // Arc per 'static
    offset:     i64,                       // update_id + 1 dell'ultimo elaborato
    state_path: PathBuf,                   // per persistenza pairing
}

/// Mappa Vec<ServerMsg> → String (funzione pura, testabile senza mock).
pub fn format_response(msgs: &[ServerMsg]) -> String
```

Mapping `format_response`:
- `Chunk { content }` → accumulato in buffer fino al `Done`.
- `OpenWindow { title, content }` → `"📄 {title}\n\n{content}"`.
- `Done { .. }` → emette il buffer Chunk accumulato come segmento.
- `Error { message }` → `"[errore] {message}"`.
- `ServerInfo | Pong` → ignorati (infrastruttura WS, non visibili su Telegram).
- Segmenti uniti con `"\n"`.

`run_command_buffered` — flow:
1. `unbounded_channel::<ServerMsg>()`.
2. `core::handle_command("tg", input, kind, None, &mut self.history, self.ai.as_ref(), self.tools.as_ref(), false, tx)`.
3. `while let Ok(msg) = rx.try_recv()` (tx già droppato).
4. `format_response(&msgs)` → fallback `"(nessun output)"` se vuoto.
5. `split_at_chars(text, 4096)` → `send_message` per ogni chunk.

`dispatch` — ordine branch per i messaggi:
1. `/pair <code>` → `auth.try_pair` (PRIMA di authorize).
2. `/login <totp>` → `auth.try_login` (PRIMA di authorize).
3. `auth.authorize` → `NeedPairing`→silenzio, `NeedLogin`→messaggio, `Denied`→silenzio, `RateLimited`→messaggio.
4. `gate::needs_confirmation` → `send_buttons` con gate_id opaco.
5. Altrimenti → `run_command_buffered`.

`run` — loop long-poll:
1. Skip backlog (offset = max(update_id)+1 degli update correnti).
2. `get_updates(offset, 25)` → aggiorna offset → `dispatch(update, now_ms)`.
3. Backoff esponenziale su `Err`: 1s → 2s → 4s → max 32s.
4. `now_ms` da `SystemTime::now()` (solo in `run`; `dispatch` riceve il valore iniettato).

Sicurezza:
- `answer_callback` chiamato subito per ogni callback (spinner).
- Gate id: `format!("{:032x}", rand::random::<u128>())` — 32 hex char ≤ 64 byte limit Telegram.
- `web_search=false` hardcoded (commento inline nel codice).
- Token/secret mai nei log.

### `telegram::auth`

Logica di autenticazione pura (ADR-007). Nessun clock reale — `now_ms: u64` iniettato.

```rust
// No `#[derive(Debug)]` — totp_secret_base32 non deve comparire nei log.
#[derive(Serialize, Deserialize, Default)]
pub struct TelegramState {
    pub totp_secret_base32: Option<String>,  // base32, 32 char = 20 byte (min 16 byte per totp-rs SHA1)
    pub paired_chat_id:     Option<i64>,
}
impl TelegramState {
    pub fn load(path: &Path) -> Self              // assente/corrotto → Default
    pub fn save(&self, path: &Path) -> Result<(), String>  // crea dir parent; no secret nell'errore
}

#[derive(Debug, Clone, PartialEq)]
pub enum AuthResult { Ok, NeedPairing, NeedLogin, RateLimited, Denied }

pub struct Authenticator { /* totp, secret_base32, paired_chat_id, session_expiry, login_fails, pair_fails, lockout_until, pairing_code */ }
impl Authenticator {
    pub fn from_state(state: &TelegramState) -> Result<Self, String>  // seam testabile
    pub fn init(state_path: &Path) -> (Self, Option<String>)          // genera secret se assente → Some(uri)
    pub fn new_pairing_code(&mut self, now_ms: u64) -> String         // 6 cifre, TTL PAIRING_TTL_MS
    pub fn authorize(&self, chat_id: i64, now_ms: u64) -> AuthResult  // 4 rami ordinati
    pub fn try_pair(&mut self, chat_id: i64, code: &str, now_ms: u64, state_path: &Path) -> AuthResult
    pub fn try_login(&mut self, chat_id: i64, totp_code: &str, now_ms: u64) -> AuthResult
    pub fn paired_chat_id(&self) -> Option<i64>
    pub fn secret_base32(&self) -> &str                               // solo per diagnosi primo avvio
}

pub const MAX_FAILS:       u32 = 5;
pub const LOCKOUT_MS:      u64 = 60_000;       // 1 minuto
pub const SESSION_TTL_MS:  u64 = 1_800_000;   // 30 minuti
pub const PAIRING_TTL_MS:  u64 = 600_000;     // 10 minuti
```

Dettagli chiave:

- **totp-rs API** (verificata empiricamente):
  - `TOTP::new(SHA1, 6, skew=1, step=30, secret_bytes, Some(issuer), account)` — 7 argomenti; minimo 16 byte secret.
  - `totp.get_url()` → URI `otpauth://totp/{issuer}:{account}?secret={b32}&issuer={issuer}`.
  - `totp.check(code, now_secs)` — verifica il codice con finestra ±`skew` step.
  - `totp.generate(now_secs)` — genera il codice per `now_secs`.
  - `now_ms / 1000` → conversione obbligatoria (le costanti interne sono in ms).

- **`authorize` — ordine rami (importante)**:
  1. `paired_chat_id == None` → `NeedPairing`
  2. `chat_id != paired_chat_id` → `Denied`
  3. `session_expiry` assente/scaduta → `NeedLogin`
  4. sessione attiva → `Ok`

- **Rate-limit** — semantica (spec: "5 tentativi errati, poi il 6° → bloccato"):
  - Tentativi 1..=MAX_FAILS (1..=5) → tutti `Denied`. Il 5° arma `lockout_until` ma
    ritorna ancora `Denied` (l'utente ha consumato tutti i suoi tentativi leciti).
  - Tentativo MAX_FAILS+1 (il 6°) → catturato da `is_locked_out` → `RateLimited`
    senza verificare il codice.
  - Scaduto lockout → `is_locked_out` azzera il contatore → nuova finestra di 5 tentativi.
  - Lockout unificato (stesso campo `lockout_until` per login e pairing).
  - `init` con secret invalido (corrotto/troncato): rileva `Secret::Encoded(b32).to_bytes().is_err()`
    e rigenera come se il secret fosse assente (no panic al daemon startup).

- **Persistenza**: solo `try_pair` e `init` scrivono su disco. `try_login` no (sessioni in-memory).

- **Secret hygiene**: `TelegramState` senza `Debug`; `init` scrive l'URI su `Option<String>`
  ritornata, non su log. `secret_base32()` accessore pubblico documentato "non loggare in produzione".

### `core`

```rust
#[allow(clippy::too_many_arguments)]
pub async fn handle_command(
    id: &str, input: &str, command_type: CommandKind, _cwd: Option<&str>,
    history: &mut ConversationHistory,       // Slice 2: per-connessione
    ai: &dyn AiAdapter, tools: &dyn ToolClient,
    web_search: bool,                        // Task 3: gating ricerca web interna
    confirmer: Option<&dyn ToolConfirmer>,   // Task 2: None = UI; Some(c) = gate Telegram
    tx: UnboundedSender<ServerMsg>,          // Slice 4: posseduto, droppato a fine chiamata
)
```

`handle_nl` (privato) condivide la stessa firma `confirmer` e la propaga a `respond`.
`ws.rs` passa `None` (UI locale fidata). `telegram/channel.rs` passa `None` per ora
(Task 3 passerà `Some(&TelegramConfirmer)`).

Helper puro privato:
```rust
fn percent_encode_query(s: &str) -> String  // RFC 3986 unreserved + %XX UTF-8, nessuna dipendenza
```

Flusso (v0.9.0):
- **Intercetto `/nowin`** (PRIMA di `router::classify`): `strip_nowin_prefix(input)` rileva il prefisso
  `/nowin` (case-insensitive). Se presente, chiama `handle_nl` con il prompt PULITO e
  `TurnOptions { allow_windows: false, web_search }` — l'AI riceve i tool senza `show_markdown` per quel
  solo turno, la storia salva solo il prompt originale. Input vuoto: `Chunk(hint) + Done{1}`.
- `Route::Slash`: helper puro `handle_slash` ritorna `Vec<ServerMsg>`; ogni msg inoltrato su `tx`.
  - `/web <query>` → `open_target("https://www.google.com/search?q=<encoded>")` → `Chunk + Done{0/1}`.
  - `/web` vuoto → `Error{RoutingError}`.
  - `/show <markdown>` → `OpenWindow{Markdown, title, content} + Done{exit_code: Some(0)}` (conformità protocollo).
  - `/show` vuoto/whitespace → `Error{RoutingError}`.
  - `/help` → `OpenWindow{Help, "Lare — Comandi", HELP_MARKDOWN} + Done{exit_code: Some(0)}`.
- `Route::Os`: helper puro `handle_os` ritorna `Vec<ServerMsg>`; ogni msg inoltrato su `tx`.
- `Route::Nl`: `handle_nl(id, input, history, ai, tools, TurnOptions { allow_windows: true, web_search }, tx)`.

### `test_support`

```rust
pub(crate) async fn collect<F, Fut>(f: F) -> Vec<ServerMsg>
where
    F: FnOnce(UnboundedSender<ServerMsg>) -> Fut,
    Fut: Future<Output = ()>,
```

Helper di test (solo `#[cfg(test)]`): esegue una closure produttrice, raccoglie tutti i `ServerMsg` emessi su `tx` in un `Vec`. La closure riceve `tx` per valore; quando la sua future completa, `tx` viene droppato, il canale si chiude e la coda residua viene raccolta. Permette ai test di asserire sul `Vec<ServerMsg>` senza adattare la nuova firma `respond(..., tx)`.

### `ws`

Server su `127.0.0.1:7331`. Per ogni connessione (`handle_connection`):
1. Accept TCP → upgrade WebSocket.
2. `Hello{token}` → validazione → errore: close.
3. `ServerInfo{version, ai_provider: ai.provider(), capabilities}` + `Cwd` iniziale, scritti
   direttamente sul sink prima che parta tutto il resto.
4. **Canale d'uscita persistente** (`mpsc::unbounded_channel::<ServerMsg>()`, `out_tx`/`out_rx`) +
   **writer task** dedicato (`while let Some(msg) = out_rx.recv().await { send_msg(...) }`) — un
   solo sink per l'intera vita della connessione, condiviso (clonando `out_tx`) da tutti i comandi
   che verranno spawnati. Sostituisce il vecchio modello `tokio::join!` per-comando (Slice 4): ora
   più comandi/ricerche possono scrivere sullo stesso sink **contemporaneamente**, e il loop
   principale resta sempre libero di leggere il prossimo `ClientMsg` (vedi punto 6).
5. **Reader task** dedicato: parsa i frame WS e inoltra ogni `ClientMsg` deserializzato su un
   canale in-memory (`in_tx`/`in_rx`) — il loop principale non fa I/O di rete diretto.
6. **`let history = Arc::new(Mutex::new(ConversationHistory::new()))`** — storia conversazionale
   **per-connessione** (Slice 2, stateful), condivisa tramite `Arc` con i task dei comandi.
7. **Registri di cancellazione** (`id → CancellationToken`, uno per `searches` e uno per
   `commands`): il loop principale legge `in_rx` in un ciclo che non blocca mai — ogni
   `Command`/plugin/`/find` viene **spawnato** nel proprio task (`tokio::spawn`) e il loop torna
   subito a leggere, così `CancelCommand{id}`/`CancelSearch{id}` (o `PauseSearch`) arrivano e
   cancellano il token immediatamente **anche mentre** un comando precedente sta ancora
   producendo output in streaming sullo stesso sink.

**Concorrenza reale fra comandi diversi (non solo fra loop e I/O) — chiarito 2026-07-01:**
- **Plugin (`/calc`, `/counter`, …) e `/find`** non toccano `history`: girano **pienamente in
  parallelo** a un comando OS/NL/AI già in corso sulla stessa connessione, nessuna attesa.
- **Un secondo comando OS o NL/AI** (routing normale via `core::handle_command`) invece **prende
  lo stesso `Mutex<ConversationHistory>`** (`hist.lock().await` prima di chiamare
  `handle_command`, che tiene `&mut History` per l'intera durata — streaming AI + tool call
  incluso). Se un primo comando OS/NL/AI è ancora in corso, il task del secondo resta bloccato
  sul lock **invisibilmente**: nessun errore, nessun "in coda" mostrato in UI — il frontend
  (`app.js`) non disabilita l'input né controlla `inFlight` prima di inviare, quindi l'utente PUÒ
  digitare e inviare un secondo comando OS/NL in qualsiasi momento, ma quel secondo comando non
  **inizia realmente** finché il primo non rilascia il lock (cioè non è `Done`). Nessuna
  interruzione del primo comando in nessun caso: solo lo Stop (⏹, `CancelCommand` sullo specifico
  `id`) lo annulla davvero.
- `Ping` → `Pong`; `Close` → exit; alla disconnessione tutti i token nei due registri vengono
  cancellati (teardown).

---

## Configuration

| Env var | Default | Description |
|---|---|---|
| `LARE_TOKEN` | random 32-char | Auth token per WS handshake. Generato per run, loggato a stderr se non impostato. |
| `LARE_MCP_SERVER` | sibling of exe | Path al binary `mcp-server`. |
| `ANTHROPIC_API_KEY` | (nessun default) | Chiave Anthropic. Assente → fallback `StubAdapter`. Mai nel repo. |
| `LARE_AI_MODEL` | `claude-sonnet-4-6` | Modello Claude. Override a runtime senza rebuild. |

---

## Dependencies (key)

| Crate | Version | Why |
|---|---|---|
| `protocol` | path | `ClientMsg`/`ServerMsg` condivisi |
| `tokio-tungstenite` | 0.24 | WebSocket server |
| `rmcp` | =1.7.0 | MCP client (`client` + `transport-child-process`) |
| `async-trait` | 0.1 | dyn-safe async traits (non ancora stabile in Rust 1.96) |
| `rand` | 0.8 | Token efimero |
| `reqwest` | 0.12 | HTTP client Anthropic Messages API (rustls-tls, no OpenSSL) |
| `qr2term` | 0.3 | QR a terminale (primo avvio Telegram: URI otpauth scansionabile) |

---

## How to run

```powershell
# Build tutto (orchestrator + mcp-server)
cargo build

# Start con Claude reale (Slice 2: tool-use)
$env:LARE_TOKEN = "lare-dev"
$env:ANTHROPIC_API_KEY = "sk-ant-..."
cargo run -p orchestrator

# Start con StubAdapter (nessuna chiave → fallback automatico)
$env:LARE_TOKEN = "lare-dev"
cargo run -p orchestrator

# Override modello
$env:LARE_AI_MODEL = "claude-opus-4-8"
```

## Canale Telegram — avvio e setup (v0.11.0)

### Prerequisiti

1. Crea un bot Telegram tramite `@BotFather` → ottieni il token.
2. Crea `telegramsettings.json` nella stessa directory dell'eseguibile (o in `LARE_TELEGRAM_SETTINGS`):
   ```json
   { "token": "123456:ABC-DEF..." }
   ```
   Il file NON deve entrare nel repository (aggiunto a `.gitignore`).

### Primo avvio (setup TOTP)

Al primo avvio (secret TOTP non ancora presente), l'orchestratore stampa su stderr:

```
[Telegram] *** PRIMO AVVIO — TOTP setup ***
[Telegram] Scansiona questo QR con Google Authenticator:
<QR a caratteri ANSI — blocchi bianco/nero>
[Telegram] Oppure inserisci la chiave manualmente / usa questo URI:
  otpauth://totp/Lare%20Terminal:lare@terminal?secret=...&issuer=Lare%20Terminal
[Telegram] Conserva l'URI in modo sicuro. Non verrà mostrato di nuovo.

[Telegram] Per appaiare la chat invia al bot: /pair 123456
```

- Scansiona il **QR** direttamente con Google Authenticator (punta la fotocamera).
- Se il QR non è leggibile (terminale limitato), usa l'URI testuale come fallback.
- Il QR/URI vengono mostrati **una sola volta** e non vengono mai loggati da `tracing`.
- Se `render_terminal_qr` fallisce, viene stampato `"(QR non disponibile: …)"` e
  il canale si avvia comunque — l'URI testuale è sempre presente.
- Lo state (`totp_secret_base32` + `paired_chat_id`) è persistito in:
  - Windows: `%LOCALAPPDATA%\dev.lare.terminal\telegram-state.json`
  - Fallback (macOS/Linux/CI): `.lare-data/telegram-state.json` nella cwd.

### Pairing della chat

1. Invia `/pair <codice>` al bot (codice da stderr; valido 10 minuti).
2. Il bot risponde "Pairing completato".
3. Ai successivi avvii (chat già appaiata), il codice `/pair` **non viene più stampato**.

### Login e uso

```
/login <codice-TOTP-6-cifre>    → apre una sessione (30 min)
dir                              → OS command (richiede conferma via bottoni inline)
spiega il codice ...             → NL → AI
/open C:\path\to\file            → apre app nativa (richiede conferma)
/reset                           → riavvia la sessione shell
```

### Sicurezza

- Token Telegram mai nei log di `tracing` (solo in `telegramsettings.json` locale).
- Secret TOTP mai loggato. URI TOTP solo su `eprintln!` al primo avvio.
- Sessioni in-memory (non persistite): `/login` richiesto dopo ogni riavvio.
- Gate ADR-007: comandi OS e `/open`/`/web` richiedono conferma via bottoni inline Telegram.
- Rate-limit: 5 tentativi falliti → lockout 1 minuto (sia `/pair` che `/login`).

---

## Test summary

```
cargo test -p orchestrator

unit tests (src/lib.rs):    209 tests  (Task 2: += confirm × 0 aggiornati, ai_adapter × 3 nuovi)
integration tests (tests/):   4 tests (ws_integration.rs)
ignored:                       3 tests (mcp_tool_client_run_in_session,
                                         real_messages_api_returns_text,
                                         real_streaming_invokes_on_text)
total:                        213 tests — 209 unit + 4 integration passano; 3 ignorati
```

Test per modulo (Task 2 — in corso):
- `telegram::confirm::tests`: 5 — `confirm_ok_tap_returns_true`, `confirm_no_tap_returns_false`, `confirm_timeout_returns_false`, `confirm_intervening_message_replies_pending_then_resolves`, `confirm_tap_from_other_chat_ignored`. (Aggiornati: `Cell<i64>` → `AtomicI64`.)
- `telegram::qr::tests`: 2 — `render_valid_otpauth_uri_returns_ok_non_empty`, `render_overlong_input_returns_err_not_panic`.
- `telegram::channel::tests`: 23 — `format_response` (6 test puri), `split_at_chars` (4), `dispatch` (9 async: unpaired/pair/login/NL/OS/callback-ok/callback-repeat/no/wrong-chat), `run_command_buffered` (1: fallback output vuoto).
- `telegram::gate::tests`: 22 — `needs_confirmation` (15), `PendingGates` (7: insert/take, single-use, scadenza, purge).
- `telegram::auth::tests`: 34 — TelegramState (load/save/corrupted/mkdir), init (genera/riusa/invalid), authorize (4 rami), try_login (TOTP corretto/errato/±skew/rate-limit/lockout), try_pair (corretto/single-use/errato/scaduto/no-codice/rate-limit/lockout), flow completo, sicurezza.
- `telegram::client::tests`: 7 — deserializzazione Update/Message/CallbackQuery, wire sendButtons, FakeTelegramClient (get_updates, send_message, send_buttons, answer_callback).
- `telegram::settings::tests`: 6 — token valido, file assente, JSON malformato, campo mancante, token vuoto, sicurezza messaggio errore.
- `agent::tests`: 14 — truncate (passthrough, head+tail+marker, char-safe UTF-8), dispatch run_in_session (stdout, nonzero), open_target, tool sconosciuto, **markdown_window** (titolo esplicito, da prima riga, fallback, troncamento ≤60), **tools_for**: `tools_for_default_has_three_custom`, `tools_for_no_windows_excludes_show_markdown`, `tools_for_web_search_adds_two_server_tools`.
- `ai_adapter::tests`: 18 (+3 Task 2) — stub respond, Claude text-only, loop canonico, loop_show_markdown_emits_open_window_and_acks, stateful, cap, error, refusal, provider, igiene storia (×2), nowin_mode, web_search_mode, empty_assistant_turn; **+** `confirmer_deny_skips_tool_execution`, `confirmer_allow_executes_tool`, `no_confirmer_executes_autonomously`.
- `messages_client::tests`: 15 — one_shot, wire, ToolUse wire, ToolResult wire, Unknown(thinking), text, errore; 5 SseAccumulator; 3 ToolSpec.
- `core::tests`: nowin (×3), slash_web (×4), slash_show (×6), slash_help, slash_open/reset/unknown/config/whitespace, OS (×5), NL (×2), cwd, dollar prefix (×5).
- `tool_client::tests`: 6 — FakeToolClient success/failure/with_output/open_target/dyn compatibility.
- `router::tests`: 20 — tutti i rami di classify.
- `ws_integration.rs`: 4 — handshake ok/ko, ping/pong, NL stub.

---

## AI Chat — fix integrità consenso (review 2026-07-03, orchestrator 0.34.0)

Cinque fix in `aichat/service.rs`, tutti TDD RED-first (8 nuovi test nel modulo `tests`):

- **#1 generazione del voto.** `PendingVote` ha un campo `generation: u64` assegnato da
  `start_admission_vote` dal contatore monotono `next_vote_gen`. `Effect::StartVoteTimeout` e
  `ServiceEvent::VoteTimeout` la trasportano; l'arm `VoteTimeout` risolve **solo** se
  `pending_votes[candidate].generation == generation` (match su `get`, non più `contains_key`).
  Chiude la race "timer di un turno risolto ammette il turno successivo dopo un re-request".
- **#2 veto solo dai presenti.** `record_vote`, dopo aver risolto `candidate_id` per etichetta,
  ignora il voto se `voter ∉ pv.present` (guard simmetrico: un `sì` da estraneo era già inerte).
- **#3 no admit fantasma su PeerGone del candidato.** L'arm `PeerGone` fa
  `pending_votes.remove(&id)` (cattura in `dropped_candidate_vote`): un candidato non è tra i
  propri `present`, quindi senza questo `ready_after_peer_gone`/`VoteTimeout` lo ammetteva da
  sparito. La rimozione è no-op se `id` non è candidato di alcun voto.
- **#5 cooldown re-request lato server.** Set `cooling_down: HashSet<PeerId>`; `resolve_reject`
  ci inserisce il candidato + emette `Effect::StartCooldownTimer` (gemello di `StartVoteTimeout`
  in `perform` → `ServiceEvent::CooldownExpired`, che lo rimuove). L'arm `RequestAdmission`
  ignora un `from` in cooldown. `PeerGone` ripulisce `cooling_down` per igiene. `handle_event`
  resta puro (nessun `Instant::now()`: il tempo è sempre un timer-Effect).
- **#7 notifica di chiusura del voto.** Helper `notify_vote_resolved(&pv)`: per ogni presente,
  `ToUi(AiChatAdmissionResolved)` a sé stesso (`self.me.id`) o `SendToPeer(ChatMsg::AdmissionResolved)`
  ai presenti remoti. Chiamato da `resolve_admit`, `resolve_reject` e dal drop-candidato in
  `PeerGone`. Bridge lato presente-client: `ChatMsg::AdmissionResolved` → `ToUi`.

**Differiti a follow-up:** #4 (i destinatari del relay restano `connected`, non `admitted` — un
peer connesso non-ammesso riceve ancora i messaggi di stanza) e #6 (liveness dell'elezione: un
peer UDP-annunciato ma TCP-irraggiungibile con IP più basso è eletto all'infinito).

## Known debits / Slice 4+ todos

- Token comparison usa `==` (non constant-time). Fase 2: `subtle::ConstantTimeEq`.
- ~~`handle_command` ritorna `Vec<ServerMsg>` (fully buffered). Slice 4: `mpsc::Sender<ServerMsg>` per streaming SSE.~~ FATTO (0.8.0).
- ~~`show_markdown → OpenWindow` da AI (Slice 3)~~ FATTO (0.7.0).
- ~~Streaming SSE (Slice 4)~~ FATTO (0.8.0). E2e richiede `ANTHROPIC_API_KEY` + avvio dal vivo.
- `McpToolClient` runtime test (`mcp_tool_client_run_in_session`) è `#[ignore]`; richiede binary `mcp-server`.
- `real_messages_api_returns_text` e `real_streaming_invokes_on_text` sono `#[ignore]`; richiedono `ANTHROPIC_API_KEY`.
- ~~Gate ADR-007 (conferma utente per comandi) — deferred a Telegram integration.~~ FATTO (Task 3+4: `gate.rs` + `channel.rs`).
- Timeout per-comando in `mcp-server` — deferred (comandi interattivi bloccano la sessione).
- Crescita contesto conversazione (long conversations) — prompt caching / truncation storia non ancora implementati.
- `AiAdapter::provider()` ritorna `String` (by value); potrebbe diventare `&str` se la firma si semplifica.
- `core.rs` flow diagram nel doc-comment aggiornato a v0.8.0 (intercetto `/nowin`, `/show` + Done).

## [0.34.1] — Diagnostica keepalive: distingue le 3 cause di `PeerGone`

Investigando dal vivo un `PeerGone` falso (2 macchine: `rumpleteazer` server, `skimble` client
appena avviato) sono emersi **due problemi distinti**, non uno solo:

1. **Gate 1 perso al boot (bug reale, CONFERMATO dal vivo con un secondo test e RISOLTO in
   0.34.2, sezione sotto).** `begin_join_gate` (riga ~1999) spara
   `Effect::ToUi(AiChatJoinPrompt)` non appena il `ConnectOutcome::Success` arriva — cioè non
   appena `decide_and_connect` stabilisce il TCP verso il server eletto, in genere **prima** che
   l'umano abbia aperto la finestra AI Chat (la connessione parte da sola all'avvio
   dell'orchestrator; la finestra è un webview separato che l'utente apre a parte). Se
   `server_tx` è ancora `None` in quel momento, `Effect::ToUi` lo scarta silenziosamente (riga
   2358: "se la UI è chiusa, il messaggio viene ignorato") — e **nessun replay esiste**: il gap
   è documentato nel commento di `SetServerTx` (righe 723-730) fin da quando è stato introdotto,
   ma non ancora chiuso. Risultato: il nuovo arrivato resta bloccato in
   `SelfAdmission::Deciding` a tempo indeterminato, senza alcun prompt visibile e senza modo di
   ri-richiederlo aprendo la finestra più tardi — "tutto fermo" dal punto di vista dell'utente.
   Confermato dal vivo: la sequenza reale era orchestrator+10s poi ui.exe+5s su `skimble`, quindi
   il connect (innescato dalla scoperta UDP di `rumpleteazer`, non dall'apertura della finestra)
   precede sistematicamente l'apertura della finestra chat con questi tempi manuali — non un
   caso raro. **Secondo run di conferma**: `skimble` ha aperto `ui.exe` 8s **dopo** l'elezione
   (quindi ben dopo il connect) e il gate 1 non è comunque mai apparso — la finestra non
   ripresenta lo stato perso, prova diretta dell'assenza di replay (non solo teorica dal
   commento). Vedi 0.34.2 per il fix.
2. **`PeerGone` spurio non diagnosticabile (RISOLTO in questa release, solo strumentazione).**
   Separatamente, `rumpleteazer` ha riportato "skimble-human è sparito" una volta senza causa
   apparente, seguito da una ri-scoperta pulita (conferma che l'orchestrator di `skimble` non era
   mai morto — falso positivo, non una vera disconnessione). I tre percorsi che portano a
   `PeerGone` dal reader task (timeout keepalive `DEAD_THRESHOLD`, EOF pulito, errore I/O)
   loggavano tutti a `debug!` (EOF: nessun log affatto) — con filtro di default `INFO`
   (`main.rs`), **indistinguibili a posteriori**. Non essendo riproducibile un incidente
   ambientale con precisione (compilazione `cargo tauri dev` in corso su `skimble` all'avvio è
   l'ipotesi principale, non confermata — l'asimmetria osservata, `skimble` NON ha riportato
   `rumpleteazer` come sparito, è compatibile ma non conclusiva), non si è forzata una root
   cause: si è promossa la sola strumentazione (timeout/errore I/O → `warn!`, EOF → nuovo
   `info!`, testo `PEER-GONE per <causa>`) così che la PROSSIMA occorrenza sia diagnosticabile
   senza `RUST_LOG=debug`. Nessun cambio di comportamento.

## [0.34.2] — Fix "tutto fermo": `SetServerTx` ri-emette il gate di ammissione perso

Chiude il punto 1 di 0.34.1 sopra. TDD RED-first (3 test nuovi in `aichat::service::tests`,
sezione "Bug fix (2026-07-04): SetServerTx ri-emette il gate di ammissione perso").

Nell'arm `ServiceEvent::SetServerTx` (riga ~715), dopo il replay già esistente di
`last_known_roster`/`history`, un nuovo `match self.self_admission`:

- `Deciding` → `Effect::ToUi(AiChatJoinPrompt { present: self.known_peer_labels() })` (gate 1,
  esattamente ciò che `begin_join_gate` avrebbe emesso al momento del connect).
- `Pending` → `Effect::ToUi(AiChatPending { present: self.known_peer_labels() })` (gate 2, in
  attesa del voto — ciò che `request_admission` avrebbe emesso).
- `NotJoining` / `Admitted` / `Rejected` → nessun effetto aggiuntivo.

**Perché `Rejected` non è coperto:** `SelfAdmission::Rejected` non porta un timestamp per scelta
esplicita del design (vedi il suo doc-comment: tenere `handle_event` pura, niente
`Instant::now()` nell'attore — il cooldown di 30s del re-request vive lato UI,
`admission.mjs`). Senza un `retry_at` memorizzato, un replay di `AiChatRejected{retry_after_secs}`
alla riapertura non potrebbe calcolare correttamente il tempo residuo — si sceglie di non
ri-emetterlo piuttosto che ri-emettere un valore sbagliato (es. sempre "30s" anche se il
cooldown reale è già scaduto). Impatto pratico minore rispetto a `Deciding`/`Pending`: un
rifiuto è un esito esplicito già visto dall'umano prima che la finestra si chiudesse (non un
prompt mai apparso), e il flusso "chiedi di entrare" resta comunque disponibile al successivo
evento di dominio (es. un nuovo `Discovered`).

**Resta fuori scope** (annotato in-code nel commento dell'arm): il gate 2 di un PRESENTE che
deve ancora votare (`self.pending_votes`, lato server — es. `rumpleteazer` con la finestra
chiusa mentre `skimble` chiede l'ammissione) non viene ri-emesso alla riapertura. Diverso da
questo fix: qui lo stato perso è quello del CANDIDATO (`self_admission`), non quello di un
votante. Stessa categoria di gap, lato opposto della stanza — non riprodotto dal vivo in questa
sessione, da valutare se/quando osservato.

506 lib test verdi, clippy pulito sul modulo `aichat`.

## [0.35.0] — Library "Share with": consenso + scadenza 24h (Slice 1a, backend)

Prima slice della feature "Share with" (spec:
`Docs/superpowers/specs/2026-07-02-library-share-with-design.md`, piano:
`Docs/superpowers/plans/2026-07-05-library-share-slice1-consent-net.md`). Costruita task-per-task
via `superpowers:subagent-driven-development` (12 task, un subagent implementatore + una review
per task). Copre il round-trip di offerta/consenso/scadenza tra due macchine via AI Chat —
**nessun trasferimento di contenuto** (`ChatMsg::ShareData`, bytes su disco — Slice 2) e
**nessuna UI** (pulsante Library, banner di consenso — slice successiva) ancora.

**Wire (Contratto A, `protocol` 0.11.0 → 0.12.0):** `ShareTarget::{One,All}`,
`ShareOutcome::{Accepted,Rejected,Failed}`, `ClientMsg::ShareDocument/ShareConsent`,
`ServerMsg::ShareRequest/ShareResult`.

**Wire (Contratto N, `aichat::wire`):** `ChatMsg::ShareOffer/Accept/Reject/Expired` — peer↔peer,
stesso pattern additivo degli altri `ChatMsg`.

**Routing (`aichat::relay`):** `route_to_label(label_base, peers) -> Option<PeerId>` — unicast per
label, gemello di `fan_out_targets` (broadcast); introdotto in questa slice invece che rinviato,
perché sia il mittente (risolve il destinatario di "Share with uno") sia il destinatario (risponde
al mittente) ne hanno bisogno da subito.

**Stato (`aichat::service::AiChatService`):**
- `pending_shares: HashMap<ShareId, PendingShare>` (lato destinatario, offerte in attesa di
  consenso) e `outgoing_shares: HashMap<ShareId, OutgoingShare>` (lato mittente, in attesa
  dell'esito) — cap `MAX_PENDING_SHARES = 1000` (soft ceiling, oltre soglia auto-rifiuto
  immediato, spec §9.1).
- `SHARE_EXPIRY_SECS = 24 * 60 * 60`: un solo orologio per "mai risposto" e "accettato ma il
  trasferimento non si è mai completato" (nessun trasferimento reale in questa slice, quindi in
  pratica solo il primo caso) — timer-effect `Effect::StartShareExpiryTimer` /
  `ServiceEvent::ShareExpiryTimeout`, terza coppia della famiglia dopo voto ammissione e cooldown
  re-request.
- `MAX_SHARE_SIZE_BYTES = 512 * 1024`: cap dimensione documento (costante di codice, non
  configurabile — l'utente ha già anticipato che vorrà raddoppiarlo in futuro).
- Funzioni chiave per orientarsi: `request_share` (mittente avvia l'offerta),
  `handle_share_offer` (destinatario riceve `ChatMsg::ShareOffer`, valida cap dimensione/cap
  pendenti), `share_consent` (destinatario decide, via `ServiceEvent::ShareConsentUi`),
  `resolve_outgoing_share` (mittente riceve Accept/Reject/Expired, riporta l'esito alla UI e pulisce
  `outgoing_shares`).
  **Nota per Slice 2:** `share_consent(accept: true)` oggi rimuove subito l'entry da
  `pending_shares` (nessun `ShareData` da attendere in questa slice). La spec §9.1 prevede che
  un'offerta accettata resti `AwaitingDecision`/in attesa finché il contenuto non arriva — quando
  Slice 2 introduce `ChatMsg::ShareData`, questa funzione dovrà cambiare per NON rimuovere l'entry
  all'accettazione, ma solo quando il trasferimento si completa (o scade).
- `SetServerTx` ri-emette `ServerMsg::ShareRequest` per ogni offerta ancora in `pending_shares`
  alla riconnessione della UI — stesso pattern di replay già usato per storico/roster/gate
  ammissione, questa volta introdotto **prima ancora che la feature avesse una UI reale** (nessun
  gap da scoprire dal vivo, a differenza di 0.34.2).

**Dispatch (`ws.rs`):** `ClientMsg::ShareDocument` → `ServiceEvent::ShareDocumentRequested`,
`ClientMsg::ShareConsent` → `ServiceEvent::ShareConsentUi`.

**Test dal vivo (Task 11, descoped):** `tests/aichat_share_loopback.rs` — un loopback TCP reale
(due `AiChatService`, non simulato) prova la sola metà mittente→destinatario del round-trip
(consegna di `ChatMsg::ShareOffer`). La metà destinatario→mittente (accetta/rifiuta/risultato) non
è testabile con due peer reali su loopback Windows: l'accept loop del server osserva sempre
`127.0.0.1` come sorgente della connessione in ingresso, indipendentemente dalla vera identità
nominale del client connesso — `route_to_label`+`Effect::SendToPeer` lato server non trovano mai
il link giusto. Limite strutturale del loopback (confermato con l'advisor dopo due diagnosi
indipendenti — non risolvibile scegliendo `PeerId` diversi), assente su una LAN reale (`peer_addr()`
riporta l'IP vero e distinto di ciascuna macchina). Quella metà resta coperta dai test
`handle_event` puri di Task 5/6/8/9 e andrà confermata dal vivo multi-macchina.

**Debiti annotati (fuori scope, spec §6/§9.1):** flag `auto_answer_share_requests` (Slice 4,
file-only in `aichat.json`, riavvio richiesto — vedi memoria `aichat-config-restart-policy`);
`ChatMsg::ShareData`/scrittura su disco (Slice 2); `ShareTarget::All` (Slice 3, oggi risponde
subito `Failed{reason: "\"Share with all\" non è ancora disponibile"}`).

529 lib test verdi (era 506 a fine 0.34.2), clippy pulito, `cargo fmt` pulito sui file toccati.

## [0.36.0] — Library "Share with": UI (Slice 1a-ui)

Nessuna nuova logica di dominio in `handle_event` — solo un cambio di tipo per
esporre `doc_name` alla UI. `outgoing_shares: HashMap<ShareId, OutgoingShare>`
(era `HashMap<ShareId, String>`); `request_share`/`resolve_outgoing_share`
aggiornati di conseguenza (vedi commit per il diff esatto). La UI vera e propria
(pulsante Library, banner AI Chat, riga pannello) vive interamente nel frontend
JS — nessun altro cambio Rust in questa slice.

## [0.37.0] — Library "Share with": trasferimento contenuto (Slice 2a)

`PendingShare` (destinatario) da struct a enum a due stadi: `AwaitingDecision` (in attesa della
decisione locale, o già accettata ma senza contenuto — `share_consent` NON rimuove più l'entry
sull'accettazione) e `AwaitingUiWrite` (contenuto arrivato via `ChatMsg::ShareData`, in attesa che
la UI lo scriva con `archive_save`). `OutgoingShare` (mittente) resta uno struct (non un enum: i
due stadi portano dati identici) con un nuovo campo `stage: OutgoingShareStage`
(`AwaitingAccept`/`AwaitingContent`) e `rel_path` — `ChatMsg::ShareAccept` non risolve più
`outgoing_shares` direttamente: `handle_share_accepted` riporta `Accepted` (invariato) e transiziona
ad `AwaitingContent`, chiedendo il contenuto alla UI con `ServerMsg::ShareContentRequest`.

`ClientMsg::ShareContent`/`ShareContentFailed` (mittente) e `ClientMsg::ShareWritten`
(destinatario) chiudono il giro. Rivalidazione dimensione al momento dell'invio contenuto
(`content.len() as u64 > MAX_SHARE_SIZE_BYTES`), stesso filtro di raggiungibilità TCP di
`request_share` (`self.links.contains_key`) applicato di nuovo al momento dell'invio di
`ChatMsg::ShareData` — il peer viene risolto fresco via `route_to_label`, mai un `PeerId` cacheato
dall'Accept.

Replay su `SetServerTx` esteso simmetricamente: `pending_shares` ora emette `ShareRequest` o
`ShareIncomingData` a seconda dello stadio; nuovo replay di `ShareContentRequest` per ogni
`outgoing_shares` ancora `AwaitingContent`.

**Debiti annotati (fuori scope, design §1):** guardia anti-cancellazione in Library (Slice 2b,
avviso prima di eliminare un documento con una condivisione in corso); `ShareTarget::All` (Slice 3,
invariato); flag `auto_answer_share_requests` (Slice 4, invariato).

541 lib test verdi (era 530 a fine Slice 1a-ui, +5 dal Task 3 di questo piano, +6 dal Task 4 —
un test in più del previsto: la review del Task 4 ha trovato che il replay ShareContentRequest su
SetServerTx non aveva copertura, aggiunta con un 6° test), clippy pulito, `cargo fmt` pulito sui
file toccati.

## [0.37.1] — Fix: il destinatario impone il consenso prima del contenuto (Slice 2a)

Il review finale del branch (pre-merge in main) ha trovato che la nota "Slice 2" lasciata a fine
0.35.0 (§ sopra) non era mai stata effettivamente chiusa: `share_consent(accept: true)` mandava
`ChatMsg::ShareAccept` ma NON modificava `pending_shares` — l'entry restava `AwaitingDecision`,
identica a un'offerta mai decisa. `handle_share_data` accettava `ChatMsg::ShareData` per QUALUNQUE
entry `AwaitingDecision`, senza distinguere "appena arrivata" da "già accettata". Risultato: un peer
già ammesso nella stanza (buggato o non-conforme al protocollo previsto — il flusso onesto, che
aspetta `ShareAccept` prima di mandare dati, non era mai a rischio) poteva mandare `ShareOffer`
seguito SUBITO da `ShareData`, prima che l'umano cliccasse qualunque cosa, e ottenere la scrittura
del file in Library del destinatario senza consenso — viola la garanzia di spec §8 ("niente
contenuto prima del consenso… un destinatario che rifiuta non ha MAI ricevuto byte del documento").
Effetto collaterale: una volta scritta l'entry come `AwaitingUiWrite`, il click successivo
dell'umano su "Accetta"/"Rifiuta" nel banner cadeva nel ramo di default di `share_consent` (entry
non più `AwaitingDecision`) — un no-op silenzioso, banner decorativo.

**Fix — `PendingShare` guadagna un terzo stadio**, `AwaitingContent { from_label, doc_name,
size_bytes }` (stessi campi di `AwaitingDecision`), simmetrico allo stadio omonimo già esistente
lato mittente (`OutgoingShareStage::AwaitingContent`):
- `share_consent(accept: true)` ora `insert`-a esplicitamente `AwaitingContent` al posto
  dell'`AwaitingDecision` rimossa — non lascia più l'entry invariata.
- `handle_share_data` richiede `Some(PendingShare::AwaitingContent { .. })`: un `ShareData` che
  arriva mentre l'entry è ancora `AwaitingDecision` cade nel ramo `_ =>` (stesso trattamento
  difensivo già riservato a uno `share_id` sconosciuto — `tracing::debug!` + `Vec::new()`, nessun
  reject attivo, coerente con la filosofia "silenzio verso comportamenti anomali" del modulo).
  Aggiunge anche la rivalidazione dimensione mancante lato destinatario (il mittente la fa già in
  `handle_share_content`): contenuto reale sopra `MAX_SHARE_SIZE_BYTES` → rimuove l'entry e
  rifiuta ATTIVAMENTE via `auto_reject_share` (stesso trattamento dell'offerta iniziale
  sovradimensionata — qui la violazione è scoperta più tardi, ma è la stessa categoria di anomalia).
- `ServiceEvent::ShareExpiryTimeout` e il replay in `SetServerTx` estesi al terzo stadio: scadenza
  notifica il mittente come per gli altri due; replay non ri-mostra il banner (già superato).

3 nuovi test (`peer_share_data_before_accept_is_noop`,
`share_expiry_timeout_notifies_sender_when_awaiting_content`,
`peer_share_data_over_cap_rejects_and_removes_entry`), 4 test esistenti aggiornati per riflettere
la transizione esplicita e la nuova sequenza `ShareConsentUi{accept:true}` → `ShareData` (prima
mancava il passo di accettazione, ora obbligatorio per raggiungere `AwaitingUiWrite`). 544 lib
test verdi (era 541), clippy pulito (stessi 4 warning pre-esistenti, nessuno nuovo).

## ChatBackend — seam multi-provider per il tool-use (0.37.2)

**Obiettivo:** isolare la parte "parla col provider LLM" del loop di tool-use dietro un trait,
così Claude e (in uno slice successivo) OpenRouter possono condividere lo stesso loop invece di
duplicarlo. Vedi `Docs/superpowers/specs/2026-07-06-openrouter-tooluse-adapter-design.md`.

**Design 1 — `ChatBackend::send_turn`:** un solo metodo, `(system, tools, history, on_text) ->
Result<BackendTurn, BackendError>`. `tools: &[ToolSpec]` (non `&[ToolDef]`) perché deve poter
portare anche i tool server-side Anthropic (`ToolSpec::Server`, es. `web_search`) che un backend
non-Anthropic non può eseguire — vedi il commento sul trait per il perché.

**Design 2 — `TurnStop` sostituisce le stringhe `stop_reason`/`finish_reason`:** ogni provider ha
un proprio vocabolario per "perché il turno è finito" (Anthropic: `end_turn`/`tool_use`/
`refusal`/`max_tokens`; OpenAI: `stop`/`tool_calls`/`length`/`content_filter`). `TurnStop` è il
vocabolario NEUTRO che il loop di `ai_adapter.rs` consulta — ogni backend mappa il proprio.

**Test:** `fake_chat_backend_records_request_and_replays_responses` prova che il test double
registra le richieste e le rigioca nell'ordine configurato — è l'infrastruttura su cui poggeranno
i test del loop (Task 3 di questo piano).

---

## ClaudeBackend — prima implementazione di ChatBackend (0.37.3)

**Obiettivo:** dimostrare che `ChatBackend` (Task precedente) è sufficiente per rappresentare
fedelmente il comportamento Anthropic esistente, prima di introdurre un secondo backend con
traduzione reale.

**Design 1 — passthrough quasi totale:** `Block`/`ToolSpec` in `messages_client.rs` SONO già
la forma wire Anthropic (derivano `Serialize` col tag `"type"` che produce esattamente il JSON
Anthropic) — `ClaudeBackend::send_turn` non fa alcuna conversione di dato, solo assembla
`MessagesRequest{model, max_tokens, system, tools, messages, thinking:None}` e mappa
`stop_reason`/`stop_details` in `TurnStop`.

**Design 2 — `impl From<MessagesError> for BackendError`:** permette a `send_turn` di usare `?`
sulla chiamata a `self.messages.create_streaming(...)` senza match espliciti — stessa forma
di errore (`Http`/`Network`/`Decode`), solo un tipo diverso.

**Test:** 4 test — passthrough di system/tools/history, mapping refusal→`Refused`, mapping
max_tokens→`MaxTokens`, mapping errore di rete. Nessun test di streaming reale qui: quello resta
coperto dai test SSE esistenti in `messages_client.rs`, invariati.

---

## LlmAdapter — il loop di tool-use diventa backend-agnostico (0.38.0)

**Obiettivo:** completare il refactor iniziato con `ChatBackend`/`ClaudeBackend`: il loop di
tool-use vive UNA sola volta, non più dentro un tipo `ClaudeAdapter` cablato su Anthropic.

**Design 1 — `LlmAdapter` sostituisce `ClaudeAdapter` come UNICO tipo adapter:** non esistono
`ClaudeAdapter`/`OpenRouterAdapter` come tipi separati — si costruisce `LlmAdapter::new(Arc::new(
ClaudeBackend::new(...)), model)` (o, in uno slice futuro, lo stesso con un altro `ChatBackend`).
`provider_name` è passato dal chiamante invece di essere derivato dal backend via un metodo
`describe()` sul trait — tiene `ChatBackend` minimo (un solo metodo).

**Design 2 — il corpo del loop non cambia, cambia solo la chiamata al provider:** `respond()`
è la stessa macchina a stati di prima (gate in blocco, dispatch, cap `MAX_ITERATIONS`,
cancellazione cooperativa, igiene storia su ogni percorso di uscita) — l'unica differenza reale è
`self.backend.send_turn(&system, &turn_tools, &snapshot_history, &mut on_text)` al posto di
`self.messages.create_streaming(req, &mut on_text)`, e `TurnStop`/`resp.stop`/`turn.blocks` al
posto di `resp.stop_reason`/`resp.assistant_blocks()`.

**Design 3 — perché `tools: &[ToolSpec]` e non `&[ToolDef]`:** vedi il commento sul trait in
`chat_backend.rs` e il test `web_search_mode_includes_server_tools` — Anthropic espone tool
server-side (`web_search`/`web_fetch`) che non hanno una forma `ToolDef`; `&[ToolSpec]` li
preserva senza che il loop debba saperne nulla.

**Test:** i 24 test non-`StubAdapter` di `ai_adapter.rs` sono stati migrati (stessa copertura,
stesse asserzioni) da `FakeMessagesClient`/`ClaudeAdapter` a `FakeChatBackend`/`LlmAdapter`; il
test `nowin_excludes_show_markdown_through_handle_command` in `core.rs` è stato aggiornato allo
stesso modo (costruisce `LlmAdapter`+`ClaudeBackend` invece di `ClaudeAdapter`, stesso
`FakeMessagesClient` sotto per ispezionare il wire reale). Un secondo test in `core.rs`,
`web_search_flag_threads_to_request`, costruiva `ClaudeAdapter` direttamente allo stesso modo e
non era stato elencato nel piano originale: migrato con la stessa identica trasformazione (era
obbligatorio per la compilazione, non opzionale — `ClaudeAdapter` non esiste più). Una singola
asserzione (`req.model` in `claude_text_only_end_turn_returns_text`) non ha un equivalente sul
`RecordedTurn` di `FakeChatBackend` (il campo `model` non attraversa più il seam `ChatBackend`,
resta interno a `ClaudeBackend`): la copertura non è persa, è già garantita da
`claude_backend::tests::forwards_system_tools_and_history_into_messages_request`. Rimossi anche
tre metodi di `FakeMessagesClient` (`repeating`/`nth_request`/`request_count`) rimasti orfani
dopo la migrazione (i loro unici chiamanti erano proprio i test di `ai_adapter.rs` migrati a
`FakeChatBackend`) — altrimenti avrebbero prodotto un nuovo warning `dead_code` non presente
prima di questo task.

**Nota per il prossimo slice:** aggiungere un secondo `ChatBackend` (OpenRouter) richiede
un lavoro speculativo sul wire format OpenAI-compatibile — da fondare su una risposta REALE
catturata da OpenRouter/DeepSeek (non solo sulla documentazione), per evitare di scrivere test di
traduzione contro un contratto sbagliato. Vedi `Docs/superpowers/specs/2026-07-06-openrouter-tooluse-adapter-design.md`.

## OpenRouterBackend — tipi wire OpenAI-compatibili (0.38.1, Slice 2 Task 1)

**Obiettivo:** rappresentare in Rust il wire format `chat/completions` (OpenRouter/DeepSeek),
validato contro una risposta REALE catturata (non solo la documentazione) — vedi
`Docs/superpowers/fixtures/2026-07-06-deepseek-openrouter-real-roundtrip/README.md`.

**Design 1 — `OrMessage` è condiviso fra richiesta e risposta:** a differenza di
`MessagesRequest`/`MessagesResponse` (Anthropic, tipi separati), `OrMessage` serve sia per
costruire l'array `messages` in richiesta sia per deserializzare `choices[].message` in
risposta — stessa forma in entrambe le direzioni. I campi `refusal`/`reasoning` che la risposta
porta (sempre `null` nei casi osservati) non sono modellati: un campo JSON extra non dichiarato
nello struct viene scartato da serde di default.

**Design 2 — `tool_calls: Option<Vec<OrToolCall>>` assente vs. `null`:** la fixture mostra che
`tool_calls` è **assente** dalla risposta (non `null`, non `[]`) quando il modello non chiama un
tool. `Option<T>` in serde tratta "chiave assente" e "chiave presente con `null`" allo stesso
modo (`None`), senza bisogno di `#[serde(default)]` esplicito — verificato dal test
`or_response_turn2_deserializes_text_only_with_absent_tool_calls`.

**Design 3 — `content: Option<String>`, non `Vec<Block>`:** a differenza del `Block` Anthropic
(un array di blocchi tipizzati), il `content` OpenAI-compatibile per un turno `user`/`tool` è una
stringa semplice (confermato dalla fixture: `"content": "Elenca i file..."`, non un array). Le
strutture dati esistenti (`Message`/`Block` in `messages_client.rs`) restano la rappresentazione
canonica in memoria — la traduzione avviene solo ai bordi (Task 2/3/4).

**Design 4 — `content` NON ha `skip_serializing_if` (deviazione dal piano originale, scoperta dal
ciclo RED→GREEN, non un'assunzione a priori):** il piano di Slice 2 annotava tutti e tre i campi
`Option` di `OrMessage` (`content`/`tool_calls`/`tool_call_id`) con
`#[serde(skip_serializing_if = "Option::is_none")]` in modo uniforme. Il test
`or_request_turn2_serializes_exactly_like_the_captured_fixture` (confronto byte-per-byte via
`serde_json::Value` con `turn2-request.json`, la fixture reale) fallisce con quell'attributo su
`content`: il messaggio `assistant` con `tool_calls` nella richiesta REALMENTE accettata da
OpenRouter/DeepSeek porta `"content": null` **esplicito**, non il campo assente. `tool_calls` e
`tool_call_id`, invece, sono davvero assenti (non `null`) quando non applicabili — per quei due
campi `skip_serializing_if` resta corretto e invariato. `Option<String>` senza l'attributo
serializza `None` come JSON `null` per default in `serde_json`, esattamente il comportamento
richiesto; le richieste dove `content` è `Some(...)` (turno 1, messaggi `user`/`tool` del turno 2)
non sono affette, perché in quei casi il campo era comunque presente. La fixture reale, non la
documentazione OpenAI generica, è stata il giudice finale (coerente con l'obiettivo dichiarato di
questo task).

**Test:** 4 test — 2 confrontano la serializzazione di un `OrRequest` costruito a mano con
`serde_json::Value` letto dalla fixture reale (`turn1-request.json`/`turn2-request.json`,
byte-per-byte via uguaglianza `Value`, quindi indipendente dall'ordine dei campi); 2 deserializzano
le risposte reali (`turn1-response.json`/`turn2-response.json`) e verificano `content: null`,
`arguments` come stringa, `tool_calls` assente.

## OpenRouterBackend — to_or_messages (0.38.2, Slice 2 Task 2)

**Obiettivo:** tradurre la storia `Vec<Message>` (rappresentazione canonica, invariata) nella
forma OpenAI-compatibile, per i tre casi enumerati nella spec §4.2.

**Design — system prompt come messaggio, non campo:** OpenAI-compatibile non ha un campo
top-level `system` (a differenza di `MessagesRequest::system`, Anthropic): `to_or_messages`
antepone un messaggio `role:"system"` solo se `system` non è vuoto — nessuna differenza di
comportamento quando `system` è vuoto (es. da un futuro caller che non lo usa).

**Design — srotolamento dei risultati tool:** un turno `user` con N blocchi `Block::ToolResult`
(prodotto oggi da `ai_adapter.rs` con `Message::with_blocks("user", results)`, un blocco per tool
eseguito nello stesso turno) diventa N messaggi `role:"tool"` separati — OpenAI richiede un
messaggio indipendente per ogni `tool_call_id`, non un turno multi-blocco. Verificato contro la
fixture reale nel test end-to-end di Task 6.

**Test:** 5 — system omesso/anteposto, assistant testo+tool_use in un solo messaggio, assistant
tool_use-solo (content None, non stringa vuota), N tool result → N messaggi separati, `is_error`
ripiegato in `content`.

## OpenRouterBackend — to_or_tools (0.38.3, Slice 2 Task 3)

**Obiettivo:** tradurre `&[ToolSpec]` (già passato al trait `ChatBackend::send_turn` intatto —
decisione presa in Slice 1 proprio per questo, vedi `chat_backend.rs`) in `Vec<OrTool>`.

**Design — perché `ToolSpec::Server` si scarta silenziosamente (con log):** un tool server-side
Anthropic (es. `web_search`, eseguito da Anthropic stessa) non ha un `input_schema`/endpoint
equivalente su un provider OpenAI-compatibile — non c'è nulla da tradurre. Scartarlo con
`tracing::warn!` (non un errore) è coerente con la decisione di design "nessuna probing statica di
capacità dei modelli" (spec §5): il cursore su OpenRouter semplicemente non avrà `web_search`,
comportamento visibile solo come assenza di quel tool, non come crash.

**Test:** 3 — tool custom tradotto 1:1, tool server-side scartato in una lista mista, lista vuota
→ output vuoto.

## OpenRouterBackend — from_or_response (0.38.4, Slice 2 Task 4)

**Obiettivo:** tradurre `OrResponse` (Task 1) in `BackendTurn` (Slice 1) — l'ultimo pezzo di
traduzione pura prima di assemblare `OpenRouterBackend` (Task 6).

**Design — `arguments` malformato è un errore esplicito, mai un drop silenzioso:** a differenza di
Anthropic (dove `input` è già un `serde_json::Value` nativo, nessun parsing coinvolto),
`function.arguments` OpenAI-compatibile è una stringa JSON che PUÒ non esserlo — `BackendError::Decode`
esiste proprio per questo caso (introdotto in Slice 1 come "nuovo, senza equivalente Anthropic").

**Design — `content: None`/stringa vuota non produce un `Block::Text` vuoto:** un blocco di testo
vuoto in storia sarebbe rumore puro (mai utile a un turno successivo) — lo stesso principio già
applicato lato Anthropic (`MessagesResponse::assistant_blocks`, che filtra solo `Text`/`ToolUse`,
non introduce blocchi vuoti).

**Test:** 6 — testo semplice → `Normal`, tool_call decodificato in `Block::ToolUse`, `arguments`
malformato → `Decode`, `length`→`MaxTokens`, `content_filter`→`Refused{None}`, `content:null` non
produce un blocco di testo fantasma.

## OpenRouterBackend — OpenRouterClient + FakeOpenRouterClient (0.38.5, Slice 2 Task 5)

**Obiettivo:** un seam di trasporto (mirror esatto di `MessagesClient`/`FakeMessagesClient` in
`messages_client.rs`) sotto cui Slice 3 innesterà `HttpOpenRouterClient` senza toccare
`OpenRouterBackend` (Task 6) — stesso principio DIP già usato per `ClaudeBackend`/`MessagesClient`.

**Test:** 3 — il fake registra la richiesta e rigioca le risposte in sequenza; un errore
configurato riemerge invariato da `create()`; `ok()` risponde con la risposta data.

## OpenRouterBackend — assemblaggio ChatBackend (0.38.6, Slice 2 Task 6)

**Obiettivo:** completare la Slice 2 — un secondo `ChatBackend` reale (dietro un client
iniettabile, non ancora HTTP reale), pronto per essere costruito con `LlmAdapter::new(Arc::new(
OpenRouterBackend::new(...)), "openrouter".to_string())` non appena Slice 3/4 forniranno il
trasporto e il wiring.

**Design — stesso schema di `ClaudeBackend`, traduzione sostanziale invece di passthrough:**
`send_turn` assembla `OrRequest` (Task 2+3), chiama `self.client.create(req)`, e traduce la
risposta (Task 4) — identico a `ClaudeBackend::send_turn`, con la differenza che qui il "wire" non
coincide con la rappresentazione interna (`Block`/`Message`), quindi c'è vera conversione ad ogni
passo.

**Design — perché `max_tokens: Option<u32>` (non `u32` come in `ClaudeBackend`):** Anthropic
richiede `max_tokens` in ogni richiesta; OpenAI-compatibile lo rende opzionale (il modello ha un
default). `#[serde(skip_serializing_if = "Option::is_none")]` su `OrRequest::max_tokens` omette il
campo quando `None` — coerente con la fixture reale, che non lo includeva affatto.

**Test:** 5 — richiesta assemblata correttamente (system anteposto, tools tradotti, model/
max_tokens forwardati), `on_text` invocata una sola volta (non-streaming), errore di rete
propagato, `Decode` di un `arguments` malformato propagato attraverso `send_turn` (non solo a
livello di `from_or_response`), e il test capstone: l'intero round-trip a 2 turni della fixture
reale rigiocato attraverso `OpenRouterBackend`, verificando sui campi discriminanti (ruoli,
`content:None`, `tool_calls[].function.arguments`, `tool_call_id`) che la richiesta del turno 2
(`assistant` con `tool_calls`+`content:null`, seguito da un messaggio `tool` separato) abbia la
forma che la fixture ha dimostrato essere accettata dal provider reale — il confronto
byte-per-byte via uguaglianza `Value` resta quello di
`or_request_turn2_serializes_exactly_like_the_captured_fixture` (Task 1), qui non ripetuto.

**Nota per la prossima slice (Slice 3):** `HttpOpenRouterClient` (trasporto reqwest reale, stessa
forma di `HttpMessagesClient` in `messages_client.rs`: URL, `Authorization: Bearer`, header
opzionali `HTTP-Referer`/`X-Title`), un test `#[ignore]` e2e reale contro
`deepseek/deepseek-chat`, ed e2e manuale dal vivo prima del commit (convenzione di progetto). Vedi
spec §7 punti 4-6 e §8.

## HttpOpenRouterClient — trasporto reale (0.38.7, Slice 3 Task 1)

**Obiettivo:** dare a `OpenRouterClient` (Slice 2) un'implementazione reale, dopo che
`FakeOpenRouterClient` ha permesso di validare `OpenRouterBackend` senza rete.

**Design 1 — un solo timeout, non il doppio idle/totale di `HttpMessagesClient`:** OpenRouter
(v1, non-streaming) risponde con un singolo `send().await` — non c'è uno stream SSE che può
restare silenzioso a metà per un tool-use server-side (il problema che ha motivato il timeout
di inattività per-chunk in `messages_client.rs`). Un solo `DEFAULT_TIMEOUT` (120s) di backstop
contro un hang patologico basta.

**Design 2 — `HTTP-Referer`/`X-Title` opzionali, mai richiesti:** OpenRouter li usa solo per
attribuire la chiamata a un progetto nella propria dashboard (leaderboard) — nessun effetto
sul comportamento dell'API. `with_attribution` è l'unico modo per impostarli, `None` di default.

**Design 3 — test contro un server TCP locale, non una libreria di mocking:** stesso pattern
già collaudato in `messages_client.rs` (nessuna nuova dipendenza `wiremock`/`mockito`) — un
`tokio::net::TcpListener` locale risponde con byte HTTP scritti a mano, permettendo di verificare
header/status/errori senza toccare la rete reale né introdurre dipendenze.

**Test:** 5 — auth header + path corretti (risposta reale dalla fixture Slice 2 rigiocata),
mapping status non-2xx → `OrError::Http`, mapping fallimento di connessione → `OrError::Network`,
header di attribuzione inviati quando configurati, assenti di default.

## e2e reale contro DeepSeek (0.38.8, Slice 3 Task 2 — completa)

**Obiettivo:** chiudere la Slice 3 dimostrando che `HttpOpenRouterClient`+`OpenRouterBackend`
funzionano DAVVERO contro il provider reale, non solo contro `FakeOpenRouterClient`.

**Design — costruito direttamente nel test, non via `main.rs`:** stessa scelta di
`real_streaming_invokes_on_text` in `messages_client.rs` — validare il trasporto non richiede
il wiring di selezione provider (`llms/llms.json`, Slice 4); il test costruisce
`HttpOpenRouterClient::new(key)` e `OpenRouterBackend::new(...)` direttamente.

**Test:** 1, `#[ignore]` — richiede `OPENROUTER_API_KEY` e rete, verificato manualmente
(convenzione di progetto: e2e dal vivo prima del commit per test che toccano provider reali).

## llms_config — schema e validazione di llms/llms.json (0.38.9, Slice 4 Task 1)

**Obiettivo:** rendere `ClaudeBackend`/`OpenRouterBackend` (Slice 1-3) selezionabili tramite un
file di config opzionale, senza ancora toccare `main.rs` (Task 2 collega il modulo).

**Design 1 — mirror di `telegram::settings`:** `resolve_path`/`load` hanno la STESSA forma già
collaudata per `telegramsettings.json` (override env var o cartella di lancio; `Ok(None)` per
file assente, `Err` per file illeggibile/malformato) — stessa convenzione, stessa robustezza,
nessuna sorpresa per chi già conosce quel modulo.

**Design 2 — `Err` di `build_adapter` è sempre "usa il fallback", mai un'invenzione di dettaglio:**
provider attivo assente, `api_key_ref` non trovato, o campo `provider` sconosciuto tornano tutti
`Err(String)` — il chiamante (Task 2) non distingue i casi, tratta qualunque `Err` allo stesso modo
(fallback al comportamento pre-Slice-4). Questo tiene la logica di `main.rs` semplice: un solo
punto di decisione (`Ok` vs `Err`), non un match su N tipi di errore diversi.

**Design 3 — le key non compaiono MAI in un messaggio di errore:** `resolve_api_key` include il
NOME del riferimento mancante (`api_key_ref`) ma mai il valore di una key esistente — stessa
cautela di `telegram::settings` per il token del bot. Verificato dal test
`resolve_api_key_errs_and_does_not_leak_when_ref_missing`.

**Deviazione dal brief (mechanical, non architetturale):** il test
`build_adapter_errs_on_unknown_provider_kind` nel brief usa `.unwrap_err()` su
`build_adapter(&cfg)` (tipo `Result<Arc<dyn AiAdapter>, String>`); questo non compila perché
`unwrap_err()` richiede `T: Debug` sul tipo `Ok`, e `dyn AiAdapter` non implementa `Debug`. Fix:
`.err().unwrap()` — stesso identico idiom già adottato in `telegram::settings::tests` per lo
stesso vincolo del compilatore (vedi il commento lì: "Usiamo `err().unwrap()` per evitare il
bound `T: Debug` su `unwrap_err()`"). Nessun cambio di comportamento, solo sintassi del test.

**Hardening post-review: `Debug` rimosso da `LlmsConfig`/`ProviderConfig`.** Il brief lo
specificava (distrazione, non scelta di design) — corretto in review: `LlmsConfig` porta
`api_keys: HashMap<String, String>` con segreti reali, e il modulo che questo mirror-a
(`telegram::settings::TelegramSettings`) omette `Debug` di proposito proprio per questo motivo
("il campo `token` non deve mai comparire nei log"). Nessun path di produzione formattava questi
tipi con `{:?}` (non era un leak attivo), ma lasciarli `Debug` sarebbe stato un gap di hardening
latente rispetto al precedente già stabilito. Rimosso da entrambi i derive; due test che usavano
`.unwrap_err()` su un `Result` con `&ProviderConfig`/`Arc<dyn AiAdapter>` come tipo `Ok` sono
passati a `.err().unwrap()` (stesso idiom, nessun cambio di asserzione).

**Test:** 14 — 3 `resolve_path` (default/override-vuoto/override-esplicito), 3 `load`
(assente/valido/malformato), 2 `find_active` (trovato/non-trovato), 2 `resolve_api_key`
(trovato/non-trovato-senza-leak), 4 `build_adapter` (seleziona OpenRouter, seleziona Claude,
provider sconosciuto, provider attivo assente).

**Verifica:**
```
cargo test -p orchestrator llms_config  → 14 passed
cargo test -p orchestrator               → tutti i test lib + integration invariati, verdi
cargo clippy -p orchestrator --all-targets → stessi 4 warning pre-esistenti (ai_adapter.rs
  map_or, ws.rs deref, 2x aichat/service.rs expect_fun_call), nessuno nuovo introdotto da
  llms_config.rs
cargo build -p orchestrator → pulito
```

**Resta per il Task 2:** wiring in `main.rs` — al momento nessun codice di produzione chiama
`llms_config::load`/`build_adapter`; il modulo è completo e testato ma dormiente.

## llms.json wired in main.rs (0.39.0, Slice 4 Task 2 — completa)

**Obiettivo:** chiudere la Slice 4 collegando `llms_config` (Task 1) a `main.rs` — la prima volta
che un provider diverso da Anthropic diretto è selezionabile in produzione.

**Design 1 — `default_ai_adapter()` è un'estrazione, non una riscrittura:** il branch
`ANTHROPIC_API_KEY`/`StubAdapter` esisteva da Slice 1 (in realtà da prima) — questo task lo
sposta in una funzione a sé stante, senza toccarne una riga di logica. È il fallback per
QUALUNQUE esito non-`Ok(Some(_))` di `llms_config::load` o QUALUNQUE `Err` di `build_adapter`:
la retrocompatibilità è garantita dal fatto che è LETTERALMENTE lo stesso codice eseguito, non
da un test che lo confronta.

**Design 2 — `launch_dir` catturato PRIMA del blocco AI adapter:** prima di questa slice,
`launch_dir` veniva catturato più tardi (subito prima del `cd` verso la home, per
`telegram::settings::resolve_path`). `llms_config::resolve_path` ne ha bisogno prima — la
cattura (`std::env::current_dir()`, economica) è stata spostata più in alto; il blocco
`set_current_dir(home)` resta dove sempre.

**Design 3 — bump MINOR, non patch:** stesso criterio di `0.38.0` (rimozione di `ClaudeAdapter`
in favore di `LlmAdapter`) — non perché l'API pubblica del crate cambi, ma perché il
COMPORTAMENTO selezionabile dall'utente finale cambia in modo sostanziale per la prima volta in
questo arco di slice.

**Verifica manuale (responsabilità del supervisore, non del subagent implementatore — vedi
`CLAUDE.md`):** avvio dal vivo SENZA `llms/llms.json` → stessi log di sempre
("AI provider: Claude..." o "ANTHROPIC_API_KEY non impostata — uso StubAdapter"); avvio dal vivo
CON un `llms/llms.json` che seleziona `deepseek-openrouter` → log
"AI provider da llms.json: deepseek-openrouter".

## llms_config — path in %LOCALAPPDATA% (0.39.1, Slice 5 Task 1)

**Obiettivo:** correggere `llms_config::resolve_path` (Slice 4) prima di aggiungere un secondo
processo (il tab `/config` della UI) che deve scrivere nello stesso file — vedi
`Docs/superpowers/specs/2026-07-07-llms-config-ui-tab-design.md` §0 per il ragionamento completo
(perché `launch_dir`-relativo non funziona più con due processi).

**Design — `default_path` parametrizzata, non letta da `std::env::var` al proprio interno:**
mutare `std::env::var` nei test è pericoloso sotto esecuzione parallela — `aichat_settings.rs`/
`search_settings.rs` (crate `ui`) evitano il problema semplicemente non testando le loro funzioni
di path. Qui si fa un passo in più: `default_path` prende `local_appdata: Option<String>` come
parametro esplicito, quindi è testabile end-to-end (incluso il fallback `.lare-data`) senza
toccare l'ambiente reale del processo — `resolve_path` (pubblica) resta la sola a leggere
`std::env::var("LOCALAPPDATA")` davvero, in un punto solo.

**Test:** 4 — override esplicito vince (invariato dalla Slice 4), default con `LOCALAPPDATA`
impostata, fallback `.lare-data` quando assente, fallback `.lare-data` quando vuota (stesso
trattamento di un override vuoto).

## `LARE_LOCAL_DIR` — override generale della cartella Local app-data (0.40.74)

**Obiettivo:** lasciare all'utente la possibilità di ridirigere l'intera base
`%LOCALAPPDATA%\dev.lare.terminal\` a un percorso arbitrario (es. una cartella condivisa fra più
macchine sotto una radice comune), senza toccare il comportamento di default quando non impostata.

**Precedenza in ogni sito che risolve la base Local** (override specifico del sito, se esiste →
`LARE_LOCAL_DIR` → `LOCALAPPDATA` → fallback `.lare-data`): `llms_config::default_path` (sopra),
`ai_adapter.rs::memory_file_path_with_base`, `aichat/service.rs::memory_file_path_with_base`
(duplicato indipendente, stesso schema), e i 5 siti inline di `main.rs` (token, stato Telegram,
`search-paths.json`, plugin host/storage, note/aichat inbox). `LARE_LOCAL_DIR` è **verbatim**: se
impostata (non vuota dopo `.trim()`), sostituisce l'intera cartella base — nessun `dev.lare.terminal`
viene unito sopra — e i `.join(...)` specifici di ogni sito (`token`, `network.json`, `llms.json`,
`memory-*.md`, ecc.) restano invariati sopra qualunque base risolta.

**Design — nessun helper condiviso:** ogni sito duplica la stessa piccola guardia
(`match std::env::var("LARE_LOCAL_DIR") { Ok(dir) if !dir.trim().is_empty() => ..., _ => ... }`)
invece di un modulo comune — coerente con lo stile preesistente del crate: il pattern
`LOCALAPPDATA`-con-fallback-`.lare-data` era già duplicato ~13 volte prima di questo piano; questo
non introduce un'astrazione nuova, allunga di un ramo quella già in uso ovunque.

**Diagnostica:** nuovo `tracing::info!("Local data dir: {path}")` sul sito del token in `main.rs`
(il primo risolto all'avvio), così la cartella effettiva è visibile in log senza doverla dedurre a
mano.

**Test:** 4 nuovi — 2 in `llms_config.rs` (`default_path_local_dir_override_wins_over_local_appdata`,
`default_path_ignores_empty_local_dir_override`), 1 in `ai_adapter.rs` e 1 in `aichat/service.rs`
(`memory_file_path_local_dir_override_wins_over_local_appdata`). I 5 siti inline di `main.rs` non
sono unit-testati (stesso limite del codice che sostituiscono — dentro `main()`, non estratto in
funzioni).

## AI Chat — ancoraggio identita' + ruoli veri (0.39.2)

**Obiettivo:** correggere il mirroring d'identita' osservato dal vivo — vedi
`Docs/superpowers/specs/2026-07-07-aichat-identity-anchor-design.md` per la diagnosi completa
(due transcript reali riprodotti nello spec).

**Design 1 — due fix indipendenti, non uno:** il system prompt (`chat_system_prompt`/
`autoparticipate_system_prompt`, funzioni non piu' `const`) porta l'identita' (label+provider);
la history a ruoli veri (`build_ai_history`) fa in modo che le proprie righe passate siano
strutturalmente `assistant`, non testo piatto indistinguibile. Il primo dice "chi sono", il
secondo "quali righe erano mie" — servono entrambi, verificato con due transcript reali in cui
l'identita' dichiarata IN CONVERSAZIONE (dall'umano, a voce) e' sopravvissuta un solo turno prima
di rompersi di nuovo: il transcript non e' un posto stabile per l'identita', il system prompt si
(l'unico slot rimandato identico ad ogni turno).

**Design 2 — coalescenza dei ruoli consecutivi:** una stanza puo' avere piu' partecipanti diversi
da chi viene invocato (altri umani, altre AI) che scrivono di fila prima dell'invocazione — tutte
quelle righe sono "user" dal punto di vista di chi risponde. Senza coalescenza, l'array di turni
avrebbe `[user, user, ...]` consecutivi, rifiutati dall'API Anthropic con HTTP 400 (stessa igiene
gia' garantita nel loop del cursore, `ai_adapter.rs::history_stays_clean_after_error`).

**Invariante:** l'ultima entry della history costruita da `build_ai_history` e' sempre `user` —
`chat_reply`/`chat_autoparticipate` scattano solo su un messaggio altrui, mai auto-invocazione.

**Test:** 6 nuovi per `build_ai_history` (mappatura ruoli, coalescenza, non-coalescenza fra ruoli
diversi, storico vuoto, invariante "riga altrui mai assistant"), 5 test esistenti aggiornati alla
nuova firma di `chat_reply`/`chat_autoparticipate` (incluso un test che riproduce lo scenario
reale: history con l'auto-identificazione errata di un'altra AI, verifica che il system prompt
calcolato ancori comunque l'identita' del CHIAMANTE).

**Nota sul piano/esecuzione:** il piano originale divideva questo lavoro in due task ("Task 1":
`ai_adapter.rs`; "Task 2": `aichat/service.rs`), ma il cambio di firma di un trait e' atomico
attraverso il crate — la definizione, ogni `impl`, e ogni call site devono muoversi insieme o
niente compila. Non esiste un sottoinsieme a singolo file che raggiunga uno stato verde; il
censimento esaustivo (`grep -rn "impl.*AiAdapter for"` + tutti i call site di `chat_reply`/
`chat_autoparticipate`) ha anche scoperto un terzo `impl AiAdapter` non censito dal piano
originale: `telegram::channel::ProbeAdapter` (un fake usato solo per testare il wiring del
`ToolConfirmer`, con firma qualificata per esteso `impl crate::ai_adapter::AiAdapter for
ProbeAdapter`, che un grep in testo semplice sul piano non aveva individuato). Le tre modifiche —
`ai_adapter.rs`, `telegram/channel.rs`, `aichat/service.rs` — sono state quindi eseguite come
un'unica unita' di esecuzione ed entrano in un solo commit.

**Fuori scope (slice futura):** file di memoria persistente (`memory-<label_base>.md` in
`%LOCALAPPDATA%`), marker `MEMORIA:` per il remember umano/AI-con-consenso — vedi spec §7.

## AI Chat — memoria persistente per-macchina (0.40.0)

**Obiettivo:** vedi `Docs/superpowers/specs/2026-07-07-aichat-persistent-memory-design.md` —
continuazione diretta della Slice 1 (ancoraggio identità), stessa infrastruttura
(`chat_system_prompt`/`autoparticipate_system_prompt` come funzioni, `my_ai_label` già passato).

**Design 1 — niente nuova firma di trait:** dopo il costo della Slice 1 (5 impl da aggiornare),
questa slice evita deliberatamente di ritoccare `AiAdapter::chat_reply`/`chat_autoparticipate` —
`LlmAdapter` deriva `label_base` da `my_ai_label` (togliendo il suffisso `-ai`) e risolve/legge il
proprio file da sé.

**Design 2 — duplicazione deliberata del path fra lettura e scrittura:** `ai_adapter.rs` e
`aichat/service.rs` hanno CIASCUNO una propria `memory_file_path` (stesso corpo, stesso schema
`%LOCALAPPDATA%\dev.lare.terminal\memory-{label_base}.md`) — stesso principio già in uso fra
`llms_config::resolve_path` (orchestrator) e `llm_settings::llms_json_path` (crate `ui`, Slice
5): due lati indipendenti, path identico per costruzione, nessun coordinamento a runtime.

**Design 3 — un solo meccanismo di scrittura, mai deterministico:** sia il remember su richiesta
di un companion sia la proposta spontanea dell'AI passano dallo STESSO canale — un marker
`MEMORIA:` nella risposta dell'AI stessa. Non c'è mai un'estrazione diretta dal testo di una
richiesta umana: la scelta (se, cosa, come formulare) resta sempre dell'AI.

**Design 4 — concorrenza:** `chat_reply` e `chat_autoparticipate` non sono mutuamente esclusi
(solo autoparticipate-vs-autoparticipate lo è, via `autoparticipate_inflight`) — un
`Arc<Mutex<()>>` (`memory_write_lock`, `std::sync::Mutex` non `tokio::sync::Mutex`: la sezione
critica è I/O sincrono, nessun `.await` mentre il lock è tenuto) protegge il read-modify-write
dell'append.

**Test:** 5 in `ai_adapter.rs` (risoluzione path con/senza `LOCALAPPDATA`, system prompt con/senza
memoria esistente, marker sempre presente nelle istruzioni), 11 in `aichat/service.rs`
(`extract_memoria_marker`: nessun marker, estrazione, solo la prima occorrenza, nota vuota →
`None`; `Effect::PersistMemory` emesso su `AiReply` con/senza marker e su `AutoParticipateDone`
con marker — il caso senza marker su quest'ultimo arm è coperto indirettamente dai test puri di
`extract_memoria_marker`; `memory_file_path_with_base` con/senza `LOCALAPPDATA`;
`append_note_at` crea cartella+file assenti e accoda senza sovrascrivere, su una cartella
temporanea — trovato mancante in review, aggiunto prima del merge).

**Fuori scope:** nessun limite di crescita del file (YAGNI, si affronta se/quando diventa un
problema osservabile); nessuna UI per leggere/editare la memoria (resta un file di testo);
nessun comando umano esplicito tipo `/remember` (per scelta di design, la scrittura resta SOLO
una scelta dell'AI).

## AI Chat — sopprimi auto-partecipazione su invocazione altrui (0.40.1)

**Obiettivo:** vedi `Docs/superpowers/specs/2026-07-08-aichat-autoparticipate-explicit-invocation-design.md`
— bug osservato dal vivo, non teorico: un `@ai` bare da un peer remoto faceva rispondere anche
macchine non targetate se avevano `ai_autoparticipate` attivo.

**Design — il gate guarda "era un'invocazione", non "mi ha invocato":** `was_explicit_invocation`
(catturato PRIMA di valutare `targets_me`) sostituisce `already_invoked` (che guardava
`Effect::InvokeLocalAi` già prodotto). Un'invocazione esplicita ha SEMPRE un destinatario
chiaro — se stesso, tutti, o un'altra macchina specifica — quindi non è mai il caso "nessuno mi
ha invocato" che l'auto-partecipazione esiste per coprire.

**Decisione rivista, non un bug latente:** un test esistente
(`peer_say_at_other_label_with_auto_on_can_still_autoparticipate`, ora
`peer_say_at_other_label_with_auto_on_does_not_autoparticipate`) documentava esplicitamente il
comportamento precedente come design scelto. Discusso di nuovo con l'utente dopo l'osservazione
dal vivo, che ha preferito la garanzia deterministica a un'istruzione nel system prompt
(probabilistica, non garantita).

**Test:** 2 nuovi/invertiti (`@<altro-label>-ai` non autopartecipa più; lo scenario esatto
osservato dal vivo, `@ai` bare da remoto, non autopartecipa) + verifica di non-regressione sui 5
test esistenti dell'area (double-fire su invocazione propria, messaggio normale, `@all` remoto,
AI↔AI spontanea, guardia anti-loop sulla propria `-ai`).

## AI Chat — SILENCE tollerante a punteggiatura finale (0.40.2)

**Obiettivo:** bug osservato dal vivo (screenshot utente), successivo al fix 0.40.1: il testo
`"SILENCE."` (con punto finale, aggiunto da un modello chat-tuned che tende a punteggiare anche
una parola isolata come fosse una frase) veniva pubblicato in chat come contributo genuino invece
di essere riconosciuto come il sentinel di silenzio — rumore visibile, e osservato capace di
innescare a sua volta ulteriori cicli di auto-partecipazione sulle altre macchine.

**Fix — un solo punto, in `chat_autoparticipate` (`ai_adapter.rs`):** prima del confronto
case-insensitive con `"silence"`, il testo trimmato passa da
`trimmed.trim_end_matches(['.', '!', '?'])`, che rimuove SOLO caratteri di punteggiatura dalla
fine della stringa — non è un match di sottostringa, quindi una parola più lunga che contiene
"silen" come prefisso (es. "silenzioso") non collassa mai in "silence" per nessun trimming. Il
testo pubblicato in caso di `Some(..)` resta comunque `trimmed` originale (con l'eventuale
punteggiatura), non la versione trimmata — la tolleranza riguarda solo il riconoscimento del
sentinel, non altera il testo di un contributo genuino.

**Perché solo qui:** `chat_autoparticipate` è l'unico punto del crate che interpreta il sentinel
SILENCE — `chat_reply` (risposta a un'invocazione esplicita) non ha una convenzione di silenzio
analoga, quindi non serve toccarlo.

**Fuori scope, esplicitamente:** questo fix chiude la CODA rumorosa (SILENCE malriconosciuto
pubblicato come messaggio vero, e la cascata che ne seguiva), non il primo aggancio non voluto —
una macchina che risponde di sua iniziativa a un `@ai` rivolto esplicitamente a un'ALTRA macchina
resta comportamento intenzionale della chat "partecipativa" (Slice 2 del design originale), che
l'utente sta deliberatamente studiando dal vivo con due macchine reali. Non toccato in questa
fix — eventuale ridisegno (le AI che imparano a riconoscere quando sono o non sono interpellate)
resta materia di una futura sessione di brainstorming, non di una patch.

**Test:** 3 nuovi (`claude_autoparticipate_silence_with_trailing_punctuation_maps_to_none` —
`"SILENCE."`; `claude_autoparticipate_silence_with_other_trailing_punctuation_maps_to_none` —
`"Silence!"`/`"silence?"`/`"SILENCE.."`; `claude_autoparticipate_word_containing_silence_is_not_suppressed`
— guardia di non-regressione su "Il silenzioso non è la stessa cosa."). RED confermato prima del
fix (assert reali falliti, non errori di compilazione). 619 test verdi (crate intero), clippy
invariato (4 warning pre-esistenti, 0 nuovi).

## llms.json — `base_url` custom per provider `"anthropic"` (0.40.3)

**Obiettivo:** l'utente ha una API key DeepSeek diretta (non tramite OpenRouter) e DeepSeek
espone un endpoint dichiarato Anthropic-compatibile — stesso wire `/v1/messages`,
`x-api-key`/`anthropic-version`, sotto un host proprio. Voleva poterlo configurare in
`llms.json` come provider `"anthropic"` puntato altrove, invece che passare per forza da
`"openrouter"`.

**Nessun nuovo backend:** `ClaudeBackend` (`claude_backend.rs`) è già un passthrough puro sopra
`Arc<dyn MessagesClient>`, e `HttpMessagesClient::with_base_url(api_key, base_url)`
(`messages_client.rs`) accetta già un host arbitrario — `HttpMessagesClient::new` è solo
`with_base_url(key, "https://api.anthropic.com")`. Il buco era unicamente in `llms_config.rs`:
`build_adapter` costruiva sempre `HttpMessagesClient::new(key)`, nessun modo di override.

**Fix — un campo, una funzione pura, una riga di plumbing:**
- `ProviderConfig::base_url: Option<String>` (`#[serde(default)]` — assente = comportamento
  invariato, `api.anthropic.com` reale; ignorato per `provider: "openrouter"`, che ha un solo
  host fisso).
- `resolve_anthropic_base_url(provider: &ProviderConfig) -> String` — pura, stesso pattern di
  `resolve_api_key`/`default_path` già nel file: valore custom se presente, altrimenti
  `DEFAULT_ANTHROPIC_BASE_URL` (nuova const, stesso valore letterale di
  `HttpMessagesClient::new`).
- `build_adapter`, ramo `"anthropic"`: `HttpMessagesClient::with_base_url(key,
  resolve_anthropic_base_url(provider))` sostituisce `HttpMessagesClient::new(key)`.

**Uso:** in `llms.json`, un'entry `{"name": "deepseek-direct", "provider": "anthropic",
"model": "deepseek-chat", "api_key_ref": "deepseek", "base_url":
"https://api.deepseek.com/anthropic"}` — nessuna verifica automatica del wire DeepSeek in questa
slice, la conferma è l'uso dal vivo dell'utente. Editing manuale del file per ora: nessun campo
`base_url` nella UI `/config` → LLM in questa slice (fuori scope, deciso con l'utente).

**Test:** 3 nuovi (`resolve_anthropic_base_url_defaults_to_anthropic_when_absent`,
`resolve_anthropic_base_url_uses_custom_value_when_present`,
`build_adapter_succeeds_with_custom_anthropic_base_url`). RED confermato come errore di
compilazione (campo/funzione non ancora esistenti — codice nuovo, non cambio di comportamento
esistente). 622 test verdi (crate intero), clippy invariato (4 warning pre-esistenti, 0 nuovi).

## fix: DeepSeek 400 su `web_fetch`, ogni comando NL falliva (0.40.4)

**Sintomo riportato dall'utente:** con `deepseek-direct` (0.40.3) attivo, i comandi verso il SO
funzionavano ma OGNI comando in linguaggio naturale falliva con `[errore AI] HTTP 400:
{"error":{"message":"Failed to deserialize the JSON body into the target type: tools[4]: unknown
variant web_fetch_20260209, expected web_search_20250305 or web_search_20260209"...}`.

**Root cause (debug sistematico, `superpowers:systematic-debugging`):** `webSearchEnabled` in
`app.js` è `true` di default (mirror di `Config::default().web_search_enabled`). Con la ricerca
web attiva, `agent::tools_for` (`agent.rs`) aggiunge SEMPRE entrambi i tool server-side Anthropic
via `web_tools()` — `web_search` E `web_fetch`, nessuna distinzione — a ogni turno, per qualunque
provider `"anthropic"`. Va bene per Anthropic reale (supporta entrambi), ma un `base_url` custom
(0.40.3) è garanzia solo di compatibilità di WIRE — stesso `/v1/messages`, stessi header — MAI di
feature-parity. Lo shim Anthropic-compatibile di DeepSeek valida `tools[].type` contro un enum
Rust-like che ha SOLO varianti `web_search_*`: `web_fetch_20260209` non esiste in quell'enum,
quindi l'intera richiesta viene rifiutata prima ancora che DeepSeek veda il prompt — per questo
"ogni comando NL", non solo quelli che davvero avrebbero usato `web_fetch`.

**Perché non in `agent.rs`:** `agent::tools_for` è deliberatamente provider-agnostico (produce
l'insieme di tool INTENZIONALE, indipendente dal backend attivo). Il filtro per capacità
appartiene al layer di traduzione specifico del backend — stesso posto/stessa filosofia già
adottata da `OpenRouterBackend::to_or_tools`, che scarta `ToolSpec::Server` perché OpenAI non ha
un equivalente wire per i tool server-side Anthropic. `ClaudeBackend::send_turn` fa lo stesso,
ma selettivo: scarta SOLO `web_fetch` (non tutto `ToolSpec::Server`), perché `web_search` resta
supportato — confermato dal messaggio d'errore DeepSeek stesso, che lo elenca fra le varianti
attese.

**Fix:**
- `ClaudeBackend` — nuovo campo privato `web_fetch_supported: bool` (default `true` via `new`,
  comportamento invariato per Anthropic reale/`claude-direct`). Builder
  `with_web_fetch_supported(bool) -> Self` (non-breaking: `new` a 3 argomenti resta uguale, tutti
  i call site esistenti — test, `main.rs`, `core.rs` — invariati).
- `send_turn`: se `!web_fetch_supported`, filtra `tools` con `.filter(|t| t.name() !=
  WEB_FETCH_TOOL_NAME)` prima di costruire `MessagesRequest` — un `Vec<ToolSpec>` locale
  (`sent_tools`), non tocca lo slice ricevuto.
- `ProviderConfig::web_fetch_supported: bool` (`#[serde(default = "default_web_fetch_supported")]`
  → `true`, stesso pattern non-breaking di `base_url`). `build_adapter` la passa a
  `with_web_fetch_supported` incondizionatamente (il default `true` la rende un no-op per chi non
  ha ancora il campo in `llms.json`).

**Uso:** in `llms.json`, aggiungere `"web_fetch_supported": false` all'entry `deepseek-direct`.
Editing manuale per ora — stesso scope limitato di `base_url` (nessun campo UI in questa slice).

**Test:** 2 nuovi in `claude_backend.rs` (`strips_web_fetch_server_tool_when_not_supported` —
riproduce esattamente il bug con `FakeMessagesClient` + `.recorded()`, verifica che `web_search`
resti e `web_fetch` sparisca dalla richiesta effettiva; `forwards_web_fetch_when_supported_by_default`
— non-regressione per Anthropic reale), 2 nuovi in `llms_config.rs`
(`web_fetch_supported_defaults_to_true_when_absent`, `web_fetch_supported_reads_explicit_false`).
630 test verdi (crate intero), clippy invariato (2 warning pre-esistenti in `ws.rs`/`agent.rs`
non toccati da questa fix, 0 nuovi).

## llms.json — `base_url` custom anche per provider `"openrouter"` (0.40.5)

**Contesto:** subito dopo la 0.40.4 (fix HTTP 400), l'utente ha riportato dal vivo un sintomo
NUOVO su `deepseek-direct`: il testo mostrato in terminale conteneva markup letterale tipo
`<|DSML|tool_calls><|DSML|invoke name="open_target">...` invece di eseguire il tool. Investigato
con `superpowers:systematic-debugging` + ricerca web mirata: NON un bug di parsing lato client
(`messages_client.rs` riconosce già correttamente `content_block_start` di tipo `tool_use` via
SSE, coperto da test esistenti) — è lo shim Anthropic-compatibile di DeepSeek che a volte non
traduce le tool-call native del modello (sintassi "DSML", proprietaria di DeepSeek) in blocchi
Anthropic strutturati, e le lascia passare come testo grezzo. Confermato come bug upstream NOTO
e NON deterministico ("a volte sì, a volte no") — stesso identico sintomo riscontrato da un altro
progetto agentic (`pi-mono`) contro lo stesso modello via un provider diverso (NVIDIA), issue
pubblica citata nella conversazione con l'utente.

**Soluzione scelta dall'utente:** DeepSeek espone anche un endpoint OpenAI-compatibile DIRETTO
(`https://api.deepseek.com`, path `/chat/completions`, auth `Authorization: Bearer` — verificato
sulla documentazione ufficiale DeepSeek, non assunto). Il wire OpenAI (tool-calls come
`delta.tool_calls` strutturati, non testo libero) è la superficie PRIMARIA e matura di DeepSeek,
a differenza dello shim Anthropic-compatibile (più nuovo, meno testato) — e `OpenRouterBackend`
parla già quel wire, con traduzione validata contro una fixture REALE catturata da
OpenRouter/DeepSeek (`Docs/superpowers/fixtures/2026-07-06-deepseek-openrouter-real-roundtrip/`).
Bastava permettergli di puntare a un host diverso da `openrouter.ai`.

**Nessun nuovo codice di traduzione:** `HttpOpenRouterClient::with_base_url` esisteva già (usato
dai test, mai esposto a `llms_config.rs`). Il buco era identico a quello del fix 0.40.3 per
`"anthropic"`, ma sul ramo gemello: `build_adapter` costruiva sempre `HttpOpenRouterClient::new(key)`
(hardcoded `openrouter.ai`), e il commento su `ProviderConfig::base_url` dichiarava esplicitamente
"ignorato per `provider: openrouter`" — non più vero dopo questa slice.

**Fix — una riga di plumbing, zero logica DeepSeek-specifica:**
```rust
let http = match &provider.base_url {
    Some(url) => Arc::new(HttpOpenRouterClient::with_base_url(key, url.clone())),
    None => Arc::new(HttpOpenRouterClient::new(key)),
};
```
A differenza del ramo `"anthropic"` (che ha una `resolve_anthropic_base_url` pura + una costante
`DEFAULT_ANTHROPIC_BASE_URL` duplicata da `messages_client.rs`), qui non serve duplicare nulla:
`HttpOpenRouterClient::new` incapsula già il proprio default internamente — il branch su
`Option` delega direttamente. Generico per QUALUNQUE endpoint OpenAI-compatibile, non solo
DeepSeek.

**Uso:** in `llms.json`, nuova entry `{"name": "deepseek-openai-direct", "provider": "openrouter",
"model": "deepseek-v4-pro", "api_key_ref": "deepseek", "base_url": "https://api.deepseek.com"}`,
impostata come `active` al posto di `deepseek-direct`. Nota: `web_fetch_supported` non si applica
qui — `OpenRouterBackend::to_or_tools` scarta comunque OGNI `ToolSpec::Server` (nessun equivalente
wire OpenAI per i tool server-side Anthropic), stessa limitazione già presente per
`deepseek-openrouter`: niente `web_search`/`web_fetch` su questo percorso, di natura strutturale.

**Test:** 1 nuovo (`build_adapter_succeeds_with_custom_openrouter_base_url`, stesso pattern
black-box del test gemello per `"anthropic"` — verifica solo che la costruzione non fallisca, i
campi di `HttpOpenRouterClient` sono privati). 631 test verdi (crate intero), clippy invariato.

## Ricerca nei contenuti dei file (0.40.8)

**Obiettivo:** vedi `Docs/superpowers/specs/2026-07-10-ricerca-contenuti-design.md` — `/find` (v1,
solo-nome) guadagna un secondo filtro indipendente `in:"<frase>"` che cerca dentro il CONTENUTO
dei file, componibile in qualunque ordine con la query sul nome. `/find` senza `in:` resta
bit-per-bit invariato.

### Architettura a due fasi

Cercare nel contenuto richiede aprire il file — bisogna evitare binari ed evitare di rallentare
l'albero su file enormi. Ogni file incontrato dal walk (dopo il match sul nome, se presente) è
classificato per estensione in tre bucket (`search::content::ExtensionClass`, letti da
`search-content.json`, stesso pattern espandibile di `search-paths.json` — auto-generato al 1°
avvio, poi editabile a mano):

- **Text** → **Fase A**: aperto e cercato subito, durante il walk normale — hit live come oggi
  (`walk_root` chiama `ContentMatcher::find_first_match` inline, stesso ciclo che già cammina
  l'albero per il match-nome).
- **Binary** → mai aperto, saltato sempre. `.pdf` è qui fin da subito (vedi "Fuori scope" sotto).
- **Unknown** (né l'uno né l'altro, incluso nessuna estensione) → NON aperto durante il walk:
  accodato su un canale dedicato (`ContentSearch::unknown_tx`, `mpsc::Sender<(PathBuf,
  SearchSource)>`, capacità 256) per essere processato in **Fase B**, dopo che tutti i walker
  della Fase A sono terminati. La Fase B sniffa i primi 8000 byte di ogni file in coda
  (`sniff_file`/`looks_like_text` — assenza di byte null ⇒ sembra testo, euristica minima ma
  sufficiente: i formati binari comuni ne contengono quasi sempre uno molto presto); se passa lo
  sniff, viene cercato come i file Text. Cap `max_unknown_scan` (default 5000): oltre quel numero
  di file accodati, la Fase B si ferma e il risultato finale segnala `truncated`.

### Perché `hit_rx`/`unknown_rx` NON si possono drenare in sequenza — il deadlock trovato in review

Il design di riferimento del piano (task 8 originale) drenava `hit_rx` (Fase A) fino a esaurimento
PRIMA di iniziare a leggere `unknown_rx` (coda Fase B) — sequenziale. Questo è un **deadlock
permanente** su qualunque albero con più di 256 file Unknown (la capacità del canale
`unknown_tx`; `max_unknown_scan` di default è 5000, quindi lo scenario è realistico, non un caso
limite teorico):

1. Un walker trova il file Unknown #257 e chiama `unknown_tx.blocking_send(..)` — bloccante per
   davvero (`spawn_blocking`, non un `.await`).
2. Il canale è pieno (256 slot, tutti occupati) e **nessuno lo sta ancora drenando** (siamo ancora
   in Fase A, che secondo il design sequenziale drena solo `hit_rx`).
3. Il walker resta bloccato dentro `walk_root` per sempre — non ritorna mai, quindi non droppa mai
   il proprio `hit_tx` clonato.
4. `hit_rx.recv()` (che sta drenando la Fase A) non vede mai "tutti i sender spariti" — quel
   walker specifico tiene ancora in vita un `hit_tx`, anche se non lo userà mai più.
5. La Fase A non finisce **mai** → la Fase B non parte **mai** → nessuno drena mai `unknown_rx` →
   il walker del punto 1 resta bloccato per sempre. Deadlock, e **silenzioso**: nessun panic,
   nessun errore, la ricerca resta semplicemente appesa (osservabile solo come un `/find` che non
   emette mai `SearchDone`).

Non era un errore dell'implementatore del task 8: era un difetto del design originale del piano
stesso — la garanzia "chi droppa per ultimo `hit_tx`" era corretta in isolamento, ma non teneva
conto che il canale `unknown_tx`, del tutto separato, poteva bloccare lo stesso walker da cui quel
`hit_tx` dipendeva. Trovato in code review prima del merge (non da un test che falliva a runtime —
i test esistenti usavano tutti alberi piccoli, sotto la soglia dei 256 file).

**Fix (`be07ef4`):** un unico loop `tokio::select!` drena **entrambi** i canali
**concorrentemente**, mai in sequenza:

```rust
while hit_rx_open || unknown_rx_open {
    tokio::select! {
        hit = hit_rx.recv(), if hit_rx_open => { /* inoltra subito come SearchHit */ }
        item = unknown_rx.recv(), if unknown_rx_open => { /* accumula in un Vec, zero I/O */ }
    }
}
```

Gli hit di Fase A vengono ancora inoltrati subito (comportamento live invariato). I candidati
Unknown vengono SOLO accumulati in un `Vec` — questo loop non fa mai I/O, il suo unico scopo è
svuotare `unknown_rx` abbastanza in fretta perché nessun walker resti mai bloccato su
`blocking_send`. Il lavoro vero (sniff+cerca, Fase B) parte SOLO dopo che il loop esce — il che
succede quando ENTRAMBI `hit_rx_open` e `unknown_rx_open` sono `false`, cioè quando entrambi i
canali hanno riportato "nessun sender rimasto": una garanzia che ora vale per davvero, perché
ogni walker può sempre completare il proprio `blocking_send` (il loop lo sta drenando) e quindi
può sempre tornare da `walk_root` e droppare i propri sender.

**Secondo bug, trovato nella prima bozza dello stesso fix:** al raggiungimento di `result_cap`,
il codice originale smetteva di drenare solo `hit_rx`, lasciando `unknown_rx_open = true`. Ma
`hit_tx` è anch'esso un canale bounded (capacità 256): un walker ancora in corsa che trova un
altro match-nome si sarebbe bloccato per sempre su `hit_tx.blocking_send` — la stessa identica
classe di deadlock, spostata sull'altro canale. I due test preesistenti
(`caps_results_and_marks_truncated`, `launch_reloads_result_cap_from_disk`) sono stati il RED che
ha reso visibile questo secondo bug (fallivano non per timeout ma perché il conteggio finale non
tornava). Fix: al cap, `hit_rx_open = false` E `unknown_rx_open = false` insieme — uscire da
ENTRAMBI i rami fa sì che i receiver vengano droppati alla fine di `run`, e ogni `blocking_send`
pendente riceva `Err` e sblocchi il walker (non un hang, un errore gestito che il walker ignora).

**Lezione di concorrenza (generale, non specifica di questo progetto):** un canale bounded +
nessun drain attivo durante un invio bloccante = deadlock, indipendentemente da quanto sia
accurato il ragionamento su "chi droppa il sender per ultimo" relativo a un canale DIVERSO. Il
fix non ha introdotto nuovi meccanismi di sincronizzazione — ha solo smesso di assumere che i due
canali potessero essere trattati come indipendenti nel tempo.

**Test di regressione:** `content_search_does_not_deadlock_on_many_unknown_files` — 300 file
Unknown (oltre la capacità storica del canale), l'intera chiamata a `run()` è avvolta in
`tokio::time::timeout(Duration::from_secs(10), ..)`: se una futura regressione reintroduce il
deadlock, questo test fallisce in 10 secondi invece di appendere l'intera test suite per sempre.

### Fase B ora onora la pausa (trovato nella review finale sull'intero branch)

La Fase B controllava `cancel.is_cancelled()` ma non chiamava mai `gate.wait_while_paused(...)` —
il `PauseGate` passato a `run()` era riusato solo dai walker di Fase A. Deviazione dalla spec §5
("stesso `id`/`CancellationToken`/`PauseGate` riusati") e bug utente concreto: il pulsante ⏹ resta
visibile per tutta la Fase B (si nasconde solo su `search:done`), quindi mettere in pausa durante
una Fase B lunga (fino a `max_unknown_scan` = 5000 file, ognuno fino a `max_file_size_kb` = 5 MB)
cambiava l'etichetta di stato senza fermare nulla — gli hit continuavano ad arrivare.

Fix: il check di pausa entra nella STESSA chiusura `spawn_blocking` per-file di Fase B (quella che
fa `sniff_file`/`find_first_match`), PRIMA del check di cancellazione — stesso ordine già usato da
`walk_root` (`walk.rs`), per lo stesso motivo: un resume che arriva durante una pausa deve comunque
lasciar uscire un cancel arrivato nel frattempo, non solo un resume esplicito.

**Test:** `phase_b_respects_pause_gate` — usa un corpus di soli file `Unknown` (Fase A non emette
mai un `SearchHit` diretto per quelli, solo li accoda) così il *primo* `SearchHit` che arriva è
prova inequivocabile che la Fase B è partita: la pausa scatta esattamente lì, senza indovinare un
confine di fase basato sul tempo. RED confermato 3× contro il codice pre-fix (fallisce in
~20-30ms), GREEN confermato 5× dopo il fix, nessuna flakiness osservata.

### Decisioni chiave

- **Frase esatta, non token:** `ContentMatcher` cerca una sottostringa letterale
  case-insensitive riga per riga (`line.to_lowercase().contains(&phrase_lower)`), non un
  matcher AND-di-token come quello sul nome file — coerente con la spec (§2: "regex nel
  contenuto" e "token multipli" sono esplicitamente fuori scope).
- **Sniff su byte null, non su un parser di formato:** `looks_like_text` è un'euristica a costo
  quasi-zero (un solo `read` da 8000 byte, un solo `.contains(&0)`), non un rilevamento MIME
  vero. Falsi negativi/positivi sono accettati per design — è un filtro "sembra ragionevolmente
  testo", non una garanzia.
- **`.pdf` binario noto, aggancio futuro esplicito:** i PDF reali sono tipicamente
  compressi/codificati internamente — leggerne i byte grezzi quasi mai trova il testo, quindi
  metterli in `Unknown` sprecherebbe solo un giro di sniff che fallisce sempre. Restano nel
  bucket `binary_extensions` di `search-content.json` finché non esisterà un estrattore
  PDF→Markdown dedicato (backlog separato, memoria `open-doc-to-markdown-button`); a quel punto
  `/find in:"..."` dovrà usarlo esplicitamente per i `.pdf`, non riclassificarli come `Unknown`.
- **`launch` fa lo split, `SearchEngine::run` resta ignaro della sintassi `/find`:** `query::
  parse_find_input(&str) -> (Option<String>, String)` estrae `in:"<frase>"` dall'input grezzo
  (ovunque compaia) PRIMA che `run` veda la query — `run` riceve sempre una query-nome già
  risolta (mai la stringa `/find` grezza), mantenendo l'engine di ricerca disaccoppiato dal
  parsing della sintassi slash (SRP: `ws.rs`/`launch` sanno di `/find`, `SearchEngine` no).
- **Canale `unknown_tx` sempre creato, anche senza `in:`:** costo trascurabile (un `mpsc::channel`
  vuoto mai scritto) contro la semplicità di non avere due percorsi di codice paralleli in
  `SearchEngine::run` a seconda che `content` sia `Some`/`None`.

**Test:** 38 nuovi nel crate (622 → 660 lib): `query::parse_find_input` (posizione della
direttiva, assenza, virgolette non chiuse), `content::classify_extension` (Text/Binary/Unknown,
case-insensitive, nessuna estensione), `content::looks_like_text`/`sniff_file` (bytes con/senza
null, file assente), `content::ContentMatcher::find_first_match` (match, nessun match, multi-riga
non scatta, solo la prima riga, cap dimensione file, frase vuota, file assente), `walk_root`
content-aware (Text aperto e filtrato, Binary mai aperto, Unknown accodato non aperto in Fase A),
`SearchEngine::run` con content-search (Fase A, Fase B, nessun content-search invariato,
regressione deadlock, regressione pausa Fase B), `launch` (split `in:"..."` e attivazione
content-search). **676 verdi nel crate orchestrator** (660 lib + 2 main.rs + 13 ws_integration + 1
doc-test; 4 ignored invariati); whole-workspace `cargo test` verde (877 test totali sommando anche
protocol/mcp-server/plugin-*); `cargo clippy --all-targets`: 4 warning pre-esistenti invariati (0
nuovi).

## /find folder:from-here — restringere la ricerca alla sola cwd (0.40.9)

**Obiettivo:** vedi `Docs/superpowers/specs/2026-07-10-find-folder-scope-design.md` — `/find`
guadagna una terza direttiva indipendente e componibile con `in:"<frase>"` e la query sul nome:
`folder:from-here` restringe la ricerca al **solo** root Cwd. Quattro task sequenziali, ognuno un
commit separato: Task 1 (`aa86d51`, parsing), Task 2 (`c120f0d`, `resolve_roots`), Task 3
(`85a6123`, wiring in `launch`), Task 4 (`53080eb`, gate in `ws.rs`).

### Perché `resolve_roots` SALTA la costruzione dei candidati, invece di filtrarli dopo

L'alternativa più ovvia sarebbe stata: costruisci sempre i quattro bucket di candidati
(Cwd/Standard/Cloud/External) come oggi, poi — se `only_cwd` — scarta tutto tranne Cwd con un
`.filter()`. Non è quello che fa il codice. `resolve_roots` (`roots.rs`, Step 1) salta del tutto i
tre blocchi `for p in cfg.expanded_standard()/expanded_cloud()/expanded_external(...)` quando
`only_cwd` è `true` — non li itera nemmeno. Due motivi, non uno solo estetico:

- **Costo:** `expanded_external(provider)` in particolare può enumerare le unità disco reali (`PathProvider::external_drives`, I/O). Costruire candidati che verranno comunque scartati è lavoro
  sprecato ad ogni singolo `/find folder:from-here` — non enorme, ma gratuito da evitare.
- **Correttezza per costruzione, non per verifica a valle:** un filtro-dopo richiederebbe che ogni
  futuro cambiamento a valle (dedup, fan-out, prune) ricordi di preservare l'esclusione — un bug
  di omissione è possibile. Non costruire il candidato rende l'esclusione strutturalmente
  impossibile da dimenticare: non c'è nulla da filtrare perché non è mai esistito. Stesso principio
  già applicato al filtro `content` in `walk.rs` (un file `Binary` non viene mai aperto, non
  "aperto e poi il risultato scartato").

Il resto della funzione (canonicalizzazione, dedup, fan-out di un livello, calcolo `prune`) resta
IDENTICO e si applica normalmente al solo root Cwd — `only_cwd: true` non è un percorso di codice
parallelo, è lo stesso percorso con un candidate-set più piccolo in ingresso. Test
`only_cwd_true_skips_standard_cloud_external` popola `cfg.standard`/`cloud`/`external` con
directory reali ed esistenti (che altrimenti sopravvivrebbero tutte) e verifica che il risultato
sia UN solo root, `source: Cwd` — non basta verificare "meno root", serve verificare che quelli
esclusi non compaiano affatto. `only_cwd_false_behaves_exactly_like_before` è la rete di sicurezza
anti-regressione sul percorso esistente (i call-site esistenti sono stati aggiornati con `, false`
esplicito al 4° argomento, non un default silenzioso che avrebbe mascherato la firma cambiata).

### Perché il gate di validazione vive in `ws.rs` e non in `launch`

`parse_find_input` può fallire (`folder:<valore-ignoto>` ⇒ `Err`). C'erano due punti possibili
dove intercettare quell'`Err` e trasformarlo in un messaggio per l'utente: `ws.rs` (dove arriva il
comando dal client) o `search::mod::launch` (dove la ricerca viene effettivamente lanciata). La
scelta è `ws.rs`, per lo stesso motivo per cui il controllo `query.is_empty()` preesistente vive
già lì (`ws.rs:447`, poche righe sopra il nuovo check): è il punto che decide SE aprire la finestra
di ricerca. Se il gate vivesse solo in `launch`, la sequenza sarebbe "apri la finestra di ricerca,
POI scopri che l'input era invalido, POI mostra un errore dentro una finestra che non doveva mai
esistere" — un `SearchOpen` seguito a ruota da un errore è un'esperienza peggiore di "nessuna
finestra, un errore diretto nel pannello". Il test
`find_folder_invalid_value_errors_before_opening_search_window`
(`ws_integration.rs`) verifica esattamente questo: `Error{RoutingError}` +
`Done{exit_code: Some(1)}`, e implicitamente NESSUN `SearchOpen` (il test non lo aspetta, quindi se
arrivasse prima dell'Error il test fallirebbe per messaggio sbagliato).

`launch` chiama la STESSA funzione `parse_find_input` internamente — non per validare di nuovo (è
ridondante: `ws.rs` ha già validato lo stesso identico input pochi millisecondi prima), ma perché
`launch` ha comunque bisogno del `ParsedFind` risultante (`from_here`/`content`/`name_query`) per
fare il proprio lavoro. Il match sul risultato è deliberatamente asimmetrico rispetto a `ws.rs`:
`Err(_) => return` — esce silenziosamente, senza emettere `ServerMsg::Error`. Non è pigrizia: è la
garanzia che ci sia **un solo punto** nel codice che decide il testo del messaggio d'errore mostrato
all'utente. Se `launch` duplicasse la segnalazione, un domani un cambiamento al messaggio in un
posto solo (es. `ws.rs`) lascerebbe l'altro percorso (mai raggiungibile in pratica, ma comunque
codice vivo) con un messaggio stantio — `launch` resta difensivo (non va in panic né produce un
comportamento indefinito se mai ricevesse un input che `ws.rs` non ha validato, es. da un futuro
chiamante diverso) ma non è la fonte di verità sul messaggio.

### La generalizzazione del fallback `"*"` — non più solo per `in:`

Il fallback esisteva già prima di questo task: `/find in:"TODO"` (nessuna query sul nome) non deve
restituire zero risultati — `parse_query("")` produce `Matcher::Tokens(vec![])`, che per
costruzione non matcha mai nulla (`empty_query_matches_nothing`, test preesistente in
`query.rs`), quindi senza intervento un `in:` da solo troverebbe sistematicamente zero file,
contraddicendo l'intento dell'utente ("qualunque nome, purché il contenuto combaci"). La condizione
che decide se applicare `"*"` (`query.rs`, dentro `parse_find_input`) è stata generalizzata da "solo
`content.is_some()`" a `content.is_some() || from_here`: **decisione dell'utente in brainstorming**,
non una deduzione tecnica automatica — `folder:from-here` da solo (`/find folder:from-here`, nessun
nome, nessun `in:`) deve significare "tutto ciò che c'è nella cwd", non zero risultati per lo stesso
motivo di `in:` da solo. Test `folder_from_here_alone_falls_back_to_match_all_glob` lo fissa. Un
`/find` completamente bare (nessuna direttiva, nessun nome) resta gestito a monte da `ws.rs`
(`query.is_empty()` sul testo grezzo, invariato — quel caso non raggiunge nemmeno
`parse_find_input`).

**Test:** 11 nuovi nel crate (7 in `query.rs`: posizione di `folder:from-here` rispetto a nome/
`in:`, da solo, valore ignoto ⇒ errore che nomina il valore rifiutato, case-sensitivity distinta
tra chiave e valore; 2 in `roots.rs`: `only_cwd` true/false; 1 in `mod.rs`: `launch` end-to-end
con root Standard/Cloud/External popolati di file che altrimenti matcherebbero; 1 in
`ws_integration.rs`: gate d'errore prima dell'apertura finestra). **692 test verdi nel crate
orchestrator** (675 lib + 2 main.rs + 14 ws_integration + 1 doc-test; 4 ignored invariati);
whole-workspace `cargo test` verde; `cargo clippy --all-targets`: 4 warning pre-esistenti
invariati (0 nuovi, nessuno in quest'area).

## `repair_invalid_json_escapes` — recupero JSON per escape orfani in `arguments` OpenRouter (0.40.11)

**Bug reale segnalato dall'utente:** con un modello OpenAI-compatibile via OpenRouter
(osservato con DeepSeek), un turno AI lungo che chiama `show_markdown` a volte falliva con
`[errore AI] decodifica: arguments di 'show_markdown' non è JSON valido: invalid escape at
line 1 column N` — l'INTERO turno veniva scartato (storia troncata al punto del turno
precedente, l'utente non riceveva nulla), pur essendo la risposta del modello altrimenti
completa e valida (il modello aveva già "fatto il lavoro", solo la serializzazione degli
`arguments` era imperfetta).

**Causa:** `OrFunctionCall::arguments` (`openrouter_backend.rs`) è una stringa JSON che il
MODELLO stesso serializza — a differenza di Claude (`claude_backend.rs`, NON toccato da
questa fix), dove il tool-use arriva già come oggetto `serde_json::Value` strutturato
dall'API, senza passare da testo libero generato dal modello. Un modello può scrivere dentro
quella stringa un escape Markdown letterale come `\*` (sintassi Markdown valida) senza
raddoppiare il backslash come richiederebbe JSON grezzo (`\\*`) — `serde_json` rifiuta `\*`
con "invalid escape" perché `*` non è uno dei 9 caratteri di escape validi della grammatica
JSON (RFC 8259 §7: `"` `\` `/` `b` `f` `n` `r` `t` `u`). Questo NON è un bug di streaming/
riassemblaggio SSE lato nostro: `OpenRouterBackend` è non-streaming (una singola risposta
HTTP, vedi doc-comment sopra `send_turn` in `openrouter_backend.rs`) — il JSON malformato è
generato così com'è dal modello.

**Design — macchina a stati minima invece di un `.replace()` globale:** `repair_invalid_json_escapes`
(pura, nessun I/O) scorre `arguments` `char` per `char` tenendo un flag `in_string` (si
inverte solo su una `"` non già "consumata" come parte di una coppia di escape valida — vedi
sotto perché questo evita la desincronizzazione), e quando trova un `\` DENTRO una stringa
seguito da un carattere NON tra i 9 escape validi, raddoppia il backslash orfano (`\*` →
`\\*`, un backslash letterale seguito da un asterisco — quasi certamente ciò che il modello
intendeva).

**Perché il tracking dentro/fuori-stringa è necessario (non un dettaglio implementativo):**
senza sapere se ci si trova dentro una stringa JSON, non si può distinguere in modo
affidabile una `"` che CHIUDE una stringa da una `"` che ne fa parte perché preceduta da un
escape valido (`\"`). Sbagliare questo desincronizza il resto della scansione. La funzione
evita il problema alla radice: quando incontra una coppia di escape valida (`\` + uno dei 9
caratteri), consuma ESPLICITAMENTE entrambi i caratteri nello stesso ciclo (`chars.next()`
sul carattere sbirciato con `peek()`), cosicché il giro successivo del `while` riparta dal
carattere DOPO la coppia — la `"` di un `\"` non viene mai rivista come una virgoletta "nuova".

**Integrazione in `from_or_response`:** 1) prova il parsing diretto (percorso veloce, nessuna
scansione di riparazione nel caso comune, che è la stragrande maggioranza delle chiamate); 2)
se fallisce, prova `repair_invalid_json_escapes` e ri-parsa il risultato; 3) riparazione
riuscita → `tracing::warn!` (visibilità in produzione: "riparato automaticamente" non deve
passare inosservato) e uso del valore riparato; 4) riparazione NON sufficiente (JSON rotto
oltre un backslash orfano — troncato, struttura sbagliata...) → `tracing::warn!` con gli
`arguments` GREZZI ORIGINALI (troncati a 2000 char per non inondare i log con input
patologici) — prima di questa fix quella stringa andava persa per sempre nell'istante in cui
l'errore scattava, rendendo impossibile diagnosticare una FUTURA malformazione diversa da
questa — poi `Err(BackendError::Decode(...))` costruito dall'errore di parsing ORIGINALE (non
da quello del tentativo di riparazione, che in questo ramo non è mai stato un fix reale):
stesso identico formato di messaggio di prima della fix.

**Scope deliberatamente limitato (non un'omissione):**
- Ripara SOLO la classe "invalid escape" (backslash + carattere non tra i 9 validi). NON
  tocca backslash seguiti da un carattere di escape VALIDO — anche quando quell'escape è
  quasi certamente un errore del modello in un contesto diverso (es. `C:\temp` scritto senza
  raddoppio del backslash produce `\t`, che JSON interpreta come carattere TAB, non come
  backslash-letterale-seguito-da-t). Quel caso "passa" il parsing silenziosamente producendo
  un valore SBAGLIATO invece di un errore di decodifica — una categoria di bug
  qualitativamente diversa (corruzione silenziosa, non fallimento visibile), fuori scope qui.
- OpenRouter-only: nessuna modifica a `claude_backend.rs` — il tool-use Anthropic non passa
  mai da una stringa JSON generata dal modello, quindi questa classe di bug non può esistere
  lì.
- Non inventa mai caratteri mancanti: un documento JSON genuinamente troncato (es. stringa
  senza virgoletta di chiusura) resta non parsabile dopo la riparazione — il chiamante vede
  lo stesso errore di prima, non un "successo" fasullo su dati incompleti.

**Test:** 5 unit test su `repair_invalid_json_escapes` (`repair_leaves_already_valid_json_untouched`;
`repair_leaves_all_valid_escapes_untouched` — i 9 escape validi TUTTI presenti in un'unica
stringa, incluso `\uXXXX` e un carattere UTF-8 non-ASCII grezzo, nessuno alterato — la guardia
anti-over-fix più importante; `repair_doubles_stray_backslash_before_invalid_escape_char` —
asserisce sia la stringa di output esatta SIA che il risultato riparato parsi davvero, con
commento esplicito sulla differenza fra escaping Rust ed escaping JSON nello stesso letterale;
`repair_does_not_desync_on_escaped_quote` — un `\"` valido seguito da un `\*` da riparare
nello stesso documento, per dimostrare che il tracking resta corretto dopo l'escape valido;
`repair_does_not_invent_a_closing_quote` — documento troncato, deve restare non parsabile) +
1 test di integrazione su `from_or_response`
(`from_or_response_recovers_from_stray_markdown_escape_in_arguments`, con un fixture `\*, \*`
plausibile — confermato RED contro il codice pre-fix con lo stesso tipo di errore
`Decode("...invalid escape...")` riportato dall'utente, GREEN dopo la fix, verificando che il
contenuto generato dal modello sopravviva integro). Il test di regressione preesistente
`from_or_response_malformed_arguments_is_a_decode_error` (JSON genuinamente rotto — non un
problema di escape) resta invariato e verde, a dimostrazione che JSON davvero malformato
continua a produrre un `Decode` esplicito anche dopo l'introduzione del percorso di recupero.

**Esito verifiche:** `cargo test -p orchestrator` — 683 lib test verdi (6 nuovi aggiunti da
questa fix: 5 unit test su `repair_invalid_json_escapes` + 1 di integrazione su
`from_or_response`), 0 falliti, 4 ignored invariati; `ws_integration.rs` 14/14 verdi; nessuna
regressione altrove nel crate. `cargo clippy --all-targets` — stessi 4 warning pre-esistenti
di prima (nessuno in `openrouter_backend.rs`), 0 nuovi introdotti da questa fix.

## `external_channel.rs` — registro canali tool esterni (0.40.16)

`ExternalToolChannel { id, slash_trigger, window_title, tool_client, format_invocation }`,
registro statico `EXTERNAL_TOOL_CHANNELS` (vuoto in questa release) e
`resolve_channel_tools(channel, registry, default_tools) -> Result<(Arc<dyn ToolClient>, Option<fn(...)->String>), String>`.
Consumato da `ws.rs` nell'handshake (vedi entry successiva). Design:
`Docs/superpowers/specs/2026-07-16-external-tool-channel-design.md`.

## `ws.rs` handshake channel resolution + `TurnOptions.format_invocation` (0.40.18)

`handle_connection`'s handshake calls `external_channel::resolve_channel_tools`
right after the token check; the result shadows the `tools` parameter for the
rest of the connection (no signature change to `handle_connection`/`serve`).
`format_invocation` rides inside `agent::TurnOptions` (not a new parameter on
`AiAdapter::respond`) from `core::handle_command` down to `LlmAdapter::respond`'s
two `display_invocation` call sites. `EXTERNAL_TOOL_CHANNELS` empty ⇒ every
production connection today takes the `channel: None` branch, byte-identical
to pre-0.40.16 behaviour.

Two deviations from the original task briefs, both compiler-forced (found only
once the exact code landed and the crate was rebuilt):

- `telegram/channel.rs`'s own direct call to `handle_command` (not covered by
  the brief's "the production call site is `ws.rs`" note — Telegram is a
  separate `Channel` with no handshake/channel-resolution concept) needed the
  new 8th positional argument too. Passed `None`: Telegram keeps using
  `agent::display_invocation` unconditionally, exactly as before.
- `TurnOptions`'s `#[derive(... PartialEq)]` combined with the new `fn`-pointer
  field trips rustc's `unpredictable_function_pointer_comparisons` lint (fn
  pointer equality isn't guaranteed stable across codegen units). `TurnOptions`
  is never compared with `==`/`assert_eq!` anywhere in this crate (grepped to
  confirm), so the lint is a false positive here; suppressed locally with
  `#[allow(unpredictable_function_pointer_comparisons)]` on the struct rather
  than dropping `PartialEq` (smaller diff against the brief's literal code).

## `router::external_channel_command` (0.40.19)

Pura, mirror di `plugin_command`. Non invocata da `ws.rs` in questa release —
vedi la nota di scope in `Docs/superpowers/plans/2026-07-16-external-tool-channel.md`
Task 5 per il perché.

## `ToolClient::tool_defs()`/`dispatch()` (0.40.20, fix di review incluso)

Due nuovi metodi sul trait `ToolClient`
(`crates/orchestrator/src/tool_client.rs`):

- `tool_defs(&self) -> Vec<ToolDef>` — metodo **con default**
  (`agent::tool_defs()`, i 3 tool storici).
- `async fn dispatch(&self, name, input) -> (String, bool)` — metodo
  **required** (NON default): un default trait method non può castare `&Self`
  a `&dyn ToolClient` per chiamare la funzione libera `agent::dispatch_tool`
  (vincolo reale di object-safety di Rust). Ognuno dei tre `ToolClient`
  esistenti (`FakeToolClient`, `McpToolClient`, `CwdTrackingToolClient`)
  fornisce quindi il proprio corpo per `dispatch`.

Il corpo corretto, che ognuno dei tre impl fornisce, è:
```rust
async fn dispatch(&self, name: &str, input: &serde_json::Value) -> (String, bool) {
    crate::agent::dispatch_tool(self as &dyn ToolClient, name, input).await
}
```
Il punto cruciale è **`self`, non un eventuale `inner`**: `dispatch` re-entra
`agent::dispatch_tool` con l'impl CONCRETO come receiver, così una chiamata
`run_in_session` dentro `dispatch_tool` ricade sull'override di
`run_in_session` di QUEL tipo concreto, non su una versione "nuda"/interna.
Per `FakeToolClient` e `McpToolClient` questo non fa differenza osservabile
(non sono decorator, non hanno un `inner` da bypassare). Per
`CwdTrackingToolClient` — un decorator che wrappa un `inner: Arc<dyn
ToolClient>` e aggiorna `cwd_state` nel proprio override di
`run_in_session` — la distinzione è invece l'intero punto: dispacciare su
`self` è l'UNICO modo che fa sì che `dispatch("run_in_session", ...)`
attraversi l'override che aggiorna `cwd_state`, invece di saltarlo.

**Bug corretto in questo fix di review:** la prima versione di
`CwdTrackingToolClient::dispatch` (in `crates/orchestrator/src/cwd_tracking.rs`)
NON seguiva questo pattern — delegava a `self.inner.dispatch(name, input)`.
Per `name == "run_in_session"` questo re-entrava `agent::dispatch_tool` con
`self.inner` (il client wrappato) come receiver, chiamando `run_in_session`
**direttamente sul client interno** e bypassando totalmente l'override di
`CwdTrackingToolClient::run_in_session` — quello che scrive in `cwd_state`.
Confermato indipendentemente da due review come finding critico. Non era un
bug live al momento del commit (nessun sito di produzione chiama ancora
`ToolClient::dispatch`; `ai_adapter.rs` chiama tuttora la funzione libera
`agent::dispatch_tool(tools, ...)` direttamente), ma lo sarebbe stato
silenziosamente non appena un task futuro avesse collegato il loop tool-use
dell'AI a `tools.dispatch(...)`: i comandi shell guidati dall'AI avrebbero
smesso di aggiornare il cwd tracciato (letto da `/find` e dalla logica di
emissione `Cwd` in `ws.rs`), mentre i comandi diretti (route `Os`, che
chiamano `.run_in_session()` senza passare da `dispatch`) avrebbero
continuato a funzionare — una regressione subdola, plausibilmente notata
solo molto più tardi. Corretto con il pattern sopra; test di regressione
`dispatch_run_in_session_updates_cwd_state` in
`crates/orchestrator/src/cwd_tracking.rs` (RED confermato contro il codice
buggy — `cwd_state` restava al valore iniziale invece di aggiornarsi — poi
GREEN dopo il fix).

`FixtureChannelToolClient` (nuovo, non `#[cfg(test)]` — stesso trattamento di
`FakeToolClient`, pubblico per essere usato da test in altri moduli del
crate) sovrascrive sia `tool_defs` che `dispatch` PIÙ le 3 chiamate storiche,
dimostrando il pattern completo che un `ToolClient` di canale reale
(mcp-nmap) dovrà seguire. Design:
`Docs/superpowers/specs/2026-07-16-tool-isolation-design.md`.

## `TurnOptions.system_prompt_override` + `respond()` su `tools.tool_defs()`/`dispatch()` (0.40.21)

Stesso filo di `format_invocation` (0.40.18), stesso punto d'uso in
`LlmAdapter::respond` (`crates/orchestrator/src/ai_adapter.rs`): la
sostituzione del system prompt avviene interamente dentro
`agent::system_prompt(opts)`, nessuna modifica al call site
(`let system = agent::system_prompt(opts);` resta identico). `tools_for`
cambia firma (`tools_for(opts, defs)` invece di `tools_for(opts)`) — l'unico
chiamante nel crate è `respond()`, che ora passa `tools.tool_defs()`. Il
dispatch di un `tool_use` passa da `agent::dispatch_tool(tools, name, input)`
a `tools.dispatch(name, input)` — stesso identico comportamento per
`FakeToolClient`/`McpToolClient` (ereditano il default del trait, che a sua
volta chiama `agent::dispatch_tool`), diverso per un `ToolClient` di canale
che lo sovrascrive. `resolve_channel_tools`
(`crates/orchestrator/src/external_channel.rs`) ritorna una 3-upla
`(Arc<dyn ToolClient>, Option<fn(...)->String>, Option<&'static str>)` —
`ws.rs` la destruttura e clona il terzo elemento nello stesso punto in cui
già clonava `format_invocation` per il task spawnato. Design:
`Docs/superpowers/specs/2026-07-16-tool-isolation-design.md`.

## Prova end-to-end isolamento per-canale (0.40.22)

Test-only. `core.rs`: 2 nuovi test (`channel_tool_client_blocks_os_route_shell_execution`,
`channel_tool_client_blocks_open_target_slash`) chiamano `handle_command` con
`FixtureChannelToolClient` come `tools` su `Route::Os`/`Route::Slash` — provano
che quelle vie (invariate, non channel-aware) restano bloccate perché il
`ToolClient` del canale non delega a una shell/target reale.
`ai_adapter.rs`: 2 nuovi test su `LlmAdapter::respond` — il primo ispeziona
`FakeChatBackend::recorded().tools`/`.system` per confermare che il backend
riceve SOLO i 2 tool del canale col system prompt del canale; il secondo
ispeziona `FakeChatBackend::nth_request(1).history` per un `Block::ToolResult`
con `is_error: true` quando il modello (scriptato per farlo deliberatamente)
invoca un nome fuori menu. Nessuna modifica a codice non di test.

## `DispatchOutcome`/`ChannelReport` (0.40.23)

`tool_client.rs` guadagna `DispatchOutcome{output, is_error, report}` e
`ChannelReport{title, markdown}`. `ToolClient::dispatch`'s ritorna
`DispatchOutcome` (era `(String, bool)`); `agent::dispatch_tool` costruisce
sempre `report: None` (nessun canale generico produce documenti — solo un
`ToolClient` specifico, tramite il proprio `dispatch()` override, lo fa).
`LlmAdapter::respond` (`ai_adapter.rs`), dopo aver dispacciato un tool
non-`show_markdown`, controlla `outcome.report`: se `Some` (e
`opts.allow_windows`), emette `ServerMsg::OpenWindow` per QUEL report,
prima di costruire il `Block::ToolResult` che torna al modello (che non
vede mai `report`, solo `output` troncato). **0.40.26:** non emette più
anche `ServerMsg::SaveToLibrary` — quella variante è stata rimossa da
`protocol` 0.14.6 dopo che il primo uso dal vivo ha prodotto un doppione
in Library (il pulsante "Salva" preesistente nella finestra Markdown
salvava di nuovo lo stesso contenuto). Il salvataggio resta scelta
dell'utente tramite quel pulsante.

## `NmapToolClient` (0.40.24)

Nuovo file `crates/orchestrator/src/nmap_tool_client.rs` (dichiarato in
`lib.rs` con `pub mod nmap_tool_client;`, alfabeticamente tra
`messages_client` e `router` — la posizione realmente alfabetica per la `n`
di "nmap", non subito dopo `local_confirm` come una prima lettura del piano
suggeriva letteralmente; vedi nota di deviazione più sotto).

Struttura identica a `McpToolClient` (`tool_client.rs`): `mcp_nmap_path:
PathBuf` + `peer: Arc<Mutex<Option<rmcp::Peer<rmcp::RoleClient>>>>`,
`resolve()` con lo stesso ordine di risoluzione (`LARE_MCP_NMAP` env var,
poi sibling dell'eseguibile corrente — `mcp-nmap.exe`/`mcp-nmap` a seconda
della piattaforma). `ensure_connected()` fa lo spawn lazy + handshake MCP la
prima volta (via `TokioChildProcess` + `().serve(transport)`, handler `()`
perché nmap non manda `notifications/progress` — nessun
`LareClientHandler`/`ProgressDispatcher` qui, a differenza di
`McpToolClient::run_in_session`), poi riusa il peer clonato sulle chiamate
successive.

`call_scan_tool(tool_name, target)` è il punto unico chiamato da entrambi i
tool esposti: costruisce `{"target": ...}` come argomenti, chiama
`peer.call_tool(...)` sotto un `tokio::time::timeout(NMAP_CALL_TIMEOUT_SECS)`
— **900s**, non i 90s di `McpToolClient`: deve eccedere sia il timeout
interno di `mcp-nmap::elevate::SCAN_TIMEOUT` (600s, lo scan elevato con UAC)
sia i 180s del gate di conferma locale, altrimenti questo timeout
scadrebbe per primo e abbandonerebbe uno scan ancora legittimamente in corso
nel sidecar. Sul timeout: il peer viene azzerato (riconnessione forzata alla
chiamata successiva) e ritorna un `DispatchOutcome{is_error: true, report:
None}` con un messaggio leggibile. Sulla risposta valida, il testo JSON del
content viene deserializzato in `NmapScanOutcomeJson{summary, report_markdown,
is_error}` (mirror del JSON contract di `mcp-nmap::scan::ScanOutcome`, nessuna
dipendenza Cargo tra i due crate — stesso schema di `McpOutput`/
`McpOpenResult` privati in `McpToolClient`): `is_error: true` → `report: None`
(un errore non produce documento, es. nmap assente dal PATH o XML malformato
— design spec §7); altrimenti `report: Some(ChannelReport{title: "Scansione
nmap — {target}", markdown: out.report_markdown})`.

`tool_defs()` sovrascrive il default del trait per esporre SOLO
`nmap_quick_scan` (TCP connect `-sT`, nessun privilegio) e `nmap_os_detect`
(`-O`, richiede UAC) — mai i 3 tool storici. `dispatch()` instrada per nome a
`call_scan_tool`; un nome fuori menu è rifiutato come "tool sconosciuto sul
canale nmap" (stessa via di un nome davvero inventato dal modello — nessuna
differenza di trattamento). Le 3 chiamate storiche (`run_in_session`,
`open_target`, `reset_session`) sono stub che mirror-ano esattamente
`FixtureChannelToolClient` (`tool_client.rs`): `run_in_session` e
`open_target` ritornano "non disponibile su questo canale" (`exit_code: -1`
per il primo, `ok: false` per il secondo), `reset_session` è un no-op. Questo
è ciò che chiude strutturalmente `Route::Os`/`Route::Slash` di `core.rs` per
questo canale — `core.rs` non sa nulla di canali, l'isolamento vive
interamente nel `ToolClient`.

**Deviazioni dal piano (Task 3 brief), corrette in scrittura invece che
seguite alla lettera — entrambe bug di compilazione reali, non scelte di
stile:**
1. Il test `nmap_scan_outcome_json_parses_success_shape` nel brief usa
   `r#"...","report_markdown":"# Report",...}"#` — una raw string a un solo
   hash il cui contenuto include letteralmente la sequenza `"#` (dentro `"#
   Report"`), che chiude prematuramente la raw string stessa (errore di
   parsing, non semantico). Corretto usando un delimitatore a due hash
   (`r##"..."##`) per quel solo test; l'altro test di parsing errore
   (`nmap_scan_outcome_json_parses_error_shape`) non contiene `"#` nel JSON e
   resta `r#"..."#` invariato.
2. `call_scan_tool` passava `CallToolRequestParams::new(tool_name)` con
   `tool_name: &str` a lifetime di funzione — ma `CallToolRequestParams::new`
   richiede `impl Into<Cow<'static, str>>` (verificato nel sorgente rmcp
   1.7.0, `model.rs`): un riferimento non-`'static` non soddisfa il bound
   (`E0521: borrowed data escapes outside of method`). `McpToolClient` non
   incontra questo problema perché passa sempre una `&'static str` letterale
   (`"run_in_session"`, `"open_target"`) — qui il nome del tool è invece un
   parametro runtime (`"nmap_quick_scan"` o `"nmap_os_detect"` scelti da
   `dispatch()`). Corretto con `tool_name.to_string()`: una `String` di
   proprietà soddisfa `Into<Cow<'static, str>>` via `Cow::Owned`, perché non
   prende in prestito nulla dal chiamante.
3. La posizione del `pub mod nmap_tool_client;` in `lib.rs`: il brief chiede
   "subito dopo `local_confirm`" ma anche "rispettando l'ordine alfabetico
   esistente" — le due istruzioni sono in conflitto (alfabeticamente "n" di
   `nmap_tool_client` viene dopo "m" di `messages_client`, non subito dopo
   "l" di `local_confirm`). Ho seguito l'ordine alfabetico reale (tra
   `messages_client` e `router`), che è il vincolo più forte e verificabile
   meccanicamente.

Non ancora collegato a `EXTERNAL_TOOL_CHANNELS` (`external_channel.rs`), che
resta un registro vuoto — la registrazione è Task 4.

## Task 4 — registrazione del canale `/nmap` (`external_channel.rs` + `local_confirm.rs`)

`EXTERNAL_TOOL_CHANNELS` (`external_channel.rs`) ha ora la sua prima voce
reale, non più un array vuoto:

```rust
pub const EXTERNAL_TOOL_CHANNELS: &[ExternalToolChannel] = &[ExternalToolChannel {
    id: "nmap",
    slash_trigger: "/nmap",
    window_title: "Lare — nmap",
    tool_client: || NmapToolClient::resolve().map(|c| Arc::new(c) as Arc<dyn ToolClient>),
    format_invocation: Some(format_nmap_invocation),
    system_prompt_override: Some(NMAP_SYSTEM_PROMPT),
}];
```

- `tool_client` è la stessa factory sincrona-fallibile già usata per gli
  altri canali (mirror di `McpToolClient::resolve`): costruisce solo il
  path del sidecar, lo spawn vero e proprio resta lazy al primo dispatch —
  per questo il test `nmap_channel_tool_client_factory_constructs_without_panicking`
  può chiamarla in un test unitario senza spawnare nulla.
- `format_nmap_invocation` (nuova funzione privata del modulo) duplica
  `mcp_nmap::markdown::format_nmap_invocation` — stessa logica a 10 righe,
  nessuna dipendenza Cargo tra `orchestrator` e `mcp-nmap` per una sola
  funzione di formattazione (YAGNI: si condivide solo quando un terzo
  canale ne avrà bisogno). Per `nmap_os_detect` include ESPLICITAMENTE
  "richiede privilegi elevati" nel testo: è l'unico punto in cui l'utente
  vede il target prima dell'elevazione UAC, che non lo mostra.
- `NMAP_SYSTEM_PROMPT` dice all'AI del canale che ha ESATTAMENTE
  `nmap_quick_scan`/`nmap_os_detect`, niente shell/file/URL/Markdown — evita
  che il modello provi comunque i 3 tool storici che questo canale non offre.

In `local_confirm.rs`, `SENSITIVE_TOOLS` passa da `&[]` a
`&["nmap_quick_scan", "nmap_os_detect"]`: sono i primi due tool reali a
richiedere conferma esplicita dalla UI locale via `LocalUiConfirmer::should_gate`
(prima di questo task il gate esisteva già come seam ma non gateizzava mai
nulla per davvero).

### 3 test preesistenti riscritti (non 2 — un terzo era nascosto in `router.rs`)

Il piano (Task 4 brief) elencava solo 2 test da riscrivere
(`external_channel.rs`, `local_confirm.rs`). In fase di verifica
(`cargo test -p orchestrator`) è emerso un terzo test con la stessa identica
famiglia di problema, non menzionato nel piano: `router::tests::
external_channel_command_empty_registry_always_none` asserisce
`external_channel_command("/nmap", EXTERNAL_TOOL_CHANNELS) == None` — scritto
quando il registro era vuoto e "/nmap" era l'esempio di trigger sconosciuto;
ora "/nmap" è registrato davvero, quindi l'asserzione è diventata falsa
(rossa). Rinominato in
`external_channel_command_unregistered_trigger_against_real_registry_is_none`,
stesso principio delle altre due riscritture: un trigger genuinamente non
registrato (`/some-future-trigger-not-yet-built`) al posto di "/nmap".

I 3 test riscritti:
1. `external_channel.rs`: `empty_production_registry_never_matches_anything`
   → `unregistered_channel_id_still_errs_against_the_real_registry`.
2. `local_confirm.rs`: `should_gate_is_false_for_any_tool_while_sensitive_tools_is_empty`
   → `should_gate_is_true_for_sensitive_tools_false_for_others` (ora asserisce
   sia il caso gateizzato — `nmap_quick_scan`/`nmap_os_detect` → `true` — sia
   quello non gateizzato — `run_in_session`/`open_target`/altro → `false`).
3. `router.rs`: `external_channel_command_empty_registry_always_none` →
   `external_channel_command_unregistered_trigger_against_real_registry_is_none`.

Anche 3 commenti non-test (non test, quindi non "rossi", ma fattualmente
falsi dopo la registrazione) sono stati corretti: due in `ws.rs` (attorno
allo spawn del task per comando, dicevano "SENSITIVE_TOOLS ancora vuoto,
nessun tool viene gateizzato davvero") e uno in `ai_adapter.rs` (doc-comment
del test `selective_confirmer_leaves_ungated_tools_autonomous`, diceva
"simula LocalUiConfirmer con SENSITIVE_TOOLS vuoto"). Nessun cambio di
logica in questi 3 punti, solo testo.

### Deviazione dal piano (Task 4 brief), corretta in scrittura

Il test `nmap_channel_tool_client_factory_constructs_without_panicking` nel
brief usa `assert!(result.is_ok(), "...: {result:?}")` — non compila:
`Result<Arc<dyn ToolClient>, anyhow::Error>` non implementa `Debug` perché
`Arc<dyn ToolClient>` (variante `Ok`) non lo implementa. Stesso vincolo già
aggirato nel file per `unknown_channel_is_err_not_panic` (commento esistente:
"non `.unwrap_err()` — richiederebbe `Debug` sul tipo `Ok`... `match`
esplicito verifica lo stesso invariante senza il bound"). Corretto con lo
stesso pattern: `match` esplicito, `panic!` col messaggio solo nel branch
`Err` (dove `e: anyhow::Error` implementa `Display`, quindi `{e}` non
`{e:?}`).

### Verifica

`cargo build` (workspace, default-members) pulito. `cargo test -p
orchestrator`: 729 lib test passati (0 falliti, 4 ignorati — invariati,
richiedono binari esterni), più le suite di integrazione (`ws_integration`,
16/16) e doc-test (1/1) invariate. `cargo test` (intero workspace, inclusi
`protocol` e `mcp-server`): tutto verde. `cargo clippy --all-targets`: 4
warning pre-esistenti, tutti in righe non toccate da questo task
(`ai_adapter.rs:331`, `ws.rs:580`, `aichat/service.rs:5604`/`7958`) — zero
warning nuovi introdotti dalle modifiche di questo task.

## `NmapToolClient`: `local_network_info` + `traceroute` (0.40.27)

`tool_defs()` passa da 2 a 4 `ToolDef`: `local_network_info` (nessun
parametro — `properties: {}`, `required: []`, in linea con come `ToolDef`
già rappresenta tool a zero argomenti altrove nel crate) e `traceroute`
(un solo parametro `target`, stesso schema di `nmap_quick_scan`/
`nmap_os_detect`). Entrambi i nuovi tool sono descritti in modo da guidare
il modello: `local_network_info` dichiara esplicitamente "usa PRIMA di uno
scan quando l'utente non specifica un target" — è il meccanismo con cui
l'AI del canale può determinare da sola un CIDR di partenza invece di
chiedere all'utente.

`dispatch()` instrada i 4 nomi: i 2 storici restano su `call_scan_tool`
invariato, i 2 nuovi vanno su un metodo separato, `call_info_tool(tool_name,
target: Option<&str>)` — `target: None` per `local_network_info` (nessun
argomento da mandare), `Some(target)` per `traceroute`. `call_info_tool`
NON è un semplice alias di `call_scan_tool`: condivide la stessa macchina
di lazy-connect (`ensure_connected`) e lo stesso `tokio::time::timeout(
NMAP_CALL_TIMEOUT_SECS)` con lo stesso comportamento sul timeout (peer
azzerato, riconnessione forzata alla chiamata successiva), ma deserializza
un JSON **diverso**: `NmapInfoOutcomeJson{output, is_error}` invece di
`NmapScanOutcomeJson{summary, report_markdown, is_error}`. Questo mirror-a
esattamente `mcp_nmap::network_info::NetworkInfoOutcome` (Task 1, crate
`mcp-nmap` 0.7.0/0.7.1) — niente `summary`/`report_markdown` perché questi 2
tool non producono un documento: `DispatchOutcome.report` resta **sempre
`None`** per `local_network_info`/`traceroute`, a differenza degli scan
tool dove `report: Some(...)` quando non è un errore. La ragione è nel
module doc di `network_info.rs`: output di rete grezzo (ipconfig/arp/route,
poche righe) è utile all'AI per ragionare sul prossimo passo, non un
report da salvare in Library o aprire in una finestra Markdown — quel
trattamento resta riservato agli scan nmap veri e propri.

**`SENSITIVE_TOOLS` (`local_confirm.rs`) non è stato toccato.** I 2 tool
storici (`nmap_quick_scan`/`nmap_os_detect`) restano gli unici gateizzati
dal gate di conferma locale. `local_network_info`/`traceroute` sono
entrambi read-only e locali: il primo legge solo lo stato di rete della
macchina che esegue l'orchestrator (nessun pacchetto inviato altrove), il
secondo traccia un percorso di rete senza alcun side-effect (a differenza
di uno scan, non prova connessioni multiple su una gamma di IP/porte). Né
l'uno né l'altro richiedono privilegi elevati o toccano un target remoto
in modo da giustificare un prompt di conferma — coerente col principio già
in `local_confirm.rs`: il gate esiste per proteggere l'utente da azioni con
effetto reale su una macchina target, non per ogni chiamata a un tool.

`format_nmap_invocation` (`external_channel.rs`) guadagna 2 rami:
`local_network_info` → etichetta statica `"informazioni di rete locali"`
(nessun target da mostrare, a differenza degli altri 3 tool);
`traceroute` → `"traceroute → {target}"`, stesso stile di
`nmap_quick_scan`. Poiché nessuno dei due tool è in `SENSITIVE_TOOLS`,
questa etichetta non produce mai un banner di conferma per loro — resta
comunque usata per la riga di trasparenza nel cursore (`agent::
display_invocation` fallback altrimenti). `NMAP_SYSTEM_PROMPT` è stato
riscritto per elencare tutti e 4 i tool, con la stessa istruzione
d'ordine già nella descrizione di `local_network_info`: usarlo prima di
uno scan quando l'utente non specifica un target.

### Verifica

`cargo build` (workspace) pulito. `cargo test -p orchestrator --lib
nmap_tool_client`: 7/7 passati (5 preesistenti — 1 rinominato da
`tool_defs_exposes_only_the_two_nmap_tools` a
`tool_defs_exposes_exactly_the_four_nmap_channel_tools` — + 2 nuovi
`nmap_info_outcome_json_parses_*`, 0 falliti). `cargo test` (intero
workspace): tutto verde, nessuna regressione nei crate `protocol`,
`mcp-server`, `mcp-nmap`. `cargo clippy --all-targets`: stessi 4 warning
pre-esistenti di 0.40.25 (`ai_adapter.rs:331`, `ws.rs:580`,
`aichat/service.rs:5604`/`7958`), nessuno nei file toccati da questo task.

## `connection_owns_plugin_sink` — fix del sink condiviso di `PluginHost` (0.40.28)

Bug reale trovato dal vivo dall'utente dopo il rilascio del canale `/nmap`:
i plugin (`/lc`/`/calc`/`/counter`) fallivano ad aprirsi in modo
intermittente, a volte con `Done{exit_code:0}` e nessuna finestra.

**Diagnosi.** `PluginHost` (`plugins/host.rs`) è un `Arc<Mutex<PluginHost>>`
costruito UNA volta all'avvio dell'orchestrator e condiviso da OGNI
connessione WS — cursore principale incluso, e ora anche le finestre di
canale come `/nmap`. `handle_connection` (`ws.rs`), nella sua sequenza di
setup per QUALSIASI connessione, chiamava incondizionatamente
`plugin_host.lock().await.set_server_tx(out_tx.clone())`. Prima del canale
esterno, un orchestrator serviva sempre e solo UNA connessione WS attiva (il
cursore) — questa chiamata era innocua per costruzione, mai esercitata due
volte. Il canale `/nmap` introduce la prima SECONDA connessione mai
esistita nel processo: aprire quella finestra chiama di nuovo
`set_server_tx`, sovrascrivendo il sink condiviso con l'`out_tx` della
connessione `/nmap`.

`PluginHost::spawn_and_handshake` cattura `self.server_tx.clone()` UNA sola
volta, al momento dello spawn del plugin (lazy, al primo `/comando`) — il
pump task che ne deriva instrada per sempre i `ServerMsg` verso QUALUNQUE
sink fosse `server_tx` in quel preciso istante. Un plugin già attivo PRIMA
dell'apertura di `/nmap` mantiene il pump (e il sink) corretto e continua a
funzionare; un plugin attivato per la PRIMA volta DOPO cattura il sink
sbagliato — il suo `ShowWindow` (tradotto in `ServerMsg::OpenPluginWindow`
dal pump) arriva sulla connessione `/nmap`, che non ha alcun `case` per
quel tipo di messaggio in `external-channel-window.js` (finisce nel
`default: console.warn(...)`, scartato silenziosamente). Da qui l'esito
osservato: `activate()` ritorna comunque `Some(window_id)` (successo,
`exit_code: 0`), ma nessuna finestra appare mai. Se la finestra `/nmap`
viene poi chiusa, quella connessione muore e il sink diventa un canale
morto: stesso esito silenzioso, plugin ancora "rotti" anche a `/nmap`
chiuso — coerente con quanto riportato dall'utente in entrambi i casi.

**Fix.** Nuova funzione pura in `ws.rs`:
```rust
fn connection_owns_plugin_sink(channel: Option<&str>) -> bool {
    channel.is_none()
}
```
`handle_connection` ora chiama `set_server_tx` solo dentro
`if connection_owns_plugin_sink(requested_channel.as_deref())`. Una
connessione di canale non usa mai plugin (il suo `ToolClient` è isolato per
canale — vedi `Docs/superpowers/specs/2026-07-16-tool-isolation-design.md`)
quindi non ha mai un motivo legittimo di reclamare quel sink.

**Copertura test — limite dichiarato.** 2 test unitari su
`connection_owns_plugin_sink` (RED genuino: pre-fix la funzione non esiste,
`E0425` è la fase RED). Un test end-to-end a livello `ws_integration.rs`
(due connessioni reali + attivazione di un plugin lazy) è stato tentato ma
scartato: il dispatch dei plugin lazy in `handle_connection` costruisce la
sua factory di spawn INLINE, hardcoded su `plugins::transport::spawn_plugin`
(processo reale) — non c'è un punto di iniezione per un transport fake in
quel percorso specifico (a differenza di `PluginHost::start`, che accetta
una factory iniettabile, ma che viene usata solo per i plugin *eager*,
spawnati prima che qualunque connessione — e quindi `server_tx` — esista).
Un test che tentasse questa strada fallirebbe per una ragione estranea al
bug (spawn di un binario inesistente), non per la regressione reale — un
falso RED che rimarrebbe rosso anche col fix, quindi fuorviante più che
utile. La wiring del call site è verificata per ispezione.

**Trovato ma non toccato:** `ServiceEvent::SetServerTx` di AI Chat
(`ws.rs`, la riga subito dopo quella del fix) ha la stessa forma —
incondizionata per ogni connessione. Una connessione di canale probabilmente
ruba anche quel sink. Non gateizzato qui: un fix alla cieca potrebbe rompere
AI Chat; da verificare con lo stesso rigore prima di intervenire.

## Wiring dei 3 nuovi tool scan nmap — `NmapToolClient`/`SENSITIVE_TOOLS`/`NMAP_SYSTEM_PROMPT` (0.40.29)

Task 2 del piano `Docs/superpowers/plans/2026-07-18-nmap-scan-variants.md`
(Task 1, in `mcp-nmap` 0.8.0, ha aggiunto i 3 tool scan lato sidecar — vedi
`crates/mcp-nmap/IMPLEMENTATION.md`). Questo task li espone all'AI del
canale `/nmap`.

**`NmapToolClient::tool_defs()`/`dispatch()`.** I 3 nuovi tool
(`nmap_version_scan`, `nmap_host_discovery`, `nmap_vuln_scan`) hanno la
stessa identica identità strutturale di `nmap_quick_scan`/`nmap_os_detect`:
input `{target: string}`, output `{summary, report_markdown, is_error}`
dal sidecar. Questo è diverso dalla situazione di `local_network_info`/
`traceroute` (piano precedente), che avevano uno shape di output diverso
(`{output, is_error}`, niente `report_markdown`) e per questo avevano
richiesto un secondo metodo client-side dedicato (`call_info_tool`) più una
seconda struct di deserializzazione (`NmapInfoOutcomeJson`). Qui non serve
nulla di nuovo: i 3 nuovi tool sono semplicemente 3 nuovi rami nel `match`
di `dispatch()` che chiamano lo stesso `call_scan_tool` già usato da
`nmap_quick_scan`/`nmap_os_detect`, con la stessa `NmapScanOutcomeJson` già
esistente a deserializzare la risposta. `tool_defs()` passa da 4 a 7
`ToolDef`.

**Gate di conferma (`local_confirm::SENSITIVE_TOOLS`).** Tutti e 3 i nuovi
tool sono aggiunti a `SENSITIVE_TOOLS` (ora 5 voci invece di 2): qualunque
tool che esegue attivamente uno scan su un target scelto dall'AI richiede
conferma esplicita dell'utente prima di partire — lo stesso principio già
applicato a `nmap_quick_scan`/`nmap_os_detect`, non una policy nuova
introdotta da questo task. Nessuna logica nuova in `local_confirm.rs`: solo
l'estensione della costante e del test che ne verifica il comportamento.

**`format_nmap_invocation`/`NMAP_SYSTEM_PROMPT` (`external_channel.rs`).**
`format_nmap_invocation` (la copia duplicata lato orchestrator della
funzione omonima di `mcp-nmap::markdown` — stessa motivazione delle voci
precedenti: 8 righe non giustificano una dipendenza cross-crate) guadagna 3
nuovi rami identici a quelli aggiunti in mcp-nmap. `NMAP_SYSTEM_PROMPT` è
stato riscritto da zero (non solo esteso) per elencare tutti e 7 i tool con
prosa propria; l'unico vincolo esplicito aggiunto rispetto alla versione
precedente è che `nmap_vuln_scan` esegue SOLO la categoria NSE `vuln`
integrata in nmap e MAI uno script personalizzato — enunciato per evitare
che l'AI lasci intendere all'utente di poter eseguire NSE arbitrario (il
tool, lato mcp-nmap, non accetta comunque un parametro di script: è
un'affermazione veritiera, non solo una policy). Le istruzioni preesistenti
(chiamare `local_network_info` prima di uno scan senza target, niente
shell/file/URL/finestre Markdown, non ripetere il report) restano invariate
nel contenuto.

**Copertura test.** Nessun nuovo test di parsing JSON necessario: lo shape
*scan* (`NmapScanOutcomeJson`) era già coperto da
`nmap_scan_outcome_json_parses_success_shape`/`_error_shape`, scritti nel
task precedente per `nmap_quick_scan`/`nmap_os_detect` — riusati senza
modifiche, dato che i 3 nuovi tool condividono esattamente lo stesso shape.
Il test `tool_defs_exposes_exactly_the_four_nmap_channel_tools` è stato
rinominato in `tool_defs_exposes_exactly_the_seven_nmap_channel_tools` e
esteso con le 3 nuove asserzioni; il test `should_gate_is_true_for_
sensitive_tools_false_for_others` in `local_confirm.rs` è stato esteso con
le 3 nuove asserzioni sullo stesso principio. Numero di funzioni di test
invariato in entrambi i file (nessun test nuovo aggiunto, due test estesi
in place) — coerente col fatto che nessun comportamento nuovo è stato
introdotto, solo la superficie di 3 tool già identici nella forma a quelli
esistenti.

## Fix: `mcp-nmap.exe` orfano permanente al timeout — `ProcessTreeKiller` (0.40.30)

Bug reale, confermato con una riproduzione dal vivo eseguita PRIMA di
questo task (non un'ipotesi): `NMAP_CALL_TIMEOUT_SECS` abbassata
temporaneamente a 10s, un vero `nmap_vuln_scan` eseguito attraverso
`NmapToolClient::dispatch` direttamente (bypassando WS/router, stesso
percorso di codice di produzione), poi verifica via `Get-Process`/
`Get-CimInstance Win32_Process`/`tasklist` sia subito dopo il timeout sia
90s dopo.

**Diagnosi.** `nmap.exe` termina da solo (limitato dal fix
`--script-timeout` di mcp-nmap 0.8.2) e sparisce entro i 90s — non è il
processo di questo fix. `mcp-nmap.exe` invece resta presente e **invariato**
(stesso PID, nessun riuso) 90s dopo il timeout: il vecchio codice, in
entrambi i rami `Err(_elapsed)` di `call_scan_tool`/`call_info_tool`,
eseguiva solo `*self.peer.lock().await = None;` — azzera l'handle `Peer` in
cache (un riferimento economico) così la chiamata *successiva* spawna un
`mcp-nmap.exe` *nuovo*, ma non tocca in alcun modo il processo figlio già in
esecuzione. `mcp-nmap.exe` è un server MCP via stdio: una volta che
l'orchestrator smette di leggere dal suo stdout (il timeout scade, nessuno
legge più), il processo resta vivo per design, in attesa del prossimo
messaggio — che non arriva mai. Ogni timeout lascia quindi un `mcp-nmap.exe`
in più a vivere per sempre: leak permanente e illimitata, visibile solo dal
Task Manager, un processo/PID in più per ogni scan che supera i 900s.

**Perché non un fix lato `mcp-nmap`.** L'alternativa valutata era rendere
asincrono `NmapProcess::run` (oggi `fn run(&self, args: &[&str], xml_path:
&Path) -> Result<ExitStatus, String>`, sincrono) per propagare un timeout
interno fino al processo `nmap.exe`. Scartata: il cambio si propaga a
`RealNmapProcess` e a circa 15 fake di test (`FakeNmapProcess`/
`CapturingProcess`/`PanicsIfCalled`) sparsi in `scan.rs` — una modifica
molto più ampia e rischiosa per un problema che in realtà riguarda
l'orchestrator che non uccide il **proprio** processo figlio diretto.
`mcp-nmap.exe` è un figlio a un solo salto dell'*orchestrator* (non di
mcp-nmap stesso, che a sua volta genera `nmap.exe`): ucciderlo direttamente
dal lato orchestrator è il fix minimo per la leak confermata. `mcp-nmap`
resta a 0.8.2, invariato — nessun file sotto `crates/mcp-nmap/` toccato da
questo task.

**Il seam: trait `ProcessTreeKiller`.** Stesso pattern di dependency
injection già stabilito nel progetto (mirror di `NmapProcess` in
`mcp-nmap`):
```rust
trait ProcessTreeKiller: Send + Sync {
    fn kill_tree(&self, pid: u32);
}
```
`RealProcessTreeKiller` (implementazione di produzione) esegue `taskkill
/F /T /PID <pid>` su Windows (`spawn()`, non atteso — l'orchestrator non
deve bloccarsi per il tempo di uccisione del processo; esito loggato via
`tracing::info!`/`tracing::warn!`). Il flag `/T` (tree-kill) è difesa in
profondità: Windows non termina a cascata i processi figli quando muore il
padre, quindi senza `/T` un `nmap.exe` ancora vivo al momento del timeout
(es. un hang nella fase di port-scan, NON coperta dal fix `--script-timeout`
di mcp-nmap 0.8.2, che coperiva solo la fase script NSE) resterebbe comunque
orfano. Il fallback non-Windows (`kill -9 <pid>`, nessun tree-kill — niente
process-group/session setup su questa piattaforma ancora) è deliberatamente
una mitigazione più piccola, non un fix completo: macOS/Linux non sono la
piattaforma target attuale del progetto (CLAUDE.md: "Windows attuale;
macOS/Linux pianificati").

**`NmapToolClient`: due nuovi campi.**
```rust
child_pid: Arc<Mutex<Option<u32>>>,
killer: Arc<dyn ProcessTreeKiller>,
```
`child_pid` è il PID di `mcp-nmap.exe` catturato in `ensure_connected`
subito dopo lo spawn (`transport.id()`, chiamato PRIMA che `transport` sia
consumato da `.serve(...)`) — necessario perché non esiste un handle vivo al
processo figlio da cui recuperare il PID più tardi: `ensure_connected` cede
la proprietà del processo al task `tokio::spawn` che attende
`running.waiting()`, quindi l'unico modo per killare il processo dal
metodo di timeout è tenerne il PID a parte. Entrambi i costruttori in
`resolve()` (ramo `LARE_MCP_NMAP` e ramo sibling-del-binario) inizializzano
`child_pid: Arc::new(Mutex::new(None))` e `killer: Arc::new(
RealProcessTreeKiller)`.

**Il helper condiviso `abandon_connection_after_timeout`.**
```rust
async fn abandon_connection_after_timeout(&self) {
    *self.peer.lock().await = None;
    if let Some(pid) = self.child_pid.lock().await.take() {
        self.killer.kill_tree(pid);
    }
}
```
`.take()` su `child_pid` fa doppio lavoro: recupera il PID E azzera lo
stato in un solo passaggio, così una chiamata ripetuta (in teoria non
dovrebbe accadere, ma il codice non lo presuppone) non tenta un secondo
`kill_tree` su un PID già gestito. Usato nei due rami `Err(_elapsed)`
(quello, non il generico `Err(e)`) di `call_scan_tool` e `call_info_tool`,
al posto della riga inline `*self.peer.lock().await = None;` che c'era
prima.

**I rami `Err(e)` generici restano deliberatamente invariati** — continuano
a fare solo `*self.peer.lock().await = None;`, senza chiamare
`abandon_connection_after_timeout` e senza killare nulla. Motivazione: un
errore generico (fallimento di trasporto/protocollo MCP, non un timeout)
indica quasi sempre che il processo si è già chiuso da solo o è comunque in
uno stato che non giustifica un kill esplicito — la distinzione tra "il
timeout è scaduto mentre il processo probabilmente lavora ancora" (kill
necessario) e "la connessione MCP è fallita" (il processo è verosimilmente
già morto) è quella che guida quale dei due rami chiama il nuovo helper.

**Copertura test.** Le 3 struct-literal preesistenti nei test
(`run_in_session_never_executes_a_real_shell`,
`open_target_never_opens_anything`,
`tool_defs_exposes_exactly_the_seven_nmap_channel_tools`) hanno smesso di
compilare non appena `NmapToolClient` ha guadagnato i due nuovi campi — RED
genuino (errore di compilazione `E0405`/campi mancanti, non un typo), fase
attesa e prevista dal task. Aggiornate con
`child_pid: Arc::new(Mutex::new(None)), killer:
Arc::new(FakeProcessTreeKiller::default())`. Nuovo fake di test:
```rust
#[derive(Default)]
struct FakeProcessTreeKiller {
    killed_pids: std::sync::Mutex<Vec<u32>>,
}
impl ProcessTreeKiller for FakeProcessTreeKiller {
    fn kill_tree(&self, pid: u32) {
        self.killed_pids.lock().unwrap().push(pid);
    }
}
```
2 nuovi test (la copertura di regressione reale di questo fix):
`abandon_connection_after_timeout_kills_the_stored_pid_and_clears_state`
(un `child_pid` presente → `kill_tree` chiamato con quel PID, `child_pid` e
`peer` entrambi azzerati dopo) e
`abandon_connection_after_timeout_does_not_kill_when_never_connected`
(nessun `child_pid` mai salvato → `kill_tree` mai chiamato). Nessun processo
reale spawnato in nessuno dei due test — solo il fake.

RED verificato prima del fix: `cargo test -p orchestrator nmap_tool_client`
falliva a compilare con `error[E0405]: cannot find trait 'ProcessTreeKiller'
in this scope` (i 2 nuovi test referenziano il trait/i campi/il metodo
prima che esistano). GREEN dopo il fix: `cargo test -p orchestrator --lib
nmap_tool_client`: 9/9 passati (7 preesistenti + 2 nuovi, 0 falliti).
`cargo test -p orchestrator` (suite intera): 734 test di libreria verdi (4
ignorati, tutti e2e reali preesistenti che richiedono binari plugin
compilati — invariato), 16/16 `ws_integration`, 1/1 doc-test. `cargo build`
(intero workspace, inclusi `mcp-nmap`/`mcp-server`/`protocol`): pulito.
`cargo clippy -p orchestrator --all-targets`: stessi 4 warning
pre-esistenti di 0.40.29 (`ai_adapter.rs:331`, `ws.rs:586`,
`aichat/service.rs:5604`/`7958`), zero warning nuovi in
`nmap_tool_client.rs` o altrove. `cargo fmt -p orchestrator --check`: il
file aveva già drift di formattazione pre-esistente prima di questo task
(confermato confrontando con `git stash`/`fmt --check` sulla versione
precedente — 14 punti di diff pre-esistenti in questo stesso file), coerente
col drift repo-wide già documentato (commit `0088e39`); il codice nuovo
segue lo stile già presente nel file, nessuna regressione di formattazione
introdotta da questo task specificamente.

## Fix: `mcp-nmap.exe` orfano permanente alla chiusura normale — `ToolClient::shutdown()` (0.40.31)

Secondo trigger della stessa leak di 0.40.30, lasciato esplicitamente come
debito aperto in quella voce di CHANGELOG/IMPLEMENTATION: la review
indipendente che aveva scoperto il bug del timeout aveva notato che una
chiusura NORMALE della sessione `/nmap` (uno scan completato con successo,
poi la finestra chiusa — o anche nessuno scan mai lanciato) lascia lo
stesso `mcp-nmap.exe` orfano per sempre. Nessuna regressione di 0.40.30:
il vecchio codice (prima di ENTRAMBI i fix) aveva la stessa lacuna sui due
trigger indipendentemente.

**Diagnosi (rilettura completa di `handle_connection`, non un'ipotesi).**
`NmapToolClient` non aveva alcun `Drop`/hook di teardown, e prima di
questo task nemmeno il trait `ToolClient` ne esponeva uno. Il task
`tokio::spawn(async move { running.waiting().await.ok(); })` dentro
`ensure_connected` (`nmap_tool_client.rs`) tiene vivo l'handle del
processo `mcp-nmap.exe` in modo completamente scollegato dal ciclo di vita
di `NmapToolClient` stesso — quando la connessione WS finisce e
`NmapToolClient` va fuori scope, quel task resta comunque vivo, aspettando
`running.waiting()` che non si risolve mai finché il processo non muore da
solo (mai, essendo un server MCP stdio che aspetta messaggi). Per
localizzare l'UNICO punto di aggancio necessario, `handle_connection`
(`ws.rs`) è stata letta per intero, non solo la sezione del teardown:
dopo che il binding locale `tools` viene risolto da
`resolve_channel_tools` (riga ~199 — shadowing del parametro condiviso,
commento già presente in loco lo spiega), l'unico percorso che raggiunge
la fine della funzione — quindi il blocco di chiusura righe ~655-676 — è
l'uscita naturale dal loop principale `while let Some(client_msg) =
in_rx.recv().await` quando la connessione WS si chiude (utente chiude la
finestra). Ogni altro `return` nella funzione (handshake fallito, token
sbagliato, canale sconosciuto) avviene PRIMA di quel binding — verificato
con una scansione mirata di tutti i `return`/`?` tra la riga 216 e la 655,
zero trovati. Un solo punto di aggancio, quindi, chiude completamente la
lacuna: nessun'altra via di uscita da `handle_connection` resta scoperta.

**Il nuovo hook: `ToolClient::shutdown()` (`tool_client.rs`).**
```rust
async fn shutdown(&self) {}
```
Aggiunto come ultimo metodo del trait, con **default no-op** — la stessa
tecnica di "opt-in silenzioso" già usata per `tool_defs()`/`dispatch()`
(0.40.20): ogni `ToolClient` esistente (`McpToolClient`, `FakeToolClient`,
`FixtureChannelToolClient`) eredita il default senza alcuna modifica al
proprio codice. Il no-op è corretto per `McpToolClient` non per pigrizia
ma per design: il processo `mcp-server` che gestisce è un singleton a vita
di *workspace* (condiviso da tutte le connessioni cursore/Telegram), non
una risorsa di proprietà esclusiva della singola connessione — ucciderlo
al termine di UNA connessione romperebbe tutte le altre ancora attive.
Solo un `ToolClient` di canale come `NmapToolClient`, che possiede in modo
esclusivo un processo per-connessione (garanzia già documentata nel
commento sull'invariante di concorrenza del tipo, 0.40.24), ha davvero
qualcosa da rilasciare in `shutdown()`.

**Il rename: `abandon_connection_after_timeout` → `close_connection`
(`nmap_tool_client.rs`).** Il nome originale (0.40.30) descriveva un solo
chiamante (il timeout); ora ne ha tre (i due rami `Err(_elapsed)` di
`call_scan_tool`/`call_info_tool`, invariati, più il nuovo `shutdown()`),
quindi il nome doveva perdere il riferimento esplicito al timeout. Corpo
del metodo **invariato byte-per-byte**:
```rust
async fn close_connection(&self) {
    *self.peer.lock().await = None;
    if let Some(pid) = self.child_pid.lock().await.take() {
        self.killer.kill_tree(pid);
    }
}
```
`Option::take()` su `child_pid` continua a fare doppio lavoro (recupera E
azzera in un solo passaggio), quindi resta idempotente e race-safe anche
con tre chiamanti anziché uno: se un timeout e una chiusura di finestra
corressero (scenario oggi impossibile per l'invariante di concorrenza
documentato sul tipo — una sola `dispatch()` in volo per connessione — ma
il codice non lo presuppone strutturalmente), solo il primo vedrebbe
`Some(pid)`, l'altro `None` e farebbe no-op. Aggiornati anche tutti i
riferimenti in prosa al vecchio nome nei doc-comment del modulo
(invariante di concorrenza sul tipo, doc del campo `child_pid`) — non solo
le due call-site nel codice — perché altrimenti avrebbero puntato a un
metodo inesistente dopo il rename.

**`impl ToolClient for NmapToolClient`: `shutdown()`.** Aggiunto come
primo metodo del blocco impl, prima di `run_in_session`:
```rust
async fn shutdown(&self) {
    self.close_connection().await;
}
```
Nessuna logica propria: riusa esattamente il meccanismo di kill già
provato dal fix 0.40.30, zero duplicazione.

**`ws.rs`: il call site.** Una riga nel blocco di chiusura di
`handle_connection`, subito dopo il drain di `commands`/`searches` e
prima della notifica `ServiceEvent::UiClosed` ad AI Chat:
```rust
tools.shutdown().await;
```
Il binding `tools` in scope qui è quello RISOLTO da
`resolve_channel_tools` (~riga 199, shadowing locale, non il parametro
condiviso originale) — per una connessione `/nmap` è l'`Arc<dyn
ToolClient>` che avvolge un `NmapToolClient`, quindi questa chiamata
uccide davvero `mcp-nmap.exe` se ancora vivo. Per cursore/Telegram è il
`McpToolClient` condiviso, che eredita il default no-op — nessun impatto,
nessuna regressione: il singleton `mcp-server` non viene mai toccato da
questa chiamata.

**Copertura test.** Rinominati (stesso corpo, solo il nome del metodo
chiamato) `abandon_connection_after_timeout_kills_the_stored_pid_and_
clears_state` → `close_connection_kills_the_stored_pid_and_clears_state` e
`abandon_connection_after_timeout_does_not_kill_when_never_connected` →
`close_connection_does_not_kill_when_never_connected`. Nuovo test in
`nmap_tool_client.rs`:
```rust
#[tokio::test]
async fn shutdown_kills_the_connected_process() {
    use crate::tool_client::ToolClient;
    let killer = Arc::new(FakeProcessTreeKiller::default());
    let client = NmapToolClient {
        mcp_nmap_path: "unused".into(),
        peer: Arc::new(Mutex::new(None)),
        child_pid: Arc::new(Mutex::new(Some(9001))),
        killer: killer.clone(),
    };
    client.shutdown().await;
    assert_eq!(killer.killed_pids.lock().unwrap().as_slice(), &[9001]);
}
```
Prova la ROTTA reale (`ToolClient::shutdown()`, il metodo che `ws.rs`
chiama davvero attraverso il trait object), non solo che `close_connection`
funziona isolatamente — differenza rilevante perché un bug di wiring (es.
`shutdown()` che dimentica di chiamare `close_connection()`) sarebbe
passato inosservato testando solo il metodo inerente. Nuovo test in
`tool_client.rs`:
```rust
#[tokio::test]
async fn default_shutdown_is_a_true_noop() {
    let client = FakeToolClient::success("ok");
    client.shutdown().await;
}
```
Nessun assert oltre "non panica e ritorna" — è la prova che serve per un
default no-op: guardia contro un futuro refactor che renda `shutdown` un
metodo required (romperebbe ogni `ToolClient` storico che non ha nulla da
rilasciare).

**RED verificato prima del fix** (non assunto): `cargo test -p
orchestrator --lib nmap_tool_client` e `--lib tool_client` fallivano a
compilare con `error[E0599]: no method named 'close_connection'/
'shutdown' found for struct ...` — i test rinominati/nuovi referenziano
metodi che non esistevano ancora, non un typo. **GREEN dopo il fix**:
`cargo test -p orchestrator --lib nmap_tool_client`: 10/10 passati (7
preesistenti + 2 rinominati con corpo invariato + 1 nuovo, 0 falliti).
`cargo test -p orchestrator --lib tool_client`: 39/39 passati (38
preesistenti + 1 nuovo). `cargo test -p orchestrator --lib` (suite
completa): 736 passati (734 di 0.40.30 + 2 nuovi), 0 falliti, 4 ignorati
(invariato — e2e reali preesistenti che richiedono binari plugin
compilati). `cargo test -p orchestrator` (intera suite incl. integration
test): 16/16 `ws_integration`, 1/1 doc-test, tutto verde — conferma che il
nuovo `tools.shutdown().await` in `ws.rs` non ha rotto nessuno scenario di
chiusura connessione già coperto. `cargo build` (intero workspace,
inclusi `mcp-nmap`/`mcp-server`/`protocol`/plugin): pulito. `cargo clippy
-p orchestrator --all-targets`: stessi 4 warning pre-esistenti di 0.40.30
(`ai_adapter.rs:331`, `ws.rs:586`, `aichat/service.rs:5604`/`7958`), zero
warning nuovi introdotti da questo task in nessuno dei 3 file toccati.
`cargo fmt -p orchestrator --check`: confrontato il conteggio di diff nei
3 file toccati PRIMA (via `git stash`) e DOPO questo task — 13 in
`nmap_tool_client.rs`, 8 in `tool_client.rs`, 14 in `ws.rs` in entrambi i
casi, identico — zero drift di formattazione nuovo introdotto; il codice
aggiunto segue a mano lo stile già presente, evitando di far esplodere il
diff con un `cargo fmt` cieco sull'intero file (che avrebbe anche
"corretto" il drift pre-esistente non richiesto da questo task).

Nessun file sotto `crates/mcp-nmap/` toccato. Nessun processo reale
nmap/mcp-nmap eseguito in nessuna fase di questo task — validazione
interamente tramite `FakeProcessTreeKiller`/`FakeToolClient`,
deterministica, coerente col vincolo esplicito del task.

### Debito residuo trovato dalla review, NON chiuso in questo commit

L'affermazione sopra ("chiude completamente la lacuna") è vera nel suo
scope preciso — nessun'altra via di uscita da `handle_connection` resta
scoperta — ma NON copre un secondo, più stretto percorso di leak trovato
dalla stessa review: `ai_adapter.rs`'s loop di dispatch dei tool_use
controlla la cancellazione una sola volta per iterazione ESTERNA del turno
AI (`ai_adapter.rs:331`, dentro `for _ in 0..agent::MAX_ITERATIONS`), MAI
tra un `tools.dispatch()` e il successivo nel loop INTERNO che itera sui
tool_use dello stesso turno (`ai_adapter.rs:432`, `for (tool_id, name,
input) in tool_uses`). Verificato con un grep mirato di `is_cancelled`, non
ipotizzato.

Se un turno AI richiede più tool_use in un solo batch (il system prompt di
questo canale incoraggia esplicitamente `local_network_info` seguito da uno
scan) e la connessione WS cade a metà batch, il PRIMO dispatch di quel
batch può già aver chiamato `close_connection` (via il proprio timeout);
il dispatch SUCCESSIVO nello stesso batch, non essendo mai stato
interrotto da un controllo di cancellazione, procede comunque: trova
`peer == None`, fa ripartire `ensure_connected`, spawna un `mcp-nmap.exe`
nuovo. Se quella chiamata completa con successo (il caso comune — non un
timeout), nulla la uccide più: `shutdown()` è già stato chiamato una volta
per questa connessione da `ws.rs` e non verrà richiamato una seconda.

Deliberatamente non risolto qui: `ai_adapter.rs`'s loop di dispatch serve
OGNI canale (cursore, Telegram, `/nmap`), non solo nmap — un cambio lì per
chiudere un leak specifico di un canale ha un raggio d'azione troppo ampio
per essere deciso dentro questo task. Molto più stretto del bug pre-0.40.31
(che si verificava a OGNI chiusura normale, non solo in questo scenario
composto), quindi il fix resta un miglioramento netto — ma non
un'eliminazione totale del problema del processo orfano. Task futuro, non
schedulato.

### Nota minore, non bloccante: `CwdTrackingToolClient` non inoltra `shutdown()`

`cwd_tracking.rs`'s `impl ToolClient for CwdTrackingToolClient` non
sovrascrive `shutdown()` — eredita il default no-op del trait invece di
inoltrare a `self.inner.shutdown()`. Oggi innocuo: `CwdTrackingToolClient`
avvolge solo `McpToolClient` (il cui `shutdown()` è anch'esso un no-op), mai
`NmapToolClient`. Ma è un Decorator incompleto — se in futuro qualcosa
avvolgesse un `ToolClient` che possiede davvero una risorsa (come
`NmapToolClient`) dentro `CwdTrackingToolClient`, questo stesso leak
tornerebbe in modo silenzioso, senza alcun test a intercettarlo. Fix da una
riga (`async fn shutdown(&self) { self.inner.shutdown().await; }`), non
applicato qui perché difende un caso che oggi non esiste — segnalato per
quando (se) servirà davvero.

## `ChatBackend::supports_web_search` — il system prompt non sapeva cosa il tool set stava scartando (0.40.34)

**Bug osservato dal vivo dall'utente** (2026-07-20): toggle "Ricerca web" ON,
provider attivo DeepSeek via OpenRouter. L'AI invoca `[tool web_search]`,
riceve un `tool_result` di errore, e la sua risposta successiva fabbrica una
scusa plausibile ("non ho accesso diretto a strumenti di ricerca web") — non
un crash, ma un comportamento silenziosamente sbagliato che sembra un limite
del modello invece che un bug dell'orchestrator.

**Causa radice — due metà del meccanismo già note separatamente, mai
riconciliate:** `agent::tools_for(opts)` aggiunge i due `ToolSpec::Server`
(`web_search`/`web_fetch`) se `opts.web_search`; `OpenRouterBackend::to_or_tools`
scarta OGNI `ToolSpec::Server` in traduzione — comportamento corretto e già
documentato (vedi nota 0.40.8 sopra, "niente `web_search`/`web_fetch` su
questo percorso, di natura strutturale"). Ma `agent::system_prompt(opts)`
appende `WEB_SEARCH_ADDENDUM` ("Hai inoltre due strumenti per il web... USALI")
in base allo STESSO `opts.web_search`, **senza sapere** che il tool set
effettivamente inviato al backend OpenRouter non li conterrà. Il modello
legge nel prompt di avere una capacità che il suo schema di funzioni
disponibili smentisce — e la tenta comunque (niente, lato OpenRouter/DeepSeek,
impedisce di emettere un `tool_call` per un nome non dichiarato). L'unico
punto che conosceva ENTRAMBE le metà (il prompt promesso e il tool set
davvero inviato) era `LlmAdapter::respond`, che ha sia `opts` sia
`self.backend` — ma non li metteva in relazione.

**Fix — un metodo di capacità sul backend, un gate a monte nel chiamante:**
```rust
// chat_backend.rs — default true (comportamento Claude reale, invariato)
trait ChatBackend {
    fn supports_web_search(&self) -> bool { true }
}
// openrouter_backend.rs — coerente con to_or_tools, stesso posto logico
impl ChatBackend for OpenRouterBackend {
    fn supports_web_search(&self) -> bool { false }
}
// ai_adapter.rs — LlmAdapter::respond, PRIMA del loop di tool-use
let opts = agent::TurnOptions {
    web_search: opts.web_search && self.backend.supports_web_search(),
    ..opts
};
```
Un solo flag (`opts.web_search`) guida sia `agent::system_prompt` sia
`agent::tools_for` — bastava spegnerlo una volta, a monte, perché entrambe le
metà del meccanismo restassero coerenti tra loro. Nessuna nuova granularità:
il caso `ClaudeBackend`/`web_fetch_supported` (endpoint Anthropic-compatibili
che capiscono `web_search` ma non `web_fetch`, vedi release 0.38.9) resta un
meccanismo distinto e non tocca il prompt — fuori scope di questo fix, che
copre solo lo scenario osservato (assenza TOTALE di web search, non parziale).

**Test (RED-first):** `openrouter_backend_does_not_support_web_search`
(`openrouter_backend.rs`, la capacità dichiarata) +
`web_search_mode_is_neutered_when_backend_does_not_support_it`
(`ai_adapter.rs`, riproduce lo scenario esatto del bug via `FakeChatBackend
::with_web_search_supported(false)` — asserisce che NÉ il tool set NÉ il
system prompt inviati al backend menzionino `web_search`). 739 test lib
verdi (+2), clippy/fmt sui 3 file toccati (`chat_backend.rs`,
`openrouter_backend.rs`, `ai_adapter.rs`) senza nuovi warning/drift oltre
quelli pre-esistenti già noti (vedi nota release 0.34.0).

**Non toccato, deliberatamente:** il caso `ClaudeBackend`/DeepSeek-via-
endpoint-Anthropic-diretto (`with_web_fetch_supported`) ha lo stesso
possibile gap teorico sul solo `web_fetch` (il prompt promette anche quello,
sempre) — mai osservato dal vivo, endpoint diverso (wire nativo Anthropic,
probabilmente più severo su tool non dichiarati), non riprodotto: non è
in scope di un fix guidato da un bug osservato specifico.

## Canale `library-expand` — turno AI isolato per l'espansione di un documento Library (0.40.35)

Secondo consumatore del meccanismo canale esterno (`external_channel.rs`),
dopo `/nmap`. Zero modifiche a `ws.rs`/protocollo: `resolve_channel_tools`
era già generico su qualunque voce di `EXTERNAL_TOOL_CHANNELS`, aggiungere
un canale è SOLO una voce di registro + il suo `ToolClient`.

`EmptyToolClient` non ha alcun tool custom (`tool_defs()` → `[]`) — l'AI di
questo canale vede SOLO `web_search`/`web_fetch` se il turno li richiede
(`TurnOptions.web_search`, dal toggle "Ricerca web" — ortogonale al
`ToolClient` del canale, li aggiunge sempre `agent::tools_for`). Qualunque
altro tool (anche `run_in_session`, se il modello lo invocasse per errore)
è rifiutato strutturalmente da `dispatch()`, stesso principio di
`NmapToolClient`.

`LIBRARY_EXPAND_SYSTEM_PROMPT` è fisso: dice all'AI che riceverà un
documento + una richiesta e deve restituire l'INTERO documento fuso, non
un'aggiunta in coda. Il documento stesso NON può stare qui — è
`&'static str`, non può contenere dati che variano per richiesta — vive
invece nel testo del messaggio utente del turno (`input`, già un parametro
per-chiamata di `LlmAdapter::respond`), esattamente come qualunque
richiesta NL normale.

Nessuno `slash_trigger` reale collegato: il campo è obbligatorio sullo
struct ma il registro frontend `external-channels.js` (che decide quali
trigger il cursore riconosce) non guadagna una voce corrispondente — la
finestra di espansione si apre solo dal pulsante dentro un documento
Library già aperto (Task 4 del piano), mai da uno slash del cursore.

Vedi `Docs/superpowers/specs/2026-07-20-library-expand-design.md`.

## `PythonMcpToolClient` — infrastruttura tool esterni Python, Task 1/4 (0.40.37)

Nuovo file `crates/orchestrator/src/python_mcp_tool_client.rs` (dichiarato in
`lib.rs` con `pub mod python_mcp_tool_client;`, alfabeticamente tra
`nmap_tool_client` e `router` — nessun conflitto stavolta tra "subito dopo
nmap_tool_client" e l'ordine alfabetico, a differenza di quanto successo per
`nmap_tool_client` stesso in 0.40.24).

Vedi `Docs/superpowers/specs/2026-07-21-pytools-infrastructure-design.md` per
il design completo (perché ora, alberatura `pytools/<domain>/venv/`,
provisioning manuale del venv, granularità per-dominio non per-tool).

**Struttura, mirror di `NmapToolClient`:** `python_path`/`script_path:
PathBuf` (invece del singolo `mcp_nmap_path` — qui ci sono DUE cammini
risolti, interprete E script) + `peer: Arc<Mutex<Option<rmcp::Peer<
rmcp::RoleClient>>>>` + `child_pid: Arc<Mutex<Option<u32>>>` + `killer:
Arc<dyn ProcessKiller>`. `ProcessKiller` è la stessa astrazione DI-per-test
di `NmapToolClient::ProcessTreeKiller`, ma senza kill-tree (`/T`): lo script
Python di questo canale non spawna figli elevati via UAC come fa
`nmap_os_detect::elevate.rs`, quindi un semplice `taskkill /F /PID` (Windows)
o `kill -9` (Unix) basta.

**`resolve(domain_id, script_relpath, env_override)`.** A differenza di
`NmapToolClient::resolve()` (che ha UN solo env var fisso, `LARE_MCP_NMAP`),
qui l'env var di override è un **parametro**, non una costante — coerente col
fatto che questo tipo è generico e riusabile da più domini futuri (nmap ha
un solo dominio, non ne servirebbe mai un secondo). Radice: `env_override`
se la var è impostata, altrimenti `pytools/<domain_id>/` sibling di
`current_exe()`. Interprete: `venv/Scripts/python.exe` su Windows,
`venv/bin/python3` su Unix (helper `#[cfg(windows)]`/`#[cfg(not(windows))]`
inline, stile `PathProvider` — nessun trait dedicato, l'unica biforcazione è
questa singola espressione). Verifica **prima di spawnare** che interprete E
script esistano — se manca l'uno o l'altro, `anyhow::bail!` con un messaggio
che nomina esplicitamente il `domain_id` e il path mancante ("crea il
virtual environment (vedi pytools/README.md)" per il venv, il path atteso
per lo script) — mai un panic, l'utente deve poter risolvere da solo senza
leggere il sorgente.

**`ensure_connected()`/`close_connection()`** sono la copia esatta del
pattern di `NmapToolClient` (spawn lazy via `TokioChildProcess::new` +
`().serve(transport).await` per l'handshake MCP, peer clonato e riusato alle
chiamate successive, task `tokio::spawn` che tiene vivo il servizio). Unica
differenza: il comando spawnato è `python_path` con **un argomento**,
`script_path` (`c.arg(&self.script_path)`) — `NmapToolClient` non passa
argomenti al sidecar (`mcp-nmap.exe` non ne ha bisogno, è un binario
autonomo). `close_connection()` azzera il peer e uccide il PID tracciato,
se presente — stessa motivazione del fix nmap 0.40.30/0.40.31 (senza
questo, il processo Python resterebbe orfano indefinitamente al timeout o
alla chiusura della connessione WS).

**`impl ToolClient`.** `run_in_session`/`open_target`/`reset_session` sono
stub identici a `FixtureChannelToolClient`/`NmapToolClient`: "non
disponibile su questo canale" (`exit_code: -1` / `ok: false`), `
reset_session` no-op. `tool_defs()` espone **un solo** tool, `pyping`
(argomento `message: string`) — un tool di prova deliberatamente banale,
mirror di `plugin-ping` per il sistema plugin.json: isola i bug di
meccanismo (venv, spawn, handshake MCP) da quelli di dominio, prima che
arrivi un tool Python reale (investpy, sub-progetto 2). `dispatch()`
rifiuta ogni nome diverso da `"pyping"` come "tool sconosciuto sul canale
python-ping" (stessa via di un nome davvero inventato dal modello); per
`"pyping"`, chiama `ensure_connected()`, poi `peer.call_tool(...)` sotto un
`tokio::time::timeout(PYTHON_TOOL_CALL_TIMEOUT_SECS)` — **60s**, non i 900s
di nmap: oggi questo canale copre solo un'eco istantanea, un timeout più
lungo non è generalizzato finché non esiste un tool Python reale che lo
richieda (YAGNI, annotato nel commento della costante). Sul timeout: la
connessione viene chiusa (`close_connection().await`, uccide il processo)
e ritorna `DispatchOutcome{is_error: true, report: None}` con un messaggio
leggibile; sulla risposta valida, il testo del content MCP diventa
`DispatchOutcome.output` direttamente (nessun contratto JSON intermedio da
deserializzare, a differenza di `NmapScanOutcomeJson`/`NmapInfoOutcomeJson`
— `pyping` è troppo semplice per giustificarne uno: l'eco è già la
risposta finale per il modello).

**Deviazione dal brief (Task 1), corretta in scrittura — bug reale, non
scelta di stile:** i 2 test `resolve_errs_with_readable_message_when_*`
nel brief chiamavano `.expect_err("...")` sul `Result<PythonMcpToolClient,
anyhow::Error>` restituito da `resolve()`. `Result::expect_err` richiede
`T: Debug` sul tipo Ok (per poter stampare il valore nel panic message se
il risultato fosse `Ok` a sorpresa) — ma `PythonMcpToolClient` non
implementa (e non può derivare a costo zero) `Debug`: contiene `Arc<dyn
ProcessKiller>`, un trait object il cui trait non ha `Debug` come
supertrait, quindi non soddisfa il bound. Risultato: `error[E0277]:
PythonMcpToolClient doesn't implement std::fmt::Debug`, non l'errore di
compilazione "manca il tipo" atteso al gate RED — un secondo errore di
compilazione sopra a quello atteso, che andava distinto dal RED
legittimo. Corretto sostituendo `.expect_err(msg)` con un `match` esplicito
(`Err(e) => e, Ok(_) => panic!(msg)`) in entrambi i test — `match` non
richiede alcun bound su `T`, solo ownership del `Result`. Non ho aggiunto
`#[derive(Debug)]`/un impl manuale a `PythonMcpToolClient` (avrebbe
richiesto anche `Debug` come supertrait di `ProcessKiller` + impl su
`RealProcessKiller`/`FakeProcessKiller`, solo per soddisfare due
assert! di test) — `NmapToolClient` non deriva `Debug` neppure lui, per
coerenza lo stesso vale qui.

**Warning transitori attesi, non un problema:** tra lo Step 3 (solo
`resolve()`) e lo Step 7 (impl `ToolClient` completa), `cargo build`
segnalava `field \`peer\`/\`child_pid\`/\`killer\` is never read` — atteso,
questi campi non erano ancora letti da nessun metodo prima che
`ensure_connected`/`close_connection`/`ToolClient` esistessero. Il warning
sparisce non appena l'impl completa è in albero (verificato: build finale
pulita, zero warning).

9 unit test (3 di `resolve()` + 6 di `ToolClient`, tutti via costruzione
diretta per struct literal con `FakeProcessKiller` — nessuno spawna un
processo reale) + 1 test `#[ignore = "richiede pytools/python-ping/venv
creato a mano"]` di round-trip reale contro `pytools/python-ping/venv`, che
non esiste ancora (crea lo script + il venv è il Task 3 del piano — questo
test resta rosso/non eseguibile finché quel task non è fatto, per design).

**Non ancora collegato a `EXTERNAL_TOOL_CHANNELS`** (`external_channel.rs`,
invariato in questo task) — la voce di registro `"python-ping"` +
`PYTHON_PING_SYSTEM_PROMPT` è il Task 2/4.

## Canale `"python-ping"` registrato in `EXTERNAL_TOOL_CHANNELS`, Task 2/4 (0.40.38)

Terzo consumatore del meccanismo canale esterno (`external_channel.rs`), dopo
`/nmap` e `library-expand`. Mirror strutturale esatto delle voci precedenti:
zero modifiche a `ws.rs`/protocollo, `resolve_channel_tools` era già generico
su qualunque voce di `EXTERNAL_TOOL_CHANNELS` — aggiungere un canale resta
SOLO una voce di registro + il suo `ToolClient`.

`PYTHON_PING_SYSTEM_PROMPT` (const, dichiarato subito dopo
`LIBRARY_EXPAND_SYSTEM_PROMPT`, prima di `EXTERNAL_TOOL_CHANNELS`): dice
all'AI di questo canale che ha ESATTAMENTE un tool (`pyping`) e che il suo
unico compito è chiamarlo con il messaggio dell'utente e riportarne l'eco,
senza commenti propri — stesso principio di `NMAP_SYSTEM_PROMPT`/
`LIBRARY_EXPAND_SYSTEM_PROMPT` (dire esplicitamente all'AI cosa NON ha, per
non farle inventare un tool che non esiste).

Voce di registro `EXTERNAL_TOOL_CHANNELS[2]`: `id: "python-ping"`,
`slash_trigger: "/pyping"` (Task 3 userà questi due valori ESATTI nel mirror
frontend `external-channels.js`), `window_title: "Lare — Python ping"`,
`format_invocation: None` (fallback su `agent::display_invocation` di
default — il canale non ha bisogno di una formattazione custom del banner
di conferma, a differenza di `format_nmap_invocation`).

**`tool_client` factory — differenza di comportamento importante rispetto
alle voci precedenti.** `NmapToolClient::resolve()` e la factory di
`EmptyToolClient` (`library-expand`) non falliscono MAI — sono chiamate
sincrone che non toccano il filesystem in modo che possa mancare qualcosa.
`PythonMcpToolClient::resolve("python-ping", "server.py",
"LARE_PYTOOLS_DIR")` invece PUÒ legittimamente restituire `Err`: verifica
che il venv Python (`pytools/python-ping/venv/...`) esista PRIMA di
costruire il client, e quel venv non esiste ancora su questa macchina di
sviluppo — arriva solo nel Task 4 del piano (script + venv reali). Questo
non è un bug della registrazione: è lo stato atteso oggi, esattamente come
documentato nel commento del test `python_ping_channel_tool_client_factory_
does_not_panic`.

**Conseguenza sui test.** Il test sul system prompt
(`python_ping_system_prompt_constant_is_set`) legge il campo statico
`EXTERNAL_TOOL_CHANNELS[2].system_prompt_override` DIRETTAMENTE, non tramite
`resolve_channel_tools` (che invocherebbe la factory fallibile e sarebbe
quindi ambientalmente fragile — verde solo sulle macchine con il venv già
creato). Il test sulla factory
(`python_ping_channel_tool_client_factory_does_not_panic`) tollera
esplicitamente sia `Ok` che `Err` (`match ... { Ok(_) | Err(_) => {} }`):
verifica solo l'assenza di panic nella costruzione, non l'esito. Nessuno dei
due test dipende dallo stato del filesystem locale (venv presente o
assente) per passare — a differenza di `nmap_channel_tool_client_factory_
constructs_without_panicking`/`library_expand_tool_client_has_no_custom_
tools`, che invece ASSERISCONO `Ok` perché per quelle due voci è garantito
dalla factory stessa.

Aggiornato anche `production_registry_has_the_library_expand_channel`: il
registro è ora a 3 voci (non più 2), `library-expand` resta comunque
all'indice 1, invariato.

3 nuovi test + 1 modificato, tutti in `external_channel.rs`. Suite completa
`cargo test -p orchestrator`: nessuna regressione (vedi `task-2-report.md`
per l'output completo).

## Fix di review su `python_mcp_tool_client.rs` (0.40.39)

Tre finding da una code review sull'intero branch pytools-infra, tutti
dentro `python_mcp_tool_client.rs` — nessuno tocca `external_channel.rs` né
la voce di registro `"python-ping"`.

**Fix 1 — `resolve()` era asimmetrico sull'env override.** Prima:
`root = env_override_path` (l'INTERO valore della variabile, senza
`domain_id` unito) nel ramo con la variabile impostata, contro
`<exe_dir>/pytools/<domain_id>` nel ramo di default — due formule diverse
per lo stesso concetto ("dov'è la cartella di questo ambito"). Rifattorizzato
in due passi distinti dentro `resolve()`: prima si calcola `pytools_root`
(env_override se impostata, altrimenti `<exe_dir>/pytools`, SENZA
`domain_id`), poi `let root = pytools_root.join(domain_id);` fuori
dall'if/else — un solo punto che unisce `domain_id`, eseguito sempre.
Zero cambi sotto quel punto: interprete/script si calcolano da `root`
esattamente come prima.

Perché importava: (1) il ramo di default non risolve mai in pratica — non
esiste un `build.rs`/una risorsa Tauri che copi `pytools/` accanto
all'eseguibile compilato, quindi prima di questo fix la variabile era
obbligatoria de-facto pur essendo documentata come override; (2) con la
vecchia semantica, la variabile era di fatto per-canale (il suo valore
*era* la cartella di `python-ping`) mentre `PythonMcpToolClient` è
deliberatamente generico — il prossimo dominio Python (dati finanziari,
citato nel design doc come motivo di questa infrastruttura) avrebbe dovuto
o introdurre una seconda variabile o rischiare di risolvere silenziosamente
dentro la cartella di `python-ping` se qualcuno avesse riusato
`LARE_PYTOOLS_DIR` per errore.

**TDD**: i 3 test esistenti di `resolve()` puntavano la variabile a
`tmp.path().join("test-domain")` (la cartella dell'ambito, coerente con la
vecchia semantica). Aggiornati a puntare a `tmp.path()` (la radice) — RED
confermato: 2 dei 3 test falliscono contro il codice non ancora modificato
(`resolve_errs_with_readable_message_when_script_missing` e
`resolve_succeeds_when_venv_and_script_exist`; il terzo passa per
coincidenza — il messaggio d'errore nomina comunque `domain_id`, indipendente
da quale path esatto sia stato cercato). Verde dopo il fix di produzione.
Aggiornato anche il test `#[ignore]` `real_pyping_roundtrip_via_venv`: ora
punta `LARE_PYTOOLS_TEST_REAL` a `pytools/` (non più `pytools/python-ping/`),
lasciando che `resolve("python-ping", ...)` unisca il dominio da solo — lo
stesso path che percorrerà `external_channel.rs` in produzione.

**Documentazione aggiornata in coerenza:** `pytools/README.md` (sostituita
la sezione "due percorsi diversi, non simmetrici" introdotta in b169a65 —
che documentava correttamente il comportamento vecchio ma ora descriverebbe
un bug — con un'unica formula simmetrica + nota esplicita che in dev
`LARE_PYTOOLS_DIR` va sempre impostata, con l'one-liner PowerShell) e
`CLAUDE.md` (riga `LARE_PYTOOLS_DIR` nella tabella variabili d'ambiente: da
"override opzionale test/dev" a "radice richiesta in dev").

**Fix 2 — invariante di concorrenza non documentato.** `NmapToolClient`
porta, sopra il suo struct, un lungo doc comment che spiega perché tenere
`peer`/`child_pid` in due `Mutex` separati (invece di uno unico) è sicuro
oggi: al più una `dispatch()` per connessione è mai in volo (garantito da
`ws.rs` che tiene il lock di `ConversationHistory` per tutta la durata di
`handle_command(...).await`), e ogni connessione ha il proprio
`ToolClient` fresco (`external_channel.rs`, factory chiamata una volta per
connessione). `PythonMcpToolClient` condivide esattamente la stessa
struttura (due `Mutex` separati) e quindi lo stesso invariante, ma non
aveva il commento. Portato sopra lo struct, adattato: rimossa ogni
menzione del tracking multi-tool di nmap (qui c'è un solo tool, `pyping`)
e del kill-tree (qui `ProcessKiller::kill` è un kill singolo, non un
`/T`— già spiegato nel doc comment di modulo in testa al file, richiamato
qui per non duplicare). Aggiunti anche doc comment per-campo sui 5 campi
dello struct (`python_path`, `script_path`, `peer`, `child_pid`, `killer`)
e i brevi commenti "perché" che il sibling nmap ha su `shutdown`/
`run_in_session`/`reset_session`/`open_target` (il punto comune: questo
canale non ha strutturalmente una via shell, `dispatch()` è l'unica porta
verso il processo Python). Nessun cambiamento di comportamento — solo
commenti e doc comment.

**Fix 3 — `shutdown_kills_the_connected_process` copriva solo metà
dell'invariante.** Asseriva solo `killed_pids == [9001]`, non che
`peer`/`child_pid` fossero stati azzerati da `Option::take()` — che è
l'invariante reale "niente doppio kill" (una seconda `shutdown()`/
`close_connection()` sullo stesso client non deve richiamare `kill()` una
seconda volta, perché non troverebbe più nulla in `child_pid`). Aggiunte le
due asserzioni mancanti (`child_pid.lock().await.is_none()`,
`peer.lock().await.is_none()`), mirror esatto di
`NmapToolClient::close_connection_kills_the_stored_pid_and_clears_state`.

**Verifica.** `cargo test -p orchestrator`: 757 lib passed / 0 failed / 5
ignored, invariato rispetto alla baseline pre-fix (nessun test aggiunto —
solo 2 assert in più su un test esistente + 3 test riscritti per la nuova
semantica). `cargo clippy -p orchestrator --all-targets`: stessi 4 warning
pre-esistenti (`ai_adapter.rs:360`, `ws.rs:586`,
`aichat/service.rs:5604`/`7958`), zero nuovi.

## Report `stock_report` differito a fine turno — `defer_to_turn_end` (0.40.44)

Seconda correzione dal vivo dopo 0.40.43 (che ha introdotto la finestra
Markdown dedicata, ma solo con le 4 sezioni fattuali). L'utente ha
osservato dal vivo: (a) il pannello del canale mostrava per intero la
prosa dell'AI (narrativa + ipotesi) — troppo lunga per un pannello; (b)
quelle stesse due sezioni non finivano MAI nella finestra/documento
salvabile — un lettore che riapre il documento da Library perderebbe
l'analisi dell'AI per sempre. Richiesta: la finestra porta tutte e 6 le
sezioni; il pannello mostra solo un riassunto numerico ≤5 righe.

### Il vincolo architetturale

Al momento in cui `tools.dispatch("stock_report", ...)` ritorna (dentro il
loop di `ai_adapter::LlmAdapter::respond`), il testo dell'AI per le sezioni
5-6 NON esiste ancora: arriva in un'iterazione successiva dello STESSO
loop, dopo che il modello ha letto il `tool_result` (il riassunto
fattuale) e ha prodotto la sua risposta. L'apertura della finestra, prima
sincrona col dispatch, doveva spostarsi a "quando il turno produce il suo
testo finale, senza altri tool_use" — un momento che oggi non ha un nome
nel codice, va introdotto.

### `ChannelReport::defer_to_turn_end: bool` (`tool_client.rs`)

```rust
pub struct ChannelReport {
    pub title: String,
    pub markdown: String,
    pub defer_to_turn_end: bool,
}
```

`false` per nmap (`nmap_tool_client.rs::call_scan_tool` — il documento è
già completo al ritorno della chiamata, nessuna narrativa AI da attendere:
comportamento storico, invariato). `true` SEMPRE per i report Python
(`python_mcp_tool_client.rs::split_report`) — non è parametrizzato per
tool: oggi l'unico tool Python con un `report_title` è `stock_report`, e la
sua natura di "documento fattuale + narrativa AI in due tempi" è inerente
al canale, non una scelta per-tool da esporre ora (YAGNI).

### `DispatchOutcome.channel_summary: Option<String>`

Un riassunto breve e deterministico (mai scritto dall'AI), mostrato nel
pannello SUBITO dopo il dispatch — indipendentemente da come/se l'AI
risponderà poi. Popolato dal terzo campo del contratto JSON Python
(`PythonReportJson.channel_summary`), lato `report.py::build_channel_
summary(fx, idx)`: 5 righe, ogni numero già presente nelle sezioni
Fondamentale/Comparative del documento — mai un dato "nuovo" che solo il
pannello mostri.

### Il meccanismo in `ai_adapter.rs::LlmAdapter::respond`

Tre pezzi nuovi, tutti locali alla singola chiamata `respond` (nessuno
stato a vita più lunga):

```rust
struct PendingReportBuf { title: String, markdown: String }

let pending_report: std::sync::Mutex<Option<PendingReportBuf>> = std::sync::Mutex::new(None);
```

**Perché `std::sync::Mutex`, non `RefCell`.** Prima scelta naturale (un
solo task async, mai davvero concorrente) — ma `on_text` deve restare
`&mut dyn FnMut(&str) + Send` (il trait `ChatBackend::send_turn` lo
richiede, perché `respond` gira in un task spawnato da `ws.rs`).
`RefCell<T>` non è `Sync`, quindi `&RefCell<T>` non è `Send` — la closure
che lo cattura per riferimento non compila. Nessuna `.await` viene mai
fatta col lock preso: `std::sync::Mutex` (sincrono) basta, non serve
`tokio::sync::Mutex`.

**`on_text` sospende lo streaming quando c'è un report in sospeso:**

```rust
let mut on_text = |delta: &str| {
    if pending_report.lock().unwrap().is_some() {
        return;
    }
    let _ = tx.send(ServerMsg::Chunk { id: id.to_string(), content: delta.to_string() });
};
```

Il testo scartato qui NON va perso: alla fine del turno (nessun altro
tool_use), viene letto di nuovo da `turn.blocks` (già disponibile in quel
punto del loop) e fuso nel documento — un solo posto sa come estrarlo,
niente buffer duplicato dentro la closure.

**Stash invece di apertura immediata, nel ramo di dispatch dei tool:**

```rust
if let Some(report) = &outcome.report {
    if report.defer_to_turn_end {
        *pending_report.lock().unwrap() = Some(PendingReportBuf {
            title: report.title.clone(),
            markdown: report.markdown.clone(),
        });
    } else {
        emit!(ServerMsg::OpenWindow { title: report.title.clone(), kind: WindowKind::Markdown, content: report.markdown.clone() });
    }
}
```

**`flush_pending_report`, chiamata su ogni via d'uscita del loop tranne
l'annullamento:**

```rust
let flush_pending_report = |extra_text: Option<&str>| {
    if let Some(p) = pending_report.lock().unwrap().take() {
        if opts.allow_windows {
            let content = match extra_text {
                Some(t) if !t.trim().is_empty() => format!("{}\n\n{}", p.markdown, t.trim()),
                _ => p.markdown,
            };
            let _ = tx.send(ServerMsg::OpenWindow { title: p.title, kind: WindowKind::Markdown, content });
        }
    }
};
```

Chiamata (con `Some(&final_text)`, estratto da `blocks` filtrando
`Block::Text`) subito prima dell'`emit!(done())` del completamento
normale/`MaxTokens` — SEMPRE prima del `Done`, non dopo: un consumatore
deve poter vedere la finestra prima che il turno si consideri concluso.
Chiamata anche (con `None`, nessun testo AI da fondere) su: risposta
degenere (né testo né tool_use), errore del backend, rifiuto, cap
iterazioni raggiunto — un tool ha già prodotto un documento REALE in
un'iterazione precedente, un problema SUCCESSIVO nella conversazione con
l'AI non deve farlo sparire (regressione rispetto all'apertura immediata
di prima di questo cambiamento, che l'avrebbe già mostrato). **Non**
chiamata sul ramo di annullamento (Stop dell'utente): rispettarlo vale più
di mostrare comunque un documento che l'utente non ha chiesto di vedere
ora — la finestra resta chiusa, il report in sospeso si scarta col resto
della funzione.

### Test nuovi (`ai_adapter.rs`)

- `deferred_report_merges_ai_text_and_suppresses_channel_echo` — il caso
  d'oro: `channel_summary` compare nel pannello, il testo AI non ci
  compare mai, la finestra porta documento+testo fusi, e precede sempre il
  `Done` (verificato per indice di posizione nel vettore di `ServerMsg`).
- `deferred_report_still_opens_when_the_following_turn_errors` — un
  `BackendError::Network` nel turno successivo al tool non deve far
  sparire il documento fattuale.
- `deferred_report_is_dropped_on_cancel_before_final_text` — un fixture di
  test dedicato (`CancelingDeferredReportToolClient`) cancella il
  `CancellationToken` DENTRO il proprio `dispatch()`, non da un task
  separato: deterministico, nessuna corsa fra due task — al ritorno di
  `dispatch()` il token è già cancellato, quindi il controllo in testa
  alla PROSSIMA iterazione del loop lo vede di sicuro.

### `FINANCIAL_MARKETS_SYSTEM_PROMPT` (`external_channel.rs`)

Riscritto: la risposta dell'AI è ora Markdown vero (`## Narrativa di
trend`, `## Ipotesi di investimento` — intestazioni reali, come nel resto
del documento su cui si fondono), non più testo semplice per un pannello
che non renderizza Markdown. Il vincolo "poche righe" che valeva prima
(quando quel testo finiva nel pannello) è sparito da questo prompt — vive
ora in `report.py::build_channel_summary`, che è deterministico e non
dipende dalla disciplina del modello.

### Lato Python (`pytools/financial-markets/`)

- **`number_format.py`** (rinominato da un primo tentativo `numbers.py` —
  vedi sotto) — `format_number(n, decimals=0)` (separatori it-IT via
  `str.translate`, un solo passaggio: due `.replace()` sequenziali con un
  carattere-placeholder avevano introdotto un bug di codifica, vedi sotto)
  e `format_abbreviated(n)` (scala K/M/B/T dinamica in base alla
  grandezza — l'esempio "4786,46B" proposto dall'utente era esso stesso
  sbagliato: il valore reale è ~4,79 **trilioni**, non miliardi).
- **Incidente in scrittura — collisione di nome col modulo stdlib.** Il
  primo tentativo si chiamava `numbers.py`. Dato che la cartella dello
  script è su `sys.path` (necessario per gli import relativi fra i moduli
  del server MCP), quel file OSCURAVA il modulo stdlib `numbers` —
  `decimal.py` (importato da pytest stesso) fa `import numbers` per
  registrare le sue ABC, e riceveva il file locale invece dello stdlib,
  con un errore fuorviante ("`SyntaxError: source code string cannot
  contain null bytes`") che in realtà nascondeva un secondo bug indotto
  dalla prima versione del file: uno swap di separatori via due
  `.replace()` sequenziali con un carattere-placeholder (uno spazio) che,
  per qualche corruzione in scrittura del file mai isolata con certezza,
  è finito sul disco come un byte nullo. Risolto rinominando il modulo
  (`number_format.py`) E riscrivendo lo swap con `str.translate` (una
  mappa a un solo passaggio, senza placeholder, immune a questa classe di
  bug per costruzione).
- **`option_chain.py`** (nuovo) — `build_rows(options, current_price,
  window=10)` accoppia call/put per strike (un lato può essere `None` se
  quello strike ha solo call o solo put quotati) e ritaglia a `window`
  strike sopra e sotto quello più vicino a `current_price` (ATM, cercato
  per distanza minima — non un arrotondamento, lo strike ATM esatto può
  non esistere nella lista). `render_table(rows)` — Markdown 9 colonne,
  ordine confermato dall'utente: `Call IV | Call Bid | Call Ask | Call
  Ultimo | Strike | Put IV | Put Bid | Put Ask | Put Ultimo`.
- **`charts.py`** — `_dark_style()` (mplfinance `make_mpf_style`) allineato
  al tema scuro dell'app (`crates/ui/frontend/config.css`: sfondo
  `rgba(20,24,36,...)`, testo `#e8eefc`); `savefig(..., facecolor=_DARK_BG)`
  copre anche il margine attorno agli assi. Verificato con un test che
  decodifica il PNG risultante (Pillow, già nel venv) e legge un pixel
  d'angolo — il modulo docstring di `test_charts.py` dichiarava
  esplicitamente "non il contenuto dei pixel", corretto per riflettere che
  il tema scuro ORA è un requisito stabile, non un dettaglio implementativo.
- **`data_sources/__init__.py`** — `Fundamentals.current_price` (nuovo,
  serve sia alla riga "Prezzo attuale" del Fondamentale sia a centrare
  l'option chain sull'ATM); `OptionContract` guadagna `bid`/`ask`/
  `last_price` (prima solo `iv`).
- **`data_sources/yfinance_source.py`** — `fetch_options` ora prende SOLO
  la scadenza più vicina (`expiries[0]`, non le prime 3 come prima): una
  option chain vera si legge per singola scadenza, e più scadenze
  mescolate insieme avrebbero più righe per lo stesso strike senza un
  ordinamento naturale attorno all'ATM. `fetch_fundamentals` popola
  `current_price` da `info.get("currentPrice") or info.get
  ("regularMarketPrice")` (il primo manca per alcuni strumenti, es. certi
  ETF).
- **`report.py`** — `build_report(source, ticker)` ritorna ora `(full_
  markdown, channel_summary)` (prima solo la stringa) — `idx`
  (`fundamentals.index_comparison`) calcolato UNA volta e passato sia a
  `_comparative_section` sia a `build_channel_summary`, per non
  raddoppiare una fetch di rete reale (`fetch_index_series`) a ogni
  `stock_report`. `_comparative_section` riscritta con prosa esplicativa
  (cosa significano market cap/P/E/variazione-vs-indice) oltre ai numeri
  formattati. Tutti i chiamanti di test aggiornati per il nuovo tipo di
  ritorno (`markdown, _summary = report.build_report(...)`).
- **`server.py`** — `stock_report` spacchetta la tupla e la ricompone nel
  terzo campo JSON `channel_summary`.

### Verifica

`cargo test -p orchestrator --lib`: 770 passed / 0 failed / 7 ignored
(+13 rispetto alla baseline pre-modifica: 3 nuovi in `ai_adapter.rs`, il
resto assert aggiuntive su test esistenti). `cargo clippy -p orchestrator
--all-targets`: stessi 4 warning pre-esistenti, zero nuovi. Lato Python,
`pytest` nel venv di `financial-markets`: 59 passed / 0 failed (11 nuovi in
`test_number_format.py`, 9 in `test_option_chain.py`, 1 in `test_charts.py`
per il tema scuro, 4 nuovi/estesi in `test_report.py`).

## `pytools/` → `scripts/pytools/` — financial-markets segue la migrazione già avviata su `main` (0.40.45)

Al momento di preparare il merge di questo branch su `main`, il checkout di
`main` aveva già (non ancora committata) una migrazione in corso:
`pytools/python-ping/{requirements.txt,server.py}` e `pytools/README.md`
spostati a `scripts/pytools/`, `pytools/financial-markets/` (esistente
SOLO su questo branch, mai su `main`) ancora al vecchio path implicito.
Per restare coerenti, spostato anche quello: `git mv pytools/financial-
markets scripts/pytools/financial-markets` — il comando muove l'intera
cartella a livello filesystem (non file-per-file), quindi anche `venv/`
(ignorato da git, mai toccato da `git mv` di per sé) si sposta insieme al
resto senza bisogno di un passo separato.

**Cosa NON è cambiato:** il fallback runtime in `PythonMcpToolClient::
resolve()` — `parent.join("pytools")`, usato SOLO quando `LARE_PYTOOLS_DIR`
non è impostata (un binario deployato, mai il workflow di dev normale) —
resta `"pytools"`. È il nome della cartella copiata accanto al binario a
destinazione (`deploy_binary_only.ps1`), una convenzione di DEPLOY
indipendente da dove vive il codice sorgente nel repo — rinominarla
avrebbe richiesto aggiornare anche quello script, fuori dallo scope di
"spostare financial-markets per coerenza col resto del repo" (segnalato
all'utente come debito separato, non toccato qui).

**Cosa è cambiato:**
- `.gitignore`: `pytools/**/venv/` → `scripts/pytools/**/venv/` (idem
  `__pycache__/`, `tickers_us.json`) — la vecchia `pytools/` non contiene
  più nulla di tracciato dopo questa migrazione, quindi le vecchie righe
  sono state sostituite, non semplicemente affiancate.
- `crates/orchestrator/src/python_mcp_tool_client.rs`: i 3 test `#[ignore]`
  (`real_pyping_roundtrip_via_venv`, `real_search_ticker_multi_match_
  roundtrip_via_venv`, `real_stock_report_roundtrip_via_venv`) costruivano
  `repo_root.join("pytools")` per puntare alla radice reale nel repo di
  sviluppo — ora `repo_root.join("scripts").join("pytools")`. Ogni
  riferimento testuale a `pytools/README.md`/`pytools/financial-markets/`
  in messaggi d'errore, doc comment e motivazioni `#[ignore]` aggiornato a
  `scripts/pytools/...` per lo stesso file.
- `CLAUDE.md`: riga `LARE_PYTOOLS_DIR` e il puntatore a
  `scripts/pytools/README.md` in "Dove guardare".

**Verifica dal vivo dopo lo spostamento** (non solo `cargo test`, che non
tocca il filesystem reale del venv): `cargo test -p orchestrator --lib --
--ignored real_search_ticker_multi_match_roundtrip_via_venv` e
`real_stock_report_roundtrip_via_venv` — entrambi PASS contro il venv
rilocato e rete reale verso Yahoo Finance, la prova più diretta che la
nuova risoluzione del path funziona davvero, non solo a compile-time.

## `PythonToolSpec.defer_report_to_turn_end` — apertura finestra configurabile per-tool (0.40.46)

Task 1/4 del piano `list_stocks` (Docs/superpowers/plans/2026-07-22-
financial-markets-list-stocks.md, non ancora completo — questo task non
introduce ancora `list_stocks`, solo il seam che lo abiliterà nel Task 4).

**Il problema:** `split_report()` — la funzione pura che, quando
`report_title` è iniettata E l'output del tool è JSON `{summary,
report_markdown, channel_summary}`, produce un `ChannelReport` per la
finestra Markdown — hardcodava `defer_to_turn_end: true` incondizionatamente.
Corretto finché `stock_report` era l'UNICO tool con `report_title`: il suo
documento include narrativa scritta dall'AI (sezioni 5-6) che non esiste
ancora al momento del dispatch, quindi la finestra deve attendere il testo
finale del turno (`ai_adapter.rs` fonde quella narrativa in coda al
documento prima di aprirla). `list_stocks` (Task 4) produce invece un
documento interamente deterministico — nessuna narrativa AI da attendere —
e deve aprire la finestra SUBITO al dispatch, stesso pattern di
`NmapToolClient::call_scan_tool`. Non c'era modo di esprimere questa
differenza per-tool: il bool era hardcoded dentro `split_report()`, non
un parametro.

**Il fix:** aggiunto `PythonToolSpec.defer_report_to_turn_end: bool`.
Documentato come letto SOLO quando `report_title` è `Some` (se `report_title`
è `None`, come `pyping`/`search_ticker`, nessun report viene mai prodotto,
il valore del bool è irrilevante — non un `Option<bool>`: il tipo non ha
bisogno di rappresentare "non applicabile" perché quel caso è già escluso
strutturalmente da `report_title: None`). `split_report()` guadagna un
quarto parametro posizionale, `defer_to_turn_end: bool`, tra `report_title`
e `input`: `fn split_report(text: String, report_title: Option<fn(&
serde_json::Value) -> String>, defer_to_turn_end: bool, input:
&serde_json::Value) -> DispatchOutcome`. Il corpo non cambia struttura,
solo `ChannelReport { defer_to_turn_end: true, .. }` diventa `ChannelReport
{ defer_to_turn_end, .. }` (shorthand, il valore viene dal parametro).
L'unico call site (dentro `dispatch()`, il ramo `Ok(tool_result) =>`) passa
`spec.defer_report_to_turn_end`.

**Nessun cambio di comportamento osservabile per i tool esistenti**:
`stock_report` (`external_channel.rs`) porta `defer_report_to_turn_end:
true`, preservando l'apertura posposta esistente; `pyping`/`search_ticker`
portano `false`, ma non producono mai un report comunque (`report_title:
None`) quindi il valore non è mai letto in pratica — è lì solo perché il
campo è ora obbligatorio nello struct (niente `Default`, apposta: costringe
ogni nuova voce di registro a una scelta esplicita, non un default silente
che potrebbe sbagliare per un tool futuro con `report_title: Some(..)`).

Aggiornati anche i 3 `PythonToolSpec` literal `#[cfg(test)] #[ignore]`
real-network in `python_mcp_tool_client.rs` (stesso schema: `pyping`/
`search_ticker` → `false`, `stock_report` → `true`) e `fake_spec` nei test
unitari (hardcoded `false`, nessuno dei suoi 2 call site esercita un tool
con report).

**Test nuovo**: `defer_to_turn_end_false_produces_immediate_open_report`
in `split_report_tests` — il primo caso a esercitare `defer_to_turn_end:
false` su un report EFFETTIVAMENTE prodotto (a differenza dei test
`no_report_title_...`/`non_json_text_...`, che non producono report affatto
e quindi non avrebbero mai potuto rilevare una regressione sul valore del
flag). Il test `splits_into_summary_and_report_when_title_fn_present_and_
json_matches` ora passa esplicitamente `true` e verifica che si propaghi.

**File**: `crates/orchestrator/src/python_mcp_tool_client.rs`,
`crates/orchestrator/src/external_channel.rs`.

**Verifica**: `cargo build -p orchestrator` pulito. `cargo test -p
orchestrator --lib`: 771 passed, 0 failed, 7 ignored (invariato nel totale
rispetto a prima del task: il conteggio "771" include sia il nuovo test sia
i 3 preesistenti aggiornati nella stessa suite — nessuna regressione,
nessun test perso). `cargo clippy -p orchestrator --all-targets`: 0 warning
nei 2 file toccati da questo task (i warning residui nel crate sono
pre-esistenti, in `ai_adapter.rs`/`ws.rs`/`aichat/service.rs`, fuori
scope). Nessun test e2e dal vivo per questo task: non c'è ancora nessun
tool nuovo da esercitare (Task 4 registrerà `list_stocks` con
`defer_report_to_turn_end: false` e sarà quello il primo caso reale del
ramo `false`); la correttezza qui è garantita dal compilatore (campo
obbligatorio, ogni literal di produzione deve scegliere esplicitamente) e
dai 4 test di `split_report_tests`, che coprono la funzione pura
direttamente.

## `list_stocks` registrato nel canale `financial-markets` (0.40.47)

Task 4/4 (ultimo) del piano `list_stocks` (Docs/superpowers/plans/
2026-07-22-financial-markets-list-stocks.md). Chiude l'arco aperto dal
Task 1: il seam `defer_report_to_turn_end` esisteva già, mancava solo un
secondo tool reale che lo esercitasse col valore `false`.

**Cosa cambia**: terzo `PythonToolSpec` nella voce `"financial-markets"` di
`EXTERNAL_TOOL_CHANNELS`, subito dopo `stock_report` nello stesso
`vec![...]` passato a `PythonMcpToolClient::resolve`. `search_ticker` e
`stock_report` restano bit-per-bit invariati.

```rust
crate::python_mcp_tool_client::PythonToolSpec {
    def: ToolDef {
        name: "list_stocks".to_string(),
        description: "Elenco tabellare dei titoli conosciuti (Nome, Asset,
            Paese, Indice), filtrato per paese — solo 'USA' supportato oggi."
            .to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": { "country": { "type": "string" } },
            "required": []
        }),
    },
    report_title: Some(list_stocks_title),
    defer_report_to_turn_end: false,
}
```

`country` non è `required`: assente → il default lato Python è `"USA"`
(già cablato nel Task 3, `list_stocks(country: str = "USA")`), coerente col
titolo della finestra che ricade sullo stesso default lato Rust (vedi sotto).

**`list_stocks_title`** — nuova funzione, subito dopo `stock_report_title`,
stesso pattern (titolo derivato dall'INPUT della chiamata, non dalla
risposta — mirror di `format_nmap_invocation`):

```rust
fn list_stocks_title(input: &serde_json::Value) -> String {
    let country = input.get("country").and_then(|v| v.as_str()).unwrap_or("USA");
    format!("Financial Markets — Elenco titoli ({country})")
}
```

**Perché `defer_report_to_turn_end: false` (a differenza di `stock_report:
true`)**: il documento di `list_stocks` è una tabella pura — Nome, Asset,
Paese, Indice — generata interamente lato Python, senza alcuna narrativa
scritta dall'AI da attendere. Non c'è motivo di posporre l'apertura della
finestra a fine turno (che serve SOLO a fondere il testo dell'AI in coda al
documento, vedi Task 1/0.40.44): `list_stocks` apre SUBITO al dispatch,
stesso pattern di `nmap`/`NmapToolClient::call_scan_tool`. Questo è il
primo caso di produzione a esercitare il ramo `false` introdotto dal Task 1
(prima esisteva solo nei test unitari di `split_report_tests`).

**Il fallback paese-non-supportato non richiede codice Rust nuovo**: su un
paese diverso da `"USA"`, `list_stocks` (lato Python, Task 3) ritorna una
stringa semplice leggibile (es. "Solo 'USA' è supportato oggi"), NON il
JSON `{summary, report_markdown, channel_summary}` che `split_report()`
cerca. `split_report()` (esistente dal Task 1/design originale
`stock_report`) prova il parse JSON e, fallendo, ricade già sul percorso
"testo semplice, nessun report" — lo stesso path testato da
`non_json_text_with_report_title_returns_plain_output_no_report` in
`split_report_tests`. Il fallback funziona quindi per costruzione, senza
bisogno di un `match` esplicito su "paese supportato sì/no" lato Rust.

**`FINANCIAL_MARKETS_SYSTEM_PROMPT`** aggiornato per nominare tutti e tre i
tool (era "due strumenti", ora "tre strumenti") e per istruire l'AI: dopo
`list_stocks`, la finestra si apre subito (nessuna narrativa da attendere,
a differenza di `stock_report`) e la risposta dell'AI deve limitarsi a una
conferma breve del conteggio risultati, senza ripetere la tabella — stesso
principio già applicato a `stock_report` (non ripetere i dati fattuali che
arrivano nel pannello), esteso qui alla tabella completa.

**Test**: in `external_channel.rs`, `mod tests`:
- `financial_markets_system_prompt_names_all_three_tools` (sostituisce
  `financial_markets_system_prompt_names_both_tools`) — verifica che il
  prompt nomini tutti e tre i nomi tool, incluso `list_stocks`.
- `financial_markets_channel_exposes_three_tools` (nuovo) — chiama
  `resolve_channel_tools(Some("financial-markets"), EXTERNAL_TOOL_CHANNELS,
  ..)` per davvero. **Corretto lo stesso giorno in review finale
  whole-branch (0.40.48, vedi sotto)**: la prima versione faceva
  `.unwrap()` sul risultato — panic garantito su `cargo test -p
  orchestrator --lib` su qualunque macchina senza il venv
  `scripts/pytools/financial-markets` creato a mano, in contraddizione col
  precedente già esistente nello stesso file
  (`financial_markets_channel_tool_client_factory_does_not_panic`, che
  tollera `Ok`/`Err` per lo stesso identico motivo — "il venv assente in
  questo momento è uno stato valido"). Ora è un `match`: `Ok(...)` verifica
  `tools.tool_defs().len() == 3` più la presenza di `list_stocks`, `Err(_)`
  non asserisce nulla (nessun panic) — coerente col resto della suite del
  canale, mai una dipendenza ambientale nuova.

**Verifica**: RED confermato (`financial_markets_channel_exposes_three_
tools` falliva `left: 2, right: 3`; `financial_markets_system_prompt_names_
all_three_tools` falliva su `contains("list_stocks")`) prima
dell'implementazione. GREEN dopo: `cargo test -p orchestrator --lib
financial_markets -- --include-ignored` (con `LARE_PYTOOLS_DIR` impostato)
— 4 passed, 0 failed. Suite completa: `cargo build -p orchestrator` pulito;
`cargo test -p orchestrator --lib` — 772 passed, 0 failed, 7 ignored
(+1 rispetto a 0.40.46: il nuovo test `financial_markets_channel_exposes_
three_tools`). `cargo clippy -p orchestrator --all-targets`: 0 warning in
`external_channel.rs` (i warning residui nel crate sono pre-esistenti,
fuori scope, in `ws.rs`/`aichat/service.rs`). `cargo fmt --check`: alcune
righe del file non sono formattate secondo rustfmt di default (linee
lunghe con più campi inline) — pre-esistente in tutto il file/crate PRIMA
di questo task (verificato: la stessa condizione vale su righe non toccate
da questo diff), non una regressione introdotta qui.

Verifica dal vivo (manuale, non automatizzata) rimane il passo finale del
piano — vedi §"After all tasks: live verification" nel task brief e
`Docs/HANDOFF.md`.

## Fix di review finale whole-branch — `BRK-B`, normalizzazione `country`, test sort (0.40.48)

Tre difetti trovati dalla review finale sull'INTERO branch `list_stocks`
(dopo i 4 task, non durante nessuno di essi singolarmente) — il tipo di
problema visibile solo guardando il branch nel suo complesso, non un task
alla volta.

**1 — `SP500_TICKERS` conteneva `"BRK.B"`, la cache reale usa `"BRK-B"`.**
`tickers_us.json` è sourced verbatim dal `company_tickers.json` ufficiale
di SEC (`refresh_tickers.py`) — SEC spella OGNI azione a classi multiple
con un TRATTINO (`BRK-B`, `BF-B`, `MOG-A`), mai un punto: il suo file non
contiene alcun ticker puntato. La Task 2 (`index_membership.py`) aveva
scritto `"BRK.B"` nella lista `SP500_TICKERS` — probabile confusione con la
notazione NYSE ufficiale (che USA il punto), diversa dalla notazione SEC
usata dalla cache di questo progetto. Il proprio spot-check di Task 2 (Step
5 del piano) verificava `indices_for("BRK.B")` — la spelling SBAGLIATA,
quella scritta nel set stesso — che ovviamente faceva match con sé stessa,
mascherando il bug invece di scoprirlo: un guard che testa il lato
sbagliato del join non può mai fallire.

Risultato pratico: `build_rows` chiama `indices_for(t["ticker"])` con
`t["ticker"]` dalla cache reale (sempre `BRK-B`) — mai un match con
`"BRK.B"` nel set — quindi Berkshire Hathaway Class B, una delle prime 10
posizioni per capitalizzazione dell'S&P 500, mostrava sempre `-` nella
colonna Indice invece di `"S&P 500"`. Non è il compromesso "le liste
invecchiano" già accettato per questo modulo (Docs/superpowers/specs/
2026-07-24-financial-markets-list-stocks-design.md §4) — è uno sbaglio di
trascrizione presente fin dal primo commit, indipendente da qualunque
ribilanciamento futuro dell'indice.

Fix: `"BRK.B"` → `"BRK-B"` in `SP500_TICKERS`
(`scripts/pytools/financial-markets/index_membership.py`). Nuovo test in
`test_index_membership.py` che verifica ESPLICITAMENTE `indices_for
("BRK-B") == "S&P 500"` E `indices_for("BRK.B") == "-"` — quest'ultima
assert è il guard che sarebbe servito dall'inizio: se qualcuno reintroduce
la spelling col punto, questo test la scopre subito, invece di continuare
a testare (con successo) il lato sbagliato del join.

**2 — `country` non normalizzato per la visualizzazione.**
`is_country_supported` valida case-insensitive
(`country.upper() in SUPPORTED_COUNTRIES`), ma il valore grezzo (es.
`"usa"` se l'AI lo passa in minuscolo) finiva non normalizzato in tre
punti: la colonna Paese di ogni riga (`stock_list.py::build_rows`), il
titolo del documento (`server.py`'s `full_markdown`), e il titolo della
finestra (`external_channel.rs::list_stocks_title`). Corretto con
`.upper()`/`.to_uppercase()` in tutti e tre — un `list_stocks
(country="usa")` mostra ora "USA" ovunque, non "usa".

**3 — `test_build_rows_sorts_alphabetically_by_name` non discriminava
l'ordinamento per nome da quello per ticker.** I due ticker di prova
(`ZZZ`/"Zeta Corp", `AAA`/"Alpha Corp") avevano ordine-ticker e
ordine-nome coincidenti — un regresso a "ordina per ticker" (bug plausibile:
`build_rows` accetta sia `ticker` sia `name` nello stesso dict di input)
sarebbe passato comunque. Sostituiti con `AAA`/"Zeta Corp" e `ZZZ`/"Alpha
Corp" (ordini deliberatamente divergenti) — l'implementazione
(`sorted(rows, key=lambda r: r["name"])`, mai cambiata) continua a passare,
ma ora il test fallirebbe davvero su quel regresso specifico.

**File**: `scripts/pytools/financial-markets/index_membership.py`,
`test_index_membership.py`, `stock_list.py`, `test_stock_list.py`,
`server.py`; `crates/orchestrator/src/external_channel.rs`
(`list_stocks_title`).

**Verifica**: `pytest` (venv `financial-markets`): 80 passed, 0 failed
(+2 rispetto a 0.40.47: il nuovo guard `BRK-B`/`BRK.B` e il test country
normalizzato). `cargo build -p orchestrator` pulito; `cargo test -p
orchestrator --lib`: 774 passed, 0 failed, 7 ignored.

## `screen_stocks` registrato nel canale `financial-markets`, timeout 120→180 (0.40.49)

Task 6/6 (ultimo) del piano `screen_stocks` (Docs/superpowers/plans/
2026-07-27-financial-markets-screen-stocks.md; design in
Docs/superpowers/specs/2026-07-26-financial-markets-screen-stocks-design.md).
Chiude l'arco: lato Python (Task 1-5, già mergiati) `fetch_screening_
snapshot`, `fundamentals_cache.py`, `discoveries.py`, `screening.py` e il
tool `screen_stocks(top: int = 25) -> str` in `server.py` erano già cablati
— mancava solo l'esposizione lato Rust all'AI del canale.

**Cosa cambia**: quarto `PythonToolSpec` nella voce `"financial-markets"`
di `EXTERNAL_TOOL_CHANNELS`, subito dopo `list_stocks` nello stesso
`vec![...]` passato a `PythonMcpToolClient::resolve`. `search_ticker`,
`stock_report` e `list_stocks` restano bit-per-bit invariati.

```rust
crate::python_mcp_tool_client::PythonToolSpec {
    def: ToolDef {
        name: "screen_stocks".to_string(),
        description: "Screening 'potenziale inespresso': selezione di
            aziende consumer USA con score quantitativo, escludendo quelle
            già mostrate di recente (scoperte sempre nuove).".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": { "top": { "type": "integer" } },
            "required": []
        }),
    },
    report_title: Some(screen_stocks_title),
    defer_report_to_turn_end: true,
}
```

`top` non è `required`: assente → il default lato Python è `25` (già
cablato in `server.py::screen_stocks`).

**`screen_stocks_title`** — nuova funzione, subito dopo `list_stocks_
title`, ma con uno schema DIVERSO dalle due precedenti: `stock_report_
title`/`list_stocks_title` derivano il titolo da un parametro della
chiamata (`ticker`/`country`); qui il titolo è FISSO, `top` non è un
identificativo utile per il titolo (è solo "quanti", non "quali" o "di
chi") — la selezione stessa non ha un nome breve rappresentabile in un
titolo di finestra. La firma resta comunque `fn(&serde_json::Value) ->
String` (input ignorato con `_`) per restare uniforme al resto del
registro, che passa sempre una funzione con quella firma a `report_title`:

```rust
fn screen_stocks_title(_input: &serde_json::Value) -> String {
    "Financial Markets — Screening potenziale".to_string()
}
```

**Perché `defer_report_to_turn_end: true` (come `stock_report`, non come
`list_stocks`)**: il documento di `screen_stocks` include la sezione '##
Giudizio uso di massa' scritta dall'AI (per ogni titolo della selezione:
archetipo d'uso, verdetto Sì/No/Parziale sull'uso di massa reale,
motivazione) — esattamente come `stock_report` attende '## Narrativa di
trend'/'## Ipotesi di investimento'. La finestra non può aprirsi al
dispatch come `list_stocks` (documento puramente deterministico, nessuna
narrativa da attendere): deve posporre l'apertura a fine turno per fondere
il testo dell'AI in coda al documento (`ChannelReport::defer_to_turn_end`,
Task 1/0.40.44 del piano `list_stocks`).

**Timeout canale 120 → 180s**: `call_timeout_secs` è un parametro
PER-CANALE (terzo argomento posizionale di `PythonMcpToolClient::resolve`,
condiviso da tutti i `PythonToolSpec` del `vec![...]`), non per-tool.
`screen_stocks` fetcha fino a 60 ticker via `yfinance` (~1-2s l'uno) in una
singola chiamata — 120s (il valore ereditato da quando il canale aveva solo
`search_ticker`/`stock_report`) era al limite per questo nuovo tool più
pesante. Innocuo per gli altri tre: nessuno dei tre si avvicina a quella
soglia, quindi alzarla non introduce alcun comportamento osservabile
diverso per loro (solo un timeout più permissivo che non scatterebbe mai
comunque).

**`FINANCIAL_MARKETS_SYSTEM_PROMPT`** aggiornato per nominare tutti e
quattro i tool (era "tre strumenti", ora "quattro") e per istruire l'AI sul
comportamento di `screen_stocks`: la finestra si apre a fine turno (come
`stock_report`, diversamente da `list_stocks`) e la risposta dell'AI è
ESCLUSIVAMENTE la sezione '## Giudizio uso di massa', con lo schema
per-titolo (archetipo/verdetto/motivazione) e l'avviso di chiusura
(giudizio qualitativo, selezione non una raccomandazione operativa) —
stesso principio già applicato alle sezioni di `stock_report` (risposta
testuale vincolata a un formato Markdown fisso che continua il documento).
Il doc comment sopra la costante aggiornato da "tre tool" a "quattro tool",
con riferimento al nuovo design spec.

**Test**: in `external_channel.rs`, `mod tests`:
- `financial_markets_system_prompt_names_all_four_tools` (sostituisce
  `financial_markets_system_prompt_names_all_three_tools`) — verifica che
  il prompt nomini tutti e quattro i nomi tool, incluso `screen_stocks`.
- `financial_markets_channel_exposes_four_tools` (rinomina di
  `financial_markets_channel_exposes_three_tools`, stesso `match`
  venv-tolerante ereditato dal fix 0.40.48 — MAI un `.unwrap()`) — conteggio
  `tool_defs().len() == 4` più la presenza di `screen_stocks`, `Err(_)` non
  asserisce nulla (nessun panic su una macchina senza venv).
- `screen_stocks_title_is_fixed` (nuovo) — verifica che il titolo sia
  identico con `{"top": 10}` e con `{}` (input completamente ignorato).

**Verifica**: RED confermato — `cargo test -p orchestrator --lib
financial_markets` falliva in compilazione (`screen_stocks_title` non
trovata; l'help del compilatore suggeriva erroneamente `list_stocks_title`,
prova che la funzione non esisteva ancora) prima dell'implementazione.
GREEN dopo: `cargo build -p orchestrator` pulito; `cargo test -p
orchestrator --lib` — 775 passed, 0 failed, 7 ignored (+1 rispetto a
0.40.48: il nuovo test `screen_stocks_title_is_fixed`), **con
`LARE_PYTOOLS_DIR` impostata O NO** — verificato dal vivo in entrambi i
casi. Con la var impostata sul path reale (`scripts/pytools`) e il venv
`financial-markets` presente sulla macchina, i quattro test
`financial_markets_*` sono stati eseguiti isolatamente
(`cargo test -p orchestrator --lib financial_markets`) e confermano che il
ramo `Ok` di `financial_markets_channel_exposes_four_tools` è
effettivamente esercitato (non solo tollerato in `Err`): `tool_defs()
.len() == 4` verificato per davvero contro il client Python reale. `cargo
clippy -p orchestrator --all-targets`: 0 warning in `external_channel.rs`
(i warning residui nel crate sono pre-esistenti, fuori scope, in
`aichat/service.rs`). `cargo fmt --check`: la stessa non-conformità
pre-esistente del file (righe lunghe con più campi inline, già segnalata
in 0.40.47/0.40.48) resta, +1 blocco di diff per la nuova entry — non una
regressione introdotta da questo task (verificato confrontando il conteggio
dei blocchi di diff prima/dopo via `git stash`).

Verifica dal vivo (manuale, non automatizzata) del comportamento end-to-end
(run reale del canale `/markets`, crescita della cache, esclusione dei
ticker già scoperti) rimane il passo finale del piano — vedi §"After all
tasks: live verification" nel task brief e `Docs/HANDOFF.md`.

## `PythonReportJson.title_suffix` — timestamp nel titolo finestra (0.40.50)

Refinement post-verifica dal vivo di `screen_stocks` (2026-07-27, terza
richiesta della stessa giornata dopo colonna Ultimo/legenda e timestamp
nell'H1): data/ora anche nel titolo della FINESTRA, che è il nome con cui
il documento viene salvato in Library — senza, due screening salvati in
giorni diversi sarebbero indistinguibili nell'archivio.

**Perché non lato Rust**: il titolo finestra nasce da `title_fn(input)` del
registro (`external_channel.rs::screen_stocks_title`), che riceve SOLO i
parametri della chiamata — nessun orologio. `std` non formatta date locali
(niente timezone/DST senza chrono), e aggiungere chrono per un titolo
sarebbe sproporzionato. Il tool Python invece il timestamp ce l'ha già
(`server.py` lo calcola per l'H1 del documento).

**Il meccanismo**: quarto campo OPZIONALE nel contratto JSON report —
`title_suffix: Option<String>` con `#[serde(default)]`. `split_report()`
lo appende al risultato della title_fn quando presente:

```rust
let mut title = title_fn(input);
if let Some(suffix) = &parsed.title_suffix {
    title.push_str(suffix);
}
```

Il titolo BASE resta del registro Rust (autoritativo sul naming — il tool
non può rinominare la finestra, solo appendere la parte dinamica che solo
lui conosce). Campo assente → titolo invariato: retrocompatibilità totale
per `stock_report`/`list_stocks` e qualunque tool futuro che non lo manda.
Lato Python, `server.py::screen_stocks` calcola il timestamp UNA volta e
lo usa sia per l'H1 (`generated_at`) sia per `title_suffix` — documento e
finestra non possono divergere a cavallo del minuto.

**Test**: 2 nuovi in `split_report_tests` — `title_suffix_from_the_tool_
json_is_appended_to_the_window_title` e `missing_title_suffix_keeps_the_
title_fn_result_unchanged` (RED confermato prima dell'implementazione: il
primo falliva con il titolo senza suffisso). `cargo test -p orchestrator
--lib`: 777 passed, 0 failed, 7 ignored. `pytest` financial-markets: 125
passed.

## Task 5: `ChatMsg` wire additions — Blocco note (0.40.58)

Quattro nuove varianti sul wire peer↔peer (Contratto N, `src/aichat/wire.rs`):

- `NotesDigest { entries: Vec<(String, NoteDigest)> }` — impronte di tutte le
  note note al mittente, senza corpo.
- `NotesDigestReply { want: Vec<String>, push: Vec<Note> }` — risposta: `want`
  elenca id di cui serve il contenuto pieno; `push` contiene note intere che il
  mittente crede mancanti o più vecchie dal lato ricevente.
- `NotesData { notes: Vec<Note> }` — fulfillment di un `want` da una
  `NotesDigestReply` precedente.
- `NoteUpdated { note: Note }` — steady-state (creazione/modifica/cancellazione
  con link vivo) E fan-out post-riconciliazione verso i client già collegati.

**Perché isolato dalla service**: i messaggi viaggiano sul wire prima che la
logica di `AiChatService` (Task 7+) li usi. Il contratto (serializzazione,
serde tag snake_case, `Eq` trait per round-trip) è stabile a prescindere da
chi li consuma.

**Tag snake_case**: `"notes_digest"`, `"notes_digest_reply"`, `"notes_data"`,
`"note_updated"` (documenti nel doc-comment della `enum ChatMsg`).

**Dipendenze**: consuma `crate::notes::{Note, digest::NoteDigest}` (Task 2-3).

**Placeholder in service.rs**: `match &msg` in `ServiceEvent::PeerMsg` acquisisce
4 nuovi arm no-op per non rompere l'esaustività del match fino a Task 7+. Commento
segnala che la logica reale arriverà nei task successivi.

**Test TDD (3 nuovi, verificati RED↔GREEN)**:
- `chatmsg_notes_digest_roundtrip_and_tag` — serializzazione/deserializzazione
  e verifica del tag.
- `chatmsg_notes_digest_reply_roundtrip_and_tag` — idem per reply.
- `chatmsg_notes_data_and_note_updated_roundtrip_and_tag` — idem per le due
  varianti restanti in un unico test.

`cargo test -p orchestrator --lib aichat::wire`: 14 passed (include i 3 nuovi).
`cargo test -p orchestrator`: 810 passed (full suite, nessun errore di `Eq`
trait-bound).

## Task 7: `AiChatService` gestisce creazione/modifica/cancellazione locale — Blocco note (0.40.60)

`AiChatService` (`src/aichat/service.rs`) guadagna un `NotesStore` interno
(campo `notes`) e un contatore `next_note_id: u64` (stesso schema di
`next_share_id`/`fresh_share_id`, nessuna dipendenza `uuid`).

**Nuovi `ServiceEvent`** (dopo `ShareContentFailedUi`, il "Contratto locale"
UI→attore):
- `NoteCreateRequested { title, text }`
- `NoteEditRequested { id, text }` — sostituisce SOLO il segmento di
  QUESTA macchina (mai l'intera nota).
- `NoteEditTitleRequested { id, title }` — last-write-wins, separato dal corpo.
- `NoteDeleteRequested { id }` — imposta il tombstone (`deleted: true`, mai
  ripristinato da `merge_note`).

**Nuovo `Effect::SaveNotes`** (dopo `StartShareExpiryTimer`): nessun payload —
`perform` clona `self.notes` (sincrono, economico — `NotesStore` ora deriva
`Clone`) e spawna un task che chiama `snapshot.save()` sotto un nuovo lock
dedicato, `notes_write_lock: Arc<Mutex<()>>` (campo gemello di
`memory_write_lock`, stesso motivo: previene che due `SaveNotes` ravvicinati
interlaccino la scrittura dello stesso file).

**Fix di review** (prima versione di questo arm, corretta prima del commit
finale): la prima implementazione chiamava `self.notes.save()`
SINCRONAMENTE dentro `perform` — che gira INLINE nel `select!` principale di
`run`, lo stesso loop che gestisce keepalive/elezione/relay per l'attore.
Uno `std::fs::write` lento (antivirus/indicizzazione Windows) avrebbe
bloccato l'intero attore. Ora `Effect::SaveNotes` è un vero gemello di
`Effect::PersistMemory`, non solo a parole: solo il clone (economico) resta
sincrono, il write vero è nel task spawnato.

**Helper privati aggiunti** (vicino a `reachable_peer_labels`):
- `next_note_id(&mut self) -> String`
- `to_note_view(note: &Note) -> protocol::NoteView` (corpo già renderizzato
  via `notes::digest::render_body`)
- `broadcast_note_to_links(&self, note: &Note) -> Vec<Effect>` — un
  `Effect::SendToPeer(id, ChatMsg::NoteUpdated { note })` per ogni voce di
  `self.links` (client: unico link verso il server; server: già il fan-out
  verso tutti i client).

**Seam per l'orologio**: nuova funzione di modulo `now_ms()` + campo
`wallclock_ms_for_test: Option<u64>` (sempre `None` in produzione), con un
setter `#[cfg(test)]` `set_wallclock_ms_for_test(&mut self, ms: u64)` nel
blocco `impl AiChatService` di test già esistente (accanto a `role_for_test`).
Stesso principio del "clock iniettato" di `PeerTable::observe` in
`discovery.rs`, applicato qui perché a differenza di quel caso il chiamante
(`ClientMsg::NoteCreate` ecc.) non porta un timestamp nel messaggio: l'attore
deve generarlo da sé al momento dell'esecuzione. Il setter serve al test del
tie-break LWW su `title_touched` sotto: senza un orologio iniettabile, due
`handle_event` ravvicinati nello stesso millisecondo produrrebbero lo stesso
timestamp e il test sarebbe non deterministico.

**`AiChatService::new` cambia firma**: 5° parametro `notes: NotesStore`.
`new_for_test`/`new_for_test_with_participation`/`new_for_test_with_autoparticipate`
passano `NotesStore::empty_in_memory()` (Task 4). Tutti i call site del crate
aggiornati nello stesso commit: `main.rs` (produzione — carica
`notes.json` da `app_data_dir` con `NotesStore::load_or_generate`, anticipando
quello che era pianificato come Task 15) e i due test di integrazione
`tests/aichat_relay_loopback.rs`/`tests/aichat_share_loopback.rs` (compilati,
anche se `#[ignore]`, da `cargo test -p orchestrator`).

**Test TDD (5 nuovi, RED confermato — variant/campo inesistenti prima
dell'implementazione; `NoteEditTitleRequested` verificato a parte con un
break-and-restore mirato dell'arm, per non lasciare quella logica priva di
copertura fallita-prima)**:
- `note_create_requested_upserts_locally_pushes_to_ui_and_saves`
- `note_edit_requested_replaces_own_segment_not_whole_note`
- `note_delete_requested_sets_tombstone`
- `note_edit_title_requested_changes_title_without_losing_the_body`
- `note_create_requested_sends_to_every_currently_linked_peer`

`cargo test -p orchestrator aichat::service::tests::note_`: 6 passed (i 5 nuovi
+ un test preesistente il cui nome combacia col filtro per substring).
`cargo build -p orchestrator`: OK. `cargo test -p orchestrator`: 815 passed nel
lib, 0 failed, full suite (incluso `main.rs` e i due test di loopback).

## `SENSITIVE_TOOLS` guadagna `run_routine` — repository di routine PowerShell (0.40.71)

Docs-only (Task 4 del piano `Docs/superpowers/plans/2026-08-03-routines-repository.md`;
il cambio di codice è nel task precedente, Task 3). Design:
`Docs/superpowers/specs/2026-08-03-routines-repository-design.md`. `SENSITIVE_TOOLS`
(`local_confirm.rs`) guadagna una **seconda voce reale**, dopo i cinque tool
`nmap_*` (v. sezione "Wiring dei 3 nuovi tool scan nmap" sopra): `run_routine`
(mcp-server 0.6.0, `crates/mcp-server/src/routines.rs`). Stesso principio di
rischio già alla base della lista, declinato diversamente: `run_in_session` è
autonomo in locale perché il suo testo completo è già il chunk di trasparenza
mostrato all'utente, ma una routine si invoca per **nome** — il corpo dello
script non è ri-mostrato a ogni esecuzione, quindi resta un'azione non
pienamente visibile dal solo nome, la stessa ragione per cui gli scan `nmap_*`
sono gateizzati. `search_routines` (ricerca sola-lettura nell'indice) resta
FUORI da `SENSITIVE_TOOLS` e autonomo, come `local_network_info`/`traceroute`
prima di lui: nessun side-effect, nessun target remoto, nessun prompt di
conferma giustificato. Nessuna logica nuova in `local_confirm.rs`: solo
l'estensione della costante e del test `should_gate_is_true_for_sensitive_tools_false_for_others`.

## Task 5: wiring `search_routines`/`run_routine` nel loop AI (0.40.72)

**Il problema** (Critical trovato nella review finale whole-branch, non un errore degli
implementer dei Task 1-4): nessun task del piano `2026-08-03-routines-repository.md` toccava
`agent.rs`/`tool_client.rs` — lo spec di design salta da §4 (tool MCP lato mcp-server) a §5
(gate) senza un passo di esposizione lato orchestrator. Risultato: `agent::tool_defs()`
tornava ancora solo i 3 tool storici, `agent::dispatch_tool` rifiutava
`search_routines`/`run_routine` come "sconosciuto", e nessun `ToolClient` sapeva chiamarli —
i tool esistevano in `mcp-server` ma erano irraggiungibili dal modello.

**`tool_client.rs`** — trait `ToolClient` guadagna due metodi obbligatori (nessun default,
vincolo di object-safety, vedi doc-comment del trait):
```rust
async fn search_routines(&self, query: Option<&str>) -> SearchRoutinesResult;
async fn run_routine(&self, name: &str, args: Option<&str>) -> RunRoutineResult;
```
`SearchRoutinesResult { results: Vec<RoutineSummary>, error: Option<String> }` — a differenza
di `CommandResult`/`OpenResult`, la forma JSON di mcp-server (`{results: [...]}`) non porta un
canale di errore (è sola lettura, "zero risultati" è normale); `error` qui copre SOLO i
fallimenti di trasporto (spawn/handshake/parse), mai "nessuna routine salvata".
`RunRoutineResult` mirror esatto di mcp-server's `RunRoutineOutput` (`ok`, `message`,
`stdout`, `stderr`, `exit_code`, `cwd`).

`McpToolClient::search_routines`/`run_routine` replicano verbatim il pattern lazy-connect già
usato da `run_in_session`/`open_target` (spawn+handshake se `peer` è `None`, poi
`call_tool`) — duplicazione INTENZIONALE, stessa convenzione del resto del file, non
refactored in un helper condiviso. `run_routine` ha lo stesso timeout di sicurezza 90s di
`run_in_session`. `FakeToolClient` ritorna esiti canned (nessuna routine per `search_routines`;
i campi canned del costruttore per `run_routine`, stesso principio di `run_in_session`).
`FixtureChannelToolClient` li rifiuta con "non disponibile su questo canale".

**`agent.rs`** — due nuove `ToolDef` in `tool_defs()` (dopo `show_markdown`), due arm in
`dispatch_tool` (`search_routines` formatta un elenco leggibile o "Nessuna routine trovata.";
`run_routine` combina stdout+stderr come `run_in_session`, `is_error` da `exit_code != 0`
quando `ok=true`, altrimenti l'errore di `message`), due arm in `display_invocation`. Quest'ultimo
punto è vincolante per la sicurezza, non cosmetico: `run_routine` è in `SENSITIVE_TOOLS`
(0.40.71), e `display_invocation` è ESATTAMENTE il testo del banner di conferma — senza un
arm dedicato ricadrebbe sul fallback generico `[tool run_routine]`, che non dice quale routine
né con quali argomenti, vanificando lo scopo del gate. `SYSTEM_PROMPT` aggiornato: elenca i
due nuovi tool e dice all'AI di preferire `search_routines`+`run_routine` a riscrivere un
comando da zero quando la richiesta somiglia a qualcosa già fatto.

**Gap del piano scoperto IN CORSO d'opera** (oltre alla review finale che ha originato questo
task): il piano di dettaglio nominava solo 3 `ToolClient` da estendere
(`FakeToolClient`/`FixtureChannelToolClient`/`McpToolClient`). Essendo i due nuovi metodi
`async fn` obbligatori sul trait, la prima `cargo build -p orchestrator` dopo l'estensione del
trait ha rivelato altri **7 implementor** nel workspace, tutti da estendere per compilare:

- **`cwd_tracking.rs::CwdTrackingToolClient`** — CRITICO, non cosmetico. È il decorator che
  `main.rs:201` avvolge attorno al vero `McpToolClient` per costruire il `ToolClient` di
  produzione (cursore/Telegram) — `Arc::new(CwdTrackingToolClient::new(raw_tools, cwd_state))`.
  Uno stub di rifiuto qui avrebbe riprodotto ESATTAMENTE il bug che questo task esiste per
  risolvere, un livello più sopra: i tool avrebbero continuato a essere irraggiungibili in
  produzione anche dopo aver "sistemato" `agent.rs`/`tool_client.rs`. Fix: delega pura per
  `search_routines` (come `open_target`, nessun effetto su cwd); `run_routine` delega E
  aggiorna `cwd_state` se `result.cwd` non è vuoto, stesso principio di `run_in_session`
  sopra di lui nel file — verificato che `mcp-server`'s `run_routine` (`main.rs`) popola
  `cwd: result.cwd` dal vero `Session::run`, non un placeholder vuoto, quindi il campo non è
  dead code: uno script di routine con `Set-Location` cambia la cwd reale della shell
  condivisa esattamente come un comando raw, e senza questo tracking `/find` e la UI
  leggerebbero una cwd stantia dopo l'esecuzione. Nuovo test
  `dispatch_run_routine_updates_cwd_state`, mirror di `dispatch_run_in_session_updates_cwd_state`
  già esistente (stesso principio: il `dispatch()` del decorator deve re-entrare
  `agent::dispatch_tool` con `self`, non `self.inner`, altrimenti la chiamata AI-path
  bypasserebbe silenziosamente il tracking — bug storico già documentato sopra per
  `run_in_session`, commit 39370d4).
- **`external_channel.rs::EmptyToolClient`**, **`nmap_tool_client.rs::NmapToolClient`**,
  **`python_mcp_tool_client.rs::PythonMcpToolClient`** — stub "non disponibile su questo
  canale" per entrambi i metodi, identico al pattern già in uso su questi 3 client per
  `run_in_session`/`open_target`. Difesa in profondità, non il vero gate di isolamento: la
  `tool_defs()` di questi canali non include mai `search_routines`/`run_routine`, quindi il
  modello non li vede nel menu in primo luogo — lo stub copre solo il caso (già testato per
  `run_in_session`) di un nome allucinato fuori menu.
- **`ai_adapter.rs`** — 3 `ToolClient` locali a singole funzioni di test
  (`ReportProducingToolClient`, `DeferredReportToolClient`, `CancelingDeferredReportToolClient`):
  stessi stub "non disponibile" dei loro `run_in_session`/`open_target` esistenti.

**Conteggi tool aggiornati** (base storica 3 → 5 dopo l'aggiunta): il piano di dettaglio
nominava 5 asserzioni da correggere; la stessa aritmetica (3→5 sulla lista base) ha
conseguenze su altre 4 non nominate, trovate con la prima `cargo test -p orchestrator`
post-Parte-C — tutte E SOLO conteggi, nessuna assert diversa ha cambiato esito:
- `agent.rs`: `tools_for_default_has_five_custom` (era `_three_`, 3→5, nominato dal piano),
  `tools_for_no_windows_excludes_show_markdown` (2→4, nominato dal piano),
  `tools_for_web_search_adds_two_server_tools` (5→7, NON nominato dal piano).
- `tool_client.rs`: `default_tool_defs_matches_historical_five` (era `_three`, 3→5); nuovo
  test `fixture_channel_dispatch_rejects_run_routine_as_unknown` (dimostra end-to-end ciò che
  lo stub di `FixtureChannelToolClient` rende vero strutturalmente).
- `ai_adapter.rs`: linea ~797 (3→5, nominata dal piano); `nowin_mode_excludes_show_markdown_tool`
  (2→4, NON nominato) e `web_search_mode_includes_server_tools` (5→7, NON nominato).
- `core.rs`: linea ~1437 "NL normale" (3→5, nominata dal piano); il ramo `/nowin` nella STESSA
  funzione di test (2→4, NON nominato dal piano pur essendo 20 righe sopra quella nominata).

`external_channel.rs`: commento di modulo aggiornato (3→5 tool storici sostituiti dal
`tool_defs()` di un canale).

**Verifica**: `cargo build` (intero workspace) pulita. `cargo test -p mcp-server -p
orchestrator`: 836 passed nel lib di orchestrator (0 failed), nessuna regressione fuori dai
conteggi sopra. `cargo clippy -p orchestrator --all-targets`: nessun nuovo warning nei file
toccati da questo task (i pochi residui sono preesistenti in `aichat/config.rs`/
`aichat/service.rs`, fuori scope).

Vedi anche `crates/mcp-server/IMPLEMENTATION.md` per le due correzioni di accuratezza
documentale (claim di streaming inesistente) e il minor `resolve_root` bundlati nello stesso
commit (Parte D del task).

## Trasparenza `run_routine`: label arricchita con la description (0.40.75)

`run_routine` è in `SENSITIVE_TOOLS` (0.40.71) — la sua label è ESATTAMENTE il testo del banner
di conferma (vincolo di sicurezza, non cosmetico, vedi sopra), ma fino a questa versione era solo
`routine: <name> <args>` (via `agent::display_invocation`): l'utente approva senza sapere cosa fa
la routine. Nuova `ai_adapter::build_label(name, input, tools, format_invocation)` (privata):
`format_invocation` (override canale esterno) vince sempre come oggi; per `run_routine` fa un
lookup via `tools.search_routines(Some(name))` e cerca un match ESATTO case-insensitive sul nome
fra i risultati (`search_routines` è per sottostringa — un nome potrebbe esserne sottostringa di
un altro, il primo risultato non basta) — trovato ⇒ `"Uso la routine disponibile {name}
({description})"`; non trovato (routine sparita fra scelta AI ed esecuzione) o altro tool ⇒
fallback su `agent::display_invocation`, invariato. Sostituisce le due chiamate dirette a
`display_invocation` in `respond` (banner di conferma `gated_labels` + label per-tool del Chunk
post-esecuzione) — entrambe ora passano da `build_label`, quindi async (`gated_labels` da
`.map().collect()` a un for-loop con `.await`). Nessun cambio di wire format/protocollo, nessun
tool nuovo, `mcp-server` non toccato. Test: `run_routine_label_includes_description_from_index`
(`ai_adapter.rs`), fake `ToolClient` locale con `search_routines` che risponde con un
`RoutineSummary` reale.

## `PythonReportJson.title` (0.41.4)

`split_report` (`python_mcp_tool_client.rs`) ora legge un campo opzionale
`title` dal JSON del tool Python: se presente, sostituisce il risultato di
`report_title(input)` come titolo BASE della finestra (`title_suffix` si
applica comunque dopo, invariato). Motivazione: `run_screener` (dispatcher
generico per N screener, prossima voce di questa serie) non può avere una
title-fn Rust per-screener come `screen_stocks_title` — solo il registry
Python (`screeners/__init__.py`) sa il titolo dello screener appena
eseguito. `stock_report`/`list_stocks` non mandano ancora questo campo →
comportamento invariato per loro (fallback automatico sulla title-fn).

Effetto collaterale: `telegram/channel.rs::format_response` fa match
esaustivo su `ServerMsg` — la nuova variante `OpenScreenerPicker` (protocol
0.15.2) ha richiesto un braccio no-op, stesso trattamento delle altre
finestre UI-only già presenti lì (`OpenPluginWindow`, `RoutineSavePreview`,
…): Telegram non ha una superficie di selezione a lista.

## `list_screeners` → `OpenScreenerPicker` (0.41.5)

`ai_adapter.rs`, branch generico del loop tool-use: se `name ==
"list_screeners"` e `outcome.output` parsa come `{"summary": String,
"items": Vec<ScreenerListItem>}`, l'adapter (a) emette
`ServerMsg::OpenScreenerPicker{items}` (rispettando `allow_windows`, come
`report`/`show_markdown`) e (b) usa `summary` — non il JSON grezzo — come
contenuto del `Block::ToolResult` restituito all'AI. Nessun campo nuovo su
`PythonToolSpec`/`DispatchOutcome` (condivisi da 7 file): il routing è un
check sul NOME del tool, esattamente come `show_markdown` è già gestito in
questo stesso file. `list_screeners` resta l'UNICA eccezione nominata di
tutta la feature — i 10+ screener veri restano dietro il generico
`run_screener`, zero eccezioni nominate per loro.

## `/markets`: dispatcher screener generico (0.41.6)

`external_channel.rs`, entry `"financial-markets"` in `EXTERNAL_TOOL_CHANNELS`:
l'unica `PythonToolSpec` di `screen_stocks` è sostituita da due entry,
`list_screeners` (`report_title: None` — apre il picker, gestito per nome in
`ai_adapter.rs`, 0.41.5) e `run_screener` (`report_title:
Some(run_screener_title_fallback)`, `defer_report_to_turn_end: true` — il
titolo VERO arriva da Python via `PythonReportJson.title`, 0.41.4; la fn Rust
resta solo un fallback difensivo). `run_screener_title_fallback` sostituisce
`screen_stocks_title` (rimossa). `FINANCIAL_MARKETS_SYSTEM_PROMPT` non porta
più le istruzioni di giudizio per un singolo screener — quelle vivono ora nel
`summary` che Python ritorna (`screeners/__init__.py::dispatch`, iniettate
per screener), il prompt Rust si limita a dire quando chiamare
`list_screeners` vs `run_screener`. `description` di `run_screener` porta
l'elenco statico "id: titolo" degli screener noti (compromesso dichiarato,
vedi spec §2: l'unico tocco Rust residuo per screener nuovo, perché
`ToolDef.description` — non il docstring Python — è ciò che l'AI vede
davvero, verificato in `python_mcp_tool_client.rs::tool_defs()`).

## `/markets`: screener `goldman-sachs` (0.41.7)

Secondo screener del registry (`screeners/__init__.py::SCREENERS`), zero
tocco a `server.py`/`ai_adapter.rs`/protocol — solo una riga nella
`description` statica di `run_screener` in `external_channel.rs` (elenco
id/titolo che l'AI legge per matchare "esegui lo screener X", vedi
0.41.6). Tutta la logica nuova è in Python
(`scripts/pytools/financial-markets/screeners/goldman_sachs.py`): score a
percentili su 4 metriche (P/E, crescita ricavi, debito/patrimonio, upside
sul target) + bonus fino a 8 punti per prezzo sotto 80-100$, selezione
deterministica con quota minima 30% Technology/Communication Services
(nessun RNG, a differenza di `consumer-usage`: nessuna esclusione 30gg per
questo screener, top assoluto ripetibile), arricchimento post-selezione
(P/E medio di settore via `peers.py`, trend ricavi) calcolato SOLO sui
vincitori (default 25) con fetch live mai persistito in cache. Condivide
`fundamentals_cache.json` con `consumer-usage` (stesso universo indici,
stesso file) — entrambi gli screener contribuiscono alla stessa cache nel
tempo. Modulo indipendente da `consumer_usage.py` per design esplicito del
progetto: nessun import incrociato, piccoli helper di rendering duplicati.

## `/help` -- comandi mancanti aggiunti (0.41.8)

`HELP_MARKDOWN` (`core.rs`) era rimasto fermo ai comandi noti a v0.10.0.
Aggiunti i 4 comandi core introdotti da allora e mai riportati: `/aichat`
(UI-local, `app.js`) nella lista principale; `/markets`, `/nmap`, `/pyping`
(i tre canali tool esterni con `slash_trigger` digitabile da cursore, vedi
`external_channel.rs::EXTERNAL_TOOL_CHANNELS` / `external-channels.js`) in
una nuova sezione "Strumenti esterni" -- questi tre non passano MAI da
`handle_slash`/`Route::Slash` (aprono una connessione WS `Hello{channel}`
separata), quindi restavano invisibili a chi guardava solo il flusso di
`handle_command` documentato in cima a questo file. Esclusi deliberatamente:
comandi plugin (`/calc`, `/lc`, `/crypto`, ...) -- scoperti a runtime dal
manifest, mai in questa costante statica -- e `/library-expand` (nessun
trigger da cursore per design).

## `AiChatConfig`: campi `display_name`/`ai_display_name` (0.41.12)

Task 1 (indipendente, primo dei 9) del piano
`Docs/superpowers/plans/2026-08-13-aichat-display-names.md`: AI Chat mostra
oggi solo la bare `label_base` tecnica (es. "skimble") sia per l'umano che
per la sua AI. Due nuovi campi opzionali su `AiChatConfig`
(`aichat/config.rs`) preparano un nickname per ciascuno:

- `display_name: Option<String>` -- nickname dell'umano che usa questa
  macchina (es. "Maurizio"). `None` = nessun nickname impostato, fallback
  al comportamento odierno (bare label).
- `ai_display_name: Option<String>` -- nickname che l'AI si è scelta o si
  è fatta assegnare dall'utente (es. "Aria"), indipendente da
  `display_name`. `None` = l'AI non ha ancora un nome (task successivi:
  `ai_adapter::needs_ai_name_prompt` per il nudge di auto-nominazione, e
  il tool `set_ai_display_name` che lo valorizza).

Entrambi `#[serde(default)]` (bool/`Option` di default già `None`) --
stesso pattern già in uso per `ai_participates`/`ai_autoparticipate` in
questo stesso file: un `network.json` scritto PRIMA che questi campi
esistessero deserializza senza errore "missing field", coprendo la
migrazione silenziosa da versioni precedenti. `impl Default` per
`AiChatConfig` è un blocco letterale (non `#[derive(Default)]`), quindi
richiede l'aggiunta esplicita dei due campi (`display_name: None,
ai_display_name: None`) accanto agli altri, altrimenti `cargo build`
fallisce con "missing fields in initializer" (E0063).

Questo task è **solo dati**: nessun consumatore legge ancora questi due
campi (né `main.rs`, né `AiChatService`, né il frontend) -- il wiring
arriva nei task successivi dello stesso piano (Task 4 legge da
`network.json` in `main.rs`; Task 7 aggiunge il tool
`set_ai_display_name` che scrive `ai_display_name`; Task 9 mostra il
nickname/badge nel frontend AI Chat).

**Nota per Task 5** (`AiChatSettings` mirror per `/config` backend, non
ancora esistente al momento di questo task): se quel mirror finisce per
essere una struct SEPARATA da `AiChatConfig` che rilegge/riscrive
`network.json` per conto proprio, deve portare anche questi due campi --
altrimenti un round-trip attraverso quella struct scriverebbe
`network.json` senza `display_name`/`ai_display_name`, perdendo in
silenzio il nickname dell'utente (nessun `deny_unknown_fields` da nessuna
parte, quindi l'errore non sarebbe un fallimento di deserializzazione ma
una perdita silenziosa al primo save). Verificato con una grep mirata
(`network.json`/`AiChatSettings` in `crates/orchestrator/src`): oggi solo
`aichat/config.rs` e `main.rs` toccano `network.json`, nessun mirror
esiste ancora -- il rischio è puramente preventivo per quando Task 5 lo
introdurrà.

Test (`aichat::config::tests`): `load_or_generate_round_trips_display_names`
(save con entrambi valorizzati -> load rilegge gli stessi valori, più
l'asserzione sul default `None` su entrambi) e
`missing_display_name_fields_read_as_none` (JSON senza le due chiavi ->
deserializza con entrambi a `None`, non un errore).

## Wire protocol peer↔peer: `display_name`/`is_ai` additivi (0.41.13)

Task 2 (indipendente dal Task 1) del piano
`Docs/superpowers/plans/2026-08-13-aichat-display-names.md`: gemello, sul
protocollo peer↔peer (`src/aichat/wire.rs`, Contratto N), dei due campi già
aggiunti a `protocol::ChatLine`/`ServerMsg::AiChatMessage` (Contratto A, WS
orchestrator↔UI, stessa release lato crate `protocol` 0.15.3):

- `wire::ChatLine::display_name: Option<String>` -- nickname risolto dal
  mittente al momento dell'invio (umano o AI). `None` = fallback a
  `from_label` come oggi.
- `wire::ChatLine::is_ai: bool` -- `true` se il mittente è l'AI, non un
  umano. Pilota il badge nel frontend (Task 9). `false` di default.
- `ChatMsg::Say` guadagna gli stessi due campi, stessa semantica --
  l'attribuzione (`from_label`) viaggia SEMPRE nel messaggio, ora insieme
  al nickname risolto e al flag AI.

Entrambi `#[serde(default)]` su entrambi i siti: un peer con un binario non
ancora aggiornato che manda `{"type":"say","from_label":"...","text":"..."}`
senza i due campi nuovi deserializza comunque, con `display_name: None,
is_ai: false` -- nessuna rottura di compatibilità sulla rete LAN (peer
vecchi e nuovi coesistono).

`impl From<wire::ChatLine> for protocol::ChatLine` (il punto in cui una
riga di chat peer↔peer attraversa il confine e diventa un messaggio WS
verso la UI) ora copia anche `display_name`/`is_ai` invece di ignorarli --
prima di questo task la conversione era letterale campo-per-campo e li
avrebbe silenziosamente scartati anche se fossero esistiti.

Questo task resta **puro trasporto**: nessun sito del crate valorizza
ancora `display_name`/`is_ai` con un valore reale -- ogni costruzione
esistente (eco locale, relay, storico) li popola con il placeholder
`display_name: None, is_ai: false`. La risoluzione vera (leggere
`AiChatConfig::display_name`/`ai_display_name` e instradarli nei messaggi)
arriva con il Task 3 (`AiChatService`), stesso piano.

**Cascata meccanica** (nessuna logica cambiata, solo la shape del dato):
aggiungere due campi non-opzionali-per-i-literal-Rust a `wire::ChatLine`/
`ChatMsg::Say` ha rotto la compilazione di OGNI sito del crate che li
costruiva o decostruiva con un literal esaustivo -- `#[serde(default)]`
copre solo la deserializzazione JSON, non un `StructName { campo: valore
}` scritto a mano in Rust. Trovati tutti con `cargo build --tests -p
orchestrator` (iterando finché l'output è pulito, non a occhio): il
compilatore distingue da solo le due categorie via il codice d'errore --
`E0063` ("missing fields ... in initializer") per le COSTRUZIONI, sanate
aggiungendo `display_name: None, is_ai: false`; `E0027` ("pattern does not
mention fields") per le DECOSTRUZIONI (`match`/`matches!`), sanate
aggiungendo `..` al pattern (i due campi nuovi non servono a quei siti, che
guardano solo `from_label`/`text`). Totale: 33 costruzioni + 6 pattern in
`aichat/service.rs`, 1 costruzione in `aichat/transport.rs`, 1 in
`aichat/channel.rs`, 2 costruzioni + 1 pattern nei test di integrazione
(`tests/aichat_loopback.rs`, `tests/aichat_relay_loopback.rs`).

Test nuovi in `aichat::wire::tests`:
`wire_say_defaults_display_name_none_and_is_ai_false_when_absent`
(deserializza un `Say` storico senza i due campi -> `None`/`false`) e
`wire_chat_line_into_protocol_chat_line_carries_display_name_and_is_ai`
(round-trip attraverso `From` con entrambi valorizzati -> arrivano intatti
sul lato `protocol::ChatLine`).

## `AiChatService` risolve `is_ai`/`display_name` in `publish_say` (0.41.14)

Task 3 del piano `Docs/superpowers/plans/2026-08-13-aichat-display-names.md`: il
"collante" che chiude il cerchio aperto dai Task 1 (`AiChatConfig::display_name`/
`ai_display_name`, dati soli) e 2 (i campi wire additivi, mai valorizzati) --
`AiChatService` ora CALCOLA questi due campi per i propri messaggi e li
STAMPA sui tre siti di output (storico, rete, eco UI).

### Due campi nuovi sulla struct

```rust
display_name: Option<String>,    // nickname umano di QUESTA macchina
ai_display_name: Option<String>, // nickname AI di QUESTA macchina
```

Letti da `network.json` (via `AiChatConfig`, Task 1) e passati UNA volta a
`new()` -- NON "live", stesso pattern di `ai_participates`/`ai_autoparticipate`
già esistenti sulla struct (nessun ri-caricamento a caldo se l'utente modifica
`/config` mentre il servizio gira). `new()` guadagna due parametri finali:

```rust
pub fn new(
    me: PeerInfo,
    ai_adapter: Arc<dyn AiAdapter>,
    ai_participates: bool,
    ai_autoparticipate: bool,
    notes: NotesStore,
    display_name: Option<String>,      // NUOVO
    ai_display_name: Option<String>,   // NUOVO
) -> Self
```

### `publish_say`: CALCOLA (messaggi ORIGINATI da questa macchina)

`publish_say` è il choke-point riusato da tre chiamanti che hanno TUTTI in
comune "questo messaggio l'ho scritto io" (`HumanSay`, `AiReply`,
`AutoParticipateDone{Some}`, vedi il doc-comment del choke-point già
esistente). Subito dopo `let mut effects = Vec::new();`:

```rust
let is_ai = from_label.ends_with("-ai");
let display_name = if is_ai { self.ai_display_name.clone() } else { self.display_name.clone() };
```

Riusa lo stesso pattern `ends_with("-ai")` già in uso in `note_room_message`
per instradare -- una sola fonte di verità su "come si riconosce un
messaggio AI dalla sua label", non due. Calcolato una volta, propagato
INVARIATO ai tre costruttori che seguono nella stessa funzione: il push in
`self.history` (`ChatLine`), il `ChatMsg::Say` spedito in rete (relay
server/client secondo `self.channel.role()`), e l'`Effect::ToUi(AiChatMessage)`
per l'eco nella propria finestra.

### Arm `PeerMsg::Say`: SOLO PASS-THROUGH (messaggi RICEVUTI da un peer remoto)

Semantica deliberatamente OPPOSTA a `publish_say`. Il pattern che estrae i
campi dal messaggio ricevuto ora cattura anche i due nuovi (prima ignorati
con `..`):

```rust
ChatMsg::Say { from_label, text, display_name, is_ai } => {
```

...e li ricopia SENZA ricalcolarli in `ChatLine`/`AiChatMessage`:

```rust
self.history.push(ChatLine {
    from_label: from_label.clone(),
    text: text.clone(),
    display_name: display_name.clone(),  // pass-through, non `self.display_name`
    is_ai: *is_ai,                       // pass-through, non `from_label.ends_with(...)`
});
```

Perché non ricalcolare: il peer remoto ha GIÀ stampato questi campi con la
propria `publish_say`, usando i SUOI `self.display_name`/`self.ai_display_name`
(il nickname configurato sulla SUA macchina). Ricalcolare qui da
`from_label.ends_with("-ai")` darebbe comunque lo stesso `is_ai`
(deterministico dalla sola label, non dipende da chi lo calcola), ma
`display_name` sarebbe SBAGLIATO: risolveremmo il nickname configurato su
QUESTA macchina per un'etichetta (`from_label`) che appartiene a un'altra
macchina -- nel caso comune (nickname diversi sulle due macchine) il badge
mostrerebbe il nome sbagliato; nel caso in cui SOLO questa macchina ha un
nickname configurato, un messaggio altrui senza nickname erediterebbe il
nostro invece di restare `None` (fallback a `from_label` nel frontend).

Il relay server -> altri client (`self.channel.server_relay(from, msg.clone(), ...)`)
non richiede alcuna modifica: clona l'intero `ChatMsg` (pattern matchato per
riferimento, `match &msg`), quindi `display_name`/`is_ai` viaggiano a bordo
automaticamente verso gli altri client.

### Costruttori di comodo (`#[cfg(test)]`)

I 3 esistenti (`new_for_test`, `new_for_test_with_participation`,
`new_for_test_with_autoparticipate`) passano `None, None` come ultimi due
argomenti a `Self::new(...)` -- comportamento invariato (nessun nickname
configurato) per tutta la suite esistente, che non doveva sapere nulla di
questo task per continuare a compilare/passare.

Nuovo 4° costruttore, per i test che esercitano la risoluzione:

```rust
#[cfg(test)]
pub fn new_for_test_with_display_names(
    me: PeerInfo,
    display_name: Option<String>,
    ai_display_name: Option<String>,
) -> Self
```

`ai_participates`/`ai_autoparticipate` restano ai default di `new_for_test`
(`true`/`false`) -- non oggetto di questi test.

Fuori dai costruttori di comodo, altri 3 call-site diretti a
`AiChatService::new(...)` in `aichat/service.rs` (test che passano un
adapter custom, non lo `StubAdapter` dei costruttori di comodo) e 5 nei test
di integrazione (`tests/aichat_relay_loopback.rs` x3,
`tests/aichat_share_loopback.rs` x2) -- questi ultimi FUORI dal `cfg(test)`
del lib crate, quindi non intercettati da un `cargo test -p orchestrator`
senza `--tests`; la firma vecchia rompeva `cargo build --tests -p
orchestrator` finché non aggiornati con `None, None`.

### `main.rs`: NON ancora wired

Il call-site di produzione passa `None, None` per ora -- deliberatamente,
NON uno sbaglio. Il wiring vero da `cfg.display_name`/`cfg.ai_display_name`
(già presenti su `AiChatConfig` dal Task 1, letti da `network.json`) è
compito del Task 4 dello stesso piano, tenuto fuori da questo slice per
restare nel suo perimetro.

### Test

`aichat::service::tests`, 4 nuovi (2 su `publish_say` + 2 sull'arm
`PeerMsg::Say`):

- `publish_say_stamps_is_ai_false_and_human_display_name_for_local_human_message`
  -- `ServiceEvent::HumanSay` con `display_name = Some("Maurizio")`
  configurato produce un `Effect::ToUi(AiChatMessage)` con
  `from_label = "skimble-human"`, `is_ai = false`,
  `display_name = Some("Maurizio")`. RED prima dell'implementazione:
  falliva in compilazione (`AiChatService::new` non accettava ancora 7
  argomenti).
- `publish_say_stamps_is_ai_true_and_ai_display_name_for_local_ai_message`
  -- `ServiceEvent::AiReply` con `ai_display_name = Some("Aria")`
  configurato (e `display_name = Some("Maurizio")`, per verificare che NON
  venga usato) produce `from_label = "skimble-ai"`, `is_ai = true`,
  `display_name = Some("Aria")`. Stesso RED del test sopra.
- `peer_msg_say_passes_through_received_display_name_and_is_ai_without_recomputing`
  -- un `PeerMsg::Say` RICEVUTO con `display_name: Some("Gianni")` (il
  nickname del MITTENTE) deve produrre `AiChatMessage` con
  `display_name = Some("Gianni")`, anche se il RICEVENTE ha nickname
  locali DIVERSI configurati (`"Maurizio"`/`"Aria"`, via
  `new_for_test_with_display_names`). Discrimina davvero pass-through da
  ricalcolo -- con `new_for_test` semplice (nessun nickname locale) i due
  comportamenti darebbero lo stesso risultato e il test non proverebbe
  nulla. Scritto DOPO l'implementazione (nessuna compilazione mancante da
  sfruttare per un RED genuino, a differenza dei due test sopra); RED
  simulato a mano: swap temporaneo del corpo di produzione a
  `if *is_ai { self.ai_display_name.clone() } else {
  self.display_name.clone() }` (lo stesso bug di ricalcolo che il test
  deve prevenire), verificato che il test fallisce con `Some("Maurizio")`
  invece di `Some("Gianni")`, poi revertito.
- `peer_msg_say_passes_through_none_display_name_without_inheriting_local_ai_nickname`
  -- gemello sul caso `is_ai: true`/`display_name: None` (mittente senza
  `ai_display_name` ancora configurato): il ricevente (con
  `ai_display_name = Some("Aria")` locale) non deve ereditare "Aria" per
  l'etichetta di un altro peer. Stesso falsificazione a mano del test
  precedente.

Il servizio in questi ultimi due test resta `Role::Undecided` (nessun peer
marcato connesso/ammesso via `mark_connected_for_test`/
`mark_admitted_for_test`): il relay-guard su `self.admitted` si applica
solo da `Role::Server`, quindi non serve simulare l'ammissione per
raggiungere l'`Effect::ToUi`.

`cargo test -p orchestrator`: 879 passed, 0 failed, 8 ignored (875 nella
baseline pre-Task-3 + questi 4 test nuovi -- nessuna regressione).
`cargo clippy --all-targets`: nessun warning nuovo introdotto da questo
task (i 7 preesistenti sono su file non toccati qui, `config.rs`/`ws.rs`,
o su righe di test preesistenti in `service.rs` non modificate da questo
slice).

## `/markets`: screener `jensen-huang` (0.41.20)

Terzo screener del registry (`screeners/__init__.py::SCREENERS`), zero tocco
a `server.py`/protocol — solo una riga nella `description` statica di
`run_screener` in `external_channel.rs`. Tutta la logica è in Python
(`scripts/pytools/financial-markets/screeners/jensen_huang.py`): universo
filtrato per industry AI-supplier via `industry_qualifies()` (match
normalizzato su dash/maiuscole — il separatore delle etichette yfinance non
è garantito), score a percentili su 4 metriche proprie (crescita ricavi,
PSG = P/S ÷ crescita, pullback dal massimo 52 settimane, target upside, min
2/4), bonus additivo +10 e badge per i ticker della costante curata a mano
`NVIDIA_LINKED` (ultima revisione 2026-08-17). Selezione deterministica
top-N (nessuna quota settore, nessuna esclusione 30gg, nessun RNG), NESSUN
enrichment live post-selezione. Condivide `fundamentals_cache.json` con gli
altri screener; `ScreeningSnapshot` esteso col campo nullable `industry`
(percorso: TypedDict, yfinance/fake/ibkr source, `_SNAPSHOT_KEYS`). Modulo
indipendente per design: nessun import incrociato, helper duplicati.

## `/markets`: screener `citadel` (0.41.21)

Quarto screener del registry (`screeners/__init__.py::SCREENERS`), primo
NON fondamentale — analisi tecnica su serie storiche OHLC invece di uno
snapshot `.info` puntuale. Zero tocco a `server.py`/protocol — solo una
riga nella `description` statica di `run_screener` in
`external_channel.rs`. Tutta la logica è in Python:
`screeners/citadel.py` (indicatori hand-rolled — SMA/RSI/MACD/Bollinger/
volume ratio/6 mesi/swing/Fibonacci/trend settimanale-mensile via resampling
locale, nessuna libreria TA nelle dipendenze) + `technical_cache.py`
(mirror strutturale di `fundamentals_cache.py`, modulo SEPARATO: entry
shape `{"fetched_at","bars"}` incompatibile con lo snapshot piatto dei
fondamentali, freschezza 1 giorno invece di 7). Regime per ticker
(momentum se prezzo >= SMA50, altrimenti mean-reversion) deciso in
`build_rows()`, un solo score per riga dalla formula del proprio regime;
`score_rows()` calcola percentili SEPARATAMENTE per gruppo (con guardia
`MIN_GROUP_FOR_PERCENTILE=10` — sotto soglia, 50.0 neutro invece di un
percentile pieno che gonfierebbe un gruppo piccolo) poi unisce i due pool
ordinati per score. `select()` applica un TETTO (non quota) di 2 titoli
per bucket (industry grezza normalizzata, o "AI" per i fornitori di
infrastruttura AI — stessa lista duplicata da `jensen_huang.py`, con
commento di sincronizzazione), con backfill se il tetto impedisce di
raggiungere `top_n`. `industry` per il bucket è letto in SOLA LETTURA da
`fundamentals_cache.json` (lo stesso `cache_path` di ogni screener,
popolato dagli altri tre) — mai scritto da citadel, che deriva il proprio
path OHLC come file sibling (`cache_path.parent / "technical_cache.json"`).
Pattern grafici ed entry/stop/target/R:R restano giudizio narrativo
dell'AI (`judgment_instructions`), mai calcolati in Python. Modulo
indipendente per design: nessun import incrociato con gli altri screener.
