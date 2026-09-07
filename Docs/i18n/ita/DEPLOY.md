# DEPLOY — Lare Terminal 2.0

> Stato a fine piano 2b: `lare-shell.exe` (host C# del motore PowerShell, ADR-015) esiste ed è
> pubblicata da `deploy_test_run.ps1` in `Test Run\shell\`, verificabile in **modalità B** (profilo
> Windows Terminal "Lare Terminal" — `install-wt-profile.ps1`). La modalità A (`ui.exe` che lancia
> `lare-shell.exe` dentro una ConPTY) è il piano 3, completato — vedi `RUN-LOCAL.md` sezione
> "Modalità A".

## Prerequisiti sulla macchina di destinazione

- **Windows 10/11 x64**.
- **WebView2 Runtime** (di norma già presente su Windows 10/11 aggiornati; Tauri lo richiede per
  ospitare le finestre).
- **pwsh 7.6+ installato** (`C:\Program Files\PowerShell\7` o in PATH): `lare-shell` carica
  PSReadLine dai moduli di pwsh, che il NuGet `Microsoft.PowerShell.SDK` non include
  (`PwshLocator`, ADR-019 punto 2) — senza pwsh l'editing di riga ricade su un fallback povero
  (`Console.ReadLine`), la shell resta comunque usabile.
- **.NET 10 runtime x64**: `lare-shell` è pubblicata *framework-dependent* (publish
  `-r win-x64 --self-contained false`, ~100 MB invece di ~200 self-contained). Misurata dal vivo
  nel Task 8: `Test Run\shell\` (lare-shell.exe + le DLL del motore PowerShell, `System.Management.
  Automation.dll` compresa) pesa **41,4 MB**. Senza il runtime il processo non parte (errore di
  framework mancante, non un crash Lare).
- Un venv Python per dominio in `pytools\<dominio>\venv\` **creato a mano** sulla macchina di
  destinazione (mai deployato: vedi sotto) — necessario solo se si useranno i tool che dipendono
  da quel dominio (es. `/markets` → `financial-markets`).

## Layout di deploy

`Test Run\` nel repo è lo specchio esatto del layout (spec §7): struttura, manifest e script sono
committati; binari, DLL, segreti e dati generati dall'uso no. Per creare un deploy altrove, copia
`Test Run\` (dopo un `deploy_test_run.ps1` che l'abbia popolata di binari) in una cartella
qualunque — i percorsi in `startup.json` sono relativi alla radice del deploy (la cartella che
contiene `Configuration\`), non alla posizione originale, quindi funziona senza modifiche.

```
<deploy>\
├── orchestrator.exe  mcp-server.exe  mcp-nmap.exe  ui.exe
├── shell\                                            ← lare-shell.exe + DLL del motore PowerShell
│                                                        + powershell.config.json (execution policy,
│                                                        ADR-019); publish framework-dependent win-x64
├── install-wt-profile.ps1  uninstall-wt-profile.ps1  ← modalità B: profilo Windows Terminal "Lare Terminal"
├── init_orchestrator.ps1  init_tauri.ps1            ← avvio manuale, nessuna variabile d'ambiente
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
due è disponibile).

## Passi

1. Compila in release (`cargo build --release`, `cargo build --release -p ui`, `cargo build
   --release -p plugin-ping -p plugin-calc` — vedi `RUN-LOCAL.md` per gli equivalenti debug usati
   in sviluppo).
2. `.\deploy_test_run.ps1 -BuildConfig release -IncludePlugins` popola `Test Run\` da
   `target\release\` (deve esistere: senza il passo 1 in release, lo script fallisce con "Manca
   ...\target\release") **e pubblica `lare-shell` in `Test Run\shell\`** (`dotnet publish -c
   Release -r win-x64 --self-contained false`, sempre — la host non ha una build "debug" utile nel
   deploy). `-SkipShell` salta questo passo (utile quando si ricompila solo il Rust: il publish
   .NET costa circa 1 minuto).
3. `.\install-wt-profile.ps1` (da `Test Run\`, o passando `-ShellExe` a un percorso diverso)
   installa il profilo "Lare Terminal" in Windows Terminal (modalità B, spec §2.3): scrive un
   fragment JSON in `%LOCALAPPDATA%\Microsoft\Windows Terminal\Fragments\Lare\` — **riavvia
   Windows Terminal** (chiudi tutte le finestre) perché il fragment venga letto, aprire una nuova
   scheda in una finestra già avviata non basta.
4. `.\shell\lare-shell.exe --selftest` verifica il deploy senza tastiera né orchestratore: una
   riga `[OK]`/`[FAIL]` per controllo (cartella di configurazione, `startup.json`, pwsh trovato,
   …), exit code 0/1 — utile anche dopo aver copiato `Test Run\` altrove (passo 5 sotto, percorsi
   relativi).
5. Copia `Test Run\` nella cartella di destinazione (qualunque percorso: i path relativi
   ripartono dalla nuova radice).
6. Se serve un provider AI diverso dal default, crea `Configuration\llms.json` a mano (mai
   copiarlo da un altro deploy: contiene chiavi).
7. Se serve un dominio `pytools` (es. `/markets`), crea il venv a mano dentro
   `pytools\<dominio>\` sulla macchina di destinazione (vedi `RUN-LOCAL.md` §Python) — i venv non
   sono mai copiati dal deploy script.
8. Modalità B (consigliata): apri la scheda "Lare Terminal" in Windows Terminal (passo 3) —
   `lare-shell.exe` avvia da sola `orchestrator.exe` e `ui.exe` se mancano (self-heal, §6.4).
   Modalità manuale (senza `lare-shell`, come nei piani precedenti): `.\init_orchestrator.ps1` poi
   `.\init_tauri.ps1` (o viceversa, l'ordine non conta).
