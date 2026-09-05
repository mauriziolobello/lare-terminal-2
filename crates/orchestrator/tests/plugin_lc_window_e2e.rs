//! REVIEW e2e (ignorato di default): prova, con un orchestrator REALE + binario plugin
//! REALE + `plugin.json` REALE su disco, che la dimensione finestra dichiarata nel
//! manifest raggiunga il wire `ServerMsg::OpenPluginWindow` con `width`/`height`
//! popolati — e che un plugin SENZA quel campo produca un messaggio dove quei campi
//! sono del tutto assenti dal JSON.
//!
//! Questi due test dimostrano gli "hop 1-3" della catena:
//!   1. plugin.json su disco  →  discover() → manifest.window
//!   2. pump task (host.rs)   →  plugin_msg_to_server(msg, window)
//!   3. wire ServerMsg        →  serde_json → JSON grezzo osservato
//!
//! NON coprono l'hop IPC di Tauri (JS undefined → Rust None sul comando
//! open_plugin_window): quello richiede la app desktop e resta al supervisore.
//!
//! Run:
//!   cargo build -p plugin-lc -p plugin-counter
//!   cargo test -p orchestrator --test plugin_lc_window_e2e -- --ignored --nocapture

use std::fs;
use std::path::PathBuf;
use tokio::sync::mpsc::unbounded_channel;

use orchestrator::plugins::{
    discovery::discover,
    host::PluginHost,
    transport::{spawn_plugin, PluginReader, PluginWriter},
};

/// Risale dalla dir del crate orchestrator alla root del repo.
fn repo_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // crates/orchestrator → crates
    p.pop(); // crates             → repo root
    p
}

/// Localizza un binario plugin compilato in `target/debug/`.
/// Onora `CARGO_TARGET_DIR` se impostato (necessario quando si compila da un
/// worktree verso il target dir del repo principale).
fn debug_bin(stem: &str) -> PathBuf {
    let exe = if cfg!(windows) { format!("{stem}.exe") } else { stem.to_string() };
    let target_dir = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_root().join("target"));
    target_dir.join("debug").join(exe)
}

/// **HOP 1-3, caso CON dimensione** — usa il `plugin.json` REALE di plugin-lc
/// (che dichiara `window: {960, 620}`) copiato dal repository, non un literal.
#[tokio::test]
#[ignore = "e2e reale: richiede cargo build -p plugin-lc"]
async fn lc_real_manifest_window_size_reaches_wire() {
    // ── 1. Binario reale lc + plugin.json REALE su disco ─────────────────────────
    let bin = debug_bin("lc");
    assert!(bin.exists(), "compila prima: cargo build -p plugin-lc (cercato {})", bin.display());

    // Il manifest REALE del repository — quello che il commit ha modificato.
    let real_manifest = repo_root().join("crates").join("plugin-lc").join("plugin.json");
    assert!(real_manifest.exists(), "plugin.json reale non trovato: {}", real_manifest.display());

    let tmp = tempfile::tempdir().unwrap();
    let pdir = tmp.path().join("lc");
    fs::create_dir_all(&pdir).unwrap();
    // COPIA il vero plugin.json (con il campo `window`) — nessun literal.
    fs::copy(&real_manifest, pdir.join("plugin.json")).unwrap();
    fs::copy(&bin, pdir.join(if cfg!(windows) { "lc.exe" } else { "lc" })).unwrap();

    // ── 2. discover(): prova che il campo window è parsato DAL DISCO (hop 1) ──────
    let discovered = discover(tmp.path());
    assert_eq!(discovered.len(), 1, "lc non scoperto");
    let window = discovered[0].manifest.window;
    println!("[HOP 1] manifest.window parsato dal plugin.json reale = {window:?}");
    assert_eq!(
        window,
        Some(plugin_protocol::WindowSize { width: 960.0, height: 620.0 }),
        "il plugin.json reale deve dichiarare window 960x620"
    );

    // ── 3. Orchestrator reale: start + set_server_tx + activate ──────────────────
    let mut host = PluginHost::start(
        discovered,
        |_p| -> std::io::Result<(Box<dyn PluginWriter>, Box<dyn PluginReader>)> {
            panic!("lc è lazy: la factory NON deve essere chiamata durante start")
        },
        tmp.path(),
    )
    .await;

    let (tx, mut rx) = unbounded_channel::<protocol::ServerMsg>();
    host.set_server_tx(tx);

    let mut make_fn = |p: &orchestrator::plugins::discovery::DiscoveredPlugin| {
        // `tmp.path()` come config_dir: inerte per questo test, ma la firma
        // lo richiede sempre (spec 2.0, ogni binario riceve --config-dir).
        spawn_plugin(&p.bin_path, tmp.path()).map(|(w, r)| {
            (Box::new(w) as Box<dyn PluginWriter>, Box::new(r) as Box<dyn PluginReader>)
        })
    };

    let wid = host.activate("lc", &mut make_fn).await;
    assert_eq!(wid, Some(1), "activate deve ritornare Some(1)");

    // ── 4. Ricevi OpenPluginWindow e osserva il JSON GREZZO del wire (hop 2-3) ────
    let msg = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout 5s: nessun OpenPluginWindow dal pump")
        .expect("channel chiuso");

    // Serializza col vero serde del protocollo → è ESATTAMENTE ciò che va sul WS.
    let wire = serde_json::to_string(&msg).unwrap();
    println!("[HOP 2-3] wire JSON (lc, con window):\n{wire}");

    match msg {
        protocol::ServerMsg::OpenPluginWindow { window_id, width, height, .. } => {
            assert_eq!(window_id, 1);
            assert_eq!(width, Some(960.0), "width deve arrivare al wire come Some(960.0)");
            assert_eq!(height, Some(620.0), "height deve arrivare al wire come Some(620.0)");
        }
        other => panic!("atteso OpenPluginWindow, ricevuto {other:?}"),
    }
    // Prova sul JSON grezzo: i campi sono PRESENTI e numerici.
    assert!(wire.contains("\"width\":960.0"), "il wire deve contenere width:960.0 → {wire}");
    assert!(wire.contains("\"height\":620.0"), "il wire deve contenere height:620.0 → {wire}");

    host.shutdown().await;
}

/// **HOP 1-3, caso SENZA dimensione** — un plugin (counter) il cui manifest NON
/// dichiara `window` deve produrre un OpenPluginWindow dove width/height sono
/// del tutto ASSENTI dal JSON (retro-compatibilità: calc/ping/counter invariati).
#[tokio::test]
#[ignore = "e2e reale: richiede cargo build -p plugin-counter"]
async fn counter_no_window_omits_size_on_wire() {
    let bin = debug_bin("counter");
    assert!(bin.exists(), "compila prima: cargo build -p plugin-counter (cercato {})", bin.display());

    let tmp = tempfile::tempdir().unwrap();
    let pdir = tmp.path().join("counter");
    fs::create_dir_all(&pdir).unwrap();
    // Manifest SENZA campo `window` (com'è realmente counter).
    fs::write(
        pdir.join("plugin.json"),
        r#"{"name":"Counter","id":"counter","version":"1.0.0","protocol_version":1,"triggers":{"command":"/counter"}}"#,
    )
    .unwrap();
    fs::copy(&bin, pdir.join(if cfg!(windows) { "counter.exe" } else { "counter" })).unwrap();

    let discovered = discover(tmp.path());
    assert_eq!(discovered.len(), 1);
    assert_eq!(discovered[0].manifest.window, None, "counter non dichiara window → None");

    let mut host = PluginHost::start(
        discovered,
        |_p| -> std::io::Result<(Box<dyn PluginWriter>, Box<dyn PluginReader>)> {
            panic!("counter è lazy")
        },
        tmp.path(),
    )
    .await;

    let (tx, mut rx) = unbounded_channel::<protocol::ServerMsg>();
    host.set_server_tx(tx);

    let mut make_fn = |p: &orchestrator::plugins::discovery::DiscoveredPlugin| {
        // `tmp.path()` come config_dir: inerte per questo test, ma la firma
        // lo richiede sempre (spec 2.0, ogni binario riceve --config-dir).
        spawn_plugin(&p.bin_path, tmp.path()).map(|(w, r)| {
            (Box::new(w) as Box<dyn PluginWriter>, Box::new(r) as Box<dyn PluginReader>)
        })
    };

    let wid = host.activate("counter", &mut make_fn).await;
    assert_eq!(wid, Some(1));

    let msg = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout 5s: nessun OpenPluginWindow dal pump")
        .expect("channel chiuso");

    let wire = serde_json::to_string(&msg).unwrap();
    println!("[HOP 2-3] wire JSON (counter, senza window):\n{wire}");

    match msg {
        protocol::ServerMsg::OpenPluginWindow { width, height, .. } => {
            assert_eq!(width, None, "counter: width deve essere None");
            assert_eq!(height, None, "counter: height deve essere None");
        }
        other => panic!("atteso OpenPluginWindow, ricevuto {other:?}"),
    }
    // Prova sul JSON grezzo: i campi NON compaiono affatto (skip_serializing_if).
    assert!(!wire.contains("width"), "width NON deve comparire nel wire → {wire}");
    assert!(!wire.contains("height"), "height NON deve comparire nel wire → {wire}");

    host.shutdown().await;
}
