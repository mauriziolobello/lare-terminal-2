# Implementation — startup-config

## Crate iniziale (v0.1.0)

Crate puro, nessuna dipendenza da tokio/tauri/rmcp — stessa categoria di
`protocol`. Vive in `crates/startup-config/`, aggiunto a `members` E
`default-members` del workspace root (serve a `orchestrator` e
`mcp-server`, entrambi già in `default-members`).

`exe_dir()` e `default_local_dir()` sono wrapper sottili sul confine col
sistema operativo (`current_exe()`/`std::env::var("LOCALAPPDATA")`) —
intenzionalmente NON testati con unit test diretti, stesso pattern già in
uso altrove nel progetto (es. `McpToolClient::resolve()`): la logica pura
sotto (`resolve()`, `load_from_dir()`) è invece testata a fondo via
injection, senza mai mutare l'ambiente reale del processo di test.

`resolve()` valuta la closure `default` pigramente — costruita SOLO se né
la env var né il campo del file la rendono superflua (test
`resolve_default_closure_not_evaluated_when_env_wins`, che fa panic se la
closure viene comunque chiamata).

Design completo, inventario dei 15 siti censiti, e le 3 asimmetrie di
comportamento trovate e decise (LARE_PLUGINS_DIR senza trim, `llms_config`
e `telegram::settings` senza trim sul proprio override — normalizzate a
trim-sempre come bug-fix minore) in
`Docs/superpowers/specs/2026-08-12-startup-config-design.md`.
