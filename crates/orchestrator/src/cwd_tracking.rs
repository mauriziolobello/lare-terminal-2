//! # cwd_tracking — Decorator that tracks the current working directory
//!
//! `CwdTrackingToolClient` wraps any `ToolClient` and updates a shared
//! `Arc<Mutex<String>>` after every `run_in_session` call if the returned
//! `CommandResult.cwd` is non-empty.
//!
//! ## Design (SOLID — Decorator pattern, OCP/SRP)
//! - The tracker has a single responsibility: propagate cwd changes to shared state.
//! - It delegates all tool calls to the `inner` client — no logic duplication.
//! - Both the OS-direct path and the AI tool-use path go through the same
//!   `ToolClient` trait, so wrapping with this decorator covers both with one
//!   hook.
//!
//! ## Shared state
//! The `cwd_state: Arc<tokio::sync::Mutex<String>>` is the single source of truth
//! for the current shell cwd inside the orchestrator.  It is:
//! - Initialised in `main.rs` to `home_dir()` (or current process dir as fallback).
//! - Written here after each `run_in_session` if `result.cwd` is non-empty.
//! - Read by `/find` (ws.rs) and by the Cwd-emit logic (ws.rs) on each connection.

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::tool_client::{CommandResult, GetRoutineContentResult, OpenResult, RunRoutineResult, SaveRoutineResult, SearchRoutinesResult, ToolClient};

/// Wraps a `ToolClient` and updates `cwd_state` after every `run_in_session`.
///
/// The inner client is stored as `Arc<dyn ToolClient>` so the decorator can be
/// wrapped in another `Arc` and shared across tasks without copying the inner.
pub struct CwdTrackingToolClient {
    inner: Arc<dyn ToolClient>,
    /// Shared cwd state updated after each command.
    pub cwd_state: Arc<Mutex<String>>,
    /// Cartella di configurazione (2.0, D6) — riusata SOLO da `dispatch()`
    /// per `agent::dispatch_tool_at`'s `network_json_path`
    /// (`set_ai_display_name`), mai ri-derivata: stesso `config_dir` di
    /// `RuntimeConfig`, ricevuto a costruzione da `main.rs`.
    config_dir: std::path::PathBuf,
}

impl CwdTrackingToolClient {
    /// Create a new tracking decorator.
    ///
    /// # Arguments
    /// * `inner`      — The underlying tool client to delegate to.
    /// * `cwd_state`  — Shared state to update with the cwd from each result.
    /// * `config_dir` — Config dir (2.0, D6), used only by `dispatch()`'s
    ///   `set_ai_display_name` branch.
    pub fn new(inner: Arc<dyn ToolClient>, cwd_state: Arc<Mutex<String>>, config_dir: std::path::PathBuf) -> Self {
        Self { inner, cwd_state, config_dir }
    }

    /// Costruttore di comodo per i test di QUESTO file: nessuno di essi
    /// esercita il ramo `set_ai_display_name` di `dispatch()` (quello che
    /// legge/scrive `config_dir`), quindi un placeholder basta.
    #[cfg(test)]
    fn new_for_test(inner: Arc<dyn ToolClient>, cwd_state: Arc<Mutex<String>>) -> Self {
        Self::new(inner, cwd_state, std::path::PathBuf::from("/test-config"))
    }
}

#[async_trait]
impl ToolClient for CwdTrackingToolClient {
    async fn run_in_session(
        &self,
        command: &str,
        progress_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
    ) -> CommandResult {
        let result = self.inner.run_in_session(command, progress_tx).await;
        // Update shared cwd if the shell reported one (non-empty).
        // Empty means mcp-server didn't report it (older version or error path):
        // keep the previous value in that case.
        if !result.cwd.is_empty() {
            *self.cwd_state.lock().await = result.cwd.clone();
        }
        result
    }

    async fn reset_session(&self) {
        self.inner.reset_session().await;
    }

    async fn open_target(&self, target: &str) -> OpenResult {
        // open_target does not affect cwd — just delegate.
        self.inner.open_target(target).await
    }

    async fn search_routines(&self, query: Option<&str>) -> SearchRoutinesResult {
        // Read-only — does not affect cwd, just delegate (like open_target).
        self.inner.search_routines(query).await
    }

    async fn run_routine(&self, name: &str, args: Option<&str>) -> RunRoutineResult {
        // `run_routine` runs in the SAME persistent shell session as
        // `run_in_session` (mcp-server delegates to `Session::run` — see
        // `RunRoutineResult.cwd`, which mirrors `CommandResult.cwd`
        // exactly). A routine's script can `cd`/`Set-Location` just like any
        // other command, so it must update `cwd_state` the same way
        // `run_in_session` does above — otherwise `/find` and the UI would
        // silently read a stale cwd after running a routine that changes
        // directory.
        let result = self.inner.run_routine(name, args).await;
        if !result.cwd.is_empty() {
            *self.cwd_state.lock().await = result.cwd.clone();
        }
        result
    }

    async fn get_routine_content(&self, name: &str) -> GetRoutineContentResult {
        // Sola lettura — non tocca la cwd, delega semplicemente (come open_target).
        self.inner.get_routine_content(name).await
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
        // save_routine non gira nella shell persistente (mcp-server lo esegue
        // come scrittura file diretta, mai via Session::run) — non aggiorna
        // mai cwd_state, a differenza di run_in_session/run_routine sopra.
        self.inner.save_routine(name, description, tags, category, content, replace).await
    }

    async fn dispatch(&self, name: &str, input: &serde_json::Value) -> crate::tool_client::DispatchOutcome {
        // IMPORTANT: dispatch on `self`, NOT `self.inner` — this re-enters
        // `agent::dispatch_tool_at` with THIS decorator as the receiver, so a
        // "run_in_session" tool_use routes back through `self.run_in_session`
        // (the override above that updates `cwd_state`), not straight to the
        // inner client (which would silently skip cwd tracking on the AI
        // tool-use path — found in review of commit 39370d4).
        // `self.config_dir` (2.0, D6): niente ri-derivazione qui.
        crate::agent::dispatch_tool_at(self as &dyn ToolClient, name, input, &self.config_dir.join("network.json")).await
    }
}

// ── Tests — TDD: RED then GREEN ───────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_client::FakeToolClient;

    /// After `run_in_session` with a cwd-reporting fake, `cwd_state` is updated.
    #[tokio::test]
    async fn tracking_updates_cwd_state_when_cwd_non_empty() {
        let fake = Arc::new(FakeToolClient::with_cwd("out", "/home/user/projects"));
        let cwd_state = Arc::new(Mutex::new("/home/user".to_string()));
        let tracker = CwdTrackingToolClient::new_for_test(fake, Arc::clone(&cwd_state));

        tracker.run_in_session("cd projects", None).await;

        assert_eq!(*cwd_state.lock().await, "/home/user/projects");
    }

    /// `dispatch("run_in_session", ...)` must ALSO update `cwd_state` — not just
    /// the direct `run_in_session` call. `dispatch` re-enters `agent::dispatch_tool_at`
    /// with `self` (the tracker), so the resulting `run_in_session` call must land
    /// on THIS decorator's own override (the one that updates `cwd_state`), never
    /// straight on `self.inner` (which would silently skip cwd tracking — the bug
    /// found in review of commit 39370d4, where `dispatch` delegated to
    /// `self.inner.dispatch` instead of re-dispatching through `self`).
    #[tokio::test]
    async fn dispatch_run_in_session_updates_cwd_state() {
        let fake = Arc::new(FakeToolClient::with_cwd("out", "/home/user/projects"));
        let cwd_state = Arc::new(Mutex::new("/home/user".to_string()));
        let tracker = CwdTrackingToolClient::new_for_test(fake, Arc::clone(&cwd_state));

        tracker
            .dispatch(
                "run_in_session",
                &serde_json::json!({"command": "cd projects"}),
            )
            .await;

        assert_eq!(*cwd_state.lock().await, "/home/user/projects");
    }

    /// `dispatch("run_routine", ...)` must ALSO update `cwd_state` — same
    /// principle as `dispatch_run_in_session_updates_cwd_state` above, but
    /// for the new tool: `run_routine` runs in the same persistent shell, so
    /// a routine that `cd`s must be tracked exactly like a raw command does.
    #[tokio::test]
    async fn dispatch_run_routine_updates_cwd_state() {
        let fake = Arc::new(FakeToolClient::with_cwd("out", "/home/user/projects"));
        let cwd_state = Arc::new(Mutex::new("/home/user".to_string()));
        let tracker = CwdTrackingToolClient::new_for_test(fake, Arc::clone(&cwd_state));

        tracker
            .dispatch("run_routine", &serde_json::json!({"name": "list-big-files"}))
            .await;

        assert_eq!(*cwd_state.lock().await, "/home/user/projects");
    }

    /// When the fake returns empty cwd, `cwd_state` is NOT changed.
    #[tokio::test]
    async fn tracking_does_not_overwrite_cwd_when_result_cwd_is_empty() {
        let fake = Arc::new(FakeToolClient::success("out"));
        let cwd_state = Arc::new(Mutex::new("/original".to_string()));
        let tracker = CwdTrackingToolClient::new_for_test(fake, Arc::clone(&cwd_state));

        tracker.run_in_session("echo hi", None).await;

        assert_eq!(*cwd_state.lock().await, "/original");
    }

    /// `run_in_session` result is transparently forwarded by the decorator.
    #[tokio::test]
    async fn tracking_propagates_result_stdout() {
        let fake = Arc::new(FakeToolClient::success("hello\n"));
        let cwd_state = Arc::new(Mutex::new(String::new()));
        let tracker = CwdTrackingToolClient::new_for_test(fake, cwd_state);

        let result = tracker.run_in_session("echo hello", None).await;

        assert_eq!(result.stdout, "hello\n");
        assert_eq!(result.exit_code, 0);
    }

    /// After multiple commands, cwd_state reflects the LAST non-empty cwd.
    #[tokio::test]
    async fn tracking_reflects_most_recent_cwd() {
        // First command returns cwd "/a", second returns "/b".
        // FakeToolClient returns the same cwd for every call; we test via
        // two separate tracker instances sharing the same state.
        let cwd_state = Arc::new(Mutex::new("/initial".to_string()));

        let fake_a = Arc::new(FakeToolClient::with_cwd("", "/a"));
        let tracker_a =
            CwdTrackingToolClient::new_for_test(fake_a, Arc::clone(&cwd_state));
        tracker_a.run_in_session("cd /a", None).await;
        assert_eq!(*cwd_state.lock().await, "/a");

        let fake_b = Arc::new(FakeToolClient::with_cwd("", "/b"));
        let tracker_b =
            CwdTrackingToolClient::new_for_test(fake_b, Arc::clone(&cwd_state));
        tracker_b.run_in_session("cd /b", None).await;
        assert_eq!(*cwd_state.lock().await, "/b");
    }

    /// `CwdTrackingToolClient` is itself dyn-compatible (implements `ToolClient`).
    #[tokio::test]
    async fn tracking_is_dyn_compatible() {
        let fake = Arc::new(FakeToolClient::success("ok"));
        let cwd_state = Arc::new(Mutex::new(String::new()));
        let tracker: Arc<dyn ToolClient> =
            Arc::new(CwdTrackingToolClient::new_for_test(fake, cwd_state));

        let result = tracker.run_in_session("any", None).await;
        assert_eq!(result.exit_code, 0);
    }

    /// `open_target` is delegated and result forwarded.
    #[tokio::test]
    async fn open_target_is_delegated() {
        let fake = Arc::new(FakeToolClient::success(""));
        let cwd_state = Arc::new(Mutex::new(String::new()));
        let tracker = CwdTrackingToolClient::new_for_test(fake, cwd_state);

        let result = tracker.open_target("http://example.com").await;
        assert!(result.ok);
    }

    /// `get_routine_content` is delegated to inner.
    /// Without an override, `CwdTrackingToolClient` inherits the trait's REJECTING
    /// default, not the inner `FakeToolClient`'s override. The test verifies that
    /// the decorator properly forwards to the inner client (error: None from Fake),
    /// not the trait default (error: Some("non disponibile...")).
    #[tokio::test]
    async fn get_routine_content_delegates_to_inner() {
        let fake = Arc::new(FakeToolClient::success(""));
        let cwd_state = Arc::new(Mutex::new(String::new()));
        let tracker = CwdTrackingToolClient::new_for_test(fake, cwd_state);

        let r = tracker.get_routine_content("x").await;
        assert!(!r.found);
        assert!(r.error.is_none(), "atteso il default del Fake (error:None), non quello del trait: {r:?}");
    }

    /// `save_routine` is delegated to inner.
    /// Without an override, `CwdTrackingToolClient` inherits the trait's REJECTING
    /// default, not the inner `FakeToolClient`'s override. The test verifies that
    /// the decorator properly forwards to the inner client (ok: true from Fake),
    /// not the trait default (ok: false).
    #[tokio::test]
    async fn save_routine_delegates_to_inner() {
        let fake = Arc::new(FakeToolClient::success(""));
        let cwd_state = Arc::new(Mutex::new(String::new()));
        let tracker = CwdTrackingToolClient::new_for_test(fake, cwd_state);

        let r = tracker.save_routine("n", "d", vec![], "c", "content", None).await;
        assert!(r.ok, "atteso il default del Fake (ok:true): {r:?}");
    }
}
