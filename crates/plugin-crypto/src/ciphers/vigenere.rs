//! Vigenère — sostituzione polialfabetica, shift per-lettera dato dalla
//! parola chiave (ripetuta ciclicamente). Stesso principio di Cesare ma
//! con uno spostamento variabile invece di uno fisso.

use super::{Cipher, ParamField, ParamValues};
use crate::normalize::normalize;

pub struct Vigenere;

impl Cipher for Vigenere {
    fn id(&self) -> &'static str {
        "vigenere"
    }

    fn display_name(&self) -> &'static str {
        "Vigenère"
    }

    fn params(&self) -> Vec<ParamField> {
        vec![ParamField::Text { key: "keyword", label: "Parola chiave" }]
    }

    fn encode(&self, plaintext: &str, params: &ParamValues) -> Result<String, String> {
        let keyword = normalized_keyword(params)?;
        Ok(apply(plaintext, &keyword, 1))
    }

    fn decode(&self, ciphertext: &str, params: &ParamValues) -> Result<String, String> {
        let keyword = normalized_keyword(params)?;
        Ok(apply(ciphertext, &keyword, -1))
    }
}

/// Estrae e normalizza `keyword` da `params` — errore leggibile se, dopo
/// la normalizzazione (§6: solo A-Z), non resta nessuna lettera valida
/// (keyword vuota, o composta solo da spazi/punteggiatura/numeri).
fn normalized_keyword(params: &ParamValues) -> Result<String, String> {
    let raw = params.get("keyword").map(String::as_str).unwrap_or("");
    let normalized = normalize(raw);
    if normalized.is_empty() {
        return Err("la parola chiave deve contenere almeno una lettera".to_string());
    }
    Ok(normalized)
}

fn apply(text: &str, keyword: &str, direction: i64) -> String {
    let key_chars: Vec<char> = keyword.chars().collect();
    text.chars()
        .enumerate()
        .map(|(i, c)| {
            let k = key_chars[i % key_chars.len()];
            let shift = (k as i64 - 'A' as i64) * direction;
            shift_char(c, shift)
        })
        .collect()
}

/// Stessa logica di `caesar::shift_char` — duplicata deliberatamente (2
/// implementazioni indipendenti, 3 righe ciascuna, non vale l'astrazione
/// condivisa per così poco codice — YAGNI).
fn shift_char(c: char, shift: i64) -> char {
    let base = b'A' as i64;
    let idx = (c as i64 - base + shift).rem_euclid(26);
    (base + idx) as u8 as char
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn params_with_keyword(keyword: &str) -> ParamValues {
        let mut p = HashMap::new();
        p.insert("keyword".to_string(), keyword.to_string());
        p
    }

    #[test]
    fn encode_and_decode_round_trip() {
        let v = Vigenere;
        let params = params_with_keyword("LARE");
        let encoded = v.encode("HELLOWORLD", &params).unwrap();
        assert_eq!(v.decode(&encoded, &params).unwrap(), "HELLOWORLD");
    }

    #[test]
    fn known_vector() {
        // Vigenère classico: "ATTACKATDAWN" con chiave "LEMON" → "LXFOPVEFRNHR".
        let v = Vigenere;
        let params = params_with_keyword("LEMON");
        assert_eq!(v.encode("ATTACKATDAWN", &params).unwrap(), "LXFOPVEFRNHR");
    }

    #[test]
    fn keyword_shorter_than_text_repeats_cyclically() {
        let v = Vigenere;
        let params = params_with_keyword("AB"); // shift 0, poi shift 1, ripetuto
        assert_eq!(v.encode("AAAA", &params).unwrap(), "ABAB");
    }

    #[test]
    fn empty_keyword_is_an_error() {
        let v = Vigenere;
        let params = params_with_keyword("");
        assert!(v.encode("TEST", &params).is_err());
    }

    #[test]
    fn keyword_with_only_punctuation_is_an_error_after_normalization() {
        let v = Vigenere;
        let params = params_with_keyword("123!!!");
        assert!(v.encode("TEST", &params).is_err());
    }

    #[test]
    fn keyword_gets_normalized_before_use() {
        // "la re" normalizzata → "LARE", stesso risultato di una keyword
        // già pulita — prova che la normalizzazione avviene anche qui,
        // non solo sul testo.
        let v = Vigenere;
        let a = v.encode("HELLO", &params_with_keyword("la re")).unwrap();
        let b = v.encode("HELLO", &params_with_keyword("LARE")).unwrap();
        assert_eq!(a, b);
    }
}
