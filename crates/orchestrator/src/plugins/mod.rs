//! Plugin host: discovery, spawn-policy, transport, lifecycle (Fase 0).
//!
//! Struttura del modulo:
//! - `discovery`   — scandisce `plugins/`, parse del manifest, localizza il binario.
//! - `spawn_policy`— regole pure trigger -> SpawnPolicy (Eager/Lazy).
//! - `transport`   — trait `PluginTransport` + impl reale (child-stdio) + fake (test). [Task 4]
//! - `host`        — `PluginHost`: lifecycle Init/Deinit per gli eager. [Task 5]
pub mod discovery;
pub mod host;
pub mod spawn_policy;
pub mod transport;
