# HANDOFF — Lare Terminal 2.0

> Checkpoint per ripartire a contesto azzerato. Si aggiorna nello stesso commit di ogni release
> (hook `commit-msg`). Stato dettagliato per area: `crates/<crate>/IMPLEMENTATION.md`.

## Versioni correnti

(a fine piano 3 "finestra terminale" e i18n Parte 3 — lette da ogni `Cargo.toml`/`.csproj`)

- protocol 2.2.0 (da v1 0.15.4; 2.2.0 — `ServerMsg::MarkdownWindowTurnEnded` additivo per show_markdown update-in-place)
- startup-config 2.0.4 (da v1 0.1.0; `spawn_detached` piano 3 Task 3; 2.0.4 — modulo `logging`
  condiviso + stdio chiuse + `child_stderr_log_sink`, fix finestre console spurie)
- mcp-server 2.0.1 (da v1 0.7.1)
- mcp-nmap 2.1.0 (da v1 0.8.2; 2.1.0 — tool fritzbox_status + --config-dir in main)
- orchestrator 2.4.0 (da v1 0.41.21; 2.4.0 — show_markdown aggiorna in-place invece di aprire una
  finestra nuova ogni chiamata, Parte A del compito update-in-place)
- plugin-protocol 2.0.0 (da v1 0.2.1)
- plugin-ping 2.0.0 (da v1 0.1.0)
- plugin-counter 2.0.0 (da v1 0.1.0)
- plugin-calc 2.3.2 (da v1 0.2.0; 2.1.0-2.3.0 — modalità programmatore hex/oct/bin/bitwise/
  shift/rotate/larghezza bit; 2.3.1 — fix cifre fuori dall'alfabeto della base accettate nel
  buffer, riga di stato mostra sempre la base, layout tasti A-F riordinato; 2.3.2 — NOT e
  SHL/SHR riposizionati nel layout)
- plugin-lc 2.0.0 (da v1 0.4.5)
- plugin-crypto 2.0.0 (da v1 1.0.1)
- ui 2.3.4 (da v1 0.47.1; 2.3.4 — rinomina canale /nmap → /netsec nel frontend)
- lare-shell 2.0.1 (piano 2b 2.0.0 → 2.0.1 nel piano 3, Task 3: `Launcher.EnsureUi()` passa
  `--no-terminal` — non un crate Cargo: `shell/lare-shell/`, .NET/C#)

## FATTO

- **`show_markdown` aggiorna in-place — Parte A (2026-09-15)** — compito
  `Docs/i18n/ita/compiti-ai-esterne/2026-09-15-show-markdown-update-in-place.md` (Parte A, B e C
  ancora da fare). Bug trovato l'8/9 (mitigato solo con un'istruzione nel prompt, commit
  `6e0dc85`) e rivissuto dal vivo il 12/9: un turno `/ai` con ricerca web apriva la stessa
  risposta in più finestre separate mentre il modello la raffinava. Ora `show_markdown` apre la
  finestra una volta (`window_id = "{id}-md"`, stabile per il turno) e le chiamate successive
  aggiornano quella stessa finestra (`OutputWindowContent`) invece di aprirne una nuova — nuovo
  `ServerMsg::MarkdownWindowTurnEnded` (guard RAII, emesso a qualunque uscita del turno, incluso
  cancellazione) segnala alla UI quando il turno è concluso, per la Parte B (gate di chiusura con
  conferma) e C (badge di progresso). Prerequisito trovato in investigazione: il buffer
  `output-buffer.mjs` lato frontend era scritto per un caso one-shot — reso persistente, altrimenti
  il secondo aggiornamento si sarebbe perso in silenzio. protocol 2.1.1 → 2.2.0, orchestrator
  2.3.0 → 2.4.0. 951 test (orchestrator), tutti verdi.

- **Rinomina canale `/nmap` → `/netsec` (2026-09-12)** — compito
  `Docs/i18n/ita/compiti-ai-esterne/2026-09-12-netsec-rename-fritzbox.md`:
  il canale tool esterno di rete cambia nome user-facing: slash trigger, id canale, titolo
  finestra. Tutti i nomi Rust interni (crate `mcp-nmap`, `NmapToolClient`, ecc.) restano
  invariati. Aggiornati `external_channel.rs`, `shell_slash.rs`, `core.rs`, `ws.rs`,
  `external-channels.js`, `help/*.md` (3 lingue). Orchestrator 2.2.6 → 2.2.7, UI 2.3.3 → 2.3.4.
  947 test passano.

- **Tool fritzbox_status nel canale netsec (2026-09-12)** — Parte B del compito:
  canale `/netsec` (ex `/nmap`) guadagna l'ottavo tool: `fritzbox_status`, che legge
  lo stato del router FRITZ!Box domestico via `fritzconnection` (log eventi, IP pubblico,
  dispositivi LAN). Nuovo modulo `crates/mcp-nmap/src/fritzbox.rs` (script Python one-shot,
  stesso pattern di `network_info.rs`), `--config-dir` ora interpretato da `mcp-nmap::main()`.
  Config: `fritzbox.example.json` committato, `fritzbox.json` in `.gitignore`.
  System prompt aggiornato con la nota d'onestà sul limite del log eventi.
  `mcp-nmap` 2.0.1 → 2.1.0, `orchestrator` 2.2.7 → 2.3.0.

- **plugin-calc 2.3.2 — Fix: NOT e SHL/SHR riposizionati nella sezione programmatore
  (2026-09-10)** — segnalato da Maurizio dal vivo (screenshot): NOT stava isolato nella riga
  base, separato da AND/OR/XOR; SHL/SHR stava lontano da ROL/ROR. Scambiati: NOT ora accanto a
  XOR (blocco booleano 2×2 con AND/OR), SHL/SHR ora subito sopra ROL/ROR (stessa colonna).
  Solo uno scambio di due `data-evt` nel markup di `programmer_key_grid`, nessuna logica
  toccata. 1 nuovo test, 136 totali, tutti verdi, clippy pulito. Fix diretto del supervisore.

- **plugin-calc 2.3.1 — Fix: cifre fuori dall'alfabeto della base accettate nel buffer
  (2026-09-10)** — segnalato da Maurizio dal vivo: in Bin il tasto "2" veniva accettato dopo
  "1101"; in Dec i tasti esadecimali A-F (es. "C") venivano accettati anch'essi — in entrambi i
  casi l'errore si vedeva solo al successivo "=". Nuova `is_valid_digit_for_base(c, base)` in
  `main.rs`: cifra/lettera/punto fuori dall'alfabeto della base attiva ignorata silenziosamente
  alla pressione (tasti restano tutti visibili, nessun tasto disabilitato in UI). Stessa
  funzione estende la regola "smart clear after result" alle cifre esadecimali (prima solo
  `is_ascii_digit()`). Riga di stato: la base è ora sempre mostrata, anche `DEC` (prima solo
  Hex/Oct/Bin la mostravano) — la larghezza bit resta l'unica cosa nascosta in Dec (inerte lì).
  Layout `programmer_key_grid` riorganizzato su richiesta di Maurizio: le cifre A-F ora seguono
  lo stesso ordine "dal basso, sinistra poi alto" della tastiera decimale (A parte dalla 3ª riga
  contando dal basso, D-E-F occupano la riga in cima) — le altre righe (base/larghezza)
  riorganizzate di conseguenza. 6 nuovi test TDD, 135 totali, tutti verdi, clippy pulito. Fix
  diretto del supervisore (non delegato — troppo piccolo/vincolato per giustificare il ciclo
  compito→dispatch→revisione).

- **plugin-calc 2.3.0 — Modalità programmatore Parte C: UI, tasti, CSS, conversione (2026-09-10)** —
  `CalcState` guadagna `base_mode`, `bit_width`, `prog_visible`. Nuovi rami `handle_key` per
  basi (DEC/HEX/OCT/BIN), larghezze (BYTE/WORD/DWORD/QWORD), operatori bitwise (AND/OR/XOR/NOT),
  shift/rotate con 2nd, toggle sezione programmatore. Nuova funzione `programmer_key_grid`
  (4 righe × 5, 20 tasti ambra). Display lineare (mai 2D) in base non-Dec. Righello stato
  mostra base+larghezza quando non-Dec. CSS: `.lare-prog-section` ambra, `.lare-prog-toggle`.
  12 nuovi test. 129 totali (43 engine + 64 format/render/main esistenti + 22 nuovi), tutti
  verdi, clippy pulito. plugin-calc 2.2.0 → 2.3.0.

- **plugin-calc 2.2.0 — Modalità programmatore Parte B: format_integer_in_base (2026-09-10)** —
  Aggiunto `format_integer_in_base(x, base, width)` in `format.rs`: formattazione in
  Hex/Oct/Bin con mascheratura alla larghezza bit, zero-padding SEMPRE a cifre piene
  e raggruppamento `_` per Hex/Bin. 8 nuovi test; 114 totali, tutti verdi.

- **plugin-calc 2.1.0 — Modalità programmatore Parte A: engine base-aware (2026-09-10)** —
  Compito `Docs/i18n/ita/compiti-ai-esterne/2026-09-10-calc-modalita-programmatore.md`:
  introdotti `NumBase` (Dec/Hex/Oct/Bin) e `BitWidth` (Byte/Word/Dword/Qword) in `engine.rs`.
  Tokenizer base-aware con `tokenize_with_base`, parser esteso con 4 nuovi livelli di
  precedenza C-like (shift/rotate > AND > XOR > OR sopra `expr`), valutatore
  `evaluate_with_width` con `to_i64_checked`. 8 nuovi operatori bitwise/shift/rotate/NOT
  a simboli Unicode dedicati. 15 nuovi test. Render.rs: `prec()` rinumerata + nuovi rami
  (necessario per compilare, i branch esaustivi richiedevano i nuovi pattern). Tutti i
  106 test passano. Bump plugin-calc a 2.1.0.

- **Doc `/help`: chiarito che `/open <target>` non vuole virgolette (2026-09-10)** —
  segnalazione dal vivo di Maurizio: `/open "https://www.linux.org"` (con virgolette, per
  abitudine da riga di comando) produceva `target non trovato o non riconosciuto:
  C:\Users\Maurizio"https://www.linux.org"`. Root cause verificata in
  `crates/orchestrator/src/core.rs::handle_slash`: il testo dopo `/open` è preso verbatim con
  `splitn(2, char::is_whitespace)` — non c'è un parser di shell, quindi le virgolette digitate
  restano parte del target e `resolve_open_target` non le riconosce né come URL né come path
  assoluto, cadendo nel ramo relativo (`Path::new(cwd).join(target)`). Nessun fix di codice: dato
  che `rest` è già l'intera coda della riga (spazi compresi, non ri-tokenizzata), le virgolette
  non servono MAI per `/open`, nemmeno per path con spazi — a differenza di `/ai`, che le richiede
  davvero. Aggiunta una riga di chiarimento alla voce `/open` in tutti e 3 i file
  `Test Run/Configuration/help/{it,en,es}.md`, in contrasto esplicito con `/ai` poco sopra nello
  stesso file. Nessun bump di versione (solo contenuto testuale, stessa regola già applicata alle
  traduzioni).

- **Fix reconnect infinito su canale esterno e reset backoff (2026-09-09)** —
  Compito `Docs/i18n/ita/compiti-ai-esterne/2026-09-09-reconnect-infinito-canale-esterno.md`:
  risolto il debito architetturale della v1 tracciato in `KNOWN-ISSUES.md`. Aggiunto parametro opzionale
  `maxRetries` a `LareWsClient` (`ws-client.js`); se superato, il client arresta i tentativi ed emette
  lo stato terminale `"failed"`. `external-channel-window.js` imposta `maxRetries: 6` (~31s totali di backoff).
  `host.js` (canale cursore) mantiene il retry infinito per sopravvivere ai riavvii backend. Spostato inoltre il
  reset del backoff da `"open"` (livello TCP) a `server_info` (handshake applicativo riuscito), evitando che un server
  che accetta e chiude subito resetti il timer a 1s all'infinito. Aggiornato `renderer.js` con il badge `"failed"`
  e tradotta la chiave `ext_channel.status_failed` in `it.json`, `en.json`, `es.json`. UI bump a 2.3.3.

- **i18n Terza lingua: spagnolo per AI, UI e /help (2026-09-09)** —
  Parte B del piano `Docs/i18n/ita/compiti-ai-esterne/2026-09-09-i18n-help-esterno-e-spagnolo.md`:
  introdotta la terza lingua (`es`, spagnolo). Aggiunta costante `RESPOND_SPANISH` in `crates/orchestrator/src/agent.rs`
  e gestito `Some("es")` in `system_prompt()`. Aggiunta costante `HELP_TITLE_SPANISH` e ramo `Some("es")` per `/help`
  in `crates/orchestrator/src/core.rs`. Creato il corpo Markdown esterno `Test Run/Configuration/help/es.md`.
  Aggiunta opzione `"Español"` al menu di selezione lingua in `crates/ui/frontend/config-dialog.js`.
  Generalizzato `crates/ui/frontend/i18n-parity.test.mjs` per la verifica dinamica di tutti i dizionari `*.json`.
  Creato `Test Run/Configuration/i18n/es.json` con 209 chiavi interamente tradotte. Test frontend (255 passati),
  test Rust orchestrator (948 lib + 2 bin + 23 int) e test ui (100 lib + 47 bin) tutti verdi; clippy senza warning.
  Orchestrator bump a 2.2.6, UI bump a 2.3.2.

- **/help: aggiunti /find, /reset, /nowin ai file help/<lang>.md (2026-09-09)** —
  I tre comandi erano attivi da codice ma assenti dalla finestra di /help. Solo contenuto
  Markdown in it.md/en.md/es.md, nessun cambiamento di codice.

- **i18n /help su file esterni help/<lang>.md (2026-09-09)** —
  Parte A del piano `Docs/i18n/ita/compiti-ai-esterne/2026-09-09-i18n-help-esterno-e-spagnolo.md`:
  migrato il corpo Markdown di `/help` da costanti Rust a file esterni sotto `Configuration/help/<lang>.md`.
  Nuovo modulo `crates/orchestrator/src/help.rs` (`help_dir_path`, `load_help_body`) con catena di fallback
  `<lang>.md` -> `it.md` -> stringa minima di sicurezza. Creati `Test Run/Configuration/help/it.md` ed `en.md`.
  Rimossi `HELP_MARKDOWN_IT` e `HELP_MARKDOWN_EN` da `core.rs`. Propagato `config_dir: &Path` a `handle_command`
  e `handle_slash` con cablaggio in `ws.rs`, `shell_turn.rs` e `telegram::channel`. Test unitari TDD in `help.rs`
  e aggiornamento test in `core.rs` con `tempfile::tempdir()`. Tutti i test dell'orchestrator passano,
  clippy senza nuovi warning. Orchestrator bump a 2.2.5.

- **i18n Fix: /help rispetta la lingua selezionata (2026-09-09)** —
  Risolto il difetto riscontrato dal vivo in cui `/help` rimaneva in italiano con l'interfaccia in inglese (`Docs/i18n/ita/compiti-ai-esterne/2026-09-09-i18n-fix-help.md`).
  Sdoppiato il contenuto statico in `HELP_MARKDOWN_IT` e `HELP_MARKDOWN_EN` in `crates/orchestrator/src/core.rs`.
  Aggiunto parametro `lang: Option<&str>` a `handle_slash` e propagato da `handle_command` (`lang.as_deref()`).
  Nel ramo `"help"`, selezionati titolo e testo tradotto (`Commands` vs `Comandi`).
  Nuovo test TDD `slash_help_respects_language_directive` (ciclo RED→GREEN completato e verificato).
  Tutti i 969 test dell'orchestrator passano; clippy senza warning. Orchestrator bump a 2.2.4.

- **i18n Parte 3 — Lingua dell'AI sul canale cursore (2026-09-09)** —
  Terza fase del piano `Docs/i18n/ita/compiti-ai-esterne/2026-09-08-i18n-programma.md`:
  esteso il protocollo con campo additivo `ClientMsg::Command.lang: String` (`#[serde(default)]`,
  default `""` sul wire). Isolamento della frase direttiva finale del prompt di sistema in
  `crates/orchestrator/src/agent.rs` (`RESPOND_ITALIAN` e `RESPOND_ENGLISH`); `system_prompt()` appende
  la direttiva inglese per `lang == Some("en")` e la direttiva italiana di default per `None`/`"it"`.
  I canali con `system_prompt_override` (Telegram, mcp-nmap, AI Chat) restano intatti.
  Implementata la lettura della preferenza da `config.json` (`read_language`) come fallback
  per chiamanti che inviano `lang` vuoto. Cablato `lang` in `ws.rs`, `shell_turn.rs` e `core.rs`.
  Nel frontend, `LareWsClient` accetta `lang` nel constructor, fornisce `setLanguage(lang)` e
  popola `ClientMsg::Command.lang` in `sendCommand`; `window.js` ed `external-channel-window.js`
  propagano la lingua corrente. Tutti i test passano: protocol (82), orchestrator (969),
  ui (147), e test frontend Node.js (255 passati su 255). Protocol bump a 2.1.1, orchestrator a 2.2.3.

- **i18n Parte 2 — Internazionalizzazione a cascata di tutte le finestre (2026-09-09)** —
  Seconda fase del piano `Docs/i18n/ita/compiti-ai-esterne/2026-09-08-i18n-programma.md`:
  conversione a cascata di tutte le restanti 10 finestre dell'applicazione (`note-window`,
  `window-search`, `library`, `aichat-window`, `routine-preview`, `plugin-window`,
  `external-channel`, `screener-picker`, `window.html`, `host.js`, `terminal.html`/`terminal.js`).
  Titoli nativi delle finestre in Rust (`main.rs`) internazionalizzati dinamicamente via
  `ui_lib::i18n::t_sync` nel builder Tauri. Dizionari `it.json` ed `en.json` portati a 209 chiavi
  in sincronia e ordine alfabetico perfetto. Test di parità `i18n-parity.test.mjs` verde.
  Tutti i 251 test frontend e 147 test Rust passano; clippy pulito senza warning. UI bump a 2.3.1.

- **i18n Parte 1 — Fondamenta e internazionalizzazione /config (2026-09-08)** —
  Prima fase del piano `Docs/i18n/ita/compiti-ai-esterne/2026-09-08-i18n-programma.md`:
  nuovo modulo backend `crates/ui/src-tauri/src/i18n.rs` con caricamento infallibile
  `load_dict`, catena di fallback `load_merged_dict` (`<lang> -> it -> key`) e `t_sync`.
  Nuovo comando Tauri IPC `get_i18n` registrato in `main.rs`. Esteso `Config` con campo
  `language: String` (default `"it"`) e piena compatibilità all'indietro. Dizionari
  in `Test Run/Configuration/i18n/{it,en}.json` con 34 chiavi iniziali. Modulo frontend
  `crates/ui/frontend/i18n.mjs` (`t`, `fetchI18n`, DOM walker `applyI18n`). Conversione
  completa di `/config` (`config.html`, `config-window.js`, `config-dialog.js`) con
  dropdown per la selezione della lingua ("Italiano" / "English"). Test automatico di
  parità chiavi `i18n-parity.test.mjs` attivo e verde. Tutti i test passano (`cargo test -p ui`:
  100 lib + 47 bin; `node --test`: 251 test). UI bump a 2.3.0.

- **Porting plugin v1, completamento deploy (2026-09-08)** — `build.ps1` e
  `deploy_test_run.ps1 -IncludePlugins` includono ping/calc/counter/crypto/lc.
  Aggiunti i manifest di counter/crypto/lc in `Test Run/plugins/`, copie dei sorgenti
  come per ping/calc. Nuova e2e reale `plugin_crypto_e2e.rs`: handshake, Cesare,
  cifratura/decifratura e dialog parametri (Applica e chiusura). Verifica dal vivo:
  tutte e tre le finestre si aprono, nessuna nuova console spuria. Trovato e corretto
  nell'host il sender WS obsoleto dopo riapertura UI: slot condiviso consultato dai pump,
  regressione e2e RED→GREEN e riapertura verificata dal vivo sugli stessi PID plugin.
  Solo orchestrator passa a 2.2.2; plugin invariati. Esiti e limite della chiusura UI nel
  report `reports/2026-09-08-codex-plugin-porting-v1.md`; lavoro per revisione del supervisore.

- 2026-09-04/05 — brainstorming, spec, due spike (`Docs/i18n/ita/spikes/`).

**Piano 1 — "fondamenta"** (`Docs/i18n/ita/superpowers/plans/2026-09-05-piano-1-fondamenta.md`),
completato il 2026-09-05:

- **Task 0** — Scaffolding del repo: workspace Cargo, `CLAUDE.md`, hook `commit-msg`. Commit `3ab5848`.
- **Task 1** — Copia dei crate v1 nel workspace 2.0, bump a `2.0.0`, baseline verde
  (`orchestrator`: 901 test passati). Commit `3c620fc`; fix round 1 `da7a7b8` (fixture di test
  spostate dentro il crate, non in `Docs/`).
- **Task 2** — `startup-config` 2.0: riscrittura completa, `--config-dir`/`startup.json` nuovo
  schema, nessuna variabile d'ambiente. Commit `38a6d39`.
- **Task 3** — `mcp-server`: migrato a `--config-dir`, niente più env var. Commit `12cc6ee`.
- **Task 4** — `orchestrator`: `--config-dir`, `RuntimeConfig` (context object), figli spawnati
  con `--config-dir` esplicito, log su file + `--console-log`. Commit `9aa1737`; fix round 1
  `91fda19` (modulo `logging.rs`: niente più panic se la cartella di log non è scrivibile).
- **Task 5** — Python `pytools`: `config_dir.py` sostituisce `local_dir.py`, `--config-dir` al
  posto di `LARE_LOCAL_DIR`. Commit `41fd5ff`.
- **Task 6** — `ui`: `--config-dir`, `startup.json`, un solo risolutore (`ConfigDirState`) al
  posto dei sei sparsi in v1. Commit `e99e3fe`.
- **Task 7** — `ui`: overlay F2 rimosso, pagina host nascosta (`host.html`/`host.js`), flag di
  sviluppo `--open config|library`. Commit `632fb45`.
- **Task 8** — `Test Run\` (layout di deploy dentro il repo) e `deploy_test_run.ps1`; verificato
  dal vivo (orchestrator + `ui.exe` da `Test Run\`, nessun processo residuo). Commit `68bfb9d`;
  fix `d75cdee` (gitignore file generati), `2b01172` (gitignore `config.json`).
- **Task 9** — Documentazione narrativa 2.0 (`RUN-LOCAL.md`, `DEPLOY.md`, `TESTING-e2e.md`,
  `KNOWN-ISSUES.md`, `06-decisions.md` con ADR-015..017) e questo aggiornamento di `HANDOFF.md`.
  Questo commit di release (`release: piano 1 completato — fondamenta 2.0 (config unica, ui
  host di finestre, Test Run)`).
- Fix wave della review finale (whole-branch, findings I1-I3/M1/M2/M3/M4/M6/M7/M8): assolutizzazione
  di `--config-dir` spostata in `startup-config`, `--config-dir` anche ai plugin, pulizia commenti
  frontend `app.js`→`host.js`, `LISTEN_ADDR` morta, exit code di `deploy_test_run.ps1`. Commit
  `93eb355`.

**Piano 2a — "protocollo shell"** (`Docs/i18n/ita/superpowers/plans/2026-09-05-piano-2a-protocollo-shell.md`),
completato il 2026-09-06 — `protocol`/`orchestrator`/`ui` 2.1.0. Test: 1472 nel workspace
(`cargo test`, default-members, senza `ui`) + 136 di `ui` (`cargo test -p ui`) + 231 JS
(`node --test crates/ui/frontend/*.test.mjs`); nessuna regressione clippy rispetto a `main`
(10 warning invariati):

- **Task 1** — `protocol`: `Role{Ui,Shell}`, campi additivi di `Hello`, i messaggi del canale
  shell (`ExecInShell`/`ExecResult`, `OpenOutputWindow`/`OutputWindowContent`, `OpenUiLocal`,
  `UiPing`/`UiPong`, `ActivityIndicator`), `ServerMsg::surface()` (match esaustivo senza
  wildcard). Commit `44e025a`.
- **Task 2** — `orchestrator`: `connections.rs`, il registro delle connessioni vive (sink `ui`,
  sessioni shell, ping `ui` pendenti). Commit `90d91c0`.
- **Task 3** — `orchestrator`: `ShellConfirmer` (gate SEMPRE, composizione su `LocalUiConfirmer`)
  ed etichetta `(interattivo)` nel prompt `[Y/n]` di `run_in_session`. Commit `a3070ce`.
- **Task 4** — `orchestrator`: `ShellSessionToolClient`/`ShellSessionState` — `run_in_session`
  come round-trip `ExecInShell`/`ExecResult`, cwd per sessione. Commit `a027994`.
- **Task 5** — `orchestrator`: `surface.rs`, il router che bufferizza i `Chunk` e li consegna
  una volta alla finestra di output a `Done`/`Error`. Commit `f53094e`; fix `5f863d7` (una sola
  terminazione per turno verso la shell).
- **Task 6** — `orchestrator`: `shell_slash.rs` (pre-router: `/ai` con virgolette, discard,
  `OpenUiLocal`) e `/help` 2.0. Commit `6f12ea1`; fix `6bd58e8` (testo di `/ping` in `/help`,
  doc di `KNOWN_BACKEND_SLASHES`).
- **Task 7** — `orchestrator`: `/ping` built-in — sonda plugin usa-e-getta, uptime, report per
  strato. Commit `4244b7b`.
- **Task 8** — `orchestrator`: cablaggio in `ws.rs` — `HelloInfo`, dispatch per `Role`, nuovo
  modulo `shell_turn.rs`, `ExecResult`/`UiPong` risolti nel registro. Commit `e33a455`.
- **Task 9** — `ui`: finestra di output aggiornabile, `open_ui_local`, `/help` singleton,
  `ui_pong`. Commit `1f3db93`; fix `63b99f6` (buffer-and-replay di `output:content` —
  `output-buffer.mjs` + protocollo `output:subscribe`/`output:closed`).
- **Task 10** — `orchestrator`/dev: 5 scenari e2e del canale shell (`tests/ws_integration.rs`) +
  client di sviluppo `scripts/dev/shell-client.mjs` (imita `lare-shell`, nessuna dipendenza).
  Commit `2449318`; fix `c3325a4` (niente marcatore di cwd sulla console con `capture:false`,
  `--selftest`).
- **Task 11** — documentazione: ADR-018, tre emendamenti allo spec (`Hello.version`, discard di
  `/find`/`/nowin`, eccezione `/help`/`/show` senza finestra di output), versioni 2.1.0,
  CHANGELOG/IMPLEMENTATION dei tre crate, questo aggiornamento di `HANDOFF.md`. Commit di
  release (`release: piano 2a completato — canale shell nel protocollo e nell'orchestratore
  (protocol/orchestrator/ui 2.1.0)`).

**Piano 2b — "host C# `lare-shell`"** (`Docs/i18n/ita/superpowers/plans/2026-09-06-piano-2b-host-lare-shell.md`),
completato il 2026-09-07 — nuovo componente **`lare-shell` 2.0.0** (`protocol`/`orchestrator`/`ui`
restano 2.1.0, non toccati). Test: 115/115 (`dotnet test shell/lare-shell/LareShell.sln`); `cargo
test` invariato (1475 verdi, riverificato). Verificabile in **modalità B** (profilo Windows
Terminal "Lare Terminal", `lare-shell.exe` nudo):

- **Task 0** — soluzione .NET, configurazione (`--config-dir`, `startup.json`, `token`), log su
  file. Commit `dcc5a63`; fix `6b29d7a` (`UnauthorizedAccessException` catturata in
  `StartupConfig`/`TokenFile`, test del log no-op).
- **Task 1** — `Wire`: i messaggi del canale shell (parse/serializzazione snake_case). Commit
  `2ce340b`; fix `89aff4b` (`ts` di `pong` obbligatorio, campi numerici malformati →
  `WireException` invece di un'eccezione grezza).
- **Task 2** — `OrchestratorClient`: handshake `hello`/`server_info`, loop di ricezione su thread
  proprio → `Channel`, server WS finto in-process (`FakeOrchestrator` su `TcpListener`, mai
  `HttpListener`). Commit `78fa217`; fix `3990362` (socket rilasciato su timeout di connessione,
  `Connect` no-op se già connesso).
- **Task 3** — classi host (`LareHost`/`LareHostUI`/`LareRawUI`), copiate dallo spike con
  adeguamenti (seam `IConsoleModes`, `OutputRecorder` con cap 200 KB testa+coda). Commit `a59d454`;
  fix `1ef1703` (`SupportedOSPlatform=windows` al posto di `NoWarn`, test `WriteProgress` e
  fallback delle mode).
- **Task 4** — runspace ospitata (PSReadLine da pwsh, execution policy da file), profili con
  `$PROFILE`, `Executor` (capture, exit code, cwd, Stop) — include il fix RID `win-x64` scoperto
  durante l'implementazione (vedi "Debiti" sotto). Commit `36c4c0a`; fix `174a59f` (comando in
  `try/finally`, non un append diretto: `return`/`. { }` rompevano la cattura di `$?`, verificato
  empiricamente in due iterazioni).
- **Task 5** — gate `[Y/n]` (`ConsoleGate`, polling dei tasti) e `SlashTurn` (ciclo del turno sul
  thread del REPL, contratti (a)-(d) del piano 2a). Commit `6614985`; fix `1a2f267` (guardia
  `Done.Id` rafforzata nei test, gate chiuso su EOF, eccezione dell'executor → cancel invece di un
  turno appeso).
- **Task 6** — `Launcher`: autostart di `orchestrator.exe` (retry 5 s) e di `ui.exe`, processi
  senza console ereditata. Commit `1970b4e` (review Approved, nessun fix necessario).
- **Task 7** — `Repl` completo (PSReadLine, profili, slash → `SlashTurn`, Ctrl+C, riconnessione on
  demand), OSC 9001 `intercept`, `--selftest`; smoke test dal vivo (autostart di
  orchestrator+ui verificato). Commit `ba890f5`; fix `dda8c0a` (handler Ctrl+C mai lancia — niente
  `using` sul CTS del turno, `--selftest` fallisce se `startup.json` manca, banner PSReadLine
  preciso).
- **Task 8** — `deploy_test_run.ps1` pubblica la host (`dotnet publish` framework-dependent
  win-x64) in `Test Run\shell\` (41,4 MB misurati), `install-wt-profile.ps1` installa il profilo
  Windows Terminal "Lare Terminal". Commit `8f3dc9f`.
- **E2E dal vivo** (controller, due passate in modalità B): trovato e corretto un difetto (`ui.exe`
  in build debug rubava il fuoco a Windows Terminal) — fix `a8d148f` (2 file + test, 115/115), poi
  riverificato (autostart senza scheda WT spuria, riconnessione dopo aver ucciso l'orchestratore).
  Dettaglio in `TESTING-e2e.md` Parte 6.
- **Task 9** — Documentazione (`CHANGELOG.md`/`IMPLEMENTATION.md` di `shell/lare-shell/`, ADR-019,
  emendamenti allo spec §4.4/§6.4/§9/§13, `DEPLOY.md`/`RUN-LOCAL.md`/`TESTING-e2e.md`/
  `KNOWN-ISSUES.md`, questo aggiornamento di `HANDOFF.md`). Questo commit di release (`release:
  piano 2b completato — host C# lare-shell 2.0.0 (modalità B in Windows Terminal), ADR-019, e2e
  Parte 6`).

**Piano 3 — "finestra terminale"** (`Docs/i18n/ita/superpowers/plans/2026-09-07-piano-3-finestra-terminale.md`),
completato il 2026-09-07 — **modalità A** (spec §2.3/§5, ADR-016): `ui.exe` apre di default una
finestra terminale (xterm.js + ConPTY) con `lare-shell.exe` come processo figlio, diventando
l'app che l'utente avvia direttamente — `orchestrator`/`ui` **2.2.0**, `startup-config` 2.0.3,
`lare-shell` 2.0.1. Test (riverificati al Task 7): `cargo test` (default-members) 1481 verdi;
`cargo test -p ui` 141 verdi (92 lib + 49 bin); `node --test crates/ui/frontend/*.test.mjs`
242/242; `dotnet test shell/lare-shell/LareShell.sln` 118/118 (115 alla chiusura del piano 2b + 3
aggiunti da `22dcb09` (review finale piano 2b — `ReplTests.cs`/`ExecutorTests.cs`/
`SlashTurnTests.cs`), PRIMA dell'inizio di questo piano — Task 3 di questo piano modifica
un'asserzione esistente in `LauncherTests.cs`, non aggiunge test propri):

- **Task 0** — scaffold: vendored xterm.js/`addon-fit` dallo spike, dipendenze Rust
  (`portable-pty`/`base64`/`rand`), `build.rs` con `rerun-if-changed`, rimosso il flag di sviluppo
  `--open` (piano 1). Commit `83f9a77`; pre-flight `7dbc143` (Task 0 Step 3 corretto: `build.rs`
  esisteva già).
- **Task 1** — 4 moduli JS puri (`base64.mjs`, `osc-lare.mjs`, `indicators.mjs`,
  `fit-debounce.mjs`, TDD RED→GREEN). Commit `60cf744`.
- **Task 2** — `pty.rs`: comandi `pty_spawn`/`pty_write`/`pty_resize`, `PtyOutputSink` (seam di
  test), `SharedPtyState`. Commit `a5d66f2` (review clean; deviazione isolata ai test — DSR nel
  fake, approvata come correzione legittima).
- **Task 3** — self-heal Rust dell'orchestratore (`launcher.rs`, `ensure_orchestrator`),
  `startup_config::spawn_detached`, flag `--no-terminal` (letto da `ui.exe`, generato da
  `Launcher.cs` — `lare-shell` 2.0.1). Commit `59ba6f4`.
- **Task 4** — la finestra terminale (`terminal.{html,css,js}`, capability
  `terminal-window.json`, costruzione in `.setup()`): xterm.js + ConPTY, consumo dell'OSC 9001
  `intercept` (emesso dalla host dal piano 2b, mai letto da un emulatore prima d'ora), bottone
  "riavvia". Commit `61ae928`; smoke test manuale dal vivo deferito al Task 7 (nessun ambiente
  GUI per i subagenti).
- **Task 5** — consumo di `ActivityIndicator` (emesso dal piano 2a, mai letto da un consumatore
  prima d'ora): relay `host-dispatch.mjs` → evento `terminal:activity`. Commit `651743b`.
- **Task 6** — `orchestrator`: autostart di `ui.exe --no-terminal` quando manca il sink
  (`RuntimeConfig::ui_exe()`, `ensure_ui_sink` in `shell_turn.rs`) — chiude l'autostart reciproco
  completo previsto da §6.4. Commit `3c9cb02`.
- **Task 7** — Documentazione: ADR-020, emendamento allo spec §9 (riga "finestra non disponibile"
  → `NO_UI_ACK`, non un `Error` dedicato) e §6.4 (nota `--no-terminal`), sezione "Modalità A" in
  `RUN-LOCAL.md` (con la sua seconda nota `--open` aggiornata), "Parte 7" in `TESTING-e2e.md`
  (checklist e2e modalità A **ancora da eseguire dal vivo** — nessun accesso GUI durante questo
  task, esplicitamente deferita al controller), versioni 2.2.0/2.2.0/2.0.3, CHANGELOG/
  IMPLEMENTATION di `ui`/`orchestrator`/`startup-config` (chiude anche il gap IMPLEMENTATION.md
  di `ui` lasciato aperto dal Task 3, e lo `[Unreleased]` di `startup-config` lasciato dal Task 3),
  questo aggiornamento di `HANDOFF.md`. Questo commit di release.

- **Fix chiusura finestra (dopo il Task 7, non uno degli 8 task pianificati)** — difetto reale
  trovato dal controller nell'e2e dal vivo condotto DOPO il completamento del piano: chiudere la
  finestra terminale (bottone X) non terminava né `lare-shell.exe` (pty child) né `ui.exe` stesso,
  che restavano entrambi in esecuzione. Corretto: `pty.rs` cattura un `ChildKiller` allo spawn e
  lo usa su `WindowEvent::CloseRequested` (`main.rs`), poi l'intero processo esce
  (`AppHandle::exit(0)`). Nel farlo, scoperto un bug upstream in `portable-pty` 0.9.0 su Windows:
  `WinChildKiller::kill` legge la condizione di successo di `TerminateProcess` al contrario (quella
  API Win32 ritorna non-zero in caso di successo, al contrario della convenzione POSIX che il resto
  del crate segue) — un kill riuscito viene riportato come errore. Aggirato in `pty::kill`
  riconoscendo quel caso specifico come successo (commentato nel codice) — **una futura release di
  `portable-pty` andrebbe verificata per capire se il bug è stato corretto a monte, nel qual caso
  l'aggiramento locale può essere rimosso**. Commit `07c584c`, `ui` 2.2.0 → 2.2.1 (CHANGELOG/
  IMPLEMENTATION di `ui`).

**Da fare prima di considerare il piano 3 chiuso al 100%**: solo poche righe residue della Parte 7
di `TESTING-e2e.md` (`/calc`, `/config`/`/library` e i pulsanti della barra, `/aichat` doppio,
avvio senza autostart, resize) — la parte sostanziale (self-heal, OSC 9001, ActivityIndicator, un
turno `/ai` reale, chiusura finestra, autostart di `ui.exe` dall'orchestratore, bottone "riavvia")
è stata eseguita dal vivo dal controller ed è OK, vedi la nota in testa a quella sezione.

- **Fix finestre console spurie + log su file (dopo il piano 3, richiesta diretta di Maurizio da
  uso reale)** — tre richieste: (1) niente più finestre console spurie né all'avvio (plugin
  sidecar) né al primo uso di un tool (`mcp-server`/`mcp-nmap`/python), né la console di debug di
  `ui.exe` (ora incondizionata via `windows_subsystem = "windows"`, prima solo in release); (2) log
  di `ui.exe` su file (`Configuration/logs/ui.log.<data>`, prima assente — solo `println!`/
  `eprintln!`), stesso modulo `startup_config::logging` condiviso con l'orchestrator (spostato lì
  da `orchestrator/src/logging.rs`); (3) via il thread di debug "digita 'q' per uscire", obsoleto
  dal fix di chiusura finestra del piano 3. `CREATE_NO_WINDOW` su 9 spawn lato orchestrator
  (mcp-server ×6 fattorizzati in un metodo, mcp-nmap, python, plugin sidecar, +2 `taskkill`), il
  loro stderr ora su file invece di `inherit()` verso il nulla. Verificato dal vivo (enumerazione
  UIA **senza filtro per PID** — un filtro sui soli PID `ui`/`orchestrator`/`lare-shell` non
  vedrebbe mai una scheda spuria, che appartiene al PID di `WindowsTerminal.exe`, non a uno dei
  tre; primo tentativo di verifica caduto in questo stesso errore, corretto rieseguendolo):
  confronto "prima"/"dopo" di TUTTE le top-level window, solo `ui.exe` (self-heal, incluso il
  plugin sidecar `ping.exe` già attivo) e il turno `/ai` reale (via `financial-markets`, il tool
  Python già connesso in automatico all'avvio) producono nuove finestre — nessuna. `mcp-server.exe`
  specificamente non è stato innescato dal vivo (gate `[Y/n]` del client di sviluppo instabile in
  automazione non interattiva): stesso identico pattern `CREATE_NO_WINDOW`, verificato via
  code-review + test unitario sugli argomenti del comando (`mcp_server_command`) — punto 13bis
  aggiunto a `TESTING-e2e.md` per chi vuole chiuderlo dal vivo con un turno reale. Test-hygiene:
  due test (`mcp_server_command_...`, `dispatch_accepts_any_injected_tool_name_...`) usavano un
  `config_dir` letterale invece di una tempdir — `child_stderr_log_sink` fa I/O reale alla
  costruzione del `Command`, creava davvero cartelle sul filesystem (`C:\Lare\...`,
  `crates/orchestrator/unused/...`) a ogni `cargo test`; corretto. Versioni startup-config
  2.0.3→2.0.4, orchestrator 2.2.0→2.2.1, ui 2.2.1→2.2.2.

- **`favicon.ico` per `ui.exe`** (chiudeva il DA FARE lasciato dal fix sopra: il subscriber
  `tracing` globale rendeva visibile per la prima volta `ERROR tauri::manager: asset not found:
  favicon.ico`, 3-4 volte per avvio — non una regressione, solo rumore nuovo in un log che
  Maurizio ha chiesto pulito). Generato un `.ico` multi-risoluzione (16/32/48/64/128/256, ~19KB)
  con Pillow (venv usa-e-getta, non nel repo) — casa vittoriana stilizzata con fantasma che
  aleggia sul tetto, su richiesta di Maurizio ("una casa fine '800 americana dove aleggia il
  fantasma molto stilizzato di una figura umana"); due passate per la leggibilità a 16×16 (il
  vincolo duro di un favicon): la prima versione aveva troppi dettagli sottili (luna, comignolo
  stretto, crocette sulle finestre) e contrasto casa/cielo troppo basso, tutto si fondeva in una
  macchia scura — tolti i dettagli sottili, alzato il contrasto, verificato estraendo i singoli
  frame dell'ico e ingrandendoli con nearest-neighbor (non un resize morbido, che avrebbe
  nascosto il problema). File in `crates/ui/frontend/favicon.ico` (root del `frontendDist`, dove
  WebView2 lo richiede implicitamente per ogni pagina) — richiede `cargo clean -p ui` prima del
  rebuild (`generate_context!` incorpora `frontendDist` a compile time, gotcha noto). Verificato
  dal vivo: nessuna riga `asset not found: favicon.ico` in un avvio fresco dopo il fix.

- **Fix codepage OEM in `mcp-nmap` (`network_info.rs`) — recupero da v1 (2026-09-08)**:
  risolto il mojibake delle etichette accentate italiane (`Sì` → `S`, U+FFFD) nell'output dei
  comandi diagnostici Win32 (`ipconfig`, `arp`, `route`, `netstat`, `tracert`) invocati da
  `local_network_info` e `traceroute`. `run_and_capture` decodifica ora stdout/stderr usando
  il codepage reale della console (`GetConsoleOutputCP()`) o, se il processo gira senza console
  allocata (lanciato dall'orchestratore con `CREATE_NO_WINDOW`), il codepage OEM di sistema
  (`GetOEMCP()`), tramite tabella `DECODING_TABLE_CP_MAP` del crate `oem_cp` (`decode_oem`).
  Fallback garantito e deterministico su `String::from_utf8_lossy` per codepage sconosciuti.
  Versione `mcp-nmap` 2.0.0 → 2.0.1.

## DA FARE

- **Idea (Maurizio, 2026-09-12) — evoluzione del canale `/netsec` (ex `/nmap`) verso uno
  strumento di analisi/diagnostica di rete più ampio ("quasi pentesting").** Nata da: Maurizio ha
  chiesto a `/nmap` di verificare tentativi di accesso dall'esterno sulla propria rete
  (`192.168.178.x`) e il canale ha correttamente risposto di non avere gli strumenti per farlo.
  Primo passo (stato FRITZ!Box via `fritzconnection`) è già stato progettato e affidato a un'AI
  esterna nel compito `Docs/i18n/ita/compiti-ai-esterne/2026-09-12-netsec-rename-fritzbox.md`.
  Idee ulteriori, deliberatamente RIMANDATE (non nel compito sopra) — ciascuna aggiungerebbe un
  tool al canale `/netsec`, stesso principio di isolamento (tool fissi, mai una shell arbitraria):
  - **arp-scan / scapy** — scoperta dispositivi sulla LAN a livello 2 (MAC/vendor), utile per
    notare un dispositivo "rogue" mai visto prima sulla rete — complementare a
    `nmap_host_discovery` (che lavora a livello 3/IP) e all'elenco host del FRITZ!Box.
  - **Nikto** — scanner di vulnerabilità web (richiederebbe un binario esterno da installare,
    stesso pattern di dipendenza di `nmap` stesso — vedi `crates/mcp-nmap/src/scan.rs`).
  - **testssl.sh / sslyze** — verifica configurazione TLS di un host (cifrari deboli, certificati
    scaduti/auto-firmati) — utile per controllare i propri servizi esposti.
  - **tshark** — cattura pacchetti mirata (filtri, durata limitata) per diagnosi puntuali;
    più delicato in termini di consenso/privacy degli altri device in rete rispetto ai tool sopra
    — da progettare con più cautela (durata massima, gate di conferma esplicito, forse un
    riepilogo invece del pcap grezzo).
  - **Suricata / Zeek** — IDS/IDS-like vero e proprio: richiederebbe una porta mirror/span sul
    router o switch per vedere il traffico passante, non solo probing attivo come gli altri tool
    — cambio di scala rispetto a tutto il resto del canale (da processo "invocato su richiesta" a
    "servizio sempre attivo che produce eventi"), probabilmente il pezzo più impegnativo e quello
    da valutare per ultimo.
  Nessuno di questi è ancora stato progettato in dettaglio (nessun compito scritto) — da riprendere
  quando arriva il turno di questa idea, verosimilmente uno alla volta seguendo lo stesso schema
  del compito FRITZ!Box (un tool nuovo per volta, con la propria dose di design/revisione).
- **Idea (Maurizio, 2026-09-09) — metodologia guidata per assistere un sistemista verso un
  qualunque strumento/dispositivo esterno (nata da un caso reale: configurazione di uno switch
  di rete).** Non un plugin per UNO strumento specifico, ma un METODO ripetibile che Lare applica
  a QUALUNQUE dispositivo/strumento con cui il tecnico deve interfacciarsi — il soggetto a cui si
  fa sempre capo è il dispositivo stesso, non un'astrazione software. Quattro fasi, le prime
  gestite a quattro mani (tecnico + AI insieme), le ultime sempre più autonome per l'AI:
  1. **Conoscenza/recupero informazioni** — l'AI legge la documentazione DEL dispositivo per
     capire come interagirci. Nel caso reale che ha originato l'idea: le pagine web di gestione
     integrate nello switch stesso (il suo "strumento di configurazione", non un sito esterno) —
     ma il concetto generalizza a manuali PDF, pagine di supporto del produttore, changelog di
     firmware, ecc., a seconda del dispositivo.
  2. **Comunicazione/collegamento** — l'AI suggerisce i passaggi per aprire un canale di
     controllo verso il dispositivo (nel caso reale: come entrare in modalità CLI dalla web UI,
     abilitare SSH, ottenere i comandi "estesi"/enable, poi collegarsi in SSH). Generalizza ad
     altri canali a seconda dello strumento (seriale/console, API REST, SNMP, ecc.).
  3. **Test dei comandi suggeriti, raccolta di quelli VALIDI** — l'AI può proporre comandi da
     documentazione non aggiornata o non pertinente a quel modello/quella versione firmware
     esatta (caso reale: apparecchio vecchio, sintassi CLI cambiata nel tempo); il tecnico
     testa dal vivo e, insieme all'AI, costruisce un elenco di comandi VERIFICATI funzionanti
     per quello specifico dispositivo — non un elenco teorico preso da un manuale generico.
  4. **Interrogazione/richieste operative** — una volta noti i comandi validi per QUEL
     dispositivo, il tecnico esprime una necessità in linguaggio naturale e l'AI o (a) traduce
     in comandi che il tecnico stesso sottomette, o (b) — più avanzato, da valutare con
     attenzione — invia i comandi direttamente allo strumento (Maurizio ipotizza `SendKeys` o
     equivalente: implicazioni di sicurezza reali, un comando sbagliato su un apparecchio di rete
     può interrompere la connettività stessa con cui l'AI ci sta parlando — da trattare con la
     stessa cautela del gate di conferma ADR-007, forse più).
  **Esplicitamente non ancora pronta per un piano**: Maurizio la definisce "ci dobbiamo lavorare
  ancora un po'" — da riprendere in un brainstorming dedicato quando arriva il suo turno. Domande
  aperte da affrontare allora: come si aggancia all'architettura esistente (un plugin nuovo? un
  canale tool esterno come `/nmap`/`/markets`? qualcosa di più simile a `run_routine` ma per
  dispositivi invece che script?); dove/come si conserva la "conoscenza acquisita" per dispositivo
  (i comandi validi della fase 3) fra una sessione e l'altra, così un secondo intervento sullo
  stesso switch non riparte da zero; se e come si generalizza oltre agli switch di rete (altri
  apparecchi con CLI/web UI di gestione — router, firewall, NAS, ecc.); il perimetro esatto della
  fase 4(b) (invio diretto comandi) e le sue conseguenze di sicurezza.
- **Idea (Maurizio, 2026-09-08) — pagina interattiva client-only generata dall'AI per un
  argomento, con refresh via Python.** L'utente chiede una pagina interattiva su un tema;
  istruzioni di base per l'AI: pagina **solo client** (al massimo un DB SQL leggero tipo SQLite
  accanto), mostra i valori richiesti, può usare tutto il parco librerie disponibile; **Python di
  appoggio** per librerie ulteriori e per rileggere i dati a richiesta (pulsante nella pagina) o a
  schedulazione (di sistema o decisa dall'AI, da stabilire), conservandoli da qualche parte (JSON,
  non deciso) così quando l'utente richiede di rivedere la pagina più avanti (anche se nel
  frattempo l'ha chiusa) si riapre nel browser con i dati già noti, non vuota. Distinta dalle
  finestre Markdown/report attuali (quelle sono un colpo solo, questa è pensata per essere
  rivisitata nel tempo). Da brainstormare: come si aggancia all'architettura plugin/canale tool
  esistente, cosa è tecnicamente "la pagina" (documento Library con JS incorporato? finestra
  plugin dedicata?), chi decide la schedulazione, dove vivono i dati persistiti. **Estensione
  (2026-09-08) — un "progetto" come entità di lavoro multi-pagina**: l'utente parte dalla
  descrizione di un PROGETTO, non di una singola pagina — un progetto può comprendere una o più
  pagine web client che insieme formano un'unica "unità di lavoro" (non pagine indipendenti).
  Esempio: un progetto di monitoraggio risorse di rete, con attività in tempo reale E attività
  schedulate nel tempo, su più pagine mostrate su più monitor contemporaneamente (un "muro" di
  dashboard, non una pagina riaperta alla volta). Da decidere anche a livello di progetto (non
  solo di singola pagina): un unico store dati condiviso fra le pagine del progetto; dove/come si
  aprono le pagine di un progetto insieme (posizionamento multi-monitor, ricordato per progetto);
  tempo reale e schedulato come due meccanismi di aggiornamento distinti dichiarati dal progetto.
  **Estensione (2026-09-08, più avanti nella giornata) — descritto da un prompt, consapevole
  degli strumenti, versionato a "release"**: il progetto nasce da un PROMPT; l'AI conosce il
  proprio parco strumenti (pagine web client, SQLite, Python, lo scheduler — "altro?", elenco
  ancora da fare) e deve risolvere il progetto solo con quelli, avvisando esplicitamente
  l'utente di cosa manca se non ci riesce, invece di fare un lavoro parziale in silenzio. Il
  prompt è pensato per crescere nel tempo: in place, oppure come nuova "release" del progetto
  (parola di Maurizio) quando modificare il progetto base è impraticabile/rischioso — a
  richiesta dell'utente o su suggerimento della AI stessa. Meccanismo: l'AI ha il vecchio
  prompt, l'utente aggiunge le nuove caratteristiche desiderate, l'AI combina i due e costruisce
  il nuovo progetto pescando dal vecchio tutto quello che può ancora servire (dati, pagine, job
  schedulati ancora validi), non da zero. **Estensione (2026-09-08, ancora più avanti) — un
  framework UI fra gli strumenti fissi a priori**: fra gli strumenti dell'AI (accanto a web
  client, SQLite, Python, scheduler) anche un framework UI per le pagine, stabilito A PRIORI come
  standard di progetto — non scelto ad hoc di volta in volta. Esempio illustrativo di Maurizio:
  si stabilisce che un framework UI reale esistente (es. "Bookmark") sia LO standard per la UI
  di ogni progetto; le sue caratteristiche (aspetto,
  componenti, convenzioni) vengono definite insieme da utente e AI nel tempo e diventano poi lo
  standard fisso — così ogni progetto condivide la stessa estetica per costruzione, non a caso.
  Da studiare più avanti.
- **Idea (Maurizio, 2026-09-08) — `show_markdown` chiamato più volte in un turno dovrebbe
  aggiornare la STESSA finestra, non aprirne una nuova.** Bug reale trovato dal vivo (piano 3):
  un turno `/ai` lungo con ricerca web può portare il modello a richiamare `show_markdown` più
  volte mentre affina la risposta — oggi (`agent.rs`/`surface.rs`) ogni chiamata apre una
  finestra Markdown NUOVA, indipendente dalle altre (comportamento voluto e testato per un altro
  caso, l'`OpenWindow` di `/help` durante un turno — non va toccato). Riprodotto dal vivo: un
  turno mai completato aveva già aperto 6 finestre quasi identiche. Fix immediato applicato
  (`6e0dc85`): la descrizione del tool ora istruisce esplicitamente il modello a chiamarlo una
  sola volta, solo con la risposta finale pronta — riduce il problema ma non lo elimina (dipende
  dal modello). **Non implementato deliberatamente** (scelta di Maurizio, non un rinvio per
  pigrizia): scartare le chiamate in più butterebbe via un comportamento potenzialmente
  interessante — l'AI che rivede la propria risposta mentre cerca. L'idea da studiare in un
  brainstorming: far sì che una seconda chiamata a `show_markdown` nello STESSO turno aggiorni
  in place il contenuto della finestra già aperta (stesso meccanismo di `OutputWindowContent`/
  `window_id`, oggi usato solo per il testo semplice del turno, non per le finestre aperte
  dall'AI) invece di aprirne una nuova — l'utente vedrebbe la risposta "vivere" mentre l'AI la
  affina, una sola finestra, non N.
- **Idea (Maurizio, 2026-09-07) — più LLM dalla riga di comando: `/ai:<nome>`, gruppi, AI che parlano fra loro.** Oggi `/ai "…"` usa solo il provider `active` di `llms.json` (che però ha già un registro di provider con nome: `claude-direct`, `deepseek-openrouter`, …). Atteso: `/ai:Claude-sonnet "…"`, `/ai:Gemini "…"`, `/ai:Kimi "…"` per rivolgersi a una AI specifica configurata; **gruppi con nome** (es. `Coders` = quelle tre) e `/ai:Coders "…"` che interroga tutte in un colpo solo; poi, la parte più interessante, **le AI che interagiscono fra loro** sul modello di `/aichat` (chat fra due macchine Lare, ciascuna con una AI diversa). In parte esiste, andrà "aggiustato": sintassi nel pre-router della shell (`shell_slash.rs`), gruppi in `llms.json`, fan-out e aggregazione delle risposte nella finestra di output. Da studiare più avanti (dopo il piano 3).
- **Idea (Maurizio, 2026-09-07) — interazione dell'AI "da tastiera" e descrittore di form.** La metodologia usata per l'e2e del piano 2b (tasti via `SendKeys`, screenshot letti come immagine, UI Automation per finestre e schede — `scripts/dev/e2e-driver/`, README con le lezioni) va conservata e fatta diventare un plugin o un metodo interno di interazione dell'AI dentro Lare Terminal. Estensione ancora embrionale: un **modello descrittore di form** (forma da definire) per pagine web, che faccia da "traccia" all'AI: l'utente chiede, l'AI apre la pagina e, seguendo il descrittore, inserisce i valori ricevuti. Da brainstormare quando arriva il suo turno (dopo il piano 3).
- **E2E manuale dal vivo, modalità A (piano 3)**: `TESTING-e2e.md` Parte 7 — la parte sostanziale
  eseguita dal vivo dal controller dopo il piano (self-heal, OSC 9001, ActivityIndicator, un turno
  `/ai` reale, chiusura finestra, autostart di `ui.exe` dall'orchestratore, bottone "riavvia", tutti
  OK — ha anche trovato e fatto correggere due difetti reali: chiusura finestra senza terminare i
  processi, poi bottone "riavvia" invisibile per uno `z-index` mancante). Restano da eseguire solo
  righe minori: `/calc`, `/config`/`/library` e i pulsanti della barra, `/aichat` doppio, avvio
  senza autostart, resize.
- **Streaming token-per-token nella finestra di output** (spec §12, fuori MVP finora): il
  contenuto arriva tutto insieme a `Done`/`Error` da sempre (piano 2a) — il piano 3 lo esclude
  esplicitamente dal proprio scope (Global Constraints), non è quindi legato a un piano
  particolare. Da studiare quando/se emerge come esigenza reale.
- **Chore separata**: `cargo fmt` globale sul codice copiato dalla v1 (non fmt-clean), fuori dai
  piani per non sporcare i diff di review.
- **`ensure_ui_sink` (orchestrator, Task 6) non ha una guardia `IsRunning` come il suo specchio C#
  `Launcher.EnsureUi`** (`shell/lare-shell/src/LareShell/Shell/Launcher.cs`): decide se autostartare
  `ui.exe` solo da "esiste un sink `ui` registrato ORA nel `Registry`", non da "esiste già un
  processo `ui.exe` vivo" — `Launcher.EnsureUi` controlla invece `_starter.IsRunning(UiExe)` prima
  di avviarlo. Innesco concreto: se l'orchestratore stesso riparte, il client WS di `ui.exe`
  (`crates/ui/frontend/ws-client.js`) si riconnette con un backoff esponenziale che arriva fino a
  8s (`RETRY_MAX_MS`); un qualunque turno shell che nel frattempo ha bisogno di una finestra vede
  "nessun sink registrato" e fa partire un SECONDO `ui.exe --no-terminal`. Quella seconda istanza
  non ha via d'uscita oggi: senza finestra terminale non c'è handler di chiusura, e il thread
  debug-only che legge "q" da stdin non esiste in una build release — resta viva finché non viene
  uccisa a mano. Decisione deliberata di questa review: non correggerlo ora, solo documentarlo con
  precisione perché non resti solo nella memoria dei partecipanti.

- **E2E con AI reale fatto il 2026-09-07** (`llms.json` copiato dal deploy v1 in `Test Run\Configuration\`, gitignored): punti 5-9 di `TESTING-e2e.md` Parte 6 tutti OK (tre gate in sequenza, exec nel runspace, `cd` persistente, rifiuto, Ctrl+C in attesa e durante l'exec, `python` interattivo). Trovato e protetto un caso di gate accettato da tasti pendenti (svuotamento del buffer prima del prompt + log): vedi `KNOWN-ISSUES.md`.

### Debiti / decisioni del piano 2a

Deliberati durante l'implementazione (revisione advisor + review del controller), da tenere
presenti nel piano 2b/3:

- `/find` e `/nowin` dalla shell sono **scartati** (come uno slash ignoto) — `/find` vive in
  `ws.rs` fuori dal ciclo di un turno, `/nowin` non ha senso ora che l'output è sempre in
  finestra. Emendamento allo spec (§3, Task 11).
- `/help`/`/show` (`core::WINDOW_SLASHES`) **non** aprono la finestra di output col segnaposto —
  il loro esito È già una finestra, altrimenti se ne aprirebbero due. Emendamento allo spec
  (§3.2, Task 11).
- `web_search` per un turno shell = `Command.web_search` **oppure** `config.json.
  web_search_enabled` — la shell non ha una propria casella, vale la scelta fatta in `/config`.
- `/ping` misura `plugin-ping` con una sonda usa-e-getta Init→Ready (mai l'istanza eager già
  viva) e `ui.exe` con `UiPing`/`UiPong` (timeout 2 s); non è cancellabile a metà (durata
  limitata, ≈ 7 s nel caso peggiore).
- `Hello.version` è stato aggiunto nel codice (Task 1) prima che lo spec §4.1 lo documentasse —
  emendamento allo spec in questo task, non una deviazione.
- **La cwd della sessione non entra nel prompt di sistema dell'AI** (spec §4.6 lo chiede): gap
  preesistente della v1, non introdotto da questo piano — `grep cwd ai_adapter.rs` trova solo
  test, l'adapter non ha mai ricevuto la cwd, quindi né la `ui` né la shell gliela passano oggi.
  Da risolvere in un task dedicato del piano 2b/3, con test propri.
- Il router consegna **un solo messaggio terminale per turno** alla shell (`surface.rs`): un
  `Done` che arrivasse dopo un `Error` già inoltrato viene scartato. Contratto vincolante per la
  host del piano 2b — "il turno finisce al primo terminale".
- `ActivityIndicator` è emesso da questo piano ma non ha ancora un consumatore — arriva nel
  piano 3 (segnalini nella finestra terminale).
- Lo streaming token-per-token nella finestra di output resta fuori MVP (spec §12) — il
  contenuto arriva tutto insieme a `Done`/`Error`.
- `emitToPlugin` (`host.js`) è riusato come emettitore di eventi Tauri generico anche per il
  canale shell (`output:content`) — nome storico, non specifico ai plugin; non rinominato per non
  allargare il diff.
- Test end-to-end del gate di conferma con un'AI **reale** (non lo `StubAdapter`) restano solo
  `--ignored` (richiedono una chiave API) — invariato rispetto alla v1.
- Markdown della finestra di output: i chunk di trasparenza e il testo AI sono concatenati senza
  separatore — cosmetico, vedi `KNOWN-ISSUES.md`.
- **Contratti per la host (piano 2b)**, consolidati dalla review finale del piano 2a (dettaglio
  completo in `crates/protocol/IMPLEMENTATION.md` §"Contratti per la host"): (a) un solo turno AI
  alla volta per connessione — la history è sotto lock per tutto il turno, gate incluso: un
  secondo `/ai` apre la finestra e resta bloccato senza errore, in coda; (b) `Command.id` deve
  essere unico per connessione — un duplicato sovrascrive il cancel token e riusa la finestra di
  output del primo turno con quell'id; (c) il turno finisce al primo `Done`/`Error` — la host
  deve scartare i messaggi dei turni già chiusi, l'orchestratore non ne manda comunque un secondo
  per la via normale; (d) `ExecResult.turn_id` deve corrispondere al `turn_id` ricevuto
  nell'`ExecInShell` a cui si risponde, altrimenti viene scartato con un warn (fix M8) senza
  consumare l'esecuzione pendente.

### Debiti / decisioni del piano 2b

Deliberati durante l'implementazione (revisione advisor + review del controller + e2e dal vivo);
dettaglio completo nel piano (`Docs/i18n/ita/superpowers/plans/2026-09-06-piano-2b-host-lare-shell.md`
§"Ruling presi in questo piano") e in `shell/lare-shell/IMPLEMENTATION.md`.

**Ruling 1-9 del piano** (una riga ciascuno; 4 rivisto durante l'e2e, vedi sotto):

1. Server finto dei test (`FakeOrchestrator`) su `TcpListener` + upgrade WS a mano, mai
   `HttpListener` (registrazioni http.sys che sopravvivono a un test caduto a metà).
2. Riconnessione **on demand**: nessun task in background — ogni `/…` verifica e ristabilisce la
   connessione (con autostart, finestra 5 s) se serve.
3. `exit_code` = 0 se `$?` (catturato in coda allo stesso script) è vero, altrimenti
   `$LASTEXITCODE` se ≠ 0, altrimenti 1; i comandi digitati dall'utente non toccano `$?`/
   `$LASTEXITCODE` (il prompt deve vederli intatti).
4. Processi figli con `UseShellExecute=true`; **entrambi** (`orchestrator.exe` e `ui.exe`) con
   finestra nascosta — **rivisto durante l'e2e**: la formulazione iniziale lasciava `ui.exe`
   `WindowStyle.Normal`, ma in build debug `ui.exe` è un'app console e, con Windows Terminal come
   terminale predefinito, apriva una scheda WT rubando il fuoco alla shell (fix `a8d148f`).
5. `ToolConfirmRequest` senza `turn_id`: attribuita al turno corrente durante l'attesa; i messaggi
   di turni già chiusi scartati a inizio turno.
6. Cap dell'output in **caratteri** (200·1024), non byte.
7. Log della host `<config-dir>\logs\lare-shell.log` senza rotazione (debito: la spec chiederebbe
   rotazione giornaliera).
8. `ui.exe` avviata dopo la prima connessione riuscita e **ricontrollata prima di ogni turno
   slash** (self-heal: se l'utente l'ha chiusa, il prossimo `/…` la riapre).
9. `deploy_test_run.ps1` pubblica la host di default (framework-dependent win-x64); `-SkipShell`
   per saltarla.

**Difetti del piano trovati ed emendati durante l'esecuzione** (dettaglio nei `task-N-report.md`):

- Catch troppo stretti in `StartupConfig.Load`/`TokenFile.Read`: `UnauthorizedAccessException`
  sfuggiva invece di degradare a default/`null` (T0).
- `pong.ts` doveva essere obbligatorio; i campi numerici malformati di `done`/`pong` sfuggivano
  come eccezioni .NET grezze invece di un `WireException` uniforme (T1).
- Socket non rilasciato su `OperationCanceledException` dentro `ConnectAsync`; `Connect` su una
  connessione già viva accodava un `Disconnected` spurio invece di essere un no-op (T2).
- `<NoWarn>CA1416</NoWarn>` (fuori lista del piano) sostituito con `SupportedOSPlatform=windows`
  via un item `AssemblyAttribute` nel `.csproj` — una property `<SupportedOSPlatform>` piatta non
  genera l'attributo che serve (T3).
- **Senza `<RuntimeIdentifier>` gli asset RID-specifici del SDK (`System.Management.Automation.dll`
  compreso) finiscono sotto `runtimes\win\lib\net10.0\` → `$PSHOME` risolve lì invece che nella
  cartella dell'exe → `powershell.config.json` accanto all'exe non viene letto** (7 test rossi su
  11 finché non scoperto): aggiunto `<RuntimeIdentifier>win-x64</RuntimeIdentifier>` +
  `<SelfContained>false</SelfContained>` in ENTRAMBI i `.csproj` — coerente col publish del deploy,
  output di build sotto `bin\Debug\net10.0\win-x64\` (T4).
- `PowerShell.Stop()` su Microsoft.PowerShell.SDK 7.6.5 NON lancia `PipelineStoppedException`:
  `Invoke()` torna normalmente con `InvocationStateInfo.State == Stopped` (T4; il catch
  dell'eccezione resta comunque, per altri SDK/percorsi).
- Exit code catturato con `try { <cmd> } finally { $global:__lare_ok = $? }`, non un append
  diretto (un `return` di primo livello lo salterebbe) né un blocco `. { }`/`& { }` (azzera sempre
  `$?` al proprio confine, qualunque cosa sia successa dentro) — verificato empiricamente in due
  iterazioni di fix (T4).
- Test della guardia `Done.Id` rafforzato (un `Done` di un altro turno non deve chiudere quello
  corrente); `ConsoleGate` con stdin rediretto ora fallisce **chiuso** su EOF (rifiuta, non
  accetta); eccezione dell'executor durante un turno → `CancelCommand` invece di un turno appeso
  da entrambe le parti (T5).
- Handler di Ctrl+C mai lancia: niente `using` sul `CancellationTokenSource` del turno — evita la
  race con la fine naturale del turno (`ObjectDisposedException` nel thread dell'handler avrebbe
  altrimenti terminato il processo, violando "Ctrl+C non esce mai dalla shell") (T7).
- `ui.exe` avviata con console nascosta anche lei, non solo `orchestrator.exe` — scoperto
  dal vivo durante l'e2e (T8/e2e, vedi ruling 4 sopra).

**Limiti noti** (dettaglio in `KNOWN-ISSUES.md` §"Debiti nativi della host `lare-shell`" e
`shell/lare-shell/IMPLEMENTATION.md` §Debiti): profili AllUsers non caricati; log della host senza
rotazione; `$?` nel prompt resta quello dell'ultimo comando digitato dall'utente, mai un riflesso
di un turno slash appena concluso; `exit` dentro un comando dell'AI termina il processo come un
`exit` digitato (il gate ha già mostrato il comando); `StopCurrent()` ha una finestra TOCTOU (un
Ctrl+C fra l'assegnazione di `_current` e `Invoke()` è un no-op silenzioso, nessun crash);
`TerminalPending` guarda solo la testa della coda dei messaggi in arrivo (`TryPeek`) — per un
turno gateizzato questo basta: l'ack `Chunk` che precede il `Done` conta come terminale anch'esso
(fix F3, revisione finale), perché `route_shell_turn` non manda mai altro testo alla shell nel
mezzo; `ToolConfirmRequest`
senza `turn_id`; il gate con stdin rediretto blocca dentro `Console.ReadLine`; **`ConsoleGate` e
`Repl` non hanno test automatici** (richiedono una console interattiva vera — coperti solo
dall'e2e manuale, `TESTING-e2e.md` Parte 6); Ctrl+C su un comando digitato è verificato dal vivo in
Windows Terminal (log `Ctrl+C ricevuto` + riga `"[LARE] comando interrotto (Ctrl+C)."`): il caso
"riga mancante" delle prime passate era il driver dell'e2e in `conhost`, non la host; `#pragma warning
disable xUnit1031` nei test sincroni di `SlashTurn` (bloccano di proposito, come il thread REPL
vero); il fragment del profilo Windows Terminal (`install-wt-profile.ps1`) è letto solo all'avvio
di WT — installarlo/aggiornarlo richiede di riavviarlo, non basta una nuova scheda; `ui.exe` in
build debug è un'app console (spiega perché va nascosta anche lei, non un bug di Lare).

### Debiti noti del piano 1

Minori, rimandati deliberatamente (non bloccano il piano 1, da valutare/chiudere nei piani
successivi):

- 12 `Cargo.toml` riscritti CRLF→LF e voci CHANGELOG con endings misti.
- `RuntimeConfig::{mcp_server_exe,mcp_nmap_exe,pytools_dir}` non usati dai resolver (formula
  duplicata inline).
- Test `generate_token_is_random` rimosso senza sostituto.
- `scripts/pytools/README.md` riga ~5 "convenzione invariata" (falso) e riga ~27 etichetta
  interna "Task 4/5".
- 3 costruzioni `LareWsClient` senza guardia su `url` vuota (`config-dialog.js`,
  `external-channel-window.js`, `window.js`) — `host.js` ha la guardia.
- `capabilities/default.json` concede `core:window:allow-set-size`/`allow-start-dragging`
  inutilizzati.
- `diagnose_connection` registrato senza chiamante JS.
- `library.js` invoca `open_saved_find_window` direttamente (pre-esistente).
- `Test Run/Configuration/README.md` dice "a ogni avvio" per network/search json (solo se
  assenti/corrotti).
- Commenti nel frontend copiato dalla v1 citano percorsi `Docs/superpowers/...` che nel 2.0
  non esistono (la documentazione è sotto `Docs/i18n/`).
- `ui.exe --open` accetta solo la forma con spazio (`--open config`), non `--open=config`.
- `ui.log` su file non implementato (`ui.exe` logga solo su stdout).

**Nota per il piano 2 — risolta nel piano 2a**: `host.js` `openAiChatWindow` non aveva chiamanti
finché `/aichat` non arrivava via l'orchestratore. `shell_slash::UI_LOCAL_SLASHES` include
`aichat` fin dal Task 6: `OpenUiLocal{name:"aichat"}` lo raggiunge già.
