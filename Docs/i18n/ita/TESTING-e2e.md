# TESTING-e2e — checklist end-to-end

Verifica dal vivo (non automatizzabile: processi reali, finestre reali) che il deploy funzioni
davvero — non solo che i test unitari passino. Parti 1-4 riproducono quanto verificato dal vivo a
chiusura del **piano 1** ("fondamenta": deploy in `Test Run\`, avvio di orchestrator e `ui.exe`);
Parte 5 aggiunge il canale shell del **piano 2a**. Compila la colonna **Esito** eseguendo i passi
in ordine, da una macchina pulita se possibile (nessun
`Test Run\Configuration\token`/`logs\` residui da run precedenti, per vedere anche il caso
"primo avvio").

**Prerequisiti**: build fatta (`cargo build`, `cargo build -p ui`, `cargo build -p plugin-ping -p
plugin-calc`) e `.\deploy_test_run.ps1 -IncludePlugins` eseguito — vedi `RUN-LOCAL.md`.

## Parte 1 — Orchestrator da `Test Run\`

| # | Passo | Atteso | Esito |
|---|---|---|---|
| 1 | Da `Test Run\`: `.\init_orchestrator.ps1` (equivalente a `orchestrator.exe --console-log`) | Avvio senza panic; sulla console compare `config dir: ...\Test Run\Configuration` | |
| 2 | Stessa esecuzione, riga successiva | Compare `Lare Terminal orchestrator v2.0.x starting (...)` con le versioni dei crate correlati | |
| 3 | Se `Test Run\Configuration\token` NON esisteva prima del passo 1 | Compare `[token] primo avvio: token creato in ...\Test Run\Configuration\token`; il file esiste dopo, 64 caratteri esadecimali | |
| 4 | Stessa esecuzione | Compare `plugin host: 2 plugin/i scoperti in ...\Test Run\plugins` (richiede il deploy con `-IncludePlugins`, altrimenti 0) | |
| 5 | Stessa esecuzione | Compare `Lare Terminal orchestrator listening on ws://127.0.0.1:<ws_port>` (default 7331, da `Configuration\startup.json`) | |
| 6 | Dopo qualche secondo, controlla il filesystem | `Test Run\Configuration\logs\orchestrator.log.<data-di-oggi>` esiste | |
| 7 | Nessun passo precedente ha mostrato un panic o uno stack trace Rust | — | |

## Parte 2 — `ui.exe` con l'orchestrator attivo

| # | Passo | Atteso | Esito |
|---|---|---|---|
| 8 | Con l'orchestrator del passo 1 ancora attivo, da `Test Run\`: `.\ui.exe --open library` (nota: `init_tauri.ps1` non passa `--open` — per questa verifica lancia `ui.exe` direttamente) | Sulla console compare `[ui] config dir: ...\Test Run\Configuration`, poi `[ui] Loaded config: ...`, poi `[ui] Lare Terminal v2.0.x started.`; nessun crash | |
| 9 | Stesso avvio | Si apre la finestra **Library** (non la vecchia finestra overlay/cursore: quella non esiste più — vedi ADR-016) | |
| 10 | Premi F2 mentre una finestra Lare è in focus | Non succede nulla (l'hotkey globale F2 e il suo overlay sono stati rimossi nel piano 1, ADR-016 — nessun plugin `global-shortcut` registrato) | |
| 11 | Ripeti il passo 8 con `--open config` al posto di `--open library` | Si apre la finestra **`/config`** (tab Ricerca web + Trasparenza, senza i campi hotkey/posizione/cursore rimossi in v1→2.0) | |
| 12 | Chiudi tutto con un solo comando: `Get-Process | Where-Object { $_.Path -like "*Test Run*" } | Stop-Process -Force` (prende `orchestrator`, `ui` E l'eventuale `ping.exe` del plugin — non confonderlo col `ping` di sistema, che `taskkill /IM ping.exe` colpirebbe anche lui) | Rilanciando lo stesso `Get-Process` subito dopo: nessun risultato (nessun processo residuo) | |

## Parte 3 — `ui.exe` SENZA orchestrator

| # | Passo | Atteso | Esito |
|---|---|---|---|
| 13 | Assicurati che l'orchestrator NON sia in esecuzione, poi da `Test Run\`: `.\ui.exe --open library` | Il processo si avvia e resta vivo (nessun crash, nessun panic); stdout mostra comunque `[ui] config dir: ...` e `[ui] Lare Terminal v2.0.x started.` — `ui.exe` non verifica la raggiungibilità dell'orchestrator prima di avviarsi | |
| 14 | Osserva la finestra Library appena aperta per qualche secondo | La finestra resta aperta e reattiva (il fallimento della connessione WS è gestito lato JS in `host.js`/`ws-client.js`: retry automatico con backoff crescente, **visibile solo aprendo i DevTools della webview** — non nello stdout/stderr del processo `ui.exe`, verificato dal vivo durante il piano 1) | |
| 15 | Chiudi `ui.exe` | Il processo termina senza lasciare residui (`Get-Process` come al passo 12) | |

> Nota: il comportamento "nessun crash" è l'unica cosa verificabile da terminale in questo piano.
> Un log applicativo esplicito tipo "orchestratore non raggiungibile" non esiste ancora: la UI
> (pagina host nascosta) non ha oggi un percorso che lo stampi su stdout — solo il retry di
> `ws-client.js`, osservabile in DevTools. Rifinire questo segnale è lavoro dei piani successivi.

## Parte 4 — Deploy portabile

| # | Passo | Atteso | Esito |
|---|---|---|---|
| 16 | Copia l'intera cartella `Test Run\` (con i binari già deployati) in un'altra posizione, es. `C:\LareTest\` | La copia riesce (nessun percorso assoluto incorporato nei file committati) | |
| 17 | Da `C:\LareTest\`: `.\init_orchestrator.ps1` | Si avvia esattamente come al passo 1, ma `config dir:` e ogni percorso derivato (plugin, log) puntano alla NUOVA posizione (`C:\LareTest\Configuration`, `C:\LareTest\plugins`, ...) — nessuna modifica a `startup.json` necessaria (percorsi relativi alla radice del deploy, non alla posizione originale) | |
| 18 | Da `C:\LareTest\`: `.\init_tauri.ps1` (senza `--open`, se `orchestrator` è attivo) | Si avvia senza crash; senza `--open` e senza `lare-shell` (non ancora esistente) nessuna finestra si apre da sola — comportamento atteso in questo piano, non un difetto | |
| 19 | Ripulisci: chiudi i processi avviati dalla copia | Nessun processo residuo | |

## Parte 5 — Canale shell col client di sviluppo

Verifica il canale shell del piano 2a (`Docs/i18n/ita/06-decisions.md` ADR-018) senza `lare-shell`
(host C#, piano 2b): `scripts/dev/shell-client.mjs` imita la host — vedi `RUN-LOCAL.md` §"Canale
shell senza la host" per l'uso e il gotcha Git Bash/MSYS (lancia da **PowerShell**). Prerequisiti:
orchestrator e `ui.exe` avviati da `Test Run\` (Parte 1/2); nessun `ANTHROPIC_API_KEY` impostata
va bene — l'orchestrator ricade sullo `StubAdapter`, sufficiente per verificare il giro dei
messaggi (non la qualità delle risposte).

| # | Comando | Atteso nel terminale (dev client) | Atteso su `ui.exe` |
|---|---|---|---|
| 20 | `node scripts/dev/shell-client.mjs -- '/ping'` | `→ finestra "Lare — /ping" aperta` + `[done exit_code=0]` | Si apre la finestra **"Lare — /ping"** con la tabella per strato (`lare-shell`/`orchestrator`/`plugin-ping`/`ui.exe`) |
| 21 | `node scripts/dev/shell-client.mjs -- '/nonesiste'` | Solo `[done exit_code=0]` (nessuna riga di conferma: slash scartato in silenzio, log `discard slash` sull'orchestrator) | Nessuna finestra |
| 22 | `node scripts/dev/shell-client.mjs -- '/ai ciao'` | `[error routing_error] sintassi: /ai "testo" (virgolette obbligatorie)` | Nessuna finestra |
| 23 | `node scripts/dev/shell-client.mjs -- '/reset'` | `non applicabile: la sessione è la tua` + `[done exit_code=0]` | Nessuna finestra |
| 24 | `node scripts/dev/shell-client.mjs -- '/config'` | Solo `[done exit_code=0]` (nessuna riga di conferma per `OpenUiLocal`) | Si apre (o va in primo piano) la finestra **"Lare — Configurazione"** |
| 25 | `node scripts/dev/shell-client.mjs -- '/help'` | `→ finestra aperta` (eccezione spec §3.2: `/help` non apre ANCHE la finestra di output, riga di conferma generica) | Si apre (o va in primo piano) la finestra singleton **"Lare — Comandi"** |
| 26 | `node scripts/dev/shell-client.mjs -- '/open .'` | `→ finestra "Lare — /open" aperta` | Si apre la finestra **"Lare — /open"** con l'esito del comando |
| 27 | `node scripts/dev/shell-client.mjs -- '/ai "ciao, chi sei?"'` | `[Y/n]` solo se lo `StubAdapter` propone un tool (altrimenti diretto a) `→ finestra "ciao, chi sei?" aperta` + `[done]` (`exit_code` **null**, percorso AI) | Si apre la finestra **"ciao, chi sei?"** (titolo dal testo fra virgolette) col contenuto della risposta |
