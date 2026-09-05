//! # search::pause — PauseGate
//!
//! Cooperative pause/resume primitive for blocking search walkers.
//!
//! `PauseGate` wraps a `Mutex<bool>` + `Condvar` pair so that walker threads
//! running inside `tokio::task::spawn_blocking` can park themselves without
//! burning CPU and without blocking the async runtime.
//!
//! ## Design rationale
//! - The walk runs in a blocking thread (spawn_blocking), so a standard
//!   `std::sync::Condvar` park is the correct primitive — not an async channel.
//! - `wait_while_paused` polls the `CancellationToken` with a short `wait_timeout`
//!   so that a cancel received during a pause is noticed within ~100 ms.
//! - `PauseGate::new()` returns `Arc<Self>` directly so callers never need to
//!   wrap it manually (reduces boilerplate at every call site).

use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Cooperative pause/resume gate for blocking search walkers.
///
/// Created via [`PauseGate::new`] which returns `Arc<Self>` directly.
/// Shared between the async `ws` handler (calls `pause`/`resume`) and the
/// blocking walker threads (call `wait_while_paused`).
pub struct PauseGate {
    paused: Mutex<bool>,
    cvar: Condvar,
}

impl PauseGate {
    /// Create a new, non-paused gate wrapped in an `Arc`.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            paused: Mutex::new(false),
            cvar: Condvar::new(),
        })
    }

    /// Set the gate to paused state.
    /// Walkers calling `wait_while_paused` will block until `resume` is called.
    pub fn pause(&self) {
        *self.paused.lock().unwrap() = true;
    }

    /// Resume: clear the paused flag and wake all waiting walker threads.
    pub fn resume(&self) {
        *self.paused.lock().unwrap() = false;
        self.cvar.notify_all();
    }

    /// Returns `true` if the gate is currently in the paused state.
    pub fn is_paused(&self) -> bool {
        *self.paused.lock().unwrap()
    }

    /// Park the calling (blocking) thread while paused; return when resumed or cancelled.
    ///
    /// Polls `cancel` via a short `wait_timeout` so a cancel during pause is
    /// noticed within ~100 ms. Safe to call from `spawn_blocking` threads.
    pub fn wait_while_paused(&self, cancel: &CancellationToken) {
        let mut paused = self.paused.lock().unwrap();
        while *paused && !cancel.is_cancelled() {
            let (g, _) = self.cvar.wait_timeout(paused, Duration::from_millis(100)).unwrap();
            paused = g;
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests (written before implementation — RED → GREEN)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::PauseGate;

    #[test]
    fn pause_and_is_paused() {
        let g = PauseGate::new();
        assert!(!g.is_paused());
        g.pause();
        assert!(g.is_paused());
        g.resume();
        assert!(!g.is_paused());
    }

    #[test]
    fn wait_parks_until_resume() {
        use std::sync::Arc;
        use std::thread;
        use std::time::Duration;
        let g = PauseGate::new();
        let cancel = tokio_util::sync::CancellationToken::new();
        g.pause();
        let g2 = Arc::clone(&g);
        let c2 = cancel.clone();
        let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let d2 = Arc::clone(&done);
        let h = thread::spawn(move || {
            g2.wait_while_paused(&c2);
            d2.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        thread::sleep(Duration::from_millis(50));
        assert!(!done.load(std::sync::atomic::Ordering::SeqCst), "deve restare parcheggiato");
        g.resume();
        h.join().unwrap();
        assert!(done.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn wait_released_by_cancel() {
        use std::sync::Arc;
        use std::thread;
        let g = PauseGate::new();
        let cancel = tokio_util::sync::CancellationToken::new();
        g.pause();
        let g2 = Arc::clone(&g);
        let c2 = cancel.clone();
        let h = thread::spawn(move || g2.wait_while_paused(&c2));
        cancel.cancel();
        h.join().unwrap(); // si libera entro il timeout di poll
    }
}
