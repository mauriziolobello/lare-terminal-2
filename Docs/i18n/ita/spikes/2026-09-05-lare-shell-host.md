# Spike — host PowerShell custom in C# (`lare-shell`)

**Date:** 2026-09-04 (costruzione, round 1) → 2026-09-05 (round 2, test finale).
**Codice:** `spikes/lare-shell-host/` (usa-e-getta: non è la base dell'implementazione).
**Domanda:** è fattibile un "Lare Terminal" che sia una **host custom del motore PowerShell**
(come lo è `pwsh.exe`), indistinguibile da pwsh per l'utente, che intercetti le righe `/…`, esegua
tutto il resto in-process, e disegni una UI estendibile nel terminale (barra di stato, segnalini)?
**Risposta: sì**, con un costo noto (scrollback) legato solo alla tecnica delle barre fisse.

## Cosa è stato costruito

App console C# (`net10.0`, NuGet `Microsoft.PowerShell.SDK` **7.6.5** — combacia con pwsh 7.6.5
installato): `PSHost` + `PSHostUserInterface` + `PSHostRawUserInterface` propri; runspace con
`InitialSessionState.CreateDefault()` e import di **PSReadLine 2.4.5** (dal path di pwsh 7);
lettura riga tramite la funzione `PSConsoleHostReadLine`, esattamente come fa `ConsoleHost`
(`ConsoleHostUserInterface.cs:2189`, `TryInvokeUserDefinedReadLine`); riga `/…` intercettata e
non eseguita; ogni altra riga eseguita con `AddScript(riga) | Out-Default`; barra di stato fissa in
basso (inverse video, con un hyperlink **OSC 8**) e riga indicatori fissa in alto (spinner +
orologio + ultimo `/comando`) tramite regione di scroll **DECSTBM**, ridisegnate da un thread in
background; profilo Windows Terminal via *fragment* JSON; modalità `--selftest` non interattiva.
~1.700 righe C#, costruite da un subagente Sonnet in due round, build e selftest riverificati
indipendentemente a ogni round.

## Esito dei test interattivi (Windows Terminal 1.24, pwsh 7.6.5, Python 3.14.7)

| # | Verifica | Round 1 | Round 2 |
|---|---|---|---|
| 1 | Sembra pwsh: prompt, colori sintassi, history (frecce), Tab completion | **sì** | sì |
| 2 | `/help` `/library` `/x` intercettati, non eseguiti; segnalino aggiornato | **sì** | sì |
| 3 | Comandi veri (`Get-ChildItem`, `git status`), `Read-Host`, `cd` persistente, Ctrl+C su `Start-Sleep` | **sì** | sì |
| 3b | `python` (REPL interattivo, comando nativo) | **no** — loop infinito di traceback | **sì** |
| 4a | Barre fisse mentre l'output scorre; prompt sopra la barra bassa | no — sovrapposizioni, cursore sulla riga 1, doppioni al resize | **sì** |
| 4b | Riga di editing intatta mentre lo spinner gira | no — un "3" seguiva il cursore | **sì** |
| 4c | `clear` → barre ridisegnate | no | parziale: riga alta subito, riga bassa al comando successivo (dettaglio dello spike, non indagato) |
| 4d | Resize → barre ridisegnate, niente doppioni | no | **sì** |
| 5 | Ctrl+click su `/library` (OSC 8) apre l'URL | **sì** | sì |
| 6 | Scrollback dopo un output lungo | — | **perso**: le righe scorse fuori dalla regione non finiscono nello scrollback |
| 7 | Voce "Lare Terminal (spike)" nel menu profili di WT | non verificabile (WT mai riavviato: ospita la sessione Claude Code) | idem |

## Cause trovate (lezioni da conservare)

1. **Escape `\x` goloso in C#.** `"\x1b7"` non è `ESC 7`: `\x` consuma fino a 4 cifre esadecimali,
   quindi vale U+01B7 (Ʒ) e `"\x1b8"` vale U+01B8 (Ƹ, il "3" visto a schermo). Salva/ripristina
   cursore non venivano mai emessi: da qui **tutti** i difetti di rendering del round 1. Rimedio:
   ESC costruito da `(char)0x1B`, nessun `\x`/`\u` nel codice.
2. **Un cmdlet fra il comando nativo e `Out-Default` gli toglie la console.** Lo spike inseriva
   `Tee-Object -Variable` per contare gli oggetti: il `NativeCommandProcessor` redirige allora lo
   stdout del processo nativo su una pipe; `_pyrepl/windows_console.py::getheightwidth` chiama
   `GetConsoleScreenBufferInfo` su quell'handle, fallisce (WinError 123) e ricomincia all'infinito.
   Con `riga | Out-Default` puro, come in `ConsoleHost`, il figlio eredita la console vera.
3. **`NotifyBeginApplication`/`NotifyEndApplication` non sono opzionali.** `ConsoleHost.cs:1227-1270`
   salva e ripristina la modalità console (output; noi anche input) attorno ai programmi esterni.
   Implementati uguali; dopo un programma esterno le barre vanno ridisegnate.
4. **Execution policy in-process.** Una runspace ospitata non eredita la policy di `pwsh.exe`:
   senza `iss.ExecutionPolicy = RemoteSigned` PSReadLine (`.psm1`) non si carica. Scelta da
   rivedere nel prodotto, non da copiare alla cieca.
5. **PSReadLine con stdin rediretto va in loop**: `ConsoleHost` lo salta se
   `Console.IsInputRedirected`; fare lo stesso.

## Implicazioni per il design 2.0

- **Confermato**: la host custom è la base giusta per il lato shell. Intercettazione a riga grezza,
  esecuzione in-process, console viva per i programmi interattivi, cwd = runspace, nessun hook in
  `$PROFILE`, nessun exit-and-resume. Le sezioni §2/§4/§6 dello spec vanno riscritte su questa base.
- **Costo della UI "nel terminale"**: le barre fisse via DECSTBM funzionano ma **cancellano lo
  scrollback** — un prezzo alto per un terminale. Alternative dentro il terminale: barre
  transitorie ridisegnate a ogni prompt (scrollback intatto, nessuna fissità) e segnalini nel titolo
  della scheda (OSC 2). Alternativa fuori dal terminale: rendere la host dentro una finestra
  propria (Tauri + emulatore xterm.js + ConPTY), dove barre e segnalini sono HTML fuori dall'area
  terminale e lo scrollback è quello dell'emulatore — vedi la discussione successiva allo spike.
- **Windows Terminal**: il profilo via fragment è banale; resta da vedere a WT riavviato.
