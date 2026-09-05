# IMPLEMENTATION — plugin-counter

**Version:** 0.1.0  
**Binary:** `counter` (discovered as `plugins/counter/counter.exe` on Windows)  
**Role:** Slice 1 prover plugin — demonstrates the full activate→showWindow→uiEvent→updateWindow round-trip.

## Files

| File | Purpose |
|------|---------|
| `Cargo.toml` | `[[bin]] name="counter"`, deps: `plugin-protocol` + `serde_json` |
| `src/main.rs` | All logic: `Counter` struct, `render_html`, `handle`, `main` loop |
| `plugin.json` | Manifest: `id="counter"`, `triggers.command="/counter"`, `protocol_version=1` |

## Architecture

### State

`struct Counter { count: u64 }` — owned by `main`, passed as `&mut Counter` to `handle`. Single-process, no concurrency needed (stdio plugins are single-task).

### Pure seam: `handle`

```
Init        → Ready { name: "counter", protocol_version: 1 }
Activate    → ShowWindow { window_id, title: "Counter", html: render_html(0) }
UiEvent/inc → count += 1; UpdateWindow { window_id, html: render_html(count) }
UiEvent/??  → []   (ignore; explicit arm preserves exhaustiveness check)
Deinit      → []   (main loop breaks after)
```

### HTML template (`render_html`)

```html
<div class="lare-window">
  <span class="lare-label">Count: {n}</span>
  <button class="lare-button" data-evt="inc">+1</button>
</div>
```

Classes (`lare-window`, `lare-label`, `lare-button`) are resolved by the host's `plugin-catalog.css` injected at render time. `data-evt="inc"` is the event identifier: the UI runtime translates clicks on `[data-evt]` elements into `UiEvent { element_id: "inc" }`.

### Main loop

Identical to `plugin-ping`: blocking `BufRead::lines()` on stdin, one JSON message per line, flush on every reply, clean exit on Deinit.

## Tests (TDD)

| Test | Covers |
|------|--------|
| `init_then_activate_shows_count_zero` | Init→Ready chain; Activate→ShowWindow with "Count: 0" and `data-evt="inc"` |
| `ui_event_inc_updates_count` | UiEvent{inc}→UpdateWindow with "Count: 1"; state.count == 1 |

Both tests were RED before implementation (behavioral failure, not compile error).

## Manifest

```json
{"name":"Counter","id":"counter","version":"1.0.0","protocol_version":1,"triggers":{"command":"/counter"}}
```

Note: `"version"` in the manifest is the plugin's own version (`1.0.0`), separate from the crate semver (`0.1.0`).
