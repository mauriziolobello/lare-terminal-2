//! # token_store — token di autenticazione del WS (ADR-007), 2.0
//!
//! Il token vive SOLO nel file `<config_dir>/token`. Nessuna variabile
//! d'ambiente (D6): nella v1 `LARE_TOKEN` era un override che, impostato in
//! un solo processo dei due, produceva "connection refused" senza spiegazione.
//! Al primo avvio il token viene generato (256 bit) e scritto; ui.exe e
//! lare-shell.exe leggono lo stesso file dalla stessa cartella.

use std::path::Path;

/// Legge `<config_dir>/token`; se assente o vuoto lo genera e lo scrive.
pub fn resolve_token(config_dir: &Path) -> String {
    let token_path = config_dir.join("token");
    if let Some(t) = read_token_file(&token_path) {
        return t;
    }
    let token = generate_token();
    let _ = std::fs::create_dir_all(config_dir);
    match std::fs::write(&token_path, &token) {
        Ok(_) => eprintln!(
            "[token] primo avvio: token creato in {}",
            token_path.display()
        ),
        Err(e) => eprintln!(
            "[token] impossibile salvare il token in {}: {e} — token temporaneo non persistito",
            token_path.display()
        ),
    }
    token
}

/// 256 bit casuali, 64 caratteri esadecimali minuscoli.
pub fn generate_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn read_token_file(path: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    let t = raw.trim().to_string();
    if t.is_empty() {
        None
    } else {
        Some(t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn generate_token_is_64_lowercase_hex_chars() {
        let t = generate_token();
        assert_eq!(t.len(), 64);
        assert!(t
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }
    #[test]
    fn reads_existing_token_file_trimmed() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("token"), "  abc123  \n").unwrap();
        assert_eq!(resolve_token(dir.path()), "abc123");
    }
    #[test]
    fn first_launch_generates_and_persists_token() {
        let dir = tempdir().unwrap();
        let t = resolve_token(dir.path());
        assert_eq!(t.len(), 64);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("token")).unwrap(),
            t
        );
        // seconda chiamata: legge lo stesso token, non ne genera un altro
        assert_eq!(resolve_token(dir.path()), t);
    }
    #[test]
    fn empty_token_file_is_regenerated() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("token"), "   ").unwrap();
        let t = resolve_token(dir.path());
        assert_eq!(t.len(), 64);
    }
}
