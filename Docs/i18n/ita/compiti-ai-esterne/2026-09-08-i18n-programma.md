# Compito per AI esterna — i18n del programma (solo Tauri UI + lingua AI sul canale cursore)

> **Prima cosa**: leggi per intero `Docs/i18n/ita/BRIEFING-AI-ESTERNE.md` alla radice del
> repository, poi questo file per intero, PRIMA di scrivere codice.

## Reframe onesto della richiesta originale

Maurizio ha chiesto "i18n sul programma (solo programma, non la documentazione)". Non è un
compito piccolo: tocca la finestra Tauri (frontend + Rust), il protocollo WS, e il system prompt
dell'AI. **MVP di questo compito = tutto ciò che la finestra Tauri mostra all'utente, più la
lingua in cui risponde l'AI sul canale cursore (lare-shell).** Fuori scope, ciascuno un compito a
sé in futuro (motivazione sotto): testo composto dinamicamente dall'orchestratore e spedito come
stringa già pronta (banner di conferma, errori tool, testo di `/help`), l'HTML che ogni plugin si
costruisce da solo, la host C# (`lare-shell`), Telegram, il contenuto del terminale xterm stesso,
le descrizioni dei tool per l'LLM (già in inglese, sono per l'AI non per l'utente), `Docs/`.

Le due lingue di partenza, come richiesto: **italiano** (`it`, default) e **inglese** (`en`).

## Perché il fuori-scope resta fuori (non è pigrizia, è un confine netto)

- **Orchestratore → stringhe già composte**: `local_confirm.rs` (banner di conferma),
  `ai_adapter.rs` (etichette di trasparenza), messaggi d'errore dei tool — il frontend le riceve
  già pronte come testo, non come chiavi. Tradurle richiede un design a parte (probabilmente le
  stesse chiavi/dizionario JSON, ma letto lato Rust orchestrator invece che lato frontend) — non
  improvvisarlo dentro questo compito.
- **`/help`**: verificato che il suo testo vive SIA in `crates/ui/frontend/{host,terminal}.js`
  (help locale del frontend — quello sì in scope, vedi sotto) SIA in
  `crates/orchestrator/src/{core,shell_slash,shell_turn,surface}.rs` (help lato server) — quello
  lato server è fuori scope per lo stesso motivo del punto sopra.
- **Plugin**: ogni plugin manda `ShowWindow{html}` con l'HTML già completo — nessun canale i18n
  esiste oggi per i plugin, va progettato a parte.
- **lare-shell (C#)**: toolchain diversa (.NET/C#, non Rust/JS) — compito distinto, probabilmente
  per un'altra AI più a suo agio con quello stack.

## Architettura (decisa qui, non da inventare durante il compito)

### 1. Dizionari — `Test Run/Configuration/i18n/it.json` e `en.json`

Dizionario piatto chiave→stringa, ESATTAMENTE come richiesto da Maurizio — niente nesting JSON,
la struttura gerarchica è tutta nel NOME della chiave:

```json
{
  "config.title": "Configurazione",
  "config.transparency_label": "Trasparenza",
  "config.language_label": "Lingua",
  "common.save": "Salva",
  "common.cancel": "Annulla"
}
```

**Convenzione chiave**: `<finestra>.<elemento>` minuscolo, snake_case dove serve
(`config.web_search_note`), namespace `common.*` per bottoni/testi condivisi fra più finestre
(salva/annulla/chiudi/elimina). Verificato: **non esiste** una cartella di config-template tipo
`base_config/` in questo repo 2.0 (c'era in v1, non qui) — quindi i due file vanno committati
DIRETTAMENTE in `Test Run/Configuration/i18n/`, stesso trattamento di
`Test Run/Configuration/startup.json` (già committato, verificato con `git ls-files`).

**Interpolazione**: placeholder `{nome}` nella stringa, sostituito a runtime — es.
`"routine.not_found": "Routine '{name}' non trovata."` più una funzione `t(key, {name: "x"})`.
Nessuna regola di plurale nell'MVP: se serve singolare/plurale, due chiavi distinte o una
formulazione neutra ("N elementi" invece di "1 elemento"/"N elementi").

### 2. Caricamento — nuovo modulo Rust `crates/ui/src-tauri/src/i18n.rs`

Stesso pattern INFALLIBILE già usato da `config.rs::load_from` (leggi quel file per intero prima
di scrivere questo, è il riferimento esatto):
- file assente → dizionario vuoto (mai un errore che blocca l'avvio);
- JSON malformato → dizionario vuoto + `eprintln!`/`tracing::warn!` di avviso (usa
  `tracing::warn!`, non `eprintln!` — il progetto è passato a logging strutturato in una sessione
  precedente, vedi `crates/ui/src-tauri/src/main.rs` per lo stile);
- valido → il dizionario deserializzato (`HashMap<String, String>` va benissimo, è un dict piatto).

Nuovo comando Tauri `get_i18n(lang: String) -> HashMap<String, String>` in `main.rs`, stesso
schema IPC di `get_config`/`set_config` (righe ~248-261 di `main.rs`, leggile per il pattern
esatto: `State<'_, ...>`, path costruito da `config_file_path`-equivalente per la cartella
`i18n/`).

**Fallback chain quando una chiave manca**: `en.json` manca la chiave → prova `it.json` → se
manca anche lì, mostra la chiave stessa (es. `config.title`) — mai una stringa vuota o un crash:
deve essere visibile e grep-abile che manca una traduzione, non silenziosamente nascosto.

### 3. Preferenza lingua — nuovo campo su `Config` (`crates/ui/src-tauri/src/config.rs`)

```rust
#[serde(default = "default_language")]
pub language: String,
```
con `default_language() -> String { "it".into() }`. Leggi il commento in cima al file (il blocco
"Task 7, piano 1" che spiega perché il file non ha `#[serde(deny_unknown_fields)]`) e il test
`legacy_v1_config_json_with_extra_fields_still_loads` — il tuo campo nuovo deve rispettare lo
stesso contratto di compatibilità all'indietro (un `config.json` vecchio senza `language` deve
continuare a caricare, con default `"it"`).

Nessuna rilevazione automatica della lingua del sistema operativo nell'MVP: il default è sempre
`"it"`, punto.

### 4. Frontend — modulo `crates/ui/frontend/i18n.mjs` + `data-i18n` in HTML

- `i18n.mjs`: espone `t(key, params)` — **SOLO chiavi letterali**, mai `t(variabile)` né
  `` t(`prefisso.${x}`) `` — questo vincolo è quello che rende possibile un test automatico di
  parità (vedi sotto). All'avvio di ogni finestra, chiama `invoke("get_i18n", {lang})` (lingua
  letta da `get_config()`, già presente per `window_alpha`/`web_search_enabled` — segui lo stesso
  punto di lettura) e popola il dizionario in memoria per quella finestra.
- Negli `.html`: testo statico marcato `data-i18n="chiave"` (un walker DOM al caricamento
  sostituisce `textContent`), `data-i18n-placeholder="chiave"` per gli attributi `placeholder`,
  `data-i18n-title="chiave"` per `title`/tooltip. **Non spostare il testo dall'HTML al JS** — se
  oggi è in un tag HTML, resta in HTML con l'attributo `data-i18n`, il JS lo traduce sul posto.
- Nei `.js`: dove il testo è costruito a runtime (es. `note.textContent = "Le modifiche
  richiedono..."` in `config-dialog.js`), sostituisci con `t("chiave")`.
- **Cambio lingua a runtime**: ha effetto sulla PROSSIMA apertura/riapertura della finestra, MAI
  ri-traduzione live di una finestra già aperta — non è un requisito di questo compito, non
  inventarlo.

### 5. Titoli finestra — `crates/ui/src-tauri/src/main.rs`

Verificato con `grep -n '\.title('`: ci sono **titoli letterali in Rust**, non nel frontend —
`"Lare — Seleziona screener"`, `"Lare — Archivio"`, `"Lare — Configurazione"`, `"Lare — AI Chat"`,
`"Lare — Nota"` (righe indicative 525/974/1008/1084/1154, verifica i numeri esatti leggendo il
file, possono essere shiftati). Questi vanno tradotti anche loro, tramite lo stesso `get_i18n`
letto lato Rust (non serve un giro IPC: `i18n.rs` può esporre una funzione sincrona
`t_sync(lang, key) -> String` usata sia dal comando Tauri sia da questi siti in `main.rs`).
**Eccezione esplicita**: `"Lare Terminal"` (riga ~1598, titolo della finestra principale) è un
nome di prodotto, NON va tradotto — lascialo letterale.

### 6. Lingua dell'AI — SOLO sul canale cursore (lare-shell), Parte 3 più sotto

## Struttura del lavoro — 3 parti, un commit e uno STOP dopo ciascuna

**Non fare tutto in un colpo solo.** Dopo la Parte 1, fermati e scrivi il report parziale — è il
punto più economico per Maurizio/Claude per accorgersi di una convenzione sbagliata prima che sia
replicata su altre 10 finestre.

### Parte 1 — fondamenta + UNA finestra completa (`/config`), poi COMMIT e STOP

1. `crates/ui/src-tauri/src/i18n.rs` (nuovo) — `load_dict(path) -> HashMap<String,String>` con i
   test dello stesso stile di `config.rs` (assente, corrotto, valido) — leggi
   `crates/ui/src-tauri/src/config.rs` per intero prima di scrivere questi test, è il riferimento
   letterale di stile.
2. `get_i18n` come comando Tauri in `main.rs`, registrato nell'`invoke_handler!` (cerca dove sono
   elencati `get_config`/`set_config` per il punto esatto).
3. `Config.language` (§3 sopra) + test di compatibilità all'indietro.
4. `Test Run/Configuration/i18n/it.json` + `en.json` — popolali PRIMA SOLO con le chiavi che
   servono per `/config` (§5 sotto), non con tutte le chiavi del programma: cresceranno nelle
   Parti 2/3.
5. `crates/ui/frontend/i18n.mjs` (nuovo, con `node --test` — vedi §7 sotto per cosa deve testare).
6. Converti **`/config`** per intero: `config.html`, `config-dialog.js`, `config-window.js` — è la
   finestra più piccola (2 stringhe HTML dirette, ~9 stringhe in JS, verificato) ed è quella dove
   vive il nuovo controllo di scelta lingua, quindi va convertita comunque per prima.
7. Aggiungi il **dropdown lingua** in `/config` (`config.language_label` come etichetta, opzioni
   "Italiano"/"English" — i NOMI delle lingue nel selettore restano fissi, non tradotti: un
   utente che ha selezionato per sbaglio la lingua sbagliata deve poter sempre riconoscere la
   propria lingua nell'elenco).
8. Test di parità chiavi (§7) già attivo e verde per le chiavi esistenti a questo punto.
9. `cargo test -p ui`, `node --test crates/ui/frontend/*.test.mjs`, `cargo clippy -p ui
   --all-targets` — tutti verdi.
10. Commit. **Scrivi il report finale qui (usa il template §9 di `BRIEFING-AI-ESTERNE.md`) e
    fermati** — non proseguire alla Parte 2 nello stesso giro senza che Maurizio/Claude abbiano
    guardato la convenzione usata su `/config`.

### Parte 2 — le finestre restanti (dopo via libera su Parte 1)

Stesso trattamento di `/config` per ogni altra finestra con testo utente-visibile, in quest'ordine
(dal più piccolo, verificato per conteggio di stringhe): `note-window.html`/`.js`,
`window-search.html`/`.js`, `library.html`/`.js` (+ `library-nav.mjs`), `aichat-window.html`/`.js`
(+ `aichat-view.mjs`), `routine-preview.html`/`.js`, `plugin-window.html`/`.js`,
`external-channel.html`/`.js`, `screener-picker.html`/`.js`, `window.html`/`.js`, `host.html`/
`.js` (solo le stringhe UI-locali, es. `/help` locale — NON il testo che arriva già composto
dall'orchestratore via WS, resta fuori scope come spiegato sopra), `terminal.js` (idem).
**Metodo per trovare tutte le stringhe**, non fidarti solo di questo elenco: cerca
`textContent =`, `innerText`, `innerHTML`, `placeholder=`, `title=`, `aria-label=`, `alert(`,
literal template `` `...` `` con testo italiano, e ogni `>Testo<` negli `.html`.

**Effetto collaterale atteso, non un bug**: alcuni test esistenti (specialmente i `.test.mjs`)
asseriscono su stringhe UI letterali (es. verificano che un bottone dica "Salva"). Quei test
vanno aggiornati per asserire sulla CHIAVE o sull'output di `t()`, non riscritti per aggirare la
rottura — è lavoro atteso di questa parte, non un side-effect da minimizzare.

Commit (uno o più, a tua discrezione se la finestra è grande), report, poi Parte 3.

### Parte 3 — lingua dell'AI sul canale cursore

1. `crates/protocol/src/lib.rs` — nuovo campo `lang: String` su `ClientMsg::Command`, con
   `#[serde(default)]` (stringa vuota di default quando assente — NON `"it"` qui, la Parte
   orchestrator sotto decide cosa fare con una stringa vuota). Riferimento letterale per lo stile
   del test di round-trip: `command_web_search_defaults_false_when_absent` e
   `command_web_search_true_roundtrip` (stesso file) — scrivi l'equivalente per `lang`. Bump
   versione `crates/protocol` (patch, è additivo).
2. `crates/ui/frontend/ws-client.js` — quando costruisce il `Command` da mandare (dove oggi setta
   `web_search: !!webSearch`, riga indicativa 109), aggiungi `lang` letto dalla stessa
   `Config.language` già disponibile lato frontend.
3. `crates/orchestrator/src/agent.rs` — **NON tradurre `SYSTEM_PROMPT`** (è enorme, e va tenuto
   sincronizzato con OGNI futura modifica al comportamento dell'AI in OGNI lingua: manutenzione
   insostenibile, fuori scope). Invece:
   - Estrai l'ultima frase `"Rispondi in italiano, in modo conciso."` fuori da `SYSTEM_PROMPT` in
     una costante separata (es. `const RESPOND_ITALIAN: &str = " Rispondi in italiano, in modo
     conciso.";`), e aggiungi l'equivalente inglese (es. `const RESPOND_ENGLISH: &str = " Answer
     in English, concisely.";`).
   - `TurnOptions` guadagna un campo `lang: Option<String>` (mai un default hard-coded a `"it"`
     qui — `None`/stringa vuota → comportamento INVARIATO, cioè italiano, per non rompere i canali
     che non passano `lang` — Telegram, AI Chat, mcp-nmap — tutti quelli con
     `system_prompt_override` già impostato, che va lasciato ESATTAMENTE come oggi: la direttiva
     di lingua si applica SOLO al percorso di default di `system_prompt()`, mai quando
     `system_prompt_override` è `Some(...)`).
   - `system_prompt()` sceglie `RESPOND_ITALIAN` (default, quando `lang` è `None`/vuoto/`"it"`) o
     `RESPOND_ENGLISH` (quando `lang == "en"`) e la appende ESATTAMENTE dove oggi c'è la frase
     fissa, dopo l'eventuale `WEB_SEARCH_ADDENDUM`.
4. Punto di innesco reale: dove il valore `lang` del `ClientMsg::Command` arriva oggi diventa
   `TurnOptions.lang` — segui lo stesso percorso già usato da `web_search` (cerca dove
   `Command { web_search, .. }` viene letto e passato a `TurnOptions` in
   `crates/orchestrator/src/ws.rs` o dove il dispatch della sessione lo consuma).
5. Test: `system_prompt()` con `lang: Some("en".into())` deve contenere la frase inglese e NON
   quella italiana (e viceversa per `Some("it".into())`/`None`) — stesso stile di
   `system_prompt_mentions_web_tools_only_when_enabled` già in `agent.rs`.

Commit, report finale.

## Test che devono esistere (oltre a quelli citati per parte sopra)

- **Test di parità chiavi** (`node --test`, Parte 1, poi mantenuto verde in Parte 2): ogni
  chiamata letterale `t("...")` o attributo `data-i18n="..."` sotto `crates/ui/frontend/**`
  (esclusi `vendor/` e `*.test.mjs`) esiste in ENTRAMBI `it.json` ed `en.json`; ogni chiave nei due
  JSON è usata almeno una volta da qualche parte. Questo è il test che cattura "dimenticato di
  aggiungerlo a `en.json`" — è il più importante di tutto il compito, non saltarlo.
- `i18n.rs`: stessi 3 casi di `config.rs::load_from` (assente/corrotto/valido) + un test esplicito
  della fallback chain `en → it → chiave stessa`.
- Round-trip protocollo per `lang` (Parte 3).
- `agent::system_prompt` per le due lingue (Parte 3).

## Documentazione da aggiornare

Solo per i crate che tocchi davvero: `crates/ui/CHANGELOG.md` + `IMPLEMENTATION.md` (Parte 1+2),
`crates/protocol/CHANGELOG.md` + `crates/orchestrator/CHANGELOG.md`/`IMPLEMENTATION.md` (Parte 3).
`Docs/i18n/ita/HANDOFF.md` — una voce in FATTO per parte completata (tre voci separate, non una
sola a fine compito: il valore di questo doc è tracciare lo stato REALE anche se il compito si
ferma a metà). Bump versione solo dei crate effettivamente modificati in quella parte.

## Verifica finale (per ciascuna parte, prima del commit di quella parte)

```powershell
cargo build -p ui -p orchestrator -p protocol
cargo test -p ui -p orchestrator -p protocol
node --test crates/ui/frontend/*.test.mjs
cargo clippy -p ui -p orchestrator -p protocol --all-targets
```

Verifica dal vivo (dopo la Parte 1, e di nuovo dopo la Parte 3): `deploy_test_run.ps1` NON copia
`Configuration/` (verificato leggendo lo script — commento "Non tocca Test Run\Configuration"),
quindi i due file `it.json`/`en.json` committati in `Test Run/Configuration/i18n/` sono già al
posto giusto senza bisogno di modificare lo script di deploy. Avvia `Test Run\ui.exe`, apri
`/config`, cambia lingua a English, salva, riapri `/config`: deve mostrarsi in inglese. Dopo la
Parte 3: manda un comando dal terminale con lingua inglese impostata, verifica che l'AI risponda
in inglese.

## Cosa NON fare

- Non tradurre `SYSTEM_PROMPT` per intero — solo la frase finale, come specificato in Parte 3.
- Non toccare testo composto lato orchestratore e spedito come stringa già pronta (banner di
  conferma, errori tool, `/help` server-side) — fuori scope, spiegato sopra.
- Non toccare plugin, lare-shell (C#), Telegram, `Docs/`.
- Non tradurre `"Lare Terminal"` (nome prodotto) né i nomi delle lingue nel dropdown
  ("Italiano"/"English" restano fissi).
- Non inventare ri-traduzione live di finestre già aperte, né rilevazione automatica della lingua
  di sistema — entrambe esplicitamente fuori dall'MVP.
- Non fare tutte e 3 le parti in un solo commit/report — fermati dopo la Parte 1 come da
  struttura sopra.
