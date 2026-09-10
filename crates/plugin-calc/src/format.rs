//! Smart formatter: trasforma un f64 nel testo "intelligente" del display.
//! Regole (utente): 0→"0"; intero→senza decimali; [1e-6,1e12)→float (zeri tagliati);
//! fuori→esponenziale; NaN/Inf→"Error".
//!
//! Modalità programmatore: `format_integer_in_base` per Hex/Oct/Bin con mascheratura
//! alla larghezza bit, zero-padding e raggruppamento `_`.

use crate::engine::{to_i64_checked, BitWidth, NumBase};

/// Formatta un valore `f64` nella stringa più leggibile per il display della calcolatrice.
///
/// Analogo alle "smart display rules" di calcolatrici fisiche:
/// - valori speciali (NaN, Inf) → "Error"
/// - zero esatto → "0"
/// - fuori dall'intervallo leggibile [1e-6, 1e12) → notazione esponenziale
/// - intero esatto (niente parte frazionaria) → niente decimali
/// - tutti gli altri float → "{:.10}" con zeri finali tagliati
pub fn format_number(x: f64) -> String {
    // Valori speciali IEEE 754 non hanno rappresentazione numerica significativa
    // per l'utente della calcolatrice → mostriamo "Error".
    if x.is_nan() || x.is_infinite() {
        return "Error".to_string();
    }

    // Zero è un caso speciale: il ramo "intero" sotto farebbe `0 as i64 = 0` lo stesso,
    // ma è più chiaro trattarlo esplicitamente per chiarezza.
    if x == 0.0 {
        return "0".to_string();
    }

    // Snap-to-zero: residui minuscoli da imprecisione floating-point (es. cos(90°) in gradi
    // = 6.12e-17 invece di 0, per via della conversione gradi→radianti) vengono mostrati
    // come "0". Soglia ASSOLUTA 1e-12: ben sopra i residui tipici delle trig (~1e-16) e
    // sotto i risultati pratici. È un'euristica di DISPLAY (il valore reale non è esattamente
    // 0); il buffer dopo "=" diventa "0", quindi anche i calcoli successivi proseguono da 0.
    if x.abs() < 1e-12 {
        return "0".to_string();
    }

    let abs = x.abs();

    // Fuori dall'intervallo "leggibile" → esponenziale.
    // Valori molto piccoli (< 1e-6) o molto grandi (>= 1e12) sono difficili da leggere
    // in forma decimale estesa, quindi usiamo la notazione scientifica con la mantissa
    // ripulita dagli zeri finali (es. "4.5e-9" invece di "4.500000000e-9").
    if !(1e-6..1e12).contains(&abs) {
        // `{:e}` di Rust produce es. "1.23e15" o "4.5e-9".
        let mantissa_exp = format!("{:e}", x);
        return trim_mantissa_zeros(&mantissa_exp);
    }

    // Intero esatto: `fract()` è la parte frazionaria (0.0 se il numero è intero).
    // Poiché siamo in [−1e12, 1e12), il cast a i64 è sicuro (i64::MAX ≈ 9.2e18).
    if x.fract() == 0.0 {
        return format!("{}", x as i64);
    }

    // Float a virgola mobile nell'intervallo leggibile: stampa con 10 decimali di
    // precisione (sufficienti per f64) e poi taglia gli zeri finali superflui.
    // Es. 1.5 → "1.5000000000" → "1.5".
    let s = format!("{:.10}", x);
    trim_trailing_zeros(&s)
}

/// Toglie gli zeri finali (e l'eventuale punto) da una stringa con parte decimale.
/// Es.: "12.3400" → "12.34",  "5.0000" → "5",  "42" → "42" (invariato).
fn trim_trailing_zeros(s: &str) -> String {
    // Se non c'è il punto, non c'è niente da tagliare.
    if !s.contains('.') {
        return s.to_string();
    }
    // `trim_end_matches('0')` toglie tutti gli zeri in coda.
    // Poi `trim_end_matches('.')` toglie il punto se non restano decimali.
    let t = s.trim_end_matches('0');
    t.trim_end_matches('.').to_string()
}

/// Per la notazione esponenziale "1.230000e15" → "1.23e15":
/// taglia gli zeri nella mantissa (parte prima della 'e'), lasciando intatta l'esponente.
fn trim_mantissa_zeros(s: &str) -> String {
    match s.split_once('e') {
        Some((mantissa, exp)) => format!("{}e{}", trim_trailing_zeros(mantissa), exp),
        // Se per qualche motivo non c'è 'e', restituisci la stringa com'è.
        None => s.to_string(),
    }
}

/// Formatta un valore f64 come intero SENZA SEGNO nella base e larghezza date.
/// `None` se il valore non è un intero rappresentabile in i64 (usa `to_i64_checked`).
///
/// Mascheratura: il valore viene troncato ai bit bassi di `width` PRIMA di
/// formattare — sia per i risultati di NOT/shift/rotate, sia per un input
/// digitato che eccede la larghezza selezionata (comportamento di Windows Calculator,
/// non un bug: testalo esplicitamente).
///
/// Zero-padding: SEMPRE alla larghezza piena del numero di cifre che quella base
/// richiede per rappresentare `width` bit — mai cifre "risparmiate" per un valore
/// piccolo. Motivo: rende inequivocabile il pattern di bit di un valore negativo
/// (es. ¬(0) a Byte in Hex DEVE mostrare "FF", non "F").
///
/// | Base | Cifre per larghezza | Raggruppamento |
/// |---|---|---|
/// | Hex | width/4 | ogni 4 cifre, `_` |
/// | Bin | width | ogni 4 cifre, `_` |
/// | Oct | ⌈width/3⌉ | nessuno |
pub fn format_integer_in_base(x: f64, base: NumBase, width: BitWidth) -> Option<String> {
    let n = to_i64_checked(x)?;
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
            let digits = bits_total.div_ceil(3) as usize;   // ⌈width/3⌉
            Some(format!("{bits:0digits$o}"))
        }
    }
}

/// Inserisce `sep` ogni `n` caratteri contando DA DESTRA (cifra meno significativa),
/// così un numero di cifre non multiplo di `n` ha il gruppo corto all'inizio.
/// Con le combinazioni base/larghezza di `format_integer_in_base` questo non succede
/// mai (Hex e Bin hanno sempre un numero di cifre multiplo di 4 o < 4), ma la funzione
/// resta corretta in generale.
fn group_every(s: &str, n: usize, sep: char) -> String {
    let rev: String = s.chars().rev().collect();
    let mut out = String::new();
    for (i, c) in rev.chars().enumerate() {
        if i > 0 && i % n == 0 { out.push(sep); }
        out.push(c);
    }
    out.chars().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn zero() { assert_eq!(format_number(0.0), "0"); }
    #[test] fn integer_no_decimals() { assert_eq!(format_number(4.0), "4"); assert_eq!(format_number(-12.0), "-12"); }
    #[test] fn plain_float_trimmed() { assert_eq!(format_number(1.5), "1.5"); assert_eq!(format_number(0.75), "0.75"); assert_eq!(format_number(1234.5), "1234.5"); }
    #[test] fn small_uses_exp() { assert!(format_number(4.5e-9).contains('e')); }
    #[test] fn large_uses_exp() { assert!(format_number(1.23e15).contains('e')); }
    #[test] fn boundary_below_1e12_is_plain() { assert_eq!(format_number(999_999_999_999.0), "999999999999"); }
    #[test] fn nan_inf_error() { assert_eq!(format_number(f64::NAN), "Error"); assert_eq!(format_number(f64::INFINITY), "Error"); }

    #[test]
    fn snap_to_zero_tiny_residuals() {
        // Residui minuscoli da imprecisione FP (es. cos(90°) deg = 6.12e-17) → "0".
        assert_eq!(format_number(6.123_233_995_736_766e-17), "0"); // cos(90°)
        assert_eq!(format_number(1.224_646_799_147_353e-16), "0"); // sin(180°)
        assert_eq!(format_number(-3.5e-13), "0");                  // sotto soglia, negativo
        // Valori legittimi SOPRA la soglia (1e-12) NON vengono azzerati.
        assert!(format_number(4.5e-9).contains('e'));
        assert_ne!(format_number(1e-11), "0");
    }

    // ── Nuovi test TDD per I-1 (coverage gap § 3 spec) ────────────────────────

    #[test]
    fn boundary_at_1e12_is_exp() {
        // 1e12 è la soglia ALTA (esclusa dall'intervallo plain [1e-6, 1e12)).
        // Quindi esattamente 1e12 → notazione esponenziale.
        // Questo pin test garantisce che la soglia non venga spostata per sbaglio.
        assert_eq!(format_number(1e12), "1e12");
    }

    #[test]
    fn boundary_at_1e_minus_6_is_plain() {
        // 1e-6 è la soglia BASSA INCLUSA dell'intervallo plain [1e-6, 1e12).
        // Quindi 1e-6 → formato plain (no 'e'). Il valore esatto dipende da {:.10} + trim:
        // format!("{:.10}", 1e-6) = "0.0000010000" → trim → "0.000001".
        //
        // IMPORTANTE: questo è un boundary test esatto — se l'implementazione cambia
        // il formato, questo test fallirà e attira l'attenzione. Non approssimare.
        assert_eq!(format_number(1e-6), "0.000001");
    }

    #[test]
    fn just_below_1e_minus_6_is_exp() {
        // 9.9e-7 < 1e-6 → fuori dall'intervallo plain → notazione esponenziale.
        // Non verifichiamo la stringa esatta (la mantissa f64 può variare di 1 ULP);
        // basta che il risultato contenga 'e' (siamo nel ramo esponenziale).
        assert!(format_number(9.9e-7).contains('e'),
            "9.9e-7 dovrebbe usare notazione esponenziale, ma got: {:?}", format_number(9.9e-7));
    }

    // ══ Modalità programmatore (Parte B) — format_integer_in_base ═════════════
    use crate::engine::{BitWidth, NumBase};

    #[test]
    fn format_hex_byte_255_is_ff() {
        assert_eq!(
            format_integer_in_base(255.0, NumBase::Hex, BitWidth::Byte),
            Some("FF".to_string())
        );
    }

    #[test]
    fn format_hex_qword_255_is_sixteen_digits_grouped() {
        assert_eq!(
            format_integer_in_base(255.0, NumBase::Hex, BitWidth::Qword),
            Some("0000_0000_0000_00FF".to_string())
        );
    }

    #[test]
    fn format_oct_byte_8_is_010() {
        assert_eq!(
            format_integer_in_base(8.0, NumBase::Oct, BitWidth::Byte),
            Some("010".to_string())
        );
    }

    #[test]
    fn format_bin_byte_5_is_grouped() {
        // 5 = 0b101, 8 bit → "00000101", raggruppamento ogni 4 da destra → "0000_0101".
        assert_eq!(
            format_integer_in_base(5.0, NumBase::Bin, BitWidth::Byte),
            Some("0000_0101".to_string())
        );
    }

    #[test]
    fn format_not_zero_hex_byte_ff() {
        // ¬(0) = -1, reinterpretato come unsigned 8 bit → 0xFF → "FF".
        assert_eq!(
            format_integer_in_base(-1.0, NumBase::Hex, BitWidth::Byte),
            Some("FF".to_string())
        );
    }

    #[test]
    fn format_not_zero_hex_qword_all_f() {
        assert_eq!(
            format_integer_in_base(-1.0, NumBase::Hex, BitWidth::Qword),
            Some("FFFF_FFFF_FFFF_FFFF".to_string())
        );
    }

    #[test]
    fn format_overflow_truncates_silently() {
        // 256.0 (binario a 9 cifre) in Bin/Byte → 256 = 0x100 = 0b1_0000_0000.
        // Mascherato a 8 bit → 0 → "0000_0000".
        // Comportamento intenzionale: coerente con Windows Calculator.
        assert_eq!(
            format_integer_in_base(256.0, NumBase::Bin, BitWidth::Byte),
            Some("0000_0000".to_string())
        );
    }

    #[test]
    fn format_non_integer_returns_none() {
        assert_eq!(format_integer_in_base(0.5, NumBase::Hex, BitWidth::Byte), None);
    }
}
