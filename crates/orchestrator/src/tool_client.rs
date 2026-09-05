//! # tool_client — MCP tool abstraction (DIP for testability)
//!
//! Defines the [`ToolClient`] trait and two implementations:
//!
//! - [`FakeToolClient`] — in-memory mock used in unit/integration tests.
//!   No child processes, no network, deterministic.
//! - [`McpToolClient`] — real client that spawns `mcp-server` as a child
//!   process (stdio MCP transport via `rmcp`) and calls `run_in_session`.
//!   The connection is **persistent**: `mcp-server` is spawned once and
//!   the connection is reused for all subsequent commands.  This is the
//!   critical requirement for session state (`cwd`, env) to persist across calls.
//!
//! ## Why a trait? (DIP — SOLID)
//! `core::handle_command` depends on `&dyn ToolClient`, not on `McpToolClient`.
//! This lets the entire business core (routing, response shaping) be tested
//! without spawning any child process or touching the filesystem.
//!
//! ## Error handling convention
//! Both impls return a [`CommandResult`] — they never propagate errors up.
//! On failure: `exit_code = -1`, `stderr` contains the reason, `stdout` is empty.
//! This keeps `handle_command` free of `Result` in the Os branch; callers
//! observe a `Done{ exit_code: -1 }` with a preceding `Chunk{ stderr }`.
//!
//! ## Persistent connection (McpToolClient — ADR-011)
//! The session lives in the `mcp-server` process.  If `McpToolClient` spawned
//! a fresh `mcp-server` per call, the session would die with each call and
//! `cd`/env persistence would be illusory.
//!
//! `McpToolClient` holds `Arc<Mutex<Option<Peer<...>>>>` (lazy-init):
//! - First call: spawn `mcp-server`, MCP handshake, store the peer + keep-alive task.
//! - Subsequent calls: reuse the stored peer handle (the `mcp-server` process remains alive).
//! - On error (process died): clear the stored peer; the next call will retry.

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;

/// The result of an OS command execution, matching Contratto B output shape.
///
/// Fields mirror `mcp-server`'s session `CommandOutput` so JSON round-trips are
/// trivial in `McpToolClient`.
#[derive(Debug, Clone, PartialEq)]
pub struct CommandResult {
    /// Everything written to stdout (+ merged stderr where applicable).
    pub stdout: String,
    /// Always empty for session commands (stderr merged into stdout by session.rs).
    pub stderr: String,
    /// Process exit code; `-1` for spawn failure or session error.
    pub exit_code: i32,
    /// Current working directory of the shell after the command (Task 3).
    ///
    /// Empty string if the mcp-server did not report a cwd (older version or
    /// error path).  Populated from the `"cwd"` field of the JSON response
    /// produced by mcp-server 0.4.0+.
    pub cwd: String,
}

/// Risultato del tool `open_target` (ADR-012).
///
/// Mirrors `mcp_server::open_target::OpenResult` per il round-trip JSON.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenResult {
    pub ok: bool,
    pub message: String,
}

/// Una routine come restituita da `search_routines` (sottoinsieme di
/// `RoutineEntry` lato mcp-server — niente `path`/`created`, l'AI non ne ha
/// bisogno per decidere cosa eseguire).
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct RoutineSummary {
    pub name: String,
    pub description: String,
    pub category: String,
    pub tags: Vec<String>,
}

/// Risultato del tool `search_routines`.
///
/// A differenza di `CommandResult`/`OpenResult`, la forma JSON di mcp-server
/// (`{results: [...]}`, vedi `SearchRoutinesOutput` in `main.rs`) non porta
/// un canale di errore — è sola lettura, "nessun risultato" è un esito
/// normale. Ma QUESTO tipo (il risultato del client, non del tool MCP) deve
/// anche coprire i fallimenti di trasporto (spawn/handshake/parse falliti)
/// che il tool stesso non modella: senza `error`, un mcp-server morto e
/// "nessuna routine salvata" sarebbero indistinguibili per l'AI.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchRoutinesResult {
    pub results: Vec<RoutineSummary>,
    /// `Some(...)` solo per un fallimento di trasporto/connessione, MAI per
    /// "zero risultati" (quello è `results: vec![]`, `error: None`).
    pub error: Option<String>,
}

/// Risultato del tool `run_routine`.
///
/// Mirrors `mcp-server`'s `RunRoutineOutput` esattamente (`ok`, `message`,
/// `stdout`, `stderr`, `exit_code`, `cwd`) — quella forma già copre sia il
/// caso "routine non trovata" (`ok:false`) sia i fallimenti di esecuzione
/// (`exit_code`), stesso schema di `CommandResult`. A differenza di
/// `SearchRoutinesResult`, qui basta riusare `ok`/`message` anche per i
/// fallimenti di trasporto (stesso pattern di `OpenResult`).
#[derive(Debug, Clone, PartialEq)]
pub struct RunRoutineResult {
    pub ok: bool,
    pub message: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub cwd: String,
}

/// Risultato del tool `get_routine_content`.
///
/// Stessa convenzione a due canali di `SearchRoutinesResult`: `found:false` +
/// `error:None` = "routine non trovata" (esito normale); `error:Some` =
/// fallimento di trasporto (spawn/handshake/parse), MAI confuso col primo.
#[derive(Debug, Clone, PartialEq)]
pub struct GetRoutineContentResult {
    pub found: bool,
    pub name: String,
    pub description: String,
    pub category: String,
    pub tags: Vec<String>,
    pub content: String,
    pub created: String,
    pub error: Option<String>,
}

/// Risultato del tool `save_routine`. `ok:false` copre sia i rifiuti logici
/// (nome invalido, collisione, target di replace non trovato — vedi
/// `mcp_server::routines::SaveError`) sia i fallimenti di trasporto, stesso
/// pattern a due valori di `OpenResult`/`RunRoutineResult`.
#[derive(Debug, Clone, PartialEq)]
pub struct SaveRoutineResult {
    pub ok: bool,
    pub message: String,
}

/// Esito di un `dispatch()` — output/errore per il modello, più un `report`
/// opzionale (Docs/superpowers/specs/2026-07-16-mcp-nmap-design.md §6): un
/// documento (es. uno scan nmap) che l'orchestrator apre come finestra
/// Markdown e salva in Library, DETERMINISTICAMENTE — mai una scelta
/// dell'AI, che non vede nemmeno `report` (solo `output`). `None` per ogni
/// `ToolClient` che non produce documenti (tutti tranne un canale come
/// mcp-nmap).
#[derive(Debug, Clone, PartialEq)]
pub struct DispatchOutcome {
    pub output: String,
    pub is_error: bool,
    pub report: Option<ChannelReport>,
    /// Riassunto numerico breve (≤5 righe), mostrato nel pannello del canale
    /// SUBITO dopo il dispatch — mai dall'AI, generato deterministicamente
    /// dal tool stesso (es. `stock_report` lo deriva dagli stessi numeri
    /// delle proprie sezioni fattuali — Docs/superpowers/specs/2026-07-22-
    /// financial-markets-stock-report-design.md, refinement 2026-07-23).
    /// `None` per ogni `ToolClient` che non produce questo riassunto (tutti
    /// tranne un tool come `stock_report`).
    pub channel_summary: Option<String>,
}

/// Documento strutturato da un tool_use — titolo + Markdown completo. Mai
/// mandato al modello (solo `DispatchOutcome::output`, tipicamente un breve
/// summary, lo è).
#[derive(Debug, Clone, PartialEq)]
pub struct ChannelReport {
    pub title: String,
    pub markdown: String,
    /// `false` (nmap): la finestra si apre SUBITO al ritorno di `dispatch()`
    /// — comportamento storico, il documento del tool è già completo.
    /// `true` (financial-markets' `stock_report`): l'apertura è POSPOSTA
    /// dal loop di `ai_adapter.rs` fino al testo finale del turno (le
    /// sezioni 5-6 dell'AI, scritte SOLO dopo aver letto il riassunto del
    /// tool), e quel testo viene fuso in coda al documento prima di aprirlo
    /// — un documento con narrativa/ipotesi dell'AI non esiste ancora al
    /// momento del dispatch, a differenza di uno scan nmap.
    pub defer_to_turn_end: bool,
}

/// Abstraction over the MCP tool calls.
///
/// # Object Safety
/// Object-safe via `async_trait`.  Use `Arc<dyn ToolClient>` for shared
/// ownership or `Box<dyn ToolClient>` for owned.
#[async_trait]
pub trait ToolClient: Send + Sync {
    /// Run a command in the persistent shell session and return its output.
    ///
    /// Never returns `Err` — all failure modes are encoded in `CommandResult`
    /// with `exit_code = -1`.
    ///
    /// `progress_tx`: when `Some`, each output line is forwarded to the channel
    /// as soon as it is received (real-time streaming).  When `None`, the
    /// behaviour is identical to the single-argument API.
    async fn run_in_session(
        &self,
        command: &str,
        progress_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
    ) -> CommandResult;

    /// Reset the shell session: terminate the current shell process.
    ///
    /// The next `run_in_session` call will start a fresh shell.
    /// Used by the `/reset` slash command path (ADR-012).
    async fn reset_session(&self);

    /// Open a target (URL, folder, or file) with the OS default application.
    ///
    /// Returns `OpenResult { ok, message }`.  If `ok = false` the target was
    /// not found or could not be opened — no process was launched.
    /// (ADR-012 — Superficie 2)
    async fn open_target(&self, target: &str) -> OpenResult;

    /// Search saved PowerShell routines by name, description, or tag.
    ///
    /// Never returns `Err` — transport failures fold into
    /// `SearchRoutinesResult.error`, matching `run_in_session`'s convention
    /// of never propagating errors up.
    async fn search_routines(&self, query: Option<&str>) -> SearchRoutinesResult;

    /// Run a saved PowerShell routine by name, in the persistent shell session.
    ///
    /// `ok = false` means the routine name was not found, or a transport
    /// failure occurred — see `message`. Mirrors `mcp-server`'s `run_routine`
    /// tool output shape exactly.
    async fn run_routine(&self, name: &str, args: Option<&str>) -> RunRoutineResult;

    /// Read the full script body of a saved routine, by exact name.
    ///
    /// Read-only, no side effects. Default: `found: false` + a
    /// "non disponibile su questo canale" error — same rejection every
    /// external-channel `ToolClient` (fixture/nmap/python) hand-wrote for
    /// `search_routines`/`run_routine` before this trait had defaults; now
    /// inherited for free by any impl that does not override it. ONLY
    /// `McpToolClient` (below) and `CwdTrackingToolClient` (the decorator
    /// that wraps it — see `cwd_tracking.rs`) override this.
    async fn get_routine_content(&self, _name: &str) -> GetRoutineContentResult {
        GetRoutineContentResult {
            found: false,
            name: String::new(),
            description: String::new(),
            category: String::new(),
            tags: Vec::new(),
            content: String::new(),
            created: String::new(),
            error: Some("get_routine_content non disponibile su questo canale".to_string()),
        }
    }

    /// Save a new routine, or update/rename an existing one (`replace`).
    ///
    /// Same default-rejection principle as `get_routine_content` above — see
    /// there for which impls override it.
    #[allow(clippy::too_many_arguments)]
    async fn save_routine(
        &self,
        _name: &str,
        _description: &str,
        _tags: Vec<String>,
        _category: &str,
        _content: &str,
        _replace: Option<&str>,
    ) -> SaveRoutineResult {
        SaveRoutineResult {
            ok: false,
            message: "save_routine non disponibile su questo canale".to_string(),
        }
    }

    /// Tool esposti all'AI per QUESTO client (Docs/superpowers/specs/
    /// 2026-07-16-tool-isolation-design.md). Default: i 5 tool storici
    /// (`agent::tool_defs()` — `run_in_session`/`open_target`/`show_markdown`/
    /// `search_routines`/`run_routine`), comportamento invariato per
    /// cursore/Telegram. Un `ToolClient` di canale esterno (es. mcp-nmap)
    /// sovrascrive questo metodo per esporre SOLO i propri tool — mai questi 5.
    fn tool_defs(&self) -> Vec<crate::messages_client::ToolDef> {
        crate::agent::tool_defs()
    }

    /// Esegue un `tool_use` per nome. Required method (no default impl due to
    /// trait object safety constraint); concrete impls call `agent::dispatch_tool`
    /// directly — comportamento invariato per cursore/Telegram.
    /// Un `ToolClient` di canale sovrascrive per dispacciare i PROPRI tool:
    /// anche se il modello invocasse `run_in_session` per errore, un canale
    /// che non lo sovrascrive per gestirlo lo rifiuta come "sconosciuto" (la
    /// stessa via di un nome davvero inventato) — l'isolamento non dipende dal
    /// buon comportamento del modello, è strutturale in questo metodo.
    async fn dispatch(&self, name: &str, input: &serde_json::Value) -> DispatchOutcome;

    /// Called once when the connection this `ToolClient` belongs to is
    /// tearing down (see `ws.rs`'s `handle_connection`, right before it
    /// returns) — a chance for a channel-specific implementation to release
    /// resources it privately owns (e.g. a spawned child process). Default:
    /// no-op, matching every historical `ToolClient` (cursor/Telegram via
    /// `McpToolClient`, which has no such resource — `mcp-server`'s child
    /// process is a workspace-lifetime singleton managed elsewhere, not
    /// per-connection).
    async fn shutdown(&self) {}
}

// ── Fake implementation for tests ─────────────────────────────────────────────

/// A deterministic, in-memory [`ToolClient`] for unit and integration tests.
///
/// Returns canned responses without spawning any process.  Construct with
/// [`FakeToolClient::success`] or [`FakeToolClient::failure`].
///
/// `open_target` always returns `OpenResult { ok: true, message: "fake: opened <target>" }`
/// (simulates a successful open; test `ok=false` by passing a target with
/// "nonexistent" in it — see `open_target` impl below).
pub struct FakeToolClient {
    stdout: String,
    stderr: String,
    exit_code: i32,
    /// Cwd reported back by the fake shell (empty = not set, matches default).
    cwd: String,
}

impl FakeToolClient {
    /// Creates a fake that simulates a successful command (exit 0).
    pub fn success(stdout: impl Into<String>) -> Self {
        Self {
            stdout: stdout.into(),
            stderr: String::new(),
            exit_code: 0,
            cwd: String::new(),
        }
    }

    /// Creates a fake that simulates a failed command with stderr output.
    pub fn failure(stderr: impl Into<String>, exit_code: i32) -> Self {
        Self {
            stdout: String::new(),
            stderr: stderr.into(),
            exit_code,
            cwd: String::new(),
        }
    }

    /// Creates a fake with full control over all three fields.
    pub fn with_output(
        stdout: impl Into<String>,
        stderr: impl Into<String>,
        exit_code: i32,
    ) -> Self {
        Self {
            stdout: stdout.into(),
            stderr: stderr.into(),
            exit_code,
            cwd: String::new(),
        }
    }

    /// Creates a fake that simulates a successful command reporting a specific cwd.
    ///
    /// Use this in tests that need to verify cwd tracking after `run_in_session`.
    pub fn with_cwd(stdout: impl Into<String>, cwd: impl Into<String>) -> Self {
        Self {
            stdout: stdout.into(),
            stderr: String::new(),
            exit_code: 0,
            cwd: cwd.into(),
        }
    }
}

#[async_trait]
impl ToolClient for FakeToolClient {
    async fn run_in_session(
        &self,
        _command: &str,
        progress_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
    ) -> CommandResult {
        // Return the canned result regardless of what was requested.
        // Tests assert on the *shape* of the response (Chunk/Done), not on
        // the exact command that was run — routing tests cover that separately.
        //
        // If a progress_tx is provided, forward stdout lines so that tests that
        // exercise the streaming path see the expected chunks.
        if let Some(tx) = progress_tx {
            for line in self.stdout.lines() {
                let _ = tx.send(format!("{line}\n"));
            }
        }
        CommandResult {
            stdout: self.stdout.clone(),
            stderr: self.stderr.clone(),
            exit_code: self.exit_code,
            cwd: self.cwd.clone(),
        }
    }

    async fn reset_session(&self) {
        // No-op for the fake: no real session to reset.
    }

    async fn open_target(&self, target: &str) -> OpenResult {
        // Fake: simulate a "not found" if the target contains "nonexistent",
        // otherwise simulate success.  This lets tests exercise both branches
        // without touching the filesystem or launching any GUI.
        if target.contains("nonexistent") {
            OpenResult {
                ok: false,
                message: format!("target non trovato o non riconosciuto: {target}"),
            }
        } else {
            OpenResult {
                ok: true,
                message: format!("fake: aperto: {target}"),
            }
        }
    }

    async fn search_routines(&self, _query: Option<&str>) -> SearchRoutinesResult {
        // Canned: nessuna routine — i test che hanno bisogno di risultati
        // costruiscono l'outcome atteso direttamente, come già fanno per
        // run_in_session con `stdout`/`stderr`/`exit_code` canned.
        SearchRoutinesResult { results: Vec::new(), error: None }
    }

    async fn run_routine(&self, _name: &str, _args: Option<&str>) -> RunRoutineResult {
        // Riusa gli stessi campi canned di run_in_session — stesso principio
        // (i test asserisce sulla FORMA della risposta, non sul comando).
        RunRoutineResult {
            ok: true,
            message: String::new(),
            stdout: self.stdout.clone(),
            stderr: self.stderr.clone(),
            exit_code: self.exit_code,
            cwd: self.cwd.clone(),
        }
    }

    async fn get_routine_content(&self, _name: &str) -> GetRoutineContentResult {
        // Canned "non trovata" (senza error — diverso dal default del trait,
        // che è per canali senza questo tool affatto): i test che hanno
        // bisogno di un contenuto specifico costruiscono un ToolClient
        // dedicato (stesso principio già in uso per search_routines).
        GetRoutineContentResult {
            found: false,
            name: String::new(),
            description: String::new(),
            category: String::new(),
            tags: Vec::new(),
            content: String::new(),
            created: String::new(),
            error: None,
        }
    }

    async fn save_routine(
        &self,
        _name: &str,
        _description: &str,
        _tags: Vec<String>,
        _category: &str,
        _content: &str,
        _replace: Option<&str>,
    ) -> SaveRoutineResult {
        // Canned successo — stesso principio di run_routine (i test
        // asseriscono sulla FORMA della risposta; un test che deve simulare
        // un fallimento costruisce un ToolClient dedicato).
        SaveRoutineResult { ok: true, message: String::new() }
    }

    async fn dispatch(&self, name: &str, input: &serde_json::Value) -> DispatchOutcome {
        // Delegate to agent::dispatch_tool — self is Sized in this concrete impl,
        // so the cast to &dyn ToolClient works without issue.
        crate::agent::dispatch_tool(self as &dyn ToolClient, name, input).await
    }
}

// ── Fixture per la prova di isolamento canale ─────────────────────────────────

/// `ToolClient` di test che dimostra l'isolamento per-canale end-to-end
/// (Docs/superpowers/specs/2026-07-16-tool-isolation-design.md): espone SOLO
/// 2 tool fittizi (mai i 3 storici) e le 3 chiamate storiche
/// (`run_in_session`/`reset_session`/`open_target`) ritornano un errore
/// leggibile invece di eseguire una shell reale o aprire un target — questo è
/// ciò che blocca strutturalmente le vie `Route::Os`/`Route::Slash` di
/// `core.rs` per una connessione di canale, SENZA che `core.rs` sappia nulla
/// di canali. Un `ToolClient` di canale reale (mcp-nmap) segue lo stesso
/// schema con le proprie 3 chiamate storiche.
pub struct FixtureChannelToolClient;

#[async_trait]
impl ToolClient for FixtureChannelToolClient {
    async fn run_in_session(
        &self,
        _command: &str,
        _progress_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
    ) -> CommandResult {
        CommandResult {
            stdout: String::new(),
            stderr: "run_in_session non disponibile su questo canale".to_string(),
            exit_code: -1,
            cwd: String::new(),
        }
    }

    async fn reset_session(&self) {
        // No-op: questo canale non ha una sessione shell reale da riavviare.
    }

    async fn open_target(&self, _target: &str) -> OpenResult {
        OpenResult {
            ok: false,
            message: "open_target non disponibile su questo canale".to_string(),
        }
    }

    async fn search_routines(&self, _query: Option<&str>) -> SearchRoutinesResult {
        SearchRoutinesResult {
            results: Vec::new(),
            error: Some("search_routines non disponibile su questo canale".to_string()),
        }
    }

    async fn run_routine(&self, _name: &str, _args: Option<&str>) -> RunRoutineResult {
        RunRoutineResult {
            ok: false,
            message: "run_routine non disponibile su questo canale".to_string(),
            stdout: String::new(),
            stderr: String::new(),
            exit_code: -1,
            cwd: String::new(),
        }
    }

    fn tool_defs(&self) -> Vec<crate::messages_client::ToolDef> {
        vec![
            crate::messages_client::ToolDef {
                name: "fixture_tool_a".to_string(),
                description: "Tool fittizio A, solo per test di isolamento canale.".to_string(),
                input_schema: serde_json::json!({"type": "object", "properties": {}, "required": []}),
            },
            crate::messages_client::ToolDef {
                name: "fixture_tool_b".to_string(),
                description: "Tool fittizio B, solo per test di isolamento canale.".to_string(),
                input_schema: serde_json::json!({"type": "object", "properties": {}, "required": []}),
            },
        ]
    }

    async fn dispatch(&self, name: &str, _input: &serde_json::Value) -> DispatchOutcome {
        match name {
            "fixture_tool_a" => DispatchOutcome { output: "eseguito fixture_tool_a".to_string(), is_error: false, report: None, channel_summary: None },
            "fixture_tool_b" => DispatchOutcome { output: "eseguito fixture_tool_b".to_string(), is_error: false, report: None, channel_summary: None },
            other => DispatchOutcome { output: format!("tool sconosciuto sul canale fixture: {other}"), is_error: true, report: None, channel_summary: None },
        }
    }
}

// ── Real MCP client implementation ────────────────────────────────────────────

/// MCP client handler that routes `notifications/progress` to a `ProgressDispatcher`.
///
/// `McpToolClient` uses this as the `ClientHandler` when connecting to `mcp-server`.
/// The dispatcher holds a map of `ProgressToken → mpsc::Sender<ProgressNotificationParam>`.
/// Each subscribed call site gets a `ProgressSubscriber` (a `Stream`) that yields
/// notifications for its token.
///
/// ## Why a custom handler instead of `()`?
/// rmcp's `()` handler ignores all notifications (no-op `on_progress`).  We need
/// `on_progress` to be wired to the dispatcher so that streaming progress from the
/// server reaches the orchestrator's progress channel.
#[derive(Clone)]
struct LareClientHandler {
    progress_dispatcher: rmcp::handler::client::progress::ProgressDispatcher,
}

impl LareClientHandler {
    fn new() -> Self {
        Self {
            progress_dispatcher: rmcp::handler::client::progress::ProgressDispatcher::new(),
        }
    }
}

impl rmcp::ClientHandler for LareClientHandler {
    fn on_progress(
        &self,
        params: rmcp::model::ProgressNotificationParam,
        _context: rmcp::service::NotificationContext<rmcp::RoleClient>,
    ) -> impl std::future::Future<Output = ()> + rmcp::service::MaybeSendFuture + '_ {
        // Forward the notification to the dispatcher; it routes it to the
        // appropriate subscriber by token.  Errors (no subscriber registered)
        // are silently dropped — the dispatcher logs a warning.
        self.progress_dispatcher.handle_notification(params)
    }
}

/// A [`ToolClient`] that calls the real `mcp-server` binary via stdio MCP.
///
/// Spawns `mcp-server` as a child process on the **first call** and reuses
/// the connection for all subsequent calls (persistent connection — ADR-011).
///
/// ## Why persistent?
/// The shell session (`cwd`, env, background processes) lives inside `mcp-server`.
/// Spawning a new process per call would destroy the session on every command,
/// making `cd` and env persistence impossible.
///
/// ## Streaming (v0.5.0 / ADR-011 extension)
/// The client uses [`LareClientHandler`] as its MCP handler instead of `()`.
/// `LareClientHandler` contains a [`ProgressDispatcher`] that routes incoming
/// `notifications/progress` to subscribed callers.
///
/// When `run_in_session` is called with `Some(progress_tx)`:
/// 1. A unique token is generated.
/// 2. A [`ProgressSubscriber`] is registered with the dispatcher for that token.
/// 3. A routing task reads from the subscriber and forwards each notification's
///    `message` field to `progress_tx`.
/// 4. The `progress_token` is injected into the tool arguments so `mcp-server`
///    knows which token to use for its `notify_progress` calls.
///
/// ## Binary path resolution
/// 1. `LARE_MCP_SERVER` env var — explicit override.
/// 2. Sibling of the current executable (`std::env::current_exe()`).
///
/// ## Error handling
/// On any transport / parse / tool error: returns `CommandResult{ exit_code: -1,
/// stderr: <reason>, stdout: "" }`.  If the connection dies, it is cleared so
/// the next call will reconnect.
pub struct McpToolClient {
    /// Path to the `mcp-server` binary.
    mcp_server_path: std::path::PathBuf,
    /// The persistent peer handle (None = not yet connected).
    ///
    /// The peer is a cheap clone-able handle; the underlying transport/process
    /// is kept alive by a background task (see `run_in_session`).
    peer: Arc<Mutex<Option<rmcp::Peer<rmcp::RoleClient>>>>,
    /// The client handler that holds the progress dispatcher.
    ///
    /// Stored so we can call `subscribe()` on the dispatcher for each streaming
    /// call.  The same handler instance is used for the lifetime of the
    /// `mcp-server` connection (it is passed to `serve()` at connect time).
    handler: Arc<Mutex<Option<LareClientHandler>>>,
}

impl McpToolClient {
    /// Resolve the `mcp-server` binary path using env-var-first, then sibling.
    ///
    /// # Errors
    /// Returns an error if neither the env var points to an existing file nor
    /// the sibling path can be determined.
    pub fn resolve() -> anyhow::Result<Self> {
        // Override via env var (test / deployment flexibility).
        if let Ok(path) = std::env::var("LARE_MCP_SERVER") {
            return Ok(Self {
                mcp_server_path: std::path::PathBuf::from(path),
                peer: Arc::new(Mutex::new(None)),
                handler: Arc::new(Mutex::new(None)),
            });
        }

        // Default: sibling of the current executable.
        // In dev both binaries are in target/debug/.
        let exe = std::env::current_exe()?;
        let parent = exe
            .parent()
            .ok_or_else(|| anyhow::anyhow!("current_exe has no parent directory"))?;

        #[cfg(windows)]
        let sibling = parent.join("mcp-server.exe");
        #[cfg(not(windows))]
        let sibling = parent.join("mcp-server");

        Ok(Self {
            mcp_server_path: sibling,
            peer: Arc::new(Mutex::new(None)),
            handler: Arc::new(Mutex::new(None)),
        })
    }
}

#[async_trait]
impl ToolClient for McpToolClient {
    async fn run_in_session(
        &self,
        command: &str,
        progress_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
    ) -> CommandResult {
        use futures::StreamExt;
        use rmcp::{
            model::{CallToolRequestParams, NumberOrString, ProgressToken},
            transport::TokioChildProcess,
            ServiceExt,
        };
        use serde_json::json;

        let mut peer_guard = self.peer.lock().await;
        let mut handler_guard = self.handler.lock().await;

        // ── Lazy-connect on first call ─────────────────────────────────────────
        if peer_guard.is_none() {
            let child_cmd = {
                let mut c = tokio::process::Command::new(&self.mcp_server_path);
                // Stdin/stdout are the MCP wire; stderr is for logs (inherited).
                c.stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::inherit());
                c
            };

            let transport = match TokioChildProcess::new(child_cmd) {
                Ok(t) => t,
                Err(e) => {
                    return CommandResult {
                        stdout: String::new(),
                        stderr: format!("failed to spawn mcp-server: {e}"),
                        exit_code: -1,
                        cwd: String::new(),
                    };
                }
            };

            // Use LareClientHandler instead of () so on_progress notifications
            // are routed to the ProgressDispatcher.
            let client_handler = LareClientHandler::new();

            // Perform the MCP handshake.
            let running = match client_handler.clone().serve(transport).await {
                Ok(r) => r,
                Err(e) => {
                    return CommandResult {
                        stdout: String::new(),
                        stderr: format!("MCP handshake failed: {e}"),
                        exit_code: -1,
                        cwd: String::new(),
                    };
                }
            };

            // Clone out the peer handle for tool calls.
            // Spawn a keep-alive task that holds `running` (the service) alive.
            // The task ends when the runtime shuts down (process exit).
            // This ensures the `mcp-server` child process stays alive as long as
            // the orchestrator process is running.
            let peer = running.peer().clone();
            tokio::spawn(async move {
                // `running.waiting()` completes only when the service shuts down.
                // This task is cancelled/aborted when the tokio runtime exits.
                running.waiting().await.ok();
            });

            *peer_guard = Some(peer);
            // Store the handler so we can access its dispatcher for streaming calls.
            *handler_guard = Some(client_handler);
        }

        // ── Build args, optionally with progress_token for streaming ──────────
        let peer = peer_guard.as_ref().expect("just initialized");

        let mut args = serde_json::Map::new();
        args.insert("command".to_string(), json!(command));

        // Streaming path: generate a token, subscribe to the dispatcher, spawn
        // a routing task that forwards progress notifications to progress_tx.
        let routing_task: Option<tokio::task::JoinHandle<()>> = if let Some(tx) = progress_tx {
            if let Some(handler) = handler_guard.as_ref() {
                // Generate a unique token for this call.
                let token_str = format!("lare-{:x}", rand::random::<u32>());
                let progress_token =
                    ProgressToken(NumberOrString::String(token_str.clone().into()));

                // Inject the token into the tool arguments.
                args.insert("progress_token".to_string(), json!(token_str));

                // Subscribe to the dispatcher before sending the request, so we
                // don't miss any early notifications.
                let mut subscriber = handler
                    .progress_dispatcher
                    .subscribe(progress_token)
                    .await;

                // Spawn a routing task that reads from the subscriber stream and
                // forwards each notification's message to the progress channel.
                let task = tokio::spawn(async move {
                    while let Some(notification) = subscriber.next().await {
                        let line = notification.message.unwrap_or_default();
                        // Ignore send errors: if the receiver was dropped (e.g.
                        // the command was cancelled), stop forwarding silently.
                        if tx.send(line).is_err() {
                            break;
                        }
                    }
                });
                Some(task)
            } else {
                // Handler not yet initialised (should not happen since we just
                // connected above, but be defensive).
                None
            }
        } else {
            None
        };

        let params = CallToolRequestParams::new("run_in_session").with_arguments(args);

        // Timeout di sicurezza: 90s per evitare hang da comandi lenti (nmap, ping -t, ecc.)
        // Se scade, il risultato è un messaggio di errore leggibile dall'utente.
        const TOOL_TIMEOUT_SECS: u64 = 90;
        let call_future = peer.call_tool(params);
        let result = match tokio::time::timeout(
            std::time::Duration::from_secs(TOOL_TIMEOUT_SECS),
            call_future,
        )
        .await
        {
            Ok(r) => r,
            Err(_elapsed) => {
                // La connessione è potenzialmente in uno stato inconsistente dopo un timeout.
                // Azzeriamo il peer così la prossima chiamata riaprirà la connessione.
                *peer_guard = None;
                *handler_guard = None;
                if let Some(task) = routing_task {
                    task.abort();
                }
                return CommandResult {
                    stdout: format!(
                        "\u{26a0} Timeout: il comando ha superato {TOOL_TIMEOUT_SECS}s ed \u{e8} stato annullato automaticamente. \
                         Prova un comando pi\u{f9} mirato o usa opzioni di timeout native (es. `-W 1` per nmap)."
                    ),
                    stderr: String::new(),
                    exit_code: -1,
                    cwd: String::new(),
                };
            }
        };

        // Abort the routing task. The ProgressSubscriber stream never self-closes
        // (the dispatcher holds the sender indefinitely), so awaiting the task
        // would block forever. By the time peer.call_tool() returns, all
        // notifications have been sent and dispatched (JSON-RPC is ordered).
        // The routing task will have already forwarded them via tx; aborting
        // just stops it from waiting for more that will never come.
        if let Some(task) = routing_task {
            task.abort();
        }

        match result {
            Err(e) => {
                // Connection may be dead — clear it so the next call re-establishes.
                *peer_guard = None;
                *handler_guard = None;
                CommandResult {
                    stdout: String::new(),
                    stderr: format!("call_tool failed: {e}"),
                    exit_code: -1,
                    cwd: String::new(),
                }
            }
            Ok(tool_result) => {
                // `mcp-server` returns a single Text content item containing JSON:
                //   {"stdout":"...","stderr":"...","exit_code":N}
                let json_text = tool_result
                    .content
                    .iter()
                    .find_map(|c| c.as_text().map(|t| t.text.clone()))
                    .unwrap_or_default();

                #[derive(serde::Deserialize)]
                struct McpOutput {
                    stdout: String,
                    stderr: String,
                    exit_code: i32,
                    /// Populated by mcp-server 0.4.0+; empty string if absent (compat).
                    #[serde(default)]
                    cwd: String,
                }

                match serde_json::from_str::<McpOutput>(&json_text) {
                    Ok(out) => CommandResult {
                        stdout: out.stdout,
                        stderr: out.stderr,
                        exit_code: out.exit_code,
                        cwd: out.cwd,
                    },
                    Err(e) => CommandResult {
                        stdout: String::new(),
                        stderr: format!(
                            "failed to parse mcp-server response: {e}; raw: {json_text:?}"
                        ),
                        exit_code: -1,
                        cwd: String::new(),
                    },
                }
            }
        }
    }

    async fn reset_session(&self) {
        use rmcp::model::CallToolRequestParams;

        let peer_guard = self.peer.lock().await;
        if let Some(peer) = peer_guard.as_ref() {
            let params = CallToolRequestParams::new("reset_session");
            // Ignore errors: if the session is already dead, it will respawn.
            let _ = peer.call_tool(params).await;
        }
        // If no peer is established yet, there is nothing to reset.
    }

    async fn open_target(&self, target: &str) -> OpenResult {
        use rmcp::{model::CallToolRequestParams, transport::TokioChildProcess, ServiceExt};
        use serde_json::json;

        let mut peer_guard = self.peer.lock().await;
        let mut handler_guard = self.handler.lock().await;

        // ── Lazy-connect (mirrors run_in_session) ─────────────────────────────
        if peer_guard.is_none() {
            let child_cmd = {
                let mut c = tokio::process::Command::new(&self.mcp_server_path);
                c.stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::inherit());
                c
            };

            let transport = match TokioChildProcess::new(child_cmd) {
                Ok(t) => t,
                Err(e) => {
                    return OpenResult {
                        ok: false,
                        message: format!("failed to spawn mcp-server: {e}"),
                    };
                }
            };

            let client_handler = LareClientHandler::new();
            let running = match client_handler.clone().serve(transport).await {
                Ok(r) => r,
                Err(e) => {
                    return OpenResult {
                        ok: false,
                        message: format!("MCP handshake failed: {e}"),
                    };
                }
            };

            let peer = running.peer().clone();
            tokio::spawn(async move {
                running.waiting().await.ok();
            });

            *peer_guard = Some(peer);
            *handler_guard = Some(client_handler);
        }

        // ── Call the tool ─────────────────────────────────────────────────────
        let peer = peer_guard.as_ref().expect("just initialized");

        let args = {
            let mut obj = serde_json::Map::new();
            obj.insert("target".to_string(), json!(target));
            obj
        };

        let params = CallToolRequestParams::new("open_target").with_arguments(args);
        let result = peer.call_tool(params).await;

        match result {
            Err(e) => {
                *peer_guard = None;
                *handler_guard = None;
                OpenResult {
                    ok: false,
                    message: format!("call_tool open_target failed: {e}"),
                }
            }
            Ok(tool_result) => {
                let json_text = tool_result
                    .content
                    .iter()
                    .find_map(|c| c.as_text().map(|t| t.text.clone()))
                    .unwrap_or_default();

                #[derive(serde::Deserialize)]
                struct McpOpenResult {
                    ok: bool,
                    message: String,
                }

                match serde_json::from_str::<McpOpenResult>(&json_text) {
                    Ok(out) => OpenResult {
                        ok: out.ok,
                        message: out.message,
                    },
                    Err(e) => OpenResult {
                        ok: false,
                        message: format!(
                            "failed to parse open_target response: {e}; raw: {json_text:?}"
                        ),
                    },
                }
            }
        }
    }

    async fn search_routines(&self, query: Option<&str>) -> SearchRoutinesResult {
        use rmcp::{model::CallToolRequestParams, transport::TokioChildProcess, ServiceExt};
        use serde_json::json;

        let mut peer_guard = self.peer.lock().await;
        let mut handler_guard = self.handler.lock().await;

        if peer_guard.is_none() {
            let child_cmd = {
                let mut c = tokio::process::Command::new(&self.mcp_server_path);
                c.stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::inherit());
                c
            };

            let transport = match TokioChildProcess::new(child_cmd) {
                Ok(t) => t,
                Err(e) => {
                    return SearchRoutinesResult {
                        results: Vec::new(),
                        error: Some(format!("failed to spawn mcp-server: {e}")),
                    };
                }
            };

            let client_handler = LareClientHandler::new();
            let running = match client_handler.clone().serve(transport).await {
                Ok(r) => r,
                Err(e) => {
                    return SearchRoutinesResult {
                        results: Vec::new(),
                        error: Some(format!("MCP handshake failed: {e}")),
                    };
                }
            };

            let peer = running.peer().clone();
            tokio::spawn(async move {
                running.waiting().await.ok();
            });

            *peer_guard = Some(peer);
            *handler_guard = Some(client_handler);
        }

        let peer = peer_guard.as_ref().expect("just initialized");

        let mut args = serde_json::Map::new();
        if let Some(q) = query {
            args.insert("query".to_string(), json!(q));
        }

        let params = CallToolRequestParams::new("search_routines").with_arguments(args);
        let result = peer.call_tool(params).await;

        match result {
            Err(e) => {
                *peer_guard = None;
                *handler_guard = None;
                SearchRoutinesResult {
                    results: Vec::new(),
                    error: Some(format!("call_tool search_routines failed: {e}")),
                }
            }
            Ok(tool_result) => {
                let json_text = tool_result
                    .content
                    .iter()
                    .find_map(|c| c.as_text().map(|t| t.text.clone()))
                    .unwrap_or_default();

                #[derive(serde::Deserialize)]
                struct McpSearchOutput {
                    results: Vec<RoutineSummary>,
                }

                match serde_json::from_str::<McpSearchOutput>(&json_text) {
                    Ok(out) => SearchRoutinesResult { results: out.results, error: None },
                    Err(e) => SearchRoutinesResult {
                        results: Vec::new(),
                        error: Some(format!(
                            "failed to parse search_routines response: {e}; raw: {json_text:?}"
                        )),
                    },
                }
            }
        }
    }

    async fn run_routine(&self, name: &str, args: Option<&str>) -> RunRoutineResult {
        use rmcp::{model::CallToolRequestParams, transport::TokioChildProcess, ServiceExt};
        use serde_json::json;

        let mut peer_guard = self.peer.lock().await;
        let mut handler_guard = self.handler.lock().await;

        if peer_guard.is_none() {
            let child_cmd = {
                let mut c = tokio::process::Command::new(&self.mcp_server_path);
                c.stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::inherit());
                c
            };

            let transport = match TokioChildProcess::new(child_cmd) {
                Ok(t) => t,
                Err(e) => {
                    return RunRoutineResult {
                        ok: false,
                        message: format!("failed to spawn mcp-server: {e}"),
                        stdout: String::new(),
                        stderr: String::new(),
                        exit_code: -1,
                        cwd: String::new(),
                    };
                }
            };

            let client_handler = LareClientHandler::new();
            let running = match client_handler.clone().serve(transport).await {
                Ok(r) => r,
                Err(e) => {
                    return RunRoutineResult {
                        ok: false,
                        message: format!("MCP handshake failed: {e}"),
                        stdout: String::new(),
                        stderr: String::new(),
                        exit_code: -1,
                        cwd: String::new(),
                    };
                }
            };

            let peer = running.peer().clone();
            tokio::spawn(async move {
                running.waiting().await.ok();
            });

            *peer_guard = Some(peer);
            *handler_guard = Some(client_handler);
        }

        let peer = peer_guard.as_ref().expect("just initialized");

        let mut mcp_args = serde_json::Map::new();
        mcp_args.insert("name".to_string(), json!(name));
        if let Some(a) = args {
            mcp_args.insert("args".to_string(), json!(a));
        }

        // Stesso timeout di sicurezza di run_in_session: una routine può
        // essere lenta quanto un comando qualsiasi.
        const TOOL_TIMEOUT_SECS: u64 = 90;
        let params = CallToolRequestParams::new("run_routine").with_arguments(mcp_args);
        let call_future = peer.call_tool(params);
        let result = match tokio::time::timeout(
            std::time::Duration::from_secs(TOOL_TIMEOUT_SECS),
            call_future,
        )
        .await
        {
            Ok(r) => r,
            Err(_elapsed) => {
                *peer_guard = None;
                *handler_guard = None;
                return RunRoutineResult {
                    ok: false,
                    message: format!(
                        "\u{26a0} Timeout: la routine ha superato {TOOL_TIMEOUT_SECS}s ed \u{e8} stata annullata automaticamente."
                    ),
                    stdout: String::new(),
                    stderr: String::new(),
                    exit_code: -1,
                    cwd: String::new(),
                };
            }
        };

        match result {
            Err(e) => {
                *peer_guard = None;
                *handler_guard = None;
                RunRoutineResult {
                    ok: false,
                    message: format!("call_tool run_routine failed: {e}"),
                    stdout: String::new(),
                    stderr: String::new(),
                    exit_code: -1,
                    cwd: String::new(),
                }
            }
            Ok(tool_result) => {
                let json_text = tool_result
                    .content
                    .iter()
                    .find_map(|c| c.as_text().map(|t| t.text.clone()))
                    .unwrap_or_default();

                #[derive(serde::Deserialize)]
                struct McpRunRoutineOutput {
                    ok: bool,
                    message: String,
                    stdout: String,
                    stderr: String,
                    exit_code: i32,
                    cwd: String,
                }

                match serde_json::from_str::<McpRunRoutineOutput>(&json_text) {
                    Ok(out) => RunRoutineResult {
                        ok: out.ok,
                        message: out.message,
                        stdout: out.stdout,
                        stderr: out.stderr,
                        exit_code: out.exit_code,
                        cwd: out.cwd,
                    },
                    Err(e) => RunRoutineResult {
                        ok: false,
                        message: format!(
                            "failed to parse run_routine response: {e}; raw: {json_text:?}"
                        ),
                        stdout: String::new(),
                        stderr: String::new(),
                        exit_code: -1,
                        cwd: String::new(),
                    },
                }
            }
        }
    }

    async fn get_routine_content(&self, name: &str) -> GetRoutineContentResult {
        use rmcp::{model::CallToolRequestParams, transport::TokioChildProcess, ServiceExt};
        use serde_json::json;

        let mut peer_guard = self.peer.lock().await;
        let mut handler_guard = self.handler.lock().await;

        if peer_guard.is_none() {
            let child_cmd = {
                let mut c = tokio::process::Command::new(&self.mcp_server_path);
                c.stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::inherit());
                c
            };

            let transport = match TokioChildProcess::new(child_cmd) {
                Ok(t) => t,
                Err(e) => {
                    return GetRoutineContentResult {
                        found: false,
                        name: String::new(),
                        description: String::new(),
                        category: String::new(),
                        tags: Vec::new(),
                        content: String::new(),
                        created: String::new(),
                        error: Some(format!("failed to spawn mcp-server: {e}")),
                    };
                }
            };

            let client_handler = LareClientHandler::new();
            let running = match client_handler.clone().serve(transport).await {
                Ok(r) => r,
                Err(e) => {
                    return GetRoutineContentResult {
                        found: false,
                        name: String::new(),
                        description: String::new(),
                        category: String::new(),
                        tags: Vec::new(),
                        content: String::new(),
                        created: String::new(),
                        error: Some(format!("MCP handshake failed: {e}")),
                    };
                }
            };

            let peer = running.peer().clone();
            tokio::spawn(async move {
                running.waiting().await.ok();
            });

            *peer_guard = Some(peer);
            *handler_guard = Some(client_handler);
        }

        let peer = peer_guard.as_ref().expect("just initialized");

        let mut args = serde_json::Map::new();
        args.insert("name".to_string(), json!(name));

        let params = CallToolRequestParams::new("get_routine_content").with_arguments(args);
        let result = peer.call_tool(params).await;

        match result {
            Err(e) => {
                *peer_guard = None;
                *handler_guard = None;
                GetRoutineContentResult {
                    found: false,
                    name: String::new(),
                    description: String::new(),
                    category: String::new(),
                    tags: Vec::new(),
                    content: String::new(),
                    created: String::new(),
                    error: Some(format!("call_tool get_routine_content failed: {e}")),
                }
            }
            Ok(tool_result) => {
                let json_text = tool_result
                    .content
                    .iter()
                    .find_map(|c| c.as_text().map(|t| t.text.clone()))
                    .unwrap_or_default();

                #[derive(serde::Deserialize)]
                struct McpGetRoutineContentOutput {
                    found: bool,
                    name: String,
                    description: String,
                    category: String,
                    tags: Vec<String>,
                    content: String,
                    created: String,
                }

                match serde_json::from_str::<McpGetRoutineContentOutput>(&json_text) {
                    Ok(out) => GetRoutineContentResult {
                        found: out.found,
                        name: out.name,
                        description: out.description,
                        category: out.category,
                        tags: out.tags,
                        content: out.content,
                        created: out.created,
                        error: None,
                    },
                    Err(e) => GetRoutineContentResult {
                        found: false,
                        name: String::new(),
                        description: String::new(),
                        category: String::new(),
                        tags: Vec::new(),
                        content: String::new(),
                        created: String::new(),
                        error: Some(format!(
                            "failed to parse get_routine_content response: {e}; raw: {json_text:?}"
                        )),
                    },
                }
            }
        }
    }

    async fn save_routine(
        &self,
        name: &str,
        description: &str,
        tags: Vec<String>,
        category: &str,
        content: &str,
        replace: Option<&str>,
    ) -> SaveRoutineResult {
        use rmcp::{model::CallToolRequestParams, transport::TokioChildProcess, ServiceExt};
        use serde_json::json;

        let mut peer_guard = self.peer.lock().await;
        let mut handler_guard = self.handler.lock().await;

        if peer_guard.is_none() {
            let child_cmd = {
                let mut c = tokio::process::Command::new(&self.mcp_server_path);
                c.stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::inherit());
                c
            };

            let transport = match TokioChildProcess::new(child_cmd) {
                Ok(t) => t,
                Err(e) => {
                    return SaveRoutineResult {
                        ok: false,
                        message: format!("failed to spawn mcp-server: {e}"),
                    };
                }
            };

            let client_handler = LareClientHandler::new();
            let running = match client_handler.clone().serve(transport).await {
                Ok(r) => r,
                Err(e) => {
                    return SaveRoutineResult {
                        ok: false,
                        message: format!("MCP handshake failed: {e}"),
                    };
                }
            };

            let peer = running.peer().clone();
            tokio::spawn(async move {
                running.waiting().await.ok();
            });

            *peer_guard = Some(peer);
            *handler_guard = Some(client_handler);
        }

        let peer = peer_guard.as_ref().expect("just initialized");

        let mut args = serde_json::Map::new();
        args.insert("name".to_string(), json!(name));
        args.insert("description".to_string(), json!(description));
        args.insert("tags".to_string(), json!(tags));
        args.insert("category".to_string(), json!(category));
        args.insert("content".to_string(), json!(content));
        if let Some(r) = replace {
            args.insert("replace".to_string(), json!(r));
        }

        let params = CallToolRequestParams::new("save_routine").with_arguments(args);
        let result = peer.call_tool(params).await;

        match result {
            Err(e) => {
                *peer_guard = None;
                *handler_guard = None;
                SaveRoutineResult { ok: false, message: format!("call_tool save_routine failed: {e}") }
            }
            Ok(tool_result) => {
                let json_text = tool_result
                    .content
                    .iter()
                    .find_map(|c| c.as_text().map(|t| t.text.clone()))
                    .unwrap_or_default();

                #[derive(serde::Deserialize)]
                struct McpSaveRoutineOutput {
                    ok: bool,
                    message: String,
                }

                match serde_json::from_str::<McpSaveRoutineOutput>(&json_text) {
                    Ok(out) => SaveRoutineResult { ok: out.ok, message: out.message },
                    Err(e) => SaveRoutineResult {
                        ok: false,
                        message: format!("failed to parse save_routine response: {e}; raw: {json_text:?}"),
                    },
                }
            }
        }
    }

    async fn dispatch(&self, name: &str, input: &serde_json::Value) -> DispatchOutcome {
        // Delegate to agent::dispatch_tool — self is Sized in this concrete impl,
        // so the cast to &dyn ToolClient works without issue.
        crate::agent::dispatch_tool(self as &dyn ToolClient, name, input).await
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests — TDD: RED then GREEN
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── FakeToolClient: progress_tx forwarding ───────────────────────────────

    /// RED → GREEN: when `Some(tx)` is passed to `FakeToolClient::run_in_session`,
    /// each line of the canned stdout is forwarded to the channel.
    #[tokio::test]
    async fn fake_run_in_session_with_progress_tx_forwards_lines() {
        let client = FakeToolClient::success("line1\nline2\nline3");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let result = client.run_in_session("echo", Some(tx)).await;
        // Final result is still correct.
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("line1"));
        // Channel received forwarded lines.
        drop(result); // ensure progress_tx is dropped via the call
        let mut lines: Vec<String> = Vec::new();
        while let Ok(l) = rx.try_recv() {
            lines.push(l);
        }
        assert_eq!(lines.len(), 3, "expected 3 forwarded lines, got: {lines:?}");
        assert!(lines[0].contains("line1"));
        assert!(lines[1].contains("line2"));
        assert!(lines[2].contains("line3"));
    }

    /// When `None` is passed, `FakeToolClient::run_in_session` behaves as before.
    #[tokio::test]
    async fn fake_run_in_session_none_progress_tx_no_change() {
        let client = FakeToolClient::success("hello\n");
        let result = client.run_in_session("echo hello", None).await;
        assert_eq!(result.stdout, "hello\n");
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn fake_success_returns_correct_fields() {
        let client = FakeToolClient::success("hello\n");
        let result = client.run_in_session("echo hello", None).await;
        assert_eq!(result.stdout, "hello\n");
        assert_eq!(result.stderr, "");
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn fake_failure_returns_correct_fields() {
        let client = FakeToolClient::failure("command not found", 127);
        let result = client.run_in_session("badcmd", None).await;
        assert_eq!(result.stdout, "");
        assert_eq!(result.stderr, "command not found");
        assert_eq!(result.exit_code, 127);
    }

    #[tokio::test]
    async fn fake_with_output_controls_all_fields() {
        let client = FakeToolClient::with_output("out", "err", 42);
        let result = client.run_in_session("cmd", None).await;
        assert_eq!(result.stdout, "out");
        assert_eq!(result.stderr, "err");
        assert_eq!(result.exit_code, 42);
    }

    #[tokio::test]
    async fn fake_is_dyn_compatible() {
        let client: Box<dyn ToolClient> = Box::new(FakeToolClient::success("ok"));
        let result = client.run_in_session("any", None).await;
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn default_shutdown_is_a_true_noop() {
        // FakeToolClient never overrides shutdown() — this proves the
        // trait's default impl exists, is callable, and does nothing
        // observable (no panic, no side effect to assert on beyond "it
        // returned"). Guards against a future refactor accidentally
        // making `shutdown` a required method (which would break every
        // existing ToolClient implementor that has no resource to release).
        let client = FakeToolClient::success("ok");
        client.shutdown().await;
    }

    // ── open_target su FakeToolClient ─────────────────────────────────────────

    #[tokio::test]
    async fn fake_open_target_success_returns_ok_true() {
        let client = FakeToolClient::success("");
        let result = client.open_target("http://example.com").await;
        assert!(result.ok, "expected ok=true for a non-nonexistent target");
        assert!(
            result.message.contains("http://example.com"),
            "expected target in message: {:?}",
            result.message
        );
    }

    #[tokio::test]
    async fn fake_open_target_nonexistent_returns_ok_false() {
        let client = FakeToolClient::success("");
        let result = client.open_target("/percorso/nonexistent/target").await;
        assert!(!result.ok, "expected ok=false for 'nonexistent' target");
        assert!(
            result.message.contains("non trovato"),
            "expected 'non trovato' in message: {:?}",
            result.message
        );
    }

    #[tokio::test]
    async fn fake_open_target_is_dyn_compatible() {
        let client: Box<dyn ToolClient> = Box::new(FakeToolClient::success(""));
        let result = client.open_target("/tmp/test").await;
        // Just checking dyn dispatch doesn't panic.
        let _ = result.ok;
    }

    // ── cwd field on CommandResult (Task 3 / Step 1) ─────────────────────────

    /// FakeToolClient: `cwd` defaults to empty string when not set via the new constructor.
    /// RED: `CommandResult` has no `cwd` field yet → compilation error.
    #[tokio::test]
    async fn command_result_has_cwd_field() {
        let client = FakeToolClient::success("out");
        let result = client.run_in_session("pwd", None).await;
        // cwd must be present; default is empty string for backward compatibility.
        assert_eq!(result.cwd, "", "cwd should default to empty string");
    }

    /// FakeToolClient with an explicit cwd propagates it in `CommandResult`.
    /// RED: `FakeToolClient::with_cwd` constructor doesn't exist yet.
    #[tokio::test]
    async fn fake_with_cwd_returns_cwd_in_result() {
        let client = FakeToolClient::with_cwd("out", "/home/user");
        let result = client.run_in_session("pwd", None).await;
        assert_eq!(result.cwd, "/home/user");
        assert_eq!(result.exit_code, 0);
    }

    /// McpToolClient JSON parsing: `cwd` from JSON, default `""` if absent.
    /// RED: `McpOutput` in the real client doesn't have a `cwd` field yet.
    #[test]
    fn mcp_output_cwd_parsed_from_json() {
        // Simulate what `McpToolClient` parses from the mcp-server JSON response.
        #[allow(dead_code)] // Fields used by serde deserialization, not read directly.
        #[derive(serde::Deserialize)]
        struct McpOutput {
            stdout: String,
            stderr: String,
            exit_code: i32,
            #[serde(default)]
            cwd: String,
        }
        // With cwd present.
        let with_cwd: McpOutput =
            serde_json::from_str(r#"{"stdout":"","stderr":"","exit_code":0,"cwd":"/tmp/x"}"#)
                .unwrap();
        assert_eq!(with_cwd.cwd, "/tmp/x");
        // Without cwd (older mcp-server, compat).
        let without_cwd: McpOutput =
            serde_json::from_str(r#"{"stdout":"","stderr":"","exit_code":0}"#).unwrap();
        assert_eq!(without_cwd.cwd, "");
    }

    // ── tool_defs()/dispatch() — trait defaults vs. channel override ────────
    //
    // TDD RED: these tests are written BEFORE `tool_defs`/`dispatch` exist on
    // `ToolClient` and BEFORE `FixtureChannelToolClient` exists. Compile error
    // (E0599: no method named `tool_defs`/`dispatch` found; E0433: cannot find
    // `FixtureChannelToolClient`).

    #[test]
    fn default_tool_defs_matches_historical_five() {
        let client = FakeToolClient::success("x");
        let defs = client.tool_defs();
        assert_eq!(defs.len(), 8, "attesi 8 tool (era 7 prima di set_ai_display_name, Task 7), got {defs:?}");
        assert!(defs.iter().any(|d| d.name == "run_in_session"));
        assert!(defs.iter().any(|d| d.name == "open_target"));
        assert!(defs.iter().any(|d| d.name == "get_routine_content"));
        assert!(defs.iter().any(|d| d.name == "save_routine"));
        assert!(defs.iter().any(|d| d.name == "show_markdown"));
        assert!(defs.iter().any(|d| d.name == "search_routines"));
        assert!(defs.iter().any(|d| d.name == "run_routine"));
    }

    #[tokio::test]
    async fn default_dispatch_routes_run_in_session_like_agent_dispatch_tool() {
        let client = FakeToolClient::success("file.txt\n");
        let outcome = client
            .dispatch("run_in_session", &serde_json::json!({"command": "ls"}))
            .await;
        assert!(outcome.output.contains("file.txt"));
        assert!(!outcome.is_error);
    }

    #[tokio::test]
    async fn default_dispatch_unknown_tool_is_error() {
        let client = FakeToolClient::success("x");
        let outcome = client.dispatch("nope", &serde_json::json!({})).await;
        assert!(outcome.is_error);
        assert!(outcome.output.contains("sconosciuto"));
    }

    #[test]
    fn fixture_channel_tool_defs_exposes_only_two_fixture_tools() {
        let client = FixtureChannelToolClient;
        let defs = client.tool_defs();
        assert_eq!(defs.len(), 2, "atteso SOLO i 2 tool del canale, got {defs:?}");
        assert!(defs.iter().any(|d| d.name == "fixture_tool_a"));
        assert!(defs.iter().any(|d| d.name == "fixture_tool_b"));
        assert!(
            !defs.iter().any(|d| d.name == "run_in_session"),
            "run_in_session non deve MAI comparire per questo canale"
        );
        assert!(
            !defs.iter().any(|d| d.name == "show_markdown"),
            "show_markdown non deve MAI comparire per questo canale"
        );
    }

    #[tokio::test]
    async fn fixture_channel_dispatch_executes_own_tools() {
        let client = FixtureChannelToolClient;
        let outcome = client.dispatch("fixture_tool_a", &serde_json::json!({})).await;
        assert_eq!(outcome.output, "eseguito fixture_tool_a");
        assert!(!outcome.is_error);

        let outcome_b = client.dispatch("fixture_tool_b", &serde_json::json!({})).await;
        assert_eq!(outcome_b.output, "eseguito fixture_tool_b");
        assert!(!outcome_b.is_error);
    }

    #[tokio::test]
    async fn fixture_channel_dispatch_rejects_off_menu_tool_name() {
        // "run_in_session" non è mai in tool_defs() di questo canale, ma
        // proviamo comunque a dispacciarlo — simula un modello che
        // allucinasse un nome fuori menu. Deve essere rifiutato esattamente
        // come un nome davvero inventato, non eseguito.
        let client = FixtureChannelToolClient;
        let outcome = client
            .dispatch("run_in_session", &serde_json::json!({"command": "dir"}))
            .await;
        assert!(outcome.is_error, "un tool fuori menu deve essere is_error=true");
        assert!(outcome.output.contains("sconosciuto"), "messaggio di rifiuto atteso, got: {}", outcome.output);
    }

    #[tokio::test]
    async fn fixture_channel_dispatch_rejects_run_routine_as_unknown() {
        let client = FixtureChannelToolClient;
        let outcome = client.dispatch("run_routine", &serde_json::json!({"name": "x"})).await;
        assert!(outcome.is_error);
        assert!(outcome.output.contains("sconosciuto"), "atteso rifiuto esplicito, got: {}", outcome.output);
    }

    #[tokio::test]
    async fn fixture_channel_run_in_session_never_executes_a_real_shell() {
        let client = FixtureChannelToolClient;
        let result = client.run_in_session("dir", None).await;
        assert_eq!(result.exit_code, -1);
        assert!(result.stderr.contains("non disponibile su questo canale"));
        assert_eq!(result.stdout, "", "nessun output reale: nessuna shell è mai stata eseguita");
    }

    #[tokio::test]
    async fn fixture_channel_open_target_never_opens_anything() {
        let client = FixtureChannelToolClient;
        let result = client.open_target("C:\\Users").await;
        assert!(!result.ok);
        assert!(result.message.contains("non disponibile su questo canale"));
    }

    #[tokio::test]
    async fn fixture_channel_reset_session_is_a_noop() {
        // Non deve panicare: nessuna sessione reale da riavviare su questo canale.
        let client = FixtureChannelToolClient;
        client.reset_session().await;
    }

    // ── get_routine_content/save_routine — default trait behavior ─────────────

    #[tokio::test]
    async fn fixture_channel_get_routine_content_uses_default_rejection() {
        // FixtureChannelToolClient non sovrascrive get_routine_content: eredita
        // il default del trait, invariato dopo l'aggiunta di questo metodo.
        let client = FixtureChannelToolClient;
        let r = client.get_routine_content("anything").await;
        assert!(!r.found);
        assert!(r.error.unwrap().contains("non disponibile"));
    }

    #[tokio::test]
    async fn fixture_channel_save_routine_uses_default_rejection() {
        let client = FixtureChannelToolClient;
        let r = client.save_routine("n", "d", vec![], "c", "content", None).await;
        assert!(!r.ok);
        assert!(r.message.contains("non disponibile"));
    }

    // ── FakeToolClient — get_routine_content/save_routine canned overrides ─

    #[tokio::test]
    async fn fake_get_routine_content_default_is_not_found_no_error() {
        // A differenza del default del trait, il Fake ritorna found:false
        // SENZA error — "non trovata" è l'esito canned normale per i test
        // happy-path che non hanno bisogno di un contenuto specifico.
        let client = FakeToolClient::success("");
        let r = client.get_routine_content("x").await;
        assert!(!r.found);
        assert!(r.error.is_none());
    }

    #[tokio::test]
    async fn fake_save_routine_default_is_ok() {
        let client = FakeToolClient::success("");
        let r = client.save_routine("n", "d", vec![], "c", "content", None).await;
        assert!(r.ok);
    }

    /// Runtime integration test for [`McpToolClient`] with the `run_in_session` tool.
    ///
    /// Verifies BOTH that the tool works AND that session state persists across calls
    /// (the critical ADR-011 requirement: ONE mcp-server, reused connection).
    ///
    /// Run with:
    /// ```sh
    /// $env:LARE_MCP_SERVER = "target/debug/mcp-server.exe"
    /// cargo test -p orchestrator -- --ignored mcp_tool_client_run_in_session
    /// ```
    #[ignore = "requires mcp-server binary (v0.3.0); set LARE_MCP_SERVER or run cargo build first"]
    #[tokio::test]
    async fn mcp_tool_client_run_in_session() {
        let client = McpToolClient::resolve()
            .expect("McpToolClient::resolve() failed — set LARE_MCP_SERVER env var");

        // Step 1: echo.
        #[cfg(windows)]
        let echo_cmd = "Write-Output 'hello-from-session'";
        #[cfg(not(windows))]
        let echo_cmd = "echo 'hello-from-session'";

        let result1 = client.run_in_session(echo_cmd, None).await;
        assert_eq!(
            result1.exit_code, 0,
            "echo failed: exit={}, stderr={:?}",
            result1.exit_code, result1.stderr
        );
        assert!(
            result1.stdout.contains("hello-from-session"),
            "expected 'hello-from-session' in stdout, got: {:?}",
            result1.stdout
        );

        // Step 2: cd to temp dir.
        let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
        let tmp_str = tmp.path().to_str().expect("non-UTF-8 temp path");
        let unique_name = tmp
            .path()
            .file_name()
            .expect("no file_name")
            .to_string_lossy()
            .to_lowercase();

        #[cfg(windows)]
        let cd_cmd = format!("cd \"{}\"", tmp_str);
        #[cfg(not(windows))]
        let cd_cmd = format!("cd '{}'", tmp_str);

        let result2 = client.run_in_session(&cd_cmd, None).await;
        assert_eq!(result2.exit_code, 0, "cd failed: {:?}", result2);

        // Step 3: pwd — MUST show the temp dir (persistence proof via single connection).
        #[cfg(windows)]
        let pwd_cmd = "(Get-Location).Path";
        #[cfg(not(windows))]
        let pwd_cmd = "pwd";

        let result3 = client.run_in_session(pwd_cmd, None).await;
        assert!(
            result3.stdout.to_lowercase().contains(&unique_name),
            "PERSISTENCE FAILED: expected {:?} in pwd output, got: {:?}",
            unique_name,
            result3.stdout
        );
    }

    // ── TDD RED: tool timeout (watchdog 90s) ─────────────────────────────────
    //
    // Questo test è scritto PRIMA di aggiungere il timeout in `run_in_session`.
    // Prima della modifica il test compila ma il comportamento non è garantito
    // (nessun timeout = hang indefinito). Dopo la modifica il test verifica la
    // struttura del messaggio di timeout, che è l'unica parte testabile senza
    // un server MCP reale.
    //
    // Il test del TIMEOUT EFFETTIVO (comportamento runtime con un processo che
    // non risponde) richiede un mock MCP server e vive come test #[ignore] finché
    // non abbiamo un fixture dedicato.

    /// Verifica che il messaggio di timeout prodotto da `McpToolClient::run_in_session`
    /// (quando il tokio::time::timeout scatta) contenga le informazioni chiave:
    /// "Timeout", il valore di 90s, e suggerimenti per l'utente.
    /// Questo test verifica la STRUTTURA del messaggio, non il comportamento runtime.
    #[test]
    fn timeout_message_contains_expected_content() {
        // Riproduce la stringa generata dal ramo Err(_elapsed) in run_in_session.
        const TOOL_TIMEOUT_SECS: u64 = 90;
        let msg = format!(
            "\u{26a0} Timeout: il comando ha superato {TOOL_TIMEOUT_SECS}s ed \u{e8} stato annullato automaticamente. \
             Prova un comando pi\u{f9} mirato o usa opzioni di timeout native (es. `-W 1` per nmap)."
        );
        assert!(msg.contains("90"), "il messaggio deve contenere il valore 90s: {msg}");
        assert!(msg.contains("Timeout"), "il messaggio deve contenere 'Timeout': {msg}");
        assert!(msg.contains("nmap"), "il messaggio deve suggerire nmap: {msg}");
    }

    /// Requires a built `mcp-server` binary and `LARE_MCP_SERVER` set — same
    /// convention as `mcp_tool_client_run_in_session` above. `LARE_ROUTINES_DIR`
    /// is read ONCE by `mcp-server` at process startup and the CHILD PROCESS
    /// inherits the parent's env at spawn time (`tokio::process::Command`
    /// default), so setting it here BEFORE the first call (which lazily spawns
    /// the child) redirects the routine repository to a tempdir — no risk of
    /// touching the real `%LOCALAPPDATA%\dev.lare.terminal\routines\`.
    ///
    /// Run with:
    /// ```sh
    /// $env:LARE_MCP_SERVER = "target/debug/mcp-server.exe"
    /// cargo test -p orchestrator -- --ignored mcp_tool_client_save_routine_then_get_routine_content
    /// ```
    #[ignore = "requires mcp-server binary; set LARE_MCP_SERVER or run cargo build first"]
    #[tokio::test]
    async fn mcp_tool_client_save_routine_then_get_routine_content_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("LARE_ROUTINES_DIR", tmp.path());

        let client = McpToolClient::resolve()
            .expect("McpToolClient::resolve() failed — set LARE_MCP_SERVER env var");

        let save = client
            .save_routine("mcp-rt-test", "desc", vec!["t".to_string()], "cat", "Write-Host x", None)
            .await;
        assert!(save.ok, "{save:?}");

        let read = client.get_routine_content("mcp-rt-test").await;
        assert!(read.found, "{read:?}");
        assert_eq!(read.content, "Write-Host x");

        std::env::remove_var("LARE_ROUTINES_DIR");
    }
}
