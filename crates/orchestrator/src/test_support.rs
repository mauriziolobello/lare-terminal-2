//! Helper di test condivisi (solo `#[cfg(test)]`).

use std::future::Future;

use protocol::ServerMsg;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

/// Esegue una closure produttrice che emette `ServerMsg` su un canale e
/// raccoglie tutti i messaggi in un `Vec`, preservando l'ordine.
///
/// La closure riceve il `Sender` per valore: quando la sua future completa, il
/// `Sender` viene droppato, il canale si chiude e raccogliamo la coda residua.
///
/// Permette ai test di asserire sul `Vec<ServerMsg>` come prima del passaggio
/// al modello a streaming.
pub(crate) async fn collect<F, Fut>(f: F) -> Vec<ServerMsg>
where
    F: FnOnce(UnboundedSender<ServerMsg>) -> Fut,
    Fut: Future<Output = ()>,
{
    let (tx, mut rx) = unbounded_channel();
    f(tx).await; // la future possiede `tx` e lo droppa al termine
    let mut out = Vec::new();
    while let Ok(msg) = rx.try_recv() {
        out.push(msg);
    }
    out
}
