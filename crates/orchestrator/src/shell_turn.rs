// crates/orchestrator/src/shell_turn.rs
//! # shell_turn — esecuzione di un `Command` arrivato da una sessione shell
//!
//! `ws.rs` fa una cosa sola per la shell: costruisce [`ShellTurnDeps`] e
//! spawna [`run_shell_command`]. Tutto il resto vive qui, in ordine:
//! 1. `shell_slash::classify_shell_input` decide il tipo di riga (spec §3);
//! 2. le risposte immediate (`Error` di sintassi, `Done` muto, `/reset`,
//!    `OpenUiLocal`) vanno direttamente sulla connessione shell;
//! 3. `/ai "…"`, i comandi backend e `/ping` aprono un **turno**: un canale
//!    interno consumato da `surface::route_shell_turn` (che apre la finestra
//!    di output su `ui` e smista il resto) e alimentato da
//!    `core::handle_command` (o dal built-in `/ping`).
//!
//! Le dipendenze arrivano tutte per costruzione (nessun globale): è un
//! "command handler" con le sue collaborazioni esplicite, testabile con
//! un registro vuoto e un `StubAdapter`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use protocol::{CommandKind, ErrCode, ServerMsg};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::ai_adapter::AiAdapter;
use crate::connections::SharedRegistry;
use crate::local_confirm::{PendingConfirms, ShellConfirmer};
use crate::messages_client::ConversationHistory;
use crate::plugins::discovery::DiscoveredPlugin;
use crate::plugins::host::PluginHost;
use crate::plugins::transport::{PluginReader, PluginWriter};
use crate::runtime_config::RuntimeConfig;
use crate::shell_session::{ShellSessionState, ShellSessionToolClient};
use crate::shell_slash::{
    classify_shell_input, is_window_slash, read_web_search_enabled, ShellInput, RESET_MESSAGE,
};
use crate::surface::{output_window_title, route_shell_turn, ShellTurn, NO_UI_ACK};

/// Collaborazioni di un comando shell (vedi doc-comment del modulo).
pub(crate) struct ShellTurnDeps {
    pub ai: Arc<dyn AiAdapter>,
    pub history: Arc<Mutex<ConversationHistory>>,
    pub pending_confirms: PendingConfirms,
    pub registry: SharedRegistry,
    pub plugin_host: Arc<Mutex<PluginHost>>,
    pub rt: Arc<RuntimeConfig>,
    pub shell: Arc<ShellSessionState>,
    pub out_tx: UnboundedSender<ServerMsg>,
    /// `Hello.version` della host, per la riga `lare-shell` di `/ping`.
    pub shell_version: Option<String>,
}

/// Timeout del gate `[Y/n]` sulla shell: come la UI locale v1 (180 s).
const CONFIRM_TIMEOUT: Duration = Duration::from_secs(180);

/// Punto d'ingresso: consuma il `Command{id, input, cwd, web_search}` di una
/// sessione shell. Ritorna appena il lavoro è avviato (i turni girano in
/// task spawnati): `ws.rs` lo chiama dentro `tokio::spawn`, quindi il loop
/// della connessione resta libero per `CancelCommand`/`ExecResult`.
pub(crate) async fn run_shell_command(
    deps: ShellTurnDeps,
    id: String,
    input: String,
    cwd: Option<String>,
    web_search: bool,
    cancel: CancellationToken,
) {
    // `Command.cwd` = `$PWD` del runspace al momento dell'invio (spec §4.6).
    if let Some(c) = cwd.as_deref().filter(|s| !s.is_empty()) {
        deps.shell.set_cwd(c).await;
    }
    let known = |c: &str| crate::core::KNOWN_BACKEND_SLASHES.contains(&c);
    match classify_shell_input(&input, &known) {
        ShellInput::SyntaxError(message) => {
            let _ = deps.out_tx.send(ServerMsg::Error {
                id,
                code: ErrCode::RoutingError,
                message,
            });
        }
        ShellInput::Discard(cmd) => {
            tracing::info!(
                "discard slash from shell session {}: /{cmd}",
                deps.shell.session_id()
            );
            let _ = deps.out_tx.send(ServerMsg::Done {
                id,
                exit_code: Some(0),
            });
        }
        ShellInput::Reset => {
            let _ = deps.out_tx.send(ServerMsg::Chunk {
                id: id.clone(),
                content: RESET_MESSAGE.to_string(),
            });
            let _ = deps.out_tx.send(ServerMsg::Done {
                id,
                exit_code: Some(0),
            });
        }
        ShellInput::OpenUiLocal(name) => {
            let sink = deps.registry.lock().await.ui_sink();
            match sink {
                Some(ui) => {
                    let _ = ui.send(ServerMsg::OpenUiLocal { name });
                    let _ = deps.out_tx.send(ServerMsg::Done {
                        id,
                        exit_code: Some(0),
                    });
                }
                None => {
                    let _ = deps.out_tx.send(ServerMsg::Chunk {
                        id: id.clone(),
                        content: NO_UI_ACK.to_string(),
                    });
                    let _ = deps.out_tx.send(ServerMsg::Done {
                        id,
                        exit_code: Some(1),
                    });
                }
            }
        }
        ShellInput::Ping => {
            let turn_tx = start_turn(&deps, &id, &input).await;
            tokio::spawn(async move { run_ping_turn(deps, id, turn_tx).await });
        }
        ShellInput::Nl(text) => {
            let turn_tx = start_turn(&deps, &id, &input).await;
            tokio::spawn(async move {
                run_ai_turn(deps, id, text, CommandKind::Nl, web_search, cancel, turn_tx).await
            });
        }
        ShellInput::Backend => {
            let turn_tx = start_turn(&deps, &id, &input).await;
            tokio::spawn(async move {
                run_ai_turn(
                    deps,
                    id,
                    input,
                    CommandKind::Auto,
                    web_search,
                    cancel,
                    turn_tx,
                )
                .await
            });
        }
    }
}

/// Apre il turno: canale interno + router di superficie (che apre subito la
/// finestra di output su `ui`). Ritorna il sender su cui il produttore emette.
async fn start_turn(deps: &ShellTurnDeps, id: &str, input: &str) -> UnboundedSender<ServerMsg> {
    let (turn_tx, turn_rx) = unbounded_channel::<ServerMsg>();
    let turn = ShellTurn {
        id: id.to_string(),
        session_id: deps.shell.session_id().to_string(),
        window_id: id.to_string(),
        title: output_window_title(input),
        output_window: !is_window_slash(input),
    };
    let ui = deps.registry.lock().await.ui_sink();
    tokio::spawn(route_shell_turn(turn_rx, deps.out_tx.clone(), ui, turn));
    turn_tx
}

/// `/ai "…"` (Nl) o comando backend (Auto): `core::handle_command` con il
/// `ToolClient` della sessione e il gate della shell. La ricerca web vale se
/// il client l'ha chiesta O se l'utente l'ha attivata in `/config`.
async fn run_ai_turn(
    deps: ShellTurnDeps,
    id: String,
    input: String,
    kind: CommandKind,
    web_search: bool,
    cancel: CancellationToken,
    turn_tx: UnboundedSender<ServerMsg>,
) {
    let tools = ShellSessionToolClient::for_turn(&deps.shell, id.clone());
    let confirmer = ShellConfirmer::new(
        deps.out_tx.clone(),
        deps.pending_confirms.clone(),
        CONFIRM_TIMEOUT,
        id.clone(),
    );
    let cwd = deps.shell.cwd().await;
    let web_search = web_search || read_web_search_enabled(&deps.rt.config_dir);
    let mut guard = deps.history.lock().await;
    crate::core::handle_command(
        &id,
        &input,
        kind,
        if cwd.is_empty() { None } else { Some(&cwd) },
        &mut guard,
        deps.ai.as_ref(),
        tools.as_ref(),
        None,
        None,
        web_search,
        Some(&confirmer),
        Some(cancel),
        turn_tx,
    )
    .await;
}

/// Built-in `/ping` (spec §3.1): sonde reali, poi `ping::run_ping` formatta.
async fn run_ping_turn(deps: ShellTurnDeps, id: String, turn_tx: UnboundedSender<ServerMsg>) {
    // Plugin: `find` sotto lock breve; la sonda (fino a 5 s) FUORI dal lock.
    let found = deps.plugin_host.lock().await.find("ping");
    let plugin = match found {
        None => None,
        Some((p, storage_dir)) => {
            let config_dir = deps.rt.config_dir.clone();
            let mut make = |p: &DiscoveredPlugin| {
                crate::plugins::transport::spawn_plugin(&p.bin_path, &config_dir).map(|(w, r)| {
                    (
                        Box::new(w) as Box<dyn PluginWriter>,
                        Box::new(r) as Box<dyn PluginReader>,
                    )
                })
            };
            Some(
                crate::plugins::host::probe(&p, &storage_dir, &mut make)
                    .await
                    .map(|d| (p.manifest.version.clone(), d)),
            )
        }
    };
    // ui.exe: `UiPing` sul sink, `UiPong` risolto dal registro (arm in ws.rs).
    let sink = deps.registry.lock().await.ui_sink();
    let ui = match sink {
        None => None,
        Some(sink) => {
            let ping_id = format!("{:032x}", rand::random::<u128>());
            let rx = deps.registry.lock().await.register_ui_ping(&ping_id);
            let start = Instant::now();
            if sink
                .send(ServerMsg::UiPing {
                    id: ping_id.clone(),
                })
                .is_err()
            {
                Some(Err("invio di UiPing fallito".to_string()))
            } else {
                match tokio::time::timeout(crate::ping::UI_PING_TIMEOUT, rx).await {
                    Ok(Ok(version)) => Some(Ok((version, start.elapsed()))),
                    _ => {
                        deps.registry
                            .lock()
                            .await
                            .resolve_ui_ping(&ping_id, String::new());
                        Some(Err(format!(
                            "nessun UiPong entro {} s",
                            crate::ping::UI_PING_TIMEOUT.as_secs()
                        )))
                    }
                }
            }
        }
    };
    let markdown = crate::ping::run_ping(
        deps.shell.session_id(),
        deps.shell_version.as_deref(),
        plugin,
        ui,
    );
    let _ = turn_tx.send(ServerMsg::Chunk {
        id: id.clone(),
        content: markdown,
    });
    let _ = turn_tx.send(ServerMsg::Done {
        id,
        exit_code: Some(0),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_adapter::StubAdapter;
    use crate::connections::Registry;
    use crate::tool_client::FakeToolClient;
    use crate::tool_client::ToolClient;

    // `async fn` (non `futures::executor::block_on`, come da revisione): un
    // `block_on` dentro a un test `#[tokio::test]` (single-thread di default)
    // fa da bomba a orologeria — innocuo qui SOLO perché `PluginHost::start`
    // con zero plugin non attraversa mai un vero `.await` sospeso, ma
    // deadlocka non appena qualcosa (un timer, un vero I/O) lo facesse. Un
    // `.await` normale non ha questo rischio: gira sullo stesso runtime.
    async fn deps(out_tx: UnboundedSender<ServerMsg>, registry: SharedRegistry) -> ShellTurnDeps {
        let mcp: Arc<dyn ToolClient> = Arc::new(FakeToolClient::success("ok"));
        let shell = Arc::new(ShellSessionState::new(
            "s1",
            out_tx.clone(),
            "C:\\w",
            mcp,
            std::env::temp_dir(),
        ));
        let plugin_host = PluginHost::start(
            vec![],
            |_p: &DiscoveredPlugin| -> std::io::Result<(Box<dyn PluginWriter>, Box<dyn PluginReader>)> {
                unreachable!()
            },
            std::path::Path::new("."),
        )
        .await;
        ShellTurnDeps {
            ai: Arc::new(StubAdapter),
            history: Arc::new(Mutex::new(ConversationHistory::new())),
            pending_confirms: PendingConfirms::new(),
            registry,
            plugin_host: Arc::new(Mutex::new(plugin_host)),
            rt: Arc::new(RuntimeConfig::for_test(&std::env::temp_dir())),
            shell,
            out_tx,
            shell_version: Some("2.0.0".into()),
        }
    }

    fn drain(rx: &mut tokio::sync::mpsc::UnboundedReceiver<ServerMsg>) -> Vec<ServerMsg> {
        let mut v = vec![];
        while let Ok(m) = rx.try_recv() {
            v.push(m);
        }
        v
    }

    #[tokio::test]
    async fn unknown_slash_yields_a_mute_done() {
        let (tx, mut rx) = unbounded_channel();
        run_shell_command(
            deps(tx, Registry::shared()).await,
            "c1".into(),
            "/nonesiste".into(),
            None,
            false,
            CancellationToken::new(),
        )
        .await;
        let out = drain(&mut rx);
        assert!(
            matches!(
                out.as_slice(),
                [ServerMsg::Done {
                    exit_code: Some(0),
                    ..
                }]
            ),
            "{out:?}"
        );
    }

    #[tokio::test]
    async fn ai_without_quotes_yields_error_only() {
        let (tx, mut rx) = unbounded_channel();
        run_shell_command(
            deps(tx, Registry::shared()).await,
            "c1".into(),
            "/ai ciao".into(),
            None,
            false,
            CancellationToken::new(),
        )
        .await;
        let out = drain(&mut rx);
        assert!(
            matches!(
                out.as_slice(),
                [ServerMsg::Error {
                    code: ErrCode::RoutingError,
                    ..
                }]
            ),
            "{out:?}"
        );
    }

    #[tokio::test]
    async fn reset_answers_not_applicable() {
        let (tx, mut rx) = unbounded_channel();
        run_shell_command(
            deps(tx, Registry::shared()).await,
            "c1".into(),
            "/reset".into(),
            None,
            false,
            CancellationToken::new(),
        )
        .await;
        let out = drain(&mut rx);
        assert!(
            matches!(&out[0], ServerMsg::Chunk { content, .. } if content == RESET_MESSAGE),
            "{out:?}"
        );
        assert!(matches!(
            &out[1],
            ServerMsg::Done {
                exit_code: Some(0),
                ..
            }
        ));
    }

    #[tokio::test]
    async fn open_ui_local_goes_to_ui_sink_or_warns_without_it() {
        let (tx, mut rx) = unbounded_channel();
        let registry = Registry::shared();
        run_shell_command(
            deps(tx.clone(), Arc::clone(&registry)).await,
            "c1".into(),
            "/config".into(),
            None,
            false,
            CancellationToken::new(),
        )
        .await;
        let out = drain(&mut rx);
        assert!(
            matches!(&out[0], ServerMsg::Chunk { content, .. } if content == NO_UI_ACK),
            "{out:?}"
        );
        assert!(matches!(
            &out[1],
            ServerMsg::Done {
                exit_code: Some(1),
                ..
            }
        ));

        let (ui_tx, mut ui_rx) = unbounded_channel();
        registry.lock().await.set_ui_sink(ui_tx);
        run_shell_command(
            deps(tx, registry).await,
            "c2".into(),
            "/library".into(),
            None,
            false,
            CancellationToken::new(),
        )
        .await;
        assert!(
            matches!(ui_rx.try_recv(), Ok(ServerMsg::OpenUiLocal { name }) if name == "library")
        );
        let out = drain(&mut rx);
        assert!(
            matches!(
                out.as_slice(),
                [ServerMsg::Done {
                    exit_code: Some(0),
                    ..
                }]
            ),
            "{out:?}"
        );
    }

    /// `/ai "x"` con `StubAdapter`: il testo dello stub finisce nella finestra
    /// (`OutputWindowContent` su `ui`), la shell riceve conferma + `Done`, la
    /// cwd del `Command` aggiorna la sessione.
    #[tokio::test]
    async fn ai_turn_opens_output_window_and_acks_the_shell() {
        let (tx, mut rx) = unbounded_channel();
        let registry = Registry::shared();
        let (ui_tx, mut ui_rx) = unbounded_channel();
        registry.lock().await.set_ui_sink(ui_tx);
        let d = deps(tx, registry).await;
        let shell = Arc::clone(&d.shell);
        run_shell_command(
            d,
            "c1".into(),
            "/ai \"ciao\"".into(),
            Some("C:\\nuova".into()),
            false,
            CancellationToken::new(),
        )
        .await;
        // Il turno gira in task spawnati: attendi il Done sulla shell.
        let done = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(ServerMsg::Done { .. }) = rx.recv().await {
                    break;
                }
            }
        })
        .await;
        assert!(done.is_ok(), "nessun Done entro 5 s");
        assert_eq!(shell.cwd().await, "C:\\nuova");
        let ui = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(ServerMsg::OutputWindowContent { markdown, .. }) = ui_rx.recv().await {
                    break markdown;
                }
            }
        })
        .await
        .expect("OutputWindowContent");
        assert!(ui.contains("[stub AI] ricevuto: ciao"), "{ui}");
    }

    /// `/help` dalla shell: UNA finestra su `ui` (quella di `handle_slash`),
    /// nessuna finestra di output; conferma generica + `Done` alla shell.
    #[tokio::test]
    async fn help_turn_opens_only_the_help_window() {
        let (tx, mut rx) = unbounded_channel();
        let registry = Registry::shared();
        let (ui_tx, mut ui_rx) = unbounded_channel();
        registry.lock().await.set_ui_sink(ui_tx);
        run_shell_command(
            deps(tx, registry).await,
            "c1".into(),
            "/help".into(),
            None,
            false,
            CancellationToken::new(),
        )
        .await;
        let ack = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(ServerMsg::Chunk { content, .. }) = rx.recv().await {
                    break content;
                }
            }
        })
        .await
        .expect("Chunk di conferma");
        assert_eq!(ack, "\u{2192} finestra aperta");
        let mut ui = vec![];
        while let Ok(m) = ui_rx.try_recv() {
            ui.push(m);
        }
        assert!(
            ui.iter().any(|m| matches!(
                m,
                ServerMsg::OpenWindow {
                    kind: protocol::WindowKind::Help,
                    ..
                }
            )),
            "{ui:?}"
        );
        assert!(
            !ui.iter().any(|m| matches!(
                m,
                ServerMsg::OpenOutputWindow { .. } | ServerMsg::OutputWindowContent { .. }
            )),
            "{ui:?}"
        );
    }

    /// `/ping` senza plugin e senza `ui`: tabella con le righe degradate,
    /// consegnata sulla finestra (qui assente → solo l'avviso) e `Done`.
    #[tokio::test]
    async fn ping_without_layers_still_completes() {
        let (tx, mut rx) = unbounded_channel();
        run_shell_command(
            deps(tx, Registry::shared()).await,
            "c1".into(),
            "/ping".into(),
            None,
            false,
            CancellationToken::new(),
        )
        .await;
        let done = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match rx.recv().await {
                    Some(ServerMsg::Done { exit_code, .. }) => break exit_code,
                    Some(_) => continue,
                    None => panic!("canale chiuso senza Done"),
                }
            }
        })
        .await
        .expect("Done");
        assert_eq!(done, Some(0));
    }
}
