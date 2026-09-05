# HANDOFF — Lare Terminal 2.0

> Checkpoint per ripartire a contesto azzerato. Si aggiorna nello stesso commit di ogni release
> (hook `commit-msg`). Stato dettagliato per area: `crates/<crate>/IMPLEMENTATION.md`.

## Versioni correnti

(a fine piano 1 "fondamenta" — lette da ogni `Cargo.toml`)

- protocol 2.0.0 (da v1 0.15.4)
- startup-config 2.0.1 (da v1 0.1.0)
- mcp-server 2.0.1 (da v1 0.7.1)
- mcp-nmap 2.0.0 (da v1 0.8.2)
- orchestrator 2.0.1 (da v1 0.41.21)
- plugin-protocol 2.0.0 (da v1 0.2.1)
- plugin-ping 2.0.0 (da v1 0.1.0)
- plugin-counter 2.0.0 (da v1 0.1.0)
- plugin-calc 2.0.0 (da v1 0.2.0)
- plugin-lc 2.0.0 (da v1 0.4.5)
- plugin-crypto 2.0.0 (da v1 1.0.1)
- ui 2.0.2 (da v1 0.47.1)

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

## DA FARE

- **Piano 2** — protocollo host↔orchestratore (spec §4) e `lare-shell` (host C# del motore
  PowerShell, ADR-015), verificabile in **modalità B** (profilo Windows Terminal, `lare-shell.exe`
  nudo — spec §2.3). Include: messaggi `Command`/gate di conferma sul canale shell, avvio
  reciproco orchestratore↔host quando l'uno non trova l'altro (§6.4, primo pezzo).
- **Piano 3** — finestra terminale Tauri (xterm.js + ConPTY, ADR-016), **modalità A** (`ui.exe`
  lancia `lare-shell.exe` dentro una ConPTY e diventa l'app che l'utente avvia — spec §2.3/§5).
  Include: autostart reciproco completo (`ui.exe` avvia l'orchestratore se assente, e viceversa,
  §6.4), rimozione del flag di sviluppo `--open` (sostituito dal canale shell reale).

### Debiti noti del piano 1

Minori, rimandati deliberatamente (non bloccano il piano 1, da valutare/chiudere nei piani
successivi):

- 12 `Cargo.toml` riscritti CRLF→LF e voci CHANGELOG con endings misti.
- `startup-config::resolve_path` con valore rooted senza drive (`/x`) su Windows: `Path::join`
  scarta la base — da documentare/testare.
- `RuntimeConfig::{mcp_server_exe,mcp_nmap_exe,pytools_dir}` non usati dai resolver (formula
  duplicata inline).
- Test `generate_token_is_random` rimosso senza sostituto.
- Commenti in `orchestrator/src/lib.rs`/`ws.rs` citano ancora la porta fissa 7331.
- `crates/orchestrator/tests/ws_integration.rs` ~riga 1150 fa ancora
  `set_var("LARE_PYTOOLS_DIR")` (inerte).
- `scripts/pytools/README.md` riga ~5 "convenzione invariata" (falso) e riga ~27 etichetta
  interna "Task 4/5".
- 3 costruzioni `LareWsClient` senza guardia su `url` vuota.
- `capabilities/default.json` concede `core:window:allow-set-size`/`allow-start-dragging`
  inutilizzati.
- `diagnose_connection` registrato senza chiamante JS.
- Titolo `crates/ui/IMPLEMENTATION.md` ancora "v0.47.1".
- `library.js` invoca `open_saved_find_window` direttamente (pre-esistente).
- `Test Run/Configuration/README.md` dice "a ogni avvio" per network/search json (solo se
  assenti/corrotti).
- `deploy_test_run.ps1` lascia `$LASTEXITCODE` di robocopy a fine script.

**Nota per il piano 2**: `host.js` `openAiChatWindow` non ha chiamanti finché `/aichat` non
arriva via l'orchestratore (`OpenUiLocal` dovrà includere `aichat`).
