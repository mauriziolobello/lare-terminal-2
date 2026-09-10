# Compito per AI esterna — Calcolatrice: modalità programmatore (hex/oct/bin, bitwise, shift/rotate, larghezza bit)

> **Prima cosa**: leggi per intero `Docs/i18n/ita/BRIEFING-AI-ESTERNE.md` alla radice del
> repository, poi questo file per intero, PRIMA di scrivere codice. **Crea la tua worktree
> dedicata come primissima cosa** (`git worktree add .worktrees/calc-programmatore -b
> feat/calc-programmatore`), come impone BRIEFING-AI-ESTERNE.md §1/§6 — Maurizio l'ha chiesto
> esplicitamente anche qui, oltre che nella regola generale.

## Contesto — cosa esiste oggi

`crates/plugin-calc` è la calcolatrice (plugin sidecar, HTML generato lato Rust). Quattro file:
`src/engine.rs` (tokenizer + parser a discesa ricorsiva + valutatore, tutto su `f64`),
`src/format.rs` (formattazione numero → stringa display, SOLO decimale), `src/render.rs`
(AST → HTML 2D: frazioni impilate per `÷`, apici per `^`), `src/main.rs` (`CalcState`,
`handle_key`, `key_grid`, `render_window`). **Leggi tutti e 4 i file per intero prima di
iniziare** — questo compito estende pattern già presenti (vedi sotto), non li inventa da zero.

Precedente diretto da studiare: `AngleMode` (Deg/Rad). È uno stato in `CalcState`, cambiato dal
tasto `"mode"` (`data-evt="mode"`), che influenza SOLO `evaluate()` — il buffer/tokenizer non lo
conoscono affatto. Questo compito introduce DUE nuovi stati dello stesso tipo, `NumBase`
(Dec/Hex/Oct/Bin) e `BitWidth` (Byte/Word/Dword/Qword), con una differenza importante: influenzano
anche il **tokenizer** (che cifre sono valide) e il **formatter** (come si scrive il risultato).

## Cosa vuole Maurizio (richiesta originale + due chiarimenti successivi)

Tasti hex (A-F), conversione bin/hex/oct/dec, tasti AND/OR/XOR/NOT, bit shift sinistra/destra,
le stesse operazioni aritmetiche (+ − × ÷) su numeri in basi diverse da 10. **Al massimo due
funzioni per tasto**, usando il tasto `2nd` già esistente (mai 3+ funzioni sullo stesso tasto come
una calcolatrice fisica).

**Chiarimento 1 — visibilità della sezione programmatore**: NON fissa/sempre visibile. Un
controllo dedicato (non un tasto della griglia 5 colonne — vedi §7) mostra/nasconde la sezione.
Di default nascosta: la calcolatrice si presenta esattamente come nello screenshot che Maurizio ha
allegato (nessun cambiamento visivo per chi non tocca la funzione).

**Chiarimento 2 — le 3 idee inizialmente proposte come "rimandate"**: Maurizio ha chiesto di NON
rimandarle. Sono quindi **dentro lo scope di questo compito**: selettore larghezza bit
(BYTE/WORD/DWORD/QWORD), rotazione bit (ROL/ROR), raggruppamento cifre nel display (leggibilità).
Il raggruppamento, nella forma in cui l'avevo scartato (spazi), rompeva un invariante testato
altrove in questo crate (round-trip buffer → parser); più sotto (§6) la versione corretta
(separatore `_`, che il tokenizer sa ignorare — stesso principio dei separatori di cifre `1_000`
di molti linguaggi) che non ha questo problema.

## Decisioni di design — vincolanti, verificate con due giri di revisione prima di scrivere

Il primo abbozzo di questo compito aveva 4 bug bloccanti (collisione simboli/cifre esadecimali,
comportamento sbagliato al cambio base, display 2D scorretto su buffer non-decimale, invariante
round-trip rotto dal raggruppamento). Il secondo giro (dopo l'estensione a larghezza bit +
rotazione) ne ha trovati altri 3 (range di validità dello shift dipendente dalla larghezza,
mascheratura del risultato NON coerente fra Dec e le altre basi, separatore `_` che doveva
escludere solo Dec). Le decisioni sotto sono il risultato finale — seguile esattamente; dove serve
un dettaglio non specificato, usa il tuo giudizio ma documentalo nel report.

### 1. Un solo tokenizer/parser, non due

`tokenize(s)` e `parse(s)` (esistenti, `pub fn parse`) **diventano wrapper** che chiamano nuove
funzioni con un parametro `base: NumBase` esplicito, passando `NumBase::Dec`:

```rust
pub fn parse(s: &str) -> Result<Expr, CalcError> {
    parse_with_base(s, NumBase::Dec)
}

pub fn parse_with_base(s: &str, base: NumBase) -> Result<Expr, CalcError> {
    let toks = tokenize_with_base(s, base)?;
    if toks.is_empty() { return Err(CalcError::Empty); }
    let mut p = Parser { toks, pos: 0 };
    let e = p.or_expr()?;   // NOTA: entry point ora è or_expr(), non più expr()!
    if p.pos != p.toks.len() { return Err(CalcError::Syntax); }
    Ok(e)
}
```

Perché un solo tokenizer e non due separati: con `NumBase::Dec` come default, il comportamento
di `parse("6/3")`, `parse("sin(30)")`, ecc. per TUTTI i ~40 test esistenti in `engine.rs`
**deve restare byte-identico** — sono la prova che il refactor non ha rotto nulla. Se scrivessi un
secondo tokenizer duplicato invece di parametrizzare quello esistente, rischieresti divergenza
silenziosa. Bonus di questo design (non richiesto esplicitamente ma corretto e verificato
intenzionale): un'espressione come `255∧15` funziona ANCHE in modalità Dec (mostra il risultato in
decimale) — non è "vietato" usare gli operatori bitwise fuori dalla modalità programmatore, è solo
che i tasti che li inseriscono stanno nella sezione UI a parte.

`tokenize` (privata, non `pub`) diventa `tokenize_with_base(s, base)`: la parte NUOVA
(whitespace, operatori aritmetici, parentesi, `^`, `π`, `√`, `∛`, `!`, `%`, il branch alfabetico
per le funzioni/costanti) resta **identica al codice esistente, copiala verbatim**, con AGGIUNTI
gli 8 nuovi branch a simbolo singolo (§2). L'UNICA parte che cambia comportamento in base a `base`
è il branch che riconosce le cifre:

```rust
// Branch cifre — SOSTITUISCE il branch numerico esistente di `tokenize`.
// In Dec: comportamento IDENTICO all'originale (mantissa + punto + esponente e/E,
// NESSUN separatore '_' accettato — resta un errore di sintassi come oggi, es.
// "1_000" in Dec continua a dare Syntax; il separatore è SOLO per Hex/Oct/Bin).
// In Hex/Oct/Bin: consuma cifre valide per quella base PIÙ eventuali separatori
// '_' (per poter ri-leggere un risultato formattato con raggruppamento, §6),
// NIENTE punto decimale, NIENTE esponente (interi soltanto).
//
// Nota sul guard: la condizione che SELEZIONA questo branch (sotto, nel match
// esterno) richiede che il carattere DI INNESCO sia una cifra vera — mai '_'.
// Questo garantisce automaticamente che un run non possa MAI iniziare con '_'
// (un buffer tipo "_FF" cade nel branch "carattere sconosciuto" → Syntax, corretto).
c if (base == NumBase::Dec && (c.is_ascii_digit() || c == '.'))
     || (base != NumBase::Dec && c.is_digit(base.radix())) =>
{
    if base == NumBase::Dec {
        // ⚠️ Copia ESATTAMENTE il branch numerico esistente di `tokenize` (mantissa
        // + il blocco "esponente scientifico opzionale" con tutto il suo commento —
        // è la logica che protegge dal bug C-1, non toccarla, non riassumerla).
    } else {
        let start = i;
        while i < chars.len() && (chars[i].is_digit(base.radix()) || chars[i] == '_') {
            i += 1;
        }
        let raw: String = chars[start..i].iter().collect();
        let digits: String = raw.chars().filter(|c| *c != '_').collect();
        // u64, NON i64: `from_str_radix::<i64>` fallirebbe su "FFFFFFFFFFFFFFFF"
        // (tutti i bit a 1 su 64 bit è un u64 valido ma supera i64::MAX). Il cast
        // `as i64` reinterpreta il pattern di bit in complemento a due — corretto
        // per rappresentare valori "negativi" a 64 bit (es. ¬(0) = tutti 1 = -1).
        let n = u64::from_str_radix(&digits, base.radix())
            .map_err(|_| CalcError::Syntax)? as i64 as f64;
        out.push(Token::Num(n));
    }
}
```

`NumBase` e `BitWidth` (nuovi, in `engine.rs`, `pub`):

```rust
/// Base numerica per la modalità programmatore. `Dec` è il default — comportamento
/// identico alla calcolatrice scientifica esistente in ogni sua parte.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NumBase { Dec, Hex, Oct, Bin }

impl Default for NumBase {
    fn default() -> Self { NumBase::Dec }
}

impl NumBase {
    /// Base numerica (10/16/8/2) usata da `char::is_digit`/`u64::from_str_radix`.
    /// Non chiamata mai per `Dec` nel branch cifre (quel ramo ha la sua logica
    /// dedicata), ma resta definita per completezza e per uso diretto nei test.
    fn radix(self) -> u32 {
        match self {
            NumBase::Dec => 10,
            NumBase::Hex => 16,
            NumBase::Oct => 8,
            NumBase::Bin => 2,
        }
    }
}

/// Larghezza del "registro" su cui operano NOT/shift/rotate e la formattazione
/// in base non-decimale. Default: Qword (64 bit) — comportamento a piena
/// ampiezza i64, coerente con l'unica larghezza che esisteva prima di questo
/// compito. **Non si applica MAI in modalità Dec** (vedi §4/§5): lì il valore
/// mostrato è il numero reale, non mascherato — la larghezza bit è uno stato
/// "inerte" finché `base_mode != Dec`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BitWidth { Byte, Word, Dword, Qword }

impl Default for BitWidth {
    fn default() -> Self { BitWidth::Qword }
}

impl BitWidth {
    /// Numero di bit: 8/16/32/64.
    pub fn bits(self) -> u32 {
        match self {
            BitWidth::Byte => 8,
            BitWidth::Word => 16,
            BitWidth::Dword => 32,
            BitWidth::Qword => 64,
        }
    }
}
```

### 2. Nuovi simboli — SEMPRE glyph Unicode dedicati, MAI parole chiave testuali

Motivo (verificato, non ipotetico): in modalità Hex i tasti A-F devono produrre le cifre esadecimali
10-15. Se AND/OR/XOR fossero parole testuali "AND"/"OR"/"XOR" appese al buffer (come `sin(`),
"AND" comincia per 'A' — in modalità Hex il branch cifre consumerebbe la 'A' come cifra esadecimale
PRIMA che il branch alfabetico veda "ND", producendo un errore di sintassi silenzioso e
difficile da diagnosticare. Usa invece simboli Unicode dedicati a carattere singolo, esattamente
come già fanno `×` `÷` `−` `√` `∛` `π` per lo stesso motivo (branch O(1) nel tokenizer, zero
ambiguità con le cifre o gli identificatori):

| Operazione | Simbolo | Nome Unicode | Codepoint |
|---|---|---|---|
| AND bitwise | `∧` | LOGICAL AND | U+2227 |
| OR bitwise | `∨` | LOGICAL OR | U+2228 |
| XOR bitwise | `⊻` | XOR | U+22BB |
| NOT bitwise (unario, prefisso) | `¬` | NOT SIGN | U+00AC |
| Shift sinistra | `≪` | MUCH LESS-THAN | U+226A |
| Shift destra | `≫` | MUCH GREATER-THAN | U+226B |
| Rotazione sinistra (ROL) | `↺` | ANTICLOCKWISE OPEN CIRCLE ARROW | U+21BA |
| Rotazione destra (ROR) | `↻` | CLOCKWISE OPEN CIRCLE ARROW | U+21BB |

Le **etichette dei tasti** restano testo leggibile ("AND", "OR", "XOR", "NOT", "SHL"/"SHR",
"ROL"/"ROR") — è SOLO il carattere che `handle_key` appende al buffer (e che il tokenizer
riconosce) a dover essere il glyph dedicato. Stesso principio già in uso: il tasto mostra "sin" ma
per √ mostra proprio "√" — qui va sempre per la via del simbolo dedicato, mai per la via testuale.

`NOT` è unario/prefisso: riusa il meccanismo `Func` già esistente (stesso schema di `√`/`∛`),
**non serve nessuna nuova struttura di grammatica** — aggiungi `FuncId::Not` all'enum esistente,
il branch simbolo `'\u{00AC}' => Token::Func(FuncId::Not)` nel tokenizer, e il match in `atom()`
già gestisce `Func '(' expr ')'` per qualunque `FuncId` senza modifiche.

`AND`/`OR`/`XOR`/shift/rotate sono binari infissi: servono nuovi `BinOp` (`And`, `Or`, `Xor`,
`Shl`, `Shr`, `Rol`, `Ror`) sull'enum esistente — **riusano il nodo `Expr::Bin` esistente**, zero
nuove varianti di `Expr`.

### 3. Nuova precedenza grammaticale — livelli C-like, INSERITI SOPRA `expr()` esistente

Precedenza da stretta a larga (standard C per shift/AND/XOR/OR, verificata a mano — vedi test
discriminanti sotto; rotate condivide lo stesso livello dello shift, sono concettualmente la
stessa famiglia "riposiziona i bit di N posizioni"):
`× ÷ %` (più stretto, invariato) > `+ −` (invariato) > `≪ ≫ ↺ ↻` (nuovo, un solo livello) >
`∧` AND (nuovo) > `⊻` XOR (nuovo) > `∨` OR (più largo, nuovo).

I 4 nuovi metodi su `impl Parser` (aggiunti, NON modificano `expr`/`term`/`unary`/`power`/
`postfix`/`atom` esistenti — li chiamano soltanto come livello più stretto, stesso pattern già
usato da `power` che chiama `postfix`):

```rust
/// Livello OR bitwise — precedenza più bassa di tutte (nuovo entry point di `parse_with_base`).
fn or_expr(&mut self) -> Result<Expr, CalcError> {
    let mut lhs = self.xor_expr()?;
    while let Some(Token::Or) = self.peek() {
        self.bump();
        let rhs = self.xor_expr()?;
        lhs = Expr::Bin { op: BinOp::Or, lhs: Box::new(lhs), rhs: Box::new(rhs) };
    }
    Ok(lhs)
}

fn xor_expr(&mut self) -> Result<Expr, CalcError> {
    let mut lhs = self.and_expr()?;
    while let Some(Token::Xor) = self.peek() {
        self.bump();
        let rhs = self.and_expr()?;
        lhs = Expr::Bin { op: BinOp::Xor, lhs: Box::new(lhs), rhs: Box::new(rhs) };
    }
    Ok(lhs)
}

fn and_expr(&mut self) -> Result<Expr, CalcError> {
    let mut lhs = self.shift_expr()?;
    while let Some(Token::And) = self.peek() {
        self.bump();
        let rhs = self.shift_expr()?;
        lhs = Expr::Bin { op: BinOp::And, lhs: Box::new(lhs), rhs: Box::new(rhs) };
    }
    Ok(lhs)
}

/// Shift E rotazione condividono lo stesso livello (4 token invece di 2).
fn shift_expr(&mut self) -> Result<Expr, CalcError> {
    let mut lhs = self.expr()?;   // ← livello esistente, invariato
    loop {
        let op = match self.peek() {
            Some(Token::Shl) => BinOp::Shl,
            Some(Token::Shr) => BinOp::Shr,
            Some(Token::Rol) => BinOp::Rol,
            Some(Token::Ror) => BinOp::Ror,
            _ => break,
        };
        self.bump();
        let rhs = self.expr()?;
        lhs = Expr::Bin { op, lhs: Box::new(lhs), rhs: Box::new(rhs) };
    }
    Ok(lhs)
}
```

**Test discriminanti obbligatori** (stesso principio di `modulo_basic` — pinnano l'ordine, non
solo il calcolo; usa `parse_with_base`/`evaluate_with_width` con `NumBase::Hex`/`BitWidth::Qword`
o semplicemente `parse` per Dec, dove il glyph funziona comunque per il motivo del punto 1):
- `2∨1∧0` deve dare `2` (AND più stretto di OR: `2 ∨ (1∧0)` = `2∨0` = 2, non `(2∨1)∧0` = 0).
- `1≪2+3` deve dare `32` (shift più largo di `+`: `1≪(2+3)` = `1≪5` = 32, non `(1≪2)+3` = 7).
- `8≫1∧3` deve dare `0` (AND più largo di shift: `(8≫1)∧3` = `4∧3` = 0, non `8≫(1∧3)` = `8≫1` = 4).

### 4. Valutazione — conversione f64 ↔ intero, larghezza bit SOLO per la validità di shift/rotate

`to_i64_checked` **non cambia con la larghezza bit** — resta un controllo puro "è un intero
esatto rappresentabile in i64?", indipendente da `BitWidth`:

```rust
/// Converte un f64 in i64 SOLO se rappresenta esattamente un intero nel range i64.
/// None per: parte frazionaria non nulla, fuori range [i64::MIN, i64::MAX].
///
/// Limite noto (documentalo anche nel commento finale, non solo qui): f64 ha 53 bit
/// di mantissa — interi oltre ±2^53 potrebbero non essere rappresentati esattamente
/// anche se in teoria dentro il range i64. Per l'uso da calcolatrice (conteggi, byte,
/// maschere di bit, indirizzi) è un limite accettabile e coerente con TUTTO il resto
/// di questo engine (già basato su f64 ovunque, vedi format_number).
fn to_i64_checked(v: f64) -> Option<i64> {
    if v.fract() != 0.0 { return None; }
    if v < -(2f64.powi(63)) || v >= 2f64.powi(63) { return None; }
    Some(v as i64)
}
```

**La mascheratura alla larghezza selezionata avviene SOLO nel formatter (§6), MAI qui.** Motivo:
AND/OR/XOR/NOT di due operandi già dentro la larghezza restano naturalmente dentro la larghezza
(le operazioni bitwise non creano bit alti che non erano in nessuno dei due operandi); un input
che ECCEDE la larghezza selezionata (es. l'utente digita un binario a 9 cifre con `BitWidth::Byte`
attivo) viene sì valutato a piena ampiezza qui, ma poi **troncato silenziosamente** dal formatter
al momento della visualizzazione — comportamento scelto deliberatamente (coerente con Windows
Calculator), non un bug: documentalo nel commento e nel test (§6, "overflow di input").

`evaluate` **guadagna un parametro di contesto**, con lo stesso pattern additivo già visto per
`parse`/`tokenize` — serve SOLO per validare l'AMMONTARE di shift/rotate (non per mascherare
risultati, che resta compito del formatter):

```rust
pub fn evaluate(e: &Expr, mode: AngleMode) -> Result<f64, CalcError> {
    evaluate_with_width(e, mode, BitWidth::Qword)   // default: 64 bit, comportamento invariato
}

pub fn evaluate_with_width(e: &Expr, mode: AngleMode, width: BitWidth) -> Result<f64, CalcError> {
    // stesso corpo esistente di `evaluate`, ricorsione ricorsiva PASSA `width`
    // (evaluate_with_width(x, mode, width)?, non più evaluate(x, mode)?)
    ...
}
```

Nel match di `evaluate_with_width()` dentro `Expr::Bin`, i nuovi operatori bitwise/shift seguono lo
STESSO principio già in uso per il fattoriale/radice negativa: dominio invalido → `Ok(f64::NAN)`,
mai `Err` (il formatter lo trasforma in "Error"):

```rust
BinOp::And | BinOp::Or | BinOp::Xor => {
    match (to_i64_checked(l), to_i64_checked(r)) {
        (Some(a), Some(b)) => (match op {
            BinOp::And => a & b,
            BinOp::Or  => a | b,
            BinOp::Xor => a ^ b,
            _ => unreachable!(),
        }) as f64,
        _ => f64::NAN,
    }
}
BinOp::Shl | BinOp::Shr => {
    // checked_shl/checked_shr: MAI l'operatore `<<`/`>>` grezzo su interi Rust —
    // panica in debug (e quindi in `cargo test`) per un ammontare di shift >= 64,
    // raggiungibile da tastiera. L'ammontare valido dipende da `width`, NON è
    // sempre 0..64: `1≪9` con BitWidth::Byte deve dare Errore (Windows Calc fa
    // così), non essere silenziosamente troncato — la larghezza bit limita anche
    // COSA è uno shift legale, non solo come appare il risultato.
    match (to_i64_checked(l), to_i64_checked(r)) {
        (Some(a), Some(b)) if (0..width.bits() as i64).contains(&b) => {
            let shifted = if *op == BinOp::Shl {
                a.checked_shl(b as u32)
            } else {
                a.checked_shr(b as u32)
            };
            shifted.map(|x| x as f64).unwrap_or(f64::NAN)
        }
        _ => f64::NAN,
    }
}
BinOp::Rol | BinOp::Ror => {
    // Rotazione: a differenza dello shift, un ammontare >= width è LEGALE (si
    // riduce modulo width — ruotare un byte di 8 posizioni è un giro completo,
    // identità). Solo un ammontare negativo è invalido.
    match (to_i64_checked(l), to_i64_checked(r)) {
        (Some(a), Some(b)) if b >= 0 => {
            let w = width.bits();
            let n = (b as u32) % w;
            let bits_mask: u64 = if w == 64 { u64::MAX } else { (1u64 << w) - 1 };
            let v = (a as u64) & bits_mask;
            let rotated = if n == 0 {
                v
            } else if *op == BinOp::Rol {
                ((v << n) | (v >> (w - n))) & bits_mask
            } else {
                ((v >> n) | (v << (w - n))) & bits_mask
            };
            // Reinterpretazione bit u64→i64→f64: stesso limite di precisione
            // ±2^53 già documentato per to_i64_checked, non un problema nuovo.
            rotated as i64 as f64
        }
        _ => f64::NAN,
    }
}
```

E in `Expr::Func`, dentro il match su `id` (NOT non ha bisogno di `width` — vedi §6 sul perché la
mascheratura a valle nel formatter basta da sola):

```rust
FuncId::Not => match to_i64_checked(a) {
    Some(n) => !n as f64,
    None => f64::NAN,
},
```

**Test di confine obbligatori**:
- Funzionali base: `5∧3=1`, `5∨2=7`, `5⊻1=4`, `1≪4=16`, `256≫4=16`.
- `2^53 - 1` va e torna esatto tramite `to_i64_checked` (round-trip).
- Un valore appena sopra `i64::MAX` (es. `9223372036854775808.0`, cioè `2^63`) → `to_i64_checked`
  deve dare `None`.
- `i64::MIN as f64` va e torna esatto.
- Shift con ammontare negativo o ≥ `width.bits()` (es. `1≪-1` a Qword, `1≪9` a Byte) → `NaN`, mai
  panic. Verifica ESPLICITAMENTE che lo stesso `1≪9` sia **valido** (non NaN) a `BitWidth::Qword`
  — la validità dipende dalla larghezza, non è un limite assoluto.
- Rotazione: `0b0001` (1) ruotato a sinistra di 1 posizione con `BitWidth::Byte` → `0b0010` (2);
  ruotato di 8 posizioni con `BitWidth::Byte` → torna `0b0001` (giro completo, identità); ammontare
  negativo → `NaN`.

### 5. Base-switch — le 8 funzioni chiave del tokenizer/evaluator sopra vanno RIUSATE, non ripetute

Non c'è altro codice nuovo di "conversione": una conversione è semplicemente
`parse_with_base(vecchio) → evaluate_with_width → format_integer_in_base(nuova base/larghezza)`.
Il dettaglio di dove questo si applica (i tasti in `main.rs`) è alla Parte C, §8.

## FINE PARTE A — commit e stop

A questo punto **fermati, esegui la verifica, committa, e NON procedere alla Parte B nello stesso
commit**. Verifica:

```powershell
cargo test -p plugin-calc
cargo clippy -p plugin-calc --all-targets
```

Tutti i test esistenti (quelli che chiamano `parse(...)`/`tokenize(...)`/`evaluate(...)` con la
FIRMA ORIGINALE) devono restare verdi **senza essere stati modificati** — è la prova che
`NumBase::Dec`/`BitWidth::Qword` sono byte-identici al comportamento precedente. Se un test
esistente fallisce o l'hai dovuto toccare per farlo passare, **fermati e segnalalo nel report
invece di "aggiustarlo"** — significa che il refactor ha rotto qualcosa che non doveva rompere.

Bump `crates/plugin-calc` da `2.0.0` a `2.1.0` (nuova capacità additiva), `CHANGELOG.md` +
`IMPLEMENTATION.md`, una riga in `Docs/i18n/ita/HANDOFF.md` FATTO. Poi procedi alla Parte B.

---

## Parte B — `format.rs`: formattazione base-aware (mascheratura, padding, raggruppamento)

### 6. `format_integer_in_base` — mascheratura alla larghezza, zero-padding, raggruppamento `_`

**`format_number` (decimale) resta byte-identica, INTOCCATA.** La modalità Dec non applica MAI la
larghezza bit — è la decisione più importante di questo compito, verificata con un revisore:
mostrare `255` in Dec dopo `¬(0)` a `BitWidth::Byte` sarebbe incoerente con quel `255` che poi
digitato di nuovo in Dec e sommato a `1` deve dare `256` (aritmetica normale, non mascherata) — se
invece Dec mascherasse, l'utente vedrebbe `-1` invece di `255`, e la sequenza "cambia base, torna
a Dec, fai un calcolo" diventerebbe imprevedibile. **Regola netta: `base_mode == Dec` → la
larghezza bit è completamente inerte, il valore mostrato è il numero reale via `format_number`,
punto.** Solo `base_mode != Dec` applica mascheratura/padding/raggruppamento.

```rust
/// Formatta un valore f64 come intero SENZA SEGNO nella base e larghezza date.
/// `None` se il valore non è un intero rappresentabile in i64 (stesso limite di
/// `to_i64_checked` in engine.rs — se preferisci non duplicare il controllo,
/// rendi `to_i64_checked` `pub(crate)` invece di privato e importalo qui; usa
/// il tuo giudizio, documenta la scelta nel report).
///
/// Mascheratura: il valore viene troncato ai bit bassi di `width` PRIMA di
/// formattare — sia per i risultati di NOT/shift/rotate (che a Qword non hanno
/// mai bisogno di troncamento, essendo già a piena ampiezza) sia per un input
/// digitato che eccede la larghezza selezionata (es. un binario a 9 cifre con
/// BitWidth::Byte attivo → troncato agli 8 bit bassi, silenziosamente — è il
/// comportamento di Windows Calculator, non un bug: testalo esplicitamente).
///
/// Zero-padding: SEMPRE alla larghezza piena del numero di cifre che quella base
/// richiede per rappresentare `width` bit (tabella sotto) — mai cifre "risparmiate"
/// per un valore piccolo. Motivo: rende inequivocabile il pattern di bit di un
/// valore negativo (es. ¬(0) a Byte in Hex DEVE mostrare "FF", non "F" — "F" da
/// solo si leggerebbe come 15, non come il pattern di 8 bit tutti a 1).
///
/// | Base | Cifre per larghezza | Raggruppamento |
/// |---|---|---|
/// | Hex | width/4 (Byte=2, Word=4, Dword=8, Qword=16) | ogni 4 cifre, separatore `_` |
/// | Bin | width (Byte=8, Word=16, Dword=32, Qword=64) | ogni 4 cifre, separatore `_` |
/// | Oct | ⌈width/3⌉ (Byte=3, Word=6, Dword=11, Qword=22) | nessuno (non allinea a potenze di 2) |
pub fn format_integer_in_base(x: f64, base: NumBase, width: BitWidth) -> Option<String> {
    if x.fract() != 0.0 { return None; }
    if x < -(2f64.powi(63)) || x >= 2f64.powi(63) { return None; }
    let n = x as i64;
    let bits_total = width.bits();
    let mask: u64 = if bits_total == 64 { u64::MAX } else { (1u64 << bits_total) - 1 };
    let bits = (n as u64) & mask;
    match base {
        NumBase::Dec => unreachable!(
            "il chiamante non deve MAI invocare questa funzione per NumBase::Dec — \
             vedi §6, la larghezza bit è inerte in Dec, si usa format_number"
        ),
        NumBase::Hex => {
            let digits = (bits_total / 4) as usize;
            Some(group_every(&format!("{bits:0digits$X}"), 4, '_'))
        }
        NumBase::Bin => {
            let digits = bits_total as usize;
            Some(group_every(&format!("{bits:0digits$b}"), 4, '_'))
        }
        NumBase::Oct => {
            let digits = ((bits_total + 2) / 3) as usize;   // ⌈width/3⌉
            Some(format!("{bits:0digits$o}"))
        }
    }
}

/// Inserisce `sep` ogni `n` caratteri contando DA DESTRA (cifra meno significativa),
/// così un numero di cifre non multiplo di `n` avrebbe il gruppo corto all'inizio
/// (non succede mai con le combinazioni base/larghezza di questa tabella — Hex e Bin
/// hanno sempre un numero di cifre che è 0 mod 4 o < 4 — ma la funzione resta
/// corretta in generale, più semplice da fidarsi senza casi speciali).
fn group_every(s: &str, n: usize, sep: char) -> String {
    let rev: String = s.chars().rev().collect();
    let mut out = String::new();
    for (i, c) in rev.chars().enumerate() {
        if i > 0 && i % n == 0 { out.push(sep); }
        out.push(c);
    }
    out.chars().rev().collect()
}
```

**Perché il separatore `_` e non lo spazio** (idea originale, scartata): dopo "=" il buffer
contiene il risultato formattato, e deve restare ri-parsabile da `parse_with_base` per poter
continuare un calcolo (`engine_round_trips_its_own_formatter_output`, test esistente in
`engine.rs` — stessa classe del bug C-1 già risolto altrove in questo file). Uno spazio
spezzerebbe il buffer in più token (`[Num, Num, ...]`, token in eccesso → Syntax); `_` invece è
esplicitamente ignorato dal branch cifre del tokenizer (§1) — il buffer resta UN token, `+1`
funziona.

**Test obbligatori**:
- `255` (Hex, Byte) → `"FF"` (2 cifre, la larghezza è Byte — nessun padding oltre le 2 cifre).
- `255` (Hex, Qword) → `"0000_0000_0000_00FF"` (16 cifre, 4 gruppi — `format_integer_in_base`
  riceve sempre `width` dal chiamante: STESSO valore, output diverso a seconda della larghezza
  passata, non dare per scontato "il valore è piccolo quindi poche cifre").
- `8` (Oct, Byte) → `"010"` (3 cifre, ⌈8/3⌉=3).
- `5` (Bin, Byte) → `"00000101"` raggruppato → `"0000_0101"`.
- `¬(0)` (Hex, Byte) → `"FF"`; `¬(0)` (Hex, Qword) → 16 F raggruppate `"FFFF_FFFF_FFFF_FFFF"`.
- **Overflow di input silenzioso** (comportamento intenzionale, testalo esplicitamente): il valore
  `256.0` (es. da un binario a 9 cifre digitato con `BitWidth::Byte` attivo) in Bin/Byte →
  `"0000_0000"` (troncato agli 8 bit bassi di 256, che sono tutti zero — 256 = 0x100).
- Un valore non intero (es. `0.5`) in qualunque base → `None`.

## FINE PARTE B — commit e stop

```powershell
cargo test -p plugin-calc
cargo clippy -p plugin-calc --all-targets
```

Bump `crates/plugin-calc` da `2.1.0` a `2.2.0`, `CHANGELOG.md` + `IMPLEMENTATION.md`, una riga in
`Docs/i18n/ita/HANDOFF.md` FATTO. Poi procedi alla Parte C.

---

## Parte C — `render.rs`, `main.rs` (stato, tasti, UI, toggle), CSS

### 7. Visibilità della sezione — un TOGGLE, non tasti/griglia fissi

**Risposta alla domanda di Maurizio**: la sezione programmatore è nascosta di default (la
calcolatrice si presenta esattamente come nello screenshot allegato) e appare tramite un piccolo
controllo dedicato, **non** un tasto della griglia 5 colonne esistente — verificato che
`plugin-window.js` lega i click con delega generica su QUALUNQUE elemento con `[data-evt]` (non
solo `.lare-key`), quindi il controllo può essere un piccolo elemento cliccabile nella riga di
stato (`.lare-status`, quella che oggi mostra "DEG") invece di consumare uno slot della griglia
7×5 esistente — che resta quindi **completamente intoccata, zero rischio di regressione sui suoi
35 tasti**.

Nuovo campo `CalcState.prog_visible: bool` (default `false`, tramite `#[derive(Default)]`). Nuovo
tasto `data-evt="toggle_prog"` che lo inverte. **`base_mode`/`bit_width` sopravvivono al
nascondimento** — nascondere la sezione non li resetta a Dec/Qword: se l'utente era in Hex/Byte,
nasconde la sezione, il valore mostrato resta in Hex/Byte finché non lo cambia esplicitamente (la
riga di stato lo rende sempre visibile, vedi sotto — non lascia l'utente confuso su perché un
numero "sembra strano").

### 8. `main.rs` — `CalcState`, nuovi tasti, conversione al cambio base/larghezza

Nuovi campi in `CalcState` (default: `NumBase::Dec`/`BitWidth::Qword`/`false`, tramite
`#[derive(Default)]` + gli `impl Default` già scritti in Parte A):

```rust
/// Base numerica per la modalità programmatore (default: Dec). Cambiata dai 4
/// tasti DEC/HEX/OCT/BIN — CONVERTE anche il valore corrente nel buffer (non solo
/// switch di visualizzazione, vedi sotto).
base_mode: NumBase,
/// Larghezza del registro per NOT/shift/rotate e per la formattazione in base
/// non-decimale (default: Qword). Cambiata dai 4 tasti BYTE/WORD/DWORD/QWORD,
/// stesso comportamento "converte" dei tasti base. INERTE se base_mode == Dec.
bit_width: BitWidth,
/// Sezione programmatore visibile/nascosta (default: nascosta — la calcolatrice
/// si presenta come oggi finché l'utente non la apre esplicitamente).
prog_visible: bool,
```

**Tasti base E larghezza — CONVERTONO il buffer**, stesso comportamento per entrambe le famiglie
di tasti (fattorizza in UN helper condiviso, non duplicare la logica 8 volte):

```rust
/// Se il buffer corrente valuta a un intero rappresentabile nella VECCHIA base/
/// larghezza, lo riformatta nella NUOVA base/larghezza e lo scrive nel buffer
/// (comportamento "conversione", imita Windows Calculator modalità Programmatore).
/// Altrimenti (buffer vuoto, espressione incompleta, non intero) non tocca il
/// buffer — lascia change_state() (chiamata dal chiamante) libera di cambiare
/// comunque la modalità.
fn try_convert_buf(state: &mut CalcState, new_base: NumBase, new_width: BitWidth) {
    let Ok(ast) = engine::parse_with_base(&state.buf, state.base_mode) else { return };
    let Ok(v) = engine::evaluate_with_width(&ast, state.angle_mode, state.bit_width) else { return };
    let converted = if new_base == engine::NumBase::Dec {
        Some(format_number(v))
    } else {
        format::format_integer_in_base(v, new_base, new_width)
    };
    if let Some(text) = converted {
        state.buf = text;
        state.last_was_result = true;
    }
    // else: non un intero rappresentabile — buffer invariato, solo la modalità cambia
    // (fatto dal chiamante DOPO questa funzione, vedi i due match arm sotto).
}
```

```rust
"base_dec" | "base_hex" | "base_oct" | "base_bin" => {
    let new_base = match key {
        "base_dec" => NumBase::Dec, "base_hex" => NumBase::Hex,
        "base_oct" => NumBase::Oct, "base_bin" => NumBase::Bin,
        _ => unreachable!(),
    };
    try_convert_buf(state, new_base, state.bit_width);
    state.base_mode = new_base;
}
"width_byte" | "width_word" | "width_dword" | "width_qword" => {
    let new_width = match key {
        "width_byte" => BitWidth::Byte, "width_word" => BitWidth::Word,
        "width_dword" => BitWidth::Dword, "width_qword" => BitWidth::Qword,
        _ => unreachable!(),
    };
    try_convert_buf(state, state.base_mode, new_width);
    state.bit_width = new_width;
}
"toggle_prog" => { state.prog_visible = !state.prog_visible; }
```

**Test obbligatori** (stati intermedi, non solo il risultato finale — pinna la sequenza):
- `d2 d5 d5 base_hex` → buf `"0000_0000_0000_00FF"` (255 convertito, ma `BitWidth::Qword` è il
  default — il padding è SEMPRE alla piena larghezza, §6, quindi 16 cifre raggruppate, NON `"FF"`
  nudo). Per un test con `"FF"` pulito, precedi con `width_byte`:
  `d2 d5 d5 width_byte base_hex` → `"FF"`.
- **Test integrato end-to-end** (l'intera catena in una sequenza, verificato a mano — usalo
  com'è, corregge un errore che avevo fatto io stesso nella prima stesura di questa sequenza):
  `base_hex width_byte hexF hexF eq` → buf `"FF"` (0xFF=255, Byte, 2 cifre, nessun raggruppamento
  visibile — 2 cifre < gruppo di 4). Poi `op_and d0 hexF eq` → buf `"0F"` (0xFF AND 0x0F = 0x0F).
  Poi `base_bin` → buf `"0000_1111"` (15 in binario, 8 bit Byte, un separatore a metà). Poi
  `base_dec` → buf `"15"` (Dec non mascherato — mostra il vero valore 15).
- `d5 op_add base_hex` → buf resta `"5+"` (espressione incompleta, nessuna conversione, solo
  `state.base_mode` cambia — verifica anche `state.base_mode == Hex` dopo).
- Overflow silenzioso: `width_byte base_bin` poi 9 cifre binarie (`d1` seguito da 8×`d0` — le
  cifre 0/1 passano dai tasti normali `d0`/`d1`, validi in Bin) poi `eq` → buf `"0000_0000"`
  (256 troncato a 8 bit = 0, coerente col test di formato in Parte B).

**Tasto `"eq"`** — ramifica su `state.base_mode` (il ramo `Dec` è quello ESISTENTE, testuale
identico, non toccarlo — aggiungi solo l'else); nota che a differenza di `try_convert_buf` questo
ramo mostra `"Error"` esplicito su fallimento, non lascia il buffer invariato — `=` è una richiesta
esplicita di valutazione, un cambio di base/larghezza no:

```rust
"eq" => {
    state.last_expr = state.buf.clone();
    let result = engine::parse_with_base(&state.buf, state.base_mode)
        .and_then(|e| engine::evaluate_with_width(&e, state.angle_mode, state.bit_width));
    state.buf = match (result, state.base_mode) {
        (Ok(v), NumBase::Dec) => format_number(v),
        (Ok(v), _) => format::format_integer_in_base(v, state.base_mode, state.bit_width)
            .unwrap_or_else(|| "Error".to_string()),
        (Err(_), _) => "Error".to_string(),
    };
    state.last_was_result = true;
}
```

**Nuovi tasti** (nella sezione match esistente, stesso stile dei tasti scientifici):
```rust
// Cifre esadecimali A-F: stesso trattamento delle cifre 0-9 — passano per il ramo
// `ch: Option<char>` generico (già gestisce la regola "smart clear after result"),
// NON per append_str. Aggiungi ai match esistenti di `ch` (vicino a "d0".."d9"):
//   "hexA" => Some('A'), "hexB" => Some('B'), ... "hexF" => Some('F'),

"op_and" => append_str(state, "∧", false),
"op_or"  => append_str(state, "∨", false),
"op_xor" => append_str(state, "⊻", false),
"fn_not" => append_str(state, "¬(", true),   // prefisso, apre parentesi come √/∛
// Shift: UN tasto, 2nd sceglie la direzione — esattamente come fn_sqrt (√/∛).
"op_shift" => append_str(state, if was_shifted { "≫" } else { "≪" }, false),
// Rotazione: stessa idea, tasto separato dallo shift (famiglia diversa di operatore
// anche se stesso livello di precedenza — vedi §3).
"op_rotate" => append_str(state, if was_shifted { "↻" } else { "↺" }, false),
```

### 9. `key_grid` — nuova sezione, 20 tasti (4 righe × 5 colonne)

Layout (4 basi + 6 cifre esadecimali + 4 booleani + 1 shift + 4 larghezze + 1 rotate = 20 = 4
righe piene):

```
Riga 1: DEC | HEX | OCT | BIN | NOT
Riga 2: A   | B   | C   | D   | E
Riga 3: F   | AND | OR  | XOR | SHL/SHR (2nd → SHR)
Riga 4: BYTE| WORD| DWORD|QWORD| ROL/ROR (2nd → ROR)
```

Genera questa griglia in una funzione dedicata `programmer_key_grid(state: &CalcState) -> String`
(non infilarla dentro `key_grid` esistente — separare le due funzioni rende ovvio nel diff che la
griglia scientifica non è stata toccata). Ogni tasto segue ESATTAMENTE il pattern già in uso
(`data-evt`, `data-key` per la tastiera fisica — per A-F usa `data-key="a A"` ecc., minuscolo e
maiuscolo, nessuna collisione con le mappature esistenti verificata: `"* x"` per ×, `". ,"` per
il punto, nessun'altra lettera singola è già mappata). Le etichette SHL/SHR e ROL/ROR cambiano con
`state.shift` come già fa `sqrt_label`/`square_label`.

`render_window`: **la sezione intera va inclusa SOLO se `state.prog_visible`**:

```rust
let prog_html = if state.prog_visible {
    format!("<div class=\"lare-prog-section\">{}</div>", programmer_key_grid(state))
} else {
    String::new()
};
```

concatenata DOPO la griglia esistente, dentro lo stesso `.lare-calc`.

**Riga di stato** — estendi `.lare-status`. Regola: la larghezza bit non compare MAI se
`base_mode == Dec` (è inerte, §6 — mostrarla sarebbe fuorviante), indipendentemente da
`prog_visible` (l'indicatore deve restare corretto anche a sezione chiusa, così l'utente capisce
perché un numero sembra "strano" anche senza riaprire i tasti):

```rust
let status_line = if state.base_mode == NumBase::Dec {
    mode_label.to_string()
} else {
    format!("{mode_label} · {base_label} · {width_label}")
};
```

più il controllo di toggle accanto (piccolo elemento cliccabile, non un `.lare-key`):

```rust
let toggle_label = if state.prog_visible { "▲ PROG" } else { "▼ PROG" };
// nel div status, dopo status_line:
//   <span class="lare-prog-toggle" data-evt="toggle_prog">{toggle_label}</span>
```

Il test esistente `render_shows_deg_by_default_and_shift_on_class` verifica `h.contains("DEG")`:
resta vero con `base_mode == Dec` di default (la stringa "DEG" bare, esattamente come oggi) —
verificalo comunque, non dare per scontato.

### 10. `render_window` — display lineare (mai 2D) quando `base_mode != Dec`

**Punto critico, verificato con un revisore**: NON puoi lasciare `render_window` a chiamare
`parse(&state.buf)` (decimale) invariato quando `base_mode != Dec`. Esempio concreto del bug che
eviti: buffer `"E+1"` in modalità Hex — `parse()` decimale lo tokenizza come `Const(E) + Num(1)`
(la vecchia identificazione della costante di Eulero!) e lo renderizza come se fosse un calcolo
con *e*, non come l'esadecimale `E+1` che l'utente ha digitato. Fix (3 righe, prima del calcolo
di `display` esistente):

```rust
let display = if state.base_mode != engine::NumBase::Dec {
    html_escape(&state.buf)
} else {
    match parse(&state.buf) {
        Ok(ast) => render::render(&ast),
        Err(_)  => html_escape(&state.buf),
    }
};
```

In modalità non-Dec il display è SEMPRE lineare (mai frazioni 2D) — `render.rs` in quel percorso
non viene proprio chiamato, resta invocato solo per Dec esattamente come oggi.

### 11. `render.rs` — nuovi rami, STESSO pattern di `Mod` (operatore inline, non 2D speciale)

`render()` e `prec()` diventano exhaustive-match-incompleti finché non gestisci i nuovi `BinOp`/
`FuncId` — il compilatore te lo segnala (7 nuovi `BinOp`: And/Or/Xor/Shl/Shr/Rol/Ror, più
`FuncId::Not`). Anche in modalità Dec (§1, il bonus "255∧15 funziona anche in Dec") queste
espressioni possono arrivare a `render()` — devono avere un rendering sensato anche lì, per questo
`render.rs` va esteso pur restando NON usato dal percorso non-Dec (§10).

`prec()` va rinumerata per fare spazio ai 4 nuovi livelli SOTTO l'attuale minimo (Add/Sub=1): i
valori numerici sono puramente relativi (solo l'ordine conta — verificato leggendo `operand()`),
ma **riscrivi l'intera funzione verbatim come sotto**, non tentare un editing incrementale dei
numeri esistenti (rischio di lasciare un ramo con il vecchio valore per errore):

```rust
fn prec(e: &Expr) -> u8 {
    match e {
        Expr::Num(_) | Expr::Neg(_) => 7,
        Expr::Bin { op: BinOp::Mul | BinOp::Div | BinOp::Mod, .. } => 6,
        Expr::Bin { op: BinOp::Add | BinOp::Sub, .. } => 5,
        Expr::Bin { op: BinOp::Shl | BinOp::Shr | BinOp::Rol | BinOp::Ror, .. } => 4,
        Expr::Bin { op: BinOp::And, .. } => 3,
        Expr::Bin { op: BinOp::Xor, .. } => 2,
        Expr::Bin { op: BinOp::Or, .. } => 1,
        Expr::Const(_) | Expr::Func { .. } | Expr::Pow { .. } | Expr::Factorial(_) => 7,
    }
}
```

Nel match `Expr::Bin{op,lhs,rhs}` esistente (quello "inline", non il ramo `Div`), aggiungi i 7
nuovi simboli alla mappa `sym`: `And => "∧"`, `Or => "∨"`, `Xor => "⊻"`, `Shl => "≪"`,
`Shr => "≫"`, `Rol => "↺"`, `Ror => "↻"` (stessi glyph della tabella §2 — coerenza
display/input). Nel match `Expr::Func`, aggiungi `FuncId::Not => "¬"` alla mappa `nome`.

Tutti gli esistenti test di `render.rs` (frazioni, potenze, precedenza `+`/`×`) devono restare
verdi senza modifiche — la rinumerazione di `prec()` è invisibile alle stringhe HTML prodotte, che
sono ciò che quei test verificano.

### 12. CSS — `crates/ui/frontend/plugin-catalog.css`, sezione visivamente separata + toggle

Aggiungi accanto alle regole esistenti di `.lare-key-grid`/`.lare-key`/`.lare-status` (non
duplicarle):

```css
/* ── Sezione programmatore — sfondo leggermente diverso, delimita i tasti bitwise/base ── */
.lare-prog-section {
  margin-top: 8px;
  padding: 6px;
  border: 1px solid rgba(240, 180, 80, 0.30);   /* accento caldo, contrasta col blu esistente */
  border-radius: 8px;
  background: rgba(240, 180, 80, 0.05);
}
.lare-prog-section .lare-key-grid { margin: 0; }

/* ── Toggle sezione programmatore — piccolo controllo nella riga di stato ── */
.lare-prog-toggle {
  cursor: pointer;
  margin-left: 6px;
  padding: 1px 6px;
  border: 1px solid rgba(240, 180, 80, 0.4);
  border-radius: 4px;
  font-size: 0.85em;
}
.lare-prog-toggle:hover { background: rgba(240, 180, 80, 0.15); }
```

(Valori indicativi — usa il tuo giudizio sulla tinta esatta purché resti "leggermente diversa" dal
blu esistente (`rgba(80, 160, 240, ...)` dei tasti scientifici) e coerente con lo sfondo scuro del
resto della UI. Non serve chiedere conferma per la tinta esatta.)

Nessuna modifica a `crates/ui/frontend/*.js` è necessaria: verificato che `plugin-window.js` lega i
click tramite delega generica su `[data-evt]` (nessuna assunzione sul numero di tasti o sulla
struttura della griglia, e funziona su QUALUNQUE elemento con quell'attributo, non solo
`.lare-key` — è così che il toggle nella riga di stato funziona senza codice nuovo) e la tastiera
fisica via scan generico di `[data-key]`. **Non toccare nessun file `.js`.**

## Verifica finale (dopo la Parte C)

```powershell
cargo build -p plugin-calc
cargo test -p plugin-calc
cargo clippy -p plugin-calc --all-targets
```

Se il tuo ambiente ha un display: avvia orchestrator+ui, apri `/calc`. Verifica che di default sia
**identica allo screenshot che Maurizio ha allegato** (nessuna sezione programmatore visibile).
Clicca il toggle: appare la sezione a sfondo ambra. Prova `255 → HEX` (deve dare, a `BitWidth`
default Qword, `0000_0000_0000_00FF` — cambia a `BYTE` prima se vuoi vedere solo `FF`), `5 ∧ 3 =`
(deve dare `1`), `1 ≪ 4 =` (deve dare `16`). Se non puoi verificare dal vivo, scrivilo nel
report — il supervisore la fa lui prima del merge.

Bump `crates/plugin-calc` da `2.2.0` a `2.3.0` (Parte C espone tutto nella UI), `CHANGELOG.md` +
`IMPLEMENTATION.md`, una riga in `Docs/i18n/ita/HANDOFF.md` FATTO.

## Cosa NON fare

- Non toccare `tokenize`/`parse`/`evaluate`/`Parser::expr`/`term`/`unary`/`power`/`postfix`/`atom`
  esistenti — solo `NumBase::Dec`/`BitWidth::Qword` devono produrne il comportamento, tramite
  delega, mai duplicazione né modifica del corpo.
- Non usare l'operatore Rust grezzo `<<`/`>>` su interi — SEMPRE `checked_shl`/`checked_shr` (vedi
  §4, motivazione: panic in debug/test per shift ≥ larghezza selezionata, raggiungibile da input
  utente). Grep finale prima di committare: `grep -n "<<\|>>" crates/plugin-calc/src/engine.rs`
  non deve mostrare NESSUN operatore shift grezzo su un valore intero (i riferimenti a
  `Token::Shl`/`Shr`, ai glyph `≪`/`≫` o ai commenti sono ovviamente esclusi).
- Non usare parole testuali (`"AND"`, `"OR"`, ecc.) come token appesi al buffer — solo i glyph
  Unicode dedicati della tabella §2.
- Non applicare la mascheratura/larghezza bit in modalità Dec — mai, per nessun motivo (§6). Se
  trovi un caso in cui ti sembra necessario, fermati e segnalalo nel report invece di aggirare
  la regola.
- Non toccare `format_number` (decimale) — resta byte-identica, la nuova
  `format_integer_in_base` è una funzione SEPARATA.
- Non toccare nessun file `.js` — verificato che non serve.
- Non aggiungere un tasto "PROG" nella griglia 5 colonne esistente — il toggle vive nella riga di
  stato (§7/§9), la griglia scientifica resta esattamente 35 tasti, invariata.
