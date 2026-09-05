# DEPLOY — Lare Terminal 2.0

> Stato a fine piano 1: questo documento descrive il deploy dei componenti che esistono oggi
> (`orchestrator`, `mcp-server`, `mcp-nmap`, `ui`, plugin, `pytools`). **`shell\lare-shell.exe`
> non esiste ancora** (arriva col piano 2): senza di lui non c'è ancora un canale che apra le
> finestre da solo — vedi il flag di sviluppo `--open` in `RUN-LOCAL.md`.

## Prerequisiti sulla macchina di destinazione

- **Windows 10/11 x64**.
- **WebView2 Runtime** (di norma già presente su Windows 10/11 aggiornati; Tauri lo richiede per
  ospitare le finestre).
- **pwsh 7.6+ e .NET 10 runtime: dal piano 2 (host C#)** — non servono per il layout attuale
  (nessun binario 2.0 di oggi li richiede); saranno prerequisiti quando `lare-shell.exe` (host
  del motore PowerShell, ADR-015) sarà pubblicata framework-dependent.
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

1. Compila (`cargo build`, `cargo build -p ui`, `cargo build -p plugin-ping -p plugin-calc` —
   vedi `RUN-LOCAL.md`), possibilmente `-BuildConfig release` per un deploy reale.
2. `.\deploy_test_run.ps1 -BuildConfig release -IncludePlugins` popola `Test Run\`.
3. Copia `Test Run\` nella cartella di destinazione (qualunque percorso: i path relativi
   ripartono dalla nuova radice).
4. Se serve un provider AI diverso dal default, crea `Configuration\llms.json` a mano (mai
   copiarlo da un altro deploy: contiene chiavi).
5. Se serve un dominio `pytools` (es. `/markets`), crea il venv a mano dentro
   `pytools\<dominio>\` sulla macchina di destinazione (vedi `RUN-LOCAL.md` §Python) — i venv non
   sono mai copiati dal deploy script.
6. Avvia con `.\init_orchestrator.ps1` poi `.\init_tauri.ps1` (o viceversa; nessuno dei due
   dipende dall'ordine in questo piano — l'auto-avvio reciproco tra `orchestrator` e `ui.exe`,
   spec §6.4, non è ancora implementato).
