# IMPLEMENTATION — plugin-calc

**Version:** 2.2.0 (Parte B: `format_integer_in_base` con mascheratura/padding/raggruppamento per Hex/Oct/Bin)  
**Binary:** `calc` (discovered as `plugins/calc/calc.exe` on Windows)  
**Role:** Slice 2 aritmetica base + Slice scientifica Tasks 1–4 (engine esteso, render scientifico, Shift sticky + DEG/RAD + tasti scientifici, CSS) + Refinements v2 Task vA (engine: ∛, n!, %) + Task vB (layout 7×5, nuovi tasti UI, Shift v2, riga stato) + modalità programmatore Parte A (engine base-aware).

## Files

| File | Purpose |
|------|---------|
| `Cargo.toml` | `[[bin]] name="calc"`, deps: `plugin-protocol` + `serde_json` |
| `src/engine.rs` | Parser recursive-descent → AST + evaluate |
| `src/format.rs` | Smart number formatter |
| `src/render.rs` | AST → HTML 2D (frazione impilata per `÷`) |
| `src/main.rs` | `CalcState` + `handle_key` + `render_window` + main loop |
| `plugin.json` | Manifest: `id="calc"`, `triggers.command="/calc"` |

## Architecture

Il crate è organizzato in 3 moduli puri (no I/O, testabili in isolamento) + il binario:

```
engine.rs  →  parse(&str) -> Result<Expr,CalcError>
               evaluate(&Expr) -> Result<f64,CalcError>
format.rs  →  format_number(f64) -> String
render.rs  →  render(&Expr) -> String   (HTML 2D)
main.rs    →  CalcState + handle_key + render_window + stdio loop
```

### engine.rs — AST (Slice scientifica Task 1, `impl Default` Task 3, Refinements v2 Task vA)

```
Expr::Num(f64)
Expr::Const(ConstId)               // Pi | E
Expr::Neg(Box<Expr>)
Expr::Func { id: FuncId, arg: Box<Expr> }  // Sin|Cos|Tan|Asin|Acos|Atan|Log|Ln|Sqrt|Cbrt
Expr::Bin { op: BinOp, lhs: Box<Expr>, rhs: Box<Expr> }
Expr::Pow { base: Box<Expr>, exp: Box<Expr> }  // destra-associativa
Expr::Factorial(Box<Expr>)         // postfisso, lega più stretto di ^ (Task vA)

ConstId:   Pi | E
FuncId:    Sin | Cos | Tan | Asin | Acos | Atan | Log | Ln | Sqrt | Cbrt
AngleMode: Deg | Rad   ← impl Default → Deg (Task 3, necessario per derive(Default) su CalcState)
BinOp:     Add | Sub | Mul | Div | Mod
CalcError: Empty | Syntax | DivByZero
```

Parser recursive-descent a **6 livelli** (grammatica v2 con `postfix`):
- `expr` — `+ −` (precedenza più bassa)
- `term` — `× ÷ %` (precedenza media; `%` è moltiplicativo left-assoc)
- `unary` — `−` unario (destra-assoc ricorsivo); fa sì che `-3^2 = -(3^2) = -9`
- `power` — `^` (destra-assoc: rhs = `unary`); `2^3^2 = 2^(3^2) = 512`
- `postfix` — `!` (postfisso, nuovo livello v2; lhs di `power`): `3!^2 = (3!)^2 = 36`
- `atom` — `Num | Const | Func '(' expr ')' | '(' expr ')'`

**Tokenizer esteso (v2 aggiunge):**
- Accetta ASCII + Unicode (`× ÷ −` U+00D7/F7/2212) come prima
- `^` → `Caret`; `π` (U+03C0) → `Const(Pi)`; `√` (U+221A) → `Func(Sqrt)`
- `∛` (U+221B) → `Func(FuncId::Cbrt)` (Task vA)
- `!` → `Token::Bang` (Task vA)
- `%` → `Token::Percent` (Task vA)
- Run alfabetico `[A-Za-z]+`: `sin/cos/tan/asin/acos/atan/log/ln` → `Func`; `e` → `Const(E)`;
  `pi` → `Const(Pi)`; identificatore sconosciuto → `Err(Syntax)`
- Letterali in **notazione scientifica** (`1e12`, `4.5e-9`): il branch numerico viene PRIMA di
  quello alfabetico, quindi `1e5` è ancora consumato come `Num(100000)` (fix C-1 preservato).
  Un esponente malformato (`"2e"`, `"3e+"`) non viene consumato: la 'e' cade nel branch
  alfabetico → `Const(E)` → token in eccesso → `Syntax`.

**`evaluate(e: &Expr, mode: AngleMode)`:**
- `Const(Pi)` → `f64::consts::PI`; `Const(E)` → `f64::consts::E`
- Trig dirette: `a → if Deg { a.to_radians() } → .sin()/.cos()/.tan()`
- Trig inverse: `.asin()/.acos()/.atan()` → `if Deg { r.to_degrees() }`
- `Log` → `log10`; `Ln` → `ln`; `Sqrt` → `sqrt` (fuori dominio → NaN, non Err)
- `Cbrt` → `a.cbrt()` (definita per tutti i reali, inclusi negativi)
- `Pow{b,e}` → `b.powf(e)`
- `Factorial(inner)`: valuta `inner`; se intero non-negativo → prodotto `1·2·…·n` in f64
  (loop con early-exit su `is_infinite()` per valori >170); negativo o non-intero → `Ok(NaN)`
- `Bin{Mod, l, r}`: `l % r` (f64 remainder); `r == 0.0` → `Err(DivByZero)`

**Task 3 fix (completato):**
- `main.rs` ramo `"eq"`: usa `state.angle_mode` al posto di `AngleMode::Rad` hardcoded.

### Modalità programmatore — Parte A (engine base-aware)

Due nuovi enum pubblici in `engine.rs`:

```
NumBase:  Dec | Hex | Oct | Bin            (impl Default → Dec;  radix() → 10/16/8/2)
BitWidth: Byte | Word | Dword | Qword      (impl Default → Qword; bits() → 8/16/32/64)
```

- **Tokenizer** `tokenize_with_base(s, base)`: il branch cifre è l'unico a dipendere da `base`.
  In `Dec` è **identico** all'originale (mantissa + punto + esponente scientifico; `_` resta
  un errore di sintassi). In `Hex/Oct/Bin` consuma cifre valide nella base + separatori `_`,
  niente punto/esponente. Il guard del match richiede che il char d'innesco sia una cifra vera
  (mai `_` → un buffer `"_FF"` dà Syntax). I valori sono letti via `u64::from_str_radix` e
  reinterpretati in complemento a due (`as i64 as f64`) così `FFFFFFFFFFFFFFFF` = -1.
- **8 simboli Unicode dedicati** (mai parole testuali — "AND" inizierebbe per 'A', che in Hex
  è una cifra): `∧ ∨ ⊻ ≪ ≫ ↺ ↻` + `¬` (NOT, via `FuncId::Not`, prefisso come √/∛).
- **Precedenza C-like** sopra `expr`: `or_expr` > `xor_expr` > `and_expr` > `shift_expr` > `expr`.
  Entry point di `parse_with_base` = `or_expr`. `shift_expr` gestisce 4 token (shift E rotate,
  stesso livello). I nodi riusano `Expr::Bin` — zero nuove varianti di `Expr`.
- **Valutazione** `evaluate_with_width(e, mode, width)`: `width` limita SOLO la legalità dello
  shift e il modulo della rotazione, NON maschera i risultati (compito del formatter, §6).
  `to_i64_checked` (pub(crate)) valida il dominio intero i64; operandi non-interi → `f64::NAN`.
  Shift via `checked_shl`/`checked_shr` (mai `<<`/`>>` grezzi), rotate con maschera a `width` bit.
  NOT (`FuncId::Not`) → `!n as f64` — la mascheratura a valle è del formatter.

### format.rs — smart output

| Condizione | Output |
|-----------|--------|
| `x == 0.0` | `"0"` |
| `x.fract() == 0.0` | `"{x as i64}"` (no decimali) |
| `abs ∈ [1e-6, 1e12)` | float con zeri finali tagliati |
| fuori range | `"{m}e{e}"` con mantissa trimmata |
| NaN o Inf | `"Error"` |

### Modalità programmatore — Parte B (format_integer_in_base)

`format_integer_in_base(x, base, width) -> Option<String>` (`format.rs`):
formattazione in Hex/Oct/Bin con mascheratura alla larghezza, zero-padding e raggruppamento `_`.

| Base | Cifre per larghezza | Raggruppamento |
|---|---|---|
| Hex | width/4 (Byte=2, Word=4, Dword=8, Qword=16) | ogni 4, `_` |
| Bin | width (Byte=8, Word=16, Dword=32, Qword=64) | ogni 4, `_` |
| Oct | ⌈width/3⌉ (Byte=3, Word=6, Dword=11, Qword=22) | nessuno |

Non chiamabile con `NumBase::Dec` (unreachable — la larghezza bit è inerte in Dec).
Usa `to_i64_checked` (pub(crate)) per la validazione intero. Il troncamento degli
overflow di input è intenzionale (comportamento Windows Calculator).

### render.rs — HTML 2D (Task 2 aggiornato, Task vA aggiornato)

La divisione produce una frazione impilata con le classi `lare-frac*`:

```html
<span class="lare-frac">
  <span class="lare-frac-num">{numeratore}</span>
  <span class="lare-frac-bar"></span>
  <span class="lare-frac-den">{denominatore}</span>
</span>
```

Le **costanti** rendono il simbolo Unicode direttamente: `Const(Pi)` → `"π"`, `Const(E)` → `"e"`.

Le **funzioni** rendono `nome(arg)`:

| FuncId | Output |
|--------|--------|
| Sin/Cos/Tan | `sin(…)` / `cos(…)` / `tan(…)` |
| Asin/Acos/Atan | `asin(…)` / `acos(…)` / `atan(…)` |
| Log / Ln | `log(…)` / `ln(…)` |
| Sqrt | `√(…)` (simbolo ∛, non la parola "sqrt") |
| Cbrt | `∛(…)` (simbolo ∛ U+221B, Task vA) |

Le **potenze** rendono un apice 2D via HTML superscript:

```html
<span class="lare-pow">
  <span class="lare-pow-base">{base}</span>
  <sup class="lare-pow-exp">{exp}</sup>
</span>
```

La base è parentesizzata tramite `operand(base, 3)` — la soglia `3` corrisponde a `prec(^)`, l'unico valore forzato-corretto (>2 per wrappare `Mul`/`Add`; ≤3 per non wrappare `Num`/`Const`/`Func`). L'esponente non viene mai parentesizzato (il `<sup>` lo raggruppa). Esempi: `(2+3)^2` → base `(2 + 3)`; `5^2` → base `5`; `π^2` → base `π`.

Il **fattoriale** (Task vA) rende `{operand}!`:
- `operand` è parentesizzato se `prec(inner) < 3`: `(2+3)!`, `(2×3)!` con parens; `5!` senza.
- `prec(Factorial)` = 3 (atomico postfisso; come `Pow`, `Func`).

Il **modulo** (Task vA) rende inline `{l} % {r}` (stesso stile di `×`).
- `prec(Bin{Mod, ..})` = 2 (stesso livello di `Mul`/`Div`): `(2×3)%4` → `"2 × 3 % 4"` (nessuna parentesi necessaria).

Gli operatori `+ − × %` sono inline. Le parentesi si mostrano **solo** dove la precedenza le richiede: `(2+3)*4` → `"(2 + 3) × 4"`; `2+3*4` → `"2 + 3 × 4"`.

### main.rs — plugin (Task 3 + Refinements v2 Task vB)

```
CalcState {
    buf: String,
    last_was_result: bool,
    last_expr: String,
    shift: bool,           // flag "2nd/Shift" sticky
    angle_mode: AngleMode, // Deg (default) | Rad
}
```

**`append_str(state, text, is_fresh)`** — helper per i rami scientifici.
Centralizza la regola "cancella buffer se last_was_result e is_fresh".
- `is_fresh=true`: funzioni/costanti che iniziano un nuovo input (√, ∛, sin, cos, tan, log, ln, π, e).
- `is_fresh=false`: operatori/postfissi che continuano dal risultato (^, ^-1, ^2, ^3, ^(1/, !, %).

`handle_key(state, key)` — mappa `data-evt` in operazioni sul buffer:

| data-evt | Azione | is_fresh |
|---------|--------|---------|
| `d0`..`d9`, `dot` | appende cifra/punto | (logica inline) |
| `op_add`/`sub`/`mul`/`div` | appende `+`/`−`/`×`/`÷` | (logica inline) |
| `paren_open`/`close` | appende `(`/`)` | (logica inline) |
| `eq` | cattura `last_expr`, poi `parse`→`evaluate(state.angle_mode)`→`format_number` | — |
| `clear` | svuota `buf`, resetta `last_was_result` e `last_expr` | — |
| `back` | rimuove ultimo char | — |
| `shift` | toglie/attiva `state.shift` (return immediato, non soggetto a sticky-off) | — |
| `mode` | alterna `state.angle_mode` Deg ↔ Rad | — |
| `fn_sin/cos/tan` | `"sin("/…` o `"asin("/…` se shift | `true` |
| `fn_log` | `"log("` o `"10^("` se shift | `true` |
| `fn_ln` | `"ln("` o `"e^("` se shift | `true` |
| `fn_sqrt` | `"√("` (normale) o `"∛("` (shift, v2) | `true` (entrambi) |
| `fn_square` (v2) | `"^2"` (normale) o `"^3"` (shift) | `false` |
| `op_pow` | `"^"` (normale) o `"^(1/"` (shift, v2) | `false` |
| `fn_recip` | `"^-1"` | `false` |
| `fn_factorial` (v2) | `"!"` (postfisso) | `false` |
| `fn_mod` (v2) | `"%"` (modulo) | `false` |
| `const_pi` | `"π"` (nessuna 2ª funzione in v2) | `true` |
| `const_e` (v2) | `"e"` (costante di Eulero, tasto separato) | `true` |

**Sticky Shift rule** (fondo di `handle_key`): dopo qualsiasi tasto non-`shift`, se `was_shifted` era true → `state.shift = false`.

`render_window(state)` — genera la finestra `<div class="lare-window lare-calc">`.
Struttura HTML (v2): **display → expr → status → key_grid**:
- **Display** (`.lare-display`): 2D se il buffer parserizza, lineare altrimenti.
- **Eco** (`.lare-expr`): input grezzo verbatim (NO più `<span class="lare-mode">`).
  Dopo `=`: mostra `last_expr`. Mentre si digita: mostra `buf`.
- **Riga stato** (`.lare-status`): indicatore `DEG`/`RAD` corrente — tra l'eco e la griglia.
  Sempre visibile, non scorre con il testo. Riservato a etichette di stato future.
- **Griglia** (`.lare-key-grid`): generata da `fn key_grid(state)` — **7×5 = 35 celle singole**,
  etichette shift-aware, classe `lare-key--shift-on` sul pulsante "2nd" quando `state.shift=true`.
  Nessun `lare-key--wide`.

`key_grid(state)` — layout 7 righe × 5 colonne (v2):
```
2nd | DEG/RAD | sin/asin | cos/acos | tan/atan
x²/x³ | √/∛ | x^y/y√x | log/10^x | ln/e^x
1/x | n! | mod | π | e
7   | 8  | 9   | C | ÷
4   | 5  | 6   | ⌫ | ×
1   | 2  | 3   | ( | −
0   | .  | =   | ) | +
```
Il tasto DEG/RAD ha **etichetta fissa "DEG/RAD"** (indica il toggle; la modalità corrente
è in `.lare-status`). Tutti gli operatori ÷ × − + sono celle singole (nessun span).

### Protocollo (modellato su plugin-counter)

```
Init { .. }              → Ready { name: "calc", protocol_version: 1 }
Activate { window_id }   → ShowWindow { window_id, title: "Calcolatrice", html: render_window(default) }
UiEvent { element_id }   → UpdateWindow { window_id, html: render_window(dopo handle_key) }
Deinit {}                → (break loop)
```

### Main loop

Identico a `plugin-counter`: blocking `BufRead::lines()` su stdin, una riga JSON per messaggio, flush su ogni risposta, uscita pulita su Deinit.

## Tests (TDD)

| Modulo | Test | Comportamento |
|--------|------|---------------|
| `engine` | `precedence` | `2+3*4 == 14` |
| `engine` | `parens_override` | `(2+3)*4 == 20` |
| `engine` | `unary_minus` | `-3+2 == -1`; `2*-3 == -6` |
| `engine` | `decimals` | `0.5+0.25 == 0.75` |
| `engine` | `unicode_ops` | `6÷3×2 == 4`; `5−2 == 3` |
| `engine` | `div_by_zero` | `1/0 → DivByZero` |
| `engine` | `empty_and_syntax` | `""→Empty`; `"2+"→Syntax`; `"(2+3"→Syntax` |
| `engine` | `ast_shape` | `6/3 → Bin{Div, Num(6), Num(3)}` |
| `engine` | `exponent_literals_parse` | `4.5e-9`, `1.23e15`, `1e12` come letterali esponenziali |
| `engine` | `continue_from_exponential_value` | `1e12+3 == 1000000000003` (C-1, valore pinnato) |
| `engine` | `malformed_exponent_is_syntax_not_panic` | `"2e"`, `"3e+"` → `Syntax`, senza panico |
| `engine` | `trig_deg` | `sin(30)≈0.5`, `cos(0)=1`, `asin(0.5)≈30` in DEG |
| `engine` | `trig_rad` | `sin(0)=0` in RAD |
| `engine` | `sqrt_log_ln` | `√(9)=3`, `log(100)=2`, `ln(e)≈1` |
| `engine` | `power_basic` | `2^10=1024` |
| `engine` | `power_negative_exponent` | `5^-1=0.2`, `2^-3=0.125` |
| `engine` | `power_unary_minus_outside` | `-3^2=-9` (unario fuori dalla potenza) |
| `engine` | `power_right_assoc` | `2^3^2=512` (destra-assoc: `2^(3^2)=2^9`) |
| `engine` | `constants_pi_e` | `parse("π")=Const(Pi)`; `π≈3.14159`; `e≈2.71828` |
| `engine` | `e_disambiguation` | `parse("1e5")=Num(100000)`; `ev("e")≈E` |
| `engine` | `unknown_identifier_is_syntax` | `parse("foo(2)")=Err(Syntax)` |
| `engine` | `sqrt_negative_is_nan` | `ev("√(-1)").map(is_nan)=Ok(true)` |
| `engine` | `cbrt_basic` | `∛(27)≈3`, `∛(8)≈2` (Task vA) |
| `engine` | `factorial_integers` | `5!=120`, `0!=1`, `1!=1` (Task vA) |
| `engine` | `factorial_tighter_than_pow` | `3!^2=36`, `2^3!=64` (fattoriale > potenza) |
| `engine` | `factorial_invalid_is_nan` | `(-1)!`→NaN, `2.5!`→NaN (Task vA) |
| `engine` | `modulo_basic` | `7%3=1`, `2×3%4=2`, `2%3×4=8` (discriminante livello moltiplicativo) |
| `engine` | `modulo_by_zero` | `7%0→DivByZero` (Task vA) |
| `engine` | `eighth_root_via_pow` | `8^(1/3)≈2` (test di regressione y√x esistente) |
| `format` | `zero` | `0.0 → "0"` |
| `format` | `integer_no_decimals` | `4.0 → "4"`; `-12.0 → "-12"` |
| `format` | `plain_float_trimmed` | `1.5 → "1.5"`; `0.75 → "0.75"` |
| `format` | `small_uses_exp` | `4.5e-9 → contiene 'e'` |
| `format` | `large_uses_exp` | `1.23e15 → contiene 'e'` |
| `format` | `boundary_below_1e12_is_plain` | `999_999_999_999.0 → "999999999999"` |
| `format` | `nan_inf_error` | `NAN/INF → "Error"` |
| `format` | `boundary_at_1e12_is_exp` | `1e12 → "1e12"` (I-1, confine alto) |
| `format` | `boundary_at_1e_minus_6_is_plain` | `1e-6 → "0.000001"` (I-1, confine basso incluso) |
| `format` | `just_below_1e_minus_6_is_exp` | `9.9e-7 → contiene 'e'` (I-1, sotto il confine) |
| `render` | `simple_fraction` | `"6/3"` → HTML con `lare-frac-num/bar/den` |
| `render` | `fraction_of_expressions_no_parens` | num+den senza parentesi, con `+` e `×` inline |
| `render` | `parens_only_where_needed` | `(2+3)*4` → ha `(`; `2+3*4` → non ha `(` |
| `render` | `inline_ops_and_unary` | `"5-2"→"5 − 2"`; `"-3"→"−3"` |
| `render` | `render_const_pi` | `Expr::Const(Pi)` → `"π"` |
| `render` | `render_const_e` | `Expr::Const(E)` → `"e"` |
| `render` | `render_func_sin` | `parse("sin(30)")` → `"sin(30)"` |
| `render` | `render_func_sqrt` | `parse("√(9)")` → `"√(9)"` |
| `render` | `render_pow_contains_lare_pow_classes` | `"2^3"` → HTML con `lare-pow`, `lare-pow-exp`, `2`, `3` |
| `render` | `render_pow_base_parens_for_low_prec` | `"(2+3)^2"` → base contiene `(2 + 3)` |
| `render` | `render_pow_no_parens_for_num_base` | `"5^2"` → no `(5)`, base è `5` nudo |
| `render` | `render_cbrt` | `parse("∛(27)")` → `"∛(27)"` (Task vA) |
| `render` | `render_factorial_simple` | `parse("5!")` → `"5!"` (Task vA) |
| `render` | `render_factorial_with_parens` | `parse("(2+3)!")` → `"(2 + 3)!"` (prec<3 → parens) |
| `render` | `render_mod` | `parse("7%3")` → `"7 % 3"` (Task vA) |
| `main` | `typing_builds_buffer` | `d7,op_mul,d8 → buf="7×8"` |
| `main` | `equals_computes_and_formats` | `d7,op_mul,d8,eq → buf="56"` |
| `main` | `equals_div_by_zero_is_error` | `d1,op_div,d0,eq → buf="Error"` |
| `main` | `backspace_and_clear` | `back` rimuove 1 char; `clear` svuota |
| `main` | `digit_after_result_starts_fresh` | `d2,eq,d5 → buf="5"` |
| `main` | `op_after_result_continues` | `d2,eq,op_add,d3,eq → buf="5"` |
| `main` | `window_has_display_and_keys` | HTML ha `lare-display`, `lare-key-grid`, `lare-frac`, `data-evt="eq"` |
| `main` | `window_is_marked_calc` | la finestra porta la classe `lare-window lare-calc` (la UI vi aggancia il display grow-only) |
| `main` | `incomplete_shows_linear` | buffer incompleto `"6÷"` → no `lare-frac` |
| `main` | `continue_from_exponential_result_does_not_error` | `1000000×1000000=` poi `+3=` → buf ≠ `"Error"` e ri-parserizzabile (C-1) |
| `main` | `engine_round_trips_its_own_formatter_output` | `parse(format_number(x)) ≈ x` (incl. valori esponenziali) |
| `main` | `eq_captures_last_expr` | dopo `d7,op_mul,d8,eq`: `last_expr = "7×8"` |
| `main` | `equals_keeps_expression_in_echo` | HTML prodotto da `eq` contiene sia `"7×8"` (eco) sia `"56"` (display) |
| `main` | `echo_shows_raw_buffer_while_typing` | HTML mentre si digita `6÷4` contiene `"6÷4"` nell'eco |
| `main` | `digit_after_result_resets_echo` | dopo `eq+digit`, l'HTML non contiene la vecchia espressione nell'eco |
| `main` | `clear_resets_last_expr` | dopo `eq,clear`: `last_expr = ""` |
| `main` | `key_grid_has_data_keys` | `key_grid(&CalcState::default())` ha le mappature fisiche (`"7"`, `"= Enter"`, `"Backspace"`, `"Escape Delete"`, `"* x"`, `". ,"`) |
| `main` | `shift_toggle` | `keys(&["shift"]).shift==true`; doppio → false |
| `main` | `shift_fn_sin_appends_asin_and_turns_off` | shift+fn_sin → buf `"asin("`, shift off |
| `main` | `fn_sin_no_shift_appends_sin` | fn_sin → buf `"sin("` |
| `main` | `mode_toggle` | `mode` alterna Deg→Rad→Deg |
| `main` | `shift_fn_sqrt_appends_cbrt_or_sqrt` | shift+fn_sqrt → `"∛("` (v2); normale → `"√("` |
| `main` | `const_pi_appends_pi_no_shift` | const_pi → `"π"`; shift+const_pi → `"π"` (nessuna 2ª funzione) |
| `main` | `fn_square_appends_pow2_or_pow3` | fn_square → `"^2"`; shift → `"^3"` (v2) |
| `main` | `shift_op_pow_appends_yroot` | op_pow → `"^"`; shift → `"^(1/"` (v2) |
| `main` | `const_e_appends_e` | const_e → `"e"` (v2, tasto separato) |
| `main` | `fn_factorial_appends_bang` | fn_factorial → `"!"` (v2) |
| `main` | `fn_mod_appends_percent` | fn_mod → `"%"` (v2) |
| `main` | `factorial_five_equals_120` | end-to-end `5! = 120` (v2) |
| `main` | `modulo_seven_mod_three_equals_one` | end-to-end `7%3 = 1` (v2) |
| `main` | `cbrt_twenty_seven_equals_three` | end-to-end `∛(27) = 3` via shift+fn_sqrt (v2) |
| `main` | `render_has_lare_status_with_mode_indicator` | `lare-status` presente; "DEG/RAD" nel button mode; no `lare-mode` nell'eco (v2) |
| `main` | `op_pow_appends_caret_and_fn_recip_appends_inv` | op_pow → `"^"`; fn_recip → `"^-1"` |
| `main` | `sin30_deg_evaluates_to_half` | `fn_sin,d3,d0,paren_close,eq` in Deg → buf `"0.5"` |
| `main` | `render_shows_deg_by_default_and_shift_on_class` | render contiene `"DEG"` e `lare-key-grid`; con shift: `lare-key--shift-on` |

## Manifest

```json
{"name":"Calcolatrice","id":"calc","version":"1.0.0","protocol_version":1,"triggers":{"command":"/calc"}}
```

## Note di design

- **Estensione additiva:** le nuove varianti `Const`, `Func`, `Pow` sono state aggiunte all'enum `Expr`
  senza rompere nessuno dei test esistenti. Il tokenizer e il parser sono stati sostituiti in blocco
  (grammatica più ricca), ma la semantica di tutto il codice base (aritmetica, parentesi, unario) è invariata.
- **`e` disambiguation:** l'ordine dei branch del tokenizer (numerico → alfabetico) garantisce che `1e5`
  sia sempre `Num(100000)`, mai `[Const(E), ...]`. Questo è il punto più delicato dell'implementazione.
- **Dominio fuori range → NaN, non Err:** `√(-1)`, `ln(-1)`, `log(0)` producono NaN IEEE 754 che
  `format_number` converte in "Error". Mantiene `CalcError` minimo (solo errori strutturali).
- **AST-centrico:** lo stesso albero serve sia a `evaluate` sia a `render` — design conscio, documentato nei commenti.
- **Input incompleto → display lineare:** `"6÷"` non parserizza → display grezzo. Il 2D appare solo quando l'AST è valido.
- **Render scientifico (Task 2):** `Const/Func/Pow` ora rendono π/e/nome(arg)/apice 2D HTML.
  `prec(^)=3` è il valore forzato-corretto: wrappa Add/Mul come base (`(2+3)^2`) ma non Num/Const
  (`5^2`, `π^2`).
- **Shift sticky + DEG/RAD (Task 3):** `evaluate` usa `state.angle_mode` (non più hardcoded Rad).
  `fn key_grid(state)` è dinamica: 5 colonne, etichette shift-aware, classe shift-on. Il renderer
  include `<span class="lare-mode">` nell'eco. Helper `append_str` elimina la duplicazione tra i rami scientifici.
