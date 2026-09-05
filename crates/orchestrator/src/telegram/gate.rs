//! # telegram::gate — gate predicato + store dei pending
//!
//! Due responsabilità:
//!
//! 1. **`needs_confirmation`** — predicato puro che stabilisce se un input
//!    richiede la conferma dell'utente prima di essere eseguito via Telegram.
//!    Riusa `crate::router::classify` come unica fonte di verità sulla route.
//!
//! 2. **`PendingGates`** — store in-memory dei comandi in attesa di conferma.
//!    Ogni entry è single-use (consumata al `take`) e ha una scadenza.
//!    L'id opaco viene fornito dal chiamante (es. random via `rand`) — il
//!    gate non conosce né genera id.
//!
//! ## Logica del predicato (Docs/14 §3)
//!
//! | Route       | Gate? |
//! |-------------|-------|
//! | `Route::Os` | true  |
//! | `Route::Slash` con prima parola `"open"` o `"web"` | true |
//! | `Route::Slash` con altra parola | false |
//! | `Route::Nl` | false |
//!
//! `/open` e `/web` passano dal gate perché toccano il sistema dall'esterno:
//! `open_target` lancia file/app/URL sul PC; `/web` apre il browser.
//! `/reset`, `/show`, `/help`, `/nowin`, `/config`, `/library` non passano dal gate:
//! sono operazioni UI-locali o a basso rischio.
//!
//! ## Sicurezza
//! - `now_ms` è sempre iniettato — nessun clock reale in questo modulo.
//! - `take` è single-use: ogni conferma consuma il pending.
//! - L'id opaco NON deriva dal contenuto del comando (non forgiabile).

use std::collections::HashMap;

use protocol::CommandKind;

use crate::router::{classify, Route};

// ── Costanti ─────────────────────────────────────────────────────────────────

/// Tempo di vita di un pending gate in millisecondi.
/// Un comando in attesa di conferma scade dopo 2 minuti.
pub const GATE_TTL_MS: u64 = 120_000; // 2 minuti

// ── needs_confirmation ────────────────────────────────────────────────────────

/// Restituisce `true` se l'input richiede conferma prima di essere eseguito
/// da remoto via Telegram.
///
/// **Predicato (Docs/14 §3):**
/// - `Route::Os` → `true` (comandi shell diretti: toccano il sistema).
/// - `Route::Slash` la cui prima parola è `"open"` o `"web"` → `true`
///   (`/open` lancia file/app/URL; `/web` apre il browser).
/// - Tutti gli altri casi → `false`.
///
/// # Arguments
/// * `input`        — input grezzo (come arriva da Telegram).
/// * `command_type` — hint di routing del canale (`Auto` di norma).
pub fn needs_confirmation(input: &str, command_type: CommandKind) -> bool {
    match classify(input, command_type) {
        Route::Os => true,
        Route::Nl => false,
        Route::Slash => {
            // Estrai la prima parola dopo il '/', lowercase, per il confronto.
            // `classify` ha già verificato che `input.trim()` inizia con '/';
            // strip_prefix gestisce il caso degenere "/" senza parole.
            let first_word = input
                .trim()
                .strip_prefix('/')
                .unwrap_or("")
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_lowercase();
            matches!(first_word.as_str(), "open" | "web")
        }
    }
}

// ── PendingGates ──────────────────────────────────────────────────────────────

/// Una singola entry in attesa di conferma.
struct Pending {
    input: String,
    kind: CommandKind,
    /// Timestamp di scadenza in millisecondi (wall clock iniettato).
    expiry_ms: u64,
}

/// Store in-memory dei comandi in attesa di conferma via inline keyboard.
///
/// Invarianti di sicurezza:
/// - Single-use: `take` rimuove sempre l'entry (valida o scaduta).
/// - L'id è opaco e fornito dal chiamante; il gate non lo genera né lo ispeziona.
/// - `now_ms` è sempre iniettato — nessuna chiamata a `SystemTime` qui.
#[derive(Default)]
pub struct PendingGates {
    map: HashMap<String, Pending>,
}

impl PendingGates {
    /// Crea un nuovo store vuoto.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registra un comando in attesa di conferma.
    ///
    /// L'id opaco `id` è fornito dal chiamante (tipicamente un UUID o stringa
    /// random). Il pending scade a `now_ms + GATE_TTL_MS`.
    ///
    /// # Arguments
    /// * `id`      — identificatore opaco (non derivato dal comando).
    /// * `input`   — testo del comando originale.
    /// * `kind`    — hint di routing originale.
    /// * `now_ms`  — timestamp corrente in millisecondi.
    pub fn insert(&mut self, id: &str, input: &str, kind: CommandKind, now_ms: u64) {
        self.map.insert(
            id.to_string(),
            Pending {
                input: input.to_string(),
                kind,
                expiry_ms: now_ms + GATE_TTL_MS,
            },
        );
    }

    /// Preleva e **rimuove** un pending (single-use) se presente e non scaduto.
    ///
    /// - Entry presente e `now_ms < expiry_ms` → ritorna `Some((input, kind))`.
    /// - Entry scaduta (`now_ms >= expiry_ms`) → rimuove e ritorna `None`.
    /// - Entry assente → `None`.
    pub fn take(&mut self, id: &str, now_ms: u64) -> Option<(String, CommandKind)> {
        // Rimuove sempre l'entry (sia per uso che per scadenza): single-use.
        let pending = self.map.remove(id)?;
        if now_ms < pending.expiry_ms {
            Some((pending.input, pending.kind))
        } else {
            // Scaduto: già rimosso, ritorna None.
            None
        }
    }

    /// Rimuove tutti i pending scaduti (`now_ms >= expiry_ms`).
    ///
    /// Chiamato periodicamente per evitare memory leak su pending mai confermati.
    pub fn purge_expired(&mut self, now_ms: u64) {
        self.map.retain(|_, p| now_ms < p.expiry_ms);
    }
}

// ── Tests (TDD: scritti PRIMA dell'implementazione — vedi ciclo RED→GREEN) ───

#[cfg(test)]
mod tests {
    use super::{needs_confirmation, PendingGates, GATE_TTL_MS};
    use protocol::CommandKind;

    // ── needs_confirmation — Os variants ─────────────────────────────────────

    #[test]
    fn os_dir_needs_confirmation() {
        // "dir" è un known shell token → Route::Os → true
        assert!(needs_confirmation("dir", CommandKind::Auto));
    }

    #[test]
    fn os_dollar_rm_needs_confirmation() {
        // "$"-prefix forza Route::Os → true
        assert!(needs_confirmation("$ rm x", CommandKind::Auto));
    }

    #[test]
    fn explicit_os_kind_needs_confirmation() {
        // CommandKind::Os sempre → Route::Os → true
        assert!(needs_confirmation("anything", CommandKind::Os));
    }

    // ── needs_confirmation — Slash /open e /web → true ───────────────────────

    #[test]
    fn slash_open_needs_confirmation() {
        assert!(needs_confirmation("/open foo", CommandKind::Auto));
    }

    #[test]
    fn slash_open_uppercase_needs_confirmation() {
        // case-insensitive: /OPEN passa dal gate
        assert!(needs_confirmation("/OPEN foo", CommandKind::Auto));
    }

    #[test]
    fn slash_web_needs_confirmation() {
        assert!(needs_confirmation("/web gatti", CommandKind::Auto));
    }

    #[test]
    fn slash_open_with_leading_whitespace_needs_confirmation() {
        // classify fa trim prima del check '/'; il nostro extractor fa lo stesso
        assert!(needs_confirmation("  /open x", CommandKind::Auto));
    }

    // ── needs_confirmation — Slash che NON gatano → false ────────────────────

    #[test]
    fn slash_show_does_not_need_confirmation() {
        assert!(!needs_confirmation("/show some markdown", CommandKind::Auto));
    }

    #[test]
    fn slash_reset_does_not_need_confirmation() {
        assert!(!needs_confirmation("/reset", CommandKind::Auto));
    }

    #[test]
    fn slash_help_does_not_need_confirmation() {
        assert!(!needs_confirmation("/help", CommandKind::Auto));
    }

    #[test]
    fn slash_nowin_does_not_need_confirmation() {
        // /nowin è intercettato in core prima di classify ma qui viene visto come
        // Route::Slash; "nowin" ∉ {"open","web"} → false
        assert!(!needs_confirmation("/nowin some prompt", CommandKind::Auto));
    }

    #[test]
    fn slash_config_does_not_need_confirmation() {
        assert!(!needs_confirmation("/config", CommandKind::Auto));
    }

    #[test]
    fn slash_library_does_not_need_confirmation() {
        assert!(!needs_confirmation("/library", CommandKind::Auto));
    }

    // ── needs_confirmation — NL → false ──────────────────────────────────────

    #[test]
    fn nl_sentence_does_not_need_confirmation() {
        assert!(!needs_confirmation("spiega async await", CommandKind::Auto));
    }

    #[test]
    fn explicit_nl_kind_does_not_need_confirmation() {
        // CommandKind::Nl → Route::Nl → false (anche se input sembra OS)
        assert!(!needs_confirmation("dir", CommandKind::Nl));
    }

    // ── PendingGates — insert → take consuma (single-use) ────────────────────

    #[test]
    fn insert_then_take_returns_input_and_consumes() {
        let mut gates = PendingGates::new();
        let now = 0u64;
        gates.insert("id-1", "dir", CommandKind::Auto, now);
        let result = gates.take("id-1", now + 1);
        assert_eq!(result, Some(("dir".to_string(), CommandKind::Auto)));
        // seconda take — consumato
        assert_eq!(gates.take("id-1", now + 1), None);
    }

    #[test]
    fn take_unknown_id_returns_none() {
        let mut gates = PendingGates::new();
        assert_eq!(gates.take("ghost", 0), None);
    }

    // ── PendingGates — scadenza ───────────────────────────────────────────────

    #[test]
    fn take_expired_returns_none_and_removes_entry() {
        let mut gates = PendingGates::new();
        let now = 1_000u64;
        gates.insert("id-exp", "rm -rf /", CommandKind::Auto, now);
        // Avanza oltre la scadenza
        let past_expiry = now + GATE_TTL_MS + 1;
        assert_eq!(gates.take("id-exp", past_expiry), None);
        // Verifica che sia stato rimosso (non solo None al momento)
        assert_eq!(gates.take("id-exp", now), None);
    }

    #[test]
    fn take_exactly_at_expiry_boundary_is_expired() {
        // expiry_ms = now + GATE_TTL_MS; condizione: now_ms < expiry_ms
        // all'uguaglianza → scaduto → None
        let mut gates = PendingGates::new();
        let now = 500u64;
        gates.insert("id-boundary", "ls", CommandKind::Auto, now);
        let expiry = now + GATE_TTL_MS;
        assert_eq!(gates.take("id-boundary", expiry), None);
    }

    #[test]
    fn take_one_ms_before_expiry_succeeds() {
        let mut gates = PendingGates::new();
        let now = 500u64;
        gates.insert("id-valid", "git status", CommandKind::Auto, now);
        let just_before = now + GATE_TTL_MS - 1;
        let result = gates.take("id-valid", just_before);
        assert!(result.is_some());
    }

    // ── PendingGates — purge_expired ──────────────────────────────────────────

    #[test]
    fn purge_expired_removes_expired_keeps_valid() {
        let mut gates = PendingGates::new();
        let now = 1_000u64;
        // "expired" è inserito a `now`: scade a now + GATE_TTL_MS
        gates.insert("expired", "rm foo", CommandKind::Os, now);
        // "valid" è inserito a `now + GATE_TTL_MS`: scade a now + 2*GATE_TTL_MS
        gates.insert("valid", "ls", CommandKind::Auto, now + GATE_TTL_MS);

        // purga a now + GATE_TTL_MS + 1: "expired" è andato, "valid" è ancora vivo
        let purge_time = now + GATE_TTL_MS + 1;
        gates.purge_expired(purge_time);

        assert_eq!(gates.take("expired", purge_time), None);
        // "valid" ha expiry = now + 2*GATE_TTL_MS > purge_time → ancora vivo
        assert!(gates.take("valid", purge_time).is_some());
    }

    #[test]
    fn purge_expired_empty_store_does_not_panic() {
        let mut gates = PendingGates::new();
        gates.purge_expired(9_999_999);
    }
}
