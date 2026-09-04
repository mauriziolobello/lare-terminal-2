# Lare Terminal 2.0 — Design

> **Stato:** spec approvata a sezioni in chat il 2026-09-04 (brainstorming), in attesa della revisione
> finale dell'utente sul file. Prossimo passo dopo l'approvazione: piano di implementazione
> (`Docs/superpowers/plans/`).
>
> **Cosa decide questo documento:** il modello di esecuzione 2.0 (front-end integrato nella shell
> reale), il protocollo fra shell, client CLI e orchestratore, la configurazione unificata, il
> layout di deploy, i confini dell'MVP. **Cosa NON decide:** la struttura interna dei crate copiati
> dalla v1 (resta quella, vedi `Docs/STATO-ATTUALE.md` del repo v1) e tutto ciò che è elencato in
> §11 "Fuori MVP".
>
> **Punti di partenza:** repo v1 `C:\Users\Maurizio\Documents\Progetti\Lare Terminal` (leggere
> `Docs/STATO-ATTUALE.md`, Parte II §18-20 — contratti verificati sul codice e domande aperte);
> Google Doc "Lare Terminal 2.0" (bozza di un'altra AI, usata solo come inquadramento — vedi §1.2);
> layout di deploy v1 `C:\Lare Terminal`.

## 1. Contesto e obiettivo

### 1.1 Obiettivo (parole dell'utente)

> L'obiettivo di questa versione 2.0 è che l'utente scrive nella stessa riga di comando della
> finestra PowerShell, i comandi slash vengono interpretati dal nostro programma. Se esiste il
> corrispondente comando esso viene elaborato, se non esiste il comando viene silenziosamente
> scartato.

Vincolo dichiarato: **mantenere il più possibile** di quanto costruito nella v1 ("implementato in
maniera molto robusta e riutilizzabile"). Comportamento **non difforme dalla v1** per le superfici
che restano: `/markets`, `/calc`, `/library` aprono le stesse finestre Tauri di oggi.

### 1.2 Cosa si prende dalla bozza Google Doc e cosa no

La bozza è stata scritta senza conoscere la v1. Si adotta come **inquadramento**: "command layer
sopra le shell esistenti" (mai una nuova shell, mai hook tastiera globali), adapter shell sottile →
daemon che ignora quale shell lo chiama, self-heal del daemon all'avvio del client. Si **scarta**
ciò che ri-deriva cose già esistenti (named pipe al posto del WS v1, `hubd`/`hubctl` al posto
dell'orchestratore, "moduli compilati ora e WASM dopo" al posto dei plugin sidecar) e la forma
request/response del protocollo, incompatibile con lo streaming (token AI, trasparenza tool,
cancel, gate di conferma) su cui vive la v1. L'output testuale dei plugin (`/calc 5*20 → 840`) è
lavoro nuovo, non riuso (i plugin v1 emettono solo HTML): fuori MVP, §11.

### 1.3 Decisioni prese (2026-09-04)

| # | Decisione | Nota |
|---|---|---|
| D1 | I comandi OS proposti dall'AI eseguono **nella shell reale dell'utente** fin dalla prima versione (Lare ospite, non proprietario) | Meccanismo *exit-and-resume*, §4. La shell posseduta dal `mcp-server` v1 resta per i canali senza shell utente |
| D2 | L'overlay F2 **sopravvive** come secondo canale accanto alla shell | Comportamento v1 invariato sull'overlay (NL senza slash → AI, comandi OS → `mcp-server`) |
| D3 | Linguaggio naturale solo con slash: `/ai "testo"` e `/ "testo"` equivalenti, **virgolette obbligatorie** | Mai testo nudo, mai fallback `CommandNotFoundAction` (verificato: smonta gli argomenti) |
| D4 | Slash sconosciuto digitato nella shell → **scartato in silenzio** nel terminale, una riga di log nell'orchestratore | Dall'overlay resta `Error` come v1 |
| D5 | MVP: plugin `/ping` e `/calc`; slash `/help`, `/config`, `/open`, `/web`, `/library`, `/ai` | `/ping` diventa comando utente (v1: solo prova di concetto interna) |
| D6 | Cartella unica `Configuration\` al posto di `Local\` + `Roaming\`; **nessuna variabile d'ambiente** letta da alcun binario; unico override `--config-dir` | §5 |
| D7 | `Test Run\` dentro il repo, specchio di `C:\Lare Terminal`, copiabile fuori senza modifiche | §6 |
| D8 | Repo GitHub nuovo e privato `mauriziolobello/lare-terminal-2`, completamente slegato dalla v1: crate **copiati**, niente path-dep né subtree | La v1 resta congelata come riferimento |
| D9 | pwsh **7+** unica shell Windows supportata; bash/zsh dopo, stesso `lare.exe` | Windows PowerShell 5.1 non supportata |
| D10 | Tutti i crate v1 copiati e compilanti; nell'MVP verificate dal vivo solo le superfici di D5 | Telegram, AI Chat, `/find`, `/markets`, nmap, routine: "come v1, non testate" fino a slice dedicata |
| D11 | Il codice lo scrivono subagenti su modelli meno costosi (Sonnet, Haiku dove basta); Fable supervisiona e decide l'architettura | Convenzione v1 (`lare-builder` Sonnet), rafforzata |

## 2. Architettura

```
Shell utente (pwsh 7+; bash/zsh dopo, stesso lare.exe)
 │ hook Enter PSReadLine: riga inizia con "/" → Invoke-Lare (wrapper lare.ps1)
 │   riga non "/" → PowerShell normale, Lare non la vede mai
 ▼
lare.exe  (NUOVO crate `cli`, Rust) ──── WS 127.0.0.1:7331 + token ──┐
ui.exe    (overlay F2 + finestre, v1) ── WS, Hello{role: "ui"} ───────┤
Telegram  (in-process, v1) ───────────────────────────────────────────┤
                                                                      ▼
                                              orchestrator.exe (v1 + 3 estensioni)
   ① Router: slash noto → dispatch · `/ai "…"` e `/ "…"` → AI · ignoto da cli → discard+log
   ② ToolClient per canale (trait v1, nessun cambio di firma):
        cli     → ShellProxyToolClient  (NUOVO: exec nella shell utente, exit-and-resume)
        ui      → McpToolClient          (v1: mcp-server, shell posseduta)
        Telegram→ McpToolClient          (v1)
   ③ Routing di superficie (NUOVO): ServerMsg "apri/aggiorna finestra" → connessione `ui`,
        non al mittente; ServerMsg testuali → al mittente (cli)
   plugin host sidecar stdio (v1) · mcp-server (v1) · pytools MCP Python (v1)
```

I tre strati v1 (canali → orchestratore → server MCP) restano. Cambia il **primo canale**: la
shell dell'utente entra tramite un client CLI Rust (`lare.exe`) lanciato da un wrapper PowerShell
che intercetta le righe `/…` prima che PowerShell le analizzi.

### 2.1 Componenti

| Componente | Stato | Cosa fa nel 2.0 |
|---|---|---|
| `crates/cli` → `lare.exe` | **nuovo** | Client WS del canale shell: manda la riga, stampa lo stream in terminale (ANSI), prompt `[Y/n]` per il gate, esce con codice 10 su `ExecInShell`, riprende con `--resume`. Self-heal dell'orchestratore |
| `Test Run\lare.ps1` | **nuovo** | Hook Enter PSReadLine + `Invoke-Lare` (loop exit-and-resume) + `-Install` in `$PROFILE` |
| `orchestrator` — `ws.rs` | esteso | Registro connessioni con ruolo (`ui`/`cli`); registro turni (attach/detach/resume); routing di superficie |
| `orchestrator` — `shell_proxy_tool_client.rs` | **nuovo** | `impl ToolClient`: `run_in_session` → `ExecInShell` + attesa `ExecResult` |
| `orchestrator` — `router.rs` | esteso | `/ai "…"` ≡ `/ "…"` (virgolette obbligatorie); discard+log per slash ignoto da `cli`; `/ping` |
| `protocol` | esteso (additivo) | `Hello.role`, `ExecInShell`, `Detach`, `Resume`, `ExecResult`, `OpenUiLocal`, `UiPing`/`UiPong`, `ServerMsg::surface()`. `Command.cwd` esiste già in v1 (`Option<String>`) e viene semplicemente valorizzato dal CLI |
| `startup-config` | modificato | Via il livello env var; `--config-dir`; percorsi relativi a exe_dir |
| `ui` (Tauri) | esteso | Legge `startup.json`/`--config-dir` (fase 2 v1, mai fatta); risponde a `UiPing`; esegue `OpenUiLocal` come se digitato nel cursore |
| `plugin-ping` | copiato, invariato | Risponde `Ready` a `Init` (v1). Il handler `/ping` dell'orchestratore lo attiva e misura il round-trip `Init→Ready`; la versione viene da `plugin.json`. Manifest resta `triggers: {}` (`/ping` è slash built-in, non trigger plugin) |
| `mcp-server`, `mcp-nmap`, `plugin-protocol`, `plugin-calc`, pytools | copiati | Solo `--config-dir` al posto dell'env var ereditata |
| `plugin-counter`, `plugin-lc`, `plugin-crypto` | copiati, non deployati in `Test Run\` nell'MVP | Compilano; rientrano quando serviranno |

### 2.2 Versioni

Ogni crate del 2.0 riparte da **`2.0.0`**, con una voce di `CHANGELOG.md` "2.0.0 — fork da v1
x.y.z" che cita la versione v1 da cui è stato copiato. Il crate nuovo `cli` parte anch'esso da
`2.0.0`. Storia git v1 non importata (D8).

## 3. Routing dei comandi

Una riga digitata nella shell che inizia con `/` viene intercettata dall'hook e inviata intera
(riga grezza, virgolette e parentesi intatte) all'orchestratore con `Command{line, cwd}`.

| Riga (dalla shell) | Esito |
|---|---|
| `/ai "testo"` · `/ "testo"` | Turno AI (loop tool-use v1; comandi OS via `ShellProxyToolClient`) |
| `/ai testo` (senza virgolette) · `/ testo` | `Error` "sintassi: `/ai \"testo\"`", stampato |
| `/open <target>` · `/web <query>` | Backend slash v1 → `Chunk`+`Done` stampati |
| `/help` | Backend slash v1 (`core.rs`, `HELP_MARKDOWN`) → `OpenWindow` → instradato a `ui` |
| `/config` · `/library` | `OpenUiLocal{name}` → `ui` li tratta come digitati nel cursore |
| `/calc` | Plugin → `OpenPluginWindow` → `ui` |
| `/markets`, `/nmap`, `/pyping`, `/lc`, `/crypto`, `/counter`, `/aichat`, `/find` | Come v1 (finestra → `ui`); **non verificati nell'MVP** (D10) |
| `/ping` | Una riga per strato, §3.1 |
| `/reset` | `Done` con messaggio "non applicabile dalla shell" (la sessione è la tua) |
| `/qualunque-altro` | **Scartato**: `Done` immediata, niente in terminale; riga di log `info` nell'orchestratore (`discard slash: /qualunque-altro`) |
| riga senza `/` | Mai vista da Lare |

Dall'overlay (`role: ui`) il routing resta quello v1, incluso `Error` sullo slash ignoto e il
linguaggio naturale senza slash.

### 3.1 `/ping`

Comando utente che attraversa tutti gli strati e stampa una riga per ciascuno:

```
lare.exe       2.0.0  ok
orchestrator   2.0.0  ok   uptime 1h12m · 3 ms
plugin-ping    2.0.0  ok   round-trip 8 ms
ui.exe         2.0.0  ok   12 ms
```

Se uno strato non risponde: `ui.exe  --  non connesso` (dopo l'eventuale autostart, §5.4) oppure
`plugin-ping  --  errore: <motivo>`; il comando finisce comunque con `Done`. Nuovi messaggi
`UiPing{id}` / `UiPong{id, version}` fra orchestratore e `ui`. Per il plugin non serve nulla di
nuovo: il handler `/ping` lo spawna (o riusa il processo vivo), manda `Init`, cronometra la
`Ready` (protocollo v1 invariato) e legge la versione dal suo `plugin.json`. `/ping` è uno slash
built-in dell'orchestratore, non un trigger `command` del manifest.

## 4. Protocollo shell ↔ CLI ↔ orchestratore

### 4.1 Hook (installato in `$PROFILE` da `lare.ps1 -Install`)

```powershell
# Lare Terminal 2.0 — integrazione shell (pwsh 7+, PSReadLine 2.x)
Set-PSReadLineKeyHandler -Key Enter -ScriptBlock {
    $line = $null; $cursor = $null
    [Microsoft.PowerShell.PSConsoleReadLine]::GetBufferState([ref]$line, [ref]$cursor)
    if ($line -match '^\s*/') {
        [Microsoft.PowerShell.PSConsoleReadLine]::AddToHistory($line)
        $escaped = $line -replace "'", "''"
        [Microsoft.PowerShell.PSConsoleReadLine]::Replace(0, $line.Length, "Invoke-Lare '$escaped'")
    }
    [Microsoft.PowerShell.PSConsoleReadLine]::AcceptLine()
}
Set-PSReadLineOption -AddToHistoryHandler {
    param($line)
    -not ($line -like 'Invoke-Lare *')   # la riga originale è già stata aggiunta sopra
}
```

Perché l'hook Enter e non `CommandNotFoundAction`: verificato il 2026-09-04 che quest'ultimo vede
gli argomenti **dopo** il parsing di PowerShell (`sqrt(123)` → `sqrt|123`, `directory, per` →
array, virgolette perse). L'hook Enter vede il buffer grezzo. Righe che non iniziano con `/`
passano ad `AcceptLine()` senza alcuna modifica.

### 4.2 `Invoke-Lare` — loop exit-and-resume

Un processo figlio non può eseguire comandi nella shell padre. Quindi: quando l'orchestratore
vuole eseguire un comando nella shell dell'utente, `lare.exe` **esce** con codice 10 lasciando la
richiesta in un file di scambio; il wrapper esegue il comando **nella sessione corrente**
(`Invoke-Expression`), scrive il risultato in un secondo file e **rilancia** `lare.exe --resume`.

```
wrapper  → lare.exe run --cwd <pwd> --exchange <tmp>\lare-<PID> -- '<riga>'
lare     → WS: Hello{token, role:"cli"} · Command{id, line, cwd}
orch     → Chunk… (stream AI) · ToolConfirmRequest{id, commands}     ← gate ADR-007 v1, invariato
lare     → prompt "[Y/n]" in terminale · ToolConfirmResponse{id, accept}
orch     → ExecInShell{turn_id, exec_id, command}
lare     → scrive <exchange>.exec.json · Detach{turn_id} · exit 10
wrapper  → Invoke-Expression $cmd 2>&1 | Tee-Object | Out-Host   (output live nel terminale)
         → scrive <exchange>.result.json
         → lare.exe run --resume <turn_id> --exchange <tmp>\lare-<PID>
lare     → WS: Hello · Resume{turn_id} · ExecResult{turn_id, exec_id, exit_code, output, cwd}
orch     → riprende run_in_session → l'AI continua → Chunk… → Done
lare     → exit 0 · wrapper termina
```

Il loop si ripete a ogni `ExecInShell` dello stesso turno (l'AI può eseguire più comandi in
sequenza, ciascuno col proprio gate batched per turno come in v1).

**Codici di uscita di `lare.exe`:** `0` completato · `10` esegui-e-riprendi · `2` orchestratore
non raggiungibile (dopo eventuale autostart) · `1` ogni altro errore (già stampato).

**File di scambio** (prefisso passato dal wrapper: cartella temporanea di sistema, `lare-<PID>`):
- `<prefisso>.exec.json` — `{ "turn_id", "exec_id", "command" }`, scritto da `lare.exe`.
- `<prefisso>.result.json` — `{ "turn_id", "exec_id", "exit_code", "output", "cwd" }`, scritto
  dal wrapper. `output` = stdout+stderr fusi; il wrapper tronca oltre **200 KB** (testa + coda,
  marcatore `[… troncato N byte …]`), stesso principio del cap output v1.
- Entrambi cancellati dal wrapper a fine loop (`finally`).

**Sottocomandi `lare.exe`:** `run` (sopra) · `run --resume` · `abort --turn <id>` (best-effort dal
`finally` del wrapper su Ctrl+C durante l'exec) · `version`. Opzione globale `--config-dir`.

### 4.3 Lato orchestratore

- **Registro connessioni** (`ws.rs`): ogni connessione dichiara `Hello.role` (`"ui"` | `"cli"`;
  assente → `"ui"`, compatibilità con la v1). Al più una connessione `ui` attiva (l'ultima vince).
- **Registro turni** — solo per turni originati da `cli`: `turn_id → { outbound: mpsc unbounded,
  exec_pendente: Option<oneshot>, stato: Attached | Detached }`. Un task *forwarder* per turno
  drena `outbound` verso la connessione attaccata; mentre nessuna è attaccata i messaggi restano in
  coda: **niente perso** fra `exit 10` e `--resume`.
- **`Detach` + disconnessione** = turno vivo in attesa di `Resume`, con timeout
  `resume_timeout_min` (default **30** — un comando lungo lanciato dall'AI non va ucciso).
  **Disconnessione senza `Detach`** (Ctrl+C sul CLI, crash) = cancel del turno, come v1.
- **`ShellProxyToolClient`** (`impl ToolClient`): `run_in_session` → manda `ExecInShell`,
  registra la oneshot, attende `ExecResult`, restituisce `CommandResult{exit_code, output, cwd}`
  (stesso tipo v1). `reset_session` → no-op. `open_target`, `search_routines`, `run_routine`,
  `get_routine_content`, `save_routine` → **delegati** al `McpToolClient` v1 (non hanno bisogno
  della shell dell'utente; composizione, non ereditarietà). `tool_defs`/`dispatch` come
  `McpToolClient`. Decorato da `CwdTrackingToolClient` v1 come oggi.
- **`Command.cwd`** dalla shell inizializza `cwd_state` del turno (prompt di sistema AI, `/find`);
  ogni `ExecResult.cwd` lo aggiorna.
- **Routing di superficie**: metodo `ServerMsg::surface() -> Surface { Ui, Origin }` sul crate
  `protocol`, tabella esaustiva. `Ui`: le 7 varianti che aprono/gestiscono finestre (`OpenWindow`,
  `OpenScreenerPicker`, `SearchOpen`, `OpenPluginWindow`, `UpdatePluginWindow`,
  `ClosePluginWindow`, `RoutineSavePreview`) più le altre UI-only che oggi Telegram degrada a
  no-op (§18.1 STATO-ATTUALE v1), più `OpenUiLocal` e `UiPing`. `Origin`: `Chunk`, `Done`,
  `Error`, `Cwd`, `ToolConfirmRequest`, `ExecInShell`, `Pong`. Per turni `ui` il routing è identità
  (tutto al mittente, come v1). Un messaggio `Ui` con nessuna connessione `ui` disponibile:
  autostart (§5.4), attesa fino a **10 s**, poi scartato con un `Chunk` di avviso al `cli`
  ("finestra non disponibile: ui.exe non raggiungibile").

### 4.4 Messaggi nuovi (tutti additivi, `protocol` resta compatibile con i client v1)

```jsonc
// ClientMsg
{ "type": "Hello",      "token": "…", "channel": null, "role": "cli" }   // role: "ui" | "cli", default "ui"; channel come v1
{ "type": "Command",    "id": "…", "input": "/ai \"…\"", "input_mode": "…", "command_type": "Auto", "cwd": "C:\\…" }  // campi v1 invariati; il CLI valorizza cwd
{ "type": "Detach",     "turn_id": "…" }
{ "type": "Resume",     "turn_id": "…" }
{ "type": "ExecResult", "turn_id": "…", "exec_id": "…", "exit_code": 0, "output": "…", "cwd": "…" }
{ "type": "UiPong",     "id": "…", "version": "2.0.0" }
// ServerMsg
{ "type": "ExecInShell", "turn_id": "…", "exec_id": "…", "command": "…" }
{ "type": "OpenUiLocal", "name": "config" }                           // "config" | "library"
{ "type": "UiPing",      "id": "…" }
```

### 4.5 bash / zsh (dopo l'MVP, D9)

Stesso `lare.exe`, stesso loop, stessi file di scambio. Cambia solo l'hook: zsh — widget che
sostituisce `accept-line` e guarda `$BUFFER`; bash — `bind -x` con `READLINE_LINE`. Il wrapper
`lare.sh` replica `Invoke-Lare` con `eval` + `tee`. Nessuna decisione qui vincola quella slice
oltre al protocollo di §4.2.

## 5. Configurazione

### 5.1 Risoluzione della cartella (unica regola per tutti i binari)

1. `--config-dir <path>` sulla riga di comando, se presente.
2. Altrimenti `<cartella dell'eseguibile>\Configuration\`.

Vale per `orchestrator.exe`, `ui.exe`, `mcp-server.exe`, `mcp-nmap.exe`, `lare.exe`, i plugin e i
server Python. **Nessun binario legge variabili d'ambiente `LARE_*`** (la v1 ne aveva 5 residue
senza equivalente in `startup.json`: `LARE_TOKEN`, `LARE_MCP_SERVER`, `LARE_MCP_NMAP`,
`LARE_PYTOOLS_DIR`, `LARE_AI_MODEL`). I processi figli ricevono `--config-dir` esplicito dal padre
(la v1 lo faceva ereditare come env var, con tre lettori indipendenti — orchestrator, ui, Python —
e divergenze silenziose documentate in `CLAUDE.md` v1). Il crate `startup-config` v1 resta e
perde il livello env: `resolve(file_value, default)`.

### 5.2 Contenuto di `Configuration\`

| File 2.0 | Da v1 | Contenuto / note |
|---|---|---|
| `startup.json` | `startup.json` accanto all'exe + le 5 env residue | Vedi §5.3. **Nessun segreto** → committato come template |
| `token` | `Local\token` | Token WS, auto-generato al primo avvio (256 bit, come v1). Gitignored |
| `llms.json` | `Local` | Provider AI + API key. Gitignored |
| `config.json` | `Roaming` | Impostazioni UI (tasto azione, colore, font, posizione, alpha, ricerca web…) |
| `network.json`, `search-paths.json`, `search-content.json`, `market_data.json`, `notes.json` | `Local` | Invariati |
| `telegramsettings.json`, `telegram-state.json`, `memory-*.md` | accanto all'exe / `Local` | Gitignored |
| `routines\`, `plugin-storage\` | `Local` | Invariati |
| `library\` | `Roaming` | `documents\`, `find\` — invariata |

### 5.3 `startup.json`

```jsonc
{
  "ws_port": 7331,
  "paths": {                         // relativi alla cartella dell'exe, oppure assoluti
    "mcp_server":   "mcp-server.exe",
    "mcp_nmap":     "mcp-nmap.exe",
    "plugins_dir":  "plugins",
    "pytools_dir":  "pytools",
    "routines_dir": "Configuration/routines"
  },
  "ai_model": "claude-sonnet-4-6",   // override del modello del provider attivo in llms.json
  "autostart": { "orchestrator": true, "ui": true },
  "resume_timeout_min": 30,
  "log_level": "info"
}
```

Tutti i campi opzionali con default uguali ai valori sopra; file assente = tutti i default. I
percorsi relativi sono risolti rispetto alla **cartella dell'eseguibile**, non alla cwd né alla
cartella di configurazione — così `Test Run\` copiata altrove funziona senza modifiche (la v1
aveva percorsi assoluti `C:/Lare Terminal/...`).

### 5.4 Avvio e self-heal

- `lare.exe` non raggiunge il WS → se `autostart.orchestrator`, avvia `orchestrator.exe` (stessa
  cartella, processo staccato) e ritenta per **5 s**; altrimenti errore, exit 2.
- L'orchestratore senza connessione `ui` entro **3 s** dall'avvio (o quando serve instradare un
  messaggio `Ui`) → se `autostart.ui`, avvia `ui.exe` (stessa cartella, processo staccato).
- Servizio Windows, autorun al login, tray: fuori MVP (§11). Gli script `init_*.ps1` restano per
  l'avvio manuale.

## 6. `Test Run\` — layout di deploy dentro il repo

Specchio di `C:\Lare Terminal` (D7). Committata la **struttura** (cartelle, manifest, script,
template di configurazione); mai binari, segreti, venv, dati generati dall'uso (`.gitignore`).

```
Test Run\
├── orchestrator.exe  mcp-server.exe  mcp-nmap.exe  ui.exe  lare.exe   ← gitignored, da deploy_test_run.ps1
├── lare.ps1               ← Invoke-Lare + hook (§4.1-4.2); `.\lare.ps1 -Install` scrive
│                             `. "<percorso assoluto>\lare.ps1"` in $PROFILE (lo crea se manca —
│                             sulla macchina di sviluppo oggi non esiste)
├── init_orchestrator.ps1  ← & "$PSScriptRoot\orchestrator.exe" — nessuna env var
├── init_tauri.ps1         ← & "$PSScriptRoot\ui.exe"
├── Configuration\         ← §5.2
├── plugins\
│   ├── ping\plugin.json   (+ ping.exe gitignored)
│   └── calc\plugin.json   (+ calc.exe gitignored)
└── pytools\               ← script + requirements committati, venv no (come v1)
```

`deploy_test_run.ps1` (root del repo): copia da `target\<debug|release>\` in `Test Run\` gli exe e
gli exe dei plugin. Non tocca `Configuration\`. Sviluppo: `cargo run -p orchestrator --
--config-dir "Test Run\Configuration"` — la stessa configurazione del deploy, nessun doppione.
Copiando `Test Run\` in `C:\Lare Terminal 2.0` basta rieseguire `lare.ps1 -Install` da lì (il
percorso in `$PROFILE` è assoluto).

## 7. Sicurezza

- WS solo su `127.0.0.1` + token su file: invariato dalla v1.
- Il gate di conferma ADR-007 resta l'unico punto in cui un comando proposto dall'AI viene
  autorizzato: `lare.exe` mostra il batch e chiede `[Y/n]` **prima** di qualunque `ExecInShell`.
  Nessun comando arriva al wrapper senza risposta affermativa.
- Il wrapper esegue **solo** ciò che legge da `<prefisso>.exec.json` scritto da `lare.exe` nello
  stesso loop (file nella cartella temporanea dell'utente, nome legato al PID della shell,
  cancellato a fine loop). Non esegue mai testo ricevuto per altre vie.
- Il comando gira coi privilegi dell'utente nella sua sessione: nessuna escalation, stessa
  superficie di un comando digitato a mano.
- L'hook Enter modifica il buffer solo per righe che iniziano con `/`; ogni altra riga passa ad
  `AcceptLine()` inalterata.

## 8. Gestione errori

| Situazione | Comportamento |
|---|---|
| WS non raggiungibile, autostart off o fallito entro 5 s | `lare.exe` stampa l'errore, exit 2 |
| Token errato | Rifiuto della connessione, come v1; `lare.exe` stampa l'errore, exit 1 |
| Nessuna connessione `ui` per un messaggio `Ui` | Autostart `ui.exe`, attesa 10 s, poi scarto + `Chunk` di avviso al `cli` |
| Slash ignoto da `cli` | `Done` muta; log `info` nell'orchestratore |
| `/ai` o `/` senza virgolette | `Error` con la sintassi corretta, stampato |
| `Resume` con `turn_id` sconosciuto o scaduto | `Error`, exit 1 |
| `Detach` senza `Resume` entro `resume_timeout_min` | Turno cancellato, log `warn` |
| Disconnessione `cli` senza `Detach` | Turno cancellato (v1) |
| Ctrl+C durante l'exec nella shell | `finally` del wrapper → `lare.exe abort --turn` best-effort; altrimenti scade il timeout |
| `Invoke-Expression` solleva eccezione | Messaggio nell'`output`, `exit_code` 1, il turno continua (l'AI vede l'errore) |
| Output dell'exec > 200 KB | Troncato testa+coda dal wrapper con marcatore |
| `startup.json` malformato | Log + default, mai panic (v1) |
| `--config-dir` inesistente | Creata al primo avvio (come le cartelle app-data v1) |

## 9. Test

Convenzioni v1 non derogabili: TDD con RED reale prima del codice; i test come specifica leggibile
(l'utente impara Rust leggendoli); trait come confini, fake ai seam.

**Rust**
- `protocol`: `ServerMsg::surface()` testata su **tutte** le varianti (un test che fallisce se ne
  viene aggiunta una senza classificazione); serde round-trip dei messaggi nuovi; `Hello` senza
  `role` → `ui`.
- `orchestrator`: registro turni (attach/detach/resume; messaggi bufferizzati mentre staccato e
  consegnati in ordine al resume; timeout → cancel; disconnessione senza `Detach` → cancel);
  `ShellProxyToolClient` con canale finto (`ExecInShell` emesso, `ExecResult` → `CommandResult`;
  delega a `McpToolClient` finto per `open_target`/routine); routing di superficie (messaggio `Ui`
  → connessione `ui`; `Origin` → mittente; nessuna `ui` → hook autostart invocato, poi avviso);
  router (`/ai "x"` ≡ `/ "x"`; senza virgolette → `Error`; ignoto da `cli` → discard+log; ignoto
  da `ui` → `Error` v1); `/ping` aggrega le righe e degrada per strato assente.
- `cli`: parsing argomenti e codici di uscita; scrittura/lettura dei file di scambio; sequenza
  completa contro un server WS finto in-process (`Hello→Command→…→ExecInShell→Detach→exit 10`,
  poi `Resume→ExecResult→…→Done→exit 0`); autostart chiamato quando il WS è chiuso.
- `startup-config`: nessun livello env; `--config-dir` > `exe_dir\Configuration`; percorsi
  relativi risolti su exe_dir; default completi con file assente.
- `ui`: lettura `startup.json`/`--config-dir`; `UiPing` → `UiPong`; `OpenUiLocal` — logica pura
  estratta in `.mjs` e testata con `node:test`, convenzione v1.

**PowerShell** — Pester su `Invoke-Lare` con un `lare.exe` finto (script che esce 10 una volta con
un `.exec.json` noto, poi 0): verifica il `.result.json` (exit code, output, `cwd`), che un `cd`
nel comando sopravviva al loop, la pulizia dei file, e `abort` chiamato su interruzione. Se Pester
non è disponibile sulla macchina: gli stessi casi nella checklist manuale.

**E2E manuale** (`Docs/TESTING-e2e.md`, da compilare a mano prima di ogni release):
`/ping` con tutti gli strati · `/calc` apre la finestra · `/config`, `/library`, `/help` ·
`/open`, `/web` · `/ai "elenca i 3 file più grandi qui"` → `[Y/n]` → esegue nella shell, cwd
invariata · `/ai "vai in Documents"` → il `cd` persiste dopo il turno · rifiuto al gate ·
Ctrl+C sul CLI e Ctrl+C durante l'exec · `/nonesiste` muto · overlay F2 in parallelo alla shell
· copia di `Test Run\` in un'altra cartella e riavvio senza modifiche.

## 10. Convenzioni di progetto (ereditate dalla v1, non derogabili)

TDD genuino · OOP+SOLID (trait come confini, composizione preferita a ereditarietà; mappare i
costrutti Rust su concetti OOP noti nelle spiegazioni) · commenti prodighi e didattici (l'utente
impara Rust leggendo il codice) · `CHANGELOG.md` + `IMPLEMENTATION.md` per crate aggiornati nello
stesso commit · `Docs/HANDOFF.md` aggiornato a ogni release (hook `commit-msg` v1 da riportare) ·
commit con trailer `Co-Authored-By` + `Claude-Session` · il codice lo costruiscono subagenti su
modelli meno costosi (D11), il supervisore rivede, riesegue i test, fa l'e2e, decide le
architetture. Il `CLAUDE.md` del repo 2.0 le riporta (primo task del piano).

## 11. Fuori MVP (esplicito)

- Hook bash/zsh e `lare.sh` (§4.5): design pronto, non costruito.
- Output testuale dei plugin nel terminale (`/calc 5*20 → 840`), namespace gerarchici
  (`/markets quote NVDA`): feature nuove, da brainstormare a parte.
- Verifica dal vivo di Telegram, AI Chat, `/find`, `/markets`, nmap, routine, `/lc`, `/crypto`,
  `/counter`: codice copiato e compilante (D10), ciascuno con slice dedicata.
- Servizio Windows, autorun al login, tray icon, supervisione/restart (Fase 5 v1).
- Port macOS/Linux (Fase 6 v1), voce (Fase 3 v1).
- Backlog v1 (`Docs/STATO-ATTUALE.md` §16): `/towin`, Library broadcast, `find_files` AI,
  codepage output nativi, reconnect infinito su canale esterno rotto, strumenti dinamici, AI↔AI
  slice 3+.
- Estrazione di `archive.rs` (Library) dal crate `ui` (§20.4 STATO-ATTUALE v1): non necessaria
  finché `ui.exe` resta l'unico host delle finestre; `OpenUiLocal{library}` la raggiunge com'è.
