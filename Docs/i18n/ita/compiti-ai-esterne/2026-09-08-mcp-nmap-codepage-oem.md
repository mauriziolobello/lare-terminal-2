# Compito per AI esterna — fix codepage OEM in `mcp-nmap` (recupero da v1)

> **Prima cosa**: leggi per intero `Docs/i18n/ita/BRIEFING-AI-ESTERNE.md` alla radice del
> repository. Questo file presume che tu l'abbia già letto — non ripete le regole generali del
> progetto, solo il compito specifico.

## Contesto — perché questo bug esiste

Bug ereditato dalla v1, segnalato dal vivo dall'utente il 2026-06-26: l'output dei comandi
diagnostici di rete nativi di Windows (`ipconfig`, `arp`, `route`, `netstat`) mostra le etichette
accentate italiane come mojibake — `Sì` diventa `S�` (U+FFFD, carattere di sostituzione Unicode).
I DATI numerici (IP, subnet, gateway, indirizzi MAC) restano leggibili perché sono ASCII puro —
solo le etichette italiane ("Server DNS", "Indirizzo IPv4 predefinito", ecc., generate da
`ipconfig` stesso) si corrompono.

**Causa esatta**: questi comandi sono binari Win32 nativi, non generati da PowerShell — scrivono
byte direttamente nel **codepage OEM** della console (in Italia tipicamente CP850, ma dipende
dalla localizzazione di Windows: potrebbe essere CP437 o altro), non in UTF-8. Il codice che li
invoca decodifica invece i byte assumendo sempre UTF-8
(`String::from_utf8_lossy`), e ogni byte OEM che non è anche una sequenza UTF-8 valida diventa
U+FFFD.

Documentato per intero in `Docs/i18n/ita/KNOWN-ISSUES.md`, sezione "Codepage — output dei comandi
NATIVI (`ipconfig`, …) → accenti come `S�`" — leggila, contiene anche perché `chcp 65001` NON è
la soluzione (corromperebbe il protocollo a marker di un altro punto del programma, non toccato
da questo compito).

## Scope — SOLO questo, non di più

**Tocca un solo file**: `crates/mcp-nmap/src/network_info.rs`, funzione `run_and_capture` (righe
~26-54 al momento in cui scrivo — verifica i numeri aggiornati leggendo il file). Il suo
doc-comment attuale dice esplicitamente "non risolto qui deliberatamente" — è il punto esatto in
cui il progetto ha lasciato questo bug in attesa di essere ripreso.

**NON toccare `crates/mcp-server/src/session.rs`.** Ha lo stesso sintomo ma un problema più
difficile e volutamente FUORI SCOPE per questo compito: in quel file, l'output di UNA sessione
PowerShell mescola testo generato da PowerShell stesso (già corretto, decodificato come UTF-8
grazie a un'iniezione di `[Console]::OutputEncoding` già presente) CON output di eventuali comandi
nativi lanciati dentro quella sessione (ancora OEM) — un fix lì richiederebbe distinguere riga per
riga quale delle due fonti ha prodotto quel testo, un problema diverso e più grande. Se durante il
lavoro ti accorgi di dettagli utili per un futuro compito su `session.rs`, scrivili nel report
finale (sezione "Deviazioni dal compito assegnato" del template in `BRIEFING-AI-ESTERNE.md`), ma
non implementare nulla lì.

`crates/mcp-nmap/src/network_info.rs` è invece il caso PULITO: **tutto** l'output catturato da
`run_and_capture` viene da un processo nativo Win32 (mai da PowerShell) — nessuna ambiguità su
quale codepage usare per decodificarlo, sempre quello attivo sulla console.

## Cosa fare

1. Sostituisci la decodifica ingenua (`String::from_utf8_lossy` diretta sui byte grezzi) con una
   decodifica che usa il **codepage OEM realmente attivo sulla console**, non un valore fisso
   indovinato (mai hardcodare "850": un utente con Windows in inglese US potrebbe avere 437, e
   sbagliare qui riprodurrebbe lo stesso bug con un travestimento diverso).

2. **Approccio consigliato per COME si ottiene il codepage e si decodificano i byte** (i nomi di
   funzione/crate qui sotto non sono vincolanti al carattere — puoi usare un'alternativa
   equivalente se la documentazione del crate ti mostra un'API diversa da quella descritta qui,
   spiegalo nel report — ma il **contratto di comportamento** del punto 3 più sotto NON è
   negoziabile, è quello che i test verificano):
   - Leggi il codepage output realmente attivo con la funzione Win32 `GetConsoleOutputCP`,
     raggiungibile tramite il crate `windows` (versione `0.62`, **già dipendenza di questo
     crate** — vedi `crates/mcp-nmap/Cargo.toml`, sezione `[target.'cfg(windows)'.dependencies]`,
     già usato in `crates/mcp-nmap/src/elevate.rs` per un'altra chiamata Win32: usa quel file
     come riferimento di stile per come si importano/chiamano funzioni di questo crate). Serve
     probabilmente aggiungere la feature `Win32_System_Console` all'elenco features già presente
     in `Cargo.toml` — verifica sulla documentazione del crate (`docs.rs/windows/0.62`) il modulo
     e la firma esatti di `GetConsoleOutputCP`.
   - **Punto critico, non ovvio — leggilo con attenzione**: `mcp-nmap.exe` viene lanciato
     dall'orchestratore SENZA una console propria (`CREATE_NO_WINDOW`, fix recente dello stesso
     progetto — vedi `crates/orchestrator/src/nmap_tool_client.rs` per il contesto, non toccarlo).
     `GetConsoleOutputCP()` chiamata da un processo senza console attiva ritorna **0** (non un
     errore Rust, un valore di ritorno Win32 che significa "nessuna console"). Se ignori questo
     caso, il codepage letto sarà sempre 0 in produzione (quando gira davvero sotto
     l'orchestratore) — il fix funzionerebbe nei test/da riga di comando manuale ma MAI nell'uso
     reale, un fallimento silenzioso che nessun test qui sotto può catturare (i test lavorano su
     `decode_oem`, non su come il codepage viene letto). Se `GetConsoleOutputCP()` ritorna 0, usa
     invece `GetOEMCP()` (stessa famiglia Win32, probabilmente sotto la stessa feature
     `Win32_Globalization` di cui hai bisogno per la decodifica — verifica su `docs.rs`): ritorna
     il codepage OEM di sistema, sempre disponibile indipendentemente da una console attiva.
   - Decodifica i byte OEM in una `String` usando il crate `oem_cp`
     (<https://crates.io/crates/oem_cp>, <https://github.com/tats-u/rust-oem-cp>) — pura logica
     Rust, nessun `unsafe`, copre CP437/CP850/CP852/CP857 e altri codepage OEM comuni tramite
     `DECODING_TABLE_CP_MAP` (una tabella indicizzata per numero di codepage — lo stesso numero
     che ritornano `GetConsoleOutputCP`/`GetOEMCP`, ma **verifica il tipo esatto della chiave**:
     la ricerca fatta per preparare questo compito suggerisce che la tabella sia indicizzata per
     `u16`, mentre le funzioni Win32 ritornano `u32` — se è così serve una conversione esplicita
     e verificata (`u16::try_from(codepage)`, gestendo l'errore se il numero non ci sta — non un
     `as u16` silenzioso, che tronca senza avvisare). Aggiungi `oem_cp` come nuova dipendenza in
     `crates/mcp-nmap/Cargo.toml`, con un commento che spiega perché esiste (stesso stile degli
     altri commenti di dipendenza già nel file — guardali, sono didattici). Verifica sulla
     documentazione del crate (`docs.rs/oem_cp`) i nomi esatti delle funzioni di decodifica: la
     ricerca fatta per preparare questo compito ha trovato `decode_string_checked`/
     `decode_string_lossy` come punti d'ingresso probabili, ma verifica tu la firma esatta prima
     di usarli.
   - Struttura consigliata: una funzione pura e testabile `decode_oem(bytes: &[u8], codepage:
     u32) -> String` (nessuna chiamata di sistema dentro — prende il codepage già come numero,
     così è testabile su QUALUNQUE piattaforma, non solo Windows) + una funzione separata
     `#[cfg(windows)] fn active_console_output_codepage() -> u32` che fa la lettura del codepage
     reale (con la gestione del caso "ritorna 0" descritta sopra — non testabile in isolamento in
     modo affidabile, è una chiamata di sistema, va bene che resti non coperta da un test
     unitario). `run_and_capture`, su Windows, chiama `active_console_output_codepage()` poi
     `decode_oem`; su piattaforme non-Windows (`#[cfg(not(windows))]`) resta
     `String::from_utf8_lossy` com'è oggi — i comandi diagnostici di questo modulo sono già
     Windows-only (vedi gli stub `#[cfg(not(windows))]` più sotto nello stesso file), quindi il
     ramo non-Windows di `run_and_capture` è raggiungibile solo da lì.

3. **Contratto di `decode_oem` — questo NON è negoziabile, è ciò che i test verificano**: se il
   codepage passato non è nella tabella di `oem_cp` (numero sconosciuto/non OEM), `decode_oem`
   deve ripiegare ESATTAMENTE su `String::from_utf8_lossy(bytes)` — lo stesso identico
   comportamento di oggi, non un'euristica diversa (es. non assumere CP437 come default silenzioso
   se la tabella non trova il codepage: sarebbe un'invenzione, non un fallback). Mai un errore
   fatale/panic che impedisca al tool di rispondere. I dati (IP/MAC/ecc., ASCII puro) restano
   leggibili anche in caso di fallback, solo le etichette italiane tornano a essere mojibake come
   oggi — meglio di un tool che smette di funzionare.

4. Aggiorna il doc-comment di `run_and_capture` (oggi dice "non risolto qui deliberatamente" — non
   è più vero dopo questo fix, riscrivilo per riflettere la soluzione).

## Test — TDD, questi sono i vettori di verità

(verificati con Python `str.encode('cp850')` durante la stesura di questo compito, non a
memoria — puoi riverificarli tu stesso allo stesso modo se hai un interprete Python a
disposizione)

Scrivi (PRIMA del fix, devono fallire con l'implementazione attuale — RED reale) test per
`decode_oem` con questi casi, nello stesso stile Italiano-descrittivo già usato nel modulo
(guarda `#[cfg(test)] mod tests` in fondo al file per lo stile esatto dei nomi):

```rust
// "Sì" in CP850 (Europa occidentale) = byte [0x53, 0x8D]
assert_eq!(decode_oem(&[0x53, 0x8D], 850), "Sì");

// "è" da sola, CP850 = byte [0x8A]
assert_eq!(decode_oem(&[0x8A], 850), "è");

// "città" per intero, CP850 = byte [0x63, 0x69, 0x74, 0x74, 0x85]
assert_eq!(decode_oem(&[0x63, 0x69, 0x74, 0x74, 0x85], 850), "città");

// Codepage sconosciuta (numero inventato, non in nessuna tabella OEM reale):
// il CONTRATTO (punto 3 di "Cosa fare", non negoziabile) impone di ripiegare
// sullo STESSO comportamento di oggi (String::from_utf8_lossy), non deve mai
// panicare né inventare un codepage di default diverso. Verificato con
// Python (`bytes([0x53,0x8D]).decode('utf-8', errors='replace')`): il
// fallback per questi byte è "S\u{FFFD}" — la 'S' resta leggibile (è ASCII
// valido), lo 0x8D isolato (byte di continuazione UTF-8 senza un byte
// iniziale, invalido da solo) diventa il carattere di sostituzione.
assert_eq!(decode_oem(&[0x53, 0x8D], 999999), "S\u{FFFD}");
```

Aggiungi anche un test che l'ASCII puro passa invariato per qualunque codepage (i dati
IP/subnet/MAC che oggi già funzionano non devono rompersi):

```rust
assert_eq!(decode_oem(b"192.168.1.1", 850), "192.168.1.1");
```

Questi test vanno in `crates/mcp-nmap/src/network_info.rs`, dentro il modulo `#[cfg(test)] mod
tests` già esistente — NON serve `#[cfg(windows)]` su questi test specifici, dato che
`decode_oem` come progettato sopra non fa chiamate di sistema ed è quindi eseguibile anche in CI
non-Windows (se questo repository/ambiente ne avesse una — verifica comunque che compili anche
`#[cfg(not(windows))]`, dato che il resto del crate lo prevede).

## Verifica finale

(comandi da eseguire davvero, non a memoria — vedi `BRIEFING-AI-ESTERNE.md` sezione 5)

```powershell
cargo test -p mcp-nmap
cargo clippy -p mcp-nmap --all-targets
cargo build -p mcp-nmap
```

Se possibile, verifica anche dal vivo (l'unica verifica che copre il "punto critico" del
`GetConsoleOutputCP()` che ritorna 0 — nessun test unitario lo copre, solo l'uso reale sotto
l'orchestratore):

```powershell
.\stop_lare.ps1
.\build.ps1 -SkipShell
.\deploy_test_run.ps1 -SkipShell
.\Test Run\ui.exe          # attendi qualche secondo (self-heal dell'orchestratore)
```

Nella finestra "Lare Terminal", usa il canale `/nmap` e chiedi le informazioni di rete locali
(`local_network_info` è uno dei 7 tool esposti su quel canale — il testo esatto del comando dipende
dall'interfaccia `/nmap`, esplorala). Atteso: nell'output, etichette come "Server DNS",
"Indirizzo IPv4" leggibili correttamente (non `S�`/altri `�`). Se preferisci non interagire con
l'interfaccia utente, apri direttamente `Test Run\Configuration\logs\mcp-nmap.log` dopo l'uso (lo
stderr del processo finisce lì, fix di una sessione precedente di questo progetto) — non è
l'output del tool in sé, ma un'esecuzione reale del processo sotto l'orchestratore, che è
esattamente il contesto "senza console" che il punto critico sopra riguarda. Se non riesci a fare
questa verifica dal vivo (nessun accesso a un ambiente Windows reale, o altro limite), scrivilo
esplicitamente nel report finale — è l'unico modo per il supervisore di sapere che deve
verificarla lui prima di considerare il compito chiuso.

## File da aggiornare nello stesso commit (oltre al codice)

- `crates/mcp-nmap/CHANGELOG.md` — nuova voce, versione `2.0.1` (fix, patch bump — vedi la testa
  del file già esistente per lo stile esatto delle voci).
- `crates/mcp-nmap/Cargo.toml` — bump `version = "2.0.1"`, più la nuova dipendenza `oem_cp` con
  un commento.
- `crates/mcp-nmap/IMPLEMENTATION.md` — se esiste una sezione per `network_info.rs`, estendila; se
  non esiste, guarda lo stile delle altre sezioni del file e aggiungine una.
- `Docs/i18n/ita/KNOWN-ISSUES.md` — la sezione "Codepage — output dei comandi NATIVI" va
  aggiornata: la parte che riguarda `mcp-nmap`/`local_network_info`/`traceroute` è risolta da
  questo compito, ma la parte che riguarda `mcp-server/src/session.rs` resta aperta (fuori scope,
  vedi sopra) — non cancellare l'intera voce, restringila a quanto resta vero. Se la sezione ha
  un tag `[APERTO]` in testa, valuta se va cambiato (es. `[PARZIALE]`) dato che ora copre due
  problemi con stato diverso — a tua discrezione, spiegalo nel report se scegli di lasciarlo
  invariato.
- `Docs/i18n/ita/HANDOFF.md` — DUE punti, non uno: (1) la riga `mcp-nmap 2.0.0 (...)` nella
  sezione "Versioni correnti" in cima al file diventa `2.0.1`, con una parentesi che spiega il
  fix (stesso stile delle altre righe già lì, es. quella di `startup-config`/`orchestrator`); (2)
  una voce nella sezione "FATTO" che descrive il fix (guarda come sono scritte le voci più
  recenti per lo stile).

## Cosa NON fare (oltre alle regole generali di `BRIEFING-AI-ESTERNE.md`)

- Non toccare `crates/mcp-server/src/session.rs` (vedi sopra).
- Non introdurre `chcp` o modifiche al codepage ATTIVO della console — questo compito è solo
  decodifica, mai cambiare cosa la console usa.
- Non toccare `crates/mcp-nmap/src/scan.rs` o `elevate.rs` — fuori scope, anche se `elevate.rs` ti
  serve come riferimento di stile per le chiamate Win32 (leggilo, non modificarlo).
