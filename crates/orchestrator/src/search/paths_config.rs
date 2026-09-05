//! # search::paths_config
//!
//! Config JSON for file search paths (`search-paths.json`) — auto-generated on first run,
//! user-editable. Encapsulates OS-specific path knowledge behind the `PathProvider` trait.
//!
//! ## Key types
//! - `PathProvider` — trait for OS-specific directory detection (standard, cloud, external drives).
//! - `OsPathProvider` — real implementation using `dirs` crate + per-OS best-effort detection.
//! - `PathsConfig` — serialized/deserialized config struct; `load_or_generate` handles missing
//!   or corrupt files gracefully (always returns a usable config).
//! - `ExternalMode` — either `Auto` (delegate to `PathProvider::external_drives`) or an explicit
//!   list of paths, with custom serde: `"auto"` ↔ `Auto`, JSON array ↔ `List(Vec<String>)`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// PathProvider trait
// ─────────────────────────────────────────────────────────────────────────────

/// Encapsulates OS-specific knowledge for file search root detection.
///
/// Implementations must never panic — missing directories, unreadable environment
/// variables, or absent cloud config files should result in an empty `Vec`, not a crash.
pub trait PathProvider: Send + Sync {
    /// Returns standard user directories (Documents, Downloads, Desktop, …).
    fn standard_dirs(&self) -> Vec<PathBuf>;

    /// Returns detected cloud sync folders (Dropbox path from info.json, OneDrive, …).
    fn detect_cloud(&self) -> Vec<PathBuf>;

    /// Returns external/removable drives, excluding the primary OS drive.
    fn external_drives(&self) -> Vec<PathBuf>;

    /// Default OS-specific directory NAMES to exclude (system/heavy folders).
    /// Returned as bare name components, matched by the walker against each entry's name.
    /// Default empty so non-OS providers (and test fakes) need not override it.
    fn system_excludes(&self) -> Vec<String> {
        Vec::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ExternalMode — custom serde: "auto" ↔ Auto, array ↔ List
// ─────────────────────────────────────────────────────────────────────────────

/// How external drives are discovered.
///
/// Serialized as:
/// - `"auto"` for `Auto` (delegate to `PathProvider::external_drives`).
/// - `["C:\\ext1", "D:\\"]` for `List(...)` (explicit path list).
#[derive(Debug, Clone, PartialEq, Default)]
pub enum ExternalMode {
    /// Discover drives automatically via `PathProvider::external_drives`.
    #[default]
    Auto,
    /// Use this explicit list of paths instead of auto-detection.
    List(Vec<String>),
}

impl Serialize for ExternalMode {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            ExternalMode::Auto => s.serialize_str("auto"),
            ExternalMode::List(v) => v.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for ExternalMode {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // Deserialize into an intermediate that can be either a string or a vec.
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Helper {
            Str(String),
            List(Vec<String>),
        }

        match Helper::deserialize(d)? {
            Helper::Str(s) if s == "auto" => Ok(ExternalMode::Auto),
            Helper::Str(s) => Err(serde::de::Error::custom(format!(
                "unknown ExternalMode string: expected \"auto\", got {s:?}"
            ))),
            Helper::List(v) => Ok(ExternalMode::List(v)),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Default-value functions (used by both #[serde(default = ...)] and construction)
// ─────────────────────────────────────────────────────────────────────────────

fn default_max_depth() -> usize {
    8
}

fn default_result_cap() -> usize {
    2000
}

/// Previous default result cap (config schema v1). The migration bumps configs
/// still at this old default up to the current default, leaving user-customized
/// values untouched.
const PREVIOUS_DEFAULT_RESULT_CAP: usize = 1000;

fn default_exclude() -> Vec<String> {
    vec!["node_modules".to_string(), ".git".to_string()]
}

/// Bump when the default config policy changes so existing configs get migrated.
/// v1: system_excludes merged into `exclude`. v2: result_cap default 1000 → 2000.
pub const CURRENT_CONFIG_VERSION: u32 = 2;

// ─────────────────────────────────────────────────────────────────────────────
// PathsConfig
// ─────────────────────────────────────────────────────────────────────────────

/// Persistent file-search configuration stored in `search-paths.json`.
///
/// JSON format (partial example — unknown fields are ignored):
/// ```json
/// {
///   "standard": ["C:\\Users\\u\\Documents"],
///   "cloud":    ["C:\\Users\\u\\Dropbox"],
///   "external": "auto",
///   "max_depth": 8,
///   "exclude": ["node_modules", ".git"],
///   "result_cap": 1000
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathsConfig {
    /// Paths in the "standard" category (Documents, Downloads, Desktop, …).
    #[serde(default)]
    pub standard: Vec<String>,

    /// Paths in the "cloud" category (Dropbox, OneDrive, …).
    #[serde(default)]
    pub cloud: Vec<String>,

    /// How external drives are handled.
    #[serde(default)]
    pub external: ExternalMode,

    /// Maximum directory recursion depth during walk.
    #[serde(default = "default_max_depth")]
    pub max_depth: usize,

    /// Directory names / path segments to exclude from the walk.
    #[serde(default = "default_exclude")]
    pub exclude: Vec<String>,

    /// Maximum number of results before the search is truncated.
    #[serde(default = "default_result_cap")]
    pub result_cap: usize,

    /// Schema/policy version of this config; see CURRENT_CONFIG_VERSION.
    #[serde(default)]
    pub version: u32,
}

/// Removes exact-duplicate strings, preserving first-seen order.
fn dedup_preserving_order(items: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    items.into_iter().filter(|s| seen.insert(s.clone())).collect()
}

impl PathsConfig {
    /// Loads from `path` if it exists and is valid JSON.
    ///
    /// If the file is absent, unreadable, or contains invalid JSON, a fresh config
    /// is generated from `provider`, saved to `path`, and returned. The caller always
    /// gets a usable `PathsConfig` regardless of disk state.
    ///
    /// If the loaded config has an older `version`, a one-time migration runs:
    /// dead (path-like) exclude entries are removed and the provider's
    /// `system_excludes` are merged in; then the file is rewritten.
    /// Configs already at `CURRENT_CONFIG_VERSION` are never re-migrated,
    /// so user removals of system entries are respected.
    pub fn load_or_generate(path: &Path, provider: &dyn PathProvider) -> Self {
        if let Ok(content) = std::fs::read_to_string(path) {
            if let Ok(mut cfg) = serde_json::from_str::<PathsConfig>(&content) {
                if cfg.version < CURRENT_CONFIG_VERSION {
                    cfg.migrate(provider);
                    let _ = cfg.save(path);
                }
                return cfg;
            }
            // File present but JSON invalid → regenerate (overwrites corrupt file).
        }

        let standard: Vec<String> = provider
            .standard_dirs()
            .into_iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        let cloud: Vec<String> = provider
            .detect_cloud()
            .into_iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();

        let mut exclude = default_exclude();
        exclude.extend(provider.system_excludes());
        let exclude = dedup_preserving_order(exclude);

        let cfg = PathsConfig {
            standard,
            cloud,
            external: ExternalMode::Auto,
            max_depth: default_max_depth(),
            exclude,
            result_cap: default_result_cap(),
            version: CURRENT_CONFIG_VERSION,
        };

        let _ = cfg.save(path);
        cfg
    }

    /// One-time upgrade of an older config:
    /// - drop dead (path-like) exclude entries and merge the provider's system
    ///   excludes (excludes match by NAME component, so entries with a separator
    ///   are dead);
    /// - bump a still-default `result_cap` (the old default) to the new default,
    ///   leaving user-customized values untouched;
    /// - set the version.
    fn migrate(&mut self, provider: &dyn PathProvider) {
        self.exclude.retain(|e| !e.contains('/') && !e.contains('\\'));
        let mut merged = std::mem::take(&mut self.exclude);
        merged.extend(provider.system_excludes());
        self.exclude = dedup_preserving_order(merged);

        if self.result_cap == PREVIOUS_DEFAULT_RESULT_CAP {
            self.result_cap = default_result_cap();
        }

        self.version = CURRENT_CONFIG_VERSION;
    }

    /// Saves the config to `path`, creating parent directories as needed.
    fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("search-paths: cannot create dir: {e}"))?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("search-paths: serialization error: {e}"))?;
        std::fs::write(path, json)
            .map_err(|e| format!("search-paths: write error: {e}"))
    }

    /// Returns the `standard` paths with `~` and env-var placeholders expanded.
    pub fn expanded_standard(&self) -> Vec<PathBuf> {
        self.standard.iter().map(|s| expand_path(s)).collect()
    }

    /// Returns the `cloud` paths with `~` and env-var placeholders expanded.
    pub fn expanded_cloud(&self) -> Vec<PathBuf> {
        self.cloud.iter().map(|s| expand_path(s)).collect()
    }

    /// Returns the external paths, either from `provider.external_drives()` (Auto)
    /// or by expanding the explicit list (List).
    pub fn expanded_external(&self, provider: &dyn PathProvider) -> Vec<PathBuf> {
        match &self.external {
            ExternalMode::Auto => provider.external_drives(),
            ExternalMode::List(paths) => paths.iter().map(|s| expand_path(s)).collect(),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Path expansion helpers (tilde + env vars)
// ─────────────────────────────────────────────────────────────────────────────

/// Expands leading `~` to `dirs::home_dir()` and `$VAR` / `%VAR%` placeholders.
///
/// Best-effort: unresolvable env vars are left as-is. Never panics.
fn expand_path(s: &str) -> PathBuf {
    let s = expand_tilde(s);
    let s = expand_env_vars(&s);
    PathBuf::from(s)
}

/// Replaces a leading `~` with the user's home directory.
///
/// - `~/foo` → `<home>/foo`
/// - `~` alone → `<home>`
/// - No leading `~` → unchanged
/// - Home unavailable → unchanged (best-effort)
fn expand_tilde(s: &str) -> String {
    if s == "~" {
        return dirs::home_dir()
            .map(|h| h.to_string_lossy().into_owned())
            .unwrap_or_else(|| s.to_string());
    }
    if let Some(rest) = s.strip_prefix("~/").or_else(|| s.strip_prefix("~\\")) {
        if let Some(home) = dirs::home_dir() {
            return format!("{}/{rest}", home.to_string_lossy());
        }
    }
    s.to_string()
}

/// Expands `%VAR%` (Windows style) and `$VAR` (Unix style) environment variables.
///
/// Unresolvable variables are left in-place. Never panics.
fn expand_env_vars(s: &str) -> String {
    let mut result = expand_windows_vars(s);
    result = expand_unix_vars(&result);
    result
}

/// Expands `%VAR%` style Windows env vars.
fn expand_windows_vars(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            // Collect up to the next '%'.
            let var_name: String = chars.by_ref().take_while(|&x| x != '%').collect();
            if var_name.is_empty() {
                // `%%` — literal percent sign
                result.push('%');
            } else if let Ok(val) = std::env::var(&var_name) {
                result.push_str(&val);
            } else {
                // Unresolved: put it back verbatim
                result.push('%');
                result.push_str(&var_name);
                result.push('%');
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Expands `$VAR` style Unix env vars (stops at `/`, `\`, end of string, or non-identifier char).
fn expand_unix_vars(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '$' {
            // Collect the variable name: alphanumeric + '_'.
            let var_name: String = chars
                .by_ref()
                .take_while(|x| x.is_alphanumeric() || *x == '_')
                .collect();
            if var_name.is_empty() {
                result.push('$');
            } else if let Ok(val) = std::env::var(&var_name) {
                result.push_str(&val);
            } else {
                // Unresolved: put it back verbatim
                result.push('$');
                result.push_str(&var_name);
            }
        } else {
            result.push(c);
        }
    }
    result
}

// ─────────────────────────────────────────────────────────────────────────────
// OsPathProvider — real implementation
// ─────────────────────────────────────────────────────────────────────────────

/// Real `PathProvider` implementation using `dirs` crate + per-OS best-effort detection.
///
/// - `standard_dirs`: `dirs::document_dir()`, `dirs::download_dir()`, `dirs::desktop_dir()`.
/// - `detect_cloud`: Windows only — Dropbox via `%APPDATA%\Dropbox\info.json`, OneDrive via `%OneDrive%`.
/// - `external_drives`: Windows only — enumerates volumes that are not `C:`.
pub struct OsPathProvider;

impl PathProvider for OsPathProvider {
    fn standard_dirs(&self) -> Vec<PathBuf> {
        [
            dirs::document_dir(),
            dirs::download_dir(),
            dirs::desktop_dir(),
        ]
        .into_iter()
        .flatten() // discard None entries
        .collect()
    }

    fn detect_cloud(&self) -> Vec<PathBuf> {
        detect_cloud_impl()
    }

    fn external_drives(&self) -> Vec<PathBuf> {
        external_drives_impl()
    }

    fn system_excludes(&self) -> Vec<String> {
        system_excludes_impl()
    }
}

// Windows-specific cloud detection.
#[cfg(windows)]
fn detect_cloud_impl() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // Dropbox: read path from %APPDATA%\Dropbox\info.json, field "path"
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let info_path = PathBuf::from(appdata).join("Dropbox").join("info.json");
        if let Ok(content) = std::fs::read_to_string(&info_path) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                // info.json format: { "personal": { "path": "C:\\Users\\u\\Dropbox" }, ... }
                for section in ["personal", "business"] {
                    if let Some(path_str) = val
                        .get(section)
                        .and_then(|s| s.get("path"))
                        .and_then(|p| p.as_str())
                    {
                        let p = PathBuf::from(path_str);
                        if p.exists() {
                            paths.push(p);
                        }
                    }
                }
            }
        }
    }

    // OneDrive: %OneDrive% env var (set by Windows when OneDrive is installed).
    if let Ok(onedrive) = std::env::var("OneDrive") {
        let p = PathBuf::from(onedrive);
        if p.exists() && !paths.contains(&p) {
            paths.push(p);
        }
    }

    paths
}

#[cfg(not(windows))]
fn detect_cloud_impl() -> Vec<PathBuf> {
    // Non-Windows: no detection for now (added by whoever ports to Linux/macOS).
    vec![]
}

// Windows-specific external drive enumeration.
#[cfg(windows)]
fn external_drives_impl() -> Vec<PathBuf> {
    // Enumerate drive letters A:-Z: and include those that exist and are not C:.
    let mut drives = Vec::new();
    for letter in b'A'..=b'Z' {
        let drive = format!("{}:\\", letter as char);
        let path = PathBuf::from(&drive);
        // Skip the primary OS drive (C:) and non-existent drives.
        if letter != b'C' && path.exists() {
            drives.push(path);
        }
    }
    drives
}

#[cfg(not(windows))]
fn external_drives_impl() -> Vec<PathBuf> {
    // Non-Windows: no detection for now.
    vec![]
}

// Windows-specific system directory exclusions.
#[cfg(windows)]
fn system_excludes_impl() -> Vec<String> {
    [
        "Windows",
        "Program Files",
        "Program Files (x86)",
        "ProgramData",
        "$Recycle.Bin",
        "System Volume Information",
        "$WinREAgent",
        "Recovery",
        "PerfLogs",
        "AppData",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

#[cfg(not(windows))]
fn system_excludes_impl() -> Vec<String> {
    // Filled in when ported to Linux/macOS.
    Vec::new()
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    struct Fake;
    impl PathProvider for Fake {
        fn standard_dirs(&self) -> Vec<std::path::PathBuf> {
            vec!["/home/u/Documents".into()]
        }
        fn detect_cloud(&self) -> Vec<std::path::PathBuf> {
            vec!["/home/u/Dropbox".into()]
        }
        fn external_drives(&self) -> Vec<std::path::PathBuf> {
            vec![]
        }
        fn system_excludes(&self) -> Vec<String> {
            vec!["SysFake".to_string()]
        }
    }

    #[test]
    fn generate_on_first_run_then_reload() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("search-paths.json");
        let cfg = PathsConfig::load_or_generate(&p, &Fake);
        assert!(p.exists(), "il file deve essere generato al 1° avvio");
        assert!(cfg.standard.iter().any(|s| s.contains("Documents")));
        assert_eq!(cfg.max_depth, 8);
        assert_eq!(cfg.result_cap, 2000);
        // ricarica: stesso contenuto, niente rigenerazione distruttiva
        let cfg2 = PathsConfig::load_or_generate(&p, &Fake);
        assert_eq!(cfg.standard, cfg2.standard);
    }

    #[test]
    fn corrupted_file_regenerates_default() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("search-paths.json");
        std::fs::write(&p, "{not json").unwrap();
        let cfg = PathsConfig::load_or_generate(&p, &Fake);
        assert_eq!(cfg.max_depth, 8); // tornato ai default
    }

    #[test]
    fn expands_tilde() {
        // expanded_standard espande "~" verso la home; verifica che non resti il "~".
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("search-paths.json");
        let mut cfg = PathsConfig::load_or_generate(&p, &Fake);
        cfg.standard = vec!["~/Documents".into()];
        for path in cfg.expanded_standard() {
            assert!(
                !path.to_string_lossy().starts_with('~'),
                "tilde non espanso: {path:?}"
            );
        }
    }

    // ── ExternalMode serde round-trip ─────────────────────────────────────────

    #[test]
    fn external_mode_auto_serializes_to_string() {
        let json = serde_json::to_string(&ExternalMode::Auto).unwrap();
        assert_eq!(json, r#""auto""#);
    }

    #[test]
    fn external_mode_list_serializes_to_array() {
        let mode = ExternalMode::List(vec!["D:\\".to_string(), "E:\\".to_string()]);
        let json = serde_json::to_string(&mode).unwrap();
        assert!(json.starts_with('['), "expected JSON array, got: {json}");
        assert_eq!(
            serde_json::from_str::<ExternalMode>(&json).unwrap(),
            ExternalMode::List(vec!["D:\\".to_string(), "E:\\".to_string()])
        );
    }

    #[test]
    fn external_mode_auto_deserializes_from_string() {
        let mode: ExternalMode = serde_json::from_str(r#""auto""#).unwrap();
        assert_eq!(mode, ExternalMode::Auto);
    }

    #[test]
    fn external_mode_unknown_string_is_error() {
        let result = serde_json::from_str::<ExternalMode>(r#""manual""#);
        assert!(result.is_err(), "unknown string should be an error");
    }

    #[test]
    fn external_mode_round_trip_in_config() {
        // Verify ExternalMode survives a full PathsConfig round-trip through JSON.
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("search-paths.json");
        let cfg = PathsConfig::load_or_generate(&p, &Fake);
        assert_eq!(cfg.external, ExternalMode::Auto);
        let json = serde_json::to_string(&cfg).unwrap();
        let cfg2: PathsConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg2.external, ExternalMode::Auto);
    }

    // ── expanded_cloud / expanded_external ────────────────────────────────────

    #[test]
    fn expanded_cloud_returns_paths_from_config() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("search-paths.json");
        let cfg = PathsConfig::load_or_generate(&p, &Fake);
        let cloud = cfg.expanded_cloud();
        // Fake provides "/home/u/Dropbox"
        assert!(
            cloud.iter().any(|pb| pb.to_string_lossy().contains("Dropbox")),
            "expected Dropbox in cloud: {cloud:?}"
        );
    }

    #[test]
    fn expanded_external_auto_delegates_to_provider() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("search-paths.json");
        let cfg = PathsConfig::load_or_generate(&p, &Fake);
        // Fake::external_drives returns empty
        assert_eq!(cfg.external, ExternalMode::Auto);
        let ext = cfg.expanded_external(&Fake);
        assert!(ext.is_empty(), "Fake returns no drives; got: {ext:?}");
    }

    #[test]
    fn expanded_external_list_expands_explicit_paths() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("search-paths.json");
        let mut cfg = PathsConfig::load_or_generate(&p, &Fake);
        cfg.external = ExternalMode::List(vec!["D:\\Data".to_string()]);
        let ext = cfg.expanded_external(&Fake);
        assert_eq!(ext.len(), 1);
        assert_eq!(ext[0], PathBuf::from("D:\\Data"));
    }

    #[test]
    fn fresh_config_merges_system_excludes_and_sets_version() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("search-paths.json");
        let cfg = PathsConfig::load_or_generate(&p, &Fake);
        assert!(cfg.exclude.contains(&"node_modules".to_string()), "base mancante: {:?}", cfg.exclude);
        assert!(cfg.exclude.contains(&".git".to_string()), "base mancante: {:?}", cfg.exclude);
        assert!(cfg.exclude.contains(&"SysFake".to_string()), "system_excludes non uniti: {:?}", cfg.exclude);
        assert_eq!(cfg.version, CURRENT_CONFIG_VERSION);
    }

    #[test]
    fn migrates_old_config_dropping_dead_entries_and_merging_system() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("search-paths.json");
        // Config "vecchia": niente campo version (→ 0), con una voce morta path-like.
        let old = r#"{
            "standard": [], "cloud": [], "external": "auto",
            "max_depth": 8, "result_cap": 1000,
            "exclude": ["node_modules", "AppData/Local/Temp"]
        }"#;
        std::fs::write(&p, old).unwrap();

        let cfg = PathsConfig::load_or_generate(&p, &Fake);
        assert_eq!(cfg.version, CURRENT_CONFIG_VERSION, "version non aggiornata");
        assert!(!cfg.exclude.iter().any(|e| e.contains('/')), "voce morta non rimossa: {:?}", cfg.exclude);
        assert!(cfg.exclude.contains(&"SysFake".to_string()), "system_excludes non uniti: {:?}", cfg.exclude);
        assert!(cfg.exclude.contains(&"node_modules".to_string()), "exclude utente persa: {:?}", cfg.exclude);

        // File riscritto e migrazione persistita: una seconda load non cambia nulla.
        let again = PathsConfig::load_or_generate(&p, &Fake);
        assert_eq!(again.exclude, cfg.exclude);
        assert_eq!(again.version, CURRENT_CONFIG_VERSION);
    }

    #[test]
    fn migration_is_idempotent_and_respects_user_removals() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("search-paths.json");
        // Config GIA' a version corrente, con un system-exclude rimosso dall'utente.
        let current = format!(r#"{{
            "standard": [], "cloud": [], "external": "auto",
            "max_depth": 8, "result_cap": 1000,
            "exclude": ["node_modules"], "version": {CURRENT_CONFIG_VERSION}
        }}"#);
        std::fs::write(&p, current).unwrap();

        let cfg = PathsConfig::load_or_generate(&p, &Fake);
        // Niente re-merge: "SysFake" NON deve ricomparire.
        assert!(!cfg.exclude.contains(&"SysFake".to_string()), "re-merge indebito: {:?}", cfg.exclude);
        assert_eq!(cfg.exclude, vec!["node_modules".to_string()]);
    }

    #[test]
    fn migration_bumps_old_default_result_cap_to_new_default() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("search-paths.json");
        // Config v1 col vecchio default result_cap (1000).
        let old = r#"{
            "standard": [], "cloud": [], "external": "auto",
            "max_depth": 8, "result_cap": 1000, "exclude": [], "version": 1
        }"#;
        std::fs::write(&p, old).unwrap();

        let cfg = PathsConfig::load_or_generate(&p, &Fake);
        assert_eq!(cfg.result_cap, 2000, "il vecchio default deve salire al nuovo default");
        assert_eq!(cfg.version, CURRENT_CONFIG_VERSION);
    }

    #[test]
    fn migration_keeps_custom_result_cap() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("search-paths.json");
        // Config v1 con result_cap personalizzato (diverso dal vecchio default).
        let custom = r#"{
            "standard": [], "cloud": [], "external": "auto",
            "max_depth": 8, "result_cap": 5000, "exclude": [], "version": 1
        }"#;
        std::fs::write(&p, custom).unwrap();

        let cfg = PathsConfig::load_or_generate(&p, &Fake);
        assert_eq!(cfg.result_cap, 5000, "il valore personalizzato non va toccato");
        assert_eq!(cfg.version, CURRENT_CONFIG_VERSION);
    }
}
