//! # local_confirm — gate di conferma per la UI locale (per-tool)
//!
//! Estende ADR-007 (Docs/06-decisions.md) oltre il solo canale Telegram: alcuni
//! tool dell'AI possono essere marcati "sensibili" e richiedere una conferma
//! esplicita dell'utente **anche dalla UI locale**, che oggi è autonoma su tutto
//! (`confirmer: None`). Design:
//! `Docs/superpowers/specs/2026-07-15-local-tool-confirm-gate-design.md`.
//!
//! ## Meccanismo
//! Stesso principio del round-trip poll↔conferma di `telegram::confirm`, ma sul
//! canale WS locale già persistente (niente polling): `LocalUiConfirmer::confirm`
//! manda `ServerMsg::ToolConfirmRequest{id, commands}` e attende (con timeout) la
//! risposta `ClientMsg::ToolConfirmResponse{id, accept}` tramite un oneshot
//! registrato in [`PendingConfirms`] — la stessa mappa che il reader loop di
//! `ws.rs` consulta quando arriva la risposta del client.
//!
//! ## `SENSITIVE_TOOLS`
//! Prima voce reale: i cinque tool del canale `nmap` che eseguono attivamente
//! uno scan sul target scelto dall'AI (`nmap_quick_scan`, `nmap_os_detect`,
//! `nmap_version_scan`, `nmap_host_discovery`, `nmap_vuln_scan` — vedi
//! `Docs/superpowers/specs/2026-07-16-mcp-nmap-design.md` e il piano
//! `2026-07-18-nmap-scan-variants.md` per le tre varianti aggiunte dopo).
//! Seconda voce: `run_routine` (`Docs/superpowers/specs/2026-08-03-routines-repository-design.md`) — a differenza di
//! `run_in_session`, il cui testo completo è già il chunk di trasparenza,
//! una routine si invoca per nome: il suo corpo non è ri-mostrato a ogni
//! esecuzione, stesso principio di rischio "azione non pienamente visibile
//! dal solo nome" già alla base della lista.
//! Terza voce: `save_routine` (Docs/superpowers/specs/2026-08-05-save-routine-design.md)
//! — passa da una superficie di conferma DEDICATA (`confirm_routine_save`, non il
//! `confirm` generico usato da tutto il resto di questa lista): il corpo dello script
//! che sta per essere scritto su disco va rivisto per intero, non riassunto in una
//! riga di banner.
//! Per ogni altro tool (`run_in_session`, `open_target`, `search_routines`, …)
//! `should_gate` resta `false` — nessun comportamento cambia fuori da questi.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use protocol::ServerMsg;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::{oneshot, Mutex};

use crate::ai_adapter::ToolConfirmer;

/// Nomi dei tool che richiedono conferma esplicita anche in locale (vedi
/// doc-comment del modulo).
const SENSITIVE_TOOLS: &[&str] = &[
    "nmap_quick_scan",
    "nmap_os_detect",
    "nmap_version_scan",
    "nmap_host_discovery",
    "nmap_vuln_scan",
    "run_routine",
    "save_routine",
];

/// Registro condiviso `id opaco → mittente oneshot` per le conferme in sospeso.
///
/// Cloneabile (wrappa un `Arc<Mutex<...>>`): un'istanza vive per connessione WS,
/// condivisa fra il loop principale di `ws.rs` (che risolve su
/// `ClientMsg::ToolConfirmResponse`) e ogni `LocalUiConfirmer` costruito nei task
/// spawnati per i singoli comandi.
#[derive(Clone, Default)]
pub(crate) struct PendingConfirms {
    inner: Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>>,
}

impl PendingConfirms {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Registra `id` come in attesa di risposta; ritorna il receiver da attendere.
    pub(crate) async fn register(&self, id: String) -> oneshot::Receiver<bool> {
        let (tx, rx) = oneshot::channel();
        self.inner.lock().await.insert(id, tx);
        rx
    }

    /// Risolve `id` con `accept`, se ancora in sospeso. Ritorna `true` se un
    /// pending c'era davvero (utile nei test); un id sconosciuto o già risolto è
    /// un no-op silenzioso — stesso principio "risposta tardiva scartata" già in
    /// uso altrove in `ws.rs` (es. `CancelCommand` su un id già terminato).
    pub(crate) async fn resolve(&self, id: &str, accept: bool) -> bool {
        if let Some(tx) = self.inner.lock().await.remove(id) {
            let _ = tx.send(accept); // ignora errore: il receiver può essere già droppato (timeout)
            true
        } else {
            false
        }
    }
}

/// Gate di conferma per la UI locale (ADR-007, estensione per-tool).
///
/// A differenza di `TelegramConfirmer` (gate su tutto tranne `show_markdown`),
/// gateizza SOLO i tool in [`SENSITIVE_TOOLS`] (`should_gate`); tutti gli altri
/// (`run_in_session`, `open_target`) restano autonomi come oggi.
pub(crate) struct LocalUiConfirmer {
    out_tx: UnboundedSender<ServerMsg>,
    pending: PendingConfirms,
    timeout: Duration,
    /// Id del comando/turno AI corrente — lo stesso `id` che `ai_adapter::
    /// LlmAdapter::respond` usa per ogni `ServerMsg::Chunk` di quel turno.
    /// Usato SOLO da `confirm_routine_save` per emettere il chunk "anteprima
    /// aperta" associato alla bolla di comando giusta (review finale
    /// save_routine, finding I3 — quel chunk viveva prima incondizionato in
    /// `ai_adapter.rs`, ora è responsabilità di QUESTO confirmer perché è
    /// vero solo per la UI locale). `confirm()` non lo usa: il gate batch
    /// non emette messaggistica propria, solo il `ToolConfirmRequest`.
    command_id: String,
}

impl LocalUiConfirmer {
    pub(crate) fn new(
        out_tx: UnboundedSender<ServerMsg>,
        pending: PendingConfirms,
        timeout: Duration,
        command_id: String,
    ) -> Self {
        Self { out_tx, pending, timeout, command_id }
    }
}

#[async_trait]
impl ToolConfirmer for LocalUiConfirmer {
    async fn confirm(&self, commands: &str) -> bool {
        // Id opaco a 128 bit, NON derivato dal comando — stessa garanzia già
        // testata per `TelegramConfirmer` (nessun frammento del comando nell'id).
        let id = format!("{:032x}", rand::random::<u128>());
        let rx = self.pending.register(id.clone()).await;

        // `id.clone()` qui (non move): serve ancora nei due branch di cleanup
        // sotto (send fallito, timeout) per rimuovere l'entry da `pending`.
        if self
            .out_tx
            .send(ServerMsg::ToolConfirmRequest { id: id.clone(), commands: commands.to_string() })
            .is_err()
        {
            // Cleanup: senza questo, l'entry resterebbe orfana in `pending` (mai
            // risolta, mai droppata) perché il client è già disconnesso e non
            // manderà mai una `ToolConfirmResponse`. Idempotente (v. resolve()).
            self.pending.resolve(&id, false).await;
            return false; // client disconnesso
        }

        match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(accept)) => accept,
            Ok(Err(_)) => false, // sender droppato senza rispondere
            Err(_elapsed) => {
                // Timeout: nega, stesso comportamento di TelegramConfirmer. Cleanup
                // dell'entry orfana — vedi test `confirm_timeout_removes_orphaned_entry_from_registry`.
                self.pending.resolve(&id, false).await;
                false
            }
        }
    }

    async fn confirm_routine_save(&self, req: &crate::ai_adapter::RoutineSaveRequest) -> bool {
        // Chunk di trasparenza SOLO per questo confirmer (review finale
        // save_routine, finding I3): a differenza di `TelegramConfirmer`
        // (nessun override — eredita il default che appiattisce in
        // `confirm()`, nessuna finestra), la UI locale APRE DAVVERO una
        // finestra dedicata subito dopo — quindi SOLO qui dire "rivedila
        // nella finestra" è vero. Best-effort (`let _ =`): se il send fallisce
        // il client è già disconnesso, e il send del `RoutineSavePreview`
        // sotto fallirà a sua volta e negherà — stesso esito di prima.
        let _ = self.out_tx.send(ServerMsg::Chunk {
            id: self.command_id.clone(),
            content: format!(
                "\u{1F4C4} anteprima routine '{}' aperta \u{2014} rivedila nella finestra",
                req.name
            ),
        });

        // Stessa logica di `confirm()` sopra (id opaco, registra, timeout,
        // cleanup) ma manda `RoutineSavePreview` invece di
        // `ToolConfirmRequest`: la UI apre una finestra dedicata invece del
        // banner Sì/No nel cursore per questo id — vedi design doc §5.
        let id = format!("{:032x}", rand::random::<u128>());
        let rx = self.pending.register(id.clone()).await;

        if self
            .out_tx
            .send(ServerMsg::RoutineSavePreview {
                id: id.clone(),
                name: req.name.clone(),
                description: req.description.clone(),
                tags: req.tags.clone(),
                category: req.category.clone(),
                script: req.script.clone(),
                replace: req.replace.clone(),
            })
            .is_err()
        {
            self.pending.resolve(&id, false).await;
            return false; // client disconnesso
        }

        match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(accept)) => accept,
            Ok(Err(_)) => false,
            Err(_elapsed) => {
                self.pending.resolve(&id, false).await;
                false
            }
        }
    }

    fn should_gate(&self, tool_name: &str) -> bool {
        SENSITIVE_TOOLS.contains(&tool_name)
    }
}

/// Gate di conferma per una sessione **shell** (2.0, spec §4.3 e §8).
///
/// Composizione, non ereditarietà: riusa la meccanica di `LocalUiConfirmer`
/// (id opaco, `ToolConfirmRequest` sulla connessione, `PendingConfirms`,
/// timeout che nega) e cambia SOLO la politica:
/// - `should_gate`: **default del trait** — tutto tranne `show_markdown`. Sul
///   canale shell `run_in_session` esegue nel runspace dell'utente: ogni
///   `ExecInShell` deve essere preceduto da `[Y/n]` (spec §8).
/// - `confirm_routine_save`: **default del trait** — la shell non ha una
///   finestra di anteprima; il corpo della routine viene appiattito nel testo
///   del prompt `[Y/n]`.
///
/// Non aggiunge campi: tenere qui la `LocalUiConfirmer` interna evita di
/// duplicare `confirm()` (che è l'unica parte con logica vera).
///
/// `#[allow(dead_code)]`: nessun canale collega ancora `ShellConfirmer` (lo
/// farà il Task 8, che instrada la sessione shell) — finché non esiste quel
/// filo, `cargo build`/`clippy` (senza i test, che invece la usano) la
/// vedrebbero come mai costruita.
#[allow(dead_code)]
pub(crate) struct ShellConfirmer {
    inner: LocalUiConfirmer,
}

impl ShellConfirmer {
    // `#[allow(dead_code)]` anche qui: senza, clippy segnala `new` come "mai
    // usata" nella build senza test (vedi doc-comment dello struct sopra).
    #[allow(dead_code)]
    pub(crate) fn new(
        out_tx: UnboundedSender<ServerMsg>,
        pending: PendingConfirms,
        timeout: Duration,
        command_id: String,
    ) -> Self {
        Self { inner: LocalUiConfirmer::new(out_tx, pending, timeout, command_id) }
    }
}

#[async_trait]
impl ToolConfirmer for ShellConfirmer {
    async fn confirm(&self, commands: &str) -> bool {
        self.inner.confirm(commands).await
    }
    // `confirm_routine_save` e `should_gate`: default del trait, di proposito
    // (vedi doc-comment dello struct).
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests — TDD: RED poi GREEN
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── PendingConfirms ────────────────────────────────────────────────────

    #[tokio::test]
    async fn register_then_resolve_completes_the_receiver() {
        let pending = PendingConfirms::new();
        let rx = pending.register("id-1".to_string()).await;

        let found = pending.resolve("id-1", true).await;
        assert!(found, "resolve deve trovare un id appena registrato");

        let accept = rx.await.expect("il receiver deve ricevere un valore");
        assert!(accept, "deve propagare l'accept passato a resolve");
    }

    #[tokio::test]
    async fn resolve_unknown_id_is_noop() {
        let pending = PendingConfirms::new();
        let found = pending.resolve("mai-registrato", true).await;
        assert!(!found, "un id sconosciuto non deve trovare nulla");
    }

    #[tokio::test]
    async fn resolve_twice_second_call_is_noop() {
        let pending = PendingConfirms::new();
        let _rx = pending.register("id-2".to_string()).await;

        assert!(pending.resolve("id-2", false).await, "prima resolve trova il pending");
        assert!(!pending.resolve("id-2", true).await, "seconda resolve sullo stesso id è no-op");
    }

    // ── LocalUiConfirmer ────────────────────────────────────────────────────

    /// Simula il lato UI: legge il `ToolConfirmRequest` mandato su `out_tx` e
    /// risponde subito tramite `pending.resolve`, come farebbe il reader loop di
    /// `ws.rs` su un vero `ClientMsg::ToolConfirmResponse`.
    #[tokio::test]
    async fn confirm_resolves_true_when_ui_accepts() {
        let (out_tx, mut out_rx) = tokio::sync::mpsc::unbounded_channel();
        let pending = PendingConfirms::new();
        let confirmer = LocalUiConfirmer::new(out_tx, pending.clone(), Duration::from_secs(5), "c1".to_string());

        let confirm_task = tokio::spawn(async move { confirmer.confirm("$ nmap_quick_scan 10.0.0.5").await });

        let msg = out_rx.recv().await.expect("deve arrivare ToolConfirmRequest");
        let id = match msg {
            ServerMsg::ToolConfirmRequest { id, commands } => {
                assert_eq!(commands, "$ nmap_quick_scan 10.0.0.5");
                id
            }
            other => panic!("atteso ToolConfirmRequest, trovato {other:?}"),
        };
        assert!(pending.resolve(&id, true).await, "resolve deve trovare l'id appena registrato");

        assert!(confirm_task.await.unwrap(), "accept=true deve far tornare true da confirm()");
    }

    #[tokio::test]
    async fn confirm_resolves_false_when_ui_rejects() {
        let (out_tx, mut out_rx) = tokio::sync::mpsc::unbounded_channel();
        let pending = PendingConfirms::new();
        let confirmer = LocalUiConfirmer::new(out_tx, pending.clone(), Duration::from_secs(5), "c1".to_string());

        let confirm_task = tokio::spawn(async move { confirmer.confirm("$ nmap_os_detect 10.0.0.5").await });

        let msg = out_rx.recv().await.expect("deve arrivare ToolConfirmRequest");
        let id = match msg {
            ServerMsg::ToolConfirmRequest { id, .. } => id,
            other => panic!("atteso ToolConfirmRequest, trovato {other:?}"),
        };
        pending.resolve(&id, false).await;

        assert!(!confirm_task.await.unwrap(), "accept=false deve far tornare false da confirm()");
    }

    /// Sicurezza: l'id nel `ToolConfirmRequest` non contiene frammenti del comando
    /// (stessa garanzia già testata per `TelegramConfirmer`).
    #[tokio::test]
    async fn confirm_uses_opaque_id_not_derived_from_command() {
        let (out_tx, mut out_rx) = tokio::sync::mpsc::unbounded_channel();
        let pending = PendingConfirms::new();
        let confirmer = LocalUiConfirmer::new(out_tx, pending.clone(), Duration::from_millis(50), "c1".to_string());

        let confirm_task = tokio::spawn(async move { confirmer.confirm("$ rm -rf /tmp/segreto").await });

        let msg = out_rx.recv().await.expect("deve arrivare ToolConfirmRequest");
        let id = match msg {
            ServerMsg::ToolConfirmRequest { id, commands } => {
                assert_eq!(commands, "$ rm -rf /tmp/segreto", "il prompt mostra il comando esatto");
                id
            }
            other => panic!("atteso ToolConfirmRequest, trovato {other:?}"),
        };
        assert_eq!(id.len(), 32, "id a 32 hex");
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()), "id solo esadecimale: {id}");
        assert!(!id.contains("rm") && !id.contains("segreto"), "id non deriva dal comando: {id}");

        assert!(!confirm_task.await.unwrap(), "nessuna risposta → timeout → false");
    }

    #[tokio::test]
    async fn confirm_times_out_and_denies_without_hanging() {
        let (out_tx, mut out_rx) = tokio::sync::mpsc::unbounded_channel();
        let pending = PendingConfirms::new();
        let confirmer = LocalUiConfirmer::new(out_tx, pending, Duration::from_millis(50), "c1".to_string());

        let result = tokio::time::timeout(Duration::from_secs(2), confirmer.confirm("$ qualsiasi"))
            .await
            .expect("confirm() deve tornare entro il proprio timeout, senza bloccarsi");
        assert!(!result, "timeout deve negare");

        assert!(out_rx.recv().await.is_some(), "il messaggio è comunque stato mandato");
    }

    /// Regressione (trovata in review Task 3): un timeout deve anche RIMUOVERE
    /// l'entry da `PendingConfirms`, non solo negare. Altrimenti resta orfana
    /// nella mappa finché la connessione WS non chiude (mai risolta, mai droppata).
    /// La prova non è sul valore di ritorno di `confirm()` (già `false` prima di
    /// questo fix) ma sullo STATO del registro: se il cleanup è avvenuto, una
    /// `resolve()` successiva sullo stesso id non trova più nulla (`false`); se
    /// l'entry fosse rimasta orfana, la troverebbe (`true`).
    #[tokio::test]
    async fn confirm_timeout_removes_orphaned_entry_from_registry() {
        let (out_tx, mut out_rx) = tokio::sync::mpsc::unbounded_channel();
        let pending = PendingConfirms::new();
        // Cloniamo `pending` PRIMA di spostarlo nel confirmer: ci serve un handle
        // per ispezionare lo stato del registro dopo il timeout.
        let confirmer = LocalUiConfirmer::new(out_tx, pending.clone(), Duration::from_millis(50), "c1".to_string());

        // Await diretto (non spawnato): il timeout interno di 50ms è già un limite
        // stretto, non serve un timeout esterno, e restare sincroni evita ogni
        // race con l'assert successivo (il cleanup è garantito completo quando
        // `confirm()` ritorna).
        let result = confirmer.confirm("$ qualsiasi").await;
        assert!(!result, "timeout deve negare");

        let msg = out_rx.recv().await.expect("il messaggio è comunque stato mandato");
        let id = match msg {
            ServerMsg::ToolConfirmRequest { id, .. } => id,
            other => panic!("atteso ToolConfirmRequest, trovato {other:?}"),
        };

        assert!(!pending.resolve(&id, true).await, "il timeout deve rimuovere l'entry dal registro, non lasciarla orfana");
    }

    // ── LocalUiConfirmer::confirm_routine_save ─────────────────────────────

    fn sample_routine_save_request() -> crate::ai_adapter::RoutineSaveRequest {
        crate::ai_adapter::RoutineSaveRequest {
            name: "list-big-files".to_string(),
            description: "d".to_string(),
            tags: vec!["t".to_string()],
            category: "c".to_string(),
            script: "Get-ChildItem".to_string(),
            replace: None,
        }
    }

    /// Consuma e verifica il chunk di trasparenza "anteprima aperta" che
    /// `confirm_routine_save` emette SEMPRE per primo (review finale
    /// save_routine, finding I3 — prima viveva incondizionato in
    /// `ai_adapter.rs`, ora è responsabilità di `LocalUiConfirmer` stesso).
    /// Verifica sia l'`id` (deve coincidere col `command_id` passato al
    /// costruttore, non un id opaco nuovo) sia che il testo nomini la routine
    /// e "la finestra" — testo corretto SOLO per questo confirmer, che una
    /// finestra la apre davvero.
    async fn expect_routine_preview_chunk(
        out_rx: &mut tokio::sync::mpsc::UnboundedReceiver<ServerMsg>,
        expected_command_id: &str,
        expected_routine_name: &str,
    ) {
        let msg = out_rx.recv().await.expect("deve arrivare il chunk di anteprima");
        match msg {
            ServerMsg::Chunk { id, content } => {
                assert_eq!(id, expected_command_id, "il chunk deve portare l'id del comando/turno corrente");
                assert!(content.contains(expected_routine_name), "il chunk deve nominare la routine: {content}");
                assert!(content.contains("finestra"), "testo corretto solo per la UI locale (apre una finestra): {content}");
            }
            other => panic!("atteso Chunk (anteprima aperta), trovato {other:?}"),
        }
    }

    #[tokio::test]
    async fn confirm_routine_save_sends_routine_save_preview_not_tool_confirm_request() {
        let (out_tx, mut out_rx) = tokio::sync::mpsc::unbounded_channel();
        let pending = PendingConfirms::new();
        let confirmer = LocalUiConfirmer::new(out_tx, pending.clone(), Duration::from_secs(5), "c1".to_string());
        let req = sample_routine_save_request();

        let task = tokio::spawn(async move { confirmer.confirm_routine_save(&req).await });

        expect_routine_preview_chunk(&mut out_rx, "c1", "list-big-files").await;

        let msg = out_rx.recv().await.expect("deve arrivare un messaggio");
        let id = match msg {
            ServerMsg::RoutineSavePreview { id, name, script, .. } => {
                assert_eq!(name, "list-big-files");
                assert_eq!(script, "Get-ChildItem");
                id
            }
            other => panic!("atteso RoutineSavePreview, trovato {other:?}"),
        };

        assert!(pending.resolve(&id, true).await);
        assert!(task.await.unwrap());
    }

    #[tokio::test]
    async fn confirm_routine_save_resolves_false_when_ui_rejects() {
        let (out_tx, mut out_rx) = tokio::sync::mpsc::unbounded_channel();
        let pending = PendingConfirms::new();
        let confirmer = LocalUiConfirmer::new(out_tx, pending.clone(), Duration::from_secs(5), "c1".to_string());
        let req = sample_routine_save_request();

        let task = tokio::spawn(async move { confirmer.confirm_routine_save(&req).await });

        expect_routine_preview_chunk(&mut out_rx, "c1", "list-big-files").await;

        let msg = out_rx.recv().await.expect("deve arrivare un messaggio");
        let id = match msg {
            ServerMsg::RoutineSavePreview { id, .. } => id,
            other => panic!("atteso RoutineSavePreview, trovato {other:?}"),
        };
        pending.resolve(&id, false).await;

        assert!(!task.await.unwrap());
    }

    #[tokio::test]
    async fn confirm_routine_save_times_out_and_cleans_up_pending() {
        let (out_tx, mut out_rx) = tokio::sync::mpsc::unbounded_channel();
        let pending = PendingConfirms::new();
        let confirmer = LocalUiConfirmer::new(out_tx, pending.clone(), Duration::from_millis(50), "c1".to_string());
        let req = sample_routine_save_request();

        let result = confirmer.confirm_routine_save(&req).await;
        assert!(!result, "timeout deve negare");

        expect_routine_preview_chunk(&mut out_rx, "c1", "list-big-files").await;

        let msg = out_rx.recv().await.expect("il messaggio è comunque stato mandato");
        let id = match msg {
            ServerMsg::RoutineSavePreview { id, .. } => id,
            other => panic!("atteso RoutineSavePreview, trovato {other:?}"),
        };
        assert!(!pending.resolve(&id, true).await, "il timeout deve rimuovere l'entry dal registro");
    }

    // ── should_gate ("save_routine" aggiunto a SENSITIVE_TOOLS) ────────────

    #[test]
    fn should_gate_is_true_for_save_routine() {
        let (out_tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let confirmer = LocalUiConfirmer::new(out_tx, PendingConfirms::new(), Duration::from_secs(5), "c1".to_string());
        assert!(confirmer.should_gate("save_routine"));
    }

    // ── should_gate ──────────────────────────────────────────────────────────

    #[test]
    fn should_gate_is_true_for_sensitive_tools_false_for_others() {
        let (out_tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let confirmer = LocalUiConfirmer::new(out_tx, PendingConfirms::new(), Duration::from_secs(5), "c1".to_string());
        assert!(confirmer.should_gate("nmap_quick_scan"));
        assert!(confirmer.should_gate("nmap_os_detect"));
        assert!(confirmer.should_gate("nmap_version_scan"));
        assert!(confirmer.should_gate("nmap_host_discovery"));
        assert!(confirmer.should_gate("nmap_vuln_scan"));
        assert!(confirmer.should_gate("run_routine"));
        assert!(!confirmer.should_gate("run_in_session"));
        assert!(!confirmer.should_gate("open_target"));
        assert!(!confirmer.should_gate("search_routines"));
        assert!(!confirmer.should_gate("qualunque_nome"));
    }

    // ── ShellConfirmer (2.0, spec §4.3/§8) ─────────────────────────────────

    /// Sul canale shell il gate vale per OGNI tool di sistema: `run_in_session`
    /// e `open_target` inclusi (a differenza di `LocalUiConfirmer`, che li
    /// lascia autonomi). `show_markdown` resta libero: apre solo una finestra.
    #[test]
    fn shell_confirmer_gates_run_in_session_and_open_target_but_not_show_markdown() {
        let (out_tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let c = ShellConfirmer::new(out_tx, PendingConfirms::new(), Duration::from_secs(5), "c1".into());
        assert!(c.should_gate("run_in_session"));
        assert!(c.should_gate("open_target"));
        assert!(c.should_gate("nmap_quick_scan"));
        assert!(c.should_gate("save_routine"));
        assert!(!c.should_gate("show_markdown"));
    }

    /// La meccanica è quella di `LocalUiConfirmer`: manda `ToolConfirmRequest`
    /// sulla connessione e attende la risposta registrata in `PendingConfirms`.
    #[tokio::test]
    async fn shell_confirmer_sends_request_and_returns_the_answer() {
        let (out_tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let pending = PendingConfirms::new();
        let c = ShellConfirmer::new(out_tx, pending.clone(), Duration::from_secs(5), "c1".into());
        let confirm = tokio::spawn(async move { c.confirm("$ dir  (interattivo)").await });
        let req = rx.recv().await.expect("ToolConfirmRequest");
        let id = match req {
            ServerMsg::ToolConfirmRequest { id, commands } => {
                assert_eq!(commands, "$ dir  (interattivo)");
                id
            }
            other => panic!("atteso ToolConfirmRequest, ricevuto {other:?}"),
        };
        assert!(pending.resolve(&id, true).await);
        assert!(confirm.await.unwrap());
    }

    /// `save_routine` sulla shell non ha una finestra di anteprima: passa dal
    /// default del trait (testo appiattito → `confirm()` → `[Y/n]` nel terminale),
    /// quindi arriva un `ToolConfirmRequest`, mai un `RoutineSavePreview`.
    #[tokio::test]
    async fn shell_confirmer_routine_save_falls_back_to_plain_confirm() {
        let (out_tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let pending = PendingConfirms::new();
        let c = ShellConfirmer::new(out_tx, pending.clone(), Duration::from_secs(5), "c1".into());
        let req = crate::ai_adapter::RoutineSaveRequest {
            name: "r".into(), description: "d".into(), tags: vec![], category: "c".into(),
            script: "Get-Date".into(), replace: None,
        };
        let task = tokio::spawn(async move { c.confirm_routine_save(&req).await });
        let first = rx.recv().await.unwrap();
        assert!(matches!(first, ServerMsg::ToolConfirmRequest { .. }), "ricevuto {first:?}");
        if let ServerMsg::ToolConfirmRequest { id, .. } = first {
            pending.resolve(&id, false).await;
        }
        assert!(!task.await.unwrap());
    }
}
