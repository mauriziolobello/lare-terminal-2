//! # telegram::channel — polling loop + dispatch + mapping ServerMsg→testo
//!
//! Questo modulo è il punto di ingresso del canale Telegram:
//!
//! - **`TelegramChannel`** — stato del canale (client, auth, gates, history, ai, tools).
//! - **`dispatch`** — smista un `Update` in arrivo (callback o messaggio).
//! - **`run_command_buffered`** — chiama `handle_command`, drena il canale mpsc,
//!   mappa i `ServerMsg` in testo e li invia via Telegram.
//! - **`format_response`** — funzione pura: `Vec<ServerMsg>` → `String`.
//! - **`run`** — loop di polling long-poll con backoff esponenziale sugli errori.
//!
//! ## Sicurezza (ADR-007)
//! - Il token non compare mai in log o errori (`HttpTelegramClient` lo nasconde).
//! - Il TOTP secret non viene mai loggato.
//! - `chat_id` + sessione vengono verificati su OGNI update E callback.
//! - `/pair` e `/login` vengono gestiti PRIMA del gate `authorize`.
//! - `web_search = false` in v1 (nessun toggle Telegram).
//! - Il gate è single-use (ogni conferma o annullamento consuma il pending).
//! - I gate opachi non derivano dal contenuto del comando.

use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicI64, Ordering},
        Arc,
    },
};

use protocol::{CommandKind, ServerMsg};
use tokio::sync::mpsc::unbounded_channel;

use crate::{
    ai_adapter::AiAdapter,
    messages_client::ConversationHistory,
    telegram::{
        auth::{AuthResult, Authenticator},
        client::{InlineButton, TelegramClient, Update},
        confirm::{TelegramConfirmer, CONFIRM_MAX_POLLS},
        gate::{needs_confirmation, PendingGates},
    },
    tool_client::ToolClient,
};

// ─────────────────────────────────────────────────────────────────────────────
// TelegramChannel
// ─────────────────────────────────────────────────────────────────────────────

/// Stato del canale Telegram (polling loop + dispatch + exec).
///
/// Tutte le dipendenze asincrone usano `Arc<dyn Trait>` per compatibilità con
/// `tokio::spawn` (`'static` richiesto) nel Task 5.
pub struct TelegramChannel {
    /// Client Telegram (HTTP in produzione, Fake nei test).
    pub(crate) client: Arc<dyn TelegramClient>,
    /// Autenticatore (pairing, TOTP, sessioni).
    pub(crate) auth: Authenticator,
    /// Store dei comandi in attesa di conferma via inline keyboard.
    pub(crate) gates: PendingGates,
    /// Storia conversazionale (per il path NL).
    history: ConversationHistory,
    /// Adapter AI (Arc per 'static + Send).
    ai: Arc<dyn AiAdapter>,
    /// Tool client MCP (Arc per 'static + Send).
    tools: Arc<dyn ToolClient>,
    /// Offset corrente per `getUpdates` (update_id + 1 dell'ultimo elaborato).
    ///
    /// `AtomicI64` (non `i64`) perché è **condiviso** col [`TelegramConfirmer`]:
    /// durante l'attesa di una conferma in-loop, il confirmer fa lui stesso il
    /// polling sullo stesso offset (single-thread → `Ordering::Relaxed` basta;
    /// l'atomic serve anche a rendere la future di `respond` `Send`).
    offset: AtomicI64,
    /// Path del file di stato Telegram (per persistenza del pairing).
    state_path: PathBuf,
}

impl TelegramChannel {
    /// Costruisce un nuovo `TelegramChannel`.
    pub fn new(
        client: Arc<dyn TelegramClient>,
        auth: Authenticator,
        state_path: PathBuf,
        ai: Arc<dyn AiAdapter>,
        tools: Arc<dyn ToolClient>,
    ) -> Self {
        Self {
            client,
            auth,
            gates: PendingGates::new(),
            history: ConversationHistory::new(),
            ai,
            tools,
            offset: AtomicI64::new(0),
            state_path,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// format_response — funzione pura (testabile senza mock)
// ─────────────────────────────────────────────────────────────────────────────

/// Converte una sequenza di `ServerMsg` in una stringa di testo per Telegram.
///
/// Regole di mapping:
/// - `Chunk { content }` → accumula in un buffer.
/// - `OpenWindow { title, content, .. }` → `"📄 {title}\n\n{content}"` + newline.
/// - `Done { .. }` → emette il buffer accumulato (da Chunk precedenti).
/// - `Error { message, .. }` → `"[errore] {message}"`.
///
/// Il risultato è la stringa finale composta da tutti i segmenti separati da `\n`.
/// Se non c'è contenuto (nessun Chunk, nessuna finestra, nessun errore), ritorna `""`.
pub fn format_response(msgs: &[ServerMsg]) -> String {
    let mut segments: Vec<String> = Vec::new();
    let mut chunk_buf = String::new();

    for msg in msgs {
        match msg {
            ServerMsg::Chunk { content, .. } => {
                chunk_buf.push_str(content);
            }
            ServerMsg::OpenWindow { title, content, .. } => {
                // Emetti il buffer Chunk accumulato prima della finestra (se presente).
                if !chunk_buf.is_empty() {
                    segments.push(chunk_buf.clone());
                    chunk_buf.clear();
                }
                segments.push(format!("📄 {title}\n\n{content}"));
            }
            ServerMsg::Done { .. } => {
                // Fine sequenza: emetti il buffer Chunk accumulato.
                if !chunk_buf.is_empty() {
                    segments.push(chunk_buf.clone());
                    chunk_buf.clear();
                }
            }
            ServerMsg::Error { message, .. } => {
                // Emetti il buffer Chunk accumulato prima dell'errore (se presente).
                if !chunk_buf.is_empty() {
                    segments.push(chunk_buf.clone());
                    chunk_buf.clear();
                }
                segments.push(format!("[errore] {message}"));
            }
            // ServerInfo, Pong e Cwd sono messaggi di infrastruttura WS:
            // non hanno contenuto visibile per l'utente Telegram.
            ServerMsg::ServerInfo { .. } | ServerMsg::Pong { .. } | ServerMsg::Cwd { .. } => {}
            // TODO(Task 11): handle Search messages (SearchOpen, SearchHit, SearchDone)
            ServerMsg::SearchOpen { .. } | ServerMsg::SearchHit { .. } | ServerMsg::SearchDone { .. } => {}
            // Plugin window messages are UI-only (Task 5/6): Telegram channel has no
            // plugin-window surface, so these are intentional no-ops here.
            ServerMsg::OpenPluginWindow { .. }
            | ServerMsg::UpdatePluginWindow { .. }
            | ServerMsg::ClosePluginWindow { .. } => {}
            // I messaggi AI Chat (Slice 1a-ui) sono superficie locale (finestra-chat):
            // il canale Telegram non li rappresenta → no-op (come le finestre plugin).
            // Include le varianti di ammissione alla stanza (gate 1/2, pending,
            // admitted/rejected — Docs/superpowers/plans/2026-07-03-aichat-admission.md):
            // stesso trattamento permanente, non un bridge temporaneo.
            ServerMsg::AiChatMessage { .. }
            | ServerMsg::AiChatRoster { .. }
            | ServerMsg::AiChatJoinRequest { .. }
            | ServerMsg::AiChatHistory { .. }
            | ServerMsg::AiChatSelf { .. }
            | ServerMsg::AiChatPeerLost { .. }
            | ServerMsg::AiChatReachablePeers { .. }
            | ServerMsg::AiChatJoinPrompt { .. }
            | ServerMsg::AiChatAdmissionRequest { .. }
            | ServerMsg::AiChatPending { .. }
            | ServerMsg::AiChatAdmitted {}
            | ServerMsg::AiChatRejected { .. }
            | ServerMsg::AiChatAdmissionResolved { .. } => {}
            // Task 10: Library "Share with" — share request/result are UI-only (Slice 1a).
            // Telegram channel has no share consent surface, so these are no-ops.
            ServerMsg::ShareRequest { .. } | ServerMsg::ShareResult { .. } => {}
            // Slice 2a: content-transfer round-trip is UI-only, same reasoning as
            // ShareRequest/ShareResult above — Telegram has no Library surface to
            // fetch content from or write files to, so these are permanent no-ops.
            ServerMsg::ShareContentRequest { .. } | ServerMsg::ShareIncomingData { .. } => {}
            // Tool confirmation (Task 2-3): gate di conferma per-tool.
            // Telegram uses `TelegramConfirmer` (Task 3), not the local UI gate.
            // This message is not sent to Telegram (it's local UI only) — no-op.
            ServerMsg::ToolConfirmRequest { .. } => {}
            // Library "Blocco note" (Task 6 placeholder — Task 14/Task 16 replaces with
            // real display logic; kept here only so the workspace compiles between
            // Task 6 and Task 16).
            ServerMsg::NotesSnapshot { .. } | ServerMsg::NoteUpserted { .. } => {}
            // Task 10: Routine save preview (routine save dialog) is UI-only — Telegram
            // channel has no routine editor surface, so this is a permanent no-op.
            ServerMsg::RoutineSavePreview { .. } => {}
            // Task 2 (heartbeat watchdog, 2026-08-07): battito di vita per resettare il
            // timer di silenzio del watchdog frontend — Telegram non ha un tale timer/
            // superficie visiva, quindi è un no-op permanente come gli altri messaggi
            // di infrastruttura sopra.
            ServerMsg::Heartbeat { .. } => {}
            // Registry screener multipli (2026-08-11): il picker è una finestra
            // Tauri (screener-picker.html) — Telegram non ha una superficie di
            // selezione a lista, quindi no-op permanente come le altre finestre
            // UI-only sopra (OpenPluginWindow, RoutineSavePreview, ...).
            ServerMsg::OpenScreenerPicker { .. } => {}
            // Task 11 (IBKR data source): risposta al test manuale "fonte dati
            // mercato" innescato da `/config` — superficie UI-only (pulsante di
            // test nella finestra di configurazione), il canale Telegram non ha
            // un equivalente, quindi no-op permanente come ToolConfirmRequest sopra.
            ServerMsg::MarketDataSourceTestResult { .. } => {}
            // Piano 2a (canale shell, Task 1): superficie `lare-shell`/`ui.exe`.
            // `ExecInShell` torna alla connessione shell che ha emesso il comando
            // (esecuzione nel runspace dell'utente); le altre aprono/aggiornano
            // finestre o segnalano stato su `ui.exe`. Il canale Telegram non ha né
            // una shell propria né finestre: no-op permanente, come le altre
            // superfici UI-only sopra (OpenPluginWindow, RoutineSavePreview, ...).
            ServerMsg::ExecInShell { .. }
            | ServerMsg::OpenOutputWindow { .. }
            | ServerMsg::OutputWindowContent { .. }
            | ServerMsg::OpenUiLocal { .. }
            | ServerMsg::UiPing { .. }
            | ServerMsg::ActivityIndicator { .. } => {}
        }
    }

    // Tronca a 4096 caratteri (limite Telegram) su char-boundary, non byte.
    // Gestiamo la concatenazione per segmento; qui restituiamo tutto, il
    // chiamante (`run_command_buffered`) splitterà se necessario.
    segments.join("\n")
}

/// Splita una stringa in chunk di al massimo `max_chars` caratteri (char-safe).
///
/// Telegram ha un limite di 4096 unità UTF-16 per messaggio. Per v1 usiamo
/// char-count come approssimazione conservativa.
fn split_at_chars(text: &str, max_chars: usize) -> Vec<String> {
    let mut result = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut start = 0;
    while start < chars.len() {
        let end = (start + max_chars).min(chars.len());
        result.push(chars[start..end].iter().collect());
        start = end;
    }
    result
}

// ─────────────────────────────────────────────────────────────────────────────
// run_command_buffered — integrazione con handle_command
// ─────────────────────────────────────────────────────────────────────────────

impl TelegramChannel {
    /// Esegue un comando tramite `handle_command`, raccoglie i `ServerMsg` emessi,
    /// li formatta come testo e li invia via Telegram (splitting a 4096 char).
    ///
    /// Se il testo formattato è vuoto, invia "(nessun output)".
    async fn run_command_buffered(
        &mut self,
        chat_id: i64,
        input: &str,
        command_type: CommandKind,
    ) {
        // Crea il canale mpsc unbounded.
        let (tx, mut rx) = unbounded_channel::<ServerMsg>();

        // Gate in-loop dei tool dell'AI (Docs/15, Task 3): un `TelegramConfirmer`
        // che borrowa `client`/`offset`/`auth` — prestiti DISGIUNTI da
        // `&mut self.history` (campi diversi), quindi consentiti. Ogni tool che
        // l'AI propone (`run_in_session`/`open_target`) passa da una conferma su
        // Telegram; i comandi non-AI restano soggetti al gate a monte (PendingGates).
        let confirmer = TelegramConfirmer::new(
            self.client.as_ref(),
            chat_id,
            &self.offset,
            &self.auth,
            Box::new(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64
            }),
            CONFIRM_MAX_POLLS,
        );

        // Esegui il comando. `handle_command` droppa `tx` al termine → rx si chiude.
        // `cancel: None` — il canale Telegram non ha ancora un meccanismo di stop button
        // (la cancellazione è una feature futura per questo canale).
        crate::core::handle_command(
            "tg",
            input,
            command_type,
            None,
            &mut self.history,
            self.ai.as_ref(),
            self.tools.as_ref(),
            None, // format_invocation: Telegram non partecipa alla risoluzione canale — fallback su agent::display_invocation, invariato
            None, // system_prompt_override: idem — Telegram non ha un concetto di canale, resta sempre agent::SYSTEM_PROMPT
            false, // web_search=false in v1 (nessun toggle Telegram)
            None,  // lang: Telegram non passa lang, usa il default (italiano)
            Some(&confirmer),
            None,
            tx,
        )
        .await;

        // Drena i messaggi con try_recv (il tx è già droppato).
        let mut msgs: Vec<ServerMsg> = Vec::new();
        while let Ok(msg) = rx.try_recv() {
            msgs.push(msg);
        }

        // Formatta in testo.
        let text = format_response(&msgs);
        let text = if text.trim().is_empty() {
            "(nessun output)".to_string()
        } else {
            text
        };

        // Invia in chunk ≤ 4096 char.
        for chunk in split_at_chars(&text, 4096) {
            // Ignora gli errori di invio (Telegram temporaneamente irraggiungibile).
            let _ = self.client.send_message(chat_id, &chunk).await;
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// dispatch — smista un Update in arrivo
// ─────────────────────────────────────────────────────────────────────────────

impl TelegramChannel {
    /// Smista un `Update` in arrivo: callback o messaggio.
    ///
    /// Ordine dei controlli per i messaggi:
    /// 1. `/pair <code>` → tenta il pairing (gestito PRIMA di authorize).
    /// 2. `/login <totp>` → tenta il login (gestito PRIMA di authorize).
    /// 3. `authorize` → verifica chat_id + sessione.
    /// 4. `needs_confirmation` → se vero, inserisce nel gate e invia bottoni.
    /// 5. Altrimenti esegue il comando via `run_command_buffered`.
    ///
    /// `now_ms` è iniettato per i test deterministici (nessun clock reale).
    pub(crate) async fn dispatch(&mut self, update: Update, now_ms: u64) {
        if let Some(cq) = update.callback_query {
            self.dispatch_callback(cq, now_ms).await;
        } else if let Some(msg) = update.message {
            let chat_id = msg.chat_id;
            let text = match msg.text.as_deref() {
                Some(t) if !t.trim().is_empty() => t.to_string(),
                _ => return, // ignora messaggi senza testo
            };
            self.dispatch_message(chat_id, &text, now_ms).await;
        }
    }

    /// Gestisce una callback da un bottone inline.
    async fn dispatch_callback(
        &mut self,
        cq: crate::telegram::client::CallbackQuery,
        now_ms: u64,
    ) {
        // Risponde PRIMA alla callback per togliere lo spinner dal bottone.
        // Non registriamo l'id della callback nei log (contiene token dell'utente
        // in alcuni client — best practice: rispondere e ignorare).
        let _ = self.client.answer_callback(&cq.id).await;

        // Verifica autorizzazione per OGNI callback.
        match self.auth.authorize(cq.chat_id, now_ms) {
            AuthResult::Ok => {}
            AuthResult::NeedLogin => {
                // Chat appaiata ma sessione scaduta: dai feedback (un tap su un
                // bottone "vecchio" non deve restare senza risposta — UX e2e).
                // Coerente con `dispatch_message`. Le chat non autorizzate
                // (Denied/NeedPairing) restano in silenzio (anti-enumerazione).
                let _ = self
                    .client
                    .send_message(cq.chat_id, "Sessione scaduta. Usa /login <codice_totp>.")
                    .await;
                return;
            }
            _ => return, // NeedPairing/Denied → scarta silenziosamente
        }

        let data = match cq.data.as_deref() {
            Some(d) => d.to_string(),
            None => return,
        };

        if let Some(gate_id) = data.strip_prefix("ok:") {
            // Conferma: preleva il pending dal gate.
            match self.gates.take(gate_id, now_ms) {
                Some((input, kind)) => {
                    // Esegui il comando confermato.
                    self.run_command_buffered(cq.chat_id, &input, kind).await;
                }
                None => {
                    // Gate scaduto, non trovato o già usato.
                    let _ = self
                        .client
                        .send_message(cq.chat_id, "Richiesta scaduta o non valida.")
                        .await;
                }
            }
        } else if let Some(gate_id) = data.strip_prefix("no:") {
            // Annulla: consuma il pending (per pulizia) anche se scaduto.
            self.gates.take(gate_id, now_ms);
            let _ = self.client.send_message(cq.chat_id, "Annullato.").await;
        }
        // Dati non riconosciuti → ignora silenziosamente.
    }

    /// Gestisce un messaggio di testo.
    async fn dispatch_message(&mut self, chat_id: i64, text: &str, now_ms: u64) {
        let trimmed = text.trim();

        // ── 1. /pair <code> — gestito PRIMA di authorize ──────────────────────
        if let Some(rest) = trimmed.strip_prefix("/pair").map(str::trim) {
            if rest.is_empty() {
                // /pair senza codice — invia istruzioni solo se non appaiato
                if self.auth.paired_chat_id().is_none() {
                    let _ = self
                        .client
                        .send_message(
                            chat_id,
                            "Invia il codice di pairing generato all'avvio del daemon.\n\
                             Esempio: /pair 123456",
                        )
                        .await;
                }
                // Se già appaiato: silenzio (non rivela lo stato).
                return;
            }
            match self.auth.try_pair(chat_id, rest, now_ms, &self.state_path) {
                AuthResult::Ok => {
                    let _ = self
                        .client
                        .send_message(chat_id, "Pairing completato. Usa /login <codice_totp> per accedere.")
                        .await;
                }
                AuthResult::RateLimited => {
                    let _ = self
                        .client
                        .send_message(chat_id, "Troppi tentativi. Riprova tra 1 minuto.")
                        .await;
                }
                _ => {
                    // Denied / NeedPairing — silenzio: non rivela l'esistenza del bot.
                }
            }
            return;
        }

        // ── 2. /login <totp> — gestito PRIMA di authorize ─────────────────────
        if let Some(rest) = trimmed.strip_prefix("/login").map(str::trim) {
            if rest.is_empty() {
                // /login senza codice: istruzioni solo alla chat appaiata.
                if self.auth.authorize(chat_id, now_ms) == AuthResult::NeedLogin {
                    let _ = self
                        .client
                        .send_message(chat_id, "Usa /login <codice_totp> per aprire la sessione.")
                        .await;
                }
                // Altrimenti silenzio.
                return;
            }
            match self.auth.try_login(chat_id, rest, now_ms) {
                AuthResult::Ok => {
                    let _ = self
                        .client
                        .send_message(chat_id, "Sessione aperta. Buon lavoro.")
                        .await;
                }
                AuthResult::RateLimited => {
                    let _ = self
                        .client
                        .send_message(chat_id, "Troppi tentativi. Riprova tra 1 minuto.")
                        .await;
                }
                _ => {
                    // Denied — silenzio: non rivela se il chat_id è appaiato.
                }
            }
            return;
        }

        // ── 3. authorize — tutti gli altri comandi richiedono sessione ─────────
        match self.auth.authorize(chat_id, now_ms) {
            AuthResult::Ok => {}
            AuthResult::NeedPairing => {
                // Non appaiato: silenzio (non rivela l'esistenza del bot).
                return;
            }
            AuthResult::NeedLogin => {
                let _ = self
                    .client
                    .send_message(chat_id, "Sessione scaduta. Usa /login <codice_totp>.")
                    .await;
                return;
            }
            AuthResult::RateLimited => {
                let _ = self
                    .client
                    .send_message(chat_id, "Troppi tentativi. Riprova tra 1 minuto.")
                    .await;
                return;
            }
            AuthResult::Denied => {
                // Chat non autorizzata: silenzio.
                return;
            }
        }

        // ── 4. gate: conferma richiesta per comandi OS e /open /web ───────────
        let command_type = CommandKind::Auto;
        if needs_confirmation(trimmed, command_type.clone()) {
            // Genera un id opaco (random) per il gate.
            let gate_id = format!("{:032x}", rand::random::<u128>());
            self.gates.insert(&gate_id, trimmed, command_type.clone(), now_ms);

            let preview = if trimmed.len() > 100 {
                format!("{}...", &trimmed[..100])
            } else {
                trimmed.to_string()
            };

            let prompt = format!("Eseguire: {preview}");
            let buttons = vec![
                InlineButton {
                    text: "Esegui".to_string(),
                    callback_data: format!("ok:{gate_id}"),
                },
                InlineButton {
                    text: "Annulla".to_string(),
                    callback_data: format!("no:{gate_id}"),
                },
            ];
            let _ = self
                .client
                .send_buttons(chat_id, &prompt, &buttons)
                .await;
            return;
        }

        // ── 5. esegui direttamente ─────────────────────────────────────────────
        self.run_command_buffered(chat_id, trimmed, command_type).await;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// run — polling loop principale
// ─────────────────────────────────────────────────────────────────────────────

impl TelegramChannel {
    /// Avvia il loop di polling long-poll.
    ///
    /// Al primo avvio, salta il backlog esistente (skip degli update arretrati):
    /// imposta `offset` al massimo update_id presente + 1, senza dispatchare
    /// i vecchi update. Questo evita l'esecuzione accidentale di comandi in coda
    /// dall'ultimo avvio del daemon.
    ///
    /// Su errore di `getUpdates`: backoff esponenziale (1s → 2s → 4s … → max 32s).
    pub async fn run(&mut self) {
        // Skip backlog: leggi gli update correnti senza dispatchare.
        if let Ok(backlog) = self.client.get_updates(self.offset.load(Ordering::Relaxed), 0).await {
            if let Some(last) = backlog.last() {
                self.offset.store(last.update_id + 1, Ordering::Relaxed);
            }
        }

        let mut backoff_secs: u64 = 1;

        loop {
            match self.client.get_updates(self.offset.load(Ordering::Relaxed), 25).await {
                Ok(updates) => {
                    backoff_secs = 1; // reset backoff su successo
                    for update in updates {
                        // Se il confirmer (durante il dispatch di un update
                        // precedente di QUESTA batch) ha già consumato questo
                        // update avanzando l'offset, saltalo: altrimenti un
                        // messaggio intercorrente verrebbe gestito due volte
                        // (avviso "in sospeso" dal confirmer + ri-dispatch qui).
                        // Lettura FRESH dell'offset a ogni iterazione.
                        if update.update_id < self.offset.load(Ordering::Relaxed) {
                            continue;
                        }
                        self.offset.store(update.update_id + 1, Ordering::Relaxed);
                        let now_ms = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64;
                        // Garbage-collect le entry scadute del gate prima di ogni dispatch,
                        // così i pending non confermati non si accumulano indefinitamente.
                        self.gates.purge_expired(now_ms);
                        self.dispatch(update, now_ms).await;
                    }
                }
                Err(_e) => {
                    // Backoff esponenziale: non logghiamo l'errore per evitare
                    // di esporre informazioni sull'URL (che contiene il token).
                    tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
                    backoff_secs = (backoff_secs * 2).min(32);
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests (TDD: RED prima dell'implementazione)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ai_adapter::StubAdapter,
        telegram::{
            auth::{TelegramState, SESSION_TTL_MS},
            client::{FakeTelegramClient, Message, Update},
        },
        tool_client::FakeToolClient,
    };
    use std::sync::Arc;
    use tempfile::TempDir;
    use totp_rs::{Algorithm, Secret, TOTP};

    // ─── Costanti e helper ────────────────────────────────────────────────────

    /// Chat_id autorizzato nei test.
    const CHAT: i64 = 42;

    /// Secret TOTP fisso per i test (stesso di auth.rs per coerenza).
    const TEST_SECRET_B32: &str = "JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP";

    /// Timestamp fisso (ms) per test deterministici.
    const T0: u64 = 1_700_000_000_000;

    /// Genera il codice TOTP corretto per `TEST_SECRET_B32` a `T0`.
    fn totp_code_at(now_ms: u64) -> String {
        let secret_bytes = Secret::Encoded(TEST_SECRET_B32.to_string())
            .to_bytes()
            .unwrap();
        TOTP::new(
            Algorithm::SHA1,
            6,
            1,
            30,
            secret_bytes,
            Some("Lare Terminal".to_string()),
            "lare".to_string(),
        )
        .unwrap()
        .generate(now_ms / 1000)
    }

    /// Costruisce un `Authenticator` con secret fisso e chat_id opzionalmente appaiato.
    fn auth_with_secret(paired: Option<i64>) -> Authenticator {
        let state = TelegramState {
            totp_secret_base32: Some(TEST_SECRET_B32.to_string()),
            paired_chat_id: paired,
        };
        Authenticator::from_state(&state).expect("secret valido")
    }

    /// Costruisce un `Authenticator` con sessione attiva (paired + logged in).
    fn auth_logged_in(chat_id: i64, now_ms: u64) -> Authenticator {
        let mut auth = auth_with_secret(Some(chat_id));
        let code = totp_code_at(now_ms);
        let r = auth.try_login(chat_id, &code, now_ms);
        assert_eq!(r, AuthResult::Ok, "setup: try_login deve riuscire");
        auth
    }

    /// Costruisce un `TelegramChannel` di test completo.
    fn make_channel(
        fake: Arc<FakeTelegramClient>,
        auth: Authenticator,
        dir: &TempDir,
    ) -> TelegramChannel {
        let state_path = dir.path().join("state.json");
        TelegramChannel::new(
            fake,
            auth,
            state_path,
            Arc::new(StubAdapter),
            Arc::new(FakeToolClient::success("output di test")),
        )
    }

    /// Crea un `Update` con messaggio di testo.
    fn text_update(update_id: i64, chat_id: i64, text: &str) -> Update {
        Update {
            update_id,
            message: Some(Message {
                chat_id,
                text: Some(text.to_string()),
            }),
            callback_query: None,
        }
    }

    /// Crea un `Update` con callback_query.
    fn callback_update(
        update_id: i64,
        chat_id: i64,
        cq_id: &str,
        data: &str,
    ) -> Update {
        Update {
            update_id,
            message: None,
            callback_query: Some(crate::telegram::client::CallbackQuery {
                id: cq_id.to_string(),
                chat_id,
                data: Some(data.to_string()),
            }),
        }
    }

    // ─── format_response — funzione pura ─────────────────────────────────────

    /// Nessun messaggio → stringa vuota.
    #[test]
    fn format_response_empty_slice_returns_empty_string() {
        assert_eq!(format_response(&[]), "");
    }

    /// Un solo Chunk + Done → il contenuto del Chunk.
    #[test]
    fn format_response_single_chunk_and_done() {
        let msgs = vec![
            ServerMsg::Chunk {
                id: "1".to_string(),
                content: "ciao mondo".to_string(),
            },
            ServerMsg::Done {
                id: "1".to_string(),
                exit_code: Some(0),
            },
        ];
        assert_eq!(format_response(&msgs), "ciao mondo");
    }

    /// Due Chunk consecutivi vengono uniti (non separati).
    #[test]
    fn format_response_two_chunks_concatenated() {
        let msgs = vec![
            ServerMsg::Chunk {
                id: "1".to_string(),
                content: "riga uno\n".to_string(),
            },
            ServerMsg::Chunk {
                id: "1".to_string(),
                content: "riga due".to_string(),
            },
            ServerMsg::Done {
                id: "1".to_string(),
                exit_code: Some(0),
            },
        ];
        // I chunk vengono accumulati nello stesso buffer → un solo segmento.
        let result = format_response(&msgs);
        assert!(result.contains("riga uno\n"), "deve contenere riga uno: {result:?}");
        assert!(result.contains("riga due"), "deve contenere riga due: {result:?}");
    }

    /// OpenWindow → "📄 {title}\n\n{content}".
    #[test]
    fn format_response_open_window_uses_emoji_prefix() {
        use protocol::WindowKind;
        let msgs = vec![
            ServerMsg::OpenWindow {
                title: "Il Titolo".to_string(),
                kind: WindowKind::Markdown,
                content: "corpo del documento".to_string(),
            },
            ServerMsg::Done {
                id: "1".to_string(),
                exit_code: Some(0),
            },
        ];
        let result = format_response(&msgs);
        assert!(
            result.contains("📄 Il Titolo"),
            "deve contenere '📄 Il Titolo': {result:?}"
        );
        assert!(
            result.contains("corpo del documento"),
            "deve contenere il contenuto: {result:?}"
        );
    }

    /// Error → "[errore] {message}".
    #[test]
    fn format_response_error_has_prefix() {
        use protocol::ErrCode;
        let msgs = vec![ServerMsg::Error {
            id: "1".to_string(),
            code: ErrCode::RoutingError,
            message: "qualcosa è andato storto".to_string(),
        }];
        let result = format_response(&msgs);
        assert_eq!(result, "[errore] qualcosa è andato storto");
    }

    /// Done senza Chunk precedenti → stringa vuota (nessun segmento).
    #[test]
    fn format_response_done_only_returns_empty() {
        let msgs = vec![ServerMsg::Done {
            id: "1".to_string(),
            exit_code: Some(0),
        }];
        assert_eq!(format_response(&msgs), "");
    }

    // ─── split_at_chars ───────────────────────────────────────────────────────

    /// String corta (< 4096 char) → un solo chunk.
    #[test]
    fn split_at_chars_short_string_is_single_chunk() {
        let text = "ciao";
        let chunks = split_at_chars(text, 4096);
        assert_eq!(chunks, vec!["ciao".to_string()]);
    }

    /// Stringa esattamente di 4 char, limite 2 → due chunk.
    #[test]
    fn split_at_chars_splits_correctly() {
        let chunks = split_at_chars("abcd", 2);
        assert_eq!(chunks, vec!["ab".to_string(), "cd".to_string()]);
    }

    /// Split su confini char, non byte (emoji = 1 char).
    #[test]
    fn split_at_chars_respects_char_boundaries() {
        // "📄" è 1 char, 4 byte. Con limite=2 e "📄x": 2 char → [0..2] = "📄x"
        let chunks = split_at_chars("📄x", 2);
        assert_eq!(chunks, vec!["📄x".to_string()]);
    }

    /// Stringa vuota → vec vuoto.
    #[test]
    fn split_at_chars_empty_string_returns_empty_vec() {
        let chunks = split_at_chars("", 4096);
        assert!(chunks.is_empty());
    }

    // ─── dispatch: chat non appaiata ─────────────────────────────────────────

    /// Un comando da chat non appaiata non viene eseguito (silenzio totale).
    #[tokio::test]
    async fn dispatch_unpaired_chat_sends_nothing_and_does_not_exec() {
        let dir = TempDir::new().unwrap();
        let fake = Arc::new(FakeTelegramClient::new());
        // auth senza chat appaiato
        let mut ch = make_channel(fake.clone(), auth_with_secret(None), &dir);

        ch.dispatch(text_update(1, CHAT, "dir"), T0).await;

        // Nessun messaggio inviato (silenzio: non rivela l'esistenza del bot).
        let msgs = fake.recorded_messages();
        assert!(msgs.is_empty(), "atteso silenzio per chat non appaiata, got: {msgs:?}");
    }

    // ─── dispatch: /pair ─────────────────────────────────────────────────────

    /// /pair con codice corretto → pairing completato + messaggio di conferma.
    #[tokio::test]
    async fn dispatch_pair_correct_code_confirms() {
        let dir = TempDir::new().unwrap();
        let fake = Arc::new(FakeTelegramClient::new());
        let mut auth = auth_with_secret(None);
        // Genera il codice di pairing nel auth
        let code = auth.new_pairing_code(T0);
        let mut ch = make_channel(fake.clone(), auth, &dir);

        ch.dispatch(text_update(1, CHAT, &format!("/pair {code}")), T0).await;

        let msgs = fake.recorded_messages();
        assert!(!msgs.is_empty(), "atteso messaggio di conferma pairing");
        let texts: Vec<&str> = msgs.iter().map(|(_, t)| t.as_str()).collect();
        assert!(
            texts.iter().any(|t| t.contains("Pairing completato")),
            "atteso 'Pairing completato' nei messaggi, got: {texts:?}"
        );
    }

    // ─── dispatch: /login ─────────────────────────────────────────────────────

    /// /login con TOTP corretto → sessione aperta + messaggio di conferma.
    #[tokio::test]
    async fn dispatch_login_correct_totp_opens_session() {
        let dir = TempDir::new().unwrap();
        let fake = Arc::new(FakeTelegramClient::new());
        let auth = auth_with_secret(Some(CHAT)); // già appaiato, nessuna sessione
        let mut ch = make_channel(fake.clone(), auth, &dir);

        let code = totp_code_at(T0);
        ch.dispatch(text_update(1, CHAT, &format!("/login {code}")), T0).await;

        let msgs = fake.recorded_messages();
        assert!(!msgs.is_empty(), "atteso messaggio di conferma login");
        let texts: Vec<&str> = msgs.iter().map(|(_, t)| t.as_str()).collect();
        assert!(
            texts.iter().any(|t| t.contains("Sessione aperta")),
            "atteso 'Sessione aperta', got: {texts:?}"
        );
    }

    // ─── dispatch: comando NL autorizzato ────────────────────────────────────

    /// Comando NL da chat autorizzata → StubAdapter risponde (Chunk con "[stub AI]").
    #[tokio::test]
    async fn dispatch_nl_command_from_authorized_chat_executes() {
        let dir = TempDir::new().unwrap();
        let fake = Arc::new(FakeTelegramClient::new());
        let auth = auth_logged_in(CHAT, T0);
        let mut ch = make_channel(fake.clone(), auth, &dir);

        ch.dispatch(text_update(1, CHAT, "spiega async await"), T0).await;

        let msgs = fake.recorded_messages();
        assert!(!msgs.is_empty(), "atteso almeno un messaggio inviato");
        // StubAdapter emette "[stub AI] ricevuto: ..."
        let found = msgs.iter().any(|(_, t)| t.contains("[stub AI] ricevuto:"));
        assert!(found, "atteso risposta stub AI, got: {msgs:?}");
    }

    // ─── dispatch: comando OS → bottoni di conferma ───────────────────────────

    /// Comando OS da chat autorizzata → invio bottoni di conferma (gate).
    #[tokio::test]
    async fn dispatch_os_command_from_authorized_sends_confirmation_buttons() {
        let dir = TempDir::new().unwrap();
        let fake = Arc::new(FakeTelegramClient::new());
        let auth = auth_logged_in(CHAT, T0);
        let mut ch = make_channel(fake.clone(), auth, &dir);

        // "dir" è un token OS riconosciuto dal router.
        ch.dispatch(text_update(1, CHAT, "dir"), T0).await;

        let buttons = fake.recorded_buttons();
        assert!(!buttons.is_empty(), "atteso send_buttons per comando OS");
        let (chat, _text, btns) = &buttons[0];
        assert_eq!(*chat, CHAT);
        assert_eq!(btns.len(), 2, "atteso 2 bottoni (Esegui + Annulla)");
        assert_eq!(btns[0].text, "Esegui");
        assert_eq!(btns[1].text, "Annulla");
        // I callback_data devono iniziare con "ok:" e "no:".
        assert!(btns[0].callback_data.starts_with("ok:"));
        assert!(btns[1].callback_data.starts_with("no:"));
        // Il comando NON deve essere eseguito prima della conferma: nessun
        // messaggio testuale deve essere inviato in questa fase.
        assert!(
            fake.recorded_messages().is_empty(),
            "il comando OS NON deve essere eseguito prima della conferma"
        );
    }

    // ─── dispatch: callback "ok:" → esegue il comando ────────────────────────

    /// Callback "ok:<id>" con gate valido → esegue il comando.
    #[tokio::test]
    async fn dispatch_callback_ok_executes_command() {
        let dir = TempDir::new().unwrap();
        let fake = Arc::new(FakeTelegramClient::new());
        let auth = auth_logged_in(CHAT, T0);
        let mut ch = make_channel(fake.clone(), auth, &dir);

        // Prima invia il comando OS per creare il gate.
        ch.dispatch(text_update(1, CHAT, "dir"), T0).await;

        // Recupera il callback_data del bottone "ok:".
        let buttons = fake.recorded_buttons();
        assert!(!buttons.is_empty());
        let gate_cb_data = buttons[0].2[0].callback_data.clone(); // "ok:<id>"

        // Ora simula il click "Esegui".
        ch.dispatch(
            callback_update(2, CHAT, "cq_ok_1", &gate_cb_data),
            T0 + 1,
        )
        .await;

        // Il comando deve essere stato eseguito: devono esserci messaggi DOPO i bottoni.
        let msgs = fake.recorded_messages();
        // I messaggi dopo il gate devono contenere output (FakeToolClient ritorna "output di test"
        // ma StubAdapter per NL ritorna stub — "dir" è OS → FakeToolClient).
        // Con FakeToolClient.success("output di test"): stdout="output di test", exit=0 → Chunk.
        assert!(!msgs.is_empty(), "atteso output del comando dopo conferma, got: {msgs:?}");
    }

    /// Callback "ok:<id>" con lo stesso gate_id una seconda volta → "Richiesta scaduta".
    #[tokio::test]
    async fn dispatch_callback_ok_repeated_returns_expired() {
        let dir = TempDir::new().unwrap();
        let fake = Arc::new(FakeTelegramClient::new());
        let auth = auth_logged_in(CHAT, T0);
        let mut ch = make_channel(fake.clone(), auth, &dir);

        // Crea il gate.
        ch.dispatch(text_update(1, CHAT, "dir"), T0).await;
        let buttons = fake.recorded_buttons();
        let gate_cb_data = buttons[0].2[0].callback_data.clone();

        // Prima conferma: deve funzionare.
        ch.dispatch(callback_update(2, CHAT, "cq_1", &gate_cb_data), T0 + 1).await;
        // Seconda conferma con lo stesso id: gate già consumato.
        ch.dispatch(callback_update(3, CHAT, "cq_2", &gate_cb_data), T0 + 2).await;

        // Il secondo utilizzo deve aver inviato "Richiesta scaduta o non valida."
        let msgs = fake.recorded_messages();
        let found = msgs.iter().any(|(_, t)| t.contains("scaduta") || t.contains("non valida"));
        assert!(
            found,
            "atteso messaggio di gate scaduto/non valido alla seconda conferma, got: {msgs:?}"
        );
    }

    /// Callback "no:<id>" → messaggio "Annullato." e gate rimosso.
    #[tokio::test]
    async fn dispatch_callback_no_sends_cancelled_and_removes_gate() {
        let dir = TempDir::new().unwrap();
        let fake = Arc::new(FakeTelegramClient::new());
        let auth = auth_logged_in(CHAT, T0);
        let mut ch = make_channel(fake.clone(), auth, &dir);

        // Crea il gate.
        ch.dispatch(text_update(1, CHAT, "dir"), T0).await;
        let buttons = fake.recorded_buttons();
        let no_data = buttons[0].2[1].callback_data.clone(); // "no:<id>"

        ch.dispatch(callback_update(2, CHAT, "cq_no_1", &no_data), T0 + 1).await;

        let msgs = fake.recorded_messages();
        let found = msgs.iter().any(|(_, t)| t.contains("Annullato"));
        assert!(found, "atteso 'Annullato.' nei messaggi, got: {msgs:?}");

        // Verifica che il gate sia stato rimosso (un secondo "ok:" fallisce).
        let ok_data = buttons[0].2[0].callback_data.clone();
        let msgs_before = fake.recorded_messages().len();
        ch.dispatch(callback_update(3, CHAT, "cq_ok_late", &ok_data), T0 + 2).await;
        let msgs_after = fake.recorded_messages();
        // Deve essere stato inviato un messaggio "scaduta/non valida".
        let new_msgs = &msgs_after[msgs_before..];
        let found_expired = new_msgs.iter().any(|(_, t)| t.contains("scaduta") || t.contains("non valida"));
        assert!(
            found_expired,
            "dopo annullamento, ok: deve ritornare 'scaduta': {new_msgs:?}"
        );
    }

    // ─── dispatch: sessione scaduta ───────────────────────────────────────────

    /// Comando normale con sessione scaduta → NeedLogin message.
    #[tokio::test]
    async fn dispatch_expired_session_sends_need_login() {
        let dir = TempDir::new().unwrap();
        let fake = Arc::new(FakeTelegramClient::new());
        let auth = auth_logged_in(CHAT, T0);
        let mut ch = make_channel(fake.clone(), auth, &dir);

        // Avanza il tempo oltre la scadenza della sessione.
        let future = T0 + SESSION_TTL_MS + 1;
        ch.dispatch(text_update(1, CHAT, "spiega rust"), future).await;

        let msgs = fake.recorded_messages();
        assert!(!msgs.is_empty(), "atteso messaggio NeedLogin");
        let found = msgs.iter().any(|(_, t)| t.contains("Sessione scaduta") || t.contains("/login"));
        assert!(
            found,
            "atteso 'Sessione scaduta' o '/login', got: {msgs:?}"
        );
    }

    /// **UX (e2e):** un tap su un bottone "vecchio" quando la sessione è scaduta
    /// deve AVVISARE ("Sessione scaduta. Usa /login") invece di restare in
    /// silenzio — e NON deve eseguire il comando.
    #[tokio::test]
    async fn dispatch_callback_expired_session_replies_need_login() {
        let dir = TempDir::new().unwrap();
        let fake = Arc::new(FakeTelegramClient::new());
        let auth = auth_logged_in(CHAT, T0);
        let mut ch = make_channel(fake.clone(), auth, &dir);

        // Crea un gate valido a T0 (comando OS → bottoni di conferma).
        ch.dispatch(text_update(1, CHAT, "dir"), T0).await;
        let buttons = fake.recorded_buttons();
        let ok_data = buttons[0].2[0].callback_data.clone(); // "ok:<id>"

        // Tap "Esegui" DOPO la scadenza della sessione.
        let expired = T0 + SESSION_TTL_MS + 1;
        let before = fake.recorded_messages().len();
        ch.dispatch(callback_update(2, CHAT, "cq_late", &ok_data), expired).await;

        let after = fake.recorded_messages();
        let new_msgs = &after[before..];
        // (a) Deve avvisare, non restare in silenzio.
        assert!(
            new_msgs.iter().any(|(_, t)| t.contains("Sessione scaduta")),
            "tap su sessione scaduta deve avvisare, non restare in silenzio: {new_msgs:?}"
        );
        // (b) Il comando NON deve essere eseguito (nessun output del tool).
        assert!(
            !new_msgs.iter().any(|(_, t)| t.contains("output di test")),
            "sessione scaduta: il comando NON deve essere eseguito: {new_msgs:?}"
        );
        // (c) answer_callback comunque chiamato (toglie lo spinner sul bottone).
        assert!(
            fake.recorded_callbacks().contains(&"cq_late".to_string()),
            "answer_callback deve essere chiamato anche su sessione scaduta"
        );
    }

    // ─── dispatch: chat diversa da quella appaiata ────────────────────────────

    /// Update da chat non autorizzata (diversa da quella appaiata) → silenzio.
    #[tokio::test]
    async fn dispatch_wrong_chat_sends_nothing() {
        let dir = TempDir::new().unwrap();
        let fake = Arc::new(FakeTelegramClient::new());
        let auth = auth_logged_in(CHAT, T0);
        let mut ch = make_channel(fake.clone(), auth, &dir);

        // Chat 999 non è autorizzata.
        ch.dispatch(text_update(1, 999, "spiega rust"), T0).await;

        let msgs = fake.recorded_messages();
        assert!(
            msgs.is_empty(),
            "atteso silenzio per chat non autorizzata, got: {msgs:?}"
        );
    }

    // ─── dispatch: callback da chat non autorizzata ───────────────────────────

    /// Callback da chat non autorizzata → answer_callback chiamato (togliere spinner)
    /// ma nessun altro invio.
    #[tokio::test]
    async fn dispatch_callback_from_wrong_chat_only_answers_callback() {
        let dir = TempDir::new().unwrap();
        let fake = Arc::new(FakeTelegramClient::new());
        let auth = auth_logged_in(CHAT, T0);
        let mut ch = make_channel(fake.clone(), auth, &dir);

        // Callback da chat 999 (non autorizzata).
        ch.dispatch(callback_update(1, 999, "cq_x", "ok:fakeid"), T0).await;

        // answer_callback deve essere chiamato (per togliere lo spinner).
        let callbacks = fake.recorded_callbacks();
        assert_eq!(callbacks, vec!["cq_x".to_string()]);

        // Nessun messaggio inviato.
        let msgs = fake.recorded_messages();
        assert!(
            msgs.is_empty(),
            "atteso nessun messaggio per callback non autorizzata, got: {msgs:?}"
        );
    }

    // ─── dispatch: /pair senza codice ─────────────────────────────────────────

    /// /pair senza argomenti, bot non appaiato → invia istruzioni di pairing.
    #[tokio::test]
    async fn dispatch_pair_no_code_unpaired_sends_instructions() {
        let dir = TempDir::new().unwrap();
        let fake = Arc::new(FakeTelegramClient::new());
        let auth = auth_with_secret(None); // non appaiato
        let mut ch = make_channel(fake.clone(), auth, &dir);

        ch.dispatch(text_update(1, CHAT, "/pair"), T0).await;

        let msgs = fake.recorded_messages();
        assert!(!msgs.is_empty(), "atteso istruzioni di pairing");
    }

    // ─── run_command_buffered: output vuoto → "(nessun output)" ──────────────

    /// Quando handle_command non emette nessun testo formattabile (solo Done),
    /// run_command_buffered invia "(nessun output)".
    #[tokio::test]
    async fn run_command_buffered_empty_output_sends_fallback() {
        let dir = TempDir::new().unwrap();
        let fake = Arc::new(FakeTelegramClient::new());
        // FakeToolClient.with_output("", "", 0) → Done senza Chunk → format_response = ""
        let state_path = dir.path().join("state.json");
        let mut ch = TelegramChannel::new(
            fake.clone(),
            auth_logged_in(CHAT, T0),
            state_path,
            Arc::new(StubAdapter),
            Arc::new(FakeToolClient::with_output("", "", 0)),
        );

        // "/reset" → Chunk("sessione riavviata") + Done → non sarà vuoto
        // Usiamo un comando OS che produce output vuoto: FakeToolClient.with_output
        // è già impostato su ("", "", 0).
        // Invece, per forzare un output vuoto, usiamo direttamente run_command_buffered.
        ch.run_command_buffered(CHAT, "cls", CommandKind::Os).await;

        let msgs = fake.recorded_messages();
        // FakeToolClient.with_output("", "", 0) → stdout="", stderr="", exit=0
        // handle_os: no Chunk(stdout), no Chunk(stderr), solo Done{0}
        // format_response([Done]) = "" → fallback "(nessun output)"
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].1, "(nessun output)");
    }

    // ─── Wiring: il confirmer è RAGGIUNTO dal canale (Task 3) ─────────────────
    //
    // `StubAdapter` ignora il confirmer, quindi i test sopra non proverebbero che
    // `run_command_buffered` passa `Some(&confirmer)` (non `None`): una regressione
    // a `None` passerebbe inosservata. Questo adapter di prova chiama esplicitamente
    // il confirmer ed emette l'esito, così il test fallisce se il wiring passa `None`.

    struct ProbeAdapter;

    #[async_trait::async_trait]
    impl crate::ai_adapter::AiAdapter for ProbeAdapter {
        #[allow(clippy::too_many_arguments)]
        async fn respond(
            &self,
            id: &str,
            _input: &str,
            _history: &mut crate::messages_client::ConversationHistory,
            _tools: &dyn crate::tool_client::ToolClient,
            _opts: crate::agent::TurnOptions,
            confirmer: Option<&dyn crate::ai_adapter::ToolConfirmer>,
            _cancel: Option<tokio_util::sync::CancellationToken>,
            tx: tokio::sync::mpsc::UnboundedSender<ServerMsg>,
        ) {
            let outcome = match confirmer {
                Some(c) => format!("CONFIRM={}", c.confirm("$ probe").await),
                None => "CONFIRM=missing".to_string(),
            };
            let _ = tx.send(ServerMsg::Chunk {
                id: id.to_string(),
                content: outcome,
            });
            let _ = tx.send(ServerMsg::Done {
                id: id.to_string(),
                exit_code: None,
            });
        }

        // Non esercitato da questi test (che riguardano SOLO il wiring del confirmer su
        // `respond`, il path del cursore/Telegram); implementazione minima per
        // soddisfare il trait dopo l'aggiunta di `chat_reply` (AI Chat Slice 1a).
        async fn chat_reply(
            &self,
            _my_ai_label: &str,
            _history: &[crate::messages_client::Message],
            _request: &str,
            _cancel: Option<tokio_util::sync::CancellationToken>,
        ) -> String {
            String::new()
        }
    }

    /// Un comando NL (path AI) attraversa `run_command_buffered`, che costruisce
    /// `Some(&TelegramConfirmer)` e lo passa a `respond`. Con auto-conferma "Esegui"
    /// l'adapter riceve `true`. Usiamo l'orologio reale per allinearci alla sessione
    /// (il wiring usa `SystemTime::now`): costruiamo la sessione e dispatchiamo a
    /// `now`, così il TTL (30 min) copre ampiamente il delta di esecuzione.
    #[tokio::test]
    async fn run_command_reaches_confirmer_via_channel_wiring() {
        let dir = TempDir::new().unwrap();
        let fake = Arc::new(FakeTelegramClient::new());
        fake.set_auto_confirm(true); // l'utente tappa "Esegui"

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let state_path = dir.path().join("state.json");
        let mut ch = TelegramChannel::new(
            fake.clone(),
            auth_logged_in(CHAT, now),
            state_path,
            Arc::new(ProbeAdapter),
            Arc::new(FakeToolClient::success("x")),
        );

        // "spiega ..." → NL (non OS, non slash) → handle_nl → respond → confirmer.
        ch.dispatch(text_update(1, CHAT, "spiega qualcosa"), now).await;

        let msgs = fake.recorded_messages();
        assert!(
            msgs.iter().any(|(_, t)| t.contains("CONFIRM=true")),
            "il confirmer deve essere raggiunto dal wiring e ritornare true (tap Esegui): {msgs:?}"
        );
    }
}
