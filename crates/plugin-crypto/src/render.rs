//! HTML delle due finestre (2026-07-19, Docs/superpowers/specs/2026-07-19-crypto-plugin-design.md
//! §7-§8). Nessun template engine — `format!`/concatenazione di stringhe,
//! stesso principio di `plugin-calc`/`plugin-lc`. Classi CSS condivise dal
//! catalogo host (`plugin-catalog.css`) dove esistono già (`.lare-window`);
//! classi proprie del plugin (`.crypto-*`) per il layout specifico
//! (sidebar + due pannelli), come `plugin-lc` fa per le proprie.

use crate::ciphers::{all_ciphers, ParamField};
use crate::state::CryptoState;

const INLINE_CSS: &str = r#"
.crypto-root { display: flex; height: 100%; }
.crypto-sidebar { width: 140px; flex: none; border-right: 1px solid var(--border, #444); padding: 4px; }
.crypto-sidebar-item { padding: 4px 6px; cursor: pointer; border-radius: 4px; }
.crypto-sidebar-item.selected { background: rgba(255,255,255,.08); }
.crypto-panels { flex: 1; display: flex; }
.crypto-panel { flex: 1; padding: 8px; display: flex; flex-direction: column; }
.crypto-panel + .crypto-panel { border-left: 1px solid var(--border, #444); }
.crypto-panel textarea { min-height: 240px; resize: none; width: 100%; box-sizing: border-box; }
.crypto-toolbar { text-align: right; padding: 4px 8px; border-bottom: 1px solid var(--border, #444); }
.crypto-error { color: #e08080; padding: 4px 8px; font-size: .85em; }
"#;

/// Sidebar + due pannelli testo. Chiamata da `Activate` (prima
/// visualizzazione) e da ogni `UiEvent` che cambia testo/cifrario (§7).
pub fn render_main_window(state: &CryptoState) -> String {
    let sidebar = render_sidebar(state);
    let error_html = state
        .last_error
        .as_ref()
        .map(|e| format!("<div class=\"crypto-error\">{}</div>", html_escape(e)))
        .unwrap_or_default();

    format!(
        r#"<div class="lare-window crypto-root">
  <style>{INLINE_CSS}</style>
  {sidebar}
  <div style="flex:1; display:flex; flex-direction:column;">
    <div class="crypto-toolbar"><span class="lare-button" data-evt="open-params">⚙ Parametri</span></div>
    {error_html}
    <div class="crypto-panels">
      <div class="crypto-panel">
        <label>Testo in chiaro</label>
        <textarea class="lare-input" data-evt="plaintext">{plaintext}</textarea>
      </div>
      <div class="crypto-panel">
        <label>Testo cifrato</label>
        <textarea class="lare-input" data-evt="ciphertext">{ciphertext}</textarea>
      </div>
    </div>
  </div>
</div>"#,
        plaintext = html_escape(&state.plaintext),
        ciphertext = html_escape(&state.ciphertext),
    )
}

fn render_sidebar(state: &CryptoState) -> String {
    // Il nome del cifrario è incorporato nell'element_id stesso
    // (`cipher-select:caesar`, ecc.), non in un attributo `data-value` —
    // il runtime host legge `.value` solo da INPUT/SELECT/TEXTAREA (vedi
    // Task 1b), MAI da un `data-*` custom su un `<div>`. Un `data-value`
    // qui non solo non verrebbe letto: DOMPurify lo rimuoverebbe anche
    // dal DOM (non è nella allowlist `ADD_ATTR` del sanitizer host-side).
    let items: String = all_ciphers()
        .iter()
        .enumerate()
        .map(|(idx, cipher)| {
            let selected = if idx == state.current_cipher_index { " selected" } else { "" };
            format!(
                "<div class=\"crypto-sidebar-item{selected}\" data-evt=\"cipher-select:{id}\">{name}</div>",
                id = cipher.id(),
                name = html_escape(cipher.display_name()),
            )
        })
        .collect();
    format!("<div class=\"crypto-sidebar\">{items}</div>")
}

/// Dialog parametri: campi del cifrario CORRENTE (§8). Chiamata
/// all'apertura (`open-params`) e dopo "Genera" (`params-generate`, per
/// mostrare i campi di sola lettura appena popolati).
pub fn render_params_dialog(state: &CryptoState) -> String {
    let cipher = state.current_cipher();
    let params = state.current_params();
    let fields_html: String = cipher
        .params()
        .iter()
        .map(|field| render_param_field(field, &params))
        .collect();
    let error_html = state
        .last_error
        .as_ref()
        .map(|e| format!("<div class=\"crypto-error\">{}</div>", html_escape(e)))
        .unwrap_or_default();

    format!(
        r#"<div class="lare-window">
  <style>{INLINE_CSS}</style>
  <h3>{name}</h3>
  {error_html}
  {fields_html}
  <div class="crypto-toolbar">
    <span class="lare-button" data-evt="params-apply">Applica</span>
    <span class="lare-button" data-evt="params-close">Chiudi</span>
  </div>
</div>"#,
        name = html_escape(cipher.display_name()),
    )
}

fn render_param_field(field: &ParamField, params: &crate::ciphers::ParamValues) -> String {
    match field {
        ParamField::Number { key, label, default, .. } => {
            let value = params.get(*key).cloned().unwrap_or_else(|| default.to_string());
            format!(
                "<div><label>{label}</label><input class=\"lare-input\" data-evt=\"param:{key}\" value=\"{value}\"></div>",
                label = html_escape(label),
                key = key,
                value = html_escape(&value),
            )
        }
        ParamField::Text { key, label } => {
            let value = params.get(*key).cloned().unwrap_or_default();
            format!(
                "<div><label>{label}</label><input class=\"lare-input\" data-evt=\"param:{key}\" value=\"{value}\"></div>",
                label = html_escape(label),
                key = key,
                value = html_escape(&value),
            )
        }
        ParamField::GeneratedPair { action_label, fields } => {
            let readonly_fields: String = fields
                .iter()
                .map(|(key, label)| {
                    let value = params.get(*key).cloned().unwrap_or_default();
                    format!(
                        "<div><label>{label}</label><input class=\"lare-input\" readonly value=\"{value}\"></div>",
                        label = html_escape(label),
                        value = html_escape(&value),
                    )
                })
                .collect();
            format!(
                "<div><span class=\"lare-button\" data-evt=\"params-generate\">{action_label}</span>{readonly_fields}</div>",
                action_label = html_escape(action_label),
            )
        }
    }
}

/// Escape minimo per testo inserito in HTML — il DOMPurify lato host
/// (`plugin-window.html`) è l'ultima linea di difesa, ma non ci si affida
/// SOLO a quella (stesso principio di `plugin-calc`'s `html_escape`).
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::CryptoState;

    #[test]
    fn main_window_shows_all_three_ciphers_in_sidebar() {
        let state = CryptoState::default();
        let html = render_main_window(&state);
        assert!(html.contains("Cesare"));
        assert!(html.contains("Vigenère"));
        assert!(html.contains("RSA"));
    }

    #[test]
    fn main_window_marks_current_cipher_as_selected() {
        let mut state = CryptoState::default();
        state.current_cipher_index = 1; // Vigenère
        let html = render_main_window(&state);
        assert!(html.contains("selected\" data-evt=\"cipher-select:vigenere\""));
    }

    #[test]
    fn main_window_shows_current_panel_text() {
        let mut state = CryptoState::default();
        state.plaintext = "ABC".to_string();
        state.ciphertext = "DEF".to_string();
        let html = render_main_window(&state);
        assert!(html.contains(">ABC<") || html.contains("ABC</textarea>"));
        assert!(html.contains(">DEF<") || html.contains("DEF</textarea>"));
    }

    #[test]
    fn main_window_shows_error_when_present() {
        let mut state = CryptoState::default();
        state.last_error = Some("errore di prova".to_string());
        let html = render_main_window(&state);
        assert!(html.contains("errore di prova"));
    }

    #[test]
    fn params_dialog_shows_caesar_shift_field() {
        let state = CryptoState::default(); // Cesare di default
        let html = render_params_dialog(&state);
        assert!(html.contains("Spostamento"));
    }

    #[test]
    fn params_dialog_shows_rsa_generate_button() {
        let mut state = CryptoState::default();
        state.current_cipher_index = 2; // RSA
        let html = render_params_dialog(&state);
        assert!(html.contains("Genera"));
        assert!(html.contains("params-generate"));
    }

    #[test]
    fn params_dialog_has_apply_and_close_buttons() {
        let state = CryptoState::default();
        let html = render_params_dialog(&state);
        assert!(html.contains("params-apply"));
        assert!(html.contains("params-close"));
    }

    #[test]
    fn html_escape_prevents_raw_tag_injection() {
        let mut state = CryptoState::default();
        state.plaintext = "<script>".to_string();
        let html = render_main_window(&state);
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn textarea_has_explicit_min_height() {
        // Verifica che il CSS per .crypto-panel textarea contenga
        // un'altezza esplicita (min-height) invece di flex: 1,
        // che non ha una base da cui ereditare (bug fix Task 10).
        let state = CryptoState::default();
        let html = render_main_window(&state);
        assert!(
            html.contains("min-height"),
            "CSS deve contenere 'min-height' per le textarea"
        );
    }
}
