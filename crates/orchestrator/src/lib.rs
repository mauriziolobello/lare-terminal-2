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
//! - WS listens on `127.0.0.1:7331` only (never 0.0.0.0).
//! - First message from client must be `Hello{token}` with the correct token;
//!   wrong token → connection closed immediately.
//! - `mcp-server` validates `cwd` (UNC/NTLM guard, Fase 1).

pub mod agent;
pub mod aichat;
pub mod ai_adapter;
pub mod chat_backend;
pub mod claude_backend;
pub mod openrouter_backend;
pub mod plugins;
pub mod token_store;
pub mod core;
pub mod cwd_tracking;
pub mod external_channel;
pub mod llms_config;
pub mod local_confirm;
pub mod messages_client;
pub mod nmap_tool_client;
pub mod notes;
pub mod python_mcp_tool_client;
pub mod router;
pub mod runtime_config;
pub mod search;
pub mod telegram;
pub mod tool_client;
pub mod ws;

#[cfg(test)]
pub(crate) mod test_support;
