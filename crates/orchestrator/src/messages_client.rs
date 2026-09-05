//! # messages_client — seam Anthropic Messages API (DIP per testabilità)
//!
//! `ClaudeBackend` dipende da [`MessagesClient`] (il trait), non da `reqwest`.
//! Slice 2: blocchi di contenuto (text/tool_use/tool_result), tool, storia
//! conversazionale. `thinking` omesso. NON inviare `budget_tokens`/`temperature`.

use async_trait::async_trait;
use std::time::Duration;

#[cfg(test)]
use std::{collections::VecDeque, sync::Mutex};

/// Blocco di contenuto (richiesta e risposta). Wire Anthropic (`tag = "type"`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(default)]
        is_error: bool,
    },
    /// Blocchi non gestiti in risposta (es. `thinking`) — scartati.
    #[serde(other)]
    Unknown,
}

/// Un turno della conversazione.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Message {
    pub role: String,
    pub content: Vec<Block>,
}

impl Message {
    /// Turno `user` con un solo blocco di testo.
    pub fn user_text(input: &str) -> Self {
        Self {
            role: "user".to_string(),
            content: vec![Block::Text {
                text: input.to_string(),
            }],
        }
    }

    /// Turno con ruolo e blocchi arbitrari.
    pub fn with_blocks(role: &str, content: Vec<Block>) -> Self {
        Self {
            role: role.to_string(),
            content,
        }
    }
}

/// Storia conversazionale per-connessione (posseduta da `ws.rs`).
#[derive(Debug, Default)]
pub struct ConversationHistory {
    messages: Vec<Message>,
}

impl ConversationHistory {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn push(&mut self, m: Message) {
        self.messages.push(m);
    }
    pub fn to_vec(&self) -> Vec<Message> {
        self.messages.clone()
    }
    pub fn len(&self) -> usize {
        self.messages.len()
    }
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
    /// Tronca la storia ai primi `len` turni (rollback di uno scambio fallito).
    pub fn truncate(&mut self, len: usize) {
        self.messages.truncate(len);
    }
}

/// Config thinking (non usata in Slice 2: la richiesta la omette).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Thinking {
    #[serde(rename = "type")]
    pub kind: String,
}

/// Definizione di un tool esposto all'AI.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// Tool server-side Anthropic (es. web_search): forma `{type, name, max_uses?}`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ServerTool {
    #[serde(rename = "type")]
    pub kind: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_uses: Option<u32>,
}

/// Un tool nella richiesta: custom (eseguito da noi) o server-side (eseguito
/// da Anthropic). Serializzazione **untagged**: il custom produce
/// `{name,description,input_schema}`, il server `{type,name,max_uses}`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(untagged)]
pub enum ToolSpec {
    Custom(ToolDef),
    Server(ServerTool),
}

impl ToolSpec {
    /// Nome del tool (per asserzioni/diagnostica).
    pub fn name(&self) -> &str {
        match self {
            ToolSpec::Custom(t) => &t.name,
            ToolSpec::Server(s) => &s.name,
        }
    }
}

/// Corpo della richiesta a `POST /v1/messages`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct MessagesRequest {
    pub model: String,
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub system: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolSpec>,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<Thinking>,
}

impl MessagesRequest {
    /// Richiesta minima one-shot (un turno user; niente system/tools/thinking).
    /// Usata dal test di integrazione reale.
    pub fn one_shot(model: &str, max_tokens: u32, input: &str) -> Self {
        Self {
            model: model.to_string(),
            max_tokens,
            system: String::new(),
            tools: Vec::new(),
            messages: vec![Message::user_text(input)],
            thinking: None,
        }
    }
}

/// Dettagli stop (presenti quando `stop_reason=="refusal"`).
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct StopDetails {
    #[serde(default)]
    pub category: Option<String>,
}

/// Risposta della Messages API.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct MessagesResponse {
    pub content: Vec<Block>,
    #[serde(default)]
    pub stop_reason: Option<String>,
    #[serde(default)]
    pub stop_details: Option<StopDetails>,
}

impl MessagesResponse {
    /// Concatena i blocchi `text`.
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(|b| match b {
                Block::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }

    /// Blocchi per il turno `assistant` in storia: solo `text` + `tool_use`.
    pub fn assistant_blocks(&self) -> Vec<Block> {
        self.content
            .iter()
            .filter(|b| matches!(b, Block::Text { .. } | Block::ToolUse { .. }))
            .cloned()
            .collect()
    }
}

// ── Parser SSE (streaming) ─────────────────────────────────────────────────────

/// Stato di un content block durante lo streaming (assemblato a fine stream).
enum StreamBlock {
    Text { text: String },
    ToolUse { id: String, name: String, json: String },
    /// Blocchi non gestiti (es. `thinking`) — scartati a `finish`.
    Other,
}

impl StreamBlock {
    fn finish(self) -> Option<Block> {
        match self {
            StreamBlock::Text { text } => Some(Block::Text { text }),
            StreamBlock::ToolUse { id, name, json } => {
                let input = serde_json::from_str(&json)
                    .unwrap_or_else(|_| serde_json::Value::Object(Default::default()));
                Some(Block::ToolUse { id, name, input })
            }
            StreamBlock::Other => None,
        }
    }
}

/// Posizione della prima occorrenza di `needle` in `haystack`.
fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Accumulatore SSE: ingerisce byte grezzi (a confini arbitrari), isola gli
/// eventi completi (separati da `"\n\n"`), chiama `on_text` per ogni delta di
/// testo e assembla la `MessagesResponse` finale.
///
/// **Byte-framing:** i chunk di rete NON sono un-evento-per-chunk. Bufferizziamo
/// i byte e decodifichiamo solo gli eventi completi (UTF-8 valido: un carattere
/// multibyte spezzato a cavallo di due chunk resta nel resto del buffer).
struct SseAccumulator {
    buf: Vec<u8>,
    blocks: Vec<StreamBlock>,
    stop_reason: Option<String>,
    stop_details: Option<StopDetails>,
}

impl SseAccumulator {
    fn new() -> Self {
        Self {
            buf: Vec::new(),
            blocks: Vec::new(),
            stop_reason: None,
            stop_details: None,
        }
    }

    /// Ingerisce un chunk di byte; chiama `on_text` per ogni `text_delta` e
    /// `on_heartbeat` per ogni `ping` (keep-alive nativo di Anthropic) **e** per
    /// ogni `input_json_delta` (streaming dell'input di un tool-call: byte reali
    /// sul wire, quindi liveness al pari di un ping).
    fn feed(&mut self, bytes: &[u8], on_text: &mut dyn FnMut(&str), on_heartbeat: &mut dyn FnMut()) {
        self.buf.extend_from_slice(bytes);
        while let Some(pos) = find_subsequence(&self.buf, b"\n\n") {
            let event: Vec<u8> = self.buf.drain(..pos + 2).collect();
            // Un evento completo è UTF-8 valido (niente char spezzati dentro).
            let event = String::from_utf8_lossy(&event);
            for line in event.lines() {
                let data = match line.strip_prefix("data:") {
                    Some(d) => d.trim(),
                    None => continue, // "event:" / commenti / righe vuote
                };
                if data.is_empty() {
                    continue;
                }
                let v: serde_json::Value = match serde_json::from_str(data) {
                    Ok(v) => v,
                    Err(_) => continue, // evento non-JSON → ignora
                };
                self.handle_event(&v, on_text, on_heartbeat);
            }
        }
    }

    fn handle_event(&mut self, v: &serde_json::Value, on_text: &mut dyn FnMut(&str), on_heartbeat: &mut dyn FnMut()) {
        match v.get("type").and_then(|t| t.as_str()) {
            Some("content_block_start") => {
                let idx = v.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
                let cb = v.get("content_block");
                let kind = cb.and_then(|c| c.get("type")).and_then(|t| t.as_str());
                let block = match kind {
                    Some("text") => StreamBlock::Text {
                        text: String::new(),
                    },
                    Some("tool_use") => StreamBlock::ToolUse {
                        id: cb
                            .and_then(|c| c.get("id"))
                            .and_then(|s| s.as_str())
                            .unwrap_or("")
                            .to_string(),
                        name: cb
                            .and_then(|c| c.get("name"))
                            .and_then(|s| s.as_str())
                            .unwrap_or("")
                            .to_string(),
                        json: String::new(),
                    },
                    _ => StreamBlock::Other,
                };
                if idx >= self.blocks.len() {
                    self.blocks
                        .resize_with(idx + 1, || StreamBlock::Other);
                }
                self.blocks[idx] = block;
            }
            Some("content_block_delta") => {
                let idx = v.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
                let delta = v.get("delta");
                match delta.and_then(|d| d.get("type")).and_then(|t| t.as_str()) {
                    Some("text_delta") => {
                        if let Some(t) =
                            delta.and_then(|d| d.get("text")).and_then(|s| s.as_str())
                        {
                            on_text(t);
                            if let Some(StreamBlock::Text { text }) = self.blocks.get_mut(idx) {
                                text.push_str(t);
                            }
                        }
                    }
                    Some("input_json_delta") => {
                        // Il modello sta ancora producendo l'input di un tool (es. il
                        // contenuto di `show_markdown`): byte reali sul wire, quindi
                        // segnale di liveness al pari di un `ping` — vedi il test
                        // `input_json_delta_events_trigger_on_heartbeat_without_any_ping`.
                        on_heartbeat();
                        if let Some(p) = delta
                            .and_then(|d| d.get("partial_json"))
                            .and_then(|s| s.as_str())
                        {
                            if let Some(StreamBlock::ToolUse { json, .. }) =
                                self.blocks.get_mut(idx)
                            {
                                json.push_str(p);
                            }
                        }
                    }
                    _ => {} // thinking_delta ecc. ignorati
                }
            }
            Some("message_delta") => {
                let delta = v.get("delta");
                if let Some(sr) = delta
                    .and_then(|d| d.get("stop_reason"))
                    .and_then(|s| s.as_str())
                {
                    self.stop_reason = Some(sr.to_string());
                }
                if let Some(cat) = delta
                    .and_then(|d| d.get("stop_details"))
                    .and_then(|d| d.get("category"))
                    .and_then(|s| s.as_str())
                {
                    self.stop_details = Some(StopDetails {
                        category: Some(cat.to_string()),
                    });
                }
            }
            Some("ping") => on_heartbeat(),
            // message_start / content_block_stop / message_stop: niente.
            _ => {}
        }
    }

    fn finish(self) -> MessagesResponse {
        let content = self
            .blocks
            .into_iter()
            .filter_map(StreamBlock::finish)
            .collect();
        MessagesResponse {
            content,
            stop_reason: self.stop_reason,
            stop_details: self.stop_details,
        }
    }
}

/// Errori della chiamata Messages API.
#[derive(Debug, Clone, PartialEq)]
pub enum MessagesError {
    Http { status: u16, body: String },
    Network(String),
    Decode(String),
}

impl std::fmt::Display for MessagesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MessagesError::Http { status, body } => write!(f, "HTTP {status}: {body}"),
            MessagesError::Network(e) => write!(f, "rete: {e}"),
            MessagesError::Decode(e) => write!(f, "decodifica: {e}"),
        }
    }
}

/// Seam: la chiamata grezza alla Messages API.
#[async_trait]
pub trait MessagesClient: Send + Sync {
    async fn create(&self, req: MessagesRequest) -> Result<MessagesResponse, MessagesError>;

    /// Variante streaming: chiama `on_text` per ogni delta di testo man mano che
    /// arriva, e ritorna la `MessagesResponse` assemblata (come `create`).
    ///
    /// Default: fallback NON-streaming — chiama `create` ed emette tutto il testo
    /// come un unico delta. `HttpMessagesClient` fa l'override con SSE reale.
    ///
    /// La firma usa HRTB esplicito (`for<'a>`) perché `async_trait` riscrive le
    /// lifetime elise in lifetime nominate, distruggendo l'HRT implicito.
    async fn create_streaming(
        &self,
        req: MessagesRequest,
        on_text: &mut (dyn for<'a> FnMut(&'a str) + Send),
        on_heartbeat: &mut (dyn FnMut() + Send),
    ) -> Result<MessagesResponse, MessagesError> {
        let _ = on_heartbeat; // fallback non-streaming: nessun concetto di ping qui.
        let resp = self.create(req).await?;
        let text = resp.text();
        if !text.is_empty() {
            on_text(&text);
        }
        Ok(resp)
    }
}

// ── Impl reale (invariata dalla Slice 1) ──────────────────────────────────────

/// Timeout totale di BACKSTOP (connect+send+lettura completa del body). Molto generoso
/// apposta: il rilevatore rapido di stallo reale è `idle_timeout` (sotto); questo serve
/// solo a tagliare un hang veramente patologico (es. connessione che non risponde più a
/// nessun livello) senza uccidere una risposta legittima ma lunga.
const DEFAULT_TOTAL_TIMEOUT: Duration = Duration::from_secs(900);

/// Timeout di INATTIVITÀ durante lo streaming SSE: se non arriva NESSUN byte per questo
/// intervallo, la connessione è considerata bloccata. Deliberatamente più generoso del
/// vecchio timeout totale (120s, causa del bug "[errore AI] rete: error decoding response
/// body" del 2026-07-01): una singola risposta può includere più chiamate server-side
/// (`web_search`/`web_fetch`) con pause silenziose fra un delta di testo e l'altro mentre
/// Anthropic esegue il tool — non è uno stallo, è lavoro in corso.
const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(90);

/// Client reale: `reqwest`+rustls verso `POST {base_url}/v1/messages`.
pub struct HttpMessagesClient {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
    /// Vedi `DEFAULT_IDLE_TIMEOUT`. Applicato per-chunk in `create_streaming`.
    idle_timeout: Duration,
}

impl HttpMessagesClient {
    pub fn new(api_key: String) -> Self {
        Self::with_base_url(api_key, "https://api.anthropic.com".to_string())
    }

    pub fn with_base_url(api_key: String, base_url: String) -> Self {
        Self::with_timeouts(api_key, base_url, DEFAULT_IDLE_TIMEOUT, DEFAULT_TOTAL_TIMEOUT)
    }

    /// Costruttore completo: permette di iniettare timeout diversi dai default di
    /// produzione. Usato dai test per non dover aspettare minuti reali; `new`/
    /// `with_base_url` restano l'API pubblica con i default sensati.
    fn with_timeouts(
        api_key: String,
        base_url: String,
        idle_timeout: Duration,
        total_timeout: Duration,
    ) -> Self {
        let client = reqwest::Client::builder()
            .timeout(total_timeout)
            .build()
            .expect("failed to build reqwest client");
        Self {
            client,
            api_key,
            base_url,
            idle_timeout,
        }
    }
}

/// Formatta un `reqwest::Error` per `MessagesError::Network`, rendendo esplicito un
/// eventuale timeout. Senza questo, reqwest classifica un timeout scattato DURANTE la
/// lettura del body anche come `is_decode()==true` e il suo `Display` dice solo "error
/// decoding response body" — testo indistinguibile da una vera corruzione di rete (è
/// esattamente ciò che ha reso opaco il bug del 2026-07-01, vedi test sotto).
fn describe_network_error(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        format!("timeout: {e}")
    } else {
        e.to_string()
    }
}

#[async_trait]
impl MessagesClient for HttpMessagesClient {
    async fn create(&self, req: MessagesRequest) -> Result<MessagesResponse, MessagesError> {
        let url = format!("{}/v1/messages", self.base_url);
        let resp = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&req)
            .send()
            .await
            .map_err(|e| MessagesError::Network(describe_network_error(&e)))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(MessagesError::Http {
                status: status.as_u16(),
                body,
            });
        }

        resp.json::<MessagesResponse>()
            .await
            .map_err(|e| MessagesError::Decode(e.to_string()))
    }

    async fn create_streaming(
        &self,
        req: MessagesRequest,
        on_text: &mut (dyn for<'a> FnMut(&'a str) + Send),
        on_heartbeat: &mut (dyn FnMut() + Send),
    ) -> Result<MessagesResponse, MessagesError> {
        use futures_util::StreamExt;

        let url = format!("{}/v1/messages", self.base_url);
        // Aggiungi "stream": true senza inquinare MessagesRequest.
        let mut body =
            serde_json::to_value(&req).map_err(|e| MessagesError::Decode(e.to_string()))?;
        body["stream"] = serde_json::Value::Bool(true);

        let resp = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| MessagesError::Network(describe_network_error(&e)))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(MessagesError::Http {
                status: status.as_u16(),
                body,
            });
        }

        let mut acc = SseAccumulator::new();
        let mut stream = resp.bytes_stream();
        loop {
            // Timeout di INATTIVITÀ per-chunk (non totale): un turno con più chiamate
            // server-side (web_search/web_fetch) può avere pause silenziose legittime
            // fra un delta e l'altro — qui rileviamo solo l'ASSENZA PROLUNGATA di byte,
            // non la durata complessiva dello stream (quella la copre il timeout del
            // client `reqwest`, molto più generoso — vedi `DEFAULT_TOTAL_TIMEOUT`).
            let next = tokio::time::timeout(self.idle_timeout, stream.next())
                .await
                .map_err(|_elapsed| {
                    MessagesError::Network(format!(
                        "timeout: nessun dato ricevuto da oltre {}s (connessione probabilmente bloccata)",
                        self.idle_timeout.as_secs()
                    ))
                })?;
            let Some(chunk) = next else { break };
            let bytes = chunk.map_err(|e| MessagesError::Network(describe_network_error(&e)))?;
            acc.feed(&bytes, on_text, on_heartbeat);
        }
        Ok(acc.finish())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Fake (cfg(test), crate-visibile): sequenza o ripetizione; registra TUTTE le
// richieste (per asserire la forma del secondo invio nel loop).
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
pub(crate) struct FakeMessagesClient {
    requests: Mutex<Vec<MessagesRequest>>,
    responses: Mutex<VecDeque<Result<MessagesResponse, MessagesError>>>,
    repeat: bool,
}

#[cfg(test)]
impl FakeMessagesClient {
    /// Risponde in sequenza (una per chiamata); esaurita → `Err` di cortesia.
    pub(crate) fn sequence(responses: Vec<Result<MessagesResponse, MessagesError>>) -> Self {
        Self {
            requests: Mutex::new(Vec::new()),
            responses: Mutex::new(responses.into_iter().collect()),
            repeat: false,
        }
    }
    pub(crate) fn ok(resp: MessagesResponse) -> Self {
        Self::sequence(vec![Ok(resp)])
    }
    pub(crate) fn err(e: MessagesError) -> Self {
        Self::sequence(vec![Err(e)])
    }
    pub(crate) fn recorded(&self) -> MessagesRequest {
        self.requests
            .lock()
            .unwrap()
            .last()
            .cloned()
            .expect("nessuna richiesta registrata")
    }
}

#[cfg(test)]
#[async_trait]
impl MessagesClient for FakeMessagesClient {
    async fn create(&self, req: MessagesRequest) -> Result<MessagesResponse, MessagesError> {
        self.requests.lock().unwrap().push(req);
        let mut q = self.responses.lock().unwrap();
        if self.repeat {
            q.front()
                .cloned()
                .unwrap_or_else(|| Err(MessagesError::Network("fake: vuoto".to_string())))
        } else {
            q.pop_front()
                .unwrap_or_else(|| Err(MessagesError::Network("fake: niente più risposte".to_string())))
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_shot_builds_single_user_text_turn() {
        let req = MessagesRequest::one_shot("claude-sonnet-4-6", 16000, "ciao");
        assert_eq!(req.model, "claude-sonnet-4-6");
        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.messages[0].role, "user");
        assert_eq!(
            req.messages[0].content,
            vec![Block::Text {
                text: "ciao".to_string()
            }]
        );
        assert!(req.thinking.is_none());
    }

    #[test]
    fn request_serializes_to_correct_wire_shape() {
        // Forma della richiesta del loop (system + tools, niente thinking/sampling).
        let req = MessagesRequest {
            model: "claude-sonnet-4-6".to_string(),
            max_tokens: 16000,
            system: "SP".to_string(),
            tools: vec![ToolSpec::Custom(ToolDef {
                name: "run_in_session".to_string(),
                description: "d".to_string(),
                input_schema: serde_json::json!({"type":"object"}),
            })],
            messages: vec![Message::user_text("ciao")],
            thinking: None,
        };
        let v = serde_json::to_value(&req).unwrap();
        let obj = v.as_object().unwrap();
        assert_eq!(v["model"], "claude-sonnet-4-6");
        assert_eq!(v["max_tokens"], 16000);
        assert_eq!(v["system"], "SP");
        assert_eq!(v["tools"][0]["name"], "run_in_session");
        assert_eq!(v["messages"][0]["role"], "user");
        assert_eq!(v["messages"][0]["content"][0]["type"], "text");
        assert_eq!(v["messages"][0]["content"][0]["text"], "ciao");
        for forbidden in ["budget_tokens", "temperature", "top_p", "top_k", "thinking"] {
            assert!(!obj.contains_key(forbidden), "non deve contenere {forbidden}: {v}");
        }
    }

    #[test]
    fn block_tool_use_serializes_to_wire_shape() {
        let b = Block::ToolUse {
            id: "tu_1".to_string(),
            name: "run_in_session".to_string(),
            input: serde_json::json!({"command":"ls"}),
        };
        let v = serde_json::to_value(&b).unwrap();
        assert_eq!(v["type"], "tool_use");
        assert_eq!(v["id"], "tu_1");
        assert_eq!(v["name"], "run_in_session");
        assert_eq!(v["input"]["command"], "ls");
    }

    #[test]
    fn block_tool_result_serializes_to_wire_shape() {
        let b = Block::ToolResult {
            tool_use_id: "tu_1".to_string(),
            content: "file.txt".to_string(),
            is_error: false,
        };
        let v = serde_json::to_value(&b).unwrap();
        assert_eq!(v["type"], "tool_result");
        assert_eq!(v["tool_use_id"], "tu_1");
        assert_eq!(v["content"], "file.txt");
        assert_eq!(v["is_error"], false);
    }

    #[test]
    fn response_deserializes_text_and_tool_use_ignoring_unknown() {
        let json = r#"{
          "content": [
            {"type":"thinking","thinking":"..."},
            {"type":"tool_use","id":"tu_1","name":"run_in_session","input":{"command":"ls"}},
            {"type":"text","text":"fatto"}
          ],
          "stop_reason":"tool_use"
        }"#;
        let resp: MessagesResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
        let blocks = resp.assistant_blocks(); // scarta lo unknown (thinking)
        assert_eq!(blocks.len(), 2);
        assert!(matches!(&blocks[0], Block::ToolUse { name, .. } if name == "run_in_session"));
        assert!(matches!(&blocks[1], Block::Text { text } if text == "fatto"));
        assert_eq!(resp.text(), "fatto");
    }

    #[test]
    fn response_text_concatenates_text_blocks() {
        let resp = MessagesResponse {
            content: vec![
                Block::Text {
                    text: "Ciao".to_string(),
                },
                Block::Text {
                    text: " mondo".to_string(),
                },
            ],
            stop_reason: Some("end_turn".to_string()),
            stop_details: None,
        };
        assert_eq!(resp.text(), "Ciao mondo");
    }

    #[test]
    fn error_display_is_human_readable() {
        let e = MessagesError::Http {
            status: 401,
            body: "unauthorized".to_string(),
        };
        assert_eq!(e.to_string(), "HTTP 401: unauthorized");
    }

    /// Integrazione reale: richiede `ANTHROPIC_API_KEY` e rete.
    #[ignore = "richiede ANTHROPIC_API_KEY e rete"]
    #[tokio::test]
    async fn real_messages_api_returns_text() {
        let key =
            std::env::var("ANTHROPIC_API_KEY").expect("imposta ANTHROPIC_API_KEY per questo test");
        let model = std::env::var("LARE_AI_MODEL")
            .unwrap_or_else(|_| "claude-sonnet-4-6".to_string());
        let client = HttpMessagesClient::new(key);
        let req = MessagesRequest::one_shot(&model, 1024, "Rispondi solo con: pong");
        let resp = client.create(req).await.expect("chiamata API fallita");
        assert!(!resp.text().is_empty(), "testo vuoto; stop_reason={:?}", resp.stop_reason);
    }

    // ── SseAccumulator ────────────────────────────────────────────────────────

    /// Una risposta SSE testuale completa (text block) con stop_reason.
    fn sse_text_sample() -> &'static str {
        "event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"id\":\"m\"}}\n\
\n\
event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\
\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Ciao \"}}\n\
\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"mondo\"}}\n\
\n\
event: content_block_stop\n\
data: {\"type\":\"content_block_stop\",\"index\":0}\n\
\n\
event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\
\n\
event: message_stop\n\
data: {\"type\":\"message_stop\"}\n\
\n"
    }

    #[test]
    fn sse_parses_text_in_one_chunk() {
        let mut acc = SseAccumulator::new();
        let mut deltas = String::new();
        acc.feed(sse_text_sample().as_bytes(), &mut |s| deltas.push_str(s), &mut || {});
        let resp = acc.finish();
        assert_eq!(deltas, "Ciao mondo");
        assert_eq!(resp.text(), "Ciao mondo");
        assert_eq!(resp.stop_reason.as_deref(), Some("end_turn"));
    }

    #[test]
    fn sse_byte_framing_one_byte_at_a_time() {
        // Stessi byte, alimentati UNO alla volta: il byte-framing deve reggere.
        let mut acc = SseAccumulator::new();
        let mut deltas = String::new();
        for b in sse_text_sample().as_bytes() {
            acc.feed(&[*b], &mut |s| deltas.push_str(s), &mut || {});
        }
        let resp = acc.finish();
        assert_eq!(deltas, "Ciao mondo");
        assert_eq!(resp.text(), "Ciao mondo");
        assert_eq!(resp.stop_reason.as_deref(), Some("end_turn"));
    }

    #[test]
    fn sse_byte_framing_multibyte_split_across_chunks() {
        // Un text_delta con un carattere multibyte (€ = 3 byte) spezzato a metà
        // tra due chunk: decodificando solo eventi COMPLETI non si corrompe.
        let event = "event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\
\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"prezzo 5€\"}}\n\
\n";
        let bytes = event.as_bytes();
        // Trova un punto di taglio in mezzo ai byte di '€' (l'ultimo char prima di "\n\n").
        let split = bytes.len() - 4; // dentro la sequenza UTF-8 di €
        let mut acc = SseAccumulator::new();
        let mut deltas = String::new();
        acc.feed(&bytes[..split], &mut |s| deltas.push_str(s), &mut || {});
        acc.feed(&bytes[split..], &mut |s| deltas.push_str(s), &mut || {});
        assert_eq!(deltas, "prezzo 5€");
    }

    #[test]
    fn sse_accumulates_tool_use_json_across_deltas() {
        let sse = "event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"tu_1\",\"name\":\"run_in_session\",\"input\":{}}}\n\
\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"command\\\":\"}}\n\
\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"ls\\\"}\"}}\n\
\n\
event: content_block_stop\n\
data: {\"type\":\"content_block_stop\",\"index\":0}\n\
\n\
event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"}}\n\
\n";
        let mut acc = SseAccumulator::new();
        acc.feed(sse.as_bytes(), &mut |_| {}, &mut || {});
        let resp = acc.finish();
        assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
        let blocks = resp.assistant_blocks();
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            Block::ToolUse { id, name, input } => {
                assert_eq!(id, "tu_1");
                assert_eq!(name, "run_in_session");
                assert_eq!(input["command"], "ls");
            }
            other => panic!("atteso ToolUse, trovato {other:?}"),
        }
    }

    #[test]
    fn sse_parses_refusal_stop_reason() {
        let sse = "event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"refusal\",\"stop_details\":{\"category\":\"cyber\"}}}\n\
\n";
        let mut acc = SseAccumulator::new();
        acc.feed(sse.as_bytes(), &mut |_| {}, &mut || {});
        let resp = acc.finish();
        assert_eq!(resp.stop_reason.as_deref(), Some("refusal"));
        assert_eq!(
            resp.stop_details.and_then(|d| d.category).as_deref(),
            Some("cyber")
        );
    }

    #[test]
    fn toolspec_server_serializes_type_name_maxuses() {
        let s = ToolSpec::Server(ServerTool {
            kind: "web_search_20260209".to_string(),
            name: "web_search".to_string(),
            max_uses: Some(5),
        });
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["type"], "web_search_20260209");
        assert_eq!(v["name"], "web_search");
        assert_eq!(v["max_uses"], 5);
        assert!(v.get("description").is_none(), "server tool non ha description");
    }

    #[test]
    fn toolspec_custom_serializes_name_desc_schema() {
        let c = ToolSpec::Custom(ToolDef {
            name: "run_in_session".to_string(),
            description: "d".to_string(),
            input_schema: serde_json::json!({"type":"object"}),
        });
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(v["name"], "run_in_session");
        assert_eq!(v["description"], "d");
        assert!(v.get("type").is_none(), "custom tool non ha type");
    }

    #[test]
    fn toolspec_name_accessor() {
        let c = ToolSpec::Custom(ToolDef {
            name: "x".to_string(), description: "".to_string(),
            input_schema: serde_json::json!({}),
        });
        assert_eq!(c.name(), "x");
    }

    /// Integrazione reale streaming: richiede `ANTHROPIC_API_KEY` e rete.
    #[ignore = "richiede ANTHROPIC_API_KEY e rete"]
    #[tokio::test]
    async fn real_streaming_invokes_on_text() {
        let key =
            std::env::var("ANTHROPIC_API_KEY").expect("imposta ANTHROPIC_API_KEY per questo test");
        let model =
            std::env::var("LARE_AI_MODEL").unwrap_or_else(|_| "claude-sonnet-4-6".to_string());
        let client = HttpMessagesClient::new(key);
        let req = MessagesRequest::one_shot(&model, 256, "Conta lentamente da 1 a 5.");
        let mut calls = 0usize;
        let mut text = String::new();
        let resp = client
            .create_streaming(
                req,
                &mut |s| {
                    calls += 1;
                    text.push_str(s);
                },
                &mut || {},
            )
            .await
            .expect("chiamata streaming fallita");
        assert!(calls >= 1, "on_text mai invocato");
        assert!(!text.is_empty(), "nessun testo ricevuto in streaming");
        assert_eq!(resp.text(), text, "la risposta assemblata deve combaciare coi delta");
    }

    // -------------------------------------------------------------------------
    // Scenario di Test: idle-timeout vs total-timeout nello streaming SSE
    // -------------------------------------------------------------------------
    //
    // Bug segnalato dall'utente (2026-07-01): risposte AI lunghe (ricerca web +
    // generazione esaustiva) fallivano con "[errore AI] rete: error decoding response
    // body" — il `.timeout()` totale del client reqwest (120s, connect+send+lettura
    // COMPLETA del body) scadeva a metà stream. reqwest classifica un timeout durante
    // la lettura del body come `is_decode()==true` OLTRE che `is_timeout()==true`, e
    // il codice catturava solo `e.to_string()` (il testo "decode"), perdendo il segnale
    // di timeout. Fix: timeout di INATTIVITÀ per-chunk (rileva stalli veri in fretta,
    // generoso abbastanza da non scattare durante una pausa silenziosa per tool-use
    // server-side) + timeout totale di backstop molto più alto (solo per hang patologici).

    /// Un server locale che manda UN chunk poi tace (senza chiudere la connessione)
    /// oltre l'idle-timeout, ma entro il total-timeout: deve fallire con un errore
    /// che PARLA ESPLICITAMENTE di timeout — non il testo opaco "error decoding
    /// response body" del bug originale.
    #[tokio::test]
    async fn idle_timeout_produces_clear_error_when_stream_goes_quiet() {
        use tokio::io::AsyncWriteExt;
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await;
            let head = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n";
            let _ = stream.write_all(head.as_bytes()).await;
            let _ = stream.flush().await;
            // Tace oltre l'idle-timeout (100ms) ma resta entro il total-timeout (5s):
            // la connessione NON è chiusa, semplicemente non arriva altro byte.
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        });

        let client = HttpMessagesClient::with_timeouts(
            "fake-key".to_string(),
            format!("http://{addr}"),
            std::time::Duration::from_millis(100),
            std::time::Duration::from_secs(5),
        );

        let req = MessagesRequest::one_shot("claude-sonnet-4-6", 16, "ciao");
        let err = client
            .create_streaming(req, &mut |_s| {}, &mut || {})
            .await
            .expect_err("deve fallire: nessun dato oltre l'idle-timeout");
        let msg = err.to_string();
        assert!(
            msg.to_lowercase().contains("timeout"),
            "il messaggio deve parlare esplicitamente di timeout, non di 'decoding': {msg:?}"
        );
        assert!(
            !msg.contains("decoding"),
            "non deve più mostrare il testo opaco di reqwest: {msg:?}"
        );
    }

    /// Uno stream che manda chunk regolari con pause SOTTO l'idle-timeout, ma la cui
    /// durata TOTALE supera quello che sarebbe stato il vecchio timeout-totale-corto:
    /// deve comunque completare con successo — è lavoro attivo, non uno stallo.
    #[tokio::test]
    async fn steady_trickle_within_idle_window_succeeds_despite_long_total_duration() {
        use tokio::io::AsyncWriteExt;
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await;
            let head = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n";
            let _ = stream.write_all(head.as_bytes()).await;
            // 6 chunk, ciascuno seguito da una pausa di 150ms (< idle-timeout 300ms).
            // Durata totale ~750ms: avrebbe fallito con un ipotetico timeout-totale-corto
            // (es. 500ms), ma qui il total-timeout è generoso (5s) e l'idle non scatta mai.
            for i in 0..6 {
                let word = format!("tick{i} ");
                let chunked = format!("{:x}\r\n{word}\r\n", word.len());
                let _ = stream.write_all(chunked.as_bytes()).await;
                let _ = stream.flush().await;
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            }
            let _ = stream.write_all(b"0\r\n\r\n").await;
            let _ = stream.flush().await;
        });

        let client = HttpMessagesClient::with_timeouts(
            "fake-key".to_string(),
            format!("http://{addr}"),
            std::time::Duration::from_millis(300),
            std::time::Duration::from_secs(5),
        );

        let req = MessagesRequest::one_shot("claude-sonnet-4-6", 16, "ciao");
        let mut received = String::new();
        client
            .create_streaming(req, &mut |s| received.push_str(s), &mut || {})
            .await
            .expect("non deve fallire: ogni pausa è sotto l'idle-timeout");
        // Il body grezzo non è SSE valido (niente "data: {...}\n\n"), quindi
        // SseAccumulator non produce delta di testo — ciò che importa qui è che
        // NON sia tornato un errore di timeout: lo stream è stato letto per intero.
        assert!(received.is_empty(), "body non-SSE: nessun delta atteso, solo nessun errore");
    }

    /// L'evento SSE `ping` (keep-alive nativo di Anthropic) deve far scattare
    /// `on_heartbeat` e NON deve mai comparire come testo in `on_text` — vedi
    /// Docs/superpowers/specs/2026-08-07-ai-turn-heartbeat-watchdog-design.md §2.
    #[tokio::test]
    async fn ping_event_triggers_on_heartbeat_not_on_text() {
        use tokio::io::AsyncWriteExt;
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await;
            let sse_body = concat!(
                "event: content_block_start\n",
                "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
                "event: content_block_delta\n",
                "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"ciao\"}}\n\n",
                "event: ping\n",
                "data: {\"type\":\"ping\"}\n\n",
                "event: content_block_delta\n",
                "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\" mondo\"}}\n\n",
                "event: message_stop\n",
                "data: {\"type\":\"message_stop\"}\n\n",
            );
            let head = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n";
            let chunked = format!("{:x}\r\n{sse_body}\r\n0\r\n\r\n", sse_body.len());
            let _ = stream.write_all(head.as_bytes()).await;
            let _ = stream.write_all(chunked.as_bytes()).await;
            let _ = stream.flush().await;
        });

        let client = HttpMessagesClient::with_base_url("fake-key".to_string(), format!("http://{addr}"));
        let req = MessagesRequest::one_shot("claude-sonnet-4-6", 16, "ciao");
        let mut text = String::new();
        let mut heartbeats = 0usize;
        let resp = client
            .create_streaming(req, &mut |s| text.push_str(s), &mut || heartbeats += 1)
            .await
            .expect("non deve fallire");

        assert_eq!(heartbeats, 1, "il singolo evento ping deve scatenare on_heartbeat esattamente una volta");
        assert_eq!(text, "ciao mondo", "il ping non deve comparire nel testo assemblato");
        assert_eq!(resp.text(), "ciao mondo");
    }

    /// Un tool-call lungo (es. `show_markdown` con un contenuto esteso) arriva sul
    /// wire come una sequenza di eventi `input_json_delta`, SENZA alcun `ping` — è
    /// comunque attività reale del modello che sta ancora producendo. Deve far
    /// scattare `on_heartbeat` una volta per delta, esattamente come farebbe un
    /// `ping`, altrimenti un turno che streamma solo tool-input (zero testo, zero
    /// ping) appare "silenzioso" al watchdog anche se il modello sta lavorando.
    /// Vedi Docs/superpowers/specs/2026-08-07-ai-turn-heartbeat-watchdog-design.md §0/§1.
    #[tokio::test]
    async fn input_json_delta_events_trigger_on_heartbeat_without_any_ping() {
        use tokio::io::AsyncWriteExt;
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await;
            let sse_body = concat!(
                "event: content_block_start\n",
                "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"show_markdown\"}}\n\n",
                "event: content_block_delta\n",
                "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"con\"}}\n\n",
                "event: content_block_delta\n",
                "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"tent\\\":\"}}\n\n",
                "event: content_block_delta\n",
                "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"ciao\\\"}\"}}\n\n",
                "event: message_stop\n",
                "data: {\"type\":\"message_stop\"}\n\n",
            );
            let head = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n";
            let chunked = format!("{:x}\r\n{sse_body}\r\n0\r\n\r\n", sse_body.len());
            let _ = stream.write_all(head.as_bytes()).await;
            let _ = stream.write_all(chunked.as_bytes()).await;
            let _ = stream.flush().await;
        });

        let client = HttpMessagesClient::with_base_url("fake-key".to_string(), format!("http://{addr}"));
        let req = MessagesRequest::one_shot("claude-sonnet-4-6", 16, "ciao");
        let mut text = String::new();
        let mut heartbeats = 0usize;
        let _resp = client
            .create_streaming(req, &mut |s| text.push_str(s), &mut || heartbeats += 1)
            .await
            .expect("non deve fallire");

        assert_eq!(
            heartbeats, 3,
            "3 eventi input_json_delta, zero ping: on_heartbeat deve scattare una volta per delta"
        );
        assert!(text.is_empty(), "nessun text_delta in questo stream: on_text non deve mai scattare");
    }
}
