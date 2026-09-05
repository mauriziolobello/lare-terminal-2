//! # external_channel — registro dei canali tool esterni (base comune)
//!
//! Vedi `Docs/superpowers/specs/2026-07-16-external-tool-channel-design.md` e
//! `Docs/superpowers/specs/2026-07-16-tool-isolation-design.md`. Un canale
//! esterno è una connessione WS **scoped**: la sua AI vede SOLO i tool di quel
//! canale (mai `run_in_session`/`open_target`), isolata dalla connessione
//! condivisa del cursore sia a livello di tool sia di concorrenza (una
//! conferma pendente su un canale non blocca il cursore). L'isolamento è
//! completo su 3 assi, tutti guidati dal `ToolClient` del canale:
//! - **Definizioni**: `tool_client().tool_defs()` sostituisce i 5 tool
//!   storici con quelli del canale (mai `run_in_session`/`show_markdown`).
//! - **Dispatch**: `tool_client().dispatch(name, input)` esegue SOLO i tool
//!   del canale — un nome fuori menu (anche `run_in_session`, se il modello
//!   lo invocasse per errore) è rifiutato come sconosciuto.
//! - **Vie Os/Slash**: lo stesso `ToolClient` rende `run_in_session`/
//!   `open_target`/`reset_session` "non disponibili su questo canale" —
//!   questo blocca ANCHE `Route::Os`/`Route::Slash` di `core.rs` (che
//!   restano invariati, non sanno nulla di canali).
//!
//! `EXTERNAL_TOOL_CHANNELS` ha la sua prima voce reale: `"nmap"` (vedi
//! `Docs/superpowers/specs/2026-07-16-mcp-nmap-design.md`). `channel: None`
//! resta invariato: `resolve_channel_tools` con `channel: None` si comporta
//! esattamente come prima della registrazione — nessun comportamento
//! visibile cambia per il cursore/Telegram, che non passano mai un `channel`.

use std::sync::Arc;

use crate::messages_client::ToolDef;
use crate::tool_client::{CommandResult, DispatchOutcome, OpenResult, ToolClient};

/// Etichetta di trasparenza/banner-di-conferma per un tool_use nmap (design
/// spec §5). `nmap_os_detect` dichiara ESPLICITAMENTE "richiede privilegi
/// elevati" — la dialog UAC di Windows non mostra il target, quindi questo è
/// l'unico punto dove l'utente lo vede prima dell'elevazione. Duplica
/// `mcp_nmap::markdown::format_nmap_invocation` (stessa logica, 10 righe) —
/// `orchestrator` non dipende dal crate `mcp-nmap` per una sola funzione di
/// formattazione; se un secondo canale avesse la stessa esigenza, quello
/// sarebbe il momento di condividerla.
fn format_nmap_invocation(tool_name: &str, args: &serde_json::Value) -> String {
    let target = args.get("target").and_then(|v| v.as_str()).unwrap_or("?");
    match tool_name {
        "nmap_quick_scan" => format!("nmap quick scan → {target}"),
        "nmap_os_detect" => format!("nmap OS detect (richiede privilegi elevati) → {target}"),
        "nmap_version_scan" => format!("nmap version scan (-sV) → {target}"),
        "nmap_host_discovery" => format!("nmap host discovery (-sn) → {target}"),
        "nmap_vuln_scan" => format!("nmap scansione vulnerabilità (--script vuln) → {target}"),
        "local_network_info" => "informazioni di rete locali".to_string(),
        "traceroute" => format!("traceroute → {target}"),
        other => format!("[tool {other}]"),
    }
}

/// System prompt del canale nmap — l'AI qui NON ha `run_in_session`/
/// `open_target`/`show_markdown`, solo i sette tool nmap (5 scan + 2
/// info). Dirlo esplicitamente evita che il modello provi comunque a
/// invocare un tool che non ha (Docs/superpowers/specs/2026-07-16-tool-
/// isolation-design.md). Riscritto per il piano 2026-07-18-nmap-scan-
/// variants.md, che ha portato i tool scan da 2 a 5: `nmap_vuln_scan` è
/// descritto in modo esplicito come limitato alla categoria NSE `vuln`
/// integrata in nmap, mai a uno script arbitrario, perché l'AI non dia
/// all'utente l'impressione di poter eseguire NSE a piacere.
const NMAP_SYSTEM_PROMPT: &str = "Sei l'assistente del canale nmap di Lare Terminal. Hai ESATTAMENTE sette strumenti: local_network_info (informazioni di rete della macchina locale — IP, subnet, gateway, ARP, routing — usalo PRIMA di uno scan se l'utente non specifica un target, per determinarlo da solo), traceroute (traccia il percorso di rete verso un host), nmap_quick_scan (scansione TCP connect rapida, nessun privilegio elevato), nmap_os_detect (rilevamento del sistema operativo, richiede privilegi elevati e un consenso esplicito dell'utente), nmap_version_scan (rilevamento delle versioni dei servizi in ascolto, -sV, nessun privilegio elevato), nmap_host_discovery (scoperta di quali host di una rete/range sono attivi, -sn, nessuna scansione porte, nessun privilegio elevato) e nmap_vuln_scan (verifica di vulnerabilità note tramite gli script NSE della categoria 'vuln' integrata in nmap — SOLO questa categoria fissa, non puoi eseguire né proporre script NSE diversi o personalizzati). Non hai accesso a una shell generica, non puoi aprire file o URL, non puoi mostrare finestre Markdown: il report di uno scan viene mostrato automaticamente all'utente, non serve che tu lo ripeta per intero — commenta brevemente l'esito. Rispondi in italiano, in modo conciso.";

/// `ToolClient` del canale `"library-expand"`: nessun tool custom (niente
/// `run_in_session`/`open_target`/`show_markdown`) — l'AI di questo canale ha
/// SOLO `web_search`/`web_fetch` server-side, aggiunti indipendentemente da
/// `agent::tools_for` quando `TurnOptions.web_search` è attivo. Mirror
/// strutturale di `NmapToolClient` (`nmap_tool_client.rs`), ma senza alcuno
/// stato: nessun processo sidecar da tenere in vita.
pub struct EmptyToolClient;

#[async_trait::async_trait]
impl ToolClient for EmptyToolClient {
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

    async fn reset_session(&self) {
        // No-op: nessuna sessione shell su questo canale.
    }

    async fn open_target(&self, _target: &str) -> OpenResult {
        OpenResult { ok: false, message: "open_target non disponibile su questo canale".to_string() }
    }

    async fn search_routines(&self, _query: Option<&str>) -> crate::tool_client::SearchRoutinesResult {
        crate::tool_client::SearchRoutinesResult {
            results: Vec::new(),
            error: Some("search_routines non disponibile su questo canale".to_string()),
        }
    }

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
        vec![]
    }

    async fn dispatch(&self, name: &str, _input: &serde_json::Value) -> DispatchOutcome {
        DispatchOutcome {
            output: format!("{name} non disponibile su questo canale"),
            is_error: true,
            report: None,
            channel_summary: None,
        }
    }
}

/// System prompt del canale `"library-expand"` (Docs/superpowers/specs/
/// 2026-07-20-library-expand-design.md §2). Il documento attuale e la
/// richiesta di ampliamento arrivano nel messaggio utente del turno — questa
/// stringa è fissa a compile-time, non può contenerli.
const LIBRARY_EXPAND_SYSTEM_PROMPT: &str = "Sei l'assistente del canale di espansione documenti di Lare Terminal. Riceverai un documento Markdown esistente e una richiesta di ampliamento. Il tuo compito è restituire l'INTERO documento risultante: fondi il contenuto vecchio e quello nuovo in un testo coeso e lineare — mai un elenco di aggiunte in coda, mai un patchwork di sezioni scollegate. Se hai a disposizione i tool web_search/web_fetch e la richiesta lo giustifica, usali per cercare informazioni pertinenti PRIMA di scrivere la versione finale. La tua risposta testuale è ESATTAMENTE il nuovo contenuto del documento: niente commenti fuori dal documento, niente frasi come \"Ecco la versione aggiornata:\", solo Markdown puro. Rispondi in italiano.";

/// System prompt del canale di prova `"python-ping"` — valida l'infrastruttura
/// tool Python (Docs/superpowers/specs/2026-07-21-pytools-infrastructure-
/// design.md) prima che arrivi un tool Python reale.
const PYTHON_PING_SYSTEM_PROMPT: &str = "Sei un canale di prova per l'infrastruttura tool Python di Lare Terminal. Hai un solo strumento disponibile, pyping, che accetta un messaggio testuale e restituisce una risposta di eco. Il tuo unico compito è chiamare pyping con il messaggio ricevuto dall'utente e riportarne il risultato, senza commenti aggiuntivi. Rispondi in italiano.";

/// System prompt del canale `"financial-markets"` — quattro tool:
/// search_ticker (risolve nome societario→ticker), stock_report (report
/// fattuale), list_stocks (elenco tabellare filtrabile per paese, Task 4) e
/// screen_stocks (screening "potenziale inespresso", finestra a fine turno
/// con la sezione AI '## Giudizio uso di massa' fusa in coda, come
/// stock_report). Vedi
/// Docs/superpowers/specs/2026-07-22-financial-markets-stock-report-design.md §5
/// e Docs/superpowers/specs/2026-07-26-financial-markets-screen-stocks-design.md.
///
/// Refinement 2026-07-23 (seconda correzione dal vivo, dopo la prima che ha
/// introdotto la finestra dedicata): la finestra Markdown ORA porta TUTTE e
/// 6 le sezioni, non solo le 4 fattuali — `ai_adapter.rs` la apre a fine
/// turno fondendo la risposta testuale dell'AI (letta qui sotto) in coda al
/// documento (`ChannelReport::defer_to_turn_end`), quindi questa risposta
/// deve essere Markdown VERO che continua lo stile del documento (intestazioni
/// `##`), non più testo semplice. Il pannello del canale, invece, mostra un
/// riassunto numerico deterministico (generato da `stock_report` stesso,
/// PRIMA che questo prompt entri in gioco) e NON la risposta di questo
/// prompt: l'AI può scrivere con calma, senza il vincolo "massimo poche
/// righe" che valeva quando il proprio testo finiva nel pannello.
const FINANCIAL_MARKETS_SYSTEM_PROMPT: &str = "Sei l'assistente del canale Financial Markets di Lare Terminal. Hai cinque strumenti: search_ticker (risolve un nome societario nel suo ticker USA — usalo PRIMA di stock_report se l'utente scrive un nome invece di un ticker; se restituisce più risultati ambigui, chiedi all'utente quale), stock_report (report fattuale completo: fondamentale, grafici a 5 timeframe, option chain, tabelle comparative — invocalo con il ticker risolto), list_stocks (elenco tabellare dei titoli conosciuti — Nome, Asset, Paese, Indice di appartenenza — filtrabile per paese; solo 'USA' è supportato oggi), list_screeners (elenca gli screener disponibili in questo canale e apre una finestra di selezione — usalo quando l'utente chiede di eseguire uno screener SENZA nominarne uno specifico, o quando il nome che usa non corrisponde con sicurezza a un id noto) e run_screener (esegue lo screener identificato da screener_id, con selezione e metriche proprie di quello screener — usalo quando riconosci con sicurezza quale screener l'utente vuole; vedi la descrizione del tool per gli id validi oggi, e NON indovinare se non sei sicuro). Quando chiami stock_report, un riassunto numerico appare SUBITO nel pannello del canale (non lo ripetere), e la finestra Markdown completa (documento fattuale + la TUA risposta fusa in coda) si apre automaticamente non appena finisci di scrivere, con un pulsante per salvarla — non devi mai ripetere i dati fattuali che ricevi, ti servono solo per il tuo ragionamento. La tua risposta dopo stock_report è ESCLUSIVAMENTE le due sezioni '## Narrativa di trend' (un commento sul trend di lungo periodo guardando i dati ricevuti) e '## Ipotesi di investimento' (elementi a favore e contrari per l'apertura di una posizione, su due orizzonti separati: breve termine entro 2 mesi e medio termine entro 6 mesi, ciascuno con un punteggio da -5 a +5 dove negativo è ribassista, 0 è neutro, positivo è rialzista — chiudi con un breve avviso che questa è un'analisi di fattori, non una raccomandazione operativa), in Markdown vero con intestazioni ##. Quando chiami list_stocks, la finestra si apre SUBITO: non ripetere la tabella, conferma solo quanti titoli sono stati trovati. Quando chiami list_screeners, la finestra di selezione si apre SUBITO: non elencare di nuovo gli screener in chat, conferma solo quanti sono disponibili. Quando chiami run_screener, la finestra si apre a fine turno con la TUA sezione fusa in coda: la risposta del tool include, oltre ai dati, le istruzioni di giudizio finale SPECIFICHE di quello screener — la tua risposta dopo run_screener è ESCLUSIVAMENTE quanto richiesto da quelle istruzioni, in Markdown vero con intestazioni ##. Non inventare mai dati fattuali oltre quelli forniti dagli strumenti. Rispondi in italiano.";

/// Titolo della finestra Markdown deterministica per `stock_report` —
/// derivato dall'INPUT della chiamata (`ticker`), non dalla risposta: stesso
/// schema di `format_nmap_invocation`/il titolo di `NmapToolClient::call_
/// scan_tool` (costruito da `target`, un parametro della chiamata).
fn stock_report_title(input: &serde_json::Value) -> String {
    let ticker = input.get("ticker").and_then(|v| v.as_str()).unwrap_or("?");
    format!("Financial Markets — {ticker}")
}

/// Titolo della finestra Markdown deterministica per `list_stocks` — a
/// differenza di `stock_report_title` (un ticker preciso), qui l'unico
/// parametro rilevante della chiamata è `country`.
fn list_stocks_title(input: &serde_json::Value) -> String {
    // Il titolo compare PRIMA che il documento sia pronto: normalizzato in
    // maiuscolo anche qui, stessa normalizzazione già applicata lato Python
    // in stock_list.build_rows/server.list_stocks per la tabella e il titolo
    // del documento — l'AI/utente può passare "usa" in qualunque casing.
    let country = input.get("country").and_then(|v| v.as_str()).unwrap_or("USA").to_uppercase();
    format!("Financial Markets — Elenco titoli ({country})")
}

/// Fallback per `run_screener` — usato SOLO se Python non manda "title"
/// nel JSON (`PythonReportJson.title`, difensivo): il registry Python è
/// autoritativo sul nome vero (titolo dello screener eseguito), questa fn
/// esiste solo per non lasciare la finestra senza titolo in caso di bug
/// lato Python. Docs/superpowers/specs/2026-08-11-markets-screener-registry-design.md.
fn run_screener_title_fallback(input: &serde_json::Value) -> String {
    let id = input.get("screener_id").and_then(|v| v.as_str()).unwrap_or("?");
    format!("Financial Markets — Screener ({id})")
}

/// Una voce del registro: un tool esterno con la propria finestra dedicata.
pub struct ExternalToolChannel {
    /// Combacia con `ClientMsg::Hello.channel` (es. `"nmap"`).
    pub id: &'static str,
    /// Slash command che apre la finestra dedicata a questo canale (es. `"/nmap"`).
    /// Obbligatorio ma può restare inerte: un canale senza innesco da cursore
    /// (es. `"library-expand"`) lo valorizza comunque, senza registrarlo in
    /// `external-channels.js` (vedi il commento sulla voce `"library-expand"`
    /// più sotto).
    pub slash_trigger: &'static str,
    /// Titolo della finestra dedicata.
    pub window_title: &'static str,
    /// Factory del `ToolClient` per questo canale. Sincrona e fallibile —
    /// mirror di `McpToolClient::resolve(config_dir, cfg) -> Self`: la
    /// costruzione vera e propria (spawn del sidecar) resta lazy dentro le
    /// singole chiamate ai tool, non qui. Il `ToolClient` restituito guida
    /// ANCHE quali tool l'AI vede (`tool_defs()`) e come li dispaccia
    /// (`dispatch()`) — Docs/superpowers/specs/2026-07-16-tool-isolation-design.md.
    ///
    /// Firma (2.0, Task 4, D6): riceve `&RuntimeConfig` (config_dir +
    /// `startup.json`, risolti una volta in `main()` — MAI ri-derivati qui,
    /// niente più `LARE_MCP_NMAP`/`LARE_PYTOOLS_DIR`) e `&Arc<dyn ToolClient>`
    /// (il `default_tools` condiviso del cursore, per un ipotetico canale
    /// futuro che voglia delegargli/avvolgerlo invece di costruirne uno
    /// nuovo da zero — nessun canale odierno lo usa, ma la factory resta un
    /// puntatore a funzione semplice, non una closure che catturi stato, per
    /// poter restare dentro un array `const`). Un canale che non ne ha
    /// bisogno li ignora entrambi (`|_rt, _default| ...`).
    // Tipo "complesso" per clippy, stesso compromesso già accettato sul tipo
    // di ritorno di `resolve_channel_tools` sotto: è il contratto d'interfaccia
    // di questo campo, un `type` alias sposterebbe solo il problema.
    #[allow(clippy::type_complexity)]
    pub tool_client: fn(&crate::runtime_config::RuntimeConfig, &Arc<dyn ToolClient>) -> anyhow::Result<Arc<dyn ToolClient>>,
    /// Formattazione della trasparenza/banner di conferma per questo canale.
    /// `None` → fallback su `agent::display_invocation` (comportamento di sempre).
    pub format_invocation: Option<fn(&str, &serde_json::Value) -> String>,
    /// Override del system prompt per l'AI di questo canale. `None` →
    /// `agent::SYSTEM_PROMPT` di default (comportamento di sempre). Un canale
    /// reale (mcp-nmap) lo userà per dire all'AI ESATTAMENTE quali tool ha —
    /// mai `run_in_session`/`open_target`, che il suo `tool_client` non offre.
    pub system_prompt_override: Option<&'static str>,
}

/// Registro statico dei canali esterni. Prima voce reale: `"nmap"` (design
/// spec `Docs/superpowers/specs/2026-07-16-mcp-nmap-design.md`). Un canale
/// futuro aggiunge una voce qui, senza toccare nessun'altra riga di questo file.
pub const EXTERNAL_TOOL_CHANNELS: &[ExternalToolChannel] = &[
    ExternalToolChannel {
        id: "nmap",
        slash_trigger: "/nmap",
        window_title: "Lare — nmap",
        tool_client: |rt, _default| {
            Ok(std::sync::Arc::new(crate::nmap_tool_client::NmapToolClient::resolve(&rt.config_dir, &rt.startup))
                as std::sync::Arc<dyn ToolClient>)
        },
        format_invocation: Some(format_nmap_invocation),
        system_prompt_override: Some(NMAP_SYSTEM_PROMPT),
    },
    ExternalToolChannel {
        id: "library-expand",
        // Nessun innesco da slash cursore per questo canale (deciso in
        // brainstorming — la finestra si apre SOLO dal pulsante "Espandi"
        // di un documento Library già aperto, vedi Task 4). Il campo è
        // obbligatorio nello struct ma resta inerte: non viene registrato
        // nel mirror frontend `external-channels.js`, quindi digitare
        // questo trigger nel cursore non produce alcun effetto.
        slash_trigger: "/library-expand",
        window_title: "Lare — Documento",
        tool_client: |_rt, _default| Ok(std::sync::Arc::new(EmptyToolClient) as std::sync::Arc<dyn ToolClient>),
        format_invocation: None,
        system_prompt_override: Some(LIBRARY_EXPAND_SYSTEM_PROMPT),
    },
    ExternalToolChannel {
        id: "python-ping",
        slash_trigger: "/pyping",
        window_title: "Lare — Python ping",
        tool_client: |rt, _default| {
            crate::python_mcp_tool_client::PythonMcpToolClient::resolve(
                &rt.config_dir,
                &rt.startup,
                "python-ping",
                "server.py",
                vec![crate::python_mcp_tool_client::PythonToolSpec {
                    def: ToolDef {
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
            .map(|c| std::sync::Arc::new(c) as std::sync::Arc<dyn ToolClient>)
        },
        format_invocation: None,
        system_prompt_override: Some(PYTHON_PING_SYSTEM_PROMPT),
    },
    ExternalToolChannel {
        id: "financial-markets",
        slash_trigger: "/markets",
        window_title: "Lare — Financial Markets",
        tool_client: |rt, _default| {
            crate::python_mcp_tool_client::PythonMcpToolClient::resolve(
                &rt.config_dir,
                &rt.startup,
                "financial-markets",
                "server.py",
                vec![
                    crate::python_mcp_tool_client::PythonToolSpec {
                        def: ToolDef {
                            name: "search_ticker".to_string(),
                            description: "Risolve un nome societario nel ticker USA corrispondente, via cache locale.".to_string(),
                            input_schema: serde_json::json!({
                                "type": "object",
                                "properties": { "query": { "type": "string", "description": "Nome societario o ticker (anche parziale)" } },
                                "required": ["query"]
                            }),
                        },
                        // Nessuna finestra: il canale è già pensato per un
                        // riassunto breve (poche corrispondenze) in testo
                        // semplice, non serve una finestra Markdown a parte.
                        report_title: None,
                        defer_report_to_turn_end: false,
                    },
                    crate::python_mcp_tool_client::PythonToolSpec {
                        def: ToolDef {
                            name: "stock_report".to_string(),
                            description: "Report fattuale completo (fondamentale, grafici 5 timeframe, option chain, comparative) per un ticker USA.".to_string(),
                            input_schema: serde_json::json!({
                                "type": "object",
                                "properties": { "ticker": { "type": "string", "description": "Ticker USA (es. AAPL, NVO)" } },
                                "required": ["ticker"]
                            }),
                        },
                        // Documento con grafici → finestra Markdown dedicata
                        // (deterministica, mai una scelta dell'AI) + Salva in
                        // Library, come nmap. Richiede che server.py segua il
                        // contratto JSON {summary, report_markdown} — vedi
                        // Docs/superpowers/specs/2026-07-22-financial-markets-
                        // stock-report-design.md.
                        report_title: Some(stock_report_title),
                        defer_report_to_turn_end: true,
                    },
                    crate::python_mcp_tool_client::PythonToolSpec {
                        def: ToolDef {
                            name: "list_stocks".to_string(),
                            description: "Elenco tabellare dei titoli conosciuti (Nome, Asset, Paese, Indice), filtrato per paese — solo 'USA' supportato oggi.".to_string(),
                            input_schema: serde_json::json!({
                                "type": "object",
                                "properties": { "country": { "type": "string", "description": "Paese da filtrare (solo 'USA' oggi); ometti per tutti i paesi supportati" } },
                                "required": []
                            }),
                        },
                        // Documento interamente deterministico (nessuna
                        // narrativa AI da attendere) → apertura immediata,
                        // a differenza di stock_report.
                        report_title: Some(list_stocks_title),
                        defer_report_to_turn_end: false,
                    },
                    crate::python_mcp_tool_client::PythonToolSpec {
                        def: ToolDef {
                            name: "list_screeners".to_string(),
                            description: "Elenca gli screener disponibili in questo canale (id, titolo, descrizione) e apre una finestra di selezione per l'utente. Usalo quando l'utente chiede di eseguire uno screener SENZA nominarne uno specifico, o quando il nome che usa non corrisponde con sicurezza a un id noto.".to_string(),
                            input_schema: serde_json::json!({
                                "type": "object",
                                "properties": {},
                                "required": []
                            }),
                        },
                        // Nessuna finestra Markdown: apre il picker (gestito
                        // per nome in ai_adapter.rs, non da report_title —
                        // vedi Docs/superpowers/specs/2026-08-11-markets-
                        // screener-registry-design.md).
                        report_title: None,
                        defer_report_to_turn_end: false,
                    },
                    crate::python_mcp_tool_client::PythonToolSpec {
                        def: ToolDef {
                            name: "run_screener".to_string(),
                            description: "Esegue lo screener identificato da screener_id, con selezione/metriche proprie di quello screener. screener_id validi oggi: 'consumer-usage' (Uso Consumer — screening 'potenziale inespresso' aziende consumer USA, esclude chi è già apparso negli ultimi 30 giorni); 'goldman-sachs' (Goldman Sachs — report equity research stile analista senior, top titoli (default 25), quota minima 30% tech, preferenza prezzo sotto 80-100$, nessuna esclusione recente); 'jensen-huang' (Jensen Huang — fornitori infrastruttura AI in ipercrescita, filtro industry, bonus legame Nvidia, nessuna esclusione recente); 'citadel' (Citadel — analisi tecnica quant, doppio regime momentum/mean-reversion deciso dal trend di ciascun titolo, tetto 2 titoli per settore, nessuna esclusione recente). Se non sei sicuro dell'id corretto per la richiesta dell'utente, chiama prima list_screeners invece di indovinare. La risposta include, in coda, le istruzioni di giudizio finale specifiche di quello screener: la tua risposta successiva deve seguirle alla lettera.".to_string(),
                            input_schema: serde_json::json!({
                                "type": "object",
                                "properties": {
                                    "screener_id": { "type": "string", "description": "Id dello screener da eseguire (vedi l'elenco in questa descrizione, o chiama list_screeners)" },
                                    "top": { "type": "integer", "description": "Quanti titoli selezionare (5-50, default 25) — significato specifico dipende dallo screener" }
                                },
                                "required": ["screener_id"]
                            }),
                        },
                        // Il documento include la sezione di giudizio finale
                        // (nome/criteri specifici dello screener eseguito,
                        // vedi il "summary" ritornato da Python) scritta
                        // dall'AI → finestra a fine turno, come stock_report.
                        report_title: Some(run_screener_title_fallback),
                        defer_report_to_turn_end: true,
                    },
                ],
                // 300s (era 180): oltre ai fino a 60 ticker via yfinance (~1-2s l'uno) dello
                // screening (list_stocks/run_screener), lo screener goldman-sachs aggiunge
                // un arricchimento POST-selezione (enrich_selection) con fino a ~25 fetch
                // fundamentals sui vincitori + fino a ~25 fetch P/E sui peer di settore,
                // ciascuno 2 chiamate yfinance (.info + .financials) — nel caso peggiore
                // quasi raddoppia il carico della singola chiamata tool. Innocuo per gli
                // altri tool del canale (list_stocks/stock_report/list_screeners restano
                // ben sotto il vecchio limite).
                300,
            )
            .map(|c| std::sync::Arc::new(c) as std::sync::Arc<dyn ToolClient>)
        },
        format_invocation: None,
        system_prompt_override: Some(FINANCIAL_MARKETS_SYSTEM_PROMPT),
    },
    ExternalToolChannel {
        id: "config-market-data-test",
        // Nessun innesco da slash cursore -- questa connessione si apre solo
        // dal bottone "Test connessione" nel tab /config Dati Mercato (Task
        // 12), mai digitando qualcosa nel cursore. Campo obbligatorio nello
        // struct ma resta inerte, stesso trattamento di "library-expand".
        slash_trigger: "/config-market-data-test",
        window_title: "Lare — Test fonte dati mercato",
        tool_client: |_rt, _default| Ok(std::sync::Arc::new(EmptyToolClient) as std::sync::Arc<dyn ToolClient>),
        format_invocation: None,
        system_prompt_override: None,
    },
];

/// Risolve il `ToolClient`/`format_invocation` da usare per QUESTA connessione,
/// in base al canale richiesto nell'handshake.
///
/// - `channel: None` → `default_tools` invariato (stessa istanza, `Arc::clone`),
///   nessun `format_invocation` — comportamento di sempre (cursore/Telegram).
/// - `channel: Some(id)` trovato in `registry` → costruisce il `ToolClient` del
///   canale (via `tool_client(rt, default_tools)`) e ritorna il suo `format_invocation`.
/// - `channel: Some(id)` non trovato, o costruzione fallita → `Err` con un
///   messaggio leggibile (mai un panic); il chiamante lo trasforma in
///   `ServerMsg::Error` e chiude la connessione.
///
/// `rt` (2.0, Task 4, D6): `RuntimeConfig` risolto una volta in `main()`,
/// inoltrato qui SOLO per passarlo a `tool_client()` — mai riletto/ricalcolato.
// Il tipo di ritorno è "complesso" per clippy, ma è il contratto d'interfaccia
// dichiarato nello spec (Task 3 in `ws.rs` chiama questa funzione con questa
// esatta firma): introdurre un `type` alias qui sposterebbe solo il problema
// senza semplificare nulla per il chiamante.
#[allow(clippy::type_complexity)]
pub fn resolve_channel_tools(
    channel: Option<&str>,
    registry: &[ExternalToolChannel],
    default_tools: &Arc<dyn ToolClient>,
    rt: &crate::runtime_config::RuntimeConfig,
) -> Result<(Arc<dyn ToolClient>, Option<fn(&str, &serde_json::Value) -> String>, Option<&'static str>), String> {
    let Some(id) = channel else {
        return Ok((Arc::clone(default_tools), None, None));
    };
    match registry.iter().find(|c| c.id == id) {
        None => Err(format!("canale sconosciuto: {id}")),
        Some(c) => (c.tool_client)(rt, default_tools)
            .map(|tc| (tc, c.format_invocation, c.system_prompt_override))
            .map_err(|e| format!("canale \"{id}\" non disponibile: {e}")),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests — TDD: RED poi GREEN
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_client::FakeToolClient;

    /// `RuntimeConfig` di comodo per i test di questo modulo: `StartupConfig::
    /// default()` + una tempdir vera (2.0, Task 4 — i canali nmap/python
    /// ricevono `&RuntimeConfig`, non più niente). La tempdir non è tenuta
    /// viva/ripulita: nessun test qui scrive file, solo `resolve()` che
    /// calcola path o (per i canali python) tenta `Path::exists()` — un
    /// fallimento tollerato, non un panic (vedi i test dedicati sotto).
    fn test_rt() -> crate::runtime_config::RuntimeConfig {
        let dir = tempfile::tempdir().unwrap().keep();
        crate::runtime_config::RuntimeConfig::for_test(&dir)
    }

    /// Secondo parametro placeholder per le factory `tool_client(rt, default)`
    /// chiamate direttamente (fuori da `resolve_channel_tools`) — nessuna
    /// factory di produzione lo usa oggi (vedi il doc-comment del campo
    /// `tool_client`), quindi un `FakeToolClient` qualunque basta.
    fn test_default_tools() -> Arc<dyn ToolClient> {
        Arc::new(FakeToolClient::success("unused"))
    }

    fn fixture_registry() -> Vec<ExternalToolChannel> {
        vec![ExternalToolChannel {
            id: "fixture",
            slash_trigger: "/fixture",
            window_title: "Fixture",
            tool_client: |_rt, _default| {
                Ok(Arc::new(FakeToolClient::success("CHANNEL_TOOLCLIENT_OUTPUT")) as Arc<dyn ToolClient>)
            },
            format_invocation: None,
            system_prompt_override: None,
        }]
    }

    #[test]
    fn channel_none_uses_default_tools_unchanged() {
        let default_tools: Arc<dyn ToolClient> = Arc::new(FakeToolClient::success("DEFAULT_TOOLCLIENT_OUTPUT"));
        let (tools, fmt, sys) = resolve_channel_tools(None, &[], &default_tools, &test_rt()).unwrap();
        assert!(
            Arc::ptr_eq(&tools, &default_tools),
            "channel:None deve riusare la stessa istanza, non ricostruirla"
        );
        assert!(fmt.is_none());
        assert!(sys.is_none());
    }

    #[test]
    fn unknown_channel_is_err_not_panic() {
        let default_tools: Arc<dyn ToolClient> = Arc::new(FakeToolClient::success("DEFAULT_TOOLCLIENT_OUTPUT"));
        // Nota: non `.unwrap_err()` — richiederebbe `Debug` sul tipo `Ok`
        // (Arc<dyn ToolClient>), che il trait non implementa. `match` esplicito
        // verifica lo stesso invariante ("è un Err", mai un panic) senza il bound.
        let err = match resolve_channel_tools(Some("nope"), &[], &default_tools, &test_rt()) {
            Err(e) => e,
            Ok(_) => panic!("atteso Err per un canale sconosciuto"),
        };
        assert!(err.contains("nope"), "il messaggio deve nominare il canale sconosciuto: {err}");
    }

    #[tokio::test]
    async fn known_channel_tool_client_produces_channel_output_not_default() {
        let default_tools: Arc<dyn ToolClient> = Arc::new(FakeToolClient::success("DEFAULT_TOOLCLIENT_OUTPUT"));
        let registry = fixture_registry();
        let (tools, _fmt, _sys) = resolve_channel_tools(Some("fixture"), &registry, &default_tools, &test_rt()).unwrap();
        let result = tools.run_in_session("qualsiasi", None).await;
        assert!(result.stdout.contains("CHANNEL_TOOLCLIENT_OUTPUT"));
        assert!(!result.stdout.contains("DEFAULT_TOOLCLIENT_OUTPUT"));
    }

    #[test]
    fn unregistered_channel_id_still_errs_against_the_real_registry() {
        // Rewritten (was `empty_production_registry_never_matches_anything`,
        // written when EXTERNAL_TOOL_CHANNELS was empty and "nmap" was the
        // example of an unregistered id — it now IS registered, so this test
        // uses a genuinely unregistered id instead, same principle.
        let default_tools: Arc<dyn ToolClient> = Arc::new(FakeToolClient::success("x"));
        assert!(resolve_channel_tools(Some("some-future-channel-not-yet-built"), EXTERNAL_TOOL_CHANNELS, &default_tools, &test_rt()).is_err());
    }

    #[test]
    fn known_channel_system_prompt_override_is_returned() {
        let default_tools: Arc<dyn ToolClient> = Arc::new(FakeToolClient::success("x"));
        let registry = vec![ExternalToolChannel {
            id: "fixture",
            slash_trigger: "/fixture",
            window_title: "Fixture",
            tool_client: |_rt, _default| Ok(Arc::new(FakeToolClient::success("x")) as Arc<dyn ToolClient>),
            format_invocation: None,
            system_prompt_override: Some("SEI IL CANALE FIXTURE"),
        }];
        let (_tools, _fmt, sys) = resolve_channel_tools(Some("fixture"), &registry, &default_tools, &test_rt()).unwrap();
        assert_eq!(sys, Some("SEI IL CANALE FIXTURE"));
    }

    #[test]
    fn production_registry_starts_with_nmap_channel() {
        assert!(!EXTERNAL_TOOL_CHANNELS.is_empty(), "registry deve avere almeno nmap come prima voce");
        assert_eq!(EXTERNAL_TOOL_CHANNELS[0].id, "nmap");
        assert_eq!(EXTERNAL_TOOL_CHANNELS[0].slash_trigger, "/nmap");
    }

    #[test]
    fn nmap_channel_tool_client_factory_constructs_without_panicking() {
        // resolve() itself must not panic (it only computes a path — the real
        // sidecar spawn is lazy, on first dispatch). Doesn't assert anything
        // about the returned client's behaviour (that's nmap_tool_client.rs's
        // own tests) — just that wiring this into the registry didn't break
        // the factory's error-free construction path.
        //
        // Adattamento rispetto al brief: `result.is_ok()` con `{result:?}` nel
        // messaggio non compila — `Arc<dyn ToolClient>` (variante Ok) non
        // implementa `Debug`, quindi nemmeno `Result<Arc<dyn ToolClient>, _>`
        // lo implementa (stesso vincolo già aggirato sopra in
        // `unknown_channel_is_err_not_panic` con un `match` esplicito).
        match (EXTERNAL_TOOL_CHANNELS[0].tool_client)(&test_rt(), &test_default_tools()) {
            Ok(_) => {}
            Err(e) => panic!("nmap ToolClient factory should not fail to construct: {e}"),
        }
    }

    #[test]
    fn production_registry_has_the_library_expand_channel() {
        assert_eq!(EXTERNAL_TOOL_CHANNELS.len(), 5);
        assert_eq!(EXTERNAL_TOOL_CHANNELS[1].id, "library-expand");
    }

    #[test]
    fn library_expand_tool_client_has_no_custom_tools() {
        let client = (EXTERNAL_TOOL_CHANNELS[1].tool_client)(&test_rt(), &test_default_tools()).expect("factory must not fail");
        assert!(
            client.tool_defs().is_empty(),
            "il canale library-expand non deve esporre alcun tool custom, solo web_search/web_fetch server-side"
        );
    }

    #[tokio::test]
    async fn library_expand_tool_client_rejects_run_in_session() {
        let client = (EXTERNAL_TOOL_CHANNELS[1].tool_client)(&test_rt(), &test_default_tools()).expect("factory must not fail");
        let result = client.run_in_session("qualsiasi comando", None).await;
        assert_eq!(result.exit_code, -1);
        assert!(result.stderr.contains("non disponibile su questo canale"));
    }

    #[tokio::test]
    async fn library_expand_tool_client_rejects_open_target() {
        let client = (EXTERNAL_TOOL_CHANNELS[1].tool_client)(&test_rt(), &test_default_tools()).expect("factory must not fail");
        let result = client.open_target("qualunque target").await;
        assert!(!result.ok);
        assert!(result.message.contains("non disponibile su questo canale"));
    }

    #[tokio::test]
    async fn library_expand_tool_client_dispatch_rejects_unknown_tool() {
        let client = (EXTERNAL_TOOL_CHANNELS[1].tool_client)(&test_rt(), &test_default_tools()).expect("factory must not fail");
        let outcome = client.dispatch("run_in_session", &serde_json::json!({})).await;
        assert!(outcome.is_error);
        assert!(outcome.output.contains("non disponibile su questo canale"));
    }

    #[test]
    fn library_expand_system_prompt_is_set() {
        let default_tools: Arc<dyn ToolClient> = Arc::new(crate::tool_client::FakeToolClient::success("x"));
        let (_tools, _fmt, sys) = resolve_channel_tools(Some("library-expand"), EXTERNAL_TOOL_CHANNELS, &default_tools, &test_rt()).unwrap();
        assert_eq!(sys, Some(LIBRARY_EXPAND_SYSTEM_PROMPT));
        assert!(sys.unwrap().contains("documento"), "il prompt deve nominare il documento");
    }

    #[test]
    fn production_registry_has_the_python_ping_channel() {
        assert_eq!(EXTERNAL_TOOL_CHANNELS.len(), 5);
        assert_eq!(EXTERNAL_TOOL_CHANNELS[2].id, "python-ping");
        assert_eq!(EXTERNAL_TOOL_CHANNELS[2].slash_trigger, "/pyping");
    }

    #[test]
    fn python_ping_system_prompt_constant_is_set() {
        // Verifica diretta sul campo statico, NON tramite resolve_channel_tools:
        // il factory di python-ping può fallire se il venv non esiste ancora
        // sulla macchina di sviluppo (a differenza di nmap/library-expand, la cui
        // resolve() non fallisce mai) — un test che passa dall'esito del
        // costruttore sarebbe ambientalmente fragile.
        assert_eq!(EXTERNAL_TOOL_CHANNELS[2].system_prompt_override, Some(PYTHON_PING_SYSTEM_PROMPT));
        assert!(PYTHON_PING_SYSTEM_PROMPT.contains("pyping"));
    }

    #[test]
    fn python_ping_channel_tool_client_factory_does_not_panic() {
        // Tollera Err (venv assente in questo momento è uno stato valido, non un
        // bug) — verifica solo che costruire il client non vada in panic.
        match (EXTERNAL_TOOL_CHANNELS[2].tool_client)(&test_rt(), &test_default_tools()) {
            Ok(_) | Err(_) => {}
        }
    }

    #[test]
    fn production_registry_has_the_financial_markets_channel() {
        assert_eq!(EXTERNAL_TOOL_CHANNELS.len(), 5);
        assert_eq!(EXTERNAL_TOOL_CHANNELS[3].id, "financial-markets");
        assert_eq!(EXTERNAL_TOOL_CHANNELS[3].slash_trigger, "/markets");
    }

    #[test]
    fn financial_markets_system_prompt_names_all_five_tools() {
        assert_eq!(EXTERNAL_TOOL_CHANNELS[3].system_prompt_override, Some(FINANCIAL_MARKETS_SYSTEM_PROMPT));
        assert!(FINANCIAL_MARKETS_SYSTEM_PROMPT.contains("search_ticker"));
        assert!(FINANCIAL_MARKETS_SYSTEM_PROMPT.contains("stock_report"));
        assert!(FINANCIAL_MARKETS_SYSTEM_PROMPT.contains("list_stocks"));
        assert!(FINANCIAL_MARKETS_SYSTEM_PROMPT.contains("list_screeners"));
        assert!(FINANCIAL_MARKETS_SYSTEM_PROMPT.contains("run_screener"));
    }

    #[test]
    fn financial_markets_channel_exposes_five_tools() {
        let default_tools: Arc<dyn ToolClient> = Arc::new(crate::tool_client::FakeToolClient::success("x"));
        match resolve_channel_tools(Some("financial-markets"), EXTERNAL_TOOL_CHANNELS, &default_tools, &test_rt()) {
            Ok((tools, _fmt, _sys)) => {
                let defs = tools.tool_defs();
                assert_eq!(defs.len(), 5, "atteso search_ticker + stock_report + list_stocks + list_screeners + run_screener: {defs:?}");
                assert!(defs.iter().any(|d| d.name == "list_screeners"));
                assert!(defs.iter().any(|d| d.name == "run_screener"));
            }
            Err(_) => {
                // Venv assente in questo momento è uno stato valido (vedi
                // financial_markets_channel_tool_client_factory_does_not_panic
                // sopra) — nessuna asserzione possibile senza client reale, ma MAI
                // un panic su una macchina senza venv creato a mano.
            }
        }
    }

    #[test]
    fn run_screener_title_fallback_reads_screener_id_from_input() {
        assert_eq!(
            run_screener_title_fallback(&serde_json::json!({"screener_id": "consumer-usage"})),
            "Financial Markets — Screener (consumer-usage)"
        );
        assert_eq!(run_screener_title_fallback(&serde_json::json!({})), "Financial Markets — Screener (?)");
    }

    #[test]
    fn financial_markets_channel_tool_client_factory_does_not_panic() {
        // Tollera Err (venv assente in questo momento è uno stato valido).
        match (EXTERNAL_TOOL_CHANNELS[3].tool_client)(&test_rt(), &test_default_tools()) {
            Ok(_) | Err(_) => {}
        }
    }

    #[test]
    fn list_stocks_title_uppercases_a_lowercase_country() {
        // La finestra si apre PRIMA che il documento sia pronto: il titolo
        // (derivato solo dall'input, mai dalla risposta) deve mostrare "USA"
        // anche se l'AI ha chiamato il tool con un casing diverso ("usa") —
        // stessa normalizzazione applicata lato Python in stock_list.build_rows.
        let input = serde_json::json!({ "country": "usa" });
        assert_eq!(list_stocks_title(&input), "Financial Markets — Elenco titoli (USA)");
    }

    #[test]
    fn list_stocks_title_defaults_to_usa_when_country_is_missing() {
        let input = serde_json::json!({});
        assert_eq!(list_stocks_title(&input), "Financial Markets — Elenco titoli (USA)");
    }

    // Fix A (review-fix-wave, 2026-08-14): il bottone "Test connessione" del
    // tab /config Dati Mercato apre un LareWsClient con
    // `channel: "config-market-data-test"` (config-dialog.js) — prima di
    // questo fix quell'id non era registrato qui, quindi `ws.rs` rifiutava
    // l'handshake con un Error PRIMA che l'arm `ClientMsg::TestMarketDataSource`
    // potesse mai essere raggiunto. Mirror ESATTO dei test già esistenti per
    // "library-expand" (stesso pattern: nessun tool custom, nessun innesco da
    // cursore).
    #[test]
    fn production_registry_has_the_config_market_data_test_channel() {
        assert_eq!(EXTERNAL_TOOL_CHANNELS.len(), 5);
        assert_eq!(EXTERNAL_TOOL_CHANNELS[4].id, "config-market-data-test");
        assert_eq!(EXTERNAL_TOOL_CHANNELS[4].slash_trigger, "/config-market-data-test");
    }

    #[test]
    fn config_market_data_test_channel_resolves_ok_not_err() {
        // Prima del fix, un canale non registrato ritorna Err da
        // resolve_channel_tools (vedi unregistered_channel_id_still_errs_...
        // sopra) — questo è il mirror positivo: l'handshake deve superare
        // resolve_channel_tools con Ok, esattamente come "library-expand".
        let default_tools: Arc<dyn ToolClient> = Arc::new(FakeToolClient::success("x"));
        assert!(resolve_channel_tools(Some("config-market-data-test"), EXTERNAL_TOOL_CHANNELS, &default_tools, &test_rt()).is_ok());
    }

    #[test]
    fn config_market_data_test_tool_client_has_no_custom_tools() {
        let client = (EXTERNAL_TOOL_CHANNELS[4].tool_client)(&test_rt(), &test_default_tools()).expect("factory must not fail");
        assert!(
            client.tool_defs().is_empty(),
            "il canale config-market-data-test non deve esporre alcun tool custom"
        );
    }
}
