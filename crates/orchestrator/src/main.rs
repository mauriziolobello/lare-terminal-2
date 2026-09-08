//! # orchestrator — entry point
//!
//! Lare Terminal daemon.  Wires the token, AI adapter, and tool client
//! together, then starts the WebSocket server.
//!
//! ## Startup sequence (2.0, Task 4 — vedi `RuntimeConfig` sotto)
//! 1. Risolvi `config_dir` (`--config-dir` o `<exe_dir>/Configuration`) +
//!    `startup.json` → `RuntimeConfig`, il "context object" condiviso da
//!    tutto il resto di questo file (nessuna variabile d'ambiente, D6).
//! 2. Inizializza il tracing: sempre su file (`Configuration/logs/`),
//!    anche su stderr con `--console-log`.
//! 3. Resolve the auth token from `<config_dir>/token`, or generate a
//!    random one and persist it.
//! 4. Construct the AI adapter (Fase 1: StubAdapter or LlmAdapter+ClaudeBackend).
//! 5. Construct the tool client (McpToolClient, resolving the mcp-server path
//!    da `startup.json`, passato al figlio come `--config-dir`).
//! 6. Optionally start the Telegram channel (see telegramsettings.json below).
//! 7. Start the WebSocket server on `127.0.0.1:<startup.json.ws_port>` (blocks
//!    until shutdown).
//!
//! ## Telegram channel (ADR-007, v0.11.0)
//! Attivato se `<config_dir>/telegramsettings.json` è presente.
//! Al primo avvio: stampa l'URI TOTP su stderr (aggiungi a Google Authenticator).
//! Ad ogni avvio senza chat appaiata: stampa un codice `/pair` (valido 10 min).
//! Lo stato (secret + chat_id) è in `<config_dir>/telegram-state.json`.
//!
//! ## Security (ADR-007)
//! - WS binds to `127.0.0.1` only.
//! - Token: never hard-coded, mai in una env var (D6). Vive in
//!   `<config_dir>/token`; generato al primo avvio se assente.
//! - Telegram token: never in any log output.

use std::sync::Arc;

use anyhow::Result;
use orchestrator::{
    aichat::discovery::Discoverer as _, // importa il trait per poter chiamare announce()/next() su UdpDiscoverer
    ai_adapter::{AiAdapter, LlmAdapter, StubAdapter},
    claude_backend::ClaudeBackend,
    cwd_tracking::CwdTrackingToolClient,
    llms_config,
    messages_client::HttpMessagesClient,
    runtime_config::RuntimeConfig,
    search::{
        content::ContentConfig,
        paths_config::{OsPathProvider, PathsConfig},
        SearchContext, SearchEngine,
    },
    telegram::{self, auth::Authenticator, client::HttpTelegramClient, qr::render_terminal_qr},
    token_store,
    tool_client::McpToolClient,
    ws,
};
use tokio::sync::Mutex;

/// Determina l'IP LAN locale tramite il "UDP connect trick":
/// apre un socket UDP effimero e lo "connette" virtualmente a un IP pubblico (senza inviare nulla).
/// Il sistema operativo sceglie l'interfaccia di uscita e popola `local_addr()` con l'IP LAN.
/// Nessun pacchetto viene mai inviato: UDP non ha handshake; `connect` aggiorna solo il routing
/// interno del kernel. Questa tecnica funziona su Windows, macOS e Linux senza enumerare le NIC.
/// Fallisce (ritorna `None`) se la macchina non ha interfacce di rete configurate.
fn detect_local_ipv4() -> Option<std::net::Ipv4Addr> {
    // UdpSocket standard (sync) — bastano pochi µs, niente async.
    let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    // `connect` non invia nulla: informa solo il kernel sull'interfaccia da usare.
    s.connect("8.8.8.8:80").ok()?;
    match s.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(ip) => Some(ip),
        // IPv6: la scoperta UDP broadcast usa solo IPv4 — fallback al loop principale.
        _ => None,
    }
}

/// Riconosce il comando di uscita pulita digitato sullo stdin interattivo ("Q"/"quit",
/// case-insensitive, spazi ai bordi ignorati). Vedi il task stdin-reader in `main()`:
/// senza un modo esplicito di chiedere l'uscita, l'unica opzione era Ctrl+C (interruzione
/// brusca — niente chiusura ordinata di listener/plugin). Quando i processi gireranno
/// come servizi, l'uscita pulita sarà innescata dallo stop del servizio, non da stdin.
fn is_quit_command(line: &str) -> bool {
    matches!(line.trim().to_lowercase().as_str(), "q" | "quit")
}

/// Comportamento AI pre-Slice-4, invariato: chiave Anthropic presente → Claude
/// diretto; altrimenti `StubAdapter`. Usato come fallback quando `llms/llms.json`
/// è assente o non valido — retrocompatibilità DURA (spec Slice 4 §6).
///
/// `model` viene da `rt.startup.ai_model` (2.0, D6): niente più
/// `LARE_AI_MODEL` letta qui — `startup.json` è l'unica fonte per
/// impostazioni non segrete. `ANTHROPIC_API_KEY` invece RESTA una variabile
/// d'ambiente: è la chiave del provider AI, fuori scope D6 esattamente come
/// gli `api_keys` di `llms.json` (mai scritta in un file di config
/// committabile/sincronizzabile — stessa cautela di `llms_config.rs`).
fn default_ai_adapter(model: &str, config_dir: &std::path::Path) -> Arc<dyn AiAdapter> {
    match std::env::var("ANTHROPIC_API_KEY") {
        Ok(key) if !key.is_empty() => {
            tracing::info!("AI provider: Claude (modello: {model})");
            let http = Arc::new(HttpMessagesClient::new(key));
            let backend = Arc::new(ClaudeBackend::new(http, model.to_string(), 16000));
            Arc::new(LlmAdapter::new(backend, model.to_string(), config_dir.to_path_buf()))
        }
        _ => {
            tracing::warn!("ANTHROPIC_API_KEY non impostata — uso StubAdapter");
            Arc::new(StubAdapter)
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Forza subito PROCESS_START (LazyLock): l'uptime di `/ping` deve misurare
    // l'avvio reale del processo, non l'istante del primo ping ricevuto.
    std::sync::LazyLock::force(&orchestrator::PROCESS_START);

    // ── Configurazione (2.0, Task 4, D6): --config-dir + startup.json ──────
    // UNICA fonte di configurazione dell'intero orchestrator: nessuna
    // variabile d'ambiente `LARE_*`/`LOCALAPPDATA`/`APPDATA` viene letta in
    // questo file o altrove nel crate (eccetto `ANTHROPIC_API_KEY`/
    // `OPENROUTER_API_KEY`, chiavi dei provider AI — fuori scope D6, vedi
    // `default_ai_adapter`). `RuntimeConfig` è il "context object"
    // immutabile (analogia OOP: un `ApplicationContext`) costruito qui una
    // volta sola e condiviso (`Arc`) da tutto il resto di `main()` e da ogni
    // connessione WS — elimina le 5+ copie quasi identiche della stessa
    // catena di fallback che c'erano nella v1 (token, llms.json,
    // telegramsettings.json, search, plugin, aichat, tutte con la propria
    // risoluzione di `config_dir`).
    let args: Vec<String> = std::env::args().collect();
    // Assolutizzazione (fix wave finale, review): non più fatta qui — la fa
    // `startup_config::config_dir_from_process()` stessa, così `ui` e
    // `mcp-server` (che chiamano la stessa funzione) ne beneficiano allo
    // stesso modo, non solo l'orchestrator. Vedi il doc-comment della
    // funzione nel crate `startup-config` per il PERCHÉ (in breve: più sotto
    // in questa funzione la cwd cambia con `set_current_dir(home)` — un
    // `--config-dir` relativo va risolto PRIMA di quel cambio, alla fonte).
    let config_dir = startup_config::config_dir_from_process();
    let (startup, startup_warn) = startup_config::StartupConfig::load(&config_dir);
    let rt = Arc::new(RuntimeConfig { config_dir: config_dir.clone(), startup });

    // ── Tracing (2.0; panic-free dal fix round 1, vedi
    // `startup_config::logging`, modulo condiviso con `ui.exe`): sempre su
    // file (Configuration/logs/orchestrator.log, rotazione giornaliera);
    // ANCHE su console solo con --console-log (usato da init_*.ps1 per il
    // debug interattivo). Un orchestrator avviato in autostart non deve
    // sporcare (né bloccarsi su) un terminale che non ha — e non deve MAI
    // fermarsi all'avvio solo perché la cartella di log non è scrivibile
    // (`logging::init_logging` ripiega sulla sola console in quel caso,
    // non va mai in panic: vedi il doc-comment del modulo per il perché).
    let console = startup_config::has_flag(&args, "--console-log");
    let _log_guard = startup_config::logging::init_logging(
        &rt.log_dir(),
        "orchestrator.log",
        &rt.startup.log.level,
        console,
    );
    // `_log_guard` deve vivere fino alla fine di `main`: droppandolo si
    // interrompe il flush del writer non bloccante del file di log — tenuto
    // vivo per tutta la funzione semplicemente non spostandolo/droppandolo mai.
    if let Some(w) = startup_warn {
        tracing::warn!("{w}");
    }
    tracing::info!("config dir: {}", config_dir.display());

    tracing::info!(
        "Lare Terminal orchestrator v{} starting (protocol v{} · plugin-protocol v{} · mcp-server v{} · mcp-nmap v{})",
        ws::VERSION,
        env!("PROTOCOL_VERSION"),
        env!("PLUGIN_PROTOCOL_VERSION"),
        env!("MCP_SERVER_VERSION"),
        env!("MCP_NMAP_VERSION"),
    );

    // ── Auth token (ADR-007, D6) ─────────────────────────────────────────────
    // Vive SOLO in `<config_dir>/token` — nessuna `LARE_TOKEN` (v1: un
    // override impostato in un solo processo dei due produceva "connection
    // refused" senza spiegazione). Generato e persistito al primo avvio.
    let token = {
        let t = token_store::resolve_token(&config_dir);
        tracing::info!("token resolved (source logged to stderr on first run)");
        t
    };

    // ── AI adapter (Fase 2 / Slice 1-5) ───────────────────────────────────────
    // llms.json (opzionale, in `<config_dir>/llms.json` — 2.0: stesso path
    // risolto identicamente dall'orchestrator e dal tab /config della UI,
    // senza bisogno di condividere una cartella di lancio) sceglie il provider attivo
    // per QUESTA macchina (Anthropic diretto o OpenRouter/DeepSeek) — un solo
    // provider serve sia il cursore sia AI Chat (vedi memoria `multi-llm-openrouter`).
    // Retrocompatibilità DURA (spec Slice 4 §6): file assente, illeggibile, o
    // semanticamente non valido → fallback bit-per-bit a `default_ai_adapter()` (il
    // branch odierno, invariato), MAI un crash dell'intero orchestrator.
    let ai: Arc<dyn AiAdapter> = {
        let llms_path = llms_config::resolve_path(&config_dir);
        match llms_config::load(&llms_path) {
            Ok(Some(cfg)) => match llms_config::build_adapter(&cfg, &config_dir) {
                Ok(adapter) => {
                    tracing::info!("AI provider da llms.json: {}", adapter.provider());
                    adapter
                }
                Err(e) => {
                    tracing::error!(
                        "llms.json non valido ({e}) — fallback al comportamento odierno"
                    );
                    default_ai_adapter(&rt.startup.ai_model, &config_dir)
                }
            },
            Ok(None) => default_ai_adapter(&rt.startup.ai_model, &config_dir),
            Err(e) => {
                tracing::error!(
                    "llms.json: errore di lettura ({e}) — fallback al comportamento odierno"
                );
                default_ai_adapter(&rt.startup.ai_model, &config_dir)
            }
        }
    };

    // ── Check informativo "fonte dati mercato" (Task 11) ──────────────────────
    // Stessa filosofia "non bloccante, solo informativo" del log del provider
    // AI qui sopra: una fonte dati mercato (es. IB Gateway) spenta o non
    // raggiungibile non deve MAI impedire l'avvio del resto dell'orchestrator.
    // Spawnato in un task separato (non `.await`-ato inline qui) perché il
    // test di rete può richiedere fino al timeout dello scoped
    // `PythonMcpToolClient` (30s, vedi `ws::test_market_data_source_now`) —
    // aspettarlo inline ritarderebbe l'apertura del listener WS di altrettanto
    // ogni volta che TWS/Gateway non risponde, stesso motivo per cui
    // `IB.connect` ha un proprio timeout lato Python.
    {
        let rt = Arc::clone(&rt);
        tokio::spawn(async move {
            let (ok, message) = ws::test_market_data_source_now(&rt).await;
            if ok {
                tracing::info!("Fonte dati mercato: connessa");
            } else {
                tracing::warn!("Fonte dati mercato: non raggiungibile ({message})");
            }
        });
    }

    // ── Tool client (real MCP client → mcp-server child process) ─────────────
    let raw_tools = Arc::new(McpToolClient::resolve(&config_dir, &rt.startup));

    // ── Init home (Task 3) ────────────────────────────────────────────────────
    // Set the process cwd to the user's home directory at startup so that the
    // first cwd emitted to the UI is sensible (not the launch dir of the binary).
    // The mcp-server child inherits this cwd, so the shell also starts at home.
    // Best-effort: if `home_dir()` is None (unusual), stay in the launch dir.
    if let Some(home) = dirs::home_dir() {
        let _ = std::env::set_current_dir(&home);
        tracing::info!("cwd set to home: {}", home.display());
    }

    // Shared state: the current working directory of the persistent shell.
    // Initialised to the process cwd (= home after the set_current_dir above).
    let cwd_state = Arc::new(Mutex::new(
        std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
    ));

    // Wrap the real tool client with the cwd tracker.
    // Both the direct OS path (in ws.rs) and the AI tool-use path (in agent.rs)
    // call ToolClient::run_in_session — the tracker updates cwd_state for both.
    let tools: Arc<dyn orchestrator::tool_client::ToolClient> =
        Arc::new(CwdTrackingToolClient::new(raw_tools, Arc::clone(&cwd_state), config_dir.clone()));

    // ── Canale Telegram (opzionale, solo se telegramsettings.json presente) ───
    //
    // Flusso di avvio:
    //   1. Risolvi il path di configurazione: SEMPRE `<config_dir>/
    //      telegramsettings.json` (2.0, D6 — niente più env var/campo
    //      dedicato di `startup.json`, `config_dir` è il binding condiviso
    //      risolto una volta a inizio main()).
    //   2. Carica il file: assente → disattivo; errore → warn + disattivo.
    //   3. App-data dir: `config_dir` (stesso binding condiviso).
    //   4. Authenticator::init: prima run → stampa otpauth URI su stderr (una sola volta).
    //   5. Genera e stampa il codice di pairing su stderr.
    //   6. Spawna il canale come task tokio; ws::serve gira in parallelo.
    //
    // Security: il token Telegram NON compare mai nei log.
    {
        let tg_settings_path = telegram::settings::resolve_path(&config_dir);

        match telegram::settings::load(&tg_settings_path) {
            Ok(None) => {
                tracing::info!(
                    "canale Telegram disattivo (telegramsettings.json assente)"
                );
            }
            Err(e) => {
                // Non logghiamo il path completo né il token; solo il tipo di errore.
                tracing::warn!(
                    "telegramsettings.json: errore caricamento ({}); canale Telegram disattivo",
                    e.kind()
                );
            }
            Ok(Some(tg_settings)) => {
                if let Err(e) = std::fs::create_dir_all(&config_dir) {
                    tracing::warn!("telegram: impossibile creare app-data dir: {e}");
                }

                let state_path = config_dir.join("telegram-state.json");

                // ── TOTP init ────────────────────────────────────────────────
                let (mut auth, otpauth_uri) = Authenticator::init(&state_path);

                if let Some(uri) = otpauth_uri {
                    // Primo avvio: mostra il QR + URI una volta sola su stderr.
                    // NON viene ri-loggato né all'avvio successivo né nei log di tracing.
                    // Il QR/URI contengono il TOTP secret: solo stderr locale, mai tracing.
                    eprintln!();
                    eprintln!("[Telegram] *** PRIMO AVVIO — TOTP setup ***");
                    eprintln!("[Telegram] Scansiona questo QR con Google Authenticator:");

                    // Tenta il rendering QR; su errore salta il QR senza bloccare l'avvio.
                    match render_terminal_qr(&uri) {
                        Ok(qr) => eprint!("{qr}"),
                        Err(e) => eprintln!("[Telegram] (QR non disponibile: {e})"),
                    }

                    eprintln!("[Telegram] Oppure inserisci la chiave manualmente / usa questo URI:");
                    eprintln!("  {uri}");
                    eprintln!("[Telegram] Conserva l'URI in modo sicuro. Non verrà mostrato di nuovo.");
                    eprintln!();
                }

                // ── Codice di pairing ────────────────────────────────────────
                // Il codice vale PAIRING_TTL_MS (10 min). Se scade prima di essere
                // usato, riavviare l'orchestratore per ottenerne uno nuovo.
                // Il codice viene mostrato solo se la chat NON è già appaiata.
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;

                if auth.paired_chat_id().is_none() {
                    let pairing_code = auth.new_pairing_code(now_ms);
                    eprintln!(
                        "[Telegram] Per appaiare la chat invia al bot: /pair {pairing_code}"
                    );
                } else {
                    tracing::info!("telegram: chat già appaiata — pairing non necessario");
                }

                // ── Avvio canale ─────────────────────────────────────────────
                let tg_client = Arc::new(HttpTelegramClient::new(&tg_settings.token));
                tokio::spawn(telegram::run_channel(
                    tg_client,
                    auth,
                    state_path,
                    ai.clone(),
                    tools.clone(),
                ));
                tracing::info!("canale Telegram avviato");
            }
        }
    }

    // ── Search config + context (sempre attivo, indipendente da Telegram) ──────
    // search-paths.json vive nell'app-data dir (auto-generato al 1° avvio, editabile).
    let search = {
        let _ = std::fs::create_dir_all(&config_dir);
        let cfg_path = config_dir.join("search-paths.json");
        let content_cfg_path = config_dir.join("search-content.json");
        // Genera/migra i file all'avvio; i valori non sono tenuti: `launch`
        // rilegge da questi path a ogni ricerca.
        let _ = PathsConfig::load_or_generate(&cfg_path, &OsPathProvider);
        let _ = ContentConfig::load_or_generate(&content_cfg_path);
        SearchContext {
            engine: Arc::new(SearchEngine::default()),
            cfg_path: Arc::new(cfg_path),
            content_cfg_path: Arc::new(content_cfg_path),
            provider: Arc::new(OsPathProvider),
        }
    };

    // ── Plugin host (Task 5): scopri, eager-spawna, esponi ai client WS. ─────────
    // Se la dir è assente, `discover` restituisce un vettore vuoto e l'avvio
    // non viene bloccato. La dir dei plugin viene da `startup.json.paths.
    // plugins_dir` (2.0, D6 — **rimosso** il caso speciale `LARE_PLUGINS_DIR`
    // della v1, che bypassava trim/empty-check per restare "consistente" con
    // `plugins_view.rs` lato UI: un solo risolutore ora, `rt.plugins_dir()`).
    //
    // Task 5: `plugin_host` è ora `Arc<Mutex<PluginHost>>` per essere condiviso
    // tra connessioni WS. `plugin_commands` è la lista di comandi slash registrati
    // dai manifest dei plugin, usata dal router in ws.rs.
    let (plugin_host, plugin_commands) = {
        let plugins_dir = rt.plugins_dir();
        // Ogni plugin ottiene una sotto-dir privata per il proprio storage.
        let storage_root = config_dir.join("plugin-storage");
        let discovered = orchestrator::plugins::discovery::discover(&plugins_dir);
        tracing::info!(
            "plugin host: {} plugin/i scoperti in {}",
            discovered.len(),
            plugins_dir.display()
        );

        // Costruiamo la lista dei comandi slash PRIMA di muovere `discovered` in `start()`.
        // Ogni plugin lazy con un `triggers.command` registra la sua coppia ("/slash", "id").
        // Il router in ws.rs usa questa lista per intercettare i comandi slash dei plugin.
        let commands: Vec<(String, String)> = discovered
            .iter()
            .filter_map(|p| {
                p.manifest.triggers.command.as_ref()
                    .map(|cmd| (cmd.clone(), p.manifest.id.clone()))
            })
            .collect();

        // `PluginHost::start` eager-spawna i plugin che non hanno trigger di comando
        // (SpawnPolicy::Eager), invia Init e attende Ready su ciascuno.
        // La factory closure usa `spawn_plugin` (la versione split, Task 5) per il processo reale.
        // Nei test viene sostituita da `fake_transport` (Dependency Injection).
        let host = Arc::new(tokio::sync::Mutex::new(
            orchestrator::plugins::host::PluginHost::start(
                discovered,
                |p| {
                    orchestrator::plugins::transport::spawn_plugin(&p.bin_path, &config_dir)
                        .map(|(w, r)| {
                            (
                                Box::new(w) as Box<dyn orchestrator::plugins::transport::PluginWriter>,
                                Box::new(r) as Box<dyn orchestrator::plugins::transport::PluginReader>,
                            )
                        })
                },
                &storage_root,
            )
            .await
        ));

        (host, Arc::new(commands))
    };

    // ── Token di shutdown globale ────────────────────────────────────────────
    // Creato QUI (prima del blocco AI Chat) invece che subito prima di `ws::serve`
    // (come in versioni precedenti) perché `service.run(...)` — spawnato dentro il
    // blocco AI Chat sotto — deve ricevere un clone dello STESSO token: la
    // cancellazione (oggi innescata da "q"+invio, vedi sotto, o in futuro da un
    // segnale di stop del servizio OS) deve spegnere ordinatamente sia il WS
    // (`ws::serve`) sia l'attore del canale AI Chat, non solo il primo. Debito #2
    // ("teardown reale") del piano di hardening — vedi
    // `Docs/superpowers/specs/2026-07-02-aichat-hardening-debts-design.md`, §"Slice B".
    let shutdown = tokio_util::sync::CancellationToken::new();

    // ── Canale AI Chat (opzionale, solo se aichat.json ha enabled: true) ───────
    //
    // Pattern identico al blocco Telegram sopra:
    //   1. Risolvi il config in app-data (stesso path già usato per telegram + search).
    //   2. Se abilitato: costruisci PeerInfo (IP LAN via UDP trick), crea inbox, spawna servizio.
    //   3. Spawna il task di scoperta UDP (loop full-duplex — vedi nota sotto).
    //   4. Passa Some(inbox_tx) a ws::serve; altrimenti None.
    //
    // Nota loop full-duplex (debito #6, Slice D — risolto):
    // fino a `orchestrator` 0.27.0, `UdpDiscoverer::next` prendeva `&mut self` (per il
    // buffer riusabile), mentre `announce` prende `&self`: le due chiamate non potevano
    // coesistere in un `select!` sulla stessa variabile `disc` (aliasing `&mut`/`&`
    // vietato dal borrow checker). Il loop era quindi SEQUENZIALE (announce → finestra di
    // ascolto breve → ri-annuncio ogni ~7s), senza ascolto continuo. Fix: `next` è ora
    // `&self` anche su `UdpDiscoverer` (buffer di ricezione allocato localmente ad ogni
    // chiamata — vedi `net.rs`), quindi il loop qui sotto usa un vero `tokio::select!` fra
    // un timer di annuncio, l'ascolto continuo, e lo shutdown globale.
    let aichat_inbox: Option<
        tokio::sync::mpsc::UnboundedSender<orchestrator::aichat::service::ServiceEvent>,
    > = {
        if let Err(e) = std::fs::create_dir_all(&config_dir) {
            tracing::warn!("aichat: impossibile creare app-data dir: {e}");
        }

        // Rinominato da aichat.json: il file contiene identità di rete condivisa
        // da più funzionalità (AI Chat, Blocco Note), non solo AI Chat — vedi
        // Docs/superpowers/specs/2026-07-29-library-notes-design.md §9.
        let cfg = orchestrator::aichat::config::load_or_generate_with_migration(
            &rt.network_json_path(),
            &config_dir.join("aichat.json"),
        );

        if cfg.enabled {
            // ── IP LAN locale ────────────────────────────────────────────────
            // `detect_local_ipv4` usa il UDP connect trick (nessun pacchetto inviato).
            // Fallback a 127.0.0.1 se la macchina non ha interfacce IPv4: il servizio
            // parte comunque ma la scoperta non raggiungerà gli altri host della LAN.
            let local_ip = detect_local_ipv4().unwrap_or_else(|| {
                tracing::warn!(
                    "aichat: impossibile determinare l'IP LAN — uso 127.0.0.1 \
                     (scoperta limitata alla stessa macchina)"
                );
                std::net::Ipv4Addr::LOCALHOST
            });

            // Il `PeerInfo` identifica questo nodo nel protocollo di scoperta e di chat.
            // `label_base` è il nome configurabile (es. "lare"); `chat_port` è usata
            // sia per il listener TCP sia per il broadcast UDP (spazi di porta distinti).
            let me = orchestrator::aichat::peer::PeerInfo {
                id: orchestrator::aichat::peer::PeerId(local_ip),
                label_base: cfg.label_base.clone(),
                chat_port: cfg.chat_port,
            };

            // ── Canale inbox (UnboundedSender clonato a ws.rs + task discovery) ──
            // Il `UnboundedSender` è `Clone`; ogni clone è un handle verso lo stesso
            // attore. `ws.rs` ne riceve una copia via `serve(... Some(inbox_tx))` e
            // il task discovery ne usa un'altra per iniettare `Discovered(peer)`.
            let (inbox_tx, inbox_rx) = tokio::sync::mpsc::unbounded_channel::<
                orchestrator::aichat::service::ServiceEvent,
            >();

            // ── Task attore: possiede lo stato in esclusiva ──────────────────
            // `run` consuma `self` (`mut self`) — in Rust questo garantisce che
            // nessun altro codice possa modificare lo stato dell'attore direttamente.
            // L'unico modo di interagire con esso è inviare `ServiceEvent` via `inbox_tx`.
            // `ai.clone()` riusa lo STESSO Arc<dyn AiAdapter> costruito sopra per il
            // cursore (StubAdapter senza API key, LlmAdapter+ClaudeBackend con chiave): la propria
            // AI locale del canale AI Chat (Slice 1a/1b, invocazione `@ai`/`@all`/
            // `@<label>-ai`) è quindi sempre coerente con quella del cursore.
            // `cfg.ai_participates` (Slice 1b, design §9.4): flag "la mia AI
            // partecipa", letto UNA volta all'avvio (non live) — gata SOLO le
            // invocazioni remote (`@all`/`@<mio-label>-ai` da un umano di un'altra
            // macchina), mai la propria (vedi il doc-comment del campo in service.rs).
            // `cfg.ai_autoparticipate` (Slice 2, design §10.1): flag opt-in
            // "auto-partecipazione" — fa intervenire l'AI SPONTANEAMENTE sui
            // messaggi normali della stanza, bounded dal cap sui turni consecutivi
            // (vedi `MAX_CONSECUTIVE_AI_TURNS` in service.rs). Default `false`.
            // Blocco note (design §8): store di proprietà dell'orchestrator, stessa
            // app-data dir di network.json — sopravvive a UI chiusa e a un riavvio.
            let notes_store =
                orchestrator::notes::store::NotesStore::load_or_generate(&config_dir.join("notes.json"));
            // Display name per l'utente (label locale) e l'AI partecipante nel canale.
            // Carichi da network.json via AiChatConfig (Task 1, caricamento) +
            // Task 3 (firma del costruttore) + Task 4 qui (wiring).
            let service = orchestrator::aichat::service::AiChatService::new(
                me.clone(),
                ai.clone(),
                cfg.ai_participates,
                cfg.ai_autoparticipate,
                notes_store,
                cfg.display_name.clone(),
                cfg.ai_display_name.clone(),
                config_dir.clone(),
            );
            // Bug 2 (late-joiner) §3.6: clona l'handle PRIMA che `run` consumi `service`
            // (`run` prende `self` per valore). Il task di scoperta lo legge a ogni
            // `announce()` per includere il leader attuale nel proprio annuncio UDP.
            let believed_leader = service.believed_leader_handle();
            tokio::spawn(service.run(inbox_rx, inbox_tx.clone(), shutdown.clone()));

            // ── Task scoperta UDP (full-duplex, debito #6 Slice D) ────────────
            // Scopo: annuncia periodicamente la propria presenza (broadcast, incluso il
            // leader attualmente creduto) e inietta i `(PeerInfo, leader riportato)`
            // ricevuti nell'inbox del servizio come `Discovered(peer, leader)`.
            {
                let inbox = inbox_tx.clone();
                let me_clone = me.clone();
                let believed_leader = Arc::clone(&believed_leader);
                // Clone del token globale di shutdown (Slice B, debito #2): fino a questa
                // slice, il task di scoperta girava per sempre — nessun `break` era
                // raggiungibile anche a processo in chiusura. Il ramo `shutdown_disc.
                // cancelled()` nel `select!` sotto completa il teardown per questo task
                // come già avviene per `service.run` e per l'accept loop.
                let shutdown_disc = shutdown.clone();
                // La porta UDP di scoperta coincide numericamente con la porta TCP della chat
                // (40100 di default). Sono spazi distinti: UDP e TCP non condividono le porte.
                let disc_port = cfg.chat_port;
                tokio::spawn(async move {
                    // `UdpDiscoverer::new` può fallire se la porta è già occupata
                    // (es. altra istanza sulla stessa macchina). In quel caso logghiamo
                    // e il task termina: il servizio funziona comunque senza scoperta.
                    // NB: non più `mut` — `announce`/`next` prendono entrambi `&self` da
                    // questa slice in poi (vedi `discovery.rs`/`net.rs`), quindi il loop
                    // sotto prende solo prestiti immutabili di `disc`, mai esclusivi.
                    let disc = match orchestrator::aichat::net::UdpDiscoverer::new(
                        me_clone,
                        disc_port,
                    )
                    .await
                    {
                        Ok(d) => d,
                        Err(e) => {
                            tracing::warn!(
                                "aichat discovery: bind UDP porta {disc_port} fallito \
                                 ({e}) — scoperta disattiva"
                            );
                            return;
                        }
                    };

                    // Timer di annuncio: un tick ogni `ANNOUNCE_INTERVAL`.
                    // `tokio::time::interval` fa scattare il PRIMO tick immediatamente
                    // (comportamento di default) — equivalente al "primo annuncio subito"
                    // del vecchio schema — e poi ogni `ANNOUNCE_INTERVAL` da lì in poi.
                    const ANNOUNCE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);
                    let mut announce_tick = tokio::time::interval(ANNOUNCE_INTERVAL);

                    // Full-duplex: `select!` osserva TRE sorgenti concorrenti sullo stesso
                    // `disc` (un prestito `&disc` per ramo, mai `&mut` — è ciò che il fix
                    // del trait abilita). Prima di questa slice, `announce` e `next` non
                    // potevano coesistere qui: il loop era sequenziale (announce → finestra
                    // di ascolto breve → ri-annuncio).
                    //
                    // Sicurezza rispetto alla cancellazione: quando il ramo `announce_tick`
                    // vince, il future `disc.next()` in corso viene ABBANDONATO (drop) e
                    // ricreato al giro successivo. Questo NON perde datagrammi: `recv_from`
                    // di `tokio::UdpSocket` è cancellation-safe — se il future viene droppato
                    // mentre è ancora in attesa, non ha ancora estratto nulla dal buffer del
                    // kernel; un datagramma arrivato nel frattempo resta lì e viene letto dal
                    // prossimo `next()`. Se invece `recv_from` ha GIÀ un datagramma pronto,
                    // `next()` lo decodifica e ritorna `Some(..)` in modo sincrono (nessun
                    // `.await` fra la lettura e il `return`), quindi non può essere
                    // interrotto a metà con un risultato già pronto perso.
                    loop {
                        tokio::select! {
                            _ = announce_tick.tick() => {
                                // Bug 2 (late-joiner) §3.6: legge il leader ATTUALMENTE
                                // creduto dall'attore e lo include nell'annuncio, così un
                                // peer che si unisce dopo può adottarlo invece di
                                // ricalcolare l'elezione dal proprio IP più basso.
                                // Il valore va estratto in una variabile PRIMA dell'.await:
                                // un `MutexGuard` (std) non è `Send`, quindi non può
                                // restare "vivo" attraverso un punto di sospensione
                                // dentro un ramo di `select!` (stesso vincolo del vecchio
                                // codice, qui semplicemente riportato).
                                let leader_to_announce = *believed_leader.lock().unwrap();
                                if let Err(e) = disc.announce(leader_to_announce).await {
                                    tracing::debug!("aichat discovery: announce error: {e}");
                                }
                            }
                            maybe = disc.next() => {
                                match maybe {
                                    Some((peer, leader)) => {
                                        let _ = inbox.send(
                                            orchestrator::aichat::service::ServiceEvent::Discovered(peer, leader),
                                        );
                                    }
                                    None => {
                                        // Sorgente esaurita/chiusa in modo permanente
                                        // (non i normali errori transitori, già gestiti
                                        // e ignorati dentro `UdpDiscoverer::next`).
                                        tracing::warn!(
                                            "aichat discovery: discoverer chiuso — task terminato"
                                        );
                                        break;
                                    }
                                }
                            }
                            _ = shutdown_disc.cancelled() => {
                                // Debito #2 (Slice B) esteso al task di scoperta.
                                break;
                            }
                        }
                    }
                });
            }

            tracing::info!(
                "canale AI Chat avviato (label={}, IP={}, porta={})",
                cfg.label_base,
                local_ip,
                cfg.chat_port
            );
            Some(inbox_tx)
        } else {
            tracing::info!("canale AI Chat disattivo");
            None
        }
    };

    // ── Uscita pulita interattiva ("Q" + invio) ────────────────────────────────
    // Senza questo, l'unico modo di fermare il processo era Ctrl+C: interruzione
    // brusca, niente chiusura ordinata di listener/plugin. Il task legge stdin su
    // un thread bloccante dedicato (spawn_blocking: std::io::stdin().lines() blocca
    // il thread OS, non va mai fatto direttamente su un task async) e cancella il
    // token quando arriva "q"/"quit". Quando i processi gireranno come servizi,
    // l'uscita pulita sarà innescata dallo stop del servizio, non da stdin.
    // (`shutdown` è stato creato PRIMA del blocco AI Chat sopra — vedi il commento
    // lì — così un clone è già stato passato a `service.run(...)`.)
    {
        let shutdown = shutdown.clone();
        tokio::task::spawn_blocking(move || {
            use std::io::BufRead;
            println!("Digita 'q' e invio per uscire pulito.");
            let stdin = std::io::stdin();
            for line in stdin.lock().lines() {
                let Ok(line) = line else { break }; // stdin chiuso (EOF) → smetti di leggere
                if is_quit_command(&line) {
                    println!("uscita pulita richiesta...");
                    shutdown.cancel();
                    break;
                }
            }
        });
    }

    // ── WebSocket server ──────────────────────────────────────────────────────
    // `listen`: `127.0.0.1:<ws_port>` da `startup.json` (2.0 — sostituisce la
    // vecchia porta fissa 7331 di v1, hard-coded in una costante di questo
    // modulo; il default di `StartupConfig` è comunque 7331, quindi il
    // comportamento di sempre è invariato a config assente). Cattura il
    // risultato prima di fare shutdown: anche se serve()
    // restituisce Err, i plugin vengono fermati correttamente (kill_on_drop li termina).
    let listen = format!("127.0.0.1:{}", rt.startup.ws_port);
    let registry = orchestrator::connections::Registry::shared();
    let serve_result = ws::serve(
        &listen,
        Arc::new(token),
        ai,
        tools,
        cwd_state,
        search,
        Arc::clone(&plugin_host),
        Arc::clone(&plugin_commands),
        aichat_inbox,
        shutdown,
        Arc::clone(&rt),
        registry,
    )
    .await;
    plugin_host.lock().await.shutdown().await;
    serve_result?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_quit_command_recognizes_q_and_quit_case_insensitive() {
        assert!(is_quit_command("q"));
        assert!(is_quit_command("Q"));
        assert!(is_quit_command("quit"));
        assert!(is_quit_command("QUIT"));
        assert!(is_quit_command("  q  "), "gli spazi ai bordi vanno ignorati");
    }

    #[test]
    fn is_quit_command_rejects_everything_else() {
        assert!(!is_quit_command(""));
        assert!(!is_quit_command("quitter"));
        assert!(!is_quit_command("dir"));
    }
}
