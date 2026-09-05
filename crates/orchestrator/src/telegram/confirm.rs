//! # telegram::confirm
//!
//! Round-trip poll↔conferma per i tool dell'AI (Task 1 — de-risk).
//!
//! `confirm_via_poll` manda i bottoni [Esegui]/[Annulla] e attende il tap
//! dell'utente guidando il polling in modo single-threaded (niente spawn,
//! niente Arc/Mutex). Vedi spec `Docs/15-telegram-ai-tool-gate.md`.

use std::sync::atomic::{AtomicI64, Ordering};

use async_trait::async_trait;

use crate::ai_adapter::ToolConfirmer;
use crate::telegram::auth::{AuthResult, Authenticator};
use crate::telegram::client::{InlineButton, TelegramClient};

/// Long-poll secondi per ogni getUpdates durante l'attesa di conferma.
const CONFIRM_POLL_TIMEOUT_S: u32 = 25;

/// Manda i bottoni [Esegui]/[Annulla] per `command` (id opaco `id`) e attende il
/// tap facendo lui stesso il polling (single-threaded). Ritorna true = esegui.
///
/// - tap `ok:{id}` dalla chat giusta → true; `no:{id}` → false;
/// - un MESSAGGIO intercorrente (da qualsiasi chat) → risponde "conferma in
///   sospeso" e lo scarta (offset avanza, niente replay);
/// - dopo `max_polls` cicli senza decisione → false (timeout).
///
/// `offset` è condiviso (`AtomicI64`) col loop principale: avanza monotòno.
/// Ordering::Relaxed è sufficiente: il confirmer opera sullo stesso thread del
/// loop principale (niente contesa concorrente), ma `AtomicI64` rende la
/// future `Send` (necessario per Task 2: `respond` gira in task spawnati da ws.rs).
/// `chat_id` è la chat appaiata: solo i suoi tap contano.
pub async fn confirm_via_poll(
    client: &dyn TelegramClient,
    chat_id: i64,
    command: &str,
    id: &str,
    offset: &AtomicI64,
    max_polls: u32,
) -> bool {
    let prompt = format!("Eseguo: {command} ?");
    let buttons = [
        InlineButton { text: "Esegui".to_string(), callback_data: format!("ok:{id}") },
        InlineButton { text: "Annulla".to_string(), callback_data: format!("no:{id}") },
    ];
    let _ = client.send_buttons(chat_id, &prompt, &buttons).await;

    for _ in 0..max_polls {
        let updates = match client.get_updates(offset.load(Ordering::Relaxed), CONFIRM_POLL_TIMEOUT_S).await {
            Ok(u) => u,
            Err(_) => continue, // errore di rete → riprova (entro max_polls)
        };
        for u in updates {
            offset.store(u.update_id + 1, Ordering::Relaxed);
            if let Some(cb) = u.callback_query {
                // Risponde sempre alla callback per rimuovere lo spinner sul bottone.
                let _ = client.answer_callback(&cb.id).await;
                if cb.chat_id != chat_id {
                    continue; // tap da un'altra chat → ignora
                }
                match cb.data.as_deref() {
                    Some(d) if d == format!("ok:{id}") => return true,
                    Some(d) if d == format!("no:{id}") => return false,
                    _ => continue, // tap di un'altra conferma → ignora
                }
            } else if u.message.is_some() {
                // Messaggio intercorrente: avvisa l'utente che la conferma è in attesa,
                // poi scarta il messaggio (l'offset è già avanzato sopra).
                let _ = client
                    .send_message(chat_id, "Conferma in sospeso: usa i bottoni [Esegui] / [Annulla].")
                    .await;
            }
        }
    }
    false // timeout: nega
}

// ─────────────────────────────────────────────────────────────────────────────
// TelegramConfirmer — gate in-loop dei tool dell'AI (Task 3, Docs/15)
// ─────────────────────────────────────────────────────────────────────────────

/// Numero massimo di cicli di polling durante l'attesa di una conferma.
/// 8 × `CONFIRM_POLL_TIMEOUT_S` (25s) ≈ 3,3 min di finestra massima per il tap.
pub(crate) const CONFIRM_MAX_POLLS: u32 = 8;

/// Gate di conferma per i tool dell'AI guidato da Telegram (impl [`ToolConfirmer`]).
///
/// A ogni proposta di tool da parte dell'AI (`run_in_session`/`open_target`),
/// `respond` chiama `confirm`, che:
/// 1. verifica che la sessione sia ancora valida (`authorize`) PRIMA di chiedere;
/// 2. manda i bottoni [Esegui]/[Annulla] con un **id opaco** (random, non
///    derivato dal comando) e attende il tap (`confirm_via_poll`, polling
///    single-thread sullo stesso `offset` del loop principale);
/// 3. **ri-verifica** la sessione AL TAP: può essere scaduta durante l'attesa.
///
/// Tutti i campi sono prestiti: il confirmer vive solo per la durata di una
/// chiamata a `handle_command`, condividendo `client`/`offset`/`auth` col canale
/// (prestiti disgiunti da `&mut history`).
///
/// `Send + Sync`: tutti i campi lo sono (`&dyn TelegramClient` e `&Authenticator`
/// — i cui trait/tipi sono `Sync` —, `&AtomicI64`, `Box<dyn Fn + Send + Sync>`),
/// quindi la future di `confirm` è `Send`, come richiesto dal supertrait di
/// [`ToolConfirmer`] (in `ws.rs` `respond` gira in task spawnati).
pub(crate) struct TelegramConfirmer<'a> {
    /// Client Telegram (per bottoni, messaggi, callback): condiviso col canale.
    client: &'a dyn TelegramClient,
    /// Chat appaiata: solo i suoi tap contano.
    chat_id: i64,
    /// Offset `getUpdates` condiviso col loop principale (avanza monotòno).
    offset: &'a AtomicI64,
    /// Autenticatore: ri-controllato all'inizio e al tap (solo `authorize`, `&self`).
    auth: &'a Authenticator,
    /// Clock iniettabile (ms epoch). Prod: `SystemTime::now`. Test: valore fisso.
    now_ms: Box<dyn Fn() -> u64 + Send + Sync>,
    /// Cicli di polling massimi prima del timeout (→ nega).
    max_polls: u32,
}

impl<'a> TelegramConfirmer<'a> {
    /// Costruisce un confirmer che borrowa client/offset/auth dal canale.
    pub(crate) fn new(
        client: &'a dyn TelegramClient,
        chat_id: i64,
        offset: &'a AtomicI64,
        auth: &'a Authenticator,
        now_ms: Box<dyn Fn() -> u64 + Send + Sync>,
        max_polls: u32,
    ) -> Self {
        Self {
            client,
            chat_id,
            offset,
            auth,
            now_ms,
            max_polls,
        }
    }
}

#[async_trait]
impl ToolConfirmer for TelegramConfirmer<'_> {
    async fn confirm(&self, command: &str) -> bool {
        // (1) Sessione valida PRIMA di chiedere conferma: se è scaduta, nega
        //     subito (non ha senso mostrare bottoni che poi rifiuteremmo).
        if self.auth.authorize(self.chat_id, (self.now_ms)()) != AuthResult::Ok {
            let _ = self
                .client
                .send_message(self.chat_id, "Sessione scaduta: usa /login prima di confermare.")
                .await;
            return false;
        }

        // (2) Id opaco a 128 bit (32 hex), generato a runtime e NON derivato dal
        //     comando: chi vede il callback_data non ne deduce il contenuto.
        let id = format!("{:032x}", rand::random::<u128>());
        if !confirm_via_poll(self.client, self.chat_id, command, &id, self.offset, self.max_polls).await {
            return false;
        }

        // (3) Ri-verifica AL TAP: l'attesa può aver superato il TTL di sessione.
        //     Un tap "Esegui" su sessione scaduta NON deve eseguire il comando.
        if self.auth.authorize(self.chat_id, (self.now_ms)()) != AuthResult::Ok {
            let _ = self
                .client
                .send_message(self.chat_id, "Sessione scaduta durante la conferma: comando annullato.")
                .await;
            return false;
        }

        true
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests (TDD — RED scritto prima dell'implementazione)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telegram::client::{CallbackQuery, FakeTelegramClient, Message, Update};

    // ── Helper per costruire Update con callback_query ────────────────────────

    fn make_callback_update(update_id: i64, cb_id: &str, chat_id: i64, data: &str) -> Update {
        Update {
            update_id,
            message: None,
            callback_query: Some(CallbackQuery {
                id: cb_id.to_string(),
                chat_id,
                data: Some(data.to_string()),
            }),
        }
    }

    fn make_message_update(update_id: i64, chat_id: i64, text: &str) -> Update {
        Update {
            update_id,
            message: Some(Message {
                chat_id,
                text: Some(text.to_string()),
            }),
            callback_query: None,
        }
    }

    // ── confirm_ok_tap_returns_true ───────────────────────────────────────────

    /// Tap `ok:{id}` dalla chat giusta → `confirm_via_poll` ritorna true.
    /// Verifica anche: answer_callback chiamato e offset avanzato.
    #[tokio::test]
    async fn confirm_ok_tap_returns_true() {
        let fake = FakeTelegramClient::new();
        let id = "test-id-42";
        let chat_id = 100i64;

        // update_id=5: dopo il tap l'offset deve essere 6.
        fake.push_updates(vec![make_callback_update(5, "cq_ok", chat_id, &format!("ok:{id}"))]);

        let offset = AtomicI64::new(0i64);
        let result = confirm_via_poll(&fake, chat_id, "ls -la", id, &offset, 2).await;

        assert!(result, "tap ok deve ritornare true");
        assert_eq!(offset.load(Ordering::Relaxed), 6, "offset deve avanzare a update_id+1");
        assert_eq!(
            fake.recorded_callbacks(),
            vec!["cq_ok".to_string()],
            "answer_callback deve essere stato chiamato"
        );
    }

    // ── confirm_no_tap_returns_false ─────────────────────────────────────────

    /// Tap `no:{id}` dalla chat giusta → `confirm_via_poll` ritorna false.
    #[tokio::test]
    async fn confirm_no_tap_returns_false() {
        let fake = FakeTelegramClient::new();
        let id = "test-id-99";
        let chat_id = 200i64;

        fake.push_updates(vec![make_callback_update(10, "cq_no", chat_id, &format!("no:{id}"))]);

        let offset = AtomicI64::new(0i64);
        let result = confirm_via_poll(&fake, chat_id, "rm -rf /tmp/foo", id, &offset, 2).await;

        assert!(!result, "tap no deve ritornare false");
        assert_eq!(offset.load(Ordering::Relaxed), 11, "offset avanzato a update_id+1");
        assert!(
            !fake.recorded_callbacks().is_empty(),
            "answer_callback deve essere chiamato anche per no"
        );
    }

    // ── confirm_timeout_returns_false ────────────────────────────────────────

    /// Nessun update in coda → dopo max_polls cicli ritorna false (timeout).
    #[tokio::test]
    async fn confirm_timeout_returns_false() {
        let fake = FakeTelegramClient::new();
        // Nessun push: tutte le chiamate a get_updates ritornano vec![].

        let offset = AtomicI64::new(0i64);
        let result = confirm_via_poll(&fake, 300, "echo hello", "timeout-id", &offset, 3).await;

        assert!(!result, "timeout deve ritornare false");
        // Nessun callback, nessun messaggio, nessun bottone di conferma (solo quello iniziale).
        assert!(
            fake.recorded_callbacks().is_empty(),
            "nessun answer_callback su timeout"
        );
    }

    // ── confirm_intervening_message_replies_pending_then_resolves ────────────

    /// Primo giro: update-messaggio → invia "conferma in sospeso".
    /// Secondo giro: tap `ok:{id}` → ritorna true.
    #[tokio::test]
    async fn confirm_intervening_message_replies_pending_then_resolves() {
        let fake = FakeTelegramClient::new();
        let id = "test-id-msg";
        let chat_id = 400i64;

        // Giro 1: un messaggio intercorrente (update_id=1).
        fake.push_updates(vec![make_message_update(1, chat_id, "ciao")]);
        // Giro 2: tap ok (update_id=2).
        fake.push_updates(vec![make_callback_update(2, "cq_resolve", chat_id, &format!("ok:{id}"))]);

        let offset = AtomicI64::new(0i64);
        let result = confirm_via_poll(&fake, chat_id, "open file.txt", id, &offset, 3).await;

        assert!(result, "dopo messaggio intercorrente il tap ok deve ritornare true");

        // Verifica il messaggio "in sospeso" inviato durante l'attesa.
        let msgs = fake.recorded_messages();
        assert!(
            msgs.iter().any(|(_, text)| text.contains("Conferma in sospeso")),
            "deve essere stato inviato il messaggio 'Conferma in sospeso': {:?}",
            msgs
        );
        // Verifica esatto testo del messaggio di sospeso.
        assert!(
            msgs.iter().any(|(_, text)| text == "Conferma in sospeso: usa i bottoni [Esegui] / [Annulla]."),
            "testo esatto del messaggio di sospeso non trovato: {:?}",
            msgs
        );
    }

    // ── confirm_tap_from_other_chat_ignored ──────────────────────────────────

    /// Tap `ok:{id}` con chat_id diverso → ignorato; poi code vuote → timeout false.
    /// Verifica che answer_callback venga chiamato comunque (spec: "answer_callback
    /// comunque" anche per tap da altra chat).
    #[tokio::test]
    async fn confirm_tap_from_other_chat_ignored() {
        let fake = FakeTelegramClient::new();
        let id = "test-id-other";
        let expected_chat_id = 500i64;
        let other_chat_id = 999i64; // chat diversa

        // Tap dalla chat sbagliata.
        fake.push_updates(vec![make_callback_update(
            20,
            "cq_other",
            other_chat_id,
            &format!("ok:{id}"),
        )]);
        // Secondo giro: vuoto → timeout.
        // (non serve push: get_updates ritorna vec![] per default)

        let offset = AtomicI64::new(0i64);
        let result = confirm_via_poll(&fake, expected_chat_id, "notepad.exe", id, &offset, 2).await;

        assert!(!result, "tap da altra chat deve portare a timeout false");
        // answer_callback viene chiamato comunque (spec: "answer_callback comunque").
        assert_eq!(
            fake.recorded_callbacks(),
            vec!["cq_other".to_string()],
            "answer_callback deve essere chiamato anche per tap da altra chat"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TelegramConfirmer — gate in-loop (Task 3): 3 casi di sicurezza
    // ─────────────────────────────────────────────────────────────────────────

    use crate::telegram::auth::{TelegramState, SESSION_TTL_MS};

    /// Secret TOTP fisso (stesso vettore usato in auth.rs/channel.rs).
    const TEST_SECRET_B32: &str = "JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP";
    /// Timestamp fisso (ms) per test deterministici.
    const T0: u64 = 1_700_000_000_000;

    /// Genera il codice TOTP corretto per `TEST_SECRET_B32` a `now_ms`.
    fn totp_code_at(now_ms: u64) -> String {
        use totp_rs::{Algorithm, Secret, TOTP};
        let bytes = Secret::Encoded(TEST_SECRET_B32.to_string()).to_bytes().unwrap();
        TOTP::new(
            Algorithm::SHA1,
            6,
            1,
            30,
            bytes,
            Some("Lare Terminal".to_string()),
            "lare".to_string(),
        )
        .unwrap()
        .generate(now_ms / 1000)
    }

    /// Costruisce un `Authenticator` con sessione attiva (paired + logged in a `now_ms`).
    fn authed(chat_id: i64, now_ms: u64) -> Authenticator {
        let state = TelegramState {
            totp_secret_base32: Some(TEST_SECRET_B32.to_string()),
            paired_chat_id: Some(chat_id),
        };
        let mut auth = Authenticator::from_state(&state).expect("secret valido");
        let code = totp_code_at(now_ms);
        assert_eq!(auth.try_login(chat_id, &code, now_ms), AuthResult::Ok, "setup login");
        auth
    }

    /// **Sicurezza: id opaco non derivato dal comando.**
    /// Due `confirm` consecutivi (timeout) producono bottoni con id esadecimale
    /// a 32 cifre, diversi tra loro e senza frammenti del comando; il prompt,
    /// invece, mostra il comando esatto (trasparenza per l'utente).
    #[tokio::test]
    async fn confirmer_uses_opaque_id_not_derived_from_command() {
        let fake = FakeTelegramClient::new();
        let chat = 100i64;
        let auth = authed(chat, T0);
        let offset = AtomicI64::new(0);
        // now_ms fisso a T0 (sessione valida); max_polls=1 → timeout immediato.
        let confirmer =
            TelegramConfirmer::new(&fake, chat, &offset, &auth, Box::new(|| T0), 1);

        assert!(!confirmer.confirm("$ rm -rf /tmp/segreto").await, "nessun tap → timeout false");
        assert!(!confirmer.confirm("$ cat /etc/passwd").await, "nessun tap → timeout false");

        let buttons = fake.recorded_buttons();
        assert_eq!(buttons.len(), 2, "un set di bottoni per ogni confirm");

        // (a) Il prompt mostra il comando esatto.
        assert!(buttons[0].1.contains("rm -rf /tmp/segreto"), "prompt 1: {:?}", buttons[0].1);
        assert!(buttons[1].1.contains("cat /etc/passwd"), "prompt 2: {:?}", buttons[1].1);

        // (b) callback_data = "ok:{id}"/"no:{id}" con id opaco a 32 hex.
        let extract_id = |data: &str| data.strip_prefix("ok:").map(str::to_string);
        let id0 = extract_id(&buttons[0].2[0].callback_data).expect("ok:<id>");
        let id1 = extract_id(&buttons[1].2[0].callback_data).expect("ok:<id>");
        assert_eq!(id0.len(), 32, "id a 32 hex");
        assert!(id0.chars().all(|c| c.is_ascii_hexdigit()), "id solo esadecimale: {id0}");
        assert_ne!(id0, id1, "due id consecutivi devono differire (random)");

        // (c) L'id NON contiene frammenti del comando.
        assert!(!id0.contains("rm") && !id0.contains("segreto"), "id non deriva dal comando: {id0}");
    }

    /// **Sicurezza: sessione scaduta AL TAP → nega** (anche con tap "Esegui").
    /// La sessione è valida all'inizio (passa il primo `authorize`), ma il clock
    /// avanza oltre il TTL prima della ri-verifica: il tap `ok` non deve eseguire.
    #[tokio::test]
    async fn confirmer_session_expired_at_tap_denies() {
        use std::sync::atomic::AtomicU32;
        let fake = FakeTelegramClient::new();
        fake.set_auto_confirm(true); // l'utente tappa "Esegui"
        let chat = 200i64;
        let auth = authed(chat, T0);
        let offset = AtomicI64::new(0);

        // Clock a 2 fasi: 1ª chiamata (inizio) = T0 (valida); 2ª (ri-verifica al
        // tap) = oltre il TTL → scaduta. NB: `confirm_via_poll` NON chiama mai
        // `now_ms`, quindi il contatore conta esattamente le 2 chiamate di `confirm`.
        let calls = AtomicU32::new(0);
        let now_ms = move || {
            if calls.fetch_add(1, Ordering::Relaxed) == 0 {
                T0
            } else {
                T0 + SESSION_TTL_MS + 1
            }
        };
        let confirmer =
            TelegramConfirmer::new(&fake, chat, &offset, &auth, Box::new(now_ms), 4);

        assert!(
            !confirmer.confirm("$ shutdown now").await,
            "tap ok ma sessione scaduta al ri-controllo → deve negare"
        );
        // Deve aver avvisato dell'annullamento per scadenza.
        let msgs = fake.recorded_messages();
        assert!(
            msgs.iter().any(|(_, t)| t.contains("Sessione scaduta durante la conferma")),
            "atteso avviso di scadenza durante la conferma: {msgs:?}"
        );
    }

    /// **Sicurezza: timeout → nega + uscita garantita.**
    /// Nessun tap: dopo `max_polls` cicli `confirm` ritorna false senza bloccarsi.
    #[tokio::test]
    async fn confirmer_timeout_denies_and_returns() {
        let fake = FakeTelegramClient::new(); // auto_confirm off → nessun tap
        let chat = 300i64;
        let auth = authed(chat, T0);
        let offset = AtomicI64::new(0);
        let confirmer =
            TelegramConfirmer::new(&fake, chat, &offset, &auth, Box::new(|| T0), 3);

        assert!(!confirmer.confirm("$ format C:").await, "timeout deve negare");
        // I bottoni sono stati inviati una sola volta (nessun tap ricevuto).
        assert_eq!(fake.recorded_buttons().len(), 1);
        // Nessuna callback risolta.
        assert!(fake.recorded_callbacks().is_empty(), "nessun tap → nessun answer_callback");
    }
}
