# Changelog — crates/ui

All notable changes to this package follow [Keep a Changelog](https://keepachangelog.com/) format.
Versioning: `major.minor.update`.

---

## 2.0.3 — 2026-09-05 — fix wave della review finale (piano 1)

Solo pulizia, nessun cambio di comportamento:

- Frontend: ~37 commenti in 11 file citavano ancora `app.js` (cancellato in Task 7,
  v2.0.2) — sostituiti con `host.js`, il suo successore. Rimossi anche i riferimenti
  morti a `list_path_completions`/`line-editor.js` in `path-utils.js`/`search-status.js`.
- `config_dir.rs`: `token_path()` usa ora `startup_config::TOKEN_FILE_NAME` invece
  della stringa letterale `"token"` duplicata con `orchestrator::token_store`.
- Beneficia anche dell'assolutizzazione di `--config-dir` spostata nel crate condiviso
  `startup-config` (v2.0.2 di quel crate): `ConfigDirState::from_process()` ora riceve
  sempre un `config_dir` assoluto, anche con un `--config-dir` relativo sulla riga di
  comando (prima solo l'orchestrator se ne curava).

## 2.0.2 — 2026-09-05 — overlay F2 rimosso, pagina host nascosta, flag --open (dev)

Task 7 del piano `2026-09-05-piano-1-fondamenta`. In 2.0 la shell non gira più dentro
`ui.exe` (spec §5): l'overlay F2 — editor a riga singola, output scrollabile, spinner di
attività, pulcino idle, diagnosi automatica — non ha più ragione d'essere. `ui.exe` diventa
un puro host di finestre.

- **`app.js` → `host.js` + `host.html`**: estratte (invariate salvo i riferimenti al DOM del
  cursore) la connessione WS di default, `handleServerMsg` (solo i tipi classificati
  `window`/`relay` da `host-dispatch.mjs`), ogni `open*Window`/`emitTo*`/`setup*Events` e
  `bootstrap`. `host.html` è la nuova finestra `main`: nascosta (`visible:false`), 200×100,
  `skipTaskbar:true`, non trasparente, senza alcuna UI propria.
- **`host-dispatch.mjs`** (nuovo, TDD): `classifyServerMsg(msg)` — logica pura, nessun DOM —
  classifica ogni `ServerMsg` in `"window"` (apre/aggiorna una finestra), `"relay"` (va
  rigirato a una finestra già aperta), `"ignored"` (era per il cursore v1, che non esiste
  più) o `"deny"` (`tool_confirm_request`: senza cursore nessuno può rispondere, default
  sicuro NO). `handleServerMsg` in `host.js` la usa per decidere se entrare nello switch.
  `host-dispatch.test.mjs`: RED (modulo assente) → GREEN.
- **Rimosso**: plugin `tauri-plugin-global-shortcut` (e il suo handler in `.setup()`),
  `tauri-plugin-clipboard-manager` (usato solo da `app.js`), i comandi Tauri
  `hide_overlay`/`show_overlay`/`list_path_completions`/`home_dir`, `apply_hotkey`,
  `window.rs` (positioning dell'overlay — dipendeva solo dall'enum `Position`, sparito con
  lui), `index.html`, `app.js`, `line-editor.js`, `idle-duck.js`, `page-scroll.js`,
  `cwd-format.js`, `connection-diagnosis.js` e i rispettivi test.
- **`Config`** (`config.rs`): via `action_key`, `cursor_color`, `cursor_font`, `cursor_size`,
  `position`, `activity_indicator`, `idle_duck_minutes` e gli enum `Position`/
  `ActivityIndicator` che li tipavano — restano `web_search_enabled` e `window_alpha`.
  Nessun `#[serde(deny_unknown_fields)]`: un `config.json` v1 con i campi rimossi continua a
  caricare (`legacy_v1_config_json_with_extra_fields_still_loads`). `set_config` non valida
  più un tasto d'attivazione né ri-registra un hotkey — solo persist + stato in memoria.
  `/config` (`config-dialog.js`) mantiene solo le tab Ricerca web e Trasparenza; corretto
  anche il testo di aiuto della tab LLM (`llms.json` vive nella cartella di configurazione,
  non più in `%LOCALAPPDATA%\dev.lare.terminal`/`LARE_LOCAL_DIR`, riferimenti 2.0 non
  aggiornati da un edit precedente).
- **`--open config|library`** (dev-only, `DevOpenRequest`): senza overlay né canale shell
  nessuna superficie può ancora chiedere l'apertura di una finestra da sola — questo flag la
  apre subito all'avvio, per verificare le finestre a mano durante lo sviluppo. Rimosso nel
  piano 3, quando l'orchestratore guiderà l'apertura delle finestre.
- **`tauri.conf.json`**/**`capabilities/default.json`**/**`Cargo.toml`**: finestra `main` →
  `host.html`, non trasparente, non always-on-top, 200×100; tolte le permission
  `global-shortcut:*`/`clipboard-manager:*`; tolte le dipendenze
  `tauri-plugin-global-shortcut`/`tauri-plugin-clipboard-manager`/`dirs` (quest'ultima era
  usata solo da `home_dir`, anch'esso rimosso).

## 2.0.1 — 2026-09-05 — `--config-dir`, `startup.json`, un solo risolutore, nessuna env var

Task 6 del piano `2026-09-05-piano-1-fondamenta`. Il crate `ui` (fork v1) leggeva
`LARE_TOKEN`/`LARE_LOCAL_DIR`/`LARE_ROAMING_DIR`/`LARE_PLUGINS_DIR`/`LOCALAPPDATA` in SEI punti
indipendenti (`main.rs::read_lare_token`/`config_file_path`/`library_dir_path`/diagnostica
`.setup()`, più `search_settings.rs`, `plugins_view.rs`, `market_data_settings.rs`,
`llm_settings.rs`, `aichat_settings.rs`), e usava `app_local_data_dir()`/`app_config_dir()` di
Tauri — nessuno di questi risolutori era garantito concordare con l'orchestrator (Task 4) o col
crate `startup-config` (Task 2). Migrazione completa alla stessa regola (decisione D6):

- **`ConfigDirState`** (nuovo modulo `config_dir.rs`, sul modello di
  `orchestrator::RuntimeConfig`): stato gestito da Tauri (`app.manage`) con `config_dir`
  (risolto una volta in `main()` da `startup_config::config_dir_from_process()`) e `startup`
  (`StartupConfig::load`). Funzioni pure `token_path`/`config_file_path`/`library_dir_path`/
  `find_dir_path` (tutte `<config_dir>/...`), `read_token` (legge, NON genera — è
  l'orchestrator/`token_store::resolve_token` a crearlo al primo avvio), e i metodi
  `ws_endpoint()`/`plugins_dir()` che incapsulano `StartupConfig::resolve_path`.
- **Nuovo comando Tauri `get_ws_endpoint`**: `"ws://127.0.0.1:<ws_port>"` da `startup.json`.
  `ws-client.js` non ha più una costante `WS_URL` hardcoded a `7331` — `LareWsClient` richiede
  ora un parametro `url` obbligatorio nel costruttore; i 4 punti che lo costruiscono (`app.js`,
  `config-dialog.js`, `external-channel-window.js`, `window.js`) lo ottengono con
  `await invoke("get_ws_endpoint")` prima di aprire la connessione.
- **`read_lare_token`/`get_lare_token`**: delegano a `config_dir::read_token(&state.config_dir)`
  — via `app.state::<ConfigDirState>()` — eliminato l'ordine di precedenza
  `LARE_TOKEN`→`app_local_data_dir()`/`LARE_LOCAL_DIR`. `config_file_path(app)`/
  `library_dir_path(app)` mantengono la firma `Result<PathBuf, String>` (molti chiamanti usano
  `?`) ma il corpo ora è infallibile, delega a `config_dir::*`.
  `diagnose_connection` controlla la porta configurata (`state.startup.ws_port`), non più
  `7331` hardcoded — con `ws_port` personalizzato, il vecchio codice avrebbe controllato la
  porta sbagliata e mentito all'utente sullo stato dell'orchestrator.
- **5 moduli settings** (`search_settings`, `market_data_settings`, `llm_settings`,
  `aichat_settings`): ogni risolutore locale (`search_paths_json_path`/`market_data_json_path`/
  `llms_json_path`/`network_json_path`+`legacy_aichat_json_path`) diventa una funzione pura
  `fn(config_dir: &Path) -> PathBuf`; i comandi Tauri corrispondenti guadagnano un parametro
  `state: State<'_, ConfigDirState>`. `plugins_view::plugins_dir()` è stata rimossa: il comando
  `list_plugins` usa `state.plugins_dir()` — sparisce anche il caso speciale
  `LARE_PLUGINS_DIR` (v1: nessun trim/empty-check, un'incoerenza mai più necessaria con un
  solo risolutore).
- **Diagnostica `.setup()`**: il doppio print "Local data dir"/"Roaming data dir" (due
  etichette per un'unica cartella, con l'unificazione di questo task) è sparito — `config dir`
  resta stampato una volta in `main()`, prima del `Builder`.

Gate di verifica: `grep -rn 'env::var("LARE_\|LOCALAPPDATA\|APPDATA\|app_local_data_dir\|app_config_dir' src`
vuoto (anche nei commenti — scrupolo esteso a `config.rs`/`archive.rs`, che citavano la vecchia
risoluzione solo in un commento, mai nella logica: `Config`/l'archivio restano invariati, il
percorso arriva sempre da `main.rs` come `&Path` iniettabile). `cargo test -p ui`: 153/153
(100 lib + 53 bin, `config_dir` compreso, TDD RED→GREEN). `node --test frontend/*.test.mjs`:
269/269. `cargo clippy -p ui --all-targets`: un solo warning, preesistente e indipendente da
questo task (`open_routine_preview`, `too_many_arguments`, 9 parametri — nessun nuovo warning
nei file toccati).

Fuori scope (segnalato, non toccato): `connection-diagnosis.js`/`connection-diagnosis.test.mjs`
mostrano ancora testo di guida `$env:LARE_TOKEN = "..."` e l'etichetta `"porta TCP 7331"` — copy
utente, non un risolutore duplicato; `app.js` ha ancora un `console.error` che cita
`LARE_TOKEN=<token>`. Nessuno di questi file è nell'elenco file del brief; li lascia un
follow-up dedicato (impatto pratico nullo col default `ws_port=7331`, fuorviante solo con una
porta personalizzata in `startup.json`).

## 2.0.0 — 2026-09-05 — fork da v1 0.47.1

Copia del crate dalla v1 (`mauriziolobello/lare-terminal`) nel repo 2.0. Nessuna modifica
funzionale in questa voce; le modifiche del piano 1 seguono nelle voci successive.

## [0.47.1] — 2026-08-14 — Fix review finale whole-branch: asimmetria lettura/scrittura `market_data.json` + nota UI sbagliata

Piano `2026-08-14-financial-markets-ibkr-data-source`, fix wave post-review
(`.superpowers/sdd/2026-08-14-financial-markets-ibkr-data-source/review-fix-wave-brief.md`,
Fix B — CRITICO, e Fix G).

### Fixed

- **Fix B (CRITICO) — `market_data_settings.rs::merge_active` bloccava
  QUALUNQUE salvataggio di QUALUNQUE tab `/config`, non solo Dati Mercato**:
  `get_market_data_settings()`/`parse_market_data_settings` sintetizzano un
  default `sources:[{kind:"yfinance"}]` in LETTURA quando il file non ha una
  chiave `"sources"` valida (es. `{"active":"yfinance"}` scritto a mano, o
  creato da un'altra feature). Ma `merge_active` (usata da
  `set_market_data_settings`, chiamata INCONDIZIONATAMENTE a ogni Save di
  `/config` — il tab Dati Mercato mostra sempre almeno la radio YFinance
  sintetizzata) controllava il JSON GREZZO su disco, non trovava `sources`,
  e falliva con `"Fonte 'yfinance' non trovata in market_data.json"` — un
  errore fuorviante che bloccava il Save di QUALUNQUE tab (AI Chat, LLM,
  ecc.), non solo Dati Mercato, ogni volta che `market_data.json` era in
  questo stato (esattamente lo stato "mai toccato il tab Dati Mercato").
  Fix: `merge_active` ora sintetizza LO STESSO default in scrittura, quando
  `active == "yfinance"` E `sources` non è un array valido/non-vuoto — stesso
  principio già applicato al ramo file-assente, ora anche al ramo
  file-presente-senza-sources. Per qualunque altro `active` (tws/ib_gateway)
  senza `sources[]` corrispondente, l'errore resta corretto (comportamento
  voluto, non toccato).
- **Fix G — nota fuorviante nel tab Dati Mercato**: "Le modifiche alla fonte
  dati mercato richiedono il riavvio dell'orchestrator" era sbagliata (e lo
  era già nell'entry 0.47.0 sopra, mai corretta prima d'ora). A differenza di
  `llms.json`/`aichat.json` (letti una sola volta all'avvio, cache in stato
  Rust), `market_data.json` è letto FRESCO a ogni nuovo processo Python
  (`market_data_config.build_data_source`, import di `server.py` — un nuovo
  processo per il canale `/markets` o per ogni click di "Test connessione",
  mai un riavvio dell'orchestrator). Testo corretto in
  `config-dialog.js::_buildMarketDataTab`.

### Tests

- `merge_active_synthesizes_default_sources_when_file_exists_without_sources_key`
  — RED prima del fix (`Fonte 'yfinance' non trovata`), GREEN dopo.
- `merge_active_still_rejects_non_yfinance_active_without_sources_key` —
  non-regressione: `active="tws"` senza `sources[]` continua a fallire.
- Fix G è testo statico, nessun test node:test dedicato (stessa convenzione
  delle altre note del dialog, nessuna ha un test).

**Correzione trovata in una seconda self-review (advisor), stesso giorno**:
la prima versione del fix copriva solo l'esempio letterale del brief
(chiave `"sources"` ASSENTE). Un array `"sources"` NON VUOTO ma con voci
prive di `"kind"` (es. `{"sources":[{"port":4001}]}` scritto a mano) supera
il controllo "l'array non è vuoto" e salta la sintesi — ma nessuna voce ha
`"kind":"yfinance"`, quindi lo stesso errore fuorviante si ripresenta.
`merge_active` ora sintetizza in base a `kind_exists` (non alla vacuità
dell'array) e AGGIUNGE una voce `{"kind":"yfinance"}` invece di sostituire
l'array (le voci preesistenti, es. `port`/`client_id`, non spariscono).
Nuovo test:
`merge_active_synthesizes_default_when_sources_entries_have_no_usable_kind`
— RED verificato (`Fonte 'yfinance' non trovata`) prima del fix, GREEN dopo,
verifica anche che la voce preesistente con `port` sopravviva.

`cargo test -p ui`: 50 passed; 0 failed (13 in `market_data_settings::tests`
— vedi `review-fix-wave-report.md` per i numeri completi).

---

## [0.47.0] — 2026-08-14 — `/config`: tab "Dati Mercato" + bottone Test connessione

Debito documentale sanato in un solo commit: i due commit `feat(ui)` che seguono
(Task 2 e Task 12 di `Docs/superpowers/plans/2026-08-14-financial-markets-ibkr-data-source.md`)
avevano toccato codice senza aggiornare CHANGELOG/IMPLEMENTATION/versione nello
stesso commit, in violazione della convenzione di progetto — corretto qui invece
di aggiungere una terza voce frammentata.

- **Tab "Dati Mercato"** (`config-dialog.js`, `_buildMarketDataTab`): sola
  selezione della fonte dati mercato attiva (YFinance/TWS/IB Gateway), radio
  list mirror del tab "LLM" esistente, letta/scritta da `market_data.json` via
  i comandi Tauri `get_/set_market_data_settings` (già esistenti). Nota
  informativa: richiede il riavvio dell'orchestrator (non live), stesso limite
  già noto per i tab AI Chat/LLM.
- **Bottone "Test connessione (fonte salvata)"**: apre una `LareWsClient`
  dedicata usa-e-getta (`channel: "config-market-data-test"`, mirror letterale
  del pattern one-shot già in uso per `runExpand()`/"library-expand" in
  `window.js`), invia `ClientMsg::TestMarketDataSource` e mostra
  `ok`/`message` da `ServerMsg::MarketDataSourceTestResult` — testa sempre la
  fonte ATTUALMENTE SALVATA su `market_data.json`, non la radio appena
  cliccata e non ancora salvata (vincolo del wire protocol, non negoziabile
  lato frontend). Nuovo `ws-client.js::sendTestMarketDataSource(id)` (mirror
  minimo di `sendCommand`). Nuovo `ws-client.test.mjs` (2 test di wiring puro
  sulla serializzazione del messaggio).
- Nessuna modifica lato Rust: `ClientMsg::TestMarketDataSource`/`ws.rs` erano
  già completi (protocol 0.15.4 · orchestrator 0.41.18, Task 10/11).

Suite `node --test crates/ui/frontend/*.test.mjs`: 269/269 verdi. `cargo build
-p ui` pulito. Smoke test grafico dal vivo (click bottone, disabilitazione/
riabilitazione, esito a schermo) **ancora da accettare dal supervisore** —
nessun ambiente Tauri disponibile ai subagenti che hanno costruito questi due
task, stesso trattamento già riservato ad altri tab `/config` in questo piano.

## [0.46.8] — 2026-08-13 — AI Chat: nickname affiancato all'etichetta macchina (disambiguazione)

Feedback utente dal primo smoke test dal vivo multi-macchina (rumpleteazer↔skimble,
Task 9/v0.46.7 appena mergiato): col solo `display_name` in chat, due macchine con
lo stesso nickname (stesso umano collegato da due PC, o due AI col medesimo nome —
visto in diretta: entrambe le AI si erano chiamate "Clio") diventano indistinguibili
nella lista messaggi.

- `aichat-view.mjs`: `messageLine` ora compone `label` come `"<display_name> /
  <etichetta-macchina>"` (es. `"Clio / skimble"`, `"Maurizio / rumpleteazer"`)
  quando `display_name` è presente — nuova funzione interna `machineLabel(from_label)`
  spoglia il suffisso `-human`/`-ai` da `from_label` per isolare il nome macchina.
  Senza `display_name`, fallback invariato su `from_label` grezzo (già
  disambiguante da solo, nessuna modifica al comportamento storico/compat).
  `is_ai` resta un badge visivo separato, mai concatenato al testo.
- 3 test `node:test` aggiornati/aggiunti in `aichat-view.test.mjs` (uno riscritto
  per il nuovo formato, due nuovi: badge AI col formato combinato, disambiguazione
  esplicita tra due macchine con lo stesso nickname).

Nessun cambio lato `orchestrator`/wire: solo rendering frontend. Verificato dal
vivo su rumpleteazer↔skimble dopo il fix.

---

## [0.46.7] — 2026-08-13 — AI Chat: nickname/badge nel rendering (Task 9, ultimo del piano)

Nickname umano + AI (design `Docs/superpowers/specs/2026-08-13-aichat-display-names-design.md`),
Task 9 di 9, ULTIMO: la finestra AI Chat mostra `display_name` al posto
dell'etichetta tecnica (`skimble-human`) e un badge visivo per `is_ai`. I
campi arrivano dal wire dal Task 2/3 (`orchestrator`); questo task li
consuma nel frontend.

- `aichat-view.mjs`: `messageLine({from_label, text, display_name, is_ai})`
  ora ritorna `{label, text, is_ai}` — `label` è `display_name` se presente,
  altrimenti fallback a `from_label` (compat storico/peer vecchi); `is_ai`
  normalizzato a booleano (`Boolean(is_ai)`, assente → `false`). Nessuna
  concatenazione testuale nome+ruolo: la distinzione resta solo visiva (CSS).
- `aichat-window.js`: **2 bug reali confermati e corretti** (trovati
  leggendo il codice, non ipotesi):
  1. `addLine` destrutturava solo `{from_label, text}`, scartando
     `display_name`/`is_ai` dal payload in arrivo — corretto ad accettare
     ed inoltrare tutti e 4 i campi a `messageLine`.
  2. `isSelf(line.label, myBase)` passava il label GIÀ RISOLTO (dopo questo
     task, potenzialmente un nickname come "Maurizio") a una funzione che
     confronta contro `"<label_base>-"` — un nickname non inizia mai per
     costruzione con quel prefisso, quindi il confronto era semanticamente
     sbagliato. **Nota**: il difetto è oggi LATENTE, non un regresso
     osservabile — `myBase` in questa finestra è hardcoded a `""`
     (`const myBase = ""`, mai riassegnato) e `.self` non si applica
     comunque a nessun messaggio, prima o dopo questo fix. La correzione
     rende la chiamata semanticamente corretta per quando `myBase` verrà
     popolato (oggi disponibile via `selfLabel`, riga 42/124, ma derivarne
     `myBase` è una decisione fuori scope per questo task — vedi report).
     Corretto passando `from_label` (grezzo) a `isSelf`, non `line.label`.
  Aggiunta la classe `ai` sulla riga (`div.className`) pilotata da
  `line.is_ai`, accanto a `self`.
- **Terzo bug reale trovato leggendo `app.js` (non nel brief)**: il case
  `"ai_chat_message"` del dispatcher `handleServerMsg` ricostruiva a mano il
  payload di `aichat:msg` con solo `{from_label, text}`, scartando
  `display_name`/`is_ai` PRIMA che raggiungessero `aichat-window.js` — anche
  dopo il fix di `addLine`, i messaggi LIVE (a differenza dello storico via
  `ai_chat_history`, che inoltra l'intera entry) non avrebbero mai mostrato
  nickname/badge. Corretto: il case ora inoltra anche i 2 campi.
- `aichat-window.html`: nuova regola CSS `.line.ai .lbl` — colore etichetta
  distinto + prefisso "🤖 " via `::before`. Nessuna modifica al JS per il
  badge: resta puramente CSS, coerente con la scelta di design (un nickname
  scelto dall'AI è un nome proprio, non va reso ambiguo da un suffisso
  testuale).
- 3 nuovi test in `aichat-view.test.mjs` (TDD RED→GREEN, RED confermato
  eseguendo la suite prima dell'implementazione): `messageLine usa
  display_name quando presente`, `messageLine ricade su from_label quando
  display_name è assente`, `messageLine tratta is_ai assente come false
  (compat peer vecchi)`. Il test preesistente `messageLine passa label e
  text` aggiornato per includere `is_ai: false` nell'atteso (comportamento
  intenzionalmente cambiato da questo task, non una regressione).
- Nessun altro punto di consumo trovato: `aichat-window.js` è l'unico
  importatore di `aichat-view.mjs` (grep confermato); `aichat-push.js`
  (gate emit/buffer/drop) tratta il payload come opaco, non lo altera.
- `node --test crates/ui/frontend/aichat-view.test.mjs`: 14/14 verdi.
  `node --test crates/ui/frontend/*.test.mjs` (intera suite JS): 265/265
  verdi, nessuna regressione.

## [0.46.6] — 2026-08-13 — `AiChatSettings`: mirror `display_name`/`ai_display_name` (backend, Task 5)

Nickname umano + AI (design `Docs/superpowers/specs/2026-08-13-aichat-display-names-design.md`),
Task 5 di 9: 2 nuovi campi `Option<String>` su `AiChatSettings`
(`display_name`, `ai_display_name`), mirror "sul filo" di
`orchestrator::aichat::config::AiChatConfig::{display_name,ai_display_name}`
(già aggiunti nei Task 1-4, backend `orchestrator`) — stessi nomi/tipi/
semantica, ma tipo Rust indipendente: `ui` e `orchestrator` comunicano SOLO
tramite lo stesso `network.json` su disco, nessuna dipendenza di crate.

- `merge_aichat_settings`: scrive i due nickname con `insert`/`remove`
  (invece di `insert(json!(null))`) — un nickname cancellato dall'utente
  sparisce dal file, non resta come chiave `null`.
- Nuova `validate_nickname(&Option<String>, field_label)`, condivisa da
  entrambi i campi: `None` sempre valido ("nessun nickname"); se `Some`,
  trim + non vuoto + lunghezza 1..=48; **nessuna restrizione di charset** —
  a differenza di `label_base` (`[A-Za-z0-9_-]`, niente spazi) il nickname è
  testo libero mostrato in UI, non un identificatore sul wire: spazi e
  accenti sono validi ("Maria José", "Àlex").
- `get_aichat_settings`: legge i due campi come `None` quando la chiave è
  assente (file assente/corrotto, o `network.json` scritto prima che i
  campi esistessero) — stesso pattern infallibile degli altri campi.
- `set_aichat_settings`: trim se presenti, stringa vuota-dopo-trim →
  `None` (stesso trattamento già riservato a `label_base`) — l'utente può
  "cancellare" un nickname scrivendo solo spazi nel campo.
- 5 nuovi test unitari (TDD RED→GREEN — RED per errore di compilazione,
  i 2 campi non esistevano ancora su `AiChatSettings`):
  `merge_round_trip_preserves_display_names`,
  `get_settings_reads_display_names_with_none_default`,
  `validate_rejects_display_name_too_long`,
  `validate_accepts_display_name_with_spaces_and_accents`,
  `deserializes_payload_without_display_name_keys` (quest'ultimo, aggiunto su
  suggerimento della review, PASSA subito — non è un RED "vero": documenta che
  serde tollera già di natura l'assenza della chiave per un campo `Option<T>`
  senza bisogno di `#[serde(default)]`, coprendo il payload che
  `config-dialog.js` manda oggi, PRIMA del Task 6); i 9 struct-literal
  `AiChatSettings { .. }` preesistenti nel modulo (produzione + test)
  aggiornati ai 2 nuovi campi. `cargo test -p ui` invariato per il resto:
  137/137 verdi.

Solo backend `ui` (Rust) in questo task — nessun campo raggiungibile da
`/config` finché il Task 6 (frontend) non aggiunge i due input nel tab AI
Chat di `config-dialog.js`.

**Rischio noto nella finestra Task 5 → Task 6** (segnalato in review, non
un bug — comportamento inevitabile finché il frontend non è aggiornato):
`config-dialog.js` oggi costruisce `newAiChat` SENZA `display_name`/
`ai_display_name` (Task 6 non ancora fatto) → deserializza a `None` per
entrambi (confermato dal test sopra) → `merge_aichat_settings` prende il ramo
`None` → `obj.remove(...)` → un nickname eventualmente già presente in
`network.json` (scritto a mano, o dal tool `set_ai_display_name` una volta
che il Task 7 esisterà) verrebbe CANCELLATO al primo salvataggio da
`/config` fatto in questa finestra. Non testare dal vivo un salvataggio di
`/config` con un nickname impostato finché il Task 6 non è mergiato.

## [0.46.5] — 2026-08-11 — picker screener: click seleziona, doppio click conferma

Report utente dal primo smoke test dal vivo: "click e doppio click non
fanno nulla, solo Invio conferma". Causa: il click singolo su una riga
chiamava `confirmSelection()` subito (comportamento diverso da hover/Invio)
— con un solo screener in lista la riga è già selezionata di default,
quindi un click "che seleziona soltanto" sarebbe sembrato non fare nulla;
ipotesi più probabile della causa rispetto a un problema di consegna degli
eventi DOM (la finestra si crea già con `.focused(true)`). Allineato alla
convenzione Windows richiesta esplicitamente dall'utente: 1 click seleziona
(stessa logica già usata da `mouseenter`), doppio click conferma (stessa
logica di Invio). Hint bar aggiornata di conseguenza. Nessuna modifica al
modulo puro `screener-picker-list.mjs` (8/8 test invariati) — solo wiring
DOM in `screener-picker.js`, mai testato in unit per convenzione di
progetto (glue DOM, non logica pura).

## [0.46.4] — 2026-08-11 — picker screener: comando Tauri + wiring (ora raggiungibile)

`open_screener_picker_window` (main.rs, mirror di `open_search_window`, NON
singleton) + `capabilities/screener-picker-window.json` (label
`screener-picker-*`). `external-channel-window.js`: nuovo case
`open_screener_picker` (apre la finestra) + listener `screener:picked`
(riceve la scelta, la inoltra come comando di testo normale
`"Esegui lo screener <title>"` sulla connessione WS della finestra /markets
— filtrato per label, mai `emitTo`). La feature `/markets` "esegui uno
screener senza nominarlo" è ora completa end-to-end lato codice (manca solo
lo smoke test dal vivo). Vedi
`Docs/superpowers/specs/2026-08-11-markets-screener-registry-design.md`.

## [0.46.3] — 2026-08-11 — `screener-picker.{html,js,mjs}` (non ancora raggiungibile)

Nuova finestra picker per la scelta di uno screener (canale `/markets`,
Contratto A `ServerMsg::OpenScreenerPicker`): `screener-picker-list.mjs`
(logica pura di navigazione, ↑/↓ con wraparound, `node:test`) +
`screener-picker.html`/`.js` (runtime webview, riceve gli item via
`take_window_content`, rimanda la scelta via evento globale
`screener:picked` filtrato per label opener — mai `emitTo`, coerente col
resto del codebase). Non ancora apribile: manca il comando Tauri
`open_screener_picker_window` e il wiring in `external-channel-window.js`.
Vedi `Docs/superpowers/specs/2026-08-11-markets-screener-registry-design.md`.

## [0.46.2] — 2026-08-07 — watchdog frontend riconosce l'heartbeat del turno AI

Nuovo case `"heartbeat"` in `handleServerMsg` (`app.js`): riusa `commandChunkReceived` esistente
per resettare il timer di silenzio senza stampare nulla nel pannello. Corregge il falso positivo
osservato dal vivo (documento lungo annullato automaticamente a 184s di silenzio pur essendo
ancora in lavorazione) — vedi `orchestrator` 0.41.2 e
`Docs/superpowers/specs/2026-08-07-ai-turn-heartbeat-watchdog-design.md`.

## [0.46.1] — 2026-08-05 — Review finale save_routine: race sull'emit, backstop chiusura, trasparenza, capability

Fix dalla review finale whole-branch pre-merge di `routine-preview.js`/`.html` (Fase 2
save_routine), findings C1/I1/I2 — allineamento ai pattern già corretti dal vivo in
`aichat-window.js`/`note-window.js`.

**C1 (critical) — `emitDecision` fire-and-forget correva contro `close_self`.** Il pulsante
Salva chiamava `emitDecision(accept); closeSelf();` in sequenza sincrona: `close_self`
tronca il canale IPC della webview, e può vincere la corsa contro l'`emit` ancora in volo —
app.js non riceve mai la decisione, la conferma pendente lato orchestrator scade dopo 180s
e il salvataggio viene trattato come negato NONOSTANTE il click su Salva. Stesso bug già
trovato dal vivo su AI Chat (2026-07-29) e corretto con lo stesso pattern in
`note-window.js`. Fix: `emitDecision` ora `async`, ogni chiamante (Salva/Annulla/✕/Escape)
`await`a l'emit prima di chiudere.

**I1 — nessun backstop `beforeunload`.** Chiudere la finestra per vie diverse dai pulsanti
✕/Escape (Alt-F4, task manager, chiusura da barra) lasciava la conferma pendente appesa per
l'intero timeout di 180s — il commento in testa al file dichiarava questo comportamento
coperto ma non lo era. Aggiunto un listener `beforeunload` che chiama `emitDecision(false)`
best-effort (non atteso — `beforeunload` non garantisce che una promise completi), stesso
pattern del backstop già in `aichat-window.js`.

**I2 — nessuna trasparenza configurabile.** La finestra referenziava `--window-alpha` nel
proprio CSS (`--bg: rgba(15,15,22, var(--window-alpha, 0.87))`) ma nulla la impostava mai —
bloccata al default 0.87, sorda allo slider di trasparenza dell'utente. Copiato il blocco
standard (`get_config` all'avvio + listener `config:saved`) da `note-window.js`.

**Capability Tauri mancante (trovata in verifica, non nella lista dei finding — bloccava
l'intera finestra, non solo I2).** Nessun file in `crates/ui/src-tauri/capabilities/`
copriva il pattern di label dinamico `routine-preview-<ts>-<ctr>` (`open_routine_preview`,
`main.rs`) — ogni altra finestra secondaria a label dinamica (`md-*`, `plugin-*`,
`search-*`) ha il proprio file capability, questa no. Senza un `windows` match, Tauri nega
di default ogni permesso IPC: `take_window_content`/`close_self`/`get_config`/emit-listen
sarebbero stati bloccati per l'intera finestra, a prescindere dai fix C1/I1/I2 sopra.
Aggiunto `capabilities/routine-preview.json` (`windows: ["routine-preview-*"]`,
`core:default` + `core:window:allow-start-dragging` — stesso pattern di
`search-window.json`/`note-window.json`).

## 0.46.0

- Nuova finestra dedicata **anteprima save_routine** (Fase 2 — Docs/superpowers/specs/
  2026-08-05-save-routine-design.md §5): `open_routine_preview` (Tauri command) +
  `routine-preview.html`/`.js`. Riceve `ServerMsg::RoutineSavePreview`
  (`case "routine_save_preview"` in `app.js`), mostra nome/descrizione/categoria/
  tag/corpo script (textContent, nessun innerHTML/sanitizzazione necessaria) e un
  banner "sostituisce X" quando `replace` è presente. I pulsanti Salva/Annulla NON
  chiamano un nuovo comando Rust: emettono l'evento globale Tauri
  `"routine-preview:decision"`, che `app.js` (`setupRoutinePreviewEvents`) inoltra
  come la STESSA `ClientMsg::ToolConfirmResponse` già usata dal banner Sì/No del
  cursore — zero nuovi messaggi WS lato risoluzione.

## [0.45.44] — 2026-08-04 — `LARE_LOCAL_DIR`/`LARE_ROAMING_DIR`: override delle cartelle dati

Due nuove variabili d'ambiente per ridirigere le cartelle dati dell'app a un percorso arbitrario
(es. un profilo condiviso fra macchine sotto una radice comune), lato `ui` — mirror del lavoro già
fatto sull'orchestrator per `LARE_LOCAL_DIR`. Path completo usato verbatim (nessun
`dev.lare.terminal` unito sopra); non impostate → comportamento invariato.

**`LARE_LOCAL_DIR`** (override di `%LOCALAPPDATA%\dev.lare.terminal\`), onorata in 4 moduli di
`/config`: `aichat_settings.rs` (`network.json`), `search_settings.rs` (`search-paths.json`),
`llm_settings.rs` (`llms.json`), `plugins_view.rs` (`plugins/` — `LARE_PLUGINS_DIR` continua a
vincere prima, invariato). Stessa catena di precedenza dell'orchestrator (override specifico del
sito, se esiste → `LARE_LOCAL_DIR` → `LOCALAPPDATA` → fallback `.lare-data`).

**Fix di correttezza incluso in questo giro (non cosmetico):** `read_lare_token` (`main.rs`) usava
SOLO `app.path().app_local_data_dir()` (l'API di Tauri) per trovare il file `token` scritto
dall'orchestrator. Con `LARE_LOCAL_DIR` impostata, l'orchestrator scrive il token nella cartella
di override mentre la UI avrebbe continuato a leggerlo dalla cartella Tauri di default — due
processi che si aspettano il token in due posti diversi, handshake WS rotto in silenzio. Ora
`read_lare_token` controlla `LARE_LOCAL_DIR` per prima, esattamente come l'orchestrator, e cade
su `app_local_data_dir()` solo se non impostata (comportamento di oggi).

**`LARE_ROAMING_DIR`** (nuova, override di `%APPDATA%\dev.lare.terminal\`): `config_file_path` e
`library_dir_path` (`main.rs`) la controllano prima di `app.path().app_config_dir()` — stesso
schema, applicato alla base Roaming invece che Local.

**Diagnostica:** due nuove righe di log allo startup (`.setup()`), `[ui] Local data dir: <path>` e
`[ui] Roaming data dir: <path>`, così le due cartelle risolte sono visibili senza doverle dedurre
a mano.

**Correzione doc-comment:** `config.rs` dichiarava `config.json` sotto `AppData\Local\` — errato,
`app_config_dir()` di Tauri risolve a `AppData\Roaming\` su Windows. Corretto a `Roaming`.

## [0.45.43] — 2026-08-02 — fix albero sbagliato nel dialog "Sposta" quando il parent è escluso

Bug reale segnalato dal vivo con screenshot: creando una sottocartella
(es. "Assets" dentro "Markets") e aprendo "Sposta" su un documento della
cartella padre, l'albero delle destinazioni mostrava "Assets" come se fosse
annidata sotto una cartella completamente estranea ("Internet") invece che
sotto "Markets".

Root cause: `moveTargets()` (`library-nav.mjs`) esclude sempre il parent
corrente dalla lista destinazioni (spostarcelo sarebbe un no-op) — ma lo
faceva RIMUOVENDOLO del tutto. I suoi altri figli restavano in lista alla
loro `depth` originale (assoluta, dell'intero albero), e la UI (`library.js`)
li indenta SOLO in base a `depth`, senza linee di connessione né etichetta
del genitore. Col vero genitore invisibile, l'indentazione faceva apparire
quei figli annidati sotto qualsiasi riga precedente a depth minore — nel
caso segnalato, "Markets" cadeva alfabeticamente fra "Internet" e
"Protocolli", quindi "Assets" (suo figlio) si presentava subito dopo
"Internet" come se ne fosse figlia. I dati di destinazione (`relPath`)
restavano corretti — lo spostamento reale sarebbe andato a buon fine — ma la
UI mentiva sulla gerarchia mostrata.

Fix: `moveTargets()` ora tiene il parent corrente in lista con un nuovo
campo `disabled: true` (riga-ancora non selezionabile), invece di
rimuoverlo. `library.js` rende quella riga in grigio, non cliccabile
(niente listener + `pointer-events:none` in CSS, difesa in profondità),
con suffisso "(posizione attuale)". 6 test in `library-nav.test.mjs`
aggiornati per riflettere la nuova forma dei risultati (RED confermato
contro l'implementazione precedente, poi GREEN).

## [0.45.42] — 2026-08-01 — rimosso footer hint disallineato in /library

Il `#footer` in fondo a `library.html` ("Invio o doppio-clic per aprire ·
rinomina · sposta · condividi · elimina") era testo statico, mai toccato da
JS: restava identico su ogni tab (Markdown/Find/Plugins/Note) anche se i
comandi realmente disponibili cambiano per tab (es. Find e Note non hanno
"sposta"/"condividi", Plugins non ha nessuno dei quattro). Scelta esplicita
dell'utente fra "disallinea sempre" e "rendilo per-tab": rimosso del tutto,
perché ogni pulsante riga ha già `title`/`aria-label` con l'azione esatta —
il footer era pura ridondanza, e per giunta scorretta.

Rimossi `<div id="footer">` e la regola CSS `#footer` in `library.html`.
Nessun riferimento JS al footer (verificato via grep su `library.js`), quindi
rimozione senza effetti collaterali.

## [0.45.41] — 2026-07-31 — fix #3 primo smoke test dal vivo: finestra "Nuova/Modifica nota" separata

Terzo e ultimo finding del primo smoke test multi-macchina reale (v. 0.45.39/
0.45.40 per #1/#2). Il dialog `<dialog>` dentro `library.html` (Task 19) era
inamovibile e ridimensionabile solo trascinando l'angolo della textarea, non
il bordo della finestra — essendo DOM figlio del webview Library, non una
finestra a sé. Scelta esplicita dell'utente (fra "sistema il dialog
esistente" e "vera finestra Tauri separata"): la seconda.

**Nuovi file** (stesso schema di `open_aichat_window`/`open_plugin_window`,
`WindowContentStore`+`take_window_content`, hub eventi in `app.js` — nessuna
nuova infrastruttura Tauri, solo un altro consumatore del pattern esistente):
- `src-tauri/capabilities/note-window.json` — `core:default` +
  `core:window:allow-start-dragging`, `"windows": ["note-compose"]`.
- `frontend/note-window.html`/`.js` — chromeless, nessuna `ws-client.js`
  (parla solo con app.js via eventi Tauri, come AI Chat/plugin). La textarea
  riempie lo spazio verticale (`flex:1; resize:none`) invece di avere un
  proprio handle — è la FINESTRA a essere ridimensionabile dai bordi nativi
  del SO (`resizable(true)` sul builder). Riusa `noteEditMessages`
  (note-view.mjs) per la decisione "cosa salvare", stessa logica testata di
  0.45.40 — nessuna duplicazione fra vecchio e nuovo dialog.

**`main.rs`**: nuovo comando `open_note_window(note_id, title, text)` —
singleton label `"note-compose"` (niente editing affiancato di due note).
Se la finestra esiste già (riapertura su un'altra nota), `take_window_content`
non verrebbe più letto (one-shot, già consumato al primo avvio): il contenuto
fresco arriva allora via un evento Tauri diretto (`note-window:load`),
sicuro perché il JS della finestra è già in ascolto a quel punto. Aggiunto
`"note-compose"` al loop F2 hide/show (assieme a `"library"` — le Note sono
una sotto-feature di Library, a differenza di `"aichat"` che non è nel loop).

**`app.js`**: `openNoteWindow(note)` + `setupNoteWindowEvents()` (ascolta
`note-window:save`, inoltra a `client.sendNoteCreate`/`sendNoteEdit`/
`sendNoteEditTitle` secondo lo stesso schema di `noteEditMessages`).
`setupLibraryEvents()`: le vecchie `library:note-create`/`note-edit`/
`note-edit-title` sostituite da `library:note-window-open` (richiesta
apertura finestra); `library:note-delete` invariata (non coinvolge alcuna
finestra).

**`library.js`/`library.html`**: rimossi `#note-dialog` e tutto il suo
wiring (`openNoteDialog`, `wireNoteDialog`, `noteEditState`) — "Nuova"/✏️
ora emettono `library:note-window-open` verso il main invece di aprire un
dialog locale.

## [0.45.40] — 2026-07-31 — `noteEditMessages` estratta e testata (`note-view.mjs`)

La decisione "cosa mandare al salvataggio di Modifica nota" (0.45.39) era
logica pura ma viveva inline nell'handler `noteSaveBtn` — l'unico pezzo del
dialog nota rimasto non coperto da `node:test`, e proprio quello appena
toccato da un fix reale. Estratta in `noteEditMessages({title, text,
originalTitle, originalBodyText}) → {editText?, editTitle?}`, 5 nuovi test
in `note-view.test.mjs` (nulla cambiato, corpo svuotato deliberatamente,
solo titolo, entrambi, confronto grezzo senza trim). `library.js` ora
chiama la funzione invece di duplicarne la logica — comportamento
invariato, copertura reale al suo posto.

## [0.45.39] — 2026-07-31 — fix #1/#2 del primo smoke test dal vivo: testo nota precompilato, stile pulsante/toolbar

Due dei tre finding del primo smoke test multi-macchina reale del Blocco note:

**#1 (funzionale)** — "Modifica" mostrava il titolo ma mai il testo della
nota. `openNoteDialog` (`library.js`) ora precompila la textarea da
`note.my_segment_text` (protocol 0.14.9 / orchestrator 0.40.69), non più
sempre vuota. Effetto a cascata sull'handler di "Salva" (FIX 3 della review
finale di branch): il confronto "il corpo è cambiato?" era `text.trim() !==
""` — corretto quando la textarea partiva sempre vuota, ma con un valore
precompilato reale diventava un bug speculare: cancellare deliberatamente
tutto il testo per svuotare il proprio segmento veniva silenziosamente
ignorato al salvataggio. Corretto a `text !== noteEditState.originalBodyText`
(confronto contro il valore catturato all'apertura, non contro vuoto).

**#2 (stile)** — il pulsante "Nuova" (nota) e la toolbar del tab Note non
erano allineati allo stile del programma (Task 17 non li aveva stilizzati).
`library.html`: `#new-note-btn` aggiunto al blocco di stile condiviso di
`#new-folder-btn`/`#reload-btn`; nuovo blocco `#note-toolbar` a specchio di
`#lib-toolbar` esistente.

Finding #3 (finestra "Nuova/Modifica nota" da `&lt;dialog&gt;` a vera finestra
Tauri separata, spostabile/ridimensionabile) resta aperto — richiede una
finestra `WebviewWindow` genuina, non uno style fix; trattato a parte.

## [0.45.38] — 2026-07-31 — fix race `peerNetworkEnabled` (parcheggiato dalla review finale)

`loadNoteTab` legge `get_aichat_settings` (config su disco) in parallelo al
listener `library:notes-snapshot` (prova live che il servizio è acceso) —
nessuna Promise attesa dall'altra. Se la lettura del config risolveva DOPO
uno snapshot già arrivato, un `enabled: false` letto dal file poteva
sovrascrivere un `peerNetworkEnabled = true` già confermato dal vivo,
mostrando "rete disabilitata" mentre il Blocco note era di fatto attivo —
tipicamente dopo che l'utente cambia `network.json` senza riavviare
l'orchestrator. Fix: la lettura del config non scrive più sopra un `true`
già confermato (`library.js`, guardia `peerNetworkEnabled !== true` prima
dell'assegnazione) — solo lo snapshot live può affermare "acceso", il file
può solo affermare "spento" o lasciare lo stato invariato.

Trovato e parcheggiato come Minor non bloccante nella review finale del
branch "Blocco note" (`Docs/superpowers/plans/2026-07-29-library-notes.md`);
corretto qui prima dello smoke test dal vivo su richiesta esplicita.

---

## [0.45.37] — 2026-07-31 — fix di review finale: corpo nota visibile, salvataggio non distruttivo, `/config` su network.json

Quattro correzioni trovate nella review dell'INTERO branch "Blocco note".

**FIX 2 (Critical)** — `buildNoteItem` renderizzava solo titolo e meta:
`note.body` era plumbato da Rust fino all'oggetto JS e poi MAI mostrato. La
funzione era di sola scrittura — si potevano creare e sincronizzare note, ma
non leggerne il contenuto su nessuna macchina. Il corpo è ora sempre visibile
nella riga (`.note-item-text`, `white-space: pre-wrap` per preservare gli
a-capo e le intestazioni `— macchina —` del corpo fuso; `max-height` +
`overflow-y: auto` per le note lunghe), impostato con `textContent`.

**FIX 3 (Critical, perdita di dati)** — "Salva" in modalità Modifica inviava
SEMPRE sia `note-edit` sia `note-edit-title`. Poiché la textarea si apre
vuota per design, correggere un refuso nel titolo mandava una stringa VUOTA
come segmento di questa macchina, cancellando il proprio contributo al corpo.
Ora l'invio è CONDIZIONALE (design §10): `note-edit` solo se l'utente ha
scritto qualcosa (payload GREZZO, il `trim()` serve solo al test di
emptiness), `note-edit-title` solo se il titolo differisce da quello
catturato all'apertura (`noteEditState.originalTitle`); se non è cambiato
niente, "Salva" chiude e basta, senza messaggi in rete.

**FIX 4 (Critical)** — `aichat_settings.rs` (backend Tauri del tab /config →
AI Chat) leggeva e scriveva ancora `aichat.json`, mentre l'orchestrator è
passato a `network.json` (che vince sempre sul legacy). Risultato: era
IMPOSSIBILE abilitare la funzione dalla UI preposta. Ora: lettura da
`network.json` con fallback al legacy `aichat.json` se il primo non esiste
ancora; scrittura SEMPRE e SOLO su `network.json`. Aggiornati anche i
riferimenti al nome del file in `config-dialog.js`.

**FIX 6 (Important)** — con `enabled: false` (il default) il servizio non
parte, nessuno snapshot arriva mai e il tab Note mostrava "Nessuna nota." —
indistinguibile da "hai zero note". Il tab legge ora `get_aichat_settings` e,
a rete peer spenta, mostra un messaggio che nomina file, campo e dove
cambiarlo, disabilitando il pulsante "Nuova". Stato "non so" (lettura
fallita) tenuto distinto da "disabilitata": in dubbio non si afferma nulla e
il pulsante resta attivo.

## [0.45.36] — 2026-07-31 — cap 256 KB sul corpo nota, validato client-side (Blocco note)

Gap trovato in self-review del piano: il cap di design (§9) non aveva alcuna
enforcement. `isBodyValid` (byte UTF-8, non caratteri) in `note-view.mjs`,
validato in `wireNoteDialog` prima dell'invio.

## [0.45.35] — 2026-07-31 — rendering + dialog + wiring WS per Blocco note

`library.js` completo per la scheda "Note": lista (filtrata/ordinata via
`note-view.mjs`), dialog di composizione riusato per crea/modifica (mirror di
`wireMoveDialog`), listener WS per snapshot/push live. Smoke test end-to-end
single-machine NON eseguito in questo commit (ambiente agent senza
interazione GUI reale) — resta da fare, vedi Task 21 e
`task-19-report.md`. Nota (debito, non bloccante): il dialog "Modifica"
precompila vuoto invece del segmento proprio della macchina — `NoteView` non
lo espone ancora; da rivedere se emerge come problema d'uso reale.

Fix post-review (stesso commit/versione, due findings su codice letterale del
brief): (1) titolo+meta della riga nota ora impilati verticalmente in
`.note-item-body` invece che affiancati orizzontalmente (`.archive-item` è
flex row); (2) eliminazione nota ora a due click di conferma
(`wireNoteDeleteButton`, stesso meccanismo di `wireDeleteButton` ma adattato
al trasporto fire-and-forget via evento, non `invoke`), coerente con ogni
altra eliminazione nel file. Pulsanti ✏️/🗑 ora con le classi hover esistenti
(`.archive-item-move`/`.archive-item-delete`) invece del chrome di default
del browser.

## [0.45.34] — 2026-07-31 — `note-view.mjs`, logica pura Blocco note (testata)

Modulo puro senza DOM/Tauri/I/O (stesso stile di `share-view.mjs`):
`visibleNotesSorted` (filtra cancellate, ordina per data), `isTitleValid`
(cap 200 caratteri), `formatNoteMeta`. Testato con `node:test`.

## [0.45.33] — 2026-07-29 — markup tab "Note" + dialog composizione (Blocco note)

Quarta tab nella Library (`Markdown | Find | Plugins | Note`), dialog
`#note-dialog` che riusa `.move-dialog-box`/`.move-dialog-footer` (stessa
struttura di Sposta/Condividi) invece di aprire una seconda finestra Tauri —
correzione rispetto alla bozza iniziale del design doc, che ipotizzava il
pattern del dialog parametri di `/crypto`: la Library ha già un precedente
diretto più semplice e coerente (due dialog nativi esistenti nello stesso
file). Solo markup — il rendering/wiring arriva in Task 19.

## [0.45.32] — 2026-07-29 — trasporto WS per Blocco note (`ws-client.js`/`app.js`)

Quattro nuovi metodi `send*` su `LareWsClient` (stesso schema di
`sendShareDocument`), due nuovi `case` nello switch dei `ServerMsg` in
`app.js` (`notes_snapshot`/`note_upserted`, con cache `lastKnownNotes` per il
pattern pull-on-open già usato da roster/reachable-peers), e cinque listener
in `setupLibraryEvents` per il round-trip Library↔app.js↔orchestrator.
Nessuna UI visibile ancora — solo trasporto (Task 17-19 la usano).

## [0.45.31] — 2026-07-29 — fix(ui): possibile corsa emit-vs-close nella chiusura della finestra AI Chat

Bug osservato dal vivo dopo il merge dell'icona busta (0.45.30): la primissima
notifica funzionava, ma dopo la prima VERA chiusura della finestra chat
(pulsante ×, confermato dall'utente — non una finestra rimasta aperta in
background), nessun messaggio successivo accendeva più la busta, su
entrambe le macchine, ripetutamente. Investigazione (systematic-debugging,
nessuna certezza al 100% da sola lettura statica — nessun harness per
testare una corsa IPC Tauri): `aichat-window.js::closeWindow()` mandava
`emit("aichat:closed", {})` SENZA attenderlo (fire-and-forget) prima di
chiamare `invoke("close_self")`, che distrugge la finestra — se la
distruzione vince la corsa, `app.js` non riceve mai l'evento, `aiChatGate`
resta bloccato su `live=true` per sempre, e ogni push successivo torna
`"emit"` invece di `"drop"` (return anticipato in `pushAiChat`, PRIMA del
controllo `isNotifiableDrop`).

Trovato anche un problema strutturale che rendeva l'attesa impossibile a
monte: il wrapper locale `emit()` non ritornava la Promise di
`tauriEvent.emit(...)` — nessun chiamante avrebbe potuto attenderla anche
volendo. Fix: `emit()` ora ritorna la Promise; `closeWindow()` diventa
`async` e fa `await emit(...)` PRIMA di `invoke("close_self")`. Il
percorso `beforeunload` (chiusura non dal nostro × — Alt+F4 o simili)
resta fire-and-forget: la finestra è già in fase di distruzione esterna
lì, non c'è nulla da attendere con garanzie.

Nessun test automatico (corsa IPC fra due processi, nessun harness nel
progetto per questo) — verifica dal vivo richiesta.

## [0.45.30] — 2026-07-28 — feat(ui): icona busta per attività AI Chat non vista

Nuovo indicatore passivo nella finestra principale: un'icona busta (✉) accanto allo status
badge di connessione (`#status-badge`) segnala che è arrivato un evento AI Chat rilevante
mentre la finestra chat era chiusa. Nato da un'osservazione dell'utente dopo i due bug
Condividi/gate-2 corretti nella stessa sessione: senza aprire la chat non c'era alcun modo di
sapere che qualcosa la aspettava. Design:
`Docs/superpowers/specs/2026-07-28-aichat-unseen-notification-indicator-design.md`.

- Esattamente **4 eventi notificabili** (su ~12 `ai_chat_*` gestiti da `pushAiChat`):
  `aichat:msg` (messaggio), `aichat:share-request` (richiesta condivisione Library),
  `aichat:join-prompt` (gate 1 — sei tu il candidato), `aichat:admission-request` (gate 2 —
  sei tu il server). Roster/storico/self/reachable-peers/pending/admitted/rejected/peer-lost
  restano sincronizzazione di stato passiva, non accendono l'indicatore.
- Comportamento **solo presenza**: nessun contatore numerico, nessun click-to-open — icona
  passiva, l'utente apre la chat come già fa oggi (`/aichat` o riga hint).
- `pushAiChat` ora consulta esplicitamente `isNotifiableDrop(event, action)`
  (`aichat-push.js`, Task 1 di questo piano) sul ramo `"drop"` del gate esistente; l'indicatore
  si azzera nel listener `"aichat:ready"`, appena la webview della chat rigioca il buffer.
- `index.html`: nuovo `<span id="aichat-notify">`, riusa la classe `.hidden` già esistente
  (stessa di `#stop-btn`) — nessun nuovo meccanismo di show/hide.

Test: `node --test crates/ui/frontend/*.test.mjs` → 242/242 verdi (235 pre-esistenti + 7 di
Task 1 per `isNotifiableDrop`), nessuna regressione. Nessun nuovo test in questo task: il
cablaggio DOM/dispatch in `app.js`/`index.html` non ha harness automatico nel progetto —
verifica riservata allo smoke test dal vivo (Step 10 del piano, a cura dell'utente).

## [0.45.29] — 2026-07-28 — fix(ui): "Condividi" legge la raggiungibilità di rete, non il roster chat

Bug utente: il dialog "Condividi" della Library mostrava "Nessuna macchina
connessa" nonostante discovery+elezione AI Chat riusciti nel log
dell'orchestrator. Root cause: leggeva il roster di ammissione alla stanza
chat (richiede il gate umano "vuoi entrare?"), ma il backend che esegue
davvero una condivisione non lo consulta mai — usa `peers ∩ links`
(discovery UDP + link TCP, entrambi automatici). Fix lato backend:
`ServerMsg::AiChatReachablePeers` (protocol 0.14.7, orchestrator 0.40.51).
Design: `Docs/superpowers/specs/2026-07-28-library-share-reachable-peers-design.md`.

- **`app.js`**: nuova cache `lastKnownReachablePeers`, nuovo case
  `"ai_chat_reachable_peers"` nel dispatch, risposta a
  `"library:request-reachable-peers"` — mirror esatto e SEPARATO del
  pattern `lastKnownRoster`/`"library:roster"` esistente (non condivide
  stato con la finestra AI Chat).
- **`library.js`**: nuova variabile `currentReachablePeers`, listener
  `"library:reachable-peers"`, richiesta al bootstrap.
  `openShareDialog` cambia una riga (fonte dati); `shareTargetList`
  (già testata) resta byte-identica.
- `currentRoster`/`"library:roster"` in `library.js` restano nel file
  (morti per `openShareDialog` dopo questo fix) — pulizia futura
  dichiarata fuori scope, diff minimo.

Eseguito con **executing-plans** (esecuzione inline, 3 task — protocol,
orchestrator, ui). Test: `node --test crates/ui/frontend/*.test.mjs` →
235/235 verdi, nessuna regressione (nessun nuovo file `.test.mjs`: il
cablaggio è a specchio di un pattern già in produzione e non testato
isolatamente nemmeno per il roster originale). **Smoke test dal vivo a 2
macchine ancora da fare con l'utente** (senza mai aprire `/aichat`).

## [0.45.28] — 2026-07-28 — feat(ui): ordinamento cliccabile colonne nelle finestre Markdown

Richiesta dell'utente dopo l'uso dal vivo di `screen_stocks`: due azioni per
colonna (▲ crescente, ▼ decrescente) sulla tabella "Selezione di oggi", su
tutte le colonne tranne Pos (sempre un contatore ricostruito). Design:
`Docs/superpowers/specs/2026-07-28-financial-markets-screen-stocks-sortable-table-design.md`.

- **`table-sort.mjs`** (nuovo modulo puro): `sortedOrder(keys, dir, type)` —
  nessun DOM, dato un array parallelo di `data-sort-value` (o `null` per un
  dato mancante) ritorna gli indici originali nel nuovo ordine. Valori
  mancanti **sempre in coda**, in entrambe le direzioni. Ordinamento stabile
  sui pareggi (`Array.sort` è stable da ES2019+, nessuna logica di
  tie-break scritta a mano).
- **`window.js`**: un listener delegato su `#content`, installato **una
  sola volta** al bootstrap (sopravvive a ogni `renderMarkdown()`
  successivo, incluso il re-render di "Espandi"). Al click su
  `.sort-arrow`, l'indice colonna si deriva da `th.cellIndex` — nessun
  mapping nome-colonna da mantenere sincrono col Python che genera la
  tabella. Dopo il riordino delle `<tr>` (via `appendChild` ripetuto in
  sequenza, che sfrutta lo spostamento nativo dei nodi già nel DOM), la
  colonna Pos si **rinumera 1..N** sul nuovo ordine visuale (marker
  `data-pos` sulla prima cella di ogni riga).
- **`window.html`**: nuova regola `.sort-arrow` (cursore a mano, hover).
- Nessuna modifica a DOMPurify/ADR-013: il markup usa solo `<table>`/
  `<th>`/`<td>`/`<span>`/`data-*`, già passanti oggi col sanitizer di
  default — zero `onclick`/`on*` inline (impossibili comunque, DOMPurify
  li rimuove sempre).
- Lato Python (`scripts/pytools/financial-markets/screening.py`, fuori da
  questo crate — nessun CHANGELOG proprio): `_table` da pipe-table
  Markdown a `<table>` HTML grezzo con gli header a frecce e le celle
  `data-sort-value`/`data-pos` che questo meccanismo consuma.

Eseguito con **subagent-driven-development** (3 task, implementer Sonnet/
Haiku, review per-task + whole-branch review su Opus). 0 Critical/Important
sul codice in tutte le review; alcuni Minor loggati (repr float grezzo in
`data-sort-value`, nessun HTML-escaping di nome/ticker — comportamento
preesistente, innocuo per i dati reali; test di stabilità/locale-compare
non esaustivi; CSS `.sort-arrow` non scoped come le regole sorelle) — nessuno
compone in un problema più grave a livello di branch. **Smoke test dal vivo
in finestra Tauri reale ancora da fare con l'utente** (click reale →
riordino → rinumerazione Pos → valori n/d sempre in coda) — è la sola
verifica che DOMPurify v3.2.5 preservi davvero `data-sort-value`/`data-dir`/
`data-type`/`data-pos` in un webview reale, non solo per lettura del
codice.

Test: `node --test crates/ui/frontend/*.test.mjs` → 235/235 verdi (226
preesistenti + 9 nuovi in `table-sort.test.mjs`), nessuna regressione.

---

## [0.45.27] — 2026-07-21 — feat(ui,pytools): wiring frontend del canale python-ping

Task 3 dell'infrastruttura tool esterni Python (MCP) — vedi
`Docs/superpowers/specs/2026-07-21-pytools-infrastructure-design.md`. Terzo
consumatore del meccanismo canale esterno dopo `nmap`/`library-expand`
(Task 1-2, lato `orchestrator`, già registrano `"python-ping"`/`/pyping` in
`EXTERNAL_TOOL_CHANNELS`).

- `external-channels.js`: seconda voce nel registro frontend, mirror esatto
  di come `/nmap` è già collegato — `{ id: "python-ping", slashTrigger:
  "/pyping", windowTitle: "Lare — Python ping" }`. Nessuna modifica ad
  `app.js`: `findExternalChannelBySlash`/`open_external_channel_window` sono
  già generici, leggono il registro dinamicamente.
- Aggiunto lo script server MCP di prova consumato da `PythonMcpToolClient::
  resolve` (`pytools/python-ping/server.py`, un solo tool `pyping`) e la
  documentazione della convenzione venv per gli ambiti Python
  (`pytools/README.md`), fuori da questo crate ma parte dello stesso commit
  — vedi `pytools/README.md` per i dettagli (non hanno un proprio
  CHANGELOG, non sono un crate Rust).

Test: `external-channels.test.mjs` — nuovo test `EXTERNAL_TOOL_CHANNELS has
the python-ping entry` (RED confermato: prima falliva perché la voce non
esisteva) + il test sul conteggio del registro rinominato da "has exactly
the nmap entry" a "has the nmap entry" (non è più l'unica voce, stesso
principio già applicato lato Rust per
`production_registry_starts_with_nmap_channel`). Suite frontend completa
(`node --test crates/ui/frontend/*.test.mjs`): 225/225 verdi, nessuna
regressione.

---

## [0.45.26] — 2026-07-20 — fix(ui): tempo di lettura del messaggio d'errore in "Espandi"

Riscontro utente dallo smoke test dal vivo (Step 5b, chiave API invalida):
il messaggio d'errore mostrato da `failExpand` (0.45.25, ora include il
testo reale dell'errore orchestrator) spariva dopo 3s — troppo poco per
leggerlo per esteso. Il timeout di `failExpand` sale a 8s; il messaggio di
successo ("✓ Espanso", breve) resta a 3s.

---

## [0.45.25] — 2026-07-20 — fix(ui): mostra il motivo del fallimento in "Espandi"

Finding Important della review del fix 0.45.24: `failExpand` costruiva un messaggio
dettagliato (testo d'errore reale dell'orchestrator + conferma "documento non
modificato") ma lo scartava — `expandStatusEl` mostrava sempre lo stesso "✗ errore"
generico, il messaggio finiva solo in `console.error`. Ora `expandStatusEl` mostra
`message` per intero: su fallimento del turno AI l'utente vede il testo d'errore
reale dell'orchestrator, non un placeholder muto.

---

## [0.45.24] — 2026-07-20 — fix(ui): niente sovrascrittura silenziosa su fallimento del turno AI in "Espandi"

**Critical** trovato nella review finale whole-branch: `runExpand()` trattava
QUALUNQUE `Done` con buffer non vuoto come successo e chiamava
`archive_update`. I tre percorsi di fallimento lato orchestrator (errore
backend/API, rifiuto AI, risposta degenere — `ai_adapter.rs`) emettevano un
Chunk placeholder d'errore seguito dallo STESSO `Done{exit_code: None}` di
un successo vero: il placeholder, non vuoto, superava
`isEmptyExpandResult` e finiva scritto su disco al posto del documento
originale, senza backup. Fix lato orchestrator: `orchestrator` 0.40.35
(vedi il suo CHANGELOG) ora segnala questi tre casi con
`Done{exit_code: Some(1)}`.

Nuova funzione pura `isAiTurnFailure(exitCode)` in `expand-prompt.mjs`
(`exitCode !== null && exitCode !== undefined`) — segue la convenzione già
in uso in `renderer.js` (cursor UI) che branch-a su `exitCode === null` per
decidere se un `Done` è una risposta AI pulita. `window.js`'s `onMessage`
la controlla PRIMA di `isEmptyExpandResult`; su fallimento, `failExpand`
mostra il testo d'errore reale e dichiara esplicitamente che il documento
non è stato toccato (prima mostrava solo "✗ errore", troppo generico).
4 test nuovi in `expand-prompt.test.mjs` (13 totali).

Minor: rimossa `expandBarEl` (`window.js`, riga 42) — costante dichiarata e
mai usata altrove nel file.

---

## [0.45.23] — 2026-07-20 — fix(ui): niente retry silenzioso né sovrascrittura vuota nel flusso "Espandi"

Due bug trovati nella review del task che ha introdotto "Espandi" (0.45.22).
`LareWsClient` ritenta all'infinito su una connessione persa (backoff
esponenziale, nessun tetto) — corretto per il cursore principale, sbagliato
per questa richiesta one-shot: senza gestirlo, una connessione persa lasciava
la UI bloccata su "espando…" per sempre, oppure una riconnessione automatica
successiva rimandava in silenzio LO STESSO prompt come un turno AI nuovo di
zecca. Inoltre, un turno completato con risposta vuota (0 chunk) sovrascriveva
il documento con un file vuoto, senza alcun backup. Entrambe le decisioni ora
vivono in due funzioni pure testate (`isConnectionFailureStatus`,
`isEmptyExpandResult`, `expand-prompt.mjs`) invece che inline in `window.js` —
segue la convenzione di progetto ("la logica pura del frontend si estrae ed
si testa", CLAUDE.md), non rispettata dal primo passaggio della fix. 7 test
nuovi (9 totali in `expand-prompt.test.mjs`).

---

## [0.45.22] — 2026-07-20 — feat(ui): pulsante "Espandi" nei documenti Library

Chiude la feature "espandi documento" (`Docs/superpowers/specs/2026-07-20-
library-expand-design.md`). Un documento riaperto dalla Library (`data.
source_file` presente, Task 2) mostra una barra con campo testo + pulsante
"Espandi" al posto del 💾 (nascosto, come già oggi). Click → apre una
connessione WS effimera verso il canale orchestrator `"library-expand"`
(Task 3), manda documento+richiesta come un unico turno (`expand-prompt.
mjs`, modulo puro testato), riceve lo streaming, e su `Done` sovrascrive il
file (`archive_update`, Task 1) e ri-renderizza la finestra in loco — niente
nuova finestra. Ogni click riparte da zero (nuova connessione, nessuna
history accumulata: si rimanda sempre l'intero documento aggiornato).
2 test nuovi (`expand-prompt.test.mjs`).

---

## [0.45.21] — 2026-07-20 — feat(ui): le finestre Markdown ricordano il rel-path Library di provenienza

Prerequisito Task 2 per "espandi documento". Oggi una finestra riaperta
dalla Library non sa più da quale file proviene — `open_markdown_window`
riceveva solo `title`/`content`/`kind`, il rel-path si perdeva
all'apertura. `WindowContentStore` guadagna un 4° campo `source_file`
(stringa vuota se la finestra non viene dalla Library — nessun comando
Tauri in questo progetto usa ancora `Option<T>`, si preferisce l'idioma
già in uso per `kind`); `take_window_content` lo restituisce nel JSON.
I 3 call site esistenti (`app.js`, `external-channel-window.js`,
`library.js`) aggiornati — i primi due passano sempre `""`, `library.js`
passa il rel-path reale già in mano a `openMarkdownItem`.

---

## [0.45.20] — 2026-07-20 — feat(ui): `archive::update` — sovrascrittura in-place di un documento Library

Prerequisito per la feature "espandi documento" (`Docs/superpowers/specs/
2026-07-20-library-expand-design.md`): `archive::save` genera sempre un
nuovo filename con timestamp, non esisteva un modo di sovrascrivere un
documento ESISTENTE per rel-path. Nuova `archive::update(dir, file, title,
content)` — stessa guardia anti-traversal (`validate_within_root`) di
`open`/`delete`, ma a differenza di `save` fallisce se il file non esiste
già (non è una create-or-overwrite). Nuovo comando Tauri `archive_update`.
5 test nuovi (overwrite, filename invariato, sottocartella, file
inesistente → Err, traversal rifiutato).

---

## [0.45.19] — 2026-07-19 — chore(ui): rimuove il parametro `activeTagName` mai letto in `restoreActiveField`

Review finale del branch plugin crittografia (minor #2): `activeTagName`
era documentato nella JSDoc di `restoreActiveField` (`plugin-runtime.mjs`)
e popolato dal chiamante (`plugin-window.js::render()`), ma mai letto nel
corpo della funzione — solo il `tagName` del NUOVO elemento trovato viene
controllato (`next.tagName !== "INPUT" && ... !== "TEXTAREA"`), mai
confrontato con quello vecchio. Rimosso per YAGNI (nessun comportamento
cambia, il campo era morto):
- `plugin-runtime.mjs`: tolta la riga `activeTagName` dalla JSDoc di
  `restoreActiveField`.
- `plugin-window.js::render()`: tolta la costruzione della proprietà
  `activeTagName: active.tagName` nell'oggetto `preserved` (il gate che
  decide SE costruire `preserved` — `active.tagName === "INPUT" ||
  "TEXTAREA"` — resta invariato, era già lì per un altro motivo).
- `plugin-runtime.test.mjs`: tolto `activeTagName: "..."` dai 7 oggetti
  `preserved` passati a `restoreActiveField` nei test (il campo non era
  mai asserito, solo passato in ingresso; l'ottavo test di
  `restoreActiveField`, il caso `preserved` nullo/undefined, non passa
  nessun oggetto e quindi non aveva `activeTagName` da rimuovere).

Nessun comportamento cambiato. La voce CHANGELOG di 0.45.18 (sotto) cita
ancora `activeTagName` nella forma dell'oggetto valida a quel momento —
è un record storico, non riscritta qui.

Nessuna regressione: `node --test crates/ui/frontend/*.test.mjs` →
211/211 verdi (stesso conteggio, solo campo morto rimosso dai fixture di
test, nessun nuovo test necessario — refactor puro, non nuovo
comportamento).

---

## [0.45.18] — 2026-07-19 — fix(ui): plugin field loses live-typed characters on every re-render

Bug osservato dal vivo nel pannello "Testo in chiaro" del plugin crittografia
(smoke test, Task 11): digitando, ogni carattere appariva per un istante e
spariva — restava sempre visibile un solo carattere alla volta. Causa: ogni
tasto premuto fa un round-trip completo attraverso il sidecar plugin (nessun
patching parziale del DOM — `render()` sostituisce SEMPRE l'intera finestra
tramite `sanitizeAndRender`). Il meccanismo esistente in `render()`
(costruito per un bug precedente — "il campo perde il focus mentre scrivo")
preservava già focus+selectionRange dell'elemento a fuoco dopo la
sostituzione, ma NON il `.value`: il campo veniva riscritto col valore
ritornato dal round-trip, che riflette sempre lo stato di UN TASTO PRIMA
rispetto a quanto l'utente aveva già digitato localmente — ogni tasto
successivo, più veloce del round-trip, veniva quindi sistematicamente
sovrascritto.

- Nuova funzione pura `restoreActiveField(preserved)` in `plugin-runtime.mjs`:
  estrae in un modulo testabile con `node:test` (nessun accesso a
  `window`/`document`/`CSS.escape`) la logica — prima inline in `render()` —
  che ritrova, dentro un `newRootElement` iniettato, l'elemento con lo stesso
  `data-evt` di quello che aveva il focus e gli ripristina **valore → focus →
  selectionRange**, in quest'ordine esatto (scrivere `.value` può
  normalizzare/troncare una selectionRange impostata prima, quindi l'ordine
  è parte del contratto, non un dettaglio). Il valore ripristinato è quello
  LIVE, catturato da `active.value` in `render()` PRIMA di `sanitizeAndRender`
  — non quello (in ritardo di un tasto) presente nell'HTML fresco. Usa un
  escape locale per il valore dentro il selettore `[data-evt="…"]` (solo
  virgoletta e backslash) invece del `CSS.escape` del browser, che non esiste
  come global in Node — necessario per restare puro e testabile.
- `plugin-window.js`: `render()` cattura ora anche `active.value` insieme a
  `{activeTagName, activeDatasetEvt, activeStart, activeEnd}` PRIMA di
  `sanitizeAndRender`, poi chiama `restoreActiveField({ ...preserved,
  newRootElement: pluginRoot })` al posto della logica di ripristino inline
  precedente (solo focus+selectionRange). Riguarda SOLO il campo che aveva
  il focus — un'altra textarea nella stessa finestra (es. "Testo cifrato"
  mentre l'utente digita in "Testo in chiaro") continua a ricevere il
  valore fresco dal plugin normalmente, invariata: `restoreActiveField`
  tocca esclusivamente l'elemento trovato per il singolo `data-evt`
  catturato.
- 8 nuovi test in `plugin-runtime.test.mjs` per `restoreActiveField`: valore
  live sopravvive al re-render, focus+selectionRange ripristinati, ORDINE
  valore-prima-di-selectionRange verificato esplicitamente, no-op su
  `preserved` nullo/`activeDatasetEvt` mancante/elemento non trovato/elemento
  non INPUT-TEXTAREA, escaping difensivo di una virgoletta nel `data-evt`.
  Verificato RED (import falliva: l'export non esisteva) → GREEN.
- `node --test crates/ui/frontend/*.test.mjs`: 211/211 verdi (203
  pre-esistenti + 8 nuovi, nessuna regressione).

**Nota sulla verifica:** questo fix non è stato eseguito dentro una vera
finestra Tauri in questa sessione (l'obiettivo del task era la sola
piattaforma; lo smoke test dal vivo con `/crypto` è pianificato come step
successivo, dopo Task 10). La copertura è: (1) i test uniti sopra esercitano
la logica pura di ripristino con lo stesso contratto usato da `render()`
(stessa forma di oggetto `preserved`, stesso ordine di operazioni); (2) la
lettura del codice conferma che `render()` chiama `restoreActiveField` con
esattamente i campi previsti e che nessun altro elemento della finestra è
toccato (l'unica query è per il singolo `data-evt` catturato).

## [0.45.17] — 2026-07-19 — fix(ui): plugin runtime treats TEXTAREA as value-bearing

`eventFromTarget` in `plugin-runtime.mjs` ora include `TEXTAREA` accanto a
`INPUT` e `SELECT` come elementi "value-bearing" — ossia elementi il cui
`.value` deve essere incluso nell'evento inviato all'host Tauri. Scoperto
durante la pianificazione del plugin-crypto (i due pannelli di testo
chiaro/cifrato sono `<textarea>` per gestire testo multi-riga) ma è un fix
di piattaforma generico: qualunque plugin futuro con un campo di testo
multi-riga avrebbe lo stesso problema.

- Modifica `isValueBearing` in `plugin-runtime.mjs` per includere `TEXTAREA`.
- Aggiorna commenti nel file per documentare il supporto di TEXTAREA.
- Nuovo test in `plugin-runtime.test.mjs`: verifica che un `<textarea>` con
  valore multi-riga restituisca `{ element_id, value }` correttamente.
- `node --test crates/ui/frontend/*.test.mjs`: 203/203 verdi (nessuna
  regressione).

## [0.45.16] — 2026-07-19 — fix: Tab-completion perdeva la sottocartella digitata

Secondo bug trovato dalla review del fix 0.45.15 (quote-handling), non
segnalato dall'utente ma reale e riproducibile: `list_path_completions`
torna sempre e solo il basename (mai `dirPart`), ma entrambi i rami del
completamento sostituivano l'INTERO span del token — che include
`dirPart` — con il solo candidato, facendo sparire in silenzio qualsiasi
sottocartella già digitata. Esempio: `cd Sub\Fi` + Tab completava a
`cd File.txt` invece di `cd Sub\File.txt` — un file potenzialmente
SBAGLIATO se in cwd esiste anche un `File.txt` diverso da quello dentro
`Sub\`.

- Nuova funzione pura `buildCompletionText(dirPart, candidate, quoteChar)`
  in `path-utils.js`: ri-antepone `dirPart` al candidato prima di applicare
  l'eventuale avvolgimento tra virgolette. Usata da ENTRAMBI i rami del
  completamento (primo inserimento e ciclo successivo) in `app.js`, così i
  due punti non possono più divergere tra loro (come già successo con la
  logica di avvolgimento delle virgolette in 0.45.15, duplicata inline nei
  due rami prima di questo fix).
- `tabCompletion` guadagna il campo `dirPart` (accanto a `quoteChar`, già
  presente) — portato attraverso i cicli allo stesso modo.
- Rimosso un test duplicato in `path-utils.test.mjs` (`splitPathToken`
  testato due volte con lo stesso identico input/atteso) — Minor trovato
  dalla stessa review.
- 4 nuovi test per `buildCompletionText` (nessun dirPart, dirPart
  ri-anteposto, virgolette che avvolgono dirPart+candidato insieme,
  entrambi combinati). `node --test crates/ui/frontend/*.test.mjs`:
  202/202 verdi.

## [0.45.15] — 2026-07-19 — fix: Tab-completion su percorso tra virgolette

Bug trovato dal vivo dall'utente subito dopo l'introduzione del Tab-completion
(v0.45.14): nella cartella Documenti, digitando `cd "My` (virgoletta iniziale
per gestire un nome di cartella con spazi, es. "My Documents") e premendo Tab,
il completamento non funzionava — l'esito riportato era un ciclo apparente su
tutte le voci della directory ignorando il prefisso, con un inserimento
corrotto tipo `cd "Ny My Kindle Content"`.

**Causa confermata tracciando le funzioni pure reali** (`path-utils.js`)
sull'input esatto segnalato (`text = 'cd "My'`, caret a fine stringa):
`wordBounds` tratta `"` come un carattere qualsiasi non-spazio, quindi il
token estratto è `'"My'` — la virgoletta iniziale finisce dentro `prefix` e
viene spedita così a `list_path_completions`. Nessun nome di file reale
inizia per `"`, quindi il prefisso non trova mai corrispondenze: la gestione
delle virgolette era semplicemente **assente dal design**, non una
regressione di qualcosa che prima funzionava.
(Nota di onestà: il sintomo esatto "cicla su tutto ignorando il prefisso"
descritto dall'utente non si spiega interamente con questa sola traccia — un
prefisso `'"My'` a zero corrispondenze dovrebbe rendere il primo Tab un
no-op silenzioso, non un ciclo attivo sbagliato. La spiegazione più probabile
è una build non ricompilata/riavviata tra merge e test (il watcher di
`cargo tauri dev` non sempre cattura al volo un salvataggio). Non
approfondito oltre — ciò che resta comunque confermato e reale, a prescindere
da quale sintomo esatto l'utente abbia visto, è che un prefisso con
virgoletta iniziale non ha mai potuto corrispondere a nulla.)

- `splitPathToken` guadagna un terzo campo nel valore di ritorno,
  `quoteChar` (`"` , `'` o `null`): se il token inizia con una virgoletta,
  viene tolta prima di calcolare `dirPart`/`prefix` (così il match torna a
  funzionare) e restituita separatamente. Solo la virgoletta **iniziale** è
  gestita — una virgoletta di chiusura già digitata dall'utente resta fuori
  scope (non è il caso segnalato).
- `app.js`: sia il ramo di inserimento iniziale sia quello di ciclo Tab
  successivo ora avvolgono il candidato in `quoteChar` (quando presente)
  prima di passarlo a `replaceRange` — `quoteChar + candidate + quoteChar`
  invece del nome nudo. `quoteChar` è salvato dentro `tabCompletion` e
  riportato ad ogni ciclo, così tutti i candidati di una stessa sessione di
  Tab vengono avvolti in modo coerente, non solo il primo.
- 4 test esistenti di `splitPathToken` aggiornati (guadagnano
  `quoteChar: null` nell'atteso — nessuno di questi casi coinvolge una
  virgoletta) + 4 nuovi test per la regressione: virgolette doppie/singole
  iniziali, virgoletta iniziale con sottocartella parziale, e il caso senza
  virgoletta a conferma che il comportamento esistente resta invariato.

## [0.45.14] — 2026-07-19 — Tab-completion di file/percorso sulla riga di comando

Richiesto dall'utente: premendo Tab sulla riga di comando principale, la
parola sotto il caret viene completata a nomi di file/cartella — ma solo
se è un **argomento** (non la prima parola della riga, che è il nome del
comando/cmdlet, fuori scope). Comportamento di riferimento: quello di
PowerShell (la shell effettivamente pilotata via `mcp-server`), non bash —
Tab ripetuto **cicla in avanti** fra tutti i candidati uno alla volta,
Shift+Tab cicla all'indietro (niente "completa al prefisso comune + lista
al secondo Tab").

- `path-utils.js` guadagna 4 funzioni pure nuove: `wordBounds` (i confini
  della parola toccata dal caret), `isArgumentPosition` (vero se la parola
  non è la prima della riga), `splitPathToken` (separa un token tipo
  `C:\Users\Doc` in `dirPart`/`prefix` da completare) e `replaceRange`
  (sostituzione di un intervallo di testo con caret ricalcolato — usata sia
  per l'inserimento iniziale sia per ogni ciclo successivo di Tab).
- Nuovo comando Tauri `list_path_completions(cwd, dirPart, prefix)` in
  `src-tauri/src/main.rs`: legge la directory (`cwd` unito a `dirPart` via
  `Path::join`, che gestisce già `..`/relativi/assoluti) e ritorna i nomi
  che iniziano per `prefix` (case-insensitive su Windows, case-sensitive
  altrove — coerente con la semantica del filesystem sottostante). Le
  directory nel risultato hanno un separatore finale (`MAIN_SEPARATOR`)
  per distinguerle dai file senza una seconda chiamata. Nessun errore mai
  ritornato al frontend: directory inesistente/inaccessibile → lista vuota
  (un Tab senza corrispondenze deve restare silenzioso).
- `app.js`: nuova variabile di modulo `currentCwd` (il path grezzo, popolato
  da ogni `ServerMsg::Cwd` — finora si teneva solo la versione *formattata*
  in `cwdLineEl.textContent`, con la sostituzione `~`, inutilizzabile per
  risolvere percorsi relativi) e `tabCompletion` (stato del ciclo Tab in
  corso: range da sostituire, lista candidati, indice corrente — azzerato
  da qualsiasi tasto diverso da Tab, così un ciclo sopravvive solo a Tab
  consecutivi). Il fetch dei candidati è asincrono (invoke del comando
  Tauri): se l'utente digita altro mentre la richiesta è in volo, il
  risultato viene scartato invece di sovrascrivere quanto digitato nel
  frattempo.
- 22 nuovi test `node:test` in `path-utils.test.mjs` (le 4 funzioni pure) +
  4 nuovi test `cargo test` in `main.rs` (`list_path_completions_tests`:
  case-insensitive su Windows, separatore finale su directory, directory
  inesistente → vuoto senza panico, `dirPart` relativo unito a `cwd`).

## [0.45.13] — 2026-07-18 — scrollbar della finestra di canale

Trovato dal vivo: `external-channel.html` (finestra `/nmap`) non aveva la
regola `scrollbar-width`/`::-webkit-scrollbar` che tutte le altre finestre
(`index.html`, ecc.) hanno — la barra di scorrimento restava quella di
default del browser, bianca, fuori stile. Aggiunta la stessa regola
(colore `rgba(80, 140, 220, 0.4)`, 5px, thin) a `#output-area`.

## [0.45.12] — 2026-07-18 — numero di versione nel log di avvio

Richiesto dall'utente: `orchestrator` stampa già la propria versione
all'avvio (`tracing::info!("Lare Terminal orchestrator v{VERSION}
starting")`), `ui` no — impossibile capire a colpo d'occhio quale build
gira senza controllare `Cargo.toml`. Il banner finale di `main.rs`
("Lare Terminal started...") ora include `v{CARGO_PKG_VERSION}`.

## [0.45.11] — 2026-07-18 — indicatore di elaborazione nella finestra di canale

Trovato dal vivo: uno scan nmap può impiegare fino a 15 minuti
(`NMAP_CALL_TIMEOUT_SECS = 900` in `nmap_tool_client.rs`) prima di
rispondere, e la finestra di canale non aveva alcun segnale visivo che
distinguesse "sto elaborando" da "è bloccata". Aggiunto un indicatore
minimale — spinner braille + "elaborando…" — in una nuova riga
`#status-bar` (contiene sia il nuovo `#activity-indicator` sia il badge di
stato connessione preesistente `#status`, ora spostati fuori da un div
condiviso per non farsi sovrascrivere a vicenda via `textContent`). Versione
ridotta dell'activity indicator di `app.js`: un solo stile fisso (non 3
configurabili), nessun watchdog (il timeout vero è lato server via
`tokio::time::timeout`, questa finestra non ha un pulsante di stop).
Nessun test dedicato — stessa convenzione DOM-dependent del resto di questo
file (`cwd`/`tool_confirm_request`, mai unit-testati).

## [0.45.10] — 2026-07-18 — rimosso salvataggio automatico in Library nelle finestre di canale

`external-channel-window.js`: il case `save_to_library` sparisce da
`handleServerMsg` (il messaggio stesso non esiste più — vedi `protocol`
0.14.6). Restava solo il pulsante "Salva" preesistente in ogni finestra
Markdown (`window.js`), che già funzionava correttamente da solo — il
salvataggio automatico duplicava quel salvataggio, producendo 2 voci
identiche in Library alla stessa ora/minuto quando l'utente cliccava anche
il pulsante manuale.

## [0.45.9] — 2026-07-18 — Canale esterno: titolo in-pagina dinamico per canale

Trovato durante il primo smoke test dal vivo del canale `/nmap`: la finestra
mostrava sempre "CANALE ESTERNO" invece di "Lare — nmap" nello span
`#titlebar-label`. Causa: `open_external_channel_window` chiama `.title()`
sul `WebviewWindowBuilder` (imposta il titolo nativo del SO, visibile in
Alt-Tab/taskbar), ma la finestra ha `decorations(false)` — nessuna titlebar
nativa visibile, quindi l'unico titolo che l'utente vede davvero è lo span
in pagina, mai aggiornato dinamicamente (hardcoded "Canale esterno" in
`external-channel.html`). Fix: `initialization_script` ora inietta anche
`window.__LARE_EXT_CHANNEL_TITLE__` (accanto a `__LARE_EXT_CHANNEL_ID__`
già esistente); `external-channel-window.js` lo scrive in
`#titlebar-label.textContent` all'avvio, prima di connettersi.

## [0.45.8] — 2026-07-16 — Canale nmap: CSS banner di conferma + report → finestra/Library

`external-channel.html` guadagna lo stile di `.confirm-banner`/`.confirm-buttons`/
`.cmd-echo` (era assente — il banner era l'unico punto dove l'utente vede il
target di uno scan prima di un prompt UAC, deferred item dallo spec base).
`external-channel-window.js` gestisce `open_window` (apre una finestra
Markdown col report) e `save_to_library` (salva una copia in Library,
`ServerMsg::SaveToLibrary`, senza il protocollo ack di Share). `external-channels.js`
registra il primo canale reale (`nmap`).

## [0.45.7] — 2026-07-16 — Finestra generica "canale esterno" (prima finestra con WS propria)

### Added
- `external-channel.html`/`external-channel-window.js`: finestra generica
  parametrizzata da `channel_id`/`window_title`, apre la **propria**
  `LareWsClient` (`Hello{channel}`) — prima finestra secondaria a farlo; tutte
  le altre (config/library/plugin-*/aichat) parlano al backend solo via IPC
  Tauri relayato da `app.js`. Riusa `LareRenderer` (Chunk streaming, banner di
  conferma) invariato.
- `ws-client.js`: `LareWsClient` accetta un `channel` opzionale, incluso in
  `Hello` (`undefined` per la connessione del cursore → wire identico a oggi).
- Tauri: `open_external_channel_window(channel_id, window_title)` (singleton
  per canale, label `extchannel-<id>`) + capability `extchannel-window.json`.
- `app.js`: `handleSlashCommand` intercetta i trigger di `EXTERNAL_TOOL_CHANNELS`
  (vuoto in questa release) prima dell'invio al backend, stesso pattern di
  `/config`/`/library`/`/aichat`.

Nessun canale reale registrato ancora: questo branch non scatta in produzione.
Verifica e2e (apertura reale della finestra) deferita al piano `mcp-nmap`, che
fornisce il primo trigger reale — coerente con lo spec (§7: nessun tool reale
in questo spec).

## [0.45.6] — 2026-07-16 — `external-channels.js` (registro canali tool esterni, vuoto)

### Added
- `external-channels.js`: `EXTERNAL_TOOL_CHANNELS` (vuoto) + `findExternalChannelBySlash(input, registry)`,
  mirror puro di `orchestrator::external_channel` — `node:test`, 3 test verdi.

Nessun canale reale registrato ancora — nessun comportamento visibile cambia.
Prerequisito per la finestra generica di canale esterno (release successiva).

## [0.45.5] — 2026-07-15 — Banner di conferma per il gate locale per-tool

### Added
- `renderer.js`: `confirmBanner(id, commands, onDecision)` — banner inline nel
  pannello (`#output-area`, nessuna finestra nuova) con `[Esegui]`/`[Annulla]` per
  `ServerMsg::ToolConfirmRequest`; disabilita i pulsanti dopo il primo click.
- `ws-client.js`: `sendToolConfirmResponse(id, accept)` → `ClientMsg::ToolConfirmResponse`.
- `app.js`: case `tool_confirm_request` nello switch di `handleServerMsg`.
- CSS `.confirm-banner`/`.confirm-buttons` in `index.html` (bordo ambra, coerente
  con la palette esistente di `.cmd-block`/`.sys-msg`).

Nessun tool AI è ancora marcato "sensibile" (`SENSITIVE_TOOLS` vuoto lato
orchestrator, `local_confirm.rs` 0.40.14) — il banner non compare ancora dal vivo,
ma la catena end-to-end (protocollo → orchestrator → UI) è completa. Chiude
`Docs/superpowers/specs/2026-07-15-local-tool-confirm-gate-design.md`; il
prossimo consumatore reale sarà il server MCP `nmap` (spec separato).

## [0.45.4] — 2026-07-14 — AI Chat non apre più da sola all'avvio

### Changed
- La finestra AI Chat si apriva **da sola** subito dopo l'avvio di `ui.exe`, anche senza che
  l'utente la chiedesse. Causa: l'orchestrator manda `ServerMsg::AiChatSelf` in modo
  incondizionato ad ogni nuova connessione WS (`SetServerTx` in `aichat/service.rs`, serve al
  roster di Library "Share with" indipendentemente dalla finestra-chat); nel frontend,
  `pushAiChat` reagiva a QUALSIASI evento `ai_chat_*` aprendo la finestra da solo se non era
  già aperta o in apertura — comodo in fase di test iniziale (verificare il collegamento tra
  macchine), ma rumoroso in uso normale.
- **Fix (solo frontend):** l'apertura è ora **solo esplicita**, tramite il comando `/aichat`
  (già esistente). Un evento `ai_chat_*` che arriva mentre nessuno ha chiesto la finestra viene
  **scartato** invece di aprirla; se l'apertura è in corso (`/aichat` già digitato, webview non
  ancora pronta) l'evento resta bufferizzato come prima (buffer-and-replay, nessuna perdita).
- Logica di decisione (`emit`/`buffer`/`drop`) estratta in modulo puro
  `crates/ui/frontend/aichat-push.js` (`createAiChatPushGate`), 6 test `node:test` nuovi
  (`aichat-push.test.mjs`). `app.js` ridotto a chiamare `aiChatGate.push/requestOpen/ready/closed`
  al posto delle tre variabili sciolte `aiChatLive`/`aiChatOpening`/`aiChatBuffer`.

## [0.45.3] — 2026-07-14 — Cascadia Mono Light: peso sottile del titolo esteso a tutto il testo

### Changed
- L'utente ha notato che il titolo delle finestre (es. "LARE COMMANDER") sembrava usare un
  font "più sottile" del contenuto sotto (es. i nomi delle cartelle in Library/Lare Commander).
  Indagine: nessun `font-weight` era mai stato impostato né su titlebar né su corpo — entrambi
  già alla stessa famiglia `'Cascadia Mono'` (0.45.2) e allo stesso peso Regular. L'effetto
  "sottile" del titolo veniva dal MAIUSCOLO + letter-spacing, non da un font diverso.
- Verificato (elenco font installati via `System.Drawing.Text.InstalledFontCollection`) che
  Windows installa ogni peso di Cascadia come famiglia SEPARATA, non come font variabile:
  `Cascadia Mono`, `Cascadia Mono Light`, `Cascadia Mono SemiBold`, ecc. — impostare
  `font-weight: 300` su `'Cascadia Mono'` non avrebbe avuto alcun effetto.
- Cambiato lo stack font app-wide da `'Cascadia Mono', 'Consolas', ...` a `'Cascadia Mono
  Light', 'Cascadia Mono', 'Consolas', 'Courier New', monospace` in tutte le 8 finestre —
  titlebar e corpo ereditano entrambi la stessa famiglia (nessuna proprietà separata), quindi
  ora sono genuinamente allo stesso peso sottile. Maiuscolo/letter-spacing della titlebar
  restano invariati (l'utente ha esplicitamente chiesto di non toccarli).

## [0.45.2] — 2026-07-14 — font/colore/dimensione testo unificati app-wide

### Changed
- L'utente ha notato dal vivo (screenshot: `/config` vs Lare Commander) che l'app aveva DUE
  convenzioni di font diverse: `'Consolas', 'Courier New', monospace` (cursore, Library, Markdown,
  Search, Lare Commander) vs `"Segoe UI", system-ui, sans-serif` (`/config`, AI Chat, shell
  condivisa dei plugin — quindi anche `/calc`/`/ping`/`/counter`). Scelto **`'Cascadia Mono'`**
  (monospace, più sottile di Consolas) come standard unico, con `'Consolas', 'Courier New',
  monospace` come fallback se non installato. Applicato a TUTTE le finestre: `index.html`
  (`--cursor-font`, default aggiornato), `config.html`, `aichat-window.html`, `plugin-window.html`,
  `library.html`, `window.html`, `window-search.html`, `plugin-catalog.css` (`.lare-window`).
- Allineati anche colore testo (`plugin-catalog.css`'s `--lare-text` da `rgba(230, 235, 245,
  0.95)` a `rgba(220, 235, 255, 0.92)`, lo stesso `--text` già condiviso da 6 finestre su 8) e
  dimensione (`.lare-window` da 14px a 13px, lo standard già in uso ovunque tranne il cursore,
  che resta configurabile via `--cursor-size`).

## [0.45.1] — 2026-07-13 — rimosso lo scarto relativo: alpha uniforme su ogni pannello

### Changed
- Correzione post-live-test: l'utente ha verificato dal vivo che lo "scarto relativo"
  introdotto in 0.45.0 (tab-bar/toolbar Library, dialog Sposta/Condividi, pannelli
  interni di Lare Commander a un alpha DIVERSO dal valore configurato in `/config`)
  produceva trasparenze visibilmente disomogenee tra le finestre — non quello che
  voleva. Rimossa la formula `clamp(0, calc(var(--window-alpha, 0.87) ± offset), 1)`
  da tutti i selettori che la usavano: ora OGNI sfondo pannello (in ogni finestra,
  plugin inclusi) usa `rgba(R, G, B, var(--window-alpha, 0.87))` — lo stesso identico
  valore configurato in `/config`, senza eccezioni.

## [0.45.0] — 2026-07-13 — trasparenza configurabile (`Config.window_alpha`)

### Added
- `Config.window_alpha` (default 0.87, range [0,1], clamp in lettura): trasparenza
  configurabile da uno slider nel tab UI di `/config`.
- Propagazione runtime a TUTTE le finestre (cursore, Library, `/config`, AI Chat,
  Markdown, Search, shell condivisa dei plugin): ogni finestra applica
  `--window-alpha` all'avvio (`get_config`) e la riapplica live al salvataggio
  di `/config` (evento globale Tauri `config:saved`, già esistente — oggi
  consumato anche da Library/AI Chat/plugin/Markdown/Search, non solo dal
  pannello principale).
- Sfondi con uno scarto storico rispetto a 0.87 (tab-bar Library, toolbar
  Library, dialog Sposta/Condividi) mantengono il proprio scarto relativo via
  `clamp(0, calc(var(--window-alpha, 0.87) ± offset), 1)`.

## [0.44.5] — 2026-07-13 — trasparenza alpha allineata a 0.87 in tutte le finestre

L'utente ha notato che i plugin (e `/config`) non avevano la stessa trasparenza della finestra
principale. Indagine: l'app aveva 4 valori alpha diversi sparsi tra le finestre (0.82 cursore,
0.86 config/aichat, 0.87 window/window-search, 0.92 shell-plugin+catalogo) — nessuno era davvero
"lo standard". Scelto **0.87** come riferimento unico (decisione utente), allineate TUTTE le
finestre in un solo commit.

### Changed
- `index.html` (cursore, finestra principale): `0.82` → `0.87`.
- `config.html`: `--bg` `0.86` → `0.87`. `config.css`'s `.config-dialog-box` (il box che di fatto
  riempie l'intera finestra `/config`): `0.98` → `0.87` — questo, non il body, era la vera causa
  del "niente alpha" percepito su `/config` (0.98 è quasi opaco, mascherava del tutto il body
  translucido sottostante).
- `library.html`: `0.77` → `0.87`.
- `aichat-window.html`: `0.86` → `0.87`.
- `plugin-window.html` (shell condivisa di TUTTI i plugin): `0.92` → `0.87`.
- `plugin-catalog.css`'s `--lare-bg` (default ereditato da `/calc`/`/ping`/`/counter` quando non lo
  sovrascrivono): `0.92` → `0.87`.
- `window.html`/`window-search.html`: già a `0.87`, nessuna modifica.

### Note
- `crates/plugin-lc`'s propria `.lc-root` (che sovrascrive il default del catalogo con un colore
  navy distinto) aggiornata nello stesso commit, versione crate separata (0.3.5) — vedi il suo
  CHANGELOG.
- Gli alpha degli strati INTERNI di ciascuna finestra (es. pannelli/dialog/toolbar di Library,
  `.lc-panels`/`.lc-panel`/header/funcbar di Lare Commander) NON sono stati toccati: sono scelte
  di leggibilità deliberatamente diverse dall'alpha "di finestra" (il contenitore più esterno),
  che è l'unico oggetto di questo allineamento.

## [0.44.4] — 2026-07-12 — finestre plugin: doppio-click opt-in + dimensione iniziale dichiarata

Estensioni condivise dell'infrastruttura finestra-plugin (usate da Lare Commander, disponibili a
ogni plugin futuro). Entrambe opt-in: i plugin che non le usano (calc/ping/counter) sono invariati.

### Added
- **Doppio-click delegato**: nuovo attributo HTML `data-dblevt` (parallelo a `data-evt`). `plugin-runtime.mjs` guadagna la funzione pura `eventFromDblTarget(target)`; `plugin-window.js` aggiunge UN listener `dblclick` delegato su `#plugin-root`. Un elemento con solo `data-evt` NON reagisce al doppio-click (attributi indipendenti). 5 nuovi test in `plugin-runtime.test.mjs`.
- **Dimensione iniziale finestra plugin**: il comando Tauri `open_plugin_window` guadagna i parametri `width: Option<f64>` / `height: Option<f64>`; se assenti (calc/ping/counter) si usa il default 480×360, altrimenti la dimensione richiesta dal plugin. `app.js` inoltra `msg.width`/`msg.height` da `open_plugin_window` (WS) al comando (assenti ⇒ `undefined` ⇒ omessi dall'IPC ⇒ `None` lato Rust).

### Unchanged
- Nessun impatto su calcolatrice/counter/ping: nessun `data-dblevt`, nessuna dimensione dichiarata ⇒ comportamento byte-per-byte identico.

## [0.44.3] — 2026-07-10 — Library: terzo tab "Plugins" (sola lettura)

Prima slice del tab Plugins nella finestra Library: elenca i plugin installati e mostra il
contenuto grezzo di ciascun `plugin.json` al click sulla riga. Nessun editing di configurazione,
nessuna finestra dedicata, nessuna cifratura — tutto fuori scope, vedi `IMPLEMENTATION.md`.

### Added

- `plugins_view.rs` (nuovo): `plugins_dir()` risolve la cartella plugin duplicando ESATTAMENTE la
  logica di `orchestrator/src/main.rs` (`LARE_PLUGINS_DIR` override, altrimenti
  `%LOCALAPPDATA%\dev.lare.terminal\plugins`, fallback `.lare-data/plugins`) — `ui` non dipende dal
  crate `orchestrator`, stesso pattern già usato da `llm_settings.rs` per `llms.json`.
  `scan_plugins(dir)` è il nucleo puro/testabile: per ogni sottocartella con un `plugin.json`
  leggibile produce una `PluginListEntry {id, name, manifest_json}`; a differenza di
  `orchestrator::plugins::discovery::discover` (che scarta i manifest invalidi prima di eseguirli),
  qui un manifest malformato o senza `id`/`name` NON viene scartato — l'entry usa il nome della
  cartella come fallback, perché questa è una vista diagnostica di sola lettura, non un percorso di
  spawn. Comando Tauri `list_plugins` registrato in `main.rs`. 6 nuovi test unitari (tempdir), RED
  confermato con uno stub `todo!()` prima dell'implementazione.
- `library.html`/`library.js`: terzo bottone `#tab-plugins` nel tab-bar (stesso pattern
  Markdown/Find), `loadPluginsTab()` + `buildPluginItem(entry)` — riga `.archive-item` (navigabile
  con le frecce, riuso della navigazione da tastiera generica già esistente) più un `<pre
  class="plugin-manifest">` sorella (NON `.archive-item`, per non essere trattata come riga
  navigabile) che si apre/chiude a click mostrando `manifest_json` via `textContent` (mai
  `innerHTML` — il file può contenere testo arbitrario).

Nessun nuovo test JS (`buildPluginItem`/`loadPluginsTab` sono DOM-wired come `buildFindItem`,
stessa convenzione del resto di `library.js`); 162 test `node --test` invariati. `cargo test -p ui`:
91+28 (28 include i 6 nuovi di `plugins_view`) tutti verdi, `cargo clippy -p ui --all-targets`
pulito.

## [0.44.2] — 2026-07-10 — Finestra ricerca: riga+snippet per gli hit di contenuto

La finestra `/find` mostra riga+frammento di testo quando l'hit proviene da una ricerca
**contenuto** (`/find in:"<frase>"`, orchestrator 0.40.8/protocol 0.14.1) — un hit solo-nome
continua a mostrare esclusivamente il path, **invariato**. `ServerMsg::SearchHit.line`/`.snippet`
erano già sul wire dal Task 1 del piano; questa slice li inoltra e li renderizza.

### Added

- `window-search.js`: `addHit(source, path, line, snippet)` — se `line`/`snippet` sono presenti,
  aggiunge una riga secondaria `"<numero>: <frammento>"` sotto il path (`div.row-snippet`, via
  `textContent`, mai `innerHTML` — stessa cautela già applicata al path). Passthrough anche nel
  replay di una ricerca salvata (Library → tab Find) e nel replay del buffer (`search:subscribe`).
- `app.js`: `search_hit` e il replay del buffer (`searchBuffers.hit`/`subscribe`) inoltrano
  `line`/`snippet` insieme a `path`/`source`, invariati per assenza (`undefined` → nessuna riga
  extra in `window-search.js`).
- `window-search.html`: CSS `.row-snippet` (monospace, muted, sotto il path).

Solo frontend: nessuna modifica a `protocol`/`orchestrator` in questa slice (i campi sul wire
erano già stati aggiunti a monte, Task 1 del piano). 162 test JS (`node --test
crates/ui/frontend/*.test.mjs`) invariati (nessun modulo puro nuovo — `addHit` resta imperativo/
DOM, stesso trattamento del resto di `window-search.js`).

## [0.44.1] — 2026-07-09 — Trasparenza finestre +10%

Sfondo (`--bg`/`.overlay-panel`) di tutte le finestre principali un po' più trasparente, su
richiesta esplicita dell'utente: cursore (`index.html` `.overlay-panel`) `0.92` → `0.82`, AI Chat
(`aichat-window.html`) `0.96` → `0.86`, `/config` (`config.html`) `0.96` → `0.86`, Library
(`library.html`) `0.87` → `0.77`. Stessa convenzione del precedente passaggio "+5%" (ui 0.6.1,
`0.92` → `0.87`): -0.10 di alpha assoluto, non un fattore moltiplicativo. Fuori scope: finestre
Markdown/plugin/ricerca (`window.html`/`plugin-window.html`/`window-search.html`, non nominate
dall'utente) e gli elementi interni di `/config` (`config.css`, box/pulsanti — decorazioni del
pannello, non lo sfondo della finestra). Nessun test (solo valori CSS, nessuna logica).

## [0.44.0] — 2026-07-07 — Tab "LLM" in `/config` (Slice 5 completa)

**Prima volta che l'utente può scegliere il provider AI attivo dalla UI**, invece di editare
`llms.json` a mano. Nuovo tab "LLM": elenca i provider già presenti nel file (nome + modello),
radio-list per scegliere `active`. **Sola selezione** — aggiungere/rimuovere provider o editare
le API key resta editing a mano del file (decisione di scope, vedi
`Docs/superpowers/specs/2026-07-07-llms-config-ui-tab-design.md` §1). Nessun `llms.json` → nota
informativa al posto della lista. Stesso limite non-live di AI Chat: richiede il riavvio
dell'orchestrator.

### Added

- `config-dialog.js`: tab "LLM" (`_buildLlmTab`/`_buildRadio`), wired in `open()`/`_buildPanel`/
  Save. Nessuna nuova classe CSS (riusa `config-field-row`/`config-note` esistenti).

## [0.43.1] — 2026-07-07 — `llm_settings.rs`: lettura/scrittura provider attivo (Slice 5 Task 2)

Nuovo modulo che legge `active`+`providers[].{name,model}` da `llms.json` (di proprietà
dell'orchestrator, in `%LOCALAPPDATA%\dev.lare.terminal\`) e scrive SOLO `active` — mai
`api_keys`, mai gli altri campi di un provider. Puramente additivo — nessun tab lo usa ancora
(Task 3).

### Added

- `llm_settings.rs`: `LlmProviderInfo`/`LlmSettings`, `get_llm_settings`/`set_llm_settings`
  (comandi Tauri). 7 test sulle funzioni pure sottostanti (`parse_llm_settings`/`merge_active`),
  incluso il test di sicurezza centrale: il merge di scrittura lascia `api_keys` byte-per-byte
  intatte.

## [0.43.0] — 2026-07-06 — Library "Share with": trasferimento contenuto (Slice 2a)

Il documento condiviso arriva DAVVERO nella Library del destinatario. Due nuovi gestori in
`app.js`: `"share_content_request"` (il mittente fornisce il contenuto letto con `archive_open`,
comando Tauri già esistente — nessun nuovo comando Rust) e `"share_incoming_data"` (il destinatario
scrive il documento con `archive_save`, già esistente, e mostra `📥 Ricevuto "..." da ..., salvato
in Library` nel pannello del cursore). Nuova funzione pura `shareReceivedLine` in `share-view.mjs`.
Fallimento del recupero contenuto (documento cancellato/spostato) riportato SUBITO al mittente
("fallita: documento non più disponibile"), non dopo la scadenza di 24h. Nessuna guardia
anti-cancellazione in questa slice (Slice 2b, deferred). Spec:
`Docs/superpowers/specs/2026-07-06-library-share-slice2a-content-transfer-design.md`.

## [0.42.0] — 2026-07-05 — Library "Share with": pulsante, dialog, banner, esito

UI per la Slice 1a backend (orchestrator 0.35.0/0.36.0): la feature "Share with"
diventa finalmente azionabile da un umano, non solo da test automatici. Spec:
`Docs/superpowers/specs/2026-07-05-library-share-ui-slice1a-design.md`.

**Ancora mancante** (fuori scope, vedi spec §1): nessun trasferimento di
contenuto reale (chi accetta non vede comparire alcun file — Slice 2), nessun
pulsante "Condividi con tutti" (backend risponde comunque "non disponibile").

### Added

- `share-view.mjs` (nuovo modulo puro, testato): `shareTargetList`,
  `formatShareSize`, `shareConsentPrompt`, `shareResultLine`.
- Library: pulsante 📤 "Condividi" per documento, dialog di scelta macchina
  (`#share-dialog`, stessa struttura del dialog "Sposta" esistente).
- AI Chat: banner di consenso `#share-consent` (elemento separato da `#consent`,
  stessa grammatica visiva `.gate-banner`).
- Pannello cursore: nuovo `renderer.systemMessage(text)` per notifiche non
  legate a un comando digitato — usato per l'esito Share ("📤 Condivisione di
  ... : accettata/rifiutata/fallita"), che può arrivare fino a 24h dopo.
- Ponte Library↔app.js: nuovi eventi Tauri globali `library:request-roster`/
  `library:roster`/`library:share-document` (nessun nuovo comando Tauri lato
  Rust — solo eventi frontend-to-frontend, come già usato per AI Chat/plugin).

### Known limitation

- Il banner di consenso Share (`#share-consent`) gestisce UNA offerta pendente alla volta
  (`pendingShare`, non una coda). Se arrivano due offerte da mittenti diversi prima che l'umano
  risponda alla prima, la seconda sovrascrive silenziosamente la prima nel banner — quella
  scavalcata resta comunque pendente lato backend fino alla sua scadenza naturale di 24h (il
  mittente riceve poi un esito "fallita"). Fallisce in modo sicuro (nessun crash, nessuna perdita
  di stato lato backend), solo differita. Il flusso di ammissione alla stanza (`admissionQueue`,
  stesso file) risolve lo stesso problema con una coda esplicita — da valutare se replicare qui
  in una slice futura, se osservato come problema reale nell'uso quotidiano.

## [0.41.2] — 2026-07-05 — /aichat aggiunto ai comandi cliccabili nella riga hint

- `index.html`: nuovo `<span class="hint-cmd" data-cmd="/aichat">` dopo `/library`, prima di
  `/help` — riusa 1:1 il delegato di click generico già esistente in `app.js`
  (`.hint-cmd[data-cmd]` → `handleSlashCommand`), che già gestiva `/config`/`/library`. Nessuna
  logica nuova: `/aichat` era già un comando valido digitabile (apre la finestra AI Chat).

## [0.41.1] — 2026-07-05 — AI Chat: testo avviso keepalive più naturale

Feedback utente durante i test dal vivo dell'ammissione a stanza: "`<label>` è sparito" suona
allarmante/gergale per un umano che legge la chat. Cambiato in "`<label>` risulta scollegato"
(stessa causa tecnica — keepalive scaduto o disconnessione — solo il testo cambia).

### Changed

- `aichat-view.mjs::peerLostText` — `"${label} è sparito"` → `"${label} risulta scollegato"`
  (fallback senza label: `"un peer risulta scollegato"`). 2 test aggiornati.

## [0.41.0] — 2026-07-03 — AI Chat: pulizia del banner gate 2 alla chiusura del voto (fix review #7)

Frontend del fix **#7** (`orchestrator` 0.34.0 / `protocol` 0.11.0): quando un voto di
ammissione si conclude altrove (veto di un altro presente, timeout, o uscita del candidato), il
server manda `ai_chat_admission_resolved` e la finestra chat toglie quel candidato dalla coda del
gate 2 — prima il banner "ammetti X?" restava appeso e il click era un no-op silenzioso.

### Added

- **`removeResolvedCandidate(queue, candidate)`** in `admission.mjs` (logica PURA, 5 nuovi test
  in `admission.test.mjs`): rimuove tutte le occorrenze del candidato risolto dalla coda
  (idempotente). `aichat-window.js` la usa nel nuovo listener `aichat:admission-resolved`,
  ridisegnando il banner solo se il candidato risolto era quello in testa.
- Dispatch di `ai_chat_admission_resolved` → evento `aichat:admission-resolved` in `app.js`.

## [0.40.0] — 2026-07-03 — AI Chat: ammissione alla stanza (finestra chat, 2 gate)

Frontend del redesign di ammissione (`orchestrator` 0.33.0 / `protocol` 0.10.0): sostituisce
il vecchio consenso pairwise (che perdeva il saluto del nuovo arrivato) col modello a 2 gate
con voto coordinato dal server eletto. Design:
`Docs/superpowers/specs/2026-07-03-aichat-admission-consent-design.md`.

### Added

- **`frontend/admission.mjs`** (nuovo modulo puro) — logica del flusso di ammissione senza
  DOM/Tauri:
  - `joinPromptText(present)`: testo del gate 1 mostrato al nuovo arrivato — "Vuoi entrare in
    chat con A, B e C?" (congiunzione italiana standard: virgole tranne l'ultimo elemento,
    introdotto da " e "); `present` vuoto → "Vuoi entrare in chat?".
  - `rerequestState(nowMs, retryAtMs)`: stato del pulsante "Chiedi di entrare" dato l'orologio
    corrente e la scadenza del cooldown (`retry_after_secs` da `AiChatRejected`) —
    `{ enabled, secondsLeft }`; `retryAtMs` assente o scaduto → abilitato; altrimenti
    disabilitato coi secondi rimanenti arrotondati per eccesso (mai un countdown a "0" prima
    dello scadere reale).
- **`frontend/admission.test.mjs`** (nuovo) — test `node:test` per `joinPromptText` (0/1/2/3
  elementi) e `rerequestState` (nessun cooldown, cooldown attivo, cooldown scaduto).
- **`frontend/ws-client.js`** — `sendAiChatJoinDecision(accept)` (`ClientMsg::AiChatJoinDecision`,
  risposta al gate 1), `sendAiChatAdmissionVote(candidate, accept)`
  (`ClientMsg::AiChatAdmissionVote`, risposta al gate 2 — voto di un presente),
  `sendAiChatRequestAdmission()` (`ClientMsg::AiChatRequestAdmission`, pulsante "chiedi di
  entrare" dopo un rifiuto).
- **`frontend/app.js`** — nuovi case nello switch `ServerMsg`: `"ai_chat_join_prompt"`,
  `"ai_chat_admission_request"`, `"ai_chat_pending"`, `"ai_chat_admitted"`,
  `"ai_chat_rejected"` → inoltrati alla finestra-chat via `pushAiChat`. Routing dei relativi
  eventi Tauri in ingresso (gate 1/gate 2/pulsante re-request) verso i rispettivi
  `sendAiChat*` sopra.
- **`frontend/aichat-window.js`** — gestori della finestra AI Chat per il nuovo flusso:
  - **gate 1** (`aichat:join_prompt`): mostra il prompt (testo da `joinPromptText`) con
    [Entra]/[Annulla] → `sendAiChatJoinDecision`.
  - **gate 2** (`aichat:admission_request`): mostra "`candidate` chiede di entrare" con
    [Ammetti]/[Rifiuta] a un presente già ammesso → `sendAiChatAdmissionVote`.
  - **pending** (`aichat:pending`): **blocca l'input** del nuovo arrivato finché non arriva
    esito — il fix del bug principale del vecchio consenso (il saluto scritto prima
    dell'ammissione andava perso).
  - **admitted** (`aichat:admitted`): sblocca l'input, annuncio "Sei entrato in chat."
    (con guardia anti-doppio-annuncio rispetto al diff del roster).
  - **rejected** (`aichat:rejected`): annuncio "Ingresso rifiutato.", pulsante "Chiedi di
    entrare" disabilitato per il cooldown (`rerequestState`, ri-renderizzato via timer finché
    non scade e si autodistrugge).

### Test

```
node --test crates/ui/frontend/*.test.mjs → 146 passed (nessuna regressione)
```

Nessun test Rust dedicato in questa slice (solo wiring frontend + protocol/orchestrator).

**E2e GUI da accettare dal vivo** (multi-macchina): vedi `Docs/TESTING-e2e.md` §H.

---

## [0.39.0] — 2026-07-02 — /config: flag "auto-partecipazione" (AI Chat Slice 2)

Aggiunge al tab "AI Chat" (0.37.0/0.38.0) un SECONDO flag, distinto da "la mia AI
partecipa" (0.38.0): l'auto-partecipazione fa intervenire l'AI SPONTANEAMENTE sui
messaggi normali della stanza (nessuna invocazione), giudicando da sola la rilevanza —
vedi `orchestrator` 0.32.0, design
`Docs/superpowers/specs/2026-07-02-aichat-ai-participants-design.md` §10.

### Added

- **`src-tauri/src/aichat_settings.rs`** — nuovo campo `pub ai_autoparticipate: bool`
  su `AiChatSettings` (5° campo, mirror di
  `orchestrator::aichat::config::AiChatConfig::ai_autoparticipate`). `get_aichat_settings`
  lo legge con default `false` — a DIFFERENZA di `ai_participates` (default `true`):
  l'auto-partecipazione è opt-in esplicito (file assente/corrotto O `aichat.json`
  scritto prima che il campo esistesse → `false`). `merge_aichat_settings` lo scrive
  preservando ogni altro campo esistente. 3 nuovi test:
  `get_settings_reads_ai_autoparticipate_with_false_default`,
  `missing_ai_autoparticipate_field_reads_as_false`,
  `merge_round_trip_preserves_ai_autoparticipate`; gli 8 struct-literal `AiChatSettings`
  esistenti nel modulo aggiornati al nuovo 5° campo.
- **`frontend/config-dialog.js`** — nel tab "AI Chat" (`_buildAiChatTab`), nuova
  checkbox "Auto-partecipazione (l'AI interviene da sola)" (`_buildCheckbox`, id
  `config-aichat-autoparticipate`, default **UNCHECKED** — a differenza del checkbox
  "la mia AI partecipa", che è default checked). Letta/salvata come gli altri campi
  aichat (`newAiChat.ai_autoparticipate` nel salvataggio di `/config`); default locale
  `aichat.ai_autoparticipate ?? false` se il backend non è raggiungibile.

### Notes

- Nessun nuovo test JS dedicato: `config-dialog.js` non ha un modulo `*.test.mjs`
  (stesso motivo già documentato in 0.38.0) — la copertura del round-trip è lato Rust
  (`aichat_settings.rs`), la UI è wiring diretto.

### Verifica

```
cargo test -p ui   → 91 passed (lib) + 15 passed (bin, incl. 3 nuovi test) + 0 doc-tests
cargo clippy -p ui --all-targets → pulito, nessun warning
node --test crates/ui/frontend/*.test.mjs → 138 passed (nessuna regressione)
```

---

## [0.38.0] — 2026-07-02 — /config: flag "la mia AI partecipa" (AI Chat Slice 1b)

Aggiunge al tab "AI Chat" (0.37.0) il flag lato macchina che gata le invocazioni
AI **remote** (`@all`/`@<mio-label>-ai` da un umano di un'altra macchina — vedi
`orchestrator` 0.31.0, design
`Docs/superpowers/specs/2026-07-02-aichat-ai-participants-design.md` §9.4).

### Added

- **`src-tauri/src/aichat_settings.rs`** — nuovo campo `pub ai_participates: bool`
  su `AiChatSettings` (4° campo, mirror di
  `orchestrator::aichat::config::AiChatConfig::ai_participates`). `get_aichat_settings`
  lo legge con default `true` (stesso pattern infallibile degli altri 3 campi — file
  assente/corrotto O `aichat.json` scritto prima che il campo esistesse → `true`).
  `merge_aichat_settings` lo scrive preservando ogni altro campo esistente (stesso
  comportamento già testato per gli altri 3). `validate` invariato (un `bool` non
  richiede validazione). 2 nuovi test: `get_settings_reads_ai_participates_with_true_default`,
  `merge_round_trip_preserves_ai_participates`; i 6 test esistenti aggiornati al
  nuovo 4° campo di `AiChatSettings`.
- **`frontend/config-dialog.js`** — nel tab "AI Chat" (`_buildAiChatTab`), nuova
  checkbox "La mia AI partecipa (risponde alle richieste della stanza)"
  (`_buildCheckbox`, id `config-aichat-participates`, default checked). Letta/salvata
  come gli altri campi aichat (`newAiChat.ai_participates` nel salvataggio di
  `/config`); default locale `aichat.ai_participates ?? true` se il backend non è
  raggiungibile (stesso fallback già usato per gli altri campi).

### Notes

- Nessun nuovo test JS dedicato: `config-dialog.js` non ha un modulo `*.test.mjs`
  (è DOM/Tauri-dipendente, come gli altri campi del tab AI Chat già esistenti) — la
  copertura del round-trip è lato Rust (`aichat_settings.rs`), la UI è wiring diretto.

---

## [0.37.0] — 2026-07-02 — /config: tab "AI Chat"

### Added

- **`src-tauri/src/aichat_settings.rs`** (nuovo) — mirror di `search_settings.rs`
  per i 3 campi di `aichat.json` (`enabled`, `label_base`, `chat_port`):
  `AiChatSettings`, `merge_aichat_settings` (preserva ogni altro campo esistente
  nel file, ripartendo da `{}` se assente/corrotto), `aichat_json_path()`
  (`%LOCALAPPDATA%\dev.lare.terminal\aichat.json`, fallback `.lare-data`),
  comandi Tauri `get_aichat_settings` (infallibile, default sicuri) e
  `set_aichat_settings` (valida poi merge-scrive). Validazione estratta in
  `validate()` pura e testata: `label_base` non vuota dopo `trim()`, 1..=32
  caratteri, solo `[A-Za-z0-9_-]` (diventa `"<label_base>-human"` sul wire);
  `chat_port` in `1024..=65535` (niente porte privilegiate). 6 test unitari
  (RED→GREEN via TDD): `merge_preserves_other_fields`,
  `merge_from_empty_or_invalid_yields_object_with_fields`,
  `validate_rejects_empty_label`, `validate_rejects_bad_charset`,
  `validate_rejects_privileged_port`, `validate_accepts_defaults`.
- **`src-tauri/src/main.rs`** — `mod aichat_settings;`, comandi
  `aichat_settings::get_aichat_settings` / `set_aichat_settings` registrati in
  `invoke_handler`.
- **`frontend/config-dialog.js`** — nuovo tab "AI Chat" in `/config`:
  `_buildAiChatTab(panel, aichat)` (checkbox `Attivo`, campo testo `Etichetta`,
  campo numero `Porta` 1024–65535, nota `.config-note` "Le modifiche all'AI Chat
  richiedono il riavvio dell'orchestrator." — il servizio legge `aichat.json`
  solo all'avvio, a differenza dei parametri Search che sono live). `open()`
  carica `get_aichat_settings` con fallback ai default; `_buildPanel` ora
  accetta `aichat` e aggiunge `{ id: "aichat", label: "AI Chat" }` a `TABS`. Il
  salvataggio pre-valida `label_base`/`chat_port` lato JS (messaggio in
  `errorEl`, coerente con Search) poi chiama
  `set_aichat_settings({ settings: newAiChat })` nello stesso blocco `try` di
  `set_config`/`set_search_settings`.
- **`frontend/config.css`** — classe `.config-note` (testo muted, 10px, per
  note informative in fondo a un tab).

### Changed

- `src-tauri/Cargo.toml` — `version` `0.36.4` → `0.37.0`.

Nessun cambiamento a `protocol`/`orchestrator`/`mcp-server`: `ui` scrive lo
stesso `aichat.json` che l'orchestrator legge al proprio avvio — nessun
contratto WS coinvolto.

---

## [0.36.4] — 2026-07-02 — AI Chat: avviso keepalive "peer sparito"

### Added

- **`frontend/aichat-view.mjs`** — `peerLostText(label)`: testo dell'avviso live
  "`<label>` è sparito" (fallback "un peer è sparito" se il label manca). 2 nuovi test
  `node:test`.
- **`frontend/app.js`** — case `"ai_chat_peer_lost"` → `pushAiChat("aichat:peer_lost", { label })`.
- **`frontend/aichat-window.js`** — listener `aichat:peer_lost`: riga di sistema
  transitoria nel trascritto — MAI aggiunta allo stato ri-renderizzato da
  `aichat:history` (evento live-only, coerente col backend: `orchestrator` 0.25.8).

Backend: vedi `orchestrator` 0.25.8 / `protocol` 0.9.4.

---

## [0.36.3] — 2026-07-01 — Uscita pulita interattiva ("Q" + invio)

### Added
- **`src/main.rs`** — `is_quit_command(line)` (pura, 2 test) riconosce "q"/"quit"
  case-insensitive. In `.setup()` (solo `#[cfg(debug_assertions)]` — in release la
  console è staccata, `windows_subsystem = "windows"`), un thread legge stdin e su
  "q" chiama `app_handle.cleanup_before_exit()` seguito da `std::process::exit(0)`
  (API Tauri v2 dedicata: lascia a WebView2 il tempo di chiudere le sue finestre
  interne prima che il processo termini). Verificato dal vivo (3 run consecutivi):
  elimina il log `"Failed to unregister class Chrome_WidgetWin_0"` che compariva
  interrompendo il processo con Ctrl+C — quirk noto e innocuo di Chromium/WebView2
  in chiusura, ma comunque evitabile con un'uscita ordinata.

Prima di questo, l'unico modo di fermare `ui.exe` in sviluppo era Ctrl+C. Quando i
processi gireranno come servizi, l'uscita pulita sarà innescata dallo stop del
servizio, non da stdin.

---

## [0.36.2] — 2026-07-01 — AI Chat: titolo finestra con la propria etichetta

### Added
- **`frontend/aichat-view.mjs`** — `chatWindowTitle(selfLabel)`: `"AICHAT - <label>"` se
  nota, altrimenti solo `"AICHAT"` (fallback prima che arrivi `AiChatSelf`). 2 nuovi test
  `node:test`.
- **`frontend/aichat-window.js`** — listener `aichat:self` aggiorna `#titlebar-text` col
  titolo calcolato.
- **`frontend/app.js`** — case `"ai_chat_self"` → `pushAiChat("aichat:self", { label })`.
- **`frontend/aichat-window.html`** — testo statico del titlebar cambiato da
  `"AI Chat — /aichat"` (ridondante) a `"AICHAT"` (id `titlebar-text` aggiunto per
  l'aggiornamento da JS).

Backend: vedi `orchestrator` 0.25.5 / `protocol` 0.9.3.

---

## [0.36.1] — 2026-07-01 — AI Chat: replay storico messaggi nella finestra-chat

### Added
- **`frontend/aichat-view.mjs`** — `historyEntries(entries)`: normalizza il payload di storico
  (assente/vuoto → `[]`), passthrough grezzo (non il formato `messageLine` — `addLine` lo applica
  già internamente). 2 nuovi test `node:test`.
- **`frontend/aichat-window.js`** — listener `aichat:history`: svuota il trascritto e lo
  ri-renderizza per intero (niente merge/dedup) — catch-up al Join o riapertura finestra con
  storico non vuoto.
- **`frontend/app.js`** — case `"ai_chat_history"` nello switch dei messaggi WS →
  `pushAiChat("aichat:history", { entries })`.

Backend: vedi `orchestrator` 0.25.2 / `protocol` 0.9.2.

---

## [0.36.0] — 2026-06-28 — calcolatrice scientifica: tasti compatti 5-col + indicatore + apice

### Added
- **`frontend/plugin-catalog.css`** — nuove classi per la calcolatrice scientifica:
  - `.lare-key-grid`: override a **5 colonne** (da 4) con `gap: 5px` — accoglie le righe scientifiche
    da 5 tasti (sin/cos/tan/log/ln + varianti Shift).
  - `.lare-key`: override compatto: `font-size: 15px`, `padding: 8px 0` (da 20px/14px).
  - `.lare-key--shift-on`: sfondo blu evidenziato (`rgba(80,160,240,0.55)`, testo scuro, bordo brillante)
    applicato al tasto "2nd/Shift" quando `state.shift=true`.
  - `.lare-status` (v2): riga stato/etichette **tra l'eco e i tasti** (sostituisce `.lare-mode`,
    che stava dentro `.lare-expr` e scrollava con l'espressione) — mostra `DEG`/`RAD`, allineata a
    destra, smorzata, con sottile bordo di separazione; riservata a etichette future.
  - `.lare-pow` / `.lare-pow-exp`: rendering apice 2D di potenze (`x²`, `x³` ecc.) — `inline-flex;
    align-items: flex-start` per l'apice; `font-size: 0.7em; line-height: 1` per l'esponente.
  - `.lare-key--wide`: utility tasti larghi (span 2 col), attualmente non usata (layout 7×5 tutto a
    celle singole con i tasti scientifici v2: x²/n!/mod/e separati, operatori non più larghi).

## [0.35.0] — 2026-06-28 — calcolatrice: display grow-only (niente shrink-jitter)

### Added
- **`frontend/plugin-catalog.css`** — regola `.lare-calc .lare-display { min-height: 3.2em;
  align-items: flex-end; }`: il display della calcolatrice parte a ~3 righe e ancora il
  contenuto in basso a destra (stile calcolatrice fisica).
- **`frontend/plugin-window.js`** — `growCalcDisplay()`: tiene il **high-water** dell'altezza
  del display in una variabile di modulo (`calcDisplayHighWaterPx`, per-webview) e lo ri-applica
  come `min-height` sul `.lare-calc .lare-display` ad ogni render. Il modulo è necessario perché
  ogni render re-inietta l'HTML (il `.lare-display` è un elemento nuovo che perderebbe lo
  `style.minHeight` inline). Misura `scrollHeight` del display reso, aggiorna il massimo storico
  e lo riapplica → il display **non rimpicciolisce mai** entro la sessione della finestra.
  Chiamata **prima** di `autoFitHeight()` (invariato) in init e su `plugin:update`. Conseguenza:
  la finestra non rimpicciolisce più → sparisce il "saltellio" all'indietro segnalato in
  accettazione. Guard su `.lare-calc` → counter/ping non toccati. Reset = riapertura `/calc`
  (nuova webview ⇒ modulo ricaricato ⇒ high-water a 0 ⇒ pavimento CSS 3 righe). Richiede
  `plugin-calc 0.1.3` (classe `lare-calc` sulla finestra).

## [0.34.0] — 2026-06-27 — /config in finestra dedicata (config window)

### Added
- **`frontend/config.html`** — finestra /config come Tauri webview separata (chromeless,
  480×600, resizable, always_on_top).
- **`frontend/config.css`** — stili della finestra config (estratti da `index.html` dove
  erano dichiarati ma mai usati post-Task 3).
- **`frontend/config-window.js`** — controller della finestra config: carica la config
  corrente via `get_config`, istanzia `ConfigDialog` con `onClose` = `invoke("close_self")`,
  emette l'evento Tauri `config:saved` con il payload raw della config salvata
  (`tauriEvent.emit("config:saved", savedCfg)` — broadcast).
- **`src-tauri/src/main.rs`** — comando Tauri `open_config_window`: apre `config.html` in
  una `WebviewWindowBuilder` (singleton — se già aperta, `set_focus`).
- **`src-tauri/capabilities/config-window.json`** — capability per la webview config
  (permessi: `core:default`, `core:window:allow-start-dragging`; label: `"config"`).
- **`frontend/app.js`** — `applySavedConfig(savedCfg)`: funzione estratta dall'inline
  callback (era in `new ConfigDialog(...)`), ora chiamata dall'evento `config:saved`.
- **`frontend/app.js`** — `setupConfigWindowEvents()`: registra `tauriEvent.listen("config:saved",
  ...)` che chiama `applySavedConfig(ev.payload)` — back-channel tra la webview config e
  il pannello principale.

### Changed
- **`frontend/config-dialog.js`** — il costruttore accetta un 4° parametro opzionale
  `onClose` (default no-op), invocato in `close()`. Lo usa la finestra dedicata /config
  (`config-window.js`) per chiudere la propria webview su Cancel/Esc/×/dopo-save; i
  chiamatori a 3 argomenti restano invariati.
- **`frontend/app.js`** — `/config` handler: da `configDialog.open()` a
  `invokeCmd("open_config_window").catch(...)` (simmetria con `/library`: nessuna
  unhandled rejection fuori dal contesto IPC Tauri). La Rust-side gestisce il pattern
  singleton (se la finestra esiste già, le dà il focus invece di aprirne un'altra).
- **`frontend/app.js`** — rimosso `import { ConfigDialog }` e l'istanza
  `const configDialog = new ConfigDialog(overlayPanel, ...)` — il dialog non è più
  montato nel pannello principale; vive solo nella webview `config.html`.
- **`frontend/app.js`** — rimosso `const overlayPanel` (era usato solo da `configDialog`).
- **`frontend/app.js`** — rimossa la guardia `if (configDialog.isOpen()) return;` da
  `grabFocus()` (la finestra config è una webview separata, non cattura il focus del
  pannello principale).
- **`frontend/app.js`** — rimossa la guardia `.config-dialog-overlay` dal click handler
  (l'overlay non è mai montato nel DOM principale, la guardia era dead code).
- **`frontend/index.html`** — polish post-review: rimossi/aggiornati i commenti obsoleti
  (il placeholder "/config dialog injected here" e il riferimento a `config-dialog.js` tra
  i moduli ES caricati nel pannello — il dialog ora vive solo nella finestra `config.html`).
- **`src-tauri/capabilities/default.json`** — invariato: `core:default` include già l'event
  listen API necessaria per ricevere `config:saved` nel pannello principale.

### Bumped
- `ui` → `0.34.0`

---

## [0.33.0] — 2026-06-27 — plugin Slice 2b: input da tastiera nelle finestre plugin

### Added
- **`frontend/plugin-runtime.mjs`** — seam puro `eventFromKey(key, buttons)`: mappa un
  `KeyboardEvent.key` sul **primo** pulsante i cui token `data-key` (separati da spazio,
  es. `"= Enter"`) lo includono, ritornando `{ element_id, value: null }` oppure `null`
  (tasto non mappato). Così tastiera e mouse condividono lo stesso percorso `PluginUiEvent`.
  6 nuovi test `node:test` (RED osservato → GREEN 12/12).
- **`frontend/plugin-window.js`** — listener `keydown` sul `window`: ad ogni keydown legge
  tutti i `[data-key]` presenti nel `#plugin-root` (HTML del plugin), li mappa via
  `eventFromKey(ev.key, buttons)` e, su match, chiama `ev.preventDefault()` +
  `emitUiEvent(result.element_id, result.value)` — stessa via del click delegato.
  I pulsanti vengono riletti ad ogni keydown perché l'HTML viene re-iniettato ad ogni
  `plugin:update`; `querySelectorAll` è economico per la manciata di tasti della
  calcolatrice. La prevenzione del default sopprime effetti del browser (es. `/` apre
  il quick-find di Chromium, `Backspace` naviga indietro).
  Nessuna regola `data-key` hardcoded qui: la mappatura è dichiarata dall'HTML del plugin
  (Task 2, attributi `data-key` sui `<button>` di `plugin-calc`) e interpretata dalla
  funzione pura `eventFromKey` (Task 1, già in `plugin-runtime.mjs`).
- **`frontend/plugin-window.js`** — import di `eventFromKey` da `./plugin-runtime.mjs`
  (aggiunto alla riga di import esistente di `eventFromTarget` e `sanitizeAndRender`).
- **`frontend/plugin-window.js`** — guard sui modificatori nel listener `keydown` (commit
  `8b4d23e`): le combo con `Ctrl`/`Cmd`/`Alt` (es. `Ctrl+0`, `Ctrl++`/`=` zoom, `Ctrl+R`
  reload) escono **subito** e restano al browser/sistema — non vengono dirottate come input
  della calcolatrice. `Shift` **non** è escluso di proposito: serve a digitare `* ( ) = /`
  (combo con Shift sul layout italiano); nessun simbolo della calcolatrice usa AltGr.

### Note: DOMPurify ammette `data-key`
`sanitizeAndRender` in `plugin-runtime.mjs` (Task 1) già include `"data-key"` in
`ADD_ATTR` di DOMPurify, quindi gli attributi `data-key` nei pulsanti non vengono
rimossi durante la sanitizzazione dell'HTML del plugin.

### Bumped
- `crates/ui/src-tauri/Cargo.toml` 0.32.0 → 0.33.0

---

## [0.32.0] — 2026-06-27 — plugin Slice 2a: auto-fit finestra plugin + CSS .lare-expr

### Added
- **`frontend/plugin-window.js`** — `autoFitHeight()`: ridimensiona la finestra in **altezza**
  al contenuto del plugin, sia all'apertura **sia ad ogni `plugin:update`**, così che la
  crescita del contenuto (es. il display che passa da riga singola a frazione 2D impilata) non
  venga **mai tagliata**. Solo l'altezza è guidata dal contenuto; la **larghezza resta** quella
  corrente dell'utente (un resize orizzontale manuale è preservato, niente "lotta"); a contenuto
  invariato `resize_self` alla stessa altezza è un no-op visivo. Usa il flex-collapse trick (lo
  stesso di `window.js`): `#plugin-root` viene temporaneamente portato a `flex: 0 0 auto` così
  che `scrollHeight` riporti l'altezza reale del contenuto (non quella allungata da `flex:1`);
  aggiunge l'altezza della titlebar + 4 px buffer; clamp `[120 px, 85 % schermo]`; chiama
  `resize_self` (comando Tauri già presente, già autorizzato in `plugin-window.json`).
- **`frontend/plugin-catalog.css`** — classe `.lare-expr`: riga "eco espressione digitata"
  per la calcolatrice (input verbatim sotto il display). Allineata a destra, muted
  (`var(--text-muted, rgba(160,185,220,0.70))`), font-size 0.85em, `overflow-x:auto` con
  scrollbar nascosta (`scrollbar-width:none` + `::-webkit-scrollbar {height:0}`), `min-height`
  per layout stabile da vuota.

### Bumped
- `crates/ui/src-tauri/Cargo.toml` 0.31.0 → 0.32.0

---

## [0.31.0] — 2026-06-27 — plugin Slice 2: catalogo CSS calcolatrice

### Added
- **`frontend/plugin-catalog.css`** — classi CSS per la calcolatrice (Slice 2):
  `.lare-display` (display 28px con overflow orizzontale e allineamento a destra);
  `.lare-key-grid` (griglia 4 colonne, gap 8px);
  `.lare-key` (pulsante 20px con hover/active animati, palette dark coerente con il resto della UI);
  `.lare-frac` / `.lare-frac-num` / `.lare-frac-bar` / `.lare-frac-den`
  (frazione impilata: `flex-column`, barra = `border-bottom 2px solid currentColor`).

### Bumped
- `crates/ui/src-tauri/Cargo.toml` 0.30.0 → 0.31.0

---

## [0.30.0] — 2026-06-27 — plugin Slice 1: finestra-plugin generica + runtime host + catalogo

### Added

- **`frontend/plugin-runtime.mjs`** — modulo puro (no DOM/Tauri):
  `eventFromTarget(el)` legge `data-evt` e `value` da un elemento; `sanitizeAndRender(html)`
  purifica con DOMPurify mantenendo `class` e `data-evt`. 6 test `node:test` verdi.
- **`frontend/plugin-runtime.test.mjs`** — 6 test TDD per `plugin-runtime.mjs`.
- **`frontend/plugin-catalog.css`** — stili condivisi per le finestre plugin:
  classi `.lare-window`, `.lare-label`, `.lare-button` (+ classi forward per Slice 2).
- **`frontend/plugin-window.html` / `frontend/plugin-window.js`** — finestra-plugin generica
  (label stabile `plugin-<window_id>`, transparent/always_on_top). `plugin-window.js`
  bootstrap: `invoke("take_window_content")` (comando esistente) → imposta titolo, inietta HTML
  del plugin via `sanitizeAndRender`. Listener click delegato sui `data-evt="..."` → emette
  evento Tauri `"plugin:ui-event"`. Listener `"plugin:update"` → aggiorna `innerHTML`.
  Listener `"plugin:close"` → `getCurrentWindow().close()` + emette `"plugin:window-closed"`.
- **`src-tauri/src/main.rs`** — un nuovo comando Tauri `open_plugin_window(window_id, title, html)`:
  label stabile `plugin-<window_id>`; riusa `WindowContentStore` (state esistente) con triple
  `(title, html, window_id_str)`. `plugin-window.js` legge l'entry via `take_window_content`
  (comando esistente riusato). F2 hide/show esteso alle finestre `plugin-*`. Registrato in
  `generate_handler!`. Nessun nuovo file `.rs` — tutto in `main.rs`.
  `plugin:ui-event` e `plugin:window-closed` **non sono comandi Tauri**: sono eventi Tauri
  emessi da `plugin-window.js` e ascoltati da `app.js` → WS.
- **`frontend/ws-client.js`**: `sendPluginUiEvent(window_id, element_id, value)` e
  `sendPluginWindowClosed(window_id)` — inviano `ClientMsg::PluginUiEvent` /
  `ClientMsg::PluginWindowClosed` via WS.
- **`app.js`**: gestione `ServerMsg::OpenPluginWindow` → `invokeCmd("open_plugin_window")`;
  `ServerMsg::UpdatePluginWindow` → emette evento Tauri globale `"plugin:update"`;
  `ServerMsg::ClosePluginWindow` → emette `"plugin:close"`.
  Listener `"plugin:ui-event"` → `client.sendPluginUiEvent(...)`;
  listener `"plugin:window-closed"` → `client.sendPluginWindowClosed(...)`.
- **Capability `plugin-window.json`** — permissions `["core:default", "core:window:allow-start-dragging"]`
  scoped `windows: ["plugin-*"]`.

### Bumped
- `crates/ui/src-tauri/Cargo.toml` 0.29.0 → 0.30.0

---

## [0.29.0] — 2026-06-26 — Overlay: il pannello riempie la finestra + auto-grow + reset

### Fixed
- **Intercettazione mouse sopra altre app (Explorer)** — la finestra `main` era 1100×480
  trasparente ma il pannello ne copriva solo una parte: le bande trasparenti, vive ai click,
  catturavano i click destinati a ciò che stava sotto. Risolto facendo **riempire la finestra
  al pannello** (sotto) → nessuna banda trasparente.

> **Nota di percorso (accettazione GUI):** la prima implementazione faceva *inseguire il
> pannello alla finestra* (`ResizeObserver → setSize`, modulo puro `window-sizing.js`). Si è
> rivelata fragile su Windows: una finestra `decorations:false` + `transparent` non si
> ridimensiona in modo affidabile via `setSize` (il comando si risolve ma l'OS non applica).
> Col supervisore si è **invertito il flusso** (la finestra è la sorgente di verità, il
> pannello la segue). `window-sizing.js` + test **rimossi**.

### Architettura finale — il PANNELLO riempie la finestra (`index.html`)
- `body` è `display:flex` e `.overlay-panel` è `flex:1` → il pannello **riempie l'intera
  finestra** (niente bande trasparenti → niente intercettazione). `#output-area` è `flex:1`
  e scrolla. Rimosse le larghezze fisse `1060px`.
- Finestra **ridimensionabile dall'utente** (`resizable:true`): il bordo del `body` è un
  **bordo doppio blu** (stesso colore/raggio del pannello, gap 1px) che fa da handle del
  resize; ridimensionare la finestra ridimensiona il pannello.
- `data-tauri-drag-region` sul `.overlay-panel` → la finestra si **trascina** dallo sfondo del
  pannello (i figli interattivi — input, button, `.hint-cmd` — restano cliccabili).

### Auto-grow all'output (`app.js`)
- All'arrivo di nuovo output la finestra **cresce da sola** per mostrarlo, fino a un tetto
  (≤70% schermo, senza sforare il bordo basso), poi l'output scrolla. **Solo crescita**: non
  rimpicciolisce mai da sola (history e resize manuale rispettati).
- Trigger: `MutationObserver` sul **contenuto** dell'`#output-area` (non `ResizeObserver`):
  `setSize` cambia la size, non il contenuto, quindi non ri-triggera l'observer → niente loop.
  Debounce 80ms per non saltellare durante lo streaming.

### Reset finestra — `Ctrl+T` (`app.js` + hint in `index.html`)
- `Ctrl+T`: **pulisce** input+output (come `Ctrl+L`) **e** riporta la finestra alla dimensione
  compatta di default + **ri-centra** (la centratura resta in Rust: comando `show_overlay`).
  Hint "Ctrl+T to reset" accanto a "Ctrl+L to clear". (Non `Ctrl+R`: la WebView2 lo riserva al
  reload pagina.)

### Changed — config & capabilities
- `tauri.conf.json` → `main`: `1100×480 → 1064×140` (default compatto), `resizable: true`.
- `capabilities/default.json`: `core:window:allow-set-size` (per il `setSize` dell'auto-grow e
  del reset) + `core:window:allow-start-dragging` (per `data-tauri-drag-region`). Nessuno dei
  due è in `core:default` → senza, l'azione è un **no-op silenzioso**.

### Note
- Centratura finestra: resta in Rust (`window.rs`, su F2-show), invariata.
- e2e GUI **accettato dal supervisore** (bordo doppio + angoli, auto-grow, `Ctrl+T`/`Ctrl+L`,
  drag, resize manuale, niente intercettazione).

---

## [0.28.1] — 2026-06-26 — Hardening patch: boundary guards + test assertions

### Added — `crates/ui/src-tauri/src/archive.rs`

- **`rename_folder`**: guardia empty-rel (rifiuta `folder_rel=""` con messaggio "vuoto") e
  guardia collisione esplicita prima di `fs::rename` (messaggio "esiste già").
- **`delete_folder`**: guardia empty-rel (rifiuta `folder_rel=""` con messaggio "vuoto").
- **`move_file`**: guardia empty-rel (`file_rel=""`) e guardia tipo `!src_abs.is_file()`
  (rifiuta directory come sorgente con messaggio "file").

### Added — tests (`archive.rs` + `library-nav.test.mjs`)

- 5 nuovi test RED→GREEN (I-1): `rename_folder_rejects_empty_rel`,
  `rename_folder_rejects_collision`, `delete_folder_rejects_empty_rel`,
  `move_file_rejects_empty_rel`, `move_file_rejects_directory_source`.
- 4 test esistenti rafforzati con asserzioni sul messaggio di errore (M-5):
  `create_folder_parent_not_found_returns_err` (→ "not found"),
  `rename_folder_rejects_invalid_name` (→ "invalid"),
  `move_folder_source_not_found` (→ "sorgente"),
  `move_folder_target_parent_not_found` (→ "non trovata").
- 1 nuovo test JS (M-7): `moveTargets: guardia prefisso — 'AB' non viene esclusa quando
  si sposta 'A'` — documenta che `startsWith(rel + "/")` evita falsi positivi tra sibling.

---

## [0.28.0] — 2026-06-26 — Library: UI "Sposta" — modale ↗ (Slice 4b)

### Added — `crates/ui/frontend/library-nav.mjs`

- **`flattenFolders(tree)`**: visita l'albero `LibraryTree` in pre-order e restituisce
  un array piatto `{ name, relPath, depth }` di tutte le cartelle.
  `depth` è la profondità assoluta (0 = primo livello sotto la root).
  Pura, testabile senza DOM/Tauri.
- **`moveTargets(tree, itemRelPath, isFolder)`**: calcola le destinazioni valide per
  spostare un elemento. Applica le regole di esclusione:
  - Esclude il parent corrente (no-op / collisione).
  - Se cartella: esclude sé stessa e tutti i discendenti (prefisso `"/"` per evitare
    falsi positivi, es. `"AB"` non è discendente di `"A"`).
  - Antepone l'opzione ROOT `{name:"Documenti (radice)", relPath:"", depth:0}` solo se
    il parent non è già la root (caso: elemento già al primo livello).
  Pura, testabile senza DOM/Tauri.

### Added — `crates/ui/frontend/library-nav.test.mjs`

- 10 nuovi test TDD per `flattenFolders` e `moveTargets`:
  albero vuoto, flat, pre-order con nesting, singolo livello,
  documento alla radice, documento in cartella A, cartella di primo livello,
  cartella annidato `A/B`, cartella foglia profonda `A/B/C`,
  cartella unica (nessuna destinazione disponibile).
- Aggiornato l'import per includere `flattenFolders` e `moveTargets`.
- Totale test: 37 verdi.

### Added — `crates/ui/frontend/library.html`

- **`<dialog id="move-dialog">`**: modale nativa con `showModal()`.
  Struttura: titolo `#move-dialog-title`, lista scrollabile `#move-targets`,
  riga errore `#move-error` (nascosta di default), footer con `#move-confirm`
  (disabilitato finché non c'è selezione) e `#move-cancel`.
- CSS nella palette esistente: `.move-dialog-box` coerente con l'overlay panel,
  `.move-target` (indentata per depth), `.move-target--selected` (evidenziata),
  `dialog::backdrop` con velo semitrasparente.
- Footer hint aggiornato: aggiunto `· ↗ sposta`.

### Added — `crates/ui/frontend/library.js`

- Import di `flattenFolders`, `moveTargets` da `library-nav.mjs`.
- Riferimenti DOM per la modale (`moveDialog`, `moveDialogTitle`, `moveTargetsEl`,
  `moveErrorEl`, `moveConfirmBtn`, `moveCancelBtn`).
- **`moveState`**: oggetto condiviso `{ itemRel, isFolder, chosen }` letto dai
  handler del dialog; azzerato a ogni apertura.
- **`openMoveDialog(itemRel, isFolder, displayName)`**: calcola `moveTargets`,
  popola il titolo e la lista delle destinazioni, apre con `showModal()`.
  Se `targets` è vuoto mostra "Nessuna destinazione disponibile.".
  Ogni riga è selezionabile con click; selezione → abilita `#move-confirm`.
  Solo `textContent` per tutti i nomi (sicurezza XSS).
- **`wireMoveDialog()`**: registra i handler di `#move-confirm` e `#move-cancel`
  una sola volta a bootstrap (evita listener duplicati).
  - `#move-confirm`: invoca `archive_move_file { fileRel, targetFolderRel }` o
    `archive_move_folder { folderRel, targetParentRel }` in base a `isFolder`;
    successo → `moveDialog.close()` + `reloadCurrentView()`; errore → mostra
    messaggio in `#move-error` senza chiudere il dialog.
  - `#move-cancel`: `moveDialog.close()`.
  - **GOTCHA Esc**: listener `keydown` sul dialog con `stopPropagation` su `Escape`
    (senza `preventDefault`) → il dialog si chiude nativamente ma l'evento non
    bubbla all'handler globale che chiuderebbe la finestra Library.
- **Pulsante ↗ in `buildFolderRow`**: aggiunto tra ✏️ e 🗑.
  Click: `stopPropagation` + `openMoveDialog(folder.rel_path, true, folder.name)`.
  Dblclick: `stopPropagation` (evita entrata nella cartella).
- **Pulsante ↗ in `buildMarkdownItem`**: aggiunto solo se `onDelete !== null`
  (contesto Explorer; tab Find invariato). Posizionato prima di 🗑.
  Click: `stopPropagation` + `openMoveDialog(entry.file, false, entry.title)`.
  Dblclick: `stopPropagation` (la riga non ha guard `closest("button")`).
- **fs-watch guard**: nel listener `library:changed` aggiunta guardia
  `if (moveDialog?.open) return;` accanto a quella sull'INPUT, per evitare
  reload durante la modale aperta.
- `wireMoveDialog()` chiamato in `bootstrap()` dopo `activateTab("markdown")`.

### Versioning

- `crates/ui/src-tauri/Cargo.toml`: `0.27.0` → `0.28.0`.

---

## [0.27.0] — 2026-06-26 — Library: `move_folder` + hardening `move_file` (Slice 4a)

### Added — `crates/ui/src-tauri/src/archive.rs`

- **`pub fn move_folder(dir, folder_rel, target_parent_rel) -> Result<(), String>`**:
  sposta una cartella nella Library preservandone il nome.
  Implementa la guardia anti-ciclo sulle rel-path normalizzate
  (`target_parent_rel == folder_rel` oppure `starts_with(folder_rel + "/")`)
  per bloccare spostamenti che creerebbero loop nel filesystem.
  Rifiuta con `Err` in caso di: traversal, cartella sorgente inesistente,
  parent di destinazione inesistente, ciclo, collisione (nessun merge silenzioso).
  Usa `std::fs::rename` (atomico su Windows sullo stesso volume).
- 8 nuovi test TDD per `move_folder`:
  `move_folder_into_subfolder`,
  `move_folder_to_root`,
  `move_folder_rejects_into_itself`,
  `move_folder_rejects_into_descendant`,
  `move_folder_rejects_name_collision`,
  `move_folder_rejects_traversal`,
  `move_folder_source_not_found`,
  `move_folder_target_parent_not_found`.

### Changed — `crates/ui/src-tauri/src/archive.rs`

- **Hardening `move_file`**: aggiunto controllo collisione prima di `std::fs::rename`.
  Se il file di destinazione esiste già → `Err("a destinazione esiste già un documento
  con quel nome: ...")`. Previene sovrascrittura silente con perdita di dati.
  I 3 test esistenti (`move_file_moves_to_subfolder`, `move_file_moves_to_root`,
  `move_file_rejects_traversal`) non sono influenzati (non creano collisioni).
- 1 nuovo test TDD: `move_file_rejects_collision_no_overwrite`.

### Added — `crates/ui/src-tauri/src/main.rs`

- **`#[tauri::command] fn archive_move_folder(app, folder_rel, target_parent_rel)`**:
  comando Tauri che risolve `documents_dir_path(&app)` e delega a `archive::move_folder`.
  Registrato in `generate_handler!` accanto agli altri comandi `archive_*`.

### Totale test: 86 verdi (9 nuovi).

---

## [0.26.0] — 2026-06-26 — Library: correzione layout → `library/documents/`

### Added — `crates/ui/src-tauri/src/archive.rs`

- **`migrate_to_documents_layout(library_dir: &Path) -> Result<(), String>`**: migra il
  vecchio layout (file `.md` sciolti in `library/` root) al nuovo (`library/documents/`).
  Idempotente (seconda chiamata è no-op), senza perdita dati (collisioni → suffisso numerico),
  atomica su stesso-volume (usa `std::fs::rename` / MoveFileEx). Implementazione a due fasi:
  collect entries → close iterator → rename, per evitare `os error 5` su Windows.
- **`unique_name_in(target_dir, name, is_dir)`** (funzione privata): calcola un path di
  destinazione unico aggiungendo suffisso numerico crescente da 2 ("foo.md" → "foo 2.md";
  "Bar" → "Bar 2"). Limite anti-loop a 1000 tentativi con fallback timestamp.
- 7 nuovi test TDD per la migrazione:
  `migrate_creates_documents_dir_even_when_empty`,
  `migrate_moves_md_files_to_documents`,
  `migrate_moves_user_folders_to_documents`,
  `migrate_leaves_find_at_root`,
  `migrate_does_not_move_documents_into_itself`,
  `migrate_idempotent`,
  `migrate_collision_appends_suffix_no_overwrite`.

### Removed — `crates/ui/src-tauri/src/archive.rs`

- **Filtro `find/`** in `collect_node_contents`: rimosso il blocco
  `if depth == 0 && name == "find" { continue; }`. Non più necessario: `list_tree` ora
  opera su `documents/` (passato da `archive_list_tree` in `main.rs`), che non contiene `find/`.
- **Test `list_tree_ignores_find_subdir`**: rimosso perché il comportamento testato non esiste
  più (la funzione non filtra più `find/`). Totale: 77 test verdi.

### Added — `crates/ui/src-tauri/src/main.rs`

- **`documents_dir_path(app)`** (helper privato): ritorna `{app_config_dir}/library/documents`.
- **`#[tauri::command] fn documents_dir(app)`**: comando Tauri che espone il path di
  `documents/` come stringa. Registrato in `generate_handler!`.

### Changed — `crates/ui/src-tauri/src/main.rs`

- **9 comandi documento** (`archive_delete`, `archive_list_tree`, `archive_create_folder`,
  `archive_rename_folder`, `archive_move_file`, `archive_delete_folder`, `archive_save`,
  `archive_list`, `archive_open`) switchati da `library_dir_path` a `documents_dir_path`.
  I comandi find (`save_find`, `list_find`, `open_find`, `delete_find`,
  `open_saved_find_window`) e il comando `library_dir` restano invariati.
- **`.setup`**: (a) chiamata best-effort `archive::migrate_to_documents_layout(&dir)` subito
  dopo `create_dir_all` della root library/ — garantisce che `documents/` esista prima del watch;
  (b) il fs-watch ora osserva `documents_dir_path` invece di `library_dir_path` (modifiche a
  `find/` non triggerano più reload inutili nel tab Markdown).

### Changed — `crates/ui/frontend/library.js`

- Bottone 📂 (`open-dir-btn`): invoca `documents_dir` invece di `library_dir`, così apre
  `library/documents/` (coerente con la vista ad albero).

---

## [0.25.0] — 2026-06-26 — Library a cartelle: fs-watch auto-reload (Slice 3)

### Added — `crates/ui/src-tauri/src/library_watch.rs` (nuovo modulo lib)

- **`watch_dir(dir, debounce, on_change)`**: avvia un watcher ricorsivo su `dir`
  (tramite `notify-debouncer-mini` 0.7.x). Raggruppa i burst in un debounce e chiama
  `on_change()` una volta per burst. Ritorna un `WatchGuard` da tenere vivo (RAII).
- Seam testabile: la callback `impl Fn() + Send + 'static` non conosce Tauri —
  nei test si inietta un `Arc<AtomicUsize>` contatore; nell'app si inietta l'emit Tauri.
- **TDD red→green verificato**: stub no-op per RED, implementazione reale per GREEN.
- 2 nuovi test (`watch_dir_detects_a_change`, `watch_dir_debounces_burst`), totale 71 test verdi.

### Added — `crates/ui/src-tauri/Cargo.toml`

- Nuova dipendenza: `notify-debouncer-mini = "0"` (risolve a 0.7.x con notify 8.x interno).
  Accediamo a `notify` tramite i re-export di `notify-debouncer-mini` per evitare conflitti di
  versione nel grafo delle dipendenze.

### Changed — `crates/ui/src-tauri/src/lib.rs`

- Esposto `pub mod library_watch` affinché i test (`cargo test -p ui`) lo raggiungano
  senza avviare il runtime Tauri (stessa convenzione di `config` e `archive`).

### Changed — `crates/ui/src-tauri/src/main.rs`

- Import: aggiunto `use tauri::Emitter` (necessario per `win.emit()` in Tauri v2),
  `use std::time::Duration`, `use ui_lib::library_watch`.
- `LibraryWatchGuard(Mutex<library_watch::WatchGuard>)`: newtype managed state;
  il `Mutex` rende il tipo `Sync` (richiesto da `app.manage()`) senza overhead a
  runtime, poiché il guard non viene mai riletto dopo la creazione.
- Nel hook `.setup`, subito dopo `create_dir_all` della Library: avvia `watch_dir` con
  debounce 400 ms e callback che emette `"library:changed"` alla finestra `"library"`
  (solo se aperta). Il guard è conservato via `app.manage(LibraryWatchGuard(...))`.

### Changed — `crates/ui/frontend/library.js`

- In `bootstrap()`: si iscrive all'evento `"library:changed"` via `tauriEvent.listen`.
  Alla ricezione chiama `reloadCurrentView()` solo se `activeTab === "markdown"`
  (il tab Find non ha bisogno di ricaricare l'albero cartelle).
- **Guardia anti-clobber rinomina**: se al momento dell'evento `library:changed` il focus
  è su un `<input>` (`document.activeElement?.tagName === "INPUT"`), il reload viene saltato.
  Motivazione: `notify` segnala ogni modifica al filesystem incluse quelle della nostra UI
  (es. `archive_create_folder` scrive la dir → trigger debounce → reload dopo 400 ms).
  Senza questa guardia, un reload arriverebbe mentre l'utente sta ancora digitando il nome
  nella rinomina inline, distruggendo l'`<input>` e resettando il nome.

---

## [0.24.0] — 2026-06-26 — Library a cartelle: vista Explorer (frontend)

### Added — `crates/ui/frontend/library-nav.mjs` (nuovo modulo puro)

- **`isValidFolderName(name)`**: valida un nome di cartella; `false` se vuoto/solo-spazi
  o contiene `/`, `\`, `..`; `true` altrimenti. Guardia client-side (il backend ricontrolla).
- **`isFolderEmpty(node)`**: stima ottimistica `node.folders.length === 0 && node.files.length === 0`.
  Usata SOLO per mostrare/nascondere il 🗑; la fonte di verità è il backend.
- **`findNode(tree, relPath)`**: naviga `LibraryTree` seguendo i segmenti di `relPath`.
  `""` → root; `null` se il path non esiste (cartella scomparsa dall'esterno).
- **`breadcrumbSegments(relPath)`**: converte un relPath in array di `{ name, relPath }`
  con relPath cumulativo per ogni segmento (usato dal click handler del breadcrumb).
- **`parentPath(relPath)`**: ritorna il relPath del parent (`""` per root e singolo segmento).
- **TDD**: 27 test in `library-nav.test.mjs` (ciclo RED→GREEN verificato con output).
  Stubs sbagliati prima dell'implementazione per garantire assertion failure (non module-not-found).

### Added — `crates/ui/frontend/library-nav.test.mjs` (nuovo file test)

- 27 test `node:test` per le 5 funzioni pure del modulo. Coprono: path vuoto, un livello,
  annidato, path inesistente, nomi validi/invalidi, breadcrumb cumulativo, parentPath.

### Changed — `crates/ui/frontend/library.html`

- Aggiunta toolbar `#lib-toolbar` dopo `#tab-bar`: `#lib-breadcrumb` (a sinistra) +
  `#new-folder-btn` ("+ Cartella") + `#reload-btn` (🔄).
- Nuovo CSS in armonia con la palette esistente: `.lib-crumb` (breadcrumb cliccabili),
  `.lib-crumb-sep`, `.lib-folder-row` (icona 📁), `.lib-folder-btn` (✏️/🗑 per-cartella),
  `.lib-rename-input` + `.lib-rename-input--error` (rinomina inline).
- `#footer` hint aggiornato: "Invio o doppio-clic per aprire · ✏️ rinomina · 🗑 elimina".

### Changed — `crates/ui/frontend/library.js`

- **Import**: `isValidFolderName`, `isFolderEmpty`, `findNode`, `breadcrumbSegments`
  da `./library-nav.mjs`.
- **Stato**: `let tree = null; let currentPath = "";` per la navigazione Explorer.
- **`reloadCurrentView()`**: richiama `archive_list_tree` → aggiorna `tree` → `renderCurrent()`.
  Esposta come `window.libraryReload` per la futura Slice 3 (fs-watch).
- **`renderCurrent()`**: trova il nodo per `currentPath`, fallback a root se scomparso,
  renderizza breadcrumb + sottocartelle + documenti (cartelle prima, file dopo).
- **`renderBreadcrumb()`**: costruisce 🏠 + segmenti cliccabili con `textContent` (mai innerHTML).
- **`buildFolderRow(folder)`**: riga 📁 con nome, ✏️ (rinomina), 🗑 (elimina se vuota).
  Usa `.archive-item` per la navigazione ArrowUp/Down unificata. `row._folderData` per
  la rinomina post-create senza round-trip aggiuntivo.
- **`startRename(row, folder)`**: rinomina inline — input con testo selezionato, commit su
  Invio/blur, annulla su Esc (con `stopPropagation` per non chiudere la finestra).
- **`wireFolderDeleteButton()`**: due passi di conferma per cartelle; su Err → `flashRowError`
  + `reloadCurrentView()` (il backend è la fonte di verità sul vuoto della cartella).
- **`wireDeleteButton`**: aggiunto parametro opzionale `onSuccess`. Nel contesto Explorer
  chiama `reloadCurrentView()` invece di `row.remove()`; retrocompatibile (tab Find usa null).
- **`buildMarkdownItem`**: aggiunto parametro opzionale `onDelete` passato a `wireDeleteButton`.
  Nel contesto Explorer = `reloadCurrentView`; per il tab Find = null (comportamento invariato).
- **`loadMarkdownTab()`**: sostituisce `archive_list` (piatto) con `reloadCurrentView()` (albero).
- **`activateTab()`**: mostra/nasconde `#lib-toolbar` in base al tab attivo.
- **Arrow nav guard**: `if (document.activeElement?.tagName === "INPUT") return;` — evita
  che ArrowUp/Down interferisca col caret durante la rinomina inline.
- **`+ Cartella`**: invoca `archive_create_folder`, ricarica, trova la riga per il `rel_path`
  restituito dal backend (gestisce collisioni "Nuova cartella 2"), avvia rinomina inline.

### Verification

```
node --test crates/ui/frontend/library-nav.test.mjs
  tests 27 | pass 27 | fail 0

node --check crates/ui/frontend/library.js
  (no output — syntax OK)

cargo build -p ui
  Compiling ui v0.24.0
  Finished dev profile — no errors
```

E2e GUI (navigazione cartelle, breadcrumb, rinomina, elimina, "+ Cartella") richiede
run interattivo dell'utente.

---

## [0.23.0] — 2026-06-25 — Library a cartelle: delete_folder (backend)

### Added — `archive.rs`

- `delete_folder(dir, folder_rel)`: elimina una cartella SOLO se fisicamente vuota.
  Il filesystem è la fonte di verità (rifiuta anche se contiene file non-.md invisibili
  nella Library UI). Usa `std::fs::remove_dir` (NON `remove_dir_all`) come secondo livello
  di sicurezza contro race condition TOCTOU. 6 nuovi test unitari (TDD RED→GREEN).

### Added — `main.rs`

- Comando Tauri `archive_delete_folder` registrato in `invoke_handler!`.

---

## [0.22.0] — 2026-06-25 — Library a cartelle (backend)

### Added — `archive.rs`

- Nuovi tipi `LibraryNode` e `LibraryTree` (serializzabili via Serde) per rappresentare
  l'albero della Library con cartelle annidate.
- `validate_within_root(base, rel_path)`: guardia di sicurezza path-traversal basata su
  `path.components()` (non `canonicalize()`); blocca `..`, path assoluti e backslash;
  l'errore contiene sempre la parola `"traversal"`.
- `list_tree(dir)`: visita DFS limitata a 10 livelli di profondità; ignora la sottocartella
  riservata `find/` alla root; restituisce `LibraryTree` con `root_files` e `folders`.
- `create_folder(dir, parent_rel, name)`: crea una cartella fisica; gestisce collisioni
  di nome con suffisso numerico ("Nome 2", "Nome 3", …).
- `rename_folder(dir, folder_rel, new_name)`: rinomina una cartella esistente.
- `move_file(dir, file_rel, target_folder_rel)`: sposta un file `.md` tra cartelle.
- 18 nuovi test unitari per le nuove funzioni + 2 test di riconciliazione `open`/`delete`.

### Changed — `archive.rs`

- `open` e `delete` ora usano `validate_within_root` invece di `reject_traversal`:
  i path con sottocartelle (es. `"Progetti/doc.md"`) sono ora **legittimi**.
  I path `..`, assoluti e con backslash restano rifiutati.
- Rimossi 4 test obsoleti che codificavano il vecchio contratto "slash sempre illegale":
  `open_rejects_slash_in_filename`, `open_rejects_backslash_in_filename`,
  `delete_rejects_slash_in_filename`, `delete_rejects_backslash_in_filename`.

### Added — `main.rs`

- 4 nuovi comandi Tauri registrati in `invoke_handler!`:
  `archive_list_tree`, `archive_create_folder`, `archive_rename_folder`, `archive_move_file`.

---

## [0.20.2] — 2026-06-24 — comandi riga hint cliccabili + dedupe Library

### Changed — `index.html`, `app.js`

- I comandi nella riga hint sotto il prompt (`/config`, `/library`, `/help`) sono ora
  **cliccabili** (`.hint-cmd` con `data-cmd`): un click esegue lo stesso comando del digitato,
  riusando `handleSlashCommand`. Le diciture dei tasti (Enter/Esc/Ctrl+L) restano testo.
- Rimosso il pulsante **📚** in alto (`#library-btn` + CSS + listener): ridondante ora che
  `/library` è cliccabile nella riga hint.

## [0.20.1] — 2026-06-24 — polish: scheda dialog `/config` + finestra senza ombra

### Changed — `index.html`, `tauri.conf.json`

- `.config-dialog-box`: ora ha un aspetto a **scheda** (sfondo solido, bordo sottile, angoli
  arrotondati, ombra) invece di un form senza cornice; `.config-dialog-overlay` con velo più
  leggero (`0.96` → `0.72`).
- Finestra `main`: `"shadow": false` — rimuove la cornice/ombra grigia attorno al pannello
  (la finestra è più larga del pannello; l'ombra di sistema appariva come un riquadro inutile).

## [0.20.0] — 2026-06-24 — Dialog `/config` a tab (UI | Search)

### Added

- Dialog `/config` riorganizzato a tab: tab **UI** (campi visivi esistenti invariati) e tab
  **Search** con i campi **Risultati max** (`result_cap`) e **Profondità max** (`max_depth`).
- I valori Search vengono caricati da `get_search_settings` all'apertura del dialog (con
  default `result_cap=2000`, `max_depth=8`) e salvati via `set_search_settings` al click
  su Salva — insieme al salvataggio UI config (`set_config`).
- Effetto live: l'orchestrator rilegge `search-paths.json` a ogni `/find`; nessun riavvio
  necessario per applicare i nuovi valori.
- CSS minimo per i tab (`.config-tabs`, `.config-tab-btn`, `.config-tab-btn.active`,
  `.config-tab-panel.hidden`) aggiunto in `index.html`.
- Struttura a lista di tab (`TABS`) in `_buildPanel` — estensibile senza refactoring.

---

## [0.19.0] — 2026-06-24 — Fix finestra /find vuota: buffer-and-replay risultati

### Fixed — Race tra apertura webview e raffica di eventi search_hit/search_done

La finestra di ricerca è una webview separata che si apre in modo asincrono; gli
eventi Tauri non sono bufferizzati, quindi hit/done emessi prima che la finestra
fosse in ascolto andavano persi, lasciandola vuota.

#### Frontend — `search-buffer.js` (nuovo modulo puro)

- **`createSearchBuffers()`**: gestore di buffer per-ricerca (per `sid`). Accumula
  `hit`/`done` finché la finestra non si iscrive; al `subscribe` ritorna il replay
  (hit nell'ordine d'arrivo + done se già ricevuto) e passa in modalità live
  (eventi successivi → `{ emit: true }`, emessi direttamente).
- Difensivo: `hit`/`done` su sid sconosciuto → `{ emit: true }` (nessuna perdita).
- Puro: nessun `import` di Tauri/DOM; testabile con `node:test` (6 test, tutti verdi).

#### Frontend — `app.js`

- Import di `createSearchBuffers` da `./search-buffer.js`; istanza `searchBuffers`
  a livello di modulo (condivisa tra tutti i `ServerMsg`).
- `case "search_open"`: chiama `searchBuffers.open(msg.id)` prima di aprire la finestra.
- `case "search_hit"`: emette l'evento Tauri solo se `searchBuffers.hit(...).emit === true`.
- `case "search_done"`: stesso pattern con `searchBuffers.done(...)`.
- `setupSearchEvents` — `search:cancel`: in più chiama `searchBuffers.close(sid)`
  per liberare memoria quando la ricerca viene annullata.
- `setupSearchEvents` — nuovo listener `search:subscribe { sid }`: rigioca dal buffer
  gli hit/done accumulati (replay), poi il buffer va live.

#### Frontend — `window-search.js`

- In fondo a `bootstrap()`, modalità live (dopo i listener `search:hit`/`search:done`):
  emette `search:subscribe { sid: mySid }` per avviare il replay dal main.
- La modalità "saved" non è interessata (torna prima con `return`).

### Invariato

Backend (`crates/orchestrator`, `crates/protocol`) e flusso WS
`SearchOpen/Hit/Done` invariati; cambia solo come la UI smista gli eventi.

---

## [0.18.0] — 2026-06-24 — Find window: ⏹ pause + Resume (Task SR4)

### Added — Pause/Resume in the live search window

#### Frontend — `search-status.js` + `search-status.test.mjs`

- **`statusLabel`** extended with `paused` parameter: `paused && !done` → `"⏸ in pausa — N risultati"`. Priority: `stopped` > `paused` > `done` > default. 4 new TDD tests (all 11 pass).

#### Frontend — `window-search.html` / `window-search.js`

- **`#stop-btn`** (⏹) now emits `search:pause` (instead of `search:cancel`); sets status to `"⏸ in pausa — N"`, hides ⏹, shows `#resume-btn`.
- **`#resume-btn`** (▶ "Riprendi"): new button, hidden by default. On click → emits `search:resume`, status returns to `"⏳ ricerca…"`, hides ▶, shows ⏹.
- **`×` (`#close-btn`)**: still emits `search:cancel` (cancel and close — unchanged behaviour).
- `#resume-btn` is also hidden in saved/replay mode (alongside ⏹ and 💾).
- `hitCount` state variable tracks running hit count for accurate pause label.
- On `search:done`: hides both `#stop-btn` and `#resume-btn`.

#### Frontend — `ws-client.js`

- **`pauseSearch(id)`**: sends `{ type: "pause_search", id }` over WebSocket.
- **`resumeSearch(id)`**: sends `{ type: "resume_search", id }` over WebSocket.

#### Frontend — `app.js`

- **`setupSearchEvents`**: two new Tauri event listeners — `search:pause` → `client.pauseSearch(sid)`; `search:resume` → `client.resumeSearch(sid)`.

### Verification

```
node --test crates/ui/frontend/search-status.test.mjs
tests 11 | pass 11 | fail 0
```

---

## [0.17.0] — 2026-06-24 — Find salvabili + Library a tab + riapertura

### Added — Save Find sessions + two-tab Library (Markdown | Find) + replay

This release consolidates three previously `[Unreleased]` slices (T1, T2, T3) and
introduces the final frontend piece (T4): the Library window gains a tab bar so the
user can browse both Markdown archives and saved Find sessions in the same window.

#### Rust — `src-tauri/src/archive.rs` (T1)

- **`ArchiveHit`** (`Serialize`, `Deserialize`): single file-search hit with `path` and `source` fields.
- **`ArchiveFind`** (`Serialize`, `Deserialize`): saved Find session — `query`, `ts` (ms since epoch), `hits: Vec<ArchiveHit>`.
- **`FindEntry`** (`Serialize`): metadata entry for `list_find` — `file`, `query`, `count`, `modified`.
- **`save_find(dir, query, hits) -> Result<String, String>`**: writes `find-{slug(query)}-{ts}.json` (pretty-printed JSON). Creates the directory if absent. Returns the plain filename.
- **`list_find(dir) -> Vec<FindEntry>`**: reads `*.json` files, deserializes each as `ArchiveFind`, skips unparseable files, sorts by mtime descending. Returns empty `Vec` if `dir` absent.
- **`open_find(dir, file) -> Result<ArchiveFind, String>`**: rejects path-traversal (`/`, `\`, `..`) via the shared `reject_traversal` guard, then deserializes and returns the `ArchiveFind`.
- **`delete_find(dir, file) -> Result<(), String>`**: same path-traversal guard, then removes the file.
- 5 new TDD tests: round-trip (`save_find` → `list_find` → `open_find` → `delete_find`); `open_find` rejects `../x.json` and `sub/x.json`; `delete_find` rejects the same patterns.

#### Rust — `src-tauri/src/main.rs` (T2)

- **`library_find_dir_path(app)`**: resolves to `library_dir_path(app)?.join("find")`.
- Setup: `create_dir_all` also for `library_find_dir_path` (best-effort, alongside the existing `library` dir).
- Tauri commands: `save_find`, `list_find`, `open_find`, `delete_find` — all delegate to `archive::*_find` using `library_find_dir_path`.
- **`open_saved_find_window(app, file)`**: reads `open_find`, serializes the `ArchiveFind` as JSON, creates a `WebviewWindow` `search-*` with content `"saved:" + json` in the `WindowContentStore`.  The search window boots in replay (saved) mode.

#### Frontend — `window-search.html` / `window-search.js` (T3)

- New `#save-btn` (💾) in the title-bar (between ⏹ and ×); hidden initially, shown only in live mode.
- **`wireSaveButton(query)`**: idempotent click handler — collects current hits from `groups` state (`path` via `textContent`); invokes `save_find({ query, hits })`; on success → "✓ Salvato" + permanently disabled for the session; on error → "✗" for 1.5 s + re-enable for retry.
- **Saved mode (replay)**: if `data.content` starts with `"saved:"` → parse `ArchiveFind` JSON; replay hits via `addHit(source, path)`; final status `✅ N risultati (salvati)`; no live `search:hit`/`search:done` listeners; ⏹ and 💾 hidden.
- **Live mode** (existing behaviour): `mySid = data.content`; calls `wireSaveButton(title)`.

#### Frontend — `library.html` / `library.js` (T4)

- New `#tab-bar` (between title-bar and list area): two `<button class="lib-tab">` elements (`Markdown` | `Find`).  The active tab gets `.lib-tab--active` (accent underline + brighter text).  Full ARIA `role="tablist"` / `role="tab"` / `aria-selected` markup.
- **`activeTab`** state (`"markdown"` | `"find"`); switching tab clears the list and reloads.
- **Markdown tab**: `archive_list` → `buildMarkdownItem`; open via `archive_open` + `open_markdown_window`; delete via `archive_delete`.
- **Find tab**: `list_find` → `buildFindItem` (shows query + hit count + date); open via `open_saved_find_window`; delete via `delete_find`.  `list_find` returns `Result` from Rust → `invokeCmd` throws on `Err`; caught with try/catch → empty state shown (same pattern as Markdown).
- Two-step 🗑 confirm (3 s timeout) preserved for both tabs via shared `wireDeleteButton(deleteBtn, row, file, deleteCmd)`.
- `formatDate` extended to handle both ms (`modified_ms`, Markdown) and seconds (`modified`, Find).
- All text set via `textContent` only — no `innerHTML` with external data.

### Verification

```
cargo test -p ui archive
running 27 tests
test result: ok. 27 passed; 0 failed; 0 ignored

cargo clippy -p ui
Finished dev profile — no warnings

node --test crates/ui/frontend/search-status.test.mjs
  pass 7 / fail 0  (no regressions)

node --check crates/ui/frontend/window-search.js
  (no output — syntax OK)
```

E2e GUI (💾 on a live `/find`; Find tab in Library; replay window with clickable results)
requires an interactive run by the user.

---

## [0.16.0] — 2026-06-23 — Scrollback output: PgUp/PgDn + `pageDelta`

### Added — scrollback keyboard navigation of `#output-area`

#### Frontend — `frontend/page-scroll.js` (nuovo modulo)

- **`export function pageDelta(clientHeight, overlap = 24): number`**: helper puro,
  niente DOM/IO. Calcola i pixel da scorrere per un passo PageUp/PageDown: una
  "pagina" meno un overlap di contesto (default 24 px), con minimo 40 px. Gestisce
  input non finiti (`NaN`, `undefined`) restituendo il minimo.
- 4 test TDD in `page-scroll.test.mjs` (RED → GREEN): pagina standard, pannello
  piccolo/zero, overlap personalizzato, input non finito.

#### Frontend — `frontend/app.js`

- Import aggiunto in cima: `import { pageDelta } from "./page-scroll.js";`.
- Ramo `PageUp`/`PageDown` aggiunto nel keydown handler di `hiddenInput`, posizionato
  prima dei rami ArrowLeft/ArrowRight (fa `return` prima della logica di editing):
  - `e.preventDefault()` per entrambi.
  - `outputArea.scrollBy({ top: ±pageDelta(outputArea.clientHeight) })`.
  - ↑/↓ (`ArrowUp`/`ArrowDown`) **non** gestiti qui — riservati alla futura history comandi.

#### Verification

```
node --test crates/ui/frontend/page-scroll.test.mjs
  ✔ una pagina meno l'overlap di default
  ✔ minimo 40 px quando il pannello è piccolo
  ✔ overlap personalizzato
  ✔ input non finito → minimo
  pass 4 / fail 0
```

E2e GUI (PgUp/PgDn scorrono `#output-area`) richiede run interattivo utente.

---

## [0.15.0] — 2026-06-23 — Tier 1 "apri cartella": Library + Find + parentDir

### Added — pulsante "Apri cartella" nella Library, pulsante per-riga in Find, helper `parentDir`, comando Tauri `library_dir`

#### Rust — `src-tauri/src/main.rs`

- **`fn library_dir(app: AppHandle) -> Result<String, String>`** (nuovo Tauri command):
  - Ritorna `library_dir_path(&app)?.to_string_lossy().into_owned()`.
  - Usato da `library.js` per recuperare il path della cartella archivio prima di
    emettere l'evento globale `library:open-folder`.
  - Registrato in `invoke_handler![..., library_dir]`.
  - Non richiede capability aggiuntive (coperto da `core:default` in
    `library-window.json`).
- **Setup: ensure della cartella library** — nel `.setup(|app| …)`, `create_dir_all`
  su `library_dir_path` (idempotente, best-effort): la cartella archivio esiste
  sempre, così il pulsante "Apri" funziona anche prima del primo salvataggio.
  La UI possiede questo path (Tauri `app_config_dir`), quindi è il lato giusto.

#### Frontend — `frontend/library.html`

- Nuovo pulsante `#open-dir-btn` (U+1F4C2) nell'header (`#titlebar`), a **sinistra**
  di `#close-btn`. Attributi: `title="Apri la cartella nel file manager"`, `aria-label`.
- Stile condiviso con `#close-btn` via selettore `#open-dir-btn, #close-btn`; hover
  azzurro (`rgba(130,200,255,0.9)`) distinto dal rosso della ×.

#### Frontend — `frontend/library.js`

- Import `tauriEvent = window.__TAURI__?.event`.
- DOM ref `openDirBtnEl = document.getElementById("open-dir-btn")`.
- Click handler: `await invokeCmd("library_dir")` → `await tauriEvent.emit("library:open-folder", { path })`.

#### Frontend — `frontend/app.js`

- Nuova funzione `setupLibraryEvents()`: ascolta `library:open-folder` e invia
  `/open <path>` al backend via `handleSlashCommand` (stesso schema di `search:open-path`).
- `bootstrap()`: chiama `setupLibraryEvents()` dopo `setupSearchEvents()`.

#### Frontend — `frontend/path-utils.js` (Task T2, in scope per questo bump)

- `export function parentDir(path)`: helper puro, gestisce `\` e `/`, radice drive e POSIX.
- 6 test TDD in `path-utils.test.mjs` (RED → GREEN).

#### Frontend — `frontend/window-search.html` / `window-search.js` (Task T3, in scope)

- Pulsante 📂 `.row-folder-btn` per ogni risultato Find; click → `openPath(parentDir(path))`.
- Path via `textContent` (mai `innerHTML`).

#### Verification

```
cargo check -p ui
  Compiling ui v0.15.0
  Finished dev profile — no errors, no warnings
```

E2e GUI (📂 Library apre cartella archivio; 📂 Find apre cartella del file) richiede
run interattivo utente.

---

## [0.14.0] — 2026-06-23 — cwd line above the prompt

### Added — `#cwd-line` element + `ServerMsg::Cwd` handler + `home_dir` Tauri command

#### Frontend — `frontend/index.html`

- New `<div id="cwd-line" class="cwd-line hidden"></div>` placed immediately above
  `.input-row` (below `#output-area`).  Initially hidden; revealed on the first
  `cwd` message from the orchestrator.
- CSS `.cwd-line`: `font-size: 11px`, same muted colour as `.hint`
  (`rgba(110, 150, 190, 0.45)`), `white-space: nowrap` with `text-overflow: ellipsis`
  so very long paths clip gracefully instead of breaking the layout.
- CSS `.hidden { display: none }` (utility class, used by `#cwd-line` before the
  first `cwd` message arrives).

#### Frontend — `frontend/app.js`

- Import: `import { formatCwd } from "./cwd-format.js"` (Task 5 module).
- New DOM reference: `cwdLineEl = document.getElementById("cwd-line")`.
- New module-level variable `homeDir = ""`: stores the result of `home_dir` (fetched
  once at bootstrap); used as the second argument to `formatCwd()`.
- `bootstrap()`: after `get_config`, calls `invokeCmd("home_dir")` and stores the
  result in `homeDir`.  Errors are caught and logged; `homeDir` stays `""` so
  `formatCwd()` skips `~` substitution gracefully.
- `handleServerMsg()`: new `case "cwd"` — updates `cwdLineEl.textContent` with
  `formatCwd(msg.path, homeDir)` and removes the `.hidden` class.  Path is set via
  `textContent` only (never `innerHTML`) — no XSS risk.

#### Frontend — `frontend/ws-client.js`

- Updated wire-format comment to document `ServerMsg::Cwd → {"type":"cwd","path":"..."}`.
  No behaviour change: all server messages were already forwarded to the `onMessage`
  callback unconditionally.

#### Rust — `src-tauri/src/main.rs`

- New Tauri command `home_dir() -> String`: returns `dirs::home_dir()` as a string,
  or `""` if the home directory cannot be resolved.  Registered in
  `invoke_handler![..., home_dir, ...]`.

#### Rust — `src-tauri/Cargo.toml`

- Added `dirs = "5"` dependency (resolves to `dirs 5.0.1` in the lock file; the
  `home_dir()` API is stable across v5).
- Version bumped `0.13.0` → `0.14.0`.

#### Verification

```
cargo check -p ui
  Compiling ui v0.14.0
  Finished dev profile — no errors, no warnings
```

E2e GUI verification (connect → cwd line appears as `~`; `cd <dir>` → line updates)
is pending the user's interactive run.

---

## [0.13.0] — 2026-06-23 — stop button nella finestra di ricerca

### Added — pulsante ⏹ per fermare una ricerca senza chiudere la finestra
- `window-search.html`/`.js`: pulsante **⏹** nella title-bar (a sinistra della ×),
  visibile solo mentre la ricerca è attiva. Click → emette `search:cancel {sid}`
  (annulla tutti i task della ricerca) **senza** chiudere: la finestra resta coi
  risultati parziali cliccabili. Stato → "⏹ interrotta — N risultati".
- Logica della label estratta in `search-status.js` (`statusLabel`, funzione pura)
  con 7 test `node:test` (`search-status.test.mjs`). La × resta invariata
  (annulla **e** chiude).
- Solo frontend: nessuna modifica a `protocol`/`orchestrator`.

---

## [0.12.0] — 2026-06-22 — finestra di ricerca file

### Added — finestra di ricerca live (Task 11, ricerca file)

Nuova finestra dedicata e **live** per i risultati di `/find`:
- `window-search.html` + `window-search.js`: finestra trasparente chromeless (come
  quelle Markdown) con risultati **raggruppati per sorgente** (Cartella corrente →
  Standard → Cloud → Unità esterne), contatori live e stato (⏳ → ✅ N, "(troncati)").
- Comando Rust `open_search_window(title, sid)` (in `main.rs`): crea la `WebviewWindow`
  (`search-*`); riusa `WindowContentStore`/`take_window_content` per passare `(title, sid)`
  (campo `content` = `sid`). Nuova capability `search-window.json` (scoped `search-*`).
- `app.js`: gestisce `search_open` (apre la finestra), `search_hit`/`search_done`
  (inoltrati alla finestra via **eventi Tauri globali**, filtrati per `sid`).
- Interazione: **click** su un risultato → la finestra emette `search:open-path` → il
  main invia `/open <path>`; **chiusura** (×/Esc) → emette `search:cancel` → il main
  invia `CancelSearch{sid}` (`ws-client.js` guadagna `cancelSearch(id)`).
- Sicurezza: i path sono inseriti solo via `textContent` (mai `innerHTML`).

---

## [0.11.2] — 2026-06-22

### Fixed
- Una finestra **riaperta dall'archivio** (da `/library`) non mostra più il
  pulsante 💾 — è già salvata. `library.js` la apre con `kind: "archived"`,
  `window.js` aggiunge `body.archived`, e `window.html` nasconde il pulsante
  (`body.archived #save-btn { display: none }`).

---

## [0.11.1] — 2026-06-22

### Fixed
- `frontend/window.html`: la finestra `/help` (di sistema, `body.help`) non mostra
  più il pulsante 💾 — non ha senso archiviarla. Regola CSS `body.help #save-btn { display: none }`.

---

## [0.11.0] — 2026-06-22

### Added — Salvataggio idempotente + eliminazione dall'archivio

#### Rust — `src-tauri/src/archive.rs`

- **Helper `reject_traversal(file: &str) -> Result<(), String>`** (privato):
  - Estrae la guardia anti-path-traversal precedentemente inline in `open` in un
    helper condiviso. Rifiuta `file` contenente `/`, `\`, o `..`. DRY/SOLID:
    singolo punto di modifica se le regole cambiano.
  - Usato da `open` e `delete` (entrambi chiamano `reject_traversal(file)?`).
- **`pub fn delete(dir: &Path, file: &str) -> Result<(), String>`**:
  - Elimina `{dir}/{file}` dopo aver validato `file` via `reject_traversal`.
  - Errore leggibile se il file non esiste o non è eliminabile.
  - Scritta TDD: 5 test RED→GREEN prima dell'implementazione.
- **Test TDD — `delete`** (5 nuovi, aggiunti prima del codice produzione):
  - `delete_removes_file_from_archive` — `save → delete → list` vuota.
  - `delete_nonexistent_file_returns_err` — file assente → `Err`.
  - `delete_rejects_dotdot_traversal` — `"../x"` → `Err` con "traversal".
  - `delete_rejects_slash_in_filename` — `"a/b.md"` → `Err` con "traversal".
  - `delete_rejects_backslash_in_filename` — `"a\\b.md"` → `Err` con "traversal".

#### Rust — `src-tauri/src/main.rs`

- **`fn archive_delete(app, file: String) -> Result<(), String>`** (nuovo Tauri command):
  - Risolve `library_dir_path(app)` (stesso helper degli altri comandi archive).
  - Delega a `archive::delete(&dir, &file)`.
  - Registrato in `invoke_handler![..., archive_delete, ...]`.
  - Coperto dalla capability `library-window.json` via `core:default` — nessuna
    modifica alle capabilities necessaria.

#### Frontend — `frontend/window.js`

- **Salvataggio idempotente (#save-btn)**:
  - Il click handler ora disabilita `saveBtnEl` **immediatamente all'ingresso**
    (guard anti-duplicato, copre anche click in-flight: se il primo click è ancora
    in attesa di `archive_save`, il secondo trova il bottone già disabilitato).
  - Su **successo**: il bottone rimane disabilitato e mostra "✓ Salvato" (stabile
    per la sessione della finestra — nessun ripristino automatico).
  - Su **errore**: il bottone viene riabilitato, mostra "✗" per 1.5 s, poi
    ripristina il testo originale (comportamento esistente preservato per il caso
    di errore).

#### Frontend — `frontend/library.html`

- **CSS `.archive-item-delete`**: bottone elimina per-riga; colore muted, hover
  rosso, `-webkit-app-region: no-drag` (non drag-target).
- **CSS `.archive-item-delete--confirm`**: stato "conferma" — colore rosso
  acceso, testo in grassetto, font-size ridotto per il testo "Conferma".

#### Frontend — `frontend/library.js`

- **`wireDeleteButton(deleteBtn, row, file)`** (nuova funzione):
  - Implementa la conferma in due passi con timeout 3 s.
  - Primo click: arma lo stato confirm (`.archive-item-delete--confirm`, testo
    "Conferma"); imposta `setTimeout(resetConfirm, 3000)`.
  - Secondo click entro 3 s: chiama `invokeCmd("archive_delete", { file })`.
    Su successo: `row.remove()`; se la lista rimane vuota, ripristina stato empty
    (`className = "empty"`, `textContent = "Nessun documento archiviato."`).
    Su errore: `resetConfirm()` + `flashRowError(row)` (pattern esistente).
  - Timeout scaduto: `resetConfirm()` (tornare allo stato normale).
  - `stopPropagation` su **`click` E `dblclick`**: impedisce che un doppio-click
    rapido sul 🗑 bubbli fino al row handler `dblclick → openDocument`.
- **Guard `e.target !== row`** nel listener `keydown` → `Enter`: Enter apre il
    documento solo quando la riga stessa ha il focus; se il focus è sul bottone 🗑
    figlio, il browser attiva nativamente il bottone (click → arm/confirm delete),
    evitando la collisione tra Enter-sul-bottone e openDocument.
- **`buildItem(entry)`**: aggiunge il `deleteBtn` generato da `wireDeleteButton`;
  appeso al row dopo `dateSpan`. Tutto via `createElement`/`textContent` — nessun
  `innerHTML` con dati esterni. `CSS.escape` usato solo dove si interroga per
  `data-file` (in `openDocument`).

### Changed

- `archive::open` ora usa `reject_traversal(file)?` al posto del check inline —
  comportamento identico, codice deduplicato.

---

## [0.10.0] — 2026-06-21

### Added — Archivio B2: browser `/library` + pulsante pannello + riapertura

#### Rust — `src-tauri/src/main.rs`

- **`open_library_window(app) -> Result<(), String>`** (async Tauri command):
  - Singleton: se esiste già una `WebviewWindow` con label `"library"`, chiama
    `set_focus()` e ritorna (non apre duplicati).
  - Altrimenti crea la finestra con label `"library"`, carica `library.html`.
  - Stessa configurazione delle finestre Markdown: `decorations(false)`,
    `transparent(true)`, `always_on_top(true)`, `focused(true)`.
  - Dimensioni: 460×560 (lista compatta). `resizable(false)` — la finestra è
    a larghezza fissa; il contenuto scorre verticalmente.
  - Dichiarata `async` per evitare il deadlock Windows in `build()` (stessa
    ragione di `open_markdown_window`).
  - Registrata in `invoke_handler![..., open_library_window]`.
- **F2 hide/show handler**: la condizione di hide/show ora include anche il
  label `"library"`, così la finestra archivio si nasconde e riappare insieme
  al resto della UI (stessa estensione del fix BUG-003 per "md-*").

#### Rust — `src-tauri/capabilities/library-window.json` (nuovo)

- Capability per la finestra `"library"` (label fisso, non compreso da `md-*`
  o `main`).
- Permessi: `core:default` (IPC per `archive_list`, `archive_open`,
  `open_markdown_window`, `close_self`) + `core:window:allow-start-dragging`
  (drag region del titlebar chromeless).
- Senza questo file la finestra library avrebbe zero permessi Tauri e tutti
  gli invoke fallirebbero silenziosamente a runtime.

#### Frontend — `frontend/library.html` (nuovo)

- Stessa estetica di `window.html` (palette `--bg` 0.87, bordo blu, font
  Consolas, border-radius 10px, trasparente).
- `#titlebar` con `data-tauri-drag-region` + pulsante `#close-btn` (×).
- Titolo barra: "Lare — Archivio".
- `#list-area` (`role="listbox"`) scrollabile: accoglie le righe `.archive-item`.
- `#footer` hint: "Invio o doppio-clic per riaprire".
- Carica `<script type="module" src="library.js">`.
- NO vendor libs (non serve markdown rendering).

#### Frontend — `frontend/library.js` (nuovo)

- Al load: `invoke("archive_list")`.
  - Array vuoto / lista vuota → mostra "Nessun documento archiviato."
  - Errore invoke → `showError(msg)` (messaggio discreto, non crash).
- **Render lista**: una `.archive-item` per voce, costruita con
  `createElement`/`textContent` (mai `innerHTML` con dati esterni).
  - Colonne: titolo (troncato con `text-overflow: ellipsis`) + data leggibile
    (`new Date(entry.modified_ms).toLocaleString("it-IT", ...)`).
  - `tabindex="0"`, `role="option"`, `data-file` → focusable + navigabile.
  - `data.modified_ms` (snake_case): coerente con la serializzazione Rust di
    `ArchiveEntry` (nessun `rename_all`; la conversione camelCase si applica
    solo agli argomenti, non ai valori di ritorno).
- **Riapertura** (`openDocument(file)`):
  - `invoke("archive_open", { file })` → `{ title, content }`.
  - `invoke("open_markdown_window", { title, content, kind: "markdown" })`.
  - La finestra archivio resta aperta.
  - Errori: `flashRowError(row)` — flash rosso transiente (1,5 s) sulla
    singola riga fallita; la lista NON viene cancellata e le altre voci
    restano accessibili. `showError` è riservata al solo fallimento bootstrap
    (`archive_list` fallisce), dove non c'è lista da preservare.
- **Interazione**:
  - `dblclick` su una voce → `openDocument`.
  - `Enter` su voce focalizzata → `openDocument`.
  - `ArrowUp` / `ArrowDown` → sposta focus tra le voci (wrap circolare).
  - `Escape` → `close_self` (stessa tecnica di window.js; NON usa
    `getCurrentWindow().close()` che richiede `core:window:allow-close`
    non presente in `core:default`).
  - Prima voce riceve focus all'apertura.
- **Close button** (`#close-btn`): `close_self` via IPC (mirror esatto di
  window.js).

#### Frontend — `frontend/app.js`

- Nuovo ramo in `handleSlashCommand`: se `cmd === "/library"` → UI-local →
  `invokeCmd("open_library_window")` + `return` prima del send WS.
  (Pattern identico a `/config`.)
- Nuovo listener su `#library-btn`:
  - Cerca `document.getElementById("library-btn")` (guard `if (libraryBtnEl)`).
  - Click → `invokeCmd("open_library_window")`.
  - Il global click handler chiama `grabFocus()` in seguito: corretto — la
    finestra library si apre async, e il focus torna all'input.

#### Frontend — `frontend/index.html`

- `header-right`: aggiunto `<button id="library-btn" title="Archivio (/library)">📚</button>`
  prima di `#status-badge` (dentro `.header-right`).
- CSS `#library-btn`: trasparente, colore muted `rgba(130,190,255,0.45)`,
  `:hover` → `0.85`; `margin-right: 4px`; no-drag.
- `.header-right`: aggiunto `gap: 6px` per spaziatura uniforme degli elementi.
- `.activity-status`: rimosso `margin-right: 10px` (ora gestito dal `gap`).
- Hint row: aggiunto `/library` tra `/config` e `/help`.

#### TDD

La logica di questa feature è prevalentemente DOM/IPC (difficile da unit-testare
senza Tauri runtime e senza un browser reale). Non sono stati scritti nuovi test
unitari Rust (la logica è tutta in `library.js` e nel comando Tauri).

Verifica automatica:
- `cargo test -p ui`: 35/35 pass (invariato — nessuna regressione).
- `cargo build -p ui`: compila clean (nuovo comando + invoke_handler).
- `cargo clippy -p ui --all-targets -- -D warnings`: pulito, no warning.
- `node --check library.js` → OK (no output, verificato dopo fix `flashRowError`).
- `node --check app.js` → OK (no output).
- `node --test line-editor.test.mjs`: 24/24 pass (no regressioni).

#### Note per la verifica interattiva (supervisore)

- **`/library` da tastiera**: digitare `/library` + Invio → finestra archivio
  si apre. Digitare di nuovo `/library` → finestra portata in focus (singleton).
- **Pulsante 📚**: cliccare il pulsante in alto a destra → stessa apertura.
- **Lista vuota**: se nessuna finestra è mai stata salvata → "Nessun documento
  archiviato."
- **Lista popolata**: dopo aver salvato con 💾 su una finestra `/show` →
  doppio-clic o Invio su una voce → apre una nuova finestra Markdown con il
  contenuto archiviato.
- **F2 hide/show**: con la finestra library aperta → F2 → si nasconde con il
  pannello principale. F2 di nuovo → riappare.
- **Escape**: Esc nella finestra library → chiude.

#### Deviazioni dalla spec

1. `close_self` (comando Rust esistente) invece di `getCurrentWindow().close()`
   (API JS Tauri). Motivazione: `getCurrentWindow().close()` richiede
   `core:window:allow-close` non incluso in `core:default`; `close_self` è già
   registrato e funziona senza capability aggiuntive. Comportamento identico.
2. `resizable: false` — la spec non specifica; scelta per una finestra lista
   compatta. Il supervisore può cambiare in `true` se preferisce.

#### Version bump

- `src-tauri/Cargo.toml`: `0.9.0` → `0.10.0`.

---

## [0.9.0] — 2026-06-21

### Added — Archivio B1: modulo `archive.rs` + comandi Tauri + pulsante salva

#### Rust — `src-tauri/src/archive.rs` (nuovo modulo, logica pura)

- **`ArchiveEntry { title, file, modified_ms }`** — metadata di un file archiviato.
  Deriva `Debug, Clone, PartialEq, Serialize`.
- **`ArchiveDoc { title, content }`** — contenuto aperto da archivio.
  Deriva `Debug, Clone, PartialEq, Serialize`.
- **`slug(title: &str) -> String`**: slug ASCII-safe.
  - Transliterazione accenti (é→e, à→a, ñ→n, ß→ss, æ→ae, œ→oe, ecc.) via tabella `transliterate`.
  - Lowercase + sostituisce non-alfanumerici con `-`.
  - Collassa dashes consecutivi, taglia leading/trailing `-`.
  - Tronca a 40 caratteri senza lasciare `-` finale.
  - Input vuoto o tutto-punteggiatura → `"untitled"`.
- **`save(dir, title, content, suffix) -> Result<String, String>`**:
  - Crea `dir` se assente (`create_dir_all`).
  - Scrive `{slug(title)}-{suffix}.md` con marker `<!-- lare-title: {title} -->` in prima riga.
  - Sanifica il marker stripping `-->` dal titolo (previene injection).
  - Ritorna il nome file creato.
- **`list(dir) -> Vec<ArchiveEntry>`**:
  - Dir assente → `vec![]` (non è errore).
  - Legge i `.md` in `dir`, estrae title dal marker (fallback: stem del nome file).
  - Ordina per `modified_ms` desc (file più recente prima).
- **`open(dir, file) -> Result<ArchiveDoc, String>`**:
  - Guarda traversal PRIMA di qualsiasi FS access: rifiuta `file` se contiene `/`, `\`, o `..`.
  - Legge il file, separa la prima riga (marker) dal resto (content).
  - Title estratto dal marker; content = tutto dopo la prima riga (senza marker).

#### Rust — `src-tauri/src/lib.rs`

- Aggiunto `pub mod archive;` (modulo esposto per i test headless).

#### Rust — `src-tauri/src/main.rs`

- Import: `use ui_lib::archive;`.
- Nuova helper `library_dir_path(app) -> Result<PathBuf, String>`:
  stessa radice di `config_file_path` (app_config_dir), sottopath `"library"`.
- **`archive_save(app, title, content) -> Result<String, String>`**:
  genera suffix timestamp (`SystemTime::now()` → ms), chiama `archive::save`.
- **`archive_list(app) -> Vec<archive::ArchiveEntry>`**:
  chiama `archive::list`; su errore path resolution → `vec![]` + log stderr.
- **`archive_open(app, file) -> Result<archive::ArchiveDoc, String>`**:
  chiama `archive::open` (la validazione traversal è nel modulo puro).
- Tutti e 3 i comandi registrati in `invoke_handler![..., archive_save, archive_list, archive_open]`.

#### Frontend — `frontend/window.html`

- CSS `#close-btn, #save-btn`: stile condiviso (era solo `#close-btn`).
- Nuovo `#save-btn:hover`: colore `rgba(130, 220, 130, 0.9)` (verde, distingue da ×).
- Titlebar HTML: aggiunto `<button id="save-btn">💾</button>` PRIMA di `#close-btn`,
  entrambi wrappati in un `<span>` flex per raggruppamento; `-webkit-app-region: no-drag` garantito.

#### Frontend — `frontend/window.js`

- Nuovo riferimento DOM: `const saveBtnEl = document.getElementById("save-btn")`.
- Inside `bootstrap()`, dopo il caricamento di `data`:
  - Cattura `saveTitle = data.title` e `saveContent = data.content` (Markdown grezzo,
    non l'HTML renderizzato — si archivia la sorgente, non l'output di DOMPurify).
  - `saveBtnEl.addEventListener("click", async () => { ... })`:
    - Invoca `archive_save({ title: saveTitle, content: saveContent })`.
    - Successo → `saveBtnEl.textContent = "✓"` per 1.5s, poi ripristina.
    - Errore → `console.error(...)` + `saveBtnEl.textContent = "✗"` per 1.5s, poi ripristina.

#### TDD

**Nota trasparenza (come in `config.rs`):** test e implementazione sono stati scritti
nella stessa sessione senza un commit RED separato. Non si è osservato un RED genuino;
il ciclo RED→GREEN non è stato commit-separato. Il supervisore deve decidere se questa
sessione richiede un ciclo separato per le prossime feature.

17 test in `archive::tests`:
- `slug_*` (10): spazi, lowercase, accenti, punteggiatura, dashes collassati, trim, troncamento, troncamento-no-dash, input-vuoto, tutto-punteggiatura.
- `round_trip_*` (2): title+content preservati, marker non incluso nel content ritornato.
- `list_absent_dir_returns_empty`.
- `open_rejects_*` (3): dotdot, slash, backslash.
- `list_sorts_by_modified_desc` (usa `File::set_modified` per mtime deterministici).

#### Deviazioni dalla spec (`Docs/13-window-archive-help.md`)

1. `archive_save` ritorna `Result<String, String>` (nome file creato) invece di `Result<(), String>`.
   Utile per debugging; il JS ignora il valore di ritorno (non ne ha bisogno).
2. Il marker è un commento HTML (`<!-- lare-title: … -->`) invece di YAML front-matter.
   Il documento li tratta come equivalenti ("front-matter con il titolo").
   Il commento HTML è ignorato da `marked`/DOMPurify se il file viene ri-aperto come Markdown.

#### Verifica

```
cargo test --lib -p ui
  running 35 tests
  test result: ok. 35 passed; 0 failed (17 archive + 18 config)

cargo build -p ui
  Compiling ui v0.9.0 — Finished dev profile, no errors

cargo clippy --all-targets -- -D warnings
  Finished dev profile — no warnings

node --check crates/ui/frontend/window.js → OK (no output)
```

#### Note verifica interattiva (richiesta al supervisore)

- **Pulsante salva**: cliccare 💾 su una finestra `/show` aperta; atteso `✓` per 1.5s
  e file `.md` in `%LOCALAPPDATA%\dev.lare.terminal\library\`.
- **`archive_list` / `archive_open`**: non hanno caller in B1 (B2 = `/library` browser).
  Compile-verified soltanto.
- **No nuova capability**: `archive_save` è un custom command come `close_self`/`resize_self`,
  coperto da `core:default` nella capability `markdown-window.json` esistente.

#### Version bump

- `src-tauri/Cargo.toml`: `0.8.0` → `0.9.0`.

---

## [0.8.0] — 2026-06-21

### Added — Supporto `kind` nella pipeline finestre (`/help` visual identity)

#### Rust — `src-tauri/src/main.rs`

- **`WindowContentStore`**: tipo della tuple cambiato da `(String, String)` a
  `(String, String, String)` — i tre campi sono `(title, content, kind)`.
  `kind` è il wire value di `protocol::WindowKind` (es. `"markdown"`, `"help"`).

- **`open_markdown_window`**: aggiunto parametro `kind: String` (dopo `content`).
  Il valore viene memorizzato nella tupla dentro `WindowContentStore`.

- **`take_window_content`**: il JSON restituito ora include il campo `"kind"`:
  `{ "title": ..., "content": ..., "kind": ... }`.
  `window.js` legge `kind` per applicare classi CSS alle finestre speciali.

#### Frontend — `frontend/app.js`

- **`case "open_window"`**: passa `msg.kind` a `openMarkdownWindow`.
- **`openMarkdownWindow(title, content, kind = "markdown")`**: aggiunto terzo
  parametro `kind` (default `"markdown"` per retro-compatibilità con `/show`);
  viene incluso nel payload `invoke("open_markdown_window", { title, content, kind })`.

#### Frontend — `frontend/window.js`

- Dopo il rendering del Markdown, verifica `data.kind`:
  se `data.kind === "help"` → aggiunge `document.body.classList.add("help")`.
  Questo hook è generalizzabile: future varianti `WindowKind` possono
  aggiungere le proprie classi CSS senza toccare la logica di rendering.

#### Frontend — `frontend/window.html`

- Nuova regola CSS `body.help`:
  - `--border: rgba(120, 180, 255, 0.80)` (blu più luminoso del default 0.55).
  - `border-width: 3px` (più spesso del default 1.5px).
  - Identifica visivamente la finestra `/help` come finestra di sistema.
  - **Nota cosmetic**: `window.js` aggiunge `+4 px` nell'auto-height per il
    bordo 1.5px. Con bordo 3px la finestra sarà ~3px più corta del previsto.
    Accettabile; un resize-handle custom è fuori scope.

#### Frontend — `frontend/index.html`

- Hint row: aggiunto `/help per i comandi` dopo `/config to configure`.

#### Version bump

- `src-tauri/Cargo.toml`: `0.7.0` → `0.8.0`.

#### Verification outputs

```
cargo test -p protocol
  running 30 tests — test result: ok. 30 passed; 0 failed

cargo test -p orchestrator
  running 106 unit tests + 4 integration tests — all pass; 3 ignored

cargo test -p ui
  running 18 tests — test result: ok. 18 passed; 0 failed

cargo clippy -p protocol -p orchestrator -p ui --all-targets -- -D warnings
  Finished dev profile — no warnings

node --check app.js / window.js → OK (no errors)

node --test crates/ui/frontend/line-editor.test.mjs
  pass 24, fail 0
```

#### Deviation from spec — spinner fix

`Docs/13-window-archive-help.md` says `/help` → "un solo `ServerMsg::OpenWindow`".
This was NOT implemented as specified. The orchestrator emits
`[OpenWindow{Help}, Done{exit_code: Some(0)}]` (two messages) because:
1. The UI's `handleSlashCommand` calls `commandStarted(slashId)`.
2. `commandEnded` only fires on `done` or `error` — never on `open_window`.
3. Shipping one-message `/help` would produce a permanently stuck spinner,
   re-introducing the regression fixed in orchestrator 0.8.0 for `/show`.

The supervisor should update the spec document (Docs/13-window-archive-help.md)
to reflect the two-message pattern.

---

## [0.7.0] — 2026-06-21

### Added — Config `web_search_enabled` + toggle UI + invio flag nei Command

#### Rust — `src-tauri/src/config.rs`

- Nuovo campo `web_search_enabled: bool` su `Config`, annotato con
  `#[serde(default = "default_true")]` (valore di default `true`): i `config.json`
  esistenti senza questo campo continuano a caricarsi correttamente.
- Helper privato `fn default_true() -> bool { true }` richiesto da
  `#[serde(default = "...")]` (il default di `bool` è `false`, non `true`).
- `impl Default for Config`: aggiunto `web_search_enabled: true`.
- 3 nuovi test TDD (RED → GREEN):
  - `default_web_search_enabled_is_true` — `Config::default().web_search_enabled == true`.
  - `missing_web_search_field_defaults_to_true` — scrive un JSON con `action_key: "F7"` ma
    senza `web_search_enabled`; verifica che il parse riesca (action_key preservato a "F7",
    non ricaduto a "F2") **e** che `web_search_enabled == true`. Il doppio assert è il test
    discriminante: se manca `#[serde(default = "default_true")]`, serde fallisce, `load_from`
    torna al default con `action_key == "F2"`, e la prima asserzione fallisce.
  - `round_trip_custom_config` aggiornato con `web_search_enabled: false` per coprire il
    round-trip di tutti i campi.
- `cargo test -p ui`: 18/18 pass (era 16).

#### Frontend — `frontend/config-dialog.js`

- Oggetto `current` di default in `open()`: aggiunto `web_search_enabled: true`.
- Nuovo metodo `_buildCheckbox(parent, labelText, id, checked)`: costruisce una riga
  `config-field-row` con `<label>` + `<input type="checkbox">` (riusa il pattern delle
  altre righe; `!!checked` coerce `undefined`/`null` a `false`).
- `_buildPanel`: dopo `activitySelect`, chiama `_buildCheckbox` con label "Ricerca web" e
  id `config-web-search`; cattura il risultato in `webSearchInput`.
- `newCfg` nell'handler Salva: aggiunto `web_search_enabled: webSearchInput.checked`.

#### Frontend — `frontend/ws-client.js`

- `sendCommand(input, id)` → `sendCommand(input, id, webSearch = false)`: aggiunto
  terzo parametro opzionale (default `false`) e campo `web_search: !!webSearch` nel
  payload inviato via WebSocket. Guard `if (!this._isOpen()) return false` e `return true`
  finali preservati.
- Commento wire format aggiornato con `"web_search":false`.

#### Frontend — `frontend/app.js`

- Variabile di modulo `let webSearchEnabled = true;` (dopo la sezione CSS vars, prima del
  config dialog).
- `bootstrap()`: dopo `get_config`, aggiunge
  `webSearchEnabled = cfg.web_search_enabled !== false;`
  (pattern `!== false` è robusto contro `undefined` in caso di versione di config vecchia).
- Callback `onSaved` del `ConfigDialog`: aggiunge
  `webSearchEnabled = savedCfg.web_search_enabled !== false;`
  per aggiornamento immediato senza restart.
- Call site Enter (`client.sendCommand(input, cmdId)`):
  → `client.sendCommand(input, cmdId, webSearchEnabled)`.
- Call site `handleSlashCommand` (`client.sendCommand(input, slashId)`):
  → `client.sendCommand(input, slashId, webSearchEnabled)`.

#### Version bump

- `src-tauri/Cargo.toml`: `0.6.1` → `0.7.0`.

#### Verification outputs

```
cargo test -p ui
  running 18 tests
  test result: ok. 18 passed; 0 failed

cargo clippy -p ui --all-targets -- -D warnings
  Finished dev profile — no warnings

node --check config-dialog.js / app.js / ws-client.js → OK (no errors)

node --test crates/ui/frontend/line-editor.test.mjs
  pass 24, fail 0
```

---

## [0.6.1] — 2026-06-21

### Changed
- Finestre Markdown (`frontend/window.html`): sfondo `--bg` da `rgba(15,15,22,0.92)`
  a `rgba(15,15,22,0.87)` → **+5% di trasparenza** delle finestre.

---

## [0.6.0] — 2026-06-21

### Added — Echo del comando su Invio

- `frontend/renderer.js`: nuovo metodo `echoCommand(text)` — alla pressione di Invio
  il prompt digitato viene mostrato subito nel pannello principale come blocco
  `.cmd-echo` (`› <testo>`), così l'utente vede cosa ha scritto e che è stato
  ricevuto, anche prima che arrivi la risposta (l'input viene azzerato su Invio).
- `frontend/app.js`: chiama `renderer.echoCommand(input)` nell'handler Invio per ogni
  input non vuoto (comandi normali e slash).
- `frontend/index.html`: stile `.cmd-echo` (colore prompt, soft-wrap).

### Added — Activity indicator with 3 selectable styles (from /config)

#### Rust — `src-tauri/src/config.rs`

- New enum `ActivityIndicator` (serde `snake_case`): `Title` (default), `Status`, `Prompt`.
- New field `activity_indicator: ActivityIndicator` on `Config`, annotated with
  `#[serde(default)]` so existing `config.json` files without the field continue to load
  correctly (deserializes to `Title`).
- `impl Default for Config`: `activity_indicator: ActivityIndicator::Title`.
- 2 new unit tests (TDD, RED → GREEN):
  - `default_activity_indicator_is_title` — asserts `Config::default().activity_indicator == Title`.
  - `missing_activity_indicator_field_defaults_to_title` — writes a JSON config with `action_key: "F7"`
    but no `activity_indicator`; asserts parse succeeds (action_key preserved as "F7") and field
    defaults to `Title`.  This test would fail without `#[serde(default)]` because serde would
    error, `load_from` would fall back to `Config::default()`, and `action_key` would be "F2".
- `round_trip_custom_config` updated to include `activity_indicator: ActivityIndicator::Status`
  so the round-trip covers the new field.

#### Frontend — `frontend/index.html`

- Header DOM restructured: `<span class="label">` now contains an inline
  `<span id="activity-title" class="activity-spinner">` for the title spinner.
  `<span id="status-badge">` is wrapped inside a new `<span class="header-right">` flex
  container alongside a sibling `<span id="activity-status" class="activity-spinner activity-status">`.
- New CSS rules:
  - `.header-right` — flex container aligning status-area siblings.
  - `.activity-spinner` — shared spinner colour and font-size.
  - `#activity-title` — margin-left: 8px (space after "Lare Terminal").
  - `.activity-status` — margin-right: 10px (gap before the status badge).
  - `.prompt.busy` — CSS animation `prompt-pulse` (fade 1→0.25→1 over 1s ease-in-out).
  - `@keyframes prompt-pulse`.

#### Frontend — `frontend/config-dialog.js`

- Default `current` object in `open()` includes `activity_indicator: "title"`.
- `_buildPanel` builds and captures `activitySelect` via new `_buildActivitySelect` call.
- `newCfg` in the Save handler includes `activity_indicator: activitySelect.value`.
- New method `_buildActivitySelect(parent, current)`: mirror of `_buildPositionSelect`,
  with options `title` / `status` / `prompt` and label "Indicatore".

#### Frontend — `frontend/app.js`

Activity indicator controller (new module-level section):
- DOM references: `activityTitleEl`, `activityStatusEl`, `promptEl`.
- `SPINNER_FRAMES` (braille, 10 frames) + `SPINNER_INTERVAL_MS = 90`.
- `inFlight: Set<string>` — ids of commands sent but not yet done/error.
- `activityStyle: string` — current style, default "title".
- `_spinnerTimer: number|null` — guard ensuring only one `setInterval` runs at a time.
- `setActivityStyle(style)` — clears UI state, restarts activity if in-flight. Called from
  `bootstrap()` (on load) and from the `onSaved` callback (instant style switch).
- `commandStarted(id)` — `inFlight.add(id)` then `startActivity()`.
- `commandEnded(id)` — `inFlight.delete(id)`, calls `stopActivity()` when set empties.
- `startActivity()` — idempotent (no-op if timer already running); sets CSS class for
  "prompt" or starts braille interval for "title"/"status".
- `stopActivity()` — clears interval and removes all UI artefacts.
- `_tickSpinner()` — advances frame, writes `frame` (title) or `"${frame} elaboro…"` (status).
- `_clearActivityUI()` — removes `.busy` class and empties spinner text elements.

Wiring:
- `handleServerMsg` `done` case: calls `commandEnded(msg.id)` after `renderer.markDone`.
- `handleServerMsg` `error` case: calls `commandEnded(msg.id)` after `renderer.markError`.
- Enter path: generates `cmdId = uuidv4()`, calls `commandStarted(cmdId)`, passes same
  id to `client.sendCommand`. `commandStarted` is placed AFTER the not-connected guard
  (prevents spinner starting on a connection-error path).
- `handleSlashCommand` backend path: same pattern with `slashId`. The `/config` branch
  exits before the send path — no `commandStarted` for a UI-local command.
- `onStatus` callback: if status is `"disconnected"` or `"error"`, `inFlight.clear()` +
  `stopActivity()` to prevent a permanently stuck spinner on connection loss.
- `bootstrap()`: after `get_config`, calls `setActivityStyle(cfg.activity_indicator)`.
- `onSaved` callback: calls `setActivityStyle(savedCfg.activity_indicator)` in addition
  to `applyAppearance` for immediate style change without restart.

#### Supervisor note — `/show` and stuck spinners

`/show <markdown>` commands produce an `open_window` ServerMsg; the orchestrator may not
emit a subsequent `done`/`error` for those commands.  If this is the case, the spinner
will run until the connection drops (which triggers `stopActivity`).  This cannot be fixed
in the UI layer without touching the orchestrator.  Flag for the supervisor to verify during
e2e testing: type `/show # Test`, confirm whether the spinner stops after the window opens
or only on disconnect.

#### Version bump

- `src-tauri/Cargo.toml`: `0.5.2` → `0.6.0`.

#### Verification outputs

```
cargo test -p ui
  running 16 tests
  test result: ok. 16 passed; 0 failed

cargo build -p ui
  Finished dev profile [unoptimized + debuginfo] — no errors

cargo clippy -p ui -- -D warnings
  Finished dev profile — no warnings

node --test crates/ui/frontend/line-editor.test.mjs
  pass 24, fail 0
```

---

## [0.5.2] — 2026-06-21

### Fixed — BUG-002 (input wrap) + BUG-003 (Markdown window z-order / F2) + Enhanced — auto-height Markdown window

#### Auto-height Markdown window

**Enhancement:** the Markdown window now resizes its height to fit the content instead of
always opening at a fixed 600 px.

**Files changed:**

- **`src-tauri/src/main.rs`** — new Tauri command `resize_self(width: f64, height: f64)`:
  calls `webview.set_size(LogicalSize::new(width, height))` on the calling window.
  Registered in `invoke_handler![]`.  Mirrors the `close_self` pattern.

- **`frontend/window.js`** — after `renderMarkdown(data.content)` in `bootstrap()`,
  schedules a `requestAnimationFrame` callback that:
  1. Temporarily sets `contentEl.style.flex = "0 0 auto"` to collapse flex growth,
     reads `contentEl.scrollHeight` (true content height, no longer stretched by
     `flex:1`), then restores the previous flex value.  Without this trick `scrollHeight`
     is always ≥ `clientHeight` (spec: `max(padding-box, content)`), so for short content
     it would return ~567 px (the stretched height) instead of the actual content height.
     `body` / `documentElement` scrollHeight cannot be used either — they are clamped by
     `overflow:hidden`.  Adds `titlebarEl.offsetHeight + 4` (4 px = 1.5 px top/bottom
     body border + 1 px anti-scrollbar buffer).
  2. Clamps to `[MIN=120, MAX=round(screen.availHeight * 0.85)]`.
  3. Calls `invokeCmd("resize_self", { width: window.outerWidth || 800, height: clampedHeight })`
     with `.catch(() => {})` (best-effort; ignored outside Tauri IPC context).

#### BUG-002 — Input long text now wraps (soft-wrap)

**Symptom:** typing a long prompt caused the text to overflow horizontally beyond the
panel boundary and be clipped.

**Files changed:**

- **`frontend/index.html`** — `#typed-text`:
  - `white-space: pre` → `white-space: pre-wrap` (enables soft-wrap at word / char boundaries).
  - Added `overflow-wrap: anywhere` (forces wrap even for unbreakable sequences).
  - `flex: 0 0 auto` → `flex: 1 1 auto; min-width: 0` (lets the span grow into available
    width and shrink below its content size, which is the flex prerequisite for wrapping).
  - `.input-row { align-items: center }` → `align-items: flex-start` so the `›` prompt
    stays vertically aligned to the first line when the input wraps across multiple rows.

- **`frontend/app.js`** — `renderEditor()`:
  - `spanBefore.style.whiteSpace = "pre"` → `"pre-wrap"`.
  - `spanAfter.style.whiteSpace = "pre"` → `"pre-wrap"`.
  - Updated doc comment (was: "white-space: pre is set on all spans").

#### BUG-003 — Markdown window above main + F2 hides both

**Symptom:** Markdown windows opened behind the main overlay; F2 hid only the main window,
leaving orphaned Markdown windows on screen.

**Files changed:**

- **`src-tauri/src/main.rs`** — `open_markdown_window` builder chain:
  - Added `.always_on_top(true)` so the Markdown window matches the main overlay's z-level.
  - Added `.focused(true)` so the window comes to the foreground immediately on open.

- **`src-tauri/src/main.rs`** — global-shortcut handler (F2 / configured key):
  - **Hide branch**: now iterates `app.webview_windows()` and hides every window whose
    label is `"main"` or starts with `"md-"`.  Previously only `"main"` was hidden.
  - **Show branch**: shows all `md-*` windows first (via `w.show()`), then calls
    `window::show_and_focus(app, &position)` for the main window (unchanged behavior
    for the main window itself).
  - `Manager` trait already imported — no new `use` added.

#### Version bump

- `src-tauri/Cargo.toml`: `version = "0.5.1"` → `"0.5.2"`.

#### Verification outputs

```
cargo build -p ui
  Compiling ui v0.5.2 ...
  Finished `dev` profile [unoptimized + debuginfo] — no errors

cargo clippy -p ui -- -D warnings
  Compiling ui v0.5.2 ...
  Finished `dev` profile — no warnings

cargo test -p ui
  running 14 tests
  test result: ok. 14 passed; 0 failed  (14 config unit tests, headless)

node --test crates/ui/frontend/line-editor.test.mjs
  pass 24, fail 0  (line-editor logic untouched — confirmed no regression)
```

#### Supervisor verification required (GUI, not automatable)

- **BUG-002**: type a command longer than ~120 characters; text should wrap to the next
  line inside the panel; the `›` prompt should stay on the first line.
- **BUG-003**: open a Markdown window (`/show # Test`); it should appear above the main
  overlay.  Press F2 twice (hide → show): both windows should hide and reappear together.

---

## [0.5.1] — 2026-06-20

### Changed — Transparent, chromeless Markdown windows (Superficie 3 visual identity)

Markdown output windows (Superficie 3) now share the visual identity of the main overlay
panel: transparent background with the same `rgba(15, 15, 22, 0.92)` alpha, blue border
`rgba(80, 160, 240, 0.55)`, and `border-radius: 10px`.  The window is chromeless
(no native title bar/close button); a custom drag region and close button are provided.

#### Rust (`src-tauri/src/main.rs`)

- **`open_markdown_window`** builder: `.decorations(false)` + `.transparent(true)`.
  On Windows, decorations must be off for WebView2 transparency to take effect.
  `.resizable(true)` is kept; native resize handles are absent on a decoration-free
  window (known limitation, out of scope).

- **`close_self` (new Tauri command):** `webview.close()` on the calling `WebviewWindow`.
  Mirrors the `take_window_content` pattern (Tauri injects the calling window automatically).
  Registered in `generate_handler![]`.

#### Frontend (`frontend/window.html`)

- `html { background: transparent }` — OS sees through the rounded-corner gap.
- `body` is now the panel: `background: var(--bg)`, `border: 1.5px solid var(--border)`,
  `border-radius: 10px`, `overflow: hidden` (clips children to rounded corners).
- `--bg` updated from `rgba(15,15,22,0.97)` to `rgba(15,15,22,0.92)` (same as main overlay).
- `--border` updated from `rgba(80,160,240,0.45)` to `rgba(80,160,240,0.55)` (same as main overlay).
- `#titlebar` carries `data-tauri-drag-region` for OS-level window dragging.
- `#titlebar` split into `#titlebar-label` (text) + `#close-btn` (× button).
  `#close-btn` has `-webkit-app-region: no-drag` so its click is never swallowed by the
  drag handler.

#### Frontend (`frontend/window.js`)

- `titlebarEl` reference renamed to `titlebarLabelEl` (now targets `#titlebar-label`).
- `closeBtnEl` wired: click → `invokeCmd("close_self")`.
- `document` keydown listener: `Escape` → `close_self` (with `preventDefault()`).
- `closeWindow()` helper: awaits `close_self` IPC, ignores errors (OS handles edge cases).

#### Capabilities (`src-tauri/capabilities/markdown-window.json`)

- `"windows"` tightened from `["*"]` to `["md-*"]` (explicit scope; main window unaffected).
- Added `"core:window:allow-start-dragging"` — gates the Tauri `start_dragging` command
  triggered by `data-tauri-drag-region`.  Permission identifier verified against
  `gen/schemas/desktop-schema.json`.

#### Tauri v2 APIs used

| API / Permission | Where | Notes |
|---|---|---|
| `WebviewWindowBuilder::transparent(true)` | `main.rs` | Runtime transparent window |
| `WebviewWindowBuilder::decorations(false)` | `main.rs` | Required for transparency on Windows |
| `tauri::WebviewWindow::close()` | `main.rs close_self` | Programmatic window close |
| `data-tauri-drag-region` HTML attribute | `window.html #titlebar` | OS-level drag |
| `core:window:allow-start-dragging` | `markdown-window.json` | ACL for drag region |
| `close_self` custom command | covered by `core:default` | No extra permission needed |

#### Verification outputs

```
cargo check -p ui          → Finished dev profile (no errors)
cargo clippy -p ui -- -D warnings → Finished dev profile (no warnings)
cargo fmt --all -- --check → (no output = no diffs)
node --check crates/ui/frontend/window.js → (no output = OK)
```

`cargo build -p ui` produced "Accesso negato" (os error 5) because the previous
`ui.exe` was locked by a running instance of the app.  `cargo check` (which does
not attempt to replace the binary) compiled cleanly with no errors.

#### Known limitations / supervisor notes

- **Transparency visual confirmation:** the main overlay is transparent via static
  `tauri.conf.json`; the Markdown window is transparent via the runtime
  `WebviewWindowBuilder`.  Both use the same Tauri/WebView2 backend — compilation
  is clean — but the actual visual result needs interactive verification by the
  supervisor (no automated test can confirm alpha blending).

- **Native resize handles absent:** `decorations(false)` removes OS-provided resize
  borders.  `resizable(true)` is kept (API-level resizable) but the user has no
  drag-to-resize edge.  Custom resize handles are out of scope for this change.

---

## [0.5.0] — 2026-06-20

### Added — Cursor-editing with block caret (Superficie 4)

#### Pure module: `frontend/line-editor.js`

- **`insert(state, str)`** — inserts `str` at caret, advances caret by `str.length`.
- **`backspace(state)`** — deletes char before caret; no-op if caret === 0.
- **`deleteForward(state)`** — deletes char at caret; no-op if caret === text.length.
- **`left(state)` / `right(state)`** — moves caret ±1 (clamped to 0..text.length).
- **`home(state)` / `end(state)`** — moves caret to 0 / text.length.
- **`clear(state)`** — returns `{text:"", caret:0}`.
- All functions are **pure** (return new state, never mutate input).
- Zero dependencies (no DOM, no async).

#### TDD cycle — RED → GREEN

```
RED:  node --test crates/ui/frontend/line-editor.test.mjs
      ✖ 13 tests (assertion failures against stub exports)
      ✔ 11 tests (no-op cases coincidentally matched stubs)

GREEN: node --test crates/ui/frontend/line-editor.test.mjs
      ✔ 24/24 tests pass
```

Full test file: `frontend/line-editor.test.mjs` (24 cases covering insert at
start/middle/end, multi-char paste, backspace/delete at boundaries, left/right
clamp, home/end, clear, and immutability of input state).

#### Clipboard (Tauri v2 `tauri-plugin-clipboard-manager`)

- **`Cargo.toml`**: `tauri-plugin-clipboard-manager = "2"` added.
- **`main.rs`**: `.plugin(tauri_plugin_clipboard_manager::init())` registered.
- **`capabilities/default.json`**: two new permissions:
  - `clipboard-manager:allow-read-text`
  - `clipboard-manager:allow-write-text`
- **JS**: accessed via `window.__TAURI__.clipboardManager` (available because
  `withGlobalTauri: true` in `tauri.conf.json`), matching the existing IPC
  pattern (`window.__TAURI__.core.invoke`). Wrapped defensively — clipboard
  errors are `console.warn`'d, never crash the terminal.

#### Keyboard wiring in `frontend/app.js`

- **Replaced** the old append/slice text model with `lineEditor` state
  `{text, caret}` managed by the pure module.
- **New keys handled** (all with `e.preventDefault()`):
  - `ArrowLeft` / `ArrowRight` → `lineEditor.left` / `right`
  - `Home` / `End` → `lineEditor.home` / `end`
  - `Delete` → `lineEditor.deleteForward`
  - `Ctrl+C` → `clipboardWrite(editorState.text)`
  - `Ctrl+V` → `clipboardRead().then(text => insert at caret + re-render)`
- **Preserved** (behavior unchanged):
  - `Backspace` → `lineEditor.backspace` (was `slice(-1)`)
  - `Enter` → sends `editorState.text.trim()`, then `lineEditor.clear()`
  - `Esc` → hides overlay; text+caret preserved (no clear)
  - `Ctrl+L` → `lineEditor.clear()` + `renderer.clear()`
  - Printable chars → `lineEditor.insert(editorState, e.key)`
  - Slash dispatch → unchanged
  - Focus management → unchanged

#### Block cursor render in `frontend/app.js` + `frontend/index.html`

- `renderEditor()` function renders the input row as three `createElement` spans
  inside `#typed-text` (no `innerHTML`, so `<`, `&`, etc. are safe):
  1. **before** — text to the left of the caret (`white-space: pre`).
  2. **cursor block** (`.cursor` class, blink animation) — char at caret (inverse
     colours: `background: var(--cursor-color)`, `color: rgba(15,15,22,1)`).
     At end-of-line the block contains a space with `min-width: 0.6em`.
  3. **after** — text to the right of the caret.
- `renderEditor()` called after every state-changing keydown event.
- The original static `<span class="cursor">` sibling in `index.html` is given
  an additional `cursor-static` class and hidden via `display:none` in CSS,
  since the dynamic block is now created inside `#typed-text`.
- Blink animation (`@keyframes blink`) is preserved unchanged on the `.cursor`
  CSS class.

#### `frontend/package.json` (new file)

- `{"type": "module"}` — required so `node --test` treats `line-editor.js` as
  an ES module when imported by `line-editor.test.mjs`. No effect on the Tauri
  build (`frontendDist` is a static directory with no npm build step).

#### Version bump

- `Cargo.toml`: `version = "0.5.0"`.

#### Verification outputs

```
node --test crates/ui/frontend/line-editor.test.mjs
  ✔ 24/24  (GREEN — see TDD cycle above for RED output)

node --check app.js           → OK
node --check line-editor.js   → OK

cargo build -p ui             → Finished dev profile
cargo clippy -p ui -- -D warnings → no warnings
cargo fmt --all -- --check    → (no output = no diffs)
cargo test                    → 122 passed (protocol 25, orchestrator 56+4, mcp-server 37)
```

#### Interactive verification (supervisor)

- Type characters: they appear at the caret (block moves right).
- Press ← / → to move caret left/right; block shifts, characters remain.
- Press Home / End: caret jumps to start/end.
- Backspace / Delete: correct adjacent character removed, caret moves (or stays for Delete).
- Type in the middle of a word: text inserts at caret, not appended.
- Ctrl+C then Ctrl+V: pastes the full line at the caret position.
- Enter: sends the command, clears input.
- Esc: hides overlay; text+caret are preserved when re-shown.
- Ctrl+L: clears both input and output.

#### Decisions for supervisor

| Item | Decision | Note |
|------|----------|------|
| RED demo | Stub exports (functions return `state` unchanged) | Produces per-assertion RED output, not import errors |
| Clipboard API path | `window.__TAURI__.clipboardManager` | Confirmed `withGlobalTauri:true` in tauri.conf.json; matches existing IPC pattern |
| Clipboard errors | `console.warn`, never throw | Satisfies "gestisci errori clipboard senza crash" |
| Static `.cursor` span | Hidden via `cursor-static` CSS class | Preserved in HTML for validity; superseded by dynamic render |
| Selezione/highlight | Out of scope | Not implemented per spec |

---

## [0.4.0] — 2026-06-20

### Added — Superficie 3: Markdown output windows (ADR-013)

#### Rust (`src-tauri/src/main.rs`)

- **`WindowContentStore`** new managed state:
  `Mutex<HashMap<String, (String, String)>>` mapping window label → (title, content).
  Entries inserted before window creation; removed on first read (one-shot, no leak).

- **`open_markdown_window(title, content)` (async Tauri command):**
  - Generates a unique label `md-<timestamp_ms>-<counter>` (no extra crate needed;
    collision-resistant for a local overlay).
  - Inserts (title, content) into `WindowContentStore` with lock released BEFORE
    calling `build()` — avoids holding mutex across async window creation.
  - Creates a `WebviewWindowBuilder::new(app, label, WebviewUrl::App("window.html?label=<label>"))`,
    decorated, 800×600, resizable.
  - **Declared `async`** because `WebviewWindowBuilder::build()` deadlocks in
    synchronous Tauri commands on Windows (Tauri v2 documented limitation).

- **`take_window_content(label)` (sync Tauri command):**
  - Looks up and removes the entry for `label` from `WindowContentStore`.
  - Returns `Some({ "title": ..., "content": ... })` or `None` if already consumed.
  - Called by `window.js` at load time; removal prevents memory leaks.

- Both commands registered in `invoke_handler![]`.

- `WindowContentStore` initialized in `setup()` via `app.manage(...)`.

#### Frontend

- **`frontend/vendor/marked.min.js`** — `marked` v15.0.12 (pinned, vendored locally).
- **`frontend/vendor/purify.min.js`** — DOMPurify v3.2.5 (pinned, vendored locally).
  Both downloaded from jsDelivr CDN at build time; referenced locally at runtime
  (zero CDN dependency at runtime → offline-safe).

- **`frontend/window.html`** — Markdown output window:
  - Dark background `rgba(15,15,22,0.97)`, Consolas font, project colour palette.
  - Loads `vendor/marked.min.js`, `vendor/purify.min.js` as plain scripts (globals),
    then `window.js` as ES module.
  - Normal window (decorations=true): has title bar and close button.

- **`frontend/window.js`** — Markdown window logic (SRP):
  - Reads label from `window.location.search` (`?label=<label>`).
  - Calls `take_window_content(label)` via Tauri IPC.
  - Render pipeline: `rawMd → marked.parse() → DOMPurify.sanitize() → innerHTML`.
  - **Security:** no `innerHTML` assignment without `DOMPurify.sanitize()` first.
    `<script>` tags and event handlers are stripped unconditionally.
  - Displays title in `#titlebar` and `document.title`.
  - Error states: loading / error / missing-label handled gracefully.

- **`frontend/app.js`** — added `case "open_window":` to `handleServerMsg`:
  - Calls `openMarkdownWindow(msg.title, msg.content)` async wrapper.
  - `invokeCmd("open_markdown_window", { title, content })` → Rust command.

#### Capabilities

- **`capabilities/markdown-window.json`** — new capability file:
  - `"windows": ["*"]` — covers dynamically-labelled Markdown windows (`md-<ts>-<n>`).
  - `"permissions": ["core:default"]` — grants Tauri IPC for `take_window_content`.
  - Global-shortcut permissions intentionally excluded (only needed by `main`).

#### Design notes

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Content passing | Managed state (`HashMap`) | URL query params have size limits; managed state is atomic and leak-free |
| Label in URL | `?label=<label>` query param | Simpler than JS window-label API; robust across Tauri versions |
| Window style | Decorated, resizable | Closeable; multiple windows can coexist |
| Sanitization | DOMPurify v3.2.5 | Industry standard; allows `marked` output while stripping dangerous content |
| Markdown library | marked v15.0.12 | Widely used, GFM-compatible; paired with DOMPurify per spec |

#### Security note for supervisor

The spec warns that `/show <script>alert(1)</script>` must NOT execute.
`DOMPurify.sanitize()` unconditionally removes `<script>` tags and all
`on*` event handlers — the attack is blocked at the sanitize step, before
any DOM insertion. Interactive verification: type `/show <script>alert(1)</script>`
in the terminal; a window should open with no alert and the script tag absent
from `#content`'s innerHTML (verifiable via DevTools).

#### Capability note for supervisor

The task wording says "permessi per creare webview window" — however, in Tauri v2
the ACL only gates JS-initiated API calls. Since Rust creates the window (not JS),
no `core:webview:allow-create-webview-window` permission is required or added.
The only permission the Markdown window needs is `core:default` (for Tauri IPC),
which is granted by `markdown-window.json`. This deviation from the task wording
is flagged here.

---

## [0.3.0] — 2026-06-20

### Changed — Slash command routing (ADR-012)

- **`app.js` `handleSlashCommand` refactored** (Slice B wiring close-out):
  - **`/config` remains UI-local** (opens the config dialog without a WS round-trip).
  - **Every other `/…` is now forwarded to the orchestrator** as a normal `Command`
    via `client.sendCommand(input, uuidv4())`.
  - The "sconosciuto" inline error is **removed**: the backend now owns the response
    for unknown slash commands (it returns `Error{RoutingError}`).
  - The not-connected guard is duplicated in `handleSlashCommand` so the user
    gets immediate feedback even before the WS is established.
- Updated JSDoc comment to document the UI-local vs backend slash split (ADR-012).

---

## [0.2.0] — 2026-06-20

### Changed (refinement post-review)
- `/config` colore: griglia di **swatch** curati + **color picker nativo** (`<input type="color">`) per il custom — al posto del campo hex testuale.
- `/config` font: **dropdown dei font monospace installati**, rilevati via `document.fonts.check()` su una lista curata di candidati (no dipendenze Rust).

### Added

#### Core config (Rust, fully TDD)
- `src/lib.rs` + `src/config.rs`: new `ui_lib` library target exposing the `config` module.
  Enables `cargo test -p ui` to run config tests without a Tauri runtime.
- `Config` struct with serde `Deserialize`/`Serialize`:
  - `action_key: String` — default `"F2"`.
  - `cursor_color: String` — default `"#FFFFFF"`.
  - `cursor_font: String` — default `"Consolas"`.
  - `cursor_size: u32` — default `11`.
  - `position: Position` — default `Center`.
- `Position` enum (serde snake_case): `Center` / `BottomCenter` / `NearMouse`.
- `load_from(&Path) -> Config`: missing file → default (silent); corrupt JSON → default + stderr warning.
- `save_to(&Config, &Path) -> Result<(),String>`: pretty-printed JSON; creates parent dirs.
- `validate_action_key(&str) -> Result<(),String>`: delegates to `Shortcut::from_str` — same parser as the plugin.
- 14 unit tests covering all required cases (defaults, round-trip, missing file, corrupt JSON, valid/invalid accelerators).
  RED→GREEN reconstructed post-hoc: 9 tests fail with stub bodies; all 14 pass with real implementation.
  See IMPLEMENTATION.md §TDD RED→GREEN cycle for full output.
- `tempfile = "3"` as dev-dependency.

#### Tauri commands
- `get_config() -> Config`: reads from `{app_config_dir}/config.json` (creates dir if absent).
- `set_config(new_cfg: Config) -> Result<(),String>`: validate → persist → update managed state → re-register hotkey.
  Validation fails → nothing changes (all-or-nothing).
- `ConfigState(Mutex<Config>)` in Tauri managed state; populated on startup from loaded config.

#### Hot-applied hotkey
- Startup: loads config and registers `action_key` via `apply_hotkey()` (no longer hard-coded F2).
- `set_config`: calls `unregister_all()` then `register(new_shortcut)` for atomic hotkey swap.
- Handler: identity check (`if shortcut != &f2`) removed; any press of the sole registered shortcut triggers toggle.

#### Position-driven window placement (`window.rs`)
- `show_and_focus` now accepts `&Position`.
- `Center`: original behavior (center of cursor's monitor).
- `BottomCenter`: horizontally centered, 60 px above bottom edge.
- `NearMouse`: near cursor position, clamped to stay within monitor bounds.

#### Capabilities
- Added `global-shortcut:allow-unregister-all` to `capabilities/default.json`.

#### Frontend
- `config-dialog.js`: in-overlay `/config` dialog (new module, SRP).
  - Action-key capture: click badge → press combo → accelerator string auto-built.
  - Fields: colour, font, size (number 6–72), position (`<select>`).
  - Save: `invoke("set_config", ...)`, on success calls `onSaved(cfg)` then closes.
  - Error display on Rust-side validation failure (e.g. invalid accelerator).
  - Cancel / Esc: closes without saving.
  - No `innerHTML` with external data; DOM built with `createElement`/`textContent`/`.value`.
- `app.js` updated:
  - `handleSlashCommand`: `/config` → `configDialog.open()`; other `/x` → inline error (no WS send).
  - `applyAppearance(cfg)`: sets `--cursor-color`, `--cursor-font`, `--cursor-size` on `:root`.
  - `bootstrap()`: calls `get_config` → `applyAppearance` on startup before WS init.
  - Focus guard: `grabFocus()` skips when config dialog is open; click inside dialog doesn't steal focus.
- `index.html` updated:
  - `:root` CSS variables `--cursor-color`, `--cursor-font`, `--cursor-size` (defaults match Rust defaults).
  - `body` / `#typed-text` / `.cursor` use `var(--cursor-color/font/size)`.
  - Dialog CSS: `.config-dialog-overlay`, `.config-dialog-box`, `.config-field-row`, buttons, key badge, capture input — all in-theme.
  - Hint row updated: "F2 to toggle" → "/config to configure".
  - `overlay-panel` has `position: relative` for the absolute-positioned dialog overlay.

### Changed
- `crates/ui/src-tauri/Cargo.toml`: version bumped to `0.2.0`; added `[lib]` target; added `[dev-dependencies] tempfile`.
- `window.rs`: `show_and_focus` now takes `&Position` parameter; split into `apply_position` + 3 sub-functions; added `BottomCenter` and `NearMouse` logic.
- `main.rs`: removed hard-coded F2; added `ConfigState` managed state; added `get_config`/`set_config` commands; startup now loads config and registers hotkey from config.

---

## [0.1.0] — 2026-06-20

### Added
- Tauri v2 overlay window: chromeless, transparent, always-on-top, skip-taskbar, starts hidden.
- F2 global hotkey (tauri-plugin-global-shortcut): toggles show/hide.
- On show: centers on the monitor under the cursor (falls back to primary monitor), then steals OS keyboard focus using the validated spike sequence (show → set_always_on_top → set_focus).
- Esc key: hides the overlay; typed text is preserved.
- Ctrl+L: explicit clear of both the input text and the output area.
- WebSocket client (`ws-client.js`): connects to `ws://127.0.0.1:7331` with exponential-backoff retry (1 s → 8 s cap). Sends `Hello{token}` handshake, then `Command{id,input,input_mode,command_type,cwd}` messages on Enter.
- Token sourced from the `LARE_TOKEN` environment variable via a Rust command (`get_lare_token`); never hard-coded.
- Output renderer (`renderer.js`): shows streaming `Chunk` content, `Done` (with exit code badge), and `Error` messages in a scrollable output area. Max 500 blocks before oldest are pruned.
- Connection status badge (connecting / connected / offline / error).
- Slash command placeholder: inputs starting with `/` are intercepted client-side and display a "coming in Slice B" message.
- Appearance: white `#FFFFFF` text, Consolas 11 px, wide panel (1060 px), single border, no outer glow, blinking block cursor.
- `ws-client.js` / `renderer.js` / `app.js` separation (SRP).
- `window.rs` module for show/center/focus logic (SRP).

### Known limitations (Slice A)
- `cwd` is always `null`: each command opens a fresh shell, so `cd` does not persist between commands.
- `/config` dialog and slash-command routing are Slice B.
- Hotkey is hard-coded to F2; configurable hotkey is Slice B (ADR-008).
- No Tauri bundling (`bundle.active = false`); packaging is a later phase.
- CSP not set: Tauri v2 removed the top-level `security.csp` key; `app.security.csp` was omitted to avoid breaking Tauri IPC without interactive verification. Slice B should add it.
