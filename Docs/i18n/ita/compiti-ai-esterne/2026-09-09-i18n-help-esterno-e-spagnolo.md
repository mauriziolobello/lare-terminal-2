# Compito per AI esterna — /help su file esterni + terza lingua (spagnolo)

> **Prima cosa**: leggi per intero `Docs/i18n/ita/BRIEFING-AI-ESTERNE.md` alla radice del
> repository, poi questo file per intero, PRIMA di scrivere codice.

## Contesto — perché questo compito esiste

Il fix precedente (`Docs/i18n/ita/compiti-ai-esterne/2026-09-09-i18n-fix-help.md`, report
`Docs/i18n/ita/reports/2026-09-09-gemini-i18n-fix-help.md`) ha reso `/help` sensibile alla lingua
usando due costanti Rust `HELP_MARKDOWN_IT`/`HELP_MARKDOWN_EN` in `core.rs` — funziona, verificato
dal supervisore (build/test/clippy verdi). Ma Maurizio ha fatto notare un problema di
manutenibilità: quel blocco è 29 righe di prosa, non una frase — con ogni nuova lingua `core.rs`
crescerebbe di altre 29 righe mescolate alla logica, ed è inconsistente con l'architettura già
scelta per i testi della UI (`it.json`/`en.json` sono file ESTERNI sotto `Configuration/`, non
costanti Rust). **Questo compito arriva proprio ora che serve aggiungere la terza lingua**
(spagnolo) — il momento giusto per fare la migrazione una volta sola, prima che si accumuli altro.

Questo compito fa DUE cose, in quest'ordine — **committa e fermati dopo la Parte A**, verifica che
tutto funzioni ancora come prima per italiano/inglese, POI procedi con la Parte B:

- **Parte A**: migra `/help` da costanti Rust a file esterni (`Configuration/help/<lang>.md`) —
  refactor puro, zero nuovo comportamento visibile, italiano e inglese devono continuare a
  funzionare esattamente come oggi.
- **Parte B**: aggiungi lo spagnolo (`es`) dappertutto — UI (`config-dialog.js`, `es.json`),
  direttiva AI (`agent.rs`), e il nuovo `help/es.md` (reso possibile dalla Parte A).

## Parte A — `/help` da costanti Rust a file esterni

### A.1 — Nuovo modulo `crates/orchestrator/src/help.rs`

Stesso pattern infallibile di `crates/ui/src-tauri/src/i18n.rs` (leggilo per intero prima di
scrivere questo — è il riferimento letterale, anche se qui il file è `.md` non `.json` e non
serve un dizionario chiave→valore, solo un blob di testo):

```rust
//! Corpo di /help caricato da file esterni (Configuration/help/<lang>.md), non
//! da costanti Rust — evita che core.rs cresca ad ogni nuova lingua aggiunta.

use std::path::{Path, PathBuf};

/// Cartella dei file di help: `<config_dir>/help`.
pub fn help_dir_path(config_dir: &Path) -> PathBuf {
    config_dir.join("help")
}

/// Corpo Markdown per la lingua richiesta.
/// Fallback: `<lang>.md` assente/illeggibile → `it.md` → stringa minima di
/// sicurezza (mai un errore che rompe /help).
pub fn load_help_body(help_dir: &Path, lang: &str) -> String {
    std::fs::read_to_string(help_dir.join(format!("{lang}.md")))
        .or_else(|_| std::fs::read_to_string(help_dir.join("it.md")))
        .unwrap_or_else(|_| "Aiuto non disponibile: file mancante.".to_string())
}
```

Registra il modulo (`pub mod help;`) accanto agli altri moduli dello stesso livello — cerca
`pub mod notes;` in `lib.rs` per il punto esatto.

Test (TDD, stesso stile di `i18n.rs::tests`, con `tempfile::tempdir()` — MAI un path letterale
finto, vedi `BRIEFING-AI-ESTERNE.md` sezione 3): file assente → fallback su `it.md`; `it.md`
assente anche lui → stringa minima; entrambi presenti e diversi → ritorna quello della lingua
richiesta, non quello di fallback.

### A.2 — `Test Run/Configuration/help/it.md` ed `en.md`

`it.md` = contenuto ESATTO di `HELP_MARKDOWN_IT` oggi in `core.rs` (copia letterale, incluso
l'header `# Lare — Comandi` — fa parte del corpo Markdown mostrato nella finestra). `en.md` =
contenuto ESATTO di `HELP_MARKDOWN_EN` oggi in `core.rs` (già tradotto dal compito precedente,
copialo così com'è, non ritradurre). Committali direttamente — verificato che
`deploy_test_run.ps1` non tocca `Configuration/`, quindi vanno committati a mano, stesso
trattamento di `Test Run/Configuration/i18n/it.json`/`en.json`.

Poi **elimina** `HELP_MARKDOWN_IT` ed `HELP_MARKDOWN_EN` da `core.rs` — non lasciarli come residuo
morto. Il titolo della finestra RESTA una costante Rust breve (non il problema di ingombro):

```rust
const HELP_TITLE_IT: &str = "Lare \u{2014} Comandi";
const HELP_TITLE_EN: &str = "Lare \u{2014} Commands";
```

### A.3 — Propagazione `config_dir` fino a `handle_slash`

Oggi (dopo il fix precedente) `handle_slash` riceve già `lang: Option<&str>`, propagato da
`handle_command` che ha già `lang: Option<String>` fra i parametri. Manca **`config_dir`**, che
serve per costruire il path di `help_dir_path`. Stesso identico trattamento meccanico già fatto
per `lang` (Parte 3 del programma i18n originale) — aggiungi `config_dir: &Path` come nuovo
parametro:

- `handle_slash(id, input, tools, cwd, lang, config_dir)` — firma + il branch `"help"`:
  ```rust
  "help" => {
      let (title, lang_code) = match lang {
          Some("en") => (HELP_TITLE_EN, "en"),
          _ => (HELP_TITLE_IT, "it"),
      };
      let help_dir = crate::help::help_dir_path(config_dir);
      vec![
          ServerMsg::OpenWindow {
              title: title.to_string(),
              kind: WindowKind::Help,
              content: crate::help::load_help_body(&help_dir, lang_code),
          },
          ServerMsg::Done { id: id.to_string(), exit_code: Some(0) },
      ]
  }
  ```
- `handle_command`: aggiungi `config_dir: &Path` ai parametri (accanto a `lang`), passalo al
  chiamare `handle_slash`.
- **Tutti** i chiamanti/test di `handle_command` e di `handle_slash` vanno aggiornati con il nuovo
  argomento — stesso identico esercizio meccanico già fatto per `lang`, stessi punti (cerca ogni
  occorrenza di `handle_command(` e `handle_slash(` in `core.rs`, inclusi i test). Nei test, usa
  `tempfile::tempdir()` con dentro una sotto-cartella `help/` e un `it.md` scritto a mano (stesso
  principio già applicato ai test di `read_language`/`read_web_search_enabled`) — MAI un path
  letterale finto.
- Chiamanti di produzione da aggiornare, entrambi con `config_dir` già disponibile localmente
  (verificato leggendo il codice, non serve procurarselo da altrove):
  - `crates/orchestrator/src/ws.rs`, riga indicativa ~685 (`crate::core::handle_command(...)`) —
    `config_dir` è già una variabile locale lì (`let config_dir = rt.config_dir.clone();`, usata
    poco sopra per `effective_lang`), passa `&config_dir`.
  - `crates/orchestrator/src/shell_turn.rs`, riga indicativa ~222 (`crate::core::handle_command(...)`
    dentro `run_ai_turn`) — usa `&deps.rt.config_dir` (stesso campo già usato lì per
    `read_language`/`read_web_search_enabled`).

### Verifica di fine Parte A (fai questo PRIMA di iniziare la Parte B)

```powershell
cargo build -p orchestrator
cargo test -p orchestrator
cargo clippy -p orchestrator --all-targets
```

Se hai un display: `/config` → English, `/help`, verifica che sia identico a prima del refactor
(stesso testo, ora caricato da `en.md` invece che da una costante). Commit, poi Parte B.

## Parte B — terza lingua: spagnolo (`es`)

Segui **esattamente** `Docs/i18n/ita/compiti-ai-esterne/TEMPLATE-i18n-nuova-lingua.md` (nella sua
forma corrente, che NON menziona ancora l'help — lo fa questo compito) per i 3 punti già noti:

1. `crates/orchestrator/src/agent.rs` — `RESPOND_SPANISH` + `arm` nel match (leggi il template per
   il pattern esatto).
2. `crates/ui/frontend/config-dialog.js` — terza voce dropdown `{ value: "es", label: "Español" }`.
3. `crates/ui/frontend/i18n-parity.test.mjs` — se non è già generico su N file (dovrebbe esserlo
   se il compito precedente l'ha fatto bene, verifica leggendolo), generalizzalo ora.
4. `Test Run/Configuration/i18n/es.json` — traduzione completa di tutte le chiavi oggi in
   `it.json` (fallo tu, non serve chiedere a Maurizio — stesso stile/placeholder `{nome}` delle
   altre due lingue, registro naturale, non un traduttore automatico letterale).

Più il **quarto punto, nuovo da questo compito** (esiste solo dopo la Parte A):

5. `Test Run/Configuration/help/es.md` — traduzione in spagnolo di `it.md` (Parte A.2), stessa
   struttura esatta (stesse sezioni `##`, stesso ordine dei comandi), stesso registro conciso.
   Header tradotto: `# Lare — Comandos` (o l'equivalente naturale che sceglieresti come
   madrelingua — usa il tuo giudizio sulla resa esatta).

## Documentazione e versioni

- `crates/orchestrator`: bump patch — tocca sia la Parte A (refactor) sia la Parte B (spagnolo).
  `CHANGELOG.md` + `IMPLEMENTATION.md`, due voci separate (una per parte) o una sola se preferisci,
  usa il tuo giudizio, basta che sia chiaro cosa è cambiato.
- `crates/ui`: bump patch (Parte B — `config-dialog.js`, `i18n-parity.test.mjs`, eventuale tocco a
  `es.json`... `es.json` è un file dati, non serve bump per quello da solo, il bump è per il codice
  toccato).
- `Docs/i18n/ita/HANDOFF.md`: voce/i in FATTO.

## Verifica finale (dopo la Parte B)

```powershell
cargo build -p orchestrator -p ui
cargo test -p orchestrator -p ui
node --test crates/ui/frontend/*.test.mjs
cargo clippy -p orchestrator -p ui --all-targets
```

Il test di parità deve risultare verde con 3 file (`it`/`en`/`es`).

## Verifica dal vivo (se il tuo ambiente lo consente)

`/config` → Español, `/help` → deve mostrare `help/es.md`; verifica anche che un turno AI in
spagnolo risponda in spagnolo. Se non puoi farlo, scrivilo nel report — il supervisore la fa lui
prima del merge.

## Cosa NON fare

- Non tenere `HELP_MARKDOWN_IT`/`HELP_MARKDOWN_EN` come costanti Rust in nessuna forma dopo la
  Parte A.
- Non spezzettare il corpo dell'help in un dizionario chiave→frase — resta un blob di testo per
  file, non decine di chiavi in `it.json`/`en.json`.
- Non toccare `/show` (contenuto arbitrario dell'utente).
- Non toccare `crates/protocol` — `lang` è già una stringa libera, nessuna modifica serve né per
  la Parte A né per la Parte B.
- Non tradurre "Lare Terminal" né i nomi delle lingue nel dropdown ("Italiano"/"English"/"Español"
  restano fissi, nella propria lingua nativa).

