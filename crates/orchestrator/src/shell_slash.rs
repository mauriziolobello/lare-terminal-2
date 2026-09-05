//! # shell_slash — che cosa fare di una riga `/…` arrivata da una shell
//!
//! La host manda all'orchestratore OGNI riga che inizia con `/` (spec §3):
//! non ha un elenco proprio. Questo modulo è quell'elenco, in forma di
//! classificazione pura (nessun I/O, nessun `await`): `ws.rs` la consulta
//! PRIMA del routing v1 e agisce di conseguenza. Regole (tabella §3):
//! - `/ai "testo"` e `/ "testo"` → turno AI con `testo` (virgolette
//!   obbligatorie, D8; senza → errore di sintassi nel terminale);
//! - `/config` `/library` `/aichat` e i canali esterni (`/nmap`, `/markets`,
//!   `/pyping`) → `OpenUiLocal{name}` verso `ui` (singleton, D15);
//! - `/reset` → messaggio "non applicabile"; `/ping` → built-in;
//! - slash del backend v1 (`core::KNOWN_BACKEND_SLASHES`) → `handle_command`;
//! - tutto il resto → **scartato** (`Done` muto + log `info`).
//!
//! "Conosciuto" non è deciso qui: arriva dal chiamante come predicato
//! (`is_known_backend`), così plugin e backend restano la loro unica lista.

use std::path::Path;

/// Esito della classificazione (vedi doc-comment del modulo).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellInput {
    /// Turno AI in linguaggio naturale col testo fra virgolette.
    Nl(String),
    /// `/ai`/`/ ` senza virgolette (o vuoto): errore da stampare nel terminale.
    SyntaxError(String),
    /// Finestra locale di `ui`: `"config"`, `"library"`, `"aichat"` o id canale.
    OpenUiLocal(String),
    Reset,
    Ping,
    /// Comando slash del backend v1 (`/open`, `/web`, `/show`, `/help`).
    Backend,
    /// Slash ignoto: il nome (senza `/`) per il log.
    Discard(String),
}

pub const AI_SYNTAX_ERROR: &str = "sintassi: /ai \"testo\" (virgolette obbligatorie)";
pub const RESET_MESSAGE: &str = "non applicabile: la sessione è la tua";
/// Finestre locali di `ui` raggiungibili per nome (D15).
pub const UI_LOCAL_SLASHES: &[&str] = &["config", "library", "aichat"];
/// Trigger dei canali esterni esposti all'utente (gli altri in
/// `EXTERNAL_TOOL_CHANNELS` — `library-expand`, `config-market-data-test` —
/// sono interni a `ui`). Stessa terna della tabella JS `external-channels.js`.
const USER_FACING_CHANNEL_TRIGGERS: &[&str] = &["/nmap", "/pyping", "/markets"];

/// Trigger → id canale dei canali esterni esposti all'utente, derivati dal
/// registro reale (`EXTERNAL_TOOL_CHANNELS`) e filtrati con
/// `USER_FACING_CHANNEL_TRIGGERS`. Senza la `/` iniziale.
pub fn shell_channel_table() -> Vec<(String, String)> {
    crate::external_channel::EXTERNAL_TOOL_CHANNELS
        .iter()
        .filter(|c| USER_FACING_CHANNEL_TRIGGERS.contains(&c.slash_trigger))
        .map(|c| {
            (
                c.slash_trigger.trim_start_matches('/').to_string(),
                c.id.to_string(),
            )
        })
        .collect()
}

/// `true` se la riga è un comando di `core::WINDOW_SLASHES` (`/help`, `/show`).
pub fn is_window_slash(input: &str) -> bool {
    let Some(after) = input.trim().strip_prefix('/') else {
        return false;
    };
    let cmd = after
        .split_once(char::is_whitespace)
        .map(|(c, _)| c)
        .unwrap_or(after)
        .to_ascii_lowercase();
    crate::core::WINDOW_SLASHES.contains(&cmd.as_str())
}

/// Testo fra virgolette (`"x"` → `x`), o `None` se non è racchiuso da UNA
/// coppia di virgolette esterne o è vuoto.
fn quoted_text(rest: &str) -> Option<String> {
    let r = rest.trim();
    let inner = r.strip_prefix('"')?.strip_suffix('"')?;
    if r.len() < 2 || inner.trim().is_empty() {
        return None;
    }
    Some(inner.to_string())
}

fn ai_or_error(rest: &str) -> ShellInput {
    match quoted_text(rest) {
        Some(text) => ShellInput::Nl(text),
        None => ShellInput::SyntaxError(AI_SYNTAX_ERROR.to_string()),
    }
}

/// Classifica una riga della shell (vedi tabella nel doc-comment del modulo).
/// `is_known_backend(cmd)` dice se `cmd` (minuscolo, senza `/`) è un comando
/// del backend v1: il chiamante lo costruisce da `core::KNOWN_BACKEND_SLASHES`.
pub fn classify_shell_input(input: &str, is_known_backend: &dyn Fn(&str) -> bool) -> ShellInput {
    let trimmed = input.trim();
    let Some(after) = trimmed.strip_prefix('/') else {
        return ShellInput::Discard(trimmed.to_string());
    };
    // `/ "testo"`: slash, spazio, testo.
    if after.starts_with(char::is_whitespace) {
        return ai_or_error(after);
    }
    let (cmd, rest) = match after.split_once(char::is_whitespace) {
        Some((c, r)) => (c.to_ascii_lowercase(), r.trim()),
        None => (after.to_ascii_lowercase(), ""),
    };
    if cmd == "ai" {
        return ai_or_error(rest);
    }
    if cmd == "ping" {
        return ShellInput::Ping;
    }
    if cmd == "reset" {
        return ShellInput::Reset;
    }
    if UI_LOCAL_SLASHES.contains(&cmd.as_str()) {
        return ShellInput::OpenUiLocal(cmd);
    }
    if let Some((_, id)) = shell_channel_table()
        .into_iter()
        .find(|(trigger, _)| *trigger == cmd)
    {
        return ShellInput::OpenUiLocal(id);
    }
    if is_known_backend(&cmd) {
        return ShellInput::Backend;
    }
    ShellInput::Discard(cmd)
}

/// `web_search_enabled` da `<config_dir>/config.json` (il file di `ui`,
/// scritto da `/config`): la shell non ha una propria casella per la ricerca
/// web, quindi vale la scelta dell'utente in `/config`. Assente o
/// illeggibile → `false` (comportamento conservativo, come un client che non
/// manda il campo).
pub fn read_web_search_enabled(config_dir: &Path) -> bool {
    std::fs::read_to_string(config_dir.join("config.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("web_search_enabled").and_then(|b| b.as_bool()))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known(c: &str) -> bool {
        crate::core::KNOWN_BACKEND_SLASHES.contains(&c)
    }

    #[test]
    fn ai_and_bare_slash_with_quotes_are_the_same_nl_turn() {
        assert_eq!(
            classify_shell_input("/ai \"elenca i file\"", &known),
            ShellInput::Nl("elenca i file".into())
        );
        assert_eq!(
            classify_shell_input("/ \"elenca i file\"", &known),
            ShellInput::Nl("elenca i file".into())
        );
        assert_eq!(
            classify_shell_input("  /AI   \"x\"  ", &known),
            ShellInput::Nl("x".into())
        );
        // Le virgolette interne restano parte del testo.
        assert_eq!(
            classify_shell_input("/ai \"dimmi \"ciao\"\"", &known),
            ShellInput::Nl("dimmi \"ciao\"".into())
        );
    }

    #[test]
    fn ai_without_quotes_or_empty_is_a_syntax_error() {
        for input in [
            "/ai elenca",
            "/ elenca",
            "/ai",
            "/ai \"\"",
            "/ai \"x\" y",
            "/ai \"x",
        ] {
            assert_eq!(
                classify_shell_input(input, &known),
                ShellInput::SyntaxError(AI_SYNTAX_ERROR.into()),
                "{input}"
            );
        }
    }

    #[test]
    fn ui_local_reset_ping_and_backend_are_recognised() {
        assert_eq!(
            classify_shell_input("/config", &known),
            ShellInput::OpenUiLocal("config".into())
        );
        assert_eq!(
            classify_shell_input("/Library extra", &known),
            ShellInput::OpenUiLocal("library".into())
        );
        assert_eq!(
            classify_shell_input("/aichat", &known),
            ShellInput::OpenUiLocal("aichat".into())
        );
        assert_eq!(
            classify_shell_input("/nmap", &known),
            ShellInput::OpenUiLocal("nmap".into())
        );
        assert_eq!(
            classify_shell_input("/markets", &known),
            ShellInput::OpenUiLocal("financial-markets".into())
        );
        assert_eq!(
            classify_shell_input("/pyping", &known),
            ShellInput::OpenUiLocal("python-ping".into())
        );
        assert_eq!(classify_shell_input("/reset", &known), ShellInput::Reset);
        assert_eq!(classify_shell_input("/ping", &known), ShellInput::Ping);
        for input in ["/open C:\\x", "/web gatti", "/show # t", "/help"] {
            assert_eq!(
                classify_shell_input(input, &known),
                ShellInput::Backend,
                "{input}"
            );
        }
    }

    #[test]
    fn unknown_slashes_are_discarded_including_find_and_nowin() {
        assert_eq!(
            classify_shell_input("/nonesiste a b", &known),
            ShellInput::Discard("nonesiste".into())
        );
        assert_eq!(
            classify_shell_input("/find x", &known),
            ShellInput::Discard("find".into())
        );
        assert_eq!(
            classify_shell_input("/nowin x", &known),
            ShellInput::Discard("nowin".into())
        );
        assert_eq!(
            classify_shell_input("/", &known),
            ShellInput::Discard(String::new())
        );
        assert_eq!(
            classify_shell_input("dir", &known),
            ShellInput::Discard("dir".into())
        );
    }

    /// I comandi il cui esito È una finestra non devono aprire anche quella di output.
    #[test]
    fn window_slashes_are_help_and_show_only() {
        assert!(is_window_slash("/help"));
        assert!(is_window_slash("  /Show # titolo"));
        assert!(!is_window_slash("/open x"));
        assert!(!is_window_slash("/ai \"x\""));
        assert!(!is_window_slash("/ping"));
    }

    #[test]
    fn channel_table_exposes_the_three_user_facing_channels() {
        let mut t = shell_channel_table();
        t.sort();
        assert_eq!(
            t,
            vec![
                ("markets".to_string(), "financial-markets".to_string()),
                ("nmap".to_string(), "nmap".to_string()),
                ("pyping".to_string(), "python-ping".to_string()),
            ]
        );
    }

    #[test]
    fn web_search_enabled_is_read_from_config_json_default_false() {
        let dir = std::env::temp_dir().join(format!("lare-shell-slash-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!read_web_search_enabled(&dir), "file assente → false");
        std::fs::write(
            dir.join("config.json"),
            r#"{"window_alpha":0.9,"web_search_enabled":true}"#,
        )
        .unwrap();
        assert!(read_web_search_enabled(&dir));
        std::fs::write(dir.join("config.json"), "{ non json").unwrap();
        assert!(!read_web_search_enabled(&dir), "corrotto → false");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
