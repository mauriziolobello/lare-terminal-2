//! # router — pure command-type classifier
//!
//! Classifies an input string + [`CommandKind`] hint into a [`Route`].
//!
//! ## Rules (from `Docs/03-protocol.md` + ADR-012)
//!
//! | priority | condition                                     | route   |
//! |----------|-----------------------------------------------|---------|
//! | 1 (top)  | input starts with `/` (after trim)            | `Slash` |
//! | 2        | `command_type` = `Os`                         | `Os`    |
//! | 2        | `command_type` = `Nl`                         | `Nl`    |
//! | 3        | `command_type` = `Auto`, starts with `$`      | `Os`    |
//! | 3        | `command_type` = `Auto`, known shell keyword  | `Os`    |
//! | 3        | `command_type` = `Auto`, bare drive-letter    | `Os`    |
//! | 3        | `command_type` = `Auto`, otherwise            | `Nl`    |
//!
//! ## Slash priority (ADR-012)
//! The `/` check runs BEFORE `command_type` matching, so an explicit `Os`
//! command starting with `/` also routes to `Slash`.  This mirrors the `$`
//! prefix for OS commands: the character is the definitive discriminator.
//! In practice the UI sends `/open …` with `command_type: Auto`, so explicit
//! `Os`/`Nl` slash inputs are theoretical but handled consistently.
//!
//! ## "Known shell keyword" heuristic (Auto mode)
//!
//! We keep the heuristic **small and documented** so it is testable and auditable:
//!
//! ```text
//! SHELL_TOKENS = {
//!   // Windows/cross
//!   "dir", "cd", "cls", "copy", "del", "move", "mkdir", "rmdir", "md", "rd",
//!   "type", "echo", "set", "path", "where", "whoami", "ipconfig", "ping",
//!   "tasklist", "taskkill", "netstat", "net", "reg", "sc",
//!   // Unix/cross
//!   "ls", "pwd", "cat", "mv", "rm", "cp", "mkdir", "rmdir", "touch",
//!   "grep", "find", "ps", "kill", "top", "df", "du", "chmod", "chown",
//!   "ssh", "scp", "curl", "wget", "tar", "zip", "unzip", "man",
//!   "git", "cargo", "python", "python3", "node", "npm", "npx",
//!   // Common programs
//!   "code", "vim", "nano", "nvim",
//! }
//! ```
//!
//! The heuristic is intentionally conservative: if in doubt it routes to `Nl`.
//! Additions should be driven by test cases and documented here.
//!
//! ## SRP
//! This module is a **pure function**: no I/O, no async, no side effects.
//! It takes a string and an enum; it returns an enum.  All business-logic
//! decisions live here so `core::handle_command` and tests can depend on them.

use protocol::CommandKind;

/// The route the orchestrator must take for this input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// Execute via `run_in_session` tool (Contratto B → mcp-server).
    Os,
    /// Process via the AI adapter (stub in Fase 1, Claude in Fase 2).
    Nl,
    /// Slash command (e.g. `/open`, `/reset`): dispatched by `core::handle_command`
    /// without involving the shell or AI adapter (ADR-012).
    Slash,
}

/// Shell-command first tokens that trigger `Os` routing in `Auto` mode.
///
/// Lowercase; comparison is case-insensitive.
const SHELL_TOKENS: &[&str] = &[
    // ── Windows built-ins ────────────────────────────────────────────────────
    "dir", "cd", "cls", "copy", "del", "move", "mkdir", "rmdir", "md", "rd", "type", "echo", "set",
    "path", "where", "whoami", "ipconfig", "ping", "tasklist", "taskkill", "netstat", "net", "reg",
    "sc", "attrib", "xcopy",
    // ── Unix / cross-platform ────────────────────────────────────────────────
    "ls", "pwd", "cat", "mv", "rm", "cp", "touch", "grep", "find", "ps", "kill", "top", "df", "du",
    "chmod", "chown", "ssh", "scp", "curl", "wget", "tar", "zip", "unzip", "man", "ln", "uname",
    // ── Developer tools ──────────────────────────────────────────────────────
    "git", "cargo", "python", "python3", "node", "npm", "npx", "yarn", "rustc", "rustup", "docker",
    "kubectl",
    // ── Editors (useful to route to OS so the UI can intercept) ─────────────
    "code", "vim", "nano", "nvim", "emacs",
];

/// Controlla se `input` (dopo trim) corrisponde esattamente al comando slash di uno dei plugin
/// scoperti. Ritorna il `plugin_id` del plugin corrispondente, o `None` se nessun plugin
/// rivendica quel comando.
///
/// ## Design (SRP / pure function)
/// La funzione è completamente pura: niente I/O, niente stato globale.
/// Riceve la lista di `(command, plugin_id)` dall'esterno (DIP) — l'host la costruisce
/// dinamicamente dai `DiscoveredPlugin`; i test la passano inline.
///
/// Il confronto è sul trim dell'input: l'utente potrebbe digitare "  /counter  " e
/// il comando registrato è "/counter" — devono combaciare.
///
/// ## Esempio
/// ```
/// use orchestrator::router::plugin_command;
/// let cmds = vec![("/counter".into(), "counter".into())];
/// assert_eq!(plugin_command("/counter", &cmds), Some("counter"));
/// assert_eq!(plugin_command("/nope", &cmds), None);
/// ```
pub fn plugin_command<'a>(input: &str, commands: &'a [(String, String)]) -> Option<&'a str> {
    let trimmed = input.trim();
    // `find` scansiona lineare la lista: ok per le dimensioni attese (<<100 plugin).
    // La closure confronta il comando registrato con l'input trimmed.
    commands
        .iter()
        .find(|(cmd, _)| cmd == trimmed)
        .map(|(_, id)| id.as_str())
}

/// Controlla se `input` (dopo trim) corrisponde esattamente allo `slash_trigger`
/// di uno dei canali tool esterni registrati. Ritorna l'`id` del canale
/// corrispondente, o `None` se nessuno lo rivendica. Mirror esatto di
/// `plugin_command` sopra, sul registro `ExternalToolChannel` invece che sui
/// plugin.
///
/// **Non ancora invocata da `ws.rs`** — vedi la nota di scope in
/// `Docs/superpowers/plans/2026-07-16-external-tool-channel.md` (Task 5):
/// aprire un canale esterno è un'azione lato frontend (nuova finestra Tauri),
/// non un `Command` che il backend dispatcha come per i plugin.
pub fn external_channel_command<'a>(
    input: &str,
    registry: &'a [crate::external_channel::ExternalToolChannel],
) -> Option<&'a str> {
    let trimmed = input.trim();
    registry
        .iter()
        .find(|c| c.slash_trigger == trimmed)
        .map(|c| c.id)
}

/// Classify a command `input` with the given `command_type` hint.
///
/// # Arguments
/// * `input`        — The raw input string from the client.
/// * `command_type` — The client's routing hint (`Auto`, `Os`, or `Nl`).
///
/// # Returns
/// [`Route::Slash`], [`Route::Os`], or [`Route::Nl`].
///
/// # Panics
/// Never.
pub fn classify(input: &str, command_type: CommandKind) -> Route {
    // ── Priority 1: slash check (ADR-012) ─────────────────────────────────────
    // The `/` check runs BEFORE command_type matching, making it the definitive
    // discriminator (parallel to `$` for OS commands).
    if input.trim().starts_with('/') {
        return Route::Slash;
    }

    match command_type {
        // Explicit override — always honour it.
        CommandKind::Os => Route::Os,
        CommandKind::Nl => Route::Nl,

        // Auto: apply the heuristic.
        CommandKind::Auto => classify_auto(input),
    }
}

/// Internal: apply heuristics for `CommandKind::Auto`.
fn classify_auto(input: &str) -> Route {
    let trimmed = input.trim();

    // Rule 1: "$"-prefix → always OS.
    // This lets users explicitly force the OS path without setting command_type.
    if trimmed.starts_with('$') {
        return Route::Os;
    }

    // Rule 2: first token is a known shell keyword (case-insensitive) → OS.
    let first_token = trimmed
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_lowercase();

    if SHELL_TOKENS.contains(&first_token.as_str()) {
        return Route::Os;
    }

    // Rule 3: bare drive-letter change (e.g. "c:", "P:\") → OS (PowerShell drive switch).
    if is_drive_change(trimmed) {
        return Route::Os;
    }

    // Default: treat as natural language.
    Route::Nl
}

/// True if `s` is a bare drive-letter change: one ASCII letter + ':' + optional '\'.
fn is_drive_change(s: &str) -> bool {
    let b = s.as_bytes();
    match b.len() {
        2 => b[0].is_ascii_alphabetic() && b[1] == b':',
        3 => b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\',
        _ => false,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests — written FIRST (TDD: RED → GREEN → REFACTOR)
//
// The test table is the specification.  Add a row, run cargo test, see it fail
// (RED), then extend classify() to make it pass (GREEN).
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::CommandKind;

    // ── Helper ────────────────────────────────────────────────────────────────

    /// Shorthand: classify with a given kind and assert the expected route.
    fn check(input: &str, kind: CommandKind, expected: Route) {
        // Clone kind so we can use it in the format string after the move.
        let kind_dbg = format!("{kind:?}");
        let got = classify(input, kind);
        assert_eq!(
            got, expected,
            "classify({input:?}, {kind_dbg}) → expected {expected:?}, got {got:?}"
        );
    }

    // ── Explicit CommandKind::Os ──────────────────────────────────────────────

    #[test]
    fn explicit_os_is_always_os() {
        check("tell me about Rust", CommandKind::Os, Route::Os);
        check("what is the weather", CommandKind::Os, Route::Os);
        check("dir", CommandKind::Os, Route::Os);
        check("", CommandKind::Os, Route::Os);
    }

    // ── Explicit CommandKind::Nl ──────────────────────────────────────────────

    #[test]
    fn explicit_nl_is_always_nl() {
        check("dir", CommandKind::Nl, Route::Nl);
        check("$ pwd", CommandKind::Nl, Route::Nl);
        check("echo hello", CommandKind::Nl, Route::Nl);
        check("", CommandKind::Nl, Route::Nl);
    }

    // ── Auto: "$" prefix → Os ─────────────────────────────────────────────────

    #[test]
    fn auto_dollar_prefix_routes_to_os() {
        check("$ dir", CommandKind::Auto, Route::Os);
        check("$pwd", CommandKind::Auto, Route::Os);
        check("$ ls -la", CommandKind::Auto, Route::Os);
        check("$ cargo build", CommandKind::Auto, Route::Os);
    }

    #[test]
    fn auto_dollar_prefix_with_leading_whitespace_routes_to_os() {
        // trim() is applied before the "$" check
        check("   $ dir", CommandKind::Auto, Route::Os);
        check("\t$ ls", CommandKind::Auto, Route::Os);
    }

    // ── Auto: known shell tokens → Os ────────────────────────────────────────

    #[test]
    fn auto_windows_builtins_route_to_os() {
        check("dir", CommandKind::Auto, Route::Os);
        check("cd C:\\Users", CommandKind::Auto, Route::Os);
        check("cls", CommandKind::Auto, Route::Os);
        check("echo hello world", CommandKind::Auto, Route::Os);
        check("ipconfig /all", CommandKind::Auto, Route::Os);
        check("whoami", CommandKind::Auto, Route::Os);
        check("tasklist", CommandKind::Auto, Route::Os);
        check("netstat -an", CommandKind::Auto, Route::Os);
        check("ping 8.8.8.8", CommandKind::Auto, Route::Os);
    }

    #[test]
    fn auto_unix_commands_route_to_os() {
        check("ls -la", CommandKind::Auto, Route::Os);
        check("ls", CommandKind::Auto, Route::Os);
        check("pwd", CommandKind::Auto, Route::Os);
        check("cat /etc/hosts", CommandKind::Auto, Route::Os);
        check("ps aux", CommandKind::Auto, Route::Os);
        check("grep foo bar.txt", CommandKind::Auto, Route::Os);
        check("find . -name '*.rs'", CommandKind::Auto, Route::Os);
        check("curl https://example.com", CommandKind::Auto, Route::Os);
        check("tar -xzf archive.tar.gz", CommandKind::Auto, Route::Os);
    }

    #[test]
    fn auto_developer_tools_route_to_os() {
        check("git status", CommandKind::Auto, Route::Os);
        check("git log --oneline", CommandKind::Auto, Route::Os);
        check("cargo build", CommandKind::Auto, Route::Os);
        check("cargo test", CommandKind::Auto, Route::Os);
        check("cargo clippy", CommandKind::Auto, Route::Os);
        check("docker ps", CommandKind::Auto, Route::Os);
        check("npm install", CommandKind::Auto, Route::Os);
        check("node server.js", CommandKind::Auto, Route::Os);
        check("python3 script.py", CommandKind::Auto, Route::Os);
    }

    #[test]
    fn auto_shell_token_is_case_insensitive() {
        // Windows users often write DIR in uppercase.
        check("DIR", CommandKind::Auto, Route::Os);
        check("Git status", CommandKind::Auto, Route::Os);
        check("ECHO hello", CommandKind::Auto, Route::Os);
        check("LS -la", CommandKind::Auto, Route::Os);
    }

    // ── Auto: natural language → Nl ───────────────────────────────────────────

    #[test]
    fn auto_natural_language_routes_to_nl() {
        check("What is Rust?", CommandKind::Auto, Route::Nl);
        check("explain async await", CommandKind::Auto, Route::Nl);
        check("how do I open a file in Rust", CommandKind::Auto, Route::Nl);
        check("translate this to Italian", CommandKind::Auto, Route::Nl);
        check(
            "write a unit test for fn foo()",
            CommandKind::Auto,
            Route::Nl,
        );
        check("summarize the error above", CommandKind::Auto, Route::Nl);
    }

    #[test]
    fn auto_empty_string_routes_to_nl() {
        // Empty input: no token, no "$" → Nl (safe default).
        check("", CommandKind::Auto, Route::Nl);
        check("   ", CommandKind::Auto, Route::Nl);
    }

    #[test]
    fn auto_unknown_token_routes_to_nl() {
        // A program not in SHELL_TOKENS → Nl (safe default; user can use "$" to force).
        check("laretool --help", CommandKind::Auto, Route::Nl);
        check("myapp run", CommandKind::Auto, Route::Nl);
    }

    // ── Edge cases ────────────────────────────────────────────────────────────

    #[test]
    fn auto_dollar_sign_alone_routes_to_os() {
        // A lone "$" still starts with "$" → Os.
        check("$", CommandKind::Auto, Route::Os);
    }

    #[test]
    fn explicit_nl_overrides_dollar_prefix() {
        // Even if the user types "$ pwd", an explicit Nl kind forces Nl.
        check("$ pwd", CommandKind::Nl, Route::Nl);
    }

    #[test]
    fn explicit_os_overrides_natural_language() {
        // Even if it looks like NL, explicit Os forces Os.
        check("What is Rust?", CommandKind::Os, Route::Os);
    }

    // ── Slash route (ADR-012) ─────────────────────────────────────────────────
    //
    // TDD: questi test sono stati scritti PRIMA dell'implementazione della
    // branch `/` in `classify`. Prima dell'implementazione, `/open x` con
    // CommandKind::Auto restituiva Route::Nl (nessun token noto, nessun `$`).
    //
    // RED output (run pre-implementazione):
    // test router::tests::slash_open_routes_to_slash ... FAILED
    // failures:
    //   classify("/open http://x", Auto) → expected Slash, got Nl
    // test router::tests::slash_reset_routes_to_slash ... FAILED
    //   classify("/reset", Auto) → expected Slash, got Nl
    //
    // GREEN dopo l'aggiunta del check `/` in `classify` prima del `match`.

    #[test]
    fn slash_open_routes_to_slash() {
        // /open con qualsiasi target → Slash (qualunque command_type)
        check("/open http://example.com", CommandKind::Auto, Route::Slash);
        check("/open /tmp/foo", CommandKind::Auto, Route::Slash);
        check("/open C:\\Users", CommandKind::Auto, Route::Slash);
    }

    #[test]
    fn slash_reset_routes_to_slash() {
        check("/reset", CommandKind::Auto, Route::Slash);
    }

    #[test]
    fn slash_unknown_routes_to_slash() {
        // Comandi slash sconosciuti: raggiungono `Slash` e poi core emette Error.
        check("/sconosciuto", CommandKind::Auto, Route::Slash);
        check("/config", CommandKind::Auto, Route::Slash);
    }

    #[test]
    fn slash_with_leading_whitespace_routes_to_slash() {
        // trim() prima del check `/`
        check("  /open x", CommandKind::Auto, Route::Slash);
        check("\t/reset", CommandKind::Auto, Route::Slash);
    }

    #[test]
    fn slash_overrides_explicit_os_and_nl() {
        // Il check `/` ha priorità MASSIMA (viene prima di command_type).
        check("/open x", CommandKind::Os, Route::Slash);
        check("/open x", CommandKind::Nl, Route::Slash);
    }

    #[test]
    fn non_slash_inputs_unaffected_by_slash_rule() {
        // Assicuriamo che OS/NL normali non vengano risucchiati dal check `/`.
        check("dir", CommandKind::Auto, Route::Os);
        check("ls -la", CommandKind::Auto, Route::Os);
        check("What is Rust?", CommandKind::Auto, Route::Nl);
        check("$ ls", CommandKind::Auto, Route::Os);
    }

    // ── plugin_command — riconoscimento slash-comando plugin ──────────────────
    //
    // TDD: test scritti PRIMA dell'implementazione di `plugin_command`.
    // RED atteso: `error[E0425]: cannot find function 'plugin_command' in module`
    //
    // GREEN dopo l'aggiunta della funzione pura `plugin_command` in questo modulo.

    #[test]
    fn recognizes_plugin_command() {
        let cmds = vec![
            ("/counter".to_string(), "counter".to_string()),
            ("/calc".into(), "calc".into()),
        ];
        // Match esatto (dopo trim) -> restituisce il plugin_id.
        assert_eq!(plugin_command("/counter", &cmds), Some("counter"));
        // Trim degli spazi iniziali/finali prima del confronto.
        assert_eq!(plugin_command("  /counter  ", &cmds), Some("counter"));
        // Secondo plugin riconosciuto correttamente.
        assert_eq!(plugin_command("/calc", &cmds), Some("calc"));
        // Slash-comando non in lista -> None.
        assert_eq!(plugin_command("/nope", &cmds), None);
        // Senza slash non è un comando plugin (non può coincidere con "/counter").
        assert_eq!(plugin_command("counter", &cmds), None);
    }

    // ── Bare drive-letter change (Rule 3) ─────────────────────────────────────

    #[test]
    fn auto_bare_drive_letter_routes_to_os() {
        check("c:", CommandKind::Auto, Route::Os);
        check("C:\\", CommandKind::Auto, Route::Os);
        check("p:", CommandKind::Auto, Route::Os);
        check("Z:", CommandKind::Auto, Route::Os);
    }

    #[test]
    fn auto_non_bare_drive_stays_nl() {
        check("c:foo", CommandKind::Auto, Route::Nl);   // non è un bare drive change
        check("cc:", CommandKind::Auto, Route::Nl);
        check(":", CommandKind::Auto, Route::Nl);
        check("c:\\Users", CommandKind::Auto, Route::Nl); // ha un path: lo gestisce cd/NL, non questa regola
    }

    // ── external_channel_command — riconoscimento slash-comando canale tool esterno ──

    #[test]
    fn external_channel_command_matches_registered_trigger() {
        use crate::external_channel::ExternalToolChannel;
        fn fake_factory() -> anyhow::Result<std::sync::Arc<dyn crate::tool_client::ToolClient>> {
            unreachable!("mai chiamata in questo test")
        }
        let registry = vec![ExternalToolChannel {
            id: "nmap",
            slash_trigger: "/nmap",
            window_title: "Nmap",
            tool_client: fake_factory,
            format_invocation: None,
            system_prompt_override: None,
        }];
        assert_eq!(external_channel_command("/nmap", &registry), Some("nmap"));
        assert_eq!(external_channel_command("  /nmap  ", &registry), Some("nmap"), "trim come plugin_command");
        assert_eq!(external_channel_command("/altro", &registry), None);
    }

    #[test]
    fn external_channel_command_unregistered_trigger_against_real_registry_is_none() {
        // Rewritten (was `external_channel_command_empty_registry_always_none`,
        // written when EXTERNAL_TOOL_CHANNELS was empty and "/nmap" was chosen
        // as the example of an unregistered trigger — "/nmap" IS registered
        // now, so this test uses a genuinely unregistered trigger instead,
        // same principle as `unregistered_channel_id_still_errs_against_the_real_registry`
        // in external_channel.rs.
        assert_eq!(
            external_channel_command("/some-future-trigger-not-yet-built", crate::external_channel::EXTERNAL_TOOL_CHANNELS),
            None
        );
    }
}
