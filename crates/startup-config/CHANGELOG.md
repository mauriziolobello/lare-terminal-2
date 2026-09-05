# Changelog — startup-config

All notable changes to this crate are documented here.
Format: [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), versioning: [SemVer](https://semver.org/).

---

## 2.0.1 — 2026-09-05 — `--config-dir`, nuovo schema `startup.json`, via env var

Riscrittura completa dell'API (piano 1 "fondamenta", Task 2). Applica la
decisione D6 dello spec 2.0: nessun binario Lare legge più `LARE_*` — nella
v1 tre lettori indipendenti (orchestrator, ui, Python) della stessa env var
divergevano in silenzio.

Nuova regola: la cartella di configurazione è `--config-dir <path>` sulla
riga di comando, altrimenti `<cartella dell'eseguibile>\Configuration\`. I
percorsi relativi in `startup.json` si risolvono contro la RADICE DEL
DEPLOY (cartella padre di `Configuration\`), non contro la cwd né contro la
cartella dell'eseguibile.

API pubblica: `CONFIG_DIR_FLAG`, `DEFAULT_CONFIG_DIR_NAME`,
`STARTUP_FILE_NAME`, `parse_config_dir()`, `has_flag()`,
`resolve_config_dir()`, `config_dir_from_process()`, `exe_dir()` (invariata
dalla v1), `deploy_root()`, `StartupConfig` (con `Paths`, `Autostart`,
`LogConfig`) con `StartupConfig::load()` e `StartupConfig::resolve_path()`.

Rimossi: `resolve(env, file, default)`, `default_local_dir()`,
`load_from_dir()`, i campi `local_dir`/`roaming_dir`/`telegram_settings`/
`llms_config`. Nessuna lettura di variabili d'ambiente nel crate (prima:
`LOCALAPPDATA` in `default_local_dir()`).

## 2.0.0 — 2026-09-05 — fork da v1 0.1.0

Copia del crate dalla v1 (`mauriziolobello/lare-terminal`) nel repo 2.0. Nessuna modifica
funzionale in questa voce; le modifiche del piano 1 seguono nelle voci successive.

## [0.1.0] — 2026-08-12 — crate iniziale

Nuovo crate infrastrutturale: risolve la configurazione (`local_dir`,
`llms_config`, `telegram_settings`, `routines_dir`, `plugins_dir`,
`roaming_dir`) con precedenza a 3 livelli — env var > campo di
`startup.json` (accanto all'eseguibile) > default. Sostituisce, in
`mcp-server` e `orchestrator` (fase 1 — vedi
`Docs/superpowers/specs/2026-08-12-startup-config-design.md`), la catena
`LARE_LOCAL_DIR` → `%LOCALAPPDATA%\dev.lare.terminal\` → `.lare-data\`
oggi duplicata in ~7 punti. `ui` resta fuori scope (fase 2, Tauri managed
state).

API: `StartupConfig`, `exe_dir()`, `load_from_dir()`, `resolve()`,
`default_local_dir()`.
