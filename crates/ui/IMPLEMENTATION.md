# Implementation — crates/ui v2.3.3

## Limite tentativi di reconnect canali esterni e fix reset backoff (v2.3.3)

Nel piano `Docs/i18n/ita/compiti-ai-esterne/2026-09-09-reconnect-infinito-canale-esterno.md`:
1. **Parametro opzionale `maxRetries` in `LareWsClient` (`crates/ui/frontend/ws-client.js`)**:
   - Consente di limitare per-istanza i tentativi di riconnessione consecutivi.
   - Quando `_retryCount >= maxRetries`, `_scheduleRetry()` emette lo stato terminale `"failed"` e si arresta senza schedulare ulteriori timer.
   - `host.js` continua a non specificare `maxRetries`, mantenendo il comportamento di retry indefinito richiesto per il canale cursore primario.
2. **Correzione punto di reset del backoff**:
   - Rimosso `this._retryMs = RETRY_INITIAL_MS` dal listener `"open"` (livello TCP).
   - Il reset avviene ora nel blocco `msg.type === "server_info"` (handshake applicativo completato con successo) e in `connect()` all'avvio.
3. **Canali esterni (`crates/ui/frontend/external-channel-window.js`)**:
   - Impostato `maxRetries: 6` (~31s totali di backoff prima di arrendersi).
4. **Renderer e Localizzazione (`renderer.js`, `it.json`, `en.json`, `es.json`)**:
   - Aggiunto mapping dello stato `"failed"` sull'etichetta `ext_channel.status_failed` ("●  non disponibile", "●  failed", "●  no disponible").

## Supporto terza lingua: spagnolo (v2.3.2)

Nel piano `Docs/i18n/ita/compiti-ai-esterne/2026-09-09-i18n-help-esterno-e-spagnolo.md` (Parte B):
1. **Dropdown Lingua (`crates/ui/frontend/config-dialog.js`)**:
   - Aggiunta voce `{ value: "es", label: "Español" }` al menu a tendina di `/config`.
2. **Test di Parità Generalizzato (`crates/ui/frontend/i18n-parity.test.mjs`)**:
   - Automatizzata la scansione di tutti i dizionari `*.json` in `Configuration/i18n/` per verificare
     la corrispondenza completa con le 209 chiavi di `it.json`.
3. **Dizionario Spagnolo (`Test Run/Configuration/i18n/es.json`)**:
   - Completo di tutte le 209 chiavi tradotte in spagnolo, con parametri dinamici preservati.

## i18n Parte 2: Cascata di tutte le finestre frontend e titoli nativi (v2.3.1)

Completamento dell'internazionalizzazione a cascata di tutte le restanti finestre dell'applicazione e dei titoli nativi di Tauri (`Docs/i18n/ita/compiti-ai-esterne/2026-09-08-i18n-programma.md`):

1. **Finestre e moduli convertiti**:
   - `note-window.html` + `note-window.js` (`note.*`): etichette, placeholder, validazioni corpo/titolo.
   - `window-search.html` + `window-search.js` + `search-status.js` (`search.*`): stati di ricerca live e replay, label sorgenti, aria-label, fallback titolo.
   - `library.html` + `library.js` + `library-nav.mjs` (`library.*`): tab bar, toolbar, filtri per tag, dialoghi eliminazione/rinomina, form nuova cartella, lista note e find, share picker, stati vuoti/errori.
   - `aichat-window.html` + `aichat-window.js` + `aichat-view.mjs` + `admission.mjs` + `share-view.mjs` (`aichat.*`): input chat, banner ammissione e consenso peer, roster presenti, banner e log esiti share, messaggi di stato.
   - `routine-preview.html` + `routine-preview.js` (`routine_preview.*`): metadati routine, banner sostituzione/aggiornamento, bottoni azione, titoli finestra.
   - `plugin-window.html` + `plugin-window.js` (`plugin.*`): messaggi di errore caricamento e assenza contenuto, titolo barra.
   - `external-channel.html` + `external-channel-window.js` + `renderer.js` (`external_channel.*`): form parametri, bottoni esecuzione, messaggi di caricamento/errore IPC/stato.
   - `screener-picker.html` + `screener-picker.js` (`screener_picker.*`): hint bar, stato lista vuota, titolo barra.
   - `window.html` + `window.js` (`md_window.*`): barra espandi (input, bottone, status, errori AI/connessione), bottone salvataggio in archivio, caricamento e fallback titoli.
   - `host.html` + `host.js`: pulizia fallback rigidi (passa stringa vuota a `open_search_window` demandando a Rust il titolo localizzato), invariati i contratti wire.
   - `terminal.html` + `terminal.js` (`terminal.*`): indicatore ultimo comando, etichetta sessione, banner riavvio shell, messaggi errore pty e terminazione processo.

2. **Titoli nativi delle finestre in Rust (`main.rs`)**:
   - Tutte le funzioni Tauri di creazione finestra (`open_search_window`, `open_screener_picker_window`, `open_plugin_window`, `open_routine_preview`, `open_library_window`, `open_aichat_window`, `open_note_window`, `open_saved_find_window`) leggono la lingua da `ConfigState` e usano `ui_lib::i18n::t_sync(&i18n_dir, &lang, "key")`.
   - Il titolo nativo `"Lare Terminal"` per la finestra principale è preservato letterale come richiesto da specifica.

3. **Dizionari e Parità**:
   - `Test Run/Configuration/i18n/{it,en}.json` espansi da 34 a 209 chiavi sincronizzate biunivocamente.
   - Test `i18n-parity.test.mjs` verde (0 chiavi mancanti, 0 chiavi orfane).
   - 251 unit test frontend (`node --test`) e 147 test Rust (`cargo test -p ui`) passano con successo.
   - `cargo clippy -p ui --all-targets` pulito senza alcun warning.

## i18n Parte 1: Fondamenta e finestra /config (v2.3.0)

Implementazione della prima fase dell'internazionalizzazione dell'interfaccia utente (`Docs/i18n/ita/compiti-ai-esterne/2026-09-08-i18n-programma.md`):

1. **Backend Rust (`i18n.rs`)**:
   - `load_dict(path: &Path) -> HashMap<String, String>`: caricamento infallibile da file JSON piatto.
     Se il file non esiste, ritorna mappa vuota. Se il JSON è corrotto o si verifica un errore IO,
     registra un avviso con `tracing::warn!` e ritorna mappa vuota senza bloccare l'applicazione.
   - `load_merged_dict(i18n_dir: &Path, lang: &str) -> HashMap<String, String>`: carica `it.json` come
     base e, se `lang != "it"`, sovrascrive con i valori di `<lang>.json`.
   - `t_sync(i18n_dir: &Path, lang: &str, key: &str) -> String`: traduce sincronicamente applicando la
     catena completa `lang -> it -> key` (se una chiave non è tradotta né in `lang` né in `it`,
     ritorna la chiave letterale).
   - Esportato in `lib.rs` (`pub mod i18n;`) per consentire i test unitari isolati senza runtime Tauri.
2. **Configurazione utente (`config.rs`)**:
   - Campo `pub language: String` su `Config`, con `#[serde(default = "default_language")]` che restituisce `"it"`.
   - Mantenuta piena compatibilità con file `config.json` legacy che omettono il campo.
3. **Comando Tauri IPC e titoli finestra (`main.rs`, `config_dir.rs`)**:
   - Aggiunto `config_dir::i18n_dir_path(config_dir: &Path) -> PathBuf` (`<config_dir>/i18n`).
   - Registrato comando `get_i18n(lang: String, state: State<'_, ConfigDirState>) -> HashMap<String, String>`
     nell'invocazione di `generate_handler!`.
   - `open_config_window` imposta dinamicamente il titolo finestra nativo usando `i18n::t_sync(&i18n_dir, &lang, "config.title")`
     mantenendo `"Lare — "` prefisso. `"Lare Terminal"` resta letterale come da specifica.
4. **Dizionari JSON (`Test Run/Configuration/i18n/`)**:
   - `it.json` ed `en.json` contengono 34 chiavi iniziali per controlli comuni (`common.*`) e dialog `/config` (`config.*`).
5. **Modulo Frontend (`crates/ui/frontend/i18n.mjs`)**:
   - `t(key, params)`: traduce chiavi letterali con sostituzione parametri `{param}` via `replaceAll`.
   - `fetchI18n(invoke, lang)`: carica dizionario via IPC `get_i18n`, con gestione fallback infallibile.
   - `applyI18n(root)`: scansiona il DOM e applica le traduzioni per `data-i18n` (textContent),
     `data-i18n-placeholder` (placeholder), `data-i18n-title` (title) e `data-i18n-aria-label` (aria-label).
6. **Finestra `/config` convertita**:
   - `config.html`: attributi `data-i18n` su titolo barra e pulsante di chiusura.
   - `config-window.js`: all'avvio carica `get_config`, scarica il dizionario con `fetchI18n`,
     esegue `applyI18n()` e infine apre `dlg.open()`.
   - `config-dialog.js`: tutte le etichette, bottoni, tab, messaggi di validazione e note usano `t("...")`.
   - Selettore lingua: dropdown aggiunto nella tab UI con opzioni fisse "Italiano" ed "English".
     Il salvataggio persiste `language` nel `config.json`.
7. **Test suite**:
   - Rust: 4 unit test in `i18n.rs` (assente, corrotto, valido, fallback chain) + 4 in `config.rs`.
   - JS: `i18n.test.mjs` (8 test unitari su `t`, `initI18n`, `fetchI18n`, `applyI18n`).
   - Parità: `i18n-parity.test.mjs` garantisce che ogni chiave usata nel frontend o in Rust
     esista in entrambi i file di localizzazione e che non vi siano chiavi orfane.

## Favicon (v2.2.3, fix del DA FARE lasciato da 2.2.2)

`crates/ui/frontend/favicon.ico` — nuovo file, root del `frontendDist`. Il subscriber `tracing`
globale aggiunto in 2.2.2 rendeva visibile per la prima volta un log interno di Tauri
(`ERROR tauri::manager: asset not found: favicon.ico`, ripetuto a ogni apertura di webview):
WebView2 richiede implicitamente `/favicon.ico` per ogni pagina caricata, e prima di questo file
la richiesta risolveva sempre in 404. Non una regressione (l'assenza era già vera, solo
invisibile prima del subscriber) ma rumore in un log che l'utente ha chiesto pulito.

`.ico` multi-risoluzione (16/32/48/64/128/256, ~19KB), generato con Pillow in una venv usa-e-getta
(non nel repo — non è un tool di progetto ricorrente, un'unica generazione). Soggetto su
richiesta esplicita dell'utente: casa vittoriana stilizzata con un fantasma che aleggia sul
tetto. Due passate per la leggibilità a 16×16 (il vincolo duro di un favicon — deve leggersi come
icona di scheda, non solo come immagine grande): la prima versione aveva troppi dettagli sottili
(luna, comignolo stretto, crocette sulle finestre) e un contrasto casa/cielo troppo basso, tutto
si fondeva in una macchia scura a 16px; tolti i dettagli sottili, alzato il contrasto tra corpo
casa/tetto/cielo. Verificato estraendo i singoli frame dell'`.ico` (non l'anteprima a piena
risoluzione, che con un resize morbido avrebbe nascosto il problema) e ingrandendoli con
nearest-neighbor per un giudizio onesto sulla leggibilità reale.

Gotcha noto (`BUILD.md`): `generate_context!` incorpora `frontendDist` a compile time — serve
`cargo clean -p ui` prima del build perché un nuovo file nella cartella frontend venga incluso,
altrimenti la build sembra ignorarlo. Verificato dal vivo: nessuna riga `asset not found:
favicon.ico` in un avvio fresco dopo il fix (prima: 3-4 per avvio).

## Niente console, log su file, via il thread "q" (v2.2.2, fix da uso reale)

Tre cambiamenti in `main.rs`/`config_dir.rs`/`launcher.rs`, vedi CHANGELOG 2.2.2 per il perché
(finestre console spurie osservate in uso reale con Windows Terminal come terminale predefinito):

- **`#![windows_subsystem = "windows"]` incondizionato** (prima `#[cfg_attr(not(debug_assertions),
  ...)]`, quindi presente solo in release): ora `ui.exe` non ha mai una console, nemmeno in debug.
  Necessario perché i processi figli console-subsystem che l'orchestrator spawna in self-heal
  (mcp-server/mcp-nmap/python, vedi il fix `CREATE_NO_WINDOW` gemello in `orchestrator` 2.2.1)
  erediterebbero altrimenti la console di `ui.exe` se ne avesse una.
- **Logging su file**: `startup_config::logging::init_logging(&cfg_state.log_dir(), "ui.log",
  &cfg_state.startup.log.level, false)` chiamato in `main()` subito dopo
  `ConfigDirState::from_process()`, PRIMA di ogni altro uso di `println!`/`eprintln!` (che infatti
  non esistono più in questo file — vedi sotto). `_log_guard` è una variabile locale di `main()`,
  viva per tutta la funzione (spostarla dentro `.setup()` la farebbe droppare troppo presto,
  interrompendo il flush del writer non bloccante — la closure di `.setup()` ritorna prima che
  `app.run(...)` finisca). `console: false` sempre (a differenza dell'orchestrator, che con
  `--console-log` logga anche su stderr): `ui.exe` non ha mai una console da cui farlo. Nuovo
  `ConfigDirState::log_dir()` in `config_dir.rs`, stesso schema di `RuntimeConfig::log_dir()`
  nell'orchestrator (`StartupConfig::resolve_path(&self.config_dir, &self.startup.log.dir)`).
  Ogni `println!`/`eprintln!` rimasto in `main.rs` e `launcher.rs` (16 punti in `main.rs`, 6 in
  `launcher.rs`) convertito meccanicamente: `println!` → `tracing::info!`, `eprintln!` →
  `tracing::warn!`, testo del messaggio invariato (incluso il prefisso `[ui]`/`[archive]`).
- **Via il thread "digita 'q' per uscire"** (era `#[cfg(debug_assertions)]`, vedi il blockquote
  v0.36.3 più sotto, ora superato): obsoleto dal fix v2.2.1 (chiudere la finestra terminale già
  termina il processo pulito) — un secondo modo di uscire da stdin non serve più, e comunque
  `ui.exe` non ha più stdin da leggere (nessuna console, punto sopra). Rimossi `is_quit_command` e
  i suoi 2 test insieme al blocco.

## Finestra terminale: `pty.rs`, `launcher.rs`, `terminal.js` (v2.2.0, fix v2.2.1)

Piano `Docs/i18n/ita/superpowers/plans/2026-09-07-piano-3-finestra-terminale.md`, spec §2.3/§5,
ADR-016/ADR-020. `ui.exe` guadagna la **modalità A**: una finestra terminale (xterm.js dentro
ConPTY) con `lare-shell.exe` come processo figlio — l'app che l'utente avvia direttamente, invece
del solo host di finestre nascosto.

### `pty.rs` — plumbing ConPTY testabile

Nuovo modulo (`crates/ui/src-tauri/src/pty.rs`), sopra `portable-pty` (non l'API Windows ConPTY
diretta — `portable-pty` la incapsula e resta portabile in teoria, anche se il prodotto è
Windows-only per ora). Tre comandi Tauri (`main.rs`): `pty_spawn(session_id, cols, rows)` (risolve
`lare-shell.exe` da `cfg.shell_exe()`, passa `--config-dir <dir> --session <session_id>` — una
sola sessione pty alla volta, una seconda `pty_spawn` mentre la prima è ancora attiva viene
rifiutata, non accodata né sostituita), `pty_write(data)`, `pty_resize(cols, rows)`.

**Perché è testabile senza una ConPTY reale**: l'output della pty non va MAI direttamente a una
`WebviewWindow` — passa per `PtyOutputSink` (trait: `on_output(base64_chunk)`/`on_exit(code)`),
con due implementazioni: `AppHandleSink` (produzione, `main.rs` — un newtype su `AppHandle` che
fa `self.0.emit("pty-out"/"pty-exit", ...)`, evento globale) e `FakeSink` (test, un buffer in
memoria dietro un `Mutex`). Stesso schema di `IProcessStarter` nella host C# (composizione, non
eredità — dal doc-comment del trait). I test spawnano un processo reale ma innocuo (`cmd /c echo
hello`/`cmd /c pause` — "una shell finta disponibile su ogni Windows", non `lare-shell.exe`, che
non esiste nel sandbox dei test) e devono rispondere a mano alla DSR `ESC[6n` che conhost manda al
primo avvio del figlio (altrimenti la pty resta bloccata a tempo indefinito — scoperto dal vivo,
`answer_cursor_position_query` nei test simula la stessa risposta CPR che xterm.js dà nativamente
in produzione). Non toccano mai Tauri: `SharedPtyState` (lo stato condiviso dietro
`Arc<Mutex<...>>`) è lo stesso tipo passato a `.manage(PtyState(...))` in produzione e costruito a
mano nei test.

**Concorrenza nota**: `on_exit` (thread `spawn_exit_watcher`) può arrivare PRIMA dell'ultimo
`on_output` (thread `spawn_reader_thread`) — due thread indipendenti che scrivono sullo stesso
sink senza un ordine garantito tra loro. Documentato nel doc-comment di `on_exit`; il frontend
(sotto) non decide "ho finito" solo sull'evento di uscita.

**`kill(state)` (v2.2.1, fix post-piano)**: terza operazione oltre a `write`/`resize`, per
terminare il processo pty da un contesto completamente indipendente (l'handler di chiusura
finestra in `main.rs`, non il thread di `spawn_exit_watcher`). `PtySession` tiene un
`killer: Box<dyn ChildKiller + Send + Sync>`, ottenuto con `child.clone_killer()` subito dopo lo
spawn — è esattamente il caso d'uso per cui `portable-pty` espone quel metodo (poter segnalare il
processo da un thread diverso da quello bloccato in `Child::wait()`, che ne ha il possesso
esclusivo). **Bug verificato in `portable-pty` 0.9.0 su Windows**: `WinChildKiller::kill`
(`src/win/mod.rs`) ha la condizione invertita sull'esito di `TerminateProcess` — quell'API Win32
ritorna non-zero in caso di SUCCESSO (a differenza della convenzione POSIX/errno usata altrove nel
crate), ma il codice fa `if res != 0 { Err(err) } else { Ok(()) }`: un kill riuscito arriva come
`Err` con `raw_os_error() == Some(0)` (ERROR_SUCCESS, "operazione completata con successo"). `kill`
tratta esplicitamente quel caso come successo (commento nel codice rimanda al file/riga esatti del
crate), propagando ogni altro errore. Provato dal vivo dal test
`kill_termina_il_processo_pty_attivo` (spawna una shell finta a vita lunga, chiama `kill`, verifica
con `wait_until` che l'exit watcher se ne accorga — prova che il processo OS muore davvero).

### `launcher.rs` — self-heal Rust dell'orchestratore + `--no-terminal`

Nuovo modulo (`crates/ui/src-tauri/src/launcher.rs`), mirror Rust di `Launcher.cs` (host C#,
piano 2b): `ensure_orchestrator(connect, autostart, orchestrator_exe, config_dir, window, retry)`
— `connect: &dyn Fn() -> Option<String>` (`None` = connessione riuscita, altrimenti il motivo,
stessa forma di `Func<string?>` in `Launcher.cs::EnsureConnected`); se fallisce e `autostart` è
vero ED `orchestrator_exe.exists()`, chiama `startup_config::spawn_detached(orchestrator_exe,
&["--config-dir", ...])` e ritenta `connect()` ogni `retry` (`RETRY_INTERVAL`, 250ms in
produzione) fino a `window` (`CONNECT_WINDOW`, 5s) prima di arrendersi (ritorna `bool`: successo
finale). Stesso schema di firma (funzione di controllo iniettata, non una chiamata diretta a
`TcpStream`) di `ensure_ui_sink` (`orchestrator`, Task 6 dello stesso piano) — implementazioni
indipendenti dello stesso pattern, non condivisione di codice fra i due crate.

**`--no-terminal`** (ADR-020): letto in `main.rs` da `std::env::args()`, tenuto in uno stato
gestito `NoTerminal(bool)`. Con il flag, `ui.exe` NON costruisce la finestra terminale — resta il
puro host di finestre nascosto delle versioni precedenti. Chi avvia `ui.exe` per il solo ruolo
host lo passa sempre: `Launcher.EnsureUi()` (C#, `lare-shell` 2.0.1) e `ensure_ui_sink`
(`orchestrator`, Task 6). Senza il flag (uso interattivo diretto, modalità A) la finestra si apre
sempre — comportamento di default intenzionale.

### `terminal.{html,css,js}` — la finestra

Costruita in `.setup()` (a meno di `--no-terminal`): xterm.js + `addon-fit` (vendored, Task 0),
1000×650, `lare-shell.exe` spawnato via `pty_spawn` con `--config-dir <dir> --session <id>`
(percorso da `config_dir::shell_exe()`, risolto da `startup.json` → `paths.shell`). Codice di
cablaggio imperativo (`terminal.js`), non testato in isolamento — stesso trattamento di
`host.js`/`library.js` per la parte non-pura, mentre la logica pura che consuma è nei 4 moduli
testati del Task 1 (`base64.mjs`, `osc-lare.mjs`, `indicators.mjs`, `fit-debounce.mjs`).
`get_terminal_session` (comando Tauri) espone l'id di sessione risolto a `terminal.js` — non
gestito in modalità `--no-terminal` (nessun chiamante possibile in quel ruolo, la finestra non
esiste).

**Chiusura finestra → fine processo (v2.2.1, fix post-piano)**: in `main.rs`, dopo `.build()`
della finestra terminale, `WebviewWindow::on_window_event` (metodo sull'oggetto costruito — il
builder `WebviewWindowBuilder` non lo espone) intercetta `WindowEvent::CloseRequested` e chiama
`pty::kill(...)` poi `AppHandle::exit(0)`. "Una finestra, una sessione" (spec §5) applicato fino in
fondo: in modalità A la finestra terminale è l'unica ragione per cui `ui.exe` esiste, quindi
chiuderla (bottone X) termina anche `lare-shell.exe` (altrimenti orfano — nessuno lo possiede più)
e l'intero processo `ui.exe`, comprese le altre finestre eventualmente aperte (output Markdown,
config, ...), che sono secondarie e non tengono in vita l'app da sole per progetto (l'host nascosta
resterebbe altrimenti sempre viva). `PtyState`/`AppHandle` sono catturati prima della closure —
`app: &AppHandle` di `.setup()` non è disponibile dentro un handler di evento finestra, che vive
più a lungo di quello scope.

Consuma due segnali mai letti prima d'ora: l'OSC 9001 `intercept` (emesso dalla host dal piano 2b)
via `registerOscHandler(9001, …)` di xterm.js — segnalino acceso dopo un comando gateizzato — e
`ActivityIndicator` (emesso dall'orchestratore dal piano 2a, Task 5: relay da `host-dispatch.mjs`
— classificato `"relay"` — all'evento `terminal:activity`) — segnalino acceso per la durata di un
turno `/ai` lungo. Bottone "riavvia" se la pty termina (processo figlio chiuso/crashato).

## Finestra di output, `open_ui_local`, `/help` singleton, `ui_pong`, output-buffer (v2.1.0)

Piano `Docs/i18n/ita/superpowers/plans/2026-09-05-piano-2a-protocollo-shell.md` Task 9, spec
`Docs/i18n/ita/superpowers/specs/2026-09-04-lare-terminal-2-design.md` §3.2/§4.1/§5, ADR-018.
`ui.exe` guadagna la superficie che riceve l'output dei comandi slash lanciati da una sessione
`lare-shell`: nessuna finestra/flusso v1 esistente cambia.

### Finestra di output (`open_output_window`)

Stessa "forma" delle finestre Markdown esistenti (chromeless, `window.html`/`window.js`), ma con
un ciclo di vita diverso: si apre SUBITO (all'inizio del turno, dall'orchestratore) col
segnaposto `"_in corso…_"`, e riceve il contenuto vero DOPO, una volta sola, via evento Tauri
globale — non tramite `WindowContentStore`/`take_window_content` come le altre. `main.rs`:
`build_markdown_window(app, label, title)` è stata estratta da `open_markdown_window` (v1) proprio
per essere condivisa dal nuovo comando `open_output_window(window_id, title, …)`, che usa la label
`output-<window_id>` — se la stessa label arriva due volte (non dovrebbe succedere: un `window_id`
è un `turn_id`, univoco) riusa la finestra esistente invece di aprirne una seconda.

`window.js`/`host.js` fanno **buffer-and-replay** del contenuto (vedi sotto): il messaggio
`ServerMsg::OutputWindowContent` arriva sul canale WS dentro `host.js` (la pagina host nascosta),
che lo re-inoltra alla finestra di output tramite l'evento globale Tauri `output:content` — le due
webview sono processi separati, comunicano SOLO con `invoke`/eventi, mai per import diretto.

### `open_ui_local` — finestre locali chieste da una shell

`ServerMsg::OpenUiLocal{name}` arriva su `host.js`, che risolve `name` in un comando Tauri
esistente tramite `resolveUiLocal(name, EXTERNAL_TOOL_CHANNELS)` (nuovo modulo puro
`ui-local.mjs`, senza DOM/Tauri — testabile con `node:test`): `"config"`/`"library"`/`"aichat"` →
i tre comandi singleton già esistenti (`open_config_window`, `open_library_window`,
`open_aichat_window`); l'id di un canale esterno (`/nmap`, `/markets`, `/pyping`) →
`open_external_channel_window`. Nome ignoto → `console.warn`, nessuna finestra (stesso trattamento
conservativo di uno slash ignoto lato orchestratore). Nessun comando Tauri nuovo: `open_ui_local`
riusa interamente la superficie v1, orchestrata da un modulo di puro instradamento.

### `/help` singleton (D15) — `open_markdown_window` guadagna `label`

`open_markdown_window` accetta ora un quinto parametro opzionale, `label: Option<String>`: con
una label fissa, se `app.get_webview_window(&label)` trova già una finestra con quel nome, la
porta in primo piano (`set_focus`) e ritorna SENZA aprirne una seconda — altrimenti genera la
label univoca `md-<ts>-<ctr>` di sempre. `host.js::openMarkdownWindow` passa
`markdownWindowLabel(kind)` (`ui-local.mjs`): `"help"` per `kind === "help"`, `undefined`/`null`
per ogni altro kind — Rust lo riceve come `None`, comportamento invariato per tutte le altre
finestre Markdown (`/show`, routine, ecc.). Motivo: il piano vuole UNA finestra `/help` per
macchina, non una nuova ogni volta che l'utente digita il comando (coerente con `/config`/
`/library`/`/aichat`, già singleton in v1).

### `ui_pong` — risposta al built-in `/ping`

`get_ui_version()` (comando Tauri, `env!("CARGO_PKG_VERSION")`) è letta UNA volta in
`bootstrap()`, prima di `initClient()`, e tenuta in una variabile di modulo (`uiVersion`): quando
arriva `ServerMsg::UiPing{id}`, `host.js` risponde subito con `client.sendUiPong(id, uiVersion)`
(nuovo metodo su `LareWsClient`, wire `{"type":"ui_pong", id, version}`) — nessuna chiamata IPC
nel percorso caldo della risposta.

### `output-buffer.mjs` — fix round 1: buffer-and-replay di `output:content`

Bug trovato in review (stesso principio del bug già risolto per la ricerca live in
`search-buffer.js`): la finestra di output è una webview separata che si apre in modo
**asincrono** — `open_output_window` crea la finestra, poi `window.js` deve caricare ed eseguire
il proprio bootstrap prima di potersi mettere in ascolto. L'evento Tauri globale `output:content`,
a differenza di una coda, **non è bufferizzato dal runtime**: se l'orchestratore risponde PRIMA
che la finestra abbia registrato il listener (un comando slash veloce, es. `/open x`), quel
contenuto va perso per sempre e la finestra resta bloccata sul segnaposto — senza che nessuna
superficie se ne accorga.

Soluzione (stesso pattern di `search-buffer.js`/`createSearchBuffers()`): `createOutputBuffers()`
(nuovo modulo **puro**, niente Tauri/DOM — 62 test in `output-buffer.test.mjs`) accumula il
contenuto per `windowId` finché la finestra non si iscrive esplicitamente. Protocollo a tre
eventi:
- `host.js` chiama `outputBuffers.open(windowId)` **PRIMA** di invocare `open_output_window`
  (sincrono, come per `search_open`): non c'è mai una finestra di tempo scoperta fra apertura e
  buffer pronto.
- Quando arriva `output_window_content`, `outputBuffers.content(windowId, markdown)` ritorna
  `{emit: true}` se la finestra è GIÀ sottoscritta (emette subito l'evento `output:content`) o
  `{emit: false}` se non lo è ancora (bufferizza, in attesa).
- `window.js` si mette in ascolto di `output:content` **PRIMA** di annunciarsi (`await` sul
  `listen`, poi `emit("output:subscribe", {window_id})`): `host.js` risponde con
  `outputBuffers.subscribe(windowId)` — se c'era contenuto bufferizzato, lo rigioca come lo
  STESSO evento `output:content` (un evento live e un replay arrivano quindi allo stesso
  handler, nessuna logica duplicata in `window.js`).
- `closeWindow()` (`window.js`) emette `output:closed` PRIMA di `close_self`; `host.js` libera il
  buffer (`outputBuffers.close(windowId)`) — un contenuto arrivato dopo la chiusura non deve
  restare in memoria per sempre (mini leak altrimenti, una entry per ogni comando slash lanciato
  e mai più riaperto).

### `capabilities/markdown-window.json`

Lo scope `windows` passa da `["md-*"]` a `["md-*", "help", "output-*"]`: le due nuove label fisse
(il singleton `/help`, le finestre `output-<id>`) non avrebbero altrimenti i permessi IPC
(`take_window_content`, `close_self`, l'API eventi per `output:content`/`output:subscribe`) che
`window.js` richiede — la capability era scoperta solo per il pattern `md-*` generato
dinamicamente.

## Fix wave della review finale del piano 1 (v2.0.3)

Ripulitura dei commenti del frontend copiato dalla v1: ~37 occorrenze in 11 file citavano
ancora `app.js`, il file cancellato in Task 7 (v. sezione sotto) — sostituite con `host.js`,
il suo successore, ovunque descrivessero il contratto ATTUALE (`host.js:19,731` restano
invariati: sono riferimenti storici corretti, "estratto da app.js (v1)"). `path-utils.js` e
`search-status.js` citavano anche `list_path_completions`/`line-editor.js`, entrambi cancellati
insieme al "cursore" v1 — rimossi anche questi riferimenti morti. `config_dir.rs` (`token_path`)
ora usa `startup_config::TOKEN_FILE_NAME` invece della stringa letterale `"token"` duplicata
(anche in `orchestrator::token_store`). Nessun cambio di comportamento: solo commenti e la
costante condivisa.

## Pagina host: overlay F2 rimosso, `host.js`/`host.html`, flag dev `--open` (v2.0.2)

Piano `2026-09-05-piano-1-fondamenta`, Task 7. In 2.0 la shell interattiva vive nel processo
"lare-shell" ospitato da una finestra Tauri separata (spec §5, spike 2) — l'overlay F2 di v1
(`app.js` + `index.html`: editor a riga singola, output scrollabile, spinner di attività,
pulcino idle, diagnosi automatica) non ha più un ruolo. Ma `ui.exe` resta necessario: è
l'unico proprietario della connessione WebSocket "di default" (`Hello` senza `channel`) verso
l'orchestrator, l'unico lanciatore di ogni finestra (`invokeCmd("open_*_window")`), e il
relay che ri-emette `AiChat*`/`Search*`/`Share*`/`OpenPluginWindow` come eventi Tauri globali
verso AI Chat, `/find`, Library e le finestre plugin. Questo task è un'estrazione chirurgica:
il "tenere" di `app.js` migra in `host.js` (owner della finestra `main`, ora nascosta e
chiamata `host.html`), il "cursore" viene cancellato.

### `app.js` (1811 righe) → `host.js` (finestra nascosta) — cosa resta

Estratte **invariate** salvo la rimozione dei riferimenti al DOM del cursore (che non esiste
più in `host.html`, una pagina senza alcuna UI):

`uuidv4`, `invokeCmd`, `getToken`, `initClient` (`onStatus` ridotto a un `console.info` di
diagnostica — non c'è più un badge di stato da aggiornare), `handleServerMsg` (solo i `case`
dei tipi classificati `window`/`relay` da `host-dispatch.mjs` — vedi sotto), `openMarkdownWindow`,
`openSearchWindow`, `emitToSearch`, `setupSearchEvents`, `openPluginWindow`,
`openRoutinePreviewWindow`, `emitToPlugin`, `emitToLibrary`, `libraryRosterParticipants`,
`openAiChatWindow`, `openNoteWindow`, `setupNoteWindowEvents`, `emitToAiChat`, `pushAiChat`,
`setupAiChatEvents`, `setupPluginEvents`, `setupRoutinePreviewEvents`,
`setupConfigWindowEvents`, `setupLibraryEvents`, `bootstrap` (senza editor/focus/duck/diagnosi
— e senza il caricamento della config, che nel cursore v1 serviva solo a vestire il DOM del
cursore stesso: la pagina host non ha aspetto da applicare).

### `host-dispatch.mjs` (nuovo, TDD) — classificazione pura dei `ServerMsg`

`classifyServerMsg(msg)` — nessun DOM, testabile con `node:test` — sostituisce la necessità di
mantenere a mano, sincronizzata con lo switch di `handleServerMsg`, la lista dei tipi che la
pagina host sa gestire. Quattro categorie: `"window"` (apre/aggiorna una finestra),
`"relay"` (va rigirato a una finestra già aperta), `"ignored"` (era per il cursore v1 — output,
watchdog, riga cwd, `server_info` — nessuna superficie lo consuma più), `"deny"`
(`tool_confirm_request`: senza cursore nessuno può rispondere Sì/No, il default sicuro è NO —
l'orchestratore ha comunque un timeout che nega da solo). `handleServerMsg` chiama
`classifyServerMsg` PRIMA di entrare nello switch: `"deny"`/`"ignored"` sono gestiti senza
bisogno di un `case` per ogni tipo, e lo switch contiene solo i tipi che hanno davvero
qualcosa da fare (`open_screener_picker`, presente nel set `"window"` per completezza, non ha
mai un `case`: arriva solo sul canale dedicato di `/markets`, mai sulla connessione di
default).

TDD: `host-dispatch.test.mjs` scritto per primo (i 4 test del brief, verbatim) → `node --test`
fallisce con `ERR_MODULE_NOT_FOUND` (RED, modulo assente) → `host-dispatch.mjs` → 4/4 GREEN.

### Dipendenze che non tornavano pulite — risolte senza inventare comportamento nuovo

Estrarre non è stato un taglia-incolla meccanico: tre punti di `app.js` dipendevano da
superfici del cursore che host.js non ha. In ognuno, la scelta è stata "tieni l'azione sul
wire, togli solo il feedback visivo che non ha più una casa" — mai inventare una nuova
superficie non richiesta dal brief:

- **`search:open-path`/`library:open-folder`** chiamavano `handleSlashCommand(`/open ${path}`)`
  — funzione del cursore, cancellata (il routing degli slash digitati migra
  all'orchestratore nel piano 2). L'UNICA cosa che quella chiamata faceva per `/open` non
  legata al cursore era `client.sendCommand(input, id, webSearch)`; `host.js` la sostituisce
  con un piccolo `sendOpenCommand(path)` che fa solo quello (un WS non connesso finisce in
  console, non più in un banner d'errore nel pannello).
- **`share_result`/`share_incoming_data`** chiamavano `renderer.systemMessage(...)` — la
  stessa doc-comment di `share-view.mjs` dice "riga di esito mostrata nel pannello del
  cursore principale": quella riga non ha più una superficie. `host.js` la sostituisce con un
  `console.info` (diagnostica, non un nuovo canale verso una finestra — sarebbe stato un
  comportamento MAI esistito) e mantiene intatta la parte protocollare (`archive_open`/
  `archive_save` + `sendShareContent(Failed)`/`sendShareWritten`, l'ack che l'orchestratore si
  aspetta).
- **`pushAiChat`** accendeva `hasUnseenAiChatActivity` per pilotare un'icona
  (`updateAiChatNotifyIndicator`) nel DOM del cursore. Senza quell'icona il flag diventa
  write-only (nessun consumatore): non è "logica di relay" da preservare, è stato rimosso
  insieme al suo reset in `setupAiChatEvents`.
- **`setupConfigWindowEvents`** ascoltava `config:saved` per richiamare `applySavedConfig`
  (riapplicare aspetto/flag al pannello del cursore). La pagina host non ha aspetto da
  riapplicare — il listener resta registrato (documenta il contratto per le finestre che
  emettono l'evento) ma con corpo vuoto.

### Rust: rimozione dell'hotkey globale e della clipboard

`main.rs`: tolti `.plugin(tauri_plugin_global_shortcut::Builder::new()...)` (l'handler F2 che
mostrava/nascondeva l'intera superficie UI), `apply_hotkey`, i comandi `hide_overlay`/
`show_overlay`/`list_path_completions`/`home_dir` (e i relativi test — `list_path_completions`
aveva un intero modulo `#[cfg(test)]` in fondo al file), `.plugin(tauri_plugin_clipboard_manager::init())`
(usato solo da `app.js`). `window.rs` (positioning `Center`/`BottomCenter`/`NearMouse`
dell'overlay) è stato CANCELLATO per intero — non era nella lista file del brief, ma
dipendeva solo dall'enum `Position` (rimosso da `config.rs`, sotto) e i suoi unici due
chiamanti (`show_overlay`, l'handler del global-shortcut) sono entrambi spariti: tenerlo
avrebbe rotto la build. `set_config` non valida più un tasto d'attivazione né ri-registra un
hotkey — solo persist + aggiornamento dello stato in memoria. `Cargo.toml`: tolte
`tauri-plugin-global-shortcut`, `tauri-plugin-clipboard-manager`, e `dirs` (usata solo da
`home_dir`, anch'esso rimosso — nessun altro punto del crate la usa).

### `Config` ridotto a due campi

`config.rs`: via `action_key`, `cursor_color`, `cursor_font`, `cursor_size`, `position`,
`activity_indicator`, `idle_duck_minutes` e gli enum `Position`/`ActivityIndicator` che li
tipavano (con i rispettivi test) — restano `web_search_enabled` e `window_alpha`, entrambi
`#[serde(default = ...)]` come prima. Nessun `#[serde(deny_unknown_fields)]` su questo
struct: un `config.json` v1 con i campi rimossi carica lo stesso, ignorando silenziosamente
le chiavi sconosciute (`legacy_v1_config_json_with_extra_fields_still_loads`, test scritto
contro il vecchio `Config` a 9 campi — dove sarebbe stato RED per mancanza di campi
obbligatori — poi verificato GREEN contro il nuovo).

`config-dialog.js`: la tab UI di `/config` mantiene solo Ricerca web e Trasparenza — rimossi
il campo di cattura del tasto d'attivazione (`_buildActionKeyField`/`keyEventToAccelerator`/
`codeToToken`, l'intero meccanismo di cattura tasti), la palette colore
(`_buildColorField`), la selezione font (`_buildFontSelect`), posizione
(`_buildPositionSelect`) e indicatore di attività (`_buildActivitySelect`) — tutti diventati
dead code una volta tolti i campi corrispondenti dall'oggetto `newCfg` salvato. Corretto anche
il testo di aiuto della tab LLM: `llms.json` vive nella cartella di configurazione
(`--config-dir`, di default `Configuration\` accanto all'eseguibile), non più in
`%LOCALAPPDATA%\dev.lare.terminal`/`LARE_LOCAL_DIR` (riferimento v1 mai aggiornato).

### `tauri.conf.json` — finestra `main` diventa la pagina host, nascosta

`"url": "host.html"`, `"visible": false` (era già `false` — cambia il significato: prima
"nascosta finché F2 non la mostra", ora "nascosta per tutta la vita del processo"),
`"skipTaskbar": true`, `"transparent": false`, `"decorations": true`, `"alwaysOnTop": false`,
`200×100` (era `1064×140`, la dimensione compatta del pannello cursore — non ha più senso per
una finestra che non si vede mai). `capabilities/default.json`: tolte le permission
`global-shortcut:*`/`clipboard-manager:*`; le capability delle altre finestre (`aichat-window.json`
… `search-window.json`) restano invariate.

### Flag di sviluppo `--open config|library` (TEMPORANEO, piano 1 → rimosso nel piano 3)

`DevOpenRequest(Option<String>)`, stato gestito popolato una volta in `main()` leggendo argv
reali (stesso pattern di `--config-dir`), letto dal comando `dev_open_request`. Senza overlay
né canale shell nessuna superficie può ancora chiedere l'apertura di `/config` o Library da
sola: `host.js::bootstrap` chiama `dev_open_request` una volta all'avvio e invoca
`open_config_window`/`open_library_window` di conseguenza. Un valore diverso da
`"config"`/`"library"` (o l'assenza del flag) risolve silenziosamente a `None`: nessuna
finestra si apre da sola, comportamento invariato rispetto a prima dell'introduzione del
flag. Il piano 3 lo rimuove quando l'orchestratore guiderà l'apertura delle finestre.

### Verifica manuale (nessun overlay, nessuna hotkey)

`cargo run -p ui -- --config-dir <tempdir-con-token> --open config` per ~10s: nessun crash,
nessun panic (`timeout` termina il processo con exit 124 = ancora in esecuzione). stdout mostra
la cartella di configurazione, la config caricata (solo `web_search_enabled`/`window_alpha`),
l'avvio del watch della Library e la versione — niente più "Press <tasto> to toggle". Nessuna
finestra overlay appare, nessuna hotkey è registrata (il plugin non è più nel builder).
`--open library` risulta nello stesso comportamento. Nota: l'output di `console.*` lato
JS/WebView2 (incluso il log di retry di `ws-client.js`) non è mai stato inoltrato allo
stdout/stderr del processo host — né in v1 né qui — quindi il tentativo di connessione WS
verso un orchestratore assente si osserva in DevTools, non nel terminale.

## Configurazione 2.0: `ConfigDirState`, `get_ws_endpoint`, un solo risolutore (v2.0.1)

Piano `2026-09-05-piano-1-fondamenta`, Task 6. Il fork v1 di questo crate leggeva
`LARE_TOKEN`/`LARE_LOCAL_DIR`/`LARE_ROAMING_DIR`/`LARE_PLUGINS_DIR`/`LOCALAPPDATA` in sei punti
indipendenti, più `app_local_data_dir()`/`app_config_dir()` di Tauri — nessuna garanzia che
concordassero fra loro né con l'orchestrator (Task 4, `RuntimeConfig`) o `mcp-server` (Task 3),
entrambi già migrati allo stesso crate `startup-config` (Task 2).

### `ConfigDirState` — lo stesso pattern "context object" di `RuntimeConfig`

Nuovo modulo `config_dir.rs` (dichiarato SOLO in `main.rs`, non in `lib.rs`: è binary-only,
come `search_settings`/`aichat_settings`/ecc.):

```rust
pub struct ConfigDirState {
    pub config_dir: PathBuf,   // SEMPRE risolto in main(), prima del Builder
    pub startup: StartupConfig,
}
impl ConfigDirState {
    pub fn from_process() -> (Self, Option<String>) { /* config_dir_from_process() + StartupConfig::load */ }
    pub fn ws_endpoint(&self) -> String { format!("ws://127.0.0.1:{}", self.startup.ws_port) }
    pub fn plugins_dir(&self) -> PathBuf { StartupConfig::resolve_path(&self.config_dir, &self.startup.paths.plugins_dir) }
}
pub fn token_path(config_dir: &Path) -> PathBuf { config_dir.join("token") }
pub fn config_file_path(config_dir: &Path) -> PathBuf { config_dir.join("config.json") }
pub fn library_dir_path(config_dir: &Path) -> PathBuf { config_dir.join("library") }
pub fn find_dir_path(config_dir: &Path) -> PathBuf { library_dir_path(config_dir).join("find") }
pub fn read_token(config_dir: &Path) -> String { /* legge, trim, "" se assente/vuoto */ }
```

Gestito da Tauri con `.manage(cfg_state)` in `main()` (prima del `Builder`, così è disponibile
sia a `.setup()` sia a ogni comando via `State<'_, ConfigDirState>`/`app.state::<ConfigDirState>()`).
A differenza dell'orchestrator (`token_store::resolve_token`, che genera il token al primo
avvio), `config_dir::read_token` è SOLA lettura: chi lo crea resta l'orchestrator, `ui` legge
lo stesso file `<config_dir>/token`.

TDD: `config_dir.rs` scritto PRIMA come solo modulo test (con `use super::*` che referenzia
`token_path`/`read_token`/`ConfigDirState`/`StartupConfig` non ancora esistenti) →
`cargo test -p ui config_dir` → 11 errori di compilazione (E0422/E0425/E0433) → RED. Poi
l'implementazione sopra → 3 test GREEN.

### `get_ws_endpoint` — la porta WS non è più hardcoded nel frontend

`ws-client.js` aveva `const WS_URL = "ws://127.0.0.1:7331";` — un SECONDO risolutore della
porta, indipendente da `startup.json.ws_port` e capace di divergerne silenziosamente.
`LareWsClient` ora richiede `url` nel costruttore (nessun default: un default silenzioso
reintrodurrebbe lo stesso problema); i 4 punti che lo costruiscono (`app.js::initClient`,
`config-dialog.js::_runMarketDataTest`, `external-channel-window.js::init`,
`window.js::runExpand`) chiamano `await invoke("get_ws_endpoint")` — nuovo comando Tauri,
`state.ws_endpoint()` — prima di aprire la connessione. `diagnose_connection` verifica la
stessa porta configurata (`state.startup.ws_port`), non più `7331` hardcoded: con una porta
personalizzata in `startup.json`, il vecchio codice avrebbe controllato la porta sbagliata e
mostrato una diagnosi falsa all'utente.

### I 5 moduli settings — un parametro al posto di un risolutore locale

`search_settings.rs`, `market_data_settings.rs`, `llm_settings.rs`, `aichat_settings.rs`
avevano ciascuno una funzione locale (`search_paths_json_path`/`market_data_json_path`/
`llms_json_path`/`app_data_dir`) che ripeteva la stessa catena
`LARE_LOCAL_DIR`→`LOCALAPPDATA`→`.lare-data`. Ora ogni funzione prende `config_dir: &Path` ed è
`config_dir.join(<nome-file>)`; i comandi Tauri corrispondenti guadagnano
`state: State<'_, ConfigDirState>` e passano `&state.config_dir`. `aichat_settings.rs` conserva
il fallback non distruttivo `network.json` → `aichat.json` legacy (`read_existing_settings_json`,
`legacy_aichat_json_path`) — cambia solo la base (`config_dir` invece di `app_data_dir()`), la
logica di migrazione resta identica.

`plugins_view::plugins_dir()` è stata rimossa (non solo modificata): il comando `list_plugins`
usa `state.plugins_dir()` — sparisce anche il caso speciale `LARE_PLUGINS_DIR` (v1: nessun
trim/empty-check su quella variabile, un'incoerenza deliberata mai più necessaria con un solo
risolutore condiviso con l'orchestrator).

### `main.rs` — cablaggio

`config_file_path(app)`/`library_dir_path(app)` mantengono la firma `Result<PathBuf, String>`
(molti chiamanti in questo file usano `?`), ma il corpo diventa infallibile — delega a
`config_dir::config_file_path`/`library_dir_path` con `&app.state::<ConfigDirState>().config_dir`.
`library_find_dir_path` delega a `config_dir::find_dir_path` invece di ricomporre
`library_dir_path(app)?.join("find")` a mano. La diagnostica in `.setup()` che stampava
"Local data dir"/"Roaming data dir" (due etichette per un'unica cartella, dopo questa
unificazione) è sparita: `config dir` resta stampato una volta in `main()`, prima del
`Builder` — `.manage(cfg_state)` è la prima chiamata sulla catena, così sia `.setup()` sia
ogni comando lo trovano già gestito.

### Verifica

`grep -rn 'env::var("LARE_\|LOCALAPPDATA\|APPDATA\|app_local_data_dir\|app_config_dir' src`
vuoto — esteso anche a `config.rs`/`archive.rs` (`ui_lib`), che citavano la vecchia risoluzione
solo in un commento di modulo, mai nella logica (`Config`/l'archivio prendono `&Path` iniettabile
da sempre, invariati). `cargo test -p ui`: 153/153 (100 lib + 53 bin). `node --test
frontend/*.test.mjs`: 269/269 (`ws-client.test.mjs` aggiornato con `url: "ws://127.0.0.1:7331"`
nei 2 costruttori, anche se quei test non chiamano mai `connect()`). `cargo clippy -p ui
--all-targets`: un solo warning, preesistente (`open_routine_preview`, `too_many_arguments`),
verificato via `git stash` che esisteva già prima di questo task.

**Fuori scope, segnalato non corretto**: `connection-diagnosis.js`/`.test.mjs` mostrano ancora
`$env:LARE_TOKEN = "..."` come guida utente e l'etichetta `"porta TCP 7331"`; `app.js` ha un
`console.error` con lo stesso testo. Nessuno di questi file è nell'elenco file del Task 6 —
copy utente, non un risolutore duplicato — impatto pratico nullo col default `ws_port=7331`,
fuorviante solo con una porta personalizzata.

> Piano `2026-08-14-financial-markets-ibkr-data-source`, fix wave post-review
> (`.superpowers/sdd/2026-08-14-financial-markets-ibkr-data-source/review-fix-wave-brief.md`,
> Fix B — CRITICO, e Fix G).
>
> **Fix B — `market_data_settings.rs::merge_active`**: la funzione
> controllava il JSON GREZZO su disco per una chiave `"sources"` valida,
> mentre `get_market_data_settings()`/`parse_market_data_settings` (lettura)
> sintetizzano un default `sources:[{kind:"yfinance"}]` quando quella chiave
> manca. Poiché `set_market_data_settings` (che chiama `merge_active`) viene
> invocata INCONDIZIONATAMENTE a ogni Save del dialog `/config` — il tab Dati
> Mercato mostra sempre almeno la radio YFinance sintetizzata, quindi
> `marketDataRefs.radios.length > 0` in `config-dialog.js` è sempre vero —
> QUALUNQUE salvataggio di QUALUNQUE tab (AI Chat, LLM, ecc.) falliva con
> `"Fonte 'yfinance' non trovata in market_data.json"` se `market_data.json`
> era nello stato "mai toccato il tab Dati Mercato" (es. creato a mano con
> solo `{"active":"yfinance"}`, o da un'altra feature). Fix: prima di
> controllare `kind_exists`, se `obj.get("sources")` non è un array
> valido/non-vuoto E `active == "yfinance"`, `merge_active` ora inserisce
> `obj["sources"] = [{"kind":"yfinance"}]` — stesso identico default
> sintetizzato in lettura — poi procede normalmente. Per qualunque altro
> `active` (tws/ib_gateway) senza `sources[]` corrispondente, l'errore resta
> corretto (comportamento voluto, non si può attivare una fonte che richiede
> port/client_id senza che l'utente li abbia scritti a mano almeno una
> volta) — verificato con un test di non-regressione dedicato
> (`merge_active_still_rejects_non_yfinance_active_without_sources_key`).
>
> **Fix G — nota fuorviante**: "Le modifiche alla fonte dati mercato
> richiedono il riavvio dell'orchestrator" (introdotta in v0.47.0, mai
> corretta prima d'ora) era sbagliata. `market_data.json` è letto FRESCO a
> ogni nuovo processo Python (`market_data_config.build_data_source`, a
> livello di import di `server.py`) — un nuovo processo parte per ogni
> apertura del canale `/markets` o ogni click di "Test connessione", MAI un
> riavvio dell'intero orchestrator (a differenza di `llms.json`/
> `aichat.json`, letti una sola volta all'avvio e tenuti in cache nello
> stato Rust). Testo in `config-dialog.js::_buildMarketDataTab` corretto:
> "Le modifiche alla fonte dati mercato hanno effetto dalla prossima apertura
> di `/markets` o dal prossimo Test connessione — non serve riavviare
> l'orchestrator." Nessun test node:test dedicato (testo statico, stessa
> convenzione delle altre note del dialog).
>
> TDD: due test nuovi in `market_data_settings.rs::tests` — RED verificato
> per `merge_active_synthesizes_default_sources_when_file_exists_without_sources_key`
> (falliva con `"Fonte 'yfinance' non trovata in market_data.json"` prima del
> fix, per il motivo esatto descritto sopra), GREEN dopo.
>
> **Correzione trovata in una seconda self-review (advisor)**: la guardia
> sopra si basava su "`sources` è un array non vuoto?" — insufficiente per
> `{"sources":[{"port":4001}]}` (array non vuoto, ma nessuna voce ha
> `"kind"`): `kind_exists` restava comunque false, stesso errore fuorviante.
> Riscritta come una chiusura `kind_exists(obj) -> bool` (evita di
> duplicare la stessa query due volte, prima e dopo l'eventuale sintesi) e
> la guardia ora si basa su `!kind_exists(obj) && active == "yfinance"` —
> AGGIUNGE `{"kind":"yfinance"}` all'array (`Map::entry("sources").or_insert_with`,
> crea l'array se assente) invece di sostituirlo, cosa che avrebbe fatto
> sparire una voce preesistente come `{"port":4001}` e contraddetto
> `merge_active_writes_only_active_preserving_port_and_client_id`. Se
> `"sources"` esiste ma non è un array (es. una stringa scritta a mano),
> `as_array_mut()` ritorna `None`: nessuna sintesi possibile, l'errore
> scatta correttamente invece di un panic. Nuovo test:
> `merge_active_synthesizes_default_when_sources_entries_have_no_usable_kind`
> — RED verificato, GREEN dopo, verifica anche che la voce preesistente con
> `port` sopravviva (append, non replace).
>
> `cargo test -p ui`: 50 passed; 0 failed (13 in `market_data_settings::tests`
> — vedi `review-fix-wave-report.md` per i numeri completi della suite).

# Implementation — crates/ui v0.47.0 (`/config`: tab "Dati Mercato" + Test connessione)

> v0.47.0: **Debito documentale sanato**, non solo codice nuovo. I commit
> `c98b088` (Task 2: tab) e `a977379` (Task 12: bottone) del piano
> `Docs/superpowers/plans/2026-08-14-financial-markets-ibkr-data-source.md`
> avevano toccato `crates/ui/frontend/` senza il corrispettivo bump
> versione/CHANGELOG/IMPLEMENTATION nello stesso commit — corretto qui in un
> solo commit `docs:`, invece di frammentare la voce su più commit a
> posteriori.
>
> **Tab "Dati Mercato"** (`config-dialog.js:654`, `_buildMarketDataTab`):
> radio-list sola-selezione (YFinance/TWS/IB Gateway), stesso pattern del tab
> "LLM" (`_buildRadio`, nome gruppo condiviso `config-market-data-active`).
> Letta/scritta da `market_data.json` via `get_/set_market_data_settings`
> (comandi Tauri lato `src-tauri`, non toccati da questi due task — già
> esistenti dal lavoro Rust del piano). Nota "richiede riavvio" invariata.
>
> **Bottone "Test connessione (fonte salvata)"**: nuovo metodo privato
> `ConfigDialog._runMarketDataTest(btn, resultEl)`. Apre una `LareWsClient`
> **dedicata e usa-e-getta** — non quella del pannello principale, questa
> finestra `/config` non ne possiede una — con `channel:
> "config-market-data-test"`, mirror letterale del pattern one-shot già
> esistente per `runExpand()`/"library-expand" in `window.js` (stesso
> `settled`-guard contro il doppio fire tra `onMessage`/`onStatus` di
> fallimento, stesso `client.disconnect()` a fine corsa su ogni path). Invia
> `ClientMsg::TestMarketDataSource { id }` (via il nuovo
> `ws-client.js::sendTestMarketDataSource`, mirror minimo di `sendCommand`),
> attende **una sola** `ServerMsg::MarketDataSourceTestResult` correlata per
> `id`, mostra `(ok ? "✅ " : "❌ ") + message`.
>
> **Vincolo del wire protocol, non una scelta UI**: `ClientMsg::TestMarketDataSource`
> non porta alcun campo "sorgente" — testa sempre e solo quella
> ATTUALMENTE SALVATA su `market_data.json` (vedi doc-comment in
> `protocol/src/lib.rs` e l'arm in `orchestrator/src/ws.rs`, Task 10/11,
> invariati da questi due task). Il testo del bottone lo dichiara
> esplicitamente ("fonte salvata") per non promettere di testare una radio
> appena cliccata e non ancora salvata.
>
> **Deviazione corretta durante l'implementazione (Task 12)**: lo snippet del
> brief per `get_lare_token` non aveva guardia try/catch, a differenza di
> ogni altro `_invoke` in `config-dialog.js` — un rifiuto avrebbe lasciato il
> bottone bloccato su "Verifica in corso…" per sempre. Corretto con lo stesso
> pattern già in uso nel file.
>
> **Test**: `ws-client.test.mjs` (nuovo file — nessun altro `sendXxx` in
> questo crate aveva un test dedicato prima) con 2 test di wiring puro sulla
> serializzazione del messaggio JSON sul wire. Suite completa `node --test
> crates/ui/frontend/*.test.mjs`: 269/269 verdi (267 preesistenti + 2 nuovi).
> `cargo build -p ui` pulito, 0 warning.
>
> Nessun cambio lato `orchestrator`/`protocol`: `ClientMsg::TestMarketDataSource`/
> `ServerMsg::MarketDataSourceTestResult`/l'arm in `ws.rs` erano già completi
> (Task 10 · protocol 0.15.4, Task 11 · orchestrator 0.41.18). **Ancora da
> accettare dal vivo dal supervisore**: smoke test grafico reale (click
> bottone, disabilitazione/riabilitazione, esito a schermo con YFinance e con
> IBKR) — nessun ambiente Tauri disponibile ai subagenti, stesso trattamento
> già riservato agli altri tab `/config` di questo piano.

# Implementation — crates/ui v0.46.8 (AI Chat: nickname affiancato all'etichetta macchina)

> v0.46.8: **Disambiguazione nickname in AI Chat.** Trovato nel primo smoke test
> dal vivo multi-macchina di v0.46.7 (Task 9): con solo `display_name` mostrato,
> due macchine con lo stesso nickname sono indistinguibili in chat — capitato
> davvero in diretta, entrambe le AI di rumpleteazer e skimble si erano scelte
> "Clio". Stesso problema per un umano collegato da due PC diversi.
>
> **`aichat-view.mjs` — `messageLine`**: nuova funzione privata `machineLabel(from_label)`
> spoglia il suffisso finale `-human`/`-ai` da `from_label` (regex
> `/-(human|ai)$/`) per isolare l'etichetta macchina (`"skimble-ai"` →
> `"skimble"`). `messageLine` ora compone `label` come `` `${display_name} /
> ${machineLabel(from_label)}` `` quando `display_name` è presente (es. `"Clio /
> skimble"`), invece del solo `display_name`. Senza `display_name`, fallback
> INVARIATO su `from_label` grezzo — nessuna regressione sul path storico/peer
> che non mandano ancora il campo. `is_ai` resta un booleano separato per il
> badge visivo: nessuna concatenazione testuale, la distinzione umano/AI resta
> solo grafica (decisione già presa in v0.46.7, confermata qui).
>
> **Test** (`aichat-view.test.mjs`, TDD: RED prima dell'implementazione — 3
> fallimenti sui casi con `display_name`, verificato che l'unico caso senza
> `display_name` restasse GREEN): il test preesistente `"messageLine usa
> display_name quando presente"` riscritto per il nuovo formato; 2 nuovi — uno
> che verifica il formato anche per un umano (non solo per l'AI), uno che
> verifica ESPLICITAMENTE la disambiguazione (stesso `display_name`, due
> `from_label` diversi → due `label` finali diversi). Suite completa `node
> --test crates/ui/frontend/*.test.mjs`: 267/267 verdi dopo il cambio, nessuna
> regressione sui moduli vicini (`aichat-push`, `share-view`, ecc.).
>
> Nessun cambio lato `orchestrator` o sul wire (`ChatMsg`/`ChatLine` invariati,
> `protocol` non toccato): `display_name`/`is_ai` arrivavano già correttamente
> al frontend dal fix del Task 9 (v0.46.7) — qui si cambia solo come vengono
> COMPOSTI in `label` per il render. Unico consumatore: `aichat-window.js:62`
> (`line.label` → `textContent`, invariato). Verificato dal vivo:
> rumpleteazer↔skimble↔quaxo dopo rebuild+redeploy, nickname disambiguati
> correttamente in chat.

# Implementation — crates/ui v0.46.7 (AI Chat: nickname/badge nel rendering, Task 9/9 finale)

> v0.46.7: **AI Chat mostra `display_name` + badge `is_ai`** (design
> `Docs/superpowers/specs/2026-08-13-aichat-display-names-design.md`, Task 9 di 9,
> ULTIMO del piano). I campi sono sul wire dal Task 2 (`protocol`) e
> risolti lato `orchestrator` dal Task 3; questo task li fa arrivare al
> rendering nella finestra AI Chat.
>
> **`aichat-view.mjs` — `messageLine`** (logica pura, testata):
> firma estesa da `{from_label, text}` a `{from_label, text, display_name,
> is_ai}`. Ritorna `{label, text, is_ai}`: `label = display_name ?? from_label`
> (nullish coalescing — `display_name` vince solo se non è `null`/`undefined`,
> una stringa vuota resterebbe comunque `label`, ma la validazione lato
> `AiChatSettings`/orchestrator già impedisce nickname vuoti a monte);
> `is_ai: Boolean(is_ai)` normalizza `undefined` (peer/storico vecchi che non
> mandano il campo) a `false`, coerente col default `#[serde(default)]` lato
> Rust su `ChatLine::is_ai`.
>
> **`aichat-window.js` — 2 bug reali, confermati leggendo il codice PRIMA di
> scrivere qualunque modifica (non ipotesi da brief), entrambi corretti:**
>
> 1. `addLine({from_label, text})` destrutturava SOLO 2 dei 4 campi del
>    payload in arrivo (da `e.payload` sull'evento Tauri `aichat:msg`, o da
>    ogni entry nel loop di `aichat:history`) — `display_name`/`is_ai`
>    ARRIVAVANO nel payload (dopo il fix di app.js, vedi sotto) ma venivano
>    scartati silenziosamente dalla destrutturazione stessa, prima ancora di
>    raggiungere `messageLine`. Corretto estendendo la firma della funzione
>    a tutti e 4 i campi e inoltrandoli a `messageLine({from_label, text,
>    display_name, is_ai})`.
> 2. `isSelf(line.label, myBase)` passava il label GIÀ RISOLTO da
>    `messageLine` — dopo questo task, potenzialmente un nickname come
>    `"Maurizio"` — a `isSelf`, che verifica `label.startsWith(`${myBase}-`)`
>    (confronta contro l'etichetta TECNICA, es. `"skimble-"`). Un nickname
>    libero non inizia mai per costruzione con quel prefisso: il confronto
>    era semanticamente sbagliato. **Il difetto è oggi LATENTE, non un
>    regresso osservabile**: in questa finestra `myBase` è hardcoded a
>    `""` (`const myBase = "";` riga 36, mai riassegnato — commento
>    preesistente: "base etichetta locale non nota lato finestra → no
>    self-highlight"), quindi `isSelf(qualunque_cosa, "")` riduce a
>    `label.startsWith("-")`, sempre falso per etichette reali: la classe
>    CSS `.self` non si applica a NESSUN messaggio, né prima né dopo questo
>    fix — verificato leggendo il file, non ipotizzato. Corretto comunque
>    passando `from_label` (l'etichetta GREZZA, non processata da
>    `messageLine`) a `isSelf`, mentre `line.label` (potenzialmente un
>    nickname) resta riservato al testo mostrato: la chiamata è ora corretta
>    per il momento in cui `myBase` verrà popolato (la finestra riceve già
>    la propria etichetta via l'evento `aichat:self` → variabile `selfLabel`,
>    righe 42/124 — derivarne `myBase` sbloccherebbe `.self` "gratis", ma è
>    una decisione di scope che NON ho preso: fuori dal Task 9, segnalata nel
>    report al supervisore).
>
> `div.className` ora aggiunge anche `" ai"` quando `line.is_ai` è vero,
> accanto a `" self"` — le due classi sono indipendenti (un messaggio può
> essere "mio" E dell'AI, se l'AI parlasse a nome della propria macchina in
> futuro, anche se oggi non capita nella pratica).
>
> **Terzo bug reale, trovato eseguendo la verifica esplicitamente richiesta
> dal brief ("altri punti di consumo oltre a quelli citati") — non era nel
> brief:** `app.js`, dispatcher `handleServerMsg`, case `"ai_chat_message"`.
> Il case ricostruiva a mano l'oggetto payload da inoltrare come evento Tauri
> `aichat:msg` (`pushAiChat("aichat:msg", {from_label: msg.from_label, text:
> msg.text})`), scartando `display_name`/`is_ai` che sono comunque presenti
> su `msg` (JSON piatto del wire, `ServerMsg::AiChatMessage` ha `#[serde(tag
> = "type")]`, non nidificato). Anche con `addLine` corretto (bug 1 sopra),
> i messaggi LIVE non avrebbero MAI portato nickname/badge — solo lo storico
> via `ai_chat_history` (che inoltra `entries: msg.entries`, l'intera
> `ChatLine` con tutti i campi) ne sarebbe stato immune. Corretto includendo
> esplicitamente `display_name: msg.display_name, is_ai: msg.is_ai` nel
> payload inoltrato. Nessun test unitario diretto per questo case (`app.js`
> è wiring/dispatch DOM-adiacente, stesso pattern già in uso per l'intero
> file — la logica pura testata vive nei moduli `.mjs` che `app.js` importa,
> non nel dispatcher stesso).
>
> **`aichat-window.html`**: nuova regola CSS `.line.ai .lbl` (colore
> etichetta distinto, `rgba(200, 170, 255, 0.85)` — violetto, per non
> collidere col verde già usato da `.line.self .lbl`) + `.line.ai
> .lbl::before { content: "🤖 "; }` (prefisso emoji + spazio). Nessuna
> concatenazione testuale nel JS: la distinzione resta SOLO visiva/CSS,
> scelta di design esplicita nel brief — un nickname scelto dall'AI è un
> nome proprio a tutti gli effetti, un suffisso testuale lo renderebbe
> ambiguo.
>
> **Verifica "altri punti di consumo" (richiesta esplicita nel task,
> ripetendo il pattern che nei Task 7/8 aveva trovato gap reali)**: grep di
> `messageLine`/`aichat-view.mjs` su tutto `crates/ui/frontend/` conferma
> `aichat-window.js` come UNICO importatore. Grep di `aichat:msg`/
> `aichat:history`/`addLine(` conferma solo i 2 listener già noti (nessuna
> altra finestra — Library, Markdown, plugin — consuma queste righe).
> `aichat-push.js` (gate emit/buffer/drop per l'apertura ritardata della
> finestra) tratta `payload` come opaco, non lo ispeziona né lo altera —
> nessun rischio di perdita campi lì.
>
> TDD: 3 nuovi test in `aichat-view.test.mjs` — RED confermato (`node --test
> aichat-view.test.mjs` prima dell'implementazione: 4 fallimenti, il test
> preesistente + i 3 nuovi, tutti per assenza di `is_ai`/uso di
> `display_name` in `messageLine`), poi GREEN dopo l'implementazione. Il
> test preesistente `messageLine passa label e text` aggiornato per
> aspettarsi `is_ai: false` nell'oggetto ritornato — cambiamento di
> comportamento INTENZIONALE di questo task (non una regressione su un
> campo esistente: `roster`/`consentPrompt`/`historyEntries`/
> `chatWindowTitle`/`peerLostText` invariati, verificato dai rispettivi
> test ancora verdi).
>
> `node --test crates/ui/frontend/aichat-view.test.mjs`: 14/14 verdi.
> `node --test crates/ui/frontend/*.test.mjs` (intera suite JS, 18 file):
> 265/265 verdi, nessuna regressione.
>
> **Non eseguito in questo task (per istruzione esplicita)**: lo smoke test
> dal vivo (Step 7 del brief) — spetta al supervisore umano dopo che
> l'intero piano a 9 task è mergiato, non a un singolo task del piano.
>
> **Nota fuori scope, osservata ma non toccata**: il commit del Task 6
> (`7bd7f17`, campi nickname in `config-dialog.js`) non ha aggiunto una voce
> a questo CHANGELOG/IMPLEMENTATION.md — l'ultima voce prima di questa era
> ancora 0.46.6/Task 5. Non è compito di questo task colmare quel gap
> retroattivamente; segnalato nel report per il supervisore.

# Implementation — crates/ui v0.46.6 (`AiChatSettings`: mirror display_name/ai_display_name)

> v0.46.6: **Nickname umano + AI, mirror `display_name`/`ai_display_name` in
> `AiChatSettings`** (design `Docs/superpowers/specs/2026-08-13-aichat-display-names-design.md`,
> Task 5 di 9). 2 nuovi campi `Option<String>` su `AiChatSettings`
> (`src-tauri/src/aichat_settings.rs`), mirror "sul filo" di
> `orchestrator::aichat::config::AiChatConfig::{display_name,ai_display_name}`
> (Task 1-4, già mergiati lato `orchestrator`) — stessi nomi/tipi/semantica, ma
> tipo Rust separato: `ui` e `orchestrator` sono processi/crate distinti che
> comunicano SOLO tramite lo stesso `network.json` su disco, mai tramite una
> dipendenza di crate diretta.
>
> **`display_name`**: nickname dell'umano al posto della bare label tecnica
> (es. "skimble") in AI Chat. **`ai_display_name`**: nickname dell'AI di questa
> macchina; normalmente popolato dal tool `set_ai_display_name` (Task 7, non
> ancora costruito) invece che da `/config`, ma resta editabile a mano in ogni
> momento — le due strade scrivono sullo stesso campo JSON, nessun conflitto
> possibile.
>
> `get_aichat_settings` legge entrambi come `None` quando la chiave è assente
> (file assente/corrotto, o `network.json` scritto prima che i due campi
> esistessero) — stesso pattern infallibile già usato per gli altri campi, ma
> con `Option::None` come default invece di un valore concreto.
>
> `merge_aichat_settings` scrive i due nickname con `insert`/`remove` — a
> differenza degli altri campi (sempre `insert`), un nickname `None` viene
> RIMOSSO dal JSON (`obj.remove(...)`) invece di scriversi come chiave `null`:
> un nickname cancellato dall'utente deve sparire dal file, più pulito di una
> chiave `null` che comunque `get_aichat_settings` tratterebbe come assente.
>
> Nuova funzione pura `validate_nickname(&Option<String>, field_label: &str)`,
> condivisa da entrambi i campi (chiamata due volte in `validate`, una per
> `display_name` una per `ai_display_name`, col rispettivo `field_label` per
> distinguere il messaggio d'errore): `None` è SEMPRE valido ("nessun
> nickname" è lo stato di default, non un errore); se `Some`, trim + non vuoto
> + lunghezza 1..=48. **Nessuna restrizione di charset** — a differenza di
> `label_base` (`[A-Za-z0-9_-]`, perché diventa `"<label_base>-human"` sul
> wire) il nickname è testo libero mostrato solo in UI, mai un identificatore
> tecnico: spazi e accenti sono validi ("Maria José", "Àlex").
>
> `set_aichat_settings` applica lo stesso trattamento già riservato a
> `label_base` (trim) ai due nickname, con un'aggiunta: stringa vuota-dopo-trim
> → convertita in `None` (`.filter(|s| !s.is_empty())`) invece di essere
> validata e rifiutata — così l'utente può "cancellare" un nickname scrivendo
> solo spazi nel campo, senza dover distinguere a mano fra "cancella" e
> "lascia invariato".
>
> 5 nuovi test unitari (TDD RED→GREEN — RED per errore di compilazione E0560/
> E0063: i 2 campi non esistevano ancora su `AiChatSettings`, poi mancavano
> dai 9 struct-literal preesistenti nel modulo):
> `merge_round_trip_preserves_display_names`,
> `get_settings_reads_display_names_with_none_default`,
> `validate_rejects_display_name_too_long`,
> `validate_accepts_display_name_with_spaces_and_accents` (quest'ultimo
> verifica esplicitamente che spazi/accenti siano accettati, a differenza di
> `label_base`),
> `deserializes_payload_without_display_name_keys` (aggiunto in self-review su
> suggerimento dell'advisor — PASSA subito, non un RED genuino: documenta che
> serde_derive tollera GIÀ l'assenza della chiave per un campo `Option<T>`
> senza `#[serde(default)]`, quindi il payload che `config-dialog.js` manda
> oggi — PRIMA del Task 6, senza `display_name`/`ai_display_name` — deserializza
> comunque a `None` invece di fallire con "missing field"). I 9 struct-literal
> `AiChatSettings { .. }` preesistenti nel modulo (produzione + test)
> aggiornati ai 2 nuovi campi.
> `cargo test -p ui` (intera suite, non solo il modulo): 137/137 verdi, nessuna
> regressione. `cargo clippy -p ui --all-targets`: pulito (l'unico warning
> presente, `too_many_arguments` su `open_routine_preview` in `main.rs`, è
> preesistente e non toccato da questa slice).
>
> **Rischio noto nella finestra Task 5 → Task 6** (non un bug di questa
> slice, comportamento inevitabile finché il frontend non manda i 2 campi):
> siccome `newAiChat` in `config-dialog.js` (riga ~368) non include ancora
> `display_name`/`ai_display_name`, ogni deserializzazione in questa finestra
> produce `None` per entrambi → `merge_aichat_settings` prende il ramo `None`
> → `obj.remove(...)` → un nickname eventualmente già presente in
> `network.json` (scritto a mano, o in futuro dal tool `set_ai_display_name`
> del Task 7) verrebbe CANCELLATO al primo salvataggio di `/config` fatto in
> questa finestra. **Non fare uno smoke test dal vivo di un salvataggio
> `/config` con un nickname impostato finché il Task 6 non è mergiato.**
>
> **Solo backend `ui` (Rust) in questo task** — nessun campo ancora
> raggiungibile da `/config`: il Task 6 (frontend) aggiungerà i due input nel
> tab AI Chat di `config-dialog.js`. Nessuna modifica a `search_settings.rs`
> né ad altri moduli: `aichat_settings.rs` è l'unico file toccato.

# Implementation — crates/ui v0.46.2 (heartbeat frontend + watchdog riconosce SSE ping)

> v0.46.2: **Frontend wiring di `ServerMsg::Heartbeat`** (protocol 0.15.1 · orchestrator 0.41.2).
> Nuovo case `"heartbeat"` nel dispatcher `handleServerMsg` (`app.js`) che riusa
> `commandChunkReceived(msg.id)` senza stampare nulla — consente al watchdog di 180s di
> distinguere un turno AI **genuinamente ancora in lavorazione** (con keep-alive SSE `ping`
> dal server) da un silenzio reale. Corregge il falso positivo dove un documento lungo veniva
> annullato automaticamente dopo 184s pur essendo ancora generato. Design:
> `Docs/superpowers/specs/2026-08-07-ai-turn-heartbeat-watchdog-design.md §5`.

## Dispatch serverMsg in handleServerMsg (v0.46.2+)

Case lista: `"server_info"`, `"chunk"`, `"heartbeat"` (nuovo), `"done"`, `"error"`, `"pong"`,
`"tool_confirm_request"`, `"open_window"`, `"routine_save_preview"`, `"notes_snapshot"`,
`"note_upserted"`, `"ai_chat_*"` (7 varianti), `"external_tool:*"` (custom event prefix),
default→`console.warn`.

# Implementation — crates/ui v0.46.0 (routine-preview window — Fase 2 save_routine)

> v0.46.0: **Finestra di anteprima dedicata per save_routine** (orchestrator 0.41.0 ·
> protocol 0.15.0 ·
> mcp-server 0.7.0). Quando l'AI propone di salvare una routine PowerShell, una finestra
> separata (`routine-preview.html`/`.js`) mostra nome/descrizione/categoria/tag/script
> (textContent only, nessun HTML) e pulsanti Salva/Annulla. I pulsanti emettono un evento
> Tauri globale (`"routine-preview:decision"`) che `app.js` inoltra come la STESSA
> `ClientMsg::ToolConfirmResponse` già usata dal banner Sì/No del cursore — zero nuovi
> messaggi WS. Design:
> `Docs/superpowers/specs/2026-08-05-save-routine-design.md §5`.

## Finestra routine-preview (v0.46.0 — save_routine Fase 2)

**Tauri command: `open_routine_preview(...)`** (`main.rs`). Riceve i dati della routine
(id/name/description/tags/category/script/replace) come parametri e serializza un JSON
composto nello slot `content` di `WindowContentStore` (kind = `"routine_preview"`). Riusa
il meccanismo one-shot di `take_window_content` come `open_plugin_window` e
`open_markdown_window` — nessuna nuova infrastruttura. La finestra apre a dimensione
640×560, chromeless, trasparente, sempre in primo piano.

**HTML/JS: `routine-preview.html`/`.js`** (nuovi). Al caricamento, `routine-preview.js`
chiama `take_window_content`, legge il JSON e popola i campi con `textContent` (MAI
innerHTML): script dall'AI = intrinsecamente sicuro senza sanitizzazione. Se `replace`
è presente, mostra un banner giallo "Sostituisce/aggiorna: <nome>". Pulsanti Salva/Annulla
(e tasto Esc/✕) emettono `tauriEvent.emit("routine-preview:decision", {id, accept})`
che `app.js` (`setupRoutinePreviewEvents`) ottiene e inoltrada come `client.sendToolConfirmResponse(id, accept)`.

**Nessun nuovo messaggio WS**: riusa la risposta già progettata per `tool_confirm_request`
(il banner Sì/No del cursore). L'orchestrator vede una `ToolConfirmResponse` indistintamente
da quale interfaccia UI (routine-preview window vs cursore) l'ha generata.

**`app.js` wiring**: nuovo case `"routine_save_preview"` nel dispatcher `handleServerMsg`;
funzione helper `openRoutinePreviewWindow` che invoca il comando Tauri; listener
`setupRoutinePreviewEvents` registrato al bootstrap.

**Verifica**: `cargo check -p ui` pulito, 254 test `node:test` verdi (nessun nuovo test
— plumbing DOM/Tauri, strutturalmente invisibile ai test unitari, stesso limite
di `plugin-window.js` e `aichat-window.js`). Smoke test dal vivo in Task 11.

## Review finale save_routine: allineamento ai pattern aichat/note-window (v0.46.1)

Tre fix su `routine-preview.js` per allinearla ai pattern già corretti dal vivo nelle
altre finestre secondarie, più una capability Tauri mancante trovata in verifica.

- **C1 (critical):** `emitDecision` fire-and-forget correva contro `close_self`
  (che tronca il canale IPC della webview) — un Salva poteva perdere la corsa e
  arrivare "perso" ad app.js, facendo scadere la conferma lato orchestrator dopo
  180s nonostante il click. Ora `emitDecision` è `async` e ogni chiamante
  (Salva/Annulla/✕/Escape) la `await`a prima di chiudere — stesso pattern già
  corretto dal vivo in `aichat-window.js`/`note-window.js`.
- **I1:** aggiunto un listener `beforeunload` (best-effort, non atteso) che
  chiama `emitDecision(false)` — backstop per chiusura Alt-F4/task-manager/barra
  che bypassa sia ✕ sia Escape, prima lasciata scoperta nonostante il commento
  in testa al file la dichiarasse coperta.
- **I2:** aggiunto il boilerplate `--window-alpha` standard (`get_config`
  all'avvio + listener `config:saved`), copiato da `note-window.js` — la
  finestra referenziava già la CSS var ma nulla la impostava mai.
- **Capability Tauri mancante (trovata in verifica, non uno dei tre finding
  sopra — bloccava l'INTERA finestra, non solo I2):** nessun file in
  `capabilities/` copriva il pattern di label dinamico `routine-preview-*`
  (`open_routine_preview`, `main.rs`) — a differenza di ogni altra finestra
  secondaria a label dinamica (`md-*`/`plugin-*`/`search-*`, ciascuna col
  proprio file). Senza un `windows` match Tauri nega di default ogni permesso
  IPC: `take_window_content`/`close_self`/`get_config`/emit-listen sarebbero
  stati bloccati a prescindere dai tre fix sopra. Aggiunto
  `capabilities/routine-preview.json` (stesso pattern di
  `search-window.json`/`note-window.json`).

## Finestra "Nuova/Modifica nota" separata (v0.45.41)

Fix #3 del primo smoke test dal vivo: sostituisce `#note-dialog`
(`<dialog>` HTML dentro `library.html`, DOM figlio del webview Library —
inamovibile, ridimensionabile solo dall'angolo della textarea) con una vera
`WebviewWindow`, label singleton `"note-compose"`. Nessuna nuova
infrastruttura Tauri: riusa lo schema esistente di `open_aichat_window`/
`open_plugin_window` (`WindowContentStore` + `take_window_content` per il
contenuto iniziale, `close_self`/`resize_self` già generici) e l'hub eventi
di `app.js`.

**`open_note_window(note_id: Option<String>, title: String, text: String)`**
(`main.rs`): stasha `(title, text, kind, "")` in `WindowContentStore` —
`kind` porta l'id nota o `""` per una nota nuova — PRIMA di creare/focalizzare
la finestra. Se la finestra esiste già (l'utente riapre "Modifica" su
un'altra nota mentre quella corrente è ancora aperta), `take_window_content`
non tornerebbe più nulla di fresco (canale one-shot, consumato al primo
avvio): il caso è coperto da un `win.emit("note-window:load", {...})`
diretto — sicuro perché a quel punto il JS della finestra è di sicuro già
in ascolto (non c'è la race che `take_window_content` risolve al PRIMO
avvio). Aggiunto anche al loop F2 hide/show insieme a `"library"`.

**`note-window.html`/`.js`** (nuovi): modellati su `aichat-window.html`/`.js`
— chromeless, `data-tauri-drag-region` sulla titlebar, nessuna `ws-client.js`
(parla solo con app.js via eventi Tauri), stesso boilerplate `--window-alpha`
+ `config:saved`. La textarea usa `flex:1; resize:none` invece del vecchio
`resize:vertical`: è la FINESTRA a essere ridimensionabile dai bordi nativi
del SO (`resizable(true)` sul builder), non più un handle CSS sulla
textarea — quello era esattamente il finding #3 ("ridimensionabile solo
dall'angolo del campo nota multilinea, non dal bordo della finestra").
Escape chiude direttamente (`document.addEventListener("keydown", ...)`):
a differenza del vecchio `<dialog>`, qui non c'è una finestra padre da
proteggere con uno `stopPropagation` — quel workaround esisteva solo perché
il dialog viveva nello STESSO documento DOM della finestra Library.

Riusa `noteEditMessages`/`isTitleValid`/`isBodyValid` (`note-view.mjs`,
v0.45.40) per la decisione "cosa salvare" — stessa logica testata, nessuna
duplicazione fra il vecchio dialog (rimosso) e la nuova finestra.

**`app.js`**: `openNoteWindow(note)` invoca `open_note_window`;
`setupNoteWindowEvents()` ascolta `"note-window:save"` e inoltra a
`client.sendNoteCreate`/`sendNoteEdit`/`sendNoteEditTitle`. In
`setupLibraryEvents()`, le vecchie `library:note-create`/`note-edit`/
`note-edit-title` (dirette dal dialog) sostituite da un'unica
`library:note-window-open` (richiesta apertura finestra, dal pulsante
"Nuova" o da ✏️); `library:note-delete` resta invariata.

**Capability**: `capabilities/note-window.json`, stesso schema di
`aichat-window.json` (`core:default` + `core:window:allow-start-dragging`,
`"windows": ["note-compose"]`). Nessuna modifica a `tauri.conf.json` (ogni
finestra secondaria di questa app è creata a runtime, non dichiarata
staticamente) né a `Cargo.toml` (nessuna nuova dipendenza).

**Verifica**: `cargo build -p ui` pulito, 254 test `node:test` verdi
(nessun nuovo test — `note-window.js` è plumbing DOM/finestra,
strutturalmente invisibile ai test unitari, stesso limite già noto per
`plugin-window.js`). Smoke test dal vivo multi-macchina resta da fare.

## `noteEditMessages` estratta e testata (v0.45.40)

`frontend/note-view.mjs` guadagna `noteEditMessages({title, text,
originalTitle, originalBodyText}) → {editText?, editTitle?}`: la decisione
"cosa mandare al salvataggio" del dialog nota, prima logica inline
nell'handler `noteSaveBtn` (`library.js`) e unica parte del dialog non
coperta da `node:test` — esattamente la parte appena toccata dal fix 0.45.39
(confronto contro l'originale invece che contro vuoto). 5 test in
`note-view.test.mjs`. `library.js` chiama la funzione pura invece di
duplicarne la logica; comportamento invariato (254 test node verdi).

## Fix #1/#2 primo smoke test dal vivo del Blocco note (v0.45.39)

Due dei tre finding del primo smoke test multi-macchina reale.

**#1 — testo nota mancante in "Modifica" (`frontend/library.js`).**
`openNoteDialog(note)` precompila ora sia `noteTitleInput` sia
`noteTextInput` da `note.my_segment_text` (protocol 0.14.9 / orchestrator
0.40.69 — v. i loro CHANGELOG), non più titolo-solo con corpo sempre vuoto.
`noteEditState` guadagna `originalBodyText` accanto a `originalTitle`, per
poter confrontare al salvataggio ciò che l'utente vede davvero.

Effetto a cascata sull'handler `noteSaveBtn` (FIX 3 della review finale di
branch, v0.40.xx): il test "il corpo è cambiato?" era `text.trim() !== ""`
— corretto quando la textarea partiva sempre vuota (vuoto = "niente da
aggiungere"), ma con un valore precompilato reale diventa un bug speculare:
un utente che cancella DELIBERATAMENTE tutto il testo per svuotare il
proprio segmento vedrebbe il salvataggio ignorato in silenzio, come se
niente fosse cambiato. Corretto a `text !== noteEditState.originalBodyText`
— confronto contro il valore catturato all'apertura del dialog, non contro
la stringa vuota.

**#2 — stile pulsante "Nuova"/toolbar Note (`frontend/library.html`).**
Task 17 aveva aggiunto `#new-note-btn`/`#note-toolbar` senza CSS dedicato —
non allineati al resto del programma. `#new-note-btn` unito al blocco di
stile condiviso con `#new-folder-btn`/`#reload-btn`; nuovo blocco
`#note-toolbar` a specchio di `#lib-toolbar` esistente (stesso padding,
bordo, sfondo con `--window-alpha`).

Finding #3 (dialog "Nuova/Modifica nota" → vera finestra Tauri separata,
spostabile/ridimensionabile oltre i confini della finestra Library) resta
aperto — cambio architetturale più ampio, trattato a parte.

## Fix race `peerNetworkEnabled` (v0.45.38)

`loadNoteTab` (`frontend/library.js`) legge `get_aichat_settings` in parallelo
al listener `library:notes-snapshot`, senza attendere l'uno prima dell'altro.
Se la lettura del config risolveva dopo che uno snapshot live era già
arrivato, il valore letto dal file (`enabled: false`, tipico quando l'utente
ha cambiato `network.json` senza ancora riavviare l'orchestrator) poteva
sovrascrivere un `peerNetworkEnabled = true` già confermato dal vivo — l'UI
avrebbe mostrato "rete disabilitata" con la funzione di fatto attiva.

Fix: la lettura del config non scrive più sopra un `true` già confermato
(guardia `peerNetworkEnabled !== true` prima dell'assegnazione, riga ~1716).
Solo lo snapshot live (prova che `AiChatService` è davvero acceso) può
affermare "acceso"; il file di config può solo affermare "spento" o lasciare
lo stato invariato — mai retrocedere un "acceso" già osservato.

Trovato come Minor non bloccante nella review finale del branch (v0.45.37),
parcheggiato lì, corretto qui su richiesta esplicita prima dello smoke test
dal vivo. Nessun test automatico (stessa classe DOM/Tauri delle altre
correzioni di quella review — verificato leggendo, non eseguendo; la
frontend suite esistente, 249/249, resta verde perché nessun test copre
questa specifica race).

## Fix di review finale del branch "Blocco note" (v0.45.37)

Quattro gap di INTEGRAZIONE fra task, invisibili alle review dei singoli task
(ognuno dei quali era passato). File toccati:
`frontend/library.js`, `frontend/library.html`, `frontend/config-dialog.js`,
`src-tauri/src/aichat_settings.rs`.

- **FIX 2 — corpo nota mai mostrato** (`buildNoteItem`). `note.body` era
  plumbato da Rust (`render_body` → `NoteView.body` → payload evento Tauri →
  oggetto JS) e poi mai renderizzato: la funzione era **di sola scrittura**.
  Ora la riga include un `<div class="note-item-text">` con `textContent`
  (mai `innerHTML`: è testo che può arrivare da un'altra macchina della rete
  peer). CSS in `library.html`: `white-space: pre-wrap` — non cosmetico, è
  necessario, perché il corpo fuso di più macchine contiene righe di
  intestazione `— macchina —` che senza a-capo collasserebbero illeggibili;
  più `overflow-wrap: anywhere` (URL lunghi) e `max-height: 12em` +
  `overflow-y: auto` (note lunghe scorrono nel loro riquadro invece di
  allungare la riga). Corpo vuoto → nessun elemento, niente spaziatura
  fantasma. Nessun collapse/expand: la nota si legge senza click extra.
- **FIX 3 — salvataggio condizionale** (`openNoteDialog`/`wireNoteDialog`).
  Vedi la correzione dettagliata nella sezione v0.45.35 sopra: `noteEditState`
  porta ora anche `originalTitle` (catturato all'apertura); `note-edit` parte
  solo se `text.trim() !== ""` (payload **grezzo**: il `trim()` serve solo al
  test di emptiness, trimmare il payload butterebbe via indentazione voluta),
  `note-edit-title` solo se il titolo differisce dall'originale; se non è
  cambiato nulla, "Salva" chiude senza traffico. Limite noto e accettato: un
  aggiornamento remoto che arriva a dialog aperto lascia `originalTitle`
  stantio — il merge LWW lato orchestrator risolve comunque, e non vale la
  complessità di risincronizzare un dialog aperto.
- **FIX 4 — `/config` → AI Chat puntava ancora ad `aichat.json`**
  (`aichat_settings.rs`). Il file non era nella touch-point table del design né
  in nessuna lista di task, quindi la rinomina a `network.json` (Task 1) non lo
  aveva mai raggiunto: il tab leggeva e scriveva un file che l'orchestrator
  ignora (`network.json` vince sempre), rendendo **impossibile abilitare la
  funzione dalla UI preposta**. Ora rispecchia la stessa migrazione non
  distruttiva di `aichat::config::load_or_generate_with_migration`:
  `network_json_path()` per la scrittura (sempre e solo), lettura via
  `read_existing_settings_json()` — `network.json` se **esiste** (test di
  esistenza, non "la lettura è riuscita", identico al precedente lato
  orchestrator), altrimenti fallback a `legacy_aichat_json_path()`. Il
  fallback vale anche per il merge in scrittura, così i campi extra del
  vecchio file (es. `peer_ttl_secs`) vengono travasati invece che persi.
  `merge_aichat_settings`/`validate` non sono stati toccati.
- **FIX 6 — "Nessuna nota." mentiva a rete spenta** (`loadNoteTab`/
  `renderNoteList`). Con `enabled: false` (il default) il servizio AI Chat non
  parte, quindi nessun `NotesSnapshot` arriva mai: il tab mostrava lo stesso
  messaggio di "hai zero note", un vicolo cieco silenzioso. Non esiste un
  segnale sul filo WS che lo distingua; si legge invece il comando Tauri già
  esistente `get_aichat_settings` (che **dopo il FIX 4** guarda lo stesso
  `network.json` dell'orchestrator). La variabile `peerNetworkEnabled` è a
  **tre stati**: `null` = non so (o lettura fallita) → non si afferma nulla e
  il pulsante "Nuova" resta attivo; `true` → lista vuota significa davvero
  zero note; `false` → messaggio che nomina file, campo e dove cambiarlo, e
  "Nuova" disabilitato (meglio un pulsante visibilmente spento di uno che
  accetta il click e non fa nulla).
  **Due sorgenti, non una:** `loadNoteTab` legge il config (com'è
  CONFIGURATO), ma il listener `library:notes-snapshot` imposta anche
  `peerNetworkEnabled = true` — uno snapshot che arriva è la prova più forte
  che il servizio sia davvero SU (solo `AiChatService` lo emette, e parte solo
  con `enabled: true`). Senza questa seconda sorgente, dopo aver abilitato la
  rete e riavviato l'orchestrator il pulsante "Nuova" resterebbe disabilitato
  — senza motivo visibile — finché l'utente non esce dal tab e ci rientra: la
  lettura del config avviene SOLO all'attivazione della scheda.

## Cap 256 KB sul corpo nota, validato client-side (v0.45.36)

**Task 20 del piano SDD (self-review addendum)**.
Enforce il cap di design (§9) sul lato client prima di inviare un'operazione di
crea/modifica nota:

- **Funzione `isBodyValid`** in `note-view.mjs`: misura il byte-length del
  testo con `TextEncoder`, confrontato col limite 256 KB. I caratteri multi-byte
  UTF-8 pesano più sulla rete/serde rispetto alla lunghezza JS (`.length`);
  `TextEncoder.encode()` fornisce il byte-count reale.
- **Test** in `note-view.test.mjs`: verifica che esattamente 256 KB passi e
  256 KB + 1 byte fallisca.
- **Integrazione** in `library.js` `wireNoteDialog`: se `isBodyValid` fallisce,
  mostra un errore (`"⚠ Testo troppo lungo (max 256 KB)."`) e ritorna senza
  inviare l'evento Tauri, mirror esatto del comportamento della validazione
  titolo (`isTitleValid`).

## Rendering + dialog + wiring WS per Blocco note (v0.45.35)

**Task 19 del piano SDD (Blocco note, ultimo task frontend sostanziale)**.
Completa `crates/ui/frontend/library.js` per la scheda "Note":

- **Rendering** (`loadNoteTab`/`renderNoteList`/`buildNoteItem`): a differenza
  di Markdown/Find/Plugins (che PULLano via `invoke`), le note arrivano
  spinte via eventi Tauri (`library:notes-snapshot`/`library:note-upserted`,
  Task 16). `loadNoteTab` renderizza subito `currentNotes` e in parallelo
  chiede un refresh esplicito (`library:request-notes`) per coprire il caso
  in cui lo snapshot sia arrivato prima dell'apertura della finestra.
  `renderNoteList` usa `visibleNotesSorted` (Task 18, `note-view.mjs`) per
  filtrare le tombstoned e ordinare per data. `buildNoteItem` costruisce la
  riga con `textContent` per titolo/meta (mai `innerHTML` — dati rete/utente).
- **Dialog composizione** (`openNoteDialog`/`wireNoteDialog`): mirror esatto
  del pattern `wireMoveDialog`/`wireShareDialog` — singleton listener
  registrato una volta a bootstrap, stato esterno (`noteEditState`) letto al
  click, stesso gotcha Esc (`stopPropagation` sul `keydown` del dialog PRIMA
  che bubbli al document handler che chiuderebbe l'intera finestra Library).
  Il dialog "Modifica" precompila SOLO il titolo — il corpo parte vuoto
  (semplificazione deliberata: `NoteView`, Task 6, espone solo il `body` fuso
  di tutte le macchine, non un breakdown per-macchina).
  **Correzione (v0.45.37, FIX 3 della review finale di branch):** questa nota
  affermava che «il comportamento resta corretto anche precompilando vuoto —
  solo meno comodo». Era **falso** come implementato: "Salva" inviava
  incondizionatamente sia `note-edit` sia `note-edit-title`, quindi la
  textarea vuota diventava il nuovo segmento di questa macchina — una
  cancellazione silenziosa del proprio contributo al corpo ogni volta che si
  correggeva solo il titolo. Il comportamento è ora davvero corretto perché
  l'invio è CONDIZIONALE (v. la sezione v0.45.37 sopra): una textarea lasciata
  vuota significa "non ho nulla da aggiungere al mio segmento" e non viene
  inviata affatto. Precompilare vuoto è quindi, adesso sì, solo meno comodo.
- **Tab switching**: `activateTab` include ora `tabNoteEl` nel loop di
  stato attivo, mostra/nasconde `#note-toolbar` in base al tab, e dispatcha a
  `loadNoteTab`.
- **WS listeners**: `library:notes-snapshot` sostituisce `currentNotes` per
  intero (replay completo); `library:note-upserted` fa upsert per id in
  `currentNotes`. Entrambi ri-renderizzano solo se `activeTab === "note"`.

Smoke test end-to-end single-machine (Step 6 del brief) **NON eseguito in
questo commit**: l'ambiente agent che ha svolto il Task 19 non ha
interazione GUI reale (nessun modo di guidare `cargo tauri dev` a mano).
Verificato invece per via statica: schema payload di ogni evento Tauri
(`library:note-create/-edit/-edit-title/-delete/-request-notes` e
`library:notes-snapshot`/`library:note-upserted`) incrociato con i listener
già presenti in `app.js` (Task 16) — combacia campo per campo. Il primo
smoke test dal vivo resta da fare (Task 21 del piano, o prima). Dettagli e
concern noti in `task-19-report.md`.

**Fix post-review (stesso commit/versione, due findings su codice letterale
del brief — non un errore del subagente, confermato dal coordinatore):**

- **Layout riga nota**: `.archive-item` è flex ORIZZONTALE
  (`justify-content: space-between`, per separare testo da pulsanti); il
  codice letterale del brief appendeva `titleEl`/`metaEl` come figli diretti
  della riga, quindi finivano affiancati invece che impilati. Fix: nuovo
  sotto-contenitore `.note-item-body` (`flex-direction: column`) che li
  avvolge, CSS aggiunto in `library.html`.
- **Eliminazione a un click**: il codice letterale del brief eseguiva
  `library:note-delete` al primo click, inconsistente con
  `wireDeleteButton`/`wireFolderDeleteButton` (doppio click di conferma,
  usati ovunque altrove nel file). Fix: nuova `wireNoteDeleteButton(deleteBtn,
  noteId)`, stesso meccanismo (classe `--confirm`, testo "Conferma", timeout
  3s) ma non un riuso diretto di `wireDeleteButton` — quella è accoppiata a
  `invokeCmd(deleteCmd, { file })` (awaitable, con `onSuccess`/reload),
  mentre l'eliminazione nota è un `tauriEvent.emit` fire-and-forget (Task
  16); adattare localmente lo stesso meccanismo era il cambio più piccolo e
  fedele.
- **Classi pulsanti**: ✏️/🗑 ora usano le classi hover esistenti del file
  (`.archive-item-move`/`.archive-item-delete`) invece di restare senza
  classe (chrome di default del browser).

Verificato: `node --test crates/ui/frontend/*.test.mjs` (248/248, nessuna
regressione — questi test non coprono il DOM di `library.js`, quindi il fix
non ne intacca la copertura). Cronologia completa del fix in
`task-19-report.md`.

## Modulo puro note-view.mjs (v0.45.34)

**Task 18 del piano SDD (Blocco note)**. Crea il modulo puro `crates/ui/frontend/note-view.mjs`
(nessuna dipendenza da DOM, Tauri, I/O) con tre funzioni:

- `visibleNotesSorted(notes)`: filtra le note cancellate (l'orchestrator le tiene in memoria
  per il protocollo di riconciliazione, la UI non le mostra mai) e ordina per `created_at_ms`
  decrescente (più recente prima).
- `isTitleValid(title)`: valida il titolo — non vuoto dopo `trim()`, massimo 200 caratteri
  (cap del design §9).
- `formatNoteMeta(createdBy, createdAtMs)`: formatta la riga di metadati mostrata sotto il
  titolo nella lista ("macchina — data/ora").

Testato con `node:test` (6 test, tutti passanti). Segue la convenzione di `share-view.mjs`:
modulo puro testato in isolamento, nessuna logica UI. Task 19 lo importerà per il rendering.

## Markup tab "Note" + dialog composizione (v0.45.33)

**Task 17 del piano SDD (Blocco note)**. Aggiunge la quarta tab `Note` alla
Library (accanto a `Markdown | Find | Plugins`), e il dialog `#note-dialog`
per la composizione/modifica delle note. Il dialog riusa le classi CSS
`.move-dialog-box` e `.move-dialog-footer` già presenti nel file (identiche a
quelle usate dai dialog "Sposta" e "Condividi"), aggiungendo solo un nuovo
`<input>` per il titolo (cap 200 char) e una `<textarea>` per il corpo della
nota. Nessuna logica/rendering ancora — markup puro. Task 18-19 aggiungono la
logica e il wiring verso JS.

## Trasporto WS per Blocco note (v0.45.32)

**Task 16 del piano SDD (Blocco note)**. Strato trasporto puro: quattro nuovi
metodi `send*` su `LareWsClient` (`sendNoteCreate`, `sendNoteEdit`,
`sendNoteEditTitle`, `sendNoteDelete`) che rilanciano i messaggi di protocollo
verso l'orchestrator, due nuovi `case` nello switch `handleServerMsg` di
`app.js` (`notes_snapshot`/`note_upserted`) con cache `lastKnownNotes` per il
pattern pull-on-open (identico a roster/reachable-peers), e cinque listener
Tauri in `setupLibraryEvents` per il bidirezionale Library←→app.js←→orchestrator.

Nessuna UI resa ancora — Task 17-19 aggiungono la parte visibile
(libraryhtml + note-view.mjs + library.js).

## Fix: corsa emit-vs-close nella chiusura della finestra AI Chat (v0.45.31)

**Bug osservato dal vivo** (2 macchine, subito dopo il merge dell'icona busta
v0.45.30, confermato riproducibile dall'utente): la primissima notifica
funzionava (busta accesa correttamente per la prima richiesta di ammissione,
finestra mai aperta prima). Da lì in poi, ogni messaggio arrivato con la
finestra chat REALMENTE chiusa (× cliccato, confermato esplicitamente
dall'utente — non una finestra rimasta in background) non accendeva più la
busta, su entrambe le macchine, ripetutamente.

**Investigazione** (systematic-debugging): tracciata l'intera catena
`requestOpen()`/`ready()`/`closed()` — inclusa l'apertura via riga hint
cliccabile (stesso `handleSlashCommand`, nessuna scorciatoia che bypassa
`requestOpen()`), nessun secondo punto che tocca `hasUnseenAiChatActivity`
oltre ai due previsti. Un solo punto resta sospetto e non escludibile da
sola lettura statica (nessun harness per testare una corsa IPC fra due
processi Tauri): `aichat-window.js::closeWindow()`:

```javascript
// PRIMA (bug):
function closeWindow() {
  stopRerequestTimer();
  emit("aichat:closed", {});               // fire-and-forget
  invoke?.("close_self").catch(() => {});  // distrugge la finestra subito dopo
}
```

`emit()` è IPC asincrona, mai attesa — nulla garantisce che `"aichat:closed"`
raggiunga `app.js` PRIMA che `close_self` distrugga la finestra (e col suo
canale IPC). Se la distruzione vince la corsa, `aiChatGate` (`aichat-push.js`)
resta bloccato su `live=true` per sempre: ogni `push()` successivo ritorna
`{action:"emit"}` invece di `"drop"`, e `pushAiChat` fa `return` PRIMA di
controllare `isNotifiableDrop` — la busta non può più accendersi, qualunque
sia il vero stato della finestra.

**Problema strutturale a monte:** il wrapper locale `emit(event, payload)`
faceva `tauriEvent?.emit(event, payload)` **senza `return`** — nessun
chiamante avrebbe potuto attenderlo anche volendo, la Promise veniva
scartata a prescindere.

**Fix:**
```javascript
function emit(event, payload) {
  return tauriEvent?.emit(event, payload);   // ora ritorna la Promise
}

async function closeWindow() {
  stopRerequestTimer();
  await emit("aichat:closed", {});           // atteso PRIMA della distruzione
  invoke?.("close_self").catch(() => {});
}
```

Il percorso `beforeunload` (chiusura NON dal nostro × — Alt+F4 o simile)
resta fire-and-forget deliberatamente: lì la finestra è già in fase di
distruzione esterna, non c'è nulla da attendere con garanzie reali.

**Nessun test automatico** — una corsa IPC fra due processi Tauri non ha
harness in questo progetto. Verifica dal vivo richiesta (stesso scenario
del bug: chiudere per davvero la finestra chat col ×, verificare che la
busta si accenda per il messaggio successivo, su entrambe le macchine).

## Icona busta per attività AI Chat non vista (v0.45.30)

**Obiettivo:** nella finestra principale, un'icona busta (✉) accanto allo
status badge di connessione (`#status-badge`) segnala che è arrivato un
evento AI Chat rilevante mentre la finestra chat era chiusa (messaggio,
richiesta di condivisione, una delle due richieste di ammissione). Nato
da un'osservazione dell'utente dopo i due bug Condividi/gate-2 corretti
nella stessa sessione: senza aprire la chat non c'era alcun modo di
sapere che qualcosa la aspettava lì. Design:
`Docs/superpowers/specs/2026-07-28-aichat-unseen-notification-indicator-design.md`.

**Perché estendere `aichat-push.js` (Task 1) invece di un modulo nuovo:**
il gate esistente (`createAiChatPushGate`) già classifica ogni evento
`ai_chat_*` in `"emit"`/`"buffer"`/`"drop"` — la domanda "questo drop
merita una notifica?" è una domanda sullo STESSO stato (finestra
aperta/richiesta/né l'una né l'altra), non un concetto nuovo. Un modulo
separato avrebbe dovuto ridondare la classificazione o accoppiarsi al
gate; la funzione pura vive dove il dato che consulta (`action`) nasce
già:

```javascript
// aichat-push.js (Task 1)
export function isNotifiableDrop(event, action) {
  return action === "drop" && NOTIFIABLE_EVENTS.has(event);
}
```

**Perché esattamente 4 eventi notificabili:** solo `aichat:msg`,
`aichat:share-request`, `aichat:join-prompt`, `aichat:admission-request`
rappresentano un'azione umana in attesa (qualcuno ha scritto, vuole
condividere, o aspetta un voto di ammissione — in entrambe le direzioni
del gate a 2 fasi). Gli altri eventi `ai_chat_*` che passano DAVVERO da
`pushAiChat` (`roster`, `history`, `self`, `join-request` — mai emesso,
residuo pre-Task 8 —, `admission-resolved`, `pending`, `admitted`,
`rejected`, `peer_lost`) sono sincronizzazione di stato passiva, filtrata
da `isNotifiableDrop`. `reachable-peers` è un caso a parte: non passa
nemmeno da `pushAiChat`/`aiChatGate` — il suo `case` nel dispatch di
`app.js` inoltra direttamente a `emitToLibrary("library:reachable-peers",
...)`, un canale separato per la finestra Library (vedi
`2026-07-28-library-share-reachable-peers-design.md`). Non notifica per
costruzione, non per essere stato filtrato qui. L'elenco degli eventi
filtrati è stato verificato leggendo il `switch` completo delle chiamate a
`pushAiChat` in `app.js` in fase di design, non enumerato a memoria.

**Wiring in `app.js` — `pushAiChat` guadagna il controllo sull'azione:**

```javascript
function pushAiChat(event, payload) {
  const { action } = aiChatGate.push(event, payload);
  if (action === "emit") {
    emitToAiChat(event, payload);
    return;
  }
  if (isNotifiableDrop(event, action)) {
    hasUnseenAiChatActivity = true;
    updateAiChatNotifyIndicator();
  }
}
```

`hasUnseenAiChatActivity` è in-memory, azzerata a ogni avvio di `ui.exe`
— stesso trattamento del resto del gate (che si azzera già su
`closed()`), nessuna persistenza tra riavvii (dichiarato fuori scope). Il
listener `"aichat:ready"` (si attiva quando la webview della chat
registra i suoi listener e rigioca il buffer) azzera l'indicatore nello
stesso momento in cui l'utente sta per vedere tutto ciò che era in
attesa.

**UI — `index.html`:** nuovo `<span id="aichat-notify" class="aichat-notify
hidden">`, riusa la classe `.hidden` già esistente nel file (stessa di
`#stop-btn`, `display: none`) — nessun nuovo meccanismo di show/hide.
Comportamento deciso in brainstorming: **solo presenza** (✉ compare/
scompare), nessun contatore numerico (un numero non aggiunge
informazione utile allo scopo "c'è qualcosa che aspetta"); click
sull'icona **non** apre la chat — indicatore passivo, l'utente apre la
finestra come fa già oggi (`/aichat` o riga hint cliccabile).

Test: `node --test crates/ui/frontend/*.test.mjs` → 242/242 verdi (235
pre-esistenti + 7 di Task 1 per `isNotifiableDrop`), nessuna regressione.
Nessun nuovo test in questo task: il cablaggio DOM/dispatch in
`app.js`/`index.html` non ha harness automatico nel progetto — stesso
limite già incontrato più volte in questa sessione. Verifica riservata
allo smoke test dal vivo (Step 10 del piano, a cura dell'utente: far
arrivare uno dei 4 eventi da un'altra macchina a finestra chat chiusa,
confermare comparsa/scomparsa di ✉).

## Library "Condividi": raggiungibilità di rete, non ammissione chat (v0.45.29)

**Bug utente:** "Condividi" mostrava "Nessuna macchina connessa" con
discovery+elezione AI Chat già riusciti. **Root cause** (lato Rust,
`crates/orchestrator/IMPLEMENTATION.md` v0.40.51 ha il dettaglio
completo): il dialog leggeva il roster di AMMISSIONE alla stanza chat,
che il backend di condivisione (`request_share`) non ha mai consultato —
usa discovery+link TCP, entrambi automatici, senza consenso umano.

**Fix qui (solo frontend, il backend è la parte pesante — Task 2 del
piano):** canale Tauri nuovo e SEPARATO da quello del roster chat, mirror
esatto del pattern esistente:

```javascript
// app.js
let lastKnownReachablePeers = null;
// nel dispatch:
case "ai_chat_reachable_peers":
  lastKnownReachablePeers = msg.labels;
  emitToLibrary("library:reachable-peers", { labels: msg.labels });
  break;
// risposta a richiesta esplicita:
tauriEvent.listen("library:request-reachable-peers", () => {
  emitToLibrary("library:reachable-peers", { labels: lastKnownReachablePeers ?? [] });
});
```

```javascript
// library.js
let currentReachablePeers = [];
tauriEvent.listen("library:reachable-peers", (e) => {
  currentReachablePeers = e.payload?.labels ?? [];
});
// al bootstrap, accanto alla richiesta roster esistente:
tauriEvent.emit("library:request-reachable-peers", {});
// openShareDialog, un solo cambio:
const targets = shareTargetList(currentReachablePeers); // era: shareTargetList(currentRoster)
```

`shareTargetList` (già testata, `share-view.mjs`) resta byte-identica:
accetta un array di etichette `-human`/`-ai`, le spoglia e dedupe —
`currentReachablePeers` ha già quella forma (il backend appende
`-human`).

**Perché un canale separato e non riusare `library:roster`:** quel
roster alimenta ANCHE la finestra AI Chat ("chi è presente nella
stanza") — semantica corretta per quello scopo, non va toccata. Un
segnale nuovo, non una reinterpretazione di uno esistente.

`currentRoster`/`"library:roster"` restano nel file (morti per
`openShareDialog` dopo questo fix) — rimozione dichiarata fuori scope
nella spec, per un diff minimo e reversibile.

Test: `node --test crates/ui/frontend/*.test.mjs` → 235/235 verdi.
Nessun nuovo file di test: il dispatch `switch(msg.type)`/forward Tauri
è cablaggio a specchio di un pattern già in produzione, non testato
isolatamente nemmeno per il roster originale — stesso livello di
copertura, non un regresso. Verifica reale: smoke test dal vivo a 2
macchine (ancora da fare con l'utente).

## Ordinamento cliccabile colonne nelle finestre Markdown (v0.45.28)

**Obiettivo:** la tabella "Selezione di oggi" del documento `screen_stocks`
(canale Financial Markets, `scripts/pytools/financial-markets/`) ottiene
due azioni per colonna — freccia ▲ (crescente) e freccia ▼ (decrescente) —
su tutte le colonne tranne Pos. Click reale nella finestra già aperta,
senza rilanciare il tool Python (che cambierebbe anche la selezione, non
solo l'ordine). Design:
`Docs/superpowers/specs/2026-07-28-financial-markets-screen-stocks-sortable-table-design.md`.

**Vincolo architetturale che detta tutto il meccanismo:** le finestre
Markdown sono statiche — `window.js` fa `marked.parse() →
DOMPurify.sanitize() → innerHTML` (ADR-013). DOMPurify di default rimuove
ogni attributo `on*`, quindi un `onclick` inline nel Markdown generato da
Python è impossibile. L'interattività deve vivere in `window.js` — file
**condiviso da ogni finestra Markdown** dell'app (nmap, stock_report,
list_stocks, documenti Library), non solo da `screen_stocks`.

**Contratto di markup** (prodotto da `screening.py::_table`, consumato da
`window.js` — vedi anche `crates/orchestrator`... non toccato: il contratto
`PythonReportJson`/`split_report` non cambia, solo il Markdown restituito
contiene più HTML grezzo di prima):

```html
<th>Score <span class="sort-arrow" data-dir="asc" data-type="num">▲</span><span class="sort-arrow" data-dir="desc" data-type="num">▼</span></th>
...
<td data-pos>1</td>
<td data-sort-value="234.56">234,56</td>
```

`data-sort-value` porta il valore grezzo (non formattato); **assente**
quando il dato è n/d — il JS lo tratta come sempre-ultimo, in qualunque
direzione. `data-pos` marca la prima cella di ogni riga (il contatore
posizione, MAI sortabile: è un progressivo ricostruito, non un dato della
riga).

**`table-sort.mjs`** (nuovo, puro, testato `node:test` — 9 test): un solo
export, `sortedOrder(keys, dir, type)`. `keys` è l'array parallelo di
`data-sort-value` per una colonna (o `null` dove l'attributo è assente).
Partiziona indici presenti/mancanti PRIMA di ordinare, così i mancanti
finiscono sempre in coda indipendentemente dalla direzione — la logica di
direzione (`dir === "asc" ? cmp(a,b) : -cmp(a,b)`) si applica solo al
sottoinsieme presente. `Array.prototype.sort` è stable da ES2019+: nessuna
logica di pareggio scritta a mano, i pareggi mantengono l'ordine visuale
precedente.

**`window.js`** — listener delegato, installato **una volta** a livello di
modulo su `#content` (mai sostituito da un'assegnazione `innerHTML`, quindi
sopravvive a ogni `renderMarkdown()` successivo incluso il re-render di
"Espandi"). Al click su un elemento `.sort-arrow`:
1. `arrow.closest("th")` → `th.cellIndex` dà l'indice colonna — **nessun
   mapping nome-colonna** da mantenere sincrono col Python che genera la
   tabella: se un giorno una colonna cambia posizione nell'header, il JS
   la segue automaticamente.
2. `arrow.closest("table")` scopa il riordino alla tabella cliccata (utile
   se un documento futuro avesse più tabelle sortabili).
3. Per ogni riga del `tbody`, legge `row.cells[colIndex].dataset.sortValue`
   (`undefined` → normalizzato a `null` prima di passarlo a `sortedOrder`).
4. `tbody.appendChild(row)` per ogni riga, in sequenza secondo l'ordine
   calcolato — `appendChild` su un nodo già presente nel DOM lo **sposta**
   in coda (comportamento nativo `Node.appendChild`), quindi iterare in
   sequenza riproduce esattamente l'ordine finale voluto senza
   rimuovere/ricreare nodi.
5. Rinumera la cella `data-pos` di ogni riga a `newIndex + 1` — Pos rifletta
   sempre l'ordine VISUALE corrente, non il piazzamento di merito
   originale (decisione utente in brainstorming).

**`window.html`**: regola CSS `.sort-arrow`/`:hover` (cursore a mano,
opacità che aumenta in hover) — nessun cambiamento strutturale.

**Perché nessun ridimensionamento finestra dopo un sort:** il numero di
righe non cambia, solo l'ordine — l'auto-height calcolato al bootstrap
resta valido.

**Perché nessuna corsa con la fusione `defer_to_turn_end`:** per
`screen_stocks` la finestra si apre SOLO dopo che il testo dell'AI è già
fuso nel documento (architettura esistente) — non esiste uno stato
intermedio in cui l'utente potrebbe cliccare un sort prima che il documento
finale sia pronto.

**Eseguito con subagent-driven-development** (3 task: Python/markup,
JS puro, JS wiring+CSS; implementer Sonnet/Haiku, review per-task +
whole-branch review su Opus). 0 Critical/Important sul codice in tutte le
review. Minor loggati (non bloccanti, nessuno compone in un problema più
grave a livello di branch): repr float grezzo in `data-sort-value` per
metriche calcolate (es. `0.19999999999999996` per un drawdown) — non
influisce sul `parseFloat` lato JS; nessun HTML-escaping di nome/ticker/
settore nell'attributo — comportamento preesistente (la vecchia pipe-table
interpolava le stesse stringhe grezze), verificato innocuo per nomi reali
con `&` (DOMPurify lo neutralizza in `&amp;`, il browser lo decodifica
correttamente prima di `localeCompare`); test di `localeCompare` non
esaustivo (dati di test che non distinguono da un confronto lessicografico
naive) e nessun test di stabilità su pareggio in direzione desc/testo —
gap nella lista test del piano, logica sottostante verificata corretta a
mano dal reviewer; CSS `.sort-arrow` non scoped su `#content` come le
regole sorelle — innocuo, la classe non compare altrove nel chrome della
finestra.

**Smoke test dal vivo ancora da fare con l'utente** (unico modo di
verificare che DOMPurify v3.2.5 preservi davvero `data-sort-value`/
`data-dir`/`data-type`/`data-pos` in un webview reale — la spec
`option_chain.py` citata come precedente usa solo `style`/`colspan`, non
`data-*`, quindi questo è la prima conferma live per attributi `data-*` in
questa app): click ▲/▼ su almeno 3 colonne (una testuale, una numerica con
qualche n/d, Score), verifica riordino + rinumerazione Pos + valori n/d
sempre in coda.

Test: `node --test crates/ui/frontend/*.test.mjs` → 235/235 verdi (226
preesistenti + 9 nuovi in `table-sort.test.mjs`).

## Rimosso `activeTagName` morto da `restoreActiveField` (v0.45.19)

Review finale del branch plugin crittografia (minor #2): `activeTagName`
era documentato nella JSDoc di `restoreActiveField` (introdotto in v0.45.18,
sezione sotto) e popolato dal chiamante in `plugin-window.js::render()`, ma
mai letto nel corpo della funzione — il controllo difensivo "l'elemento
trovato è INPUT/TEXTAREA?" (punto 3 dell'algoritmo sotto) confronta il
`tagName` del NUOVO elemento ritrovato con sé stesso, non con
`activeTagName` del vecchio. Nessun confronto vecchio-vs-nuovo esiste nel
codice: il parametro era puro overhead documentale.

Rimosso per YAGNI (nessun comportamento cambia, era un campo morto):
- `plugin-runtime.mjs`: tolta la riga `activeTagName: string, // ...` dalla
  JSDoc di `restoreActiveField` (la firma della funzione già non lo
  destrutturava — riceveva `preserved` intero e leggeva solo i campi che
  usa).
- `plugin-window.js::render()`: tolta `activeTagName: active.tagName,`
  dalla costruzione dell'oggetto `preserved`. Il gate che decide SE
  costruire `preserved` in primo luogo (`active.tagName === "INPUT" ||
  active.tagName === "TEXTAREA"`, poche righe sopra) resta invariato — è
  lì per un altro motivo (decidere se c'è un campo di testo a fuoco),
  indipendente dal campo rimosso.
- `plugin-runtime.test.mjs`: tolto `activeTagName: "..."` dai 7 literal
  `preserved` passati a `restoreActiveField` nei test che ne costruivano
  uno — non era mai asserito, solo passato in ingresso senza effetto sul
  comportamento testato. L'ottavo test di `restoreActiveField` (il caso
  `preserved` nullo/undefined) non costruisce un oggetto, quindi non aveva
  nulla da rimuovere.

Refactor puro, nessun nuovo comportamento: nessun nuovo test TDD richiesto
(la regola RED→GREEN si applica a comportamento nuovo o bugfix, non alla
rimozione di un parametro mai consultato). Verifica: `node --test
crates/ui/frontend/*.test.mjs` → 211/211 verdi, stesso conteggio di prima
(nessun test aggiunto né rimosso, solo un campo tolto dai fixture).

## Fix: plugin field loses live-typed characters on every re-render (v0.45.18)

**Bug (osservato dal vivo, Task 11):** nel pannello "Testo in chiaro" del
plugin crittografia, digitando, ogni carattere appariva per un istante e
spariva — restava sempre visibile un solo carattere alla volta.

**Causa:** ogni tasto premuto fa un round-trip completo attraverso il
sidecar plugin — nessun patching parziale del DOM, `render()` in
`plugin-window.js` sostituisce SEMPRE l'intera finestra via
`sanitizeAndRender` (full-replace strategy, vedi sopra). `render()` aveva
GIÀ un meccanismo (costruito per un bug precedente — "il campo perde il
focus mentre scrivo") che cattura `{evt, start, end}` dell'elemento attivo
PRIMA di `sanitizeAndRender` e, dopo il render, ritrova il nuovo elemento
via `data-evt` per ripristinargli focus+selectionRange. Questo meccanismo
NON copriva però il `.value`: il campo veniva lasciato con il valore
ritornato dal round-trip, che riflette sempre lo stato di UN TASTO PRIMA
rispetto a quanto l'utente aveva già digitato localmente (l'evento `input`
è asincrono verso il plugin — vedi il round-trip descritto sopra per
`eventFromTarget`). Ogni tasto successivo, più veloce del giro completo
tastiera→WS→plugin→WS→render, arrivava PRIMA che il round-trip del tasto
precedente tornasse indietro — quel round-trip in ritardo sovrascriveva
sistematicamente quanto già digitato.

### Nuova funzione pura `restoreActiveField(preserved)` in `plugin-runtime.mjs`

Estrae la logica di ripristino — prima inline dentro `render()` — in una
funzione pura (nessun accesso a `window`/`document`/`CSS.escape`), seguendo
lo stesso pattern delle altre funzioni del modulo (dependency injection: la
radice DOM in cui cercare è un parametro, non un global).

```js
restoreActiveField({
  activeTagName,      // "INPUT" | "TEXTAREA" dell'elemento che aveva il focus
  activeDatasetEvt,   // il suo data-evt (identità semantica, sopravvive al render)
  activeValue,        // il valore LIVE catturato da active.value PRIMA del render
  activeStart,        // selectionStart catturato prima del render
  activeEnd,          // selectionEnd catturato prima del render
  newRootElement,     // radice DOVE cercare il nuovo elemento (iniettata dal chiamante)
})
```

Algoritmo:
1. No-op immediato se `preserved` è nullo/assente o `activeDatasetEvt` è
   assente (nessuna identità semantica per ritrovare l'elemento).
2. Cerca `newRootElement.querySelector('[data-evt="<escaped>"]')`. L'escape
   NON usa `CSS.escape` del browser (non esiste come global in Node — questo
   modulo deve restare privo di globali browser per restare testabile con
   `node:test` senza jsdom): un piccolo escape locale (`\` e `"` → `\\` e
   `\"`) è sufficiente e corretto per un valore già dentro un selettore ad
   attributo tra virgolette (a differenza di `CSS.escape`, pensato per
   produrre identificatori CSS validi ed escapa molti più caratteri del
   necessario, es. `:`).
3. No-op se l'elemento non è stato ritrovato o non è INPUT/TEXTAREA
   (difensivo — un `data-evt` riusato su un tipo diverso non dovrebbe mai
   accadere, ma la funzione non lo assume).
4. **Ordine delle operazioni — parte del contratto, non un dettaglio:**
   `.value` PRIMA, poi `.focus()`, poi `.setSelectionRange(start, end)`
   PER ULTIMA. Scrivere `.value` su un `<input>`/`<textarea>` reale può
   normalizzare o troncare una `selectionRange` impostata in precedenza (il
   browser non garantisce che sopravviva a un cambio di value) — impostarla
   prima verrebbe silenziosamente vanificato.

### Modifica in `plugin-window.js`

`render()` cattura ora anche `active.value` (oltre a
`activeTagName`/`activeDatasetEvt`/`activeStart`/`activeEnd`) PRIMA di
`sanitizeAndRender`, poi chiama:

```js
restoreActiveField({ ...preserved, newRootElement: pluginRoot });
```

al posto della logica di ripristino inline precedente (che copriva solo
focus+selectionRange). `{ ...null, newRootElement: pluginRoot }` è
sicuro — lo spread di `null`/`undefined` in un literal oggetto non lancia,
produce solo `{}` — quindi quando nessun campo aveva il focus,
`restoreActiveField` riceve `{ newRootElement }` senza `activeDatasetEvt` e
fa no-op immediatamente.

**Scoping — riguarda SOLO il campo che aveva il focus:** `restoreActiveField`
interroga il DOM per un unico `data-evt` (quello catturato). Un'altra
`<textarea>` nella stessa finestra plugin (es. "Testo cifrato" mentre
l'utente digita in "Testo in chiaro") non viene mai toccata da questa
funzione — riceve il valore fresco renderizzato da `sanitizeAndRender`
esattamente come prima di questo fix, invariata.

### Test: `plugin-runtime.test.mjs` (+8, TDD RED→GREEN)

`plugin-window.js` resta non testabile direttamente con `node:test` (importa
`window.__TAURI__` al top-level — fallirebbe fuori da un contesto Tauri),
quindi tutta la logica nuova è stata scritta ed è coperta nel modulo puro:

- Il valore LIVE catturato sopravvive al re-render (non quello, in ritardo,
  presente nel nuovo DOM).
- Focus e selectionRange vengono ripristinati oltre al valore.
- **Ordine esplicito:** l'indice della chiamata `value` precede l'indice
  della chiamata `setSelectionRange` (asserzione diretta sull'ordine, non
  solo sull'esito finale).
- No-op su: `preserved` nullo/undefined, `activeDatasetEvt` mancante (e in
  quel caso il DOM non viene nemmeno interrogato), nessun elemento
  corrispondente trovato, elemento trovato ma non INPUT/TEXTAREA.
- Un `data-evt` contenente una virgoletta letterale non rompe il selettore
  costruito internamente (escaping difensivo verificato sul selettore
  esatto ricevuto dal `querySelector` fake).

RED verificato: `import { ..., restoreActiveField }` falliva con
`SyntaxError: ... does not provide an export named 'restoreActiveField'`
prima dell'implementazione (fallimento per il motivo giusto: la funzione
non esisteva ancora, non un typo). GREEN dopo l'implementazione.

Intera suite: `node --test crates/ui/frontend/*.test.mjs` → 211/211 verdi
(203 pre-esistenti + 8 nuovi, nessuna regressione).

**Verifica senza una finestra Tauri reale:** questo fix non è stato eseguito
dentro il webview Tauri in questa sessione — lo smoke test dal vivo con
`/crypto` è pianificato come step successivo del piano (dopo Task 10). La
copertura effettiva è duplice: (1) i test uniti sopra esercitano la
funzione pura con lo stesso "contratto a oggetto" (`preserved` con gli
stessi campi, stesso ordine di operazioni) che `render()` le passa
realmente; (2) lettura diretta del codice per confermare che `render()` in
`plugin-window.js` cattura `active.value` PRIMA di `sanitizeAndRender` e
passa `{ ...preserved, newRootElement: pluginRoot }` — cioè lo stesso
`pluginRoot` su cui `sanitizeAndRender` ha appena scritto il nuovo HTML,
non una reference stale.

## Fix: plugin runtime treats TEXTAREA as value-bearing (v0.45.17)

`plugin-runtime.mjs` — funzione pura `eventFromTarget(target)` — ora include
`TEXTAREA` accanto a `INPUT` e `SELECT` come elemento "value-bearing". Il
`.value` di un `<textarea>` viene quindi letto e incluso nell'evento inviato
all'host Tauri come `{ element_id, value }`, parallelo a INPUT/SELECT.

**Scoperta:** durante la pianificazione del plugin-crypto (task 1b di un piano
10-task), i due pannelli di testo principale (plaintext / ciphertext) sono
implementati come `<textarea>` — elementi multi-riga, ideali per testo
potenzialmente lungo — anziché `<input type="text">` singola riga. Senza questo
fix, la funzione live bidirezionale del plugin (aggiornare il pannello while
l'utente digita) non funzionerebbe: `eventFromTarget` avrebbe ritornato
`value: null` silenziosamente, rompendo il bind.

**Fix di piattaforma:** il problema non è specifico di crypto — è un gap
generico del runtime: qualunque plugin futuro con un campo di testo multi-riga
avrebbe avuto lo stesso problema.

### Modifica in `plugin-runtime.mjs`

La costante `isValueBearing` (riga ~56) decideva gli elementi "value-bearing"
come `el.tagName === "INPUT" || el.tagName === "SELECT"`. Estesa a:
```js
const isValueBearing = el.tagName === "INPUT" || el.tagName === "SELECT" || el.tagName === "TEXTAREA";
```
Con commenti aggiornati in funzione doc e inline per spiegare il supporto di
TEXTAREA e il suo ruolo nel plugin-crypto.

### Test: `plugin-runtime.test.mjs`

Nuovo test `"TEXTAREA element includes its value in the result"` che verifica:
- Element stub con `tagName: "TEXTAREA"` e valore multi-riga (`"riga uno\nriga due"`).
- Atteso: `eventFromTarget` ritorna `{ element_id, value: "riga uno\nriga due" }` (non `value: null`).
- Verificato RED (falliva prima del fix), GREEN dopo.

Intera suite: `node --test crates/ui/frontend/*.test.mjs` → 203/203 verdi
(202 pre-esistenti + 1 nuovo).

## Fix: Tab-completion perdeva la sottocartella digitata (v0.45.16)

Secondo bug trovato dalla review del fix v0.45.15 (virgolette), non
segnalato dall'utente — trovato tracciando a mano la logica di
sostituzione del range, non da un test che falliva. `list_path_completions`
torna sempre solo il basename (`entry.file_name()`, mai `dir_part`
riecheggiato indietro — vedi la sua doc comment in `main.rs`), ma sia il
ramo "fresh completion" sia il ramo di ciclo in `app.js` chiamavano
`replaceRange` sostituendo l'INTERO span del token — che parte da PRIMA di
`dirPart`, non dopo — con il solo candidato nudo. Risultato: `cd Sub\Fi` +
Tab completava a `cd File.txt`, non `cd Sub\File.txt` — `Sub\` spariva in
silenzio dalla riga. Peggio di un no-op: se in cwd esiste ANCHE un
`File.txt` diverso da quello dentro `Sub\`, il comando finito eseguirebbe
sul file sbagliato senza alcun avviso.

### Nuova funzione pura `buildCompletionText(dirPart, candidate, quoteChar)`

Prima di questo fix, la logica "avvolgi il candidato tra virgolette se
`quoteChar`" viveva INLINE, duplicata identica nei due rami del Tab handler
(fresh completion e ciclo) — esattamente il tipo di duplicazione che aveva
già causato un rischio di divergenza silenziosa nel fix v0.45.15. Estratta
in `path-utils.js` come singola funzione pura: ri-antepone `dirPart` al
candidato (`${dirPart}${candidate}`), poi avvolge il tutto tra `quoteChar`
se presente. Entrambi i rami di `app.js` ora chiamano
`pathUtils.buildCompletionText(dirPart, candidate, quoteChar)` — un solo
punto che può rompersi, non due che possono divergere. `tabCompletion`
guadagna il campo `dirPart` (accanto a `quoteChar`, già portato attraverso
i cicli dal fix precedente) con lo stesso meccanismo: distrutto una volta
sola nel ramo fresh, letto (mai ricalcolato) in ogni ciclo successivo.

### Pulizia minore

La stessa review ha trovato un test duplicato in `path-utils.test.mjs` —
`splitPathToken("Doc")` testato due volte con lo stesso identico atteso
(uno dei 4 test originali, uno aggiunto per contrasto nei test delle
virgolette). Rimosso il duplicato.

4 nuovi test per `buildCompletionText`: nessun `dirPart`/nessuna
virgoletta (comportamento minimo), `dirPart` ri-anteposto (il caso del
bug), virgolette che avvolgono `dirPart`+candidato insieme, ed entrambi
combinati. `node --test crates/ui/frontend/*.test.mjs`: 202/202 verdi
(199 di v0.45.15 − 1 duplicato rimosso + 4 nuovi).

## Tab-completion di file/percorso sulla riga di comando (v0.45.14)

Feature richiesta dall'utente: Tab sulla riga di comando principale completa
il nome di file/cartella sotto il caret, **solo in posizione argomento** (non
la prima parola della riga — quella è il nome del comando/cmdlet, e
completarla richiederebbe conoscere `$PATH`/l'elenco dei cmdlet, fuori
scope). Comportamento di riferimento: PowerShell (la shell realmente
pilotata via `mcp-server`'s persistent session), non bash — Tab ripetuto
**cicla in avanti** fra i candidati uno alla volta, Shift+Tab cicla
all'indietro. Niente "completa al prefisso comune poi lista al secondo Tab"
(quello è lo stile bash, deliberatamente non replicato).

### `path-utils.js` — 4 nuove funzioni pure

Stesso principio del resto del file (niente DOM, niente I/O, solo stringhe/
numeri in-out — testabili con `node:test` senza dipendenze):

- **`wordBounds(text, caret)`** — i confini della "parola" toccata dal
  caret: risale all'indietro dal caret fino al primo spazio (o inizio
  stringa). Non guarda oltre il caret sulla stessa parola — è la stessa
  convenzione di ogni shell reale ("completa fino al cursore", non l'intera
  parola se il cursore è nel mezzo).
- **`isArgumentPosition(text, wordStart)`** — vero se prima di `wordStart`,
  ignorando gli spazi, c'è dell'altro testo sulla riga. Falso solo per la
  prima parola (il comando). Questo è il guard che tiene la feature dentro
  lo scope deciso: Tab sulla prima parola non fa nulla.
- **`splitPathToken(token)`** — separa un token tipo `C:\Users\Doc` in
  `dirPart` (`"C:\Users\"`, separatore incluso, riusato verbatim) e `prefix`
  (`"Doc"`, la parte da completare/sostituire). Stessa logica di
  riconoscimento separatore di `parentDir` (`\` e `/` entrambi validi) ma,
  a differenza di `parentDir`, non normalizza né toglie il separatore —
  serve solo a spezzare il token per la chiamata al comando Tauri.
- **`replaceRange(text, rangeStart, rangeEnd, replacement)`** — sostituisce
  `text[rangeStart..rangeEnd)` con `replacement`, ritorna `{text, caret}`
  (stessa forma dello stato di `line-editor.js`) col caret subito dopo
  l'inserimento. Riusata sia per il primo inserimento (`rangeEnd` = caret
  originale) sia per ogni Tab successivo nello stesso ciclo (`rangeEnd` =
  fine del candidato appena inserito, per sostituirlo col prossimo).

### `list_path_completions` — nuovo comando Tauri (`src-tauri/src/main.rs`)

`fn list_path_completions(cwd: String, dir_part: String, prefix: String) -> Vec<String>`.
`base = Path::new(&cwd).join(&dir_part)` — `Path::join` gestisce già
correttamente sia `dir_part` relativo (es. `"sub\"`, `".."`) sia assoluto
(sostituisce interamente `cwd`), nessuna logica di risoluzione path scritta
a mano. `std::fs::read_dir` fallisce silenziosamente (directory inesistente,
permessi) → `Vec::new()`: **mai** un errore verso il frontend, un Tab senza
corrispondenze deve limitarsi a non fare nulla, non mostrare un problema
all'utente. Match del prefisso case-insensitive su Windows
(`to_lowercase()` su entrambi i lati — coerente col filesystem NTFS, che è
case-insensitive), case-sensitive altrove (coerente con ext4/APFS). Le
directory nel risultato terminano con `std::path::MAIN_SEPARATOR`: permette
al frontend di distinguerle dai file (per continuare subito a completare
dentro la sottocartella) senza una seconda chiamata IPC. Risultato ordinato
case-insensitive (`sort_by_key(|s| s.to_lowercase())`), così l'ordine di
ciclo Tab è prevedibile e non dipende dall'ordine di iterazione del
filesystem. 4 test `cargo test` (`list_path_completions_tests`, usa
`tempfile::tempdir()`, già dev-dependency del crate): match case-insensitive
su Windows, directory con separatore finale vs file senza, directory
inesistente → vuoto senza panico, `dir_part` relativo unito correttamente a
`cwd`.

### `app.js` — wiring

Due nuove variabili di modulo accanto a `homeDir`:

- **`currentCwd`** — il path grezzo della cwd corrente, aggiornato ad ogni
  `ServerMsg::Cwd` (`case "cwd":` ora fa `currentCwd = msg.path;` prima di
  aggiornare `cwdLineEl`). Prima di questa feature si teneva solo la
  versione *formattata* (con la sostituzione `~`) nel DOM — inutilizzabile
  per `list_path_completions`, che ha bisogno di un path filesystem reale.
- **`tabCompletion`** — stato del ciclo Tab in corso, `null` quando nessun
  ciclo è attivo. Forma: `{ rangeStart, rangeEnd, candidates, index, text }`.
  Azzerato incondizionatamente all'inizio del listener `keydown` per
  qualsiasi tasto diverso da `"Tab"` (prima di tutti gli altri branch —
  così nessun branch futuro può dimenticarsi di farlo): un ciclo di
  completamento sopravvive solo a pressioni consecutive di Tab, come in
  PowerShell.

Il branch `if (e.key === "Tab")` nel listener `keydown` (fra Ctrl+V e
PageUp/PageDown) distingue due casi:

1. **Ciclo in corso** (`tabCompletion` non nullo E `text`/`rangeEnd`
   combaciano esattamente con lo stato corrente dell'editor — guardia
   contro qualunque mutazione inattesa di `editorState` fra due Tab senza
   passare dal reset sopra): calcola l'indice successivo modulo la
   lunghezza dei candidati (`+ candidates.length) % candidates.length`
   gestisce sia Tab in avanti sia Shift+Tab all'indietro, compreso il
   wraparound da 0 a `length-1`), sostituisce il candidato precedente con
   quello nuovo via `replaceRange`.
2. **Completamento nuovo**: calcola `wordBounds`, esce silenziosamente se
   `isArgumentPosition` è falso (prima parola della riga), altrimenti separa
   il token con `splitPathToken` e invoca `list_path_completions` in modo
   asincrono. **Guardia anti-race**: la richiesta cattura `caretAtRequest`/
   `textAtRequest` al momento della chiamata; quando la Promise risolve,
   se `editorState.caret`/`.text` sono cambiati nel frattempo (l'utente ha
   continuato a digitare mentre la lookup filesystem era in volo) il
   risultato viene scartato invece di sovrascrivere quanto digitato dopo.

## Fix: Tab-completion su percorso tra virgolette (v0.45.15)

Bug trovato dal vivo dall'utente subito dopo il merge della v0.45.14: `cd "My`
seguito da Tab non completava nulla di sensato. Confermato tracciando le
funzioni pure sull'input esatto — `wordBounds` non ha nozione di virgolette,
tratta `"` come un carattere di token qualsiasi, quindi il token passato a
`splitPathToken` era `'"My'` (virgoletta inclusa). Il vecchio `splitPathToken`
non toglieva quel carattere, quindi `prefix` diventava `'"My'`: nessun file
reale inizia per `"`, zero corrispondenze, sempre. La gestione delle
virgolette non era mai stata implementata.

### `splitPathToken(token)` — firma cambiata, guadagna `quoteChar`

Ritorno ora `{ dirPart, prefix, quoteChar }` invece di `{ dirPart, prefix }`.
`quoteChar` è `"`, `'`, oppure `null`. Implementazione: se `token[0]` è una
virgoletta (doppia o singola), la si toglie PRIMA di cercare l'ultimo
separatore — così `dirPart`/`prefix` sono calcolati sulla stringa "pulita",
esattamente come se la virgoletta non ci fosse mai stata, e la virgoletta
tolta viene restituita a parte. Solo la virgoletta *iniziale* è riconosciuta
(scope della fix, coerente col bug segnalato); una virgoletta di chiusura già
digitata dall'utente non viene toccata.

Questo è un cambio di contratto: i 4 test esistenti (`nessun separatore`,
`sottocartella parziale`, `separatore finale`, `separatore POSIX`) hanno
dovuto aggiungere `quoteChar: null` all'atteso — `assert.deepEqual` sul
valore di ritorno confronta l'intero oggetto, quindi un campo in più basta a
far fallire il confronto anche se il caso non coinvolge virgolette. TDD
genuino qui ha significato: aggiornare prima quei 4 test (RED — falliscono
per il motivo giusto, il campo mancante), poi aggiungere i 4 nuovi test per
il caso virgolette (RED anche loro), solo dopo implementare.

### `app.js` — `quoteChar` propagato per tutta la sessione di ciclo Tab

Perché non bastava avvolgere solo il PRIMO candidato inserito: l'utente cicla
con Tab ripetuti tra più candidati (`tabCompletion.index`) e ogni ciclo
sostituisce interamente il testo precedentemente inserito via `replaceRange`
— se `quoteChar` non sopravvivesse nello stato `tabCompletion`, il secondo
Tab perderebbe l'informazione "questo candidato va tra virgolette" e
inserirebbe il nome nudo, rompendo l'invariante "un solo tipo di wrapping per
sessione di completamento". Soluzione: `quoteChar` viene distrutto da
`splitPathToken` una volta sola nel ramo "fresh completion", salvato dentro
`tabCompletion`, e riletto (mai ricalcolato) in ogni successivo `if (tabCompletion && ...)`
del ramo di ciclo. Entrambi i rami calcolano
`wrapped = quoteChar ? `${quoteChar}${candidate}${quoteChar}` : candidate`
e passano `wrapped` (non `candidate` nudo) sia a `replaceRange` sia al calcolo
di `rangeEnd` del prossimo `tabCompletion` (`rangeStart + wrapped.length`,
non più `candidate.length` — altrimenti il range da sostituire al Tab
successivo sarebbe troppo corto, tagliando fuori le virgolette appena
inserite).

`replaceRange` stesso non è stato toccato: il suo contratto (sostituisce
`text[rangeStart..rangeEnd)` con la stringa data, caret a fine inserimento)
resta identico — il chiamante ora gli passa semplicemente una stringa diversa
(già avvolta) invece del nome nudo.

## Banner di conferma per il gate locale per-tool (v0.45.5)

Frontend-only: nessuna nuova finestra Tauri, il banner vive in `#output-area` del
pannello cursore (`renderer.js::confirmBanner`, stessa famiglia visiva di
`systemMessage`/`.cmd-block`). Round-trip: `case "tool_confirm_request"` in
`app.js` → `renderer.confirmBanner` → click → `client.sendToolConfirmResponse` →
`ClientMsg::ToolConfirmResponse` sul WS. Nessuna logica pura da estrarre (solo
costruzione DOM + callback), coerente con le altre primitive di `renderer.js` (il
file è dichiaratamente il layer DOM, SRP nel suo doc-comment di testa). Design:
`Docs/superpowers/specs/2026-07-15-local-tool-confirm-gate-design.md`.

## AI Chat: apertura solo esplicita, niente più auto-apertura all'avvio (v0.45.4)

`ServerMsg::AiChatSelf` arriva **incondizionato** ad ogni nuova connessione WS
(`SetServerTx` in `orchestrator/src/aichat/service.rs`, dalla v0.36.2 — serve al roster di
Library "Share with" indipendentemente dal fatto che la finestra-chat sia aperta). Prima di
questa versione, `pushAiChat` reagiva a QUALSIASI evento `ai_chat_*` (incluso questo) aprendo
la finestra-chat da sola se non era già aperta o in apertura: comodo nella fase iniziale di
test (verificare il collegamento tra macchine diverse), ma indesiderato in uso normale — la
chat si apriva ad ogni avvio di `ui.exe` anche senza che l'utente la chiedesse.

**Fix, solo frontend** (il flusso backend `SetServerTx`/`AiChatOpen` resta invariato — serve
comunque per il resync degli eventi quando la finestra si apre davvero): l'apertura diventa
**solo esplicita**, tramite il comando `/aichat` già esistente. La logica di decisione è
estratta in un modulo puro `aichat-push.js` (`createAiChatPushGate`, stesso principio
buffer-and-replay di `search-buffer.js` per `/find`), con tre stati (`live`/`opening`/scarto)
e quattro operazioni:

- `push(event, payload)` → `{action: "emit"}` se la finestra è live, `{action: "buffer"}` se
  l'apertura è in corso (richiesta con `requestOpen()`, webview non ancora pronta),
  `{action: "drop"}` altrimenti — **questo è il fix**: prima, il ramo "altrimenti" chiamava
  `openAiChatWindow()` da solo.
- `requestOpen()` — chiamata SOLO dal comando `/aichat` (`app.js`, handler slash UI-local).
- `ready()` — chiamata dal listener `aichat:ready` (webview pronta): passa `live`, ritorna e
  svuota il buffer accumulato nel frattempo.
- `closed()` — chiamata dal listener `aichat:closed`: azzera tutto lo stato.

`app.js` non tiene più le tre variabili sciolte `aiChatLive`/`aiChatOpening`/`aiChatBuffer`:
un'unica istanza `aiChatGate` (`createAiChatPushGate()`) le sostituisce. 6 test `node:test`
nuovi in `aichat-push.test.mjs` (drop senza richiesta, buffer durante apertura, ordine di
replay, transizione a live, reset su `closed()`, buffer scartato se la finestra si chiude
prima di diventare pronta).

## Library: tab Plugins (sola lettura, v0.44.3)

Terzo tab della finestra Library (`crates/ui/src-tauri/src/plugins_view.rs` + `library.html`/
`library.js`), prima slice: elenca i plugin trovati in `plugins_dir()` (stessa risoluzione di
`orchestrator/src/main.rs`: `LARE_PLUGINS_DIR` o `%LOCALAPPDATA%\dev.lare.terminal\plugins`,
`.lare-data/plugins` di fallback — duplicata deliberatamente, `ui` non dipende dal crate
`orchestrator`) e mostra il `plugin.json` grezzo di ognuno al click sulla riga. Scelta di design:
`scan_plugins` è deliberatamente **più tollerante** di `discovery::discover` (che valida l'`id` e
cerca il binario prima di eseguirlo) — qui non si esegue nulla, quindi un manifest malformato o
senza `id`/`name` produce comunque un'entry (fallback sul nome della cartella) invece di sparire:
per un pannello diagnostico, mostrare "c'è un plugin.json rotto qui" batte il silenzio. Editing
della configurazione, una finestra dedicata al posto del `<pre>` inline, e cifratura di eventuali
segreti nel manifest restano fuori scope, rimandati a una slice futura se servirà. Il `<pre
class="plugin-manifest">` di dettaglio è un elemento fratello della riga (non annidato, non
`.archive-item`) così la navigazione da tastiera generica (ArrowUp/Down su `.archive-item` in
`#list-area`) non lo tratta come riga selezionabile.

## Ammissione (0.40.0)

### `admission.mjs` — logica pura (no DOM/Tauri)

Due funzioni pure, testate con `node:test` senza dipendenze (stesso pattern di
`search-status.js`/`connection-diagnosis.js`):

- **`joinPromptText(present)`** — testo del gate 1 mostrato al nuovo arrivato. `present` è
  l'array delle etichette già nella stanza. Applica la congiunzione italiana standard per
  liste: virgole fra tutti gli elementi tranne l'ultimo, agganciato con `" e "`.
  - `[]` → `"Vuoi entrare in chat?"`
  - `["A"]` → `"Vuoi entrare in chat con A?"`
  - `["A","B"]` → `"Vuoi entrare in chat con A e B?"`
  - `["A","B","C"]` → `"Vuoi entrare in chat con A, B e C?"`
- **`rerequestState(nowMs, retryAtMs)`** — stato del pulsante "Chiedi di entrare" dopo un
  rifiuto, dato l'orologio corrente e la scadenza del cooldown (`retry_after_secs` da
  `ServerMsg::AiChatRejected`, convertito in epoch ms lato chiamante). `retryAtMs` assente o
  già superato → `{ enabled: true, secondsLeft: 0 }`; altrimenti `{ enabled: false,
  secondsLeft }` coi secondi arrotondati **per eccesso** (`Math.ceil`) — il countdown
  mostrato all'utente non tocca mai "0" prima che il cooldown sia davvero scaduto. Il
  cooldown stesso (**30s**) è deciso e applicato **lato UI**: il server manda solo
  `retry_after_secs`, il conteggio/enable è tutto qui.

### Wiring WS (`ws-client.js` / `app.js`)

- **`ws-client.js`** — 3 nuovi metodi di invio, uno per ciascun `ClientMsg` additivo di
  `protocol` 0.10.0: `sendAiChatJoinDecision(accept)`, `sendAiChatAdmissionVote(candidate,
  accept)`, `sendAiChatRequestAdmission()`.
- **`app.js`** — 5 nuovi case nello switch di `ServerMsg` (`"ai_chat_join_prompt"`,
  `"ai_chat_admission_request"`, `"ai_chat_pending"`, `"ai_chat_admitted"`,
  `"ai_chat_rejected"`), ciascuno inoltrato alla finestra-chat via `pushAiChat` (stesso
  bus di eventi Tauri già usato da roster/history/self-label); e i listener degli eventi
  Tauri in ingresso dalla finestra (click sui pulsanti dei gate, click su "Chiedi di
  entrare") verso i rispettivi `sendAiChat*` di `ws-client.js`.

### Finestra chat (`aichat-window.js`)

Cinque nuovi gestori, uno per evento:

- **`aichat:join_prompt`** (gate 1) — mostra `joinPromptText(present)` con due pulsanti,
  [Entra] → `sendAiChatJoinDecision(true)`, [Annulla] → `sendAiChatJoinDecision(false)`.
- **`aichat:admission_request`** (gate 2) — mostrato a un presente già ammesso: "`candidate`
  chiede di entrare" con [Ammetti] → `sendAiChatAdmissionVote(candidate, true)` / [Rifiuta]
  → `sendAiChatAdmissionVote(candidate, false)`.
- **`aichat:pending`** — **blocca l'input** della finestra (il fix del bug principale: senza
  questo, l'umano poteva scrivere il proprio saluto prima che il voto si risolvesse, e quel
  messaggio andava perso perché mandato mentre il service era ancora in
  `SelfAdmission::Deciding`/prima della registrazione lato server).
- **`aichat:admitted`** — sblocca l'input, annuncio locale "Sei entrato in chat." (con una
  guardia per non duplicare l'annuncio se il diff del roster lo mostrerebbe comunque).
- **`aichat:rejected`** — annuncio "Ingresso rifiutato.", il pulsante "Chiedi di entrare"
  passa a disabilitato per il cooldown; un timer ricalcola `rerequestState` a intervalli e
  si autodistrugge quando il countdown arriva a 0 (pulsante torna abilitato, click →
  `sendAiChatRequestAdmission()`, che rimanda la richiesta e la UI torna in `pending`).

### Test

```
node --test crates/ui/frontend/*.test.mjs → 146 passed (nessuna regressione; nuovi: admission.test.mjs)
```

Nessun test Rust in questa slice (nessun comando Tauri nuovo — solo wiring WS/eventi
frontend).

### Resta per l'accettazione live

e2e GUI multi-macchina (checklist in `Docs/TESTING-e2e.md` §H): gate 1/2 visibili e
funzionanti, input bloccato durante `pending` (saluto non perso), veto → rifiutato +
cooldown + re-request, timeout → ammesso, un pending non riesce a mandare messaggi
(relay-guard lato orchestrator, verificabile solo dal vivo).

---

# Implementation — crates/ui v0.39.0 (/config: flag "auto-partecipazione")

> v0.39.0: **Flag "auto-partecipazione" nel tab AI Chat (AI Chat Slice 2).** 5° campo
> `ai_autoparticipate: bool` su `AiChatSettings` (`src-tauri/src/aichat_settings.rs`),
> mirror di `orchestrator::aichat::config::AiChatConfig::ai_autoparticipate` (design
> `Docs/superpowers/specs/2026-07-02-aichat-ai-participants-design.md` §10).
> `get_aichat_settings` lo legge con default **`false`** — a DIFFERENZA di
> `ai_participates` (default `true`, v0.38.0 sotto): l'auto-partecipazione è opt-in
> esplicito, coerente col fatto che rimuove la loop-safety-per-costruzione lato
> orchestrator (bounded solo dal cap sui turni AI consecutivi). Stesso pattern
> infallibile degli altri campi (file assente/corrotto O `aichat.json` scritto prima
> che il campo esistesse → `false`, via `unwrap_or(false)`). Nessuna validazione
> aggiuntiva in `validate()` (un `bool` non ha stati invalidi). 3 nuovi test unitari
> (TDD RED→GREEN):
> `get_settings_reads_ai_autoparticipate_with_false_default`,
> `missing_ai_autoparticipate_field_reads_as_false`,
> `merge_round_trip_preserves_ai_autoparticipate`; gli 8 struct-literal
> `AiChatSettings { .. }` esistenti nel modulo (sia produzione sia test) aggiornati al
> nuovo 5° campo.
>
> **Semantica del flag** (decisa lato orchestrator, la UI è solo editor): a differenza
> di `ai_participates` (risponde a un'invocazione ESPLICITA `@all`/`@<mio-label>-ai`),
> `ai_autoparticipate` fa intervenire l'AI SPONTANEAMENTE sui messaggi normali della
> stanza, giudicando da sola la rilevanza — indipendentemente da qualunque `@`.
>
> Frontend: `config-dialog.js` `_buildAiChatTab(panel, aichat)` guadagna una 5°
> checkbox, "Auto-partecipazione (l'AI interviene da sola)" (`_buildCheckbox`, id
> `config-aichat-autoparticipate`, default **UNCHECKED** — coerente col default `false`
> lato Rust, a differenza della checkbox "la mia AI partecipa" sopra che è default
> checked). Ritornata come `autoparticipateInput` nell'oggetto refs; letta al
> salvataggio come `newAiChat.ai_autoparticipate` nello stesso blocco che costruisce gli
> altri campi. Nessun nuovo test JS: `config-dialog.js` non ha un modulo `*.test.mjs`
> dedicato (stesso motivo già documentato in v0.38.0 sotto) — la copertura del
> round-trip resta lato Rust.

> v0.38.0: **Flag "la mia AI partecipa" nel tab AI Chat (AI Chat Slice 1b).** 4°
> campo `ai_participates: bool` su `AiChatSettings`
> (`src-tauri/src/aichat_settings.rs`), mirror di
> `orchestrator::aichat::config::AiChatConfig::ai_participates` (design
> `Docs/superpowers/specs/2026-07-02-aichat-ai-participants-design.md` §9.4).
> `get_aichat_settings` lo legge con default `true` (stesso pattern infallibile
> degli altri 3 campi — file assente/corrotto O `aichat.json` scritto prima che il
> campo esistesse leggono comunque `true`, via `unwrap_or(true)`). Nessuna
> validazione aggiuntiva in `validate()` (un `bool` non ha stati invalidi). 2 nuovi
> test unitari (TDD RED→GREEN): `get_settings_reads_ai_participates_with_true_default`,
> `merge_round_trip_preserves_ai_participates`; i 6 test esistenti aggiornati al
> nuovo 4° campo del literal `AiChatSettings { .. }`.
>
> **Semantica del flag** (decisa lato orchestrator, la UI è solo editor): gata SOLO
> le invocazioni AI **remote** (`@all`/`@<mio-label>-ai` scritte da un umano di
> un'altra macchina) — l'invocazione della PROPRIA macchina (`@ai` nella propria
> finestra AI Chat) agisce sempre, indipendentemente da questo flag.
>
> Frontend: `config-dialog.js` `_buildAiChatTab(panel, aichat)` guadagna una 4°
> checkbox, "La mia AI partecipa (risponde alle richieste della stanza)"
> (`_buildCheckbox`, id `config-aichat-participates`, default checked — coerente col
> default `true` lato Rust). Ritornata come `participatesInput` nell'oggetto refs;
> letta al salvataggio come `newAiChat.ai_participates` nello stesso blocco che
> costruisce gli altri 3 campi. Nessun nuovo test JS: `config-dialog.js` non ha un
> modulo `*.test.mjs` dedicato (DOM/Tauri-dipendente, come gli altri campi del tab
> AI Chat) — la copertura del round-trip resta lato Rust.

> v0.37.0: **Tab "AI Chat" in /config.** Nuovo modulo
> `src-tauri/src/aichat_settings.rs`, mirror di `search_settings.rs`: legge/scrive i
> 3 campi editabili di `aichat.json` (`enabled`, `label_base`, `chat_port`) nello
> stesso path che usa l'orchestrator (`%LOCALAPPDATA%\dev.lare.terminal\aichat.json`,
> fallback `.lare-data`), con `merge_aichat_settings` che preserva ogni altro campo
> presente nel file. Comandi Tauri `get_aichat_settings` (infallibile: default sicuri
> `enabled=false`/`label_base="lare"`/`chat_port=40100` se file assente o corrotto) e
> `set_aichat_settings` (`Result<(), String>`: valida — tramite `validate()`, funzione
> pura testata separatamente — poi merge-scrive). Validazione: `label_base` non vuota
> dopo `trim()`, lunghezza 1..=32, solo `[A-Za-z0-9_-]` (diventa
> `"<label_base>-human"` sul wire in `orchestrator/src/aichat/service.rs` — niente
> spazi/`@`/`.`); `chat_port` in `1024..=65535` (esclude le porte privilegiate). 6 test
> unitari (TDD RED→GREEN: prima uno stub deliberatamente sbagliato che fa fallire 5/6
> test per il motivo giusto, poi l'implementazione vera).
>
> **Differenza chiave rispetto al tab Search**: questi valori NON sono "live" — il
> servizio AI Chat dell'orchestrator legge `aichat.json` solo al proprio avvio.
> `config-dialog.js` lo segnala con una nota `.config-note` nel tab
> ("Le modifiche all'AI Chat richiedono il riavvio dell'orchestrator.").
>
> Frontend: `_buildPanel(cfg, search, aichat)` ora accetta un terzo argomento;
> `_buildAiChatTab(panel, aichat)` costruisce checkbox `Attivo` + campo testo
> `Etichetta` + campo numero `Porta` (min 1024, max 65535) riusando gli helper
> esistenti (`_buildCheckbox`, `_buildField`). Il salvataggio pre-valida
> `label_base`/`chat_port` lato JS (stesso pattern dei campi Search) poi chiama
> `set_aichat_settings({ settings: ... })` nello stesso blocco `try` di
> `set_config`/`set_search_settings` — un errore di validazione backend appare in
> `errorEl` senza chiudere il dialog. Nessun cambiamento a `protocol`/`orchestrator`:
> comunicazione solo tramite il file `aichat.json` condiviso su disco.

> v0.36.4: **Avviso live "peer sparito" (keepalive).** `aichat-view.mjs`:
> `peerLostText(label)` (pura, 2 test `node:test`) — `"<label> è sparito"` o fallback
> `"un peer è sparito"` se il label manca. `app.js`: case `"ai_chat_peer_lost"` →
> `pushAiChat("aichat:peer_lost", {label})`. `aichat-window.js`: listener
> `aichat:peer_lost` appende una riga di sistema (`.line.system`) direttamente al DOM
> del trascritto — **transitoria**: a differenza delle righe normali non entra mai
> nello stato ri-renderizzato da `aichat:history` (che sostituisce
> `transcript.innerHTML` in blocco), quindi un replay successivo la fa sparire
> naturalmente. Coerente col design "live-only" per gli eventi di rete effimeri.
> Backend: `orchestrator` 0.25.8 (ping/timeout roster + fan-out `PeerLost`) /
> `protocol` 0.9.4 (`AiChatPeerLost`). Spec:
> `Docs/superpowers/specs/2026-07-02-aichat-keepalive-design.md`.

> v0.41.1: **Testo avviso più naturale.** Feedback utente durante i test dal vivo
> dell'ammissione a stanza: "sparito" suona allarmante/gergale in una chat tra umani.
> `peerLostText` cambia da `"<label> è sparito"` a `"<label> risulta scollegato"`
> (fallback: `"un peer risulta scollegato"`). Nessun cambio di meccanismo — stesso
> evento live-only del v0.36.4 sopra, solo il testo.

> v0.36.3: **Uscita pulita interattiva.** `is_quit_command(line)` (pura, 2 test) in
> `main.rs` riconosce "q"/"quit". Thread stdin in `.setup()`, solo
> `#[cfg(debug_assertions)]` (in release niente console, `windows_subsystem =
> "windows"`): su "q" chiama `app_handle.cleanup_before_exit()` + `std::process::exit(0)`
> — API Tauri v2 dedicata a lasciare a WebView2 il tempo di chiudersi prima
> dell'uscita del processo. Verificato dal vivo: elimina il log innocuo ma rumoroso
> `"Failed to unregister class Chrome_WidgetWin_0"` che appariva interrompendo con
> Ctrl+C. Backend: `orchestrator` 0.25.6 (stesso affordance, via `CancellationToken`
> su `ws::serve`).
>
> **RIMOSSO in v2.2.2**: obsoleto dal fix v2.2.1 (chiudere la finestra terminale già
> termina il processo pulito) — vedi la sezione in cima a questo file.

> v0.36.2: **Titolo finestra-chat con la propria etichetta.** `aichat-view.mjs`:
> `chatWindowTitle(selfLabel)` (pura, 2 test `node:test`) — `"AICHAT - <label>"` o solo
> `"AICHAT"` come fallback. `aichat-window.js`: listener `aichat:self` → aggiorna
> `#titlebar-text`. `app.js`: case `"ai_chat_self"` → `pushAiChat("aichat:self",
> {label})`. `aichat-window.html`: testo statico `"AI Chat — /aichat"` (ridondante) →
> `"AICHAT"`, id `titlebar-text` aggiunto. Backend: `orchestrator` 0.25.5 / `protocol`
> 0.9.3 (`SetServerTx` emette `AiChatSelf` incondizionatamente).

> v0.36.1: **Replay storico messaggi nella finestra-chat.** `aichat-view.mjs`:
> `historyEntries(entries)` (pura, 2 test `node:test`) — normalizza il payload, passthrough grezzo
> (non il formato `messageLine`, applicato già da `addLine`). `aichat-window.js`: listener
> `aichat:history` → svuota `#transcript` e ri-renderizza in blocco (niente merge/dedup).
> `app.js`: case `"ai_chat_history"` → `pushAiChat("aichat:history", {entries})`. Backend:
> `orchestrator` 0.25.2 / `protocol` 0.9.2 (storico persistente in-memory + log elezione server).

> v0.36.0: **calcolatrice scientifica: tasti compatti 5-col + indicatore + apice**

> v0.12.0: **Finestra di ricerca file live (Task 11).** `window-search.html`/
> `window-search.js`: finestra trasparente con risultati raggruppati per sorgente, live,
> click→`/open`, chiusura→`CancelSearch`. Comando Rust `open_search_window` (label `search-*`,
> riusa `WindowContentStore`; capability `search-window.json`). `app.js`: `search_open/hit/done`
> via eventi Tauri globali (filtrati per `sid`). `ws-client.js`: `cancelSearch(id)`.

> v0.13.0: **Stop button nella finestra di ricerca (Task 2).** `search-status.js` esporta
> `statusLabel({ done, stopped, count, truncated })` (funzione pura, testata con 7 casi
> `node:test` in `search-status.test.mjs`). `window-search.html`: pulsante `#stop-btn` ⏹
> nella title-bar (a sinistra della ×); CSS `.hidden { display:none !important }`.
> `window-search.js`: import ESM di `statusLabel`; flag `stopped`/`done`; handler
> `stopSearch()` (idempotente — emette `search:cancel {sid}` senza chiudere la finestra,
> nasconde ⏹, aggiorna stato); listener `search:done` aggiornato per usare `statusLabel`
> e rispettare il flag `stopped`. Solo frontend: nessuna modifica a `protocol`/`orchestrator`.

## Structure

```
crates/ui/
├── frontend/
│   ├── index.html              # Overlay markup + styles (CSS vars, dialog styles, cursor-static)
│   ├── app.js                  # Wiring: keyboard (lineEditor), WS client, renderer, CSS vars, clipboard
│   │                           #   v0.30.0: gestione open/update/close_plugin_window + plugin event routing
│   │                           #   v0.34.0: /config → open_config_window (webview); config:saved listener
│   ├── line-editor.js          # Pure line-editor state machine {text,caret} — no DOM/async
│   ├── line-editor.test.mjs    # TDD test suite (node:test, 24 cases)
│   ├── package.json            # {"type":"module"} — enables node:test ESM imports
│   ├── ws-client.js            # WebSocket lifecycle + wire protocol (no DOM)
│   │                           #   v0.30.0: sendPluginUiEvent / sendPluginWindowClosed
│   ├── renderer.js             # DOM output rendering (no network)
│   ├── config-dialog.js        # /config dialog — usato da config-window.js (non da app.js)
│   │                           #   v0.37.0: 3° tab "AI Chat" — _buildAiChatTab; _buildPanel(cfg,search,aichat)
│   ├── config.html             # Finestra /config dedicata (v0.34.0): chromeless, 480×600, resizable, always_on_top
│   ├── config.css              # Stili della finestra config (estratti da index.html)
│   │                           #   v0.37.0: .config-note (nota muted in fondo a un tab)
│   ├── config-window.js        # Bootstrap finestra /config: get_config → ConfigDialog; emette config:saved
│   ├── window.html             # Markdown window: dark-themed, save button + close button
│   ├── window.js               # Markdown window init: IPC → marked → DOMPurify → innerHTML; save btn
│   ├── library.html            # Archive browser: toolbar (breadcrumb, + Cartella, 🔄) + list area
│   │                           #   + <dialog id="move-dialog"> modale Sposta (Slice 4b)
│   ├── library.js              # Archive browser Explorer: tree nav, folder rows, rename, delete,
│   │                           #   sposta (↗ su cartelle+documenti, modale move-dialog)
│   ├── library-nav.mjs         # Modulo puro: isValidFolderName, isFolderEmpty, findNode,
│   │                           #   breadcrumbSegments, parentPath, flattenFolders, moveTargets
│   │                           #   — testabili senza DOM/Tauri
│   ├── library-nav.test.mjs    # TDD 37 test (node:test) per library-nav.mjs
│   ├── plugin-runtime.mjs      # Modulo puro: eventFromTarget + eventFromDblTarget + sanitizeAndRender + eventFromKey
│   │                           #   v0.30.0: eventFromTarget, sanitizeAndRender (ADD_ATTR: data-evt)
│   │                           #   v0.33.0: eventFromKey(key, buttons) — mappa KeyboardEvent.key
│   │                           #     sul pulsante [data-key] corrispondente; DOMPurify ADD_ATTR
│   │                           #     esteso a "data-key"; no DOM/Tauri, testabile con node:test
│   │                           #   v0.44.4: eventFromDblTarget(target) — gemello di eventFromTarget
│   │                           #     ma legge [data-dblevt] (doppio-click, opt-in per elemento;
│   │                           #     value sempre null). Attributo INDIPENDENTE da data-evt.
│   │                           #   v0.45.18: restoreActiveField(preserved) — ripristina valore LIVE
│   │                           #     (non quello in ritardo del round-trip) + focus + selectionRange
│   │                           #     sul campo che aveva il focus dopo un re-render; estratta da
│   │                           #     plugin-window.js perché puro/testabile (no CSS.escape del browser)
│   ├── plugin-runtime.test.mjs # TDD (v0.45.18: +8 test restoreActiveField, oltre ai precedenti
│   │                           #   6 v0.30.0 + 6 v0.33.0 eventFromKey + 5 v0.44.4 eventFromDblTarget)
│   ├── plugin-catalog.css      # Stili condivisi finestre plugin (v0.32.0): lare-window/label/button/
│   │                           #   display/key-grid/key/frac* (Slice 2) + lare-expr (Slice 2a)
│   │                           #   v0.35.0: .lare-calc .lare-display { min-height:3.2em; align-items:flex-end }
│   │                           #     (display calcolatrice: ~3 righe, contenuto ancorato in basso a destra)
│   ├── plugin-window.html      # Finestra-plugin generica (v0.30.0): chromeless, always_on_top
│   ├── plugin-window.js        # Bootstrap: take_window_content → render; click delegato → "plugin:ui-event";
│   │                           #   ascolta "plugin:update" / "plugin:close"; emette "plugin:window-closed"
│   │                           #   v0.45.18: render() cattura anche active.value (oltre a
│   │                           #     tag/data-evt/selection) PRIMA di sanitizeAndRender e chiama
│   │                           #     restoreActiveField({...preserved, newRootElement: pluginRoot})
│   │                           #     invece del ripristino inline precedente (solo focus+selection)
│   │                           #   v0.44.4: secondo listener delegato "dblclick" → eventFromDblTarget →
│   │                           #     emitUiEvent (opt-in [data-dblevt]); plugin senza data-dblevt inalterati
│   │                           #   v0.32.0: autoFitHeight() — flex-collapse trick per misurare scrollHeight
│   │                           #   reale (non quello allungato da flex:1); chiama resize_self
│   │                           #   all'apertura e a ogni plugin:update (il contenuto può crescere)
│   │                           #   v0.33.0: listener window.keydown — legge [data-key] dal DOM ad ogni
│   │                           #     evento, chiama eventFromKey(ev.key, buttons), su match:
│   │                           #     ev.preventDefault() + emitUiEvent (stessa via del click).
│   │                           #     Guard (commit 8b4d23e): Ctrl/Cmd/Alt escono subito (zoom/reload
│   │                           #     restano al browser); Shift NON escluso (serve per * ( ) = / layout IT)
│   │                           #   v0.35.0: growCalcDisplay() — high-water altezza display in variabile
│   │                           #     di modulo (calcDisplayHighWaterPx, per-webview) ri-applicata come
│   │                           #     min-height ad ogni render (l'HTML è re-iniettato → l'elemento è nuovo
│   │                           #     e perderebbe lo style inline). Monotòno → display non rimpicciolisce;
│   │                           #     chiamata PRIMA di autoFitHeight() → niente "saltellio". Reset = riapertura.
│   │                           #     Guard .lare-calc (altri plugin intatti)
│   └── vendor/
│       ├── marked.min.js       # marked v15.0.12 (pinned, local — no CDN at runtime)
│       └── purify.min.js       # DOMPurify v3.2.5 (pinned, local — no CDN at runtime)
├── src-tauri/
│   ├── src/
│   │   ├── lib.rs             # Library entry point — espone `config` + `archive` + `library_watch`
│   │   ├── config.rs          # Config struct, Position enum, load_from/save_to, validate_action_key
│   │   ├── archive.rs         # Archive logic: slug, save, list_tree, create/rename/delete folder, open, delete
│   │   ├── library_watch.rs   # fs-watch seam: watch_dir(dir, debounce, on_change) → WatchGuard (RAII)
│   │   │                      #   Usa notify-debouncer-mini 0.7.x. Testabile senza Tauri (seam DIP).
│   │   │                      #   2 test: watch_dir_detects_a_change, watch_dir_debounces_burst (LENIENT)
│   │   ├── main.rs            # Tauri entry point: clipboard + global-shortcut plugins, commands
│   │   │                      #   LibraryWatchGuard managed state; watcher avviato in .setup
│   │   │                      #   v0.30.0: open_plugin_window (label plugin-<window_id>) + F2 toggle plugin-*
│   │   │                      #   v0.34.0: open_config_window — singleton webview (label "config")
│   │   │                      #   v0.44.4: open_plugin_window guadagna width/height: Option<f64>
│   │   │                      #     (inner_size = unwrap_or(480/360); assenti da JS ⇒ None ⇒ default)
│   │   ├── window.rs          # show/center/focus helpers, position-driven placement
│   │   ├── search_settings.rs # get_/set_search_settings — result_cap/max_depth in search-paths.json
│   │   │                      #   (merge preserva altri campi; parametri LIVE lato orchestrator)
│   │   └── aichat_settings.rs # v0.37.0: get_/set_aichat_settings — enabled/label_base/chat_port
│   │                          #   in aichat.json (mirror di search_settings.rs). NON live: effetto
│   │                          #   solo al prossimo riavvio dell'orchestrator (nota nel tab /config).
│   │                          #   validate() pura testata separatamente (charset label, range porta)
│   ├── capabilities/
│   │   ├── default.json          # Permissions: core:default, global-shortcut, clipboard-manager,
│   │   │                         #   core:window:allow-set-size + allow-start-dragging (window: main)
│   │   ├── markdown-window.json  # Permissions for Markdown windows: core:default, windows: ["md-*"]
│   │   ├── library-window.json   # Permissions for archive browser: core:default + start-dragging, windows: ["library"]
│   │   ├── plugin-window.json    # Permissions for plugin windows (v0.30.0): core:default + start-dragging,
│   │   │                         #   windows: ["plugin-*"]
│   │   └── config-window.json    # Permissions for config window (v0.34.0): core:default + start-dragging,
│   │                             #   windows: ["config"] (core:default include già event emit/listen)
│   ├── icons/
│   │   └── icon.ico
│   ├── build.rs
│   ├── Cargo.toml        # v0.36.0; [lib] target; tauri-plugin-clipboard-manager; notify-debouncer-mini = "0"
│   └── tauri.conf.json   # main: 1064×140, resizable:true, decorations:false — pannello a riposo
├── CHANGELOG.md
└── IMPLEMENTATION.md     (this file)
```

## Plugin Slice 1 — Finestra-plugin generica (v0.30.0)

### Architettura

```
Orchestrator → ServerMsg::OpenPluginWindow{window_id, title, html, width?, height?}
  → app.js: invokeCmd("open_plugin_window", {windowId, title, html, width, height})
      → Rust main.rs: label = "plugin-{window_id}"
                      WindowContentStore.insert(label, (title, html, window_id_str))
                      WebviewWindowBuilder::new(label, "plugin-window.html")
                        .inner_size(width.unwrap_or(480), height.unwrap_or(360)).build()
          ← plugin-window.html aperta, chromeless, always_on_top

plugin-window.js (nella nuova finestra):
  invoke("take_window_content")  → {title, content: html, kind: window_id_str}
  sanitizeAndRender(html)        → innerHTML (DOMPurify)
  click su [data-evt="inc"]      → emits "plugin:ui-event" {window_id, element_id: "inc", value: null}

app.js (main window) ascolta "plugin:ui-event":
  client.sendPluginUiEvent(window_id, element_id, value)  → WS → orchestratore

Orchestrator → ServerMsg::UpdatePluginWindow{window_id, html}
  → app.js: emitToPlugin("plugin:update", {window_id, html})
      → plugin-window.js ascolta "plugin:update" → filtra per window_id → sanitizeAndRender(html)

Chiusura finestra → plugin-window.js emette "plugin:window-closed" {window_id}
  → app.js: client.sendPluginWindowClosed(window_id) → WS → orchestratore
```

### Punti chiave

- **Label stabile** (`plugin-<window_id>`): non usa `<ts>-<ctr>` come Markdown/search
  perché il `window_id` viene dall'orchestratore ed è già univoco per ciclo di vita.
  Idempotency check: se la finestra è già aperta, `build()` restituisce errore (bug
  visibile invece di duplicato silenzioso).
- **Riusa `WindowContentStore` e `take_window_content`**: nessun nuovo managed state.
  Il campo `kind` (terzo elemento della tripla) trasporta `window_id_str` — `plugin-window.js`
  lo parsa con `Number()` per avere il `window_id` numerico.
- **Nessun Tauri command per `plugin_ui_event` / `plugin_window_closed`**: il back-channel
  usa eventi Tauri globali (`"plugin:ui-event"` / `"plugin:window-closed"`) ascoltati da `app.js`
  — stesso schema di `"search:open-path"` nella finestra di ricerca. Evita di aggiungere comandi
  Rust per operazioni puramente di routing JS→WS.
- **`plugin-runtime.mjs`** puro (no DOM/Tauri): `eventFromTarget(el)` e `sanitizeAndRender(html)`.
  Testabile con `node:test` senza runner Tauri. 6 test verdi.
- **Capability `plugin-window.json`**: scope `["plugin-*"]`, permissions
  `core:default + core:window:allow-start-dragging`. Senza questa capability le chiamate IPC
  dalla finestra plugin sarebbero no-op silenziosi.

### Comandi Tauri aggiunti

| Command | JS signature | Description |
|---------|-------------|-------------|
| `open_plugin_window` | `invoke("open_plugin_window", { window_id, title, html })` | Crea finestra plugin con label `plugin-<window_id>`; `async` per evitare deadlock Windows in Tauri v2 |

### Test

| Test | Tipo | Risultato |
|------|------|-----------|
| 6 test in `plugin-runtime.test.mjs` | `node:test` puro | 6/6 verdi |
| `counter_activate_and_ui_event_round_trip` in `orchestrator/tests/plugin_window_e2e.rs` | `#[ignore]`, richiede `cargo build -p plugin-counter` | verde (0.04 s) |

GUI (finestra plugin che appare e risponde ai click) = accettazione utente.

## Overlay: il pannello riempie la finestra + auto-grow (v0.29.0)

La finestra `main` non ha più bande trasparenti che catturavano i click: il pannello **riempie
l'intera finestra**, la finestra è ridimensionabile dall'utente e cresce da sola all'output.

> **Storia (accettazione GUI):** la v0.29.0 nasceva come "auto-size" (la finestra inseguiva il
> pannello via `ResizeObserver → setSize`, modulo puro `window-sizing.js`). Su Windows una
> finestra `decorations:false`+`transparent` non si ridimensiona affidabilmente via `setSize`
> (il comando si risolve ma l'OS non applica) → si è **invertito il flusso**: la finestra è la
> sorgente di verità, il pannello la segue. `window-sizing.js` + test rimossi.

### Il pannello riempie la finestra (`index.html`)
- `body { display:flex }` + `.overlay-panel { flex:1; min-width/height:0 }` → il pannello
  riempie la finestra. `#output-area { flex:1; min-height:0 }` scrolla. Niente larghezze fisse.
- Bordo del `body` = **bordo doppio blu** (stesso colore/raggio del pannello, gap 1px) = handle
  del resize (`resizable:true`). Ridimensionare la finestra ridimensiona il pannello.
- `data-tauri-drag-region` sul `.overlay-panel` (NON i figli interattivi: input, button,
  `.hint-cmd`) → la finestra si trascina dallo sfondo del pannello.

### Auto-grow (`app.js/setupAutoGrow`)
- `MutationObserver` sul **contenuto** dell'`#output-area`: a ogni cambio (debounce 80ms), se
  `scrollHeight > clientHeight` cresce la finestra (`setSize`, **solo crescita**) fino a ≤70%
  schermo e senza sforare il bordo basso. `MutationObserver` (non `ResizeObserver`) così
  `setSize` non ri-triggera l'observer → **niente loop**.

### Reset (`Ctrl+T`)
- `app.js`: `Ctrl+T` pulisce input+output (come `Ctrl+L`) + `resetWindow()` = `setSize` alla
  default compatta + `show_overlay` (ri-centra in Rust). `Ctrl+R` no: la WebView2 lo riserva al
  reload pagina.

### Ownership centratura (INVARIANTE)
- Centratura in Rust (`window.rs`, invariato): `show_and_focus` su F2-show. Il frontend NON
  chiama mai `setPosition`/re-center — solo `setSize` (crescita/reset) e `show_overlay` (che
  ri-centra in Rust).

### Configurazione
- `tauri.conf.json main`: `1064×140` (default compatto), `resizable:true`.
- `capabilities/default.json`: `core:window:allow-set-size` + `core:window:allow-start-dragging`
  (entrambi assenti in `core:default` → senza, no-op silenzioso).

## Vista Explorer — Library a cartelle (v0.24.0)

### Modello UX

Il tab Markdown mostra **una cartella per volta** (come un file manager):
- **Breadcrumb**: `🏠 / Progetti / Web` — ogni segmento è un `<button>` cliccabile.
- Contenuto corrente: **prima le sottocartelle 📁, poi i documenti `.md`**.
- Doppio-click / Invio su cartella = entra; su documento = apri.
- Toolbar: `[+ Cartella]` (crea nella posizione corrente) e `[🔄]` (ricarica).
- Il tab Find resta invariato.

### Flusso dati

```
bootstrap()
  └─ activateTab("markdown")
       └─ loadMarkdownTab()
            └─ reloadCurrentView()
                 ├─ archive_list_tree()  →  tree (LibraryTree)
                 └─ renderCurrent()
                      ├─ findNode(tree, currentPath)  →  { folders, files }
                      ├─ renderBreadcrumb()
                      ├─ buildFolderRow(folder) per ogni folder
                      └─ buildMarkdownItem(file, reloadCurrentView) per ogni file
```

### Modulo puro `library-nav.mjs`

| Funzione | Descrizione |
|---|---|
| `isValidFolderName(name)` | Valida nome cartella: rifiuta vuoto, spazi-only, `/`, `\`, `..` |
| `isFolderEmpty(node)` | Stima ottimistica: `folders.length === 0 && files.length === 0` |
| `findNode(tree, relPath)` | Naviga LibraryTree per relPath; `null` se non esiste |
| `breadcrumbSegments(relPath)` | `"A/B"` → `[{name:"A",relPath:"A"},{name:"B",relPath:"A/B"}]` |
| `parentPath(relPath)` | `"A/B/C"` → `"A/B"`, `"A"` → `""`, `""` → `""` |
| `flattenFolders(tree)` | Pre-order piatto dell'albero: `[{name,relPath,depth}, …]`; depth 0 = primo livello |
| `moveTargets(tree, itemRelPath, isFolder)` | Destinazioni valide per spostare un elemento: self+discendenti esclusi del tutto (se cartella); il parent corrente resta in lista con `disabled:true` (ancora visiva, non selezionabile — v0.45.43); antepone ROOT se parent ≠ "" |

### Modale "Sposta" (Slice 4b — v0.28.0)

**HTML**: `<dialog id="move-dialog">` con struttura:
- `#move-dialog-title` — titolo dinamico ("Sposta «nome» in…")
- `#move-targets` — lista scrollabile delle destinazioni valide
- `#move-error` — riga errore (nascosta di default; mostrata se backend risponde Err)
- `#move-confirm` / `#move-cancel` — pulsanti footer

**CSS** (in `library.html`): `.move-dialog-box` coerente col pannello; `.move-target`
indentata per `depth`; `.move-target--selected` con background accent; `.move-target--disabled`
grigia/non cliccabile (`pointer-events:none`) per la riga-ancora del parent corrente (v0.45.43);
`dialog::backdrop` con velo semitrasparente.

**v0.45.43 — bugfix albero sbagliato**: `moveTargets` rimuoveva del tutto il parent corrente
(no-op come destinazione), ma questo orfanizzava visivamente i suoi altri figli rimasti in
lista alla loro `depth` assoluta — la UI li indentava sotto qualunque riga precedente a depth
minore, anche senza relazione reale (bug dal vivo: una sottocartella appena creata sembrava
annidata sotto una cartella estranea, semplicemente perché alfabeticamente adiacente al vero
genitore escluso). Ora il parent resta come riga `disabled` — vedi `library-nav.mjs`.

**JS** (in `library.js`):
- `moveState = { itemRel, isFolder, chosen }` — stato condiviso (un solo oggetto, non closure)
- `openMoveDialog(itemRel, isFolder, displayName)` — calcola targets, popola DOM, `showModal()`
- `wireMoveDialog()` — registra handler `#move-confirm` + `#move-cancel` + Esc fix (una volta sola a bootstrap)
- Pulsante ↗ in `buildFolderRow` + `buildMarkdownItem` (solo se `onDelete !== null`)
- Guard `fs-watch`: `if (moveDialog?.open) return;` nel listener `library:changed`

**GOTCHA Esc** (v0.28.0, invariante critica):
L'handler globale `document.keydown` chiude la finestra Library su Escape.
Il `<dialog>` nativo emette `keydown` che bubbla al document. Senza blocco, Esc
chiude il dialog E la finestra. Fix: listener `keydown` sul dialog stesso,
`stopPropagation` (non `preventDefault`) → il browser chiude il dialog nativamente
ma l'evento non raggiunge il document handler.

### Sicurezza

- Tutti i testi utente impostati via `textContent` (MAI `innerHTML`).
- Pulsanti azione con `stopPropagation()` su `click` e `dblclick`.
- Esc durante rinomina: `stopPropagation()` per non chiudere la finestra.
- Esc durante modale: `stopPropagation()` su keydown del dialog (non `preventDefault`).
- Arrow nav guarda `document.activeElement.tagName === "INPUT"` per non interferire con rinomina.
- `window.libraryReload = reloadCurrentView` espone la funzione per la Slice 3 (fs-watch).

## fs-watch auto-reload — Library a cartelle (v0.25.0)

### Modulo `library_watch.rs` (seam DIP)

Logica di watch testabile senza runtime Tauri. La callback generica
`on_change: impl Fn() + Send + 'static` è il seam: nell'app riceve l'emit
Tauri, nei test riceve un contatore `Arc<AtomicUsize>`.

```
watch_dir(dir, debounce=400ms, on_change)
  ├─ new_debouncer(debounce, mpsc::Sender)   ← notify-debouncer-mini 0.7.x
  ├─ debouncer.watcher().watch(dir, Recursive)
  └─ thread {
       for events in rx.iter().flatten() {
           if !events.is_empty() { on_change(); }
       }
     }
  → Ok(WatchGuard { _debouncer })   ← RAII: droppare stoppa il watch
```

Dipendenza: `notify-debouncer-mini = "0"` (risolto a 0.7.x con notify 8.x).
Accediamo a `notify` tramite `notify_debouncer_mini::notify::*` per garantire
unicità di versione nel grafo (evita duplicazione notify 6.x + 8.x).

### Wiring in `main.rs`

```
.setup(|app| {
  ...
  // Dopo create_dir_all della library dir:
  library_watch::watch_dir(&lib_dir, 400ms, move || {
      if let Some(win) = handle.get_webview_window("library") {
          let _ = win.emit("library:changed", ());
      }
  }) → app.manage(LibraryWatchGuard(Mutex::new(guard)))
})
```

`LibraryWatchGuard(Mutex<WatchGuard>)`: il `Mutex` rende il tipo `Sync` (bound
richiesto da `app.manage()`) senza overhead a runtime — il guard non viene mai
riletto. `#[allow(dead_code)]` sopprime il warning "field never read" (è
intentionalmente held-for-lifetime).

### Frontend `library.js`

In `bootstrap()`:
```js
if (tauriEvent?.listen) {
  tauriEvent.listen("library:changed", () => {
    if (activeTab !== "markdown") return;
    // Guardia anti-clobber: salta il reload se c'è una rinomina inline attiva.
    if (document.activeElement?.tagName === "INPUT") return;
    reloadCurrentView();
  });
}
```

`reloadCurrentView()` era già esposta come `window.libraryReload`. Il listener
condiziona il reload al tab attivo (il tab Find non dipende dall'albero cartelle)
e all'assenza di un `<input>` con focus.

**Perché serve la guardia anti-clobber:** `notify` segnala *tutte* le modifiche al
filesystem, incluse quelle originate dalla UI stessa. Es: `archive_create_folder`
scrive la dir → il watcher scatta dopo 400 ms → `reloadCurrentView()` distruggerebbe
un `<input>` di rinomina ancora aperto. La guardia `document.activeElement?.tagName === "INPUT"`
è intenzionalmente più larga di `.lib-rename-input` per coprire eventuali future input
inline senza richiedere manutenzione.

### Test

| Test | Strategia | Asserzione |
|---|---|---|
| `watch_dir_detects_a_change` | Poll fino a 3 s dopo scrittura file | `counter > 0` |
| `watch_dir_debounces_burst` | 5 write in 100 ms, sleep 700 ms | `1 ≤ count < 8` (LENIENT) |

LENIENT perché il fs-watch su Windows è timing-sensitive. Soglie volutamente
larghe per evitare flakiness su CI.

## `move_folder` + hardening `move_file` (v0.27.0)

### `archive::move_folder`

```rust
pub fn move_folder(dir: &Path, folder_rel: &str, target_parent_rel: &str) -> Result<(), String>
```

Sposta `folder_rel` dentro `target_parent_rel` (oppure nella radice documents se `""`).
Il nome della cartella è preservato. Errori:

| Condizione | Errore |
|---|---|
| `folder_rel` vuoto | Err — impossibile spostare la radice |
| Traversal su `folder_rel` / `target_parent_rel` | Err — "traversal" nel messaggio |
| Sorgente non trovata / non è dir | Err |
| Parent destinazione non trovata / non è dir | Err |
| Anti-ciclo: `target == folder` o `starts_with(folder + "/")` | Err |
| Collisione: esiste già entry con stesso nome in destinazione | Err (nessun merge) |

**Guardia anti-ciclo** — confronto sulle stringhe rel-path normalizzate (i backslash
vengono rifiutati da `validate_within_root` prima ancora del confronto):
```rust
let cycle = target_parent_rel == folder_rel
    || target_parent_rel.starts_with(&format!("{folder_rel}/"));
```

**Collisione** — `dest.exists()` prima di `rename`. Per le cartelle un merge silenzioso
è pericoloso (file con nomi coincidenti verrebbero sovrascritti invisibilmente).

### Hardening `move_file` (v0.27.0)

Aggiunto controllo `if dest_abs.exists() { return Err(...) }` prima di `std::fs::rename`.
`rename` su Windows e Linux sovrascrive silenziosamente — questo check è la rete di sicurezza.

### Nuovo comando Tauri: `archive_move_folder`

```rust
fn archive_move_folder(app, folder_rel: String, target_parent_rel: String) -> Result<(), String>
```

Risolve `documents_dir_path(&app)` e delega a `archive::move_folder`.
Registrato in `generate_handler!` accanto agli altri comandi `archive_*`.

### Test TDD — 9 nuovi casi (totale 86 test verdi)

| Test | Cosa verifica |
|---|---|
| `move_file_rejects_collision_no_overwrite` | move_file → Err se dest esiste; contenuto A intatto |
| `move_folder_into_subfolder` | "A" → "B": B/A esiste, A root scomparsa |
| `move_folder_to_root` | "Sub/Archivio" → "": Archivio in root, Sub/Archivio scomparsa |
| `move_folder_rejects_into_itself` | "A" → "A": Err anti-ciclo |
| `move_folder_rejects_into_descendant` | "A" → "A/B": Err anti-ciclo |
| `move_folder_rejects_name_collision` | dest già ha "A": Err collisione |
| `move_folder_rejects_traversal` | "../fuori": Err con "traversal" nel msg |
| `move_folder_source_not_found` | sorgente inesistente: Err |
| `move_folder_target_parent_not_found` | parent dest inesistente: Err |

## Correzione layout `library/documents/` (v0.26.0)

### Struttura filesystem

```
{app_config_dir}/
└── library/                  ← root del repository (invariata)
    ├── documents/            ← NUOVO: tutti i .md e le cartelle utente vivono qui
    │   ├── foo.md
    │   └── Progetti/
    │       └── doc.md
    └── find/                 ← INVARIATA: sessioni /find salvate
        └── find--pdf-...json
```

Il vecchio layout aveva `.md` e cartelle utente sciolti nella root `library/`, con `find/` filtrata
dall'albero — un workaround. Il nuovo layout separa nettamente i contenitori.

### Migrazione (`archive::migrate_to_documents_layout`)

```
migrate_to_documents_layout(library_dir)
  ├─ create_dir_all(library_dir/documents/)       ← sempre (idempotente)
  ├─ FASE 1: read_dir(library_dir).collect:Vec   ← close iterator prima dei rename
  │    filter: skip "find" e "documents"
  └─ FASE 2: per ogni entry
       dest = unique_name_in(documents_dir, name, is_dir)
       rename(src, dest)
```

Due fasi (collect → rename) perché su Windows modificare una directory mentre il suo
`read_dir` handle è aperto può produrre `os error 5` (Accesso negato) o entry saltate.

**Collisione**: `unique_name_in` aggiunge suffisso numerico crescente (2, 3, …):
- file con estensione: `"foo.md"` → `"foo 2.md"` (suffisso prima dell'estensione)
- cartelle/file senza ext: `"Bar"` → `"Bar 2"` (suffisso in coda)

### Wiring in `main.rs` (`.setup`, ordine)

```
1. create_dir_all(library_dir)
2. archive::migrate_to_documents_layout(&library_dir)  ← NUOVO, best-effort
3. create_dir_all(library_find_dir)
4. library_watch::watch_dir(&documents_dir, 400ms, …)  ← CAMBIATO: documents/ non library/
```

La migrazione garantisce che `documents/` esista prima del `watch_dir`.

### Comandi documento — tutti su `documents_dir_path`

| Comando Tauri | Prima (v0.25) | Dopo (v0.26) |
|---|---|---|
| `archive_delete` | `library_dir_path` | `documents_dir_path` |
| `archive_list_tree` | `library_dir_path` | `documents_dir_path` |
| `archive_create_folder` | `library_dir_path` | `documents_dir_path` |
| `archive_rename_folder` | `library_dir_path` | `documents_dir_path` |
| `archive_move_file` | `library_dir_path` | `documents_dir_path` |
| `archive_delete_folder` | `library_dir_path` | `documents_dir_path` |
| `archive_save` | `library_dir_path` | `documents_dir_path` |
| `archive_list` | `library_dir_path` | `documents_dir_path` |
| `archive_open` | `library_dir_path` | `documents_dir_path` |
| `library_dir` | `library_dir_path` | invariato (root) |
| `documents_dir` | *(non esisteva)* | `documents_dir_path` (NUOVO) |
| `save_find`, `list_find`, … | `library_find_dir_path` | invariati |

### Test TDD — 7 nuovi casi (totale 77 test verdi)

| Test | Cosa verifica |
|---|---|
| `migrate_creates_documents_dir_even_when_empty` | Fresh-install: `documents/` creata |
| `migrate_moves_md_files_to_documents` | `.md` sciolti in root → spostati |
| `migrate_moves_user_folders_to_documents` | Cartelle utente in root → spostate |
| `migrate_leaves_find_at_root` | `find/` non viene toccata |
| `migrate_does_not_move_documents_into_itself` | `documents/` non entra in se stessa |
| `migrate_idempotent` | Seconda chiamata → no-op, nessun errore |
| `migrate_collision_appends_suffix_no_overwrite` | Collisione → suffisso; entrambi sopravvivono |

### Test rimosso

`list_tree_ignores_find_subdir` — rimosso perché `list_tree` non filtra più `find/`
(non la vede: opera su `documents/`, passata da `archive_list_tree` in `main.rs`).

## Workspace integration

`crates/ui/src-tauri/Cargo.toml` declares `[workspace]` (empty table), making it a
standalone Cargo workspace.  It is listed as a member of the root workspace
(`Terminal/Cargo.toml`) but is excluded from `default-members` so `cargo test` /
`cargo build` at the repo root only touch `protocol`, `orchestrator`, and `mcp-server`.

The `[lib]` target added in Slice B (`name = "ui_lib"`, `path = "src/lib.rs"`) enables
running `cargo test -p ui` headlessly — the config tests run via `ui_lib` without
any Tauri runtime.

## `LARE_LOCAL_DIR` / `LARE_ROAMING_DIR` — override delle cartelle dati (0.45.44)

**Obiettivo:** mirror lato `ui` del lavoro fatto sull'orchestrator per `LARE_LOCAL_DIR` — lasciare
all'utente la possibilità di ridirigere le cartelle dati dell'app a un percorso arbitrario (es. un
profilo condiviso fra macchine sotto una radice comune), senza toccare il comportamento di default
quando non impostate. Entrambe usano il path fornito **verbatim** (nessun `dev.lare.terminal`
unito sopra) — i `.join(...)` specifici di ogni sito (`token`, `network.json`, `llms.json`,
`config.json`, `library`, ecc.) restano invariati sopra qualunque base risolta.

**`LARE_LOCAL_DIR`** (override di `%LOCALAPPDATA%\dev.lare.terminal\`), stessa catena di
precedenza dell'orchestrator (override specifico del sito, se esiste → `LARE_LOCAL_DIR` →
`LOCALAPPDATA` → fallback `.lare-data`), onorata in:
- `aichat_settings.rs` (`network.json`), `search_settings.rs` (`search-paths.json`),
  `llm_settings.rs` (`llms.json`) — tre funzioni di path indipendenti, stesso pattern duplicato.
- `plugins_view.rs::plugins_dir` — `LARE_PLUGINS_DIR` continua a vincere per primo (invariato);
  `LARE_LOCAL_DIR` interviene solo nel fallback quando `LARE_PLUGINS_DIR` non è impostata.
- `main.rs::read_lare_token` — **fix di correttezza, non solo estensione**: prima di questo giro
  la funzione usava SOLO `app.path().app_local_data_dir()` (l'API Tauri) per trovare il file
  `token` scritto dall'orchestrator. Con `LARE_LOCAL_DIR` impostata, l'orchestrator scrive il
  token nella cartella di override mentre la UI avrebbe continuato a leggerlo dalla cartella Tauri
  di default — due processi con due cartelle diverse in testa, handshake WS rotto in silenzio (la
  UI non troverebbe mai il token giusto). Ora controlla `LARE_LOCAL_DIR` per prima, esattamente
  come l'orchestrator, e cade su `app_local_data_dir()` solo se non impostata.

**`LARE_ROAMING_DIR`** (nuova, override di `%APPDATA%\dev.lare.terminal\`) — `config_file_path` e
`library_dir_path` (`main.rs`, vedi anche § Config module e § Archive module sotto) la controllano
prima di `app.path().app_config_dir()`, stesso schema applicato alla base Roaming invece che
Local.

**Diagnostica:** due nuove righe di log in `.setup()`, `[ui] Local data dir: <path>` (risolta con
lo stesso fallback di `read_lare_token`) e `[ui] Roaming data dir: <path>` (derivata dal parent di
`config_file_path`), così le due cartelle effettive sono visibili in console senza doverle dedurre
a mano.

**Design — nessun helper condiviso:** ogni sito duplica la stessa piccola guardia invece di un
modulo comune, coerente con lo stile preesistente del crate (`plugins_view.rs`/`search_settings.rs`
già duplicavano il pattern `LOCALAPPDATA`-con-fallback prima di questo piano).

## Config module (`config.rs`)

### Config struct

```rust
pub struct Config {
    pub action_key:         String,            // Tauri accelerator, default "F2"
    pub cursor_color:       String,            // CSS hex, default "#FFFFFF"
    pub cursor_font:        String,            // font family, default "Consolas"
    pub cursor_size:        u32,               // px, default 11
    pub position:           Position,          // default Center
    #[serde(default)]
    pub activity_indicator: ActivityIndicator, // default Title
    #[serde(default = "default_true")]
    pub web_search_enabled: bool,              // default true — authorises AI web tools per-turn
}

pub enum Position { Center, BottomCenter, NearMouse }
// serde: rename_all = "snake_case" → "center" / "bottom_center" / "near_mouse"

pub enum ActivityIndicator { Title, Status, Prompt }
// serde: rename_all = "snake_case" → "title" / "status" / "prompt"
// #[serde(default)] on the field → existing config.json without this key still loads fine

// web_search_enabled uses a named helper `default_true() -> bool` because bool's Default is
// false; #[serde(default = "default_true")] ensures missing-field deserialises to true.
```

### Where the config file lives

| OS      | Path |
|---------|------|
| Windows | `%APPDATA%\dev.lare.terminal\config.json` |
| macOS   | `~/Library/Application Support/dev.lare.terminal/config.json` |
| Linux   | `~/.config/dev.lare.terminal/config.json` |

`app_config_dir()` (Tauri v2 `PathResolver`) resolves the platform path at runtime — on Windows
this is the **Roaming** app-data folder, not Local (corrected here in 0.45.44; the doc-comment in
`config.rs` had the same stale claim, also fixed). `LARE_ROAMING_DIR` (0.45.44, see the section
above) overrides this base with a full path, checked before `app_config_dir()`.
The `config.json` filename is appended in `config_file_path()` in `main.rs`.
The directory is created by `save_to` if it does not exist.

### Persistence

- `load_from(&Path) -> Config` — infallible:
  - File missing → `Config::default()` (silent).
  - IO error → `Config::default()` + stderr warning.
  - JSON invalid → `Config::default()` + stderr warning.
  - Valid JSON → deserialized `Config`.
- `save_to(&Config, &Path) -> Result<(), String>`:
  - Creates parent dirs (`create_dir_all`).
  - Writes pretty-printed JSON via `serde_json::to_string_pretty`.

### Validation

`validate_action_key(&str) -> Result<(), String>` delegates to `Shortcut::from_str`
(from `global_hotkey` crate, re-exported by `tauri-plugin-global-shortcut`).  This is
the same parser used internally by `app.global_shortcut().register(...)`, so a key that
passes validation is guaranteed to be registerable.

## Tauri commands

| Command | JS signature | Description |
|---------|-------------|-------------|
| `get_lare_token` | `invoke("get_lare_token")` | Read `LARE_TOKEN` env var |
| `hide_overlay` | `invoke("hide_overlay")` | Hide the overlay |
| `show_overlay` | `invoke("show_overlay")` | Show + position + focus |
| `get_config` | `invoke("get_config")` | Return current Config |
| `set_config` | `invoke("set_config", { newCfg })` | Validate + persist + hot-apply |
| `open_markdown_window` | `invoke("open_markdown_window", { title, content, kind })` | Open a new Markdown output window (async) |
| `take_window_content` | `invoke("take_window_content", { label })` | One-shot retrieve `{title, content, kind}` from managed state; removes entry |
| `close_self` | `invoke("close_self")` | Close the calling Markdown window |
| `resize_self` | `invoke("resize_self", { width, height })` | Resize the calling window to logical dimensions; used for auto-height after render |
| `archive_save` | `invoke("archive_save", { title, content })` | Save Markdown content to `library/` dir; returns filename created |
| `archive_list` | `invoke("archive_list")` | List archive entries (newest first); returns `ArchiveEntry[]` |
| `archive_open` | `invoke("archive_open", { file })` | Open archived file by name; returns `ArchiveDoc`; validates no traversal |
| `archive_delete` | `invoke("archive_delete", { file })` | Delete archived file; ora accetta path relativi con `/` (es. `"Progetti/doc.md"`); usa `validate_within_root` |
| `archive_list_tree` | `invoke("archive_list_tree")` | Restituisce `LibraryTree` con `root_files` e `folders` (albero ricorsivo) |
| `archive_create_folder` | `invoke("archive_create_folder", { parent_rel, name })` | Crea una cartella; gestisce collisioni con suffisso numerico; restituisce `rel_path` creato |
| `archive_rename_folder` | `invoke("archive_rename_folder", { folder_rel, new_name })` | Rinomina una cartella esistente |
| `archive_move_file` | `invoke("archive_move_file", { file_rel, target_folder_rel })` | Sposta un file `.md`; `target_folder_rel=""` = root; v0.27.0: Err se la destinazione esiste già (no overwrite) |
| `archive_move_folder` | `invoke("archive_move_folder", { folder_rel, target_parent_rel })` | Sposta una cartella; `target_parent_rel=""` = root; Err se anti-ciclo, collisione, traversal, non trovata |
| `archive_delete_folder` | `invoke("archive_delete_folder", { folder_rel })` | Elimina una cartella SOLO se fisicamente vuota (fs è fonte di verità); rifiuta non-vuote, non trovate, traversal |
| `open_library_window` | `invoke("open_library_window")` | Open (or focus if already open) the archive browser window (`library.html`); singleton by label `"library"` |

`get_config` / `set_config` use `ConfigState(Mutex<Config>)` in Tauri managed state.
`open_markdown_window` / `take_window_content` use `WindowContentStore(Mutex<HashMap<String,(String,String,String)>>)` — tuple: (title, content, kind).
`archive_save` / `archive_list` / `archive_open` delegate to `ui_lib::archive` (pure module, no managed state).

`open_markdown_window` is declared `async fn` — required on Windows to avoid a Tauri v2 deadlock
in `WebviewWindowBuilder::build()` when called from a synchronous command context.

## Archive module (`archive.rs`) — v0.22.0

### File format

Each archived file is a plain `.md` file with this structure:

```
<!-- lare-title: <title> -->
<original Markdown content>
```

The marker is an HTML comment, which `marked` renders as an empty node and `DOMPurify` leaves
intact (harmless).  It is stripped from `content` when the file is opened via `archive::open`.

### Directory structure (v0.22.0+)

```
{app_config_dir}/library/
├── *.md                      # file flat in root
├── Progetti/                 # cartella utente
│   ├── *.md
│   └── Web/                  # sotto-cartella
│       └── *.md
└── find/                     # RISERVATA — sessioni /find (ignorata da list_tree)
    └── *.json
```

| OS      | Path |
|---------|------|
| Windows | `%APPDATA%\dev.lare.terminal\library\` |
| macOS   | `~/Library/Application Support/dev.lare.terminal/library/` |
| Linux   | `~/.config/dev.lare.terminal/library/` |

Same Roaming base as `config.json` (corrected here in 0.45.44 — see § Config module above); same
`LARE_ROAMING_DIR` override applies (`library_dir_path`, checked before `app_config_dir()`).
Directory is created lazily on first save (`create_dir_all` in `save`).

### API

```rust
// Tipi esistenti
pub struct ArchiveEntry { pub title: String, pub file: String, pub modified_ms: u64 }
pub struct ArchiveDoc   { pub title: String, pub content: String }

// Nuovi tipi (v0.22.0)
pub struct LibraryNode {
    pub name: String,           // nome cartella, es. "Progetti"
    pub rel_path: String,       // path relativo con "/", es. "Progetti/Web"
    pub folders: Vec<LibraryNode>,
    pub files: Vec<ArchiveEntry>,
}
pub struct LibraryTree {
    pub root_files: Vec<ArchiveEntry>,  // file .md in root (file semplice)
    pub folders: Vec<LibraryNode>,      // cartelle di primo livello
}

// Funzioni originali
pub fn slug(title: &str) -> String;
pub fn save(dir: &Path, title: &str, content: &str, suffix: &str) -> Result<String, String>;
pub fn list(dir: &Path) -> Vec<ArchiveEntry>;
pub fn open(dir: &Path, file: &str) -> Result<ArchiveDoc, String>;   // ora accetta path con "/"
pub fn delete(dir: &Path, file: &str) -> Result<(), String>;         // ora accetta path con "/"

// Nuove funzioni (v0.22.0)
pub fn list_tree(dir: &Path) -> LibraryTree;
pub fn create_folder(dir: &Path, parent_rel: &str, name: &str) -> Result<String, String>;
pub fn rename_folder(dir: &Path, folder_rel: &str, new_name: &str) -> Result<(), String>;
pub fn move_file(dir: &Path, file_rel: &str, target_folder_rel: &str) -> Result<(), String>;
    // v0.27.0: hardened — Err se la destinazione esiste già (no overwrite silente)
pub fn delete_folder(dir: &Path, folder_rel: &str) -> Result<(), String>;

// Nuova funzione (v0.27.0)
pub fn move_folder(dir: &Path, folder_rel: &str, target_parent_rel: &str) -> Result<(), String>;
    // Sposta una cartella preservandone il nome.
    // Guardie: traversal, src-non-trovata, target-parent-non-trovata, anti-ciclo, collisione.
```

### Security: path traversal

**`validate_within_root(base, rel_path) -> Result<PathBuf, String>`** (privata):
- Usata da `open`, `delete`, `create_folder`, `rename_folder`, `move_file`, `delete_folder`.
- Usa `path.components()` (NON `canonicalize()`) perché la destinazione può non esistere.
- Rifiuta: backslash `\`, `Component::ParentDir` (`..`), `Component::RootDir`, `Component::Prefix`.
- L'errore contiene sempre la parola `"traversal"`.

**`reject_traversal(file)`** (privata, mantenuta):
- Usata da `save`, `open_find`, `delete_find` — dove il nome è sempre semplice.
- Rifiuta qualsiasi `/`, `\`, o `..`.

### `list_tree` — strategia di visita

Visita DFS tramite funzione helper ricorsiva `collect_node_contents` con limite `MAX_DEPTH = 10`.
Questo previene loop infiniti su symlink o strutture patologicamente annidate.
La cartella `find/` è ignorata SOLO alla root (depth == 0).

### `ArchiveEntry.file` nei file in sotto-cartella

Dopo v0.22.0, `ArchiveEntry.file` può contenere path relativi con `/`:
- File in root: `"doc.md"` (invariato rispetto a prima)
- File in sotto-cartella: `"Progetti/Web/doc.md"` (nuovo)

### Slug algorithm

1. Transliterate accented chars (é→e, ñ→n, ß→ss, æ→ae, œ→oe, etc.).
2. Lowercase + map non-ASCII-alphanumeric → `-`.
3. Collapse consecutive `-`; trim leading/trailing `-`.
4. Truncate to 40 chars (no trailing `-`).
5. Empty result → `"untitled"`.

### Tauri glue (`main.rs`)

`library_dir_path(app)` mirrors `config_file_path` but returns `app_config_dir/library`.
The 3 commands call pure module functions — no managed state needed.
`archive_save` generates a millisecond timestamp as the uniqueness suffix.

## Archive browser window (`library.html` / `library.js`) — v0.11.0

### Data flow

```
User: /library OR clicks #library-btn
  → app.js: invokeCmd("open_library_window")
      → Rust: if "library" label exists → set_focus, return
              else WebviewWindowBuilder::new("library", "library.html").build()
          ← library window appears
  → library.js (in new window):
       invoke("archive_list")
           → Rust: archive::list(library_dir) → Vec<ArchiveEntry>
       render list: buildItem(entry) for each — createElement/textContent only
       first item receives focus

User: dblclick or Enter on a row
  → openDocument(file):
       invoke("archive_open", { file })
           → Rust: archive::open(library_dir, file) → ArchiveDoc{title, content}
       invoke("open_markdown_window", { title, content, kind: "markdown" })
           → new md-* window opens (library stays open)
```

### Singleton behaviour

`open_library_window` checks `app.get_webview_window("library")` before building.
If the window already exists, it calls `set_focus()` and returns immediately.
This prevents multiple identical list windows accumulating.

### Capability (`library-window.json`)

`"library"` is a fixed label not matched by `"md-*"` or `"main"`.
Without `library-window.json`, the window gets zero Tauri permissions at runtime
and all `invoke(...)` calls fail silently.

Permissions granted:
- `core:default` — IPC for `archive_list`, `archive_open`, `open_markdown_window`, `close_self`.
- `core:window:allow-start-dragging` — titlebar drag region.

### Close behaviour

`library.js` uses `close_self` (Rust command) instead of `getCurrentWindow().close()`.
`getCurrentWindow().close()` requires `core:window:allow-close`, which is not in
`core:default`; `close_self` is already registered and needs no extra capability.
This mirrors the exact approach in `window.js`.

### `modified_ms` field name in JS

`ArchiveEntry` has no `#[serde(rename_all)]`, so Tauri serialises the field as
`modified_ms` (snake_case). The JS side uses `entry.modified_ms` — NOT `entry.modifiedMs`.
Note: Tauri's camelCase conversion applies only to **command arguments**, not to
serialized return values.

### Error handling

- **Bootstrap failure** (`archive_list` fails): full-area `showError(msg)` replaces
  the list area. There is no list to preserve, so this is the right scope.
- **Per-row open failure** (`archive_open` / `open_markdown_window` fails):
  `flashRowError(row)` applies `.archive-item--error` (red inset border + background
  tint) to the specific row for 1.5 s, then removes it. The list is NOT cleared;
  all other entries remain accessible. This mirrors the `💾→✗` pattern in `window.js`.
- `CSS.escape(file)` is used in the `querySelector` attribute selector inside
  `openDocument` to guard against filenames containing CSS-unsafe characters.

### Security

All user-controlled text (title, date) is set via `textContent` only.
`data-file` is stored in `element.dataset` (a safe attribute; the value goes to
`archive_open`, which validates it against path traversal in Rust before any FS access).

### Save button in Markdown windows — sempre attivo, stesso file (v2.3.9)

Riscritto in v2.3.9: il design "una tantum" originale (v0.11.0, sotto) presumeva un contenuto
statico che non cambia più dopo l'apertura — sbagliato per una finestra `show_markdown`/canale di
output, che continua a ricevere `output:content` dopo che l'utente l'ha già salvata una volta (bug
vissuto dal vivo da Maurizio: Salva premuto a metà ricerca ha archiviato un testo parziale, mai
più riallineabile al testo finale dall'interfaccia — vedi CHANGELOG 2.3.9 e
`Docs/i18n/ita/compiti-ai-esterne/2026-09-20-salva-copia-markdown-window.md`).

`window.js` wires the `#save-btn` inside `bootstrap()` after `data` is loaded:
- Captures `data.title`; il contenuto è letto da `currentContent` (aggiornata sia dagli
  aggiornamenti di turno sia dal completamento di "Espandi" — unica fonte di verità, vedi sotto).
- Tiene `savedFile` (filename restituito dal primo `archive_save` di questa finestra, `null` finché
  non si è ancora salvato — anche per una finestra archiviata: `data.source_file` NON viene
  riusato come filename iniziale, è la sorgente del flusso "Espandi", indipendente).
- Ogni click delega a `save-state.mjs::planSave(savedFile, title, content)` (modulo puro, testato
  con `node --test`, nessuna dipendenza da DOM/Tauri) per decidere quale comando IPC chiamare:
  - `savedFile === null` → `archive_save` (comportamento invariato); il filename restituito viene
    memorizzato in `savedFile`.
  - `savedFile` valorizzato → `archive_update` (comando già esistente, già usato da "Espandi",
    vedi sezione successiva) sullo stesso filename — sovrascrive invece di duplicare.
- Guard: disabilita il bottone SOLO per la durata della singola chiamata IPC in corso (evita due
  save/update sovrapposti), **ri-abilitato sempre** in un blocco `finally`, successo o errore.
- Il testo resta SEMPRE "Salva" — mai rinominato in "Salvato" (era lo stato stabile ingannevole
  del design v0.11.0). Su errore: flash "✗" per 1.5s poi torna al testo originale (invariato).
- Chiave i18n `common.saved` rimossa (era usata solo qui, ora orfana).

#### Design originale (v0.11.0, sostituito da quanto sopra)

- Captures `data.title` and `data.content` (the original Markdown, not the rendered HTML).
- On click:
  1. If `saveBtnEl.disabled` is already true → returns immediately (guard against in-flight race).
  2. Sets `saveBtnEl.disabled = true` immediately (before the async call).
  3. On **success**: button stays disabled, text changes to "✓ Salvato" (stable for the session).
  4. On **error**: re-enables the button; shows "✗" for 1.5 s, then restores original label.
- This prevents duplicate archive files from rapid clicks or clicks overlapping with a slow save.
- Riaprire la stessa finestra in futuro era una nuova sessione → il bottone tornava abilitato.
  **Non più rilevante**: in v2.3.9 il bottone non si disabilita mai in modo stabile.

### Copia come testo / Copia come markdown (v2.4.0)

`#copy-text-btn`/`#copy-markdown-btn` in `window.html`, tra `#save-btn` e `#close-btn` — visibili
su OGNI finestra Markdown (non gated da `data.kind`, a differenza di `#save-btn` che è nascosto
su `body.help`/`body.archived`, righe 241-242).

- `#copy-text-btn`: `navigator.clipboard.writeText(contentEl.innerText)` al momento del click.
  `innerText`, non `textContent` — rispetta il layout a blocchi (a-capo tra paragrafi/voci di
  lista, celle di tabella separate da tab).
- `#copy-markdown-btn`: `navigator.clipboard.writeText(currentContent)` — stessa variabile usata
  da "Salva" (sezione precedente), il markdown sorgente attuale.
- `copyToClipboard(text)` (helper condiviso): lancia esplicitamente se
  `navigator.clipboard?.writeText` non esiste, invece di lasciare che la chiamata fallisca con un
  `TypeError` generico — unico diagnostico disponibile se il webview negasse l'API (mai usata
  prima in questo repository).
- Nessuna nuova capability Tauri: `navigator.clipboard` è un'API del webview, non un comando IPC
  (ogni file in `capabilities/` dichiara già esplicitamente "clipboard NON inclusi" — riferito al
  plugin Tauri clipboard-manager, non a questa API browser).
- Feedback: `flashCopyFeedback(btnEl, symbol)` — sostituisce il testo del bottone con "✓"/"✗" per
  1.2s poi lo ripristina (stesso pattern del flash d'errore di "Salva", ma qui usato anche per il
  successo: a differenza di "Salva", copiare non ha un problema di "stato stabile ingannevole" da
  evitare).
- CSS: aggiunti al selettore condiviso `-webkit-app-region: no-drag` insieme a
  `#save-btn`/`#close-btn` — un bottone titlebar senza questa regola erediterebbe il
  comportamento draggabile del contenitore e il click trascinerebbe la finestra invece di
  attivare il bottone.
- i18n: `md_window.copy_text_title`/`copy_text_aria`/`copy_markdown_title`/`copy_markdown_aria`
  (title/aria-label dei due bottoni). Nessuna chiave per il feedback transitorio (simboli
  hardcoded, stesso pattern di "Salva").

**Non verificato dal vivo**: la scrittura reale negli appunti richiede un click nella webview
desktop — non automatizzabile in sessione di sviluppo senza interazione diretta.

### Delete button in archive list (two-step confirm — v0.11.0)

`library.js` adds a 🗑 button to each row via `wireDeleteButton(deleteBtn, row, file)`:
- **First click** on 🗑: button enters confirm state (`.archive-item-delete--confirm`, label "Conferma");
  a 3 s timeout resets to normal if the user does not confirm.
- **Second click within 3 s**: calls `invokeCmd("archive_delete", { file })`.
  - Success: `row.remove()`; if no `.archive-item` remains, restores empty state
    (`listAreaEl.className = "empty"`, `textContent = "Nessun documento archiviato."`).
  - Error: `resetConfirm()` + `flashRowError(row)` (same 1.5 s red-tint pattern as open errors).
- `stopPropagation` on both `click` and `dblclick` on the delete button prevents
  the row's `dblclick → openDocument` from firing during a delete interaction.

## Hot-apply on `set_config`

```
set_config(new_cfg):
  1. validate_action_key(new_cfg.action_key)  ← bail here if invalid
  2. save_to(&new_cfg, config_path)
  3. *state.lock() = new_cfg.clone()
  4. apply_hotkey(app, &new_cfg.action_key)
       → unregister_all()
       → register(Shortcut::from_str(key))
```

Appearance (color/font/size) is applied by the frontend via CSS variables.
Position is used by the next `show_overlay` / hotkey call.

## Hotkey registration (Tauri v2 API)

- `Shortcut`: type alias for `HotKey` from `global_hotkey` crate.
  Implements `FromStr` — format: `"F2"`, `"Ctrl+Alt+T"`, `"Alt+F4"`, etc.
- `app.global_shortcut().register(shortcut)` — registers one shortcut.
- `app.global_shortcut().unregister_all()` — removes all registered shortcuts.
  Requires `global-shortcut:allow-unregister-all` capability.
- Handler: no identity check (`if shortcut != &f2`); only one shortcut is ever
  registered, so any handler fire is the configured key.

## Window positioning (`window.rs`)

`show_and_focus(app, &Position)`:
- `Center`       → center of monitor under cursor (Slice A behaviour).
- `BottomCenter` → horizontally centered, 60 px above bottom edge.
- `NearMouse`    → 16 px right / 8 px below cursor, clamped to monitor bounds.

Fallback chain (all variants): monitor under cursor → primary monitor → skip.

## Markdown output windows (Superficie 3 / ADR-013)

### Data flow

```
UI receives ServerMsg{ type: "open_window", title, kind: "markdown"|"help", content }
  → app.js: openMarkdownWindow(title, content, kind)
      → invoke("open_markdown_window", { title, content, kind })
           → Rust: generate label "md-<ts>-<n>"
                   store (title, content, kind) in WindowContentStore
                   WebviewWindowBuilder::new(label, "window.html")
                   .build()  ← async to avoid Windows deadlock
          ← new window appears
  → window.js (in new window):
       invoke("take_window_content")  ← label derived server-side
           → Rust: remove & return { title, content, kind } from WindowContentStore
       if kind === "help" → document.body.classList.add("help")
       marked.parse(content) → rawHtml
       DOMPurify.sanitize(rawHtml) → safeHtml
       contentEl.innerHTML = safeHtml   ← ONLY sanitized HTML ever assigned
```

### Label generation

`md-{unix_ms}-{AtomicU32}` — no uuid crate needed. Collision-resistant for a local
overlay UI (single process, single user).

### Security contract (XSS prevention)

- `DOMPurify.sanitize()` is called before **every** `innerHTML` assignment in `window.js`.
- `marked` and `DOMPurify` are vendored as local files — no CDN at runtime (offline-safe, pinned).
- `/show <script>alert(1)</script>` → DOMPurify strips the `<script>` tag → no alert fires.

### Capability: `markdown-window.json`

Dynamic window labels (`md-<ts>-<n>`) cannot be enumerated at build time.
Tauri v2 supports `"windows": ["*"]` wildcard to grant capabilities to all windows.
Only `core:default` is needed — Rust creates the window (not JS), so
`core:webview:allow-create-webview-window` is not required.

### Auto-height (v0.5.2)

After `renderMarkdown()`, `window.js` schedules a `requestAnimationFrame` callback that:
1. Temporarily sets `contentEl.style.flex = "0 0 auto"` (collapses flex growth), reads
   `contentEl.scrollHeight` (now the true content height, not the stretched height), then
   restores the previous flex value.
   **Why the flex hack is necessary:** `body` is `height:100% + flex-direction:column`;
   `#content` is `flex:1`, so the browser stretches it to fill the window.  `scrollHeight`
   is spec-defined as `max(padding-box height, content height)` — for short content it
   equals the stretched `clientHeight` (~567 px), not the actual content size.  Collapsing
   flex first lets the element report its intrinsic content height.
   `body` / `documentElement` scrollHeight cannot be used either — `overflow:hidden` clamps
   them to the viewport.
2. Adds `titlebarEl.offsetHeight + 4` (4 px = 1.5 px top/bottom body border + 1 px
   anti-scrollbar buffer).
3. Clamps the result to `[120, round(screen.availHeight * 0.85)]`.
4. Calls `resize_self({ width: window.outerWidth || 800, height: clampedHeight })`.
   Errors are silently ignored (`.catch(() => {})`).

### Vendored libraries

| Library | Version | File | Size |
|---------|---------|------|------|
| marked  | 15.0.12 | `frontend/vendor/marked.min.js` | ~40 KB |
| DOMPurify | 3.2.5 | `frontend/vendor/purify.min.js` | ~22 KB |

Both loaded as plain `<script>` tags (globals: `window.marked`, `window.DOMPurify`).

## Frontend: slash commands (`app.js`)

`/config` and `/library` are handled locally (open Tauri webview windows). Every other `/…`
is forwarded to the orchestrator as a normal `Command` via `client.sendCommand(input, slashId, webSearchEnabled)`.

| Input | Action |
|-------|--------|
| `/config` | Opens dedicated config window via `open_config_window` (UI-local, no WS send) |
| `/library` | Opens the archive browser window (UI-local, no WS send) |
| `/help` | Forwarded to orchestrator → opens Help window (WindowKind::Help) |
| `/open <target>` | Forwarded to orchestrator → opens native app |
| `/show <markdown>` | Forwarded to orchestrator → opens Markdown window |
| `/web <query>` | Forwarded to orchestrator → opens browser on search URL |
| `/reset` | Forwarded to orchestrator → resets conversation history |
| `/anything-else` | Forwarded to orchestrator → returns `Error{RoutingError}` |

## Frontend: web search toggle (`app.js` + `ws-client.js`)

`webSearchEnabled: bool` (module-level `let`, default `true`) mirrors `Config.web_search_enabled`.

| Event | Action |
|-------|--------|
| App startup (`bootstrap`) | `get_config` → `webSearchEnabled = cfg.web_search_enabled !== false` |
| Config saved (Tauri `config:saved`) | `applySavedConfig` → `webSearchEnabled = savedCfg.web_search_enabled !== false` |
| Enter (normal command) | `client.sendCommand(input, cmdId, webSearchEnabled)` |
| Slash forwarded to backend | `client.sendCommand(input, slashId, webSearchEnabled)` |

`sendCommand(input, id, webSearch = false)` includes `web_search: !!webSearch` in the WS payload,
which the orchestrator reads as `ClientMsg::Command.web_search` (protocol 0.3.0).

The toggle **only controls server-side AI web search** (`web_search_20260209`/`web_fetch_20260209`).
The `/web <query>` slash command (opens browser) is always available regardless of this toggle.

## Frontend: /config window (`config-dialog.js` + `config-window.js` + `config.html`)

La finestra /config (v0.34.0) è una Tauri webview separata: `config.html` caricato in una
`WebviewWindowBuilder` con label `"config"` (singleton — se esiste già, `set_focus`; 480×600, resizable).
`config-window.js` istanzia `ConfigDialog` con `onClose = invoke("close_self")` e, al salvataggio,
emette `tauriEvent.emit("config:saved", savedCfg)` (broadcast, payload raw). Il pannello principale
(`app.js`) ascolta questo evento in `setupConfigWindowEvents()` e chiama `applySavedConfig(ev.payload)`.

### Fields
| Field | UI | Notes |
|-------|----|-------|
| Tasto azione | Click badge → press combo | `keyEventToAccelerator(e)` builds accelerator string |
| Colour | Swatch grid + `<input type="color">` | Hex string, passed to CSS var |
| Font | `<select>` of detected monospace fonts | Font family name |
| Size | `<input type="number" min=6 max=72>` | Pixels |
| Posizione | `<select>` | Values: `center` / `bottom_center` / `near_mouse` (serde snake_case) |
| Indicatore | `<select>` | Values: `title` / `status` / `prompt` (serde snake_case) |
| Ricerca web | `<input type="checkbox">` | `web_search_enabled`: authorises AI web tools per-turn |

### Lifecycle
1. `config-window.js` chiama `invoke("get_config")` → popola i campi via `ConfigDialog.open()`.
2. **Save**: `invoke("set_config", { newCfg })` → on Ok: `onSaved(cfg)` emette `config:saved` (broadcast) → `closeSelf()`.
   On Err: display error string from Rust (e.g. invalid accelerator).
3. **Cancel** / **Esc** / **×**: `onClose()` → `invoke("close_self")` chiude la webview.

### Security
- No `innerHTML` with external data; all field values assigned via `.value` / `.textContent`.
- Accelerator is captured by listening to `keydown` and building the string from `e.code` + modifiers — user can never type an arbitrary string directly.

## Frontend: AI Chat — pulizia banner gate 2 (fix review #7, ui 0.41.0)

Quando il server risolve un voto di ammissione (admit/reject/uscita candidato) emette
`ServerMsg::AiChatAdmissionResolved { candidate }`. Percorso frontend:

- `app.js` — nuovo case `ai_chat_admission_resolved` → `pushAiChat("aichat:admission-resolved", { candidate })`.
- `admission.mjs` — funzione **pura** `removeResolvedCandidate(queue, candidate)`: ritorna una
  nuova coda senza tutte le occorrenze del candidato (idempotente). 5 test in `admission.test.mjs`.
- `aichat-window.js` — listener `aichat:admission-resolved`: applica `removeResolvedCandidate` alla
  `admissionQueue` e ridisegna il banner (`showNextAdmissionRequest`) **solo** se il candidato
  risolto era quello in testa (banner mostrato). Toglie il caso in cui il banner "ammetti X?"
  restava appeso su un voto già deciso altrove (click no-op scartato dal server come stale).

## Frontend: CSS variables

```css
:root {
  --cursor-color: #FFFFFF;
  --cursor-font:  'Consolas', 'Courier New', monospace;
  --cursor-size:  11px;
}
```

`applyAppearance(cfg)` in `app.js` sets these on `document.documentElement.style`.
Called at startup (after `get_config`) and by `applySavedConfig()` (triggered by the `config:saved` Tauri event).

## How to launch (Slice B)

Same as Slice A. From `crates/ui/`:

```powershell
$env:LARE_TOKEN = "my-dev-token"
cargo tauri dev
```

Or from repo root:

```powershell
$env:LARE_TOKEN = "my-dev-token"
cargo build -p ui
.\target\debug\ui.exe
```

## Cargo commands

```powershell
# From repo root:
cargo build -p ui           # compile only (debug)
cargo test -p ui            # 14 config unit tests (headless, no Tauri)
cargo clippy -p ui -- -D warnings
cd crates\ui\src-tauri && cargo fmt -- --check
cargo test                  # backend only (protocol + orchestrator + mcp-server)
```

## Manual acceptance test (Superficie 3 additions)

Before running: start orchestrator (`LARE_TOKEN=x cargo run -p orchestrator`), start UI (`LARE_TOKEN=x cargo run -p ui`), press F2.

11. **Basic Markdown window**: type `/show # Ciao` + Enter.
    Expected: a separate window opens, titled "# Ciao" (or truncated), rendering `<h1>Ciao</h1>`.

12. **XSS prevention**: type `/show <script>alert(1)</script>` + Enter.
    Expected: window opens. No alert dialog fires. DevTools → Elements: no `<script>` tag in DOM.
    (DOMPurify removed it before `innerHTML` assignment.)

13. **Empty show**: type `/show` + Enter.
    Expected: inline error in overlay — "nessun contenuto da mostrare". No window opens.

14. **Long title truncation**: type `/show ` followed by 80 `A` characters + Enter.
    Expected: window title bar shows exactly 60 `A` characters.

## Manual acceptance test (Slice B)

Start both orchestrator and UI as in Slice A, then:

1. **(Slice A baseline)** All Slice A tests should still pass (F2 toggle, Enter, Esc, Ctrl+L).

2. **Slash command — unknown**: type `/foo` + Enter.
   Expected: inline error "Comando slash sconosciuto: /foo" + "Comandi disponibili: /config". No WS send.

3. **Open /config dialog**: type `/config` + Enter.
   Expected: the config dialog appears inside the overlay panel, covering it.
   Fields show current values (defaults on first run: F2, #FFFFFF, Consolas, 11, Center).

4. **Change colour**: change the Colour field to `#00FF88`, click Salva.
   Expected: dialog closes; typed text and blinking cursor turn green immediately.
   Re-open `/config` — colour shows `#00FF88`.

5. **Change font + size**: open `/config`, change Font to `Courier New`, Size to `14`, Salva.
   Expected: text in the overlay uses Courier New at 14 px.

6. **Change hotkey**: open `/config`, click the key badge, press `Alt+F4`
   (then immediately refocus the terminal to avoid closing it — or use `F5` for safety).
   Badge shows `F5`/`Alt+F4`. Salva. Expected: old hotkey (F2) no longer works; new key toggles.

7. **Invalid hotkey**: open `/config`, click badge, then type — the capture input is read-only,
   so you can only press real key combos. The accelerator string is always valid.
   (To test Rust-side validation, modify `action_key` directly via devtools console and invoke;
   the command should return an error string and the dialog should display it.)

8. **Cancel**: open `/config`, change a field, press Esc or click Annulla.
   Expected: dialog closes; no changes applied; config unchanged.

9. **Persistence**: close and re-open the UI.
   Expected: the config saved in steps 4–6 is reloaded; CSS vars applied on startup.

10. **Position change**: open `/config`, change Posizione to "Basso-centro", Salva.
    Press the hotkey. Expected: overlay appears near the bottom of the screen.

## Architecture notes (SOLID)

- **SRP**: `config.rs` owns only config logic; `window.rs` owns only positioning;
  `main.rs` owns app setup and command wiring; `config-dialog.js` owns the dialog;
  `app.js` wires everything.
- **DIP**: `window.rs` receives `&Position` (value type) — not a Tauri state ref.
  `config-dialog.js` receives `tauriInvoke` and `onSaved` callbacks — no direct imports.
- **OCP**: Adding a new `Position` variant or a new slash command requires changes in
  one place each (`window.rs`/`app.js`), not a rewrite.

## Bug fixes applied in v0.5.2

### BUG-002 — Input soft-wrap (`frontend/index.html`, `frontend/app.js`)

`#typed-text` in CSS:
- `white-space: pre` → `pre-wrap` + `overflow-wrap: anywhere` — text wraps at the panel
  boundary instead of clipping horizontally.
- `flex: 0 0 auto` → `flex: 1 1 auto; min-width: 0` — flex prerequisites for the span
  to actually wrap (a `flex: 0 0 auto` item ignores the available-width constraint).
- `.input-row { align-items: flex-start }` — the `›` prompt stays on the first line when
  the input wraps.

`renderEditor()` in app.js:
- `spanBefore.style.whiteSpace` and `spanAfter.style.whiteSpace` both changed from
  `"pre"` to `"pre-wrap"` so inline styles do not override the corrected CSS rule.

### BUG-003 — Markdown window z-order and F2 (`src-tauri/src/main.rs`)

`open_markdown_window` builder:
- `.always_on_top(true)` — matches the main overlay's z-level (which is `alwaysOnTop: true`
  in `tauri.conf.json`); prevents the Markdown window from appearing behind the main overlay.
- `.focused(true)` — brings the window to the foreground immediately on open.

Global-shortcut handler (F2):
- **Hide**: iterates `app.webview_windows()` and calls `.hide()` on every window with label
  `"main"` or starting with `"md-"`.
- **Show**: iterates `app.webview_windows()`, calls `.show()` on every `"md-*"` window, then
  calls `window::show_and_focus(app, &position)` for the main window.
- `Manager` trait (already imported) provides `webview_windows()` returning
  `HashMap<String, WebviewWindow>`.

## Known limits (Superficie 3, open for future phases)

| Limit | Detail | Fix in |
|-------|--------|--------|
| `cwd` null | Each command spawns a fresh shell | Slice C |
| CSP not set | `app.security.csp` omitted pending interactive test | Slice C hardening |
| No bundling | `bundle.active = false` | Later phase |
| Config format | JSON only; no migration on schema change | Future |
| Hotkey conflicts | If another app holds the same shortcut, `register` returns an error surfaced in the dialog | Acceptable |
| `?label=` unverified | `window.html?label=X` passing through `WebviewUrl::App` to `URLSearchParams` is compile-verified but not interactive-tested | Interactive test needed |
| Markdown window memory | `WindowContentStore` entries removed on first read (`take_window_content`); if window.js never runs (crash), entry leaks until app restart | Future: TTL eviction |

## What is verified vs only-compiled

| Item | Status |
|------|--------|
| `cargo build -p ui` | Verified: compiles clean (v0.27.0) |
| `cargo clippy -p ui --all-targets -- -D warnings` | Verified: clean (v0.27.0) |
| `cargo fmt -- --check` (ui) | Verified: clean |
| `cargo test -p ui` (86 tests) | Verified: 86/86 pass (v0.27.0) |
| `cargo test` (root, backend) | Verified: 56+4 orchestrator + 25 protocol pass |
| Config round-trip, missing/corrupt file, validate_action_key | Verified by unit tests |
| `open_markdown_window` / `take_window_content` Rust compile | Verified: compiles clean (clippy/check) |
| `web_search_enabled` field + `default_true` + 3 TDD tests | Verified: `cargo test -p ui` 18/18 pass |
| `kind` threading (WindowContentStore, open_markdown_window, take_window_content) | Compile-verified (`cargo clippy -p ui` clean) |
| `data.kind === "help"` → `body.help` class in window.js | Compile-verified (`node --check window.js` OK) |
| `body.help` CSS rule in window.html | Visual verification required |
| `/help` hint in index.html | Visual verification required |
| `sendCommand` third param + `web_search` payload field | Verified: `node --check ws-client.js` + `app.js` |
| `WindowContentStore` managed state + label generation | Compile-verified |
| `?label=` survival through `WebviewUrl::App` → `URLSearchParams` | **Interactive only** — if blank window appears, fallback: pass `WebviewWindow` param to `take_window_content` and call `.label()` server-side |
| DOMPurify strips `<script>alert(1)</script>` | Interactive verification required (`/show <script>alert(1)</script>`) |
| GUI acceptance (Markdown window, /config dialog, hotkey) | Interactive only |
| `invoke("set_config", { newCfg })` arg mapping (JS→Rust) | Compile-only; not exercised at runtime in CI |

## TDD RED→GREEN cycle (reconstructed)

The tests and implementation were written together in one session (not in separate
commits). To provide an honest RED→GREEN demonstration, a stub version of `config.rs`
was prepared post-hoc (wrong defaults, `save_to` → `Err`, `validate_action_key` → always `Ok`).

**RED** — with stub bodies (9/14 fail):

```
running 14 tests
test config::tests::validate_ctrl_alt_t_is_ok     ... ok  (stub always returns Ok)
test config::tests::validate_f5_is_ok              ... ok
test config::tests::validate_f2_is_ok              ... ok
test config::tests::validate_empty_string_returns_err ... FAILED  ← stub accepts everything
test config::tests::validate_invalid_key_returns_err  ... FAILED
test config::tests::default_position_is_center     ... FAILED  ← stub returns NearMouse
test config::tests::default_cursor_font_is_consolas ... FAILED  ← stub returns "WRONG"
test config::tests::default_cursor_size_is_11       ... FAILED  ← stub returns 0
test config::tests::default_cursor_color_is_white   ... FAILED  ← stub returns "WRONG"
test config::tests::default_action_key_is_f2        ... FAILED  ← stub returns "WRONG"
test config::tests::corrupt_json_returns_default    ... ok  (stub/real both return default)
test config::tests::missing_file_returns_default    ... ok
test config::tests::round_trip_default_config       ... FAILED  ← save_to returns Err
test config::tests::round_trip_custom_config        ... FAILED

test result: FAILED. 5 passed; 9 failed
```

**GREEN** — with real implementation (14/14 pass):

```
running 14 tests
test config::tests::default_action_key_is_f2        ... ok
test config::tests::default_cursor_color_is_white   ... ok
test config::tests::default_cursor_font_is_consolas ... ok
test config::tests::default_cursor_size_is_11       ... ok
test config::tests::default_position_is_center      ... ok
test config::tests::validate_ctrl_alt_t_is_ok       ... ok
test config::tests::validate_empty_string_returns_err ... ok
test config::tests::validate_f2_is_ok               ... ok
test config::tests::validate_f5_is_ok               ... ok
test config::tests::validate_invalid_key_returns_err ... ok
test config::tests::corrupt_json_returns_default    ... ok
test config::tests::missing_file_returns_default    ... ok
test config::tests::round_trip_custom_config        ... ok
test config::tests::round_trip_default_config       ... ok

test result: ok. 14 passed; 0 failed
```

**Transparency note**: The stub→restore cycle was performed after implementation to
produce these artifacts. The tests were written with the TDD intent but not in a
separate commit before implementation. The supervisor should decide if a strict
commit-separated RED→GREEN is required for Slice C.

## Supervisor open points

1. **`set_config` JS→Rust argument mapping**: `invoke("set_config", { newCfg })` uses
   Tauri v2's default camelCase↔snake_case mapping (`newCfg` → `new_cfg`). This is
   spec-correct per Tauri v2 docs but has never executed in this codebase (all Slice A
   commands had zero JS args). Needs interactive verification on first real run.

2. **Config dir creation on `get_config`**: The spec says `get_config` "crea la dir se
   manca." The implementation reads from managed state (no disk I/O per call); the dir
   is created lazily by `save_to` on first save. This is a deliberate deviation for
   performance. The supervisor may prefer creating the dir at startup.

3. **`global-shortcut:allow-unregister-all` capability name**: Added to
   `capabilities/default.json`. Build succeeds with it, confirming it is syntactically
   valid, but runtime behavior (whether the plugin enforces it) needs an interactive run.

## [0.42.0] — Library "Share with": pulsante, dialog, banner, esito (Slice 1a-ui)

Costruita task-per-task via `superpowers:subagent-driven-development` (6 task, un subagent
implementatore + una review per task, più una review finale whole-branch). Collega il backend
già mergiato (Slice 1a, orchestrator 0.35.0) a interfaccia reale in tre finestre Tauri separate:

- **Library** (`library.js`/`library.html`): pulsante 📤 "Condividi" per documento, dialog
  `#share-dialog` che rispecchia la struttura già esistente del dialog "Sposta" (stesse classi
  CSS `.move-dialog-box`/`.move-target`/`.move-dialog-footer`, riuso deliberato). `size_bytes`
  calcolato lato client da `archive_open` + `TextEncoder` (byte UTF-8 esatti, non `.length` JS).
- **AI Chat** (`aichat-window.js`/`.html`): banner di consenso `#share-consent`, elemento
  SEPARATO dal banner `#consent` del gate di ammissione (i due flussi possono essere pendenti
  insieme). **Limite noto:** gestisce una sola offerta pendente alla volta, vedi
  `crates/ui/CHANGELOG.md` 0.42.0 per il dettaglio.
- **Pannello cursore** (`app.js`/`renderer.js`): nuovo `renderer.systemMessage(text)` per
  notifiche non legate a un comando digitato — mostra l'esito Share (accettata/rifiutata/fallita)
  fino a 24h dopo l'invio, quando la finestra Library che ha avviato la condivisione è quasi
  certamente già chiusa.
- **Ponte cross-finestra nuovo**: `library:request-roster`/`library:roster`/`library:share-document`
  (Library non aveva mai comunicato con lo stato AI Chat prima di questa slice). Pattern "pull"
  più semplice del buffer-and-replay già usato per AI Chat (il roster cambia raramente).
- `share-view.mjs` (nuovo modulo puro, testato): `shareTargetList`, `formatShareSize`,
  `shareConsentPrompt`, `shareResultLine` — riusato dalle tre finestre, zero duplicazione.
- **Fix review finale (M2):** il roster inoltrato a Library esclude ora la propria macchina
  (altrimenti compariva nel picker "Condividi con…" e falliva sempre con un messaggio
  fuorviante) — vedi `app.js::mySelfLabel`/`libraryRosterParticipants()`.

**Ancora mancante** (fuori scope, spec §1): nessun trasferimento di contenuto reale (Slice 2),
nessun pulsante "Condividi con tutti" (Slice 3, backend risponde comunque "non disponibile").

160 test JS (`node --test crates/ui/frontend/*.test.mjs`) verdi, nessuna regressione.

## [0.43.0] — Library "Share with": trasferimento contenuto (Slice 2a)

Riuso totale dei comandi Tauri esistenti — `archive_open`/`archive_save` (già usati per Library e
per il salvataggio delle finestre Markdown) coprono rispettivamente lettura-per-invio e
scrittura-per-ricezione. Zero nuovo codice Rust lato crate `ui`: le uniche modifiche sono due nuovi
`case` in `app.js` (`share_content_request`/`share_incoming_data`) e tre nuovi metodi in
`ws-client.js` (`sendShareContent`/`sendShareContentFailed`/`sendShareWritten`).

Fallimento della lettura (`archive_open` rifiuta la promise) → `ClientMsg::ShareContentFailed` con
la frase fissa `"documento non più disponibile"` (mai l'errore tecnico grezzo di `archive_open`,
che include un path completo e un errore OS in inglese). Fallimento della scrittura
(`archive_save` rifiuta) → nessun ack, nessuna riga nel pannello: silenzio, coerente con la
scelta già presa in Slice 1a-ui di non segnalare mai una "scrittura confermata" esplicita.

162 test JS (`node --test crates/ui/frontend/*.test.mjs`) verdi, nessuna regressione.

## llm_settings — lettura/scrittura provider attivo (0.43.1, Slice 5 Task 2)

**Obiettivo:** dare al futuro tab `/config` (Task 3) un modo di leggere/scrivere il provider
attivo di `llms.json`, senza mai toccare `api_keys` — vedi
`Docs/superpowers/specs/2026-07-07-llms-config-ui-tab-design.md` §4.

**Design 1 — i tipi Rust non hanno il campo `api_keys`, non lo omettono:** a differenza di
`orchestrator::llms_config::LlmsConfig` (che omette `Debug` di proposito perché PORTA
`api_keys`), `LlmSettings` non ha proprio quel campo nello schema — non c'è modo che finisca in
un log o in un payload IPC, per costruzione del tipo, non per disciplina di chi lo usa.

**Design 2 — `parse_llm_settings`/`merge_active` sono funzioni pure, separate dai comandi
Tauri:** stesso principio di `validate`/`merge_aichat_settings` in `aichat_settings.rs` — la
logica testabile non tocca il filesystem, i comandi Tauri (`get_llm_settings`/
`set_llm_settings`) sono thin wrapper che aggiungono solo l'I/O.

**Design 3 — nessun caso "crea con default" in `set_llm_settings`:** a differenza di
`set_aichat_settings` (che scrive sempre, creando la cartella se serve), qui il file DEVE già
esistere con il provider scelto — non ha senso che la UI inventi un `llms.json` vuoto quando
l'utente non ha ancora configurato nulla a mano (nessuna key da mettere).

**Test:** 7 — `parse_llm_settings` (malformato→vuoto, legge active+providers ignorando
api_keys, providers assente→lista vuota), `merge_active` (active vuoto rifiutato, provider
sconosciuto rifiutato, JSON esistente malformato rifiutato, merge preserva api_keys/altri campi
byte-per-byte — il test di sicurezza centrale di questa slice).

## Tab "LLM" in /config (0.44.0, Slice 5 Task 3 — completa)

**Obiettivo:** chiudere la Slice 5 collegando `llm_settings.rs` (Task 2) al dialog `/config` —
stesso pattern dei tab Search/AI Chat esistenti, ma con una radio-list invece di campi editabili
(sola selezione, vedi spec §1).

**Design — `_buildRadio` è un nuovo helper, non un riuso di `_buildCheckbox`:** stessa struttura
DOM (`config-field-row` con label+input), ma un gruppo di radio condivide un `name` — l'unico
modo per JS di sapere "quale riga è selezionata" è leggere `.checked` su ciascun input dello
stesso `name`, cosa che un singolo checkbox non richiede mai.

**Design — nessuna nuova classe CSS:** `config-field-row`/`config-field-label`/`config-note` già
coprono esattamente questa forma (etichetta + controllo, riga per riga, nota informativa in
fondo).

**Design — nessun tab visibile se `llms.json` non esiste:** a differenza degli altri tab (che
mostrano sempre i campi coi default), qui non ha senso mostrare radio senza opzioni — il tab
mostra invece un messaggio che spiega dove creare il file a mano.

**Verifica:** manuale (nessun modulo puro estraibile per questo tab, stesso trattamento di
`_buildAiChatTab`/`_buildSearchTab`) — vedi il piano per la checklist completa (creazione file,
selezione, ispezione post-salvataggio, file assente, riavvio orchestrator).

## Finestra ricerca: riga+snippet per gli hit di contenuto (0.44.2)

**Obiettivo:** vedi `Docs/superpowers/specs/2026-07-10-ricerca-contenuti-design.md` — la finestra
`/find` deve mostrare riga+frammento di testo quando l'hit proviene da una ricerca CONTENUTO
(`/find in:"<frase>"`, orchestrator 0.40.4/protocol 0.14.1); un hit solo-nome resta invariato.

**Design — nessun modulo puro nuovo:** a differenza di altre estensioni della finestra ricerca
(`search-status.js`, funzioni pure testate `node:test`), `addHit` resta imperativa/DOM — non c'è
logica di decisione da estrarre, solo un `if (typeof line === "number" && typeof snippet ===
"string")` che aggiunge un secondo elemento DOM. Stesso trattamento del resto di
`window-search.js` (nessuna astrazione introdotta per un ramo a costo quasi zero).

**Design — `line`/`snippet` attraversano TUTTI i percorsi che portano un hit alla finestra, non
solo il caso "live":** oltre al canale `search:hit` dell'evento Tauri in tempo reale, anche il
replay del buffer (`searchBuffers.hit`/`subscribe` in `app.js`, per una finestra che si apre DOPO
che i primi hit sono già arrivati — v0.19.0) e il replay di una ricerca salvata (Library → tab
Find, `bootstrap()` in `window-search.js`) passano i due campi. Ometterne anche solo uno avrebbe
fatto sparire lo snippet in metà dei percorsi di apertura finestra, un bug non ovvio da un test
manuale rapido.

**Sicurezza:** lo snippet segue la stessa cautela già in uso per il path — `textContent`, mai
`innerHTML` (nessuna nuova superficie di injection: il contenuto del file è dati non fidati tanto
quanto un nome file).

**Test:** nessun test nuovo (nessuna logica pura estratta) — 162 test JS invariati (`node --test
crates/ui/frontend/*.test.mjs`), nessuna regressione. Verifica manuale suggerita al supervisore:
`/find in:"<qualcosa di presente in un file di testo>"` deve mostrare una riga secondaria sotto il
path; `/find <query solo-nome>` deve restare identico a prima.

## Trasparenza alpha allineata a 0.87 (v0.44.5)

L'utente ha notato che i plugin (`/lc`, `/calc`, ecc.) e `/config` non avevano la stessa
trasparenza della finestra principale (il cursore, `index.html`). Indagine: l'app aveva 4 valori
alpha diversi sparsi tra le finestre, nessuno realmente "lo standard":

| Finestra | Alpha prima | File |
|---|---|---|
| `index.html` (cursore, finestra principale) | 0.82 | body |
| `config.html` | 0.86 (body) / **0.98** (`.config-dialog-box`, il box visibile) | `config.html` + `config.css` |
| `library.html` | 0.77 | `--bg` |
| `aichat-window.html` | 0.86 | `--bg` |
| `plugin-window.html` (shell condivisa di TUTTI i plugin) + `plugin-catalog.css`'s `--lare-bg` | 0.92 | entrambi |
| `window.html` / `window-search.html` | 0.87 (già corretti) | `--bg` |

**Causa reale del "niente alpha" su `/config`:** il body ha sempre avuto un `--bg` translucido
(0.86), ma il contenuto REALMENTE visibile è `.config-dialog-box` (in `config.css`, il pannello
che riempie quasi tutta la finestra) — quello era a **0.98**, praticamente opaco, e mascherava del
tutto la trasparenza sottostante. Bastava correggere il body a nulla sarebbe cambiato visivamente.

**Fix:** scelto **0.87** come riferimento unico (decisione utente — il valore già in uso da
`window.html`/`window-search.html`), allineate tutte le righe della tabella sopra nello stesso
commit. `crates/plugin-lc`'s propria `.lc-root` (che sovrascrive il default del catalogo con un
colore navy distinto, non il grigio-blu standard) aggiornata nello stesso commit ma con versione
di crate separata (0.3.5) — vedi il suo `CHANGELOG.md`/`IMPLEMENTATION.md`.

**Deliberatamente NON toccato:** gli alpha degli strati INTERNI di ciascuna finestra (pannelli
di `/library`, dialog/toolbar, `.lc-panels`/`.lc-panel`/header-funcbar di Lare Commander) — sono
scelte di leggibilità del contenuto, distinte dall'alpha "di finestra" (il contenitore più
esterno), che è l'unico oggetto di questo allineamento.

**Test:** nessun test automatico per questi valori (CSS puro, non logica) — verificato a mano
leggendo ogni file dopo la modifica (`grep` mirato per confermare tutti i `--bg`/`--lare-bg`/
background diretti ora dicono `0.87`), più i 168 test JS + 104 test `plugin-lc` invariati (nessuna
regressione, nessuna logica toccata). Verifica visiva finale suggerita al supervisore: aprire
`/config`, `/lc`, `/calc` e confrontare a occhio con la finestra principale.
path; `/find <query solo-nome>` deve restare identico a prima.

## Trasparenza configurabile — `Config.window_alpha` (v0.45.0)

v0.44.5 (sopra) aveva allineato l'alpha di sfondo di ogni finestra a un unico **letterale**
`0.87` — un valore fisso, non modificabile senza ricompilare. Questa release lo rende
**configurabile dall'utente** senza cambiare il default né lo scarto relativo delle finestre
che già ne avevano uno.

**Campo `Config.window_alpha` (`config.rs`).** Nuovo campo `f64` sul `Config` condiviso da
tutta la UI (non `protocol` — è **locale a `ui`**, letto/scritto solo da `get_config`/
`set_config`, mai attraversa il WS verso l'orchestrator). `#[serde(default =
"default_window_alpha")]` → un `config.json` scritto da una versione precedente (senza il
campo) carica con `window_alpha: 0.87` invece di fallire il parse. **Clamp in lettura**:
`load_from` fa sempre `cfg.window_alpha = cfg.window_alpha.clamp(0.0, 1.0)` dopo il parse —
scelta deliberata di clampare **una sola volta, alla fonte**, così ogni consumatore (7
finestre + tutti i plugin) riceve già un valore sicuro senza doverlo ri-validare ciascuno per
conto proprio. Un valore fuori range scritto a mano nel JSON (es. da un utente che edita il
file direttamente) non produce mai un `rgba(...)` invalido lato CSS.

**Slider nel tab UI di `/config` (`config-dialog.js`).** Nuovo helper `_buildSliderField`
(primo controllo `<input type="range">` del dialog — i campi numerici precedenti, es. "Duck
idle", usano `_buildField` con `type="number"`, un input testuale, non uno slider): range
0–100 (%), step 1, mostrato come percentuale ma salvato come frazione `window_alpha:
alphaField.getValue() / 100` nell'oggetto passato al comando Tauri `set_config` al salvataggio.
Il valore iniziale dello slider legge `cfg.window_alpha ?? 0.87` (mai `undefined` sullo
schermo anche se il backend non avesse ancora il campo).

**Pattern di propagazione, replicato su ognuna delle 7 "famiglie" di finestra** (cursore/
`app.js`, Library/`library.js`, `/config` stessa/`config-window.js`, AI Chat/
`aichat-window.js`, shell plugin/`plugin-window.js`, Markdown/`window.js`, Search/
`window-search.js`):
1. **Bootstrap**: al caricamento della finestra, il comando Tauri `get_config` (chiamato via
   `invoke` o l'omonimo wrapper locale `invokeCmd`, a seconda del file) legge la config
   corrente; il risultato alimenta `document.documentElement.style.setProperty(
   "--window-alpha", cfg.window_alpha)` — imposta la custom property CSS sulla radice del
   documento. Presente in **tutte e 7**, senza eccezioni.
2. **Live update**: ogni finestra registra un listener sull'evento globale Tauri
   `config:saved` (emesso da `config-window.js` al salvataggio di `/config`, **non** nuovo —
   già usato da altre feature prima di questa) e ri-applica la stessa `setProperty` col
   payload appena salvato. Risultato: cambiare lo slider e premere Salva aggiorna la
   trasparenza di tutte le altre finestre aperte in quel momento, senza riavvio e senza dover
   riaprirle. **Unica eccezione deliberata: `config-window.js` stessa** — è l'*emitter* di
   `config:saved` e si chiude subito dopo ogni salvataggio (`closeSelf` nel callback
   `onSaved`), quindi non le serve un listener: applica il valore corrente una volta sola,
   al bootstrap (vedi commento in testa al file).

Per la shell condivisa dei plugin (`plugin-window.js`), la property viene impostata su
`document.documentElement` **prima** di iniettare l'HTML del plugin dentro `#plugin-root`:
essendo il markup del plugin un discendente del documento, la eredita per la normale
ereditarietà delle CSS custom properties — nessun meccanismo di propagazione dedicato serve
lato plugin. Dettagli lato `plugin-lc` (che deriva i propri 5 `background:` inline da
`--window-alpha` con gli scarti storici preservati) in `crates/plugin-lc/IMPLEMENTATION.md`
(sezione "Alpha configurabile da `--window-alpha`") e relativo `CHANGELOG.md` (0.4.0).

**Meccanismo CSS — due varianti.** Ogni finestra dichiara `--window-alpha` con un **fallback**
letterale `0.87` (`var(--window-alpha, 0.87)`), così un mancato bootstrap (raro: solo se
`get_config` fallisse prima del primo listener) non lascia mai un `rgba(...)` senza valore
alpha:
- **Elementi a scarto zero** (lo sfondo "di finestra" propriamente detto — body/root di ogni
  finestra): `background: rgba(R, G, B, var(--window-alpha, 0.87))` diretto, nessun calcolo.
- **Elementi con uno scarto storico** rispetto al riferimento 0.87 (tab-bar/toolbar di
  Library, dialog Sposta/Condividi, pannelli interni di Lare Commander): lo scarto **relativo**
  va preservato anche quando l'utente sposta lo slider, non solo il valore assoluto. Pattern:
  `rgba(R, G, B, clamp(0, calc(var(--window-alpha, 0.87) ± OFFSET), 1))`. Il `clamp(0, …, 1)`
  è necessario perché uno scarto additivo o sottrattivo può altrimenti produrre un alpha
  fuori `[0,1]` quando `--window-alpha` è vicina agli estremi del range che lo slider consente
  (es. offset `+0.05` con `--window-alpha` a 0.98 darebbe 1.03 senza il clamp).

**Test.** `config.rs`: `default_window_alpha_is_0_87`, `missing_window_alpha_field_defaults_to_0_87`,
`window_alpha_below_zero_clamps_to_zero_on_load`, `window_alpha_above_one_clamps_to_one_on_load`
(round-trip di un valore custom coperto dai test di save/load esistenti, esteso col nuovo
campo). Lato frontend: nessun modulo puro nuovo per la propagazione (stesso trattamento già
in uso per `config:saved` nelle feature precedenti — imperativo, non logica di decisione da
estrarre); i moduli `*.mjs` esistenti restano invariati. Suite invariate salvo l'estensione
Rust: nessuna regressione nei test JS.

**Debito noto:** nessuno introdotto da questo slice. Il debito pre-esistente di `plugin-lc`
(gli offset -0.32/-0.09/-0.02 sono costanti duplicate nel CSS inline generato server-side,
non condivise con le costanti frontend) resta invariato — vedi `plugin-lc/IMPLEMENTATION.md`.

## v0.45.1 — Rimosso lo scarto relativo (correzione post-live-test)

L'utente ha testato dal vivo 0.45.0 e ha rifiutato esplicitamente il design "scarto
relativo" (§5 dello spec originale, decisione presa in brainstorming): con alpha
impostato a un valore come 0.82, i pannelli con uno scarto storico (tab-bar/toolbar
Library, dialog Sposta/Condividi, `.lc-panels`/`.lc-panel`/`.lc-panel-header`/
`.lc-funcbar` di Lare Commander) mostravano un alpha VISIBILMENTE diverso dagli altri
pannelli e diverso l'uno dall'altro — l'obiettivo dichiarato dall'utente è invece che
"se il valore di alpha è, ad esempio, 0.82, deve essere 0.82 per tutte le finestre,
intendo proprio tutte".

Rimossa la formula `clamp(0, calc(var(--window-alpha, 0.87) ± offset), 1)` da tutti e 6
i selettori che la usavano (3 in `library.html`, 4 in `plugin-lc/src/render.rs` — vedi
il changelog di quel crate), sostituita ovunque con la stessa forma piatta già usata
dai selettori a scarto zero: `rgba(R, G, B, var(--window-alpha, 0.87))`. Il colore RGB
di ciascun selettore (la sua tinta storica, non l'alpha) resta invariato — solo il
canale alpha ora è identico ovunque.

Nessun cambiamento a `Config.window_alpha`, allo slider di `/config`, o al meccanismo
di propagazione (`get_config`/`config:saved`) — la correzione è puramente nella
derivazione CSS, non nel dato o nel suo trasporto.

## v0.45.2 — Font/colore/dimensione testo unificati app-wide

L'utente ha mandato uno screenshot che confronta `/config` (titolo "CONFIGURAZIONE — /CONFIG",
testo del pannello) con Lare Commander (elenco cartelle) — i due sembravano "un'altra
applicazione". Investigato: il problema non era solo `/lc`. L'app aveva DUE convenzioni di font
coesistenti fin dall'inizio, mai riconciliate:
- `'Consolas', 'Courier New', monospace` — cursore (`index.html`, configurabile via
  `--cursor-font`), `library.html`, `window.html`, `window-search.html`, e `plugin-lc` (che
  l'aveva scelto deliberatamente per allinearsi a `library.html`, round 2 della saga plugin-lc).
- `"Segoe UI", system-ui, sans-serif` — `config.html`, `aichat-window.html`,
  `plugin-window.html` (la shell condivisa di TUTTI i plugin) e `plugin-catalog.css`'s
  `.lare-window` (quindi anche `/calc`/`/ping`/`/counter`, non solo `/lc`).

Chiesto all'utente quale standard scegliere (2 opzioni: Consolas monospace ovunque — già
maggioranza, coerente con l'identità "terminale" del progetto — vs Segoe UI sans-serif ovunque).
L'utente ha risposto con una domanda tecnica ("non esiste un font più sottile di Consolas ma
comunque monospace?") — confermato: **Cascadia Mono** (font monospace moderno di Microsoft,
spedito con Windows Terminal/VS Code, tipicamente già presente su Windows 11), più sottile di
Consolas mantenendo l'allineamento a colonna fisso. Scelto come nuovo standard unico, con
`'Consolas', 'Courier New', monospace` come fallback CSS se non installato (nessuna verifica
runtime necessaria — il fallback della `font-family` list gestisce il caso assente in modo
trasparente).

Applicato a **tutte** le finestre: `index.html` (`--cursor-font`, il default che l'utente può
ancora cambiare da `/config` — solo il FALLBACK iniziale è cambiato), `config.html`,
`aichat-window.html`, `plugin-window.html`, `library.html` (2 occorrenze: body + pannello
manifest plugin), `window.html`, `window-search.html`, `plugin-catalog.css`'s `.lare-window`, e
`plugin-lc/src/render.rs`'s `.lc-root` (vedi `plugin-lc/CHANGELOG.md` 0.4.4).

Controllato anche colore e dimensione testo mentre si era lì: **colore** — 6 finestre su 8 già
condividevano lo stesso `--text: rgba(220, 235, 255, 0.92)`; gli unici due outlier erano
`plugin-catalog.css`'s `--lare-text` (`rgba(230, 235, 245, 0.95)`, leggermente diverso — allineato)
e `plugin-lc`'s `.lc-root` (`#e0e0e0`, grigio puro senza tinta blu — allineato, vedi
`plugin-lc/CHANGELOG.md` 0.4.4). **Dimensione** — sorprendentemente già quasi uniforme: 7 finestre
su 8 (incluso `plugin-lc`) usavano già `13px`; solo `plugin-catalog.css`'s `.lare-window` era a
`14px` (allineato). Il cursore (`index.html`) resta a parte, configurabile via `--cursor-size`
(default 11px) — non toccato, è l'unica dimensione pensata per essere personalizzabile dall'utente.

Nessun test automatico copre questi valori CSS nei file `ui` (stesso trattamento di ogni altra
verifica CSS pura in questo crate — verifica solo tramite grep mirato + build/test esistenti
invariati). `plugin-lc`, che ha un harness di test Rust reale per il proprio CSS inline, ha 2 test
dedicati (vedi `plugin-lc/IMPLEMENTATION.md`).

## v0.45.3 — Cascadia Mono Light: il "font sottile" del titolo esteso al corpo

L'utente ha inviato un secondo screenshot con frecce rosse sui testi delle titlebar ("LARE —
ARCHIVIO", "LARE COMMANDER") confrontati col contenuto sotto (nomi cartella), chiedendo se il
titolo fosse in grassetto — sembrava "troppo diverso". Investigato: nessuno dei due usa
`font-weight` esplicito (`#titlebar`/`#titlebar-label` in ogni file, verificato con grep — solo
`text-transform: uppercase` + `letter-spacing`). Risposto la domanda fattuale, poi chiesto
all'utente se volesse uniformare la titlebar al corpo o viceversa — risposta: il CONTRARIO di
quello proposto. L'utente vuole il font "sottile" del titolo esteso al corpo (minuscolo dove è
minuscolo oggi, nessun maiuscolo forzato), non l'uniformazione del case.

Root cause reale: nessuna differenza di peso esisteva nel CSS (entrambi Regular/400) — la
percezione di "sottile" nel titolo veniva dall'illusione ottica maiuscolo+letter-spacing, non da
un font diverso. Per ottenere un peso VISIBILMENTE più sottile serviva un font realmente più
leggero. Verificato con PowerShell (`[System.Drawing.Text.InstalledFontCollection]`) quali pesi
Cascadia sono installati su questa macchina: Windows installa ogni peso come famiglia CSS
separata — `Cascadia Mono`, `Cascadia Mono Light`, `Cascadia Mono SemiBold`, ecc. — NON come font
variabile con asse di peso. Questo significa che `font-weight: 300` su `'Cascadia Mono'` non
avrebbe avuto alcun effetto: serve referenziare `'Cascadia Mono Light'` per nome esatto.

Cambiato lo stack font in tutte le 8 finestre + `plugin-catalog.css` + `plugin-lc/src/render.rs`
da `'Cascadia Mono', 'Consolas', ...` a `'Cascadia Mono Light', 'Cascadia Mono', 'Consolas',
'Courier New', monospace'` — poiché titlebar/tab NON avevano mai un `font-family` proprio (sempre
`inherit`/nessuna dichiarazione, ereditavano dal body), questo singolo cambio rende peso sottile
sia il titolo sia il corpo, senza toccare `text-transform`/`letter-spacing`/case da nessuna parte.
Fallback graduale: Light → Regular Cascadia Mono → Consolas → monospace generico, se una macchina
non ha il peso Light installato.

## `external-channels.js` (0.45.6)

Mirror puro (nessuna dipendenza DOM/Tauri) del registro Rust
`orchestrator::external_channel`. `EXTERNAL_TOOL_CHANNELS` vuoto in questa
release. Consumato da `app.js` (release successiva) per intercettare
`/nmap`-style slash trigger lato frontend, prima dell'invio al backend —
stesso pattern di `/config`/`/library`/`/aichat`.

> **Nota (0.45.8):** `EXTERNAL_TOOL_CHANNELS` non è più vuoto da 0.45.8 — vedi
> la sezione dedicata più sotto. Il branch d'intercettazione in `app.js` che
> qui sopra si descrive come "non scatta ancora in produzione" ORA scatta per
> `/nmap` (nessuna modifica ad `app.js` è stata necessaria: la lettura del
> registro è già dinamica).

## Finestra generica "canale esterno" (0.45.7)

`external-channel.html`/`external-channel-window.js` + `open_external_channel_window`
(Rust, singleton per `channel_id`, label `extchannel-<id>`) + capability
`extchannel-window.json`. Prima finestra secondaria che possiede una propria
`LareWsClient` invece di parlare col backend solo via IPC/eventi Tauri
relayati da `app.js` — architettura deliberatamente nuova, non un'estensione
del pattern config/library/plugin-*. `window.__LARE_EXT_CHANNEL_ID__` iniettato
via `initialization_script` prima del caricamento del modulo JS. Riusa
`LareRenderer` invariato per Chunk streaming/banner di conferma.

> **Nota (0.45.8):** le due frasi seguenti descrivevano lo stato di questa
> release e sono ORA SUPERATE — non cancellate per mantenere la cronologia,
> ma vedi la sezione 0.45.8 per lo stato corrente:
> ~~Nessun `ServerMsg::OpenWindow` gestito in questa finestra: i canali tool
> esterno non espongono `show_markdown` (per design).~~ `open_window` è
> gestito da 0.45.8 (il report nmap lo usa per aprire una finestra Markdown).
> ~~`EXTERNAL_TOOL_CHANNELS` vuoto: `app.js` non intercetta ancora nessuno
> slash reale.~~ Popolato da 0.45.8 con la voce `nmap`.

## Canale nmap: CSS banner di conferma + report → finestra/Library (0.45.8)

Tre correzioni per rendere la finestra "canale esterno" pienamente
funzionante ora che il canale nmap è il primo consumatore reale (registrato
lato backend in 0.40.19, `orchestrator::EXTERNAL_TOOL_CHANNELS`):

- **CSS del banner di conferma** (`external-channel.html`): `.confirm-banner`/
  `.confirm-buttons`/`.cmd-echo` erano definiti SOLO in `index.html`. Il
  `<style>` di `external-channel.html` è un blocco separato (nessuna
  condivisione di CSS fra le due finestre) e non li conteneva — gap deferred
  dallo spec base (`Docs/superpowers/specs/2026-07-16-external-tool-channel-design.md`),
  rimasto innocuo finché il registro dei canali era vuoto (nessun banner
  poteva mai comparire). Ora che nmap è registrato e i suoi 2 tool sono
  `SENSITIVE_TOOLS` (gate locale per-tool, ADR-007), `renderer.confirmBanner(...)`
  emette DOM reale in questa finestra prima di `nmap_os_detect` — il caso
  critico: il prompt UAC di Windows per l'elevazione non mostra MAI il target
  dello scan, quindi questo banner è l'UNICO punto in cui l'utente lo vede
  prima di concedere privilegi Administrator. Valori CSS copiati da
  `index.html` (alcuni adattati alle custom property già definite in
  `external-channel.html`, `--text`/`--border`, invece dei colori letterali
  hardcoded in `index.html`) per coerenza visiva fra le due finestre.
- **`open_window` in `external-channel-window.js`**: case nello switch di
  `handleServerMsg`, dispatch sottile verso il comando Tauri già registrato
  `open_markdown_window` (invariato, usato anche da `app.js`/`window.js`).
  Il report di uno scan nmap arriva già "confezionato" dall'orchestrator
  (Task 2 del piano di integrazione), la finestra si limita a inoltrarlo.
  Nessun test automatico dedicato (file DOM/Tauri-dependent, non logica
  pura — stessa convenzione di `cwd`/`tool_confirm_request`, mai
  unit-testati); verificato con lo smoke test dal vivo (Task 6 del piano).
  **0.45.10:** il case `save_to_library` (e il comando `archive_save` da
  quel percorso) è stato rimosso — salvava automaticamente lo stesso
  contenuto che il pulsante "Salva" preesistente della finestra Markdown
  già salvava su click, producendo doppioni in Library. Vedi `protocol`
  0.14.6.
- **Activity indicator (`external-channel-window.js`, `external-channel.html`,
  0.45.11):** `#status` (badge di connessione, gestito da
  `renderer.setStatus`) e il nuovo `#activity-indicator` sono ora fratelli
  dentro `#status-bar`, non annidati — `renderer.setStatus` sovrascrive
  `#status.textContent` per intero, quindi un indicatore annidato dentro
  sarebbe stato cancellato ad ogni cambio di stato connessione.
  `commandStarted(id)`/`commandEnded(id)` tracciano un `Set` di comandi in
  volo (più comandi potenzialmente in coda se l'utente digita più in fretta
  di quanto l'AI risponda); lo spinner braille gira finché il set non è
  vuoto. Chiamato da: il listener `keydown` dell'input (`commandStarted`,
  prima di `client.sendCommand`) e dai case `done`/`error` di
  `handleServerMsg` (`commandEnded`). Nessun watchdog — a differenza di
  `app.js`, dove serve a rilevare un'AI bloccata lato client, qui il
  timeout reale è lato server (`NmapToolClient::NMAP_CALL_TIMEOUT_SECS =
  900`, `tokio::time::timeout` in `call_scan_tool`/`call_info_tool`), e
  questa finestra non ha un pulsante di stop da mostrare/nascondere.
- **`external-channels.js`**: `EXTERNAL_TOOL_CHANNELS` passa da `[]` a
  un'unica voce (`{id: "nmap", slashTrigger: "/nmap", windowTitle: "Lare —
  nmap"}`). Effetto collaterale non richiesto ma verificato: il branch di
  intercettazione già presente in `app.js` (aggiunto nel piano base, mai
  attivo perché il registro era vuoto) diventa VIVO — `/nmap` ora apre la
  finestra dedicata invece di proseguire verso il routing normale. Nessuna
  modifica ad `app.js` in questo task: la lettura del registro era già
  dinamica, bastava popolarlo.

Test: `external-channels.test.mjs` esisteva già (dal piano base, con un test
che asseriva registro vuoto) — quel test è stato AGGIORNATO (non solo esteso)
perché la sua asserzione centrale ("il registro di produzione è vuoto") è
diventata falsa per costruzione con questa release; le 3 nuove asserzioni sul
contenuto reale del registro ne sono la copertura sostitutiva.

## Titolo in-pagina dinamico per canale esterno (0.45.9)

`open_external_channel_window` (`main.rs`) inietta ora `window.__LARE_EXT_CHANNEL_TITLE__`
oltre a `__LARE_EXT_CHANNEL_ID__`, entrambi nello stesso `initialization_script`
(un solo statement JS, due assegnazioni separate da `;`). `external-channel-window.js`
legge `window.__LARE_EXT_CHANNEL_TITLE__` all'avvio e lo scrive in
`document.getElementById("titlebar-label").textContent` prima di connettersi —
sostituisce il fallback hardcoded "Canale esterno" dell'HTML. Nessun test
automatico (logica DOM-dipendente, stessa categoria delle altre righe non
testate in questo file) — trovato e verificato nel primo smoke test dal vivo
del canale `/nmap`.

## Versione nel banner di avvio (0.45.12)

`main.rs`: l'ultimo `println!` prima di `Ok(())` in `setup()` ("Lare
Terminal started...") include ora `v{}` con `env!("CARGO_PKG_VERSION")` —
stesso pattern di `orchestrator/src/main.rs`'s `tracing::info!("Lare
Terminal orchestrator v{} starting", ws::VERSION)`, ma senza introdurre un
`VERSION` const dedicato (usato in un solo punto qui, a differenza di
`ws::VERSION` che alimenta anche `ServerMsg::ServerInfo`).

> Dal v2.2.2: quel `println!` è diventato `tracing::info!` (log su file, vedi la
> sezione in cima a questo file) — stesso testo, stessa posizione in `setup()`.

## `archive::update` — sovrascrittura in-place (0.45.20)

Prerequisito Task 1 della feature "espandi documento". `save` genera sempre
un nuovo filename (suffisso timestamp) — non c'era modo di sovrascrivere un
file ESISTENTE mantenendo lo stesso nome, necessario perché ogni click
"Espandi" deve aggiornare lo stesso documento, non crearne uno nuovo a ogni
richiesta. `update(dir, file, title, content)` riusa `validate_within_root`
(stessa guardia anti-traversal di `open`/`delete`) e fallisce esplicitamente
se `file` non esiste già — a differenza di `save`, non è mai una create.
Comando Tauri `archive_update` (wrapper diretto, stesso pattern di
`archive_delete`). Vedi `Docs/superpowers/specs/2026-07-20-library-expand-
design.md` §3.

## `source_file` — le finestre Markdown ricordano la provenienza Library (0.45.21)

Prerequisito Task 2 per "espandi documento" (Docs/superpowers/specs/2026-
07-20-library-expand-design.md §3). `WindowContentStore` era una tupla
`(title, content, kind)`: una finestra riaperta dalla Library non aveva
modo di sapere quale file l'aveva generata. Aggiunto un 4° campo
`source_file: String` (sentinella `""` = non da Library, stesso idioma di
`kind`, non `Option<String>` — nessun comando Tauri qui usa ancora quel
pattern). `open_markdown_window` lo accetta come parametro, `take_window_
content` lo restituisce nel JSON insieme agli altri campi. `library.js::
openMarkdownItem` passa il rel-path che già possiede; gli altri due call
site (`app.js`, output AI diretto; `external-channel-window.js`, output di
un canale esterno come nmap) passano sempre `""` — nessuno dei due produce
un documento che vive nella Library con un rel-path proprio.

`WindowContentStore` è una mappa condivisa da altri tre comandi che aprono
finestre non-Library (`open_search_window`, `open_plugin_window`,
`open_saved_find_window`): cambiare l'arità della tupla da 3 a 4 campi
obbliga anche i loro `insert` a fornire il 4° campo, sempre `""` per lo
stesso motivo (nessuno dei tre produce un documento Library aperto).

## Pulsante "Espandi" nei documenti Library (0.45.22)

Chiude Docs/superpowers/specs/2026-07-20-library-expand-design.md. Vive
interamente in `window.js` — nessuna finestra nuova: quando `data.source_
file` è presente (documento aperto dalla Library, Task 2), la barra
`#expand-bar` (nascosta via CSS altrimenti) mostra un campo testo +
pulsante. Il pulsante resta disabilitato a campo vuoto (nessun default
silenzioso "aggiungi altre informazioni" — decisione esplicita di
brainstorming).

Ogni click su "Espandi" apre una `LareWsClient` fresca con `channel:
"library-expand"` (canale Task 3), aspetta `server_info` per sapere che
l'handshake è accettato, poi manda UN `Command` il cui testo è prodotto da
`expand-prompt.mjs::buildExpandPrompt(currentContent, request)` — modulo
puro, testato senza Tauri/WS. I `Chunk` si accumulano in un buffer locale
(nessun rendering incrementale a metà streaming, per non mostrare Markdown
a metà formattato); su `Done` il buffer intero sostituisce `currentContent`,
va a `archive_update` (Task 1, sovrascrive lo stesso file) e poi a
`renderMarkdown` (funzione già esistente in questo file). Su `Error` il
documento NON viene toccato — `archive_update` si chiama SOLO dopo un
`Done` riuscito. La connessione si chiude (`client.disconnect()`) subito
dopo `Done`/`Error`: nessuno stato persiste tra un click e l'altro, perché
ogni richiesta rimanda l'INTERO documento (già aggiornato dal giro
precedente) — riusare una connessione con history accumulata rimanderebbe
anche versioni vecchie, confondendo il modello (vedi spec §4).

`web_search_enabled` letto da `get_config` allo stesso punto in cui
`window.js` già leggeva `window_alpha` — passato a `sendCommand` come
terzo argomento, stesso meccanismo già usato dal cursore principale.

Nessun backup pre-overwrite (decisione esplicita di brainstorming: l'utente
si fida dell'AI, il contenuto non è considerato critico).

## Fix: niente retry silenzioso né sovrascrittura vuota in "Espandi" (0.45.23)

Segue immediatamente 0.45.22 (pulsante "Espandi"). Due problemi trovati in
review, entrambi risolti con logica pura testata in `expand-prompt.mjs`
invece che inline in `window.js`:

`isConnectionFailureStatus(status, alreadySettled)` — `LareWsClient` non ha
un tetto ai tentativi di riconnessione; senza questa funzione, `onStatus`
era un no-op e una connessione persa a metà streaming o prima ancora di
ricevere `server_info` non veniva mai trattata come fallimento — la UI
restava bloccata su "espando…", oppure una riconnessione futura faceva
ripartire silenziosamente lo stesso prompt (un nuovo `server_info` fa
scattare di nuovo `sendCommand`). Ora `onStatus` chiama questa funzione;
su esito vero, `failExpand` + `client.disconnect()` (che ferma anche il
retry interno di `LareWsClient`, impostando `_intentionalClose`).

`isEmptyExpandResult(content)` — un `Done` che arriva con un buffer
DAVVERO vuoto (zero chunk mai ricevuti) è distinto dal caso "l'AI ha
scritto qualcosa di sbagliato" (rischio accettato, nessun backup per
decisione esplicita) — qui non c'è NULLA da salvare, quindi si tratta
come un fallimento invece che un successo con sovrascrittura vuota.
Questo è un caso più stretto e separato da un fallimento del turno
AI (errore backend, rifiuto, risposta degenere) — quei tre casi sono
ora segnalati dall'orchestrator con `Done{ exit_code: Some(1) }` e
intercettati da `isAiTurnFailure` PRIMA che questo controllo scatti
(vedi la voce di CHANGELOG di questo fix).

Entrambe le funzioni sono testate in isolamento (`expand-prompt.test.mjs`,
7 test nuovi) — nessuna dipendenza da DOM/WebSocket, stesso principio già
seguito per `buildExpandPrompt`.

## Fix: sovrascrittura silenziosa su fallimento del turno AI in "Espandi" (0.45.24)

Bug Critical trovato nella review finale whole-branch: `runExpand()` trattava
QUALUNQUE `Done` con buffer non vuoto come successo e chiamava
`archive_update` — ma i tre percorsi di fallimento lato orchestrator
(`ai_adapter.rs`: errore backend/API, rifiuto AI, risposta degenere)
emettono un Chunk placeholder ("[errore AI] …", "[AI: richiesta rifiutata
— …]", "[nessuna risposta dall'AI — riprova]") seguito dallo STESSO
`Done{exit_code: None}` di un successo vero. Il buffer non vuoto (il
placeholder stesso) superava `isEmptyExpandResult` e veniva scritto su
disco al posto del documento originale, senza backup.

`isAiTurnFailure(exitCode)` — nuova funzione pura in `expand-prompt.mjs`:
`exitCode !== null && exitCode !== undefined`. Segue una convenzione già
in uso in questo codebase (`renderer.js` nella cursor UI branch già su
`exitCode === null` per decidere se renderizzare come Markdown un
`Done`); `exit_code: Some(1)` per i tre fallimenti sopra è l'estensione di
quella stessa convenzione, valore-only, nessun cambio di schema del
protocollo (vedi `orchestrator` CHANGELOG 0.40.35).

`window.js`'s `onMessage` chiama `isAiTurnFailure(msg.exit_code)` PRIMA di
`isEmptyExpandResult(buffer)` nel branch `done`: se vero, `failExpand`
riceve un messaggio che include il testo d'errore reale (`buffer.trim()`)
e dichiara esplicitamente che il documento NON è stato toccato. Finding
Important della review di questo fix (0.45.24): `failExpand` costruiva
quel messaggio ma lo scartava, mostrando sempre "✗ errore" generico nella
barra di stato — troppo poco per rassicurare l'utente che il file su
disco è intatto. Corretto in 0.45.25: `expandStatusEl` mostra `message`
per intero.

Test: 4 nuovi in `expand-prompt.test.mjs` (13 totali) per `isAiTurnFailure`
su `null`/`undefined`/`1`/`130`; nessun test automatico per il wiring in
`window.js` (nessun modulo puro estraibile per quella parte — dipende da
`LareWsClient`/DOM, coerente con la convenzione di progetto per cui solo
la logica pura viene estratta e testata).

Rimossa anche `expandBarEl` (`document.getElementById("expand-bar")`,
riga 42): costante dichiarata e mai usata altrove nel file (debito Minor
dalla review del Task che ha introdotto "Espandi").

## Canale python-ping: seconda voce del registro (0.45.27)

Task 3 dell'infrastruttura tool esterni Python (MCP) — vedi
`Docs/superpowers/specs/2026-07-21-pytools-infrastructure-design.md`. I
Task 1-2 (lato `orchestrator`) hanno costruito `PythonMcpToolClient` e
registrato `"python-ping"`/`/pyping` in `EXTERNAL_TOOL_CHANNELS` lato
Rust; questo task lo rende raggiungibile dal cursore.

`external-channels.js`: `EXTERNAL_TOOL_CHANNELS` passa da una voce (`nmap`)
a due — aggiunta `{ id: "python-ping", slashTrigger: "/pyping",
windowTitle: "Lare — Python ping" }`, mirror strutturale esatto della voce
`nmap` preesistente. Nessuna modifica ad `app.js`: `findExternalChannelBySlash`/
`open_external_channel_window` leggono il registro dinamicamente (già
verificato quando `nmap` fu aggiunto in 0.45.8) — una seconda voce nell'array
è sufficiente perché `/pyping` apra la finestra "canale esterno" generica,
esattamente come `/nmap`.

Effetto collaterale verificabile dal vivo: digitando `/pyping <messaggio>`
dal cursore si apre la finestra dedicata; il messaggio dentro quella
finestra viene inoltrato all'AI del canale (system prompt
`PYTHON_PING_SYSTEM_PROMPT`, Task 2), che chiama il tool `pyping` esposto
dal server MCP Python (`pytools/python-ping/server.py`, Task 3 — vedi
`pytools/README.md`), a patto che il venv sia stato creato a mano
sull'macchina di sviluppo (provisioning manuale, per design — vedi lo spec).

Test: `external-channels.test.mjs` — nuovo test `EXTERNAL_TOOL_CHANNELS has
the python-ping entry` (RED confermato prima della modifica: la voce non
esisteva, `findExternalChannelBySlash` restituiva `null`); il test sul
conteggio del registro è stato rinominato da "has exactly the nmap entry" a
"has the nmap entry" (la lunghezza esatta non è più un invariante stabile,
stesso principio già applicato lato Rust per
`production_registry_starts_with_nmap_channel`). Nessun test automatico per
`pytools/python-ping/server.py` in questo repo (è Python, non Rust/JS — la
sua copertura vive lato `orchestrator`: `python_mcp_tool_client.rs` ha un
test `#[ignore]` che fa lo spawn reale contro il venv effettivo).

## `screener-picker.{html,js,mjs}` (0.46.3)

Finestra Tauri separata (decisione utente in brainstorming — non un modale
dentro /markets, per avere più spazio). `screener-picker-list.mjs`: stato
puro `{items, index}`, `index === -1` = nessuna selezione possibile (items
vuoto). `screener-picker.js`: riceve `{title, content, kind, source_file}`
da `take_window_content` — riuso deliberato dei 4 campi generici esistenti,
`content` = JSON degli item (stesso schema con cui `open_search_window`
riusa `content` per `sid`), `source_file` = label della finestra /markets
opener. Conferma (click/Enter) → `tauriEvent.emit("screener:picked",
{opener_label, id, title})` (evento GLOBALE, filtrato lato ricevente per
label — questo codebase non usa mai `emitTo`, verificato via grep prima di
scrivere questo modulo). Esc/chiudi → nessun evento, nessuna modifica alla
conversazione /markets. Non ancora raggiungibile da nessuna finestra: manca
il comando Tauri `open_screener_picker_window` (prossima voce).

## Picker screener: comando + wiring (0.46.4)

`open_screener_picker_window` (main.rs): `webview: tauri::WebviewWindow`
iniettato da Tauri identifica la finestra CHIAMANTE (`/markets`), la sua
label finisce nello slot `source_file` di `WindowContentStore` — riuso
deliberato dei 4 campi generici esistenti (title/content/kind/source_file),
nessuno struct nuovo. Label univoca per invocazione
(`screener-picker-<ts>-<ctr>`), NON singleton (a differenza di
`open_external_channel_window`): ogni apertura è una lista fresca.

`external-channel-window.js`: `submitCommand(text)` estrae la logica
condivisa fra l'Enter sull'input-box e il listener `screener:picked` —
entrambi finiscono per mandare un `Command` di testo normale sulla
connessione WS della finestra, esattamente come se l'utente avesse
digitato "Esegui lo screener <title>" a mano. Nessun `ClientMsg` nuovo:
il picker è "solo" un modo più comodo di scrivere quel testo.

## Picker screener: click seleziona, doppio click conferma (0.46.5)

`screener-picker.js`: il listener `click` sulla riga non chiama più
`confirmSelection()` -- ora fa solo selezione (stessa logica già usata da
`mouseenter`: aggiorna `state.index` + `render()`). Aggiunto un listener
`dblclick` che chiama `confirmSelection()`, stessa funzione già usata da
Invio. Nessuna modifica a `screener-picker-list.mjs` (il modulo puro
testato non cambia forma: `createPickerState`/`moveSelection`/
`selectedItem` restano gli stessi). Motivato da un report utente nel primo
smoke test dal vivo -- vedi CHANGELOG per il dettaglio della diagnosi.
