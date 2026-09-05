//! Integration test REALE (#[ignore]) del core di rete su loopback.
//! Due `TcpPeerLink` connessi via TcpListener si scambiano un ChatMsg::Say.
//! Eseguire a mano: `cargo test -p orchestrator --test aichat_loopback -- --ignored`
//!
//! Nota Windows: `listener.local_addr()` restituisce `0.0.0.0:PORT` quando si fa bind su
//! `0.0.0.0`. Su Windows, `TcpStream::connect("0.0.0.0:PORT")` NON viene instradato al
//! loopback (comportamento Winsock diverso da POSIX/Linux). Per questo il test costruisce
//! esplicitamente l'indirizzo `127.0.0.1:PORT` per la connessione del client.

use orchestrator::aichat::net::{bind_listener, connect_peer, TcpPeerLink};
use orchestrator::aichat::transport::PeerLink;
use orchestrator::aichat::wire::ChatMsg;
use std::net::{Ipv4Addr, SocketAddr};

#[tokio::test]
#[ignore = "I/O reale su loopback; eseguire con --ignored"]
async fn tcp_peer_link_roundtrips_a_say_over_loopback() {
    // 1. Il "server" apre un listener su una porta effimera (0 = il SO ne sceglie una).
    let listener = bind_listener(0).await.expect("bind listener");

    // Estraggo solo la porta e costruisco esplicitamente 127.0.0.1:PORT.
    // Su Windows, local_addr() su un socket 0.0.0.0 restituisce 0.0.0.0:PORT, ma
    // TcpStream::connect("0.0.0.0:PORT") NON raggiunge il listener locale (bug Winsock).
    let port = listener.local_addr().expect("local_addr").port();
    let loopback_addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));

    // 2. In un task: il client si connette e manda un Say.
    let client = tokio::spawn(async move {
        let mut link = connect_peer(loopback_addr).await.expect("connect");
        link.send(ChatMsg::Say {
            from_label: "client".into(),
            text: "ciao".into(),
            display_name: None,
            is_ai: false,
        })
        .await
        .expect("send");
    });

    // 3. Il server accetta la connessione e legge il messaggio.
    let (stream, _peer) = listener.accept().await.expect("accept");
    let mut server_link = TcpPeerLink::from_stream(stream);
    let got = server_link.recv().await.expect("recv").expect("un messaggio");

    assert_eq!(
        got,
        ChatMsg::Say {
            from_label: "client".into(),
            text: "ciao".into(),
            display_name: None,
            is_ai: false,
        }
    );
    client.await.expect("client task");
}
