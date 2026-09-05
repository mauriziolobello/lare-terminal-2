# Implementation — startup-config

## API 2.0 (v2.0.1 — riscrittura Task 2, piano 1 "fondamenta")

Crate puro, nessuna dipendenza da tokio/tauri/rmcp — stessa categoria di
`protocol`. Vive in `crates/startup-config/`, aggiunto a `members` E
`default-members` del workspace root (serve a `orchestrator` e
`mcp-server`, entrambi già in `default-members`).

### Regola della cartella di configurazione (spec D6/§6)

Ogni binario Lare (orchestrator, mcp-server, ui, plugin, server Python)
segue la STESSA regola, senza eccezioni:

1. `--config-dir <path>` (o `--config-dir=<path>`) sulla riga di comando,
   se presente → vince sempre.
2. Altrimenti: `<cartella dell'eseguibile>\Configuration\`.

Nessuna variabile d'ambiente viene letta da questo crate (decisione D6
dello spec 2.0). Nella v1 tre lettori indipendenti (orchestrator, ui,
Python) della stessa `LARE_*` env var divergevano in silenzio — motivo per
cui la v2 sposta tutto su un flag esplicito, uguale ovunque.

Le funzioni sono divise per essere testabili senza mai toccare l'ambiente
reale del processo di test:

- `parse_config_dir(args)` — pura, prende `argv` come parametro (non legge
  `std::env::args()` da sola).
- `resolve_config_dir(flag, exe_dir)` — pura, prende `exe_dir` come
  parametro (non chiama `exe_dir()` da sola): il test non dipende da dove
  si trova il binario di test compilato.
- `config_dir_from_process()` — l'UNICA funzione che tocca il processo
  reale (`std::env::args()` + `exe_dir()`): comodità per i vari `main`,
  intenzionalmente non testata con unit test diretti, stesso pattern già
  in uso per `exe_dir()` (wrapper sottile sul confine col sistema
  operativo — vedi nota più sotto).
- `exe_dir()` — invariata dalla v1: `current_exe()` riflette sempre il
  path del binario realmente lanciato, mai la cwd (che PUÒ differire —
  es. `System32` per un servizio Windows senza working directory
  esplicita).
- `has_flag(args, flag)` — helper generico per flag booleani come
  `--console-log`, usato dai binari indipendentemente da `--config-dir`.

`exe_dir()` e `config_dir_from_process()` sono wrapper sottili sul confine
col sistema operativo — intenzionalmente NON testati con unit test
diretti, stesso pattern già in uso altrove nel progetto (es.
`McpToolClient::resolve()`): la logica pura sotto (`parse_config_dir()`,
`resolve_config_dir()`, `deploy_root()`, `StartupConfig::load()`,
`StartupConfig::resolve_path()`) è invece testata a fondo via injection di
parametri, senza mai mutare l'ambiente reale del processo di test.

### Regola dei percorsi relativi in `startup.json` (radice del deploy)

`deploy_root(config_dir)` = cartella padre di `config_dir` (tipicamente
`Configuration\`). I percorsi relativi dentro `startup.json` (es.
`paths.shell = "shell/lare-shell.exe"`) si risolvono SEMPRE contro questa
radice — mai contro la cwd, mai contro la cartella dell'eseguibile che
legge il file.

Perché non la cartella dell'eseguibile: `lare-shell.exe` vive in `shell\`,
i binari Rust (`orchestrator.exe`, `mcp-server.exe`) vivono nella radice
del deploy — usare "la cartella di chi legge" darebbe due basi diverse a
seconda del binario. La radice del deploy è un'unica base, stabile,
indipendente da chi interpreta il valore.

`StartupConfig::resolve_path(config_dir, value)` applica la regola:
`value` assoluto → restituito com'è; `value` relativo →
`deploy_root(config_dir).join(value)`.

### Schema di `StartupConfig` (spec §6.3)

```rust
pub struct StartupConfig {
    pub ws_port: u16,          // default 7331
    pub paths: Paths,
    pub ai_model: String,      // default "claude-sonnet-4-6"
    pub autostart: Autostart,
    pub log: LogConfig,
}
pub struct Paths {
    pub shell: String,          // default "shell/lare-shell.exe"
    pub mcp_server: String,     // default "mcp-server.exe"
    pub mcp_nmap: String,       // default "mcp-nmap.exe"
    pub plugins_dir: String,    // default "plugins"
    pub pytools_dir: String,    // default "pytools"
    pub routines_dir: String,   // default "Configuration/routines"
}
pub struct Autostart { pub orchestrator: bool, pub ui: bool }  // default true, true
pub struct LogConfig { pub level: String, pub dir: String }    // default "info", "Configuration/logs"
```

Ogni struct ha `#[serde(default)]` a livello di struct: un campo assente
nel JSON prende il default di quel campo, il file intero assente prende
`StartupConfig::default()`. Questo permette un `startup.json` PARZIALE
(es. solo `{ "ws_port": 8000 }`) senza dover ripetere tutti i campi.

`StartupConfig::load(config_dir)` non fallisce mai in modo silenzioso né
con un panic:

- file assente → `(StartupConfig::default(), None)` — caso normale,
  nessun avviso: uno `startup.json` mancante è il default previsto, non un
  errore.
- file presente ma illeggibile (permessi, ecc.) → default + `Some(msg)`
  con il path e l'errore di I/O.
- file presente ma JSON malformato → default + `Some(msg)` con il path e
  l'errore di parsing. Un `startup.json` con una virgola di troppo non
  deve bloccare l'avvio: il chiamante logga l'avviso e procede con i
  default.

### Rimosso dalla v1 (2.0.0 → 2.0.1)

`resolve(env, file, default)`, `default_local_dir()`, `load_from_dir()`, i
campi `local_dir`/`roaming_dir`/`telegram_settings`/`llms_config`. Con
questi va via anche l'unica lettura di variabile d'ambiente del crate
(`LOCALAPPDATA` dentro `default_local_dir()`): il crate oggi non chiama
`std::env::var` in nessun punto.

Design completo, inventario dei 15 siti censiti nella v1, e le 3 asimmetrie
di comportamento trovate e decise in quella fase in
`Docs/superpowers/specs/2026-08-12-startup-config-design.md` (documento
storico della v1 — la v2 lo supera sostituendo l'intero meccanismo a env
var con `--config-dir`).
