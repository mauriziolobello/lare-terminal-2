# Implementation — crates/plugin-crypto v1.0.1

## Clamp difensivo su RSA bits + version drift plugin.json (v1.0.1)

**`ciphers/rsa_math.rs::generate_keypair`**: `bits` viene clampato a
`bits.max(16)` come prima operazione della funzione, prima di dividerlo per
2 e passarlo a `generate_prime`. Bug reale (non ipotetico) trovato in
review finale: il campo "Bit chiave" della dialog parametri è un `<input>`
libero — `min: 512, max: 4096` dichiarati in `ParamField::Number` non sono
mai applicati né dal rendering (`render.rs` non emette gli attributi HTML
`min`/`max`) né da `Rsa::generate` (`bits` viene solo parsato, mai
clampato). Con `bits` piccolo passato dall'utente:
- `generate_keypair(4)` → `generate_prime(2)`: `gen_biguint(2)` genera un
  valore a 2 bit, poi `set_bit(bits-1=1, true)` e `set_bit(0, true)`
  saturano ENTRAMBI i bit disponibili — il candidato finale è
  deterministicamente sempre `3`, ad ogni chiamata. `generate_prime(2)`
  ritorna quindi sempre `3` (è primo, nessun loop qui).
- Conseguenza in `generate_keypair`: `p` e `q` valgono SEMPRE `3` (stessa
  chiamata deterministica) → `if p == q { continue }` non esce MAI dal
  loop. Hang reale del processo sidecar, non un caso limite raro.
- `bits=0` sarebbe ancora peggio: `set_bit(bits-1, ..)` = `set_bit(u64::MAX,
  ..)`, tentativo di allocazione enorme invece di un loop infinito.

Fix minimo, difesa in profondità (non tocca la UI/dropdown, che resta un
miglioramento futuro se si vuole un vero `min`/`max` HTML):
```rust
pub fn generate_keypair(bits: u64) -> RsaKeyPair {
    let bits = bits.max(16); // difesa: bit-length troppo piccoli causano loop infinito in generate_prime
    ...
```
16 bit totali (8 per primo) è il minimo che garantisce a Miller-Rabin uno
spazio di candidati sufficiente a terminare velocemente, ben al di sotto
di qualunque valore ragionevole (i default UI partono da 512) quindi non
cambia comportamento per l'uso normale.

**Test nuovo** (`ciphers/rsa_math.rs`, dentro `#[cfg(test)] mod tests`):
`generate_keypair_with_tiny_bits_does_not_hang` — chiama
`generate_keypair(4)` e asserisce `kp.n.bits() >= 14` (tolleranza di 2 bit
sul minimo garantito di 16, stesso pattern del test preesistente
`keypair_modulus_is_product_of_two_distinct_primes` che usa `TEST_BITS -
2`; 14 invece di 16 perché due primi di 8 bit possono produrre un prodotto
di 15 bit nel caso più piccolo, es. 131×137=17947). **RED verificato dal
vivo**: prima del fix, `cargo test -p plugin-crypto
generate_keypair_with_tiny_bits_does_not_hang` non terminava — compilava,
stampava `running 1 test` e restava appeso indefinitamente (osservato con
timeout di 20s, nessun output oltre "running 1 test"), coerente con
l'analisi del loop infinito sopra. GREEN dopo il fix: passa in pochi
millisecondi insieme al resto della suite.

**`plugin.json`**: `"version"` era rimasto a `"0.1.0"` (valore del Task 2 /
scaffold) mentre `Cargo.toml` era già a `1.0.0`. Il campo non è consumato
funzionalmente da nessun codice (solo parsing all'attivazione), ma restava
una vetrina fuorviante per un crate 1.0.0. Allineato a `1.0.1` (versione di
questo stesso rilascio, non `1.0.0`, per non reintrodurre drift nello
stesso commit che bump-a `Cargo.toml`).

**Verifica:** `cargo test -p plugin-crypto` → 52/52 verdi (51 precedenti +
1 nuovo). `cargo build` (workspace default-members) pulito. `cargo clippy
--all-targets` senza warning nuovi attribuibili a questo crate (i warning
preesistenti sono tutti in `orchestrator`, file non toccati da questo fix).

**Suite crate: 52 test totali** (51 precedenti + 1 nuovo in `rsa_math.rs`).

## Primo rilascio funzionalmente completo (v1.0.0)

Piano `Docs/superpowers/plans/2026-07-19-crypto-plugin.md` (12 task)
completato. Smoke test dal vivo (due giri, Task 9) con orchestrator + UI
Tauri reali, `/crypto`:

**Primo giro** — 3 bug reali trovati, nessuno rilevabile dai 51 test
unitari (asseriscono tutti su `Vec<PluginToHost>`, mai su un round-trip
renderizzato in una finestra vera):
- Pannelli testo minuscoli — CSS `.crypto-root { height: 100% }` risolveva
  contro `.lare-window` (host), che non ha altezza esplicita in
  quest'architettura "auto-fit al contenuto" → fix Task 10 (`min-height`
  esplicito sulle textarea).
- Testo digitato sfarfallava, un carattere alla volta — ogni tasto fa un
  round-trip completo attraverso il sidecar (full-HTML-replace, nessun
  patching parziale); il meccanismo di preservazione focus/cursore
  esistente in `plugin-window.js` non preservava il `.value` → fix
  piattaforma Task 11 (`restoreActiveField` in `plugin-runtime.mjs`,
  riusabile da ogni plugin futuro con digitazione live).
- Pulsanti Applica/Chiudi della dialog irresponsivi — diagnosticato con un
  test e2e diagnostico (crypto.exe reale attraverso `PluginHost` reale,
  bypassando Tauri/JS): **il backend era corretto**, causa mai isolata con
  certezza — risolto (verosimilmente) come side-effect del fix Task 10 sul
  layout, non investigato oltre dato che il sintomo è sparito nel secondo
  giro.

**Secondo giro** — Task 10/11 confermati funzionanti dal vivo; 3 item di
solo stile trovati (pannelli/campi a sfondo bianco, pulsanti senza aspetto
di pulsante) → fix Task 12 (classi condivise `.lare-input`/`.lare-button`
dal catalogo host). Terzo giro: tutto confermato, incluso il requisito
esplicito che ha motivato il fix orchestrator del Task 1 — la dialog
parametri è una vera seconda finestra Tauri, spostabile indipendentemente
dalla finestra principale.

Nessuna riga di logica cambiata in questa entry — segna la chiusura del
piano. Suite invariata: 51 test.

## Classi CSS condivise input/pulsanti (v0.7.2)

**`render.rs` applica classi CSS del catalogo host:** Task 12 integra il
plugin con le classi `.lare-input` e `.lare-button` già definite in
`crates/ui/frontend/plugin-catalog.css` (caricate automaticamente in ogni
finestra plugin), uniformando lo stile della UI all'app e al tema scuro:

- **`.lare-input`** (sfondo #222/bordo #666/colore #f0f0f0, font 13px,
  `width:100%; box-sizing:border-box`, padding uniforme) applicata a:
  - Due `<textarea data-evt="plaintext">` e `<textarea data-evt="ciphertext">`
    in `render_main_window()` (riga 44, 48).
  - Tre `<input>` nella dialog parametri: nel ramo `Number` (riga 117),
    nel ramo `Text` (riga 126), nel ramo `GeneratedPair` readonly (riga 138).
  - Nessun nuovo CSS aggiunto a `INLINE_CSS` — la classe esiste già nel catalogo,
    solo applicata agli elementi. Non dichiara altezza, si combina senza conflitto
    con `.crypto-panel textarea { min-height:240px; ... }` pre-esistente
    (l'altezza esplicita rimane, `.lare-input` non la sovrascrive).

- **`.lare-button`** (bordo 1px #888, padding 4px 8px, `display:inline-flex`,
  hover su `#bbb`, cursor pointer) applicata a:
  - `<span data-evt="open-params">⚙ Parametri</span>` nella toolbar della
    finestra principale (riga 39).
  - Tre `<span>` pulsanti della dialog parametri: `params-apply` (riga 104),
    `params-close` (riga 105), `params-generate` (riga 145).
  - Stesso difetto dello user report (textare/input bianci senza `.lare-input`):
    gli span non avevano bordo/sfondo/hover, rendendo indistinguibili dai
    link. Ora coerenti nel tema.

**Verifica:** nessun test modificato — gli assert di `render.rs` non facevano
match esatto sul markup HTML dei tag (nessun
`assert!(html.contains("<input ...exact string...>"))`), solo verificavano
la presenza di attributi/stringhe (es. `"params-apply"`, `"Cesare"`).
La build e test locali green.

**Suite crate:** 51 test totali (invariati).

## Fix CSS layout textarea (v0.7.1)

**`render.rs` fix CSS Task 10:** `.crypto-panel textarea` modifica dal
precedente `flex: 1` a `min-height: 240px; width: 100%; box-sizing:
border-box;`. Il valore `flex: 1` non ha una base verticale da cui ereditare
(l'architettura delle finestre è auto-fit al contenuto) — le textarea
collassavano al default minuscolo del browser. Fix esplicita l'altezza in px,
coerente col resto della codebase (`plugin-catalog.css` usa `min-height:
56px` sugli elementi di catalogo). Layout `.crypto-panels` reste `display:
flex; flex-direction: column` — la disposizione verticale tra label e
textarea non cambia, solo l'altezza textara resa esplicita.

**Test nuovo:** `textarea_has_explicit_min_height()` in `render.rs` asserisce
che il CSS ritornato da `render_main_window` contenga la stringa `"min-height"`
— verifica automatizzabile della presenza dell'altezza esplicita nel blocco
`<style>`.

**Suite crate:** 51 test totali (50 precedenti + 1 nuovo in `render.rs`).

## Loop I/O finale `main.rs` (v0.7.0)

**`main.rs`** (sostituisce lo stub del Task 2, implementazione completa del
loop I/O): shell I/O sottile (stdin/stdout sincroni, JSON per riga).

- **Input**: legge linee da stdin, parse come `HostToPlugin` (serde_json).
- **Dispatch**: crea e mantiene una singola istanza di `CryptoState::default()`
  (una per sessione), delega ogni messaggio a `state::handle(&mut crypto_state, msg)`.
- **Output**: scrive il vettore di `PluginToHost` ritornato, una risposta per
  riga (JSON serializzato via `send()`, con flush dopo ogni messaggio).
- **Ciclo**: continua fino a `HostToPlugin::Deinit`, dopodichè esce.

**Nessuna logica di stato o dispatch inline**: il match sulla variante di
`HostToPlugin` (`Init`, `Activate`, `UiEvent`, `Deinit`) è delegato
completamente a `state::handle`. Il modulo `state.rs` è responsabile di:
- Inizializzazione (`Init` → `Ready`).
- Attivazione e rendering di finestre (`Activate` → `ShowWindow`).
- Gestione degli eventi UI (`UiEvent` → smistamento su elemento, modifica
  stato, ri-render).
- Chiusura pulita (`Deinit`).

Il loop stesso è una semplice "pompa di messaggi" che coordina I/O e stato
persistente — un pattern identico a `plugin-lc`, scelto al Task 2 per gestire
lo stato più complesso di questo plugin rispetto a `plugin-calc`.

**`send()` helper**: serializza un `&PluginToHost` e lo scrive su stdout con
newline, garantendo flush. Qualunque errore di I/O (stderr chiuso, pipe
rotta) è ingoiato silenziosamente (`let _ = ...`).

**Test**: nessun nuovo test specifico di `main.rs` (i 50 test precedenti di
`state.rs`, `render.rs`, `ciphers.rs` e `normalize.rs` restano tutti verdi).
La verifica della corretta integrazione è il test manuale facoltativo via
stdin (Step 3 del brief): `'{"type":"Init",...}' | crypto.exe` → output
`{"type":"Ready",...}`.

Suite crate: **50 test totali** (invariati dai task precedenti; nessuna
nuova logica, solo collegamento del loop I/O).

## HTML rendering finestra principale + dialog parametri (v0.6.0)

**`render.rs`** (sostituisce il placeholder del Task 6, implementazione completa):
due funzioni pubbliche che generano HTML per le finestre, nessun template engine
— pura concatenazione di stringhe con `format!` (pattern di `plugin-calc`,
`plugin-lc`).

- **`render_main_window(state: &CryptoState) -> String`**: sidebar cifrari +
  due pannelli testo + toolbar "Parametri".
  - **Sidebar dinamica**: `all_ciphers()` in loop, ogni cifrario diventa un
    `<div class="crypto-sidebar-item">` con `data-evt="cipher-select:{id}"`.
    Il cifrario corrente (indice in `state.current_cipher_index`) riceve la
    classe CSS `.selected` (sfondo traslucido).
  - **Pannelli testo**: due `<textarea data-evt="plaintext">` e
    `data-evt="ciphertext">`, riempiti dai valori in `state.plaintext` e
    `state.ciphertext` (escapati manualmente).
  - **Errore**: se `state.last_error.is_some()`, mostra `<div
    class="crypto-error">` con il messaggio; altrimenti niente.
  - **CSS inline**: `INLINE_CSS` costante che definisce il layout flex
    (sidebar stretta, i due pannelli al fianco uguali), colori neutri
    (`--border`), altezza 100%, textarea senza resize.

- **`render_params_dialog(state: &CryptoState) -> String`**: dialog parametri
  del cifrario corrente.
  - **Titolo**: `<h3>{display_name del cifrario corrente}</h3>`.
  - **Campi**: `state.current_cipher().params()` genera i campi via
    `render_param_field()` (helper interno).
  - **Errore**: stessa logica di `render_main_window`.
  - **Pulsanti**: `<span data-evt="params-apply">Applica</span>` e
    `<span data-evt="params-close">Chiudi</span>` — pulsanti senza `type`
    (sono `<span>`, non `<button>`; il runtime host li interpreta come
    evento, non come form submit).

- **`render_param_field(field: &ParamField, params: &ParamValues) -> String`**
  (helper): smista su 3 varianti di `ParamField`:
  - `Number { key, label, default, .. }`: `<input>` editabile, valore da
    `params.get(key)` o `default.to_string()`.
  - `Text { key, label }`: `<input>` editabile, valore da `params.get(key)`.
  - `GeneratedPair { action_label, fields }`: `<span data-evt="params-generate">`
    (pulsante), seguito da `<input readonly>` per ogni campo della coppia
    (chiave pubblica/privata RSA). I readonly non ricevono più input da
    keyboard (il browser inibisce edit), mostrati dopo "Genera".

- **`html_escape(s: &str) -> String`**: sanitizzazione minima prima di
  inserire user input in HTML — sostituisce `&` → `&amp;`, `<` → `&lt;`,
  `>` → `&gt;`, `"` → `&quot;`. È la prima linea di difesa lato plugin;
  DOMPurify lato host (`plugin-window.html`) è la seconda — nessun
  affidarsi a SOLO una (pattern di `plugin-calc`).

**Visibilità dei campi `CryptoState`** (`state.rs`): i campi
`main_window_id`, `current_cipher_index`, `plaintext`, `ciphertext`,
`params_by_cipher`, `last_error` diventano `pub(crate)` (visibili dal crate,
non esternamente). Anche i metodi `current_cipher()` e `current_params()`
diventano `pub(crate)`, così `render.rs` li legge direttamente senza getter
uno per uno — coerente con `plugin-lc` (`LcState` ha campi `pub(crate)`,
letti direttamente dal suo `render.rs`). Il campo `last_edited` rimane
privato (usato solo da `resync()`).

**Test (8 nuovi in `render.rs`):**
- `main_window_shows_all_three_ciphers_in_sidebar`: verifica "Cesare",
  "Vigenère", "RSA" nel markup.
- `main_window_marks_current_cipher_as_selected`: con `current_cipher_index = 1`,
  attende `selected" data-evt="cipher-select:vigenere"`.
- `main_window_shows_current_panel_text`: `state.plaintext = "ABC"`,
  `state.ciphertext = "DEF"` → markup contiene entrambi (in `textarea`).
- `main_window_shows_error_when_present`: se `last_error = Some("errore")`,
  markup contiene il messaggio.
- `params_dialog_shows_caesar_shift_field`: Cesare di default ha campo
  "Spostamento" → atteso nel markup.
- `params_dialog_shows_rsa_generate_button`: RSA (index 2) mostra "Genera" e
  `data-evt="params-generate"`.
- `params_dialog_has_apply_and_close_buttons`: sempre presenti
  `params-apply` e `params-close` nel markup.
- `html_escape_prevents_raw_tag_injection`: `state.plaintext = "<script>"` →
  markup non contiene `<script>` grezzo, ma contiene `&lt;script&gt;`.

Suite crate: 50 test totali (42 precedenti in `state.rs`/`ciphers.rs` + 8 nuovi
in `render.rs`).

## `CryptoState` + dispatch `HostToPlugin` (v0.5.0)

**`state.rs`** (nuovo): stato del plugin + dispatch puro
(Docs/superpowers/specs/2026-07-19-crypto-plugin-design.md §7-§8).
`main.rs` resta uno shell I/O sottile (per ora nel senso letterale: la sua
logica di dispatch inline del Task 2 non è ancora sostituita — quel
collegamento è Task 8); tutta la logica nuova vive qui, testabile senza
stdin/stdout reali (stesso pattern di `plugin-lc`).

- **`CryptoState`**: `main_window_id: Option<u64>` (assegnato da
  `Activate`, `None` prima), `current_cipher_index: usize` (indice in
  `all_ciphers()`), `plaintext`/`ciphertext: String` (i due pannelli
  fissi), `last_edited: LastEdited` (`Plaintext`/`Ciphertext` — quale dei
  due è stato toccato per ultimo, usato da "Applica" per sapere quale
  ri-cifrare), `params_by_cipher: HashMap<String, ParamValues>` (chiave =
  `Cipher::id()`, così cambiare cifrario nella sidebar non perde i
  parametri già impostati per gli altri), `last_error: Option<String>`
  (mostrato al posto del pannello di destinazione quando encode/decode
  fallisce).
- **`current_cipher(&self) -> Box<dyn Cipher>`**: legge l'indice corrente
  da `all_ciphers()` e ne estrae un `Box<dyn Cipher>` **owned** (non un
  riferimento — vedi "Deviazione dal brief" sotto).
- **`current_params(&self) -> ParamValues`**: valori correnti per il
  cifrario selezionato, o default vuoto.
- **`dialog_window_id(&self) -> Option<u64>`**: `main_window_id +
  DIALOG_WINDOW_ID_OFFSET` (`1_000_000`) — namespace separato da quello
  sequenziale assegnato dall'host per ogni `Activate`, per non collidere
  mai (si appoggia al fix orchestrator del Task 1 di questo piano, che
  rende funzionante una seconda `ShowWindow` con `window_id` inventato dal
  plugin).
- **`resync(&mut self)`**: ri-cifra/decifra in base a `last_edited` —
  `Plaintext` → normalizza e chiama `encode`, aggiornando `ciphertext` (o
  `last_error`); `Ciphertext` → chiama `decode` direttamente (il testo
  cifrato non passa da `normalize`, è già nel formato del cifrario, es.
  esadecimale per RSA), aggiornando `plaintext` (o `last_error`).

**`pub fn handle(&mut CryptoState, HostToPlugin) -> Vec<PluginToHost>`**:
dispatch puro, zero I/O.
- `Init` → `Ready { name: "crypto", protocol_version: 1 }`.
- `Activate { window_id, .. }` → salva `window_id` come `main_window_id`,
  `ShowWindow` con `render::render_main_window`.
- `UiEvent { window_id, element_id, value }` → `handle_ui_event`.
- `Deinit` → nessuna risposta.

**`handle_ui_event`** smista su `element_id`:
- **`cipher-select:<id>`** (prefisso): il nome del cifrario è
  nell'`element_id` stesso, mai in `value` — un `<div>` di sidebar non è
  "value-bearing" per il runtime host (solo INPUT/SELECT/TEXTAREA lo sono,
  fix Task 1b), quindi `value` è sempre `None` per questo evento nella
  realtà; il dispatch non lo consulta affatto. Aggiorna
  `current_cipher_index` se l'id è trovato in `all_ciphers()`, azzera
  `last_error`, ri-renderizza la finestra principale. Il testo dei due
  pannelli NON viene toccato (cambiare cifrario non perde quanto scritto).
- **`param:<key>`** (prefisso): un campo della dialog parametri è un
  INPUT, quindi value-bearing — arriva con `value: Some(...)` ad ogni
  tasto (evento "input" del browser), non solo al click su "Applica".
  Salva subito in `params_by_cipher` per il cifrario corrente; nessun
  re-render dei pannelli (ritorna `vec![]`) finché non si preme "Applica".
- **`plaintext`/`ciphertext`**: aggiorna il pannello, imposta
  `last_edited`, chiama `resync()`, ri-renderizza la finestra principale.
- **`open-params`**: apre la dialog parametri con `dialog_window_id()`
  (nessuna azione se `main_window_id` è ancora `None`, cioè prima di
  `Activate`).
- **`params-close`**: `CloseWindow` per il `window_id` della dialog
  stessa (quello ricevuto nell'evento, non `dialog_window_id()` —
  simmetria con l'host, che manda gli eventi della dialog col suo proprio
  `window_id`).
- **`params-generate`**: chiama `Cipher::generate` (RSA genera una nuova
  coppia di chiavi; Cesare/Vigenère non sovrascrivono il default e
  ritornano `Err`, la UI non mostra comunque il pulsante per loro),
  estende `params_by_cipher` per il cifrario corrente coi valori generati,
  ri-renderizza SOLO la dialog (`UpdateWindow` sul suo `window_id`).
- **`params-apply`**: nessun valore proprio (è un pulsante, non
  value-bearing) — chiama `resync()` usando i `params_by_cipher` già
  salvati dai singoli eventi `param:*`, poi ri-renderizza SIA la finestra
  principale SIA la dialog (il pannello aggiornato e un'eventuale
  segnalazione errore vivono in finestre diverse).
- Qualunque altro `element_id` → `vec![]` (nessuna azione).

**`render.rs`** (nuovo, placeholder reale non TODO): due funzioni,
`render_main_window`/`render_params_dialog`, che ritornano HTML fisso
minimo. Sblocca la compilazione di `state.rs` (che le referenzia) senza
anticipare la vera logica di rendering, prevista al Task 7.

**`main.rs`**: aggiunte `mod render;` e `mod state;`. Il ciclo
Init/Activate/UiEvent/Deinit del Task 2 resta inline (non chiama ancora
`state::handle`) — il collegamento è esplicitamente Task 8, non in scope
qui. Conseguenza attesa: `cargo build -p plugin-crypto` mostra warning
`dead_code` estesi su tutto `ciphers::*` e su `state.rs` (33 warning),
perché nulla nel percorso di esecuzione reale li usa ancora; spariscono
quando Task 8 collega il runtime a `state::handle`. `cargo test` non li
mostra (i test di `state.rs` usano tutto il percorso).

### Deviazione dal brief (necessaria per compilare)

Il brief specifica `current_cipher(&self) -> &dyn Cipher` implementato
come `all_ciphers_static()[self.current_cipher_index].as_ref()`. Questo
codice non compila: `all_ciphers_static()` ritorna un `Vec<Box<dyn
Cipher>>` locale alla funzione, distrutto alla fine dell'espressione — il
riferimento preso al suo interno sarebbe dangling (`E0515`, "cannot
return value referencing temporary value"). L'errore si propagava anche a
`resync()`: `let cipher = self.current_cipher();` avrebbe tenuto un
prestito di `self` per tutta la funzione (via l'elisione del lifetime sul
tipo di ritorno), in conflitto con `self.last_error = None;` subito dopo
(`E0506`).

Fix: tipo di ritorno cambiato in `Box<dyn Cipher>` (owned), ottenuto con
`all_ciphers_static().into_iter().nth(self.current_cipher_index).expect(...)`
invece di indicizzare e prendere un riferimento. I cifrari (`Caesar`,
`Vigenere`, `Rsa`) sono unit struct a costo zero — "ricrearne uno" ad ogni
chiamata non alloca nulla di sostanziale, solo la `Box` che già serviva.
`Box<dyn Cipher>` deref-coercisce automaticamente ai metodi del trait
(`.id()`, `.display_name()`, `.encode()`, ecc.), quindi nessun altro sito
di chiamata in `state.rs` ha richiesto modifiche. Nessun test del brief
modificato — tutti e 12 passano invariati col fix.

### Nota sul conteggio dei test

Il brief indicava "10 nuovi test" nello Step 3, ma il blocco `#[cfg(test)]`
fornito contiene 12 funzioni di test. Implementato il codice esatto del
brief (12 test, tutti verdi) — la discrepanza è nel testo descrittivo del
brief, non nel codice sorgente che definisce la spec.

**Test totali del crate: 42** (30 precedenti + 12 nuovi in `state.rs`,
verificato con `cargo test -p plugin-crypto`).

## RSA completo (v0.4.0)

**`ciphers/rsa_math.rs`** (nuovo, modulo privato `mod rsa_math;` dichiarato
in `ciphers/mod.rs` — dettaglio implementativo di `rsa.rs`, non parte
dell'API pubblica del registro): matematica RSA da manuale, pura (nessuna
dipendenza da `Cipher`, testabile in isolamento). Costruita su `num-bigint`
(`BigUint`/`BigInt`) e `num-traits` (`One`/`Zero`):
- `generate_keypair(bits) -> RsaKeyPair` (`{n, e, d}`): `e = 65537` fisso
  (scelta standard). Genera `p`/`q` da `bits/2` bit ciascuno, rigenera la
  coppia finché `p != q` e `gcd(e, φ(n)) == 1` (necessario per
  l'invertibilità di `e` modulo `φ(n)` — collisione rarissima con `e`
  fisso e primi grandi, ma gestita, non ignorata).
- `generate_prime(bits)`: candidato casuale con bit più significativo e
  bit meno significativo forzati a 1 (lunghezza esatta + disparità),
  ripete finché `is_probably_prime`.
- `is_probably_prime(n, rounds)`: Miller-Rabin, 20 round in produzione
  (probabilità di falso positivo trascurabile, 4^-20). Scompone
  `n - 1 = d * 2^r`, testa `rounds` basi casuali.
- `mod_inverse(a, m)`: inverso modulare via Euclide esteso
  (`extended_gcd` ricorsivo su `BigInt` con segno); `None` se
  `gcd(a, m) != 1`. 7 test: modulo/esponente della keypair, round-trip
  cifra/decifra puro (`modpow`), primi/composti noti, inverso modulare
  noto e caso non invertibile.

**`ciphers/rsa.rs`** (sostituisce il placeholder del Task 3): implementa
`Cipher` a chiave pubblica/privata (asimmetrico, a differenza di
Cesare/Vigenère — `encode` usa `(n, e)`, `decode` usa `(n, d)`).
- `params()`: `Number` per i bit chiave (512-4096, default 2048) +
  `GeneratedPair` con 3 campi di sola lettura (`pub_n`, `pub_e`, `priv_d`)
  dietro il pulsante "Genera".
- `generate(params)`: legge `bits` (default 2048 se assente/non
  parsabile), chiama `rsa_math::generate_keypair`, serializza `n`/`e`/`d`
  in esadecimale (`to_str_radix(16)`) nei `ParamValues` di ritorno.
- `encode`/`decode`: leggono i parametri chiave via `get_biguint_param`
  (parse esadecimale, `None` se assente/vuoto/non valido → errore
  "genera prima una coppia di chiavi"). Il testo in chiaro passa da
  `normalize::normalize` prima di `encrypt_blocks` (garantisce solo A-Z
  ASCII, mai byte `0x00` — nessun blocco perde uno zero iniziale nella
  conversione byte→`BigUint`→byte, che altrimenti tronca gli zeri guida).
- `encrypt_blocks`: spezza il testo in chunk di `(bit(n) - 1) / 8` byte
  (strettamente più piccoli del modulo, garantisce `m < n`), ogni chunk →
  `BigUint::from_bytes_be` → `modpow(e, n)` → esadecimale, blocchi uniti
  da spazio (il cifrato non è testo stampabile, l'esadecimale evita
  ambiguità di encoding).
- `decrypt_blocks`: inverso — ogni blocco esadecimale → `BigUint` →
  `modpow(d, n)` → byte, concatenati e riconvertiti a UTF-8 (errore se
  non valido). 5 test: generazione popola i 3 campi, round-trip
  encode/decode ("HELLO WORLD" → normalizzato "HELLOWORLD" dopo
  decifratura, gli spazi sono rimossi da `normalize`), errore senza
  chiave pubblica generata, errore senza chiave privata, cifrato composto
  solo da cifre esadecimali e spazi.

**Dipendenze nuove** (`Cargo.toml`): `num-bigint` (con feature `rand` —
necessaria per il trait `RandBigInt`/i metodi `gen_biguint`/
`gen_biguint_range` su `ThreadRng`; il brief originale non la indicava,
aggiunta per far compilare il codice esatto specificato — vedi
`.superpowers/sdd/task-5-report.md`), `num-traits`, `rand = "0.8"`.

**Nota di sicurezza (per design, ripetuta qui perché rilevante — vedi
Docs/superpowers/specs/2026-07-19-crypto-plugin-design.md §5):** RSA da
manuale per uno strumento di studio. Niente padding OAEP (i blocchi sono
`modpow` diretto sul testo normalizzato), niente aritmetica a tempo
costante (`num-bigint` non garantisce timing costante). Non è per uso
reale — è il punto del plugin, non una lacuna.

**Test totali del crate: 30** (5 normalize + 7 caesar + 6 vigenere +
7 rsa_math + 5 rsa = 30, verificato con `cargo test -p plugin-crypto`).

## Trait `Cipher` + registro (v0.2.0)

**`ciphers/mod.rs`**: Definisce il trait `Cipher` (interface per ogni
cifrario). Metodi core:
- `id()` → identificatore stabile (routing UiEvent).
- `display_name()` → nome visualizzato nella sidebar.
- `params()` → descrizione dei campi della dialog parametri.
- `encode(plaintext, params)` → testo cifrato (input già normalizzato).
- `decode(ciphertext, params)` → testo in chiaro.
- `generate(params)` → azione opzionale per generare chiavi (RSA).

**`ParamField` enum**: Number (min/max/default), Text, GeneratedPair
(pulsante + campi sola lettura).

**`all_ciphers()` registro**: Funzione statica che ritorna tutti i cifrari
disponibili in ordine di visualizzazione. Estensibile senza cambiare UI/routing.

**`ciphers/caesar.rs`**: Cesare completo, shift A-Z con `rem_euclid` modulo 26.
Helper: `shift_param()` (legge/default il parametro "shift"), `shift_text()`
(mappa su caratteri), `shift_char()` (logica di spostamento puro). 7 test.

**`ciphers/vigenere.rs`** (v0.3.0 completo): Sostituzione polialfabetica,
shift per-lettera dal ripetimento ciclico della keyword normalizzata.
Helper: `normalized_keyword()` (estrae/valida la chiave, errore se vuota dopo
normalizzazione A-Z), `apply()` (loop enum sui char, mappa su shift via chiave
ripetuta), `shift_char()` (stessa logica di caesar, duplicata intenzionalmente
per non creare astrazione sottodimensionata — YAGNI). 6 test: round-trip,
vettore di test standard (ATTACKATDAWN+LEMON→LXFOPVEFRNHR), ciclo della chiave,
errori di keyword, normalizzazione.

**`ciphers/rsa.rs`**: Placeholder reale compilato. Dichiara Number ("bits")
e GeneratedPair per il pulsante "Genera". `encode`/`decode` non implementati
(Task 5).

## Scaffold iniziale (v0.1.0)

Mirror di `plugin-lc`, non `plugin-calc`: `main.rs` è pensato per restare
uno shell I/O sottile che delega a `state::handle(&mut CryptoState,
HostToPlugin) -> Vec<PluginToHost>` puro (arriva al Task 6) — scelto per lo
stato più complesso di plugin-calc (due finestre, cifrario corrente,
parametri per-cifrario, chiavi RSA generate).

`normalize()` (`src/normalize.rs`): maiuscolo, solo A-Z ASCII — spazi,
punteggiatura, accenti, numeri rimossi (non traslitterati). Punto unico
condiviso dai 3 cifrari (Task 3-5) — se la regola cambia in futuro, un solo
posto da toccare.
