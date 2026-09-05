# Changelog — crates/plugin-crypto

All notable changes to this package follow [Keep a Changelog](https://keepachangelog.com/) format.
Versioning: `major.minor.update`.

---

## 2.0.0 — 2026-09-05 — fork da v1 1.0.1

Copia del crate dalla v1 (`mauriziolobello/lare-terminal`) nel repo 2.0. Nessuna modifica
funzionale in questa voce; le modifiche del piano 1 seguono nelle voci successive.

## [1.0.1] — 2026-07-19 — fix: clamp difensivo su RSA bits + version drift plugin.json

**Fix Important — `generate_keypair` poteva appendere il sidecar (RSA):**
il campo "Bit chiave" della dialog parametri è un `<input>` libero — i
valori `min: 512, max: 4096` dichiarati in `ParamField::Number` non sono mai
applicati (né dal rendering né da `Rsa::generate`). Con un valore piccolo
(es. `4`), `generate_keypair` chiamava `generate_prime(2)`: il candidato a
2 bit è deterministicamente sempre `3` (i bit forzati a 1 saturano l'intero
spazio a 2 bit), quindi `p` e `q` valevano SEMPRE `3` — il controllo `if p
== q { continue }` non usciva mai dal loop, hang reale del processo
sidecar (verificato: con `bits=0`, `set_bit(bits-1, ..)` sottrarrebbe con
overflow, ancora peggio). Fix: `generate_keypair` clampa `bits` a un minimo
di 16 (8 bit per primo) PRIMA di dividerlo per 2 — difesa in profondità,
non richiede toccare la UI/dropdown in questo fix.
- **Test nuovo** (`ciphers/rsa_math.rs`):
  `generate_keypair_with_tiny_bits_does_not_hang` — chiama
  `generate_keypair(4)` e verifica `kp.n.bits() >= 14`. RED verificato dal
  vivo: **il test non terminava** (hang reale via timeout, non un assert
  fallito) prima del fix; GREEN dopo il clamp, in millisecondi.

**Fix minor — `plugin.json` fermo alla versione scaffold:** dichiarava
ancora `"version": "0.1.0"` (valore del Task 2) mentre il crate era già
alla 1.0.0. Il campo non è consumato funzionalmente (solo parsing) ma era
una vetrina fuorviante. Allineato alla versione corrente di questo rilascio
(`1.0.1`).

**Suite crate: 52 test** (51 precedenti + 1 nuovo in `rsa_math.rs`).

---

## [1.0.0] — 2026-07-19 — primo rilascio funzionalmente completo

**Smoke test dal vivo superato (Task 9, due giri):** tutti i 12 task del
piano implementati, revisionati e verificati con l'app reale in esecuzione
(`/crypto`, orchestrator + UI Tauri). I 3 bug reali trovati nel primo giro
(pannelli minuscoli, testo che sfarfalla in digitazione, pulsanti dialog
irresponsivi — nessuno rilevabile dai 51 test unitari, tutti asseriscono su
`Vec<PluginToHost>`, mai su un round-trip renderizzato in una finestra vera)
sono stati diagnosticati e risolti (Task 10/11/12); un secondo giro di smoke
test dal vivo conferma tutto funzionante: i 3 cifrari (Cesare, Vigenère,
RSA con generazione chiavi), la finestra principale (sidebar + due pannelli
live-bidirezionali), la dialog parametri come vera seconda finestra Tauri
**spostabile indipendentemente** (il requisito che ha motivato il fix
dell'orchestrator al Task 1), stile coerente col resto dell'app.

Nessuna nuova funzionalità in questa entry — segna la fine del piano
`Docs/superpowers/plans/2026-07-19-crypto-plugin.md`, non un cambio di
codice. Suite crate invariata: **51 test**, tutti verdi.

---

## [0.7.2] — 2026-07-19 — Applica classi CSS condivise input/pulsanti (Task 12)

**Classi `.lare-input` e `.lare-button` dal catalogo host:** `render.rs` ora
applica le classi CSS già definite in `crates/ui/frontend/plugin-catalog.css`
(caricate automaticamente in ogni finestra plugin):
- `.lare-input` (sfondo/bordo/colore coerenti col tema scuro) applicata alle
  due `<textarea data-evt="plaintext">`/`<textarea data-evt="ciphertext">` in
  `render_main_window` e ai tre `<input>` della dialog parametri (due editabili
  nei rami `ParamField::Number`/`Text`, uno readonly in `ParamField::GeneratedPair`).
- `.lare-button` (bordo/padding/hover coerenti) applicata ai quattro `<span>`
  pulsanti: `open-params` (toolbar finestra principale), `params-apply`,
  `params-close`, `params-generate` (dialog parametri).

Le classi si combinano senza conflitto coi CSS inline del plugin (es.
`.crypto-panel textarea { min-height:240px; ... }` resta invariato, `.lare-input`
non dichiara altezza). Nessuna nuova regola CSS aggiunta a `INLINE_CSS`.
Suite crate: **51 test totali** (invariati dai task precedenti, tutti verdi).

---

## [0.7.1] — 2026-07-19 — Fix CSS layout textarea altezza (Task 10)

**Fix textarea minuscole:** il CSS `.crypto-panel textarea { flex: 1 }` non
funziona perché il genitore `.crypto-root` ha `height: 100%` senza base da cui
ereditare — l'intera architettura delle finestre plugin è auto-fit al
contenuto. Sostituito con `min-height: 240px` (altezza esplicita) +
`width: 100%; box-sizing: border-box` per mantenere il filling orizzontale.
`.crypto-root`/`.crypto-panels`/`.crypto-panel` rimangono `display: flex` per
la disposizione orizzontale (sidebar + due colonne), che non ha questo
problema.

**Test:** `textarea_has_explicit_min_height()` verifica che il CSS ritornato
da `render_main_window` contenga `min-height`, prima linea di difesa
automatizzabile. La verifica visiva reale resta il supervisore umano (le
textarea ora riempiono l'altezza della finestra come previsto).

Suite crate: **51 test totali** (50 precedenti + 1 nuovo in `render.rs`).

---

## [0.7.0] — 2026-07-19 — `main.rs` — loop I/O completo

**Loop I/O finale:** `main.rs` sostituisce lo stub del Task 2. Shell I/O sottile
(stdin/stdout sincroni, JSON per riga): legge `HostToPlugin`, delega tutta la
logica di stato e dispatch a `state::handle(&mut CryptoState, msg)`, scrive le
`Vec<PluginToHost>` su stdout. Nessuna nuova logica — tutta la complessità
rimane in `state.rs` e nei moduli subordinati (`ciphers/`, `render.rs`,
`normalize.rs`), che sono testabili senza I/O reale. Eseguibile completo e
funzionante end-to-end, verificabile dal vivo dopo il Task 9. Pattern identico
a `plugin-lc`, scelto al Task 2 per gestire lo stato più complesso di questo
plugin.

---

## [0.6.0] — 2026-07-19 — HTML rendering finestra principale + dialog parametri

**Rendering reale di `render.rs`:** sostituisce il placeholder del Task 6.
Due funzioni pubbliche: `render_main_window()` (sidebar cifrari + due pannelli
testo chiaro/cifrato + riga toolbar "Parametri") e `render_params_dialog()`
(titolo cifrario, campi parametri del cifrario corrente, pulsanti Applica/Chiudi).
Campi dinamici prese direttamente da `CryptoState`: `current_cipher_index` per
il marker "selected", `plaintext`/`ciphertext` per il contenuto textarea,
`params_by_cipher` per i valori campo, `last_error` per il messaggio d'errore
opzionale. Layout CSS inline (classe `.crypto-root`, `.crypto-sidebar`,
`.crypto-panels`, etc.) — completamente indipendente dal catalogo host
(come `plugin-lc`), al fianco del quale si adatta facilmente.

**Visibilità campi CryptoState:** i campi `main_window_id`, `current_cipher_index`,
`plaintext`, `ciphertext`, `params_by_cipher`, `last_error` diventano
`pub(crate)` (visibili nel crate, non all'esterno), così come i metodi
`current_cipher()` e `current_params()`. Consente a `render.rs` di leggerli
direttamente senza getter uno per uno — pattern coerente con `plugin-lc`.
`last_edited` rimane privato (usato solo da `resync()`).

**HTML escape manuale:** funzione `html_escape()` sanitizza `&`, `<`, `>`, `"`
nel testo utente prima di inserirlo in attributi/content HTML. Il DOMPurify
lato host (`plugin-window.html`) è l'ultima linea di difesa, ma non ci si
affida SOLO a quella (stesso principio di `plugin-calc`).

8 nuovi test su render: markup dei tre cifrari, marker "selected", pannelli con
testo, messaggi d'errore, campo Spostamento Cesare, pulsante Genera RSA,
pulsanti Applica/Chiudi, prevenzione tag injection.

---

## [0.5.0] — 2026-07-19 — `CryptoState` + dispatch `HostToPlugin`

**Nuovo `state.rs`:** `CryptoState` (window_id principale, indice cifrario
selezionato, testo dei due pannelli chiaro/cifrato, `last_edited` per
sapere quale ri-cifrare su "Applica", `params_by_cipher` per-cifrario,
`last_error`) + `pub fn handle(&mut CryptoState, HostToPlugin) ->
Vec<PluginToHost>` — dispatch puro, nessun I/O, testabile senza
stdin/stdout reali. Smista `Init`/`Activate`/`UiEvent`/`Deinit`; `UiEvent`
si smista ulteriormente su `cipher-select:<id>` (nome cifrario
nell'`element_id`, mai in `value` — un `<div>` di sidebar non è
value-bearing), `param:<key>` (auto-salva ad ogni tasto, un INPUT è
value-bearing), `plaintext`/`ciphertext` (ri-cifra e aggiorna l'altro
pannello), `open-params`/`params-close`/`params-generate`/`params-apply`.
`DIALOG_WINDOW_ID_OFFSET = 1_000_000`: il `window_id` della dialog
parametri è sempre `main_window_id + 1_000_000`, namespace separato da
quello sequenziale dell'host. 12 nuovi test.

**Nuovo `render.rs`** (placeholder reale, non TODO): due funzioni che
ritornano HTML fisso minimo — sblocca la compilazione di `state.rs` senza
anticipare la logica di rendering vera (Task 7).

**Fix rispetto al codice del brief (necessario per compilare):**
`current_cipher()` era specificato con tipo di ritorno `&dyn Cipher`,
ottenuto indicizzando il `Vec` temporaneo di `all_ciphers_static()` — il
`Vec` viene distrutto a fine funzione, quindi il riferimento sarebbe stato
dangling (`E0515`), con un conflitto di borrow a cascata in `resync()`
(`E0506`, `self.last_error = None` dopo un prestito ancora vivo). Cambiato
il tipo di ritorno in `Box<dyn Cipher>` (owned, via
`.into_iter().nth(idx)`) — i cifrari sono unit struct, "ricostruirne uno"
è a costo pressoché nullo, e l'owned value non porta alcun prestito di
`self`, risolvendo entrambi gli errori. Nessun altro sito di chiamata ha
richiesto modifiche (`Box<dyn Cipher>` derefa automaticamente ai metodi
del trait).

`main.rs`: aggiunte `mod render;` e `mod state;`. Il dispatch runtime
resta quello inline del Task 2 — collegarlo a `state::handle` è
esplicitamente Task 8 (non in scope qui); di conseguenza `cargo build`
mostra warning `dead_code` estesi su tutto il modulo `ciphers` e su
`state.rs`, attesi e temporanei.

Suite crate a 42 test totali (30 precedenti + 12 nuovi).

---

## [0.4.0] — 2026-07-19 — RSA completo

**Implementazione RSA:** matematica da manuale in `ciphers/rsa_math.rs`
(modulo puro, nessuna dipendenza da `Cipher`) — Miller-Rabin (20 round) per
la primalità, generazione chiavi con `e = 65537` fisso e retry su
`gcd(e, φ(n)) != 1`, inverso modulare via Euclide esteso. `ciphers/rsa.rs`
sostituisce il placeholder del Task 3: `generate()` produce `(n, e, d)` in
esadecimale, `encode`/`decode` cifrano/decifrano a blocchi (testo
normalizzato → byte → `BigUint` → `modpow` → blocchi esadecimali separati
da spazio). Nuove dipendenze: `num-bigint` (con feature `rand`, necessaria
per `RandBigInt`/`gen_biguint*` — non indicata nel brief originale, aggiunta
per far compilare il codice esatto specificato), `num-traits`, `rand`.
12 nuovi test (7 `rsa_math` + 5 `rsa`), suite crate a 30 test totali.

**Nota di sicurezza (per design, non un difetto):** RSA da manuale per uno
strumento di studio — niente padding OAEP, niente aritmetica a tempo
costante. Non adatto a uso reale, non è lo scopo di questo plugin.

---

## [0.3.0] — 2026-07-19 — Vigenère completo

**Implementazione Vigenère:** sostituzione polialfabetica, shift per-lettera
da parola chiave ripetuta ciclicamente. Algoritmo corretto verificato dal
vettore di test standard ("ATTACKATDAWN"+"LEMON"→"LXFOPVEFRNHR"). Round-trip
encode/decode, normalizzazione della keyword (solo A-Z, errore se vuota dopo
normalizzazione), ripetizione ciclica della chiave. 6 test totali.

---

## [0.2.0] — 2026-07-19 — Trait `Cipher` + registro + Cesare

**Sistema portante:** trait `Cipher` (`id`, `display_name`, `params`, `encode`,
`decode`, `generate` opzionale); enum `ParamField` (Number, Text,
GeneratedPair); registro statico `all_ciphers()` → `Vec<Box<dyn Cipher>>`.

**Cesare completo:** Implementazione reale (shift A-Z con `rem_euclid`),
7 test (forward/backward, wraparound, identity, zero shift, edge cases,
default param).

**Placeholder reali:** Vigenère e RSA compilano subito (non TODO), logica
completa nei Task 4 e 5. RSA dichiara `GeneratedPair` per il pulsante "Genera".

---

## [0.1.0] — 2026-07-19 — scaffold iniziale

Crate creato (mirror di `plugin-lc`: shell I/O sottile, logica delegata a
`state::handle` — vedi Task 6). `normalize()` (maiuscolo, solo A-Z) — regola
condivisa dai 3 cifrari (Docs/superpowers/specs/2026-07-19-crypto-plugin-design.md).
`main.rs` gestisce Init/Activate/UiEvent/Deinit minimi — finestra reale nei
task successivi.
