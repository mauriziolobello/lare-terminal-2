//! e2e reale (ignorato di default): prova il round-trip finestra-plugin con il binario
//! `counter` reale (plugin-counter).
//!
//! Sequenza testata:
//!   1. `start` (counter è lazy → non spawnato)
//!   2. `set_server_tx(tx)` — wiring come fa ws.rs dopo la connessione WS
//!   3. `activate("counter")` → lazy-spawn → Init → Ready → Activate → ShowWindow
//!   4. pump forwarda ShowWindow → OpenPluginWindow → rx
//!   5. `route_ui_event(1, "inc", None)` → UiEvent → UpdateWindow
//!   6. pump forwarda UpdateWindow → UpdatePluginWindow → rx
//!   7. `shutdown()`
//!
//! Run: `cargo test -p orchestrator --test plugin_window_e2e -- --ignored`.

use std::fs;
use std::path::PathBuf;
use tokio::sync::mpsc::unbounded_channel;

/// Test di integrazione end-to-end:
/// - Localizza il binario `counter` compilato in `target/debug/`.
/// - Crea una dir temporanea con `plugins/counter/` (manifest + copia del binario).
/// - `start` con factory che fa panic (counter è lazy → mai chiamata durante start).
/// - `set_server_tx(tx)` PRIMA di `activate` (invariante critica: il pump deve catturare Some(tx)).
/// - `activate("counter")` → lazy-spawn reale → pump avviato con Some(tx).
/// - Riceve `OpenPluginWindow{window_id:1, html contains "Count: 0"}` dal pump.
/// - `route_ui_event(1, "inc", None)` → il counter risponde con UpdateWindow.
/// - Riceve `UpdatePluginWindow{window_id:1, html contains "Count: 1"}` dal pump.
/// - `shutdown()`.
#[tokio::test]
#[ignore = "e2e reale: richiede il binario plugin-counter compilato (cargo build -p plugin-counter)"]
async fn counter_activate_and_ui_event_round_trip() {
    // ── 1. Individua il binario counter compilato in target/debug/ ────────────────
    //
    // `env!("CARGO_MANIFEST_DIR")` = crates/orchestrator.
    // Pop due volte per risalire alla root del repo, poi scendi in target/debug/.
    let bin = {
        let exe = if cfg!(windows) { "counter.exe" } else { "counter" };
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.pop(); // crates/orchestrator → crates
        p.pop(); // crates            → repo root
        p.join("target").join("debug").join(exe)
    };
    assert!(
        bin.exists(),
        "compila prima con: cargo build -p plugin-counter  (cercato: {})",
        bin.display()
    );

    // ── 2. Allestisci plugins/counter/ in una dir temporanea ──────────────────────
    //
    // Il manifest è il minimo valido: `triggers.command="/counter"` rende il plugin
    // lazy (SpawnPolicy::Lazy), quindi `start` non lo spawnerà.
    let tmp = tempfile::tempdir().unwrap();
    let pdir = tmp.path().join("counter");
    fs::create_dir_all(&pdir).unwrap();
    fs::write(
        pdir.join("plugin.json"),
        r#"{"name":"Counter","id":"counter","version":"1.0.0","protocol_version":1,"triggers":{"command":"/counter"}}"#,
    )
    .unwrap();
    // Copia il binario compilato nella dir del plugin.
    let dest = pdir.join(if cfg!(windows) { "counter.exe" } else { "counter" });
    fs::copy(&bin, &dest).unwrap();

    // ── 3. Discovery → PluginHost::start (factory che fa panic: counter è lazy) ──
    //
    // La factory viene passata a `start` ma NON deve mai essere chiamata durante
    // lo startup perché counter ha `triggers.command` → SpawnPolicy::Lazy.
    // Se viene chiamata, il test fallisce con panic (factory intenzionalmente "testante").
    use orchestrator::plugins::{
        discovery::discover,
        host::PluginHost,
        transport::{spawn_plugin, PluginReader, PluginWriter},
    };
    let discovered = discover(tmp.path());
    assert_eq!(discovered.len(), 1, "counter non scoperto dalla discovery");

    let mut host = PluginHost::start(
        discovered,
        |_p| -> std::io::Result<(Box<dyn PluginWriter>, Box<dyn PluginReader>)> {
            panic!("counter è lazy: la factory NON deve essere chiamata durante start")
        },
        tmp.path(),
    )
    .await;

    // Counter è lazy: non deve essere in writers dopo lo start.
    assert_eq!(
        host.running_count(),
        0,
        "counter lazy: non deve essere attivo prima di activate"
    );

    // ── 4. set_server_tx PRIMA di activate ────────────────────────────────────────
    //
    // INVARIANTE CRITICA: il pump task viene spawnato DENTRO spawn_and_handshake,
    // che è chiamato da activate. Il pump cattura `server_tx.clone()` al momento
    // dello spawn. Se set_server_tx non è stato chiamato prima, il pump cattura None
    // e tutti i ServerMsg vengono scartati silenziosamente → rx.recv() va in timeout.
    let (tx, mut rx) = unbounded_channel::<protocol::ServerMsg>();
    host.set_server_tx(tx);

    // ── 5. activate: lazy-spawn + Init/Ready + Activate ──────────────────────────
    //
    // `activate` chiama spawn_and_handshake (spawn_plugin → Init → Ready-gate),
    // poi avvia il pump task (che ora cattura Some(tx)), poi invia Activate{window_id:1}.
    // Il counter risponderà con ShowWindow{window_id:1, html:"...Count: 0..."}.
    // Il pump leggerà ShowWindow e lo tradurrà in OpenPluginWindow → server_tx.
    let mut make_fn = |p: &orchestrator::plugins::discovery::DiscoveredPlugin| {
        // `tmp.path()` come config_dir: inerte per questo test, ma la firma
        // lo richiede sempre (spec 2.0, ogni binario riceve --config-dir).
        spawn_plugin(&p.bin_path, tmp.path()).map(|(w, r)| {
            (
                Box::new(w) as Box<dyn PluginWriter>,
                Box::new(r) as Box<dyn PluginReader>,
            )
        })
    };

    let wid = host.activate("counter", &mut make_fn).await;
    assert_eq!(
        wid,
        Some(1),
        "activate deve ritornare Some(1) per il primo window_id"
    );

    // ── 6. Ricevi OpenPluginWindow (ShowWindow via pump) ─────────────────────────
    //
    // Il counter ha risposto a Activate con ShowWindow; il pump ha tradotto in
    // OpenPluginWindow e l'ha inviato su server_tx. Usiamo un timeout di 2 secondi:
    // se il pump non invia entro 2s c'è un bug (set_server_tx ordering, pump non avviato, ...).
    let open_msg = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        rx.recv(),
    )
    .await
    .expect("timeout 2s: il pump non ha inviato OpenPluginWindow (verifica set_server_tx prima di activate)")
    .expect("channel chiuso: nessun OpenPluginWindow ricevuto");

    // Estrai window_id e html dal messaggio (non confronto tutto il ServerMsg per robustezza).
    let (open_wid, open_html) = match open_msg {
        protocol::ServerMsg::OpenPluginWindow { window_id, html, .. } => (window_id, html),
        other => panic!("atteso OpenPluginWindow, ricevuto: {other:?}"),
    };
    assert_eq!(open_wid, 1, "OpenPluginWindow.window_id deve essere 1");
    assert!(
        open_html.contains("Count: 0"),
        "html di OpenPluginWindow deve contenere \"Count: 0\" (ricevuto: {open_html:?})"
    );

    // ── 7. route_ui_event: invia UiEvent{inc} al counter ─────────────────────────
    //
    // Il counter incrementa il contatore e risponde con UpdateWindow.
    // Il pump traduce UpdateWindow → UpdatePluginWindow → server_tx.
    host.route_ui_event(1, "inc", None).await;

    // ── 8. Ricevi UpdatePluginWindow (UpdateWindow via pump) ─────────────────────
    let update_msg = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        rx.recv(),
    )
    .await
    .expect("timeout 2s: il pump non ha inviato UpdatePluginWindow dopo UiEvent{inc}")
    .expect("channel chiuso: nessun UpdatePluginWindow ricevuto");

    let (update_wid, update_html) = match update_msg {
        protocol::ServerMsg::UpdatePluginWindow { window_id, html } => (window_id, html),
        other => panic!("atteso UpdatePluginWindow, ricevuto: {other:?}"),
    };
    assert_eq!(update_wid, 1, "UpdatePluginWindow.window_id deve essere 1");
    assert!(
        update_html.contains("Count: 1"),
        "html di UpdatePluginWindow deve contenere \"Count: 1\" (ricevuto: {update_html:?})"
    );

    // ── 9. Shutdown ───────────────────────────────────────────────────────────────
    //
    // Invia Deinit al counter → il processo termina → kill_on_drop cleanup.
    host.shutdown().await;
}
