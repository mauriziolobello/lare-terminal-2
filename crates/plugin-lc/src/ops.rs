//! ops.rs — Operazioni sul filesystem (copia, sposta, mkdir, confronta).
//!
//! Funzioni pure (`compare`) e funzioni con I/O (`folder_compare`, `diff_files`,
//! `copy_item`, `move_item`, `mkdir`), tutte testabili con directory temporanee.

use std::path::Path;
use crate::fs::Entry;

// ─── Confronto listing puro (senza I/O) ───────────────────────────────────────

/// Risultato del confronto tra i listing dei due pannelli (solo nomi).
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub struct CompareResult {
    /// Nomi presenti solo nel pannello sinistro.
    pub only_left: Vec<String>,
    /// Nomi presenti solo nel pannello destro.
    pub only_right: Vec<String>,
    /// Numero di voci presenti in entrambi i pannelli.
    pub common: usize,
}

/// Confronta i listing di due pannelli per nome. Ignora l'entry `..`.
/// Funzione pura: non accede al filesystem.
#[allow(dead_code)]
pub fn compare(left: &[Entry], right: &[Entry]) -> CompareResult {
    use std::collections::HashSet;
    let left_names: HashSet<&str> = left.iter()
        .filter(|e| e.name != "..").map(|e| e.name.as_str()).collect();
    let right_names: HashSet<&str> = right.iter()
        .filter(|e| e.name != "..").map(|e| e.name.as_str()).collect();

    let mut only_left: Vec<String> = left_names.difference(&right_names)
        .map(|s| s.to_string()).collect();
    let mut only_right: Vec<String> = right_names.difference(&left_names)
        .map(|s| s.to_string()).collect();
    let common = left_names.intersection(&right_names).count();

    only_left.sort();
    only_right.sort();
    CompareResult { only_left, only_right, common }
}

// ─── Confronto cartelle con metadati reali ────────────────────────────────────

/// Risultato del confronto tra due cartelle con data/ora di modifica reale.
#[derive(Debug, Clone, PartialEq)]
pub struct FolderCompareResult {
    /// File/dir presenti solo nella cartella sinistra.
    pub only_left:   Vec<String>,
    /// File/dir presenti solo nella cartella destra.
    pub only_right:  Vec<String>,
    /// File presenti in entrambe le cartelle, più recenti a sinistra.
    pub newer_left:  Vec<String>,
    /// File presenti in entrambe le cartelle, più recenti a destra.
    pub newer_right: Vec<String>,
    /// File/dir presenti in entrambe le cartelle con la stessa data.
    pub identical:   Vec<String>,
}

/// Confronta due cartelle leggendo i metadati reali dal filesystem.
///
/// - Rileva differenze per **presenza/assenza** e per **data/ora di modifica**.
/// - Per le directory, non confronta la data (le marca come `identical`).
/// - Non ricorsivo: confronta solo il livello top delle cartelle.
/// - Ignora l'entry `..`.
pub fn folder_compare(left_path: &Path, right_path: &Path) -> std::io::Result<FolderCompareResult> {
    use std::collections::HashMap;
    use std::time::SystemTime;

    fn read_meta(path: &Path) -> HashMap<String, (bool, Option<SystemTime>)> {
        let mut map = HashMap::new();
        let Ok(rd) = std::fs::read_dir(path) else { return map };
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == ".." { continue; }
            let meta   = entry.metadata();
            let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
            let mtime  = meta.ok().and_then(|m| m.modified().ok());
            map.insert(name, (is_dir, mtime));
        }
        map
    }

    let left_map  = read_meta(left_path);
    let right_map = read_meta(right_path);

    let mut only_left   = Vec::new();
    let mut only_right  = Vec::new();
    let mut newer_left  = Vec::new();
    let mut newer_right = Vec::new();
    let mut identical   = Vec::new();

    for (name, (is_dir_l, mtime_l)) in &left_map {
        match right_map.get(name) {
            None => only_left.push(name.clone()),
            Some((is_dir_r, mtime_r)) => {
                if *is_dir_l || *is_dir_r {
                    identical.push(name.clone());
                } else {
                    match (mtime_l, mtime_r) {
                        (Some(ml), Some(mr)) => {
                            if ml > mr      { newer_left.push(name.clone()); }
                            else if mr > ml { newer_right.push(name.clone()); }
                            else            { identical.push(name.clone()); }
                        }
                        _ => identical.push(name.clone()),
                    }
                }
            }
        }
    }
    for name in right_map.keys() {
        if !left_map.contains_key(name) {
            only_right.push(name.clone());
        }
    }

    only_left.sort(); only_right.sort();
    newer_left.sort(); newer_right.sort();
    identical.sort();

    Ok(FolderCompareResult { only_left, only_right, newer_left, newer_right, identical })
}

// ─── Diff file di testo ───────────────────────────────────────────────────────

/// Una riga nel risultato del diff side-by-side.
#[derive(Debug, Clone, PartialEq)]
pub enum DiffLine {
    Same  { line: String },
    Left  { line: String },
    Right { line: String },
}

/// Risultato del diff tra due file di testo.
#[derive(Debug, Clone)]
pub struct FileDiffResult {
    pub left_name:  String,
    pub right_name: String,
    pub lines:      Vec<DiffLine>,
    pub left_only:  usize,
    pub right_only: usize,
    pub same:       usize,
    /// `true` se i file erano > MAX_DIFF_LINES e sono stati troncati.
    pub truncated:  bool,
}

const MAX_DIFF_LINES: usize = 1000;
const MAX_DIFF_BYTES: u64   = 512 * 1024;

/// Calcola il diff tra due file di testo con LCS (Longest Common Subsequence).
///
/// # Errori
/// - File > 512 KB  
/// - File binario (byte NUL)  
/// - Errore I/O
pub fn diff_files(left: &Path, right: &Path) -> std::io::Result<FileDiffResult> {
    let check_size = |p: &Path| -> std::io::Result<()> {
        let len = std::fs::metadata(p)?.len();
        if len > MAX_DIFF_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("File troppo grande (max 512 KB, trovati {} KB)", len / 1024),
            ));
        }
        Ok(())
    };
    check_size(left)?;
    check_size(right)?;

    let left_bytes  = std::fs::read(left)?;
    let right_bytes = std::fs::read(right)?;

    if left_bytes.contains(&0u8) || right_bytes.contains(&0u8) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "File binario: diff di testo non supportato",
        ));
    }

    let left_str  = String::from_utf8_lossy(&left_bytes);
    let right_str = String::from_utf8_lossy(&right_bytes);
    let left_lines:  Vec<&str> = left_str.lines().collect();
    let right_lines: Vec<&str> = right_str.lines().collect();

    let truncated = left_lines.len() > MAX_DIFF_LINES || right_lines.len() > MAX_DIFF_LINES;
    let lhs = &left_lines[..left_lines.len().min(MAX_DIFF_LINES)];
    let rhs = &right_lines[..right_lines.len().min(MAX_DIFF_LINES)];

    let lines      = lcs_diff(lhs, rhs);
    let left_only  = lines.iter().filter(|l| matches!(l, DiffLine::Left  { .. })).count();
    let right_only = lines.iter().filter(|l| matches!(l, DiffLine::Right { .. })).count();
    let same       = lines.iter().filter(|l| matches!(l, DiffLine::Same  { .. })).count();

    let left_name  = left.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let right_name = right.file_name().unwrap_or_default().to_string_lossy().into_owned();

    Ok(FileDiffResult { left_name, right_name, lines, left_only, right_only, same, truncated })
}

/// LCS diff: O(m×n) tempo e spazio, m,n ≤ 1000.
fn lcs_diff(left: &[&str], right: &[&str]) -> Vec<DiffLine> {
    let (m, n) = (left.len(), right.len());
    let mut dp = vec![vec![0u16; n + 1]; m + 1];
    for i in 1..=m {
        for j in 1..=n {
            dp[i][j] = if left[i-1] == right[j-1] {
                dp[i-1][j-1] + 1
            } else {
                dp[i-1][j].max(dp[i][j-1])
            };
        }
    }
    let mut result = Vec::with_capacity(m + n);
    let (mut i, mut j) = (m, n);
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && left[i-1] == right[j-1] {
            result.push(DiffLine::Same  { line: left[i-1].to_string() });
            i -= 1; j -= 1;
        } else if j > 0 && (i == 0 || dp[i][j-1] >= dp[i-1][j]) {
            result.push(DiffLine::Right { line: right[j-1].to_string() });
            j -= 1;
        } else {
            result.push(DiffLine::Left  { line: left[i-1].to_string() });
            i -= 1;
        }
    }
    result.reverse();
    result
}

// ─── Operazioni file ──────────────────────────────────────────────────────────

/// Guardie difensive comuni a `copy_item` e `move_item`.
///
/// Invarianti che ogni sorgente/destinazione legittima deve rispettare:
/// 1. `src` deve avere un `file_name()` concreto. Un path che termina in ".."
///    (o è una radice) non ne ha: operarci significherebbe agire sull'intera
///    directory padre — è esattamente il footgun che ha cancellato dati.
/// 2. `dst` non deve essere un discendente di `src` (né coincidere con esso):
///    copiare una cartella dentro se stessa ricorre finché il path non esplode.
///    `Path::starts_with` confronta per *componenti*, quindi `/a/dir2` NON è
///    considerato "dentro" `/a/dir` (nessun falso positivo su prefissi di nome).
fn validate_src_dst(src: &Path, dst: &Path) -> std::io::Result<()> {
    if src.file_name().is_none() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "impossibile operare su un path senza nome (es. '..')",
        ));
    }
    if dst.starts_with(src) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "impossibile copiare/spostare una cartella dentro se stessa",
        ));
    }
    Ok(())
}

/// Copia `src` in `dst` (file o directory, ricorsiva).
pub fn copy_item(src: &Path, dst: &Path) -> std::io::Result<()> {
    validate_src_dst(src, dst)?;
    if src.is_dir() { copy_dir_all(src, dst) } else { std::fs::copy(src, dst)?; Ok(()) }
}

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let dst_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &dst_path)?;
        } else {
            std::fs::copy(entry.path(), dst_path)?;
        }
    }
    Ok(())
}

/// Sposta `src` in `dst`. Tenta `rename`; se fallisce (cross-device) fa copia + rimozione.
///
/// Le guardie di `validate_src_dst` vanno applicate ESPLICITAMENTE qui: il
/// percorso veloce `rename` non passa da `copy_item`, quindi senza questo check
/// un `..`/copy-into-self salterebbe la validazione (`rename` potrebbe pure
/// riuscire e spostare la directory padre).
pub fn move_item(src: &Path, dst: &Path) -> std::io::Result<()> {
    validate_src_dst(src, dst)?;
    match std::fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(_) => {
            copy_item(src, dst)?;
            if src.is_dir() { std::fs::remove_dir_all(src) } else { std::fs::remove_file(src) }
        }
    }
}

/// Crea `path` (e tutti gli antenati mancanti).
pub fn mkdir(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)
}

/// Elimina `path`: un file direttamente, una directory SOLO se vuota.
/// Nessuna ricorsione — una directory non vuota produce un errore esplicito,
/// non viene mai cancellata (scelta esplicita: eliminare un intero albero è
/// un'operazione troppo distruttiva per un singolo tasto senza un flag
/// aggiuntivo che qui non esiste ancora).
pub fn delete_item(path: &Path) -> std::io::Result<()> {
    if path.is_dir() {
        // `read_dir(path)?.next()` legge la PRIMA voce senza materializzare tutto
        // il listing: se esiste, la directory non è vuota → rifiuta. `remove_dir`
        // (non `remove_dir_all`) elimina solo directory vuote, quindi il controllo
        // esplicito qui serve a dare un messaggio d'errore chiaro in italiano
        // invece dell'errore OS grezzo ("directory non vuota"). `Error::other` è
        // la forma idiomatica per `ErrorKind::Other` (allineata a config.rs dopo
        // la pulizia clippy di round 3).
        let mut entries = std::fs::read_dir(path)?;
        if entries.next().is_some() {
            return Err(std::io::Error::other(
                "la cartella non è vuota — eliminazione ricorsiva non supportata",
            ));
        }
        std::fs::remove_dir(path)
    } else {
        std::fs::remove_file(path)
    }
}

// ─── Test ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use crate::fs::Entry;

    fn ent(name: &str, is_dir: bool) -> Entry {
        Entry { name: name.to_string(), is_dir, size: 0 }
    }

    // ── compare ──────────────────────────────────────────────────────────────

    #[test]
    fn compare_finds_only_left_and_only_right() {
        let left  = vec![ent("..", true), ent("a.txt", false), ent("b.txt", false)];
        let right = vec![ent("..", true), ent("b.txt", false), ent("c.txt", false)];
        let r = compare(&left, &right);
        assert_eq!(r.only_left,  vec!["a.txt"]);
        assert_eq!(r.only_right, vec!["c.txt"]);
        assert_eq!(r.common, 1);
    }

    #[test]
    fn compare_identical_panels() {
        let both = vec![ent("..", true), ent("x.txt", false), ent("y.txt", false)];
        let r = compare(&both, &both);
        assert!(r.only_left.is_empty());
        assert!(r.only_right.is_empty());
        assert_eq!(r.common, 2);
    }

    #[test]
    fn compare_empty_panels() {
        let r = compare(&[], &[]);
        assert!(r.only_left.is_empty());
        assert!(r.only_right.is_empty());
        assert_eq!(r.common, 0);
    }

    #[test]
    fn compare_ignores_dotdot() {
        let left  = vec![ent("..", true), ent("file.txt", false)];
        let right = vec![ent("..", true)];
        let r = compare(&left, &right);
        assert_eq!(r.only_left, vec!["file.txt"]);
        assert!(r.only_right.is_empty());
        assert_eq!(r.common, 0);
    }

    #[test]
    fn compare_result_is_sorted() {
        let left  = vec![ent("zzz.txt", false), ent("aaa.txt", false)];
        let right: Vec<Entry> = vec![];
        let r = compare(&left, &right);
        assert_eq!(r.only_left, vec!["aaa.txt", "zzz.txt"]);
    }

    // ── folder_compare ───────────────────────────────────────────────────────

    fn tempd(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("lc_fc_{tag}"));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn folder_compare_finds_only_sides() {
        let left  = tempd("ol_l"); let right = tempd("ol_r");
        fs::write(left.join("alpha.txt"),   b"a").unwrap();
        fs::write(left.join("common.txt"),  b"c").unwrap();
        fs::write(right.join("beta.txt"),   b"b").unwrap();
        fs::write(right.join("common.txt"), b"c").unwrap();
        let r = folder_compare(&left, &right).unwrap();
        assert_eq!(r.only_left,  vec!["alpha.txt"]);
        assert_eq!(r.only_right, vec!["beta.txt"]);
        let common_found = r.identical.contains(&"common.txt".to_string())
            || r.newer_left.contains(&"common.txt".to_string())
            || r.newer_right.contains(&"common.txt".to_string());
        assert!(common_found, "common.txt deve apparire in identical o newer_*");
        let _ = fs::remove_dir_all(&left); let _ = fs::remove_dir_all(&right);
    }

    #[test]
    fn folder_compare_detects_newer_left() {
        let left = tempd("nl_l"); let right = tempd("nl_r");
        fs::write(right.join("doc.txt"), b"old").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
        fs::write(left.join("doc.txt"),  b"new").unwrap();
        let r = folder_compare(&left, &right).unwrap();
        assert!(r.newer_left.contains(&"doc.txt".to_string())
             || r.identical.contains(&"doc.txt".to_string()),
            "doc.txt deve essere in newer_left o identical");
        let _ = fs::remove_dir_all(&left); let _ = fs::remove_dir_all(&right);
    }

    #[test]
    fn folder_compare_detects_newer_right() {
        let left = tempd("nr_l"); let right = tempd("nr_r");
        fs::write(left.join("doc.txt"),  b"old").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
        fs::write(right.join("doc.txt"), b"new").unwrap();
        let r = folder_compare(&left, &right).unwrap();
        assert!(r.newer_right.contains(&"doc.txt".to_string())
             || r.identical.contains(&"doc.txt".to_string()),
            "doc.txt deve essere in newer_right o identical");
        let _ = fs::remove_dir_all(&left); let _ = fs::remove_dir_all(&right);
    }

    // ── copy / move / mkdir ───────────────────────────────────────────────────

    #[test]
    fn copy_file_works() {
        let base = std::env::temp_dir().join("lc_ops_copy");
        let _ = fs::remove_dir_all(&base); fs::create_dir_all(&base).unwrap();
        let src = base.join("src.txt"); let dst = base.join("dst.txt");
        fs::write(&src, b"hello world").unwrap();
        copy_item(&src, &dst).unwrap();
        assert_eq!(fs::read(&dst).unwrap(), b"hello world");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn copy_dir_works() {
        let base = std::env::temp_dir().join("lc_ops_copydir");
        let _ = fs::remove_dir_all(&base);
        let src_dir = base.join("src"); let dst_dir = base.join("dst");
        fs::create_dir_all(&src_dir).unwrap();
        fs::write(src_dir.join("inner.txt"), b"inner").unwrap();
        copy_item(&src_dir, &dst_dir).unwrap();
        assert!(dst_dir.join("inner.txt").exists());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn move_file_works() {
        let base = std::env::temp_dir().join("lc_ops_move");
        let _ = fs::remove_dir_all(&base); fs::create_dir_all(&base).unwrap();
        let src = base.join("to_move.txt"); let dst = base.join("moved.txt");
        fs::write(&src, b"data").unwrap();
        move_item(&src, &dst).unwrap();
        assert!(dst.exists()); assert!(!src.exists());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn mkdir_creates_directory() {
        let base = std::env::temp_dir().join("lc_ops_mkdir");
        let _ = fs::remove_dir_all(&base);
        let new_dir = base.join("level1").join("level2");
        mkdir(&new_dir).unwrap(); assert!(new_dir.is_dir());
        let _ = fs::remove_dir_all(&base);
    }

    // ── delete_item (round 4) ─────────────────────────────────────────────────

    /// Eliminare un file esistente lo rimuove dal disco e ritorna `Ok(())`.
    #[test]
    fn delete_item_removes_file() {
        let base = std::env::temp_dir().join("lc_ops_del_file");
        let _ = fs::remove_dir_all(&base); fs::create_dir_all(&base).unwrap();
        let f = base.join("target.txt");
        fs::write(&f, b"da eliminare").unwrap();
        assert!(f.exists(), "precondizione: il file esiste");
        delete_item(&f).unwrap();
        assert!(!f.exists(), "il file deve essere stato eliminato");
        let _ = fs::remove_dir_all(&base);
    }

    /// Eliminare una directory VUOTA la rimuove e ritorna `Ok(())`.
    #[test]
    fn delete_item_removes_empty_directory() {
        let base = std::env::temp_dir().join("lc_ops_del_empty");
        let _ = fs::remove_dir_all(&base); fs::create_dir_all(&base).unwrap();
        let d = base.join("vuota");
        fs::create_dir(&d).unwrap();
        assert!(d.is_dir(), "precondizione: la directory vuota esiste");
        delete_item(&d).unwrap();
        assert!(!d.exists(), "la directory vuota deve essere stata eliminata");
        let _ = fs::remove_dir_all(&base);
    }

    /// Eliminare una directory NON vuota deve fallire (niente ricorsione) e NON
    /// toccare né la directory né il suo contenuto.
    #[test]
    fn delete_item_refuses_non_empty_directory() {
        let base = std::env::temp_dir().join("lc_ops_del_nonempty");
        let _ = fs::remove_dir_all(&base); fs::create_dir_all(&base).unwrap();
        let d = base.join("piena");
        fs::create_dir(&d).unwrap();
        let inner = d.join("dentro.txt");
        fs::write(&inner, b"contenuto").unwrap();

        let err = delete_item(&d).unwrap_err();
        // La directory e il suo contenuto devono sopravvivere (non-cancellazione,
        // non solo l'errore).
        assert!(d.exists(),     "la directory non vuota NON deve essere eliminata");
        assert!(inner.exists(), "il contenuto della directory deve sopravvivere");
        assert!(err.to_string().to_lowercase().contains("vuota"),
            "il messaggio d'errore deve spiegare che la cartella non è vuota (trovato: {err})");
        let _ = fs::remove_dir_all(&base);
    }

    // ── Fix 1: guardie difensive di copy_item / move_item ─────────────────────

    /// Un `src` che termina in ".." non ha `file_name()` → operazione ambigua e
    /// pericolosa (opererebbe sull'intera directory padre). Deve essere rifiutata.
    #[test]
    fn copy_item_rejects_src_without_filename() {
        let base = std::env::temp_dir().join("lc_ops_nofn_copy");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("inner")).unwrap(); // base contiene solo "inner" vuota
        let dst = std::env::temp_dir().join("lc_ops_nofn_copy_dst");
        let _ = fs::remove_dir_all(&dst);

        let src = base.join("inner").join(".."); // = base, ma file_name() == None
        assert_eq!(src.file_name(), None, "il src costruito non deve avere file_name");
        let err = copy_item(&src, &dst).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);

        let _ = fs::remove_dir_all(&base);
        let _ = fs::remove_dir_all(&dst);
    }

    #[test]
    fn move_item_rejects_src_without_filename() {
        let base = std::env::temp_dir().join("lc_ops_nofn_move");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("inner")).unwrap();
        let dst = std::env::temp_dir().join("lc_ops_nofn_move_dst");
        let _ = fs::remove_dir_all(&dst);

        let src = base.join("inner").join("..");
        let err = move_item(&src, &dst).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);

        let _ = fs::remove_dir_all(&base);
        let _ = fs::remove_dir_all(&dst);
    }

    /// `dst` discendente di `src` → copierebbe ricorsivamente dentro se stesso.
    /// Usiamo un `src` FILE (dst "dentro" un file): il predicato di guardia
    /// `dst.starts_with(src)` è identico per file e directory, ma con un file il
    /// codice pre-fix fallisce SUBITO (il genitore non è una dir) invece di
    /// ricorrere all'infinito — RED sicuro senza il runaway confermato dal probe.
    #[test]
    fn copy_item_rejects_dst_inside_src() {
        let base = std::env::temp_dir().join("lc_ops_inside_copy");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let src = base.join("thing.txt");
        fs::write(&src, b"x").unwrap();
        let dst = src.join("child"); // = base/thing.txt/child (discendente di src)
        assert!(dst.starts_with(&src));
        let err = copy_item(&src, &dst).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn move_item_rejects_dst_inside_src() {
        let base = std::env::temp_dir().join("lc_ops_inside_move");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let src = base.join("thing.txt");
        fs::write(&src, b"x").unwrap();
        let dst = src.join("child");
        let err = move_item(&src, &dst).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        let _ = fs::remove_dir_all(&base);
    }

    /// Scenario reale del task: copiare una CARTELLA dentro una sua sottocartella.
    /// (NB: il RED di questo caso NON è stato eseguito sul codice pre-fix perché
    /// ricorrerebbe all'infinito — hazard confermato empiricamente dal probe; il
    /// predicato di guardia è dimostrato in RED dalla variante file-src qui sopra.
    /// Post-fix la guardia cortocircuita PRIMA di qualsiasi ricorsione → sicuro.)
    #[test]
    fn copy_item_rejects_folder_into_own_subfolder() {
        let base = std::env::temp_dir().join("lc_ops_folder_into_self_copy");
        let _ = fs::remove_dir_all(&base);
        let src = base.join("mydir");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("a.txt"), b"x").unwrap();
        let dst = src.join("copy"); // sottocartella di src
        let err = copy_item(&src, &dst).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn move_item_rejects_folder_into_own_subfolder() {
        let base = std::env::temp_dir().join("lc_ops_folder_into_self_move");
        let _ = fs::remove_dir_all(&base);
        let src = base.join("mydir");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("a.txt"), b"x").unwrap();
        let dst = src.join("copy");
        let err = move_item(&src, &dst).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        let _ = fs::remove_dir_all(&base);
    }

    // ── diff_files ────────────────────────────────────────────────────────────

    fn tmp_file(name: &str, content: &[u8]) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(name);
        fs::write(&p, content).unwrap();
        p
    }

    #[test]
    fn diff_identical_files_no_diffs() {
        let l = tmp_file("lc_diff_id_l.txt", b"aaa\nbbb\nccc\n");
        let r = tmp_file("lc_diff_id_r.txt", b"aaa\nbbb\nccc\n");
        let d = diff_files(&l, &r).unwrap();
        assert_eq!(d.left_only, 0); assert_eq!(d.right_only, 0); assert_eq!(d.same, 3);
        let _ = fs::remove_file(&l); let _ = fs::remove_file(&r);
    }

    #[test]
    fn diff_added_line_right() {
        let l = tmp_file("lc_diff_addr_l.txt", b"aaa\nccc\n");
        let r = tmp_file("lc_diff_addr_r.txt", b"aaa\nbbb\nccc\n");
        let d = diff_files(&l, &r).unwrap();
        assert_eq!(d.right_only, 1); assert_eq!(d.left_only, 0);
        let _ = fs::remove_file(&l); let _ = fs::remove_file(&r);
    }

    #[test]
    fn diff_removed_line_left() {
        let l = tmp_file("lc_diff_reml_l.txt", b"aaa\nbbb\nccc\n");
        let r = tmp_file("lc_diff_reml_r.txt", b"aaa\nccc\n");
        let d = diff_files(&l, &r).unwrap();
        assert_eq!(d.left_only, 1); assert_eq!(d.right_only, 0);
        let _ = fs::remove_file(&l); let _ = fs::remove_file(&r);
    }

    #[test]
    fn diff_binary_returns_error() {
        let l = tmp_file("lc_diff_bin_l.bin", b"hello\x00world");
        let r = tmp_file("lc_diff_bin_r.bin", b"hello\x00world");
        assert_eq!(diff_files(&l, &r).unwrap_err().kind(), std::io::ErrorKind::InvalidData);
        let _ = fs::remove_file(&l); let _ = fs::remove_file(&r);
    }

    #[test]
    fn diff_large_returns_error() {
        let big: Vec<u8> = b"x\n".repeat(300_000);
        let l = tmp_file("lc_diff_big_l.txt", &big);
        let r = tmp_file("lc_diff_big_r.txt", b"small\n");
        assert_eq!(diff_files(&l, &r).unwrap_err().kind(), std::io::ErrorKind::InvalidData);
        let _ = fs::remove_file(&l); let _ = fs::remove_file(&r);
    }
}
