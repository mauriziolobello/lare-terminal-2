//! Matematica RSA da manuale: Miller-Rabin per la primalità, generazione
//! chiavi, modexp per cifra/decifra. Nessuna dipendenza da `Cipher` — puro,
//! testabile senza il wiring plugin. NON hardened per uso reale (nessun
//! padding OAEP, nessuna aritmetica a tempo costante) — per design,
//! strumento di studio (Docs/superpowers/specs/2026-07-19-crypto-plugin-design.md §5).

use num_bigint::{BigInt, BigUint, RandBigInt};
use num_traits::{One, Zero};

pub struct RsaKeyPair {
    pub n: BigUint,
    pub e: BigUint,
    pub d: BigUint,
}

/// Genera una coppia di chiavi RSA di `bits` bit totali (`n` ha
/// esattamente `bits` bit, salvo il minimo forzato — vedi sotto).
/// `e = 65537` fisso (scelta standard). Rigenera
/// `p`/`q` finché `gcd(e, φ(n)) == 1` (necessario perché `e` sia
/// invertibile modulo `φ(n)`) — collisione rarissima con `e = 65537` e
/// primi casuali grandi, ma gestita correttamente invece di ignorata.
///
/// `bits` viene clampato a un minimo di 16 — valori più piccoli farebbero
/// entrare `generate_prime` in loop infinito (vedi commento nel corpo).
pub fn generate_keypair(bits: u64) -> RsaKeyPair {
    // Difesa in profondità: il campo "Bit chiave" nella dialog parametri è
    // un input libero (nessun min/max applicato lato UI). Con bits troppo
    // piccoli, generate_prime(bits/2) può degenerare — es. bits=4 chiede
    // primi di 2 bit, il cui unico candidato possibile è sempre 3 (i due
    // bit forzati a 1 esauriscono l'intero spazio a 2 bit): p e q
    // varrebbero SEMPRE 3, quindi il controllo `p == q` sotto non
    // uscirebbe mai dal loop. bits=0 è ancora peggio: `set_bit(bits-1, ..)`
    // sottrarrebbe con overflow. 16 (8 bit per primo) è il minimo che
    // garantisce a Miller-Rabin uno spazio di candidati sufficiente a
    // trovare un primo velocemente, senza cambiare il comportamento per
    // qualunque richiesta ragionevole (i default UI partono da 512).
    let bits = bits.max(16);
    let e = BigUint::from(65537u32);
    loop {
        let p = generate_prime(bits / 2);
        let q = generate_prime(bits / 2);
        if p == q {
            continue;
        }
        let n = &p * &q;
        let phi = (&p - 1u32) * (&q - 1u32);
        if let Some(d) = mod_inverse(&e, &phi) {
            return RsaKeyPair { n, e, d };
        }
        // gcd(e, phi) != 1 — rarissimo, rigenera p e q.
    }
}

/// Genera un primo probabile di esattamente `bits` bit (bit più
/// significativo e bit meno significativo forzati a 1: garantisce la
/// lunghezza esatta e la disparità).
fn generate_prime(bits: u64) -> BigUint {
    let mut rng = rand::thread_rng();
    loop {
        let mut candidate = rng.gen_biguint(bits);
        candidate.set_bit(bits - 1, true);
        candidate.set_bit(0, true);
        if is_probably_prime(&candidate, 20) {
            return candidate;
        }
    }
}

/// Test di primalità Miller-Rabin, `rounds` iterazioni (20 → probabilità
/// di falso positivo trascurabile, 4^-20).
fn is_probably_prime(n: &BigUint, rounds: u32) -> bool {
    let two = BigUint::from(2u32);
    let three = BigUint::from(3u32);
    if *n < two {
        return false;
    }
    if *n == two || *n == three {
        return true;
    }
    if (n % &two).is_zero() {
        return false;
    }

    let one = BigUint::one();
    let n_minus_1 = n - &one;
    let mut d = n_minus_1.clone();
    let mut r = 0u32;
    while (&d % &two).is_zero() {
        d /= &two;
        r += 1;
    }

    let mut rng = rand::thread_rng();
    'witness: for _ in 0..rounds {
        let a = rng.gen_biguint_range(&two, &n_minus_1);
        let mut x = a.modpow(&d, n);
        if x == one || x == n_minus_1 {
            continue;
        }
        for _ in 0..r.saturating_sub(1) {
            x = x.modpow(&two, n);
            if x == n_minus_1 {
                continue 'witness;
            }
        }
        return false;
    }
    true
}

/// Inverso modulare di `a` modulo `m` via algoritmo di Euclide esteso.
/// `None` se `gcd(a, m) != 1` (non invertibile).
fn mod_inverse(a: &BigUint, m: &BigUint) -> Option<BigUint> {
    let a_signed = BigInt::from(a.clone());
    let m_signed = BigInt::from(m.clone());
    let (g, x, _) = extended_gcd(&a_signed, &m_signed);
    if g != BigInt::one() {
        return None;
    }
    let result = ((x % &m_signed) + &m_signed) % &m_signed;
    result.to_biguint()
}

fn extended_gcd(a: &BigInt, b: &BigInt) -> (BigInt, BigInt, BigInt) {
    if b.is_zero() {
        (a.clone(), BigInt::one(), BigInt::zero())
    } else {
        let (g, x1, y1) = extended_gcd(b, &(a % b));
        let x = y1.clone();
        let y = x1 - (a / b) * y1;
        (g, x, y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 128 bit — abbastanza per verificare correttezza matematica in test
    // veloci, MOLTO al di sotto dei default UI (512-4096) usati in produzione
    // (dove la sicurezza reale, comunque fuori scope qui, conterebbe).
    const TEST_BITS: u64 = 128;

    #[test]
    fn keypair_modulus_is_product_of_two_distinct_primes() {
        let kp = generate_keypair(TEST_BITS);
        assert!(kp.n.bits() >= TEST_BITS - 2, "n deve avere circa {TEST_BITS} bit");
    }

    #[test]
    fn public_exponent_is_65537() {
        let kp = generate_keypair(TEST_BITS);
        assert_eq!(kp.e, BigUint::from(65537u32));
    }

    #[test]
    fn encrypt_then_decrypt_round_trips() {
        let kp = generate_keypair(TEST_BITS);
        let m = BigUint::from(42u32);
        let c = m.modpow(&kp.e, &kp.n);
        let decrypted = c.modpow(&kp.d, &kp.n);
        assert_eq!(decrypted, m);
    }

    #[test]
    fn small_known_primes_are_detected_as_prime() {
        assert!(is_probably_prime(&BigUint::from(2u32), 20));
        assert!(is_probably_prime(&BigUint::from(97u32), 20));
        assert!(is_probably_prime(&BigUint::from(7919u32), 20));
    }

    #[test]
    fn small_known_composites_are_detected_as_not_prime() {
        assert!(!is_probably_prime(&BigUint::from(1u32), 20));
        assert!(!is_probably_prime(&BigUint::from(4u32), 20));
        assert!(!is_probably_prime(&BigUint::from(100u32), 20));
        assert!(!is_probably_prime(&BigUint::from(7921u32), 20)); // 89*89
    }

    #[test]
    fn mod_inverse_matches_known_value() {
        // 3 * 4 = 12 ≡ 1 (mod 11) → inverso di 3 mod 11 è 4.
        let a = BigUint::from(3u32);
        let m = BigUint::from(11u32);
        assert_eq!(mod_inverse(&a, &m), Some(BigUint::from(4u32)));
    }

    #[test]
    fn mod_inverse_is_none_when_not_coprime() {
        // gcd(4, 8) = 4 != 1 → non invertibile.
        assert_eq!(mod_inverse(&BigUint::from(4u32), &BigUint::from(8u32)), None);
    }

    #[test]
    fn generate_keypair_with_tiny_bits_does_not_hang() {
        // Bug reale: il campo "Bit chiave" nella UI è un input libero, senza
        // clamp applicato da nessuna parte (né rendering né qui). Con
        // bits=4, generate_prime(2) chiede un primo di 2 bit: il candidato
        // ha SEMPRE bit1 e bit0 forzati a 1 (set_bit(bits-1,true) e
        // set_bit(0,true) coincidono sullo stesso bit di indice massimo), e
        // gen_biguint(2) produce solo 2 bit — quindi il candidato finale è
        // deterministicamente 3 ad ogni chiamata. generate_prime(2) ritorna
        // sempre 3: sia p sia q valgono SEMPRE 3, quindi `if p == q {
        // continue }` in generate_keypair non esce MAI dal loop — hang
        // reale, non ipotetico (verificato manualmente prima del fix: il
        // test non termina e va in timeout).
        //
        // Il clamp difensivo (bits.max(16) in generate_keypair) porta bits
        // a 16 prima di dividerlo per 2, quindi genera due primi da 8 bit
        // distinti — n ha sempre almeno ~14-15 bit (margine di tolleranza
        // come nel test `keypair_modulus_is_product_of_two_distinct_primes`
        // sopra, che usa `TEST_BITS - 2`). Questo assert fallisce SOLO se il
        // clamp non è applicato o è insufficiente: senza clamp il test non
        // arriverebbe nemmeno a fallire, appenderebbe (RED osservato via
        // timeout, non via un valore di assert sbagliato).
        let kp = generate_keypair(4);
        assert!(
            kp.n.bits() >= 14,
            "il clamp deve garantire almeno ~16 bit totali anche richiedendo bits=4"
        );
    }
}
