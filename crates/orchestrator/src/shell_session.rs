//! # shell_session — la shell dell'utente come `ToolClient` (2.0, spec §4)
//!
//! In v1 `run_in_session` girava nella shell **posseduta** da mcp-server
//! (modello A). Nel 2.0 il canale shell è la sessione PowerShell dell'utente
//! stesso (modello B): il tool diventa un round-trip sul WebSocket —
//! `ServerMsg::ExecInShell{exec_id}` → la host esegue → `ClientMsg::ExecResult
//! {exec_id}` — correlato da un `oneshot` per `exec_id`.
//!
//! Due oggetti, per separare ciò che vive quanto la connessione da ciò che
//! vive quanto un turno:
//! - [`ShellSessionState`]: per **connessione** (cwd della sessione, mappa
//!   delle esecuzioni pendenti, canale d'uscita, `McpToolClient` per i tool
//!   che non riguardano la shell). Creato in `ws.rs` dopo la `Hello`.
//! - [`ShellSessionToolClient`]: per **turno** (conosce il `turn_id` da
//!   scrivere in ogni `ExecInShell`). Implementa `ToolClient`; è ciò che
//!   `core::handle_command` riceve come `tools`.
//!
//! La cwd è **per sessione** (D17): non tocca mai il `cwd_state` globale v1.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use protocol::ServerMsg;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::{oneshot, Mutex};

use crate::messages_client::ToolDef;
use crate::tool_client::{
    CommandResult, DispatchOutcome, GetRoutineContentResult, OpenResult, RunRoutineResult,
    SaveRoutineResult, SearchRoutinesResult, ToolClient,
};

/// Esito di un `ExecInShell`, come arriva in `ClientMsg::ExecResult`.
#[derive(Debug, Clone, PartialEq)]
pub struct ExecReply {
    pub exit_code: i32,
    pub output: String,
    pub cwd: String,
}

/// Messaggio restituito all'AI quando un'esecuzione non può concludersi
/// (connessione shell chiusa, turno annullato con Ctrl+C).
pub const EXEC_ABORTED: &str = "esecuzione interrotta: sessione shell chiusa o turno annullato";

pub struct ShellSessionState {
    session_id: String,
    out_tx: UnboundedSender<ServerMsg>,
    cwd: Mutex<String>,
    /// `exec_id → (turn_id, sender)`: il `turn_id` permette di abortire tutte
    /// le esecuzioni di UN turno (Ctrl+C) senza toccare le altre.
    pending: Mutex<HashMap<String, (String, oneshot::Sender<ExecReply>)>>,
    /// Tool che NON riguardano la shell dell'utente (`open_target`, routine):
    /// delegati al `McpToolClient` condiviso, come per ogni altra connessione.
    mcp: Arc<dyn ToolClient>,
    config_dir: PathBuf,
}

impl ShellSessionState {
    pub fn new(
        session_id: impl Into<String>,
        out_tx: UnboundedSender<ServerMsg>,
        initial_cwd: impl Into<String>,
        mcp: Arc<dyn ToolClient>,
        config_dir: PathBuf,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            out_tx,
            cwd: Mutex::new(initial_cwd.into()),
            pending: Mutex::new(HashMap::new()),
            mcp,
            config_dir,
        }
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub async fn cwd(&self) -> String {
        self.cwd.lock().await.clone()
    }

    /// Aggiorna la cwd della sessione; una stringa vuota (host che non la
    /// riporta) lascia il valore precedente.
    pub async fn set_cwd(&self, cwd: &str) {
        if !cwd.is_empty() {
            *self.cwd.lock().await = cwd.to_string();
        }
    }

    /// Manda `ExecInShell` e attende l'`ExecResult` corrispondente.
    /// Non ha timeout di proposito (spec §4.4): un comando può durare
    /// quanto vuole; è Ctrl+C (`abort_turn`) o la chiusura della connessione
    /// (`abort_all`) a sbloccarlo.
    pub async fn exec(&self, turn_id: &str, command: &str, capture: bool) -> ExecReply {
        let exec_id = format!("{:032x}", rand::random::<u128>());
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .await
            .insert(exec_id.clone(), (turn_id.to_string(), tx));
        let msg = ServerMsg::ExecInShell {
            turn_id: turn_id.to_string(),
            exec_id: exec_id.clone(),
            command: command.to_string(),
            capture,
        };
        if self.out_tx.send(msg).is_err() {
            // Connessione già chiusa: niente attesa, e via l'entry orfana.
            self.pending.lock().await.remove(&exec_id);
            return aborted();
        }
        match rx.await {
            Ok(reply) => {
                self.set_cwd(&reply.cwd).await;
                reply
            }
            Err(_) => aborted(), // sender droppato da abort_turn/abort_all
        }
    }

    /// Risolve l'esecuzione `exec_id`. `false` = id ignoto, già risolto, O
    /// (M8, review finale) `turn_id` non corrispondente a quello memorizzato
    /// per questo `exec_id`: in quel caso l'entry NON viene rimossa — una
    /// risposta con `turn_id` sbagliato (host che manda `ExecResult` di un
    /// turno diverso, o un doppione tardivo) non deve poter consumare
    /// l'attesa di un altro turno con dati che non gli appartengono. La
    /// risposta corretta, se arriva più tardi, risolve regolarmente.
    pub async fn resolve_exec(&self, exec_id: &str, turn_id: &str, reply: ExecReply) -> bool {
        let mut pending = self.pending.lock().await;
        // Prestito immutabile che finisce subito dopo il controllo — non
        // confligge col `remove` (mutabile) subito sotto, perché qui sotto
        // costruiamo solo un `bool`, nessun riferimento sopravvive.
        match pending.get(exec_id) {
            Some((stored_turn, _)) if stored_turn == turn_id => {}
            Some(stored_turn) => {
                tracing::warn!(
                    "ExecResult per exec_id={exec_id}: turn_id={turn_id} non corrisponde a quello atteso ({}) — scartato, esecuzione pendente non rimossa",
                    stored_turn.0
                );
                return false;
            }
            None => return false,
        }
        match pending.remove(exec_id) {
            Some((_turn, tx)) => tx.send(reply).is_ok(),
            None => false,
        }
    }

    /// Abortisce le esecuzioni pendenti del turno `turn_id` (Ctrl+C sulla
    /// host: la pipeline è già stata fermata lì, nessun `ExecResult` arriverà).
    pub async fn abort_turn(&self, turn_id: &str) {
        self.pending.lock().await.retain(|_, (t, _)| t != turn_id);
    }

    /// Abortisce tutto (teardown della connessione).
    pub async fn abort_all(&self) {
        self.pending.lock().await.clear();
    }
}

fn aborted() -> ExecReply {
    ExecReply {
        exit_code: -1,
        output: EXEC_ABORTED.to_string(),
        cwd: String::new(),
    }
}

/// I tool esposti all'AI su una sessione shell: quelli storici
/// (`agent::tool_defs()`) meno `run_routine` (la routine girerebbe nella shell
/// posseduta da mcp-server, non in quella dell'utente — fuorviante), con
/// `run_in_session` ridescritto e arricchito dell'input `interactive` (§4.5).
/// Costruito trasformando la lista v1, non ricopiandola: descrizioni e schemi
/// degli altri tool restano una cosa sola.
pub fn shell_tool_defs() -> Vec<ToolDef> {
    crate::agent::tool_defs()
        .into_iter()
        .filter(|t| t.name != "run_routine")
        .map(|mut t| {
            if t.name == "run_in_session" {
                t.description = "Esegui un comando nella sessione PowerShell dell'utente (la sua shell: cwd ed env persistono, l'output compare nel suo terminale). Usa per qualsiasi operazione fattibile da terminale.".to_string();
                t.input_schema = serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": { "type": "string", "description": "Il comando da eseguire" },
                        "interactive": { "type": "boolean", "description": "true se il programma ha bisogno di un terminale interattivo (editor, REPL, pager, wizard): l'output non viene catturato e non ti torna indietro; restano exit_code e cwd. Default false." }
                    },
                    "required": ["command"]
                });
            }
            t
        })
        .collect()
}

/// `ToolClient` di UN turno di una sessione shell (vedi doc-comment del modulo).
pub struct ShellSessionToolClient {
    state: Arc<ShellSessionState>,
    turn_id: String,
}

impl ShellSessionToolClient {
    pub fn for_turn(state: &Arc<ShellSessionState>, turn_id: String) -> Arc<dyn ToolClient> {
        Arc::new(Self {
            state: Arc::clone(state),
            turn_id,
        })
    }

    async fn exec_to_outcome(&self, command: &str, capture: bool) -> DispatchOutcome {
        let r = self.state.exec(&self.turn_id, command, capture).await;
        let output = if r.output.is_empty() {
            format!("(nessun output, exit_code={})", r.exit_code)
        } else {
            r.output
        };
        DispatchOutcome {
            output,
            is_error: r.exit_code != 0,
            report: None,
            channel_summary: None,
        }
    }
}

#[async_trait]
impl ToolClient for ShellSessionToolClient {
    /// `progress_tx` è ignorato: l'output arriva in blocco con l'`ExecResult`
    /// (l'utente lo vede già scorrere nel suo terminale, spec §3.2).
    async fn run_in_session(
        &self,
        command: &str,
        _progress_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
    ) -> CommandResult {
        let r = self.state.exec(&self.turn_id, command, true).await;
        CommandResult {
            stdout: r.output,
            stderr: String::new(),
            exit_code: r.exit_code,
            cwd: r.cwd,
        }
    }

    /// La sessione è dell'utente: non c'è nulla da riavviare (spec §3, `/reset`).
    async fn reset_session(&self) {}

    async fn open_target(&self, target: &str) -> OpenResult {
        self.state.mcp.open_target(target).await
    }

    async fn search_routines(&self, query: Option<&str>) -> SearchRoutinesResult {
        self.state.mcp.search_routines(query).await
    }

    async fn run_routine(&self, _name: &str, _args: Option<&str>) -> RunRoutineResult {
        RunRoutineResult {
            ok: false,
            message: "run_routine non disponibile nella sessione shell: usa run_in_session"
                .to_string(),
            stdout: String::new(),
            stderr: String::new(),
            exit_code: -1,
            cwd: String::new(),
        }
    }

    async fn get_routine_content(&self, name: &str) -> GetRoutineContentResult {
        self.state.mcp.get_routine_content(name).await
    }

    async fn save_routine(
        &self,
        name: &str,
        description: &str,
        tags: Vec<String>,
        category: &str,
        content: &str,
        replace: Option<&str>,
    ) -> SaveRoutineResult {
        self.state
            .mcp
            .save_routine(name, description, tags, category, content, replace)
            .await
    }

    fn tool_defs(&self) -> Vec<ToolDef> {
        shell_tool_defs()
    }

    async fn dispatch(&self, name: &str, input: &serde_json::Value) -> DispatchOutcome {
        if name == "run_in_session" {
            // Intercettato QUI (non in `agent::dispatch_tool_at`) perché solo la
            // shell conosce `interactive` → `capture` (§4.5).
            let command = input.get("command").and_then(|v| v.as_str()).unwrap_or("");
            let interactive = input
                .get("interactive")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            return self.exec_to_outcome(command, !interactive).await;
        }
        // Tutto il resto: stessa via di `CwdTrackingToolClient::dispatch` — su
        // `self`, così un tool che rientra passa da questo impl.
        crate::agent::dispatch_tool_at(
            self as &dyn ToolClient,
            name,
            input,
            &self.state.config_dir.join("network.json"),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_client::FakeToolClient;
    use tokio::sync::mpsc::unbounded_channel;

    fn state() -> (
        Arc<ShellSessionState>,
        tokio::sync::mpsc::UnboundedReceiver<ServerMsg>,
    ) {
        let (tx, rx) = unbounded_channel();
        let mcp: Arc<dyn ToolClient> = Arc::new(FakeToolClient::success("mcp output\n"));
        let s = Arc::new(ShellSessionState::new(
            "s1",
            tx,
            "C:\\start",
            mcp,
            PathBuf::from("/cfg"),
        ));
        (s, rx)
    }

    /// `run_in_session` manda `ExecInShell{turn_id, capture: true}` e torna
    /// SOLO quando arriva l'`ExecResult` con lo stesso `exec_id`; la cwd della
    /// sessione viene aggiornata da `ExecResult.cwd`.
    #[tokio::test]
    async fn run_in_session_round_trips_exec_in_shell_and_updates_cwd() {
        let (s, mut rx) = state();
        let tools = ShellSessionToolClient::for_turn(&s, "t1".into());
        let run = tokio::spawn(async move { tools.run_in_session("dir", None).await });
        let msg = rx.recv().await.unwrap();
        let exec_id = match msg {
            ServerMsg::ExecInShell {
                turn_id,
                exec_id,
                command,
                capture,
            } => {
                assert_eq!(turn_id, "t1");
                assert_eq!(command, "dir");
                assert!(capture);
                exec_id
            }
            other => panic!("atteso ExecInShell, ricevuto {other:?}"),
        };
        assert!(
            s.resolve_exec(
                &exec_id,
                "t1",
                ExecReply {
                    exit_code: 0,
                    output: "a.txt\n".into(),
                    cwd: "C:\\dopo".into()
                }
            )
            .await
        );
        let r = run.await.unwrap();
        assert_eq!(
            (r.exit_code, r.stdout.as_str(), r.cwd.as_str()),
            (0, "a.txt\n", "C:\\dopo")
        );
        assert_eq!(s.cwd().await, "C:\\dopo");
    }

    /// `dispatch("run_in_session", {interactive: true})` → `capture: false` (§4.5).
    #[tokio::test]
    async fn dispatch_interactive_sends_capture_false() {
        let (s, mut rx) = state();
        let tools = ShellSessionToolClient::for_turn(&s, "t1".into());
        let run = tokio::spawn(async move {
            tools
                .dispatch(
                    "run_in_session",
                    &serde_json::json!({"command": "python", "interactive": true}),
                )
                .await
        });
        let exec_id = match rx.recv().await.unwrap() {
            ServerMsg::ExecInShell {
                exec_id, capture, ..
            } => {
                assert!(!capture);
                exec_id
            }
            other => panic!("ricevuto {other:?}"),
        };
        s.resolve_exec(
            &exec_id,
            "t1",
            ExecReply {
                exit_code: 0,
                output: String::new(),
                cwd: String::new(),
            },
        )
        .await;
        let out = run.await.unwrap();
        assert!(!out.is_error);
        assert_eq!(out.output, "(nessun output, exit_code=0)");
    }

    /// Ctrl+C sulla host (`CancelCommand`) → `abort_turn`: l'`await` pendente
    /// torna subito con exit_code -1 e il testo `EXEC_ABORTED`.
    #[tokio::test]
    async fn abort_turn_releases_pending_exec_with_error() {
        let (s, mut rx) = state();
        let tools = ShellSessionToolClient::for_turn(&s, "t1".into());
        let run = tokio::spawn(async move { tools.run_in_session("sleep 100", None).await });
        let _ = rx.recv().await.unwrap();
        s.abort_turn("t1").await;
        let r = run.await.unwrap();
        assert_eq!(r.exit_code, -1);
        assert_eq!(r.stdout, EXEC_ABORTED);
        // Un ExecResult tardivo per l'exec abortito è scartato (id ignoto:
        // l'entry è già stata rimossa da `abort_turn`, `turn_id` qui non conta).
        assert!(
            !s.resolve_exec(
                "qualunque",
                "t1",
                ExecReply {
                    exit_code: 0,
                    output: String::new(),
                    cwd: String::new()
                }
            )
            .await
        );
    }

    /// M8 (review finale): un `ExecResult` con `turn_id` diverso da quello
    /// memorizzato per `exec_id` viene RIFIUTATO (non un panic) e — punto
    /// chiave — NON consuma l'esecuzione pendente: la risposta corretta,
    /// arrivata dopo, deve poter ancora risolvere lo stesso `exec_id`.
    #[tokio::test]
    async fn resolve_exec_with_wrong_turn_id_is_rejected() {
        let (s, mut rx) = state();
        let tools = ShellSessionToolClient::for_turn(&s, "t1".into());
        let run = tokio::spawn(async move { tools.run_in_session("dir", None).await });
        let exec_id = match rx.recv().await.unwrap() {
            ServerMsg::ExecInShell { exec_id, .. } => exec_id,
            other => panic!("ricevuto {other:?}"),
        };
        // Risposta con turn_id sbagliato: scartata, l'entry resta pendente.
        assert!(
            !s.resolve_exec(
                &exec_id,
                "t-altro-turno",
                ExecReply {
                    exit_code: 0,
                    output: "risposta sbagliata".into(),
                    cwd: String::new(),
                }
            )
            .await
        );
        // La risposta corretta arriva DOPO: risolve regolarmente, prova che
        // il tentativo scartato sopra non ha rimosso l'entry.
        assert!(
            s.resolve_exec(
                &exec_id,
                "t1",
                ExecReply {
                    exit_code: 0,
                    output: "a.txt\n".into(),
                    cwd: "C:\\dopo".into(),
                }
            )
            .await
        );
        let r = run.await.unwrap();
        assert_eq!((r.exit_code, r.stdout.as_str()), (0, "a.txt\n"));
    }

    /// Una cwd vuota nell'`ExecResult` (host che non la riporta) non cancella
    /// quella nota — stessa regola di `CwdTrackingToolClient`.
    #[tokio::test]
    async fn empty_cwd_in_result_keeps_previous_cwd() {
        let (s, mut rx) = state();
        let tools = ShellSessionToolClient::for_turn(&s, "t1".into());
        let run = tokio::spawn(async move { tools.run_in_session("dir", None).await });
        let exec_id = match rx.recv().await.unwrap() {
            ServerMsg::ExecInShell { exec_id, .. } => exec_id,
            other => panic!("ricevuto {other:?}"),
        };
        s.resolve_exec(
            &exec_id,
            "t1",
            ExecReply {
                exit_code: 1,
                output: String::new(),
                cwd: String::new(),
            },
        )
        .await;
        run.await.unwrap();
        assert_eq!(s.cwd().await, "C:\\start");
    }

    /// I tool esposti all'AI: quelli v1 SENZA `run_routine`, e `run_in_session`
    /// con l'input opzionale `interactive`.
    #[test]
    fn shell_tool_defs_drop_run_routine_and_add_interactive() {
        let defs = shell_tool_defs();
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(!names.contains(&"run_routine"), "{names:?}");
        assert!(names.contains(&"run_in_session"));
        assert!(names.contains(&"open_target"));
        assert!(names.contains(&"show_markdown"));
        let ris = defs.iter().find(|d| d.name == "run_in_session").unwrap();
        assert!(ris.input_schema["properties"]["interactive"].is_object());
        assert_eq!(ris.input_schema["required"], serde_json::json!(["command"]));
    }

    /// `run_routine` non esiste sulla shell: rifiuto esplicito, mai un panic.
    #[tokio::test]
    async fn run_routine_is_rejected_and_other_tools_delegate_to_mcp() {
        let (s, _rx) = state();
        let tools = ShellSessionToolClient::for_turn(&s, "t1".into());
        let r = tools.run_routine("x", None).await;
        assert!(!r.ok);
        assert!(r.message.contains("non disponibile"));
        let o = tools.open_target("C:\\x").await;
        assert!(o.ok, "open_target delega a McpToolClient (fake ok)");
    }
}
