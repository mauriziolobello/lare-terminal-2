//! E2E reale di crypto: discovery, handshake, Cesare e dialog parametri.
//! Compilare prima `cargo build -p plugin-crypto`, poi eseguire:
//! `cargo test -p orchestrator --test plugin_crypto_e2e -- --ignored`.

use orchestrator::plugins::{
    discovery::{discover, DiscoveredPlugin},
    host::PluginHost,
    transport::{spawn_plugin, PluginReader, PluginWriter},
};
use plugin_protocol::PluginToHost;
use protocol::ServerMsg;
use std::{fs, path::PathBuf, time::Duration};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

/// Decoratore del reader reale: controlla il primo messaggio senza sostituirlo.
/// L'host riceve lo stesso Ready e continua la propria negoziazione; nessun fake
/// genera risposte. Serve perché running_count da solo non verifica nome/versione.
struct VerificaReady {
    inner: Box<dyn PluginReader>,
    primo: bool,
}

#[async_trait::async_trait]
impl PluginReader for VerificaReady {
    async fn recv(&mut self) -> std::io::Result<Option<PluginToHost>> {
        let msg = self.inner.recv().await?;
        if self.primo {
            assert_eq!(
                msg,
                Some(PluginToHost::Ready {
                    name: "crypto".into(),
                    protocol_version: 1,
                }),
                "il processo deve rispondere a Init con il Ready di crypto"
            );
            self.primo = false;
        }
        Ok(msg)
    }
}

/// Ogni attesa è limitata: un errore nel routing deve fallire, non bloccare la suite.
async fn ricevi(rx: &mut UnboundedReceiver<ServerMsg>) -> ServerMsg {
    tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("timeout 2s aspettando la risposta di crypto")
        .expect("canale chiuso prima della risposta di crypto")
}

async fn aggiornamento(rx: &mut UnboundedReceiver<ServerMsg>, atteso: u64) -> String {
    match ricevi(rx).await {
        ServerMsg::UpdatePluginWindow { window_id, html } => {
            assert_eq!(window_id, atteso, "aggiornata la finestra sbagliata");
            html
        }
        altro => panic!("atteso UpdatePluginWindow, ricevuto {altro:?}"),
    }
}

/// Controlla i due pannelli distintamente: trovare il testo nella sidebar o nel
/// pannello di origine non dimostrerebbe che la trasformazione è stata eseguita.
fn verifica_pannelli(html: &str, chiaro: &str, cifrato: &str) {
    for (campo, testo) in [("plaintext", chiaro), ("ciphertext", cifrato)] {
        assert!(
            html.contains(&format!("data-evt=\"{campo}\">{testo}</textarea>")),
            "valore atteso {testo:?} nel pannello {campo}; HTML: {html}"
        );
    }
}

#[tokio::test]
#[ignore = "e2e reale: richiede il binario plugin-crypto compilato (cargo build -p plugin-crypto)"]
async fn crypto_cifra_decifra_e_applica_parametri_dalla_dialog() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let exe = if cfg!(windows) {
        "crypto.exe"
    } else {
        "crypto"
    };
    let bin = root.join("target/debug").join(exe);
    assert!(
        bin.exists(),
        "compila prima: cargo build -p plugin-crypto ({})",
        bin.display()
    );

    // Il manifest è quello distribuito col crate, senza versioni/trigger inventati.
    // TempDir possiede il layout e lo elimina al drop (RAII); anche i log del
    // transport finiscono qui, mai nella configurazione personale dell'utente.
    let tmp = tempfile::tempdir().unwrap();
    let plugins = tmp.path().join("plugins");
    let pdir = plugins.join("crypto");
    fs::create_dir_all(&pdir).unwrap();
    fs::copy(
        root.join("crates/plugin-crypto/plugin.json"),
        pdir.join("plugin.json"),
    )
    .unwrap();
    fs::copy(bin, pdir.join(exe)).unwrap();
    let discovered = discover(&plugins);
    assert_eq!(discovered.len(), 1, "crypto deve essere scoperto");
    assert_eq!(discovered[0].manifest.id, "crypto");

    let mut host = PluginHost::start(
        discovered,
        |_| -> std::io::Result<(Box<dyn PluginWriter>, Box<dyn PluginReader>)> {
            panic!("crypto è lazy: non deve partire durante start")
        },
        tmp.path(),
    )
    .await;
    assert_eq!(host.running_count(), 0);
    // Collega il sink prima dell'attivazione per ricevere il primo ShowWindow.
    let (tx, mut rx) = unbounded_channel();
    host.set_server_tx(tx);
    let mut make = |p: &DiscoveredPlugin| {
        spawn_plugin(&p.bin_path, tmp.path()).map(|(writer, reader)| {
            (
                Box::new(writer) as Box<dyn PluginWriter>,
                Box::new(VerificaReady {
                    inner: Box::new(reader),
                    primo: true,
                }) as Box<dyn PluginReader>,
            )
        })
    };
    let main_id = host
        .activate("crypto", &mut make)
        .await
        .expect("attivazione fallita");
    assert_eq!(host.running_count(), 1);
    match ricevi(&mut rx).await {
        ServerMsg::OpenPluginWindow {
            window_id,
            title,
            html,
            ..
        } => {
            assert_eq!(window_id, main_id);
            assert_eq!(title, "Crittografia");
            assert!(html.contains("selected\" data-evt=\"cipher-select:caesar\""));
            verifica_pannelli(&html, "", "");
        }
        altro => panic!("attesa finestra principale, ricevuto {altro:?}"),
    }

    // Cesare usa shift=3 di default. XYZ verifica anche il riavvolgimento Z→C.
    host.route_ui_event(main_id, "plaintext", Some("XYZ".into()))
        .await;
    verifica_pannelli(&aggiornamento(&mut rx, main_id).await, "XYZ", "ABC");
    host.route_ui_event(main_id, "ciphertext", Some("DEF".into()))
        .await;
    verifica_pannelli(&aggiornamento(&mut rx, main_id).await, "ABC", "DEF");
    // Ripristina l'ultima modifica sul chiaro: Applica dovrà ricifrare questo lato.
    host.route_ui_event(main_id, "plaintext", Some("ABC".into()))
        .await;
    verifica_pannelli(&aggiornamento(&mut rx, main_id).await, "ABC", "DEF");

    host.route_ui_event(main_id, "open-params", None).await;
    let dialog_id = match ricevi(&mut rx).await {
        ServerMsg::OpenPluginWindow {
            window_id,
            title,
            html,
            ..
        } => {
            assert_ne!(
                window_id, main_id,
                "dialog e principale devono essere distinte"
            );
            assert_eq!(title, "Parametri — Cesare");
            assert!(html.contains("data-evt=\"param:shift\" value=\"3\""));
            window_id
        }
        altro => panic!("attesa dialog parametri, ricevuto {altro:?}"),
    };
    // Usiamo l'id realmente ricevuto: questo esercita la registrazione di OGNI
    // ShowWindow da parte del pump, non solo dell'id creato da Activate.
    // param:shift non rende HTML; Applica produce due aggiornamenti ordinati.
    host.route_ui_event(dialog_id, "param:shift", Some("1".into()))
        .await;
    host.route_ui_event(dialog_id, "params-apply", None).await;
    verifica_pannelli(&aggiornamento(&mut rx, main_id).await, "ABC", "BCD");
    assert!(aggiornamento(&mut rx, dialog_id)
        .await
        .contains("data-evt=\"param:shift\" value=\"1\""));
    host.route_ui_event(dialog_id, "params-close", None).await;
    assert!(
        matches!(ricevi(&mut rx).await, ServerMsg::ClosePluginWindow { window_id } if window_id == dialog_id)
    );

    // Come l'UI reale, comunica all'host che la dialog è stata chiusa. La finestra
    // principale resta utilizzabile con il parametro appena applicato.
    host.forget_window(dialog_id).await;
    host.route_ui_event(main_id, "ciphertext", Some("BCD".into()))
        .await;
    verifica_pannelli(&aggiornamento(&mut rx, main_id).await, "ABC", "BCD");

    // Regressione trovata dal vivo: chiudere e riaprire ui.exe lascia il daemon
    // e il plugin vivi. Il pump deve usare il NUOVO sink, non quello catturato
    // alla prima attivazione. Il drop simula la vecchia connessione chiusa.
    drop(rx);
    let (nuovo_tx, mut nuovo_rx) = unbounded_channel();
    host.set_server_tx(nuovo_tx);
    let nuovo_id = host.activate("crypto", &mut make).await.unwrap();
    assert_ne!(nuovo_id, main_id);
    match ricevi(&mut nuovo_rx).await {
        ServerMsg::OpenPluginWindow {
            window_id, html, ..
        } => {
            assert_eq!(window_id, nuovo_id);
            verifica_pannelli(&html, "ABC", "BCD");
        }
        altro => panic!("attesa finestra sulla UI riconnessa, ricevuto {altro:?}"),
    }
    host.route_ui_event(nuovo_id, "plaintext", Some("XYZ".into()))
        .await;
    verifica_pannelli(&aggiornamento(&mut nuovo_rx, nuovo_id).await, "XYZ", "YZA");
    host.shutdown().await;
    assert_eq!(host.running_count(), 0);
}
