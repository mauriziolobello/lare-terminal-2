//! Integration tests for the WebSocket transport layer.
//!
//! These tests spin up a real `ws::serve` instance on an ephemeral port,
//! connect a WS client, and verify end-to-end behaviour:
//!
//! - Correct token + Command → ServerInfo, Chunk(s), Done.
//! - Wrong token → connection closed before ServerInfo.
//! - Ping → Pong.
//!
//! The tool client is [`FakeToolClient`] so no child process or mcp-server
//! binary is needed.  This makes the integration test reliable in CI and
//! on a freshly cloned repo (no `cargo build` of mcp-server required).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use futures_util::{SinkExt, StreamExt};
use orchestrator::{
    agent::TurnOptions,
    ai_adapter::{AiAdapter, StubAdapter, ToolConfirmer},
    aichat::service::ServiceEvent,
    cwd_tracking::CwdTrackingToolClient,
    messages_client::ConversationHistory,
    plugins::{
        discovery::DiscoveredPlugin,
        host::PluginHost,
        transport::{PluginWriter, PluginReader},
    },
    runtime_config::RuntimeConfig,
    search::{
        paths_config::OsPathProvider,
        SearchContext, SearchEngine,
    },
    tool_client::{FakeToolClient, ToolClient},
    ws,
};
use protocol::{ClientMsg, CommandKind, ErrCode, InputMode, Role, ServerMsg};
use async_trait::async_trait;
use tokio::net::TcpListener;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::Mutex;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tokio_util::sync::CancellationToken;

// ── Test harness helpers ───────────────────────────────────────────────────────

/// `RuntimeConfig` di comodo per questi test (2.0, Task 4): `StartupConfig::
/// default()` + un `config_dir` fittizio — questo file non ha accesso a
/// `RuntimeConfig::for_test` (`#[cfg(test)]` interno al crate `orchestrator`,
/// non visibile da un binario di test esterno come questo), quindi costruisce
/// direttamente lo struct pubblico. Nessun test qui esercita i canali esterni
/// (nmap/python) o `dispatch("set_ai_display_name", ...)`, che sono gli unici
/// punti che leggono `config_dir` — un path non esistente è innocuo.
fn test_rt() -> Arc<RuntimeConfig> {
    Arc::new(RuntimeConfig {
        config_dir: std::env::temp_dir().join("lare-ws-integration-test-config"),
        startup: startup_config::StartupConfig::default(),
    })
}

/// Writes a minimal `search-paths.json` with empty roots to a unique temp path
/// and returns that path. Using empty roots ensures that integration tests only
/// search the `cwd` from the `Command`, without OsPathProvider adding system
/// directories that could produce thousands of hits and saturate `result_cap`.
fn make_test_cfg_path() -> std::path::PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("lare-ws-test-cfg-{}-{n}.json", std::process::id()));
    std::fs::write(
        &path,
        r#"{"standard":[],"cloud":[],"external":[],"max_depth":8,"result_cap":10000,"exclude":[],"version":2}"#,
    )
    .unwrap();
    path
}

/// Writes a minimal `search-content.json` to a unique temp path — analogous to
/// `make_test_cfg_path`, for `SearchContext.content_cfg_path` (Task 9). None of
/// the tests in this file exercise `in:"..."` content search, so this file's
/// content is never actually read by `launch`; it exists only to give
/// `SearchContext` a valid path for the field.
fn make_test_content_cfg_path() -> std::path::PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("lare-ws-test-content-cfg-{}-{n}.json", std::process::id()));
    std::fs::write(
        &path,
        r#"{"text_extensions":["txt"],"binary_extensions":[],"max_file_size_kb":5120,"max_unknown_scan":100,"version":1}"#,
    )
    .unwrap();
    path
}

/// Spawn a WS server on an OS-chosen ephemeral port, con l'`AiAdapter` scelto
/// dal chiamante (Task 10: `spawn_server` resta la comodità con `StubAdapter`,
/// gli scenari col round-trip AI→tool→shell usano `ToolCallingStubAdapter`).
/// Returns the `ws://` URL and a join handle for the server task.
async fn spawn_server_with(token: &str, ai: Arc<dyn AiAdapter>) -> String {
    // Bind to an ephemeral port to avoid conflicts between parallel tests.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener); // Release the port briefly — we'll re-bind inside serve().

    let token_str = token.to_string();
    let addr_str = addr.to_string();
    let addr_str_clone = addr_str.clone();

    tokio::spawn(async move {
        let tools = Arc::new(FakeToolClient::success("fake output\n"));
        // SearchContext con liste vuote + external []: i root di una ricerca
        // sono solo la cwd passata nel Command (il provider non viene interrogato).
        // Scriviamo il JSON su disco con radici vuote per evitare che load_or_generate
        // usi OsPathProvider e aggiunga directory di sistema che potrebbero saturare result_cap.
        let cfg_path = make_test_cfg_path();
        let content_cfg_path = make_test_content_cfg_path();
        let search = SearchContext {
            engine: Arc::new(SearchEngine::default()),
            cfg_path: Arc::new(cfg_path),
            content_cfg_path: Arc::new(content_cfg_path),
            provider: Arc::new(OsPathProvider),
        };
        // cwd_state: empty string = no cwd tracking in basic tests (compat).
        let cwd_state = Arc::new(Mutex::new(String::new()));

        // Plugin host vuoto: i test di integrazione WS non testano i plugin.
        // La factory `|_p|` non viene mai chiamata (nessun plugin scoperto).
        let plugin_host = Arc::new(tokio::sync::Mutex::new(
            PluginHost::start(
                vec![],
                |_p: &DiscoveredPlugin| -> std::io::Result<(Box<dyn PluginWriter>, Box<dyn PluginReader>)> {
                    unreachable!("nessun plugin nei test di integrazione WS")
                },
                std::path::Path::new("."),
            )
            .await,
        ));
        let plugin_commands: Arc<Vec<(String, String)>> = Arc::new(vec![]);

        // Pass the addr string to ws::serve so it binds the ephemeral port.
        // `None` = canale AI Chat non attivo in questo helper di test generico.
        ws::serve(&addr_str_clone, Arc::new(token_str), ai, tools, cwd_state, search, plugin_host, plugin_commands, None, tokio_util::sync::CancellationToken::new(), test_rt(), orchestrator::connections::Registry::shared())
            .await
            .ok();
    });

    // Give the server a moment to bind.
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    format!("ws://{addr}")
}

/// Comodità: la maggior parte dei test non ha bisogno di un'AI reale, solo
/// dello `StubAdapter` (eco testuale, nessun tool). Invariato rispetto al
/// vecchio `spawn_server` — ora è solo un guscio sottile su `spawn_server_with`.
async fn spawn_server(token: &str) -> String {
    spawn_server_with(token, Arc::new(StubAdapter)).await
}

/// Send a [`ClientMsg`] as a JSON text frame.
async fn send(
    sink: &mut (impl SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin),
    msg: &ClientMsg,
) {
    let json = serde_json::to_string(msg).unwrap();
    sink.send(Message::Text(json)).await.unwrap();
}

/// Receive the next text frame and deserialize it as [`ServerMsg`].
async fn recv(
    source: &mut (impl StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin),
) -> ServerMsg {
    loop {
        match source.next().await {
            Some(Ok(Message::Text(t))) => {
                return serde_json::from_str(&t)
                    .unwrap_or_else(|e| panic!("failed to parse ServerMsg: {e}; raw: {t}"));
            }
            Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => continue,
            Some(Ok(other)) => panic!("unexpected WS frame: {other:?}"),
            Some(Err(e)) => panic!("WS read error: {e}"),
            None => panic!("connection closed unexpectedly"),
        }
    }
}

// ── Task 10: helper e adapter finto per gli scenari e2e del canale shell ───────

/// Adapter AI finto che, come farebbe Claude, propone UN comando: chiede il
/// gate (se il confirmer lo gateizza), esegue `run_in_session` via `dispatch`
/// e riporta l'output come testo. Serve a provare il round-trip completo
/// ToolConfirmRequest → ExecInShell → ExecResult senza API reali.
struct ToolCallingStubAdapter;

#[async_trait]
impl AiAdapter for ToolCallingStubAdapter {
    #[allow(clippy::too_many_arguments)]
    async fn respond(
        &self,
        id: &str,
        _input: &str,
        _history: &mut ConversationHistory,
        tools: &dyn ToolClient,
        _opts: TurnOptions,
        confirmer: Option<&dyn ToolConfirmer>,
        _cancel: Option<CancellationToken>,
        tx: UnboundedSender<ServerMsg>,
    ) {
        let approved = match confirmer {
            Some(c) if c.should_gate("run_in_session") => c.confirm("$ echo x").await,
            _ => true,
        };
        if !approved {
            let _ = tx.send(ServerMsg::Chunk { id: id.to_string(), content: "annullato".into() });
            let _ = tx.send(ServerMsg::Done { id: id.to_string(), exit_code: None });
            return;
        }
        let out = tools.dispatch("run_in_session", &serde_json::json!({"command": "echo x"})).await;
        let _ = tx.send(ServerMsg::Chunk { id: id.to_string(), content: format!("output: {}", out.output.trim()) });
        let _ = tx.send(ServerMsg::Done { id: id.to_string(), exit_code: None });
    }

    fn provider(&self) -> String {
        "tool-calling-stub".to_string()
    }

    async fn chat_reply(&self, _my_ai_label: &str, _history: &[orchestrator::messages_client::Message], _request: &str, _cancel: Option<CancellationToken>) -> String {
        String::new()
    }
}

/// `Hello{role: Shell}` di comodo per gli scenari: session `s1`, cwd `C:\w`,
/// versione della host finta `2.0.0` (finisce nella riga `/ping`).
fn shell_hello(token: &str) -> ClientMsg {
    ClientMsg::Hello {
        token: token.into(), channel: None, role: Role::Shell,
        session_id: Some("s1".into()), cwd: Some("C:\\w".into()), version: Some("2.0.0".into()),
    }
}

/// `Hello{role: Ui}` di comodo (nessuna sessione/cwd/versione — come `ui.exe`).
fn ui_hello(token: &str) -> ClientMsg {
    ClientMsg::Hello { token: token.into(), channel: None, role: Role::Ui, session_id: None, cwd: None, version: None }
}

/// `Command` di comodo per gli scenari shell: cwd fissa `C:\w`, niente ricerca web.
fn command(id: &str, input: &str) -> ClientMsg {
    ClientMsg::Command {
        id: id.into(), input: input.into(), input_mode: InputMode::Keyboard,
        command_type: CommandKind::Auto, cwd: Some("C:\\w".into()), web_search: false,
        lang: String::new(),
    }
}

/// Riceve finché `pred` è vera (max 5 s), ritornando il messaggio che l'ha soddisfatta.
/// Serve dove un messaggio atteso può essere preceduto da altri (es. un
/// `ActivityIndicator` prima di un `OpenOutputWindow`/`ToolConfirmRequest`).
async fn recv_until(
    source: &mut (impl StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin),
    pred: impl Fn(&ServerMsg) -> bool,
) -> ServerMsg {
    tokio::time::timeout(tokio::time::Duration::from_secs(5), async {
        loop {
            let m = recv(source).await;
            if pred(&m) { return m; }
        }
    }).await.expect("timeout: messaggio atteso mai arrivato")
}

// ── Tests ──────────────────────────────────────────────────────────────────────

/// Correct handshake: Hello → ServerInfo → Command → Chunk + Done.
#[tokio::test]
async fn correct_token_gets_server_info_then_command_response() {
    let url = spawn_server("test-token-correct").await;
    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut sink, mut source) = ws.split();

    // Send Hello with correct token.
    send(
        &mut sink,
        &ClientMsg::Hello {
            token: "test-token-correct".to_string(),
            channel: None,
            role: protocol::Role::Ui,
            session_id: None,
            cwd: None,
            version: None,
        },
    )
    .await;

    // First response must be ServerInfo.
    let info = recv(&mut source).await;
    assert!(
        matches!(info, ServerMsg::ServerInfo { .. }),
        "expected ServerInfo, got {info:?}"
    );

    // Send a command.
    send(
        &mut sink,
        &ClientMsg::Command {
            id: "t1".to_string(),
            input: "dir".to_string(),
            input_mode: InputMode::Keyboard,
            command_type: CommandKind::Os,
            cwd: None,
            web_search: false,
            lang: String::new(),
        },
    )
    .await;

    // Collect responses until Done.
    let mut chunks: Vec<String> = vec![];
    let done = loop {
        let msg = recv(&mut source).await;
        match msg {
            ServerMsg::Chunk { content, .. } => chunks.push(content),
            ServerMsg::Done { .. } => break msg,
            other => panic!("unexpected message: {other:?}"),
        }
    };

    // FakeToolClient returns "fake output\n".
    assert!(
        chunks.iter().any(|c| c.contains("fake output")),
        "expected 'fake output' in chunks: {chunks:?}"
    );

    assert!(
        matches!(
            done,
            ServerMsg::Done {
                exit_code: Some(0),
                ..
            }
        ),
        "expected Done{{exit_code: 0}}, got {done:?}"
    );
}

/// Wrong token: connection must be closed without sending ServerInfo.
#[tokio::test]
async fn wrong_token_closes_connection() {
    let url = spawn_server("secret-token").await;
    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut sink, mut source) = ws.split();

    send(
        &mut sink,
        &ClientMsg::Hello {
            token: "wrong-token".to_string(),
            channel: None,
            role: protocol::Role::Ui,
            session_id: None,
            cwd: None,
            version: None,
        },
    )
    .await;

    // The server should close the connection; we expect either a Close frame
    // or a stream end — NOT a ServerInfo.
    let next = source.next().await;
    match next {
        None | Some(Ok(Message::Close(_))) => {
            // Expected: connection closed cleanly.
        }
        Some(Ok(Message::Text(t))) => {
            let msg: ServerMsg = serde_json::from_str(&t)
                .unwrap_or_else(|_| panic!("received unexpected text after wrong token: {t}"));
            panic!("expected connection close, got ServerMsg: {msg:?}");
        }
        Some(other) => {
            // Ping/Pong/Binary are unexpected but not the ServerInfo we're
            // guarding against — fail explicitly.
            panic!("unexpected frame after wrong token: {other:?}");
        }
    }
}

/// `Hello{channel: Some("nope")}` con il registro di produzione vuoto
/// (`EXTERNAL_TOOL_CHANNELS`): la connessione deve ricevere un `Error`
/// leggibile (non un panic, non `ServerInfo`) e poi chiudersi.
#[tokio::test]
async fn unknown_channel_gets_error_and_connection_closes_not_panics() {
    let url = spawn_server("unknown-channel-token").await;
    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut sink, mut source) = ws.split();

    send(
        &mut sink,
        &ClientMsg::Hello {
            token: "unknown-channel-token".to_string(),
            channel: Some("nope".to_string()),
            role: protocol::Role::Ui,
            session_id: None,
            cwd: None,
            version: None,
        },
    )
    .await;

    // Il canale è sconosciuto (registro di produzione vuoto): niente ServerInfo,
    // un Error leggibile, poi la connessione chiude — mai un panic del server.
    let msg = recv(&mut source).await;
    match msg {
        ServerMsg::Error { code, message, .. } => {
            assert_eq!(code, protocol::ErrCode::RoutingError);
            assert!(message.contains("nope"), "il messaggio deve nominare il canale: {message}");
        }
        other => panic!("expected Error, got {other:?}"),
    }

    // Stesso pattern di verifica-chiusura di `wrong_token_closes_connection` (sopra
    // in questo file): dopo l'Error, il prossimo frame deve essere la chiusura,
    // non un'altra ServerInfo/Text.
    let next = source.next().await;
    match next {
        None | Some(Ok(Message::Close(_))) => {
            // Expected: connection closed cleanly.
        }
        Some(Ok(Message::Text(t))) => {
            panic!("expected connection close after Error, got text: {t}");
        }
        Some(other) => {
            panic!("unexpected frame after Error: {other:?}");
        }
    }
}

/// Ping → Pong (application-level keep-alive).
#[tokio::test]
async fn ping_receives_pong() {
    let url = spawn_server("ping-test-token").await;
    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut sink, mut source) = ws.split();

    // Handshake.
    send(
        &mut sink,
        &ClientMsg::Hello {
            token: "ping-test-token".to_string(),
            channel: None,
            role: protocol::Role::Ui,
            session_id: None,
            cwd: None,
            version: None,
        },
    )
    .await;
    let _info = recv(&mut source).await; // ServerInfo

    // Send application-level Ping.
    send(&mut sink, &ClientMsg::Ping { ts: 12345 }).await;

    // Expect Pong with same ts.
    let msg = recv(&mut source).await;
    assert_eq!(
        msg,
        ServerMsg::Pong { ts: 12345 },
        "expected Pong{{ts:12345}}, got {msg:?}"
    );
}

/// `ToolConfirmResponse` con un id sconosciuto (nessuna richiesta di conferma
/// pendente — nessun tool sensibile esiste ancora in questo piano) è un no-op
/// silenzioso: non deve chiudere né bloccare la connessione. Stesso principio già
/// verificato per `CancelSearch`/`CancelCommand` su id ignoti.
#[tokio::test]
async fn tool_confirm_response_unknown_id_keeps_connection_alive() {
    let url = spawn_server("test-token-confirm").await;
    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut sink, mut source) = ws.split();

    send(&mut sink, &ClientMsg::Hello { token: "test-token-confirm".to_string(), channel: None, role: protocol::Role::Ui, session_id: None, cwd: None, version: None, }).await;
    let info = recv(&mut source).await;
    assert!(matches!(info, ServerMsg::ServerInfo { .. }), "expected ServerInfo, got {info:?}");

    send(&mut sink, &ClientMsg::ToolConfirmResponse { id: "mai-esistito".to_string(), accept: true }).await;

    // La connessione resta viva: un Ping successivo riceve ancora Pong.
    send(&mut sink, &ClientMsg::Ping { ts: 42 }).await;
    let pong = recv(&mut source).await;
    assert!(matches!(pong, ServerMsg::Pong { ts: 42 }), "expected Pong, got {pong:?}");
}

/// Natural-language command: stub response expected.
#[tokio::test]
async fn nl_command_returns_stub_response() {
    let url = spawn_server("nl-test-token").await;
    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut sink, mut source) = ws.split();

    send(
        &mut sink,
        &ClientMsg::Hello {
            token: "nl-test-token".to_string(),
            channel: None,
            role: protocol::Role::Ui,
            session_id: None,
            cwd: None,
            version: None,
        },
    )
    .await;
    let _info = recv(&mut source).await;

    send(
        &mut sink,
        &ClientMsg::Command {
            id: "nl-1".to_string(),
            input: "What is Rust?".to_string(),
            input_mode: InputMode::Keyboard,
            command_type: CommandKind::Nl,
            cwd: None,
            web_search: false,
            lang: String::new(),
        },
    )
    .await;

    let chunk = recv(&mut source).await;
    match &chunk {
        ServerMsg::Chunk { content, id } => {
            assert_eq!(id, "nl-1");
            assert!(
                content.contains("[stub AI]"),
                "expected stub AI response, got: {content:?}"
            );
        }
        other => panic!("expected Chunk, got {other:?}"),
    }

    let done = recv(&mut source).await;
    assert!(
        matches!(
            done,
            ServerMsg::Done {
                exit_code: None,
                ..
            }
        ),
        "expected Done{{exit_code: None}}, got {done:?}"
    );
}

// ── Ricerca file (Task 7) ────────────────────────────────────────────────────

/// `/find` apre una finestra di ricerca live e invia i risultati in streaming:
/// SearchOpen → SearchHit(s) → SearchDone. La cwd della ricerca è quella passata
/// nel Command (qui un temp dir con un file noto).
#[tokio::test]
async fn find_command_streams_search_results() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("hello.txt"), "hi").unwrap();
    let cwd = dir.path().to_string_lossy().to_string();

    let url = spawn_server("find-token").await;
    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut sink, mut source) = ws.split();
    send(&mut sink, &ClientMsg::Hello { token: "find-token".to_string(), channel: None, role: protocol::Role::Ui, session_id: None, cwd: None, version: None, }).await;
    let _info = recv(&mut source).await;

    send(
        &mut sink,
        &ClientMsg::Command {
            id: "f1".to_string(),
            input: "/find *.txt".to_string(),
            input_mode: InputMode::Keyboard,
            command_type: CommandKind::Auto,
            cwd: Some(cwd),
            web_search: false,
            lang: String::new(),
        },
    )
    .await;

    // Dopo /find il server emette `Done{id del Command}` (la UI chiude lo spinner) e,
    // indipendentemente, i messaggi di ricerca con un id proprio (`sid`). Salta il Done
    // del comando e attendi SearchOpen.
    loop {
        match recv(&mut source).await {
            ServerMsg::SearchOpen { .. } => break,
            ServerMsg::Done { exit_code: Some(0), .. } => continue,
            other => panic!("atteso SearchOpen (o il Done del comando), got {other:?}"),
        }
    }

    let mut got_hit = false;
    loop {
        match recv(&mut source).await {
            ServerMsg::SearchHit { path, .. } => {
                if path.contains("hello.txt") {
                    got_hit = true;
                }
            }
            ServerMsg::SearchDone { truncated, .. } => {
                assert!(!truncated, "non dovrebbe essere troncato");
                break;
            }
            ServerMsg::Done { .. } => continue, // Done del comando (ordine non garantito)
            other => panic!("messaggio inatteso durante /find: {other:?}"),
        }
    }
    assert!(got_hit, "atteso un SearchHit per hello.txt");
}

/// `/find folder:<valore-non-riconosciuto>` deve rispondere con un errore
/// esplicito PRIMA di aprire la finestra di ricerca — nessun `SearchOpen`
/// deve mai arrivare (spec `2026-07-10-find-folder-scope-design.md` §4).
#[tokio::test]
async fn find_folder_invalid_value_errors_before_opening_search_window() {
    let url = spawn_server("find-folder-token").await;
    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut sink, mut source) = ws.split();
    send(&mut sink, &ClientMsg::Hello { token: "find-folder-token".to_string(), channel: None, role: protocol::Role::Ui, session_id: None, cwd: None, version: None, }).await;
    let _info = recv(&mut source).await;

    send(
        &mut sink,
        &ClientMsg::Command {
            id: "f1".to_string(),
            input: "/find folder:altrove".to_string(),
            input_mode: InputMode::Keyboard,
            command_type: CommandKind::Auto,
            cwd: None,
            web_search: false,
            lang: String::new(),
        },
    )
    .await;

    match recv(&mut source).await {
        ServerMsg::Error { code: protocol::ErrCode::RoutingError, message, .. } => {
            assert!(
                message.contains("altrove"),
                "il messaggio deve nominare il valore rifiutato: {message}"
            );
        }
        other => panic!("atteso Error{{RoutingError}}, got {other:?}"),
    }

    match recv(&mut source).await {
        ServerMsg::Done { exit_code: Some(1), .. } => {}
        other => panic!("atteso Done{{exit_code: Some(1)}}, got {other:?}"),
    }

    // Rinforzo (final review): i due `recv` sopra da soli non provano che il
    // gate abbia davvero IMPEDITO l'apertura della finestra di ricerca — un
    // eventuale `SearchOpen` spedito DOPO `Error`+`Done` passerebbe comunque
    // inosservato (mutation testing lo ha confermato: rimuovendo il `continue`
    // del gate in `ws.rs`, questo test restava verde). Con un breve timeout
    // confermiamo che non arriva NESSUN altro messaggio — in particolare non
    // `SearchOpen` — entro una finestra ragionevole dopo `Done`.
    match tokio::time::timeout(std::time::Duration::from_millis(200), recv(&mut source)).await {
        Err(_) => {} // timeout: nessun messaggio ulteriore — atteso, il gate ha bloccato tutto.
        Ok(ServerMsg::SearchOpen { .. }) => {
            panic!("SearchOpen NON deve essere emesso per un folder: invalido — il gate ha fallito")
        }
        Ok(other) => panic!("nessun messaggio ulteriore atteso dopo Done, got {other:?}"),
    }
}

/// Smoke test di responsività: un Ping inviato subito dopo `/find` ottiene il Pong e
/// la connessione non si blocca. NB: con una dir piccola la ricerca è quasi istantanea,
/// quindi l'ordine Pong-vs-SearchDone è racy e NON viene asserito (sarebbe un falso
/// segnale). La prova forte del non-blocco è strutturale (`/find` è spawnato, non
/// awaited nel read-loop); l'interruzione deterministica del walk è in `walk` (T5).
#[tokio::test]
async fn loop_stays_responsive_during_find() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "x").unwrap();
    let cwd = dir.path().to_string_lossy().to_string();

    let url = spawn_server("resp-token").await;
    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut sink, mut source) = ws.split();
    send(&mut sink, &ClientMsg::Hello { token: "resp-token".to_string(), channel: None, role: protocol::Role::Ui, session_id: None, cwd: None, version: None, }).await;
    let _info = recv(&mut source).await;

    send(
        &mut sink,
        &ClientMsg::Command {
            id: "f".to_string(),
            input: "/find *.txt".to_string(),
            input_mode: InputMode::Keyboard,
            command_type: CommandKind::Auto,
            cwd: Some(cwd.clone()),
            web_search: false,
            lang: String::new(),
        },
    )
    .await;
    send(&mut sink, &ClientMsg::Ping { ts: 777 }).await;

    // Tra i messaggi in arrivo (SearchOpen/Hit/Done + Pong) deve comparire Pong{777}.
    let mut saw_pong = false;
    for _ in 0..30 {
        match recv(&mut source).await {
            ServerMsg::Pong { ts } => {
                assert_eq!(ts, 777);
                saw_pong = true;
                break;
            }
            ServerMsg::SearchOpen { .. }
            | ServerMsg::SearchHit { .. }
            | ServerMsg::SearchDone { .. }
            | ServerMsg::Done { .. } => continue,
            other => panic!("messaggio inatteso: {other:?}"),
        }
    }
    assert!(saw_pong, "atteso Pong → la connessione resta responsiva, niente blocco");
}

/// `CancelSearch` non blocca la connessione: dopo `/find` + `CancelSearch{id}`, un
/// Ping successivo ottiene Pong. NB: con una dir piccola la ricerca può finire prima
/// che il cancel arrivi (→ `searches.remove` no-op); questo test prova il WIRING del
/// cancel + il non-blocco, NON l'interruzione mid-walk (deterministica in `walk`, T5).
#[tokio::test]
async fn cancel_search_keeps_connection_alive() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "x").unwrap();
    let cwd = dir.path().to_string_lossy().to_string();

    let url = spawn_server("cancel-token").await;
    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut sink, mut source) = ws.split();
    send(&mut sink, &ClientMsg::Hello { token: "cancel-token".to_string(), channel: None, role: protocol::Role::Ui, session_id: None, cwd: None, version: None, }).await;
    let _info = recv(&mut source).await;

    send(
        &mut sink,
        &ClientMsg::Command {
            id: "f".to_string(),
            input: "/find *.txt".to_string(),
            input_mode: InputMode::Keyboard,
            command_type: CommandKind::Auto,
            cwd: Some(cwd),
            web_search: false,
            lang: String::new(),
        },
    )
    .await;

    // Leggi SearchOpen per ottenere l'id della ricerca.
    let sid = loop {
        match recv(&mut source).await {
            ServerMsg::SearchOpen { id, .. } => break id,
            ServerMsg::SearchHit { .. }
            | ServerMsg::SearchDone { .. }
            | ServerMsg::Done { .. } => continue,
            other => panic!("inatteso prima di SearchOpen: {other:?}"),
        }
    };

    send(&mut sink, &ClientMsg::CancelSearch { id: sid }).await;
    send(&mut sink, &ClientMsg::Ping { ts: 99 }).await;

    let mut saw_pong = false;
    for _ in 0..30 {
        match recv(&mut source).await {
            ServerMsg::Pong { ts } => {
                assert_eq!(ts, 99);
                saw_pong = true;
                break;
            }
            ServerMsg::SearchHit { .. }
            | ServerMsg::SearchDone { .. }
            | ServerMsg::Done { .. } => continue,
            other => panic!("inatteso dopo CancelSearch: {other:?}"),
        }
    }
    assert!(saw_pong, "la connessione deve restare viva dopo CancelSearch");
}

// ── cwd tracking + ServerMsg::Cwd (Task 3 / Steps 4-5) ────────────────────────

/// Spawn a WS server that uses a `CwdTrackingToolClient` with a pre-set cwd.
///
/// RED: `ws::serve` doesn't accept `cwd_state` yet → compile error until
/// the signature is updated.
async fn spawn_server_with_cwd(token: &str, initial_cwd: &str, fake_cwd: &str) -> (String, Arc<Mutex<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);

    let token_str = token.to_string();
    let addr_str = addr.to_string();
    let addr_str_clone = addr_str.clone();

    let cwd_state = Arc::new(Mutex::new(initial_cwd.to_string()));
    let cwd_state_clone = Arc::clone(&cwd_state);
    let fake_cwd_str = fake_cwd.to_string();

    tokio::spawn(async move {
        let ai = Arc::new(StubAdapter);
        // Use a fake that reports a specific cwd, wrapped by the tracker.
        let fake = Arc::new(FakeToolClient::with_cwd("fake output\n", &fake_cwd_str));
        let tools = Arc::new(CwdTrackingToolClient::new(fake, Arc::clone(&cwd_state_clone), std::path::PathBuf::from("/test-config")));

        // Stesso approccio di spawn_server: config con radici vuote.
        let cfg_path = make_test_cfg_path();
        let content_cfg_path = make_test_content_cfg_path();
        let search = SearchContext {
            engine: Arc::new(SearchEngine::default()),
            cfg_path: Arc::new(cfg_path),
            content_cfg_path: Arc::new(content_cfg_path),
            provider: Arc::new(OsPathProvider),
        };
        // Plugin host vuoto anche per i test con cwd tracking.
        let plugin_host = Arc::new(tokio::sync::Mutex::new(
            PluginHost::start(
                vec![],
                |_p: &DiscoveredPlugin| -> std::io::Result<(Box<dyn PluginWriter>, Box<dyn PluginReader>)> {
                    unreachable!("nessun plugin nei test di integrazione WS")
                },
                std::path::Path::new("."),
            )
            .await,
        ));
        let plugin_commands: Arc<Vec<(String, String)>> = Arc::new(vec![]);

        // `None` = canale AI Chat non attivo in questo helper.
        ws::serve(&addr_str_clone, Arc::new(token_str), ai, tools, Arc::clone(&cwd_state_clone), search, plugin_host, plugin_commands, None, tokio_util::sync::CancellationToken::new(), test_rt(), orchestrator::connections::Registry::shared())
            .await
            .ok();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    (format!("ws://{addr}"), cwd_state)
}

/// On connection, the server emits `ServerMsg::Cwd` with the initial cwd
/// right after `ServerInfo`.
///
/// RED: `ws::serve` doesn't emit `Cwd` yet; test will timeout/fail.
#[tokio::test]
async fn connection_emits_initial_cwd() {
    let initial = "/home/testuser";
    let (url, _state) = spawn_server_with_cwd("cwd-init-token", initial, "").await;
    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut sink, mut source) = ws.split();

    send(&mut sink, &ClientMsg::Hello { token: "cwd-init-token".to_string(), channel: None, role: protocol::Role::Ui, session_id: None, cwd: None, version: None, }).await;

    let info = recv(&mut source).await;
    assert!(matches!(info, ServerMsg::ServerInfo { .. }), "expected ServerInfo, got {info:?}");

    // The next message after ServerInfo must be Cwd with the initial path.
    let cwd_msg = recv(&mut source).await;
    match cwd_msg {
        ServerMsg::Cwd { path } => {
            assert_eq!(path, initial, "expected initial cwd '{initial}', got '{path}'");
        }
        other => panic!("expected ServerMsg::Cwd, got {other:?}"),
    }
}

/// After an OS command that changes cwd, the server emits a new `ServerMsg::Cwd`.
///
/// RED: `ws::serve` doesn't emit `Cwd` after commands yet.
#[tokio::test]
async fn command_that_changes_cwd_emits_new_cwd() {
    let initial = "/home/testuser";
    let new_cwd = "/home/testuser/projects";
    let (url, _state) = spawn_server_with_cwd("cwd-change-token", initial, new_cwd).await;
    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut sink, mut source) = ws.split();

    send(&mut sink, &ClientMsg::Hello { token: "cwd-change-token".to_string(), channel: None, role: protocol::Role::Ui, session_id: None, cwd: None, version: None, }).await;

    // Consume ServerInfo + initial Cwd.
    let _info = recv(&mut source).await;
    let _initial_cwd = recv(&mut source).await; // the initial Cwd message

    // Send an OS command.
    send(
        &mut sink,
        &ClientMsg::Command {
            id: "c1".to_string(),
            input: "cd projects".to_string(),
            input_mode: InputMode::Keyboard,
            command_type: CommandKind::Os,
            cwd: None,
            web_search: false,
            lang: String::new(),
        },
    )
    .await;

    // Collect all messages until we see the new Cwd (may arrive after Done).
    // The order from ws.rs is: Chunk(s) → Done → Cwd (emitted after handle_command returns).
    let mut saw_new_cwd = false;
    let mut saw_done = false;
    for _ in 0..20 {
        match recv(&mut source).await {
            ServerMsg::Cwd { path } => {
                if path == new_cwd {
                    saw_new_cwd = true;
                    break; // found what we were looking for
                }
            }
            ServerMsg::Done { .. } => {
                saw_done = true;
                // Don't break — Cwd arrives AFTER Done.
            }
            ServerMsg::Chunk { .. } => continue,
            other => panic!("unexpected message: {other:?}"),
        }
        // Stop after Done+Cwd both seen, or after Done if cwd already found.
        if saw_done && saw_new_cwd {
            break;
        }
    }
    assert!(saw_new_cwd, "expected ServerMsg::Cwd {{ path: '{new_cwd}' }} after cd command");
}

/// `/find` uses the tracked cwd (not `current_dir()`).
///
/// RED: `/find` still uses `current_dir()` which is the orchestrator launch dir.
/// With cwd_state set to a temp dir with a known file, `/find` should find it.
#[tokio::test]
async fn find_uses_tracked_cwd() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("target.txt"), "hi").unwrap();
    let tracked_cwd = dir.path().to_string_lossy().to_string();

    // initial_cwd = tracked_cwd; fake_cwd irrelevant (no commands before /find).
    let (url, _state) = spawn_server_with_cwd("find-cwd-token", &tracked_cwd, "").await;
    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut sink, mut source) = ws.split();

    send(&mut sink, &ClientMsg::Hello { token: "find-cwd-token".to_string(), channel: None, role: protocol::Role::Ui, session_id: None, cwd: None, version: None, }).await;
    let _info = recv(&mut source).await; // ServerInfo
    let _cwd_msg = recv(&mut source).await; // initial Cwd

    // Send /find without a `cwd` in the Command (so the server must use cwd_state).
    send(
        &mut sink,
        &ClientMsg::Command {
            id: "f2".to_string(),
            input: "/find *.txt".to_string(),
            input_mode: InputMode::Keyboard,
            command_type: CommandKind::Auto,
            cwd: None, // no cwd from client — server must use tracked cwd
            web_search: false,
            lang: String::new(),
        },
    )
    .await;

    // Wait for SearchOpen.
    loop {
        match recv(&mut source).await {
            ServerMsg::SearchOpen { .. } => break,
            ServerMsg::Done { exit_code: Some(0), .. } => continue,
            other => panic!("expected SearchOpen or command Done, got {other:?}"),
        }
    }

    // Collect hits.
    let mut found = false;
    loop {
        match recv(&mut source).await {
            ServerMsg::SearchHit { path, .. } => {
                if path.contains("target.txt") {
                    found = true;
                }
            }
            ServerMsg::SearchDone { .. } => break,
            ServerMsg::Done { .. } | ServerMsg::Cwd { .. } => continue,
            other => panic!("unexpected during search: {other:?}"),
        }
    }
    assert!(found, "expected 'target.txt' in search results from tracked cwd");
}

// ── Pausa/Resume (Task 2+3) ────────────────────────────────────────────────

/// Una `/find` può essere messa in **pausa** (`PauseSearch`) e ripresa (`ResumeSearch`).
///
/// Proprietà verificate:
/// 1. Dopo `PauseSearch`, il loop resta responsivo: un `Ping` ottiene `Pong`.
/// 2. Dopo `ResumeSearch`, la ricerca finisce con `SearchDone` (non si perde).
/// 3. `CancelSearch` dopo il resume chiude pulito (la connessione resta viva).
///
/// NB: La dir di test è piccola — la ricerca può finire prima che il pause
/// arrivi. Il test verifica il *wiring* e la non-regressione (no blocco del
/// loop), non un'interruzione mid-walk deterministica (quella è in `walk`).
#[tokio::test]
async fn pause_resume_search_loop_stays_responsive() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("pause_test.txt"), "content").unwrap();
    let cwd = dir.path().to_string_lossy().to_string();

    let url = spawn_server("pause-token").await;
    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut sink, mut source) = ws.split();
    send(&mut sink, &ClientMsg::Hello { token: "pause-token".to_string(), channel: None, role: protocol::Role::Ui, session_id: None, cwd: None, version: None, }).await;
    let _info = recv(&mut source).await; // ServerInfo

    // Launch /find.
    send(
        &mut sink,
        &ClientMsg::Command {
            id: "pf1".to_string(),
            input: "/find *.txt".to_string(),
            input_mode: InputMode::Keyboard,
            command_type: CommandKind::Auto,
            cwd: Some(cwd),
            web_search: false,
            lang: String::new(),
        },
    )
    .await;

    // Collect SearchOpen to get the search id.
    let sid = loop {
        match recv(&mut source).await {
            ServerMsg::SearchOpen { id, .. } => break id,
            ServerMsg::Done { exit_code: Some(0), .. } => continue,
            other => panic!("expected SearchOpen or command Done, got {other:?}"),
        }
    };

    // Pause the search.
    send(&mut sink, &ClientMsg::PauseSearch { id: sid.clone() }).await;

    // The loop must stay responsive: Ping → Pong arrives even while (nominally) paused.
    send(&mut sink, &ClientMsg::Ping { ts: 42 }).await;

    let mut saw_pong = false;
    for _ in 0..30 {
        match recv(&mut source).await {
            ServerMsg::Pong { ts } => {
                assert_eq!(ts, 42, "Pong ts mismatch");
                saw_pong = true;
                break;
            }
            // Any search messages or command Done can arrive in any order.
            ServerMsg::SearchHit { .. }
            | ServerMsg::SearchDone { .. }
            | ServerMsg::Done { .. } => continue,
            other => panic!("unexpected message during pause: {other:?}"),
        }
    }
    assert!(saw_pong, "loop must stay responsive during PauseSearch");

    // Resume the search so it can complete, then cancel to clean up.
    send(&mut sink, &ClientMsg::ResumeSearch { id: sid.clone() }).await;
    send(&mut sink, &ClientMsg::CancelSearch { id: sid }).await;

    // Send another Ping to confirm the connection is still alive after ResumeSearch + CancelSearch.
    send(&mut sink, &ClientMsg::Ping { ts: 43 }).await;

    let mut saw_final_pong = false;
    for _ in 0..30 {
        match recv(&mut source).await {
            ServerMsg::Pong { ts } => {
                assert_eq!(ts, 43, "final Pong ts mismatch");
                saw_final_pong = true;
                break;
            }
            ServerMsg::SearchHit { .. }
            | ServerMsg::SearchDone { .. }
            | ServerMsg::Done { .. } => continue,
            other => panic!("unexpected message after resume/cancel: {other:?}"),
        }
    }
    assert!(saw_final_pong, "connection must remain alive after ResumeSearch + CancelSearch");
}

// ── AI Chat wiring tests (Task 9) ─────────────────────────────────────────────

/// Spawn un WS server con un handle REALE al canale AI Chat.
/// Il server inoltrerà i `ClientMsg::AiChat*` come `ServiceEvent` al canale
/// fornito, così il test può verificare il dispatch senza un servizio completo.
async fn spawn_server_with_aichat(
    token: &str,
    aichat_tx: tokio::sync::mpsc::UnboundedSender<ServiceEvent>,
) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener); // Rilascia brevemente: ws::serve rifa il bind sull'indirizzo.

    let token_str = token.to_string();
    let addr_str_clone = addr.to_string();

    tokio::spawn(async move {
        let ai = Arc::new(StubAdapter);
        let tools = Arc::new(FakeToolClient::success("fake output\n"));
        let cfg_path = make_test_cfg_path();
        let content_cfg_path = make_test_content_cfg_path();
        let search = SearchContext {
            engine: Arc::new(SearchEngine::default()),
            cfg_path: Arc::new(cfg_path),
            content_cfg_path: Arc::new(content_cfg_path),
            provider: Arc::new(OsPathProvider),
        };
        let cwd_state = Arc::new(Mutex::new(String::new()));
        let plugin_host = Arc::new(tokio::sync::Mutex::new(
            PluginHost::start(
                vec![],
                |_p: &DiscoveredPlugin| -> std::io::Result<(Box<dyn PluginWriter>, Box<dyn PluginReader>)> {
                    unreachable!("nessun plugin nei test AI Chat")
                },
                std::path::Path::new("."),
            )
            .await,
        ));
        let plugin_commands: Arc<Vec<(String, String)>> = Arc::new(vec![]);

        // Passa `Some(aichat_tx)` al server: il wiring è attivo.
        ws::serve(
            &addr_str_clone,
            Arc::new(token_str),
            ai,
            tools,
            cwd_state,
            search,
            plugin_host,
            plugin_commands,
            Some(aichat_tx),
            tokio_util::sync::CancellationToken::new(),
            test_rt(),
            orchestrator::connections::Registry::shared(),
        )
        .await
        .ok();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    format!("ws://{addr}")
}

/// Verifica che `ClientMsg::AiChatSend { text }` venga inoltrato come
/// `ServiceEvent::HumanSay(text)` al canale del servizio AI Chat.
///
/// Questo è il test fondamentale del wiring Task 9: la UI parla, il servizio
/// riceve. Con il no-op arm il test fallisce (RED); con i bracci reali passa (GREEN).
#[tokio::test]
async fn aichat_send_dispatches_human_say_event() {
    // Crea il canale "fake inbox" del servizio AI Chat.
    let (aichat_tx, mut aichat_rx) = tokio::sync::mpsc::unbounded_channel::<ServiceEvent>();

    let token = "aichat-test-token";
    let url = spawn_server_with_aichat(token, aichat_tx).await;

    // Connetti il client WS.
    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut sink, mut source) = ws.split();

    // Handshake.
    send(&mut sink, &ClientMsg::Hello { token: token.to_string(), channel: None, role: protocol::Role::Ui, session_id: None, cwd: None, version: None, }).await;
    // Drena ServerInfo (e l'eventuale Cwd iniziale) prima di procedere.
    loop {
        match recv(&mut source).await {
            ServerMsg::ServerInfo { .. } => break,
            _ => continue,
        }
    }

    // Invia AiChatSend.
    send(&mut sink, &ClientMsg::AiChatSend { text: "ciao".to_string() }).await;

    // Cerca `HumanSay("ciao")` nell'inbox del servizio entro 1 secondo.
    // `SetServerTx` arriva PRIMA (sincrono, durante il setup della connessione);
    // il drain-loop lo ignora e aspetta solo l'evento atteso.
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        async {
            loop {
                match aichat_rx.recv().await {
                    Some(ServiceEvent::HumanSay(text)) => return text,
                    Some(_other) => continue, // SetServerTx, UiClosed ecc.
                    None => panic!("canale aichat chiuso prima di HumanSay"),
                }
            }
        },
    )
    .await;

    match result {
        Ok(text) => assert_eq!(text, "ciao", "HumanSay deve contenere il testo originale"),
        Err(_) => panic!("timeout: HumanSay non ricevuto entro 1 s — il wiring è mancante"),
    }
}

// ── Uscita pulita interattiva ("Q" per terminare) ─────────────────────────────

/// `ws::serve` deve tornare (`Ok`) entro un tempo ragionevole quando il
/// `CancellationToken` passato viene cancellato — è il meccanismo dietro il comando
/// "Q" per l'uscita pulita interattiva (vedi `main.rs`). Prima di questo, l'unico modo
/// di fermare il processo era Ctrl+C (interruzione brusca, niente cleanup ordinato).
#[tokio::test]
async fn serve_returns_when_shutdown_token_is_cancelled() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener); // Rilascia brevemente: ws::serve rifa il bind sull'indirizzo.

    let ai = Arc::new(StubAdapter);
    let tools = Arc::new(FakeToolClient::success("fake output\n"));
    let cfg_path = make_test_cfg_path();
    let content_cfg_path = make_test_content_cfg_path();
    let search = SearchContext {
        engine: Arc::new(SearchEngine::default()),
        cfg_path: Arc::new(cfg_path),
        content_cfg_path: Arc::new(content_cfg_path),
        provider: Arc::new(OsPathProvider),
    };
    let cwd_state = Arc::new(Mutex::new(String::new()));
    let plugin_host = Arc::new(tokio::sync::Mutex::new(
        PluginHost::start(
            vec![],
            |_p: &DiscoveredPlugin| -> std::io::Result<(Box<dyn PluginWriter>, Box<dyn PluginReader>)> {
                unreachable!("nessun plugin in questo test")
            },
            std::path::Path::new("."),
        )
        .await,
    ));
    let plugin_commands: Arc<Vec<(String, String)>> = Arc::new(vec![]);

    let shutdown = tokio_util::sync::CancellationToken::new();
    let shutdown_clone = shutdown.clone();
    let addr_str = addr.to_string();

    let handle = tokio::spawn(async move {
        ws::serve(
            &addr_str,
            Arc::new("tok".to_string()),
            ai,
            tools,
            cwd_state,
            search,
            plugin_host,
            plugin_commands,
            None,
            shutdown_clone,
            test_rt(),
            orchestrator::connections::Registry::shared(),
        )
        .await
    });

    // Lascia il tempo al listener di bindare prima di cancellare.
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    shutdown.cancel();

    let result = tokio::time::timeout(tokio::time::Duration::from_secs(2), handle)
        .await
        .expect("serve() non è tornato entro 2s dopo la cancellazione — il loop non risponde allo shutdown")
        .expect("il task di serve() è panicato");
    assert!(result.is_ok(), "serve() deve tornare Ok dopo uno shutdown pulito: {result:?}");
}

/// Task 11: `ClientMsg::TestMarketDataSource` deve echeggiare l'`id` ricevuto
/// e rispondere con `ServerMsg::MarketDataSourceTestResult` — copre la
/// wiring dell'arm (id round-trip, un classico punto di copia-incolla
/// sbagliata) senza dipendere da I/O reale (IB Gateway/rete): `test_rt()`
/// usa un `config_dir` fittizio (vedi sopra), quindi `pytools_dir` risolve
/// a una cartella che non esiste mai su disco — `PythonMcpToolClient::
/// resolve()` fallisce in modo sincrono e deterministico (nessuno spawn
/// Python), un attimo dopo il fallimento diventa `ok: false` — l'unico ramo
/// testabile senza I/O reale (il ramo "connessa" richiede TWS/Gateway vero,
/// fuori scope di un test automatico, vedi task-11-report.md). Il ping
/// successivo dimostra che il read-loop della connessione resta vivo dopo
/// l'arm (non spawnare il task avrebbe bloccato l'intera connessione fino
/// al timeout di 30s — vedi commento sull'arm in `ws.rs`).
#[tokio::test]
async fn test_market_data_source_echoes_id_and_reports_unreachable_when_venv_missing() {
    let url = spawn_server("market-data-test-token").await;
    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut sink, mut source) = ws.split();

    send(
        &mut sink,
        &ClientMsg::Hello { token: "market-data-test-token".to_string(), channel: None, role: protocol::Role::Ui, session_id: None, cwd: None, version: None, },
    )
    .await;
    let _info = recv(&mut source).await; // ServerInfo

    send(&mut sink, &ClientMsg::TestMarketDataSource { id: "t-market-1".to_string() }).await;

    let msg = recv(&mut source).await;
    match msg {
        ServerMsg::MarketDataSourceTestResult { id, ok, message } => {
            assert_eq!(id, "t-market-1", "l'id della richiesta deve tornare invariato nella risposta");
            assert!(!ok, "venv assente: il test deve riportare ok:false, non un finto successo");
            assert!(!message.is_empty(), "il messaggio non deve mai essere vuoto");
        }
        other => panic!("atteso ServerMsg::MarketDataSourceTestResult, ricevuto: {other:?}"),
    }

    // La connessione deve restare viva e reattiva dopo l'arm (prova che è
    // stato spawnato, non eseguito inline nel read-loop principale).
    send(&mut sink, &ClientMsg::Ping { ts: 999 }).await;
    let pong = recv(&mut source).await;
    assert_eq!(pong, ServerMsg::Pong { ts: 999 }, "il read-loop deve restare reattivo dopo TestMarketDataSource");
}

// ── Canale shell — scenari e2e (Task 10) ──────────────────────────────────────
//
// Questi cinque scenari sono ANCHE la copertura smoke che la revisione del
// Task 8 chiedeva: i cambi ad `handle_connection` per `role: "shell"` erano
// finora verificati solo dai test unitari di `shell_turn.rs` (che chiamano
// `run_shell_command` direttamente, bypassando `ws::serve`/l'handshake reale).
// Qui invece si passa da un vero handshake WS, come farà `lare-shell`.

/// Uno slash sconosciuto dalla shell ottiene solo `Done{0}` — niente `Cwd`
/// (la shell tiene la propria cwd, non è compito del server rimandarla) e
/// niente `Error` (uno slash "muto" non è un errore, spec §3).
#[tokio::test]
async fn shell_unknown_slash_gets_a_mute_done_and_no_cwd() {
    let url = spawn_server("tok-shell-1").await;
    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut sink, mut source) = ws.split();
    send(&mut sink, &shell_hello("tok-shell-1")).await;
    assert!(matches!(recv(&mut source).await, ServerMsg::ServerInfo { .. }));
    send(&mut sink, &command("c1", "/nonesiste")).await;
    // Il PRIMO messaggio dopo ServerInfo è Done (niente Cwd, niente Error).
    let next = recv(&mut source).await;
    assert!(matches!(next, ServerMsg::Done { exit_code: Some(0), .. }), "{next:?}");
}

/// `/ai` senza virgolette è un errore di sintassi (`RoutingError`) col
/// messaggio esatto di `shell_slash::AI_SYNTAX_ERROR`.
#[tokio::test]
async fn shell_ai_without_quotes_gets_syntax_error() {
    let url = spawn_server("tok-shell-2").await;
    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut sink, mut source) = ws.split();
    send(&mut sink, &shell_hello("tok-shell-2")).await;
    recv(&mut source).await;
    send(&mut sink, &command("c1", "/ai ciao")).await;
    let m = recv(&mut source).await;
    assert!(matches!(&m, ServerMsg::Error { code: ErrCode::RoutingError, message, .. } if message == orchestrator::shell_slash::AI_SYNTAX_ERROR), "{m:?}");
}

/// `/reset` risponde solo sulla shell (`RESET_MESSAGE` + `Done`, non
/// applicabile alla sessione della shell stessa); `/config` invece apre la
/// finestra sulla connessione `ui` registrata (`OpenUiLocal`).
#[tokio::test]
async fn shell_reset_and_open_ui_local_reach_the_ui_sink() {
    let url = spawn_server("tok-shell-3").await;
    let (ui_ws, _) = connect_async(&url).await.unwrap();
    let (mut ui_sink, mut ui_source) = ui_ws.split();
    send(&mut ui_sink, &ui_hello("tok-shell-3")).await;
    recv(&mut ui_source).await; // ServerInfo
    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut sink, mut source) = ws.split();
    send(&mut sink, &shell_hello("tok-shell-3")).await;
    recv(&mut source).await;
    send(&mut sink, &command("c1", "/reset")).await;
    let m = recv(&mut source).await;
    assert!(matches!(&m, ServerMsg::Chunk { content, .. } if content == orchestrator::shell_slash::RESET_MESSAGE), "{m:?}");
    assert!(matches!(recv(&mut source).await, ServerMsg::Done { .. }));
    send(&mut sink, &command("c2", "/config")).await;
    let m = recv_until(&mut ui_source, |m| matches!(m, ServerMsg::OpenUiLocal { .. })).await;
    assert!(matches!(&m, ServerMsg::OpenUiLocal { name } if name == "config"));
    assert!(matches!(recv(&mut source).await, ServerMsg::Done { exit_code: Some(0), .. }));
}

/// Il round-trip completo dello spec §4.2 con un'AI finta che propone `echo x`.
#[tokio::test]
async fn shell_ai_turn_round_trips_gate_exec_and_output_window() {
    let url = spawn_server_with("tok-shell-4", Arc::new(ToolCallingStubAdapter)).await;
    let (ui_ws, _) = connect_async(&url).await.unwrap();
    let (mut ui_sink, mut ui_source) = ui_ws.split();
    send(&mut ui_sink, &ui_hello("tok-shell-4")).await;
    recv(&mut ui_source).await;
    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut sink, mut source) = ws.split();
    send(&mut sink, &shell_hello("tok-shell-4")).await;
    recv(&mut source).await;

    send(&mut sink, &command("c1", "/ai \"stampa x\"")).await;
    // ui: la finestra si apre subito, col titolo derivato dal testo.
    let m = recv_until(&mut ui_source, |m| matches!(m, ServerMsg::OpenOutputWindow { .. })).await;
    assert!(matches!(&m, ServerMsg::OpenOutputWindow { window_id, title } if window_id == "c1" && title == "stampa x"), "{m:?}");
    // shell: gate → accetta.
    let gate = recv_until(&mut source, |m| matches!(m, ServerMsg::ToolConfirmRequest { .. })).await;
    let gate_id = match gate { ServerMsg::ToolConfirmRequest { id, commands } => { assert_eq!(commands, "$ echo x"); id } _ => unreachable!() };
    send(&mut sink, &ClientMsg::ToolConfirmResponse { id: gate_id, accept: true }).await;
    // shell: ExecInShell → rispondi come farebbe la host.
    let exec = recv_until(&mut source, |m| matches!(m, ServerMsg::ExecInShell { .. })).await;
    let exec_id = match exec {
        ServerMsg::ExecInShell { turn_id, exec_id, command, capture } => {
            assert_eq!((turn_id.as_str(), command.as_str(), capture), ("c1", "echo x", true));
            exec_id
        }
        _ => unreachable!(),
    };
    send(&mut sink, &ClientMsg::ExecResult { turn_id: "c1".into(), exec_id, exit_code: 0, output: "x\n".into(), cwd: "C:\\dopo".into() }).await;
    // shell: riga di conferma + Done (nessun Chunk di testo AI).
    let ack = recv(&mut source).await;
    assert!(matches!(&ack, ServerMsg::Chunk { content, .. } if content == "\u{2192} finestra \"stampa x\" aperta"), "{ack:?}");
    assert!(matches!(recv(&mut source).await, ServerMsg::Done { .. }));
    // ui: il contenuto arriva una volta, con l'output del comando.
    let content = recv_until(&mut ui_source, |m| matches!(m, ServerMsg::OutputWindowContent { .. })).await;
    assert!(matches!(&content, ServerMsg::OutputWindowContent { window_id, markdown } if window_id == "c1" && markdown.contains("output: x")), "{content:?}");
}

/// I4 (review finale): `CancelCommand` mentre un `ExecInShell` è pendente —
/// Ctrl+C sulla host non manda MAI un `ExecResult` (spec §4.4: la pipeline è
/// già stata fermata lì), quindi è `CancelCommand` (→ `abort_turn`) a dover
/// sbloccare `run_in_session` in attesa. `run_in_session` torna con
/// `exit_code: -1` e il testo `EXEC_ABORTED`; l'adapter finto lo tratta come
/// un output qualunque e il turno finisce regolarmente con `Done` — la
/// cancellazione durante un exec pendente NON deve bloccare o far
/// deragliare il turno.
#[tokio::test]
async fn shell_cancel_command_during_pending_exec_unblocks_the_turn() {
    let url = spawn_server_with("tok-shell-cancel-exec", Arc::new(ToolCallingStubAdapter)).await;
    let (ui_ws, _) = connect_async(&url).await.unwrap();
    let (mut ui_sink, mut ui_source) = ui_ws.split();
    send(&mut ui_sink, &ui_hello("tok-shell-cancel-exec")).await;
    recv(&mut ui_source).await;
    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut sink, mut source) = ws.split();
    send(&mut sink, &shell_hello("tok-shell-cancel-exec")).await;
    recv(&mut source).await;

    send(&mut sink, &command("c1", "/ai \"stampa x\"")).await;
    // ui: la finestra si apre subito, come nel round-trip normale.
    let m = recv_until(&mut ui_source, |m| matches!(m, ServerMsg::OpenOutputWindow { .. })).await;
    assert!(matches!(&m, ServerMsg::OpenOutputWindow { window_id, title } if window_id == "c1" && title == "stampa x"), "{m:?}");
    // shell: gate → accetta.
    let gate = recv_until(&mut source, |m| matches!(m, ServerMsg::ToolConfirmRequest { .. })).await;
    let gate_id = match gate { ServerMsg::ToolConfirmRequest { id, commands } => { assert_eq!(commands, "$ echo x"); id } _ => unreachable!() };
    send(&mut sink, &ClientMsg::ToolConfirmResponse { id: gate_id, accept: true }).await;
    // shell: ExecInShell pendente — invece di rispondere con ExecResult (come
    // farebbe la host per un comando che termina normalmente), la shell manda
    // CancelCommand{id: "c1"} (Ctrl+C): nessun ExecResult arriverà mai.
    recv_until(&mut source, |m| matches!(m, ServerMsg::ExecInShell { .. })).await;
    send(&mut sink, &ClientMsg::CancelCommand { id: "c1".to_string() }).await;

    // shell: riga di conferma + Done — il turno finisce regolarmente
    // nonostante l'exec non sia mai stato risolto da un ExecResult.
    let ack = recv(&mut source).await;
    assert!(matches!(&ack, ServerMsg::Chunk { content, .. } if content == "\u{2192} finestra \"stampa x\" aperta"), "{ack:?}");
    assert!(matches!(recv(&mut source).await, ServerMsg::Done { .. }));
    // ui: il contenuto della finestra riporta l'esecuzione interrotta
    // (EXEC_ABORTED), non l'output normale di "echo x".
    let content = recv_until(&mut ui_source, |m| matches!(m, ServerMsg::OutputWindowContent { .. })).await;
    match &content {
        ServerMsg::OutputWindowContent { window_id, markdown } => {
            assert_eq!(window_id, "c1");
            assert!(
                markdown.contains(orchestrator::shell_session::EXEC_ABORTED),
                "atteso EXEC_ABORTED nel markdown, got: {markdown}"
            );
        }
        other => panic!("atteso OutputWindowContent, got {other:?}"),
    }
}

/// `/ping` dalla shell: sonda `ui.exe` via `UiPing`/`UiPong` e riporta anche
/// il plugin `ping` mancante (nessun plugin registrato in `spawn_server`).
#[tokio::test]
async fn shell_ping_reports_ui_and_missing_plugin() {
    let url = spawn_server("tok-shell-5").await;
    let (ui_ws, _) = connect_async(&url).await.unwrap();
    let (mut ui_sink, mut ui_source) = ui_ws.split();
    send(&mut ui_sink, &ui_hello("tok-shell-5")).await;
    recv(&mut ui_source).await;
    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut sink, mut source) = ws.split();
    send(&mut sink, &shell_hello("tok-shell-5")).await;
    recv(&mut source).await;
    send(&mut sink, &command("c1", "/ping")).await;
    let ping = recv_until(&mut ui_source, |m| matches!(m, ServerMsg::UiPing { .. })).await;
    if let ServerMsg::UiPing { id } = ping {
        send(&mut ui_sink, &ClientMsg::UiPong { id, version: "9.9.9".into() }).await;
    }
    let content = recv_until(&mut ui_source, |m| matches!(m, ServerMsg::OutputWindowContent { .. })).await;
    let md = match content { ServerMsg::OutputWindowContent { markdown, .. } => markdown, _ => unreachable!() };
    assert!(md.contains("| lare-shell | 2.0.0 | ok | sessione s1"), "{md}");
    assert!(md.contains("| ui.exe | 9.9.9 | ok |"), "{md}");
    assert!(md.contains("| plugin-ping | -- | non trovato |"), "{md}");
    assert!(matches!(recv_until(&mut source, |m| matches!(m, ServerMsg::Done { .. })).await, ServerMsg::Done { exit_code: Some(0), .. }));
}
