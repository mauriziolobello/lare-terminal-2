# Changelog — `mcp-nmap`

All notable changes to this crate are documented here.
Format: [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).
Versioning: `major.minor.update` (SemVer).

## [2.0.1] — 2026-09-08 — fix: codepage OEM per i comandi nativi in `network_info.rs` (mojibake etichette accentate)

**Il bug:** l'output dei comandi diagnostici di rete nativi Win32 (`ipconfig`, `arp`, `route`, `netstat`, `tracert`) invocati da `local_network_info` e `traceroute` mostrava le etichette accentate italiane come caratteri corrotti/mojibake (es. `Sì` diventava `S`, U+FFFD). I dati numerici (IP, subnet, gateway, MAC) non ne risentivano perché ASCII puro.

**Causa radice:** `run_and_capture` decodificava i flussi stdout/stderr usando direttamente `String::from_utf8_lossy` sui byte grezzi. I processi nativi Win32 su console italiana emettono invece byte codificati nel codepage OEM (tipicamente CP850 in Europa occidentale/Italia, CP437 su sistemi in lingua inglese US). Qualsiasi byte accentato OEM non conforme alla sequenza UTF-8 veniva convertito in U+FFFD.

**Il fix:**
- Sostituita la decodifica ingenua UTF-8 con la decodifica basata sul codepage OEM effettivamente attivo.
- Aggiunta la funzione pura e multipiattaforma `decode_oem(bytes: &[u8], codepage: u32) -> String` che consulta la tabella `DECODING_TABLE_CP_MAP` del crate `oem_cp` (convertendo in modo verificato `u16::try_from(codepage)`). Se il codepage non è presente o sconosciuto, ripiega sul comportamento invariato di `String::from_utf8_lossy`.
- Aggiunta la funzione `active_console_output_codepage() -> u32` (`#[cfg(windows)]`): interroga `GetConsoleOutputCP()` dal crate `windows` (feature `Win32_System_Console`).
- **Gestione del processo senza console (`CREATE_NO_WINDOW`)**: se `mcp-nmap.exe` gira senza console (lanciato dall'orchestratore), `GetConsoleOutputCP()` restituisce `0`. In tal caso, la funzione ripiega su `GetOEMCP()` (feature `Win32_Globalization`), ottenendo il codepage OEM di default a livello di sistema operativo Windows.
- `run_and_capture` usa `active_console_output_codepage()` e `decode_oem` su Windows, mantenendo il fallback UTF-8 lossy su piattaforme non-Windows.

### Fixed

- `crates/mcp-nmap/src/network_info.rs`: decodifica stdout e stderr dei comandi diagnostici nativi usando il codepage OEM attivo della console o il codepage OEM di sistema (risolvendo il mojibake `Sì` → `S`).

### Dependencies

- Aggiunta dipendenza `oem_cp = "2"` in `Cargo.toml`.
- Aggiunte features `"Win32_System_Console"` e `"Win32_Globalization"` a `windows = "0.62"` sotto `[target.'cfg(windows)'.dependencies]`.

### TDD (RED → GREEN)

1. **`decode_oem_decodifica_correttamente_accentate_cp850`**, **`decode_oem_codepage_sconosciuta_ripiega_su_utf8_lossy`**, **`decode_oem_preserva_invariati_i_dati_ascii_puri`**: scritti prima del fix con implementazione stub di `decode_oem` ripiegata su `String::from_utf8_lossy`. Confermato RED con `cargo test -p mcp-nmap decode_oem`: l'assert su `[0x53, 0x8D]` (CP850 per "Sì") falliva con `left: "S\u{FFFD}", right: "Sì"`. Con l'integrazione di `oem_cp`, GREEN (64 test passati).

### Verification

- `cargo test -p mcp-nmap` → 64 passati, 0 falliti.
- `cargo clippy -p mcp-nmap --all-targets` → nessun warning.
- `cargo build -p mcp-nmap` → compilazione pulita.
- `cargo test` (intero workspace) → tutti i test passati puliti.
- **Verifica dal vivo con `CREATE_NO_WINDOW`**: testata l'invocazione di `mcp-nmap.exe` via `ProcessStartInfo` con `CreateNoWindow = $true` e piping JSON-RPC `local_network_info`. L'output ha confermato la corretta lettura di `"DHCP abilitato : Sì"` tramite il fallback `GetOEMCP()`.

## 2.0.0 — 2026-09-05 — fork da v1 0.8.2

Copia del crate dalla v1 (`mauriziolobello/lare-terminal`) nel repo 2.0. Nessuna modifica
funzionale in questa voce; le modifiche del piano 1 seguono nelle voci successive.

## [0.8.2] — 2026-07-18 — fix: `nmap_vuln_scan` hung indefinitely, zero report produced

**Il bug:** l'utente ha girato `nmap_vuln_scan` contro il proprio router
(`192.168.178.1`). La chiamata ha colpito il timeout esterno da 900s
dell'orchestrator (`NMAP_CALL_TIMEOUT_SECS`, `nmap_tool_client.rs`) **senza
aprire nessuna finestra di report** — perdita totale di tutti i dati della
scansione (tutte le porte, tutti i risultati script che nmap aveva già
completato).

**Causa radice (confermata da riproduzione live, non ipotizzata):**
`run_vuln_scan` (`src/scan.rs`) invoca `nmap --script vuln -oX <path>
<target>` senza alcun limite di tempo. Una diagnosi dal vivo contro lo
stesso identico router ha riprodotto l'hang: la stima di completamento NSE
di nmap ha raggiunto il 99.76% in 45 secondi e poi è rimasta **piatta lì per
oltre 5 minuti, zero progressi ulteriori** — uno specifico script NSE della
categoria "vuln" resta bloccato indefinitamente contro questo router, molto
probabilmente uno che sonda la porta SIP (5060) o UPnP/wsdapi (5357),
entrambe note per scartare silenziosamente probe inattese sui router consumer
anziché rispondere con reset/errore, il che fa attendere per sempre uno
script NSE privo di un proprio timeout limitato. Niente nell'invocazione
nmap di `run_vuln_scan` limitava il tempo di esecuzione di un singolo
script, quindi l'UNICA cosa che fermava mai una scansione bloccata era il
timeout esterno dell'orchestrator da 900s — che uccide **l'attesa
dell'orchestrator**, non il processo nmap sottostante, e scarta il 100% dei
dati della scansione (tutte le 9 porte e ogni altro risultato script GIÀ
completato) perché nmap non arriva mai a scrivere il proprio file `-oX`.

**Il fix, verificato contro l'hang reale:** aggiungere il flag nativo di
nmap `--script-timeout <tempo>` limita il tempo di esecuzione di ogni
singolo script NSE — quando uno script supera quel limite, nmap abbandona
solo quello script e prosegue, scrivendo comunque un file `-oX` completo con
tutto il resto. Ri-test dal vivo contro lo stesso identico router con
`--script-timeout 60s` aggiunto: la scansione si è completata pulitamente in
**71.44 secondi** (contro l'hang per 900s+ e zero output) — report completo,
tutte le 9 porte, tutti gli altri risultati script intatti. Flag nmap reale
confermato (non un nome inventato): verificato sia leggendo `nmap --help`
(elenca `--host-timeout` nella stessa sezione) sia eseguendo `nmap
--script-timeout 5s -sn 127.0.0.1`, che gira senza alcun errore "unrecognized
option".

**`src/scan.rs`:** `run_vuln_scan`'s `extra_args` diventa `&["--script",
"vuln", "--script-timeout", "60s"]` (prima: solo `&["--script", "vuln"]`).
60s scelto generosamente al di sopra di quanto impiegasse qualunque script
legittimo nella scansione riprodotta (il più lento tra quelli completati ha
impiegato ben meno di 10s) — non tarato sul fallimento, solo un limite
superiore ragionevole per singolo script. Nessun'altra funzione di scan
tocca questo percorso: `run_quick_scan`/`run_version_scan`/
`run_host_discovery` non usano NSE script affatto, e `run_os_detect` (`-O`,
elevato) è un percorso separato che non passa da `run_scan_with_flags`.

### Fixed

- `src/scan.rs`: `run_vuln_scan` ora passa anche `--script-timeout 60s` a
  nmap — prima nessun limite bloccava l'esecuzione di un singolo script NSE,
  lasciando che l'unico argine fosse il timeout esterno da 900s
  dell'orchestrator, che scarta tutti i dati della scansione invece di
  produrre un report parziale.

### TDD (RED → GREEN)

1. **`vuln_scan_always_bounds_script_execution_time`** — nuovo test in
   `scan.rs`'s `#[cfg(test)] mod tests`, subito dopo
   `vuln_scan_always_uses_the_fixed_vuln_category_never_a_custom_script`
   (stesso stile, stessa `CapturingProcess` fake locale — ridefinita
   localmente perché due funzioni `#[test]` non possono condividere una
   struct definita nel corpo di un'altra, stesso motivo per cui questo file
   ha già due `PanicsIfCalled` separate). Scritto per primo, riferendo un
   comportamento (`--script-timeout 60s` negli argv) non ancora
   implementato. `cargo test -p mcp-nmap
   vuln_scan_always_bounds_script_execution_time` confermato RED per il
   motivo giusto: l'assert fallisce (non un errore di compilazione) con
   `expected --script-timeout 60s in args, got: ["--script", "vuln", "-oX",
   ..., "192.168.1.10"]` — il flag manca davvero, non un typo nel test. Poi
   aggiunto `"--script-timeout", "60s"` a `extra_args` in `run_vuln_scan`;
   GREEN.

### Verification

- `cargo test -p mcp-nmap` → `test result: ok. 61 passed; 0 failed` (60
  preesistenti + 1 nuovo), nessun warning.
- `cargo build` (intero workspace, default-members) → pulito — conferma che
  `orchestrator` non richiede alcuna modifica (`ScanOutcome{summary,
  report_markdown, is_error}` resta invariato in forma JSON; questo fix
  cambia solo gli argv passati a nmap, non lo shape dell'output).
- `cargo clippy -p mcp-nmap --all-targets` → nessun warning.
- `cargo fmt -p mcp-nmap --check` → `scan.rs` (l'unico file di codice
  toccato da questo fix) pulito dopo un riformattazione a mano della riga
  `assert!` del nuovo test per matchare l'output atteso di rustfmt. Il
  drift residuo in `markdown.rs` (5 diff, tutte su righe preesistenti mai
  toccate da questo fix) resta **preesistente**, già documentato come fuori
  scope dalle voci 0.3.0/0.4.0/0.5.0/0.6.0/0.7.0/0.8.0/0.8.1 di questo
  changelog.

## [0.8.1] — 2026-07-18 — fix: `nmap_vuln_scan` report window byte-identical to `nmap_quick_scan`'s

**Il bug:** l'utente ha girato `nmap_quick_scan` poi `nmap_vuln_scan` contro
lo stesso target (`192.168.178.1`) dal canale `/nmap`. Entrambe le chiamate
hanno prodotto banner di conferma e riassunti chat corretti e DISTINTI, ma
le due finestre Markdown aperte erano identiche byte per byte — il report
del vuln scan non conteneva alcun contenuto specifico di vulnerabilità, era
una copia esatta del report del quick scan.

**Causa radice (confermata leggendo il sorgente + una cattura reale di `nmap
--script vuln` contro un router vero):** `parse_nmap_xml`
(`src/report.rs`) estrae stato host, indirizzo, porte
(portid/protocol/state/service) e OS match dall'XML `-oX` di nmap, ma non
aveva **alcun percorso di codice** che leggesse gli elementi `<script>`/
`<hostscript>` — esattamente la forma XML che nmap usa per riportare i
risultati degli script NSE (incluso tutto ciò che trova `--script vuln`).
Dato che `ScanReport` non catturava mai questi dati, sia
`format_report_markdown` sia `format_summary` (`src/markdown.rs`) erano
strutturalmente incapaci di mostrarli — un vuln scan contro un target le cui
porte aperte non cambiano tra due run produce necessariamente un report
identico a un semplice port scan, anche se nmap ha davvero girato flag
diversi e scritto XML diverso.

**Perché la fix tocca `format_summary`, non solo la finestra Markdown:**
`format_summary` è l'UNICO canale che il modello AI vede davvero nel
tool_result — il report originale dell'utente includeva l'AI che
*narrava* "trovate potenziali vulnerabilità" basandosi su nient'altro che
un conteggio di porte, perché `format_summary` non le diceva mai se uno
script fosse effettivamente girato. Correggere solo
`format_report_markdown` avrebbe lasciato in vita metà del bug.

**Il fix:**

- **`src/report.rs`** — nuovo tipo `ScriptResult { id: String, output:
  String }` (solo l'attributo `output` è catturato — nmap's own flattened
  text, già decodificato dalle entity da roxmltree; i figli strutturati
  `<table>`/`<elem>` che alcuni script NSE emettono in più non sono
  parsati, deliberatamente fuori scope per una prima fix corretta). Nuovo
  campo `scripts: Vec<ScriptResult>` su `PortReport` (dopo `service`), nuovo
  campo `host_scripts: Vec<ScriptResult>` su `HostReport` (dopo
  `os_matches`). Nuovo helper privato `parse_scripts(parent: roxmltree::Node)
  -> Vec<ScriptResult>`, condiviso tra il loop delle porte (`<port><script>`)
  e il lookup a livello host (`<hostscript><script>`) — stessa forma di
  elemento, genitore diverso.
- **`src/markdown.rs`** — `format_report_markdown` guadagna due nuovi blocchi
  per host (dopo la lista porte, prima della sezione OS detection):
  "**Risultati script per porta:**" (solo per le porte con `scripts` non
  vuoto) e "**Risultati script a livello host:**" (solo se `host_scripts`
  non è vuoto). `format_summary` conta i risultati script su tutti gli host
  attivi (`host_scripts.len() + Σ port.scripts.len()`) e, solo se il
  conteggio è > 0, aggiunge la clausola ", N risultato/i script NSE" alla
  frase esistente — **deliberatamente un conteggio, non un verdetto**: nmap
  formatta l'`output` dei propri script NSE in modo troppo incoerente perché
  classificarlo "vulnerabile"/"non vulnerabile" sia affidabile. Un plain
  quick_scan/version_scan/host_discovery (nessuno script girato) produce la
  frase esistente byte per byte invariata — verificato da un test di
  regressione dedicato.

**Nuova fixture `src/fixtures/host_with_vuln_scripts.xml`** — non inventata,
ricalca esattamente la forma di una cattura reale di `nmap -sT --script vuln`
contro un router vero (vedi la lezione già imparata da questo crate: la
fixture `real_nmap_output_with_dtd.xml` in 0.6.0 esiste proprio perché una
fixture scritta a mano aveva già causato un bug non rilevato in produzione —
vedi "Provenienza della fixture" in `IMPLEMENTATION.md`). Include
deliberatamente un `<script>` a livello host con SIA un attributo
`output="false"` SIA un figlio di testo `false` — il parser deve leggere
l'attributo `output`, non il figlio di testo, e il secondo script della
fixture (`samba-vuln-cve-2012-1182`, attributo `output` una frase lunga, ma
figlio di testo comunque `"false"`) è ciò che rende quell'assunzione
verificabile davvero, non solo plausibile.

### Fixed

- `src/report.rs`: `parse_nmap_xml` ora legge `<script>`/`<hostscript>` e
  popola `PortReport::scripts`/`HostReport::host_scripts` — prima non li
  leggeva affatto.
- `src/markdown.rs`: `format_report_markdown` mostra i risultati script per
  porta e a livello host quando presenti; `format_summary` include un
  conteggio dei risultati script NSE quando > 0.

### Every existing `PortReport`/`HostReport` struct literal fixed

I due nuovi campi rompono la compilazione di ogni literal preesistente in
questo crate — atteso, non una regressione (il brief lo segnala
esplicitamente). Corretti: i due literal di confronto in
`report.rs::tests::parses_host_up_with_open_and_closed_ports`, l'helper
`sample_report` e il literal inline di
`summary_zero_hosts_says_none_active_not_an_error` in `markdown.rs`.
`scan.rs`'s test module non costruisce nessuno dei due a mano (usa solo
fixture XML attraverso `parse_nmap_xml`), quindi non ha richiesto modifiche
— confermato da `cargo build -p mcp-nmap --tests` pulito dopo il fix di
`report.rs`/`markdown.rs`.

### TDD (RED → GREEN)

1. **`report.rs`'s 2 nuovi test** (`parses_per_port_script_results`,
   `parses_host_level_script_results_using_output_attribute_not_text_child`)
   + un'asserzione aggiunta al test esistente
   `parses_host_up_with_open_and_closed_ports` — scritti per primi,
   riferendo `scripts`/`host_scripts` non ancora esistenti. `cargo test -p
   mcp-nmap` confermato RED: errori di compilazione `E0609` ("no field
   `scripts`"/"no field `host_scripts`"), non un typo, non un errore non
   correlato. Poi aggiunta l'implementazione completa (`ScriptResult`,
   `parse_scripts`, i due nuovi campi, il wiring nel loop porte e nel loop
   host); GREEN.
2. **`markdown.rs`'s 5 nuovi test**
   (`markdown_report_includes_per_port_script_output`,
   `markdown_report_includes_host_level_script_output`,
   `markdown_report_without_scripts_has_no_script_sections`,
   `summary_without_scripts_matches_existing_wording_exactly`,
   `summary_with_scripts_reports_script_count_not_a_vulnerable_verdict`) —
   scritti dopo il passaggio 1 (i due literal esistenti in `sample_report`/
   `summary_zero_hosts_says_none_active_not_an_error` erano già stati
   corretti per far ricompilare il modulo). `cargo test -p mcp-nmap
   markdown::` confermato RED per il motivo giusto: 3 dei 5 nuovi test
   fallivano un `assert!`/`assert_eq!` (non un errore di compilazione) —
   `format_report_markdown` non menzionava ancora "Risultati script...",
   `format_summary` non menzionava ancora "risultato/i script NSE"; gli
   altri 2 (nessuna sezione script quando non ce ne sono; wording invariato
   quando non ci sono script) erano già veri prima del fix, per costruzione,
   e sono passati fin da subito come guardia di regressione. Poi aggiunta
   l'implementazione (i due blocchi in `format_report_markdown`, il
   conteggio in `format_summary`); GREEN.

### Verification

- `cargo test -p mcp-nmap` → `test result: ok. 60 passed; 0 failed` (53
  preesistenti + 7 nuovi: 2 in `report.rs` + 5 in `markdown.rs`), nessun
  warning.
- `cargo build -p mcp-nmap` → pulito.
- `cargo build` (intero workspace, default-members) → pulito, conferma che
  `orchestrator` non richiede alcuna modifica (`ScanOutcome{summary,
  report_markdown, is_error}` resta invariato: entrambi i campi restano
  `String` semplici, il deserializzatore `NmapScanOutcomeJson`
  dell'orchestrator non cambia forma).
- `cargo clippy -p mcp-nmap --all-targets` → nessun warning.
- `cargo fmt -p mcp-nmap --check` → `report.rs` (interamente di proprietà di
  questo task) pulito. In `markdown.rs`, ogni riga toccata da questo task
  (il nuovo blocco in `format_report_markdown`, il corpo riscritto di
  `format_summary`, i literal di `sample_report`/
  `summary_zero_hosts_says_none_active_not_an_error` corretti per i nuovi
  campi, i 5 nuovi test) è stata riformattata a mano per matchare l'output
  atteso di rustfmt — verificato riga per riga contro `git diff` che nessuna
  delle righe rimaste nel diff di `cargo fmt --check` fosse tra quelle
  toccate da questo task. Il drift residuo (7 diff, tutte su righe
  preesistenti mai toccate da questo task: `**Stato:**`, l'assert di
  `markdown_report_includes_target_host_and_ports`,
  `markdown_report_empty_hosts_says_so`, `invocation_label_quick_scan`,
  `invocation_label_os_detect_mentions_elevation`) resta **preesistente**,
  già documentato come fuori scope dalle voci 0.3.0/0.4.0/0.5.0/0.6.0/0.7.0/
  0.8.0 di questo changelog.

### Deviazione dal brief (minore, non-bloccante)

Il brief non menzionava esplicitamente la stringa di log
`tracing::info!("Lare Terminal mcp-nmap v0.8.0 starting...")` in
`main.rs` — aggiornata comunque a `v0.8.1` in questo stesso task, per
coerenza con il precedente già stabilito (questa stringa segue sempre la
versione del crate, dal suo primo commit in Task 5, come già notato nelle
voci 0.7.0/0.8.0 di questo changelog).

## [0.8.0] — 2026-07-18 — `nmap_version_scan` + `nmap_host_discovery` + `nmap_vuln_scan`

Durante il primo test dal vivo del canale `/nmap`, l'utente ha chiesto
all'AI cos'altro sapesse fare oltre a elencare i dispositivi; l'AI ha
risposto con un menu di 7 opzioni (varianti di port scan, OS detect,
scripting NSE, traceroute, rilevamento versioni, host discovery, formati
di output). L'utente ne ha scelte esattamente 3: rilevamento versioni
servizi (`-sV`), host discovery (`-sn`) e una scansione vulnerabilità
tramite la categoria NSE curata `vuln` di nmap stesso — deliberatamente
non l'intero menu (niente `--script` arbitrario, niente passthrough di
flag grezzi).

`src/scan.rs`: `run_quick_scan` aveva finora una pipeline privata e
completa (valida target → controlla PATH → tempfile per `-oX` → spawn via
`NmapProcess` → parse XML → costruisci `ScanOutcome`). Questa pipeline è
ora estratta in un helper privato `run_scan_with_flags(target, process,
nmap_available, extra_args)`, dove `extra_args` è l'unica cosa che
differisce tra i chiamanti (`&["-sT"]` per il quick scan, `&["-sV"]` per
il version scan, `&["-sn"]` per l'host discovery, `&["--script", "vuln"]`
per il vuln scan). `run_quick_scan` diventa un one-liner che richiama
l'helper — firma pubblica e comportamento **invariati** (16 test
preesistenti confermati verdi subito dopo il refactor, prima di
aggiungere qualunque test nuovo). I 3 nuovi wrapper pubblici
(`run_version_scan`, `run_host_discovery`, `run_vuln_scan`) passano
attraverso lo stesso trait `NmapProcess` non elevato di `run_quick_scan`
— `run_os_detect` (che usa `elevate::run_elevated` direttamente) non è
toccato da questo task.

**`nmap_vuln_scan`'s script category è hardcoded, non configurabile per
design**: `run_vuln_scan` passa sempre e solo `&["--script", "vuln"]`
all'helper — non esiste alcun parametro né percorso di codice che possa
inoltrare un nome di script diverso scelto dal modello o dal chiamante.
Stessa logica "tool fissi, mai una shell/script arbitrario" già applicata
a `validate_target` per i target di scan; un nuovo test
(`vuln_scan_always_uses_the_fixed_vuln_category_never_a_custom_script`)
cattura gli argv effettivamente passati a `NmapProcess::run` e verifica
che siano sempre esattamente `["--script", "vuln"]`.

`src/markdown.rs`: `format_nmap_invocation` guadagna 3 nuovi arm per le
etichette del banner di conferma (`nmap_version_scan`,
`nmap_host_discovery`, `nmap_vuln_scan`) — quest'ultima nomina
esplicitamente `--script vuln` per non lasciare intendere all'utente che
possa girare uno script NSE arbitrario.

`src/main.rs`: 3 nuovi metodi `#[tool]` su `NmapServer`
(`nmap_version_scan`, `nmap_host_discovery`, `nmap_vuln_scan`) + 3 nuovi
DTO parametri (`NmapVersionScanParams`, `NmapHostDiscoveryParams`,
`NmapVulnScanParams`, tutti con un solo campo `target: String`, stessa
forma dei DTO di scan esistenti). Tutti e 3 ritornano
`{ summary, report_markdown, is_error }` — la stessa forma JSON di
`nmap_quick_scan`/`nmap_os_detect`, nessuna nuova struct.

### Added

- **`src/scan.rs`**:
  - `run_scan_with_flags(target, process, nmap_available, extra_args) ->
    ScanOutcome` (privata) — pipeline condivisa estratta da
    `run_quick_scan`.
  - `run_version_scan(target, process, nmap_available) -> ScanOutcome`
    (`-sV`).
  - `run_host_discovery(target, process, nmap_available) -> ScanOutcome`
    (`-sn`).
  - `run_vuln_scan(target, process, nmap_available) -> ScanOutcome`
    (`--script vuln`, categoria hardcoded).
  - 7 nuovi test: 2 per funzione (successo con flag corretto + rifiuto
    pre-processo di un target non valido/PATH assente) più il test di
    sicurezza sul vuln scan descritto sopra.
- **`src/markdown.rs`** — 3 nuovi arm in `format_nmap_invocation` + 3
  nuovi test (`invocation_label_version_scan_shows_target`,
  `invocation_label_host_discovery_shows_target`,
  `invocation_label_vuln_scan_names_the_script_category`).
- **`src/main.rs`** — 3 nuovi DTO parametri + 3 nuovi tool MCP, registrati
  dopo `nmap_os_detect` e prima di `local_network_info`. Stringa di log di
  avvio aggiornata a `v0.8.0` per coerenza con il precedente già stabilito
  in questo changelog (questa stringa segue sempre la versione del
  crate).

### TDD (RED → GREEN)

1. **Refactor `run_quick_scan` → `run_scan_with_flags`** — trattato come
   refactor di codice già testato: nessun nuovo test scritto per questo
   passaggio, la rete di sicurezza è che i 16 test preesistenti di
   `scan::tests` restano verdi **immutati** subito dopo il refactor
   (confermato con `cargo test -p mcp-nmap scan::tests` prima di
   aggiungere qualunque codice/test nuovo) — prova che il refactor
   preserva il comportamento di `run_quick_scan` byte per byte. I 3 nuovi
   wrapper pubblici sono aggiunti nello stesso passaggio del refactor
   (sono one-liner banali attorno all'helper condiviso), poi verificati
   dai 7 test del passaggio successivo.
2. **7 test nuovi per le 3 funzioni nuove** — scritti e poi eseguiti
   insieme (le funzioni erano già presenti dal passaggio 1, essendo
   wrapper triviali sull'helper già testato dal passaggio 1); tutti verdi
   al primo run (`cargo test -p mcp-nmap scan::tests`: 23 passed, 16
   preesistenti + 7 nuovi).
3. **3 test di `format_nmap_invocation`** — scritti per i 3 nuovi nomi di
   tool non ancora riconosciuti dalla `match`; verdi dopo l'aggiunta dei
   3 nuovi arm (`cargo test -p mcp-nmap`: 53 passed, 43 preesistenti + 7
   `scan.rs` + 3 `markdown.rs`).

### Verification

- `cargo test -p mcp-nmap scan::tests` (dopo il solo refactor, prima di
  qualunque test nuovo) → `test result: ok. 16 passed; 0 failed` — stesso
  conteggio di prima del task, nessuna regressione.
- `cargo build -p mcp-nmap` → pulito.
- `cargo test -p mcp-nmap` → `test result: ok. 53 passed; 0 failed`, nessun
  warning (43 preesistenti + 10 nuovi: 7 in `scan.rs` + 3 in
  `markdown.rs`).
- `cargo build` (intero workspace, default-members) → pulito.
- `cargo clippy -p mcp-nmap --all-targets` → nessun warning.
- `cargo fmt -p mcp-nmap --check` → `scan.rs` (interamente di proprietà di
  questo task) pulito dopo un run diretto di `rustfmt` sul file. Le 3
  nuove funzioni di test in `markdown.rs` (le uniche righe nuove aggiunte
  a questo file da questo task) riformattate a mano per matchare l'output
  atteso di rustfmt. Il drift residuo in `markdown.rs` (7 diff, tutte su
  righe preesistenti prima di questo task — `format_report_markdown`,
  `sample_report`, e i test già esistenti) resta **preesistente**, già
  documentato come fuori scope dalle voci 0.3.0/0.4.0/0.5.0/0.6.0/0.7.0 di
  questo changelog.
- **Smoke test manuale**: `initialize` + `tools/list` JSON-RPC pipati nel
  binario compilato (`target/debug/mcp-nmap.exe`) → tutti e 7 i tool
  presenti (`nmap_quick_scan`, `nmap_os_detect`, `nmap_version_scan`,
  `nmap_host_discovery`, `nmap_vuln_scan`, `local_network_info`,
  `traceroute`), schema di ciascun nuovo tool di scan con `target:
  string, required`, coerente con gli schemi già esistenti.

## [0.7.1] — 2026-07-18 — doc fixes (review follow-up)

Review su Task 1 (0.7.0) ha trovato 2 gap: doc-comment di `lib.rs` non
aggiornato (diceva ancora "two tools"/"Five-module") e mancava una nota sul
mojibake OEM noto (`Docs/KNOWN-ISSUES.md`) riprodotto da `run_and_capture`.
Nessun fix di codice: il mojibake resta aperto (colpisce solo le etichette
italiane, non i dati ASCII — IP/subnet/gateway/MAC — che servono all'AI),
documentato in un commento sul modulo e in una nota incrociata in
KNOWN-ISSUES.md così la issue app-wide sa che questo modulo la riproduce.

### Changed

- `src/lib.rs`: doc-comment "four tools" / "Six-module architecture",
  bullet list riordinata alfabeticamente per matchare i `pub mod`.
- `src/network_info.rs`: commento su `run_and_capture` che spiega il
  mojibake noto e perché non è corretto qui.

### Docs

- `Docs/KNOWN-ISSUES.md`: nota incrociata sotto l'entry `[APERTO]`
  codepage — anche `network_info.rs` la riproduce.

## [0.7.0] — 2026-07-18 — `local_network_info` + `traceroute`

Trovato nel primo smoke test dal vivo: l'AI del canale nmap non aveva modo
di scoprire il proprio IP/subnet per proporre un target di scan quando
l'utente non lo specificava esplicitamente — dipendeva interamente
dall'utente per fornire un range CIDR. Due nuovi tool fissi (mai una shell
arbitraria, stesso principio dei due tool di scan): `local_network_info`
(nessun parametro — `ipconfig /all` + `arp -a` + `route print` + `netstat -rn`,
concatenati in un report Markdown) e `traceroute` (un parametro `target`,
valida con `scan::validate_target` — ora `pub(crate)` — prima di invocare
`tracert`). A differenza di `ScanOutcome`, `NetworkInfoOutcome` ha solo
`{ output, is_error }`: nessun `report_markdown`/finestra/Library — l'AI
DEVE leggere questi dati per decidere il target successivo, quindi vanno
per intero nel tool_result, non nascosti dietro un summary. Windows-only
in v1 (stub leggibile su altre piattaforme, stesso trattamento di
`elevate.rs`). `format_nmap_invocation` aggiorna le etichette di
trasparenza per i 2 nuovi tool.

### Added

- **`src/network_info.rs` (new module):**
  - `NetworkInfoOutcome { output, is_error }` — `#[derive(Debug, Clone,
    PartialEq, Serialize, Deserialize)]`, forma JSON deliberatamente diversa
    da `ScanOutcome` (niente `summary`/`report_markdown`).
  - `run_and_capture(exe, args) -> Result<String, String>` — helper privato,
    unico punto che tocca `std::process::Command` in questo modulo. Non usa
    il trait `NmapProcess` (quel seam modella "processo che scrive -oX e
    ritorna un exit status"; questi comandi ritornano output diretto —
    aggiungere un'astrazione qui per un solo chiamante sarebbe YAGNI).
  - `#[cfg(windows)] mod windows_impl` — `local_network_info()` esegue in
    sequenza `ipconfig /all`, `arp -a`, `route print`, `netstat -rn`,
    concatenando l'output in sezioni Markdown; `is_error` è `true` solo se
    **tutti e 4** falliscono (un fallimento parziale lascia comunque dati
    utili, non è un errore di tool). `traceroute(target)` valida il target
    con `scan::validate_target` poi invoca `tracert -d -w 1000 <target>`
    (`-d`: niente risoluzione hostname; `-w 1000`: 1s di timeout per hop
    invece del default di alcuni secondi, per non far girare il comando per
    minuti su hop filtrati).
  - `#[cfg(not(windows))]` — stub leggibile per entrambe le funzioni
    ("...non ancora supportate su questa piattaforma"), stesso trattamento
    di `elevate.rs` per il requisito di gemellaggio macOS/Linux.
- **`src/scan.rs`** — `validate_target` da privata a `pub(crate)` (nessun
  altro cambio: stesso corpo, stessi test), unico consumatore esterno al
  modulo è `network_info::traceroute`.
- **`src/markdown.rs`** — `format_nmap_invocation` guadagna 2 nuovi arm:
  `"local_network_info"` → `"informazioni di rete locali"` (nessun target
  da mostrare), `"traceroute"` → `"traceroute → {target}"`.
- **`src/main.rs`** — `TracerouteParams { target }` (nuovo DTO parametri),
  2 nuovi metodi `#[tool]` su `NmapServer`: `local_network_info` (nessun
  parametro) e `traceroute` (un parametro `target`). Entrambi
  serializzano l'esito in una stringa JSON bare, stessa convenzione dei
  due tool di scan esistenti (successo/errore codificati come dati via
  `is_error`, mai come errore a livello di protocollo MCP).
- **`src/lib.rs`** — aggiunto `pub mod network_info;` (posizione
  alfabetica: dopo `markdown`, prima di `report`).

### TDD (RED → GREEN)

1. **`network_info.rs`'s 3 test unitari eseguibili su Windows** (più un 4°
   `#[cfg(not(windows))]`-gated) — scritti per primi come modulo
   `#[cfg(test)] mod tests` isolato, riferendo `NetworkInfoOutcome`/
   `traceroute`/`local_network_info` non ancora esistenti. Con il modulo
   già agganciato in `lib.rs` (necessario perché il file venisse anche
   solo considerato dal compilatore), `cargo test -p mcp-nmap` confermato
   RED: 4 errori di compilazione (`E0422`/`E0425`), tutti che nominano i
   simboli mancanti — non un typo, non un errore non correlato. Poi
   aggiunta l'implementazione completa (struct, `run_and_capture`,
   `windows_impl`, stub non-Windows) più il bump di visibilità di
   `validate_target`; GREEN: 41 passed (38 preesistenti + 3 nuovi — il 4°
   test è gated `#[cfg(not(windows))]` e su questa macchina Windows non
   viene compilato/contato, come previsto dal brief).
2. **`markdown.rs`'s 2 nuovi test di invocation label** — scritti per
   primi. `cargo test -p mcp-nmap invocation_label` confermato RED per il
   motivo giusto: entrambi i test **falliscono un assert** (non un errore
   di compilazione) perché `format_nmap_invocation` cadeva ancora
   nell'arm di fallback `other => "[tool {other}]"` per questi due nomi
   di tool non ancora riconosciuti. Poi aggiunti i 2 nuovi arm; GREEN:
   43 passed (41 + 2 nuovi).

### Verification

- `cargo test -p mcp-nmap` → `test result: ok. 43 passed; 0 failed`, nessun
  warning.
- `cargo build -p mcp-nmap` → pulito.
- `cargo build` (intero workspace, default-members) → pulito.
- `cargo clippy -p mcp-nmap --all-targets` → nessun warning.
- `cargo fmt -p mcp-nmap --check` → `network_info.rs` (il file
  interamente di proprietà di questo task) è pulito dopo un run diretto
  di `rustfmt` (stesso precedente di `scan.rs` in Task 3 — le due
  aggiunte di test in `markdown.rs` erano già fmt-clean). Il drift
  residuo in `markdown.rs` (7 diff, tutte su righe preesistenti prima di
  questo task) è **preesistente**, già documentato come fuori scope dalle
  voci 0.3.0/0.4.0/0.5.0/0.6.0 di questo changelog.
- **Smoke test manuale**: `initialize` + `tools/list` JSON-RPC pipati nel
  binario compilato (`target/debug/mcp-nmap.exe`) → tutti e 4 i tool
  presenti (`nmap_quick_scan`, `nmap_os_detect`, `local_network_info`,
  `traceroute`), schema di `local_network_info` correttamente vuoto
  (nessun parametro), schema di `traceroute` con `target: string,
  required`.

### Deviazione dal brief (minore, non-bloccante)

Il brief non menzionava esplicitamente la stringa di log
`tracing::info!("Lare Terminal mcp-nmap v0.6.0 starting...")` in
`main.rs` — aggiornata comunque a `v0.7.0` in questo stesso task, per
coerenza con il precedente già stabilito (questa stringa ha sempre
seguito la versione del crate dal suo primo commit in Task 5).

## [0.6.0] — 2026-07-16

### Added — `main.rs` MCP glue: serve `nmap_quick_scan`/`nmap_os_detect` over stdio (Task 5)

**Scope:** replaces the Task 1 placeholder binary with the real MCP stdio
server, mirroring `crates/mcp-server/src/main.rs`'s pattern exactly. This is
the crate's actual deliverable — the first point in this plan where
`crates/mcp-nmap` becomes a working, servable sidecar rather than a library
with a stub binary.

- **`src/main.rs` (rewritten):**
  - `RealNmapProcess` — the first (and only) real implementation of
    `scan::NmapProcess`, spawning `nmap.exe`/`nmap` via
    `std::process::Command`. Never used in any test (tests use fakes in
    `scan.rs`).
  - `NmapQuickScanParams { target }` / `NmapOsDetectParams { target }` —
    `#[derive(Deserialize, schemars::JsonSchema)]` parameter DTOs, same shape
    convention as `mcp-server`'s parameter structs.
  - `NmapServer` + `#[tool_router(server_handler)]` exposing
    `nmap_quick_scan`/`nmap_os_detect` — both call straight into
    `scan::run_quick_scan`/`scan::run_os_detect`, passing `scan::nmap_on_path()`
    as the `nmap_available` argument (the "`main.rs` is `nmap_on_path`'s only
    real caller" design already established since Task 3/4). Both return a
    bare JSON string (`ScanOutcome` serialized) — success/error both encoded
    as data (`is_error: bool`), never as an MCP-protocol-level error, same
    convention as `mcp-server`'s tools.
  - `main()` — tracing to stderr only (stdout is the MCP JSON-RPC wire),
    `NmapServer.serve(stdio())`, `service.waiting()`.
- No new unit tests for `main.rs` itself, by design (per the task brief's own
  reasoning, matching `mcp-server/src/main.rs`'s precedent): this file's glue
  is exercised by the companion integration plan's `NmapToolClient` tests
  against the real spawned binary, not by unit tests in this crate.

#### Signature note (brief vs. actual code)

The task brief was written before Task 4's `validate_target` security fix
added a third parameter (`nmap_available: bool`) to both
`run_quick_scan`/`run_os_detect`. By the time this task was executed, the
brief's own literal `main.rs` code sample had already been updated to call
both functions with the correct 3-argument signature (`&target,
&RealNmapProcess, scan::nmap_on_path()` / `&target, scan::nmap_on_path()`) —
verified directly against `scan.rs`'s actual current signatures before
writing anything, per this task's explicit instruction to not trust the
brief blindly. No signature mismatch was found; the brief's code matched.

### Fixed — two bugs found live by this task's own mandated manual smoke test (post-implementation review)

The task brief requires piping a JSON-RPC `initialize` request into the
built binary before committing. Going one step further — actually invoking
`nmap_quick_scan` against `127.0.0.1` with `nmap` installed on this machine
— surfaced two real bugs neither the brief nor any of Tasks 1-4's automated
tests (all of which mock/fake the real `nmap` process and hand-write
synthetic XML fixtures) could have caught. An `advisor()` review call helped
triage a third symptom (see "Observed, not fixed" below) as a smoke-test
artifact rather than a real bug.

**Bug 1 — nmap's own console output corrupted the MCP stdio wire
(`main.rs`, this task's own file).** `RealNmapProcess::run` spawned nmap via
`std::process::Command::new(exe).args(args).status()` — `Command`'s
**default** stdio behaviour is to *inherit* the parent's stdin/stdout/stderr.
nmap always writes its human-readable scan report (banner, port table,
"Nmap done...") to its own real stdout/stderr, **in addition to** the `-oX`
XML file this crate actually reads. Since that inherited stdout is the exact
same stream `rmcp`'s stdio transport uses for the MCP JSON-RPC wire, every
real scan interleaved nmap's plain-text console output directly into the
protocol stream a real MCP client (the orchestrator) would be reading —
confirmed live: a `tools/call` against `nmap_quick_scan` produced ~20 lines
of nmap's raw text on stdout with no valid JSON-RPC response visible at all
in a short capture window.

**Fix:** `.stdout(Stdio::null()).stderr(Stdio::null())` added to the spawned
`Command` — nmap's `-oX` XML file is unaffected (that's a separate,
explicitly-passed `-oX <path>` argument, not stdout). Verified only by
smoke (real-process spawning code in this crate is never unit-tested, per
this module's own existing doc comment on `RealNmapProcess`).

**Bug 2 — `parse_nmap_xml` (`report.rs`, Task 1) rejected every real nmap
scan as malformed XML.** Real `nmap -oX` output always begins with
`<!DOCTYPE nmaprun>` (confirmed by capturing a live `-oX` file from this
machine's own `nmap 7.95`), plus an `<?xml-stylesheet ...?>` processing
instruction and an XML comment containing entity references. `roxmltree`
(the XML parser this crate uses) rejects **any** DTD declaration by default
— `ParsingOptions::allow_dtd` defaults to `false`, specifically as a defense
against XML entity-expansion attacks — and Task 1's three hand-written test
fixtures (`host_up_open_port.xml`, `host_down.xml`,
`os_detect_with_matches.xml`) never included a DOCTYPE line, so this was
never exercised by any test in Tasks 1-4. The result: `parse_nmap_xml`
returned `ParseError::MalformedXml("XML with DTD detected")` for every real
scan on every machine — the crate was completely non-functional against
real `nmap` output from Task 1 onward, undetected until this task's
mandated end-to-end smoke check actually invoked a real `nmap` process for
the first time in this plan.

**Fix (`src/report.rs`):** `Document::parse(xml)` replaced with
`Document::parse_with_options(xml, ParsingOptions { allow_dtd: true,
..Default::default() })`. Safe here specifically because nmap's DOCTYPE
declaration has no internal or external subset (just the bare
`<!DOCTYPE nmaprun>` token) — nothing for an entity-expansion attack to
exploit.

- **New fixture `src/fixtures/real_nmap_output_with_dtd.xml`** — not
  hand-written from scratch like Task 1's fixtures; derived directly from a
  real `-oX` capture on this machine (target `127.0.0.1`), trimmed to 3
  ports for fixture-size sanity but preserving the real DOCTYPE, the
  `xml-stylesheet` PI, the entity-bearing comment, the `<hostnames>` block,
  and an `<extraports>`/`<extrareasons>` block — the actual shape real nmap
  emits, not an idealized one.
- **New test `parses_real_nmap_output_including_doctype_and_stylesheet_pi`**
  — RED-first: run against the unmodified `Document::parse(xml)` call,
  failed with `MalformedXml("XML with DTD detected")` (`cargo test -p
  mcp-nmap parses_real_nmap_output_including_doctype_and_stylesheet_pi`
  confirmed the panic message named the exact right cause, not a compile
  error or unrelated failure). Then the `parse_with_options`/`allow_dtd`
  fix was applied; GREEN.

#### Observed, not fixed — dropped `tools/call` response is a smoke-test artifact, not a production bug

A third symptom appeared during triage: piping a single-shot request with
stdin closing immediately after made the `tools/call` JSON-RPC response
disappear entirely (`rmcp`'s own stderr trace showed `input stream
terminated` almost immediately, then `WARN: timed out draining in-flight
responses` ~5s later, right around when the ~5.3s-long real scan finished).
This looked at first like a tokio-blocking-thread problem (`RealNmapProcess`
calls the *synchronous* `std::process::Command::status()` directly inside
an `async fn` tool handler, with no `spawn_blocking`), but re-running the
identical unmodified code with stdin held open past the scan's duration
(`{ printf '...'; sleep 15; } | mcp-nmap.exe`) delivered the response
correctly — proving the variable was stdin lifetime racing `rmcp`'s
in-flight-response drain timeout, not thread-pool exhaustion. In production
the orchestrator's `NmapToolClient` (companion integration plan) holds the
child process's stdin open for the sidecar's entire lifetime (same
persistent-process pattern as `mcp-server`'s session), so this race never
triggers there. Left unfixed deliberately — an `advisor()` review call
confirmed a `spawn_blocking` "fix" would not have changed the outcome (it
doesn't shrink nmap's own runtime) and would have been unwarranted scope
creep in a task meant to stay thin MCP glue.

#### Verification

- `cargo test -p mcp-nmap` → `test result: ok. 38 passed; 0 failed` (37
  pre-existing + 1 new DTD-parsing test), no warnings.
- `cargo build` (whole workspace, default-members) → clean.
- `cargo clippy -p mcp-nmap --all-targets` → no warnings, before or after
  both fixes.
- `cargo fmt -p mcp-nmap --check` → `main.rs` and the touched line in
  `report.rs` (the only files this task modifies) are clean. Remaining
  drift in `markdown.rs` is **pre-existing**, already documented as
  out-of-scope in this changelog's 0.3.0/0.4.0/0.5.0 entries.
- **Live manual smoke tests** (real `nmap 7.95` installed on this machine,
  on `PATH`):
  1. Bare `initialize` request piped in, single-shot (the brief's literal
     Step 2 check) → valid JSON-RPC `initialize` result, clean exit 0, no
     hang.
  2. `tools/list` → both `nmap_quick_scan` and `nmap_os_detect` present with
     correct JSON schemas (`target: string`, both `required`).
  3. `tools/call` against `nmap_quick_scan` targeting `127.0.0.1`, stdin
     held open past the scan's ~5s duration → **after both fixes**: stdout
     contains *only* the two expected JSON-RPC lines (no nmap console
     chatter), and the tool result is `is_error: false` with a real,
     correctly parsed 15-open-port Markdown report and LLM summary.

### Fixed — elevated process leak on timeout + stale doc summary (final whole-branch review)

Two findings from the final whole-branch review of this crate's completed
5-task plan, both corrections to already-committed code — folded into this
same `0.6.0` entry rather than bumped to a new version, matching this
changelog's own precedent (the `build_parameters` post-review fix above was
likewise folded into `0.4.0` rather than given its own bump; a `git log`
check confirmed `fix(mcp-nmap): quote/reject unsafe run_elevated arguments
(0.4.0)` kept the same version number as the feature commit it corrected).

**1. `src/elevate.rs` — `run_elevated`'s `WAIT_TIMEOUT` branch leaked the
elevated child process.** It called only `CloseHandle(hprocess)` before
returning `ElevationError::Timeout`. `CloseHandle` releases this process's
own handle reference — it does not stop the process it refers to. Design
spec §4 requires "processo elevato terminato se ancora vivo" (elevated
process terminated if still alive) on timeout; without this, an elevated
`nmap.exe` that hangs past `SCAN_TIMEOUT` (600s) kept running indefinitely,
and — since it could still hold the `-oX` tempfile open — could also make
`run_os_detect`'s `TempPath` cleanup silently fail to delete that file, a
tempfile leak stacked on top of the process leak.

**Fix:** `TerminateProcess(hprocess, 1)` (from `windows::Win32::System::
Threading`, added to the existing `use` line alongside
`GetExitCodeProcess`/`WaitForSingleObject`) is now called immediately before
`CloseHandle` in the `WAIT_TIMEOUT` branch only. `mcp-nmap` runs at medium
integrity (never elevated); the target process runs at high integrity
(elevated via UAC) — a medium-integrity process can normally terminate its
own child even when the child is elevated, since the child's token descends
from this process's own `ShellExecuteExW` call. If `TerminateProcess` still
fails for another reason, the error is deliberately ignored: the function
already returns `ElevationError::Timeout` regardless, and a failed forced
termination must not mask the timeout with a different error.

**Not unit-tested**, same category as the rest of `run_elevated` (a real
hung elevated process is needed to exercise this branch, which no automated
test in this crate can safely trigger). Verified by `cargo build -p
mcp-nmap` compiling cleanly with the new, type-correct FFI call.

**2. `IMPLEMENTATION.md`'s opening `## Scope` section was stale.** It still
described the crate as if only Task 1 were done (future tense — "This crate
will become an MCP sidecar server"; "This slice (Task 1 of 5) only builds
the crate scaffold"; `main.rs` described as "Currently a placeholder that
exits with an error message"). All false as of this branch — the crate is a
complete, working 5-tool-module MCP sidecar (`report`, `markdown`, `scan`,
`elevate`, `main.rs` all implemented) at `0.6.0`. Rewritten to describe the
crate's actual current state; the detailed per-task sections further down
were already accurate and are untouched, aside from a small correction to
the `run_elevated` flow description (steps 5-7) to reflect the
`TerminateProcess` addition from fix 1 above.

#### Verification (this fix)

- `cargo build -p mcp-nmap` → clean.
- `cargo test -p mcp-nmap` → `test result: ok. 38 passed; 0 failed` — same
  count as before this fix; no new tests, per the reasoning above (this
  branch is not safely unit-testable).
- `cargo clippy -p mcp-nmap --all-targets` → no warnings.
- `cargo fmt -p mcp-nmap --check` → the new `elevate.rs` import line needed
  wrapping to fit rustfmt's line-length rule; fixed. `elevate.rs` is fully
  fmt-clean after the fix. Remaining `markdown.rs` drift is **pre-existing**,
  already documented as out-of-scope since the 0.2.0/0.3.0 entries above.
- No crate version bump (stays `0.6.0`) — see the precedent note above.

## [0.5.0] — 2026-07-16

### Fixed — argv flag smuggling via unvalidated scan target (Critical, security review)

**La vulnerabilità:** `run_quick_scan` e `run_os_detect` (`src/scan.rs`)
costruivano gli argomenti di nmap come `["-sT"/"-O", "-oX", &xml_path_str,
target]` senza mai validare il **contenuto** di `target` — che è fornito dal
chiamante del tool (in ultima istanza l'LLM, tramite il dispatch dei tool
dell'orchestrator; non ancora agganciato in questo crate, ma questo è
esattamente il confine dove la validazione va inserita, dato che il glue MCP
del Task 5 chiamerà queste funzioni direttamente). Se `target` inizia con
`-` (es. `--script=vuln`, `--datadir=/qualche/path`), il parser degli
argomenti di nmap lo interpreta come un **flag**, non come bersaglio
posizionale — indipendentemente dal fix di quoting recente in
`elevate.rs`/`build_parameters` (0.4.0): quel fix controlla solo come
Windows spezza `lpParameters` in token argv, non la semantica
flag-vs-posizionale di nmap una volta che riceve un token argv già pulito.
Sul percorso di `run_os_detect` questo gira **elevato** (post-UAC),
raggiungendo direttamente il confine "niente script NSE / niente terzo tool
nmap" che la design spec (`Docs/superpowers/specs/2026-07-16-mcp-nmap-design.md`
§9) mette esplicitamente fuori scope per la v1 — non tramite un terzo tool
legittimo, ma tramite argument smuggling.

**Il fix — `src/scan.rs`:**

- `fn validate_target(target: &str) -> Result<(), String>` (nuova, vicino a
  `nmap_on_path`) — chiamata come **primissimo controllo** in entrambe
  `run_quick_scan` e `run_os_detect`, prima di qualunque tempfile o
  invocazione di processo. Regole:
  1. Target vuoto → rifiutato.
  2. Target che inizia con `-` → rifiutato (il vero vettore dell'exploit:
     nmap lo leggerebbe come flag).
  3. Target contenente spazi → rifiutato (questo tool accetta un solo
     target per chiamata; la sintassi multi-target di nmap separata da
     spazi resta fuori scope, stesso principio "un target per chiamata"
     già stabilito altrove in questo crate).
  4. Difesa in profondità: qualunque carattere fuori da una whitelist
     (alfanumerico ASCII + `. : / * - _`) → rifiutato. Deliberatamente
     permissiva per la sintassi legittima di nmap — IPv4, CIDR
     (`192.168.1.0/24`), range (`192.168.1.1-254`), wildcard di ottetto
     (`192.168.1.*`), hostname anche con trattini (`my-host.example.com`),
     IPv6 — dato che questi caratteri possono comparire ovunque tranne come
     primo carattere; **non** rifiuta il trattino in generale (romperebbe
     la sintassi dei range e gli hostname con trattino, molto comuni).
  5. Su fallimento, ritorna `ScanOutcome::error(...)` con un messaggio
     leggibile in italiano, stesso stile delle altre righe di errore in
     questo file (es. "nmap non trovato sul PATH").
- `run_quick_scan`/`run_os_detect` — aggiunta la chiamata a
  `validate_target(target)?` come primo statement del corpo, prima persino
  del controllo `nmap_available`.

#### TDD (RED → GREEN)

1. **`validate_target`'s 9 test unitari** — scritti per primi, riferendo una
   funzione non ancora esistente. `cargo test -p mcp-nmap validate_target`
   confermato RED: 11 errori di compilazione (`E0425`, tutti che nominano
   `validate_target` mancante). Poi aggiunta l'implementazione completa;
   GREEN (9/9).
2. **`quick_scan_invalid_target_is_a_readable_error_before_any_process_call`**
   — scritto usando lo stesso pattern del test esistente
   `quick_scan_nmap_not_available_is_a_readable_error_before_any_process_call`
   (un `NmapProcess` fake che va in panic se `run()` viene mai chiamato).
   Confermato RED per il motivo giusto: il test **panica** (non fallisce a
   compilare) perché, prima del wiring, `run_quick_scan` non validava
   affatto il target e raggiungeva `process.run(...)`. Poi aggiunta la
   chiamata a `validate_target` come primo controllo in `run_quick_scan`;
   GREEN.
3. **`os_detect_invalid_target_is_a_readable_error`** — deviazione
   deliberata e motivata dalla sicurezza dall'ordine RED-prima-del-codice
   stretto: a differenza del percorso `run_quick_scan`, `run_os_detect`
   chiama `elevate::run_elevated` **direttamente**, senza un seam
   iniettabile. Osservare un RED genuino per questo test (eseguire
   `cargo test` con `nmap_available: true` e un target-tipo-exploit *prima*
   di agganciare `validate_target` a `run_os_detect`) avrebbe realmente
   invocato `ShellExecuteExW(lpVerb="runas")` su questa macchina di sviluppo
   Windows — un vero prompt UAC/elevazione innescato da un test automatico,
   lo stesso rischio che il commento del test `os_detect_nmap_not_available_
   is_a_readable_error_before_elevation` già esistente in questo file
   documenta esplicitamente come motivo per cui quel test usa
   `nmap_available: false` anziché eseguire il percorso elevato per
   davvero. Per questo, la chiamata a `validate_target` è stata aggiunta a
   `run_os_detect` **nello stesso passaggio** in cui è stata aggiunta a
   `run_quick_scan` (prima ancora di scrivere/eseguire questo test), e il
   test è stato poi aggiunto ed eseguito solo contro il percorso già
   corretto — quindi va diretto a GREEN, senza un'osservazione empirica del
   RED per *questo* test specifico. Il test resta comunque un test reale e
   deterministico (nessun mock che finge il risultato): prova che il gate
   di validazione rigetta un target di tipo `--script=vuln` con un errore
   leggibile, usando deliberatamente `nmap_available: true` (non `false`)
   in modo che una futura regressione che rimuovesse la chiamata a
   `validate_target` non possa essere mascherata dal PATH-guard.
   **Nota SOLID/TDD:** questa è l'unica deroga all'ordine RED-poi-GREEN
   stretto in questo fix, ed è annotata qui esplicitamente perché motivata
   da un rischio di sicurezza reale (elevazione live), non da comodità.

#### Verification

- `cargo test -p mcp-nmap` → `test result: ok. 37 passed; 0 failed` (26
  pre-esistenti + 9 test di `validate_target` + 2 test di integrazione
  `quick_scan_invalid_target_...`/`os_detect_invalid_target_...`), nessun
  warning.
- `cargo build` (intero workspace, default-members) → pulito.
- `cargo clippy -p mcp-nmap --all-targets` → nessun warning, nuovo o
  preesistente.
- `cargo fmt -p mcp-nmap --check` → `scan.rs` (l'unico file toccato da
  questo fix) è pulito. Il drift residuo in `markdown.rs`/`report.rs` è
  **preesistente**, già documentato come fuori scope nelle voci 0.2.0/0.3.0
  di questo changelog.

## [0.4.0] — 2026-07-16

### Added — Windows elevation (`ShellExecuteExW`) for `nmap_os_detect` (Task 4)

**Scope:** Windows-only elevation module (`run_elevated`) that triggers a
UAC "runas" prompt via `ShellExecuteExW`, waits for the child process, and
returns its exit code — plus `run_os_detect`, `scan.rs`'s elevated
counterpart to Task 3's `run_quick_scan`.

- **`src/elevate.rs` (new):**
  - `ElevationError { UserCancelled, Timeout, Other(String) }` —
    `#[derive(Debug, Clone, PartialEq)]` + `impl Display` with readable
    Italian messages. Never a raw Windows error code surfaced directly to
    the tool caller (design spec §4).
  - `SCAN_TIMEOUT: Duration = 600s` — the elevated scan's own timeout,
    deliberately distinct from and larger than the confirm-gate's 180s
    (ADR-007/local-tool-confirm-gate): that 180s covers waiting for the
    user's confirm click, not an `-O` scan's own duration, which can run
    well past 180s on a `/24`.
  - `map_shell_execute_error(raw_code: i32) -> ElevationError` — pure
    function, `1223` (`ERROR_CANCELLED`) → `UserCancelled`, anything else →
    `Other` with the raw code embedded for diagnostics. The only part of
    this module unit-tested (see below) — everything else requires a real
    interactive UAC prompt.
  - `#[cfg(windows)] mod windows_impl::run_elevated(exe, args, timeout) ->
    Result<i32, ElevationError>` — `ShellExecuteExW(lpVerb="runas",
    fMask=SEE_MASK_NOCLOSEPROCESS)` → `WaitForSingleObject(hProcess,
    timeout)` → `GetExitCodeProcess`, with `CloseHandle` on every exit path
    (timeout, unexpected wait result, success). Every raw FFI call is
    wrapped in `unsafe` with a `// SAFETY:` comment; the public function
    itself is safe.
  - `#[cfg(not(windows))] fn run_elevated(...)` — stub returning
    `ElevationError::Other("elevazione non supportata su questa
    piattaforma...")`, keeping the crate's public API identical across
    platforms (design spec §9: elevation is Windows-only in v1; the
    macOS/Linux gemellaggio requirement is unaffected since there is no
    behavior to twin yet).
- **`src/scan.rs`:**
  - `run_os_detect(target, nmap_available) -> ScanOutcome` — same
    tempfile/PATH-check/parse/format flow as `run_quick_scan`, but pins
    `-O` instead of `-sT` and calls `crate::elevate::run_elevated` **directly**
    instead of going through `NmapProcess` — elevation has no meaningful
    cross-platform fake beyond what `elevate`'s own `cfg(not(windows))` stub
    already provides, and routing it through the same trait as the
    unelevated path would misleadingly suggest it's tested the same way.
  - Module doc and the `NmapProcess` trait doc updated to state this
    explicitly (a stale comment from Task 3, written before this task's
    actual design was finalized, previously implied `os_detect` would
    implement `NmapProcess` via `elevate::run_elevated` — it does not).
- **`src/lib.rs`** — added `pub mod elevate;` (alphabetized module order per
  `cargo fmt`'s default `reorder_modules`: `elevate, markdown, report, scan`).
  Doc comment's module list corrected to describe `scan`'s split behaviour
  accurately (mockable trait for quick scan only, direct call for os_detect).
- **`Cargo.toml`** — added `Win32_System_Registry` to the `windows` crate's
  `cfg(windows)` feature list. **Not anticipated by the design brief's
  "verified against docs" claim** — `SHELLEXECUTEINFOW` carries an unused
  `hkeyClass: HKEY` field that windows-rs 0.62.2 gates the whole struct (and
  `ShellExecuteExW` itself) behind this feature; discovered as a real
  `cargo build -p mcp-nmap` compile error (`E0432`, "no `SHELLEXECUTEINFOW`
  in `Win32::UI::Shell`"), not something either "verified" or "unverified"
  API-piece list in the task brief covered.

#### `HSTRING`/`PCWSTR` verification (Task 4 Step 1 — the brief's one
explicitly-unconfirmed API detail)

Ran `cargo doc -p mcp-nmap` (full doc generation, not `--no-deps`, so the
`windows`/`windows-strings` 0.62.2 dependency docs were generated too) and
read the actual generated source HTML
(`target/doc/src/windows_strings/{hstring,pcwstr}.rs.html`) rather than
trusting any cached knowledge:

- **`HSTRING::from(&str)`** — confirmed exact: `impl From<&str> for HSTRING`
  exists (`hstring.rs:129-133`), calling `Self::from_wide_iter(value.encode_utf16(), value.len())`.
- **`PCWSTR::from_raw(ptr: *const u16) -> Self`** — confirmed exact
  (`pcwstr.rs:10-12`), a `pub const fn`.
- **One nuance, not a mismatch:** `HSTRING` has no *inherent* `as_ptr`
  method. It implements `Deref<Target = [u16]>` (`hstring.rs:65-78`), and
  the slice's own `as_ptr(&self) -> *const u16` (stdlib) is what
  `verb.as_ptr()` resolves to via Rust's normal method-call auto-deref. The
  crate's `Deref` impl explicitly keeps the empty-string case pointing at a
  static null-terminated `[0u16]` "so that if `as_ptr` is called on the
  slice that the resulting pointer will still refer to a null-terminated
  string" (`hstring.rs:72-75`) — i.e. this is the intended idiom, not an
  accident of Deref coercion.
- **Verdict: the brief's exact code (`HSTRING::from(s)` +
  `PCWSTR::from_raw(h.as_ptr())`) is correct as written. No code change was
  needed for this piece** — confirmed by `cargo build -p mcp-nmap` compiling
  clean on the first attempt with this exact pattern (the two real compile
  errors that did occur, `Win32_System_Registry` and `WAIT_OBJECT_0`/
  `WAIT_TIMEOUT`'s module path, were both Cargo.toml/import-path issues
  unrelated to HSTRING/PCWSTR).

#### TDD (RED → GREEN), two separate slices

1. **`elevate.rs`'s 4 error-mapping tests** — written first, referencing
   `ElevationError`/`map_shell_execute_error`/`SCAN_TIMEOUT` before any of
   them existed. `cargo test -p mcp-nmap` confirmed RED: 6 compile errors
   (`E0425`/`E0433`), all naming the missing symbols. Then the full
   `elevate.rs` implementation was added; GREEN.
2. **`scan.rs`'s `os_detect_nmap_not_available_is_a_readable_error_before_elevation`**
   — added as its own RED/GREEN slice, mirroring
   `quick_scan_nmap_not_available_is_a_readable_error_before_any_process_call`.
   Not everything about `run_os_detect` is testable (the elevated path
   itself needs a real UAC prompt), but the `nmap_available == false`
   short-circuit is fully deterministic and has nothing to do with
   elevation — it was written first (`E0425: cannot find function
   run_os_detect`), confirmed RED, then `run_os_detect` was added; GREEN.

#### Real compile errors surfaced by `cargo build -p mcp-nmap` (Step 3)

Two, both fixed, neither related to the HSTRING/PCWSTR verification:

1. `E0432`: `windows::Win32::UI::Shell::{ShellExecuteExW, SHELLEXECUTEINFOW}`
   unresolved — both are gated behind the `windows` crate's
   `Win32_System_Registry` feature (because `SHELLEXECUTEINFOW` has an
   `hkeyClass: HKEY` field). Fixed by adding that feature to `Cargo.toml`.
2. `E0432`: `windows::Win32::System::Threading::{WAIT_OBJECT_0,
   WAIT_TIMEOUT}` unresolved — these `WAIT_EVENT` constants actually live in
   `windows::Win32::Foundation`, not `Win32::System::Threading`. Fixed by
   moving the import.

### Fixed — argument quoting in `run_elevated` (post-implementation review)

**Bug:** the design brief's literal code built `lpParameters` via
`args.join(" ")`. `ShellExecuteExW` passes `lpParameters` to the launched
executable as one command-line string, which nmap's own C runtime then
re-splits on whitespace. `tempfile`'s `-oX` output path lives under
`std::env::temp_dir()` (`C:\Users\<username>\AppData\Local\Temp\...`) — on
any machine whose Windows username contains a space (e.g. `John Smith`),
the unquoted path silently splits into two argv entries, truncating the
`-oX` path and making `nmap_os_detect` fail deterministically (at the later
`read_to_string`, "impossibile leggere l'output XML di nmap") on every such
machine, regardless of target. No test in the original implementation
touched this — the only guard test exercises `nmap_available == false` —
and a manual e2e run on this project's dev machine (`C:\Users\Maurizio`, no
space) would not have revealed it either. Found by an `advisor()` review
call after the initial GREEN, not by any automated check.

Also closes a related argument-injection concern: `target` is caller/tool
supplied and was being concatenated into the command line unescaped, so a
target containing a literal `"` could unbalance the whole `lpParameters`
string and smuggle extra argv entries past the intended boundary.

**Fix:** extracted `fn build_parameters(args: &[&str]) -> Result<String,
ElevationError>` — a pure, platform-agnostic function (no `cfg(windows)`,
no FFI) that wraps any argument containing whitespace in `"..."`, and
rejects (`Err`) any argument containing a literal `"` outright rather than
attempting to escape it (escaping here could itself introduce a subtly
different injection vector if this crate's escape rules don't exactly
match nmap's own unescaping). `run_elevated` now calls
`build_parameters(args)?` instead of `HSTRING::from(args.join(" "))`.

- **3 new tests**, RED-first (`cargo test` confirmed `E0425: cannot find
  function build_parameters` for all three before the function existed):
  - `build_parameters_quotes_an_argument_containing_whitespace` — a
    spaced-path argument gets wrapped in quotes; other args untouched.
  - `build_parameters_leaves_whitespace_free_arguments_unquoted` — no
    spurious quoting when nothing needs it.
  - `build_parameters_rejects_an_argument_containing_a_quote_character` —
    an embedded `"` (e.g. a malicious target) returns `Err`, never reaches
    `ShellExecuteExW`.

#### Verification

- `cargo test -p mcp-nmap` → `test result: ok. 26 passed; 0 failed` (18
  from Tasks 1-3 + 4 `elevate` error-mapping tests + 1 `os_detect`
  PATH-guard test + 3 `build_parameters` tests). Note: the task brief's
  Step 3 expected "17 previous + 4 new = 21 total" — the correct baseline
  carried over from Task 3's own corrected count is 18, and this task adds
  8 tests in total (not 4) once the `os_detect` guard test and the
  post-review `build_parameters` fix are included — so 26 is the actual,
  correct total.
- `cargo build` (whole workspace, default-members) → clean.
- `cargo clippy -p mcp-nmap --all-targets` → one `redundant_guards` warning
  on first pass (`Ok(exit_code) if exit_code == 0 => {}` in `run_os_detect`,
  copied verbatim from the brief), fixed to `Ok(0) => {}` per clippy's own
  suggestion. Clean after the fix, and clean again after the
  `build_parameters` fix.
- `cargo fmt -p mcp-nmap --check` → `elevate.rs` and `scan.rs` (the files
  this task owns) are clean. `lib.rs`'s module-order line needed
  alphabetizing (`elevate, markdown, report, scan`) after adding `pub mod
  elevate;` — fixed. Remaining drift in `markdown.rs`/`report.rs` is
  **pre-existing** (Task 2/Task 1, confirmed already noted in Task 3's own
  CHANGELOG entry) — out of scope for this task.

## [0.3.0] — 2026-07-16

### Added — unelevated `nmap_quick_scan` invocation (Task 3)

**Scope:** Process-invocation layer for the unelevated quick scan: PATH
availability check, tempfile handling for nmap's `-oX` output, a mockable
`NmapProcess` trait seam, and the orchestration (`run_quick_scan`) that ties
parsing (Task 1) + formatting (Task 2) together into one `ScanOutcome`.

- **`src/scan.rs` (new):**
  - `ScanOutcome { summary, report_markdown, is_error }` — the exact shape
    `main.rs` (Task 5) will serialize as the MCP tool's JSON response, and
    the shape the companion integration plan's `NmapToolClient` deserializes.
    `#[derive(Serialize, Deserialize)]`. `report_markdown` is always empty on
    an error outcome (design spec §7 — a tool error is not a report).
  - `NmapProcess` trait — `fn run(&self, args: &[&str], xml_path: &Path) ->
    Result<ExitStatus, String>`. The seam that keeps `run_quick_scan`
    testable without spawning a real `nmap`: tests implement it with a
    `FakeNmapProcess` that writes canned XML and returns a canned
    `ExitStatus`. The real implementation (spawning
    `std::process::Command::new("nmap")`) lands in Task 5's `main.rs`; Task
    4's elevated `-O` path implements the same trait via
    `elevate::run_elevated`.
  - `nmap_on_path() -> bool` — real `PATH` scan for `nmap.exe`/`nmap`. Called
    only from `main.rs` (Task 5, not this task) — kept separate from
    `run_quick_scan` precisely so this task's tests never depend on whether
    the machine running them has `nmap` installed.
  - `run_quick_scan(target, process, nmap_available) -> ScanOutcome` —
    `nmap_available` is an explicit parameter (not resolved internally via
    `nmap_on_path()`), so its tests pass literal `true`/`false` and stay
    deterministic. Flow: PATH check → tempfile for `-oX` → `-sT` pinned
    explicitly (never left to nmap's privilege-based default, design spec
    §3) → `process.run(...)` → non-zero exit / read / parse failures all
    become readable `ScanOutcome::error(...)` values → success builds
    `ScanOutcome` from `format_summary` + `format_report_markdown` over one
    shared `ScanReport`.
- **Test module: 4 tests** (all passing; RED-first — the module initially
  contained only the test block, which failed to compile with 6 errors all
  naming the missing `NmapProcess`/`ScanOutcome`/`run_quick_scan` symbols):
  - `quick_scan_success_produces_summary_and_report` — success path produces
    a non-error outcome with target in the summary and the open port in the
    report Markdown.
  - `quick_scan_nonzero_exit_is_a_readable_error` — non-zero exit status →
    error outcome mentioning the exit code, empty `report_markdown`.
  - `quick_scan_malformed_xml_is_a_readable_error_not_a_panic` — invalid XML
    with a successful exit → error outcome reusing `ParseError`'s "XML nmap
    malformato" message, no panic.
  - `quick_scan_nmap_not_available_is_a_readable_error_before_any_process_call`
    — `nmap_available: false` short-circuits before `process.run(...)` is
    ever called (proven with a fake process that panics if invoked).
- **`src/lib.rs`** — added `pub mod scan;`.

#### TDD (RED → GREEN)

`scan.rs` was written in two passes, same pattern as Tasks 1-2: first pass
was the test module alone (referencing `NmapProcess`/`ScanOutcome`/
`run_quick_scan`, none of which existed yet) — `cargo test -p mcp-nmap`
confirmed RED for the right reason (`E0405`/`E0425`, 6 compile errors, all
naming the missing pieces). Second pass added the full implementation;
`cargo test -p mcp-nmap` then went GREEN.

#### Verification

- `cargo test -p mcp-nmap` → `test result: ok. 18 passed; 0 failed` (14 from
  Tasks 1-2 + 4 from this task), no warnings. Note: the task brief's step 2
  expected "13 previous + 4 new = 17 total" — the actual baseline was 14 (5
  from `report.rs` + 9 from `markdown.rs`), so the correct total is 18, not
  17; this is a miscount in the brief, not a discrepancy in the code.
- `cargo build` (whole workspace, default-members) → clean.
- `cargo clippy -p mcp-nmap --all-targets` → no warnings.
- `cargo fmt -p mcp-nmap --check` → `scan.rs` (the only file this task fully
  owns) is clean after running `rustfmt` on it directly. Remaining fmt drift
  in `lib.rs`/`markdown.rs`/`report.rs` is **pre-existing at this task's
  starting commit** (confirmed via `git stash` + `cargo fmt --check` against
  the unmodified working tree) and matches the repo-wide, no-pinned-
  `rustfmt.toml` drift already documented in commit `0088e39` — out of scope
  for this task to fix.

## [0.2.0] — 2026-07-16

### Added — Markdown report formatting + invocation label (Task 2)

**Scope:** Deterministic Markdown output (full report + LLM summary) + nmap
invocation labels for the confirm banner.

- **`src/markdown.rs` — three pure output functions, zero I/O/process dependency:**
  - `format_report_markdown(report: &ScanReport) -> String` — full, per-host
    report (target → sections per host, ports list with services, OS matches
    if `-O`). Deterministic (byte-for-byte consistent for a given `ScanReport`)
    so snapshot-style test assertions are meaningful. Output is **never sent to
    the LLM** — reserved for the Markdown window + Library save.
  - `format_summary(report: &ScanReport) -> String` — brief, for the LLM's
    `tool_result` (design spec §6: host count, open-port count, no per-port
    service detail to keep the model's context small). Same spirit as the
    orchestrator's `agent::truncate_for_model` but achieved here by design —
    this crate has no orchestrator dependency, so it builds brevity into the
    function's contract.
  - `format_nmap_invocation(tool_name: &str, args: &serde_json::Value) -> String`
    — display label for the confirm banner (design spec §5). `nmap_os_detect`
    **explicitly says "richiede privilegi elevati"** — Windows' UAC dialog
    never shows the target to the user, so this is the only place they see it
    before elevation.
- **Test module: 9 tests** (all passing)
  - `markdown_report_includes_target_host_and_ports` — full report has target,
    host IP, port numbers, protocols, states, and service names.
  - `markdown_report_includes_os_matches_when_os_detection_true` — OS section
    appears only when `ScanReport.os_detection = true` and includes the matches.
  - `markdown_report_down_host_has_no_ports_section` — down host doesn't show
    a ports list (design spec §7: down hosts are legitimate results, not
    errors; ports list is omitted for hosts that aren't responding).
  - `markdown_report_empty_hosts_says_so` — empty `hosts` vector → "Nessun host
    nel risultato." (not an error).
  - `summary_counts_up_hosts_and_open_ports` — summary counts active hosts and
    open ports only.
  - `summary_zero_hosts_says_none_active_not_an_error` — all hosts down → "nessun
    host attivo trovato." (distinct from a parse or process error).
  - `invocation_label_quick_scan` — `nmap_quick_scan` → "nmap quick scan → {target}".
  - `invocation_label_os_detect_mentions_elevation` — `nmap_os_detect` → mentions
    elevation requirement and target.
  - `invocation_label_unknown_tool_falls_back` — unknown tool → "[tool {name}]"
    (graceful fallback).

#### TDD (RED → GREEN)

All 9 markdown tests were written first (RED) before any implementation. After
adding the three functions and verifying GREEN, then test module was moved into
the module (refactor, keeping tests green).

#### Verification

- `cargo test -p mcp-nmap` → `test result: ok. 14 passed; 0 failed` (5 from
  Task 1 + 9 from this task), no warnings.
- `cargo build -p mcp-nmap` → clean.
- No clippy or fmt warnings introduced.

## [0.1.0] — 2026-07-16

### Added — crate scaffold + pure XML parsing (`ScanReport`)

**Scope:** Task 1 of 5 (`Docs/superpowers/plans/2026-07-16-mcp-nmap-sidecar.md`).
New crate `crates/mcp-nmap`, mirroring `crates/mcp-server`'s `lib.rs` (pure
core) / `main.rs` (thin MCP glue) split. This slice only builds the XML
parsing/data-model layer — no MCP dependency wired up yet, no network calls,
no real `nmap` invocation.

- **`src/report.rs` — pure XML parsing, zero MCP/rmcp dependency:**
  - `parse_nmap_xml(xml: &str, os_detection: bool) -> Result<ScanReport, ParseError>`
    parses nmap's `-oX` XML output using `roxmltree` (read-only DOM parser —
    the schema is attribute-heavy and nested, so manual DOM extraction is
    more direct here than serde-derive).
  - `ScanReport { target, os_detection, hosts: Vec<HostReport> }` — `target`
    is extracted from `<nmaprun args="...">` (last whitespace-separated word
    of the command line nmap was invoked with). `os_detection` is passed in
    by the caller (reflects which tool was invoked — `nmap_quick_scan` vs
    `nmap_os_detect`), not read from the XML.
  - `HostReport { address, up, ports: Vec<PortReport>, os_matches: Vec<String> }`
    — `up` comes from `<status state="up|down">`. A host reported "down" is a
    **legitimate result, not an error** (design spec §7) — callers must not
    treat `up: false` as a failure.
  - `PortReport { port: u16, protocol: String, state: String, service: Option<String> }`
    — `state` is nmap's verbatim string (`"open"` / `"closed"` / `"filtered"`).
    `service` is `None` when nmap couldn't identify it.
  - `ParseError::MalformedXml(String)` — carries nmap's raw parse-failure
    reason for diagnostics (design spec §7: distinguishes "nmap crashed" from
    "bug in our parser"). Implements `std::fmt::Display`.
- **`src/lib.rs`** — crate root, currently exposes only `pub mod report;`.
  Doc comment lays out the full planned module architecture (`report` →
  `markdown` → `scan` → `elevate` → `main.rs`, Tasks 2-5).
- **`src/main.rs`** — placeholder binary (`[[bin]] name = "mcp-nmap"`), exits
  with an error message; real MCP stdio glue lands in Task 5.
- **`src/fixtures/`** — 3 realistic nmap `-oX` XML fixtures used by
  `report.rs`'s test module via `include_str!`:
  - `host_up_open_port.xml` — one host up, one open TCP port with a service,
    one closed port with no service.
  - `host_down.xml` — host reported down, no `<ports>` block at all.
  - `os_detect_with_matches.xml` — `-O` scan, one filtered port, two
    `<osmatch>` entries (ordered most-accurate first).
- **Dependencies:** `roxmltree = "0.21"` (XML parsing, used now), plus the
  full dependency set anticipated for later tasks (`rmcp = "=1.7.0"`, `tokio`,
  `serde`/`serde_json`, `anyhow`, `tracing`/`tracing-subscriber`, `tempfile`,
  and `windows = "0.62"` gated `cfg(windows)` for elevation) — declared now
  per the design spec so Tasks 2-5 don't need further `Cargo.toml` changes.
  None of these except `roxmltree` are used by any code yet.
- Workspace root `Cargo.toml`: added `"crates/mcp-nmap"` to both `members`
  and `default-members` (right after `"crates/mcp-server"`), so `cargo build`
  / `cargo test` without `-p` now include this crate.

#### TDD (RED → GREEN)

`report.rs` was written in two passes to get a genuine RED:

1. First pass: only the module doc comment + `#[cfg(test)] mod tests` (5
   tests, referencing `parse_nmap_xml`/`ScanReport`/`PortReport`/`ParseError`
   which did not exist yet). Crate scaffold (`Cargo.toml`, `lib.rs`,
   `main.rs`, workspace registration) was added so the crate could actually
   attempt to compile.
2. `cargo test -p mcp-nmap` confirmed RED for the right reason: `E0422`
   (`cannot find struct ... PortReport`), `E0433` (`cannot find type
   ParseError`), `E0425` (`cannot find function parse_nmap_xml`) — 8 compile
   errors, all naming the missing pieces, not a typo or unrelated failure.
3. Added the full implementation (structs, enum, `impl Display`,
   `parse_nmap_xml`). `cargo test -p mcp-nmap` → `test result: ok. 5 passed;
   0 failed`.

| Test | Assertion |
|------|-----------|
| `parses_host_up_with_open_and_closed_ports` | target extracted from `args`; 1 host up; 2 ports (open+service, closed+no service); `os_matches` empty |
| `parses_host_down_as_legitimate_zero_result_not_error` | `up: false` still parses `Ok`, no ports |
| `parses_os_detect_with_osmatches_and_filtered_port` | `os_detection: true` echoed back; `state: "filtered"`; 2 `os_matches` in nmap's order |
| `malformed_xml_is_a_readable_parse_error_not_a_panic` | truncated tag → `ParseError::MalformedXml`, `Display` contains "XML nmap malformato" |
| `empty_hosts_list_when_no_host_elements_present` | valid XML with zero `<host>` elements → `Ok`, empty `hosts` (not an error) |

#### Verification

- `cargo test -p mcp-nmap`: 5 passed, 0 failed, no warnings.
- `cargo build` (whole workspace, default-members): clean, confirms the new
  crate is correctly registered and doesn't break `protocol`/`mcp-server`/
  `orchestrator`/the plugin crates.
- `cargo clippy -p mcp-nmap --all-targets`: no warnings.
- `cargo fmt -p mcp-nmap --check`: clean (ran `cargo fmt -p mcp-nmap` once to
  reformat a few long lines/struct literals from the design brief into
  rustfmt's canonical style — semantics unchanged, all 5 tests still pass
  after reformatting).
