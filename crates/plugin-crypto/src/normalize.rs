//! Normalizzazione del testo condivisa da tutti e 3 i cifrari (2026-07-19,
//! Docs/superpowers/specs/2026-07-19-crypto-plugin-design.md §6): maiuscolo,
//! solo A-Z. Applicata SEMPRE prima di encode/decode — i moduli cifrario
//! non vedono mai testo non normalizzato.

/// Maiuscolizza e tiene solo A-Z. Spazi, punteggiatura, accenti, numeri
/// vengono rimossi (non passati invariati) — comportamento uniforme deciso
/// in brainstorming, non configurabile.
pub fn normalize(input: &str) -> String {
    input
        .chars()
        .filter_map(|c| c.to_uppercase().next())
        .filter(|c| c.is_ascii_uppercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uppercases_lowercase_letters() {
        assert_eq!(normalize("ciao"), "CIAO");
    }

    #[test]
    fn strips_spaces_punctuation_and_numbers() {
        assert_eq!(normalize("Ciao, mondo! 123"), "CIAOMONDO");
    }

    #[test]
    fn strips_accented_letters_entirely() {
        // Le accentate non sono A-Z ASCII — rimosse, non traslitterate.
        assert_eq!(normalize("perché città"), "PERCHCITT");
    }

    #[test]
    fn empty_string_stays_empty() {
        assert_eq!(normalize(""), "");
    }

    #[test]
    fn already_normalized_text_is_unchanged() {
        assert_eq!(normalize("GIAALFA"), "GIAALFA");
    }
}
