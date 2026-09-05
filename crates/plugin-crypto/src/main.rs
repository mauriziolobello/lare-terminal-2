//! plugin-crypto — sidecar del plugin crittografia. Shell I/O sottile:
//! legge `HostToPlugin` da stdin riga per riga, scrive `PluginToHost` su
//! stdout, delega tutta la logica a `state::handle` (stesso pattern di
//! `plugin-lc`, scelto al Task 2 di questo piano per lo stato più
//! complesso di questo plugin rispetto a `plugin-calc`).

mod ciphers;
mod normalize;
mod render;
mod state;

use plugin_protocol::HostToPlugin;
use state::CryptoState;
use std::io::{BufRead, Write};

fn main() {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut crypto_state = CryptoState::default();

    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<HostToPlugin>(&line) else { continue };

        let is_deinit = matches!(msg, HostToPlugin::Deinit {});
        let replies = state::handle(&mut crypto_state, msg);

        for reply in replies {
            send(&mut stdout, &reply);
        }

        if is_deinit {
            break;
        }
    }
}

fn send(out: &mut impl Write, msg: &plugin_protocol::PluginToHost) {
    let json = serde_json::to_string(msg).expect("PluginToHost deve sempre essere serializzabile");
    let _ = writeln!(out, "{json}");
    let _ = out.flush();
}
