//! Stato del plugin + dispatch puro di `HostToPlugin` (2026-07-19,
//! Docs/superpowers/specs/2026-07-19-crypto-plugin-design.md §7-§8).
//! `main.rs` resta uno shell I/O sottile — tutta la logica vive qui,
//! testabile senza stdin/stdout reali (stesso pattern di `plugin-lc`).

use crate::ciphers::{all_ciphers, Cipher, ParamValues};
use crate::normalize::normalize;
use plugin_protocol::{HostToPlugin, PluginToHost};
use std::collections::HashMap;

/// Offset usato per inventare il `window_id` della dialog parametri —
/// namespace separato dal contatore dell'host (che assegna window_id
/// piccoli, sequenziali, uno per Activate) per non collidere mai. Vedi
/// il fix orchestrator (Task 1 di questo piano) che rende funzionante
/// una seconda ShowWindow con un window_id inventato dal plugin.
const DIALOG_WINDOW_ID_OFFSET: u64 = 1_000_000;

pub struct CryptoState {
    /// `window_id` della finestra principale, assegnato da `Activate`.
    /// `None` prima della prima `Activate`.
    pub(crate) main_window_id: Option<u64>,
    /// Indice del cifrario correntemente selezionato nella sidebar, in
    /// `all_ciphers()`.
    pub(crate) current_cipher_index: usize,
    /// Ultimo testo digitato nel pannello "chiaro" e nel pannello
    /// "cifrato" — mantenuti entrambi per il render (§7: pannelli fissi).
    pub(crate) plaintext: String,
    pub(crate) ciphertext: String,
    /// Quale pannello è stato modificato per ULTIMO — usato da "Applica"
    /// per sapere quale dei due ri-cifrare (§8).
    last_edited: LastEdited,
    /// Parametri correnti PER CIFRARIO (chiave = `Cipher::id()`) — cambiare
    /// cifrario nella sidebar non perde i parametri già impostati per gli
    /// altri.
    pub(crate) params_by_cipher: HashMap<String, ParamValues>,
    /// Errore corrente da mostrare al posto del pannello di destinazione
    /// (§9) — `None` quando l'ultima encode/decode è riuscita.
    pub(crate) last_error: Option<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum LastEdited {
    Plaintext,
    Ciphertext,
}

impl Default for CryptoState {
    fn default() -> Self {
        Self {
            main_window_id: None,
            current_cipher_index: 0,
            plaintext: String::new(),
            ciphertext: String::new(),
            last_edited: LastEdited::Plaintext,
            params_by_cipher: HashMap::new(),
            last_error: None,
        }
    }
}

impl CryptoState {
    pub(crate) fn current_cipher(&self) -> Box<dyn Cipher> {
        // Ritorna un `Box<dyn Cipher>` OWNED, non un riferimento: il `Vec`
        // di `all_ciphers_static()` è locale a questa chiamata e verrebbe
        // distrutto a fine funzione, quindi un riferimento nel suo interno
        // sarebbe dangling (E0515). I cifrari sono unit struct (zero-cost),
        // quindi "ricrearne uno" ad ogni chiamata non alloca nulla di
        // rilevante — è solo una `Box` sull'unit struct già esistente.
        // `all_ciphers()` non è mai vuota (3 cifrari statici) e l'indice è
        // sempre in range: lo tiene tale il match su "cipher-select:*" in
        // `handle_ui_event`, che aggiorna `current_cipher_index` solo se
        // trova una posizione valida via `.position()`.
        all_ciphers_static()
            .into_iter()
            .nth(self.current_cipher_index)
            .expect("current_cipher_index è sempre in range")
    }

    pub(crate) fn current_params(&self) -> ParamValues {
        self.params_by_cipher
            .get(self.current_cipher().id())
            .cloned()
            .unwrap_or_default()
    }

    fn dialog_window_id(&self) -> Option<u64> {
        self.main_window_id.map(|w| w + DIALOG_WINDOW_ID_OFFSET)
    }

    /// Ricifra/decifra in base a `last_edited`, aggiorna l'altro pannello
    /// o `last_error`.
    fn resync(&mut self) {
        let cipher = self.current_cipher();
        let params = self.current_params();
        self.last_error = None;
        match self.last_edited {
            LastEdited::Plaintext => {
                let normalized = normalize(&self.plaintext);
                match cipher.encode(&normalized, &params) {
                    Ok(result) => self.ciphertext = result,
                    Err(e) => self.last_error = Some(e),
                }
            }
            LastEdited::Ciphertext => match cipher.decode(&self.ciphertext, &params) {
                Ok(result) => self.plaintext = result,
                Err(e) => self.last_error = Some(e),
            },
        }
    }
}

/// Wrapper stabile su `all_ciphers()` — usato invece di chiamarlo ogni
/// volta per chiarezza di lettura (`Vec<Box<dyn Cipher>>` non è pesante da
/// ricostruire, ma un nome dedicato rende `current_cipher` più leggibile).
fn all_ciphers_static() -> Vec<Box<dyn Cipher>> {
    all_ciphers()
}

/// Dispatch puro: un `HostToPlugin` in ingresso, zero o più `PluginToHost`
/// in uscita. Nessun I/O — testabile senza stdin/stdout reali.
pub fn handle(state: &mut CryptoState, msg: HostToPlugin) -> Vec<PluginToHost> {
    match msg {
        HostToPlugin::Init { .. } => {
            vec![PluginToHost::Ready { name: "crypto".into(), protocol_version: 1 }]
        }

        HostToPlugin::Activate { window_id, .. } => {
            state.main_window_id = Some(window_id);
            vec![PluginToHost::ShowWindow {
                window_id,
                title: "Crittografia".into(),
                html: crate::render::render_main_window(state),
            }]
        }

        HostToPlugin::UiEvent { window_id, element_id, value } => {
            handle_ui_event(state, window_id, &element_id, value)
        }

        HostToPlugin::Deinit {} => vec![],
    }
}

fn handle_ui_event(
    state: &mut CryptoState,
    window_id: u64,
    element_id: &str,
    value: Option<String>,
) -> Vec<PluginToHost> {
    // Il nome del cifrario è incorporato nell'element_id stesso
    // (`cipher-select:caesar`, ecc.), non passato via `value` — un
    // `<div>` di sidebar non è "value-bearing" per il runtime host
    // (solo INPUT/SELECT/TEXTAREA lo sono, vedi Task 1b), quindi un
    // click su di esso arriva sempre con `value: None`. Stesso motivo
    // per cui i campi parametro (sotto) si aggiornano ad ogni tasto
    // invece che tutti insieme al click su "Applica": un pulsante
    // (`<span>`) non porta mai un valore, solo i suoi INPUT lo fanno.
    if let Some(cipher_id) = element_id.strip_prefix("cipher-select:") {
        if let Some(idx) = all_ciphers_static().iter().position(|c| c.id() == cipher_id) {
            state.current_cipher_index = idx;
            state.last_error = None;
        }
        return main_window_update(state);
    }

    // Campo parametro (`<input data-evt="param:shift">`, ecc.) — INPUT
    // è value-bearing, quindi questo arriva con `value: Some(...)` ad
    // ogni tasto (evento "input" del browser, non solo al click).
    // Aggiorna subito `params_by_cipher` per il cifrario corrente — non
    // aspetta "Applica", che diventa un puro trigger di ri-cifratura
    // sui valori GIÀ salvati qui.
    if let Some(param_key) = element_id.strip_prefix("param:") {
        let cipher_id = state.current_cipher().id().to_string();
        state
            .params_by_cipher
            .entry(cipher_id)
            .or_default()
            .insert(param_key.to_string(), value.unwrap_or_default());
        return vec![]; // nessun re-render finché non premi "Applica"
    }

    match element_id {
        "plaintext" => {
            state.plaintext = value.unwrap_or_default();
            state.last_edited = LastEdited::Plaintext;
            state.resync();
            main_window_update(state)
        }

        "ciphertext" => {
            state.ciphertext = value.unwrap_or_default();
            state.last_edited = LastEdited::Ciphertext;
            state.resync();
            main_window_update(state)
        }

        "open-params" => {
            let Some(dialog_id) = state.dialog_window_id() else { return vec![] };
            vec![PluginToHost::ShowWindow {
                window_id: dialog_id,
                title: format!("Parametri — {}", state.current_cipher().display_name()),
                html: crate::render::render_params_dialog(state),
            }]
        }

        "params-close" => {
            vec![PluginToHost::CloseWindow { window_id }]
        }

        "params-generate" => {
            let cipher_id = state.current_cipher().id().to_string();
            let params = state.current_params();
            match state.current_cipher().generate(&params) {
                Ok(generated) => {
                    state
                        .params_by_cipher
                        .entry(cipher_id)
                        .or_default()
                        .extend(generated);
                }
                Err(e) => state.last_error = Some(e),
            }
            vec![PluginToHost::UpdateWindow { window_id, html: crate::render::render_params_dialog(state) }]
        }

        "params-apply" => {
            // Nessun valore da questo evento (pulsante, non value-bearing)
            // — ri-cifra semplicemente con `params_by_cipher` così com'è,
            // già tenuto aggiornato dai singoli campi "param:*" sopra.
            state.resync();
            let mut out = main_window_update(state);
            out.push(PluginToHost::UpdateWindow { window_id, html: crate::render::render_params_dialog(state) });
            out
        }

        _ => vec![],
    }
}

fn main_window_update(state: &CryptoState) -> Vec<PluginToHost> {
    let Some(window_id) = state.main_window_id else { return vec![] };
    vec![PluginToHost::UpdateWindow { window_id, html: crate::render::render_main_window(state) }]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn activate(state: &mut CryptoState, window_id: u64) {
        handle(state, HostToPlugin::Activate { window_id, args: serde_json::Value::Null });
    }

    fn ui_event(state: &mut CryptoState, window_id: u64, element_id: &str, value: Option<&str>) -> Vec<PluginToHost> {
        handle(state, HostToPlugin::UiEvent {
            window_id,
            element_id: element_id.to_string(),
            value: value.map(String::from),
        })
    }

    #[test]
    fn init_replies_ready() {
        let mut state = CryptoState::default();
        let replies = handle(&mut state, HostToPlugin::Init {
            protocol_version: 1,
            config: serde_json::Value::Null,
            storage_dir: String::new(),
        });
        assert!(matches!(replies[0], PluginToHost::Ready { .. }));
    }

    #[test]
    fn activate_shows_main_window_and_remembers_its_id() {
        let mut state = CryptoState::default();
        let replies = handle(&mut state, HostToPlugin::Activate { window_id: 7, args: serde_json::Value::Null });
        assert!(matches!(replies[0], PluginToHost::ShowWindow { window_id: 7, .. }));
        assert_eq!(state.main_window_id, Some(7));
    }

    #[test]
    fn typing_plaintext_encodes_into_ciphertext() {
        let mut state = CryptoState::default();
        activate(&mut state, 1);
        // Cesare default (shift 3, dal Task 3): "ABC" → "DEF".
        ui_event(&mut state, 1, "plaintext", Some("ABC"));
        assert_eq!(state.ciphertext, "DEF");
    }

    #[test]
    fn typing_ciphertext_decodes_into_plaintext() {
        let mut state = CryptoState::default();
        activate(&mut state, 1);
        ui_event(&mut state, 1, "ciphertext", Some("DEF"));
        assert_eq!(state.plaintext, "ABC");
    }

    #[test]
    fn switching_cipher_does_not_touch_existing_text() {
        let mut state = CryptoState::default();
        activate(&mut state, 1);
        ui_event(&mut state, 1, "plaintext", Some("ABC"));
        let before = state.plaintext.clone();
        ui_event(&mut state, 1, "cipher-select:vigenere", None);
        assert_eq!(state.plaintext, before);
    }

    #[test]
    fn cipher_select_id_is_read_from_element_id_not_value() {
        // Il nome del cifrario è nell'element_id (`cipher-select:rsa`), MAI
        // in `value` — un <div> di sidebar non è value-bearing lato host
        // (solo INPUT/SELECT/TEXTAREA lo sono), quindi `value` è sempre
        // `None` per questo evento nella realtà. Verifica che il dispatch
        // non dipenda da `value` per niente.
        let mut state = CryptoState::default();
        activate(&mut state, 1);
        ui_event(&mut state, 1, "cipher-select:rsa", None);
        assert_eq!(state.current_cipher().id(), "rsa");
    }

    #[test]
    fn open_params_uses_offset_window_id() {
        let mut state = CryptoState::default();
        activate(&mut state, 5);
        let replies = ui_event(&mut state, 5, "open-params", None);
        assert!(matches!(
            replies[0],
            PluginToHost::ShowWindow { window_id: 1_000_005, .. }
        ));
    }

    #[test]
    fn param_field_updates_state_immediately_on_every_keystroke() {
        // Un INPUT è value-bearing → "param:shift" arriva con Some(valore)
        // ad ogni tasto (evento "input"), non solo al click su "Applica".
        let mut state = CryptoState::default();
        activate(&mut state, 1);
        ui_event(&mut state, 1_000_001, "param:shift", Some("1"));
        assert_eq!(
            state.params_by_cipher.get("caesar").unwrap().get("shift").unwrap(),
            "1"
        );
    }

    #[test]
    fn apply_resyncs_using_params_already_set_by_field_events() {
        let mut state = CryptoState::default();
        activate(&mut state, 1);
        ui_event(&mut state, 1, "plaintext", Some("ABC")); // shift 3 default → "DEF", last_edited=Plaintext
        ui_event(&mut state, 1_000_001, "param:shift", Some("1")); // campo, no re-render dei pannelli
        ui_event(&mut state, 1_000_001, "params-apply", None); // trigger, nessun valore proprio
        // Con shift=1 e last_edited=Plaintext, "ABC" (ancora nel pannello) → "BCD".
        assert_eq!(state.ciphertext, "BCD");
    }

    #[test]
    fn close_dialog_sends_close_window_for_its_own_id_only() {
        let mut state = CryptoState::default();
        activate(&mut state, 1);
        let replies = ui_event(&mut state, 1_000_001, "params-close", None);
        assert_eq!(replies, vec![PluginToHost::CloseWindow { window_id: 1_000_001 }]);
    }

    #[test]
    fn rsa_generate_populates_params_for_rsa_only() {
        let mut state = CryptoState::default();
        activate(&mut state, 1);
        ui_event(&mut state, 1, "cipher-select:rsa", None);
        ui_event(&mut state, 1_000_001, "params-generate", None);
        assert!(state.params_by_cipher.get("rsa").unwrap().contains_key("pub_n"));
    }

    #[test]
    fn error_on_apply_is_visible_in_state() {
        let mut state = CryptoState::default();
        activate(&mut state, 1);
        ui_event(&mut state, 1, "cipher-select:vigenere", None);
        ui_event(&mut state, 1, "plaintext", Some("ABC")); // vigenère senza keyword → errore
        assert!(state.last_error.is_some());
    }
}
