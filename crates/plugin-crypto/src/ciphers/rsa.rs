//! RSA — chiave pubblica/privata. A differenza di Cesare/Vigenère non
//! "traduce" testo in modo simmetrico: cifra con la chiave pubblica (n, e),
//! decifra con la privata (n, d). Il testo cifrato è mostrato come blocchi
//! esadecimali separati da spazio (i byte cifrati non sono testo
//! stampabile — l'esadecimale è leggibile e senza ambiguità di encoding).

use super::rsa_math::{generate_keypair, RsaKeyPair};
use super::{Cipher, ParamField, ParamValues};
use crate::normalize::normalize;
use num_bigint::BigUint;

pub struct Rsa;

impl Cipher for Rsa {
    fn id(&self) -> &'static str {
        "rsa"
    }

    fn display_name(&self) -> &'static str {
        "RSA"
    }

    fn params(&self) -> Vec<ParamField> {
        vec![
            ParamField::Number { key: "bits", label: "Bit chiave", min: 512, max: 4096, default: 2048 },
            ParamField::GeneratedPair {
                action_label: "Genera",
                fields: vec![("pub_n", "n (pubblica/privata)"), ("pub_e", "e (pubblica)"), ("priv_d", "d (privata)")],
            },
        ]
    }

    fn encode(&self, plaintext: &str, params: &ParamValues) -> Result<String, String> {
        let n = get_biguint_param(params, "pub_n")
            .ok_or_else(|| "genera prima una coppia di chiavi".to_string())?;
        let e = get_biguint_param(params, "pub_e")
            .ok_or_else(|| "genera prima una coppia di chiavi".to_string())?;
        let normalized = normalize(plaintext);
        Ok(encrypt_blocks(&normalized, &n, &e))
    }

    fn decode(&self, ciphertext: &str, params: &ParamValues) -> Result<String, String> {
        let n = get_biguint_param(params, "pub_n")
            .ok_or_else(|| "genera prima una coppia di chiavi".to_string())?;
        let d = get_biguint_param(params, "priv_d")
            .ok_or_else(|| "genera prima una coppia di chiavi".to_string())?;
        decrypt_blocks(ciphertext, &n, &d)
    }

    fn generate(&self, params: &ParamValues) -> Result<ParamValues, String> {
        let bits = params
            .get("bits")
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(2048);
        let RsaKeyPair { n, e, d } = generate_keypair(bits);
        let mut result = ParamValues::new();
        result.insert("pub_n".to_string(), n.to_str_radix(16));
        result.insert("pub_e".to_string(), e.to_str_radix(16));
        result.insert("priv_d".to_string(), d.to_str_radix(16));
        Ok(result)
    }
}

fn get_biguint_param(params: &ParamValues, key: &str) -> Option<BigUint> {
    let raw = params.get(key)?;
    if raw.is_empty() {
        return None;
    }
    BigUint::parse_bytes(raw.as_bytes(), 16)
}

/// Spezza `text` (già normalizzato — solo A-Z ASCII, MAI byte 0x00, quindi
/// nessun blocco perde uno zero iniziale nella conversione byte→BigUint→byte)
/// in blocchi più piccoli del modulo `n`, cifra ognuno con `e`, ritorna gli
/// esadecimali separati da spazio.
fn encrypt_blocks(text: &str, n: &BigUint, e: &BigUint) -> String {
    let bytes = text.as_bytes();
    let block_bytes = (((n.bits().saturating_sub(1)) / 8).max(1)) as usize;
    bytes
        .chunks(block_bytes)
        .map(|chunk| {
            let m = BigUint::from_bytes_be(chunk);
            let c = m.modpow(e, n);
            c.to_str_radix(16)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn decrypt_blocks(ciphertext_hex: &str, n: &BigUint, d: &BigUint) -> Result<String, String> {
    let mut bytes = Vec::new();
    for block in ciphertext_hex.split_whitespace() {
        let c = BigUint::parse_bytes(block.as_bytes(), 16)
            .ok_or_else(|| format!("blocco esadecimale non valido: {block}"))?;
        let m = c.modpow(d, n);
        bytes.extend(m.to_bytes_be());
    }
    String::from_utf8(bytes).map_err(|_| "byte decifrati non validi come testo".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_BITS: u64 = 128;

    fn generated_params() -> ParamValues {
        let r = Rsa;
        let mut params = ParamValues::new();
        params.insert("bits".to_string(), TEST_BITS.to_string());
        let generated = r.generate(&params).unwrap();
        params.extend(generated);
        params
    }

    #[test]
    fn generate_populates_all_three_fields() {
        let params = generated_params();
        assert!(params.contains_key("pub_n"));
        assert!(params.contains_key("pub_e"));
        assert!(params.contains_key("priv_d"));
    }

    #[test]
    fn encode_then_decode_round_trips() {
        let r = Rsa;
        let params = generated_params();
        let encoded = r.encode("HELLO WORLD", &params).unwrap();
        assert_eq!(r.decode(&encoded, &params).unwrap(), "HELLOWORLD"); // normalizzato: niente spazio
    }

    #[test]
    fn encode_without_generated_key_is_an_error() {
        let r = Rsa;
        let params = ParamValues::new(); // mai chiamato "Genera"
        assert!(r.encode("TEST", &params).is_err());
    }

    #[test]
    fn decode_without_private_key_is_an_error() {
        let r = Rsa;
        let mut params = ParamValues::new();
        params.insert("pub_n".to_string(), "ff".to_string()); // solo pubblica, niente priv_d
        assert!(r.decode("ab cd", &params).is_err());
    }

    #[test]
    fn ciphertext_is_hex_blocks_separated_by_spaces() {
        let r = Rsa;
        let params = generated_params();
        let encoded = r.encode("HI", &params).unwrap();
        assert!(encoded.chars().all(|c| c.is_ascii_hexdigit() || c == ' '));
    }
}
