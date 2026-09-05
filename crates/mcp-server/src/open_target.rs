//! # open_target — classificazione e apertura di target nativi (ADR-012)
//!
//! ## Responsabilità (SRP)
//! Questo modulo contiene:
//! 1. [`TargetKind`] — enum che classifica il target.
//! 2. [`classify_target`] — funzione pura che determina il tipo del target.
//! 3. [`open_target`] — funzione che apre il target via crate `opener`;
//!    se il target è `NotFound` ritorna errore senza lanciare nulla.
//!
//! ## Classificazione URL
//! Solo i prefissi `http://` e `https://` vengono riconosciuti come URL (ADR-012).
//! Altri schemi (`file:`, `mailto:`, `ftp:`, ecc.) vengono trattati come path
//! e finiscono in `NotFound` se non esistono sul filesystem.
//! Motivazione: un target con schema sconosciuto potrebbe essere un file/cartella
//! valido su alcuni filesystem; invece di aprirlo in modo imprevedibile, lasciamo
//! che l'utente usi il percorso canonico.
//!
//! ## Sicurezza
//! `opener` non usa una shell string → nessun rischio di injection da quoting.
//! I target sono input dell'utente sulla propria macchina (come un doppio-click).

use std::path::Path;

/// Tipo di target da aprire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    /// URL con schema `http://` o `https://`.
    Url,
    /// Path di cartella esistente.
    Folder,
    /// Path di file esistente.
    File,
    /// Target non riconosciuto o inesistente.
    NotFound,
}

/// Risultato di `open_target`.
///
/// Serializzabile via serde per il ritorno JSON dal tool MCP.
/// Non richiede `JsonSchema` perché è un tipo di ritorno (non un parametro `Parameters<T>`).
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct OpenResult {
    pub ok: bool,
    pub message: String,
}

/// Classifica `target` in [`TargetKind`].
///
/// Ordine di controllo (importante):
/// 1. Prefisso `http://`/`https://` → [`TargetKind::Url`] (prima del filesystem
///    per evitare di provare a fare `stat` su una stringa URL).
/// 2. Path di cartella esistente → [`TargetKind::Folder`].
/// 3. Path di file esistente → [`TargetKind::File`].
/// 4. Altrimenti → [`TargetKind::NotFound`].
///
/// # Nota purezza
/// Questa funzione tocca il filesystem (per i controlli di esistenza), ma è
/// deterministica dato lo stato del filesystem e non ha effetti collaterali GUI.
pub fn classify_target(target: &str) -> TargetKind {
    // 1. URL: controllo prefisso PRIMA del filesystem.
    let lower = target.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return TargetKind::Url;
    }

    // 2 & 3. Filesystem.
    let p = Path::new(target);
    if p.is_dir() {
        return TargetKind::Folder;
    }
    if p.is_file() {
        return TargetKind::File;
    }

    TargetKind::NotFound
}

/// Apre `target` con l'applicazione di default dell'OS.
///
/// - `NotFound` → ritorna `OpenResult { ok: false, message }` senza side-effect.
/// - Altrimenti → chiama `opener::open(target)` e ritorna esito.
///
/// # Side-effect GUI
/// L'apertura reale è un side-effect GUI (Explorer, browser, ecc.) e
/// non è unit-testata. I test coprono solo il path `NotFound` e la
/// classificazione.
pub fn open_target(target: &str) -> OpenResult {
    match classify_target(target) {
        TargetKind::NotFound => OpenResult {
            ok: false,
            message: format!("target non trovato o non riconosciuto: {target}"),
        },
        kind => {
            let kind_str = match kind {
                TargetKind::Url => "URL",
                TargetKind::Folder => "cartella",
                TargetKind::File => "file",
                TargetKind::NotFound => unreachable!(),
            };
            match opener::open(target) {
                Ok(()) => OpenResult {
                    ok: true,
                    message: format!("aperto {kind_str}: {target}"),
                },
                Err(e) => OpenResult {
                    ok: false,
                    message: format!("errore apertura {kind_str} \"{target}\": {e}"),
                },
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests — TDD: RED (scritti prima dell'impl), poi GREEN.
//
// Test scritti PRIMA dell'implementazione di classify_target e open_target.
// Ciclo RED → GREEN documentato nel CHANGELOG.
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── classify_target ───────────────────────────────────────────────────────

    #[test]
    fn classify_http_url() {
        assert_eq!(classify_target("http://example.com"), TargetKind::Url);
    }

    #[test]
    fn classify_https_url() {
        assert_eq!(
            classify_target("https://example.com/path?q=1"),
            TargetKind::Url
        );
    }

    #[test]
    fn classify_url_case_insensitive() {
        // Anche HTTPS:// maiuscolo deve essere riconosciuto come URL.
        assert_eq!(classify_target("HTTPS://example.com"), TargetKind::Url);
        assert_eq!(classify_target("HTTP://example.com"), TargetKind::Url);
    }

    #[test]
    fn classify_existing_folder() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        assert_eq!(
            classify_target(dir.path().to_str().unwrap()),
            TargetKind::Folder
        );
    }

    #[test]
    fn classify_existing_file() {
        // Creiamo un file temporaneo e verifichiamo che sia classificato come File.
        let dir = tempfile::TempDir::new().expect("tempdir");
        let file_path = dir.path().join("test_file.txt");
        std::fs::write(&file_path, "hello").expect("write");
        assert_eq!(
            classify_target(file_path.to_str().unwrap()),
            TargetKind::File
        );
    }

    #[test]
    fn classify_nonexistent_path_is_not_found() {
        assert_eq!(
            classify_target("/questo/percorso/non/esiste/mai/12345xyz"),
            TargetKind::NotFound
        );
    }

    #[test]
    fn classify_invented_path_is_not_found() {
        // Path inventato su Windows e Unix — non deve crashare.
        assert_eq!(
            classify_target("C:\\NonEsiste\\Cartella\\Inesistente"),
            TargetKind::NotFound
        );
    }

    #[test]
    fn classify_other_scheme_is_not_found() {
        // Schema non http(s) — trattato come path/NotFound (ADR-012).
        assert_eq!(
            classify_target("ftp://ftp.example.com"),
            TargetKind::NotFound
        );
        assert_eq!(
            classify_target("mailto:user@example.com"),
            TargetKind::NotFound
        );
        assert_eq!(classify_target("file:///tmp/foo"), TargetKind::NotFound);
    }

    // ── open_target su target inesistente — nessun side-effect ───────────────

    #[test]
    fn open_target_not_found_returns_ok_false_no_side_effect() {
        let result = open_target("/percorso/inesistente/non-aprire-nulla-xyz");
        assert!(!result.ok, "expected ok=false for non-existent target");
        assert!(
            result.message.contains("non trovato"),
            "expected 'non trovato' in message, got: {:?}",
            result.message
        );
    }

    #[test]
    fn open_target_not_found_message_contains_target() {
        let target = "/mio/percorso/inesistente/abc123";
        let result = open_target(target);
        assert!(!result.ok);
        assert!(
            result.message.contains(target),
            "message should contain the target path: {:?}",
            result.message
        );
    }
}
