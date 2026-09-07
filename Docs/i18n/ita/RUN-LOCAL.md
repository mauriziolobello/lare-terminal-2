# RUN-LOCAL — sviluppo locale di Lare Terminal 2.0

> Stato a fine piano 3 ("finestra terminale"): esistono `orchestrator`, `mcp-server`,
> `mcp-nmap`, `ui`, i plugin (`plugin-ping`, `plugin-counter`, `plugin-calc`, `plugin-lc`,
> `plugin-crypto`), gli script Python in `scripts/pytools/` **e** `shell/lare-shell/`, la host C#
> del motore PowerShell (ADR-015). Esistono ora **entrambe** le modalità dello spec §2.3:
> **modalità A** (`ui.exe`, avviato senza `--no-terminal`, apre di default una finestra terminale
> con `lare-shell.exe` dentro ConPTY, ADR-016 — §"Modalità A" sotto) e **modalità B** (profilo
> Windows Terminal "Lare Terminal", `lare-shell.exe` nudo, §"Host C#" sotto). Il flag di sviluppo
> `--open config|library` è sparito col piano 3 (sostituito dal canale shell reale, ora
> disponibile in entrambe le modalità); `scripts/dev/shell-client.mjs` resta comunque utile per
> parlare il canale shell senza una console interattiva vera (es. da Claude Code, dove lo stdin è
> rediretto — vedi il gotcha in fondo a questa sezione).

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

### Modalità A (finestra terminale)

`ui.exe`, avviato SENZA `--no-terminal`, apre la finestra terminale all'avvio: xterm.js dentro
ConPTY, `lare-shell.exe` come processo figlio (risolto da `startup.json` → `paths.shell`).

**Avvio da sorgente:**

```powershell
cargo run -p ui -- --config-dir "Test Run\Configuration"
```

Se `Test Run\shell\lare-shell.exe` non esiste, esegui prima `.\deploy_test_run.ps1` (pubblica anche
la host C#). Chiudere la finestra termina il processo `lare-shell.exe` figlio — nessun processo
residuo (verificabile in Task Manager).

**Modalità B** (`ui.exe --no-terminal`, avviato dalla host C# o dall'orchestratore): apre solo la
host page nascosta, mai la finestra terminale — usata quando la shell "vera" è già
`lare-shell.exe` in una scheda Windows Terminal (profilo "Lare Terminal", vedi sezione "Host C#"
sopra).

### Canale shell senza la host (client di sviluppo)

`lare-shell.exe` (host C#, piano 2b) esiste ora — vedi §"Host C#" sotto per costruirla e avviarla
davvero — ma resta comodo parlare il canale shell **senza** una console interattiva vera (una
finestra Windows Terminal): è il caso di uno strumento come Claude Code, il cui terminale ha lo
stdin rediretto (niente `Console.ReadKey`/`Console.CancelKeyPress` utilizzabili, gate a rischio di
bloccarsi). `scripts/dev/shell-client.mjs` imita quel lato (Node ≥ 22, `WebSocket` globale, nessuna
dipendenza): manda `Hello{role:"shell"}`, UN `Command`, poi risponde ai messaggi dell'orchestratore
come farebbe la host — `[Y/n]` su stdin per il gate, esegue con `pwsh` per `exec_in_shell`
(`capture` sceglie stdio ereditata o catturata) e manda `exec_result`.

```powershell
# Con orchestrator e ui.exe già avviati (vedi "Avvio in sviluppo" sopra):
node scripts/dev/shell-client.mjs -- '/ping'          # finestra su ui.exe con la tabella; qui "→ finestra … aperta"
node scripts/dev/shell-client.mjs -- '/nonesiste'     # solo "[done exit_code=0]" (slash scartato)
node scripts/dev/shell-client.mjs -- '/ai ciao'       # [error routing_error] sintassi: /ai "testo" (virgolette obbligatorie)
node scripts/dev/shell-client.mjs -- '/config'        # si apre /config su ui.exe
node scripts/dev/shell-client.mjs -- '/ai "elenca i 3 file più grandi qui"'   # [Y/n] → esegue → finestra
```

`--config-dir`/`--session` accettano lo stesso valore di default degli altri binari (`Test Run\
Configuration`, un id generato dal pid): `node scripts/dev/shell-client.mjs --config-dir "Test
Run/Configuration" --session s1 -- '/ping'`. `--selftest` verifica `exec_in_shell` in locale,
senza orchestratore (utile per controllare a occhio che il marcatore interno di cwd non finisca
mai sulla console reale con `capture:false`).

**Gotcha (Git Bash/MSYS)**: da Git Bash, MSYS riscrive un argomento che sembra un path assoluto
Unix — `/ping` diventa `C:/Program Files/Git/ping`, `/config` una cosa simile — prima che Node lo
veda. Lancia il client da **PowerShell** (come negli esempi sopra), oppure, se devi restare in
Git Bash, disattiva la riscrittura per quel comando: `MSYS_NO_PATHCONV=1 node scripts/dev/shell-client.mjs -- '/ping'`.

## Host C# (`shell/lare-shell/`)

La host custom del motore PowerShell (ADR-015): una soluzione .NET separata dal workspace Cargo,
in `shell/lare-shell/LareShell.sln` (`src/LareShell` l'eseguibile, `tests/LareShell.Tests` gli
xUnit). Dettagli implementativi: `shell/lare-shell/IMPLEMENTATION.md`.

```powershell
dotnet build shell/lare-shell/LareShell.sln
dotnet test shell/lare-shell/LareShell.sln                                      # 118 test
dotnet test shell/lare-shell/LareShell.sln --filter "FullyQualifiedName~Executor"   # un solo file di test
```

**Avvio da sorgente** (senza publish): a differenza dei binari Rust, da sorgente NON esiste una
`Configuration\` accanto all'output di build (`bin\Debug\net10.0\win-x64\` — vedi il gotcha RID
sotto) — **serve sempre** `--config-dir` esplicito, la regola unica di risoluzione (D6) non ha un
default utile qui:

```powershell
dotnet run --project shell/lare-shell/src/LareShell -- --config-dir "Test Run\Configuration"
```

**Modalità B** (verificabile oggi, profilo Windows Terminal "Lare Terminal", spec §2.3): serve un
publish vero, non `dotnet run` (il fragment WT punta a un `.exe`) — `.\deploy_test_run.ps1` (di
default pubblica anche `lare-shell` in `Test Run\shell\`; `-SkipShell` per saltarlo) poi
`.\Test Run\install-wt-profile.ps1` — vedi `DEPLOY.md` §Passi per il dettaglio e il riavvio di
Windows Terminal richiesto dopo l'installazione del fragment.

**Gotcha: RID esplicito nell'output di build.** `LareShell.csproj`/`LareShell.Tests.csproj`
dichiarano `<RuntimeIdentifier>win-x64</RuntimeIdentifier>` (ADR-019 punto 1: senza un RID
esplicito `$PSHOME` risolve dentro `runtimes\win\lib\net10.0\` invece che nella cartella
dell'eseguibile, e `powershell.config.json` accanto all'exe non verrebbe letto). Conseguenza
visibile: l'output di `dotnet build`/`dotnet run` sta sotto `bin\Debug\net10.0\win-x64\` (non
`bin\Debug\net10.0\` come un progetto .NET senza RID) — utile saperlo se cerchi l'exe a mano o
scrivi uno script che lo referenzia.

**Gotcha: terminale con stdin rediretto (es. Claude Code) → niente PSReadLine, niente gate
interattivo.** `lare-shell` distingue "PSReadLine disponibile ma non in uso" (stdin rediretto,
fallback a `Console.ReadLine`) da "PSReadLine non disponibile" (banner diverso, `Repl.Banner`); il
gate `[Y/n]` (`ConsoleGate`) con stdin rediretto legge una riga di testo e **fallisce chiuso** su
EOF (nessuna tastiera vera = nessuno a cui chiedere, il gate ADR-007 in dubbio rifiuta). Per un
e2e reale (PSReadLine, prompt `[Y/n]` a tasto, Ctrl+C) serve una console interattiva vera:
**Windows Terminal**, non il terminale di uno strumento come Claude Code — vedi `TESTING-e2e.md`
Parte 6.

**Gotcha: `taskkill` per `dotnet build`.** Come per i binari Rust, un `lare-shell.exe` ancora in
esecuzione fa fallire la build con "Accesso negato": `taskkill /F /IM lare-shell.exe` (Git Bash:
`taskkill //F //IM lare-shell.exe`) prima di ricompilare.

## Test

```powershell
cargo test                                    # test dei crate backend (default-members)
cargo test -p orchestrator <substring>        # un singolo test (o un sottoinsieme per nome)
cargo test -p orchestrator -- --ignored       # test di integrazione reale (API esterne, binari, venv)
node --test crates/ui/frontend/*.test.mjs     # test JS puri del frontend (231 test, nessuna webview)
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
