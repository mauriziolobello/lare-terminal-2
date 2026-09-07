# Implementation — lare-shell 2.0.0

Piano `Docs/i18n/ita/superpowers/plans/2026-09-06-piano-2b-host-lare-shell.md` (Task 0-8), spec
`Docs/i18n/ita/superpowers/specs/2026-09-04-lare-terminal-2-design.md` §4/§6.4/§7, ADR-015/ADR-019.
`lare-shell` è una host custom del motore PowerShell (come `pwsh.exe`, non un hook su una shell
altrui): implementa `PSHost`, ospita una `Runspace`, e in più parla il canale shell del protocollo
2.1 (piano 2a) con l'orchestratore. Verificabile oggi in **modalità B** (profilo Windows Terminal
"Lare Terminal" — `lare-shell.exe` nudo, spec §2.3); la modalità A (`ui.exe` la lancia dentro una
ConPTY) è piano 3.

## Architettura in tre strati, una riga per classe

### `Config/` — chi sono e dove sono i miei file (nessuna dipendenza dal motore PowerShell)

- **`CliArgs`** — parsing di `--config-dir`/`--session`/`--selftest` (record immutabile).
- **`ConfigDir`** — regola unica di risoluzione (D6, ADR-017): `--config-dir` altrimenti
  `<exe>\..\Configuration\`; `DeployRoot` risale alla radice del deploy (dove vivono
  `orchestrator.exe`/`ui.exe`, un livello sopra `shell\`).
- **`StartupConfig`** — legge `startup.json` (`ws_port`, `autostart.orchestrator`,
  `autostart.ui`); file assente o malformato → default + `Warnings`, mai un'eccezione (spec §9).
- **`TokenFile`** — legge `<config-dir>\token`; `null` (non un'eccezione) se il file non esiste
  ancora — l'orchestratore lo crea al primo avvio, il `Launcher` rilegge a ogni tentativo.
- **`HostLog`** — log su file best-effort `<config-dir>\logs\lare-shell.log`, thread-safe (lock:
  scrivono sia il thread REPL sia il loop di ricezione WS); `HostLog.Null` per i test.

### `Protocol/` — il filo verso l'orchestratore (nessuna dipendenza dal motore PowerShell)

- **`Wire`** — un `record` C# per variante di `ServerMessage` (equivalente dell'enum `ServerMsg`
  Rust: `ServerInfo`, `Chunk`, `Done`, `TurnError`, `ToolConfirmRequest`, `ExecInShell`,
  `Heartbeat`, `Pong`, `Disconnected`, `Unknown`), parse/serializzazione snake_case; costruttori
  statici per i messaggi che la host manda (`Command`, `ToolConfirmResponse`, `ExecResult`,
  `CancelCommand`).
- **`OrchestratorClient`** — connessione WS persistente: `Connect(cwd, timeout)` fa l'handshake
  `hello{role:"shell"}` → `server_info`, poi avvia un loop di ricezione su un thread del pool che
  parsa ogni messaggio e lo **accoda** in un `Channel<ServerMessage>` (`Incoming`) — è l'unico
  ruolo del thread del socket: non tocca mai console né runspace. Nessuna riconnessione automatica
  in background: chi lo usa (`Repl`/`Launcher`) richiama `Connect` quando serve (ruling 2).

### `Host/` — l'implementazione di `PSHost` (dipende dal motore, non dal filo WS)

- **`IConsoleModes`** / **`Win32ConsoleModes`** — confine d'astrazione sulle P/Invoke
  `GetConsoleMode`/`SetConsoleMode`; un fake nei test registra le mode senza una console vera
  (stesso ruolo di un trait Rust con un fake al seam).
- **`ConsoleModes`** — le `[DllImport]` vere verso `kernel32.dll` (solo Windows), usate sia da
  `Repl` (VT processing per l'OSC 9001) sia da `Win32ConsoleModes`.
- **`LareHost`** — implementazione minima di `PSHost` (il ruolo di `ConsoleHost.cs` nel repo
  PowerShell di riferimento): `Name`/`Version` da `HostInfo`, `SetShouldExit`/`ShouldExit`/
  `ExitCode` (l'`exit` digitato o dentro un comando dell'AI), `NotifyBegin/EndApplication`
  (salva/ripristina le console mode attorno a un programma esterno, lezione spike 1).
- **`LareHostUI`** — implementazione di `PSHostUserInterface`: dove il motore scrive/legge
  (`Write*`, `Read-Host`, il formatter di `Out-Default`); possiede l'`OutputRecorder`.
- **`LareRawUI`** — implementazione di `PSHostRawUserInterface` (colori, cursore, dimensioni
  finestra, `ReadKey` per PSReadLine): ogni accesso a `System.Console` è in try/catch, perché con
  stdin/stdout rediretto (`--selftest`, una pipe) molte proprietà lanciano invece di dare un default.
- **`OutputRecorder`** — registra il testo scritto tramite `LareHostUI` durante un `ExecInShell`
  con `capture:true`; cap "200 KB testa+coda" in **caratteri**, non byte (ruling 6), con un
  marcatore che dice quanti ne sono stati omessi; thread-safe.

### `Shell/` — la logica applicativa (usa Config/Protocol/Host, non li implementa)

- **`PwshLocator`** — trova la cartella di installazione di pwsh (PATH → registro
  `HKLM\SOFTWARE\Microsoft\PowerShellCore\InstalledVersions\*` → `C:\Program Files\PowerShell\7`):
  serve per anteporre `<pwsh>\Modules` al `PSModulePath` del processo, perché il NuGet
  `Microsoft.PowerShell.SDK` non include PSReadLine.
- **`RunspaceSession`** — apre e possiede la `Runspace` (`Dispose` la chiude; va aperta sul thread
  del REPL, `Runspace.DefaultRunspace` è per-thread), espone `PsReadLineAvailable`,
  `EvaluatePrompt()` (invoca la funzione `prompt` dell'utente), `ReadLine(usePsReadLine)`,
  `CurrentDirectory`.
- **`ProfileLoader`** — calcola i quattro percorsi `$PROFILE` con la stessa convenzione di pwsh
  (`Documents\PowerShell`, nome file da `$Host.Name`), li espone come `$PROFILE` (stringa +
  4 `NoteProperty`, perché in una host custom `$PROFILE` non è popolato dal motore —
  `HostUtilities.GetDollarProfile` è `internal`), carica in ordine `profile.ps1` →
  `Microsoft.PowerShell_profile.ps1` (quello di **pwsh**: alias/oh-my-posh/moduli dell'utente
  appaiono in Lare come in pwsh) → `LareShell_profile.ps1`. AllUsers non caricati (debito).
- **`IExecutor`** / **`Executor`** — esegue un comando nel runspace dell'utente (§4.5): pipeline
  `<cmd> | ForEach-Object { $_ } | Out-Default` per `capture:true` (il cmdlet in mezzo impedisce a
  un nativo di essere l'ultimo della pipeline, così il suo stdout passa da `Out-Default` →
  `LareHostUI.Write` → `Recorder`), `<cmd> | Out-Default` puro per `capture:false` (console
  attaccata, editor/REPL/wizard funzionano, `output` vuoto). Il comando dell'AI gira dentro
  `try { <cmd> } finally { $global:__lare_ok = $? }` (non un append diretto — vedi "Ruling" sotto
  per il perché); `$LASTEXITCODE` azzerato prima; `exit_code` = 0 se `$?` catturato è vero,
  altrimenti `$LASTEXITCODE` se ≠ 0, altrimenti 1. `RunInteractive` (comando digitato al prompt) è
  una pipeline pura, senza cattura né istruzioni aggiunte, per non alterare `$?`/`$LASTEXITCODE`
  che il prompt/oh-my-posh dell'utente potrebbero leggere. `StopCurrent()` chiama
  `PowerShell.Stop()`: su SDK 7.6.5 `Invoke()` torna normalmente con
  `InvocationStateInfo.State == Stopped` (non un'eccezione — scoperta del Task 4, il catch
  `PipelineStoppedException` resta per coprire altri SDK/percorsi).
- **`IGate`** / **`ConsoleGate`** — il prompt `[Y/n]` del gate ADR-007: lettura tasto diretta (buffer svuotato prima del prompt, tasti loggati)
  (`Console.ReadKey`, non PSReadLine), polling `KeyAvailable` ogni 50 ms così può accorgersi
  (`shouldAbandon`) che il turno è già finito altrove; con stdin rediretto (pipe/test manuale)
  ricade su `Console.ReadLine` e **fallisce chiuso** su EOF (nessun umano a rispondere → rifiuta,
  mai accetta).
- **`SlashTurn`** — il ciclo di UN turno slash (§4.2), interamente sul thread del REPL: manda
  `Command`, poi consuma `OrchestratorClient.Incoming` finché il turno non finisce, reagendo a
  `ToolConfirmRequest` (gate), `ExecInShell` (`Executor.Run`), `Chunk` (stampa), `Done`/`TurnError`
  (fine), `Disconnected`. Vedi "Flusso di un turno" sotto per il dettaglio.
- **`IProcessStarter`** / **`ProcessStarter`** — confine d'astrazione sui processi figli:
  `UseShellExecute = true` (nessun handle ereditato — equivalente pratico di `DETACHED_PROCESS`),
  `WindowStyle` parametrico (`hideWindow`) per nascondere la console del figlio.
- **`Launcher`** — self-heal (§6.4): `EnsureConnected` riprova la connessione e, se manca e
  `autostart.orchestrator` è attivo, avvia `orchestrator.exe --config-dir …` e ritenta per 5 s;
  `EnsureUi()` avvia `ui.exe --config-dir …` se manca e non è già in esecuzione. **Entrambi** i
  figli partono con `hideWindow: true` (ADR-019 punto 6, rivisto durante l'e2e — vedi "Debiti").

### Radice (`src/LareShell/`) — composizione ed entry point

- **`HostInfo`** — `Name`/`Version` (`"2.0.0"`) in un solo posto: li usano `PSHost`, `Hello.version`
  (mostrato da `/ping`), il nome del profilo `LareShell_profile.ps1`, il banner del REPL.
- **`SlashLine`** — riconoscimento puro di una riga `/…` (prima cosa non-spazio è `/`) e
  normalizzazione (`Trim`); il resto del routing (`/ai` con virgolette, slash noto/ignoto) lo fa
  l'orchestratore, non la host.
- **`Osc`** — la sequenza OSC 9001 `intercept` per l'emulatore (§4.7): `Esc` costruito da
  `(char)0x1B`, **mai** da un escape di stringa (`"\x1b…"` è "goloso": mangia le cifre esadecimali
  che seguono — bug dello spike 1); `SourceScanTests` fallisce se `"\x1b"` ricompare nei sorgenti.
- **`Repl`** — il ciclo read-eval-print-loop: prompt → riga (PSReadLine se disponibile e stdin non
  rediretto) → `/…` a `SlashTurn`, resto a `Executor.RunInteractive`; l'handler di `Ctrl+C`
  (`Console.CancelKeyPress`, altro thread) ferma la pipeline corrente e cancella il turno slash in
  corso via un `CancellationTokenSource` **senza `using`** (vedi "Debiti").
- **`SelfTest`** — `--selftest`: controlli non interattivi (niente tastiera, niente orchestratore)
  con exit code 0/1, una riga `[OK]`/`[FAIL]` per controllo — per verificare un deploy o in CI.
- **`Program`** — entry point: solo composizione (config → log → runspace → client → launcher →
  `Repl`), nessuna logica propria — ogni pezzo resta testabile da solo.

## L'invariante a thread singolo

**Un solo thread (il REPL) tocca la `Runspace` e la console.** Il thread del socket
(`OrchestratorClient`) fa SOLO due cose: manda messaggi (`Send`) e accoda quelli ricevuti in un
`Channel` (`Incoming`) — non chiama mai `Executor.Run`, non scrive mai sulla console, non tocca la
`Runspace`. Il REPL consuma quel canale in modo bloccante (`Incoming.ReadAsync(ctrlC)`) quando è
dentro `SlashTurn.Run`: è così che un `ExecInShell` "arrivato dal thread socket" finisce comunque
eseguito dal thread REPL (spec §10, test di marshaling). L'unica eccezione a "un solo thread" è
l'handler di `Console.CancelKeyPress`, che gira su un thread separato ma NON tocca runspace/console
direttamente: si limita a chiamare `Executor.StopCurrent()` (thread-safe, `PowerShell.Stop()`) e a
cancellare un `CancellationTokenSource` (thread-safe per design) — mai un'operazione sincrona sulla
`Runspace`.

## Flusso di un turno `/ai` (spec §4.2, con i nomi delle classi)

```
utente  → digita "/ai \"…\"" + Invio                         Repl.Run (loop principale)
Repl    → SlashLine.IsSlash/Normalize → Repl.RunSlash          Osc.Intercept sulla console
Repl    → EnsureConnected (Launcher) · EnsureUi() (self-heal)
Repl    → new SlashTurn(...).Run(input, cwd, ctrlC)
SlashTurn → Wire.Command(id, input, cwd) → OrchestratorClient.Send
SlashTurn → loop: OrchestratorClient.Incoming.ReadAsync (bloccante, sveglia da Ctrl+C)
  ToolConfirmRequest  → IGate.Ask → ToolConfirmResponse (Accept/Reject) o Cancel (Ctrl+C)
  ExecInShell (turn_id == id) → IExecutor.Run(command, capture) → Wire.ExecResult
  Chunk (id == id)    → stampa su Console.Out
  Done/TurnError (id) → fine turno (Completed/Failed)
  Disconnected        → fine turno (Disconnected)
Repl    → torna al prompt (`_session.EvaluatePrompt()`)
```

Il REPL è bloccato per tutta la durata (`SlashTurn.Run` non ritorna finché il turno non finisce),
come per qualunque comando: PSReadLine non legge finché non è finito. Il thread WS resta reattivo
nel frattempo — accoda semplicemente in `Incoming` — ma è `SlashTurn` che decide quando e come
reagire, sempre dal thread del REPL.

## Come si testa

```powershell
dotnet test shell/lare-shell/LareShell.sln           # 115 test
dotnet test shell/lare-shell/LareShell.sln --filter "FullyQualifiedName~Executor"
```

- **Collection `runspace`** (`Shell/RunspaceCollection.cs`, `[CollectionDefinition("runspace",
  DisableParallelization = true)]`): i test che aprono una `Runspace` vera (`RunspaceSessionTests`,
  parte di `ExecutorTests`/`ProfileLoaderTests`) girano in **serie**, perché
  `Runspace.DefaultRunspace` è per-thread e `RunspaceSession.Open` modifica il `PSModulePath` del
  processo (stato globale, non isolabile fra thread paralleli) — tutto il resto dei 115 test resta
  parallelo (xUnit di default).
- **`FakeOrchestrator`** (`tests/.../Protocol/FakeOrchestrator.cs`) è un server WebSocket in-process
  su **`TcpListener`**, non `HttpListener`: `HttpListener` registra prefissi in `http.sys` che
  sopravvivono a un processo di test caduto a metà (URL ACL, richiedono privilegi) e, durante
  questa sessione, un probe precedente ha lasciato una registrazione appesa che ha fatto fallire il
  rerun ("conflicts with an existing registration"). `TcpListener` su porta 0 + handshake HTTP di
  upgrade scritto a mano (RFC 6455 §4.2.2: risposta 101 con `Sec-WebSocket-Accept =
  base64(SHA1(key + GUID magico))`) + `WebSocket.CreateFromStream` non lascia stato di processo:
  ogni test parte pulito.
- `ConsoleGate` e `Repl` **non hanno test automatici**: dipendono da una console interattiva vera
  (`Console.ReadKey`, `Console.CancelKeyPress`) — coperti solo dall'e2e manuale (`TESTING-e2e.md`
  Parte 6). `SlashTurn`/`Executor`/`Launcher`/`OrchestratorClient` sono invece test-first puri
  (fake su ogni confine: `IGate`, `IExecutor`, `IProcessStarter`, `FakeOrchestrator`).

## Ruling del piano (1-9, motivazione completa nel piano)

1. Server finto dei test su `TcpListener` + upgrade a mano, mai `HttpListener` (vedi sopra).
2. Riconnessione **on demand**: nessun task in background; ogni `/…` verifica la connessione e la
   ristabilisce (con autostart) se serve — soddisfa "riconnessione automatica" (spec §9) con molta
   meno concorrenza.
3. `exit_code`: 0 se `$?` (catturato in coda allo stesso script) è vero, altrimenti
   `$LASTEXITCODE` se ≠ 0, altrimenti 1; i comandi digitati dall'utente non toccano `$?`/
   `$LASTEXITCODE` (il prompt li deve vedere intatti).
4. Processi figli con `UseShellExecute = true`; **entrambi** con finestra nascosta (rivisto durante
   l'e2e — la formulazione originale del piano lasciava `ui.exe` `Normal`, vedi "Debiti" e ADR-019).
5. `ToolConfirmRequest` non porta `turn_id`: attribuita al turno corrente durante l'attesa; i
   messaggi di turni già chiusi vengono scartati a inizio turno (`SlashTurn.DiscardStale`).
6. Cap dell'output in **caratteri** (200·1024), non byte.
7. Log della host senza rotazione (debito, la spec chiede rotazione giornaliera).
8. `ui.exe` avviata dopo la prima connessione riuscita e **ricontrollata prima di ogni turno slash**
   (self-heal: se l'utente l'ha chiusa, il prossimo `/…` la riapre).
9. `deploy_test_run.ps1` pubblica la host di default (framework-dependent `win-x64`); `-SkipShell`
   per saltarla quando si ricompila solo il Rust.

## Debiti (dettaglio in `Docs/i18n/ita/KNOWN-ISSUES.md` e `HANDOFF.md`)

- Profili **AllUsers** (nel `$PSHOME` di pwsh) non caricati — solo CurrentUser (`ProfileLoader`).
- Log della host senza rotazione (ruling 7).
- `$?` nel prompt dell'utente è **sempre vero** dopo un turno slash (ruling 3: solo i comandi
  dell'AI toccano `$global:__lare_ok`, i comandi digitati no — ma `$?` a livello di `Repl` non è
  mai stato sporcato da un turno slash per costruzione, quindi resta quello dell'ultimo comando
  digitato, non un dato del turno).
- `ToolConfirmRequest` senza `turn_id` (ruling 5) — attribuzione al turno corrente, non a un id
  verificato.
- `PromptForCredential`/`PromptForChoice` non implementati in `LareHostUI` (nessun cmdlet della v1
  li usa oggi; un comando che li chiamasse fallirebbe).
- Nessun test automatico di `ConsoleGate`/`Repl` — richiedono una console interattiva vera, coperti
  solo dall'e2e (`TESTING-e2e.md` Parte 6).
- `StopCurrent()` ha una finestra TOCTOU: fra l'assegnazione di `_current` e `Invoke()`, un Ctrl+C
  arrivato in quella finestra non trova nulla da fermare (no-op silenzioso, nessun crash).
- `TerminalPending` (in `SlashTurn`) guarda solo la **testa** della coda (`TryPeek`), non l'intera
  coda — ma è chiamata SOLO come `shouldAbandon` dentro l'attesa del gate (`ToolConfirmRequest`),
  quindi non esiste un "prompt appeso" fuori da quel caso. Guardare solo la testa basta, dato il
  contratto reale: un solo turno alla volta (contratto a) esclude che in testa ci sia un messaggio
  di un ALTRO turno mentre si aspetta il gate di QUESTO; e per un turno gateizzato,
  `route_shell_turn` manda alla shell esattamente un `Chunk` di ack seguito subito dal `Done` (mai
  altro testo nel mezzo — fix F3, revisione finale piano 2b), quindi trovare quell'ack in testa
  basta per abbandonare il gate senza aspettare che arrivi anche il `Done` — vedi il doc-comment di
  `TerminalPending`. Nessun residuo pratico noto.
- Gate con stdin rediretto blocca dentro `Console.ReadLine` (nessun polling possibile su una pipe).
- `exit` dentro un comando dell'AI **non** è fermato dal `try/finally`: termina il processo come se
  l'utente l'avesse digitato al prompt (il gate ha già mostrato il comando prima dell'esecuzione:
  limite dichiarato, non un buco di sicurezza).
- I `Modules` inclusi nel NuGet `Microsoft.PowerShell.SDK` restano sotto
  `runtimes\win-x64\lib\net10.0\Modules\` (mai usati: i moduli "standard" (PSReadLine compreso)
  vengono dalla cartella `Modules` di **pwsh**, anteposta al `PSModulePath` del processo da
  `PwshLocator`/`RunspaceSession.Open`) — ridondanza nel publish, non un bug funzionale.
- Ctrl+C su un comando digitato: verificato dal vivo in Windows Terminal (ConPTY) — il log riporta
  `Ctrl+C ricevuto (turno in corso: False)` e il terminale `"[LARE] comando interrotto (Ctrl+C)."`.
  Il caso "riga mancante" osservato nelle prime passate dell'e2e (host in `conhost`, Ctrl+C
  sintetico via `SendKeys`/`GenerateConsoleCtrlEvent`) era il driver: lo stesso evento non
  interrompeva nemmeno un `pwsh` di controllo.
- `#pragma warning disable xUnit1031` in `SlashTurnTests` (i test del turno sono sincroni per
  mandato del piano: bloccano di proposito sul thread di test, come farebbe il thread REPL vero).
- Il fragment del profilo Windows Terminal (`install-wt-profile.ps1`) è letto solo all'**avvio** di
  Windows Terminal: installarlo/aggiornarlo richiede di riavviare WT (chiudere tutte le finestre),
  non solo aprire una nuova scheda.
- `ui.exe` in build **debug** è un'app console (Tauri tiene la console per i log): senza
  `hideWindow`, con Windows Terminal come terminale predefinito, quella console si apre come una
  scheda WT e ruba il fuoco — per questo `Launcher.EnsureUi()` nasconde anche `ui.exe` (ruling 4
  riveduto). Il commento su `ProcessStarter` è stato allineato (revisione finale piano 2b): descrive
  ora entrambi i figli avviati con console nascosta, coerente con `Launcher.EnsureUi`.
