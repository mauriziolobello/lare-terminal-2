//! # claude_backend — `ChatBackend` per Anthropic (wraps `MessagesClient`)
//!
//! Quasi un passthrough: `Block`/`ToolSpec` SONO già la forma wire Anthropic
//! (vedi `messages_client.rs`), quindi non c'è vera "traduzione" qui — solo
//! l'assemblaggio di `MessagesRequest` e la mappatura di `stop_reason` nel
//! vocabolario neutro `TurnStop`. Confronta con un futuro backend
//! OpenAI-compatibile, dove la traduzione è sostanziale.

use std::sync::Arc;

use async_trait::async_trait;

use crate::chat_backend::{BackendError, BackendTurn, ChatBackend, TurnStop};
use crate::messages_client::{Message, MessagesClient, MessagesError, MessagesRequest, ToolSpec};

/// Nome del tool server-side `web_fetch` così come appare in `ServerTool::name`
/// (vedi `agent::web_tools`). Usato per filtrarlo quando l'endpoint non lo supporta.
const WEB_FETCH_TOOL_NAME: &str = "web_fetch";

impl From<MessagesError> for BackendError {
    fn from(e: MessagesError) -> Self {
        match e {
            MessagesError::Http { status, body } => BackendError::Http { status, body },
            MessagesError::Network(s) => BackendError::Network(s),
            MessagesError::Decode(s) => BackendError::Decode(s),
        }
    }
}

/// `ChatBackend` che parla la Anthropic Messages API tramite un
/// `Arc<dyn MessagesClient>` (lo stesso seam usato oggi da `HttpMessagesClient`).
pub struct ClaudeBackend {
    messages: Arc<dyn MessagesClient>,
    model: String,
    max_tokens: u32,
    /// `true` se l'endpoint capisce il tool server-side `web_fetch` (Anthropic reale:
    /// sempre; endpoint Anthropic-COMPATIBILI di terzi come DeepSeek: non è garantito —
    /// vedi `with_web_fetch_supported`). Default `true` via `new`, non cambia il
    /// comportamento verso Anthropic reale.
    web_fetch_supported: bool,
}

impl ClaudeBackend {
    pub fn new(messages: Arc<dyn MessagesClient>, model: String, max_tokens: u32) -> Self {
        Self { messages, model, max_tokens, web_fetch_supported: true }
    }

    /// Builder: disattiva l'inclusione del tool server-side `web_fetch` nella richiesta.
    ///
    /// Serve per gli endpoint Anthropic-COMPATIBILI (`base_url` custom in `llms.json`,
    /// es. DeepSeek) che parlano lo stesso wire `/v1/messages` ma non implementano
    /// l'intera superficie Anthropic: DeepSeek conosce solo i tipi `web_search_*` e
    /// risponde HTTP 400 ("unknown variant web_fetch_20260209") se lo si include —
    /// bug reale osservato in produzione (ogni comando in linguaggio naturale falliva,
    /// perché la ricerca web è attiva di default e aggiunge SEMPRE entrambi i tool
    /// server-side, vedi `agent::web_tools`). `web_search` resta incluso: è l'unico dei
    /// due che DeepSeek dichiara di supportare nel messaggio d'errore.
    pub fn with_web_fetch_supported(mut self, supported: bool) -> Self {
        self.web_fetch_supported = supported;
        self
    }
}

#[async_trait]
impl ChatBackend for ClaudeBackend {
    async fn send_turn(
        &self,
        system: &str,
        tools: &[ToolSpec],
        history: &[Message],
        on_text: &mut (dyn for<'a> FnMut(&'a str) + Send),
        on_heartbeat: &mut (dyn FnMut() + Send),
    ) -> Result<BackendTurn, BackendError> {
        // `agent::tools_for` è provider-agnostico: non sa se l'endpoint attivo capisce
        // `web_fetch`. Il filtro vive qui, nel layer di traduzione specifico di Claude —
        // stesso punto in cui `OpenRouterBackend::to_or_tools` scarta `ToolSpec::Server`
        // che non ha equivalente OpenAI (vedi commento lì): capacità diverse, stesso posto.
        let sent_tools: Vec<ToolSpec> = if self.web_fetch_supported {
            tools.to_vec()
        } else {
            tools
                .iter()
                .filter(|t| t.name() != WEB_FETCH_TOOL_NAME)
                .cloned()
                .collect()
        };
        let req = MessagesRequest {
            model: self.model.clone(),
            max_tokens: self.max_tokens,
            system: system.to_string(),
            tools: sent_tools,
            messages: history.to_vec(),
            thinking: None,
        };
        let resp = self.messages.create_streaming(req, on_text, on_heartbeat).await?;
        let stop = match resp.stop_reason.as_deref() {
            Some("refusal") => TurnStop::Refused {
                category: resp.stop_details.as_ref().and_then(|d| d.category.clone()),
            },
            Some("max_tokens") => TurnStop::MaxTokens,
            _ => TurnStop::Normal,
        };
        Ok(BackendTurn { blocks: resp.assistant_blocks(), stop })
    }
}

#[cfg(test)]
mod tests {
    use crate::chat_backend::{BackendError, ChatBackend, TurnStop};
    use crate::claude_backend::ClaudeBackend;
    use crate::messages_client::{
        Block, FakeMessagesClient, Message, MessagesError, MessagesResponse, ServerTool,
        StopDetails, ToolDef, ToolSpec,
    };
    use std::sync::Arc;

    fn text_response(text: &str) -> MessagesResponse {
        MessagesResponse {
            content: vec![Block::Text { text: text.to_string() }],
            stop_reason: Some("end_turn".to_string()),
            stop_details: None,
        }
    }

    #[tokio::test]
    async fn forwards_system_tools_and_history_into_messages_request() {
        let fake = Arc::new(FakeMessagesClient::ok(text_response("ok")));
        let backend = ClaudeBackend::new(fake.clone(), "claude-sonnet-4-6".to_string(), 16000);
        let tools = vec![ToolSpec::Custom(ToolDef {
            name: "run_in_session".to_string(),
            description: "d".to_string(),
            input_schema: serde_json::json!({"type":"object"}),
        })];
        let history = vec![Message::user_text("ciao")];
        let mut on_text = |_: &str| {};
        let mut on_heartbeat = || {};

        backend
            .send_turn("sistema", &tools, &history, &mut on_text, &mut on_heartbeat)
            .await
            .unwrap();

        let req = fake.recorded();
        assert_eq!(req.model, "claude-sonnet-4-6");
        assert_eq!(req.max_tokens, 16000);
        assert_eq!(req.system, "sistema");
        assert_eq!(req.tools, tools);
        assert_eq!(req.messages, history);
    }

    /// Riproduce il bug DeepSeek: HTTP 400 "unknown variant web_fetch_20260209" perché
    /// il loro endpoint Anthropic-compatibile non implementa `web_fetch`, solo
    /// `web_search`. `with_web_fetch_supported(false)` deve togliere SOLO `web_fetch`
    /// dalla richiesta effettivamente inviata, lasciando `web_search` intatto.
    #[tokio::test]
    async fn strips_web_fetch_server_tool_when_not_supported() {
        let fake = Arc::new(FakeMessagesClient::ok(text_response("ok")));
        let backend = ClaudeBackend::new(fake.clone(), "deepseek-v4-pro".to_string(), 16000)
            .with_web_fetch_supported(false);
        let tools = vec![
            ToolSpec::Server(ServerTool {
                kind: "web_search_20260209".to_string(),
                name: "web_search".to_string(),
                max_uses: Some(5),
            }),
            ToolSpec::Server(ServerTool {
                kind: "web_fetch_20260209".to_string(),
                name: "web_fetch".to_string(),
                max_uses: Some(5),
            }),
        ];
        let mut on_text = |_: &str| {};
        let mut on_heartbeat = || {};

        backend
            .send_turn("sistema", &tools, &[], &mut on_text, &mut on_heartbeat)
            .await
            .unwrap();

        let req = fake.recorded();
        assert!(
            req.tools.iter().any(|t| t.name() == "web_search"),
            "web_search deve restare: DeepSeek lo supporta"
        );
        assert!(
            !req.tools.iter().any(|t| t.name() == "web_fetch"),
            "web_fetch va scartato: causava HTTP 400 su DeepSeek"
        );
    }

    /// Comportamento di default (nessuna chiamata a `with_web_fetch_supported`, es.
    /// Anthropic reale): entrambi i tool server-side passano invariati, come prima
    /// di questo fix — non deve rompersi nulla per `claude-direct`.
    #[tokio::test]
    async fn forwards_web_fetch_when_supported_by_default() {
        let fake = Arc::new(FakeMessagesClient::ok(text_response("ok")));
        let backend = ClaudeBackend::new(fake.clone(), "claude-sonnet-4-6".to_string(), 16000);
        let tools = vec![ToolSpec::Server(ServerTool {
            kind: "web_fetch_20260209".to_string(),
            name: "web_fetch".to_string(),
            max_uses: Some(5),
        })];
        let mut on_text = |_: &str| {};
        let mut on_heartbeat = || {};

        backend
            .send_turn("sistema", &tools, &[], &mut on_text, &mut on_heartbeat)
            .await
            .unwrap();

        assert!(fake.recorded().tools.iter().any(|t| t.name() == "web_fetch"));
    }

    #[tokio::test]
    async fn maps_refusal_stop_reason_to_turnstop_refused() {
        let mut resp = text_response("");
        resp.stop_reason = Some("refusal".to_string());
        resp.stop_details = Some(StopDetails { category: Some("cyber".to_string()) });
        let fake = Arc::new(FakeMessagesClient::ok(resp));
        let backend = ClaudeBackend::new(fake, "claude-sonnet-4-6".to_string(), 16000);
        let mut on_text = |_: &str| {};
        let mut on_heartbeat = || {};

        let turn = backend
            .send_turn("sistema", &[], &[], &mut on_text, &mut on_heartbeat)
            .await
            .unwrap();

        assert_eq!(turn.stop, TurnStop::Refused { category: Some("cyber".to_string()) });
    }

    #[tokio::test]
    async fn maps_max_tokens_stop_reason() {
        let mut resp = text_response("troncato");
        resp.stop_reason = Some("max_tokens".to_string());
        let fake = Arc::new(FakeMessagesClient::ok(resp));
        let backend = ClaudeBackend::new(fake, "claude-sonnet-4-6".to_string(), 16000);
        let mut on_text = |_: &str| {};
        let mut on_heartbeat = || {};

        let turn = backend
            .send_turn("sistema", &[], &[], &mut on_text, &mut on_heartbeat)
            .await
            .unwrap();

        assert_eq!(turn.stop, TurnStop::MaxTokens);
        assert_eq!(turn.text(), "troncato");
    }

    #[tokio::test]
    async fn network_error_maps_to_backend_error() {
        let fake = Arc::new(FakeMessagesClient::err(MessagesError::Network("boom".to_string())));
        let backend = ClaudeBackend::new(fake, "claude-sonnet-4-6".to_string(), 16000);
        let mut on_text = |_: &str| {};
        let mut on_heartbeat = || {};

        let err = backend
            .send_turn("sistema", &[], &[], &mut on_text, &mut on_heartbeat)
            .await
            .unwrap_err();

        assert_eq!(err, BackendError::Network("boom".to_string()));
    }
}
