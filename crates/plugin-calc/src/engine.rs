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
    /// NOT bitwise unario (simbolo ¬, U+00AC) — modalità programmatore.
    /// Prefisso come √/∛: il parser lo gestisce già come `Func '(' expr ')'`
    /// senza alcuna modifica alla grammatica (basta la variante nell'enum).
    Not,
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
    pub fn radix(self) -> u32 {
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
/// compito. **Non si applica MAI in modalità Dec**: lì il valore mostrato è il
/// numero reale, non mascherato — la larghezza bit è uno stato "inerte" finché
/// `base_mode != Dec`.
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
    // ── Operatori bitwise (modalità programmatore) ─────────────────────────────
    // Precedenza C-like, inseriti SOPRA i livelli aritmetici esistenti:
    //   × ÷ %  >  + −  >  shift/rotate  >  AND  >  XOR  >  OR
    /// AND bitwise `l ∧ r` (simbolo ∧, U+2227).
    And,
    /// OR bitwise `l ∨ r` (simbolo ∨, U+2228).
    Or,
    /// XOR bitwise `l ⊻ r` (simbolo ⊻, U+22BB).
    Xor,
    /// Shift a sinistra `l ≪ r` (simbolo ≪, U+226A).
    Shl,
    /// Shift a destra `l ≫ r` (simbolo ≫, U+226B).
    Shr,
    /// Rotazione a sinistra (ROL) `l ↺ r` (simbolo ↺, U+21BA).
    Rol,
    /// Rotazione a destra (ROR) `l ↻ r` (simbolo ↻, U+21BB).
    Ror,
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
    /// Funzione matematica (sin, cos, …, ln, √, ∛, ¬).
    Func(FuncId),
    // ── Operatori bitwise (simboli Unicode) — modalità programmatore ────────────
    /// `∧` (U+2227) — AND bitwise.
    And,
    /// `∨` (U+2228) — OR bitwise.
    Or,
    /// `⊻` (U+22BB) — XOR bitwise.
    Xor,
    /// `≪` (U+226A) — shift a sinistra.
    Shl,
    /// `≫` (U+226B) — shift a destra.
    Shr,
    /// `↺` (U+21BA) — rotazione a sinistra (ROL).
    Rol,
    /// `↻` (U+21BB) — rotazione a destra (ROR).
    Ror,
}

/// Tokenizer base-aware: converte la stringa in una sequenza di Token,
/// con il comportamento per le CIFRE che dipende da `base`.
///
/// In Dec: comportamento **identico** al tokenizer originale in ogni sua parte.
/// In Hex/Oct/Bin: consuma cifre valide per quella base PIÙ eventuali separatori
/// `_` (per poter ri-leggere un risultato formattato con raggruppamento),
/// NIENTE punto decimale, NIENTE esponente (interi soltanto).
///
/// Accetta sia ASCII (`* / -`) sia i simboli display Unicode (`× ÷ −`).
/// Nuovi simboli bitwise (§2): ∧ ∨ ⊻ ¬ ≪ ≫ ↺ ↻ — tutti glyph Unicode a carattere singolo.
///
/// Ordine dei branch importante:
///   1. Whitespace, operatori semplici, parentesi, `^`, π, √ — O(1) per carattere.
///   2. Branch numerico (cifre con guardia base-aware) — consuma letterale numerico.
///   3. Branch alfabetico (`c.is_ascii_alphabetic()`) — funzioni/costanti note.
///   4. `_` — qualsiasi altro carattere → Err(Syntax).
fn tokenize_with_base(s: &str, base: NumBase) -> Result<Vec<Token>, CalcError> {
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

            // ── Operatori bitwise (modalità programmatore) — simboli Unicode ───
            // Usiamo glyph dedicati a carattere singolo per evitare collisioni
            // con le cifre esadecimali A-F: un token testuale "AND" comincerebbe
            // per 'A' che in modalità Hex è una cifra, rompendo il tokenizer.
            '\u{2227}' => { out.push(Token::And);  i += 1; }  // ∧ AND
            '\u{2228}' => { out.push(Token::Or);   i += 1; }  // ∨ OR
            '\u{22BB}' => { out.push(Token::Xor);  i += 1; }  // ⊻ XOR
            '\u{226A}' => { out.push(Token::Shl);  i += 1; }  // ≪ shift left
            '\u{226B}' => { out.push(Token::Shr);  i += 1; }  // ≫ shift right
            '\u{21BA}' => { out.push(Token::Rol);  i += 1; }  // ↺ rotate left
            '\u{21BB}' => { out.push(Token::Ror);  i += 1; }  // ↻ rotate right

            // '¬' U+00AC — NOT bitwise unario/prefisso (stesso schema di √/∛).
            // Gestito dal parser come `Func '(' expr ')'` — nessuna nuova grammatica.
            '\u{00AC}' => { out.push(Token::Func(FuncId::Not)); i += 1; }

            // ── Numero: branch base-aware ─────────────────────────────────────
            //
            // NOTA CRITICA: questo branch DEVE venire prima del branch alfabetico.
            // In Dec: comportamento IDENTICO all'originale (mantissa + punto + esponente).
            // In Hex/Oct/Bin: solo cifre valide nella base + separatore `_`;
            // NIENTE punto decimale, NIENTE esponente (interi soltanto).
            //
            // Il guard richiede che il carattere DI INNESCO sia una cifra vera — mai `_`.
            // Questo garantisce che un run non possa MAI iniziare con `_`
            // (un buffer tipo "_FF" cade nel branch "carattere sconosciuto" → Syntax).
            c if (base == NumBase::Dec && (c.is_ascii_digit() || c == '.'))
                 || (base != NumBase::Dec && c.is_digit(base.radix())) =>
            {
                if base == NumBase::Dec {
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
                } else {
                    // Hex/Oct/Bin: consuma cifre valide nella base + separatori '_'.
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

    // ── Livelli bitwise (modalità programmatore) — SOPRA `expr`, precedenza C-like ──
    // Da stretta a larga: shift/rotate > AND > XOR > OR.
    // Riusano il nodo `Expr::Bin` esistente: zero nuove varianti di `Expr`.

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

    /// Livello XOR bitwise — più stretto di OR, più largo di AND.
    fn xor_expr(&mut self) -> Result<Expr, CalcError> {
        let mut lhs = self.and_expr()?;
        while let Some(Token::Xor) = self.peek() {
            self.bump();
            let rhs = self.and_expr()?;
            lhs = Expr::Bin { op: BinOp::Xor, lhs: Box::new(lhs), rhs: Box::new(rhs) };
        }
        Ok(lhs)
    }

    /// Livello AND bitwise — più stretto di XOR, più largo di shift/rotate.
    fn and_expr(&mut self) -> Result<Expr, CalcError> {
        let mut lhs = self.shift_expr()?;
        while let Some(Token::And) = self.peek() {
            self.bump();
            let rhs = self.shift_expr()?;
            lhs = Expr::Bin { op: BinOp::And, lhs: Box::new(lhs), rhs: Box::new(rhs) };
        }
        Ok(lhs)
    }

    /// Shift E rotazione condividono lo stesso livello (4 token invece di 2):
    /// concettualmente la stessa famiglia "riposiziona i bit di N posizioni".
    /// Il livello più stretto chiamato qui è `expr()` (aritmetica esistente, invariata).
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
}

// ─── API pubblica ─────────────────────────────────────────────────────────────

/// Parse di una stringa in un AST in base decimale (wrapper di compatibilità).
/// Restituisce `CalcError::Empty` per stringa vuota e `CalcError::Syntax`
/// se ci sono token in eccesso dopo la fine dell'espressione.
///
/// Questo wrapper mantiene il comportamento storico di `parse`: il default
/// `NumBase::Dec` produce un risultato byte-identico a prima dell'introduzione
/// della modalità programmatore. Tutti i chiamanti esistenti restano invariati.
pub fn parse(s: &str) -> Result<Expr, CalcError> {
    parse_with_base(s, NumBase::Dec)
}

/// Parse di una stringa in un AST con la base numerica esplicita.
/// L'entry point è `or_expr()` (precedenza più bassa, OR bitwise): la catena
/// di discesa termina su `expr()` esistente, quindi tutte le espressioni
/// puramente aritmetiche si comportano esattamente come prima.
pub fn parse_with_base(s: &str, base: NumBase) -> Result<Expr, CalcError> {
    let toks = tokenize_with_base(s, base)?;
    if toks.is_empty() {
        return Err(CalcError::Empty);
    }
    let mut p = Parser { toks, pos: 0 };
    let e = p.or_expr()?;
    // Se ci sono token rimasti (es. "2 3" → l'AST è `2`, il `3` è in eccesso), è un errore.
    if p.pos != p.toks.len() {
        return Err(CalcError::Syntax);
    }
    Ok(e)
}

/// Converte un f64 in i64 SOLO se rappresenta esattamente un intero nel range i64.
/// None per: parte frazionaria non nulla, fuori range [i64::MIN, i64::MAX].
///
/// Limite noto: f64 ha 53 bit di mantissa — interi oltre ±2^53 potrebbero non essere
/// rappresentati esattamente anche se in teoria dentro il range i64. Per l'uso da
/// calcolatrice (conteggi, byte, maschere di bit, indirizzi) è un limite accettabile
/// e coerente con TUTTO il resto di questo engine (già basato su f64 ovunque).
pub(crate) fn to_i64_checked(v: f64) -> Option<i64> {
    if v.fract() != 0.0 { return None; }
    if v < -(2f64.powi(63)) || v >= 2f64.powi(63) { return None; }
    Some(v as i64)
}

/// Valuta l'AST ricorsivamente, dato il modo angolare per le funzioni trig.
///
/// Wrapper di compatibilità: chiama `evaluate_with_width` con `BitWidth::Qword`
/// (64 bit, comportamento invariato — l'unica larghezza che esisteva prima
/// della modalità programmatore).
pub fn evaluate(e: &Expr, mode: AngleMode) -> Result<f64, CalcError> {
    evaluate_with_width(e, mode, BitWidth::Qword)
}

/// Valuta l'AST ricorsivamente, con modo angolare E larghezza bit esplicita.
///
/// `mode` influenza solo le funzioni trigonometriche (vedi `evaluate` originale).
/// `width` influenza SOLO la validità di shift/rotate (§4) — NON maschera i
/// risultati (la mascheratura è compito del formatter, §6).
///
/// NOT bitwise non riceve `width`: la mascheratura a valle nel formatter
/// basta da sola — come per gli operandi di AND/OR/XOR, il risultato bitwise
/// di due operandi già dentro la larghezza resta naturalmente dentro la larghezza.
pub fn evaluate_with_width(e: &Expr, mode: AngleMode, width: BitWidth) -> Result<f64, CalcError> {
    match e {
        // Foglie: numeri e costanti.
        Expr::Num(n) => Ok(*n),

        Expr::Const(id) => Ok(match id {
            ConstId::Pi => std::f64::consts::PI,
            ConstId::E  => std::f64::consts::E,
        }),

        // Meno unario: nega ricorsivamente il sottoalbero.
        Expr::Neg(x) => Ok(-evaluate_with_width(x, mode, width)?),

        // Applicazione di funzione: valuta l'argomento, poi applica la funzione.
        Expr::Func { id, arg } => {
            let a = evaluate_with_width(arg, mode, width)?;
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
                FuncId::Log  => a.log10(),
                FuncId::Ln   => a.ln(),
                FuncId::Sqrt => a.sqrt(),
                FuncId::Cbrt => a.cbrt(),
                // NOT bitwise unario (¬): stesso dominio di AND/OR/XOR.
                // La mascheratura alla larghezza avviene a valle nel formatter
                // (come per tutti gli operatori bitwise — la larghezza non è
                // applicata durante la valutazione, §4/§6).
                FuncId::Not => match to_i64_checked(a) {
                    Some(n) => !n as f64,
                    None => f64::NAN,
                },
            };
            Ok(result)
        }

        // Potenza: base^exp tramite f64::powf.
        Expr::Pow { base, exp } => {
            Ok(evaluate_with_width(base, mode, width)?.powf(evaluate_with_width(exp, mode, width)?))
        }

        // Fattoriale postfisso: n! dove n deve essere un intero non-negativo.
        Expr::Factorial(inner) => {
            let v = evaluate_with_width(inner, mode, width)?;
            if v < 0.0 || v.fract() != 0.0 {
                return Ok(f64::NAN);
            }
            let n = v as u64;
            let mut acc = 1.0_f64;
            for k in 2..=n {
                acc *= k as f64;
                if acc.is_infinite() { break; }
            }
            Ok(acc)
        }

        // Operazioni binarie: + − × ÷ % e operatori bitwise.
        Expr::Bin { op, lhs, rhs } => {
            let (l, r) = (evaluate_with_width(lhs, mode, width)?, evaluate_with_width(rhs, mode, width)?);
            Ok(match op {
                BinOp::Add => l + r,
                BinOp::Sub => l - r,
                BinOp::Mul => l * r,
                BinOp::Div => {
                    if r == 0.0 { return Err(CalcError::DivByZero); }
                    l / r
                }
                BinOp::Mod => {
                    if r == 0.0 { return Err(CalcError::DivByZero); }
                    l % r
                }
                // ── Operatori bitwise ─────────────────────────────────────────
                // Dominio invalido (non-intero, fuori range i64) → f64::NAN,
                // mai Err — stesso principio di √(-1). Il formatter trasforma
                // NaN in "Error".
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
                    // checked_shl/checked_shr: MAI l'operatore `<<`/`>>` grezzo —
                    // panica in debug/test per shift >= 64, raggiungibile da tastiera.
                    // L'ammontare valido dipende da `width`: `1≪9` con BitWidth::Byte
                    // deve dare NaN (Windows Calc fa così), non essere silenziosamente
                    // troncato — la larghezza bit limita anche COSA è uno shift legale.
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
                    // Rotazione: un ammontare >= width è LEGALE (si riduce modulo
                    // width — ruotare un byte di 8 posizioni è un giro completo).
                    // Solo un ammontare negativo è invalido.
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
                            // Reinterpretazione bit u64→i64→f64: stesso limite di
                            // precisione ±2^53 già documentato per to_i64_checked.
                            rotated as i64 as f64
                        }
                        _ => f64::NAN,
                    }
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

    // ══ Modalità programmatore (Parte A) — RED prima, GREEN dopo ══════════════

    /// Helper: parse in una base esplicita + evaluate a larghezza Qword.
    fn ev_base(s: &str, base: NumBase) -> Result<f64, CalcError> {
        evaluate_with_width(&parse_with_base(s, base)?, AngleMode::Rad, BitWidth::Qword)
    }

    /// Helper: parse decimale + evaluate a larghezza esplicita.
    fn ev_w(s: &str, width: BitWidth) -> Result<f64, CalcError> {
        evaluate_with_width(&parse(s)?, AngleMode::Rad, width)
    }

    /// Operatori bitwise di base: AND, OR, XOR.
    #[test]
    fn bitwise_and_or_xor_values() {
        assert_eq!(ev("5∧3", AngleMode::Rad).unwrap(), 1.0, "5∧3 deve essere 1");
        assert_eq!(ev("5∨2", AngleMode::Rad).unwrap(), 7.0, "5∨2 deve essere 7");
        assert_eq!(ev("5⊻1", AngleMode::Rad).unwrap(), 4.0, "5⊻1 deve essere 4");
    }

    /// Shift di base: sinistra e destra.
    #[test]
    fn shift_values() {
        assert_eq!(ev("1≪4", AngleMode::Rad).unwrap(), 16.0, "1≪4 deve essere 16");
        assert_eq!(ev("256≫4", AngleMode::Rad).unwrap(), 16.0, "256≫4 deve essere 16");
    }

    /// NOT bitwise unario: ¬(0) = tutti i bit a 1 = -1 a piena ampiezza i64.
    #[test]
    fn not_bitwise_value() {
        assert_eq!(ev("¬(0)", AngleMode::Rad).unwrap(), -1.0, "¬(0) deve essere -1");
    }

    /// Precedenza discriminante 1: AND più stretto di OR.
    /// `2∨1∧0` = `2∨(1∧0)` = `2∨0` = 2 (NON `(2∨1)∧0` = `3∧0` = 0).
    #[test]
    fn precedence_and_tighter_than_or() {
        assert_eq!(ev("2∨1∧0", AngleMode::Rad).unwrap(), 2.0,
            "2∨1∧0 deve essere 2 (AND più stretto di OR)");
    }

    /// Precedenza discriminante 2: shift più largo di `+`.
    /// `1≪2+3` = `1≪(2+3)` = `1≪5` = 32 (NON `(1≪2)+3` = 4+3 = 7).
    #[test]
    fn precedence_shift_wider_than_add() {
        assert_eq!(ev("1≪2+3", AngleMode::Rad).unwrap(), 32.0,
            "1≪2+3 deve essere 32 (shift più largo di +)");
    }

    /// Precedenza discriminante 3: AND più largo di shift.
    /// `8≫1∧3` = `(8≫1)∧3` = `4∧3` = 0 (NON `8≫(1∧3)` = `8≫1` = 4).
    #[test]
    fn precedence_and_wider_than_shift() {
        assert_eq!(ev("8≫1∧3", AngleMode::Rad).unwrap(), 0.0,
            "8≫1∧3 deve essere 0 (AND più largo di shift)");
    }

    /// Bonus verificato: gli operatori bitwise funzionano anche in modalità Dec.
    /// Non è vietato usarli fuori dalla modalità programmatore.
    #[test]
    fn bitwise_works_in_dec() {
        assert_eq!(ev("255∧15", AngleMode::Rad).unwrap(), 15.0,
            "255∧15 deve essere 15 anche in Dec");
    }

    /// `to_i64_checked`: conversione f64 → i64 esatta e con i confini giusti.
    #[test]
    fn to_i64_checked_boundaries() {
        // 2^53 - 1: il massimo intero esattamente rappresentabile in f64.
        let two53_minus_1 = 2f64.powi(53) - 1.0;
        assert_eq!(to_i64_checked(two53_minus_1), Some(2i64.pow(53) - 1),
            "2^53-1 deve convertire esattamente");
        // i64::MIN è rappresentabile esattamente in f64 (potenza di due negativa).
        assert_eq!(to_i64_checked(i64::MIN as f64), Some(i64::MIN),
            "i64::MIN deve convertire esattamente");
        // 2^63 va fuori dal range i64 (>= i64::MAX + 1) → None.
        assert_eq!(to_i64_checked(2f64.powi(63)), None,
            "2^63 deve essere fuori range (None)");
        // Non-intero → None.
        assert_eq!(to_i64_checked(0.5), None, "0.5 non è un intero");
    }

    /// Shift: l'ammontare valido dipende dalla larghezza, non è un limite assoluto.
    #[test]
    fn shift_amount_depends_on_width() {
        // 1≪9 è VALIDO a Qword (9 < 64): binario 10_0000_0000 = 512.
        assert_eq!(ev_w("1≪9", BitWidth::Qword).unwrap(), 512.0,
            "1≪9 deve essere valido a Qword");
        // 1≪9 è INVALIDO a Byte (9 >= 8) → NaN, mai panic.
        assert!(ev_w("1≪9", BitWidth::Byte).unwrap().is_nan(),
            "1≪9 deve essere NaN a Byte (larghezza limita la legalità)");
        // Ammontare negativo → NaN.
        assert!(ev("1≪-1", AngleMode::Rad).unwrap().is_nan(),
            "1≪-1 deve essere NaN");
    }

    /// Rotazione: ammontare >= width è legale (modulo width).
    #[test]
    fn rotate_byte_values() {
        // 0b0001 ruotato a sinistra di 1 con Byte → 0b0010 = 2.
        assert_eq!(ev_w("1↺1", BitWidth::Byte).unwrap(), 2.0,
            "1↺1 a Byte deve essere 2");
        // Ruotato di 8 posizioni con Byte → giro completo → torna 1.
        assert_eq!(ev_w("1↺8", BitWidth::Byte).unwrap(), 1.0,
            "1↺8 a Byte deve essere 1 (giro completo)");
        // Ammontare negativo → NaN.
        assert!(ev_w("1↺-1", BitWidth::Byte).unwrap().is_nan(),
            "1↺-1 deve essere NaN");
    }

    /// Parsing di letterali nelle basi non-decimali.
    #[test]
    fn non_decimal_literal_parsing() {
        assert_eq!(ev_base("FF", NumBase::Hex).unwrap(), 255.0, "FF hex = 255");
        assert_eq!(ev_base("177", NumBase::Oct).unwrap(), 127.0, "177 oct = 127");
        assert_eq!(ev_base("1010", NumBase::Bin).unwrap(), 10.0, "1010 bin = 10");
        // "FFFFFFFFFFFFFFFF" (tutti i bit a 1) è un u64 valido → reinterpretato come -1.
        assert_eq!(ev_base("FFFFFFFFFFFFFFFF", NumBase::Hex).unwrap(), -1.0,
            "FFFFFFFFFFFFFFFF hex = -1 (complemento a due)");
    }

    /// Il separatore `_` è ignorato in Hex/Oct/Bin (per ri-leggere un risultato
    /// formattato con raggruppamento), ma resta Syntax in Dec.
    #[test]
    fn underscore_separator_only_non_decimal() {
        assert_eq!(ev_base("FF_FF", NumBase::Hex).unwrap(), 65535.0,
            "FF_FF hex deve essere 65535 (separatore ignorato)");
        assert_eq!(parse("1_000"), Err(CalcError::Syntax),
            "in Dec il separatore '_' resta un errore di sintassi");
        // Un run non può MAI iniziare con '_': "_FF" → Syntax.
        assert_eq!(parse_with_base("_FF", NumBase::Hex), Err(CalcError::Syntax),
            "un token che inizia con '_' deve essere Syntax");
    }

    /// Shift a destra con segno: 256≫4 = 16 a Qword.
    #[test]
    fn shift_right_value() {
        assert_eq!(ev("256≫4", AngleMode::Rad).unwrap(), 16.0);
    }
}
