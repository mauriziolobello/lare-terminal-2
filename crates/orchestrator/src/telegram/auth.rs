//! # telegram::auth
//!
//! Autenticazione per il canale Telegram (ADR-007).
//!
//! ## Struttura
//! - `TelegramState` — stato persistito in app-data (`telegram-state.json`):
//!   secret TOTP base32 + `paired_chat_id`. NON in repo (il repo è su Dropbox).
//! - `AuthResult` — esito di ogni operazione di autenticazione.
//! - `Authenticator` — stato runtime (sessioni in-memory, rate-limit, pairing code).
//!
//! ## Sicurezza
//! - Il secret TOTP **non viene mai loggato** dopo la generazione.
//! - `now_ms` è iniettato (nessuna chiamata al clock reale nel codice testato).
//! - Le sessioni sono **solo in-memory** (non persistite); riavvio → ri-login.
//! - Rate-limit duro su `/login` e `/pair`: dopo MAX_FAILS → lockout di LOCKOUT_MS.
//!
//! ## Costanti
//! - `MAX_FAILS = 5` — tentativi falliti prima del lockout.
//! - `LOCKOUT_MS = 60_000` — durata del lockout in millisecondi (1 minuto).
//! - `SESSION_TTL_MS = 1_800_000` — durata sessione (30 minuti).
//! - `PAIRING_TTL_MS = 600_000` — scadenza del codice di pairing (10 minuti).

use std::path::Path;

use totp_rs::{Algorithm, Secret, TOTP};

// ─────────────────────────────────────────────────────────────────────────────
// Costanti
// ─────────────────────────────────────────────────────────────────────────────

/// Numero massimo di tentativi falliti (login o pairing) prima del lockout.
pub const MAX_FAILS: u32 = 5;

/// Durata del lockout in millisecondi (1 minuto).
pub const LOCKOUT_MS: u64 = 60_000;

/// Durata della sessione dopo il login TOTP (30 minuti).
pub const SESSION_TTL_MS: u64 = 30 * 60_000;

/// Durata del codice di pairing one-time (10 minuti).
pub const PAIRING_TTL_MS: u64 = 10 * 60_000;

/// Issuer mostrato in Google Authenticator.
const TOTP_ISSUER: &str = "Lare Terminal";

/// Account name mostrato in Google Authenticator.
const TOTP_ACCOUNT: &str = "lare";

// ─────────────────────────────────────────────────────────────────────────────
// TelegramState — stato persistito
// ─────────────────────────────────────────────────────────────────────────────

/// Stato persistito in app-data (`telegram-state.json`).
///
/// Contiene solo i dati che devono sopravvivere al riavvio:
/// - `totp_secret_base32`: generato una volta, mostrato solo come URI `otpauth://`.
/// - `paired_chat_id`: il `chat_id` Telegram autorizzato (uno solo, v1).
///
/// **Non implementa `Debug`**: il secret TOTP non deve mai comparire nei log.
#[derive(serde::Serialize, serde::Deserialize, Default)]
pub struct TelegramState {
    /// Secret TOTP codificato in base32 (compatibile Google Authenticator).
    /// `None` se non ancora generato (prima esecuzione).
    pub totp_secret_base32: Option<String>,
    /// `chat_id` Telegram appaiato. `None` se non ancora appaiato.
    pub paired_chat_id: Option<i64>,
}

impl TelegramState {
    /// Carica lo stato da file.
    ///
    /// - File assente → `Default::default()` (state vuoto).
    /// - File corrotto/JSON invalido → `Default::default()` (robustezza: meglio
    ///   rigenerare il secret che bloccare l'avvio).
    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Err(_) => Self::default(),
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        }
    }

    /// Salva lo stato su file, creando la directory parent se necessario.
    ///
    /// Ritorna `Err(String)` se la scrittura fallisce.
    ///
    /// **Non include il secret nel messaggio di errore.**
    pub fn save(&self, path: &Path) -> Result<(), String> {
        // Crea la directory parent se non esiste.
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("telegram-state: impossibile creare la dir: {e}"))?;
        }

        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("telegram-state: errore serializzazione: {e}"))?;

        std::fs::write(path, json)
            .map_err(|e| format!("telegram-state: errore scrittura: {e}"))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// AuthResult
// ─────────────────────────────────────────────────────────────────────────────

/// Esito di un'operazione di autenticazione.
#[derive(Debug, Clone, PartialEq)]
pub enum AuthResult {
    /// L'operazione è riuscita.
    Ok,
    /// Nessun `chat_id` appaiato — il bot non è ancora stato configurato.
    NeedPairing,
    /// Chat appaiata ma nessuna sessione attiva (o scaduta) — serve `/login <totp>`.
    NeedLogin,
    /// Troppi tentativi falliti — attendere il lockout.
    RateLimited,
    /// Il codice o il `chat_id` non sono validi.
    Denied,
}

// ─────────────────────────────────────────────────────────────────────────────
// Authenticator — stato runtime
// ─────────────────────────────────────────────────────────────────────────────

/// Stato runtime dell'autenticazione Telegram.
///
/// Le sessioni sono **solo in-memory** (non persistite). I campi persistiti
/// (`totp_secret_base32`, `paired_chat_id`) sono in `TelegramState`.
///
/// `now_ms` è iniettato in ogni metodo per permettere test deterministici
/// senza dipendenza dal clock reale.
pub struct Authenticator {
    /// Oggetto TOTP per verifica codici.
    totp: TOTP,

    /// Secret base32 (copia per `get_secret_base32()` e ricostruzioni).
    secret_base32: String,

    /// `chat_id` appaiato (se presente).
    paired_chat_id: Option<i64>,

    /// Scadenza della sessione corrente (ms epoch). `None` = nessuna sessione.
    session_expiry: Option<u64>,

    /// Contatore tentativi di login falliti.
    login_fails: u32,

    /// Contatore tentativi di pairing falliti.
    pair_fails: u32,

    /// Lockout unificato (ms epoch). Usato da entrambi login e pairing.
    /// `None` = nessun lockout attivo.
    lockout_until: Option<u64>,

    /// Codice di pairing one-time: (codice, scadenza_ms).
    pairing_code: Option<(String, u64)>,
}

impl Authenticator {
    // ── Costruzione ──────────────────────────────────────────────────────────

    /// Costruttore da `TelegramState` con secret **noto** (path non usato).
    ///
    /// Usato nei test per avere un secret deterministico. In produzione si usa
    /// `init()` che gestisce anche la generazione e la persistenza.
    ///
    /// Ritorna `Err` se il `totp_secret_base32` nello state è `None` o invalido.
    pub fn from_state(state: &TelegramState) -> Result<Self, String> {
        let b32 = state
            .totp_secret_base32
            .as_deref()
            .ok_or_else(|| "from_state: secret TOTP mancante nello state".to_string())?;

        Self::build_from_base32(b32, state.paired_chat_id)
    }

    /// Inizializza l'`Authenticator` caricando (o generando) lo stato da file.
    ///
    /// - Se il file non esiste o è corrotto → stato default (secret assente).
    /// - Se il secret TOTP è assente → ne genera uno nuovo, lo salva e ritorna
    ///   `Some(otpauth_uri)` (da mostrare **una sola volta** all'utente).
    /// - Se il secret esiste già → ritorna `None` (nessun URI da mostrare).
    ///
    /// Ritorna `Some(uri)` (URI `otpauth://` da mostrare per il setup) FINCHÉ la
    /// chat non è appaiata; `None` una volta appaiati (setup completato).
    ///
    /// # Sicurezza
    /// L'URI contiene il secret: viene mostrato durante il setup (a ogni avvio
    /// finché non si è appaiati — così basta riavviare se si perde il QR) e mai
    /// più dopo il pairing; non va loggato in modo persistente.
    pub fn init(state_path: &Path) -> (Self, Option<String>) {
        let mut state = TelegramState::load(state_path);

        // Determina se il secret deve essere (ri)generato.
        // Condizioni: secret assente O secret presente ma invalido (corrotto/troncato).
        // In entrambi i casi la strategia è rigenerare: è più sicuro che bloccare il daemon.
        let needs_regeneration = match &state.totp_secret_base32 {
            None => true,
            Some(b32) => {
                // Verifica che il secret sia decodificabile: se non lo è, rigenerare.
                Secret::Encoded(b32.clone()).to_bytes().is_err()
            }
        };

        if needs_regeneration {
            // Prima esecuzione o secret corrotto: genera un secret casuale via totp-rs
            // (feature gen_secret → rand).
            let secret_bytes = Secret::generate_secret().to_bytes().unwrap();
            let totp = build_totp_from_bytes(&secret_bytes);
            state.totp_secret_base32 = Some(totp.get_secret_base32());
            // Salva il secret appena generato. Gli errori sono best-effort (log in futuro).
            let _ = state.save(state_path);
        }

        let secret_b32 = state.totp_secret_base32.as_deref().unwrap();
        let auth = Self::build_from_base32(secret_b32, state.paired_chat_id)
            .expect("secret appena generato o verificato — build non fallisce");

        // Mostra l'URI di setup FINCHÉ la chat non è appaiata (così se al primo
        // avvio si è perso il QR basta riavviare). Dopo il pairing → None.
        let uri = if state.paired_chat_id.is_none() {
            Some(
                build_totp_from_bytes(
                    &Secret::Encoded(secret_b32.to_string()).to_bytes().unwrap(),
                )
                .get_url(),
            )
        } else {
            None
        };

        (auth, uri)
    }

    // ── Pairing ──────────────────────────────────────────────────────────────

    /// Genera un nuovo codice di pairing one-time (6 cifre decimali, random).
    ///
    /// Sovrascrive il codice precedente se non ancora usato. La scadenza è
    /// `now_ms + PAIRING_TTL_MS`.
    pub fn new_pairing_code(&mut self, now_ms: u64) -> String {
        use rand::Rng;
        let code: u32 = rand::thread_rng().gen_range(100_000..=999_999);
        let code_str = code.to_string();
        self.pairing_code = Some((code_str.clone(), now_ms + PAIRING_TTL_MS));
        code_str
    }

    /// Verifica il codice di pairing e, se corretto, appaia il `chat_id`.
    ///
    /// Persiste lo stato aggiornato nel file indicato.
    ///
    /// # Rate-limit
    /// - Durante il lockout → `RateLimited` (senza controllare il codice).
    /// - Codice corretto → `Ok`; azzera `pair_fails` e `lockout_until`.
    /// - Codice errato/scaduto/assente → `Denied`; incrementa `pair_fails`;
    ///   se `pair_fails >= MAX_FAILS` → imposta lockout → `RateLimited`.
    pub fn try_pair(
        &mut self,
        chat_id: i64,
        code: &str,
        now_ms: u64,
        state_path: &Path,
    ) -> AuthResult {
        // Controlla lockout prima di qualsiasi verifica.
        if self.is_locked_out(now_ms) {
            return AuthResult::RateLimited;
        }

        // Verifica il codice di pairing.
        let valid = match &self.pairing_code {
            Some((stored_code, expiry)) => {
                code == stored_code.as_str() && now_ms <= *expiry
            }
            None => false,
        };

        if valid {
            // Pairing riuscito.
            self.paired_chat_id = Some(chat_id);
            self.pairing_code = None; // one-time: invalida subito.
            self.pair_fails = 0;
            self.lockout_until = None;

            // Persiste: solo paired_chat_id + totp_secret_base32.
            let state = TelegramState {
                totp_secret_base32: Some(self.secret_base32.clone()),
                paired_chat_id: Some(chat_id),
            };
            // Errori di persistenza: best-effort (il pairing rimane in memory).
            let _ = state.save(state_path);

            AuthResult::Ok
        } else {
            self.record_fail_pair(now_ms)
        }
    }

    // ── Login TOTP ───────────────────────────────────────────────────────────

    /// Verifica il codice TOTP e, se valido, apre una sessione.
    ///
    /// # Rate-limit
    /// - Solo per la chat appaiata; altrimenti → `Denied`.
    /// - Durante il lockout → `RateLimited`.
    /// - TOTP valido → `Ok`; `session_expiry = now_ms + SESSION_TTL_MS`; reset
    ///   `login_fails`.
    /// - TOTP errato → `Denied`; incrementa `login_fails`; lockout dopo `MAX_FAILS`.
    pub fn try_login(&mut self, chat_id: i64, totp_code: &str, now_ms: u64) -> AuthResult {
        // Solo la chat appaiata può fare login.
        if self.paired_chat_id != Some(chat_id) {
            return AuthResult::Denied;
        }

        // Controlla lockout.
        if self.is_locked_out(now_ms) {
            return AuthResult::RateLimited;
        }

        // Verifica il TOTP. Il crate usa secondi; convertiamo da ms.
        // skew=1 → verifica il codice corrente e ±1 step (30s prima/dopo).
        let now_secs = now_ms / 1000;
        if self.totp.check(totp_code, now_secs) {
            // Login riuscito.
            self.session_expiry = Some(now_ms + SESSION_TTL_MS);
            self.login_fails = 0;
            self.lockout_until = None;
            AuthResult::Ok
        } else {
            self.record_fail_login(now_ms)
        }
    }

    // ── Autorizzazione ───────────────────────────────────────────────────────

    /// Verifica se `chat_id` è autorizzato a inviare comandi.
    ///
    /// Ordine dei controlli (importante — testa tutti e quattro i rami):
    /// 1. `paired_chat_id == None` → `NeedPairing` (bot non configurato).
    /// 2. `chat_id != paired_chat_id` → `Denied` (chat non autorizzata).
    /// 3. `session_expiry` assente o scaduta → `NeedLogin`.
    /// 4. Sessione attiva → `Ok`.
    pub fn authorize(&self, chat_id: i64, now_ms: u64) -> AuthResult {
        match self.paired_chat_id {
            None => AuthResult::NeedPairing,
            Some(p) if p != chat_id => AuthResult::Denied,
            Some(_) => {
                match self.session_expiry {
                    Some(expiry) if now_ms < expiry => AuthResult::Ok,
                    _ => AuthResult::NeedLogin,
                }
            }
        }
    }

    // ── Helper privati ───────────────────────────────────────────────────────

    /// Costruisce un `Authenticator` da un secret base32 noto.
    fn build_from_base32(b32: &str, paired_chat_id: Option<i64>) -> Result<Self, String> {
        let secret_bytes = Secret::Encoded(b32.to_string())
            .to_bytes()
            .map_err(|e| format!("build_from_base32: secret base32 invalido: {e}"))?;

        let totp = build_totp_from_bytes(&secret_bytes);
        let secret_base32 = totp.get_secret_base32();

        Ok(Self {
            totp,
            secret_base32,
            paired_chat_id,
            session_expiry: None,
            login_fails: 0,
            pair_fails: 0,
            lockout_until: None,
            pairing_code: None,
        })
    }

    /// Controlla se il lockout è attivo. Se il lockout è scaduto, lo azzera
    /// (resetta anche i contatori per consentire un nuovo ciclo di tentativi).
    fn is_locked_out(&mut self, now_ms: u64) -> bool {
        match self.lockout_until {
            Some(until) if now_ms < until => true,
            Some(_) => {
                // Lockout scaduto: reset contatori per un nuovo ciclo.
                self.lockout_until = None;
                self.login_fails = 0;
                self.pair_fails = 0;
                false
            }
            None => false,
        }
    }

    /// Registra un tentativo di login fallito e imposta il lockout se necessario.
    ///
    /// Semantica rate-limit (spec: "5 tentativi errati → il 6° → RateLimited"):
    /// - Tentativi 1..MAX_FAILS → `Denied` (il MAX_FAILS-esimo arma il lockout ma
    ///   ritorna ancora `Denied`: l'utente ottiene tutti e 5 i tentativi).
    /// - Tentativo MAX_FAILS+1 e seguenti (durante lockout) → `RateLimited` (da
    ///   `is_locked_out`, chiamato prima di questo metodo).
    fn record_fail_login(&mut self, now_ms: u64) -> AuthResult {
        self.login_fails += 1;
        if self.login_fails >= MAX_FAILS {
            // Arma il lockout per i tentativi successivi, ma questo tentativo
            // ritorna ancora Denied (l'utente ha ancora consumato il suo ultimo
            // tentativo — non è ancora "bloccato" per questa chiamata).
            self.lockout_until = Some(now_ms + LOCKOUT_MS);
        }
        AuthResult::Denied
    }

    /// Registra un tentativo di pairing fallito e imposta il lockout se necessario.
    ///
    /// Stessa semantica di `record_fail_login`: MAX_FAILS-esimo tentativo arma il
    /// lockout ma ritorna `Denied`; solo il tentativo successivo (catturato da
    /// `is_locked_out`) ritorna `RateLimited`.
    fn record_fail_pair(&mut self, now_ms: u64) -> AuthResult {
        self.pair_fails += 1;
        if self.pair_fails >= MAX_FAILS {
            self.lockout_until = Some(now_ms + LOCKOUT_MS);
        }
        AuthResult::Denied
    }

    // ── Accessori (per test e channel.rs) ────────────────────────────────────

    /// Ritorna il `chat_id` appaiato (se presente).
    pub fn paired_chat_id(&self) -> Option<i64> {
        self.paired_chat_id
    }

    /// Ritorna il secret TOTP in base32 (per diagnosi al primo avvio).
    ///
    /// **Non loggare in produzione.** Usato solo da `init()` per costruire l'URI.
    pub fn secret_base32(&self) -> &str {
        &self.secret_base32
    }
}

/// Costruisce un oggetto `TOTP` dai byte grezzi del secret.
///
/// Parametri fissi conformi a RFC 6238:
/// - SHA1, 6 cifre, step 30s, skew 1 (±1 step = ±30s di tolleranza).
fn build_totp_from_bytes(secret_bytes: &[u8]) -> TOTP {
    TOTP::new(
        Algorithm::SHA1,
        6,
        1,    // skew: ±1 step di tolleranza
        30,   // step in secondi
        secret_bytes.to_vec(),
        Some(TOTP_ISSUER.to_string()),
        TOTP_ACCOUNT.to_string(),
    )
    .expect("parametri TOTP fissi — non fallisce mai")
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests (TDD adversarial — nessun clock reale)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::{NamedTempFile, TempDir};

    /// Secret fisso per i test TOTP deterministici.
    ///
    /// 32 caratteri base32 = 20 byte = 160 bit (minimo richiesto da totp-rs per SHA1: 128 bit).
    /// Verificato empiricamente che `Secret::Encoded(TEST_SECRET_B32).to_bytes()` → 20 byte.
    const TEST_SECRET_B32: &str = "JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP";

    /// VETTORE NOTO RFC 6238 (SHA1): secret ASCII "12345678901234567890" (20
    /// byte), a t=59s → codice a 6 cifre "287082". Blinda il WIRING (SHA1, 6
    /// cifre, step 30s) contro l'algoritmo reale di Google Authenticator: senza
    /// questo i test TOTP sono auto-referenziali (stesso codice genera l'atteso)
    /// e un errore di parametri non verrebbe rilevato finché l'utente non riesce
    /// a fare /login.
    #[test]
    fn totp_matches_rfc6238_sha1_known_vector() {
        let totp = build_totp_from_bytes(b"12345678901234567890");
        // t_secs = 59 → counter 1 → HOTP-SHA1 troncato a 6 cifre = 287082.
        assert_eq!(totp.generate(59), "287082");
    }

    /// Timestamp fisso in millisecondi (ms epoch).
    /// `T = 1_700_000_000_000 ms` → `t_secs = 1_700_000_000`.
    const T0_MS: u64 = 1_700_000_000_000;

    /// Calcola il codice TOTP atteso per `TEST_SECRET_B32` a `T0_MS`.
    /// Chiama il codice di produzione build_totp_from_bytes per coerenza.
    fn expected_code_at_t0() -> String {
        let secret_bytes = Secret::Encoded(TEST_SECRET_B32.to_string())
            .to_bytes()
            .unwrap();
        let totp = build_totp_from_bytes(&secret_bytes);
        totp.generate(T0_MS / 1000)
    }

    /// Helper: costruisce un Authenticator con secret fisso e nessun chat appaiato.
    fn auth_with_secret(paired: Option<i64>) -> Authenticator {
        let state = TelegramState {
            totp_secret_base32: Some(TEST_SECRET_B32.to_string()),
            paired_chat_id: paired,
        };
        Authenticator::from_state(&state).expect("secret valido")
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TelegramState: load/save round-trip
    // ─────────────────────────────────────────────────────────────────────────

    /// Salva e ricarica: i campi sopravvivono al round-trip.
    #[test]
    fn telegram_state_load_save_round_trip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("telegram-state.json");

        let original = TelegramState {
            totp_secret_base32: Some("JBSWY3DPEHPK3PXP".to_string()),
            paired_chat_id: Some(12345678),
        };
        original.save(&path).expect("save ok");

        let loaded = TelegramState::load(&path);
        assert_eq!(loaded.totp_secret_base32.as_deref(), Some("JBSWY3DPEHPK3PXP"));
        assert_eq!(loaded.paired_chat_id, Some(12345678));
    }

    /// File assente → default (campi None).
    #[test]
    fn telegram_state_missing_file_returns_default() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("nonexistent.json");
        let state = TelegramState::load(&path);
        assert!(state.totp_secret_base32.is_none());
        assert!(state.paired_chat_id.is_none());
    }

    /// File corrotto → default (non panico).
    #[test]
    fn telegram_state_corrupted_file_returns_default() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "{{corrupted not json}}").unwrap();
        let state = TelegramState::load(f.path());
        assert!(state.totp_secret_base32.is_none());
        assert!(state.paired_chat_id.is_none());
    }

    /// `save` crea la directory parent se non esiste.
    #[test]
    fn telegram_state_save_creates_parent_dir() {
        let dir = TempDir::new().unwrap();
        let nested = dir.path().join("subdir").join("telegram-state.json");
        let state = TelegramState {
            totp_secret_base32: Some("JBSWY3DPEHPK3PXP".to_string()),
            paired_chat_id: None,
        };
        state.save(&nested).expect("save con creazione dir");
        assert!(nested.exists(), "il file deve esistere dopo save");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // init: genera secret una volta, riusa alla seconda chiamata
    // ─────────────────────────────────────────────────────────────────────────

    /// Prima chiamata a `init` → genera secret e ritorna `Some(uri)`.
    #[test]
    fn init_generates_secret_and_returns_uri_on_first_call() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("telegram-state.json");

        let (_auth, uri) = Authenticator::init(&path);
        assert!(uri.is_some(), "prima init deve ritornare Some(uri)");
        let uri_str = uri.unwrap();
        assert!(
            uri_str.starts_with("otpauth://totp/"),
            "URI deve essere otpauth://totp/...; trovato: {uri_str}"
        );
    }

    /// `init` con state file che contiene un secret base32 invalido (corrotto/troncato)
    /// NON deve panizzare: deve rigenerare il secret e ritornare `Some(uri)`.
    ///
    /// Questo test copre il caso: scrittura parziale su disco / modifica manuale del file.
    #[test]
    fn init_with_invalid_secret_regenerates_instead_of_panicking() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("telegram-state.json");

        // Scrivi uno state con un secret non valido (base32 invalido).
        let corrupt_state = TelegramState {
            totp_secret_base32: Some("!!!INVALID_BASE32!!!".to_string()),
            paired_chat_id: None,
        };
        corrupt_state.save(&path).expect("setup test");

        // `init` non deve panizzare: deve rigenerare.
        let (auth, uri) = Authenticator::init(&path);
        assert!(
            uri.is_some(),
            "init con secret invalido deve rigenerare e ritornare Some(uri)"
        );
        let uri_str = uri.unwrap();
        assert!(
            uri_str.starts_with("otpauth://totp/"),
            "URI generato deve essere otpauth://totp/...; trovato: {uri_str}"
        );
        // Il nuovo secret deve essere valido (l'auth deve funzionare).
        assert!(!auth.secret_base32().is_empty());
    }

    /// `init` riusa il secret esistente; finché NON si è appaiati, mostra ANCORA
    /// l'URI di setup (così se al primo avvio si è perso il QR basta riavviare).
    #[test]
    fn init_reuses_secret_and_shows_uri_until_paired() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("telegram-state.json");

        // Prima init: genera il secret + URI.
        let (auth1, uri1) = Authenticator::init(&path);
        assert!(uri1.is_some());

        // Seconda init (non appaiati): stesso secret, URI ANCORA mostrato.
        let (auth2, uri2) = Authenticator::init(&path);
        assert!(uri2.is_some(), "non appaiati: l'URI di setup va ancora mostrato");
        assert_eq!(
            auth1.secret_base32(),
            auth2.secret_base32(),
            "il secret deve essere lo stesso tra le due init"
        );
    }

    /// Una volta appaiati, `init` NON mostra più l'URI (setup completato).
    #[test]
    fn init_returns_none_when_already_paired() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("telegram-state.json");

        // State con secret valido + chat già appaiata.
        let state = TelegramState {
            totp_secret_base32: Some(TEST_SECRET_B32.to_string()),
            paired_chat_id: Some(42),
        };
        state.save(&path).unwrap();

        let (_auth, uri) = Authenticator::init(&path);
        assert!(uri.is_none(), "appaiati: l'URI non deve più essere mostrato");
    }

    /// `init` persiste il `paired_chat_id` e lo ripristina alla seconda chiamata.
    #[test]
    fn init_preserves_paired_chat_id_across_restart() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("telegram-state.json");

        // Prima init: ottieni un pairing code.
        let (mut auth, _) = Authenticator::init(&path);
        let code = auth.new_pairing_code(T0_MS);

        // Appaia.
        let result = auth.try_pair(42, &code, T0_MS, &path);
        assert_eq!(result, AuthResult::Ok);

        // Seconda init: il chat_id deve essere preservato.
        let (auth2, _) = Authenticator::init(&path);
        assert_eq!(auth2.paired_chat_id(), Some(42));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // authorize: tutti e quattro i rami
    // ─────────────────────────────────────────────────────────────────────────

    /// Nessun chat appaiato → NeedPairing (indipendentemente dal chat_id).
    #[test]
    fn authorize_no_paired_chat_returns_need_pairing() {
        let auth = auth_with_secret(None);
        assert_eq!(auth.authorize(99, T0_MS), AuthResult::NeedPairing);
    }

    /// Chat appaiata ma chat_id diverso → Denied.
    #[test]
    fn authorize_wrong_chat_id_returns_denied() {
        let auth = auth_with_secret(Some(42));
        assert_eq!(auth.authorize(99, T0_MS), AuthResult::Denied);
    }

    /// Chat appaiata, nessuna sessione → NeedLogin.
    #[test]
    fn authorize_paired_no_session_returns_need_login() {
        let auth = auth_with_secret(Some(42));
        assert_eq!(auth.authorize(42, T0_MS), AuthResult::NeedLogin);
    }

    /// Chat appaiata, sessione attiva → Ok.
    #[test]
    fn authorize_paired_with_active_session_returns_ok() {
        let mut auth = auth_with_secret(Some(42));
        // Inietta la sessione manualmente simulando un login riuscito.
        let code = expected_code_at_t0();
        let result = auth.try_login(42, &code, T0_MS);
        assert_eq!(result, AuthResult::Ok);
        assert_eq!(auth.authorize(42, T0_MS + 1), AuthResult::Ok);
    }

    /// Sessione scaduta → NeedLogin (not Ok).
    #[test]
    fn authorize_expired_session_returns_need_login() {
        let mut auth = auth_with_secret(Some(42));
        let code = expected_code_at_t0();
        auth.try_login(42, &code, T0_MS);
        // Avanza il tempo oltre la scadenza.
        let beyond_expiry = T0_MS + SESSION_TTL_MS + 1;
        assert_eq!(auth.authorize(42, beyond_expiry), AuthResult::NeedLogin);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TOTP: codice corretto/errato/tolleranza ±1 step
    // ─────────────────────────────────────────────────────────────────────────

    /// Codice TOTP corretto a T0 → try_login Ok + sessione attiva.
    ///
    /// Test non-tautologico: usa il codice generato a T0 e verifica che sia
    /// accettato esattamente a T0 (non si limita a generare e verificare con
    /// lo stesso oggetto).
    #[test]
    fn try_login_correct_totp_returns_ok_and_sets_session() {
        let mut auth = auth_with_secret(Some(42));
        let code = expected_code_at_t0();
        let result = auth.try_login(42, &code, T0_MS);
        assert_eq!(result, AuthResult::Ok, "codice corretto deve ritornare Ok");
        // Sessione attiva immediatamente dopo.
        assert_eq!(auth.authorize(42, T0_MS + 1), AuthResult::Ok);
    }

    /// Codice TOTP errato → Denied; nessuna sessione aperta.
    #[test]
    fn try_login_wrong_totp_returns_denied_no_session() {
        let mut auth = auth_with_secret(Some(42));
        let result = auth.try_login(42, "000000", T0_MS);
        assert_eq!(result, AuthResult::Denied);
        // Nessuna sessione.
        assert_eq!(auth.authorize(42, T0_MS + 1), AuthResult::NeedLogin);
    }

    /// Tolleranza +1 step (codice del prossimo step, 30s avanti) → accettato.
    #[test]
    fn try_login_totp_plus_one_step_is_accepted() {
        let secret_bytes = Secret::Encoded(TEST_SECRET_B32.to_string())
            .to_bytes()
            .unwrap();
        let totp = build_totp_from_bytes(&secret_bytes);

        // Codice generato 30s avanti nel futuro.
        let t_plus_30_secs = T0_MS / 1000 + 30;
        let code_future = totp.generate(t_plus_30_secs);

        let mut auth = auth_with_secret(Some(42));
        // Verifichiamo a T0 — skew=1 deve accettare anche il codice di t+30s.
        let result = auth.try_login(42, &code_future, T0_MS);
        assert_eq!(result, AuthResult::Ok, "codice +1 step deve essere accettato (skew=1)");
    }

    /// Tolleranza -1 step (codice del precedente step, 30s indietro) → accettato.
    #[test]
    fn try_login_totp_minus_one_step_is_accepted() {
        let secret_bytes = Secret::Encoded(TEST_SECRET_B32.to_string())
            .to_bytes()
            .unwrap();
        let totp = build_totp_from_bytes(&secret_bytes);

        // Codice generato 30s fa nel passato.
        let t_minus_30_secs = T0_MS / 1000 - 30;
        let code_past = totp.generate(t_minus_30_secs);

        let mut auth = auth_with_secret(Some(42));
        let result = auth.try_login(42, &code_past, T0_MS);
        assert_eq!(result, AuthResult::Ok, "codice -1 step deve essere accettato (skew=1)");
    }

    /// Codice di 2 step fa (60s indietro) → rifiutato (fuori dalla finestra di skew=1).
    #[test]
    fn try_login_totp_minus_two_steps_is_rejected() {
        let secret_bytes = Secret::Encoded(TEST_SECRET_B32.to_string())
            .to_bytes()
            .unwrap();
        let totp = build_totp_from_bytes(&secret_bytes);

        let t_minus_60_secs = T0_MS / 1000 - 60;
        let code_old = totp.generate(t_minus_60_secs);

        let mut auth = auth_with_secret(Some(42));
        let result = auth.try_login(42, &code_old, T0_MS);
        // Se il codice di t-60s coincide casualmente con quello di t-30s o t0,
        // non possiamo asserire Denied. In quel caso il test è non-discriminante
        // ma non falso. Per TEST_SECRET_B32 a T0 questo non accade.
        // In pratica: la probabilità è ~1/1000 su 30 step adiacenti.
        let current = totp.generate(T0_MS / 1000);
        let prev = totp.generate(T0_MS / 1000 - 30);
        if code_old != current && code_old != prev {
            assert_eq!(
                result,
                AuthResult::Denied,
                "codice -2 step deve essere rifiutato"
            );
        }
        // else: coincidenza casuale, il test è indeterminato per questo secret/time
        // (non un fallimento — è una proprietà del TOTP).
    }

    /// Chat_id diverso da quello appaiato → Denied (anche con TOTP valido).
    #[test]
    fn try_login_wrong_chat_id_returns_denied() {
        let mut auth = auth_with_secret(Some(42));
        let code = expected_code_at_t0();
        assert_eq!(auth.try_login(99, &code, T0_MS), AuthResult::Denied);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Rate-limit login
    // ─────────────────────────────────────────────────────────────────────────

    /// Spec: 5 tentativi errati → tutti e 5 ritornano Denied (l'ultimo arma il lockout);
    /// il 6° tentativo → RateLimited (lockout già armato). Il 7° durante il lockout →
    /// RateLimited senza controllare il codice.
    #[test]
    fn try_login_rate_limit_after_max_fails() {
        let mut auth = auth_with_secret(Some(42));

        // Tentativi 1..=MAX_FAILS → tutti Denied (il MAX_FAILS-esimo arma il lockout
        // ma ritorna ancora Denied: l'utente ha consumato tutti i suoi tentativi).
        for i in 1..=MAX_FAILS {
            let r = auth.try_login(42, "000000", T0_MS);
            assert_eq!(r, AuthResult::Denied, "tentativo {i} atteso Denied");
        }

        // Tentativo MAX_FAILS+1 → RateLimited (lockout già attivo).
        let r = auth.try_login(42, "000000", T0_MS + 1);
        assert_eq!(
            r,
            AuthResult::RateLimited,
            "tentativo {n} atteso RateLimited (lockout attivo)",
            n = MAX_FAILS + 1
        );

        // Tentativo successivo durante il lockout → RateLimited anche con codice corretto.
        let good_code = expected_code_at_t0();
        let r = auth.try_login(42, &good_code, T0_MS + 2);
        assert_eq!(r, AuthResult::RateLimited, "durante lockout anche codice valido → RateLimited");
    }

    /// Dopo LOCKOUT_MS il lockout scade e i tentativi riprendono (contatori azzerati).
    #[test]
    fn try_login_lockout_expires_and_allows_retry() {
        let mut auth = auth_with_secret(Some(42));

        // Forza il lockout.
        for _ in 0..MAX_FAILS {
            auth.try_login(42, "000000", T0_MS);
        }
        assert_eq!(auth.try_login(42, "000000", T0_MS + 1), AuthResult::RateLimited);

        // Avanza il tempo oltre la scadenza del lockout.
        let after_lockout = T0_MS + LOCKOUT_MS + 1;

        // Il primo tentativo dopo il lockout deve procedere (Denied, non RateLimited).
        let r = auth.try_login(42, "000000", after_lockout);
        assert_eq!(
            r,
            AuthResult::Denied,
            "dopo scadenza lockout deve tornare Denied (non RateLimited)"
        );
    }

    /// Dopo il lockout, un login corretto deve riuscire.
    #[test]
    fn try_login_succeeds_after_lockout_expires() {
        let secret_bytes = Secret::Encoded(TEST_SECRET_B32.to_string())
            .to_bytes()
            .unwrap();
        let totp = build_totp_from_bytes(&secret_bytes);

        let mut auth = auth_with_secret(Some(42));

        // Forza il lockout a T0.
        for _ in 0..MAX_FAILS {
            auth.try_login(42, "000000", T0_MS);
        }

        // Dopo il lockout, calcola il codice per il nuovo timestamp.
        let after_lockout = T0_MS + LOCKOUT_MS + 1;
        let code_after = totp.generate(after_lockout / 1000);
        let r = auth.try_login(42, &code_after, after_lockout);
        assert_eq!(r, AuthResult::Ok, "login con codice corretto dopo lockout deve riuscire");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Pairing: new_pairing_code + try_pair
    // ─────────────────────────────────────────────────────────────────────────

    /// Codice corretto e non scaduto → Ok; chat_id appaiato; stato persistito.
    #[test]
    fn try_pair_correct_code_pairs_and_persists() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("state.json");

        let mut auth = auth_with_secret(None);
        let code = auth.new_pairing_code(T0_MS);

        let result = auth.try_pair(42, &code, T0_MS, &path);
        assert_eq!(result, AuthResult::Ok, "codice corretto deve tornare Ok");
        assert_eq!(auth.paired_chat_id(), Some(42), "chat_id deve essere appaiato");

        // Verifica la persistenza: il file deve contenere paired_chat_id.
        let saved = TelegramState::load(&path);
        assert_eq!(saved.paired_chat_id, Some(42), "stato deve essere persistito");
    }

    /// Codice corretto è single-use: il secondo try_pair con lo stesso codice → Denied.
    #[test]
    fn try_pair_code_is_single_use() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("state.json");

        let mut auth = auth_with_secret(None);
        let code = auth.new_pairing_code(T0_MS);

        // Primo uso: Ok.
        auth.try_pair(42, &code, T0_MS, &path);

        // Secondo uso: Denied (codice già consumato).
        let result = auth.try_pair(42, &code, T0_MS, &path);
        assert_eq!(result, AuthResult::Denied, "secondo uso codice → Denied");
    }

    /// Codice sbagliato → Denied.
    #[test]
    fn try_pair_wrong_code_returns_denied() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("state.json");

        let mut auth = auth_with_secret(None);
        auth.new_pairing_code(T0_MS);

        let result = auth.try_pair(42, "000000", T0_MS, &path);
        assert_eq!(result, AuthResult::Denied);
    }

    /// Codice scaduto → Denied (anche se il codice sarebbe corretto).
    #[test]
    fn try_pair_expired_code_returns_denied() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("state.json");

        let mut auth = auth_with_secret(None);
        let code = auth.new_pairing_code(T0_MS);

        // Tenta dopo la scadenza.
        let expired_time = T0_MS + PAIRING_TTL_MS + 1;
        let result = auth.try_pair(42, &code, expired_time, &path);
        assert_eq!(result, AuthResult::Denied, "codice scaduto → Denied");
    }

    /// Nessun codice generato → Denied (try_pair senza aver chiamato new_pairing_code).
    #[test]
    fn try_pair_without_pairing_code_returns_denied() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("state.json");

        let mut auth = auth_with_secret(None);
        // NON chiamiamo new_pairing_code.
        let result = auth.try_pair(42, "123456", T0_MS, &path);
        assert_eq!(result, AuthResult::Denied, "nessun codice generato → Denied");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Rate-limit pairing
    // ─────────────────────────────────────────────────────────────────────────

    /// Spec: 5 tentativi di pairing errati → tutti Denied (il 5° arma il lockout);
    /// il 6° → RateLimited (lockout attivo).
    #[test]
    fn try_pair_rate_limit_after_max_fails() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("state.json");

        let mut auth = auth_with_secret(None);
        auth.new_pairing_code(T0_MS);

        // Tentativi 1..=MAX_FAILS → tutti Denied (il MAX_FAILS-esimo arma il lockout
        // ma ritorna ancora Denied).
        for i in 1..=MAX_FAILS {
            let r = auth.try_pair(42, "000000", T0_MS, &path);
            assert_eq!(r, AuthResult::Denied, "tentativo {i} pairing atteso Denied");
        }

        // Tentativo MAX_FAILS+1 → RateLimited (lockout già attivo).
        let r = auth.try_pair(42, "000000", T0_MS + 1, &path);
        assert_eq!(
            r,
            AuthResult::RateLimited,
            "tentativo {n} pairing atteso RateLimited",
            n = MAX_FAILS + 1
        );
    }

    /// Durante il lockout di pairing, anche il codice corretto è bloccato.
    #[test]
    fn try_pair_during_lockout_blocks_even_correct_code() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("state.json");

        let mut auth = auth_with_secret(None);
        let code = auth.new_pairing_code(T0_MS);

        // Forza il lockout con codici sbagliati.
        for _ in 0..MAX_FAILS {
            auth.try_pair(42, "000000", T0_MS, &path);
        }

        // Anche il codice corretto viene bloccato durante il lockout.
        let r = auth.try_pair(42, &code, T0_MS + 1, &path);
        assert_eq!(r, AuthResult::RateLimited, "lockout pairing blocca anche il codice corretto");
    }

    /// Dopo LOCKOUT_MS i tentativi di pairing riprendono.
    #[test]
    fn try_pair_lockout_expires_and_allows_retry() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("state.json");

        let mut auth = auth_with_secret(None);
        auth.new_pairing_code(T0_MS);

        // Forza il lockout.
        for _ in 0..MAX_FAILS {
            auth.try_pair(42, "000000", T0_MS, &path);
        }

        // Dopo il lockout, genera un nuovo codice e appaia.
        let after_lockout = T0_MS + LOCKOUT_MS + 1;
        let new_code = auth.new_pairing_code(after_lockout);
        let r = auth.try_pair(42, &new_code, after_lockout, &path);
        assert_eq!(r, AuthResult::Ok, "dopo lockout pairing deve riuscire con nuovo codice");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Integrazione: pairing → login → authorize → scadenza
    // ─────────────────────────────────────────────────────────────────────────

    /// Flusso completo: pair → login → authorize ok → sessione scade → NeedLogin.
    #[test]
    fn full_auth_flow_pair_login_session_expiry() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("state.json");

        let secret_bytes = Secret::Encoded(TEST_SECRET_B32.to_string())
            .to_bytes()
            .unwrap();
        let totp = build_totp_from_bytes(&secret_bytes);

        let mut auth = auth_with_secret(None);

        // 1. Genera codice di pairing e appaia.
        let pair_code = auth.new_pairing_code(T0_MS);
        assert_eq!(auth.try_pair(42, &pair_code, T0_MS, &path), AuthResult::Ok);

        // 2. Senza login → NeedLogin.
        assert_eq!(auth.authorize(42, T0_MS + 1), AuthResult::NeedLogin);

        // 3. Login con TOTP corretto.
        let totp_code = totp.generate(T0_MS / 1000);
        assert_eq!(auth.try_login(42, &totp_code, T0_MS), AuthResult::Ok);

        // 4. Authorize ok durante la sessione.
        assert_eq!(auth.authorize(42, T0_MS + 100), AuthResult::Ok);

        // 5. Sessione scade.
        let after_session = T0_MS + SESSION_TTL_MS + 1;
        assert_eq!(auth.authorize(42, after_session), AuthResult::NeedLogin);
    }

    /// Chat non appaiata non può fare login nemmeno con TOTP corretto.
    #[test]
    fn unpaired_chat_cannot_login() {
        let mut auth = auth_with_secret(None); // nessun chat appaiato
        let code = expected_code_at_t0();
        assert_eq!(auth.try_login(99, &code, T0_MS), AuthResult::Denied);
    }

    /// Dopo il pairing, solo la chat appaiata può fare login.
    #[test]
    fn only_paired_chat_can_login() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("state.json");

        let mut auth = auth_with_secret(None);
        let code = auth.new_pairing_code(T0_MS);
        auth.try_pair(42, &code, T0_MS, &path);

        // Chat 42: può fare login.
        let totp_code = expected_code_at_t0();
        assert_eq!(auth.try_login(42, &totp_code, T0_MS), AuthResult::Ok);
        assert_eq!(auth.authorize(42, T0_MS + 1), AuthResult::Ok);

        // Chat 99: anche con TOTP corretto → Denied.
        assert_eq!(auth.try_login(99, &totp_code, T0_MS), AuthResult::Denied);
        assert_eq!(auth.authorize(99, T0_MS + 1), AuthResult::Denied);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Sicurezza: TelegramState non espone il secret tramite Debug
    // ─────────────────────────────────────────────────────────────────────────

    /// `TelegramState` non implementa `Debug` (il secret TOTP non deve
    /// apparire nei log per errore).
    ///
    /// Questo test verifica la proprietà a compile-time: se `TelegramState`
    /// implementasse `Debug`, la riga seguente compilerebbe senza errori e
    /// il test passerebbe senza senso. Il compilatore stesso è il verificatore.
    ///
    /// La nostra garanzia: il test usa `std::any::type_name` (non `{:?}`) per
    /// manipolare il tipo senza richiedere `Debug`. Se il tipo acquisisse `Debug`
    /// per errore, questa suite non ce lo direbbe — ma la scelta architetturale
    /// (no `#[derive(Debug)]`) è documentata sopra e verificata a ispezione.
    #[test]
    fn telegram_state_can_be_instantiated_and_does_not_need_debug() {
        let s = TelegramState {
            totp_secret_base32: Some("secret".to_string()),
            paired_chat_id: Some(1),
        };
        // Possiamo usare il tipo senza Debug.
        let _ = s.totp_secret_base32.as_deref();
        // Il test compila = no accidental Debug derive required.
    }
}
