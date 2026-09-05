//! Seam verso UNA connessione peer (un link TCP). Split simmetrico send/recv su un solo
//! trait (un link è posseduto da un task dedicato → niente contesa di lock come nei plugin).
//! In produzione: `TcpPeerLink` (Task 10). Nei test: `FakePeerLink`.

use crate::aichat::wire::ChatMsg;
use async_trait::async_trait;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// Log condiviso dei `ChatMsg` inviati su un link: clonabile, ispezionabile dal test
/// dopo che il link è stato spostato nel canale.
pub type SentChat = Arc<Mutex<Vec<ChatMsg>>>;

/// Una connessione bidirezionale verso un peer.
///
/// Astrae il trasporto reale (TCP in produzione) da quello fittizio (in-memory nei test).
/// Il possessore del `PeerLink` è tipicamente un task dedicato: non serve `Arc<Mutex<...>>`
/// perché un solo task alla volta legge/scrive sul link.
#[async_trait]
pub trait PeerLink: Send {
    /// Invia un messaggio al peer remoto.
    async fn send(&mut self, msg: ChatMsg) -> std::io::Result<()>;
    /// Riceve il prossimo messaggio dal peer remoto.
    /// `Ok(None)` = connessione chiusa (EOF): il task deve terminare.
    async fn recv(&mut self) -> std::io::Result<Option<ChatMsg>>;
}

/// Fake in-memory: registra gli invii nel `SentChat` condiviso e rigioca una coda di
/// messaggi in arrivo.
///
/// Usato nei test per isolare il codice che usa `PeerLink` dall'I/O TCP reale.
/// La coda `incoming` è un `VecDeque`: `pop_front()` restituisce i messaggi nell'ordine
/// in cui sono stati prefissati; quando la coda è vuota, `recv()` restituisce `Ok(None)`
/// (= EOF del link).
pub struct FakePeerLink {
    /// Dove finiscono tutti i messaggi inviati via `send()`.
    sent: SentChat,
    /// Coda FIFO dei messaggi simulati in arrivo (prefissata al momento della costruzione).
    incoming: VecDeque<ChatMsg>,
}

impl FakePeerLink {
    /// Costruisce un fake con il log condiviso `sent` e una coda di messaggi in arrivo
    /// prefissata. Il log è clonato dall'esterno così il test può ispezionarlo dopo
    /// aver ceduto la ownership del link al codice da testare.
    pub fn new(sent: SentChat, incoming: Vec<ChatMsg>) -> Self {
        Self { sent, incoming: incoming.into() }
    }
}

#[async_trait]
impl PeerLink for FakePeerLink {
    async fn send(&mut self, msg: ChatMsg) -> std::io::Result<()> {
        // Accoda il messaggio nel log condiviso. `unwrap()` è ammesso nei test:
        // un lock avvelenato indica un bug nel test stesso.
        self.sent.lock().unwrap().push(msg);
        Ok(())
    }

    async fn recv(&mut self) -> std::io::Result<Option<ChatMsg>> {
        // `pop_front()` restituisce `None` quando la coda è esaurita → segnala EOF al chiamante.
        Ok(self.incoming.pop_front())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fake_link_records_sent_and_replays_incoming() {
        let log: SentChat = Default::default();
        let mut link = FakePeerLink::new(
            log.clone(),
            vec![ChatMsg::Say { from_label: "a".into(), text: "ciao".into(), display_name: None, is_ai: false }],
        );
        link.send(ChatMsg::Join { label: "me".into() }).await.unwrap();
        let got = link.recv().await.unwrap();
        assert!(matches!(got, Some(ChatMsg::Say { .. })));
        assert!(link.recv().await.unwrap().is_none()); // EOF
        assert_eq!(log.lock().unwrap().len(), 1);
        assert!(matches!(log.lock().unwrap()[0], ChatMsg::Join { .. }));
    }
}
