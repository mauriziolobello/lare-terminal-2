# lare-terminal-window (spike 2)

Spike tecnico **throwaway** (non è codice di produzione) per **Lare Terminal
2.0**: una finestra Tauri 2 che ospita un vero terminale — **xterm.js** nel
webview, collegato via **ConPTY** (crate `portable-pty`) a un processo figlio
(di norma lo spike 1, `lare-shell-spike.exe`). Attorno all'area del terminale,
chrome **HTML pura**: una barra superiore con indicatori e una barra inferiore
con comandi "/" cliccabili.

Obiettivo dello spike, in quattro punti verificabili:

1. l'host C# dello spike 1 gira dentro la finestra Tauri con lo stesso aspetto
   (pwsh, PSReadLine, `python`, Ctrl+C) che avrebbe in Windows Terminal;
2. lo scrollback è quello dell'emulatore (xterm.js), non va perso;
3. la chrome (le due barre) è HTML/CSS, nessun trucco VT/ANSI lato Rust/JS;
4. una riga "/" intercettata dall'host C# può accendere un indicatore HTML
   tramite una sequenza OSC custom.

Non dipende da, e non modifica, il progetto v1 (`Lare Terminal/`): è stato
usato solo come riferimento di stile/struttura (letto, non toccato).

## Versioni (verificate su questa macchina)

| Componente | Versione |
|---|---|
| rustc / cargo | 1.97.1 |
| Node.js / npm | v26.7.0 / 11.19.0 (usati SOLO per scaricare xterm.js via `npm pack`, nessun `package.json`/bundler nello spike) |
| `@xterm/xterm` | **6.0.0** (vendorizzato in `frontend/vendor/xterm.js` + `xterm.css`) |
| `@xterm/addon-fit` | **0.11.0** (vendorizzato in `frontend/vendor/addon-fit.js`) |
| `tauri` (crate) | 2.11.5 |
| `tauri-build` | 2.6.3 |
| `tauri-runtime` / `wry` / `tao` | 2.11.3 / 0.55.1 / 0.35.3 |
| `portable-pty` | 0.9.0 (richiesta "0.9" in Cargo.toml, risolta senza problemi) |
| `base64` | 0.22.1 (richiesta "0.22"; nel grafo delle dipendenze compare ANCHE 0.21.7, ma è una dipendenza transitiva di un altro crate, non la nostra — verificato in Cargo.lock, sezione dipendenze di `lare-terminal-window`) |
| `serde` / `serde_json` | 1.0.229 / 1.0.151 |
| .NET SDK (per lare-shell-host) | 10.0.400, target `net10.0` |

## Struttura

```
spikes/lare-terminal-window/
├── README.md
├── frontend/
│   ├── index.html      griglia 3 righe: barra superiore / #terminal / barra inferiore
│   ├── app.js           wiring xterm.js <-> comandi Tauri (invoke/listen)
│   ├── style.css        tema scuro Campbell (vicino a pwsh in Windows Terminal)
│   └── vendor/          xterm.js 6.0.0 UMD + xterm.css + addon-fit 0.11.0 UMD
└── src-tauri/
    ├── Cargo.toml       pacchetto standalone (NON membro di un workspace)
    ├── build.rs         tauri_build::build()
    ├── tauri.conf.json  finestra "main", frontendDist=../frontend, bundle.active=false
    ├── capabilities/default.json
    ├── icons/icon.ico   copiato da v1 (solo l'asset, nessuna dipendenza di codice)
    └── src/main.rs      comandi pty_spawn / pty_write / pty_resize / shell_info
```

## Come compilare

Prima lo spike 1 (l'host C#, se non già compilato):

```powershell
cd 'C:\Users\Maurizio\Documents\Progetti\Lare Terminal 2.0\spikes\lare-shell-host'
dotnet build -c Release
# produce bin\Release\net10.0\lare-shell-spike.exe
```

Poi questo spike (niente `cargo tauri` CLI: non è installato, si usa `cargo`
puro con un `frontendDist` statico, esattamente come in v1):

```powershell
cd 'C:\Users\Maurizio\Documents\Progetti\Lare Terminal 2.0\spikes\lare-terminal-window\src-tauri'
cargo build
```

La prima build compila da zero l'intero albero di dipendenze Tauri (~250
crate in questo caso).

**Attenzione, lezione appresa in questa sessione**: `tauri::generate_context!()`
incorpora nel binario il contenuto di `frontendDist` (`../frontend`) al
momento della compilazione — NON lo legge dal disco a ogni avvio, nemmeno in
`cargo build`/`cargo run` "di sviluppo" (qui non c'è `cargo tauri dev` con un
suo file-watcher). Cargo, a sua volta, non sembra riconoscere le modifiche
dentro `frontend/` come motivo sufficiente per ricompilare: un `cargo build`
lanciato dopo aver modificato SOLO file in `frontend/` (nessun file `.rs`
toccato) può concludere "Finished ... in 0.53s" senza ricompilare nulla,
lasciando nel binario la versione VECCHIA della cartella frontend. Se cambi
qualunque cosa in `frontend/` senza aver toccato `src-tauri/src/*.rs`, fai
prima `cargo clean -p lare-terminal-window` (pulisce SOLO gli artefatti di
questo pacchetto, non l'intera cache delle ~250 dipendenze) e poi `cargo
build`, altrimenti rischi di testare una build con una webview vecchia (è
esattamente l'errore fatto una volta durante lo sviluppo di questo spike:
vedi "Smoke-run" sotto per come si è manifestato — pty_spawn non veniva mai
invocato perché la webview stava caricando un `frontendDist` senza
`index.html`).

## Come eseguire

```powershell
cd 'C:\Users\Maurizio\Documents\Progetti\Lare Terminal 2.0\spikes\lare-terminal-window\src-tauri'
cargo run
# oppure, dopo la build:
.\target\debug\lare-terminal-window.exe
```

Argomento opzionale `--shell <path>` per usare una shell diversa da quella
risolta di default (utile per confrontare con `pwsh.exe` puro):

```powershell
.\target\debug\lare-terminal-window.exe --shell "C:\Program Files\PowerShell\7\pwsh.exe"
```

### Risoluzione della shell (ordine di precedenza)

1. `--shell <path>` sulla riga di comando del processo Tauri;
2. il percorso assoluto noto `..\lare-shell-host\bin\Release\net10.0\lare-shell-spike.exe`, se il file esiste;
3. fallback a `pwsh.exe` (deve essere nel PATH).

`--no-bars` viene aggiunto automaticamente agli argomenti SOLO se la shell
risolta è `lare-shell-spike.exe` (verificato per nome file, case-insensitive):
`pwsh.exe` non conosce quel flag e lo interpreterebbe come nome di script da
eseguire, quindi non va mai passato in quel caso.

## Cosa dimostra ciascun check (e come verificarlo a mano)

Questo spike richiede una finestra visibile e interazione manuale: quanto
segue è **da verificare dal vivo**, non è stato (né poteva essere) verificato
in modo automatico in questa sessione — vedi la sezione "Cosa NON è stato
verificato" più sotto.

1. **Look & feel pwsh / PSReadLine / python / Ctrl+C** — apri la finestra,
   verifica che il prompt sia quello di PowerShell con syntax highlighting di
   PSReadLine (history con frecce, tab-completion), lancia `python` e
   controlla che il REPL non vada in loop di errori (il bug storico descritto
   in `Repl.cs` — vedi lo spike 1), premi Ctrl+C durante un comando lungo e
   verifica che interrompa solo quello, non l'intera shell.
2. **Scrollback** — genera output più lungo di una schermata (es. `Get-ChildItem
   -Recurse C:\Windows\System32 | Select -First 200`) e scrolla indietro con
   la rotellina/PageUp: deve funzionare (xterm.js `scrollback: 5000`), a
   differenza della StatusBar C# che userebbe DECSTBM per confinare l'output.
3. **Chrome HTML pura** — ispeziona `frontend/index.html`/`style.css`: le due
   barre sono `<header>`/`<footer>` con CSS grid, nessuna sequenza VT/ANSI
   scritta da Rust o JS per disegnarle (a differenza della StatusBar C# dello
   spike 1, che invece usa DECSTBM + inverse video).
4. **Indicatore OSC custom** — digita una riga che inizia con `/` (es.
   `/help`) e osserva la barra superiore: il pallino accanto a "ultimo: ..."
   deve diventare verde per 2 secondi. **Punto delicato, vedi sotto.**

### Punto delicato: OSC 9001 potrebbe non arrivare (limite noto di ConPTY)

ConPTY (la pseudo-console Windows usata da `portable-pty`) non è un
pass-through trasparente: ri-emette verso il lato lettura SOLO le sequenze VT
che riconosce lui stesso, e può scartare una OSC custom come la 9001 prima
ancora che arrivi a xterm.js (comportamento documentato in issue pubbliche del
progetto Windows Terminal/ConPTY, non un bug di questo spike). Per questo lo
spike 1 (vedi modifiche a `Repl.cs`) invia il segnale "riga / intercettata" su
**due canali indipendenti**:

- **Canale 1**: OSC 9001 custom (`ESC ] 9001 ; lare ; intercept ; <riga> ESC \`),
  letta da `term.parser.registerOscHandler(9001, ...)` in `app.js`.
- **Canale 2 (riserva)**: cambio del titolo console (`Console.Title =
  "lare;intercept;<riga>"`, poi ripristinato subito dopo). Il titolo è un
  canale che ConPTY DEVE ri-emettere per costruzione (è così che la scheda di
  Windows Terminal segue il titolo di un'app), quindi dovrebbe funzionare
  anche se il canale 1 venisse inghiottito. Letto da `term.onTitleChange(...)`
  in `app.js`.

**Verifica da fare a mano**: se il pallino si accende, controllare (con gli
strumenti di sviluppo del webview, se accessibili) quale dei due canali lo ha
acceso. Se NESSUNO dei due arriva, è la conferma che ConPTY sta scartando
entrambi i tipi di sequenza per questa configurazione — un risultato
comunque utile per lo spike (significa che serve un canale diverso, es. named
pipe separata fra i due processi, per una versione non-throwaway).

## Modifiche minime a `lare-shell-host` (step 4)

1. **`--no-bars`** (`Program.cs`, stesso pattern di parsing di `--selftest`):
   se presente, `Repl.RunInteractive(noBars: true)` non crea affatto la
   `StatusBar`. In `Repl.cs`, `StatusBar? statusBar = noBars ? null : new
   StatusBar(ConsoleLock);` e tutte le chiamate diventano `statusBar?.Metodo()`
   (operatore Elvis): quando `noBars` è true diventano no-op invece di
   `NullReferenceException`. Composizione (un collaboratore opzionale) invece
   di una seconda gerarchia di classi "con barra"/"senza barra".
2. **Abilitazione VT processing spostata** da un metodo privato dentro
   `StatusBar` a `ConsoleModes.TryEnableVirtualTerminalProcessing()` (metodo
   pubblico condiviso): serve poterla chiamare anche quando la `StatusBar` non
   viene creata affatto (`--no-bars`), perché l'OSC 9001 va comunque scritta
   come sequenza VT interpretata, non come testo letterale.
3. **OSC 9001 + canale di riserva (titolo console)**: nuovo metodo
   `Repl.NotifyLareIntercept(string rawLine)`, chiamato PRIMA della riga ciano
   `[LARE] intercettato: ...`, dentro lo stesso `lock (ConsoleLock)` già
   esistente. Il carattere ESC è **sempre** `StatusBar.Esc` (ora `internal`),
   **mai** un escape di stringa `"\x1b..."` scritto a mano: vedi il commento
   storico in cima a `StatusBar.cs` sul bug "`\x` greedy" del round 2 di
   questo stesso spike (un `"\x1b]9001..."` scritto a mano avrebbe fatto
   leggere al compilatore C# `\x1b9` come un'unica cifra esadecimale,
   producendo un glifo Unicode invece del carattere ESC).

File C# modificati: `Program.cs`, `Repl.cs`, `StatusBar.cs`, `ConsoleModes.cs`
(vedi sotto per i conteggi di righe finali). Nessuna modifica a `LareHost.cs`,
`LareHostUI.cs`, `LareRawUI.cs`.

## Verifiche eseguite (fatti, non opinioni)

### Build C# (`dotnet build -c Release`)

Ultime righe:
```
lare-shell-spike -> ...\bin\Release\net10.0\lare-shell-spike.dll
Compilazione completata.
    Avvisi: 6   (tutti CA1416 "Console.Title/CursorSize ecc. solo Windows": preesistenti + uno nuovo identico, coerente con lo stile già presente in LareRawUI.cs)
    Errori: 0
```

### `--selftest` (dopo le modifiche)

```
=== lare-shell-spike --selftest ===
[OK] Apertura runspace ospitata (LareHost + PSReadLine importato)
[OK] Funzione PSConsoleHostReadLine presente dopo l'import di PSReadLine (YES)
  Get-Module PSReadLine -> Name=PSReadLine Version=2.4.5 Path=C:\program files\powershell\7\Modules\PSReadLine\PSReadLine.psm1
[OK] Get-ChildItem $env:TEMP | Select-Object -First 3 (cattura oggetti) (oggetti catturati: 3)
[OK] Intercettazione riga '/help' tramite la stessa funzione usata dal REPL ([LARE] intercettato: /help)
[OK] Riga 'Get-Date' (senza '/') NON intercettata

TUTTI I CONTROLLI SONO PASSATI.
```
Exit code: **0**.

### Run con stdin rediretto (`printf '/help\n/exit\n' | lare-shell-spike.exe`)

Esce pulito con **exit code 0** (comportamento invariato rispetto a prima
delle modifiche). Nell'output si vede la sequenza OSC scritta come byte
grezzi, non interpretata: qui stdout è una pipe catturata dal tool di
verifica, non una vera console, quindi `ConsoleModes.TryEnableVirtualTerminalProcessing()`
fallisce (`GetConsoleMode` su un handle di pipe ritorna false) e il carattere
ESC risulta semplicemente invisibile nel testo catturato:
```
PS ...> ]9001;lare;intercept;/help\[LARE] intercettato: /help
PS ...> ]9001;lare;intercept;/exit\[LARE] intercettato: /exit
```

Stessa identica prova ripetuta con **`--no-bars`**, per esercitare il ramo
`StatusBar? statusBar = null` (altrimenti mai eseguito dalle altre due
verifiche, che non passano quel flag): stesso output OSC/intercettazione,
**exit code 0**, nessuna `NullReferenceException`. Nota: in questa prova
stdout era comunque una pipe (catturata dal tool), quindi la StatusBar
sarebbe stata disabilitata (`Console.IsOutputRedirected`) anche SENZA
`--no-bars` — questa prova non isola quindi l'effetto del flag da solo, prova
solo che il percorso `statusBar?.` (mai eseguito prima in questa sessione)
funziona end-to-end senza eccezioni. Controllato anche, per sicurezza, che
`StatusBar.Current` sia usato SOLO con `?.` nel resto del codice
(`LareHost.cs:238`, `LareRawUI.cs:152`) — quindi `--no-bars` non introduce un
`NullReferenceException` nemmeno nei percorsi toccati da `Clear-Host` o dal
ritorno da un'app a schermo intero (`python`, `vim`), che restano comunque da
verificare dal vivo (vedi "Cosa NON è stato verificato").

### Build Rust (`cargo build`, profilo dev)

```
   Compiling lare-terminal-window v0.1.0 (...\spikes\lare-terminal-window\src-tauri)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 12.87s
```
Zero errori. (Nella primissima stesura c'era un errore di compilazione — una
variabile locale in `main()` chiamata `shell_info`, stesso nome del comando
`#[tauri::command] fn shell_info`, che "nascondeva" la funzione dentro la
macro `tauri::generate_handler!`: rinominata in `resolved_shell`, risolto.)

### Smoke-run (`target/debug/lare-terminal-window.exe`, timeout 15s)

**Prima prova (fuorviante, spiegata sotto)**: un primo smoke-run, fatto con
un binario compilato PRIMA che `frontend/index.html`/`app.js`/`style.css`
esistessero (vedi l'avviso in "Come compilare" su `generate_context!` +
cache di Cargo), mostrava solo la riga "shell risolta all'avvio" e mai la
conferma di `pty_spawn`: la webview stava caricando un `frontendDist` privo
di `index.html`, quindi nessun JS veniva mai eseguito. Non era un limite
dello spike, era un binario scaduto. Rifatto con `cargo clean -p
lare-terminal-window && cargo build` (ricompilazione pulita, 24.97s) e poi
ripetuto lo smoke-run: risultato corretto sotto.

Comando: `timeout 15 ./target/debug/lare-terminal-window.exe 2>stderr.txt`

```
EXIT CODE: 124
```
(124 = ucciso da `timeout` dopo 15s piena attività, cioè il processo NON è
andato in crash — è il risultato atteso per questo smoke-test).

stderr:
```
[lare-terminal-window] shell risolta all'avvio: C:\Users\Maurizio\Documents\Progetti\Lare Terminal 2.0\spikes\lare-shell-host\bin\Release\net10.0\lare-shell-spike.exe (spike host: true)
[lare-terminal-window] avvio pty con shell: C:\Users\Maurizio\Documents\Progetti\Lare Terminal 2.0\spikes\lare-shell-host\bin\Release\net10.0\lare-shell-spike.exe (--no-bars: true)
[lare-terminal-window] processo figlio terminato, exit code: 143
[0905/021045.533:ERROR:ui\gfx\win\window_impl.cc:172] Failed to unregister class Chrome_WidgetWin_0. Error = 1411
```
stdout: vuoto.

Lettura riga per riga:
- **"shell risolta all'avvio"**: conferma che il lato Rust risolve
  correttamente lo spike 1 come shell di default (il file esiste, quindi non
  scatta il fallback a `pwsh.exe`) — stampata da `main()`, prima ancora che
  il runtime Tauri parta.
- **"avvio pty con shell ... (--no-bars: true)"**: conferma che la webview ha
  effettivamente caricato `index.html`, eseguito `app.js`, ottenuto
  `cols`/`rows` da xterm.js e invocato con successo `pty_spawn` — cioè che
  l'intera catena JS→invoke→ConPTY→spawn del processo figlio funziona, non
  solo che la finestra Rust/Tauri si apre. Conferma anche che `--no-bars`
  viene aggiunto correttamente (lo spike host è stato riconosciuto per nome
  file).
- **"processo figlio terminato, exit code: 143"**: il thread
  `spawn_exit_watcher` ha rilevato la fine di `lare-shell-spike.exe` E l'ha
  loggata PRIMA che il processo padre stesso morisse (altrimenti quel thread
  non avrebbe potuto scrivere su stderr): il figlio è quindi morto per primo,
  verosimilmente perché `timeout` (sotto Git Bash/msys2) ha terminato l'intero
  albero di processi allo scadere dei 15s, non per un effetto a cascata
  master-pty-droppato→figlio-chiuso innescato dalla sola morte del padre. 143
  = 128+15 è la convenzione msys2 per "terminato da SIGTERM". In ogni caso è
  la controprova che il meccanismo di rilevazione fine-processo descritto in
  "Semplificazioni note" (thread dedicato su `Child::wait()`, non EOF di
  lettura) funziona davvero, non solo in teoria: l'evento è stato rilevato e
  loggato correttamente.
- `Failed to unregister class Chrome_WidgetWin_0`: warning noto e innocuo di
  WebView2/Chromium in chiusura (stesso comportamento già documentato nei
  commenti del `main.rs` del progetto v1), non un errore di questo spike.

Processi orfani dopo la prova: **nessuno** (verificato con `tasklist` subito
dopo `taskkill //F //IM lare-terminal-window.exe` e `//IM
lare-shell-spike.exe`: entrambi già usciti da soli, coerente con la riga
"processo figlio terminato" qui sopra).

## File creati/modificati (conteggio righe)

Nuovi (spike 2):
- `frontend/index.html` — 53 righe
- `frontend/app.js` — 187 righe
- `frontend/style.css` — 128 righe
- `frontend/vendor/xterm.js` — vendorizzato, 488'663 byte (minificato, non significativo in righe)
- `frontend/vendor/xterm.css` — vendorizzato, 7'112 byte
- `frontend/vendor/addon-fit.js` — vendorizzato, 1'521 byte
- `src-tauri/Cargo.toml` — 35 righe
- `src-tauri/build.rs` — 7 righe
- `src-tauri/tauri.conf.json` — 29 righe
- `src-tauri/capabilities/default.json` — 12 righe
- `src-tauri/src/main.rs` — 307 righe
- `src-tauri/icons/icon.ico` — copiato da v1 (1150 byte, solo asset)

Modificati (`spikes/lare-shell-host/`, conteggio righe FINALE del file intero,
non del solo diff):
- `Program.cs` — 174 righe (era 166)
- `Repl.cs` — 497 righe (era 427)
- `StatusBar.cs` — 381 righe (era 402: la rimozione del metodo privato duplicato ha più che compensato i commenti aggiunti)
- `ConsoleModes.cs` — 110 righe (era 68)

## Semplificazioni note

- **`PtySession` non contiene un campo `child`** (a differenza della bozza
  iniziale della specifica). Motivo: su Windows/ConPTY la lettura dalla pty
  NON restituisce mai EOF alla terminazione del processo figlio (a differenza
  di una pipe Unix "vera") — resterebbe bloccata finché la pseudo-console
  stessa non viene chiusa. L'unico segnale affidabile di "il processo è
  terminato" è `Child::wait()`, che richiede possesso esclusivo (`&mut`) del
  child: viene quindi spostato per intero in un thread dedicato
  (`spawn_exit_watcher`), che lo "consuma" e poi emette `pty-exit` con l'exit
  code. Master e writer restano invece nello stato Tauri (`PtySession`) per
  `pty_write`/`pty_resize`. Il thread di lettura (`spawn_reader_thread`) resta
  comunque presente come rete di sicurezza (esce anche su `Ok(0)`/errore),
  ma NON è il meccanismo primario di rilevazione fine-processo.
- **Nessun limite/lock su spawn concorrenti oltre un `Err` testuale**:
  `pty_spawn` rifiuta una seconda chiamata se una sessione è già attiva
  (`"una sessione pty è già attiva"`), ma non c'è un comando esplicito per
  terminare/riavviare la sessione (chiudere la finestra è l'unico modo, in
  questo spike, per liberare la pty).
- **I bottoni della barra inferiore scrivono testo + invio nella pty** (come
  se l'utente li avesse digitati), non un vero protocollo IPC verso l'host
  C#: è la scelta più semplice per uno spike "il click aziona uno slash
  command", coerente con la richiesta della specifica.
- **Font `Cascadia Mono`** non è vendorizzato come web-font: se non è
  installato sul sistema (di norma lo è, con Windows Terminal/VS Code), il
  browser userà il fallback `Consolas, monospace` dichiarato nel CSS —
  esteticamente diverso ma funzionalmente equivalente.
- **`bundle.active: false`**: nessun installer/pacchetto prodotto, solo
  l'eseguibile di sviluppo — coerente con la natura "spike, non prodotto".
- **Canale di riserva OSC via titolo console** (vedi sopra) è un'aggiunta
  rispetto alla specifica originale (che prevedeva solo l'OSC 9001), fatta
  per lo stesso motivo per cui la sezione "punto delicato" qui sopra esiste:
  senza di essa lo spike rischierebbe di "fallire silenziosamente" il check
  4 per un limite di ConPTY indipendente dalla correttezza del codice scritto
  in questa sessione.

## Cosa NON è stato verificato in questa sessione (va testato dal vivo)

Tutto ciò che richiede vedere/usare la finestra:
- aspetto reale del terminale (font, colori Campbell, cursore che lampeggia);
- PSReadLine funzionante dentro il webview (history, syntax highlighting, tab-completion);
- `python` REPL senza il loop di errori storico (vedi commento in `Repl.cs`);
- Ctrl+C che interrompe solo il comando corrente, non l'intera shell;
- scrollback (rotellina/PageUp) su output più lungo di una schermata;
- l'indicatore OSC 9001 (o il canale di riserva via titolo) che si accende
  davvero al digitare una riga "/" — vedi la sezione dedicata sopra;
- i quattro bottoni della barra inferiore (click → comando scritto + invio,
  focus che torna al terminale);
- il ridimensionamento della finestra che aggiorna `cols`/`rows` sia in
  xterm.js sia nella pty (`ResizeObserver` + debounce 50ms + `pty_resize`);
- chiusura NORMALE della finestra (click sulla X) → `lare-shell-spike.exe`
  termina pulito: lo smoke-run l'ha chiuso con `timeout` (uccisione
  dell'intero albero di processi, verosimilmente non passando dal semplice
  drop di `PtySession`/`master`), quindi questo percorso "chiusura ordinata
  della finestra" resta da verificare a mano (aprire la finestra, digitare
  qualcosa, chiuderla con la X, controllare con `tasklist` che
  `lare-shell-spike.exe` non resti residente).

(Lo smoke-run conferma, invece, che la catena JS→invoke→ConPTY→spawn
funziona: vedi la riga "avvio pty con shell" nella sezione Smoke-run sopra —
non è quindi più un'incognita "il webview carica la pagina?", solo il
"come appare/si comporta visivamente" e "come si comporta alla chiusura
normale" restano da vedere dal vivo.)
