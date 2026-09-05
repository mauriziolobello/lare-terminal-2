# Changelog — plugin-counter

All notable changes to this crate will be documented here.
Format: [Keep a Changelog](https://keepachangelog.com/en/1.0.0/). Semver from `0.1.0`.

## 2.0.0 — 2026-09-05 — fork da v1 0.1.0

Copia del crate dalla v1 (`mauriziolobello/lare-terminal`) nel repo 2.0. Nessuna modifica
funzionale in questa voce; le modifiche del piano 1 seguono nelle voci successive.

## [Unreleased]

## [0.1.0] — 2026-06-27

### Added
- Initial crate: `plugin-counter` binary `counter` (Slice 1 prover plugin).
- `struct Counter { count: u64 }` — persisted state across messages.
- `fn render_html(n: u64) -> String` — generates full-HTML body with `lare-window`/`lare-label`/`lare-button` classes and `data-evt="inc"` button.
- `fn handle(state: &mut Counter, msg: HostToPlugin) -> Vec<PluginToHost>` — pure seam: Init→Ready; Activate→ShowWindow(count=0); UiEvent{inc}→count+1→UpdateWindow; unknown UiEvent→no-op; Deinit→exit.
- `main`: stdin line loop (modeled on `plugin-ping`), exits cleanly on Deinit.
- `plugin.json`: manifest with `id="counter"`, `triggers.command="/counter"`, `protocol_version=1`.
- 2 TDD tests (`init_then_activate_shows_count_zero`, `ui_event_inc_updates_count`), RED verified before implementation.
