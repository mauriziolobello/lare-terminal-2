# Lare Terminal 2.0 — Piano 1: Fondamenta

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Esecutori:** ogni task va a un subagente `model: "sonnet"` (Task 1 anche `haiku`). Il supervisore riverifica **ogni** report rieseguendo i comandi di verifica del task (lezione v1: report con affermazioni non verificate).

**Goal:** il repo 2.0 compila e gira da `Test Run\` con un'unica cartella `Configuration\`, nessuna variabile d'ambiente, `ui.exe` ridotto a host di finestre (overlay F2 rimosso, pagina host nascosta), tutti i crate v1 copiati, testati e versionati `2.0.0`.

**Architecture:** copia dei crate v1 nel workspace 2.0, poi refactor in place: il crate `startup-config` diventa l'unico risolutore di `--config-dir`/`startup.json`; ogni binario (orchestrator, mcp-server, ui) e ogni figlio (mcp-server, mcp-nmap, server Python) riceve la cartella di configurazione per argomento; in `ui` il cursore overlay viene sostituito da `host.html`/`host.js`, che conserva connessione WS, lancio finestre e relay.

**Tech Stack:** Rust 1.97 (workspace Cargo), Tauri 2 (vanilla JS, `node:test`), Python 3.14 (pytools MCP, pytest), PowerShell 7.6 (script). Nessun C# in questo piano (arriva col piano 2).

**Spec:** `Docs/i18n/ita/superpowers/specs/2026-09-04-lare-terminal-2-design.md` — sezioni implementate qui: §1.3 D2/D6/D7/D8/D10, §2.1 (righe `ui`, `startup-config`, crate copiati), §5 (pagina host, rimozioni, flag `--open`), §6 (configurazione), §7 (`Test Run\`), §11 (convenzioni). **Non** implementa: protocollo host↔orchestratore (§4, piano 2), finestra terminale (§5 xterm, piano 3), finestra Markdown di output (§3.2, piano 2).

## Global Constraints

- Rust edition/toolchain: quella del workspace v1 (nessun `rust-toolchain` file; Rust 1.97.1 sulla macchina).
- **Nessun binario legge variabili d'ambiente `LARE_*`** (D6). Verifica per crate: `grep -rn 'env::var("LARE_' crates/<crate>/src` deve essere vuoto. `LOCALAPPDATA`/`APPDATA` non vengono più letti da nessun binario Lare.
- Risoluzione cartella di configurazione, unica regola: `--config-dir <path>` altrimenti `<cartella dell'eseguibile>\Configuration\` (§6.1).
- Percorsi relativi in `startup.json`: relativi alla **radice del deploy** = cartella padre di `Configuration\` (§6.3), mai alla cwd né a `exe_dir`.
- Versione di ogni crate: `2.0.0`; `CHANGELOG.md` per crate con voce "2.0.0 — fork da v1 x.y.z" (§2.2).
- TDD: test RED prima del codice; commenti prodighi e didattici in italiano (l'utente impara Rust); trait/composizione (§11).
- Commit: trailer `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>` + `Claude-Session: https://claude.ai/code/session_011CbyTfi9kvq5LfAVcvVELe`.
- Sorgente v1 (sola lettura): `C:\Users\Maurizio\Documents\Progetti\Lare Terminal`. Non modificarlo mai.
- Repo 2.0: `C:\Users\Maurizio\Documents\Progetti\Lare Terminal 2.0`, branch `main`. Ogni task finisce con un commit.

## Struttura dei file (decisa qui)

```
Lare Terminal 2.0/
├── Cargo.toml                       workspace (Task 0)
├── CLAUDE.md                        convenzioni 2.0 (Task 0)
├── .githooks/commit-msg             hook release: → Docs/i18n/ita/HANDOFF.md (Task 0)
├── crates/                          copiati da v1 (Task 1): protocol, startup-config, mcp-server,
│                                    mcp-nmap, orchestrator, plugin-protocol, plugin-ping,
│                                    plugin-counter, plugin-calc, plugin-lc, plugin-crypto, ui
├── scripts/pytools/                 copiati da v1 senza venv (Task 1)
├── deploy_test_run.ps1              target\ → Test Run\ (Task 8)
├── Test Run/                        layout di deploy (Task 8)
└── Docs/i18n/ita/                   HANDOFF, RUN-LOCAL, DEPLOY, TESTING-e2e, 06-decisions (Task 9)
```

Responsabilità nuove/cambiate:
- `crates/startup-config/src/lib.rs` — **riscritto**: `parse_config_dir`, `resolve_config_dir`, `deploy_root`, `StartupConfig::{load, resolve_path}` (Task 2).
- `crates/mcp-server/src/main.rs` — `--config-dir` (Task 3).
- `crates/orchestrator/src/{main.rs, token_store.rs, llms_config.rs, telegram/settings.rs, tool_client.rs, nmap_tool_client.rs, python_mcp_tool_client.rs, aichat/config.rs, ai_adapter.rs, aichat/service.rs}` — `--config-dir` e figli (Task 4).
- `scripts/pytools/financial-markets/{config_dir.py, server.py}` — `--config-dir` (Task 5).
- `crates/ui/src-tauri/src/{main.rs, config_dir.rs (nuovo), config.rs, search_settings.rs, plugins_view.rs, market_data_settings.rs, llm_settings.rs, aichat_settings.rs}` — `--config-dir` (Task 6).
- `crates/ui/frontend/{host.html, host.js}` (nuovi) al posto di `index.html`/`app.js`; rimozioni (Task 7).

---

### Task 0: Scaffolding del repo (workspace, CLAUDE.md, hook)

**Files:**
- Create: `Cargo.toml`
- Create: `CLAUDE.md`
- Create: `.githooks/commit-msg`
- Create: `Docs/i18n/ita/HANDOFF.md` (scheletro; Task 9 lo completa)

**Interfaces:**
- Produces: il workspace con i member elencati (Task 1 li riempie); `core.hooksPath` attivato.

- [ ] **Step 1: Cargo.toml del workspace**

```toml
[workspace]
resolver = "2"
members = [
    "crates/protocol",
    "crates/startup-config",
    "crates/mcp-server",
    "crates/mcp-nmap",
    "crates/orchestrator",
    "crates/plugin-protocol",
    "crates/plugin-ping",
    "crates/plugin-counter",
    "crates/plugin-calc",
    "crates/plugin-lc",
    "crates/plugin-crypto",
    # Il crate Tauri: membro del workspace così `cargo build -p ui` funziona dalla root.
    "crates/ui/src-tauri",
]
# `cargo build`/`cargo test` senza -p NON toccano la UI (Tauri = ~400 crate):
# la UI si compila solo su richiesta esplicita (`cargo build -p ui`).
default-members = [
    "crates/protocol", "crates/startup-config", "crates/mcp-server", "crates/mcp-nmap",
    "crates/orchestrator", "crates/plugin-protocol", "crates/plugin-ping",
    "crates/plugin-counter", "crates/plugin-calc", "crates/plugin-lc", "crates/plugin-crypto",
]
```

- [ ] **Step 2: `.githooks/commit-msg`** (copia dell'hook v1 con il nuovo percorso di HANDOFF)

```sh
#!/bin/sh
# commit-msg hook — impone l'aggiornamento di HANDOFF.md sui commit di release.
msg_file="$1"
subject="$(head -n1 "$msg_file" 2>/dev/null)"
case "$subject" in
  release:*)
    if git diff --cached --name-only | grep -qx "Docs/i18n/ita/HANDOFF.md"; then
      exit 0
    fi
    echo "[hook] Commit di release senza aggiornare Docs/i18n/ita/HANDOFF.md." >&2
    echo "[hook] Aggiorna HANDOFF.md (versioni correnti + voce FATTO) e mettilo in stage," >&2
    echo "[hook] oppure usa un prefisso non-release (feat/fix/docs/chore)." >&2
    exit 1
    ;;
  *) exit 0 ;;
esac
```

Poi: `git config core.hooksPath .githooks`.

- [ ] **Step 3: `CLAUDE.md`** — contenuto completo:

```markdown
# CLAUDE.md

## Cos'è

**Lare Terminal 2.0** — un terminale in tutto simile a una sessione PowerShell, con i "power
command" `/…` e l'AI integrata: una **host custom del motore PowerShell** in C# (`lare-shell`)
dentro una **finestra Tauri** (xterm.js + ConPTY), collegata a un **orchestratore Rust** (daemon
WS `127.0.0.1:7331` + token) che riusa quasi tutto della v1 (loop tool-use multi-LLM, gate di
conferma, plugin sidecar, tool Python MCP, Library, Telegram, AI Chat). Spec:
`Docs/i18n/ita/superpowers/specs/2026-09-04-lare-terminal-2-design.md`. La v1 resta in
`C:\Users\Maurizio\Documents\Progetti\Lare Terminal` come riferimento (sola lettura).

## Architettura (3 strati, come v1)

```
Canali (lare-shell via WS, Telegram in-process)
  → orchestrator (daemon WS + router + AI + plugin host)
    → mcp-server (stdio; shell posseduta solo per Telegram/AI Chat)
ui.exe (Tauri) = finestra terminale + host di finestre (Markdown, /config, /library, plugin, /find)
```

Crate in `crates/`: `protocol`, `startup-config`, `mcp-server`, `mcp-nmap`, `orchestrator`,
`plugin-protocol`, `plugin-*`, `ui` (in `crates/ui/src-tauri`). Host C# in `shell/lare-shell/`
(dal piano 2). Tool Python in `scripts/pytools/<dominio>/` (un venv per dominio, creato a mano).

## Configurazione (regola unica, D6)

`--config-dir <path>` altrimenti `<cartella dell'eseguibile>\Configuration\`. **Nessun binario
legge variabili d'ambiente `LARE_*`** (né `LOCALAPPDATA`/`APPDATA`). I figli ricevono
`--config-dir` per argomento. Percorsi relativi in `startup.json` sono relativi alla radice del
deploy (cartella padre di `Configuration\`). Layout di deploy e config di sviluppo: `Test Run\`
(`deploy_test_run.ps1` copia i binari lì).

## Comandi

```powershell
cargo build                                   # solo i crate backend (default-members)
cargo build -p ui                             # UI Tauri (~400 crate, lento)
cargo test                                    # test backend
cargo test -p orchestrator <substring>        # un test
cargo test -p orchestrator -- --ignored       # integrazione reale (API, binari, venv)
node --test crates/ui/frontend/*.test.mjs     # test JS puri del frontend
cargo clippy --all-targets ; cargo fmt --check

# Avvio in sviluppo (due terminali), stessa config del deploy:
cargo run -p orchestrator -- --config-dir "Test Run\Configuration" --console-log
cargo run -p ui -- --config-dir "Test Run\Configuration"
# Flag di sviluppo (solo finché non c'è il canale shell, piano 2): ui.exe --open config|library
```

Gotcha Windows: build che fallisce con `Accesso negato (os error 5)` = processo in esecuzione →
`taskkill //F //IM orchestrator.exe //IM ui.exe`. **Riavvia sempre l'orchestratore dopo una build
del backend.** Tauri: `generate_context!` incorpora `frontendDist` a compile time — dopo modifiche
al solo frontend, `cargo clean -p ui` se la build non ricompila.

## Convenzioni di progetto (non derogabili)

- **TDD genuino**: RED reale prima del codice. I test sono anche specifica leggibile.
- **OOP + SOLID**: trait/interfacce come confini d'astrazione, composizione preferita a
  ereditarietà; fake ai seam. L'utente impara Rust partendo da basi OOP: mappa i costrutti Rust
  su concetti OOP noti quando spieghi. Stesso stile in C# e JS.
- **Commenti prodighi e didattici**, in italiano — più forte del default "commenta la logica
  complessa".
- **Per ogni crate/progetto**: `CHANGELOG.md` (semver, da `2.0.0`) + `IMPLEMENTATION.md`,
  aggiornati nello stesso commit del codice.
- **`Docs/i18n/ita/HANDOFF.md`** aggiornato nello stesso commit di ogni release (hook
  `commit-msg` in `.githooks/`, attivalo una volta per clone: `git config core.hooksPath .githooks`).
- **Commit** con trailer `Co-Authored-By` + `Claude-Session`.
- **Workflow**: il codice lo costruiscono subagenti su modelli meno costosi (Sonnet; Haiku per il
  lavoro meccanico) su task ben definiti; il supervisore **riverifica ogni report** rieseguendo i
  comandi, fa l'e2e dal vivo, decide le architetture. Mai `AskUserQuestion` con l'utente.
- **Documentazione**: tutta sotto `Docs/i18n/<lingua>/` (italiano di riferimento). Spec in
  `superpowers/specs/`, piani in `superpowers/plans/`, esiti spike in `spikes/`.
- **Sicurezza**: WS solo su `127.0.0.1` + token su file; ogni comando proposto dall'AI passa dal
  gate di conferma (ADR-007) prima di eseguire.

## Dove guardare

- `Docs/i18n/ita/HANDOFF.md` — stato corrente, versioni, FATTO/DA FARE. Leggi questo per primo.
- `Docs/i18n/ita/06-decisions.md` — log ADR (continua la numerazione v1 da 015).
- `Docs/i18n/ita/superpowers/` — spec e piani. `Docs/i18n/ita/spikes/` — esiti degli spike.
- `crates/<crate>/IMPLEMENTATION.md` — dettagli per crate.
- v1: `..\Lare Terminal\Docs\STATO-ATTUALE.md` (cosa esiste e come funziona) e `Docs\06-decisions.md`.
```

- [ ] **Step 4: `Docs/i18n/ita/HANDOFF.md` scheletro**

```markdown
# HANDOFF — Lare Terminal 2.0

> Checkpoint per ripartire a contesto azzerato. Si aggiorna nello stesso commit di ogni release
> (hook `commit-msg`). Stato dettagliato per area: `crates/<crate>/IMPLEMENTATION.md`.

## Versioni correnti

(compilato dal Task 1 del piano 1)

## FATTO

- 2026-09-04/05 — brainstorming, spec, due spike (`Docs/i18n/ita/spikes/`).

## DA FARE

- Piano 1 (fondamenta) in corso: `Docs/i18n/ita/superpowers/plans/2026-09-05-piano-1-fondamenta.md`.
- Piano 2 (protocollo + host C#), piano 3 (finestra terminale Tauri).
```

- [ ] **Step 5: Verifica e commit**

Run: `git config core.hooksPath .githooks; cargo metadata --no-deps --format-version 1 2>&1 | Select-Object -First 1`
Expected: errore "failed to read `crates/protocol/Cargo.toml`" (i crate non ci sono ancora: è atteso; il workspace è sintatticamente valido). `git status` mostra i 4 file nuovi.

```bash
git add Cargo.toml CLAUDE.md .githooks/commit-msg Docs/i18n/ita/HANDOFF.md
git commit -m "chore: scaffolding del workspace 2.0 (Cargo.toml, CLAUDE.md, hook commit-msg, HANDOFF)"
```

---

### Task 1: Copia dei crate v1, bump a 2.0.0, baseline verde

**Files:**
- Create: `crates/**` (copia da `C:\Users\Maurizio\Documents\Progetti\Lare Terminal\crates\**`, escluso qualunque `target/`)
- Create: `scripts/pytools/**` (copia da v1, esclusi `venv/`, `__pycache__/`, `.pytest_cache/`, `tickers_us.json`, `*_cache.json`, `discoveries.json`)
- Modify: ogni `crates/*/Cargo.toml` (`version = "2.0.0"`), `crates/ui/src-tauri/tauri.conf.json` (`"version": "2.0.0"`), ogni `CHANGELOG.md`

**Interfaces:**
- Produces: workspace che compila; baseline test: Rust verde su tutti i default-members, `cargo test -p ui --no-run` verde, `node --test` 269 test verdi (v1 al 2026-09-05).

- [ ] **Step 1: Copia** (PowerShell, dalla root del repo 2.0)

```powershell
$v1 = "C:\Users\Maurizio\Documents\Progetti\Lare Terminal"
robocopy "$v1\crates" ".\crates" /E /XD target node_modules gen /NFL /NDL /NJH /NJS
robocopy "$v1\scripts\pytools" ".\scripts\pytools" /E /XD venv __pycache__ .pytest_cache /XF tickers_us.json fundamentals_cache.json technical_cache.json discoveries.json /NFL /NDL /NJH /NJS
Copy-Item "$v1\Cargo.lock" ".\Cargo.lock"
```

- [ ] **Step 2: Versioni** — in ogni `crates/*/Cargo.toml` e `crates/ui/src-tauri/Cargo.toml`: `version = "2.0.0"`. In `crates/ui/src-tauri/tauri.conf.json`: `"version": "2.0.0"`. Verifica: `Select-String -Path crates\*\Cargo.toml,crates\ui\src-tauri\Cargo.toml -Pattern '^version' ` → tutte `2.0.0`.

- [ ] **Step 3: CHANGELOG** — in testa a ogni `crates/<crate>/CHANGELOG.md` (per `ui`: `crates/ui/CHANGELOG.md`), sopra la voce v1 più recente:

```markdown
## 2.0.0 — 2026-09-05 — fork da v1 <versione v1 del crate>

Copia del crate dalla v1 (`mauriziolobello/lare-terminal`) nel repo 2.0. Nessuna modifica
funzionale in questa voce; le modifiche del piano 1 seguono nelle voci successive.
```

(la versione v1 è quella letta nel `Cargo.toml` copiato: protocol 0.15.4, startup-config 0.1.0, mcp-server 0.7.1, mcp-nmap 0.8.2, orchestrator 0.41.21, plugin-protocol 0.2.1, plugin-ping 0.1.0, plugin-counter 0.1.0, plugin-calc 0.2.0, plugin-lc 0.4.5, plugin-crypto 1.0.1, ui 0.47.1 — confronta col file, non fidarti di questa lista).

- [ ] **Step 4: Baseline**

Run: `cargo build` → Expected: `Finished`. Run: `cargo test` → Expected: tutti i test passano (esclusi gli `#[ignore]`). Run: `cargo build -p ui` → `Finished` (prima volta: minuti). Run: `cargo test -p ui --no-run` → `Finished`. Run: `node --test crates/ui/frontend/*.test.mjs` → Expected: `# pass 269` / `# fail 0`.

Se un test fallisce per percorsi assoluti della macchina v1, correggi solo quello e annotalo nel report.

- [ ] **Step 5: HANDOFF versioni** — in `Docs/i18n/ita/HANDOFF.md` sezione "Versioni correnti": elenco `crate 2.0.0 (da v1 x.y.z)`.

- [ ] **Step 6: Commit**

```bash
git add -A crates scripts Cargo.lock Docs/i18n/ita/HANDOFF.md
git commit -m "chore: copia dei crate v1 nel workspace 2.0, versioni 2.0.0, baseline verde"
```

---

### Task 2: `startup-config` 2.0 — `--config-dir`, `startup.json` nuovo schema

**Files:**
- Modify (riscrittura): `crates/startup-config/src/lib.rs`
- Modify: `crates/startup-config/Cargo.toml` (aggiungi `serde = { version = "1", features = ["derive"] }`, `serde_json = "1"` se non già presenti; dev-dep `tempfile = "3"`)
- Modify: `crates/startup-config/CHANGELOG.md`, `IMPLEMENTATION.md`

**Interfaces:**
- Produces (usate da Task 3, 4, 6):

```rust
pub const CONFIG_DIR_FLAG: &str = "--config-dir";
pub const DEFAULT_CONFIG_DIR_NAME: &str = "Configuration";

pub fn parse_config_dir(args: &[String]) -> Option<PathBuf>;
pub fn resolve_config_dir(flag: Option<PathBuf>, exe_dir: &Path) -> PathBuf;
pub fn config_dir_from_process() -> PathBuf;      // argv reali + current_exe()
pub fn exe_dir() -> std::io::Result<PathBuf>;     // invariato dalla v1
pub fn deploy_root(config_dir: &Path) -> PathBuf;
pub fn has_flag(args: &[String], flag: &str) -> bool;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)] pub struct StartupConfig { pub ws_port: u16, pub paths: Paths, pub ai_model: String, pub autostart: Autostart, pub log: LogConfig }
#[derive(...)] pub struct Paths { pub shell: String, pub mcp_server: String, pub mcp_nmap: String, pub plugins_dir: String, pub pytools_dir: String, pub routines_dir: String }
#[derive(...)] pub struct Autostart { pub orchestrator: bool, pub ui: bool }
#[derive(...)] pub struct LogConfig { pub level: String, pub dir: String }

impl StartupConfig {
    pub fn load(config_dir: &Path) -> (StartupConfig, Option<String>); // (config, avviso se malformato)
    pub fn resolve_path(config_dir: &Path, value: &str) -> PathBuf;   // assoluto → com'è; relativo → deploy_root(config_dir)/value
}
```

Rimossi dalla v1: `resolve(env, file, default)`, `default_local_dir()`, `load_from_dir()`, i campi `local_dir`/`roaming_dir`/`telegram_settings`/`llms_config`.

- [ ] **Step 1: Test RED** — sostituisci il modulo `tests` di `lib.rs` con:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn args(v: &[&str]) -> Vec<String> { v.iter().map(|s| s.to_string()).collect() }

    // ── parse_config_dir ─────────────────────────────────────────────────
    #[test]
    fn parse_flag_with_separate_value() {
        assert_eq!(parse_config_dir(&args(&["ui.exe", "--config-dir", "D:/cfg"])), Some(PathBuf::from("D:/cfg")));
    }
    #[test]
    fn parse_flag_with_equals() {
        assert_eq!(parse_config_dir(&args(&["x", "--config-dir=D:/cfg"])), Some(PathBuf::from("D:/cfg")));
    }
    #[test]
    fn parse_absent_flag_is_none() {
        assert_eq!(parse_config_dir(&args(&["x", "--open", "config"])), None);
    }
    #[test]
    fn parse_flag_without_value_is_none() {
        assert_eq!(parse_config_dir(&args(&["x", "--config-dir"])), None);
    }

    // ── resolve_config_dir ───────────────────────────────────────────────
    #[test]
    fn resolve_flag_wins() {
        let p = resolve_config_dir(Some(PathBuf::from("D:/cfg")), Path::new("C:/app"));
        assert_eq!(p, PathBuf::from("D:/cfg"));
    }
    #[test]
    fn resolve_default_is_configuration_next_to_exe() {
        let p = resolve_config_dir(None, Path::new("C:/Lare"));
        assert_eq!(p, Path::new("C:/Lare").join("Configuration"));
    }

    // ── deploy_root ──────────────────────────────────────────────────────
    #[test]
    fn deploy_root_is_parent_of_config_dir() {
        assert_eq!(deploy_root(Path::new("C:/Lare/Configuration")), PathBuf::from("C:/Lare"));
    }

    // ── StartupConfig::load ──────────────────────────────────────────────
    #[test]
    fn load_absent_file_gives_defaults_without_warning() {
        let dir = tempfile::tempdir().unwrap();
        let (cfg, warn) = StartupConfig::load(dir.path());
        assert_eq!(cfg, StartupConfig::default());
        assert!(warn.is_none());
        assert_eq!(cfg.ws_port, 7331);
        assert_eq!(cfg.paths.mcp_server, "mcp-server.exe");
        assert_eq!(cfg.paths.routines_dir, "Configuration/routines");
        assert_eq!(cfg.ai_model, "claude-sonnet-4-6");
        assert!(cfg.autostart.orchestrator && cfg.autostart.ui);
        assert_eq!(cfg.log.dir, "Configuration/logs");
        assert_eq!(cfg.log.level, "info");
    }
    #[test]
    fn load_partial_file_fills_missing_with_defaults() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("startup.json"), r#"{ "ws_port": 8000, "paths": { "plugins_dir": "plug" } }"#).unwrap();
        let (cfg, warn) = StartupConfig::load(dir.path());
        assert!(warn.is_none());
        assert_eq!(cfg.ws_port, 8000);
        assert_eq!(cfg.paths.plugins_dir, "plug");
        assert_eq!(cfg.paths.mcp_server, "mcp-server.exe"); // default conservato
    }
    #[test]
    fn load_malformed_file_gives_defaults_with_warning() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("startup.json"), "{ not json").unwrap();
        let (cfg, warn) = StartupConfig::load(dir.path());
        assert_eq!(cfg, StartupConfig::default());
        assert!(warn.unwrap().contains("startup.json"));
    }

    // ── resolve_path ─────────────────────────────────────────────────────
    #[test]
    fn resolve_path_relative_is_against_deploy_root() {
        let p = StartupConfig::resolve_path(Path::new("C:/Lare/Configuration"), "shell/lare-shell.exe");
        assert_eq!(p, Path::new("C:/Lare").join("shell/lare-shell.exe"));
    }
    #[test]
    fn resolve_path_absolute_is_kept() {
        let p = StartupConfig::resolve_path(Path::new("C:/Lare/Configuration"), "D:/tools/x.exe");
        assert_eq!(p, PathBuf::from("D:/tools/x.exe"));
    }
    #[test]
    fn resolve_path_with_nested_config_dir() {
        let p = StartupConfig::resolve_path(Path::new("C:/a/b/Configuration"), "plugins");
        assert_eq!(p, Path::new("C:/a/b").join("plugins"));
    }

    // ── has_flag ─────────────────────────────────────────────────────────
    #[test]
    fn has_flag_detects_console_log() {
        assert!(has_flag(&args(&["orchestrator.exe", "--console-log"]), "--console-log"));
        assert!(!has_flag(&args(&["orchestrator.exe"]), "--console-log"));
    }
}
```

- [ ] **Step 2: RED** — Run: `cargo test -p startup-config` → Expected: errori di compilazione (`parse_config_dir` non definito, ecc.).

- [ ] **Step 3: Implementazione** — `lib.rs` (sostituisce tutto il codice non-test):

```rust
//! # startup-config — cartella di configurazione e `startup.json` (2.0)
//!
//! Regola unica per OGNI binario Lare (orchestrator, mcp-server, ui, plugin,
//! server Python): la cartella di configurazione è `--config-dir <path>` se
//! passato sulla riga di comando, altrimenti `<cartella dell'eseguibile>\Configuration\`.
//! Nessuna variabile d'ambiente viene letta (decisione D6 dello spec 2.0):
//! nella v1 tre lettori indipendenti (orchestrator, ui, Python) della stessa
//! env var divergevano in silenzio.
//!
//! Dentro la cartella vive `startup.json`, i cui percorsi relativi sono
//! risolti rispetto alla RADICE DEL DEPLOY = cartella padre di `Configuration\`
//! (non alla cwd, non alla cartella dell'eseguibile: `lare-shell.exe` vive in
//! `shell\`, i binari Rust nella radice — un'unica base evita due risultati).
//!
//! Analogia OOP: `StartupConfig` è un "value object" immutabile con default;
//! le funzioni libere sono metodi statici di una classe di utilità.

use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

pub const CONFIG_DIR_FLAG: &str = "--config-dir";
pub const DEFAULT_CONFIG_DIR_NAME: &str = "Configuration";
pub const STARTUP_FILE_NAME: &str = "startup.json";

/// Estrae `--config-dir <path>` oppure `--config-dir=<path>` da `args`
/// (argv completo, `args[0]` = eseguibile). Ignora ogni altro argomento.
/// Flag presente ma senza valore → `None` (il chiamante userà il default).
pub fn parse_config_dir(args: &[String]) -> Option<PathBuf> {
    let mut iter = args.iter();
    while let Some(a) = iter.next() {
        if a == CONFIG_DIR_FLAG {
            return iter.next().filter(|v| !v.trim().is_empty()).map(PathBuf::from);
        }
        if let Some(v) = a.strip_prefix(&format!("{CONFIG_DIR_FLAG}=")) {
            if !v.trim().is_empty() {
                return Some(PathBuf::from(v));
            }
        }
    }
    None
}

/// `true` se `flag` compare in `args` (flag booleani come `--console-log`).
pub fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|a| a == flag)
}

/// Flag esplicito > `<exe_dir>/Configuration`. `exe_dir` è un parametro (non
/// letto qui) così il test non dipende dalla posizione del binario di test.
pub fn resolve_config_dir(flag: Option<PathBuf>, exe_dir: &Path) -> PathBuf {
    flag.unwrap_or_else(|| exe_dir.join(DEFAULT_CONFIG_DIR_NAME))
}

/// Comodità per i `main`: argv reali + cartella dell'eseguibile reale.
/// Se `current_exe()` fallisce (caso rarissimo) cade sulla cwd.
pub fn config_dir_from_process() -> PathBuf {
    let args: Vec<String> = std::env::args().collect();
    let exe = exe_dir().unwrap_or_else(|_| PathBuf::from("."));
    resolve_config_dir(parse_config_dir(&args), &exe)
}

/// Cartella dell'eseguibile in esecuzione (invariata dalla v1): `current_exe()`
/// riflette il binario davvero lanciato, mai la cwd.
pub fn exe_dir() -> io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    exe.parent().map(Path::to_path_buf).ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "current_exe() non ha una cartella padre")
    })
}

/// Radice del deploy = cartella padre di `config_dir`. Se `config_dir` non ha
/// padre (es. `/`), è la radice stessa.
pub fn deploy_root(config_dir: &Path) -> PathBuf {
    config_dir.parent().map(Path::to_path_buf).unwrap_or_else(|| config_dir.to_path_buf())
}

/// Schema di `startup.json` (spec §6.3). Ogni campo ha un default: file
/// assente = tutti i default; campo assente = default di quel campo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StartupConfig {
    pub ws_port: u16,
    pub paths: Paths,
    pub ai_model: String,
    pub autostart: Autostart,
    pub log: LogConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Paths {
    pub shell: String,
    pub mcp_server: String,
    pub mcp_nmap: String,
    pub plugins_dir: String,
    pub pytools_dir: String,
    pub routines_dir: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Autostart {
    pub orchestrator: bool,
    pub ui: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LogConfig {
    pub level: String,
    pub dir: String,
}

impl Default for StartupConfig {
    fn default() -> Self {
        Self { ws_port: 7331, paths: Paths::default(), ai_model: "claude-sonnet-4-6".into(), autostart: Autostart::default(), log: LogConfig::default() }
    }
}
impl Default for Paths {
    fn default() -> Self {
        Self {
            shell: "shell/lare-shell.exe".into(),
            mcp_server: "mcp-server.exe".into(),
            mcp_nmap: "mcp-nmap.exe".into(),
            plugins_dir: "plugins".into(),
            pytools_dir: "pytools".into(),
            routines_dir: "Configuration/routines".into(),
        }
    }
}
impl Default for Autostart {
    fn default() -> Self { Self { orchestrator: true, ui: true } }
}
impl Default for LogConfig {
    fn default() -> Self { Self { level: "info".into(), dir: "Configuration/logs".into() } }
}

impl StartupConfig {
    /// Legge `<config_dir>/startup.json`. Ritorna sempre una configurazione
    /// utilizzabile: file assente → default, nessun avviso; file presente ma
    /// illeggibile/malformato → default + avviso (il chiamante lo logga: un
    /// `startup.json` con una virgola di troppo non deve fallire in silenzio).
    pub fn load(config_dir: &Path) -> (StartupConfig, Option<String>) {
        let path = config_dir.join(STARTUP_FILE_NAME);
        match std::fs::read_to_string(&path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => (StartupConfig::default(), None),
            Err(e) => (StartupConfig::default(), Some(format!("{}: errore di lettura ({e}) — uso i default", path.display()))),
            Ok(content) => match serde_json::from_str::<StartupConfig>(&content) {
                Ok(cfg) => (cfg, None),
                Err(e) => (StartupConfig::default(), Some(format!("{}: JSON malformato ({e}) — uso i default", path.display()))),
            },
        }
    }

    /// Risolve un valore di `paths`: assoluto → com'è; relativo → rispetto
    /// alla radice del deploy (`deploy_root(config_dir)`).
    pub fn resolve_path(config_dir: &Path, value: &str) -> PathBuf {
        let p = Path::new(value);
        if p.is_absolute() { p.to_path_buf() } else { deploy_root(config_dir).join(p) }
    }
}
```

- [ ] **Step 4: GREEN** — Run: `cargo test -p startup-config` → Expected: 14 test passano. Nota: `cargo build` del workspace ora **fallisce** in orchestrator/mcp-server (usano l'API vecchia): atteso, li sistemano i Task 3-4. Per il commit di questo task basta `cargo test -p startup-config`.

- [ ] **Step 5: Docs** — `CHANGELOG.md`: voce `2.0.1 — --config-dir, nuovo schema startup.json, via env var`; `IMPLEMENTATION.md`: descrivi API e regola dei percorsi. Bump `version = "2.0.1"` nel `Cargo.toml`.

- [ ] **Step 6: Commit**

```bash
git add crates/startup-config
git commit -m "feat(startup-config): --config-dir, startup.json 2.0, nessuna env var"
```

---

### Task 3: `mcp-server` — `--config-dir`, niente env var

**Files:**
- Modify: `crates/mcp-server/src/main.rs` (blocco righe ~511-541 della copia v1: `exe_dir`/`startup_cfg`/`local_dir`/`routines_root`)
- Modify: `crates/mcp-server/src/routines.rs` (`resolve_root`, ~righe 41-47)
- Modify: `crates/mcp-server/CHANGELOG.md`, `IMPLEMENTATION.md`

**Interfaces:**
- Consumes: `startup_config::{config_dir_from_process, StartupConfig}`.
- Produces: `mcp-server.exe --config-dir <dir>` (Task 4 lo passa); `mcp_server::routines::resolve_root(config_dir: &Path, cfg: &StartupConfig) -> PathBuf`.

- [ ] **Step 1: Test RED** — in `routines.rs`, nel modulo test esistente, sostituisci i test di `resolve_root` con:

```rust
#[test]
fn resolve_root_uses_startup_routines_dir_relative_to_deploy_root() {
    let cfg = startup_config::StartupConfig::default(); // routines_dir = "Configuration/routines"
    let root = resolve_root(std::path::Path::new("C:/Lare/Configuration"), &cfg);
    assert_eq!(root, std::path::Path::new("C:/Lare").join("Configuration/routines"));
}
#[test]
fn resolve_root_absolute_routines_dir_is_kept() {
    let mut cfg = startup_config::StartupConfig::default();
    cfg.paths.routines_dir = "D:/routines".into();
    let root = resolve_root(std::path::Path::new("C:/Lare/Configuration"), &cfg);
    assert_eq!(root, std::path::PathBuf::from("D:/routines"));
}
```

- [ ] **Step 2: RED** — Run: `cargo test -p mcp-server resolve_root` → Expected: errore di compilazione (firma diversa).

- [ ] **Step 3: Implementazione**

`routines.rs`:
```rust
/// Cartella delle routine: `paths.routines_dir` di startup.json, relativa alla
/// radice del deploy (default `Configuration/routines`). Nessuna env var.
pub fn resolve_root(config_dir: &Path, cfg: &startup_config::StartupConfig) -> PathBuf {
    startup_config::StartupConfig::resolve_path(config_dir, &cfg.paths.routines_dir)
}
```

`main.rs`, al posto del blocco `exe_dir`/`startup_cfg`/`local_dir`/`routines_root`:
```rust
    // ── Configurazione (2.0): --config-dir oppure <exe_dir>/Configuration ─
    let config_dir = startup_config::config_dir_from_process();
    let (startup_cfg, warn) = startup_config::StartupConfig::load(&config_dir);
    if let Some(w) = warn {
        tracing::warn!("{w}");
    }
    tracing::info!("config dir: {}", config_dir.display());
    let routines_root = mcp_server::routines::resolve_root(&config_dir, &startup_cfg);
```

Rimuovi ogni riferimento a `local_dir` non più necessario (verifica con `cargo build -p mcp-server`). Se `SessionServer` o altri moduli usavano `local_dir` per altro (grep `local_dir` nel crate), passa `config_dir` al loro posto — è la stessa cartella.

- [ ] **Step 4: GREEN + grep** — Run: `cargo test -p mcp-server` → tutti passano. Run: `grep -rn 'env::var("LARE_' crates/mcp-server/src` → vuoto. Run: `grep -rn "LOCALAPPDATA" crates/mcp-server/src` → vuoto.

- [ ] **Step 5: Docs** — CHANGELOG `2.0.1 — --config-dir al posto di LARE_LOCAL_DIR/LARE_ROUTINES_DIR`; IMPLEMENTATION aggiornato; bump `2.0.1`.

- [ ] **Step 6: Commit**

```bash
git add crates/mcp-server
git commit -m "feat(mcp-server): --config-dir, routine da startup.json, nessuna env var"
```

---

### Task 4: `orchestrator` — `--config-dir`, figli con argomento, log su file

**Files:**
- Modify: `crates/orchestrator/src/main.rs` (righe della copia v1: ~78-92 `default_ai_adapter`, ~126-175 config/token/llms, ~258-268 telegram, ~366-386 plugin, `ws::serve(LISTEN_ADDR…)` ~687, init `tracing` all'inizio di `main`)
- Modify: `crates/orchestrator/src/token_store.rs` (riscrittura senza env)
- Modify: `crates/orchestrator/src/llms_config.rs` (`resolve_path` ~126-132)
- Modify: `crates/orchestrator/src/telegram/settings.rs` (`resolve_path` ~43-51)
- Modify: `crates/orchestrator/src/tool_client.rs` (`McpToolClient::resolve` ~646-673, spawn ~696-703)
- Modify: `crates/orchestrator/src/nmap_tool_client.rs` (`resolve` ~146-176, spawn ~190-196)
- Modify: `crates/orchestrator/src/python_mcp_tool_client.rs` (`resolve` ~201-246, spawn ~261-268) e i 3 call site: `external_channel.rs:271,385`, `ws.rs:852`
- Modify: `crates/orchestrator/src/aichat/config.rs` (~146-179), `crates/orchestrator/src/ai_adapter.rs` (~359-364 `memory_file_path`), `crates/orchestrator/src/aichat/service.rs` (~4646-4657 duplicato)
- Modify: `crates/orchestrator/Cargo.toml` (aggiungi `tracing-appender = "0.2"`)
- Modify: `crates/orchestrator/CHANGELOG.md`, `IMPLEMENTATION.md`

**Interfaces:**
- Consumes: `startup_config::{config_dir_from_process, has_flag, StartupConfig, deploy_root}`.
- Produces: `orchestrator.exe --config-dir <dir> [--console-log]`; spawn dei figli con `--config-dir <dir>`; `token_store::resolve_token(config_dir: &Path) -> String`; `McpToolClient::resolve(config_dir: &Path, cfg: &StartupConfig) -> Self`; `NmapToolClient::resolve(config_dir, cfg)`; `PythonMcpToolClient::resolve(config_dir, cfg, domain_id, script_relpath, tool_specs, call_timeout_secs)`; `memory_file_path(config_dir: &Path, label_base: &str) -> PathBuf` (una sola copia, in `ai_adapter.rs`, riusata da `aichat/service.rs`).

Struttura consigliata: un tipo di comodo in `main.rs`

```rust
/// Tutto ciò che il resto dell'orchestratore deve sapere della configurazione,
/// risolto UNA volta in `main`. Analogia OOP: un "context object" immutabile
/// passato per riferimento (Arc) a chi ne ha bisogno.
pub struct RuntimeConfig {
    pub config_dir: PathBuf,
    pub startup: StartupConfig,
}
impl RuntimeConfig {
    pub fn path(&self, value: &str) -> PathBuf { StartupConfig::resolve_path(&self.config_dir, value) }
    pub fn plugins_dir(&self) -> PathBuf { self.path(&self.startup.paths.plugins_dir) }
    pub fn pytools_dir(&self) -> PathBuf { self.path(&self.startup.paths.pytools_dir) }
    pub fn mcp_server_exe(&self) -> PathBuf { self.path(&self.startup.paths.mcp_server) }
    pub fn mcp_nmap_exe(&self) -> PathBuf { self.path(&self.startup.paths.mcp_nmap) }
    pub fn log_dir(&self) -> PathBuf { self.path(&self.startup.log.dir) }
}
```

- [ ] **Step 1: Test RED — token** — in `token_store.rs` sostituisci i test con:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn generate_token_is_64_lowercase_hex_chars() {
        let t = generate_token();
        assert_eq!(t.len(), 64);
        assert!(t.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }
    #[test]
    fn reads_existing_token_file_trimmed() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("token"), "  abc123  \n").unwrap();
        assert_eq!(resolve_token(dir.path()), "abc123");
    }
    #[test]
    fn first_launch_generates_and_persists_token() {
        let dir = tempdir().unwrap();
        let t = resolve_token(dir.path());
        assert_eq!(t.len(), 64);
        assert_eq!(std::fs::read_to_string(dir.path().join("token")).unwrap(), t);
        // seconda chiamata: legge lo stesso token, non ne genera un altro
        assert_eq!(resolve_token(dir.path()), t);
    }
    #[test]
    fn empty_token_file_is_regenerated() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("token"), "   ").unwrap();
        let t = resolve_token(dir.path());
        assert_eq!(t.len(), 64);
    }
}
```

- [ ] **Step 2: Implementazione token** — `token_store.rs` (sostituisce tutto il non-test):

```rust
//! # token_store — token di autenticazione del WS (ADR-007), 2.0
//!
//! Il token vive SOLO nel file `<config_dir>/token`. Nessuna variabile
//! d'ambiente (D6): nella v1 `LARE_TOKEN` era un override che, impostato in
//! un solo processo dei due, produceva "connection refused" senza spiegazione.
//! Al primo avvio il token viene generato (256 bit) e scritto; ui.exe e
//! lare-shell.exe leggono lo stesso file dalla stessa cartella.

use std::path::Path;

/// Legge `<config_dir>/token`; se assente o vuoto lo genera e lo scrive.
pub fn resolve_token(config_dir: &Path) -> String {
    let token_path = config_dir.join("token");
    if let Some(t) = read_token_file(&token_path) {
        return t;
    }
    let token = generate_token();
    let _ = std::fs::create_dir_all(config_dir);
    match std::fs::write(&token_path, &token) {
        Ok(_) => eprintln!("[token] primo avvio: token creato in {}", token_path.display()),
        Err(e) => eprintln!("[token] impossibile salvare il token in {}: {e} — token temporaneo non persistito", token_path.display()),
    }
    token
}

/// 256 bit casuali, 64 caratteri esadecimali minuscoli.
pub fn generate_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn read_token_file(path: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    let t = raw.trim().to_string();
    if t.is_empty() { None } else { Some(t) }
}
```

- [ ] **Step 3: Test RED — resolver dei figli** — in `tool_client.rs` (modulo test) aggiungi:

```rust
#[test]
fn mcp_server_path_comes_from_startup_paths() {
    let cfg = startup_config::StartupConfig::default();
    let c = McpToolClient::resolve(std::path::Path::new("C:/Lare/Configuration"), &cfg);
    assert_eq!(c.mcp_server_path, std::path::Path::new("C:/Lare").join("mcp-server.exe"));
    assert_eq!(c.config_dir, std::path::PathBuf::from("C:/Lare/Configuration"));
}
```
Analogo in `nmap_tool_client.rs` (`mcp_nmap_path`, `mcp-nmap.exe`) e in `python_mcp_tool_client.rs`:
```rust
#[test]
fn python_client_paths_come_from_startup_pytools_dir() {
    let cfg = startup_config::StartupConfig::default(); // pytools_dir = "pytools"
    let r = PythonMcpToolClient::resolve(std::path::Path::new("C:/Lare/Configuration"), &cfg, "python-ping", "server.py", vec![], 30);
    // il venv non esiste: l'errore deve citare il percorso atteso, calcolato dalla radice del deploy
    let msg = format!("{:?}", r.err().unwrap());
    assert!(msg.contains("python-ping"));
    assert!(msg.replace('\\', "/").contains("C:/Lare/pytools/python-ping"));
}
```
Se i campi `mcp_server_path`/`config_dir` sono privati, rendili `pub(crate)`.

- [ ] **Step 4: RED** — Run: `cargo test -p orchestrator mcp_server_path_comes` → errore di compilazione.

- [ ] **Step 5: Implementazione figli**

`tool_client.rs` — `McpToolClient`: aggiungi campo `config_dir: PathBuf`; `resolve`:
```rust
    /// Percorso di mcp-server.exe da `startup.json` (`paths.mcp_server`,
    /// default: sibling nella radice del deploy). Nessuna env var.
    pub fn resolve(config_dir: &Path, cfg: &startup_config::StartupConfig) -> Self {
        let mcp_server_path = startup_config::StartupConfig::resolve_path(config_dir, &cfg.paths.mcp_server);
        Self::new(mcp_server_path, config_dir.to_path_buf())
    }
```
e nello spawn (~696): `c.arg(startup_config::CONFIG_DIR_FLAG).arg(&self.config_dir)` prima di `.stdin(...)`.

`nmap_tool_client.rs` — identico con `paths.mcp_nmap` e `mcp_nmap_path`; spawn con `--config-dir`.

`python_mcp_tool_client.rs` — nuova firma:
```rust
    pub fn resolve(
        config_dir: &Path,
        cfg: &startup_config::StartupConfig,
        domain_id: &str,
        script_relpath: &str,
        tool_specs: Vec<PythonToolSpec>,
        call_timeout_secs: u64,
    ) -> anyhow::Result<Self> {
        let pytools_root = startup_config::StartupConfig::resolve_path(config_dir, &cfg.paths.pytools_dir);
        let domain_dir = pytools_root.join(domain_id);
        // ... resto invariato (venv/Scripts/python.exe, controllo esistenza) ...
        // conserva `config_dir` nel client per passarlo al figlio
    }
```
spawn (~261): `c.arg(&self.script_path).arg(startup_config::CONFIG_DIR_FLAG).arg(&self.config_dir)`.
Call site: `external_channel.rs:271,385` e `ws.rs:852` ricevono `config_dir` e `cfg` (passa un `Arc<RuntimeConfig>` fino a lì: `EXTERNAL_TOOL_CHANNELS` è una tabella di closure — cambia la firma delle factory in `Fn(&RuntimeConfig, &Arc<dyn ToolClient>) -> …` e `resolve_channel_tools` di conseguenza; i test del registry usano `RuntimeConfig` con `StartupConfig::default()` e una tempdir).

- [ ] **Step 6: `main.rs`** — all'inizio di `main`, PRIMA del tracing:

```rust
    let args: Vec<String> = std::env::args().collect();
    let config_dir = startup_config::config_dir_from_process();
    let (startup, startup_warn) = startup_config::StartupConfig::load(&config_dir);
    let rt = Arc::new(RuntimeConfig { config_dir: config_dir.clone(), startup });

    // ── Log: sempre su file (Configuration/logs/orchestrator.log, rotazione
    //    giornaliera); ANCHE su console solo con --console-log (init_*.ps1).
    //    Un orchestratore avviato in autostart non deve sporcare il terminale.
    let log_dir = rt.log_dir();
    let _ = std::fs::create_dir_all(&log_dir);
    let file_appender = tracing_appender::rolling::daily(&log_dir, "orchestrator.log");
    let (file_writer, _guard) = tracing_appender::non_blocking(file_appender);
    let level = rt.startup.log.level.parse::<tracing::Level>().unwrap_or(tracing::Level::INFO);
    let console = startup_config::has_flag(&args, "--console-log");
    use tracing_subscriber::prelude::*;
    let file_layer = tracing_subscriber::fmt::layer().with_writer(file_writer).with_ansi(false);
    let registry = tracing_subscriber::registry().with(tracing_subscriber::filter::LevelFilter::from_level(level)).with(file_layer);
    if console {
        registry.with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr)).init();
    } else {
        registry.init();
    }
    if let Some(w) = startup_warn { tracing::warn!("{w}"); }
    tracing::info!("config dir: {}", config_dir.display());
```
(adatta all'`init` tracing già presente nel file: sostituiscilo, non duplicarlo; `_guard` deve vivere fino alla fine di `main`.)

Poi, in ordine nel file:
- token: `token_store::resolve_token(&config_dir)`.
- `default_ai_adapter(model: &str)` riceve `rt.startup.ai_model` (via `ANTHROPIC_API_KEY` resta env: è la chiave del provider, esclusa da D6 come `llms.json`; documentalo nel commento).
- llms: `llms_config::resolve_path(config_dir: &Path) -> PathBuf { config_dir.join("llms.json") }`.
- telegram: `telegram::settings::resolve_path(config_dir: &Path) -> PathBuf { config_dir.join("telegramsettings.json") }`; lo stato `telegram-state.json` già sotto `local_dir` → `config_dir`.
- plugin: `let plugins_dir = rt.plugins_dir(); let storage_root = config_dir.join("plugin-storage");` — **rimuovi** il caso speciale di `LARE_PLUGINS_DIR` (commento v1 righe 370-373): un solo risolutore.
- aichat: `aichat::config` riceve `config_dir` (file `network.json` in `config_dir`), niente `exe_dir`/`startup_cfg` propri.
- memory: `ai_adapter::memory_file_path(config_dir, label_base) = config_dir.join(format!("memory-{label_base}.md"))`; `aichat/service.rs` chiama quella (elimina il duplicato).
- search/notes/altri file oggi sotto `local_dir`: `config_dir` (grep `local_dir` nel crate: ogni occorrenza diventa `config_dir` o `rt.config_dir`).
- ws: `let listen = format!("127.0.0.1:{}", rt.startup.ws_port);` al posto di `LISTEN_ADDR`; `McpToolClient::resolve(&config_dir, &rt.startup)`.

- [ ] **Step 7: GREEN + grep**

Run: `cargo test -p orchestrator` → tutti passano (esclusi `#[ignore]`). Run: `grep -rn 'env::var("LARE_' crates/orchestrator/src` → vuoto. Run: `grep -rn 'LOCALAPPDATA\|APPDATA' crates/orchestrator/src` → vuoto. Run: `grep -rn "local_dir" crates/orchestrator/src` → solo commenti storici o nulla. Aggiorna i messaggi dei test `#[ignore]` che citavano `LARE_MCP_SERVER` ("set LARE_MCP_SERVER or run cargo build first" → "esegui cargo build -p mcp-server: il binario viene cercato in <deploy_root>/mcp-server.exe; nei test passa `paths.mcp_server` assoluto verso target/debug").

- [ ] **Step 8: Test di integrazione con binario reale** (già `#[ignore]` in v1, `tool_client.rs` ~1800): adegua per usare `StartupConfig { paths.mcp_server = <target/debug/mcp-server.exe assoluto> }` e verifica che il figlio riceva `--config-dir` (il test crea una tempdir come config_dir e controlla che `mcp-server` scriva/legga le routine lì). Run: `cargo build -p mcp-server; cargo test -p orchestrator --test '*' -- --ignored mcp_server` → passa.

- [ ] **Step 9: Docs + commit** — CHANGELOG `2.0.1`, IMPLEMENTATION (sezione "Configurazione 2.0: RuntimeConfig, figli con --config-dir, log su file"), bump `2.0.1`.

```bash
git add crates/orchestrator Cargo.lock
git commit -m "feat(orchestrator): --config-dir, RuntimeConfig, figli con argomento, log su file, nessuna env var"
```

---

### Task 5: Python pytools — `--config-dir`

**Files:**
- Create: `scripts/pytools/financial-markets/config_dir.py` (sostituisce `local_dir.py`, che va eliminato)
- Create: `scripts/pytools/financial-markets/test_config_dir.py` (sostituisce `test_local_dir.py`)
- Modify: `scripts/pytools/financial-markets/server.py` (riga ~19 `import local_dir`, ~33 uso), ogni altro modulo che importa `local_dir` (grep)
- Modify: `scripts/pytools/python-ping/server.py` (accetta e ignora `--config-dir`)
- Modify: `scripts/pytools/README.md`

**Interfaces:**
- Consumes: `--config-dir <dir>` come 2° argomento dopo lo script (Task 4).
- Produces: `config_dir.resolve(argv: list[str] | None = None) -> Path`.

**Venv per i test:** l'esecutore PUÒ creare localmente `scripts/pytools/financial-markets/venv` (gitignored) con `python -m venv venv; venv\Scripts\pip install -r requirements.txt pytest` per eseguire pytest. Non è il venv di deploy (quello resta manuale, `Test Run\pytools\`).

- [ ] **Step 1: Test RED** — `test_config_dir.py`:

```python
from pathlib import Path
import config_dir


def test_flag_with_value():
    assert config_dir.resolve(["server.py", "--config-dir", "D:/cfg"]) == Path("D:/cfg")


def test_flag_with_equals():
    assert config_dir.resolve(["server.py", "--config-dir=D:/cfg"]) == Path("D:/cfg")


def test_default_is_configuration_under_deploy_root():
    # server.py vive in <deploy_root>/pytools/<dominio>/ → default <deploy_root>/Configuration
    expected = Path(config_dir.__file__).resolve().parents[2] / "Configuration"
    assert config_dir.resolve(["server.py"]) == expected


def test_no_environment_variable_is_read(monkeypatch):
    monkeypatch.setenv("LARE_LOCAL_DIR", "D:/should-be-ignored")
    monkeypatch.setenv("LOCALAPPDATA", "D:/should-be-ignored-too")
    assert "should-be-ignored" not in str(config_dir.resolve(["server.py"]))
```

- [ ] **Step 2: RED** — Run (nel venv): `pytest scripts/pytools/financial-markets/test_config_dir.py -q` → `ModuleNotFoundError: config_dir`.

- [ ] **Step 3: Implementazione** — `config_dir.py`:

```python
"""Cartella di configurazione di Lare (2.0): `--config-dir <path>` passato
dall'orchestratore come argomento (dopo il percorso dello script), altrimenti
`<radice del deploy>/Configuration`, dove la radice del deploy è la cartella
che contiene `pytools/` (questo file vive in `pytools/<dominio>/`).

Nessuna variabile d'ambiente viene letta (decisione D6 dello spec 2.0): nella
v1 questo modulo rispecchiava la env var `LARE_LOCAL_DIR` ereditata dal padre,
un terzo lettore indipendente che poteva divergere dagli altri due.
"""

from __future__ import annotations

import sys
from pathlib import Path

FLAG = "--config-dir"


def resolve(argv: list[str] | None = None) -> Path:
    args = sys.argv if argv is None else argv
    it = iter(args)
    for a in it:
        if a == FLAG:
            value = next(it, "")
            if value.strip():
                return Path(value)
        elif a.startswith(FLAG + "="):
            value = a[len(FLAG) + 1:]
            if value.strip():
                return Path(value)
    return Path(__file__).resolve().parents[2] / "Configuration"
```

`server.py` (financial-markets): `import config_dir` e `config_dir.resolve() / "market_data.json"` al posto di `local_dir.resolve()`; grep `local_dir` in tutta la cartella e sostituisci; elimina `local_dir.py` e `test_local_dir.py`. `python-ping/server.py`: nessuna lettura necessaria; aggiungi il commento "accetta `--config-dir` per uniformità, non lo usa" (FastMCP ignora argv sconosciuti — verifica lanciando `python server.py --config-dir X` con stdin chiuso: deve avviarsi senza errore di argomenti).

- [ ] **Step 4: GREEN** — Run: `pytest scripts/pytools/financial-markets -q` → tutti passano (i test che richiedono rete restano marcati come in v1). Run: `grep -rn "LARE_\|LOCALAPPDATA" scripts/pytools --include=*.py` → vuoto.

- [ ] **Step 5: README + commit** — `scripts/pytools/README.md`: sezione "Cartella di configurazione: `--config-dir`". 

```bash
git add scripts/pytools
git commit -m "feat(pytools): --config-dir al posto di LARE_LOCAL_DIR"
```

---

### Task 6: `ui` — `--config-dir`, `startup.json`, un solo risolutore

**Files:**
- Create: `crates/ui/src-tauri/src/config_dir.rs`
- Modify: `crates/ui/src-tauri/Cargo.toml` (dipendenza `startup-config = { path = "../../startup-config" }`)
- Modify: `crates/ui/src-tauri/src/main.rs` (`read_lare_token` ~107-134 → firma nuova; `config_file_path`/`library_dir_path` ~748-776; diagnostica in `.setup()` ~1466-1473; `get_lare_token`; nuovo comando `get_ws_endpoint`)
- Modify: `crates/ui/src-tauri/src/{search_settings.rs:29-38, plugins_view.rs:43-55, market_data_settings.rs:35-44, llm_settings.rs:35-44, aichat_settings.rs:105-115}` — sostituisci ogni risolutore locale con `ConfigDirState`
- Modify: `crates/ui/frontend/ws-client.js` (riga ~21: URL da `get_ws_endpoint` invece di costante) e chi costruisce `LareWsClient` (`app.js` — che diventa `host.js` nel Task 7 — `window.js`, `external-channel-window.js`)
- Modify: `crates/ui/CHANGELOG.md`, `IMPLEMENTATION.md`

**Interfaces:**
- Consumes: `startup_config::{parse_config_dir, resolve_config_dir, exe_dir, StartupConfig}`.
- Produces: `pub struct ConfigDirState { pub config_dir: PathBuf, pub startup: StartupConfig }` gestito da Tauri (`app.state::<ConfigDirState>()`); funzioni pure `pub fn token_path(config_dir: &Path) -> PathBuf`, `config_file_path(config_dir) -> PathBuf` (= `config_dir/config.json`), `library_dir_path(config_dir)` (= `config_dir/library`), `plugins_dir(config_dir, &StartupConfig) -> PathBuf`; comando Tauri `get_ws_endpoint() -> String` (`"ws://127.0.0.1:<ws_port>"`).

- [ ] **Step 1: Test RED** — `config_dir.rs`:

```rust
//! Risoluzione della cartella di configurazione per ui.exe (2.0): UNA sola
//! funzione al posto dei sei risolutori duplicati della v1 (main.rs,
//! search_settings, plugins_view, market_data_settings, llm_settings,
//! aichat_settings), che leggevano LARE_LOCAL_DIR/LOCALAPPDATA ciascuno per
//! conto proprio. Ora: `--config-dir` oppure `<exe_dir>/Configuration`,
//! identico all'orchestratore (stesso crate `startup-config`).

use std::path::{Path, PathBuf};
use startup_config::StartupConfig;

/// Stato gestito da Tauri (`app.manage`): la cartella risolta e lo
/// `startup.json` letto una volta all'avvio.
pub struct ConfigDirState {
    pub config_dir: PathBuf,
    pub startup: StartupConfig,
}

impl ConfigDirState {
    /// Da argv reali + cartella dell'eseguibile. `warn` va loggato dal chiamante.
    pub fn from_process() -> (Self, Option<String>) {
        let config_dir = startup_config::config_dir_from_process();
        let (startup, warn) = StartupConfig::load(&config_dir);
        (Self { config_dir, startup }, warn)
    }
    pub fn ws_endpoint(&self) -> String { format!("ws://127.0.0.1:{}", self.startup.ws_port) }
    pub fn plugins_dir(&self) -> PathBuf { StartupConfig::resolve_path(&self.config_dir, &self.startup.paths.plugins_dir) }
}

pub fn token_path(config_dir: &Path) -> PathBuf { config_dir.join("token") }
pub fn config_file_path(config_dir: &Path) -> PathBuf { config_dir.join("config.json") }
pub fn library_dir_path(config_dir: &Path) -> PathBuf { config_dir.join("library") }
pub fn find_dir_path(config_dir: &Path) -> PathBuf { library_dir_path(config_dir).join("find") }

/// Legge il token dal file; stringa vuota se assente/vuoto (il frontend
/// mostra lo stato "errore" come in v1).
pub fn read_token(config_dir: &Path) -> String {
    std::fs::read_to_string(token_path(config_dir)).map(|s| s.trim().to_string()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paths_are_all_under_config_dir() {
        let d = Path::new("C:/Lare/Configuration");
        assert_eq!(token_path(d), d.join("token"));
        assert_eq!(config_file_path(d), d.join("config.json"));
        assert_eq!(library_dir_path(d), d.join("library"));
        assert_eq!(find_dir_path(d), d.join("library").join("find"));
    }
    #[test]
    fn read_token_trims_and_defaults_to_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_token(dir.path()), "");
        std::fs::write(dir.path().join("token"), " t0k3n \n").unwrap();
        assert_eq!(read_token(dir.path()), "t0k3n");
    }
    #[test]
    fn ws_endpoint_and_plugins_dir_come_from_startup() {
        let mut startup = StartupConfig::default();
        startup.ws_port = 7400;
        let st = ConfigDirState { config_dir: PathBuf::from("C:/Lare/Configuration"), startup };
        assert_eq!(st.ws_endpoint(), "ws://127.0.0.1:7400");
        assert_eq!(st.plugins_dir(), Path::new("C:/Lare").join("plugins"));
    }
}
```
Scrivi PRIMA solo il modulo test (con `use super::*`) e il `mod config_dir;` in `main.rs`; Run: `cargo test -p ui config_dir` → errori di compilazione (RED). Poi il codice sopra → GREEN.

- [ ] **Step 2: Cablaggio in `main.rs`**
  - In `main()`, prima di `tauri::Builder`: `let (cfg_state, warn) = config_dir::ConfigDirState::from_process(); if let Some(w) = warn { eprintln!("[ui] {w}"); } println!("[ui] config dir: {}", cfg_state.config_dir.display());` e `.manage(cfg_state)` sul builder.
  - `read_lare_token(app)` → `config_dir::read_token(&app.state::<ConfigDirState>().config_dir)`; elimina il codice env.
  - `config_file_path(app)`/`library_dir_path(app)` → delegano a `config_dir::*` con la cartella dallo stato (mantieni le firme che `main.rs` usa, cambia il corpo).
  - Diagnostica in `.setup()` (~1466-1473): stampa solo `config dir`.
  - Nuovo comando: `#[tauri::command] fn get_ws_endpoint(state: tauri::State<ConfigDirState>) -> String { state.ws_endpoint() }`, registrato in `generate_handler!`.
  - Nei 5 moduli settings: sostituisci la funzione locale di risoluzione con un parametro `config_dir: &Path` (o `tauri::State<ConfigDirState>` nel comando) — `search-paths.json`, `search-content.json`, `market_data.json`, `llms.json`, `network.json` sono tutti `config_dir.join(<nome>)`; `plugins_view::plugins_dir` → `state.plugins_dir()` (**via** il caso speciale `LARE_PLUGINS_DIR`).
  - `ws-client.js`: `constructor({ token, onStatus, onMessage, channel, url })` con `url` obbligatorio; i chiamanti fanno `const url = await invoke("get_ws_endpoint")` prima di costruirlo. Aggiorna `ws-client.test.mjs` se costruisce il client (passa `url: "ws://127.0.0.1:7331"`).

- [ ] **Step 3: GREEN + grep** — Run: `cargo test -p ui` → passano. Run: `grep -rn 'env::var("LARE_\|LOCALAPPDATA\|APPDATA\|app_local_data_dir\|app_config_dir' crates/ui/src-tauri/src` → vuoto. Run: `node --test crates/ui/frontend/*.test.mjs` → verde. Run: `cargo build -p ui` → `Finished`.

- [ ] **Step 4: Docs + commit** — `crates/ui/CHANGELOG.md` `2.0.1`, IMPLEMENTATION; bump `2.0.1` (Cargo.toml e tauri.conf.json).

```bash
git add crates/ui Cargo.lock
git commit -m "feat(ui): --config-dir e startup.json, un solo risolutore, nessuna env var"
```

---

### Task 7: `ui` — overlay rimosso, pagina host nascosta, flag `--open`

**Files:**
- Create: `crates/ui/frontend/host.html`, `crates/ui/frontend/host.js`
- Delete: `crates/ui/frontend/{index.html, app.js, line-editor.js, line-editor.test.mjs, idle-duck.js, idle-duck.test.mjs, page-scroll.js, page-scroll.test.mjs, cwd-format.js, cwd-format.test.mjs, connection-diagnosis.js, connection-diagnosis.test.mjs}`
- Modify: `crates/ui/src-tauri/tauri.conf.json` (finestra `main`), `crates/ui/src-tauri/capabilities/default.json` (via `global-shortcut:*`), `crates/ui/src-tauri/Cargo.toml` (via `tauri-plugin-global-shortcut`)
- Modify: `crates/ui/src-tauri/src/main.rs` (via plugin global-shortcut e handler ~1357-1410, `apply_hotkey` ~720-737, comandi `hide_overlay`/`show_overlay`/`list_path_completions`/`home_dir`, registrazione hotkey in `.setup()` ~1488-1496; nuovo flag `--open`)
- Modify: `crates/ui/src-tauri/src/config.rs` (`Config`: via `action_key`, `cursor_color`, `cursor_font`, `cursor_size`, `position`, `activity_indicator`, `idle_duck_minutes`; restano `web_search_enabled`, `window_alpha`), `crates/ui/frontend/config-dialog.js` (tab UI: via i campi corrispondenti, righe ~158-163, ~375-382, ~443-473)
- Modify: `crates/ui/CHANGELOG.md`, `IMPLEMENTATION.md`

**Interfaces:**
- Consumes: `LareWsClient` (ws-client.js), `get_ws_endpoint`, `get_lare_token`, i moduli di relay `search-buffer.js`, `aichat-push.js`, `external-channels.js` (invariati).
- Produces: `host.js` = connessione WS di default + `handleServerMsg` (solo i casi che aprono/aggiornano finestre o fanno relay) + `open*Window`/`emitTo*`/`setup*Events` estratti da `app.js`; `ui.exe --open config|library` (dev-only).

- [ ] **Step 1: Test RED (JS)** — `crates/ui/frontend/host-dispatch.test.mjs` per la logica pura estratta in `host-dispatch.mjs`:

```js
import { test } from "node:test";
import assert from "node:assert/strict";
import { classifyServerMsg } from "./host-dispatch.mjs";

test("messaggi che aprono finestre sono classificati window", () => {
  for (const type of ["open_window", "search_open", "open_plugin_window", "update_plugin_window", "close_plugin_window", "routine_save_preview"]) {
    assert.equal(classifyServerMsg({ type }), "window");
  }
});
test("messaggi AI Chat / note / share sono relay", () => {
  for (const type of ["ai_chat_message", "ai_chat_roster", "notes_snapshot", "note_upserted", "share_request", "share_result", "share_content_request", "share_incoming_data", "ai_chat_join_prompt"]) {
    assert.equal(classifyServerMsg({ type }), "relay");
  }
});
test("messaggi del cursore v1 senza superficie sono ignored", () => {
  for (const type of ["chunk", "heartbeat", "done", "error", "pong", "cwd", "server_info"]) {
    assert.equal(classifyServerMsg({ type }), "ignored");
  }
});
test("tool_confirm_request senza cursore viene negato (safe default)", () => {
  assert.equal(classifyServerMsg({ type: "tool_confirm_request" }), "deny");
});
```

Run: `node --test crates/ui/frontend/host-dispatch.test.mjs` → fallisce (modulo assente).

- [ ] **Step 2: `host-dispatch.mjs`**

```js
// Classificazione pura dei ServerMsg per la pagina host (nessun DOM, testabile
// con node:test). "window": apre/aggiorna una finestra; "relay": va rigirato a
// una finestra già aperta (AI Chat, Library/note, share); "ignored": era per il
// cursore v1, che non esiste più; "deny": richiesta di conferma tool arrivata
// sulla connessione di default — senza cursore nessuno può rispondere, e la
// risposta sicura è NO (l'orchestratore ha comunque un timeout che nega).
const WINDOW = new Set(["open_window", "search_open", "search_hit", "search_done", "open_screener_picker", "open_plugin_window", "update_plugin_window", "close_plugin_window", "routine_save_preview"]);
const RELAY = new Set(["ai_chat_message", "ai_chat_roster", "ai_chat_reachable_peers", "ai_chat_history", "ai_chat_self", "ai_chat_join_request", "ai_chat_peer_lost", "ai_chat_join_prompt", "ai_chat_admission_request", "ai_chat_admission_resolved", "ai_chat_pending", "ai_chat_admitted", "ai_chat_rejected", "notes_snapshot", "note_upserted", "share_request", "share_result", "share_content_request", "share_incoming_data"]);

export function classifyServerMsg(msg) {
  if (msg.type === "tool_confirm_request") return "deny";
  if (WINDOW.has(msg.type)) return "window";
  if (RELAY.has(msg.type)) return "relay";
  return "ignored";
}
```

- [ ] **Step 3: `host.js`** — costruiscilo **estraendo** da `app.js` (non riscrivendo) queste funzioni, invariate salvo i riferimenti al DOM del cursore che vanno rimossi: `uuidv4`, `invokeCmd`, `getToken`, `initClient` (con `url: await invokeCmd("get_ws_endpoint")`), `handleServerMsg` (solo i `case` dei tipi `window`/`relay` di `host-dispatch.mjs`; per `tool_confirm_request` chiama `client.sendToolConfirmResponse(msg.id, false)`; tutto il resto → `return`), `openMarkdownWindow`, `openSearchWindow`, `emitToSearch`, `setupSearchEvents`, `openPluginWindow`, `openRoutinePreviewWindow`, `emitToPlugin`, `emitToLibrary`, `libraryRosterParticipants`, `openAiChatWindow`, `openNoteWindow`, `setupNoteWindowEvents`, `emitToAiChat`, `pushAiChat`, `setupAiChatEvents`, `setupPluginEvents`, `setupRoutinePreviewEvents`, `setupConfigWindowEvents`, `setupLibraryEvents`, `bootstrap` (senza editor/focus/duck/diagnosi). Importa `search-buffer.js`, `aichat-push.js`, `external-channels.js`, `share-view.mjs`, `path-utils.js` come faceva `app.js`. In più, in `bootstrap`: `const open = await invokeCmd("dev_open_request"); if (open === "config") invokeCmd("open_config_window"); if (open === "library") invokeCmd("open_library_window");`.

  `host.html`:
  ```html
  <!doctype html>
  <html lang="it"><head><meta charset="utf-8"><title>Lare host</title></head>
  <body><!-- pagina host nascosta: nessuna UI. Tiene la connessione WS di default,
       apre le finestre e fa da relay (vedi spec §5). --><script type="module" src="host.js"></script></body></html>
  ```

- [ ] **Step 4: Rust** — in `main.rs`: rimuovi `.plugin(tauri_plugin_global_shortcut…)` e l'handler (~1357-1410), `apply_hotkey`, i comandi `hide_overlay`, `show_overlay`, `list_path_completions`, `home_dir` (e le loro voci in `generate_handler!`), la registrazione in `.setup()` (~1488-1496), `use tauri_plugin_global_shortcut::*`. Aggiungi:

```rust
/// Flag di sviluppo TEMPORANEO (piano 1 → rimosso nel piano 3): `--open config|library`
/// apre subito quella finestra, perché senza overlay né canale shell nessuna
/// superficie può chiederlo. Letto una volta da argv.
pub struct DevOpenRequest(pub Option<String>);

#[tauri::command]
fn dev_open_request(state: tauri::State<DevOpenRequest>) -> Option<String> { state.0.clone() }
```
in `main()`: `let args: Vec<String> = std::env::args().collect(); let open = args.iter().position(|a| a == "--open").and_then(|i| args.get(i + 1)).filter(|v| *v == "config" || *v == "library").cloned(); … .manage(DevOpenRequest(open))` e registra `dev_open_request`. `tauri.conf.json` finestra `main`: `"url": "host.html"`, `"visible": false`, `"skipTaskbar": true`, `"transparent": false`, `"decorations": true`, `"alwaysOnTop": false`, `"width": 200, "height": 100`. `capabilities/default.json`: togli le righe `global-shortcut:*`; `Cargo.toml`: togli `tauri-plugin-global-shortcut`.

- [ ] **Step 5: `Config`** — `config.rs`: struct con soli `web_search_enabled` e `window_alpha` (default e `#[serde(default)]` come oggi); rimuovi `Position`, `ActivityIndicator` e i loro test; aggiungi test:
```rust
#[test]
fn legacy_v1_config_json_with_extra_fields_still_loads() {
    let json = r#"{"action_key":"F4","cursor_color":"#FFF","window_alpha":0.5,"web_search_enabled":false}"#;
    let cfg: Config = serde_json::from_str(json).unwrap(); // i campi ignoti vengono ignorati
    assert_eq!(cfg.window_alpha, 0.5);
    assert!(!cfg.web_search_enabled);
}
```
`config-dialog.js`: rimuovi i campi corrispondenti dalla tab UI (righe ~158-163 default, ~375-382 salvataggio, ~443-473 costruzione) lasciando alpha e ricerca web; `applyAppearance` non esiste più (era del cursore).

- [ ] **Step 6: GREEN + verifiche**

Run: `node --test crates/ui/frontend/*.test.mjs` → verde (attesi 269 − 5 file rimossi + 4 nuovi). Run: `cargo test -p ui` → verde. Run: `cargo build -p ui` → `Finished`. Run: `grep -rn "global_shortcut\|apply_hotkey\|hide_overlay\|action_key\|cursor_color" crates/ui/src-tauri/src crates/ui/frontend` → vuoto. Verifica manuale minima (il supervisore la ripete): con l'orchestratore in esecuzione da `Test Run\Configuration` (Task 8 lo prepara; in attesa, una tempdir con `startup.json` vuoto), `cargo run -p ui -- --config-dir <dir> --open config` → si apre `/config` con la sola tab alpha/ricerca web; `--open library` → Library; nessuna finestra overlay; nessuna hotkey registrata.

- [ ] **Step 7: Docs + commit** — CHANGELOG `2.0.2 — overlay F2 rimosso, pagina host nascosta, flag --open (dev)`; IMPLEMENTATION: sezione "Pagina host" con l'elenco di ciò che è stato estratto da `app.js` e ciò che è stato rimosso.

```bash
git add -A crates/ui
git commit -m "feat(ui): overlay F2 rimosso, pagina host nascosta con WS e relay, flag dev --open"
```

---

### Task 8: `Test Run\` e `deploy_test_run.ps1`

**Files:**
- Create: `Test Run/Configuration/startup.json`, `Test Run/Configuration/README.md`
- Create: `Test Run/init_orchestrator.ps1`, `Test Run/init_tauri.ps1`
- Create: `Test Run/plugins/ping/plugin.json`, `Test Run/plugins/calc/plugin.json` (copia dei manifest da `crates/plugin-ping/plugin.json`, `crates/plugin-calc/plugin.json`)
- Create: `Test Run/pytools/README.md` (rimando a `scripts/pytools/README.md`; il deploy script copia gli script)
- Create: `deploy_test_run.ps1`
- Create: `Test Run/Configuration/.gitkeep` per `routines/`, `library/documents/`, `library/find/`, `logs/`

**Interfaces:**
- Consumes: i binari di Task 3-7; `.gitignore` già esclude exe/DLL/segreti sotto `Test Run\`.
- Produces: `.\deploy_test_run.ps1 [-BuildConfig debug|release] [-IncludePlugins]` → `Test Run\` pronta; `Test Run\init_*.ps1` avviano senza env var.

- [ ] **Step 1: `Test Run/Configuration/startup.json`** (template, nessun segreto)

```json
{
  "ws_port": 7331,
  "paths": {
    "shell": "shell/lare-shell.exe",
    "mcp_server": "mcp-server.exe",
    "mcp_nmap": "mcp-nmap.exe",
    "plugins_dir": "plugins",
    "pytools_dir": "pytools",
    "routines_dir": "Configuration/routines"
  },
  "ai_model": "claude-sonnet-4-6",
  "autostart": { "orchestrator": true, "ui": true },
  "log": { "level": "info", "dir": "Configuration/logs" }
}
```

- [ ] **Step 2: `init_orchestrator.ps1`**
```powershell
# Avvio manuale dell'orchestratore da questa cartella di deploy. Nessuna
# variabile d'ambiente: la configurazione è in .\Configuration (default
# accanto all'eseguibile). --console-log: log anche in questa console.
& "$PSScriptRoot\orchestrator.exe" --console-log
```
`init_tauri.ps1`: `& "$PSScriptRoot\ui.exe"`.

- [ ] **Step 3: `deploy_test_run.ps1`** (root del repo)
```powershell
<#
.SYNOPSIS  Copia i binari compilati in "Test Run\" (layout di deploy dentro il repo).
.DESCRIPTION
  target\<BuildConfig>\ -> Test Run\ : orchestrator.exe, mcp-server.exe, mcp-nmap.exe, ui.exe;
  scripts\pytools\ -> Test Run\pytools\ (mai venv/cache);
  con -IncludePlugins: target\<BuildConfig>\<id>.exe -> Test Run\plugins\<id>\<id>.exe
  (i plugin.json sono già committati). Non tocca Test Run\Configuration.
#>
param(
    [ValidateSet("debug", "release")] [string]$BuildConfig = "debug",
    [switch]$IncludePlugins
)
$ErrorActionPreference = "Stop"
$Repo = $PSScriptRoot
$Target = Join-Path $Repo "target\$BuildConfig"
$Dest = Join-Path $Repo "Test Run"
if (-not (Test-Path $Target)) { throw "Manca $Target: compila prima (cargo build; cargo build -p ui)." }
foreach ($exe in "orchestrator.exe", "mcp-server.exe", "mcp-nmap.exe", "ui.exe") {
    $src = Join-Path $Target $exe
    if (-not (Test-Path $src)) { throw "Manca $src" }
    Copy-Item $src (Join-Path $Dest $exe) -Force
    Write-Host "copiato $exe"
}
robocopy (Join-Path $Repo "scripts\pytools") (Join-Path $Dest "pytools") /E /XD venv __pycache__ .pytest_cache /XF tickers_us.json fundamentals_cache.json technical_cache.json discoveries.json /NFL /NDL /NJH /NJS | Out-Null
Write-Host "copiato pytools\ (senza venv)"
if ($IncludePlugins) {
    foreach ($id in "ping", "calc") {
        $src = Join-Path $Target "$id.exe"
        if (Test-Path $src) { Copy-Item $src (Join-Path $Dest "plugins\$id\$id.exe") -Force; Write-Host "copiato plugin $id" }
        else { Write-Warning "plugin $id non compilato ($src): saltato" }
    }
}
Write-Host "Test Run pronta: $Dest"
```

- [ ] **Step 4: Verifica dal vivo** (il supervisore la ripete)

Run: `cargo build; cargo build -p ui; cargo build -p plugin-ping -p plugin-calc; .\deploy_test_run.ps1 -IncludePlugins`. Poi in un terminale `& ".\Test Run\init_orchestrator.ps1"` → log: `config dir: …\Test Run\Configuration`, `[token] primo avvio: token creato in …\Configuration\token`, plugin scoperti: 2; file `Test Run\Configuration\logs\orchestrator.log.<data>` creato. In un secondo terminale `& ".\Test Run\ui.exe" --open library` → si apre la Library (vuota), nessun overlay; `git status` non mostra `token`/exe (gitignore). Chiudi tutto: `taskkill //F //IM orchestrator.exe //IM ui.exe`.

- [ ] **Step 5: Commit**
```bash
git add "Test Run" deploy_test_run.ps1
git commit -m "chore: Test Run (layout di deploy con Configuration unica) e deploy_test_run.ps1"
```

---

### Task 9: Documentazione 2.0 e checklist e2e del piano 1

**Files:**
- Create: `Docs/i18n/ita/RUN-LOCAL.md`, `Docs/i18n/ita/DEPLOY.md`, `Docs/i18n/ita/TESTING-e2e.md`, `Docs/i18n/ita/KNOWN-ISSUES.md`
- Create: `Docs/i18n/ita/06-decisions.md` (copia di `..\Lare Terminal\Docs\06-decisions.md` + ADR nuove)
- Modify: `Docs/i18n/ita/HANDOFF.md`, `README.md` (root)

- [ ] **Step 1: `06-decisions.md`** — copia il log ADR v1 (ADR-001..014) intatto, poi aggiungi:

```markdown
## ADR-015 — Lato shell: host custom del motore PowerShell in C# (2026-09-04)
Contesto: la 2.0 vuole i comandi `/…` nella riga di comando di una sessione PowerShell e i comandi
dell'AI eseguiti nella shell dell'utente. Decisione: `lare-shell` è una host custom del motore
PowerShell (come `pwsh.exe`), non un hook su una shell altrui. Provata dallo spike
`spikes/2026-09-05-lare-shell-host.md`. Conseguenze: componente C#; Linux/macOS pwsh-flavored.

## ADR-016 — Lare Terminal è una finestra Tauri con xterm.js + ConPTY (2026-09-05)
Decisione: la host gira dentro una finestra Tauri con emulatore xterm.js; barre e segnalini in HTML.
Provata dallo spike `spikes/2026-09-05-lare-terminal-window.md`. L'overlay F2 della v1 muore;
`ui.exe` resta host di finestre con una pagina host nascosta al posto del cursore.

## ADR-017 — Configurazione: una cartella, nessuna variabile d'ambiente (2026-09-04)
Decisione: `--config-dir` o `<exe>\Configuration\`; percorsi relativi alla radice del deploy;
i figli ricevono `--config-dir` per argomento. Motivo: nella v1 tre lettori indipendenti della
stessa env var divergevano in silenzio (`llms_config.rs:119-125` v1).
```

- [ ] **Step 2: `RUN-LOCAL.md`** — sviluppo: build (`cargo build`, `cargo build -p ui`, plugin), `deploy_test_run.ps1`, avvio con `--config-dir "Test Run\Configuration" --console-log`, `--open` dev, test (Rust, JS, Python con venv locale), gotcha (`taskkill`, `cargo clean -p ui`). **`DEPLOY.md`** — copiare `Test Run\` altrove; prerequisiti (Windows x64, WebView2; pwsh 7.6+ e .NET 10 runtime arrivano col piano 2, annotarli come "dal piano 2"); venv pytools a mano; `Configuration\` contenuto e cosa è segreto. **`TESTING-e2e.md`** — checklist del piano 1 = Step 4 del Task 8 più: `ui.exe` senza orchestratore → nessun crash, log "orchestratore non raggiungibile"; copia di `Test Run\` in un'altra cartella e riavvio senza modifiche. **`KNOWN-ISSUES.md`** — riporta dalla v1 il KNOWN-ISSUE codepage (ancora vivo) e il reconnect infinito su canale esterno rotto.

- [ ] **Step 3: `HANDOFF.md`** — versioni correnti (tutte `2.0.x`), FATTO: piano 1 task 0-9 con date, DA FARE: piano 2 e 3. **`README.md`** root: sezione "Stato" → "piano 1 completato: fondamenta; prossimo piano 2".

- [ ] **Step 4: Commit di release**
```bash
git add Docs README.md
git commit -m "release: piano 1 completato — fondamenta 2.0 (config unica, ui host di finestre, Test Run)"
```
(l'hook richiede `Docs/i18n/ita/HANDOFF.md` in stage: c'è.)

---

## Self-review del piano (eseguita alla scrittura)

- **Copertura spec**: D2 (Task 7), D6 (Task 2-6), D7 (Task 8), D8 (Task 1), D10 (Task 1: tutti i crate compilano; le superfici non MVP non vengono verificate), §5 pagina host e flag `--open` (Task 7), §6.1-6.4 (Task 2-6: log su file e `--console-log` in Task 4; autostart **non** in questo piano — richiede `ui.exe` che avvia l'orchestratore: rimandato al piano 3 dove `ui.exe` diventa l'app; annotato in HANDOFF), §7 (Task 8: senza `shell\`, che arriva col piano 2), §11 (Task 0). §3.2 finestra Markdown di output e §4 protocollo: piano 2.
- **Placeholder**: nessun TBD; i punti "grep `local_dir` e sostituisci" sono istruzioni meccaniche verificabili col grep finale del task.
- **Coerenza nomi**: `config_dir_from_process`, `StartupConfig::load` → `(cfg, Option<String>)`, `StartupConfig::resolve_path(config_dir, value)`, `has_flag`, `CONFIG_DIR_FLAG` usati identici in Task 3, 4, 5 (Python: `config_dir.resolve`), 6 (`ConfigDirState`), 8 (`--console-log`).
