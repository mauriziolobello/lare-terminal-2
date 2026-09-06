//! # surface — dove va ogni `ServerMsg` di un turno originato da una shell
//!
//! `core::handle_command` e l'adapter AI emettono `ServerMsg` su UN canale
//! senza sapere chi li consuma (v1: la stessa connessione). Per una sessione
//! shell (spec §3.2, D14) i messaggi si dividono:
//! - il **testo** (`Chunk`: risposta AI token-per-token + trasparenza dei
//!   tool) viene **bufferizzato** e consegnato una volta sola alla finestra
//!   di output su `ui` a `Done`/`Error` (`OutputWindowContent`) — ECCEZIONE
//!   (fix M1, review finale): un turno `WINDOW_SLASHES` (`turn.output_window
//!   == false`, es. `/help`) non ha una finestra di output dove metterlo, quindi
//!   il buffer non vuoto va invece alla shell come un `Chunk` normale, subito
//!   PRIMA della riga di conferma — prima di questo fix veniva scartato senza
//!   lasciare traccia;
//! - `Done` torna alla shell preceduto da UNA riga di conferma (`Chunk`) —
//!   l'unico modo in cui la host sa che la finestra esiste. La conferma
//!   diventa `NO_UI_ACK` non solo quando `ui.exe` non è mai stato connesso,
//!   ma anche se il sink `ui` è caduto A METÀ turno (fix I2, review finale:
//!   prima l'ack mentiva sempre "finestra aperta" quando il sink esisteva
//!   all'inizio ma un invio successivo falliva in silenzio — `let _ =
//!   u.send(m)` — perché la scelta guardava solo `ui.is_some()`, mai
//!   aggiornato). `Error` torna alla shell da solo, senza riga di conferma;
//! - tutto il resto segue `ServerMsg::surface()`: `Origin` → shell
//!   (gate, `ExecInShell`, heartbeat), `Ui` → sink `ui` (finestre, relay).
//!
//! Un turno ha **una sola terminazione** verso la shell: il contratto della
//! host (piano 2b) è "il turno finisce al primo `Done`/`Error`", quindi un
//! secondo `Done`/`Error` per lo stesso turno viene scartato (log `debug`) —
//! il lato `ui` resta comunque consegnato una volta sola, come sempre.
//!
//! Questo task consuma da un `UnboundedReceiver` e scrive su due
//! `UnboundedSender`: nessuna rete, testabile con tre canali.

use protocol::{ServerMsg, Surface};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

/// Identità di un turno shell: `window_id` e `title` sono decisi PRIMA di
/// avviare il comando, così la finestra si apre subito col segnaposto.
#[derive(Debug, Clone)]
pub struct ShellTurn {
    pub id: String,
    pub session_id: String,
    pub window_id: String,
    pub title: String,
    /// `false` per i comandi il cui esito È già una finestra (`/help`, `/show`:
    /// `core::WINDOW_SLASHES`): niente finestra di output col segnaposto,
    /// altrimenti se ne aprirebbero due (una vuota). Spec §3.2, eccezione.
    pub output_window: bool,
}

/// Contenuto della finestra quando il turno non ha prodotto testo.
pub const PLACEHOLDER_EMPTY: &str = "_(nessun output)_";
/// Riga di conferma nel terminale quando `ui.exe` non è connesso.
pub const NO_UI_ACK: &str = "→ ui.exe non connesso: output non mostrato";
pub const DEFAULT_TITLE: &str = "Lare — Output";

/// Titolo della finestra di output derivato dall'input (prima di eseguirlo).
/// `/ai "testo"` e `/ "testo"` → il testo (max 60 caratteri); `/cmd …` →
/// `Lare — /cmd`; input vuoto → `DEFAULT_TITLE`.
pub fn output_window_title(input: &str) -> String {
    let trimmed = input.trim();
    let Some(after) = trimmed.strip_prefix('/') else {
        return DEFAULT_TITLE.to_string();
    };
    let (cmd, rest) = match after.split_once(char::is_whitespace) {
        Some((c, r)) => (c, r.trim()),
        None => (after, ""),
    };
    let quoted = if cmd.is_empty() || cmd.eq_ignore_ascii_case("ai") {
        rest.strip_prefix('"').and_then(|r| r.strip_suffix('"'))
    } else {
        None
    };
    match quoted {
        Some(text) if !text.trim().is_empty() => text.trim().chars().take(60).collect(),
        _ if cmd.is_empty() => DEFAULT_TITLE.to_string(),
        _ => format!("Lare \u{2014} /{}", cmd.to_ascii_lowercase()),
    }
}

/// Manda `m` a `ui`. Ritorna `false` (invio fallito) se il sink non esiste,
/// o se esiste ma l'invio fallisce (canale chiuso — `ui.exe` disconnesso a
/// metà turno). Il chiamante usa il valore per aggiornare `*ui_lost`: PRIMA
/// di questo fix (I2, review finale) un invio fallito veniva ignorato del
/// tutto (`let _ = u.send(m)`) — l'ack finale "finestra aperta" mentiva,
/// perché era scelto guardando solo `ui.is_some()`, vero anche quando il
/// sink era ormai stale. Il `warn` è loggato UNA sola volta per turno (guardia
/// su `*ui_lost` già `true`): i messaggi successivi del turno vengono
/// scartati allo stesso modo, ma senza inondare i log.
///
/// # Nota di stile (funzione libera, non closure)
/// `to_ui`/`flush` erano closure che catturavano `ui`/`turn` per riferimento;
/// per farle aggiornare un flag di stato condiviso (`ui_lost`) servirebbe
/// `FnMut`, e sia `to_ui` sia `flush` (che chiama `to_ui`) verrebbero invocate
/// più volte nello stesso scope — il borrow-checker sulle catture annidiate
/// è più fragile che con parametri espliciti. Funzioni libere con `&mut bool`
/// esplicito (come consigliato dalla review) sono equivalenti e più semplici.
fn to_ui(
    ui: &Option<UnboundedSender<ServerMsg>>,
    ui_lost: &mut bool,
    turn: &ShellTurn,
    m: ServerMsg,
) {
    let ok = match ui {
        Some(u) => u.send(m).is_ok(),
        None => false,
    };
    if !ok {
        if !*ui_lost {
            tracing::warn!(
                "turno {} (sessione {}): ui.exe non raggiungibile (mai connesso o sink caduto a metà turno), scarto i messaggi verso la finestra",
                turn.id,
                turn.session_id
            );
        }
        *ui_lost = true;
    }
}

/// Consegna il Markdown bufferizzato alla finestra di output — UNA sola
/// volta per turno (`*flushed` fa da guardia, come nella versione a closure).
/// No-op verso `ui` per un turno `WINDOW_SLASHES` (`turn.output_window ==
/// false`): il suo esito È già una finestra propria (`/help`, `/show`), niente
/// segnaposto — ma la guardia `*flushed` scatta comunque, così una successiva
/// `Error` tardiva non ritenta l'invio.
fn flush(
    ui: &Option<UnboundedSender<ServerMsg>>,
    ui_lost: &mut bool,
    turn: &ShellTurn,
    flushed: &mut bool,
    markdown: String,
) {
    if !*flushed {
        *flushed = true;
        if turn.output_window {
            to_ui(
                ui,
                ui_lost,
                turn,
                ServerMsg::OutputWindowContent {
                    window_id: turn.window_id.clone(),
                    markdown,
                },
            );
        }
    }
}

/// Consuma i `ServerMsg` del turno da `rx` finché il produttore droppa il
/// sender (`handle_command` lo fa a fine turno), instradando come descritto
/// nel doc-comment del modulo. `ui: None` = `ui.exe` non connesso: i
/// messaggi per `ui` vengono scartati (log `warn`) e la shell riceve
/// `NO_UI_ACK` al posto della conferma.
pub async fn route_shell_turn(
    mut rx: UnboundedReceiver<ServerMsg>,
    shell: UnboundedSender<ServerMsg>,
    ui: Option<UnboundedSender<ServerMsg>>,
    turn: ShellTurn,
) {
    // `ui_lost` (I2): parte `false` e diventa `true` alla prima volta che un
    // invio a `ui` fallisce (sink assente FIN DALL'INIZIO, o caduto a metà
    // turno) — l'`ActivityIndicator{on:true}` qui sotto viene mandato PRIMA
    // di qualunque altra cosa, quindi se `ui` è `None` fin dall'inizio
    // `ui_lost` è già `true` ben prima che serva per l'ack a `Done`.
    let mut ui_lost = false;

    if turn.output_window {
        to_ui(
            &ui,
            &mut ui_lost,
            &turn,
            ServerMsg::OpenOutputWindow {
                window_id: turn.window_id.clone(),
                title: turn.title.clone(),
            },
        );
    }
    to_ui(
        &ui,
        &mut ui_lost,
        &turn,
        ServerMsg::ActivityIndicator {
            session_id: turn.session_id.clone(),
            kind: "ai_busy".into(),
            on: true,
        },
    );

    let mut buffer = String::new();
    let mut flushed = false;

    // Il contratto della host (piano 2b) è "il turno finisce al primo
    // Done/Error": un secondo terminale per lo stesso turno spezzerebbe la
    // sua macchina a stati. `terminal_sent` fa da guardia SEPARATA da
    // `flushed` (che governa solo il lato `ui`): un `Done` dopo un `Error`
    // già inoltrato va scartato anche se `flushed` è già `true`.
    let mut terminal_sent = false;

    while let Some(msg) = rx.recv().await {
        match msg {
            ServerMsg::Chunk { content, .. } => buffer.push_str(&content),
            ServerMsg::Done { id, exit_code } => {
                if terminal_sent {
                    tracing::debug!(
                        "turno {} (sessione {}): Done tardivo dopo un terminale già inoltrato, scarto",
                        turn.id,
                        turn.session_id
                    );
                    continue;
                }
                terminal_sent = true;
                let markdown = if buffer.trim().is_empty() {
                    PLACEHOLDER_EMPTY.to_string()
                } else {
                    buffer.clone()
                };
                flush(&ui, &mut ui_lost, &turn, &mut flushed, markdown);

                // M1 (review finale): un turno WINDOW_SLASHES non apre MAI la
                // finestra di output — ma se nel frattempo è arrivato del
                // testo (Chunk, es. trasparenza tool o risposta AI), quel
                // testo non ha altrove dove andare: la shell lo stampa come
                // un Chunk normale, PRIMA della riga di conferma. Prima di
                // questo fix il buffer per questi turni non veniva MAI letto
                // — testo prodotto e silenziosamente perso.
                if !turn.output_window && !buffer.is_empty() {
                    let _ = shell.send(ServerMsg::Chunk {
                        id: id.clone(),
                        content: buffer.clone(),
                    });
                }

                let ack = if ui_lost {
                    NO_UI_ACK.to_string()
                } else if turn.output_window {
                    format!("\u{2192} finestra \"{}\" aperta", turn.title)
                } else {
                    "\u{2192} finestra aperta".to_string()
                };
                let _ = shell.send(ServerMsg::Chunk {
                    id: id.clone(),
                    content: ack,
                });
                let _ = shell.send(ServerMsg::Done { id, exit_code });
            }
            ServerMsg::Error { id, code, message } => {
                if terminal_sent {
                    tracing::debug!(
                        "turno {} (sessione {}): Error tardivo dopo un terminale già inoltrato, scarto",
                        turn.id,
                        turn.session_id
                    );
                    continue;
                }
                terminal_sent = true;
                flush(
                    &ui,
                    &mut ui_lost,
                    &turn,
                    &mut flushed,
                    format!("**Errore:** {message}\n\n{buffer}"),
                );
                let _ = shell.send(ServerMsg::Error { id, code, message });
            }
            other => match other.surface() {
                Surface::Ui => to_ui(&ui, &mut ui_lost, &turn, other),
                Surface::Origin => {
                    let _ = shell.send(other);
                }
            },
        }
    }
    to_ui(
        &ui,
        &mut ui_lost,
        &turn,
        ServerMsg::ActivityIndicator {
            session_id: turn.session_id.clone(),
            kind: "ai_busy".into(),
            on: false,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{ErrCode, WindowKind};
    use tokio::sync::mpsc::unbounded_channel;

    fn turn() -> ShellTurn {
        ShellTurn {
            id: "c1".into(),
            session_id: "s1".into(),
            window_id: "c1".into(),
            title: "T".into(),
            output_window: true,
        }
    }

    fn drain(rx: &mut tokio::sync::mpsc::UnboundedReceiver<ServerMsg>) -> Vec<ServerMsg> {
        let mut v = vec![];
        while let Ok(m) = rx.try_recv() {
            v.push(m);
        }
        v
    }

    /// Sequenza completa: apre la finestra all'inizio, bufferizza i chunk,
    /// inoltra il gate alla shell, a `Done` consegna il Markdown a `ui` e
    /// UNA riga di conferma + `Done` alla shell.
    #[tokio::test]
    async fn buffers_chunks_and_delivers_content_on_done() {
        let (tx, rx) = unbounded_channel();
        let (shell_tx, mut shell_rx) = unbounded_channel();
        let (ui_tx, mut ui_rx) = unbounded_channel();
        let router = tokio::spawn(route_shell_turn(rx, shell_tx, Some(ui_tx), turn()));
        tx.send(ServerMsg::Chunk {
            id: "c1".into(),
            content: "$ dir\n".into(),
        })
        .unwrap();
        tx.send(ServerMsg::ToolConfirmRequest {
            id: "k".into(),
            commands: "$ dir".into(),
        })
        .unwrap();
        tx.send(ServerMsg::Chunk {
            id: "c1".into(),
            content: "Ecco ".into(),
        })
        .unwrap();
        tx.send(ServerMsg::Chunk {
            id: "c1".into(),
            content: "i file.".into(),
        })
        .unwrap();
        tx.send(ServerMsg::Done {
            id: "c1".into(),
            exit_code: None,
        })
        .unwrap();
        drop(tx);
        router.await.unwrap();

        let ui = drain(&mut ui_rx);
        assert!(
            matches!(&ui[0], ServerMsg::OpenOutputWindow { window_id, title } if window_id == "c1" && title == "T"),
            "{ui:?}"
        );
        assert!(
            matches!(&ui[1], ServerMsg::ActivityIndicator { on: true, .. }),
            "{ui:?}"
        );
        assert!(
            matches!(&ui[2], ServerMsg::OutputWindowContent { window_id, markdown } if window_id == "c1" && markdown == "$ dir\nEcco i file."),
            "{ui:?}"
        );
        assert!(
            matches!(&ui[3], ServerMsg::ActivityIndicator { on: false, .. }),
            "{ui:?}"
        );
        assert_eq!(ui.len(), 4);

        let shell = drain(&mut shell_rx);
        assert!(
            matches!(&shell[0], ServerMsg::ToolConfirmRequest { .. }),
            "{shell:?}"
        );
        assert!(
            matches!(&shell[1], ServerMsg::Chunk { content, .. } if content == "→ finestra \"T\" aperta"),
            "{shell:?}"
        );
        assert!(matches!(&shell[2], ServerMsg::Done { .. }), "{shell:?}");
        assert_eq!(shell.len(), 3);
    }

    /// Messaggi con `surface() == Ui` (es. `OpenWindow` di `show_markdown`)
    /// vanno a `ui`; quelli `Origin` (es. `ExecInShell`) alla shell; un turno
    /// senza testo consegna il segnaposto.
    #[tokio::test]
    async fn routes_by_surface_and_uses_placeholder_when_empty() {
        let (tx, rx) = unbounded_channel();
        let (shell_tx, mut shell_rx) = unbounded_channel();
        let (ui_tx, mut ui_rx) = unbounded_channel();
        let router = tokio::spawn(route_shell_turn(rx, shell_tx, Some(ui_tx), turn()));
        tx.send(ServerMsg::OpenWindow {
            title: "x".into(),
            kind: WindowKind::Markdown,
            content: "y".into(),
        })
        .unwrap();
        tx.send(ServerMsg::ExecInShell {
            turn_id: "c1".into(),
            exec_id: "e".into(),
            command: "dir".into(),
            capture: true,
        })
        .unwrap();
        tx.send(ServerMsg::Done {
            id: "c1".into(),
            exit_code: Some(0),
        })
        .unwrap();
        drop(tx);
        router.await.unwrap();

        // ui: OpenOutputWindow, ActivityIndicator(on), OpenWindow (relay),
        // OutputWindowContent (segnaposto a Done), ActivityIndicator(off).
        let ui = drain(&mut ui_rx);
        assert_eq!(ui.len(), 5, "{ui:?}");
        assert!(
            matches!(&ui[0], ServerMsg::OpenOutputWindow { .. }),
            "{ui:?}"
        );
        assert!(
            matches!(&ui[1], ServerMsg::ActivityIndicator { on: true, .. }),
            "{ui:?}"
        );
        assert!(matches!(&ui[2], ServerMsg::OpenWindow { .. }), "{ui:?}");
        assert!(
            matches!(&ui[3], ServerMsg::OutputWindowContent { markdown, .. } if markdown == PLACEHOLDER_EMPTY),
            "{ui:?}"
        );
        assert!(
            matches!(&ui[4], ServerMsg::ActivityIndicator { on: false, .. }),
            "{ui:?}"
        );

        // shell: ExecInShell (relay), Chunk (ack), Done.
        let shell = drain(&mut shell_rx);
        assert_eq!(shell.len(), 3, "{shell:?}");
        assert!(
            matches!(&shell[0], ServerMsg::ExecInShell { .. }),
            "{shell:?}"
        );
        assert!(matches!(&shell[1], ServerMsg::Chunk { .. }), "{shell:?}");
        assert!(matches!(&shell[2], ServerMsg::Done { .. }), "{shell:?}");
    }

    /// `Error`: la finestra riceve l'errore (una sola volta) e la shell l'`Error`
    /// stesso — nessuna riga di conferma "finestra aperta".
    #[tokio::test]
    async fn error_flushes_window_once_and_forwards_error() {
        let (tx, rx) = unbounded_channel();
        let (shell_tx, mut shell_rx) = unbounded_channel();
        let (ui_tx, mut ui_rx) = unbounded_channel();
        let router = tokio::spawn(route_shell_turn(rx, shell_tx, Some(ui_tx), turn()));
        tx.send(ServerMsg::Chunk {
            id: "c1".into(),
            content: "parziale".into(),
        })
        .unwrap();
        tx.send(ServerMsg::Error {
            id: "c1".into(),
            code: ErrCode::AiError,
            message: "boom".into(),
        })
        .unwrap();
        drop(tx);
        router.await.unwrap();
        let ui = drain(&mut ui_rx);
        let contents: Vec<&ServerMsg> = ui
            .iter()
            .filter(|m| matches!(m, ServerMsg::OutputWindowContent { .. }))
            .collect();
        assert_eq!(contents.len(), 1);
        assert!(
            matches!(contents[0], ServerMsg::OutputWindowContent { markdown, .. } if markdown.contains("boom") && markdown.contains("parziale"))
        );
        let shell = drain(&mut shell_rx);
        assert!(
            matches!(&shell[0], ServerMsg::Error { message, .. } if message == "boom"),
            "{shell:?}"
        );
        assert_eq!(shell.len(), 1);
    }

    /// Senza sink `ui` il turno gira lo stesso: niente panic, la shell riceve
    /// l'avviso al posto della conferma.
    #[tokio::test]
    async fn without_ui_sink_shell_gets_warning_ack() {
        let (tx, rx) = unbounded_channel();
        let (shell_tx, mut shell_rx) = unbounded_channel();
        let router = tokio::spawn(route_shell_turn(rx, shell_tx, None, turn()));
        tx.send(ServerMsg::Chunk {
            id: "c1".into(),
            content: "x".into(),
        })
        .unwrap();
        tx.send(ServerMsg::Done {
            id: "c1".into(),
            exit_code: None,
        })
        .unwrap();
        drop(tx);
        router.await.unwrap();
        let shell = drain(&mut shell_rx);
        assert!(
            matches!(&shell[0], ServerMsg::Chunk { content, .. } if content == NO_UI_ACK),
            "{shell:?}"
        );
        assert!(matches!(&shell[1], ServerMsg::Done { .. }));
    }

    /// Fix I2 (review finale): il sink `ui` esiste all'inizio del turno, ma
    /// il RICEVITORE viene droppato PRIMA che il turno finisca (`ui.exe` si
    /// disconnette a metà) — ogni `send()` successivo fallisce. L'ack non deve
    /// mentire "finestra aperta": diventa `NO_UI_ACK`, esattamente come se
    /// `ui` non fosse mai stato connesso.
    #[tokio::test]
    async fn ui_sink_dropped_mid_turn_yields_no_ui_ack() {
        let (tx, rx) = unbounded_channel();
        let (shell_tx, mut shell_rx) = unbounded_channel();
        let (ui_tx, ui_rx) = unbounded_channel();
        // Il sink ESISTE (Some(ui_tx)) ma il ricevitore viene droppato subito:
        // ogni `ui_tx.send(...)` dentro `route_shell_turn` fallirà da qui in poi.
        drop(ui_rx);
        let router = tokio::spawn(route_shell_turn(rx, shell_tx, Some(ui_tx), turn()));
        tx.send(ServerMsg::Done {
            id: "c1".into(),
            exit_code: Some(0),
        })
        .unwrap();
        drop(tx);
        router.await.unwrap();
        let shell = drain(&mut shell_rx);
        assert!(
            matches!(&shell[0], ServerMsg::Chunk { content, .. } if content == NO_UI_ACK),
            "{shell:?}"
        );
        assert!(matches!(&shell[1], ServerMsg::Done { .. }), "{shell:?}");
    }

    /// `/help`: l'esito è l'`OpenWindow` di `handle_slash` (surface `Ui`) — la
    /// finestra di output NON si apre (né segnaposto né contenuto) e la
    /// conferma nel terminale è generica. Fix M1 (review finale): del testo
    /// bufferizzato (`Chunk "nota"`, es. trasparenza tool o testo AI) durante
    /// un turno così NON deve sparire — va alla shell come Chunk normale,
    /// PRIMA della riga di conferma.
    #[tokio::test]
    async fn window_slash_turn_skips_the_output_window() {
        let (tx, rx) = unbounded_channel();
        let (shell_tx, mut shell_rx) = unbounded_channel();
        let (ui_tx, mut ui_rx) = unbounded_channel();
        let t = ShellTurn {
            output_window: false,
            ..turn()
        };
        let router = tokio::spawn(route_shell_turn(rx, shell_tx, Some(ui_tx), t));
        tx.send(ServerMsg::OpenWindow {
            title: "Lare \u{2014} Comandi".into(),
            kind: WindowKind::Help,
            content: "# h".into(),
        })
        .unwrap();
        tx.send(ServerMsg::Chunk {
            id: "c1".into(),
            content: "nota".into(),
        })
        .unwrap();
        tx.send(ServerMsg::Done {
            id: "c1".into(),
            exit_code: Some(0),
        })
        .unwrap();
        drop(tx);
        router.await.unwrap();
        let ui = drain(&mut ui_rx);
        assert!(
            !ui.iter().any(|m| matches!(
                m,
                ServerMsg::OpenOutputWindow { .. } | ServerMsg::OutputWindowContent { .. }
            )),
            "{ui:?}"
        );
        assert!(ui.iter().any(|m| matches!(m, ServerMsg::OpenWindow { .. })));
        let shell = drain(&mut shell_rx);
        assert!(
            matches!(&shell[0], ServerMsg::Chunk { content, .. } if content == "nota"),
            "{shell:?}"
        );
        assert!(
            matches!(&shell[1], ServerMsg::Chunk { content, .. } if content == "\u{2192} finestra aperta"),
            "{shell:?}"
        );
        assert!(matches!(&shell[2], ServerMsg::Done { .. }), "{shell:?}");
        assert_eq!(shell.len(), 3, "{shell:?}");
    }

    /// Regola del controller: dopo il primo terminale (`Error`) un `Done`
    /// tardivo per lo stesso turno va SCARTATO — niente secondo flush,
    /// niente secondo invio alla shell (il contratto della host è "il turno
    /// finisce al primo `Done`/`Error`").
    #[tokio::test]
    async fn done_after_error_is_dropped() {
        let (tx, rx) = unbounded_channel();
        let (shell_tx, mut shell_rx) = unbounded_channel();
        let (ui_tx, mut ui_rx) = unbounded_channel();
        let router = tokio::spawn(route_shell_turn(rx, shell_tx, Some(ui_tx), turn()));
        tx.send(ServerMsg::Chunk {
            id: "c1".into(),
            content: "parziale".into(),
        })
        .unwrap();
        tx.send(ServerMsg::Error {
            id: "c1".into(),
            code: ErrCode::AiError,
            message: "boom".into(),
        })
        .unwrap();
        tx.send(ServerMsg::Done {
            id: "c1".into(),
            exit_code: Some(0),
        })
        .unwrap();
        drop(tx);
        router.await.unwrap();

        let ui = drain(&mut ui_rx);
        let contents: Vec<&ServerMsg> = ui
            .iter()
            .filter(|m| matches!(m, ServerMsg::OutputWindowContent { .. }))
            .collect();
        assert_eq!(contents.len(), 1, "{ui:?}");
        assert!(
            matches!(contents[0], ServerMsg::OutputWindowContent { markdown, .. } if markdown.contains("boom")),
            "{ui:?}"
        );

        let shell = drain(&mut shell_rx);
        assert_eq!(shell.len(), 1, "{shell:?}");
        assert!(matches!(&shell[0], ServerMsg::Error { .. }), "{shell:?}");
    }

    /// Due `Done` di fila per lo stesso turno: solo il primo produce la
    /// coppia ack+`Done` verso la shell, il secondo viene scartato.
    #[tokio::test]
    async fn duplicate_done_sends_one_ack_and_one_done() {
        let (tx, rx) = unbounded_channel();
        let (shell_tx, mut shell_rx) = unbounded_channel();
        let (ui_tx, _ui_rx) = unbounded_channel();
        let router = tokio::spawn(route_shell_turn(rx, shell_tx, Some(ui_tx), turn()));
        tx.send(ServerMsg::Done {
            id: "c1".into(),
            exit_code: Some(0),
        })
        .unwrap();
        tx.send(ServerMsg::Done {
            id: "c1".into(),
            exit_code: Some(0),
        })
        .unwrap();
        drop(tx);
        router.await.unwrap();

        let shell = drain(&mut shell_rx);
        assert_eq!(shell.len(), 2, "{shell:?}");
        assert!(matches!(&shell[0], ServerMsg::Chunk { .. }), "{shell:?}");
        assert!(matches!(&shell[1], ServerMsg::Done { .. }), "{shell:?}");
    }

    #[test]
    fn output_window_title_from_input() {
        assert_eq!(
            output_window_title("/ai \"elenca i file\""),
            "elenca i file"
        );
        assert_eq!(output_window_title("/ \"ciao\""), "ciao");
        assert_eq!(output_window_title("/help"), "Lare — /help");
        assert_eq!(output_window_title("/open C:\\x"), "Lare — /open");
        assert_eq!(output_window_title("/ping"), "Lare — /ping");
        let long = format!("/ai \"{}\"", "a".repeat(100));
        assert_eq!(output_window_title(&long).chars().count(), 60);
        assert_eq!(output_window_title(""), DEFAULT_TITLE);
    }
}
