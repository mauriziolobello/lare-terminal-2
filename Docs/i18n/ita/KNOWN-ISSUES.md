# Known issues — Lare Terminal 2.0

Due famiglie di problemi noti: quelli **ereditati dalla v1** (sezioni sotto, nessun task del
piano 1/2a ha toccato le aree coinvolte) e quelli **nativi del 2.0**, introdotti da una decisione
di design del piano corrente (marcati come tali). Per lo storico completo dei problemi v1
(inclusi quelli già risolti) vedi
`C:\Users\Maurizio\Documents\Progetti\Lare Terminal\Docs\KNOWN-ISSUES.md` (sola lettura).

---

## [APERTO] Codepage — output dei comandi NATIVI (`ipconfig`, …) → accenti come `S�`

**Segnalato:** dal vivo dall'utente in v1, 2026-06-26 (`ipconfig /all` → "Configurazione automatica
abilitata : S�", dove dovrebbe esserci `Sì`). **Ancora presente in 2.0**: il codice di
`crates/mcp-server/src/session.rs` copiato dalla v1 all'inizio del piano 1 non è stato modificato
da nessun task del piano.

**Sintomo.** I caratteri accentati nell'output dei **comandi nativi** Win32 (`ipconfig`, ecc.)
appaiono come U+FFFD (`�`). L'output *proprio* di PowerShell (`Write-Output`) è corretto.

**Causa.** `crates/mcp-server/src/session.rs` inietta nella sessione
`[Console]::OutputEncoding = UTF8` + `$OutputEncoding = UTF8`: copre la codifica del testo
*generato da PowerShell*, ma i comandi **nativi** interrogano direttamente il code page della
console (`GetConsoleOutputCP`) ed emettono byte nel codepage **OEM** (CP850/437), poi decodificati
come UTF-8 → mojibake. Un `chcp 65001` risolverebbe, ma emette "Active code page: 65001" su
stdout, corrompendo il protocollo a marker — per questo non è stato applicato in v1.

**Anche in `crates/mcp-nmap/src/network_info.rs`** (`local_network_info`/`traceroute`): stesso
`String::from_utf8_lossy` diretto sull'output di `ipconfig`/`arp`/`route`/`netstat` — le etichette
italiane si corrompono, i dati (IP/subnet/gateway/MAC) restano leggibili (ASCII).

**Rilevanza per il piano 2/3.** Nella 2.0 l'utente interagisce con la shell reale tramite
`lare-shell` (host C# del motore PowerShell, ADR-015/016), non più con la sessione posseduta di
`mcp-server` — quella resta solo per i canali senza shell utente (Telegram, AI Chat, vedi
`CLAUDE.md`). Il problema quindi si sposta: per l'utente locale bisognerà verificare se ConPTY
(che restituisce byte grezzi del programma, non testo ridecodificato da `mcp-server`) ha lo stesso
sintomo o lo evita per costruzione — da verificare quando il piano 2/3 costruirà quel percorso. Per
Telegram/AI Chat il problema architetturale resta identico alla v1.

---

## [APERTO] Reconnect infinito su un canale esterno permanentemente rotto

**Segnalato:** come debito architetturale nella v1 (`Docs/STATO-ATTUALE.md` §16: "un fallimento
permanente di canale esterno (config/venv mancante) causa reconnect infinito invece di un errore
mostrato una volta"). **Ancora presente in 2.0**, verificato leggendo il codice copiato:
`crates/ui/frontend/ws-client.js` (`_scheduleRetry`) non ha un numero massimo di tentativi — il
backoff esponenziale è **capped** a `RETRY_MAX_MS` ma non si ferma mai. Ogni finestra che apre una
propria connessione (`external-channel-window.js`, `config-dialog.js`, `window.js`, oltre alla
connessione di default di `host.js`) usa la stessa `LareWsClient` e quindi lo stesso comportamento.

**Sintomo.** Se un canale esterno (es. un plugin con endpoint sbagliato, o una dipendenza mancante
lato server — `venv` non trovato, config assente) fallisce **in modo permanente** (non transitorio),
la finestra continua a ritentare la connessione all'infinito invece di mostrare una volta un
messaggio d'errore stabile e smettere.

**Rilevanza per il piano 2/3.** Non toccato dal piano 1: il piano ha cambiato solo il parametro
`url` di `ws-client.js` (per rendere la porta configurabile via `startup.json`), mai la logica di
retry. Resta un debito architetturale aperto da affrontare quando si costruirà l'esperienza utente
attorno ai canali esterni.

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
