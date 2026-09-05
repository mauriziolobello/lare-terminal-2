// archive.rs — Window archive for Lare Terminal.
//
// Responsibility (SRP): owns the archive logic — saving Markdown window content
// to disk as `.md` files, listing archived files, and opening them.
// Does NOT touch Tauri APIs (those are in main.rs).
//
// File location at runtime: `<config_dir>/library/*.md` (2.0: `config_dir` is
// resolved once via `ConfigDirState`, see main.rs's `library_dir_path`).
//
// The `dir` parameter is injectable (&Path) so all functions are fully
// testable without a running Tauri instance — same pattern as config.rs.
//
// File format:
//   Line 1: `<!-- lare-title: <title> -->` (HTML comment, sanitized by marked/DOMPurify on re-render)
//   Line 2+: the original Markdown content

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::UNIX_EPOCH;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A metadata entry for a file in the archive directory.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ArchiveEntry {
    /// Title extracted from the `<!-- lare-title: … -->` marker, or the filename stem.
    pub title: String,
    /// Plain filename (no directory component) inside the archive dir.
    pub file: String,
    /// File modification time as milliseconds since UNIX epoch.
    pub modified_ms: u64,
}

/// The full content of an archived Markdown window.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ArchiveDoc {
    /// Title extracted from the `<!-- lare-title: … -->` marker.
    pub title: String,
    /// The Markdown content (marker line excluded).
    pub content: String,
}

// ---------------------------------------------------------------------------
// Library folder types (Slice 1 — backend)
// ---------------------------------------------------------------------------

/// Un nodo dell'albero della Library: rappresenta una cartella fisica con
/// le sue sotto-cartelle (`folders`) e i file `.md` contenuti (`files`).
///
/// La ricorsione è sicura: `list_tree` usa un contatore di profondità per
/// evitare loop infiniti su symlink o strutture patologicamente annidate.
#[derive(Debug, Clone, Serialize)]
pub struct LibraryNode {
    /// Nome della cartella (solo l'ultimo segmento, es. "Progetti").
    pub name: String,
    /// Path relativo alla root della Library, con separatori `/`,
    /// es. "Progetti/Web". Usato dal frontend come chiave di navigazione.
    pub rel_path: String,
    /// Sotto-cartelle (albero ricorsivo).
    pub folders: Vec<LibraryNode>,
    /// File `.md` presenti direttamente in questa cartella.
    /// Il campo `file` di ciascun `ArchiveEntry` è il path relativo alla root
    /// Library con `/`, es. "Progetti/doc.md".
    pub files: Vec<ArchiveEntry>,
}

/// L'albero completo della Library, restituito da `list_tree`.
///
/// Separa i file nella root ("piatti") dalle cartelle di primo livello.
#[derive(Debug, Clone, Serialize)]
pub struct LibraryTree {
    /// File `.md` nella radice della Library (non in nessuna sottocartella).
    pub root_files: Vec<ArchiveEntry>,
    /// Cartelle di primo livello, ciascuna con il suo albero di sotto-cartelle.
    pub folders: Vec<LibraryNode>,
}

// ---------------------------------------------------------------------------
// Find types
// ---------------------------------------------------------------------------

/// A single file-search hit: a path and how it was found.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArchiveHit {
    /// Absolute path of the matching file.
    pub path: String,
    /// Origin of the match (e.g. "cwd", "external").
    pub source: String,
}

/// The full content of a saved Find session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArchiveFind {
    /// The search query that produced the hits.
    pub query: String,
    /// Timestamp (milliseconds since UNIX epoch) when the session was saved.
    pub ts: u64,
    /// All file-system hits from this search session.
    pub hits: Vec<ArchiveHit>,
}

/// A metadata entry for a saved Find session, used by `list_find`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FindEntry {
    /// Plain filename (no directory component) of the `.json` file.
    pub file: String,
    /// The search query stored inside the file.
    pub query: String,
    /// Number of hits in this saved session.
    pub count: usize,
    /// File modification time as milliseconds since UNIX epoch.
    pub modified: u64,
}

// ---------------------------------------------------------------------------
// Slug
// ---------------------------------------------------------------------------

/// Generate an ASCII-safe slug from a window title.
///
/// Rules:
/// - Transliterate accented letters to their ASCII base (é→e, à→a, etc.)
///   via a hand-rolled table; non-transliterable chars pass through as-is.
/// - Convert to lowercase.
/// - Replace everything outside `[a-z0-9]` with `-`.
/// - Collapse consecutive `-` to one.
/// - Trim leading and trailing `-`.
/// - Truncate to at most 40 characters (never ending with `-`).
///
/// An empty (or all-punctuation) input returns `"untitled"`.
pub fn slug(title: &str) -> String {
    // Step 1: transliterate accented chars + collect remaining chars.
    // `transliterate` returns a &str slice ("a", "e", ...) for known
    // accented chars and a single-char string for everything else.
    let mut transliterated = String::with_capacity(title.len() * 2);
    for ch in title.chars() {
        if let Some(replacement) = transliterate(ch) {
            transliterated.push_str(replacement);
        } else {
            transliterated.push(ch);
        }
    }

    // Step 2: lowercase + replace non-ASCII-alphanumerics with `-`.
    let normalized: String = transliterated
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();

    // Step 3: collapse consecutive `-` and trim leading dashes.
    let mut result = String::with_capacity(normalized.len());
    let mut last_was_dash = true; // start as true to trim leading dashes
    for c in normalized.chars() {
        if c == '-' {
            if !last_was_dash {
                result.push('-');
                last_was_dash = true;
            }
        } else {
            result.push(c);
            last_was_dash = false;
        }
    }
    // Trim trailing dash.
    let result = result.trim_end_matches('-').to_string();

    // Step 4: truncate to 40 chars without leaving a trailing `-`.
    let truncated = if result.len() <= 40 {
        result
    } else {
        // Truncate then trim any trailing dash caused by the cut.
        result[..40].trim_end_matches('-').to_string()
    };

    if truncated.is_empty() {
        "untitled".to_string()
    } else {
        truncated
    }
}

/// Hand-rolled transliteration table for common accented Latin characters.
///
/// Returns `Some(&str)` with the ASCII equivalent for known accented chars,
/// or `None` to signal "pass through as-is" for everything else.
fn transliterate(ch: char) -> Option<&'static str> {
    match ch {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' => Some("a"),
        'è' | 'é' | 'ê' | 'ë' | 'È' | 'É' | 'Ê' | 'Ë' => Some("e"),
        'ì' | 'í' | 'î' | 'ï' | 'Ì' | 'Í' | 'Î' | 'Ï' => Some("i"),
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | 'Ø' => Some("o"),
        'ù' | 'ú' | 'û' | 'ü' | 'Ù' | 'Ú' | 'Û' | 'Ü' => Some("u"),
        'ý' | 'ÿ' | 'Ý' => Some("y"),
        'ñ' | 'Ñ' => Some("n"),
        'ç' | 'Ç' => Some("c"),
        'ß' => Some("ss"),
        'æ' | 'Æ' => Some("ae"),
        'œ' | 'Œ' => Some("oe"),
        _ => None, // pass through; step 2 will map non-ASCII to '-'
    }
}

// ---------------------------------------------------------------------------
// Marker helpers
// ---------------------------------------------------------------------------

const MARKER_PREFIX: &str = "<!-- lare-title: ";
const MARKER_SUFFIX: &str = " -->";

/// Build the first-line marker for a title.
///
/// The title is sanitized to prevent marker injection: we strip `-->` from the
/// title string (valid HTML comment syntax) and collapse newlines to spaces so
/// the marker always stays on the first line (un titolo multi-riga
/// corromperebbe il parsing al `open`).
fn make_marker(title: &str) -> String {
    let safe_title = title.replace("-->", "").replace(['\n', '\r'], " ");
    format!("{}{}{}", MARKER_PREFIX, safe_title, MARKER_SUFFIX)
}

/// Extract title from the first line of a saved file.
///
/// Returns `None` if the line does not match the expected marker format.
fn parse_marker(first_line: &str) -> Option<String> {
    let line = first_line.trim();
    if line.starts_with(MARKER_PREFIX) && line.ends_with(MARKER_SUFFIX) {
        let inner = &line[MARKER_PREFIX.len()..line.len() - MARKER_SUFFIX.len()];
        Some(inner.to_string())
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Security helpers
// ---------------------------------------------------------------------------

/// Rifiuta un filename che contiene caratteri di path traversal.
///
/// Restituisce `Err` se `file` contiene `/`, `\` o `..`; `Ok` altrimenti.
/// Rimane in uso solo per `save`, `open_find`, `delete_find` — dove il nome
/// è sempre semplice (nessun separatore di cartella è legittimo).
///
/// `open` e `delete` usano invece `validate_within_root`, che permette `/`
/// ma comunque blocca escape dalla root.
fn reject_traversal(file: &str) -> Result<(), String> {
    if file.contains('/') || file.contains('\\') || file.contains("..") {
        return Err(format!("invalid file name: {:?} (traversal not allowed)", file));
    }
    Ok(())
}

/// Guardia di sicurezza per i path che possono contenere sottocartelle.
///
/// Verifica che `rel_path`, risolto all'interno di `base`, rimanga dentro `base`.
/// Restituisce il `PathBuf` assoluto risolto, oppure `Err` se esce dalla root.
///
/// ## Perché NON usiamo `Path::canonicalize()`?
/// `canonicalize()` richiede che il path esista sul filesystem — fallisce
/// per le destinazioni di `create_folder`/`move_file` che non esistono ancora.
/// Soluzione: usiamo `path.components()` per iterare i segmenti del path e
/// ricostruirlo manualmente su `base`, senza mai toccare il filesystem.
///
/// ## Cosa viene rifiutato
/// - Qualsiasi backslash `\`: il separatore interno è sempre `/` (cross-platform).
/// - Componenti `..` (`Component::ParentDir`): risalita fuori dalla root.
/// - Path assoluti (`Component::RootDir`, `Component::Prefix`): già fuori dalla root.
///
/// L'errore restituito contiene sempre la parola `"traversal"` — questo permette
/// ai test di `open_rejects_dotdot_traversal` e `delete_rejects_dotdot_traversal`
/// di continuare a verificare il messaggio d'errore senza cambiare.
fn validate_within_root(base: &Path, rel_path: &str) -> Result<std::path::PathBuf, String> {
    use std::path::Component;

    // Rifiuta esplicitamente backslash prima ancora di fare il parsing.
    // Su Windows, `Path::new("foo\\bar").components()` li tratterebbe come
    // separatori — vogliamo rifiutarli a prescindere dalla piattaforma.
    if rel_path.contains('\\') {
        return Err(format!(
            "path traversal: {:?} contains backslash (use '/' as separator)",
            rel_path
        ));
    }

    // Costruiamo il path finale partendo da `base` e aggiungendo solo
    // i componenti Normal (nomi semplici di file/cartella).
    // Ogni altro tipo di componente è un tentativo di escape → errore.
    let mut result = base.to_path_buf();
    for component in Path::new(rel_path).components() {
        match component {
            // Componente normale (es. "Progetti", "doc.md") → aggiunge al path.
            Component::Normal(name) => result.push(name),
            // "." corrente: ignorato (innocuo).
            Component::CurDir => {}
            // ".." parent, path assoluti (RootDir, Prefix): tutti traversal.
            _ => {
                return Err(format!(
                    "path traversal: {:?} contains illegal path component",
                    rel_path
                ));
            }
        }
    }

    // Difesa in profondità: verifica che il risultato parta davvero con `base`.
    // In teoria già garantito dalla logica sopra, ma meglio verificare esplicitamente.
    if !result.starts_with(base) {
        return Err(format!(
            "path traversal: {:?} escapes the library root",
            rel_path
        ));
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// Archive operations
// ---------------------------------------------------------------------------

/// Save `content` as `{dir}/{slug}-{suffix}.md`.
///
/// The first line of the file is the marker `<!-- lare-title: <title> -->`.
/// The remaining lines are the original Markdown `content`.
/// Creates `dir` if it does not exist.
///
/// Returns the plain filename (no directory) of the created file, e.g.
/// `"hello-world-1718000000000.md"`.
///
/// # Errors
/// Returns an error string if the directory cannot be created or the file
/// cannot be written.
pub fn save(dir: &Path, title: &str, content: &str, suffix: &str) -> Result<String, String> {
    // Create directory if absent.
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("cannot create archive dir {:?}: {e}", dir))?;

    let filename = format!("{}-{}.md", slug(title), suffix);
    let path = dir.join(&filename);

    let marker = make_marker(title);
    // Format: marker \n content (no trailing newline forced, content may have its own)
    let file_content = format!("{}\n{}", marker, content);

    std::fs::write(&path, file_content)
        .map_err(|e| format!("cannot write archive file {:?}: {e}", path))?;

    Ok(filename)
}

/// List all `.md` files in `dir`, sorted by modification time (newest first).
///
/// For each file:
/// - `title`: extracted from the `<!-- lare-title: … -->` marker on the first line;
///   falls back to the file stem if the marker is absent or the file is unreadable.
/// - `file`: the plain filename (no directory component).
/// - `modified_ms`: file mtime as milliseconds since UNIX epoch (0 if unavailable).
///
/// If `dir` does not exist, returns an empty `Vec`.
pub fn list(dir: &Path) -> Vec<ArchiveEntry> {
    // Dir absent → empty list (not an error — archive may be unused).
    if !dir.exists() {
        return vec![];
    }

    let read_dir = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return vec![],
    };

    let mut entries: Vec<(u64, ArchiveEntry)> = Vec::new();

    for entry in read_dir.flatten() {
        let path = entry.path();

        // Only process `.md` files.
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }

        let filename = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        // Read modification time.
        let modified_ms = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        // Read first line to extract title.
        let title = read_title_from_file(&path).unwrap_or_else(|| {
            // Fallback: use filename stem.
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("untitled")
                .to_string()
        });

        entries.push((
            modified_ms,
            ArchiveEntry {
                title,
                file: filename,
                modified_ms,
            },
        ));
    }

    // Sort by modified_ms descending (newest first).
    entries.sort_by_key(|e| std::cmp::Reverse(e.0));

    entries.into_iter().map(|(_, e)| e).collect()
}

/// Read only the first line of a file to extract the title marker.
/// Returns `None` on any I/O error or if the marker is absent.
fn read_title_from_file(path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let first_line = content.lines().next()?;
    parse_marker(first_line)
}

/// Delete an archived file from `dir`.
///
/// # Security: path traversal prevention
/// `file` può essere un path relativo con sottocartelle (es. "Progetti/doc.md")
/// oppure un semplice filename (es. "doc.md"). La funzione usa `validate_within_root`
/// (non più `reject_traversal`) per permettere i path con `/` ma bloccare comunque
/// escape dalla root, path assoluti e componenti `..`.
///
/// # Errors
/// - `file` esce dalla root → `Err("path traversal: ...")`
/// - File non esiste o non rimovibile → `Err("cannot remove ...")`
pub fn delete(dir: &Path, file: &str) -> Result<(), String> {
    // validate_within_root: permette "/" (sottocartella), blocca ".." e assoluti.
    let path = validate_within_root(dir, file)?;
    std::fs::remove_file(&path)
        .map_err(|e| format!("cannot remove archive file {:?}: {e}", path))
}

/// Open an archived file and return its `ArchiveDoc`.
///
/// # Security: path traversal prevention
/// `file` può contenere sottocartelle (es. "Progetti/doc.md"): `validate_within_root`
/// permette i path con `/` ma blocca escape dalla root, path assoluti e `..`.
///
/// # Errors
/// - `file` esce dalla root → `Err("path traversal: ...")`
/// - File non leggibile → `Err("cannot read ...")`
pub fn open(dir: &Path, file: &str) -> Result<ArchiveDoc, String> {
    // ── Security: usa validate_within_root invece di reject_traversal ─────
    // reject_traversal vietava qualsiasi "/": ora "/" è legittimo come separatore
    // di sottocartella. validate_within_root blocca comunque "..", path assoluti
    // e backslash — l'invariante di sicurezza resta intatto.
    let path = validate_within_root(dir, file)?;

    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read archive file {:?}: {e}", path))?;

    // Split off the first line (marker) from the rest (content).
    let (first_line, content) = match raw.split_once('\n') {
        Some((fl, rest)) => (fl, rest.to_string()),
        None => {
            // File has only one line — treat it as all-marker, empty content.
            (raw.as_str(), String::new())
        }
    };

    let title = parse_marker(first_line)
        .unwrap_or_else(|| {
            // No marker: use the file stem as fallback title.
            Path::new(file)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("untitled")
                .to_string()
        });

    Ok(ArchiveDoc { title, content })
}

/// Sovrascrive un documento ESISTENTE in `dir`, mantenendo lo STESSO filename
/// (a differenza di `save`, che ne genera sempre uno nuovo con suffisso
/// timestamp). Usata dalla feature "espandi documento": il testo fuso
/// dall'AI rimpiazza il contenuto precedente sotto lo stesso rel-path.
///
/// # Security: path traversal prevention
/// Stessa guardia di `open`/`delete`: `validate_within_root` permette "/"
/// come separatore di sottocartella ma blocca "..", path assoluti e backslash.
///
/// # Errors
/// - `file` esce dalla root → `Err("path traversal: ...")`
/// - `file` non esiste già → `Err("...: file not found")` — `update` non crea
///   mai un file nuovo, quello è compito di `save`.
/// - Scrittura fallita → `Err("cannot write archive file ...")`
pub fn update(dir: &Path, file: &str, title: &str, content: &str) -> Result<(), String> {
    let path = validate_within_root(dir, file)?;

    if !path.exists() {
        return Err(format!("cannot update archive file {:?}: file not found", path));
    }

    let marker = make_marker(title);
    let file_content = format!("{}\n{}", marker, content);

    std::fs::write(&path, file_content)
        .map_err(|e| format!("cannot write archive file {:?}: {e}", path))
}

// ---------------------------------------------------------------------------
// Find operations
// ---------------------------------------------------------------------------

/// Save a Find session as `{dir}/find-{slug(query)}-{suffix}.json`.
///
/// The file contains an `ArchiveFind` serialized as pretty-printed JSON.
/// Creates `dir` if it does not exist.
///
/// Returns the plain filename (no directory) of the created file, e.g.
/// `"find--pdf-1718000000000.json"`.
///
/// # Errors
/// Returns an error string if the directory cannot be created or the file
/// cannot be written.
pub fn save_find(dir: &Path, query: &str, hits: &[ArchiveHit]) -> Result<String, String> {
    // Create directory if absent (same pattern as `save`).
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("cannot create find archive dir {:?}: {e}", dir))?;

    // Build a timestamp-based suffix so concurrent saves don't collide.
    let ts = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let filename = format!("find-{}-{}.json", slug(query), ts);
    let path = dir.join(&filename);

    let record = ArchiveFind {
        query: query.to_string(),
        ts,
        hits: hits.to_vec(),
    };

    let json = serde_json::to_string_pretty(&record)
        .map_err(|e| format!("cannot serialize ArchiveFind: {e}"))?;

    std::fs::write(&path, json)
        .map_err(|e| format!("cannot write find archive file {:?}: {e}", path))?;

    Ok(filename)
}

/// List all `.json` files in `dir`, sorted by modification time (newest first).
///
/// Each file is deserialized as `ArchiveFind` to extract `query` and `count`.
/// Files that cannot be parsed are silently skipped.
///
/// If `dir` does not exist, returns an empty `Vec`.
pub fn list_find(dir: &Path) -> Vec<FindEntry> {
    if !dir.exists() {
        return vec![];
    }

    let read_dir = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return vec![],
    };

    let mut entries: Vec<(u64, FindEntry)> = Vec::new();

    for entry in read_dir.flatten() {
        let path = entry.path();

        // Only process `.json` files.
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        let filename = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        // Read modification time.
        let modified = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        // Parse the JSON to extract query and hit count; skip on error.
        let raw = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let record: ArchiveFind = match serde_json::from_str(&raw) {
            Ok(r) => r,
            Err(_) => continue,
        };

        entries.push((
            modified,
            FindEntry {
                file: filename,
                query: record.query,
                count: record.hits.len(),
                modified,
            },
        ));
    }

    // Sort by mtime descending (newest first) — same pattern as `list`.
    entries.sort_by_key(|e| std::cmp::Reverse(e.0));

    entries.into_iter().map(|(_, e)| e).collect()
}

/// Open a saved Find session and return its `ArchiveFind`.
///
/// # Security: path traversal prevention
/// Same guard as `open` and `delete` (shared via `reject_traversal`): `file`
/// must be a plain filename with no directory separators (`/`, `\`) and must
/// not contain `..`.
///
/// # Errors
/// - `file` fails the traversal check → `Err("invalid file name: ...")`
/// - File does not exist or cannot be read → `Err("cannot read ...")`
/// - File cannot be parsed as `ArchiveFind` → `Err("cannot parse ...")`
pub fn open_find(dir: &Path, file: &str) -> Result<ArchiveFind, String> {
    reject_traversal(file)?;

    let path = dir.join(file);

    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read find archive file {:?}: {e}", path))?;

    serde_json::from_str(&raw)
        .map_err(|e| format!("cannot parse find archive file {:?}: {e}", path))
}

/// Delete a saved Find session from `dir`.
///
/// # Security: path traversal prevention
/// Same guard as `open_find` (shared via `reject_traversal`).
///
/// # Errors
/// - `file` fails the traversal check → `Err("invalid file name: ...")`
/// - File does not exist or cannot be removed → `Err("cannot remove ...")`
pub fn delete_find(dir: &Path, file: &str) -> Result<(), String> {
    reject_traversal(file)?;

    let path = dir.join(file);
    std::fs::remove_file(&path)
        .map_err(|e| format!("cannot remove find archive file {:?}: {e}", path))
}

// ---------------------------------------------------------------------------
// Library folder operations (Slice 1 — backend)
// ---------------------------------------------------------------------------

/// Restituisce l'albero completo della Library come `LibraryTree`.
///
/// Esegue una visita DFS con limite di profondità (MAX_DEPTH = 10) per evitare
/// loop infiniti su symlink o strutture patologicamente annidate.
///
/// Questa funzione è chiamata da `main.rs` con `documents_dir_path` come `dir`:
/// opera sulla cartella `library/documents/` e non vede mai `library/find/`
/// (separata nella root del repository). Non vi è alcun filtro hardcoded:
/// tutte le sotto-cartelle e i file `.md` presenti in `dir` sono inclusi.
///
/// Il campo `file` di ogni `ArchiveEntry` prodotto da questa funzione è un
/// path relativo a `dir` con separatori `/`, es. "Progetti/doc.md".
/// I file nella root di `dir` hanno invece solo il filename (es. "doc.md").
pub fn list_tree(dir: &Path) -> LibraryTree {
    if !dir.exists() {
        return LibraryTree {
            root_files: vec![],
            folders: vec![],
        };
    }

    // Raccoglie il contenuto della directory `dir` (che punta a `documents/`):
    // file `.md` in root + cartelle di primo livello.
    // Non c'è nessuna logica di filtraggio hardcoded: `collect_node_contents` è generica.
    let (root_files, folders) = collect_node_contents(dir, "", 0);

    LibraryTree { root_files, folders }
}

/// Raccoglie ricorsivamente i contenuti di una directory.
///
/// ## Parametri
/// - `dir`: path assoluto della directory corrente.
/// - `rel_prefix`: path relativo alla root Library della directory corrente,
///   con separatori `/`. Stringa vuota per la root.
/// - `depth`: profondità corrente. Limite MAX_DEPTH per evitare ricorsione illimitata.
///
/// ## Ritorno
/// Coppia `(files, folders)` dove:
/// - `files`: i file `.md` trovati in questa directory.
/// - `folders`: le sotto-cartelle (ognuna con il proprio sottoalbero).
fn collect_node_contents(
    dir: &Path,
    rel_prefix: &str,
    depth: u32,
) -> (Vec<ArchiveEntry>, Vec<LibraryNode>) {
    /// Profondità massima di ricorsione: protegge da symlink ciclici e nesting
    /// patologico. 10 livelli sono abbondanti per qualsiasi Library reale.
    const MAX_DEPTH: u32 = 10;

    if depth > MAX_DEPTH {
        return (vec![], vec![]);
    }

    let read_dir = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        // Directory non leggibile (permessi, ecc.) → skip silenzioso.
        Err(_) => return (vec![], vec![]),
    };

    let mut files: Vec<ArchiveEntry> = vec![];
    let mut folders: Vec<LibraryNode> = vec![];

    for entry in read_dir.flatten() {
        let path = entry.path();
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        if path.is_dir() {
            // Costruisce il rel_path di questa sotto-cartella.
            // Esempio: se rel_prefix = "Progetti" e name = "Web" → "Progetti/Web".
            let child_rel = if rel_prefix.is_empty() {
                name.clone()
            } else {
                format!("{}/{}", rel_prefix, name)
            };

            // Ricorsione con profondità incrementata.
            let (child_files, child_folders) = collect_node_contents(&path, &child_rel, depth + 1);

            folders.push(LibraryNode {
                name,
                rel_path: child_rel,
                folders: child_folders,
                files: child_files,
            });
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            // File .md: costruisce il path relativo con "/".
            let file_rel = if rel_prefix.is_empty() {
                // In root: filename semplice.
                name
            } else {
                // In sotto-cartella: "PrefixPath/filename.md".
                format!("{}/{}", rel_prefix, name)
            };

            let modified_ms = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);

            let title = read_title_from_file(&path).unwrap_or_else(|| {
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("untitled")
                    .to_string()
            });

            files.push(ArchiveEntry {
                title,
                file: file_rel,
                modified_ms,
            });
        }
        // Tutti gli altri file (non-.md) vengono ignorati silenziosamente.
    }

    // Ordina i file per mtime desc (più recente prima) — coerente con `list`.
    files.sort_by_key(|e| std::cmp::Reverse(e.modified_ms));

    (files, folders)
}

/// Crea una nuova cartella all'interno della Library.
///
/// ## Parametri
/// - `dir`: root della Library.
/// - `parent_rel`: path relativo della cartella padre ("" = root).
/// - `name`: nome semplice della nuova cartella (no `/`, no `..`).
///
/// ## Collisioni
/// Se esiste già una cartella con quel nome, aggiunge un suffisso numerico:
/// "Nome" → "Nome 2" → "Nome 3" → … (comportamento standard degli OS).
///
/// ## Ritorno
/// Il path relativo della cartella creata (es. "Progetti/Nuova 2").
pub fn create_folder(dir: &Path, parent_rel: &str, name: &str) -> Result<String, String> {
    // Valida il nome: deve essere un nome semplice (niente separatori, niente "..").
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(format!("invalid folder name: {:?} (must be a simple name)", name));
    }

    // Risolve il parent in un path assoluto.
    // "" = root Library; altrimenti validate_within_root garantisce che resti inside.
    let parent_abs = if parent_rel.is_empty() {
        dir.to_path_buf()
    } else {
        validate_within_root(dir, parent_rel)?
    };

    if !parent_abs.exists() {
        return Err(format!("parent folder not found: {:?}", parent_rel));
    }

    // Loop di collisione: tenta "Nome", "Nome 2", "Nome 3", …
    let mut candidate_name = name.to_string();
    let mut counter = 2u32;
    loop {
        let candidate_path = parent_abs.join(&candidate_name);
        if !candidate_path.exists() {
            std::fs::create_dir(&candidate_path)
                .map_err(|e| format!("cannot create folder {:?}: {e}", candidate_path))?;

            // Restituisce il rel_path della cartella appena creata.
            let created_rel = if parent_rel.is_empty() {
                candidate_name
            } else {
                format!("{}/{}", parent_rel, candidate_name)
            };
            return Ok(created_rel);
        }

        // Nome occupato → prova con suffisso numerico.
        candidate_name = format!("{} {}", name, counter);
        counter += 1;

        if counter > 1000 {
            return Err(format!("too many folders named {:?}: all suffix slots occupied", name));
        }
    }
}

/// Rinomina una cartella nella Library.
///
/// ## Parametri
/// - `dir`: root della Library.
/// - `folder_rel`: path relativo della cartella da rinominare (es. "Progetti").
/// - `new_name`: nuovo nome semplice (no `/`, no `..`, no `\`).
pub fn rename_folder(dir: &Path, folder_rel: &str, new_name: &str) -> Result<(), String> {
    // Guardia: folder_rel="" punta alla radice della Library — non rinominabile.
    // Scatta prima di qualsiasi validazione del nome o accesso al filesystem.
    if folder_rel.is_empty() {
        return Err(
            "folder_rel non può essere vuoto: impossibile rinominare la cartella radice".into(),
        );
    }

    // Valida il nuovo nome (deve essere semplice, non un path).
    if new_name.is_empty()
        || new_name.contains('/')
        || new_name.contains('\\')
        || new_name.contains("..")
    {
        return Err(format!("invalid folder name: {:?} (must be a simple name)", new_name));
    }

    // Risolve il path assoluto della cartella sorgente.
    let folder_abs = validate_within_root(dir, folder_rel)?;

    if !folder_abs.exists() {
        return Err(format!("folder not found: {:?}", folder_rel));
    }

    // Il parent è la directory che contiene la cartella: ottiene il path del nuovo nome.
    let parent = folder_abs
        .parent()
        .ok_or_else(|| format!("cannot determine parent of {:?}", folder_abs))?;
    let new_path = parent.join(new_name);

    // Guardia collisione: rifiuta esplicitamente se esiste già una cartella
    // con il nome di destinazione. Senza questo controllo, `fs::rename`
    // su Windows restituisce un errore OS generico; qui forniamo un messaggio
    // chiaro e non ambiguo per l'utente.
    if new_path.exists() {
        return Err(format!(
            "esiste già una cartella con quel nome nella destinazione: {:?}",
            new_name
        ));
    }

    std::fs::rename(&folder_abs, &new_path)
        .map_err(|e| format!("cannot rename folder {:?}: {e}", folder_abs))
}

/// Sposta un file `.md` da una posizione a un'altra nella Library.
///
/// ## Parametri
/// - `dir`: root della Library.
/// - `file_rel`: path relativo del file sorgente (es. "Progetti/doc.md").
/// - `target_folder_rel`: path relativo della cartella destinazione ("" = root).
///
/// Il nome del file è preservato. Se il file destinazione esiste già, viene
/// sovrascritto (`std::fs::rename` ha questo comportamento su tutti i SO).
pub fn move_file(dir: &Path, file_rel: &str, target_folder_rel: &str) -> Result<(), String> {
    // Guardia: file_rel="" punta alla radice — non è un file e non può essere spostata.
    if file_rel.is_empty() {
        return Err("file_rel non può essere vuoto".into());
    }

    // Risolve il path assoluto del file sorgente.
    let src_abs = validate_within_root(dir, file_rel)?;

    if !src_abs.exists() {
        return Err(format!("file not found: {:?}", file_rel));
    }

    // Guardia tipo: rifiuta le directory — questa funzione sposta solo file.
    // Il controllo scatta dopo exists() per distinguere "non trovato" da "tipo sbagliato".
    if !src_abs.is_file() {
        return Err(format!("la sorgente non è un file: {:?}", file_rel));
    }

    // Risolve la cartella destinazione.
    let target_dir_abs = if target_folder_rel.is_empty() {
        // "" = root Library.
        dir.to_path_buf()
    } else {
        validate_within_root(dir, target_folder_rel)?
    };

    // Il nome del file sorgente è preservato nella destinazione.
    let filename = src_abs
        .file_name()
        .ok_or_else(|| format!("cannot determine filename of {:?}", src_abs))?;
    let dest_abs = target_dir_abs.join(filename);

    // Hardening v0.27.0: rifiutiamo lo spostamento se nella destinazione esiste già
    // un documento con lo stesso nome. `std::fs::rename` sovrascrive silenziosamente
    // su Windows e Linux — questo controllo previene la perdita di dati.
    // (Race condition TOCTOU teorica, ma accettabile in un uso monoutente locale.)
    if dest_abs.exists() {
        return Err(format!(
            "a destinazione esiste già un documento con quel nome: {:?}",
            filename
        ));
    }

    // std::fs::rename sposta il file atomicamente (stesso filesystem).
    // Se src e dest sono su filesystem diversi, fallisce → restituisce Err.
    std::fs::rename(&src_abs, &dest_abs)
        .map_err(|e| format!("cannot move file {:?}: {e}", src_abs))
}

/// Elimina una cartella SOLO se vuota.
///
/// Il filesystem è la FONTE DI VERITÀ: controlla i contenuti reali (anche file
/// non-.md lasciati dall'utente via Explorer), non l'albero di `list_tree`.
/// Rifiuta se la cartella non è vuota, non è trovata, o se il path è un traversal.
///
/// # Parametri
/// - `dir`: root assoluta della Library (cartella base).
/// - `folder_rel`: path relativo della cartella da eliminare (es. "Progetti").
///
/// # Sicurezza
/// Usa `validate_within_root` → blocca path con ".." prima di toccare il fs.
/// Usa `std::fs::remove_dir` (NON `remove_dir_all`) → il SO stesso rifiuta se non vuota.
pub fn delete_folder(dir: &Path, folder_rel: &str) -> Result<(), String> {
    // 0. Guardia: folder_rel="" punta alla radice della Library — non eliminabile.
    //    Scatta prima di `validate_within_root` per evitare che la root stessa
    //    venga risolta e poi passata a `remove_dir`.
    if folder_rel.is_empty() {
        return Err(
            "folder_rel non può essere vuoto: impossibile eliminare la cartella radice".into(),
        );
    }

    // 1. Risolve il path assoluto e blocca qualsiasi tentativo di traversal
    //    (es. "../fuori-dalla-library"). `validate_within_root` restituisce Err
    //    se il path contiene ".." o componenti che uscirebbero dalla root.
    let path = validate_within_root(dir, folder_rel)?;

    // 2. La cartella deve esistere sul filesystem.
    if !path.exists() {
        return Err(format!("folder not found: {:?}", folder_rel));
    }

    // 3. Il filesystem è la FONTE DI VERITÀ: controlliamo i contenuti reali,
    //    non l'albero in memoria di `list_tree`. Anche un singolo file .txt
    //    (invisibile nella Library UI) blocca la cancellazione.
    //
    //    `read_dir().next()` legge solo il primo entry — O(1) sul numero di file.
    //    Se `next()` restituisce `Some(...)` la cartella non è vuota → Err.
    let is_empty = std::fs::read_dir(&path)
        .map_err(|e| format!("cannot read folder {:?}: {e}", folder_rel))?
        .next()
        .is_none();

    if !is_empty {
        return Err(format!("folder not empty: {:?}", folder_rel));
    }

    // 4. Elimina la cartella. `remove_dir` (NON `remove_dir_all`) rifiuta a
    //    livello SO se la cartella non è vuota — doppia rete di sicurezza contro
    //    race conditions TOCTOU tra il controllo al passo 3 e questa chiamata.
    std::fs::remove_dir(&path)
        .map_err(|e| format!("cannot delete folder {:?}: {e}", folder_rel))
}

// ---------------------------------------------------------------------------
// Sposta cartella (ui v0.27.0)
// ---------------------------------------------------------------------------

/// Sposta una cartella `folder_rel` dentro la cartella `target_parent_rel`
/// ("" = radice documents). Il nome della cartella è preservato.
///
/// # Errori
/// - traversal (`..`, path assoluti, `\`) su `folder_rel` o `target_parent_rel`
/// - `folder_rel` vuoto (impossibile spostare la radice stessa)
/// - la cartella sorgente non esiste o non è una directory
/// - `target_parent_rel` non esiste o non è una directory
/// - **anti-ciclo**: non si può spostare una cartella in sé stessa ("A" → "A")
///   o in una propria discendente ("A" → "A/B") — questo creerebbe un loop nel
///   filesystem e renderebbe la struttura inaccessibile
/// - **collisione**: nella destinazione esiste già una entry con lo stesso nome
///   → Err (niente merge; per le cartelle un merge accidentale è pericoloso)
///
/// # Implementazione
/// Usa `std::fs::rename`, che su Windows è atomico (MoveFileEx) sullo stesso
/// volume. Se sorgente e destinazione sono su volumi diversi, `rename` fallirà
/// con un errore OS — accettabile perché la Library è sempre sullo stesso disco.
pub fn move_folder(dir: &Path, folder_rel: &str, target_parent_rel: &str) -> Result<(), String> {
    // 1. Rifiutiamo immediatamente un folder_rel vuoto: sarebbe la radice stessa.
    //    Non ha senso "spostare la libreria" e causerebbe un rename sulla dir root.
    if folder_rel.is_empty() {
        return Err("folder_rel non può essere vuoto: impossibile spostare la cartella radice".into());
    }

    // 2. Risolve il path assoluto della cartella sorgente.
    //    validate_within_root blocca "..", path assoluti e backslash.
    let src_abs = validate_within_root(dir, folder_rel)?;

    // 3. La sorgente deve esistere ed essere una directory.
    if !src_abs.exists() {
        return Err(format!("cartella sorgente non trovata: {:?}", folder_rel));
    }
    if !src_abs.is_dir() {
        return Err(format!("la sorgente non è una cartella: {:?}", folder_rel));
    }

    // 4. Risolve il path assoluto della cartella padre di destinazione.
    //    "" = radice documents (dir stesso), altrimenti validate_within_root.
    let target_dir_abs = if target_parent_rel.is_empty() {
        dir.to_path_buf()
    } else {
        validate_within_root(dir, target_parent_rel)?
    };

    // 5. Guardia anti-ciclo sulle rel-path normalizzate.
    //    Confrontiamo le stringhe dirette (i path in ingresso sono già normalizzati
    //    con "/" da validate_within_root; i backslash vengono rifiutati al passo 2/4).
    //
    //    Caso 1: target == folder (es. "A" → "A"): si creerebbe "A/A" → ciclo.
    //    Caso 2: target inizia con folder + "/" (es. "A" → "A/B"): B è discendente
    //            di A → spostare A dentro B lascerebbe A dentro sé stesso → ciclo.
    let cycle = target_parent_rel == folder_rel
        || target_parent_rel.starts_with(&format!("{folder_rel}/"));
    if cycle {
        return Err(format!(
            "anti-ciclo: non si può spostare la cartella {:?} in sé stessa o in una sua discendente {:?}",
            folder_rel, target_parent_rel
        ));
    }

    // 6. Il parent di destinazione deve esistere ed essere una directory.
    //    (La root "dir" esiste sempre; questa verifica vale solo per il ramo
    //    non-vuoto di target_parent_rel.)
    if !target_dir_abs.exists() {
        return Err(format!("cartella padre di destinazione non trovata: {:?}", target_parent_rel));
    }
    if !target_dir_abs.is_dir() {
        return Err(format!("il padre di destinazione non è una cartella: {:?}", target_parent_rel));
    }

    // 7. Calcola il path di destinazione finale: parent_dest / nome_cartella.
    //    Il nome della cartella è l'ultimo segmento di folder_rel.
    let folder_name = src_abs
        .file_name()
        .ok_or_else(|| format!("impossibile determinare il nome della cartella: {:?}", src_abs))?;
    let dest_abs = target_dir_abs.join(folder_name);

    // 8. Controllo collisione: rifiutiamo se esiste già una entry con quel nome.
    //    Per le cartelle, un merge accidentale sarebbe pericoloso (perdita di file
    //    nascosta se i nomi dei file dentro coincidono). → Err, nessun merge.
    if dest_abs.exists() {
        return Err(format!(
            "nella destinazione esiste già una cartella con quel nome: {:?}",
            folder_name
        ));
    }

    // 9. Sposta la cartella. Su Windows usa MoveFileEx (atomico sullo stesso volume).
    //    Su filesystem diversi restituirà un errore OS — accettabile, la Library
    //    è sempre sullo stesso disco dell'app.
    std::fs::rename(&src_abs, &dest_abs)
        .map_err(|e| format!("impossibile spostare la cartella {:?}: {e}", src_abs))
}

// ---------------------------------------------------------------------------
// Migrazione layout: library/ → library/documents/ (ui v0.26.0)
// ---------------------------------------------------------------------------

/// Migra il vecchio layout (file `.md` sciolti in `library/` root) al nuovo
/// (`library/documents/`). La funzione è **idempotente** e **senza perdita dati**.
///
/// ## Cosa fa
/// 1. Crea `library/documents/` sempre (anche se la root è vuota): serve per
///    fresh-install, per il fs-watch e per il primo save documento.
/// 2. Sposta in `documents/` ogni entry della root TRANNE `find` e `documents`
///    (file `.md` e cartelle utente create dalla feature "Library a cartelle").
/// 3. Su collisione di nome in `documents/`: aggiunge un **suffisso numerico**
///    per rendere il nome unico, senza mai sovrascrivere o saltare file.
///    - File: suffisso prima dell'estensione → "foo.md" → "foo 2.md"
///    - Cartelle: suffisso in coda → "Bar" → "Bar 2"
/// 4. **Idempotente**: alla seconda esecuzione la root ha solo `find/` e
///    `documents/` → il filtro le salta entrambe → no-op senza errori.
///
/// ## Sicurezza atomicità (Windows)
/// Usa `std::fs::rename` che su Windows è un'operazione atomica (MoveFileEx)
/// sullo stesso volume. Nessuna copia intermedia: il file non è mai perduto
/// tra sorgente e destinazione.
///
/// ## Implementazione a due fasi
/// Prima raccoglie tutte le entry in un `Vec`, chiude l'iteratore `read_dir`,
/// poi esegue i rename. Questo evita il problema di Windows dove modificare
/// una directory mentre il suo handle di lettura è aperto può causare
/// `os error 5` (Accesso negato) o entry saltate.
///
/// ## Errori
/// In caso di errore (es. rename fallisce), ritorna `Err(descrizione)`.
/// La creazione di `documents/` è l'unica operazione non best-effort: se
/// fallisce l'intero processo è bloccato (senza documents/ non si può salvare).
pub fn migrate_to_documents_layout(library_dir: &Path) -> Result<(), String> {
    let documents_dir = library_dir.join("documents");

    // Passo 1: crea sempre `documents/` (anche se non c'è nulla da spostare).
    // `create_dir_all` è idempotente: se la cartella esiste già, non fa nulla.
    std::fs::create_dir_all(&documents_dir)
        .map_err(|e| format!("cannot create documents dir {:?}: {e}", documents_dir))?;

    // Passo 2 — FASE 1: raccoglie le entry da spostare in un Vec,
    // poi chiude l'iteratore (drop implicito) prima di fare qualsiasi rename.
    // Motivazione: su Windows, modificare una directory mentre il suo handle
    // di read_dir è ancora aperto può provocare `os error 5` (Accesso negato)
    // o entry saltate. Due fasi (collect → rename) eliminano il problema.
    let entries_to_move: Vec<std::fs::DirEntry> = std::fs::read_dir(library_dir)
        .map_err(|e| format!("cannot read library dir {:?}: {e}", library_dir))?
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            // Salta "find" (riservata al /find interno) e "documents" (la destinazione).
            // Tutto il resto (file .md, cartelle utente) viene spostato in documents/.
            name_str != "find" && name_str != "documents"
        })
        .collect();
    // L'iteratore read_dir è droppato qui — il handle sulla directory è chiuso.

    // Passo 2 — FASE 2: sposta ogni entry raccolta in `documents/`.
    for entry in entries_to_move {
        let src = entry.path();
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();
        let is_dir = src.is_dir();

        // Calcola un nome di destinazione unico in `documents/`.
        // Se esiste già una entry con lo stesso nome, aggiunge suffisso numerico.
        let dest = unique_name_in(&documents_dir, &name_str, is_dir);

        std::fs::rename(&src, &dest)
            .map_err(|e| format!("cannot move {:?} → {:?}: {e}", src, dest))?;
    }

    Ok(())
}

/// Calcola un `PathBuf` di destinazione unico in `target_dir` per un'entry di nome `name`.
///
/// ## Algoritmo
/// - Se `target_dir/name` non esiste → restituisce `target_dir/name` (nessun suffisso).
/// - Altrimenti incrementa un contatore a partire da 2, inserendo il suffisso:
///   - **File** (con estensione): prima dell'estensione → `"foo.md"` → `"foo 2.md"`
///   - **Cartelle** o file senza estensione: in coda → `"Bar"` → `"Bar 2"`
/// - Limite di 1000 tentativi (anti-loop di sicurezza — in pratica irraggiungibile).
///
/// ## Nota
/// Questa funzione non crea il file/cartella — restituisce solo il path calcolato.
/// La creazione avviene con `std::fs::rename` nel chiamante (`migrate_to_documents_layout`).
fn unique_name_in(target_dir: &Path, name: &str, is_dir: bool) -> std::path::PathBuf {
    // Caso semplice: il nome non è in conflitto.
    let candidate = target_dir.join(name);
    if !candidate.exists() {
        return candidate;
    }

    // Separa stem ed estensione per costruire il suffisso nel posto giusto.
    // Per le cartelle (is_dir=true) trattiamo l'intero nome come "stem" senza estensione.
    let (stem, ext) = if !is_dir {
        let p = std::path::Path::new(name);
        let s = p
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(name)
            .to_string();
        let e = p
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{e}"))
            .unwrap_or_default();
        (s, e)
    } else {
        // Cartella: nessuna estensione, il suffisso va in coda al nome intero.
        (name.to_string(), String::new())
    };

    // Ciclo suffisso: 2, 3, 4, ... fino a trovare uno slot libero.
    let mut counter = 2u32;
    loop {
        let candidate_name = format!("{stem} {counter}{ext}");
        let candidate = target_dir.join(&candidate_name);
        if !candidate.exists() {
            return candidate;
        }
        counter += 1;
        // Limite anti-loop: in pratica non raggiungibile in uso normale.
        if counter > 1000 {
            // Fallback emergenza: usa il timestamp come suffisso univoco.
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            return target_dir.join(format!("{stem} {ts}{ext}"));
        }
    }
}

// ---------------------------------------------------------------------------
// Tests (TDD — written before the final implementation)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};
    use tempfile::tempdir;

    // ── slug ─────────────────────────────────────────────────────────────

    #[test]
    fn slug_converts_spaces_to_dashes() {
        assert_eq!(slug("hello world"), "hello-world");
    }

    #[test]
    fn slug_lowercases_input() {
        assert_eq!(slug("Hello World"), "hello-world");
    }

    #[test]
    fn slug_converts_accented_chars() {
        // é → e, à → a, etc.
        assert_eq!(slug("résumé"), "resume");
        assert_eq!(slug("naïve"), "naive");
        assert_eq!(slug("città"), "citta");
    }

    #[test]
    fn slug_strips_punctuation() {
        assert_eq!(slug("hello, world!"), "hello-world");
        assert_eq!(slug("test: something (here)"), "test-something-here");
    }

    #[test]
    fn slug_collapses_consecutive_dashes() {
        // Multiple spaces or punctuation → single dash.
        assert_eq!(slug("hello   world"), "hello-world");
        assert_eq!(slug("a--b"), "a-b");
    }

    #[test]
    fn slug_trims_leading_trailing_dashes() {
        assert_eq!(slug("--hello--"), "hello");
        assert_eq!(slug("  hello  "), "hello");
    }

    #[test]
    fn slug_truncates_at_40_chars() {
        let long_title = "abcdefghij".repeat(5); // 50 chars
        let result = slug(&long_title);
        assert!(
            result.len() <= 40,
            "slug must be <= 40 chars, got {} chars: {:?}",
            result.len(),
            result
        );
        // Must not end with a dash.
        assert!(
            !result.ends_with('-'),
            "slug must not end with '-': {:?}",
            result
        );
    }

    #[test]
    fn slug_truncation_does_not_end_with_dash() {
        // A title where the 40th char is in the middle of a "-" run.
        // 38 a's + "-- extra"
        let title = format!("{}----- extra content", "a".repeat(38));
        let result = slug(&title);
        assert!(result.len() <= 40, "slug too long: {:?}", result);
        assert!(
            !result.ends_with('-'),
            "slug ends with dash: {:?}",
            result
        );
    }

    #[test]
    fn slug_empty_input_returns_untitled() {
        assert_eq!(slug(""), "untitled");
    }

    #[test]
    fn slug_all_punctuation_returns_untitled() {
        assert_eq!(slug("!!!---???"), "untitled");
    }

    // ── round-trip: save → list → open ───────────────────────────────────

    #[test]
    fn round_trip_title_and_content_preserved() {
        let dir = tempdir().expect("tempdir");
        let title = "My Test Window";
        let content = "# Hello\n\nThis is *markdown*.";
        let suffix = "12345";

        let filename = save(dir.path(), title, content, suffix).expect("save");

        // list: should find one entry with matching title.
        let entries = list(dir.path());
        assert_eq!(entries.len(), 1, "expected 1 entry, got {}", entries.len());
        assert_eq!(entries[0].title, title);
        assert_eq!(entries[0].file, filename);

        // open: should return exact title and content.
        let doc = open(dir.path(), &filename).expect("open");
        assert_eq!(doc.title, title, "title must be preserved exactly");
        assert_eq!(
            doc.content, content,
            "content must be preserved exactly (marker line not included)"
        );
    }

    #[test]
    fn round_trip_content_without_marker_line() {
        let dir = tempdir().expect("tempdir");
        let content = "line one\nline two\nline three";
        let filename = save(dir.path(), "Test", content, "99").expect("save");
        let doc = open(dir.path(), &filename).expect("open");
        // The content returned must NOT include the marker line.
        assert_eq!(doc.content, content);
        assert!(
            !doc.content.contains("lare-title"),
            "marker must not appear in returned content"
        );
    }

    // ── list on absent dir → empty ────────────────────────────────────────

    #[test]
    fn list_absent_dir_returns_empty() {
        let dir = tempdir().expect("tempdir");
        let absent = dir.path().join("no_such_dir");
        let entries = list(&absent);
        assert!(
            entries.is_empty(),
            "expected empty list for absent dir, got {:?}",
            entries
        );
    }

    // ── open path traversal guard ─────────────────────────────────────────

    #[test]
    fn open_rejects_dotdot_traversal() {
        let dir = tempdir().expect("tempdir");
        let result = open(dir.path(), "../config.json");
        assert!(result.is_err(), "expected Err for '../config.json'");
        let msg = result.unwrap_err();
        assert!(
            msg.contains("traversal"),
            "error must mention traversal, got: {:?}",
            msg
        );
    }

    // ── delete: happy path and error cases ───────────────────────────────

    #[test]
    fn delete_removes_file_from_archive() {
        // RED target: save → delete → list should be empty.
        let dir = tempdir().expect("tempdir");
        let filename = save(dir.path(), "To Delete", "content", "del1").expect("save");

        // File must exist before delete.
        assert!(dir.path().join(&filename).exists(), "file must exist before delete");

        // Delete it.
        delete(dir.path(), &filename).expect("delete should succeed");

        // File must be gone.
        assert!(!dir.path().join(&filename).exists(), "file must not exist after delete");

        // list should now be empty.
        let entries = list(dir.path());
        assert!(entries.is_empty(), "list should be empty after delete, got: {:?}", entries);
    }

    #[test]
    fn delete_nonexistent_file_returns_err() {
        let dir = tempdir().expect("tempdir");
        let result = delete(dir.path(), "does-not-exist.md");
        assert!(result.is_err(), "deleting a nonexistent file should return Err");
    }

    #[test]
    fn delete_rejects_dotdot_traversal() {
        let dir = tempdir().expect("tempdir");
        let result = delete(dir.path(), "../sensitive.md");
        assert!(result.is_err(), "expected Err for '../sensitive.md'");
        let msg = result.unwrap_err();
        assert!(
            msg.contains("traversal"),
            "error must mention traversal, got: {:?}",
            msg
        );
    }

    // ── list sorts by modified descending ─────────────────────────────────

    #[test]
    fn list_sorts_by_modified_desc() {
        let dir = tempdir().expect("tempdir");

        // Save first file.
        let f1 = save(dir.path(), "First", "content1", "001").expect("save f1");

        // Set mtime of f1 to 10 seconds ago so f2 (saved after) is definitely newer.
        let path1 = dir.path().join(&f1);
        let older_time = SystemTime::now() - Duration::from_secs(10);
        set_file_mtime(&path1, older_time);

        // Save second file (after mtime manipulation → it will have NOW mtime).
        let f2 = save(dir.path(), "Second", "content2", "002").expect("save f2");

        let entries = list(dir.path());
        assert_eq!(entries.len(), 2, "expected 2 entries");
        // f2 is newer → should be first.
        assert_eq!(
            entries[0].file, f2,
            "newest file must be first; got {:?}",
            entries.iter().map(|e| &e.file).collect::<Vec<_>>()
        );
        assert_eq!(entries[1].file, f1, "oldest file must be last");
    }

    /// Helper: set the mtime of a file to the given time.
    /// Uses `std::fs::File::set_modified` (stable since Rust 1.75).
    fn set_file_mtime(path: &Path, time: SystemTime) {
        // Open the file and set its modification time.
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .expect("open file for mtime set");
        file.set_modified(time).expect("set_modified");
    }

    // ── Find round-trip ───────────────────────────────────────────────────

    /// Build a test ArchiveHit helper.
    fn hit(path: &str, source: &str) -> ArchiveHit {
        ArchiveHit {
            path: path.to_string(),
            source: source.to_string(),
        }
    }

    #[test]
    fn find_round_trip_save_list_open_delete() {
        let dir = tempdir().expect("tempdir");

        // save_find returns a filename.
        let file = save_find(
            dir.path(),
            "*.pdf",
            &[hit("C:/a/x.pdf", "cwd"), hit("D:/y.pdf", "external")],
        )
        .expect("save_find");

        // list_find: 1 entry with matching query and count.
        let entries = list_find(dir.path());
        assert_eq!(entries.len(), 1, "expected 1 FindEntry, got {}", entries.len());
        assert_eq!(entries[0].query, "*.pdf", "query mismatch");
        assert_eq!(entries[0].count, 2, "count must match number of hits");
        assert_eq!(entries[0].file, file, "file name mismatch");

        // open_find: hits must be identical to what was saved.
        let found = open_find(dir.path(), &file).expect("open_find");
        assert_eq!(found.query, "*.pdf");
        assert_eq!(found.hits.len(), 2);
        assert_eq!(found.hits[0].path, "C:/a/x.pdf");
        assert_eq!(found.hits[0].source, "cwd");
        assert_eq!(found.hits[1].path, "D:/y.pdf");
        assert_eq!(found.hits[1].source, "external");

        // delete_find: after deletion the list is empty.
        delete_find(dir.path(), &file).expect("delete_find");
        let after = list_find(dir.path());
        assert!(after.is_empty(), "list_find must be empty after delete_find");
    }

    #[test]
    fn find_open_rejects_dotdot_traversal() {
        let dir = tempdir().expect("tempdir");
        let result = open_find(dir.path(), "../x.json");
        assert!(result.is_err(), "open_find must reject '../x.json'");
        let msg = result.unwrap_err();
        assert!(
            msg.contains("traversal"),
            "error must mention traversal, got: {:?}",
            msg
        );
    }

    #[test]
    fn find_open_rejects_subdir_path() {
        let dir = tempdir().expect("tempdir");
        let result = open_find(dir.path(), "sub/x.json");
        assert!(result.is_err(), "open_find must reject 'sub/x.json'");
    }

    #[test]
    fn find_delete_rejects_dotdot_traversal() {
        let dir = tempdir().expect("tempdir");
        let result = delete_find(dir.path(), "../x.json");
        assert!(result.is_err(), "delete_find must reject '../x.json'");
        let msg = result.unwrap_err();
        assert!(
            msg.contains("traversal"),
            "error must mention traversal, got: {:?}",
            msg
        );
    }

    #[test]
    fn find_delete_rejects_subdir_path() {
        let dir = tempdir().expect("tempdir");
        let result = delete_find(dir.path(), "sub/x.json");
        assert!(result.is_err(), "delete_find must reject 'sub/x.json'");
    }

    // ── Riconciliazione open/delete con path di sottocartella ────────────
    // Questi due test sono il RED genuino: con il vecchio reject_traversal
    // (che vietava qualsiasi `/`) falliscono perché "Progetti/doc.md" contiene
    // un `/`. Una volta cambiato open/delete a usare validate_within_root,
    // i path con sottocartella diventano legittimi e i test passano (GREEN).

    #[test]
    fn archive_open_with_subfolder_path() {
        // Dato: un file .md dentro una sottocartella.
        let dir = tempdir().expect("tempdir");
        let subfolder = dir.path().join("Progetti");
        std::fs::create_dir(&subfolder).expect("create subfolder");

        // Scriviamo il file con il marker di titolo standard.
        let file_path = subfolder.join("doc.md");
        let marker = "<!-- lare-title: Test Doc -->";
        let body = "Contenuto di prova";
        std::fs::write(&file_path, format!("{}\n{}", marker, body))
            .expect("write file");

        // Con il vecchio reject_traversal questo falliva perché "/" era vietato.
        // Con validate_within_root "Progetti/doc.md" è un path legittimo.
        let result = open(dir.path(), "Progetti/doc.md");
        assert!(
            result.is_ok(),
            "open con subfolder path deve riuscire, got: {:?}",
            result
        );
        let doc = result.unwrap();
        assert_eq!(doc.title, "Test Doc");
        assert_eq!(doc.content, body);
    }

    #[test]
    fn archive_delete_with_subfolder_path() {
        // Dato: un file .md dentro una sottocartella.
        let dir = tempdir().expect("tempdir");
        let subfolder = dir.path().join("Progetti");
        std::fs::create_dir(&subfolder).expect("create subfolder");

        let file_path = subfolder.join("doc.md");
        std::fs::write(&file_path, "<!-- lare-title: Test -->\nContent")
            .expect("write file");

        assert!(file_path.exists(), "file deve esistere prima del delete");

        // Con il vecchio reject_traversal questo falliva perché "/" era vietato.
        let result = delete(dir.path(), "Progetti/doc.md");
        assert!(
            result.is_ok(),
            "delete con subfolder path deve riuscire, got: {:?}",
            result
        );
        assert!(!file_path.exists(), "file deve essere rimosso dopo delete");
    }

    // ── update: sovrascrittura in-place ───────────────────────────────────

    #[test]
    fn update_overwrites_existing_file_content_and_title() {
        let dir = tempdir().expect("tempdir");
        let filename = save(dir.path(), "Sumeri", "Prima versione, poche righe.", "111").expect("save");

        update(dir.path(), &filename, "Sumeri Antichi", "Versione fusa, molto più ricca.")
            .expect("update should succeed");

        let doc = open(dir.path(), &filename).expect("open after update");
        assert_eq!(doc.title, "Sumeri Antichi", "update deve scrivere il NUOVO titolo, non conservare il vecchio");
        assert_eq!(doc.content, "Versione fusa, molto più ricca.");
    }

    #[test]
    fn update_preserves_filename_no_new_file_created() {
        let dir = tempdir().expect("tempdir");
        let filename = save(dir.path(), "Doc", "content", "222").expect("save");

        update(dir.path(), &filename, "Doc", "updated content").expect("update");

        let entries = list(dir.path());
        assert_eq!(entries.len(), 1, "update non deve creare un secondo file, entries: {:?}", entries);
        assert_eq!(entries[0].file, filename);
    }

    #[test]
    fn update_with_subfolder_path_succeeds() {
        let dir = tempdir().expect("tempdir");
        let subfolder = dir.path().join("Progetti");
        std::fs::create_dir(&subfolder).expect("create subfolder");
        let file_path = subfolder.join("doc.md");
        std::fs::write(&file_path, "<!-- lare-title: Test Doc -->\nVecchio contenuto")
            .expect("write file");

        let result = update(dir.path(), "Progetti/doc.md", "Test Doc", "Nuovo contenuto fuso");
        assert!(result.is_ok(), "update con sottocartella deve riuscire, got: {:?}", result);

        let doc = open(dir.path(), "Progetti/doc.md").expect("open");
        assert_eq!(doc.content, "Nuovo contenuto fuso");
    }

    #[test]
    fn update_nonexistent_file_returns_err() {
        let dir = tempdir().expect("tempdir");
        let result = update(dir.path(), "does-not-exist.md", "Title", "content");
        assert!(result.is_err(), "update su file inesistente deve fallire (non è una create)");
        let msg = result.unwrap_err();
        assert!(msg.contains("not found"), "il messaggio deve dire che il file non esiste, got: {:?}", msg);
    }

    #[test]
    fn update_rejects_dotdot_traversal() {
        let dir = tempdir().expect("tempdir");
        let result = update(dir.path(), "../sensitive.md", "Title", "content");
        assert!(result.is_err(), "expected Err for '../sensitive.md'");
        let msg = result.unwrap_err();
        assert!(msg.contains("traversal"), "error must mention traversal, got: {:?}", msg);
    }

    // ── validate_within_root ──────────────────────────────────────────────

    #[test]
    fn validate_within_root_blocks_escape() {
        // "../etc/passwd" contiene ".." → deve essere rifiutato con "traversal".
        let dir = tempdir().expect("tempdir");
        let result = validate_within_root(dir.path(), "../etc/passwd");
        assert!(result.is_err(), "expected Err for '../etc/passwd'");
        let msg = result.unwrap_err();
        assert!(
            msg.contains("traversal"),
            "error must mention traversal, got: {:?}",
            msg
        );
    }

    #[test]
    fn validate_within_root_rejects_backslash() {
        // I backslash non sono mai separatori validi nel nostro protocollo.
        let dir = tempdir().expect("tempdir");
        let result = validate_within_root(dir.path(), "folder\\file.md");
        assert!(result.is_err(), "expected Err for 'folder\\\\file.md'");
        let msg = result.unwrap_err();
        assert!(
            msg.contains("traversal"),
            "error must mention traversal, got: {:?}",
            msg
        );
    }

    // ── list_tree ─────────────────────────────────────────────────────────

    #[test]
    fn list_tree_empty_dir_returns_empty_tree() {
        // Una directory vuota → albero con root_files e folders vuoti.
        let dir = tempdir().expect("tempdir");
        let tree = list_tree(dir.path());
        assert!(tree.root_files.is_empty(), "root_files deve essere vuoto");
        assert!(tree.folders.is_empty(), "folders deve essere vuoto");
    }

    #[test]
    fn list_tree_root_files_only() {
        // Solo file .md in root, nessuna sottocartella.
        let dir = tempdir().expect("tempdir");
        std::fs::write(dir.path().join("a.md"), "<!-- lare-title: A -->\nContenuto A")
            .expect("write a.md");
        std::fs::write(dir.path().join("b.md"), "<!-- lare-title: B -->\nContenuto B")
            .expect("write b.md");

        let tree = list_tree(dir.path());
        assert_eq!(tree.root_files.len(), 2, "devono esserci 2 root_files");
        assert!(tree.folders.is_empty(), "nessuna cartella attesa");
        // I file in root hanno un filename semplice (senza "/").
        let files: Vec<&str> = tree.root_files.iter().map(|e| e.file.as_str()).collect();
        assert!(files.contains(&"a.md"), "a.md deve essere in root_files");
        assert!(files.contains(&"b.md"), "b.md deve essere in root_files");
    }

    #[test]
    fn list_tree_with_one_folder() {
        // Una cartella con un file .md dentro.
        let dir = tempdir().expect("tempdir");
        let sub = dir.path().join("Progetti");
        std::fs::create_dir(&sub).expect("create dir");
        std::fs::write(sub.join("doc.md"), "<!-- lare-title: Doc -->\nTesto")
            .expect("write doc.md");

        let tree = list_tree(dir.path());
        assert!(tree.root_files.is_empty(), "nessun root_file atteso");
        assert_eq!(tree.folders.len(), 1, "deve esserci 1 cartella");

        let node = &tree.folders[0];
        assert_eq!(node.name, "Progetti");
        assert_eq!(node.rel_path, "Progetti");
        assert_eq!(node.files.len(), 1, "1 file nella cartella");
        // Il campo `file` dell'entry deve essere il path relativo con "/".
        assert_eq!(node.files[0].file, "Progetti/doc.md");
        assert_eq!(node.files[0].title, "Doc");
    }

    #[test]
    fn list_tree_nested_folders() {
        // Cartella annidata: Library/Progetti/Web/page.md
        let dir = tempdir().expect("tempdir");
        let nested = dir.path().join("Progetti").join("Web");
        std::fs::create_dir_all(&nested).expect("create nested dirs");
        std::fs::write(nested.join("page.md"), "<!-- lare-title: Page -->\nContenuto")
            .expect("write page.md");

        let tree = list_tree(dir.path());
        assert_eq!(tree.folders.len(), 1, "1 cartella di primo livello");

        let progetti = &tree.folders[0];
        assert_eq!(progetti.name, "Progetti");
        assert_eq!(progetti.folders.len(), 1, "Progetti deve avere 1 sotto-cartella");

        let web = &progetti.folders[0];
        assert_eq!(web.name, "Web");
        assert_eq!(web.rel_path, "Progetti/Web");
        assert_eq!(web.files.len(), 1, "1 file in Web");
        assert_eq!(web.files[0].file, "Progetti/Web/page.md");
    }

    #[test]
    fn list_tree_ignores_non_md_files_in_folders() {
        // Solo i file .md vengono inclusi; .txt, .json ecc. vengono ignorati.
        let dir = tempdir().expect("tempdir");
        let sub = dir.path().join("Misc");
        std::fs::create_dir(&sub).expect("create dir");
        std::fs::write(sub.join("doc.md"), "<!-- lare-title: Doc -->\nOk")
            .expect("write doc.md");
        std::fs::write(sub.join("readme.txt"), "testo semplice")
            .expect("write readme.txt");
        std::fs::write(sub.join("data.json"), "{}")
            .expect("write data.json");

        let tree = list_tree(dir.path());
        assert_eq!(tree.folders.len(), 1);
        assert_eq!(
            tree.folders[0].files.len(), 1,
            "solo doc.md deve essere incluso (readme.txt e data.json ignorati)"
        );
        assert_eq!(tree.folders[0].files[0].file, "Misc/doc.md");
    }

    // ── create_folder ─────────────────────────────────────────────────────

    #[test]
    fn create_folder_creates_directory() {
        // Caso base: crea una cartella in root (parent_rel = "").
        let dir = tempdir().expect("tempdir");
        let result = create_folder(dir.path(), "", "Nuova");
        assert!(result.is_ok(), "create_folder deve riuscire, got: {:?}", result);
        let rel_path = result.unwrap();
        assert_eq!(rel_path, "Nuova", "rel_path deve essere 'Nuova'");
        assert!(dir.path().join("Nuova").is_dir(), "la cartella deve esistere su disco");
    }

    #[test]
    fn create_folder_rejects_slash_in_name() {
        // Il nome di cartella non può contenere "/" (un nome, non un path).
        let dir = tempdir().expect("tempdir");
        let result = create_folder(dir.path(), "", "foo/bar");
        assert!(result.is_err(), "create_folder con '/' nel nome deve fallire");
    }

    #[test]
    fn create_folder_rejects_dotdot() {
        // ".." come nome non è consentito.
        let dir = tempdir().expect("tempdir");
        let result = create_folder(dir.path(), "", "..");
        assert!(result.is_err(), "create_folder con '..' deve fallire");
    }

    #[test]
    fn create_folder_parent_not_found_returns_err() {
        // Se il parent non esiste, restituisce Err.
        let dir = tempdir().expect("tempdir");
        let result = create_folder(dir.path(), "InesistenteParent", "Nuova");
        assert!(result.is_err(), "parent inesistente deve dare Err");
        // M-5: il messaggio deve identificare la causa (cartella padre non trovata).
        let msg = result.unwrap_err();
        assert!(
            msg.contains("not found"),
            "errore deve contenere 'not found', got: {:?}",
            msg
        );
    }

    #[test]
    fn create_folder_collision_appends_numeric_suffix() {
        // Se "Nuova" esiste già, crea "Nuova 2".
        let dir = tempdir().expect("tempdir");
        std::fs::create_dir(dir.path().join("Nuova")).expect("crea Nuova");

        let result = create_folder(dir.path(), "", "Nuova");
        assert!(result.is_ok(), "create_folder con collisione deve riuscire");
        let rel_path = result.unwrap();
        assert_eq!(rel_path, "Nuova 2", "col suffisso numerico deve essere 'Nuova 2'");
        assert!(dir.path().join("Nuova 2").is_dir());
    }

    // ── rename_folder ─────────────────────────────────────────────────────

    #[test]
    fn rename_folder_renames_directory() {
        // Rinomina una cartella esistente.
        let dir = tempdir().expect("tempdir");
        std::fs::create_dir(dir.path().join("Vecchio")).expect("crea Vecchio");

        let result = rename_folder(dir.path(), "Vecchio", "Nuovo");
        assert!(result.is_ok(), "rename_folder deve riuscire, got: {:?}", result);
        assert!(!dir.path().join("Vecchio").exists(), "Vecchio non deve più esistere");
        assert!(dir.path().join("Nuovo").is_dir(), "Nuovo deve esistere");
    }

    #[test]
    fn rename_folder_rejects_invalid_name() {
        // Il nuovo nome non può contenere "/" o "..".
        let dir = tempdir().expect("tempdir");
        std::fs::create_dir(dir.path().join("Cartella")).expect("crea Cartella");

        let result = rename_folder(dir.path(), "Cartella", "foo/bar");
        assert!(result.is_err(), "rename_folder con '/' nel nome deve fallire");
        // M-5: il messaggio deve identificare la causa (nome invalido).
        let msg = result.unwrap_err();
        assert!(
            msg.contains("invalid"),
            "errore deve contenere 'invalid', got: {:?}",
            msg
        );
    }

    #[test]
    fn rename_folder_rejects_empty_rel() {
        // La cartella radice non può essere rinominata: folder_rel="" è la root stessa.
        // La guardia deve scattare prima di qualsiasi validazione del filesystem.
        let dir = tempdir().expect("tempdir");
        let result = rename_folder(dir.path(), "", "NuovoNome");
        assert!(result.is_err(), "rename_folder con folder_rel vuoto deve fallire");
        let msg = result.unwrap_err();
        assert!(
            msg.contains("vuoto"),
            "errore deve contenere 'vuoto', got: {:?}",
            msg
        );
    }

    #[test]
    fn rename_folder_rejects_collision() {
        // Collisione: esiste già "B" → non si può rinominare "A" in "B".
        // Verifica che la guardia esplicita di collisione scatti prima di fs::rename.
        let dir = tempdir().expect("tempdir");
        std::fs::create_dir(dir.path().join("A")).expect("crea A");
        std::fs::create_dir(dir.path().join("B")).expect("crea B");

        let result = rename_folder(dir.path(), "A", "B");
        assert!(
            result.is_err(),
            "rename_folder deve fallire se esiste già una cartella con il nuovo nome"
        );
        let msg = result.unwrap_err();
        assert!(
            msg.contains("esiste già"),
            "errore deve contenere 'esiste già', got: {:?}",
            msg
        );
    }

    // ── move_file ─────────────────────────────────────────────────────────

    #[test]
    fn move_file_moves_to_subfolder() {
        // Sposta un file dalla root a una sottocartella.
        let dir = tempdir().expect("tempdir");
        std::fs::write(dir.path().join("doc.md"), "<!-- lare-title: D -->\nTesto")
            .expect("write doc.md");
        let sub = dir.path().join("Archivio");
        std::fs::create_dir(&sub).expect("create dir");

        let result = move_file(dir.path(), "doc.md", "Archivio");
        assert!(result.is_ok(), "move_file deve riuscire, got: {:?}", result);
        assert!(!dir.path().join("doc.md").exists(), "il file non deve più essere in root");
        assert!(sub.join("doc.md").exists(), "il file deve essere nella sottocartella");
    }

    #[test]
    fn move_file_moves_to_root() {
        // Sposta un file dalla sottocartella alla root (target_folder_rel = "").
        let dir = tempdir().expect("tempdir");
        let sub = dir.path().join("Archivio");
        std::fs::create_dir(&sub).expect("create dir");
        std::fs::write(sub.join("doc.md"), "<!-- lare-title: D -->\nTesto")
            .expect("write doc.md");

        // target_folder_rel = "" significa root.
        let result = move_file(dir.path(), "Archivio/doc.md", "");
        assert!(result.is_ok(), "move_file verso root deve riuscire, got: {:?}", result);
        assert!(dir.path().join("doc.md").exists(), "il file deve essere in root");
        assert!(!sub.join("doc.md").exists(), "il file non deve più essere nella sotto-cartella");
    }

    #[test]
    fn move_file_rejects_traversal() {
        // Un file_rel con ".." deve essere rifiutato.
        let dir = tempdir().expect("tempdir");
        let result = move_file(dir.path(), "../sensitive.md", "");
        assert!(result.is_err(), "move_file con traversal deve fallire");
        let msg = result.unwrap_err();
        assert!(
            msg.contains("traversal"),
            "error deve contenere 'traversal', got: {:?}",
            msg
        );
    }

    // ── move_file: hardening collisione ───────────────────────────────────

    #[test]
    fn move_file_rejects_collision_no_overwrite() {
        // Verifica che move_file NON sovrascriva un file già esistente in destinazione.
        //
        // Scenario: la cartella "Dest" ha già un "doc.md" con contenuto A.
        // Tentiamo di spostare "doc.md" (contenuto B) dalla root in "Dest".
        // La funzione deve restituire Err; il file originale (contenuto A) deve
        // essere intatto — nessuna perdita di dati.
        let dir = tempdir().expect("tempdir");
        let dst = dir.path().join("Dest");
        std::fs::create_dir(&dst).expect("create Dest");

        // Contenuto A nella destinazione (deve sopravvivere intatto).
        std::fs::write(dst.join("doc.md"), "<!-- lare-title: A -->\nContenuto A")
            .expect("write dst/doc.md");

        // Contenuto B nella root (il file che vorremmo spostare).
        std::fs::write(dir.path().join("doc.md"), "<!-- lare-title: B -->\nContenuto B")
            .expect("write src/doc.md");

        let result = move_file(dir.path(), "doc.md", "Dest");
        assert!(
            result.is_err(),
            "move_file deve fallire se nella destinazione esiste già un file con lo stesso nome"
        );

        // Sicurezza: il file in destinazione non deve essere stato sovrascritto.
        let content = std::fs::read_to_string(dst.join("doc.md")).expect("read dst/doc.md");
        assert_eq!(
            content,
            "<!-- lare-title: A -->\nContenuto A",
            "il file in dst non deve essere sovrascritto: contenuto A deve sopravvivere"
        );
    }

    #[test]
    fn move_file_rejects_empty_rel() {
        // file_rel="" punta alla radice della Library: non è un file
        // e non può essere spostata. La guardia deve scattare prima di
        // qualsiasi accesso al filesystem.
        let dir = tempdir().expect("tempdir");
        let result = move_file(dir.path(), "", "");
        assert!(result.is_err(), "move_file con file_rel vuoto deve fallire");
        let msg = result.unwrap_err();
        assert!(
            msg.contains("vuoto"),
            "errore deve contenere 'vuoto', got: {:?}",
            msg
        );
    }

    #[test]
    fn move_file_rejects_directory_source() {
        // Spostare una cartella con move_file non è consentito: la guardia
        // is_file() deve restituire Err prima di qualsiasi fs::rename.
        // Usiamo una cartella "Dest" separata per evitare false collisioni.
        let dir = tempdir().expect("tempdir");
        std::fs::create_dir(dir.path().join("Cartella")).expect("crea Cartella");
        std::fs::create_dir(dir.path().join("Dest")).expect("crea Dest");

        let result = move_file(dir.path(), "Cartella", "Dest");
        assert!(
            result.is_err(),
            "move_file con una directory come sorgente deve fallire"
        );
        let msg = result.unwrap_err();
        assert!(
            msg.contains("file"),
            "errore deve contenere 'file', got: {:?}",
            msg
        );
    }

    // ── move_folder ───────────────────────────────────────────────────────

    #[test]
    fn move_folder_into_subfolder() {
        // Caso normale: sposta la cartella "A" (dalla root) dentro "B".
        // Risultato atteso: "B/A" esiste, "A" non esiste più in root.
        let dir = tempdir().expect("tempdir");
        std::fs::create_dir(dir.path().join("A")).expect("create A");
        std::fs::create_dir(dir.path().join("B")).expect("create B");

        let result = move_folder(dir.path(), "A", "B");
        assert!(result.is_ok(), "move_folder deve riuscire, got: {:?}", result);
        assert!(
            dir.path().join("B").join("A").exists(),
            "B/A deve esistere dopo lo spostamento"
        );
        assert!(
            !dir.path().join("A").exists(),
            "A non deve più esistere in root dopo lo spostamento"
        );
    }

    #[test]
    fn move_folder_to_root() {
        // Sposta la cartella "Sub/Archivio" nella root (target_parent_rel = "").
        // Risultato atteso: "Archivio" esiste in root, "Sub/Archivio" non esiste più.
        let dir = tempdir().expect("tempdir");
        std::fs::create_dir(dir.path().join("Sub")).expect("create Sub");
        std::fs::create_dir(dir.path().join("Sub").join("Archivio")).expect("create Sub/Archivio");

        let result = move_folder(dir.path(), "Sub/Archivio", "");
        assert!(result.is_ok(), "move_folder verso root deve riuscire, got: {:?}", result);
        assert!(
            dir.path().join("Archivio").exists(),
            "Archivio deve esistere in root dopo lo spostamento"
        );
        assert!(
            !dir.path().join("Sub").join("Archivio").exists(),
            "Sub/Archivio non deve più esistere dopo lo spostamento"
        );
    }

    #[test]
    fn move_folder_rejects_into_itself() {
        // Anti-ciclo: non si può spostare "A" dentro "A" (sarebbe "A/A").
        // La guardia confronta le rel-path normalizzate:
        // target_parent_rel ("A") == folder_rel ("A") → Err.
        //
        // Nota: su Windows fs::rename fallisce comunque, ma non a causa della
        // nostra guardia. Verifichiamo che l'errore contenga "ciclo" per provare
        // che è la nostra guardia a scattare — non il sistema operativo.
        let dir = tempdir().expect("tempdir");
        std::fs::create_dir(dir.path().join("A")).expect("create A");

        let result = move_folder(dir.path(), "A", "A");
        assert!(
            result.is_err(),
            "move_folder non può spostare una cartella in sé stessa"
        );
        let msg = result.unwrap_err();
        assert!(
            msg.contains("ciclo"),
            "l'errore deve identificare la guardia anti-ciclo (contenere 'ciclo'), got: {:?}",
            msg
        );
    }

    #[test]
    fn move_folder_rejects_into_descendant() {
        // Anti-ciclo: non si può spostare "A" dentro "A/B" (B è discendente di A).
        // Se lo facessimo, "A" si troverebbe al suo interno come "A/B/A" — loop.
        // La guardia: target_parent_rel.starts_with("A/") → Err.
        //
        // Stessa nota: verifichiamo "ciclo" nel messaggio per distinguere la nostra
        // guardia da un eventuale errore OS.
        let dir = tempdir().expect("tempdir");
        std::fs::create_dir(dir.path().join("A")).expect("create A");
        std::fs::create_dir(dir.path().join("A").join("B")).expect("create A/B");

        let result = move_folder(dir.path(), "A", "A/B");
        assert!(
            result.is_err(),
            "move_folder non può spostare una cartella in una sua discendente"
        );
        let msg = result.unwrap_err();
        assert!(
            msg.contains("ciclo"),
            "l'errore deve identificare la guardia anti-ciclo (contenere 'ciclo'), got: {:?}",
            msg
        );
    }

    #[test]
    fn move_folder_rejects_name_collision() {
        // Collisione: "B" contiene già una cartella "A"; non possiamo sovrascriverla
        // con un merge silenzioso (comportamento pericoloso). → Err.
        //
        // Su Linux, fs::rename di una directory su una directory VUOTA ha successo
        // (POSIX rename semantics). Questo test verifica che la nostra guardia
        // esplicita blocchi l'operazione prima — il messaggio deve contenere "esiste già".
        let dir = tempdir().expect("tempdir");
        // A in root (da spostare).
        std::fs::create_dir(dir.path().join("A")).expect("create A");
        // B in root (destinazione).
        std::fs::create_dir(dir.path().join("B")).expect("create B");
        // B/A già esiste (vuota) → collisione.
        std::fs::create_dir(dir.path().join("B").join("A")).expect("create B/A");

        let result = move_folder(dir.path(), "A", "B");
        assert!(
            result.is_err(),
            "move_folder deve fallire se nella destinazione esiste già una entry con lo stesso nome"
        );
        let msg = result.unwrap_err();
        assert!(
            msg.contains("esiste già"),
            "l'errore deve identificare il no-merge (contenere 'esiste già'), got: {:?}",
            msg
        );
    }

    #[test]
    fn move_folder_rejects_traversal() {
        // Sicurezza: rel-path con ".." devono essere rifiutate da validate_within_root.
        // L'errore deve contenere "traversal".
        let dir = tempdir().expect("tempdir");

        let result = move_folder(dir.path(), "../fuori", "");
        assert!(result.is_err(), "move_folder con traversal deve fallire");
        let msg = result.unwrap_err();
        assert!(
            msg.contains("traversal"),
            "error deve contenere 'traversal', got: {:?}",
            msg
        );
    }

    #[test]
    fn move_folder_source_not_found() {
        // La cartella sorgente non esiste → Err.
        let dir = tempdir().expect("tempdir");

        let result = move_folder(dir.path(), "Inesistente", "");
        assert!(
            result.is_err(),
            "move_folder deve fallire se la sorgente non esiste"
        );
        // M-5: il messaggio deve identificare la causa (sorgente mancante).
        let msg = result.unwrap_err();
        assert!(
            msg.contains("sorgente"),
            "errore deve contenere 'sorgente', got: {:?}",
            msg
        );
    }

    #[test]
    fn move_folder_target_parent_not_found() {
        // La cartella padre di destinazione non esiste → Err.
        let dir = tempdir().expect("tempdir");
        std::fs::create_dir(dir.path().join("A")).expect("create A");

        let result = move_folder(dir.path(), "A", "ParentInesistente");
        assert!(
            result.is_err(),
            "move_folder deve fallire se il parent di destinazione non esiste"
        );
        // M-5: il messaggio deve identificare la causa (destinazione non trovata).
        let msg = result.unwrap_err();
        assert!(
            msg.contains("non trovata"),
            "errore deve contenere 'non trovata', got: {:?}",
            msg
        );
    }

    // ── delete_folder ─────────────────────────────────────────────────────

    #[test]
    fn delete_folder_removes_empty_folder() {
        // Una cartella vuota deve essere eliminata con successo.
        let dir = tempdir().expect("tempdir");
        std::fs::create_dir(dir.path().join("Vuota")).expect("crea Vuota");

        let result = delete_folder(dir.path(), "Vuota");
        assert!(
            result.is_ok(),
            "delete_folder su cartella vuota deve riuscire, got: {:?}",
            result
        );
        assert!(
            !dir.path().join("Vuota").exists(),
            "la cartella deve essere eliminata dal filesystem"
        );
    }

    #[test]
    fn delete_folder_rejects_nonempty_with_md_file() {
        // Una cartella con un file .md non deve essere eliminata.
        let dir = tempdir().expect("tempdir");
        let sub = dir.path().join("Piena");
        std::fs::create_dir(&sub).expect("crea Piena");
        std::fs::write(sub.join("doc.md"), "# Titolo").expect("write doc.md");

        let result = delete_folder(dir.path(), "Piena");
        assert!(result.is_err(), "delete_folder su cartella con .md deve fallire");
        let msg = result.unwrap_err();
        assert!(
            msg.contains("not empty"),
            "errore deve contenere 'not empty', got: {:?}",
            msg
        );
    }

    #[test]
    fn delete_folder_rejects_nonempty_with_non_md_file() {
        // La UI non mostrerebbe un .txt (non è un documento Lare), ma il fs lo vede
        // comunque come entry → delete_folder deve rifiutare.
        // Questo garantisce che l'utente non perda file nascosti (es. file creati
        // da Esplora Risorse nella cartella Library).
        let dir = tempdir().expect("tempdir");
        let sub = dir.path().join("ConTxt");
        std::fs::create_dir(&sub).expect("crea ConTxt");
        std::fs::write(sub.join("note.txt"), "testo qualsiasi").expect("write note.txt");

        let result = delete_folder(dir.path(), "ConTxt");
        assert!(result.is_err(), "delete_folder su cartella con .txt deve fallire");
        let msg = result.unwrap_err();
        assert!(
            msg.contains("not empty"),
            "errore deve contenere 'not empty', got: {:?}",
            msg
        );
    }

    #[test]
    fn delete_folder_rejects_folder_with_only_empty_subfolder() {
        // Una sotto-cartella (anche se vuota) è pur sempre una directory entry:
        // read_dir().next() restituisce quell'entry → is_none() è false → Err.
        // Questo previene la cancellazione ricorsiva implicita.
        let dir = tempdir().expect("tempdir");
        let parent = dir.path().join("Genitore");
        std::fs::create_dir(&parent).expect("crea Genitore");
        std::fs::create_dir(parent.join("Figlia")).expect("crea Figlia");

        let result = delete_folder(dir.path(), "Genitore");
        assert!(
            result.is_err(),
            "delete_folder su cartella con sotto-cartella deve fallire"
        );
        let msg = result.unwrap_err();
        assert!(
            msg.contains("not empty"),
            "errore deve contenere 'not empty', got: {:?}",
            msg
        );
    }

    #[test]
    fn delete_folder_not_found_returns_err() {
        // Una cartella inesistente deve restituire un errore che contiene "not found".
        let dir = tempdir().expect("tempdir");

        let result = delete_folder(dir.path(), "Inesistente");
        assert!(
            result.is_err(),
            "delete_folder su cartella inesistente deve fallire"
        );
        let msg = result.unwrap_err();
        assert!(
            msg.contains("not found"),
            "errore deve contenere 'not found', got: {:?}",
            msg
        );
    }

    #[test]
    fn delete_folder_rejects_traversal() {
        // Un folder_rel che contiene ".." deve essere rifiutato prima di
        // toccare il filesystem, per prevenire traversal fuori dalla Library.
        let dir = tempdir().expect("tempdir");

        let result = delete_folder(dir.path(), "../x");
        assert!(result.is_err(), "delete_folder con traversal deve fallire");
        let msg = result.unwrap_err();
        assert!(
            msg.contains("traversal"),
            "errore deve contenere 'traversal', got: {:?}",
            msg
        );
    }

    #[test]
    fn delete_folder_rejects_empty_rel() {
        // folder_rel="" punta alla radice della Library: eliminarla non è
        // consentito. La guardia deve scattare prima di qualsiasi accesso
        // al filesystem, evitando che la root venga rimossa.
        let dir = tempdir().expect("tempdir");
        let result = delete_folder(dir.path(), "");
        assert!(result.is_err(), "delete_folder con folder_rel vuoto deve fallire");
        let msg = result.unwrap_err();
        assert!(
            msg.contains("vuoto"),
            "errore deve contenere 'vuoto', got: {:?}",
            msg
        );
    }

    // ── migrate_to_documents_layout ──────────────────────────────────────────

    /// Test 1: anche su root vuota, `documents/` viene creata.
    /// Verifica che fresh-install e primo avvio non richiedano file pre-esistenti.
    #[test]
    fn migrate_creates_documents_dir_even_when_empty() {
        let dir = tempdir().expect("tempdir");
        // Root vuota — nessun file né cartella.
        let result = migrate_to_documents_layout(dir.path());
        assert!(result.is_ok(), "migrate deve riuscire su root vuota, got: {:?}", result);
        assert!(
            dir.path().join("documents").is_dir(),
            "documents/ deve esistere dopo la migrazione anche se la root era vuota"
        );
    }

    /// Test 2: i file `.md` sciolti nella root vengono spostati in `documents/`.
    #[test]
    fn migrate_moves_md_files_to_documents() {
        let dir = tempdir().expect("tempdir");
        // Prepara due file .md nella root (vecchio layout).
        std::fs::write(
            dir.path().join("a.md"),
            "<!-- lare-title: A -->\nContenuto A",
        )
        .expect("write a.md");
        std::fs::write(
            dir.path().join("b.md"),
            "<!-- lare-title: B -->\nContenuto B",
        )
        .expect("write b.md");

        let result = migrate_to_documents_layout(dir.path());
        assert!(result.is_ok(), "migrate deve riuscire, got: {:?}", result);

        // I file devono essere in documents/, non più nella root.
        let docs = dir.path().join("documents");
        assert!(docs.join("a.md").exists(), "a.md deve essere in documents/");
        assert!(docs.join("b.md").exists(), "b.md deve essere in documents/");
        assert!(!dir.path().join("a.md").exists(), "a.md non deve restare in root");
        assert!(!dir.path().join("b.md").exists(), "b.md non deve restare in root");
    }

    /// Test 3: le cartelle utente (es. create con "Library a cartelle") vengono spostate.
    #[test]
    fn migrate_moves_user_folders_to_documents() {
        let dir = tempdir().expect("tempdir");
        // Una cartella utente con un file dentro (vecchio layout: cartella in root).
        let user_folder = dir.path().join("Progetti");
        std::fs::create_dir(&user_folder).expect("create Progetti");
        std::fs::write(
            user_folder.join("doc.md"),
            "<!-- lare-title: Doc -->\nTesto",
        )
        .expect("write doc.md");

        let result = migrate_to_documents_layout(dir.path());
        assert!(result.is_ok(), "migrate deve riuscire, got: {:?}", result);

        let docs = dir.path().join("documents");
        assert!(docs.join("Progetti").is_dir(), "Progetti/ deve essere in documents/");
        assert!(
            docs.join("Progetti").join("doc.md").exists(),
            "doc.md deve seguire la cartella"
        );
        assert!(
            !dir.path().join("Progetti").exists(),
            "Progetti/ non deve restare in root"
        );
    }

    /// Test 4: `find/` nella root NON viene spostata (è riservata al /find interno).
    #[test]
    fn migrate_leaves_find_at_root() {
        let dir = tempdir().expect("tempdir");
        // Simula la struttura reale: find/ con una sessione salvata.
        let find_dir = dir.path().join("find");
        std::fs::create_dir(&find_dir).expect("create find");
        std::fs::write(
            find_dir.join("session.json"),
            r#"{"query":"*.pdf","ts":1,"hits":[]}"#,
        )
        .expect("write session.json");

        let result = migrate_to_documents_layout(dir.path());
        assert!(result.is_ok(), "migrate deve riuscire, got: {:?}", result);

        // find/ deve restare NELLA ROOT, non finire in documents/.
        assert!(dir.path().join("find").is_dir(), "find/ deve restare in root");
        assert!(
            !dir.path().join("documents").join("find").exists(),
            "find/ NON deve apparire in documents/"
        );
    }

    /// Test 5: `documents/` pre-esistente non viene spostata dentro se stessa.
    /// Verifica che il filtro "skip documents" funzioni anche quando documents/ già esiste.
    #[test]
    fn migrate_does_not_move_documents_into_itself() {
        let dir = tempdir().expect("tempdir");
        // Pre-crea documents/ con un file dentro (simula migrazione già avvenuta).
        let docs = dir.path().join("documents");
        std::fs::create_dir(&docs).expect("create documents");
        std::fs::write(
            docs.join("pre.md"),
            "<!-- lare-title: Pre -->\nPre-esistente",
        )
        .expect("write pre.md");

        let result = migrate_to_documents_layout(dir.path());
        assert!(result.is_ok(), "migrate deve riuscire, got: {:?}", result);

        // documents/ deve restare in root; NON deve finire in documents/documents/.
        assert!(
            dir.path().join("documents").is_dir(),
            "documents/ deve restare in root"
        );
        assert!(
            !dir.path().join("documents").join("documents").exists(),
            "documents/ NON deve finire in documents/documents/"
        );
        // Il file pre-esistente deve essere ancora lì.
        assert!(
            dir.path().join("documents").join("pre.md").exists(),
            "pre.md pre-esistente deve sopravvivere"
        );
    }

    /// Test 6: la migrazione è idempotente.
    /// Chiamarla due volte non produce errori né raddoppia le cartelle.
    #[test]
    fn migrate_idempotent() {
        let dir = tempdir().expect("tempdir");
        // Prepara un file .md in root.
        std::fs::write(
            dir.path().join("note.md"),
            "<!-- lare-title: Note -->\nTesto",
        )
        .expect("write note.md");
        // Prepara find/ (deve restare in root).
        std::fs::create_dir(dir.path().join("find")).expect("create find");

        // Prima chiamata: sposta note.md in documents/, crea find/ resta invariata.
        let r1 = migrate_to_documents_layout(dir.path());
        assert!(r1.is_ok(), "prima chiamata deve riuscire, got: {:?}", r1);

        // Seconda chiamata (root ha ora solo find/ e documents/) → no-op.
        let r2 = migrate_to_documents_layout(dir.path());
        assert!(r2.is_ok(), "seconda chiamata (idempotente) deve riuscire, got: {:?}", r2);

        // Stato finale corretto dopo due chiamate.
        assert!(
            dir.path().join("documents").join("note.md").exists(),
            "note.md deve essere in documents/"
        );
        assert!(dir.path().join("find").is_dir(), "find/ deve restare in root");
        assert!(
            !dir.path().join("documents").join("documents").exists(),
            "documents/documents/ non deve esistere"
        );
        // Solo due entry nella root: find/ e documents/.
        let root_entries: Vec<_> = std::fs::read_dir(dir.path())
            .expect("read dir")
            .flatten()
            .collect();
        assert_eq!(
            root_entries.len(),
            2,
            "root deve avere esattamente 2 entry dopo migrazione idempotente, got: {:?}",
            root_entries
                .iter()
                .map(|e| e.file_name())
                .collect::<Vec<_>>()
        );
    }

    /// Test 7: collisione di nome → suffisso numerico, NESSUNA perdita dati.
    ///
    /// Scenario: `documents/foo.md` (contenuto A) pre-esiste; `library/foo.md` (contenuto B)
    /// deve essere spostato. Dopo la migrazione entrambi devono sopravvivere con
    /// contenuti distinti: `documents/foo.md`=A, `documents/foo 2.md`=B.
    #[test]
    fn migrate_collision_appends_suffix_no_overwrite() {
        let dir = tempdir().expect("tempdir");

        // Pre-crea documents/ con un file già presente (contenuto A).
        let docs = dir.path().join("documents");
        std::fs::create_dir(&docs).expect("create documents");
        let content_a = "<!-- lare-title: A -->\nContenuto originale";
        std::fs::write(docs.join("foo.md"), content_a).expect("write foo.md in documents");

        // File omonimo nella root library (contenuto B) → deve essere spostato con suffisso.
        let content_b = "<!-- lare-title: B -->\nContenuto da spostare";
        std::fs::write(dir.path().join("foo.md"), content_b).expect("write foo.md in root");

        let result = migrate_to_documents_layout(dir.path());
        assert!(result.is_ok(), "migrate deve riuscire, got: {:?}", result);

        // Il file originale deve conservare il suo contenuto (NON sovrascritto).
        let original =
            std::fs::read_to_string(docs.join("foo.md")).expect("read foo.md");
        assert_eq!(
            original, content_a,
            "foo.md (originale) deve conservare il contenuto A"
        );

        // Il file spostato deve esistere con suffisso e contenuto B distinto.
        assert!(
            docs.join("foo 2.md").exists(),
            "foo 2.md deve esistere (collisione con suffisso numerico)"
        );
        let moved =
            std::fs::read_to_string(docs.join("foo 2.md")).expect("read foo 2.md");
        assert_eq!(moved, content_b, "foo 2.md deve contenere il contenuto B");

        // La root non deve più avere foo.md.
        assert!(
            !dir.path().join("foo.md").exists(),
            "foo.md non deve restare in root dopo la migrazione"
        );
    }
}
