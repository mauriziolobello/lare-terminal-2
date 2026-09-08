//! # orchestrator
//!
//! Lare Terminal daemon — the heart of Step 1.
//!
//! ## Architecture (SOLID)
//!
//! The crate is split into transport-agnostic core modules and a WebSocket
//! transport layer.  The core never imports `tokio-tungstenite`; the WS layer
//! never contains business logic.
//!
//! ```text
//! ┌────────────────────────────────────────────────────────┐
//! │  ws  (WebSocket transport, main.rs glue)               │
//! │  • handshake + token check                             │
//! │  • encode/decode ClientMsg / ServerMsg                 │
//! │  • calls core::handle_command                          │
//! └───────────────────────┬────────────────────────────────┘
//!                         │  mpsc::UnboundedSender<ServerMsg>  (Slice 4)
//! ┌───────────────────────▼────────────────────────────────┐
//! │  core  (transport-agnostic command handler)            │
//! │  • handle_command(&cmd, &dyn AiAdapter, &dyn ToolClient│
//! │  • emits ServerMsg on tx; calls router::classify       │
//! └──────┬─────────────────────────────────────────────────┘
//!        │
//!  ┌─────▼──────┐    ┌──────────────┐    ┌───────────────┐
//!  │   router   │    │  ai_adapter  │    │  tool_client  │
//!  │ (pure fn)  │    │  (trait)     │    │  (trait)      │
//!  └────────────┘    │  StubAdapter │    │ McpToolClient │
//!                    └──────────────┘    │ FakeToolClient│
//!                                        └───────────────┘
//! ```
//!
//! ## Configuration (2.0 — nessuna env var, decisione D6)
//! Tutto viene da `--config-dir <dir>` (o `<exe_dir>/Configuration` di
//! default) + `<dir>/startup.json` — vedi il crate `startup-config` e
//! `runtime_config::RuntimeConfig`, il "context object" risolto una volta in
//! `main()`. Il token WS vive in `<config_dir>/token` (`token_store`); il
//! path di `mcp-server.exe` viene da `startup.json` (`paths.mcp_server`).
//! Uniche eccezioni (fuori scope D6): `ANTHROPIC_API_KEY`/`OPENROUTER_API_KEY`
//! (chiavi dei provider AI, mai in un file di config committabile).
//!
//! ## Security (ADR-007)
//! - WS listens on `127.0.0.1:<ws_port>` only (never 0.0.0.0) — port from
//!   `startup.json` (`ws_port`, default 7331).
//! - First message from client must be `Hello{token}` with the correct token;
//!   wrong token → connection closed immediately.
//! - `mcp-server` validates `cwd` (UNC/NTLM guard, Fase 1).

use std::sync::LazyLock;
use std::time::Instant;

pub mod agent;
pub mod ai_adapter;
pub mod aichat;
pub mod chat_backend;
pub mod claude_backend;
pub mod connections;
pub mod core;
pub mod cwd_tracking;
pub mod external_channel;
pub mod llms_config;
pub mod local_confirm;
pub mod messages_client;
pub mod nmap_tool_client;
pub mod notes;
pub mod openrouter_backend;
pub mod ping;
pub mod plugins;
pub mod python_mcp_tool_client;
pub mod router;
pub mod runtime_config;
pub mod search;
pub mod shell_session;
pub mod shell_slash;
pub mod shell_turn;
pub mod surface;
pub mod telegram;
pub mod token_store;
pub mod tool_client;
pub mod ws;

/// Istante di avvio del processo, per l'uptime di `/ping`. `main()` lo forza
/// subito (`LazyLock::force`), così vale davvero l'avvio e non il primo ping.
pub static PROCESS_START: LazyLock<Instant> = LazyLock::new(Instant::now);

#[cfg(test)]
pub(crate) mod test_support;
