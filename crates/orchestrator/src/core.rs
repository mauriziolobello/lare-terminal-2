//! # core — transport-agnostic command handler
//!
//! The nucleus of the orchestrator.  Contains a single public function:
//!
//! ```text
//! async fn handle_command(…, tx: UnboundedSender<ServerMsg>)
//! ```
//!
//! This function knows nothing about WebSocket, HTTP, or Telegram.  The
//! transport layer calls it and drains the messages emitted on `tx` over
//! whatever channel it manages.  This separation lets Telegram, CLI tests,
//! and future channels reuse the same logic without duplication.
//!
//! ## Command flow (v0.10.0 — /help)
//!
//! ```text
//! handle_command
//!   │
//!   ├─ /nowin intercept (PRIMA di router::classify)
//!   │     ├─ "/nowin <prompt>" → handle_nl(allow_windows=false) con prompt PULITO
//!   │     │     → AI riceve tool SENZA show_markdown → Chunk(streaming) + Done{exit_code: None}
//!   │     └─ "/nowin" (vuoto) → Chunk("usa /nowin <richiesta>") + Done{exit_code: Some(1)}
//!   │
//!   ├─ router::classify(input, command_type)
//!   │
//!   ├─ Route::Slash
//!   │     ├─ parse first word after "/"
//!   │     ├─ "open <target>"   → tools.open_target(target)
//!   │     │     ok=true  → Chunk(message) + Done{exit_code: Some(0)}
//!   │     │     ok=false → Chunk(message) + Done{exit_code: Some(1)}
//!   │     ├─ "reset"           → tools.reset_session() + Chunk("sessione riavviata") + Done{0}
//!   │     ├─ "show <markdown>" → OpenWindow{kind:Markdown, title, content} + Done{exit_code: Some(0)}
//!   │     │     empty content  → Error{RoutingError}
//!   │     ├─ "help"            → OpenWindow{kind:Help, title:"Lare — Comandi", HELP_MARKDOWN} + Done{0}
//!   │     └─ altro             → Error{RoutingError, "comando slash sconosciuto: /<cmd>"}
//!   │
//!   ├─ Route::Os
//!   │     ├─ strip leading "$" + whitespace from input (if present)
//!   │     ├─ tools.run_in_session(cleaned_input)   ← persistent shell session
//!   │     ├─ Chunk { id, content: stdout }   (if stdout non-empty)
//!   │     ├─ Chunk { id, content: stderr }   (if stderr non-empty)
//!   │     └─ Done  { id, exit_code }
//!   │
//!   └─ Route::Nl
//!         └─ ai.respond(...) — loop tool-use stateful che EMETTE su `tx`:
//!              Chunk(testo in streaming) / OpenWindow / trasparenza tool,
//!              terminato da Done { id, exit_code: None }
//! ```
//!
//! ## Error handling convention (stabilised in v0.3.0)
//! - `Done{exit_code: Some(N)}` for "command dispatched, result is N"
//!   (both the Os branch and `/open` with ok=false use this).
//! - `Error{RoutingError}` for "could not dispatch at all"
//!   (unknown slash command, not found in the router dispatch table).
//!
//! This preserves the client's ability to distinguish "ran and failed" from
//! "could not route."  Precedent: core.rs inherited this convention from the
//! Os branch (see doc comment in v0.2.0).
//!
//! ## Streaming (Fase 2 / Slice 4)
//! Fase 1 era fully buffered (`Vec<ServerMsg>`).  Dal Slice 4 `handle_command`
//! emette i `ServerMsg` su un `tokio::sync::mpsc::UnboundedSender<ServerMsg>`
//! (posseduto, droppato a fine chiamata → canale chiuso).  `ws.rs` fa girare
//! produttore (`handle_command`) e consumatore (drain → WS sink) concorrentemente
//! via `tokio::join!`.  La logica di routing/shaping resta invariata.

use crate::ai_adapter::{AiAdapter, ToolConfirmer};
use crate::router;
use crate::tool_client::ToolClient;
use protocol::{CommandKind, ErrCode, ServerMsg, WindowKind};
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

// ── Routing dalla shell (2.0, spec §3) ────────────────────────────────────────
//
// Liste usate dal pre-router `shell_slash` per sapere quali slash del backend
// v1 esistono e quali di questi producono già una finestra propria. Vivono
// qui (non in `shell_slash.rs`) perché `handle_slash`, che le due liste
// descrivono, è definita in questo modulo.

/// Comandi slash che `handle_slash` dispaccia davvero (senza `/`), esclusi
/// `reset` (che sulla shell ha una risposta propria, spec §3) e `find`/`nowin`
/// (gestiti in `ws.rs`/`handle_command`, non disponibili dalla shell in questa
/// versione). È l'unica fonte per "questo slash esiste" del pre-router
/// `shell_slash`.
///
/// Il test `known_backend_slashes_are_all_dispatched_by_handle_slash` prova
/// SOLO la direzione lista → `handle_slash`: che ogni comando elencato qui sia
/// davvero dispacciato (nessun `Error{"sconosciuto"}`). Non prova il
/// contrario — un nuovo `match` arm aggiunto a `handle_slash` senza essere
/// elencato qui non fa fallire alcun test: resterebbe raggiungibile dal
/// backend ma invisibile a `shell_slash`, che lo scarterebbe in silenzio.
/// Tenere le due cose allineate è quindi una responsabilità di revisione: chi
/// aggiunge un braccio a `handle_slash` lo aggiunge anche qui.
pub const KNOWN_BACKEND_SLASHES: &[&str] = &["open", "web", "show", "help"];

/// Sottoinsieme di `KNOWN_BACKEND_SLASHES` il cui esito È già una finestra
/// (`OpenWindow`): per questi la shell NON apre la finestra di output col
/// segnaposto (spec §3.2, eccezione) — altrimenti ne comparirebbero due.
pub const WINDOW_SLASHES: &[&str] = &["help", "show"];

// ── Help content ─────────────────────────────────────────────────────────────
//
// Titoli della finestra speciale di `/help` (WindowKind::Help).
// Il corpo Markdown vive in file esterni sotto <config_dir>/help/<lang>.md
// ed è caricato da crate::help::load_help_body.

const HELP_TITLE_IT: &str = "Lare \u{2014} Comandi";
const HELP_TITLE_EN: &str = "Lare \u{2014} Commands";

/// Handle a single client `Command`, emitting all response messages on `tx`.
///
/// # Arguments
/// * `id`           — Command correlation ID (echoed in every response).
/// * `input`        — Raw user input string.
/// * `command_type` — Routing hint from the client.
/// * `cwd`          — Tracked shell cwd, used by `/open` to resolve relative
///   targets (Task 4).  `None` = unknown (pre-Task-4 behaviour: targets passed
///   verbatim).  Does NOT change the session's working directory (ADR-011).
/// * `ai`           — AI adapter (dyn: test → stub, prod → Claude).
/// * `tools`        — Tool client (dyn: test → fake, prod → McpToolClient).
/// * `tx`           — Unbounded sender; owned (dropped at end → channel closes).
// Argomenti fissi da contratto (Slice 4 / Task 3 + Task 2 + stop button + lang): id, input, kind, cwd, history, ai, tools, web_search, lang, confirmer, cancel, tx.
#[allow(clippy::too_many_arguments)]
pub async fn handle_command(
    id: &str,
    input: &str,
    command_type: CommandKind,
    cwd: Option<&str>,
    history: &mut crate::messages_client::ConversationHistory,
    ai: &dyn AiAdapter,
    tools: &dyn ToolClient,
    format_invocation: Option<fn(&str, &serde_json::Value) -> String>,
    system_prompt_override: Option<&'static str>,
    web_search: bool,
    lang: Option<String>,
    config_dir: &std::path::Path,
    confirmer: Option<&dyn ToolConfirmer>,
    cancel: Option<CancellationToken>,
    tx: UnboundedSender<ServerMsg>,
) {
    // `/nowin <prompt>`: forza risposta testo-semplice rimuovendo `show_markdown`
    // per quel solo turno — la storia salva il prompt PULITO (niente inquinamento).
    if let Some(rest) = strip_nowin_prefix(input) {
        if rest.is_empty() {
            let _ = tx.send(ServerMsg::Chunk {
                id: id.to_string(),
                content: "usa /nowin <richiesta>".to_string(),
            });
            let _ = tx.send(ServerMsg::Done {
                id: id.to_string(),
                exit_code: Some(1),
            });
            return;
        }
        // /nowin: niente finestre. web_search e lang sono propagati dal Command del client.
        handle_nl(id, rest, history, ai, tools,
            crate::agent::TurnOptions { allow_windows: false, web_search, format_invocation, system_prompt_override, lang }, confirmer, cancel, tx).await;
        return;
    }

    let route = router::classify(input, command_type);

    match route {
        // Slash/Os restano helper puri (ritornano Vec); li inoltriamo su `tx`
        // (per riferimento: `tx.send` prende `&self`). `tx` si droppa a fine arm.
        router::Route::Slash => {
            // Pass the tracked cwd so `/open` can resolve relative targets.
            // `cwd.unwrap_or("")` → empty string when cwd is unknown, which
            // makes `resolve_open_target` leave relative targets verbatim
            // (safe for URLs and absolute paths; same as pre-Task-4 for relative).
            for m in handle_slash(id, input, tools, cwd.unwrap_or(""), lang.as_deref(), config_dir).await {
                if tx.send(m).is_err() {
                    return;
                }
            }
        }
        router::Route::Os => {
            handle_os(id, input, tools, &tx).await;
        }
        // Nl: `tx` spostato per valore in `handle_nl` → `respond`.
        router::Route::Nl => handle_nl(id, input, history, ai, tools,
            crate::agent::TurnOptions { allow_windows: true, web_search, format_invocation, system_prompt_override, lang }, confirmer, cancel, tx).await,
    }
}

// ── Slash branch ──────────────────────────────────────────────────────────────

/// Dispatch a slash command (input starts with `/`).
///
/// Parsing: the raw input (e.g. `"/open http://example.com"`) is trimmed,
/// the leading `/` is stripped, and the first word is the command name.
///
/// | Slash command     | Action                                              |
/// |-------------------|-----------------------------------------------------|
/// | `open <target>`   | `tools.open_target(resolved)` → Chunk + Done{0/1}  |
/// | `reset`           | `tools.reset_session()` → Chunk + Done{0}           |
/// | `show <markdown>` | `OpenWindow{Markdown, title, content}` + `Done{0}`  |
/// | `show` (empty)    | `Error{RoutingError}` (usage error)                 |
/// | anything else     | `Error{RoutingError}` (no dispatch)                 |
///
/// Error convention (ADR-012, stabilised in v0.3.0):
/// - Unknown slash: `Error{RoutingError}` — routing failure, not "ran and failed."
/// - `/open <missing>`: `Done{exit_code: Some(1)}` — dispatched, target invalid.
/// - `/open <found>`: `Done{exit_code: Some(0)}`.
///
/// # `cwd` parameter
/// The tracked shell cwd (from `cwd_state` in `ws.rs`).  Used by `/open` to
/// resolve relative targets against the real shell directory rather than the
/// orchestrator process directory.  Pass `""` when the cwd is unknown (no
/// resolution performed — relative targets are passed verbatim, which is the
/// pre-Task-4 behaviour and is safe for URLs and absolute paths).
/// * `config_dir` — Path della directory di configurazione, usata per individuare `help/`.
async fn handle_slash(
    id: &str,
    input: &str,
    tools: &dyn ToolClient,
    cwd: &str,
    lang: Option<&str>,
    config_dir: &std::path::Path,
) -> Vec<ServerMsg> {
    // Strip leading `/` (after trim).
    let trimmed = input.trim();
    let without_slash = trimmed.strip_prefix('/').unwrap_or(trimmed);

    // Split into command + rest.
    let mut parts = without_slash.splitn(2, char::is_whitespace);
    let cmd = parts.next().unwrap_or("").to_ascii_lowercase();
    let rest = parts.next().unwrap_or("").trim();

    match cmd.as_str() {
        "open" => {
            // `/open <target>`: target may be empty if the user typed just `/open`.
            // An empty target → classify_target("") → NotFound → ok=false → Done{1}.
            //
            // Resolve relative paths against the tracked shell cwd (Task 4):
            // `resolve_open_target` returns the target unchanged if it is a URL
            // or an absolute path; otherwise it joins it against `cwd`.
            // This ensures `/open fattura.pdf` opens the file in the shell's
            // current directory, not in the orchestrator process directory.
            let resolved = resolve_open_target(rest, cwd);
            let result = tools.open_target(&resolved).await;
            let exit_code = if result.ok { 0 } else { 1 };
            vec![
                ServerMsg::Chunk {
                    id: id.to_string(),
                    content: result.message,
                },
                ServerMsg::Done {
                    id: id.to_string(),
                    exit_code: Some(exit_code),
                },
            ]
        }

        "reset" => {
            tools.reset_session().await;
            vec![
                ServerMsg::Chunk {
                    id: id.to_string(),
                    content: "sessione riavviata".to_string(),
                },
                ServerMsg::Done {
                    id: id.to_string(),
                    exit_code: Some(0),
                },
            ]
        }

        // `/show <markdown>` — ADR-013, Superficie 3.
        //
        // Emits `ServerMsg::OpenWindow{kind:Markdown}` followed by
        // `Done{exit_code: Some(0)}` (conformità protocollo: ogni Command termina
        // con Done/Error; evita spinner UI bloccato).
        // The UI creates a new `WebviewWindow` and renders the sanitised Markdown.
        //
        // Title derivation (char-safe, no byte-slice on UTF-8):
        //   1. Split `content` by lines.
        //   2. Find first non-empty line after trimming.
        //   3. Truncate to at most 60 `char`s (not bytes — content may be UTF-8).
        //   4. Fall back to `"Lare — Output"` if no non-empty line exists.
        //
        // Design decision for empty content: return `Error{RoutingError}`.
        // An empty `/show` is a usage error (nothing to display), not a valid
        // empty window.  Rationale documented in CHANGELOG.
        "show" => {
            // `rest` is already trimmed in `handle_slash`.
            // For `/show` with no content, `rest` is empty.
            if rest.is_empty() {
                return vec![ServerMsg::Error {
                    id: id.to_string(),
                    code: ErrCode::RoutingError,
                    message: "nessun contenuto da mostrare: usa /show <markdown>".to_string(),
                }];
            }

            // `content` preserves internal whitespace/newlines verbatim.
            let content = rest.to_string();

            // Derive title from first non-empty line (char-safe truncation).
            let title = content
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty())
                .map(|line| {
                    // Truncate to 60 chars (not bytes).
                    line.chars().take(60).collect::<String>()
                })
                .unwrap_or_else(|| "Lare \u{2014} Output".to_string());

            vec![
                ServerMsg::OpenWindow {
                    title,
                    kind: WindowKind::Markdown,
                    content,
                },
                ServerMsg::Done {
                    id: id.to_string(),
                    exit_code: Some(0),
                },
            ]
        }

        // `/web <query>`: apre il browser di default su una ricerca esterna (Google).
        // Encoding inline (nessuna dipendenza) via `percent_encode_query`.
        "web" => {
            if rest.is_empty() {
                return vec![ServerMsg::Error {
                    id: id.to_string(),
                    code: ErrCode::RoutingError,
                    message: "uso: /web <ricerca>".to_string(),
                }];
            }
            let url = format!(
                "https://www.google.com/search?q={}",
                percent_encode_query(rest)
            );
            let result = tools.open_target(&url).await;
            let exit_code = if result.ok { 0 } else { 1 };
            vec![
                ServerMsg::Chunk {
                    id: id.to_string(),
                    content: result.message,
                },
                ServerMsg::Done {
                    id: id.to_string(),
                    exit_code: Some(exit_code),
                },
            ]
        }

        // `/help` — apre una finestra di sistema con la lista dei comandi.
        //
        // Emits `ServerMsg::OpenWindow{kind:Help}` followed by
        // `Done{exit_code: Some(0)}` — same pattern as `/show` (orchestrator 0.8.0):
        // every Command must terminate with Done/Error to close the UI spinner.
        // NOT emitting Done would reintroduce the "spinner bloccato" regression.
        "help" => {
            let (title, lang_code) = match lang {
                Some("en") => (HELP_TITLE_EN, "en"),
                _ => (HELP_TITLE_IT, "it"),
            };
            let help_dir = crate::help::help_dir_path(config_dir);
            vec![
                ServerMsg::OpenWindow {
                    title: title.to_string(),
                    kind: WindowKind::Help,
                    content: crate::help::load_help_body(&help_dir, lang_code),
                },
                ServerMsg::Done {
                    id: id.to_string(),
                    exit_code: Some(0),
                },
            ]
        }

        other => {
            // Unknown slash command: routing failure → Error (not Done).
            vec![ServerMsg::Error {
                id: id.to_string(),
                code: ErrCode::RoutingError,
                message: format!("comando slash sconosciuto: /{other}"),
            }]
        }
    }
}

// ── OS branch ─────────────────────────────────────────────────────────────────

/// Strip the optional leading `$` prefix (and surrounding whitespace) from a
/// user-typed command.
///
/// Users may type `$ dir` in Auto mode as a shorthand for "force OS".  The
/// shell should receive `dir`, not `$ dir`.
///
/// Examples:
/// * `"$ pwd"`   → `"pwd"`
/// * `"$pwd"`    → `"pwd"`
/// * `"dir"`     → `"dir"` (unchanged — no prefix)
/// * `"$ "`      → `""` (just whitespace after $)
fn strip_dollar_prefix(input: &str) -> &str {
    let trimmed = input.trim();
    if let Some(rest) = trimmed.strip_prefix('$') {
        rest.trim_start()
    } else {
        trimmed
    }
}

async fn handle_os(id: &str, input: &str, tools: &dyn ToolClient, tx: &UnboundedSender<ServerMsg>) {
    let command = strip_dollar_prefix(input);

    let (progress_tx, progress_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    let tx_clone = tx.clone();
    let id_owned = id.to_string();

    // Oneshot signal: the chunk task fires it on the first line received.
    // After chunk_task completes, try_recv() tells us whether any line arrived.
    let (first_tx, mut first_rx) = tokio::sync::oneshot::channel::<()>();
    let mut first_signal = Some(first_tx);

    // Send each output line directly to the WebSocket sink (tx_clone) in real time.
    let chunk_task = tokio::spawn(async move {
        let mut rx = progress_rx;
        while let Some(line) = rx.recv().await {
            if let Some(s) = first_signal.take() {
                let _ = s.send(());
            }
            if tx_clone.send(ServerMsg::Chunk {
                id: id_owned.clone(),
                content: line,
            }).is_err() {
                break; // receiver dropped (command cancelled)
            }
        }
    });

    let result = tools.run_in_session(command, Some(progress_tx)).await;
    chunk_task.await.ok();

    // Fallback: if no line arrived via streaming (e.g. FakeToolClient in tests
    // that don't forward progress) but stdout is non-empty, emit it as one Chunk.
    let has_streamed = first_rx.try_recv().is_ok();
    if !has_streamed && !result.stdout.is_empty() {
        let _ = tx.send(ServerMsg::Chunk {
            id: id.to_string(),
            content: result.stdout,
        });
    }

    if !result.stderr.is_empty() {
        let _ = tx.send(ServerMsg::Chunk {
            id: id.to_string(),
            content: result.stderr,
        });
    }

    let _ = tx.send(ServerMsg::Done {
        id: id.to_string(),
        exit_code: Some(result.exit_code),
    });
}

// ── NL branch ─────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn handle_nl(
    id: &str,
    input: &str,
    history: &mut crate::messages_client::ConversationHistory,
    ai: &dyn AiAdapter,
    tools: &dyn ToolClient,
    opts: crate::agent::TurnOptions,
    confirmer: Option<&dyn ToolConfirmer>,
    cancel: Option<CancellationToken>,
    tx: UnboundedSender<ServerMsg>,
) {
    ai.respond(id, input, history, tools, opts, confirmer, cancel, tx).await
}

// ── /web helper ──────────────────────────────────────────────────────────────

/// Percent-encode una query per un URL (RFC 3986 unreserved restano, il resto
/// diventa %XX in UTF-8). Sufficiente per costruire un URL di ricerca.
/// Nessuna dipendenza esterna — inline per semplicità (scope limitato a URL di ricerca).
fn percent_encode_query(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

// ── resolve_open_target ───────────────────────────────────────────────────────

/// Resolve an `/open` target against the tracked shell cwd.
///
/// Rules (evaluated in order):
/// 1. If `target` starts with `http://` or `https://` → return unchanged (URL).
/// 2. If `target` is an absolute path (`Path::is_absolute`) → return unchanged.
/// 3. Otherwise → relative path: return `Path::new(cwd).join(target)` as a String.
///
/// This function is *pure* (no I/O, no async) so it is easy to unit-test and
/// can be composed freely.  The actual OS/URL classification (file vs. folder vs.
/// URL) stays inside `mcp-server`'s `open_target`; here we only normalise the
/// path representation before passing it downstream.
pub fn resolve_open_target(target: &str, cwd: &str) -> String {
    // URL check first: only `http://` and `https://` are recognised as URLs
    // and passed verbatim to the OS URL handler (browser).
    // Known limitation: other schemes (e.g. `ftp://`, `file://`) are NOT
    // detected here and fall through to the relative-path join below.
    if target.starts_with("http://") || target.starts_with("https://") {
        return target.to_string();
    }

    let path = std::path::Path::new(target);
    if path.is_absolute() {
        // Already absolute — nothing to do.
        return target.to_string();
    }

    // Relative path: join against the tracked cwd.
    // `Path::join` handles both `/` and `\` separators on Windows correctly.
    std::path::Path::new(cwd)
        .join(target)
        .to_string_lossy()
        .into_owned()
}

// ── /nowin helper ────────────────────────────────────────────────────────────

/// Se `input` (trim) inizia con `/nowin` (case-insensitive) seguito da spazio o
/// fine, ritorna il resto della richiesta (trimmed). Altrimenti `None`.
fn strip_nowin_prefix(input: &str) -> Option<&str> {
    let t = input.trim_start();
    let rest = t.strip_prefix('/')?;
    let (cmd, after) = match rest.find(char::is_whitespace) {
        Some(i) => (&rest[..i], rest[i..].trim()),
        None => (rest, ""),
    };
    if cmd.eq_ignore_ascii_case("nowin") {
        Some(after)
    } else {
        None
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests — TDD: RED then GREEN
//
// All tests here use FakeToolClient + StubAdapter so they are:
//  • deterministic (no processes, no network)
//  • fast (async without I/O)
//  • self-contained (no external dependencies)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_adapter::StubAdapter;
    use crate::tool_client::{FakeToolClient, FixtureChannelToolClient};
    use protocol::{CommandKind, ServerMsg};

    // ── helper ────────────────────────────────────────────────────────────────

    /// Call `handle_command` con StubAdapter e FakeToolClient, raccogliendo i msg.
    /// `confirmer: None` — UI locale (autonoma); nessun gate sui tool AI.
    /// `cancel: None` — nessuna cancellazione (comportamento invariato).
    async fn run(
        id: &str,
        input: &str,
        kind: CommandKind,
        cwd: Option<&str>,
        tools: FakeToolClient,
    ) -> Vec<ServerMsg> {
        let ai = StubAdapter;
        let mut history = crate::messages_client::ConversationHistory::new();
        let tmp = tempfile::tempdir().expect("tempdir");
        let help_dir = tmp.path().join("help");
        let _ = std::fs::create_dir_all(&help_dir);
        let it_content = "# Lare \u{2014} Comandi\n\n## Comandi\n- /help\n- /ping\n- /config\n- /library\n- /aichat\n- /markets\n- /nmap\n- /pyping\n";
        let _ = std::fs::write(help_dir.join("it.md"), it_content);
        let _ = std::fs::write(help_dir.join("en.md"), "# Lare \u{2014} Commands\n\n## Commands\n- /help\n- /config\n");
        crate::test_support::collect(|tx| {
            handle_command(id, input, kind, cwd, &mut history, &ai, &tools, None, None, false, None, tmp.path(), None, None, tx)
        })
        .await
    }

    // ── strip_dollar_prefix unit tests ────────────────────────────────────────

    #[test]
    fn strip_dollar_with_space() {
        assert_eq!(strip_dollar_prefix("$ pwd"), "pwd");
    }

    #[test]
    fn strip_dollar_no_space() {
        assert_eq!(strip_dollar_prefix("$dir"), "dir");
    }

    #[test]
    fn strip_dollar_leading_whitespace() {
        assert_eq!(strip_dollar_prefix("  $ ls -la"), "ls -la");
    }

    #[test]
    fn no_dollar_unchanged() {
        assert_eq!(strip_dollar_prefix("git status"), "git status");
    }

    #[test]
    fn dollar_only() {
        assert_eq!(strip_dollar_prefix("$"), "");
    }

    // ── OS command: stdout only ───────────────────────────────────────────────

    #[tokio::test]
    async fn os_command_stdout_only_produces_chunk_then_done() {
        let msgs = run(
            "cmd-1",
            "dir",
            CommandKind::Os,
            None,
            FakeToolClient::success("file.txt\n"),
        )
        .await;

        assert_eq!(msgs.len(), 2, "expected [Chunk, Done], got {msgs:?}");

        assert_eq!(
            msgs[0],
            ServerMsg::Chunk {
                id: "cmd-1".to_string(),
                content: "file.txt\n".to_string(),
            }
        );
        assert_eq!(
            msgs[1],
            ServerMsg::Done {
                id: "cmd-1".to_string(),
                exit_code: Some(0),
            }
        );
    }

    // ── OS command: stderr only (e.g. validation failure) ────────────────────

    #[tokio::test]
    async fn os_command_stderr_only_produces_chunk_then_done() {
        let msgs = run(
            "cmd-2",
            "$ badcmd",
            CommandKind::Auto,
            None,
            FakeToolClient::failure("command not found\n", 127),
        )
        .await;

        assert_eq!(
            msgs.len(),
            2,
            "expected [Chunk(stderr), Done], got {msgs:?}"
        );

        assert_eq!(
            msgs[0],
            ServerMsg::Chunk {
                id: "cmd-2".to_string(),
                content: "command not found\n".to_string(),
            }
        );
        assert_eq!(
            msgs[1],
            ServerMsg::Done {
                id: "cmd-2".to_string(),
                exit_code: Some(127),
            }
        );
    }

    // ── OS command: both stdout and stderr ───────────────────────────────────

    #[tokio::test]
    async fn os_command_stdout_and_stderr_produces_two_chunks_then_done() {
        let msgs = run(
            "cmd-3",
            "git status",
            CommandKind::Auto,
            None,
            FakeToolClient::with_output("On branch main\n", "warning: ...\n", 0),
        )
        .await;

        assert_eq!(
            msgs.len(),
            3,
            "expected [Chunk(stdout), Chunk(stderr), Done], got {msgs:?}"
        );

        assert_eq!(
            msgs[0],
            ServerMsg::Chunk {
                id: "cmd-3".to_string(),
                content: "On branch main\n".to_string(),
            }
        );
        assert_eq!(
            msgs[1],
            ServerMsg::Chunk {
                id: "cmd-3".to_string(),
                content: "warning: ...\n".to_string(),
            }
        );
        assert_eq!(
            msgs[2],
            ServerMsg::Done {
                id: "cmd-3".to_string(),
                exit_code: Some(0),
            }
        );
    }

    // ── OS command: no output at all ─────────────────────────────────────────

    #[tokio::test]
    async fn os_command_no_output_produces_only_done() {
        let msgs = run(
            "cmd-4",
            "cls",
            CommandKind::Os,
            None,
            FakeToolClient::with_output("", "", 0),
        )
        .await;

        assert_eq!(msgs.len(), 1, "expected [Done] only, got {msgs:?}");
        assert_eq!(
            msgs[0],
            ServerMsg::Done {
                id: "cmd-4".to_string(),
                exit_code: Some(0),
            }
        );
    }

    // ── OS command: failure (exit -1) ─────────────────────────────────────────

    #[tokio::test]
    async fn os_command_failure_carries_exit_minus_one() {
        let msgs = run(
            "cmd-5",
            "$ ls",
            CommandKind::Auto,
            None,
            FakeToolClient::failure("spawn failed", -1),
        )
        .await;

        assert_eq!(msgs.len(), 2);
        assert_eq!(
            msgs[1],
            ServerMsg::Done {
                id: "cmd-5".to_string(),
                exit_code: Some(-1),
            }
        );
    }

    // ── OS command: dollar-prefix stripped before reaching tool ───────────────

    /// This test verifies that "$ dir" → the *tool* receives "dir" (not "$ dir").
    /// We can only verify the *response shape* here (FakeToolClient ignores the
    /// command string), but the strip is tested directly in strip_dollar_prefix
    /// unit tests above.
    #[tokio::test]
    async fn os_command_dollar_prefix_auto_routes_to_os() {
        let msgs = run(
            "cmd-6",
            "$ dir",
            CommandKind::Auto,
            None,
            FakeToolClient::success("result\n"),
        )
        .await;

        // "$ dir" auto-routes to Os → we get Chunk + Done (not NL stub).
        assert!(
            matches!(
                &msgs[msgs.len() - 1],
                ServerMsg::Done {
                    exit_code: Some(0),
                    ..
                }
            ),
            "expected Done{{exit_code:0}}, got {msgs:?}"
        );
        // No stub AI prefix in output.
        for msg in &msgs {
            if let ServerMsg::Chunk { content, .. } = msg {
                assert!(
                    !content.contains("[stub AI]"),
                    "Os route must not produce stub AI output: {content:?}"
                );
            }
        }
    }

    // ── NL command ────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn nl_command_produces_chunk_with_stub_then_done_none() {
        let msgs = run(
            "cmd-7",
            "What is Rust?",
            CommandKind::Nl,
            None,
            FakeToolClient::success("unreachable"),
        )
        .await;

        assert_eq!(msgs.len(), 2, "expected [Chunk, Done], got {msgs:?}");

        assert_eq!(
            msgs[0],
            ServerMsg::Chunk {
                id: "cmd-7".to_string(),
                content: "[stub AI] ricevuto: What is Rust?".to_string(),
            }
        );
        assert_eq!(
            msgs[1],
            ServerMsg::Done {
                id: "cmd-7".to_string(),
                exit_code: None, // AI path has no exit code.
            }
        );
    }

    #[tokio::test]
    async fn nl_auto_route_on_sentence_uses_stub() {
        let msgs = run(
            "cmd-8",
            "explain async/await",
            CommandKind::Auto,
            None,
            FakeToolClient::success("unreachable"),
        )
        .await;

        // "explain" is not in SHELL_TOKENS, so Auto → Nl.
        assert_eq!(
            msgs[0],
            ServerMsg::Chunk {
                id: "cmd-8".to_string(),
                content: "[stub AI] ricevuto: explain async/await".to_string(),
            }
        );
        assert_eq!(
            msgs[1],
            ServerMsg::Done {
                id: "cmd-8".to_string(),
                exit_code: None,
            }
        );
    }

    // ── cwd parameter is accepted but no longer forwarded (ADR-011) ────────────
    // cwd is now session state; users run `cd` to change it.
    // This test verifies handle_command doesn't panic when cwd is provided.

    #[tokio::test]
    async fn cwd_param_is_accepted_without_panic() {
        // With persistent sessions, cwd is session state, not a per-call param.
        // We still accept the parameter in the API for backward compat with ws.rs.
        let msgs = run(
            "cmd-9",
            "dir",
            CommandKind::Os,
            Some("C:\\Users"),
            FakeToolClient::success("output"),
        )
        .await;

        assert!(
            matches!(
                &msgs[msgs.len() - 1],
                ServerMsg::Done {
                    exit_code: Some(0),
                    ..
                }
            ),
            "expected Done{{0}}, got {msgs:?}"
        );
    }

    // ── Slash commands (ADR-012) ──────────────────────────────────────────────
    //
    // TDD: questi test sono stati scritti PRIMA di aggiungere `Route::Slash`
    // e `handle_slash` a questo modulo.
    //
    // RED (pre-implementazione): il codice non compilava perché
    //   - `Route::Slash` era definito in router.rs ma non coperto nel match
    //     in core.rs → errore E0004 "non-exhaustive patterns: Route::Slash not covered"
    //
    // GREEN: dopo l'aggiunta di `handle_slash` e del relativo arm nel match.

    #[tokio::test]
    async fn slash_open_found_produces_chunk_and_done_zero() {
        // FakeToolClient.open_target("/tmp/test") → ok=true (non contiene "nonexistent")
        let msgs = run(
            "open-1",
            "/open /tmp/test",
            CommandKind::Auto,
            None,
            FakeToolClient::success(""),
        )
        .await;

        assert_eq!(msgs.len(), 2, "expected [Chunk, Done], got {msgs:?}");
        assert!(
            matches!(&msgs[0], ServerMsg::Chunk { id, .. } if id == "open-1"),
            "first msg should be Chunk: {msgs:?}"
        );
        assert_eq!(
            msgs[1],
            ServerMsg::Done {
                id: "open-1".to_string(),
                exit_code: Some(0),
            },
            "expected Done{{0}} for successful open: {msgs:?}"
        );
    }

    #[tokio::test]
    async fn slash_open_not_found_produces_chunk_and_done_one() {
        // FakeToolClient.open_target("nonexistent/target") → ok=false
        let msgs = run(
            "open-2",
            "/open /percorso/nonexistent/target",
            CommandKind::Auto,
            None,
            FakeToolClient::success(""),
        )
        .await;

        assert_eq!(msgs.len(), 2, "expected [Chunk, Done], got {msgs:?}");
        assert_eq!(
            msgs[1],
            ServerMsg::Done {
                id: "open-2".to_string(),
                exit_code: Some(1),
            },
            "expected Done{{1}} for not-found target: {msgs:?}"
        );
        // Il messaggio del Chunk deve contenere info sull'errore.
        if let ServerMsg::Chunk { content, .. } = &msgs[0] {
            assert!(
                content.contains("non trovato"),
                "expected 'non trovato' in chunk content: {content:?}"
            );
        }
    }

    #[tokio::test]
    async fn slash_reset_produces_chunk_and_done_zero() {
        let msgs = run(
            "reset-1",
            "/reset",
            CommandKind::Auto,
            None,
            FakeToolClient::success(""),
        )
        .await;

        assert_eq!(msgs.len(), 2, "expected [Chunk, Done], got {msgs:?}");
        assert_eq!(
            msgs[0],
            ServerMsg::Chunk {
                id: "reset-1".to_string(),
                content: "sessione riavviata".to_string(),
            }
        );
        assert_eq!(
            msgs[1],
            ServerMsg::Done {
                id: "reset-1".to_string(),
                exit_code: Some(0),
            }
        );
    }

    #[tokio::test]
    async fn slash_unknown_produces_routing_error() {
        use protocol::ErrCode;

        let msgs = run(
            "slash-err",
            "/sconosciuto",
            CommandKind::Auto,
            None,
            FakeToolClient::success(""),
        )
        .await;

        assert_eq!(msgs.len(), 1, "expected [Error], got {msgs:?}");
        assert_eq!(
            msgs[0],
            ServerMsg::Error {
                id: "slash-err".to_string(),
                code: ErrCode::RoutingError,
                message: "comando slash sconosciuto: /sconosciuto".to_string(),
            }
        );
    }

    #[tokio::test]
    async fn slash_config_produces_routing_error_from_backend() {
        // /config is UI-local (handled by the frontend before sending to backend).
        // If it somehow reaches the backend, it is treated as an unknown slash.
        use protocol::ErrCode;

        let msgs = run(
            "config-1",
            "/config",
            CommandKind::Auto,
            None,
            FakeToolClient::success(""),
        )
        .await;

        assert_eq!(msgs.len(), 1, "expected [Error], got {msgs:?}");
        assert!(
            matches!(
                &msgs[0],
                ServerMsg::Error {
                    code: ErrCode::RoutingError,
                    ..
                }
            ),
            "expected RoutingError for /config reaching backend: {msgs:?}"
        );
    }

    #[tokio::test]
    async fn slash_with_whitespace_prefix_routes_to_slash() {
        // Trim before `/` check → leading spaces still produce Slash route.
        let msgs = run(
            "ws-1",
            "  /reset",
            CommandKind::Auto,
            None,
            FakeToolClient::success(""),
        )
        .await;

        assert_eq!(msgs.len(), 2, "expected [Chunk, Done] for /reset: {msgs:?}");
        assert_eq!(
            msgs[0],
            ServerMsg::Chunk {
                id: "ws-1".to_string(),
                content: "sessione riavviata".to_string(),
            }
        );
    }

    // ── /show slash command (ADR-013, Superficie 3) ───────────────────────────
    //
    // TDD RED: these tests were written BEFORE adding the `"show"` arm to
    // `handle_slash`.  Running `cargo test -p orchestrator` at this point
    // produces failures (wrong message type returned).
    //
    // Design decision on empty `/show`: return Error{RoutingError} with a
    // human-readable message ("nessun contenuto da mostrare").  Rationale:
    // an empty `/show` is a usage error, not a valid empty window.

    #[tokio::test]
    async fn slash_show_markdown_produces_open_window() {
        use protocol::WindowKind;

        // `/show # Ciao\n- a` → [OpenWindow, Done{exit_code: Some(0)}].
        let msgs = run(
            "show-1",
            "/show # Ciao\n- a",
            CommandKind::Auto,
            None,
            FakeToolClient::success(""),
        )
        .await;

        assert_eq!(
            msgs.len(),
            2,
            "expected [OpenWindow, Done], got {msgs:?}"
        );
        assert!(
            matches!(
                &msgs[0],
                ServerMsg::OpenWindow {
                    kind: WindowKind::Markdown,
                    ..
                }
            ),
            "expected OpenWindow{{Markdown}} at index 0, got {msgs:?}"
        );

        // Verify content is preserved verbatim.
        if let ServerMsg::OpenWindow { content, .. } = &msgs[0] {
            assert_eq!(content, "# Ciao\n- a", "content mismatch: {content:?}");
        }

        assert_eq!(
            msgs[1],
            ServerMsg::Done {
                id: "show-1".to_string(),
                exit_code: Some(0),
            },
            "expected Done{{exit_code: Some(0)}} at index 1, got {msgs:?}"
        );
    }

    #[tokio::test]
    async fn slash_show_title_from_first_line() {
        use protocol::WindowKind;

        // Title = first non-empty line (stripped of leading '#' spaces).
        let msgs = run(
            "show-2",
            "/show # My Title\nsome body",
            CommandKind::Auto,
            None,
            FakeToolClient::success(""),
        )
        .await;

        assert_eq!(msgs.len(), 2, "expected [OpenWindow, Done], got {msgs:?}");
        if let ServerMsg::OpenWindow {
            title,
            kind,
            content,
        } = &msgs[0]
        {
            assert_eq!(kind, &WindowKind::Markdown);
            // Title is derived from first line.  The exact trimming is our choice;
            // at minimum it must not be the default and must contain "My Title".
            assert!(
                title.contains("My Title"),
                "expected title to contain 'My Title', got {title:?}"
            );
            assert_eq!(content, "# My Title\nsome body");
        } else {
            panic!("expected OpenWindow at index 0, got {msgs:?}");
        }

        assert_eq!(
            msgs[1],
            ServerMsg::Done {
                id: "show-2".to_string(),
                exit_code: Some(0),
            },
            "expected Done{{exit_code: Some(0)}} at index 1, got {msgs:?}"
        );
    }

    #[tokio::test]
    async fn slash_show_long_first_line_truncated_to_60_chars() {
        use protocol::WindowKind;

        // First line longer than 60 chars must be truncated.
        let long_line = "A".repeat(100);
        let input = format!("/show {long_line}");
        let msgs = run(
            "show-3",
            &input,
            CommandKind::Auto,
            None,
            FakeToolClient::success(""),
        )
        .await;

        assert_eq!(msgs.len(), 2, "expected [OpenWindow, Done], got {msgs:?}");
        if let ServerMsg::OpenWindow { title, kind, .. } = &msgs[0] {
            assert_eq!(kind, &WindowKind::Markdown);
            assert!(
                title.chars().count() <= 60,
                "title must be ≤60 chars, got {} chars: {title:?}",
                title.chars().count()
            );
        } else {
            panic!("expected OpenWindow at index 0, got {msgs:?}");
        }

        assert_eq!(
            msgs[1],
            ServerMsg::Done {
                id: "show-3".to_string(),
                exit_code: Some(0),
            },
            "expected Done{{exit_code: Some(0)}} at index 1, got {msgs:?}"
        );
    }

    #[tokio::test]
    async fn slash_show_default_title_when_content_empty() {
        // Design decision: `/show` with no content → Error (usage error, not empty window).
        // The user typed `/show` without anything to show.
        use protocol::ErrCode;

        let msgs = run(
            "show-empty",
            "/show",
            CommandKind::Auto,
            None,
            FakeToolClient::success(""),
        )
        .await;

        assert_eq!(msgs.len(), 1, "expected [Error], got {msgs:?}");
        assert!(
            matches!(
                &msgs[0],
                ServerMsg::Error {
                    code: ErrCode::RoutingError,
                    ..
                }
            ),
            "expected RoutingError for empty /show, got {msgs:?}"
        );
    }

    #[tokio::test]
    async fn slash_show_whitespace_only_content_is_error() {
        // `/show   ` (only spaces) → same as empty.
        use protocol::ErrCode;

        let msgs = run(
            "show-ws",
            "/show   ",
            CommandKind::Auto,
            None,
            FakeToolClient::success(""),
        )
        .await;

        assert_eq!(msgs.len(), 1, "expected [Error], got {msgs:?}");
        assert!(
            matches!(
                &msgs[0],
                ServerMsg::Error {
                    code: ErrCode::RoutingError,
                    ..
                }
            ),
            "expected RoutingError for whitespace-only /show, got {msgs:?}"
        );
    }

    // ── /nowin command ───────────────────────────────────────────────────────
    //
    // TDD RED: questi test sono scritti PRIMA di aggiungere `strip_nowin_prefix`
    // e l'intercetto in `handle_command`.
    // Pre-implementazione: "/nowin ..." inizia con "/" → Route::Slash →
    // comando sconosciuto → Error{RoutingError}.
    // Le asserzioni sotto falliscono perché la feature manca.

    #[tokio::test]
    async fn nowin_routes_to_ai_plain() {
        // `/nowin <prompt>` deve instradare al path NL con il prompt PULITO
        // (senza direttiva iniettata). StubAdapter echeggia l'input ricevuto:
        // il Chunk deve contenere il prompt originale e NON "show_markdown".
        let msgs = run(
            "n1",
            "/nowin raccontami una storia",
            CommandKind::Auto,
            None,
            FakeToolClient::success(""),
        )
        .await;

        // Non deve essere un Error di routing.
        assert!(
            !matches!(&msgs[0], ServerMsg::Error { .. }),
            "expected NL path (no Error), got {msgs:?}"
        );

        // Deve esserci un Chunk che contiene il prompt pulito.
        let found_chunk = msgs.iter().any(|m| {
            if let ServerMsg::Chunk { content, .. } = m {
                content.contains("raccontami una storia")
            } else {
                false
            }
        });
        assert!(
            found_chunk,
            "expected Chunk with clean prompt, got {msgs:?}"
        );

        // Ultimo messaggio è Done{exit_code: None} (path NL, non OS/Slash).
        assert_eq!(
            msgs.last().unwrap(),
            &ServerMsg::Done {
                id: "n1".to_string(),
                exit_code: None,
            },
            "expected Done{{exit_code: None}} as last msg, got {msgs:?}"
        );
    }

    #[tokio::test]
    async fn nowin_empty_prompt_gives_hint() {
        // `/nowin` senza testo → Chunk con suggerimento + Done{exit_code: Some(1)}.
        let msgs = run(
            "n2",
            "/nowin",
            CommandKind::Auto,
            None,
            FakeToolClient::success(""),
        )
        .await;

        let has_hint = msgs.iter().any(|m| {
            if let ServerMsg::Chunk { content, .. } = m {
                content.contains("/nowin <richiesta>")
            } else {
                false
            }
        });
        assert!(
            has_hint,
            "expected Chunk with '/nowin <richiesta>' hint, got {msgs:?}"
        );

        assert_eq!(
            msgs.last().unwrap(),
            &ServerMsg::Done {
                id: "n2".to_string(),
                exit_code: Some(1),
            },
            "expected Done{{exit_code: Some(1)}} as last msg, got {msgs:?}"
        );
    }

    #[tokio::test]
    async fn nowin_case_insensitive() {
        // `/NoWin ciao` deve prendere il path NL, non un Error.
        let msgs = run(
            "n3",
            "/NoWin ciao",
            CommandKind::Auto,
            None,
            FakeToolClient::success(""),
        )
        .await;

        // Non deve essere un Error di routing.
        assert!(
            !matches!(&msgs[0], ServerMsg::Error { .. }),
            "expected NL path for '/NoWin ciao', got {msgs:?}"
        );

        // Deve contenere un Chunk che include "ciao".
        let has_ciao = msgs.iter().any(|m| {
            if let ServerMsg::Chunk { content, .. } = m {
                content.contains("ciao")
            } else {
                false
            }
        });
        assert!(has_ciao, "expected Chunk containing 'ciao', got {msgs:?}");
    }

    /// Wiring end-to-end: `/nowin` deve far arrivare al modello la richiesta
    /// SENZA `show_markdown` (2 tool), mentre una NL normale la conserva (3 tool).
    /// Usa `LlmAdapter`+`ClaudeBackend`+`FakeMessagesClient` per ispezionare la richiesta reale
    /// (lo `StubAdapter` del helper `run` ignorerebbe il flag `allow_windows`).
    #[tokio::test]
    async fn nowin_excludes_show_markdown_through_handle_command() {
        use crate::ai_adapter::LlmAdapter;
        use crate::claude_backend::ClaudeBackend;
        use crate::messages_client::{
            Block, ConversationHistory, FakeMessagesClient, MessagesResponse,
        };

        fn ok_text() -> MessagesResponse {
            MessagesResponse {
                content: vec![Block::Text {
                    text: "ok".to_string(),
                }],
                stop_reason: Some("end_turn".to_string()),
                stop_details: None,
            }
        }

        let tools = FakeToolClient::success("");

        // `/nowin` → la richiesta deve avere 2 tool (niente show_markdown).
        let fake_nowin = std::sync::Arc::new(FakeMessagesClient::ok(ok_text()));
        let backend_nowin = std::sync::Arc::new(ClaudeBackend::new(
            fake_nowin.clone(), "claude-sonnet-4-6".to_string(), 16000,
        ));
        let ai_nowin = LlmAdapter::new(backend_nowin, "claude-sonnet-4-6".to_string(), std::path::PathBuf::from("/test-config"));
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut hist = ConversationHistory::new();
        crate::test_support::collect(|tx| {
            handle_command(
                "n1",
                "/nowin storia lunga",
                CommandKind::Nl,
                None,
                &mut hist,
                &ai_nowin,
                &tools,
                None,
                None,
                false,
                None,
                tmp.path(),
                None,
                None,
                tx,
            )
        })
        .await;
        let req = fake_nowin.recorded();
        // 8 tool storici - show_markdown (era 7 prima di set_ai_display_name, Task 7).
        assert_eq!(req.tools.len(), 7, "/nowin: attesi 7 tool, got {:?}", req.tools);
        assert!(
            !req.tools.iter().any(|t| t.name() == "show_markdown"),
            "/nowin: show_markdown deve essere escluso, got {:?}",
            req.tools
        );

        // NL normale → la richiesta conserva tutti e 8 i tool (show_markdown incluso).
        let fake_norm = std::sync::Arc::new(FakeMessagesClient::ok(ok_text()));
        let backend_norm = std::sync::Arc::new(ClaudeBackend::new(
            fake_norm.clone(), "claude-sonnet-4-6".to_string(), 16000,
        ));
        let ai_norm = LlmAdapter::new(backend_norm, "claude-sonnet-4-6".to_string(), std::path::PathBuf::from("/test-config"));
        let mut hist2 = ConversationHistory::new();
        crate::test_support::collect(|tx| {
            handle_command(
                "n2",
                "raccontami una storia",
                CommandKind::Nl,
                None,
                &mut hist2,
                &ai_norm,
                &tools,
                None,
                None,
                false,
                None,
                tmp.path(),
                None,
                None,
                tx,
            )
        })
        .await;
        let req2 = fake_norm.recorded();
        assert_eq!(req2.tools.len(), 8, "NL normale: attesi 8 tool, got {:?}", req2.tools);
        assert!(
            req2.tools.iter().any(|t| t.name() == "show_markdown"),
            "NL normale: show_markdown deve essere presente, got {:?}",
            req2.tools
        );
    }

    // ── /help command (0.10.0) ───────────────────────────────────────────────
    //
    // TDD RED: questo test è scritto PRIMA di aggiungere il ramo "help" in
    // handle_slash e la costante HELP_MARKDOWN. Prima della feature, "/help"
    // produce Error{RoutingError, "comando slash sconosciuto: /help"}.
    //
    // Design note: il test aspetta 2 messaggi [OpenWindow{Help}, Done{Some(0)}],
    // non 1. Questo DEVIA dalla specifica ("un solo OpenWindow") ma è NECESSARIO:
    // il frontend usa commandStarted/commandEnded per lo spinner; /help triggera
    // commandStarted → il Done è OBBLIGATORIO per chiudere lo spinner.
    // Pattern precedente: /show fa lo stesso (orchestrator 0.8.0, "evita spinner
    // UI bloccato"). Non emettere Done sarebbe la stessa regressione.

    #[tokio::test]
    async fn slash_help_produces_open_window_and_done() {
        use protocol::WindowKind;

        let msgs = run(
            "help-1",
            "/help",
            CommandKind::Auto,
            None,
            FakeToolClient::success(""),
        )
        .await;

        // Must be exactly [OpenWindow{Help}, Done{Some(0)}].
        assert_eq!(
            msgs.len(),
            2,
            "expected [OpenWindow{{Help}}, Done{{0}}], got {msgs:?}"
        );

        // First message: OpenWindow with kind=Help.
        assert!(
            matches!(
                &msgs[0],
                ServerMsg::OpenWindow {
                    kind: WindowKind::Help,
                    ..
                }
            ),
            "expected OpenWindow{{Help}} at index 0, got {msgs:?}"
        );

        // Title must contain "Comandi" (Italian: "Commands").
        if let ServerMsg::OpenWindow { title, .. } = &msgs[0] {
            assert!(
                title.contains("Comandi"),
                "title should contain 'Comandi', got {title:?}"
            );
        }

        // Content must be non-empty and contain "/config" (verifies it's real help content).
        if let ServerMsg::OpenWindow { content, .. } = &msgs[0] {
            assert!(!content.is_empty(), "content must not be empty");
            assert!(
                content.contains("/config"),
                "content should contain '/config', got {content:?}"
            );
            // Comandi core aggiunti dopo la scrittura originale di HELP_MARKDOWN
            // (v0.10.0) e mai riportati qui — /aichat (app.js, UI-local) e i tre
            // canali tool esterni cursor-typeable (external-channels.js /
            // EXTERNAL_TOOL_CHANNELS): /markets, /nmap, /pyping. NON include i
            // comandi dei plugin (es. /calc, /lc, /crypto — scoperti a runtime
            // dal manifest, non fanno parte di questa costante statica) né
            // `/library-expand` (deliberatamente non digitabile da cursore).
            for cmd in ["/aichat", "/markets", "/nmap", "/pyping"] {
                assert!(
                    content.contains(cmd),
                    "content should contain '{cmd}', got {content:?}"
                );
            }
        }

        // Second message: Done{exit_code: Some(0)} — closes the spinner.
        assert_eq!(
            msgs[1],
            ServerMsg::Done {
                id: "help-1".to_string(),
                exit_code: Some(0),
            },
            "expected Done{{exit_code: Some(0)}} at index 1, got {msgs:?}"
        );
    }

    // ── /web command (Task 3) ─────────────────────────────────────────────────
    //
    // TDD RED: questi test sono scritti PRIMA di aggiungere il ramo "web" in
    // handle_slash e percent_encode_query. Prima della feature, "/web gatti buffi"
    // produce Error{RoutingError, "comando slash sconosciuto: /web"}.

    #[tokio::test]
    async fn slash_web_opens_browser_search() {
        let msgs = run(
            "w1",
            "/web gatti buffi",
            CommandKind::Auto,
            None,
            FakeToolClient::success(""),
        )
        .await;
        // open_target chiamato → Chunk + Done{0}; FakeToolClient.open_target
        // ritorna ok=true per target non-"nonexistent".
        assert!(
            matches!(msgs.last().unwrap(), ServerMsg::Done { exit_code: Some(0), .. }),
            "expected Done{{exit_code: Some(0)}} as last msg, got {msgs:?}"
        );
        assert!(
            matches!(&msgs[0], ServerMsg::Chunk { .. }),
            "expected Chunk at index 0, got {msgs:?}"
        );
    }

    #[tokio::test]
    async fn slash_web_empty_is_error() {
        use protocol::ErrCode;

        let msgs = run(
            "w2",
            "/web",
            CommandKind::Auto,
            None,
            FakeToolClient::success(""),
        )
        .await;
        assert!(
            matches!(&msgs[0], ServerMsg::Error { code: ErrCode::RoutingError, .. }),
            "expected RoutingError for empty /web, got {msgs:?}"
        );
    }

    #[test]
    fn percent_encode_query_basic() {
        assert_eq!(percent_encode_query("a b"), "a%20b");
        assert_eq!(percent_encode_query("c++"), "c%2B%2B");
        assert_eq!(percent_encode_query("ciao"), "ciao");
    }

    // ── resolve_open_target (Task 4 — cwd reale) ────────────────────────────
    //
    // TDD RED: these tests are written BEFORE `resolve_open_target` exists.
    // Running `cargo test -p orchestrator` at this point will produce a compile
    // error: E0425 (cannot find function `resolve_open_target`).
    //
    // The function is a *pure* helper: no async, no I/O, no trait objects.
    // It resolves a relative path against the tracked cwd so that
    // `tools.open_target` always receives an absolute path or a URL —
    // never a bare relative path that the OS would resolve against the
    // process cwd (which may differ from the user's shell cwd).

    #[test]
    fn resolve_open_target_url_https_is_unchanged() {
        let result = resolve_open_target("https://example.com/page", "/home/user");
        assert_eq!(result, "https://example.com/page");
    }

    #[test]
    fn resolve_open_target_url_http_is_unchanged() {
        let result = resolve_open_target("http://localhost:8080/api", "/home/user");
        assert_eq!(result, "http://localhost:8080/api");
    }

    #[cfg(windows)]
    #[test]
    fn resolve_open_target_absolute_windows_path_is_unchanged() {
        let result = resolve_open_target(r"C:\Users\x\doc.pdf", r"C:\Users\x");
        assert_eq!(result, r"C:\Users\x\doc.pdf");
    }

    #[cfg(not(windows))]
    #[test]
    fn resolve_open_target_absolute_unix_path_is_unchanged() {
        let result = resolve_open_target("/etc/hosts", "/home/user");
        assert_eq!(result, "/etc/hosts");
    }

    #[cfg(windows)]
    #[test]
    fn resolve_open_target_relative_resolved_against_cwd_windows() {
        let result = resolve_open_target("fattura.pdf", r"C:\Users\x\Docs");
        // Path::new(cwd).join("fattura.pdf") on Windows
        assert_eq!(result, r"C:\Users\x\Docs\fattura.pdf");
    }

    #[cfg(not(windows))]
    #[test]
    fn resolve_open_target_relative_resolved_against_cwd_unix() {
        let result = resolve_open_target("fattura.pdf", "/home/user/docs");
        assert_eq!(result, "/home/user/docs/fattura.pdf");
    }

    #[cfg(not(windows))]
    #[test]
    fn resolve_open_target_sub_path_relative_resolved_against_cwd() {
        let result = resolve_open_target("sub/x.txt", "/home/user");
        assert_eq!(result, "/home/user/sub/x.txt");
    }

    #[cfg(windows)]
    #[test]
    fn resolve_open_target_sub_path_relative_resolved_against_cwd_windows() {
        let result = resolve_open_target(r"sub\x.txt", r"C:\Users\x");
        assert_eq!(result, r"C:\Users\x\sub\x.txt");
    }

    #[tokio::test]
    async fn web_search_flag_threads_to_request() {
        use crate::ai_adapter::LlmAdapter;
        use crate::claude_backend::ClaudeBackend;
        use crate::messages_client::{Block, ConversationHistory, FakeMessagesClient, MessagesResponse};

        let resp = MessagesResponse {
            content: vec![Block::Text { text: "ok".to_string() }],
            stop_reason: Some("end_turn".to_string()),
            stop_details: None,
        };
        let fake = std::sync::Arc::new(FakeMessagesClient::ok(resp));
        let backend = std::sync::Arc::new(ClaudeBackend::new(
            fake.clone(), "claude-sonnet-4-6".to_string(), 16000,
        ));
        let ai = LlmAdapter::new(backend, "claude-sonnet-4-6".to_string(), std::path::PathBuf::from("/test-config"));
        let tools = FakeToolClient::success("");
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut hist = ConversationHistory::new();
        crate::test_support::collect(|tx| {
            handle_command(
                "n1",
                "che ore sono a Tokyo",
                CommandKind::Nl,
                None,
                &mut hist,
                &ai,
                &tools,
                None,
                None,
                true,
                None,
                tmp.path(),
                None,
                None,
                tx,
            )
        })
        .await;
        let req = fake.recorded();
        assert!(
            req.tools.iter().any(|t| t.name() == "web_search"),
            "web_search assente nei tool della richiesta: {:?}",
            req.tools
        );
    }

    #[tokio::test]
    async fn slash_show_default_title_when_first_line_empty() {
        use protocol::WindowKind;

        // If content starts with empty lines, fall through to default title.
        let msgs = run(
            "show-4",
            "/show \n\nbody text here",
            CommandKind::Auto,
            None,
            FakeToolClient::success(""),
        )
        .await;

        assert_eq!(msgs.len(), 2, "expected [OpenWindow, Done], got {msgs:?}");
        if let ServerMsg::OpenWindow { title, kind, .. } = &msgs[0] {
            assert_eq!(kind, &WindowKind::Markdown);
            // When first non-empty line is missing (just whitespace before body),
            // title falls back to default.
            // In this input, content is "\n\nbody text here" and first non-empty
            // line is "body text here".
            assert!(!title.is_empty(), "title must not be empty: {title:?}");
        } else {
            panic!("expected OpenWindow at index 0, got {msgs:?}");
        }

        assert_eq!(
            msgs[1],
            ServerMsg::Done {
                id: "show-4".to_string(),
                exit_code: Some(0),
            },
            "expected Done{{exit_code: Some(0)}} at index 1, got {msgs:?}"
        );
    }

    // ── TDD RED: handle_command accetta Some(cancel) ──────────────────────────
    //
    // Questo test è scritto PRIMA di aggiungere il parametro `cancel` a
    // `handle_command`. Prima della modifica la firma ha 10 argomenti e il test
    // non compila (numero argomenti errato) → compile error = fase RED.
    // Dopo l'aggiunta il test deve passare senza panica.

    /// Verifica che handle_command accetti Some(CancellationToken) senza panica.
    /// Il token NON viene cancellato — vogliamo solo che il parametro passi correttamente
    /// attraverso handle_command → handle_nl → ai.respond (StubAdapter ignora il token).
    #[tokio::test]
    async fn handle_command_cancel_param_accepted_without_panic() {
        use tokio_util::sync::CancellationToken;

        let token = CancellationToken::new();
        // NON cancellare il token: vogliamo il path normale con None→Some propagato.
        let ai = StubAdapter;
        let tools = FakeToolClient::success("ok");
        let mut history = crate::messages_client::ConversationHistory::new();
        let tmp = tempfile::tempdir().expect("tempdir");
        let msgs = crate::test_support::collect(|tx| {
            handle_command(
                "cancel-test",
                "test input",
                CommandKind::Nl,
                None,
                &mut history,
                &ai,
                &tools,
                None,
                None,
                false,
                None,
                tmp.path(),
                None,
                Some(token),
                tx,
            )
        })
        .await;
        // Se non panica e Done arriva, il parametro è stato accettato.
        assert!(
            msgs.iter().any(|m| matches!(m, ServerMsg::Done { .. })),
            "handle_command con Some(cancel) deve terminare con Done: {msgs:?}"
        );
    }

    // ── Isolamento per-canale: le vie Os/Slash restano bloccate (Task 3, ────
    // Docs/superpowers/specs/2026-07-16-tool-isolation-design.md) ───────────
    //
    // Questi test NON toccano `core.rs`: dimostrano che `FixtureChannelToolClient`
    // (che non delega a una shell/target reale) blocca `Route::Os`/`Route::Slash`
    // da solo, perché quelle vie chiamano `tools.run_in_session`/
    // `tools.open_target` direttamente — senza che `handle_os`/`handle_slash`
    // sappiano nulla di "canali".

    #[tokio::test]
    async fn channel_tool_client_blocks_os_route_shell_execution() {
        let ai = StubAdapter;
        let tools = FixtureChannelToolClient;
        let mut history = crate::messages_client::ConversationHistory::new();
        let tmp = tempfile::tempdir().expect("tempdir");
        let msgs = crate::test_support::collect(|tx| {
            handle_command(
                "chan-os-1", "dir", CommandKind::Os, None,
                &mut history, &ai, &tools, None, None, false, None, tmp.path(), None, None, tx,
            )
        })
        .await;

        assert_eq!(msgs.len(), 2, "expected [Chunk(stderr), Done], got {msgs:?}");
        assert!(
            matches!(&msgs[0], ServerMsg::Chunk { content, .. } if content.contains("non disponibile su questo canale")),
            "expected the channel's stub message, got {msgs:?}"
        );
        assert_eq!(
            msgs[1],
            ServerMsg::Done { id: "chan-os-1".to_string(), exit_code: Some(-1) },
            "expected Done{{-1}} — nessuna shell reale è mai stata eseguita: {msgs:?}"
        );
    }

    #[tokio::test]
    async fn channel_tool_client_blocks_open_target_slash() {
        let ai = StubAdapter;
        let tools = FixtureChannelToolClient;
        let mut history = crate::messages_client::ConversationHistory::new();
        let tmp = tempfile::tempdir().expect("tempdir");
        let msgs = crate::test_support::collect(|tx| {
            handle_command(
                "chan-open-1", "/open C:\\Users", CommandKind::Auto, None,
                &mut history, &ai, &tools, None, None, false, None, tmp.path(), None, None, tx,
            )
        })
        .await;

        assert_eq!(msgs.len(), 2, "expected [Chunk, Done], got {msgs:?}");
        assert!(
            matches!(&msgs[0], ServerMsg::Chunk { content, .. } if content.contains("non disponibile su questo canale")),
            "expected the channel's stub message, got {msgs:?}"
        );
        assert_eq!(
            msgs[1],
            ServerMsg::Done { id: "chan-open-1".to_string(), exit_code: Some(1) },
            "expected Done{{1}} (not-ok open) — nessun target reale è mai stato aperto: {msgs:?}"
        );
    }

    /// `KNOWN_BACKEND_SLASHES` è l'UNICA lista dei comandi che `handle_slash`
    /// dispaccia (esclusi `reset`, gestito a parte dalla shell): ognuno deve
    /// davvero rispondere senza `Error{RoutingError, "sconosciuto"}`.
    #[tokio::test]
    async fn known_backend_slashes_are_all_dispatched_by_handle_slash() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let help_dir = tmp.path().join("help");
        let _ = std::fs::create_dir_all(&help_dir);
        let _ = std::fs::write(help_dir.join("it.md"), "# Lare \u{2014} Comandi\nTest IT");
        let tools = FakeToolClient::success("ok");
        for cmd in KNOWN_BACKEND_SLASHES {
            let input = match *cmd {
                "show" => "/show # titolo".to_string(),
                "open" => "/open C:\\x".to_string(),
                "web" => "/web gatti".to_string(),
                other => format!("/{other}"),
            };
            let out = handle_slash("id", &input, &tools, "", None, tmp.path()).await;
            let unknown = out.iter().any(|m| matches!(m, ServerMsg::Error { message, .. } if message.contains("sconosciuto")));
            assert!(!unknown, "/{cmd} risulta sconosciuto a handle_slash: {out:?}");
        }
    }

    #[tokio::test]
    async fn slash_help_respects_language_directive() {
        use protocol::WindowKind;

        let tmp = tempfile::tempdir().expect("tempdir");
        let help_dir = tmp.path().join("help");
        std::fs::create_dir_all(&help_dir).expect("create_dir_all");
        std::fs::write(
            help_dir.join("it.md"),
            "# Lare \u{2014} Comandi\n\nTesto di aiuto in italiano con comandi.",
        )
        .expect("write it.md");
        std::fs::write(
            help_dir.join("en.md"),
            "# Lare \u{2014} Commands\n\nHelp text in English with commands.",
        )
        .expect("write en.md");

        async fn run_help_with_lang(lang: Option<&str>, config_dir: &std::path::Path) -> Vec<ServerMsg> {
            let ai = StubAdapter;
            let tools = FakeToolClient::success("");
            let mut history = crate::messages_client::ConversationHistory::new();
            crate::test_support::collect(|tx| {
                handle_command(
                    "help-test",
                    "/help",
                    CommandKind::Auto,
                    None,
                    &mut history,
                    &ai,
                    &tools,
                    None,
                    None,
                    false,
                    lang.map(str::to_string),
                    config_dir,
                    None,
                    None,
                    tx,
                )
            })
            .await
        }

        // 1. With lang: Some("en") -> English title and content, NOT Italian
        let msgs_en = run_help_with_lang(Some("en"), tmp.path()).await;
        assert_eq!(msgs_en.len(), 2);
        if let ServerMsg::OpenWindow { title, content, kind } = &msgs_en[0] {
            assert_eq!(*kind, WindowKind::Help);
            assert!(title.contains("Commands"), "title should contain 'Commands', got {title:?}");
            assert!(!title.contains("Comandi"), "title should NOT contain 'Comandi', got {title:?}");
            assert!(content.contains("Commands"), "content should contain 'Commands', got {content:?}");
            assert!(!content.contains("Comandi"), "content should NOT contain 'Comandi', got {content:?}");
        } else {
            panic!("expected OpenWindow at index 0, got {:?}", msgs_en[0]);
        }

        // 2. With lang: Some("it") -> Italian title and content, NOT English
        let msgs_it = run_help_with_lang(Some("it"), tmp.path()).await;
        assert_eq!(msgs_it.len(), 2);
        if let ServerMsg::OpenWindow { title, content, kind } = &msgs_it[0] {
            assert_eq!(*kind, WindowKind::Help);
            assert!(title.contains("Comandi"), "title should contain 'Comandi', got {title:?}");
            assert!(!title.contains("Commands"), "title should NOT contain 'Commands', got {title:?}");
            assert!(content.contains("Comandi"), "content should contain 'Comandi', got {content:?}");
            assert!(!content.contains("Commands"), "content should NOT contain 'Commands', got {content:?}");
        } else {
            panic!("expected OpenWindow at index 0, got {:?}", msgs_it[0]);
        }

        // 3. With lang: None -> default Italian
        let msgs_none = run_help_with_lang(None, tmp.path()).await;
        assert_eq!(msgs_none.len(), 2);
        if let ServerMsg::OpenWindow { title, content, kind } = &msgs_none[0] {
            assert_eq!(*kind, WindowKind::Help);
            assert!(title.contains("Comandi"), "title should contain 'Comandi', got {title:?}");
            assert!(!title.contains("Commands"), "title should NOT contain 'Commands', got {title:?}");
            assert!(content.contains("Comandi"), "content should contain 'Comandi', got {content:?}");
            assert!(!content.contains("Commands"), "content should NOT contain 'Commands', got {content:?}");
        } else {
            panic!("expected OpenWindow at index 0, got {:?}", msgs_none[0]);
        }
    }
}
