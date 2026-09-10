//! plugin-calc — la calcolatrice. Stato = buffer di input; i tasti lo costruiscono;
//! `=` valuta (engine → format). Il display rende il buffer come HTML 2D (render) se
//! parserizza, altrimenti lineare.
//!
//! Struttura:
//!   - `CalcState`     — stato mutabile (buffer + flag "ultimo evento = risultato").
//!   - `handle_key`    — logica pura testabile: traduce un `data-evt` in una modifica di stato.
//!   - `render_window` — genera l'HTML completo: display 2D (o lineare) + griglia tasti.
//!   - `main`          — loop stdin riga-per-riga, modellato su `plugin-counter`.

mod engine;
mod format;
mod render;

use engine::{parse, parse_with_base, evaluate_with_width, AngleMode, BitWidth, NumBase};
use format::{format_integer_in_base, format_number};
use plugin_protocol::{HostToPlugin, PluginToHost};
use std::io::{BufRead, Write};

/// Stato del plugin: il buffer che l'utente sta costruendo tasto per tasto.
///
/// `last_was_result`: flag per distinguere se l'ultimo evento era `=` (risultato).
/// Serve a implementare la regola "cifra dopo risultato → ricomincia; operatore → continua".
///
/// `last_expr`: espressione catturata all'ultimo `=`, mostrata nell'eco a due righe
/// (display = risultato sopra, eco = espressione grezza sotto).
/// Esempio: dopo "7 × 8 =", `buf` = "56" e `last_expr` = "7×8".
///
/// `shift`: flag "Shift / 2nd function" sticky — vero quando l'utente ha premuto il tasto
/// Shift; il successivo tasto scientifico usa la sua variante (es. sin→asin).
/// Si azzera automaticamente dopo ogni tasto non-shift (Task 3).
///
/// `angle_mode`: modalità angolare per le funzioni trig (Deg = default, Rad).
/// Cambiata dal tasto DEG/RAD (`data-evt="mode"`).
///
/// In OOP sarebbe un oggetto con campi privati; in Rust un semplice struct con `Default`.
/// `#[derive(Default)]` genera automaticamente tutti i campi col loro valore di default
/// (`String::new()` per `String`, `false` per `bool`, `AngleMode::Deg` via `impl Default`).
#[derive(Default)]
struct CalcState {
    buf: String,
    last_was_result: bool,
    /// Espressione catturata all'ultimo "=", mostrata nell'eco a due righe
    /// (display = risultato sopra, eco = espressione grezza sotto).
    last_expr: String,
    /// Flag "Shift / 2nd": se true, il prossimo tasto scientifico usa la variante shiftata.
    /// Sticky: si azzera automaticamente dopo il primo tasto non-shift.
    shift: bool,
    /// Modalità angolare per le funzioni trigonometriche (default: Deg).
    angle_mode: AngleMode,
    /// Base numerica per la modalità programmatore (default: Dec).
    /// In Dec: comportamento identico alla calcolatrice scientifica.
    /// In Hex/Oct/Bin: solo interi, niente punto decimale/esponente.
    base_mode: NumBase,
    /// Larghezza del registro per NOT/shift/rotate e per la formattazione in base
    /// non-decimale (default: Qword). INERTE se base_mode == Dec.
    bit_width: BitWidth,
    /// Sezione programmatore visibile/nascosta (default: nascosta — la calcolatrice
    /// si presenta come oggi finché l'utente non la apre esplicitamente).
    prog_visible: bool,
}

/// Helper per appendere una stringa al buffer con la semantica "chiaro-dopo-risultato".
///
/// `is_fresh = true`  → si comporta come una cifra/funzione prefissa: se il buffer contiene
///   un risultato precedente (`last_was_result`) o "Error", lo cancella prima di appendere.
///   Usato da: fn_sin/cos/tan, fn_log, fn_ln, fn_sqrt (entrambe le varianti, normale e ∛),
///   const_pi, const_e.
///   Nota v2: `fn_sqrt` con shift appende "∛(" (funzione, apre parentesi) → is_fresh=true.
///
/// `is_fresh = false` → si comporta come un operatore/postfisso: continua dal risultato
///   corrente senza cancellarlo (eccetto se il buffer è "Error").
///   Usato da: op_pow (^ e ^(1/), fn_recip (^-1), fn_square (^2 / ^3),
///   fn_factorial (!), fn_mod (%).
///
/// Centralizzare questa logica in un helper evita la duplicazione nei rami dei
/// tasti scientifici (SOLID Single Responsibility, DRY).
fn append_str(state: &mut CalcState, text: &str, is_fresh: bool) {
    if state.last_was_result {
        if is_fresh || state.buf == "Error" {
            state.buf.clear();
        }
        state.last_was_result = false;
    }
    state.buf.push_str(text);
}

/// Vero se `c` è una cifra (0-9, A-F esadecimale, o punto decimale) valida per
/// l'alfabeto della base numerica corrente.
///
/// Fix 2026-09-10 (segnalato da Maurizio dal vivo, due casi concreti: "2" accettato
/// in modalità Bin dopo "1101"; "C" — un tasto esadecimale — accettato in modalità
/// Dec): prima di questo fix, i tasti cifra (d0-d9, hexA-F, '.') finivano SEMPRE
/// nel buffer indipendentemente dalla base attiva — solo l'engine, al momento di
/// "=", rifiutava il letterale non valido (mostrando "Error" a posteriori). I tasti
/// restano SEMPRE visibili (Maurizio: "tutti i tasti numerici vengono mostrati"),
/// ma solo quelli che appartengono davvero all'alfabeto della base corrente vengono
/// accettati nel buffer — esattamente come un tasto disabilitato su una calcolatrice
/// fisica, semplicemente non succede nulla alla pressione.
///
/// Operatori e parentesi (+, −, ×, ÷, (, )) NON passano da questo controllo — sono
/// sempre validi indipendentemente dalla base (gli operatori bitwise funzionano
/// anche in Dec, vedi il compito 2026-09-10-calc-modalita-programmatore.md §1); la
/// funzione chiamante applica questo controllo solo ai caratteri "cifra-simili"
/// (vedi `is_ascii_alphanumeric() || c == '.'` nel chiamante).
fn is_valid_digit_for_base(c: char, base: NumBase) -> bool {
    match base {
        // Dec: invariato rispetto al comportamento storico — cifre 0-9 e punto.
        NumBase::Dec => c.is_ascii_digit() || c == '.',
        // Hex: 0-9 e A-F (case-insensitive; i tasti emettono solo maiuscole,
        // `is_ascii_hexdigit` accetta comunque entrambi i casi senza problemi).
        NumBase::Hex => c.is_ascii_hexdigit(),
        // Oct: solo 0-7.
        NumBase::Oct => ('0'..='7').contains(&c),
        // Bin: solo 0/1.
        NumBase::Bin => c == '0' || c == '1',
    }
}

/// Se il buffer corrente valuta a un intero rappresentabile nella VECCHIA base/
/// larghezza, lo riformatta nella NUOVA base/larghezza e lo scrive nel buffer
/// (comportamento "conversione", imita Windows Calculator modalità Programmatore).
/// Altrimenti (buffer vuoto, espressione incompleta, non intero) non tocca il
/// buffer — lascia al chiamante la libertà di cambiare comunque la modalità.
fn try_convert_buf(state: &mut CalcState, new_base: NumBase, new_width: BitWidth) {
    let Ok(ast) = parse_with_base(&state.buf, state.base_mode) else { return };
    let Ok(v) = evaluate_with_width(&ast, state.angle_mode, state.bit_width) else { return };
    let converted = if new_base == NumBase::Dec {
        Some(format_number(v))
    } else {
        format_integer_in_base(v, new_base, new_width)
    };
    if let Some(text) = converted {
        state.buf = text;
        state.last_was_result = true;
    }
    // else: non un intero rappresentabile — buffer invariato, solo la modalità cambia
    // (fatto dal chiamante DOPO questa funzione, vedi i rami match sotto).
}

/// Traduce un `data-evt` nel carattere/azione corrispondente e aggiorna lo stato.
///
/// Mappa `data-evt` → azione:
///   d0..d9  → cifre ASCII,  dot → '.', op_add → '+',
///   op_sub  → '−' (U+2212), op_mul → '×' (U+00D7), op_div → '÷' (U+00F7),
///   paren_open → '(', paren_close → ')'
///   back    → rimuove l'ultimo carattere Unicode dal buffer
///   clear   → svuota il buffer
///   eq      → valuta l'espressione e sostituisce il buffer con il risultato
///
///   Tasti scientifici (Task 3 v1 + Refinements v2 Task vB):
///   shift          → alterna `state.shift` (flag sticky 2nd, non soggetto a sticky-off)
///   mode           → alterna `state.angle_mode` Deg ↔ Rad
///   fn_sin/cos/tan → append "sin("/… o "asin("/… se shift (is_fresh=true)
///   fn_log/ln      → append "log("/… o "10^("/… se shift (is_fresh=true)
///   fn_sqrt        → append "√(" (normale) o "∛(" (shift, v2) — entrambi is_fresh=true
///   fn_square      → append "^2" (normale) o "^3" (shift, v2) — entrambi is_fresh=false
///   op_pow         → append "^" (normale) o "^(1/" (shift, v2) — entrambi is_fresh=false
///   fn_recip       → append "^-1" (operatore, is_fresh=false)
///   fn_factorial   → append "!" (postfisso, is_fresh=false, v2)
///   fn_mod         → append "%" (operatore moltiplicativo, is_fresh=false, v2)
///   const_pi       → append "π" (is_fresh=true; nessuna 2ª funzione in v2)
///   const_e        → append "e" (is_fresh=true, v2; tasto separato da const_pi)
///
///   **Sticky Shift**: dopo ogni tasto NON-shift, se `state.shift` era attivo
///   viene azzerato automaticamente (comportamento "2nd" delle calcolatrici fisiche).
///
/// I simboli Unicode vengono spinti nel buffer perché il tokenizer dell'engine li accetta.
/// In questo modo il buffer è leggibile dall'utente E valutabile senza conversione.
fn handle_key(state: &mut CalcState, key: &str) {
    // Il tasto "shift" (2nd) è speciale: toglie/attiva il flag e torna subito,
    // senza applicare la regola sticky-off (non si può "shift-off" da sé stesso).
    if key == "shift" {
        state.shift = !state.shift;
        return;
    }

    // Cattura lo stato shift PRIMA di processare il tasto: serve per determinare quale
    // variante (normale o shiftata) usare. Dopo il match verrà azzerato (sticky).
    let was_shifted = state.shift;

    // Mappa cifra/operatore/parentesi → carattere del buffer.
    // Usiamo Option<char>: None per i tasti speciali (eq, back, clear, scientifici).
    let ch: Option<char> = match key {
        "d0" => Some('0'),
        "d1" => Some('1'),
        "d2" => Some('2'),
        "d3" => Some('3'),
        "d4" => Some('4'),
        "d5" => Some('5'),
        "d6" => Some('6'),
        "d7" => Some('7'),
        "d8" => Some('8'),
        "d9" => Some('9'),
        // Cifre esadecimali A-F: stesso ramo generico delle cifre 0-9 (regola
        // "smart clear after result" inclusa). In Dec il tokenizer le rifiuterà
        // (Syntax), ma i tasti sono visibili solo nella sezione programmatore.
        "hexA" => Some('A'),
        "hexB" => Some('B'),
        "hexC" => Some('C'),
        "hexD" => Some('D'),
        "hexE" => Some('E'),
        "hexF" => Some('F'),
        "dot"          => Some('.'),
        "op_add"       => Some('+'),
        "op_sub"       => Some('−'),   // U+2212 — display minus sign (accettato dall'engine)
        "op_mul"       => Some('×'),   // U+00D7 — multiplication sign
        "op_div"       => Some('÷'),   // U+00F7 — division sign
        "paren_open"   => Some('('),
        "paren_close"  => Some(')'),
        _ => None,
    };

    match key {
        // C (clear): azzera il buffer, il flag risultato, e l'eco.
        "clear" => {
            state.buf.clear();
            state.last_was_result = false;
            state.last_expr.clear();
        }

        // ⌫ (backspace): rimuove l'ultimo codepoint Unicode.
        // `String::pop()` è sicuro anche per caratteri multi-byte (es. '×' = 2 byte in UTF-8).
        "back" => {
            state.buf.pop();
            state.last_was_result = false;
        }

        // = (uguale): valuta l'espressione corrente con la base e larghezza correnti.
        "eq" => {
            // Cattura l'espressione digitata PRIMA di sovrascrivere il buffer col risultato.
            state.last_expr = state.buf.clone();
            let result = parse_with_base(&state.buf, state.base_mode)
                .and_then(|e| evaluate_with_width(&e, state.angle_mode, state.bit_width));
            state.buf = match (result, state.base_mode) {
                (Ok(v), NumBase::Dec) => format_number(v),
                (Ok(v), _) => format_integer_in_base(v, state.base_mode, state.bit_width)
                    .unwrap_or_else(|| "Error".to_string()),
                (Err(_), _) => "Error".to_string(),
            };
            state.last_was_result = true;
        }

        // DEG ↔ RAD: alterna la modalità angolare senza toccare il buffer.
        "mode" => {
            state.angle_mode = match state.angle_mode {
                AngleMode::Deg => AngleMode::Rad,
                AngleMode::Rad => AngleMode::Deg,
            };
        }

        // ── Base numerica — modalità programmatore ─────────────────────────────
        "base_dec" | "base_hex" | "base_oct" | "base_bin" => {
            let new_base = match key {
                "base_dec" => NumBase::Dec, "base_hex" => NumBase::Hex,
                "base_oct" => NumBase::Oct, "base_bin" => NumBase::Bin,
                _ => unreachable!(),
            };
            try_convert_buf(state, new_base, state.bit_width);
            state.base_mode = new_base;
        }
        // ── Larghezza bit — stesso pattern "converti se il buffer valuta" ──────
        "width_byte" | "width_word" | "width_dword" | "width_qword" => {
            let new_width = match key {
                "width_byte" => BitWidth::Byte, "width_word" => BitWidth::Word,
                "width_dword" => BitWidth::Dword, "width_qword" => BitWidth::Qword,
                _ => unreachable!(),
            };
            try_convert_buf(state, state.base_mode, new_width);
            state.bit_width = new_width;
        }
        // ── Toggle sezione programmatore ───────────────────────────────────────
        "toggle_prog" => { state.prog_visible = !state.prog_visible; }

        // ── Funzioni trigonometriche dirette e inverse ─────────────────────────
        // is_fresh=true: aprono un nuovo input (cancellano il risultato precedente).
        // La variante inversa (asin/acos/atan) è attivata dallo Shift.
        "fn_sin" => append_str(state, if was_shifted { "asin(" } else { "sin(" }, true),
        "fn_cos" => append_str(state, if was_shifted { "acos(" } else { "cos(" }, true),
        "fn_tan" => append_str(state, if was_shifted { "atan(" } else { "tan(" }, true),

        // ── Logaritmi ──────────────────────────────────────────────────────────
        // log (base 10) / 10^( ; ln (naturale) / e^(
        "fn_log" => append_str(state, if was_shifted { "10^(" } else { "log(" }, true),
        "fn_ln"  => append_str(state, if was_shifted { "e^("  } else { "ln("  }, true),

        // ── Radici (v2) ────────────────────────────────────────────────────────
        // fn_sqrt: entrambe le varianti aprono una funzione con parentesi → is_fresh=true.
        //   Normale: √( (radice quadrata).
        //   Shift:   ∛( (radice cubica, U+221B; l'engine riconosce ∛ come FuncId::Cbrt).
        // Nota: in v1, shift produceva "^2" (operatore, is_fresh=false).
        //       In v2, "^2"/"^3" si spostano sul tasto fn_square.
        "fn_sqrt" => {
            if was_shifted {
                append_str(state, "∛(", true);
            } else {
                append_str(state, "√(", true);
            }
        }

        // fn_square (v2, nuovo tasto): elevazione a potenza fissa — operatore, is_fresh=false.
        //   Normale: ^2 (al quadrato); Shift: ^3 (al cubo).
        //   is_fresh=false: "5 = 5" poi fn_square → "5^2", non "^2" su buffer vuoto.
        "fn_square" => {
            if was_shifted {
                append_str(state, "^3", false);
            } else {
                append_str(state, "^2", false);
            }
        }

        // ── Operatori potenza ──────────────────────────────────────────────────
        // op_pow (v2 aggiunge Shift): entrambe le varianti continuano dal risultato (is_fresh=false).
        //   Normale: ^ (potenza generica).
        //   Shift:   ^(1/ (y-esima radice: "x^(1/y)" calcola la y-esima radice di x).
        // fn_recip: 1/x come "^-1", operatore.
        "op_pow"   => append_str(state, if was_shifted { "^(1/" } else { "^" }, false),
        "fn_recip" => append_str(state, "^-1", false),

        // ── Fattoriale e modulo (v2, nuovi tasti) ─────────────────────────────
        // Entrambi sono operatori/postfissi che continuano dal risultato (is_fresh=false).
        //   fn_factorial: appende "!" — postfisso (engine: lega più stretto di ^).
        //   fn_mod:       appende "%" — operatore binario livello moltiplicativo.
        "fn_factorial" => append_str(state, "!", false),
        "fn_mod"       => append_str(state, "%", false),

        // ── Operatori bitwise (modalità programmatore) — glyph Unicode dedicati ──
        // is_fresh=false continuano dal risultato (operatori binari/postfissi).
        "op_and" => append_str(state, "∧", false),   // ∧ U+2227
        "op_or"  => append_str(state, "∨", false),   // ∨ U+2228
        "op_xor" => append_str(state, "⊻", false),   // ⊻ U+22BB
        // NOT bitwise unario/prefisso — is_fresh=true (nuovo input, come √/∛).
        "fn_not" => append_str(state, "¬(", true),
        // Shift: UN tasto, 2nd sceglie la direzione — esattamente come fn_sqrt (√/∛).
        "op_shift" => append_str(state, if was_shifted { "≫" } else { "≪" }, false),
        // Rotazione: tasto separato dallo shift (famiglia diversa di operatore
        // anche se stesso livello di precedenza).
        "op_rotate" => append_str(state, if was_shifted { "↻" } else { "↺" }, false),

        // ── Costanti ───────────────────────────────────────────────────────────
        // v2: π ed e sono tasti separati — nessuna 2ª funzione via Shift.
        // Entrambe iniziano un nuovo input (is_fresh=true).
        "const_pi" => append_str(state, "π", true),
        "const_e"  => append_str(state, "e", true),

        // Tasto con carattere associato (cifre, operatori base, parentesi).
        _ => {
            if let Some(c) = ch {
                // Cifra/lettera esadecimale/punto fuori dall'alfabeto della base
                // corrente → ignorata silenziosamente, non entra nel buffer (fix
                // 2026-09-10: prima veniva accettata e falliva solo dopo, a "=").
                // Operatori/parentesi non sono "cifra-simili" e bypassano il controllo.
                let is_digit_like = c.is_ascii_alphanumeric() || c == '.';
                if is_digit_like && !is_valid_digit_for_base(c, state.base_mode) {
                    return;
                }

                // Regola "smart clear after result":
                //   - Se l'ultimo evento era `=` e l'utente preme una cifra (anche
                //     esadecimale A-F) / '.' / '(' → nuovo input.
                //   - Se l'ultimo evento era `=` e l'utente preme un operatore → concatena al risultato.
                //   - Se il buffer è "Error" (risultato di errore) → qualsiasi nuovo tasto lo cancella.
                if state.last_was_result {
                    let is_digit_or_open = is_digit_like || c == '(';
                    if is_digit_or_open || state.buf == "Error" {
                        state.buf.clear();
                    }
                    state.last_was_result = false;
                }
                state.buf.push(c);
            }
            // Tasto sconosciuto (es. data-evt non gestito): ignorato silenziosamente.
        }
    }

    // ── Sticky Shift ───────────────────────────────────────────────────────────
    // Dopo qualsiasi tasto che NON sia "shift" stesso, se il flag era attivo va azzerato.
    // Questo implementa il comportamento "2nd" delle calcolatrici Casio/TI: un solo
    // tasto shiftato, poi torna allo stato normale.
    if was_shifted {
        state.shift = false;
    }
}

/// Genera l'HTML della finestra: display + riga eco + griglia tasti.
///
/// **Display** (prima riga): il buffer corrente reso come HTML 2D (frazioni impilate)
///   se il buffer parserizza, oppure come testo lineare HTML-escaped se è incompleto.
///   La funzione `parse` dell'engine tenta l'interpretazione:
///   - SUCCESS → `render::render(&ast)` (HTML 2D, es. frazione `÷`).
///   - FAILURE → `html_escape(&state.buf)` (lineare; si vede mentre si digita).
///
///   Il display mostra "0" se il buffer è vuoto.
///
/// **Eco** (seconda riga, `.lare-expr`): l'input grezzo verbatim, senza render 2D.
///   - Dopo `=` (`last_was_result == true`): mostra `last_expr` — l'espressione che ha
///     prodotto il risultato. Questo realizza il comportamento "calcolatrice a due righe":
///     display = "56", eco = "7×8".
///   - Mentre si digita (`last_was_result == false`): mostra `buf` direttamente.
fn render_window(state: &CalcState) -> String {
    // In modalità non-Dec il display è SEMPRE lineare (mai render 2D):
    // render.rs non viene proprio chiamato per Hex/Oct/Bin — evita che un buffer
    // "E+1" in Hex venga tokenizzato come Const(E)+Num(1) dal parse decimale.
    let display = if state.base_mode != NumBase::Dec {
        html_escape(&state.buf)
    } else {
        match parse(&state.buf) {
            Ok(ast) => render::render(&ast),
            Err(_)  => html_escape(&state.buf),
        }
    };
    // Buffer vuoto → mostra "0" (display da calcolatrice a riposo).
    let display = if display.is_empty() { "0".to_string() } else { display };

    // Eco dell'input grezzo (verbatim, no render 2D):
    let echo_src = if state.last_was_result { &state.last_expr } else { &state.buf };
    let echo = html_escape(echo_src);

    // Indicatori di stato: DEG/RAD + (se base_mode != Dec) base + larghezza.
    let mode_label = match state.angle_mode {
        AngleMode::Deg => "DEG",
        AngleMode::Rad => "RAD",
    };
    // La base è SEMPRE mostrata (anche Dec, per simmetria — fix 2026-09-10,
    // segnalato da Maurizio: prima Dec non mostrava nulla, incoerente con
    // Hex/Oct/Bin che mostrano sempre la propria etichetta). La larghezza bit
    // resta l'unica cosa nascosta in Dec: è inerte lì (§6 del compito
    // 2026-09-10-calc-modalita-programmatore.md), mostrarla sarebbe fuorviante.
    let base_label = match state.base_mode {
        NumBase::Dec => "DEC",
        NumBase::Hex => "HEX",
        NumBase::Oct => "OCT",
        NumBase::Bin => "BIN",
    };
    let status_line = if state.base_mode == NumBase::Dec {
        format!("{mode_label} · {base_label}")
    } else {
        let width_label = match state.bit_width {
            BitWidth::Byte => "BYTE",
            BitWidth::Word => "WORD",
            BitWidth::Dword => "DWORD",
            BitWidth::Qword => "QWORD",
        };
        format!("{mode_label} · {base_label} · {width_label}")
    };
    // Toggle programmatore: piccolo controllo nella riga di stato.
    let toggle_label = if state.prog_visible { "▲ PROG" } else { "▼ PROG" };
    let toggle = format!("<span class=\"lare-prog-toggle\" data-evt=\"toggle_prog\">{toggle_label}</span>");

    let grid = key_grid(state);
    // Sezione programmatore: visibile solo se state.prog_visible.
    let prog_html = if state.prog_visible {
        programmer_key_grid(state)
    } else {
        String::new()
    };

    format!(
        "<div class=\"lare-window lare-calc\">\
           <div class=\"lare-display\">{display}</div>\
           <div class=\"lare-expr\">{echo}</div>\
           <div class=\"lare-status\">{status_line}{toggle}</div>\
           {grid}\
           {prog_html}\
         </div>"
    )
}

/// Escape HTML minimale per il testo lineare del display.
/// Anche se il buffer proviene dall'input utente (tasti del plugin), è buona pratica
/// non iniettare HTML raw — garantisce che '<', '>' e '&' non rompano il markup.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
     .replace('<', "&lt;")
     .replace('>', "&gt;")
}

/// Genera la griglia HTML dei tasti in base allo stato corrente (v2: layout 7×5, 35 tasti).
///
/// v2 porta il layout da 5+4-colonne-miste a **35 tasti singoli** (7 righe × 5 colonne,
/// nessun `lare-key--wide`). Gli operatori ÷ × − + tornano a cella singola:
/// con 35 celle divisibili esattamente per 5, l'auto-flow CSS non produce salti di riga.
///
/// Layout 7×5 (riga per riga):
///   Riga 1: 2nd | DEG/RAD | sin/asin | cos/acos | tan/atan
///   Riga 2: x²/x³ | √/∛ | x^y/ʸ√x | log/10^x | ln/e^x
///   Riga 3: 1/x | n! | mod | π | e
///   Riga 4: 7 | 8 | 9 | C | ÷
///   Riga 5: 4 | 5 | 6 | ⌫ | ×
///   Riga 6: 1 | 2 | 3 | ( | −
///   Riga 7: 0 | . | = | ) | +
///
/// Etichette shift-aware: quando `state.shift` è true, i tasti scientifici mostrano la
/// loro 2ª funzione. Il tasto DEG/RAD ha etichetta fissa "DEG/RAD" (il tasto è il *toggle*;
/// la modalità corrente è in `.lare-status`, non nell'etichetta del tasto).
///
/// Ogni pulsante ha:
///   `data-evt`  — evento semantico che la UI cattura → `handle_key`.
///   `data-key`  — lista di `KeyboardEvent.key` separati da spazio che attivano questo tasto
///                 dalla tastiera fisica (letto dal listener keydown in `plugin-window.js`).
fn key_grid(state: &CalcState) -> String {
    // Classe aggiuntiva sul tasto "2nd/Shift" quando il flag shift è attivo.
    // `lare-key--shift-on` serve al CSS per evidenziare il tasto premuto.
    let shift_extra = if state.shift { " lare-key--shift-on" } else { "" };

    // Etichette shift-aware: quando shift è attivo mostriamo la 2ª funzione.
    let sin_label    = if state.shift { "asin" } else { "sin" };
    let cos_label    = if state.shift { "acos" } else { "cos" };
    let tan_label    = if state.shift { "atan" } else { "tan" };
    let log_label    = if state.shift { "10^x" } else { "log" };
    let ln_label     = if state.shift { "e^x"  } else { "ln"  };
    // fn_sqrt: √ (radice quadrata) ↔ ∛ (radice cubica) con Shift.
    let sqrt_label   = if state.shift { "\u{221B}" } else { "\u{221A}" }; // ∛ / √
    // fn_square: x² ↔ x³ con Shift.
    let square_label = if state.shift { "x\u{00B3}" } else { "x\u{00B2}" }; // x³ / x²
    // op_pow: x^y ↔ ʸ√x (y-esima radice) con Shift.
    // "ʸ√x" = indice ad apice (U+02B8 MODIFIER LETTER SMALL Y) + simbolo radice + x.
    let pow_label    = if state.shift { "\u{02B8}\u{221A}x" } else { "x^y" }; // ʸ√x / x^y

    format!(
        // ── Riga 1: 2nd + DEG/RAD (etichetta fissa toggle) + trig ────────
        "<div class=\"lare-key-grid\">\
          <button class=\"lare-key{shift_extra}\" data-evt=\"shift\">2nd</button>\
          <button class=\"lare-key\" data-evt=\"mode\">DEG/RAD</button>\
          <button class=\"lare-key\" data-evt=\"fn_sin\">{sin_label}</button>\
          <button class=\"lare-key\" data-evt=\"fn_cos\">{cos_label}</button>\
          <button class=\"lare-key\" data-evt=\"fn_tan\">{tan_label}</button>\
          \
          <button class=\"lare-key\" data-evt=\"fn_square\">{square_label}</button>\
          <button class=\"lare-key\" data-evt=\"fn_sqrt\">{sqrt_label}</button>\
          <button class=\"lare-key\" data-evt=\"op_pow\">{pow_label}</button>\
          <button class=\"lare-key\" data-evt=\"fn_log\">{log_label}</button>\
          <button class=\"lare-key\" data-evt=\"fn_ln\">{ln_label}</button>\
          \
          <button class=\"lare-key\" data-evt=\"fn_recip\">1/x</button>\
          <button class=\"lare-key\" data-evt=\"fn_factorial\">n!</button>\
          <button class=\"lare-key\" data-evt=\"fn_mod\">mod</button>\
          <button class=\"lare-key\" data-evt=\"const_pi\">\u{03C0}</button>\
          <button class=\"lare-key\" data-evt=\"const_e\">e</button>\
          \
          <button class=\"lare-key\" data-evt=\"d7\" data-key=\"7\">7</button>\
          <button class=\"lare-key\" data-evt=\"d8\" data-key=\"8\">8</button>\
          <button class=\"lare-key\" data-evt=\"d9\" data-key=\"9\">9</button>\
          <button class=\"lare-key\" data-evt=\"clear\" data-key=\"Escape Delete\">C</button>\
          <button class=\"lare-key\" data-evt=\"op_div\" data-key=\"/\">\u{00F7}</button>\
          \
          <button class=\"lare-key\" data-evt=\"d4\" data-key=\"4\">4</button>\
          <button class=\"lare-key\" data-evt=\"d5\" data-key=\"5\">5</button>\
          <button class=\"lare-key\" data-evt=\"d6\" data-key=\"6\">6</button>\
          <button class=\"lare-key\" data-evt=\"back\" data-key=\"Backspace\">\u{232B}</button>\
          <button class=\"lare-key\" data-evt=\"op_mul\" data-key=\"* x\">\u{00D7}</button>\
          \
          <button class=\"lare-key\" data-evt=\"d1\" data-key=\"1\">1</button>\
          <button class=\"lare-key\" data-evt=\"d2\" data-key=\"2\">2</button>\
          <button class=\"lare-key\" data-evt=\"d3\" data-key=\"3\">3</button>\
          <button class=\"lare-key\" data-evt=\"paren_open\" data-key=\"(\">(</button>\
          <button class=\"lare-key\" data-evt=\"op_sub\" data-key=\"-\">\u{2212}</button>\
          \
          <button class=\"lare-key\" data-evt=\"d0\" data-key=\"0\">0</button>\
          <button class=\"lare-key\" data-evt=\"dot\" data-key=\". ,\">.</button>\
          <button class=\"lare-key\" data-evt=\"eq\" data-key=\"= Enter\">=</button>\
          <button class=\"lare-key\" data-evt=\"paren_close\" data-key=\")\">)</button>\
          <button class=\"lare-key\" data-evt=\"op_add\" data-key=\"+\">+</button>\
        </div>"
    )
}

/// Genera la griglia HTML della sezione programmatore: 4 righe × 5 colonne, 20 tasti.
/// Visibile solo quando `state.prog_visible` — toggle nella riga di stato.
///
/// Layout (4 righe × 5 colonne):
///   Riga 1 (in cima):        D   | E   | F   | AND | OR
///   Riga 2:                  A   | B   | C   | XOR | SHL/SHR (2nd)
///   Riga 3:                  DEC | HEX | OCT | BIN | NOT
///   Riga 4 (in fondo):       BYTE| WORD| DWORD|QWORD| ROL/ROR (2nd)
///
/// Ordine A-F (fix 2026-09-10, due giri di correzione su indicazione di
/// Maurizio): coerente con la tastiera decimale esistente, dove le cifre
/// iniziano dal BASSO (0 in fondo) e salgono (1-2-3, poi 4-5-6, poi 7-8-9 in
/// cima) — sempre da sinistra a destra dentro ogni riga. Le 6 cifre
/// esadecimali seguono la STESSA logica sul blocco 3 colonne × 2 righe che
/// occupano (colonne 1-3 di Riga 1/2): contando le 4 righe DAL BASSO, A parte
/// dalla 3ª riga (= Riga 2 qui, la seconda dall'alto) e prosegue prima a
/// destra (A→B→C) poi in alto, alla 4ª riga dal basso (= Riga 1, la
/// TOPMOST) per D→E→F. Gli altri tasti (base/larghezza/booleani) sono stati
/// riorganizzati di conseguenza nelle 2 righe restanti, in basso.
///
/// Etichette shift-aware per shift e rotate (stesso pattern di sqrt/square_label).
fn programmer_key_grid(state: &CalcState) -> String {
    let shift_label = if state.shift { "SHR" } else { "SHL" };
    let rotate_label = if state.shift { "ROR" } else { "ROL" };

    format!(
        "<div class=\"lare-prog-section\">\
          <div class=\"lare-key-grid\">\
            <button class=\"lare-key\" data-evt=\"hexD\" data-key=\"d D\">D</button>\
            <button class=\"lare-key\" data-evt=\"hexE\" data-key=\"e E\">E</button>\
            <button class=\"lare-key\" data-evt=\"hexF\" data-key=\"f F\">F</button>\
            <button class=\"lare-key\" data-evt=\"op_and\">AND</button>\
            <button class=\"lare-key\" data-evt=\"op_or\">OR</button>\
            \
            <button class=\"lare-key\" data-evt=\"hexA\" data-key=\"a A\">A</button>\
            <button class=\"lare-key\" data-evt=\"hexB\" data-key=\"b B\">B</button>\
            <button class=\"lare-key\" data-evt=\"hexC\" data-key=\"c C\">C</button>\
            <button class=\"lare-key\" data-evt=\"op_xor\">XOR</button>\
            <button class=\"lare-key\" data-evt=\"op_shift\">{shift_label}</button>\
            \
            <button class=\"lare-key\" data-evt=\"base_dec\">DEC</button>\
            <button class=\"lare-key\" data-evt=\"base_hex\">HEX</button>\
            <button class=\"lare-key\" data-evt=\"base_oct\">OCT</button>\
            <button class=\"lare-key\" data-evt=\"base_bin\">BIN</button>\
            <button class=\"lare-key\" data-evt=\"fn_not\">NOT</button>\
            \
            <button class=\"lare-key\" data-evt=\"width_byte\">BYTE</button>\
            <button class=\"lare-key\" data-evt=\"width_word\">WORD</button>\
            <button class=\"lare-key\" data-evt=\"width_dword\">DWORD</button>\
            <button class=\"lare-key\" data-evt=\"width_qword\">QWORD</button>\
            <button class=\"lare-key\" data-evt=\"op_rotate\">{rotate_label}</button>\
          </div>\
        </div>"
    )
}

/// Loop principale: legge messaggi JSON dal host (stdin) riga per riga, risponde su stdout.
///
/// Protocollo (Contract P):
///   Init        → Ready { name: "calc", protocol_version: 1 }
///   Activate    → resetta lo stato + ShowWindow (HTML iniziale della calcolatrice)
///   UiEvent     → handle_key + UpdateWindow (HTML aggiornato)
///   Deinit      → break (terminazione pulita del processo)
///
/// Modello: identico a `plugin-counter` (single-task, niente tokio: i plugin stdio
/// sono sequenziali per design — un solo utente alla volta interagisce con la finestra).
fn main() {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut state = CalcState::default();

    // `window_id` viene ricevuto in Activate e NON viene salvato perché in Slice 2
    // c'è una sola finestra e ogni UiEvent porta il proprio `wid` nel pattern.
    // La variabile è dichiarata comunque per future estensioni (es. Slice 3 multi-window).
    let mut window_id: u64 = 0;

    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() { continue; }
        let Ok(msg) = serde_json::from_str::<HostToPlugin>(&line) else { continue };

        let replies: Vec<PluginToHost> = match msg {
            // Init → solo negozio il protocollo e mi presento con il nome.
            // "calc" è il nome interno; "Calcolatrice" è il titolo della finestra (in Activate).
            HostToPlugin::Init { .. } => {
                vec![PluginToHost::Ready { name: "calc".into(), protocol_version: 1 }]
            }

            // Activate → resetto lo stato (nuova sessione calcolatrice) e apro la finestra.
            HostToPlugin::Activate { window_id: wid, .. } => {
                window_id = wid;
                state = CalcState::default();
                vec![PluginToHost::ShowWindow {
                    window_id: wid,
                    title: "Calcolatrice".into(),
                    html: render_window(&state),
                }]
            }

            // UiEvent → gestisco il tasto e aggiorno la finestra.
            HostToPlugin::UiEvent { window_id: wid, element_id, .. } => {
                handle_key(&mut state, &element_id);
                vec![PluginToHost::UpdateWindow { window_id: wid, html: render_window(&state) }]
            }

            // Deinit → uscita pulita (break dal loop → il processo termina).
            HostToPlugin::Deinit {} => break,
        };

        // Serializzo e invio ogni risposta su stdout (una per riga, come da Contract P).
        for reply in replies {
            let mut out = serde_json::to_string(&reply).unwrap();
            out.push('\n');
            if stdout.write_all(out.as_bytes()).is_err() {
                return; // pipe rotta → uscita silenziosa
            }
            let _ = stdout.flush();
        }
    }

    // Sopprime il warning "window_id assigned but never read" per il caso Deinit
    // (Activate scrive, Deinit esce prima di qualunque lettura).
    // In Slice 3 (multi-window) questa variabile sarà effettivamente usata.
    let _ = window_id;
}

// ─── Tests ────────────────────────────────────────────────────────────────────
// La suite testifica il comportamento di CalcState, handle_key, render_window.
// I test sono stati scritti PRIMA dell'implementazione (TDD: RED → GREEN).
// Ogni test è una specifica leggibile; l'utente può capire il comportamento
// del plugin solo leggendo i test, senza aprire l'app.
#[cfg(test)]
mod tests {
    use super::*;
    // `evaluate` è usato solo dai test (il codice non-test usa `evaluate_with_width`).
    use engine::evaluate;

    /// Helper: parte da uno stato default e applica una sequenza di tasti.
    /// Equivalente a "l'utente ha premuto questi tasti dall'inizio".
    fn keys(seq: &[&str]) -> CalcState {
        let mut s = CalcState::default();
        for k in seq { handle_key(&mut s, k); }
        s
    }

    #[test]
    fn typing_builds_buffer() {
        // d7 op_mul d8 → buffer "7×8" (simbolo display U+00D7).
        assert_eq!(keys(&["d7", "op_mul", "d8"]).buf, "7×8");
    }

    #[test]
    fn equals_computes_and_formats() {
        // 7 × 8 = 56 (intero → nessun decimale, come da smart formatter).
        assert_eq!(keys(&["d7", "op_mul", "d8", "eq"]).buf, "56");
    }

    #[test]
    fn equals_div_by_zero_is_error() {
        // 1 ÷ 0 → "Error" (engine ritorna DivByZero → gestito come "Error").
        assert_eq!(keys(&["d1", "op_div", "d0", "eq"]).buf, "Error");
    }

    #[test]
    fn backspace_and_clear() {
        // back toglie l'ultimo carattere; clear svuota completamente.
        assert_eq!(keys(&["d1", "d2", "back"]).buf, "1");
        assert_eq!(keys(&["d1", "d2", "clear"]).buf, "");
    }

    #[test]
    fn digit_after_result_starts_fresh() {
        // Dopo `=`, una cifra inizia un nuovo input (cancella il risultato precedente).
        assert_eq!(keys(&["d2", "eq", "d5"]).buf, "5");
    }

    #[test]
    fn op_after_result_continues() {
        // Dopo `=`, un operatore continua dal risultato (2 + 3 = 5).
        assert_eq!(keys(&["d2", "eq", "op_add", "d3", "eq"]).buf, "5");
    }

    #[test]
    fn window_has_display_and_keys() {
        // La finestra contiene il display e la griglia tasti; 6÷3 è valido → frazione 2D.
        let h = render_window(&keys(&["d6", "op_div", "d3"]));
        assert!(h.contains("lare-display") && h.contains("lare-key-grid"));
        assert!(h.contains("lare-frac"));         // 6÷3 parserizza → render 2D
        assert!(h.contains("data-evt=\"eq\""));   // il tasto = è presente
    }

    #[test]
    fn incomplete_shows_linear() {
        // "6÷" è incompleto → parse fallisce → display lineare (nessuna frazione).
        let h = render_window(&keys(&["d6", "op_div"]));
        assert!(!h.contains("lare-frac")); // lineare finché non parserizza
    }

    #[test]
    fn window_is_marked_calc() {
        // La finestra calcolatrice porta la classe `lare-calc`: la UI ci aggancia il
        // display grow-only (min-height high-water) senza toccare gli altri plugin.
        let h = render_window(&CalcState::default());
        assert!(h.contains("class=\"lare-window lare-calc\""));
    }

    // ── Nuovi test TDD per C-1 (esponenziale via handle_key) ──────────────────

    #[test]
    fn continue_from_exponential_result_does_not_error() {
        // Bug C-1 via handle_key: dopo 1000000 × 1000000 = il buffer diventa "1e12"
        // (o simile notazione esponenziale). Premere "+ 3 =" deve dare un risultato
        // numerico valido, NON "Error".
        //
        // La sequenza di tasti per "1000000":
        let million = ["d1","d0","d0","d0","d0","d0","d0"];
        let mut s = CalcState::default();
        for k in &million { handle_key(&mut s, k); }   // digita 1000000
        handle_key(&mut s, "op_mul");
        for k in &million { handle_key(&mut s, k); }   // digita 1000000
        handle_key(&mut s, "eq");   // = → buf diventa es. "1e12"

        // A questo punto handle_key ha messo nel buf il risultato esponenziale.
        // Ora l'utente continua: preme + 3 =
        handle_key(&mut s, "op_add");   // operatore → non resetta il buf, lo continua
        handle_key(&mut s, "d3");
        handle_key(&mut s, "eq");       // ricalcola "1e12+3" → dovrebbe dare ~1000000000003

        // Il buf NON deve essere "Error" (era il bug C-1).
        assert_ne!(s.buf, "Error",
            "continuare da un risultato esponenziale non deve dare Error, buf={:?}", s.buf);
        // Il buf deve essere ri-parsabile: se il formatter produce "1.000000000003e12"
        // o simile, l'engine deve poterlo leggere per un calcolo successivo.
        assert!(parse(&s.buf).is_ok(),
            "il buffer risultante deve essere ri-parsabile dall'engine, buf={:?}", s.buf);
    }

    // ── Nuovi test TDD per Task 1 Slice 2a — riga eco "espressione digitata" ────

    #[test]
    fn eq_captures_last_expr() {
        // "=" cattura l'espressione digitata PRIMA di sovrascrivere il buffer col risultato.
        assert_eq!(keys(&["d7", "op_mul", "d8", "eq"]).last_expr, "7×8");
    }

    #[test]
    fn equals_keeps_expression_in_echo() {
        // Due righe: dopo "=" il display mostra il risultato (56), l'eco l'espressione grezza (7×8).
        let h = render_window(&keys(&["d7", "op_mul", "d8", "eq"]));
        assert!(h.contains("7×8"), "eco deve mostrare l'espressione grezza");
        assert!(h.contains("56"), "display deve mostrare il risultato");
    }

    #[test]
    fn echo_shows_raw_buffer_while_typing() {
        // Mentre si digita, l'eco mostra il buffer corrente verbatim (no render 2D).
        let h = render_window(&keys(&["d6", "op_div", "d4"]));
        assert!(h.contains("6÷4"), "eco deve mostrare l'input grezzo mentre si digita");
    }

    #[test]
    fn digit_after_result_resets_echo() {
        // Dopo "=", una cifra inizia un nuovo input → l'eco non mostra più la vecchia espressione.
        let s = keys(&["d7", "op_mul", "d8", "eq", "d3"]);
        assert_eq!(s.buf, "3");
        assert!(!s.last_was_result);
        let h = render_window(&s);
        assert!(!h.contains("7×8"), "la vecchia espressione non deve restare come eco");
    }

    #[test]
    fn clear_resets_last_expr() {
        // C azzera anche last_expr.
        assert_eq!(keys(&["d7", "op_mul", "d8", "eq", "clear"]).last_expr, "");
    }

    // ── Task 2 Slice 2b — data-key sui pulsanti (input da tastiera) ───────────

    #[test]
    fn key_grid_has_data_keys() {
        // KEY_GRID è ora una funzione; chiamiamo con stato default per il test di baseline.
        // Il contratto: tutti i pulsanti numerici/operatori devono dichiarare il tasto fisico
        // che li attiva (attributo `data-key`) per il listener `keydown` in plugin-window.js.
        let g = key_grid(&CalcState::default());
        assert!(g.contains("data-key=\"7\""));
        assert!(g.contains("data-key=\"= Enter\""));   // = e Invio
        assert!(g.contains("data-key=\"Backspace\""));
        assert!(g.contains("data-key=\"Escape Delete\""));
        assert!(g.contains("data-key=\"* x\""));       // * e x → moltiplicazione
        assert!(g.contains("data-key=\". ,\""));       // . e , → punto decimale
    }

    // ══ Task 3 — Shift sticky + DEG/RAD + tasti scientifici (RED prima, GREEN dopo) ══

    #[test]
    fn shift_toggle() {
        // "shift" attiva il flag; "shift" di nuovo lo spegne (toggle).
        assert!(keys(&["shift"]).shift,
            "dopo 'shift' lo stato .shift deve essere true");
        assert!(!keys(&["shift", "shift"]).shift,
            "due 'shift' → .shift deve tornare false");
    }

    #[test]
    fn shift_fn_sin_appends_asin_and_turns_off() {
        // Con shift attivo, fn_sin → "asin(" e il flag shift si azzera (sticky).
        let s = keys(&["shift", "fn_sin"]);
        assert_eq!(s.buf, "asin(",
            "shift+fn_sin deve appendere 'asin(', got {:?}", s.buf);
        assert!(!s.shift,
            "shift deve spegnersi (sticky) dopo il tasto scientifico");
    }

    #[test]
    fn fn_sin_no_shift_appends_sin() {
        // Senza shift, fn_sin → "sin(".
        assert_eq!(keys(&["fn_sin"]).buf, "sin(",
            "fn_sin senza shift deve appendere 'sin('");
    }

    #[test]
    fn mode_toggle() {
        // "mode" alterna la modalità angolare: Deg → Rad → Deg.
        assert_eq!(keys(&["mode"]).angle_mode, AngleMode::Rad,
            "il primo 'mode' da Deg (default) deve passare a Rad");
        assert_eq!(keys(&["mode", "mode"]).angle_mode, AngleMode::Deg,
            "due 'mode' → torna a Deg");
    }

    #[test]
    fn shift_fn_sqrt_appends_cbrt_or_sqrt() {
        // v2: shift + fn_sqrt → "∛(" (radice cubica, funzione prefissa, nuovo input);
        // fn_sqrt normale → "√(" (funzione radice quadrata, nuovo input).
        // Entrambe le varianti aprono una parentesi → is_fresh=true.
        // Il vecchio comportamento shift → "^2" si sposta sul nuovo tasto fn_square (v2).
        assert_eq!(keys(&["shift", "fn_sqrt"]).buf, "∛(",
            "shift+fn_sqrt deve appendere '∛(' (radice cubica)");
        assert_eq!(keys(&["fn_sqrt"]).buf, "√(",
            "fn_sqrt senza shift deve appendere '√('");
    }

    #[test]
    fn const_pi_appends_pi_no_shift() {
        // v2: π ed e sono tasti separati — const_pi non ha più una 2ª funzione via Shift.
        // Premere const_pi produce sempre "π", anche con shift attivo (lo Shift è consumato
        // ma ignorato dal ramo const_pi). La costante e è sul tasto separato const_e.
        assert_eq!(keys(&["const_pi"]).buf, "π",
            "const_pi deve appendere 'π'");
        // Con shift attivo, const_pi deve comunque appendere "π" (nessuna 2ª funzione in v2).
        // (In v1 produceva "e"; ora il tasto separato const_e gestisce la costante di Eulero.)
        assert_eq!(keys(&["shift", "const_pi"]).buf, "π",
            "shift+const_pi deve appendere 'π' — nessuna 2ª funzione in v2");
    }

    #[test]
    fn op_pow_appends_caret_and_fn_recip_appends_inv() {
        // op_pow → "^" (operatore potenza, continua dal risultato);
        // fn_recip → "^-1" (operatore reciproco).
        assert_eq!(keys(&["op_pow"]).buf, "^",
            "op_pow deve appendere '^'");
        assert_eq!(keys(&["fn_recip"]).buf, "^-1",
            "fn_recip deve appendere '^-1'");
    }

    #[test]
    fn fn_cos_and_fn_tan_append_correctly() {
        // Copertura: fn_cos e fn_tan condividono la struttura di fn_sin ma
        // producono stringhe *diverse* — un typo sarebbe silenzioso senza test dedicati.
        assert_eq!(keys(&["fn_cos"]).buf, "cos(");
        assert_eq!(keys(&["shift", "fn_cos"]).buf, "acos(");
        assert_eq!(keys(&["fn_tan"]).buf, "tan(");
        assert_eq!(keys(&["shift", "fn_tan"]).buf, "atan(");
    }

    #[test]
    fn fn_log_and_fn_ln_append_correctly() {
        // Copertura: fn_log e fn_ln e le loro varianti shift ("10^(" e "e^(").
        // Analogo a fn_cos/fn_tan: le stringhe shiftate sono diverse e non coperte da fn_sin.
        assert_eq!(keys(&["fn_log"]).buf, "log(");
        assert_eq!(keys(&["shift", "fn_log"]).buf, "10^(");
        assert_eq!(keys(&["fn_ln"]).buf, "ln(");
        assert_eq!(keys(&["shift", "fn_ln"]).buf, "e^(");
    }

    #[test]
    fn sin30_deg_evaluates_to_half() {
        // sin(30) in modalità Deg = 0.5 (esatto all'arrotondamento IEEE 754 / 10 dp).
        // Sequenza: fn_sin → buf "sin("; d3 → "sin(3"; d0 → "sin(30";
        //           paren_close → "sin(30)"; eq → evaluta con AngleMode::Deg.
        // FAIL finché handle_key non gestisce fn_sin e finché eq non usa state.angle_mode.
        let s = keys(&["fn_sin", "d3", "d0", "paren_close", "eq"]);
        assert_eq!(s.buf, "0.5",
            "sin(30) in DEG deve valere 0.5, got {:?}", s.buf);
    }

    #[test]
    fn render_shows_deg_by_default_and_shift_on_class() {
        // render_window con stato default deve contenere "DEG" come indicatore modalità angolare.
        let h = render_window(&CalcState::default());
        assert!(h.contains("DEG"),
            "la finestra deve mostrare 'DEG' (modalità default), html={h:?}");
        assert!(h.contains("lare-key-grid"),
            "la finestra deve contenere la griglia tasti");

        // Con shift attivo, il tasto "2nd" deve avere la classe lare-key--shift-on.
        // Usiamo struct-update syntax per evitare il warning `field_reassign_with_default`.
        let s = CalcState { shift: true, ..Default::default() };
        let h2 = render_window(&s);
        assert!(h2.contains("lare-key--shift-on"),
            "con shift attivo la griglia deve includere 'lare-key--shift-on'");
    }

    #[test]
    fn engine_round_trips_its_own_formatter_output() {
        // Invariante fondamentale: per ogni valore x che format_number produce,
        // l'engine deve poter ri-parsare e ri-valutare la stringa ottenendo x.
        //
        // Questo è il contratto tra format.rs e engine.rs:
        //   engine::evaluate(engine::parse(format::format_number(x))) ≈ x
        //
        // Usiamo tolleranza relativa per coprire errori di arrotondamento f64:
        //   |back - x| <= |x| * 1e-9 + 1e-12
        let values: &[f64] = &[1e12, 4.5e-9, 1.23e15, 1_000_000_000_003.0, 42.0, 0.5];
        for &x in values {
            let formatted = format_number(x);
            let parsed = parse(&formatted)
                .unwrap_or_else(|e| panic!("parse fallito per format_number({x}) = {formatted:?}: {e:?}"));
            let back = evaluate(&parsed, AngleMode::Rad)
                .unwrap_or_else(|e| panic!("evaluate fallito per {formatted:?}: {e:?}"));
            let rel_tol = x.abs() * 1e-9 + 1e-12;
            assert!(
                (back - x).abs() <= rel_tol,
                "round-trip fallito per x={x}: format=\"{formatted}\", back={back}, diff={}",
                (back - x).abs()
            );
        }
    }

    // ══ Task vB — layout 7×5, nuovi tasti, Shift v2 (RED prima, GREEN dopo) ══

    #[test]
    fn fn_square_appends_pow2_or_pow3() {
        // fn_square (nuovo tasto v2): elevazione a potenza fissa, operatore (is_fresh=false).
        //   Normale: ^2 (al quadrato); Shift: ^3 (al cubo).
        // is_fresh=false significa che continua dal risultato precedente senza cancellarlo.
        assert_eq!(keys(&["fn_square"]).buf, "^2",
            "fn_square senza shift deve appendere '^2'");
        assert_eq!(keys(&["shift", "fn_square"]).buf, "^3",
            "shift+fn_square deve appendere '^3' (cubo)");
    }

    #[test]
    fn shift_op_pow_appends_yroot() {
        // op_pow v2: la 2ª funzione via Shift è y√x = "x^(1/y)".
        // Premendo shift+op_pow si appende "^(1/" (l'utente digita poi l'indice radice e ")").
        // Entrambe le varianti continuano dal risultato (is_fresh=false — operatori).
        assert_eq!(keys(&["op_pow"]).buf, "^",
            "op_pow senza shift deve appendere '^'");
        assert_eq!(keys(&["shift", "op_pow"]).buf, "^(1/",
            "shift+op_pow deve appendere '^(1/' (y-esima radice)");
    }

    #[test]
    fn const_e_appends_e() {
        // const_e (nuovo tasto v2): appende la costante di Eulero "e" (is_fresh=true).
        // Prima di v2 era la 2ª funzione di const_pi; ora è un tasto autonomo.
        assert_eq!(keys(&["const_e"]).buf, "e",
            "const_e deve appendere 'e' (costante di Eulero)");
    }

    #[test]
    fn fn_factorial_appends_bang() {
        // fn_factorial (nuovo tasto v2): appende "!" (fattoriale postfisso, is_fresh=false).
        // L'engine riconosce il token "!" come operatore postfisso più stretto di "^".
        // is_fresh=false: su "5" produce "5!", non "!" standalone.
        assert_eq!(keys(&["fn_factorial"]).buf, "!",
            "fn_factorial deve appendere '!'");
    }

    #[test]
    fn fn_mod_appends_percent() {
        // fn_mod (nuovo tasto v2): appende "%" (modulo, operatore binario, is_fresh=false).
        // L'engine tratta "%" come operatore moltiplicativo (stessa precedenza di × e ÷).
        assert_eq!(keys(&["fn_mod"]).buf, "%",
            "fn_mod deve appendere '%'");
    }

    #[test]
    fn factorial_five_equals_120() {
        // End-to-end: 5! = 120. Sequenza: d5 → buf "5"; fn_factorial → "5!"; eq → "120".
        // Verifica che fn_factorial sia cablato correttamente e che l'engine valuti il fattoriale.
        let s = keys(&["d5", "fn_factorial", "eq"]);
        assert_eq!(s.buf, "120",
            "5! deve essere 120, buf={:?}", s.buf);
    }

    #[test]
    fn modulo_seven_mod_three_equals_one() {
        // End-to-end: 7 % 3 = 1. Sequenza: d7 → "7"; fn_mod → "7%"; d3 → "7%3"; eq → "1".
        // Verifica che fn_mod sia cablato e che l'engine valuti l'operatore modulo.
        let s = keys(&["d7", "fn_mod", "d3", "eq"]);
        assert_eq!(s.buf, "1",
            "7%%3 deve essere 1, buf={:?}", s.buf);
    }

    #[test]
    fn cbrt_twenty_seven_equals_three() {
        // End-to-end: ∛(27) = 3. Sequenza: shift+fn_sqrt → "∛("; d2 d7 → "∛(27";
        // paren_close → "∛(27)"; eq → "3".
        // Verifica shift di fn_sqrt (→ "∛(") + engine FuncId::Cbrt + format.
        let s = keys(&["shift", "fn_sqrt", "d2", "d7", "paren_close", "eq"]);
        assert_eq!(s.buf, "3",
            "∛(27) deve essere 3, buf={:?}", s.buf);
    }

    #[test]
    fn render_has_lare_status_with_mode_indicator() {
        // v2: la riga stato `.lare-status` sta TRA `.lare-expr` e la griglia tasti.
        // Il tasto DEG/RAD ha etichetta fissa "DEG/RAD" (non la modalità corrente).
        // La modalità corrente (DEG default) è nell'indicatore `.lare-status`.
        // Il vecchio `<span class="lare-mode">` è rimosso da `.lare-expr`.
        let h = render_window(&CalcState::default());
        // 1. Esiste la riga stato con la classe lare-status.
        assert!(h.contains("lare-status"),
            "render_window deve contenere 'lare-status', html={h:?}");
        // 2. Il tasto DEG/RAD ha l'etichetta fissa "DEG/RAD".
        assert!(h.contains("DEG/RAD"),
            "il tasto mode deve avere etichetta fissa 'DEG/RAD', html={h:?}");
        // 3. L'indicatore di modalità DEG è nella riga stato.
        assert!(h.contains("lare-status"),
            "DEG deve apparire nella riga stato, html={h:?}");
        // 4. Il vecchio span lare-mode è rimosso (l'indicatore non è più nell'eco).
        assert!(!h.contains("lare-mode"),
            "la classe 'lare-mode' non deve più comparire nell'eco, html={h:?}");
    }

    // ══ Modalità programmatore (Parte C) — nuovi tasti, UI, conversione ═══════

    #[test]
    fn toggle_prog_visibility() {
        let s = keys(&["toggle_prog"]);
        assert!(s.prog_visible, "toggle_prog deve attivare prog_visible");
        let s2 = keys(&["toggle_prog", "toggle_prog"]);
        assert!(!s2.prog_visible, "doppio toggle deve tornare falso");
        // Nascondere la sezione non resetta base/larghezza.
        assert_eq!(s2.base_mode, NumBase::Dec);
    }

    #[test]
    fn prog_section_not_visible_by_default() {
        let h = render_window(&CalcState::default());
        assert!(!h.contains("lare-prog-section"),
            "di default la sezione programmatore non deve essere visibile");
    }

    #[test]
    fn prog_section_visible_after_toggle() {
        let s = keys(&["toggle_prog"]);
        let h = render_window(&s);
        assert!(h.contains("lare-prog-section"),
            "dopo toggle la sezione programmatore deve apparire");
        assert!(h.contains("DEC"), "i tasti base devono essere presenti");
    }

    /// Ordine dei tasti esadecimali A-F: fix 2026-09-10, segnalato da Maurizio —
    /// deve rispecchiare la tastiera decimale esistente (cifre basse in basso,
    /// alte in alto, sinistra-destra dentro ogni riga). Nell'HTML generato
    /// (reso riga per riga dall'alto) questo significa D-E-F PRIMA di A-B-C:
    /// la riga D-E-F è visivamente sopra, la riga A-B-C sotto — A in basso a
    /// sinistra, F in alto a destra, come 0 in basso e 9 in alto a destra.
    #[test]
    fn hex_digit_order_matches_decimal_keypad_convention() {
        let h = render_window(&keys(&["toggle_prog"]));
        let pos_d = h.find("data-evt=\"hexD\"").expect("hexD deve essere presente");
        let pos_a = h.find("data-evt=\"hexA\"").expect("hexA deve essere presente");
        assert!(pos_d < pos_a,
            "hexD deve comparire PRIMA di hexA nell'HTML (riga D-E-F sopra, A-B-C sotto)");
        let pos_f = h.find("data-evt=\"hexF\"").expect("hexF deve essere presente");
        let pos_c = h.find("data-evt=\"hexC\"").expect("hexC deve essere presente");
        assert!(pos_f < pos_c,
            "hexF (fine della riga alta) deve comparire prima di hexC (fine della riga bassa)");
    }

    /// Posizione dell'intero blocco esadecimale rispetto alle altre righe: fix
    /// 2026-09-10 (bis) — Maurizio ha chiesto che A parta dalla 3ª riga contando
    /// dal basso, quindi il blocco D-E-F/A-B-C deve stare SOPRA la riga
    /// base (DEC/HEX/OCT/BIN/NOT), che a sua volta sta sopra la riga larghezza
    /// (BYTE/WORD/DWORD/QWORD/ROL-ROR) — quest'ultima resta l'ultima riga (in
    /// fondo, come già prima di questo fix).
    #[test]
    fn hex_block_sits_above_base_and_width_rows() {
        let h = render_window(&keys(&["toggle_prog"]));
        let pos_hex_a = h.find("data-evt=\"hexA\"").expect("hexA presente");
        let pos_base_dec = h.find("data-evt=\"base_dec\"").expect("base_dec presente");
        let pos_width_byte = h.find("data-evt=\"width_byte\"").expect("width_byte presente");
        assert!(pos_hex_a < pos_base_dec,
            "il blocco esadecimale deve stare sopra la riga base (DEC/HEX/OCT/BIN)");
        assert!(pos_base_dec < pos_width_byte,
            "la riga base deve stare sopra la riga larghezza (BYTE/WORD/DWORD/QWORD)");
    }

    /// Conversione: 255 in Dec, poi switch a Hex (a Qword default) → 16 cifre.
    #[test]
    fn convert_dec_to_hex_qword() {
        let s = keys(&["d2", "d5", "d5", "base_hex"]);
        assert_eq!(s.buf, "0000_0000_0000_00FF",
            "255 in Hex a Qword: {}, want 16 cifre raggruppate", s.buf);
        assert_eq!(s.base_mode, NumBase::Hex);
    }

    /// Conversione con Byte prima: 255 → Byte → Hex → "FF" nudo.
    #[test]
    fn convert_dec_to_hex_byte() {
        let s = keys(&["d2", "d5", "d5", "width_byte", "base_hex"]);
        assert_eq!(s.buf, "FF",
            "255 in Hex a Byte: {}, want solo FF", s.buf);
        assert_eq!(s.base_mode, NumBase::Hex);
        assert_eq!(s.bit_width, BitWidth::Byte);
    }

    /// Test integrato end-to-end: intera catena Hex→Byte, AND, Bin→Dec.
    #[test]
    fn integrated_programmer_sequence() {
        let s = keys(&[
            "base_hex", "width_byte", "hexF", "hexF", "eq",
        ]);
        assert_eq!(s.buf, "FF", "0xFF=255 a Byte → FF");

        let s = keys(&[
            "base_hex", "width_byte", "hexF", "hexF", "eq",
            "op_and", "d0", "hexF", "eq",
        ]);
        assert_eq!(s.buf, "0F", "0xFF AND 0x0F = 0x0F");

        let s = keys(&[
            "base_hex", "width_byte", "hexF", "hexF", "eq",
            "op_and", "d0", "hexF", "eq",
            "base_bin",
        ]);
        assert_eq!(s.buf, "0000_1111", "15 in Bin/Byte deve essere 0000_1111");

        let s = keys(&[
            "base_hex", "width_byte", "hexF", "hexF", "eq",
            "op_and", "d0", "hexF", "eq",
            "base_bin",
            "base_dec",
        ]);
        assert_eq!(s.buf, "15", "tornando a Dec mostra il valore reale 15");
    }

    /// Espressione incompleta: cambio base non converte il buffer.
    #[test]
    fn incomplete_expression_no_conversion() {
        let s = keys(&["d5", "op_add", "base_hex"]);
        assert_eq!(s.buf, "5+", "espressione incompleta → buffer invariato");
        assert_eq!(s.base_mode, NumBase::Hex, "la modalità base cambia comunque");
    }

    /// Overflow silenzioso: 9 cifre binarie a Byte → troncate.
    #[test]
    fn overflow_input_truncates() {
        // 1 seguito da 8 "0" = "100000000" (9 cifre) = 256 decimale.
        // In Bin a Byte: troncato a 8 bit bassi → 0 → "0000_0000".
        let mut v = vec!["width_byte", "base_bin", "d1"];
        v.extend_from_slice(&["d0"; 8]);
        v.push("eq");
        let seq = v;
        let s = keys(&seq);
        assert_eq!(s.buf, "0000_0000",
            "overflow di input (9 bit in Byte) deve essere troncato a 0");
    }

    /// La base è sempre mostrata (anche Dec, per simmetria con Hex/Oct/Bin —
    /// segnalato da Maurizio dal vivo, 2026-09-10). La larghezza bit resta
    /// inerte/nascosta in Dec (non è mai stata un problema, solo la base
    /// mancava quando era Dec).
    #[test]
    fn status_line_dec_shows_deg_and_dec() {
        let h = render_window(&CalcState::default());
        assert!(h.contains("DEG"), "status line con Dec default deve contenere DEG");
        assert!(h.contains("DEC"), "status line con Dec deve mostrare anche DEC (simmetria)");
        assert!(!h.contains("QWORD"), "status line con Dec NON deve mostrare la larghezza");
    }

    /// In Hex, la riga stato mostra base + larghezza.
    #[test]
    fn status_line_hex_shows_base_and_width() {
        let s = keys(&["base_hex"]);
        let h = render_window(&s);
        assert!(h.contains("HEX"), "status line in Hex deve contenere HEX");
        assert!(h.contains("QWORD"), "status line in Hex deve contenere la larghezza");
    }

    // ══ Fix 2026-09-10 (bis) — cifre fuori dall'alfabeto della base corrente ═══
    // Segnalato da Maurizio dal vivo: in Bin, digitando "1101" poi "2", il tasto
    // "2" veniva accettato nel buffer (visibile solo come "Error" al successivo
    // "="). Il gap: i tasti cifra (d0-d9, hexA-F, '.') non erano mai stati
    // ristretti all'alfabeto della base attiva — solo l'engine, a "=", rifiutava
    // il letterale. Fix: il tasto invalido per la base corrente viene ignorato
    // silenziosamente (non entra nel buffer), come un tasto disabilitato su una
    // calcolatrice fisica — non serve prima digitare ed errare dopo.

    /// Il caso esatto segnalato da Maurizio: "2" in modalità Bin viene ignorato.
    #[test]
    fn bin_mode_rejects_digit_2() {
        let s = keys(&["base_bin", "d1", "d1", "d0", "d1", "d2"]);
        assert_eq!(s.buf, "1101",
            "in Bin, il tasto '2' deve essere ignorato (non è una cifra binaria valida), got {:?}",
            s.buf);
    }

    /// Oct rifiuta 8 e 9 (non cifre ottali valide).
    #[test]
    fn oct_mode_rejects_digits_8_and_9() {
        let s = keys(&["base_oct", "d7", "d8", "d9"]);
        assert_eq!(s.buf, "7",
            "in Oct, '8' e '9' devono essere ignorati, got {:?}", s.buf);
    }

    /// Le cifre esadecimali A-F sono valide SOLO in Hex — rifiutate altrove
    /// (anche in Dec, dove oggi "sin"/"cos" sono le uniche lettere ammesse
    /// nel buffer, mai una cifra esadecimale isolata).
    #[test]
    fn hex_digits_rejected_outside_hex_mode() {
        assert_eq!(keys(&["hexA"]).buf, "",
            "in Dec, 'A' non è una cifra valida — deve essere ignorata");
        assert_eq!(keys(&["base_bin", "hexA"]).buf, "",
            "in Bin, 'A' non è una cifra valida — deve essere ignorata");
        assert_eq!(keys(&["base_oct", "hexA"]).buf, "",
            "in Oct, 'A' non è una cifra valida — deve essere ignorata");
        // In Hex invece è valida.
        assert_eq!(keys(&["base_hex", "hexA"]).buf, "A",
            "in Hex, 'A' è una cifra valida e deve essere accettata");
    }

    /// Bonus verificato nello stesso punto di codice: la regola "smart clear
    /// after result" ora si applica anche alle cifre esadecimali (prima solo
    /// `is_ascii_digit()` la innescava — "F" dopo un "=" in Hex concatenava
    /// invece di iniziare un nuovo input, incoerente col comportamento di "5").
    #[test]
    fn hex_digit_after_result_starts_fresh() {
        let s = keys(&["base_hex", "width_byte", "hexF", "eq", "hexA"]);
        assert_eq!(s.buf, "A",
            "una cifra esadecimale dopo '=' deve iniziare un nuovo input, got {:?}", s.buf);
    }

    /// NOT bitwise end-to-end.
    #[test]
    fn not_bitwise_end_to_end() {
        let s = keys(&["base_hex", "width_byte", "fn_not", "d0", "paren_close", "eq"]);
        assert_eq!(s.buf, "FF", "¬(0) in Hex/Byte deve essere FF, got {}", s.buf);
    }

    /// Shift end-to-end: 1≪4 in Dec → 16.
    #[test]
    fn shift_end_to_end() {
        let s = keys(&["d1", "op_shift", "d4", "eq"]);
        assert_eq!(s.buf, "16", "1≪4 deve essere 16, got {}", s.buf);
    }
}
