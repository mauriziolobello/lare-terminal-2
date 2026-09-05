//! # surface — dove va ogni `ServerMsg` di un turno originato da una shell
//!
//! `core::handle_command` e l'adapter AI emettono `ServerMsg` su UN canale
//! senza sapere chi li consuma (v1: la stessa connessione). Per una sessione
//! shell (spec §3.2, D14) i messaggi si dividono:
//! - il **testo** (`Chunk`: risposta AI token-per-token + trasparenza dei
//!   tool) viene **bufferizzato** e consegnato una volta sola alla finestra
//!   di output su `ui` a `Done`/`Error` (`OutputWindowContent`);
//! - `Done`/`Error` tornano alla shell, preceduti da UNA riga di conferma
//!   (`Chunk`) — l'unico modo in cui la host sa che la finestra esiste;
//! - tutto il resto segue `ServerMsg::surface()`: `Origin` → shell
//!   (gate, `ExecInShell`, heartbeat), `Ui` → sink `ui` (finestre, relay).
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
    let to_ui = |m: ServerMsg| match &ui {
        Some(u) => {
            let _ = u.send(m);
        }
        None => tracing::warn!(
            "turno {} (sessione {}): ui.exe non connesso, scarto {m:?}",
            turn.id,
            turn.session_id
        ),
    };
    if turn.output_window {
        to_ui(ServerMsg::OpenOutputWindow {
            window_id: turn.window_id.clone(),
            title: turn.title.clone(),
        });
    }
    to_ui(ServerMsg::ActivityIndicator {
        session_id: turn.session_id.clone(),
        kind: "ai_busy".into(),
        on: true,
    });

    let mut buffer = String::new();
    let mut flushed = false;
    let flush = |markdown: String, flushed: &mut bool| {
        if !*flushed {
            *flushed = true;
            if turn.output_window {
                to_ui(ServerMsg::OutputWindowContent {
                    window_id: turn.window_id.clone(),
                    markdown,
                });
            }
        }
    };

    while let Some(msg) = rx.recv().await {
        match msg {
            ServerMsg::Chunk { content, .. } => buffer.push_str(&content),
            ServerMsg::Done { id, exit_code } => {
                let markdown = if buffer.trim().is_empty() {
                    PLACEHOLDER_EMPTY.to_string()
                } else {
                    buffer.clone()
                };
                flush(markdown, &mut flushed);
                let ack = match (ui.is_some(), turn.output_window) {
                    (false, _) => NO_UI_ACK.to_string(),
                    (true, true) => format!("\u{2192} finestra \"{}\" aperta", turn.title),
                    (true, false) => "\u{2192} finestra aperta".to_string(),
                };
                let _ = shell.send(ServerMsg::Chunk {
                    id: id.clone(),
                    content: ack,
                });
                let _ = shell.send(ServerMsg::Done { id, exit_code });
            }
            ServerMsg::Error { id, code, message } => {
                flush(format!("**Errore:** {message}\n\n{buffer}"), &mut flushed);
                let _ = shell.send(ServerMsg::Error { id, code, message });
            }
            other => match other.surface() {
                Surface::Ui => to_ui(other),
                Surface::Origin => {
                    let _ = shell.send(other);
                }
            },
        }
    }
    to_ui(ServerMsg::ActivityIndicator {
        session_id: turn.session_id.clone(),
        kind: "ai_busy".into(),
        on: false,
    });
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
        let ui = drain(&mut ui_rx);
        assert!(ui.iter().any(|m| matches!(m, ServerMsg::OpenWindow { .. })));
        assert!(ui.iter().any(|m| matches!(m, ServerMsg::OutputWindowContent { markdown, .. } if markdown == PLACEHOLDER_EMPTY)));
        let shell = drain(&mut shell_rx);
        assert!(
            matches!(&shell[0], ServerMsg::ExecInShell { .. }),
            "{shell:?}"
        );
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

    /// `/help`: l'esito è l'`OpenWindow` di `handle_slash` (surface `Ui`) — la
    /// finestra di output NON si apre (né segnaposto né contenuto) e la
    /// conferma nel terminale è generica.
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
            matches!(&shell[0], ServerMsg::Chunk { content, .. } if content == "\u{2192} finestra aperta"),
            "{shell:?}"
        );
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
