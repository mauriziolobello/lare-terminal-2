//! e2e reale (ignorato di default): build di `ping`, dir plugins/ temporanea, spawn via host,
//! verifica Init -> Ready -> Deinit.
//! Run: `cargo test -p orchestrator --test plugin_e2e -- --ignored`.

use std::fs;
use std::path::PathBuf;

/// Test di integrazione end-to-end:
/// - Localizza il binario `ping` compilato in `target/debug/`.
/// - Crea una dir temporanea con `plugins/ping/` (manifest + copia del binario).
/// - Chiama `discover` -> `PluginHost::start` con transport reale (split).
/// - Verifica che il plugin sia attivo via `running_count()` (ha risposto Ready).
/// - Chiama `host.shutdown()` -> Deinit -> kill_on_drop.
#[tokio::test]
#[ignore = "e2e reale: richiede il binario plugin-ping compilato (cargo build -p plugin-ping)"]
async fn ping_roundtrips_init_ready_deinit() {
    // 1. Individua il binario ping compilato (target/debug/ping(.exe)).
    //    `env!("CARGO_MANIFEST_DIR")` = crates/orchestrator; pop due volte per
    //    arrivare alla root del repo, poi scendi in target/debug/.
    let bin = {
        let exe = if cfg!(windows) { "ping.exe" } else { "ping" };
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.pop(); // crates/orchestrator -> crates
        p.pop(); // crates            -> repo root
        p.join("target").join("debug").join(exe)
    };
    assert!(
        bin.exists(),
        "compila prima con: cargo build -p plugin-ping  (cercato: {})",
        bin.display()
    );

    // 2. Allestisci plugins/ping/ (manifest + copia del binario) in una dir temporanea.
    //    `tempfile::tempdir()` crea una dir e la rimuove al drop (RAII).
    let tmp = tempfile::tempdir().unwrap();
    let pdir = tmp.path().join("ping");
    fs::create_dir_all(&pdir).unwrap();
    fs::write(
        pdir.join("plugin.json"),
        r#"{"name":"Ping","id":"ping","version":"1.0.0","protocol_version":1,"triggers":{}}"#,
    )
    .unwrap();
    let dest = pdir.join(if cfg!(windows) { "ping.exe" } else { "ping" });
    fs::copy(&bin, &dest).unwrap();

    // 3. Discovery + host con transport reale (ChildPluginWriter/Reader split — Task 5).
    //    Questo esercita l'intera catena: discover -> spawn_plugin -> Init -> Ready.
    use orchestrator::plugins::{
        discovery::discover,
        host::PluginHost,
        transport::{spawn_plugin, PluginWriter, PluginReader},
    };
    let discovered = discover(tmp.path());
    assert_eq!(discovered.len(), 1, "ping non scoperto dalla discovery");

    let mut host = PluginHost::start(
        discovered,
        |p| {
            // `tmp.path()` come config_dir: inerte per questo test (nessun
            // plugin legge argv), ma la firma lo richiede sempre (spec 2.0).
            spawn_plugin(&p.bin_path, tmp.path())
                .map(|(w, r)| {
                    (
                        Box::new(w) as Box<dyn PluginWriter>,
                        Box::new(r) as Box<dyn PluginReader>,
                    )
                })
        },
        tmp.path(),
    )
    .await;

    // Il plugin deve essere attivo dopo aver risposto Ready.
    // `running_count()` è il nuovo accessor pubblico (sostituisce `running.len()`).
    assert_eq!(
        host.running_count(),
        1,
        "il plugin non e' attivo dopo il ciclo Init/Ready"
    );

    // 4. Shutdown: invia Deinit -> writers si svuota -> kill_on_drop chiude il processo.
    host.shutdown().await;
}
