//! plugin-ping — il plugin minimale di Fase 0: su Init risponde Ready, su Deinit esce.
//! Valida la catena host<->plugin (discovery -> spawn -> Init -> Ready -> Deinit).
use plugin_protocol::{HostToPlugin, PluginToHost};
use std::io::{BufRead, Write};

/// Logica pura: dato un messaggio host, produce zero o piu' risposte. Testabile senza I/O.
fn handle(msg: HostToPlugin) -> Vec<PluginToHost> {
    match msg {
        HostToPlugin::Init { .. } => {
            // Risponde con Ready: il plugin si identifica per nome e dichiara la versione
            // di protocollo supportata. L'host usa questo per validare la compatibilita'.
            vec![PluginToHost::Ready { name: "ping".into(), protocol_version: 1 }]
        }
        HostToPlugin::Deinit {} => {
            // Su Deinit non serve rispondere: il main loop uscira' dopo aver inviato
            // tutte le risposte di handle (qui nessuna), poi rompe il ciclo e termina.
            Vec::new()
        }
        // Slice 1 ha aggiunto Activate e UiEvent a HostToPlugin. plugin-ping e' un plugin
        // di Fase 0 che non gestisce finestre: ignora silenziosamente questi messaggi.
        //
        // NOTA: usiamo un or-pattern ESPLICITO invece del wildcard `_`, per conservare
        // il controllo di esaustivita' del compilatore Rust. Se in futuro viene aggiunto
        // un nuovo variant a HostToPlugin (es. Phase 2 OnTimer), il compilatore
        // segnalerà questo `match` come non-esaustivo, forzando una decisione consapevole.
        // Con `_ =>` la protezione sarebbe silenziosa: nessun errore, ma nessuna garanzia.
        // In OOP è come avere un `default` in un switch su un tipo che non si possiede —
        // meglio elencare i casi noti.
        HostToPlugin::Activate { .. } | HostToPlugin::UiEvent { .. } => Vec::new(),
    }
}

fn main() {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    // Loop bloccante riga-per-riga (il plugin e' single-task: niente tokio necessario qui).
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<HostToPlugin>(&line) else {
            eprintln!("[ping] messaggio illegale: {line}");
            continue;
        };
        let is_deinit = matches!(msg, HostToPlugin::Deinit {});
        for reply in handle(msg) {
            let mut out = serde_json::to_string(&reply).unwrap();
            out.push('\n');
            if stdout.write_all(out.as_bytes()).is_err() {
                return;
            }
            let _ = stdout.flush();
        }
        if is_deinit {
            break; // stop pulito
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plugin_protocol::{HostToPlugin, PluginToHost};

    #[test]
    fn init_yields_ready() {
        let out = handle(HostToPlugin::Init {
            protocol_version: 1,
            config: serde_json::Value::Null,
            storage_dir: "x".into(),
        });
        assert_eq!(out, vec![PluginToHost::Ready { name: "ping".into(), protocol_version: 1 }]);
    }

    #[test]
    fn deinit_yields_nothing() {
        assert!(handle(HostToPlugin::Deinit {}).is_empty());
    }
}
