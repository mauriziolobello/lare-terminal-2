//! # ai_adapter — cervello AI pluggable (ADR-005)
//!
//! - **Fase 1/Slice 1:** testo (sostituito).
//! - **Slice 2:** [`AiAdapter::respond`] guida un loop di tool-use stateful,
//!   eseguendo `run_in_session`/`open_target` via [`ToolClient`] ed emettendo
//!   `ServerMsg` (trasparenza dei comandi + testo finale).

use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;

use protocol::{ServerMsg, WindowKind};
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

use crate::agent;
use crate::chat_backend::{ChatBackend, TurnStop};
use crate::messages_client::{Block, ConversationHistory, Message};
use crate::tool_client::ToolClient;

/// Etichetta di trasparenza/conferma per un tool_use.
///
/// Caso speciale `run_routine`: a differenza degli altri tool, è GATEIZZATO
/// (`local_confirm.rs::SENSITIVE_TOOLS`) ma la label semplice "routine: `<name>`"
/// non basta a decidere se approvare — non dice cosa fa la routine. Qui si
/// arricchisce con la `description` salvata nell'indice (lookup via
/// `search_routines`, match esatto case-insensitive sul nome — non il primo
/// risultato: `search_routines` fa match per sottostringa, un nome potrebbe
/// esserne sottostringa di un altro). Se il lookup non trova un match esatto
/// (routine sparita fra la scelta dell'AI e l'esecuzione, o canale senza
/// routine) ricade su `agent::display_invocation`, comportamento invariato.
///
/// `format_invocation` (override del canale esterno) vince sempre, prima di
/// ogni altra logica — invariato rispetto a prima di questo cambiamento.
async fn build_label(
    name: &str,
    input: &serde_json::Value,
    tools: &dyn ToolClient,
    format_invocation: Option<fn(&str, &serde_json::Value) -> String>,
) -> String {
    if let Some(f) = format_invocation {
        return f(name, input);
    }
    if name == "run_routine" {
        if let Some(routine_name) = input.get("name").and_then(|v| v.as_str()) {
            let r = tools.search_routines(Some(routine_name)).await;
            if let Some(entry) = r
                .results
                .iter()
                .find(|e| e.name.eq_ignore_ascii_case(routine_name))
            {
                return format!(
                    "Uso la routine disponibile {} ({})",
                    entry.name, entry.description
                );
            }
        }
    }
    agent::display_invocation(name, input)
}

/// Richiesta di salvataggio di una routine, passata a
/// `ToolConfirmer::confirm_routine_save`. Costruita dall'`input` JSON del
/// tool_use `save_routine` (vedi `build_routine_save_request`, Task 8): tutti
/// i campi arrivano già dall'AI nella stessa chiamata, nessun giro
/// `ToolClient` separato serve per il gate. Struct interna all'orchestrator,
/// NON un tipo `protocol` — a differenza di `ServerMsg::RoutineSavePreview`
/// (il messaggio WS che `LocalUiConfirmer` costruisce a partire da questa,
/// vedi Task 7).
#[derive(Debug, Clone, PartialEq)]
pub struct RoutineSaveRequest {
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub category: String,
    pub script: String,
    pub replace: Option<String>,
}

/// Appiattisce una `RoutineSaveRequest` in testo per un confirmer senza una
/// superficie ricca dedicata — usato dal default di `confirm_routine_save`
/// (sotto), quindi da `TelegramConfirmer` (nessun override: stesso
/// round-trip a bottoni [Esegui]/[Annulla] di sempre). Limite noto,
/// accettato: i messaggi Telegram hanno un tetto ~4096 caratteri — nessuna
/// routine di questo repository si avvicina a quella dimensione (design doc
/// §6).
fn flatten_for_inline_review(req: &RoutineSaveRequest) -> String {
    let replace_line = match &req.replace {
        Some(old) => format!("\nSostituisce/aggiorna: {old}"),
        None => String::new(),
    };
    format!(
        "Salva routine '{}' ({})\nTag: {}\nCategoria: {}{}\n\n{}",
        req.name,
        req.description,
        req.tags.join(", "),
        req.category,
        replace_line,
        req.script,
    )
}

/// Costruisce una `RoutineSaveRequest` dall'`input` JSON di un tool_use
/// `save_routine` — tutti i campi arrivano già dall'AI in questa stessa
/// chiamata, nessun giro `ToolClient` separato serve per il gate (a
/// differenza della label di `run_routine`, che deve andare a cercare la
/// `description` altrove — vedi `build_label` sopra).
fn build_routine_save_request(input: &serde_json::Value) -> RoutineSaveRequest {
    RoutineSaveRequest {
        name: input.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        description: input.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        tags: input
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|t| t.as_str().map(str::to_string)).collect())
            .unwrap_or_default(),
        category: input.get("category").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        script: input.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        replace: input.get("replace").and_then(|v| v.as_str()).map(str::to_string),
    }
}

/// Gate di conferma per i tool dell'AI (ADR-007; estensione per-tool —
/// Docs/superpowers/specs/2026-07-15-local-tool-confirm-gate-design.md).
///
/// Implementazioni: `TelegramConfirmer` (gate su tutto tranne `show_markdown`,
/// comportamento storico — NON sovrascrive `should_gate`), `LocalUiConfirmer`
/// (gate SOLO sui tool marcati sensibili — sovrascrive `should_gate`). `None`
/// (nessun confirmer) = nessun tool è mai gateizzato.
/// I supertrait `Send + Sync` sono obbligatori: `respond` gira in task spawnati
/// da `ws.rs` → la sua future deve essere `Send`; `Option<&dyn ToolConfirmer>`
/// è `Send` solo se il trait lo è.
#[async_trait]
pub trait ToolConfirmer: Send + Sync {
    /// Ritorna `true` = esegui il/i tool, `false` = annulla.
    /// `command` è la stringa di display del/i tool (es. `"$ ls -la"`, o più
    /// righe se più tool dello stesso turno sono gateizzati insieme).
    async fn confirm(&self, command: &str) -> bool;

    /// Chiede conferma per il salvataggio di una routine (`save_routine`,
    /// Fase 2 — Docs/superpowers/specs/2026-08-05-save-routine-design.md
    /// §5). A differenza di `confirm`, la richiesta è STRUTTURATA (non una
    /// singola stringa): un confirmer con una superficie ricca (finestra
    /// dedicata) può mostrare il corpo pieno dello script prima di
    /// risolvere.
    ///
    /// Default: appiattisce in testo e passa da `confirm()` — corretto per
    /// `TelegramConfirmer` (nessun override: stesso round-trip a bottoni di
    /// sempre). `LocalUiConfirmer` è l'UNICO override (Task 7): apre una
    /// finestra dedicata invece del banner Sì/No nel cursore.
    async fn confirm_routine_save(&self, req: &RoutineSaveRequest) -> bool {
        self.confirm(&flatten_for_inline_review(req)).await
    }

    /// Il tool `tool_name` deve passare dal gate PRIMA di essere eseguito?
    ///
    /// Default: gate su tutto tranne `show_markdown` (comportamento storico —
    /// `TelegramConfirmer` eredita questo default senza sovrascriverlo).
    /// `LocalUiConfirmer` lo sovrascrive per gateizzare solo un elenco esplicito
    /// di tool sensibili, lasciando `run_in_session`/`open_target` autonomi.
    fn should_gate(&self, tool_name: &str) -> bool {
        tool_name != "show_markdown"
    }
}

/// Astrazione del cervello AI. La storia è DATO esterno (per-connessione).
#[async_trait]
pub trait AiAdapter: Send + Sync {
    /// Produce la risposta per `input`: testo dell'AI + output dei comandi
    /// eseguiti, usando ed estendendo `history`, eseguendo i tool via `tools`.
    /// `opts`: flag per-turno (`allow_windows`, `web_search`) — usato da `/nowin`
    /// per escludere `show_markdown`, e in futuro per abilitare la ricerca web.
    /// `confirmer`: gate opzionale per i tool di sistema (`run_in_session`,
    /// `open_target`); `None` = autonomo (UI locale).
    /// **Emette** i `ServerMsg` su `tx` man mano (sempre terminati da `Done`).
    /// Possiede `tx` (lo droppa al termine); se `tx` è chiuso (client
    /// disconnesso), abbandona senza errore.
    /// `cancel`: token di cancellazione per interrompere il loop prima dell'iterazione
    /// successiva; `None` = nessuna cancellazione richiesta (comportamento invariato).
    #[allow(clippy::too_many_arguments)]
    async fn respond(
        &self,
        id: &str,
        input: &str,
        history: &mut ConversationHistory,
        tools: &dyn ToolClient,
        opts: agent::TurnOptions,
        confirmer: Option<&dyn ToolConfirmer>,
        cancel: Option<CancellationToken>,
        tx: UnboundedSender<ServerMsg>,
    );

    /// Nome del provider/modello per `ServerInfo`. Default: `"unknown"`.
    fn provider(&self) -> String {
        "unknown".to_string()
    }

    /// Risposta **text-only** per la stanza AI Chat (Slice 1a — vedi
    /// `Docs/superpowers/specs/2026-07-02-aichat-ai-participants-design.md` §4): NESSUN
    /// tool, NESSUNA finestra, solo prosa. Questo è il confine di sicurezza della slice:
    /// l'AI "commenta" la conversazione, non "agisce" su di essa (a differenza di
    /// `respond`, che guida un loop di tool-use). `my_ai_label` è la propria etichetta AI
    /// in questa stanza (es. "rumpleteazer-ai") — usata per comporre il system prompt con
    /// l'identità (label+provider). `history` è la conversazione della stanza già
    /// convertita in turni a ruoli veri (proprie righe passate = `assistant`, tutte le
    /// altre = `user` con prefisso label — vedi `build_ai_history` in
    /// `aichat/service.rs`); `request` è la richiesta esplicita dell'umano che ha invocato
    /// l'AI con `@ai`. Ritorna il testo della risposta (mai un errore "duro":
    /// un'implementazione incorpora un eventuale errore leggibile nel testo stesso, come
    /// fa già `respond` con `[errore AI] ...`).
    /// `cancel`: token di cancellazione opzionale, rispettato solo "alla leggera" (una
    /// singola chiamata HTTP non ha iterazioni da interrompere a metà come in `respond`).
    async fn chat_reply(
        &self,
        my_ai_label: &str,
        history: &[Message],
        request: &str,
        cancel: Option<CancellationToken>,
    ) -> String;

    /// Giudizio di rilevanza + eventuale contributo spontaneo per l'AI Chat Slice 2
    /// (auto-partecipazione — vedi
    /// `Docs/superpowers/specs/2026-07-02-aichat-ai-participants-design.md` §10.2). A
    /// differenza di `chat_reply` (che risponde SEMPRE a un'invocazione esplicita
    /// `@ai`), qui l'AI legge la `history` della stanza SENZA che nessuno l'abbia
    /// invocata e decide da sola se ha qualcosa di genuinamente utile da aggiungere:
    /// `None` = silenzio (scelta esplicita dell'AI, non un errore), `Some(text)` =
    /// contributo. Text-only, nessun tool — stesso confine di sicurezza di `chat_reply`.
    ///
    /// **Default `None`** (silenzio incondizionato): questo è ciò che rende
    /// `StubAdapter` deterministicamente silenzioso SENZA bisogno di un override
    /// esplicito (nessuna chiamata HTTP, nessun costo, nessun non-determinismo nei
    /// test del servizio AI Chat — Slice 2 va costruita e testata SOLO con questo
    /// comportamento, mai con l'API reale). `LlmAdapter` (sotto) sovrascrive con
    /// l'implementazione reale. Qualunque altro `impl AiAdapter` (es. i fake di test
    /// in `aichat/service.rs`) eredita questo default silenzioso senza dover
    /// implementare il metodo esplicitamente — nessuna rottura di build a valle.
    async fn chat_autoparticipate(
        &self,
        _my_ai_label: &str,
        _history: &[Message],
        _cancel: Option<CancellationToken>,
    ) -> Option<String> {
        None
    }
}

// ── Stub (fallback senza chiave) ──────────────────────────────────────────────

pub struct StubAdapter;

#[async_trait]
impl AiAdapter for StubAdapter {
    #[allow(clippy::too_many_arguments)]
    async fn respond(
        &self,
        id: &str,
        input: &str,
        _history: &mut ConversationHistory,
        _tools: &dyn ToolClient,
        _opts: agent::TurnOptions,
        _confirmer: Option<&dyn ToolConfirmer>,
        _cancel: Option<CancellationToken>,
        tx: UnboundedSender<ServerMsg>,
    ) {
        let _ = tx.send(ServerMsg::Chunk {
            id: id.to_string(),
            content: format!("[stub AI] ricevuto: {input}"),
        });
        let _ = tx.send(ServerMsg::Done {
            id: id.to_string(),
            exit_code: None,
        });
    }

    fn provider(&self) -> String {
        "stub".to_string()
    }

    async fn chat_reply(
        &self,
        _my_ai_label: &str,
        _history: &[Message],
        request: &str,
        _cancel: Option<CancellationToken>,
    ) -> String {
        format!("[stub AI] commento su: {request}")
    }
}

// ── LlmAdapter (backend-agnostico: loop tool-use, stateful) ──────────────────

/// System prompt "da partecipante chat" (`chat_reply`, invocazione esplicita `@ai`) —
/// calcolato, non più una `const` fissa: porta SEMPRE l'identità (label+provider) del
/// chiamante, fresca ad ogni chiamata. Fix del bug osservato dal vivo (Slice 5, 2026-07-07):
/// senza questa identità, un'AI può "specchiare" l'auto-identificazione di un'altra AI letta
/// nella history — vedi `Docs/superpowers/specs/2026-07-07-aichat-identity-anchor-design.md` §0.
fn chat_system_prompt(my_ai_label: &str, provider: &str, memory_content: Option<&str>) -> String {
    format!(
        "Tu sei '{my_ai_label}', un'istanza AI basata su {provider}, in esecuzione su questa \
         macchina. Sei un partecipante AI in una chat condivisa tra umani e altre AI su macchine \
         diverse (Lare Terminal — AI Chat). Altri partecipanti (umani o altre AI, anche con un \
         nome o un provider simile al tuo) NON sei tu: non impersonarli, non assumere la loro \
         identità né il loro provider, anche se il trascritto mostra un'auto-identificazione \
         recente di qualcun altro. Ti viene mostrato il trascritto della conversazione seguito \
         dalla richiesta specifica di un umano che ti ha invocato esplicitamente. Commenta in \
         modo conciso e utile la discussione, rispondendo alla richiesta. NON puoi eseguire \
         comandi né aprire finestre: rispondi SOLO con testo semplice, in italiano.{memory}",
        memory = memory_block(memory_content),
    )
}

/// System prompt per l'AI Chat Slice 2 (auto-partecipazione). Stesso blocco identità di
/// `chat_system_prompt`, stessa ragione — vedi il doc-comment lì sopra. A differenza di
/// `chat_system_prompt` (che risponde a un'invocazione esplicita), qui l'AI deve giudicare DA
/// SOLA se ha qualcosa da dire, con un'opzione esplicita di silenzio ("SILENCE") che
/// `chat_autoparticipate` traduce in `None`.
fn autoparticipate_system_prompt(my_ai_label: &str, provider: &str, memory_content: Option<&str>) -> String {
    format!(
        "Tu sei '{my_ai_label}', un'istanza AI basata su {provider}, in esecuzione su questa \
         macchina. Sei un partecipante AI in una chat condivisa tra umani e altre AI su macchine \
         diverse (Lare Terminal — AI Chat). Altri partecipanti (umani o altre AI, anche con un \
         nome o un provider simile al tuo) NON sei tu: non impersonarli, non assumere la loro \
         identità né il loro provider, anche se il trascritto mostra un'auto-identificazione \
         recente di qualcun altro. Ti viene mostrato il trascritto della conversazione: NESSUNO \
         ti ha invocato esplicitamente, sei tu a decidere se intervenire. Se hai qualcosa di \
         GENUINAMENTE utile o rilevante da aggiungere, rispondi in modo conciso, in italiano. Se \
         non hai nulla da aggiungere, o il tuo intervento sarebbe solo rumore, rispondi \
         ESATTAMENTE con la parola: SILENCE. NON puoi eseguire comandi né aprire finestre: \
         rispondi SOLO con testo semplice.{memory}",
        memory = memory_block(memory_content),
    )
}

/// Blocco comune "istruzioni memoria" appeso in coda a entrambi i system prompt — SEMPRE
/// presente (le istruzioni sul marker `MEMORIA:` valgono anche a file assente/vuoto), con
/// un paragrafo aggiuntivo SOLO se `memory_content` ha davvero del testo (Slice 2, memoria
/// persistente — vedi
/// `Docs/superpowers/specs/2026-07-07-aichat-persistent-memory-design.md` §2).
fn memory_block(memory_content: Option<&str>) -> String {
    let instructions = " Hai una memoria persistente in un file locale, tua e solo tua, che \
         sopravvive fra sessioni di chat diverse: se vuoi ricordare qualcosa in modo permanente \
         — un fatto, una preferenza, qualcosa che un companion ti ha chiesto di ricordare, o che \
         hai notato tu stessa — scrivi una riga ESATTAMENTE nella forma \"MEMORIA: <la tua \
         nota>\" nella tua risposta: verrà salvata. È una scelta SEMPRE tua: anche se un umano ti \
         chiede esplicitamente di ricordare qualcosa, decidi tu se farlo, come formularlo, o se \
         declinare spiegando perché.";
    match memory_content {
        Some(content) if !content.trim().is_empty() => {
            format!("{instructions} Questo è ciò che hai già scelto di ricordare finora:\n{content}")
        }
        _ => instructions.to_string(),
    }
}

/// Risolve il path di `memory-{label_base}.md`: SEMPRE
/// `<config_dir>/memory-{label_base}.md`, nessuna variabile d'ambiente (D6,
/// 2.0 — la v1 risolveva `LARE_LOCAL_DIR`/`LOCALAPPDATA` qui). `label_base` è
/// SENZA il suffisso `-ai` (es. "rumpleteazer", non "rumpleteazer-ai").
///
/// UNICA copia nel crate: `aichat/service.rs` (scrittura, tool `MEMORIA:`)
/// riusa questa stessa funzione invece di una propria copia quasi identica
/// (prima di questo fix c'erano due resolver indipendenti che avrebbero
/// potuto divergere in silenzio — stesso principio già corretto per
/// `resolve_network_json_path`, vedi `dispatch_tool_at` in `agent.rs`).
pub fn memory_file_path(config_dir: &std::path::Path, label_base: &str) -> PathBuf {
    config_dir.join(format!("memory-{label_base}.md"))
}

/// True se AI Chat è attivo ma l'AI non ha ancora un nome proprio
/// (`ai_display_name` assente in network.json) — un turno cursore in questo
/// stato deve chiedere all'utente/scegliersi un nome (vedi
/// `AI_NAME_REQUEST_ADDENDUM` sotto), UNA volta sola: dopo che il tool
/// `set_ai_display_name` lo scrive, questa funzione ricomincia a
/// restituire `false` dal turno successivo — lettura fresca ad ogni turno,
/// nessuno stato in memoria da tenere sincronizzato col file.
///
/// Versione testabile, parametrizzata sul path invece di risolverlo
/// internamente — stesso principio di `memory_file_path` sopra: il
/// chiamante reale (`respond`, sotto) passa `self.config_dir.join(
/// "network.json")`, MAI ri-derivato qui (D6 — vedi il doc-comment di
/// `agent::dispatch_tool_at` per il perché).
fn needs_ai_name_prompt_at(path: &std::path::Path) -> bool {
    let Ok(bytes) = std::fs::read(path) else { return false };
    let Ok(cfg) = serde_json::from_slice::<crate::aichat::config::AiChatConfig>(&bytes) else {
        return false;
    };
    cfg.enabled && cfg.ai_display_name.is_none()
}

/// Addendum one-shot al system prompt del cursore quando AI Chat è attivo ma
/// l'AI non ha ancora un nome (vedi `needs_ai_name_prompt_at`). Istruisce l'AI a
/// chiedere all'utente PRIMA di rispondere alla sua richiesta corrente, e a
/// persistere la scelta col tool `set_ai_display_name` (Task 7) — dopo, questo
/// addendum smette di comparire (la funzione che lo attiva ricontrolla il file
/// ad ogni turno).
const AI_NAME_REQUEST_ADDENDUM: &str = " PRIMA di rispondere alla richiesta sottostante, fai anche questo: \
    non hai ancora un nome per AI Chat (la stanza di chat condivisa fra questa e altre macchine). \
    Chiedi all'utente: ha già un nome che usa per te in un altro contesto (es. un'altra AI che lo aiuta \
    altrove)? Se sì, usa quello. Se no, sceglilo tu e comunicaglielo. In ENTRAMBI i casi, appena hai un \
    nome, chiama il tool set_ai_display_name con quel nome — dopo non te lo richiederò più.";

/// Applica `AI_NAME_REQUEST_ADDENDUM` a `system`, MA SOLO se il canale del
/// turno non ha un proprio `system_prompt_override` (`has_override`, rispecchia
/// `TurnOptions::system_prompt_override.is_some()`). I canali tool esterni
/// (es. mcp-nmap — Docs/superpowers/specs/2026-07-16-tool-isolation-design.md)
/// hanno un `ToolClient::tool_defs()` COMPLETAMENTE diverso, che non include
/// `set_ai_display_name`: senza questa guardia, un canale così riceverebbe
/// un'istruzione a chiamare un tool che non possiede (stesso genere di gap
/// trovato dalla review del Task 7 su `display_invocation` — un punto che
/// assume un contesto "cursore" senza verificarlo per gli altri canali).
/// Funzione pura (nessun I/O) apposta: `needs_prompt` è già calcolato dal
/// chiamante (`needs_ai_name_prompt_at(&self.config_dir.join("network.json"))`
/// in `respond`, sotto) così questa guardia si testa senza toccare
/// env/filesystem reali.
fn apply_ai_name_addendum(system: String, has_override: bool, needs_prompt: bool) -> String {
    if has_override || !needs_prompt {
        system
    } else {
        format!("{system}{AI_NAME_REQUEST_ADDENDUM}")
    }
}

/// Documento fattuale + titolo di un `ChannelReport` con `defer_to_turn_end:
/// true`, tenuto in sospeso da `respond` fino al testo finale del turno —
/// vedi il commento su `pending_report` in `respond` e su
/// `ChannelReport::defer_to_turn_end` in `tool_client.rs`. Nessun campo per
/// il testo AI: viene letto da `turn.blocks` al momento del flush, non
/// accumulato qui (un solo posto che sa come estrarlo, non due).
struct PendingReportBuf {
    title: String,
    markdown: String,
}

/// Adapter unico, parametrizzato dal `ChatBackend` iniettato — sostituisce i (mai esistiti
/// come tipi separati) `ClaudeAdapter`/`OpenRouterAdapter`: un solo tipo, sostituibile per
/// composizione (Liskov). `provider_name` è passato dal chiamante (es. `main.rs`) invece di
/// essere derivato dal backend, per non aggiungere un metodo `describe()` al trait
/// `ChatBackend` solo per questo scopo di display.
pub struct LlmAdapter {
    backend: Arc<dyn ChatBackend>,
    provider_name: String,
    /// Cartella di configurazione (2.0, D6) — risolta una volta in `main()`
    /// (`RuntimeConfig::config_dir`) e portata qui a costruzione: serve a
    /// `memory_file_path`/`needs_ai_name_prompt_at` (`chat_reply`/
    /// `chat_autoparticipate`/`respond`, sotto), MAI ri-derivata da questo
    /// adapter con una propria lettura di env/`startup.json`.
    config_dir: PathBuf,
}

impl LlmAdapter {
    pub fn new(backend: Arc<dyn ChatBackend>, provider_name: String, config_dir: PathBuf) -> Self {
        Self { backend, provider_name, config_dir }
    }
}

#[async_trait]
impl AiAdapter for LlmAdapter {
    #[allow(clippy::too_many_arguments)]
    async fn respond(
        &self,
        id: &str,
        input: &str,
        history: &mut ConversationHistory,
        tools: &dyn ToolClient,
        opts: agent::TurnOptions,
        confirmer: Option<&dyn ToolConfirmer>,
        cancel: Option<CancellationToken>,
        tx: UnboundedSender<ServerMsg>,
    ) {
        let chunk = |content: String| ServerMsg::Chunk {
            id: id.to_string(),
            content,
        };
        let done = || ServerMsg::Done {
            id: id.to_string(),
            exit_code: None,
        };
        // `fail`: come `done`, ma con `exit_code: Some(1)` — distingue un
        // turno AI fallito (errore backend/API, rifiuto, risposta degenere)
        // da un successo. Prima di questo fix i tre casi condividevano lo
        // stesso `Done{exit_code:None}` di un successo, indistinguibile lato
        // consumatore (es. window.js's "Espandi") dal testo VERO scritto
        // dall'AI: un buffer non vuoto (il placeholder di errore stesso!)
        // arrivava con `done` e veniva scambiato per una risposta valida,
        // sovrascrivendo silenziosamente il documento in Library con testo
        // d'errore dell'orchestrator. Precedente già in questa funzione: la
        // cancellazione emette `Done{exit_code: Some(130)}` per lo stesso
        // motivo (vedi sopra nel loop).
        let fail = || ServerMsg::Done {
            id: id.to_string(),
            exit_code: Some(1),
        };

        // emit!: invia un ServerMsg; se il ricevitore (WS) è andato, abbandona
        // (stop-on-send-error: niente altre iterazioni HTTP nel vuoto).
        // `tx.send` prende `&self` → utilizzabile più volte; `tx` viene droppato
        // a fine `respond`, chiudendo il canale.
        macro_rules! emit {
            ($msg:expr) => {
                if tx.send($msg).is_err() {
                    return;
                }
            };
        }

        // Snapshot per ripristinare la storia se lo scambio fallisce (igiene
        // alternanza ruoli: niente turni `user` consecutivi alla chiamata dopo).
        // Turno user.
        let snapshot = history.len();
        history.push(Message::user_text(input));

        // Report Python differito (financial-markets' `stock_report`, ecc. —
        // vedi `ChannelReport::defer_to_turn_end`): un documento fattuale già
        // pronto, la cui finestra NON si apre subito (a differenza di nmap)
        // perché attende il testo finale del turno (le sezioni 5-6 dell'AI,
        // scritte SOLO dopo aver letto il riassunto del tool). `std::sync::
        // Mutex` (non `RefCell`): `on_text` deve essere `Send` (il trait lo
        // richiede — `respond` gira in un task spawnato), e `&RefCell<_>`
        // non è `Send` perché `RefCell` non è `Sync`. Nessuna `.await` viene
        // mai fatta mentre il lock è preso: non serve `tokio::sync::Mutex`.
        let pending_report: std::sync::Mutex<Option<PendingReportBuf>> = std::sync::Mutex::new(None);

        // Streaming: ogni delta di testo diventa un Chunk inviato su `tx` —
        // TRANNE mentre un report differito è in sospeso: quel testo (le
        // sezioni 5-6 dell'AI) non deve comparire nel pannello, verrà fuso
        // in coda al documento da `flush_pending_report` più sotto, che lo
        // legge da `turn.blocks` a fine turno (non da qui: bufferizzare
        // nel closure duplicherebbe la stessa logica in due posti).
        // `on_text` cattura `tx`/`id`/`pending_report` per riferimento
        // condiviso (tx.send prende &self); coesiste con `emit!` che usa
        // anch'esso `tx`. `on_text` è &mut-preso solo durante `send_turn`,
        // sequenziale rispetto agli emit! successivi → il borrow checker è
        // soddisfatto.
        let mut on_text = |delta: &str| {
            if pending_report.lock().unwrap().is_some() {
                return;
            }
            let _ = tx.send(ServerMsg::Chunk {
                id: id.to_string(),
                content: delta.to_string(),
            });
        };

        // `on_heartbeat`: gemello di `on_text` ma senza payload — un `ping` SSE
        // osservato da `messages_client.rs` arriva qui e diventa un
        // `ServerMsg::Heartbeat`. Stessa cattura per riferimento di `on_text`
        // (vedi commento lì): `tx`/`id` presi in prestito, mai mossi — coesiste
        // col resto delle chiusure che usano `tx` per lo stesso motivo.
        let mut on_heartbeat = || {
            let _ = tx.send(ServerMsg::Heartbeat { id: id.to_string() });
        };

        // Spegne `web_search` se il backend attivo non può eseguirlo davvero
        // (`ChatBackend::supports_web_search`, es. `OpenRouterBackend`), PRIMA
        // di costruire system prompt e tool set: un backend che scarta
        // `ToolSpec::Server` in traduzione già non offre il tool al modello,
        // ma senza questo spegnimento `agent::system_prompt` gli direbbe
        // comunque (nell'addendum) di averlo — il modello tenta la chiamata,
        // riceve "tool sconosciuto: web_search", e fabbrica una scusa. Un solo
        // flag guida sia il prompt sia il tool set (`agent::tools_for`), quindi
        // basta correggerlo una volta qui.
        let opts = agent::TurnOptions {
            web_search: opts.web_search && self.backend.supports_web_search(),
            ..opts
        };

        // Chiude un eventuale report differito in sospeso, aprendo la finestra
        // col documento fattuale + `extra_text` (le sezioni 5-6 dell'AI, se
        // disponibili) fuso in coda — `None`/vuoto lascia il documento
        // com'è. Chiamata su OGNI via d'uscita del loop TRANNE la
        // cancellazione (vedi il ramo cancel più sotto: rispettare lo Stop
        // dell'utente significa non fargli comunque comparire una finestra),
        // non solo il completamento normale: il tool ha già prodotto dati
        // reali, un errore SUCCESSIVO nella conversazione con l'AI (backend
        // down, rifiuto, cap iterazioni) non deve farli sparire — regressione
        // rispetto all'apertura immediata di prima di questo cambiamento, che
        // li mostrava comunque perché apriva la finestra ancora dentro
        // l'iterazione che ha eseguito il tool. `take()` consuma il valore:
        // una seconda chiamata nello stesso `respond` (non dovrebbe succedere,
        // ma per costruzione) è un no-op, mai un doppio OpenWindow.
        let flush_pending_report = |extra_text: Option<&str>| {
            if let Some(p) = pending_report.lock().unwrap().take() {
                if opts.allow_windows {
                    let content = match extra_text {
                        Some(t) if !t.trim().is_empty() => format!("{}\n\n{}", p.markdown, t.trim()),
                        _ => p.markdown,
                    };
                    let _ = tx.send(ServerMsg::OpenWindow { title: p.title, kind: WindowKind::Markdown, content });
                }
            }
        };

        for _ in 0..agent::MAX_ITERATIONS {
            // Controlla la cancellazione all'inizio di ogni iterazione: se il token
            // è stato cancellato (es. l'utente ha premuto Stop), scarta lo scambio
            // corrente dalla storia (igiene alternanza ruoli) e termina.
            if cancel.as_ref().map_or(false, |c| c.is_cancelled()) {
                history.truncate(snapshot);
                emit!(chunk("\u{26d4} Operazione annullata.".to_string()));
                // Deliberatamente NESSUN flush qui: l'utente ha premuto Stop —
                // fargli comunque comparire una finestra tradirebbe quella
                // scelta. Un eventuale report in sospeso viene scartato con
                // `pending_report` stesso, alla fine di questa funzione.
                emit!(ServerMsg::Done { id: id.to_string(), exit_code: Some(130) });
                return;
            }

            // `needs_ai_name_prompt_at` legge `network.json` fresco, una volta per
            // iterazione — stesso principio già in uso poco sopra per
            // `self.backend.supports_web_search()`: un controllo economico fatto
            // proprio qui, prima di costruire il prompt, invece di infilare un
            // altro campo in `TurnOptions` (struct-literal esaustivo in ~50 punti
            // fra questo file e `agent.rs`, quasi nessuno con `..Default::default()`
            // — vedi Docs/superpowers/plans/2026-08-13-aichat-display-names.md, Task 8).
            // `apply_ai_name_addendum` fa da guardia: un canale tool esterno (es.
            // mcp-nmap, `opts.system_prompt_override.is_some()`) non ha il tool
            // `set_ai_display_name` nel proprio `tool_defs()` — l'addendum comparirebbe
            // solo sul cursore/Telegram, dove quel tool è sempre disponibile.
            // `self.config_dir` (2.0, D6): niente ri-derivazione qui, stesso
            // `config_dir` risolto una volta in `main()` via `RuntimeConfig`.
            let system = apply_ai_name_addendum(
                agent::system_prompt(opts.clone()),
                opts.system_prompt_override.is_some(),
                needs_ai_name_prompt_at(&self.config_dir.join("network.json")),
            );
            let turn_tools = agent::tools_for(opts.clone(), tools.tool_defs());
            let snapshot_history = history.to_vec();

            let turn = match self
                .backend
                .send_turn(&system, &turn_tools, &snapshot_history, &mut on_text, &mut on_heartbeat)
                .await
            {
                Ok(t) => t,
                Err(e) => {
                    history.truncate(snapshot);
                    emit!(chunk(format!("[errore AI] {e}")));
                    // Il tool (se presente) ha già prodotto un documento reale
                    // in un'iterazione precedente — un errore qui è successivo
                    // a quel successo, non deve far sparire la finestra.
                    flush_pending_report(None);
                    emit!(fail());
                    return;
                }
            };

            if let TurnStop::Refused { category } = &turn.stop {
                let cat = category.as_deref().unwrap_or("non specificata");
                history.truncate(snapshot);
                emit!(chunk(format!("[AI: richiesta rifiutata \u{2014} {cat}]")));
                flush_pending_report(None);
                emit!(fail());
                return;
            }

            // Salva il turno assistant (solo text + tool_use — per contratto del trait
            // `ChatBackend`, `turn.blocks` non contiene mai altro).
            // NON pushare un turno assistant VUOTO: una risposta di soli blocchi
            // server-side senza testo (tipico di `pause_turn` con web_search)
            // darebbe content vuoto → la richiesta successiva della sessione
            // verrebbe rifiutata dall'API (HTTP 400). Sicuro: blocks vuoti ⇒
            // nessun tool_use ⇒ il loop termina comunque sotto.
            let blocks = turn.blocks;
            if !blocks.is_empty() {
                history.push(Message::with_blocks("assistant", blocks.clone()));
            }

            // Il testo è già stato inviato delta-by-delta da `on_text` durante
            // `send_turn` — non ri-emettiamo il testo completo qui.

            // Raccogli i tool_use.
            let tool_uses: Vec<(String, String, serde_json::Value)> = blocks
                .iter()
                .filter_map(|b| match b {
                    Block::ToolUse { id: tool_id, name, input } => {
                        Some((tool_id.clone(), name.clone(), input.clone()))
                    }
                    _ => None,
                })
                .collect();

            if tool_uses.is_empty() {
                if blocks.is_empty() {
                    // Risposta degenere (né testo né tool_use: es. soli blocchi
                    // server-side / `pause_turn`). Avendo saltato il push del
                    // turno assistant (guardia sopra), il turno user resterebbe
                    // penzolante → `[user, user]` consecutivi alla chiamata dopo
                    // (HTTP 400). Scarta l'intero scambio (igiene storia) e segnala.
                    // `fail()` (non `done()`): non c'è testo vero da mostrare come
                    // successo, solo il placeholder — un consumatore come "Espandi"
                    // non deve poterlo scambiare per una risposta valida.
                    history.truncate(snapshot);
                    emit!(chunk("[nessuna risposta dall'AI \u{2014} riprova]".to_string()));
                    flush_pending_report(None);
                    emit!(fail());
                    return;
                } else if matches!(turn.stop, TurnStop::MaxTokens) {
                    emit!(chunk("[\u{2026}risposta troncata: max_tokens]".to_string()));
                }
                // Risposta finale vera del turno: se un report Python era in
                // sospeso, QUESTO è il testo (sezioni 5-6) da fondere in coda
                // al documento prima di aprirne la finestra — mai prima
                // d'ora, perché prima d'ora quel testo non esisteva ancora.
                let final_text: String = blocks
                    .iter()
                    .filter_map(|b| match b {
                        Block::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n");
                flush_pending_report(Some(&final_text));
                emit!(done());
                return;
            }

            // Gate in blocco (ADR-007; per-tool dal 2026-07-15) CON UN'ECCEZIONE:
            // `save_routine` (Fase 2 — Docs/superpowers/specs/2026-08-05-save-
            // routine-design.md §5) ha una superficie di conferma PROPRIA
            // (finestra dedicata, via `confirm_routine_save`) e non entra MAI
            // nel banner batched — altrimenti il corpo dello script finirebbe
            // nel testo del banner Sì/No che il design esplicitamente evita.
            // Ogni ALTRO tool gateizzato nello stesso turno segue il
            // comportamento invariato: un solo banner, un solo `confirm()`.
            // Il risultato non è più un singolo `bool` ma una mappa
            // `tool_use_id → approvato`, perché un turno può contenere
            // ENTRAMBI i tipi di conferma insieme (es. save_routine +
            // run_routine) e devono poter avere esiti indipendenti.
            let mut approvals: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
            let mut gated_labels: Vec<String> = Vec::new();
            let mut batched_ids: Vec<String> = Vec::new();
            for (tool_id, name, input) in tool_uses
                .iter()
                .filter(|(_, name, _)| confirmer.is_some_and(|c| c.should_gate(name)))
            {
                if name == "save_routine" {
                    let req = build_routine_save_request(input);
                    // Il chunk "anteprima aperta" NON si emette più qui: era
                    // incondizionato (ogni confirmer, ogni canale) ma vero solo per
                    // la UI locale, che apre davvero una finestra. Su Telegram
                    // (nessuna finestra, round-trip a bottoni via il default di
                    // `confirm_routine_save`) il testo "rivedila nella finestra" era
                    // fuorviante — review finale save_routine, finding I3. Spostato
                    // in `LocalUiConfirmer::confirm_routine_save` (local_confirm.rs):
                    // ogni confirmer è ora responsabile della propria messaggistica
                    // channel-appropriate.
                    let ok = match confirmer {
                        Some(c) => c.confirm_routine_save(&req).await,
                        None => true,
                    };
                    approvals.insert(tool_id.clone(), ok);
                } else {
                    gated_labels.push(build_label(name, input, tools, opts.format_invocation).await);
                    batched_ids.push(tool_id.clone());
                }
            }
            let batch_approved = if gated_labels.is_empty() {
                true
            } else {
                match confirmer {
                    Some(c) => c.confirm(&gated_labels.join("\n")).await,
                    None => true,
                }
            };
            for id in batched_ids {
                approvals.insert(id, batch_approved);
            }

            // Esegui i tool, emetti trasparenza, costruisci i tool_result.
            let mut results: Vec<Block> = Vec::new();
            for (tool_id, name, input) in tool_uses {
                if name == "show_markdown" {
                    // Tool orchestrator-native: apre una finestra Markdown (ADR-013).
                    if opts.allow_windows {
                        let (title, content) = agent::markdown_window(&input);
                        emit!(ServerMsg::OpenWindow {
                            title: title.clone(),
                            kind: WindowKind::Markdown,
                            content,
                        });
                        emit!(chunk(format!("\u{1F4C4} finestra aperta: {title}")));
                        results.push(Block::ToolResult {
                            tool_use_id: tool_id,
                            content: "Finestra Markdown aperta.".to_string(),
                            is_error: false,
                        });
                    } else {
                        // Modalità /nowin: niente finestre, anche se il modello
                        // rievoca show_markdown da un turno precedente in storia.
                        results.push(Block::ToolResult {
                            tool_use_id: tool_id,
                            content: "show_markdown non è disponibile in questa modalità: fornisci la risposta direttamente come testo semplice nel terminale.".to_string(),
                            is_error: true,
                        });
                    }
                } else {
                    let label = build_label(&name, &input, tools, opts.format_invocation).await;
                    // Blocca SOLO se il tool è individualmente gateizzato (`should_gate`)
                    // E il verdetto del turno è negato. `approved` da solo non basta: in
                    // un turno misto (un tool gateizzato + uno no, es. run_in_session),
                    // il tool NON gateizzato deve eseguire comunque, anche se `approved`
                    // è `false` per via del tool gateizzato nello stesso turno (bug
                    // trovato in review Task 2 — Docs/superpowers/specs/2026-07-15-
                    // local-tool-confirm-gate-design.md).
                    let this_tool_gated = confirmer.is_some_and(|c| c.should_gate(&name));
                    let this_tool_approved =
                        !this_tool_gated || approvals.get(&tool_id).copied().unwrap_or(true);
                    if this_tool_gated && !this_tool_approved {
                        // Annullato: notifica nel pannello e restituisce is_error all'AI
                        // (l'AI può adattarsi e rispondere diversamente).
                        emit!(chunk(format!("{label} \u{2014} annullato")));
                        results.push(Block::ToolResult {
                            tool_use_id: tool_id,
                            content: "Comando annullato dall'utente.".to_string(),
                            is_error: true,
                        });
                    } else {
                        let outcome = tools.dispatch(&name, &input).await;
                        // "list_screeners" (eccezione nominata, mirror di show_markdown
                        // — ADR-013): NON produce un ChannelReport, produce direttamente
                        // un ServerMsg::OpenScreenerPicker + un ToolResult con testo
                        // breve invece del JSON grezzo {"summary","items"}. Il registry
                        // vero vive in Python (screeners/__init__.py) — qui c'è SOLO il
                        // routing sul nome del tool, zero logica di dominio.
                        // Docs/superpowers/specs/2026-08-11-markets-screener-registry-design.md.
                        #[derive(serde::Deserialize)]
                        struct ScreenerListJson {
                            summary: String,
                            items: Vec<protocol::ScreenerListItem>,
                        }
                        let screener_list_json: Option<ScreenerListJson> = if name == "list_screeners" {
                            serde_json::from_str(&outcome.output).ok()
                        } else {
                            None
                        };
                        let tool_result_text = match &screener_list_json {
                            Some(parsed) => parsed.summary.clone(),
                            None => outcome.output.clone(),
                        };
                        let truncated = agent::truncate_for_model(&tool_result_text);
                        // BUG-004: nel pannello principale mostra SOLO il comando, non
                        // l'output — l'output va all'AI (tool_result) che lo presenta dove
                        // serve (finestra Markdown / testo conciso), senza duplicarlo.
                        // \n\n = paragraph break in Markdown, so the AI's text response
                        // that follows starts a new paragraph instead of concatenating.
                        emit!(chunk(format!("{label}\n\n")));
                        // Riassunto numerico breve, deterministico (mai dall'AI) — mostrato
                        // SUBITO nel pannello, indipendentemente da cosa l'AI scriverà poi
                        // (refinement 2026-07-23, financial-markets' `stock_report`).
                        if let Some(summary) = &outcome.channel_summary {
                            emit!(chunk(summary.clone()));
                        }
                        // Report strutturato (Docs/superpowers/specs/2026-07-16-mcp-nmap-
                        // design.md §6): apertura finestra è DETERMINISTICA, decisa qui —
                        // mai una scelta dell'AI (il canale che produce `report`, es.
                        // mcp-nmap, non espone nemmeno `show_markdown`). Rispetta
                        // `allow_windows` per coerenza con show_markdown: /nowin significa
                        // "niente finestre", punto. Il salvataggio in Library resta invece
                        // una scelta dell'UTENTE: la finestra aperta qui porta già il
                        // pulsante "Salva" esistente (window.js) — un salvataggio
                        // automatico qui produceva doppioni in Library a parità di
                        // titolo/orario quando l'utente cliccava anche quel pulsante.
                        if opts.allow_windows {
                            if let Some(report) = &outcome.report {
                                if report.defer_to_turn_end {
                                    // Documento fattuale pronto, ma la narrativa/ipotesi
                                    // dell'AI non esiste ancora (arriva SOLO nel testo
                                    // finale di questo stesso turno) — sospeso qui,
                                    // aperto da `flush_pending_report` più sotto.
                                    *pending_report.lock().unwrap() = Some(PendingReportBuf {
                                        title: report.title.clone(),
                                        markdown: report.markdown.clone(),
                                    });
                                } else {
                                    emit!(ServerMsg::OpenWindow {
                                        title: report.title.clone(),
                                        kind: WindowKind::Markdown,
                                        content: report.markdown.clone(),
                                    });
                                }
                            }
                            if let Some(parsed) = screener_list_json {
                                emit!(ServerMsg::OpenScreenerPicker { items: parsed.items });
                            }
                        }
                        results.push(Block::ToolResult {
                            tool_use_id: tool_id,
                            content: truncated,
                            is_error: outcome.is_error,
                        });
                    }
                }
            }
            history.push(Message::with_blocks("user", results));
        }

        // Cap raggiunto: scarta lo scambio runaway (igiene storia).
        history.truncate(snapshot);
        emit!(chunk("[\u{2026}limite di iterazioni raggiunto\u{2026}]".to_string()));
        flush_pending_report(None);
        emit!(done());
    }

    fn provider(&self) -> String {
        self.provider_name.clone()
    }

    async fn chat_reply(
        &self,
        my_ai_label: &str,
        history: &[Message],
        request: &str,
        cancel: Option<CancellationToken>,
    ) -> String {
        // Rispetto "leggero" della cancellazione: se il token è già cancellato prima di
        // spendere la richiesta HTTP, non la facciamo affatto. A differenza di `respond`
        // (un loop di iterazioni con un `tx` streaming da chiudere ordinatamente), qui
        // c'è UNA sola chiamata a `send_turn` (per `ClaudeBackend`, instrada comunque sul
        // trasporto streaming con un `on_text` no-op — vedi `claude_backend.rs` — ma per
        // questo adapter è comunque un solo scambio, non un'iterazione a metà da annullare).
        if cancel.as_ref().is_some_and(|c| c.is_cancelled()) {
            return String::new();
        }

        // History a ruoli veri (costruita dal chiamante, `aichat/service.rs` — vedi
        // `build_ai_history`) + un turno finale con la richiesta esplicita dell'umano.
        // SENZA storia stateful: a differenza di `respond` (dove `history` accumula i
        // turni fra chiamate), ogni invocazione `@ai` riparte da zero — il "contesto" è
        // proprio la history passata qui, non uno stato interno all'adapter.
        let mut turns = history.to_vec();
        turns.push(Message::user_text(request));
        let label_base = my_ai_label.strip_suffix("-ai").unwrap_or(my_ai_label);
        let memory_content = std::fs::read_to_string(memory_file_path(&self.config_dir, label_base)).ok();
        let system = chat_system_prompt(my_ai_label, &self.provider_name, memory_content.as_deref());
        let mut noop = |_: &str| {};
        let mut noop_heartbeat = || {};
        match self.backend.send_turn(&system, &[], &turns, &mut noop, &mut noop_heartbeat).await {
            Ok(turn) => turn.text(),
            Err(e) => format!("[errore AI] {e}"),
        }
    }

    async fn chat_autoparticipate(
        &self,
        my_ai_label: &str,
        history: &[Message],
        cancel: Option<CancellationToken>,
    ) -> Option<String> {
        // Stesso rispetto "leggero" della cancellazione di `chat_reply`: un solo scambio
        // (via `send_turn`, `on_text` no-op), niente iterazioni da interrompere a metà.
        if cancel.as_ref().is_some_and(|c| c.is_cancelled()) {
            return None;
        }
        // A differenza di `chat_reply` (history + richiesta esplicita dell'umano), qui
        // non c'è alcuna richiesta aggiuntiva: la history passata (già a ruoli veri) è
        // tutto il contesto — l'AI deve giudicare da sola cosa aggiungere (o se tacere),
        // guidata dal system prompt dedicato.
        let label_base = my_ai_label.strip_suffix("-ai").unwrap_or(my_ai_label);
        let memory_content = std::fs::read_to_string(memory_file_path(&self.config_dir, label_base)).ok();
        let system = autoparticipate_system_prompt(my_ai_label, &self.provider_name, memory_content.as_deref());
        let mut noop = |_: &str| {};
        let mut noop_heartbeat = || {};
        let turn = match self.backend.send_turn(&system, &[], history, &mut noop, &mut noop_heartbeat).await {
            Ok(t) => t,
            Err(e) => {
                // Un errore di rete/API non deve MAI diventare un messaggio di chat
                // "fantasma" nella stanza: a differenza di `chat_reply` (dove l'umano ha
                // esplicitamente chiesto e si aspetta un riscontro, anche di errore), qui
                // nessuno ha invocato nulla — il fallimento silenzioso (None) è il
                // comportamento corretto, coerente col "silenzio" che l'AI stessa può
                // scegliere.
                tracing::warn!("aichat: chat_autoparticipate fallita: {e}");
                return None;
            }
        };
        let text = turn.text();
        let trimmed = text.trim();
        // "SILENCE" (case-insensitive, dopo trim, tollerante a UNA punteggiatura finale
        // — modelli chat-tuned tendono a punteggiare anche una parola isolata come
        // fosse una frase, es. "SILENCE.") o testo vuoto → None. Qualunque altro testo
        // → Some, intatto. `trim_end_matches` toglie SOLO caratteri dal set indicato
        // dalla fine, non fa un match di sottostringa: "silenzioso" non diventa mai
        // "silence" per nessun trimming.
        let without_trailing_punctuation = trimmed.trim_end_matches(['.', '!', '?']);
        if trimmed.is_empty() || without_trailing_punctuation.eq_ignore_ascii_case("silence") {
            None
        } else {
            Some(trimmed.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::TurnOptions;
    use crate::chat_backend::{BackendError, BackendTurn, FakeChatBackend, TurnStop};
    use crate::test_support::collect;
    use crate::tool_client::{FakeToolClient, FixtureChannelToolClient};

    fn text_turn(text: &str) -> BackendTurn {
        BackendTurn {
            blocks: vec![Block::Text {
                text: text.to_string(),
            }],
            stop: TurnStop::Normal,
        }
    }

    fn tool_use_turn(tool_id: &str, name: &str, input: serde_json::Value) -> BackendTurn {
        BackendTurn {
            blocks: vec![Block::ToolUse {
                id: tool_id.to_string(),
                name: name.to_string(),
                input,
            }],
            stop: TurnStop::Normal,
        }
    }

    #[tokio::test]
    async fn stub_respond_returns_stub_chunk_and_done_none() {
        let stub = StubAdapter;
        let mut hist = ConversationHistory::new();
        let tools = FakeToolClient::success("");
        let msgs = collect(|tx| stub.respond("c1", "What is Rust?", &mut hist, &tools, TurnOptions::default(), None, None, tx)).await;
        assert_eq!(msgs.len(), 2);
        assert!(matches!(&msgs[0], ServerMsg::Chunk { content, .. } if content.contains("[stub AI]")));
        assert!(matches!(&msgs[1], ServerMsg::Done { exit_code: None, .. }));
    }

    #[tokio::test]
    async fn claude_text_only_end_turn_returns_text() {
        let fake = Arc::new(FakeChatBackend::ok(text_turn("Ciao!")));
        let adapter = LlmAdapter::new(fake.clone(), "claude-sonnet-4-6".to_string(), std::path::PathBuf::from("/test-config"));
        let mut hist = ConversationHistory::new();
        let tools = FakeToolClient::success("");
        let msgs =
            collect(|tx| adapter.respond("c1", "salutami", &mut hist, &tools, TurnOptions::default(), None, None, tx)).await;
        assert!(matches!(&msgs[0], ServerMsg::Chunk { content, .. } if content == "Ciao!"));
        assert!(matches!(msgs.last().unwrap(), ServerMsg::Done { exit_code: None, .. }));
        let req = fake.recorded();
        // NOTA: `model` non è più un campo osservabile a questo livello — `ChatBackend::send_turn`
        // non lo riceve (è interno a `ClaudeBackend`); il forwarding del modello è già coperto da
        // `claude_backend::tests::forwards_system_tools_and_history_into_messages_request`.
        assert!(!req.system.is_empty());
        assert_eq!(req.tools.len(), 8); // run_in_session + open_target + show_markdown + search_routines + run_routine + get_routine_content + save_routine + set_ai_display_name
    }

    /// Un heartbeat emesso dal backend durante `send_turn` deve arrivare sul canale
    /// come `ServerMsg::Heartbeat{id}` col medesimo id del comando — vedi
    /// Docs/superpowers/specs/2026-08-07-ai-turn-heartbeat-watchdog-design.md §3.
    #[tokio::test]
    async fn heartbeat_from_backend_emits_server_msg_heartbeat() {
        let fake = Arc::new(FakeChatBackend::ok(text_turn("Ciao!")).with_heartbeat());
        let adapter = LlmAdapter::new(fake, "claude-sonnet-4-6".to_string(), std::path::PathBuf::from("/test-config"));
        let mut hist = ConversationHistory::new();
        let tools = FakeToolClient::success("");
        let msgs =
            collect(|tx| adapter.respond("c1", "salutami", &mut hist, &tools, TurnOptions::default(), None, None, tx)).await;

        assert!(
            msgs.iter().any(|m| matches!(m, ServerMsg::Heartbeat { id } if id == "c1")),
            "atteso ServerMsg::Heartbeat{{id:\"c1\"}} fra i messaggi emessi: {msgs:?}"
        );
    }

    /// CANONICO: il loop esegue il tool e rimanda il `tool_result` corretto.
    #[tokio::test]
    async fn loop_executes_tool_and_feeds_result_back() {
        let fake = Arc::new(FakeChatBackend::sequence(vec![
            Ok(tool_use_turn(
                "tu_1",
                "run_in_session",
                serde_json::json!({"command":"ls"}),
            )),
            Ok(text_turn("Ci sono 3 file.")),
        ]));
        let adapter = LlmAdapter::new(fake.clone(), "claude-sonnet-4-6".to_string(), std::path::PathBuf::from("/test-config"));
        let mut hist = ConversationHistory::new();
        let tools = FakeToolClient::success("file.txt\n");

        let msgs =
            collect(|tx| adapter.respond("c1", "conta i file", &mut hist, &tools, TurnOptions::default(), None, None, tx)).await;

        // (a) due richieste
        assert_eq!(fake.request_count(), 2);
        // (b) la SECONDA richiesta porta un tool_result con id corrispondente + contenuto
        let second = fake.nth_request(1);
        let last_msg = second.history.last().unwrap();
        assert_eq!(last_msg.role, "user");
        let has_tool_result = last_msg.content.iter().any(|b| {
            matches!(b, Block::ToolResult { tool_use_id, content, .. }
                if tool_use_id == "tu_1" && content.contains("file.txt"))
        });
        assert!(has_tool_result, "atteso tool_result tu_1 con 'file.txt': {:?}", last_msg.content);
        // (c) Chunk di trasparenza col SOLO comando (BUG-004: niente output nel pannello)
        assert!(msgs.iter().any(|m| matches!(m, ServerMsg::Chunk { content, .. }
            if content.contains("$ ls"))));
        assert!(
            !msgs.iter().any(|m| matches!(m, ServerMsg::Chunk { content, .. }
                if content.contains("file.txt"))),
            "BUG-004: l'output del comando non deve comparire nei Chunk del pannello"
        );
        // (d) Chunk finale col testo
        assert!(msgs.iter().any(|m| matches!(m, ServerMsg::Chunk { content, .. }
            if content.contains("Ci sono 3 file"))));
        assert!(matches!(msgs.last().unwrap(), ServerMsg::Done { .. }));
    }

    #[tokio::test]
    async fn stateful_second_call_includes_first_turn() {
        let fake = Arc::new(FakeChatBackend::sequence(vec![
            Ok(text_turn("Prima risposta")),
            Ok(text_turn("Seconda risposta")),
        ]));
        let adapter = LlmAdapter::new(fake.clone(), "claude-sonnet-4-6".to_string(), std::path::PathBuf::from("/test-config"));
        let mut hist = ConversationHistory::new();
        let tools = FakeToolClient::success("");

        collect(|tx| adapter.respond("c1", "prima", &mut hist, &tools, TurnOptions::default(), None, None, tx)).await;
        collect(|tx| adapter.respond("c2", "seconda", &mut hist, &tools, TurnOptions::default(), None, None, tx)).await;

        let second = fake.nth_request(1);
        assert!(second.history.len() >= 3, "storia troppo corta: {:?}", second.history);
        assert_eq!(second.history[0].role, "user");
        assert_eq!(second.history[1].role, "assistant");
    }

    #[tokio::test]
    async fn loop_caps_iterations() {
        let fake = Arc::new(FakeChatBackend::repeating(Ok(tool_use_turn(
            "tu_x", "run_in_session", serde_json::json!({"command":"loop"}),
        ))));
        let adapter = LlmAdapter::new(fake.clone(), "claude-sonnet-4-6".to_string(), std::path::PathBuf::from("/test-config"));
        let mut hist = ConversationHistory::new();
        let tools = FakeToolClient::success("out");

        let msgs = collect(|tx| adapter.respond(
            "c1", "vai", &mut hist, &tools, TurnOptions::default(), None, None, tx,
        )).await;

        assert_eq!(fake.request_count(), agent::MAX_ITERATIONS, "deve fermarsi al cap");
        assert!(msgs.iter().any(|m| matches!(m, ServerMsg::Chunk { content, .. }
            if content.contains("limite"))));
        assert!(matches!(msgs.last().unwrap(), ServerMsg::Done { .. }));
    }

    #[tokio::test]
    async fn error_path_returns_visible_error() {
        let fake = Arc::new(FakeChatBackend::err(BackendError::Http {
            status: 401,
            body: "x".to_string(),
        }));
        let adapter = LlmAdapter::new(fake, "claude-sonnet-4-6".to_string(), std::path::PathBuf::from("/test-config"));
        let mut hist = ConversationHistory::new();
        let tools = FakeToolClient::success("");
        let msgs = collect(|tx| adapter.respond(
            "c1", "x", &mut hist, &tools, TurnOptions::default(), None, None, tx,
        )).await;
        assert!(matches!(&msgs[0], ServerMsg::Chunk { content, .. } if content.starts_with("[errore AI]")));
        assert!(matches!(msgs.last().unwrap(), ServerMsg::Done { exit_code: Some(1), .. }),
            "atteso Done{{exit_code:1}} su errore backend: {msgs:?}");
    }

    #[tokio::test]
    async fn refusal_path_is_clear() {
        let fake = Arc::new(FakeChatBackend::ok(BackendTurn {
            blocks: Vec::new(),
            stop: TurnStop::Refused { category: Some("cyber".to_string()) },
        }));
        let adapter = LlmAdapter::new(fake, "claude-sonnet-4-6".to_string(), std::path::PathBuf::from("/test-config"));
        let mut hist = ConversationHistory::new();
        let tools = FakeToolClient::success("");
        let msgs = collect(|tx| adapter.respond(
            "c1", "x", &mut hist, &tools, TurnOptions::default(), None, None, tx,
        )).await;
        assert!(matches!(&msgs[0], ServerMsg::Chunk { content, .. }
            if content.contains("rifiutata") && content.contains("cyber")));
        assert!(matches!(msgs.last().unwrap(), ServerMsg::Done { exit_code: Some(1), .. }),
            "atteso Done{{exit_code:1}} su errore backend: {msgs:?}");
    }

    #[tokio::test]
    async fn provider_reports_model() {
        let fake = Arc::new(FakeChatBackend::ok(text_turn("ok")));
        let adapter: Box<dyn AiAdapter> =
            Box::new(LlmAdapter::new(fake, "claude-sonnet-4-6".to_string(), std::path::PathBuf::from("/test-config")));
        assert_eq!(adapter.provider(), "claude-sonnet-4-6");
    }

    /// Igiene storia: un `respond` fallito (Err) non deve lasciare un turno
    /// `user` penzolante, altrimenti la chiamata successiva produce `[user, user]`
    /// consecutivi (che l'API può rifiutare con 400).
    #[tokio::test]
    async fn history_stays_clean_after_error() {
        let fake = Arc::new(FakeChatBackend::sequence(vec![
            Err(BackendError::Network("boom".to_string())),
            Ok(text_turn("ok")),
        ]));
        let adapter = LlmAdapter::new(fake.clone(), "claude-sonnet-4-6".to_string(), std::path::PathBuf::from("/test-config"));
        let mut hist = ConversationHistory::new();
        let tools = FakeToolClient::success("");

        collect(|tx| adapter.respond("c1", "first", &mut hist, &tools, TurnOptions::default(), None, None, tx)).await; // errore → storia ripulita
        collect(|tx| adapter.respond("c2", "second", &mut hist, &tools, TurnOptions::default(), None, None, tx)).await; // ok

        // La seconda richiesta deve partire pulita: solo [user("second")].
        let second = fake.nth_request(1);
        assert_eq!(
            second.history.len(),
            1,
            "storia non pulita dopo errore: {:?}",
            second.history
        );
        assert_eq!(second.history[0].role, "user");
    }

    #[tokio::test]
    async fn loop_show_markdown_emits_open_window_and_acks() {
        let fake = Arc::new(FakeChatBackend::sequence(vec![
            Ok(tool_use_turn(
                "tu_md",
                "show_markdown",
                serde_json::json!({"content":"# Titolo\ncorpo","title":"Spiegazione"}),
            )),
            Ok(text_turn("Ecco.")),
        ]));
        let adapter = LlmAdapter::new(fake.clone(), "claude-sonnet-4-6".to_string(), std::path::PathBuf::from("/test-config"));
        let mut hist = ConversationHistory::new();
        let tools = FakeToolClient::success("");

        let msgs =
            collect(|tx| adapter.respond("c1", "spiegami X", &mut hist, &tools, TurnOptions::default(), None, None, tx)).await;

        // (a) un OpenWindow{Markdown} col titolo/contenuto attesi
        let ow = msgs.iter().find_map(|m| match m {
            ServerMsg::OpenWindow { title, kind, content } => {
                Some((title.clone(), kind.clone(), content.clone()))
            }
            _ => None,
        });
        let (title, kind, content) = ow.expect("atteso un OpenWindow");
        assert_eq!(kind, WindowKind::Markdown);
        assert_eq!(title, "Spiegazione");
        assert_eq!(content, "# Titolo\ncorpo");

        // (b) la seconda richiesta porta un tool_result (ack) con l'id giusto
        let second = fake.nth_request(1);
        let last = second.history.last().unwrap();
        assert!(last.content.iter().any(|b| matches!(b,
            Block::ToolResult { tool_use_id, .. } if tool_use_id == "tu_md")));

        // (c) Chunk di traccia
        assert!(msgs.iter().any(|m| matches!(m, ServerMsg::Chunk { content, .. }
            if content.contains("finestra aperta"))));
    }

    /// Igiene storia: il cap iterazioni scarta lo scambio runaway (truncate).
    #[tokio::test]
    async fn history_cleaned_after_cap() {
        let fake = Arc::new(FakeChatBackend::repeating(Ok(tool_use_turn(
            "tu",
            "run_in_session",
            serde_json::json!({"command":"loop"}),
        ))));
        let adapter = LlmAdapter::new(fake, "claude-sonnet-4-6".to_string(), std::path::PathBuf::from("/test-config"));
        let mut hist = ConversationHistory::new();
        let tools = FakeToolClient::success("out");
        collect(|tx| adapter.respond("c1", "vai", &mut hist, &tools, TurnOptions::default(), None, None, tx)).await;
        assert!(hist.is_empty(), "il cap deve ripulire la storia, len={}", hist.len());
    }

    #[tokio::test]
    async fn nowin_refuses_show_markdown_even_if_model_emits_it() {
        // allow_windows=false: anche se il modello emette show_markdown (es.
        // rievocato da un turno precedente), NON deve aprirsi alcuna finestra;
        // il tool_result deve nudge-are verso il testo, e la risposta finale è testo.
        let fake = Arc::new(FakeChatBackend::sequence(vec![
            Ok(tool_use_turn(
                "tu_md",
                "show_markdown",
                serde_json::json!({"content":"# Storia","title":"Storia"}),
            )),
            Ok(text_turn("C'era una volta il mare.")),
        ]));
        let adapter = LlmAdapter::new(fake.clone(), "claude-sonnet-4-6".to_string(), std::path::PathBuf::from("/test-config"));
        let mut hist = ConversationHistory::new();
        let tools = FakeToolClient::success("");

        let msgs = collect(|tx| adapter.respond("c1", "storia", &mut hist, &tools, TurnOptions { allow_windows: false, web_search: false, format_invocation: None, system_prompt_override: None, lang: None }, None, None, tx)).await;

        // (a) NESSUN OpenWindow.
        assert!(
            !msgs.iter().any(|m| matches!(m, ServerMsg::OpenWindow { .. })),
            "/nowin non deve mai aprire una finestra: {msgs:?}"
        );
        // (b) la seconda richiesta porta un tool_result is_error per quell'id.
        let second = fake.nth_request(1);
        let last = second.history.last().unwrap();
        assert!(last.content.iter().any(|b| matches!(b,
            Block::ToolResult { tool_use_id, is_error, .. } if tool_use_id == "tu_md" && *is_error)));
        // (c) la risposta finale di testo è emessa.
        assert!(msgs.iter().any(|m| matches!(m, ServerMsg::Chunk { content, .. }
            if content.contains("C'era una volta il mare"))));
    }

    /// `/nowin` (allow_windows=false): la richiesta all'AI NON deve includere
    /// `show_markdown` tra i tool — così l'AI non può aprire finestre per quel
    /// turno e la storia resta pulita (nessuna direttiva iniettata).
    #[tokio::test]
    async fn nowin_mode_excludes_show_markdown_tool() {
        let fake = Arc::new(FakeChatBackend::ok(text_turn("ok")));
        let adapter = LlmAdapter::new(fake.clone(), "claude-sonnet-4-6".to_string(), std::path::PathBuf::from("/test-config"));
        let mut hist = ConversationHistory::new();
        let tools = FakeToolClient::success("");
        collect(|tx| adapter.respond("c1", "storia lunga", &mut hist, &tools, TurnOptions { allow_windows: false, web_search: false, format_invocation: None, system_prompt_override: None, lang: None }, None, None, tx)).await;
        let req = fake.recorded();
        assert!(!req.tools.iter().any(|t| t.name() == "show_markdown"),
            "/nowin deve escludere show_markdown");
        // 8 tool custom - show_markdown (era 7 prima di set_ai_display_name, Task 7).
        assert_eq!(req.tools.len(), 7);
    }

    #[tokio::test]
    async fn empty_assistant_turn_not_pushed_keeps_history_valid() {
        // Una risposta SENZA testo né tool_use (es. soli blocchi server-side):
        // non deve lasciare un turno assistant vuoto in storia, altrimenti la
        // chiamata successiva costruirebbe una richiesta non valida.
        let empty = BackendTurn { blocks: vec![], stop: TurnStop::Normal };
        let fake = Arc::new(FakeChatBackend::sequence(vec![
            Ok(empty),
            Ok(text_turn("seconda")),
        ]));
        let adapter = LlmAdapter::new(fake.clone(), "claude-sonnet-4-6".to_string(), std::path::PathBuf::from("/test-config"));
        let mut hist = ConversationHistory::new();
        let tools = FakeToolClient::success("");
        let first_msgs = collect(|tx| adapter.respond("c1", "prima", &mut hist, &tools, TurnOptions::default(), None, None, tx)).await;
        assert!(matches!(first_msgs.last().unwrap(), ServerMsg::Done { exit_code: Some(1), .. }),
            "risposta degenere deve segnalare fallimento: {first_msgs:?}");
        collect(|tx| adapter.respond("c2", "seconda", &mut hist, &tools, TurnOptions::default(), None, None, tx)).await;
        let second = fake.nth_request(1);
        assert!(second.history.iter().all(|m| !m.content.is_empty()),
            "turno vuoto in storia: {:?}", second.history);
        for w in second.history.windows(2) {
            assert_ne!(w[0].role, w[1].role, "ruoli consecutivi uguali: {:?}", second.history);
        }
    }

    #[tokio::test]
    async fn web_search_mode_includes_server_tools() {
        let fake = Arc::new(FakeChatBackend::ok(text_turn("ok")));
        let adapter = LlmAdapter::new(fake.clone(), "claude-sonnet-4-6".to_string(), std::path::PathBuf::from("/test-config"));
        let mut hist = ConversationHistory::new();
        let tools = FakeToolClient::success("");
        collect(|tx| adapter.respond("c1", "che ore sono a Tokyo", &mut hist, &tools,
            TurnOptions { allow_windows: true, web_search: true, format_invocation: None, system_prompt_override: None, lang: None }, None, None, tx)).await;
        let req = fake.recorded();
        assert!(req.tools.iter().any(|t| t.name() == "web_search"));
        assert!(req.tools.iter().any(|t| t.name() == "web_fetch"));
        // 8 tool custom (era 7 prima di set_ai_display_name, Task 7) + 2 web.
        assert_eq!(req.tools.len(), 10);
    }

    #[tokio::test]
    async fn web_search_mode_is_neutered_when_backend_does_not_support_it() {
        // Riproduce il bug dal vivo (2026-07-20): toggle "Ricerca web" ON +
        // backend OpenRouter/DeepSeek (`supports_web_search() == false`). Senza
        // il gate in `respond`, il system prompt promette web_search/web_fetch
        // anche se il tool set inviato non li contiene (li scarta la
        // traduzione OpenRouter) — il modello tenta comunque la chiamata e
        // riceve "tool sconosciuto: web_search".
        let fake = Arc::new(
            FakeChatBackend::ok(text_turn("ok")).with_web_search_supported(false),
        );
        let adapter = LlmAdapter::new(fake.clone(), "deepseek/deepseek-chat".to_string(), std::path::PathBuf::from("/test-config"));
        let mut hist = ConversationHistory::new();
        let tools = FakeToolClient::success("");
        collect(|tx| adapter.respond("c1", "che ore sono a Tokyo", &mut hist, &tools,
            TurnOptions { allow_windows: true, web_search: true, format_invocation: None, system_prompt_override: None, lang: None }, None, None, tx)).await;
        let req = fake.recorded();
        assert!(
            !req.tools.iter().any(|t| t.name() == "web_search"),
            "web_search non deve comparire nel tool set se il backend non lo supporta"
        );
        assert!(
            !req.tools.iter().any(|t| t.name() == "web_fetch"),
            "web_fetch non deve comparire nel tool set se il backend non lo supporta"
        );
        assert!(
            !req.system.contains("web_search"),
            "il system prompt non deve promettere web_search se il backend non può eseguirlo: {}",
            req.system
        );
    }

    // ── ToolConfirmer gate (Docs/15 — Task 2) ────────────────────────────────
    //
    // TDD RED (scritto prima del gate): senza il gate nel ramo `else`,
    // `confirmer_deny_skips_tool_execution` fallirebbe perché il tool_result
    // conterrebbe l'output di FakeToolClient, non "Comando annullato".
    // GREEN: con il gate inserito, i tool_result discriminano correttamente.

    struct AllowAll;
    struct DenyAll;

    #[async_trait]
    impl ToolConfirmer for AllowAll {
        async fn confirm(&self, _command: &str) -> bool {
            true
        }
    }

    #[async_trait]
    impl ToolConfirmer for DenyAll {
        async fn confirm(&self, _command: &str) -> bool {
            false
        }
    }

    /// DenyAll: il tool NON viene eseguito → tool_result "Comando annullato"
    /// con is_error=true; FakeToolClient non deve essere chiamato (il suo output
    /// non compare nel tool_result). Il loop prosegue fino a Done.
    #[tokio::test]
    async fn confirmer_deny_skips_tool_execution() {
        let fake = Arc::new(FakeChatBackend::sequence(vec![
            Ok(tool_use_turn(
                "tu_deny",
                "run_in_session",
                serde_json::json!({"command": "ls"}),
            )),
            Ok(text_turn("Ok, annullato.")),
        ]));
        let adapter = LlmAdapter::new(fake.clone(), "claude-sonnet-4-6".to_string(), std::path::PathBuf::from("/test-config"));
        let mut hist = ConversationHistory::new();
        // FakeToolClient con output distintivo — se fosse eseguito apparirebbe nel tool_result.
        let tools = FakeToolClient::success("OUTPUT_DA_NON_VEDERE");

        let msgs = collect(|tx| {
            adapter.respond("c1", "esegui ls", &mut hist, &tools, TurnOptions::default(), Some(&DenyAll), None, tx)
        }).await;

        // (a) La seconda richiesta all'AI porta un tool_result "annullato" con is_error.
        let second = fake.nth_request(1);
        let last_msg = second.history.last().unwrap();
        assert_eq!(last_msg.role, "user");
        let has_cancelled = last_msg.content.iter().any(|b| {
            matches!(b, Block::ToolResult { tool_use_id, content, is_error }
                if tool_use_id == "tu_deny"
                && content == "Comando annullato dall'utente."
                && *is_error)
        });
        assert!(has_cancelled, "tool_result deve essere 'annullato': {:?}", last_msg.content);

        // (b) L'output del FakeToolClient NON compare nel tool_result.
        let has_fake_output = last_msg.content.iter().any(|b| {
            matches!(b, Block::ToolResult { content, .. } if content.contains("OUTPUT_DA_NON_VEDERE"))
        });
        assert!(!has_fake_output, "l'output del tool non deve comparire se negato: {:?}", last_msg.content);

        // (c) Il loop arriva a Done.
        assert!(matches!(msgs.last().unwrap(), ServerMsg::Done { .. }));
        assert_eq!(fake.request_count(), 2);
    }

    /// `TurnOptions.format_invocation: Some(f)` (canale tool esterno) deve
    /// sostituire `agent::display_invocation` nel Chunk di trasparenza emesso
    /// nel ramo `else` di `respond` (ai_adapter.rs:472-486 circa) quando il tool
    /// eseguito non è `show_markdown`. Stesso harness di
    /// `confirmer_deny_skips_tool_execution`: `FakeChatBackend::sequence` propone
    /// un tool_use per `run_in_session` poi chiude con testo; `confirmer: None`
    /// (nessun gate) fa eseguire il tool senza bloccarsi sul banner di conferma.
    #[tokio::test]
    async fn format_invocation_override_is_used_when_some() {
        fn fixture_format(name: &str, _input: &serde_json::Value) -> String {
            format!("CHANNEL_FMT[{name}]")
        }
        let fake = Arc::new(FakeChatBackend::sequence(vec![
            Ok(tool_use_turn(
                "tu_fmt",
                "run_in_session",
                serde_json::json!({"command": "ls"}),
            )),
            Ok(text_turn("fatto")),
        ]));
        let adapter = LlmAdapter::new(fake.clone(), "claude-sonnet-4-6".to_string(), std::path::PathBuf::from("/test-config"));
        let mut hist = ConversationHistory::new();
        let tools = FakeToolClient::success("output");

        let msgs = collect(|tx| adapter.respond(
            "c1", "fai qualcosa", &mut hist, &tools,
            TurnOptions { allow_windows: true, web_search: false, format_invocation: Some(fixture_format), system_prompt_override: None, lang: None },
            None, None, tx,
        )).await;

        let joined: String = msgs.iter().filter_map(|m| match m {
            ServerMsg::Chunk { content, .. } => Some(content.clone()),
            _ => None,
        }).collect();
        assert!(joined.contains("CHANNEL_FMT[run_in_session]"), "atteso il formatter del canale nell'output: {joined}");
    }

    /// `run_routine` è gateizzato (`SENSITIVE_TOOLS`), ma la label "routine: `<name>`"
    /// da sola non dice all'utente cosa fa la routine prima di doverla approvare —
    /// qui si verifica che `build_label` la arricchisca con la `description`
    /// dell'indice (lookup via `search_routines`, ADR/spec del 2026-08-04). Nessun
    /// confirmer (`None`): il tool esegue comunque autonomamente (stesso harness di
    /// `no_confirmer_executes_autonomously`), la label arricchita compare nel Chunk
    /// finale indipendentemente dal gate — `build_label` è chiamata per OGNI tool
    /// non-`show_markdown`, gateizzato o no.
    #[tokio::test]
    async fn run_routine_label_includes_description_from_index() {
        struct RoutineAwareToolClient;
        #[async_trait::async_trait]
        impl crate::tool_client::ToolClient for RoutineAwareToolClient {
            async fn run_in_session(
                &self,
                _command: &str,
                _progress_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
            ) -> crate::tool_client::CommandResult {
                unreachable!("non usato in questo test")
            }
            async fn reset_session(&self) {}
            async fn open_target(&self, _target: &str) -> crate::tool_client::OpenResult {
                unreachable!("non usato in questo test")
            }
            async fn search_routines(&self, _query: Option<&str>) -> crate::tool_client::SearchRoutinesResult {
                crate::tool_client::SearchRoutinesResult {
                    results: vec![crate::tool_client::RoutineSummary {
                        name: "list-big-files".to_string(),
                        description: "Elenca i file più grandi di 100MB nella cwd".to_string(),
                        category: "disco".to_string(),
                        tags: vec![],
                    }],
                    error: None,
                }
            }
            async fn run_routine(&self, _name: &str, _args: Option<&str>) -> crate::tool_client::RunRoutineResult {
                crate::tool_client::RunRoutineResult {
                    ok: true,
                    message: String::new(),
                    stdout: "file1.zip\n".to_string(),
                    stderr: String::new(),
                    exit_code: 0,
                    cwd: String::new(),
                }
            }
            async fn dispatch(&self, name: &str, input: &serde_json::Value) -> crate::tool_client::DispatchOutcome {
                crate::agent::dispatch_tool_at(self as &dyn crate::tool_client::ToolClient, name, input, std::path::Path::new("network.json")).await
            }
        }

        let fake = Arc::new(FakeChatBackend::sequence(vec![
            Ok(tool_use_turn(
                "tu_routine",
                "run_routine",
                serde_json::json!({"name": "list-big-files"}),
            )),
            Ok(text_turn("fatto")),
        ]));
        let adapter = LlmAdapter::new(fake.clone(), "claude-sonnet-4-6".to_string(), std::path::PathBuf::from("/test-config"));
        let mut hist = ConversationHistory::new();
        let tools = RoutineAwareToolClient;

        let msgs = collect(|tx| {
            adapter.respond("c1", "esegui la routine", &mut hist, &tools, TurnOptions::default(), None, None, tx)
        }).await;

        let joined: String = msgs.iter().filter_map(|m| match m {
            ServerMsg::Chunk { content, .. } => Some(content.clone()),
            _ => None,
        }).collect();
        assert!(
            joined.contains("Uso la routine disponibile list-big-files (Elenca i file più grandi di 100MB nella cwd)"),
            "atteso la label arricchita con la description nell'output: {joined}"
        );
    }

    /// Un turno con `save_routine` + `run_routine` gateizzati INSIEME deve
    /// produrre DUE conferme distinte (una `confirm_routine_save`, una
    /// `confirm` batched) — non un singolo verdetto condiviso. Rifiutare
    /// save_routine non deve annullare run_routine, e viceversa (design doc
    /// §5: save_routine ha una superficie di conferma propria).
    struct RoutineAndBatchConfirmer {
        routine_save_calls: std::sync::Mutex<Vec<String>>,
        batch_calls: std::sync::Mutex<Vec<String>>,
    }

    #[async_trait::async_trait]
    impl ToolConfirmer for RoutineAndBatchConfirmer {
        async fn confirm(&self, command: &str) -> bool {
            self.batch_calls.lock().unwrap().push(command.to_string());
            true // il tool batched (run_routine) è sempre approvato in questo test
        }
        async fn confirm_routine_save(&self, req: &RoutineSaveRequest) -> bool {
            self.routine_save_calls.lock().unwrap().push(req.name.clone());
            false // save_routine è sempre rifiutato in questo test
        }
        fn should_gate(&self, tool_name: &str) -> bool {
            tool_name == "save_routine" || tool_name == "run_routine"
        }
    }

    #[tokio::test]
    async fn save_routine_gets_its_own_confirmation_separate_from_batched_gate() {
        let fake = Arc::new(FakeChatBackend::sequence(vec![
            Ok(BackendTurn {
                blocks: vec![
                    Block::ToolUse {
                        id: "tu_save".to_string(),
                        name: "save_routine".to_string(),
                        input: serde_json::json!({
                            "name": "new-routine", "description": "d", "tags": [],
                            "category": "c", "content": "Write-Host hi"
                        }),
                    },
                    Block::ToolUse {
                        id: "tu_run".to_string(),
                        name: "run_routine".to_string(),
                        input: serde_json::json!({"name": "list-big-files"}),
                    },
                ],
                stop: TurnStop::Normal,
            }),
            Ok(text_turn("fatto")),
        ]));
        let adapter = LlmAdapter::new(fake.clone(), "claude-sonnet-4-6".to_string(), std::path::PathBuf::from("/test-config"));
        let mut hist = ConversationHistory::new();
        let tools = FakeToolClient::success("output run_routine");
        let confirmer = RoutineAndBatchConfirmer {
            routine_save_calls: std::sync::Mutex::new(Vec::new()),
            batch_calls: std::sync::Mutex::new(Vec::new()),
        };

        let msgs = collect(|tx| {
            adapter.respond("c1", "salva e poi esegui", &mut hist, &tools, TurnOptions::default(), Some(&confirmer), None, tx)
        })
        .await;

        // (a) entrambe le vie di conferma sono state chiamate, ciascuna una volta.
        assert_eq!(
            confirmer.routine_save_calls.lock().unwrap().as_slice(),
            &["new-routine".to_string()]
        );
        assert_eq!(confirmer.batch_calls.lock().unwrap().len(), 1);

        // (b) save_routine è stato annullato (rifiutato dalla sua conferma dedicata)...
        assert!(
            msgs.iter().any(|m| matches!(m, ServerMsg::Chunk { content, .. } if content.contains("annullato"))),
            "atteso un annullamento per save_routine: {msgs:?}"
        );
        // (c) ...ma run_routine è stato eseguito comunque (approvato dal batch).
        // NOTA (deviazione dal brief verificata con evidenza, vedi task-8-report.md):
        // il brief originale asseriva `content.contains("output run_routine")` su un
        // `ServerMsg::Chunk` — ma l'output REALE del tool non finisce mai in un
        // Chunk (solo la label ci finisce: commento "BUG-004" sopra nel dispatch
        // loop). L'output vero arriva SOLO nel `ToolResult` della richiesta AI
        // successiva — stesso pattern di verifica già usato da
        // `confirmer_allow_executes_tool` in questo stesso file (`fake.nth_request(1)`).
        // Verifico qui allo stesso modo, sul tool_use_id `tu_run`.
        let second = fake.nth_request(1);
        let last_msg = second.history.last().unwrap();
        let run_routine_executed = last_msg.content.iter().any(|b| {
            matches!(b, Block::ToolResult { tool_use_id, content, is_error }
                if tool_use_id == "tu_run" && content.contains("output run_routine") && !is_error)
        });
        assert!(
            run_routine_executed,
            "run_routine deve eseguire comunque, indipendentemente dal rifiuto di save_routine: {:?}",
            last_msg.content
        );
    }

    /// AllowAll: il tool viene eseguito come con None → tool_result porta l'output
    /// del FakeToolClient; la seconda richiesta all'AI riceve il risultato reale.
    #[tokio::test]
    async fn confirmer_allow_executes_tool() {
        let fake = Arc::new(FakeChatBackend::sequence(vec![
            Ok(tool_use_turn(
                "tu_allow",
                "run_in_session",
                serde_json::json!({"command": "ls"}),
            )),
            Ok(text_turn("Ci sono file.")),
        ]));
        let adapter = LlmAdapter::new(fake.clone(), "claude-sonnet-4-6".to_string(), std::path::PathBuf::from("/test-config"));
        let mut hist = ConversationHistory::new();
        let tools = FakeToolClient::success("file.txt\n");

        collect(|tx| {
            adapter.respond("c1", "conta i file", &mut hist, &tools, TurnOptions::default(), Some(&AllowAll), None, tx)
        }).await;

        // La seconda richiesta porta un tool_result con l'output reale.
        let second = fake.nth_request(1);
        let last_msg = second.history.last().unwrap();
        let has_real_output = last_msg.content.iter().any(|b| {
            matches!(b, Block::ToolResult { tool_use_id, content, .. }
                if tool_use_id == "tu_allow" && content.contains("file.txt"))
        });
        assert!(has_real_output, "tool_result deve contenere l'output reale: {:?}", last_msg.content);
        assert_eq!(fake.request_count(), 2);
    }

    /// None (UI locale): il tool viene eseguito autonomamente (comportamento invariato).
    #[tokio::test]
    async fn no_confirmer_executes_autonomously() {
        let fake = Arc::new(FakeChatBackend::sequence(vec![
            Ok(tool_use_turn(
                "tu_none",
                "run_in_session",
                serde_json::json!({"command": "dir"}),
            )),
            Ok(text_turn("Ecco i file.")),
        ]));
        let adapter = LlmAdapter::new(fake.clone(), "claude-sonnet-4-6".to_string(), std::path::PathBuf::from("/test-config"));
        let mut hist = ConversationHistory::new();
        let tools = FakeToolClient::success("foo.txt\n");

        collect(|tx| {
            adapter.respond("c1", "lista file", &mut hist, &tools, TurnOptions::default(), None, None, tx)
        }).await;

        // La seconda richiesta porta il tool_result con l'output reale (nessun gate).
        let second = fake.nth_request(1);
        let last_msg = second.history.last().unwrap();
        let has_output = last_msg.content.iter().any(|b| {
            matches!(b, Block::ToolResult { tool_use_id, content, .. }
                if tool_use_id == "tu_none" && content.contains("foo.txt"))
        });
        assert!(has_output, "None: tool eseguito autonomamente: {:?}", last_msg.content);
        assert_eq!(fake.request_count(), 2);
    }

    /// Un confirmer che gateizza SOLO tool "sensibili" (should_gate selettivo) —
    /// simula `LocalUiConfirmer` con `run_in_session` non marcato sensibile (non
    /// è nella lista `SENSITIVE_TOOLS` di local_confirm.rs, che oggi gateizza solo
    /// i cinque tool nmap): `run_in_session` resta autonomo come con `None`, e
    /// `confirm()` non deve MAI essere chiamato.
    #[tokio::test]
    async fn selective_confirmer_leaves_ungated_tools_autonomous() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct SelectiveConfirmer(Arc<AtomicUsize>);

        #[async_trait]
        impl ToolConfirmer for SelectiveConfirmer {
            async fn confirm(&self, _commands: &str) -> bool {
                self.0.fetch_add(1, Ordering::SeqCst);
                false // se venisse chiamato negherebbe — il test verifica che non accada
            }

            fn should_gate(&self, _tool_name: &str) -> bool {
                false // nessun tool è "sensibile" in questa configurazione
            }
        }

        let fake = Arc::new(FakeChatBackend::sequence(vec![
            Ok(tool_use_turn(
                "tu_sel",
                "run_in_session",
                serde_json::json!({"command": "dir"}),
            )),
            Ok(text_turn("Ecco i file.")),
        ]));
        let adapter = LlmAdapter::new(fake.clone(), "claude-sonnet-4-6".to_string(), std::path::PathBuf::from("/test-config"));
        let mut hist = ConversationHistory::new();
        let tools = FakeToolClient::success("foo.txt\n");
        let calls = Arc::new(AtomicUsize::new(0));
        let confirmer = SelectiveConfirmer(calls.clone());

        collect(|tx| {
            adapter.respond("c1", "lista file", &mut hist, &tools, TurnOptions::default(), Some(&confirmer), None, tx)
        }).await;

        assert_eq!(calls.load(Ordering::SeqCst), 0, "confirm non deve essere chiamato per tool non gateizzati");

        let second = fake.nth_request(1);
        let last_msg = second.history.last().unwrap();
        let has_output = last_msg.content.iter().any(|b| {
            matches!(b, Block::ToolResult { tool_use_id, content, .. }
                if tool_use_id == "tu_sel" && content.contains("foo.txt"))
        });
        assert!(has_output, "tool eseguito autonomamente nonostante confirmer Some: {:?}", last_msg.content);
    }

    /// Regressione trovata in review Task 2: il verdetto `approved` del turno era
    /// applicato in blocco a TUTTI i tool non-`show_markdown` nel loop di
    /// esecuzione, non solo a quelli che `should_gate` marca. Con un turno misto
    /// (un tool NON gateizzato + un tool gateizzato) e il gate negato, il tool
    /// NON gateizzato (es. `run_in_session`) veniva bloccato per errore insieme a
    /// quello gateizzato — violando "run_in_session/open_target restano
    /// autonomi". Qui un fixture `"fixture_sensitive_tool"` simula un futuro tool
    /// sensibile: `should_gate` lo marca vero, `run_in_session` resta falso.
    #[tokio::test]
    async fn deny_blocks_only_the_gated_tool_in_a_mixed_turn() {
        struct MixedConfirmer;

        #[async_trait]
        impl ToolConfirmer for MixedConfirmer {
            async fn confirm(&self, _commands: &str) -> bool {
                false // nega sempre — verifica che SOLO il tool gateizzato ne risenta
            }

            fn should_gate(&self, tool_name: &str) -> bool {
                tool_name == "fixture_sensitive_tool"
            }
        }

        // Un turno assistant con DUE tool_use: uno non gateizzato (run_in_session)
        // e uno gateizzato (fixture_sensitive_tool), poi la risposta finale.
        let fake = Arc::new(FakeChatBackend::sequence(vec![
            Ok(BackendTurn {
                blocks: vec![
                    Block::ToolUse {
                        id: "tu_run".to_string(),
                        name: "run_in_session".to_string(),
                        input: serde_json::json!({"command": "dir"}),
                    },
                    Block::ToolUse {
                        id: "tu_sensitive".to_string(),
                        name: "fixture_sensitive_tool".to_string(),
                        input: serde_json::json!({}),
                    },
                ],
                stop: TurnStop::Normal,
            }),
            Ok(text_turn("ok")),
        ]));
        let adapter = LlmAdapter::new(fake.clone(), "claude-sonnet-4-6".to_string(), std::path::PathBuf::from("/test-config"));
        let mut hist = ConversationHistory::new();
        let tools = FakeToolClient::success("foo.txt\n");
        let confirmer = MixedConfirmer;

        collect(|tx| {
            adapter.respond("c1", "misto", &mut hist, &tools, TurnOptions::default(), Some(&confirmer), None, tx)
        }).await;

        let second = fake.nth_request(1);
        let last_msg = second.history.last().unwrap();

        // Il tool NON gateizzato deve essere eseguito (output reale presente).
        let run_executed = last_msg.content.iter().any(|b| {
            matches!(b, Block::ToolResult { tool_use_id, content, .. }
                if tool_use_id == "tu_run" && content.contains("foo.txt"))
        });
        assert!(run_executed, "run_in_session (should_gate=false) deve eseguire nonostante il turno sia negato: {:?}", last_msg.content);

        // Il tool gateizzato deve restare annullato.
        let sensitive_cancelled = last_msg.content.iter().any(|b| {
            matches!(b, Block::ToolResult { tool_use_id, content, is_error }
                if tool_use_id == "tu_sensitive"
                && content == "Comando annullato dall'utente."
                && *is_error)
        });
        assert!(sensitive_cancelled, "fixture_sensitive_tool (should_gate=true) deve restare annullato: {:?}", last_msg.content);
    }

    /// Sicurezza (Docs/15): anche `open_target` passa dal gate (stesso ramo `else`
    /// di `run_in_session`). Con un confirmer che nega, l'apertura NON viene
    /// eseguita → tool_result "annullato" + l'output del tool non compare.
    #[tokio::test]
    async fn confirmer_deny_skips_open_target() {
        let fake = Arc::new(FakeChatBackend::sequence(vec![
            Ok(tool_use_turn(
                "tu_open",
                "open_target",
                serde_json::json!({"target": "https://example.com"}),
            )),
            Ok(text_turn("Ok, non apro.")),
        ]));
        let adapter = LlmAdapter::new(fake.clone(), "claude-sonnet-4-6".to_string(), std::path::PathBuf::from("/test-config"));
        let mut hist = ConversationHistory::new();
        let tools = FakeToolClient::success("APERTO_DA_NON_VEDERE");

        collect(|tx| {
            adapter.respond("c1", "apri example", &mut hist, &tools, TurnOptions::default(), Some(&DenyAll), None, tx)
        }).await;

        let second = fake.nth_request(1);
        let last_msg = second.history.last().unwrap();
        let has_cancelled = last_msg.content.iter().any(|b| {
            matches!(b, Block::ToolResult { tool_use_id, content, is_error }
                if tool_use_id == "tu_open"
                && content == "Comando annullato dall'utente."
                && *is_error)
        });
        assert!(has_cancelled, "open_target negato deve dare 'annullato': {:?}", last_msg.content);

        let has_output = last_msg.content.iter().any(|b| {
            matches!(b, Block::ToolResult { content, .. } if content.contains("APERTO_DA_NON_VEDERE"))
        });
        assert!(!has_output, "open_target negato non deve eseguire: {:?}", last_msg.content);
    }

    // ── TDD RED: CancellationToken (stop button) ─────────────────────────────
    //
    // Questi test sono scritti PRIMA di aggiungere il parametro `cancel` alla firma
    // di `respond` e il controllo dentro il loop. Pre-implementazione producono
    // errori di compilazione (numero argomenti sbagliato) → quel compile error È la fase RED.
    // Dopo l'aggiunta del parametro i test devono passare.

    /// Token già cancellato prima della chiamata: `respond` deve emettere
    /// `Done{exit_code: Some(130)}` senza chiamare alcun tool né l'AI.
    #[tokio::test]
    async fn respond_cancel_before_first_iteration_emits_done() {
        let token = CancellationToken::new();
        token.cancel(); // già cancellato

        let adapter = StubAdapter;
        let mut hist = ConversationHistory::new();
        let tools = FakeToolClient::success("");
        let msgs = collect(|tx| adapter.respond(
            "id1", "test", &mut hist, &tools, TurnOptions::default(), None, Some(token), tx,
        )).await;

        // StubAdapter non controlla il cancel token (è lo Stub — non ha iterazioni).
        // Questo test verifica che la firma accetti Some(token): se compila, è sufficiente.
        // Il comportamento di cancellazione è verificato su LlmAdapter qui sotto.
        assert!(msgs.iter().any(|m| matches!(m, ServerMsg::Done { .. })),
            "StubAdapter deve emettere Done anche con cancel token: {msgs:?}");
    }

    /// LlmAdapter con token già cancellato: deve emettere Done{130} senza
    /// chiamare l'API (zero richieste al FakeChatBackend).
    #[tokio::test]
    async fn claude_cancel_before_first_iteration_skips_api_call() {
        let fake = Arc::new(FakeChatBackend::ok(text_turn("mai chiamato")));
        let adapter = LlmAdapter::new(fake.clone(), "claude-sonnet-4-6".to_string(), std::path::PathBuf::from("/test-config"));
        let mut hist = ConversationHistory::new();
        let tools = FakeToolClient::success("");

        let token = CancellationToken::new();
        token.cancel(); // già cancellato prima della chiamata

        let msgs = collect(|tx| adapter.respond(
            "id1", "test", &mut hist, &tools, TurnOptions::default(), None, Some(token), tx,
        )).await;

        // (a) nessuna chiamata all'API
        assert_eq!(fake.request_count(), 0,
            "cancel prima della prima iterazione: nessuna richiesta API attesa");
        // (b) Done{exit_code: Some(130)} terminale
        assert!(msgs.iter().any(|m| matches!(m, ServerMsg::Done { exit_code: Some(130), .. })),
            "atteso Done{{exit_code:130}} con cancel: {msgs:?}");
        // (c) storia pulita (nessun turno penzolante)
        assert!(hist.is_empty(),
            "storia deve restare vuota dopo cancel: len={}", hist.len());
    }

    /// LlmAdapter con cancel=None: il comportamento normale è invariato.
    #[tokio::test]
    async fn claude_no_cancel_token_behaves_normally() {
        let fake = Arc::new(FakeChatBackend::ok(text_turn("ok normale")));
        let adapter = LlmAdapter::new(fake.clone(), "claude-sonnet-4-6".to_string(), std::path::PathBuf::from("/test-config"));
        let mut hist = ConversationHistory::new();
        let tools = FakeToolClient::success("");

        let msgs = collect(|tx| adapter.respond(
            "id1", "test", &mut hist, &tools, TurnOptions::default(), None, None, tx,
        )).await;

        assert_eq!(fake.request_count(), 1, "cancel=None: richiesta API deve avvenire");
        assert!(msgs.iter().any(|m| matches!(m, ServerMsg::Done { exit_code: None, .. })),
            "cancel=None: Done normale atteso: {msgs:?}");
    }

    /// Gate in blocco (ricerca file): due tool soggetti al gate nello STESSO turno → UNA sola
    /// chiamata a `confirm` (non due), e il verdetto si applica a entrambi.
    #[tokio::test]
    async fn confirmer_batches_multiple_tools_in_one_turn() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct CountingDeny(Arc<AtomicUsize>);
        #[async_trait]
        impl ToolConfirmer for CountingDeny {
            async fn confirm(&self, _commands: &str) -> bool {
                self.0.fetch_add(1, Ordering::SeqCst);
                false
            }
        }

        // Un turno assistant con DUE open_target, poi la risposta finale.
        let fake = Arc::new(FakeChatBackend::sequence(vec![
            Ok(BackendTurn {
                blocks: vec![
                    Block::ToolUse {
                        id: "t1".to_string(),
                        name: "open_target".to_string(),
                        input: serde_json::json!({"target": "a.txt"}),
                    },
                    Block::ToolUse {
                        id: "t2".to_string(),
                        name: "open_target".to_string(),
                        input: serde_json::json!({"target": "b.txt"}),
                    },
                ],
                stop: TurnStop::Normal,
            }),
            Ok(text_turn("ok")),
        ]));
        let adapter = LlmAdapter::new(fake.clone(), "claude-sonnet-4-6".to_string(), std::path::PathBuf::from("/test-config"));
        let mut hist = ConversationHistory::new();
        let tools = FakeToolClient::success("X");
        let calls = Arc::new(AtomicUsize::new(0));
        let confirmer = CountingDeny(calls.clone());

        collect(|tx| {
            adapter.respond("c1", "apri due file", &mut hist, &tools, TurnOptions::default(), Some(&confirmer), None, tx)
        }).await;

        // (a) confirm chiamato UNA sola volta per il turno.
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "confirm deve essere chiamato una sola volta per il turno (gate in blocco)"
        );
        // (b) Entrambi i tool risultano annullati (verdetto unico applicato a tutti).
        let second = fake.nth_request(1);
        let last = second.history.last().unwrap();
        let cancelled = last
            .content
            .iter()
            .filter(|b| matches!(b, Block::ToolResult { content, is_error, .. }
                if content == "Comando annullato dall'utente." && *is_error))
            .count();
        assert_eq!(cancelled, 2, "entrambi i tool devono risultare annullati: {:?}", last.content);
    }

    #[tokio::test]
    async fn stub_chat_reply_mentions_request() {
        let stub = StubAdapter;
        let text = stub
            .chat_reply("rumpleteazer-ai", &[], "commenta X", None)
            .await;
        assert!(
            text.contains("commenta X"),
            "atteso che la risposta citi la richiesta: {text}"
        );
    }

    /// `LlmAdapter::chat_reply` deve essere UNA sola chiamata a `ChatBackend::send_turn`
    /// **text-only**: il payload non porta alcun tool (confine di sicurezza della
    /// Slice 1a — l'AI "commenta", non "agisce"), riceve la history così com'è passata
    /// (nessuna trasformazione qui — la costruzione dei ruoli è responsabilità del
    /// chiamante, `aichat/service.rs`), e ritorna il testo della risposta del fake senza
    /// modifiche.
    #[tokio::test]
    async fn claude_chat_reply_returns_text_only() {
        let fake = Arc::new(FakeChatBackend::ok(text_turn("Ecco un commento.")));
        let adapter = LlmAdapter::new(fake.clone(), "anthropic".to_string(), std::path::PathBuf::from("/test-config"));

        let history = vec![
            Message::user_text("skimble-human: ciao"),
            Message::user_text("quaxo-human: come va?"),
        ];
        let text = adapter
            .chat_reply("rumpleteazer-ai", &history, "commenta la discussione", None)
            .await;

        assert_eq!(text, "Ecco un commento.");
        let req = fake.recorded();
        assert!(
            req.tools.is_empty(),
            "chat_reply deve essere text-only: nessun tool nel payload: {:?}",
            req.tools
        );
        // La history passata arriva intatta (2 turni), PIÙ un turno finale con la
        // richiesta esplicita dell'umano — chat_reply aggiunge SOLO quello, non
        // ricostruisce la history ricevuta.
        assert_eq!(req.history.len(), 3, "history + richiesta: {:?}", req.history);
        assert_eq!(req.history[0].role, "user");
        assert_eq!(req.history[1].role, "user");
        let last_has_request = req.history[2].content.iter().any(|b| {
            matches!(b, Block::Text { text } if text.contains("commenta la discussione"))
        });
        assert!(last_has_request, "l'ultimo turno deve portare la richiesta: {:?}", req.history[2]);
        // Il system prompt deve portare l'identità (label+provider) — è il fix di
        // questa slice: senza questa asserzione, un regresso che tornasse alla const
        // generica passerebbe questo test comunque.
        assert!(
            req.system.contains("rumpleteazer-ai") && req.system.contains("anthropic"),
            "il system prompt deve portare label+provider: {}",
            req.system
        );
        assert!(
            !req.system.contains("run_in_session"),
            "chat_reply non deve usare il system prompt del cursore: {}",
            req.system
        );
    }

    // ── AI Chat — Slice 2: `chat_autoparticipate` (giudizio di rilevanza) ──

    #[tokio::test]
    async fn stub_autoparticipate_is_silent() {
        let stub = StubAdapter;
        let reply = stub.chat_autoparticipate("skimble-ai", &[], None).await;
        assert_eq!(reply, None, "lo Stub deve sempre tacere: {reply:?}");
    }

    #[tokio::test]
    async fn claude_autoparticipate_silence_maps_to_none() {
        let fake = Arc::new(FakeChatBackend::ok(text_turn("  Silence  ")));
        let adapter = LlmAdapter::new(fake.clone(), "anthropic".to_string(), std::path::PathBuf::from("/test-config"));

        let history = vec![Message::user_text("skimble-human: che ore sono?")];
        let reply = adapter.chat_autoparticipate("rumpleteazer-ai", &history, None).await;

        assert_eq!(reply, None, "\"SILENCE\" (case-insensitive, con spazi) deve mappare a None");
        let req = fake.recorded();
        assert!(
            req.tools.is_empty(),
            "chat_autoparticipate deve essere text-only: nessun tool nel payload: {:?}",
            req.tools
        );
        assert_eq!(req.history.len(), 1, "history passata intatta, nessuna richiesta aggiunta: {:?}", req.history);
    }

    /// Bug osservato dal vivo (2026-07-08): il modello a volte aggiunge un punto finale
    /// ("SILENCE." invece di "SILENCE") — comune nei modelli chat-tuned, che tendono a
    /// punteggiare anche una singola parola come fosse una frase. Il confronto esatto
    /// precedente (`eq_ignore_ascii_case("silence")`) falliva su questa variazione, e il
    /// testo "SILENCE." veniva pubblicato in chat come un contributo vero — rumoroso e,
    /// osservato dal vivo, capace di innescare un'ulteriore cascata di
    /// auto-partecipazione sulle altre macchine (che reagivano a quel messaggio come a
    /// un contributo genuino).
    #[tokio::test]
    async fn claude_autoparticipate_silence_with_trailing_punctuation_maps_to_none() {
        let fake = Arc::new(FakeChatBackend::ok(text_turn("SILENCE.")));
        let adapter = LlmAdapter::new(fake.clone(), "anthropic".to_string(), std::path::PathBuf::from("/test-config"));

        let history = vec![Message::user_text("skimble-human: che ore sono?")];
        let reply = adapter.chat_autoparticipate("rumpleteazer-ai", &history, None).await;

        assert_eq!(reply, None, "\"SILENCE.\" (con punto finale) deve comunque mappare a None: {reply:?}");
    }

    /// Stessa tolleranza per punto esclamativo/interrogativo — variazioni altrettanto
    /// plausibili di un modello che "recita" la parola invece di scriverla nuda.
    #[tokio::test]
    async fn claude_autoparticipate_silence_with_other_trailing_punctuation_maps_to_none() {
        for text in ["Silence!", "silence?", "SILENCE.."] {
            let fake = Arc::new(FakeChatBackend::ok(text_turn(text)));
            let adapter = LlmAdapter::new(fake.clone(), "anthropic".to_string(), std::path::PathBuf::from("/test-config"));
            let history = vec![Message::user_text("skimble-human: ciao")];
            let reply = adapter.chat_autoparticipate("rumpleteazer-ai", &history, None).await;
            assert_eq!(reply, None, "\"{text}\" deve mappare a None: {reply:?}");
        }
    }

    /// Guardia di non-regressione: un testo che CONTIENE "silence" come sottostringa di
    /// una parola più lunga (non la parola isolata) non deve essere soppresso — solo la
    /// punteggiatura finale è tollerata, non un match parziale generico.
    #[tokio::test]
    async fn claude_autoparticipate_word_containing_silence_is_not_suppressed() {
        let fake = Arc::new(FakeChatBackend::ok(text_turn("Il silenzioso non è la stessa cosa.")));
        let adapter = LlmAdapter::new(fake.clone(), "anthropic".to_string(), std::path::PathBuf::from("/test-config"));
        let history = vec![Message::user_text("skimble-human: ciao")];
        let reply = adapter.chat_autoparticipate("rumpleteazer-ai", &history, None).await;
        assert_eq!(
            reply,
            Some("Il silenzioso non è la stessa cosa.".to_string()),
            "una parola che contiene \"silen\" come prefisso non e' 'SILENCE' + punteggiatura: {reply:?}"
        );
    }

    /// Se il modello contribuisce con un commento genuino (non "SILENCE"), il testo
    /// arriva intatto come `Some(..)`. Copre anche IL BUG OSSERVATO: una history che
    /// contiene una riga con l'auto-identificazione (errata) di un altro partecipante
    /// non deve influenzare il system prompt calcolato — che porta SEMPRE label+provider
    /// del chiamante, indipendentemente dal contenuto della history.
    #[tokio::test]
    async fn claude_autoparticipate_text_maps_to_some() {
        let fake = Arc::new(FakeChatBackend::ok(text_turn(
            "Occhio: quel comando cancella la cartella senza conferma.",
        )));
        let adapter = LlmAdapter::new(fake.clone(), "anthropic".to_string(), std::path::PathBuf::from("/test-config"));

        // Riproduce lo scenario reale: la history contiene un'altra AI che si è
        // auto-identificata (correttamente, dal SUO punto di vista) come DeepSeek.
        let history = vec![
            Message::user_text("skimble-ai: sono DeepSeek, sviluppato da DeepSeek, opero su skimble."),
        ];
        let reply = adapter.chat_autoparticipate("rumpleteazer-ai", &history, None).await;

        assert_eq!(
            reply,
            Some("Occhio: quel comando cancella la cartella senza conferma.".to_string())
        );
        let req = fake.recorded();
        assert!(req.tools.is_empty(), "text-only: nessun tool: {:?}", req.tools);
        assert!(
            req.system.contains("SILENCE"),
            "il system prompt dedicato deve istruire sull'opzione del silenzio: {}",
            req.system
        );
        assert!(
            req.system.contains("rumpleteazer-ai") && req.system.contains("anthropic"),
            "il system prompt deve ancorare l'identità del chiamante (rumpleteazer-ai/anthropic), \
             non quella letta nella history (skimble-ai/DeepSeek): {}",
            req.system
        );
        assert!(
            !req.system.contains("run_in_session"),
            "chat_autoparticipate non deve usare il system prompt del cursore: {}",
            req.system
        );
    }

    #[test]
    fn memory_file_path_is_config_dir_join_memory_label_md() {
        let p = memory_file_path(std::path::Path::new("C:/Lare/Configuration"), "rumpleteazer");
        assert_eq!(p, PathBuf::from("C:/Lare/Configuration").join("memory-rumpleteazer.md"));
    }

    #[test]
    fn needs_ai_name_prompt_true_when_enabled_and_no_ai_name() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("network.json");
        let cfg = crate::aichat::config::AiChatConfig { enabled: true, ..Default::default() };
        crate::aichat::config::save(&path, &cfg).unwrap();
        assert!(needs_ai_name_prompt_at(&path));
    }

    #[test]
    fn needs_ai_name_prompt_false_when_ai_already_has_a_name() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("network.json");
        let cfg = crate::aichat::config::AiChatConfig {
            enabled: true,
            ai_display_name: Some("Aria".to_string()),
            ..Default::default()
        };
        crate::aichat::config::save(&path, &cfg).unwrap();
        assert!(!needs_ai_name_prompt_at(&path));
    }

    #[test]
    fn needs_ai_name_prompt_false_when_aichat_disabled() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("network.json");
        let cfg = crate::aichat::config::AiChatConfig { enabled: false, ..Default::default() };
        crate::aichat::config::save(&path, &cfg).unwrap();
        assert!(!needs_ai_name_prompt_at(&path));
    }

    #[test]
    fn needs_ai_name_prompt_false_when_file_missing() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("network.json"); // mai scritto
        assert!(!needs_ai_name_prompt_at(&path));
    }

    // --- `apply_ai_name_addendum`: guardia contro i canali con
    // `system_prompt_override` (es. mcp-nmap) — vedi il doc-comment sulla
    // funzione per il perché. Pura, nessun I/O: niente `tempfile`/env qui,
    // a differenza dei test sopra su `needs_ai_name_prompt_at`.

    #[test]
    fn ai_name_addendum_skipped_when_channel_has_system_prompt_override() {
        let out = apply_ai_name_addendum("BASE".to_string(), true, true);
        assert_eq!(out, "BASE", "un canale con override non deve mai ricevere il nudge");
    }

    #[test]
    fn ai_name_addendum_skipped_when_not_needed() {
        let out = apply_ai_name_addendum("BASE".to_string(), false, false);
        assert_eq!(out, "BASE");
    }

    #[test]
    fn ai_name_addendum_appended_when_needed_and_no_override() {
        let out = apply_ai_name_addendum("BASE".to_string(), false, true);
        assert_eq!(out, format!("BASE{AI_NAME_REQUEST_ADDENDUM}"));
    }

    #[test]
    fn chat_system_prompt_without_memory_still_instructs_on_marker() {
        let prompt = chat_system_prompt("rumpleteazer-ai", "anthropic", None);
        assert!(
            prompt.contains("MEMORIA:"),
            "anche senza memoria esistente, il prompt deve spiegare il marker: {prompt}"
        );
        assert!(
            !prompt.contains("hai già scelto di ricordare"),
            "senza contenuto non deve comparire il blocco \"memoria esistente\": {prompt}"
        );
    }

    #[test]
    fn chat_system_prompt_with_memory_includes_it_verbatim() {
        let prompt = chat_system_prompt(
            "rumpleteazer-ai",
            "anthropic",
            Some("Il companion preferisce risposte concise."),
        );
        assert!(
            prompt.contains("Il companion preferisce risposte concise."),
            "il contenuto della memoria deve comparire verbatim nel prompt: {prompt}"
        );
        assert!(prompt.contains("MEMORIA:"));
    }

    #[test]
    fn autoparticipate_system_prompt_with_memory_includes_it_verbatim() {
        let prompt = autoparticipate_system_prompt(
            "rumpleteazer-ai",
            "anthropic",
            Some("Nota di prova."),
        );
        assert!(prompt.contains("Nota di prova."));
        assert!(prompt.contains("MEMORIA:"));
        assert!(prompt.contains("SILENCE"), "resta il prompt di auto-partecipazione: {prompt}");
    }

    /// Prova end-to-end (Docs/superpowers/specs/2026-07-16-tool-isolation-design.md):
    /// un `ToolClient` di canale (`FixtureChannelToolClient`) fa sì che
    /// `LlmAdapter::respond` (1) mandi al backend SOLO i 2 tool del canale —
    /// mai run_in_session/open_target/show_markdown; (2) usi il
    /// `system_prompt_override` del canale, non `agent::SYSTEM_PROMPT`; (3)
    /// dispacci con successo un tool_use del canale.
    #[tokio::test]
    async fn channel_tool_client_restricts_tools_and_uses_own_system_prompt() {
        let fake = Arc::new(FakeChatBackend::sequence(vec![
            Ok(tool_use_turn("tu1", "fixture_tool_a", serde_json::json!({}))),
            Ok(text_turn("fatto")),
        ]));
        let adapter = LlmAdapter::new(fake.clone(), "claude-sonnet-4-6".to_string(), std::path::PathBuf::from("/test-config"));
        let mut hist = ConversationHistory::new();
        let tools = FixtureChannelToolClient;

        let msgs = collect(|tx| adapter.respond(
            "c1", "fai qualcosa", &mut hist, &tools,
            TurnOptions {
                allow_windows: true,
                web_search: false,
                format_invocation: None,
                system_prompt_override: Some("SEI IL CANALE FIXTURE, hai solo fixture_tool_a/b"),
                lang: None,
            },
            None, None, tx,
        )).await;

        // (1) Il backend ha ricevuto SOLO i 2 tool del canale.
        let req = fake.recorded();
        assert_eq!(req.tools.len(), 2, "atteso 2 tool del canale, got {:?}", req.tools);
        assert!(req.tools.iter().any(|t| t.name() == "fixture_tool_a"));
        assert!(req.tools.iter().any(|t| t.name() == "fixture_tool_b"));
        assert!(
            !req.tools.iter().any(|t| t.name() == "run_in_session"),
            "run_in_session non deve mai comparire per un canale: {:?}", req.tools
        );
        assert!(
            !req.tools.iter().any(|t| t.name() == "show_markdown"),
            "show_markdown non deve mai comparire per un canale: {:?}", req.tools
        );

        // (2) Il system prompt è quello del canale, non il default.
        assert_eq!(req.system, "SEI IL CANALE FIXTURE, hai solo fixture_tool_a/b");

        // (3) Il tool del canale è stato dispacciato con successo: il loop
        // arriva a Done senza nessun messaggio di errore nel transcript.
        assert!(msgs.iter().any(|m| matches!(m, ServerMsg::Done { .. })));
        let joined: String = msgs.iter().filter_map(|m| match m {
            ServerMsg::Chunk { content, .. } => Some(content.clone()),
            _ => None,
        }).collect();
        assert!(!joined.contains("tool sconosciuto"), "fixture_tool_a è nel menu del canale: {joined}");
    }

    /// Anche se il modello (per errore/allucinazione) invocasse `run_in_session`
    /// — un nome MAI offerto a questo canale — il dispatch del canale lo
    /// rifiuta come "sconosciuto": il gate di isolamento non dipende dal buon
    /// comportamento del modello, è strutturale nel `ToolClient` del canale.
    #[tokio::test]
    async fn channel_tool_client_rejects_off_menu_tool_use() {
        let fake = Arc::new(FakeChatBackend::sequence(vec![
            Ok(tool_use_turn("tu1", "run_in_session", serde_json::json!({"command": "dir"}))),
            Ok(text_turn("fatto")),
        ]));
        let adapter = LlmAdapter::new(fake.clone(), "claude-sonnet-4-6".to_string(), std::path::PathBuf::from("/test-config"));
        let mut hist = ConversationHistory::new();
        let tools = FixtureChannelToolClient;

        collect(|tx| adapter.respond(
            "c1", "fai qualcosa", &mut hist, &tools,
            TurnOptions { allow_windows: true, web_search: false, format_invocation: None, system_prompt_override: None, lang: None },
            None, None, tx,
        )).await;

        // Il loop deve proseguire (2 richieste, non bloccarsi sulla prima) —
        // il tool_result d'errore torna al modello, che risponde "fatto".
        assert_eq!(fake.request_count(), 2, "il loop deve proseguire con l'errore di dispatch, non bloccarsi");

        // Il secondo turno (quello successivo al tool_use rifiutato) porta
        // nella storia un ToolResult con is_error=true e il messaggio di rifiuto.
        let second_req = fake.nth_request(1);
        let tool_result = second_req.history.iter().find_map(|m| {
            m.content.iter().find_map(|b| match b {
                Block::ToolResult { content, is_error, .. } => Some((content.clone(), *is_error)),
                _ => None,
            })
        });
        let (content, is_error) = tool_result.expect("atteso un ToolResult nella storia del secondo turno");
        assert!(is_error, "run_in_session fuori menu deve essere is_error=true");
        assert!(content.contains("sconosciuto"), "messaggio di rifiuto atteso, got: {content}");
    }

    // ── DispatchOutcome::report → OpenWindow deterministico (mai SaveToLibrary) ──

    /// `ToolClient` di test minimale che produce un `ChannelReport` per un
    /// solo tool (`produces_report`) — non aggiunto a `tool_client.rs` perché
    /// serve SOLO a questo test, non a un canale reale.
    struct ReportProducingToolClient;

    #[async_trait::async_trait]
    impl ToolClient for ReportProducingToolClient {
        async fn run_in_session(&self, _command: &str, _progress_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>) -> crate::tool_client::CommandResult {
            crate::tool_client::CommandResult { stdout: String::new(), stderr: "non disponibile".to_string(), exit_code: -1, cwd: String::new() }
        }
        async fn reset_session(&self) {}
        async fn open_target(&self, _target: &str) -> crate::tool_client::OpenResult {
            crate::tool_client::OpenResult { ok: false, message: "non disponibile".to_string() }
        }
        async fn search_routines(&self, _query: Option<&str>) -> crate::tool_client::SearchRoutinesResult {
            crate::tool_client::SearchRoutinesResult { results: Vec::new(), error: Some("non disponibile".to_string()) }
        }
        async fn run_routine(&self, _name: &str, _args: Option<&str>) -> crate::tool_client::RunRoutineResult {
            crate::tool_client::RunRoutineResult { ok: false, message: "non disponibile".to_string(), stdout: String::new(), stderr: String::new(), exit_code: -1, cwd: String::new() }
        }
        fn tool_defs(&self) -> Vec<crate::messages_client::ToolDef> {
            vec![crate::messages_client::ToolDef {
                name: "produces_report".to_string(),
                description: "d".to_string(),
                input_schema: serde_json::json!({}),
            }]
        }
        async fn dispatch(&self, name: &str, _input: &serde_json::Value) -> crate::tool_client::DispatchOutcome {
            use crate::tool_client::{ChannelReport, DispatchOutcome};
            match name {
                "produces_report" => DispatchOutcome {
                    output: "riepilogo breve".to_string(),
                    is_error: false,
                    report: Some(ChannelReport { title: "Report di prova".to_string(), markdown: "# Report\ncontenuto".to_string(), defer_to_turn_end: false }),
                    channel_summary: None,
                },
                other => DispatchOutcome { output: format!("sconosciuto: {other}"), is_error: true, report: None, channel_summary: None },
            }
        }
    }

    #[tokio::test]
    async fn dispatch_report_opens_window_but_never_auto_saves() {
        let fake = Arc::new(FakeChatBackend::sequence(vec![
            Ok(tool_use_turn("tu1", "produces_report", serde_json::json!({}))),
            Ok(text_turn("fatto")),
        ]));
        let adapter = LlmAdapter::new(fake.clone(), "claude-sonnet-4-6".to_string(), std::path::PathBuf::from("/test-config"));
        let mut hist = ConversationHistory::new();
        let tools = ReportProducingToolClient;

        let msgs = collect(|tx| {
            adapter.respond("c1", "genera un report", &mut hist, &tools, TurnOptions::default(), None, None, tx)
        })
        .await;

        let open_window = msgs
            .iter()
            .find_map(|m| match m {
                ServerMsg::OpenWindow { title, kind, content } => Some((title.clone(), kind.clone(), content.clone())),
                _ => None,
            })
            .expect("atteso un OpenWindow per il report");
        assert_eq!(open_window.0, "Report di prova");
        assert_eq!(open_window.1, WindowKind::Markdown);
        assert!(open_window.2.contains("contenuto"));

        // Il salvataggio in Library è ora scelta dell'utente (pulsante "Salva"
        // esistente nella finestra), non più automatico — vedi il commento sopra
        // `if opts.allow_windows` nel codice di produzione.
    }

    // ── Report differito (`defer_to_turn_end: true`) — refinement 2026-07-23 ──
    //
    // A differenza di `ReportProducingToolClient` sopra (nmap-style, apertura
    // subito), questo fixture simula `stock_report`: un `channel_summary`
    // deterministico + un `ChannelReport` la cui finestra deve aprirsi SOLO a
    // fine turno, con dentro anche il testo che l'AI scrive nella risposta
    // successiva al tool_result (mai visibile nel pannello).

    struct DeferredReportToolClient;

    #[async_trait::async_trait]
    impl ToolClient for DeferredReportToolClient {
        async fn run_in_session(&self, _command: &str, _progress_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>) -> crate::tool_client::CommandResult {
            crate::tool_client::CommandResult { stdout: String::new(), stderr: "non disponibile".to_string(), exit_code: -1, cwd: String::new() }
        }
        async fn reset_session(&self) {}
        async fn open_target(&self, _target: &str) -> crate::tool_client::OpenResult {
            crate::tool_client::OpenResult { ok: false, message: "non disponibile".to_string() }
        }
        async fn search_routines(&self, _query: Option<&str>) -> crate::tool_client::SearchRoutinesResult {
            crate::tool_client::SearchRoutinesResult { results: Vec::new(), error: Some("non disponibile".to_string()) }
        }
        async fn run_routine(&self, _name: &str, _args: Option<&str>) -> crate::tool_client::RunRoutineResult {
            crate::tool_client::RunRoutineResult { ok: false, message: "non disponibile".to_string(), stdout: String::new(), stderr: String::new(), exit_code: -1, cwd: String::new() }
        }
        fn tool_defs(&self) -> Vec<crate::messages_client::ToolDef> {
            vec![crate::messages_client::ToolDef {
                name: "stock_report".to_string(),
                description: "d".to_string(),
                input_schema: serde_json::json!({}),
            }]
        }
        async fn dispatch(&self, name: &str, _input: &serde_json::Value) -> crate::tool_client::DispatchOutcome {
            use crate::tool_client::{ChannelReport, DispatchOutcome};
            match name {
                "stock_report" => DispatchOutcome {
                    output: "riassunto fattuale per l'AI".to_string(),
                    is_error: false,
                    channel_summary: Some("AAPL: prezzo 150, market cap 4,79T".to_string()),
                    report: Some(ChannelReport {
                        title: "Financial Markets — AAPL".to_string(),
                        markdown: "# AAPL\n\n## Fondamentale\ndati fattuali".to_string(),
                        defer_to_turn_end: true,
                    }),
                },
                other => DispatchOutcome { output: format!("sconosciuto: {other}"), is_error: true, report: None, channel_summary: None },
            }
        }
    }

    /// Il caso d'oro: il tool produce SUBITO il riassunto numerico (mostrato
    /// nel pannello), ma la finestra si apre solo dopo che l'AI ha scritto la
    /// sua analisi (narrativa/ipotesi) — quel testo finisce fuso nel
    /// documento, non nel pannello, e la finestra precede sempre il Done.
    #[tokio::test]
    async fn deferred_report_merges_ai_text_and_suppresses_channel_echo() {
        let fake = Arc::new(FakeChatBackend::sequence(vec![
            Ok(tool_use_turn("tu1", "stock_report", serde_json::json!({"ticker": "AAPL"}))),
            Ok(text_turn("## Narrativa di trend\n\nTesto di analisi AI.")),
        ]));
        let adapter = LlmAdapter::new(fake.clone(), "claude-sonnet-4-6".to_string(), std::path::PathBuf::from("/test-config"));
        let mut hist = ConversationHistory::new();
        let tools = DeferredReportToolClient;

        let msgs = collect(|tx| {
            adapter.respond("c1", "report AAPL", &mut hist, &tools, TurnOptions::default(), None, None, tx)
        })
        .await;

        // (a) il riassunto numerico compare nel pannello, subito.
        assert!(
            msgs.iter().any(|m| matches!(m, ServerMsg::Chunk { content, .. } if content.contains("AAPL: prezzo 150"))),
            "atteso il channel_summary come Chunk: {msgs:?}"
        );
        // (b) il testo dell'AI (sezioni 5-6) NON deve mai comparire come Chunk.
        assert!(
            !msgs.iter().any(|m| matches!(m, ServerMsg::Chunk { content, .. } if content.contains("Testo di analisi AI"))),
            "il testo dell'AI deve restare fuori dal pannello, va solo nella finestra: {msgs:?}"
        );
        // (c) esattamente UNA finestra, col documento fattuale + testo AI fusi.
        let open_windows: Vec<(String, String)> = msgs
            .iter()
            .filter_map(|m| match m {
                ServerMsg::OpenWindow { title, content, .. } => Some((title.clone(), content.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(open_windows.len(), 1, "atteso UNA sola finestra: {open_windows:?}");
        assert_eq!(open_windows[0].0, "Financial Markets — AAPL");
        assert!(open_windows[0].1.contains("## Fondamentale"), "manca il documento fattuale: {open_windows:?}");
        assert!(open_windows[0].1.contains("Testo di analisi AI"), "manca il testo AI fuso in coda: {open_windows:?}");
        // (d) la finestra precede sempre il Done (il client deve poterla vedere
        // prima che il turno si consideri concluso).
        let open_idx = msgs.iter().position(|m| matches!(m, ServerMsg::OpenWindow { .. })).expect("atteso un OpenWindow");
        let done_idx = msgs.iter().position(|m| matches!(m, ServerMsg::Done { .. })).expect("atteso un Done");
        assert!(open_idx < done_idx, "OpenWindow deve precedere Done: apertura={open_idx}, done={done_idx}");
    }

    /// Se il backend fallisce nel turno SUCCESSIVO al tool (l'AI non riesce a
    /// scrivere le sue sezioni), il documento fattuale — già prodotto con
    /// successo — deve comunque apparire, senza testo AI aggiunto: perderlo
    /// per un errore successivo sarebbe una regressione rispetto
    /// all'apertura immediata di prima di questo cambiamento.
    #[tokio::test]
    async fn deferred_report_still_opens_when_the_following_turn_errors() {
        let fake = Arc::new(FakeChatBackend::sequence(vec![
            Ok(tool_use_turn("tu1", "stock_report", serde_json::json!({"ticker": "AAPL"}))),
            Err(BackendError::Network("boom".to_string())),
        ]));
        let adapter = LlmAdapter::new(fake.clone(), "claude-sonnet-4-6".to_string(), std::path::PathBuf::from("/test-config"));
        let mut hist = ConversationHistory::new();
        let tools = DeferredReportToolClient;

        let msgs = collect(|tx| {
            adapter.respond("c1", "report AAPL", &mut hist, &tools, TurnOptions::default(), None, None, tx)
        })
        .await;

        let open_window = msgs.iter().find_map(|m| match m {
            ServerMsg::OpenWindow { title, content, .. } => Some((title.clone(), content.clone())),
            _ => None,
        });
        let (title, content) = open_window.expect("il documento fattuale deve apparire anche se il turno successivo fallisce");
        assert_eq!(title, "Financial Markets — AAPL");
        assert!(content.contains("## Fondamentale"));
    }

    /// Se l'utente annulla (Stop) DOPO che il tool ha prodotto un report in
    /// sospeso ma PRIMA che l'AI scriva le sue sezioni, la finestra non deve
    /// aprirsi — rispettare l'annullamento vale più di mostrare un documento
    /// parziale che l'utente non ha chiesto di vedere ora. Il token viene
    /// cancellato DENTRO `dispatch()` stesso (non da un task separato):
    /// deterministico, nessuna corsa fra due task — al ritorno di
    /// `dispatch()` il token È già cancellato, quindi il controllo in testa
    /// alla PROSSIMA iterazione del loop lo vede di sicuro.
    #[tokio::test]
    async fn deferred_report_is_dropped_on_cancel_before_final_text() {
        struct CancelingDeferredReportToolClient(CancellationToken);

        #[async_trait::async_trait]
        impl ToolClient for CancelingDeferredReportToolClient {
            async fn run_in_session(&self, _command: &str, _progress_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>) -> crate::tool_client::CommandResult {
                crate::tool_client::CommandResult { stdout: String::new(), stderr: "non disponibile".to_string(), exit_code: -1, cwd: String::new() }
            }
            async fn reset_session(&self) {}
            async fn open_target(&self, _target: &str) -> crate::tool_client::OpenResult {
                crate::tool_client::OpenResult { ok: false, message: "non disponibile".to_string() }
            }
            async fn search_routines(&self, _query: Option<&str>) -> crate::tool_client::SearchRoutinesResult {
                crate::tool_client::SearchRoutinesResult { results: Vec::new(), error: Some("non disponibile".to_string()) }
            }
            async fn run_routine(&self, _name: &str, _args: Option<&str>) -> crate::tool_client::RunRoutineResult {
                crate::tool_client::RunRoutineResult { ok: false, message: "non disponibile".to_string(), stdout: String::new(), stderr: String::new(), exit_code: -1, cwd: String::new() }
            }
            fn tool_defs(&self) -> Vec<crate::messages_client::ToolDef> {
                vec![crate::messages_client::ToolDef { name: "stock_report".to_string(), description: "d".to_string(), input_schema: serde_json::json!({}) }]
            }
            async fn dispatch(&self, name: &str, _input: &serde_json::Value) -> crate::tool_client::DispatchOutcome {
                use crate::tool_client::{ChannelReport, DispatchOutcome};
                self.0.cancel();
                match name {
                    "stock_report" => DispatchOutcome {
                        output: "riassunto fattuale".to_string(),
                        is_error: false,
                        channel_summary: Some("AAPL: prezzo 150".to_string()),
                        report: Some(ChannelReport {
                            title: "Financial Markets — AAPL".to_string(),
                            markdown: "# AAPL\n\n## Fondamentale\ndati".to_string(),
                            defer_to_turn_end: true,
                        }),
                    },
                    other => DispatchOutcome { output: format!("sconosciuto: {other}"), is_error: true, report: None, channel_summary: None },
                }
            }
        }

        let fake = Arc::new(FakeChatBackend::sequence(vec![
            Ok(tool_use_turn("tu1", "stock_report", serde_json::json!({"ticker": "AAPL"}))),
            Ok(text_turn("mai raggiunto")),
        ]));
        let adapter = LlmAdapter::new(fake.clone(), "claude-sonnet-4-6".to_string(), std::path::PathBuf::from("/test-config"));
        let mut hist = ConversationHistory::new();
        let token = CancellationToken::new();
        let tools = CancelingDeferredReportToolClient(token.clone());

        let msgs = collect(|tx| {
            adapter.respond("c1", "report AAPL", &mut hist, &tools, TurnOptions::default(), None, Some(token), tx)
        })
        .await;

        assert!(
            !msgs.iter().any(|m| matches!(m, ServerMsg::OpenWindow { .. })),
            "annullato prima del testo finale: nessuna finestra deve apparire: {msgs:?}"
        );
        assert_eq!(fake.request_count(), 1, "il loop deve fermarsi dopo il tool, non chiamare l'AI una seconda volta");
    }

    // ── list_screeners → ServerMsg::OpenScreenerPicker (2026-08-11) ─────────
    //
    // Stesso pattern di ReportProducingToolClient sopra, ma per l'eccezione
    // nominata "list_screeners" (mirror di show_markdown, ADR-013): il tool
    // NON produce un ChannelReport, produce direttamente un
    // ServerMsg::OpenScreenerPicker + un ToolResult con testo breve invece
    // del JSON grezzo.

    struct ScreenerListingToolClient;

    #[async_trait::async_trait]
    impl ToolClient for ScreenerListingToolClient {
        async fn run_in_session(&self, _command: &str, _progress_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>) -> crate::tool_client::CommandResult {
            crate::tool_client::CommandResult { stdout: String::new(), stderr: "non disponibile".to_string(), exit_code: -1, cwd: String::new() }
        }
        async fn reset_session(&self) {}
        async fn open_target(&self, _target: &str) -> crate::tool_client::OpenResult {
            crate::tool_client::OpenResult { ok: false, message: "non disponibile".to_string() }
        }
        async fn search_routines(&self, _query: Option<&str>) -> crate::tool_client::SearchRoutinesResult {
            crate::tool_client::SearchRoutinesResult { results: Vec::new(), error: Some("non disponibile".to_string()) }
        }
        async fn run_routine(&self, _name: &str, _args: Option<&str>) -> crate::tool_client::RunRoutineResult {
            crate::tool_client::RunRoutineResult { ok: false, message: "non disponibile".to_string(), stdout: String::new(), stderr: String::new(), exit_code: -1, cwd: String::new() }
        }
        fn tool_defs(&self) -> Vec<crate::messages_client::ToolDef> {
            vec![crate::messages_client::ToolDef {
                name: "list_screeners".to_string(),
                description: "d".to_string(),
                input_schema: serde_json::json!({}),
            }]
        }
        async fn dispatch(&self, name: &str, _input: &serde_json::Value) -> crate::tool_client::DispatchOutcome {
            use crate::tool_client::DispatchOutcome;
            match name {
                "list_screeners" => DispatchOutcome {
                    output: serde_json::json!({
                        "summary": "Ho aperto la finestra di selezione con 1 screener disponibile.",
                        "items": [{"id": "consumer-usage", "title": "Uso Consumer", "description": "Screening potenziale."}]
                    }).to_string(),
                    is_error: false,
                    report: None,
                    channel_summary: None,
                },
                other => DispatchOutcome { output: format!("sconosciuto: {other}"), is_error: true, report: None, channel_summary: None },
            }
        }
    }

    #[tokio::test]
    async fn list_screeners_emits_open_screener_picker_and_uses_short_summary() {
        let fake = Arc::new(FakeChatBackend::sequence(vec![
            Ok(tool_use_turn("tu1", "list_screeners", serde_json::json!({}))),
            Ok(text_turn("Ecco gli screener disponibili.")),
        ]));
        let adapter = LlmAdapter::new(fake.clone(), "claude-sonnet-4-6".to_string(), std::path::PathBuf::from("/test-config"));
        let mut hist = ConversationHistory::new();
        let tools = ScreenerListingToolClient;

        let msgs = collect(|tx| {
            adapter.respond("c1", "esegui uno screener", &mut hist, &tools, TurnOptions::default(), None, None, tx)
        })
        .await;

        let picker = msgs
            .iter()
            .find_map(|m| match m {
                ServerMsg::OpenScreenerPicker { items } => Some(items.clone()),
                _ => None,
            })
            .expect("atteso un OpenScreenerPicker");
        assert_eq!(picker.len(), 1);
        assert_eq!(picker[0].id, "consumer-usage");
        assert_eq!(picker[0].title, "Uso Consumer");

        // Il ToolResult per l'AI deve usare "summary" (breve), MAI il JSON grezzo.
        let tool_result_content = hist.to_vec().iter().find_map(|m| {
            m.content.iter().find_map(|b| match b {
                Block::ToolResult { content, .. } => Some(content.clone()),
                _ => None,
            })
        }).expect("atteso un ToolResult nella storia");
        assert_eq!(tool_result_content, "Ho aperto la finestra di selezione con 1 screener disponibile.");
        assert!(!tool_result_content.contains("report_markdown"), "il ToolResult non deve contenere il JSON grezzo");
    }

    #[tokio::test]
    async fn list_screeners_malformed_json_falls_back_to_raw_text_no_picker() {
        struct MalformedListingToolClient;
        #[async_trait::async_trait]
        impl ToolClient for MalformedListingToolClient {
            async fn run_in_session(&self, _c: &str, _p: Option<tokio::sync::mpsc::UnboundedSender<String>>) -> crate::tool_client::CommandResult {
                crate::tool_client::CommandResult { stdout: String::new(), stderr: "non disponibile".to_string(), exit_code: -1, cwd: String::new() }
            }
            async fn reset_session(&self) {}
            async fn open_target(&self, _t: &str) -> crate::tool_client::OpenResult {
                crate::tool_client::OpenResult { ok: false, message: "non disponibile".to_string() }
            }
            async fn search_routines(&self, _q: Option<&str>) -> crate::tool_client::SearchRoutinesResult {
                crate::tool_client::SearchRoutinesResult { results: Vec::new(), error: Some("non disponibile".to_string()) }
            }
            async fn run_routine(&self, _n: &str, _a: Option<&str>) -> crate::tool_client::RunRoutineResult {
                crate::tool_client::RunRoutineResult { ok: false, message: "non disponibile".to_string(), stdout: String::new(), stderr: String::new(), exit_code: -1, cwd: String::new() }
            }
            fn tool_defs(&self) -> Vec<crate::messages_client::ToolDef> {
                vec![crate::messages_client::ToolDef { name: "list_screeners".to_string(), description: "d".to_string(), input_schema: serde_json::json!({}) }]
            }
            async fn dispatch(&self, name: &str, _input: &serde_json::Value) -> crate::tool_client::DispatchOutcome {
                use crate::tool_client::DispatchOutcome;
                match name {
                    "list_screeners" => DispatchOutcome { output: "non è JSON valido".to_string(), is_error: false, report: None, channel_summary: None },
                    other => DispatchOutcome { output: format!("sconosciuto: {other}"), is_error: true, report: None, channel_summary: None },
                }
            }
        }

        let fake = Arc::new(FakeChatBackend::sequence(vec![
            Ok(tool_use_turn("tu1", "list_screeners", serde_json::json!({}))),
            Ok(text_turn("fatto")),
        ]));
        let adapter = LlmAdapter::new(fake.clone(), "claude-sonnet-4-6".to_string(), std::path::PathBuf::from("/test-config"));
        let mut hist = ConversationHistory::new();
        let tools = MalformedListingToolClient;

        let msgs = collect(|tx| {
            adapter.respond("c1", "esegui uno screener", &mut hist, &tools, TurnOptions::default(), None, None, tx)
        })
        .await;

        assert!(
            !msgs.iter().any(|m| matches!(m, ServerMsg::OpenScreenerPicker { .. })),
            "JSON malformato non deve MAI produrre un OpenScreenerPicker"
        );
        let tool_result_content = hist.to_vec().iter().find_map(|m| {
            m.content.iter().find_map(|b| match b {
                Block::ToolResult { content, .. } => Some(content.clone()),
                _ => None,
            })
        }).expect("atteso un ToolResult nella storia");
        assert_eq!(tool_result_content, "non è JSON valido", "testo grezzo invariato sul fallback");
    }

    // ── TDD RED: flatten_for_inline_review ───────────────────────────────────
    //
    // Test scritti PRIMA dell'implementazione della funzione.

    #[test]
    fn flatten_for_inline_review_includes_all_fields() {
        let req = RoutineSaveRequest {
            name: "list-big-files".to_string(),
            description: "Elenca i file grandi".to_string(),
            tags: vec!["files".to_string(), "disk".to_string()],
            category: "files".to_string(),
            script: "Get-ChildItem -Recurse".to_string(),
            replace: None,
        };
        let flat = flatten_for_inline_review(&req);
        assert!(flat.contains("list-big-files"));
        assert!(flat.contains("Elenca i file grandi"));
        assert!(flat.contains("files, disk"));
        assert!(flat.contains("Get-ChildItem -Recurse"));
        assert!(!flat.contains("Sostituisce"), "nessuna riga replace quando None: {flat}");
    }

    #[test]
    fn flatten_for_inline_review_shows_replace_when_some() {
        let req = RoutineSaveRequest {
            name: "n".to_string(),
            description: "d".to_string(),
            tags: vec![],
            category: "c".to_string(),
            script: "s".to_string(),
            replace: Some("old-name".to_string()),
        };
        let flat = flatten_for_inline_review(&req);
        assert!(flat.contains("old-name"));
    }

    // ── TDD RED: ToolConfirmer::confirm_routine_save default ─────────────────
    //
    // Test scritto PRIMA dell'aggiunta del metodo al trait.

    /// Un confirmer che NON sovrascrive `confirm_routine_save` deve comunque
    /// funzionare, cadendo sul default (appiattisce + chiama `confirm()`).
    /// `RecordingConfirmer` implementa SOLO `confirm`/`should_gate` — se
    /// questo compila e passa, il default esiste ed è corretto.
    struct RecordingConfirmer {
        seen: std::sync::Mutex<Vec<String>>,
    }

    #[async_trait]
    impl ToolConfirmer for RecordingConfirmer {
        async fn confirm(&self, command: &str) -> bool {
            self.seen.lock().unwrap().push(command.to_string());
            true
        }
        fn should_gate(&self, _tool_name: &str) -> bool {
            true
        }
    }

    #[tokio::test]
    async fn confirm_routine_save_default_falls_back_to_confirm_with_flattened_text() {
        let confirmer = RecordingConfirmer { seen: std::sync::Mutex::new(Vec::new()) };
        let req = RoutineSaveRequest {
            name: "n".to_string(),
            description: "d".to_string(),
            tags: vec![],
            category: "c".to_string(),
            script: "Write-Host hi".to_string(),
            replace: None,
        };

        let ok = confirmer.confirm_routine_save(&req).await;

        assert!(ok);
        let seen = confirmer.seen.lock().unwrap();
        assert_eq!(seen.len(), 1, "il default deve chiamare confirm() esattamente una volta");
        assert!(seen[0].contains("Write-Host hi"), "atteso il testo appiattito passato a confirm(): {:?}", seen[0]);
    }
}
