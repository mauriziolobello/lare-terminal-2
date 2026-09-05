//! PluginHost: scoperta → spawn (eager/lazy) → Init → (vita) → Deinit.
//!
//! ## Task 5: transport split + pump task
//! Il design precedente (Fase 0) usava un `Box<dyn PluginTransport>` combinato.
//! Il problema: se il transport fosse dietro un `Mutex`, il task pump avrebbe
//! dovuto tenere il lock per l'intera durata di `recv()`, bloccando tutti gli
//! `send()` dell'host. Deadlock garantito.
//!
//! Soluzione adottata (Task 5):
//! - L'host possiede la metà "writer" di ogni plugin (HashMap<id, Box<dyn PluginWriter>>).
//!   Inviare un messaggio = `writers[id].send(msg).await` senza alcun lock.
//! - Un task pump separato possiede la metà "reader" (non c'è lock: proprietà esclusiva).
//!   Legge in loop e traduce i messaggi plugin→UI via `plugin_msg_to_server`.
//! - Il pump invia al client WS tramite `server_tx: UnboundedSender<ServerMsg>`,
//!   clonato al momento dello spawn (può essere `None` per i plugin eager avviati
//!   prima che la connessione WS sia stabilita; quei messaggi sono scartati silenziosamente).
//!
//! ## Design (SOLID — SRP + DIP)
//! `PluginHost::start` riceve la lista dei plugin già scoperti (NON fa discovery da sé)
//! e una factory `make` che crea le due metà del transport per ciascuno.
//! In produzione: `spawn_plugin`; nei test: `fake_transport`.
//!
//! ## Ciclo di vita (Task 5)
//! ```text
//! start():
//!   per ogni plugin Eager:
//!     make(p) -> (writer, reader)
//!     writer.send(Init)
//!     reader.recv() -> DEVE essere Ready (else drop, non entra in writers)
//!     tokio::spawn(pump_task(reader, server_tx.clone()))
//!     writers.insert(id, writer)
//!
//! set_server_tx(tx):  // chiamato da ws.rs dopo la connessione WS
//!   server_tx = Some(tx)  // i pump futuri catturano questo tx
//!
//! activate(id, make):  // lazy-spawn al primo comando slash
//!   if id non in writers: spawn_and_handshake(p, make) — come start
//!   genera window_id, send Activate, windows.insert(wid, id)
//!
//! shutdown():
//!   per ogni writer: send(Deinit)
//!   writers.clear(); windows.clear()
//! ```

use super::discovery::DiscoveredPlugin;
use super::spawn_policy::{spawn_policy, SpawnPolicy};
use super::transport::{plugin_msg_to_server, PluginReader, PluginWriter};
use plugin_protocol::{HostToPlugin, PluginToHost};
use protocol::ServerMsg;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::Mutex;

/// Host del sistema plugin: gestisce il lifecycle (Init/Deinit), il lazy-spawn
/// e il routing degli eventi UI verso i plugin attivi.
///
/// ## SOLID
/// - SRP: l'host gestisce solo il lifecycle e il routing; I/O è delegato ai writer/reader.
/// - DIP: dipende dai trait `PluginWriter`/`PluginReader`, non dagli impl concreti.
///   In produzione `make` = `spawn_plugin`; nei test = `fake_transport`.
pub struct PluginHost {
    /// Writer di ogni plugin attivo, indicizzati per `plugin_id`.
    /// Il pump task possiede il reader corrispondente; l'host possiede il writer.
    /// Separazione esplicita per evitare il deadlock reader-vs-sender descritto sopra.
    writers: HashMap<String, Box<dyn PluginWriter>>,

    /// Plugin "lazy" scoperti all'avvio ma non ancora spawnati.
    /// Al primo `activate(id)` il plugin viene spawnato con lo stesso Ready-gate degli eager.
    discovered_lazy: Vec<DiscoveredPlugin>,

    /// Registro delle finestre aperte: `window_id` → `plugin_id`.
    /// `Arc<Mutex<..>>` (non un semplice `HashMap`) perché il pump task (vedi
    /// `spawn_and_handshake`) deve poterci scrivere SENZA il lock esterno su
    /// `PluginHost` — esattamente lo stesso motivo per cui `writers`/`reader`
    /// sono già separati (vedi module doc): il pump gira fuori dal lock
    /// `Arc<Mutex<PluginHost>>` per non rischiare un deadlock reader-vs-sender.
    /// Popolato da `activate` (prima finestra, richiesta da un comando slash)
    /// E dal pump task stesso (finestre ulteriori che il plugin apre di
    /// propria iniziativa, es. una dialog — 2026-07-19, vedi
    /// `Docs/superpowers/specs/2026-07-19-crypto-plugin-design.md` §7bis).
    /// Usato da `route_ui_event` e `forget_window`.
    pub windows: Arc<Mutex<HashMap<u64, String>>>,

    /// Prossimo window_id da assegnare. Parte da 1; non viene mai riusato.
    next_window_id: u64,

    /// Radice per lo storage per-plugin: `<storage_root>/<plugin_id>`.
    /// Conservata qui perché `activate` la usa anche dopo che `start` è ritornato.
    storage_root: PathBuf,

    /// Canale verso il client WS. Impostato da ws.rs dopo la connessione.
    /// I pump task che girano PRIMA di `set_server_tx` ricevono `None` e scartano
    /// silenziosamente i messaggi (Ready/Log durante handshake → None, nessuna perdita).
    server_tx: Option<UnboundedSender<ServerMsg>>,
}

impl PluginHost {
    /// Avvia gli EAGER: per ognuno esegue lo spawn, invia `Init`, attende `Ready`,
    /// avvia il pump task. I plugin LAZY vengono salvati in `discovered_lazy`.
    ///
    /// # Arguments
    /// - `plugins`: la lista di plugin scoperti (da `discovery::discover`).
    /// - `make`: factory che ritorna `(Box<dyn PluginWriter>, Box<dyn PluginReader>)`.
    ///   In produzione = `spawn_plugin`; nei test = `fake_transport`.
    /// - `storage_root`: dir radice per lo storage privato per-plugin.
    pub async fn start<F>(plugins: Vec<DiscoveredPlugin>, mut make: F, storage_root: &Path) -> Self
    where
        F: FnMut(&DiscoveredPlugin) -> std::io::Result<(Box<dyn PluginWriter>, Box<dyn PluginReader>)>,
    {
        let mut host = Self {
            writers: HashMap::new(),
            discovered_lazy: Vec::new(),
            windows: Arc::new(Mutex::new(HashMap::new())),
            next_window_id: 1,
            storage_root: storage_root.to_path_buf(),
            server_tx: None, // impostato da ws.rs via set_server_tx dopo la connessione
        };

        for p in &plugins {
            if spawn_policy(&p.manifest.triggers) != SpawnPolicy::Eager {
                // Plugin lazy: salvalo per il lazy-spawn al primo `activate`.
                host.discovered_lazy.push(p.clone());
                continue;
            }
            // Eager: spawn + handshake + pump task.
            host.spawn_and_handshake(p, &mut make).await;
        }

        host
    }

    /// Imposta il canale WS verso cui il pump task inoltrerà i ServerMsg dei plugin.
    ///
    /// Chiamato da ws.rs subito dopo la creazione di `out_tx` (il canale persistente).
    /// I pump task attivati DOPO questa chiamata riceveranno una copia di `tx` e potranno
    /// forwarding i messaggi plugin→UI. I pump già in esecuzione (eager) hanno `None`
    /// e scartano silenziosamente (comportamento by-design, documentato nel module doc).
    pub fn set_server_tx(&mut self, tx: UnboundedSender<ServerMsg>) {
        self.server_tx = Some(tx);
    }

    /// Numero di plugin attualmente attivi (con writer registrato).
    /// Usato dai test e dall'e2e per verificare il ciclo di vita senza esporre `writers`.
    pub fn running_count(&self) -> usize {
        self.writers.len()
    }

    /// Attiva un plugin per finestra: lazy-spawna se necessario, genera un `window_id`,
    /// invia `Activate{window_id, args:Null}` e registra la finestra.
    ///
    /// ## Comportamento
    /// 1. Se il plugin non è in `writers`, lo cerca in `discovered_lazy` e lo spawna
    ///    (`spawn_and_handshake` — stessa logica Init→Ready degli eager).
    /// 2. Genera `window_id = next_window_id` (incrementa il contatore).
    /// 3. Invia `Activate { window_id, args: Null }` al plugin.
    /// 4. Registra `windows[window_id] = plugin_id`.
    /// 5. Ritorna `Some(window_id)`.
    ///
    /// Ritorna `None` se il plugin è sconosciuto, o se lo spawn/Ready fallisce.
    pub async fn activate<F>(
        &mut self,
        plugin_id: &str,
        make: &mut F,
    ) -> Option<u64>
    where
        F: FnMut(&DiscoveredPlugin) -> std::io::Result<(Box<dyn PluginWriter>, Box<dyn PluginReader>)>,
    {
        // Lazy-spawn se il plugin non è ancora in writers.
        if !self.writers.contains_key(plugin_id) {
            let lazy_pos = self
                .discovered_lazy
                .iter()
                .position(|p| p.manifest.id == plugin_id)?;
            let discovered = self.discovered_lazy[lazy_pos].clone();
            // spawn_and_handshake inserisce il writer in self.writers se ha successo.
            self.spawn_and_handshake(&discovered, make).await?;
        }

        // Genera il window_id (contatore monotono, mai riusato).
        let window_id = self.next_window_id;
        self.next_window_id += 1;
        // Non rollback del window_id su errore: un invio fallito è raro e il contatore
        // avanzato evita race condition con future attivazioni concorrenti.

        // Invia Activate al plugin.
        let writer = self.writers.get_mut(plugin_id)?;
        if let Err(e) = writer
            .send(HostToPlugin::Activate {
                window_id,
                args: serde_json::Value::Null,
            })
            .await
        {
            eprintln!("[plugins] Activate '{plugin_id}' fallito: {e}");
            // I1 FIX (robustezza rispawn): rimuoviamo il writer morto da `writers`.
            // Senza questa rimozione, `writers.contains_key(plugin_id)` continuerebbe a
            // ritornare `true`, saltando il branch lazy-spawn → tutte le `activate` future
            // fallirebbero ricicando lo stesso writer rotto all'infinito.
            // Con la rimozione, la prossima `activate` entra nel branch spawn e ricrea
            // il plugin da zero (vero "rispawn automatico" dopo un crash post-handshake).
            // Nota: `windows` non va pulito perché la registrazione window avviene DOPO
            // questa send (riga `self.windows.insert(...)` che segue), quindi niente entry
            // spuri possono essere presenti per questo window_id fallito.
            self.writers.remove(plugin_id);
            return None;
        }

        // Registra la finestra: route_ui_event la usa per trovare il plugin destinatario.
        // Viene registrata SOLO dopo la send riuscita — mai per window_id di send falliti.
        self.windows.lock().await.insert(window_id, plugin_id.to_string());
        Some(window_id)
    }

    /// Inoltra un evento UI (clic, input) al plugin che possiede la finestra `window_id`.
    ///
    /// Se la finestra è sconosciuta (già chiusa o mai aperta) è un no-op silenzioso.
    /// Il campo `value` è `Some(str)` per `<input>`/`<select>`, `None` per pulsanti.
    pub async fn route_ui_event(
        &mut self,
        window_id: u64,
        element_id: &str,
        value: Option<String>,
    ) {
        let Some(plugin_id) = self.windows.lock().await.get(&window_id).cloned() else { return };

        // I1 FIX (robustezza rispawn): scoping esplicito del borrow su `writers`.
        // Estraiamo prima il risultato del send (il borrow di `writer` termina qui),
        // poi — fuori dal borrow — rimuoviamo il writer dalla mappa se il send è fallito.
        // Senza lo scope esplicito, il borrow checker non ci lascerebbe chiamare
        // `self.writers.remove(...)` mentre `writer` è ancora in vita.
        let send_result = match self.writers.get_mut(&plugin_id) {
            Some(writer) => {
                writer
                    .send(HostToPlugin::UiEvent {
                        window_id,
                        element_id: element_id.to_string(),
                        value,
                    })
                    .await
            }
            // Nessun writer per questo plugin_id: niente da inviare (no-op silenzioso).
            None => return,
        };
        // Il borrow di `writer` è terminato qui (fine del match); possiamo ora accedere
        // a `self.writers` mutabilmente per rimuovere il writer morto se necessario.
        if let Err(e) = send_result {
            eprintln!("[plugins] UiEvent '{plugin_id}' (window {window_id}) fallito: {e} — writer rimosso");
            // Rimuovi il writer rotto: la prossima attivazione ripartirà dal lazy-spawn.
            self.writers.remove(&plugin_id);
        }
    }

    /// Rimuove la registrazione di una finestra (chiamato quando l'UI chiude la finestra).
    ///
    /// No-op se `window_id` è sconosciuto.
    pub async fn forget_window(&mut self, window_id: u64) {
        self.windows.lock().await.remove(&window_id);
    }

    /// Invia `Deinit` a tutti i plugin attivi e svuota i writer.
    ///
    /// I `ChildPluginReader` (posseduti dai pump task) escono dal loop su EOF
    /// (lo stdin del plugin è chiuso al drop del writer). `kill_on_drop(true)` garantisce
    /// che nessun processo plugin rimanga orfano.
    pub async fn shutdown(&mut self) {
        for (id, writer) in &mut self.writers {
            if let Err(e) = writer.send(HostToPlugin::Deinit {}).await {
                eprintln!("[plugins] Deinit '{id}' fallito: {e}");
            }
        }
        // Drop dei writer: i ChildPluginReader dei pump task vedono EOF e terminano.
        self.writers.clear();
        self.windows.lock().await.clear();
    }
}

// ─── Helper privato ────────────────────────────────────────────────────────────

impl PluginHost {
    /// Spawna un plugin via factory `make`, esegue l'handshake Init→Ready, poi avvia
    /// il pump task che possiede il reader.
    ///
    /// Se l'handshake ha successo:
    /// - Il writer viene inserito in `self.writers[plugin_id]`.
    /// - Un `tokio::task` viene spawnato con il reader e un clone di `server_tx`.
    ///
    /// Ritorna `Some(())` se tutto è andato bene, `None` su errore (make, Init, Ready).
    ///
    /// ## Design
    /// - Funzione metodo (prende `&mut self`) perché inserisce direttamente in `writers`.
    /// - Il pump task cattura `server_tx.clone()` al momento dello spawn. Questo significa
    ///   che i pump degli eager (avviati in `start`, prima di `set_server_tx`) catturano
    ///   `None`; i pump dei lazy (attivati da `activate`, dopo `set_server_tx`) catturano
    ///   `Some(tx)`. Comportamento by-design: i plugin eager non mandano ShowWindow durante
    ///   lo startup normale.
    async fn spawn_and_handshake<F>(
        &mut self,
        p: &DiscoveredPlugin,
        make: &mut F,
    ) -> Option<()>
    where
        F: FnMut(&DiscoveredPlugin) -> std::io::Result<(Box<dyn PluginWriter>, Box<dyn PluginReader>)>,
    {
        // Chiama la factory per ottenere le due metà del transport.
        let (mut writer, mut reader) = match make(p) {
            Ok(pair) => pair,
            Err(e) => {
                eprintln!("[plugins] spawn '{}' fallito: {e}", p.manifest.id);
                return None;
            }
        };

        // Costruisce il path di storage privato per il plugin.
        // In Fase 3 questa cartella verrà creata se assente; per ora la passiamo come stringa.
        let storage_dir = self.storage_root.join(&p.manifest.id);

        // Invia Init: primo messaggio host → plugin, negozia protocollo e fornisce storage.
        let init = HostToPlugin::Init {
            protocol_version: p.manifest.protocol_version,
            config: serde_json::Value::Null, // Fase 3: config utente reale da file JSON
            storage_dir: storage_dir.to_string_lossy().into_owned(),
        };
        if let Err(e) = writer.send(init).await {
            eprintln!("[plugins] Init '{}' fallito: {e}", p.manifest.id);
            return None;
        }

        // Ready-gate: il PRIMO messaggio dal plugin DEVE essere `Ready`.
        // EOF o qualsiasi altro messaggio → non attivo.
        // Questo è il "costruttore validante" del plugin: entra in `writers`
        // solo se la negoziazione del protocollo ha avuto successo.
        //
        // I2 FIX (robustezza timeout): avvolgiamo recv() in un timeout di 5 secondi.
        // Senza timeout, un plugin che non invia mai `Ready` (crash pre-invio, deadlock
        // interno, slow startup) terrebbe bloccato questo metodo per sempre. Poiché
        // `spawn_and_handshake` è chiamata mentre l'host è in uso (lazy-spawn durante
        // la gestione di un comando WS), il blocco congelerebbe l'intera connessione.
        // Con il timeout, l'host rileva il plugin "sordo" entro 5s e lo scarta.
        // Il plugin sconosciuto non entra in `writers`, quindi un tentativo successivo
        // può riprovare il lazy-spawn (se il plugin è riapparso).
        let ready_result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            reader.recv(),
        )
        .await;

        match ready_result {
            // Timeout scaduto: il plugin non ha risposto entro 5 secondi.
            Err(_timeout) => {
                eprintln!(
                    "[plugins] '{}' non ha inviato Ready entro 5s — non avviato",
                    p.manifest.id
                );
                return None;
            }
            // Ready ricevuto entro il timeout: handshake riuscito.
            Ok(Ok(Some(PluginToHost::Ready { name, protocol_version }))) => {
                eprintln!(
                    "[plugins] '{}' Ready (name={name}, proto={protocol_version})",
                    p.manifest.id
                );
            }
            // Messaggio non-Ready: il plugin non rispetta il protocollo.
            Ok(Ok(other)) => {
                eprintln!(
                    "[plugins] '{}' non ha riportato Ready (msg={other:?}) — non avviato",
                    p.manifest.id
                );
                return None;
            }
            // Errore I/O durante il recv (pipe rotta, processo uscito, …).
            Ok(Err(e)) => {
                eprintln!(
                    "[plugins] '{}' recv Ready fallito: {e} — non avviato",
                    p.manifest.id
                );
                return None;
            }
        }

        // Avvia il pump task: possiede `reader` (nessun lock), traduce PluginToHost → ServerMsg.
        //
        // `server_tx` è clonato ORA: se siamo in `start()` (prima di `set_server_tx`),
        // il clone sarà `None` e i messaggi verranno scartati. Se siamo in `activate()`
        // (dopo `set_server_tx`), il clone sarà `Some(tx)` e i messaggi arriveranno al client.
        let server_tx = self.server_tx.clone();
        let pid = p.manifest.id.clone();
        // Dimensione iniziale dichiarata dal plugin nel manifest (o None): la catturiamo
        // qui, accanto a `pid`, per l'`async move` del pump. È `Copy` (vedi WindowSize),
        // quindi un semplice assegnamento la copia — nessun `.clone()` necessario.
        let window_hint = p.manifest.window;
        // Arc clone (economico) — il pump registra qui i window_id di OGNI
        // ShowWindow che osserva, non solo quello assegnato da `activate`
        // (2026-07-19, vedi doc comment sul campo `windows`).
        let windows = self.windows.clone();
        tokio::spawn(async move {
            loop {
                match reader.recv().await {
                    Ok(Some(pm)) => {
                        // Registra il window_id PRIMA di consumare `pm` (la riga sotto
                        // sposta `pm` dentro `plugin_msg_to_server`). Un blind insert è
                        // corretto anche per il window_id già registrato da `activate`
                        // (stesso plugin_id, overwrite idempotente) — non serve un
                        // controllo "se assente".
                        if let PluginToHost::ShowWindow { window_id, .. } = &pm {
                            windows.lock().await.insert(*window_id, pid.clone());
                        }
                        // Traduce il messaggio plugin→UI. Ready/Log mappano a None e vengono
                        // scartati; ShowWindow/Update/Close mappano a ServerMsg.
                        // `window_hint` è usato solo per ShowWindow → OpenPluginWindow.
                        if let Some(sm) = plugin_msg_to_server(pm, window_hint) {
                            if let Some(ref tx) = server_tx {
                                // Se il send fallisce, il client si è disconnesso.
                                // Il pump continua (il prossimo iter vedrà l'errore).
                                let _ = tx.send(sm);
                            }
                        }
                    }
                    // Ok(None) = EOF (plugin terminato); Err = I/O error.
                    // In entrambi i casi il pump non ha più niente da fare.
                    _ => {
                        eprintln!("[plugins] pump '{pid}': reader chiuso — pump task terminato");
                        break;
                    }
                }
            }
        });

        // Registra il writer: da qui l'host può inviare messaggi al plugin.
        self.writers.insert(p.manifest.id.clone(), writer);
        Some(())
    }
}

// ─── Test ─────────────────────────────────────────────────────────────────────
//
// I test usano `FakeWriter`/`FakeReader` via `fake_transport()`.
// Il `SentLog` condiviso permette di ispezionare i messaggi inviati DOPO il box+move.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::transport::{
        fake_transport, fake_transport_fail_after, fake_transport_hanging,
        SentLog, PluginWriter, PluginReader,
    };
    use plugin_protocol::{HostToPlugin, PluginManifest, PluginToHost, Triggers, WindowSize};
    use std::path::PathBuf;

    /// Costruisce un `DiscoveredPlugin` fittizio con id e trigger dati.
    /// `window: None` = plugin senza dimensione dichiarata (default host 480×360).
    fn discovered(id: &str, triggers: Triggers) -> DiscoveredPlugin {
        discovered_with_window(id, triggers, None)
    }

    /// Come `discovered`, ma consente di dichiarare una dimensione finestra nel
    /// manifest (per i test che verificano il pass-through manifest → OpenPluginWindow).
    fn discovered_with_window(
        id: &str,
        triggers: Triggers,
        window: Option<WindowSize>,
    ) -> DiscoveredPlugin {
        DiscoveredPlugin {
            manifest: PluginManifest {
                name: id.into(),
                id: id.into(),
                version: "1".into(),
                protocol_version: 1,
                triggers,
                window,
            },
            bin_path: PathBuf::from(format!("/fake/{id}")),
        }
    }

    /// Plugin eager (nessun trigger = Eager) che risponde Ready: deve finire in writers.
    /// Il log condiviso prova che Init è stato inviato per primo (test ONESTO).
    #[tokio::test]
    async fn eager_plugin_gets_init_and_reports_ready() {
        let log: SentLog = Default::default();
        let l = log.clone();
        let host = PluginHost::start(
            vec![discovered("ping", Triggers::default())],
            move |_p| {
                let (w, r) = fake_transport(l.clone(), vec![
                    PluginToHost::Ready { name: "ping".into(), protocol_version: 1 },
                ]);
                Ok((Box::new(w) as Box<dyn PluginWriter>, Box::new(r) as Box<dyn PluginReader>))
            },
            std::path::Path::new("/tmp/lare-plugins"),
        ).await;

        assert_eq!(host.running_count(), 1, "il plugin che riporta Ready deve essere attivo");
        // Test ONESTO: il log condiviso prova che Init è stato inviato per primo.
        // `FakeWriter::send` è sincrono → nessuna race condition sulle asserzioni.
        assert!(
            matches!(log.lock().unwrap()[0], HostToPlugin::Init { .. }),
            "Init deve essere il primo messaggio inviato al plugin"
        );
    }

    /// Plugin Lazy (solo command trigger) NON deve essere spawnato all'avvio.
    /// La factory fa panic se chiamata: verifica che non venga mai invocata.
    #[tokio::test]
    async fn lazy_plugin_is_not_spawned_eagerly() {
        let cmd = Triggers { command: Some("/calc".into()), interval: None };
        let host = PluginHost::start(
            vec![discovered("calc", cmd)],
            |_p| -> std::io::Result<(Box<dyn PluginWriter>, Box<dyn PluginReader>)> {
                panic!("un plugin lazy NON deve essere spawnato all'avvio")
            },
            std::path::Path::new("/tmp"),
        ).await;
        assert_eq!(host.running_count(), 0);
    }

    /// Plugin che non invia Ready come primo messaggio (EOF immediato)
    /// NON deve essere considerato attivo.
    #[tokio::test]
    async fn plugin_without_ready_is_not_running() {
        let log: SentLog = Default::default();
        let l = log.clone();
        let host = PluginHost::start(
            vec![discovered("dead", Triggers::default())],
            move |_p| {
                let (w, r) = fake_transport(l.clone(), vec![]); // nessun Ready = EOF
                Ok((Box::new(w) as Box<dyn PluginWriter>, Box::new(r) as Box<dyn PluginReader>))
            },
            std::path::Path::new("/tmp"),
        ).await;
        assert_eq!(host.running_count(), 0, "senza Ready il plugin non e' attivo");
    }

    /// `activate` su un plugin lazy: deve spawnarlo (se non già in writers),
    /// inviare `Init`, attendere `Ready`, poi inviare `Activate{window_id:1}`.
    /// Ritorna `Some(1)` (primo window_id).
    #[tokio::test]
    async fn activate_lazy_spawns_and_sends_activate() {
        let log: SentLog = Default::default();
        let l = log.clone();

        // `start` con un plugin lazy: la factory NON deve essere chiamata.
        let mut host = PluginHost::start(
            vec![discovered("counter", Triggers { command: Some("/counter".into()), interval: None })],
            |_p| -> std::io::Result<(Box<dyn PluginWriter>, Box<dyn PluginReader>)> {
                panic!("la factory NON deve essere chiamata durante start per un plugin lazy")
            },
            std::path::Path::new("/tmp"),
        ).await;
        assert_eq!(host.running_count(), 0, "plugin lazy: non attivo prima di activate");

        // `activate`: la factory STAVOLTA viene chiamata (spawn lazy al primo uso).
        let mut make = move |_p: &DiscoveredPlugin| {
            let (w, r) = fake_transport(
                l.clone(),
                vec![PluginToHost::Ready { name: "counter".into(), protocol_version: 1 }],
            );
            Ok((Box::new(w) as Box<dyn PluginWriter>, Box::new(r) as Box<dyn PluginReader>))
        };

        let wid = host.activate("counter", &mut make).await;
        assert_eq!(wid, Some(1), "activate deve ritornare Some(window_id)");

        // Il log deve contenere Init (spawn lazy) e Activate{window_id:1} (dopo Ready-gate).
        assert!(
            log.lock().unwrap().iter().any(|m| matches!(m, HostToPlugin::Init { .. })),
            "Init deve essere nel log (spawn lazy)"
        );
        assert!(
            log.lock().unwrap().iter().any(|m| matches!(m, HostToPlugin::Activate { window_id: 1, .. })),
            "Activate{{window_id:1}} deve essere nel log"
        );
    }

    /// Il pump task deve tradurre i messaggi Plugin→Host in ServerMsg e inoltrarli
    /// al canale WS impostato da `set_server_tx`.
    ///
    /// Questo è il test del "INTEGRATION crux" di Task 5: prova che l'intera
    /// catena pump → server_tx → client WS funzioni.
    ///
    /// ## Sequenza
    /// 1. `start` con plugin lazy (factory non chiamata).
    /// 2. `set_server_tx(tx)` — simula il wiring di ws.rs dopo la connessione.
    /// 3. `activate` — spawna il plugin con FakeReader che emette [Ready, ShowWindow{...}].
    ///    Il pump consuma Ready (handshake), poi ShowWindow → OpenPluginWindow → tx.
    /// 4. `rx.recv()` con timeout: deve ricevere esattamente `OpenPluginWindow{...}`.
    ///
    /// ## Onestà
    /// Se si commenta `let _ = tx.send(sm)` nel pump, questo test va in timeout
    /// (RED retroattivo che dimostra che il test vincola davvero il pump).
    #[tokio::test]
    async fn pump_forwards_show_window_to_server_tx() {
        use tokio::sync::mpsc::unbounded_channel;

        // Canale che simula il sink WS → riceve i ServerMsg dal pump.
        let (tx, mut rx) = unbounded_channel::<ServerMsg>();

        // Start con un plugin lazy (non spawnato ancora).
        let mut host = PluginHost::start(
            vec![discovered("counter", Triggers { command: Some("/counter".into()), interval: None })],
            |_p| -> std::io::Result<(Box<dyn PluginWriter>, Box<dyn PluginReader>)> {
                panic!("lazy: non deve essere spawnato in start")
            },
            std::path::Path::new("/tmp"),
        ).await;

        // Simula il wiring di ws.rs: set_server_tx deve essere chiamato PRIMA di activate
        // affinché il pump catturi Some(tx) e possa forwardare i messaggi.
        host.set_server_tx(tx);

        let log: SentLog = Default::default();
        let l = log.clone();
        let mut make = move |_p: &DiscoveredPlugin| {
            // FakeReader: prima Ready (consumato dall'handshake), poi ShowWindow{1}
            // (consumato dal pump dopo l'handshake).
            let (w, r) = fake_transport(
                l.clone(),
                vec![
                    PluginToHost::Ready { name: "counter".into(), protocol_version: 1 },
                    PluginToHost::ShowWindow {
                        window_id: 1,
                        title: "Titolo".to_string(),
                        html: "<b>corpo</b>".to_string(),
                    },
                ],
            );
            Ok((Box::new(w) as Box<dyn PluginWriter>, Box::new(r) as Box<dyn PluginReader>))
        };

        // activate spawna il plugin, esegue l'handshake Init/Ready, avvia il pump.
        let wid = host.activate("counter", &mut make).await;
        assert_eq!(wid, Some(1), "activate deve tornare Some(1)");

        // Il pump task (asincrono, spawned da spawn_and_handshake) legge ShowWindow
        // dal FakeReader e lo traduce in OpenPluginWindow → server_tx.
        // Usiamo un timeout di 1 secondo: se il pump non invia entro 1s, il test fallisce.
        let msg = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            rx.recv(),
        )
        .await
        .expect("timeout: il pump non ha inviato OpenPluginWindow entro 1 secondo")
        .expect("channel chiuso: nessun messaggio dal pump");

        assert_eq!(
            msg,
            ServerMsg::OpenPluginWindow {
                window_id: 1,
                title: "Titolo".to_string(),
                html: "<b>corpo</b>".to_string(),
                // Il plugin "counter" non dichiara `window` nel manifest → None.
                width: None,
                height: None,
            },
            "il pump deve tradurre ShowWindow → OpenPluginWindow e inviarlo via server_tx"
        );
    }

    /// **B4 end-to-end** — una dimensione dichiarata nel manifest deve raggiungere
    /// il `ServerMsg::OpenPluginWindow` prodotto dal pump. È la prova che il campo
    /// `manifest.window` viene catturato dal pump task e inoltrato con i valori esatti.
    ///
    /// Stessa struttura di `pump_forwards_show_window_to_server_tx`, ma il plugin
    /// dichiara `window: Some(960×620)`.
    #[tokio::test]
    async fn pump_forwards_manifest_window_size() {
        use tokio::sync::mpsc::unbounded_channel;

        let (tx, mut rx) = unbounded_channel::<ServerMsg>();

        // Plugin lazy CHE DICHIARA una dimensione finestra nel manifest.
        let mut host = PluginHost::start(
            vec![discovered_with_window(
                "lc",
                Triggers { command: Some("/lc".into()), interval: None },
                Some(WindowSize { width: 960.0, height: 620.0 }),
            )],
            |_p| -> std::io::Result<(Box<dyn PluginWriter>, Box<dyn PluginReader>)> {
                panic!("lazy: non deve essere spawnato in start")
            },
            std::path::Path::new("/tmp"),
        ).await;

        host.set_server_tx(tx);

        let log: SentLog = Default::default();
        let l = log.clone();
        let mut make = move |_p: &DiscoveredPlugin| {
            let (w, r) = fake_transport(
                l.clone(),
                vec![
                    PluginToHost::Ready { name: "lc".into(), protocol_version: 1 },
                    PluginToHost::ShowWindow {
                        window_id: 1,
                        title: "Lare Commander".to_string(),
                        html: "<div/>".to_string(),
                    },
                ],
            );
            Ok((Box::new(w) as Box<dyn PluginWriter>, Box::new(r) as Box<dyn PluginReader>))
        };

        let wid = host.activate("lc", &mut make).await;
        assert_eq!(wid, Some(1));

        let msg = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("timeout: nessun OpenPluginWindow entro 1s")
            .expect("channel chiuso");

        assert_eq!(
            msg,
            ServerMsg::OpenPluginWindow {
                window_id: 1,
                title: "Lare Commander".to_string(),
                html: "<div/>".to_string(),
                // La dimensione dichiarata nel manifest deve arrivare fin qui.
                width: Some(960.0),
                height: Some(620.0),
            },
            "la dimensione del manifest deve raggiungere OpenPluginWindow via pump"
        );
    }

    /// **§7bis** — un plugin che apre una SECONDA finestra di propria
    /// iniziativa (non richiesta da un comando slash, quindi il suo
    /// window_id non passa mai da `activate`) deve comunque poter ricevere
    /// UiEvent per quella finestra. Prova il fix del pump task: registra
    /// `windows` per OGNI ShowWindow osservato, non solo per quello di
    /// `activate` (Docs/superpowers/specs/2026-07-19-crypto-plugin-design.md §7bis).
    #[tokio::test]
    async fn pump_registers_window_for_unsolicited_show_window() {
        use tokio::sync::mpsc::unbounded_channel;

        let (tx, mut rx) = unbounded_channel::<ServerMsg>();

        let mut host = PluginHost::start(
            vec![discovered("crypto", Triggers { command: Some("/crypto".into()), interval: None })],
            |_p| -> std::io::Result<(Box<dyn PluginWriter>, Box<dyn PluginReader>)> {
                panic!("lazy: non deve essere spawnato da start")
            },
            std::path::Path::new("/tmp"),
        ).await;
        host.set_server_tx(tx);

        let log: SentLog = Default::default();
        let l = log.clone();
        let mut make = move |_p: &DiscoveredPlugin| {
            let (w, r) = fake_transport(
                l.clone(),
                vec![
                    PluginToHost::Ready { name: "crypto".into(), protocol_version: 1 },
                    // Seconda finestra APERTA DAL PLUGIN, window_id=42, MAI passato da activate.
                    PluginToHost::ShowWindow {
                        window_id: 42,
                        title: "Parametri".to_string(),
                        html: "<div/>".to_string(),
                    },
                ],
            );
            Ok((Box::new(w) as Box<dyn PluginWriter>, Box::new(r) as Box<dyn PluginReader>))
        };

        // activate assegna window_id=1 (finestra principale) — la seconda (42)
        // arriva dal reader DOPO Ready, consumata dal pump come un messaggio normale.
        let wid = host.activate("crypto", &mut make).await;
        assert_eq!(wid, Some(1));

        // Attende che il pump abbia processato ShowWindow{42,...} — il suo arrivo
        // su rx prova che il pump l'ha già visto e (col fix) già registrato in
        // `windows` prima di inoltrarlo.
        let msg = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("timeout: nessun OpenPluginWindow per la seconda finestra")
            .expect("channel chiuso");
        assert_eq!(
            msg,
            ServerMsg::OpenPluginWindow {
                window_id: 42,
                title: "Parametri".to_string(),
                html: "<div/>".to_string(),
                width: None,
                height: None,
            }
        );

        // DISCRIMINANTE: route_ui_event su window_id=42 deve raggiungere il
        // plugin (pre-fix: scartato in silenzio, 42 non era mai in `windows`).
        host.route_ui_event(42, "apply", Some("3".to_string())).await;
        assert!(
            log.lock().unwrap().iter().any(|m| matches!(
                m,
                HostToPlugin::UiEvent { window_id: 42, element_id, .. } if element_id == "apply"
            )),
            "UiEvent per la finestra non richiesta da activate deve raggiungere il plugin"
        );
    }

    // ─── I1: robustezza rispawn dopo writer morto ──────────────────────────────────
    //
    // Scenario: l'handshake Init→Ready riesce (plugin entra in `writers`), ma il successivo
    // `Activate` fallisce perché il plugin è crashato nel frattempo (broken pipe).
    // Bug pre-fix: il writer morto resta in `writers` → `contains_key` è true → il plugin
    // non viene mai rispawnato; tutte le attivazioni successive falliscono all'infinito.
    // Fix: in `activate`, quando `writer.send(Activate)` ritorna `Err`, rimuovere il writer
    // da `writers` prima di ritornare `None`, così la prossima `activate` entra nel branch
    // lazy-spawn e ricrea il plugin.

    /// **I1 RED test** — verifica che un writer morto venga rimosso da `writers`
    /// dopo un fallimento di `Activate`, consentendo il rispawn al tentativo successivo.
    ///
    /// ## Discriminante onesto
    /// `activate` ritorna già `None` pre-fix (il `?` sulla riga `writer.send(..).await`
    /// esiste già). Il caso che cambia è `running_count()`: pre-fix il dead writer resta
    /// → `running_count() == 1`; post-fix viene rimosso → `running_count() == 0`.
    ///
    /// Il FakeWriter con `ok_sends = 1` fa: Init ok (send #1), Activate Err (send #2).
    #[tokio::test]
    async fn activate_dead_writer_is_removed_so_respawn_is_possible() {
        let log: SentLog = Default::default();
        let l = log.clone();

        // Start con plugin lazy: la factory NON viene chiamata da start().
        let mut host = PluginHost::start(
            vec![discovered("counter", Triggers { command: Some("/counter".into()), interval: None })],
            |_p| -> std::io::Result<(Box<dyn PluginWriter>, Box<dyn PluginReader>)> {
                panic!("lazy: non deve essere spawnato da start")
            },
            std::path::Path::new("/tmp"),
        ).await;

        // Prima activate: writer che fallisce su Activate (send #1 = Init ok, send #2 = Err).
        // Il reader restituisce Ready così l'handshake riesce e il writer entra in `writers`;
        // poi la `Activate` send fallisce simulando un crash post-handshake.
        let mut make = move |_p: &DiscoveredPlugin| {
            let (w, r) = fake_transport_fail_after(
                l.clone(),
                1, // ok_sends = 1: Init riesce, Activate fallisce
                vec![PluginToHost::Ready { name: "counter".into(), protocol_version: 1 }],
            );
            Ok((Box::new(w) as Box<dyn PluginWriter>, Box::new(r) as Box<dyn PluginReader>))
        };

        let result = host.activate("counter", &mut make).await;
        assert_eq!(result, None, "activate deve ritornare None se Activate send fallisce");

        // DISCRIMINANTE (va RED pre-fix, GREEN post-fix):
        // il writer morto NON deve restare in `writers` — deve essere rimosso
        // affinché la prossima `activate` possa fare il lazy-spawn da capo.
        assert_eq!(
            host.running_count(),
            0,
            "il writer morto deve essere rimosso da writers (pre-fix: running_count==1)"
        );
    }

    // ─── I2: timeout sul Ready-gate ────────────────────────────────────────────────
    //
    // Scenario: il plugin viene spawnato, Init viene inviato, ma il plugin non invia mai
    // `Ready` (blocco, crash pre-invio, deadlock interno). Senza un timeout, `recv()` aspetta
    // per sempre tenendo il mutex dell'host bloccato — tutti i comandi WS si bloccano.
    // Fix: `tokio::time::timeout(5s, reader.recv())` in `spawn_and_handshake`.
    //
    // ## Nota RED
    // Pre-fix: questo test si blocca per sempre (il `pending()` in FakeReader non risolve
    // e non c'è un timer tokio da avanzare). Questo è il bug: l'host freeze.
    // Post-fix con `start_paused = true`: tokio vede il `timeout(5s)` come unico future
    // in attesa, avanza il tempo virtuale di 5s, il timeout scatta, il test completa
    // istantaneamente. Verificare GREEN basta: il RED era il freeze.

    /// **I2 test** — verifica che `spawn_and_handshake` ritorni `None` entro 5s se il
    /// plugin non invia `Ready`, e che non lasci il plugin in `writers`.
    ///
    /// Usa `start_paused = true` (tokio auto-avanza il tempo virtuale al prossimo timer):
    /// il timeout di 5s scatta istantaneamente — la suite non subisce rallentamenti reali.
    #[tokio::test(start_paused = true)]
    async fn spawn_and_handshake_times_out_if_plugin_never_sends_ready() {
        let log: SentLog = Default::default();
        let l = log.clone();

        // Start con plugin lazy.
        let mut host = PluginHost::start(
            vec![discovered("hung", Triggers { command: Some("/hung".into()), interval: None })],
            |_p| -> std::io::Result<(Box<dyn PluginWriter>, Box<dyn PluginReader>)> {
                panic!("lazy: non deve essere spawnato da start")
            },
            std::path::Path::new("/tmp"),
        ).await;

        // Il reader è congelato: recv() → pending() → non ritorna mai.
        // Il writer funziona normalmente (Init viene inviato correttamente).
        let mut make = move |_p: &DiscoveredPlugin| {
            let (w, r) = fake_transport_hanging(l.clone());
            Ok((Box::new(w) as Box<dyn PluginWriter>, Box::new(r) as Box<dyn PluginReader>))
        };

        // Pre-fix: questa riga si blocca per sempre (bug).
        // Post-fix: il timeout(5s) in spawn_and_handshake scatta; tokio avanza il tempo
        // virtuale di 5s istantaneamente (start_paused = true); activate ritorna None.
        let result = host.activate("hung", &mut make).await;

        assert_eq!(result, None, "activate deve ritornare None se Ready non arriva entro 5s");
        assert_eq!(host.running_count(), 0, "plugin senza Ready non deve essere in writers");
    }

    /// **I1b RED test** — verifica che `route_ui_event` rimuova il writer morto da `writers`
    /// quando `send(UiEvent)` ritorna `Err` (plugin crashato dopo l'attivazione).
    ///
    /// ## Contatore dei send
    /// - send #1: `Init` (in `spawn_and_handshake`) → ok
    /// - send #2: `Activate` → ok
    /// - send #3: `UiEvent` → Err (broken pipe simulata)
    ///
    /// `ok_sends = 2` significa: i primi 2 send riescono, il terzo fallisce.
    ///
    /// ## Discriminante
    /// `running_count() == 0` dopo `route_ui_event`: pre-fix il writer resta in `writers`
    /// (→ `running_count() == 1`); post-fix viene rimosso (→ `running_count() == 0`).
    #[tokio::test]
    async fn route_ui_event_removes_dead_writer_so_respawn_is_possible() {
        let log: SentLog = Default::default();
        let l = log.clone();

        // Start con plugin lazy.
        let mut host = PluginHost::start(
            vec![discovered("counter", Triggers { command: Some("/counter".into()), interval: None })],
            |_p| -> std::io::Result<(Box<dyn PluginWriter>, Box<dyn PluginReader>)> {
                panic!("lazy: non deve essere spawnato da start")
            },
            std::path::Path::new("/tmp"),
        ).await;

        // Writer che tollera 2 send (Init + Activate ok), poi fallisce (UiEvent = Err).
        let mut make = move |_p: &DiscoveredPlugin| {
            let (w, r) = fake_transport_fail_after(
                l.clone(),
                2, // send #1 Init ok, send #2 Activate ok, send #3 UiEvent → Err
                vec![PluginToHost::Ready { name: "counter".into(), protocol_version: 1 }],
            );
            Ok((Box::new(w) as Box<dyn PluginWriter>, Box::new(r) as Box<dyn PluginReader>))
        };

        // Activate riuscita: Init+Ready+Activate sono tutti ok.
        let wid = host.activate("counter", &mut make).await.expect("activate deve ritornare Some(wid)");
        assert_eq!(host.running_count(), 1, "dopo activate riuscita il plugin deve essere in writers");

        // UiEvent: send #3 → Err → il writer deve essere rimosso.
        host.route_ui_event(wid, "inc", None).await;

        // DISCRIMINANTE (va RED pre-fix, GREEN post-fix):
        // il writer morto NON deve restare in `writers`.
        assert_eq!(
            host.running_count(),
            0,
            "il writer morto deve essere rimosso da writers dopo UiEvent Err (pre-fix: running_count==1)"
        );
    }

    /// Dopo `shutdown()`, Deinit deve essere inviato a ogni plugin attivo
    /// e `writers` deve essere svuotato.
    #[tokio::test]
    async fn shutdown_sends_deinit() {
        let log: SentLog = Default::default();
        let l = log.clone();
        let mut host = PluginHost::start(
            vec![discovered("ping", Triggers::default())],
            move |_p| {
                let (w, r) = fake_transport(l.clone(), vec![
                    PluginToHost::Ready { name: "ping".into(), protocol_version: 1 },
                ]);
                Ok((Box::new(w) as Box<dyn PluginWriter>, Box::new(r) as Box<dyn PluginReader>))
            },
            std::path::Path::new("/tmp"),
        ).await;

        host.shutdown().await;

        assert_eq!(host.running_count(), 0, "svuotato dopo Deinit");
        assert!(
            log.lock().unwrap().iter().any(|m| matches!(m, HostToPlugin::Deinit {})),
            "Deinit deve essere inviato a ogni plugin attivo"
        );
    }
}
