//! Corpo di /help caricato da file esterni (Configuration/help/<lang>.md), non
//! da costanti Rust — evita che core.rs cresca ad ogni nuova lingua aggiunta.
//!
//! # Architettura (SRP)
//! Questo modulo ha la responsabilità unica di localizzare e caricare il testo di aiuto
//! per la lingua specificata. Se il file richiesto non esiste o è illeggibile, applica
//! una catena di fallback: `<lang>.md` -> `it.md` -> stringa minima di sicurezza.
//! Non lancia mai panic e non propaga errori IO al chiamante.

use std::path::{Path, PathBuf};

/// Cartella dei file di help: `<config_dir>/help`.
pub fn help_dir_path(config_dir: &Path) -> PathBuf {
    config_dir.join("help")
}

/// Corpo Markdown per la lingua richiesta.
///
/// Catena di fallback:
/// `<lang>.md` assente/illeggibile → `it.md` → stringa minima di sicurezza
/// (mai un errore che rompe l'apertura della finestra di /help).
pub fn load_help_body(help_dir: &Path, lang: &str) -> String {
    std::fs::read_to_string(help_dir.join(format!("{lang}.md")))
        .or_else(|_| std::fs::read_to_string(help_dir.join("it.md")))
        .unwrap_or_else(|_| "Aiuto non disponibile: file mancante.".to_string())
}

// ─────────────────────────────────────────────────────────────────────────────
// Test unitari (TDD: coprono fallback su it, fallback stringa di sicurezza, lingua presente)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_dir_path_appends_help_to_config_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = help_dir_path(dir.path());
        assert_eq!(path, dir.path().join("help"));
    }

    #[test]
    fn missing_file_falls_back_to_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let help_dir = dir.path().join("help");
        std::fs::create_dir_all(&help_dir).expect("create_dir_all");

        let it_content = "# Aiuto Italiano\nContenuto di prova in italiano.";
        std::fs::write(help_dir.join("it.md"), it_content).expect("write it.md");

        // Richiediamo "en" che non esiste: deve ritornare il contenuto di it.md
        let body = load_help_body(&help_dir, "en");
        assert_eq!(body, it_content);
    }

    #[test]
    fn missing_it_falls_back_to_safe_string() {
        let dir = tempfile::tempdir().expect("tempdir");
        let help_dir = dir.path().join("help");
        std::fs::create_dir_all(&help_dir).expect("create_dir_all");

        // Nessun file presente nella cartella help
        let body = load_help_body(&help_dir, "en");
        assert_eq!(body, "Aiuto non disponibile: file mancante.");

        let body_it = load_help_body(&help_dir, "it");
        assert_eq!(body_it, "Aiuto non disponibile: file mancante.");
    }

    #[test]
    fn existing_lang_returns_requested_content() {
        let dir = tempfile::tempdir().expect("tempdir");
        let help_dir = dir.path().join("help");
        std::fs::create_dir_all(&help_dir).expect("create_dir_all");

        let it_content = "# Aiuto Italiano\nContenuto in italiano.";
        let en_content = "# English Help\nContent in English.";
        std::fs::write(help_dir.join("it.md"), it_content).expect("write it.md");
        std::fs::write(help_dir.join("en.md"), en_content).expect("write en.md");

        // Richiediamo "en": deve ritornare en.md, non it.md
        let body_en = load_help_body(&help_dir, "en");
        assert_eq!(body_en, en_content);

        // Richiediamo "it": deve ritornare it.md
        let body_it = load_help_body(&help_dir, "it");
        assert_eq!(body_it, it_content);
    }
}
