# Changelog — startup-config

All notable changes to this crate are documented here.
Format: [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), versioning: [SemVer](https://semver.org/).

---

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
