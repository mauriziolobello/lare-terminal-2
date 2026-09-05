//! # token_store — Auth token resolution (ADR-007)
//!
//! Resolution order (first non-empty wins):
//!   1. `LARE_TOKEN` environment variable  — backward compat + dev override
//!   2. `<app_data_dir>/token` file         — persistent, works with services
//!   3. Generate a 256-bit random token, write the file, print first-launch help
//!
//! ## Why a file?
//! When the orchestrator runs as a Windows service (Fase 5), it runs in Session 0
//! with a service account.  Environment variables set in a user PowerShell session
//! are not visible there.  A file in `%LOCALAPPDATA%\dev.lare.terminal\` is owned
//! by the user and readable by any process that user authorises — including a
//! service configured to run as that user.
//!
//! The env var is kept as a higher-priority override for development workflows
//! and CI.

use std::path::Path;

// ── Public API ────────────────────────────────────────────────────────────────

/// Resolve the auth token using the standard resolution order.
///
/// # Side effects on first run
/// If neither the env var nor an existing file is found, generates a new token,
/// writes it to `<app_data_dir>/token`, and prints setup instructions to stderr.
pub fn resolve_token(app_data_dir: &Path) -> String {
    let env_val = std::env::var("LARE_TOKEN").ok();
    resolve_with_env(env_val.as_deref(), app_data_dir)
}

/// Generate a cryptographically random 256-bit token encoded as 64 lowercase
/// hex characters.
pub fn generate_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32]; // 256 bits
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ── Internal helpers (pub(crate) so tests can reach them) ────────────────────

/// Inner resolver — accepts the env-var value as a parameter so unit tests can
/// inject arbitrary values without mutating the real process environment.
pub(crate) fn resolve_with_env(env_token: Option<&str>, app_data_dir: &Path) -> String {
    // 1. env var override
    if let Some(t) = env_token {
        let t = t.trim();
        if !t.is_empty() {
            return t.to_string();
        }
    }

    // 2. token file
    let token_path = app_data_dir.join("token");
    if let Some(t) = read_token_file(&token_path) {
        return t;
    }

    // 3. first launch: generate, persist, instruct
    let token = generate_token();
    let _ = std::fs::create_dir_all(app_data_dir);
    match std::fs::write(&token_path, &token) {
        Ok(_) => print_first_launch(&token, &token_path),
        Err(e) => {
            eprintln!("[token] impossibile salvare il token in {}: {e}", token_path.display());
            eprintln!("[token] usando un token temporaneo (non persistito).");
            print_token_env_hint(&token);
        }
    }
    token
}

/// Read a token from `path`.  Returns `Some(trimmed)` if the file exists and
/// contains a non-empty string after trimming; `None` otherwise.
pub(crate) fn read_token_file(path: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    let t = raw.trim().to_string();
    if t.is_empty() { None } else { Some(t) }
}

// ── Stderr output ─────────────────────────────────────────────────────────────

fn print_first_launch(token: &str, path: &Path) {
    eprintln!();
    eprintln!("══════════════════════════════════════════════════════════════");
    eprintln!(" Lare Terminal — Primo avvio: token creato");
    eprintln!("══════════════════════════════════════════════════════════════");
    eprintln!();
    eprintln!(" Token 256-bit generato e salvato in:");
    eprintln!("   {}", path.display());
    eprintln!();
    eprintln!(" L'UI e l'orchestrator leggeranno automaticamente questo file");
    eprintln!(" ad ogni avvio. Non è necessaria nessuna configurazione.");
    eprintln!();
    eprintln!(" Per sovrascrivere (sviluppo / override):");
    eprintln!("   PowerShell:  $env:LARE_TOKEN = \"{}\"", token);
    eprintln!("   Persistente: [Environment]::SetEnvironmentVariable(");
    eprintln!("                  \"LARE_TOKEN\", \"{}\", \"User\")", token);
    eprintln!("══════════════════════════════════════════════════════════════");
    eprintln!();
}

fn print_token_env_hint(token: &str) {
    eprintln!(" Imposta manualmente la variabile per questa sessione:");
    eprintln!("   $env:LARE_TOKEN = \"{}\"", token);
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    // ── generate_token ────────────────────────────────────────────────────────

    #[test]
    fn generate_token_is_64_lowercase_hex_chars() {
        let t = generate_token();
        assert_eq!(t.len(), 64, "256 bit = 32 byte = 64 hex chars");
        assert!(
            t.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')),
            "token must be lowercase hex, got: {t}"
        );
    }

    #[test]
    fn generate_token_is_random() {
        // Two consecutive calls must produce different tokens.
        // The probability of a collision is 1/2^256 — negligible.
        assert_ne!(generate_token(), generate_token());
    }

    // ── read_token_file ───────────────────────────────────────────────────────

    #[test]
    fn read_token_file_returns_trimmed_content() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("token");
        fs::write(&path, "  abc123xyz  \n").unwrap();
        assert_eq!(read_token_file(&path), Some("abc123xyz".to_string()));
    }

    #[test]
    fn read_token_file_missing_returns_none() {
        let dir = tempdir().unwrap();
        assert_eq!(read_token_file(&dir.path().join("token")), None);
    }

    #[test]
    fn read_token_file_empty_returns_none() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("token");
        fs::write(&path, "   \n  ").unwrap();
        assert_eq!(read_token_file(&path), None);
    }

    // ── resolve_with_env ──────────────────────────────────────────────────────

    #[test]
    fn resolve_uses_env_when_present() {
        let dir = tempdir().unwrap();
        let result = resolve_with_env(Some("myenvtoken"), dir.path());
        assert_eq!(result, "myenvtoken");
        // No token file should have been created (env wins before file check).
        assert!(!dir.path().join("token").exists());
    }

    #[test]
    fn resolve_ignores_blank_env_falls_back_to_file() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("token"), "filetoken42").unwrap();
        let result = resolve_with_env(Some("   "), dir.path());
        assert_eq!(result, "filetoken42");
    }

    #[test]
    fn resolve_falls_back_to_file_when_env_absent() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("token"), "filetoken99").unwrap();
        let result = resolve_with_env(None, dir.path());
        assert_eq!(result, "filetoken99");
    }

    #[test]
    fn resolve_generates_and_writes_token_on_first_run() {
        let dir = tempdir().unwrap();
        let token_path = dir.path().join("token");

        let result = resolve_with_env(None, dir.path());

        // Token must be a 64-char hex string.
        assert_eq!(result.len(), 64);
        assert!(result.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')));

        // File must have been written with the same value.
        let saved = fs::read_to_string(&token_path).unwrap();
        assert_eq!(saved.trim(), result);
    }

    #[test]
    fn resolve_uses_existing_file_on_subsequent_runs() {
        let dir = tempdir().unwrap();
        // First run — generates the token.
        let first = resolve_with_env(None, dir.path());
        // Second run — must reuse the file without generating a new one.
        let second = resolve_with_env(None, dir.path());
        assert_eq!(first, second);
    }

    #[test]
    fn resolve_env_trimmed_whitespace_treated_as_absent() {
        let dir = tempdir().unwrap();
        // Even with \t or spaces the env var is treated as absent.
        let result = resolve_with_env(Some("\t \n"), dir.path());
        // Falls through to generate → must be a 64-char hex token.
        assert_eq!(result.len(), 64);
    }
}
