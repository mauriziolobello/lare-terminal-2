//! Il "sistema portante" (Docs/superpowers/specs/2026-07-19-crypto-plugin-design.md
//! §4): ogni cifrario implementa `Cipher`. Aggiungere un cifrario futuro
//! significa un nuovo modulo + una riga in `all_ciphers()` — nessun altro
//! file da toccare (finestra principale, dialog parametri, e routing
//! UiEvent leggono tutti da questo registro).

pub mod caesar;
pub mod rsa;
mod rsa_math;
pub mod vigenere;

use std::collections::HashMap;

/// Un cifrario dichiara i propri parametri e sa cifrare/decifrare
/// puramente (nessun I/O, nessuna dipendenza dal wiring plugin —
/// testabile in isolamento).
pub trait Cipher {
    /// Id stabile (per il routing UiEvent, MAI mostrato all'utente).
    fn id(&self) -> &'static str;
    /// Nome visualizzato nella sidebar.
    fn display_name(&self) -> &'static str;
    /// Campi della dialog parametri, in ordine di visualizzazione.
    fn params(&self) -> Vec<ParamField>;
    /// Cifra `plaintext` (già normalizzato) con i valori parametro correnti.
    /// Ritorna `Err` leggibile se un parametro manca/non è valido per
    /// cifrare (es. RSA senza chiave pubblica generata).
    fn encode(&self, plaintext: &str, params: &ParamValues) -> Result<String, String>;
    /// Decifra `ciphertext` con i valori parametro correnti. Stessa
    /// semantica d'errore di `encode`.
    fn decode(&self, ciphertext: &str, params: &ParamValues) -> Result<String, String>;
    /// Azione opzionale per i cifrari con generazione di chiavi (RSA:
    /// "Genera"). Ritorna i valori dei campi da aggiornare nella dialog.
    /// Default: nessuna azione disponibile (Cesare/Vigenère non la
    /// sovrascrivono — la UI non mostra un pulsante "Genera" per loro
    /// perché `params()` non dichiara un `GeneratedPair`).
    fn generate(&self, _params: &ParamValues) -> Result<ParamValues, String> {
        Err("questo cifrario non supporta la generazione di chiavi".to_string())
    }
}

/// Un campo della dialog parametri — abbastanza generico da coprire un
/// numero (spostamento Cesare, bit RSA), una stringa (parola chiave
/// Vigenère), o un'azione con campi di sola lettura popolati da essa
/// (pulsante "Genera" di RSA) senza una variante per ogni cifrario.
#[derive(Debug, Clone, PartialEq)]
pub enum ParamField {
    Number { key: &'static str, label: &'static str, min: i64, max: i64, default: i64 },
    Text { key: &'static str, label: &'static str },
    /// Campi di sola lettura popolati da un'azione. `fields` è una lista
    /// di `(key, label)` — RSA ne dichiara 3 (n, e, d) sotto un solo
    /// pulsante "Genera".
    GeneratedPair { action_label: &'static str, fields: Vec<(&'static str, &'static str)> },
}

pub type ParamValues = HashMap<String, String>;

/// Registro statico dei cifrari disponibili, in ordine di visualizzazione
/// nella sidebar.
pub fn all_ciphers() -> Vec<Box<dyn Cipher>> {
    vec![Box::new(caesar::Caesar), Box::new(vigenere::Vigenere), Box::new(rsa::Rsa)]
}
