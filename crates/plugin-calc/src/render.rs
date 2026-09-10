//! Render the AST to 2D HTML (display). Division is rendered as a stacked fraction;
//! `+ − ×` are inline; parentheses appear ONLY where precedence requires them.
//!
//! HTML classes used (defined in `plugin-catalog.css`):
//!   `.lare-frac`      — outer container for the stacked fraction
//!   `.lare-frac-num`  — numerator (top)
//!   `.lare-frac-bar`  — horizontal bar separating num from den
//!   `.lare-frac-den`  — denominator (bottom)
//!
//! Numbers are formatted via the smart formatter (`format::format_number`).
//!
//! # Precedence and parentheses
//! The AST already encodes evaluation order — there is no ambiguity at the semantic level.
//! The question for the renderer is purely visual: when does the reader *need* parentheses
//! to correctly understand the printed expression?
//!
//! Rule: wrap a child in `(…)` only when its display precedence is strictly LOWER than the
//! context (parent operator) precedence.  Same-or-higher → no parens.
//!
//! Examples:
//!   `(2+3)*4` — AST is `Mul{Add{2,3},4}`. Add has prec=1, Mul context has prec=2.
//!               1 < 2 → wrap Add: "(2 + 3) × 4". ✓
//!   `2+3*4`   — AST is `Add{2,Mul{3,4}}`. Mul has prec=2, Add context has prec=1.
//!               2 < 1 is false → no wrap: "2 + 3 × 4". ✓
//!   `6/3`     — Division renders as a fraction. The bar *groups* num and den,
//!               exactly like parentheses in standard math notation, so we never
//!               add extra parens inside a fraction. ✓

use crate::engine::{BinOp, ConstId, Expr, FuncId};
// Nota: `BinOp::Mod` e `FuncId::Cbrt` e `Expr::Factorial` sono usati nei nuovi bracci sotto.
use crate::format::format_number;

/// Returns the display precedence of an expression node.
///
/// Higher value = binds tighter (like in algebra):
///   7  — Num, Neg, Const, Func, Pow, Factorial (atomic / self-delimiting)
///   6  — Mul / Div / Mod
///   5  — Add / Sub
///   4  — Shl / Shr / Rol / Ror (shift e rotazione, stesso livello)
///   3  — And
///   2  — Xor
///   1  — Or
///
/// In OOP terms this would be a virtual method on an `Expr` interface;
/// in Rust we use a plain function on the enum.
///
/// Nota: i valori numerici sono puramente relativi (conta solo l'ordine) — la
/// rinumerazione rispetto alla versione scientifica (3/2/1 → 7/6/5…) è invisibile
/// alle stringhe HTML prodotte, che sono ciò che i test verificano.
fn prec(e: &Expr) -> u8 {
    match e {
        // Atomic or already-prefixed: no wrapping needed as a child.
        Expr::Num(_) | Expr::Neg(_) => 7,
        // Multiplicative group: Mul, Div, Mod — tutti allo stesso livello.
        // Div rende come frazione (la barra raggruppa), Mod rende inline con `%`.
        Expr::Bin { op: BinOp::Mul | BinOp::Div | BinOp::Mod, .. } => 6,
        // Additive group.
        Expr::Bin { op: BinOp::Add | BinOp::Sub, .. } => 5,
        // Shift e rotazione — stesso livello (famiglia "riposiziona i bit").
        Expr::Bin { op: BinOp::Shl | BinOp::Shr | BinOp::Rol | BinOp::Ror, .. } => 4,
        // Bitwise AND.
        Expr::Bin { op: BinOp::And, .. } => 3,
        // Bitwise XOR.
        Expr::Bin { op: BinOp::Xor, .. } => 2,
        // Bitwise OR — precedenza più bassa di tutte.
        Expr::Bin { op: BinOp::Or, .. } => 1,
        // Const e Func sono nodi atomici (foglie o con delimitatori propri): non hanno mai
        // bisogno di essere avvolti in parentesi come figli di un altro operatore.
        //
        // Pow/Factorial: nodi auto-delimitanti (<span class="lare-pow">… / `!` suffisso).
        Expr::Const(_) | Expr::Func { .. } | Expr::Pow { .. } | Expr::Factorial(_) => 7,
    }
}

/// Renders `child` and wraps it in parentheses if its precedence is STRICTLY LOWER
/// than `parent_prec` (the precedence of the surrounding operator context).
fn operand(child: &Expr, parent_prec: u8) -> String {
    if prec(child) < parent_prec {
        // Wrap — the child binds less tightly than the context: ambiguous without parens.
        format!("({})", render(child))
    } else {
        // No wrap — same or higher precedence: the reader can unambiguously parse it.
        render(child)
    }
}

/// Renders an `Expr` AST node to an HTML string suitable for the calculator display.
///
/// - `Num(n)`             → smart-formatted number string (no HTML tags)
/// - `Neg(x)`             → "−{operand}" (U+2212)
/// - `Bin{Div, lhs, rhs}` → stacked fraction HTML (`lare-frac*` classes).
///   No extra parens around lhs/rhs because the bar itself groups them visually
/// - `Bin{Add, …}`        → "lhs + rhs" (space-separated, parens where needed)
/// - `Bin{Sub, …}`        → "lhs − rhs" (U+2212)
/// - `Bin{Mul, …}`        → "lhs × rhs" (U+00D7)
pub fn render(e: &Expr) -> String {
    match e {
        // Leaf node: just the formatted number.
        Expr::Num(n) => format_number(*n),

        // Unary minus: "−" (U+2212, the proper math minus sign) followed by the operand.
        // We pass parent_prec=7 (prec of atomic/self-delimiting nodes) so that a complex
        // child (Add, Sub, …) gets wrapped.
        // Example: render(Neg(Add{2,3})) → "−(2 + 3)".
        // Example: render(Neg(Num(3)))   → "−3" (Num has prec=7, 7 < 7 is false → no wrap).
        Expr::Neg(x) => format!("−{}", operand(x, 7)),

        // Division → stacked fraction.
        // The bar groups numerator and denominator exactly as parentheses would,
        // so we call `render(lhs)` / `render(rhs)` directly — no `operand()` wrapper.
        // This means "(2+3×4) / (3×5×2)" renders as the fraction
        //   num="2 + 3 × 4"  /  den="3 × 5 × 2"   with NO extra parentheses.
        Expr::Bin { op: BinOp::Div, lhs, rhs } => format!(
            "<span class=\"lare-frac\">\
               <span class=\"lare-frac-num\">{}</span>\
               <span class=\"lare-frac-bar\"></span>\
               <span class=\"lare-frac-den\">{}</span>\
             </span>",
            render(lhs),
            render(rhs)
        ),

        // Costanti matematiche: π (U+03C0) e e (Eulero).
        // Sono nodi foglia — nessun argomento, nessuna ricorsione.
        Expr::Const(id) => match id {
            ConstId::Pi => "π".to_string(),
            ConstId::E  => "e".to_string(),
        },

        // Funzione matematica: nome(argomento).
        // L'argomento è reso ricorsivamente; non serve parentesizzare (le parentesi
        // letterali fanno già parte della sintassi della funzione: `sin(x)`, `√(x)`, `∛(x)`).
        // Per √ e ∛ usiamo il simbolo direttamente come prefisso: √(arg), ∛(arg).
        Expr::Func { id, arg } => {
            let nome = match id {
                FuncId::Sin  => "sin",
                FuncId::Cos  => "cos",
                FuncId::Tan  => "tan",
                FuncId::Asin => "asin",
                FuncId::Acos => "acos",
                FuncId::Atan => "atan",
                FuncId::Log  => "log",
                FuncId::Ln   => "ln",
                FuncId::Sqrt => "√",
                FuncId::Cbrt => "∛",   // U+221B, radice cubica (Refinements v2)
                FuncId::Not  => "¬",   // U+00AC, NOT bitwise (modalità programmatore)
            };
            format!("{}({})", nome, render(arg))
        }

        // Fattoriale postfisso: {operand}!
        //
        // L'operando viene parentesizzato se la sua precedenza è strettamente inferiore
        // a quella del fattoriale (prec=3). Esempi:
        //   `5!`        → prec(Num)=3, 3<3 è falso → "5!"         (nessuna parentesi) ✓
        //   `(2+3)!`    → prec(Add)=1, 1<3 è vero  → "(2 + 3)!"  (parentesi necessarie) ✓
        //   `(2×3)!`    → prec(Mul)=2, 2<3 è vero  → "(2 × 3)!"  (parentesi necessarie) ✓
        Expr::Factorial(inner) => {
            format!("{}!", operand(inner, 7))
        }

        // Potenza: apice 2D tramite HTML superscript.
        //
        // La BASE viene parentesizzata solo se la sua precedenza è strettamente inferiore
        // a 3 (la soglia che corrisponde a `prec(^)`) — vedi il commento in `prec` sopra.
        //   • `(2+3)^2`: base = Add{2,3}, prec=1 < 3 → `operand` aggiunge `(2 + 3)`. ✓
        //   • `5^2`: base = Num(5), prec=3, 3 < 3 → falso → base resa senza parentesi. ✓
        //   • `π^2`: base = Const(Pi), prec=3 → nessuna parentesi. ✓
        //   • `sin(x)^2`: base = Func{…}, prec=3 → nessuna parentesi. ✓
        //
        // L'ESPONENTE non viene mai parentesizzato: il tag <sup> lo raggruppa visivamente.
        Expr::Pow { base, exp } => format!(
            // Il wrapper lare-pow allinea base e apice verticalmente (CSS: align-items:flex-start).
            // lare-pow-base: la base, eventualmente parentesizzata.
            // lare-pow-exp:  l'esponente nel tag <sup> (font ridotto, in alto a destra).
            "<span class=\"lare-pow\">\
               <span class=\"lare-pow-base\">{}</span>\
               <sup class=\"lare-pow-exp\">{}</sup>\
             </span>",
            operand(base, 7),   // 7 = prec dei nodi atomici/auto-delimitanti
            render(exp)
        ),

        // Inline binary operators: Add, Sub, Mul, Mod.
        // (Div è gestito nel ramo precedente con la frazione impilata.)
        Expr::Bin { op, lhs, rhs } => {
            // Display precedence of THIS node — used to decide whether children need parens.
            let p = prec(e);
            // Map operator to display symbol (Unicode, not ASCII).
            let sym = match op {
                BinOp::Add => "+",
                BinOp::Sub => "−",  // U+2212
                BinOp::Mul => "×",  // U+00D7
                BinOp::Mod => "%",
                // ── Operatori bitwise (stessi glyph della tabella §2 — coerenza display/input)
                BinOp::And => "∧",  // U+2227
                BinOp::Or  => "∨",  // U+2228
                BinOp::Xor => "⊻",  // U+22BB
                BinOp::Shl => "≪",  // U+226A
                BinOp::Shr => "≫",  // U+226B
                BinOp::Rol => "↺",  // U+21BA
                BinOp::Ror => "↻",  // U+21BB
                // Div is handled above; this arm is unreachable but Rust requires exhaustiveness.
                BinOp::Div => unreachable!("Div handled in the previous arm"),
            };
            // Note on `operand(lhs, p)` for equal-precedence children:
            // Because the AST is built left-associatively (e.g., `2*3*4` → `Mul{Mul{2,3},4}`),
            // `lhs` of a Mul node can itself be a Mul node. prec(lhs)=2 == p=2 → no parens.
            // This gives "2 × 3 × 4" instead of "(2 × 3) × 4", which is correct visually.
            // Similarly, `(2×3)%4` → lhs=Mul, p(Mod)=2, p(Mul)=2 → no parens: "2 × 3 % 4". ✓
            format!("{} {} {}", operand(lhs, p), sym, operand(rhs, p))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{parse, ConstId, Expr};

    /// Helper: parse a string expression and render it to HTML.
    fn r(s: &str) -> String { render(&parse(s).unwrap()) }

    // ── Task 2: nuovi test per Const, Func, Pow ───────────────────────────────

    #[test]
    fn render_const_pi() {
        // La costante π costruita direttamente dall'AST → simbolo Unicode.
        // Questo test verifica che il braccio Const(Pi) di render sia corretto.
        assert_eq!(render(&Expr::Const(ConstId::Pi)), "π");
    }

    #[test]
    fn render_const_e() {
        // La costante e (Eulero) → "e" (una singola lettera).
        assert_eq!(render(&Expr::Const(ConstId::E)), "e");
    }

    #[test]
    fn render_func_sin() {
        // Funzione sin con argomento letterale → "sin(30)".
        // Nota: l'argomento è renderizzato ricorsivamente con render(arg).
        assert_eq!(r("sin(30)"), "sin(30)");
    }

    #[test]
    fn render_func_sqrt() {
        // Radice quadrata: il "nome" è √ (non "sqrt"), seguito dall'argomento tra parentesi.
        assert_eq!(r("√(9)"), "√(9)");
    }

    #[test]
    fn render_pow_contains_lare_pow_classes() {
        // 2^3: l'HTML prodotto deve contenere le classi lare-pow e lare-pow-exp,
        // e i valori 2 (base) e 3 (esponente) devono essere presenti nel testo.
        let h = r("2^3");
        assert!(h.contains("lare-pow"), "atteso 'lare-pow' in: {h}");
        assert!(h.contains("lare-pow-exp"), "atteso 'lare-pow-exp' in: {h}");
        assert!(h.contains('2'), "atteso '2' in: {h}");
        assert!(h.contains('3'), "atteso '3' in: {h}");
    }

    #[test]
    fn render_pow_base_parens_for_low_prec() {
        // (2+3)^2: la base è Add{2,3} con prec=1 < prec(^)=3 → deve essere parentesizzata.
        // HTML atteso per la base: "(2 + 3)".
        let h = r("(2+3)^2");
        assert!(h.contains("(2 + 3)"), "atteso '(2 + 3)' come base parentesizzata: {h}");
    }

    #[test]
    fn render_pow_no_parens_for_num_base() {
        // 5^2: la base è Num(5) con prec=3, uguale alla soglia di ^ → nessuna parentesi.
        // L'HTML non deve contenere "(5)".
        let h = r("5^2");
        assert!(!h.contains("(5)"), "non atteso '(5)' per base atomica: {h}");
        assert!(h.contains('5'), "atteso '5' nella base: {h}");
    }

    #[test]
    fn simple_fraction() {
        // 6/3 → stacked fraction with num, bar, den.
        let h = r("6/3");
        assert!(h.contains("lare-frac-num") && h.contains("lare-frac-bar") && h.contains("lare-frac-den"));
        assert!(h.contains(">6<") && h.contains(">3<"));
    }

    #[test]
    fn fraction_of_expressions_no_parens() {
        // (2+3×4)/(3×5×2): numerator and denominator rendered inline, NO parens
        // (the fraction bar groups them, just like real math notation).
        let h = r("(2+3*4)/(3*5*2)");
        assert!(h.contains("lare-frac"));
        assert!(!h.contains('('));   // no parentheses around num/den
        assert!(h.contains("2 + 3 × 4") && h.contains("3 × 5 × 2"));
    }

    #[test]
    fn parens_only_where_needed() {
        // (2+3)*4 → parens needed because Add has lower precedence than Mul.
        assert!(render(&parse("(2+3)*4").unwrap()).contains("(2 + 3) × 4")); // needed
        // 2+3*4  → no parens needed (Mul already binds tighter than Add in the AST).
        assert!(!render(&parse("2+3*4").unwrap()).contains('('));             // not needed
    }

    #[test]
    fn inline_ops_and_unary() {
        // Subtraction uses the display minus sign (U+2212).
        assert_eq!(render(&parse("5-2").unwrap()), "5 − 2");
        // Unary minus uses the display minus sign, no parens for a bare number.
        assert_eq!(render(&parse("-3").unwrap()), "−3");
    }

    // ── Refinements v2 — render per ∛, n!, % ─────────────────────────────────

    /// Test render della radice cubica: ∛(27) → "∛(27)".
    /// Il simbolo ∛ (U+221B) è usato come prefisso, uguale a come √ rende `√(…)`.
    #[test]
    fn render_cbrt() {
        assert_eq!(r("∛(27)"), "∛(27)", "render di ∛(27) deve essere '∛(27)'");
    }

    /// Test render del fattoriale semplice: 5! → "5!".
    /// Il numero atomico non richiede parentesi (sua prec = 3).
    #[test]
    fn render_factorial_simple() {
        assert_eq!(r("5!"), "5!", "render di 5! deve essere '5!'");
    }

    /// Test render del fattoriale con argomento a bassa precedenza: (2+3)! → "(2 + 3)!".
    /// Add ha prec=1 < prec(Factorial)=3 → `operand` aggiunge le parentesi.
    #[test]
    fn render_factorial_with_parens() {
        assert_eq!(r("(2+3)!"), "(2 + 3)!", "render di (2+3)! deve parentesizzare l'argomento additivo");
    }

    /// Test render del modulo: 7%3 → "7 % 3" (inline, con spazi).
    #[test]
    fn render_mod() {
        assert_eq!(r("7%3"), "7 % 3", "render di 7%3 deve essere '7 % 3'");
    }

    // ── Modalità programmatore — render degli operatori bitwise (parte C §11) ──
    // In Dec gli stessi glyph funzionano comunque (bonus del design §1):
    // questi test verificano la mappa `sym`/`nome` dei nuovi rami.

    #[test]
    fn render_bitwise_binary_ops() {
        assert_eq!(r("5∧3"), "5 ∧ 3", "AND inline con spazi");
        assert_eq!(r("5∨3"), "5 ∨ 3", "OR inline con spazi");
        assert_eq!(r("5⊻3"), "5 ⊻ 3", "XOR inline con spazi");
        assert_eq!(r("5≪3"), "5 ≪ 3", "SHL inline con spazi");
        assert_eq!(r("5≫3"), "5 ≫ 3", "SHR inline con spazi");
        assert_eq!(r("5↺3"), "5 ↺ 3", "ROL inline con spazi");
        assert_eq!(r("5↻3"), "5 ↻ 3", "ROR inline con spazi");
    }

    #[test]
    fn render_not_bitwise() {
        assert_eq!(r("¬(5)"), "¬(5)", "NOT unario: ¬(5)");
    }

    #[test]
    fn render_shift_wider_than_add_no_parens() {
        // `1≪2+3` parsa come `1≪(2+3)` (shift più largo di `+`, precedenza C-like).
        // Nel render, operand(rhs=Add{2,3}, p=prec(Shl)=4): prec(Add)=5 NON < 4 →
        // nessuna parentesi intorno alla somma. L'AST già codifica la semantica;
        // il render la riflette solo con la precedenza (vedi docstring di `render`).
        assert!(!r("1≪2+3").contains('('),
            "1≪2+3 non richiede parentesi: prec(Add)=5 >= prec(Shl)=4");
    }
}
