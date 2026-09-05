//! plugin-counter — plugin di prova per Slice 1: gestione finestre.
//! Su `Activate` apre una finestra con un contatore a zero e un pulsante "+1".
//! Su `UiEvent { element_id: "inc" }` incrementa il contatore e aggiorna la finestra.
//!
//! Struttura analoga a plugin-ping (Fase 0):
//!   - `main`: loop stdin riga-per-riga, serializza le risposte su stdout.
//!   - `handle`: logica pura, testabile senza I/O; prende `&mut Counter` (stato condiviso
//!     tra i messaggi, a differenza di ping che era stateless).
//!   - `render_html`: genera l'HTML del body — unica sorgente di verita' per il markup.

use plugin_protocol::{HostToPlugin, PluginToHost};
use std::io::{BufRead, Write};

/// Stato del plugin: un singolo contatore che persiste tra i messaggi.
///
/// In OOP sarebbe un campo privato di un oggetto `CounterPlugin`.
/// In Rust lo modelliamo come un valore mutabile (owned) passato per `&mut`.
struct Counter {
    count: u64,
}

/// Produce l'HTML della finestra per il contatore corrente.
///
/// Il markup usa le classi del catalogo host (`lare-window`, `lare-label`, `lare-button`)
/// che l'host inietta tramite `plugin-catalog.css` prima del render.
/// `data-evt="inc"` e' il meccanismo di eventi: la UI cattura i click su elementi
/// con questo attributo e li traduce in `UiEvent { element_id: "inc" }`.
fn render_html(n: u64) -> String {
    format!(
        r#"<div class="lare-window"><span class="lare-label">Count: {n}</span><button class="lare-button" data-evt="inc">+1</button></div>"#
    )
}

/// Logica pura: dato lo stato e un messaggio host, produce zero o piu' risposte.
/// Testabile senza I/O — e' il "seam" di test per questo plugin.
///
/// In OOP equivale a un metodo `handleMessage(state, msg)` su un'interfaccia plugin.
/// `&mut Counter` permette di modificare lo stato tra i messaggi.
fn handle(state: &mut Counter, msg: HostToPlugin) -> Vec<PluginToHost> {
    match msg {
        // Fase 0: Init -> risponde Ready con nome e versione protocollo.
        // L'host usa questo per confermare che il plugin e' operativo.
        HostToPlugin::Init { .. } => {
            vec![PluginToHost::Ready { name: "counter".into(), protocol_version: 1 }]
        }

        // Slice 1: Activate -> mostra la finestra con il contatore attuale.
        // `window_id` e' l'identificatore univoco di questa finestra generato dall'host.
        HostToPlugin::Activate { window_id, .. } => {
            vec![PluginToHost::ShowWindow {
                window_id,
                title: "Counter".into(),
                html: render_html(state.count),
            }]
        }

        // Slice 1: UiEvent { element_id: "inc" } -> incrementa e aggiorna la finestra.
        // Il pattern guard `if element_id == "inc"` seleziona solo l'evento noto;
        // altri element_id cadono nel caso successivo.
        HostToPlugin::UiEvent { window_id, element_id, .. } if element_id == "inc" => {
            state.count += 1;
            vec![PluginToHost::UpdateWindow { window_id, html: render_html(state.count) }]
        }

        // UiEvent con element_id sconosciuto: ignora silenziosamente.
        // Usiamo un arm esplicito (non wildcard `_`) per preservare il controllo
        // di esaustivita' del compilatore su HostToPlugin.
        // In OOP: come avere un `default` su un enum aperto — meglio elencare i casi noti.
        HostToPlugin::UiEvent { .. } => vec![],

        // Fase 0: Deinit -> nessuna risposta; il main loop terminera' dopo.
        HostToPlugin::Deinit {} => vec![],
    }
}

fn main() {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    // Stato del plugin: persiste tra i messaggi per l'intera vita del processo.
    let mut state = Counter { count: 0 };

    // Loop bloccante riga-per-riga (single-task: niente tokio necessario per i plugin stdio).
    // Ogni riga JSON e' un messaggio host -> plugin.
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<HostToPlugin>(&line) else {
            eprintln!("[counter] messaggio illegale: {line}");
            continue;
        };
        let is_deinit = matches!(msg, HostToPlugin::Deinit {});
        for reply in handle(&mut state, msg) {
            let mut out = serde_json::to_string(&reply).unwrap();
            out.push('\n');
            if stdout.write_all(out.as_bytes()).is_err() {
                return;
            }
            let _ = stdout.flush();
        }
        if is_deinit {
            break; // stop pulito: il plugin ha finito, termina il processo
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plugin_protocol::{HostToPlugin, PluginToHost};

    /// Verifica che Init produca Ready e che Activate apra la finestra con contatore a zero.
    /// Test della catena iniziale: il plugin si presenta e risponde alla prima attivazione.
    #[test]
    fn init_then_activate_shows_count_zero() {
        let mut st = Counter { count: 0 };
        // Init deve produrre esattamente un Ready con nome "counter".
        assert!(
            handle(
                &mut st,
                HostToPlugin::Init {
                    protocol_version: 1,
                    config: serde_json::Value::Null,
                    storage_dir: "x".into()
                }
            ) == vec![PluginToHost::Ready { name: "counter".into(), protocol_version: 1 }]
        );
        // Activate deve produrre ShowWindow con html che contiene "Count: 0" e il bottone.
        let out = handle(
            &mut st,
            HostToPlugin::Activate { window_id: 5, args: serde_json::Value::Null },
        );
        match &out[0] {
            PluginToHost::ShowWindow { window_id: 5, html, .. } => {
                assert!(html.contains("Count: 0"), "html deve contenere Count: 0");
                assert!(
                    html.contains("data-evt=\"inc\""),
                    "html deve contenere data-evt=\"inc\""
                );
            }
            other => panic!("atteso ShowWindow, ho {other:?}"),
        }
    }

    /// Verifica che UiEvent { element_id: "inc" } incrementi il contatore e aggiorni la finestra.
    /// Test del ciclo aggiornamento: click -> count++ -> UpdateWindow con nuovo html.
    #[test]
    fn ui_event_inc_updates_count() {
        let mut st = Counter { count: 0 };
        // Prima attiviamo la finestra (Activate).
        let _ = handle(
            &mut st,
            HostToPlugin::Activate { window_id: 5, args: serde_json::Value::Null },
        );
        // Poi simuliamo un click sul pulsante "+1".
        let out = handle(
            &mut st,
            HostToPlugin::UiEvent { window_id: 5, element_id: "inc".into(), value: None },
        );
        match &out[0] {
            PluginToHost::UpdateWindow { window_id: 5, html } => {
                assert!(html.contains("Count: 1"), "html deve contenere Count: 1");
            }
            other => panic!("atteso UpdateWindow, ho {other:?}"),
        }
        // Lo stato interno deve riflettere l'incremento.
        assert_eq!(st.count, 1);
    }
}
