# HANDOFF — Lare Terminal 2.0

> Checkpoint per ripartire a contesto azzerato. Si aggiorna nello stesso commit di ogni release
> (hook `commit-msg`). Stato dettagliato per area: `crates/<crate>/IMPLEMENTATION.md`.

## Versioni correnti

(a fine piano 3 "finestra terminale" — lette da ogni `Cargo.toml`/`.csproj`; `protocol` resta
2.1.0, non toccato da questo piano)

- protocol 2.1.0 (da v1 0.15.4)
- startup-config 2.0.4 (da v1 0.1.0; `spawn_detached` piano 3 Task 3; 2.0.4 — modulo `logging`
  condiviso + stdio chiuse + `child_stderr_log_sink`, fix finestre console spurie)
- mcp-server 2.0.1 (da v1 0.7.1)
- mcp-nmap 2.0.0 (da v1 0.8.2)
- orchestrator 2.2.1 (da v1 0.41.21; 2.2.1 — `CREATE_NO_WINDOW` sui figli + stderr su file)
- plugin-protocol 2.0.0 (da v1 0.2.1)
- plugin-ping 2.0.0 (da v1 0.1.0)
- plugin-counter 2.0.0 (da v1 0.1.0)
- plugin-calc 2.0.0 (da v1 0.2.0)
- plugin-lc 2.0.0 (da v1 0.4.5)
- plugin-crypto 2.0.0 (da v1 1.0.1)
- ui 2.2.2 (da v1 0.47.1; 2.2.2 — niente console mai, log su file, via il thread "q")
- lare-shell 2.0.1 (piano 2b 2.0.0 → 2.0.1 nel piano 3, Task 3: `Launcher.EnsureUi()` passa
  `--no-terminal` — non un crate Cargo: `shell/lare-shell/`, .NET/C#)

## FATTO

- 2026-09-04/05 — brainstorming, spec, due spike (`Docs/i18n/ita/spikes/`).

**Piano 1 — "fondamenta"** (`Docs/i18n/ita/superpowers/plans/2026-09-05-piano-1-fondamenta.md`),
completato il 2026-09-05:

- **Task 0** — Scaffolding del repo: workspace Cargo, `CLAUDE.md`, hook `commit-msg`. Commit `3ab5848`.
- **Task 1** — Copia dei crate v1 nel workspace 2.0, bump a `2.0.0`, baseline verde
  (`orchestrator`: 901 test passati). Commit `3c620fc`; fix round 1 `da7a7b8` (fixture di test
  spostate dentro il crate, non in `Docs/`).
- **Task 2** — `startup-config` 2.0: riscrittura completa, `--config-dir`/`startup.json` nuovo
  schema, nessuna variabile d'ambiente. Commit `38a6d39`.
- **Task 3** — `mcp-server`: migrato a `--config-dir`, niente più env var. Commit `12cc6ee`.
- **Task 4** — `orchestrator`: `--config-dir`, `RuntimeConfig` (context object), figli spawnati
  con `--config-dir` esplicito, log su file + `--console-log`. Commit `9aa1737`; fix round 1
  `91fda19` (modulo `logging.rs`: niente più panic se la cartella di log non è scrivibile).
- **Task 5** — Python `pytools`: `config_dir.py` sostituisce `local_dir.py`, `--config-dir` al
  posto di `LARE_LOCAL_DIR`. Commit `41fd5ff`.
- **Task 6** — `ui`: `--config-dir`, `startup.json`, un solo risolutore (`ConfigDirState`) al
  posto dei sei sparsi in v1. Commit `e99e3fe`.
- **Task 7** — `ui`: overlay F2 rimosso, pagina host nascosta (`host.html`/`host.js`), flag di
  sviluppo `--open config|library`. Commit `632fb45`.
- **Task 8** — `Test Run\` (layout di deploy dentro il repo) e `deploy_test_run.ps1`; verificato
  dal vivo (orchestrator + `ui.exe` da `Test Run\`, nessun processo residuo). Commit `68bfb9d`;
  fix `d75cdee` (gitignore file generati), `2b01172` (gitignore `config.json`).
- **Task 9** — Documentazione narrativa 2.0 (`RUN-LOCAL.md`, `DEPLOY.md`, `TESTING-e2e.md`,
  `KNOWN-ISSUES.md`, `06-decisions.md` con ADR-015..017) e questo aggiornamento di `HANDOFF.md`.
  Questo commit di release (`release: piano 1 completato — fondamenta 2.0 (config unica, ui
  host di finestre, Test Run)`).
- Fix wave della review finale (whole-branch, findings I1-I3/M1/M2/M3/M4/M6/M7/M8): assolutizzazione
  di `--config-dir` spostata in `startup-config`, `--config-dir` anche ai plugin, pulizia commenti
  frontend `app.js`→`host.js`, `LISTEN_ADDR` morta, exit code di `deploy_test_run.ps1`. Commit
  `93eb355`.

**Piano 2a — "protocollo shell"** (`Docs/i18n/ita/superpowers/plans/2026-09-05-piano-2a-protocollo-shell.md`),
completato il 2026-09-06 — `protocol`/`orchestrator`/`ui` 2.1.0. Test: 1472 nel workspace
(`cargo test`, default-members, senza `ui`) + 136 di `ui` (`cargo test -p ui`) + 231 JS
(`node --test crates/ui/frontend/*.test.mjs`); nessuna regressione clippy rispetto a `main`
(10 warning invariati):

- **Task 1** — `protocol`: `Role{Ui,Shell}`, campi additivi di `Hello`, i messaggi del canale
  shell (`ExecInShell`/`ExecResult`, `OpenOutputWindow`/`OutputWindowContent`, `OpenUiLocal`,
  `UiPing`/`UiPong`, `ActivityIndicator`), `ServerMsg::surface()` (match esaustivo senza
  wildcard). Commit `44e025a`.
- **Task 2** — `orchestrator`: `connections.rs`, il registro delle connessioni vive (sink `ui`,
  sessioni shell, ping `ui` pendenti). Commit `90d91c0`.
- **Task 3** — `orchestrator`: `ShellConfirmer` (gate SEMPRE, composizione su `LocalUiConfirmer`)
  ed etichetta `(interattivo)` nel prompt `[Y/n]` di `run_in_session`. Commit `a3070ce`.
- **Task 4** — `orchestrator`: `ShellSessionToolClient`/`ShellSessionState` — `run_in_session`
  come round-trip `ExecInShell`/`ExecResult`, cwd per sessione. Commit `a027994`.
- **Task 5** — `orchestrator`: `surface.rs`, il router che bufferizza i `Chunk` e li consegna
  una volta alla finestra di output a `Done`/`Error`. Commit `f53094e`; fix `5f863d7` (una sola
  terminazione per turno verso la shell).
- **Task 6** — `orchestrator`: `shell_slash.rs` (pre-router: `/ai` con virgolette, discard,
  `OpenUiLocal`) e `/help` 2.0. Commit `6f12ea1`; fix `6bd58e8` (testo di `/ping` in `/help`,
  doc di `KNOWN_BACKEND_SLASHES`).
- **Task 7** — `orchestrator`: `/ping` built-in — sonda plugin usa-e-getta, uptime, report per
  strato. Commit `4244b7b`.
- **Task 8** — `orchestrator`: cablaggio in `ws.rs` — `HelloInfo`, dispatch per `Role`, nuovo
  modulo `shell_turn.rs`, `ExecResult`/`UiPong` risolti nel registro. Commit `e33a455`.
- **Task 9** — `ui`: finestra di output aggiornabile, `open_ui_local`, `/help` singleton,
  `ui_pong`. Commit `1f3db93`; fix `63b99f6` (buffer-and-replay di `output:content` —
  `output-buffer.mjs` + protocollo `output:subscribe`/`output:closed`).
- **Task 10** — `orchestrator`/dev: 5 scenari e2e del canale shell (`tests/ws_integration.rs`) +
  client di sviluppo `scripts/dev/shell-client.mjs` (imita `lare-shell`, nessuna dipendenza).
  Commit `2449318`; fix `c3325a4` (niente marcatore di cwd sulla console con `capture:false`,
  `--selftest`).
- **Task 11** — documentazione: ADR-018, tre emendamenti allo spec (`Hello.version`, discard di
  `/find`/`/nowin`, eccezione `/help`/`/show` senza finestra di output), versioni 2.1.0,
  CHANGELOG/IMPLEMENTATION dei tre crate, questo aggiornamento di `HANDOFF.md`. Commit di
  release (`release: piano 2a completato — canale shell nel protocollo e nell'orchestratore
  (protocol/orchestrator/ui 2.1.0)`).

**Piano 2b — "host C# `lare-shell`"** (`Docs/i18n/ita/superpowers/plans/2026-09-06-piano-2b-host-lare-shell.md`),
completato il 2026-09-07 — nuovo componente **`lare-shell` 2.0.0** (`protocol`/`orchestrator`/`ui`
restano 2.1.0, non toccati). Test: 115/115 (`dotnet test shell/lare-shell/LareShell.sln`); `cargo
test` invariato (1475 verdi, riverificato). Verificabile in **modalità B** (profilo Windows
Terminal "Lare Terminal", `lare-shell.exe` nudo):

- **Task 0** — soluzione .NET, configurazione (`--config-dir`, `startup.json`, `token`), log su
  file. Commit `dcc5a63`; fix `6b29d7a` (`UnauthorizedAccessException` catturata in
  `StartupConfig`/`TokenFile`, test del log no-op).
- **Task 1** — `Wire`: i messaggi del canale shell (parse/serializzazione snake_case). Commit
  `2ce340b`; fix `89aff4b` (`ts` di `pong` obbligatorio, campi numerici malformati →
  `WireException` invece di un'eccezione grezza).
- **Task 2** — `OrchestratorClient`: handshake `hello`/`server_info`, loop di ricezione su thread
  proprio → `Channel`, server WS finto in-process (`FakeOrchestrator` su `TcpListener`, mai
  `HttpListener`). Commit `78fa217`; fix `3990362` (socket rilasciato su timeout di connessione,
  `Connect` no-op se già connesso).
- **Task 3** — classi host (`LareHost`/`LareHostUI`/`LareRawUI`), copiate dallo spike con
  adeguamenti (seam `IConsoleModes`, `OutputRecorder` con cap 200 KB testa+coda). Commit `a59d454`;
  fix `1ef1703` (`SupportedOSPlatform=windows` al posto di `NoWarn`, test `WriteProgress` e
  fallback delle mode).
- **Task 4** — runspace ospitata (PSReadLine da pwsh, execution policy da file), profili con
  `$PROFILE`, `Executor` (capture, exit code, cwd, Stop) — include il fix RID `win-x64` scoperto
  durante l'implementazione (vedi "Debiti" sotto). Commit `36c4c0a`; fix `174a59f` (comando in
  `try/finally`, non un append diretto: `return`/`. { }` rompevano la cattura di `$?`, verificato
  empiricamente in due iterazioni).
- **Task 5** — gate `[Y/n]` (`ConsoleGate`, polling dei tasti) e `SlashTurn` (ciclo del turno sul
  thread del REPL, contratti (a)-(d) del piano 2a). Commit `6614985`; fix `1a2f267` (guardia
  `Done.Id` rafforzata nei test, gate chiuso su EOF, eccezione dell'executor → cancel invece di un
  turno appeso).
- **Task 6** — `Launcher`: autostart di `orchestrator.exe` (retry 5 s) e di `ui.exe`, processi
  senza console ereditata. Commit `1970b4e` (review Approved, nessun fix necessario).
- **Task 7** — `Repl` completo (PSReadLine, profili, slash → `SlashTurn`, Ctrl+C, riconnessione on
  demand), OSC 9001 `intercept`, `--selftest`; smoke test dal vivo (autostart di
  orchestrator+ui verificato). Commit `ba890f5`; fix `dda8c0a` (handler Ctrl+C mai lancia — niente
  `using` sul CTS del turno, `--selftest` fallisce se `startup.json` manca, banner PSReadLine
  preciso).
- **Task 8** — `deploy_test_run.ps1` pubblica la host (`dotnet publish` framework-dependent
  win-x64) in `Test Run\shell\` (41,4 MB misurati), `install-wt-profile.ps1` installa il profilo
  Windows Terminal "Lare Terminal". Commit `8f3dc9f`.
- **E2E dal vivo** (controller, due passate in modalità B): trovato e corretto un difetto (`ui.exe`
  in build debug rubava il fuoco a Windows Terminal) — fix `a8d148f` (2 file + test, 115/115), poi
  riverificato (autostart senza scheda WT spuria, riconnessione dopo aver ucciso l'orchestratore).
  Dettaglio in `TESTING-e2e.md` Parte 6.
- **Task 9** — Documentazione (`CHANGELOG.md`/`IMPLEMENTATION.md` di `shell/lare-shell/`, ADR-019,
  emendamenti allo spec §4.4/§6.4/§9/§13, `DEPLOY.md`/`RUN-LOCAL.md`/`TESTING-e2e.md`/
  `KNOWN-ISSUES.md`, questo aggiornamento di `HANDOFF.md`). Questo commit di release (`release:
  piano 2b completato — host C# lare-shell 2.0.0 (modalità B in Windows Terminal), ADR-019, e2e
  Parte 6`).

**Piano 3 — "finestra terminale"** (`Docs/i18n/ita/superpowers/plans/2026-09-07-piano-3-finestra-terminale.md`),
completato il 2026-09-07 — **modalità A** (spec §2.3/§5, ADR-016): `ui.exe` apre di default una
finestra terminale (xterm.js + ConPTY) con `lare-shell.exe` come processo figlio, diventando
l'app che l'utente avvia direttamente — `orchestrator`/`ui` **2.2.0**, `startup-config` 2.0.3,
`lare-shell` 2.0.1. Test (riverificati al Task 7): `cargo test` (default-members) 1481 verdi;
`cargo test -p ui` 141 verdi (92 lib + 49 bin); `node --test crates/ui/frontend/*.test.mjs`
242/242; `dotnet test shell/lare-shell/LareShell.sln` 118/118 (115 alla chiusura del piano 2b + 3
aggiunti da `22dcb09` (review finale piano 2b — `ReplTests.cs`/`ExecutorTests.cs`/
`SlashTurnTests.cs`), PRIMA dell'inizio di questo piano — Task 3 di questo piano modifica
un'asserzione esistente in `LauncherTests.cs`, non aggiunge test propri):

- **Task 0** — scaffold: vendored xterm.js/`addon-fit` dallo spike, dipendenze Rust
  (`portable-pty`/`base64`/`rand`), `build.rs` con `rerun-if-changed`, rimosso il flag di sviluppo
  `--open` (piano 1). Commit `83f9a77`; pre-flight `7dbc143` (Task 0 Step 3 corretto: `build.rs`
  esisteva già).
- **Task 1** — 4 moduli JS puri (`base64.mjs`, `osc-lare.mjs`, `indicators.mjs`,
  `fit-debounce.mjs`, TDD RED→GREEN). Commit `60cf744`.
- **Task 2** — `pty.rs`: comandi `pty_spawn`/`pty_write`/`pty_resize`, `PtyOutputSink` (seam di
  test), `SharedPtyState`. Commit `a5d66f2` (review clean; deviazione isolata ai test — DSR nel
  fake, approvata come correzione legittima).
- **Task 3** — self-heal Rust dell'orchestratore (`launcher.rs`, `ensure_orchestrator`),
  `startup_config::spawn_detached`, flag `--no-terminal` (letto da `ui.exe`, generato da
  `Launcher.cs` — `lare-shell` 2.0.1). Commit `59ba6f4`.
- **Task 4** — la finestra terminale (`terminal.{html,css,js}`, capability
  `terminal-window.json`, costruzione in `.setup()`): xterm.js + ConPTY, consumo dell'OSC 9001
  `intercept` (emesso dalla host dal piano 2b, mai letto da un emulatore prima d'ora), bottone
  "riavvia". Commit `61ae928`; smoke test manuale dal vivo deferito al Task 7 (nessun ambiente
  GUI per i subagenti).
- **Task 5** — consumo di `ActivityIndicator` (emesso dal piano 2a, mai letto da un consumatore
  prima d'ora): relay `host-dispatch.mjs` → evento `terminal:activity`. Commit `651743b`.
- **Task 6** — `orchestrator`: autostart di `ui.exe --no-terminal` quando manca il sink
  (`RuntimeConfig::ui_exe()`, `ensure_ui_sink` in `shell_turn.rs`) — chiude l'autostart reciproco
  completo previsto da §6.4. Commit `3c9cb02`.
- **Task 7** — Documentazione: ADR-020, emendamento allo spec §9 (riga "finestra non disponibile"
  → `NO_UI_ACK`, non un `Error` dedicato) e §6.4 (nota `--no-terminal`), sezione "Modalità A" in
  `RUN-LOCAL.md` (con la sua seconda nota `--open` aggiornata), "Parte 7" in `TESTING-e2e.md`
  (checklist e2e modalità A **ancora da eseguire dal vivo** — nessun accesso GUI durante questo
  task, esplicitamente deferita al controller), versioni 2.2.0/2.2.0/2.0.3, CHANGELOG/
  IMPLEMENTATION di `ui`/`orchestrator`/`startup-config` (chiude anche il gap IMPLEMENTATION.md
  di `ui` lasciato aperto dal Task 3, e lo `[Unreleased]` di `startup-config` lasciato dal Task 3),
  questo aggiornamento di `HANDOFF.md`. Questo commit di release.

- **Fix chiusura finestra (dopo il Task 7, non uno degli 8 task pianificati)** — difetto reale
  trovato dal controller nell'e2e dal vivo condotto DOPO il completamento del piano: chiudere la
  finestra terminale (bottone X) non terminava né `lare-shell.exe` (pty child) né `ui.exe` stesso,
  che restavano entrambi in esecuzione. Corretto: `pty.rs` cattura un `ChildKiller` allo spawn e
  lo usa su `WindowEvent::CloseRequested` (`main.rs`), poi l'intero processo esce
  (`AppHandle::exit(0)`). Nel farlo, scoperto un bug upstream in `portable-pty` 0.9.0 su Windows:
  `WinChildKiller::kill` legge la condizione di successo di `TerminateProcess` al contrario (quella
  API Win32 ritorna non-zero in caso di successo, al contrario della convenzione POSIX che il resto
  del crate segue) — un kill riuscito viene riportato come errore. Aggirato in `pty::kill`
  riconoscendo quel caso specifico come successo (commentato nel codice) — **una futura release di
  `portable-pty` andrebbe verificata per capire se il bug è stato corretto a monte, nel qual caso
  l'aggiramento locale può essere rimosso**. Commit `07c584c`, `ui` 2.2.0 → 2.2.1 (CHANGELOG/
  IMPLEMENTATION di `ui`).

**Da fare prima di considerare il piano 3 chiuso al 100%**: solo poche righe residue della Parte 7
di `TESTING-e2e.md` (`/calc`, `/config`/`/library` e i pulsanti della barra, `/aichat` doppio,
avvio senza autostart, resize) — la parte sostanziale (self-heal, OSC 9001, ActivityIndicator, un
turno `/ai` reale, chiusura finestra, autostart di `ui.exe` dall'orchestratore, bottone "riavvia")
è stata eseguita dal vivo dal controller ed è OK, vedi la nota in testa a quella sezione.

- **Fix finestre console spurie + log su file (dopo il piano 3, richiesta diretta di Maurizio da
  uso reale)** — tre richieste: (1) niente più finestre console spurie né all'avvio (plugin
  sidecar) né al primo uso di un tool (`mcp-server`/`mcp-nmap`/python), né la console di debug di
  `ui.exe` (ora incondizionata via `windows_subsystem = "windows"`, prima solo in release); (2) log
  di `ui.exe` su file (`Configuration/logs/ui.log.<data>`, prima assente — solo `println!`/
  `eprintln!`), stesso modulo `startup_config::logging` condiviso con l'orchestrator (spostato lì
  da `orchestrator/src/logging.rs`); (3) via il thread di debug "digita 'q' per uscire", obsoleto
  dal fix di chiusura finestra del piano 3. `CREATE_NO_WINDOW` su 9 spawn lato orchestrator
  (mcp-server ×6 fattorizzati in un metodo, mcp-nmap, python, plugin sidecar, +2 `taskkill`), il
  loro stderr ora su file invece di `inherit()` verso il nulla. Verificato dal vivo (enumerazione
  UIA): una sola finestra top-level "Lare Terminal" prima e dopo l'uso dell'app. Versioni
  startup-config 2.0.3→2.0.4, orchestrator 2.2.0→2.2.1, ui 2.2.1→2.2.2.

## DA FARE

- **Idea (Maurizio, 2026-09-08) — pagina interattiva client-only generata dall'AI per un
  argomento, con refresh via Python.** L'utente chiede una pagina interattiva su un tema;
  istruzioni di base per l'AI: pagina **solo client** (al massimo un DB SQL leggero tipo SQLite
  accanto), mostra i valori richiesti, può usare tutto il parco librerie disponibile; **Python di
  appoggio** per librerie ulteriori e per rileggere i dati a richiesta (pulsante nella pagina) o a
  schedulazione (di sistema o decisa dall'AI, da stabilire), conservandoli da qualche parte (JSON,
  non deciso) così quando l'utente richiede di rivedere la pagina più avanti (anche se nel
  frattempo l'ha chiusa) si riapre nel browser con i dati già noti, non vuota. Distinta dalle
  finestre Markdown/report attuali (quelle sono un colpo solo, questa è pensata per essere
  rivisitata nel tempo). Da brainstormare: come si aggancia all'architettura plugin/canale tool
  esistente, cosa è tecnicamente "la pagina" (documento Library con JS incorporato? finestra
  plugin dedicata?), chi decide la schedulazione, dove vivono i dati persistiti. **Estensione
  (2026-09-08) — un "progetto" come entità di lavoro multi-pagina**: l'utente parte dalla
  descrizione di un PROGETTO, non di una singola pagina — un progetto può comprendere una o più
  pagine web client che insieme formano un'unica "unità di lavoro" (non pagine indipendenti).
  Esempio: un progetto di monitoraggio risorse di rete, con attività in tempo reale E attività
  schedulate nel tempo, su più pagine mostrate su più monitor contemporaneamente (un "muro" di
  dashboard, non una pagina riaperta alla volta). Da decidere anche a livello di progetto (non
  solo di singola pagina): un unico store dati condiviso fra le pagine del progetto; dove/come si
  aprono le pagine di un progetto insieme (posizionamento multi-monitor, ricordato per progetto);
  tempo reale e schedulato come due meccanismi di aggiornamento distinti dichiarati dal progetto.
  **Estensione (2026-09-08, più avanti nella giornata) — descritto da un prompt, consapevole
  degli strumenti, versionato a "release"**: il progetto nasce da un PROMPT; l'AI conosce il
  proprio parco strumenti (pagine web client, SQLite, Python, lo scheduler — "altro?", elenco
  ancora da fare) e deve risolvere il progetto solo con quelli, avvisando esplicitamente
  l'utente di cosa manca se non ci riesce, invece di fare un lavoro parziale in silenzio. Il
  prompt è pensato per crescere nel tempo: in place, oppure come nuova "release" del progetto
  (parola di Maurizio) quando modificare il progetto base è impraticabile/rischioso — a
  richiesta dell'utente o su suggerimento della AI stessa. Meccanismo: l'AI ha il vecchio
  prompt, l'utente aggiunge le nuove caratteristiche desiderate, l'AI combina i due e costruisce
  il nuovo progetto pescando dal vecchio tutto quello che può ancora servire (dati, pagine, job
  schedulati ancora validi), non da zero. **Estensione (2026-09-08, ancora più avanti) — un
  framework UI fra gli strumenti fissi a priori**: fra gli strumenti dell'AI (accanto a web
  client, SQLite, Python, scheduler) anche un framework UI per le pagine, stabilito A PRIORI come
  standard di progetto — non scelto ad hoc di volta in volta. Esempio illustrativo di Maurizio:
  si stabilisce che un framework UI reale esistente (es. "Bookmark") sia LO standard per la UI
  di ogni progetto; le sue caratteristiche (aspetto,
  componenti, convenzioni) vengono definite insieme da utente e AI nel tempo e diventano poi lo
  standard fisso — così ogni progetto condivide la stessa estetica per costruzione, non a caso.
  Da studiare più avanti.
- **Idea (Maurizio, 2026-09-08) — `show_markdown` chiamato più volte in un turno dovrebbe
  aggiornare la STESSA finestra, non aprirne una nuova.** Bug reale trovato dal vivo (piano 3):
  un turno `/ai` lungo con ricerca web può portare il modello a richiamare `show_markdown` più
  volte mentre affina la risposta — oggi (`agent.rs`/`surface.rs`) ogni chiamata apre una
  finestra Markdown NUOVA, indipendente dalle altre (comportamento voluto e testato per un altro
  caso, l'`OpenWindow` di `/help` durante un turno — non va toccato). Riprodotto dal vivo: un
  turno mai completato aveva già aperto 6 finestre quasi identiche. Fix immediato applicato
  (`6e0dc85`): la descrizione del tool ora istruisce esplicitamente il modello a chiamarlo una
  sola volta, solo con la risposta finale pronta — riduce il problema ma non lo elimina (dipende
  dal modello). **Non implementato deliberatamente** (scelta di Maurizio, non un rinvio per
  pigrizia): scartare le chiamate in più butterebbe via un comportamento potenzialmente
  interessante — l'AI che rivede la propria risposta mentre cerca. L'idea da studiare in un
  brainstorming: far sì che una seconda chiamata a `show_markdown` nello STESSO turno aggiorni
  in place il contenuto della finestra già aperta (stesso meccanismo di `OutputWindowContent`/
  `window_id`, oggi usato solo per il testo semplice del turno, non per le finestre aperte
  dall'AI) invece di aprirne una nuova — l'utente vedrebbe la risposta "vivere" mentre l'AI la
  affina, una sola finestra, non N.
- **Idea (Maurizio, 2026-09-07) — più LLM dalla riga di comando: `/ai:<nome>`, gruppi, AI che parlano fra loro.** Oggi `/ai "…"` usa solo il provider `active` di `llms.json` (che però ha già un registro di provider con nome: `claude-direct`, `deepseek-openrouter`, …). Atteso: `/ai:Claude-sonnet "…"`, `/ai:Gemini "…"`, `/ai:Kimi "…"` per rivolgersi a una AI specifica configurata; **gruppi con nome** (es. `Coders` = quelle tre) e `/ai:Coders "…"` che interroga tutte in un colpo solo; poi, la parte più interessante, **le AI che interagiscono fra loro** sul modello di `/aichat` (chat fra due macchine Lare, ciascuna con una AI diversa). In parte esiste, andrà "aggiustato": sintassi nel pre-router della shell (`shell_slash.rs`), gruppi in `llms.json`, fan-out e aggregazione delle risposte nella finestra di output. Da studiare più avanti (dopo il piano 3).
- **Idea (Maurizio, 2026-09-07) — interazione dell'AI "da tastiera" e descrittore di form.** La metodologia usata per l'e2e del piano 2b (tasti via `SendKeys`, screenshot letti come immagine, UI Automation per finestre e schede — `scripts/dev/e2e-driver/`, README con le lezioni) va conservata e fatta diventare un plugin o un metodo interno di interazione dell'AI dentro Lare Terminal. Estensione ancora embrionale: un **modello descrittore di form** (forma da definire) per pagine web, che faccia da "traccia" all'AI: l'utente chiede, l'AI apre la pagina e, seguendo il descrittore, inserisce i valori ricevuti. Da brainstormare quando arriva il suo turno (dopo il piano 3).
- **E2E manuale dal vivo, modalità A (piano 3)**: `TESTING-e2e.md` Parte 7 — la parte sostanziale
  eseguita dal vivo dal controller dopo il piano (self-heal, OSC 9001, ActivityIndicator, un turno
  `/ai` reale, chiusura finestra, autostart di `ui.exe` dall'orchestratore, bottone "riavvia", tutti
  OK — ha anche trovato e fatto correggere due difetti reali: chiusura finestra senza terminare i
  processi, poi bottone "riavvia" invisibile per uno `z-index` mancante). Restano da eseguire solo
  righe minori: `/calc`, `/config`/`/library` e i pulsanti della barra, `/aichat` doppio, avvio
  senza autostart, resize.
- **Streaming token-per-token nella finestra di output** (spec §12, fuori MVP finora): il
  contenuto arriva tutto insieme a `Done`/`Error` da sempre (piano 2a) — il piano 3 lo esclude
  esplicitamente dal proprio scope (Global Constraints), non è quindi legato a un piano
  particolare. Da studiare quando/se emerge come esigenza reale.
- **Chore separata**: `cargo fmt` globale sul codice copiato dalla v1 (non fmt-clean), fuori dai
  piani per non sporcare i diff di review.
- **`ensure_ui_sink` (orchestrator, Task 6) non ha una guardia `IsRunning` come il suo specchio C#
  `Launcher.EnsureUi`** (`shell/lare-shell/src/LareShell/Shell/Launcher.cs`): decide se autostartare
  `ui.exe` solo da "esiste un sink `ui` registrato ORA nel `Registry`", non da "esiste già un
  processo `ui.exe` vivo" — `Launcher.EnsureUi` controlla invece `_starter.IsRunning(UiExe)` prima
  di avviarlo. Innesco concreto: se l'orchestratore stesso riparte, il client WS di `ui.exe`
  (`crates/ui/frontend/ws-client.js`) si riconnette con un backoff esponenziale che arriva fino a
  8s (`RETRY_MAX_MS`); un qualunque turno shell che nel frattempo ha bisogno di una finestra vede
  "nessun sink registrato" e fa partire un SECONDO `ui.exe --no-terminal`. Quella seconda istanza
  non ha via d'uscita oggi: senza finestra terminale non c'è handler di chiusura, e il thread
  debug-only che legge "q" da stdin non esiste in una build release — resta viva finché non viene
  uccisa a mano. Decisione deliberata di questa review: non correggerlo ora, solo documentarlo con
  precisione perché non resti solo nella memoria dei partecipanti.

- **E2E con AI reale fatto il 2026-09-07** (`llms.json` copiato dal deploy v1 in `Test Run\Configuration\`, gitignored): punti 5-9 di `TESTING-e2e.md` Parte 6 tutti OK (tre gate in sequenza, exec nel runspace, `cd` persistente, rifiuto, Ctrl+C in attesa e durante l'exec, `python` interattivo). Trovato e protetto un caso di gate accettato da tasti pendenti (svuotamento del buffer prima del prompt + log): vedi `KNOWN-ISSUES.md`.

### Debiti / decisioni del piano 2a

Deliberati durante l'implementazione (revisione advisor + review del controller), da tenere
presenti nel piano 2b/3:

- `/find` e `/nowin` dalla shell sono **scartati** (come uno slash ignoto) — `/find` vive in
  `ws.rs` fuori dal ciclo di un turno, `/nowin` non ha senso ora che l'output è sempre in
  finestra. Emendamento allo spec (§3, Task 11).
- `/help`/`/show` (`core::WINDOW_SLASHES`) **non** aprono la finestra di output col segnaposto —
  il loro esito È già una finestra, altrimenti se ne aprirebbero due. Emendamento allo spec
  (§3.2, Task 11).
- `web_search` per un turno shell = `Command.web_search` **oppure** `config.json.
  web_search_enabled` — la shell non ha una propria casella, vale la scelta fatta in `/config`.
- `/ping` misura `plugin-ping` con una sonda usa-e-getta Init→Ready (mai l'istanza eager già
  viva) e `ui.exe` con `UiPing`/`UiPong` (timeout 2 s); non è cancellabile a metà (durata
  limitata, ≈ 7 s nel caso peggiore).
- `Hello.version` è stato aggiunto nel codice (Task 1) prima che lo spec §4.1 lo documentasse —
  emendamento allo spec in questo task, non una deviazione.
- **La cwd della sessione non entra nel prompt di sistema dell'AI** (spec §4.6 lo chiede): gap
  preesistente della v1, non introdotto da questo piano — `grep cwd ai_adapter.rs` trova solo
  test, l'adapter non ha mai ricevuto la cwd, quindi né la `ui` né la shell gliela passano oggi.
  Da risolvere in un task dedicato del piano 2b/3, con test propri.
- Il router consegna **un solo messaggio terminale per turno** alla shell (`surface.rs`): un
  `Done` che arrivasse dopo un `Error` già inoltrato viene scartato. Contratto vincolante per la
  host del piano 2b — "il turno finisce al primo terminale".
- `ActivityIndicator` è emesso da questo piano ma non ha ancora un consumatore — arriva nel
  piano 3 (segnalini nella finestra terminale).
- Lo streaming token-per-token nella finestra di output resta fuori MVP (spec §12) — il
  contenuto arriva tutto insieme a `Done`/`Error`.
- `emitToPlugin` (`host.js`) è riusato come emettitore di eventi Tauri generico anche per il
  canale shell (`output:content`) — nome storico, non specifico ai plugin; non rinominato per non
  allargare il diff.
- Test end-to-end del gate di conferma con un'AI **reale** (non lo `StubAdapter`) restano solo
  `--ignored` (richiedono una chiave API) — invariato rispetto alla v1.
- Markdown della finestra di output: i chunk di trasparenza e il testo AI sono concatenati senza
  separatore — cosmetico, vedi `KNOWN-ISSUES.md`.
- **Contratti per la host (piano 2b)**, consolidati dalla review finale del piano 2a (dettaglio
  completo in `crates/protocol/IMPLEMENTATION.md` §"Contratti per la host"): (a) un solo turno AI
  alla volta per connessione — la history è sotto lock per tutto il turno, gate incluso: un
  secondo `/ai` apre la finestra e resta bloccato senza errore, in coda; (b) `Command.id` deve
  essere unico per connessione — un duplicato sovrascrive il cancel token e riusa la finestra di
  output del primo turno con quell'id; (c) il turno finisce al primo `Done`/`Error` — la host
  deve scartare i messaggi dei turni già chiusi, l'orchestratore non ne manda comunque un secondo
  per la via normale; (d) `ExecResult.turn_id` deve corrispondere al `turn_id` ricevuto
  nell'`ExecInShell` a cui si risponde, altrimenti viene scartato con un warn (fix M8) senza
  consumare l'esecuzione pendente.

### Debiti / decisioni del piano 2b

Deliberati durante l'implementazione (revisione advisor + review del controller + e2e dal vivo);
dettaglio completo nel piano (`Docs/i18n/ita/superpowers/plans/2026-09-06-piano-2b-host-lare-shell.md`
§"Ruling presi in questo piano") e in `shell/lare-shell/IMPLEMENTATION.md`.

**Ruling 1-9 del piano** (una riga ciascuno; 4 rivisto durante l'e2e, vedi sotto):

1. Server finto dei test (`FakeOrchestrator`) su `TcpListener` + upgrade WS a mano, mai
   `HttpListener` (registrazioni http.sys che sopravvivono a un test caduto a metà).
2. Riconnessione **on demand**: nessun task in background — ogni `/…` verifica e ristabilisce la
   connessione (con autostart, finestra 5 s) se serve.
3. `exit_code` = 0 se `$?` (catturato in coda allo stesso script) è vero, altrimenti
   `$LASTEXITCODE` se ≠ 0, altrimenti 1; i comandi digitati dall'utente non toccano `$?`/
   `$LASTEXITCODE` (il prompt deve vederli intatti).
4. Processi figli con `UseShellExecute=true`; **entrambi** (`orchestrator.exe` e `ui.exe`) con
   finestra nascosta — **rivisto durante l'e2e**: la formulazione iniziale lasciava `ui.exe`
   `WindowStyle.Normal`, ma in build debug `ui.exe` è un'app console e, con Windows Terminal come
   terminale predefinito, apriva una scheda WT rubando il fuoco alla shell (fix `a8d148f`).
5. `ToolConfirmRequest` senza `turn_id`: attribuita al turno corrente durante l'attesa; i messaggi
   di turni già chiusi scartati a inizio turno.
6. Cap dell'output in **caratteri** (200·1024), non byte.
7. Log della host `<config-dir>\logs\lare-shell.log` senza rotazione (debito: la spec chiederebbe
   rotazione giornaliera).
8. `ui.exe` avviata dopo la prima connessione riuscita e **ricontrollata prima di ogni turno
   slash** (self-heal: se l'utente l'ha chiusa, il prossimo `/…` la riapre).
9. `deploy_test_run.ps1` pubblica la host di default (framework-dependent win-x64); `-SkipShell`
   per saltarla.

**Difetti del piano trovati ed emendati durante l'esecuzione** (dettaglio nei `task-N-report.md`):

- Catch troppo stretti in `StartupConfig.Load`/`TokenFile.Read`: `UnauthorizedAccessException`
  sfuggiva invece di degradare a default/`null` (T0).
- `pong.ts` doveva essere obbligatorio; i campi numerici malformati di `done`/`pong` sfuggivano
  come eccezioni .NET grezze invece di un `WireException` uniforme (T1).
- Socket non rilasciato su `OperationCanceledException` dentro `ConnectAsync`; `Connect` su una
  connessione già viva accodava un `Disconnected` spurio invece di essere un no-op (T2).
- `<NoWarn>CA1416</NoWarn>` (fuori lista del piano) sostituito con `SupportedOSPlatform=windows`
  via un item `AssemblyAttribute` nel `.csproj` — una property `<SupportedOSPlatform>` piatta non
  genera l'attributo che serve (T3).
- **Senza `<RuntimeIdentifier>` gli asset RID-specifici del SDK (`System.Management.Automation.dll`
  compreso) finiscono sotto `runtimes\win\lib\net10.0\` → `$PSHOME` risolve lì invece che nella
  cartella dell'exe → `powershell.config.json` accanto all'exe non viene letto** (7 test rossi su
  11 finché non scoperto): aggiunto `<RuntimeIdentifier>win-x64</RuntimeIdentifier>` +
  `<SelfContained>false</SelfContained>` in ENTRAMBI i `.csproj` — coerente col publish del deploy,
  output di build sotto `bin\Debug\net10.0\win-x64\` (T4).
- `PowerShell.Stop()` su Microsoft.PowerShell.SDK 7.6.5 NON lancia `PipelineStoppedException`:
  `Invoke()` torna normalmente con `InvocationStateInfo.State == Stopped` (T4; il catch
  dell'eccezione resta comunque, per altri SDK/percorsi).
- Exit code catturato con `try { <cmd> } finally { $global:__lare_ok = $? }`, non un append
  diretto (un `return` di primo livello lo salterebbe) né un blocco `. { }`/`& { }` (azzera sempre
  `$?` al proprio confine, qualunque cosa sia successa dentro) — verificato empiricamente in due
  iterazioni di fix (T4).
- Test della guardia `Done.Id` rafforzato (un `Done` di un altro turno non deve chiudere quello
  corrente); `ConsoleGate` con stdin rediretto ora fallisce **chiuso** su EOF (rifiuta, non
  accetta); eccezione dell'executor durante un turno → `CancelCommand` invece di un turno appeso
  da entrambe le parti (T5).
- Handler di Ctrl+C mai lancia: niente `using` sul `CancellationTokenSource` del turno — evita la
  race con la fine naturale del turno (`ObjectDisposedException` nel thread dell'handler avrebbe
  altrimenti terminato il processo, violando "Ctrl+C non esce mai dalla shell") (T7).
- `ui.exe` avviata con console nascosta anche lei, non solo `orchestrator.exe` — scoperto
  dal vivo durante l'e2e (T8/e2e, vedi ruling 4 sopra).

**Limiti noti** (dettaglio in `KNOWN-ISSUES.md` §"Debiti nativi della host `lare-shell`" e
`shell/lare-shell/IMPLEMENTATION.md` §Debiti): profili AllUsers non caricati; log della host senza
rotazione; `$?` nel prompt resta quello dell'ultimo comando digitato dall'utente, mai un riflesso
di un turno slash appena concluso; `exit` dentro un comando dell'AI termina il processo come un
`exit` digitato (il gate ha già mostrato il comando); `StopCurrent()` ha una finestra TOCTOU (un
Ctrl+C fra l'assegnazione di `_current` e `Invoke()` è un no-op silenzioso, nessun crash);
`TerminalPending` guarda solo la testa della coda dei messaggi in arrivo (`TryPeek`) — per un
turno gateizzato questo basta: l'ack `Chunk` che precede il `Done` conta come terminale anch'esso
(fix F3, revisione finale), perché `route_shell_turn` non manda mai altro testo alla shell nel
mezzo; `ToolConfirmRequest`
senza `turn_id`; il gate con stdin rediretto blocca dentro `Console.ReadLine`; **`ConsoleGate` e
`Repl` non hanno test automatici** (richiedono una console interattiva vera — coperti solo
dall'e2e manuale, `TESTING-e2e.md` Parte 6); Ctrl+C su un comando digitato è verificato dal vivo in
Windows Terminal (log `Ctrl+C ricevuto` + riga `"[LARE] comando interrotto (Ctrl+C)."`): il caso
"riga mancante" delle prime passate era il driver dell'e2e in `conhost`, non la host; `#pragma warning
disable xUnit1031` nei test sincroni di `SlashTurn` (bloccano di proposito, come il thread REPL
vero); il fragment del profilo Windows Terminal (`install-wt-profile.ps1`) è letto solo all'avvio
di WT — installarlo/aggiornarlo richiede di riavviarlo, non basta una nuova scheda; `ui.exe` in
build debug è un'app console (spiega perché va nascosta anche lei, non un bug di Lare).

### Debiti noti del piano 1

Minori, rimandati deliberatamente (non bloccano il piano 1, da valutare/chiudere nei piani
successivi):

- 12 `Cargo.toml` riscritti CRLF→LF e voci CHANGELOG con endings misti.
- `RuntimeConfig::{mcp_server_exe,mcp_nmap_exe,pytools_dir}` non usati dai resolver (formula
  duplicata inline).
- Test `generate_token_is_random` rimosso senza sostituto.
- `scripts/pytools/README.md` riga ~5 "convenzione invariata" (falso) e riga ~27 etichetta
  interna "Task 4/5".
- 3 costruzioni `LareWsClient` senza guardia su `url` vuota (`config-dialog.js`,
  `external-channel-window.js`, `window.js`) — `host.js` ha la guardia.
- `capabilities/default.json` concede `core:window:allow-set-size`/`allow-start-dragging`
  inutilizzati.
- `diagnose_connection` registrato senza chiamante JS.
- `library.js` invoca `open_saved_find_window` direttamente (pre-esistente).
- `Test Run/Configuration/README.md` dice "a ogni avvio" per network/search json (solo se
  assenti/corrotti).
- Commenti nel frontend copiato dalla v1 citano percorsi `Docs/superpowers/...` che nel 2.0
  non esistono (la documentazione è sotto `Docs/i18n/`).
- `ui.exe --open` accetta solo la forma con spazio (`--open config`), non `--open=config`.
- `ui.log` su file non implementato (`ui.exe` logga solo su stdout).

**Nota per il piano 2 — risolta nel piano 2a**: `host.js` `openAiChatWindow` non aveva chiamanti
finché `/aichat` non arrivava via l'orchestratore. `shell_slash::UI_LOCAL_SLASHES` include
`aichat` fin dal Task 6: `OpenUiLocal{name:"aichat"}` lo raggiunge già.
