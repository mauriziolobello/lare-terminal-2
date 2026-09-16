//! # agent — helper per il loop di tool-use (Slice 2)
//!
//! Funzioni pure/dispatch usate da `LlmAdapter::respond`: system prompt,
//! definizioni dei tool, troncamento char-safe del `tool_result`, etichette di
//! trasparenza, e dispatch di un `tool_use` sul `ToolClient`.

use crate::messages_client::{ServerTool, ToolDef, ToolSpec};
use crate::tool_client::ToolClient;

/// Round-trip massimi del loop (safety rail col no-gate).
pub const MAX_ITERATIONS: usize = 8;
/// Budget di troncamento del `tool_result` rimandato al modello.
pub const TRUNCATE_HEAD: usize = 6144;
pub const TRUNCATE_TAIL: usize = 2048;

/// Direttive di lingua per la risposta dell'AI.
pub const RESPOND_ITALIAN: &str = " Rispondi in italiano, in modo conciso.";
pub const RESPOND_ENGLISH: &str = " Answer in English, concisely.";
pub const RESPOND_SPANISH: &str = " Responde en español, de forma concisa.";

/// Base del system prompt dell'assistente terminale (senza la direttiva di lingua finale).
pub const BASE_SYSTEM_PROMPT: &str = "Sei l'assistente di Lare Terminal, un terminale sulla macchina dell'utente (OS Windows, shell PowerShell persistente: cwd ed env persistono tra i comandi). Hai i seguenti strumenti: run_in_session (esegui un comando nella shell persistente), open_target (apri URL, cartella o file con l'app di default), show_markdown (mostra contenuto Markdown ricco — spiegazioni lunghe, codice, tabelle — in una finestra dedicata), search_routines (cerca fra le routine PowerShell gia' salvate, per nome/descrizione/tag) e run_routine (esegui per nome una routine gia' salvata, nella stessa shell persistente). Prima di scrivere un comando nuovo con run_in_session per una richiesta operativa (file, disco, rete, sistema), controlla SEMPRE con search_routines se esiste gia' una routine salvata pertinente anche solo per tema (es. 'file grandi' e una routine sulla dimensione dei file sono lo stesso tema): se c'e' una routine adatta usa run_routine invece di riscrivere il comando da zero. Scrivi un comando nuovo solo se nessuna routine esistente e' pertinente. Hai anche get_routine_content (leggi il corpo di una routine salvata, sola lettura) e save_routine (salva un nuovo script come routine riusabile, o aggiornane una esistente passando replace). Prima di chiamare save_routine per una routine NUOVA, controlla SEMPRE con search_routines se esiste gia' qualcosa di simile per nome o tema: se si', leggi il contenuto con get_routine_content e decidi se riusare quella esistente, aggiornarla (save_routine con replace) o crearne una distinta con un nome diverso. Chiama save_routine SOLO dopo aver gia' testato lo script con run_in_session e verificato che funzioni: mai salvare uno script mai eseguito. Preferisci ESEGUIRE i comandi invece di spiegare come farli: se la richiesta e' fattibile via shell, usa run_in_session. Quando la risposta e' formattata o lunga (spiegazioni, codice, tabelle), usa show_markdown invece di scriverla come testo semplice. Per mostrare il contenuto di un file o un output lungo usa show_markdown e non ripetere lo stesso contenuto anche come testo. Evita comandi interattivi o a esecuzione prolungata (REPL come python o node, editor come vim o nano): bloccherebbero la sessione.";

/// System prompt dell'assistente terminale (default in italiano, per retro-compatibilità).
pub const SYSTEM_PROMPT: &str = "Sei l'assistente di Lare Terminal, un terminale sulla macchina dell'utente (OS Windows, shell PowerShell persistente: cwd ed env persistono tra i comandi). Hai i seguenti strumenti: run_in_session (esegui un comando nella shell persistente), open_target (apri URL, cartella o file con l'app di default), show_markdown (mostra contenuto Markdown ricco — spiegazioni lunghe, codice, tabelle — in una finestra dedicata), search_routines (cerca fra le routine PowerShell gia' salvate, per nome/descrizione/tag) e run_routine (esegui per nome una routine gia' salvata, nella stessa shell persistente). Prima di scrivere un comando nuovo con run_in_session per una richiesta operativa (file, disco, rete, sistema), controlla SEMPRE con search_routines se esiste gia' una routine salvata pertinente anche solo per tema (es. 'file grandi' e una routine sulla dimensione dei file sono lo stesso tema): se c'e' una routine adatta usa run_routine invece di riscrivere il comando da zero. Scrivi un comando nuovo solo se nessuna routine esistente e' pertinente. Hai anche get_routine_content (leggi il corpo di una routine salvata, sola lettura) e save_routine (salva un nuovo script come routine riusabile, o aggiornane una esistente passando replace). Prima di chiamare save_routine per una routine NUOVA, controlla SEMPRE con search_routines se esiste gia' qualcosa di simile per nome o tema: se si', leggi il contenuto con get_routine_content e decidi se riusare quella esistente, aggiornarla (save_routine con replace) o crearne una distinta con un nome diverso. Chiama save_routine SOLO dopo aver gia' testato lo script con run_in_session e verificato che funzioni: mai salvare uno script mai eseguito. Preferisci ESEGUIRE i comandi invece di spiegare come farli: se la richiesta e' fattibile via shell, usa run_in_session. Quando la risposta e' formattata o lunga (spiegazioni, codice, tabelle), usa show_markdown invece di scriverla come testo semplice. Per mostrare il contenuto di un file o un output lungo usa show_markdown e non ripetere lo stesso contenuto anche come testo. Evita comandi interattivi o a esecuzione prolungata (REPL come python o node, editor come vim o nano): bloccherebbero la sessione. Rispondi in italiano, in modo conciso.";

/// Addendum al system prompt quando la ricerca web interna è attiva: dice all'AI
/// che ha i tool server-side web e che deve USARLI per rispondere, non limitarsi
/// ad aprire il browser. (Senza questo, il modello ignora `web_search` e ripiega
/// su `open_target`.)
const WEB_SEARCH_ADDENDUM: &str = " Hai inoltre due strumenti per il web: web_search (cerca sul web) e web_fetch (scarica e leggi una pagina). USALI per RISPONDERE a domande che richiedono informazioni aggiornate o dal web (prezzi, meteo, notizie, eventi, fatti recenti): cerca e fornisci la risposta direttamente nel terminale citando la fonte, NON limitarti ad aprire il browser. Usa open_target per aprire una pagina solo se l'utente chiede esplicitamente di aprirla o navigarla.";

/// System prompt per il turno: override del canale (se presente) → altrimenti
/// base + addendum web se la ricerca web è attiva + direttiva lingua.
pub fn system_prompt(opts: TurnOptions) -> String {
    if let Some(overridden) = opts.system_prompt_override {
        return overridden.to_string();
    }
    let lang_directive = match opts.lang.as_deref() {
        Some("en") => RESPOND_ENGLISH,
        Some("es") => RESPOND_SPANISH,
        _ => RESPOND_ITALIAN,
    };
    if opts.web_search {
        format!("{BASE_SYSTEM_PROMPT}{WEB_SEARCH_ADDENDUM}{lang_directive}")
    } else {
        format!("{BASE_SYSTEM_PROMPT}{lang_directive}")
    }
}

/// Flag per-turno che modellano la richiesta all'AI.
#[derive(Debug, Clone, PartialEq)]
// `format_invocation` è un fn-pointer: rustc segnala che il confronto `==` su
// puntatori a funzione non è garantito stabile (indirizzi possono coincidere
// dopo merge del codegen). In pratica `TurnOptions` non viene mai confrontato
// con `==`/`assert_eq!` in questo crate (solo costruito e letto per campo) —
// lint conservativo, non un bug: soppresso qui, scoped a questo solo derive.
#[allow(unpredictable_function_pointer_comparisons)]
pub struct TurnOptions {
    /// false in modalità `/nowin`: niente `show_markdown` (e il loop rifiuta le finestre).
    pub allow_windows: bool,
    /// true se la ricerca web interna è autorizzata per questo turno.
    pub web_search: bool,
    /// Override del formatter di trasparenza/banner-di-conferma per questo
    /// turno (canale tool esterno — Docs/superpowers/specs/2026-07-16-
    /// external-tool-channel-design.md §5). `None` (cursore/Telegram) →
    /// `agent::display_invocation` invariato.
    pub format_invocation: Option<fn(&str, &serde_json::Value) -> String>,
    /// Override completo del system prompt per questo turno (canale tool
    /// esterno — Docs/superpowers/specs/2026-07-16-tool-isolation-design.md).
    /// `None` (cursore/Telegram) → `SYSTEM_PROMPT` (+ addendum web se
    /// `web_search`), invariato. `Some(text)` → `text` sostituisce
    /// INTERAMENTE il prompt di default (addendum web escluso: i canali
    /// esterni non hanno `web_search` in v1).
    pub system_prompt_override: Option<&'static str>,
    /// Lingua della risposta AI richiesta dal client (es. "it", "en").
    /// `None`, vuoto o "it" → italiano (comportamento invariato); "en" → inglese.
    pub lang: Option<String>,
}

impl Default for TurnOptions {
    fn default() -> Self {
        Self {
            allow_windows: true,
            web_search: false,
            format_invocation: None,
            system_prompt_override: None,
            lang: None,
        }
    }
}

/// Tool server-side per la ricerca web (cap `max_uses` per turno).
fn web_tools() -> [ServerTool; 2] {
    [
        ServerTool { kind: "web_search_20260209".to_string(), name: "web_search".to_string(), max_uses: Some(5) },
        ServerTool { kind: "web_fetch_20260209".to_string(),  name: "web_fetch".to_string(),  max_uses: Some(5) },
    ]
}

/// Tool della richiesta per un turno: quelli del `ToolClient` attivo (`defs`,
/// tipicamente `tools.tool_defs()`), meno `show_markdown` se `!allow_windows`,
/// più tool web server-side se `web_search`. `defs` è un parametro esplicito
/// (non più preso da `tool_defs()` internamente) così un `ToolClient` di
/// canale può sostituire l'intero set — Docs/superpowers/specs/2026-07-16-
/// tool-isolation-design.md.
pub fn tools_for(opts: TurnOptions, defs: Vec<ToolDef>) -> Vec<ToolSpec> {
    let mut v: Vec<ToolSpec> = defs
        .into_iter()
        .filter(|t| opts.allow_windows || t.name != "show_markdown")
        .map(ToolSpec::Custom)
        .collect();
    if opts.web_search {
        for st in web_tools() {
            v.push(ToolSpec::Server(st));
        }
    }
    v
}

/// Definizioni dei tool esposti all'AI (Slice 2).
pub fn tool_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "run_in_session".to_string(),
            description: "Esegui un comando nella shell PowerShell persistente (cwd ed env persistono). Usa per qualsiasi operazione fattibile da terminale.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "command": { "type": "string", "description": "Il comando da eseguire" } },
                "required": ["command"]
            }),
        },
        ToolDef {
            name: "open_target".to_string(),
            description: "Apri un URL, una cartella o un file con l'applicazione predefinita del sistema.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "target": { "type": "string", "description": "URL, percorso cartella o file" } },
                "required": ["target"]
            }),
        },
        ToolDef {
            name: "show_markdown".to_string(),
            description: "Mostra contenuto Markdown ricco (spiegazioni lunghe, codice, tabelle) in una finestra dedicata. Usalo quando la risposta e' formattata o lunga invece di scriverla come testo semplice. Puoi chiamarlo più volte nello stesso turno via via che affini la risposta (es. dopo ricerche web successive): ogni chiamata AGGIORNA la stessa finestra con la versione più recente, non ne apre una seconda. IMPORTANTE: ogni chiamata, anche una bozza intermedia, deve contenere SOLO dati che hai realmente cercato e verificato (es. con web_search) — mai segnaposto, esempi inventati o cifre non confermate presentate come se fossero reali. Se non hai ancora fatto la ricerca, fallo PRIMA di chiamare questo tool.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "content": { "type": "string", "description": "Il contenuto Markdown da mostrare" },
                    "title": { "type": "string", "description": "Titolo opzionale della finestra" }
                },
                "required": ["content"]
            }),
        },
        ToolDef {
            name: "search_routines".to_string(),
            description: "Cerca routine PowerShell salvate per nome, descrizione o tag (case-insensitive). Ometti query per elencarle tutte. Sola lettura.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "query": { "type": "string", "description": "Testo di ricerca (opzionale)" } },
                "required": []
            }),
        },
        ToolDef {
            name: "run_routine".to_string(),
            description: "Esegui una routine PowerShell salvata per nome, nella shell persistente (stessa cwd/env di run_in_session). Passa args SOLO se l'utente nomina esplicitamente una cartella o un parametro specifico nella richiesta; altrimenti ometti args e la routine usera' la cwd corrente.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Nome esatto della routine, come restituito da search_routines" },
                    "args": { "type": "string", "description": "Argomenti opzionali passati alla routine, verbatim" }
                },
                "required": ["name"]
            }),
        },
        ToolDef {
            name: "get_routine_content".to_string(),
            description: "Leggi il corpo completo di una routine PowerShell salvata, per nome esatto. Sola lettura. Usalo PRIMA di save_routine quando search_routines mostra un nome uguale o simile a quello che vuoi salvare, per confrontare i due script e decidere se riusare, aggiornare (save_routine con replace) o creare una routine distinta.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "name": { "type": "string", "description": "Nome esatto della routine, come restituito da search_routines" } },
                "required": ["name"]
            }),
        },
        ToolDef {
            name: "save_routine".to_string(),
            description: "Salva un nuovo script PowerShell come routine riusabile, o aggiorna/rinomina una routine esistente (passa replace col nome esatto di quella da sostituire). IMPORTANTE: testa SEMPRE lo script con run_in_session e verifica che funzioni PRIMA di chiamare questo tool. Prima di creare una routine nuova, verifica SEMPRE con search_routines se esiste gia' qualcosa di simile; se si', leggi il contenuto con get_routine_content e decidi se riusare, aggiornare (replace) o creare un nome distinto. Richiede conferma esplicita dell'utente in una finestra dedicata prima che qualunque cosa venga scritta su disco.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Nome della routine (solo lettere minuscole, cifre, trattini)" },
                    "description": { "type": "string", "description": "Descrizione breve di cosa fa la routine" },
                    "tags": { "type": "array", "items": { "type": "string" }, "description": "Tag per la ricerca futura" },
                    "category": { "type": "string", "description": "Categoria libera (es. 'files', 'network')" },
                    "content": { "type": "string", "description": "Il corpo completo dello script PowerShell, gia' testato con run_in_session" },
                    "replace": { "type": "string", "description": "Nome esatto di una routine esistente da sostituire/aggiornare (omettilo per salvare una routine nuova e distinta)" }
                },
                "required": ["name", "description", "tags", "category", "content"]
            }),
        },
        ToolDef {
            name: "set_ai_display_name".to_string(),
            description: "Imposta il TUO nome (nickname) usato in AI Chat, indipendente dal nome dell'utente umano. Usalo la prima volta che ti viene chiesto di sceglierti un nome, o se l'utente ti dice esplicitamente un nome da usare. Sovrascrive un nome precedente se già presente.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Il nome scelto (o assegnato dall'utente), testo libero" }
                },
                "required": ["name"]
            }),
        },
    ]
}

/// Tronca `s` (char-safe) per limitare contesto/costo: testa + coda + marcatore.
pub fn truncate_for_model(s: &str) -> String {
    let budget = TRUNCATE_HEAD + TRUNCATE_TAIL;
    if s.len() <= budget {
        return s.to_string();
    }
    let head_end = floor_boundary(s, TRUNCATE_HEAD);
    let tail_start = ceil_boundary(s, s.len() - TRUNCATE_TAIL);
    let omitted = tail_start.saturating_sub(head_end);
    format!(
        "{}\n[\u{2026}troncato {omitted} byte\u{2026}]\n{}",
        &s[..head_end],
        &s[tail_start..]
    )
}

/// Il più grande confine di `char` ≤ `idx`.
fn floor_boundary(s: &str, mut idx: usize) -> usize {
    if idx >= s.len() {
        return s.len();
    }
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

/// Il più piccolo confine di `char` ≥ `idx`.
fn ceil_boundary(s: &str, mut idx: usize) -> usize {
    while idx < s.len() && !s.is_char_boundary(idx) {
        idx += 1;
    }
    idx
}

/// Deriva `(titolo, contenuto)` per una finestra Markdown da un `tool_use`
/// `show_markdown`. Titolo: `input.title` (≤60 char) → prima riga non vuota di
/// `content` (≤60, char-safe) → fallback `"Lare — Output"`.
pub fn markdown_window(input: &serde_json::Value) -> (String, String) {
    let content = input
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let title = input
        .get("title")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|t| t.chars().take(60).collect::<String>())
        .or_else(|| {
            content
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty())
                .map(|l| l.chars().take(60).collect::<String>())
        })
        .unwrap_or_else(|| "Lare \u{2014} Output".to_string());
    (title, content)
}

/// Etichetta leggibile dell'invocazione, per il `Chunk` di trasparenza.
pub fn display_invocation(name: &str, input: &serde_json::Value) -> String {
    match name {
        "run_in_session" => {
            let cmd = input.get("command").and_then(|v| v.as_str()).unwrap_or("");
            // `interactive` (2.0, spec §4.5): il prompt `[Y/n]` deve dichiarare che
            // l'output non verrà catturato — l'utente decide sapendolo.
            if input.get("interactive").and_then(|v| v.as_bool()).unwrap_or(false) {
                format!("$ {cmd}  (interattivo)")
            } else {
                format!("$ {cmd}")
            }
        }
        "open_target" => format!(
            "apri: {}",
            input.get("target").and_then(|v| v.as_str()).unwrap_or("")
        ),
        "search_routines" => format!(
            "cerca routine: {}",
            input.get("query").and_then(|v| v.as_str()).unwrap_or("(tutte)")
        ),
        "run_routine" => format!(
            "routine: {}{}",
            input.get("name").and_then(|v| v.as_str()).unwrap_or(""),
            input
                .get("args")
                .and_then(|v| v.as_str())
                .map(|a| format!(" {a}"))
                .unwrap_or_default()
        ),
        "get_routine_content" => format!(
            "leggi routine: {}",
            input.get("name").and_then(|v| v.as_str()).unwrap_or("")
        ),
        "save_routine" => format!(
            "salva routine: {}{}",
            input.get("name").and_then(|v| v.as_str()).unwrap_or(""),
            input
                .get("replace")
                .and_then(|v| v.as_str())
                .map(|r| format!(" (sostituisce {r})"))
                .unwrap_or_default()
        ),
        "set_ai_display_name" => format!(
            "imposta nome AI: {}",
            input.get("name").and_then(|v| v.as_str()).unwrap_or("")
        ),
        other => format!("[tool {other}]"),
    }
}

/// Esegue un `tool_use` sul `ToolClient`. Ritorna un `DispatchOutcome`
/// (`report: None` sempre qui — solo un `ToolClient` di canale, es.
/// mcp-nmap, popola quel campo, direttamente nel proprio `dispatch()`
/// override, non tramite questa funzione libera).
///
/// `network_json_path` è passato ESPLICITAMENTE dal chiamante (2.0, Task 4
/// — D6): prima esisteva un wrapper `dispatch_tool` a 3 parametri che
/// ri-derivava il path da solo (`aichat::config::resolve_network_json_path`,
/// a sua volta basata su env var/`startup.json` letti di nuovo). Quel
/// ri-derivare è esattamente il pattern vietato dalla configurazione 2.0:
/// `config_dir` va risolto UNA volta in `main()` (vedi `RuntimeConfig`) e
/// portato a chi ne ha bisogno, mai ricalcolato altrove — un `--config-dir`
/// relativo ricalcolato dopo il cambio di cwd in `main()`
/// (`set_current_dir(home)`) risolverebbe in modo diverso dalla prima volta.
/// I chiamanti reali (`McpToolClient`/`CwdTrackingToolClient`) tengono
/// ormai il proprio `config_dir` come campo, ricevuto da `RuntimeConfig` a
/// costruzione, e passano `self.config_dir.join("network.json")` qui.
pub async fn dispatch_tool_at(
    tools: &dyn ToolClient,
    name: &str,
    input: &serde_json::Value,
    network_json_path: &std::path::Path,
) -> crate::tool_client::DispatchOutcome {
    use crate::tool_client::DispatchOutcome;
    match name {
        // Fix review finale (Important #1, `Docs/superpowers/plans/
        // 2026-08-13-aichat-display-names.md`): scrittura Value-based (mai
        // più un round-trip tramite `AiChatConfig`) — vedi `merge_ai_display_name`
        // sotto per il perché.
        //
        // NOTA restart-required: `AiChatService` legge `ai_display_name` UNA
        // SOLA volta all'avvio (v0.41.14, stesso pattern di `ai_participates`/
        // `ai_autoparticipate`), non "live". Questo tool scrive
        // `network.json` a runtime: il nickname appare in AI Chat solo dopo
        // il prossimo riavvio dell'orchestrator (debito accettato, vedi
        // memoria `aichat-config-restart-policy` — non risolto da questo
        // task; il messaggio di successo sotto lo dice esplicitamente,
        // fix minor della stessa review).
        "set_ai_display_name" => {
            let raw = input.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return DispatchOutcome {
                    output: "Nome vuoto: non salvato.".to_string(),
                    is_error: true,
                    report: None,
                    channel_summary: None,
                };
            }
            if trimmed.chars().count() > 48 {
                return DispatchOutcome {
                    output: "Nome troppo lungo (massimo 48 caratteri): non salvato.".to_string(),
                    is_error: true,
                    report: None,
                    channel_summary: None,
                };
            }
            // Stringa vuota se il file è assente (`unwrap_or_default` su un
            // `Err` di lettura) — `merge_ai_display_name` distingue da lì in
            // poi "davvero assente/vuoto" (riparte da zero) da "corrotto"
            // (errore esplicito, nessuna scrittura).
            let existing = std::fs::read_to_string(network_json_path).unwrap_or_default();
            match merge_ai_display_name(&existing, trimmed) {
                Ok(merged) => match std::fs::write(network_json_path, merged) {
                    Ok(()) => DispatchOutcome {
                        output: format!(
                            "Nome impostato: {trimmed} (visibile in AI Chat dopo il riavvio dell'orchestrator)."
                        ),
                        is_error: false,
                        report: None,
                        channel_summary: None,
                    },
                    Err(e) => DispatchOutcome {
                        output: format!("network.json: scrittura fallita ({e})"),
                        is_error: true,
                        report: None,
                        channel_summary: None,
                    },
                },
                Err(msg) => DispatchOutcome { output: msg, is_error: true, report: None, channel_summary: None },
            }
        }
        "run_in_session" => {
            let command = input.get("command").and_then(|v| v.as_str()).unwrap_or("");
            // AI tool-use path: no streaming (the AI adapter collects the full
            // result before continuing the conversation loop).
            let r = tools.run_in_session(command, None).await;
            let mut out = String::new();
            if !r.stdout.is_empty() {
                out.push_str(&r.stdout);
            }
            if !r.stderr.is_empty() {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(&r.stderr);
            }
            if out.is_empty() {
                out = format!("(nessun output, exit_code={})", r.exit_code);
            }
            DispatchOutcome { output: out, is_error: r.exit_code != 0, report: None, channel_summary: None }
        }
        "open_target" => {
            let target = input.get("target").and_then(|v| v.as_str()).unwrap_or("");
            let r = tools.open_target(target).await;
            DispatchOutcome { output: r.message, is_error: !r.ok, report: None, channel_summary: None }
        }
        "search_routines" => {
            let query = input.get("query").and_then(|v| v.as_str());
            let r = tools.search_routines(query).await;
            if let Some(err) = r.error {
                return DispatchOutcome { output: err, is_error: true, report: None, channel_summary: None };
            }
            if r.results.is_empty() {
                return DispatchOutcome { output: "Nessuna routine trovata.".to_string(), is_error: false, report: None, channel_summary: None };
            }
            let listing = r
                .results
                .iter()
                .map(|e| format!("- {} ({}): {} [tag: {}]", e.name, e.category, e.description, e.tags.join(", ")))
                .collect::<Vec<_>>()
                .join("\n");
            DispatchOutcome { output: listing, is_error: false, report: None, channel_summary: None }
        }
        "run_routine" => {
            let name = input.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let args = input.get("args").and_then(|v| v.as_str());
            let r = tools.run_routine(name, args).await;
            if !r.ok {
                return DispatchOutcome { output: r.message, is_error: true, report: None, channel_summary: None };
            }
            let mut out = String::new();
            if !r.stdout.is_empty() {
                out.push_str(&r.stdout);
            }
            if !r.stderr.is_empty() {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(&r.stderr);
            }
            if out.is_empty() {
                out = format!("(nessun output, exit_code={})", r.exit_code);
            }
            DispatchOutcome { output: out, is_error: r.exit_code != 0, report: None, channel_summary: None }
        }
        "get_routine_content" => {
            let name = input.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let r = tools.get_routine_content(name).await;
            if let Some(err) = r.error {
                return DispatchOutcome { output: err, is_error: true, report: None, channel_summary: None };
            }
            if !r.found {
                return DispatchOutcome {
                    output: format!("Routine '{name}' non trovata."),
                    is_error: false, report: None, channel_summary: None,
                };
            }
            DispatchOutcome {
                output: format!(
                    "Nome: {}\nDescrizione: {}\nCategoria: {}\nTag: {}\n\n{}",
                    r.name, r.description, r.category, r.tags.join(", "), r.content
                ),
                is_error: false, report: None, channel_summary: None,
            }
        }
        "save_routine" => {
            let name = input.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let description = input.get("description").and_then(|v| v.as_str()).unwrap_or("");
            let tags: Vec<String> = input
                .get("tags")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|t| t.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            let category = input.get("category").and_then(|v| v.as_str()).unwrap_or("");
            let content = input.get("content").and_then(|v| v.as_str()).unwrap_or("");
            let replace = input.get("replace").and_then(|v| v.as_str());
            let r = tools.save_routine(name, description, tags, category, content, replace).await;
            if r.ok {
                DispatchOutcome {
                    output: format!("Routine '{name}' salvata."),
                    is_error: false, report: None, channel_summary: None,
                }
            } else {
                DispatchOutcome { output: r.message, is_error: true, report: None, channel_summary: None }
            }
        }
        other => DispatchOutcome { output: format!("tool sconosciuto: {other}"), is_error: true, report: None, channel_summary: None },
    }
}

/// Scrive `ai_display_name` nel JSON grezzo di `network.json` (`existing`),
/// preservando OGNI altra chiave presente — stesso principio del gemello
/// lato `ui`, `aichat_settings::merge_aichat_settings`
/// (`crates/ui/src-tauri/src/aichat_settings.rs`): leggi come
/// `serde_json::Value`, `insert` SOLO la chiave di propria competenza, MAI
/// un round-trip attraverso il tipo tipizzato `AiChatConfig`. Quel
/// round-trip (il comportamento di questa funzione PRIMA del fix — review
/// finale, Important #1) aveva due problemi: (1) `network.json` è
/// esplicitamente identità di RETE condivisa da più feature (il nome del
/// file lo dichiara — vedi `config.rs`), non solo AI Chat: un round-trip
/// tipizzato a 7 campi cancellerebbe silenziosamente qualunque chiave
/// ignota scritta da un'altra feature futura; (2) `load_or_generate` su un
/// file CORROTTO rigenera in silenzio i default (incluso `enabled: false`,
/// che spegne l'intero servizio AI Chat) e li scrive su disco — un "nome
/// impostato con successo" avrebbe mascherato quel reset.
///
/// A differenza di `merge_aichat_settings` (che riparte SEMPRE da `{}` su
/// QUALUNQUE fallimento di parsing, anche per un file con contenuto reale
/// ma malformato), qui un file NON VUOTO che fallisce il parse è un
/// `Err` esplicito — nessuna scrittura, l'utente/l'AI vede l'errore invece
/// di un "successo" silenzioso su un file diverso da quello che credeva.
/// Solo un file DAVVERO assente o vuoto riparte da zero — seminato da
/// `AiChatConfig::default()` e non da un oggetto `{}` vuoto: a differenza
/// di `ai_participates`/`ai_autoparticipate`/`display_name`/`ai_display_name`,
/// i campi `enabled`/`label_base`/`chat_port` NON hanno `#[serde(default)]`
/// — un file con la sola chiave `ai_display_name` romperebbe la prossima
/// lettura via `AiChatConfig` altrove nel processo (`main.rs`,
/// `ai_adapter::needs_ai_name_prompt_at`).
fn merge_ai_display_name(existing: &str, name: &str) -> Result<String, String> {
    let mut value: serde_json::Value = if existing.trim().is_empty() {
        serde_json::to_value(crate::aichat::config::AiChatConfig::default())
            .expect("AiChatConfig è sempre serializzabile (nessun campo non-JSON)")
    } else {
        serde_json::from_str(existing).map_err(|e| {
            format!(
                "network.json: JSON corrotto, nome NON salvato per non perdere il contenuto esistente ({e})"
            )
        })?
    };
    let Some(obj) = value.as_object_mut() else {
        return Err("network.json: il contenuto non è un oggetto JSON, nome NON salvato".to_string());
    };
    obj.insert("ai_display_name".to_string(), serde_json::json!(name));
    serde_json::to_string_pretty(&value).map_err(|e| format!("network.json: serializzazione fallita ({e})"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_client::FakeToolClient;

    #[test]
    fn tools_for_default_has_eight_custom() {
        let t = tools_for(TurnOptions::default(), tool_defs());
        // 8 tool custom (era 7 prima di set_ai_display_name, Task 7).
        assert_eq!(t.len(), 8);
        assert!(t.iter().any(|s| s.name() == "show_markdown"));
        assert!(t.iter().any(|s| s.name() == "search_routines"));
        assert!(t.iter().any(|s| s.name() == "run_routine"));
        assert!(t.iter().any(|s| s.name() == "get_routine_content"));
        assert!(t.iter().any(|s| s.name() == "save_routine"));
        assert!(t.iter().all(|s| matches!(s, ToolSpec::Custom(_))));
    }

    #[test]
    fn tool_defs_includes_set_ai_display_name() {
        let defs = tool_defs();
        assert!(defs.iter().any(|d| d.name == "set_ai_display_name"));
    }

    /// Questa feature (Docs/i18n/ita/compiti-ai-esterne/2026-09-15-show-markdown-
    /// update-in-place.md) è il fix vero della regressione trovata l'8/9: al
    /// posto dell'istruzione "una sola volta" (mitigazione temporanea), la
    /// descrizione ora istruisce il modello che chiamate multiple sono
    /// benvenute e aggiornano la stessa finestra invece di aprirne di nuove.
    #[test]
    fn show_markdown_description_allows_multiple_calls_per_turn_to_update() {
        let defs = tool_defs();
        let show_markdown = defs
            .iter()
            .find(|d| d.name == "show_markdown")
            .expect("show_markdown deve esistere fra i tool");
        let lower = show_markdown.description.to_lowercase();
        assert!(
            !lower.contains("una sola volta")
                && !lower.contains("al massimo una volta")
                && !lower.contains("una volta sola"),
            "la descrizione di show_markdown NON deve più dire 'una sola volta': {:?}",
            show_markdown.description
        );
        assert!(
            lower.contains("aggiorna"),
            "la descrizione di show_markdown deve dire che aggiorna la stessa finestra: {:?}",
            show_markdown.description
        );
        assert!(
            lower.contains("più volte") || lower.contains("piu' volte"),
            "la descrizione di show_markdown deve permettere chiamate multiple: {:?}",
            show_markdown.description
        );
    }

    /// Regressione osservata dal vivo (2026-09-16, Maurizio): con l'update-in-place,
    /// il modello ha mostrato più bozze intermedie con dati INVENTATI ("ho generato
    /// quella tabella senza prima eseguire una ricerca web reale"), scusandosi e
    /// ricominciando 4 volte prima di usare davvero `web_search`. La libertà di
    /// chiamare `show_markdown` più volte non deve tradursi in libertà di mostrare
    /// contenuto non verificato come se fosse definitivo — la description deve
    /// dirlo esplicitamente, non lasciarlo implicito.
    #[test]
    fn show_markdown_description_forbids_unverified_placeholder_data() {
        let defs = tool_defs();
        let show_markdown = defs
            .iter()
            .find(|d| d.name == "show_markdown")
            .expect("show_markdown deve esistere fra i tool");
        let lower = show_markdown.description.to_lowercase();
        assert!(
            lower.contains("verificat"), // "verificati"/"verificata"/"verificato"
            "la descrizione deve richiedere dati verificati (cercati davvero): {:?}",
            show_markdown.description
        );
        assert!(
            lower.contains("mai") && (lower.contains("invent") || lower.contains("segnapost")),
            "la descrizione deve vietare esplicitamente dati inventati/segnaposto: {:?}",
            show_markdown.description
        );
    }

    #[test]
    fn tools_for_no_windows_excludes_show_markdown() {
        let t = tools_for(
            TurnOptions { allow_windows: false, web_search: false, format_invocation: None, system_prompt_override: None, lang: None },
            tool_defs(),
        );
        // 8 tool custom - show_markdown (era 7 prima di set_ai_display_name, Task 7).
        assert_eq!(t.len(), 7);
        assert!(!t.iter().any(|s| s.name() == "show_markdown"));
    }

    #[test]
    fn tools_for_web_search_adds_two_server_tools() {
        let t = tools_for(
            TurnOptions { allow_windows: true, web_search: true, format_invocation: None, system_prompt_override: None, lang: None },
            tool_defs(),
        );
        // 8 tool custom (era 7 prima di set_ai_display_name, Task 7) + 2 web.
        assert_eq!(t.len(), 10);
        assert!(t.iter().any(|s| s.name() == "web_search"));
        assert!(t.iter().any(|s| s.name() == "web_fetch"));
    }

    #[test]
    fn tools_for_uses_explicit_defs_not_hardcoded_three() {
        // Un ToolClient di canale passa i PROPRI defs (tools.tool_defs()), non
        // tool_defs(): tools_for deve usarli così come sono, senza ricadere
        // sui 3 storici.
        let custom_defs = vec![ToolDef {
            name: "custom_tool".to_string(),
            description: "d".to_string(),
            input_schema: serde_json::json!({}),
        }];
        let t = tools_for(TurnOptions::default(), custom_defs);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].name(), "custom_tool");
    }

    #[test]
    fn system_prompt_override_replaces_default_entirely() {
        let opts = TurnOptions {
            allow_windows: true,
            web_search: true,
            format_invocation: None,
            system_prompt_override: Some("PROMPT FISSO DEL CANALE"),
            lang: None,
        };
        let s = system_prompt(opts);
        assert_eq!(s, "PROMPT FISSO DEL CANALE");
        assert!(!s.contains("Lare Terminal"), "l'override deve sostituire il prompt di default, non affiancarlo");
        assert!(!s.contains("web_search"), "l'addendum web non si applica quando c'è un override");
    }

    #[test]
    fn system_prompt_mentions_web_tools_only_when_enabled() {
        // Senza ricerca web: il prompt NON deve parlare di web_search (altrimenti
        // l'AI cercherebbe un tool assente).
        let off = system_prompt(TurnOptions::default());
        assert!(!off.contains("web_search"), "prompt OFF non deve menzionare web_search: {off}");
        // Con ricerca web: il prompt DEVE dire all'AI di usare web_search/web_fetch
        // (senza questo addendum il modello ignora il tool e apre il browser).
        let on = system_prompt(TurnOptions { allow_windows: true, web_search: true, format_invocation: None, system_prompt_override: None, lang: None });
        assert!(on.contains("web_search") && on.contains("web_fetch"),
            "prompt ON deve menzionare i tool web: {on}");
    }

    #[test]
    fn system_prompt_respects_language_directive() {
        // lang: None -> italiano
        let def = system_prompt(TurnOptions::default());
        assert!(def.ends_with(RESPOND_ITALIAN));
        assert!(!def.contains(RESPOND_ENGLISH.trim()));

        // lang: Some("it") -> italiano
        let it = system_prompt(TurnOptions {
            lang: Some("it".into()),
            ..Default::default()
        });
        assert!(it.ends_with(RESPOND_ITALIAN));
        assert!(!it.contains(RESPOND_ENGLISH.trim()));

        // lang: Some("") -> italiano
        let empty = system_prompt(TurnOptions {
            lang: Some("".into()),
            ..Default::default()
        });
        assert!(empty.ends_with(RESPOND_ITALIAN));
        assert!(!empty.contains(RESPOND_ENGLISH.trim()));

        // lang: Some("en") -> english
        let en = system_prompt(TurnOptions {
            lang: Some("en".into()),
            ..Default::default()
        });
        assert!(en.ends_with(RESPOND_ENGLISH));
        assert!(!en.contains(RESPOND_ITALIAN.trim()));

        // Con web_search=true e lang: Some("en") -> web addendum presente e chiusura in english
        let en_web = system_prompt(TurnOptions {
            web_search: true,
            lang: Some("en".into()),
            ..Default::default()
        });
        assert!(en_web.contains("web_search") && en_web.contains("web_fetch"));
        // lang: Some("es") -> spanish
        let es = system_prompt(TurnOptions {
            lang: Some("es".into()),
            ..Default::default()
        });
        assert!(es.ends_with(RESPOND_SPANISH));
        assert!(!es.contains(RESPOND_ITALIAN.trim()));
        assert!(!es.contains(RESPOND_ENGLISH.trim()));

        // Con web_search=true e lang: Some("es") -> web addendum presente e chiusura in spanish
        let es_web = system_prompt(TurnOptions {
            web_search: true,
            lang: Some("es".into()),
            ..Default::default()
        });
        assert!(es_web.contains("web_search") && es_web.contains("web_fetch"));
        assert!(es_web.ends_with(RESPOND_SPANISH));
        assert!(!es_web.contains(RESPOND_ITALIAN.trim()));
        assert!(!es_web.contains(RESPOND_ENGLISH.trim()));

        // system_prompt_override resta intoccato anche se lang è valorizzato
        let overridden = system_prompt(TurnOptions {
            system_prompt_override: Some("custom prompt"),
            lang: Some("en".into()),
            ..Default::default()
        });
        assert_eq!(overridden, "custom prompt");
    }

    #[test]
    fn system_prompt_mentions_save_routine_rules() {
        let s = system_prompt(TurnOptions::default());
        assert!(s.contains("save_routine"));
        assert!(s.contains("get_routine_content"));
        assert!(s.contains("testato"), "deve richiedere il test prima del salvataggio: {s}");
    }

    #[test]
    fn markdown_window_uses_explicit_title() {
        let (title, content) =
            markdown_window(&serde_json::json!({"content":"# Corpo","title":"Mio Titolo"}));
        assert_eq!(title, "Mio Titolo");
        assert_eq!(content, "# Corpo");
    }

    #[test]
    fn markdown_window_derives_title_from_first_line() {
        let (title, _) = markdown_window(&serde_json::json!({"content":"# Primo\nresto"}));
        assert!(title.contains("Primo"));
    }

    #[test]
    fn markdown_window_falls_back_to_default_title() {
        let (title, _) = markdown_window(&serde_json::json!({"content":"\n\n"}));
        assert_eq!(title, "Lare \u{2014} Output");
    }

    #[test]
    fn markdown_window_truncates_title_to_60_chars() {
        let long = "T".repeat(100);
        let (title, _) =
            markdown_window(&serde_json::json!({"content": format!("# {long}"), "title": long}));
        assert!(title.chars().count() <= 60);
    }

    #[test]
    fn display_invocation_marks_interactive_run_in_session() {
        let plain = display_invocation("run_in_session", &serde_json::json!({"command": "vim x"}));
        assert_eq!(plain, "$ vim x");
        let inter = display_invocation(
            "run_in_session",
            &serde_json::json!({"command": "vim x", "interactive": true}),
        );
        assert_eq!(inter, "$ vim x  (interattivo)");
    }

    #[test]
    fn display_invocation_run_routine_shows_name_and_args() {
        let out = display_invocation("run_routine", &serde_json::json!({"name": "list-big-files", "args": "-Path C:\\Foo"}));
        assert_eq!(out, "routine: list-big-files -Path C:\\Foo");
    }

    #[test]
    fn display_invocation_run_routine_without_args_has_no_trailing_space() {
        let out = display_invocation("run_routine", &serde_json::json!({"name": "list-big-files"}));
        assert_eq!(out, "routine: list-big-files");
    }

    #[test]
    fn display_invocation_search_routines_shows_query_or_placeholder() {
        assert_eq!(
            display_invocation("search_routines", &serde_json::json!({"query": "disk"})),
            "cerca routine: disk"
        );
        assert_eq!(
            display_invocation("search_routines", &serde_json::json!({})),
            "cerca routine: (tutte)"
        );
    }

    #[test]
    fn display_invocation_get_routine_content_shows_name() {
        let out = display_invocation("get_routine_content", &serde_json::json!({"name": "list-big-files"}));
        assert_eq!(out, "leggi routine: list-big-files");
    }

    #[test]
    fn display_invocation_save_routine_shows_name_without_replace() {
        let out = display_invocation("save_routine", &serde_json::json!({"name": "new-routine"}));
        assert_eq!(out, "salva routine: new-routine");
    }

    #[test]
    fn display_invocation_save_routine_shows_replace_when_present() {
        let out = display_invocation(
            "save_routine",
            &serde_json::json!({"name": "new-routine", "replace": "old-routine"}),
        );
        assert_eq!(out, "salva routine: new-routine (sostituisce old-routine)");
    }

    #[test]
    fn display_invocation_set_ai_display_name_shows_name() {
        let out = display_invocation("set_ai_display_name", &serde_json::json!({"name": "Aria"}));
        assert_eq!(out, "imposta nome AI: Aria");
    }

    #[test]
    fn truncate_passthrough_when_within_budget() {
        assert_eq!(truncate_for_model("abc"), "abc");
    }

    #[test]
    fn truncate_long_input_has_head_tail_and_marker() {
        let s = "a".repeat(TRUNCATE_HEAD + TRUNCATE_TAIL + 1000);
        let out = truncate_for_model(&s);
        assert!(out.contains("troncato"));
        assert!(out.starts_with(&"a".repeat(100)));
        assert!(out.ends_with(&"a".repeat(100)));
        assert!(out.len() < s.len());
    }

    #[test]
    fn truncate_is_char_safe_on_mixed_width() {
        // 'x' (1 byte) + molti 'à' (2 byte) → il taglio a 6144 cade mid-char:
        // se non fosse char-safe, lo slicing panicherebbe.
        let s = format!("x{}", "à".repeat(TRUNCATE_HEAD + TRUNCATE_TAIL));
        let out = truncate_for_model(&s);
        assert!(out.contains("troncato"));
    }

    #[tokio::test]
    async fn dispatch_run_in_session_returns_stdout() {
        let tools = FakeToolClient::success("file.txt\n");
        let outcome = dispatch_tool_at(&tools, "run_in_session", &serde_json::json!({"command":"ls"}), std::path::Path::new("network.json")).await;
        assert!(outcome.output.contains("file.txt"));
        assert!(!outcome.is_error);
    }

    #[tokio::test]
    async fn dispatch_run_in_session_marks_error_on_nonzero_exit() {
        let tools = FakeToolClient::failure("boom", 1);
        let outcome = dispatch_tool_at(&tools, "run_in_session", &serde_json::json!({"command":"x"}), std::path::Path::new("network.json")).await;
        assert!(outcome.output.contains("boom"));
        assert!(outcome.is_error);
    }

    #[tokio::test]
    async fn dispatch_open_target_maps_result() {
        let tools = FakeToolClient::success("");
        let outcome = dispatch_tool_at(&tools, "open_target", &serde_json::json!({"target":"http://x"}), std::path::Path::new("network.json")).await;
        assert!(outcome.output.contains("http://x"));
        assert!(!outcome.is_error);
    }

    #[tokio::test]
    async fn dispatch_unknown_tool_is_error() {
        let tools = FakeToolClient::success("");
        let outcome = dispatch_tool_at(&tools, "nope", &serde_json::json!({}), std::path::Path::new("network.json")).await;
        assert!(outcome.output.contains("sconosciuto"));
        assert!(outcome.is_error);
    }

    #[tokio::test]
    async fn dispatch_get_routine_content_not_found() {
        // FakeToolClient's default get_routine_content is found:false/error:None (Task 4).
        let tools = FakeToolClient::success("");
        let outcome = dispatch_tool_at(&tools, "get_routine_content", &serde_json::json!({"name": "x"}), std::path::Path::new("network.json")).await;
        assert!(!outcome.is_error);
        assert!(outcome.output.contains("non trovata"));
    }

    #[tokio::test]
    async fn dispatch_save_routine_ok_reports_success() {
        // FakeToolClient's default save_routine is ok:true (Task 4).
        let tools = FakeToolClient::success("");
        let outcome = dispatch_tool_at(
            &tools, "save_routine",
            &serde_json::json!({"name": "n", "description": "d", "tags": [], "category": "c", "content": "x"}),
            std::path::Path::new("network.json"),
        ).await;
        assert!(!outcome.is_error);
        assert!(outcome.output.contains("salvata"));
    }

    #[tokio::test]
    async fn dispatch_set_ai_display_name_writes_network_json() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("network.json");
        let input = serde_json::json!({ "name": "Aria" });
        let outcome = dispatch_tool_at(&FakeToolClient::success(""), "set_ai_display_name", &input, &path).await;
        assert!(!outcome.is_error, "output: {}", outcome.output);
        let cfg: crate::aichat::config::AiChatConfig =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(cfg.ai_display_name, Some("Aria".to_string()));
    }

    #[tokio::test]
    async fn dispatch_set_ai_display_name_rejects_empty_name() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("network.json");
        let input = serde_json::json!({ "name": "   " });
        let outcome = dispatch_tool_at(&FakeToolClient::success(""), "set_ai_display_name", &input, &path).await;
        assert!(outcome.is_error);
        assert!(!path.exists(), "un nome vuoto non deve creare/toccare network.json");
    }

    // -------------------------------------------------------------------------
    // Fix review finale (Important #1): la scrittura di `set_ai_display_name`
    // ora legge/scrive `network.json` come `serde_json::Value` (stesso principio
    // di `aichat_settings::merge_aichat_settings`, ui), mai più un round-trip
    // completo attraverso `AiChatConfig` — che cancellerebbe silenziosamente
    // qualunque chiave ignota condivisa da un'altra feature, e su un file
    // CORROTTO rigenererebbe i default (incluso `enabled: false`) mascherando
    // uno spegnimento silenzioso da "nome impostato con successo".
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn dispatch_set_ai_display_name_preserves_unknown_keys_and_existing_values() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("network.json");
        // `peer_ttl_secs` è una chiave IGNOTA ad `AiChatConfig` — simula un
        // campo scritto da un'altra feature che condivide `network.json`
        // (il nome del file lo dichiara: identità di rete condivisa, non
        // solo AI Chat). `enabled`/`label_base` sono valori GIA' scelti
        // dall'utente, diversi dai default: un round-trip via AiChatConfig
        // non li cancellerebbe (li conosce), ma questo test dimostra che
        // NEANCHE la chiave ignota sparisce.
        std::fs::write(
            &path,
            r#"{"enabled":true,"label_base":"skimble","chat_port":40100,"peer_ttl_secs":30}"#,
        )
        .unwrap();
        let input = serde_json::json!({ "name": "Aria" });
        let outcome = dispatch_tool_at(&FakeToolClient::success(""), "set_ai_display_name", &input, &path).await;
        assert!(!outcome.is_error, "output: {}", outcome.output);
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["enabled"], true, "un valore esistente non deve essere resettato");
        assert_eq!(v["label_base"], "skimble");
        assert_eq!(v["peer_ttl_secs"], 30, "una chiave ignota ad AiChatConfig deve sopravvivere");
        assert_eq!(v["ai_display_name"], "Aria");
    }

    #[tokio::test]
    async fn dispatch_set_ai_display_name_rejects_corrupt_file_with_content() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("network.json");
        // Contenuto NON vuoto ma JSON malformato — un file corrotto, non un
        // file assente. Deve fallire esplicitamente, MAI essere silenziosamente
        // sostituito dai default (che spegnerebbero AI Chat, enabled:false,
        // dietro un messaggio di successo).
        let corrupt = "{ questo non e' json valido";
        std::fs::write(&path, corrupt).unwrap();
        let input = serde_json::json!({ "name": "Aria" });
        let outcome = dispatch_tool_at(&FakeToolClient::success(""), "set_ai_display_name", &input, &path).await;
        assert!(outcome.is_error, "un network.json corrotto deve fallire, non essere silenziosamente sovrascritto");
        let content_after = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content_after, corrupt, "il file corrotto non va toccato");
    }

    #[tokio::test]
    async fn dispatch_set_ai_display_name_seeds_full_default_when_file_absent() {
        // Un file DAVVERO assente riparte da `AiChatConfig::default()` (non
        // da un oggetto vuoto `{}`): gli altri campi della struct
        // (`enabled`/`label_base`/`chat_port`) NON hanno `#[serde(default)]`,
        // quindi un file con la sola chiave `ai_display_name` romperebbe la
        // prossima lettura via `AiChatConfig` altrove (`main.rs`,
        // `needs_ai_name_prompt_at`) — vedi anche
        // `dispatch_set_ai_display_name_writes_network_json` sopra, che verifica
        // lo stesso invariante con un round-trip completo attraverso il tipo.
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("network.json");
        let input = serde_json::json!({ "name": "Aria" });
        let outcome = dispatch_tool_at(&FakeToolClient::success(""), "set_ai_display_name", &input, &path).await;
        assert!(!outcome.is_error, "output: {}", outcome.output);
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["enabled"], false, "default AiChatConfig::default()");
        assert_eq!(v["label_base"], "lare", "default AiChatConfig::default()");
        assert_eq!(v["chat_port"], 40100, "default AiChatConfig::default()");
        assert_eq!(v["ai_display_name"], "Aria");
    }

    #[tokio::test]
    async fn dispatch_set_ai_display_name_success_message_mentions_restart_required() {
        // Fix minor review finale: il messaggio di successo deve dire
        // esplicitamente che il nickname compare in AI Chat solo dopo il
        // riavvio dell'orchestrator (network.json non è letto "live") —
        // altrimenti l'AI/l'utente si aspetta l'effetto immediato che non c'è.
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("network.json");
        let input = serde_json::json!({ "name": "Aria" });
        let outcome = dispatch_tool_at(&FakeToolClient::success(""), "set_ai_display_name", &input, &path).await;
        assert!(!outcome.is_error, "output: {}", outcome.output);
        assert!(
            outcome.output.contains("riavvio dell'orchestrator"),
            "il messaggio di successo deve avvisare del riavvio richiesto: {}",
            outcome.output
        );
    }
}
