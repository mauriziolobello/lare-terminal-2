# RUN-LOCAL — sviluppo locale di Lare Terminal 2.0

> Stato a fine piano 1 ("fondamenta"): esistono `orchestrator`, `mcp-server`, `mcp-nmap`, `ui`,
> i plugin (`plugin-ping`, `plugin-counter`, `plugin-calc`, `plugin-lc`, `plugin-crypto`) e gli
> script Python in `scripts/pytools/`. **`shell\` e `lare-shell.exe` NON esistono ancora** —
> arrivano col piano 2 (protocollo + host C#). Fino ad allora `ui.exe` non ha un canale per
> aprire finestre da solo: si usa il flag di sviluppo `--open config|library` (vedi sotto).

## Build

Dalla radice del repo:

```powershell
cargo build                                   # solo i crate backend (default-members del workspace)
cargo build -p ui                             # UI Tauri (~400 crate esterni, build lenta la prima volta)
cargo build -p plugin-ping -p plugin-calc      # plugin usati da Test Run\ (vedi sotto)
```

`cargo build` (senza `-p`) compila solo i "default-members" del workspace — `orchestrator`,
`mcp-server`, `mcp-nmap`, `startup-config`, `protocol`, i plugin — non `ui` (troppo lento per
un ciclo di sviluppo rapido sul backend): va compilata a parte con `-p ui`.

## Deploy locale — `Test Run\`

`Test Run\` (nel repo) è lo specchio del layout di deploy: binari, `Configuration\`, plugin e
`pytools\`, tutti nella stessa cartella (§6-7 dello spec). Dopo una build:

```powershell
.\deploy_test_run.ps1                          # copia i 4 exe Rust + scripts\pytools\ (senza venv)
.\deploy_test_run.ps1 -IncludePlugins           # come sopra, più ping.exe/calc.exe in plugins\<id>\
.\deploy_test_run.ps1 -BuildConfig release      # da target\release\ invece di target\debug\
```

Non tocca mai `Test Run\Configuration\` (i file lì dentro — token, config generate, log — sono
gitignored e sopravvivono a ogni deploy). Lo script termina con `$LASTEXITCODE` diverso da 0
anche a successo (debito noto, `robocopy` usa 1 per "file copiati" — vedi `HANDOFF.md`): non è
un segnale di errore, guarda l'output (`Test Run pronta: ...` sull'ultima riga).

## Avvio in sviluppo (due terminali)

Stessa configurazione del deploy (`Test Run\Configuration`), ma eseguendo i binari da
`target\debug\` invece che da `Test Run\` — per questo **serve** passare `--config-dir`
esplicito (senza il flag, la regola di default userebbe `target\debug\Configuration\`, che non
esiste): è la stessa regola unica di risoluzione (D6) applicata a due layout diversi.

```powershell
# Terminale 1
cargo run -p orchestrator -- --config-dir "Test Run\Configuration" --console-log

# Terminale 2
cargo run -p ui -- --config-dir "Test Run\Configuration"
```

`--console-log` fa scrivere l'orchestrator anche su questa console, oltre che sul file di log
giornaliero in `Test Run\Configuration\logs\` (senza il flag, solo file — pensato per
l'autostart, dove nessuno guarda un terminale). Se lanci invece i binari già in `Test Run\` (con
`.\Test Run\init_orchestrator.ps1` / `.\Test Run\init_tauri.ps1`), **non serve** `--config-dir`:
gli exe vivono già accanto a `Configuration\`, che è il default.

### Flag di sviluppo `--open` (temporaneo, sparisce nel piano 3)

Senza `lare-shell` non c'è ancora modo per l'utente di chiedere l'apertura di una finestra —
`ui.exe --open config` o `ui.exe --open library` la apre subito, per poter verificare le finestre
a mano:

```powershell
cargo run -p ui -- --config-dir "Test Run\Configuration" --open library
cargo run -p ui -- --config-dir "Test Run\Configuration" --open config
```

Un valore diverso da `config`/`library` (o l'assenza del flag) non apre nulla — comportamento
identico a prima dell'introduzione del flag.

## Test

```powershell
cargo test                                    # test dei crate backend (default-members)
cargo test -p orchestrator <substring>        # un singolo test (o un sottoinsieme per nome)
cargo test -p orchestrator -- --ignored       # test di integrazione reale (API esterne, binari, venv)
node --test crates/ui/frontend/*.test.mjs     # test JS puri del frontend (219 test, nessuna webview)
cargo clippy --all-targets                    # lint su tutti i target del workspace
cargo fmt --check                             # verifica formattazione (v1 non è fmt-clean: differenze note)
```

**Python (`scripts/pytools/`)**: ogni dominio (`financial-markets/`, `python-ping/`) ha un venv
locale creato a mano, mai committato:

```powershell
cd scripts/pytools/financial-markets
python -m venv venv
venv\Scripts\Activate.ps1
pip install -r requirements.txt pytest
pytest -q                                     # dalla cartella del dominio, o:
pytest scripts/pytools/financial-markets -q   # dalla radice del repo, venv già attivo
```

`python-ping/` non ha un venv proprio (nessuna dipendenza di dominio): per un fumo-test manuale
si può riusare l'interprete del venv di `financial-markets`.

## Gotcha (Windows)

- **`Accesso negato (os error 5)` durante una build** = un binario è ancora in esecuzione.
  Chiudilo prima di ricompilare:
  ```powershell
  taskkill /F /IM orchestrator.exe /IM ui.exe
  ```
  (in Git Bash, gli stessi flag vanno raddoppiati: `taskkill //F //IM orchestrator.exe //IM ui.exe`).
- **Riavvia sempre l'orchestrator dopo una build del backend** — non si auto-ricarica.
- **Tauri (`ui`) non ricompila dopo una modifica al solo frontend**: `generate_context!` incorpora
  `frontendDist` a compile time. Se `cargo build -p ui`/`cargo run -p ui` sembra ignorare un
  cambiamento in `crates/ui/frontend/`, forza la ricompilazione:
  ```powershell
  cargo clean -p ui
  ```
- **`ANTHROPIC_API_KEY`** resta una variabile d'ambiente (fuori dallo scope di D6, che riguarda
  la *posizione* della configurazione Lare, non le credenziali dei provider AI): senza di essa
  l'orchestrator logga `ANTHROPIC_API_KEY non impostata — uso StubAdapter` e risponde con un
  adapter finto invece di chiamare Claude. In alternativa, un `llms.json` in `Configuration\`
  (mai committato) seleziona un provider diverso (anche via OpenRouter) — vedi `DEPLOY.md`.
