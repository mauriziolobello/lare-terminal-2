//! Implementazioni REALI dell'I/O di rete: `TcpPeerLink` (connessione chat, JSON-per-riga)
//! e `UdpDiscoverer` (annunci broadcast). Sono i "driver" che, dietro i trait `PeerLink`/
//! `Discoverer`, alimentano la logica pura testata nei Task 1-9.

use crate::aichat::discovery::Discoverer;
use crate::aichat::peer::PeerInfo;
use crate::aichat::transport::PeerLink;
use crate::aichat::wire::{Announce, ChatMsg};
use async_trait::async_trait;
use std::net::SocketAddr;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream, UdpSocket};

/// Apre un listener TCP sulla porta indicata (0 = porta effimera scelta dal SO).
/// Bind su `0.0.0.0:port` per accettare connessioni dagli altri host della LAN
/// (vincolo 1a: rete locale fidata; nessuna auth — vedi spec §9).
pub async fn bind_listener(port: u16) -> std::io::Result<TcpListener> {
    TcpListener::bind((std::net::Ipv4Addr::UNSPECIFIED, port)).await
}

/// Si connette al peer (server eletto) e restituisce il link pronto.
pub async fn connect_peer(addr: SocketAddr) -> std::io::Result<TcpPeerLink> {
    let stream = TcpStream::connect(addr).await?;
    Ok(TcpPeerLink::from_stream(stream))
}

/// Link reale su una connessione TCP: legge/scrive `ChatMsg` come righe JSON.
///
/// # Protocollo
/// Ogni messaggio è una singola riga JSON terminata da `\n`. Lo split read/write
/// del `TcpStream` (via `tokio::io::split`) permette di tenere reader e writer
/// in campi distinti della stessa struct senza conflitti di borrow.
pub struct TcpPeerLink {
    /// Lato lettura: avvolto in `BufReader` per la lettura riga per riga.
    reader: BufReader<tokio::io::ReadHalf<TcpStream>>,
    /// Lato scrittura: diretto, senza buffer aggiuntivo.
    writer: tokio::io::WriteHalf<TcpStream>,
}

impl TcpPeerLink {
    /// Costruisce un `TcpPeerLink` da un `TcpStream` già connesso (o accettato).
    /// Divide il socket in metà lettura + metà scrittura con `tokio::io::split`.
    pub fn from_stream(stream: TcpStream) -> Self {
        let (r, w) = tokio::io::split(stream);
        Self { reader: BufReader::new(r), writer: w }
    }
}

#[async_trait]
impl PeerLink for TcpPeerLink {
    /// Serializza `msg` come JSON + `\n` e lo invia al peer.
    async fn send(&mut self, msg: ChatMsg) -> std::io::Result<()> {
        // `serde_json::to_string` non può produrre newline dentro il JSON tagged;
        // la riga è quindi sicuramente delimitata da questo singolo `\n`.
        let mut line = serde_json::to_string(&msg).map_err(std::io::Error::other)?;
        line.push('\n');
        self.writer.write_all(line.as_bytes()).await?;
        // `writer` è un `WriteHalf<TcpStream>` puro, senza buffer in userspace:
        // `flush` è quindi ridondante (no-op), ma innocuo. La latenza a livello
        // kernel è controllata da `TCP_NODELAY` (`set_nodelay`), non da `flush`.
        self.writer.flush().await
    }

    /// Legge una riga JSON e la decodifica in `ChatMsg`.
    /// Ritorna `Ok(None)` se il peer ha chiuso la connessione (EOF: `n == 0`).
    async fn recv(&mut self) -> std::io::Result<Option<ChatMsg>> {
        let mut line = String::new();
        let n = self.reader.read_line(&mut line).await?;
        if n == 0 {
            return Ok(None); // EOF: il peer ha chiuso.
        }
        // `trim_end` rimuove il `\n` (e l'eventuale `\r\n` su Windows) prima del parse.
        let msg = serde_json::from_str(line.trim_end()).map_err(std::io::Error::other)?;
        Ok(Some(msg))
    }
}

/// Scoperta reale via UDP broadcast. `announce()` manda un `Announce` in broadcast;
/// `next()` attende il prossimo datagramma e lo decodifica in `PeerInfo`, ignorando i
/// propri annunci (stesso `id`).
///
/// # Principio di funzionamento
/// Tutti i peer della LAN fanno bind sulla stessa `disc_port` con `SO_BROADCAST`.
/// L'annuncio è un datagramma JSON `Announce` mandato all'indirizzo broadcast
/// `255.255.255.255:disc_port`. Ogni peer lo riceve (incluso il mittente) e filtra
/// il proprio tramite il confronto `ann.id == self.me.id`.
pub struct UdpDiscoverer {
    socket: UdpSocket,
    /// Info di questo nodo (chi siamo: id, etichetta base, porta TCP).
    me: PeerInfo,
    /// `255.255.255.255:disc_port` — destinazione del broadcast.
    broadcast_addr: SocketAddr,
    // NB (debito #6, "discovery full-duplex"): qui c'era un `buf: Vec<u8>` riusabile,
    // pensato per evitare un'allocazione per ogni `recv_from`. È stato rimosso: `next()`
    // è passato da `&mut self` a `&self` (per poter coesistere con `announce(&self)` in un
    // `tokio::select!` — vedi doc-comment di `Discoverer::next`), e un campo `Vec<u8>`
    // condiviso via `&self` richiederebbe a sua volta un `Mutex`/`RefCell` solo per
    // riusarlo. `tokio::net::UdpSocket::recv_from` prende già `&self` nativamente (il
    // socket supporta invii/ricezioni concorrenti), quindi la soluzione più semplice è
    // allocare un buffer LOCALE ad ogni chiamata di `next()` (vedi sotto). Costo
    // trascurabile: i datagrammi di scoperta arrivano ogni pochi secondi, non è un
    // percorso hot-path — la correttezza/full-duplex vale più della micro-ottimizzazione.
}

impl UdpDiscoverer {
    /// Crea e configura il discoverer: bind su `0.0.0.0:disc_port` + abilita `SO_BROADCAST`.
    /// `disc_port` è la porta UDP di scoperta (può coincidere col TCP: spazi distinti).
    pub async fn new(me: PeerInfo, disc_port: u16) -> std::io::Result<Self> {
        let socket = UdpSocket::bind((std::net::Ipv4Addr::UNSPECIFIED, disc_port)).await?;
        // `SO_BROADCAST` è richiesto dal SO per mandare datagrammi all'indirizzo broadcast.
        socket.set_broadcast(true)?;
        let broadcast_addr = SocketAddr::from((std::net::Ipv4Addr::BROADCAST, disc_port));
        Ok(Self { socket, me, broadcast_addr })
    }
}

#[async_trait]
impl Discoverer for UdpDiscoverer {
    /// Serializza l'`Announce` corrente (chi siamo, più il leader che crediamo valido)
    /// e lo invia in broadcast UDP.
    async fn announce(&self, leader: Option<crate::aichat::peer::PeerId>) -> std::io::Result<()> {
        let ann = Announce {
            v: 1,
            id: self.me.id,
            label_base: self.me.label_base.clone(),
            chat_port: self.me.chat_port,
            leader,
        };
        let bytes = serde_json::to_vec(&ann).map_err(std::io::Error::other)?;
        // Invia a 255.255.255.255 (broadcast limitato) E alla broadcast di SOTTORETE /24
        // derivata dal nostro IP (es. 192.168.178.35 → 192.168.178.255): quest'ultima viene
        // instradata sull'interfaccia LAN corretta dalla tabella di routing, evitando che il
        // broadcast esca da una NIC virtuale sbagliata (Hyper-V/WSL/VPN). Best-effort: un
        // singolo target che fallisce non interrompe l'altro.
        let _ = self.socket.send_to(&bytes, self.broadcast_addr).await;
        let o = self.me.id.0.octets();
        let subnet_bcast = SocketAddr::from((
            std::net::Ipv4Addr::new(o[0], o[1], o[2], 255),
            self.broadcast_addr.port(),
        ));
        let _ = self.socket.send_to(&bytes, subnet_bcast).await;
        tracing::debug!(
            "aichat udp: announce → {} + {} (io id={})",
            self.broadcast_addr, subnet_bcast, self.me.id.0
        );
        Ok(())
    }

    /// Attende un datagramma UDP, lo decodifica come `Announce`, e lo converte in
    /// `(PeerInfo, Option<PeerId>)` — le info del mittente più il leader che il
    /// mittente ha riportato di credere valido (Bug 2, late-joiner).
    /// I propri annunci (stesso `id`) vengono ignorati silenziosamente (loop interno).
    /// Ritorna `None` solo in caso di errore I/O permanente (socket chiuso/errore grave).
    async fn next(&self) -> Option<(PeerInfo, Option<crate::aichat::peer::PeerId>)> {
        // Buffer LOCALE alla chiamata (non più un campo riusabile — vedi il commento sul
        // campo rimosso in `UdpDiscoverer`): ogni invocazione di `next()` alloca il
        // proprio, così il metodo può prendere `&self` invece di `&mut self` ed essere
        // chiamato concorrentemente ad `announce(&self)` dallo stesso `select!` in `main.rs`.
        let mut buf = [0u8; 2048];
        loop {
            // `recv_from` riempie il buffer e restituisce (n_byte_letti, indirizzo_sorgente).
            let (n, src) = match self.socket.recv_from(&mut buf).await {
                Ok(v) => v,
                Err(e) => {
                    // Errore transitorio (es. WSAECONNRESET su Windows dopo un send_to a un
                    // peer irraggiungibile): la scoperta NON deve terminare. `None` è riservato
                    // alla chiusura permanente della sorgente; qui logghiamo e continuiamo.
                    tracing::debug!("aichat discovery: recv_from error ignorato: {e}");
                    continue;
                }
            };
            let Ok(ann) = serde_json::from_slice::<Announce>(&buf[..n]) else {
                tracing::debug!("aichat udp: datagramma non valido da {src} ({n}B)");
                continue;
            };
            if ann.id == self.me.id {
                tracing::debug!("aichat udp: ricevuto PROPRIO annuncio (loopback ok) da {src}");
                continue; // è il nostro stesso annuncio → ignora (il SO ci rimanda il broadcast)
            }
            tracing::debug!(
                "aichat udp: PEER ricevuto da {src} id={} label={} leader={:?}",
                ann.id.0, ann.label_base, ann.leader
            );
            return Some((
                PeerInfo {
                    id: ann.id,
                    label_base: ann.label_base,
                    chat_port: ann.chat_port,
                },
                ann.leader,
            ));
        }
    }
}
