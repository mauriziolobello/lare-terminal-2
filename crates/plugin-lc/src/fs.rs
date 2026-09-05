//! fs.rs — Lettura directory e ordinamento entry.
//!
//! Questa è la sola dipendenza I/O del plugin: `list_dir` legge una directory
//! con `std::fs`, separa cartelle da file e ordina case-insensitive (directory
//! prima). Tutto il resto del plugin lavora su `Vec<Entry>` già costruiti →
//! testabile in unit senza toccare il filesystem.

use std::path::{Path, PathBuf};

/// Una singola voce dentro una directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Nome del file o della directory (senza percorso).
    pub name: String,
    /// `true` se è una directory.
    pub is_dir: bool,
    /// Dimensione in byte (0 per le directory).
    pub size: u64,
}

impl Entry {
    /// Restituisce l'etichetta da mostrare nel pannello.
    /// Le directory hanno il suffisso `/` per distinguerle visivamente.
    pub fn label(&self) -> String {
        if self.is_dir {
            format!("{}/", self.name)
        } else {
            self.name.clone()
        }
    }
}

/// Legge il contenuto di `path` e restituisce le entry ordinate.
///
/// Ordinamento: directory prima (case-insensitive), poi file (case-insensitive).
/// La voce speciale `..` è sempre inserita come prima riga se `path` non è la
/// root del filesystem (così l'utente può sempre salire di livello).
///
/// # Errors
/// Restituisce `Err` se `path` non è accessibile o non è una directory.
pub fn list_dir(path: &Path) -> std::io::Result<Vec<Entry>> {
    // Vista sintetica "elenco unità disco": il path sentinella NON è un percorso
    // filesystem reale (stringa vuota) e non deve passare da `read_dir`. È la cima
    // assoluta della navigazione, quindi non ha alcun ".." sopra di sé.
    if path == drive_list_sentinel().as_path() {
        return Ok(list_drives());
    }

    let mut dirs: Vec<Entry> = Vec::new();
    let mut files: Vec<Entry> = Vec::new();

    for result in std::fs::read_dir(path)? {
        let entry = result?;
        let meta = entry.metadata()?;
        let name = entry.file_name().to_string_lossy().into_owned();
        // Salta le voci nascoste che iniziano con '.' (Windows: considera anche
        // i file con attributo hidden, ma std::fs non espone l'attributo in modo
        // cross-platform semplice → per ora filtriamo solo per nome).
        // Decidiamo di NON filtrare i nascosti: l'utente di un file manager
        // vuole poterli vedere. Rimuoviamo solo "." (directory corrente) che
        // std::fs non include comunque.
        if meta.is_dir() {
            dirs.push(Entry { name, is_dir: true, size: 0 });
        } else {
            let size = meta.len();
            files.push(Entry { name, is_dir: false, size });
        }
    }

    // Ordina directory e file separatamente, case-insensitive.
    dirs.sort_by_key(|e| e.name.to_lowercase());
    files.sort_by_key(|e| e.name.to_lowercase());

    let mut result = Vec::new();

    // Inserisci ".." come prima voce se esiste un livello superiore.
    // - path normale: `parent()` è `Some` → livello superiore reale.
    // - radice di unità Windows ("C:\"): `parent()` è `None`, MA un livello
    //   superiore esiste concettualmente (l'elenco delle unità disco) → mostralo,
    //   così l'utente può uscire dalla radice del disco verso le altre unità.
    if path.parent().is_some() || is_drive_root(path) {
        result.push(Entry { name: "..".to_string(), is_dir: true, size: 0 });
    }

    result.extend(dirs);
    result.extend(files);
    Ok(result)
}

/// Risolve il path del genitore di `path`.
/// Restituisce `path` invariato se è già la root (non si può salire oltre).
pub fn parent_of(path: &Path) -> PathBuf {
    // Radice di unità ("C:\"): "su" significa l'elenco delle unità disco, non
    // "resta fermo". Senza questo, alla radice del disco il Backspace/".." non
    // portava da nessuna parte.
    if is_drive_root(path) {
        return drive_list_sentinel();
    }
    // Già sull'elenco unità (sentinella): è la cima assoluta, non si sale oltre.
    if path == drive_list_sentinel().as_path() {
        return drive_list_sentinel();
    }
    path.parent()
        .map(|p| if p.as_os_str().is_empty() { path.to_path_buf() } else { p.to_path_buf() })
        .unwrap_or_else(|| path.to_path_buf())
}

/// Path sentinella che rappresenta la vista "elenco unità disco" — non un
/// percorso filesystem reale. `PathBuf::new()` (stringa vuota) non collide
/// mai con un path reale valido su Windows o Unix.
pub fn drive_list_sentinel() -> PathBuf {
    PathBuf::new()
}

/// `true` se `path` è la radice di un'unità Windows (es. "C:\\", "C:/").
/// Su piattaforme non-Windows ritorna sempre `false` (nessun concetto di
/// unità multiple — comportamento invariato lì).
#[cfg(windows)]
pub fn is_drive_root(path: &Path) -> bool {
    let s = path.to_string_lossy();
    let bytes = s.as_bytes();
    // "X:\" o "X:/" o "X:" (3 o 2 caratteri), X = lettera ASCII.
    (bytes.len() == 2 || bytes.len() == 3)
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes.len() == 2 || bytes[2] == b'\\' || bytes[2] == b'/')
}
#[cfg(not(windows))]
pub fn is_drive_root(_path: &Path) -> bool { false }

/// Enumera le unità disco disponibili (A: .. Z:) sondando l'esistenza di
/// ciascuna radice. Nessuna API Windows dedicata — un probe diretto è
/// sufficiente e non richiede dipendenze aggiuntive.
#[cfg(windows)]
fn list_drives() -> Vec<Entry> {
    let mut out = Vec::new();
    for letter in b'A'..=b'Z' {
        let root = format!("{}:\\", letter as char);
        if Path::new(&root).exists() {
            out.push(Entry { name: root, is_dir: true, size: 0 });
        }
    }
    out
}
#[cfg(not(windows))]
fn list_drives() -> Vec<Entry> { Vec::new() }

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Helper per creare una dir di test con un nome univoco per test.
    /// Crea: {tmp}/plugin_lc_test_{tag}/{Alpha_Dir,beta_dir,apple.txt,zebra.txt}
    fn make_test_dir(tag: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("plugin_lc_test_{tag}"));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        fs::create_dir_all(base.join("beta_dir")).unwrap();
        fs::create_dir_all(base.join("Alpha_Dir")).unwrap();
        fs::write(base.join("zebra.txt"), b"hello").unwrap();
        fs::write(base.join("apple.txt"), b"world").unwrap();
        base
    }

    #[test]
    fn list_dir_returns_entries() {
        let base = make_test_dir("list");
        let entries = list_dir(&base).expect("list_dir non deve fallire su una dir valida");
        assert!(!entries.is_empty(), "deve esserci almeno una entry");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn dirs_come_before_files() {
        let base = make_test_dir("order");
        let entries = list_dir(&base).unwrap();
        let first_file_idx = entries.iter().position(|e| !e.is_dir);
        let last_dir_idx = entries.iter().rposition(|e| e.is_dir);
        if let (Some(ff), Some(ld)) = (first_file_idx, last_dir_idx) {
            assert!(
                ld < ff,
                "tutte le directory devono precedere i file: last_dir={ld}, first_file={ff}"
            );
        }
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn dirs_sorted_case_insensitive() {
        let base = make_test_dir("dirsort");
        let entries = list_dir(&base).unwrap();
        let dir_names: Vec<&str> = entries.iter()
            .filter(|e| e.is_dir && e.name != "..")
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(dir_names, vec!["Alpha_Dir", "beta_dir"],
            "le directory devono essere ordinate case-insensitive: {:?}", dir_names);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn files_sorted_case_insensitive() {
        let base = make_test_dir("filesort");
        let entries = list_dir(&base).unwrap();
        let file_names: Vec<&str> = entries.iter()
            .filter(|e| !e.is_dir)
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(file_names, vec!["apple.txt", "zebra.txt"],
            "i file devono essere ordinati case-insensitive: {:?}", file_names);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn dotdot_is_first_entry() {
        let base = make_test_dir("dotdot");
        let entries = list_dir(&base).unwrap();
        assert_eq!(entries[0].name, "..", "la prima voce deve essere '..'");
        assert!(entries[0].is_dir, "'..' deve essere marcato come directory");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn entry_label_adds_slash_for_dirs() {
        let dir_entry = Entry { name: "foo".to_string(), is_dir: true, size: 0 };
        let file_entry = Entry { name: "bar.txt".to_string(), is_dir: false, size: 42 };
        assert_eq!(dir_entry.label(), "foo/");
        assert_eq!(file_entry.label(), "bar.txt");
    }

    #[test]
    fn parent_of_returns_parent() {
        let p = PathBuf::from("C:\\Users\\test\\docs");
        let parent = parent_of(&p);
        assert_eq!(parent, PathBuf::from("C:\\Users\\test"));
    }

    // ── Part C: navigazione fra unità disco (drive root ↔ elenco unità) ────────

    /// Regressione (headline): alla radice di un'unità (es. "C:\") deve comparire
    /// ".." come PRIMA voce, così l'utente può risalire all'elenco delle unità.
    /// Prima della fix `Path::parent()` per "C:\" è `None` → ".." veniva omesso e
    /// non c'era alcuna via d'uscita dalla radice del disco.
    #[cfg(windows)]
    #[test]
    fn list_dir_at_drive_root_shows_dotdot() {
        let entries = list_dir(Path::new("C:\\")).expect("C:\\ deve essere elencabile");
        assert_eq!(entries[0].name, "..",
            "la prima voce alla radice dell'unità deve essere '..'");
    }

    /// `is_drive_root` riconosce SOLO la radice di un'unità Windows.
    /// Gate `#[cfg(windows)]`: su non-Windows `is_drive_root` è sempre `false`
    /// (nessun concetto di unità multiple) e i casi "true" non varrebbero.
    #[cfg(windows)]
    #[test]
    fn is_drive_root_detects_only_drive_roots() {
        // Radici di unità (true):
        assert!(is_drive_root(Path::new("C:\\")), "C:\\ è una radice di unità");
        assert!(is_drive_root(Path::new("C:/")),  "C:/ è una radice di unità");
        assert!(is_drive_root(Path::new("C:")),   "C: è una radice di unità");
        // NON radici (false):
        assert!(!is_drive_root(Path::new("C:\\Users")), "una sottodir non è radice");
        assert!(!is_drive_root(Path::new("\\\\server\\share")), "un path UNC non è radice");
        assert!(!is_drive_root(Path::new("relativo\\sub")), "un path relativo non è radice");
        assert!(!is_drive_root(Path::new("")), "il path vuoto (sentinella) non è radice");
    }

    /// `list_dir` sulla sentinella restituisce l'elenco delle unità disco:
    /// nessun ".." (è la cima assoluta) e almeno "C:\\" (garantita in questo
    /// ambiente Windows).
    #[cfg(windows)]
    #[test]
    fn list_dir_on_sentinel_lists_drives_without_dotdot() {
        let entries = list_dir(&drive_list_sentinel()).expect("l'elenco unità non deve fallire");
        assert!(!entries.iter().any(|e| e.name == ".."),
            "l'elenco unità è la cima: NON deve contenere '..'");
        assert!(entries.iter().any(|e| e.name == "C:\\"),
            "C:\\ deve comparire fra le unità (esiste sempre in questo ambiente)");
        assert!(entries.iter().all(|e| e.is_dir),
            "ogni unità è navigabile → marcata come directory");
    }

    /// `parent_of` sulla radice di un'unità porta all'elenco unità (sentinella);
    /// sulla sentinella resta invariato (non si sale oltre la cima).
    #[cfg(windows)]
    #[test]
    fn parent_of_drive_root_and_sentinel_round_trip() {
        assert_eq!(parent_of(Path::new("C:\\")), drive_list_sentinel(),
            "'su' dalla radice dell'unità porta all'elenco unità");
        assert_eq!(parent_of(&drive_list_sentinel()), drive_list_sentinel(),
            "l'elenco unità è la cima: 'su' resta lì");
    }
}

