# Known issues — Lare Terminal 2.0

Problemi noti ereditati dalla v1 e ancora presenti nel codice 2.0: nessun task del piano 1 ha
toccato le aree coinvolte. Per lo storico completo dei problemi v1 (inclusi quelli già risolti)
vedi `C:\Users\Maurizio\Documents\Progetti\Lare Terminal\Docs\KNOWN-ISSUES.md` (sola lettura).

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
