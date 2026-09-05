//! plugin-lc — Lare Commander per Lare Terminal.
//!
//! File manager a doppio pannello: due viste di directory navigabili
//! indipendentemente, con operazioni di copia, spostamento, creazione directory,
//! confronto cartelle con date e diff di file di testo.
//!
//! ## Struttura
//! - `config` — lettura/scrittura dei path in `config.json` (dentro `storage_dir`)
//! - `fs`     — lettura directory e ordinamento
//! - `state`  — stato mutabile + handler puro `state::handle`
//! - `ops`    — operazioni file (copy, move, mkdir, compare, diff)
//! - `render` — generazione HTML della finestra
//! - `main`   — loop stdin (protocollo host↔plugin, identico a plugin-calc)
//!
//! ## Protocollo (Contratto P)
//! L'host (orchestrator) invia righe JSON su stdin; il plugin risponde su stdout.
//! Messaggi: `Init` → `Ready`, poi `Activate`/`UiEvent`/`Deinit`.
//! Il loop è sincrono (BufRead line-by-line), single-threaded.
//!
//! ## Attivazione
//! Comando slash: `/lc`
//!
//! ## Persistenza config (attraversa i riavvii)
//! - `Init`: l'host fornisce `storage_dir` (dir privata del plugin) → la memorizziamo. Nessun reset: lo stato sopravvive al riavvio.
//! - `Activate`: lettura dei path salvati da `storage_dir/config.json`.
//! - navigazione (Enter/Backspace): salvataggio immediato (`state::save_config`).
//! - `Deinit`: salvataggio dei path correnti (rete di sicurezza a shutdown).

mod config;
mod fs;
mod state;
mod ops;
mod render;

use plugin_protocol::{HostToPlugin, PluginToHost};
use state::LcState;
use config::LcConfig;
use std::io::{BufRead, Write};
use std::path::PathBuf;

fn main() {
    let stdin  = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    // Directory di storage privata del plugin: fornita dall'host nell'`Init` e
    // ricordata per l'`Activate`/salvataggi successivi. Vuota finché non arriva `Init`.
    let mut storage_dir: PathBuf = PathBuf::new();

    // Stato del plugin: costruito all'Activate.
    let mut lc_state: Option<LcState> = None;

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l)  => l,
            Err(e) => { eprintln!("[plugin-lc] stdin read error: {e}"); break; }
        };

        let line = line.trim();
        if line.is_empty() { continue; }

        let msg: HostToPlugin = match serde_json::from_str(line) {
            Ok(m)  => m,
            Err(e) => {
                eprintln!("[plugin-lc] invalid JSON from host: {e}\n  line: {line}");
                continue;
            }
        };

        // Init: memorizza la storage_dir fornita dall'host e risponde Ready.
        // NB: nessun reset dei path — la persistenza attraversa i riavvii.
        if let HostToPlugin::Init { storage_dir: sd, .. } = &msg {
            storage_dir = PathBuf::from(sd);
            send(&mut out, &PluginToHost::Ready {
                name: "Lare Commander".to_string(),
                protocol_version: 1,
            });
            continue;
        }

        // Activate: carica la config e costruisce lo stato.
        let responses = if let HostToPlugin::Activate { window_id, .. } = msg {
            let cfg = LcConfig::load(&storage_dir);

            let mut new_state = LcState::new(cfg.left_path, cfg.right_path, window_id);
            new_state.storage_dir = storage_dir.clone();

            let html = render::render_window(&new_state);
            let resp = vec![PluginToHost::ShowWindow {
                window_id,
                title: "Lare Commander".to_string(),
                html,
            }];
            lc_state = Some(new_state);
            resp
        } else {
            // Tutti gli altri messaggi richiedono uno stato inizializzato.
            match lc_state.as_mut() {
                Some(s) => state::handle(s, msg),
                None    => {
                    eprintln!("[plugin-lc] received message before Activate: {line}");
                    vec![]
                }
            }
        };

        for resp in responses {
            send(&mut out, &resp);
        }
    }
    // EOF: uscita pulita.
}

/// Serializza e scrive una riga JSON su stdout, con flush immediato.
fn send(out: &mut impl Write, msg: &PluginToHost) {
    let json = serde_json::to_string(msg)
        .expect("PluginToHost deve sempre essere serializzabile");
    let _ = writeln!(out, "{json}");
    let _ = out.flush();
}
