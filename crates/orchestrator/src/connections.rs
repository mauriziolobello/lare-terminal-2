//! # connections — registro delle connessioni WS vive (2.0, spec §4/§5)
//!
//! In v1 ogni connessione era un mondo a sé: `ws.rs` non sapeva quante ce
//! ne fossero né di che tipo. Nel 2.0 esistono due ruoli (`protocol::Role`)
//! e l'orchestratore deve poter **raggiungere** `ui.exe` da un turno
//! originato da una shell (aprire la finestra di output, chiedere un
//! `UiPong`). Questo modulo è l'unico posto che conosce "chi è connesso":
//!
//! - il **sink `ui`**: la connessione `role: Ui` senza `channel` (una sola
//!   per macchina; l'ultima vince, come per il sink dei plugin);
//! - le **sessioni shell**, per `session_id` (piano 3: push di segnalini);
//! - i **ping `ui` pendenti** (`UiPing{id}` → `oneshot` risolto da `UiPong`).
//!
//! È puro stato + `tokio::sync` (nessun I/O): testabile senza socket.
//! In termini OOP: un registry/service locator posseduto da `main()`,
//! condiviso via `Arc<Mutex<_>>` con ogni `handle_connection`.

use std::collections::HashMap;
use std::sync::Arc;

use protocol::ServerMsg;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::{oneshot, Mutex};

/// Handle condiviso: `ws::serve` lo riceve da `main()` e lo clona per ogni
/// connessione. `tokio::sync::Mutex` perché viene tenuto attraverso `await`
/// solo per operazioni brevissime (mai attraverso una send di rete).
pub type SharedRegistry = Arc<Mutex<Registry>>;

#[derive(Default)]
pub struct Registry {
    ui_sink: Option<UnboundedSender<ServerMsg>>,
    shells: HashMap<String, UnboundedSender<ServerMsg>>,
    ui_pings: HashMap<String, oneshot::Sender<String>>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Costruttore di comodo per `main()`/test: registro vuoto già condivisibile.
    pub fn shared() -> SharedRegistry {
        Arc::new(Mutex::new(Self::new()))
    }

    /// Registra (o sostituisce) il sink `ui`. L'ultima connessione `ui`
    /// senza canale vince — stessa regola di `PluginHost::set_server_tx`.
    pub fn set_ui_sink(&mut self, tx: UnboundedSender<ServerMsg>) {
        self.ui_sink = Some(tx);
    }

    /// Azzera il sink SOLO se è ancora quello di `tx` (`same_channel`): una
    /// connessione `ui` vecchia che si chiude dopo che una nuova ha preso il
    /// posto non deve lasciare la macchina senza sink. Ritorna `true` se ha
    /// azzerato davvero.
    pub fn clear_ui_sink_if(&mut self, tx: &UnboundedSender<ServerMsg>) -> bool {
        match &self.ui_sink {
            Some(current) if current.same_channel(tx) => {
                self.ui_sink = None;
                true
            }
            _ => false,
        }
    }

    pub fn ui_sink(&self) -> Option<UnboundedSender<ServerMsg>> {
        self.ui_sink.clone()
    }

    pub fn register_shell(&mut self, session_id: &str, tx: UnboundedSender<ServerMsg>) {
        self.shells.insert(session_id.to_string(), tx);
    }

    pub fn unregister_shell(&mut self, session_id: &str) {
        self.shells.remove(session_id);
    }

    pub fn shell_count(&self) -> usize {
        self.shells.len()
    }

    /// Registra un `UiPing{id}` in attesa; il chiamante manda il messaggio
    /// al sink e attende il receiver (con timeout, vedi `ping.rs`).
    pub fn register_ui_ping(&mut self, id: &str) -> oneshot::Receiver<String> {
        let (tx, rx) = oneshot::channel();
        self.ui_pings.insert(id.to_string(), tx);
        rx
    }

    /// Risolve il ping `id` con la versione dichiarata da `ui.exe`.
    /// Id ignoto o già risolto → `false` (risposta tardiva scartata, stesso
    /// principio di `PendingConfirms::resolve`).
    pub fn resolve_ui_ping(&mut self, id: &str, version: String) -> bool {
        match self.ui_pings.remove(id) {
            Some(tx) => tx.send(version).is_ok(),
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc::unbounded_channel;

    #[test]
    fn ui_sink_is_absent_until_set_and_cleared_only_by_same_sender() {
        let mut r = Registry::new();
        assert!(r.ui_sink().is_none());
        let (a, _ra) = unbounded_channel::<ServerMsg>();
        let (b, _rb) = unbounded_channel::<ServerMsg>();
        r.set_ui_sink(a.clone());
        assert!(r.ui_sink().is_some());
        // Una connessione DIVERSA che si chiude non deve azzerare il sink corrente.
        assert!(!r.clear_ui_sink_if(&b));
        assert!(r.ui_sink().is_some());
        assert!(r.clear_ui_sink_if(&a));
        assert!(r.ui_sink().is_none());
    }

    #[test]
    fn last_ui_sink_wins_like_plugin_sink() {
        let mut r = Registry::new();
        let (a, _ra) = unbounded_channel::<ServerMsg>();
        let (b, mut rb) = unbounded_channel::<ServerMsg>();
        r.set_ui_sink(a);
        r.set_ui_sink(b);
        r.ui_sink()
            .unwrap()
            .send(ServerMsg::UiPing { id: "x".into() })
            .unwrap();
        assert!(matches!(rb.try_recv(), Ok(ServerMsg::UiPing { .. })));
    }

    #[test]
    fn shells_are_registered_by_session_id() {
        let mut r = Registry::new();
        let (a, _ra) = unbounded_channel::<ServerMsg>();
        r.register_shell("s1", a);
        assert_eq!(r.shell_count(), 1);
        r.unregister_shell("s1");
        assert_eq!(r.shell_count(), 0);
        r.unregister_shell("mai-esistita"); // no-op, niente panic
    }

    #[tokio::test]
    async fn ui_ping_is_resolved_once_with_the_version() {
        let mut r = Registry::new();
        let rx = r.register_ui_ping("p1");
        assert!(r.resolve_ui_ping("p1", "2.1.0".into()));
        assert_eq!(rx.await.unwrap(), "2.1.0");
        // Seconda risoluzione (o id ignoto): no-op, ritorna false.
        assert!(!r.resolve_ui_ping("p1", "x".into()));
        assert!(!r.resolve_ui_ping("ignoto", "x".into()));
    }
}
