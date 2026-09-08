# DEPLOY — preparare una cartella eseguibile di Lare Terminal 2.0

Come popolare `Test Run\` (o una copia altrove) con i binari e la configurazione, **dopo** aver
compilato (`BUILD.md`). Non contiene istruzioni per avviare il programma — quelle sono in
`RUN.md`.

`Test Run\` nel repo **è già** un deploy: script di popolamento, manifest e struttura sono
committati; binari, DLL, segreti e dati generati dall'uso no (gitignored). Per lo sviluppo
quotidiano non serve altro — `Test Run\` stessa è la cartella da cui lanciare il programma (vedi
`RUN.md`). Questa pagina serve anche a chi vuole copiare il programma su un'altra macchina o in
un'altra cartella.

## Prerequisiti sulla macchina di destinazione

- **Windows 10/11 x64**.
- **WebView2 Runtime** (di norma già presente su Windows 10/11 aggiornati; Tauri lo richiede per
  ospitare le finestre).
- **pwsh 7.6+ installato** (`C:\Program Files\PowerShell\7` o in PATH): `lare-shell` carica
  PSReadLine dai moduli di pwsh, che il NuGet `Microsoft.PowerShell.SDK` non include
  (`PwshLocator`, ADR-019 punto 2) — senza pwsh l'editing di riga ricade su un fallback povero
  (`Console.ReadLine`), la shell resta comunque usabile.
- **.NET 10 runtime x64**: `lare-shell` è pubblicata *framework-dependent* (publish
  `-r win-x64 --self-contained false`, ~100 MB invece di ~200 self-contained). Misurata dal vivo:
  `Test Run\shell\` (lare-shell.exe + le DLL del motore PowerShell, `System.Management.
  Automation.dll` compresa) pesa **41,4 MB**. Senza il runtime il processo non parte (errore di
  framework mancante, non un crash Lare).
- Un venv Python per dominio in `pytools\<dominio>\venv\` **creato a mano** sulla macchina di
  destinazione (mai deployato: vedi sotto) — necessario solo se si useranno i tool che dipendono
  da quel dominio (es. `/markets` → `financial-markets`).

## Popolare (o ri-popolare) `Test Run\`

```powershell
.\deploy_test_run.ps1                          # da target\debug\ (build debug, vedi BUILD.md)
.\deploy_test_run.ps1 -IncludePlugins           # come sopra, più ping.exe/calc.exe in plugins\<id>\
.\deploy_test_run.ps1 -BuildConfig release      # da target\release\ invece di target\debug\
.\deploy_test_run.ps1 -SkipShell                # salta il publish di lare-shell (~1 minuto) —
                                                 # utile quando ricompili solo il Rust
```

Lo script **non tocca mai** `Test Run\Configuration\` (token, config generate, log — gitignored,
sopravvivono a ogni deploy) e pubblica sempre `lare-shell` in `Test Run\shell\` con
`dotnet publish -c Release -r win-x64 --self-contained false` (la host non ha una build "debug"
utile nel deploy — vedi `BUILD.md`), a meno di `-SkipShell`.

Termina con `$LASTEXITCODE` diverso da 0 anche a successo (debito noto, `robocopy` usa 1 per "file
copiati" — vedi `HANDOFF.md`): non è un segnale di errore, guarda l'output
(`Test Run pronta: ...` sull'ultima riga).

**Verifica rapida del deploy** (senza tastiera né orchestratore):

```powershell
& ".\Test Run\shell\lare-shell.exe" --selftest
```

(`Test Run` contiene uno spazio: PowerShell impone l'operatore di chiamata `&` quando il comando è
un'espressione fra virgolette — le virgolette avvolgono **solo il percorso**, gli argomenti
restano fuori. `.\Test Run\...` senza virgolette si spezza sullo spazio; `".\Test Run\...
--selftest"` con l'argomento dentro le virgolette diventa una stringa unica, che PowerShell si
limita a restituire invece di eseguire.)

Una riga `[OK]`/`[FAIL]` per controllo (cartella di configurazione, `startup.json`, pwsh trovato,
…), exit code 0/1.

## Layout del deploy

I percorsi in `startup.json` sono relativi alla radice del deploy (la cartella che contiene
`Configuration\`), non alla posizione originale: `Test Run\` (o una sua copia in qualunque altra
cartella) funziona senza modifiche.

```
<deploy>\
├── orchestrator.exe  mcp-server.exe  mcp-nmap.exe  ui.exe
├── shell\                                            ← lare-shell.exe + DLL del motore PowerShell
│                                                        + powershell.config.json (execution policy,
│                                                        ADR-019); publish framework-dependent win-x64
├── install-wt-profile.ps1  uninstall-wt-profile.ps1  ← installa/rimuove il profilo Windows
│                                                        Terminal "Lare Terminal" (modalità B —
│                                                        vedi RUN.md, si esegue una volta sola)
├── debug_orchestrator.ps1                           ← orchestratore con log live in console, solo
│                                                        per debug (self-heal lo copre altrimenti —
│                                                        vedi RUN.md)
├── Configuration\
│   ├── startup.json                                 ← template, nessun segreto
│   ├── README.md
│   ├── token                                         ← generato al primo avvio (segreto)
│   ├── llms.json                                     ← opzionale, a mano (segreto: api_keys)
│   └── logs\                                         ← generata al primo avvio
├── plugins\ping\{plugin.json, ping.exe}
├── plugins\calc\{plugin.json, calc.exe}
└── pytools\<dominio>\{server.py, requirements.txt, venv\}  ← venv creato a mano, mai deployato
```

## Contenuto di `Configuration\` — cosa è template, cosa è segreto

| File | Committato (template)? | Segreto? | Chi lo crea |
|---|---|---|---|
| `startup.json` | Sì | No | template nel repo, coincide con i default hard-coded |
| `README.md` | Sì | No | template nel repo |
| `token` | No (gitignored) | **Sì** — token WS (256 bit) | `orchestrator`, al primo avvio |
| `llms.json` | No (gitignored) | **Sì** — contiene `api_keys` per provider | a mano, solo se serve un provider diverso dal default |
| `telegramsettings.json` | No | **Sì** — token del bot | a mano, solo per attivare Telegram |
| `telegram-state.json` | No | **Sì** — secret TOTP + chat id | `orchestrator`, quando Telegram è attivo |
| `config.json` | No | No (locale) | `ui.exe`, al primo salvataggio impostazioni |
| `network.json`, `search-paths.json`, `search-content.json`, `market_data.json` | No | No, ma specifici macchina | `orchestrator`/`ui.exe`, a runtime |
| `notes.json`, `memory-<label>.md` | No | No | `orchestrator`, a runtime |
| `logs\`, `library\`, `plugin-storage\<id>\` | No | No | app rispettive, a runtime |

Dettaglio completo (con "quando" esatto: al primo avvio vs a ogni avvio vs al primo salvataggio)
in `Test Run/Configuration/README.md`, che è lo stesso template usato in ogni deploy.

**Credenziali dei provider AI.** `ANTHROPIC_API_KEY` (e `OPENROUTER_API_KEY`) restano variabili
d'ambiente — scelta esplicita, fuori dallo scope di D6 (che riguarda la *posizione* della
configurazione Lare, non le credenziali dei provider): impostale sulla macchina di destinazione,
oppure crea `Configuration\llms.json` a mano per selezionare un provider/modello diverso dal
default (`claude-sonnet-4-6` via Claude diretto, con `StubAdapter` come fallback se nessuna delle
due è disponibile). Senza nessuna delle due, l'orchestrator logga `ANTHROPIC_API_KEY non
impostata — uso StubAdapter` e risponde con un adapter finto invece di chiamare Claude davvero.

## Copiare il deploy altrove

Copia l'intera cartella (`Test Run\`, o l'equivalente popolato altrove) nella destinazione:
qualunque percorso va bene, i path relativi in `startup.json` ripartono dalla nuova radice.
Dopo la copia:

- Se serve un provider AI diverso dal default, crea `Configuration\llms.json` **a mano sulla
  nuova macchina** (mai copiarlo da un altro deploy: contiene chiavi).
- Se serve un dominio `pytools` (es. `/markets`), crea il venv a mano dentro
  `pytools\<dominio>\` sulla macchina di destinazione (vedi `BUILD.md` §Python) — i venv non sono
  mai copiati dal deploy script.

Per avviare il programma una volta che il deploy è pronto (qui o dopo una copia): **`RUN.md`**.
