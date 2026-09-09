# NOTA PER MAURIZIO — leggi prima di copiare, poi cancella questo blocco

Questo file è un **template riusabile**, non un compito già pronto. Per assegnarne uno:

1. Sostituisci OGNI occorrenza di `es` (minuscolo, codice ISO 639-1 a 2 lettere) con il codice
   della lingua che vuoi aggiungere — es. `fr` per il francese, `de` per il tedesco.
2. Sostituisci OGNI occorrenza di `Español` con il nome nativo di quella lingua, scritto come lo
   scriverebbe un madrelingua — es. `Français`, `Deutsch` (NON l'italiano "Francese"/"Tedesco").
3. Sostituisci OGNI occorrenza di `SPANISH` (maiuscolo, dentro nomi di costanti Rust) col nome
   della lingua in maiuscolo ASCII — es. `FRENCH`, `GERMAN`.
4. Cancella questo blocco "NOTA PER MAURIZIO" (comprese queste 4 righe) e incolla il resto così
   com'è come compito per l'AI esterna.

Se la lingua ha un codice ISO diverso da 2 lettere, o ha varianti regionali (es. `pt-BR` vs
`pt-PT`), fermati e adatta a mano i punti dove il codice è usato come chiave `match` in Rust (Parte
1 sotto) — non è coperto automaticamente da questo template.

---

# Compito per AI esterna — aggiungi la lingua Español (es) all'i18n del programma

> **Prima cosa**: leggi per intero `Docs/i18n/ita/BRIEFING-AI-ESTERNE.md` alla radice del
> repository, poi questo file per intero, PRIMA di scrivere codice.

## Contesto

Il programma ha già un sistema i18n completo (frontend Tauri + lingua dell'AI), costruito in 3
parti precedenti — vedi `Docs/i18n/ita/compiti-ai-esterne/2026-09-08-i18n-programma.md` per la
storia completa se ti serve capire il design, non è necessario leggerlo per questo compito.
Oggi supporta **italiano** (`it`, default e fallback) e **inglese** (`en`). Aggiungere una lingua
NON è solo un file di traduzione: **3 punti del codice hanno l'elenco delle lingue hardcoded**,
verificati leggendo il codice (non assunti) prima di scrivere questo compito. Mancarne anche solo
uno lascia la lingua nuova rotta in modo silenzioso — nessun errore, nessun crash, semplicemente
non funziona (l'AI resta in italiano, o il dropdown non mostra l'opzione, o un `es.json` sbagliato
passa inosservato).

## Scope — i 3 punti hardcoded da toccare, uno per uno

### 1. `crates/orchestrator/src/agent.rs` — direttiva di lingua per l'AI

Oggi (leggi il file per il contesto esatto, righe indicative):

```rust
pub const RESPOND_ITALIAN: &str = " Rispondi in italiano, in modo conciso.";
pub const RESPOND_ENGLISH: &str = " Answer in English, concisely.";
```

e dentro `system_prompt`:

```rust
let lang_directive = match opts.lang.as_deref() {
    Some("en") => RESPOND_ENGLISH,
    _ => RESPOND_ITALIAN,
};
```

Aggiungi:

```rust
pub const RESPOND_SPANISH: &str = " Responde en español, de forma concisa.";
```

(la frase dev'essere scritta NELLA lingua stessa, come le due esistenti — non tradurre "Respond
in Spanish" in italiano, scrivi l'equivalente in spagnolo, con lo stesso registro conciso/diretto
delle altre due) e aggiungi un `arm` al match:

```rust
Some("es") => RESPOND_SPANISH,
```

**Non toccare** l'ordine o il comportamento di default (`_ => RESPOND_ITALIAN` resta l'ultimo
braccio, invariato) — l'italiano resta il fallback per qualunque lingua non riconosciuta.

Estendi il test esistente `system_prompt_respects_language_directive` (nello stesso file) con lo
stesso pattern già usato per `en`: verifica che `lang: Some("es".into())` produca un prompt che
finisce con `RESPOND_SPANISH` e NON contiene né `RESPOND_ITALIAN` né `RESPOND_ENGLISH` (usa
`.trim()` sulle costanti nel confronto `contains`, stesso stile degli assert esistenti).

### 2. `crates/ui/frontend/config-dialog.js` — dropdown lingua

Oggi (riga indicativa ~350, cerca `_buildSelect` per il contesto):

```js
[
  { value: "it", label: "Italiano" },
  { value: "en", label: "English" },
],
```

Aggiungi una terza voce:

```js
{ value: "es", label: "Español" },
```

Il `label` resta **fisso, non tradotto** (stessa regola di "Italiano"/"English": un utente che ha
selezionato la lingua sbagliata per sbaglio deve sempre riconoscere la propria lingua nell'elenco,
a prescindere da quale lingua è attualmente selezionata).

### 3. `crates/ui/frontend/i18n-parity.test.mjs` — generalizza il test, non limitarti ad aggiungere

Oggi il test è scritto per ESATTAMENTE `it.json` ed `en.json` (due variabili `itFile`/`enFile`,
due dizionari confrontati a mano). **Non aggiungere un terzo blocco parallelo `esFile`/`esDict`** —
riscrivi il test perché scansioni TUTTI i file `*.json` presenti in
`Test Run/Configuration/i18n/` e verifichi che abbiano tutti lo stesso set di chiavi (usa
`it.json` come riferimento, dato che è la lingua di fallback), mantenendo intatte le due verifiche
già presenti (ogni chiave usata nel codice esiste nei dizionari; ogni chiave nei dizionari è usata
nel codice — quella logica non cambia, cambia solo che si applica a N file invece di 2).

Questo è un investimento fatto una volta sola: se lo scrivi bene, la PROSSIMA lingua che verrà
aggiunta con questo stesso template non dovrà più toccare questo file — il test la coprirà da
solo. Fallo bene.

## Parte 4 — `Test Run/Configuration/i18n/es.json` (il file di traduzione)

Crea il file traducendo **tutte** le chiavi oggi presenti in
`Test Run/Configuration/i18n/it.json` (verifica tu il numero esatto leggendo il file — è cresciuto
nel tempo, non fidarti di un numero scritto altrove). Regole:

- Traduzione in spagnolo naturale, madrelingua, stesso registro conciso/diretto di `it.json`/
  `en.json` (non formale/burocratico, non un traduttore automatico letterale).
- **Ogni placeholder `{nome}` va preservato ESATTAMENTE** (stesso nome della variabile, es. `{name}`,
  `{old}`, `{query}` — non tradurre il nome del placeholder, solo il testo intorno).
- Chiavi in ordine alfabetico, stesso stile di `it.json`/`en.json` (`git diff` deve mostrare un
  file ordinato, non un elenco alla rinfusa).
- **Non toccare** `it.json` o `en.json` — solo aggiungerne uno nuovo.
- Il nome del prodotto **"Lare Terminal"**, se compare in una chiave, resta letterale, non
  tradotto (stessa eccezione già rispettata nelle due lingue esistenti — verifica se compare
  cercando "Lare Terminal" dentro `it.json`).

## Documentazione e versioni

- `crates/orchestrator`: bump patch (tocchi `agent.rs`, comportamento nuovo) — `Cargo.toml` +
  `CHANGELOG.md` + `IMPLEMENTATION.md`.
- `crates/ui`: bump patch (tocchi `config-dialog.js` + `i18n-parity.test.mjs`) — `Cargo.toml` +
  `CHANGELOG.md` + `IMPLEMENTATION.md`.
- `crates/protocol`: **non toccare**, `lang` è già una stringa libera lì, nessuna modifica serve.
- `Docs/i18n/ita/HANDOFF.md`: una voce in FATTO che dice quale lingua hai aggiunto.

## Verifica finale (comandi da eseguire davvero)

```powershell
cargo build -p orchestrator -p ui
cargo test -p orchestrator -p ui
node --test crates/ui/frontend/*.test.mjs
cargo clippy -p orchestrator -p ui --all-targets
```

Il test di parità (Parte 3) deve risultare verde con 3 file (`it`/`en`/`es`) — se lo hai scritto
generico, questo è già garantito senza altro lavoro.

## Verifica dal vivo (se il tuo ambiente lo consente)

Come per i compiti precedenti, se non hai un display interattivo per aprire davvero `ui.exe` e
selezionare Español dal dropdown, scrivilo esplicitamente nel report — non è un blocco, il
supervisore la fa lui dal vivo prima del merge.

## Cosa NON fare

- Non creare un quarto file o un quarto punto hardcoded "per sicurezza" — solo i 3 punti elencati
  sopra più il file di traduzione.
- Non toccare `it.json`/`en.json`.
- Non toccare `crates/protocol` (già generico).
- Non toccare `shell/lare-shell` (C#, invariato — il canale cursore riceve la lingua dal fallback
  su `config.json`, non dal wire, come già verificato nelle parti precedenti).
- Non tradurre "Lare Terminal" né il nome della lingua nel dropdown.
