# Changelog — plugin-calc

All notable changes to this crate will be documented here.
Format: [Keep a Changelog](https://keepachangelog.com/en/1.0.0/). Semver from `0.1.0`.

## [2.3.1] — 2026-09-10 — Fix: cifre fuori dall'alfabeto della base accettate nel buffer

Segnalato da Maurizio dal vivo, due casi concreti: in modalità Bin, il tasto "2" veniva accettato
dopo "1101" (invalido, solo "0"/"1" sono cifre binarie); in modalità Dec, i tasti esadecimali
A-F (es. "C") venivano accettati anch'essi. In entrambi i casi il buffer accumulava un letterale
non valido, rilevato solo al successivo "=" (mostrando "Error").

### Fixed
- **`is_valid_digit_for_base(c, base)`** (`main.rs`, nuova funzione): cifre/lettere esadecimali/
  punto fuori dall'alfabeto della base numerica attiva vengono ora ignorate silenziosamente al
  momento della pressione del tasto — mai inserite nel buffer. Gli operatori/parentesi restano
  sempre validi indipendentemente dalla base (invariato). I tasti restano tutti visibili
  (nessun tasto nascosto/disabilitato in UI — semplicemente non succede nulla alla pressione,
  come un tasto disabilitato su una calcolatrice fisica).
- **Regola "smart clear after result" estesa alle cifre esadecimali**: prima usava solo
  `is_ascii_digit()`, quindi una cifra A-F dopo un risultato in Hex concatenava invece di
  iniziare un nuovo input (incoerente col comportamento già corretto delle cifre 0-9).
- **Riga di stato: la base è sempre mostrata, anche `DEC`** — prima la modalità Dec non
  mostrava alcuna etichetta di base (solo l'angolo), incoerente con Hex/Oct/Bin che la
  mostrano sempre. La larghezza bit resta l'unica cosa nascosta in Dec (è inerte lì).

- **Layout tasti esadecimali A-F riorganizzato** (`programmer_key_grid`, `main.rs`): ordine
  precedente (A-B-C-D-E-F letto dall'alto verso il basso) sostituito con l'ordine richiesto da
  Maurizio, coerente con la tastiera decimale esistente — contando le 4 righe della sezione
  programmatore DAL BASSO, A parte dalla 3ª riga e prosegue a destra (A→B→C) poi in alto alla
  4ª riga/cima (D→E→F), esattamente come 0 in fondo e 9 in cima a destra nella tastiera
  decimale. Riga base (DEC/HEX/OCT/BIN/NOT) e riga larghezza (BYTE/WORD/DWORD/QWORD/ROL-ROR)
  riorganizzate di conseguenza nelle due righe restanti, sotto il blocco esadecimale.

### Tests
- 4 nuovi test TDD (RED→GREEN): `bin_mode_rejects_digit_2` (il caso esatto segnalato),
  `oct_mode_rejects_digits_8_and_9`, `hex_digits_rejected_outside_hex_mode`,
  `hex_digit_after_result_starts_fresh`. Un test esistente rinominato/esteso:
  `status_line_dec_shows_only_deg` → `status_line_dec_shows_deg_and_dec`.
- 2 nuovi test per il layout A-F: `hex_digit_order_matches_decimal_keypad_convention`,
  `hex_block_sits_above_base_and_width_rows`.
- 135 test totali (129 esistenti + 6 nuovi), tutti verdi. `cargo clippy`: 0 warning.

## [2.3.2] — 2026-09-10 — Fix: NOT isolato dal blocco booleano, SHL/SHR lontano da ROL/ROR

Segnalato da Maurizio dal vivo (screenshot allegato): NOT stava da solo nella riga base
(DEC/HEX/OCT/BIN/NOT), separato da AND/OR/XOR; SHL/SHR stava nella riga A/B/C, lontano da
ROL/ROR (stessa famiglia concettuale — shift e rotazione).

### Fixed
- **NOT spostato accanto a XOR** (colonna 5, riga A-B-C): i 4 operatori booleani (AND/OR/XOR/NOT)
  formano ora un blocco 2×2 (AND/OR sopra, XOR/NOT sotto), invece di avere NOT isolato nella riga
  base.
- **SHL/SHR spostato nella riga base** (colonna 5, dove prima stava NOT): ora subito sopra
  ROL/ROR (stessa colonna, righe adiacenti) — shift e rotazione impilati insieme.
- Solo uno scambio di posizione di due `data-evt` nel markup di `programmer_key_grid` — nessuna
  logica di `handle_key` toccata.

### Tests
- 1 nuovo test: `boolean_ops_grouped_and_shift_above_rotate`.
- 136 test totali, tutti verdi. `cargo clippy`: 0 warning.

## [2.3.0] — 2026-09-10 — Modalità programmatore, Parte C (UI, tasti, CSS, conversione)

### Added
- **`CalcState::{base_mode, bit_width, prog_visible}`** (`main.rs`): tre nuovi campi di
  stato con default Dec/Qword/false via `#[derive(Default)]`.
- **`try_convert_buf`**: helper che riformatta il buffer nella nuova base/larghezza quando
  valuta correttamente (comportamento "conversione" stile Windows Calculator).
- **Nuovi rami `handle_key`**: `base_dec`/`hex`/`oct`/`bin`, `width_byte`/`word`/`dword`/`qword`,
  `toggle_prog`, `op_and`/`op_or`/`op_xor`, `fn_not` (¬(, prefisso), `op_shift` (≪/≫ via 2nd),
  `op_rotate` (↺/↻ via 2nd). Cifre esadecimali `hexA`–`hexF` nel ramo `ch: Option<char>`.
- **`eq` base-aware**: ramifica su `state.base_mode` — in Dec usa `format_number`, in
  Hex/Oct/Bin usa `format_integer_in_base`.
- **`programmer_key_grid(state)`**: griglia 4 righe × 5 colonne (20 tasti) con layout DEC/HEX/
  OCT/BIN/NOT, A–F, AND/OR/XOR/SHL–SHR, BYTE/WORD/DWORD/QWORD/ROL–ROR. Etichette shift-aware
  (SHL↔SHR, ROL↔ROR).
- **Display lineare in Hex/Oct/Bin**: `render_window` ora evita `render::render` quando
  `base_mode != Dec` — evita che `parse("E+1")` venga letto come Const(E)+Num(1).
- **Riga stato estesa**: mostra `base_label · width_label` quando `base_mode != Dec`; la
  larghezza bit non compare mai in Dec (è inerte). Toggle `▼ PROG`/`▲ PROG` nella riga stato.
- **CSS** (`plugin-catalog.css`): `.lare-prog-section` (bordo ambra, sfondo caldo),
  `.lare-prog-toggle` (controllo cliccabile nella riga stato). Zero codice JS toccato.
- **Nuovi test render** (`render_bitwise_binary_ops`, `render_not_bitwise`,
  `render_shift_wider_than_add_no_parens`): coprono i nuovi rami di `render.rs`.
- **12 nuovi test main** TDD (RED → GREEN): `toggle_prog_visibility`,
  `prog_section_not_visible_by_default`, `prog_section_visible_after_toggle`,
  `convert_dec_to_hex_qword`, `convert_dec_to_hex_byte`, `integrated_programmer_sequence`,
  `incomplete_expression_no_conversion`, `overflow_input_truncates`,
  `status_line_dec_shows_only_deg`, `status_line_hex_shows_base_and_width`,
  `not_bitwise_end_to_end`, `shift_end_to_end`.

## [2.2.0] — 2026-09-10 — Modalità programmatore, Parte B (format_integer_in_base)

### Added
- **`format_integer_in_base(x, base, width) -> Option<String>`** (`format.rs`): formattazione
  di un f64 come intero senza segno in Hex/Oct/Bin. Mascheratura alla larghezza bit (padding
  a cifre piene, troncamento degli overflow di input — comportamento Windows Calculator).
  Zero-padding SEMPRE alla larghezza piena (¬(0) a Byte in Hex = "FF", non "F").
  Raggruppamento `_` ogni 4 cifre per Hex/Bin; Oct senza raggruppamento. Non chiamabile
  con `NumBase::Dec` (la larghezza bit è inerte in Dec — vedi §6).
- **`group_every(s, n, sep)`**: helper privato — inserisce sep ogni n caratteri contando
  DA DESTRA, così il gruppo corto arriva all'inizio. Usata per il raggruppamento `_`.
- **8 nuovi test TDD** (RED → GREEN): `format_hex_byte_255_is_ff`,
  `format_hex_qword_255_is_sixteen_digits_grouped`, `format_oct_byte_8_is_010`,
  `format_bin_byte_5_is_grouped`, `format_not_zero_hex_byte_ff`,
  `format_not_zero_hex_qword_all_f`, `format_overflow_truncates_silently`,
  `format_non_integer_returns_none`.

## [2.1.0] — 2026-09-10 — Modalità programmatore, Parte A (engine: basi numeriche, bitwise, shift/rotate)

Prima parte del compito `Docs/i18n/ita/compiti-ai-esterne/2026-09-10-calc-modalita-programmatore.md`.
Estende `engine.rs` con il supporto alle basi non-decimali (Hex/Oct/Bin) e agli operatori
bitwise/shift/rotate, mantenendo il comportamento decimale **byte-identico**.

### Added (Parte A — engine base-aware)
- **`NumBase`** (`Dec | Hex | Oct | Bin`, default `Dec`) e **`BitWidth`**
  (`Byte | Word | Dword | Qword`, default `Qword`) — nuovi enum pubblici con `impl Default`
  e `radix()`/`bits()`.
- **`tokenize_with_base(s, base)`**: sostituisce il `tokenize` esistente. In `Dec` il branch
  numerico è **identico** all'originale (mantissa + punto + esponente scientifico, nessun
  separatore `_`); in `Hex/Oct/Bin` consuma cifre valide nella base + separatori `_`, niente
  punto né esponente. Il guard del branch richiede che il primo char sia una cifra vera (mai `_`).
- **8 nuovi token a simbolo singolo** (glyph Unicode dedicati, mai parole testuali per non
  collidere con le cifre esadecimali A-F): `∧` AND, `∨` OR, `⊻` XOR, `≪` SHL, `≫` SHR,
  `↺` ROL, `↻` ROR, e `¬` NOT (via `FuncId::Not`, prefisso come √/∛).
- **`BinOp::{And, Or, Xor, Shl, Shr, Rol, Ror}`** — riusano il nodo `Expr::Bin` esistente.
- **`FuncId::Not`** — NOT bitwise unario, gestito dal ramo `Func '(' expr ')'` già esistente.
- **Nuovi livelli di precedenza C-like** (`or_expr` → `xor_expr` → `and_expr` → `shift_expr` →
  `expr` esistente): shift/rotate > AND > XOR > OR. Entry point di `parse_with_base` = `or_expr`.
- **`parse_with_base(s, base)`** (nuova) + `parse(s)` come wrapper su `NumBase::Dec`.
- **`evaluate_with_width(e, mode, width)`** (nuova) + `evaluate(e, mode)` come wrapper su
  `BitWidth::Qword`. Valutazione bitwise con `to_i64_checked` (dominio invalido → NaN, mai Err);
  shift con `checked_shl`/`checked_shr` (mai `<<`/`>>` grezzi, niente panic); rotate con
  riduzione modulo `width`; l'ammontare di shift valido dipende da `width` (`1≪9` valido a
  Qword, NaN a Byte).
- **`to_i64_checked(v)`** (`pub(crate)`): conversione esatta f64 → i64 con confini di range.

### Tests (Parte A — 15 nuovi, RED → GREEN)
`bitwise_and_or_xor_values`, `shift_values`, `not_bitwise_value`,
`precedence_and_tighter_than_or`, `precedence_shift_wider_than_add`,
`precedence_and_wider_than_shift`, `bitwise_works_in_dec`, `to_i64_checked_boundaries`,
`shift_amount_depends_on_width`, `rotate_byte_values`, `non_decimal_literal_parsing`,
`underscore_separator_only_non_decimal`, `shift_right_value`, più i test discriminanti di
precedenza. I 91 test esistenti restano verdi **senza essere stati modificati**.

### Changed (Parte A — necessario per compilare)
- **`render.rs`**: `prec()` rinumerata (atomici 7, `×÷%` 6, `+−` 5, shift 4, AND 3, XOR 2, OR 1)
  e nuovi rami per i 7 `BinOp` + `FuncId::Not`. Necessario perché i match di `render`/`prec`
  sono esaustivi: senza questi rami il crate non compila. Le soglie `operand(…, 3)` di
  `Neg`/`Factorial`/`Pow` sono state aggiornate a `7` (prec dei nodi atomici) per preservare
  l'output HTML esistente.

## 2.0.0 — 2026-09-05 — fork da v1 0.2.0

Copia del crate dalla v1 (`mauriziolobello/lare-terminal`) nel repo 2.0. Nessuna modifica
funzionale in questa voce; le modifiche del piano 1 seguono nelle voci successive.

## [0.2.0] — 2026-06-28

### Added (snap-to-zero)
- **`format_number`**: i residui floating-point minuscoli (`|x| < 1e-12`, es. `cos(90°)` in gradi
  = `6.12e-17`) vengono mostrati come `"0"` invece che in notazione esponenziale. Euristica di
  display (valori legittimi ≥ 1e-12 non toccati). Test `snap_to_zero_tiny_residuals`.

### Added (Task vB — layout 7×5, nuovi tasti UI, Shift v2, riga stato DEG/RAD)

Refinements v2: rework completo di `key_grid`, `handle_key` e `render_window` in `main.rs`.
Nessuna modifica a `engine.rs`, `render.rs`, `format.rs`.

**Layout UI:**
- **Griglia 7×5 = 35 tasti singoli** (nessun `lare-key--wide`): tutti gli operatori ÷ × − +
  tornano a cella singola. Il layout passa da struttura mista (3+4-col con span) a 5 colonne
  uniformi che dividono esattamente 35 celle — nessun auto-flow CSS indesiderato.
- **Riga stato `.lare-status`** inserita tra `.lare-expr` (eco) e la griglia tasti.
  Contiene l'indicatore `DEG`/`RAD`. Rimossa la vecchia `<span class="lare-mode">` dall'eco
  — l'indicatore non è più parte del testo dell'espressione.
- **Etichetta tasto DEG/RAD fissa**: il pulsante `data-evt="mode"` ora mostra sempre
  "DEG/RAD" (indica che è un toggle); la modalità corrente è nella riga `.lare-status`.
- **Tasti separati `π` e `e`**: `const_pi` (π) e `const_e` (e, nuovo) sostituiscono
  il vecchio singolo `const_pi` con shift → e.

**Nuovi tasti (`data-evt`):**
- `fn_square`: `^2` (normale) / `^3` con Shift — operatore, `is_fresh=false`.
- `const_e`: appende `e` (costante di Eulero) — `is_fresh=true`.
- `fn_factorial`: appende `!` — postfisso, `is_fresh=false`.
- `fn_mod`: appende `%` — operatore moltiplicativo, `is_fresh=false`.

**Shift v2 — coppie modificate:**
| Tasto | Normale | Shift (v2) | Cambiamento |
|---|---|---|---|
| `fn_sqrt` | `√(` (`is_fresh=true`) | `∛(` (`is_fresh=true`) | Prima: `^2` (`is_fresh=false`) |
| `op_pow` | `^` | `^(1/` (y√x) | Prima: nessuna 2ª funzione |
| `const_pi` | `π` | — | Prima: `e` via shift; ora tasto separato |

**`handle_key` — nuovi rami:**
- `fn_square` → `^2`/`^3` (is_fresh=false)
- `fn_factorial` → `!` (is_fresh=false)
- `fn_mod` → `%` (is_fresh=false)
- `const_e` → `e` (is_fresh=true)
- `fn_sqrt`: entrambe le varianti ora `is_fresh=true` (∛( è funzione prefissa come √()
- `op_pow`: Shift → `^(1/` (is_fresh=false)
- `const_pi`: rimossa la logica `if was_shifted { "e" }` — appende sempre `"π"`

**Tests (TDD) — 11 nuovi RED → GREEN:**
- `fn_square_appends_pow2_or_pow3` — `fn_square` → `"^2"`; shift → `"^3"`.
- `shift_op_pow_appends_yroot` — shift+op_pow → `"^(1/"`.
- `const_e_appends_e` — `const_e` → `"e"`.
- `fn_factorial_appends_bang` — `fn_factorial` → `"!"`.
- `fn_mod_appends_percent` — `fn_mod` → `"%"`.
- `factorial_five_equals_120` — end-to-end `5! = 120`.
- `modulo_seven_mod_three_equals_one` — end-to-end `7%3 = 1`.
- `cbrt_twenty_seven_equals_three` — end-to-end `∛(27) = 3` (shift+fn_sqrt).
- `render_has_lare_status_with_mode_indicator` — `lare-status` presente; "DEG/RAD" nel
  pulsante mode; `lare-mode` rimosso dall'eco.

**Tests aggiornati (comportamento cambiato):**
- `shift_fn_sqrt_appends_pow2_or_sqrt` → rinominato `shift_fn_sqrt_appends_cbrt_or_sqrt`;
  asserzione cambiata da `"^2"` a `"∛("`.
- `shift_const_pi_appends_e_or_pi` → rinominato `const_pi_appends_pi_no_shift`;
  rimossa la asserzione shift→`"e"`, aggiunta shift→`"π"` (nessuna 2ª funzione).

### Added (Task vA — engine v2: radice cubica ∛, fattoriale n!, modulo %)

Refinements v2 post-accettazione live: tre nuovi costrutti dell'engine + render,
stessa branch `feat/calc-scientific`. No cambio di versione (pre-merge, additive).

- **`FuncId::Cbrt`** (`src/engine.rs`): radice cubica. Tokenizer: `∛` (U+221B) →
  `Token::Func(FuncId::Cbrt)`. `evaluate` → `a.cbrt()`. `render` → `∛({arg})`.
- **`Expr::Factorial(Box<Expr>)`** (`src/engine.rs`): fattoriale postfisso `n!`.
  Tokenizer: `!` → `Token::Bang`. Grammatica: nuovo livello `postfix` inserito tra
  `power` e `atom` → `power := postfix ('^' unary)?`, `postfix := atom ('!')*`.
  Il fattoriale lega più stretto di `^`: `3!^2 = (3!)^2 = 36`; `2^3! = 2^(3!) = 64`.
  `evaluate`: intero non-negativo → prodotto `1·2·…·n` in f64 (loop con early-exit
  su `is_infinite()` per prevenire iterazioni inutili su argomenti enormi); negativo
  o non-intero → `Ok(f64::NAN)` → "Error" nel formatter. `render` → `{operand}!`
  (parentesi sull'operando solo se prec < 3: `(2+3)!` sì, `5!` no).
- **`BinOp::Mod`** (`src/engine.rs`): operatore modulo `%`, livello moltiplicativo.
  Tokenizer: `%` → `Token::Percent`. Parser `term`: accetta `Percent` insieme a
  `Star`/`Slash` (left-assoc, stessa precedenza di `×` e `÷`). `evaluate`: `l % r`
  (f64 remainder); `r == 0.0` → `Err(CalcError::DivByZero)`. `render` → `{l} % {r}`.
- **`prec()` aggiornato** (`src/render.rs`): `BinOp::Mod` inserito nel ramo
  moltiplicativo (`prec = 2`); `Expr::Factorial` → `prec = 3` (atomico postfisso).
- **7 nuovi test TDD** (RED: 10 FAILED / 72 passed prima dell'implementazione;
  `eighth_root_via_pow` era GREEN immediato → test di regressione/conferma):
  `cbrt_basic`, `factorial_integers`, `factorial_tighter_than_pow`,
  `factorial_invalid_is_nan`, `modulo_basic` (incl. test discriminante `2%3×4=8`),
  `modulo_by_zero`, `eighth_root_via_pow` — in `engine::tests`.
  4 test render: `render_cbrt`, `render_factorial_simple`, `render_factorial_with_parens`,
  `render_mod` — in `render::tests`.
- **1 test di caratterizzazione** (post-hoc, derogazione TDD documentata):
  `factorial_large_overflows_to_infinity` — verifica che `200!` restituisca `+Inf`
  e pinna il guard `if acc.is_infinite() { break; }` nel loop del fattoriale.
  Il guard è necessario per prevenire hang su input enormi (`1e20!` → `v as u64`
  satura a `u64::MAX`); è stato aggiunto prima del test perché la necessità era
  evidente a compile-time, ma va caratterizzato esplicitamente per conformità TDD.
- **Nessuna modifica a `main.rs`**: `handle_key` usa solo pattern `&str`; i nuovi
  button/event (`fn_cbrt`, `fn_factorial`, `fn_mod`) sono deferred a **Task vB**
  (layout v2, riga `1/x | n! | mod | π | e`). Stub: nessuno necessario.

### Added (Task 4 — CSS tasti compatti 5-col + indicatore DEG/RAD + apice)
- **`plugin-catalog.css`** (crate `ui`): nuove classi calcolatrice:
  - `.lare-key-grid`: griglia **5 colonne** (da 4) con `gap: 5px` — ospita i 5 tasti scientifici per riga.
  - `.lare-key`: tasti compatti (`font-size: 15px`, `padding: 8px 0` — da 20px/14px).
  - `.lare-key--shift-on`: sfondo blu (`rgba(80,160,240,0.55)`) quando `state.shift=true`.
  - `.lare-mode`: indicatore DEG/RAD — `font-size: 0.8em`, colore muted, `letter-spacing: 0.1em`.
  - `.lare-pow` / `.lare-pow-exp`: wrapper apice 2D — `inline-flex` + `font-size: 0.7em; line-height: 1`.

### Added (Task 3 — Shift sticky + DEG/RAD + tasti scientifici, parte di 0.2.0)
- **`state.shift: bool`** in `CalcState`: flag "2nd function" sticky — attivato dal tasto
  `shift` (`data-evt="shift"`), azzerato automaticamente dopo il primo tasto non-shift.
- **`state.angle_mode: AngleMode`** in `CalcState` (default `Deg` via nuovo `impl Default for
  AngleMode` in `engine.rs`): modalità angolare commutata da `data-evt="mode"`.
- **`impl Default for AngleMode`** (`engine.rs`): `AngleMode::Deg` come default, necessario
  per fare derivare `Default` su `CalcState` dopo l'aggiunta del campo.
- **Nuovi rami `handle_key`** (9 nuovi `data-evt`):
  - `shift` → toglie/attiva `state.shift` (return immediato, no sticky-off).
  - `mode` → alterna `state.angle_mode` Deg ↔ Rad.
  - `fn_sin/cos/tan` → appende `"sin("`/`"cos("`/`"tan("` oppure `"asin("`/`"acos("`/`"atan("` se shift.
  - `fn_log` → appende `"log("` oppure `"10^("` se shift.
  - `fn_ln` → appende `"ln("` oppure `"e^("` se shift.
  - `fn_sqrt` → appende `"√("` (nuovo input, `is_fresh=true`) o `"^2"` (operatore, `is_fresh=false`) se shift.
  - `op_pow` → appende `"^"` (operatore).
  - `fn_recip` → appende `"^-1"` (operatore).
  - `const_pi` → appende `"π"` oppure `"e"` (costante di Eulero) se shift.
- **Regola "sticky shift"**: ogni tasto non-shift azzera `state.shift` al termine del proprio
  ramo — implementato in un unico punto alla fine di `handle_key` (`if was_shifted { state.shift = false; }`).
- **Helper `append_str(state, text, is_fresh)`** (`main.rs`): centralizza la logica
  "cancella buffer se last_was_result e is_fresh" usata da tutti i rami scientifici (DRY/SRP).
- **Fix `"eq"` hardcoded `AngleMode::Rad`**: sostituito con `state.angle_mode` — ora sin(30)
  in modalità DEG valuta correttamente 0.5.
- **`fn key_grid(state: &CalcState) -> String`** — sostituisce la `const KEY_GRID: &str`.
  Layout 5 colonne: 3 righe scientifiche sopra il blocco numerico a 4 colonne.
  Etichette shift-aware (sin↔asin, √↔x², π↔e, log↔10^x, ln↔e^x).
  Il tasto "2nd/Shift" ottiene la classe `lare-key--shift-on` quando `state.shift=true`.
- **Indicatore DEG/RAD in `render_window`**: `<span class="lare-mode">DEG|RAD</span>` nel
  `.lare-expr` (a sinistra del testo eco), aggiornato in tempo reale.
- **8 nuovi test TDD** (RED: 8 FAILED / 61 passed prima dell'implementazione):
  `shift_toggle`, `shift_fn_sin_appends_asin_and_turns_off`, `fn_sin_no_shift_appends_sin`,
  `mode_toggle`, `shift_fn_sqrt_appends_pow2_or_sqrt`, `shift_const_pi_appends_e_or_pi`,
  `op_pow_appends_caret_and_fn_recip_appends_inv`, `sin30_deg_evaluates_to_half`.
  1 test aggiuntivo `render_shows_deg_by_default_and_shift_on_class` — GREEN immediato
  perché la UI era già stata cablata durante lo scaffolding strutturale (Phase 1).
- **2 test di copertura a posteriori** (GREEN immediati, aggiunti dopo il review del gap):
  `fn_cos_and_fn_tan_append_correctly`, `fn_log_and_fn_ln_append_correctly` — coprono le
  stringhe letterali di `acos(`, `atan(`, `10^(`, `e^(` che condividono la struttura di
  `fn_sin` ma producono token diversi; un typo sarebbe stato silenzioso senza test dedicati.
  *Deviazione TDD documentata*: il codice esisteva già; i test caratterizzano il comportamento.
- **Test `key_grid_has_data_keys` aggiornato**: usa `key_grid(&CalcState::default())`
  invece del vecchio `KEY_GRID` costante.

### Added (Task 2 — render scientifico, parte di 0.2.0)
- **`render::render` — nuovi bracci `Const`, `Func`, `Pow`** (`src/render.rs`):
  - `Const(Pi)` → `"π"` (U+03C0); `Const(E)` → `"e"`.
  - `Func{id, arg}` → `"{nome}({render(arg)})"` con `nome` = `sin|cos|tan|asin|acos|atan|log|ln|√`
    (per `Sqrt` il prefisso è il simbolo `√` direttamente: `√(9)`).
  - `Pow{base, exp}` → apice 2D HTML:
    `<span class="lare-pow"><span class="lare-pow-base">{base}</span><sup class="lare-pow-exp">{exp}</sup></span>`.
    La base è parentesizzata via `operand(base, 3)` (prec(^)=3): `Add`/`Sub`/`Mul`/`Div` come base
    ricevono parentesi; `Num`/`Const`/`Func`/`Pow` no.
- **`prec()` — commento finale su `Pow`**: rimosso il `// TODO(Task2)`, aggiunto commento che
  spiega perché `prec(Pow)=3` è il valore forzato-corretto (deve essere >2 e ≤3).
- **Import** `ConstId, FuncId` aggiunto al `use crate::engine::{…}` di `render.rs`.
- **7 nuovi test TDD** (RED: 7 FAILED / 53 passed prima dell'implementazione):
  `render_const_pi`, `render_const_e`, `render_func_sin`, `render_func_sqrt`,
  `render_pow_contains_lare_pow_classes`, `render_pow_base_parens_for_low_prec`,
  `render_pow_no_parens_for_num_base`.

### Added (Task 1 — engine scientifico, parte di 0.2.0)
- **`ConstId`, `FuncId`, `AngleMode`** (`src/engine.rs`): nuovi enum pubblici.
  `ConstId`: `Pi`, `E`. `FuncId`: `Sin Cos Tan Asin Acos Atan Log Ln Sqrt`.
  `AngleMode`: `Deg | Rad` — passato a `evaluate` per la conversione trig.
- **Estensione additiva di `Expr`**: varianti `Const(ConstId)`, `Func{id,arg}`, `Pow{base,exp}`.
- **Tokenizer esteso**: riconosce `^` → `Caret`; `π` (U+03C0) → `Const(Pi)`; `√` (U+221A) →
  `Func(Sqrt)`; run alfabetico `[A-Za-z]+` → funzione nota / costante (`e`=Const(E), `pi`=Const(Pi))
  / `Err(Syntax)`. Il branch numerico resta PRIMA del branch alfabetico: `1e5` continua a essere
  tokenizzato come `Num(100000)` (l'esponente è già consumato dal branch numerico).
- **Nuova grammatica del parser** (`expr/term/unary/power/atom`):
  `^` è destra-associativo (rhs = `unary`); `-3^2 = -9` (unario fuori dalla potenza);
  `2^3^2 = 512`; `2^-3 = 0.125`.
- **`evaluate(e, AngleMode)`** (nuova firma con `mode`): trig dirette convertono gradi→rad;
  inverse convertono rad→gradi se `Deg`; `Pow` → `powf`; dominio fuori range → Ok(NaN).
- **Stub in `render.rs`** (Task 2 li completerà): bracci `Const/Func/Pow` in `prec` (→ 3)
  e `render` (→ `String::new()`).
- **Stub in `main.rs`**: `evaluate(&e, AngleMode::Rad)` ai due siti di chiamata
  (Task 3 sostituirà con `state.angle_mode`).
- **10 nuovi test TDD** (RED: 10 FAILED / 43 passed prima dell'implementazione):
  `trig_deg`, `trig_rad`, `sqrt_log_ln`, `power_basic`, `power_negative_exponent`,
  `power_unary_minus_outside`, `power_right_assoc`, `constants_pi_e`, `e_disambiguation`,
  `unknown_identifier_is_syntax`, `sqrt_negative_is_nan`.

### Changed (Task 1)
- `evaluate(&Expr)` → `evaluate(&Expr, AngleMode)`: firma aggiornata; tutti i test esistenti
  che la chiamavano dirittamente ora passano `AngleMode::Rad` (comportamento invariato).
- Helper `ev(s)` → `ev(s, mode)` nei test di `engine.rs`.

## [0.1.3] — 2026-06-28

### Changed
- **`render_window`**: la finestra ora è `<div class="lare-window lare-calc">` (aggiunta la
  classe `lare-calc`). Marca questa come la finestra calcolatrice così che la UI vi agganci il
  **display grow-only** (la `min-height` del display cresce in modo monotòno e non rimpicciolisce
  più → niente "saltellio" della finestra) senza toccare gli altri plugin. Test `window_is_marked_calc`.

## [0.1.2] — 2026-06-27

### Added
- **Attributo `data-key` su ogni pulsante di `KEY_GRID`** (`src/main.rs`): ogni `<button>`
  ora dichiara la lista di `KeyboardEvent.key` separati da spazio che lo attivano da tastiera
  fisica (es. `data-key="= Enter"` sul tasto `=`, `data-key="* x"` per `×`, `data-key=". ,"`
  per il punto decimale). Il `data-evt` e il contenuto visivo dei pulsanti restano invariati.
  Questo attributo è letto dal listener `keydown` in `plugin-window.js` (Task 3 Slice 2b)
  tramite `eventFromKey` (Task 1).
- **1 nuovo test TDD** `key_grid_has_data_keys` (RED confermato prima dell'implementazione):
  verifica che le mappature fondamentali siano presenti in `KEY_GRID` (`"7"`, `"= Enter"`,
  `"Backspace"`, `"Escape Delete"`, `"* x"`, `". ,"`).

## [0.1.1] — 2026-06-27

### Added
- **Riga eco "espressione digitata"** (`render_window`): tra il display e la griglia tasti
  ora appare `<div class="lare-expr">` che mostra il testo grezzo dell'input verbatim.
  Comportamento a due righe: dopo `=` il display mostra il risultato (es. `56`) mentre
  l'eco mostra l'espressione originale (es. `7×8`); mentre si digita l'eco rispecchia
  il buffer corrente.
- **Campo `last_expr: String` in `CalcState`**: catturato nel ramo `"eq"` di `handle_key`
  prima di sovrascrivere `buf` col risultato; azzerato da `"clear"`.
- **5 nuovi test TDD** (RED confermato prima dell'implementazione):
  `eq_captures_last_expr`, `equals_keeps_expression_in_echo`,
  `echo_shows_raw_buffer_while_typing`, `digit_after_result_resets_echo`,
  `clear_resets_last_expr`.

## [0.1.0] — 2026-06-27

### Added
- Initial crate: `plugin-calc` binary `calc` (Fase 1 Slice 2 — calcolatrice base).
- **Engine legge notazione esponenziale** (fix C-1): il tokenizer ora consuma letterali
  in notazione scientifica (`e`/`E` con segno opzionale e almeno una cifra d'esponente),
  consentendo il round-trip con il formatter — cioè continuare un calcolo a partire da
  un risultato molto grande/piccolo (es. `1e12 + 3`) senza ottenere `"Error"`.
  Esponenti malformati (`"2e"`, `"3e+"`) restano `CalcError::Syntax`, senza panico.
  8 nuovi test TDD (3 engine + 3 format + 2 main) coprono: parse di letterali esponenziali,
  continuazione dal risultato (engine + handle_key), guardia malformata, e le boundary del
  formatter (I-1).
- **`src/engine.rs`** — parser recursive-descent + AST:
  `Expr` (Num/Neg/Bin), `BinOp` (Add/Sub/Mul/Div), `CalcError` (Empty/Syntax/DivByZero);
  `parse(&str) -> Result<Expr, CalcError>` + `evaluate(&Expr) -> Result<f64, CalcError>`.
  Supporta `+ − × ÷ ()` e meno unario; simboli Unicode accettati (`×`/`÷`/`−`).
  9 test TDD (precedence, parentesi, unario, decimali, unicode, divByZero, errori, shape AST).
- **`src/format.rs`** — smart formatter:
  `format_number(f64) -> String` — `0`→`"0"`; intero→senza decimali; `[1e-6,1e12)`→float
  (zeri finali tagliati); fuori range→esponenziale; NaN/Inf→`"Error"`.
  7 test TDD (zero, intero, float, boundaries, exp, NaN/Inf).
- **`src/render.rs`** — renderer AST→HTML 2D:
  `render(&Expr) -> String` — `÷` produce frazione impilata (`lare-frac*` classes);
  `+ − ×` inline; parentesi solo dove la precedence le richiede.
  4 test TDD (frazione semplice, frazioni di espressioni senza parentesi, parentesi-only-where-needed, inline+unario).
- **`src/main.rs`** — plugin: `CalcState` + `handle_key` + `render_window` + main loop stdio:
  `handle_key(&mut CalcState, &str)` — mappa `data-evt` in azioni (cifre/operatori/`eq`/`clear`/`back`);
  regola "cifra dopo risultato → ricomincia"; regola "operatore dopo risultato → continua".
  `render_window(&CalcState) -> String` — display 2D se il buffer parserizza, lineare altrimenti.
  Griglia tasti statica 4×5 (`KEY_GRID`).
  7 test TDD + loop stdin modellato su `plugin-counter`.
- **`plugin.json`** — manifest: `id="calc"`, `triggers.command="/calc"`, `protocol_version=1`.
- `Cargo.toml` — `[[bin]] name="calc"`, deps: `plugin-protocol` + `serde_json`.
