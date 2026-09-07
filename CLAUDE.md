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
(src + test xUnit; ADR-015/019 — verificabile in modalità B, profilo Windows Terminal "Lare
Terminal"). Tool Python in `scripts/pytools/<dominio>/` (un venv per dominio, creato a mano).

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
dotnet test shell/lare-shell/LareShell.sln    # test della host C# (115)

# Avvio in sviluppo (due terminali), stessa config del deploy:
cargo run -p orchestrator -- --config-dir "Test Run\Configuration" --console-log
cargo run -p ui -- --config-dir "Test Run\Configuration"
node scripts/dev/shell-client.mjs -- '/ping'   # canale shell senza console interattiva (dev/CI)

.\deploy_test_run.ps1                         # popola Test Run\ (pubblica anche lare-shell; -SkipShell per saltarla)
```

Gotcha Windows: build che fallisce con `Accesso negato (os error 5)` = processo in esecuzione →
`taskkill //F //IM orchestrator.exe //IM ui.exe //IM lare-shell.exe`. **Riavvia sempre
l'orchestratore dopo una build del backend.** Tauri: `generate_context!` incorpora `frontendDist`
a compile time — dopo modifiche al solo frontend, `cargo clean -p ui` se la build non ricompila.
Host C#: i `.csproj` dichiarano `<RuntimeIdentifier>win-x64</RuntimeIdentifier>` (necessario perché
`$PSHOME` risolva nella cartella dell'exe, ADR-019) — l'output di build sta quindi sotto
`bin\Debug\net10.0\win-x64\`, non `bin\Debug\net10.0\`.

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
