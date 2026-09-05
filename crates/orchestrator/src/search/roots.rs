//! # search::roots
//!
//! Resolves the search root directories in priority order: Cwd → Standard → Cloud → External.
//! Handles canonicalization, exact-duplicate dropping, and computes `prune` sets so that nested
//! roots are walked at their own priority without being re-scanned by an ancestor root.

use std::path::{Path, PathBuf};

use protocol::SearchSource;

use crate::search::paths_config::{PathProvider, PathsConfig};

// ─────────────────────────────────────────────────────────────────────────────
// Root
// ─────────────────────────────────────────────────────────────────────────────

/// A resolved search root, ready for a directory walk.
#[derive(Debug, Clone, PartialEq)]
pub struct Root {
    /// Canonicalized absolute path.
    pub path: PathBuf,

    /// Where this root came from (determines walk priority ordering).
    pub source: SearchSource,

    /// Other roots that are strict descendants of this root.
    ///
    /// When walking `self.path`, the walker must NOT descend into these
    /// sub-trees: they will be walked as their own roots at their own priority.
    /// This avoids double-scanning and preserves the priority invariant
    /// (e.g. cwd stays its own root even when it lives inside a cloud folder).
    pub prune: Vec<PathBuf>,

    /// Per-root WalkDir depth budget (top-level roots get cfg.max_depth).
    pub max_depth: usize,
}

// ─────────────────────────────────────────────────────────────────────────────
// resolve_roots
// ─────────────────────────────────────────────────────────────────────────────

/// Resolves the ordered list of search roots for a file-search operation.
///
/// ## Algorithm
///
/// 1. Build candidates in priority order:
///    `[(cwd, Cwd)]` ++ `cfg.expanded_standard()` → Standard
///    ++ `cfg.expanded_cloud()` → Cloud
///    ++ `cfg.expanded_external(provider)` → External.
///    When `only_cwd` is `true`, candidates are limited to just the cwd,
///    skipping Standard/Cloud/External entirely.
/// 2. Canonicalize each path (`std::fs::canonicalize`, best-effort).
///    Paths that cannot be canonicalized (non-existent) are discarded.
/// 3. Drop exact duplicates, keeping the first occurrence (highest priority).
/// 4. Fan-out one level: each survivor's immediate non-excluded, non-symlink
///    subdirectories that are not already a survivor become additional roots
///    with `max_depth - 1`, enabling parallel walks of large roots.
/// 5. For every unit (survivor + fanned children), compute `prune` = the
///    canonicalized paths of other units that are strict descendants
///    (`x.starts_with(R) && x != R`).
/// 6. Return `Vec<Root>` in priority order.
///
/// ## Nesting semantics
///
/// When root A contains root B as a descendant, **both** are kept. The walker
/// for A must skip B's sub-tree (listed in `A.prune`), while B is walked
/// independently at its own (higher) priority. This preserves priorities and
/// prevents double-scanning.
pub fn resolve_roots(cwd: &Path, provider: &dyn PathProvider, cfg: &PathsConfig, only_cwd: bool) -> Vec<Root> {
    // Step 1: build candidates in priority order. `only_cwd` (da `/find
    // folder:from-here`) salta del tutto la costruzione dei candidati
    // Standard/Cloud/External — non li costruisce nemmeno come candidati,
    // non li filtra dopo (stesso principio del filtro `content` in
    // `walk.rs`: la decisione va presa il più a monte possibile).
    let mut candidates: Vec<(PathBuf, SearchSource)> = Vec::new();

    candidates.push((cwd.to_path_buf(), SearchSource::Cwd));

    if !only_cwd {
        for p in cfg.expanded_standard() {
            candidates.push((p, SearchSource::Standard));
        }
        for p in cfg.expanded_cloud() {
            candidates.push((p, SearchSource::Cloud));
        }
        for p in cfg.expanded_external(provider) {
            candidates.push((p, SearchSource::External));
        }
    }

    // Step 2 + 3: canonicalize and drop exact duplicates, keeping the first
    // occurrence (highest priority wins).
    let mut seen = std::collections::HashSet::<PathBuf>::new();
    let survivors: Vec<(PathBuf, SearchSource)> = candidates
        .into_iter()
        .filter_map(|(p, src)| {
            // canonicalize returns Err for non-existent paths → discard them.
            let canon = std::fs::canonicalize(&p).ok()?;
            if seen.insert(canon.clone()) {
                Some((canon, src))
            } else {
                None // exact duplicate — drop
            }
        })
        .collect();

    // Step 3.5: fan-out one level. Each survivor keeps cfg.max_depth; its
    // immediate, non-excluded, non-symlink subdirectories that are not already
    // a survivor become sub-units with cfg.max_depth - 1. read_dir failures
    // leave the survivor un-expanded (walked whole, as before).
    let mut units: Vec<(PathBuf, SearchSource, usize)> = survivors
        .iter()
        .map(|(p, s)| (p.clone(), s.clone(), cfg.max_depth))
        .collect();

    if cfg.max_depth > 0 {
        let existing: std::collections::HashSet<PathBuf> =
            survivors.iter().map(|(p, _)| p.clone()).collect();
        let child_depth = cfg.max_depth - 1;
        for (sp, ssrc) in &survivors {
            let rd = match std::fs::read_dir(sp) {
                Ok(rd) => rd,
                Err(_) => continue, // un-readable root → keep it un-expanded
            };
            for entry in rd.flatten() {
                let ft = match entry.file_type() {
                    Ok(t) => t,
                    Err(_) => continue,
                };
                if !ft.is_dir() || ft.is_symlink() {
                    continue; // only real subdirectories; never follow reparse points
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                if cfg.exclude.iter().any(|ex| ex == &name) {
                    continue; // excluded by name → must NOT become its own root
                }
                let child = entry.path();
                if existing.contains(&child) {
                    continue; // already a root (e.g. cwd inside this folder)
                }
                units.push((child, ssrc.clone(), child_depth));
            }
        }
    }

    // Step 4: prune over the full unit set (survivors + fanned children).
    let all_paths: Vec<PathBuf> = units.iter().map(|(p, _, _)| p.clone()).collect();
    units
        .into_iter()
        .map(|(root_path, source, max_depth)| {
            let prune: Vec<PathBuf> = all_paths
                .iter()
                .filter(|x| *x != &root_path && x.starts_with(&root_path))
                .cloned()
                .collect();
            Root { path: root_path, source, prune, max_depth }
        })
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::SearchSource;
    use tempfile::TempDir;

    // ── Fake PathProvider ─────────────────────────────────────────────────────

    struct Fake {
        standard: Vec<PathBuf>,
        cloud: Vec<PathBuf>,
        external: Vec<PathBuf>,
    }

    impl Fake {
        fn new(
            standard: Vec<PathBuf>,
            cloud: Vec<PathBuf>,
            external: Vec<PathBuf>,
        ) -> Self {
            Self { standard, cloud, external }
        }
    }

    impl PathProvider for Fake {
        fn standard_dirs(&self) -> Vec<PathBuf> { self.standard.clone() }
        fn detect_cloud(&self) -> Vec<PathBuf>  { self.cloud.clone() }
        fn external_drives(&self) -> Vec<PathBuf> { self.external.clone() }
    }

    // ── Helper: build a PathsConfig with explicit lists ───────────────────────

    fn cfg_from(
        standard: &[&Path],
        cloud: &[&Path],
        external: &[&Path],
    ) -> PathsConfig {
        use crate::search::paths_config::{ExternalMode, CURRENT_CONFIG_VERSION};
        PathsConfig {
            standard: standard.iter().map(|p| p.to_string_lossy().into_owned()).collect(),
            cloud:    cloud.iter().map(|p| p.to_string_lossy().into_owned()).collect(),
            external: ExternalMode::List(
                external.iter().map(|p| p.to_string_lossy().into_owned()).collect(),
            ),
            max_depth:  8,
            exclude:    vec![],
            result_cap: 1000,
            version:    CURRENT_CONFIG_VERSION,
        }
    }

    // Helper: empty provider (used when Fake is not needed for standard/cloud)
    fn empty_provider() -> Fake {
        Fake::new(vec![], vec![], vec![])
    }

    // ── Test 1: order_is_cwd_standard_cloud_external ──────────────────────────

    /// All four sources present, all disjoint → sources returned in exact
    /// priority order: Cwd, Standard, Cloud, External.
    #[test]
    fn order_is_cwd_standard_cloud_external() {
        let cwd_dir      = TempDir::new().unwrap();
        let std_dir      = TempDir::new().unwrap();
        let cloud_dir    = TempDir::new().unwrap();
        let external_dir = TempDir::new().unwrap();

        let cwd      = cwd_dir.path();
        let std_path = std_dir.path();
        let cloud    = cloud_dir.path();
        let external = external_dir.path();

        // All four are disjoint temp dirs → no nesting.
        let cfg = cfg_from(&[std_path], &[cloud], &[external]);
        let provider = empty_provider();

        let roots = resolve_roots(cwd, &provider, &cfg, false);

        assert_eq!(roots.len(), 4, "expected 4 roots, got {}: {roots:?}", roots.len());

        let sources: Vec<SearchSource> =
            roots.iter().map(|r| r.source.clone()).collect();
        assert_eq!(
            sources,
            vec![
                SearchSource::Cwd,
                SearchSource::Standard,
                SearchSource::Cloud,
                SearchSource::External,
            ],
            "sources out of order: {sources:?}"
        );

        // No nesting → all prune lists are empty.
        for r in &roots {
            assert!(
                r.prune.is_empty(),
                "expected empty prune for {:?}, got {:?}",
                r.source,
                r.prune
            );
        }
    }

    // ── Test 2: cwd_inside_cloud_keeps_both_and_prunes ────────────────────────

    /// cwd is a sub-directory of a cloud root.
    /// Both must be present; the cloud root's `prune` must contain the
    /// canonicalized cwd path; the cwd comes first in the list.
    #[test]
    fn cwd_inside_cloud_keeps_both_and_prunes() {
        let base = TempDir::new().unwrap();
        let cloud_path = base.path().join("cloud");
        let cwd_path   = cloud_path.join("proj");

        std::fs::create_dir_all(&cwd_path).unwrap();

        // Canonicalize to get the actual paths as resolve_roots will see them.
        let canon_cwd   = std::fs::canonicalize(&cwd_path).unwrap();
        let canon_cloud = std::fs::canonicalize(&cloud_path).unwrap();

        let cfg = cfg_from(&[], &[&cloud_path], &[]);
        let provider = empty_provider();

        let roots = resolve_roots(&cwd_path, &provider, &cfg, false);

        assert_eq!(roots.len(), 2, "expected 2 roots (cwd + cloud), got {}: {roots:?}", roots.len());

        // cwd comes first.
        assert_eq!(roots[0].source, SearchSource::Cwd);
        assert_eq!(roots[0].path, canon_cwd);
        assert!(roots[0].prune.is_empty(), "cwd should have empty prune: {:?}", roots[0].prune);

        // cloud comes second.
        assert_eq!(roots[1].source, SearchSource::Cloud);
        assert_eq!(roots[1].path, canon_cloud);
        // Cloud's prune must contain cwd (because cwd is inside cloud).
        assert!(
            roots[1].prune.contains(&canon_cwd),
            "cloud.prune should contain cwd ({canon_cwd:?}), got {:?}",
            roots[1].prune
        );
    }

    // ── Test 3: exact_duplicate_dropped ──────────────────────────────────────

    /// When cwd coincides exactly with a standard path, only one Root is produced
    /// (the first one: source = Cwd).
    #[test]
    fn exact_duplicate_dropped() {
        let dir = TempDir::new().unwrap();
        let path = dir.path();

        // cwd == the only standard path.
        let cfg = cfg_from(&[path], &[], &[]);
        let provider = empty_provider();

        let roots = resolve_roots(path, &provider, &cfg, false);

        assert_eq!(roots.len(), 1, "duplicate should be dropped; got {roots:?}");
        assert_eq!(roots[0].source, SearchSource::Cwd, "winner should be Cwd (first)");
    }

    // ── Test 4: nonexistent_path_skipped ─────────────────────────────────────

    /// A path listed in cfg.standard that does not exist on disk
    /// must not appear in the result.
    #[test]
    fn nonexistent_path_skipped() {
        let cwd_dir = TempDir::new().unwrap();
        let cwd = cwd_dir.path();

        let nonexistent = Path::new("C:\\this\\path\\does\\not\\exist\\at\\all\\42");

        let cfg = cfg_from(&[nonexistent], &[], &[]);
        let provider = empty_provider();

        let roots = resolve_roots(cwd, &provider, &cfg, false);

        // Only the cwd root should survive; the nonexistent standard path is dropped.
        assert_eq!(roots.len(), 1, "nonexistent path should be skipped; got {roots:?}");
        assert_eq!(roots[0].source, SearchSource::Cwd);
    }

    // ── Test 5: roots_carry_cfg_max_depth ────────────────────────────────────

    /// `resolve_roots` must populate `max_depth` on every returned root
    /// from `cfg.max_depth`. This is the RED test for Task 2.
    #[test]
    fn roots_carry_cfg_max_depth() {
        let cwd_dir = TempDir::new().unwrap();
        let cfg = cfg_from(&[], &[], &[]);
        let provider = empty_provider();
        let roots = resolve_roots(cwd_dir.path(), &provider, &cfg, false);
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].max_depth, cfg.max_depth);
    }

    // ── Helper: cfg_from with explicit exclude list ───────────────────────────

    fn cfg_from_exclude(
        standard: &[&Path],
        cloud: &[&Path],
        external: &[&Path],
        exclude: &[&str],
    ) -> PathsConfig {
        let mut c = cfg_from(standard, cloud, external);
        c.exclude = exclude.iter().map(|s| s.to_string()).collect();
        c
    }

    // ── Test 6: fanout_expands_one_level_with_decremented_depth ──────────────

    /// A single root dir with two subdirs A and B (and a file) must produce
    /// three roots: the original (max_depth), A (max_depth-1), B (max_depth-1).
    /// The parent's prune must list both children.
    #[test]
    fn fanout_expands_one_level_with_decremented_depth() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("A")).unwrap();
        std::fs::create_dir_all(root.join("B")).unwrap();
        std::fs::write(root.join("top.txt"), b"").unwrap();

        let cfg = cfg_from(&[], &[], &[]); // max_depth 8, exclude []
        let provider = empty_provider();
        let roots = resolve_roots(root, &provider, &cfg, false);

        // root + A + B
        assert_eq!(roots.len(), 3, "atteso root+A+B, ottenuto {roots:?}");
        let canon_root = std::fs::canonicalize(root).unwrap();
        let top = roots.iter().find(|r| r.path == canon_root).unwrap();
        assert_eq!(top.max_depth, cfg.max_depth);
        // i due figli sono nel prune del padre
        assert_eq!(top.prune.len(), 2, "prune padre = i due figli: {:?}", top.prune);
        // i figli hanno max_depth = padre - 1
        for r in roots.iter().filter(|r| r.path != canon_root) {
            assert_eq!(r.max_depth, cfg.max_depth - 1);
            assert_eq!(r.source, SearchSource::Cwd); // ereditano la sorgente del padre
        }
    }

    // ── Test 7: fanout_skips_excluded_children ────────────────────────────────

    /// A root with subdirs `node_modules` (excluded) and `src` must produce
    /// only two roots (root + src); `node_modules` must NOT become a sub-root.
    #[test]
    fn fanout_skips_excluded_children() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("node_modules")).unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();

        let cfg = cfg_from_exclude(&[], &[], &[], &["node_modules"]);
        let provider = empty_provider();
        let roots = resolve_roots(root, &provider, &cfg, false);

        // root + src soltanto (node_modules escluso, niente sub-root)
        assert_eq!(roots.len(), 2, "node_modules non deve diventare root: {roots:?}");
        assert!(roots.iter().all(|r| !r.path.ends_with("node_modules")));
    }

    // ── Test 8: fanout_does_not_duplicate_existing_root ──────────────────────

    /// When cwd (`cloud/proj`) is inside a cloud root (`cloud`), and `cloud`
    /// is fanned out, `proj` must appear only once (as Cwd) — not also as a
    /// fan-out child of cloud.
    #[test]
    fn fanout_does_not_duplicate_existing_root() {
        // cwd = cloud/proj ; cloud è un root ; proj NON deve essere rifanout-ato dal cloud.
        let base = TempDir::new().unwrap();
        let cloud = base.path().join("cloud");
        let proj = cloud.join("proj");
        std::fs::create_dir_all(&proj).unwrap();

        let cfg = cfg_from(&[], &[&cloud], &[]);
        let provider = empty_provider();
        let roots = resolve_roots(&proj, &provider, &cfg, false);

        let canon_proj = std::fs::canonicalize(&proj).unwrap();
        // proj compare UNA sola volta (come Cwd), non anche come figlio fanout del cloud
        assert_eq!(roots.iter().filter(|r| r.path == canon_proj).count(), 1, "proj duplicato: {roots:?}");
    }

    // ── Test: only_cwd salta Standard/Cloud/External ──────────────────────────

    /// `only_cwd: true` deve produrre SOLO un root Cwd, anche quando
    /// `cfg.standard`/`cloud`/`external` puntano a directory reali ed
    /// esistenti che altrimenti sopravvivrebbero tutte. RED per
    /// `/find folder:from-here`.
    #[test]
    fn only_cwd_true_skips_standard_cloud_external() {
        let cwd_dir      = TempDir::new().unwrap();
        let std_dir      = TempDir::new().unwrap();
        let cloud_dir    = TempDir::new().unwrap();
        let external_dir = TempDir::new().unwrap();

        let cfg = cfg_from(&[std_dir.path()], &[cloud_dir.path()], &[external_dir.path()]);
        let provider = empty_provider();

        let roots = resolve_roots(cwd_dir.path(), &provider, &cfg, true);

        assert_eq!(roots.len(), 1, "solo il root Cwd atteso, got {roots:?}");
        assert_eq!(roots[0].source, SearchSource::Cwd);
    }

    /// `only_cwd: false` deve comportarsi ESATTAMENTE come prima
    /// dell'introduzione del parametro — nessuna regressione sul path
    /// esistente.
    #[test]
    fn only_cwd_false_behaves_exactly_like_before() {
        let cwd_dir = TempDir::new().unwrap();
        let std_dir = TempDir::new().unwrap();

        let cfg = cfg_from(&[std_dir.path()], &[], &[]);
        let provider = empty_provider();

        let roots = resolve_roots(cwd_dir.path(), &provider, &cfg, false);

        assert_eq!(roots.len(), 2, "atteso Cwd + Standard, got {roots:?}");
    }
}
