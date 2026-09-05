//! # search
//!
//! File-search orchestration for Lare Terminal.
//!
//! Sub-modules:
//! - `paths_config` — persistent config (search-paths.json), path expansion, PathProvider trait.
//! - `query`        — query parsing → Matcher (glob or token-AND).
//! - `roots`        — resolve ordered search roots (Cwd → Standard → Cloud → External).
//! - `walk`         — async directory walker for one root, emits `Hit` values.
//! - `pause`        — PauseGate: cooperative pause/resume for blocking walkers.
//!
//! This module (`search/mod.rs`) owns `SearchEngine`, which orchestrates the concurrent
//! walk across all roots and streams `ServerMsg` protocol messages to the caller.

pub mod content;
pub mod paths_config;
pub mod pause;
pub mod query;
pub mod roots;
pub mod walk;

pub use pause::PauseGate;

use std::{path::Path, sync::Arc};

use tokio::sync::Semaphore;
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

use protocol::ServerMsg;

use crate::search::content::{ContentConfig, ContentMatcher};
use crate::search::paths_config::{PathProvider, PathsConfig};
use crate::search::query::parse_query;
use crate::search::roots::{resolve_roots, Root};
use crate::search::walk::{walk_root, ContentSearch, Hit};

// ─────────────────────────────────────────────────────────────────────────────
// SearchContext + launch — entry point usato dal transport (ws)
// ─────────────────────────────────────────────────────────────────────────────

/// Dipendenze condivise per lanciare una ricerca, costruite una volta all'avvio
/// (`main.rs`) e clonate per ogni ricerca (campi `Arc`, clone economico).
#[derive(Clone)]
pub struct SearchContext {
    pub engine: Arc<SearchEngine>,
    /// Percorso di search-paths.json; `launch` rilegge la config da qui a ogni
    /// ricerca, così le modifiche (es. result_cap via /config) hanno effetto
    /// subito senza riavviare l'orchestrator.
    pub cfg_path: Arc<std::path::PathBuf>,
    /// Percorso di search-content.json (estensioni testuali/binarie note, cap
    /// dimensione file, cap Fase B) — rilettura per-ricerca, stesso motivo.
    pub content_cfg_path: Arc<std::path::PathBuf>,
    pub provider: Arc<dyn PathProvider>,
}

/// Helper sottile: risolve i root da `cwd` e lancia `SearchEngine::run`.
/// Tiene il transport (`ws`) privo di logica di ricerca (SRP).
///
/// `gate` è il `PauseGate` creato da `ws` per questa ricerca; viene passato a
/// ogni walker in modo che `ws` possa sospenderli con `gate.pause()`.
pub async fn launch(
    ctx: &SearchContext,
    id: &str,
    query: &str,
    cwd: &Path,
    out_tx: &UnboundedSender<ServerMsg>,
    cancel: CancellationToken,
    gate: Arc<PauseGate>,
) {
    // Parsing PRIMA di resolve_roots ora: `only_cwd` (da `folder:from-here`)
    // serve a `resolve_roots`, quindi deve esistere già a questo punto —
    // ordine invertito rispetto a prima dell'introduzione di `folder:`.
    //
    // `ws.rs` ha già validato che il parsing non fallisce (stessa funzione
    // pura, stesso input) PRIMA di spawnare questo task (spec §4) — se
    // fallisse comunque qui (non dovrebbe mai accadere), usciamo
    // silenziosamente senza emettere nulla: `ws.rs` resta l'UNICO punto che
    // segnala l'errore all'utente, `launch` è difensivo ma non duplica la
    // segnalazione.
    let parsed = match crate::search::query::parse_find_input(query) {
        Ok(p) => p,
        Err(_) => return,
    };

    let cfg = PathsConfig::load_or_generate(&ctx.cfg_path, ctx.provider.as_ref());
    let roots = resolve_roots(cwd, ctx.provider.as_ref(), &cfg, parsed.from_here);
    let title = format!("/find {query}");

    let content = parsed.content.map(|phrase| {
        let content_cfg = crate::search::content::ContentConfig::load_or_generate(&ctx.content_cfg_path);
        (crate::search::content::ContentMatcher::new(&phrase), content_cfg)
    });

    ctx.engine
        .run(id, &title, &parsed.name_query, roots, &cfg, out_tx, cancel, gate, content)
        .await;
}

// ─────────────────────────────────────────────────────────────────────────────
// SearchEngine
// ─────────────────────────────────────────────────────────────────────────────

/// Orchestrates a file-search operation across multiple roots.
///
/// `SearchEngine` owns only configuration knobs (no state across calls), making
/// it cheap to share and reuse. Each `run` call is fully independent.
pub struct SearchEngine {
    /// Maximum number of concurrent directory-walk tasks. Limits memory usage
    /// and kernel thread pressure when many roots are configured.
    pub max_concurrency: usize,
}

impl Default for SearchEngine {
    fn default() -> Self {
        Self { max_concurrency: 8 }
    }
}

impl SearchEngine {
    /// Runs a file search and streams protocol messages to `tx`.
    ///
    /// ## Message sequence
    /// 1. `SearchOpen { id, title }` — always emitted first.
    /// 2. Zero or more `SearchHit { id, path, source }` — one per matching file.
    /// 3. `SearchDone { id, count, truncated }` — always emitted last.
    ///
    /// ## Cancellation
    /// The caller may cancel via `cancel`. The engine also calls `cancel.cancel()`
    /// internally when `cfg.result_cap` is reached (truncation).
    ///
    /// ## Pause/Resume
    /// `gate` controls whether the search is paused — reused unchanged across
    /// BOTH phases (spec §5: "stesso PauseGate riusato", no separate mechanism
    /// for Fase B). Fase A's walkers check it per directory entry (`walk_root`,
    /// `walk.rs`); Fase B checks it per queued `Unknown` file, inside the same
    /// `spawn_blocking` closure that does the sniff+search work — same pattern,
    /// pause-check before the cancellation check (a cancel arriving mid-pause
    /// must still unblock promptly). While paused, no `SearchDone` is emitted —
    /// the search is simply suspended until `gate.resume()` is called.
    ///
    /// ## Errors
    /// If `tx` is closed before the first send, `run` returns immediately without
    /// emitting `SearchDone` (receiver is gone — no point in continuing).
    #[allow(clippy::too_many_arguments)]
    pub async fn run(
        &self,
        id: &str,
        title: &str,
        query: &str,
        roots: Vec<Root>,
        cfg: &PathsConfig,
        tx: &UnboundedSender<ServerMsg>,
        cancel: CancellationToken,
        gate: Arc<PauseGate>,
        content: Option<(ContentMatcher, ContentConfig)>,
    ) {
        // Step 1: emit SearchOpen. If receiver is gone, abort immediately.
        if tx.send(ServerMsg::SearchOpen { id: id.to_string(), title: title.to_string() }).is_err() {
            return;
        }

        // Step 2: compile the query matcher and clone the exclude list into Arc
        // so they can be shared cheaply across all concurrent walker tasks.
        let matcher = Arc::new(parse_query(query));
        let exclude = Arc::new(cfg.exclude.clone());

        // Step 3: bounded channel for backpressure (walkers block when drainer is slow);
        // semaphore to cap concurrent walk tasks at max_concurrency.
        let (hit_tx, mut hit_rx) = tokio::sync::mpsc::channel::<Hit>(256);
        let sem = Arc::new(Semaphore::new(self.max_concurrency));

        // Extract scalars from cfg before the spawn loop — cfg is borrowed (&PathsConfig)
        // and cannot be moved or captured across await points in spawned tasks.
        let result_cap = cfg.result_cap;

        // Fase B: canale per i file "Unknown" (né Text né Binary) accodati durante
        // la Fase A. Creato SEMPRE, anche senza content-search: se `content` è
        // `None`, nessun walker riceve mai un `ContentSearch` e quindi nessuno
        // scrive mai qui — il canale resta inutilizzato, costo trascurabile.
        let (unknown_tx, mut unknown_rx) =
            tokio::sync::mpsc::channel::<(std::path::PathBuf, protocol::SearchSource)>(256);

        // Un solo `Arc<ContentSearch>` condiviso (via `.clone()`) fra tutti i
        // walker — stesso `unknown_tx` sottostante per ognuno.
        let content_search: Option<Arc<ContentSearch>> = content.as_ref().map(|(m, c)| {
            Arc::new(ContentSearch {
                matcher: m.clone(),
                cfg: c.clone(),
                unknown_tx: unknown_tx.clone(),
            })
        });

        // Step 4: spawn one task per root. Each task acquires one semaphore permit,
        // performs the walk, then drops the permit (releasing capacity for the next root).
        for root in roots {
            let matcher  = Arc::clone(&matcher);
            let exclude  = Arc::clone(&exclude);
            let hit_tx   = hit_tx.clone();
            let sem      = Arc::clone(&sem);
            let cancel   = cancel.clone();
            let gate     = Arc::clone(&gate);
            let content_search = content_search.clone();

            tokio::spawn(async move {
                // `acquire_owned()` returns a permit bound to this task's lifetime.
                // Using `_permit` (not `_`) ensures the permit is dropped only when
                // this async block exits — properly gating concurrency.
                let _permit = sem.acquire_owned().await.expect("semaphore closed unexpectedly");
                walk_root(root, matcher, exclude, hit_tx, cancel, gate, content_search).await;
                // _permit dropped here → semaphore slot freed
            });
        }

        // Drop the original senders so that once all spawned tasks finish and
        // their clones are dropped, the corresponding receivers close.
        drop(hit_tx);
        drop(unknown_tx);
        drop(content_search);

        // Fase A — drena hit_rx E unknown_rx CONCORRENTEMENTE (tokio::select!),
        // MAI in sequenza: se drenassimo solo hit_rx prima, un walker bloccato su
        // unknown_tx.blocking_send (canale pieno, capacità 256, nessuno lo
        // svuota ancora) non ritornerebbe MAI da walk_root — quindi non
        // droppa mai il proprio hit_tx, quindi hit_rx.recv() non vede mai
        // "tutti i sender spariti", quindi la Fase A non finisce mai, quindi
        // la Fase B non parte mai, quindi unknown_rx non si svuota mai:
        // deadlock permanente, raggiungibile con un albero che ha più di 256
        // file Unknown (scenario realistico: max_unknown_scan di default è
        // 5000). Qui accumuliamo solo i CANDIDATI Unknown in un Vec — zero
        // I/O in questo loop, serve solo a svuotare il canale così nessun
        // walker resta mai bloccato. Il lavoro vero (sniff+cerca) resta
        // dopo, in Fase B, una volta che ENTRAMBI i canali sono chiusi (cioè
        // ogni walker è genuinamente finito).
        let mut count: usize = 0;
        let mut truncated = false;
        let mut unknown_queue: Vec<(std::path::PathBuf, protocol::SearchSource)> = Vec::new();
        let mut hit_rx_open = true;
        let mut unknown_rx_open = true;

        while hit_rx_open || unknown_rx_open {
            tokio::select! {
                hit = hit_rx.recv(), if hit_rx_open => {
                    match hit {
                        Some(hit) => {
                            if tx.send(ServerMsg::SearchHit {
                                id:      id.to_string(),
                                path:    hit.path,
                                source:  hit.source,
                                line:    hit.line,
                                snippet: hit.snippet,
                            }).is_err() {
                                // Receiver gone: smetti di drenare, non c'è più nessuno a cui riportare.
                                hit_rx_open = false;
                                unknown_rx_open = false;
                            } else {
                                count += 1;
                                if count >= result_cap {
                                    truncated = true;
                                    cancel.cancel();
                                    // Cap raggiunto: smetti di drenare ENTRAMBI i canali, non solo
                                    // hit_rx. `hit_tx` è anch'esso bounded (capacità 256): se
                                    // continuassimo a drenare solo unknown_rx e smettessimo di
                                    // svuotare hit_rx, un walker ancora in corsa che trova altri
                                    // match-nome bloccherebbe per sempre su
                                    // `hit_tx.blocking_send` (canale pieno, nessuno lo svuota più)
                                    // — la stessa classe di deadlock che questa Fase A concorrente
                                    // doveva eliminare, solo sull'altro canale. Uscire da ENTRAMBI i
                                    // rami fa sì che `hit_rx`/`unknown_rx` vengano droppati alla fine
                                    // di `run`, e ogni `blocking_send` pendente riceva `Err` e
                                    // sblocchi il walker.
                                    hit_rx_open = false;
                                    unknown_rx_open = false;
                                }
                            }
                        }
                        None => hit_rx_open = false,
                    }
                }
                item = unknown_rx.recv(), if unknown_rx_open => {
                    match item {
                        Some(pair) => unknown_queue.push(pair),
                        None => unknown_rx_open = false,
                    }
                }
            }
        }

        // Fase B — entrambi i canali sono chiusi: ogni walker è finito per
        // davvero. Sniffa e cerca i candidati Unknown accumulati.
        if let Some((matcher, content_cfg)) = content {
            for (scanned, (path, source)) in unknown_queue.into_iter().enumerate() {
                if cancel.is_cancelled() {
                    break;
                }
                if scanned >= content_cfg.max_unknown_scan {
                    truncated = true;
                    break;
                }

                let matcher_clone = matcher.clone();
                let max_kb = content_cfg.max_file_size_kb;
                let path_for_blocking = path.clone();
                let gate_clone = Arc::clone(&gate);
                let cancel_clone = cancel.clone();
                let found = tokio::task::spawn_blocking(move || {
                    // Onora la pausa (stesso pattern di walk_root, walk.rs): il thread
                    // blocking si parcheggia qui finché l'utente non riprende o annulla la
                    // ricerca. Il check di pausa PRIMA di quello di cancellazione è
                    // deliberato — non c'è nessun "resume interno": `wait_while_paused`
                    // ripolla `cancel` internamente ogni 100ms (vedi la sua Condvar
                    // `wait_timeout`), quindi un cancel arrivato durante la pausa sblocca
                    // comunque il parcheggio entro quella finestra, senza bisogno di un
                    // resume esplicito (vedi `PauseGate::wait_while_paused`).
                    gate_clone.wait_while_paused(&cancel_clone);
                    if cancel_clone.is_cancelled() {
                        return None;
                    }
                    if !crate::search::content::sniff_file(&path_for_blocking) {
                        return None;
                    }
                    matcher_clone.find_first_match(&path_for_blocking, max_kb)
                })
                .await
                .unwrap_or(None);

                if let Some((line, snippet)) = found {
                    if tx.send(ServerMsg::SearchHit {
                        id: id.to_string(),
                        path: crate::search::walk::normalize_path_string(&path),
                        source,
                        line: Some(line),
                        snippet: Some(snippet),
                    }).is_err() {
                        break;
                    }
                    count += 1;
                    if count >= result_cap {
                        truncated = true;
                        cancel.cancel();
                        break;
                    }
                }
            }
        }

        // Emit SearchDone regardless of truncation.
        // If tx is closed at this point, the send failure is silently ignored
        // (receiver is gone, nothing left to report to).
        let _ = tx.send(ServerMsg::SearchDone { id: id.to_string(), count, truncated });
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::paths_config::ExternalMode;
    use protocol::SearchSource;
    use std::collections::HashMap;
    use tempfile::TempDir;
    use tokio::sync::mpsc;
    use tokio_util::sync::CancellationToken;

    // ── FakeProvider: provider vuoto per i test di launch ───────────────────
    struct FakeProvider;
    impl PathProvider for FakeProvider {
        fn standard_dirs(&self) -> Vec<std::path::PathBuf> { vec![] }
        fn detect_cloud(&self) -> Vec<std::path::PathBuf> { vec![] }
        fn external_drives(&self) -> Vec<std::path::PathBuf> { vec![] }
    }

    // ── Test: launch rilegge result_cap da disco ─────────────────────────────

    /// Verifica che `launch` rileva `result_cap` dal file a ogni chiamata (config live).
    /// Il file viene scritto con `result_cap=2`; la ricerca su 5 file `.pdf` deve
    /// restituire `SearchDone { count: 2, truncated: true }`.
    #[tokio::test]
    async fn launch_reloads_result_cap_from_disk() {
        use protocol::ServerMsg;
        let tmp = TempDir::new().unwrap();
        // cwd con 5 file .pdf
        for i in 0..5 { std::fs::write(tmp.path().join(format!("f{i}.pdf")), b"").unwrap(); }
        // search-paths.json con result_cap=2
        let cfg_dir = TempDir::new().unwrap();
        let cfg_path = cfg_dir.path().join("search-paths.json");
        std::fs::write(&cfg_path, r#"{"standard":[],"cloud":[],"external":"auto","max_depth":8,"result_cap":2,"exclude":[],"version":2}"#).unwrap();
        let content_cfg_path = cfg_dir.path().join("search-content.json"); // riusa cfg_dir già presente nel test

        let ctx = SearchContext {
            engine: Arc::new(SearchEngine::default()),
            cfg_path: Arc::new(cfg_path),
            content_cfg_path: Arc::new(content_cfg_path),
            provider: Arc::new(FakeProvider),
        };
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ServerMsg>();
        let cancel = tokio_util::sync::CancellationToken::new();
        let gate = PauseGate::new();
        launch(&ctx, "s1", "*.pdf", tmp.path(), &tx, cancel, gate).await;
        drop(tx);

        let mut done = None;
        while let Ok(m) = rx.try_recv() {
            if let ServerMsg::SearchDone { count, truncated, .. } = m { done = Some((count, truncated)); }
        }
        assert_eq!(done, Some((2, true)), "il reload deve applicare result_cap=2");
    }

    // ── Test: launch fa lo split `in:"..."` e attiva la ricerca contenuto ────

    /// Verifica che `launch` (non `SearchEngine::run` direttamente) faccia da
    /// sola lo split `in:"<frase>"` via `parse_find_input` e costruisca il
    /// `ContentMatcher`/`ContentConfig` per attivare la ricerca contenuto —
    /// il pezzo di wiring che Task 8 aveva lasciato come placeholder (`None`
    /// fisso, vedi commento rimosso da `launch`).
    #[tokio::test]
    async fn launch_finds_content_match_via_in_directive() {
        use protocol::ServerMsg;
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("note.txt"), "primo\ncontiene TOTALE qui\n").unwrap();

        let cfg_dir = TempDir::new().unwrap();
        let cfg_path = cfg_dir.path().join("search-paths.json");
        std::fs::write(&cfg_path, r#"{"standard":[],"cloud":[],"external":"auto","max_depth":8,"result_cap":2000,"exclude":[],"version":2}"#).unwrap();
        let content_cfg_path = cfg_dir.path().join("search-content.json");
        std::fs::write(&content_cfg_path, r#"{"text_extensions":["txt"],"binary_extensions":[],"max_file_size_kb":5120,"max_unknown_scan":100,"version":1}"#).unwrap();

        let ctx = SearchContext {
            engine: Arc::new(SearchEngine::default()),
            cfg_path: Arc::new(cfg_path),
            content_cfg_path: Arc::new(content_cfg_path),
            provider: Arc::new(FakeProvider),
        };
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ServerMsg>();
        launch(&ctx, "s1", r#"in:"totale" *.txt"#, tmp.path(), &tx, tokio_util::sync::CancellationToken::new(), PauseGate::new()).await;
        drop(tx);

        let mut hit_found = false;
        while let Ok(m) = rx.try_recv() {
            if let ServerMsg::SearchHit { line: Some(2), .. } = m { hit_found = true; }
        }
        assert!(hit_found, "atteso un hit di contenuto tramite launch()");
    }

    // ── Test: launch + folder:from-here restringe al solo Cwd ────────────────

    /// `folder:from-here` deve restringere la ricerca al solo root Cwd, anche
    /// con Standard/Cloud configurati e popolati di file che altrimenti
    /// matcherebbero — prova end-to-end che `launch` passa `parsed.from_here`
    /// a `resolve_roots` correttamente (non solo `resolve_roots` in
    /// isolamento, Task 2).
    #[tokio::test]
    async fn launch_folder_from_here_restricts_to_cwd_only() {
        use protocol::ServerMsg;
        let cwd_dir = TempDir::new().unwrap();
        std::fs::write(cwd_dir.path().join("here.pdf"), b"").unwrap();

        let std_dir = TempDir::new().unwrap();
        std::fs::write(std_dir.path().join("elsewhere.pdf"), b"").unwrap();
        let cloud_dir = TempDir::new().unwrap();
        std::fs::write(cloud_dir.path().join("elsewhere2.pdf"), b"").unwrap();

        let cfg_dir = TempDir::new().unwrap();
        let cfg_path = cfg_dir.path().join("search-paths.json");
        let cfg_json = format!(
            r#"{{"standard":[{:?}],"cloud":[{:?}],"external":"auto","max_depth":8,"result_cap":2000,"exclude":[],"version":2}}"#,
            std_dir.path().to_string_lossy(),
            cloud_dir.path().to_string_lossy(),
        );
        std::fs::write(&cfg_path, cfg_json).unwrap();
        let content_cfg_path = cfg_dir.path().join("search-content.json");

        let ctx = SearchContext {
            engine: Arc::new(SearchEngine::default()),
            cfg_path: Arc::new(cfg_path),
            content_cfg_path: Arc::new(content_cfg_path),
            provider: Arc::new(FakeProvider),
        };
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ServerMsg>();
        launch(&ctx, "s1", "folder:from-here *.pdf", cwd_dir.path(), &tx, tokio_util::sync::CancellationToken::new(), PauseGate::new()).await;
        drop(tx);

        let mut hits = Vec::new();
        while let Ok(m) = rx.try_recv() {
            if let ServerMsg::SearchHit { path, .. } = m { hits.push(path); }
        }
        assert_eq!(hits.len(), 1, "atteso solo l'hit della cwd, got {hits:?}");
        assert!(hits[0].ends_with("here.pdf"), "atteso here.pdf, got {hits:?}");
    }

    // ── Helper: build a minimal PathsConfig ──────────────────────────────────

    fn minimal_cfg(result_cap: usize) -> PathsConfig {
        PathsConfig {
            standard: vec![],
            cloud: vec![],
            external: ExternalMode::List(vec![]),
            max_depth: 8,
            exclude: vec![],
            result_cap,
            version: crate::search::paths_config::CURRENT_CONFIG_VERSION,
        }
    }

    // ── Helper: create a file at dir/name ────────────────────────────────────

    fn create_file(dir: &std::path::Path, name: &str) {
        std::fs::write(dir.join(name), b"").unwrap();
    }

    // ── Test 1: emits_open_hits_done_with_sources ─────────────────────────────

    /// Two roots (Cwd with 1 matching file, Cloud with 2 matching files).
    /// Expected sequence:
    ///   - first message: SearchOpen
    ///   - 3 SearchHit (order not guaranteed; count by source)
    ///   - last message: SearchDone { count: 3, truncated: false }
    #[tokio::test]
    async fn emits_open_hits_done_with_sources() {
        // Cwd root: 1 .pdf file
        let tmp_cwd = TempDir::new().unwrap();
        create_file(tmp_cwd.path(), "alpha.pdf");

        // Cloud root: 2 .pdf files
        let tmp_cloud = TempDir::new().unwrap();
        create_file(tmp_cloud.path(), "beta.pdf");
        create_file(tmp_cloud.path(), "gamma.pdf");

        let roots = vec![
            Root { path: tmp_cwd.path().to_path_buf(),   source: SearchSource::Cwd,   prune: vec![], max_depth: 8 },
            Root { path: tmp_cloud.path().to_path_buf(), source: SearchSource::Cloud, prune: vec![], max_depth: 8 },
        ];

        let cfg = minimal_cfg(1000);
        let engine = SearchEngine::default();
        let (tx, mut rx) = mpsc::unbounded_channel::<ServerMsg>();
        let cancel = CancellationToken::new();
        let gate = PauseGate::new();

        engine.run("s1", "/find *.pdf", "*.pdf", roots, &cfg, &tx, cancel, gate, None).await;
        drop(tx); // close the channel so try_recv terminates

        // Collect all messages
        let mut msgs = Vec::new();
        while let Ok(m) = rx.try_recv() {
            msgs.push(m);
        }

        // First message must be SearchOpen
        assert!(
            matches!(&msgs[0], ServerMsg::SearchOpen { id, .. } if id == "s1"),
            "first message must be SearchOpen, got: {:?}", msgs.first()
        );

        // Last message must be SearchDone { count: 3, truncated: false }
        assert!(
            matches!(&msgs[msgs.len() - 1], ServerMsg::SearchDone { id, count: 3, truncated: false } if id == "s1"),
            "last message must be SearchDone {{ count: 3, truncated: false }}, got: {:?}", msgs.last()
        );

        // Exactly 3 SearchHit in the middle (indices 1..=3)
        let hits: Vec<&ServerMsg> = msgs[1..msgs.len()-1].iter().collect();
        assert_eq!(hits.len(), 3, "expected 3 SearchHit messages, got {}", hits.len());

        // Count hits by source
        let mut by_source: HashMap<String, usize> = HashMap::new();
        for h in &hits {
            if let ServerMsg::SearchHit { source, .. } = h {
                let key = format!("{source:?}");
                *by_source.entry(key).or_insert(0) += 1;
            }
        }
        assert_eq!(*by_source.get("Cwd").unwrap_or(&0), 1, "expected 1 Cwd hit; map: {by_source:?}");
        assert_eq!(*by_source.get("Cloud").unwrap_or(&0), 2, "expected 2 Cloud hits; map: {by_source:?}");
    }

    // ── Test 2: caps_results_and_marks_truncated ──────────────────────────────

    /// One root with 5 matching files, result_cap=2.
    /// Expected: at most 2 SearchHit, SearchDone { truncated: true, count: 2 }.
    #[tokio::test]
    async fn caps_results_and_marks_truncated() {
        let tmp = TempDir::new().unwrap();
        for i in 0..5 {
            create_file(tmp.path(), &format!("file{i}.pdf"));
        }

        let roots = vec![
            Root { path: tmp.path().to_path_buf(), source: SearchSource::Cwd, prune: vec![], max_depth: 8 },
        ];

        let cfg = minimal_cfg(2); // cap at 2 results
        let engine = SearchEngine::default();
        let (tx, mut rx) = mpsc::unbounded_channel::<ServerMsg>();
        let cancel = CancellationToken::new();
        let gate = PauseGate::new();

        engine.run("s2", "/find *.pdf", "*.pdf", roots, &cfg, &tx, cancel, gate, None).await;
        drop(tx);

        let mut msgs = Vec::new();
        while let Ok(m) = rx.try_recv() {
            msgs.push(m);
        }

        // Count SearchHit messages
        let hit_count = msgs.iter().filter(|m| matches!(m, ServerMsg::SearchHit { .. })).count();
        assert!(
            hit_count <= 2,
            "expected at most 2 hits due to cap, got {hit_count}"
        );

        // Last message must be SearchDone { truncated: true, count: 2 }
        assert!(
            matches!(&msgs[msgs.len() - 1], ServerMsg::SearchDone { id, count: 2, truncated: true } if id == "s2"),
            "last message must be SearchDone {{ count: 2, truncated: true }}, got: {:?}", msgs.last()
        );
    }

    use crate::search::content::{ContentConfig, ContentMatcher};

    fn content_cfg_for_test() -> ContentConfig {
        ContentConfig {
            text_extensions: vec!["txt".to_string()],
            binary_extensions: vec!["bin".to_string()],
            max_file_size_kb: 5120,
            max_unknown_scan: 100,
            version: 1,
        }
    }

    // ── Test 3: content search — file Text trovato in Fase A ──────────────────

    #[tokio::test]
    async fn content_search_finds_text_file_in_phase_a() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("note.txt"), "primo\ncontiene TOTALE qui\n").unwrap();

        let roots = vec![Root { path: tmp.path().to_path_buf(), source: SearchSource::Cwd, prune: vec![], max_depth: 8 }];
        let cfg = minimal_cfg(1000);
        let engine = SearchEngine::default();
        let (tx, mut rx) = mpsc::unbounded_channel::<ServerMsg>();

        engine
            .run(
                "s3", "/find in:\"totale\" *.txt", "*.txt", roots, &cfg, &tx,
                CancellationToken::new(), PauseGate::new(),
                Some((ContentMatcher::new("totale"), content_cfg_for_test())),
            )
            .await;
        drop(tx);

        let mut msgs = Vec::new();
        while let Ok(m) = rx.try_recv() { msgs.push(m); }

        let hit = msgs.iter().find(|m| matches!(m, ServerMsg::SearchHit { .. })).expect("atteso 1 hit");
        if let ServerMsg::SearchHit { line, snippet, .. } = hit {
            assert_eq!(*line, Some(2));
            assert_eq!(snippet.as_deref(), Some("contiene TOTALE qui"));
        }
        assert!(matches!(msgs.last(), Some(ServerMsg::SearchDone { count: 1, truncated: false, .. })));
    }

    // ── Test 4: content search — file Unknown trovato in Fase B ────────────────

    #[tokio::test]
    async fn content_search_finds_unknown_extension_file_in_phase_b() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("mystery.xyz"), "roba varia\ncontiene TOTALE qui\n").unwrap();

        let roots = vec![Root { path: tmp.path().to_path_buf(), source: SearchSource::Cwd, prune: vec![], max_depth: 8 }];
        let cfg = minimal_cfg(1000);
        let engine = SearchEngine::default();
        let (tx, mut rx) = mpsc::unbounded_channel::<ServerMsg>();

        // Query nome "*" (equivalente a nessun filtro — vedi la nota su
        // parse_find_input nel Task 2: `run` prende la query nome GIÀ risolta,
        // non fa MAI da sola il fallback "" → "*", quella è responsabilità di
        // `launch`/`parse_find_input` a monte) ⇒ *.xyz è "Unknown" per
        // content_cfg_for_test() (solo txt/bin classificati) e finisce in Fase B.
        engine
            .run(
                "s4", "/find in:\"totale\"", "*", roots, &cfg, &tx,
                CancellationToken::new(), PauseGate::new(),
                Some((ContentMatcher::new("totale"), content_cfg_for_test())),
            )
            .await;
        drop(tx);

        let mut msgs = Vec::new();
        while let Ok(m) = rx.try_recv() { msgs.push(m); }

        let hit = msgs.iter().find(|m| matches!(m, ServerMsg::SearchHit { .. })).expect("atteso 1 hit dalla Fase B");
        if let ServerMsg::SearchHit { path, line, .. } = hit {
            assert!(path.ends_with("mystery.xyz"));
            assert_eq!(*line, Some(2));
        }
        assert!(matches!(msgs.last(), Some(ServerMsg::SearchDone { count: 1, truncated: false, .. })));
    }

    // ── Test 5: senza content-search, comportamento invariato ──────────────────

    #[tokio::test]
    async fn no_content_search_behaves_exactly_like_before() {
        let tmp = TempDir::new().unwrap();
        create_file(tmp.path(), "a.pdf");
        let roots = vec![Root { path: tmp.path().to_path_buf(), source: SearchSource::Cwd, prune: vec![], max_depth: 8 }];
        let cfg = minimal_cfg(1000);
        let engine = SearchEngine::default();
        let (tx, mut rx) = mpsc::unbounded_channel::<ServerMsg>();

        engine.run("s5", "/find *.pdf", "*.pdf", roots, &cfg, &tx, CancellationToken::new(), PauseGate::new(), None).await;
        drop(tx);

        let mut msgs = Vec::new();
        while let Ok(m) = rx.try_recv() { msgs.push(m); }
        assert!(matches!(msgs.last(), Some(ServerMsg::SearchDone { count: 1, truncated: false, .. })));
    }

    // ── Test 6: regressione deadlock — >256 file Unknown non deve bloccare ─────

    #[tokio::test]
    async fn content_search_does_not_deadlock_on_many_unknown_files() {
        let tmp = TempDir::new().unwrap();
        // Più di 256 (capacità storica del canale unknown_tx) file con
        // estensione Unknown per content_cfg_for_test() (solo txt/bin noti).
        for i in 0..300 {
            std::fs::write(tmp.path().join(format!("f{i}.xyz")), "niente di interessante\n").unwrap();
        }

        let roots = vec![Root { path: tmp.path().to_path_buf(), source: SearchSource::Cwd, prune: vec![], max_depth: 8 }];
        let cfg = minimal_cfg(10_000); // result_cap alto: non deve troncare qui
        let engine = SearchEngine::default();
        let (tx, mut rx) = mpsc::unbounded_channel::<ServerMsg>();

        let run_fut = engine.run(
            "s6", "/find in:\"totale\"", "*", roots, &cfg, &tx,
            CancellationToken::new(), PauseGate::new(),
            Some((ContentMatcher::new("totale"), content_cfg_for_test())),
        );

        // Se il fix non regge, questo va in timeout invece di hang-are per sempre.
        tokio::time::timeout(std::time::Duration::from_secs(10), run_fut)
            .await
            .expect("run() non deve fare deadlock con >256 file Unknown");
        drop(tx);

        let mut msgs = Vec::new();
        while let Ok(m) = rx.try_recv() { msgs.push(m); }
        assert!(matches!(msgs.last(), Some(ServerMsg::SearchDone { .. })), "atteso SearchDone, got {msgs:?}");
    }

    // ── Test 7: regressione — Fase B deve onorare il PauseGate (spec §5) ──────

    /// Prova che la Fase B (sniff+cerca dei file `Unknown` accodati) rispetta
    /// davvero il `PauseGate` — non solo la Fase A (`walk_root`, già testato
    /// in `walk.rs`).
    ///
    /// Design: nel corpus ci sono SOLO file `Unknown` (nessun `Text`), tutti
    /// che matchano per contenuto. La Fase A quindi non emette MAI un
    /// `SearchHit` da sola (i file Unknown vengono solo accodati, non
    /// "hit"tati direttamente) — quindi il PRIMO `SearchHit` che arriva su
    /// `rx` è una prova diretta e inequivocabile che: (a) la Fase A è
    /// terminata (per costruzione: `unknown_rx` si chiude solo quando ogni
    /// walker è finito) e (b) la Fase B ha già iniziato a processare la coda.
    /// Mettiamo in pausa esattamente in quel momento — nessun timing-guessing
    /// sulla finestra fra le due fasi, il segnale è un messaggio reale sul
    /// canale, non un ritardo arbitrario.
    #[tokio::test]
    async fn phase_b_respects_pause_gate() {
        let tmp = TempDir::new().unwrap();
        // 30 file "Unknown" che matchano TUTTI per contenuto: il primo hit
        // arriva presto (già al 1° file processato dalla Fase B), lasciando
        // fino a ~29 file ancora da processare nella coda — margine
        // sufficiente perché il codice SENZA il fix (che ignora il gate)
        // continui a produrre hit durante la finestra di pausa, rendendo il
        // test discriminante (RED sul codice vecchio, GREEN su quello nuovo).
        const N: usize = 30;
        for i in 0..N {
            std::fs::write(tmp.path().join(format!("f{i}.xyz")), "contiene TOTALE qui\n").unwrap();
        }

        let roots = vec![Root { path: tmp.path().to_path_buf(), source: SearchSource::Cwd, prune: vec![], max_depth: 8 }];
        let cfg = minimal_cfg(1000);
        let engine = SearchEngine::default();
        let (tx, mut rx) = mpsc::unbounded_channel::<ServerMsg>();
        let gate = PauseGate::new();
        let content = Some((ContentMatcher::new("totale"), content_cfg_for_test()));

        let run_fut = engine.run(
            "s7", "/find in:\"totale\"", "*", roots, &cfg, &tx,
            CancellationToken::new(), Arc::clone(&gate), content,
        );
        tokio::pin!(run_fut);

        // Fase 1: lascia correre finché non arriva il PRIMO SearchHit — prova
        // diretta che la Fase B ha iniziato a lavorare sulla coda.
        loop {
            tokio::select! {
                _ = &mut run_fut => panic!(
                    "run() è completata prima che arrivasse alcun hit — nessun file ha matchato? il test non può provare nulla sulla Fase B"
                ),
                msg = rx.recv() => {
                    // SearchHit → prova diretta che la Fase B ha iniziato a lavorare;
                    // qualunque altro messaggio (es. SearchOpen) va ignorato, si continua
                    // ad aspettare.
                    if let ServerMsg::SearchHit { .. } = msg.expect("canale chiuso inaspettatamente") {
                        break;
                    }
                }
            }
        }

        // Ora la Fase B è sicuramente a metà lavoro (almeno 1 file fatto, ne
        // restano fino a N-1): mettiamo in pausa QUI.
        gate.pause();

        // Fase 2: con la pausa attiva, `run()` non deve completare. Se lo fa
        // (codice non corretto: Fase B ignora il gate), il lavoro residuo
        // (fino a N-1 file, ciascuno minuscolo, in una tempdir) verrebbe
        // comunque completato ben entro questa finestra.
        let still_running = tokio::time::timeout(std::time::Duration::from_millis(300), &mut run_fut).await;
        assert!(
            still_running.is_err(),
            "run() ha completato mentre in pausa — la Fase B ignora il PauseGate"
        );

        // Riprendi: la ricerca deve completare e produrre TUTTI gli hit attesi.
        gate.resume();
        tokio::time::timeout(std::time::Duration::from_secs(5), &mut run_fut)
            .await
            .expect("run() deve completare dopo il resume");

        // Drena i messaggi rimanenti (il primo hit è già stato consumato
        // sopra, nel loop della Fase 1 — non è più in `rx`, va contato a parte).
        let mut remaining = Vec::new();
        while let Ok(m) = rx.try_recv() { remaining.push(m); }

        let remaining_hits = remaining.iter().filter(|m| matches!(m, ServerMsg::SearchHit { .. })).count();
        assert_eq!(
            remaining_hits + 1, N,
            "attesi {N} hit totali dopo il resume, got {} (+1 già consumato in Fase 1)", remaining_hits
        );

        assert!(
            matches!(remaining.last(), Some(ServerMsg::SearchDone { count, truncated: false, .. }) if *count == N),
            "atteso SearchDone {{ count: {N}, truncated: false }}, got {:?}", remaining.last()
        );
    }
}
