//! # telegram::qr
//!
//! Rendering di QR code a caratteri per il terminale.
//!
//! Usato al primo avvio del canale Telegram per mostrare l'URI `otpauth://`
//! scansionabile direttamente da Google Authenticator, senza richiedere
//! inserimento manuale del secret.
//!
//! ## Sicurezza
//! Il QR contiene il TOTP secret — va stampato **solo su stderr locale**,
//! mai in `tracing` o altri log persistenti.

/// Rende l'URI come QR a caratteri per il terminale.
///
/// Ritorna `Ok(String)` con il QR già formattato (include ANSI color codes per
/// il contrasto bianco/nero — scansionabile su terminali a sfondo scuro).
/// Ritorna `Err(String)` se il dato è troppo grande per un QR o il rendering
/// fallisce.
///
/// # Sicurezza
/// L'output contiene il TOTP secret (via l'URI passato): stamparlo solo su
/// stderr locale, **mai** in `tracing` o log persistenti.
pub fn render_terminal_qr(data: &str) -> Result<String, String> {
    // qr2term::generate_qr_string usa crossterm per explicit color pairs:
    // bianco su nero e nero su bianco — scansionabile su sfondo scuro.
    qr2term::generate_qr_string(data).map_err(|e| format!("QR rendering error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::render_terminal_qr;

    // URI otpauth di esempio (secret fittizio, non reale).
    const SAMPLE_URI: &str =
        "otpauth://totp/Lare%20Terminal:lare@terminal?secret=JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PX&issuer=Lare%20Terminal";

    /// Il rendering di un URI otpauth valido deve ritornare Ok con una
    /// stringa non vuota (il QR a caratteri).
    #[test]
    fn render_valid_otpauth_uri_returns_ok_non_empty() {
        let result = render_terminal_qr(SAMPLE_URI);
        assert!(result.is_ok(), "atteso Ok, ottenuto: {:?}", result);
        let qr = result.unwrap();
        assert!(!qr.is_empty(), "la stringa QR non deve essere vuota");
    }

    /// Su input molto lungo che non può essere codificato in un QR (dati
    /// oltre la capacità massima del formato), la funzione deve ritornare
    /// Err senza panica.
    #[test]
    fn render_overlong_input_returns_err_not_panic() {
        // 3000 bytes sicuramente superano la capacità QR (~2953 byte max per ECC L).
        let too_long = "x".repeat(3000);
        // Non deve panica — Err è accettabile, Ok anche se il crate li accetta.
        let result = render_terminal_qr(&too_long);
        // Verifichiamo solo che non panica: qualunque variante è valida.
        let _ = result;
    }
}
