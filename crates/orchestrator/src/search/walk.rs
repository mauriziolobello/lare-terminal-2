//! # search::walk
//!
//! Async directory walker for a single search root.
//!
//! `walk_root` runs `walkdir` inside `spawn_blocking` and streams `Hit` values
//! over a `tokio::sync::mpsc::Sender<Hit>`. The caller (Task 6 — `SearchEngine`)
//! spawns one task per `Root` and drains the shared receiver.
//!
//! ## Filter order (all applied via `filter_entry`)
//! 1. **prune**: any directory whose path equals an entry in `root.prune`
//!    is skipped entirely (no descent) — these sub-trees are walked as their own roots.
//! 2. **exclude**: any entry (file or dir) whose *name component* is in `exclude`
//!    is skipped entirely (common examples: `node_modules`, `.git`).
//! 3. **depth**: WalkDir's built-in `max_depth` gate.
//!
//! ## Windows verbatim paths
//! `std::fs::canonicalize` on Windows returns verbatim paths (`\\?\C:\...`).
//! `normalize_path_string` strips the verbatim prefix so the UI and `/open`
//! receive clean paths like `C:\Users\...`.

use std::path::PathBuf;
use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::search::content::{classify_extension, ContentConfig, ContentMatcher, ExtensionClass};
use crate::search::pause::PauseGate;
use crate::search::query::Matcher;
use crate::search::roots::Root;
use protocol::SearchSource;

// ─────────────────────────────────────────────────────────────────────────────
// Hit
// ─────────────────────────────────────────────────────────────────────────────

/// A single file-search result.
pub struct Hit {
    /// Absolute path of the matched file (verbatim prefix stripped on Windows).
    pub path: String,
    /// The search root that produced this hit.
    pub source: SearchSource,
    /// Numero di riga (1-based) del match — `Some` solo per un hit di ricerca
    /// CONTENUTO, `None` per un hit solo-nome.
    pub line: Option<u32>,
    /// Testo (troncato) della riga che matcha — `Some` solo per un hit di
    /// ricerca CONTENUTO.
    pub snippet: Option<String>,
}

/// Dipendenze condivise per la ricerca CONTENUTO durante un walk. Un solo
/// `Arc<ContentSearch>` è condiviso (via `.clone()`, economico) fra tutti i
/// walker di una ricerca — `unknown_tx` è lo stesso `Sender` sottostante per
/// tutti: i file `Unknown` di QUALUNQUE root finiscono nello stesso canale,
/// drenato dalla Fase B in `mod.rs`.
pub struct ContentSearch {
    pub matcher: ContentMatcher,
    pub cfg: ContentConfig,
    pub unknown_tx: tokio::sync::mpsc::Sender<(PathBuf, SearchSource)>,
}

// ─────────────────────────────────────────────────────────────────────────────
// normalize_path_string
// ─────────────────────────────────────────────────────────────────────────────

/// Converts a `Path` to a `String`, stripping Windows verbatim prefixes.
///
/// | Input prefix            | Output prefix  |
/// |-------------------------|----------------|
/// | `\\?\UNC\server\...`    | `\\server\...` |
/// | `\\?\C:\...`            | `C:\...`       |
/// | anything else           | unchanged      |
///
/// The function is pure and cross-platform: it operates on the string
/// representation, so it can be unit-tested with synthetic strings on any OS.
///
/// IMPORTANT: check the UNC prefix BEFORE the bare verbatim prefix, because
/// `\\?\UNC\` starts with `\\?\`.
pub fn normalize_path_string(p: &std::path::Path) -> String {
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        // \\?\UNC\server\share  →  \\server\share
        format!(r"\\{rest}")
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        // \\?\C:\...  →  C:\...
        rest.to_string()
    } else {
        s.into_owned()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// walk_root
// ─────────────────────────────────────────────────────────────────────────────

/// Walks `root` asynchronously (via `spawn_blocking`) and sends every matching
/// file as a `Hit` on `tx`.
///
/// Terminates when:
/// - the directory walk is exhausted, OR
/// - `cancel` is triggered (checked every iteration), OR
/// - `tx` is closed (receiver dropped — `blocking_send` returns `Err`).
///
/// # Parameters
/// - `root`    — the root to walk; `root.prune` lists sub-trees to skip;
///   `root.max_depth` is passed to `WalkDir::max_depth`.
/// - `matcher` — compiled query matcher (shared across concurrent walkers).
/// - `exclude` — directory/file name components to skip (e.g. `"node_modules"`).
/// - `tx`      — hit sink; `blocking_send` is safe inside `spawn_blocking`.
/// - `cancel`  — cooperative cancellation token (checked every loop iteration).
/// - `gate`    — pause gate; the walker blocks here when paused, resumes on `gate.resume()`.
/// - `content` — dipendenze di ricerca CONTENUTO, `None` per la ricerca solo-nome
///   (comportamento invariato: hit immediato su match nome). `Some` attiva il
///   filtro AND nome+contenuto per i file `Text`, scarta i `Binary`, accoda gli
///   `Unknown` su `content.unknown_tx` per la Fase B (vedi `mod.rs`).
pub async fn walk_root(
    root: Root,
    matcher: Arc<Matcher>,
    exclude: Arc<Vec<String>>,
    tx: tokio::sync::mpsc::Sender<Hit>,
    cancel: CancellationToken,
    gate: Arc<PauseGate>,
    content: Option<Arc<ContentSearch>>,
) {
    tokio::task::spawn_blocking(move || {
        use walkdir::WalkDir;

        let walker = WalkDir::new(&root.path).max_depth(root.max_depth).into_iter();

        // filter_entry: returning `false` prevents WalkDir from both yielding
        // the entry AND descending into it (for directories). This is more
        // efficient than a manual skip in the loop body for excluded/pruned dirs.
        for entry in walker.filter_entry(|e| {
            let name = e.file_name().to_string_lossy();

            // ── Prune: skip sub-trees that belong to other roots. ──────────
            // These paths were built from the same tempdir/canonicalized base
            // as root.path, so direct path equality is reliable.
            if e.file_type().is_dir() {
                let entry_path = e.path();
                for pruned in root.prune.iter() {
                    if entry_path == pruned.as_path() {
                        return false; // do not descend
                    }
                }
            }

            // ── Exclude: skip entries whose name is in the exclusion list. ──
            if exclude.iter().any(|ex| ex.as_str() == name.as_ref()) {
                return false;
            }

            true
        }) {
            // ── Pause: park the blocking thread until resumed or cancelled. ──
            // Called BEFORE the cancel check so a resume after cancel still exits.
            gate.wait_while_paused(&cancel);

            // Check cancellation at the top of every iteration (cooperative).
            if cancel.is_cancelled() {
                break;
            }

            // Skip unreadable entries rather than crashing the whole walk.
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            // Only emit hits for regular files.
            if !entry.file_type().is_file() {
                continue;
            }

            // Match on the file name component only (not the full path).
            let file_name = entry.file_name().to_string_lossy();
            if matcher.is_match(file_name.as_ref()) {
                match &content {
                    // Nessuna content-search: comportamento INVARIATO (hit immediato).
                    None => {
                        let hit = Hit {
                            path: normalize_path_string(entry.path()),
                            source: root.source.clone(),
                            line: None,
                            snippet: None,
                        };
                        // `blocking_send` blocks until there is capacity or the
                        // receiver is dropped. If Err → receiver closed → stop walking.
                        if tx.blocking_send(hit).is_err() {
                            break;
                        }
                    }
                    // Content-search attiva: il match sul nome non basta, serve
                    // anche il match sul contenuto (o l'accodamento per la Fase B).
                    Some(cs) => match classify_extension(entry.path(), &cs.cfg) {
                        ExtensionClass::Text => {
                            if let Some((line, snippet)) =
                                cs.matcher.find_first_match(entry.path(), cs.cfg.max_file_size_kb)
                            {
                                let hit = Hit {
                                    path: normalize_path_string(entry.path()),
                                    source: root.source.clone(),
                                    line: Some(line),
                                    snippet: Some(snippet),
                                };
                                if tx.blocking_send(hit).is_err() {
                                    break;
                                }
                            }
                            // Nome match ma contenuto no ⇒ nessun hit (filtro AND).
                        }
                        // Binario noto: mai aperto, mai accodato.
                        ExtensionClass::Binary => {}
                        // Né testo né binario noto: accodato per la Fase B (mod.rs).
                        ExtensionClass::Unknown => {
                            let _ = cs
                                .unknown_tx
                                .blocking_send((entry.path().to_path_buf(), root.source.clone()));
                        }
                    },
                }
            }
        }
    })
    .await
    // spawn_blocking panics are not expected; ignore the JoinError.
    .ok();
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::content::{ContentConfig, ContentMatcher};
    use crate::search::pause::PauseGate;
    use crate::search::query::parse_query;
    use protocol::SearchSource;
    use std::sync::Arc;
    use tempfile::TempDir;
    use tokio::sync::mpsc;
    use tokio_util::sync::CancellationToken;

    // ── Helper: ContentConfig minima per i test (classifica .txt=Text, .bin=Binary) ──

    fn test_content_cfg() -> ContentConfig {
        ContentConfig {
            text_extensions: vec!["txt".to_string()],
            binary_extensions: vec!["bin".to_string()],
            max_file_size_kb: 5120,
            max_unknown_scan: 100,
            version: 1,
        }
    }

    // ── Helpers ──────────────────────────────────────────────────────────────

    /// Create a file at `dir/rel_path`, making all intermediate directories.
    fn create_file(dir: &std::path::Path, rel_path: &str) {
        let full = dir.join(rel_path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&full, b"").unwrap();
    }

    /// Run a walk and collect the *file name* component of every hit.
    async fn collect_hit_names(
        root: Root,
        matcher: Arc<Matcher>,
        exclude: Arc<Vec<String>>,
        cancel: CancellationToken,
    ) -> std::collections::HashSet<String> {
        let (tx, mut rx) = mpsc::channel::<Hit>(128);
        let gate = PauseGate::new(); // not paused — transparent for existing tests
        tokio::spawn(walk_root(root, matcher, exclude, tx, cancel, gate, None));
        let mut names = std::collections::HashSet::new();
        while let Some(hit) = rx.recv().await {
            let name = std::path::Path::new(&hit.path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            names.insert(name);
        }
        names
    }

    // ── normalize_path_string ────────────────────────────────────────────────

    /// Verbatim prefix `\\?\C:\x\y` must become `C:\x\y`.
    #[test]
    fn normalize_strips_verbatim_prefix() {
        let p = std::path::Path::new(r"\\?\C:\x\y");
        assert_eq!(normalize_path_string(p), r"C:\x\y");
    }

    /// Verbatim UNC `\\?\UNC\server\share` must become `\\server\share`.
    #[test]
    fn normalize_strips_verbatim_unc_prefix() {
        let p = std::path::Path::new(r"\\?\UNC\server\share");
        assert_eq!(normalize_path_string(p), r"\\server\share");
    }

    /// A normal (non-verbatim) path must pass through unchanged.
    #[test]
    fn normalize_leaves_normal_path_unchanged() {
        let p = std::path::Path::new(r"C:\Users\foo\bar.pdf");
        assert_eq!(normalize_path_string(p), r"C:\Users\foo\bar.pdf");
    }

    // ── walk_root — depth and exclude ───────────────────────────────────────

    /// Tree layout (depth from root):
    ///   root/a.pdf              depth=1  ← MATCH
    ///   root/sub/b.pdf          depth=2  ← MATCH
    ///   root/node_modules/c.pdf depth=2  ← excluded by name
    ///   root/x/y/z.pdf          depth=3  ← beyond max_depth=2
    #[tokio::test]
    async fn finds_matching_names_respecting_depth_and_exclude() {
        let tmp = TempDir::new().unwrap();
        let root_path = tmp.path().to_path_buf();

        create_file(&root_path, "a.pdf");
        create_file(&root_path, "sub/b.pdf");
        create_file(&root_path, "node_modules/c.pdf");
        create_file(&root_path, "x/y/z.pdf"); // depth 3

        let root = Root {
            path: root_path,
            source: SearchSource::Cwd,
            prune: vec![],
            max_depth: 2,
        };

        let names = collect_hit_names(
            root,
            Arc::new(parse_query("*.pdf")),
            Arc::new(vec!["node_modules".to_string()]),
            CancellationToken::new(),
        )
        .await;

        assert!(names.contains("a.pdf"), "expected a.pdf; got {names:?}");
        assert!(names.contains("b.pdf"), "expected b.pdf; got {names:?}");
        assert!(!names.contains("c.pdf"), "c.pdf should be excluded; got {names:?}");
        assert!(!names.contains("z.pdf"), "z.pdf should be beyond max_depth; got {names:?}");
    }

    // ── walk_root — prune ────────────────────────────────────────────────────

    /// A sub-tree listed in `root.prune` must be skipped entirely.
    ///
    /// Tree:
    ///   root/outside.pdf     ← MATCH
    ///   root/proj/inside.pdf ← inside pruned sub-tree → NOT emitted
    #[tokio::test]
    async fn prune_skips_subtree() {
        let tmp = TempDir::new().unwrap();
        let root_path = tmp.path().to_path_buf();
        let proj_path = root_path.join("proj");

        create_file(&root_path, "outside.pdf");
        create_file(&root_path, "proj/inside.pdf");

        let root = Root {
            path: root_path,
            source: SearchSource::Standard,
            // prune uses the same base as root.path (both are tmp.path()-relative)
            // so filter_entry's e.path() == pruned comparison works.
            prune: vec![proj_path],
            max_depth: 8,
        };

        let names = collect_hit_names(
            root,
            Arc::new(parse_query("*.pdf")),
            Arc::new(vec![]),
            CancellationToken::new(),
        )
        .await;

        assert!(names.contains("outside.pdf"), "outside.pdf should be found; got {names:?}");
        assert!(!names.contains("inside.pdf"), "inside.pdf should be pruned; got {names:?}");
    }

    // ── walk_root — cancellation ─────────────────────────────────────────────

    /// When the token is cancelled before the walk starts, no (or very few) hits
    /// arrive and the function returns without hanging.
    #[tokio::test]
    async fn cancellation_stops_walk() {
        let tmp = TempDir::new().unwrap();
        let root_path = tmp.path().to_path_buf();

        // Create enough files that a non-cancelled walk would definitely emit many.
        for i in 0..50 {
            create_file(&root_path, &format!("file{i}.pdf"));
        }

        let root = Root {
            path: root_path,
            source: SearchSource::Cwd,
            prune: vec![],
            max_depth: 8,
        };

        let cancel = CancellationToken::new();
        // Cancel BEFORE starting the walk.
        cancel.cancel();

        let (tx, mut rx) = mpsc::channel::<Hit>(128);
        let gate = PauseGate::new();
        tokio::spawn(walk_root(
            root,
            Arc::new(parse_query("*.pdf")),
            Arc::new(vec![]),
            tx,
            cancel,
            gate,
            None,
        ));

        let mut count = 0usize;
        while rx.recv().await.is_some() {
            count += 1;
        }

        // The key property: the future resolves (no hang).
        // With cancellation set before the walk, nearly all hits are skipped.
        assert!(count < 50, "expected cancellation to stop most hits, got {count}");
    }

    // ── walk_root — content search ───────────────────────────────────────────

    /// File `Text` (.txt) che matcha per NOME e per CONTENUTO ⇒ hit con line+snippet.
    #[tokio::test]
    async fn text_extension_matching_content_emits_hit_with_snippet() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("note.txt"), "riga uno\ncontiene TOTALE qui\n").unwrap();

        let root = Root { path: tmp.path().to_path_buf(), source: SearchSource::Cwd, prune: vec![], max_depth: 8 };
        let (unknown_tx, mut unknown_rx) = mpsc::channel(8);
        let content = Arc::new(crate::search::walk::ContentSearch {
            matcher: ContentMatcher::new("totale"),
            cfg: test_content_cfg(),
            unknown_tx,
        });

        let (tx, mut rx) = mpsc::channel::<Hit>(8);
        let gate = PauseGate::new();
        walk_root(root, Arc::new(parse_query("*.txt")), Arc::new(vec![]), tx, CancellationToken::new(), gate, Some(content)).await;

        let hit = rx.recv().await.expect("atteso un hit");
        assert_eq!(hit.line, Some(2));
        assert_eq!(hit.snippet.as_deref(), Some("contiene TOTALE qui"));
        assert!(rx.recv().await.is_none(), "un solo hit atteso");
        assert!(unknown_rx.recv().await.is_none(), "nessun file Unknown in questo test");
    }

    /// File `Text` (.txt) che matcha per NOME ma NON per contenuto ⇒ nessun hit.
    #[tokio::test]
    async fn text_extension_not_matching_content_emits_no_hit() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("note.txt"), "niente di interessante qui\n").unwrap();

        let root = Root { path: tmp.path().to_path_buf(), source: SearchSource::Cwd, prune: vec![], max_depth: 8 };
        let (unknown_tx, _unknown_rx) = mpsc::channel(8);
        let content = Arc::new(crate::search::walk::ContentSearch {
            matcher: ContentMatcher::new("totale"),
            cfg: test_content_cfg(),
            unknown_tx,
        });

        let (tx, mut rx) = mpsc::channel::<Hit>(8);
        let gate = PauseGate::new();
        walk_root(root, Arc::new(parse_query("*.txt")), Arc::new(vec![]), tx, CancellationToken::new(), gate, Some(content)).await;

        assert!(rx.recv().await.is_none(), "nome match ma contenuto no ⇒ zero hit");
    }

    /// File `Binary` (.bin) che matcha per nome ⇒ mai aperto, nessun hit, mai accodato.
    #[tokio::test]
    async fn binary_extension_is_never_opened_or_queued() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("data.bin"), "TOTALE anche se scritto qui non conta\n").unwrap();

        let root = Root { path: tmp.path().to_path_buf(), source: SearchSource::Cwd, prune: vec![], max_depth: 8 };
        let (unknown_tx, mut unknown_rx) = mpsc::channel(8);
        let content = Arc::new(crate::search::walk::ContentSearch {
            matcher: ContentMatcher::new("totale"),
            cfg: test_content_cfg(),
            unknown_tx,
        });

        let (tx, mut rx) = mpsc::channel::<Hit>(8);
        let gate = PauseGate::new();
        walk_root(root, Arc::new(parse_query("*.bin")), Arc::new(vec![]), tx, CancellationToken::new(), gate, Some(content)).await;

        assert!(rx.recv().await.is_none(), "binario ⇒ zero hit");
        assert!(unknown_rx.try_recv().is_err(), "binario ⇒ mai accodato per la Fase B");
    }

    /// File `Unknown` (estensione non classificata) che matcha per nome ⇒ NON un hit
    /// diretto, ma accodato su `unknown_tx` per la Fase B.
    #[tokio::test]
    async fn unknown_extension_is_queued_not_hit_directly() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("mystery.xyz"), "contiene TOTALE ma estensione ignota\n").unwrap();

        let root = Root { path: tmp.path().to_path_buf(), source: SearchSource::Cwd, prune: vec![], max_depth: 8 };
        let (unknown_tx, mut unknown_rx) = mpsc::channel(8);
        let content = Arc::new(crate::search::walk::ContentSearch {
            matcher: ContentMatcher::new("totale"),
            cfg: test_content_cfg(),
            unknown_tx,
        });

        let (tx, mut rx) = mpsc::channel::<Hit>(8);
        let gate = PauseGate::new();
        walk_root(root, Arc::new(parse_query("*.xyz")), Arc::new(vec![]), tx, CancellationToken::new(), gate, Some(content)).await;

        assert!(rx.recv().await.is_none(), "unknown ⇒ zero hit diretto in Fase A");
        let (queued_path, queued_source) = unknown_rx.recv().await.expect("atteso un file accodato");
        assert!(queued_path.ends_with("mystery.xyz"));
        assert_eq!(queued_source, SearchSource::Cwd);
    }

    /// Senza content-search (`content: None`), comportamento IDENTICO a prima:
    /// hit immediato su match nome, `line`/`snippet` sempre `None`.
    #[tokio::test]
    async fn no_content_search_behaves_like_before() {
        let tmp = TempDir::new().unwrap();
        create_file(tmp.path(), "a.pdf");

        let root = Root { path: tmp.path().to_path_buf(), source: SearchSource::Cwd, prune: vec![], max_depth: 8 };
        let (tx, mut rx) = mpsc::channel::<Hit>(8);
        let gate = PauseGate::new();
        walk_root(root, Arc::new(parse_query("*.pdf")), Arc::new(vec![]), tx, CancellationToken::new(), gate, None).await;

        let hit = rx.recv().await.expect("atteso un hit");
        assert_eq!(hit.line, None);
        assert_eq!(hit.snippet, None);
    }
}
