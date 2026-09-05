//! Cesare — sostituzione monoalfabetica, spostamento fisso.

use super::{Cipher, ParamField, ParamValues};

pub struct Caesar;

impl Cipher for Caesar {
    fn id(&self) -> &'static str {
        "caesar"
    }

    fn display_name(&self) -> &'static str {
        "Cesare"
    }

    fn params(&self) -> Vec<ParamField> {
        vec![ParamField::Number { key: "shift", label: "Spostamento", min: 1, max: 25, default: 3 }]
    }

    fn encode(&self, plaintext: &str, params: &ParamValues) -> Result<String, String> {
        Ok(shift_text(plaintext, shift_param(params)))
    }

    fn decode(&self, ciphertext: &str, params: &ParamValues) -> Result<String, String> {
        Ok(shift_text(ciphertext, -shift_param(params)))
    }
}

fn shift_param(params: &ParamValues) -> i64 {
    params
        .get("shift")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(3)
}

fn shift_text(text: &str, shift: i64) -> String {
    text.chars().map(|c| shift_char(c, shift)).collect()
}

/// Sposta una lettera A-Z di `shift` posizioni (modulo 26, gestisce anche
/// shift negativi grazie a `rem_euclid`). `c` deve essere già A-Z
/// (garantito dal chiamante via `normalize` — vedi state.rs, Task 6).
fn shift_char(c: char, shift: i64) -> char {
    let base = b'A' as i64;
    let idx = (c as i64 - base + shift).rem_euclid(26);
    (base + idx) as u8 as char
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn params_with_shift(shift: i64) -> ParamValues {
        let mut p = HashMap::new();
        p.insert("shift".to_string(), shift.to_string());
        p
    }

    #[test]
    fn encode_shifts_forward() {
        let c = Caesar;
        assert_eq!(c.encode("ABC", &params_with_shift(3)).unwrap(), "DEF");
    }

    #[test]
    fn decode_shifts_backward() {
        let c = Caesar;
        assert_eq!(c.decode("DEF", &params_with_shift(3)).unwrap(), "ABC");
    }

    #[test]
    fn round_trip_with_wraparound() {
        // 'Z' + shift 3 deve avvolgersi a 'C', non uscire dall'alfabeto.
        let c = Caesar;
        let encoded = c.encode("XYZ", &params_with_shift(3)).unwrap();
        assert_eq!(encoded, "ABC");
        assert_eq!(c.decode(&encoded, &params_with_shift(3)).unwrap(), "XYZ");
    }

    #[test]
    fn shift_zero_is_identity() {
        let c = Caesar;
        assert_eq!(c.encode("HELLO", &params_with_shift(0)).unwrap(), "HELLO");
    }

    #[test]
    fn shift_25_is_almost_full_wraparound() {
        let c = Caesar;
        assert_eq!(c.encode("A", &params_with_shift(25)).unwrap(), "Z");
    }

    #[test]
    fn empty_text_stays_empty() {
        let c = Caesar;
        assert_eq!(c.encode("", &params_with_shift(3)).unwrap(), "");
    }

    #[test]
    fn missing_shift_param_defaults_to_3() {
        let c = Caesar;
        let empty_params: ParamValues = HashMap::new();
        assert_eq!(c.encode("A", &empty_params).unwrap(), "D");
    }
}
