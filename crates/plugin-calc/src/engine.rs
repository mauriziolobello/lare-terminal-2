//! Engine: parse di un'espressione aritmetica in un AST + valutazione.
//! AST-centrico: l'albero serve sia a `evaluate` (valore) sia a `render` (display 2D).
//! Estensibile: `+ − × ÷`, parentesi, meno unario, numeri (Slice 2a);
//! ora anche `Const` (π, e), `Func` (sin, cos, …, √), `Pow` (^) (Slice scientifica).

// ─── Tipi pubblici dell'AST ────────────────────────────────────────────────────

/// Costanti matematiche riconosciute dall'engine.
///
/// In OOP sarebbe un'enum/sealed class; in Rust usiamo un enum con `Copy` per evitare
/// allocazioni: i valori piccoli e discreti come questi non hanno bisogno di essere boxati.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConstId {
    /// π ≈ 3.14159… (U+03C0, inserita dal tasto π o scritta come "pi").
    Pi,
    /// Numero di Eulero e ≈ 2.71828… (inserito dal tasto e o scritto come "e").
    E,
}

/// Funzioni matematiche supportate dall'engine.
///
/// Le trig dirette (sin/cos/tan) interpretano l'argomento secondo `AngleMode`;
/// le inverse (asin/acos/atan) restituiscono il risultato nel sistema angolare corrente.
/// Log, Ln, Sqrt, Cbrt: argomento fuori dominio → NaN (non un errore esplicito).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FuncId {
    Sin, Cos, Tan,         // trig dirette
    Asin, Acos, Atan,      // trig inverse
    Log,                   // logaritmo in base 10
    Ln,                    // logaritmo naturale
    Sqrt,                  // radice quadrata (simbolo √, U+221A)
    Cbrt,                  // radice cubica (simbolo ∛, U+221B); Refinements v2
}

/// Modalità angolare per le funzioni trigonometriche.
///
/// `Deg` (gradi sessagesimali, default): sin(30°) = 0.5.
/// `Rad` (radianti): sin(π/6) = 0.5. Usata anche per tutti i test non-trig.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AngleMode { Deg, Rad }

impl Default for AngleMode {
    /// Default angolare: gradi (DEG), come la maggioranza delle calcolatrici fisiche.
    /// Necessario per far derivare `Default` su `CalcState` dopo l'aggiunta del campo
    /// `angle_mode: AngleMode` (Task 3).
    fn default() -> Self { AngleMode::Deg }
}

/// Nodo dell'AST (Abstract Syntax Tree).
/// In OOP sarebbe una classe astratta `Expr` con sottoclassi;
/// in Rust usiamo un enum con varianti tipate — compatto e zero-overhead.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// Numero letterale, es. `3.14` o `1e5`.
    Num(f64),
    /// Costante matematica (π o e).
    Const(ConstId),
    /// Meno unario: `-x`.
    Neg(Box<Expr>),
    /// Applicazione di funzione: `sin(x)`, `√(x)`, `∛(x)` ecc.
    Func { id: FuncId, arg: Box<Expr> },
    /// Operazione binaria: `lhs op rhs` (+ − × ÷ %).
    Bin { op: BinOp, lhs: Box<Expr>, rhs: Box<Expr> },
    /// Potenza: `base ^ exp` (destra-associativa).
    Pow { base: Box<Expr>, exp: Box<Expr> },
    /// Fattoriale postfisso: `n!`.
    /// Lega più stretto di `^`: la grammatica inserisce un livello `postfix`
    /// tra `power` e `atom` — `power := postfix ('^' unary)?`.
    /// Valutazione: se l'operando è un intero non-negativo, calcola il prodotto 1·2·…·n;
    /// altrimenti (negativo o non-intero) → `f64::NAN` (→ "Error" nel formatter).
    Factorial(Box<Expr>),
}

/// Operatori binari supportati.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    Add, Sub, Mul, Div,
    /// Modulo (resto della divisione intera): `l % r`.
    /// Stesso livello di precedenza di `×` e `÷` (livello `term`), left-associativo.
    /// `r == 0.0` → `CalcError::DivByZero` (come la divisione).
    Mod,
}

/// Errori del parser/valutatore.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CalcError {
    /// Stringa di input vuota.
    Empty,
    /// Errore sintattico (token inatteso, parentesi aperta non chiusa, identificatore sconosciuto, ecc.).
    Syntax,
    /// Divisione per zero rilevata durante la valutazione.
    DivByZero,
}

// ─── Tokenizer ────────────────────────────────────────────────────────────────

/// Token interno: unità minima riconosciuta dal tokenizer.
/// Privato al modulo — è un dettaglio implementativo del parser.
#[derive(Debug, Clone, PartialEq)]
enum Token {
    Num(f64),
    Plus, Minus, Star, Slash,
    LParen, RParen,
    /// `^` — operatore di elevazione a potenza (destra-assoc).
    Caret,
    /// `!` — fattoriale postfisso (Refinements v2).
    Bang,
    /// `%` — operatore modulo, livello moltiplicativo (Refinements v2).
    Percent,
    /// Costante matematica (π oppure e come identificatore standalone).
    Const(ConstId),
    /// Funzione matematica (sin, cos, …, ln, √, ∛).
    Func(FuncId),
}

/// Tokenizer: converte la stringa in una sequenza di Token.
/// Accetta sia ASCII (`* / -`) sia i simboli display Unicode (`× ÷ −`).
///
/// Ordine dei branch importante:
///   1. Whitespace, operatori semplici, parentesi, `^`, π, √ — O(1) per carattere.
///   2. Branch numerico (`c.is_ascii_digit() || c == '.'`) — consuma `1e5` INTERO
///      (incluso l'esponente scientifico) prima che l'`e` possa finire nel branch alfabetico.
///   3. Branch alfabetico (`c.is_ascii_alphabetic()`) — raccoglie un run `[A-Za-z]+` e lo
///      mappa a funzione nota / costante; identificatore sconosciuto → Err(Syntax).
///   4. `_` — qualsiasi altro carattere → Err(Syntax).
fn tokenize(s: &str) -> Result<Vec<Token>, CalcError> {
    // Raccogliamo i char in un Vec per poter accedere per indice.
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            // ── Whitespace ────────────────────────────────────────────────────
            ' ' | '\t' => i += 1,

            // ── Operatori aritmetici ASCII + Unicode ──────────────────────────
            '+' => { out.push(Token::Plus);  i += 1; }
            // '-' ASCII e '−' U+2212 (simbolo matematico "minus sign") sono equivalenti.
            '-' | '\u{2212}' => { out.push(Token::Minus); i += 1; }
            // '*' ASCII e '×' U+00D7 (multiplication sign).
            '*' | '\u{00D7}' => { out.push(Token::Star);  i += 1; }
            // '/' ASCII e '÷' U+00F7 (division sign).
            '/' | '\u{00F7}' => { out.push(Token::Slash); i += 1; }

            // ── Parentesi ─────────────────────────────────────────────────────
            '(' => { out.push(Token::LParen); i += 1; }
            ')' => { out.push(Token::RParen); i += 1; }

            // ── Operatore potenza e costanti/funzioni Unicode ─────────────────

            // '^' — elevazione a potenza (destra-associativa nel parser).
            '^' => { out.push(Token::Caret); i += 1; }

            // 'π' U+03C0 — costante pi greco (inserita dal tasto π della calcolatrice).
            '\u{03C0}' => { out.push(Token::Const(ConstId::Pi)); i += 1; }

            // '√' U+221A — funzione radice quadrata (inserita dal tasto √).
            // L'argomento segue come `√(expr)` nel parser (stessa sintassi di sin/cos).
            '\u{221A}' => { out.push(Token::Func(FuncId::Sqrt)); i += 1; }

            // '∛' U+221B — funzione radice cubica (Refinements v2: inserita dal tasto ∛).
            // L'argomento segue come `∛(expr)` nel parser, identico a √.
            '\u{221B}' => { out.push(Token::Func(FuncId::Cbrt)); i += 1; }

            // '!' — fattoriale postfisso (Refinements v2).
            // Viene dopo un atomo: `5!`, `(2+3)!`.
            '!' => { out.push(Token::Bang); i += 1; }

            // '%' — modulo, livello moltiplicativo (Refinements v2).
            // Stesso livello di `×` e `÷`; viene gestito in `term`.
            '%' => { out.push(Token::Percent); i += 1; }

            // ── Numero: cifre ASCII e punto decimale, con esponente scientifico ──
            //
            // NOTA CRITICA: questo branch DEVE venire prima del branch alfabetico.
            // Motivo: in "1e5" la 'e' è già consumata qui come parte dell'esponente.
            // Se il branch alfabetico venisse prima, "1e5" potrebbe essere tokenizzato
            // come [Num(1), Const(E), Num(5)] — errato!
            //
            // Una 'e' *standalone* (non preceduta da cifre) non fa mai partire questo
            // branch (le cifre lo avviano), quindi cade correttamente nel branch alfabetico.
            c if c.is_ascii_digit() || c == '.' => {
                let start = i;
                // Fase 1: consuma la mantissa (cifre + eventuale punto decimale).
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    i += 1;
                }

                // Fase 2: esponente scientifico opzionale (e/E, segno opzionale, ≥1 cifra).
                //
                // Perché serve: format_number PRODUCE notazione esponenziale ("1e12", "4.5e-9")
                // quando |x| ≥ 1e12 o |x| < 1e-6. Se l'utente preme = e poi un operatore,
                // handle_key costruisce es. "1e12+3" e lo manda al parser — che senza questo
                // blocco ritornerebbe CalcError::Syntax (bug C-1).
                //
                // Strategia "lookahead-guarded": consumiamo l'esponente SOLO se è ben formato
                // (e/E seguita da cifre, con segno opzionale). Se la 'e' è malformata (es. "2e"
                // o "3e+"), non consumiamo nulla — la 'e' rimane a `i` e cade nel branch
                // alfabetico → Const(E) → token in eccesso nel parser → Syntax (non panic).
                if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                    let mut j = i + 1; // j punta al char dopo 'e'
                    // Segno opzionale: '+' o '-' (solo se presente, poi j avanza oltre).
                    if j < chars.len() && (chars[j] == '+' || chars[j] == '-') {
                        j += 1;
                    }
                    // L'esponente è valido SOLO se c'è almeno una cifra dopo (segno compreso).
                    if j < chars.len() && chars[j].is_ascii_digit() {
                        // Esponente valido: avanza i fino alla fine delle cifre dell'esponente.
                        i = j;
                        while i < chars.len() && chars[i].is_ascii_digit() {
                            i += 1;
                        }
                        // i ora punta al primo char dopo l'esponente.
                    }
                    // (else: 'e' malformata → i NON avanza → la 'e' resta a chars[i]
                    //  → nell'iterazione successiva cade nel branch alfabetico → Const(E)
                    //  → token in eccesso nel parser → Syntax. Non panico, non crash.)
                }

                let lit: String = chars[start..i].iter().collect();
                out.push(Token::Num(lit.parse().map_err(|_| CalcError::Syntax)?));
            }

            // ── Identificatori: nomi di funzioni e costanti alfabetiche ──────
            //
            // Raccoglie un run contiguo di lettere ASCII [A-Za-z]+, poi mappa:
            //   sin/cos/tan/asin/acos/atan/log/ln → Func(FuncId)
            //   e                                  → Const(E)
            //   pi                                 → Const(Pi)
            //   qualsiasi altra parola             → Err(Syntax)
            //
            // ATTENZIONE: questo branch viene DOPO il numeric branch. In questo modo
            // "1e5" è consumato interamente dal branch numerico (esponente guardato);
            // solo una 'e' isolata (non preceduta da cifre) arriva qui → Const(E).
            c if c.is_ascii_alphabetic() => {
                let start = i;
                // Consuma tutto il run alfabetico.
                while i < chars.len() && chars[i].is_ascii_alphabetic() {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                // Il confronto è case-insensitive per accettare sia "sin" sia "Sin".
                let tok = match word.to_lowercase().as_str() {
                    "sin"  => Token::Func(FuncId::Sin),
                    "cos"  => Token::Func(FuncId::Cos),
                    "tan"  => Token::Func(FuncId::Tan),
                    "asin" => Token::Func(FuncId::Asin),
                    "acos" => Token::Func(FuncId::Acos),
                    "atan" => Token::Func(FuncId::Atan),
                    "log"  => Token::Func(FuncId::Log),
                    "ln"   => Token::Func(FuncId::Ln),
                    "e"    => Token::Const(ConstId::E),
                    "pi"   => Token::Const(ConstId::Pi),
                    // Qualsiasi altro identificatore è un errore sintattico.
                    _      => return Err(CalcError::Syntax),
                };
                out.push(tok);
            }

            // Qualsiasi altro carattere è un errore sintattico.
            _ => return Err(CalcError::Syntax),
        }
    }
    Ok(out)
}

// ─── Parser ───────────────────────────────────────────────────────────────────

/// Parser a discesa ricorsiva (recursive-descent).
///
/// Grammatica (precedenza crescente verso il basso):
///   expr  := term  (('+'|'−') term)*      — addizione/sottrazione (prec. più bassa)
///   term  := unary (('×'|'÷') unary)*     — moltiplicazione/divisione
///   unary := '−' unary | power            — meno unario (destra-assoc, ricorsivo)
///   power := atom ('^' unary)?            — potenza (destra-assoc; rhs è unary)
///   atom  := Num | Const | Func '(' expr ')' | '(' expr ')'   — atomo
///
/// Perché `power` chiama `unary` (non `power`) come rhs?
///   Questo rende `^` destra-associativo in modo naturale:
///     2^3^2 → power(atom=2, exp=unary→power(atom=3, exp=unary→power(atom=2)))
///           → Pow{2, Pow{3, 2}} → 2^9 = 512 ✓
///   E permette esponenti negativi:
///     2^-3 → power(atom=2, exp=unary=Neg(3)) → 2^(-3) = 0.125 ✓
///
/// Perché `unary` viene DOPO `term` nella grammatica?
///   Così `-3^2` parsa come `-(3^2) = -9`, non `(-3)^2 = 9`.
///   L'unario è applicato PRIMA di scendere nella potenza, avvolgendo il power risultante.
///     expr → term → unary: vede '-', ricorre → power(atom=3, exp=2) → Pow{3,2}
///     unary wraps → Neg(Pow{3,2}) → -(9) = -9 ✓
///
/// In OOP sarebbe un oggetto con stato mutabile (pos); in Rust usiamo `&mut self`.
struct Parser {
    toks: Vec<Token>,
    pos: usize,
}

impl Parser {
    /// Restituisce il token corrente senza consumarlo (lookahead di 1).
    fn peek(&self) -> Option<&Token> {
        self.toks.get(self.pos)
    }

    /// Consuma e restituisce il token corrente, avanzando la posizione.
    fn bump(&mut self) -> Option<Token> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }

    /// Livello addizione/sottrazione — precedenza più bassa.
    /// `lhs + rhs + rhs2` è left-associative: `(lhs + rhs) + rhs2`.
    fn expr(&mut self) -> Result<Expr, CalcError> {
        let mut lhs = self.term()?;
        loop {
            let op = match self.peek() {
                Some(Token::Plus)  => BinOp::Add,
                Some(Token::Minus) => BinOp::Sub,
                _ => break,
            };
            self.bump();
            let rhs = self.term()?;
            lhs = Expr::Bin { op, lhs: Box::new(lhs), rhs: Box::new(rhs) };
        }
        Ok(lhs)
    }

    /// Livello moltiplicazione/divisione/modulo — precedenza media.
    ///
    /// `term := unary (('×'|'÷'|'%') unary)*`
    ///
    /// Il modulo (`%`) è allo stesso livello di `×` e `÷`: left-associativo,
    /// cosicché `2×3%4 = (2×3)%4 = 2` e `2%3×4 = (2%3)×4 = 8`.
    /// Chiama `unary` (non più `factor`) per gestire meno unario e potenze.
    fn term(&mut self) -> Result<Expr, CalcError> {
        let mut lhs = self.unary()?;
        loop {
            let op = match self.peek() {
                Some(Token::Star)    => BinOp::Mul,
                Some(Token::Slash)   => BinOp::Div,
                Some(Token::Percent) => BinOp::Mod,
                _ => break,
            };
            self.bump();
            let rhs = self.unary()?;
            lhs = Expr::Bin { op, lhs: Box::new(lhs), rhs: Box::new(rhs) };
        }
        Ok(lhs)
    }

    /// Livello meno unario — si applica PRIMA di scendere nella potenza.
    ///
    /// `unary := '−' unary | power`
    ///
    /// La ricorsione (`'−' unary`) permette doppio negativo: `-(-3) = 3`.
    /// L'ordine grammaticale fa sì che `-3^2` = `-(3^2)` = `-9`, non `(-3)^2` = `9`:
    /// quando vediamo `−`, l'atomo del power non è ancora parsato, quindi l'unario
    /// avvolge l'intero `power` risultante.
    fn unary(&mut self) -> Result<Expr, CalcError> {
        if let Some(Token::Minus) = self.peek() {
            self.bump();
            Ok(Expr::Neg(Box::new(self.unary()?)))
        } else {
            self.power()
        }
    }

    /// Livello postfisso — tasti fattoriale (`!`), lega più stretto di `^`.
    ///
    /// `postfix := atom ('!')*`
    ///
    /// Più `!` possono concatenarsi: `3!!` = `(3!)!` = `720!` (esotico ma grammaticalmente valido).
    /// In pratica con argomenti grandi il loop esce comunque finito (f64 ∞ → NaN dopo un giro).
    ///
    /// Questo livello è interamente **nuovo** rispetto alla grammatica v1; non tocca nessun
    /// comportamento esistente: quando non c'è `!`, `postfix` è trasparente (ritorna l'atomo as-is).
    fn postfix(&mut self) -> Result<Expr, CalcError> {
        // Prima parsa l'atomo (foglia o espressione parentesizzata).
        let mut e = self.atom()?;
        // Poi consuma ogni `!` postfisso, avvolgendo via via l'espressione.
        while let Some(Token::Bang) = self.peek() {
            self.bump();
            e = Expr::Factorial(Box::new(e));
        }
        Ok(e)
    }

    /// Livello potenza — destra-associativo.
    ///
    /// `power := postfix ('^' unary)?`
    ///
    /// **Refinements v2**: il lhs è ora `postfix` (non più `atom`) in modo che il fattoriale
    /// leghi più stretto di `^`:
    ///   - `3!^2` → postfix = Factorial(3)=6, poi `^2` → Pow{Factorial(3), Num(2)} → 36.
    ///   - `2^3!` → il rhs di `^` è `unary` → `power` → `postfix` = Factorial(3)=6 → Pow{2, Factorial(3)} → 64.
    ///
    /// Il rhs rimane `unary` (non `power`) per mantenere `^` destra-associativo e permettere
    /// esponenti negativi (`2^-3 = 0.125`).
    fn power(&mut self) -> Result<Expr, CalcError> {
        // lhs: ora è postfix per rendere `!` più stretto di `^`.
        let base = self.postfix()?;
        if let Some(Token::Caret) = self.peek() {
            self.bump();
            // rhs è unary, non power → destra-assoc naturale (vedi docstring).
            let exp = self.unary()?;
            Ok(Expr::Pow { base: Box::new(base), exp: Box::new(exp) })
        } else {
            Ok(base)
        }
    }

    /// Atomo — livello più alto, nessuna ricorsione di precedenza.
    ///
    /// `atom := Num | Const | Func '(' expr ')' | '(' expr ')'`
    ///
    /// Per le funzioni: attende `(`, parsa `expr` (che può essere qualsiasi espressione,
    /// incluse somme e prodotti), poi attende `)`.
    /// Esempio: `sin(30+15)` → Func{Sin, Bin{Add, 30, 15}}.
    fn atom(&mut self) -> Result<Expr, CalcError> {
        match self.bump() {
            // Numero letterale: foglia dell'AST.
            Some(Token::Num(n)) => Ok(Expr::Num(n)),

            // Costante matematica (π o e): foglia dell'AST, nessun argomento.
            Some(Token::Const(id)) => Ok(Expr::Const(id)),

            // Funzione: Func '(' expr ')'.
            // Il token Func è già stato consumato da bump(); ora attende '('.
            Some(Token::Func(id)) => {
                // Attende la parentesi aperta obbligatoria.
                match self.bump() {
                    Some(Token::LParen) => {}
                    _ => return Err(CalcError::Syntax),
                }
                // Parsa l'argomento (qualsiasi espressione, incluse somme).
                let arg = self.expr()?;
                // Attende la parentesi chiusa.
                match self.bump() {
                    Some(Token::RParen) => Ok(Expr::Func { id, arg: Box::new(arg) }),
                    _ => Err(CalcError::Syntax),
                }
            }

            // Parentesi: parse dell'espressione interna, poi attende ')'.
            Some(Token::LParen) => {
                let e = self.expr()?;
                match self.bump() {
                    Some(Token::RParen) => Ok(e),
                    _ => Err(CalcError::Syntax), // parentesi non chiusa
                }
            }

            // Qualsiasi altro token in posizione di atomo è un errore.
            _ => Err(CalcError::Syntax),
        }
    }
}

// ─── API pubblica ─────────────────────────────────────────────────────────────

/// Parse di una stringa in un AST (tokenize + recursive descent).
/// Restituisce `CalcError::Empty` per stringa vuota e `CalcError::Syntax`
/// se ci sono token in eccesso dopo la fine dell'espressione.
pub fn parse(s: &str) -> Result<Expr, CalcError> {
    let toks = tokenize(s)?;
    if toks.is_empty() {
        return Err(CalcError::Empty);
    }
    let mut p = Parser { toks, pos: 0 };
    let e = p.expr()?;
    // Se ci sono token rimasti (es. "2 3" → l'AST è `2`, il `3` è in eccesso), è un errore.
    if p.pos != p.toks.len() {
        return Err(CalcError::Syntax);
    }
    Ok(e)
}

/// Valuta l'AST ricorsivamente, dato il modo angolare per le funzioni trig.
///
/// `mode` influenza solo le funzioni trigonometriche:
///   - Trig dirette (sin/cos/tan): se `Deg`, l'argomento viene convertito da gradi a radianti
///     prima di chiamare le funzioni f64 (che lavorano in radianti).
///   - Trig inverse (asin/acos/atan): il risultato f64 è in radianti; se `Deg`, viene
///     convertito a gradi prima di restituirlo.
///
/// Le altre funzioni (Log, Ln, Sqrt) e tutte le operazioni aritmetiche ignorano `mode`.
///
/// Argomenti fuori dominio (es. √(-1), ln(-1), log(0)):
///   Non sono trattati come errori: Rust restituisce NaN per queste operazioni f64,
///   e l'engine propaga Ok(NaN). Il formatter (`format_number`) converte NaN in "Error".
///   Questo mantiene il tipo di errore `CalcError` focalizzato su errori *strutturali*
///   (sintassi, divisione per zero), non su condizioni di dominio che dipendono dal valore.
pub fn evaluate(e: &Expr, mode: AngleMode) -> Result<f64, CalcError> {
    match e {
        // Foglie: numeri e costanti.
        Expr::Num(n) => Ok(*n),

        Expr::Const(id) => Ok(match id {
            ConstId::Pi => std::f64::consts::PI,
            ConstId::E  => std::f64::consts::E,
        }),

        // Meno unario: nega ricorsivamente il sottoalbero.
        Expr::Neg(x) => Ok(-evaluate(x, mode)?),

        // Applicazione di funzione: valuta l'argomento, poi applica la funzione.
        Expr::Func { id, arg } => {
            let a = evaluate(arg, mode)?;
            let result = match id {
                // Trig dirette: converti l'argomento gradi → radianti se siamo in DEG.
                FuncId::Sin => {
                    let r = if mode == AngleMode::Deg { a.to_radians() } else { a };
                    r.sin()
                }
                FuncId::Cos => {
                    let r = if mode == AngleMode::Deg { a.to_radians() } else { a };
                    r.cos()
                }
                FuncId::Tan => {
                    let r = if mode == AngleMode::Deg { a.to_radians() } else { a };
                    r.tan()
                }
                // Trig inverse: il risultato di asin/acos/atan è in radianti;
                // se siamo in DEG, convertiamo radianti → gradi prima di restituire.
                FuncId::Asin => {
                    let r = a.asin();
                    if mode == AngleMode::Deg { r.to_degrees() } else { r }
                }
                FuncId::Acos => {
                    let r = a.acos();
                    if mode == AngleMode::Deg { r.to_degrees() } else { r }
                }
                FuncId::Atan => {
                    let r = a.atan();
                    if mode == AngleMode::Deg { r.to_degrees() } else { r }
                }
                // Log/Ln/Sqrt/Cbrt: argomento fuori dominio → NaN (non Err).
                // f64::log10(x) con x ≤ 0 → NaN; sqrt(x) con x < 0 → NaN; ln(x) con x ≤ 0 → NaN.
                // cbrt(x) è definita per tutti i reali (incluso x < 0: ∛(-8) = -2 in f64).
                FuncId::Log  => a.log10(),
                FuncId::Ln   => a.ln(),
                FuncId::Sqrt => a.sqrt(),
                FuncId::Cbrt => a.cbrt(),
            };
            Ok(result)
        }

        // Potenza: base^exp tramite f64::powf.
        // powf gestisce correttamente valori negativi (es. 2.0.powf(-3.0) = 0.125).
        Expr::Pow { base, exp } => {
            Ok(evaluate(base, mode)?.powf(evaluate(exp, mode)?))
        }

        // Fattoriale postfisso: n! dove n deve essere un intero non-negativo.
        //
        // Strategia: valutiamo il sottoalbero, poi verifichiamo il dominio.
        //   - v < 0.0 o v.fract() != 0.0 → dominio invalido → NaN (come Sqrt(-1)).
        //   - v ∈ {0,1,...,170} → calcolo esatto tramite moltiplicazione f64.
        //   - v > 170 → il prodotto supera f64::MAX (170! ≈ 7.2e306) → diventa f64::INFINITY
        //     che il formatter converte in "Error". Il loop si interrompe appena il prodotto
        //     diventa infinito per evitare di iterare inutilmente su valori enormi.
        Expr::Factorial(inner) => {
            let v = evaluate(inner, mode)?;
            // Dominio: deve essere un intero non-negativo.
            if v < 0.0 || v.fract() != 0.0 {
                return Ok(f64::NAN);
            }
            // Calcolo: accumula il prodotto 1 × 2 × … × n in f64.
            // Il cast `v as u64` è sicuro perché abbiamo già verificato v >= 0 e v.fract()==0.
            // Per valori enormi (es. 1e300 come intero), v as u64 saturates a u64::MAX —
            // ma in pratica il prodotto diventa infinito molto prima (dopo 170 iterazioni).
            let n = v as u64;
            let mut acc = 1.0_f64;
            for k in 2..=n {
                acc *= k as f64;
                // Interrompi appena si supera f64::MAX: il risultato è già infinito/inutile.
                if acc.is_infinite() { break; }
            }
            Ok(acc)
        }

        // Operazioni binarie: + − × ÷ %.
        Expr::Bin { op, lhs, rhs } => {
            let (l, r) = (evaluate(lhs, mode)?, evaluate(rhs, mode)?);
            Ok(match op {
                BinOp::Add => l + r,
                BinOp::Sub => l - r,
                BinOp::Mul => l * r,
                BinOp::Div => {
                    // f64 == 0.0 è bit-exact qui: l'input è un letterale "0" → nessun problema di FP.
                    if r == 0.0 { return Err(CalcError::DivByZero); }
                    l / r
                }
                BinOp::Mod => {
                    // Il modulo f64 (`%`) calcola il resto della divisione: `l - r * (l/r).floor()`.
                    // `r == 0.0` → lo stesso errore della divisione (divisore nullo).
                    if r == 0.0 { return Err(CalcError::DivByZero); }
                    l % r
                }
            })
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────
// Ogni test è una specifica leggibile del comportamento atteso.
// La suite è stata scritta PRIMA dell'implementazione (TDD / RED → GREEN).
// Leggere i test è il modo più rapido per capire cosa fa il codice e imparare la sintassi Rust.
#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: parse + evaluate in una sola chiamata con il mode specificato.
    /// `AngleMode::Rad` per i test non-trig (aritmetici, potenze, radici, log).
    /// `AngleMode::Deg` per i test delle trig con argomenti in gradi.
    fn ev(s: &str, mode: AngleMode) -> Result<f64, CalcError> {
        evaluate(&parse(s)?, mode)
    }

    // ── Test esistenti (aggiornati: ev ora richiede mode; comportamento invariato) ─

    #[test]
    fn precedence() {
        // `*` lega più forte di `+`: 2 + (3*4) = 14, non (2+3)*4 = 20.
        assert_eq!(ev("2+3*4", AngleMode::Rad).unwrap(), 14.0);
    }

    #[test]
    fn parens_override() {
        // Le parentesi sovvertono la precedenza.
        assert_eq!(ev("(2+3)*4", AngleMode::Rad).unwrap(), 20.0);
    }

    #[test]
    fn unary_minus() {
        // Meno unario in posizione di testa e dopo un operatore.
        assert_eq!(ev("-3+2", AngleMode::Rad).unwrap(), -1.0);
        assert_eq!(ev("2*-3", AngleMode::Rad).unwrap(), -6.0);
    }

    #[test]
    fn decimals() {
        // Numeri decimali.
        assert_eq!(ev("0.5+0.25", AngleMode::Rad).unwrap(), 0.75);
    }

    #[test]
    fn unicode_ops() {
        // Il tokenizer accetta i simboli display Unicode: × (U+00D7), ÷ (U+00F7), − (U+2212).
        assert_eq!(ev("6 ÷ 3 × 2", AngleMode::Rad).unwrap(), 4.0);
        assert_eq!(ev("5 − 2", AngleMode::Rad).unwrap(), 3.0);
    }

    #[test]
    fn div_by_zero() {
        // La divisione per zero deve restituire `CalcError::DivByZero`.
        assert_eq!(ev("1/0", AngleMode::Rad), Err(CalcError::DivByZero));
    }

    #[test]
    fn empty_and_syntax() {
        // Stringa vuota → Empty; input incompleto o parentesi aperta → Syntax.
        assert_eq!(parse(""), Err(CalcError::Empty));
        assert_eq!(parse("2+"), Err(CalcError::Syntax));
        assert_eq!(parse("(2+3"), Err(CalcError::Syntax));
    }

    #[test]
    fn ast_shape() {
        // Verifica la struttura dell'AST prodotto da parse (il render 2D dipende da essa).
        assert_eq!(
            parse("6/3").unwrap(),
            Expr::Bin {
                op: BinOp::Div,
                lhs: Box::new(Expr::Num(6.0)),
                rhs: Box::new(Expr::Num(3.0)),
            }
        );
    }

    // ── Test C-1 (esponenziale) ───────────────────────────────────────────────

    #[test]
    fn exponent_literals_parse() {
        // L'engine deve saper leggere la notazione esponenziale che format_number produce.
        // Questo è necessario perché handle_key, dopo `=`, mette nel buffer il risultato
        // formattato (es. "1e12"); se poi l'utente preme un operatore, il buffer diventa
        // "1e12+3" e viene mandato al parser. Senza supporto esponenziale → Syntax.
        let v = evaluate(&parse("4.5e-9").unwrap(), AngleMode::Rad).unwrap();
        assert!((v - 4.5e-9).abs() < 1e-18, "4.5e-9 non approssimato correttamente: {v}");
        assert_eq!(evaluate(&parse("1.23e15").unwrap(), AngleMode::Rad).unwrap(), 1.23e15_f64);
        assert_eq!(evaluate(&parse("1e12").unwrap(), AngleMode::Rad).unwrap(), 1e12_f64);
    }

    #[test]
    fn continue_from_exponential_value() {
        // Bug C-1 (livello engine): "1e12+3" deve valutare a 1_000_000_000_003.0.
        assert_eq!(
            evaluate(&parse("1e12+3").unwrap(), AngleMode::Rad).unwrap(),
            1_000_000_000_003.0_f64
        );
    }

    #[test]
    fn malformed_exponent_is_syntax_not_panic() {
        // Guardia: esponente malformato NON deve far panicare il parser.
        // Con il tokenizer aggiornato: "2e" → [Num(2), Const(E)] → token in eccesso → Syntax.
        // "3e+" → [Num(3), Const(E), Plus] → token in eccesso dopo Num(3) → Syntax.
        assert_eq!(parse("2e"), Err(CalcError::Syntax));
        assert_eq!(parse("3e+"), Err(CalcError::Syntax));
    }

    // ── Nuovi test scientifici ────────────────────────────────────────────────

    /// Test trigonometria in gradi (modalità DEG, default della calcolatrice).
    #[test]
    fn trig_deg() {
        // sin(30°) = 0.5 (valore esatto in matematica, approssimato in f64).
        assert!((ev("sin(30)", AngleMode::Deg).unwrap() - 0.5).abs() < 1e-9,
            "sin(30) in DEG deve essere ≈ 0.5");
        // cos(0°) = 1.0 esatto.
        assert!((ev("cos(0)", AngleMode::Deg).unwrap() - 1.0).abs() < 1e-12,
            "cos(0) in DEG deve essere 1.0");
        // asin(0.5) in DEG = 30.0°
        assert!((ev("asin(0.5)", AngleMode::Deg).unwrap() - 30.0).abs() < 1e-9,
            "asin(0.5) in DEG deve essere ≈ 30.0");
    }

    /// Test trigonometria in radianti (modalità RAD).
    #[test]
    fn trig_rad() {
        // sin(0) = 0 esatto, indipendente dal modo angolare.
        assert_eq!(ev("sin(0)", AngleMode::Rad).unwrap(), 0.0);
    }

    /// Test funzioni non-trig: √, log, ln.
    #[test]
    fn sqrt_log_ln() {
        // √(9) = 3.0 esatto.
        assert_eq!(ev("√(9)", AngleMode::Rad).unwrap(), 3.0);
        // log(100) = log10(100) = 2.0 esatto.
        assert!((ev("log(100)", AngleMode::Rad).unwrap() - 2.0).abs() < 1e-12,
            "log(100) deve essere 2.0");
        // ln(e) = 1.0 (la costante e è riconosciuta dal tokenizer).
        assert!((ev("ln(e)", AngleMode::Rad).unwrap() - 1.0).abs() < 1e-12,
            "ln(e) deve essere 1.0");
    }

    /// Test potenza di base: 2^10 = 1024.
    #[test]
    fn power_basic() {
        assert_eq!(ev("2^10", AngleMode::Rad).unwrap(), 1024.0);
    }

    /// Test esponenti negativi: dimostrano che `power` chiama `unary` come rhs.
    #[test]
    fn power_negative_exponent() {
        // 5^-1 = 0.2: l'esponente è Neg(1), parsato come unary nel rhs di power.
        assert!((ev("5^-1", AngleMode::Rad).unwrap() - 0.2).abs() < 1e-12,
            "5^-1 deve essere 0.2");
        // 2^-3 = 0.125.
        assert!((ev("2^-3", AngleMode::Rad).unwrap() - 0.125).abs() < 1e-12,
            "2^-3 deve essere 0.125");
    }

    /// Test che il meno unario sia FUORI dalla potenza: -3^2 = -(3^2) = -9.
    ///
    /// Questo pinna la grammatica: `unary := '−' unary | power` con `power` che
    /// parsa prima l'atomo e poi il `^`. Se fosse `(-3)^2` il risultato sarebbe +9.
    #[test]
    fn power_unary_minus_outside() {
        assert_eq!(ev("-3^2", AngleMode::Rad).unwrap(), -9.0,
            "-3^2 deve essere -9 (unario FUORI dalla potenza: -(3^2))");
    }

    /// Test associatività destra di `^`: 2^3^2 = 2^(3^2) = 2^9 = 512.
    ///
    /// Se fosse sinistra-assoc: (2^3)^2 = 8^2 = 64. Deve essere 512.
    #[test]
    fn power_right_assoc() {
        assert_eq!(ev("2^3^2", AngleMode::Rad).unwrap(), 512.0,
            "2^3^2 deve essere 512 (destra-assoc: 2^(3^2) = 2^9)");
    }

    /// Test costanti: π e e sono riconosciute dal tokenizer e valutate correttamente.
    #[test]
    fn constants_pi_e() {
        // π riconosciuto sia come simbolo Unicode (U+03C0) sia tramite parse.
        assert_eq!(parse("π").unwrap(), Expr::Const(ConstId::Pi));
        assert!((ev("π", AngleMode::Rad).unwrap() - std::f64::consts::PI).abs() < 1e-12,
            "π deve valere std::f64::consts::PI");
        assert!((ev("e", AngleMode::Rad).unwrap() - std::f64::consts::E).abs() < 1e-12,
            "e deve valere std::f64::consts::E");
    }

    /// Test disambiguazione `e`: "1e5" è un numero (100000), "e" da solo è la costante E.
    ///
    /// Questo è il punto critico del tokenizer: il branch numerico DEVE consumare "1e5"
    /// interamente prima che la 'e' raggiunga il branch alfabetico.
    #[test]
    fn e_disambiguation() {
        // "1e5" tokenizzato come Num(100000.0) — NON come Const(E) separato.
        assert_eq!(parse("1e5").unwrap(), Expr::Num(100000.0),
            "1e5 deve essere tokenizzato come Num(100000), non come Const(E)");
        // "e" standalone → Const(E) → valuta a std::f64::consts::E.
        assert!((ev("e", AngleMode::Rad).unwrap() - std::f64::consts::E).abs() < 1e-12);
    }

    /// Test che un identificatore sconosciuto produca Err(Syntax).
    #[test]
    fn unknown_identifier_is_syntax() {
        // "foo" non è una funzione nota → il tokenizer restituisce Err(Syntax) direttamente.
        assert_eq!(parse("foo(2)"), Err(CalcError::Syntax));
    }

    /// Test che √(-1) restituisca Ok(NaN) (non Err).
    ///
    /// La radice quadrata di un numero negativo non è definita nei reali.
    /// f64::sqrt(-1.0) restituisce NaN (IEEE 754), non un'eccezione.
    /// L'engine propaga Ok(NaN); il formatter trasforma NaN in "Error" per l'utente.
    #[test]
    fn sqrt_negative_is_nan() {
        assert_eq!(
            ev("√(-1)", AngleMode::Rad).map(f64::is_nan),
            Ok(true),
            "√(-1) deve restituire Ok(NaN), non Err"
        );
    }

    // ── Refinements v2 — ∛ (cubrt), n! (factorial), % (modulo) ──────────────

    /// Test radice cubica: ∛(27)=3, ∛(8)=2.
    /// Il simbolo ∛ (U+221B) deve essere riconosciuto dal tokenizer come FuncId::Cbrt.
    #[test]
    fn cbrt_basic() {
        assert!(
            (ev("∛(27)", AngleMode::Rad).unwrap() - 3.0).abs() < 1e-9,
            "∛(27) deve essere ≈ 3.0"
        );
        assert!(
            (ev("∛(8)", AngleMode::Rad).unwrap() - 2.0).abs() < 1e-9,
            "∛(8) deve essere ≈ 2.0"
        );
    }

    /// Test fattoriale: 5!=120, 0!=1, 1!=1.
    /// `!` è un operatore postfisso — viene dopo il numero.
    #[test]
    fn factorial_integers() {
        assert_eq!(ev("5!", AngleMode::Rad).unwrap(), 120.0, "5! deve essere 120");
        assert_eq!(ev("0!", AngleMode::Rad).unwrap(), 1.0,   "0! deve essere 1 (convenzione matematica)");
        assert_eq!(ev("1!", AngleMode::Rad).unwrap(), 1.0,   "1! deve essere 1");
    }

    /// Test che il fattoriale leghi più stretto di `^` (postfix > potenza).
    ///
    /// Grammatica: `power := postfix ('^' unary)?`, `postfix := atom ('!')*`
    ///
    /// `3!^2`: la `3!` è il postfix (atom=3, poi `!` → Factorial(3)=6);
    ///         poi `^2` → Pow{Factorial(3), 2} → 6^2 = 36.
    /// `2^3!`: il rhs di `^` è `unary` → `power` → `postfix` → atom=3 poi `!` → Factorial(3)=6;
    ///         quindi Pow{2, Factorial(3)} → 2^6 = 64.
    #[test]
    fn factorial_tighter_than_pow() {
        assert_eq!(
            ev("3!^2", AngleMode::Rad).unwrap(), 36.0,
            "3!^2 deve essere 36 (fattoriale più stretto di ^: (3!)^2 = 6^2)"
        );
        assert_eq!(
            ev("2^3!", AngleMode::Rad).unwrap(), 64.0,
            "2^3! deve essere 64 (fattoriale tighter: 2^(3!) = 2^6)"
        );
    }

    /// Test che argomenti non-interi e negativi producano Ok(NaN).
    ///
    /// Come per √(-1) e ln(-1), il dominio invalido è reso come NaN (non Err),
    /// così il formatter può mostrare "Error" senza allargare CalcError.
    #[test]
    fn factorial_invalid_is_nan() {
        assert!(
            ev("(-1)!", AngleMode::Rad).unwrap().is_nan(),
            "(-1)! deve essere NaN (intero negativo)"
        );
        assert!(
            ev("2.5!", AngleMode::Rad).unwrap().is_nan(),
            "2.5! deve essere NaN (non intero)"
        );
    }

    /// Test modulo: 7%3=1, left-assoc al livello moltiplicativo.
    ///
    /// `2×3%4`: term è left-assoc → (2×3)%4 = 6%4 = 2.
    /// `2%3×4`: analogo left-assoc → (2%3)×4 = 2×4 = 8.
    /// Quest'ultimo è il test discriminante: se `%` fosse a livello additivo,
    /// `2%3×4` darebbe 2 (perché `×` avrebbe precedenza su `%` → `2%(3×4)=2%12=2`).
    /// Con `%` a livello moltiplicativo (stesso di `×`), dà 8 (left-assoc: `(2%3)×4`).
    #[test]
    fn modulo_basic() {
        assert_eq!(ev("7%3", AngleMode::Rad).unwrap(), 1.0, "7%3 deve essere 1");
        assert_eq!(ev("2×3%4", AngleMode::Rad).unwrap(), 2.0, "2×3%4 deve essere 2 (left-assoc)");
        assert_eq!(ev("2%3×4", AngleMode::Rad).unwrap(), 8.0, "2%3×4 deve essere 8 (test discriminante)");
    }

    /// Test che 7%0 restituisca Err(DivByZero).
    #[test]
    fn modulo_by_zero() {
        assert_eq!(
            ev("7%0", AngleMode::Rad),
            Err(CalcError::DivByZero),
            "7%0 deve dare DivByZero"
        );
    }

    /// Test che il fattoriale di un intero molto grande produca Ok(Infinity).
    ///
    /// Il loop di `Expr::Factorial` ha un early-exit `if acc.is_infinite() { break; }`
    /// che previene iterazioni inutili su argomenti enormi (>170 porta f64 a INFINITY).
    /// Questo test *caratterizza* quel comportamento difensivo — è stato aggiunto DOPO
    /// il codice (derogazione TDD documentata): il guard è necessario per prevenire hang
    /// su input come `1e20!` dove `v as u64` satura a `u64::MAX`.
    /// Il formatter converte INFINITY in "Error", come NaN.
    #[test]
    fn factorial_large_overflows_to_infinity() {
        let v = ev("200!", AngleMode::Rad).unwrap();
        assert!(
            v.is_infinite(),
            "200! deve essere +Infinity (f64 overflow a k=171, early-exit del loop), got {v}"
        );
    }

    /// Test ∛x come forma `x^(1/y)` (y-th-root form via Pow + parens).
    ///
    /// `8^(1/3)`: il parser produce Pow{8, Bin{Div,1,3}} → 8^(1/3) ≈ 2.
    /// Questo pinna che il parser esiste già (Pow + Div in parentesi) — nessuna nuova
    /// logica necessaria, è una regressione/conferma.
    #[test]
    fn eighth_root_via_pow() {
        let v = ev("8^(1/3)", AngleMode::Rad).unwrap();
        assert!(
            (v - 2.0).abs() < 1e-9,
            "8^(1/3) deve essere ≈ 2.0, got {v}"
        );
    }
}
