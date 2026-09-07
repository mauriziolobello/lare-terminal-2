# TESTING-e2e — checklist end-to-end

Verifica dal vivo (non automatizzabile: processi reali, finestre reali) che il deploy funzioni
davvero — non solo che i test unitari passino. Parti 1-4 riproducono quanto verificato dal vivo a
chiusura del **piano 1** ("fondamenta": deploy in `Test Run\`, avvio di orchestrator e `ui.exe`);
Parte 5 aggiunge il canale shell del **piano 2a** (client di sviluppo, senza la host reale); Parte
6 aggiunge la host C# vera del **piano 2b** in modalità B (Windows Terminal). Compila la colonna
**Esito** eseguendo i passi in ordine, da una macchina pulita se possibile (nessun
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

## Parte 6 — Modalità B in Windows Terminal (host `lare-shell`)

Verifica dal vivo del **piano 2b** (ADR-015/ADR-019): la host C# vera, aperta come profilo
Windows Terminal "Lare Terminal" (`install-wt-profile.ps1`), con `orchestrator.exe`/`ui.exe`
**non** già in esecuzione prima di aprire la scheda (`Get-Process orchestrator, ui
-ErrorAction SilentlyContinue | Stop-Process`). Eseguita dal controller il 2026-09-07 (nessun
terminale interattivo lato implementer; driver SendKeys + screenshot) — esito integrale nel
ledger (`.superpowers/sdd/2026-09-06-piano-2b-host-lare-shell/progress.md`, sezione "E2E dal vivo
(controller)"). Trascritto qui fedelmente; dove il ledger non annota un passo in modo esplicito,
la colonna Esito dice "non verificato" piuttosto che presumerlo.

**Limite della macchina**: nessuna `ANTHROPIC_API_KEY` né `Configuration\llms.json` →
l'orchestratore usa `StubAdapter` (nessuna tool call reale, quindi nessun gate ADR-007 reale). I
punti 5-9 (gate di conferma su un comando proposto dall'AI, `ExecInShell`, Ctrl+C durante
un'esecuzione avviata dall'AI, REPL interattivo via AI) **non sono verificabili dal vivo su questa
macchina** — coperti dai test automatici: `SlashTurnTests` (10 casi), `ExecutorTests` (13 casi),
`ws_integration.rs` del piano 2a.

**Due passate**: la prima ha trovato un difetto (§"Difetti trovati" sotto), corretto con il fix
`a8d148f` (2 file + test nuovi, 115/115); la seconda, dopo la ripubblicazione di `Test Run\`, ha
riverificato i punti toccati dal fix (autostart, riconnessione).

| # | Verifica | Esito |
|---|---|---|
| 1 | Banner all'apertura della scheda; "orchestratore: NON connesso … avvio … orchestrator.exe" → "orchestratore avviato e connesso"; `ui.exe` compare (autostart §6.4) | OK — autostart orchestratore (~1 s) e `ui.exe`. Prima passata: difetto, `ui.exe` (app console in build debug) apriva una SCHEDA Windows Terminal e rubava il fuoco (vedi difetto 1 sotto). Seconda passata, dopo il fix: OK, autostart senza scheda WT spuria |
| 2 | Prompt/profilo come in pwsh (PSReadLine, alias, oh-my-posh se presente); `Get-Date`, `dir`, `cd ..` funzionano | OK banner/PSReadLine/`Get-Date`; OK `cd ..` persiste nel prompt (D17). `dir` non risulta annotato esplicitamente nel ledger come passo a sé — non verificato in modo distinto |
| 3 | `/ping` → finestra "Lare — /ping" con le righe per strato + riga di conferma nel terminale | OK (finestra + riga di conferma); riverificato nella seconda passata come parte della verifica di riconnessione (punto 10) |
| 4 | `/help`, `/config`, `/library` → finestre giuste; `/nonesiste` → muto; `/ai x` senza virgolette → errore di sintassi | OK `/help` → "Lare — Comandi"; OK `/config`; OK `/library` → "Lare — Archivio" (seconda passata); OK `/nonesiste` muto (log discard); OK `/ai x` → riga di errore di sintassi |
| 5 | `/ai "elenca i 3 file più grandi in questa cartella"` → `[Y/n]` → Invio → comando eseguito NEL terminale → finestra Markdown col risultato | **Non verificabile dal vivo** (nessuna chiave AI → `StubAdapter`, nessuna tool call quindi nessun gate reale) — coperto da `SlashTurnTests`/`ExecutorTests`/`ws_integration.rs`. Verificato invece, con lo `StubAdapter`: `/ai "ciao, chi sei?"` completa un turno e apre una finestra |
| 6 | `/ai "vai nella cartella Documents"` → il prompt dopo mostra `Documents` (cwd persiste, D17) | **Non verificabile dal vivo** (richiede una tool call AI reale) — coperto dai test automatici (cwd per sessione, `ws_integration.rs`). La persistenza della cwd per un comando DIGITATO (`cd ..`) è invece verificata dal vivo (punto 2) |
| 7 | `/ai "cancella tutti i file temporanei"` → `n` al gate → nessun comando eseguito, il turno finisce | **Non verificabile dal vivo** (nessun gate reale senza tool call AI) — coperto da `SlashTurnTests` (percorso Reject) |
| 8 | Ctrl+C in attesa del turno AI, e Ctrl+C durante un `ExecInShell` lungo avviato dall'AI → "annullato (Ctrl+C)"/"comando interrotto (Ctrl+C): turno annullato" | **Non verificabile nella forma esatta del punto** (richiede un turno AI reale). Verificato invece Ctrl+C su un comando DIGITATO (`Start-Sleep`): torna al prompt correttamente, **ma la riga "[LARE] comando interrotto (Ctrl+C)." non è comparsa** (KNOWN-ISSUE, causa non investigata); il meccanismo (`PowerShell.Stop()` → `InvocationStateInfo.State == Stopped`) resta comunque coperto da `ExecutorTests` |
| 9 | `/ai "apri python in modo interattivo"` → REPL python utilizzabile, `exit()` torna al prompt | **Non verificabile dal vivo** (richiede una tool call AI con `interactive: true`) — coperto dai test automatici di `Executor`/`ws_integration.rs` (percorso `capture:false`) |
| 10 | Chiudi `ui.exe` → `/help` la riavvia (self-heal, ruling 8); uccidi l'orchestratore (`Stop-Process -Name orchestrator`) → `/ping` → "orchestratore non raggiungibile … avvio …" → riconnesso e finestra aperta | Parzialmente verificato. OK: "orchestratore ucciso → `/ping` riavvia e riconnette (stessa sessione)", finestra `/ping` aperta (seconda passata). La chiusura manuale di `ui.exe` seguita da `/help` per il self-heal non risulta annotata come passo distinto — non verificato esplicitamente (l'autostart di `ui.exe` all'apertura della scheda è invece verificato, punto 1) |
| 11 | `exit` → la scheda si chiude; `Get-Process lare-shell` → nulla; `Get-Process orchestrator, ui` → ANCORA vivi (processi staccati) | OK, verificato **due volte**: prima passata, "orchestrator e ui sopravvivono (anche quando la host girava in una scheda WT chiusa)"; seconda passata, "orchestrator e ui vivi" dopo `exit`. Il comando `Get-Process lare-shell` non risulta trascritto nel ledger come eseguito alla lettera — la sopravvivenza dei due processi staccati è comunque confermata in entrambe le passate |
| 12 | Copia `Test Run\` in `%TEMP%\LareCopia\`, esegui `LareCopia\shell\lare-shell.exe --selftest` → `[OK]` con la Configuration di `LareCopia` (percorsi relativi, §6.3) | **Non verificato**: questo scenario (copia in un'altra cartella, percorsi relativi) non risulta nel ledger dell'e2e dal vivo di oggi. `Test Run\shell\lare-shell.exe --selftest` sul deploy originale (non copiato) è stato eseguito con esito `[OK]` su ogni controllo (Task 8, riverificato dal controller) |

### Difetti trovati durante l'e2e (prima passata) e correzioni

1. **`ui.exe` rubava il fuoco alla shell.** In build debug `ui.exe` è un'app console (Tauri tiene
   la console per i log); avviata con `WindowStyle.Normal` e Windows Terminal come terminale
   predefinito, quella console si apriva come una NUOVA SCHEDA di WT. Fix: `Launcher.EnsureUi()`
   avvia anche `ui.exe` con `hideWindow: true` (verificato: le finestre vere create da `ui.exe` —
   Markdown, `/config`, `/library` — restano visibili; solo la console di debug resta nascosta).
   Vedi ADR-019 punto 6 (rivisto) e `HANDOFF.md`.
2. **Fragment del profilo WT**: `commandline` senza virgolette (come nello spike) — le virgolette
   non erano la causa del sintomo osservato (era il driver SendKeys), ma la forma senza virgolette
   resta quella collaudata e non è stata cambiata.
3. **Il fragment richiede il riavvio di Windows Terminal.** La finestra WT dell'utente era già
   aperta quando il fragment è stato installato → il profilo "Lare Terminal" non era ancora
   caricato in quella finestra → la **prima passata** dell'e2e è stata condotta in una finestra
   **conhost** (non WT), con `ui.exe` avviata a mano (`-WindowStyle Hidden`) invece che dal
   profilo. La **seconda passata**, dopo il fix e la ripubblicazione di `Test Run\`, ha riverificato
   autostart e riconnessione.

Fix wave e2e: commit `a8d148f` (2 file + test nuovi, 115/115).

**Nota per Maurizio (incidente durante l'e2e).** Durante la prima passata, guidata via SendKeys,
il controller ha chiuso per errore una scheda "PowerShell" nella finestra Windows Terminal
dell'utente (verosimilmente una scheda dell'utente stesso, non aperta da questo e2e) — nessun dato
recuperabile da questa sede, va solo segnalato.
