# Changelog — plugin-ping

All notable changes to this crate are documented here.
Format: [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), versioning: [SemVer](https://semver.org/).

---

## 2.0.0 — 2026-09-05 — fork da v1 0.1.0

Copia del crate dalla v1 (`mauriziolobello/lare-terminal`) nel repo 2.0. Nessuna modifica
funzionale in questa voce; le modifiche del piano 1 seguono nelle voci successive.

## [0.1.0] — 2026-06-26 — primo plugin di Fase 0

### Added

- **Binario `ping`** (nome via `[[bin]]` in `Cargo.toml`): il plugin minimale per validare
  la catena host↔plugin end-to-end.
- **`handle(msg: HostToPlugin) -> Vec<PluginToHost>`** — logica pura, testabile senza I/O:
  - `Init { .. }` → `[Ready { name: "ping", protocol_version: 1 }]`
  - `Deinit {}` → `[]` (il main loop esce dopo)
- **`main()`**: loop bloccante stdin JSON-per-riga (no tokio — il plugin è single-task);
  al `Deinit` rompe il ciclo e termina pulito.
- **`plugin.json`** manifest nella cartella crate-root (usato dall'e2e in `orchestrator`):
  `{ "name": "Ping", "id": "ping", "version": "1.0.0", "protocol_version": 1, "triggers": {} }`
- **2 unit test TDD**: `init_yields_ready`, `deinit_yields_nothing`.
