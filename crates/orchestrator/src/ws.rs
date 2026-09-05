//! # ws — WebSocket transport layer (Contratto A)
//!
//! Implements the server-side WebSocket listener for Lare Terminal.
//!
//! ## Responsibilities (SRP)
//! This module has one job: **adapt the WebSocket protocol to and from the
//! transport-agnostic core**.
//! - Accept TCP connections on `127.0.0.1:<ws_port>` (`startup.json`, default 7331).
//! - Perform the handshake (`Hello{token}` → validate → send `ServerInfo`).
//! - Dispatch each `Command` to `core::handle_command`.
//! - Respond to `Ping` with `Pong`.
//! - Close the connection on invalid token or deserialization error.
//!
//! ## Security (ADR-007)
//! - Listening address is hard-coded to `127.0.0.1` (localhost only).
//! - Token is validated on every new connection before accepting any command.
//! - Wrong/missing token: connection closed immediately, no `ServerInfo` sent.
//!
//! ## SRP split
//! This module does NOT contain routing, AI, or tool logic.  All of that
//! lives in `core`, `router`, `ai_adapter`, and `tool_client`.

use std::{collections::HashMap, sync::Arc};

use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use protocol::{ClientMsg, ServerMsg};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::unbounded_channel;
use tokio::sync::Mutex;
use tokio_tungstenite::{accept_async, tungstenite::Message};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::ai_adapter::AiAdapter;
use crate::aichat::service::ServiceEvent;
use crate::plugins::host::PluginHost;
use crate::router::plugin_command;
use crate::search::{PauseGate, SearchContext};
use crate::tool_client::ToolClient;

/// Orchestrator version sent in `ServerInfo`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Start the WebSocket server.
///
/// Accepts connections forever (until the process exits or a hard error).
/// Each connection is handled in its own Tokio task.
///
/// # Arguments
/// * `token`          — The auth token clients must present in their `Hello` message.
/// * `ai`             — Shared AI adapter (Arc so each connection can clone a ref).
/// * `tools`          — Shared tool client (should be wrapped in `CwdTrackingToolClient`).
/// * `cwd_state`      — Shared current working directory of the persistent shell (Task 3).
/// * `search`         — Search context (engine + path config).
/// * `plugin_host`    — Shared plugin host (Task 5): routes plugin events + manages writers.
/// * `plugin_commands`— List of `(slash_command, plugin_id)` pairs for command dispatch.
///
/// # Errors
/// Returns if the TCP listener cannot be bound (port in use, permission denied, …).
// plugin_host + plugin_commands push us past the clippy default of 7 args.
// Bundling them in a struct would be a bigger refactor outside Task 5 scope.
#[allow(clippy::too_many_arguments)]
pub async fn serve(
    addr: &str,
    token: Arc<String>,
    ai: Arc<dyn AiAdapter>,
    tools: Arc<dyn ToolClient>,
    cwd_state: Arc<Mutex<String>>,
    search: SearchContext,
    plugin_host: Arc<Mutex<PluginHost>>,
    plugin_commands: Arc<Vec<(String, String)>>,
    // Handle opzionale al servizio AI Chat. `None` = canale disabilitato.
    // Quando `Some`, per ogni connessione si inoltrano gli eventi di UI
    // (SetServerTx, UiClosed, HumanSay, JoinDecision, AdmissionVoteUi,
    // RequestAdmissionUi — Task 8: `Consent` non esiste più, sostituito
    // dall'ammissione a voto) all'inbox del servizio.
    aichat: Option<tokio::sync::mpsc::UnboundedSender<ServiceEvent>>,
    // Segnale di uscita pulita (comando "Q" interattivo, vedi `main.rs`): quando
    // cancellato, il loop di accept smette di accettare nuove connessioni e `serve`
    // torna `Ok(())`. Le connessioni già in corso non vengono chiuse esplicitamente
    // qui: terminano quando il processo esce (i socket TCP si chiudono con l'exit).
    shutdown: CancellationToken,
    // `RuntimeConfig` (2.0, Task 4, D6): risolto una volta in `main()`,
    // condiviso da ogni connessione per `resolve_channel_tools`
    // (canali nmap/python) e per il test manuale "fonte dati mercato" — mai
    // ri-derivato qui.
    rt: Arc<crate::runtime_config::RuntimeConfig>,
) -> Result<()> {
    let listener = TcpListener::bind(addr).await?;
    info!("Lare Terminal orchestrator listening on ws://{addr}");

    loop {
        tokio::select! {
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((stream, peer_addr)) => {
                        info!("new connection from {peer_addr}");
                        let token = Arc::clone(&token);
                        let ai = Arc::clone(&ai);
                        let tools = Arc::clone(&tools);
                        let cwd_state = Arc::clone(&cwd_state);
                        let search = search.clone();
                        let plugin_host = Arc::clone(&plugin_host);
                        let plugin_commands = Arc::clone(&plugin_commands);
                        // UnboundedSender è Clone, quindi Option<UnboundedSender<_>> lo è anch'esso.
                        let aichat = aichat.clone();
                        let rt = Arc::clone(&rt);
                        tokio::spawn(async move {
                            if let Err(e) =
                                handle_connection(stream, token, ai, tools, cwd_state, search, plugin_host, plugin_commands, aichat, rt).await
                            {
                                error!("connection error from {peer_addr}: {e}");
                            }
                        });
                    }
                    Err(e) => {
                        error!("accept error: {e}");
                    }
                }
            }
            _ = shutdown.cancelled() => {
                info!("uscita pulita richiesta — chiudo il listener WS");
                break;
            }
        }
    }
    Ok(())
}

/// Handle a single WebSocket connection through its full lifecycle.
///
/// # cwd tracking
/// `cwd_state` is the shared cwd of the persistent shell.  On connection we
/// emit `ServerMsg::Cwd` with the initial value.  After every non-`/find`
/// command we read `cwd_state`; if it changed relative to the last value
/// emitted on THIS connection, we emit a new `ServerMsg::Cwd`.
///
/// # Concorrenza (stop button)
/// Il read-loop non esegue più i comandi OS/NL in modo inline (bloccante):
/// ogni comando non-find viene spawnato in un task separato, così il loop
/// può ricevere immediatamente `CancelCommand` mentre il comando è ancora in corso.
/// La `history` è avvolta in `Arc<Mutex<...>>` per poter essere condivisa tra
/// il loop principale e i task spawnati.
// Same justification as `serve` above.
#[allow(clippy::too_many_arguments)]
async fn handle_connection(
    stream: TcpStream,
    token: Arc<String>,
    ai: Arc<dyn AiAdapter>,
    tools: Arc<dyn ToolClient>,
    cwd_state: Arc<Mutex<String>>,
    search: SearchContext,
    plugin_host: Arc<Mutex<PluginHost>>,
    plugin_commands: Arc<Vec<(String, String)>>,
    // Handle opzionale al servizio AI Chat (see `serve` above for documentation).
    aichat: Option<tokio::sync::mpsc::UnboundedSender<ServiceEvent>>,
    // `RuntimeConfig` (see `serve` above for documentation).
    rt: Arc<crate::runtime_config::RuntimeConfig>,
) -> Result<()> {
    let ws_stream = accept_async(stream).await?;
    let (mut sink, mut source) = ws_stream.split();

    // ── Handshake: first message MUST be Hello{token} ────────────────────────
    // Il handshake legge il primo messaggio DIRETTAMENTE da `source`, PRIMA
    // di spostarlo nel reader task (sotto). L'ordine è vincolante.
    let first = match source.next().await {
        Some(Ok(msg)) => msg,
        Some(Err(e)) => {
            warn!("handshake read error: {e}");
            return Ok(());
        }
        None => {
            warn!("connection closed before handshake");
            return Ok(());
        }
    };

    // Parse the first message.
    let (client_token, requested_channel) = match parse_hello(&first) {
        Some(pair) => pair,
        None => {
            warn!("invalid handshake message; closing connection");
            let _ = sink.close().await;
            return Ok(());
        }
    };

    // Validate the token.
    if client_token != token.as_str() {
        warn!("invalid token; closing connection");
        let _ = sink.close().await;
        return Ok(());
    }

    // ── Canale esterno (opzionale, Docs/superpowers/specs/2026-07-16-external-tool-channel-design.md) ──
    // `channel: None` (comportamento di sempre) usa `tools` così com'è — zero
    // regressione per cursore/Telegram. `channel: Some(id)` sceglie il
    // ToolClient/format_invocation del canale al posto del default condiviso;
    // canale sconosciuto o costruzione fallita → ServerMsg::Error esplicito,
    // connessione chiusa (mai un panic). Shadowing locale: nessuna modifica
    // alla firma di `handle_connection` — `tools_clone` più sotto (invariato)
    // clona automaticamente questo binding, non più il parametro condiviso.
    let (tools, format_invocation, system_prompt_override) = match crate::external_channel::resolve_channel_tools(
        requested_channel.as_deref(),
        crate::external_channel::EXTERNAL_TOOL_CHANNELS,
        &tools,
        &rt,
    ) {
        Ok(triple) => triple,
        Err(message) => {
            warn!("external channel resolution failed: {message}");
            let err = ServerMsg::Error {
                id: String::new(),
                code: protocol::ErrCode::RoutingError,
                message,
            };
            let _ = send_msg(&mut sink, &err).await;
            let _ = sink.close().await;
            return Ok(());
        }
    };

    // Send ServerInfo.
    let info = ServerMsg::ServerInfo {
        version: VERSION.to_string(),
        ai_provider: ai.provider(),
        capabilities: vec!["os_command".to_string()],
    };
    send_msg(&mut sink, &info).await?;

    // ── Emit initial cwd (Task 3 / Step 5) ────────────────────────────────────
    // Read the shared cwd_state and emit ServerMsg::Cwd immediately after
    // ServerInfo so the UI can display the current directory from the start.
    let initial_cwd = cwd_state.lock().await.clone();
    if !initial_cwd.is_empty() {
        send_msg(&mut sink, &ServerMsg::Cwd { path: initial_cwd.clone() }).await?;
    }
    let _last_cwd = initial_cwd;

    // ── Canale d'uscita persistente ────────────────────────────────────────────
    // Un solo task scrittore possiede il `sink`; tutto ciò che va al client passa da
    // `out_tx`. Così il read-loop resta libero di leggere il frame successivo (es.
    // CancelSearch, CancelCommand) MENTRE un comando o una ricerca spawnata invia
    // risultati in streaming sul sink.
    // (L'handshake ServerInfo e il Cwd iniziale sono già stati scritti DIRETTAMENTE
    // sul sink, sopra.)
    let (out_tx, mut out_rx) = unbounded_channel::<ServerMsg>();

    // Connette il plugin host al canale WS di questa connessione (Task 5) —
    // SOLO se questa è la connessione del cursore principale, non di un canale
    // esterno (`connection_owns_plugin_sink`, vedi doc-comment: PluginHost è
    // condiviso da ogni connessione, una connessione di canale non deve
    // rubare il sink alla finestra principale).
    // I pump task dei plugin lazy attivati DOPO questa riga riceveranno una copia
    // di `out_tx` e potranno forwardare ServerMsg al client WS.
    // I pump degli eager (avviati prima della connessione) avevano server_tx = None
    // e continuano a scartare silenziosamente i messaggi (comportamento by-design).
    if connection_owns_plugin_sink(requested_channel.as_deref()) {
        plugin_host.lock().await.set_server_tx(out_tx.clone());
    }

    // Notifica il servizio AI Chat che questa UI è ora connessa, passandogli
    // una copia del canale d'uscita WS. Da questo momento il servizio può
    // pushare ServerMsg direttamente al client.
    if let Some(h) = &aichat {
        let _ = h.send(ServiceEvent::SetServerTx(out_tx.clone()));
    }

    let writer = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            if send_msg(&mut sink, &msg).await.is_err() {
                break; // client disconnesso → fine scrittore
            }
        }
    });

    // ── Reader task: separa la lettura WS dal loop principale ─────────────────
    // `source` viene MOSSO qui: tutto il parsing dei frame gira nel reader task.
    // Il loop principale legge da `in_rx` (canale in-memory, senza I/O).
    let (in_tx, mut in_rx) = unbounded_channel::<ClientMsg>();
    let reader = tokio::spawn(async move {
        while let Some(msg_result) = source.next().await {
            let raw = match msg_result {
                Ok(m) => m,
                Err(e) => { warn!("ws read error: {e}"); break; }
            };
            if raw.is_close() { break; }
            if raw.is_ping() { continue; }
            if raw.is_binary() || !raw.is_text() {
                warn!("unexpected non-text frame; ignoring");
                continue;
            }
            let text = match raw.to_text() { Ok(t) => t, Err(_) => continue };
            match serde_json::from_str::<ClientMsg>(text) {
                Ok(msg) => { if in_tx.send(msg).is_err() { break; } }
                Err(e) => { warn!("JSON parse error: {e}"); continue; }
            }
        }
    });

    // Storia conversazionale per-connessione (Slice 2 — stateful).
    // Avvolta in Arc<Mutex<...>> per essere condivisa con i task spawnati dei comandi.
    let history = Arc::new(Mutex::new(crate::messages_client::ConversationHistory::new()));

    // Registro delle ricerche in corso (id → (token, gate)).
    let mut searches: HashMap<String, (CancellationToken, Arc<PauseGate>)> = HashMap::new();

    // Registro dei comandi in corso (id → cancel_token).
    // Usato per CancelCommand: il token viene cancellato dal loop principale,
    // e il task del comando controlla il token all'inizio di ogni iterazione AI.
    let mut commands: HashMap<String, CancellationToken> = HashMap::new();

    // Registro delle conferme pendenti (gate locale per-tool, Docs/superpowers/
    // specs/2026-07-15-local-tool-confirm-gate-design.md). Cloneabile: condiviso
    // con ogni `LocalUiConfirmer` costruito nei task spawnati dei singoli comandi.
    let pending_confirms = crate::local_confirm::PendingConfirms::new();

    // ── Main message loop ─────────────────────────────────────────────────────
    // Legge da `in_rx` (già deserializzato dal reader task).
    while let Some(client_msg) = in_rx.recv().await {
        match client_msg {
            ClientMsg::Hello { .. } => {
                // Duplicate handshake: ignore (already authenticated).
                warn!("received duplicate Hello after handshake; ignoring");
            }

            ClientMsg::Ping { ts } => {
                if out_tx.send(ServerMsg::Pong { ts }).is_err() {
                    break; // scrittore finito → client andato
                }
            }

            // ── Task 11: test manuale "fonte dati mercato" da /config ────────────
            // Spawnato (non `.await` inline nel loop principale): il test di rete
            // verso la fonte dati mercato attiva (`test_market_data_source_now`,
            // helper condiviso col check informativo di avvio in `main.rs`) può
            // richiedere fino al timeout di `PythonMcpToolClient` (30s) se
            // TWS/Gateway non risponde — senza spawn, questa connessione WS
            // resterebbe bloccata (niente Ping/Pong, niente CancelCommand) per
            // tutto quel tempo, stesso motivo per cui i comandi OS/NL qui sotto
            // sono spawnati invece di eseguiti inline.
            ClientMsg::TestMarketDataSource { id } => {
                let out = out_tx.clone();
                let rt = Arc::clone(&rt);
                tokio::spawn(async move {
                    let (ok, message) = test_market_data_source_now(&rt).await;
                    let _ = out.send(ServerMsg::MarketDataSourceTestResult { id, ok, message });
                });
            }

            ClientMsg::CancelSearch { id } => {
                if let Some((tok, _gate)) = searches.remove(&id) {
                    tok.cancel();
                }
            }

            ClientMsg::PauseSearch { id } => {
                if let Some((_tok, gate)) = searches.get(&id) {
                    gate.pause();
                }
            }

            ClientMsg::ResumeSearch { id } => {
                if let Some((_tok, gate)) = searches.get(&id) {
                    gate.resume();
                }
            }

            ClientMsg::CancelCommand { id } => {
                // Annulla il comando AI/OS identificato da `id` (se ancora in corso).
                // No-op se il comando è già terminato o l'id è sconosciuto.
                if let Some(cmd_token) = commands.remove(&id) {
                    cmd_token.cancel();
                }
            }

            // ── Plugin window events (Contract A, Slice 1) ───────────────────────
            // Task 5: ora pienamente implementati.

            ClientMsg::PluginUiEvent { window_id, element_id, value } => {
                // Inoltra l'evento UI (clic, input) al plugin che possiede la finestra.
                // `route_ui_event` invia `HostToPlugin::UiEvent` al writer del plugin.
                plugin_host.lock().await.route_ui_event(window_id, &element_id, value).await;
            }

            ClientMsg::PluginWindowClosed { window_id } => {
                // Rimuove la registrazione della finestra: cleanup dopo che l'UI la chiude.
                plugin_host.lock().await.forget_window(window_id).await;
            }

            // ── AI Chat (Slice 1a-ui-A): dispatch verso il servizio attore ─────
            // I tre arm sotto inoltrano i messaggi dell'UI al servizio AI Chat
            // tramite il canale `aichat` (Option<UnboundedSender<ServiceEvent>>).
            // Se `aichat` è `None` (canale disabilitato), i messaggi sono ignorati
            // silenziosamente: la UI non riceve un errore, ma il canale non funziona.
            ClientMsg::AiChatSend { text } => {
                // L'utente ha digitato un messaggio nella chat AI.
                // Viene inoltrato come `HumanSay` al servizio che coordina l'AI.
                if let Some(h) = &aichat {
                    let _ = h.send(ServiceEvent::HumanSay(text));
                }
            }

            // Task 8 (ammissione alla stanza): il vecchio consenso pairwise/asimmetrico
            // (`ServiceEvent::Consent`) è stato rimosso da `service.rs` — sostituito dal
            // voto di ammissione coordinato dal server (gate 1 + gate 2, vedi gli arm
            // sotto). Questo messaggio (`AiChatJoinConsent`) resta nel `ClientMsg` per
            // additività del protocollo (Contratto A), ma un client aggiornato non lo
            // manda più: no-op, non un errore, mai raggiunto in pratica.
            ClientMsg::AiChatJoinConsent { .. } => {}

            // ── Task 8: dispatch verso il servizio AI Chat per l'ammissione a voto ──
            // I tre arm sotto (in precedenza scaffolding no-op) inoltrano le risposte
            // dell'umano ai due gate del nuovo flusso di ammissione (Task 4-6 in
            // `service.rs`): gate 1 (il nuovo arrivato accetta/rifiuta di entrare),
            // gate 2 (un presente vota sull'ingresso di un candidato), e il pulsante
            // "chiedi di entrare" dopo un rifiuto.
            ClientMsg::AiChatJoinDecision { accept } => {
                if let Some(h) = &aichat {
                    let _ = h.send(ServiceEvent::JoinDecision { accept });
                }
            }

            ClientMsg::AiChatAdmissionVote { candidate, accept } => {
                if let Some(h) = &aichat {
                    let _ = h.send(ServiceEvent::AdmissionVoteUi { candidate_label: candidate, accept });
                }
            }

            ClientMsg::AiChatRequestAdmission {} => {
                if let Some(h) = &aichat {
                    let _ = h.send(ServiceEvent::RequestAdmissionUi);
                }
            }

            ClientMsg::AiChatClosed {} => {
                // Bug reale trovato dal vivo (2026-07-29, log Rust+JS incrociati):
                // questo arm mandava `ServiceEvent::UiClosed`, che azzera
                // `self.server_tx` — il canale WS CONDIVISO usato da OGNI
                // `Effect::ToUi` (messaggi, roster, busta di attività non vista,
                // gate 1/2, tutto). Chiudere SOLO la finestra chat (una webview
                // secondaria) veniva trattato come se l'INTERA UI si fosse
                // disconnessa: per tutto il tempo in cui la chat restava chiusa,
                // ogni evento AI Chat andava silenziosamente perso (mai un errore,
                // mai un log visibile all'utente) — nessun messaggio, nessuna
                // busta, finché la finestra non veniva riaperta (che ri-registra
                // il sink via `AiChatOpen` → `SetServerTx`).
                //
                // Il vero teardown della connessione WS (disconnessione reale,
                // sotto) resta l'UNICO punto legittimo che deve azzerare
                // `server_tx` — quello sì rappresenta l'intera UI che sparisce.
                // La chiusura della finestra chat non deve più toccarlo: lo stato
                // "la finestra chat è aperta?" lo tiene già, correttamente, il
                // frontend (`aiChatGate` in `aichat-push.js`) — al backend basta
                // sapere se la connessione WS è viva, non quale sotto-finestra è
                // aperta in un dato momento. Nessun'altra azione necessaria qui:
                // `AiChatClosed` non ha altro scopo oggi.
            }

            ClientMsg::AiChatOpen {} => {
                // La finestra-chat è stata (ri)aperta: ri-registra il sink WS nel servizio.
                // Questo ri-emette l'ultimo roster noto al nuovo webview (che parte senza
                // "presenti") e ripristina il path ToUi per i messaggi in arrivo.
                // La connessione WS sottostante è la stessa; solo il webview Tauri è nuovo.
                if let Some(h) = &aichat {
                    let _ = h.send(ServiceEvent::SetServerTx(out_tx.clone()));
                }
            }

            ClientMsg::Command {
                id,
                input,
                input_mode: _,
                command_type,
                cwd,
                web_search,
            } => {
                // ── Plugin command dispatch (Task 5) ─────────────────────────────
                // Controlla se l'input corrisponde a un comando slash di plugin
                // (es. `/counter`, `/calc`) registrato in plugin_commands.
                // Deve avvenire PRIMA di parse_find e del normale routing OS/NL/slash.
                if let Some(plugin_id) = plugin_command(&input, &plugin_commands) {
                    let pid = plugin_id.to_string();
                    let ph = Arc::clone(&plugin_host);
                    let out = out_tx.clone();
                    // il closure sotto deve essere 'static (vedi commento lì).
                    let rt = Arc::clone(&rt);
                    // id viene mosso nel task: dopo il `continue` non serve più.
                    let id_for_task = id;
                    tokio::spawn(async move {
                        // La factory crea le due metà reali del transport per il plugin lazy.
                        // Definita qui (non fuori dal task) perché `spawn_plugin` ritorna tipi
                        // non-Clone e il closure deve essere 'static.
                        let mut make_fn = |p: &crate::plugins::discovery::DiscoveredPlugin| {
                            crate::plugins::transport::spawn_plugin(&p.bin_path, &rt.config_dir)
                                .map(|(w, r)| {
                                    (
                                        Box::new(w) as Box<dyn crate::plugins::transport::PluginWriter>,
                                        Box::new(r) as Box<dyn crate::plugins::transport::PluginReader>,
                                    )
                                })
                        };
                        match ph.lock().await.activate(&pid, &mut make_fn).await {
                            Some(_wid) => {
                                // Attivazione riuscita: la finestra verrà aperta dal pump task
                                // quando il plugin invierà ShowWindow.
                                let _ = out.send(ServerMsg::Done { id: id_for_task, exit_code: Some(0) });
                            }
                            None => {
                                let _ = out.send(ServerMsg::Error {
                                    id: id_for_task.clone(),
                                    code: protocol::ErrCode::RoutingError,
                                    message: format!("plugin '{pid}' non disponibile"),
                                });
                                let _ = out.send(ServerMsg::Done { id: id_for_task, exit_code: Some(1) });
                            }
                        }
                    });
                    continue; // Salto il resto del Command arm: id e' già mosso nel task.
                }

                if let Some(query) = parse_find(&input) {
                    // `/find <query>`: ricerca live, SPAWNATA (non blocca il loop) e
                    // cancellabile via CancelSearch.
                    if query.is_empty() {
                        let _ = out_tx.send(ServerMsg::Error {
                            id: id.clone(),
                            code: protocol::ErrCode::RoutingError,
                            message: "usa /find <query>".to_string(),
                        });
                        let _ = out_tx.send(ServerMsg::Done {
                            id,
                            exit_code: Some(1),
                        });
                        continue;
                    }
                    if let Err(message) = crate::search::query::parse_find_input(&query) {
                        // `folder:` con un valore non riconosciuto: errore esplicito,
                        // PRIMA di aprire la finestra di ricerca (nessun `SearchOpen`
                        // mai emesso per una direttiva invalida) — stesso pattern del
                        // controllo `query.is_empty()` appena sopra.
                        let _ = out_tx.send(ServerMsg::Error {
                            id: id.clone(),
                            code: protocol::ErrCode::RoutingError,
                            message,
                        });
                        let _ = out_tx.send(ServerMsg::Done {
                            id,
                            exit_code: Some(1),
                        });
                        continue;
                    }
                    let sid = format!("{:032x}", rand::random::<u128>());
                    let find_token = CancellationToken::new();
                    let gate = PauseGate::new();
                    searches.insert(sid.clone(), (find_token.clone(), Arc::clone(&gate)));

                    let cwd_path = if let Some(c) = cwd.as_deref() {
                        std::path::PathBuf::from(c)
                    } else {
                        let tracked = cwd_state.lock().await.clone();
                        if tracked.is_empty() {
                            std::env::current_dir().unwrap_or_default()
                        } else {
                            std::path::PathBuf::from(tracked)
                        }
                    };

                    let search = search.clone();
                    let out = out_tx.clone();
                    tokio::spawn(async move {
                        crate::search::launch(&search, &sid, &query, &cwd_path, &out, find_token, gate).await;
                    });

                    let _ = out_tx.send(ServerMsg::Done {
                        id,
                        exit_code: Some(0),
                    });
                } else {
                    // Comando normale (OS/NL/slash): SPAWNATO in un task separato
                    // così il loop può ricevere CancelCommand immediatamente.
                    //
                    // `Some(&local_confirmer)` — la UI locale ha un gate PER-TOOL
                    // (`LocalUiConfirmer::should_gate`, local_confirm.rs): i tool in
                    // `SENSITIVE_TOOLS` (i cinque tool del canale nmap) vengono
                    // gateizzati davvero, tutti gli altri restano autonomi come col
                    // vecchio `confirmer: None` letterale.
                    // We prefer Command.cwd (client-supplied) for backwards compatibility;
                    // falling back to the tracked cwd state.
                    let effective_cwd = if let Some(c) = cwd.as_deref().filter(|s| !s.is_empty()) {
                        c.to_string()
                    } else {
                        cwd_state.lock().await.clone()
                    };

                    // Crea il cancel token per questo comando e registralo.
                    let cancel = CancellationToken::new();
                    commands.insert(id.clone(), cancel.clone());

                    // Clona gli Arc necessari per il task spawnato.
                    let hist = Arc::clone(&history);
                    let out = out_tx.clone();
                    let ai_clone = Arc::clone(&ai);
                    let tools_clone = Arc::clone(&tools);
                    let format_invocation_clone = format_invocation;
                    let system_prompt_override_clone = system_prompt_override;
                    let cwd_clone = Arc::clone(&cwd_state);
                    let out_cwd = out_tx.clone();
                    // Gate locale per-tool: LocalUiConfirmer::should_gate è true solo per i
                    // tool in SENSITIVE_TOOLS (local_confirm.rs) — oggi i cinque tool nmap;
                    // ogni altro tool (run_in_session, open_target, ...) resta autonomo.
                    let pending_confirms_clone = pending_confirms.clone();

                    tokio::spawn(async move {
                        let local_confirmer = crate::local_confirm::LocalUiConfirmer::new(
                            out.clone(),
                            pending_confirms_clone,
                            std::time::Duration::from_secs(180),
                            // Id del comando corrente: `confirm_routine_save` lo
                            // usa per il `ServerMsg::Chunk` "anteprima aperta" —
                            // deve coincidere con l'`id` che `respond()` usa per
                            // ogni altro Chunk dello stesso turno (review finale
                            // save_routine, finding I3).
                            id.clone(),
                        );
                        let mut guard = hist.lock().await;
                        crate::core::handle_command(
                            &id,
                            &input,
                            command_type,
                            if effective_cwd.is_empty() { None } else { Some(&effective_cwd) },
                            &mut *guard,
                            ai_clone.as_ref(),
                            tools_clone.as_ref(),
                            format_invocation_clone,
                            system_prompt_override_clone,
                            web_search,
                            Some(&local_confirmer),
                            Some(cancel),
                            out,
                        )
                        .await;
                        drop(guard);

                        // ── Emit Cwd if changed (Task 3 / Step 5) ─────────────
                        let current_cwd = cwd_clone.lock().await.clone();
                        if !current_cwd.is_empty() {
                            let _ = out_cwd.send(ServerMsg::Cwd { path: current_cwd });
                        }
                    });
                    // Il loop NON si blocca: il reader è libero di ricevere CancelCommand.
                }
            }

            // ── Task 10: Library "Share with" — dispatch AI Chat share messages ──
            // Dispatches Library share requests and consent decisions to the AI Chat service.
            ClientMsg::ShareDocument { rel_path, doc_name, size_bytes, target } => {
                if let Some(h) = &aichat {
                    let _ = h.send(ServiceEvent::ShareDocumentRequested {
                        rel_path,
                        doc_name,
                        size_bytes,
                        target,
                    });
                }
            }

            ClientMsg::ShareConsent { share_id, accept } => {
                if let Some(h) = &aichat {
                    let _ = h.send(ServiceEvent::ShareConsentUi { share_id, accept });
                }
            }

            ClientMsg::ShareWritten { share_id } => {
                if let Some(h) = &aichat {
                    let _ = h.send(ServiceEvent::ShareWrittenUi { share_id });
                }
            }

            ClientMsg::ShareContent { share_id, title, content } => {
                if let Some(h) = &aichat {
                    let _ = h.send(ServiceEvent::ShareContentUi { share_id, title, content });
                }
            }

            ClientMsg::ShareContentFailed { share_id, reason } => {
                if let Some(h) = &aichat {
                    let _ = h.send(ServiceEvent::ShareContentFailedUi { share_id, reason });
                }
            }

            // Risposta dell'utente a un `ServerMsg::ToolConfirmRequest` (gate locale
            // per-tool). Un id sconosciuto (già risolto, o mai esistito perché nessun
            // tool sensibile è ancora stato proposto) è un no-op silenzioso.
            ClientMsg::ToolConfirmResponse { id, accept } => {
                pending_confirms.resolve(&id, accept).await;
            }

            // Library "Blocco note" — dispatch verso il servizio AI Chat (stesso
            // canale/attore usato da Share, la rete peer è condivisa).
            ClientMsg::NoteCreate { title, text } => {
                if let Some(h) = &aichat {
                    let _ = h.send(ServiceEvent::NoteCreateRequested { title, text });
                }
            }

            ClientMsg::NoteEdit { id, text } => {
                if let Some(h) = &aichat {
                    let _ = h.send(ServiceEvent::NoteEditRequested { id, text });
                }
            }

            ClientMsg::NoteEditTitle { id, title } => {
                if let Some(h) = &aichat {
                    let _ = h.send(ServiceEvent::NoteEditTitleRequested { id, title });
                }
            }

            ClientMsg::NoteDelete { id } => {
                if let Some(h) = &aichat {
                    let _ = h.send(ServiceEvent::NoteDeleteRequested { id });
                }
            }
        }
    }

    // Chiusura: annulla tutte le operazioni in corso.
    for (_, (tok, _gate)) in searches.drain() {
        tok.cancel();
    }
    for (_, cmd_token) in commands.drain() {
        cmd_token.cancel();
    }

    // Chiude il ToolClient di questa connessione (default no-op per
    // cursore/Telegram — mcp-server è un singleton gestito altrove, non
    // per-connessione). Per un canale esterno come `/nmap`, questo uccide
    // il processo sidecar (`mcp-nmap.exe`) se ancora vivo: senza questa
    // chiamata, chiudere la finestra dopo uno scan riuscito lasciava il
    // processo orfano per sempre, esattamente come un timeout non gestito
    // (fix precedente, orchestrator 0.40.30) — stesso meccanismo, trigger
    // diverso (chiusura normale della connessione anziché timeout).
    tools.shutdown().await;

    // Segnala al servizio AI Chat che questa connessione WS è terminata —
    // l'INTERA UI (ui.exe) è sparita, non solo una sua sotto-finestra.
    // Fix 2026-07-29: questo è ora l'UNICO punto che manda `UiClosed` — vedi
    // il commento nell'arm `ClientMsg::AiChatClosed` sopra per il perché
    // quell'arm NON lo manda più.
    if let Some(h) = &aichat {
        let _ = h.send(ServiceEvent::UiClosed);
    }

    drop(out_tx);
    let _ = writer.await;
    let _ = reader.await;

    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Serialize a [`ServerMsg`] and send it as a WS text frame.
async fn send_msg(
    sink: &mut (impl SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin),
    msg: &ServerMsg,
) -> Result<()> {
    let json = serde_json::to_string(msg)?;
    sink.send(Message::Text(json)).await?;
    Ok(())
}

/// Riconosce il comando slash `/find`. Ritorna `Some(query)` (il resto dopo
/// `find`, trim) se il PRIMO token è esattamente `find` (case-insensitive);
/// `None` altrimenti (incluso `/findfoo`). Stesso parsing di `core::handle_slash`.
fn parse_find(input: &str) -> Option<String> {
    let trimmed = input.trim();
    let without_slash = trimmed.strip_prefix('/')?;
    let mut parts = without_slash.splitn(2, char::is_whitespace);
    let cmd = parts.next().unwrap_or("");
    if !cmd.eq_ignore_ascii_case("find") {
        return None;
    }
    Some(parts.next().unwrap_or("").trim().to_string())
}

/// Attempt to parse a WS frame as a `ClientMsg::Hello` and return
/// `(token, channel)`.
fn parse_hello(msg: &Message) -> Option<(String, Option<String>)> {
    let text = msg.to_text().ok()?;
    let client_msg: ClientMsg = serde_json::from_str(text).ok()?;
    match client_msg {
        ClientMsg::Hello { token, channel } => Some((token, channel)),
        _ => None,
    }
}

/// Decide se QUESTA connessione ha diritto a diventare il sink condiviso di
/// `PluginHost` (`set_server_tx`). `PluginHost` è un `Arc<Mutex<>>` unico per
/// processo, condiviso da ogni connessione WS — solo il cursore principale
/// (`channel: None`) deve poterlo reclamare. Una connessione di canale (es.
/// `/nmap`, `channel: Some("nmap")`) non usa mai plugin (`ToolClient` isolato
/// per canale) e non deve rubare il sink alla finestra principale.
///
/// Bug trovato dal vivo (2026-07-18): prima di questo controllo, QUALSIASI
/// connessione — inclusa quella di un canale esterno — sovrascriveva
/// incondizionatamente il sink condiviso. Un plugin attivato per la prima
/// volta dopo l'apertura di `/nmap` catturava (nel proprio pump task) il
/// sink sbagliato: il suo `ShowWindow` finiva sulla connessione di canale,
/// che non ha alcun case per quel messaggio (scartato silenziosamente) — da
/// cui "exit 0, nessuna finestra" osservato in modo intermittente.
fn connection_owns_plugin_sink(channel: Option<&str>) -> bool {
    channel.is_none()
}

/// Verifica se la fonte dati mercato ATTIVA (`market_data.json`, letta lato
/// Python — vedi `scripts/pytools/financial-markets/`) è raggiungibile.
/// Chiama il tool MCP `test_market_data_source` (Task 9) su un
/// `PythonMcpToolClient` SCOPED: costruito, usato una volta e lasciato
/// cadere qui — nessuno stato condiviso da gestire nel resto
/// dell'orchestrator. Il client MCP "normale" del canale `financial-markets`
/// (quello usato dall'AI in `/markets`) resta invece per-connessione dentro
/// `external_channel.rs`, invariato: sono due istanze indipendenti, questa
/// nasce e muore per una singola chiamata di test (Task 11 design note).
///
/// Un solo punto per due chiamanti: l'arm `ClientMsg::TestMarketDataSource`
/// qui sopra (azione manuale da `/config`) e il check informativo all'avvio
/// (`main.rs`) — evita di duplicare la stessa costruzione del client + lo
/// stesso parsing difensivo della risposta JSON del tool Python in due file.
///
/// Ritorna sempre `(ok, message)`, mai un panic: venv assente, timeout di
/// rete, o risposta JSON malformata diventano tutti `ok: false` con un
/// messaggio leggibile in `message`.
///
/// # Nota SRP
/// Il doc-comment di modulo in cima a questo file dichiara che `ws.rs` non
/// dovrebbe contenere "logica... di tool" — questa funzione la contiene
/// (costruisce un `PythonMcpToolClient` e lo dispaccia). È una deroga
/// consapevole al perimetro dichiarato dal modulo: lo scope del Task 11 è
/// limitato a `ws.rs` + `main.rs`, e un client "scoped" per-chiamata (non
/// condiviso altrove nell'orchestrator, vedi la nota architetturale sopra)
/// non ha oggi una casa più naturale senza aggiungere un terzo file. Se in
/// futuro questo test cresce (più fonti dati, retry, cache), estrarlo in un
/// modulo dedicato (es. `market_data_check.rs`) sarebbe la scelta corretta.
///
/// `rt` (2.0, Task 4, D6): `RuntimeConfig` risolto una volta in `main()` —
/// passato esplicitamente a `PythonMcpToolClient::resolve`, mai ri-derivato
/// qui (niente più la vecchia variabile d'ambiente della v1 per la cartella
/// pytools).
pub async fn test_market_data_source_now(rt: &crate::runtime_config::RuntimeConfig) -> (bool, String) {
    match crate::python_mcp_tool_client::PythonMcpToolClient::resolve(
        &rt.config_dir,
        &rt.startup,
        "financial-markets",
        "server.py",
        vec![crate::python_mcp_tool_client::PythonToolSpec {
            def: crate::messages_client::ToolDef {
                name: "test_market_data_source".to_string(),
                description: "Verifica se la fonte dati mercato attiva è raggiungibile.".to_string(),
                input_schema: serde_json::json!({ "type": "object", "properties": {}, "required": [] }),
            },
            // Nessuna finestra Markdown per questo test: solo ok/message.
            report_title: None,
            defer_report_to_turn_end: false,
        }],
        30,
    ) {
        Ok(client) => {
            let outcome = client
                .dispatch("test_market_data_source", &serde_json::json!({}))
                .await;
            // Il client è SCOPED (vive solo per questa chiamata): senza uno
            // `shutdown()` esplicito, il processo Python spawnato da
            // `dispatch()`/`ensure_connected()` resta orfano indefinitamente
            // — vedi il doc-comment di `PythonMcpToolClient::close_connection`,
            // stessa classe di bug già corretta una volta per nmap (fix
            // "orphan process" precedente in questo repo). Sicuro anche se
            // `dispatch` non si è mai davvero connesso (es. tool rifiutato
            // prima di `ensure_connected`): `close_connection_does_not_kill_
            // when_never_connected` copre esattamente questo caso no-op.
            client.shutdown().await;
            if outcome.is_error {
                (false, outcome.output)
            } else {
                // `outcome.output` è la stringa JSON {"ok":bool,"message":str}
                // ritornata dal tool Python (Task 9) — parsing difensivo, mai un
                // panic su una risposta inattesa.
                match serde_json::from_str::<serde_json::Value>(&outcome.output) {
                    Ok(v) => (
                        v.get("ok").and_then(|x| x.as_bool()).unwrap_or(false),
                        v.get("message")
                            .and_then(|x| x.as_str())
                            .unwrap_or("risposta non valida")
                            .to_string(),
                    ),
                    Err(_) => (
                        false,
                        "risposta del tool Python non valida (JSON malformato)".to_string(),
                    ),
                }
            }
        }
        Err(e) => (false, format!("venv financial-markets non trovato: {e}")),
    }
}

// ─── Test ─────────────────────────────────────────────────────────────────────
//
// TDD RED: scritto PRIMA che `connection_owns_plugin_sink` esistesse — pre-fix
// il codice non compilava (`E0425: cannot find function`), quel fallimento di
// compilazione era la fase RED. La copertura si ferma qui deliberatamente:
// `handle_connection` chiama `plugin_host.lock().await.set_server_tx(...)`
// dentro un `if` che consulta questa funzione — provare la wiring end-to-end
// richiederebbe un vero processo plugin (il dispatch dei plugin lazy in
// `handle_connection` usa `spawn_plugin` reale, non iniettabile nei test di
// `tests/ws_integration.rs`), quindi quel punto va verificato per ispezione,
// non con un test automatico.

#[cfg(test)]
mod tests {
    use super::connection_owns_plugin_sink;

    #[test]
    fn main_cursor_connection_owns_the_plugin_sink() {
        assert!(
            connection_owns_plugin_sink(None),
            "channel: None (cursore principale) deve poter reclamare il sink di PluginHost"
        );
    }

    #[test]
    fn channel_connection_never_owns_the_plugin_sink() {
        assert!(
            !connection_owns_plugin_sink(Some("nmap")),
            "una connessione di canale non deve mai rubare il sink di PluginHost al cursore principale"
        );
    }
}
