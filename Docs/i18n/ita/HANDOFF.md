# HANDOFF — Lare Terminal 2.0

> Checkpoint per ripartire a contesto azzerato. Si aggiorna nello stesso commit di ogni release
> (hook `commit-msg`). Stato dettagliato per area: `crates/<crate>/IMPLEMENTATION.md`.

## Versioni correnti

(a fine piano 2a "protocollo shell" — lette da ogni `Cargo.toml`)

- protocol 2.1.0 (da v1 0.15.4)
- startup-config 2.0.2 (da v1 0.1.0)
- mcp-server 2.0.1 (da v1 0.7.1)
- mcp-nmap 2.0.0 (da v1 0.8.2)
- orchestrator 2.1.0 (da v1 0.41.21)
- plugin-protocol 2.0.0 (da v1 0.2.1)
- plugin-ping 2.0.0 (da v1 0.1.0)
- plugin-counter 2.0.0 (da v1 0.1.0)
- plugin-calc 2.0.0 (da v1 0.2.0)
- plugin-lc 2.0.0 (da v1 0.4.5)
- plugin-crypto 2.0.0 (da v1 1.0.1)
- ui 2.1.0 (da v1 0.47.1)

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

## DA FARE

- **Piano 2b** — host C# `lare-shell` (ADR-015), verificabile in **modalità B** (profilo Windows
  Terminal, `lare-shell.exe` nudo — spec §2.3). Consuma il protocollo 2.1 di questo piano: il
  canale shell (`Hello{role:"shell",…}`, gate `[Y/n]`, `ExecInShell`/`ExecResult`, output in
  finestra) è già pronto lato orchestratore/`ui` — resta da scrivere la host che lo parla per
  davvero. Da scrivere con la skill `writing-plans`, partendo da spec §4.3-§4.5 (gate, vincoli di
  esecuzione, `capture`), §6.4 (avvio e self-heal reciproco orchestratore↔host), §7 (`Test Run\`)
  e dal client di sviluppo `scripts/dev/shell-client.mjs` come riferimento del giro di messaggi
  (imita esattamente ciò che la host C# dovrà fare).
- **Piano 3** — finestra terminale Tauri (xterm.js + ConPTY, ADR-016), **modalità A** (`ui.exe`
  lancia `lare-shell.exe` dentro una ConPTY e diventa l'app che l'utente avvia — spec §2.3/§5).
  Include: autostart reciproco completo (`ui.exe` avvia l'orchestratore se assente, e viceversa,
  §6.4), rimozione del flag di sviluppo `--open` (sostituito dal canale shell reale), consumo di
  `ActivityIndicator` (emesso dal piano 2a, mai ancora letto da un consumatore), streaming
  token-per-token nella finestra di output (§12, fuori MVP finora).
- **Chore separata**: `cargo fmt` globale sul codice copiato dalla v1 (non fmt-clean), fuori dai
  piani per non sporcare i diff di review.

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
