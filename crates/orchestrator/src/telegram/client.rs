//! # telegram::client
//!
//! Trait `TelegramClient` + tipi Bot API + `HttpTelegramClient` (reqwest) +
//! `FakeTelegramClient` (cfg(test)).
//!
//! Segue il pattern di `messages_client.rs`:
//! - Tipi serde minimi (solo i campi usati).
//! - `HttpTelegramClient` non implementa `Debug` (il campo `base` contiene il
//!   token; un Debug accidentale lo stamperebbe).
//! - Gli errori prodotti da `HttpTelegramClient` non contengono mai l'URL di
//!   base (che include il token). In particolare `reqwest::Error::without_url()`
//!   è chiamato prima di `.to_string()`.

use async_trait::async_trait;

#[cfg(test)]
use std::{collections::VecDeque, sync::Mutex};

// ─────────────────────────────────────────────────────────────────────────────
// Tipi Bot API (wire, solo i campi usati)
// ─────────────────────────────────────────────────────────────────────────────

/// Struttura intermedia per deserializzare `message.chat.id` da Bot API.
///
/// L'API Telegram nidifica il chat_id: `message.chat.id`. Usiamo una struct
/// intermedia wire + conversione `From` per estrarlo in modo tipizzato.
#[derive(serde::Deserialize)]
struct ChatWire {
    id: i64,
}

/// Messaggio Telegram (wire shape per deserializzazione interna).
#[derive(serde::Deserialize)]
struct MessageWire {
    chat: ChatWire,
    #[serde(default)]
    text: Option<String>,
}

/// Un messaggio Telegram con `chat_id` già estratto.
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub chat_id: i64,
    pub text: Option<String>,
}

impl From<MessageWire> for Message {
    fn from(w: MessageWire) -> Self {
        Self {
            chat_id: w.chat.id,
            text: w.text,
        }
    }
}

// Deserializziamo `Message` tramite la struttura wire intermedia.
impl<'de> serde::Deserialize<'de> for Message {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let wire = MessageWire::deserialize(d)?;
        Ok(wire.into())
    }
}

/// Struttura wire per `callback_query` con `message.chat.id` nidificato.
#[derive(serde::Deserialize)]
struct CallbackQueryWire {
    id: String,
    message: MessageWire,
    #[serde(default)]
    data: Option<String>,
}

/// Una callback da un bottone inline, con `chat_id` già estratto.
#[derive(Debug, Clone, PartialEq)]
pub struct CallbackQuery {
    pub id: String,
    pub chat_id: i64,
    pub data: Option<String>,
}

impl From<CallbackQueryWire> for CallbackQuery {
    fn from(w: CallbackQueryWire) -> Self {
        Self {
            id: w.id,
            chat_id: w.message.chat.id,
            data: w.data,
        }
    }
}

impl<'de> serde::Deserialize<'de> for CallbackQuery {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let wire = CallbackQueryWire::deserialize(d)?;
        Ok(wire.into())
    }
}

/// Un aggiornamento ricevuto da `getUpdates`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct Update {
    pub update_id: i64,
    #[serde(default)]
    pub message: Option<Message>,
    #[serde(default)]
    pub callback_query: Option<CallbackQuery>,
}

/// Bottone inline per la tastiera inline di Telegram.
///
/// Solo `Serialize` (li mandiamo noi, non li riceviamo).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct InlineButton {
    pub text: String,
    pub callback_data: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// Envelope Bot API per getUpdates
// ─────────────────────────────────────────────────────────────────────────────

/// Risposta generica dell'API Telegram: `{"ok":true,"result":[...]}`.
#[derive(serde::Deserialize)]
struct ApiResult<T> {
    result: Vec<T>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Trait
// ─────────────────────────────────────────────────────────────────────────────

/// Seam per il canale Telegram (DIP per testabilità).
///
/// Ogni errore è una `String` leggibile. Gli errori di `HttpTelegramClient`
/// non contengono mai il token.
#[async_trait]
pub trait TelegramClient: Send + Sync {
    /// Long-poll: ritorna gli update con `update_id >= offset`.
    /// `timeout_s` è il timeout di long-polling; il timeout HTTP è leggermente
    /// superiore per non tagliare la risposta.
    async fn get_updates(&self, offset: i64, timeout_s: u32) -> Result<Vec<Update>, String>;

    /// Invia un messaggio di testo a una chat.
    async fn send_message(&self, chat_id: i64, text: &str) -> Result<(), String>;

    /// Invia un messaggio con una riga di bottoni inline.
    async fn send_buttons(
        &self,
        chat_id: i64,
        text: &str,
        buttons: &[InlineButton],
    ) -> Result<(), String>;

    /// Risponde a una callback query (evita lo spinner sul bottone).
    async fn answer_callback(&self, callback_id: &str) -> Result<(), String>;
}

// ─────────────────────────────────────────────────────────────────────────────
// Funzioni pure di costruzione body (puri e testabili)
// ─────────────────────────────────────────────────────────────────────────────

/// Costruisce il body JSON per `sendMessage` con bottoni inline.
///
/// Estratto come funzione pura per permettere ai test di asserire sulla forma
/// esatta del wire senza richiedere una connessione HTTP (DIP / testabilità).
///
/// Forma attesa: `{ chat_id, text, reply_markup: { inline_keyboard: [[btn1, bn2, ...]] } }`
fn build_send_buttons_body(
    chat_id: i64,
    text: &str,
    buttons: &[InlineButton],
) -> serde_json::Value {
    serde_json::json!({
        "chat_id": chat_id,
        "text": text,
        "reply_markup": {
            "inline_keyboard": [buttons]
        }
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Implementazione HTTP reale
// ─────────────────────────────────────────────────────────────────────────────

/// Client HTTP reale verso l'API Telegram (long-polling).
///
/// Non implementa `Debug`: `base` contiene il token.
pub struct HttpTelegramClient {
    client: reqwest::Client,
    /// `https://api.telegram.org/bot{token}` — MAI incluso negli errori.
    base: String,
}

impl HttpTelegramClient {
    /// Costruisce un client con il token fornito.
    ///
    /// Il client `reqwest` non ha timeout fisso di default: `get_updates` imposta
    /// un timeout per-request superiore a `timeout_s`.
    pub fn new(token: &str) -> Self {
        let client = reqwest::Client::builder()
            .build()
            .expect("failed to build reqwest client for Telegram");
        Self {
            client,
            base: format!("https://api.telegram.org/bot{token}"),
        }
    }
}

#[async_trait]
impl TelegramClient for HttpTelegramClient {
    async fn get_updates(&self, offset: i64, timeout_s: u32) -> Result<Vec<Update>, String> {
        let url = format!("{}/getUpdates", self.base);
        // Il timeout reqwest deve essere superiore al long-poll timeout di Telegram
        // per non tagliare la risposta prima che Telegram risponda.
        let http_timeout =
            std::time::Duration::from_secs(u64::from(timeout_s) + 10);

        let resp = self
            .client
            .get(&url)
            .query(&[
                ("offset", offset.to_string()),
                ("timeout", timeout_s.to_string()),
            ])
            .timeout(http_timeout)
            .send()
            .await
            // `without_url()` rimuove l'URL dall'errore reqwest (che contiene il token).
            .map_err(|e| format!("getUpdates: {}", e.without_url()))?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            // Non includiamo il body intero (potrebbe contenere l'URL); usiamo solo lo status.
            return Err(format!("getUpdates: HTTP {status}"));
        }

        resp.json::<ApiResult<Update>>()
            .await
            .map(|r| r.result)
            .map_err(|e| format!("getUpdates: parse error — {}", e.without_url()))
    }

    async fn send_message(&self, chat_id: i64, text: &str) -> Result<(), String> {
        let url = format!("{}/sendMessage", self.base);
        let body = serde_json::json!({
            "chat_id": chat_id,
            "text": text,
        });

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("sendMessage: {}", e.without_url()))?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            return Err(format!("sendMessage: HTTP {status}"));
        }
        Ok(())
    }

    async fn send_buttons(
        &self,
        chat_id: i64,
        text: &str,
        buttons: &[InlineButton],
    ) -> Result<(), String> {
        let url = format!("{}/sendMessage", self.base);
        let body = build_send_buttons_body(chat_id, text, buttons);

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("sendButtons: {}", e.without_url()))?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            return Err(format!("sendButtons: HTTP {status}"));
        }
        Ok(())
    }

    async fn answer_callback(&self, callback_id: &str) -> Result<(), String> {
        let url = format!("{}/answerCallbackQuery", self.base);
        let body = serde_json::json!({
            "callback_query_id": callback_id,
        });

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("answerCallback: {}", e.without_url()))?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            return Err(format!("answerCallback: HTTP {status}"));
        }
        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Fake (cfg(test)) — coda di risposte per get_updates; registra le chiamate
// ─────────────────────────────────────────────────────────────────────────────

/// Client Telegram finto per i test (senza rete).
///
/// - `get_updates` ritorna le risposte scriptate dalla coda; esaurita → `Ok(vec![])`.
/// - `send_message`/`send_buttons`/`answer_callback` registrano le chiamate.
#[cfg(test)]
pub(crate) struct FakeTelegramClient {
    /// Coda di risposte per `get_updates`.
    update_queue: Mutex<VecDeque<Vec<Update>>>,
    /// Messaggi registrati: (chat_id, text).
    messages: Mutex<Vec<(i64, String)>>,
    /// Bottoni registrati: (chat_id, text, buttons).
    buttons: Mutex<Vec<(i64, String, Vec<InlineButton>)>>,
    /// Callback risposte registrate: callback_id.
    callbacks: Mutex<Vec<String>>,
    /// Auto-conferma per i test del confirmer: `None` = off (default);
    /// `Some(true)` = a ogni `send_buttons` accoda un tap sul 1° bottone
    /// (`ok:{id}`); `Some(false)` = sul 2° bottone (`no:{id}`). Estrae l'id
    /// (random) dal `callback_data` ricevuto, così i test possono "rispondere"
    /// senza conoscerlo a priori.
    auto_confirm: Mutex<Option<bool>>,
    /// Generatore di `update_id` per i tap auto-generati (monotòno).
    auto_update_id: Mutex<i64>,
}

#[cfg(test)]
impl FakeTelegramClient {
    pub(crate) fn new() -> Self {
        Self {
            update_queue: Mutex::new(VecDeque::new()),
            messages: Mutex::new(Vec::new()),
            buttons: Mutex::new(Vec::new()),
            callbacks: Mutex::new(Vec::new()),
            auto_confirm: Mutex::new(None),
            auto_update_id: Mutex::new(1),
        }
    }

    /// Abilita l'auto-conferma: ogni `send_buttons` accoderà automaticamente un
    /// tap `ok:{id}` (`decision=true`) o `no:{id}` (`decision=false`).
    pub(crate) fn set_auto_confirm(&self, decision: bool) {
        *self.auto_confirm.lock().unwrap() = Some(decision);
    }

    /// Accoda una batch di update da restituire alla prossima chiamata
    /// a `get_updates`.
    pub(crate) fn push_updates(&self, updates: Vec<Update>) {
        self.update_queue.lock().unwrap().push_back(updates);
    }

    /// Ritorna tutti i messaggi registrati (chat_id, text).
    pub(crate) fn recorded_messages(&self) -> Vec<(i64, String)> {
        self.messages.lock().unwrap().clone()
    }

    /// Ritorna tutti i bottoni registrati (chat_id, text, buttons).
    pub(crate) fn recorded_buttons(&self) -> Vec<(i64, String, Vec<InlineButton>)> {
        self.buttons.lock().unwrap().clone()
    }

    /// Ritorna tutti i callback_id registrati.
    pub(crate) fn recorded_callbacks(&self) -> Vec<String> {
        self.callbacks.lock().unwrap().clone()
    }
}

#[cfg(test)]
#[async_trait]
impl TelegramClient for FakeTelegramClient {
    async fn get_updates(&self, _offset: i64, _timeout_s: u32) -> Result<Vec<Update>, String> {
        let mut q = self.update_queue.lock().unwrap();
        Ok(q.pop_front().unwrap_or_default())
    }

    async fn send_message(&self, chat_id: i64, text: &str) -> Result<(), String> {
        self.messages
            .lock()
            .unwrap()
            .push((chat_id, text.to_string()));
        Ok(())
    }

    async fn send_buttons(
        &self,
        chat_id: i64,
        text: &str,
        buttons: &[InlineButton],
    ) -> Result<(), String> {
        self.buttons
            .lock()
            .unwrap()
            .push((chat_id, text.to_string(), buttons.to_vec()));

        // Auto-conferma (test del confirmer): estrae l'id random dal bottone e
        // accoda un tap corrispondente, così `confirm_via_poll` lo riceverà al
        // prossimo `get_updates`. Indice 0 = "Esegui" (ok), 1 = "Annulla" (no).
        if let Some(decision) = *self.auto_confirm.lock().unwrap() {
            let idx = if decision { 0 } else { 1 };
            if let Some(btn) = buttons.get(idx) {
                let mut uid = self.auto_update_id.lock().unwrap();
                let update = Update {
                    update_id: *uid,
                    message: None,
                    callback_query: Some(CallbackQuery {
                        id: format!("auto_cq_{}", *uid),
                        chat_id,
                        data: Some(btn.callback_data.clone()),
                    }),
                };
                *uid += 1;
                self.update_queue.lock().unwrap().push_back(vec![update]);
            }
        }
        Ok(())
    }

    async fn answer_callback(&self, callback_id: &str) -> Result<(), String> {
        self.callbacks
            .lock()
            .unwrap()
            .push(callback_id.to_string());
        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests (TDD — RED scritto prima dell'implementazione)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Deserializzazione tipi Bot API ────────────────────────────────────────

    /// `getUpdates` con message: deserializza correttamente `chat_id` da
    /// `message.chat.id`.
    #[test]
    fn get_updates_message_deserializes_chat_id_from_nested_chat() {
        let json = r#"{
            "ok": true,
            "result": [
                {
                    "update_id": 100,
                    "message": {
                        "chat": { "id": 42 },
                        "text": "ciao"
                    }
                }
            ]
        }"#;
        let envelope: ApiResult<Update> = serde_json::from_str(json).unwrap();
        assert_eq!(envelope.result.len(), 1);
        let u = &envelope.result[0];
        assert_eq!(u.update_id, 100);
        let msg = u.message.as_ref().expect("atteso message");
        assert_eq!(msg.chat_id, 42);
        assert_eq!(msg.text.as_deref(), Some("ciao"));
        assert!(u.callback_query.is_none());
    }

    /// `getUpdates` con callback_query: deserializza correttamente `chat_id` da
    /// `callback_query.message.chat.id`.
    #[test]
    fn get_updates_callback_query_deserializes_chat_id_from_nested_message_chat() {
        let json = r#"{
            "ok": true,
            "result": [
                {
                    "update_id": 200,
                    "callback_query": {
                        "id": "cq_1",
                        "message": {
                            "chat": { "id": 99 }
                        },
                        "data": "gate_abc"
                    }
                }
            ]
        }"#;
        let envelope: ApiResult<Update> = serde_json::from_str(json).unwrap();
        assert_eq!(envelope.result.len(), 1);
        let u = &envelope.result[0];
        assert_eq!(u.update_id, 200);
        assert!(u.message.is_none());
        let cq = u.callback_query.as_ref().expect("atteso callback_query");
        assert_eq!(cq.id, "cq_1");
        assert_eq!(cq.chat_id, 99);
        assert_eq!(cq.data.as_deref(), Some("gate_abc"));
    }

    /// Update senza né message né callback_query: campi opzionali corretti.
    #[test]
    fn update_with_neither_message_nor_callback_deserializes_ok() {
        let json = r#"{"update_id": 1}"#;
        let u: Update = serde_json::from_str(json).unwrap();
        assert_eq!(u.update_id, 1);
        assert!(u.message.is_none());
        assert!(u.callback_query.is_none());
    }

    // ── Serializzazione body sendButtons ─────────────────────────────────────

    /// `build_send_buttons_body` produce un JSON con `reply_markup.inline_keyboard`
    /// come array di array (una riga di bottoni). Questo test chiama direttamente
    /// la funzione pura usata da `HttpTelegramClient::send_buttons`, garantendo
    /// che il wire shape sia corretto nel codice di produzione e non solo nel test.
    #[test]
    fn send_buttons_body_has_correct_reply_markup_shape() {
        let buttons = vec![
            InlineButton {
                text: "Esegui".to_string(),
                callback_data: "exec_id_1".to_string(),
            },
            InlineButton {
                text: "Annulla".to_string(),
                callback_data: "cancel_id_1".to_string(),
            },
        ];
        // Chiama la funzione pura di produzione — non un json! inline separato.
        let body = build_send_buttons_body(42, "Eseguo: ls ?", &buttons);
        let obj = body.as_object().unwrap();
        // Struttura attesa: reply_markup.inline_keyboard[0][0].callback_data
        assert_eq!(obj["chat_id"], 42);
        assert_eq!(obj["text"], "Eseguo: ls ?");
        let keyboard = &obj["reply_markup"]["inline_keyboard"];
        assert!(keyboard.is_array(), "inline_keyboard deve essere array");
        let rows = keyboard.as_array().unwrap();
        assert_eq!(rows.len(), 1, "deve esserci una sola riga");
        let row = rows[0].as_array().unwrap();
        assert_eq!(row.len(), 2, "due bottoni nella riga");
        assert_eq!(row[0]["text"], "Esegui");
        assert_eq!(row[0]["callback_data"], "exec_id_1");
        assert_eq!(row[1]["text"], "Annulla");
        assert_eq!(row[1]["callback_data"], "cancel_id_1");
    }

    // ── FakeTelegramClient ────────────────────────────────────────────────────

    /// `push_updates` + `get_updates` ritorna la batch; la seconda chiamata
    /// ritorna vec![].
    #[tokio::test]
    async fn fake_get_updates_returns_queued_batch_then_empty() {
        let fake = FakeTelegramClient::new();
        let updates = vec![Update {
            update_id: 10,
            message: None,
            callback_query: None,
        }];
        fake.push_updates(updates.clone());

        let first = fake.get_updates(0, 1).await.unwrap();
        assert_eq!(first, updates);

        let second = fake.get_updates(11, 1).await.unwrap();
        assert!(second.is_empty(), "seconda chiamata deve restituire vec![]");
    }

    /// `send_message` registra (chat_id, text).
    #[tokio::test]
    async fn fake_send_message_records_call() {
        let fake = FakeTelegramClient::new();
        fake.send_message(42, "ciao").await.unwrap();
        fake.send_message(99, "mondo").await.unwrap();
        let msgs = fake.recorded_messages();
        assert_eq!(msgs, vec![(42, "ciao".to_string()), (99, "mondo".to_string())]);
    }

    /// `send_buttons` registra (chat_id, text, buttons).
    #[tokio::test]
    async fn fake_send_buttons_records_call() {
        let fake = FakeTelegramClient::new();
        let btns = vec![InlineButton {
            text: "Ok".to_string(),
            callback_data: "ok_id".to_string(),
        }];
        fake.send_buttons(5, "testo?", &btns).await.unwrap();
        let recorded = fake.recorded_buttons();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].0, 5);
        assert_eq!(recorded[0].1, "testo?");
        assert_eq!(recorded[0].2, btns);
    }

    /// `answer_callback` registra il callback_id.
    #[tokio::test]
    async fn fake_answer_callback_records_callback_id() {
        let fake = FakeTelegramClient::new();
        fake.answer_callback("cq_abc").await.unwrap();
        fake.answer_callback("cq_xyz").await.unwrap();
        assert_eq!(
            fake.recorded_callbacks(),
            vec!["cq_abc".to_string(), "cq_xyz".to_string()]
        );
    }
}
