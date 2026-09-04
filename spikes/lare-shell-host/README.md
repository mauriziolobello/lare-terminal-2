# lare-shell-spike

Spike tecnico (throwaway, non è codice di produzione) per **Lare Terminal 2.0**:
un host custom di PowerShell scritto in C#, che incorpora il motore PowerShell
(`Microsoft.PowerShell.SDK`), legge le righe con **PSReadLine** come fa
`pwsh.exe`, intercetta le righe che iniziano con `/` senza eseguirle, esegue
tutto il resto nella runspace ospitata, e disegna una barra di stato fissa
(riga in alto e riga in basso) usando sequenze VT/ANSI.

Obiettivo dello spike: capire quali API pubbliche di `System.Management.Automation`
servono per costruire un host custom "alla ConsoleHost", e quali sono i punti
delicati (execution policy, PSReadLine + input rediretto, VT processing +
scroll region concorrente con l'editor di riga).

## Come compilare

```powershell
cd 'C:\Users\Maurizio\Documents\Progetti\Lare Terminal 2.0\spikes\lare-shell-host'
dotnet build -c Release
```

L'eseguibile finisce in `bin\Release\net10.0\lare-shell-spike.exe`.

## Come eseguire

Modalità interattiva (REPL con status bar, PSReadLine, ecc.):

```powershell
dotnet run -c Release
# oppure, dopo la build:
.\bin\Release\net10.0\lare-shell-spike.exe
```

Modalità self-test (nessuna console interattiva richiesta, pensata per CI/verifica rapida):

```powershell
dotnet run -c Release -- --selftest
```

## Cosa dimostra ciascun test

### `--selftest`

Crea la stessa runspace ospitata usata dal REPL (stesso `LareHost`, stesso
`InitialSessionState` con PSReadLine importato) e verifica, senza bisogno di
un terminale interattivo:

1. **Apertura della runspace** con `LareHost` + modulo PSReadLine importato.
   Questo test include anche l'impostazione esplicita di `ExecutionPolicy =
   RemoteSigned` sull'`InitialSessionState`: senza questa riga, il caricamento
   di `PSReadLine.psm1` fallisce con *"running scripts is disabled on this
   system"*, perché un processo host che non è `pwsh.exe` eredita di default
   una execution policy più restrittiva.
2. **Presenza della funzione `PSConsoleHostReadLine`** dopo l'import di
   PSReadLine — è la stessa funzione che il REPL invoca ad ogni riga per
   leggere l'input con editing avanzato (history, syntax highlighting, ecc.),
   esattamente come fa `ConsoleHostUserInterface.TryInvokeUserDefinedReadLine`
   nel `ConsoleHost` reale. Stampa anche `Get-Module PSReadLine` per mostrare
   **da dove** è stato caricato il modulo (il `PSModulePath` di una runspace
   ospitata non è garantito essere lo stesso di `pwsh.exe`).
3. **`Get-ChildItem $env:TEMP | Select-Object -First 3`**, catturando gli
   oggetti risultato lato .NET (`PowerShell.Invoke()`) e contandoli: dimostra
   che l'host può eseguire comandi ed "vedere" gli oggetti prodotti, non solo
   il testo che verrebbe stampato a schermo.
4. **Intercettazione delle righe `/...`** tramite `Repl.TryIntercept`, la
   *stessa identica funzione* usata dal loop interattivo (non una copia
   duplicata): dimostra che REPL e self-test condividono la logica di
   intercettazione. Include anche un controllo simmetrico che una riga senza
   `/` NON venga intercettata.

Ogni controllo stampa una riga `[OK]`/`[FAIL]`; il processo esce con codice 0
se tutti passano, 1 altrimenti.

### REPL interattivo

Avviandolo senza `--selftest` si ottiene un prompt PowerShell quasi normale:

- `prompt` viene valutato invocando la funzione `prompt` nella runspace (con
  fallback su `"PS <cwd>> "` se fallisce o non è definita), come fa
  `ConsoleHost.EvaluatePrompt()`.
- Ogni riga viene letta invocando `PSConsoleHostReadLine` nella runspace (se
  disponibile), quindi con lo stesso editing/history/syntax-highlighting che
  avresti in `pwsh.exe`.
- Le righe che iniziano con `/` (dopo trim) vengono **intercettate**: stampate
  in ciano con prefisso `[LARE] intercettato: ...` e **non eseguite** in
  PowerShell. `/exit` esce dallo shell.
- Tutte le altre righe vengono eseguite nella runspace con
  `AddScript(riga).AddCommand("Tee-Object").AddCommand("Out-Default")`: l'utente
  vede l'output normale (formattato, a colori, in streaming) e in più una riga
  informativa `[LARE] oggetti prodotti: N` che conta quanti oggetti .NET sono
  transitati nella pipeline (esclusi gli `ErrorRecord`, che vengono mostrati
  come errori normali grazie al merge error→output, stesso pattern usato da
  `ConsoleHost`/`Executor.cs`).
- Ctrl+C interrompe solo il comando in corso (`PowerShell.Stop()`), non il
  processo.
- Digitare `exit` (senza `/`) esegue il comando PowerShell `exit`, che chiama
  `LareHost.SetShouldExit`: il loop se ne accorge e termina in modo pulito.
- Una barra di stato in alto (spinner + orologio + ultimo comando
  intercettato) e una in basso (menu `/help  /library  /aichat  /config`, con
  `/library` come vero hyperlink OSC 8) restano fisse mentre l'output scorre,
  grazie a una scroll region VT (`DECSTBM`, `ESC[2;{H-1}r`).

## Verifiche eseguite e risultati osservati

- **Build**: `dotnet build -c Release` completa con 0 errori (5 warning
  `CA1416`, tutti relativi a membri `System.Console` disponibili solo su
  Windows — accettabile, questo spike è Windows-only).
- **`--selftest`**: tutti i controlli `[OK]`, exit code 0. PSReadLine
  risulta caricato da `C:\program files\powershell\7\Modules\PSReadLine\
  PSReadLine.psm1`, versione 2.4.5 — cioè la copia "vera" di PSReadLine
  installata per PowerShell 7 su questa macchina, non una copia più vecchia
  di Windows PowerShell.
- **Esecuzione con stdin rediretto** (`echo "/exit" | lare-shell-spike.exe`):
  **osservazione importante**. Il primo tentativo, in cui il codice provava
  sempre a invocare `PSConsoleHostReadLine` (quindi PSReadLine) anche con
  stdin rediretto, è entrato in un **loop infinito**: PSReadLine non
  restituiva mai EOF né consumava le righe della pipe, ristampando il prompt
  all'infinito (il processo è dovuto essere terminato da un timeout esterno,
  exit code 124, dopo aver prodotto svariati MB di output ripetuto). Questo
  riproduce un problema noto: PSReadLine è pensato per un vero terminale
  interattivo, non per stdin rediretto. Il `ConsoleHost` reale evita
  esplicitamente questo scenario: `LoadPSReadline()` in `ConsoleHost.cs`
  controlla (fra le altre cose) se "stdin is redirected by a parent process" e
  in tal caso NON carica/usa PSReadLine. Abbiamo replicato la stessa guardia
  (`Console.IsInputRedirected`): con la guardia, `echo "/exit" | ...` stampa
  `[LARE] stdin rediretto: PSReadLine non verrà usato...`, usa
  `Console.ReadLine()` come fallback, intercetta `/exit` correttamente ed esce
  con **exit code 0**, senza hang.
- **Esecuzione di comandi reali via pipe** (`Get-ChildItem`, `Get-Date`, un
  comando inesistente): verificato manualmente che l'output formattato a
  colori, il conteggio oggetti (`[LARE] oggetti prodotti: N`) e la
  visualizzazione degli errori in rosso funzionano correttamente end-to-end.
- **Barra di stato fissa mentre PSReadLine ha il controllo della console
  interattiva** (la parte esplicitamente indicata come "rischiosa" nella
  consegna): **non verificabile in questa sessione**, perché non c'è un
  terminale interattivo reale disponibile per l'agente che ha scritto questo
  spike — va provata a mano da una persona, con Windows Terminal, osservando
  se il refresh dello spinner ogni secondo corrompe visivamente l'editing di
  riga di PSReadLine (posizione del cursore, ridisegno della riga durante
  history/tab-completion, ecc.).

## Come rimuovere il profilo di Windows Terminal

Se hai eseguito `install-wt-profile.ps1` (lo script NON viene eseguito
automaticamente da questo spike, va lanciato a mano):

```powershell
.\uninstall-wt-profile.ps1
```

Rimuove solo il file
`%LOCALAPPDATA%\Microsoft\Windows Terminal\Fragments\Lare\lare-terminal-spike.json`
(e la cartella `Lare` se resta vuota): nessun'altra modifica al sistema o a
`settings.json`.

## Semplificazioni note / membri non implementati

- `LareHost.EnterNestedPrompt`/`ExitNestedPrompt`: `NotImplementedException`
  — nessun supporto al debugger PowerShell (breakpoint, `[DBG]:` prompt).
- `LareHostUI.PromptForCredential` (entrambi gli overload):
  `NotImplementedException` — richiederebbe UI dedicata per
  mascheramento/credential manager, fuori scopo per lo spike.
- `LareRawUI.GetBufferContents`, `SetBufferContents(Coordinates, BufferCell[,])`,
  `ScrollBufferContents`: `NotImplementedException`. L'unico caso gestito di
  `SetBufferContents` è quello usato da `Clear-Host`/`cls` (rettangolo "tutto
  -1"), che viene tradotto in `Console.Clear()`.
- Conteggio oggetti (`[LARE] oggetti prodotti: N`): tecnica scelta è
  `Tee-Object -Variable` + una seconda, piccola invocazione PowerShell che
  legge e rimuove quella variabile. Non è un "secondo passaggio" sul comando
  dell'utente (che gira una volta sola, in streaming, tramite `Out-Default`):
  è solo una lettura della variabile già popolata da `Tee-Object`. Il
  conteggio esclude esplicitamente gli `ErrorRecord`.
- `ReadLineAsSecureString`: implementata leggendo un tasto alla volta e
  mascherando con `*`, senza dipendenze esterne — funzionale ma minimale
  (nessun supporto per frecce sinistra/destra durante l'editing della
  password, ad esempio).
- Dot-sourcing dei profili: solo `$PROFILE.CurrentUserAllHosts` e
  `$PROFILE.CurrentUserCurrentHost` (percorsi calcolati a mano, dato che in un
  host ospitato `$PROFILE` non è popolato automaticamente dal motore). Su
  questa macchina nessuno dei due file esiste, quindi il percorso "assenza
  gestita con grazia" è quello effettivamente testato.
- Barra di stato: usa una scroll region DECSTBM condivisa con l'output
  normale. Effetto collaterale noto (da verificare a mano, vedi sopra): in
  Windows Terminal, lo scroll dentro margini parziali **scarta le righe che
  escono dalla regione invece di mandarle nello scrollback** — chi testa a
  mano dovrebbe controllare se l'output "storico" resta consultabile
  scrollando indietro o viene perso.
- Execution policy: impostata esplicitamente a `RemoteSigned` sia nel REPL sia
  nel self-test (vedi sopra). È una scelta pragmatica per lo spike, non
  necessariamente quella giusta per il prodotto finale.
