# RUN — avviare Lare Terminal 2.0

Come lanciare il programma **una volta che è compilato e deployato** (`BUILD.md` → `DEPLOY.md`).
Questa pagina non parla di compilazione né di deploy — solo di quale eseguibile avviare e perché.

## Il metodo definitivo (consigliato)

Da `Test Run\` (o dalla cartella dove hai copiato il deploy):

```powershell
.\ui.exe
```

**Un solo comando.** Si apre la finestra terminale di Lare (xterm.js dentro una ConPTY) con dentro
una shell PowerShell vera e propria; se l'orchestratore non è già in esecuzione lo avvia da solo
(self-heal). Non serve avviare nient'altro a mano, non serve aprire Windows Terminal, non serve
un profilo installato: `ui.exe` **è** il programma, tutto qui dentro. Questa è la **modalità A**
dello spec — l'unico modo pensato per l'uso quotidiano, non un'alternativa fra pari.

Chiudere la finestra termina tutto (il processo `lare-shell.exe` figlio e `ui.exe` stesso) —
nessun processo residuo, l'orchestratore resta vivo in background com'è giusto (è un daemon
condiviso, non legato a una singola finestra).

Tutto il resto di questa pagina descrive alternative e casi speciali: se il comando sopra ti
basta, non serve leggere altro.

## Perché esiste anche una modalità B

Chi preferisce una scheda "nuda" dentro il **proprio** Windows Terminal (senza la chrome HTML di
`ui.exe` — barre, segnalini, pulsanti — solo il prompt PowerShell con i comandi slash che
funzionano lo stesso) può usare la **modalità B**: `lare-shell.exe` avviata direttamente da un
profilo Windows Terminal dedicato, invece che dentro la finestra di `ui.exe`. Stessa shell, stesso
motore, stessi comandi `/…` — cambia solo il "contenitore" grafico. Va installata una volta sola
(sotto), poi si usa come una qualunque altra scheda di Windows Terminal.

Le due modalità **non si escludono**: puoi avere la modalità B installata e comunque usare
`ui.exe` quando preferisci, o viceversa. Non condividono processo: sono due modi diversi di
lanciare la stessa `lare-shell.exe`.

**La modalità A non può diventare un profilo Windows Terminal** — non è una scelta, è un limite
architetturale: WT ospita processi *console-subsystem* attaccati a una ConPTY che gestisce lei, e
ne disegna l'output nella propria scheda. `ui.exe` è un'app *GUI-subsystem* in release
(`windows_subsystem = "windows"`, `main.rs`) — il terminale che vedi in modalità A lo disegna
xterm.js dentro la sua finestra Tauri; la ConPTY verso `lare-shell.exe` è plumbing interno,
invisibile al sottosistema console di Windows. Un profilo WT puntato a `ui.exe` darebbe nella
migliore delle ipotesi una scheda vuota, con la vera finestra che si apre comunque a parte — non
un tab autosufficiente. Per un lancio comodo di modalità A l'equivalente naturale è un
collegamento o un'icona sulla taskbar a `ui.exe`, non un profilo WT. `install-wt-profile.ps1`
serve solo per la modalità B.

## Modalità A — dettaglio

```powershell
.\ui.exe
```

Cosa succede, in ordine:
1. `ui.exe` controlla se l'orchestratore risponde su `127.0.0.1:<ws_port>`; se non risponde e
   `autostart.orchestrator` è attivo in `startup.json` (default: sì), lo avvia da solo e aspetta
   fino a 5 secondi che si connetta (self-heal, spec §6.4).
2. Si apre la finestra terminale: dentro, `lare-shell.exe` (la host C# del motore PowerShell) gira
   in una pseudo-console (ConPTY), con un prompt PowerShell reale — PSReadLine, history, Tab
   incluse.
3. I comandi `/…` (`/help`, `/ping`, `/ai "…"`, …) funzionano come su qualunque altra shell Lare;
   l'esito compare in una finestra dedicata (Markdown, tabella, …), una riga di conferma resta nel
   terminale.
4. Se la shell interna crash o esce, la finestra mostra un banner "shell terminata" con un
   pulsante **riavvia** — non serve chiudere e riaprire `ui.exe`.
5. Chiudere la finestra (X, Alt+F4) termina `lare-shell.exe` e l'intero processo `ui.exe`.
   L'orchestratore resta in esecuzione (è condiviso, sopravvive a ogni finestra).

Nessun flag necessario per l'uso normale. `ui.exe --no-terminal` esiste ma è per usi interni (la
host C# in modalità B e l'autostart dell'orchestratore lo passano quando hanno bisogno solo delle
finestre di `ui.exe`, senza una seconda finestra terminale non voluta) — non serve mai passarlo a
mano per un uso quotidiano.

## Modalità B — dettaglio

**Installazione (una volta sola)**, da `Test Run\` (o dalla cartella dove hai copiato il deploy):

```powershell
.\install-wt-profile.ps1
```

Scrive un fragment JSON in `%LOCALAPPDATA%\Microsoft\Windows Terminal\Fragments\Lare\`, letto da
Windows Terminal **solo al proprio avvio**: dopo l'installazione, **riavvia Windows Terminal**
(chiudi tutte le finestre) — aprire una nuova scheda in una finestra già avviata non basta.
`.\uninstall-wt-profile.ps1` rimuove il fragment.

**Uso quotidiano**: apri una nuova scheda col profilo "Lare Terminal" in Windows Terminal (dal
menu a tendina delle schede, o dalla combinazione che gli hai assegnato). Stesso self-heal della
modalità A: `lare-shell.exe` avvia da sola `orchestrator.exe` e `ui.exe` se mancano.

`ui.exe`, quando avviato così dal self-heal della host, parte con `--no-terminal` (nessuna
finestra terminale — la shell "vera" sei già dentro la scheda di Windows Terminal): vedrai comunque
le finestre Markdown/config/library quando un comando le apre, solo non una seconda finestra
terminale.

## Modalità sviluppo (solo per chi scrive codice) — NON il metodo per usare il programma

Se stai modificando il codice sorgente e vuoi eseguire i binari appena compilati **senza**
rigenerare `Test Run\` a ogni giro (`DEPLOY.md`), puoi lanciarli direttamente da `target\debug\`.
Serve **sempre** `--config-dir` esplicito (senza, la regola di risoluzione userebbe
`target\debug\Configuration\`, che non esiste):

```powershell
# Terminale 1
cargo run -p orchestrator -- --config-dir "Test Run\Configuration" --console-log

# Terminale 2
cargo run -p ui -- --config-dir "Test Run\Configuration"
```

`--console-log` fa scrivere l'orchestrator anche su questa console, oltre che sul file di log
giornaliero in `Test Run\Configuration\logs\` (senza il flag, solo file — pensato per l'autostart,
dove nessuno guarda un terminale).

Stessa idea per la host C# da sorgente (vedi `BUILD.md`):

```powershell
dotnet run --project shell/lare-shell/src/LareShell -- --config-dir "Test Run\Configuration"
```

**Gotcha: terminale con stdin rediretto (es. Claude Code) → niente PSReadLine, niente gate
interattivo.** `lare-shell` distingue "PSReadLine disponibile ma non in uso" (stdin rediretto,
fallback a `Console.ReadLine`) da "PSReadLine non disponibile" (banner diverso); il gate `[Y/n]`
con stdin rediretto legge una riga di testo e **fallisce chiuso** su EOF (nessuna tastiera vera =
nessuno a cui chiedere). Per un test reale (PSReadLine, prompt `[Y/n]` a tasto, Ctrl+C) serve una
console interattiva vera — Windows Terminal, non il terminale di uno strumento come Claude Code —
vedi `TESTING-e2e.md`.

## Caso avanzato: avvio manuale senza self-heal

Se per qualche motivo vuoi controllare a mano l'ordine di avvio (debug, `autostart` disattivato in
`startup.json`, …), da `Test Run\`:

```powershell
.\init_orchestrator.ps1
.\init_tauri.ps1
```

(l'ordine non conta — ciascuno aspetta/ritenta l'altro). Per l'uso normale non serve: la modalità
A da sola basta.
