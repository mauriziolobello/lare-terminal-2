//! # telegram
//!
//! Canale Telegram per Lare Terminal (ADR-007).
//!
//! ## Sottomoduli
//! - `settings` — caricamento token da `telegramsettings.json`
//! - `client`   — trait `TelegramClient` + `HttpTelegramClient` + tipi Bot API
//! - `auth`     — `TelegramState` + `Authenticator` (pairing/TOTP/sessioni/rate-limit)
//! - `gate`     — `needs_confirmation` predicato + `PendingGates` store
//! - `channel`  — polling loop, dispatch, mapping ServerMsg→testo (Task 4)

pub mod auth;
pub mod channel;
pub mod client;
pub mod confirm;
pub mod gate;
pub mod qr;
pub mod settings;

pub use channel::TelegramChannel;

/// Entry-point per il canale Telegram: costruisce e avvia il loop di polling.
///
/// Progettata per essere spawnata come task `'static` con `tokio::spawn`.
/// Tutti i parametri sono owned o `Arc` → la future è `Send + 'static`.
///
/// # Parametri
/// - `client`     — client HTTP Telegram (già avvolto in `Arc` per `'static`).
/// - `auth`       — `Authenticator` con TOTP e stato pairing già inizializzato.
/// - `state_path` — path del file `telegram-state.json` (per persistere il pair).
/// - `ai`         — adapter AI (`Arc<dyn AiAdapter>`).
/// - `tools`      — tool client MCP (`Arc<dyn ToolClient>`).
pub async fn run_channel(
    client: std::sync::Arc<dyn client::TelegramClient>,
    auth: auth::Authenticator,
    state_path: std::path::PathBuf,
    ai: std::sync::Arc<dyn crate::ai_adapter::AiAdapter>,
    tools: std::sync::Arc<dyn crate::tool_client::ToolClient>,
) {
    TelegramChannel::new(client, auth, state_path, ai, tools)
        .run()
        .await;
}
