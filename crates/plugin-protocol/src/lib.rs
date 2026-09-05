//! Contract P — protocollo host (orchestrator) <-> plugin sidecar, JSON-per-riga su stdio.
//! Versionato e ADDITIVO (Fase 0: Init/Deinit + Ready/Log).
use serde::{Deserialize, Serialize};

/// Trigger dichiarati nel manifest: da questi discende il ciclo di vita (Activate/OnTimer)
/// e la spawn policy (eager/lazy). `Default` = nessun trigger.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Triggers {
    /// es. "/calc" -> il plugin riceve `Activate` allo slash comando.
    #[serde(default)]
    pub command: Option<String>,
    /// es. "5m" -> il plugin riceve `OnTimer` alla cadenza (Fase 2).
    #[serde(default)]
    pub interval: Option<String>,
}

/// Dimensione iniziale della finestra plugin, dichiarata nel manifest statico.
/// Opzionale: se assente, l'host usa il default generico (480×360) — vedi
/// `open_plugin_window` in `ui/src-tauri/src/main.rs`.
///
/// È un semplice value object (`Copy`): due `f64` senza invarianti. In termini OOP
/// è una "struct di dati" senza comportamento — la teniamo `Copy` così l'host può
/// duplicarla liberamente (es. catturarla in un `async move`) senza `.clone()`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WindowSize {
    pub width: f64,
    pub height: f64,
}

/// Manifest statico `plugins/<id>/plugin.json` — ispezionabile senza eseguire il plugin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginManifest {
    pub name: String,
    pub id: String,
    pub version: String,
    pub protocol_version: u32,
    /// `triggers` assente nel JSON -> `Triggers::default()`.
    #[serde(default)]
    pub triggers: Triggers,
    /// Dimensione iniziale preferita della finestra — assente nel JSON -> `None`,
    /// l'host userà il default generico. Dichiarata UNA VOLTA per tipo di plugin,
    /// non varia per attivazione (a differenza di `ShowWindow`, che è dinamico).
    #[serde(default)]
    pub window: Option<WindowSize>,
    // config_schema (Fase 3) deferito: i campi extra nel JSON sono ignorati da serde.
}

/// Parse del manifest da stringa JSON.
pub fn parse_manifest(json: &str) -> serde_json::Result<PluginManifest> {
    serde_json::from_str(json)
}

/// Host -> Plugin. `#[serde(tag = "type")]` => `{"type":"Init", ...}`.
///
/// Ogni variante corrisponde a un messaggio JSON con il campo `"type"` discriminante.
/// In Rust questo si chiama "tagged enum" — equivalente a un'interfaccia con sottotipi in OOP.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum HostToPlugin {
    // --- Fase 0 (invariante) ---

    /// Primo messaggio inviato dopo lo spawn: negozia la versione del protocollo,
    /// passa la configurazione e la directory di storage del plugin.
    Init {
        protocol_version: u32,
        config: serde_json::Value,
        storage_dir: String,
    },
    /// Inviato prima di terminare il plugin: segnale di shutdown pulito.
    Deinit {},

    // --- Slice 1: gestione finestre ---

    /// Chiede al plugin di aprire/mostrare una finestra con l'id dato.
    /// `window_id` — identificatore unico generato dall'host per questa sessione.
    /// `args` — parametri opzionali (es. selezione corrente, path file…) passati dal
    ///           comando slash; può essere `null` se non servono argomenti.
    Activate {
        window_id: u64,
        args: serde_json::Value,
    },

    /// Notifica il plugin di un evento UI generato dall'utente nella finestra.
    /// `element_id` — corrisponde al valore di `data-evt` sull'elemento HTML cliccato.
    /// `value` — `Some(str)` per `<input>`/`<select>` (valore corrente), `None` per pulsanti.
    UiEvent {
        window_id: u64,
        element_id: String,
        value: Option<String>,
    },
}

/// Plugin -> Host. Additivo: Fase 0 = Ready/Log; Slice 1 aggiunge le varianti finestra.
///
/// Anche qui `#[serde(tag = "type")]`: ogni variante viene serializzata come
/// `{"type":"NomeVariante", ...}` — il tag permette al deserializzatore di
/// scegliere la variante giusta leggendo solo il campo `"type"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PluginToHost {
    // --- Fase 0 (invariante) ---

    /// Primo messaggio obbligatorio inviato dal plugin dopo aver ricevuto `Init`.
    /// Conferma che il plugin è operativo e dichiara il suo nome.
    Ready { name: String, protocol_version: u32 },
    /// Messaggio di log generico: l'orchestrator può stamparlo / filtrarlo per `level`.
    Log { level: String, msg: String },

    // --- Slice 1: gestione finestre ---

    /// Ordina all'host di aprire una nuova finestra con il contenuto HTML dato.
    /// L'HTML è l'intero body della finestra (full-HTML, non patch parziali).
    /// L'host inietta il foglio di stile del catalogo prima del render.
    ShowWindow {
        window_id: u64,
        title: String,
        html: String,
    },

    /// Rimpiazza l'intero contenuto HTML della finestra aperta.
    /// Strategia "full-HTML replace": più semplice e prevedibile di patch element-level.
    UpdateWindow {
        window_id: u64,
        html: String,
    },

    /// Chiede all'host di chiudere la finestra. Inviato dal plugin quando non
    /// ha più stato da mostrare (es. operazione completata).
    CloseWindow {
        window_id: u64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_parses_triggers() {
        let json = r#"{
            "name": "Ping", "id": "ping", "version": "1.0.0",
            "protocol_version": 1,
            "triggers": { "command": "/ping", "interval": null }
        }"#;
        let m = parse_manifest(json).expect("parse");
        assert_eq!(m.id, "ping");
        assert_eq!(m.protocol_version, 1);
        assert_eq!(m.triggers.command.as_deref(), Some("/ping"));
        assert_eq!(m.triggers.interval, None);
    }

    #[test]
    fn manifest_without_triggers_defaults_empty() {
        // triggers assente -> Triggers::default() (campi None)
        let json = r#"{ "name": "X", "id": "x", "version": "1", "protocol_version": 1 }"#;
        let m = parse_manifest(json).expect("parse");
        assert_eq!(m.triggers, Triggers::default());
    }

    // --- Slice UX: dimensione finestra opzionale nel manifest ---

    #[test]
    fn manifest_without_window_defaults_none() {
        // `window` assente nel JSON -> None (retro-compatibilità: i plugin esistenti
        // — calc/ping/counter — non dichiarano `window` e devono continuare a parsare).
        let json = r#"{ "name": "X", "id": "x", "version": "1", "protocol_version": 1 }"#;
        let m = parse_manifest(json).expect("parse");
        assert_eq!(m.window, None);
    }

    #[test]
    fn manifest_with_window_parses_size() {
        // `window` presente -> Some(WindowSize { width, height }).
        let json = r#"{
            "name": "Lare Commander", "id": "lc", "version": "0.3.0",
            "protocol_version": 1,
            "window": { "width": 960.0, "height": 620.0 }
        }"#;
        let m = parse_manifest(json).expect("parse");
        assert_eq!(m.window, Some(WindowSize { width: 960.0, height: 620.0 }));
    }

    #[test]
    fn host_to_plugin_init_roundtrips() {
        let msg = HostToPlugin::Init {
            protocol_version: 1,
            config: serde_json::json!({ "k": "v" }),
            storage_dir: "C:/data/ping".into(),
        };
        let line = serde_json::to_string(&msg).unwrap();
        let back: HostToPlugin = serde_json::from_str(&line).unwrap();
        assert_eq!(msg, back);
        assert!(line.contains("\"type\":\"Init\""));
    }

    #[test]
    fn plugin_to_host_ready_roundtrips() {
        let msg = PluginToHost::Ready { name: "ping".into(), protocol_version: 1 };
        let line = serde_json::to_string(&msg).unwrap();
        let back: PluginToHost = serde_json::from_str(&line).unwrap();
        assert_eq!(msg, back);
    }

    // --- Slice 1: test RED per i nuovi variant finestra ---

    #[test]
    fn activate_roundtrips() {
        let m = HostToPlugin::Activate { window_id: 7, args: serde_json::json!({"a":1}) };
        let s = serde_json::to_string(&m).unwrap();
        assert!(s.contains("\"type\":\"Activate\""));
        assert_eq!(serde_json::from_str::<HostToPlugin>(&s).unwrap(), m);
    }

    #[test]
    fn ui_event_roundtrips() {
        let m = HostToPlugin::UiEvent { window_id: 7, element_id: "inc".into(), value: None };
        assert_eq!(
            serde_json::from_str::<HostToPlugin>(&serde_json::to_string(&m).unwrap()).unwrap(),
            m
        );
    }

    #[test]
    fn show_and_update_window_roundtrip() {
        let s = PluginToHost::ShowWindow { window_id: 1, title: "T".into(), html: "<b>h</b>".into() };
        let u = PluginToHost::UpdateWindow { window_id: 1, html: "<b>h2</b>".into() };
        let c = PluginToHost::CloseWindow { window_id: 1 };
        for m in [s, u, c] {
            assert_eq!(
                serde_json::from_str::<PluginToHost>(&serde_json::to_string(&m).unwrap()).unwrap(),
                m
            );
        }
    }
}
