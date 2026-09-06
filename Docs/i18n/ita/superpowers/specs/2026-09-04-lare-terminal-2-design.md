# Lare Terminal 2.0 — Design

> **Stato:** prima stesura approvata a sezioni in chat il 2026-09-04; **riscritta il 2026-09-05
> dopo i due spike** (`Docs/i18n/ita/spikes/2026-09-05-lare-shell-host.md` e
> `2026-09-05-lare-terminal-window.md`) sulla forma finale del lato shell. In attesa della
> revisione dell'utente sul file; poi il piano di implementazione
> (`Docs/i18n/ita/superpowers/plans/`). La storia della prima stesura (hook PSReadLine + client
> CLI + exit-and-resume) è nel git log; non è più un'alternativa.
>
> **Cosa decide questo documento:** il modello di esecuzione 2.0 (Lare Terminal = finestra Tauri
> che ospita una host PowerShell custom), il protocollo fra host e orchestratore, la configurazione
> unificata, il layout di deploy, i confini dell'MVP. **Cosa NON decide:** la struttura interna dei
> crate copiati dalla v1 (resta quella, vedi `Docs/STATO-ATTUALE.md` del repo v1) e tutto ciò che è
> in §12 "Fuori MVP".
>
> **Punti di partenza:** repo v1 `C:\Users\Maurizio\Documents\Progetti\Lare Terminal` (leggere
> `Docs/STATO-ATTUALE.md`, Parte II §18-20); Google Doc "Lare Terminal 2.0" (bozza di un'altra AI,
> solo inquadramento — §1.2); layout di deploy v1 `C:\Lare Terminal`; sorgente di PowerShell in
> `C:\Users\Maurizio\Documents\GitHub\PowerShell` (riferimento per la host:
> `src/Microsoft.PowerShell.ConsoleHost/host/msh/ConsoleHost.cs` e `ConsoleHostUserInterface.cs`).

## 1. Contesto e obiettivo

### 1.1 Obiettivo (parole dell'utente)

> L'utente scrive nella stessa riga di comando di una sessione PowerShell; i comandi slash vengono
> interpretati dal nostro programma. Se esiste il corrispondente comando viene elaborato, se non
> esiste viene silenziosamente scartato.

Evoluto il 2026-09-04 sera in: **Lare Terminal è un terminale in tutto e per tutto simile a una
sessione PowerShell**, con i "power command" `/…` e l'AI integrata, con una UI estendibile (barra di
stato, segnalini, comandi cliccabili) e l'estetica grafica della v1. Vincolo: **mantenere il più
possibile** di quanto costruito nella v1; comportamento **non difforme dalla v1** per le superfici
che restano (`/markets`, `/calc`, `/library` aprono le stesse finestre Tauri).

### 1.2 Cosa si prende dalla bozza Google Doc e cosa no

Si adotta come inquadramento: "command layer sopra la shell esistente", adapter sottile → daemon
che ignora chi lo chiama, self-heal del daemon. Si scarta ciò che ri-deriva cose già esistenti
(named pipe al posto del WS v1, `hubd`/`hubctl` al posto dell'orchestratore, plugin WASM al posto
dei sidecar) e la forma request/response del protocollo (la v1 vive di streaming: token AI,
trasparenza tool, cancel, gate). L'output testuale dei plugin (`/calc 5*20 → 840`) è lavoro nuovo:
fuori MVP (§12).

### 1.3 Decisioni prese

| # | Decisione | Data / nota |
|---|---|---|
| D1 | I comandi OS proposti dall'AI eseguono **nella shell dell'utente**, in-process nella host (Lare è la shell, non un ospite di una shell altrui) | 04/09; realizzata dalla host custom (D12) |
| D2 | L'overlay F2 della v1 **muore**; il suo codice viene **rimosso** dal crate `ui` copiato, non lasciato dormiente | 04/09 sera; rimozione = mia raccomandazione, da confermare |
| D3 | Linguaggio naturale solo con slash: `/ai "testo"` ≡ `/ "testo"`, **virgolette obbligatorie** | 04/09 |
| D4 | Slash sconosciuto → **scartato in silenzio** nel terminale, una riga di log nell'orchestratore | 04/09 |
| D5 | MVP: plugin `/ping` e `/calc`; slash `/help`, `/config`, `/open`, `/web`, `/library`, `/ai` | 04/09 |
| D6 | Cartella unica `Configuration\`; **nessuna variabile d'ambiente `LARE_*` né `LOCALAPPDATA`/`APPDATA` come sorgente di configurazione Lare**; unico override `--config-dir` (eccezioni fuori scope: le chiavi dei provider AI — `ANTHROPIC_API_KEY`, `OPENROUTER_API_KEY` — restano env; la scoperta delle cartelle Dropbox per `/find` legge `APPDATA`, integrazione con un'app terza, non configurazione Lare) | 04/09 |
| D7 | `Test Run\` dentro il repo, specchio del layout di deploy, copiabile fuori senza modifiche | 04/09 |
| D8 | Repo nuovo `mauriziolobello/lare-terminal-2`, slegato dalla v1: crate **copiati** | 04/09 |
| D9 | **pwsh 7+**; Windows PowerShell 5.1 non supportata. Conseguenza (§12): Linux/macOS saranno *pwsh-flavored* | 04/09; conseguenza da confermare |
| D10 | Tutti i crate v1 copiati e compilanti; nell'MVP verificate dal vivo solo le superfici di D5 | 04/09 |
| D11 | Codice scritto da subagenti su modelli meno costosi (Sonnet, Haiku); Fable supervisiona | 04/09 |
| D12 | **Lato shell = host custom del motore PowerShell in C#** (`lare-shell`), come `pwsh.exe` è una host: possiede il REPL, legge con PSReadLine, intercetta `/…`, esegue il resto in-process | 04/09 sera; **provato dallo spike 1** |
| D13 | **Lare Terminal = finestra Tauri** con emulatore xterm.js che ospita `lare-shell` via ConPTY; barre e segnalini in HTML fuori dall'area terminale; scrollback dell'emulatore | 05/09; **provato dallo spike 2** |
| D14 | **Ogni output dei comandi slash, `/ai` incluso, va in una finestra Markdown**; nel terminale restano prompt `[Y/n]`, una riga di conferma, errori di sintassi | 04/09 sera |
| D15 | AI Chat, Library, `/config`, `/help`: **finestre uniche per macchina** (`/aichat` a finestra aperta → focus). Conversazione `/ai` **per sessione** shell | 04/09 sera |
| D16 | **Tutta** la documentazione sotto `Docs/i18n/<lingua>/`, italiano di riferimento | 04/09 sera |
| D17 | **Una sola cwd**: quella della sessione PowerShell; ogni componente la riceve col comando | 04/09 sera |

## 2. Architettura

```
ui.exe (Tauri v2 — crate `ui` v1 evoluto)
 ├─ finestra "Lare Terminal"  ── xterm.js ↔ ConPTY (portable-pty) ↔ lare-shell.exe (host C#)
 │    chrome HTML: riga segnalini (alto) · barra comandi cliccabili (basso)
 ├─ finestre v1: Markdown (output slash), /config, /library, /help, plugin, /find, AI Chat
 └─ WS ← orchestrator (role "ui": riceve tutto ciò che è "apri/aggiorna finestra")

lare-shell.exe  ── WS persistente (role "shell") ──┐
Telegram (in-process, v1) ─────────────────────────┤
                                                   ▼
                              orchestrator.exe (v1 + estensioni)
   ① Router: slash noto → dispatch · `/ai "…"`/`/ "…"` → AI · ignoto da shell → discard+log
   ② ToolClient per canale (trait v1): shell → ShellSessionToolClient (NUOVO: ExecInShell sul WS)
                                       Telegram/AI Chat → McpToolClient (v1, shell posseduta)
   ③ Routing di superficie (NUOVO): ServerMsg "finestra" → connessione ui; testo → mittente
   plugin host sidecar (v1) · mcp-server (v1) · pytools MCP Python (v1)
```

I tre strati v1 (canali → orchestratore → server MCP) restano. Cambia il **primo canale**: la
shell dell'utente è una host custom del motore PowerShell, connessa all'orchestratore per tutta la
durata della sessione, resa a schermo da una finestra Tauri.

### 2.1 Componenti

| Componente | Stato | Cosa fa nel 2.0 |
|---|---|---|
| `shell/lare-shell/` → `lare-shell.exe` | **nuovo (C#, .NET 10, `Microsoft.PowerShell.SDK` 7.6.x)** | Host custom: `PSHost`/`PSHostUserInterface`/`PSHostRawUserInterface`; runspace con PSReadLine (funzione `PSConsoleHostReadLine`, come `ConsoleHost.cs`); REPL; riga `/…` → WS; altro → `AddScript(riga) \| Out-Default`; `NotifyBegin/EndApplication` come `ConsoleHost`; client WS persistente; `[Y/n]` del gate; `ExecInShell`; OSC 9001 verso l'emulatore |
| `crates/ui` (Tauri) | esteso | **Pagina host headless** (`host.html`/`host.js`: finestra `main` nascosta che possiede la connessione WS di default, lancia ogni finestra e fa da relay per AI Chat, `/find`, plugin — vedi §5); finestra terminale (xterm.js vendored + `portable-pty`, chrome HTML); finestra Markdown di output; `startup.json`/`--config-dir` (fase 2 v1, mai fatta); `UiPing`; `OpenUiLocal`; singleton (D15). **Rimosso**: cursore overlay F2, line-editor, tab-completion, global-shortcut, idle-duck (D2) |
| `orchestrator` — `ws.rs` | esteso | Registro connessioni con ruolo (`ui`/`shell`), cwd e history **per connessione shell** (la history v1 è già per connessione: `ws.rs:298`); routing di superficie |
| `orchestrator` — `shell_session_tool_client.rs` | **nuovo** | `impl ToolClient`: `run_in_session` → `ExecInShell` sul WS della sessione, attesa `ExecResult` |
| `orchestrator` — `router.rs` / `core.rs` | esteso | `/ai "…"` ≡ `/ "…"`; discard+log; `/ping`; apertura finestra Markdown per l'output slash (D14) |
| `protocol` | esteso (additivo) | `Hello.role` + `Hello.session_id`, `ExecInShell`, `ExecResult`, `OpenUiLocal`, `UiPing`/`UiPong`, `OpenOutputWindow`/`OutputWindowContent`, `ActivityIndicator`, `ServerMsg::surface()`. `Command.cwd` e `CancelCommand` esistono già in v1 |
| `startup-config` | modificato | Via il livello env; `--config-dir`; percorsi relativi a exe_dir |
| `plugin-ping` | copiato, invariato | Il handler `/ping` lo attiva e misura `Init→Ready`; versione da `plugin.json` |
| `mcp-server`, `mcp-nmap`, `plugin-protocol`, `plugin-calc`, pytools | copiati | Solo `--config-dir` al posto dell'env var ereditata |
| `plugin-counter`, `plugin-lc`, `plugin-crypto` | copiati, non deployati in `Test Run\` nell'MVP | Compilano; rientrano quando serviranno |

### 2.2 Versioni

Ogni crate/progetto del 2.0 riparte da **`2.0.0`** con una voce di `CHANGELOG.md` "2.0.0 — fork da
v1 x.y.z". `lare-shell` parte da `2.0.0`.

### 2.3 Ciclo di vita e due modalità d'ingresso

- **Modalità A — app (predefinita):** l'utente avvia `ui.exe`; si apre la finestra terminale;
  `ui.exe` lancia `lare-shell.exe --config-dir <dir> --session <id>` dentro una ConPTY; la host si
  connette al WS; se l'orchestratore non risponde, **`ui.exe`** lo avvia (`autostart.orchestrator`).
- **Modalità B — profilo Windows Terminal:** `lare-shell.exe` nudo in una scheda WT (fragment
  installato da `install-wt-profile.ps1`, come nello spike). La host avvia orchestratore e `ui.exe`
  se assenti (§6.4). Nessuna barra VT nel terminale (D13: le barre sono HTML in modalità A);
  segnalini nel titolo della scheda (OSC 2) — fuori MVP, §12.
- **Gara all'avvio:** due host che partono insieme tentano entrambe di avviare l'orchestratore; il
  bind della porta WS decide (il secondo processo esce), le host ritentano la connessione per 5 s.
- **Vincoli ConPTY (spike 2):** la lettura dalla pty **non riceve EOF** all'uscita del figlio —
  l'uscita si rileva con `Child::wait()` in un thread dedicato. Chiusura della finestra = morte della
  pty = morte della host (verificato: nessun orfano). Dimensione iniziale della pty = colonne/righe
  dell'emulatore al primo `fit`.

## 3. Routing dei comandi

La host manda **ogni** riga che inizia con `/` all'orchestratore con `Command{input, cwd}`; non ha
un elenco di comandi proprio (lo possiede l'orchestratore). Tutto il resto va al runspace.

| Riga | Esito |
|---|---|
| `/ai "testo"` · `/ "testo"` | Turno AI (loop tool-use v1; comandi OS via `ShellSessionToolClient`); output nella finestra Markdown (§3.2) |
| `/ai testo` · `/ testo` (senza virgolette) | `Error` "sintassi: `/ai \"testo\"`", stampato nel terminale |
| `/open <target>` · `/web <query>` | Backend slash v1; esito nella finestra Markdown (D14), una riga di conferma nel terminale |
| `/help` | Backend slash v1 (`core.rs`, `HELP_MARKDOWN`) → `OpenWindow` → `ui` (singleton, D15) |
| `/config` · `/library` · `/aichat` | `OpenUiLocal{name}` → `ui` (singleton) |
| `/calc` | Plugin → `OpenPluginWindow` → `ui` (una finestra per invocazione, v1) |
| `/markets`, `/nmap`, `/pyping`, `/lc`, `/crypto`, `/counter` | Come v1 (finestra → `ui`); **non verificati nell'MVP** (D10) |
| `/ping` | §3.1 |
| `/reset` | `Done` con "non applicabile: la sessione è la tua" |
| `/qualunque-altro` | **Scartato**: `Done` muta; log `info` nell'orchestratore (`discard slash: …`) |
| `/find …` · `/nowin …` | **Scartati** in questa versione (come uno slash ignoto): `/find` vive in `ws.rs` fuori dal turno, `/nowin` non ha senso con l'output già in finestra — debito del piano 2a |
| riga senza `/` | Mai vista dall'orchestratore |

### 3.1 `/ping`

Comando built-in dell'orchestratore, una riga per strato, nella finestra Markdown (D14) e una riga
di conferma nel terminale:

```
lare-shell     2.0.0  ok   sessione a1b2
orchestrator   2.0.0  ok   uptime 1h12m · 3 ms
plugin-ping    2.0.0  ok   round-trip 8 ms
ui.exe         2.0.0  ok   12 ms
```

Strato assente: `ui.exe  --  non connesso` / `plugin-ping  --  errore: <motivo>`; il comando finisce
comunque con `Done`. `UiPing{id}`/`UiPong{id, version}` fra orchestratore e `ui`; il plugin
risponde alla `Init` con `Ready` (v1, invariato), versione dal suo `plugin.json`.

### 3.2 Output dei comandi slash: finestra Markdown (D14)

Ogni comando slash originato dalla shell produce il suo output in una **finestra Markdown**, non nel
terminale. Nell'MVP la finestra si apre **all'inizio** del comando con un segnaposto ("in corso…")
e riceve il contenuto **una volta, a `Done`** — come il pannello v1, che renderizza il Markdown a
fine risposta perché il Markdown progressivo (fence aperti, tabelle a metà) è brutto. Lo streaming
token-per-token nella finestra è una slice successiva (§12). I chunk di **trasparenza** dell'AI
("eseguo `dir`…") vanno nella stessa finestra: nel terminale l'utente vede già l'output reale del
comando, che gira lì.

Messaggi: `OpenOutputWindow{window_id, title}` (apre col segnaposto), `OutputWindowContent
{window_id, markdown}` (sostituisce il contenuto; a `Done` o su `Error`). Instradati a `ui`.

**Eccezione**: i comandi il cui esito È già una finestra (`/help`, `/show` — `core::WINDOW_SLASHES`)
non aprono anche la finestra di output; la riga di conferma nel terminale è generica (`→ finestra
aperta`).

Nel terminale, per un comando slash, compaiono solo: il prompt `[Y/n]` del gate (§4.3), l'output
reale dei comandi eseguiti dall'AI, una riga di conferma finale (`→ finestra "…" aperta` oppure
l'errore), gli errori di sintassi.

## 4. Protocollo host ↔ orchestratore

Una connessione WS **persistente per sessione shell** (`127.0.0.1:7331` + token da file), aperta
all'avvio della host e tenuta viva fino all'uscita — a differenza di un client usa-e-getta, permette
all'orchestratore di fare **push** verso la sessione (segnalini, notifiche) anche a prompt fermo.

### 4.1 Messaggi

```jsonc
// host → orchestratore (ClientMsg)
{ "type": "Hello",      "token": "…", "channel": null, "role": "shell", "session_id": "a1b2…", "cwd": "C:\\…", "version": "2.0.0" }  // versione del client, per /ping
{ "type": "Command",    "id": "…", "input": "/ai \"…\"", "input_mode": "…", "command_type": "Auto", "cwd": "C:\\…" }  // v1
{ "type": "ExecResult", "turn_id": "…", "exec_id": "…", "exit_code": 0, "output": "…", "cwd": "C:\\…" }
{ "type": "ToolConfirmResponse", "id": "…", "accept": true }                                                    // v1
{ "type": "CancelCommand", "id": "…" }                                                                          // v1
{ "type": "UiPong", "id": "…", "version": "2.0.0" }                                                             // solo ui
// orchestratore → host / ui (ServerMsg)
{ "type": "ExecInShell", "turn_id": "…", "exec_id": "…", "command": "…", "capture": true }
{ "type": "ToolConfirmRequest", "id": "…", "commands": "…" }                                                    // v1
{ "type": "Done", "id": "…" } · { "type": "Error", "id": "…", "message": "…" }                                  // v1
{ "type": "OpenOutputWindow", "window_id": "…", "title": "…" } · { "type": "OutputWindowContent", "window_id": "…", "markdown": "…" }
{ "type": "OpenUiLocal", "name": "config" }                                                                     // "config" | "library"
{ "type": "UiPing", "id": "…" }
{ "type": "ActivityIndicator", "session_id": "…", "kind": "ai_busy", "on": true }                               // → ui
```

`Hello.role`: `"ui"` | `"shell"`; assente → `"ui"` (compatibilità). `Hello.session_id`: generato
dalla host (o passato da `ui.exe` con `--session`), lega la connessione shell alla finestra
terminale che la ospita. Tutte le aggiunte sono additive: i client v1 restano validi.

### 4.2 Sequenza di un turno `/ai`

```
utente   → digita  /ai "elenca i 3 file più grandi qui"  + Invio
host     → Command{id, input, cwd = $PWD del runspace}
orch     → OpenOutputWindow → ui (segnaposto "in corso…") · ActivityIndicator{ai_busy: on} → ui
orch     → ToolConfirmRequest{commands}                       ← gate ADR-007 v1, invariato
host     → prompt "[Y/n]" nel terminale · ToolConfirmResponse
orch     → ExecInShell{turn, exec, "Get-ChildItem … | Sort …", capture: true}
host     → esegue nel runspace (thread REPL); output a schermo; ExecResult{exit_code, output, cwd}
orch     → (l'AI continua; altri ExecInShell possibili) → OutputWindowContent{markdown} → ui · Done
host     → stampa "→ finestra \"…\" aperta" · torna al prompt
```

Il REPL è **bloccato** durante il turno, come per qualunque comando: PSReadLine non legge finché il
comando non è finito. La host resta reattiva ai messaggi WS perché il client WS gira su un thread
proprio — ma vedi §4.4.

### 4.3 Gate di conferma

`ToolConfirmRequest` → la host mostra il batch di comandi e chiede `[Y/n]` nel terminale (lettura
tasto diretta, non PSReadLine). Timeout lato orchestratore come v1 (nega). Nessun `ExecInShell`
arriva senza risposta affermativa. Con `capture: false` (§4.5) il prompt dichiara "interattivo".

### 4.4 Vincoli di esecuzione nella host

- **Runspace a thread singolo.** Una runspace esegue una pipeline alla volta. `ExecInShell` e il
  prompt `[Y/n]` vengono **marshalizzati sul thread del REPL** (che sta aspettando `Done` del
  comando `/…`), mai invocati dal thread del socket. Il thread WS accoda; il REPL consuma.
- **Ctrl+C.** Durante l'attesa del turno: la host manda `CancelCommand{id}` (v1), stampa una riga,
  torna al prompt; l'orchestratore cancella il turno e la finestra mostra "annullato". Durante un
  `ExecInShell`: la host ferma la pipeline (`PowerShell.Stop()`) **e** cancella il turno — stessa
  semantica della v1 (Ctrl+C = stop di tutto), non un `ExecResult` parziale.
- **Programmi esterni.** `NotifyBeginApplication`/`NotifyEndApplication` salvano/ripristinano le
  modalità console (output e input) come `ConsoleHost.cs:1227-1270` (lezione spike 1).
- **Profilo.** La host carica `$PROFILE.CurrentUserAllHosts` (`profile.ps1`) e il proprio
  `LareShell_profile.ps1`; carica **anche** `Microsoft.PowerShell_profile.ps1` (quello di pwsh) —
  raccomandazione, così alias, oh-my-posh e moduli dell'utente appaiono in Lare come in pwsh. Da
  confermare.
- **Execution policy.** Lo spike la forzava a `RemoteSigned` in-process (senza, PSReadLine `.psm1`
  non si carica). Il prodotto la risolve come `ConsoleHost` (scope di registro/utente): voce di
  verifica del piano, §13.
- **stdin rediretto**: PSReadLine saltato (fallback `Console.ReadLine`), come `ConsoleHost`.

### 4.5 `ExecInShell.capture` — quando l'output torna all'AI

Lezione dello spike 1: un comando nativo eredita la console **solo** se nella pipeline non c'è
nulla fra lui e `Out-Default`. Quindi:
- `capture: true` (default): pipeline `<cmd> | <cmdlet di cattura> | Out-Default` — l'output
  (stdout+stderr fusi, cap 200 KB testa+coda) torna nell'`ExecResult`; i programmi che pretendono un
  terminale (editor, REPL, pager) si comportano male, come in v1.
- `capture: false`: pipeline `<cmd> | Out-Default` pura, console attaccata: editor/REPL/wizard
  funzionano; `output` torna vuoto, restano `exit_code` e `cwd`. L'AI lo chiede con l'input
  opzionale `interactive: true` del tool `run_in_session` (esposto solo dal
  `ShellSessionToolClient`).
- **Bonus gratuito**: l'output dei **cmdlet** passa comunque dai `Write*` della
  `PSHostUserInterface` della host, quindi è catturabile senza pipe qualunque sia il flag; solo
  l'output dei **programmi nativi** dipende dal flag.

### 4.6 cwd e history per sessione (D17)

`Command.cwd` = `$PWD` del runspace al momento dell'invio; ogni `ExecResult.cwd` la aggiorna.
L'orchestratore la tiene **per connessione shell** e non tocca mai il `cwd_state` globale v1 (che
resta la cwd della shell posseduta, usata da Telegram/AI Chat). Prompt di sistema AI, `/find`,
plugin (`Activate.args`) e pytools ricevono quella. La history della conversazione AI è già per
connessione in v1 (`ws.rs:298`): una per sessione shell, gratis (D15).

### 4.7 Canale diretto host → emulatore (OSC 9001)

Indipendente dal WS: la host emette `ESC ] 9001 ; lare ; <evento> ; <dato> ESC \` sulla console
(ESC costruito da `(char)0x1B`, mai `"\x1b…"` — lezione spike 1); xterm.js lo cattura con
`registerOscHandler(9001, …)`; ConPTY lo lascia passare (verificato, spike 2). Nell'MVP: evento
`intercept` (ultimo `/comando`). Il segnalino "AI al lavoro" arriva invece dal WS
(`ActivityIndicator` → `ui`), perché lo sa l'orchestratore.

## 5. Finestra terminale e finestre (`ui`)

- **Finestra "Lare Terminal"**: griglia a tre righe — riga segnalini (nome, sessione, orologio,
  `ultimo: /comando`, pallino "AI al lavoro") · area xterm.js · barra con i comandi cliccabili
  `/help /library /aichat /config /ai` (click = scrive il comando nella shell, come digitato).
  Tema Campbell (pwsh in WT), font Cascadia Mono; alpha/colore/font da `config.json` (v1). **Una
  finestra, una sessione** nell'MVP; schede/più finestre in §12.
- **xterm.js** (6.x, vendored come marked/DOMPurify in v1) + addon-fit; `write(Uint8Array)` dai
  chunk base64 della pty; `onData` → `pty_write`; `ResizeObserver` → fit (debounce) → `pty_resize`.
- **Rust (`ui`)**: `portable-pty` 0.9 (ConPTY su Windows, forkpty su unix); comandi Tauri
  `pty_spawn`/`pty_write`/`pty_resize`; thread lettore + *exit watcher*; spawn della host con
  `--config-dir` e `--session`.
- **Finestra Markdown di output** (§3.2): sanificata (marked+DOMPurify v1), segnaposto poi
  contenuto, `💾` Library come v1.
- **Singleton (D15)**: AI Chat, Library, `/config`, `/help` — una finestra per macchina; richiesta a
  finestra aperta → focus. Registro per etichetta nel crate `ui`.
- **Pagina host headless — ciò che dell'overlay deve sopravvivere.** Verificato sulla v1: `app.js`
  (la pagina del cursore) non è solo il cursore — è **l'unico proprietario della connessione WS di
  default** (`Hello` senza `channel`), **l'unico lanciatore** di ogni finestra (tutti gli
  `open_*_window` sono invocati solo da lì) e il **relay** che riceve `AiChat*`/`Search*`/
  `OpenPluginWindow` su quella connessione e li rigira alle finestre come eventi Tauri; solo la
  connessione principale alimenta il sink del `PluginHost` (`ws.rs:253`,
  `connection_owns_plugin_sink`). Togliere l'overlay senza sostituire questo ruolo spegne AI Chat,
  `/find` e le finestre plugin. Quindi: la finestra `main` resta, **nascosta** (`visible:false`,
  `skipTaskbar`, non trasparente), con `host.html`/`host.js` = client WS di default + dispatcher
  dei `ServerMsg` verso le finestre + i tre moduli di relay v1 (`search-buffer.js`,
  `aichat-push.js`, `external-channels.js`) importati invariati. Niente input, niente rendering.
  Le finestre che aprono una connessione propria (`window.js` per `library-expand`,
  `external-channel-window.js` per `/nmap` ecc.) restano com'erano.
- **Rimosso (D2)**: il cursore di `app.js` (rendering, `Chunk`, line-editor, tab-completion,
  routing slash digitato), `index.html`, `line-editor.js`, `idle-duck.js`, `page-scroll.js`,
  `cwd-format.js`, `connection-diagnosis.js` coi loro test, il plugin global-shortcut e
  `apply_hotkey`, i comandi `hide_overlay`/`show_overlay`/`list_path_completions`, e dalla `Config`
  i campi `action_key`, `cursor_*`, `position`, `activity_indicator`, `idle_duck_minutes` (restano
  `window_alpha`, `web_search_enabled`) con la relativa tab di `/config`.
- **Flag di sviluppo temporaneo** `ui.exe --open config|library`: fra la rimozione dell'overlay
  (piano 1) e l'arrivo del canale shell (piano 2) nessuna superficie può aprire una finestra; il
  flag rende verificabile il piano 1 dal vivo. Etichettato dev-only, rimosso nel piano 3.

## 6. Configurazione

### 6.1 Risoluzione della cartella (unica regola per tutti i binari)

1. `--config-dir <path>` sulla riga di comando, se presente.
2. Altrimenti `<cartella dell'eseguibile>\Configuration\` (per `lare-shell.exe`, che vive in
   `shell\`: `..\Configuration\`).

Vale per `orchestrator.exe`, `ui.exe`, `lare-shell.exe`, `mcp-server.exe`, `mcp-nmap.exe`, plugin,
server Python. **Nessuna variabile d'ambiente `LARE_*` né `LOCALAPPDATA`/`APPDATA` come sorgente
di configurazione Lare**. I figli ricevono `--config-dir` esplicito dal padre. Il crate
`startup-config` v1 perde il livello env; la host C# legge lo stesso `startup.json` con lo stesso
schema. Eccezioni fuori scope di questa regola (non sono configurazione Lare): le chiavi dei
provider AI (`ANTHROPIC_API_KEY`, `OPENROUTER_API_KEY`) restano variabili d'ambiente; la scoperta
delle cartelle Dropbox per `/find` legge `APPDATA` — è un'integrazione con un'app terza (Dropbox
scrive lì il proprio `info.json`), non un modo per configurare Lare.

### 6.2 Contenuto di `Configuration\`

| File 2.0 | Da v1 | Note |
|---|---|---|
| `startup.json` | `startup.json` + le 5 env residue | §6.3; nessun segreto → committato come template |
| `token` | `Local\token` | auto-generato (256 bit, v1); gitignored |
| `llms.json` | `Local` | provider AI + API key; gitignored |
| `config.json` | `Roaming` | UI: colore, font, alpha, ricerca web… (senza `action_key`: niente hotkey) |
| `network.json`, `search-paths.json`, `search-content.json`, `market_data.json`, `notes.json` | `Local` | invariati |
| `telegramsettings.json`, `telegram-state.json`, `memory-*.md` | accanto all'exe / `Local` | gitignored |
| `routines\`, `plugin-storage\`, `library\` | `Local` / `Roaming` | invariati |
| `logs\` | — | `orchestrator.log`, `ui.log`, `lare-shell.log`, `mcp-server.log`; rotazione giornaliera, 7 file (`ui.log` non ancora implementato: `ui.exe` logga su stdout — vedi HANDOFF DA FARE) |

### 6.3 `startup.json`

```jsonc
{
  "ws_port": 7331,
  "paths": {                         // relativi alla cartella radice del deploy (padre di Configuration\), oppure assoluti
    "shell":        "shell/lare-shell.exe",
    "mcp_server":   "mcp-server.exe",
    "mcp_nmap":     "mcp-nmap.exe",
    "plugins_dir":  "plugins",
    "pytools_dir":  "pytools",
    "routines_dir": "Configuration/routines"
  },
  "ai_model": "claude-sonnet-4-6",
  "autostart": { "orchestrator": true, "ui": true },
  "log": { "level": "info", "dir": "Configuration/logs" }
}
```

Tutti i campi opzionali con questi default; file assente = default. Percorsi relativi risolti
rispetto alla **cartella radice del deploy** (quella che contiene `Configuration\`), così
`Test Run\` copiata altrove funziona senza modifiche. I log vanno **solo su file**: un
orchestratore avviato in autostart non deve sporcare il terminale; `init_*.ps1` aggiungono la console.

### 6.4 Avvio e self-heal

- `ui.exe` (modalità A) o `lare-shell.exe` (modalità B) non raggiungono il WS → se
  `autostart.orchestrator`, avviano `orchestrator.exe` e ritentano per **5 s**; altrimenti errore.
- L'orchestratore senza connessione `ui` entro **3 s** (o quando deve instradare un messaggio
  "finestra") → se `autostart.ui`, avvia `ui.exe`.
- **Processi staccati**: `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP` su Windows (nessuna console
  ereditata, un Ctrl+C nella shell non abbatte il daemon), stdio chiusi; `setsid` su unix.
- Servizio Windows, autorun al login, tray: fuori MVP (§12).

## 7. `Test Run\` — layout di deploy dentro il repo

Specchio del layout di deploy (D7). Committata la **struttura** (cartelle, manifest, script,
template di configurazione); mai binari, DLL, segreti, venv, dati d'uso (`.gitignore`).

```
Test Run\
├── orchestrator.exe  mcp-server.exe  mcp-nmap.exe  ui.exe     ← gitignored, da deploy_test_run.ps1
├── shell\                                                    ← lare-shell.exe + DLL (~100 MB: il
│                                                                NuGet Microsoft.PowerShell.SDK porta
│                                                                l'intero motore); gitignored
├── install-wt-profile.ps1 / uninstall-wt-profile.ps1         ← modalità B (profilo Windows Terminal)
├── init_orchestrator.ps1  init_tauri.ps1                     ← avvio manuale, zero env var
├── Configuration\                                            ← §6.2
├── plugins\ping\plugin.json  plugins\calc\plugin.json        (+ exe gitignored)
└── pytools\                                                  ← script sì, venv no (v1)
```

**Prerequisiti sulla macchina di destinazione** (da scrivere nel `DEPLOY.md` 2.0): Windows 10/11
x64, WebView2 Runtime, **pwsh 7.6+ installato** (la host carica PSReadLine dai moduli di pwsh:
`C:\Program Files\PowerShell\7\Modules`; il NuGet non lo include), **.NET 10 runtime** se
`lare-shell` è pubblicata *framework-dependent* (raccomandato: ~100 MB invece di ~200 MB
self-contained; da confermare).

`deploy_test_run.ps1` (root repo): `cargo build` → exe Rust; `dotnet publish -c Release` →
`Test Run\shell\`; plugin. Non tocca `Configuration\`. Sviluppo: `cargo run -p ui -- --config-dir
"Test Run\Configuration"`.

## 8. Sicurezza

- WS solo su `127.0.0.1` + token su file: invariato.
- Il gate ADR-007 resta l'unico punto di autorizzazione di un comando proposto dall'AI: `[Y/n]`
  nel terminale **prima** di qualunque `ExecInShell`.
- La host esegue **solo** ciò che arriva come `ExecInShell` sulla connessione WS autenticata, dopo
  il gate; mai testo da altre vie (l'OSC 9001 è solo in uscita).
- I comandi girano coi privilegi dell'utente nel suo runspace: nessuna escalation, stessa
  superficie di un comando digitato a mano.
- Le finestre Markdown sanificano tutto (marked+DOMPurify, v1).

## 9. Gestione errori

| Situazione | Comportamento |
|---|---|
| WS non raggiungibile, autostart off o fallito entro 5 s | `ui.exe`/host: errore nel terminale, la shell **resta usabile** (i `/comandi` rispondono "orchestratore non raggiungibile") |
| WS cade durante un turno | Host: riga d'errore, torna al prompt; turno cancellato lato orchestratore (disconnessione = cancel, v1); riconnessione automatica con backoff |
| Token errato | Rifiuto, come v1; riga d'errore nel terminale |
| Nessuna connessione `ui` per un messaggio "finestra" | Autostart `ui.exe`, attesa 10 s, poi `Error` al mittente ("finestra non disponibile") |
| Slash ignoto da shell | `Done` muta; log `info` |
| `/ai` o `/` senza virgolette | `Error` con la sintassi corretta, stampato |
| Ctrl+C in attesa del turno / durante `ExecInShell` | `CancelCommand`; pipeline fermata; turno cancellato; finestra "annullato" |
| Comando che richiede un terminale con `capture: true` | Si blocca come in v1; Ctrl+C (riga sopra); l'AI può ripetere con `interactive: true` |
| Pipeline in errore in `ExecInShell` | Testo dell'errore nell'`output`, `exit_code` 1 (o `$LASTEXITCODE` / `$?`), il turno continua |
| Output dell'exec > 200 KB | Troncato testa+coda con marcatore |
| `startup.json` malformato | Log + default, mai panic (v1) |
| `--config-dir` inesistente | Creata al primo avvio |
| La host esce (crash/`exit`) in modalità A | Exit watcher → la finestra mostra "shell terminata" e un pulsante "riavvia" |

## 10. Test

Convenzioni v1 non derogabili: TDD con RED reale; test come specifica leggibile; trait come
confini; fake ai seam. Si estendono a C# (xUnit) e JS (`node:test`).

**C# — `lare-shell` (xUnit)**: riconoscimento riga `/…` (spazi iniziali, `/` solo, `/ "x"`);
costruzione pipeline per `capture` true/false (un cmdlet in mezzo vs `Out-Default` puro);
cattura via `PSHostUserInterface.Write*`; client WS contro un server finto in-process: sequenza
`Hello→Command→ToolConfirmRequest→ToolConfirmResponse→ExecInShell→ExecResult→Done`; marshaling
sul thread REPL (un `ExecInShell` arrivato dal thread socket viene eseguito dal thread REPL);
`CancelCommand` su Ctrl+C; `NotifyBegin/EndApplication` salva/ripristina; `startup.json` e
`--config-dir`; OSC 9001 costruito senza `\x` (test che fallisce se ricompare un `"\x1b"`).

**Rust**: `protocol` — `ServerMsg::surface()` su tutte le varianti, serde dei messaggi nuovi,
`Hello` senza `role` → `ui`; `orchestrator` — `ShellSessionToolClient` con canale finto; cwd per
connessione shell (non tocca il globale); routing di superficie; router (`/ai "x"` ≡ `/ "x"`,
senza virgolette → `Error`, ignoto da shell → discard+log, da Telegram → v1); `/ping` degrada per
strato; apertura finestra Markdown a inizio comando e contenuto a `Done`; `startup-config` senza
env; `ui` — pty plumbing con una shell finta (`cmd /c echo`), base64 dei chunk, exit watcher,
singleton, `OpenUiLocal`, `UiPing`.

**JS (`node:test`)**: logica pura estratta in `.mjs` (stato segnalini, parsing OSC 9001, base64 →
`Uint8Array`, debounce del fit, registro singleton).

**E2E manuale** (`Docs/i18n/ita/TESTING-e2e.md`): apertura `ui.exe` → finestra terminale con pwsh
dentro · `/ping` · `/calc` · `/config` `/library` `/help` · `/open` `/web` · `/ai "elenca i 3
file più grandi qui"` → `[Y/n]` → esegue nel terminale → finestra Markdown col risultato ·
`/ai "vai in Documents"` → `cd` persiste · rifiuto al gate · Ctrl+C nei due momenti ·
`/nonesiste` muto · `python` via `/ai` con `interactive` · `/aichat` due volte → una finestra ·
chiusura finestra → nessun processo residuo · modalità B in WT · copia di `Test Run\` altrove.

## 11. Convenzioni di progetto (ereditate dalla v1, non derogabili)

TDD genuino · OOP+SOLID (trait/interfacce come confini, composizione) · commenti prodighi e
didattici (l'utente impara Rust; in C# lo stesso stile) · `CHANGELOG.md` + `IMPLEMENTATION.md` per
crate/progetto nello stesso commit · `Docs/i18n/ita/HANDOFF.md` a ogni release (hook `commit-msg`
v1 da riportare) · commit con trailer `Co-Authored-By` + `Claude-Session` · codice scritto da
subagenti Sonnet/Haiku (D11), report **sempre riverificati**, il supervisore decide le architetture.
Tre linguaggi: Rust (orchestratore, protocollo, ui, plugin), C# (host), JS (webview, vanilla). Il
`CLAUDE.md` del repo 2.0 le riporta (Task 0 del piano).

## 12. Fuori MVP (esplicito)

- **Streaming nella finestra Markdown** (token per token, con re-render throttled): l'MVP mostra
  segnaposto → contenuto a `Done` (§3.2).
- **Schede / più finestre terminale** in `ui.exe`; **modalità B con segnalini nel titolo** (OSC 2).
- **bash/zsh**: con la host custom, Linux/macOS ricevono Lare *pwsh-flavored* (pwsh gira lì, la
  host è cross-platform). Un Lare per bash/zsh cambia forma: widget zle/`bind -x` + client sottile
  che parla lo stesso protocollo di §4, oppure la finestra terminale che lancia bash **senza**
  intercettazione. Non progettato qui — conseguenza di D9/D12 da confermare.
- Output testuale dei plugin nel terminale, namespace gerarchici (`/markets quote NVDA`).
- Verifica dal vivo di Telegram, AI Chat, `/find`, `/markets`, nmap, `/lc`, `/crypto`, `/counter`
  (D10). **Routine dal canale shell**: ora banale (corpo via `get_routine_content`, esecuzione via
  `ExecInShell`) ma fuori MVP per D10.
- Servizio Windows, autorun, tray, supervisione (Fase 5 v1); port macOS/Linux (Fase 6); voce
  (Fase 3); backlog v1 (`STATO-ATTUALE.md` §16); estrazione di `archive.rs` da `ui` (non serve
  finché `ui.exe` è l'unico host delle finestre).

## 13. Verifiche per il piano (da spike, prima del codice relativo)

- **Execution policy** nella host: risolverla come `ConsoleHost` (scope), non forzarla.
- **Profili**: ordine e set di file caricati (§4.4); verificare che oh-my-posh/PSReadLine
  configurati dall'utente in `Microsoft.PowerShell_profile.ps1` funzionino nella host.
- **`generate_context!`** di Tauri incorpora `frontendDist` a compile time: `rerun-if-changed`
  in `build.rs` o `cargo clean -p ui` documentato (spike 2).
- **Flicker al resize**: debounce del fit e/o renderer WebGL di xterm.js.
- **Codepage** dell'output nativo catturato con `capture: true` (KNOWN-ISSUE v1): misurare;
  eventualmente `[Console]::OutputEncoding` UTF-8 nel runspace.
- **Publish** della host: framework-dependent vs self-contained; dimensione e prerequisiti (§7).
- **Protocollo host↔orchestratore**: **mai spikato** (i due spike stampavano in locale). Primo task
  di implementazione con test contro server finto, non ultimo.
