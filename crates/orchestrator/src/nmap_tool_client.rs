//! # nmap_tool_client — `ToolClient` for the mcp-nmap sidecar
//!
//! Mirrors `McpToolClient`'s lazy-spawn-and-reuse pattern (`tool_client.rs`)
//! but: (1) no streaming/progress_tx (nmap tools don't report progress
//! incrementally); (2) a much longer call timeout (900s — must exceed
//! `mcp-nmap`'s own 600s elevated-scan timeout with headroom, and the
//! confirm-gate's 180s); (3) `tool_defs()`/`dispatch()` expose ONLY the seven
//! nmap tools, never `run_in_session`/`open_target`/`show_markdown`; (4) the
//! three historical `ToolClient` methods are stubs — this is what closes the
//! `Route::Os`/`Route::Slash` bypass for the nmap channel (Docs/superpowers/
//! specs/2026-07-16-tool-isolation-design.md), mirroring
//! `FixtureChannelToolClient`'s pattern exactly.

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::messages_client::ToolDef;
use crate::tool_client::{ChannelReport, CommandResult, DispatchOutcome, OpenResult, ToolClient};

/// Outer timeout for a single `call_tool` to the `mcp-nmap` sidecar.
///
/// Must exceed BOTH `mcp-nmap::elevate::SCAN_TIMEOUT` (600s, the elevated
/// scan's own internal timeout) and the confirm-gate's 180s — otherwise this
/// timeout could fire first and abandon a scan that was still legitimately
/// running inside the sidecar. 900s (15 minutes) gives 300s of headroom over
/// the sidecar's own longest internal timeout for MCP/process overhead.
const NMAP_CALL_TIMEOUT_SECS: u64 = 900;

/// Abstraction over "forcibly kill a process, including any children it
/// spawned" — the seam that makes `NmapToolClient`'s timeout-triggered kill
/// testable without ever touching a real process (mirrors this project's
/// established DI pattern, e.g. `mcp-nmap`'s `NmapProcess` trait).
///
/// Confirmed via a live repro (temporarily lowering `NMAP_CALL_TIMEOUT_SECS`
/// and checking `tasklist`/`Get-Process` before and after) that `mcp-nmap.exe`
/// is a PERMANENT orphan on timeout — nothing ever kills it, it just parks
/// forever waiting for MCP messages nobody sends anymore, since the old code
/// only cleared the cached `Peer` handle. `/T` (tree-kill) is defense in
/// depth for the rarer case where `nmap.exe` (mcp-nmap's own child) is still
/// running when the timeout fires — Windows does not cascade-kill child
/// processes when a parent dies, so killing only `mcp-nmap.exe` would still
/// orphan `nmap.exe` in that case.
///
/// **Known gap (review finding, not fixed here):** `/T` does NOT reliably
/// reach `nmap_os_detect`'s elevated `nmap.exe` — `elevate.rs` spawns it via
/// `ShellExecuteExW`/UAC, which reparents it under the AppInfo service
/// rather than keeping it a PPID-descendant of `mcp-nmap.exe`, and even if
/// found, a same-integrity `taskkill.exe` spawned by the (non-elevated)
/// orchestrator would likely be denied permission to terminate a
/// high-integrity process. The primary fix — killing `mcp-nmap.exe` itself,
/// the confirmed permanent leak — still applies to `nmap_os_detect` timeouts
/// exactly as it does to the other six tools; only the tree-kill's bonus
/// coverage of a still-running elevated `nmap.exe` doesn't extend that far.
trait ProcessTreeKiller: Send + Sync {
    fn kill_tree(&self, pid: u32);
}

/// Real implementation: shells out to `taskkill /F /T /PID` on Windows
/// (this project's current target platform — see CLAUDE.md, "Windows
/// attuale; macOS/Linux pianificati"). The non-Windows fallback kills only
/// the direct process (no process-group/session setup exists on this
/// platform yet to make a tree-kill meaningful) — a smaller mitigation,
/// deliberately not a full fix, since macOS/Linux aren't the current target.
struct RealProcessTreeKiller;

impl ProcessTreeKiller for RealProcessTreeKiller {
    fn kill_tree(&self, pid: u32) {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            match std::process::Command::new("taskkill")
                .args(["/F", "/T", "/PID", &pid.to_string()])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
                .spawn()
            {
                Ok(_) => tracing::info!("killed timed-out mcp-nmap process tree (PID {pid})"),
                Err(e) => tracing::warn!("failed to spawn taskkill for PID {pid}: {e}"),
            }
        }
        #[cfg(not(windows))]
        {
            match std::process::Command::new("kill").args(["-9", &pid.to_string()]).spawn() {
                Ok(_) => tracing::info!("killed timed-out mcp-nmap process (PID {pid})"),
                Err(e) => tracing::warn!("failed to spawn kill for PID {pid}: {e}"),
            }
        }
    }
}

/// Mirrors `mcp-nmap::scan::ScanOutcome`'s JSON shape exactly (the two crates
/// share no Rust dependency — only this JSON contract, same pattern as
/// `McpToolClient`'s private `McpOutput`/`McpOpenResult` structs for
/// `mcp-server`'s JSON).
#[derive(Debug, serde::Deserialize)]
struct NmapScanOutcomeJson {
    summary: String,
    report_markdown: String,
    is_error: bool,
}

/// Mirrors `mcp_nmap::network_info::NetworkInfoOutcome`'s JSON shape exactly
/// — deliberately DIFFERENT from `NmapScanOutcomeJson` (no `summary`/
/// `report_markdown`, just `output`): `local_network_info`/`traceroute`
/// return their full output straight to the model, no report/window/Library
/// step (see `network_info.rs`'s module doc for why).
#[derive(Debug, serde::Deserialize)]
struct NmapInfoOutcomeJson {
    output: String,
    is_error: bool,
}

/// **Concurrency invariant this type relies on (not enforced by the type
/// system):** at most one `dispatch()` call is ever in flight on a given
/// `NmapToolClient` at a time. Today that's guaranteed externally, not by
/// this file — each WS connection gets its own fresh `NmapToolClient`
/// (`external_channel.rs`'s `tool_client` factory, called once per
/// connection), and within one connection `ws.rs` holds its
/// `ConversationHistory` lock across the whole `handle_command(...).await`,
/// including any in-flight tool call, so a second command on the same
/// connection can't reach `dispatch()` until the first one returns. This is
/// why `peer` and `child_pid` being two separate `Mutex`es below is safe in
/// practice: nothing can interleave an `ensure_connected` write with an
/// `close_connection` read/clear of a DIFFERENT connection.
/// If that concurrency guarantee ever changes (e.g. tool calls become
/// concurrent within one connection), a timeout on one call could kill the
/// `mcp-nmap.exe` process another concurrent call is still legitimately
/// using, and/or these two fields could be read torn-apart across
/// connections — at that point they'd need to move into one
/// `Mutex<Option<(Peer, u32)>>` for atomicity. Not done here: `rmcp::Peer`
/// has no public test-friendly constructor, and merging the two fields
/// would make `close_connection`'s pid-kill behavior
/// impossible to unit-test without a real MCP connection.
pub struct NmapToolClient {
    mcp_nmap_path: std::path::PathBuf,
    /// Cartella di configurazione (2.0, D6) — passata al figlio come
    /// `--config-dir` allo spawn (vedi `ensure_connected`, sotto).
    config_dir: std::path::PathBuf,
    peer: Arc<Mutex<Option<rmcp::Peer<rmcp::RoleClient>>>>,
    /// PID of the currently-connected `mcp-nmap.exe` child process, if any
    /// — captured in `ensure_connected` right after spawning, so
    /// `close_connection` can kill it by PID without
    /// needing a live handle into the child (which is owned by the
    /// detached task spawned in `ensure_connected`).
    child_pid: Arc<Mutex<Option<u32>>>,
    killer: Arc<dyn ProcessTreeKiller>,
}

impl NmapToolClient {
    /// Percorso di `mcp-nmap.exe` da `startup.json` (`paths.mcp_nmap`,
    /// default: sibling nella radice del deploy). Nessuna env var (D6, 2.0
    /// — la v1 leggeva `LARE_MCP_NMAP` qui, mirror esatto di
    /// `McpToolClient::resolve`).
    pub fn resolve(config_dir: &std::path::Path, cfg: &startup_config::StartupConfig) -> Self {
        let mcp_nmap_path = startup_config::StartupConfig::resolve_path(config_dir, &cfg.paths.mcp_nmap);
        Self {
            mcp_nmap_path,
            config_dir: config_dir.to_path_buf(),
            peer: Arc::new(Mutex::new(None)),
            child_pid: Arc::new(Mutex::new(None)),
            killer: Arc::new(RealProcessTreeKiller),
        }
    }

    /// Lazily spawn `mcp-nmap` and perform the MCP handshake on first call;
    /// reuse the peer handle on subsequent calls (mirrors
    /// `McpToolClient::run_in_session`'s lazy-connect block exactly, minus
    /// the progress-dispatcher handler — nmap tools never stream).
    async fn ensure_connected(&self) -> Result<rmcp::Peer<rmcp::RoleClient>, String> {
        let mut peer_guard = self.peer.lock().await;
        if let Some(peer) = peer_guard.as_ref() {
            return Ok(peer.clone());
        }

        use rmcp::{transport::TokioChildProcess, ServiceExt};

        let child_cmd = {
            let mut c = tokio::process::Command::new(&self.mcp_nmap_path);
            // `--config-dir`: nessuna env var (D6) — stessa cartella
            // dell'orchestrator, passata esplicitamente.
            c.arg(startup_config::CONFIG_DIR_FLAG).arg(&self.config_dir);
            // stderr su file invece di inherit(): l'orchestrator, quando è
            // staccato (self-heal, piano 3), non ha una console da cui
            // ereditare — inherit() in quel caso fa allocare a Windows una
            // console NUOVA (finestra spuria). CREATE_NO_WINDOW la sopprime;
            // il file di log sostituisce la visibilità che l'utente perde.
            // usa la stessa cartella "logs" di default dell'orchestrator; se
            // log.dir e' personalizzato in startup.json questo file resta comunque qui
            c.stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(startup_config::child_stderr_log_sink(
                    &self.config_dir.join("logs"),
                    "mcp-nmap.log",
                ));
            // `tokio::process::Command::creation_flags` è un metodo inerente
            // (a differenza di `std::process::Command`, dove serve importare
            // `CommandExt` — vedi `RealProcessTreeKiller::kill_tree` sopra).
            #[cfg(windows)]
            c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
            c
        };

        let transport = TokioChildProcess::new(child_cmd)
            .map_err(|e| format!("failed to spawn mcp-nmap: {e}"))?;
        let pid = transport.id();

        let running = ().serve(transport).await.map_err(|e| format!("MCP handshake failed: {e}"))?;
        let peer = running.peer().clone();
        tokio::spawn(async move {
            running.waiting().await.ok();
        });

        *peer_guard = Some(peer.clone());
        *self.child_pid.lock().await = pid;
        Ok(peer)
    }

    /// Clears the cached peer (forces a fresh spawn on the next call, if
    /// any) AND kills the connected `mcp-nmap.exe` process, if one is
    /// currently tracked. Called from three places:
    /// - `call_scan_tool`'s / `call_info_tool`'s `Err(_elapsed)` branches
    ///   (the outer 900s timeout fired) — NOT their generic `Err(e)`
    ///   branches, where the process has very likely already exited on its
    ///   own, so there's nothing to kill.
    /// - `shutdown()` (`ToolClient` trait impl below), called by `ws.rs` on
    ///   normal connection teardown (window closed, WS disconnected).
    ///
    /// Both call sites are confirmed PERMANENT-leak fixes (live-reproduced
    /// for the timeout case; the teardown case follows by the same
    /// reasoning — nothing else ever touches the spawned process once
    /// `NmapToolClient` stops being used, since the `tokio::spawn(async
    /// move { running.waiting().await.ok(); })` task in `ensure_connected`
    /// holds it alive independently of this struct's own lifetime).
    ///
    /// **This race DOES happen in practice, deliberately, and that's fine:**
    /// `shutdown()` (called from `ws.rs`'s connection teardown) is NOT
    /// gated by the same-connection dispatch-serialization invariant this
    /// type's own doc comment describes — that invariant only covers
    /// multiple `dispatch()` calls never overlapping EACH OTHER, not
    /// `shutdown()` racing a `dispatch()` that's still in flight in a
    /// detached command task (`ws.rs` spawns each command via `tokio::spawn`
    /// and does not await it, so teardown can run while one is still
    /// running). If the WS connection drops while a scan is genuinely
    /// still running, `shutdown()` kills it out from under that in-flight
    /// call — this is the intended outcome (the session was abandoned,
    /// killing the process sooner than its own 900s timeout would is
    /// strictly better), and it's safe: `Option::take()` on the
    /// mutex-guarded state means at most one caller ever sees `Some(pid)`,
    /// the other sees `None` and no-ops — no double-kill, no panic.
    ///
    /// **What this does NOT cover (known residual, not fixed here):** if a
    /// single AI turn requests MULTIPLE tool_use calls in one batch (see
    /// `ai_adapter.rs`'s inner `for (tool_id, name, input) in tool_uses`
    /// loop — cancellation is only checked once per OUTER turn iteration,
    /// not between individual tool_use dispatches in the same batch), a
    /// connection drop between two dispatches in that batch can still leak:
    /// the first dispatch's timeout/teardown already cleared `peer`/
    /// `child_pid` via `close_connection`, so the batch's NEXT `dispatch()`
    /// call sees no cached peer, spawns a brand-new `mcp-nmap.exe` via
    /// `ensure_connected`, and if THAT call succeeds normally (the common
    /// case), nothing ever kills it — `shutdown()` already ran once for
    /// this connection and won't run again. Narrower than the pre-0.40.31
    /// bug (requires a batched multi-tool_use turn AND a drop timed exactly
    /// mid-batch), but real. Not closed in this commit — see CHANGELOG.
    async fn close_connection(&self) {
        *self.peer.lock().await = None;
        if let Some(pid) = self.child_pid.lock().await.take() {
            self.killer.kill_tree(pid);
        }
    }

    /// Calls one of the two nmap tools by name, with a `{target}` argument,
    /// and deserializes the sidecar's `ScanOutcome` JSON into a
    /// `DispatchOutcome` — `report: Some(...)` unless `is_error` (an error
    /// has no report to show — design spec §7: PATH-missing/malformed-XML/
    /// timeout are tool errors, not reports).
    async fn call_scan_tool(&self, tool_name: &str, target: &str) -> DispatchOutcome {
        use rmcp::model::CallToolRequestParams;
        use serde_json::json;

        let peer = match self.ensure_connected().await {
            Ok(p) => p,
            Err(e) => return DispatchOutcome { output: e, is_error: true, report: None, channel_summary: None },
        };

        let mut args = serde_json::Map::new();
        args.insert("target".to_string(), json!(target));
        let params = CallToolRequestParams::new(tool_name.to_string()).with_arguments(args);

        let call_future = peer.call_tool(params);
        let result = match tokio::time::timeout(
            std::time::Duration::from_secs(NMAP_CALL_TIMEOUT_SECS),
            call_future,
        )
        .await
        {
            Ok(r) => r,
            Err(_elapsed) => {
                self.close_connection().await;
                return DispatchOutcome {
                    output: format!("\u{26a0} Timeout: lo scan ha superato {NMAP_CALL_TIMEOUT_SECS}s ed \u{e8} stato annullato."),
                    is_error: true,
                    report: None,
                    channel_summary: None,
                };
            }
        };

        match result {
            Err(e) => {
                *self.peer.lock().await = None;
                DispatchOutcome { output: format!("call_tool {tool_name} failed: {e}"), is_error: true, report: None, channel_summary: None }
            }
            Ok(tool_result) => {
                let json_text = tool_result
                    .content
                    .iter()
                    .find_map(|c| c.as_text().map(|t| t.text.clone()))
                    .unwrap_or_default();
                match serde_json::from_str::<NmapScanOutcomeJson>(&json_text) {
                    Ok(out) => DispatchOutcome {
                        // `defer_to_turn_end: false` — uno scan nmap è già completo al
                        // ritorno di questa chiamata (nessuna narrativa AI da attendere),
                        // stesso comportamento storico: la finestra si apre subito.
                        report: if out.is_error { None } else { Some(ChannelReport { title: format!("Scansione nmap — {target}"), markdown: out.report_markdown, defer_to_turn_end: false }) },
                        output: out.summary,
                        is_error: out.is_error,
                        channel_summary: None,
                    },
                    Err(e) => DispatchOutcome {
                        output: format!("failed to parse mcp-nmap response: {e}; raw: {json_text:?}"),
                        is_error: true,
                        report: None,
                        channel_summary: None,
                    },
                }
            }
        }
    }

    /// Calls `local_network_info`/`traceroute` — same lazy-connect/timeout
    /// machinery as `call_scan_tool`, but deserializes `NmapInfoOutcomeJson`
    /// (no `report_markdown`) and never populates `DispatchOutcome.report`:
    /// this data goes straight to the model, not to a window/Library (see
    /// `network_info.rs`'s module doc). `target` is `None` for
    /// `local_network_info` (no parameters), `Some(t)` for `traceroute`.
    async fn call_info_tool(&self, tool_name: &str, target: Option<&str>) -> DispatchOutcome {
        use rmcp::model::CallToolRequestParams;

        let peer = match self.ensure_connected().await {
            Ok(p) => p,
            Err(e) => return DispatchOutcome { output: e, is_error: true, report: None, channel_summary: None },
        };

        let mut args = serde_json::Map::new();
        if let Some(t) = target {
            args.insert("target".to_string(), serde_json::json!(t));
        }
        let params = CallToolRequestParams::new(tool_name.to_string()).with_arguments(args);

        let call_future = peer.call_tool(params);
        let result = match tokio::time::timeout(
            std::time::Duration::from_secs(NMAP_CALL_TIMEOUT_SECS),
            call_future,
        )
        .await
        {
            Ok(r) => r,
            Err(_elapsed) => {
                self.close_connection().await;
                return DispatchOutcome {
                    output: format!("\u{26a0} Timeout: {tool_name} ha superato {NMAP_CALL_TIMEOUT_SECS}s ed \u{e8} stato annullato."),
                    is_error: true,
                    report: None,
                    channel_summary: None,
                };
            }
        };

        match result {
            Err(e) => {
                *self.peer.lock().await = None;
                DispatchOutcome { output: format!("call_tool {tool_name} failed: {e}"), is_error: true, report: None, channel_summary: None }
            }
            Ok(tool_result) => {
                let json_text = tool_result
                    .content
                    .iter()
                    .find_map(|c| c.as_text().map(|t| t.text.clone()))
                    .unwrap_or_default();
                match serde_json::from_str::<NmapInfoOutcomeJson>(&json_text) {
                    Ok(out) => DispatchOutcome { output: out.output, is_error: out.is_error, report: None, channel_summary: None },
                    Err(e) => DispatchOutcome {
                        output: format!("failed to parse {tool_name} response: {e}; raw: {json_text:?}"),
                        is_error: true,
                        report: None,
                        channel_summary: None,
                    },
                }
            }
        }
    }
}

#[async_trait]
impl ToolClient for NmapToolClient {
    /// Kills the connected `mcp-nmap.exe` process (if any) when this
    /// connection tears down — see `close_connection`'s doc comment. Fixes
    /// the more common of the two ways `mcp-nmap.exe` used to leak
    /// permanently: closing the `/nmap` window normally (even after a
    /// fully successful scan) never touched the spawned process before
    /// this, exactly like an un-killed timeout did.
    async fn shutdown(&self) {
        self.close_connection().await;
    }

    /// Chiuso strutturalmente: la via `Route::Os` di `core.rs` chiama questo
    /// direttamente per una connessione sul canale "nmap" — non delega mai a
    /// una shell reale (Docs/superpowers/specs/2026-07-16-tool-isolation-
    /// design.md).
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

    /// Chiuso strutturalmente: la via `Route::Slash` (`/open`) non delega mai
    /// a un target reale su questo canale.
    async fn open_target(&self, _target: &str) -> OpenResult {
        OpenResult { ok: false, message: "open_target non disponibile su questo canale".to_string() }
    }

    /// Chiuso strutturalmente: questo canale non conosce il repository di
    /// routine (che vive dietro `run_in_session`, mai esposto qui).
    async fn search_routines(&self, _query: Option<&str>) -> crate::tool_client::SearchRoutinesResult {
        crate::tool_client::SearchRoutinesResult {
            results: Vec::new(),
            error: Some("search_routines non disponibile su questo canale".to_string()),
        }
    }

    /// Chiuso strutturalmente: stesso motivo di `search_routines` sopra.
    async fn run_routine(&self, _name: &str, _args: Option<&str>) -> crate::tool_client::RunRoutineResult {
        crate::tool_client::RunRoutineResult {
            ok: false,
            message: "run_routine non disponibile su questo canale".to_string(),
            stdout: String::new(),
            stderr: String::new(),
            exit_code: -1,
            cwd: String::new(),
        }
    }

    fn tool_defs(&self) -> Vec<ToolDef> {
        vec![
            ToolDef {
                name: "nmap_quick_scan".to_string(),
                description: "Scansione nmap rapida (TCP connect, -sT, nessun privilegio elevato richiesto).".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": { "target": { "type": "string", "description": "IP, range CIDR, o hostname da scansionare" } },
                    "required": ["target"]
                }),
            },
            ToolDef {
                name: "nmap_os_detect".to_string(),
                description: "Rilevamento OS nmap (-O). Richiede privilegi elevati (prompt UAC).".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": { "target": { "type": "string", "description": "IP, range CIDR, o hostname da scansionare" } },
                    "required": ["target"]
                }),
            },
            ToolDef {
                name: "nmap_version_scan".to_string(),
                description: "Scansione nmap con rilevamento versioni dei servizi (-sV, nessun privilegio elevato richiesto).".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": { "target": { "type": "string", "description": "IP, range CIDR, o hostname da scansionare" } },
                    "required": ["target"]
                }),
            },
            ToolDef {
                name: "nmap_host_discovery".to_string(),
                description: "Scoperta host attivi su una rete/range (-sn, nessuna scansione porte, nessun privilegio elevato).".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": { "target": { "type": "string", "description": "IP, range CIDR, o hostname/rete da esplorare" } },
                    "required": ["target"]
                }),
            },
            ToolDef {
                name: "nmap_vuln_scan".to_string(),
                description: "Scansione vulnerabilità note tramite gli script NSE 'vuln' integrati in nmap (--script vuln, nessun privilegio elevato richiesto).".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": { "target": { "type": "string", "description": "IP, range CIDR, o hostname da sottoporre a scansione vulnerabilità" } },
                    "required": ["target"]
                }),
            },
            ToolDef {
                name: "local_network_info".to_string(),
                description: "Informazioni di rete della macchina locale (IP, subnet, gateway, tabella ARP/routing). Nessun parametro. Usa PRIMA di uno scan quando l'utente non specifica un target, per determinarlo da solo.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
            ToolDef {
                name: "traceroute".to_string(),
                description: "Traccia il percorso di rete verso un host.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": { "target": { "type": "string", "description": "IP o hostname" } },
                    "required": ["target"]
                }),
            },
            ToolDef {
                name: "fritzbox_status".to_string(),
                description: "Stato del router FRITZ!Box di casa: registro eventi, IP pubblico, dispositivi LAN.".to_string(),
                input_schema: serde_json::json!({ "type": "object", "properties": {} }),
            },
        ]
    }

    async fn dispatch(&self, name: &str, input: &serde_json::Value) -> DispatchOutcome {
        let target = input.get("target").and_then(|v| v.as_str()).unwrap_or("");
        match name {
            "nmap_quick_scan" => self.call_scan_tool("nmap_quick_scan", target).await,
            "nmap_os_detect" => self.call_scan_tool("nmap_os_detect", target).await,
            "nmap_version_scan" => self.call_scan_tool("nmap_version_scan", target).await,
            "nmap_host_discovery" => self.call_scan_tool("nmap_host_discovery", target).await,
            "nmap_vuln_scan" => self.call_scan_tool("nmap_vuln_scan", target).await,
            "local_network_info" => self.call_info_tool("local_network_info", None).await,
            "traceroute" => self.call_info_tool("traceroute", Some(target)).await,
            "fritzbox_status" => self.call_info_tool("fritzbox_status", None).await,
            other => DispatchOutcome { output: format!("tool sconosciuto sul canale nmap: {other}"), is_error: true, report: None, channel_summary: None },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RED (Task 4, brief Step 3): `NmapToolClient::resolve` prende
    /// `config_dir`/`StartupConfig` come parametri espliciti — niente più
    /// `LARE_MCP_NMAP` (D6). Mirror esatto del test analogo per
    /// `McpToolClient::resolve` in `tool_client.rs`.
    #[test]
    fn mcp_nmap_path_comes_from_startup_paths() {
        let cfg = startup_config::StartupConfig::default();
        let c = NmapToolClient::resolve(std::path::Path::new("C:/Lare/Configuration"), &cfg);
        assert_eq!(c.mcp_nmap_path, std::path::Path::new("C:/Lare").join("mcp-nmap.exe"));
        assert_eq!(c.config_dir, std::path::PathBuf::from("C:/Lare/Configuration"));
    }

    #[derive(Default)]
    struct FakeProcessTreeKiller {
        killed_pids: std::sync::Mutex<Vec<u32>>,
    }

    impl ProcessTreeKiller for FakeProcessTreeKiller {
        fn kill_tree(&self, pid: u32) {
            self.killed_pids.lock().unwrap().push(pid);
        }
    }

    #[tokio::test]
    async fn close_connection_kills_the_stored_pid_and_clears_state() {
        let killer = Arc::new(FakeProcessTreeKiller::default());
        let client = NmapToolClient {
            mcp_nmap_path: "unused".into(),
            config_dir: "unused".into(),
            peer: Arc::new(Mutex::new(None)),
            child_pid: Arc::new(Mutex::new(Some(4242))),
            killer: killer.clone(),
        };
        client.close_connection().await;
        assert_eq!(killer.killed_pids.lock().unwrap().as_slice(), &[4242]);
        assert!(client.child_pid.lock().await.is_none(), "child_pid must be cleared after closing");
        assert!(client.peer.lock().await.is_none(), "peer must be cleared after closing");
    }

    #[tokio::test]
    async fn close_connection_does_not_kill_when_never_connected() {
        let killer = Arc::new(FakeProcessTreeKiller::default());
        let client = NmapToolClient {
            mcp_nmap_path: "unused".into(),
            config_dir: "unused".into(),
            peer: Arc::new(Mutex::new(None)),
            child_pid: Arc::new(Mutex::new(None)),
            killer: killer.clone(),
        };
        client.close_connection().await;
        assert!(
            killer.killed_pids.lock().unwrap().is_empty(),
            "must not call kill_tree when no pid was ever stored"
        );
    }

    #[tokio::test]
    async fn shutdown_kills_the_connected_process() {
        use crate::tool_client::ToolClient;
        let killer = Arc::new(FakeProcessTreeKiller::default());
        let client = NmapToolClient {
            mcp_nmap_path: "unused".into(),
            config_dir: "unused".into(),
            peer: Arc::new(Mutex::new(None)),
            child_pid: Arc::new(Mutex::new(Some(9001))),
            killer: killer.clone(),
        };
        client.shutdown().await;
        assert_eq!(killer.killed_pids.lock().unwrap().as_slice(), &[9001]);
    }

    #[tokio::test]
    async fn run_in_session_never_executes_a_real_shell() {
        let client = NmapToolClient {
            mcp_nmap_path: "unused".into(),
            config_dir: "unused".into(),
            peer: Arc::new(Mutex::new(None)),
            child_pid: Arc::new(Mutex::new(None)),
            killer: Arc::new(FakeProcessTreeKiller::default()),
        };
        let result = client.run_in_session("dir", None).await;
        assert_eq!(result.exit_code, -1);
        assert!(result.stderr.contains("non disponibile su questo canale"));
    }

    #[tokio::test]
    async fn open_target_never_opens_anything() {
        let client = NmapToolClient {
            mcp_nmap_path: "unused".into(),
            config_dir: "unused".into(),
            peer: Arc::new(Mutex::new(None)),
            child_pid: Arc::new(Mutex::new(None)),
            killer: Arc::new(FakeProcessTreeKiller::default()),
        };
        let result = client.open_target("C:\\Users").await;
        assert!(!result.ok);
        assert!(result.message.contains("non disponibile su questo canale"));
    }

    #[test]
    fn tool_defs_exposes_exactly_the_eight_nmap_channel_tools() {
        let client = NmapToolClient {
            mcp_nmap_path: "unused".into(),
            config_dir: "unused".into(),
            peer: Arc::new(Mutex::new(None)),
            child_pid: Arc::new(Mutex::new(None)),
            killer: Arc::new(FakeProcessTreeKiller::default()),
        };
        let defs = client.tool_defs();
        assert_eq!(defs.len(), 8, "atteso 8 tool (5 scan + 3 info), got {defs:?}");
        assert!(defs.iter().any(|d| d.name == "nmap_quick_scan"));
        assert!(defs.iter().any(|d| d.name == "nmap_os_detect"));
        assert!(defs.iter().any(|d| d.name == "nmap_version_scan"));
        assert!(defs.iter().any(|d| d.name == "nmap_host_discovery"));
        assert!(defs.iter().any(|d| d.name == "nmap_vuln_scan"));
        assert!(defs.iter().any(|d| d.name == "local_network_info"));
        assert!(defs.iter().any(|d| d.name == "traceroute"));
        assert!(defs.iter().any(|d| d.name == "fritzbox_status"));
        assert!(!defs.iter().any(|d| d.name == "run_in_session"));
        assert!(!defs.iter().any(|d| d.name == "show_markdown"));
    }

    #[test]
    fn nmap_info_outcome_json_parses_success_shape() {
        let json = r#"{"output":"ipconfig output here","is_error":false}"#;
        let parsed: NmapInfoOutcomeJson = serde_json::from_str(json).expect("should parse");
        assert_eq!(parsed.output, "ipconfig output here");
        assert!(!parsed.is_error);
    }

    #[test]
    fn nmap_info_outcome_json_parses_error_shape() {
        let json = r#"{"output":"target non valido","is_error":true}"#;
        let parsed: NmapInfoOutcomeJson = serde_json::from_str(json).expect("should parse");
        assert!(parsed.is_error);
    }

    #[test]
    fn nmap_scan_outcome_json_parses_success_shape() {
        let json = r##"{"summary":"1 host attivo","report_markdown":"# Report","is_error":false}"##;
        let parsed: NmapScanOutcomeJson = serde_json::from_str(json).expect("should parse");
        assert_eq!(parsed.summary, "1 host attivo");
        assert!(!parsed.is_error);
    }

    #[test]
    fn nmap_scan_outcome_json_parses_error_shape() {
        let json = r#"{"summary":"nmap non trovato sul PATH","report_markdown":"","is_error":true}"#;
        let parsed: NmapScanOutcomeJson = serde_json::from_str(json).expect("should parse");
        assert!(parsed.is_error);
        assert!(parsed.report_markdown.is_empty());
    }
}
