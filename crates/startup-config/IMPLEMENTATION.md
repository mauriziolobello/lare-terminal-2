# Implementation — startup-config v2.0.4

## Modulo `logging` condiviso + `child_stderr_log_sink` (v2.0.4, fix da uso reale)

Spostato qui da `orchestrator/src/logging.rs` (unico consumatore all'epoca): `ui.exe` aveva
bisogno della stessa infrastruttura (init di `tracing` panic-free su file, mai un `.expect()`
interno che potrebbe far crashare un demone avviato in autostart) e `startup-config` è già
dipendenza di entrambi. Unico cambio all'API: `open_log_file`/`init_logging` prendono ora un
`filename_prefix: &str` esplicito invece di avere `"orchestrator.log"` hardcoded — ogni chiamante
ottiene il proprio file (`orchestrator.log.<data>`, `ui.log.<data>`) nella stessa cartella
`Configuration/logs/`. Il resto del ragionamento (perché `open_log_file` è pura rispetto al
subscriber globale, perché `RollingFileAppender::builder().build()` invece di
`rolling::daily(...)` per evitare il panic interno) è invariato, vedi il doc-comment del modulo.

`child_stderr_log_sink(log_dir: &Path, file_name: &str) -> std::process::Stdio`: apre (creando la
cartella se serve) un file di log in append, best-effort (`Stdio::null()` su qualunque errore —
perdere il log dello stderr di un figlio non deve mai impedirgli di partire). Riusata da 4 punti
di spawn in `orchestrator` (`tool_client.rs`, `nmap_tool_client.rs`,
`python_mcp_tool_client.rs`, `plugins/transport.rs`) che prima usavano `Stdio::inherit()` — verso
un orchestrator staccato (self-heal), `inherit()` non andava da nessuna parte di osservabile, o
(la causa delle finestre console spurie che questo fix elimina) faceva allocare a Windows una
console NUOVA per il figlio. Vedi il CHANGELOG di `orchestrator` 2.2.1 per il quadro completo.

## `spawn_detached` — un solo posto che sa staccare un processo (v2.0.3, stdio chiuse dal v2.0.4)

`spawn_detached` chiude ora esplicitamente le tre stdio del figlio (`Stdio::null()`) — richiesto
da spec §6.4 ("stdio chiusi"), mancante fino al v2.0.4. Il branch `#[cfg(not(windows))]` resta
invariato (non verificato fuori Windows, ADR-019).

Piano `Docs/i18n/ita/superpowers/plans/2026-09-07-piano-3-finestra-terminale.md` Task 3, spec
§6.4. `spawn_detached(exe: &Path, args: &[String]) -> std::io::Result<Child>`: su Windows,
`CommandExt::creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)` — equivalente Windows di
`setsid` (nessuna console ereditata dal padre, gruppo di processi proprio: un Ctrl+C nella console
del padre non raggiunge il figlio). Fuori Windows (non verificato, il prodotto oggi lo è per
intero, ADR-019): spawn semplice senza distacco, solo per non rompere la compilazione su altre
piattaforme di sviluppo.

Riusata da due chiamanti che avviano un processo "collega" senza volerne la console: `ui.exe`
(self-heal dell'orchestratore, `launcher::ensure_orchestrator` — crate `ui`) e l'orchestratore
stesso (autostart di `ui.exe --no-terminal`, `ensure_ui_sink` — crate `orchestrator`, Task 6 dello
stesso piano). Prima di questa funzione, la logica dei flag Windows sarebbe stata duplicata in
entrambi i punti (o mai scritta lato orchestratore, che non aveva alcun bisogno di spawnare
processi prima del piano 3) — un solo posto che sa COME staccare un processo, testato una volta
sola (`spawn_detached_avvia_un_processo_reale`, avvia un processo reale e verifica che parta).

## Assolutizzazione di `--config-dir` spostata qui dall'orchestrator (v2.0.2)

Fix wave della review finale (piano 1). Prima solo `orchestrator/src/main.rs` rendeva
assoluto un `--config-dir` relativo (subito dopo averlo letto, prima di
`set_current_dir(home)` più sotto in quello stesso `main()`) — `ui` e `mcp-server`,
che chiamano la stessa `config_dir_from_process()`, non ne beneficiavano affatto.

Estratta la logica in una funzione pura, `absolutize(p: PathBuf, cwd: &Path) ->
PathBuf` (assoluto invariato; relativo → `cwd.join(p)`), testabile senza toccare la
cwd reale del processo di test. `config_dir_from_process()` la chiama con
`std::env::current_dir()` al momento della chiamata; se quella lettura fallisce
(rarissimo) il path resta relativo invece di fallire l'avvio per un dettaglio
secondario. Ogni binario che chiama questa funzione (orchestrator, ui, mcp-server)
ora riceve sempre un `config_dir` assoluto, non solo l'orchestrator.

Aggiunta anche `TOKEN_FILE_NAME` (accanto a `STARTUP_FILE_NAME`): prima
`orchestrator::token_store` e `ui::config_dir` scrivevano ciascuno la stringa
letterale `"token"` per conto proprio.

Nuovi test (TDD, RED prima di `absolutize`): `absolutize` con un path relativo e uno
assoluto; `deploy_root` con `config_dir` direttamente sotto la radice di un'unità
(`C:/Configuration` → `C:/`); `resolve_path` con un valore "rooted" senza lettera di
unità (`/x`) su Windows, che fissa con un test il comportamento reale di
`PathBuf::join` in quel caso (sostituisce solo la radice della base, non l'intero
path — vedi il commento sul test per il dettaglio; chiude il debito noto #2 di
`HANDOFF.md`).

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
  reale (`std::env::args()` + `exe_dir()` + `std::env::current_dir()`, per
  assolutizzare un `--config-dir` relativo tramite `absolutize`, v2.0.2):
  comodità per i vari `main`, intenzionalmente non testata con unit test
  diretti, stesso pattern già in uso per `exe_dir()` (wrapper sottile sul
  confine col sistema operativo — vedi nota più sotto). `absolutize(p, cwd)`
  sotto di lei è invece pura e testata.
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
`resolve_config_dir()`, `deploy_root()`, `absolutize()`, `StartupConfig::
load()`, `StartupConfig::resolve_path()`) è invece testata a fondo via
injection di parametri, senza mai mutare l'ambiente reale del processo di
test.

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
