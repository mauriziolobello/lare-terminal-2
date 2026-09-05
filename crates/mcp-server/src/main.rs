//! # mcp-server — stdio entry point (v0.7.1)
//!
//! Thin MCP glue layer.  All business logic lives in [`mcp_server::session`]
//! and [`mcp_server::open_target`].
//!
//! ## What this file does
//! 1. Initialises tracing → **stderr** (stdout is the MCP wire; never log there).
//! 2. Creates a shared [`Session`] that is owned by the server process.
//! 3. Builds a `SessionServer` that exposes seven MCP tools:
//!    - `run_in_session(command)` — delegate to `Session::run`
//!    - `reset_session()` — delegate to `Session::reset`
//!    - `open_target(target)` — classify + open via `opener` (ADR-012)
//!    - `search_routines(query?)` — search the saved-routines repository (read-only)
//!    - `run_routine(name, args?)` — run a saved routine via `Session::run`
//!    - `get_routine_content(name)` — read full script body of a saved routine (read-only)
//!    - `save_routine(name, description, …, replace?)` — save a new or update existing routine
//! 4. Serves via `rmcp`'s stdio transport.
//!
//! ## Session lifetime
//! The session lives for the lifetime of the `mcp-server` process.  The shell
//! is lazily spawned on the first `run_in_session` call and auto-respawns if
//! the shell dies.  State (`cwd`, env, background processes) persists across
//! calls within a single `mcp-server` process lifetime.
//!
//! ## Streaming (v0.5.0)
//! When `progress_token` is present in the request params, each output line is
//! sent as a `notifications/progress` MCP notification to the client via
//! `Peer<RoleServer>::notify_progress()`.  The client (orchestrator) receives
//! these and converts them to `ServerMsg::Chunk` in real time.
//!
//! ## SRP
//! This file has one responsibility: wire the `session` and `open_target` modules
//! to the MCP protocol.  It contains no command-execution, shell, or opener logic.

use std::sync::Arc;

use anyhow::Result;
use mcp_server::open_target;
use mcp_server::session::Session;
use rmcp::{
    Peer, RoleServer,
    handler::server::wrapper::Parameters,
    model::{NumberOrString, ProgressNotificationParam, ProgressToken},
    schemars, tool, tool_router, transport::stdio, ServiceExt,
};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing_subscriber::{fmt, EnvFilter};

// ─── Parameter / response DTOs ────────────────────────────────────────────────

/// Input parameters for the `run_in_session` MCP tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct RunInSessionParams {
    /// The command string to execute in the persistent shell session.
    command: String,
    /// Internal: progress token for streaming output lines.
    ///
    /// When present, the server sends `notifications/progress` for each output
    /// line.  The `message` field of each notification carries the raw line.
    /// The orchestrator subscribes to the token before calling the tool and
    /// routes each notification to `ServerMsg::Chunk`.
    #[serde(default)]
    progress_token: Option<String>,
}

/// Structured output returned by `run_in_session`.
///
/// `stdout` contains combined stdout+stderr output.
/// `stderr` is always empty (stderr is merged into `stdout`).
/// `cwd` is the shell's working directory after the command completed.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct SessionOutput {
    stdout: String,
    stderr: String,
    exit_code: i32,
    /// Current working directory of the shell after the command.
    /// Empty string if the shell died before emitting the cwd marker.
    cwd: String,
}

/// Input parameters for the `open_target` MCP tool (ADR-012).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct OpenTargetParams {
    /// The target to open: `http(s)://…` URL, folder path, or file path.
    target: String,
}

/// Input parameters for the `search_routines` MCP tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SearchRoutinesParams {
    /// Optional search query matched case-insensitively against name,
    /// description, and tags. Omit or leave empty to list all routines.
    #[serde(default)]
    query: Option<String>,
}

/// One routine entry as returned by `search_routines` (no `path`/`created` —
/// internal detail, not needed by the AI to decide what to run).
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct RoutineSummary {
    name: String,
    description: String,
    category: String,
    tags: Vec<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct SearchRoutinesOutput {
    results: Vec<RoutineSummary>,
}

/// Input parameters for the `run_routine` MCP tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct RunRoutineParams {
    /// Exact name of the routine, as returned by `search_routines`.
    name: String,
    /// Optional argument string, passed verbatim to the script (e.g.
    /// `"-Path C:\\Foo -MinSizeMB 50"`). Omit unless the user's request
    /// names a specific starting folder or parameter — routines default to
    /// the current working directory otherwise.
    #[serde(default)]
    args: Option<String>,
}

/// Structured output for `run_routine`. `ok:false` means the named routine
/// was not found (see `message`) — `stdout`/`stderr`/`exit_code`/`cwd` are
/// empty/`-1` in that case, same convention as `open_target`'s `{ok, message}`.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct RunRoutineOutput {
    ok: bool,
    message: String,
    stdout: String,
    stderr: String,
    exit_code: i32,
    cwd: String,
}

/// Input parameters for the `get_routine_content` MCP tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct GetRoutineContentParams {
    /// Exact name of the routine, as returned by `search_routines`.
    name: String,
}

/// Structured output for `get_routine_content`. `found:false` means the
/// name is not in the index — every other field is empty in that case (same
/// "empty, not missing" convention as `RunRoutineOutput`).
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct GetRoutineContentOutput {
    found: bool,
    name: String,
    description: String,
    category: String,
    tags: Vec<String>,
    content: String,
    created: String,
}

/// Input parameters for the `save_routine` MCP tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SaveRoutineParams {
    name: String,
    description: String,
    tags: Vec<String>,
    category: String,
    content: String,
    /// Nome esatto di una routine esistente da sostituire/aggiornare
    /// (rinomina permessa: `name` può differire). Omesso = routine nuova e
    /// distinta.
    #[serde(default)]
    replace: Option<String>,
}

/// Structured output for `save_routine`. `ok:false` covers every rejection
/// (nome invalido, collisione, target di replace non trovato, errore di
/// scrittura) — `message` spiega quale.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct SaveRoutineOutput {
    ok: bool,
    message: String,
}

// ─── MCP server struct ────────────────────────────────────────────────────────

/// The MCP server that exposes `run_in_session` and `reset_session`.
///
/// Holds a shared [`Session`] behind an `Arc<Mutex<>>` so it can be cloned
/// (required by `ServerHandler`'s `Clone` bound) while retaining single ownership
/// of the live shell.
#[derive(Clone)]
struct SessionServer {
    session: Arc<Mutex<Session>>,
    /// Root del repository routine, risolta una volta all'avvio
    /// (`mcp_server::routines::resolve_root`, override `LARE_ROUTINES_DIR`).
    routines_root: std::path::PathBuf,
}

impl SessionServer {
    /// `routines_root` arriva già risolto da `main()` (vedi
    /// `startup_config::resolve`) — questo costruttore non legge più
    /// l'ambiente per conto proprio.
    fn new(routines_root: std::path::PathBuf) -> Self {
        Self {
            session: Arc::new(Mutex::new(Session::new())),
            routines_root,
        }
    }
}

#[tool_router(server_handler)]
impl SessionServer {
    /// Run a command in the persistent shell session.
    ///
    /// The shell is started lazily on the first call and kept alive between calls.
    /// State (`cwd`, environment variables, background processes) persists.
    ///
    /// Returns `{ stdout, stderr, exit_code }` where `stdout` contains the
    /// combined output of the command (both stdout and stderr streams).
    ///
    /// When `progress_token` is present in params, each output line is forwarded
    /// as a `notifications/progress` notification to the client peer.
    #[tool(
        description = "Execute a command in the persistent shell session (PowerShell on Windows, sh on Unix). State (cwd, env) persists between calls. Returns stdout, stderr, and exit_code."
    )]
    async fn run_in_session(
        &self,
        peer: Peer<RoleServer>,
        Parameters(RunInSessionParams { command, progress_token }): Parameters<RunInSessionParams>,
    ) -> String {
        let session = self.session.lock().await;

        if let Some(token_str) = progress_token {
            // Streaming path: each output line is sent as a progress notification.
            //
            // Architecture:
            // 1. Create an unbounded channel (drain_tx, drain_rx).
            // 2. Spawn a drain task that reads lines from drain_rx and calls
            //    peer.notify_progress() for each line.
            // 3. Pass drain_tx into session.run() so read_lines_lossy_until_marker
            //    sends each non-marker line to the channel.
            // 4. After run() returns, drop drain_tx (already moved) so drain_task
            //    exits when the channel is drained.
            let (drain_tx, mut drain_rx) =
                tokio::sync::mpsc::unbounded_channel::<String>();

            // Build the ProgressToken from the string.
            let progress_token = ProgressToken(NumberOrString::String(token_str.into()));

            let peer_clone = peer.clone();
            let token_clone = progress_token.clone();
            let drain_task = tokio::spawn(async move {
                let mut progress: f64 = 0.0;
                while let Some(line) = drain_rx.recv().await {
                    progress += 1.0;
                    let param = ProgressNotificationParam::new(token_clone.clone(), progress)
                        .with_message(line);
                    // Ignore send errors: if the client disconnected, we just stop
                    // sending notifications and let the command finish normally.
                    if let Err(e) = peer_clone.notify_progress(param).await {
                        tracing::debug!("notify_progress failed (client may have disconnected): {e}");
                        break;
                    }
                }
            });

            let result = session.run(&command, Some(drain_tx)).await;
            // Wait for the drain task to finish so all in-flight notifications
            // are sent before we return the final result.
            let _ = drain_task.await;

            serde_json::to_string(&SessionOutput {
                stdout: result.stdout,
                stderr: result.stderr,
                exit_code: result.exit_code,
                cwd: result.cwd,
            })
            .unwrap()
        } else {
            // Non-streaming path (backward compatible).
            let result = session.run(&command, None).await;
            serde_json::to_string(&SessionOutput {
                stdout: result.stdout,
                stderr: result.stderr,
                exit_code: result.exit_code,
                cwd: result.cwd,
            })
            .unwrap()
        }
    }

    /// Reset the shell session.
    ///
    /// Terminates the current shell process.  The next `run_in_session` call
    /// will start a fresh shell (new `cwd`, clean environment).
    #[tool(
        description = "Reset the shell session: terminate the current shell and start fresh on the next command."
    )]
    async fn reset_session(&self) -> String {
        let session = self.session.lock().await;
        session.reset().await;
        r#"{"reset":true}"#.to_string()
    }

    /// Open a target (URL, folder, or file) with the OS default application.
    ///
    /// Classification (ADR-012):
    /// - `http(s)://` → browser
    /// - existing folder path → file manager
    /// - existing file path → associated application
    /// - anything else → error (`ok: false`), nothing is launched
    ///
    /// Returns `{ ok: bool, message: String }` JSON.
    /// Uses the `opener` crate (no shell-string → no injection risk).
    #[tool(
        description = "Open a target (http(s):// URL, folder path, or file path) with the OS default app. Returns { ok, message }."
    )]
    async fn open_target(
        &self,
        Parameters(OpenTargetParams { target }): Parameters<OpenTargetParams>,
    ) -> String {
        // open_target::open_target is a synchronous function (opener::open is sync).
        // It is safe to call directly in an async context because it either
        // returns immediately (NotFound) or spawns an OS process and returns
        // (not blocking in the async sense).
        let result = open_target::open_target(&target);
        serde_json::to_string(&result).unwrap()
    }

    /// Search saved PowerShell routines by name, description, or tag.
    ///
    /// Read-only, no side effects — never gated by the confirmation gate
    /// (same treatment as `show_markdown`). Omit `query` to list everything
    /// in the repository.
    #[tool(
        description = "Search saved PowerShell routines by name, description, or tag (case-insensitive substring match). Omit query to list all. Read-only, no side effects."
    )]
    async fn search_routines(
        &self,
        Parameters(SearchRoutinesParams { query }): Parameters<SearchRoutinesParams>,
    ) -> String {
        let idx = mcp_server::routines::load_index(&self.routines_root);
        let results: Vec<RoutineSummary> = mcp_server::routines::search(&idx, query.as_deref())
            .into_iter()
            .map(|e| RoutineSummary {
                name: e.name.clone(),
                description: e.description.clone(),
                category: e.category.clone(),
                tags: e.tags.clone(),
            })
            .collect();
        serde_json::to_string(&SearchRoutinesOutput { results }).unwrap()
    }

    /// Run a saved PowerShell routine by name in the persistent shell session.
    ///
    /// Executes in the SAME session as `run_in_session` — same cwd, same env.
    /// Does NOT stream: `RunRoutineParams` has no `progress_token`, output
    /// arrives all at once when the routine finishes (unlike `run_in_session`,
    /// which supports real-time streaming). The routine's own working
    /// directory is whatever the session's cwd currently is; this tool never
    /// injects an absolute default path. Always gated by the confirmation
    /// gate, even from the local UI channel (unlike `run_in_session`): a
    /// routine is invoked by name, its body isn't re-shown at each run.
    #[tool(
        description = "Run a saved PowerShell routine by name in the persistent shell session (same cwd/env as run_in_session). Optional args string is passed through verbatim to the script — only pass a path argument if the user's request names a specific starting folder, otherwise omit it and the routine will use the current working directory. Only routines already present in the repository can be run; this tool does not create new routines."
    )]
    async fn run_routine(
        &self,
        Parameters(RunRoutineParams { name, args }): Parameters<RunRoutineParams>,
    ) -> String {
        let idx = mcp_server::routines::load_index(&self.routines_root);
        let entry = match mcp_server::routines::find_by_name(&idx, &name) {
            Some(e) => e.clone(),
            None => {
                return serde_json::to_string(&RunRoutineOutput {
                    ok: false,
                    message: format!(
                        "Routine '{name}' non trovata in {}",
                        self.routines_root.display()
                    ),
                    stdout: String::new(),
                    stderr: String::new(),
                    exit_code: -1,
                    cwd: String::new(),
                })
                .unwrap();
            }
        };

        let invocation =
            mcp_server::routines::build_invocation(&entry, &self.routines_root, args.as_deref());
        let session = self.session.lock().await;
        let result = session.run(&invocation, None).await;

        serde_json::to_string(&RunRoutineOutput {
            ok: true,
            message: String::new(),
            stdout: result.stdout,
            stderr: result.stderr,
            exit_code: result.exit_code,
            cwd: result.cwd,
        })
        .unwrap()
    }

    /// Read the full script body of a saved routine, by exact name.
    ///
    /// Read-only, no side effects — same treatment as `search_routines`.
    /// Companion to `save_routine`: the AI calls this BEFORE proposing a new
    /// routine when `search_routines` surfaced a similarly-named or
    /// similarly-themed one, to compare bodies and decide reuse/update/distinct
    /// (design doc §3).
    #[tool(
        description = "Read the full script body of a saved routine, by exact name. Read-only. Use this BEFORE save_routine when search_routines surfaced a similarly-named or similarly-themed routine, to compare the two scripts and decide whether to reuse the existing one, update it (save_routine with replace), or create a distinct new one."
    )]
    async fn get_routine_content(
        &self,
        Parameters(GetRoutineContentParams { name }): Parameters<GetRoutineContentParams>,
    ) -> String {
        let idx = mcp_server::routines::load_index(&self.routines_root);
        let out = match mcp_server::routines::get_content(&self.routines_root, &idx, &name) {
            Some((entry, content)) => GetRoutineContentOutput {
                found: true,
                name: entry.name,
                description: entry.description,
                category: entry.category,
                tags: entry.tags,
                content,
                created: entry.created,
            },
            None => GetRoutineContentOutput {
                found: false,
                name,
                description: String::new(),
                category: String::new(),
                tags: Vec::new(),
                content: String::new(),
                created: String::new(),
            },
        };
        serde_json::to_string(&out).unwrap()
    }

    /// Save a new PowerShell routine, or update/rename an existing one.
    ///
    /// Writes are gated at the orchestrator layer (`SENSITIVE_TOOLS`,
    /// `ToolConfirmer::confirm_routine_save`) BEFORE this tool is ever
    /// dispatched — by the time this method runs, the user has already
    /// approved the exact script body in a dedicated review window. This
    /// method itself never asks for confirmation; it only validates
    /// (`validate_name`, collision/replace-target checks) and writes.
    #[tool(
        description = "Save a new PowerShell routine, or update/rename an existing one (pass replace with the exact name of the routine being replaced). The script MUST already have been tested successfully with run_in_session before calling this — never save an untested script. Before creating a brand-new routine, always check search_routines first for a similarly-named or similarly-themed one; if found, read it with get_routine_content and decide whether to reuse it, update it (replace), or use a distinct new name. Requires explicit user confirmation in a dedicated review window before anything is written to disk."
    )]
    async fn save_routine(
        &self,
        Parameters(SaveRoutineParams { name, description, tags, category, content, replace }):
            Parameters<SaveRoutineParams>,
    ) -> String {
        let created = mcp_server::routines::format_date_from_unix_secs(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        );
        let result = mcp_server::routines::save_entry(
            &self.routines_root,
            &name,
            &description,
            tags,
            &category,
            &content,
            &created,
            replace.as_deref(),
        );
        let out = match result {
            Ok(()) => SaveRoutineOutput { ok: true, message: String::new() },
            Err(e) => {
                use mcp_server::routines::SaveError;
                let message = match e {
                    SaveError::NameCollision => format!("Esiste già una routine chiamata '{name}'."),
                    SaveError::InvalidName(n) => {
                        format!("Nome routine non valido: '{n}' (solo lettere minuscole, cifre, trattini).")
                    }
                    SaveError::ReplaceTargetNotFound(n) => {
                        format!("Routine da sostituire non trovata: '{n}'.")
                    }
                    SaveError::Io(msg) => format!("Errore di scrittura: {msg}"),
                };
                SaveRoutineOutput { ok: false, message }
            }
        };
        serde_json::to_string(&out).unwrap()
    }
}

// ─── Entry point ──────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    // All logs go to stderr — stdout is the MCP JSON-RPC wire.
    fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    tracing::info!("Lare Terminal mcp-server v0.7.1 starting (transport: stdio)");

    // ── Configurazione da startup.json (fase 1) ───────────────────────────
    // `exe_dir` = cartella di mcp-server.exe (o del binario di debug in
    // sviluppo). `startup_cfg` = None se il file non c'è (caso normale) o
    // se illeggibile/malformato (loggato, mai un crash).
    let exe_dir = startup_config::exe_dir().ok();
    // `load_from_dir` distingue "file assente" (`Ok(None)`, caso normale) da
    // "file presente ma illeggibile/malformato" (`Err`, da loggare — il
    // contratto del crate `startup-config` dice esplicitamente "il chiamante
    // logga e cade al default, MAI un panic"). Un `.ok()` qui scarterebbe
    // l'errore in silenzio: un `startup.json` con una virgola di troppo
    // farebbe cadere ai default senza NESSUN segnale in log, esattamente la
    // classe di bug-fantasma ("perché non fa nulla?") che questo progetto
    // ha già incontrato più volte con env var sbagliate.
    let startup_cfg = match exe_dir.as_deref().map(startup_config::load_from_dir) {
        Some(Ok(cfg)) => cfg,
        Some(Err(e)) => {
            tracing::warn!("{e} — uso i default");
            None
        }
        None => None,
    };
    let local_dir = startup_config::resolve(
        std::env::var("LARE_LOCAL_DIR").ok().as_deref(),
        startup_cfg.as_ref().and_then(|c| c.local_dir.as_deref()),
        startup_config::default_local_dir,
    );
    let routines_root = mcp_server::routines::resolve_root(
        std::env::var("LARE_ROUTINES_DIR").ok().as_deref(),
        startup_cfg.as_ref().and_then(|c| c.routines_dir.as_deref()),
        &local_dir,
    );

    let service = SessionServer::new(routines_root).serve(stdio()).await?;
    service.waiting().await?;

    Ok(())
}
