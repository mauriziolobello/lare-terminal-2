# IMPLEMENTATION — plugin-protocol v0.2.1

## Scopo

Crate di soli tipi condivisi ("Contract P") tra l'host (orchestrator) e i plugin sidecar.
Niente logica, niente I/O: solo `serde` + `serde_json`. Dipendenza leggera, importabile sia
dall'orchestrator sia dai binari plugin.

## Contenuto corrente

### `Triggers`
Trigger dichiarati nel manifest. Usati dalla spawn policy per decidere eager vs. lazy.
- `command: Option<String>` — slash command che attiva il plugin (es. `"/calc"`).
- `interval: Option<String>` — cadenza timer (es. `"5m"`) per `OnTimer` (Fase 2).
- Derive: `Default` (campi None), `PartialEq`, `Serialize`, `Deserialize`, `Debug`, `Clone`, `Eq`.
- `#[serde(default)]` su ogni campo: campi assenti nel JSON diventano `None`.

### `WindowSize` (v0.2.1)
Value object per la dimensione iniziale di una finestra plugin.
- `width: f64`, `height: f64`.
- Derive: `Debug, Clone, Copy, PartialEq, Serialize, Deserialize`. È `Copy` (due `f64` senza invarianti) così l'host può duplicarlo senza `.clone()` (es. catturarlo in un `async move`).

### `PluginManifest`
Manifest statico letto da `plugins/<id>/plugin.json` dalla discovery (orchestrator).
- `triggers` ha `#[serde(default)]`: manifest senza `"triggers"` ottiene `Triggers::default()`.
- `window: Option<WindowSize>` (v0.2.1), `#[serde(default)]`: manifest senza `"window"` ⇒ `None` ⇒ l'host usa il default generico (480×360). Dichiarata UNA VOLTA per tipo di plugin (statica), a differenza di `ShowWindow` che è dinamico.
- Campi extra nel JSON sono silenziosamente ignorati da serde (extensibility forward).

### `parse_manifest`
Thin wrapper su `serde_json::from_str`. Ritorna `serde_json::Result<PluginManifest>`.

### `HostToPlugin` (Fase 0 + Slice 1)
Messaggi dall'host verso il plugin, serializzati come `{"type":"NomeVariante",...}` (tagged enum).
Il tag `"type"` permette la deserializzazione polimorfica senza overhead.

| Variante | Fase | Descrizione |
|----------|------|-------------|
| `Init { protocol_version, config, storage_dir }` | 0 | Primo messaggio dopo spawn. |
| `Deinit {}` | 0 | Shutdown pulito. |
| `Activate { window_id: u64, args: Value }` | 1 | Apri/mostra finestra. `args` = parametri opzionali dello slash comando (può essere `null`). |
| `UiEvent { window_id, element_id, value }` | 1 | Evento UI: `element_id` = valore di `data-evt`; `value` = `Some(str)` per input/select, `None` per pulsanti. |

### `PluginToHost` (Fase 0 + Slice 1)
Messaggi dal plugin verso l'host.

| Variante | Fase | Descrizione |
|----------|------|-------------|
| `Ready { name, protocol_version }` | 0 | Primo messaggio obbligatorio: plugin operativo. |
| `Log { level, msg }` | 0 | Log generico (non consumato in Fase 0). |
| `ShowWindow { window_id, title, html }` | 1 | Apri nuova finestra con HTML completo (full-HTML). |
| `UpdateWindow { window_id, html }` | 1 | Rimpiazza l'intero HTML della finestra (full-HTML replace). |
| `CloseWindow { window_id }` | 1 | Chiudi la finestra. |

## Strategia di serializzazione

Entrambi gli enum usano `#[serde(tag = "type")]`: il campo JSON `"type"` contiene il nome
della variante Rust (es. `"Activate"`, `"ShowWindow"`). Questo è equivalente a un pattern
discriminated union in TypeScript o a un oggetto polimorfico con campo tipo in OOP.

## Vincoli di progetto

- **Additivo**: Fase 0 (Init/Deinit/Ready/Log) è invariante. I nuovi variant di Slice 1
  si aggiungono in coda — non rompono i client esistenti che non li gestiscono.
- **Full-HTML**: `UpdateWindow` porta l'intero HTML (nessuna patch element-level).
  Più semplice e prevedibile per i plugin.
- `protocol_version: u32` è nel manifest e nei messaggi per la negoziazione futura.

## Test (TDD)

Tutti in `src/lib.rs` (unit tests — 7 totali):

| Test | Comportamento verificato |
|------|--------------------------|
| `manifest_parses_triggers` | parse + accesso a `id`, `protocol_version`, `triggers.command` |
| `manifest_without_triggers_defaults_empty` | `triggers` assente nel JSON → `Triggers::default()` |
| `host_to_plugin_init_roundtrips` | roundtrip di `Init`; `"type":"Init"` presente |
| `plugin_to_host_ready_roundtrips` | roundtrip di `Ready` |
| `activate_roundtrips` | roundtrip di `Activate`; `"type":"Activate"` presente nel JSON |
| `ui_event_roundtrips` | roundtrip di `UiEvent` con `value: None` |
| `show_and_update_window_roundtrip` | roundtrip di `ShowWindow`, `UpdateWindow`, `CloseWindow` |
