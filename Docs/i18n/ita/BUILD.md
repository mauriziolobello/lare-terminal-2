# BUILD — compilare Lare Terminal 2.0

Solo compilazione e test. Per popolare una cartella eseguibile (`Test Run\` o altrove) vedi
`DEPLOY.md`; per avviare il programma una volta compilato/deployato vedi `RUN.md`.

Il progetto ha due toolchain indipendenti, compilate separatamente:

- **Rust** (workspace Cargo): `orchestrator`, `mcp-server`, `mcp-nmap`, `ui`, i plugin
  (`plugin-ping`, `plugin-calc`, …), `startup-config`, `protocol`.
- **.NET** (soluzione separata, non nel workspace Cargo): `shell/lare-shell/` — la host C# del
  motore PowerShell (ADR-015).

## Tutto in un colpo — `build.ps1`

Per compilare tutto quello che serve a un deploy vero (Rust + host C#) senza ricordare i comandi
uno per uno:

```powershell
.\build.ps1                                    # debug: default-members + ui + lare-shell
.\build.ps1 -BuildConfig release               # come sopra, in release
.\build.ps1 -IncludePlugins                    # aggiunge plugin-ping/plugin-calc
.\build.ps1 -SkipUi                            # salta ui (ciclo rapido sul solo backend)
.\build.ps1 -SkipShell                         # salta la host C# (~1 min risparmiato)
.\build.ps1 -CleanUi                           # + cargo clean -p ui prima (gotcha frontend, sotto)
```

Non popola `Test Run\`: dopo, esegui `.\deploy_test_run.ps1` (stesso `-BuildConfig`) — vedi
`DEPLOY.md`. Il resto di questa pagina descrive cosa fa `build.ps1` sotto il cofano, comando per
comando, per chi vuole compilare solo un pezzo durante lo sviluppo.

## Rust — debug (ciclo di sviluppo)

Dalla radice del repo:

```powershell
cargo build                                   # solo i "default-members": orchestrator, mcp-server,
                                               # mcp-nmap, startup-config, protocol, i plugin — NON ui
cargo build -p ui                             # UI Tauri (~400 crate esterni, lenta la prima volta)
cargo build -p plugin-ping -p plugin-calc     # plugin usati da Test Run\ (vedi DEPLOY.md)
```

`cargo build` senza `-p` esclude `ui` di proposito (troppo lenta per un ciclo di sviluppo rapido
sul backend) — se ti serve anche la UI, compilala a parte con `-p ui`.

Output: `target\debug\`.

## Rust — release (per un deploy vero)

Stessi comandi con `--release`:

```powershell
cargo build --release
cargo build --release -p ui
cargo build --release -p plugin-ping -p plugin-calc
```

Output: `target\release\`. Serve solo quando prepari un deploy "vero" (vedi `DEPLOY.md`) — per
lavorare sul codice, la build debug basta ed è più veloce.

## .NET — `lare-shell` (host C# del motore PowerShell)

Soluzione separata: `shell/lare-shell/LareShell.sln` (`src/LareShell` l'eseguibile,
`tests/LareShell.Tests` gli xUnit). Dettagli implementativi: `shell/lare-shell/IMPLEMENTATION.md`.

```powershell
dotnet build shell/lare-shell/LareShell.sln
```

Output debug: `shell/lare-shell/src/LareShell/bin/Debug/net10.0/win-x64/` — **non**
`bin\Debug\net10.0\` come un progetto .NET senza RID esplicito. `LareShell.csproj`/
`LareShell.Tests.csproj` dichiarano `<RuntimeIdentifier>win-x64</RuntimeIdentifier>` (ADR-019
punto 1): senza un RID esplicito, `$PSHOME` risolverebbe dentro `runtimes\win\lib\net10.0\`
invece che nella cartella dell'eseguibile, e `powershell.config.json` accanto all'exe (execution
policy) non verrebbe letto.

**`lare-shell` non ha una build "release" utile da sorgente**: per un deploy si fa sempre un
*publish* framework-dependent (vedi `DEPLOY.md`) — `dotnet build`/`dotnet run` servono solo per
sviluppare e testare la host, non per prepararla a un deploy.

Avvio da sorgente (senza publish) — serve sempre `--config-dir` esplicito, non esiste un default
utile accanto a `bin\Debug\...\`:

```powershell
dotnet run --project shell/lare-shell/src/LareShell -- --config-dir "Test Run\Configuration"
```

## Test

```powershell
cargo test                                    # test dei crate backend (default-members)
cargo test -p ui                              # test del crate ui (non è un default-member)
cargo test -p orchestrator <substring>        # un singolo test (o un sottoinsieme per nome)
cargo test -p orchestrator -- --ignored       # test di integrazione reale (API esterne, binari, venv)
node --test crates/ui/frontend/*.test.mjs     # test JS puri del frontend (242 test, nessuna webview)
cargo clippy --all-targets                    # lint su tutti i target del workspace
cargo fmt --check                             # verifica formattazione (v1 non è fmt-clean: differenze note)

dotnet test shell/lare-shell/LareShell.sln                                        # 118 test xUnit
dotnet test shell/lare-shell/LareShell.sln --filter "FullyQualifiedName~Executor"  # un solo file
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

## Strumento di sviluppo: parlare il canale shell senza una console vera

`scripts/dev/shell-client.mjs` imita lato client ciò che fa `lare-shell.exe` sul canale shell
(`Hello{role:"shell"}`, un `Command`, poi risponde ai messaggi dell'orchestratore) — comodo per
verificare l'orchestratore/`ui.exe` **senza** costruire/pubblicare la host C#, o da un terminale
con stdin rediretto (es. Claude Code) dove un gate `[Y/n]` interattivo vero non funzionerebbe.

```powershell
# Con orchestrator e ui.exe già avviati (vedi RUN.md):
node scripts/dev/shell-client.mjs -- '/ping'          # finestra su ui.exe con la tabella
node scripts/dev/shell-client.mjs -- '/nonesiste'     # solo "[done exit_code=0]" (slash scartato)
node scripts/dev/shell-client.mjs -- '/ai ciao'       # [error routing_error]: /ai "testo" (virgolette obbligatorie)
node scripts/dev/shell-client.mjs -- '/config'        # si apre /config su ui.exe
node scripts/dev/shell-client.mjs -- '/ai "elenca i 3 file più grandi qui"'   # [Y/n] → esegue → finestra
```

`--config-dir`/`--session` accettano lo stesso valore di default degli altri binari
(`Test Run\Configuration`, un id generato dal pid): `node scripts/dev/shell-client.mjs
--config-dir "Test Run/Configuration" --session s1 -- '/ping'`. `--selftest` verifica
`exec_in_shell` in locale, senza orchestratore.

**Gotcha (Git Bash/MSYS)**: da Git Bash, MSYS riscrive un argomento che sembra un path assoluto
Unix — `/ping` diventa `C:/Program Files/Git/ping` — prima che Node lo veda. Lancia il client da
**PowerShell** (come sopra), oppure disattiva la riscrittura per quel comando:
`MSYS_NO_PATHCONV=1 node scripts/dev/shell-client.mjs -- '/ping'`.

## Gotcha (Windows, validi per qualunque comando di build sopra)

- **`Accesso negato (os error 5)` durante una build** = un binario è ancora in esecuzione.
  Chiudilo prima di ricompilare — `.\stop_lare.ps1` (default: solo i processi dentro
  `Test Run\`) cerca ed elimina orchestrator/ui/lare-shell/mcp-server/mcp-nmap/i plugin, così non
  serve ricordare i nomi a mano ogni volta:
  ```powershell
  .\stop_lare.ps1                # solo i processi che girano da Test Run\ (il caso normale)
  .\stop_lare.ps1 -Release        # gli stessi nomi ovunque ALTROVE sul filesystem (mai dentro
                                  # Test Run\) — un deploy reale, non quello di sviluppo
  ```
  Per ping.exe/calc.exe (nomi che coincidono con la Calcolatrice e l'utility di rete di Windows)
  il confronto è sempre sul percorso completo (`...\plugins\<id>\<id>.exe`), mai sul nome nudo —
  un `calc.exe` che non è il nostro plugin non viene mai toccato. Equivalente manuale, se preferisci:
  ```powershell
  taskkill /F /IM orchestrator.exe /IM ui.exe /IM lare-shell.exe
  ```
  (in Git Bash, i flag vanno raddoppiati: `taskkill //F //IM orchestrator.exe //IM ui.exe //IM lare-shell.exe`).
- **Riavvia sempre l'orchestrator dopo una build del backend** — non si auto-ricarica.
- **Tauri (`ui`) non ricompila dopo una modifica al solo frontend** (`crates/ui/frontend/`):
  `generate_context!` incorpora `frontendDist` a compile time; `build.rs` ha una
  `rerun-if-changed` sulla cartella frontend che copre la maggior parte dei casi, ma se
  `cargo build -p ui`/`cargo run -p ui` sembra comunque ignorare un cambiamento, forza la
  ricompilazione:
  ```powershell
  cargo clean -p ui
  ```
- **`lare-shell.exe` in esecuzione fa fallire `dotnet build`** con lo stesso errore "Accesso
  negato" — stesso `taskkill` di cui sopra.
