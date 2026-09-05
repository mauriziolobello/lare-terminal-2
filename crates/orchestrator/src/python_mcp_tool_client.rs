//! # python_mcp_tool_client — `ToolClient` per server MCP scritti in Python
//!
//! Mirror di `nmap_tool_client.rs` per un server MCP **Python** invece che un
//! binario nativo Rust: stesso pattern lazy-spawn-and-reuse, stesso handshake
//! MCP via `rmcp`, stessa filosofia (venv creato a mano dall'utente, mai un
//! provisioning automatico — vedi Docs/superpowers/specs/2026-07-21-pytools-
//! infrastructure-design.md). A differenza di `NmapToolClient`, questo tipo è
//! GENERICO: non conosce quale dominio/tool sta parlando, riceve `domain_id`
//! e `script_relpath` da chi lo costruisce (la voce di registro in
//! `external_channel.rs`).

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::messages_client::ToolDef;
use crate::tool_client::{ChannelReport, CommandResult, DispatchOutcome, OpenResult, ToolClient};

/// Uno schema tool + un modo OPZIONALE di trasformare il suo output in un
/// `ChannelReport` (finestra Markdown deterministica + pulsante Salva in
/// Library, mirror di `nmap_tool_client.rs::call_scan_tool`). Iniettato dal
/// chiamante (voce di registro in `external_channel.rs`) — questo file
/// resta generico: non sa quali tool esistano né quali producano un report,
/// lo decide chi registra il canale, tool per tool.
///
/// `report_title`, se presente, viene chiamata con l'`input` originale
/// della chiamata (non con l'output) per derivare il titolo della finestra
/// — stesso schema di `NmapToolClient::call_scan_tool`, che costruisce il
/// titolo da `target` (un parametro della CHIAMATA, non della risposta).
/// Un tool con `report_title: None` (es. `search_ticker`) non apre mai una
/// finestra, qualunque cosa risponda: resta testo semplice in chat.
pub struct PythonToolSpec {
    pub def: ToolDef,
    pub report_title: Option<fn(&serde_json::Value) -> String>,
    /// Letto SOLO quando `report_title` è `Some` (ignorato altrimenti, nessun
    /// report da aprire in nessun caso). `true` (`stock_report`): la finestra
    /// attende il testo finale del turno, che viene fuso in coda al documento
    /// — necessario quando il documento include una narrativa scritta
    /// dall'AI che non esiste ancora al momento del dispatch. `false`
    /// (`list_stocks`): il documento è interamente deterministico, la
    /// finestra si apre SUBITO al dispatch, stesso pattern immediato di nmap.
    pub defer_report_to_turn_end: bool,
}

/// Contratto JSON opt-in che un tool Python può seguire per separare un
/// riassunto testuale (per l'AI, senza immagini — vedi
/// `scripts/pytools/financial-markets/report.py::strip_chart_images`) dal documento
/// completo (per la finestra/Library), più un terzo riassunto numerico
/// brevissimo per il pannello del canale (`channel_summary` — refinement
/// 2026-07-23, vedi `report.py::build_channel_summary`). Un tool che non lo
/// segue (es. `pyping`, che ritorna una stringa semplice) fallisce
/// silenziosamente il parsing — `dispatch()` allora usa tutto il testo
/// com'è, nessun report: mai un errore per un tool che non ha scelto questo
/// contratto.
#[derive(serde::Deserialize)]
struct PythonReportJson {
    summary: String,
    report_markdown: String,
    channel_summary: String,
    /// Titolo BASE della finestra, fornito da Python (additivo — 2026-08-11,
    /// registry screener multipli). Quando presente ha precedenza sul
    /// risultato di `report_title(input)`: il registry Python conosce il
    /// nome giusto (titolo dello screener eseguito), la title_fn Rust non
    /// può saperlo per un dispatcher generico come `run_screener`. Assente
    /// per i tool che non lo mandano ancora (`stock_report`/`list_stocks`)
    /// → comportamento invariato per loro (fallback su `report_title`).
    #[serde(default)]
    title: Option<String>,
    /// Suffisso OPZIONALE appeso al titolo della finestra (= nome del
    /// salvataggio in Library) — es. " — 27/07/2026 15:42" da
    /// `screen_stocks`, che ha l'orologio lato Python mentre la title_fn
    /// Rust del registro no. Il titolo BASE resta della title_fn (il
    /// registro rimane autoritativo sul naming); assente per i tool che
    /// non lo mandano (`stock_report`/`list_stocks`) → titolo invariato.
    #[serde(default)]
    title_suffix: Option<String>,
}

/// Astrazione su "uccidi un processo per PID" — stesso seam di
/// `nmap_tool_client.rs::ProcessTreeKiller`, semplificato: qui non serve un
/// kill-tree (`/T`), lo script Python non spawna figli elevati/reparentati
/// come fa `nmap_os_detect` via UAC.
trait ProcessKiller: Send + Sync {
    fn kill(&self, pid: u32);
}

struct RealProcessKiller;

impl ProcessKiller for RealProcessKiller {
    fn kill(&self, pid: u32) {
        #[cfg(windows)]
        {
            match std::process::Command::new("taskkill")
                .args(["/F", "/PID", &pid.to_string()])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
            {
                Ok(_) => tracing::info!("killed python tool process (PID {pid})"),
                Err(e) => tracing::warn!("failed to spawn taskkill for PID {pid}: {e}"),
            }
        }
        #[cfg(not(windows))]
        {
            match std::process::Command::new("kill").args(["-9", &pid.to_string()]).spawn() {
                Ok(_) => tracing::info!("killed python tool process (PID {pid})"),
                Err(e) => tracing::warn!("failed to spawn kill for PID {pid}: {e}"),
            }
        }
    }
}

/// **Concurrency invariant this type relies on (not enforced by the type
/// system):** at most one `dispatch()` call is ever in flight on a given
/// `PythonMcpToolClient` at a time. Today that's guaranteed externally, not
/// by this file — each WS connection gets its own fresh
/// `PythonMcpToolClient` (`external_channel.rs`'s `tool_client` factory,
/// called once per connection), and within one connection `ws.rs` holds its
/// `ConversationHistory` lock across the whole `handle_command(...).await`,
/// including any in-flight tool call, so a second command on the same
/// connection can't reach `dispatch()` until the first one returns. This is
/// why `peer` and `child_pid` being two separate `Mutex`es below is safe in
/// practice: nothing can interleave an `ensure_connected` write with a
/// `close_connection` read/clear of a DIFFERENT connection.
/// If that concurrency guarantee ever changes (e.g. tool calls become
/// concurrent within one connection), a timeout on one call could kill the
/// Python process another concurrent call is still legitimately using,
/// and/or these two fields could be read torn-apart across connections — at
/// that point they'd need to move into one `Mutex<Option<(Peer, u32)>>` for
/// atomicity. Not done here: `rmcp::Peer` has no public test-friendly
/// constructor, and merging the two fields would make `close_connection`'s
/// pid-kill behavior impossible to unit-test without a real MCP connection
/// (mirror di `NmapToolClient`'s stesso ragionamento — vedi il suo doc
/// comment per il caso completo, incluso il residuo noto sul batching
/// multi-tool_use; qui NON esiste il tracking multi-tool di quel client,
/// perché l'invariante di concorrenza sopra (un solo `dispatch()` alla
/// volta per connessione) vale indipendentemente da QUANTI tool il canale
/// espone — anche con più tool iniettati (`tool_defs`), il kill resta
/// semplice — `ProcessKiller::kill`, non un kill-tree — perché lo script
/// Python non spawna figli elevati via UAC come fa `nmap_os_detect`).
pub struct PythonMcpToolClient {
    /// Percorso dell'interprete Python del venv (`venv/Scripts/python.exe`
    /// su Windows, `venv/bin/python3` su Unix) — risolto una volta sola in
    /// `resolve()`, mai ricalcolato dopo.
    python_path: std::path::PathBuf,
    /// Percorso dello script/server MCP da passare come argomento
    /// all'interprete — anch'esso risolto una volta sola in `resolve()`.
    script_path: std::path::PathBuf,
    /// Cartella di configurazione (2.0, D6) — passata al figlio come
    /// `--config-dir` allo spawn (vedi `ensure_connected`, sotto).
    config_dir: std::path::PathBuf,
    /// Handle MCP verso il processo Python connesso, se lo spawn lazy è già
    /// avvenuto — `None` finché nessuna `dispatch()` è ancora passata da
    /// `ensure_connected()`. Vedi il doc comment del tipo per l'invariante
    /// di concorrenza che rende sicuro tenerlo in un `Mutex` separato da
    /// `child_pid` invece che in un unico `Mutex<Option<(Peer, u32)>>`.
    peer: Arc<Mutex<Option<rmcp::Peer<rmcp::RoleClient>>>>,
    /// PID del processo Python attualmente connesso, se presente —
    /// catturato in `ensure_connected` subito dopo lo spawn, così
    /// `close_connection` può ucciderlo per PID senza bisogno di un handle
    /// vivo sul child (che è di proprietà del task detached avviato in
    /// `ensure_connected`).
    child_pid: Arc<Mutex<Option<u32>>>,
    /// Seam di dependency injection per il kill del processo — permette ai
    /// test di verificare "è stato ucciso il PID giusto" con un
    /// `FakeProcessKiller`, senza mai toccare un processo reale.
    killer: Arc<dyn ProcessKiller>,
    /// Schema dei tool esposti da questo canale + eventuale derivazione
    /// report, iniettati dal chiamante (la voce di registro in
    /// `external_channel.rs`) — questo tipo resta GENERICO: non conosce
    /// quali tool esistano né quali producano un report finché non gli
    /// vengono passati. `tool_defs()`/`dispatch()` operano solo su questo
    /// vettore, mai su un nome hardcoded.
    tool_specs: Vec<PythonToolSpec>,
    /// Timeout per una singola `call_tool`, iniettato dal chiamante — un
    /// canale i cui tool fanno più round-trip di rete (es. financial-markets)
    /// può richiederne uno più alto del default storico di `python-ping`
    /// (60s), senza che questo file debba conoscere il motivo.
    call_timeout_secs: u64,
}

impl PythonMcpToolClient {
    /// Risolve interprete + script per `domain_id`/`script_relpath`.
    ///
    /// `<root>` è SEMPRE la radice `pytools/` (mai la cartella di un dominio
    /// specifico) — da `startup.json.paths.pytools_dir` (default `"pytools"`),
    /// risolta rispetto alla radice del deploy (D6, 2.0: nessuna env var —
    /// la v1 leggeva `env_override` qui). `domain_id` si unisce sopra
    /// `<root>`. Interprete: `venv/Scripts/python.exe` su Windows,
    /// `venv/bin/python3` su Unix (helper unico, stile `PathProvider`).
    /// Verifica che interprete E script esistano PRIMA di spawnare: mancante
    /// → `Err` leggibile (nomina l'ambito e il file mancante), mai un panic
    /// — l'utente deve poter capire "crea il venv" da solo, senza leggere
    /// il sorgente.
    pub fn resolve(
        config_dir: &std::path::Path,
        cfg: &startup_config::StartupConfig,
        domain_id: &str,
        script_relpath: &str,
        tool_specs: Vec<PythonToolSpec>,
        call_timeout_secs: u64,
    ) -> anyhow::Result<Self> {
        let pytools_root = startup_config::StartupConfig::resolve_path(config_dir, &cfg.paths.pytools_dir);
        let root = pytools_root.join(domain_id);

        #[cfg(windows)]
        let python_path = root.join("venv").join("Scripts").join("python.exe");
        #[cfg(not(windows))]
        let python_path = root.join("venv").join("bin").join("python3");

        let script_path = root.join(script_relpath);

        if !python_path.exists() {
            anyhow::bail!(
                "venv Python non trovato per \"{domain_id}\": {} — crea il virtual environment (vedi scripts/pytools/README.md)",
                python_path.display()
            );
        }
        if !script_path.exists() {
            anyhow::bail!("script non trovato per \"{domain_id}\": {}", script_path.display());
        }

        Ok(Self {
            python_path,
            script_path,
            config_dir: config_dir.to_path_buf(),
            peer: Arc::new(Mutex::new(None)),
            child_pid: Arc::new(Mutex::new(None)),
            killer: Arc::new(RealProcessKiller),
            tool_specs,
            call_timeout_secs,
        })
    }

    /// Spawn lazy + handshake MCP reale al primo uso; riusa il peer alle
    /// chiamate successive. Mirror esatto di
    /// `NmapToolClient::ensure_connected`, minus il tracking multi-tool (qui
    /// non serve: un solo `dispatch()` alla volta per connessione, vedi
    /// l'invariante di concorrenza sul doc comment dello struct).
    async fn ensure_connected(&self) -> Result<rmcp::Peer<rmcp::RoleClient>, String> {
        let mut peer_guard = self.peer.lock().await;
        if let Some(peer) = peer_guard.as_ref() {
            return Ok(peer.clone());
        }

        use rmcp::{transport::TokioChildProcess, ServiceExt};

        let child_cmd = {
            let mut c = tokio::process::Command::new(&self.python_path);
            // `--config-dir`: nessuna env var (D6) — stessa cartella
            // dell'orchestrator, passata esplicitamente allo script.
            c.arg(&self.script_path)
                .arg(startup_config::CONFIG_DIR_FLAG)
                .arg(&self.config_dir)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::inherit());
            c
        };

        let transport = TokioChildProcess::new(child_cmd)
            .map_err(|e| format!("failed to spawn python tool: {e}"))?;
        let pid = transport.id();

        let running = ().serve(transport).await.map_err(|e| format!("MCP handshake failed: {e}"))?;
        let peer = running.peer().clone();
        tokio::spawn(async move {
            running.waiting().await.ok();
        });

        *peer_guard = Some(peer.clone());
        *self.child_pid.lock().await = pid;
        Ok(peer)
    }

    /// Azzera il peer cache e uccide il processo Python connesso, se
    /// presente. Mirror di `NmapToolClient::close_connection` (stessa
    /// motivazione: senza questo, il processo spawnato resta orfano
    /// indefinitamente alla chiusura della finestra/timeout).
    async fn close_connection(&self) {
        *self.peer.lock().await = None;
        if let Some(pid) = self.child_pid.lock().await.take() {
            self.killer.kill(pid);
        }
    }
}

#[async_trait]
impl ToolClient for PythonMcpToolClient {
    /// Uccide il processo Python connesso (se presente) alla chiusura di
    /// questa connessione — vedi `close_connection`'s doc comment.
    async fn shutdown(&self) {
        self.close_connection().await;
    }

    /// Chiuso strutturalmente: questo canale non ha MAI una shell reale
    /// dietro — `dispatch()` è l'unica via verso il processo Python, mai
    /// `Route::Os`/`Route::Slash`'s `run_in_session` (stesso principio di
    /// `NmapToolClient`, Docs/superpowers/specs/2026-07-16-tool-isolation-
    /// design.md: isolare per canale cosa un tool esterno può toccare).
    async fn run_in_session(
        &self,
        _command: &str,
        _progress_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
    ) -> CommandResult {
        CommandResult {
            stdout: String::new(),
            stderr: "run_in_session non disponibile su questo canale".to_string(),
            exit_code: -1,
            cwd: String::new(),
        }
    }

    /// No-op: questo canale non ha una sessione shell reale da riavviare
    /// (non ce n'è mai stata una — vedi `run_in_session` sopra).
    async fn reset_session(&self) {}

    /// Chiuso strutturalmente: la via `Route::Slash` (`/open`) non delega
    /// mai a un target reale su questo canale — stesso motivo di
    /// `run_in_session` sopra.
    async fn open_target(&self, _target: &str) -> OpenResult {
        OpenResult { ok: false, message: "open_target non disponibile su questo canale".to_string() }
    }

    /// Chiuso strutturalmente: questo canale non conosce il repository di
    /// routine — stesso motivo di `run_in_session` sopra.
    async fn search_routines(&self, _query: Option<&str>) -> crate::tool_client::SearchRoutinesResult {
        crate::tool_client::SearchRoutinesResult {
            results: Vec::new(),
            error: Some("search_routines non disponibile su questo canale".to_string()),
        }
    }

    /// Chiuso strutturalmente: stesso motivo di `search_routines` sopra.
    async fn run_routine(&self, _name: &str, _args: Option<&str>) -> crate::tool_client::RunRoutineResult {
        crate::tool_client::RunRoutineResult {
            ok: false,
            message: "run_routine non disponibile su questo canale".to_string(),
            stdout: String::new(),
            stderr: String::new(),
            exit_code: -1,
            cwd: String::new(),
        }
    }

    fn tool_defs(&self) -> Vec<ToolDef> {
        self.tool_specs.iter().map(|s| s.def.clone()).collect()
    }

    async fn dispatch(&self, name: &str, input: &serde_json::Value) -> DispatchOutcome {
        let Some(spec) = self.tool_specs.iter().find(|s| s.def.name == name) else {
            return DispatchOutcome {
                output: format!("tool sconosciuto sul canale: {name}"),
                is_error: true,
                report: None,
                channel_summary: None,
            };
        };

        let peer = match self.ensure_connected().await {
            Ok(p) => p,
            Err(e) => return DispatchOutcome { output: e, is_error: true, report: None, channel_summary: None },
        };

        use rmcp::model::CallToolRequestParams;
        let args = match input {
            serde_json::Value::Object(map) => map.clone(),
            _ => serde_json::Map::new(),
        };
        let params = CallToolRequestParams::new(name.to_string()).with_arguments(args);

        let call_future = peer.call_tool(params);
        let result = match tokio::time::timeout(
            std::time::Duration::from_secs(self.call_timeout_secs),
            call_future,
        )
        .await
        {
            Ok(r) => r,
            Err(_elapsed) => {
                self.close_connection().await;
                return DispatchOutcome {
                    output: format!(
                        "\u{26a0} Timeout: {name} ha superato {}s ed è stato annullato.",
                        self.call_timeout_secs
                    ),
                    is_error: true,
                    report: None,
                    channel_summary: None,
                };
            }
        };

        match result {
            Err(e) => {
                *self.peer.lock().await = None;
                DispatchOutcome { output: format!("call_tool {name} failed: {e}"), is_error: true, report: None, channel_summary: None }
            }
            Ok(tool_result) => split_report(join_text_content(&tool_result), spec.report_title, spec.defer_report_to_turn_end, input),
        }
    }
}

/// Se `report_title` è iniettata E `text` è un JSON `{"summary":...,
/// "report_markdown":..., "channel_summary":...}`, separa in tre parti:
/// `output=summary` (all'AI, per il suo ragionamento — non mostrato nel
/// pannello), `channel_summary=Some(...)` (mostrato SUBITO nel pannello,
/// deterministico — mai dall'AI) e `report=Some(ChannelReport{title,
/// markdown: report_markdown, defer_to_turn_end})` — `defer_to_turn_end` è
/// letto dal `PythonToolSpec` del chiamante (`spec.defer_report_to_turn_end`),
/// non più hardcoded: `true` (`stock_report`) POSPONE l'apertura finestra
/// fino al testo finale del turno, che `ai_adapter.rs` fonde in coda al
/// documento (le sezioni 5-6 dell'AI non esistono ancora a questo punto);
/// `false` (`list_stocks`, Task 4) apre SUBITO al dispatch, come nmap
/// (`nmap_tool_client.rs::call_scan_tool`, il cui documento è già completo al
/// dispatch). `title_fn` è chiamata con l'INPUT originale della chiamata (non
/// con la risposta) — nmap costruisce il proprio titolo allo stesso modo, da
/// `target`, un parametro della chiamata. Se il parsing fallisce (tool che
/// non segue il contratto, es. `pyping`) o non c'è `report_title` per questo
/// tool (es. `search_ticker`), ritorna tutto il testo com'è, nessun report —
/// fallback sicuro, mai un errore per un tool che non ha scelto questo
/// contratto.
fn split_report(text: String, report_title: Option<fn(&serde_json::Value) -> String>, defer_to_turn_end: bool, input: &serde_json::Value) -> DispatchOutcome {
    if let Some(title_fn) = report_title {
        if let Ok(parsed) = serde_json::from_str::<PythonReportJson>(&text) {
            // "title" dal JSON Python ha precedenza (registry autoritativo);
            // altrimenti fallback sulla title_fn Rust (comportamento di oggi
            // per i tool che non mandano ancora "title").
            let mut title = parsed.title.clone().unwrap_or_else(|| title_fn(input));
            if let Some(suffix) = &parsed.title_suffix {
                title.push_str(suffix);
            }
            return DispatchOutcome {
                output: parsed.summary,
                report: Some(ChannelReport { title, markdown: parsed.report_markdown, defer_to_turn_end }),
                is_error: false,
                channel_summary: Some(parsed.channel_summary),
            };
        }
    }
    DispatchOutcome { output: text, is_error: false, report: None, channel_summary: None }
}

#[cfg(test)]
mod split_report_tests {
    use super::split_report;

    #[test]
    fn splits_into_summary_and_report_when_title_fn_present_and_json_matches() {
        let text = serde_json::json!({"summary": "breve", "report_markdown": "# Completo", "channel_summary": "AAPL: 150"}).to_string();
        let title_fn = |input: &serde_json::Value| format!("Titolo — {}", input.get("ticker").and_then(|v| v.as_str()).unwrap_or("?"));
        let outcome = split_report(text, Some(title_fn), true, &serde_json::json!({"ticker": "AAPL"}));
        assert_eq!(outcome.output, "breve");
        assert!(!outcome.is_error);
        assert_eq!(outcome.channel_summary, Some("AAPL: 150".to_string()));
        let report = outcome.report.expect("atteso un report");
        assert_eq!(report.title, "Titolo — AAPL");
        assert_eq!(report.markdown, "# Completo");
        assert!(report.defer_to_turn_end, "defer_to_turn_end:true richiesto in input deve propagarsi al ChannelReport");
    }

    #[test]
    fn title_from_json_takes_precedence_over_title_fn() {
        // run_screener manda "title" nel JSON — il registry Python è
        // autoritativo sul nome, la title_fn Rust resta un fallback per i
        // tool che non lo mandano ancora (list_stocks/stock_report).
        let text = serde_json::json!({
            "summary": "s", "report_markdown": "# Doc", "channel_summary": "c",
            "title": "Financial Markets — Uso Consumer"
        }).to_string();
        let title_fn = |_: &serde_json::Value| "Titolo fallback (mai usato)".to_string();
        let outcome = split_report(text, Some(title_fn), true, &serde_json::json!({}));
        let report = outcome.report.expect("atteso un report");
        assert_eq!(report.title, "Financial Markets — Uso Consumer");
    }

    #[test]
    fn title_suffix_still_applies_after_json_title() {
        let text = serde_json::json!({
            "summary": "s", "report_markdown": "# Doc", "channel_summary": "c",
            "title": "Financial Markets — Uso Consumer",
            "title_suffix": " — 11/08/2026 10:00"
        }).to_string();
        let title_fn = |_: &serde_json::Value| "fallback".to_string();
        let outcome = split_report(text, Some(title_fn), true, &serde_json::json!({}));
        let report = outcome.report.expect("atteso un report");
        assert_eq!(report.title, "Financial Markets — Uso Consumer — 11/08/2026 10:00");
    }

    #[test]
    fn missing_json_title_falls_back_to_title_fn_unchanged() {
        // list_stocks/stock_report non mandano "title" ancora — comportamento
        // di oggi invariato: la title_fn resta l'unica fonte.
        let text = serde_json::json!({
            "summary": "s", "report_markdown": "# Doc", "channel_summary": "c"
        }).to_string();
        let title_fn = |input: &serde_json::Value| format!("Titolo — {}", input.get("ticker").and_then(|v| v.as_str()).unwrap_or("?"));
        let outcome = split_report(text, Some(title_fn), true, &serde_json::json!({"ticker": "AAPL"}));
        let report = outcome.report.expect("atteso un report");
        assert_eq!(report.title, "Titolo — AAPL");
    }

    #[test]
    fn defer_to_turn_end_false_produces_immediate_open_report() {
        // list_stocks-style: nessuna narrativa AI da attendere, la finestra
        // deve aprirsi subito al dispatch — prima di questo test, split_report
        // non aveva MAI un caso che producesse defer_to_turn_end:false.
        let text = serde_json::json!({"summary": "s", "report_markdown": "# Lista", "channel_summary": "N titoli"}).to_string();
        let title_fn = |_: &serde_json::Value| "Titolo".to_string();
        let outcome = split_report(text, Some(title_fn), false, &serde_json::json!({}));
        let report = outcome.report.expect("atteso un report");
        assert!(!report.defer_to_turn_end, "defer_to_turn_end:false in input deve produrre un ChannelReport con defer_to_turn_end:false");
    }

    #[test]
    fn title_suffix_from_the_tool_json_is_appended_to_the_window_title() {
        // Richiesta post-verifica dal vivo (2026-07-27): screen_stocks vuole
        // data/ora anche nel titolo FINESTRA (= nome del salvataggio in
        // Library). Il titolo base resta della title_fn Rust (registro
        // autoritativo sul naming), ma il tool Python può appendere un
        // suffisso dinamico — l'orologio ce l'ha lui, non il registro.
        let text = serde_json::json!({"summary": "s", "report_markdown": "# Doc", "channel_summary": "c", "title_suffix": " — 27/07/2026 15:42"}).to_string();
        let title_fn = |_: &serde_json::Value| "Financial Markets — Screening potenziale".to_string();
        let outcome = split_report(text, Some(title_fn), true, &serde_json::json!({}));
        let report = outcome.report.expect("atteso un report");
        assert_eq!(report.title, "Financial Markets — Screening potenziale — 27/07/2026 15:42");
    }

    #[test]
    fn missing_title_suffix_keeps_the_title_fn_result_unchanged() {
        // stock_report/list_stocks non mandano title_suffix: il campo è
        // opzionale nel contratto, il titolo resta quello della title_fn —
        // retrocompatibilità totale per i tool esistenti.
        let text = serde_json::json!({"summary": "s", "report_markdown": "# Doc", "channel_summary": "c"}).to_string();
        let title_fn = |_: &serde_json::Value| "Titolo base".to_string();
        let outcome = split_report(text, Some(title_fn), true, &serde_json::json!({}));
        assert_eq!(outcome.report.expect("atteso un report").title, "Titolo base");
    }

    #[test]
    fn no_report_title_leaves_text_unchanged_even_if_it_matches_the_json_shape() {
        // search_ticker non ha report_title iniettata: anche se il suo
        // output SOMIGLIASSE al contratto {summary, report_markdown,
        // channel_summary} (non lo fa mai in pratica), non deve mai aprire
        // una finestra né mostrare un riassunto di canale.
        let text = serde_json::json!({"summary": "s", "report_markdown": "m", "channel_summary": "c"}).to_string();
        let outcome = split_report(text.clone(), None, false, &serde_json::json!({}));
        assert_eq!(outcome.output, text);
        assert!(outcome.report.is_none());
        assert!(outcome.channel_summary.is_none());
    }

    #[test]
    fn non_json_text_falls_back_to_plain_output_even_with_title_fn_present() {
        // pyping-style: report_title potrebbe essere iniettata per errore su
        // un tool che non segue il contratto — il fallback deve restare
        // sicuro, mai un pannico/errore per un tool che ritorna testo semplice.
        let outcome = split_report("pong: ciao".to_string(), Some(|_: &serde_json::Value| "mai usato".to_string()), true, &serde_json::json!({}));
        assert_eq!(outcome.output, "pong: ciao");
        assert!(outcome.report.is_none());
        assert!(outcome.channel_summary.is_none());
    }
}

/// Concatena TUTTI i blocchi di testo di un `CallToolResult`, non solo il
/// primo. FastMCP (lato Python) serializza un `list[dict]` ritornato da un
/// tool come UN blocco di contenuto per elemento della lista — un tool come
/// `search_ticker` che trova N corrispondenze produce N blocchi. Prendere
/// solo il primo (con `find_map`, com'era prima) troncava silenziosamente
/// tutti i risultati tranne il primo: scoperto dal vivo interrogando
/// `search_ticker("Novo Nordisk")`, che risponde con due match (NVO e
/// NONOF) — l'AI non avrebbe mai visto il secondo, vanificando l'istruzione
/// del system prompt "se risultati multipli ambigui, chiedi all'utente
/// quale". Un tool che ritorna una singola stringa (`pyping`,
/// `stock_report`) produce un solo blocco: per questi il comportamento è
/// identico a prima, unito da un separatore che qui non emerge mai.
fn join_text_content(result: &rmcp::model::CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|c| c.as_text().map(|t| t.text.clone()))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod join_text_content_tests {
    use super::join_text_content;
    use rmcp::model::{CallToolResult, Content};

    #[test]
    fn joins_multiple_text_blocks_not_just_the_first() {
        let result = CallToolResult::success(vec![Content::text("NVO"), Content::text("NONOF")]);
        assert_eq!(join_text_content(&result), "NVO\nNONOF");
    }

    #[test]
    fn single_text_block_is_unchanged_no_trailing_separator() {
        let result = CallToolResult::success(vec![Content::text("pong: ciao")]);
        assert_eq!(join_text_content(&result), "pong: ciao");
    }

    #[test]
    fn no_content_blocks_yields_empty_string_not_a_panic() {
        let result = CallToolResult::success(vec![]);
        assert_eq!(join_text_content(&result), "");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// RED (Task 4, brief Step 3): `PythonMcpToolClient::resolve` prende
    /// `config_dir`/`StartupConfig` al posto di `env_override` — niente più
    /// env var (D6). Il venv non esiste apposta: verifica che il messaggio
    /// d'errore citi il percorso ATTESO, calcolato da `paths.pytools_dir`
    /// (default `"pytools"`) risolto rispetto alla radice del deploy (padre
    /// di `config_dir`).
    #[test]
    fn python_client_paths_come_from_startup_pytools_dir() {
        let cfg = startup_config::StartupConfig::default(); // pytools_dir = "pytools"
        let r = PythonMcpToolClient::resolve(
            std::path::Path::new("C:/Lare/Configuration"),
            &cfg,
            "python-ping",
            "server.py",
            vec![],
            30,
        );
        // il venv non esiste: l'errore deve citare il percorso atteso, calcolato dalla radice del deploy
        let msg = format!("{}", r.err().unwrap());
        assert!(msg.contains("python-ping"));
        assert!(msg.replace('\\', "/").contains("C:/Lare/pytools/python-ping"));
    }

    /// Crea in una tempdir un venv FINTO (solo il file dell'interprete, vuoto)
    /// e opzionalmente lo script — abbastanza per superare i controlli di
    /// `Path::exists()` di `resolve()`, senza bisogno di Python reale.
    fn fake_domain_dir(tmp: &std::path::Path, domain_id: &str, with_python: bool, with_script: bool) -> std::path::PathBuf {
        let domain_dir = tmp.join(domain_id);
        #[cfg(windows)]
        let python_rel = domain_dir.join("venv").join("Scripts").join("python.exe");
        #[cfg(not(windows))]
        let python_rel = domain_dir.join("venv").join("bin").join("python3");

        if with_python {
            std::fs::create_dir_all(python_rel.parent().unwrap()).unwrap();
            std::fs::File::create(&python_rel).unwrap().write_all(b"").unwrap();
        }
        if with_script {
            std::fs::create_dir_all(&domain_dir).unwrap();
            std::fs::File::create(domain_dir.join("server.py")).unwrap().write_all(b"").unwrap();
        }
        domain_dir
    }

    /// `cfg.paths.pytools_dir` assoluto → usato COSÌ COM'È da
    /// `StartupConfig::resolve_path` (mai risolto contro la radice del
    /// deploy): comodo nei test per puntare direttamente a una tempdir,
    /// `config_dir` passato è quindi irrilevante (qualunque valore va bene).
    fn cfg_with_pytools_root(root: &std::path::Path) -> startup_config::StartupConfig {
        let mut cfg = startup_config::StartupConfig::default();
        cfg.paths.pytools_dir = root.display().to_string();
        cfg
    }

    #[test]
    fn resolve_errs_with_readable_message_when_venv_missing() {
        let tmp = tempfile::tempdir().unwrap();
        fake_domain_dir(tmp.path(), "test-domain", false, true);
        let cfg = cfg_with_pytools_root(tmp.path());
        let result = PythonMcpToolClient::resolve(std::path::Path::new("unused"), &cfg, "test-domain", "server.py", vec![], 60);
        // `.expect_err()`/`.unwrap_err()` richiedono `T: Debug` sul tipo Ok —
        // `PythonMcpToolClient` non lo implementa (contiene `Arc<dyn
        // ProcessKiller>`, un trait object senza supertrait Debug), quindi un
        // match esplicito evita di dover derivare Debug solo per i test.
        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("venv mancante deve produrre Err, non Ok"),
        };
        let msg = err.to_string();
        assert!(msg.contains("test-domain"), "il messaggio deve nominare l'ambito: {msg}");
        assert!(msg.to_lowercase().contains("venv") || msg.to_lowercase().contains("python"), "messaggio poco leggibile: {msg}");
    }

    #[test]
    fn resolve_errs_with_readable_message_when_script_missing() {
        let tmp = tempfile::tempdir().unwrap();
        fake_domain_dir(tmp.path(), "test-domain", true, false);
        let cfg = cfg_with_pytools_root(tmp.path());
        let result = PythonMcpToolClient::resolve(std::path::Path::new("unused"), &cfg, "test-domain", "server.py", vec![], 60);
        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("script mancante deve produrre Err, non Ok"),
        };
        assert!(err.to_string().contains("server.py"), "il messaggio deve nominare lo script: {err}");
    }

    #[test]
    fn resolve_succeeds_when_venv_and_script_exist() {
        let tmp = tempfile::tempdir().unwrap();
        fake_domain_dir(tmp.path(), "test-domain", true, true);
        let cfg = cfg_with_pytools_root(tmp.path());
        let result = PythonMcpToolClient::resolve(std::path::Path::new("unused"), &cfg, "test-domain", "server.py", vec![], 60);
        assert!(result.is_ok(), "venv e script presenti: resolve() deve riuscire, errore: {:?}", result.err());
    }

    fn fake_client(killer: Arc<dyn ProcessKiller>, child_pid: Option<u32>, tool_specs: Vec<PythonToolSpec>) -> PythonMcpToolClient {
        PythonMcpToolClient {
            python_path: "unused".into(),
            script_path: "unused".into(),
            config_dir: "unused".into(),
            peer: Arc::new(Mutex::new(None)),
            child_pid: Arc::new(Mutex::new(child_pid)),
            killer,
            tool_specs,
            call_timeout_secs: 60,
        }
    }

    fn fake_spec(name: &str, report_title: Option<fn(&serde_json::Value) -> String>) -> PythonToolSpec {
        PythonToolSpec {
            def: crate::messages_client::ToolDef { name: name.to_string(), description: "d".to_string(), input_schema: serde_json::json!({}) },
            report_title,
            defer_report_to_turn_end: false,
        }
    }

    #[derive(Default)]
    struct FakeProcessKiller {
        killed_pids: std::sync::Mutex<Vec<u32>>,
    }

    impl ProcessKiller for FakeProcessKiller {
        fn kill(&self, pid: u32) {
            self.killed_pids.lock().unwrap().push(pid);
        }
    }

    #[tokio::test]
    async fn run_in_session_never_executes_a_real_shell() {
        let client = fake_client(Arc::new(FakeProcessKiller::default()), None, vec![]);
        let result = client.run_in_session("dir", None).await;
        assert_eq!(result.exit_code, -1);
        assert!(result.stderr.contains("non disponibile su questo canale"));
    }

    #[tokio::test]
    async fn open_target_never_opens_anything() {
        let client = fake_client(Arc::new(FakeProcessKiller::default()), None, vec![]);
        let result = client.open_target("C:\\Users").await;
        assert!(!result.ok);
        assert!(result.message.contains("non disponibile su questo canale"));
    }

    #[test]
    fn tool_defs_returns_exactly_the_injected_defs() {
        let spec = fake_spec("pyping", None);
        let expected = vec![spec.def.clone()];
        let client = fake_client(Arc::new(FakeProcessKiller::default()), None, vec![spec]);
        assert_eq!(client.tool_defs(), expected);
    }

    #[tokio::test]
    async fn dispatch_rejects_unknown_tool_name_without_spawning() {
        let client = fake_client(Arc::new(FakeProcessKiller::default()), None, vec![]);
        let outcome = client.dispatch("qualcosa_altro", &serde_json::json!({})).await;
        assert!(outcome.is_error);
        assert!(outcome.output.contains("qualcosa_altro"));
    }

    #[tokio::test]
    async fn shutdown_kills_the_connected_process() {
        use crate::tool_client::ToolClient;
        let killer = Arc::new(FakeProcessKiller::default());
        let client = fake_client(killer.clone(), Some(9001), vec![]);
        client.shutdown().await;
        assert_eq!(killer.killed_pids.lock().unwrap().as_slice(), &[9001]);
        // L'invariante reale è "niente doppio kill": `Option::take()` su
        // `child_pid` garantisce che una SECONDA `shutdown()`/`close_
        // connection()` non trovi più nulla da uccidere. Verificare solo
        // `killed_pids` sopra non lo testa — un bug che dimenticasse di
        // azzerare lo stato passerebbe comunque quell'assert. Mirror di
        // `NmapToolClient::close_connection_kills_the_stored_pid_and_clears_
        // state`.
        assert!(client.child_pid.lock().await.is_none(), "child_pid deve essere azzerato dopo lo shutdown");
        assert!(client.peer.lock().await.is_none(), "peer deve essere azzerato dopo lo shutdown");
    }

    #[tokio::test]
    async fn close_connection_does_not_kill_when_never_connected() {
        let killer = Arc::new(FakeProcessKiller::default());
        let client = fake_client(killer.clone(), None, vec![]);
        client.close_connection().await;
        assert!(killer.killed_pids.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn dispatch_accepts_any_injected_tool_name_and_attempts_to_connect() {
        // Nome arbitrario, mai "pyping": prova che la validazione ora si basa
        // sui tool_defs iniettati, non su un literal hardcoded. python_path
        // inesistente forza un fallimento di spawn (mai un vero processo).
        let client = PythonMcpToolClient {
            python_path: "does-not-exist.exe".into(),
            script_path: "does-not-exist.py".into(),
            config_dir: "unused".into(),
            peer: Arc::new(Mutex::new(None)),
            child_pid: Arc::new(Mutex::new(None)),
            killer: Arc::new(FakeProcessKiller::default()),
            tool_specs: vec![fake_spec("custom_tool", None)],
            call_timeout_secs: 60,
        };
        let outcome = client.dispatch("custom_tool", &serde_json::json!({})).await;
        assert!(outcome.is_error);
        assert!(
            !outcome.output.contains("tool sconosciuto"),
            "doveva tentare lo spawn (nome iniettato valido), non rifiutare il nome: {}",
            outcome.output
        );
    }

    /// Spawn REALE contro il venv effettivo di `scripts/pytools/python-ping/` nel
    /// repo — richiede che tu l'abbia già creato (vedi scripts/pytools/README.md).
    /// Non gira in un `cargo test` normale.
    #[tokio::test]
    #[ignore = "richiede scripts/pytools/python-ping/venv creato a mano"]
    async fn real_pyping_roundtrip_via_venv() {
        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        // `pytools_dir` assoluto nella `StartupConfig` di test: punta alla
        // RADICE scripts/pytools/ — `resolve()` unisce "python-ping" sopra
        // da solo, come fa in produzione (`external_channel.rs` chiama
        // `resolve(config_dir, cfg, "python-ping", ...)`). `config_dir`
        // passato è irrilevante (path assoluto vince — D6, niente env var).
        let pytools_root = repo_root.join("scripts").join("pytools");
        let cfg = cfg_with_pytools_root(&pytools_root);

        let client = PythonMcpToolClient::resolve(
            std::path::Path::new("unused"),
            &cfg,
            "python-ping",
            "server.py",
            vec![PythonToolSpec {
                def: crate::messages_client::ToolDef {
                    name: "pyping".to_string(),
                    description: "Tool di prova: restituisce un'eco del messaggio ricevuto.".to_string(),
                    input_schema: serde_json::json!({
                        "type": "object",
                        "properties": { "message": { "type": "string", "description": "Messaggio da inviare in eco" } },
                        "required": ["message"]
                    }),
                },
                report_title: None,
                defer_report_to_turn_end: false,
            }],
            60,
        )
        .expect("resolve() deve riuscire — hai creato il venv? vedi scripts/pytools/README.md");
        let outcome = client
            .dispatch("pyping", &serde_json::json!({"message": "ciao"}))
            .await;

        assert!(!outcome.is_error, "dispatch fallito: {outcome:?}");
        assert!(outcome.output.contains("ciao"), "atteso un'eco di 'ciao': {outcome:?}");
    }

    /// Spawn REALE contro il venv effettivo di `scripts/pytools/financial-markets/`
    /// — richiede che tu l'abbia già creato (vedi scripts/pytools/README.md) e che
    /// `tickers_us.json` sia già stato scaricato almeno una volta. Regressione
    /// per `join_text_content`: `search_ticker("Novo Nordisk")` produce DUE
    /// match reali (NVO, NONOF — la stessa società ha un secondo ticker OTC),
    /// cioè due blocchi di contenuto MCP separati — se questo test tornasse a
    /// vedere solo il primo, la sintomatologia dal vivo che ha fatto scoprire
    /// il bug (l'AI non vede mai il secondo risultato per disambiguare) si
    /// ripresenterebbe silenziosamente. Non gira in un `cargo test` normale.
    #[tokio::test]
    #[ignore = "richiede scripts/pytools/financial-markets/venv creato a mano + tickers_us.json"]
    async fn real_search_ticker_multi_match_roundtrip_via_venv() {
        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let pytools_root = repo_root.join("scripts").join("pytools");
        let cfg = cfg_with_pytools_root(&pytools_root);

        let client = PythonMcpToolClient::resolve(
            std::path::Path::new("unused"),
            &cfg,
            "financial-markets",
            "server.py",
            vec![PythonToolSpec {
                def: crate::messages_client::ToolDef {
                    name: "search_ticker".to_string(),
                    description: "Risolve un nome societario nel ticker USA corrispondente.".to_string(),
                    input_schema: serde_json::json!({
                        "type": "object",
                        "properties": { "query": { "type": "string" } },
                        "required": ["query"]
                    }),
                },
                report_title: None,
                defer_report_to_turn_end: false,
            }],
            120,
        )
        .expect("resolve() deve riuscire — hai creato il venv? vedi scripts/pytools/README.md");
        let outcome = client.dispatch("search_ticker", &serde_json::json!({"query": "Novo Nordisk"})).await;

        assert!(!outcome.is_error, "dispatch fallito: {outcome:?}");
        // Entrambi i ticker devono comparire: ciascuno arriva in un blocco di
        // contenuto MCP SEPARATO (non un array JSON unico) — se join_text_
        // content tornasse a prendere solo il primo blocco (find_map), NONOF
        // scomparirebbe silenziosamente pur passando l'assert su NVO da solo.
        assert!(outcome.output.contains("NVO"), "atteso il ticker NVO tra i match: {outcome:?}");
        assert!(outcome.output.contains("NONOF"), "atteso ANCHE il secondo match NONOF — se manca, join_text_content ha ripreso a troncare al primo blocco: {outcome:?}");
    }

    /// Spawn REALE di `stock_report` su un ticker vero (AAPL). Copre DUE
    /// regressioni: (1) "Mixed timezones detected" di `charts.py::bars_to_
    /// dataframe` su OHLC intraday reale (mai esercitato dai test unitari,
    /// che usano solo `FakeDataSource`) — verificato da "il tool non
    /// esplode"; (2) lo split report/summary (`split_report`) sull'output
    /// REALE del tool Python, non su un `CallToolResult` finto — prova che
    /// `server.py` segue davvero il contratto JSON `{summary,
    /// report_markdown}` e che `report.strip_chart_images` toglie
    /// davvero le immagini dal riassunto (che va all'AI) lasciandole nel
    /// documento completo (che va alla finestra/Library). Non verifica il
    /// contenuto esatto (i dati di mercato cambiano ogni giorno). Non gira
    /// in un `cargo test` normale.
    #[tokio::test]
    #[ignore = "richiede scripts/pytools/financial-markets/venv creato a mano + rete reale verso Yahoo Finance"]
    async fn real_stock_report_roundtrip_via_venv() {
        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let pytools_root = repo_root.join("scripts").join("pytools");
        let cfg = cfg_with_pytools_root(&pytools_root);

        let client = PythonMcpToolClient::resolve(
            std::path::Path::new("unused"),
            &cfg,
            "financial-markets",
            "server.py",
            vec![PythonToolSpec {
                def: crate::messages_client::ToolDef {
                    name: "stock_report".to_string(),
                    description: "Report fattuale completo per un ticker USA.".to_string(),
                    input_schema: serde_json::json!({
                        "type": "object",
                        "properties": { "ticker": { "type": "string" } },
                        "required": ["ticker"]
                    }),
                },
                report_title: Some(|input| {
                    let ticker = input.get("ticker").and_then(|v| v.as_str()).unwrap_or("?");
                    format!("Financial Markets — {ticker}")
                }),
                defer_report_to_turn_end: true,
            }],
            120,
        )
        .expect("resolve() deve riuscire — hai creato il venv? vedi scripts/pytools/README.md");
        let outcome = client.dispatch("stock_report", &serde_json::json!({"ticker": "AAPL"})).await;

        assert!(!outcome.is_error, "dispatch fallito (report crashato?): {outcome:?}");
        for heading in ["## Fondamentale", "## Grafici", "## Option chain", "## Comparative"] {
            assert!(outcome.output.contains(heading), "sezione mancante nel riassunto (output) {heading}: primi 300 char: {}", &outcome.output[..300.min(outcome.output.len())]);
        }
        assert!(
            !outcome.output.contains("data:image/png;base64,"),
            "il riassunto per l'AI (output) non deve contenere immagini — strip_chart_images non ha funzionato"
        );
        let report = outcome.report.as_ref().expect("stock_report deve produrre un ChannelReport (report_title iniettata)");
        assert_eq!(report.title, "Financial Markets — AAPL");
        assert!(report.defer_to_turn_end, "stock_report deve sempre posporre l'apertura al testo finale del turno");
        assert!(report.markdown.contains("data:image/png;base64,"), "il documento completo (report.markdown) deve contenere i grafici");
        for heading in ["## Fondamentale", "## Grafici", "## Option chain", "## Comparative"] {
            assert!(report.markdown.contains(heading), "sezione mancante nel documento completo {heading}");
        }
        let channel_summary = outcome.channel_summary.as_ref().expect("stock_report deve produrre un channel_summary");
        assert!(channel_summary.lines().count() <= 5, "il riassunto di canale deve restare entro 5 righe: {channel_summary:?}");
    }
}
