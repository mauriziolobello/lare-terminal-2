//! Discovery: scandisce `plugins/`, parse del manifest, localizza il binario per convenzione.
//!
//! Pattern atteso sul filesystem:
//! ```text
//! plugins/
//!   <id>/
//!     plugin.json     -- manifest (PluginManifest)
//!     <id>(.exe)      -- binario del plugin (convenzione: stessa dir, stesso nome dell'id)
//! ```
//!
//! Voci invalide (nessun manifest, manifest malformato, binario assente) vengono
//! saltate con un messaggio a stderr — nessun panic, nessun errore fatale.
//! Dir inesistente o illeggibile -> risultato vuoto (comportamento corretto in Fase 0
//! quando l'utente non ha ancora installato plugin).

use plugin_protocol::{parse_manifest, PluginManifest};
use std::path::{Path, PathBuf};

/// Un plugin trovato e validato sul filesystem (manifest valido + binario presente).
///
/// Questo tipo e' prodotto da `discover` e consumato dal `PluginHost` (Task 5):
/// contiene tutto il necessario per decidere la spawn policy e avviare il processo.
#[derive(Debug, Clone)]
pub struct DiscoveredPlugin {
    /// Contenuto del `plugin.json` parsato.
    pub manifest: PluginManifest,
    /// Path assoluto al binario: `plugins/<id>/<id>(.exe)`.
    pub bin_path: PathBuf,
}

/// Scandisce `plugins_dir` e ritorna tutti i plugin validi trovati.
///
/// Per ogni entry della directory:
/// 1. Controlla che sia una sottocartella.
/// 2. Legge `plugin.json` e lo parsa come `PluginManifest`.
/// 3. Cerca il binario `<id>` (o `<id>.exe` su Windows) nella stessa cartella.
/// 4. Se tutti i passi hanno successo, aggiunge il plugin alla lista.
///
/// Voci invalide sono saltate (log a stderr). Dir inesistente -> Vec vuoto.
pub fn discover(plugins_dir: &Path) -> Vec<DiscoveredPlugin> {
    let mut out = Vec::new();

    // `read_dir` fallisce se la dir non esiste o non e' leggibile — in quel caso
    // restituiamo semplicemente un vettore vuoto (nessun plugin installato e' ok).
    let Ok(entries) = std::fs::read_dir(plugins_dir) else {
        return out;
    };

    for entry in entries.flatten() {
        let dir = entry.path();

        // Salta file e symlink: ci interessano solo le sottocartelle.
        if !dir.is_dir() {
            continue;
        }

        // Leggi e parsa il manifest.
        let manifest_path = dir.join("plugin.json");
        let Ok(text) = std::fs::read_to_string(&manifest_path) else {
            // La dir non contiene `plugin.json` -> non e' una dir plugin, saltala in silenzio.
            continue;
        };
        let Ok(manifest) = parse_manifest(&text) else {
            // JSON malformato: segnala ma non panic.
            eprintln!(
                "[plugins] manifest illegale in {}: JSON non valido",
                dir.display()
            );
            continue;
        };

        // ── Validazione id ─────────────────────────────────────────────────────────
        // ATTENZIONE — footgun di `Path::join`: se il secondo argomento è un percorso
        // assoluto, `join` SCARTA il prefisso e ritorna solo il secondo argomento.
        //   Esempio: Path::new("/plugins/foo").join("C:/Windows/calc") == "C:/Windows/calc"
        // Un `id` con `/`, `..` o lettera di drive (`C:`) consentirebbe a un manifest
        // non fidato di far puntare `bin_path` fuori dalla cartella plugin → esecuzione
        // arbitraria di binari di sistema.
        //
        // La regola è semplice: `id` deve essere composto solo da caratteri
        // ASCII alfanumerici, `_` e `-`. Qualsiasi altro carattere è rifiutato.
        let id_is_safe = !manifest.id.is_empty()
            && manifest
                .id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');

        if !id_is_safe {
            eprintln!(
                "[plugins] id non valido '{}' in {} — saltato",
                manifest.id,
                dir.display()
            );
            continue;
        }

        // Localizza il binario per convenzione: stessa dir del manifest, nome = id del plugin.
        // Su Windows i binari hanno l'estensione `.exe`; su Unix no.
        let bin_name = if cfg!(windows) {
            format!("{}.exe", manifest.id)
        } else {
            manifest.id.clone()
        };
        let bin_path = dir.join(&bin_name);

        if !bin_path.exists() {
            eprintln!(
                "[plugins] binario assente per '{}': {}",
                manifest.id,
                bin_path.display()
            );
            continue;
        }

        out.push(DiscoveredPlugin { manifest, bin_path });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    /// Crea una fake `plugins/<id>/` con manifest + un file binario fittizio.
    fn make_plugin(root: &Path, id: &str, manifest_json: &str, with_bin: bool) {
        let dir = root.join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("plugin.json"), manifest_json).unwrap();
        if with_bin {
            // su Windows il binario per convenzione e' <id>.exe
            let bin = if cfg!(windows) { format!("{id}.exe") } else { id.to_string() };
            fs::write(dir.join(bin), b"fake").unwrap();
        }
    }

    #[test]
    fn discovers_valid_plugin() {
        let tmp = tempfile::tempdir().unwrap();
        make_plugin(tmp.path(), "ping",
            r#"{"name":"Ping","id":"ping","version":"1.0.0","protocol_version":1,
                "triggers":{"command":"/ping"}}"#, true);

        let found = discover(tmp.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].manifest.id, "ping");
        assert!(found[0].bin_path.ends_with(if cfg!(windows) { "ping.exe" } else { "ping" }));
    }

    #[test]
    fn skips_dir_without_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("empty")).unwrap();
        assert_eq!(discover(tmp.path()).len(), 0);
    }

    #[test]
    fn skips_invalid_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        make_plugin(tmp.path(), "bad", "{ not json", true);
        assert_eq!(discover(tmp.path()).len(), 0);
    }

    #[test]
    fn missing_plugins_dir_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(discover(&tmp.path().join("nope")).len(), 0);
    }

    /// Verifica che un manifest con `id` contenente `.` o `/` venga rifiutato.
    ///
    /// Setup del test:
    ///   plugins_dir/attacker/plugin.json  →  id = "../evil"
    ///   plugins_dir/evil(.exe)            →  binario fittizio al path NAÏVE risolto
    ///
    /// Senza la guardia, `dir.join("../evil.exe")` risolve a `plugins_dir/evil.exe`
    /// (il sistema operativo normalizza i `..`), il file esiste, e il plugin verrebbe
    /// "scoperto" — aprendo la strada a esecuzione arbitraria.
    /// Con la guardia il plugin è saltato e `discover` ritorna 0.
    #[test]
    fn skips_plugin_with_illegal_id() {
        let tmp = tempfile::tempdir().unwrap();

        // Dir del plugin: si chiama "attacker", ma il campo `id` nel manifest è "../evil".
        // `discover` legge l'id dal contenuto JSON, non dal nome della cartella.
        let attacker_dir = tmp.path().join("attacker");
        fs::create_dir_all(&attacker_dir).unwrap();
        fs::write(
            attacker_dir.join("plugin.json"),
            r#"{"name":"Evil","id":"../evil","version":"1.0.0","protocol_version":1,
               "triggers":{"command":"/evil"}}"#,
        )
        .unwrap();

        // Crea il binario nel percorso NAÏVE a cui arriva `dir.join("../evil(.exe)")`.
        // Su Windows il binario ha estensione `.exe`; su Unix no.
        // Senza questa pedina, la discovery fallirebbe già per "binario assente"
        // e il test darebbe verde per il motivo sbagliato.
        let naive_bin = if cfg!(windows) { "evil.exe" } else { "evil" };
        fs::write(tmp.path().join(naive_bin), b"fake").unwrap();

        // Con la guardia: len == 0 (id rifiutato).
        // Senza la guardia: len == 1 (il binario esiste, verrebbe scoperto).
        assert_eq!(discover(tmp.path()).len(), 0);
    }
}
