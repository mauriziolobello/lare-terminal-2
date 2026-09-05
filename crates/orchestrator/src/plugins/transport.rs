//! Plugin transport: il SEAM verso un plugin. Task 5 introduce i trait split
//! `PluginWriter`/`PluginReader` in modo che l'host possa inviare messaggi (via writer,
//! dietro Mutex) MENTRE un task pump riceve in modo concorrente (via reader, posseduto
//! esclusivamente dal task). L'impl combinata originale `PluginTransport` +
//! `FakePluginTransport` viene mantenuta per il test `fake_records_sent_via_shared_log`.
//!
//! ## Perché il split (motivazione antipattern "Mutex deadlock")
//! Se writer e reader fossero nello stesso oggetto dietro un Mutex<Box<dyn PluginTransport>>,
//! il task pump chiamerebbe `lock().await + recv()` e terrebbe il lock per tutta la durata
//! dell'I/O asincrono. Nel frattempo l'host non potrebbe mai acquisire il lock per `send()`.
//! Il split risolve: il writer vive nell'HashMap dell'host (dietro il Mutex dell'host),
//! il reader vive nel task pump (nessun Mutex: possesso esclusivo).
//!
//! ## Design (SOLID — DIP)
//! L'host (Task 5) dipende dai trait `PluginWriter`/`PluginReader`, non dai tipi concreti.
//! In produzione: `ChildPluginWriter`/`ChildPluginReader` (processo stdio JSON-per-riga).
//! Nei test: `FakeWriter`/`FakeReader` (in-memory, senza I/O).
//!
//! ## Funzione bridge pura
//! `plugin_msg_to_server` traduce un messaggio Plugin→Host in un ServerMsg (o `None`).
//! È pura (no I/O, no side-effect), semplice da testare in isolamento.

use async_trait::async_trait;
use plugin_protocol::{HostToPlugin, PluginToHost};
use std::path::Path;

// ─── Trait combinato originale (mantenuto) ────────────────────────────────────
//
// Mantenuto per il test `fake_records_sent_via_shared_log_and_replays_incoming`
// che usa `FakePluginTransport`. Non usato nel nuovo codice di produzione.

/// Seam combinato send+recv. Usato da `FakePluginTransport` (test legacy).
/// Il nuovo codice usa i trait split `PluginWriter`/`PluginReader`.
#[async_trait]
pub trait PluginTransport: Send {
    /// Serializza e invia un messaggio Host->Plugin (una riga JSON + '\n').
    async fn send(&mut self, msg: HostToPlugin) -> std::io::Result<()>;
    /// Legge il prossimo messaggio Plugin->Host. `Ok(None)` = EOF.
    async fn recv(&mut self) -> std::io::Result<Option<PluginToHost>>;
}

// ─── Nuovi trait split (Task 5) ───────────────────────────────────────────────

/// Metà "scrittura" del transport: posseduta dall'host dietro il suo Mutex.
/// L'host invia messaggi (Init, Activate, UiEvent, Deinit) tramite questo trait.
///
/// In produzione: `ChildPluginWriter` (scrive su stdin del processo plugin).
/// Nei test: `FakeWriter` (accoda nel `SentLog` condiviso).
#[async_trait]
pub trait PluginWriter: Send {
    /// Invia un messaggio all'host. Tipicamente una riga JSON + '\n' su stdin.
    async fn send(&mut self, msg: HostToPlugin) -> std::io::Result<()>;
}

/// Metà "lettura" del transport: posseduta ESCLUSIVAMENTE dal task pump.
/// Il pump chiama `recv()` in loop senza dover competere con nessun lock.
///
/// In produzione: `ChildPluginReader` (legge da stdout del processo plugin).
/// Nei test: `FakeReader` (legge da una `VecDeque` prefissata).
#[async_trait]
pub trait PluginReader: Send {
    /// Legge il prossimo messaggio Plugin→Host. `Ok(None)` = EOF (plugin terminato).
    async fn recv(&mut self) -> std::io::Result<Option<PluginToHost>>;
}

// ─── Pure bridge fn ───────────────────────────────────────────────────────────

/// Traduce un messaggio Plugin→Host in un messaggio Server→UI (o lo scarta).
///
/// Chiamata dal pump task in host.rs per decidere se e cosa inviare al client WS.
/// È pura (no I/O, no side-effect): testabile direttamente e senza tokio runtime.
///
/// Il parametro `window` è la dimensione iniziale dichiarata nel manifest del
/// plugin (`plugin.json`), passata dall'host. Rilevante SOLO al momento della
/// creazione della finestra (`ShowWindow` → `OpenPluginWindow`): viene IGNORATO
/// per `UpdateWindow`/`CloseWindow`, dove la dimensione non ha significato.
///
/// # Mapping
/// | PluginToHost                       | ServerMsg                                    |
/// |------------------------------------|----------------------------------------------|
/// | ShowWindow { wid, title, html }    | OpenPluginWindow { wid, title, html, w?, h? } |
/// | UpdateWindow { wid, html }         | UpdatePluginWindow { wid, html }             |
/// | CloseWindow { wid }                | ClosePluginWindow { wid }                    |
/// | Ready { .. } / Log { .. }          | None (lifecycle/telemetry)                   |
pub fn plugin_msg_to_server(
    m: PluginToHost,
    window: Option<plugin_protocol::WindowSize>,
) -> Option<protocol::ServerMsg> {
    // Ogni variante "finestra" produce il ServerMsg corrispondente;
    // Ready e Log sono segnali interni (handshake / telemetria) — non vanno al client WS.
    match m {
        PluginToHost::ShowWindow { window_id, title, html } => {
            Some(protocol::ServerMsg::OpenPluginWindow {
                window_id,
                title,
                html,
                // Scompone la dimensione opzionale in due Option<f64> per il wire.
                // `window` è None per i plugin che non dichiarano `window` nel manifest
                // (calc/ping/counter) → width/height restano None → omessi dal JSON.
                width: window.map(|w| w.width),
                height: window.map(|w| w.height),
            })
        }
        PluginToHost::UpdateWindow { window_id, html } => {
            // `window` intenzionalmente ignorato: la dimensione conta solo alla creazione.
            Some(protocol::ServerMsg::UpdatePluginWindow { window_id, html })
        }
        PluginToHost::CloseWindow { window_id } => {
            Some(protocol::ServerMsg::ClosePluginWindow { window_id })
        }
        // Segnali di lifecycle/telemetria: non interessano l'UI.
        PluginToHost::Ready { .. } | PluginToHost::Log { .. } => None,
    }
}

// ─── Log condiviso ─────────────────────────────────────────────────────────────

/// Log condiviso dei messaggi inviati: clonabile, così il test lo ispeziona anche dopo che
/// il writer è stato boxed nell'host. `Default::default()` crea un `Arc<Mutex<Vec>>` vuoto.
///
/// Usato da `FakePluginTransport` (test legacy) e dai nuovi `FakeWriter`/`FakeReader`.
pub type SentLog = std::sync::Arc<std::sync::Mutex<Vec<HostToPlugin>>>;

// ─── Fake impl originale (mantenuta) ──────────────────────────────────────────
//
// Mantenuta per il test `fake_records_sent_via_shared_log_and_replays_incoming`
// che verifica il comportamento del trait combinato.

/// Fake in-memory per i test legacy del trait combinato `PluginTransport`.
/// Nuovi test usano `FakeWriter`/`FakeReader` tramite `fake_transport()`.
pub struct FakePluginTransport {
    sent: SentLog,
    incoming: std::collections::VecDeque<PluginToHost>,
}

impl FakePluginTransport {
    pub fn new(sent: SentLog, incoming: Vec<PluginToHost>) -> Self {
        Self { sent, incoming: incoming.into() }
    }
}

#[async_trait]
impl PluginTransport for FakePluginTransport {
    async fn send(&mut self, msg: HostToPlugin) -> std::io::Result<()> {
        self.sent.lock().unwrap().push(msg);
        Ok(())
    }
    async fn recv(&mut self) -> std::io::Result<Option<PluginToHost>> {
        Ok(self.incoming.pop_front())
    }
}

// ─── Nuove fake impl split (Task 5) ───────────────────────────────────────────

/// Fake writer: registra ogni messaggio inviato nel `SentLog` condiviso.
/// Il test mantiene un clone del `SentLog` per ispezionarlo dopo.
///
/// ## Onestà del test
/// `send` è sincrono (non yield-point): registra il messaggio PRIMA di ritornare.
/// Questo garantisce che le asserzioni sul `SentLog` dopo una `await` non abbiano
/// race condition — il messaggio è già lì, non "in volo".
///
/// ## Modalità "fail-after-n"
/// Se `ok_sends_left` è `Some(n)`, i primi `n` send riescono e vengono loggati;
/// il send numero `n+1` (e successivi) ritorna `Err` e NON viene loggato
/// (realism: un messaggio fallito non è stato trasmesso al plugin).
pub struct FakeWriter {
    /// Log condiviso: il test ne tiene un clone clonato PRIMA del box.
    sent: SentLog,
    /// Quanti send riescono ancora. `None` = sempre Ok (comportamento normale).
    /// Viene decrementato a ogni send riuscito; quando arriva a 0 il send fallisce.
    ok_sends_left: Option<usize>,
}

/// Fake reader: restituisce messaggi da una coda prefissata (FIFO).
/// Quando la coda è esaurita, ritorna `Ok(None)` (simula EOF del plugin).
///
/// ## Modalità "hang"
/// Se `hang` è `true`, `recv()` non ritorna mai (usa `std::future::pending`).
/// Simula un plugin che non invia mai `Ready`. Usato dal test I2 insieme al
/// timeout in `spawn_and_handshake`: pre-fix il test si blocca; post-fix il
/// `tokio::time::timeout(5s, ...)` lo interrompe.
pub struct FakeReader {
    incoming: std::collections::VecDeque<PluginToHost>,
    /// Se `true`, `recv()` non ritorna mai — simula un plugin congelato.
    hang: bool,
}

#[async_trait]
impl PluginWriter for FakeWriter {
    async fn send(&mut self, msg: HostToPlugin) -> std::io::Result<()> {
        // Controlla il contatore "fail-after-n" PRIMA di registrare nel log.
        // Un invio fallito non deve apparire nel log (il messaggio non è arrivato).
        if let Some(ref mut n) = self.ok_sends_left {
            if *n == 0 {
                return Err(std::io::Error::other(
                    "FakeWriter: send fallito (simula broken pipe — plugin crashato)",
                ));
            }
            *n -= 1;
        }
        // Registra nel log condiviso — il test lo ispeziona dopo il box+move.
        self.sent.lock().unwrap().push(msg);
        Ok(())
    }
}

#[async_trait]
impl PluginReader for FakeReader {
    async fn recv(&mut self) -> std::io::Result<Option<PluginToHost>> {
        if self.hang {
            // Simula un plugin congelato che non invia mai Ready.
            // Pre-fix: spawn_and_handshake si blocca qui per sempre.
            // Post-fix: tokio::time::timeout(5s) interrompe l'attesa.
            // `unreachable!` è raggiungibile solo in teoria — pending() non ritorna mai.
            std::future::pending::<()>().await;
            unreachable!("pending() non ritorna mai")
        }
        // FIFO: pop_front estrae in ordine di inserimento.
        // Coda vuota → None = EOF simulato del plugin.
        Ok(self.incoming.pop_front())
    }
}

/// Crea una coppia (writer, reader) fake per i test dell'host.
/// Il `log` passato è il log CONDIVISO: il test lo clona prima e lo ispeziona dopo.
///
/// Esempio:
/// ```ignore
/// let log: SentLog = Default::default();
/// let (w, r) = fake_transport(log.clone(), vec![
///     PluginToHost::Ready { name: "p".into(), protocol_version: 1 },
/// ]);
/// // passa w e r alla factory dell'host, poi controlla log dopo start()/activate()
/// ```
pub fn fake_transport(log: SentLog, incoming: Vec<PluginToHost>) -> (FakeWriter, FakeReader) {
    (
        FakeWriter { sent: log, ok_sends_left: None },
        FakeReader { incoming: incoming.into(), hang: false },
    )
}

/// Crea una coppia (writer, reader) dove il writer fallisce dopo `ok_sends` invii riusciti.
///
/// Usato nel test I1 (robustezza rispawn): `ok_sends = 1` → Init riesce (1 send), Activate
/// fallisce (send #2). Il test verifica che il writer morto venga rimosso da `writers` così
/// che una seconda `activate` possa rispawnare il plugin da zero.
pub fn fake_transport_fail_after(
    log: SentLog,
    ok_sends: usize,
    incoming: Vec<PluginToHost>,
) -> (FakeWriter, FakeReader) {
    (
        FakeWriter { sent: log, ok_sends_left: Some(ok_sends) },
        FakeReader { incoming: incoming.into(), hang: false },
    )
}

/// Crea una coppia (writer, reader) dove il reader non ritorna mai.
///
/// Usato nel test I2 (timeout Ready-gate): il writer funziona normalmente (Init viene
/// inviato), ma il reader è congelato — simula un plugin che non risponde dopo Init.
/// Con `#[tokio::test(start_paused = true)]` e il `tokio::time::timeout(5s)` in
/// `spawn_and_handshake`, tokio avanza il tempo virtuale di 5s e il timeout scatta
/// istantaneamente senza rallentare la suite.
pub fn fake_transport_hanging(log: SentLog) -> (FakeWriter, FakeReader) {
    (
        FakeWriter { sent: log, ok_sends_left: None },
        FakeReader { incoming: Default::default(), hang: true },
    )
}

// ─── Impl reale: processo figlio JSON-per-riga ─────────────────────────────────

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};

/// Writer reale: scrive messaggi JSON su stdin del processo plugin.
///
/// Quando droppato, il `ChildPluginReader` (che possiede il `Child`) termina il processo
/// grazie a `kill_on_drop(true)`. Il writer non deve fare altro alla chiusura.
pub struct ChildPluginWriter {
    /// stdin del processo plugin: destinazione dei messaggi HostToPlugin.
    stdin: ChildStdin,
}

/// Reader reale: legge messaggi JSON da stdout del processo plugin.
///
/// Possiede il `Child` per garantire che il processo venga terminato al drop.
/// `kill_on_drop(true)` su `Command` fa sì che il drop del `Child` invii SIGKILL/
/// TerminateProcess al plugin — nessun processo orfano.
pub struct ChildPluginReader {
    /// Tenuto vivo per il kill_on_drop: al drop di `ChildPluginReader` il processo
    /// plugin viene terminato automaticamente.
    #[allow(dead_code)]
    child: Child,
    /// stdout del processo plugin, bufferizzato per `read_line` efficiente.
    stdout: BufReader<tokio::process::ChildStdout>,
}

#[async_trait]
impl PluginWriter for ChildPluginWriter {
    async fn send(&mut self, msg: HostToPlugin) -> std::io::Result<()> {
        // Serializza in JSON, aggiungi newline (separatore di messaggio), flush.
        // map_err: converte serde_json::Error in std::io::Error (stabile da 1.74).
        let mut line = serde_json::to_string(&msg).map_err(std::io::Error::other)?;
        line.push('\n');
        self.stdin.write_all(line.as_bytes()).await?;
        self.stdin.flush().await
    }
}

#[async_trait]
impl PluginReader for ChildPluginReader {
    async fn recv(&mut self) -> std::io::Result<Option<PluginToHost>> {
        let mut line = String::new();
        let n = self.stdout.read_line(&mut line).await?;
        if n == 0 {
            return Ok(None); // EOF: il plugin ha chiuso stdout (terminato o in errore)
        }
        // trim_end: rimuove il '\n' (e '\r\n' su Windows) prima del parse JSON.
        let msg = serde_json::from_str(line.trim_end()).map_err(std::io::Error::other)?;
        Ok(Some(msg))
    }
}

/// Spawna il binario del plugin e ritorna le due metà del transport (writer, reader).
///
/// Il `ChildPluginReader` possiede il `Child` e lo termina al drop (`kill_on_drop(true)`).
/// Il `ChildPluginWriter` possiede solo lo stdin; al suo drop non accade nulla di speciale
/// (il processo viene gestito dal reader).
///
/// Sostituisce `ChildPluginTransport::spawn` nel nuovo design split.
///
/// `config_dir` viene passato al plugin come `--config-dir <path>` (spec
/// 2.0 §6.1: la regola vale per OGNI binario Lare, plugin inclusi, senza
/// eccezioni) — anche se, verificato, nessun plugin oggi fa parsing di argv
/// e quindi lo ignora: l'argomento resta inerte finché un plugin non ne ha
/// bisogno (es. per leggere `startup.json` da solo), ma la regola dello
/// spec vale comunque da subito, non solo quando servirà davvero.
///
/// # Errors
/// Ritorna `Err` se il binario non esiste o non può essere eseguito.
pub fn spawn_plugin(
    bin_path: &Path,
    config_dir: &Path,
) -> std::io::Result<(ChildPluginWriter, ChildPluginReader)> {
    let mut child = Command::new(bin_path)
        .arg(startup_config::CONFIG_DIR_FLAG)
        .arg(config_dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit()) // stderr del plugin → log orchestrator
        .kill_on_drop(true)
        .spawn()?;
    // take() estrae gli handle Option<> dal Child; dopo take() i campi sono None,
    // ma gli handle sono in nostro possesso separatamente.
    let stdin = child.stdin.take().ok_or_else(|| std::io::Error::other("no stdin"))?;
    let stdout = child.stdout.take().ok_or_else(|| std::io::Error::other("no stdout"))?;
    Ok((
        ChildPluginWriter { stdin },
        ChildPluginReader { child, stdout: BufReader::new(stdout) },
    ))
}

// ─── Test ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use plugin_protocol::{HostToPlugin, PluginToHost};

    // ─── Bridge fn test (TDD: RED era "cannot find function `plugin_msg_to_server`") ──
    //
    // Questo test descrive il contratto completo della bridge function:
    // - ShowWindow   → OpenPluginWindow  (i field devono essere identici, ordine incluso)
    // - UpdateWindow → UpdatePluginWindow
    // - CloseWindow  → ClosePluginWindow
    // - Ready        → None  (handshake, non un evento UI)
    // - Log          → None  (telemetria, non un evento UI)
    //
    // Usiamo title/html DIVERSI per rilevare eventuali trasposizioni di campo
    // (un test con title == html non rileva il bug "title e html sono scambiati").
    #[test]
    fn plugin_msg_to_server_maps_correctly() {
        // ShowWindow → OpenPluginWindow con field identici
        let show = PluginToHost::ShowWindow {
            window_id: 7,
            title: "Titolo finestra".to_string(),
            html: "<b>contenuto</b>".to_string(),
        };
        // Senza dimensione dichiarata (window = None): width/height restano None.
        assert_eq!(
            plugin_msg_to_server(show, None),
            Some(protocol::ServerMsg::OpenPluginWindow {
                window_id: 7,
                title: "Titolo finestra".to_string(),
                html: "<b>contenuto</b>".to_string(),
                width: None,
                height: None,
            }),
            "ShowWindow deve mappare su OpenPluginWindow con tutti i field identici"
        );

        // UpdateWindow → UpdatePluginWindow
        assert_eq!(
            plugin_msg_to_server(PluginToHost::UpdateWindow {
                window_id: 7,
                html: "<i>aggiornato</i>".to_string(),
            }, None),
            Some(protocol::ServerMsg::UpdatePluginWindow {
                window_id: 7,
                html: "<i>aggiornato</i>".to_string(),
            })
        );

        // CloseWindow → ClosePluginWindow
        assert_eq!(
            plugin_msg_to_server(PluginToHost::CloseWindow { window_id: 7 }, None),
            Some(protocol::ServerMsg::ClosePluginWindow { window_id: 7 })
        );

        // Ready → None (segnale di handshake, non va all'UI)
        assert_eq!(
            plugin_msg_to_server(PluginToHost::Ready {
                name: "test".into(),
                protocol_version: 1,
            }, None),
            None,
            "Ready non deve produrre un ServerMsg (e' il segnale di handshake)"
        );

        // Log → None (telemetria del plugin, non va all'UI)
        assert_eq!(
            plugin_msg_to_server(PluginToHost::Log {
                level: "info".into(),
                msg: "ciao".into(),
            }, None),
            None,
            "Log non deve produrre un ServerMsg (e' telemetria interna)"
        );
    }

    /// La dimensione dichiarata nel manifest (`window = Some(...)`) deve fluire nei
    /// campi width/height di `OpenPluginWindow` — SOLO per `ShowWindow`. Per
    /// `UpdateWindow`/`CloseWindow` la dimensione va ignorata (non deve "trapelare"
    /// in un tipo di messaggio dove non ha senso).
    #[test]
    fn plugin_msg_to_server_threads_window_size_only_on_show() {
        use plugin_protocol::WindowSize;
        let size = Some(WindowSize { width: 960.0, height: 620.0 });

        // ShowWindow + Some(size) → OpenPluginWindow con quei valori esatti.
        let show = PluginToHost::ShowWindow {
            window_id: 1,
            title: "Lare Commander".to_string(),
            html: "<div/>".to_string(),
        };
        assert_eq!(
            plugin_msg_to_server(show, size),
            Some(protocol::ServerMsg::OpenPluginWindow {
                window_id: 1,
                title: "Lare Commander".to_string(),
                html: "<div/>".to_string(),
                width: Some(960.0),
                height: Some(620.0),
            }),
            "la dimensione del manifest deve raggiungere OpenPluginWindow"
        );

        // UpdateWindow + Some(size) → variante invariata (dimensione ignorata).
        assert_eq!(
            plugin_msg_to_server(
                PluginToHost::UpdateWindow { window_id: 1, html: "<i/>".to_string() },
                size,
            ),
            Some(protocol::ServerMsg::UpdatePluginWindow {
                window_id: 1,
                html: "<i/>".to_string(),
            }),
            "UpdateWindow non deve trasportare la dimensione"
        );

        // CloseWindow + Some(size) → variante invariata (dimensione ignorata).
        assert_eq!(
            plugin_msg_to_server(PluginToHost::CloseWindow { window_id: 1 }, size),
            Some(protocol::ServerMsg::ClosePluginWindow { window_id: 1 }),
            "CloseWindow non deve trasportare la dimensione"
        );
    }

    // ─── Test legacy: FakePluginTransport (PluginTransport combinato) ─────────────
    //
    // Questo test rimane come prova che il trait combinato originale funziona.
    // Il suo scopo e' documentale: mostra che `SentLog` e' ispezionabile dopo il box.
    #[tokio::test]
    async fn fake_records_sent_via_shared_log_and_replays_incoming() {
        // `log` e' CONDIVISO: resta ispezionabile dal test anche dopo che il transport
        // viene boxed dentro l'host (Task 5). E' questo che rende onesti i test della lifecycle.
        let log: SentLog = Default::default();
        let mut tx = FakePluginTransport::new(log.clone(), vec![
            PluginToHost::Ready { name: "ping".into(), protocol_version: 1 },
        ]);
        tx.send(HostToPlugin::Init {
            protocol_version: 1,
            config: serde_json::Value::Null,
            storage_dir: "x".into(),
        }).await.unwrap();

        let got = tx.recv().await.unwrap();
        assert!(matches!(got, Some(PluginToHost::Ready { .. })));
        assert!(tx.recv().await.unwrap().is_none()); // esaurito -> None (EOF)
        assert_eq!(log.lock().unwrap().len(), 1);
        assert!(matches!(log.lock().unwrap()[0], HostToPlugin::Init { .. }));
    }
}
