# Implementation — plugin-ping v0.1.0

Plugin minimale di Fase 0: valida la catena host↔plugin
(`discovery → spawn → Init → Ready → Deinit`).

## Scopo

`plugin-ping` è il plugin di riferimento di Fase 0. Non fa nulla di utile per l'utente
finale — il suo unico ruolo è verificare che l'intera pipeline funzioni:
1. `discover()` lo trova nella cartella `plugins/ping/` (manifest + binario).
2. `PluginHost::start` lo spawna, invia `Init`, riceve `Ready`.
3. `PluginHost::shutdown` invia `Deinit`; il processo esce pulito.

## Struttura del crate

```
crates/plugin-ping/
├── src/main.rs    — entry point: loop stdin + fn handle (pura)
├── plugin.json    — manifest (usato dall'e2e test in orchestrator/tests/plugin_e2e.rs)
├── Cargo.toml     — [[bin]] name = "ping"; dipendenza plugin-protocol
├── CHANGELOG.md
└── IMPLEMENTATION.md  (questo file)
```

## `fn handle` — il seam testabile

```rust
fn handle(msg: HostToPlugin) -> Vec<PluginToHost>
```

Logica pura (zero I/O): dato un messaggio host, restituisce le risposte.
Testabile senza avviare processi. I test unitari coprono Init e Deinit.

## Loop `main`

Loop bloccante su `stdin.lock().lines()` (no tokio — plugin single-task).
Per ogni riga JSON: deserializza in `HostToPlugin`, chiama `handle`, scrive le
risposte su stdout (una riga JSON per risposta), poi se il messaggio era `Deinit`
esce dal loop → il processo termina con exit code 0.

## Manifest (`plugin.json`)

```json
{ "name": "Ping", "id": "ping", "version": "1.0.0", "protocol_version": 1, "triggers": {} }
```

`triggers: {}` → nessun trigger → `SpawnPolicy::Eager` → il plugin viene
eager-spawnato da `PluginHost::start` all'avvio dell'orchestratore.
