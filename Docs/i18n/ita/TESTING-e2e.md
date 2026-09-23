# TESTING-e2e — checklist end-to-end

Verifica dal vivo (non automatizzabile: processi reali, finestre reali) che il deploy funzioni
davvero — non solo che i test unitari passino. Parti 1-4 riproducono quanto verificato dal vivo a
chiusura del **piano 1** ("fondamenta": deploy in `Test Run\`, avvio di orchestrator e `ui.exe`);
Parte 5 aggiunge il canale shell del **piano 2a** (client di sviluppo, senza la host reale); Parte
6 aggiunge la host C# vera del **piano 2b** in modalità B (Windows Terminal); Parte 7 aggiunge la
finestra terminale del **piano 3** in modalità A (`ui.exe`, xterm.js + ConPTY) — **eseguita dal vivo
parzialmente**, vedi la nota in testa a quella sezione. Compila la colonna
**Esito** eseguendo i passi in ordine, da una macchina pulita se possibile (nessun
`Test Run\Configuration\token`/`logs\` residui da run precedenti, per vedere anche il caso
"primo avvio").

**Prerequisiti**: build fatta (`cargo build`, `cargo build -p ui`, `cargo build -p plugin-ping -p
plugin-calc`) e `.\deploy_test_run.ps1 -IncludePlugins` eseguito — vedi `BUILD.md`/`DEPLOY.md`.

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

Aggiornata per riflettere due fix successivi al piano 1: il self-heal (piano 3, spec §6.4 —
`ui.exe` ORA verifica la raggiungibilità dell'orchestrator all'avvio e lo avvia da sé se serve) e
questo fix (`ui.exe` non ha più stdout/stderr visibile in NESSUNA build — `windows_subsystem =
"windows"` incondizionato — ma logga su file, `Configuration\logs\ui.log.<data>`).

| # | Passo | Atteso | Esito |
|---|---|---|---|
| 13 | Enumera con UIA TUTTE le top-level window esistenti (`[System.Windows.Automation.AutomationElement]::RootElement.FindAll(TreeScope.Children, TrueCondition)`, **senza filtrare per PID** — un filtro sui soli PID di `ui`/`orchestrator`/`lare-shell` non vedrebbe MAI una finestra/scheda spuria, perché quella finestra appartiene al PID di `WindowsTerminal.exe`, non a uno dei tre processi Lare) come riferimento ("prima"). Assicurati che l'orchestrator NON sia in esecuzione, poi da `Test Run\`: `.\ui.exe`. Attendi ~4s, poi ri-enumera TUTTE le top-level window ("dopo") e confronta con "prima" (`Compare-Object`) | Il self-heal lo avvia da solo (nessun crash): il processo `orchestrator.exe` compare via `Get-Process` entro pochi secondi. Le UNICHE righe nuove nel confronto "prima"/"dopo" sono `Name = "Lare Terminal"` (`Class = "Tauri Window"`) e un elemento con `Class = "Tao Thread Event Target"` (`IsOffscreen = False` ma nessun contenuto visibile — dettaglio interno del framework Tao/wry, non una finestra utente) — **nessuna riga nuova con `ProcName = "WindowsTerminal"` o un `Name` tipo `...\ui.exe`/`...\plugins\...\exe`** (il sintomo del bug prima di questo fix) | |
| 13bis | (facoltativo, copre `mcp-server.exe`) Da una sessione con l'orchestrator già attivo, invoca un turno `/ai` che richieda un tool di `mcp-server` (es. `/ai "elenca i 3 file più grandi qui"`, `BUILD.md`) — costa una chiamata API reale. Ri-enumera le top-level window | `mcp-server.exe` compare in `Get-Process`, `Configuration\logs\mcp-server.log` viene creato — nessuna riga nuova nel confronto delle finestre | |
| 14 | Apri `Test Run\Configuration\logs\ui.log.<data-di-oggi>` | Contiene le righe di avvio: `[ui] config dir: ...\Test Run\Configuration` e `[ui] Lare Terminal v2.2.x started.` — sostituisce l'osservazione su stdout, che non esiste più (`ui.exe` non ha mai una console, in nessuna build) | |
| 15 | Chiudi `ui.exe` | Il processo termina senza lasciare residui (`Get-Process` come al passo 12) | |

> Nota: prima di questi due fix, "nessun crash" era l'unica cosa verificabile da terminale in
> questo scenario, e un log applicativo esplicito tipo "orchestratore non raggiungibile" non
> esisteva. Ora esiste: il self-heal logga il tentativo (`tracing::warn!`, via
> `startup_config::logging`, modulo condiviso con l'orchestrator) su `ui.log.<data>` — visibile
> anche in release, dove prima (prima di questo fix) `ui.exe` non aveva stderr da cui vederlo.

## Parte 4 — Deploy portabile

| # | Passo | Atteso | Esito |
|---|---|---|---|
| 16 | Copia l'intera cartella `Test Run\` (con i binari già deployati) in un'altra posizione, es. `C:\LareTest\` | La copia riesce (nessun percorso assoluto incorporato nei file committati) | |
| 17 | Da `C:\LareTest\`: `.\init_orchestrator.ps1` | Si avvia esattamente come al passo 1, ma `config dir:` e ogni percorso derivato (plugin, log) puntano alla NUOVA posizione (`C:\LareTest\Configuration`, `C:\LareTest\plugins`, ...) — nessuna modifica a `startup.json` necessaria (percorsi relativi alla radice del deploy, non alla posizione originale) | |
| 18 | Da `C:\LareTest\`: `.\init_tauri.ps1` (senza `--open`, se `orchestrator` è attivo) | Si avvia senza crash; senza `--open` e senza `lare-shell` (non ancora esistente) nessuna finestra si apre da sola — comportamento atteso in questo piano, non un difetto | |
| 19 | Ripulisci: chiudi i processi avviati dalla copia | Nessun processo residuo | |

## Parte 5 — Canale shell col client di sviluppo

Verifica il canale shell del piano 2a (`Docs/i18n/ita/02-decisions.md` ADR-018) senza `lare-shell`
(host C#, piano 2b): `scripts/dev/shell-client.mjs` imita la host — vedi `BUILD.md` §"Strumento di
sviluppo: parlare il canale shell senza una console vera" per l'uso e il gotcha Git Bash/MSYS
(lancia da **PowerShell**). Prerequisiti:
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

## Parte 6 — Modalità B in Windows Terminal (host `lare-shell`)

Verifica dal vivo del **piano 2b** (ADR-015/ADR-019): la host C# vera, aperta come profilo
Windows Terminal "Lare Terminal" (`install-wt-profile.ps1`), con `orchestrator.exe`/`ui.exe`
**non** già in esecuzione prima di aprire la scheda (`Get-Process orchestrator, ui
-ErrorAction SilentlyContinue | Stop-Process`). Eseguita dal controller il 2026-09-07 (nessun
terminale interattivo lato implementer; driver SendKeys + screenshot) — esito integrale nel
ledger (`.superpowers/sdd/2026-09-06-piano-2b-host-lare-shell/progress.md`, sezione "E2E dal vivo
(controller)"). Trascritto qui fedelmente; dove il ledger non annota un passo in modo esplicito,
la colonna Esito dice "non verificato" piuttosto che presumerlo.

**Limite della macchina (prime tre passate)**: nessuna `ANTHROPIC_API_KEY` né
`Configuration\llms.json` → `StubAdapter` (nessuna tool call, nessun gate reale): i punti 5-9 erano
coperti solo dai test automatici (`SlashTurnTests`, `ExecutorTests`, `ws_integration.rs`).
**Quarta passata (2026-09-07, AI reale)**: `llms.json` preso dal deploy v1
(`C:\Lare Terminal\Local\llms.json` → `Test Run\Configuration\llms.json`, gitignored; provider
attivo `claude-direct`) e i punti 5-9 sono stati verificati dal vivo in una nuova finestra Windows
Terminal — esiti nella tabella.

**Due passate**: la prima ha trovato un difetto (§"Difetti trovati" sotto), corretto con il fix
`a8d148f` (2 file + test nuovi, 115/115); la seconda, dopo la ripubblicazione di `Test Run\`, ha
riverificato i punti toccati dal fix (autostart, riconnessione).

| # | Verifica | Esito |
|---|---|---|
| 1 | Banner all'apertura della scheda; "orchestratore: NON connesso … avvio … orchestrator.exe" → "orchestratore avviato e connesso"; `ui.exe` compare (autostart §6.4) | OK — autostart orchestratore (~1 s) e `ui.exe`. Prima passata: difetto, `ui.exe` (app console in build debug) apriva una SCHEDA Windows Terminal e rubava il fuoco (vedi difetto 1 sotto). Seconda passata, dopo il fix: OK, autostart senza scheda WT spuria |
| 2 | Prompt/profilo come in pwsh (PSReadLine, alias, oh-my-posh se presente); `Get-Date`, `dir`, `cd ..` funzionano | OK banner/PSReadLine/`Get-Date`; OK `cd ..` persiste nel prompt (D17). `dir` non risulta annotato esplicitamente nel ledger come passo a sé — non verificato in modo distinto |
| 3 | `/ping` → finestra "Lare — /ping" con le righe per strato + riga di conferma nel terminale | OK (finestra + riga di conferma); riverificato nella seconda passata come parte della verifica di riconnessione (punto 10) |
| 4 | `/help`, `/config`, `/library` → finestre giuste; `/nonesiste` → muto; `/ai x` senza virgolette → errore di sintassi | OK `/help` → "Lare — Comandi"; OK `/config`; OK `/library` → "Lare — Archivio" (seconda passata); OK `/nonesiste` muto (log discard); OK `/ai x` → riga di errore di sintassi |
| 5 | `/ai "elenca i 3 file più grandi in questa cartella"` → `[Y/n]` → Invio → comando eseguito NEL terminale → finestra Markdown col risultato | **OK (quarta passata, AI reale)**: tre gate in sequenza (due `cerca routine`, poi `Get-ChildItem -File \| Sort-Object Length -Descending \| Select-Object -First 3 …`), `y` a ciascuno, tabella stampata nel terminale, finestra "elenca i 3 file più grandi…" con il risultato, riga `→ finestra … aperta`. **Anomalia osservata una volta**: i primi due gate risultavano già accettati (stampato `y` senza tasti inviati) e la `y` del terzo è comparsa anche al prompt successivo — tasti pendenti/duplicati nel buffer della console; fix: il gate svuota il buffer prima del prompt e logga i tasti scartati (`gate: scartato tasto pendente …`); nelle passate seguenti nessun tasto pendente e nessun doppione (log `gate: tasto Key=Y`) |
| 6 | `/ai "vai nella cartella Documents"` → il prompt dopo mostra `Documents` (cwd persiste, D17) | **OK (AI reale)**: gate `cd ~\Documents; pwd` → `y` → `Path` stampato → prompt `PS C:\Users\<utente>\Documents>` |
| 7 | `/ai "cancella tutti i file temporanei"` → `n` al gate → nessun comando eseguito, il turno finisce | **OK (AI reale)**: gate `cerca routine` + `Get-ChildItem -Force \| Where-Object …` → `n` (log `gate: tasto Key=N`) → nessuna esecuzione, turno completato, finestra aperta con la risposta dell'AI |
| 8 | Ctrl+C in attesa del turno AI, e Ctrl+C durante un `ExecInShell` lungo avviato dall'AI → "annullato (Ctrl+C)"/"comando interrotto (Ctrl+C): turno annullato" | **OK (AI reale, nuova finestra WT)**: (a) `/ai "conta lentamente fino a un milione…"` + Ctrl+C dopo 2 s → `annullato (Ctrl+C)`, prompt; log `Ctrl+C ricevuto (turno in corso: True)` / `turno … annullato`, e i `Chunk`/`Done` arrivati dopo per quel turno scartati come stantii (contratto c); (b) `/ai "esegui Start-Sleep -Seconds 60"` → `y` → Ctrl+C → `[LARE] comando interrotto (Ctrl+C).` + `comando interrotto (Ctrl+C): turno annullato`, prompt. Ctrl+C su un comando DIGITATO (`Start-Sleep 30`, terza passata): `[LARE] comando interrotto (Ctrl+C).` e prompt. Nelle prime due passate (host in `conhost`) la riga mancava: era il **driver** (`SendKeys ^c` e `GenerateConsoleCtrlEvent` non consegnavano alcun Ctrl+C, nemmeno a un `pwsh` di controllo) — non la host |
| 9 | `/ai "apri python in modo interattivo"` → REPL python utilizzabile, `exit()` torna al prompt | **OK (AI reale)**: gate `python (interattivo)` → `y` → REPL di Python 3.14 nel terminale (`>>>`), input digitato ed eseguito, `exit` → riga `→ finestra … aperta` e prompt |
| 10 | Chiudi `ui.exe` → `/help` la riavvia (self-heal, ruling 8); uccidi l'orchestratore (`Stop-Process -Name orchestrator`) → `/ping` → "orchestratore non raggiungibile … avvio …" → riconnesso e finestra aperta | Parzialmente verificato. OK: "orchestratore ucciso → `/ping` riavvia e riconnette (stessa sessione)", finestra `/ping` aperta (seconda passata). La chiusura manuale di `ui.exe` seguita da `/help` per il self-heal non risulta annotata come passo distinto — non verificato esplicitamente (l'autostart di `ui.exe` all'apertura della scheda è invece verificato, punto 1) |
| 11 | `exit` → la scheda si chiude; `Get-Process lare-shell` → nulla; `Get-Process orchestrator, ui` → ANCORA vivi (processi staccati) | OK, verificato **due volte**: prima passata, "orchestrator e ui sopravvivono (anche quando la host girava in una scheda WT chiusa)"; seconda passata, "orchestrator e ui vivi" dopo `exit`. Il comando `Get-Process lare-shell` non risulta trascritto nel ledger come eseguito alla lettera — la sopravvivenza dei due processi staccati è comunque confermata in entrambe le passate |
| 12 | Copia `Test Run\` in `%TEMP%\LareCopia\`, esegui `LareCopia\shell\lare-shell.exe --selftest` → `[OK]` con la Configuration di `LareCopia` (percorsi relativi, §6.3) | **Non verificato**: questo scenario (copia in un'altra cartella, percorsi relativi) non risulta nel ledger dell'e2e dal vivo di oggi. `Test Run\shell\lare-shell.exe --selftest` sul deploy originale (non copiato) è stato eseguito con esito `[OK]` su ogni controllo (Task 8, riverificato dal controller) |

### Difetti trovati durante l'e2e (prima passata) e correzioni

1. **`ui.exe` rubava il fuoco alla shell.** In build debug `ui.exe` è un'app console (Tauri tiene
   la console per i log); avviata con `WindowStyle.Normal` e Windows Terminal come terminale
   predefinito, quella console si apriva come una NUOVA SCHEDA di WT. Fix: `Launcher.EnsureUi()`
   avvia anche `ui.exe` con `hideWindow: true` (verificato: le finestre vere create da `ui.exe` —
   Markdown, `/config`, `/library` — restano visibili; solo la console di debug resta nascosta).
   Vedi ADR-019 punto 6 (rivisto).
2. **Fragment del profilo WT**: il sintomo "avvio di `Terminal` fallito" era il driver SendKeys,
   non le virgolette; la revisione finale ha poi imposto il `commandline` **quotato** (senza, con
   spazi nel percorso, `CreateProcess` prova prefissi come `…\Progetti\Lare.exe`), fix wave `22dcb09`.
3. **Il fragment richiede il riavvio di Windows Terminal.** La finestra WT dell'utente era già
   aperta quando il fragment è stato installato → il profilo "Lare Terminal" non era ancora
   caricato in quella finestra → la **prima passata** dell'e2e è stata condotta in una finestra
   **conhost** (non WT), con `ui.exe` avviata a mano (`-WindowStyle Hidden`) invece che dal
   profilo. La **seconda passata**, dopo il fix e la ripubblicazione di `Test Run\`, ha riverificato
   autostart e riconnessione.

Fix wave e2e: commit `a8d148f` (2 file + test nuovi, 115/115).

**Nota (incidente durante l'e2e).** Durante la prima passata, guidata via SendKeys e
UI Automation, il controller ha chiuso con `exit` una scheda "PowerShell" nella finestra Windows
Terminal dell'utente (`wt -w new` aveva aperto una seconda finestra WT nello stesso processo e
l'enumerazione leggeva la prima): quasi certamente una scheda dell'utente, non aperta da questo
e2e — nessun dato recuperabile da questa sede, va solo segnalato.

## Parte 7 — Modalità A (piano 3)

> **Esecuzione dal vivo: parzialmente fatta.** Questa parte copre la checklist di spec §10 per la
> **modalità A** (`ui.exe` con finestra terminale, xterm.js + ConPTY). Il Task 7 di questo piano
> (documentazione) non l'aveva eseguita dal vivo — chi aveva scritto questa sezione non aveva
> accesso a una GUI/interattiva per aprire davvero `ui.exe` e osservare la finestra. Il controller
> l'ha eseguita dal vivo DOPO il completamento del piano (con lo stesso metodo "da tastiera" già
> usato per la Parte 6 — `scripts/dev/e2e-driver/`, SendKeys + screenshot + UI Automation, README
> con le lezioni), incluso il fix di chiusura finestra emerso proprio da questa passata (commit
> `07c584c`), e ha anche trovato e corretto dal vivo un secondo difetto reale
> (bottone "riavvia" invisibile dietro il viewport di xterm.js, `z-index`, commit `80cd46a`). **Passi
> 1/2/5/6/7/9/10, le verifiche di modalità B/autostart Task 6 e il bottone "riavvia" eseguiti dal
> vivo dal controller dopo il completamento del piano; restano da eseguire solo `/calc`, `/config`/
> `/library`/i pulsanti della barra, `/aichat` doppio, l'avvio senza autostart, e il resize.**
> Stesso principio delle Parti precedenti: registrare gli esiti REALI una volta osservati, non
> presumerli.

Prerequisiti: build fatta (`cargo build`, `cargo build -p ui`, `cargo build -p plugin-calc`) e
`.\deploy_test_run.ps1 -IncludePlugins` eseguito (pubblica anche `lare-shell` in
`Test Run\shell\` e `calc.exe` in `Test Run\plugins\calc\`, senza cui il passo 3 non ha nulla da
rispondere) — vedi `RUN.md` §"Modalità A".

| # | Passo | Atteso | Esito |
|---|---|---|---|
| 1 | Da `Test Run\`: `.\ui.exe` (SENZA `--no-terminal`) | Si apre la finestra terminale (xterm.js dentro ConPTY) con `lare-shell.exe` già dentro — prompt pwsh visibile, banner PSReadLine | **OK.** Lanciato `ui.exe` nudo (nessun orchestratore già attivo): self-heal ha avviato da solo `orchestrator.exe` E `lare-shell.exe` (pty child via ConPTY); banner "Lare Terminal 2.0.0 — sessione \<id\>" (id 8 esadecimali confermato, es. `1fe87d14`), "orchestratore: connesso", prompt PowerShell reale |
| 2 | Digita `/ping` nella shell dentro la finestra | Finestra "Lare — /ping" con la tabella per strato; riga di conferma nel terminale | **OK** (digitato, non tramite pulsante) — confermato dalla riga di ack nel terminale ("→ finestra "Lare - /ping" aperta"); contenuto della finestra non ispezionato visivamente |
| 3 | Digita `/calc` (richiede `-IncludePlugins` nel deploy, vedi sopra) | Il plugin `calc` risponde come da `Test Run\plugins\calc\` | Da eseguire |
| 4 | `/config`, `/library`, `/help` — una volta digitati nella shell, una volta dai 4 pulsanti `.slash-btn` sopra il terminale (`terminal.html`: `/help`, `/library`, `/aichat`, `/config` — NON `/calc`/`/ping`, che non hanno un pulsante dedicato) | Si aprono le finestre corrispondenti in entrambi i casi (digitato e pulsante), stesso esito | Da eseguire — `/help` digitato (non tramite pulsante) verificato a parte: OK, apre la finestra Markdown "Lare — Comandi" col contenuto reale; `/config`/`/library` e tutti i pulsanti `.slash-btn` non sono mai stati testati |
| 5 | `/ai "elenca i 3 file più grandi in questa cartella"` | Gate `[Y/n]` nel terminale → `y` → comando eseguito nel terminale → finestra Markdown col risultato | **OK**, con una AI reale (`llms.json` presente, provider Anthropic) — testato con `/ai "conta quanti file .ps1 ci sono in questa cartella"`: due gate `[Y/n]` in sequenza (ricerca routine, poi comando PowerShell), entrambi accettati, risultato "0", finestra Markdown col titolo della richiesta aperta con l'ack nel terminale |
| 6 | Dopo il comando del passo 5, osserva il segnalino OSC 9001 nella finestra terminale | Il segnalino si accende (evento `intercept` ricevuto da `registerOscHandler(9001, …)`) | **OK.** Verificato che il segnalino "ultimo: /comando" si aggiorna correttamente per `/help`, `/ping` e per `/ai "…"` con l'intero testo tra virgolette |
| 7 | Durante un turno `/ai "…"` lungo (risposta non istantanea) | Il segnalino ActivityIndicator si accende per la durata del turno e si spegne a `Done`/`Error` | **OK.** Confermato visivamente (screenshot) che il pallino si accende durante il turno `/ai` (dal gate in poi) e si spegne dopo `Done` — la catena end-to-end orchestratore→host.js→evento Tauri→terminal.js funziona |
| 8 | Digita `/aichat` due volte di seguito | Si apre una sola finestra AI Chat (singleton), non due | Da eseguire |
| 9 | Chiudi la finestra terminale | Il processo `lare-shell.exe` figlio termina con lei — nessun processo residuo (Task Manager) | **OK, ma SOLO DOPO il fix di chiusura finestra (commit `07c584c`)** — prima di quel fix, `lare-shell.exe` e `ui.exe` restavano entrambi vivi dopo la chiusura (bug reale, trovato proprio da questo passo di e2e). Dopo il fix: verificato con `Get-Process` che `lare-shell.exe` e `ui.exe` terminano correttamente; `orchestrator.exe` resta vivo (corretto, è un daemon indipendente) |
| 10 | Uccidi `orchestrator.exe` PRIMA di avviare `ui.exe`, poi avvia `ui.exe` (modalità A) | Self-heal (Task 3): `ui.exe` avvia da solo `orchestrator.exe`, la finestra terminale funziona normalmente (nessun errore visibile all'utente) | **OK.** Stesso passo del punto 1 (avvio di `ui.exe` nudo) dimostra anche questo |
| 11 | Avvia `ui.exe` senza orchestrator E con `autostart.orchestrator` disattivato in `startup.json` | Messaggio "non raggiungibile" nel terminale; la shell resta comunque utilizzabile per comandi locali (`dir`, `cd`, …) — solo i `/…` falliscono | Da eseguire |

**Verifiche aggiuntive fatte dal vivo, senza una riga dedicata in questa tabella** (spec §10 non
prevedeva righe separate per queste — annotate qui invece di forzarle in righe sopra):

- **Modalità B (`ui.exe --no-terminal`)**: OK. `ui.exe --config-dir … --no-terminal` avvia il
  self-heal dell'orchestratore ma NESSUNA finestra terminale (0 finestre "Lare Terminal" via UIA)
  e NESSUN `lare-shell.exe`.
- **Autostart di `ui.exe` dall'orchestratore (Task 6)**: OK. Avviato SOLO `orchestrator.exe`
  (nessun `ui.exe`), poi inviato un comando `/help` via `node scripts/dev/shell-client.mjs`
  (simula una sessione shell senza `ui.exe`): l'orchestratore ha autostartato da solo
  `ui.exe --no-terminal` (confermato: processo `ui` compare, nessuna finestra terminale, nessun
  `lare-shell.exe`), la finestra "Lare — Comandi" si è aperta davvero, ack "→ finestra aperta"
  (non il fallback `NO_UI_ACK`).
- **Bottone "riavvia" dopo un crash esterno di `lare-shell.exe`**: OK. Verificato anche il difetto
  che questa stessa passata aveva trovato nel banner (bottone dipinto sotto il viewport opaco di
  xterm.js, invisibile/incliccabile — corretto con `z-index: 1` su `#restart-banner`, commit
  `80cd46a`): con la correzione il banner "shell terminata (exit code …)" compare sopra il
  terminale (screenshot confermato), il clic su "riavvia" fa ripartire `lare-shell.exe` (PID
  nuovo, stessa sessione), `term.reset()` pulisce lo schermo, prompt funzionante.

**Righe/passi esplicitamente NON testati dal vivo** (lasciati "Da eseguire", nessun esito
inventato): riga 3 (`/calc`); riga 4 (`/config`/`/library`, né digitati né coi pulsanti; i
pulsanti della barra inferiore in generale — mai cliccati, solo digitazione diretta); riga 8
(`/aichat` due volte → una sola finestra); riga 11 (avvio di `ui.exe` senza orchestratore E con
autostart disattivato); il resize della finestra (nessuna verifica di re-fit).
