// Lare Terminal — crate `ui`, src-tauri/src/main.rs
//
// Tauri v2 overlay window — 0.4.0: multi-window Markdown output (ADR-013).
//
// Architecture (SOLID / thin Rust):
//   - Rust side: window lifecycle, global-shortcut (config-driven), config
//     persistence, safe token retrieval, and multi-window Markdown support.
//   - JS side: WebSocket client, rendering, keyboard handling, /config dialog,
//     window.js for Markdown window content retrieval and render.
//
// Multi-window content passing strategy (ADR-013):
//   - `open_markdown_window(title, content, kind)` stores (title, content, kind, source_file) in a
//     `Mutex<HashMap<label, (title, content, kind, source_file)>>` BEFORE creating the window.
//   - The new window loads `window.html` and calls `take_window_content` at startup
//     to retrieve and remove the entry (derives label server-side via WebviewWindow).
//   - `kind` (e.g. "markdown", "help") lets window.js apply CSS for special windows.
//   - This avoids URL query-param size limits and race conditions; the content
//     is always present when the new window's JS executes.
//
// Prevents a console window from appearing on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod aichat_settings;
mod config_dir;
mod llm_settings;
mod market_data_settings;
mod plugins_view;
mod search_settings;

/// Riconosce il comando di uscita pulita digitato sullo stdin interattivo ("Q"/"quit",
/// case-insensitive, spazi ai bordi ignorati). Vedi il thread stdin-reader in `.setup()`:
/// senza un modo esplicito di chiedere l'uscita, l'unica opzione era Ctrl+C/chiudere il
/// terminale — interruzione brusca che lascia WebView2 a metà teardown (log "Failed to
/// unregister class Chrome_WidgetWin_0", innocuo ma rumoroso). Quando i processi
/// gireranno come servizi, l'uscita pulita sarà innescata dallo stop del servizio.
fn is_quit_command(line: &str) -> bool {
    matches!(line.trim().to_lowercase().as_str(), "q" | "quit")
}

use config_dir::ConfigDirState;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};
use ui_lib::archive;
use ui_lib::config::{self, Config};
use ui_lib::launcher;
use ui_lib::library_watch;

// ---------------------------------------------------------------------------
// Managed state: the current config, protected by a Mutex for interior
// mutability across Tauri commands (which take &self behind a &AppHandle).
// ---------------------------------------------------------------------------

/// Newtype wrapper so Tauri's managed-state lookup is unambiguous.
struct ConfigState(Mutex<Config>);

// ---------------------------------------------------------------------------
// Managed state: Markdown window content store (ADR-013)
//
// Maps window label → (title, content) for Markdown windows.
// Entries are inserted BEFORE the window is created (so the content is
// guaranteed to exist when window.js calls take_window_content), then
// removed by take_window_content (one-shot: prevents memory leaks).
// ---------------------------------------------------------------------------

/// Stores pending Markdown window content keyed by unique window label.
/// Tuple fields: (title, content, kind, source_file).
/// `kind` mirrors `protocol::WindowKind` wire value (e.g. `"markdown"`, `"help"`).
/// `source_file` is the Library rel-path this window was opened from, or ""
/// if it wasn't opened from the Library (fresh AI/show_markdown output).
struct WindowContentStore(Mutex<HashMap<String, (String, String, String, String)>>);

// ---------------------------------------------------------------------------
// Managed state: Library filesystem watcher guard (Slice 3 fs-watch).
//
// WatchGuard non è Sync (il watcher interno usa HANDLE non-Sync su Windows),
// quindi lo avvolgiamo in Mutex per soddisfare il bound Send+Sync+'static
// richiesto da app.manage(). Il Mutex non viene mai locked a runtime —
// serve solo per la derivazione di Sync.
// ---------------------------------------------------------------------------

/// Newtype wrapper attorno al WatchGuard del filesystem watch.
/// Il Mutex rende il tipo Sync (richiesto da Tauri manage) senza overhead
/// a runtime: il guard viene creato una sola volta e tenuto vivo finché
/// l'app gira, senza mai essere riletto.
///
/// `#[allow(dead_code)]`: il campo .0 non è mai letto a runtime — il suo
/// scopo è soltanto tenere vivo il watcher (RAII). Il compilatore vede il
/// campo come "mai letto" perché non lo accediamo mai dopo app.manage().
#[allow(dead_code)]
struct LibraryWatchGuard(Mutex<library_watch::WatchGuard>);

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// Legge il token dal file `<config_dir>/token` — STESSA cartella
/// dell'orchestrator (che lo genera al primo avvio, vedi
/// `orchestrator::token_store::resolve_token`), STESSO file: nessuna env var
/// (2.0, decisione D6). `config_dir` viene dallo stato gestito da Tauri
/// (`ConfigDirState`, risolto una volta in `main()`), non ri-derivato qui.
/// Stringa vuota se il file è assente/vuoto — la UI mostra lo stato
/// "errore" e innesca l'auto-diagnosi, come in v1.
fn read_lare_token(app: &AppHandle) -> String {
    config_dir::read_token(&app.state::<ConfigDirState>().config_dir)
}

/// Return the resolved Lare auth token to the frontend.
///
/// The frontend uses this to authenticate the WebSocket connection to the
/// orchestrator.  Returns an empty string if no token is available (the
/// token file is absent or empty); the frontend will then trigger
/// auto-diagnosis.
#[tauri::command]
fn get_lare_token(app: AppHandle) -> String {
    read_lare_token(&app)
}

/// Endpoint WebSocket dell'orchestrator, per il frontend (2.0: la porta è
/// configurabile via `startup.json`, non più hardcoded — vedi `ws-client.js`,
/// che la richiede con questo comando prima di aprire la connessione invece
/// di tenere una costante `WS_URL` propria).
#[tauri::command]
fn get_ws_endpoint(state: State<'_, ConfigDirState>) -> String {
    state.ws_endpoint()
}

/// Structured result returned by `diagnose_connection`.
#[derive(serde::Serialize)]
struct ConnectionDiagnosis {
    /// Whether the token file (`<config_dir>/token`) resolves to a non-empty value.
    token_set: bool,
    /// Whether the orchestrator's WS port accepted a connection within a short
    /// timeout.  True = orchestrator is listening; false = not up.
    port_open: bool,
}

/// Run a quick connectivity self-check and return the results.
///
/// Called by the frontend after persistent connection failures to surface
/// actionable guidance via a Markdown window.  Two checks:
///   1. Is a token available (`<config_dir>/token`)?
///   2. Can we open a TCP connection to `127.0.0.1:<ws_port>` within 400 ms?
///      `ws_port` viene da `startup.json` (via `ConfigDirState`), non più
///      hardcoded a 7331 — altrimenti, con una porta personalizzata, questa
///      diagnosi mentirebbe all'utente controllando la porta sbagliata.
///
/// The TCP check is blocking (≤400 ms) but runs on Tauri's thread pool, so
/// it does not block the UI event loop.
#[tauri::command]
fn diagnose_connection(app: AppHandle, state: State<'_, ConfigDirState>) -> ConnectionDiagnosis {
    let token_set = !read_lare_token(&app).is_empty();

    let addr = format!("127.0.0.1:{}", state.startup.ws_port);
    let port_open = std::net::TcpStream::connect_timeout(
        &addr.parse().expect("host:port letterale sempre valido"),
        std::time::Duration::from_millis(400),
    )
    .is_ok();

    ConnectionDiagnosis {
        token_set,
        port_open,
    }
}

/// Sink Tauri per `pty::PtyOutputSink`: inoltra output/uscita come eventi
/// `pty-out`/`pty-exit` alla finestra terminale. `Emitter::emit` è globale
/// (raggiunge tutte le finestre in ascolto) — un solo `AppHandle` per
/// processo, coerente con "una finestra, una sessione" dell'MVP.
struct AppHandleSink(tauri::AppHandle);
impl ui_lib::pty::PtyOutputSink for AppHandleSink {
    fn on_output(&self, chunk: &str) {
        let _ = self.0.emit("pty-out", chunk);
    }
    fn on_exit(&self, code: i64) {
        let _ = self.0.emit("pty-exit", code);
    }
}

struct PtyState(ui_lib::pty::SharedPtyState);

/// true se `ui.exe` è stato avviato in ruolo "solo host" (`--no-terminal`,
/// piano 3): non apre la finestra terminale — resta solo la host page
/// nascosta + le finestre aperte on-demand (config, library, ...).
///
/// `#[allow(dead_code)]`: il campo .0 non è ancora letto da nessun comando
/// in questo task — lo sarà dalla finestra terminale (Task 4), che decide
/// se aprirsi consultando questo stato gestito.
#[allow(dead_code)]
struct NoTerminal(bool);

/// Avvia `lare-shell.exe --config-dir <dir> --session <id>` nella pty della
/// finestra terminale. `session_id` arriva dal frontend, che lo ha ottenuto
/// da `get_terminal_session` (Task 4) — un solo id per finestra, generato
/// UNA volta alla creazione della finestra, non ad ogni riavvio della shell.
#[tauri::command]
fn pty_spawn(
    app: AppHandle,
    state: State<'_, PtyState>,
    cfg: State<'_, ConfigDirState>,
    session_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let exe = cfg.shell_exe();
    let exe_str = exe
        .to_str()
        .ok_or("percorso di lare-shell.exe non è UTF-8")?
        .to_string();
    let sink = std::sync::Arc::new(AppHandleSink(app));
    ui_lib::pty::spawn(
        &state.0,
        sink,
        &exe_str,
        &[
            "--config-dir".to_string(),
            cfg.config_dir.to_string_lossy().into_owned(),
            "--session".to_string(),
            session_id,
        ],
        None,
        cols,
        rows,
    )
}

#[tauri::command]
fn pty_write(state: State<'_, PtyState>, data: String) -> Result<(), String> {
    ui_lib::pty::write(&state.0, data.as_bytes())
}

#[tauri::command]
fn pty_resize(state: State<'_, PtyState>, cols: u16, rows: u16) -> Result<(), String> {
    ui_lib::pty::resize(&state.0, cols, rows)
}

/// Return a copy of the current config to the frontend.
///
/// The frontend uses this to populate the /config dialog and apply CSS vars
/// on startup.
#[tauri::command]
fn get_config(state: State<'_, ConfigState>) -> Config {
    state.0.lock().unwrap().clone()
}

/// Persist and hot-apply a new config.
///
/// Task 7 (piano 1): senza overlay non c'è più un tasto d'attivazione da
/// validare né un hotkey globale da ri-registrare — questo comando è ora
/// solo persist + aggiornamento dello stato in memoria. L'aspetto (alpha) e
/// il flag di ricerca web sono letti dalle finestre via `get_config`.
///
/// Returns `Ok(())` on success or `Err(human-readable message)` on any failure.
#[tauri::command]
fn set_config(
    new_cfg: Config,
    app: AppHandle,
    state: State<'_, ConfigState>,
) -> Result<(), String> {
    // Persist.
    let config_path = config_file_path(&app)?;
    config::save_to(&new_cfg, &config_path)?;

    // Update in-memory state.
    let mut guard = state.0.lock().unwrap();
    *guard = new_cfg;

    Ok(())
}

// ---------------------------------------------------------------------------
// Markdown window commands (ADR-013)
// ---------------------------------------------------------------------------

/// Open a new Markdown window displaying `content`.
///
/// Strategy:
///   1. Generate a unique label `md-<unix_ms>-<counter>`.
///   2. Insert (title, content, kind, source_file) into `WindowContentStore` BEFORE creating
///      the window — content must be present when window.js calls back.
///   3. Create a `WebviewWindow` loading `window.html`.
///      The window reads its label server-side via `take_window_content`.
///
/// `kind` is a string matching `protocol::WindowKind` wire values (`"markdown"`,
/// `"help"`, …).  `window.js` reads it and applies a CSS class for special
/// window types (e.g. `body.help` for `kind == "help"`).
///
/// # Why async?
/// `WebviewWindowBuilder::build()` **deadlocks in synchronous Tauri commands
/// on Windows** (Tauri v2 documented limitation).  Declaring this command
/// `async` routes it through Tokio and avoids the deadlock.
///
/// # Content passing
/// Content is stored in managed state (not query params) to avoid URL-length
/// limits for large Markdown payloads and to guarantee atomicity.
#[tauri::command]
async fn open_markdown_window(
    title: String,
    content: String,
    kind: String,
    source_file: String,
    label: Option<String>,
    app: AppHandle,
    store: State<'_, WindowContentStore>,
) -> Result<(), String> {
    // Singleton (D15): con una label fissa (`/help`), se la finestra esiste
    // già la si porta in primo piano e non se ne apre una seconda.
    if let Some(fixed) = &label {
        if let Some(win) = app.get_webview_window(fixed) {
            win.set_focus().map_err(|e| format!("open_markdown_window set_focus error: {e}"))?;
            return Ok(());
        }
    }
    let label = match label {
        Some(fixed) => fixed,
        None => {
            // Step 1: generate a unique label.
            // Format: "md-<unix_ms>-<monotonic_counter>"
            // No external uuid crate; a timestamp + process-local AtomicU32 counter is
            // collision-resistant for a local overlay UI (single process, single user).
            use std::sync::atomic::{AtomicU32, Ordering};
            use std::time::{SystemTime, UNIX_EPOCH};
            let ts = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            static CTR: AtomicU32 = AtomicU32::new(0);
            let ctr = CTR.fetch_add(1, Ordering::Relaxed);
            format!("md-{ts}-{ctr}")
        }
    };

    // Step 2: store content BEFORE creating the window.
    // The guard is dropped (by end of block) before build() is called.
    {
        let mut map = store.0.lock().map_err(|e| format!("lock error: {e}"))?;
        map.insert(label.clone(), (title.clone(), content, kind, source_file));
    } // lock released here

    // Step 3: create the WebviewWindow (estratto in build_markdown_window,
    // riusato anche da open_output_window).
    build_markdown_window(&app, &label, &title)
}

/// Crea una finestra Markdown chromeless (stile di ogni finestra Lare) con
/// `label` e `title`. Il contenuto deve essere GIÀ in `WindowContentStore`.
///
/// # Why async caller?
/// `WebviewWindowBuilder::build()` **deadlocks in synchronous Tauri commands
/// on Windows** (Tauri v2 documented limitation) — questa funzione libera non
/// è essa stessa un comando, ma va sempre chiamata da un comando `async`.
///
/// `decorations(false)` + `transparent(true)`: required on Windows to obtain
/// a chromeless, transparent frame (decorations must be off for transparency
/// to take effect on the Win32 backend).  The window provides its own
/// title bar, close button, and Esc handler via window.html/window.js.
/// NOTE: `resizable(true)` is kept but native resize handles are absent on a
/// decoration-free window; this is a known limitation flagged for the
/// supervisor — custom resize handles are out of scope for this change.
fn build_markdown_window(app: &AppHandle, label: &str, title: &str) -> Result<(), String> {
    WebviewWindowBuilder::new(app, label, WebviewUrl::App("window.html".into()))
        .title(title)
        .inner_size(800.0, 600.0)
        .decorations(false)
        .transparent(true)
        .resizable(true)
        // BUG-003 fix: match the main window's always-on-top level so the Markdown
        // window is never hidden behind the main overlay (which is alwaysOnTop: true
        // in tauri.conf.json).  focused(true) brings it to the foreground on open.
        .always_on_top(true)
        .focused(true)
        .build()
        .map(|_| ())
        .map_err(|e| format!("WebviewWindowBuilder::build() failed: {e}"))
}

/// Finestra di output di un comando slash originato da una shell (spec §3.2):
/// si apre SUBITO con un segnaposto; il contenuto arriva dopo con l'evento
/// Tauri globale `output:content` (emesso da host.js su `output_window_content`),
/// che window.js filtra per `window_id` (derivato dalla label `output-<id>`).
/// `take_window_content` resta one-shot: questa finestra è l'unica che si
/// aggiorna dopo l'apertura, e lo fa via evento, non via store.
#[tauri::command]
async fn open_output_window(
    window_id: String,
    title: String,
    app: AppHandle,
    store: State<'_, WindowContentStore>,
) -> Result<(), String> {
    let label = format!("output-{window_id}");
    if let Some(win) = app.get_webview_window(&label) {
        // Stesso id due volte (non dovrebbe succedere): riusa la finestra.
        win.set_focus().map_err(|e| format!("open_output_window set_focus error: {e}"))?;
        return Ok(());
    }
    {
        let mut map = store.0.lock().map_err(|e| format!("lock error: {e}"))?;
        map.insert(label.clone(), (title.clone(), "_in corso\u{2026}_".to_string(), "markdown".to_string(), String::new()));
    }
    build_markdown_window(&app, &label, &title)
}

/// Versione di `ui.exe` (per `UiPong`, built-in `/ping`).
#[tauri::command]
fn get_ui_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Apre una finestra di ricerca **live** (`window-search.html`).
///
/// A differenza di `open_markdown_window` (contenuto statico, letto una volta),
/// questa finestra riceve i risultati DOPO l'apertura tramite eventi Tauri
/// globali (`search:hit` / `search:done`) emessi dalla finestra principale e
/// filtrati per `sid`. Qui memorizziamo solo `(title, sid)` nello store: la
/// finestra li legge via `take_window_content` (campo `content` = `sid`,
/// `kind` = `"search"`).
///
/// `async` per la stessa ragione di `open_markdown_window` (`build()` deadlocca
/// nei comandi sincroni su Windows — limite documentato di Tauri v2).
#[tauri::command]
async fn open_search_window(
    title: String,
    sid: String,
    app: AppHandle,
    store: State<'_, WindowContentStore>,
) -> Result<(), String> {
    let label = {
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::time::{SystemTime, UNIX_EPOCH};
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        static CTR: AtomicU32 = AtomicU32::new(0);
        let ctr = CTR.fetch_add(1, Ordering::Relaxed);
        format!("search-{ts}-{ctr}")
    };

    {
        let mut map = store.0.lock().map_err(|e| format!("lock error: {e}"))?;
        // content = sid (la finestra lo usa per filtrare gli eventi); kind = "search".
        // source_file = "" — una finestra di ricerca live non è mai un documento Library.
        map.insert(
            label.clone(),
            (title.clone(), sid, "search".to_string(), String::new()),
        );
    } // lock released here

    WebviewWindowBuilder::new(&app, &label, WebviewUrl::App("window-search.html".into()))
        .title(&title)
        .inner_size(720.0, 520.0)
        .decorations(false)
        .transparent(true)
        .resizable(true)
        .always_on_top(true)
        .focused(true)
        .build()
        .map_err(|e| format!("WebviewWindowBuilder::build() failed: {e}"))?;

    Ok(())
}

/// Apre la finestra di selezione screener (canale `/markets`,
/// `ServerMsg::OpenScreenerPicker`). Mirror quasi esatto di
/// `open_search_window`: contenuto passato UNA VOLTA via
/// `WindowContentStore`/`take_window_content` (`content` = JSON degli item,
/// `source_file` = label della finestra CHIAMANTE — riuso deliberato dei 4
/// campi generici esistenti, nessuno struct nuovo). NON singleton: ogni
/// apertura è una lista fresca (evita item stale se il registry Python
/// cambia fra un'apertura e l'altra) — a differenza di
/// `open_external_channel_window`.
///
/// `webview: tauri::WebviewWindow` è la finestra CHIAMANTE (iniettata da
/// Tauri, stesso pattern di `take_window_content`/`close_self`) — la sua
/// label finisce nello slot `source_file` così `screener-picker.js` sa a
/// chi rimandare la scelta.
///
/// `async` per lo stesso motivo di `open_markdown_window`/
/// `open_search_window` (`build()` deadlocca nei comandi sincroni su
/// Windows — limite documentato di Tauri v2).
#[tauri::command]
async fn open_screener_picker_window(
    webview: tauri::WebviewWindow,
    app: AppHandle,
    items_json: String,
    store: State<'_, WindowContentStore>,
) -> Result<(), String> {
    let opener_label = webview.label().to_string();
    let label = {
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::time::{SystemTime, UNIX_EPOCH};
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        static CTR: AtomicU32 = AtomicU32::new(0);
        let ctr = CTR.fetch_add(1, Ordering::Relaxed);
        format!("screener-picker-{ts}-{ctr}")
    };

    {
        let mut map = store.0.lock().map_err(|e| format!("lock error: {e}"))?;
        // title = fisso (screener-picker.html lo hardcoda già in HTML come
        // fallback); content = JSON item; kind = "screener-picker" (non letto
        // da screener-picker.js oggi, coerenza con lo schema generico);
        // source_file = label della finestra /markets opener.
        map.insert(
            label.clone(),
            (
                "Seleziona screener".to_string(),
                items_json,
                "screener-picker".to_string(),
                opener_label,
            ),
        );
    } // lock released here

    WebviewWindowBuilder::new(&app, &label, WebviewUrl::App("screener-picker.html".into()))
        .title("Lare — Seleziona screener")
        .inner_size(420.0, 480.0)
        .decorations(false)
        .transparent(true)
        .resizable(true)
        .always_on_top(true)
        .focused(true)
        .build()
        .map_err(|e| format!("WebviewWindowBuilder::build() failed: {e}"))?;

    Ok(())
}

/// Open a generic plugin window (`plugin-window.html`).
///
/// Each plugin window has a stable label `plugin-<window_id>` derived from the
/// plugin's own unique identifier (a `u64` assigned by the orchestrator).  Using
/// the `window_id` as the label has two advantages over the `<ts>-<ctr>` scheme
/// used for search/markdown windows:
///
///  1. Idempotency: calling this command twice with the same `window_id` will
///     succeed the second time only if the first window was already closed; if
///     it is still open, `WebviewWindowBuilder::build()` will return an error
///     (duplicate label) — which surfaces the bug rather than silently creating a
///     stray duplicate.
///  2. Direct targeting: Tauri event routing and capability `windows: ["plugin-*"]`
///     both work correctly because the label prefix is stable.
///
/// The `(title, html, window_id_str, source_file)` quadruple is stored in
/// `WindowContentStore` before the window is opened — `source_file` is
/// always `""` here (sentinel): a plugin window is never a Library
/// document.  `plugin-window.js` reads it via `take_window_content` at
/// load time:
///   • `content` field → initial HTML (rendered by `sanitizeAndRender`)
///   • `kind` field    → `window_id` as decimal string → parsed as `Number`
///
/// `async` for the same reason as `open_markdown_window` and `open_search_window`:
/// `WebviewWindowBuilder::build()` deadlocks in synchronous Tauri commands on
/// Windows (Tauri v2 documented limitation).
#[tauri::command]
async fn open_plugin_window(
    window_id: u64,
    title: String,
    html: String,
    // Dimensione iniziale opzionale richiesta dal plugin (dal suo `plugin.json`).
    // Assente (None) per calc/ping/counter → si usa il default generico 480×360.
    // Nota Tauri v2: un parametro `Option<T>` non fornito da JS arriva come `None`.
    width: Option<f64>,
    height: Option<f64>,
    app: AppHandle,
    store: State<'_, WindowContentStore>,
) -> Result<(), String> {
    // Label is stable: "plugin-<window_id>".
    // This lets the capability match "plugin-*" and lets JS parse the id back
    // from the label if needed (though we use take_window_content instead).
    let label = format!("plugin-{window_id}");

    {
        let mut map = store.0.lock().map_err(|e| format!("lock error: {e}"))?;
        // content = initial HTML to render.
        // kind    = window_id as decimal string (plugin-window.js parses with Number()).
        // source_file = "" — una finestra plugin non è mai un documento Library.
        map.insert(
            label.clone(),
            (title.clone(), html, window_id.to_string(), String::new()),
        );
    } // lock released here

    WebviewWindowBuilder::new(&app, &label, WebviewUrl::App("plugin-window.html".into()))
        .title(&title)
        // Dimensione iniziale: quella dichiarata dal plugin se presente, altrimenti
        // il default generico 480×360 (adatto ai plugin compatti come la calcolatrice).
        .inner_size(width.unwrap_or(480.0), height.unwrap_or(360.0))
        .decorations(false)
        .transparent(true)
        .resizable(true)
        .always_on_top(true)
        .focused(true)
        .build()
        .map_err(|e| format!("open_plugin_window build() failed: {e}"))?;

    Ok(())
}

/// Open the routine-save preview window (`routine-preview.html`).
///
/// Companion to `open_markdown_window`/`open_search_window`: content is
/// STRUCTURED (name/description/tags/category/script/replace/id), not a
/// single string — packed as a JSON string into the existing `content` slot
/// of `WindowContentStore` (kind = `"routine_preview"`), reusing the SAME
/// one-shot `take_window_content` mechanism rather than extending its tuple
/// shape for one new window type. `routine-preview.js` does `JSON.parse` on
/// load.
///
/// The Save/Annulla buttons in this window do NOT call back into Rust: they
/// emit a Tauri global event (`"routine-preview:decision"`) that `app.js`
/// forwards over the EXISTING WebSocket connection as
/// `ClientMsg::ToolConfirmResponse{id, accept}` — the exact same message the
/// cursor's Sì/No banner already sends. No new client message type.
///
/// `async` for the same reason as every other window-opening command here:
/// `WebviewWindowBuilder::build()` deadlocks in synchronous Tauri commands on
/// Windows (Tauri v2 documented limitation).
#[tauri::command]
async fn open_routine_preview(
    id: String,
    name: String,
    description: String,
    tags: Vec<String>,
    category: String,
    script: String,
    replace: Option<String>,
    app: AppHandle,
    store: State<'_, WindowContentStore>,
) -> Result<(), String> {
    let label = {
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::time::{SystemTime, UNIX_EPOCH};
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        static CTR: AtomicU32 = AtomicU32::new(0);
        let ctr = CTR.fetch_add(1, Ordering::Relaxed);
        format!("routine-preview-{ts}-{ctr}")
    };

    let title = match &replace {
        Some(old) => format!("Aggiorna routine: {name} (sostituisce {old})"),
        None => format!("Salva routine: {name}"),
    };

    let payload = serde_json::json!({
        "id": id,
        "name": name,
        "description": description,
        "tags": tags,
        "category": category,
        "script": script,
        "replace": replace,
    })
    .to_string();

    {
        let mut map = store.0.lock().map_err(|e| format!("lock error: {e}"))?;
        map.insert(
            label.clone(),
            (
                title.clone(),
                payload,
                "routine_preview".to_string(),
                String::new(),
            ),
        );
    }

    WebviewWindowBuilder::new(&app, &label, WebviewUrl::App("routine-preview.html".into()))
        .title(&title)
        .inner_size(640.0, 560.0)
        .decorations(false)
        .transparent(true)
        .resizable(true)
        .always_on_top(true)
        .focused(true)
        .build()
        .map_err(|e| format!("open_routine_preview build() failed: {e}"))?;

    Ok(())
}

/// Retrieve and remove the (title, content, kind, source_file) quadruple for the given window label.
///
/// Called by window.js at load time.  Returns `None` if the label is not found
/// (e.g. on a page reload, the entry was already consumed).
///
/// The returned JSON is `{ "title": ..., "content": ..., "kind": ..., "source_file": ... }`.
/// `kind` is the `WindowKind` wire value (`"markdown"`, `"help"`, …).
/// `window.js` uses `kind` to apply CSS classes for special window types.
/// `source_file` is the Library rel-path this window was opened from, or ""
/// if it wasn't opened from the Library.
///
/// This is the "one-shot" read: once consumed, the entry is removed to prevent
/// the managed state from growing unbounded.
#[tauri::command]
fn take_window_content(
    webview: tauri::WebviewWindow,
    store: State<'_, WindowContentStore>,
) -> Option<serde_json::Value> {
    // Derive the label from the CALLING window — robust, no ?label= query param.
    let label = webview.label().to_string();
    let mut map = store.0.lock().ok()?;
    let (title, content, kind, source_file) = map.remove(&label)?;
    Some(
        serde_json::json!({ "title": title, "content": content, "kind": kind, "source_file": source_file }),
    )
}

/// Close the calling Markdown window programmatically.
///
/// Called by window.js when the user clicks the `×` close button or presses
/// Esc.  The calling window is identified by Tauri's `WebviewWindow` injection
/// (same pattern as `take_window_content`).
///
/// Returns `Err` only if the OS window-close call fails; this is a best-effort
/// close and the JS layer ignores the error (the window will close anyway on
/// Esc or Alt-F4 at the OS level).
#[tauri::command]
fn close_self(webview: tauri::WebviewWindow) -> Result<(), String> {
    webview
        .close()
        .map_err(|e| format!("close_self error: {e}"))
}

/// Resize the calling Markdown window to the given logical dimensions.
///
/// Called by window.js after rendering content to auto-fit the window height.
/// Width is kept as passed (typically the current window width).
/// The JS layer clamps the dimensions before calling and ignores errors.
#[tauri::command]
fn resize_self(webview: tauri::WebviewWindow, width: f64, height: f64) -> Result<(), String> {
    webview
        .set_size(tauri::LogicalSize::new(width, height))
        .map_err(|e| format!("resize_self error: {e}"))
}

// ---------------------------------------------------------------------------
// Config file path
// ---------------------------------------------------------------------------

/// Percorso del file di configurazione: `<config_dir>/config.json` (2.0: la
/// firma resta `Result<PathBuf, String>` perché molti chiamanti già usano
/// `?` su questo comando — ma il corpo ora è infallibile, delega a
/// `config_dir::config_file_path` con la cartella già risolta in `ConfigDirState`,
/// nessuna risoluzione locale/env var residua (v1 leggeva una directory dati
/// applicativa fornita dal framework, con un override a parte).
fn config_file_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(config_dir::config_file_path(
        &app.state::<ConfigDirState>().config_dir,
    ))
}

/// Percorso della cartella Library: `<config_dir>/library` — stessa cartella
/// di `config.json` (2.0: in v1 era una sottocartella della stessa directory
/// dati applicativa di sopra, con lo stesso override; ora un solo risolutore,
/// `ConfigDirState`). La cartella viene creata lazy da `archive::save`.
fn library_dir_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(config_dir::library_dir_path(
        &app.state::<ConfigDirState>().config_dir,
    ))
}

/// Compute the path to the Find archive directory: `<config_dir>/library/find`.
///
/// Subdirectory of `library_dir_path`; created eagerly in `.setup` (best-effort)
/// and also by `archive::save_find` on first save. Delega a
/// `config_dir::find_dir_path` invece di ricomporre `library_dir_path(app)?.join("find")`
/// a mano — stessa regola espressa in un solo posto.
fn library_find_dir_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(config_dir::find_dir_path(
        &app.state::<ConfigDirState>().config_dir,
    ))
}

/// `<config_dir>/library/documents` — la cartella dei markdown (nuovo layout v0.26.0).
///
/// Tutti i comandi documento (`archive_save`, `archive_list`, `archive_open`,
/// `archive_delete`, `archive_list_tree`, `archive_create_folder`,
/// `archive_rename_folder`, `archive_move_file`, `archive_delete_folder`) puntano
/// a questa directory invece che alla root `library/`.
///
/// `library_dir_path` rimane invariata: il comando `library_dir` restituisce la root
/// del repository (usato per navigare la struttura), e `library_find_dir_path`
/// la usa come base. I comandi `/find` restano su `library/find/`.
fn documents_dir_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(library_dir_path(app)?.join("documents"))
}

// ---------------------------------------------------------------------------
// Library browser window (B2)
// ---------------------------------------------------------------------------

/// Return the path to the archive library directory as a string.
///
/// Restituisce la root del repository (`library/`), NON la cartella documenti.
/// Mantenuto per compatibilità (es. navigazione struttura), ma il bottone 📂
/// nella Library ora usa `documents_dir` (coerente con la vista Explorer).
///
/// Non fallisce mai in pratica (2.0: `library_dir_path` è infallibile, la
/// firma `Result` resta per compatibilità con i chiamanti esistenti).
#[tauri::command]
fn library_dir(app: AppHandle) -> Result<String, String> {
    Ok(library_dir_path(&app)?.to_string_lossy().into_owned())
}

/// Return the path to `library/documents/` as a string.
///
/// Usato dal bottone 📂 nella Library per aprire la cartella dei documenti
/// nell'Explorer di sistema — coerente con ciò che mostra la vista ad albero.
/// Sostituisce `library_dir` per questo scopo specifico (v0.26.0).
///
/// Non fallisce mai in pratica (stesso motivo di `library_dir` sopra).
#[tauri::command]
fn documents_dir(app: AppHandle) -> Result<String, String> {
    Ok(documents_dir_path(&app)?.to_string_lossy().into_owned())
}

/// Delete an archived document by plain filename.
///
/// `file` must be a plain filename (no path separators, no `..`).
/// Returns `Err` if the filename fails the traversal check or the file
/// cannot be removed.
///
/// Called from library.js after the user confirms the two-step deletion UI.
#[tauri::command]
fn archive_delete(app: AppHandle, file: String) -> Result<(), String> {
    // v0.26.0: opera su documents/ (nuovo layout) invece della root library/.
    let library_dir = documents_dir_path(&app)?;
    archive::delete(&library_dir, &file)
}

/// Sovrascrive un documento Library ESISTENTE con nuovo contenuto, stesso
/// filename. Usato dalla feature "espandi documento" (window.js): dopo che
/// l'AI del canale `library-expand` ha prodotto il testo fuso, la finestra
/// chiama questo comando per scrivere il risultato su disco.
///
/// `file` è il rel-path del documento (può contenere sottocartelle), noto
/// alla finestra tramite `source_file` (vedi Task 2).
#[tauri::command]
fn archive_update(
    app: AppHandle,
    file: String,
    title: String,
    content: String,
) -> Result<(), String> {
    let library_dir = documents_dir_path(&app)?;
    archive::update(&library_dir, &file, &title, &content)
}

// ---------------------------------------------------------------------------
// Archive folder commands (Slice 1 — Library a cartelle, backend)
// ---------------------------------------------------------------------------

/// Restituisce l'albero completo della Library come `LibraryTree`.
///
/// Wrapper Tauri di `archive::list_tree`. Il frontend chiama questo comando
/// per ottenere la struttura ad albero (cartelle + file) da mostrare nella Library.
#[tauri::command]
fn archive_list_tree(app: AppHandle) -> Result<archive::LibraryTree, String> {
    // v0.26.0: la vista Explorer è radicata su documents/ (non sulla root library/).
    let library_dir = documents_dir_path(&app)?;
    Ok(archive::list_tree(&library_dir))
}

/// Crea una nuova cartella nella Library.
///
/// `parent_rel`: path relativo della cartella padre ("" = root Library).
/// `name`: nome semplice della cartella (no path separators, no "..").
/// Restituisce il path relativo della cartella creata (con eventuale suffisso numerico).
#[tauri::command]
fn archive_create_folder(
    app: AppHandle,
    parent_rel: String,
    name: String,
) -> Result<String, String> {
    // v0.26.0: le cartelle utente vivono in documents/ (non nella root library/).
    let library_dir = documents_dir_path(&app)?;
    archive::create_folder(&library_dir, &parent_rel, &name)
}

/// Rinomina una cartella nella Library.
///
/// `folder_rel`: path relativo della cartella da rinominare (es. "Progetti").
/// `new_name`: nuovo nome semplice.
#[tauri::command]
fn archive_rename_folder(
    app: AppHandle,
    folder_rel: String,
    new_name: String,
) -> Result<(), String> {
    // v0.26.0: opera su documents/ (nuovo layout).
    let library_dir = documents_dir_path(&app)?;
    archive::rename_folder(&library_dir, &folder_rel, &new_name)
}

/// Sposta un file `.md` da una posizione a un'altra nella Library.
///
/// `file_rel`: path relativo del file sorgente (es. "Progetti/doc.md").
/// `target_folder_rel`: cartella destinazione ("" = root Library).
#[tauri::command]
fn archive_move_file(
    app: AppHandle,
    file_rel: String,
    target_folder_rel: String,
) -> Result<(), String> {
    // v0.26.0: opera su documents/ (nuovo layout).
    let library_dir = documents_dir_path(&app)?;
    archive::move_file(&library_dir, &file_rel, &target_folder_rel)
}

/// Elimina una cartella SOLO se vuota.
///
/// `folder_rel`: path relativo della cartella da eliminare (es. "Progetti").
/// Restituisce errore se la cartella non è vuota, non esiste, o il path è un traversal.
#[tauri::command]
fn archive_delete_folder(app: AppHandle, folder_rel: String) -> Result<(), String> {
    // v0.26.0: opera su documents/ (nuovo layout).
    let library_dir = documents_dir_path(&app)?;
    archive::delete_folder(&library_dir, &folder_rel)
}

/// Sposta una cartella dentro un'altra cartella (o nella radice).
///
/// `folder_rel`: path relativo della cartella da spostare (es. "Vecchi").
/// `target_parent_rel`: path relativo della cartella padre di destinazione
///   ("" = radice documents).
/// Restituisce errore in caso di traversal, cartella inesistente, ciclo o collisione.
#[tauri::command]
fn archive_move_folder(
    app: AppHandle,
    folder_rel: String,
    target_parent_rel: String,
) -> Result<(), String> {
    // v0.27.0: opera su documents/ (stesso layout di tutti gli altri comandi archivio).
    let library_dir = documents_dir_path(&app)?;
    archive::move_folder(&library_dir, &folder_rel, &target_parent_rel)
}

/// Open (or bring to focus) the archive browser window.
///
/// The browser window has the fixed label `"library"` — singleton: if a window
/// with that label already exists, we simply focus it instead of opening a
/// duplicate.  The window loads `library.html`, which calls `archive_list` on
/// its own and renders the interactive list.
///
/// # Why async?
/// Same reason as `open_markdown_window`: `WebviewWindowBuilder::build()`
/// deadlocks in synchronous Tauri commands on Windows (Tauri v2 limitation).
#[tauri::command]
async fn open_library_window(app: AppHandle) -> Result<(), String> {
    // Singleton guard: if the window already exists, focus it and return.
    if let Some(win) = app.get_webview_window("library") {
        win.set_focus()
            .map_err(|e| format!("open_library_window set_focus error: {e}"))?;
        return Ok(());
    }

    // Build the library browser window — same transparent/chromeless style as
    // Markdown output windows, with a fixed label so the singleton check works.
    WebviewWindowBuilder::new(&app, "library", WebviewUrl::App("library.html".into()))
        .title("Lare — Archivio")
        .inner_size(460.0, 560.0)
        .decorations(false)
        .transparent(true)
        .resizable(true)
        .always_on_top(true)
        .focused(true)
        .build()
        .map_err(|e| format!("open_library_window build() failed: {e}"))?;

    Ok(())
}

/// Apre la finestra di configurazione (`/config`) come finestra a sé (label "config",
/// singleton). Stile chromeless/trasparente delle altre finestre. La finestra
/// carica `config.html`, che legge la config via `get_config` e salva via `set_config`,
/// poi emette `config:saved` per far riapplicare l'aspetto al pannello principale.
///
/// # Singleton
/// Se la finestra è già aperta, viene semplicemente portata in primo piano.
///
/// # Why async?
/// `WebviewWindowBuilder::build()` **deadlocks in synchronous Tauri commands
/// on Windows** (Tauri v2 documented limitation). `async` routes it through
/// Tokio and avoids the deadlock — same pattern di `open_library_window`.
#[tauri::command]
async fn open_config_window(app: AppHandle) -> Result<(), String> {
    // Singleton: se già aperta, portala in primo piano.
    if let Some(win) = app.get_webview_window("config") {
        win.set_focus()
            .map_err(|e| format!("open_config_window set_focus error: {e}"))?;
        return Ok(());
    }
    WebviewWindowBuilder::new(&app, "config", WebviewUrl::App("config.html".into()))
        .title("Lare — Configurazione")
        .inner_size(480.0, 600.0)
        .decorations(false)
        .transparent(true)
        .resizable(true)
        .always_on_top(true)
        .focused(true)
        .build()
        .map_err(|e| format!("open_config_window build() failed: {e}"))?;
    Ok(())
}

/// Apre (o porta in primo piano) la finestra generica "canale tool esterno"
/// (es. `/nmap`, quando un canale reale esisterà — vedi
/// `Docs/superpowers/specs/2026-07-16-external-tool-channel-design.md`).
///
/// A differenza di TUTTE le altre finestre secondarie (config/library/
/// plugin-*/aichat), questa apre una propria connessione WS (`Hello{channel}`)
/// invece di parlare col backend solo via IPC/eventi Tauri relayati da
/// `app.js` — vedi `external-channel-window.js`.
///
/// # Singleton per canale
/// Due canali diversi (es. `nmap` e un futuro secondo tool) possono avere
/// finestre aperte insieme; lo stesso canale invocato due volte si limita a
/// focalizzare la finestra già aperta (stesso pattern di `open_config_window`).
#[tauri::command]
async fn open_external_channel_window(
    app: AppHandle,
    channel_id: String,
    window_title: String,
) -> Result<(), String> {
    // Singleton PER CANALE: due canali diversi possono avere finestre
    // aperte insieme; lo stesso canale due volte si limita a focalizzare.
    let label = format!("extchannel-{channel_id}");
    if let Some(win) = app.get_webview_window(&label) {
        win.set_focus()
            .map_err(|e| format!("open_external_channel_window set_focus error: {e}"))?;
        return Ok(());
    }
    let channel_id_json = serde_json::to_string(&channel_id)
        .map_err(|e| format!("open_external_channel_window channel_id encode error: {e}"))?;
    // `decorations(false)` — niente titlebar nativa del SO: l'unico titolo che
    // l'utente vede davvero è lo span `#titlebar-label` dentro la pagina, che
    // external-channel.html porta hardcoded a "Canale esterno". `.title()` sotto
    // imposta comunque il titolo nativo (visibile in Alt-Tab/taskbar) ma NON
    // quello in pagina — serve iniettare anche `window_title` come per
    // `channel_id`, così `external-channel-window.js` può popolare lo span al bootstrap.
    let window_title_json = serde_json::to_string(&window_title)
        .map_err(|e| format!("open_external_channel_window window_title encode error: {e}"))?;
    WebviewWindowBuilder::new(&app, &label, WebviewUrl::App("external-channel.html".into()))
        .title(&window_title)
        .inner_size(480.0, 600.0)
        .decorations(false)
        .transparent(true)
        .resizable(true)
        .always_on_top(true)
        .focused(true)
        .initialization_script(format!(
            "window.__LARE_EXT_CHANNEL_ID__ = {channel_id_json}; window.__LARE_EXT_CHANNEL_TITLE__ = {window_title_json};"
        ))
        .build()
        .map_err(|e| format!("open_external_channel_window build() failed: {e}"))?;
    Ok(())
}

/// Apre (o porta in primo piano) la finestra-chat AI Chat (Slice 1a-ui-B).
/// Singleton come `open_config_window`. Contenuto live via eventi Tauri da app.js
/// (la finestra NON apre una connessione WS propria).
#[tauri::command]
async fn open_aichat_window(app: AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("aichat") {
        win.set_focus()
            .map_err(|e| format!("open_aichat_window set_focus error: {e}"))?;
        return Ok(());
    }
    WebviewWindowBuilder::new(&app, "aichat", WebviewUrl::App("aichat-window.html".into()))
        .title("Lare — AI Chat")
        .inner_size(420.0, 560.0)
        .decorations(false)
        .transparent(true)
        .resizable(true)
        .always_on_top(true)
        .focused(true)
        .build()
        .map_err(|e| format!("open_aichat_window build() failed: {e}"))?;
    Ok(())
}

/// Apre (o porta in primo piano) la finestra "Nuova nota"/"Modifica nota" del
/// Blocco note. Singleton come `open_aichat_window`, label fissa `"note-compose"`
/// (niente editing affiancato di due note — nessuno l'ha mai chiesto).
///
/// Sostituisce il vecchio `<dialog>` HTML dentro `library.html` (Task 19):
/// primo smoke test dal vivo multi-macchina ha trovato che, essendo una
/// finestra CHILD del webview Library, non era né spostabile fuori dai
/// confini della finestra Library né ridimensionabile se non trascinando
/// l'angolo della sola textarea. Una vera `WebviewWindow` risolve entrambi
/// per costruzione (resizable dai bordi nativi del SO, spostabile ovunque).
///
/// `note_id: None` → "Nuova nota" (campi vuoti); `Some(id)` → "Modifica nota"
/// precompilata con `title`/`text` (il segmento di QUESTA sola macchina,
/// `NoteView::my_segment_text` — mai il corpo fuso `body`).
///
/// Il contenuto iniziale passa da `WindowContentStore` (stesso schema a 4
/// campi di `open_plugin_window`: qui `kind` porta l'id nota, o `""` per una
/// nota nuova) — letto una tantum da `take_window_content` al primo avvio
/// del webview, nessuna race con la registrazione dei listener JS.
///
/// Se la finestra esiste già (l'utente riapre "Modifica" su un'altra nota
/// mentre la finestra nota è ancora aperta), `take_window_content` non
/// verrebbe più letto (già consumato al primo avvio) — il contenuto fresco
/// arriva quindi via un evento Tauri diretto (`note-window:load`), sicuro
/// perché a quel punto il JS della finestra è di sicuro già in ascolto.
#[tauri::command]
async fn open_note_window(
    note_id: Option<String>,
    title: String,
    text: String,
    app: AppHandle,
    store: State<'_, WindowContentStore>,
) -> Result<(), String> {
    let kind = note_id.unwrap_or_default();
    {
        let mut map = store.0.lock().map_err(|e| format!("lock error: {e}"))?;
        map.insert(
            "note-compose".to_string(),
            (title.clone(), text.clone(), kind.clone(), String::new()),
        );
    } // lock released here

    if let Some(win) = app.get_webview_window("note-compose") {
        win.set_focus()
            .map_err(|e| format!("open_note_window set_focus error: {e}"))?;
        win.emit(
            "note-window:load",
            serde_json::json!({ "noteId": kind, "title": title, "text": text }),
        )
        .map_err(|e| format!("open_note_window emit error: {e}"))?;
        return Ok(());
    }

    WebviewWindowBuilder::new(
        &app,
        "note-compose",
        WebviewUrl::App("note-window.html".into()),
    )
    .title("Lare — Nota")
    .inner_size(440.0, 400.0)
    .decorations(false)
    .transparent(true)
    .resizable(true)
    .always_on_top(true)
    .focused(true)
    .build()
    .map_err(|e| format!("open_note_window build() failed: {e}"))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Archive commands (B1: save + list + open)
// ---------------------------------------------------------------------------

/// Save the content of a Markdown window to the archive.
///
/// Writes `{library_dir}/{slug(title)}-{timestamp_ms}.md`.
/// The timestamp suffix provides a unique, sortable filename.
///
/// Returns the plain filename created (e.g. `"my-window-1718000000000.md"`).
#[tauri::command]
fn archive_save(app: AppHandle, title: String, content: String) -> Result<String, String> {
    // v0.26.0: salva in documents/ (nuovo layout) invece della root library/.
    let library_dir = documents_dir_path(&app)?;

    // Genera un suffisso timestamp per unicità del filename.
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis().to_string())
        .unwrap_or_else(|_| "0".to_string());

    archive::save(&library_dir, &title, &content, &suffix)
}

/// List all saved windows in the archive, sorted by modification time (newest first).
///
/// Returns an empty array if the library directory does not exist (no saves yet).
///
/// NB: attualmente non invocato direttamente dal frontend (library.js usa
/// `archive_list_tree` per la vista ad albero). Switchato comunque su documents/
/// per coerenza: nessun comando documento deve puntare alla root library/.
#[tauri::command]
fn archive_list(app: AppHandle) -> Vec<archive::ArchiveEntry> {
    // v0.26.0: opera su documents/ (nuovo layout).
    match documents_dir_path(&app) {
        Ok(dir) => archive::list(&dir),
        Err(e) => {
            eprintln!("[archive] archive_list: cannot resolve documents dir: {e}");
            vec![]
        }
    }
}

/// Open an archived window by filename and return its content.
///
/// `file` must be a plain filename (no path separators, no `..`).
/// Returns `Err` if the filename fails the traversal check or the file cannot be read.
#[tauri::command]
fn archive_open(app: AppHandle, file: String) -> Result<archive::ArchiveDoc, String> {
    // v0.26.0: apre da documents/ (nuovo layout).
    let library_dir = documents_dir_path(&app)?;
    archive::open(&library_dir, &file)
}

// ---------------------------------------------------------------------------
// Find commands (saved Find sessions)
// ---------------------------------------------------------------------------

/// Save a Find session (query + hits) to `library/find/`.
///
/// Delegates to `archive::save_find`; the directory is `library_find_dir_path`.
/// Returns the plain filename created (e.g. `"find--pdf-1718000000000.json"`).
#[tauri::command]
fn save_find(
    app: AppHandle,
    query: String,
    hits: Vec<archive::ArchiveHit>,
) -> Result<String, String> {
    let dir = library_find_dir_path(&app)?;
    archive::save_find(&dir, &query, &hits)
}

/// List all saved Find sessions in `library/find/`, newest first.
///
/// Returns an empty array if the directory does not exist (no saves yet).
#[tauri::command]
fn list_find(app: AppHandle) -> Result<Vec<archive::FindEntry>, String> {
    match library_find_dir_path(&app) {
        Ok(dir) => Ok(archive::list_find(&dir)),
        Err(e) => {
            eprintln!("[archive] list_find: cannot resolve find dir: {e}");
            Ok(vec![])
        }
    }
}

/// Open a saved Find session by plain filename and return its `ArchiveFind`.
///
/// `file` must be a plain filename (no path separators, no `..`).
/// Returns `Err` if the filename fails the traversal check or the file cannot be read/parsed.
#[tauri::command]
fn open_find(app: AppHandle, file: String) -> Result<archive::ArchiveFind, String> {
    let dir = library_find_dir_path(&app)?;
    archive::open_find(&dir, &file)
}

/// Delete a saved Find session by plain filename.
///
/// `file` must be a plain filename (no path separators, no `..`).
/// Returns `Err` if the filename fails the traversal check or the file cannot be removed.
#[tauri::command]
fn delete_find(app: AppHandle, file: String) -> Result<(), String> {
    let dir = library_find_dir_path(&app)?;
    archive::delete_find(&dir, &file)
}

/// Open a `window-search.html` window that replays a saved Find session.
///
/// Strategy (same as `open_search_window` for live searches):
///   1. Generate a unique label `search-<unix_ms>-<counter>`.
///   2. Read the saved `ArchiveFind` via `archive::open_find`.
///   3. Serialize the `ArchiveFind` to JSON; prefix with `"saved:"` so
///      `window-search.js` can distinguish replay from live mode.
///   4. Insert `(title, "saved:<json>", "search", "")` into `WindowContentStore`
///      BEFORE creating the window — `source_file` is the `""` sentinel: a
///      saved-search replay window is never a Library document.
///   5. Create the `WebviewWindow` loading `window-search.html`.
///
/// `async` for the same reason as `open_markdown_window` and `open_search_window`:
/// `WebviewWindowBuilder::build()` deadlocks in synchronous commands on Windows
/// (Tauri v2 documented limitation).
#[tauri::command]
async fn open_saved_find_window(
    app: AppHandle,
    file: String,
    store: State<'_, WindowContentStore>,
) -> Result<(), String> {
    // Step 1: read the saved session.
    let dir = library_find_dir_path(&app)?;
    let record = archive::open_find(&dir, &file)?;

    // Step 2: serialize as JSON and build the "saved:" payload.
    let json =
        serde_json::to_string(&record).map_err(|e| format!("cannot serialize ArchiveFind: {e}"))?;
    let content = format!("saved:{json}");

    // Step 3: generate a unique window label.
    let label = {
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::time::{SystemTime, UNIX_EPOCH};
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        static CTR: AtomicU32 = AtomicU32::new(0);
        let ctr = CTR.fetch_add(1, Ordering::Relaxed);
        format!("search-{ts}-{ctr}")
    };

    // Step 4: store content BEFORE building the window (same pattern as open_search_window).
    let title = format!("Find: {}", record.query);
    {
        let mut map = store.0.lock().map_err(|e| format!("lock error: {e}"))?;
        // source_file = "" — una finestra di ricerca salvata non è un documento Library.
        map.insert(
            label.clone(),
            (title.clone(), content, "search".to_string(), String::new()),
        );
    } // lock released here

    // Step 5: create the WebviewWindow — same flags as open_search_window.
    WebviewWindowBuilder::new(&app, &label, WebviewUrl::App("window-search.html".into()))
        .title(&title)
        .inner_size(720.0, 520.0)
        .decorations(false)
        .transparent(true)
        .resizable(true)
        .always_on_top(true)
        .focused(true)
        .build()
        .map_err(|e| format!("WebviewWindowBuilder::build() failed: {e}"))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    // ── Risoluzione della cartella di configurazione (2.0, decisione D6):
    //    `--config-dir` dagli argv reali, altrimenti `<exe_dir>/Configuration`
    //    — PRIMA di costruire il builder, così è disponibile sia per
    //    `.manage()` sotto sia per il log di avvio. `warn` è `Some` solo se
    //    `startup.json` esiste ma è illeggibile/malformato (file assente →
    //    default silenzioso, non è un errore).
    let (cfg_state, warn) = config_dir::ConfigDirState::from_process();
    if let Some(w) = warn {
        eprintln!("[ui] {w}");
    }
    println!("[ui] config dir: {}", cfg_state.config_dir.display());

    // ── Flag `--no-terminal` (piano 3): usato da chi avvia `ui.exe` in
    //    ruolo "solo host" — la host C# in modalità B (`Launcher.EnsureUi`,
    //    lare-shell 2.0.1) e l'autostart dell'orchestratore (spec §6.4)
    //    lo passano SEMPRE, altrimenti ogni `/comando` aprirebbe una
    //    seconda finestra terminale non voluta. Assente = modalità A: la
    //    finestra terminale nasce all'avvio (comportamento di default).
    let args: Vec<String> = std::env::args().collect();
    let no_terminal = args.iter().any(|a| a == "--no-terminal");

    // ── Self-heal (spec §6.4): se l'orchestratore non risponde e
    //    l'autostart è attivo, avvialo PRIMA di costruire la finestra
    //    terminale (Task 4) — così `lare-shell.exe`, quando la pty lo
    //    lancia, trova già il WS su.
    let orchestrator_exe =
        startup_config::deploy_root(&cfg_state.config_dir).join("orchestrator.exe");
    let ws_port = cfg_state.startup.ws_port;
    let connect = || -> Option<String> {
        let addr = format!("127.0.0.1:{ws_port}");
        match std::net::TcpStream::connect_timeout(
            &addr.parse().expect("host:port letterale sempre valido"),
            std::time::Duration::from_millis(400),
        ) {
            Ok(_) => None,
            Err(e) => Some(e.to_string()),
        }
    };
    launcher::ensure_orchestrator(
        &connect,
        cfg_state.startup.autostart.orchestrator,
        &orchestrator_exe,
        &cfg_state.config_dir,
        launcher::CONNECT_WINDOW,
        launcher::RETRY_INTERVAL,
    );

    tauri::Builder::default()
        // Stato gestito: cartella di configurazione + startup.json, risolti
        // una volta sopra — nessun comando/modulo li ri-deriva da solo.
        .manage(cfg_state)
        // Stato gestito: sessione pty condivisa (Task 2, piano 3) — una sola
        // sessione "una finestra, una sessione" dell'MVP, vedi pty::shared_state().
        .manage(PtyState(ui_lib::pty::shared_state()))
        // Stato gestito: ruolo "solo host" (`--no-terminal`, piano 3) — letto
        // dalla finestra terminale (Task 4) per decidere se aprirsi.
        .manage(NoTerminal(no_terminal))
        // Register Tauri commands callable from JS.
        .invoke_handler(tauri::generate_handler![
            get_lare_token,
            get_ws_endpoint,
            diagnose_connection,
            pty_spawn,
            pty_write,
            pty_resize,
            get_config,
            set_config,
            search_settings::get_search_settings,
            search_settings::set_search_settings,
            aichat_settings::get_aichat_settings,
            aichat_settings::set_aichat_settings,
            llm_settings::get_llm_settings,
            llm_settings::set_llm_settings,
            market_data_settings::get_market_data_settings,
            market_data_settings::set_market_data_settings,
            plugins_view::list_plugins,
            open_markdown_window,
            open_output_window,
            get_ui_version,
            open_search_window,
            open_screener_picker_window,
            open_plugin_window,
            open_routine_preview,
            take_window_content,
            close_self,
            resize_self,
            archive_save,
            archive_list,
            archive_open,
            archive_delete,
            archive_update,
            archive_list_tree,
            archive_create_folder,
            archive_rename_folder,
            archive_move_file,
            archive_move_folder,
            archive_delete_folder,
            open_library_window,
            open_config_window,
            open_aichat_window,
            open_note_window,
            open_external_channel_window,
            library_dir,
            documents_dir,
            save_find,
            list_find,
            open_find,
            delete_find,
            open_saved_find_window,
        ])
        .setup(|app| {
            // ── Diagnostica (2.0): un'unica cartella di configurazione, non più
            //    due (Local/Roaming) come in v1 — già stampata in main() da
            //    `ConfigDirState::from_process()` prima che il builder partisse.
            //    `config.json` e `library/` vivono entrambi lì (vedi
            //    `config_file_path`/`library_dir_path` sotto): un secondo print
            //    qui ripeterebbe la stessa cartella con un'etichetta diversa.

            // ── Load config from disk (or defaults on first run). ──────────
            let config_path = config_file_path(app.handle()).unwrap_or_else(|e| {
                eprintln!("[ui] config path error: {e} — using defaults");
                // A non-persistent fallback path (writes will fail silently).
                std::path::PathBuf::from("lare-terminal-config.json")
            });

            let cfg = config::load_from(&config_path);
            println!(
                "[ui] Loaded config: web_search_enabled={:?} window_alpha={:?}",
                cfg.web_search_enabled, cfg.window_alpha
            );

            // ── Store config in managed state. ─────────────────────────────
            app.manage(ConfigState(Mutex::new(cfg)));

            // ── Initialize Markdown window content store (ADR-013). ────────
            app.manage(WindowContentStore(Mutex::new(HashMap::new())));

            // ── Ensure the archive (library) directory exists, so the Library
            //    "Apri" button always has a folder to open — even before the
            //    first save (the dir is otherwise created lazily on save).
            //    The UI owns this path (under the resolved config dir), so it
            //    is the right side to create it; best-effort, never fatal.
            match library_dir_path(app.handle()) {
                Ok(dir) => {
                    if let Err(e) = std::fs::create_dir_all(&dir) {
                        eprintln!("[ui] cannot create library dir {dir:?}: {e}");
                    }
                    // ── Migra il vecchio layout (.md sciolti in library/) al nuovo
                    //    (library/documents/). v0.26.0.
                    //    Idempotente: alla seconda esecuzione (root ha solo find/ e
                    //    documents/) → no-op. Best-effort: un errore non blocca l'avvio.
                    //    Ordine: PRIMA della creazione di find/ e del watch (documents/
                    //    deve esistere prima che il watcher vi si attacchi).
                    if let Err(e) = archive::migrate_to_documents_layout(&dir) {
                        eprintln!("[ui] migrate_to_documents_layout: {e} (best-effort)");
                    }
                }
                Err(e) => eprintln!("[ui] library dir path error: {e}"),
            }

            // ── Ensure the Find archive subdirectory exists (library/find/).
            //    Best-effort: also created lazily by save_find on first save,
            //    but eager creation lets the Library tab list it immediately.
            match library_find_dir_path(app.handle()) {
                Ok(dir) => {
                    if let Err(e) = std::fs::create_dir_all(&dir) {
                        eprintln!("[ui] cannot create library/find dir {dir:?}: {e}");
                    }
                }
                Err(e) => eprintln!("[ui] library/find dir path error: {e}"),
            }

            // ── Avvia il watcher fs sulla cartella documents/ (v0.26.0).
            //    Quando l'utente modifica documents/ da fuori (Explorer, CLI),
            //    emettiamo l'evento "library:changed" alla finestra Library
            //    (se è aperta) affinché ricarichi la vista automaticamente.
            //
            //    Nota v0.26.0: il watcher ora osserva `documents/` (non la root
            //    `library/`): la vista Explorer è radicata su documents/; modifiche
            //    a library/find/ non triggerano più reload inutili nel tab Markdown.
            //
            //    Debounce 400 ms: raggruppa le modifiche ravvicinate (es. copia
            //    di molti file) in un unico reload, evitando reload ridondanti.
            //
            //    Il guard viene conservato in managed state: se fosse droppato
            //    qui il watcher si fermerebbe subito (il Debouncer ferma il
            //    proprio thread interno quando viene droppato).
            //
            //    `migrate_to_documents_layout` (chiamata sopra) garantisce che
            //    documents/ esiste già a questo punto — even su fresh-install.
            match documents_dir_path(app.handle()) {
                Ok(docs_dir) => {
                    let handle = app.handle().clone();
                    match library_watch::watch_dir(
                        &docs_dir,
                        Duration::from_millis(400),
                        move || {
                            // Emetti solo se la finestra Library è aperta.
                            // get_webview_window ritorna None se la finestra
                            // non esiste (non è mai stata aperta, o è stata
                            // chiusa): in quel caso il reload è inutile.
                            if let Some(win) = handle.get_webview_window("library") {
                                let _ = win.emit("library:changed", ());
                            }
                        },
                    ) {
                        Ok(guard) => {
                            app.manage(LibraryWatchGuard(Mutex::new(guard)));
                            println!("[ui] Library fs-watch avviato su {docs_dir:?} (documents/)");
                        }
                        Err(e) => {
                            // Best-effort: il watch non è critico per l'app.
                            // L'utente può sempre ricaricare manualmente con 🔄.
                            eprintln!("[ui] library watch non avviato: {e}");
                        }
                    }
                }
                Err(e) => eprintln!("[ui] library watch: path error: {e}"),
            }

            // ── Uscita pulita interattiva ("Q" + invio) ─────────────────────
            // Solo in debug (dev): in release la console è staccata (vedi
            // `windows_subsystem = "windows"` in cima al file), quindi non c'è
            // stdin da leggere — il thread sarebbe inerte, non lo spawniamo.
            // `cleanup_before_exit()` + `std::process::exit` (non più API Tauri
            // dopo, per documentazione ufficiale) lascia a WebView2 il tempo di
            // chiudere le sue finestre interne PRIMA che il processo termini —
            // a differenza di Ctrl+C (interruzione brusca) o del solo `app.exit()`
            // (verificato dal vivo: entrambi possono comunque lasciare il log
            // "Failed to unregister class Chrome_WidgetWin_0" — innocuo, quirk
            // noto di Chromium/WebView2 in chiusura, non un bug di Lare Terminal).
            #[cfg(debug_assertions)]
            {
                let app_handle = app.handle().clone();
                std::thread::spawn(move || {
                    use std::io::BufRead;
                    println!("[ui] Digita 'q' e invio per uscire pulito.");
                    let stdin = std::io::stdin();
                    for line in stdin.lock().lines() {
                        let Ok(line) = line else { break }; // stdin chiuso (EOF)
                        if is_quit_command(&line) {
                            println!("[ui] uscita pulita richiesta...");
                            app_handle.cleanup_before_exit();
                            std::process::exit(0);
                        }
                    }
                });
            }

            // Niente più "Press <tasto> to toggle": l'overlay F2 è sparito, la
            // finestra host è nascosta per tutta la vita del processo.
            println!("[ui] Lare Terminal v{} started.", env!("CARGO_PKG_VERSION"));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("[ui] Tauri runtime error");
}

#[cfg(test)]
mod quit_command_tests {
    use super::*;

    #[test]
    fn is_quit_command_recognizes_q_and_quit_case_insensitive() {
        assert!(is_quit_command("q"));
        assert!(is_quit_command("Q"));
        assert!(is_quit_command("quit"));
        assert!(is_quit_command("QUIT"));
        assert!(
            is_quit_command("  q  "),
            "gli spazi ai bordi vanno ignorati"
        );
    }

    #[test]
    fn is_quit_command_rejects_everything_else() {
        assert!(!is_quit_command(""));
        assert!(!is_quit_command("quitter"));
        assert!(!is_quit_command("dir"));
    }
}
