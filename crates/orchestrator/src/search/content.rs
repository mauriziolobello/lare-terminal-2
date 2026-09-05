//! # search::content
//!
//! Config JSON per la ricerca nel CONTENUTO dei file (`search-content.json`) —
//! stesso pattern di `paths_config.rs`: auto-generato al 1° avvio con default
//! sensati, poi editabile a mano; file assente/corrotto ⇒ rigenerato dai
//! default, mai un crash.

use std::io::{BufRead, BufReader};
use std::path::Path;

use serde::{Deserialize, Serialize};

fn default_text_extensions() -> Vec<String> {
    [
        "txt", "md", "rs", "py", "js", "mjs", "ts", "tsx", "jsx", "json", "csv", "log",
        "html", "htm", "css", "xml", "yaml", "yml", "toml", "ini", "cfg", "conf",
        "java", "c", "cpp", "h", "hpp", "go", "rb", "sql", "sh", "ps1", "bat", "php",
        "cs", "kt", "swift", "vue", "svelte",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

fn default_binary_extensions() -> Vec<String> {
    [
        "exe", "dll", "so", "dylib", "png", "jpg", "jpeg", "gif", "bmp", "ico", "webp",
        "mp3", "mp4", "avi", "mov", "mkv", "wav", "flac",
        "zip", "rar", "7z", "gz", "tar", "bz2", "xz", "iso",
        "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx",
        "db", "sqlite", "sqlite3", "bin", "dat", "class", "pyc", "o", "obj",
        "woff", "woff2", "ttf", "otf", "eot",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

fn default_max_file_size_kb() -> u64 {
    5120
}

fn default_max_unknown_scan() -> usize {
    5000
}

/// Versione dello schema di `search-content.json`. Nessuna migrazione ancora
/// (file nuovo, nessuna versione precedente da cui migrare) — il campo esiste
/// per parità con `PathsConfig::version` in vista di un futuro cambio.
pub const CURRENT_CONTENT_CONFIG_VERSION: u32 = 1;

fn default_version() -> u32 {
    CURRENT_CONTENT_CONFIG_VERSION
}

/// Config persistente per la ricerca nel contenuto, in `search-content.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentConfig {
    #[serde(default = "default_text_extensions")]
    pub text_extensions: Vec<String>,
    #[serde(default = "default_binary_extensions")]
    pub binary_extensions: Vec<String>,
    #[serde(default = "default_max_file_size_kb")]
    pub max_file_size_kb: u64,
    #[serde(default = "default_max_unknown_scan")]
    pub max_unknown_scan: usize,
    #[serde(default = "default_version")]
    pub version: u32,
}

impl ContentConfig {
    /// Carica da `path` se presente e JSON valido; altrimenti genera i default,
    /// li salva su `path` (best-effort — un fallimento di scrittura non impedisce
    /// di restituire una config usabile) e li ritorna.
    pub fn load_or_generate(path: &Path) -> Self {
        if let Ok(content) = std::fs::read_to_string(path) {
            if let Ok(cfg) = serde_json::from_str::<ContentConfig>(&content) {
                return cfg;
            }
            // File presente ma JSON invalido → rigenera (sovrascrive il file corrotto).
        }

        let cfg = ContentConfig {
            text_extensions: default_text_extensions(),
            binary_extensions: default_binary_extensions(),
            max_file_size_kb: default_max_file_size_kb(),
            max_unknown_scan: default_max_unknown_scan(),
            version: CURRENT_CONTENT_CONFIG_VERSION,
        };
        let _ = cfg.save(path);
        cfg
    }

    fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("search-content: cannot create dir: {e}"))?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("search-content: serialization error: {e}"))?;
        std::fs::write(path, json).map_err(|e| format!("search-content: write error: {e}"))
    }
}

/// Classificazione di un file per la ricerca contenuto, basata SOLO
/// sull'estensione (case-insensitive) — vedi `ContentConfig`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionClass {
    /// Estensione in `text_extensions` — aperto e cercato subito (Fase A).
    Text,
    /// Estensione in `binary_extensions` — mai aperto.
    Binary,
    /// Né l'una né l'altra (incluso nessuna estensione) — accodato per la Fase B.
    Unknown,
}

/// Classifica `path` per estensione. Nessuna estensione ⇒ `Unknown` (mai `Text`
/// né `Binary`: un file senza estensione non è mai escluso a priori).
pub fn classify_extension(path: &Path, cfg: &ContentConfig) -> ExtensionClass {
    let ext = match path.extension().and_then(|e| e.to_str()) {
        Some(e) => e.to_lowercase(),
        None => return ExtensionClass::Unknown,
    };
    if cfg.text_extensions.iter().any(|e| e == &ext) {
        return ExtensionClass::Text;
    }
    if cfg.binary_extensions.iter().any(|e| e == &ext) {
        return ExtensionClass::Binary;
    }
    ExtensionClass::Unknown
}

/// Sniff: `true` se `bytes` sembra testo. Euristica minima: assenza di byte
/// null nei primi byte (i formati binari comuni — eseguibili, immagini,
/// archivi — contengono quasi sempre un byte null molto presto; il testo
/// codificato in UTF-8/ASCII/Latin-1 non ne ha mai). `bytes` vuoto ⇒ testo
/// (file vuoto, nulla da cui dedurre binarietà).
pub fn looks_like_text(bytes: &[u8]) -> bool {
    !bytes.contains(&0)
}

/// Legge i primi 8000 byte di `path` e applica `looks_like_text`.
/// File illeggibile (assente, permessi, ecc.) ⇒ `false` — trattato come
/// binario: prudente, mai apre un file che non riesce nemmeno a leggere.
pub fn sniff_file(path: &Path) -> bool {
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    let mut buf = [0u8; 8000];
    let Ok(n) = f.read(&mut buf) else {
        return false;
    };
    looks_like_text(&buf[..n])
}

/// Matcher di frase esatta (case-insensitive) per la ricerca contenuto.
/// Frase vuota ⇒ non matcha mai (coerente con `query::Matcher::Tokens` vuoto —
/// stessa convenzione "vuoto = nessun match" del matcher sul nome).
#[derive(Debug, Clone)]
pub struct ContentMatcher {
    phrase_lower: String,
}

impl ContentMatcher {
    pub fn new(phrase: &str) -> Self {
        Self { phrase_lower: phrase.to_lowercase() }
    }

    /// Cerca la frase riga per riga in `path`; ritorna la PRIMA riga che
    /// matcha come `(numero_riga_1_based, testo_troncato_a_200_char)`.
    /// `None` se: la frase è vuota, il file è più grande di `max_file_size_kb`
    /// (mai aperto), il file non è leggibile, o nessuna riga matcha. Righe non
    /// valide UTF-8 sono saltate (non un errore fatale per l'intero file).
    pub fn find_first_match(&self, path: &Path, max_file_size_kb: u64) -> Option<(u32, String)> {
        if self.phrase_lower.is_empty() {
            return None;
        }
        let meta = std::fs::metadata(path).ok()?;
        if meta.len() > max_file_size_kb.saturating_mul(1024) {
            return None;
        }
        let file = std::fs::File::open(path).ok()?;
        let reader = BufReader::new(file);
        for (idx, line) in reader.lines().enumerate() {
            let line = match line {
                Ok(l) => l,
                Err(_) => continue,
            };
            if line.to_lowercase().contains(&self.phrase_lower) {
                let snippet: String = line.chars().take(200).collect();
                return Some(((idx + 1) as u32, snippet));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn generate_on_first_run_then_reload() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("search-content.json");
        let cfg = ContentConfig::load_or_generate(&p);
        assert!(p.exists(), "il file deve essere generato al 1° avvio");
        assert!(cfg.text_extensions.contains(&"rs".to_string()));
        assert!(cfg.binary_extensions.contains(&"pdf".to_string()));
        assert_eq!(cfg.max_file_size_kb, 5120);
        assert_eq!(cfg.max_unknown_scan, 5000);

        let cfg2 = ContentConfig::load_or_generate(&p);
        assert_eq!(cfg.text_extensions, cfg2.text_extensions);
    }

    #[test]
    fn corrupted_file_regenerates_default() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("search-content.json");
        std::fs::write(&p, "{not json").unwrap();
        let cfg = ContentConfig::load_or_generate(&p);
        assert_eq!(cfg.max_file_size_kb, 5120);
    }

    #[test]
    fn missing_fields_use_defaults() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("search-content.json");
        std::fs::write(&p, r#"{"max_unknown_scan": 42}"#).unwrap();
        let cfg = ContentConfig::load_or_generate(&p);
        assert_eq!(cfg.max_unknown_scan, 42, "il campo esplicito deve vincere");
        assert_eq!(cfg.max_file_size_kb, 5120, "il campo assente usa il default");
        assert!(!cfg.text_extensions.is_empty(), "il campo assente usa il default");
    }

    // ── classify_extension ───────────────────────────────────────────────────

    #[test]
    fn text_extension_classified_as_text() {
        let cfg = ContentConfig::load_or_generate(&TempDir::new().unwrap().path().join("c.json"));
        assert_eq!(classify_extension(Path::new("note.md"), &cfg), ExtensionClass::Text);
    }

    #[test]
    fn binary_extension_classified_as_binary() {
        let cfg = ContentConfig::load_or_generate(&TempDir::new().unwrap().path().join("c.json"));
        assert_eq!(classify_extension(Path::new("report.pdf"), &cfg), ExtensionClass::Binary);
    }

    #[test]
    fn unknown_extension_classified_as_unknown() {
        let cfg = ContentConfig::load_or_generate(&TempDir::new().unwrap().path().join("c.json"));
        assert_eq!(classify_extension(Path::new("weird.xyz123"), &cfg), ExtensionClass::Unknown);
    }

    #[test]
    fn no_extension_classified_as_unknown() {
        let cfg = ContentConfig::load_or_generate(&TempDir::new().unwrap().path().join("c.json"));
        assert_eq!(classify_extension(Path::new("Makefile"), &cfg), ExtensionClass::Unknown);
    }

    #[test]
    fn extension_match_is_case_insensitive() {
        let cfg = ContentConfig::load_or_generate(&TempDir::new().unwrap().path().join("c.json"));
        assert_eq!(classify_extension(Path::new("NOTE.MD"), &cfg), ExtensionClass::Text);
    }

    // ── looks_like_text / sniff_file ─────────────────────────────────────────

    #[test]
    fn text_bytes_look_like_text() {
        assert!(looks_like_text(b"hello world\nsecond line\n"));
    }

    #[test]
    fn null_byte_looks_binary() {
        assert!(!looks_like_text(b"hello\x00world"));
    }

    #[test]
    fn empty_bytes_look_like_text() {
        assert!(looks_like_text(b""));
    }

    #[test]
    fn sniff_file_reads_text_file() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("note.unknownext");
        std::fs::write(&p, "contenuto testuale semplice").unwrap();
        assert!(sniff_file(&p));
    }

    #[test]
    fn sniff_file_detects_binary_file() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("blob.unknownext");
        std::fs::write(&p, [0u8, 1, 2, 3, 0, 255]).unwrap();
        assert!(!sniff_file(&p));
    }

    #[test]
    fn sniff_file_missing_file_is_not_text() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("does-not-exist.unknownext");
        assert!(!sniff_file(&p), "file illeggibile ⇒ trattato come binario, mai aperto");
    }

    // ── ContentMatcher ────────────────────────────────────────────────────────

    fn write_lines(dir: &TempDir, name: &str, lines: &[&str]) -> std::path::PathBuf {
        let p = dir.path().join(name);
        std::fs::write(&p, lines.join("\n")).unwrap();
        p
    }

    #[test]
    fn finds_first_matching_line_case_insensitive() {
        let dir = TempDir::new().unwrap();
        let p = write_lines(&dir, "f.txt", &["prima riga", "qui c'è il TOTALE fattura", "altra riga"]);
        let m = ContentMatcher::new("totale fattura");
        let (line, snippet) = m.find_first_match(&p, 5120).unwrap();
        assert_eq!(line, 2);
        assert_eq!(snippet, "qui c'è il TOTALE fattura");
    }

    #[test]
    fn returns_none_when_phrase_absent() {
        let dir = TempDir::new().unwrap();
        let p = write_lines(&dir, "f.txt", &["niente qui", "né qui"]);
        let m = ContentMatcher::new("totale fattura");
        assert!(m.find_first_match(&p, 5120).is_none());
    }

    #[test]
    fn phrase_split_across_two_lines_does_not_match() {
        let dir = TempDir::new().unwrap();
        let p = write_lines(&dir, "f.txt", &["il totale", "fattura è qui"]);
        let m = ContentMatcher::new("totale fattura");
        assert!(m.find_first_match(&p, 5120).is_none(), "match multi-riga non deve scattare");
    }

    #[test]
    fn only_first_matching_line_is_returned() {
        let dir = TempDir::new().unwrap();
        let p = write_lines(&dir, "f.txt", &["TODO uno", "TODO due", "TODO tre"]);
        let m = ContentMatcher::new("todo");
        let (line, snippet) = m.find_first_match(&p, 5120).unwrap();
        assert_eq!(line, 1);
        assert_eq!(snippet, "TODO uno");
    }

    #[test]
    fn file_larger_than_cap_is_never_opened() {
        let dir = TempDir::new().unwrap();
        // 2 KB di contenuto che matcherebbe, ma il cap è 1 KB.
        let big = "x".repeat(2048) + " TOTALE";
        let p = dir.path().join("big.txt");
        std::fs::write(&p, &big).unwrap();
        let m = ContentMatcher::new("totale");
        assert!(m.find_first_match(&p, 1).is_none(), "file oltre max_file_size_kb non va aperto");
    }

    #[test]
    fn empty_phrase_matches_nothing() {
        let dir = TempDir::new().unwrap();
        let p = write_lines(&dir, "f.txt", &["qualunque riga"]);
        let m = ContentMatcher::new("");
        assert!(m.find_first_match(&p, 5120).is_none(), "frase vuota non deve matchare tutto");
    }

    #[test]
    fn missing_file_returns_none() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("does-not-exist.txt");
        let m = ContentMatcher::new("qualunque");
        assert!(m.find_first_match(&p, 5120).is_none());
    }
}
