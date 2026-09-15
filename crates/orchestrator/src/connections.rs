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
use tokio_util::sync::CancellationToken;

/// Handle condiviso: `ws::serve` lo riceve da `main()` e lo clona per ogni
/// connessione. `tokio::sync::Mutex` perché viene tenuto attraverso `await`
/// solo per operazioni brevissime (mai attraverso una send di rete).
pub type SharedRegistry = Arc<Mutex<Registry>>;

#[derive(Default)]
pub struct Registry {
    ui_sink: Option<UnboundedSender<ServerMsg>>,
    shells: HashMap<String, UnboundedSender<ServerMsg>>,
    ui_pings: HashMap<String, oneshot::Sender<String>>,
    /// Token di cancellazione per comando, raggiungibili da QUALUNQUE
    /// connessione (non solo quella che ha aperto il comando). Serve per
    /// `show_markdown` update-in-place: la finestra Markdown vive su `ui.exe`
    /// (connessione separata dalla shell), ma deve poter cancellare il turno
    /// AI aperto dalla shell — vedi `ws.rs::CancelCommand`.
    command_tokens: HashMap<String, CancellationToken>,
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

    /// Rimuove la sessione shell `session_id` SOLO se il sender registrato è ancora quello di
    /// `tx` (`same_channel`) — stesso schema di `clear_ui_sink_if`. Senza questo controllo, una
    /// connessione VECCHIA che si riconnette con lo stesso `session_id` (la host `lare-shell` lo
    /// fa: piano 2b) potrebbe finire il proprio teardown DOPO che la connessione NUOVA si è già
    /// registrata, cancellando la registrazione giusta e lasciando quella sessione shell
    /// irraggiungibile finché non arriva un'altra riconnessione. Ritorna `true` se ha rimosso
    /// davvero (trovato dalla revisione finale del piano 2b, F4).
    pub fn unregister_shell_if(&mut self, session_id: &str, tx: &UnboundedSender<ServerMsg>) -> bool {
        match self.shells.get(session_id) {
            Some(current) if current.same_channel(tx) => {
                self.shells.remove(session_id);
                true
            }
            _ => false,
        }
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

    /// Registra il token di cancellazione del comando `id`, raggiungibile
    /// da QUALUNQUE connessione (non solo quella che ha aperto il comando).
    pub fn register_command_token(&mut self, id: &str, token: CancellationToken) {
        self.command_tokens.insert(id.to_string(), token);
    }

    /// Rimuove e ritorna il token per `id`, se presente. Usato sia per
    /// cancellare (poi `.cancel()` sul risultato) sia per la pulizia a fine
    /// turno (risultato scartato) — un comando concluso non deve restare
    /// nella mappa indefinitamente (leak).
    pub fn take_command_token(&mut self, id: &str) -> Option<CancellationToken> {
        self.command_tokens.remove(id)
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
        r.register_shell("s1", a.clone());
        assert_eq!(r.shell_count(), 1);
        assert!(r.unregister_shell_if("s1", &a));
        assert_eq!(r.shell_count(), 0);
        assert!(!r.unregister_shell_if("mai-esistita", &a)); // no-op, niente panic
    }

    /// Riconnessione con lo stesso `session_id` (piano 2b, host `lare-shell`): il teardown
    /// della connessione VECCHIA non deve cancellare la registrazione della NUOVA. Stesso
    /// difetto già risolto per il sink `ui` da `clear_ui_sink_if` — trovato dalla revisione
    /// finale del piano 2b (F4) perché `lare-shell` si riconnette con lo stesso `session_id`.
    #[test]
    fn unregister_shell_if_non_cancella_una_riconnessione_con_lo_stesso_session_id() {
        let mut r = Registry::new();
        let (old_tx, _old_rx) = unbounded_channel::<ServerMsg>();
        let (new_tx, _new_rx) = unbounded_channel::<ServerMsg>();
        r.register_shell("s1", old_tx.clone());
        // La nuova connessione arriva e sovrascrive la entry PRIMA che il teardown della vecchia giri.
        r.register_shell("s1", new_tx.clone());
        assert_eq!(r.shell_count(), 1);
        // Il teardown della vecchia connessione non deve toccare la entry (che ora è `new_tx`).
        assert!(!r.unregister_shell_if("s1", &old_tx));
        assert_eq!(r.shell_count(), 1);
        // Solo il teardown della connessione giusta la rimuove davvero.
        assert!(r.unregister_shell_if("s1", &new_tx));
        assert_eq!(r.shell_count(), 0);
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

    // ── command_tokens (Parte B, show_markdown update-in-place) ────────────
    // La proprietà di fondo che questi 2 test verificano, isolata dal resto
    // del ciclo di vita WS (già coperto end-to-end da
    // `ws_integration.rs::cancel_command_from_ui_connection_cancels_a_shell_
    // originated_token`): il registro condiviso è un semplice
    // register→take, simmetrico, senza leak.

    #[test]
    fn take_command_token_removes_and_returns_the_registered_token() {
        let mut r = Registry::new();
        let token = CancellationToken::new();
        r.register_command_token("c1", token.clone());

        let taken = r.take_command_token("c1").expect("il token appena registrato deve essere trovato");
        taken.cancel();
        assert!(token.is_cancelled(), "cancellare il token preso deve riflettersi sull'originale (stesso token, Clone economico)");

        // Preso una volta: non più recuperabile (rimosso, niente doppia cancellazione).
        assert!(r.take_command_token("c1").is_none());
    }

    #[test]
    fn take_command_token_on_unknown_id_is_a_harmless_no_op() {
        let mut r = Registry::new();
        assert!(r.take_command_token("mai-registrato").is_none());
    }
}
