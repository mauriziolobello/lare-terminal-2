//! # openrouter_backend — `ChatBackend` per provider OpenAI-compatibili (OpenRouter)
//!
//! A differenza di `claude_backend.rs` (quasi passthrough — `Block`/`ToolSpec` sono già
//! la forma wire Anthropic), qui la traduzione è sostanziale: OpenAI-compatibili hanno un
//! wire format diverso per tool-call/tool-result (vedi
//! `Docs/superpowers/specs/2026-07-06-openrouter-tooluse-adapter-design.md` §4). I tipi
//! `Or*` sotto sono stati validati contro una risposta REALE catturata da OpenRouter/
//! DeepSeek, non solo dedotti dalla documentazione — vedi
//! `Docs/superpowers/fixtures/2026-07-06-deepseek-openrouter-real-roundtrip/README.md`.

use async_trait::async_trait;

use std::time::Duration;

#[cfg(test)]
use std::{collections::VecDeque, sync::Mutex};
use std::sync::Arc;

use crate::chat_backend::{BackendError, BackendTurn, ChatBackend, TurnStop};
use crate::messages_client::{Block, Message, ToolSpec};

/// Un messaggio nell'array `messages` — usato SIA per costruire la richiesta SIA per
/// deserializzare `choices[].message` nella risposta (stessa forma in entrambe le
/// direzioni; la risposta porta anche `refusal`/`reasoning`, sempre `null` nei casi
/// osservati e non modellati qui: un campo JSON non dichiarato nello struct viene
/// scartato da serde di default, non serve `deny_unknown_fields`/gestione esplicita).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OrMessage {
    pub role: String,
    /// NIENTE `skip_serializing_if` qui (a differenza di `tool_calls`/`tool_call_id`
    /// sotto): la fixture reale (`turn2-request.json`) mostra che il messaggio
    /// `assistant` con `tool_calls` porta comunque `"content": null` ESPLICITO nel
    /// JSON inviato — non il campo assente. Se questo campo venisse omesso quando
    /// `None`, il nostro `OrRequest` serializzato non combacerebbe byte-per-byte con
    /// la richiesta realmente accettata da OpenRouter/DeepSeek (verificato dal test
    /// `or_request_turn2_serializes_exactly_like_the_captured_fixture`, che infatti
    /// fallisce se si aggiunge `skip_serializing_if` qui). `Option<String>` senza
    /// l'attributo serializza `None` come `null` di default — esattamente il
    /// comportamento richiesto.
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<OrToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// Una chiamata a tool, sia in un messaggio `assistant` storico (richiesta) sia nella
/// risposta del modello. La fixture reale mostra un campo extra `"index":0` SOLO in
/// risposta (mai richiesto in richiesta — la nostra `to_or_messages`, Task 2, non lo
/// produce, ed è stato accettato da OpenRouter/DeepSeek senza, vedi il README della
/// fixture): non è modellato qui, serde lo scarta silenziosamente in deserializzazione.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OrToolCall {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
    pub function: OrFunctionCall,
}

/// `arguments` è una STRINGA JSON (`"{\"command\":\"ls\"}"`), non un oggetto nativo —
/// confermato dalla fixture reale, non un'assunzione da documentazione.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OrFunctionCall {
    pub name: String,
    pub arguments: String,
}

/// Un tool esposto al modello, nell'array `tools` della richiesta.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct OrTool {
    #[serde(rename = "type")]
    pub kind: String,
    pub function: OrFunctionDef,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct OrFunctionDef {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Corpo della richiesta a `POST /chat/completions`. A differenza di `MessagesRequest`
/// (Anthropic, `messages_client.rs`) non c'è un campo `system` separato: il system
/// prompt entra nell'array `messages` come primo messaggio `role:"system"` (vedi
/// `to_or_messages`, Task 2).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct OrRequest {
    pub model: String,
    pub messages: Vec<OrMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<OrTool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct OrResponse {
    pub choices: Vec<OrChoice>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct OrChoice {
    pub finish_reason: String,
    pub message: OrMessage,
}

// ── Traduzione pura (nessun I/O) ──────────────────────────────────────────────

/// Traduce la storia (+ eventuale system prompt) in messaggi OpenAI-compatibili. Tre
/// casi, uno per ogni forma che la storia assume oggi in `ai_adapter.rs` (vedi
/// `Docs/superpowers/specs/2026-07-06-openrouter-tooluse-adapter-design.md` §4.2):
/// - `Message{role:"user", content:[Block::Text]}` → un messaggio `user` (`content`
///   stringa).
/// - `Message{role:"assistant", content:[Block::Text?, Block::ToolUse, ...]}` → UN
///   messaggio `assistant` (`content` testo o `None`, `tool_calls` uno per `ToolUse`).
/// - `Message{role:"user", content:[Block::ToolResult, ...]}` — il caso uno-a-molti — si
///   SROTOLA in N messaggi `role:"tool"` separati (OpenAI non ha un turno
///   multi-risultato). `is_error` non ha un campo dedicato: si ripiega in `content` con
///   prefisso `"[error] "`.
///
/// OpenAI-compatibile non ha un campo top-level `system` (a differenza di
/// `MessagesRequest::system` in `messages_client.rs`): se `system` non è vuoto, viene
/// anteposto come primo messaggio `role:"system"`.
pub fn to_or_messages(system: &str, history: &[Message]) -> Vec<OrMessage> {
    let mut out = Vec::new();
    if !system.is_empty() {
        out.push(OrMessage {
            role: "system".to_string(),
            content: Some(system.to_string()),
            tool_calls: None,
            tool_call_id: None,
        });
    }
    for msg in history {
        let tool_results: Vec<&Block> = msg
            .content
            .iter()
            .filter(|b| matches!(b, Block::ToolResult { .. }))
            .collect();
        if !tool_results.is_empty() {
            for b in tool_results {
                if let Block::ToolResult { tool_use_id, content, is_error } = b {
                    let content = if *is_error {
                        format!("[error] {content}")
                    } else {
                        content.clone()
                    };
                    out.push(OrMessage {
                        role: "tool".to_string(),
                        content: Some(content),
                        tool_calls: None,
                        tool_call_id: Some(tool_use_id.clone()),
                    });
                }
            }
            continue;
        }

        let text: String = msg
            .content
            .iter()
            .filter_map(|b| match b {
                Block::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("");
        let tool_calls: Vec<OrToolCall> = msg
            .content
            .iter()
            .filter_map(|b| match b {
                Block::ToolUse { id, name, input } => Some(OrToolCall {
                    kind: "function".to_string(),
                    id: id.clone(),
                    function: OrFunctionCall {
                        name: name.clone(),
                        arguments: serde_json::to_string(input)
                            .expect("un serde_json::Value serializza sempre"),
                    },
                }),
                _ => None,
            })
            .collect();

        out.push(OrMessage {
            role: msg.role.clone(),
            content: if text.is_empty() { None } else { Some(text) },
            tool_calls: if tool_calls.is_empty() { None } else { Some(tool_calls) },
            tool_call_id: None,
        });
    }
    out
}

/// Traduce i tool disponibili. Solo `ToolSpec::Custom` ha un equivalente OpenAI
/// (`{type:"function", function:{name,description,parameters}}` — `parameters` è lo
/// stesso `input_schema` JSON Schema, nessuna conversione semantica). `ToolSpec::Server`
/// (tool server-side Anthropic, es. `web_search`) non ha modo di essere eseguito da un
/// provider non-Anthropic: viene scartato con un log, non un errore (nessuna probing
/// statica di capacità — vedi spec §3.1/§5).
pub fn to_or_tools(tools: &[ToolSpec]) -> Vec<OrTool> {
    tools
        .iter()
        .filter_map(|t| match t {
            ToolSpec::Custom(def) => Some(OrTool {
                kind: "function".to_string(),
                function: OrFunctionDef {
                    name: def.name.clone(),
                    description: def.description.clone(),
                    parameters: def.input_schema.clone(),
                },
            }),
            ToolSpec::Server(s) => {
                tracing::warn!(
                    "OpenRouterBackend: tool server-side '{}' non supportato, scartato",
                    s.name
                );
                None
            }
        })
        .collect()
}

/// I 9 caratteri che, subito dopo un `\` DENTRO una stringa JSON, formano un escape
/// valido secondo la grammatica JSON (RFC 8259 §7): `\"` `\\` `\/` `\b` `\f` `\n` `\r`
/// `\t` `\uXXXX`. Qualunque altro carattere dopo un `\` è un errore di sintassi per
/// `serde_json` — è esattamente la classe di errore "invalid escape" che questo modulo
/// tenta di riparare.
const VALID_JSON_ESCAPE_CHARS: [char; 9] = ['"', '\\', '/', 'b', 'f', 'n', 'r', 't', 'u'];

/// Ripara — su base "miglior tentativo", non una garanzia — la classe di errore JSON
/// "invalid escape": un `\` seguito da un carattere che NON è uno dei 9 escape validi
/// (vedi `VALID_JSON_ESCAPE_CHARS`). Il caso che ha innescato questa funzione: un modello
/// via OpenRouter/DeepSeek scrive `arguments` (una stringa JSON che il MODELLO stesso
/// serializza — vedi il commento sopra `OrFunctionCall`) contenente un escape Markdown
/// letterale come `\*` senza raddoppiare il backslash come richiederebbe JSON grezzo.
/// `\*` diventa `\\*` (un backslash letterale seguito da un asterisco, quasi certamente
/// ciò che il modello intendeva).
///
/// **Perché non basta un `.replace()` globale (macchina a stati minima):** un `\*` fuori
/// da una stringa (es. dentro un numero o una parola chiave come `null`/`true`) non
/// esiste in JSON valido, ma il vero motivo per tracciare "dentro/fuori stringa" è
/// un altro: senza saperlo, non potremmo distinguere in modo affidabile un `"` che CHIUDE
/// una stringa da un `"` che ne fa parte (perché preceduto da un `\` di escape valido,
/// es. `\"`) — sbagliare questo porterebbe a "perdere il filo" (desync) sul resto del
/// documento. Per questo la funzione scorre carattere per carattere tenendo un flag
/// `in_string`, che si inverte SOLO su una `"` che non è stata già "consumata" come parte
/// di una coppia di escape valida.
///
/// **Cosa NON fa (di proposito):** non tocca backslash seguiti da un carattere di escape
/// VALIDO — anche quando quell'escape è quasi certamente un errore del modello in un
/// contesto diverso (es. `C:\temp` scritto senza raddoppiare il backslash produce `\t`,
/// che JSON interpreta come TAB, non come backslash-letterale-seguito-da-t). Quel caso
/// "passa" il parsing silenziosamente producendo un valore sbagliato (corruzione
/// silenziosa) invece di un errore di decodifica — una categoria di bug completamente
/// diversa da quella che questa funzione affronta, e fuori scope qui (vedi
/// `Docs/HANDOFF.md`/changelog per la nota di scope). Non inventa MAI caratteri mancanti
/// (es. una virgoletta di chiusura per una stringa troncata): se il JSON è rotto oltre un
/// backslash orfano, resta rotto dopo la riparazione — il chiamante deve gestire quel
/// fallimento (vedi `from_or_response`).
fn repair_invalid_json_escapes(input: &str) -> String {
    // `with_capacity(input.len())`: nel caso comune (nessuna riparazione necessaria) non
    // rialloca mai, perché l'output ha la stessa lunghezza dell'input; nel caso raro in
    // cui raddoppiamo un backslash l'output è più lungo, ma `push` gestisce comunque la
    // riallocazione automaticamente — è solo un'ottimizzazione del caso comune.
    let mut out = String::with_capacity(input.len());
    // `peekable()`: un iteratore di `char` a cui si può "sbirciare" (`peek`) il prossimo
    // elemento SENZA consumarlo — ci serve per decidere se raddoppiare un backslash
    // guardando il carattere seguente senza ancora "impegnarci" a consumarlo.
    let mut chars = input.chars().peekable();
    let mut in_string = false;

    while let Some(c) = chars.next() {
        if in_string && c == '\\' {
            match chars.peek() {
                Some(&next) if VALID_JSON_ESCAPE_CHARS.contains(&next) => {
                    // Escape già valido (es. `\n`, `\"`, `\\`...): lo ricopiamo COSÌ
                    // COM'È, byte per byte, e consumiamo esplicitamente `next` con
                    // `chars.next()` così il prossimo giro del `while` riparte dal
                    // carattere DOPO la coppia — è precisamente questo "consumo di
                    // coppia" che impedisce la desincronizzazione: se non lo facessimo,
                    // un `"` che fa parte di un `\"` valido verrebbe rivisto al giro
                    // successivo come se fosse una virgoletta "nuova" e ribalterebbe
                    // `in_string` per errore (vedi test
                    // `repair_does_not_desync_on_escaped_quote`).
                    out.push(c);
                    out.push(next);
                    chars.next();
                }
                Some(&next) => {
                    // Escape NON valido: il backslash era quasi certamente destinato a
                    // essere un backslash LETTERALE (non l'inizio di un escape) — lo
                    // raddoppiamo. `\*` (2 caratteri in input) diventa `\\*` (3
                    // caratteri in output: due backslash + l'asterisco originale).
                    out.push('\\');
                    out.push('\\');
                    out.push(next);
                    chars.next();
                }
                None => {
                    // Backslash orfano a fine stringa (nessun carattere successivo):
                    // non c'è nulla da ispezionare. Lo ricopiamo così com'è — il
                    // documento è comunque troncato/malformato, e il chiamante lo
                    // scoprirà dal fallimento del parsing successivo, non da questa
                    // funzione (che non inventa mai contenuto, vedi doc-comment sopra).
                    out.push(c);
                }
            }
            continue;
        }

        if c == '"' {
            // Una `"` raggiunta QUI (cioè non già consumata come parte di una coppia di
            // escape sopra) è per definizione un delimitatore REALE di stringa JSON —
            // apre una stringa se eravamo fuori, la chiude se eravamo dentro.
            in_string = !in_string;
        }
        out.push(c);
    }

    out
}

/// Traduce la risposta del modello in `BackendTurn`. `tool_calls[].function.arguments`
/// è una stringa JSON: un parsing fallito è un `BackendError::Decode` esplicito, MAI un
/// drop silenzioso del blocco. `finish_reason` → `TurnStop`: `stop`/`tool_calls` → il
/// loop distingue già in base alla presenza di `ToolUse` nei blocchi, non serve altro
/// che `Normal`; `length` → `MaxTokens`; `content_filter` → `Refused{category: None}`
/// (OpenAI non fornisce una categoria come Anthropic — `None` è onesto, non
/// un'invenzione); qualunque altro valore → `Normal` (nessuna probing di capacità:
/// meglio degradare a "turno normale" che rifiutare per un `finish_reason` non ancora
/// visto).
pub fn from_or_response(resp: OrResponse) -> Result<BackendTurn, BackendError> {
    let choice = resp.choices.into_iter().next().ok_or_else(|| {
        BackendError::Decode("nessuna choice nella risposta OpenRouter".to_string())
    })?;

    let mut blocks = Vec::new();
    if let Some(text) = choice.message.content {
        if !text.is_empty() {
            blocks.push(Block::Text { text });
        }
    }
    for tc in choice.message.tool_calls.unwrap_or_default() {
        // Percorso veloce: la stragrande maggioranza delle chiamate ha `arguments` già
        // valido (il modello serializza correttamente). Tentiamo prima questo, senza
        // pagare il costo della scansione di `repair_invalid_json_escapes` quando non
        // serve.
        let input = match serde_json::from_str(&tc.function.arguments) {
            Ok(v) => v,
            Err(original_err) => {
                // Fallito: proviamo il recupero mirato (vedi doc-comment di
                // `repair_invalid_json_escapes`) prima di arrenderci. Se il JSON
                // riparato parsa, l'abbiamo salvato: un turno AI altrimenti completo e
                // valido non va perso solo per un backslash orfano.
                let repaired = repair_invalid_json_escapes(&tc.function.arguments);
                match serde_json::from_str(&repaired) {
                    Ok(v) => {
                        tracing::warn!(
                            "arguments di '{}': JSON non valido riparato automaticamente (escape orfano)",
                            tc.function.name
                        );
                        v
                    }
                    Err(_repair_err) => {
                        // La riparazione non ha salvato il turno: il problema è più
                        // profondo di un backslash orfano (struttura rotta, JSON
                        // troncato...). Logghiamo gli arguments GREZZI ORIGINALI (non il
                        // tentativo di riparazione, che qui non è significativo) prima di
                        // restituire l'errore — oggi questa stringa andrebbe persa per
                        // sempre nel momento in cui l'errore scatta, rendendo impossibile
                        // diagnosticare una FUTURA malformazione diversa da questa.
                        // Troncato a 2000 caratteri per non inondare i log con input
                        // patologici (l'obiettivo è diagnosticare, non archiviare).
                        let excerpt: String =
                            tc.function.arguments.chars().take(2000).collect();
                        tracing::warn!(
                            "arguments di '{}' non riparabili, JSON grezzo (troncato a 2000 char): {excerpt}",
                            tc.function.name
                        );
                        // L'errore restituito è costruito dall'errore di parsing
                        // ORIGINALE (non da quello del tentativo di riparazione): la
                        // riparazione non era mai stata una fix garantita in questo
                        // ramo, quindi l'errore sulla stringa originale resta il più
                        // significativo per chi legge il messaggio — stesso formato di
                        // prima di questa fix, così il test di regressione
                        // `from_or_response_malformed_arguments_is_a_decode_error`
                        // continua a valere invariato.
                        return Err(BackendError::Decode(format!(
                            "arguments di '{}' non è JSON valido: {original_err}",
                            tc.function.name
                        )));
                    }
                }
            }
        };
        blocks.push(Block::ToolUse { id: tc.id, name: tc.function.name, input });
    }

    let stop = match choice.finish_reason.as_str() {
        "length" => TurnStop::MaxTokens,
        "content_filter" => TurnStop::Refused { category: None },
        _ => TurnStop::Normal,
    };
    Ok(BackendTurn { blocks, stop })
}

// ── Trasporto (seam) ──────────────────────────────────────────────────────────

/// Errori di trasporto/decodifica verso OpenRouter. Mirror di `MessagesError`
/// (`messages_client.rs`).
#[derive(Debug, Clone, PartialEq)]
pub enum OrError {
    Http { status: u16, body: String },
    Network(String),
    Decode(String),
}

impl std::fmt::Display for OrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OrError::Http { status, body } => write!(f, "HTTP {status}: {body}"),
            OrError::Network(e) => write!(f, "rete: {e}"),
            OrError::Decode(e) => write!(f, "decodifica: {e}"),
        }
    }
}

impl From<OrError> for BackendError {
    fn from(e: OrError) -> Self {
        match e {
            OrError::Http { status, body } => BackendError::Http { status, body },
            OrError::Network(s) => BackendError::Network(s),
            OrError::Decode(s) => BackendError::Decode(s),
        }
    }
}

/// Seam: la chiamata grezza a `POST /chat/completions`. `HttpOpenRouterClient` (Slice
/// 3, non ancora scritto) sarà la prima implementazione reale; per ora solo
/// `FakeOpenRouterClient` (cfg(test)).
#[async_trait]
pub trait OpenRouterClient: Send + Sync {
    async fn create(&self, req: OrRequest) -> Result<OrResponse, OrError>;
}

// ─────────────────────────────────────────────────────────────────────────────
// Fake (cfg(test), crate-visibile): stesso pattern di `FakeMessagesClient`
// in `messages_client.rs` — sequenza; registra TUTTE le richieste.
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
pub(crate) struct FakeOpenRouterClient {
    requests: Mutex<Vec<OrRequest>>,
    responses: Mutex<VecDeque<Result<OrResponse, OrError>>>,
    repeat: bool,
}

#[cfg(test)]
impl FakeOpenRouterClient {
    /// Risponde in sequenza (una per chiamata); esaurita → `Err` di cortesia.
    pub(crate) fn sequence(responses: Vec<Result<OrResponse, OrError>>) -> Self {
        Self {
            requests: Mutex::new(Vec::new()),
            responses: Mutex::new(responses.into_iter().collect()),
            repeat: false,
        }
    }
    pub(crate) fn ok(resp: OrResponse) -> Self {
        Self::sequence(vec![Ok(resp)])
    }
    pub(crate) fn err(e: OrError) -> Self {
        Self::sequence(vec![Err(e)])
    }
    pub(crate) fn recorded(&self) -> OrRequest {
        self.requests
            .lock()
            .unwrap()
            .last()
            .cloned()
            .expect("nessuna richiesta registrata")
    }
    pub(crate) fn nth_request(&self, n: usize) -> OrRequest {
        self.requests
            .lock()
            .unwrap()
            .get(n)
            .cloned()
            .expect("nessuna richiesta a quell'indice")
    }
    pub(crate) fn request_count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}

#[cfg(test)]
#[async_trait]
impl OpenRouterClient for FakeOpenRouterClient {
    async fn create(&self, req: OrRequest) -> Result<OrResponse, OrError> {
        self.requests.lock().unwrap().push(req);
        let mut q = self.responses.lock().unwrap();
        if self.repeat {
            q.front()
                .cloned()
                .unwrap_or_else(|| Err(OrError::Network("fake: vuoto".to_string())))
        } else {
            q.pop_front()
                .unwrap_or_else(|| Err(OrError::Network("fake: niente più risposte".to_string())))
        }
    }
}

// ── ChatBackend ────────────────────────────────────────────────────────────

/// `ChatBackend` che parla un'API OpenAI-compatibile (OpenRouter) tramite un
/// `Arc<dyn OpenRouterClient>` — stesso principio di `ClaudeBackend`
/// (`claude_backend.rs`), ma qui la traduzione è sostanziale (vedi
/// `to_or_messages`/`to_or_tools`/`from_or_response` sopra).
///
/// V1 non-streaming (spec §4.4): `send_turn` fa una singola chiamata e invoca `on_text`
/// una sola volta con tutto il testo, prima di ritornare — riusa la stessa firma di
/// `ClaudeBackend` senza bisogno di un secondo metodo sul trait `ChatBackend`.
pub struct OpenRouterBackend {
    client: Arc<dyn OpenRouterClient>,
    model: String,
    max_tokens: Option<u32>,
}

impl OpenRouterBackend {
    pub fn new(client: Arc<dyn OpenRouterClient>, model: String, max_tokens: Option<u32>) -> Self {
        Self { client, model, max_tokens }
    }
}

#[async_trait]
impl ChatBackend for OpenRouterBackend {
    async fn send_turn(
        &self,
        system: &str,
        tools: &[ToolSpec],
        history: &[Message],
        on_text: &mut (dyn for<'a> FnMut(&'a str) + Send),
        on_heartbeat: &mut (dyn FnMut() + Send),
    ) -> Result<BackendTurn, BackendError> {
        let _ = on_heartbeat; // REST non-streaming: nessun ping, mai chiamato.
        let req = OrRequest {
            model: self.model.clone(),
            messages: to_or_messages(system, history),
            tools: to_or_tools(tools),
            max_tokens: self.max_tokens,
        };
        let resp = self.client.create(req).await.map_err(BackendError::from)?;
        let turn = from_or_response(resp)?;
        let text = turn.text();
        if !text.is_empty() {
            on_text(&text);
        }
        Ok(turn)
    }

    // `to_or_tools` scarta OGNI `ToolSpec::Server` (vedi commento lì) — questo
    // backend non può eseguire `web_search` nativo Anthropic per nessun
    // provider dietro OpenRouter. Dichiararlo qui spegne anche l'addendum del
    // system prompt lato `LlmAdapter::respond` (`chat_backend.rs`), non solo
    // il tool: senza questo, il modello si crede capace di cercare sul web
    // (glielo dice il prompt), tenta la chiamata comunque, e riceve "tool
    // sconosciuto" — osservato dal vivo con DeepSeek (2026-07-20).
    fn supports_web_search(&self) -> bool {
        false
    }
}

// ── Trasporto reale (reqwest) ──────────────────────────────────────────────

/// Timeout totale (connect+send+lettura completa) di una singola chiamata REST
/// non-streaming. Molto più semplice del doppio timeout idle/totale di
/// `HttpMessagesClient` (`messages_client.rs`): qui non c'è SSE — una risposta
/// completa arriva in un solo `send().await`, non serve rilevare uno stallo a
/// metà stream, solo un backstop contro un hang patologico.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

/// Client reale: `reqwest`+rustls verso `POST {base_url}/chat/completions`
/// (API OpenAI-compatibile di OpenRouter). Header opzionali `HTTP-Referer`/
/// `X-Title` (raccomandati da OpenRouter per comparire nella loro dashboard
/// con nome/URL del progetto invece che come chiamata anonima — MAI richiesti
/// per il funzionamento) — `None` di default.
pub struct HttpOpenRouterClient {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
    http_referer: Option<String>,
    x_title: Option<String>,
}

impl HttpOpenRouterClient {
    pub fn new(api_key: String) -> Self {
        Self::with_base_url(api_key, "https://openrouter.ai/api/v1".to_string())
    }

    pub fn with_base_url(api_key: String, base_url: String) -> Self {
        Self::with_timeout(api_key, base_url, DEFAULT_TIMEOUT)
    }

    /// Costruttore completo: permette un timeout diverso dal default di
    /// produzione. Usato dai test per non dover aspettare minuti reali; `new`/
    /// `with_base_url` restano l'API pubblica con il default sensato.
    fn with_timeout(api_key: String, base_url: String, timeout: Duration) -> Self {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .expect("failed to build reqwest client");
        Self {
            client,
            api_key,
            base_url,
            http_referer: None,
            x_title: None,
        }
    }

    /// Imposta gli header opzionali di attribuzione OpenRouter. Nessun effetto
    /// funzionale se omessi (rimangono `None` di default).
    pub fn with_attribution(
        mut self,
        http_referer: Option<String>,
        x_title: Option<String>,
    ) -> Self {
        self.http_referer = http_referer;
        self.x_title = x_title;
        self
    }
}

/// Formatta un `reqwest::Error` rendendo esplicito un eventuale timeout — stesso
/// principio di `describe_network_error` in `messages_client.rs`, ma qui è una
/// funzione indipendente (nessuna condivisione fra i due backend: ognuno resta
/// un modulo autonomo, coerente con `ClaudeBackend`/`OpenRouterBackend` oggi).
fn describe_network_error(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        format!("timeout: {e}")
    } else {
        e.to_string()
    }
}

#[async_trait]
impl OpenRouterClient for HttpOpenRouterClient {
    async fn create(&self, req: OrRequest) -> Result<OrResponse, OrError> {
        let url = format!("{}/chat/completions", self.base_url);
        let mut builder = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("content-type", "application/json");
        if let Some(referer) = &self.http_referer {
            builder = builder.header("HTTP-Referer", referer);
        }
        if let Some(title) = &self.x_title {
            builder = builder.header("X-Title", title);
        }

        let resp = builder
            .json(&req)
            .send()
            .await
            .map_err(|e| OrError::Network(describe_network_error(&e)))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(OrError::Http {
                status: status.as_u16(),
                body,
            });
        }

        resp.json::<OrResponse>()
            .await
            .map_err(|e| OrError::Decode(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use crate::chat_backend::{BackendError, ChatBackend, TurnStop};
    use crate::openrouter_backend::{
        from_or_response, repair_invalid_json_escapes, to_or_messages, to_or_tools,
        FakeOpenRouterClient, HttpOpenRouterClient, OpenRouterBackend, OrChoice, OrError,
        OrFunctionCall, OrFunctionDef, OrMessage, OrRequest, OrResponse, OrTool, OrToolCall,
    };
    use crate::openrouter_backend::OpenRouterClient;
    use crate::messages_client::{Block, Message, ServerTool, ToolDef, ToolSpec};
    use std::sync::Arc;
    use std::sync::Mutex;

    const TURN1_REQUEST: &str = include_str!(
        "../../../Docs/superpowers/fixtures/2026-07-06-deepseek-openrouter-real-roundtrip/turn1-request.json"
    );
    const TURN1_RESPONSE: &str = include_str!(
        "../../../Docs/superpowers/fixtures/2026-07-06-deepseek-openrouter-real-roundtrip/turn1-response.json"
    );
    const TURN2_REQUEST: &str = include_str!(
        "../../../Docs/superpowers/fixtures/2026-07-06-deepseek-openrouter-real-roundtrip/turn2-request.json"
    );
    const TURN2_RESPONSE: &str = include_str!(
        "../../../Docs/superpowers/fixtures/2026-07-06-deepseek-openrouter-real-roundtrip/turn2-response.json"
    );

    fn run_in_session_tool() -> OrTool {
        OrTool {
            kind: "function".to_string(),
            function: OrFunctionDef {
                name: "run_in_session".to_string(),
                description: "Esegui un comando nella shell persistente e ricevi l'output."
                    .to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": {"type": "string", "description": "Il comando da eseguire"}
                    },
                    "required": ["command"]
                }),
            },
        }
    }

    #[test]
    fn or_request_turn1_serializes_exactly_like_the_captured_fixture() {
        let req = OrRequest {
            model: "deepseek/deepseek-chat".to_string(),
            messages: vec![OrMessage {
                role: "user".to_string(),
                content: Some(
                    "Elenca i file nella cartella corrente usando lo strumento run_in_session."
                        .to_string(),
                ),
                tool_calls: None,
                tool_call_id: None,
            }],
            tools: vec![run_in_session_tool()],
            max_tokens: None,
        };

        let actual = serde_json::to_value(&req).unwrap();
        let expected: serde_json::Value = serde_json::from_str(TURN1_REQUEST).unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn or_request_turn2_serializes_exactly_like_the_captured_fixture() {
        let req = OrRequest {
            model: "deepseek/deepseek-chat".to_string(),
            messages: vec![
                OrMessage {
                    role: "user".to_string(),
                    content: Some(
                        "Elenca i file nella cartella corrente usando lo strumento run_in_session."
                            .to_string(),
                    ),
                    tool_calls: None,
                    tool_call_id: None,
                },
                OrMessage {
                    role: "assistant".to_string(),
                    content: None,
                    tool_calls: Some(vec![OrToolCall {
                        kind: "function".to_string(),
                        id: "28714ee3a8ce4c3f926a572d72bb434c".to_string(),
                        function: OrFunctionCall {
                            name: "run_in_session".to_string(),
                            arguments: "{\"command\":\"ls\"}".to_string(),
                        },
                    }]),
                    tool_call_id: None,
                },
                OrMessage {
                    role: "tool".to_string(),
                    content: Some("Cargo.lock\nCargo.toml\ncrates\nDocs\ntarget".to_string()),
                    tool_calls: None,
                    tool_call_id: Some("28714ee3a8ce4c3f926a572d72bb434c".to_string()),
                },
            ],
            tools: vec![run_in_session_tool()],
            max_tokens: None,
        };

        let actual = serde_json::to_value(&req).unwrap();
        let expected: serde_json::Value = serde_json::from_str(TURN2_REQUEST).unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn or_response_turn1_deserializes_tool_call_with_string_arguments() {
        let resp: OrResponse = serde_json::from_str(TURN1_RESPONSE).unwrap();
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(resp.choices[0].finish_reason, "tool_calls");
        let msg = &resp.choices[0].message;
        assert_eq!(msg.role, "assistant");
        assert_eq!(msg.content, None, "content è null nella risposta reale");
        let calls = msg.tool_calls.as_ref().expect("tool_calls presente");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "28714ee3a8ce4c3f926a572d72bb434c");
        assert_eq!(calls[0].function.name, "run_in_session");
        assert_eq!(
            calls[0].function.arguments, "{\"command\":\"ls\"}",
            "arguments è una STRINGA JSON, non un oggetto nativo"
        );
    }

    #[test]
    fn or_response_turn2_deserializes_text_only_with_absent_tool_calls() {
        let resp: OrResponse = serde_json::from_str(TURN2_RESPONSE).unwrap();
        assert_eq!(resp.choices[0].finish_reason, "stop");
        let msg = &resp.choices[0].message;
        assert!(
            msg.content.as_deref().unwrap_or("").contains("Cargo.lock"),
            "testo atteso nella risposta: {:?}",
            msg.content
        );
        assert_eq!(
            msg.tool_calls, None,
            "tool_calls è ASSENTE dal JSON quando non c'è tool-call — deve deserializzare a None, non a errore"
        );
    }

    #[test]
    fn to_or_messages_omits_system_message_when_system_is_empty() {
        let history = vec![Message::user_text("ciao")];
        let out = to_or_messages("", &history);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].role, "user");
        assert_eq!(out[0].content, Some("ciao".to_string()));
    }

    #[test]
    fn to_or_messages_prepends_system_when_non_empty() {
        let history = vec![Message::user_text("ciao")];
        let out = to_or_messages("sei un assistente", &history);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].role, "system");
        assert_eq!(out[0].content, Some("sei un assistente".to_string()));
        assert_eq!(out[1].role, "user");
        assert_eq!(out[1].content, Some("ciao".to_string()));
    }

    #[test]
    fn to_or_messages_maps_assistant_text_and_tool_use_into_one_message_with_tool_calls() {
        let history = vec![Message::with_blocks(
            "assistant",
            vec![
                Block::Text { text: "eseguo:".to_string() },
                Block::ToolUse {
                    id: "tu_1".to_string(),
                    name: "run_in_session".to_string(),
                    input: serde_json::json!({"command": "ls"}),
                },
            ],
        )];
        let out = to_or_messages("", &history);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].role, "assistant");
        assert_eq!(out[0].content, Some("eseguo:".to_string()));
        let calls = out[0].tool_calls.as_ref().expect("tool_calls presente");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "tu_1");
        assert_eq!(calls[0].function.name, "run_in_session");
        assert_eq!(calls[0].function.arguments, "{\"command\":\"ls\"}");
    }

    #[test]
    fn to_or_messages_assistant_tool_use_only_has_no_content() {
        let history = vec![Message::with_blocks(
            "assistant",
            vec![Block::ToolUse {
                id: "tu_1".to_string(),
                name: "run_in_session".to_string(),
                input: serde_json::json!({"command": "ls"}),
            }],
        )];
        let out = to_or_messages("", &history);
        assert_eq!(out[0].content, None, "nessun testo → content None, non stringa vuota");
    }

    #[test]
    fn to_or_messages_unrolls_multiple_tool_results_into_separate_tool_messages() {
        let history = vec![Message::with_blocks(
            "user",
            vec![
                Block::ToolResult {
                    tool_use_id: "tu_1".to_string(),
                    content: "ok1".to_string(),
                    is_error: false,
                },
                Block::ToolResult {
                    tool_use_id: "tu_2".to_string(),
                    content: "boom".to_string(),
                    is_error: true,
                },
            ],
        )];
        let out = to_or_messages("", &history);
        assert_eq!(out.len(), 2, "un turno con N risultati diventa N messaggi tool separati");
        assert_eq!(out[0].role, "tool");
        assert_eq!(out[0].tool_call_id, Some("tu_1".to_string()));
        assert_eq!(out[0].content, Some("ok1".to_string()));
        assert_eq!(out[1].role, "tool");
        assert_eq!(out[1].tool_call_id, Some("tu_2".to_string()));
        assert_eq!(
            out[1].content,
            Some("[error] boom".to_string()),
            "is_error non ha un campo OpenAI dedicato: si ripiega nel content"
        );
    }

    #[test]
    fn to_or_tools_maps_custom_tool_def() {
        let tools = vec![ToolSpec::Custom(ToolDef {
            name: "run_in_session".to_string(),
            description: "d".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
        })];
        let out = to_or_tools(&tools);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, "function");
        assert_eq!(out[0].function.name, "run_in_session");
        assert_eq!(out[0].function.description, "d");
        assert_eq!(out[0].function.parameters, serde_json::json!({"type": "object"}));
    }

    #[test]
    fn to_or_tools_discards_server_side_tools() {
        let tools = vec![
            ToolSpec::Server(ServerTool {
                kind: "web_search_20260209".to_string(),
                name: "web_search".to_string(),
                max_uses: None,
            }),
            ToolSpec::Custom(ToolDef {
                name: "run_in_session".to_string(),
                description: "d".to_string(),
                input_schema: serde_json::json!({"type": "object"}),
            }),
        ];
        let out = to_or_tools(&tools);
        assert_eq!(
            out.len(), 1,
            "il tool server-side Anthropic non ha equivalente OpenRouter: va scartato, non tradotto"
        );
        assert_eq!(out[0].function.name, "run_in_session");
    }

    #[test]
    fn to_or_tools_empty_input_is_empty_output() {
        assert_eq!(to_or_tools(&[]), Vec::new());
    }

    #[test]
    fn from_or_response_text_only_maps_to_normal_stop() {
        let resp = OrResponse {
            choices: vec![OrChoice {
                finish_reason: "stop".to_string(),
                message: OrMessage {
                    role: "assistant".to_string(),
                    content: Some("ciao".to_string()),
                    tool_calls: None,
                    tool_call_id: None,
                },
            }],
        };
        let turn = from_or_response(resp).unwrap();
        assert_eq!(turn.stop, TurnStop::Normal);
        assert_eq!(turn.text(), "ciao");
    }

    #[test]
    fn from_or_response_tool_calls_decode_arguments_into_tool_use_blocks() {
        let resp = OrResponse {
            choices: vec![OrChoice {
                finish_reason: "tool_calls".to_string(),
                message: OrMessage {
                    role: "assistant".to_string(),
                    content: None,
                    tool_calls: Some(vec![OrToolCall {
                        kind: "function".to_string(),
                        id: "tu_1".to_string(),
                        function: OrFunctionCall {
                            name: "run_in_session".to_string(),
                            arguments: "{\"command\":\"ls\"}".to_string(),
                        },
                    }]),
                    tool_call_id: None,
                },
            }],
        };
        let turn = from_or_response(resp).unwrap();
        assert_eq!(turn.stop, TurnStop::Normal);
        assert_eq!(turn.blocks.len(), 1);
        match &turn.blocks[0] {
            Block::ToolUse { id, name, input } => {
                assert_eq!(id, "tu_1");
                assert_eq!(name, "run_in_session");
                assert_eq!(input, &serde_json::json!({"command": "ls"}));
            }
            other => panic!("atteso ToolUse, trovato {other:?}"),
        }
    }

    #[test]
    fn from_or_response_malformed_arguments_is_a_decode_error() {
        let resp = OrResponse {
            choices: vec![OrChoice {
                finish_reason: "tool_calls".to_string(),
                message: OrMessage {
                    role: "assistant".to_string(),
                    content: None,
                    tool_calls: Some(vec![OrToolCall {
                        kind: "function".to_string(),
                        id: "tu_1".to_string(),
                        function: OrFunctionCall {
                            name: "run_in_session".to_string(),
                            arguments: "{questo non è json valido".to_string(),
                        },
                    }]),
                    tool_call_id: None,
                },
            }],
        };
        let err = from_or_response(resp).unwrap_err();
        assert!(
            matches!(err, BackendError::Decode(_)),
            "arguments malformato deve essere un Decode esplicito, non un panic o un drop silenzioso: {err:?}"
        );
    }

    /// Riproduce il bug reale segnalato dall'utente: OpenRouter/DeepSeek, quando chiama
    /// `show_markdown` con una risposta lunga, a volte scrive dentro `arguments` un escape
    /// Markdown letterale come `\*` (valido in Markdown) SENZA raddoppiare il backslash
    /// come richiederebbe JSON grezzo — il modello genera la stringa `arguments`
    /// serializzandola da sé (a differenza di Claude, dove il tool-use arriva già come
    /// oggetto strutturato dall'API, non da testo libero del modello). `serde_json` non
    /// riconosce `\*` come uno dei 9 escape validi e fallisce con "invalid escape".
    ///
    /// PRIMA della fix: questo test deve fallire con lo stesso tipo di errore
    /// (`BackendError::Decode` con messaggio "... non è JSON valido: invalid escape ...")
    /// che l'utente ha visto in produzione — è la conferma che riproduciamo il bug vero,
    /// non un caso inventato. DOPO la fix (repair_invalid_json_escapes innestata in
    /// `from_or_response`): deve riuscire, salvando il contenuto generato dal modello.
    #[test]
    fn from_or_response_recovers_from_stray_markdown_escape_in_arguments() {
        let resp = OrResponse {
            choices: vec![OrChoice {
                finish_reason: "tool_calls".to_string(),
                message: OrMessage {
                    role: "assistant".to_string(),
                    content: None,
                    tool_calls: Some(vec![OrToolCall {
                        kind: "function".to_string(),
                        id: "tu_1".to_string(),
                        function: OrFunctionCall {
                            name: "show_markdown".to_string(),
                            // `\*` compare due volte SENZA raddoppio del backslash: JSON
                            // grezzo invalido, ma un errore Markdown-in-JSON plausibilissimo
                            // per un modello che genera testo lungo.
                            arguments: r#"{"content":"pack list: item1\*, item2\*"}"#.to_string(),
                        },
                    }]),
                    tool_call_id: None,
                },
            }],
        };

        let turn = from_or_response(resp)
            .expect("la riparazione deve salvare il turno invece di scartarlo");
        assert_eq!(turn.blocks.len(), 1);
        match &turn.blocks[0] {
            Block::ToolUse { name, input, .. } => {
                assert_eq!(name, "show_markdown");
                // Il backslash-asterisco letterale scritto dal modello deve sopravvivere
                // (non essere silenziosamente perso né trasformato in altro): il valore
                // Rust decodificato contiene i due caratteri '\\' e '*' consecutivi.
                assert_eq!(
                    input["content"],
                    "pack list: item1\\*, item2\\*",
                    "il contenuto generato dal modello va salvato integro dopo la riparazione"
                );
            }
            other => panic!("atteso ToolUse, trovato {other:?}"),
        }
    }

    #[test]
    fn from_or_response_length_maps_to_max_tokens() {
        let resp = OrResponse {
            choices: vec![OrChoice {
                finish_reason: "length".to_string(),
                message: OrMessage {
                    role: "assistant".to_string(),
                    content: Some("troncato".to_string()),
                    tool_calls: None,
                    tool_call_id: None,
                },
            }],
        };
        let turn = from_or_response(resp).unwrap();
        assert_eq!(turn.stop, TurnStop::MaxTokens);
        assert_eq!(turn.text(), "troncato");
    }

    #[test]
    fn from_or_response_content_filter_maps_to_refused_with_no_category() {
        let resp = OrResponse {
            choices: vec![OrChoice {
                finish_reason: "content_filter".to_string(),
                message: OrMessage {
                    role: "assistant".to_string(),
                    content: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
            }],
        };
        let turn = from_or_response(resp).unwrap();
        assert_eq!(
            turn.stop,
            TurnStop::Refused { category: None },
            "OpenAI non fornisce una categoria come Anthropic: None è onesto, non un'invenzione"
        );
    }

    #[test]
    fn from_or_response_null_content_produces_no_text_block() {
        let resp = OrResponse {
            choices: vec![OrChoice {
                finish_reason: "tool_calls".to_string(),
                message: OrMessage {
                    role: "assistant".to_string(),
                    content: None,
                    tool_calls: Some(vec![OrToolCall {
                        kind: "function".to_string(),
                        id: "tu_1".to_string(),
                        function: OrFunctionCall {
                            name: "run_in_session".to_string(),
                            arguments: "{}".to_string(),
                        },
                    }]),
                    tool_call_id: None,
                },
            }],
        };
        let turn = from_or_response(resp).unwrap();
        assert!(
            !turn.blocks.iter().any(|b| matches!(b, Block::Text { .. })),
            "content null non deve produrre un Block::Text vuoto: {:?}",
            turn.blocks
        );
    }

    #[tokio::test]
    async fn fake_openrouter_client_records_request_and_replays_responses() {
        let fake = FakeOpenRouterClient::sequence(vec![
            Ok(OrResponse {
                choices: vec![OrChoice {
                    finish_reason: "stop".to_string(),
                    message: OrMessage {
                        role: "assistant".to_string(),
                        content: Some("prima".to_string()),
                        tool_calls: None,
                        tool_call_id: None,
                    },
                }],
            }),
            Ok(OrResponse {
                choices: vec![OrChoice {
                    finish_reason: "stop".to_string(),
                    message: OrMessage {
                        role: "assistant".to_string(),
                        content: Some("seconda".to_string()),
                        tool_calls: None,
                        tool_call_id: None,
                    },
                }],
            }),
        ]);
        let req = OrRequest {
            model: "deepseek/deepseek-chat".to_string(),
            messages: vec![OrMessage {
                role: "user".to_string(),
                content: Some("ciao".to_string()),
                tool_calls: None,
                tool_call_id: None,
            }],
            tools: Vec::new(),
            max_tokens: None,
        };

        let first = fake.create(req.clone()).await.unwrap();
        assert_eq!(first.choices[0].message.content, Some("prima".to_string()));
        let second = fake.create(req.clone()).await.unwrap();
        assert_eq!(second.choices[0].message.content, Some("seconda".to_string()));

        assert_eq!(fake.request_count(), 2);
        assert_eq!(fake.recorded().model, "deepseek/deepseek-chat");
        assert_eq!(fake.nth_request(0), req);
    }

    #[tokio::test]
    async fn fake_openrouter_client_err_surfaces_the_configured_error() {
        let fake = FakeOpenRouterClient::err(OrError::Network("boom".to_string()));
        let req = OrRequest {
            model: "m".to_string(),
            messages: Vec::new(),
            tools: Vec::new(),
            max_tokens: None,
        };
        let err = fake.create(req).await.unwrap_err();
        assert_eq!(err, OrError::Network("boom".to_string()));
    }

    #[tokio::test]
    async fn fake_openrouter_client_ok_replies_with_the_given_response() {
        let fake = FakeOpenRouterClient::ok(OrResponse {
            choices: vec![OrChoice {
                finish_reason: "stop".to_string(),
                message: OrMessage {
                    role: "assistant".to_string(),
                    content: Some("ok".to_string()),
                    tool_calls: None,
                    tool_call_id: None,
                },
            }],
        });
        let req = OrRequest {
            model: "m".to_string(),
            messages: Vec::new(),
            tools: Vec::new(),
            max_tokens: None,
        };
        let resp = fake.create(req).await.unwrap();
        assert_eq!(resp.choices[0].message.content, Some("ok".to_string()));
    }

    #[tokio::test]
    async fn forwards_system_tools_and_history_into_or_request() {
        let fake = Arc::new(FakeOpenRouterClient::ok(OrResponse {
            choices: vec![OrChoice {
                finish_reason: "stop".to_string(),
                message: OrMessage {
                    role: "assistant".to_string(),
                    content: Some("ok".to_string()),
                    tool_calls: None,
                    tool_call_id: None,
                },
            }],
        }));
        let backend = OpenRouterBackend::new(
            fake.clone(),
            "deepseek/deepseek-chat".to_string(),
            Some(4096),
        );
        let tools = vec![ToolSpec::Custom(ToolDef {
            name: "run_in_session".to_string(),
            description: "d".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
        })];
        let history = vec![Message::user_text("ciao")];
        let mut on_text = |_: &str| {};
        let mut on_heartbeat = || {};

        backend.send_turn("sistema", &tools, &history, &mut on_text, &mut on_heartbeat).await.unwrap();

        let req = fake.recorded();
        assert_eq!(req.model, "deepseek/deepseek-chat");
        assert_eq!(req.max_tokens, Some(4096));
        assert_eq!(req.messages[0].role, "system");
        assert_eq!(req.messages[0].content, Some("sistema".to_string()));
        assert_eq!(req.messages[1].role, "user");
        assert_eq!(req.tools.len(), 1);
        assert_eq!(req.tools[0].function.name, "run_in_session");
    }

    #[test]
    fn openrouter_backend_does_not_support_web_search() {
        // `to_or_tools` scarta già `ToolSpec::Server` (vedi
        // `to_or_tools_discards_server_side_tools`); questo test copre l'altra
        // metà del fix — il chiamante (`LlmAdapter::respond`) deve poter sapere
        // che questo backend non ha `web_search`, per spegnere anche
        // l'addendum del system prompt che lo promette.
        let fake = Arc::new(FakeOpenRouterClient::sequence(vec![]));
        let backend = OpenRouterBackend::new(fake, "deepseek/deepseek-chat".to_string(), None);
        assert!(!backend.supports_web_search());
    }

    #[tokio::test]
    async fn send_turn_invokes_on_text_once_with_full_response_text() {
        let fake = Arc::new(FakeOpenRouterClient::ok(OrResponse {
            choices: vec![OrChoice {
                finish_reason: "stop".to_string(),
                message: OrMessage {
                    role: "assistant".to_string(),
                    content: Some("risposta intera".to_string()),
                    tool_calls: None,
                    tool_call_id: None,
                },
            }],
        }));
        let backend =
            OpenRouterBackend::new(fake, "deepseek/deepseek-chat".to_string(), None);
        let mut calls = Vec::new();
        let mut on_text = |delta: &str| calls.push(delta.to_string());
        let mut on_heartbeat = || {};

        backend.send_turn("", &[], &[], &mut on_text, &mut on_heartbeat).await.unwrap();

        assert_eq!(
            calls,
            vec!["risposta intera".to_string()],
            "non-streaming v1: on_text chiamata UNA sola volta con tutto il testo"
        );
    }

    #[tokio::test]
    async fn network_error_maps_to_backend_error() {
        let fake = Arc::new(FakeOpenRouterClient::err(OrError::Network("boom".to_string())));
        let backend =
            OpenRouterBackend::new(fake, "deepseek/deepseek-chat".to_string(), None);
        let mut on_text = |_: &str| {};
        let mut on_heartbeat = || {};

        let err = backend.send_turn("", &[], &[], &mut on_text, &mut on_heartbeat).await.unwrap_err();

        assert_eq!(err, BackendError::Network("boom".to_string()));
    }

    #[tokio::test]
    async fn decode_error_from_malformed_arguments_surfaces_through_send_turn() {
        let fake = Arc::new(FakeOpenRouterClient::ok(OrResponse {
            choices: vec![OrChoice {
                finish_reason: "tool_calls".to_string(),
                message: OrMessage {
                    role: "assistant".to_string(),
                    content: None,
                    tool_calls: Some(vec![OrToolCall {
                        kind: "function".to_string(),
                        id: "tu_1".to_string(),
                        function: OrFunctionCall {
                            name: "run_in_session".to_string(),
                            arguments: "{non valido".to_string(),
                        },
                    }]),
                    tool_call_id: None,
                },
            }],
        }));
        let backend =
            OpenRouterBackend::new(fake, "deepseek/deepseek-chat".to_string(), None);
        let mut on_text = |_: &str| {};
        let mut on_heartbeat = || {};

        let err = backend.send_turn("", &[], &[], &mut on_text, &mut on_heartbeat).await.unwrap_err();

        assert!(matches!(err, BackendError::Decode(_)));
    }

    /// Rigioca l'INTERO round-trip a 2 turni della fixture reale
    /// (`Docs/superpowers/fixtures/2026-07-06-deepseek-openrouter-real-roundtrip/`) attraverso
    /// `OpenRouterBackend`: turno 1 chiama il tool, turno 2 usa il risultato e risponde in
    /// testo — esattamente il flusso che `ai_adapter::LlmAdapter::respond` guida in
    /// produzione, qui verificato end-to-end (assemblaggio richiesta + parsing risposta)
    /// senza HTTP reale.
    #[tokio::test]
    async fn replays_the_real_captured_deepseek_roundtrip_end_to_end() {
        let turn1_response: OrResponse = serde_json::from_str(TURN1_RESPONSE).unwrap();
        let turn2_response: OrResponse = serde_json::from_str(TURN2_RESPONSE).unwrap();
        let fake = Arc::new(FakeOpenRouterClient::sequence(vec![
            Ok(turn1_response),
            Ok(turn2_response),
        ]));
        let backend =
            OpenRouterBackend::new(fake.clone(), "deepseek/deepseek-chat".to_string(), None);
        let tools = vec![ToolSpec::Custom(ToolDef {
            name: "run_in_session".to_string(),
            description: "Esegui un comando nella shell persistente e ricevi l'output.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string", "description": "Il comando da eseguire"}
                },
                "required": ["command"]
            }),
        })];

        // Turno 1: l'utente chiede di elencare i file.
        let mut history = vec![Message::user_text(
            "Elenca i file nella cartella corrente usando lo strumento run_in_session.",
        )];
        let mut on_text_1 = |_: &str| {};
        let mut on_heartbeat = || {};
        let turn1 = backend
            .send_turn("", &tools, &history, &mut on_text_1, &mut on_heartbeat)
            .await
            .unwrap();
        assert_eq!(turn1.blocks.len(), 1);
        let Block::ToolUse { id: tool_id, name, input } = &turn1.blocks[0] else {
            panic!("atteso ToolUse al turno 1: {:?}", turn1.blocks);
        };
        assert_eq!(name, "run_in_session");
        assert_eq!(input, &serde_json::json!({"command": "ls"}));

        // Il chiamante (in produzione: ai_adapter::LlmAdapter::respond) esegue il tool e
        // aggiunge assistant+tool_result alla storia, esattamente come fa oggi per Claude.
        history.push(Message::with_blocks("assistant", turn1.blocks.clone()));
        history.push(Message::with_blocks(
            "user",
            vec![Block::ToolResult {
                tool_use_id: tool_id.clone(),
                content: "Cargo.lock\nCargo.toml\ncrates\nDocs\ntarget".to_string(),
                is_error: false,
            }],
        ));

        // Turno 2: il modello usa il risultato e risponde in testo.
        let mut on_text_2 = |_: &str| {};
        let mut on_heartbeat = || {};
        let turn2 = backend
            .send_turn("", &tools, &history, &mut on_text_2, &mut on_heartbeat)
            .await
            .unwrap();
        assert_eq!(turn2.stop, TurnStop::Normal);
        assert!(
            turn2.text().contains("Cargo.lock"),
            "il turno 2 deve rispondere usando il risultato del tool: {}",
            turn2.text()
        );

        // La richiesta del turno 2 deve avere la forma esatta che la fixture reale ha
        // dimostrato essere accettata da OpenRouter/DeepSeek (README della fixture, punto 3).
        let sent = fake.nth_request(1);
        assert_eq!(sent.messages.len(), 3);
        assert_eq!(sent.messages[1].role, "assistant");
        assert_eq!(sent.messages[1].content, None);
        assert_eq!(
            sent.messages[1].tool_calls.as_ref().unwrap()[0].function.arguments,
            "{\"command\":\"ls\"}"
        );
        assert_eq!(sent.messages[2].role, "tool");
        assert_eq!(sent.messages[2].tool_call_id, Some(tool_id.clone()));
    }

    #[tokio::test]
    async fn create_sends_bearer_auth_header_and_posts_to_chat_completions() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let recorded: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let recorded_clone = recorded.clone();

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 8192];
            let n = stream.read(&mut buf).await.unwrap();
            *recorded_clone.lock().unwrap() = String::from_utf8_lossy(&buf[..n]).to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
                TURN1_RESPONSE.len(),
                TURN1_RESPONSE
            );
            let _ = stream.write_all(response.as_bytes()).await;
        });

        let client =
            HttpOpenRouterClient::with_base_url("sk-test-123".to_string(), format!("http://{addr}"));
        let req = OrRequest {
            model: "deepseek/deepseek-chat".to_string(),
            messages: Vec::new(),
            tools: Vec::new(),
            max_tokens: None,
        };

        let resp = client.create(req).await.expect("richiesta fallita");
        assert_eq!(resp.choices[0].finish_reason, "tool_calls", "deve parsare la risposta reale");

        let sent = recorded.lock().unwrap().clone().to_lowercase();
        assert!(sent.starts_with("post /chat/completions"), "richiesta inattesa: {sent}");
        assert!(
            sent.contains("authorization: bearer sk-test-123"),
            "header di auth mancante o malformato: {sent}"
        );
    }

    #[tokio::test]
    async fn create_maps_non_success_status_to_http_error() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf).await;
            let body = "{\"error\":\"invalid api key\"}";
            let response = format!(
                "HTTP/1.1 401 Unauthorized\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes()).await;
        });

        let client = HttpOpenRouterClient::with_base_url("bad-key".to_string(), format!("http://{addr}"));
        let req = OrRequest {
            model: "m".to_string(),
            messages: Vec::new(),
            tools: Vec::new(),
            max_tokens: None,
        };

        let err = client.create(req).await.unwrap_err();
        match err {
            OrError::Http { status, body } => {
                assert_eq!(status, 401);
                assert!(body.contains("invalid api key"));
            }
            other => panic!("atteso Http, trovato {other:?}"),
        }
    }

    #[tokio::test]
    async fn create_maps_connection_failure_to_network_error() {
        use tokio::net::TcpListener;

        // Bind per ottenere una porta locale libera, poi chiudila subito: nessuno resta
        // in ascolto, la connessione fallisce velocemente (connection refused) — non
        // serve aspettare un vero timeout per testare il path di errore di rete.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);

        let client = HttpOpenRouterClient::with_base_url("k".to_string(), format!("http://{addr}"));
        let req = OrRequest {
            model: "m".to_string(),
            messages: Vec::new(),
            tools: Vec::new(),
            max_tokens: None,
        };

        let err = client.create(req).await.unwrap_err();
        assert!(matches!(err, OrError::Network(_)), "atteso Network, trovato {err:?}");
    }

    #[tokio::test]
    async fn create_sends_attribution_headers_when_configured() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let recorded: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let recorded_clone = recorded.clone();

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 8192];
            let n = stream.read(&mut buf).await.unwrap();
            *recorded_clone.lock().unwrap() = String::from_utf8_lossy(&buf[..n]).to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
                TURN2_RESPONSE.len(),
                TURN2_RESPONSE
            );
            let _ = stream.write_all(response.as_bytes()).await;
        });

        let client = HttpOpenRouterClient::with_base_url("k".to_string(), format!("http://{addr}"))
            .with_attribution(
                Some("https://lare.terminal".to_string()),
                Some("Lare Terminal".to_string()),
            );
        let req = OrRequest {
            model: "m".to_string(),
            messages: Vec::new(),
            tools: Vec::new(),
            max_tokens: None,
        };
        client.create(req).await.expect("richiesta fallita");

        let sent = recorded.lock().unwrap().clone().to_lowercase();
        assert!(sent.contains("http-referer: https://lare.terminal"), "header mancante: {sent}");
        assert!(sent.contains("x-title: lare terminal"), "header mancante: {sent}");
    }

    #[tokio::test]
    async fn create_omits_attribution_headers_by_default() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let recorded: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let recorded_clone = recorded.clone();

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 8192];
            let n = stream.read(&mut buf).await.unwrap();
            *recorded_clone.lock().unwrap() = String::from_utf8_lossy(&buf[..n]).to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
                TURN2_RESPONSE.len(),
                TURN2_RESPONSE
            );
            let _ = stream.write_all(response.as_bytes()).await;
        });

        let client = HttpOpenRouterClient::with_base_url("k".to_string(), format!("http://{addr}"));
        let req = OrRequest {
            model: "m".to_string(),
            messages: Vec::new(),
            tools: Vec::new(),
            max_tokens: None,
        };
        client.create(req).await.expect("richiesta fallita");

        let sent = recorded.lock().unwrap().clone().to_lowercase();
        assert!(!sent.contains("http-referer"), "non deve esserci senza configurazione: {sent}");
        assert!(!sent.contains("x-title"), "non deve esserci senza configurazione: {sent}");
    }

    // ── repair_invalid_json_escapes ─────────────────────────────────────────

    #[test]
    fn repair_leaves_already_valid_json_untouched() {
        // `\n` è uno dei 9 escape JSON validi: nessuna riparazione da fare, l'output deve
        // essere l'input, byte per byte.
        let input = r#"{"content":"a\nb"}"#;
        assert_eq!(repair_invalid_json_escapes(input), input);
    }

    #[test]
    fn repair_leaves_all_valid_escapes_untouched() {
        // I 9 escape validi JSON sono: \" \\ \/ \b \f \n \r \t \uXXXX. Questo test li
        // infila TUTTI in un unico valore stringa, più un carattere UTF-8 non-ASCII (é)
        // scritto letteralmente (JSON permette UTF-8 grezzo dentro le stringhe, non serve
        // escaparlo) — la guardia "non-over-fix" più importante: se il riparatore toccasse
        // anche solo uno di questi, romperebbe JSON già corretto.
        //
        // Costruito concatenando due letterali Rust per una ragione precisa (non solo
        // stilistica): una raw string `r#"..."#` (dove `\` NON è mai un carattere di
        // escape Rust, quindi ogni `\` che scriviamo lì è ESATTAMENTE il byte `\` che
        // arriverebbe sulla rete da OpenRouter — comoda per la parte con molti backslash
        // letterali) copre i primi 8 escape; il nono, `\uXXXX`, richiede un frammento
        // scritto come stringa Rust NORMALE (`"\\u0041é\"}"`), perché lì `\\` È
        // l'escape Rust per un backslash letterale — solo così il file sorgente
        // contiene DAVVERO i 6 caratteri letterali `\`,`u`,`0`,`0`,`4`,`1` (che
        // codificano la lettera 'A' quando `serde_json` li decodifica), invece di una
        // 'A' letterale che non eserciterebbe affatto l'escape `\u` del riparatore.
        let input: String =
            r#"{"content":"\n\\\/\"\b\f\r\t"#.to_string() + "\\u0041é\"}";
        let input: &str = &input;
        assert_eq!(
            repair_invalid_json_escapes(input),
            input,
            "nessuno dei 9 escape validi deve essere alterato"
        );
        // Verifica di sanità: l'input era già JSON valido (lo era prima di passare dal
        // riparatore, e lo studio sopra dimostra che rimane identico).
        assert!(serde_json::from_str::<serde_json::Value>(input).is_ok());
    }

    #[test]
    fn repair_doubles_stray_backslash_before_invalid_escape_char() {
        // `*` non è uno dei 9 escape JSON validi: `\*` dentro una stringa JSON grezza è un
        // errore di sintassi. Il modello intendeva quasi certamente l'escape Markdown
        // "asterisco letterale" (`\*`) e si è dimenticato che in JSON un backslash
        // letterale va raddoppiato (`\\*`). Il riparatore raddoppia il backslash orfano.
        let input = r#"{"content":"1\*2"}"#;
        let repaired = repair_invalid_json_escapes(input);
        assert_eq!(repaired, r#"{"content":"1\\*2"}"#);

        // La riparazione deve produrre JSON che PARSA DAVVERO, non solo "sembrare giusta".
        let parsed: serde_json::Value = serde_json::from_str(&repaired).unwrap();
        // Strato JSON: la stringa JSON `"1\\*2"` decodifica a 4 caratteri: '1', '\', '*', '2'
        // (il `\\` JSON è UN escape che produce UN backslash letterale).
        // Strato Rust: per scrivere quegli stessi 4 caratteri in un `"..."` Rust normale
        // (non raw), il backslash letterale va scritto come `\\` (escape Rust) — quindi
        // il literal Rust "1\\*2" (4 caratteri dopo l'escaping Rust: '1','\','*','2')
        // è il valore atteso. Due livelli di escaping diversi (JSON e Rust) che qui
        // coincidono nella stessa doppia-barra, ma per ragioni indipendenti.
        assert_eq!(parsed["content"], "1\\*2");
    }

    #[test]
    fn repair_does_not_desync_on_escaped_quote() {
        // Costruisce un caso dove sbagliare il tracking dentro/fuori-stringa produrrebbe
        // un risultato VISIBILMENTE diverso e sbagliato: il valore "a" contiene una
        // virgoletta ESCAPATA (`\"` — valida, NON va toccata) prima del valore "b", che a
        // sua volta contiene un escape orfano (`\*`, da riparare). Se il riparatore
        // scambiasse la `"` di `\"` per una vera chiusura di stringa (invece di
        // riconoscerla come parte della coppia di escape valida e saltarla), da quel
        // punto in poi crederebbe di essere FUORI stringa quando in realtà è dentro: il
        // `\*` nel valore "b" non verrebbe più riparato (rimarrebbe un escape orfano,
        // JSON invalido) — un fallimento silenzioso e diverso da quello atteso.
        //
        // JSON grezzo (bytes reali, vedi commento raw-string sopra):
        //   {"a":"x\"y","b":"z\*w"}
        let input = r#"{"a":"x\"y","b":"z\*w"}"#;
        let repaired = repair_invalid_json_escapes(input);

        let parsed: serde_json::Value = serde_json::from_str(&repaired)
            .expect("il tracking corretto deve produrre JSON valido, non desincronizzato");
        assert_eq!(parsed["a"], "x\"y", "l'escape valido \\\" non va toccato");
        assert_eq!(
            parsed["b"], "z\\*w",
            "il tracking deve restare corretto DOPO l'escape valido, riparando \\* in \"b\""
        );
    }

    #[test]
    fn repair_does_not_invent_a_closing_quote() {
        // Documento JSON troncato: manca la virgoletta di chiusura del valore (e la
        // graffa finale). Il riparatore raddoppia solo backslash orfani — non deve MAI
        // aggiungere caratteri che non c'erano (come una virgoletta di chiusura
        // "indovinata"): un documento troncato deve restare troncato, e quindi fallire
        // ancora il parsing dopo la riparazione. La riparazione è un tentativo di
        // recupero mirato, non un "auto-fix generico" del JSON.
        let input = r#"{"content":"unterminated"#;
        let repaired = repair_invalid_json_escapes(input);
        assert!(
            serde_json::from_str::<serde_json::Value>(&repaired).is_err(),
            "un documento troncato deve restare non parsabile dopo la riparazione: {repaired:?}"
        );
    }

    /// Integrazione reale: round-trip di tool-use COMPLETO contro OpenRouter/DeepSeek —
    /// richiede `OPENROUTER_API_KEY` e rete. Verifica che il tool-use funzioni DAVVERO
    /// in produzione (non solo che "torna del testo"): il modello chiama
    /// `run_in_session`, riceve un risultato fabbricato, e risponde in testo usandolo —
    /// stesso scenario già catturato nella fixture
    /// (`Docs/superpowers/fixtures/2026-07-06-deepseek-openrouter-real-roundtrip/`), qui
    /// con una chiamata di rete VERA al posto di `FakeOpenRouterClient`.
    #[ignore = "richiede OPENROUTER_API_KEY e rete"]
    #[tokio::test]
    async fn real_e2e_tool_use_roundtrip_against_deepseek() {
        let key =
            std::env::var("OPENROUTER_API_KEY").expect("imposta OPENROUTER_API_KEY per questo test");
        let http_client = Arc::new(HttpOpenRouterClient::new(key));
        let backend = OpenRouterBackend::new(http_client, "deepseek/deepseek-chat".to_string(), None);
        let tools = vec![ToolSpec::Custom(ToolDef {
            name: "run_in_session".to_string(),
            description: "Esegui un comando nella shell persistente e ricevi l'output.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string", "description": "Il comando da eseguire"}
                },
                "required": ["command"]
            }),
        })];

        let mut history = vec![Message::user_text(
            "Elenca i file nella cartella corrente usando lo strumento run_in_session.",
        )];
        let mut on_text_1 = |_: &str| {};
        let mut on_heartbeat = || {};
        let turn1 = backend
            .send_turn("", &tools, &history, &mut on_text_1, &mut on_heartbeat)
            .await
            .expect("turno 1 fallito");
        let tool_use_id = turn1
            .blocks
            .iter()
            .find_map(|b| match b {
                Block::ToolUse { id, name, .. } if name == "run_in_session" => Some(id.clone()),
                _ => None,
            })
            .expect("il modello deve chiamare run_in_session al turno 1");

        history.push(Message::with_blocks("assistant", turn1.blocks.clone()));
        history.push(Message::with_blocks(
            "user",
            vec![Block::ToolResult {
                tool_use_id,
                content: "Cargo.lock\nCargo.toml\ncrates\nDocs\ntarget".to_string(),
                is_error: false,
            }],
        ));

        let mut on_text_2 = |_: &str| {};
        let mut on_heartbeat = || {};
        let turn2 = backend
            .send_turn("", &tools, &history, &mut on_text_2, &mut on_heartbeat)
            .await
            .expect("turno 2 fallito");
        assert!(
            !turn2.text().trim().is_empty(),
            "il turno 2 deve rispondere in testo usando il risultato del tool"
        );
    }
}
