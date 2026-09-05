//! # chat_backend — seam di traduzione cross-provider per il tool-use (ADR-014)
//!
//! `ChatBackend` è il punto di sostituzione fra provider LLM: `LlmAdapter`
//! (in `ai_adapter.rs`) guida il loop di tool-use UNA sola volta, chiamando
//! `send_turn` a ogni iterazione — Claude e (in uno slice successivo)
//! OpenRouter differiscono SOLO nell'implementazione di questo trait, non
//! nel loop che lo consuma. Vedi
//! `Docs/superpowers/specs/2026-07-06-openrouter-tooluse-adapter-design.md`.

use async_trait::async_trait;

use crate::messages_client::{Block, Message, ToolSpec};

#[cfg(test)]
use std::{collections::VecDeque, sync::Mutex};

/// Esito di un turno, con vocabolario NEUTRO — non le stringhe `stop_reason`
/// (Anthropic) / `finish_reason` (OpenAI) specifiche del provider. Ogni
/// backend mappa il proprio vocabolario in questo enum.
#[derive(Debug, Clone, PartialEq)]
pub enum TurnStop {
    Normal,
    Refused { category: Option<String> },
    MaxTokens,
}

/// Risultato di un turno: blocchi (solo `Block::Text`/`Block::ToolUse` — mai
/// `ToolResult`, che appare solo nella storia INVIATA, non in una risposta
/// ricevuta) + esito.
#[derive(Debug, Clone, PartialEq)]
pub struct BackendTurn {
    pub blocks: Vec<Block>,
    pub stop: TurnStop,
}

impl BackendTurn {
    /// Concatena i blocchi `Text` — mirror di `MessagesResponse::text()`
    /// in `messages_client.rs`.
    pub fn text(&self) -> String {
        self.blocks
            .iter()
            .filter_map(|b| match b {
                Block::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }
}

/// Errori di trasporto/decodifica di un backend. Mirror di `MessagesError`
/// (`messages_client.rs`) — `Decode` copre anche un caso nuovo introdotto da
/// un futuro backend OpenAI-compatibile: `tool_calls[].function.arguments`
/// è una STRINGA JSON che può fallire il parsing (Anthropic non ha questo
/// problema: `input` è già un oggetto nativo).
#[derive(Debug, Clone, PartialEq)]
pub enum BackendError {
    Http { status: u16, body: String },
    Network(String),
    Decode(String),
}

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackendError::Http { status, body } => write!(f, "HTTP {status}: {body}"),
            BackendError::Network(e) => write!(f, "rete: {e}"),
            BackendError::Decode(e) => write!(f, "decodifica: {e}"),
        }
    }
}

/// Seam: un provider capace di condurre un turno di conversazione (con o
/// senza tool). `ClaudeBackend` (`claude_backend.rs`) è la prima
/// implementazione concreta.
#[async_trait]
pub trait ChatBackend: Send + Sync {
    /// `tools`: `&[ToolSpec]`, NON `&[ToolDef]` — include anche eventuali
    /// tool server-side Anthropic (`ToolSpec::Server`, es. `web_search`) che
    /// un backend non-Anthropic non può eseguire e dovrà scartare per conto
    /// proprio (vedi Global Constraints).
    /// `history`: storia Block-shaped esistente (`ConversationHistory::to_vec()`),
    /// invariata.
    /// `on_text`: un backend streaming la chiama a ogni delta reale; un
    /// backend non-streaming la chiama UNA sola volta con tutto il testo,
    /// appena prima di ritornare.
    /// `on_heartbeat`: segnale di liveness SENZA payload — un backend
    /// streaming lo chiama ogni volta che osserva attività reale sul wire
    /// ("ancora al lavoro"), non solo sui delta di testo che passano per
    /// `on_text` (es. `ping` di keep-alive o lo streaming dell'input di un
    /// tool-call — vedi `ClaudeBackend`/`SseAccumulator` in
    /// `messages_client.rs`). Il chiamante lo usa per resettare un watchdog
    /// di inattività senza dover interpretare il contenuto del turno. Un
    /// backend non-streaming (una singola chiamata HTTP bloccante, senza
    /// eventi intermedi da riportare) può non chiamarlo mai durante la
    /// chiamata — vedi `OpenRouterBackend`, REST one-shot.
    async fn send_turn(
        &self,
        system: &str,
        tools: &[ToolSpec],
        history: &[Message],
        on_text: &mut (dyn for<'a> FnMut(&'a str) + Send),
        on_heartbeat: &mut (dyn FnMut() + Send),
    ) -> Result<BackendTurn, BackendError>;

    /// `true` se il backend può eseguire davvero il tool server-side
    /// `web_search` nativo Anthropic. Default `true` (Claude reale). Un
    /// backend che lo scarta in traduzione (es. `OpenRouterBackend::to_or_tools`,
    /// che scarta OGNI `ToolSpec::Server`) DEVE dichiarare `false` qui: il
    /// chiamante (`LlmAdapter::respond`) lo usa per spegnere anche l'addendum
    /// del system prompt che promette la capacità — altrimenti il modello si
    /// crede capace di cercare sul web, tenta comunque la chiamata (non è nel
    /// suo schema tool ma niente glielo impedisce), e riceve "tool sconosciuto:
    /// web_search" come risultato, su cui fabbrica una scusa plausibile.
    /// Osservato dal vivo con DeepSeek via OpenRouter (2026-07-20).
    fn supports_web_search(&self) -> bool {
        true
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Fake (cfg(test), crate-visibile): stesso pattern di `FakeMessagesClient`
// in `messages_client.rs` — sequenza o ripetizione; registra TUTTE le
// richieste (per asserire la forma dell'invio successivo nel loop).
// ─────────────────────────────────────────────────────────────────────────────

/// Richiesta registrata da `FakeChatBackend` (owned, ispezionabile dopo che
/// `send_turn` è tornato).
#[cfg(test)]
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RecordedTurn {
    pub system: String,
    pub tools: Vec<ToolSpec>,
    pub history: Vec<Message>,
}

#[cfg(test)]
pub(crate) struct FakeChatBackend {
    requests: Mutex<Vec<RecordedTurn>>,
    responses: Mutex<VecDeque<Result<BackendTurn, BackendError>>>,
    repeat: bool,
    /// Override testabile di `supports_web_search` (default `true`, come il
    /// backend reale più comune — `ClaudeBackend`). I test che vogliono
    /// simulare un backend tipo `OpenRouterBackend` chiamano
    /// `with_web_search_supported(false)`.
    web_search_supported: bool,
    /// Se `true`, `send_turn` chiama anche `on_heartbeat()` (una volta) prima
    /// di tornare — simula un `ping` SSE ricevuto durante il turno, per
    /// testare la propagazione fino a `ServerMsg::Heartbeat` senza un vero
    /// server SSE (già coperto separatamente in `messages_client.rs`).
    emit_heartbeat: bool,
}

#[cfg(test)]
impl FakeChatBackend {
    /// Risponde in sequenza (una per chiamata); esaurita → panic di cortesia
    /// (stesso comportamento di `FakeMessagesClient::sequence`).
    pub(crate) fn sequence(responses: Vec<Result<BackendTurn, BackendError>>) -> Self {
        Self {
            requests: Mutex::new(Vec::new()),
            responses: Mutex::new(responses.into_iter().collect()),
            repeat: false,
            web_search_supported: true,
            emit_heartbeat: false,
        }
    }
    /// Ripete sempre la stessa risposta (per il test del cap iterazioni).
    pub(crate) fn repeating(resp: Result<BackendTurn, BackendError>) -> Self {
        Self {
            requests: Mutex::new(Vec::new()),
            responses: Mutex::new(VecDeque::from([resp])),
            repeat: true,
            web_search_supported: true,
            emit_heartbeat: false,
        }
    }
    pub(crate) fn ok(turn: BackendTurn) -> Self {
        Self::sequence(vec![Ok(turn)])
    }
    pub(crate) fn err(e: BackendError) -> Self {
        Self::sequence(vec![Err(e)])
    }
    pub(crate) fn with_web_search_supported(mut self, supported: bool) -> Self {
        self.web_search_supported = supported;
        self
    }
    pub(crate) fn with_heartbeat(mut self) -> Self {
        self.emit_heartbeat = true;
        self
    }
    pub(crate) fn recorded(&self) -> RecordedTurn {
        self.requests
            .lock()
            .unwrap()
            .last()
            .cloned()
            .expect("nessuna richiesta registrata")
    }
    pub(crate) fn nth_request(&self, n: usize) -> RecordedTurn {
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
impl ChatBackend for FakeChatBackend {
    async fn send_turn(
        &self,
        system: &str,
        tools: &[ToolSpec],
        history: &[Message],
        on_text: &mut (dyn for<'a> FnMut(&'a str) + Send),
        on_heartbeat: &mut (dyn FnMut() + Send),
    ) -> Result<BackendTurn, BackendError> {
        self.requests.lock().unwrap().push(RecordedTurn {
            system: system.to_string(),
            tools: tools.to_vec(),
            history: history.to_vec(),
        });
        if self.emit_heartbeat {
            on_heartbeat();
        }
        let result = {
            let mut resp = self.responses.lock().unwrap();
            if self.repeat {
                resp.front().cloned().expect("nessuna risposta configurata")
            } else {
                resp.pop_front().expect("risposte esaurite")
            }
        };
        if let Ok(turn) = &result {
            let text = turn.text();
            if !text.is_empty() {
                on_text(&text);
            }
        }
        result
    }

    fn supports_web_search(&self) -> bool {
        self.web_search_supported
    }
}

#[cfg(test)]
mod tests {
    use crate::chat_backend::{BackendTurn, ChatBackend, FakeChatBackend, TurnStop};
    use crate::messages_client::{Block, Message};

    #[tokio::test]
    async fn fake_chat_backend_records_request_and_replays_responses() {
        let fake = FakeChatBackend::sequence(vec![
            Ok(BackendTurn {
                blocks: vec![Block::Text { text: "prima".to_string() }],
                stop: TurnStop::Normal,
            }),
            Ok(BackendTurn {
                blocks: vec![Block::Text { text: "seconda".to_string() }],
                stop: TurnStop::Normal,
            }),
        ]);
        let history = vec![Message::user_text("ciao")];
        let mut seen = String::new();

        {
            let mut on_text = |delta: &str| seen.push_str(delta);
            let mut on_heartbeat = || {};
            let first = fake
                .send_turn("sistema", &[], &history, &mut on_text, &mut on_heartbeat)
                .await
                .unwrap();
            assert_eq!(first.text(), "prima");
        }
        assert_eq!(seen, "prima");

        {
            let mut on_text = |delta: &str| seen.push_str(delta);
            let mut on_heartbeat = || {};
            let second = fake
                .send_turn("sistema", &[], &history, &mut on_text, &mut on_heartbeat)
                .await
                .unwrap();
            assert_eq!(second.text(), "seconda");
        }

        assert_eq!(fake.request_count(), 2);
        assert_eq!(fake.recorded().system, "sistema");
        assert_eq!(fake.nth_request(0).history, history);
    }
}
