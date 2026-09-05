//! e2e reale (ignorato di default): prova il round-trip completo della calcolatrice
//! con il binario `calc` reale (plugin-calc).
//!
//! Sequenza testata:
//!   1. `cargo build -p plugin-calc` prima di eseguire (l'assert su bin.exists() lo verifica).
//!   2. Crea `tmp/calc/` con manifest + copia di `calc.exe`.
//!   3. `discover(tmp.path())` → 1 plugin scoperto.
//!   4. `PluginHost::start` + `set_server_tx(tx)` (invariante: PRIMA di activate).
//!   5. `activate("calc")` → `Some(1)` + pump avviato.
//!   6. Riceve `OpenPluginWindow { window_id:1, html }` con `lare-key-grid` e `data-evt="eq"`.
//!   7. Sequenza `route_ui_event`: d7 → op_mul → d8 → eq (4 chiamate).
//!   8. Draina i 4 `UpdatePluginWindow` dal canale; verifica che l'ultimo contenga "56".
//!   9. `shutdown().await`.
//!
//! Run: `cargo test -p orchestrator --test plugin_calc_e2e -- --ignored`.

use std::fs;
use std::path::PathBuf;
use tokio::sync::mpsc::unbounded_channel;

/// Test di integrazione end-to-end della calcolatrice:
///
/// Simula la stessa sequenza che fa l'utente quando digita `/calc` e poi preme
/// i tasti 7, ×, 8, = sulla griglia. Il risultato atteso nel display è "56".
///
/// Il test usa il binario reale `calc.exe` compilato in `target/debug/`,
/// quindi richiede `cargo build -p plugin-calc` come prerequisito.
#[tokio::test]
#[ignore = "e2e reale: richiede il binario plugin-calc compilato (cargo build -p plugin-calc)"]
async fn calc_round_trip() {
    // ── 1. Individua il binario calc compilato in target/debug/ ──────────────────
    //
    // `env!("CARGO_MANIFEST_DIR")` = crates/orchestrator.
    // Pop due volte per risalire alla root del repo, poi scendi in target/debug/.
    let bin = {
        let exe = if cfg!(windows) { "calc.exe" } else { "calc" };
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.pop(); // crates/orchestrator → crates
        p.pop(); // crates            → repo root
        p.join("target").join("debug").join(exe)
    };
    assert!(
        bin.exists(),
        "compila prima con: cargo build -p plugin-calc  (cercato: {})",
        bin.display()
    );

    // ── 2. Allestisci plugins/calc/ in una dir temporanea ────────────────────────
    //
    // `discover` scansiona le sottodirectory dirette della root passata.
    // Struttura: `tmp/calc/plugin.json` + `tmp/calc/calc.exe`.
    // Il manifest usa `triggers.command="/calc"` → SpawnPolicy::Lazy.
    let tmp = tempfile::tempdir().unwrap();
    let pdir = tmp.path().join("calc");
    fs::create_dir_all(&pdir).unwrap();
    fs::write(
        pdir.join("plugin.json"),
        r#"{"name":"Calcolatrice","id":"calc","version":"1.0.0","protocol_version":1,"triggers":{"command":"/calc"}}"#,
    )
    .unwrap();
    // Copia il binario compilato nella dir del plugin (nome identico al binario nell'host reale).
    let dest = pdir.join(if cfg!(windows) { "calc.exe" } else { "calc" });
    fs::copy(&bin, &dest).unwrap();

    // ── 3. Discovery → PluginHost::start (factory che fa panic: calc è lazy) ────
    //
    // La factory viene passata a `start` ma NON deve mai essere chiamata durante
    // lo startup perché calc ha `triggers.command` → SpawnPolicy::Lazy.
    use orchestrator::plugins::{
        discovery::discover,
        host::PluginHost,
        transport::{spawn_plugin, PluginReader, PluginWriter},
    };

    let discovered = discover(tmp.path());
    assert_eq!(discovered.len(), 1, "calc non scoperto dalla discovery");

    let mut host = PluginHost::start(
        discovered,
        |_p| -> std::io::Result<(Box<dyn PluginWriter>, Box<dyn PluginReader>)> {
            panic!("calc è lazy: la factory NON deve essere chiamata durante start")
        },
        tmp.path(),
    )
    .await;

    // Calc è lazy: non deve essere in writers dopo lo start.
    assert_eq!(
        host.running_count(),
        0,
        "calc lazy: non deve essere attivo prima di activate"
    );

    // ── 4. set_server_tx PRIMA di activate ────────────────────────────────────────
    //
    // INVARIANTE CRITICA: il pump task viene spawnato dentro `spawn_and_handshake`,
    // che è chiamato da `activate`. Il pump cattura `server_tx.clone()` al momento
    // dello spawn. Se set_server_tx non è stato chiamato prima, il pump cattura None
    // e tutti i ServerMsg vengono scartati silenziosamente → rx.recv() va in timeout.
    let (tx, mut rx) = unbounded_channel::<protocol::ServerMsg>();
    host.set_server_tx(tx);

    // ── 5. activate: lazy-spawn + Init/Ready + Activate ──────────────────────────
    //
    // `activate` chiama spawn_and_handshake (spawn_plugin → Init → Ready-gate),
    // poi avvia il pump task, poi invia Activate{window_id:1}.
    // La calcolatrice risponderà con ShowWindow{window_id:1, html: griglia iniziale}.
    let mut make_fn = |p: &orchestrator::plugins::discovery::DiscoveredPlugin| {
        // `tmp.path()` come config_dir: inerte per questo test (nessun plugin
        // legge argv), ma la firma di `spawn_plugin` lo richiede sempre (spec 2.0).
        spawn_plugin(&p.bin_path, tmp.path()).map(|(w, r)| {
            (
                Box::new(w) as Box<dyn PluginWriter>,
                Box::new(r) as Box<dyn PluginReader>,
            )
        })
    };

    let wid = host.activate("calc", &mut make_fn).await;
    assert_eq!(
        wid,
        Some(1),
        "activate deve ritornare Some(1) per il primo window_id"
    );

    // ── 6. Ricevi OpenPluginWindow (ShowWindow via pump) ─────────────────────────
    //
    // La calcolatrice ha risposto ad Activate con ShowWindow; il pump ha tradotto in
    // OpenPluginWindow e l'ha inviato su server_tx.
    // Timeout 2 secondi: se il pump non invia entro 2s c'è un bug di wiring.
    let open_msg = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        rx.recv(),
    )
    .await
    .expect("timeout 2s: il pump non ha inviato OpenPluginWindow (verifica set_server_tx prima di activate)")
    .expect("channel chiuso: nessun OpenPluginWindow ricevuto");

    // Estrae window_id e html dal messaggio (il `..` salta il campo `title`).
    let (open_wid, open_html) = match open_msg {
        protocol::ServerMsg::OpenPluginWindow { window_id, html, .. } => (window_id, html),
        other => panic!("atteso OpenPluginWindow, ricevuto: {other:?}"),
    };
    assert_eq!(open_wid, 1, "OpenPluginWindow.window_id deve essere 1");
    // L'HTML iniziale deve contenere la griglia tasti e il tasto "=".
    assert!(
        open_html.contains("lare-key-grid"),
        "html di apertura deve contenere \"lare-key-grid\" (ricevuto: {open_html:?})"
    );
    assert!(
        open_html.contains("data-evt=\"eq\""),
        "html di apertura deve contenere data-evt=\"eq\" (ricevuto: {open_html:?})"
    );

    // ── 7. Sequenza tasti: 7 × 8 = ──────────────────────────────────────────────
    //
    // Ogni `route_ui_event` invia UiEvent al plugin; il plugin aggiorna il display
    // e risponde con UpdateWindow; il pump traduce in UpdatePluginWindow → rx.
    //
    // Stato atteso:
    //   d7    → buf "7"      → display "7"
    //   op_mul → buf "7×"   → display "7 ×" (lineare: non parserizza ancora)
    //   d8    → buf "7×8"   → display "7 × 8"
    //   eq    → buf "56"    → display "56"
    host.route_ui_event(1, "d7", None).await;
    host.route_ui_event(1, "op_mul", None).await;
    host.route_ui_event(1, "d8", None).await;
    host.route_ui_event(1, "eq", None).await;

    // ── 8. Draina i 4 UpdatePluginWindow, verifica che l'ultimo contenga "56" ──
    //
    // Il pump invia un UpdatePluginWindow per ogni UiEvent ricevuto dal plugin.
    // Dreniamo tutti e 4 dal canale (ognuno con timeout 2s) e verifichiamo
    // SOLO L'ULTIMO: "56" nel display (l'output smart di format_number su 7×8=56).
    let timeout_dur = std::time::Duration::from_secs(2);
    let mut last_html = String::new();
    for i in 0..4 {
        let update_msg = tokio::time::timeout(timeout_dur, rx.recv())
            .await
            .unwrap_or_else(|_| panic!("timeout 2s attendendo UpdatePluginWindow #{i}"))
            .unwrap_or_else(|| panic!("channel chiuso prima di UpdatePluginWindow #{i}"));

        let html = match update_msg {
            protocol::ServerMsg::UpdatePluginWindow { window_id, html } => {
                assert_eq!(window_id, 1, "UpdatePluginWindow #{i}: window_id deve essere 1");
                html
            }
            other => panic!("atteso UpdatePluginWindow #{i}, ricevuto: {other:?}"),
        };
        last_html = html;
    }

    // L'ultimo UpdatePluginWindow deve mostrare "56" nel display.
    // `render_window` re-parsa "56" (intero valido) → `render(&Expr::Num(56.0))` → "56" plain text.
    assert!(
        last_html.contains("56"),
        "il display finale deve contenere \"56\" (risultato di 7×8); ricevuto: {last_html:?}"
    );

    // ── 9. Shutdown ───────────────────────────────────────────────────────────────
    //
    // Invia Deinit al plugin → il processo termina → kill_on_drop cleanup.
    host.shutdown().await;
}
