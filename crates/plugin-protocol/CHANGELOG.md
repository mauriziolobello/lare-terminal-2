# Changelog — plugin-protocol

All notable changes to this crate will be documented in this file.
Format: [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), semver `major.minor.patch`.

## 2.0.0 — 2026-09-05 — fork da v1 0.2.1

Copia del crate dalla v1 (`mauriziolobello/lare-terminal`) nel repo 2.0. Nessuna modifica
funzionale in questa voce; le modifiche del piano 1 seguono nelle voci successive.

## [0.2.1] — 2026-07-12 — `WindowSize` opzionale nel manifest (additivo)

### Added (Contract P — additivo)
- `WindowSize { width: f64, height: f64 }` — value object `Copy` per la dimensione iniziale di una finestra plugin.
- `PluginManifest.window: Option<WindowSize>` — dimensione iniziale preferita, dichiarata UNA VOLTA nel `plugin.json` statico. `#[serde(default)]`: assente nel JSON ⇒ `None` ⇒ l'host usa il default generico (480×360). Retro-compatibile: i manifest esistenti (calc/ping/counter) continuano a parsare invariati.
- 2 nuovi test TDD: `manifest_without_window_defaults_none` (assenza ⇒ `None`), `manifest_with_window_parses_size` (presenza ⇒ `Some(WindowSize{..})`).

### Unchanged
- Tutte le varianti di `HostToPlugin`/`PluginToHost`, `Triggers`, `parse_manifest`.

## [0.2.0] — 2026-06-27

### Added (Contract P — Slice 1, additivo)
- `HostToPlugin::Activate { window_id: u64, args: serde_json::Value }` — ordina al plugin di aprire/mostrare una finestra con id dato e argomenti opzionali.
- `HostToPlugin::UiEvent { window_id: u64, element_id: String, value: Option<String> }` — notifica un evento UI generato dall'utente (click/input) identificato da `data-evt`.
- `PluginToHost::ShowWindow { window_id: u64, title: String, html: String }` — il plugin chiede all'host di aprire una nuova finestra con il contenuto HTML (full-HTML).
- `PluginToHost::UpdateWindow { window_id: u64, html: String }` — rimpiazza l'intero HTML di una finestra già aperta (full-HTML replace, nessuna patch element-level).
- `PluginToHost::CloseWindow { window_id: u64 }` — il plugin chiede all'host di chiudere la finestra.
- 3 nuovi unit test TDD: `activate_roundtrips`, `ui_event_roundtrips`, `show_and_update_window_roundtrip`.

### Unchanged (Fase 0 invariante)
- `Init`, `Deinit`, `Ready`, `Log`, `PluginManifest`, `Triggers`, `parse_manifest`.

## [0.1.0] — 2026-06-26

### Added
- `Triggers` struct (`command: Option<String>`, `interval: Option<String>`, `Default`).
- `PluginManifest` struct (`name`, `id`, `version`, `protocol_version: u32`, `triggers: Triggers`).
- `parse_manifest(json: &str) -> serde_json::Result<PluginManifest>`.
- `HostToPlugin` enum (`Init { protocol_version, config, storage_dir }`, `Deinit {}`), tagged `{"type":"..."}`.
- `PluginToHost` enum (`Ready { name, protocol_version }`, `Log { level, msg }`), tagged `{"type":"..."}`.
- 4 unit tests (TDD): `manifest_parses_triggers`, `manifest_without_triggers_defaults_empty`,
  `host_to_plugin_init_roundtrips`, `plugin_to_host_ready_roundtrips`.
