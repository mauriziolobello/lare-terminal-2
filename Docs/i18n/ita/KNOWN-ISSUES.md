# Known issues — Lare Terminal 2.0

Due famiglie di problemi noti: quelli **ereditati dalla v1** (sezioni sotto, nessun task del
piano 1/2a ha toccato le aree coinvolte) e quelli **nativi del 2.0**, introdotti da una decisione
di design del piano corrente (marcati come tali). Per lo storico completo dei problemi v1
(inclusi quelli già risolti) vedi `KNOWN-ISSUES.md` nel
[repository della versione precedente](https://github.com/mauriziolobello/lare-terminal).

---

## [PARZIALE] Codepage — output dei comandi NATIVI (`ipconfig`, …) → accenti come `S`

**Segnalato:** dal vivo dall'utente in v1, 2026-06-26 (`ipconfig /all` → "Configurazione automatica
abilitata : S", dove dovrebbe esserci `Sì`).

**Stato in 2.0**:
- **Risolto in `crates/mcp-nmap` 2.0.1** (`local_network_info`/`traceroute` in `network_info.rs`):
  l'output stdout/stderr dei comandi nativi Win32 viene ora decodificato interrogando il codepage
  della console (`GetConsoleOutputCP()`) o il codepage OEM di sistema (`GetOEMCP()` in caso di processo
  senza console sotto `CREATE_NO_WINDOW`) tramite il crate `oem_cp` (`decode_oem`).
- **Ancora aperto in `crates/mcp-server/src/session.rs`** (canali Telegram / AI Chat): il codice
  copiato dalla v1 non è stato modificato (fuori scope per il compito su `mcp-nmap`). In quella sessione
  l'output mescola testo generato da PowerShell (UTF-8) con output di comandi nativi Win32 (OEM),
  richiedendo una strategia riga per riga o ConPTY.

**Sintomo.** I caratteri accentati nell'output dei **comandi nativi** Win32 (`ipconfig`, ecc.)
appaiono come U+FFFD (``) quando eseguiti nella sessione di `mcp-server`. L'output *proprio* di
PowerShell (`Write-Output`) è corretto.

**Causa.** `crates/mcp-server/src/session.rs` inietta nella sessione
`[Console]::OutputEncoding = UTF8` + `$OutputEncoding = UTF8`: copre la codifica del testo
*generato da PowerShell*, ma i comandi **nativi** interrogano direttamente il code page della
console (`GetConsoleOutputCP`) ed emettono byte nel codepage **OEM** (CP850/437), poi decodificati
come UTF-8 → mojibake. Un `chcp 65001` risolverebbe, ma emette "Active code page: 65001" su
stdout, corrompendo il protocollo a marker — per questo non è stato applicato in v1.

**Rilevanza per il piano 2/3.** Nella 2.0 l'utente interagisce con la shell reale tramite
`lare-shell` (host C# del motore PowerShell, ADR-015/016), non più con la sessione posseduta di
`mcp-server` — quella resta solo per i canali senza shell utente (Telegram, AI Chat, vedi
`CLAUDE.md`). Il problema quindi si sposta: per l'utente locale bisognerà verificare se ConPTY
(che restituisce byte grezzi del programma, non testo ridecodificato da `mcp-server`) ha lo stesso
sintomo o lo evita per costruzione — da verificare quando il piano 2/3 costruirà quel percorso. Per
Telegram/AI Chat il problema architetturale resta identico alla v1.

---

## [RISOLTO] Reconnect infinito su un canale esterno permanentemente rotto

**Segnalato:** come debito architetturale nella v1 (`Docs/STATO-ATTUALE.md` §16: "un fallimento
permanente di canale esterno (config/venv mancante) causa reconnect infinito invece di un errore
mostrato una volta").

**Risolto in `crates/ui` 2.3.3**:
- `crates/ui/frontend/ws-client.js`: introdotto parametro opzionale `maxRetries` per-istanza in
  `LareWsClient`. Quando i tentativi consecutivi raggiungono `maxRetries`, il client smette di schedulare
  timer ed emette lo stato terminale `"failed"`.
- `crates/ui/frontend/external-channel-window.js`: passa `maxRetries: 6` (~31s totali di backoff prima di fermarsi).
  `renderer.js` visualizza lo stato terminale con badge tradotto (`ext_channel.status_failed`).
- **Nota architetturale importante:** la connessione principale della finestra (`host.js`, canale cursore)
  mantiene **deliberatamente** il retry infinito (nessun `maxRetries` passato). Questo vincolo garantisce
  che la UI cursore si riconnetta automaticamente se l'orchestratore viene riavviato (build, crash, self-healing),
  anche dopo minuti, senza lasciare l'utente con l'interfaccia bloccata.
- Corretto contestualmente il reset improprio di `_retryMs` sull'evento TCP `"open"`, spostandolo sulla
  ricezione effettiva di `server_info` (handshake applicativo).

---

## [APERTO] Markdown della finestra di output: chunk di trasparenza e testo AI concatenati senza separatore

**Introdotto nel piano 2a** (nativo del 2.0, non ereditato dalla v1): `surface::route_shell_turn`
bufferizza ogni `ServerMsg::Chunk` di un turno shell (testo di trasparenza dell'AI — "eseguo
`dir`…" — e la risposta vera e propria) concatenandoli in un unico `String`, consegnato una volta
alla finestra come `OutputWindowContent` a `Done`/`Error`. Nessun separatore (newline doppia,
riga orizzontale, …) viene inserito fra un chunk e il successivo.

**Sintomo.** Se l'AI produce trasparenza multi-riga seguita da testo di risposta, i due possono
finire attaccati nella stessa "riga" Markdown renderizzata (es. l'ultima riga della trasparenza e
la prima del testo si fondono in un solo paragrafo) — puramente cosmetico, il contenuto informativo
resta tutto presente e leggibile.

**Rilevanza per il piano 2b/3.** Nessun impatto funzionale: non blocca né la lettura né il salvataggio
del contenuto in Library. Da affrontare quando si rivedrà la resa della finestra di output (insieme
allo streaming token-per-token, fuori MVP — spec §12).

---

## [APERTO] Debiti nativi della host `lare-shell` (piano 2b)

- **Gate `[Y/n]` accettato da tasti pendenti (osservato UNA volta, non riprodotto).** Nella prima passata dell'e2e con AI reale i primi due gate del turno risultavano già accettati e la `y` del terzo è comparsa anche al prompt successivo: tasti duplicati/pendenti nel buffer della console (Windows Terminal + ConPTY, driver SendKeys). Protezione in `ConsoleGate.Ask`: il buffer viene svuotato PRIMA del prompt (il gate risponde solo a un tasto premuto dopo) e i tasti scartati finiscono nel log (`gate: scartato tasto pendente …`, `gate: input mode …`). Nelle passate successive nessun tasto pendente. Se ricompare, il log dice cosa c'era.

**Introdotti nel piano 2b** (nativi del 2.0): decisioni deliberate della host C# (ruling del piano,
ADR-019), non bug scoperti per caso. Dettaglio implementativo completo in
`shell/lare-shell/IMPLEMENTATION.md` §Debiti; qui solo il sintomo osservabile dall'utente.

- **Profili AllUsers non caricati.** `ProfileLoader` carica solo i profili CurrentUser
  (`profile.ps1`, `Microsoft.PowerShell_profile.ps1`, `LareShell_profile.ps1`); i profili
  AllUsers (quelli nel `$PSHOME` di pwsh, condivisi fra tutti gli utenti della macchina) non
  vengono caricati. Se qualcosa di importante per la sessione vive lì (raro: la maggior parte
  delle personalizzazioni — alias, oh-my-posh, moduli — sta nel profilo CurrentUser), non sarà
  presente in Lare.
- **Log della host senza rotazione.** `<config-dir>\logs\lare-shell.log` cresce senza limite né
  rotazione giornaliera (a differenza del log dell'orchestratore) — ruling 7 del piano, debito
  dichiarato: la spec (§6.2) chiederebbe una rotazione a 7 file giornalieri.
- **`$?` nel prompt dopo un turno slash.** I comandi digitati dall'utente non sono mai toccati da
  un turno `/…` (ruling 3: solo il comando eseguito PER CONTO dell'AI, dentro `ExecInShell`, viene
  avvolto in `try { … } finally { $global:__lare_ok = $? }`) — quindi `$?` a livello di prompt
  resta sempre quello dell'ultimo comando digitato dall'utente, non del turno appena concluso: un
  prompt/oh-my-posh che leggesse `$?` subito dopo un `/ai` vedrebbe lo stato del comando digitato
  PRIMA di quel `/ai`, non un riflesso del turno.
- **`ui.exe` avviata da `lare-shell` con `UseShellExecute=true`, finestra nascosta.** Se dopo un
  self-heal (`Launcher.EnsureUi()`) le finestre di `ui.exe` (Markdown, `/config`, `/library`) non
  compaiono subito in primo piano, è il comportamento standard del focus di Windows per un processo
  avviato da un altro (nessuna `SetForegroundWindow` forzata) — **non un bug**: la finestra esiste
  ed è visibile, va solo selezionata (Alt+Tab o click in barra applicazioni).
- **`ToolConfirmRequest` non porta il `turn_id`.** Il messaggio `ToolConfirmRequest` (v1) ha un
  `id` opaco, non il `turn_id` del canale shell: durante un turno, `SlashTurn` attribuisce OGNI
  richiesta di conferma ricevuta al turno corrente (contratto (a): un solo turno alla volta per
  connessione, quindi non c'è ambiguità pratica), e scarta a inizio turno ciò che fosse rimasto in
  coda da un turno già chiuso.
- **All'avvio, `EnsureConnected` può bloccare fino a ~8 s (3 s di connect + retry) con Ctrl+C
  no-op.** Capita solo se qualcosa ascolta sulla porta 7331 senza rispondere (non il caso comune
  di "nessuno in ascolto", che fallisce subito, né quello di un vero orchestratore, che risponde
  entro la finestra). L'handler di `Console.CancelKeyPress` È già agganciato in quella finestra
  (lo si registra prima di `ConnectAtStartup`) e SCATTA regolarmente su Ctrl+C — ma non ha nulla
  da fermare: `_turnCts` è `null` (nessun turno slash in corso) e `Executor.StopCurrent()` non ha
  una pipeline attiva, quindi l'unico effetto è la riga nel log ("Ctrl+C ricevuto (turno in corso:
  False)", F2a) mentre il loop di connect/retry prosegue fino alla propria scadenza.
