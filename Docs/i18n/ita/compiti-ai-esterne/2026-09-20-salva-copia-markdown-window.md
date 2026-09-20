# Compito per AI esterna — Bottone "Salva" sempre attivo (stesso file) + "Copia come testo"/"Copia come markdown"

> **Prima cosa**: leggi per intero `Docs/i18n/ita/BRIEFING-AI-ESTERNE.md` alla radice del
> repository, poi questo file per intero, PRIMA di scrivere codice. **Crea la tua worktree
> dedicata come primissima cosa** (`git worktree add .worktrees/salva-copia-md-window -b
> feat/salva-copia-md-window`), come impone BRIEFING-AI-ESTERNE.md §1/§6.

## Contesto — perché serve questo cambiamento

Maurizio ha fatto una richiesta `/ai` a Lare Terminal. La finestra Markdown (`show_markdown`) si è
aperta subito con un placeholder e ha continuato ad aggiornarsi mentre la ricerca dell'AI
procedeva in background (comportamento normale, vedi `output:content`/`output:subscribe` in
`window.js`). Ha premuto il bottone "Salva" **prima che la ricerca finisse**: il testo parziale è
finito in Library. Risultato: due copie dello stesso contenuto, una (la finestra ancora aperta) che
ha continuato ad aggiornarsi fino al testo definitivo, l'altra (il file salvato in Library) ferma
alla versione parziale — e nessun modo, dall'interfaccia, di riallineare la seconda alla prima
senza rifare tutto a mano.

Causa: il bottone "Salva" attuale si comporta come un'azione **una tantum** pensata per una
finestra il cui contenuto non cambia più dopo l'apertura (il caso originale, v0.11.0) — non per una
finestra `show_markdown`/canale di output che continua a ricevere aggiornamenti (`output:content`)
dopo che l'utente l'ha già salvata una volta.

Il compito ha due parti indipendenti, **in quest'ordine, ciascuna con il proprio commit**: prima il
fix del bottone Salva (bug reale, vissuto dal vivo), poi i due bottoni Copia (feature nuova, stesso
angolo di finestra).

---

## Parte A — bottone "Salva": resta sempre "Salva", risalva sempre nello STESSO file

### A.1 — Comportamento attuale (da cambiare)

`crates/ui/frontend/window.js`, righe 327-350:

```js
saveBtnEl.addEventListener("click", async () => {
  if (saveBtnEl.disabled) return;
  saveBtnEl.disabled = true;

  try {
    await invokeCmd("archive_save", { title: saveTitle, content: saveContent });
    saveBtnEl.textContent = t("common.saved");
  } catch (e) {
    console.error("[archive] archive_save failed:", e);
    saveBtnEl.disabled = false;
    const originalText = saveBtnEl.textContent;
    saveBtnEl.textContent = "✗";
    setTimeout(() => { saveBtnEl.textContent = originalText; }, 1500);
  }
});
```

Due problemi, entrambi da correggere:

1. **Il bottone si disabilita e diventa "✓ Salvato" per il resto della sessione della finestra**
   (commento esplicito nel codice attuale, riga 337: *"button stays disabled ... The user cannot
   click again until the window is reopened"*). Questo è ESATTAMENTE ciò che ha ingannato
   Maurizio: ha visto "Salvato", ha creduto che il lavoro fosse concluso, e non si è accorto che il
   contenuto della finestra continuava a cambiare sotto quell'etichetta rassicurante.
2. **Ogni click su "Salva" chiama sempre `archive_save`**, che (vedi
   `crates/ui/src-tauri/src/main.rs:1233` e `crates/ui/src-tauri/src/archive.rs:336`) genera un
   filename NUOVO ad ogni chiamata (`{slug(title)}-{timestamp_ms}.md`, il timestamp è preso da
   `SystemTime::now()` a ogni invocazione). Anche se il bottone restasse cliccabile, ri-premerlo
   creerebbe un secondo file diverso in Library, non aggiornerebbe il primo.

### A.2 — Comportamento richiesto

- Il bottone resta **sempre** etichettato "Salva" (mai rinominato in "Salvato") ed è **sempre
  cliccabile** — sia mentre la ricerca è in corso (`turnActive === true`), sia dopo che è
  terminata. Non deve MAI passare in uno stato permanentemente disabilitato/rinominato.
- **Primo click** nella vita della finestra → chiama `archive_save` come oggi, MA memorizza il
  filename restituito (la promise di `archive_save` risolve con la stringa del filename creato —
  vedi la firma Rust: `fn archive_save(...) -> Result<String, String>`).
- **Ogni click successivo** (indifferente se la ricerca è ancora in corso o già finita) → chiama
  **`archive_update`** (comando Tauri già esistente, NON va creato da zero) passando il filename
  memorizzato al primo salvataggio, così il file su disco viene sovrascritto con il contenuto più
  recente invece di crearne uno nuovo. Firma Rust già pronta all'uso, nessuna modifica richiesta
  lato backend:

  ```rust
  // crates/ui/src-tauri/src/main.rs:895
  #[tauri::command]
  fn archive_update(app: AppHandle, file: String, title: String, content: String) -> Result<(), String> {
      let library_dir = documents_dir_path(&app)?;
      archive::update(&library_dir, &file, &title, &content)
  }
  ```

  `archive::update` (`crates/ui/src-tauri/src/archive.rs:504`) sovrascrive un file ESISTENTE con
  stesso filename, nuovo contenuto — è già usato esattamente con questo scopo dalla feature
  "Espandi" della stessa finestra (`window.js:475`, funzione `finishExpand`): prendila come
  riferimento diretto di come invocarlo (stessi tre argomenti: `file`, `title`, `content`).
  È già testato in `archive.rs` (`update_overwrites_existing_file_content_and_title`,
  `update_preserves_filename_no_new_file_created`, righe 1646 e 1659) — non serve toccare quei
  test, solo il modo in cui `window.js` decide quale comando chiamare.
- **Stato iniziale del filename memorizzato**: parti sempre da "nessun file salvato ancora"
  (`null`), ANCHE per una finestra aperta dalla Library (`data.kind === "archived"`, che arriva già
  con un `data.source_file` esistente sul disco). Non usare `source_file` come filename iniziale
  per "Salva": quel campo è la sorgente usata dal flusso "Espandi" (bottone separato) per il SUO
  overwrite — riusarlo qui cambierebbe silenziosamente il comportamento attuale di "Salva" su una
  finestra archiviata (oggi crea sempre un nuovo file duplicato in Library, comportamento
  invariato e fuori scope per questo compito). Se durante il lavoro ti accorgi che avrebbe più
  senso unificare i due filename, NON deciderlo da solo: implementa lo scope minimo (Salva parte
  sempre da `null`) e segnala l'osservazione nel report finale.
- Il contenuto salvato ad ogni click deve essere quello **attualmente mostrato nella finestra in
  quel momento**. Attenzione: `window.js` tiene DUE variabili separate che partono entrambe da
  `data.content` e sembrano equivalenti ma NON lo sono sempre:
  - `saveContent` (dichiarata riga 325) — aggiornata SOLO da `applyOutputContent`
    (righe 377-385, il listener `output:content` per le finestre `output-*`).
  - `currentContent` (dichiarata riga 357) — aggiornata sia da `applyOutputContent` (riga 380) SIA
    da `finishExpand` (riga 480, il completamento del bottone "Espandi", disponibile sulle finestre
    `kind === "archived"`).
  Per una finestra `output-*` le due restano sincronizzate (unico punto che le aggiorna entrambe).
  Ma per una finestra archiviata su cui l'utente ha usato "Espandi", `saveContent` resta FERMA al
  contenuto pre-espansione mentre `currentContent` è quello vero mostrato a schermo — usare
  `saveContent` per Salva/Copia in quel caso salverebbe/copierebbe testo vecchio, esattamente il
  tipo di bug che questo compito deve evitare. **Usa `currentContent` come unica fonte di verità**
  per il contenuto passato a `archive_save`/`archive_update` — non introdurne una terza (la Parte B
  la riusa identica per "Copia come markdown"). Se preferisci eliminare `saveContent` come
  variabile ridondante (invece di lasciarla inutilizzata), va bene, ma documentalo nel report:
  verifica con `grep -n saveContent crates/ui/frontend/window.js` che non resti nessun altro uso
  dimenticato prima di rimuoverla.
- Guardia contro doppio click concorrente: va bene disabilitare il bottone SOLO per la durata della
  singola chiamata IPC in corso (per evitare due `archive_save`/`archive_update` sovrapposti che
  scriverebbero lo stesso file in corse diverse), ma va **ri-abilitato subito dopo**, che la
  chiamata sia andata a buon fine o no — mai lasciarlo disabilitato dopo che la risposta è arrivata.
  Il testo del bottone in quella finestra di tempo può restare "Salva" invariato, oppure un
  indicatore di caricamento se preferisci — MA deve tornare a "Salva" (mai "Salvato") non appena la
  chiamata IPC risponde.
- In caso di errore: stesso pattern già presente oggi (flash "✗" per ~1.5s poi torna al testo
  originale) — quel comportamento va bene com'è, non toccarlo se non necessario.

**Nota per il supervisore (facoltativa per te, mettila comunque nel report):** rimuovere ogni
feedback di successo permanente lascia l'utente senza modo di sapere "è appena stato salvato
adesso" — se vuoi aggiungere un'indicazione di successo puramente TRANSITORIA (es. un'icona che
per ~1 secondo diventa "✓" e poi torna da sola a "Salva", stesso identico meccanismo del flash
d'errore già in codice), è un miglioramento compatibile con la richiesta e NON contraddice "il
pulsante non si trasforma in Salvato, rimane Salva" (quella frase vieta uno stato stabile
rinominato, non un lampeggio che si autoripristina). Non è obbligatorio: se preferisci lo scope
minimo, non aggiungerlo — segnala comunque la scelta nel report finale (ambiguità reversibile,
sezione 8 del BRIEFING).

### A.3 — TDD: estrai la decisione "quale comando chiamare" in un modulo puro

`window.js` non ha copertura di test a livello DOM (debito noto del progetto, commit
`f543797` — non è compito tuo risolverlo, ma non aggiungere logica testabile SENZA test solo
perché "tanto window.js non è testato"). Il pattern consolidato in questo crate per la logica che
NON dipende dal DOM è estrarla in un modulo `.mjs` puro, importato da `window.js` e testato a parte
con `node --test` — guarda `crates/ui/frontend/table-sort.mjs` +
`crates/ui/frontend/table-sort.test.mjs`, o `crates/ui/frontend/ui-local.mjs`, come riferimento
diretto di stile (commento di testata in italiano che spiega perché il modulo è "puro", funzioni
piccole, un `export` per funzione).

La decisione "quale comando Tauri chiamare, con quali argomenti" a ogni click di Salva è pura
(dipende solo da: esiste già un filename salvato? con che titolo/con che contenuto?) — NON dipende
dal DOM. Estraila in un nuovo file `crates/ui/frontend/save-state.mjs`, firma indicativa (adattala
se trovi un design migliore, ma mantienila pura):

```js
// save-state.mjs — logica pura (senza DOM, senza Tauri) per decidere se il
// click su "Salva" deve creare un nuovo documento in Library (primo salvataggio
// di questa finestra) o sovrascrivere quello già creato in precedenza (click
// successivi) — vedi window.js, wiring di #save-btn. Testabile con node:test.

/**
 * @param {string|null} savedFile — filename già restituito da un `archive_save`
 *   precedente in questa stessa finestra, o `null` se non si è ancora salvato.
 * @param {string} title
 * @param {string} content — il contenuto ATTUALE (più recente) da salvare.
 * @returns {{cmd: "archive_save"|"archive_update", args: object}}
 */
export function planSave(savedFile, title, content) {
  if (savedFile) {
    return { cmd: "archive_update", args: { file: savedFile, title, content } };
  }
  return { cmd: "archive_save", args: { title, content } };
}
```

Scrivi PRIMA il test che fallisce (RED, eseguito davvero con `node --test
crates/ui/frontend/save-state.test.mjs`), poi il codice minimo che lo fa passare (GREEN). Almeno
questi due casi (aggiungine altri se ti vengono in mente, es. contenuto vuoto):

```js
import { test } from "node:test";
import assert from "node:assert/strict";
import { planSave } from "./save-state.mjs";

test("planSave chiama archive_save quando la finestra non ha ancora un file salvato", () => {
  assert.deepEqual(planSave(null, "Titolo", "testo"), {
    cmd: "archive_save",
    args: { title: "Titolo", content: "testo" },
  });
});

test("planSave chiama archive_update con lo stesso filename una volta che esiste già", () => {
  assert.deepEqual(planSave("titolo-123.md", "Titolo", "testo aggiornato"), {
    cmd: "archive_update",
    args: { file: "titolo-123.md", title: "Titolo", content: "testo aggiornato" },
  });
});
```

`window.js` poi usa `planSave` dentro il click handler invece di decidere inline quale comando
chiamare, e aggiorna la variabile che tiene il filename salvato quando `archive_save` risolve
(quel bit di wiring resta dentro `window.js`, con `invokeCmd`/`saveBtnEl` — non è pure, va bene
che resti lì e non testato, coerente col debito noto).

Facoltativo ma economico: un test Rust in `crates/ui/src-tauri/src/archive.rs` (stesso modulo dei
test già citati) che fissi esplicitamente il contratto "il filename restituito da `save` è quello
che `update` accetta senza errore" (es.
`save_then_update_with_returned_filename_overwrites_same_file`) — non l'hai toccato tu in questo
compito, ma pinnarlo con un test evita che una modifica futura a `save`/`update` rompa
silenziosamente questo esatto flusso.

### A.4 — Verifica dal vivo obbligatoria (riproduzione del bug originale)

Il wiring "quale comando chiamare" è puro e coperto da `save-state.test.mjs`, ma la parte che lo
collega a `window.js`/IPC reale non è testabile automaticamente. Riproduci il bug originale per
essere sicuro di averlo davvero chiuso: avvia una richiesta `/ai` che apra una finestra
`show_markdown` con una ricerca che duri qualche secondo, clicca "Salva" MENTRE il badge mostra
"🔍 ricerca in corso…", aspetta che il badge passi a "✓ completato", clicca di nuovo "Salva". Apri
la cartella `documents/` (sotto la `Configuration/` di test, vedi `documents_dir_path` in
`main.rs`) e verifica ad occhio: **un solo file** per quella finestra, e il suo contenuto è il
testo DEFINITIVO (post-ricerca), non quello parziale del primo click. Se compare un secondo file,
la Parte A non è corretta.

### A.5 — Versioni e verifica

Fix di comportamento → bump **patch**: `crates/ui/src-tauri/Cargo.toml` (crate attualmente a
`2.3.8` — verifica tu il numero corrente, non fidarti di questo se nel frattempo è cambiato).
`CHANGELOG.md`/`IMPLEMENTATION.md` di `ui` (sezione `### Save button in Markdown windows`, righe
3222-3232 circa, descrive ancora il vecchio comportamento "una tantum, disabilitato, Salvato" — va
riscritta), `Docs/i18n/ita/HANDOFF.md` nello stesso commit.

```powershell
node --test crates/ui/frontend/*.test.mjs
cargo build -p ui
cargo clippy --all-targets ; cargo fmt --check
```

## FINE PARTE A — commit e stop

Commit descrittivo (`git commit -F <file>`, mai `-m` inline con backtick), poi fermati: aspetta
conferma/prossimo via libera prima di iniziare la Parte B (anche se lavori nella stessa worktree/
branch, sono due commit distinti).

---

## Parte B — due nuovi bottoni: "Copia come testo" e "Copia come markdown"

### B.1 — Dove e cosa

Accanto al bottone "Salva" (stesso contenitore `<span>` nella titlebar,
`crates/ui/frontend/window.html:365-368`):

```html
<span>
  <button id="save-btn" data-i18n-title="md_window.save_title" data-i18n-aria-label="md_window.save_aria" title="Salva nell'archivio" aria-label="Salva finestra nell'archivio">&#x1F4BE;</button>
  <button id="close-btn" data-i18n-title="common.close" data-i18n-aria-label="md_window.close_aria" title="Chiudi" aria-label="Chiudi finestra">&#x00D7;</button>
</span>
```

Aggiungi due bottoni **tra** `save-btn` e `close-btn` (stesso stile CSS dei bottoni esistenti —
guarda le regole `#save-btn`/`#close-btn` nel `<style>` in cima a `window.html` e riusale, non
inventare una classe nuova senza motivo):

- `#copy-text-btn` — "Copia come testo": copia negli appunti il **testo semplice** attualmente
  visibile nella finestra (il markdown renderizzato, senza marcatori `#`, `*`, `` ` ``, ecc. —
  esattamente ciò che l'utente legge a schermo). Il modo più diretto e coerente con la pipeline
  già in uso (`renderMarkdown`, `window.js:189-199`, che scrive già l'HTML sanificato dentro
  `#content`) è leggere `contentEl.innerText` (o `.textContent`, valuta quale rende meglio con le
  tabelle/liste già presenti — guarda `sortTableByColumn`/il markup delle tabelle generato da
  `screening.py::_table` per capire cosa produce `innerText` su una tabella reale prima di
  scegliere) al momento del click, non un valore catturato una volta sola all'apertura.
- `#copy-markdown-btn` — "Copia come markdown": copia negli appunti il **markdown sorgente**
  così com'è, cioè la stessa stringa che il bottone "Salva" salverebbe in quel momento —
  `currentContent` (vedi la nota su `saveContent` vs `currentContent` nella Parte A: usa SEMPRE
  `currentContent`, mai `saveContent`, per lo stesso motivo). Se hai lavorato la Parte A in un
  commit separato già mergiato, questa variabile è già quella giusta da riusare senza modifiche.

Entrambi:

- Usano l'API browser nativa `navigator.clipboard.writeText(testo)` — **non** serve il plugin
  Tauri clipboard-manager né una nuova capability in `crates/ui/src-tauri/capabilities/*.json`:
  è un'API del webview, non un comando IPC Tauri (nota bene: ogni file di capability in questo
  progetto elenca esplicitamente "Global-shortcut e clipboard NON inclusi" — è così di proposito,
  non serve cambiarlo per questa API browser). **Verifica comunque dal vivo** (build reale, click
  reale nella finestra Markdown) che `navigator.clipboard.writeText` funzioni davvero dentro il
  webview WebView2 di questo progetto su Windows — non è mai stato usato prima in questo
  repository (verificato: nessun uso di `clipboard`/`writeText` fuori dai file vendored). Se
  dovesse risultare bloccato/silenziosamente fallito, NON aggiungere il plugin Tauri di tua
  iniziativa: fermati, documenta il problema esatto (messaggio di errore/comportamento osservato)
  nel report finale e lascia la decisione al supervisore — aggiungere una nuova dipendenza esterna
  richiede comunque una segnalazione esplicita (sezione 7 del BRIEFING).
- Danno un feedback transitorio di successo/errore coerente con quello già usato per "Salva"
  (icona che cambia per ~1-1.5s e torna da sola, stesso pattern del flash "✗" già in `window.js`)
  — qui NON c'è il problema della Parte A (nessuno stato stabile da evitare), quindi un feedback
  che si autoripristina va bene senza remore.
- Sono cliccabili indipendentemente dallo stato di `saveBtnEl` (non li disabilitare insieme al
  bottone Salva: copiare non scrive su disco, non ha bisogno della stessa guardia anti-corsa).
- Sono visibili su OGNI finestra Markdown, non solo `output-*` — anche una finestra Markdown
  "normale" (non aperta da un turno AI) deve poterli usare sul contenuto statico che ha.

### B.2 — Nuove chiavi i18n

In tutti e tre i file (`it.json`/`en.json`/`es.json` sotto `Test Run/Configuration/i18n/` — sono
la sorgente vera delle stringhe, tracciati in git direttamente lì, non generati da una build,
verificato), accanto alle chiavi `md_window.save_*` già esistenti (per riferimento di formato, da
`it.json`):

```
"md_window.save_aria": "Salva finestra nell'archivio",
"md_window.save_failed": "salvataggio fallito: {error}",
"md_window.save_title": "Salva nell'archivio",
```

Aggiungi (valori IT indicativi, traduci EN/ES nello stesso stile sobrio già usato per le altre
righe — guarda `md_window.save_aria`/`save_title` in `en.json`/`es.json` come riferimento diretto):

```
"md_window.copy_text_title": "Copia come testo"
"md_window.copy_text_aria": "Copia il contenuto della finestra come testo semplice"
"md_window.copy_markdown_title": "Copia come markdown"
"md_window.copy_markdown_aria": "Copia il contenuto della finestra come markdown"
"md_window.copy_failed": "copia fallita: {error}"
```

Valuta tu se serve anche una chiave per il feedback transitorio di successo (es. una nuova
`common.copied` "✓ Copiato") — dettaglio reversibile, documenta la scelta nel report.

### B.3 — Verifica

Non c'è modo pulito di estrarre `contentEl.innerText`/`navigator.clipboard.writeText` in un modulo
puro testabile con `node --test` (dipendono dal DOM/browser reale, stesso limite già documentato
per il resto di `window.js`). La verifica è **dal vivo**: build reale (`cargo build -p ui`),
riavvio dell'app, apertura di una finestra Markdown reale, click su entrambi i bottoni, verifica
manuale (incolla altrove) che il contenuto copiato sia quello atteso nel formato giusto. Riportalo
esplicitamente nel report finale come verifica manuale, non provare a inventare un test automatico
che non testerebbe nulla di reale.

Feature nuova → bump **minor** (patch a 0): `crates/ui/src-tauri/Cargo.toml`. `CHANGELOG.md`/
`IMPLEMENTATION.md` di `ui` (nuova sottosezione per i due bottoni Copia, accanto a quella
riscritta in A.5), `Docs/i18n/ita/HANDOFF.md` nello stesso commit.

```powershell
node --test crates/ui/frontend/*.test.mjs
cargo build -p ui
cargo clippy --all-targets ; cargo fmt --check
```

## FINE PARTE B — commit e stop

Stesso formato di commit di Parte A, poi fermati per la revisione del supervisore.

---

## Cosa NON fare (entrambe le parti)

- Non toccare il comportamento di `archive_save`/`archive::save` (naming del file, timestamp) —
  resta invariato, serve solo per il PRIMO salvataggio.
- Non toccare `archive_update`/`archive::update` — è già corretto e già testato per questo scopo.
- Non aggiungere il plugin Tauri clipboard-manager preventivamente "perché potrebbe servire" — solo
  se la verifica dal vivo dimostra che l'API browser nativa non basta (B.1).
- Non toccare la feature "Espandi" (righe 352-499 di `window.js`) se non per il riuso in lettura
  che ti serve per capire il pattern `archive_update` — non modificarne il comportamento.
- Non toccare il gate di chiusura (`onCloseRequested`, righe 46-105) — è una feature delicata già
  corretta dopo due bug reali in produzione (vedi i commenti nel file stesso), fuori scope.
