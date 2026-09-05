# Piano 2a — Protocollo shell e orchestratore (Rust + ui)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** una connessione WS con `role: "shell"` può essere guidata da un client di test
(automatico in `ws_integration.rs`, manuale con `scripts/dev/shell-client.mjs`): `/ai "…"` apre la
finestra Markdown di output su `ui.exe`, chiede `[Y/n]`, esegue nella shell del client via
`ExecInShell`/`ExecResult` e chiude con `Done`; `/ping` misura i tre strati; slash ignoti sono
scartati in silenzio.

**Architecture:** protocollo additivo nel crate `protocol` (`Hello.role/session_id/cwd/version`,
`ExecInShell`/`ExecResult`, `OpenOutputWindow`/`OutputWindowContent`, `OpenUiLocal`,
`UiPing`/`UiPong`, `ActivityIndicator`, `ServerMsg::surface()`); nell'orchestratore un **registro
delle connessioni** (sink `ui` unico + sessioni shell), un `ShellSessionToolClient` (il tool
`run_in_session` diventa un round-trip `ExecInShell`→`ExecResult` sulla connessione shell), un
**router di superficie** per turno (buffer dei `Chunk` → contenuto della finestra a `Done`, il
resto verso `ui` o verso la shell), un pre-router per i comandi slash della shell e il built-in
`/ping`; in `ui` una finestra di output aggiornabile, `OpenUiLocal`, `/help` singleton, `UiPong`.
Il piano **2b** (host C# `lare-shell`) consuma questo protocollo: è un piano separato, scritto dopo
il merge di questo.

**Tech Stack:** Rust 2021 (tokio, serde, tokio-tungstenite 0.24, rand 0.8), Tauri 2 vanilla JS
(`node:test` per i moduli puri), Node 26 (`WebSocket` globale) per il client di sviluppo.

**Spec:** `Docs/i18n/ita/superpowers/specs/2026-09-04-lare-terminal-2-design.md` — §3 (routing),
§3.1 (`/ping`), §3.2 (finestra di output, D14), §4 (protocollo), §4.5 (`capture`), §4.6 (cwd per
sessione, D17), §5 (singleton D15, `OpenUiLocal`), §6.4 (autostart: **qui solo il pezzo
"nessun sink ui → riga di avviso"**; l'avvio di `ui.exe` è del piano 3), §8 (sicurezza), §10 (test).

## Global Constraints

Copiate dallo spec; ogni task le include implicitamente.

- **Additività del protocollo** (§4.1): i client v1 restano validi. `Hello` senza `role` → `ui`;
  ogni campo nuovo di `Hello` è `#[serde(default)]`; nessuna variante esistente cambia forma.
- **Gate di conferma** (§8): "ogni comando proposto dall'AI passa dal gate ADR-007 prima di
  eseguire" — sul canale shell **`run_in_session` e `open_target` sono gateizzati** (il
  `LocalUiConfirmer` v1 gateizza solo nmap/routine: NON va riusato tal quale per la shell).
  Nessun `ExecInShell` parte senza `ToolConfirmResponse{accept:true}`.
- **cwd per sessione** (§4.6, D17): l'orchestratore tiene la cwd **per connessione shell**
  (`Command.cwd` la inizializza, ogni `ExecResult.cwd` la aggiorna) e **non tocca mai** il
  `cwd_state` globale v1 (che resta della shell posseduta da mcp-server, per Telegram/AI Chat).
  Su una connessione shell non si emette mai `ServerMsg::Cwd`.
- **Output in finestra** (§3.2, D14): per un comando slash originato dalla shell, il terminale
  riceve solo: `ToolConfirmRequest`, `ExecInShell`, una riga di conferma finale (`Chunk`) e
  `Done`, oppure `Error`. Tutto il testo (chunk AI token-per-token **e** chunk di trasparenza)
  è bufferizzato e consegnato una volta con `OutputWindowContent` a `Done`/`Error`.
- **Sink `ui` unico**: è la connessione con `role == Ui` **e** `channel == None` (stesso predicato
  di `connection_owns_plugin_sink`); le connessioni `window.js`/`external-channel-window.js`
  (con `channel`) e le connessioni shell **non** lo sono. Le sessioni shell non ricevono mai
  `SetServerTx` del servizio AI Chat.
- **Slash ignoto dalla shell** (§3): scartato con `Done` muto + `tracing::info!` (`discard slash`);
  da Telegram resta il comportamento v1 (`Error`). "Conosciuto" viene da **una sola lista** per
  il backend (`core::KNOWN_BACKEND_SLASHES`) più i plugin e i canali esterni: mai duplicare i
  bracci del `match` di `handle_slash`.
- **`/ai "testo"` ≡ `/ "testo"`**, virgolette obbligatorie (D8): senza → `Error` con il testo
  `sintassi: /ai "testo" (virgolette obbligatorie)`.
- **Versioni**: `protocol` 2.0.0 → **2.1.0**, `orchestrator` 2.0.2 → **2.1.0**, `ui` 2.0.3 →
  **2.1.0** (bump nel task di release, con `CHANGELOG.md`/`IMPLEMENTATION.md` di ogni crate).
- **Convenzioni v1** (§11): TDD con RED reale; commenti didattici in italiano; trait come confini,
  fake ai seam; `cargo fmt` **solo sui file toccati** (il codice copiato dalla v1 non è fmt-clean:
  chore separata); niente variabili d'ambiente; WS solo `127.0.0.1` + token su file.
- **Fuori scope qui**: host C# (piano 2b), autostart di `ui.exe`/orchestratore (piano 3),
  `/find` e `/nowin` dalla shell (scartati con log: debito documentato in HANDOFF),
  streaming nella finestra (§12).

**Baseline verde a `main` = `a3d08df`** (da riverificare al Task 1, step 0): Rust default-members
1418 test, `ui` 87 + 49, JS 219, Python 339. `protocol` ha 75 test, `ws_integration.rs` 17.

**Modelli** (nota per il controller): implementer **Sonnet** per ogni task Rust/JS (il codice
è nel piano ma tocca file grandi con integrazione), **Haiku** solo per il Task 11 (docs/release);
reviewer Sonnet; re-review Haiku; review finale Opus. Un `BLOCKED` sul Task 8 (wiring di
`ws.rs`) va rifatto direttamente su Opus.

## Mappa dei file

| File | Responsabilità | Task |
|---|---|---|
| `crates/protocol/src/lib.rs` | `Role`, campi nuovi di `Hello`, 2 `ClientMsg` + 6 `ServerMsg` nuovi, `Surface` + `ServerMsg::surface()` | 1 |
| `crates/orchestrator/src/connections.rs` (nuovo) | `Registry` (sink ui, sessioni shell, ping ui pendenti) | 2 |
| `crates/orchestrator/src/local_confirm.rs` | `ShellConfirmer` (gate su tutto tranne `show_markdown`, riusa la meccanica di `LocalUiConfirmer`) | 3 |
| `crates/orchestrator/src/agent.rs` | `display_invocation`: suffisso `(interattivo)` | 3 |
| `crates/orchestrator/src/shell_session.rs` (nuovo) | `ShellSessionState` (cwd, exec pendenti), `ShellSessionToolClient`, `shell_tool_defs()` | 4 |
| `crates/orchestrator/src/surface.rs` (nuovo) | `ShellTurn`, `route_shell_turn` (buffer + instradamento per superficie), `output_window_title` | 5 |
| `crates/orchestrator/src/shell_slash.rs` (nuovo) | `classify_shell_input`, tabella canali, `read_web_search_enabled` | 6 |
| `crates/orchestrator/src/core.rs` | `KNOWN_BACKEND_SLASHES`, `HELP_MARKDOWN` 2.0 | 6 |
| `crates/orchestrator/src/plugins/host.rs` | `wait_ready` estratto, `find`, `probe` | 7 |
| `crates/orchestrator/src/ping.rs` (nuovo) + `lib.rs` | `run_ping`, `format_uptime`, `PROCESS_START` | 7 |
| `crates/orchestrator/src/ws.rs`, `main.rs`, `tests/ws_integration.rs` | wiring: `HelloInfo`, registro, ramo shell del `Command`, `ExecResult`/`UiPong`, teardown | 8 |
| `crates/ui/src-tauri/src/main.rs`, `frontend/{host.js, host-dispatch.mjs, ui-local.mjs, window.js, ws-client.js}` | finestra di output aggiornabile, `OpenUiLocal`, `/help` singleton, `UiPong` | 9 |
| `scripts/dev/shell-client.mjs`, `tests/ws_integration.rs` | client di sviluppo + scenari e2e automatici | 10 |
| `Docs/i18n/ita/*`, `CHANGELOG.md`/`IMPLEMENTATION.md`, `Cargo.toml` | ADR-018, HANDOFF, RUN-LOCAL, TESTING-e2e, bump versioni | 11 |

---

### Task 1: `protocol` — ruolo della connessione, messaggi nuovi, `ServerMsg::surface()`

**Files:**
- Modify: `crates/protocol/src/lib.rs` (enum `Role` e `Surface` dopo `WindowKind`; `Hello`
  a riga ~155; nuove varianti in coda a `ClientMsg` e `ServerMsg`; `impl ServerMsg` prima di
  `#[cfg(test)]`)
- Modify: `crates/orchestrator/src/ws.rs:790-798` (`parse_hello`: pattern con `..`)
- Modify: `crates/orchestrator/tests/ws_integration.rs` (17 costruzioni di `ClientMsg::Hello`)
- Test: `crates/protocol/src/lib.rs` (modulo `tests` esistente)

**Interfaces:**
- Produces: `protocol::Role { Ui, Shell }` (`Default = Ui`, wire `"ui"|"shell"`);
  `ClientMsg::Hello { token, channel, role, session_id, cwd, version }`;
  `ClientMsg::ExecResult { turn_id, exec_id, exit_code: i32, output, cwd }`;
  `ClientMsg::UiPong { id, version }`;
  `ServerMsg::{ExecInShell{turn_id, exec_id, command, capture: bool}, OpenOutputWindow{window_id, title}, OutputWindowContent{window_id, markdown}, OpenUiLocal{name}, UiPing{id}, ActivityIndicator{session_id, kind, on: bool}}`;
  `protocol::Surface { Origin, Ui }`; `ServerMsg::surface(&self) -> Surface`.

- [ ] **Step 0: baseline**

Run: `cargo test -p protocol 2>&1 | grep "test result"` → `75 passed`. Poi
`cargo test 2>&1 | grep "test result" | awk '{s+=$4} END {print s}'` → `1418`.

- [ ] **Step 1: test RED (serde + surface)**

Aggiungere in fondo a `mod tests` di `crates/protocol/src/lib.rs`:

```rust
    // ── Piano 2a: ruolo della connessione e messaggi host↔orchestratore (spec §4.1) ──

    /// Un client v1 manda `Hello` senza `role`: deve restare valido e valere `ui`.
    #[test]
    fn hello_without_role_defaults_to_ui() {
        let msg: ClientMsg = serde_json::from_str(r#"{"type":"hello","token":"t"}"#).unwrap();
        match msg {
            ClientMsg::Hello { token, channel, role, session_id, cwd, version } => {
                assert_eq!(token, "t");
                assert_eq!(channel, None);
                assert_eq!(role, Role::Ui);
                assert_eq!(session_id, None);
                assert_eq!(cwd, None);
                assert_eq!(version, None);
            }
            other => panic!("atteso Hello, ricevuto {other:?}"),
        }
    }

    #[test]
    fn hello_shell_round_trips_all_new_fields() {
        let msg = ClientMsg::Hello {
            token: "t".into(),
            channel: None,
            role: Role::Shell,
            session_id: Some("a1b2".into()),
            cwd: Some("C:\\Users\\x".into()),
            version: Some("2.0.0".into()),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""role":"shell""#), "wire: {json}");
        let back: ClientMsg = serde_json::from_str(&json).unwrap();
        assert_eq!(back, msg);
    }

    #[test]
    fn exec_in_shell_and_exec_result_wire_names() {
        let s = ServerMsg::ExecInShell {
            turn_id: "t1".into(), exec_id: "e1".into(), command: "dir".into(), capture: true,
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.starts_with(r#"{"type":"exec_in_shell""#), "wire: {json}");
        let c: ClientMsg = serde_json::from_str(
            r#"{"type":"exec_result","turn_id":"t1","exec_id":"e1","exit_code":0,"output":"x","cwd":"C:\\"}"#,
        ).unwrap();
        assert_eq!(c, ClientMsg::ExecResult {
            turn_id: "t1".into(), exec_id: "e1".into(), exit_code: 0, output: "x".into(), cwd: "C:\\".into(),
        });
    }

    #[test]
    fn output_window_ui_local_ping_indicator_wire_names() {
        let cases = vec![
            (ServerMsg::OpenOutputWindow { window_id: "w".into(), title: "T".into() }, "open_output_window"),
            (ServerMsg::OutputWindowContent { window_id: "w".into(), markdown: "# x".into() }, "output_window_content"),
            (ServerMsg::OpenUiLocal { name: "config".into() }, "open_ui_local"),
            (ServerMsg::UiPing { id: "p".into() }, "ui_ping"),
            (ServerMsg::ActivityIndicator { session_id: "s".into(), kind: "ai_busy".into(), on: true }, "activity_indicator"),
        ];
        for (msg, wire) in cases {
            let json = serde_json::to_string(&msg).unwrap();
            assert!(json.contains(&format!(r#""type":"{wire}""#)), "wire: {json}");
            let back: ServerMsg = serde_json::from_str(&json).unwrap();
            assert_eq!(back, msg);
        }
        let pong: ClientMsg = serde_json::from_str(r#"{"type":"ui_pong","id":"p","version":"2.1.0"}"#).unwrap();
        assert_eq!(pong, ClientMsg::UiPong { id: "p".into(), version: "2.1.0".into() });
    }

    /// `surface()`: ciò che torna alla connessione che ha emesso il comando
    /// (`Origin`) contro ciò che apre/aggiorna finestre su `ui.exe` (`Ui`).
    /// Il `match` in `surface()` è esaustivo senza wildcard: una variante
    /// nuova senza riga nella tabella NON compila — questo test copre solo un
    /// campione rappresentativo per superficie.
    #[test]
    fn surface_origin_for_turn_messages_and_ui_for_window_messages() {
        let id = || "c1".to_string();
        let origin = vec![
            ServerMsg::Chunk { id: id(), content: "x".into() },
            ServerMsg::Done { id: id(), exit_code: None },
            ServerMsg::Error { id: id(), code: ErrCode::RoutingError, message: "m".into() },
            ServerMsg::Pong { ts: 1 },
            ServerMsg::Cwd { path: "C:\\".into() },
            ServerMsg::Heartbeat { id: id() },
            ServerMsg::ToolConfirmRequest { id: id(), commands: "$ dir".into() },
            ServerMsg::ExecInShell { turn_id: id(), exec_id: "e".into(), command: "dir".into(), capture: true },
            ServerMsg::MarketDataSourceTestResult { id: id(), ok: true, message: String::new() },
        ];
        for m in origin {
            assert_eq!(m.surface(), Surface::Origin, "{m:?}");
        }
        let ui = vec![
            ServerMsg::OpenWindow { title: "t".into(), kind: WindowKind::Markdown, content: "c".into() },
            ServerMsg::SearchOpen { id: id(), title: "t".into() },
            ServerMsg::OpenPluginWindow { window_id: 1, title: "t".into(), html: "<p/>".into(), width: None, height: None },
            ServerMsg::AiChatRoster { participants: vec![] },
            ServerMsg::NotesSnapshot { notes: vec![] },
            ServerMsg::OpenOutputWindow { window_id: "w".into(), title: "T".into() },
            ServerMsg::OutputWindowContent { window_id: "w".into(), markdown: "m".into() },
            ServerMsg::OpenUiLocal { name: "library".into() },
            ServerMsg::UiPing { id: id() },
            ServerMsg::ActivityIndicator { session_id: "s".into(), kind: "ai_busy".into(), on: false },
        ];
        for m in ui {
            assert_eq!(m.surface(), Surface::Ui, "{m:?}");
        }
    }
```

- [ ] **Step 2: RED**

Run: `cargo test -p protocol 2>&1 | grep -E "^error|no variant|cannot find" | head`
Expected: errori di compilazione (`Role`, `Surface`, varianti e campi inesistenti).

- [ ] **Step 3: implementazione**

Dopo `WindowKind` (riga ~78) aggiungere:

```rust
/// Ruolo di una connessione WS (spec §4.1). Deciso dal client nella `Hello`.
///
/// - `Ui`: `ui.exe` (pagina host nascosta, finestre) — è il default per
///   compatibilità: un client v1 non manda il campo e resta valido.
/// - `Shell`: una sessione `lare-shell` (host PowerShell). Il suo `ToolClient`
///   è la shell dell'utente stesso (`ExecInShell`/`ExecResult`), la sua cwd è
///   per-connessione, e l'output dei comandi slash va in una finestra su `ui`.
///
/// Wire: `"ui"` | `"shell"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    #[default]
    Ui,
    Shell,
}

/// Superficie di destinazione di un `ServerMsg` emesso durante un turno
/// originato da una connessione **shell** (spec §3.2/§4). Per una connessione
/// `ui` non cambia nulla: tutto torna alla connessione stessa, come in v1.
///
/// - `Origin`: torna alla connessione che ha mandato il `Command` (la shell):
///   avanzamento del turno, gate, esecuzioni, risposte a richieste puntuali.
/// - `Ui`: apre o aggiorna finestre, o alimenta un relay (AI Chat, Library,
///   Share): va al sink `ui` unico della macchina.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    Origin,
    Ui,
}
```

`Hello` diventa (sostituire la variante intera):

```rust
    /// Handshake. MUST be the first message after connecting.
    Hello {
        token: String,
        /// Canale esterno richiesto da questa connessione (es. `"nmap"`).
        /// `None` = comportamento di sempre (cursore/Telegram, ToolClient di
        /// default). Vedi `Docs/superpowers/specs/2026-07-16-external-tool-channel-design.md`.
        #[serde(default)]
        channel: Option<String>,
        /// Ruolo della connessione (2.0, spec §4.1). Assente → `Ui`.
        #[serde(default)]
        role: Role,
        /// Id della sessione shell (generato dalla host, o passato da `ui.exe`
        /// con `--session`): lega la connessione alla finestra terminale.
        /// Solo per `role: Shell`; `ui` lo lascia assente.
        #[serde(default)]
        session_id: Option<String>,
        /// cwd iniziale della sessione shell (`$PWD` del runspace all'avvio).
        #[serde(default)]
        cwd: Option<String>,
        /// Versione del client (es. `lare-shell 2.0.0`), mostrata da `/ping`.
        #[serde(default)]
        version: Option<String>,
    },
```

In coda a `ClientMsg` (dopo `TestMarketDataSource`):

```rust
    // ── Canale shell (2.0, spec §4.1) — additivi ─────────────────────────
    // Wire names: ExecResult → "exec_result", UiPong → "ui_pong".

    /// Esito di un `ServerMsg::ExecInShell`: la host ha eseguito `command`
    /// nel runspace dell'utente. `output` è vuoto con `capture: false`
    /// (§4.5); `cwd` è `$PWD` dopo il comando (aggiorna la cwd per sessione).
    ExecResult { turn_id: String, exec_id: String, exit_code: i32, output: String, cwd: String },

    /// Risposta di `ui.exe` a `ServerMsg::UiPing` (built-in `/ping`, §3.1).
    UiPong { id: String, version: String },
```

In coda a `ServerMsg` (dopo `MarketDataSourceTestResult`):

```rust
    // ── Canale shell (2.0, spec §3.2/§4.1) — additivi ────────────────────
    // Wire names: exec_in_shell, open_output_window, output_window_content,
    // open_ui_local, ui_ping, activity_indicator.

    /// Esegui `command` nella shell dell'utente (solo verso `role: Shell`,
    /// SEMPRE dopo un `ToolConfirmRequest` accettato — spec §8). `capture`
    /// (§4.5): `true` = output catturato e restituito in `ExecResult.output`;
    /// `false` = console attaccata (programmi interattivi), output vuoto.
    ExecInShell { turn_id: String, exec_id: String, command: String, capture: bool },

    /// Apre su `ui` la finestra Markdown di output di un comando slash
    /// originato dalla shell, con un segnaposto ("in corso…").
    OpenOutputWindow { window_id: String, title: String },

    /// Sostituisce il contenuto della finestra `window_id` (a `Done`/`Error`).
    OutputWindowContent { window_id: String, markdown: String },

    /// Chiede a `ui` di aprire (o portare in primo piano, D15) una finestra
    /// locale: `"config"`, `"library"`, `"aichat"`, oppure l'id di un canale
    /// esterno (`"nmap"`, `"financial-markets"`, `"python-ping"`).
    OpenUiLocal { name: String },

    /// Richiesta di vita a `ui.exe` (built-in `/ping`); risposta `UiPong`.
    UiPing { id: String },

    /// Segnalino di stato per la finestra terminale della sessione
    /// (`kind`: `"ai_busy"`); consumato dal piano 3, già emesso qui.
    ActivityIndicator { session_id: String, kind: String, on: bool },
```

Prima di `#[cfg(test)]`:

```rust
impl ServerMsg {
    /// Superficie di destinazione durante un turno originato da una shell
    /// (vedi [`Surface`]). Il `match` è esaustivo **senza wildcard** di
    /// proposito: chi aggiunge una variante deve decidere dove va, e il
    /// compilatore glielo ricorda.
    pub fn surface(&self) -> Surface {
        use ServerMsg::*;
        match self {
            // Avanzamento del turno e risposte puntuali: alla connessione origine.
            ServerInfo { .. } | Chunk { .. } | Done { .. } | Error { .. } | Pong { .. }
            | Cwd { .. } | Heartbeat { .. } | ToolConfirmRequest { .. }
            | RoutineSavePreview { .. } | MarketDataSourceTestResult { .. }
            | ExecInShell { .. } => Surface::Origin,
            // Finestre e relay: al sink `ui` della macchina.
            OpenWindow { .. } | OpenScreenerPicker { .. } | SearchOpen { .. }
            | SearchHit { .. } | SearchDone { .. } | OpenPluginWindow { .. }
            | UpdatePluginWindow { .. } | ClosePluginWindow { .. }
            | AiChatMessage { .. } | AiChatRoster { .. } | AiChatJoinRequest { .. }
            | AiChatHistory { .. } | AiChatSelf { .. } | AiChatPeerLost { .. }
            | AiChatReachablePeers { .. } | NotesSnapshot { .. } | NoteUpserted { .. }
            | ShareRequest { .. } | ShareResult { .. } | ShareContentRequest { .. }
            | ShareIncomingData { .. } | AiChatJoinPrompt { .. }
            | AiChatAdmissionRequest { .. } | AiChatPending { .. } | AiChatAdmitted { .. }
            | AiChatRejected { .. } | AiChatAdmissionResolved { .. }
            | OpenOutputWindow { .. } | OutputWindowContent { .. } | OpenUiLocal { .. }
            | UiPing { .. } | ActivityIndicator { .. } => Surface::Ui,
        }
    }
}
```

Se il compilatore segnala una variante mancante (l'elenco sopra è stato estratto dal file a
`main`), aggiungerla alla riga giusta secondo la regola dei due commenti.

- [ ] **Step 4: aggiornare i consumatori di `Hello`**

`crates/orchestrator/src/ws.rs` `parse_hello` (riga ~790): il pattern
`ClientMsg::Hello { token, channel } => Some((token, channel)),` diventa
`ClientMsg::Hello { token, channel, .. } => Some((token, channel)),` (il Task 8 lo sostituirà con
`HelloInfo`; qui basta compilare). In `crates/orchestrator/tests/ws_integration.rs` ogni
costruzione `ClientMsg::Hello { token: …, channel: … }` riceve i quattro campi
`role: protocol::Role::Ui, session_id: None, cwd: None, version: None,` (17 occorrenze; usare
`grep -n "ClientMsg::Hello {" crates/orchestrator/tests/ws_integration.rs` per trovarle).
Verificare con `grep -rn "ClientMsg::Hello {" crates --include=*.rs` che non restino altre
costruzioni (a `main` sono solo in `ws.rs`, `ws_integration.rs` e `protocol`).

- [ ] **Step 5: GREEN**

Run: `cargo test -p protocol 2>&1 | grep "test result"` → `80 passed`.
Run: `cargo test 2>&1 | grep "test result" | awk '{s+=$4} END {print s}'` → `1423`.
Run: `cargo build -p ui 2>&1 | tail -1` → `Finished` (`ui` dipende da `protocol`).
Run: `cargo clippy -p protocol -p orchestrator --all-targets 2>&1 | grep -c "^warning" ` → stesso
numero di `main` (nessun warning nuovo nei file toccati).

- [ ] **Step 6: docs del crate + commit**

`crates/protocol/CHANGELOG.md`: voce `2.1.0 (in lavorazione)` con l'elenco dei messaggi nuovi;
`crates/protocol/IMPLEMENTATION.md`: paragrafo "Ruolo della connessione e superficie".

```bash
git add crates/protocol crates/orchestrator/src/ws.rs crates/orchestrator/tests/ws_integration.rs
git commit -m "feat(protocol): ruolo della connessione, messaggi del canale shell, ServerMsg::surface()"
```

---

### Task 2: `connections.rs` — registro delle connessioni (sink `ui`, sessioni shell, ping pendenti)

**Files:**
- Create: `crates/orchestrator/src/connections.rs`
- Modify: `crates/orchestrator/src/lib.rs` (`pub mod connections;` in ordine alfabetico)
- Test: modulo `tests` nello stesso file

**Interfaces:**
- Produces: `Registry::{new, shared, set_ui_sink, clear_ui_sink_if, ui_sink, register_shell, unregister_shell, shell_count, register_ui_ping, resolve_ui_ping}`,
  `SharedRegistry = Arc<tokio::sync::Mutex<Registry>>`.

- [ ] **Step 1: test RED**

```rust
// crates/orchestrator/src/connections.rs
//! # connections — registro delle connessioni WS vive (2.0, spec §4/§5)
//!
//! In v1 ogni connessione era un mondo a sé: `ws.rs` non sapeva quante ce
//! ne fossero né di che tipo. Nel 2.0 esistono due ruoli (`protocol::Role`)
//! e l'orchestratore deve poter **raggiungere** `ui.exe` da un turno
//! originato da una shell (aprire la finestra di output, chiedere un
//! `UiPong`). Questo modulo è l'unico posto che conosce "chi è connesso":
//!
//! - il **sink `ui`**: la connessione `role: Ui` senza `channel` (una sola
//!   per macchina; l'ultima vince, come per il sink dei plugin);
//! - le **sessioni shell**, per `session_id` (piano 3: push di segnalini);
//! - i **ping `ui` pendenti** (`UiPing{id}` → `oneshot` risolto da `UiPong`).
//!
//! È puro stato + `tokio::sync` (nessun I/O): testabile senza socket.
//! In termini OOP: un registry/service locator posseduto da `main()`,
//! condiviso via `Arc<Mutex<_>>` con ogni `handle_connection`.

use std::collections::HashMap;
use std::sync::Arc;

use protocol::ServerMsg;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::{oneshot, Mutex};

/// Handle condiviso: `ws::serve` lo riceve da `main()` e lo clona per ogni
/// connessione. `tokio::sync::Mutex` perché viene tenuto attraverso `await`
/// solo per operazioni brevissime (mai attraverso una send di rete).
pub type SharedRegistry = Arc<Mutex<Registry>>;

#[derive(Default)]
pub struct Registry {
    ui_sink: Option<UnboundedSender<ServerMsg>>,
    shells: HashMap<String, UnboundedSender<ServerMsg>>,
    ui_pings: HashMap<String, oneshot::Sender<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc::unbounded_channel;

    #[test]
    fn ui_sink_is_absent_until_set_and_cleared_only_by_same_sender() {
        let mut r = Registry::new();
        assert!(r.ui_sink().is_none());
        let (a, _ra) = unbounded_channel::<ServerMsg>();
        let (b, _rb) = unbounded_channel::<ServerMsg>();
        r.set_ui_sink(a.clone());
        assert!(r.ui_sink().is_some());
        // Una connessione DIVERSA che si chiude non deve azzerare il sink corrente.
        assert!(!r.clear_ui_sink_if(&b));
        assert!(r.ui_sink().is_some());
        assert!(r.clear_ui_sink_if(&a));
        assert!(r.ui_sink().is_none());
    }

    #[test]
    fn last_ui_sink_wins_like_plugin_sink() {
        let mut r = Registry::new();
        let (a, _ra) = unbounded_channel::<ServerMsg>();
        let (b, mut rb) = unbounded_channel::<ServerMsg>();
        r.set_ui_sink(a);
        r.set_ui_sink(b);
        r.ui_sink().unwrap().send(ServerMsg::UiPing { id: "x".into() }).unwrap();
        assert!(matches!(rb.try_recv(), Ok(ServerMsg::UiPing { .. })));
    }

    #[test]
    fn shells_are_registered_by_session_id() {
        let mut r = Registry::new();
        let (a, _ra) = unbounded_channel::<ServerMsg>();
        r.register_shell("s1", a);
        assert_eq!(r.shell_count(), 1);
        r.unregister_shell("s1");
        assert_eq!(r.shell_count(), 0);
        r.unregister_shell("mai-esistita"); // no-op, niente panic
    }

    #[tokio::test]
    async fn ui_ping_is_resolved_once_with_the_version() {
        let mut r = Registry::new();
        let rx = r.register_ui_ping("p1");
        assert!(r.resolve_ui_ping("p1", "2.1.0".into()));
        assert_eq!(rx.await.unwrap(), "2.1.0");
        // Seconda risoluzione (o id ignoto): no-op, ritorna false.
        assert!(!r.resolve_ui_ping("p1", "x".into()));
        assert!(!r.resolve_ui_ping("ignoto", "x".into()));
    }
}
```

- [ ] **Step 2: RED** — `cargo test -p orchestrator connections 2>&1 | grep -E "^error" | head -3`
→ metodi inesistenti.

- [ ] **Step 3: implementazione** (fra lo struct e `#[cfg(test)]`)

```rust
impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Costruttore di comodo per `main()`/test: registro vuoto già condivisibile.
    pub fn shared() -> SharedRegistry {
        Arc::new(Mutex::new(Self::new()))
    }

    /// Registra (o sostituisce) il sink `ui`. L'ultima connessione `ui`
    /// senza canale vince — stessa regola di `PluginHost::set_server_tx`.
    pub fn set_ui_sink(&mut self, tx: UnboundedSender<ServerMsg>) {
        self.ui_sink = Some(tx);
    }

    /// Azzera il sink SOLO se è ancora quello di `tx` (`same_channel`): una
    /// connessione `ui` vecchia che si chiude dopo che una nuova ha preso il
    /// posto non deve lasciare la macchina senza sink. Ritorna `true` se ha
    /// azzerato davvero.
    pub fn clear_ui_sink_if(&mut self, tx: &UnboundedSender<ServerMsg>) -> bool {
        match &self.ui_sink {
            Some(current) if current.same_channel(tx) => {
                self.ui_sink = None;
                true
            }
            _ => false,
        }
    }

    pub fn ui_sink(&self) -> Option<UnboundedSender<ServerMsg>> {
        self.ui_sink.clone()
    }

    pub fn register_shell(&mut self, session_id: &str, tx: UnboundedSender<ServerMsg>) {
        self.shells.insert(session_id.to_string(), tx);
    }

    pub fn unregister_shell(&mut self, session_id: &str) {
        self.shells.remove(session_id);
    }

    pub fn shell_count(&self) -> usize {
        self.shells.len()
    }

    /// Registra un `UiPing{id}` in attesa; il chiamante manda il messaggio
    /// al sink e attende il receiver (con timeout, vedi `ping.rs`).
    pub fn register_ui_ping(&mut self, id: &str) -> oneshot::Receiver<String> {
        let (tx, rx) = oneshot::channel();
        self.ui_pings.insert(id.to_string(), tx);
        rx
    }

    /// Risolve il ping `id` con la versione dichiarata da `ui.exe`.
    /// Id ignoto o già risolto → `false` (risposta tardiva scartata, stesso
    /// principio di `PendingConfirms::resolve`).
    pub fn resolve_ui_ping(&mut self, id: &str, version: String) -> bool {
        match self.ui_pings.remove(id) {
            Some(tx) => tx.send(version).is_ok(),
            None => false,
        }
    }
}
```

- [ ] **Step 4: GREEN** — `cargo test -p orchestrator connections 2>&1 | grep "test result"` →
`4 passed`. `cargo clippy -p orchestrator --all-targets 2>&1 | grep connections` → vuoto.

- [ ] **Step 5: commit**

```bash
git add crates/orchestrator/src/connections.rs crates/orchestrator/src/lib.rs
git commit -m "feat(orchestrator): registro delle connessioni (sink ui, sessioni shell, ping ui)"
```

---

### Task 3: `ShellConfirmer` e etichetta `(interattivo)`

**Files:**
- Modify: `crates/orchestrator/src/local_confirm.rs` (dopo `impl ToolConfirmer for LocalUiConfirmer`, riga ~215)
- Modify: `crates/orchestrator/src/agent.rs:253-260` (`display_invocation`)
- Test: moduli `tests` dei due file

**Interfaces:**
- Consumes: `LocalUiConfirmer::new(out_tx, pending, timeout, command_id)`, `PendingConfirms`.
- Produces: `pub(crate) struct ShellConfirmer` + `ShellConfirmer::new(out_tx, pending, timeout, command_id)`;
  `display_invocation("run_in_session", {"command":"x","interactive":true})` → `"$ x  (interattivo)"`.

- [ ] **Step 1: test RED**

In fondo a `mod tests` di `local_confirm.rs`:

```rust
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
```

(Se `RoutineSaveRequest` ha campi diversi da quelli elencati, allineare il test allo struct in
`ai_adapter.rs`; il contratto testato è "arriva un `ToolConfirmRequest`, non un
`RoutineSavePreview`".)

In `mod tests` di `agent.rs`:

```rust
    #[test]
    fn display_invocation_marks_interactive_run_in_session() {
        let plain = display_invocation("run_in_session", &serde_json::json!({"command": "vim x"}));
        assert_eq!(plain, "$ vim x");
        let inter = display_invocation(
            "run_in_session",
            &serde_json::json!({"command": "vim x", "interactive": true}),
        );
        assert_eq!(inter, "$ vim x  (interattivo)");
    }
```

- [ ] **Step 2: RED** — `cargo test -p orchestrator shell_confirmer 2>&1 | grep -E "^error" | head -3`
e `cargo test -p orchestrator display_invocation_marks 2>&1 | grep -E "FAILED|panicked" | head -2`.

- [ ] **Step 3: implementazione**

`local_confirm.rs`, dopo l'`impl ToolConfirmer for LocalUiConfirmer`:

```rust
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
pub(crate) struct ShellConfirmer {
    inner: LocalUiConfirmer,
}

impl ShellConfirmer {
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
```

`agent.rs` `display_invocation`, braccio `"run_in_session"`:

```rust
        "run_in_session" => {
            let cmd = input.get("command").and_then(|v| v.as_str()).unwrap_or("");
            // `interactive` (2.0, spec §4.5): il prompt `[Y/n]` deve dichiarare che
            // l'output non verrà catturato — l'utente decide sapendolo.
            if input.get("interactive").and_then(|v| v.as_bool()).unwrap_or(false) {
                format!("$ {cmd}  (interattivo)")
            } else {
                format!("$ {cmd}")
            }
        }
```

- [ ] **Step 4: GREEN** — `cargo test -p orchestrator shell_confirmer display_invocation_marks 2>&1 | grep "test result"`
→ `4 passed`. Poi `cargo test -p orchestrator 2>&1 | grep "test result"` → nessun fallimento.

- [ ] **Step 5: commit**

```bash
git add crates/orchestrator/src/local_confirm.rs crates/orchestrator/src/agent.rs
git commit -m "feat(orchestrator): ShellConfirmer (gate su run_in_session/open_target) e etichetta (interattivo)"
```

---

### Task 4: `shell_session.rs` — la shell dell'utente come `ToolClient`

**Files:**
- Create: `crates/orchestrator/src/shell_session.rs`
- Modify: `crates/orchestrator/src/lib.rs` (`pub mod shell_session;`)
- Test: modulo `tests` nello stesso file

**Interfaces:**
- Consumes: `ToolClient` (trait, `tool_client.rs:186`), `CommandResult`, `OpenResult`,
  `SearchRoutinesResult`, `RunRoutineResult`, `GetRoutineContentResult`, `SaveRoutineResult`,
  `DispatchOutcome` (tutti in `tool_client.rs`), `agent::{tool_defs, dispatch_tool_at}`,
  `messages_client::ToolDef`, `protocol::{ClientMsg, ServerMsg}`.
- Produces: `ShellSessionState::{new, session_id, cwd, set_cwd, exec, resolve_exec, abort_turn, abort_all}`,
  `ExecReply { exit_code, output, cwd }`, `ShellSessionToolClient::for_turn(&Arc<ShellSessionState>, turn_id) -> Arc<dyn ToolClient>`,
  `shell_tool_defs() -> Vec<ToolDef>`.

- [ ] **Step 1: test RED**

```rust
// crates/orchestrator/src/shell_session.rs
//! # shell_session — la shell dell'utente come `ToolClient` (2.0, spec §4)
//!
//! In v1 `run_in_session` girava nella shell **posseduta** da mcp-server
//! (modello A). Nel 2.0 il canale shell è la sessione PowerShell dell'utente
//! stesso (modello B): il tool diventa un round-trip sul WebSocket —
//! `ServerMsg::ExecInShell{exec_id}` → la host esegue → `ClientMsg::ExecResult
//! {exec_id}` — correlato da un `oneshot` per `exec_id`.
//!
//! Due oggetti, per separare ciò che vive quanto la connessione da ciò che
//! vive quanto un turno:
//! - [`ShellSessionState`]: per **connessione** (cwd della sessione, mappa
//!   delle esecuzioni pendenti, canale d'uscita, `McpToolClient` per i tool
//!   che non riguardano la shell). Creato in `ws.rs` dopo la `Hello`.
//! - [`ShellSessionToolClient`]: per **turno** (conosce il `turn_id` da
//!   scrivere in ogni `ExecInShell`). Implementa `ToolClient`; è ciò che
//!   `core::handle_command` riceve come `tools`.
//!
//! La cwd è **per sessione** (D17): non tocca mai il `cwd_state` globale v1.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use protocol::ServerMsg;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::{oneshot, Mutex};

use crate::messages_client::ToolDef;
use crate::tool_client::{
    CommandResult, DispatchOutcome, GetRoutineContentResult, OpenResult, RunRoutineResult,
    SaveRoutineResult, SearchRoutinesResult, ToolClient,
};

/// Esito di un `ExecInShell`, come arriva in `ClientMsg::ExecResult`.
#[derive(Debug, Clone, PartialEq)]
pub struct ExecReply {
    pub exit_code: i32,
    pub output: String,
    pub cwd: String,
}

/// Messaggio restituito all'AI quando un'esecuzione non può concludersi
/// (connessione shell chiusa, turno annullato con Ctrl+C).
pub const EXEC_ABORTED: &str = "esecuzione interrotta: sessione shell chiusa o turno annullato";

pub struct ShellSessionState {
    session_id: String,
    out_tx: UnboundedSender<ServerMsg>,
    cwd: Mutex<String>,
    /// `exec_id → (turn_id, sender)`: il `turn_id` permette di abortire tutte
    /// le esecuzioni di UN turno (Ctrl+C) senza toccare le altre.
    pending: Mutex<HashMap<String, (String, oneshot::Sender<ExecReply>)>>,
    /// Tool che NON riguardano la shell dell'utente (`open_target`, routine):
    /// delegati al `McpToolClient` condiviso, come per ogni altra connessione.
    mcp: Arc<dyn ToolClient>,
    config_dir: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_client::FakeToolClient;
    use tokio::sync::mpsc::unbounded_channel;

    fn state() -> (Arc<ShellSessionState>, tokio::sync::mpsc::UnboundedReceiver<ServerMsg>) {
        let (tx, rx) = unbounded_channel();
        let mcp: Arc<dyn ToolClient> = Arc::new(FakeToolClient::success("mcp output\n"));
        let s = Arc::new(ShellSessionState::new("s1", tx, "C:\\start", mcp, PathBuf::from("/cfg")));
        (s, rx)
    }

    /// `run_in_session` manda `ExecInShell{turn_id, capture: true}` e torna
    /// SOLO quando arriva l'`ExecResult` con lo stesso `exec_id`; la cwd della
    /// sessione viene aggiornata da `ExecResult.cwd`.
    #[tokio::test]
    async fn run_in_session_round_trips_exec_in_shell_and_updates_cwd() {
        let (s, mut rx) = state();
        let tools = ShellSessionToolClient::for_turn(&s, "t1".into());
        let run = tokio::spawn(async move { tools.run_in_session("dir", None).await });
        let msg = rx.recv().await.unwrap();
        let exec_id = match msg {
            ServerMsg::ExecInShell { turn_id, exec_id, command, capture } => {
                assert_eq!(turn_id, "t1");
                assert_eq!(command, "dir");
                assert!(capture);
                exec_id
            }
            other => panic!("atteso ExecInShell, ricevuto {other:?}"),
        };
        assert!(s.resolve_exec(&exec_id, ExecReply { exit_code: 0, output: "a.txt\n".into(), cwd: "C:\\dopo".into() }).await);
        let r = run.await.unwrap();
        assert_eq!((r.exit_code, r.stdout.as_str(), r.cwd.as_str()), (0, "a.txt\n", "C:\\dopo"));
        assert_eq!(s.cwd().await, "C:\\dopo");
    }

    /// `dispatch("run_in_session", {interactive: true})` → `capture: false` (§4.5).
    #[tokio::test]
    async fn dispatch_interactive_sends_capture_false() {
        let (s, mut rx) = state();
        let tools = ShellSessionToolClient::for_turn(&s, "t1".into());
        let run = tokio::spawn(async move {
            tools.dispatch("run_in_session", &serde_json::json!({"command": "python", "interactive": true})).await
        });
        let exec_id = match rx.recv().await.unwrap() {
            ServerMsg::ExecInShell { exec_id, capture, .. } => {
                assert!(!capture);
                exec_id
            }
            other => panic!("ricevuto {other:?}"),
        };
        s.resolve_exec(&exec_id, ExecReply { exit_code: 0, output: String::new(), cwd: String::new() }).await;
        let out = run.await.unwrap();
        assert!(!out.is_error);
        assert_eq!(out.output, "(nessun output, exit_code=0)");
    }

    /// Ctrl+C sulla host (`CancelCommand`) → `abort_turn`: l'`await` pendente
    /// torna subito con exit_code -1 e il testo `EXEC_ABORTED`.
    #[tokio::test]
    async fn abort_turn_releases_pending_exec_with_error() {
        let (s, mut rx) = state();
        let tools = ShellSessionToolClient::for_turn(&s, "t1".into());
        let run = tokio::spawn(async move { tools.run_in_session("sleep 100", None).await });
        let _ = rx.recv().await.unwrap();
        s.abort_turn("t1").await;
        let r = run.await.unwrap();
        assert_eq!(r.exit_code, -1);
        assert_eq!(r.stdout, EXEC_ABORTED);
        // Un ExecResult tardivo per l'exec abortito è scartato.
        assert!(!s.resolve_exec("qualunque", ExecReply { exit_code: 0, output: String::new(), cwd: String::new() }).await);
    }

    /// Una cwd vuota nell'`ExecResult` (host che non la riporta) non cancella
    /// quella nota — stessa regola di `CwdTrackingToolClient`.
    #[tokio::test]
    async fn empty_cwd_in_result_keeps_previous_cwd() {
        let (s, mut rx) = state();
        let tools = ShellSessionToolClient::for_turn(&s, "t1".into());
        let run = tokio::spawn(async move { tools.run_in_session("dir", None).await });
        let exec_id = match rx.recv().await.unwrap() {
            ServerMsg::ExecInShell { exec_id, .. } => exec_id,
            other => panic!("ricevuto {other:?}"),
        };
        s.resolve_exec(&exec_id, ExecReply { exit_code: 1, output: String::new(), cwd: String::new() }).await;
        run.await.unwrap();
        assert_eq!(s.cwd().await, "C:\\start");
    }

    /// I tool esposti all'AI: quelli v1 SENZA `run_routine`, e `run_in_session`
    /// con l'input opzionale `interactive`.
    #[test]
    fn shell_tool_defs_drop_run_routine_and_add_interactive() {
        let defs = shell_tool_defs();
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(!names.contains(&"run_routine"), "{names:?}");
        assert!(names.contains(&"run_in_session"));
        assert!(names.contains(&"open_target"));
        assert!(names.contains(&"show_markdown"));
        let ris = defs.iter().find(|d| d.name == "run_in_session").unwrap();
        assert!(ris.input_schema["properties"]["interactive"].is_object());
        assert_eq!(ris.input_schema["required"], serde_json::json!(["command"]));
    }

    /// `run_routine` non esiste sulla shell: rifiuto esplicito, mai un panic.
    #[tokio::test]
    async fn run_routine_is_rejected_and_other_tools_delegate_to_mcp() {
        let (s, _rx) = state();
        let tools = ShellSessionToolClient::for_turn(&s, "t1".into());
        let r = tools.run_routine("x", None).await;
        assert!(!r.ok);
        assert!(r.message.contains("non disponibile"));
        let o = tools.open_target("C:\\x").await;
        assert!(o.ok, "open_target delega a McpToolClient (fake ok)");
    }
}
```

- [ ] **Step 2: RED** — `cargo test -p orchestrator shell_session 2>&1 | grep -E "^error" | head -3`.

- [ ] **Step 3: implementazione** (fra lo struct e `#[cfg(test)]`)

```rust
impl ShellSessionState {
    pub fn new(
        session_id: impl Into<String>,
        out_tx: UnboundedSender<ServerMsg>,
        initial_cwd: impl Into<String>,
        mcp: Arc<dyn ToolClient>,
        config_dir: PathBuf,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            out_tx,
            cwd: Mutex::new(initial_cwd.into()),
            pending: Mutex::new(HashMap::new()),
            mcp,
            config_dir,
        }
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub async fn cwd(&self) -> String {
        self.cwd.lock().await.clone()
    }

    /// Aggiorna la cwd della sessione; una stringa vuota (host che non la
    /// riporta) lascia il valore precedente.
    pub async fn set_cwd(&self, cwd: &str) {
        if !cwd.is_empty() {
            *self.cwd.lock().await = cwd.to_string();
        }
    }

    /// Manda `ExecInShell` e attende l'`ExecResult` corrispondente.
    /// Non ha timeout di proposito (spec §4.4): un comando può durare
    /// quanto vuole; è Ctrl+C (`abort_turn`) o la chiusura della connessione
    /// (`abort_all`) a sbloccarlo.
    pub async fn exec(&self, turn_id: &str, command: &str, capture: bool) -> ExecReply {
        let exec_id = format!("{:032x}", rand::random::<u128>());
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(exec_id.clone(), (turn_id.to_string(), tx));
        let msg = ServerMsg::ExecInShell {
            turn_id: turn_id.to_string(),
            exec_id: exec_id.clone(),
            command: command.to_string(),
            capture,
        };
        if self.out_tx.send(msg).is_err() {
            // Connessione già chiusa: niente attesa, e via l'entry orfana.
            self.pending.lock().await.remove(&exec_id);
            return aborted();
        }
        match rx.await {
            Ok(reply) => {
                self.set_cwd(&reply.cwd).await;
                reply
            }
            Err(_) => aborted(), // sender droppato da abort_turn/abort_all
        }
    }

    /// Risolve l'esecuzione `exec_id`. `false` = id ignoto o già risolto.
    pub async fn resolve_exec(&self, exec_id: &str, reply: ExecReply) -> bool {
        match self.pending.lock().await.remove(exec_id) {
            Some((_turn, tx)) => tx.send(reply).is_ok(),
            None => false,
        }
    }

    /// Abortisce le esecuzioni pendenti del turno `turn_id` (Ctrl+C sulla
    /// host: la pipeline è già stata fermata lì, nessun `ExecResult` arriverà).
    pub async fn abort_turn(&self, turn_id: &str) {
        self.pending.lock().await.retain(|_, (t, _)| t != turn_id);
    }

    /// Abortisce tutto (teardown della connessione).
    pub async fn abort_all(&self) {
        self.pending.lock().await.clear();
    }
}

fn aborted() -> ExecReply {
    ExecReply { exit_code: -1, output: EXEC_ABORTED.to_string(), cwd: String::new() }
}

/// I tool esposti all'AI su una sessione shell: quelli storici
/// (`agent::tool_defs()`) meno `run_routine` (la routine girerebbe nella shell
/// posseduta da mcp-server, non in quella dell'utente — fuorviante), con
/// `run_in_session` ridescritto e arricchito dell'input `interactive` (§4.5).
/// Costruito trasformando la lista v1, non ricopiandola: descrizioni e schemi
/// degli altri tool restano una cosa sola.
pub fn shell_tool_defs() -> Vec<ToolDef> {
    crate::agent::tool_defs()
        .into_iter()
        .filter(|t| t.name != "run_routine")
        .map(|mut t| {
            if t.name == "run_in_session" {
                t.description = "Esegui un comando nella sessione PowerShell dell'utente (la sua shell: cwd ed env persistono, l'output compare nel suo terminale). Usa per qualsiasi operazione fattibile da terminale.".to_string();
                t.input_schema = serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": { "type": "string", "description": "Il comando da eseguire" },
                        "interactive": { "type": "boolean", "description": "true se il programma ha bisogno di un terminale interattivo (editor, REPL, pager, wizard): l'output non viene catturato e non ti torna indietro; restano exit_code e cwd. Default false." }
                    },
                    "required": ["command"]
                });
            }
            t
        })
        .collect()
}

/// `ToolClient` di UN turno di una sessione shell (vedi doc-comment del modulo).
pub struct ShellSessionToolClient {
    state: Arc<ShellSessionState>,
    turn_id: String,
}

impl ShellSessionToolClient {
    pub fn for_turn(state: &Arc<ShellSessionState>, turn_id: String) -> Arc<dyn ToolClient> {
        Arc::new(Self { state: Arc::clone(state), turn_id })
    }

    async fn exec_to_outcome(&self, command: &str, capture: bool) -> DispatchOutcome {
        let r = self.state.exec(&self.turn_id, command, capture).await;
        let output = if r.output.is_empty() {
            format!("(nessun output, exit_code={})", r.exit_code)
        } else {
            r.output
        };
        DispatchOutcome { output, is_error: r.exit_code != 0, report: None, channel_summary: None }
    }
}

#[async_trait]
impl ToolClient for ShellSessionToolClient {
    /// `progress_tx` è ignorato: l'output arriva in blocco con l'`ExecResult`
    /// (l'utente lo vede già scorrere nel suo terminale, spec §3.2).
    async fn run_in_session(
        &self,
        command: &str,
        _progress_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
    ) -> CommandResult {
        let r = self.state.exec(&self.turn_id, command, true).await;
        CommandResult { stdout: r.output, stderr: String::new(), exit_code: r.exit_code, cwd: r.cwd }
    }

    /// La sessione è dell'utente: non c'è nulla da riavviare (spec §3, `/reset`).
    async fn reset_session(&self) {}

    async fn open_target(&self, target: &str) -> OpenResult {
        self.state.mcp.open_target(target).await
    }

    async fn search_routines(&self, query: Option<&str>) -> SearchRoutinesResult {
        self.state.mcp.search_routines(query).await
    }

    async fn run_routine(&self, _name: &str, _args: Option<&str>) -> RunRoutineResult {
        RunRoutineResult {
            ok: false,
            message: "run_routine non disponibile nella sessione shell: usa run_in_session".to_string(),
            stdout: String::new(),
            stderr: String::new(),
            exit_code: -1,
            cwd: String::new(),
        }
    }

    async fn get_routine_content(&self, name: &str) -> GetRoutineContentResult {
        self.state.mcp.get_routine_content(name).await
    }

    async fn save_routine(
        &self,
        name: &str,
        description: &str,
        tags: Vec<String>,
        category: &str,
        content: &str,
        replace: Option<&str>,
    ) -> SaveRoutineResult {
        self.state.mcp.save_routine(name, description, tags, category, content, replace).await
    }

    fn tool_defs(&self) -> Vec<ToolDef> {
        shell_tool_defs()
    }

    async fn dispatch(&self, name: &str, input: &serde_json::Value) -> DispatchOutcome {
        if name == "run_in_session" {
            // Intercettato QUI (non in `agent::dispatch_tool_at`) perché solo la
            // shell conosce `interactive` → `capture` (§4.5).
            let command = input.get("command").and_then(|v| v.as_str()).unwrap_or("");
            let interactive = input.get("interactive").and_then(|v| v.as_bool()).unwrap_or(false);
            return self.exec_to_outcome(command, !interactive).await;
        }
        // Tutto il resto: stessa via di `CwdTrackingToolClient::dispatch` — su
        // `self`, così un tool che rientra passa da questo impl.
        crate::agent::dispatch_tool_at(self as &dyn ToolClient, name, input, &self.state.config_dir.join("network.json")).await
    }
}
```

Se `FakeToolClient::success` non produce `open_target` con `ok: true`, sostituire nel test
l'asserzione con quella che il fake garantisce (leggere `tool_client.rs:311-360`); il contratto è
"delega a `mcp`".

- [ ] **Step 4: GREEN** — `cargo test -p orchestrator shell_session 2>&1 | grep "test result"` →
`6 passed`. `cargo clippy -p orchestrator --all-targets 2>&1 | grep shell_session` → vuoto.

- [ ] **Step 5: commit**

```bash
git add crates/orchestrator/src/shell_session.rs crates/orchestrator/src/lib.rs
git commit -m "feat(orchestrator): ShellSessionToolClient — run_in_session come ExecInShell/ExecResult"
```

---

### Task 5: `surface.rs` — il router di superficie di un turno shell

**Files:**
- Create: `crates/orchestrator/src/surface.rs`
- Modify: `crates/orchestrator/src/lib.rs` (`pub mod surface;`)
- Test: modulo `tests` nello stesso file

**Interfaces:**
- Consumes: `protocol::{ServerMsg, Surface}`.
- Produces: `ShellTurn { id, session_id, window_id, title }`,
  `route_shell_turn(rx: UnboundedReceiver<ServerMsg>, shell: UnboundedSender<ServerMsg>, ui: Option<UnboundedSender<ServerMsg>>, turn: ShellTurn)`,
  `output_window_title(input: &str) -> String`, costanti `PLACEHOLDER_EMPTY`, `NO_UI_ACK`.

- [ ] **Step 1: test RED**

```rust
// crates/orchestrator/src/surface.rs
//! # surface — dove va ogni `ServerMsg` di un turno originato da una shell
//!
//! `core::handle_command` e l'adapter AI emettono `ServerMsg` su UN canale
//! senza sapere chi li consuma (v1: la stessa connessione). Per una sessione
//! shell (spec §3.2, D14) i messaggi si dividono:
//! - il **testo** (`Chunk`: risposta AI token-per-token + trasparenza dei
//!   tool) viene **bufferizzato** e consegnato una volta sola alla finestra
//!   di output su `ui` a `Done`/`Error` (`OutputWindowContent`);
//! - `Done`/`Error` tornano alla shell, preceduti da UNA riga di conferma
//!   (`Chunk`) — l'unico modo in cui la host sa che la finestra esiste;
//! - tutto il resto segue `ServerMsg::surface()`: `Origin` → shell
//!   (gate, `ExecInShell`, heartbeat), `Ui` → sink `ui` (finestre, relay).
//!
//! Questo task consuma da un `UnboundedReceiver` e scrive su due
//! `UnboundedSender`: nessuna rete, testabile con tre canali.

use protocol::{ServerMsg, Surface};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

/// Identità di un turno shell: `window_id` e `title` sono decisi PRIMA di
/// avviare il comando, così la finestra si apre subito col segnaposto.
#[derive(Debug, Clone)]
pub struct ShellTurn {
    pub id: String,
    pub session_id: String,
    pub window_id: String,
    pub title: String,
}

/// Contenuto della finestra quando il turno non ha prodotto testo.
pub const PLACEHOLDER_EMPTY: &str = "_(nessun output)_";
/// Riga di conferma nel terminale quando `ui.exe` non è connesso.
pub const NO_UI_ACK: &str = "→ ui.exe non connesso: output non mostrato";
pub const DEFAULT_TITLE: &str = "Lare — Output";

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{ErrCode, WindowKind};
    use tokio::sync::mpsc::unbounded_channel;

    fn turn() -> ShellTurn {
        ShellTurn { id: "c1".into(), session_id: "s1".into(), window_id: "c1".into(), title: "T".into() }
    }

    fn drain(rx: &mut tokio::sync::mpsc::UnboundedReceiver<ServerMsg>) -> Vec<ServerMsg> {
        let mut v = vec![];
        while let Ok(m) = rx.try_recv() {
            v.push(m);
        }
        v
    }

    /// Sequenza completa: apre la finestra all'inizio, bufferizza i chunk,
    /// inoltra il gate alla shell, a `Done` consegna il Markdown a `ui` e
    /// UNA riga di conferma + `Done` alla shell.
    #[tokio::test]
    async fn buffers_chunks_and_delivers_content_on_done() {
        let (tx, rx) = unbounded_channel();
        let (shell_tx, mut shell_rx) = unbounded_channel();
        let (ui_tx, mut ui_rx) = unbounded_channel();
        let router = tokio::spawn(route_shell_turn(rx, shell_tx, Some(ui_tx), turn()));
        tx.send(ServerMsg::Chunk { id: "c1".into(), content: "$ dir\n".into() }).unwrap();
        tx.send(ServerMsg::ToolConfirmRequest { id: "k".into(), commands: "$ dir".into() }).unwrap();
        tx.send(ServerMsg::Chunk { id: "c1".into(), content: "Ecco ".into() }).unwrap();
        tx.send(ServerMsg::Chunk { id: "c1".into(), content: "i file.".into() }).unwrap();
        tx.send(ServerMsg::Done { id: "c1".into(), exit_code: None }).unwrap();
        drop(tx);
        router.await.unwrap();

        let ui = drain(&mut ui_rx);
        assert!(matches!(&ui[0], ServerMsg::OpenOutputWindow { window_id, title } if window_id == "c1" && title == "T"), "{ui:?}");
        assert!(matches!(&ui[1], ServerMsg::ActivityIndicator { on: true, .. }), "{ui:?}");
        assert!(matches!(&ui[2], ServerMsg::OutputWindowContent { window_id, markdown } if window_id == "c1" && markdown == "$ dir\nEcco i file."), "{ui:?}");
        assert!(matches!(&ui[3], ServerMsg::ActivityIndicator { on: false, .. }), "{ui:?}");
        assert_eq!(ui.len(), 4);

        let shell = drain(&mut shell_rx);
        assert!(matches!(&shell[0], ServerMsg::ToolConfirmRequest { .. }), "{shell:?}");
        assert!(matches!(&shell[1], ServerMsg::Chunk { content, .. } if content == "→ finestra \"T\" aperta"), "{shell:?}");
        assert!(matches!(&shell[2], ServerMsg::Done { .. }), "{shell:?}");
        assert_eq!(shell.len(), 3);
    }

    /// Messaggi con `surface() == Ui` (es. `OpenWindow` di `show_markdown`)
    /// vanno a `ui`; quelli `Origin` (es. `ExecInShell`) alla shell; un turno
    /// senza testo consegna il segnaposto.
    #[tokio::test]
    async fn routes_by_surface_and_uses_placeholder_when_empty() {
        let (tx, rx) = unbounded_channel();
        let (shell_tx, mut shell_rx) = unbounded_channel();
        let (ui_tx, mut ui_rx) = unbounded_channel();
        let router = tokio::spawn(route_shell_turn(rx, shell_tx, Some(ui_tx), turn()));
        tx.send(ServerMsg::OpenWindow { title: "x".into(), kind: WindowKind::Markdown, content: "y".into() }).unwrap();
        tx.send(ServerMsg::ExecInShell { turn_id: "c1".into(), exec_id: "e".into(), command: "dir".into(), capture: true }).unwrap();
        tx.send(ServerMsg::Done { id: "c1".into(), exit_code: Some(0) }).unwrap();
        drop(tx);
        router.await.unwrap();
        let ui = drain(&mut ui_rx);
        assert!(ui.iter().any(|m| matches!(m, ServerMsg::OpenWindow { .. })));
        assert!(ui.iter().any(|m| matches!(m, ServerMsg::OutputWindowContent { markdown, .. } if markdown == PLACEHOLDER_EMPTY)));
        let shell = drain(&mut shell_rx);
        assert!(matches!(&shell[0], ServerMsg::ExecInShell { .. }), "{shell:?}");
    }

    /// `Error`: la finestra riceve l'errore (una sola volta) e la shell l'`Error`
    /// stesso — nessuna riga di conferma "finestra aperta".
    #[tokio::test]
    async fn error_flushes_window_once_and_forwards_error() {
        let (tx, rx) = unbounded_channel();
        let (shell_tx, mut shell_rx) = unbounded_channel();
        let (ui_tx, mut ui_rx) = unbounded_channel();
        let router = tokio::spawn(route_shell_turn(rx, shell_tx, Some(ui_tx), turn()));
        tx.send(ServerMsg::Chunk { id: "c1".into(), content: "parziale".into() }).unwrap();
        tx.send(ServerMsg::Error { id: "c1".into(), code: ErrCode::AiError, message: "boom".into() }).unwrap();
        drop(tx);
        router.await.unwrap();
        let ui = drain(&mut ui_rx);
        let contents: Vec<&ServerMsg> = ui.iter().filter(|m| matches!(m, ServerMsg::OutputWindowContent { .. })).collect();
        assert_eq!(contents.len(), 1);
        assert!(matches!(contents[0], ServerMsg::OutputWindowContent { markdown, .. } if markdown.contains("boom") && markdown.contains("parziale")));
        let shell = drain(&mut shell_rx);
        assert!(matches!(&shell[0], ServerMsg::Error { message, .. } if message == "boom"), "{shell:?}");
        assert_eq!(shell.len(), 1);
    }

    /// Senza sink `ui` il turno gira lo stesso: niente panic, la shell riceve
    /// l'avviso al posto della conferma.
    #[tokio::test]
    async fn without_ui_sink_shell_gets_warning_ack() {
        let (tx, rx) = unbounded_channel();
        let (shell_tx, mut shell_rx) = unbounded_channel();
        let router = tokio::spawn(route_shell_turn(rx, shell_tx, None, turn()));
        tx.send(ServerMsg::Chunk { id: "c1".into(), content: "x".into() }).unwrap();
        tx.send(ServerMsg::Done { id: "c1".into(), exit_code: None }).unwrap();
        drop(tx);
        router.await.unwrap();
        let shell = drain(&mut shell_rx);
        assert!(matches!(&shell[0], ServerMsg::Chunk { content, .. } if content == NO_UI_ACK), "{shell:?}");
        assert!(matches!(&shell[1], ServerMsg::Done { .. }));
    }

    #[test]
    fn output_window_title_from_input() {
        assert_eq!(output_window_title("/ai \"elenca i file\""), "elenca i file");
        assert_eq!(output_window_title("/ \"ciao\""), "ciao");
        assert_eq!(output_window_title("/help"), "Lare — /help");
        assert_eq!(output_window_title("/open C:\\x"), "Lare — /open");
        assert_eq!(output_window_title("/ping"), "Lare — /ping");
        let long = format!("/ai \"{}\"", "a".repeat(100));
        assert_eq!(output_window_title(&long).chars().count(), 60);
        assert_eq!(output_window_title(""), DEFAULT_TITLE);
    }
}
```

- [ ] **Step 2: RED** — `cargo test -p orchestrator surface 2>&1 | grep -E "^error" | head -3`.

- [ ] **Step 3: implementazione** (fra le costanti e `#[cfg(test)]`)

```rust
/// Titolo della finestra di output derivato dall'input (prima di eseguirlo).
/// `/ai "testo"` e `/ "testo"` → il testo (max 60 caratteri); `/cmd …` →
/// `Lare — /cmd`; input vuoto → `DEFAULT_TITLE`.
pub fn output_window_title(input: &str) -> String {
    let trimmed = input.trim();
    let Some(after) = trimmed.strip_prefix('/') else { return DEFAULT_TITLE.to_string() };
    let (cmd, rest) = match after.split_once(char::is_whitespace) {
        Some((c, r)) => (c, r.trim()),
        None => (after, ""),
    };
    let quoted = if cmd.is_empty() || cmd.eq_ignore_ascii_case("ai") {
        rest.strip_prefix('"').and_then(|r| r.strip_suffix('"'))
    } else {
        None
    };
    match quoted {
        Some(text) if !text.trim().is_empty() => text.trim().chars().take(60).collect(),
        _ if cmd.is_empty() => DEFAULT_TITLE.to_string(),
        _ => format!("Lare \u{2014} /{}", cmd.to_ascii_lowercase()),
    }
}

/// Consuma i `ServerMsg` del turno da `rx` finché il produttore droppa il
/// sender (`handle_command` lo fa a fine turno), instradando come descritto
/// nel doc-comment del modulo. `ui: None` = `ui.exe` non connesso: i
/// messaggi per `ui` vengono scartati (log `warn`) e la shell riceve
/// `NO_UI_ACK` al posto della conferma.
pub async fn route_shell_turn(
    mut rx: UnboundedReceiver<ServerMsg>,
    shell: UnboundedSender<ServerMsg>,
    ui: Option<UnboundedSender<ServerMsg>>,
    turn: ShellTurn,
) {
    let to_ui = |m: ServerMsg| {
        match &ui {
            Some(u) => {
                let _ = u.send(m);
            }
            None => tracing::warn!("turno {} (sessione {}): ui.exe non connesso, scarto {m:?}", turn.id, turn.session_id),
        }
    };
    to_ui(ServerMsg::OpenOutputWindow { window_id: turn.window_id.clone(), title: turn.title.clone() });
    to_ui(ServerMsg::ActivityIndicator { session_id: turn.session_id.clone(), kind: "ai_busy".into(), on: true });

    let mut buffer = String::new();
    let mut flushed = false;
    let flush = |markdown: String, flushed: &mut bool| {
        if !*flushed {
            *flushed = true;
            to_ui(ServerMsg::OutputWindowContent { window_id: turn.window_id.clone(), markdown });
        }
    };

    while let Some(msg) = rx.recv().await {
        match msg {
            ServerMsg::Chunk { content, .. } => buffer.push_str(&content),
            ServerMsg::Done { id, exit_code } => {
                let markdown = if buffer.trim().is_empty() { PLACEHOLDER_EMPTY.to_string() } else { buffer.clone() };
                flush(markdown, &mut flushed);
                let ack = if ui.is_some() { format!("\u{2192} finestra \"{}\" aperta", turn.title) } else { NO_UI_ACK.to_string() };
                let _ = shell.send(ServerMsg::Chunk { id: id.clone(), content: ack });
                let _ = shell.send(ServerMsg::Done { id, exit_code });
            }
            ServerMsg::Error { id, code, message } => {
                flush(format!("**Errore:** {message}\n\n{buffer}"), &mut flushed);
                let _ = shell.send(ServerMsg::Error { id, code, message });
            }
            other => match other.surface() {
                Surface::Ui => to_ui(other),
                Surface::Origin => {
                    let _ = shell.send(other);
                }
            },
        }
    }
    to_ui(ServerMsg::ActivityIndicator { session_id: turn.session_id.clone(), kind: "ai_busy".into(), on: false });
}
```

Se il borrow checker rifiuta la closure `flush` che cattura `to_ui` (due closure che prendono in
prestito `ui`/`turn`), trasformare `to_ui`/`flush` in due funzioni libere che ricevono
`&Option<UnboundedSender<ServerMsg>>` e `&ShellTurn` per argomento: il comportamento testato non
cambia.

- [ ] **Step 4: GREEN** — `cargo test -p orchestrator surface 2>&1 | grep "test result"` → `5 passed`.

- [ ] **Step 5: commit**

```bash
git add crates/orchestrator/src/surface.rs crates/orchestrator/src/lib.rs
git commit -m "feat(orchestrator): router di superficie per i turni shell (finestra di output a Done)"
```

---

### Task 6: `shell_slash.rs` — pre-router dei comandi slash della shell; `KNOWN_BACKEND_SLASHES`; `/help` 2.0

**Files:**
- Create: `crates/orchestrator/src/shell_slash.rs`
- Modify: `crates/orchestrator/src/lib.rs` (`pub mod shell_slash;`)
- Modify: `crates/orchestrator/src/core.rs` (costante `KNOWN_BACKEND_SLASHES` accanto a
  `HELP_MARKDOWN`, riga ~80; `HELP_MARKDOWN` riscritto)
- Test: moduli `tests` di `shell_slash.rs` e `core.rs`

**Interfaces:**
- Consumes: `external_channel::EXTERNAL_TOOL_CHANNELS` (campi `id`, `slash_trigger`).
- Produces: `core::KNOWN_BACKEND_SLASHES: &[&str]` = `["open", "web", "show", "help"]`;
  `shell_slash::{ShellInput, classify_shell_input(input, is_known_backend: &dyn Fn(&str) -> bool) -> ShellInput, AI_SYNTAX_ERROR, RESET_MESSAGE, UI_LOCAL_SLASHES, shell_channel_table() -> Vec<(String, String)>, read_web_search_enabled(config_dir) -> bool}`.

- [ ] **Step 1: test RED**

```rust
// crates/orchestrator/src/shell_slash.rs
//! # shell_slash — che cosa fare di una riga `/…` arrivata da una shell
//!
//! La host manda all'orchestratore OGNI riga che inizia con `/` (spec §3):
//! non ha un elenco proprio. Questo modulo è quell'elenco, in forma di
//! classificazione pura (nessun I/O, nessun `await`): `ws.rs` la consulta
//! PRIMA del routing v1 e agisce di conseguenza. Regole (tabella §3):
//! - `/ai "testo"` e `/ "testo"` → turno AI con `testo` (virgolette
//!   obbligatorie, D8; senza → errore di sintassi nel terminale);
//! - `/config` `/library` `/aichat` e i canali esterni (`/nmap`, `/markets`,
//!   `/pyping`) → `OpenUiLocal{name}` verso `ui` (singleton, D15);
//! - `/reset` → messaggio "non applicabile"; `/ping` → built-in;
//! - slash del backend v1 (`core::KNOWN_BACKEND_SLASHES`) → `handle_command`;
//! - tutto il resto → **scartato** (`Done` muto + log `info`).
//!
//! "Conosciuto" non è deciso qui: arriva dal chiamante come predicato
//! (`is_known_backend`), così plugin e backend restano la loro unica lista.

use std::path::Path;

/// Esito della classificazione (vedi doc-comment del modulo).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellInput {
    /// Turno AI in linguaggio naturale col testo fra virgolette.
    Nl(String),
    /// `/ai`/`/ ` senza virgolette (o vuoto): errore da stampare nel terminale.
    SyntaxError(String),
    /// Finestra locale di `ui`: `"config"`, `"library"`, `"aichat"` o id canale.
    OpenUiLocal(String),
    Reset,
    Ping,
    /// Comando slash del backend v1 (`/open`, `/web`, `/show`, `/help`).
    Backend,
    /// Slash ignoto: il nome (senza `/`) per il log.
    Discard(String),
}

pub const AI_SYNTAX_ERROR: &str = "sintassi: /ai \"testo\" (virgolette obbligatorie)";
pub const RESET_MESSAGE: &str = "non applicabile: la sessione è la tua";
/// Finestre locali di `ui` raggiungibili per nome (D15).
pub const UI_LOCAL_SLASHES: &[&str] = &["config", "library", "aichat"];
/// Trigger dei canali esterni esposti all'utente (gli altri in
/// `EXTERNAL_TOOL_CHANNELS` — `library-expand`, `config-market-data-test` —
/// sono interni a `ui`). Stessa terna della tabella JS `external-channels.js`.
const USER_FACING_CHANNEL_TRIGGERS: &[&str] = &["/nmap", "/pyping", "/markets"];

#[cfg(test)]
mod tests {
    use super::*;

    fn known(c: &str) -> bool {
        crate::core::KNOWN_BACKEND_SLASHES.contains(&c)
    }

    #[test]
    fn ai_and_bare_slash_with_quotes_are_the_same_nl_turn() {
        assert_eq!(classify_shell_input("/ai \"elenca i file\"", &known), ShellInput::Nl("elenca i file".into()));
        assert_eq!(classify_shell_input("/ \"elenca i file\"", &known), ShellInput::Nl("elenca i file".into()));
        assert_eq!(classify_shell_input("  /AI   \"x\"  ", &known), ShellInput::Nl("x".into()));
        // Le virgolette interne restano parte del testo.
        assert_eq!(classify_shell_input("/ai \"dimmi \"ciao\"\"", &known), ShellInput::Nl("dimmi \"ciao\"".into()));
    }

    #[test]
    fn ai_without_quotes_or_empty_is_a_syntax_error() {
        for input in ["/ai elenca", "/ elenca", "/ai", "/ai \"\"", "/ai \"x\" y", "/ai \"x"] {
            assert_eq!(classify_shell_input(input, &known), ShellInput::SyntaxError(AI_SYNTAX_ERROR.into()), "{input}");
        }
    }

    #[test]
    fn ui_local_reset_ping_and_backend_are_recognised() {
        assert_eq!(classify_shell_input("/config", &known), ShellInput::OpenUiLocal("config".into()));
        assert_eq!(classify_shell_input("/Library extra", &known), ShellInput::OpenUiLocal("library".into()));
        assert_eq!(classify_shell_input("/aichat", &known), ShellInput::OpenUiLocal("aichat".into()));
        assert_eq!(classify_shell_input("/nmap", &known), ShellInput::OpenUiLocal("nmap".into()));
        assert_eq!(classify_shell_input("/markets", &known), ShellInput::OpenUiLocal("financial-markets".into()));
        assert_eq!(classify_shell_input("/pyping", &known), ShellInput::OpenUiLocal("python-ping".into()));
        assert_eq!(classify_shell_input("/reset", &known), ShellInput::Reset);
        assert_eq!(classify_shell_input("/ping", &known), ShellInput::Ping);
        for input in ["/open C:\\x", "/web gatti", "/show # t", "/help"] {
            assert_eq!(classify_shell_input(input, &known), ShellInput::Backend, "{input}");
        }
    }

    #[test]
    fn unknown_slashes_are_discarded_including_find_and_nowin() {
        assert_eq!(classify_shell_input("/nonesiste a b", &known), ShellInput::Discard("nonesiste".into()));
        assert_eq!(classify_shell_input("/find x", &known), ShellInput::Discard("find".into()));
        assert_eq!(classify_shell_input("/nowin x", &known), ShellInput::Discard("nowin".into()));
        assert_eq!(classify_shell_input("/", &known), ShellInput::Discard(String::new()));
        assert_eq!(classify_shell_input("dir", &known), ShellInput::Discard("dir".into()));
    }

    #[test]
    fn channel_table_exposes_the_three_user_facing_channels() {
        let mut t = shell_channel_table();
        t.sort();
        assert_eq!(t, vec![
            ("markets".to_string(), "financial-markets".to_string()),
            ("nmap".to_string(), "nmap".to_string()),
            ("pyping".to_string(), "python-ping".to_string()),
        ]);
    }

    #[test]
    fn web_search_enabled_is_read_from_config_json_default_false() {
        let dir = std::env::temp_dir().join(format!("lare-shell-slash-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!read_web_search_enabled(&dir), "file assente → false");
        std::fs::write(dir.join("config.json"), r#"{"window_alpha":0.9,"web_search_enabled":true}"#).unwrap();
        assert!(read_web_search_enabled(&dir));
        std::fs::write(dir.join("config.json"), "{ non json").unwrap();
        assert!(!read_web_search_enabled(&dir), "corrotto → false");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
```

In `mod tests` di `core.rs`:

```rust
    /// `KNOWN_BACKEND_SLASHES` è l'UNICA lista dei comandi che `handle_slash`
    /// dispaccia (esclusi `reset`, gestito a parte dalla shell): ognuno deve
    /// davvero rispondere senza `Error{RoutingError, "sconosciuto"}`.
    #[tokio::test]
    async fn known_backend_slashes_are_all_dispatched_by_handle_slash() {
        let tools = FakeToolClient::success("ok");
        for cmd in KNOWN_BACKEND_SLASHES {
            let input = match *cmd {
                "show" => "/show # titolo".to_string(),
                "open" => "/open C:\\x".to_string(),
                "web" => "/web gatti".to_string(),
                other => format!("/{other}"),
            };
            let out = handle_slash("id", &input, &tools, "").await;
            let unknown = out.iter().any(|m| matches!(m, ServerMsg::Error { message, .. } if message.contains("sconosciuto")));
            assert!(!unknown, "/{cmd} risulta sconosciuto a handle_slash: {out:?}");
        }
    }
```

(Adattare il costruttore del fake a quello usato dagli altri test di `core.rs`, es.
`FakeToolClient::success`; il contratto è "nessun `Error` 'sconosciuto'".)

- [ ] **Step 2: RED** — `cargo test -p orchestrator shell_slash 2>&1 | grep -E "^error" | head -3`;
`cargo test -p orchestrator known_backend 2>&1 | grep -E "^error" | head -2`.

- [ ] **Step 3: implementazione**

`core.rs`, subito prima di `const HELP_MARKDOWN`:

```rust
/// Comandi slash che `handle_slash` dispaccia davvero (senza `/`), esclusi
/// `reset` (che sulla shell ha una risposta propria, spec §3) e `find`/`nowin`
/// (gestiti in `ws.rs`/`handle_command`, non disponibili dalla shell in questa
/// versione). È l'unica fonte per "questo slash esiste" del pre-router
/// `shell_slash` — chi aggiunge un braccio a `handle_slash` lo aggiunge qui
/// (il test `known_backend_slashes_are_all_dispatched_by_handle_slash` tiene
/// le due cose allineate).
pub const KNOWN_BACKEND_SLASHES: &[&str] = &["open", "web", "show", "help"];
```

`HELP_MARKDOWN` riscritto per il 2.0 (sostituire l'intera costante):

```rust
const HELP_MARKDOWN: &str = r#"# Lare — Comandi

Scrivi i comandi `/…` nella riga di comando di Lare Terminal (la tua sessione PowerShell).
L'esito di ogni comando slash compare in una finestra; nel terminale resta una riga di conferma.

## AI
- `/ai "richiesta"` oppure `/ "richiesta"` — l'AI risponde ed esegue comandi **nella tua shell**
  (ogni comando proposto chiede conferma `[Y/n]` prima di partire). Le virgolette sono obbligatorie.

## Comandi
- `/help` — questa finestra.
- `/ping` — verifica i tre strati (lare-shell, orchestratore, plugin, ui).
- `/config` — configurazione (aspetto, ricerca web, AI, mercati).
- `/library` — archivio dei documenti salvati (riapribili).
- `/aichat` — AI Chat (comunicazione fra macchine Lare in rete, con partecipazione dell'AI).
- `/open <target>` — apri un URL, una cartella o un file con l'app di default.
- `/web <query>` — cerca la query nel browser di default.
- `/show <markdown>` — apri una finestra con il Markdown indicato.
- `/calc` — calcolatrice (plugin).

## Strumenti esterni (finestra dedicata)
- `/markets` — strumenti sui mercati finanziari (ricerca ticker, report azionario, elenco titoli, screener).
- `/nmap` — strumenti di scansione di rete (quick scan, rilevamento OS/versioni, host discovery, ricerca vulnerabilità).
- `/pyping` — canale di prova per l'infrastruttura dei tool Python (eco di un messaggio).

## Tutto il resto
- Qualunque riga che non inizia con `/` è PowerShell, come sempre.
- Uno slash sconosciuto viene ignorato in silenzio.
"#;
```

Se un test esistente di `core.rs` verifica il contenuto di `HELP_MARKDOWN` (es. che citi `/web` o
`/reset`), aggiornarlo alla lista nuova: `/reset` non è più documentato (sulla shell non
applicabile), `/ping` e `/ai` sì.

`shell_slash.rs` (fra le costanti e `#[cfg(test)]`):

```rust
/// Trigger → id canale dei canali esterni esposti all'utente, derivati dal
/// registro reale (`EXTERNAL_TOOL_CHANNELS`) e filtrati con
/// `USER_FACING_CHANNEL_TRIGGERS`. Senza la `/` iniziale.
pub fn shell_channel_table() -> Vec<(String, String)> {
    crate::external_channel::EXTERNAL_TOOL_CHANNELS
        .iter()
        .filter(|c| USER_FACING_CHANNEL_TRIGGERS.contains(&c.slash_trigger))
        .map(|c| (c.slash_trigger.trim_start_matches('/').to_string(), c.id.to_string()))
        .collect()
}

/// Testo fra virgolette (`"x"` → `x`), o `None` se non è racchiuso da UNA
/// coppia di virgolette esterne o è vuoto.
fn quoted_text(rest: &str) -> Option<String> {
    let r = rest.trim();
    let inner = r.strip_prefix('"')?.strip_suffix('"')?;
    if r.len() < 2 || inner.trim().is_empty() {
        return None;
    }
    Some(inner.to_string())
}

fn ai_or_error(rest: &str) -> ShellInput {
    match quoted_text(rest) {
        Some(text) => ShellInput::Nl(text),
        None => ShellInput::SyntaxError(AI_SYNTAX_ERROR.to_string()),
    }
}

/// Classifica una riga della shell (vedi tabella nel doc-comment del modulo).
/// `is_known_backend(cmd)` dice se `cmd` (minuscolo, senza `/`) è un comando
/// del backend v1: il chiamante lo costruisce da `core::KNOWN_BACKEND_SLASHES`.
pub fn classify_shell_input(input: &str, is_known_backend: &dyn Fn(&str) -> bool) -> ShellInput {
    let trimmed = input.trim();
    let Some(after) = trimmed.strip_prefix('/') else {
        return ShellInput::Discard(trimmed.to_string());
    };
    // `/ "testo"`: slash, spazio, testo.
    if after.starts_with(char::is_whitespace) {
        return ai_or_error(after);
    }
    let (cmd, rest) = match after.split_once(char::is_whitespace) {
        Some((c, r)) => (c.to_ascii_lowercase(), r.trim()),
        None => (after.to_ascii_lowercase(), ""),
    };
    if cmd == "ai" {
        return ai_or_error(rest);
    }
    if cmd == "ping" {
        return ShellInput::Ping;
    }
    if cmd == "reset" {
        return ShellInput::Reset;
    }
    if UI_LOCAL_SLASHES.contains(&cmd.as_str()) {
        return ShellInput::OpenUiLocal(cmd);
    }
    if let Some((_, id)) = shell_channel_table().into_iter().find(|(trigger, _)| *trigger == cmd) {
        return ShellInput::OpenUiLocal(id);
    }
    if is_known_backend(&cmd) {
        return ShellInput::Backend;
    }
    ShellInput::Discard(cmd)
}

/// `web_search_enabled` da `<config_dir>/config.json` (il file di `ui`,
/// scritto da `/config`): la shell non ha una propria casella per la ricerca
/// web, quindi vale la scelta dell'utente in `/config`. Assente o
/// illeggibile → `false` (comportamento conservativo, come un client che non
/// manda il campo).
pub fn read_web_search_enabled(config_dir: &Path) -> bool {
    std::fs::read_to_string(config_dir.join("config.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("web_search_enabled").and_then(|b| b.as_bool()))
        .unwrap_or(false)
}
```

Nota sul test `"/ai \"x\" y"`: `quoted_text` fallisce perché la stringa non termina con `"` —
corretto (D8: tutto il testo va fra virgolette).

- [ ] **Step 4: GREEN** — `cargo test -p orchestrator shell_slash known_backend 2>&1 | grep "test result"`
→ `7 passed`; `cargo test -p orchestrator 2>&1 | grep -E "test result|FAILED"` → nessun fallimento
(se un test di `HELP_MARKDOWN` fallisce, vedi Step 3).

- [ ] **Step 5: commit**

```bash
git add crates/orchestrator/src/shell_slash.rs crates/orchestrator/src/core.rs crates/orchestrator/src/lib.rs
git commit -m "feat(orchestrator): pre-router slash della shell (/ai con virgolette, discard, OpenUiLocal), /help 2.0"
```

---

### Task 7: `/ping` — sonda del plugin, `UiPing`, uptime

**Files:**
- Modify: `crates/orchestrator/src/plugins/host.rs` (campo `discovered_all`, `find`, `wait_ready`, `probe`)
- Create: `crates/orchestrator/src/ping.rs`
- Modify: `crates/orchestrator/src/lib.rs` (`pub mod ping;` + `PROCESS_START`)
- Modify: `crates/orchestrator/src/main.rs` (forza `PROCESS_START` all'inizio di `main`)
- Test: moduli `tests` di `host.rs` e `ping.rs`

**Interfaces:**
- Consumes: `Registry::{ui_sink, register_ui_ping}` (Task 2), `DiscoveredPlugin`, `PluginWriter/PluginReader`, `HostToPlugin::{Init, Deinit}`, `PluginToHost::Ready`.
- Produces: `PluginHost::find(&self, id) -> Option<(DiscoveredPlugin, PathBuf)>`;
  `plugins::host::probe(p: &DiscoveredPlugin, storage_dir: &Path, make: &mut F) -> Result<Duration, String>`;
  `orchestrator::PROCESS_START: LazyLock<Instant>`;
  `ping::{LayerReport, run_ping(session_id, shell_version, plugin: Option<Result<(String, Duration), String>>, ui: Option<Result<(String, Duration), String>>) -> String, format_uptime(Duration) -> String, UI_PING_TIMEOUT}`.

- [ ] **Step 1: test RED (host.rs)**

In `mod tests` di `host.rs` (usa gli helper esistenti `discovered`, `fake_transport`,
`fake_transport_hanging`, `SentLog`):

```rust
    /// `probe`: spawn usa-e-getta → Init → Ready → Deinit, e ritorna il tempo
    /// Init→Ready. Non tocca `writers` (nessun pump, nessuna finestra).
    #[tokio::test]
    async fn probe_measures_init_to_ready_and_sends_deinit() {
        let log: SentLog = Default::default();
        let l = log.clone();
        let p = discovered("ping", Triggers::default());
        let mut make = move |_p: &DiscoveredPlugin| {
            let (w, r) = fake_transport(l.clone(), vec![
                PluginToHost::Ready { name: "ping".into(), protocol_version: 1 },
            ]);
            Ok((Box::new(w) as Box<dyn PluginWriter>, Box::new(r) as Box<dyn PluginReader>))
        };
        let elapsed = probe(&p, std::path::Path::new("/tmp/storage"), &mut make).await.expect("Ready");
        assert!(elapsed < std::time::Duration::from_secs(5));
        let sent = log.lock().unwrap();
        assert!(matches!(sent[0], HostToPlugin::Init { .. }));
        assert!(matches!(sent[1], HostToPlugin::Deinit {}), "{sent:?}");
    }

    /// Plugin che non risponde: errore leggibile entro il timeout, mai un panic.
    #[tokio::test]
    async fn probe_reports_error_when_plugin_never_sends_ready() {
        let p = discovered("dead", Triggers::default());
        let mut make = move |_p: &DiscoveredPlugin| {
            let (w, r) = fake_transport(Default::default(), vec![]); // EOF immediato
            Ok((Box::new(w) as Box<dyn PluginWriter>, Box::new(r) as Box<dyn PluginReader>))
        };
        let err = probe(&p, std::path::Path::new("/tmp/storage"), &mut make).await.unwrap_err();
        assert!(err.contains("Ready"), "{err}");
    }

    /// `find` conosce anche i plugin eager (che `discovered_lazy` non tiene).
    #[tokio::test]
    async fn find_returns_eager_and_lazy_plugins_with_their_storage_dir() {
        let host = PluginHost::start(
            vec![
                discovered("ping", Triggers::default()),
                discovered("calc", Triggers { command: Some("/calc".into()), interval: None }),
            ],
            |_p| {
                let (w, r) = fake_transport(Default::default(), vec![
                    PluginToHost::Ready { name: "ping".into(), protocol_version: 1 },
                ]);
                Ok((Box::new(w) as Box<dyn PluginWriter>, Box::new(r) as Box<dyn PluginReader>))
            },
            std::path::Path::new("/tmp/root"),
        ).await;
        let (p, dir) = host.find("ping").expect("eager trovato");
        assert_eq!(p.manifest.id, "ping");
        assert_eq!(dir, PathBuf::from("/tmp/root").join("ping"));
        assert!(host.find("calc").is_some());
        assert!(host.find("nope").is_none());
    }
```

- [ ] **Step 2: RED** — `cargo test -p orchestrator plugins::host::tests::probe 2>&1 | grep -E "^error" | head -3`.

- [ ] **Step 3: implementazione in `host.rs`**

Campo nuovo nello struct (dopo `discovered_lazy`):

```rust
    /// TUTTI i plugin scoperti (eager e lazy), per `find`/`probe` (2.0, `/ping`):
    /// `discovered_lazy` tiene solo i lazy, e gli eager dopo lo spawn non
    /// lasciavano traccia del loro manifest.
    discovered_all: Vec<DiscoveredPlugin>,
```

In `start`: `discovered_all: plugins.clone(),` nell'inizializzazione di `host` (prima del `for`).

Metodo `find` (dopo `running_count`):

```rust
    /// Manifest + cartella di storage del plugin `plugin_id` (per `probe`).
    pub fn find(&self, plugin_id: &str) -> Option<(DiscoveredPlugin, PathBuf)> {
        self.discovered_all
            .iter()
            .find(|p| p.manifest.id == plugin_id)
            .map(|p| (p.clone(), self.storage_root.join(plugin_id)))
    }
```

Estrarre da `spawn_and_handshake` l'attesa del `Ready` in una funzione libera (stessa logica,
stessi messaggi), usata da entrambi:

```rust
/// Ready-gate condiviso da `spawn_and_handshake` e `probe`: il PRIMO
/// messaggio del plugin deve essere `Ready` entro 5 s. `Err` = motivo
/// leggibile (timeout, EOF, messaggio diverso, errore I/O).
async fn wait_ready(reader: &mut Box<dyn PluginReader>, plugin_id: &str) -> Result<(String, u32), String> {
    match tokio::time::timeout(std::time::Duration::from_secs(5), reader.recv()).await {
        Err(_timeout) => Err(format!("'{plugin_id}' non ha inviato Ready entro 5s")),
        Ok(Ok(Some(PluginToHost::Ready { name, protocol_version }))) => Ok((name, protocol_version)),
        Ok(Ok(other)) => Err(format!("'{plugin_id}' non ha riportato Ready (msg={other:?})")),
        Ok(Err(e)) => Err(format!("'{plugin_id}' recv Ready fallito: {e}")),
    }
}
```

In `spawn_and_handshake` il blocco `let ready_result = … match ready_result { … }` diventa:

```rust
        match wait_ready(&mut reader, &p.manifest.id).await {
            Ok((name, protocol_version)) => {
                eprintln!("[plugins] '{}' Ready (name={name}, proto={protocol_version})", p.manifest.id);
            }
            Err(reason) => {
                eprintln!("[plugins] {reason} — non avviato");
                return None;
            }
        }
```

`probe` (funzione libera, dopo `impl PluginHost`):

```rust
/// Sonda usa-e-getta per `/ping` (spec §3.1): avvia un'istanza NUOVA del
/// plugin, misura Init→Ready, manda `Deinit` e la lascia morire (il
/// transport reale ha `kill_on_drop`). Non passa da `PluginHost` perché
/// l'istanza eager già viva non ha un handshake da rimisurare e non va
/// disturbata. `storage_dir`: quella che `PluginHost::find` riporta.
pub async fn probe<F>(p: &DiscoveredPlugin, storage_dir: &Path, make: &mut F) -> Result<std::time::Duration, String>
where
    F: FnMut(&DiscoveredPlugin) -> std::io::Result<(Box<dyn PluginWriter>, Box<dyn PluginReader>)>,
{
    let (mut writer, mut reader) = make(p).map_err(|e| format!("spawn fallito: {e}"))?;
    let start = std::time::Instant::now();
    writer
        .send(HostToPlugin::Init {
            protocol_version: p.manifest.protocol_version,
            config: serde_json::Value::Null,
            storage_dir: storage_dir.to_string_lossy().into_owned(),
        })
        .await
        .map_err(|e| format!("Init fallito: {e}"))?;
    wait_ready(&mut reader, &p.manifest.id).await?;
    let elapsed = start.elapsed();
    // Best-effort: il plugin è usa-e-getta, un Deinit fallito non è un errore del ping.
    let _ = writer.send(HostToPlugin::Deinit {}).await;
    Ok(elapsed)
}
```

Run: `cargo test -p orchestrator plugins 2>&1 | grep "test result"` → tutti i test di `plugins`
verdi (i preesistenti + 3).

- [ ] **Step 4: test RED (ping.rs)**

```rust
// crates/orchestrator/src/ping.rs
//! # ping — il built-in `/ping` (spec §3.1): una riga per strato
//!
//! Misura ciò che si può misurare davvero: l'orchestratore (uptime),
//! plugin-ping (una sonda Init→Ready usa-e-getta, `plugins::host::probe`),
//! `ui.exe` (`UiPing`→`UiPong` con timeout). La riga di `lare-shell` viene
//! dalla `Hello` (versione dichiarata + `session_id`). Ogni strato assente
//! degrada a una riga `--`; il comando finisce comunque con `Done`.
//!
//! Le sonde arrivano già risolte (`Option<Result<…>>`): il chiamante
//! (`ws.rs`) le esegue con le sue dipendenze, questo modulo formatta. Così è
//! testabile senza plugin host né registro.

use std::time::Duration;

/// Tempo massimo di attesa dell'`UiPong` (usato da `ws.rs`).
pub const UI_PING_TIMEOUT: Duration = Duration::from_secs(2);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uptime_is_compact_and_human() {
        assert_eq!(format_uptime(Duration::from_secs(5)), "5s");
        assert_eq!(format_uptime(Duration::from_secs(61)), "1m01s");
        assert_eq!(format_uptime(Duration::from_secs(4320)), "1h12m");
        assert_eq!(format_uptime(Duration::from_secs(90000)), "1d01h");
    }

    #[test]
    fn report_has_one_row_per_layer_in_order() {
        let md = run_ping(
            "a1b2",
            Some("2.0.0"),
            Some(Ok(("1.0.0".into(), Duration::from_millis(8)))),
            Some(Ok(("2.1.0".into(), Duration::from_millis(12)))),
        );
        let rows: Vec<&str> = md.lines().filter(|l| l.starts_with("| ") && !l.starts_with("| strato")).collect();
        assert_eq!(rows.len(), 4, "{md}");
        assert!(rows[0].starts_with("| lare-shell | 2.0.0 | ok | sessione a1b2"), "{}", rows[0]);
        assert!(rows[1].starts_with("| orchestrator | "), "{}", rows[1]);
        assert!(rows[1].contains(env!("CARGO_PKG_VERSION")) && rows[1].contains("uptime "), "{}", rows[1]);
        assert!(rows[2].starts_with("| plugin-ping | 1.0.0 | ok | round-trip 8 ms"), "{}", rows[2]);
        assert!(rows[3].starts_with("| ui.exe | 2.1.0 | ok | 12 ms"), "{}", rows[3]);
    }

    #[test]
    fn missing_layers_degrade_to_dashes_and_errors_are_shown() {
        let md = run_ping("s", None, Some(Err("spawn fallito: x".into())), None);
        assert!(md.contains("| lare-shell | -- | ok | sessione s"), "{md}");
        assert!(md.contains("| plugin-ping | -- | errore | spawn fallito: x"), "{md}");
        assert!(md.contains("| ui.exe | -- | non connesso |"), "{md}");
        let md2 = run_ping("s", None, None, None);
        assert!(md2.contains("| plugin-ping | -- | non trovato |"), "{md2}");
    }
}
```

- [ ] **Step 5: RED** — `cargo test -p orchestrator ping::tests 2>&1 | grep -E "^error" | head -3`.

- [ ] **Step 6: implementazione**

`lib.rs`, dopo i `pub mod` (con `use std::sync::LazyLock; use std::time::Instant;` in testa):

```rust
/// Istante di avvio del processo, per l'uptime di `/ping`. `main()` lo forza
/// subito (`LazyLock::force`), così vale davvero l'avvio e non il primo ping.
pub static PROCESS_START: LazyLock<Instant> = LazyLock::new(Instant::now);
```

`main.rs`, prima riga del corpo di `main`: `std::sync::LazyLock::force(&orchestrator::PROCESS_START);`

`ping.rs`:

```rust
/// `5s`, `1m01s`, `1h12m`, `1d01h`: due unità al massimo.
pub fn format_uptime(d: Duration) -> String {
    let s = d.as_secs();
    let (days, hours, mins, secs) = (s / 86_400, (s % 86_400) / 3_600, (s % 3_600) / 60, s % 60);
    if days > 0 {
        format!("{days}d{hours:02}h")
    } else if hours > 0 {
        format!("{hours}h{mins:02}m")
    } else if mins > 0 {
        format!("{mins}m{secs:02}s")
    } else {
        format!("{secs}s")
    }
}

/// Una riga della tabella: `(strato, versione, stato, dettaglio)`.
pub struct LayerReport {
    pub layer: &'static str,
    pub version: String,
    pub status: &'static str,
    pub detail: String,
}

fn probed(layer: &'static str, probe: Option<Result<(String, Duration), String>>, missing: &'static str, ok_detail: &dyn Fn(Duration) -> String) -> LayerReport {
    match probe {
        Some(Ok((version, elapsed))) => LayerReport { layer, version, status: "ok", detail: ok_detail(elapsed) },
        Some(Err(reason)) => LayerReport { layer, version: "--".into(), status: "errore", detail: reason },
        None => LayerReport { layer, version: "--".into(), status: missing, detail: String::new() },
    }
}

/// Markdown del report (spec §3.1). `plugin`/`ui`: `None` = strato non
/// presente (plugin non scoperto / `ui.exe` non connesso); `Some(Err)` =
/// presente ma in errore (con motivo); `Some(Ok((versione, tempo)))`.
pub fn run_ping(
    session_id: &str,
    shell_version: Option<&str>,
    plugin: Option<Result<(String, Duration), String>>,
    ui: Option<Result<(String, Duration), String>>,
) -> String {
    let rows = [
        LayerReport {
            layer: "lare-shell",
            version: shell_version.unwrap_or("--").to_string(),
            status: "ok",
            detail: format!("sessione {session_id}"),
        },
        LayerReport {
            layer: "orchestrator",
            version: env!("CARGO_PKG_VERSION").to_string(),
            status: "ok",
            detail: format!("uptime {}", format_uptime(crate::PROCESS_START.elapsed())),
        },
        probed("plugin-ping", plugin, "non trovato", &|e| format!("round-trip {} ms", e.as_millis())),
        probed("ui.exe", ui, "non connesso", &|e| format!("{} ms", e.as_millis())),
    ];
    let mut md = String::from("| strato | versione | stato | dettaglio |\n|---|---|---|---|\n");
    for r in rows {
        md.push_str(&format!("| {} | {} | {} | {} |\n", r.layer, r.version, r.status, r.detail));
    }
    md
}
```

- [ ] **Step 7: GREEN** — `cargo test -p orchestrator ping::tests plugins 2>&1 | grep "test result"` → verdi;
`cargo build -p orchestrator 2>&1 | tail -1` → `Finished`.

- [ ] **Step 8: commit**

```bash
git add crates/orchestrator/src/plugins/host.rs crates/orchestrator/src/ping.rs crates/orchestrator/src/lib.rs crates/orchestrator/src/main.rs
git commit -m "feat(orchestrator): /ping — sonda plugin usa-e-getta, uptime, report per strato"
```

---

### Task 8: wiring — `HelloInfo`, registro, ramo shell del `Command`, `ExecResult`/`UiPong`, teardown

**Files:**
- Create: `crates/orchestrator/src/shell_turn.rs` (esecuzione di un comando shell: pre-router + turno)
- Modify: `crates/orchestrator/src/ws.rs` (`serve`, `handle_connection`, `parse_hello`, arm `Command`, arm nuovi, teardown)
- Modify: `crates/orchestrator/src/main.rs:687-699` (`Registry::shared()` a `serve`)
- Modify: `crates/orchestrator/src/lib.rs` (`pub mod shell_turn;`)
- Modify: `crates/orchestrator/tests/ws_integration.rs:95-140` (`spawn_server` passa il registro)
- Test: `crates/orchestrator/src/ws.rs` (test di `parse_hello`), `tests/ws_integration.rs` (uno scenario di fumo; gli scenari completi sono nel Task 10)

**Interfaces:**
- Consumes: Task 1-7 (`Role`, `Registry`, `ShellConfirmer`, `ShellSessionState`, `ShellSessionToolClient`, `route_shell_turn`, `ShellTurn`, `output_window_title`, `classify_shell_input`, `read_web_search_enabled`, `PluginHost::find`, `probe`, `run_ping`, `UI_PING_TIMEOUT`).
- Produces: `ws::serve(…, rt, registry: SharedRegistry)`; `ws::HelloInfo { token, channel, role, session_id, cwd, version }`;
  `shell_turn::{ShellTurnDeps, run_shell_command}`.

- [ ] **Step 1: test RED (`parse_hello` → `HelloInfo`)**

In `mod tests` di `ws.rs` (in fondo al file, accanto ai test di `connection_owns_plugin_sink`):

```rust
    use super::parse_hello;
    use tokio_tungstenite::tungstenite::Message;

    #[test]
    fn parse_hello_reads_role_session_cwd_version_with_ui_defaults() {
        let v1 = Message::Text(r#"{"type":"hello","token":"t","channel":"nmap"}"#.into());
        let h = parse_hello(&v1).unwrap();
        assert_eq!((h.token.as_str(), h.channel.as_deref(), h.role), ("t", Some("nmap"), protocol::Role::Ui));
        assert!(h.session_id.is_none() && h.cwd.is_none() && h.version.is_none());

        let shell = Message::Text(
            r#"{"type":"hello","token":"t","role":"shell","session_id":"s1","cwd":"C:\\w","version":"2.0.0"}"#.into(),
        );
        let h = parse_hello(&shell).unwrap();
        assert_eq!(h.role, protocol::Role::Shell);
        assert_eq!(h.session_id.as_deref(), Some("s1"));
        assert_eq!(h.cwd.as_deref(), Some("C:\\w"));
        assert_eq!(h.version.as_deref(), Some("2.0.0"));
        assert!(parse_hello(&Message::Text(r#"{"type":"ping","ts":1}"#.into())).is_none());
    }
```

Run: `cargo test -p orchestrator parse_hello_reads 2>&1 | grep -E "^error" | head -2` → `HelloInfo`
non esiste (RED).

- [ ] **Step 2: `HelloInfo` e firma di `serve`/`handle_connection`**

In `ws.rs`, sostituire `parse_hello`:

```rust
/// Tutto ciò che la `Hello` dichiara (2.0, spec §4.1). Sostituisce la tupla
/// `(token, channel)` v1: con quattro campi in più una tupla non si legge.
#[derive(Debug, Clone, PartialEq)]
pub struct HelloInfo {
    pub token: String,
    pub channel: Option<String>,
    pub role: protocol::Role,
    pub session_id: Option<String>,
    pub cwd: Option<String>,
    pub version: Option<String>,
}

/// Attempt to parse a WS frame as a `ClientMsg::Hello`.
fn parse_hello(msg: &Message) -> Option<HelloInfo> {
    let text = msg.to_text().ok()?;
    let client_msg: ClientMsg = serde_json::from_str(text).ok()?;
    match client_msg {
        ClientMsg::Hello { token, channel, role, session_id, cwd, version } => {
            Some(HelloInfo { token, channel, role, session_id, cwd, version })
        }
        _ => None,
    }
}
```

`serve`: parametro finale `registry: crate::connections::SharedRegistry,` (dopo `rt`), clonato
nel loop (`let registry = Arc::clone(&registry);`) e passato come ultimo argomento a
`handle_connection`, che lo riceve come `registry: crate::connections::SharedRegistry`.
Aggiornare il doc-comment di `serve` (`* registry — registro delle connessioni vive (2.0)`).

Nel handshake: `let (client_token, requested_channel) = match parse_hello(&first) {…}` diventa
`let hello = match parse_hello(&first) { Some(h) => h, None => {…} };` e le due variabili
successive sono `hello.token` / `hello.channel` (ogni `requested_channel.as_deref()` →
`hello.channel.as_deref()`; `client_token != token.as_str()` → `hello.token != token.as_str()`).

`main.rs`: prima della chiamata a `ws::serve` aggiungere
`let registry = orchestrator::connections::Registry::shared();` e passarlo come ultimo argomento.
`ws_integration.rs` `spawn_server`: ultimo argomento `orchestrator::connections::Registry::shared()`.

Run: `cargo test -p orchestrator parse_hello_reads 2>&1 | grep "test result"` → `1 passed`;
`cargo test -p orchestrator 2>&1 | grep -E "test result|FAILED"` → verde (comportamento v1 invariato).

- [ ] **Step 3: `shell_turn.rs` — RED**

```rust
// crates/orchestrator/src/shell_turn.rs
//! # shell_turn — esecuzione di un `Command` arrivato da una sessione shell
//!
//! `ws.rs` fa una cosa sola per la shell: costruisce [`ShellTurnDeps`] e
//! spawna [`run_shell_command`]. Tutto il resto vive qui, in ordine:
//! 1. `shell_slash::classify_shell_input` decide il tipo di riga (spec §3);
//! 2. le risposte immediate (`Error` di sintassi, `Done` muto, `/reset`,
//!    `OpenUiLocal`) vanno direttamente sulla connessione shell;
//! 3. `/ai "…"`, i comandi backend e `/ping` aprono un **turno**: un canale
//!    interno consumato da `surface::route_shell_turn` (che apre la finestra
//!    di output su `ui` e smista il resto) e alimentato da
//!    `core::handle_command` (o dal built-in `/ping`).
//!
//! Le dipendenze arrivano tutte per costruzione (nessun globale): è un
//! "command handler" con le sue collaborazioni esplicite, testabile con
//! un registro vuoto e un `StubAdapter`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use protocol::{CommandKind, ErrCode, ServerMsg};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::ai_adapter::AiAdapter;
use crate::connections::SharedRegistry;
use crate::local_confirm::{PendingConfirms, ShellConfirmer};
use crate::messages_client::ConversationHistory;
use crate::plugins::discovery::DiscoveredPlugin;
use crate::plugins::host::PluginHost;
use crate::plugins::transport::{PluginReader, PluginWriter};
use crate::runtime_config::RuntimeConfig;
use crate::shell_session::{ShellSessionState, ShellSessionToolClient};
use crate::shell_slash::{classify_shell_input, read_web_search_enabled, ShellInput, RESET_MESSAGE};
use crate::surface::{output_window_title, route_shell_turn, ShellTurn, NO_UI_ACK};

/// Collaborazioni di un comando shell (vedi doc-comment del modulo).
pub(crate) struct ShellTurnDeps {
    pub ai: Arc<dyn AiAdapter>,
    pub history: Arc<Mutex<ConversationHistory>>,
    pub pending_confirms: PendingConfirms,
    pub registry: SharedRegistry,
    pub plugin_host: Arc<Mutex<PluginHost>>,
    pub rt: Arc<RuntimeConfig>,
    pub shell: Arc<ShellSessionState>,
    pub out_tx: UnboundedSender<ServerMsg>,
    /// `Hello.version` della host, per la riga `lare-shell` di `/ping`.
    pub shell_version: Option<String>,
}

/// Timeout del gate `[Y/n]` sulla shell: come la UI locale v1 (180 s).
const CONFIRM_TIMEOUT: Duration = Duration::from_secs(180);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_adapter::StubAdapter;
    use crate::connections::Registry;
    use crate::tool_client::FakeToolClient;
    use crate::tool_client::ToolClient;

    fn deps(out_tx: UnboundedSender<ServerMsg>, registry: SharedRegistry) -> ShellTurnDeps {
        let mcp: Arc<dyn ToolClient> = Arc::new(FakeToolClient::success("ok"));
        let shell = Arc::new(ShellSessionState::new("s1", out_tx.clone(), "C:\\w", mcp, std::env::temp_dir()));
        ShellTurnDeps {
            ai: Arc::new(StubAdapter),
            history: Arc::new(Mutex::new(ConversationHistory::new())),
            pending_confirms: PendingConfirms::new(),
            registry,
            plugin_host: Arc::new(Mutex::new(futures::executor::block_on(PluginHost::start(
                vec![],
                |_p: &DiscoveredPlugin| -> std::io::Result<(Box<dyn PluginWriter>, Box<dyn PluginReader>)> { unreachable!() },
                std::path::Path::new("."),
            )))),
            rt: Arc::new(RuntimeConfig::for_test(&std::env::temp_dir())),
            shell,
            out_tx,
            shell_version: Some("2.0.0".into()),
        }
    }

    fn drain(rx: &mut tokio::sync::mpsc::UnboundedReceiver<ServerMsg>) -> Vec<ServerMsg> {
        let mut v = vec![];
        while let Ok(m) = rx.try_recv() { v.push(m); }
        v
    }

    #[tokio::test]
    async fn unknown_slash_yields_a_mute_done() {
        let (tx, mut rx) = unbounded_channel();
        run_shell_command(deps(tx, Registry::shared()), "c1".into(), "/nonesiste".into(), None, false, CancellationToken::new()).await;
        let out = drain(&mut rx);
        assert!(matches!(out.as_slice(), [ServerMsg::Done { exit_code: Some(0), .. }]), "{out:?}");
    }

    #[tokio::test]
    async fn ai_without_quotes_yields_error_only() {
        let (tx, mut rx) = unbounded_channel();
        run_shell_command(deps(tx, Registry::shared()), "c1".into(), "/ai ciao".into(), None, false, CancellationToken::new()).await;
        let out = drain(&mut rx);
        assert!(matches!(out.as_slice(), [ServerMsg::Error { code: ErrCode::RoutingError, .. }]), "{out:?}");
    }

    #[tokio::test]
    async fn reset_answers_not_applicable() {
        let (tx, mut rx) = unbounded_channel();
        run_shell_command(deps(tx, Registry::shared()), "c1".into(), "/reset".into(), None, false, CancellationToken::new()).await;
        let out = drain(&mut rx);
        assert!(matches!(&out[0], ServerMsg::Chunk { content, .. } if content == RESET_MESSAGE), "{out:?}");
        assert!(matches!(&out[1], ServerMsg::Done { exit_code: Some(0), .. }));
    }

    #[tokio::test]
    async fn open_ui_local_goes_to_ui_sink_or_warns_without_it() {
        let (tx, mut rx) = unbounded_channel();
        let registry = Registry::shared();
        run_shell_command(deps(tx.clone(), Arc::clone(&registry)), "c1".into(), "/config".into(), None, false, CancellationToken::new()).await;
        let out = drain(&mut rx);
        assert!(matches!(&out[0], ServerMsg::Chunk { content, .. } if content == NO_UI_ACK), "{out:?}");
        assert!(matches!(&out[1], ServerMsg::Done { exit_code: Some(1), .. }));

        let (ui_tx, mut ui_rx) = unbounded_channel();
        registry.lock().await.set_ui_sink(ui_tx);
        run_shell_command(deps(tx, registry), "c2".into(), "/library".into(), None, false, CancellationToken::new()).await;
        assert!(matches!(ui_rx.try_recv(), Ok(ServerMsg::OpenUiLocal { name }) if name == "library"));
        let out = drain(&mut rx);
        assert!(matches!(out.as_slice(), [ServerMsg::Done { exit_code: Some(0), .. }]), "{out:?}");
    }

    /// `/ai "x"` con `StubAdapter`: il testo dello stub finisce nella finestra
    /// (`OutputWindowContent` su `ui`), la shell riceve conferma + `Done`, la
    /// cwd del `Command` aggiorna la sessione.
    #[tokio::test]
    async fn ai_turn_opens_output_window_and_acks_the_shell() {
        let (tx, mut rx) = unbounded_channel();
        let registry = Registry::shared();
        let (ui_tx, mut ui_rx) = unbounded_channel();
        registry.lock().await.set_ui_sink(ui_tx);
        let d = deps(tx, registry);
        let shell = Arc::clone(&d.shell);
        run_shell_command(d, "c1".into(), "/ai \"ciao\"".into(), Some("C:\\nuova".into()), false, CancellationToken::new()).await;
        // Il turno gira in task spawnati: attendi il Done sulla shell.
        let done = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(ServerMsg::Done { .. }) = rx.recv().await { break; }
            }
        }).await;
        assert!(done.is_ok(), "nessun Done entro 5 s");
        assert_eq!(shell.cwd().await, "C:\\nuova");
        let ui = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(ServerMsg::OutputWindowContent { markdown, .. }) = ui_rx.recv().await { break markdown; }
            }
        }).await.expect("OutputWindowContent");
        assert!(ui.contains("[stub AI] ricevuto: ciao"), "{ui}");
    }

    /// `/ping` senza plugin e senza `ui`: tabella con le righe degradate,
    /// consegnata sulla finestra (qui assente → solo l'avviso) e `Done`.
    #[tokio::test]
    async fn ping_without_layers_still_completes() {
        let (tx, mut rx) = unbounded_channel();
        run_shell_command(deps(tx, Registry::shared()), "c1".into(), "/ping".into(), None, false, CancellationToken::new()).await;
        let done = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match rx.recv().await {
                    Some(ServerMsg::Done { exit_code, .. }) => break exit_code,
                    Some(_) => continue,
                    None => panic!("canale chiuso senza Done"),
                }
            }
        }).await.expect("Done");
        assert_eq!(done, Some(0));
    }
}
```

`RuntimeConfig::for_test(&Path)` esiste (`#[cfg(test)]` in `runtime_config.rs`, riga 77); se la firma cambia,
argomenti, usare `RuntimeConfig { config_dir: std::env::temp_dir(), startup: Default::default() }`.
`futures::executor::block_on` è disponibile (`futures = "0.3"` in `Cargo.toml`); in alternativa
rendere `deps` `async` e attendere `PluginHost::start`.

Run: `cargo test -p orchestrator shell_turn 2>&1 | grep -E "^error" | head -3` → RED.

- [ ] **Step 4: `shell_turn.rs` — implementazione** (fra `CONFIRM_TIMEOUT` e `#[cfg(test)]`)

```rust
/// Punto d'ingresso: consuma il `Command{id, input, cwd, web_search}` di una
/// sessione shell. Ritorna appena il lavoro è avviato (i turni girano in
/// task spawnati): `ws.rs` lo chiama dentro `tokio::spawn`, quindi il loop
/// della connessione resta libero per `CancelCommand`/`ExecResult`.
pub(crate) async fn run_shell_command(
    deps: ShellTurnDeps,
    id: String,
    input: String,
    cwd: Option<String>,
    web_search: bool,
    cancel: CancellationToken,
) {
    // `Command.cwd` = `$PWD` del runspace al momento dell'invio (spec §4.6).
    if let Some(c) = cwd.as_deref().filter(|s| !s.is_empty()) {
        deps.shell.set_cwd(c).await;
    }
    let known = |c: &str| crate::core::KNOWN_BACKEND_SLASHES.contains(&c);
    match classify_shell_input(&input, &known) {
        ShellInput::SyntaxError(message) => {
            let _ = deps.out_tx.send(ServerMsg::Error { id, code: ErrCode::RoutingError, message });
        }
        ShellInput::Discard(cmd) => {
            tracing::info!("discard slash from shell session {}: /{cmd}", deps.shell.session_id());
            let _ = deps.out_tx.send(ServerMsg::Done { id, exit_code: Some(0) });
        }
        ShellInput::Reset => {
            let _ = deps.out_tx.send(ServerMsg::Chunk { id: id.clone(), content: RESET_MESSAGE.to_string() });
            let _ = deps.out_tx.send(ServerMsg::Done { id, exit_code: Some(0) });
        }
        ShellInput::OpenUiLocal(name) => {
            let sink = deps.registry.lock().await.ui_sink();
            match sink {
                Some(ui) => {
                    let _ = ui.send(ServerMsg::OpenUiLocal { name });
                    let _ = deps.out_tx.send(ServerMsg::Done { id, exit_code: Some(0) });
                }
                None => {
                    let _ = deps.out_tx.send(ServerMsg::Chunk { id: id.clone(), content: NO_UI_ACK.to_string() });
                    let _ = deps.out_tx.send(ServerMsg::Done { id, exit_code: Some(1) });
                }
            }
        }
        ShellInput::Ping => {
            let turn_tx = start_turn(&deps, &id, &input).await;
            tokio::spawn(async move { run_ping_turn(deps, id, turn_tx).await });
        }
        ShellInput::Nl(text) => {
            let turn_tx = start_turn(&deps, &id, &input).await;
            tokio::spawn(async move { run_ai_turn(deps, id, text, CommandKind::Nl, web_search, cancel, turn_tx).await });
        }
        ShellInput::Backend => {
            let turn_tx = start_turn(&deps, &id, &input).await;
            tokio::spawn(async move { run_ai_turn(deps, id, input, CommandKind::Auto, web_search, cancel, turn_tx).await });
        }
    }
}

/// Apre il turno: canale interno + router di superficie (che apre subito la
/// finestra di output su `ui`). Ritorna il sender su cui il produttore emette.
async fn start_turn(deps: &ShellTurnDeps, id: &str, input: &str) -> UnboundedSender<ServerMsg> {
    let (turn_tx, turn_rx) = unbounded_channel::<ServerMsg>();
    let turn = ShellTurn {
        id: id.to_string(),
        session_id: deps.shell.session_id().to_string(),
        window_id: id.to_string(),
        title: output_window_title(input),
    };
    let ui = deps.registry.lock().await.ui_sink();
    tokio::spawn(route_shell_turn(turn_rx, deps.out_tx.clone(), ui, turn));
    turn_tx
}

/// `/ai "…"` (Nl) o comando backend (Auto): `core::handle_command` con il
/// `ToolClient` della sessione e il gate della shell. La ricerca web vale se
/// il client l'ha chiesta O se l'utente l'ha attivata in `/config`.
async fn run_ai_turn(
    deps: ShellTurnDeps,
    id: String,
    input: String,
    kind: CommandKind,
    web_search: bool,
    cancel: CancellationToken,
    turn_tx: UnboundedSender<ServerMsg>,
) {
    let tools = ShellSessionToolClient::for_turn(&deps.shell, id.clone());
    let confirmer = ShellConfirmer::new(deps.out_tx.clone(), deps.pending_confirms.clone(), CONFIRM_TIMEOUT, id.clone());
    let cwd = deps.shell.cwd().await;
    let web_search = web_search || read_web_search_enabled(&deps.rt.config_dir);
    let mut guard = deps.history.lock().await;
    crate::core::handle_command(
        &id,
        &input,
        kind,
        if cwd.is_empty() { None } else { Some(&cwd) },
        &mut guard,
        deps.ai.as_ref(),
        tools.as_ref(),
        None,
        None,
        web_search,
        Some(&confirmer),
        Some(cancel),
        turn_tx,
    )
    .await;
}

/// Built-in `/ping` (spec §3.1): sonde reali, poi `ping::run_ping` formatta.
async fn run_ping_turn(deps: ShellTurnDeps, id: String, turn_tx: UnboundedSender<ServerMsg>) {
    // Plugin: `find` sotto lock breve; la sonda (fino a 5 s) FUORI dal lock.
    let found = deps.plugin_host.lock().await.find("ping");
    let plugin = match found {
        None => None,
        Some((p, storage_dir)) => {
            let config_dir = deps.rt.config_dir.clone();
            let mut make = |p: &DiscoveredPlugin| {
                crate::plugins::transport::spawn_plugin(&p.bin_path, &config_dir).map(|(w, r)| {
                    (Box::new(w) as Box<dyn PluginWriter>, Box::new(r) as Box<dyn PluginReader>)
                })
            };
            Some(crate::plugins::host::probe(&p, &storage_dir, &mut make).await.map(|d| (p.manifest.version.clone(), d)))
        }
    };
    // ui.exe: `UiPing` sul sink, `UiPong` risolto dal registro (arm in ws.rs).
    let sink = deps.registry.lock().await.ui_sink();
    let ui = match sink {
        None => None,
        Some(sink) => {
            let ping_id = format!("{:032x}", rand::random::<u128>());
            let rx = deps.registry.lock().await.register_ui_ping(&ping_id);
            let start = Instant::now();
            if sink.send(ServerMsg::UiPing { id: ping_id.clone() }).is_err() {
                Some(Err("invio di UiPing fallito".to_string()))
            } else {
                match tokio::time::timeout(crate::ping::UI_PING_TIMEOUT, rx).await {
                    Ok(Ok(version)) => Some(Ok((version, start.elapsed()))),
                    _ => {
                        deps.registry.lock().await.resolve_ui_ping(&ping_id, String::new());
                        Some(Err(format!("nessun UiPong entro {} s", crate::ping::UI_PING_TIMEOUT.as_secs())))
                    }
                }
            }
        }
    };
    let markdown = crate::ping::run_ping(deps.shell.session_id(), deps.shell_version.as_deref(), plugin, ui);
    let _ = turn_tx.send(ServerMsg::Chunk { id: id.clone(), content: markdown });
    let _ = turn_tx.send(ServerMsg::Done { id, exit_code: Some(0) });
}
```

`lib.rs`: `pub mod shell_turn;`. Run: `cargo test -p orchestrator shell_turn 2>&1 | grep "test result"`
→ `6 passed`.

- [ ] **Step 5: `handle_connection` — sink `ui`, sessione shell, arm nuovi, teardown**

Subito dopo `let (out_tx, mut out_rx) = unbounded_channel::<ServerMsg>();` sostituire il blocco
`if connection_owns_plugin_sink(…) { plugin_host…set_server_tx }` e il blocco `if let Some(h) = &aichat { SetServerTx }` con:

```rust
    // ── Ruolo della connessione (2.0, spec §4.1/§5) ─────────────────────────
    // Il sink `ui` (finestre, plugin, AI Chat) è UNA connessione per macchina:
    // `role: Ui` senza canale — stesso predicato del sink dei plugin v1. Una
    // sessione shell non deve mai rubarlo (spec: "le sessioni shell non
    // ricevono SetServerTx").
    let is_ui_sink = hello.role == protocol::Role::Ui && connection_owns_plugin_sink(hello.channel.as_deref());
    if is_ui_sink {
        plugin_host.lock().await.set_server_tx(out_tx.clone());
        registry.lock().await.set_ui_sink(out_tx.clone());
    }
    if hello.role == protocol::Role::Ui {
        if let Some(h) = &aichat {
            let _ = h.send(ServiceEvent::SetServerTx(out_tx.clone()));
        }
    }
    // Sessione shell: il suo `ToolClient` è la shell dell'utente (Task 4);
    // `tools` (McpToolClient condiviso) resta per `open_target`/routine.
    let shell: Option<Arc<crate::shell_session::ShellSessionState>> = if hello.role == protocol::Role::Shell {
        let session_id = hello.session_id.clone().unwrap_or_else(|| format!("{:08x}", rand::random::<u32>()));
        registry.lock().await.register_shell(&session_id, out_tx.clone());
        info!("sessione shell {session_id} connessa (versione {:?})", hello.version);
        Some(Arc::new(crate::shell_session::ShellSessionState::new(
            session_id,
            out_tx.clone(),
            hello.cwd.clone().unwrap_or_default(),
            Arc::clone(&tools),
            rt.config_dir.clone(),
        )))
    } else {
        None
    };
```

L'emissione del `Cwd` iniziale (`let initial_cwd = …; if !initial_cwd.is_empty() { send Cwd }`)
va racchiusa in `if hello.role == protocol::Role::Ui { … }` (la shell possiede la sua cwd).

Nell'arm `ClientMsg::Command { … }`, **dopo** il ramo dei plugin (`if let Some(plugin_id) =
plugin_command(…) { …; continue; }`) e **prima** di `if let Some(query) = parse_find(&input)`:

```rust
                // ── Sessione shell (2.0, spec §3/§4): pre-router + turno con finestra ──
                // Tutto in `shell_turn.rs`; qui solo il cancel token (per
                // `CancelCommand`, come i comandi v1) e lo spawn.
                if let Some(shell) = &shell {
                    let cancel = CancellationToken::new();
                    commands.insert(id.clone(), cancel.clone());
                    let deps = crate::shell_turn::ShellTurnDeps {
                        ai: Arc::clone(&ai),
                        history: Arc::clone(&history),
                        pending_confirms: pending_confirms.clone(),
                        registry: Arc::clone(&registry),
                        plugin_host: Arc::clone(&plugin_host),
                        rt: Arc::clone(&rt),
                        shell: Arc::clone(shell),
                        out_tx: out_tx.clone(),
                        shell_version: hello.version.clone(),
                    };
                    tokio::spawn(crate::shell_turn::run_shell_command(deps, id, input, cwd, web_search, cancel));
                    continue;
                }
```

Arm `ClientMsg::CancelCommand { id }` diventa:

```rust
            ClientMsg::CancelCommand { id } => {
                if let Some(cmd_token) = commands.remove(&id) {
                    cmd_token.cancel();
                }
                // Shell: Ctrl+C ha già fermato la pipeline lì (spec §4.4) —
                // nessun ExecResult arriverà: sblocca il run_in_session pendente.
                if let Some(s) = &shell {
                    s.abort_turn(&id).await;
                }
            }
```

Arm nuovi (accanto a `ToolConfirmResponse`):

```rust
            // Esito di un ExecInShell (solo sessioni shell; da una ui è un no-op).
            ClientMsg::ExecResult { turn_id: _, exec_id, exit_code, output, cwd } => {
                if let Some(s) = &shell {
                    s.resolve_exec(&exec_id, crate::shell_session::ExecReply { exit_code, output, cwd }).await;
                }
            }

            // Risposta di ui.exe a UiPing (built-in /ping): risolta nel registro.
            ClientMsg::UiPong { id, version } => {
                registry.lock().await.resolve_ui_ping(&id, version);
            }
```

Teardown (prima di `tools.shutdown().await;`):

```rust
    // Registro (2.0): la sessione shell sparisce; il sink ui viene azzerato
    // SOLO se è ancora il nostro (una ui nuova può averlo già sostituito).
    if let Some(s) = &shell {
        s.abort_all().await;
        registry.lock().await.unregister_shell(s.session_id());
    }
    registry.lock().await.clear_ui_sink_if(&out_tx);
```

e `UiClosed` verso AI Chat va mandato solo `if hello.role == protocol::Role::Ui` (una shell che
si chiude non è "l'intera UI che sparisce").

- [ ] **Step 6: GREEN completo**

Run: `cargo test -p orchestrator 2>&1 | grep -E "test result|FAILED"` → tutto verde.
Run: `cargo test 2>&1 | grep "test result" | awk '{s+=$4} END {print s}'` → 1418 + i nuovi
(Task 1: 5, 2: 4, 3: 4, 4: 6, 5: 5, 6: 7, 7: 6, 8: 7) = **1462** (riverificare col conteggio reale;
scarto ammesso solo se motivato nel report).
Run: `cargo clippy -p orchestrator --all-targets 2>&1 | grep -E "shell_turn|ws.rs:(1[5-9]|[2-7][0-9])[0-9]" ` → vuoto.
Run: `cargo fmt -- crates/orchestrator/src/shell_turn.rs crates/orchestrator/src/connections.rs crates/orchestrator/src/shell_session.rs crates/orchestrator/src/surface.rs crates/orchestrator/src/shell_slash.rs crates/orchestrator/src/ping.rs` (solo i file nuovi; `ws.rs`/`core.rs`/`host.rs` copiati dalla v1 NON vanno formattati).

- [ ] **Step 7: commit**

```bash
git add crates/orchestrator/src/shell_turn.rs crates/orchestrator/src/ws.rs crates/orchestrator/src/main.rs crates/orchestrator/src/lib.rs crates/orchestrator/tests/ws_integration.rs
git commit -m "feat(orchestrator): canale shell nel WS — HelloInfo, registro, turni con finestra di output, ExecResult/UiPong"
```

---

### Task 9: `ui` — finestra di output aggiornabile, `OpenUiLocal`, `/help` singleton, `UiPong`

**Files:**
- Modify: `crates/ui/src-tauri/src/main.rs` (`open_markdown_window` con `label` opzionale;
  nuovi comandi `open_output_window`, `get_ui_version`; registrazione in `generate_handler!`)
- Create: `crates/ui/frontend/ui-local.mjs` + `crates/ui/frontend/ui-local.test.mjs`
- Modify: `crates/ui/frontend/host-dispatch.mjs` + `host-dispatch.test.mjs`
- Modify: `crates/ui/frontend/host.js` (case nuovi, `openMarkdownWindow` con label, versione)
- Modify: `crates/ui/frontend/window.js` (listener `output:content`, contenuto salvabile aggiornato)
- Modify: `crates/ui/frontend/ws-client.js` (`sendUiPong`)
- Test: `node --test crates/ui/frontend/*.test.mjs`, `cargo build -p ui`

**Interfaces:**
- Consumes: wire `open_output_window{window_id,title}`, `output_window_content{window_id,markdown}`,
  `open_ui_local{name}`, `ui_ping{id}`; `ClientMsg::UiPong{id, version}`.
- Produces: Tauri `open_markdown_window(title, content, kind, source_file, label?: string)`,
  `open_output_window(window_id, title)`, `get_ui_version() -> String`; evento Tauri globale
  `output:content` `{window_id, markdown}`; `resolveUiLocal(name, channels)`, `markdownWindowLabel(kind)`,
  `outputWindowIdFromLabel(label)`.

- [ ] **Step 1: test RED (JS puro)**

`crates/ui/frontend/ui-local.test.mjs`:

```js
import { test } from "node:test";
import assert from "node:assert/strict";
import { resolveUiLocal, markdownWindowLabel, outputWindowIdFromLabel } from "./ui-local.mjs";
import { EXTERNAL_TOOL_CHANNELS } from "./external-channels.js";

test("open_ui_local: i tre singleton locali mappano sui comandi Tauri esistenti", () => {
  assert.deepEqual(resolveUiLocal("config", EXTERNAL_TOOL_CHANNELS), { cmd: "open_config_window", args: undefined });
  assert.deepEqual(resolveUiLocal("library", EXTERNAL_TOOL_CHANNELS), { cmd: "open_library_window", args: undefined });
  assert.deepEqual(resolveUiLocal("aichat", EXTERNAL_TOOL_CHANNELS), { cmd: "open_aichat_window", args: undefined });
});

test("open_ui_local: un id di canale esterno apre la sua finestra col titolo della tabella", () => {
  assert.deepEqual(resolveUiLocal("nmap", EXTERNAL_TOOL_CHANNELS), {
    cmd: "open_external_channel_window",
    args: { channelId: "nmap", windowTitle: "Lare — nmap" },
  });
  assert.deepEqual(resolveUiLocal("financial-markets", EXTERNAL_TOOL_CHANNELS).args.channelId, "financial-markets");
});

test("open_ui_local: nome ignoto → null (nessun comando inventato)", () => {
  assert.equal(resolveUiLocal("boh", EXTERNAL_TOOL_CHANNELS), null);
  assert.equal(resolveUiLocal("", EXTERNAL_TOOL_CHANNELS), null);
});

test("/help è singleton (label fissa), le altre finestre Markdown no", () => {
  assert.equal(markdownWindowLabel("help"), "help");
  assert.equal(markdownWindowLabel("markdown"), null);
  assert.equal(markdownWindowLabel(undefined), null);
});

test("window_id dalla label di una finestra di output", () => {
  assert.equal(outputWindowIdFromLabel("output-c1"), "c1");
  assert.equal(outputWindowIdFromLabel("output-abc-123"), "abc-123");
  assert.equal(outputWindowIdFromLabel("md-1-2"), null);
  assert.equal(outputWindowIdFromLabel(undefined), null);
});
```

`host-dispatch.test.mjs`, primo test: aggiungere alla lista `"open_output_window",
"output_window_content", "open_ui_local", "ui_ping"`.

Run: `node --test crates/ui/frontend/ui-local.test.mjs crates/ui/frontend/host-dispatch.test.mjs 2>&1 | grep -E "^# (pass|fail)"`
→ `fail` (modulo assente; il test di dispatch fallisce sui 4 tipi nuovi).

- [ ] **Step 2: JS puro — GREEN**

`crates/ui/frontend/ui-local.mjs`:

```js
// ui-local.mjs — Logica pura (senza DOM, senza Tauri) del canale shell lato ui:
// quale comando Tauri apre una finestra "locale" chiesta con `open_ui_local`,
// quali finestre Markdown sono singleton (D15), come si legge il window_id di
// una finestra di output dalla sua label. Testabile con node:test.

/** Prefisso delle label delle finestre di output (`open_output_window`). */
export const OUTPUT_LABEL_PREFIX = "output-";

const LOCAL_WINDOWS = {
  config: "open_config_window",
  library: "open_library_window",
  aichat: "open_aichat_window",
};

/**
 * `open_ui_local{name}` → `{cmd, args}` da passare a `invoke`, o `null` se il
 * nome non è né un singleton locale né un canale esterno della tabella.
 * @param {string} name
 * @param {{id:string, windowTitle:string}[]} channels — EXTERNAL_TOOL_CHANNELS
 */
export function resolveUiLocal(name, channels) {
  if (!name) return null;
  if (LOCAL_WINDOWS[name]) return { cmd: LOCAL_WINDOWS[name], args: undefined };
  const ch = channels.find((c) => c.id === name);
  if (ch) return { cmd: "open_external_channel_window", args: { channelId: ch.id, windowTitle: ch.windowTitle } };
  return null;
}

/** Label fissa per le finestre Markdown singleton: solo `/help` (D15). */
export function markdownWindowLabel(kind) {
  return kind === "help" ? "help" : null;
}

/** `output-<id>` → `<id>`; qualunque altra label → null. */
export function outputWindowIdFromLabel(label) {
  if (typeof label !== "string" || !label.startsWith(OUTPUT_LABEL_PREFIX)) return null;
  return label.slice(OUTPUT_LABEL_PREFIX.length);
}
```

`host-dispatch.mjs`: nel `Set` `WINDOW` aggiungere `"open_output_window", "output_window_content",
"open_ui_local", "ui_ping"` con il commento `// canale shell (2.0, spec §3.2/§4.1)`; aggiornare il
commento di testa (`"window"` include anche "apre una finestra locale chiesta da una shell" e
"risponde a un ping di `ui`").

Run: `node --test crates/ui/frontend/*.test.mjs 2>&1 | grep -E "^# (pass|fail)"` → `pass 224`, `fail 0`.

- [ ] **Step 3: Rust (`main.rs`)**

`open_markdown_window`: aggiungere il parametro `label: Option<String>` (dopo `source_file`) e,
in testa al corpo:

```rust
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
        None => { /* il blocco esistente `md-<ts>-<ctr>` */ }
    };
```

(il blocco `let label = { … format!("md-{ts}-{ctr}") };` esistente diventa il ramo `None`).

Estrarre la costruzione della finestra in una funzione libera, usata da `open_markdown_window` e
dal comando nuovo:

```rust
/// Crea una finestra Markdown chromeless (stile di ogni finestra Lare) con
/// `label` e `title`. Il contenuto deve essere GIÀ in `WindowContentStore`.
fn build_markdown_window(app: &AppHandle, label: &str, title: &str) -> Result<(), String> {
    WebviewWindowBuilder::new(app, label, WebviewUrl::App("window.html".into()))
        .title(title)
        .inner_size(800.0, 600.0)
        .decorations(false)
        .transparent(true)
        .resizable(true)
        .always_on_top(true)
        .focused(true)
        .build()
        .map(|_| ())
        .map_err(|e| format!("WebviewWindowBuilder::build() failed: {e}"))
}
```

Comando nuovo (dopo `open_markdown_window`):

```rust
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
```

Registrare `open_output_window` e `get_ui_version` in `generate_handler!` (accanto a
`open_markdown_window`).

Run: `cargo build -p ui 2>&1 | grep -E "^error|Finished" | head -3` → `Finished`.

- [ ] **Step 4: `host.js`, `ws-client.js`, `window.js`**

`ws-client.js`, accanto a `sendToolConfirmResponse`:

```js
  /** Risposta a `ui_ping` (built-in /ping): `{type:"ui_pong", id, version}`. */
  sendUiPong(id, version) {
    if (!this._isOpen()) return false;
    this._send({ type: "ui_pong", id, version: String(version ?? "") });
    return true;
  }
```

`host.js`:
- import: `import { resolveUiLocal, markdownWindowLabel } from "./ui-local.mjs";` e
  `import { EXTERNAL_TOOL_CHANNELS } from "./external-channels.js";` (se non già importato).
- variabile di modulo `let uiVersion = "";` valorizzata in `bootstrap()` PRIMA di `initClient()`:
  `uiVersion = (await invokeCmd("get_ui_version")) ?? "";`.
- `openMarkdownWindow(title, content, kind = "markdown")`: la `invokeCmd` passa anche
  `label: markdownWindowLabel(kind)`.
- nello `switch` di `handleServerMsg`, dopo il case `routine_save_preview`:

```js
    // ── Canale shell (2.0, spec §3.2/§4.1) ─────────────────────────────────
    // Finestra di output di un comando slash originato da una shell: si apre
    // subito col segnaposto, il contenuto arriva a Done via evento globale.
    case "open_output_window":
      invokeCmd("open_output_window", { windowId: msg.window_id, title: msg.title })
        .catch((e) => console.error("[host] open_output_window error:", e));
      break;
    case "output_window_content":
      // emitToPlugin è un emit globale generico (nome storico): la finestra
      // di output filtra per window_id come fanno le finestre plugin.
      emitToPlugin("output:content", { window_id: msg.window_id, markdown: msg.markdown });
      break;
    // Finestra locale chiesta da una shell (/config, /library, /aichat, canali esterni).
    case "open_ui_local": {
      const target = resolveUiLocal(msg.name, EXTERNAL_TOOL_CHANNELS);
      if (!target) { console.warn("[host] open_ui_local: nome ignoto", msg.name); break; }
      invokeCmd(target.cmd, target.args).catch((e) => console.error(`[host] ${target.cmd} error:`, e));
      break;
    }
    // Built-in /ping: rispondi con la versione di ui.exe.
    case "ui_ping":
      if (client) client.sendUiPong(msg.id, uiVersion);
      break;
```

`window.js`: importare `import { outputWindowIdFromLabel } from "./ui-local.mjs";`. Dopo il
render iniziale (dopo `renderMarkdown(...)` del contenuto di `take_window_content`) e PRIMA della
sezione "Espandi": trasformare `const saveContent = data.content || "";` in `let saveContent = …`
(il salvataggio deve archiviare il contenuto AGGIORNATO) e aggiungere:

```js
  // ── Finestra di output del canale shell (2.0, spec §3.2) ─────────────────
  // Solo le finestre `output-<id>` ricevono aggiornamenti dopo l'apertura:
  // host.js emette `output:content` a Done/Error; qui si filtra per window_id
  // (dalla propria label) e si ri-renderizza. Il testo salvabile in Library è
  // quello aggiornato, non il segnaposto.
  const myOutputId = outputWindowIdFromLabel(window.__TAURI__?.window?.getCurrentWindow?.()?.label);
  if (myOutputId && tauriEvent?.listen) {
    tauriEvent.listen("output:content", (ev) => {
      const p = ev.payload || {};
      if (p.window_id !== myOutputId) return;
      renderMarkdown(p.markdown || "");
      saveContent = p.markdown || "";
      currentContent = p.markdown || "";
    }).catch((e) => console.error("[window] listen output:content error:", e));
  }
```

(Se `currentContent` è dichiarata DOPO questo punto nel file, spostare il blocco subito dopo la
sua dichiarazione: la regola è "prima del wiring di Espandi, dopo che `currentContent` esiste".)

Run: `cargo clean -p ui && cargo build -p ui 2>&1 | grep -E "^error|Finished"` → `Finished`
(`generate_context!` incorpora il frontend a compile time: senza `clean` la build può non
ricompilare). `cargo test -p ui 2>&1 | grep "test result"` → invariato (87 + 49).

- [ ] **Step 5: verifica dal vivo (controller, non subagente)**

```powershell
cargo build ; .\deploy_test_run.ps1
# terminale 1
cargo run -p orchestrator -- --config-dir "Test Run\Configuration" --console-log
# terminale 2
cargo run -p ui -- --config-dir "Test Run\Configuration"
```

Al Task 10 il client di sviluppo permette di esercitare `open_output_window`/`open_ui_local` dal
vivo; qui basta che `ui.exe` parta e la pagina host si connetta (log `ws status: connected`).

- [ ] **Step 6: commit**

```bash
git add crates/ui/src-tauri/src/main.rs crates/ui/frontend/ui-local.mjs crates/ui/frontend/ui-local.test.mjs crates/ui/frontend/host-dispatch.mjs crates/ui/frontend/host-dispatch.test.mjs crates/ui/frontend/host.js crates/ui/frontend/window.js crates/ui/frontend/ws-client.js
git commit -m "feat(ui): finestra di output aggiornabile, open_ui_local, /help singleton, ui_pong"
```

---

### Task 10: client di sviluppo e scenari e2e automatici

**Files:**
- Create: `scripts/dev/shell-client.mjs`
- Modify: `crates/orchestrator/tests/ws_integration.rs` (`spawn_server_with`, adapter finto, 5 scenari)
- Test: `cargo test -p orchestrator --test ws_integration`

**Interfaces:**
- Consumes: tutto il Task 8; `AiAdapter` (`respond`, `provider`, `chat_reply` sono i metodi
  richiesti — vedi `StubAdapter` in `ai_adapter.rs:250-290`), `ToolClient::dispatch`,
  `shell_slash::AI_SYNTAX_ERROR`, `shell_slash::RESET_MESSAGE`.

- [ ] **Step 1: scenari RED**

In `ws_integration.rs`, rifattorizzare `spawn_server(token)` in
`spawn_server_with(token, ai: Arc<dyn AiAdapter>)` (il corpo attuale con `ai` al posto di
`Arc::new(StubAdapter)`) e `spawn_server(token)` = `spawn_server_with(token, Arc::new(StubAdapter))`.
Aggiungere gli import `orchestrator::ai_adapter::{AiAdapter, ToolConfirmer}`,
`orchestrator::agent::TurnOptions`, `orchestrator::messages_client::ConversationHistory`,
`orchestrator::tool_client::ToolClient`, `protocol::{ErrCode, Role}`, `async_trait::async_trait`,
`tokio_util::sync::CancellationToken`, `tokio::sync::mpsc::UnboundedSender`.

Helper e adapter finto (dopo `recv`):

```rust
/// Adapter AI finto che, come farebbe Claude, propone UN comando: chiede il
/// gate (se il confirmer lo gateizza), esegue `run_in_session` via `dispatch`
/// e riporta l'output come testo. Serve a provare il round-trip completo
/// ToolConfirmRequest → ExecInShell → ExecResult senza API reali.
struct ToolCallingStubAdapter;

#[async_trait]
impl AiAdapter for ToolCallingStubAdapter {
    #[allow(clippy::too_many_arguments)]
    async fn respond(
        &self,
        id: &str,
        _input: &str,
        _history: &mut ConversationHistory,
        tools: &dyn ToolClient,
        _opts: TurnOptions,
        confirmer: Option<&dyn ToolConfirmer>,
        _cancel: Option<CancellationToken>,
        tx: UnboundedSender<ServerMsg>,
    ) {
        let approved = match confirmer {
            Some(c) if c.should_gate("run_in_session") => c.confirm("$ echo x").await,
            _ => true,
        };
        if !approved {
            let _ = tx.send(ServerMsg::Chunk { id: id.to_string(), content: "annullato".into() });
            let _ = tx.send(ServerMsg::Done { id: id.to_string(), exit_code: None });
            return;
        }
        let out = tools.dispatch("run_in_session", &serde_json::json!({"command": "echo x"})).await;
        let _ = tx.send(ServerMsg::Chunk { id: id.to_string(), content: format!("output: {}", out.output.trim()) });
        let _ = tx.send(ServerMsg::Done { id: id.to_string(), exit_code: None });
    }

    fn provider(&self) -> String {
        "tool-calling-stub".to_string()
    }

    async fn chat_reply(&self, _my_ai_label: &str, _history: &[orchestrator::messages_client::Message], _request: &str, _cancel: Option<CancellationToken>) -> String {
        String::new()
    }
}

fn shell_hello(token: &str) -> ClientMsg {
    ClientMsg::Hello {
        token: token.into(), channel: None, role: Role::Shell,
        session_id: Some("s1".into()), cwd: Some("C:\\w".into()), version: Some("2.0.0".into()),
    }
}

fn ui_hello(token: &str) -> ClientMsg {
    ClientMsg::Hello { token: token.into(), channel: None, role: Role::Ui, session_id: None, cwd: None, version: None }
}

fn command(id: &str, input: &str) -> ClientMsg {
    ClientMsg::Command {
        id: id.into(), input: input.into(), input_mode: InputMode::Keyboard,
        command_type: CommandKind::Auto, cwd: Some("C:\\w".into()), web_search: false,
    }
}

/// Riceve finché `pred` è vera (max 5 s), ritornando il messaggio che l'ha soddisfatta.
async fn recv_until(
    source: &mut (impl StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin),
    pred: impl Fn(&ServerMsg) -> bool,
) -> ServerMsg {
    tokio::time::timeout(tokio::time::Duration::from_secs(5), async {
        loop {
            let m = recv(source).await;
            if pred(&m) { return m; }
        }
    }).await.expect("timeout: messaggio atteso mai arrivato")
}
```

Scenari:

```rust
#[tokio::test]
async fn shell_unknown_slash_gets_a_mute_done_and_no_cwd() {
    let url = spawn_server("tok-shell-1").await;
    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut sink, mut source) = ws.split();
    send(&mut sink, &shell_hello("tok-shell-1")).await;
    assert!(matches!(recv(&mut source).await, ServerMsg::ServerInfo { .. }));
    send(&mut sink, &command("c1", "/nonesiste")).await;
    // Il PRIMO messaggio dopo ServerInfo è Done (niente Cwd, niente Error).
    let next = recv(&mut source).await;
    assert!(matches!(next, ServerMsg::Done { exit_code: Some(0), .. }), "{next:?}");
}

#[tokio::test]
async fn shell_ai_without_quotes_gets_syntax_error() {
    let url = spawn_server("tok-shell-2").await;
    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut sink, mut source) = ws.split();
    send(&mut sink, &shell_hello("tok-shell-2")).await;
    recv(&mut source).await;
    send(&mut sink, &command("c1", "/ai ciao")).await;
    let m = recv(&mut source).await;
    assert!(matches!(&m, ServerMsg::Error { code: ErrCode::RoutingError, message, .. } if message == orchestrator::shell_slash::AI_SYNTAX_ERROR), "{m:?}");
}

#[tokio::test]
async fn shell_reset_and_open_ui_local_reach_the_ui_sink() {
    let url = spawn_server("tok-shell-3").await;
    let (ui_ws, _) = connect_async(&url).await.unwrap();
    let (mut ui_sink, mut ui_source) = ui_ws.split();
    send(&mut ui_sink, &ui_hello("tok-shell-3")).await;
    recv(&mut ui_source).await; // ServerInfo
    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut sink, mut source) = ws.split();
    send(&mut sink, &shell_hello("tok-shell-3")).await;
    recv(&mut source).await;
    send(&mut sink, &command("c1", "/reset")).await;
    let m = recv(&mut source).await;
    assert!(matches!(&m, ServerMsg::Chunk { content, .. } if content == orchestrator::shell_slash::RESET_MESSAGE), "{m:?}");
    assert!(matches!(recv(&mut source).await, ServerMsg::Done { .. }));
    send(&mut sink, &command("c2", "/config")).await;
    let m = recv_until(&mut ui_source, |m| matches!(m, ServerMsg::OpenUiLocal { .. })).await;
    assert!(matches!(&m, ServerMsg::OpenUiLocal { name } if name == "config"));
    assert!(matches!(recv(&mut source).await, ServerMsg::Done { exit_code: Some(0), .. }));
}

/// Il round-trip completo dello spec §4.2 con un'AI finta che propone `echo x`.
#[tokio::test]
async fn shell_ai_turn_round_trips_gate_exec_and_output_window() {
    let url = spawn_server_with("tok-shell-4", Arc::new(ToolCallingStubAdapter)).await;
    let (ui_ws, _) = connect_async(&url).await.unwrap();
    let (mut ui_sink, mut ui_source) = ui_ws.split();
    send(&mut ui_sink, &ui_hello("tok-shell-4")).await;
    recv(&mut ui_source).await;
    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut sink, mut source) = ws.split();
    send(&mut sink, &shell_hello("tok-shell-4")).await;
    recv(&mut source).await;

    send(&mut sink, &command("c1", "/ai \"stampa x\"")).await;
    // ui: la finestra si apre subito, col titolo derivato dal testo.
    let m = recv_until(&mut ui_source, |m| matches!(m, ServerMsg::OpenOutputWindow { .. })).await;
    assert!(matches!(&m, ServerMsg::OpenOutputWindow { window_id, title } if window_id == "c1" && title == "stampa x"), "{m:?}");
    // shell: gate → accetta.
    let gate = recv_until(&mut source, |m| matches!(m, ServerMsg::ToolConfirmRequest { .. })).await;
    let gate_id = match gate { ServerMsg::ToolConfirmRequest { id, commands } => { assert_eq!(commands, "$ echo x"); id } _ => unreachable!() };
    send(&mut sink, &ClientMsg::ToolConfirmResponse { id: gate_id, accept: true }).await;
    // shell: ExecInShell → rispondi come farebbe la host.
    let exec = recv_until(&mut source, |m| matches!(m, ServerMsg::ExecInShell { .. })).await;
    let exec_id = match exec {
        ServerMsg::ExecInShell { turn_id, exec_id, command, capture } => {
            assert_eq!((turn_id.as_str(), command.as_str(), capture), ("c1", "echo x", true));
            exec_id
        }
        _ => unreachable!(),
    };
    send(&mut sink, &ClientMsg::ExecResult { turn_id: "c1".into(), exec_id, exit_code: 0, output: "x\n".into(), cwd: "C:\\dopo".into() }).await;
    // shell: riga di conferma + Done (nessun Chunk di testo AI).
    let ack = recv(&mut source).await;
    assert!(matches!(&ack, ServerMsg::Chunk { content, .. } if content == "\u{2192} finestra \"stampa x\" aperta"), "{ack:?}");
    assert!(matches!(recv(&mut source).await, ServerMsg::Done { .. }));
    // ui: il contenuto arriva una volta, con l'output del comando.
    let content = recv_until(&mut ui_source, |m| matches!(m, ServerMsg::OutputWindowContent { .. })).await;
    assert!(matches!(&content, ServerMsg::OutputWindowContent { window_id, markdown } if window_id == "c1" && markdown.contains("output: x")), "{content:?}");
}

#[tokio::test]
async fn shell_ping_reports_ui_and_missing_plugin() {
    let url = spawn_server("tok-shell-5").await;
    let (ui_ws, _) = connect_async(&url).await.unwrap();
    let (mut ui_sink, mut ui_source) = ui_ws.split();
    send(&mut ui_sink, &ui_hello("tok-shell-5")).await;
    recv(&mut ui_source).await;
    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut sink, mut source) = ws.split();
    send(&mut sink, &shell_hello("tok-shell-5")).await;
    recv(&mut source).await;
    send(&mut sink, &command("c1", "/ping")).await;
    let ping = recv_until(&mut ui_source, |m| matches!(m, ServerMsg::UiPing { .. })).await;
    if let ServerMsg::UiPing { id } = ping {
        send(&mut ui_sink, &ClientMsg::UiPong { id, version: "9.9.9".into() }).await;
    }
    let content = recv_until(&mut ui_source, |m| matches!(m, ServerMsg::OutputWindowContent { .. })).await;
    let md = match content { ServerMsg::OutputWindowContent { markdown, .. } => markdown, _ => unreachable!() };
    assert!(md.contains("| lare-shell | 2.0.0 | ok | sessione s1"), "{md}");
    assert!(md.contains("| ui.exe | 9.9.9 | ok |"), "{md}");
    assert!(md.contains("| plugin-ping | -- | non trovato |"), "{md}");
    assert!(matches!(recv_until(&mut source, |m| matches!(m, ServerMsg::Done { .. })).await, ServerMsg::Done { exit_code: Some(0), .. }));
}
```

Run: `cargo test -p orchestrator --test ws_integration 2>&1 | grep "test result"` → `22 passed`
(17 + 5). Se uno scenario fallisce, il difetto è nel wiring del Task 8 (il piano prevede il fix
lì, non un allentamento dell'asserzione).

- [ ] **Step 2: client di sviluppo**

`scripts/dev/shell-client.mjs` (Node ≥ 22: `WebSocket` globale; nessuna dipendenza):

```js
#!/usr/bin/env node
// shell-client.mjs — Client di SVILUPPO del canale shell (piano 2a).
//
// Fa ciò che farà lare-shell (piano 2b), in piccolo: Hello{role:"shell"},
// UN Command, e poi risponde ai messaggi dell'orchestratore come una host:
//   tool_confirm_request → chiede [Y/n] su stdin
//   exec_in_shell        → esegue con pwsh (capture:true cattura, false no) e manda exec_result
//   chunk/done/error     → stampa; done/error chiudono
// Uso:
//   node scripts/dev/shell-client.mjs [--config-dir "Test Run/Configuration"] [--session s1] -- '/ai "elenca i file"'
// Legge token e ws_port dalla cartella di configurazione (mai variabili d'ambiente, D6).

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { createInterface } from "node:readline";

const argv = process.argv.slice(2);
function flag(name, def) { const i = argv.indexOf(name); return i >= 0 ? argv[i + 1] : def; }
const configDir = flag("--config-dir", join("Test Run", "Configuration"));
const sessionId = flag("--session", `dev-${process.pid}`);
const sep = argv.indexOf("--");
const input = (sep >= 0 ? argv.slice(sep + 1) : argv.filter((a, i) => !a.startsWith("--") && argv[i - 1] !== "--config-dir" && argv[i - 1] !== "--session")).join(" ");
if (!input) { console.error("manca il comando, es.: -- '/ping'"); process.exit(2); }

const token = readFileSync(join(configDir, "token"), "utf8").trim();
const port = JSON.parse(readFileSync(join(configDir, "startup.json"), "utf8")).ws_port ?? 7331;
const rl = createInterface({ input: process.stdin, output: process.stdout });
const ask = (q) => new Promise((res) => rl.question(q, res));

const ws = new WebSocket(`ws://127.0.0.1:${port}`);
const sendJson = (o) => ws.send(JSON.stringify(o));
const CWD_MARK = "__LARE_CWD__";

function execInShell(command, capture) {
  // La cwd è quella del processo: ogni comando parte da lì (una host vera tiene il runspace).
  const script = `${command}\nWrite-Output ('${CWD_MARK}' + (Get-Location).Path)`;
  const r = spawnSync("pwsh", ["-NoProfile", "-NonInteractive", "-Command", script], {
    cwd: process.cwd(), encoding: "utf8", stdio: capture ? ["ignore", "pipe", "pipe"] : ["inherit", "inherit", "inherit"],
  });
  let output = "", cwd = process.cwd();
  if (capture) {
    const lines = ((r.stdout ?? "") + (r.stderr ?? "")).split(/\r?\n/);
    const mark = lines.findLast((l) => l.startsWith(CWD_MARK));
    if (mark) cwd = mark.slice(CWD_MARK.length);
    output = lines.filter((l) => !l.startsWith(CWD_MARK)).join("\n");
    process.stdout.write(output.endsWith("\n") ? output : output + "\n");
  }
  return { exit_code: r.status ?? -1, output, cwd };
}

ws.addEventListener("open", () => {
  sendJson({ type: "hello", token, role: "shell", session_id: sessionId, cwd: process.cwd(), version: "dev-client" });
});
ws.addEventListener("message", async (ev) => {
  const msg = JSON.parse(ev.data);
  switch (msg.type) {
    case "server_info":
      sendJson({ type: "command", id: `cmd-${Date.now()}`, input, input_mode: "keyboard", command_type: "auto", cwd: process.cwd(), web_search: false });
      break;
    case "tool_confirm_request": {
      const a = (await ask(`\n${msg.commands}\nEseguire? [Y/n] `)).trim().toLowerCase();
      sendJson({ type: "tool_confirm_response", id: msg.id, accept: a === "" || a === "y" || a === "s" });
      break;
    }
    case "exec_in_shell": {
      const r = execInShell(msg.command, msg.capture);
      sendJson({ type: "exec_result", turn_id: msg.turn_id, exec_id: msg.exec_id, ...r });
      break;
    }
    case "chunk": console.log(msg.content); break;
    case "done": console.log(`[done exit_code=${msg.exit_code}]`); ws.close(); rl.close(); break;
    case "error": console.error(`[error ${msg.code}] ${msg.message}`); ws.close(); rl.close(); process.exitCode = 1; break;
    default: console.log(`[${msg.type}]`, JSON.stringify(msg));
  }
});
ws.addEventListener("error", (e) => { console.error("ws error:", e.message ?? e); process.exit(1); });
ws.addEventListener("close", () => process.exit());
```

Verifica (controller, orchestratore e `ui.exe` avviati come al Task 9 Step 5):

```powershell
node scripts/dev/shell-client.mjs -- '/ping'          # finestra su ui.exe con la tabella; qui "→ finestra … aperta"
node scripts/dev/shell-client.mjs -- '/nonesiste'     # solo "[done exit_code=0]"
node scripts/dev/shell-client.mjs -- '/ai ciao'       # [error routing_error] sintassi: …
node scripts/dev/shell-client.mjs -- '/config'        # si apre /config su ui.exe
node scripts/dev/shell-client.mjs -- '/ai "elenca i 3 file più grandi qui"'   # [Y/n] → esegue → finestra
```

(l'ultimo richiede `llms.json` con una chiave valida; senza, lo `StubAdapter` risponde e la
finestra mostra `[stub AI] ricevuto: …`).

- [ ] **Step 3: commit**

```bash
git add scripts/dev/shell-client.mjs crates/orchestrator/tests/ws_integration.rs
git commit -m "test(orchestrator): scenari e2e del canale shell + client di sviluppo shell-client.mjs"
```

---

### Task 11: documentazione, ADR-018, versioni, release

**Files:**
- Modify: `crates/protocol/Cargo.toml` (2.1.0), `crates/orchestrator/Cargo.toml` (2.1.0),
  `crates/ui/src-tauri/Cargo.toml` + `crates/ui/src-tauri/tauri.conf.json` (2.1.0); `Cargo.lock` segue
- Modify: `crates/{protocol,orchestrator,ui}/CHANGELOG.md` e `IMPLEMENTATION.md`
- Modify: `Docs/i18n/ita/06-decisions.md` (ADR-018), `HANDOFF.md`, `RUN-LOCAL.md`, `TESTING-e2e.md`, `KNOWN-ISSUES.md`
- Modify: `CLAUDE.md` (riga "Flag di sviluppo": aggiungere il client dev), `README.md` root se cita lo stato

- [ ] **Step 1: versioni e changelog**

Bump a `2.1.0` nei tre `Cargo.toml` e in `tauri.conf.json` (`"version": "2.1.0"`);
`cargo build` (aggiorna `Cargo.lock`) e `cargo build -p ui`. `CHANGELOG.md` di ciascuno: voce
`2.1.0 — <data> — piano 2a` con le modifiche dei task (protocol: Task 1; orchestrator: Task 2-8, 10;
ui: Task 9). `IMPLEMENTATION.md`: orchestrator — sezione "Canale shell" (registro, `shell_session`,
`surface`, `shell_slash`, `shell_turn`, `ping`, cosa NON cambia per le connessioni `ui`);
ui — "Finestra di output e `open_ui_local`"; protocol — "Superficie".

- [ ] **Step 2: ADR-018** (`06-decisions.md`, dopo ADR-017)

```markdown
## ADR-018 — Canale shell: registro delle connessioni, router di superficie, output a `Done` (2026-09-05)

**Contesto.** Con la host C# (ADR-015) l'orchestratore riceve comandi da sessioni shell che non
hanno una superficie di rendering: l'output va nelle finestre di `ui.exe` (D14), i comandi
proposti dall'AI girano nella shell del client (modello B), e `ui.exe` è uno per macchina.

**Decisione.** (1) Ogni connessione dichiara un `role` (`ui`|`shell`, default `ui`); un registro
in-process (`connections.rs`) tiene il sink `ui` (la connessione `ui` senza canale, l'ultima
vince) e le sessioni shell. (2) Per un turno originato da una shell, `ServerMsg::surface()`
decide la destinazione di ogni messaggio (`Origin` = la shell, `Ui` = il sink); i `Chunk` sono
bufferizzati e consegnati una volta a `Done`/`Error` (`OutputWindowContent`), la shell riceve
una sola riga di conferma. (3) `run_in_session` sul canale shell è un round-trip
`ExecInShell`→`ExecResult` sulla stessa connessione, gateizzato SEMPRE (`ShellConfirmer`);
la cwd è per sessione. (4) Slash ignoto dalla shell → `Done` muto + log; `/ai "x"` ≡ `/ "x"`.

**Conseguenze.** Le connessioni `ui`/Telegram non cambiano (nessuna regressione v1). Senza
`ui.exe` connesso un turno shell completa comunque (riga di avviso al posto della conferma);
l'autostart è del piano 3. Lo streaming nella finestra resta fuori MVP (§12). `/find` e `/nowin`
dalla shell sono scartati in questa versione (debito, HANDOFF).
```

- [ ] **Step 3: HANDOFF, RUN-LOCAL, TESTING-e2e, KNOWN-ISSUES, CLAUDE.md**

`HANDOFF.md`: "Versioni correnti" (protocol/orchestrator/ui 2.1.0); in FATTO una sezione
**Piano 2a** con una riga per task (numero, cosa, commit); in DA FARE: **Piano 2b** (host C#
`lare-shell`, modalità B; consuma il protocollo 2.1; da scrivere con `writing-plans` partendo da
spec §4.3-§4.5, §6.4, §7, §10 e dal client dev come riferimento del giro di messaggi), poi Piano 3;
**Debiti del piano 2a**: `/find` e `/nowin` dalla shell (discard), streaming nella finestra,
`ActivityIndicator` emesso ma non consumato (piano 3), test end-to-end del gate con AI reale (solo
`--ignored`), `emitToPlugin` usato come emit generico in `host.js`.

`RUN-LOCAL.md`: sezione "Canale shell senza la host (client di sviluppo)" con i cinque comandi del
Task 10 Step 2. `TESTING-e2e.md`: "Parte 5 — Canale shell col client di sviluppo" (tabella: comando,
atteso nel terminale, atteso su `ui.exe`). `KNOWN-ISSUES.md`: "Markdown della finestra di output:
i chunk di trasparenza e il testo AI sono concatenati senza separatore (cosmetico, MVP)".
`CLAUDE.md`: nel blocco "Comandi" aggiungere
`node scripts/dev/shell-client.mjs -- '/ping'   # canale shell senza la host (piano 2a)`.

- [ ] **Step 4: verifica finale (controller)**

```bash
cargo test 2>&1 | grep -E "test result|FAILED" | awk '/test result/{s+=$4} /FAILED/{f++} END {print "pass", s, "fail", f+0}'
cargo test -p ui 2>&1 | grep "test result"
node --test crates/ui/frontend/*.test.mjs 2>&1 | grep -E "^# (pass|fail)"
cargo clippy --all-targets 2>&1 | grep -c "^warning"      # non superiore a main
git status --short                                         # solo i file previsti
```

- [ ] **Step 5: commit di release** (hook `commit-msg`: `HANDOFF.md` deve essere staged)

```bash
git add Cargo.lock crates/protocol crates/orchestrator crates/ui Docs/i18n/ita CLAUDE.md README.md
git commit -m "release: piano 2a completato — canale shell nel protocollo e nell'orchestratore (protocol/orchestrator/ui 2.1.0)"
```

---

## Self-review del piano (eseguita alla scrittura)

- **Copertura spec.** §3 tabella: `/ai`+`/` (T6/T8), senza virgolette (T6), `/open /web` (T6 `Backend`
  → T8), `/help` singleton (T9), `/config /library` (T6/T9), `/calc` (ramo plugin v1 invariato,
  finestra sul sink ui via `set_server_tx` — T8), canali esterni (T6 tabella + T9), `/ping` (T7/T8),
  `/reset` (T6/T8), ignoto (T6/T8), riga senza `/` (T6 `Discard`). §3.1 (T7). §3.2 (T5/T9). §4.1
  messaggi (T1) — `ActivityIndicator` emesso (T5) e consumato nel piano 3. §4.2 sequenza (T10
  scenario 4). §4.3 gate (T3). §4.5 `capture` (T4). §4.6 cwd per sessione (T4/T8). §5 singleton
  (T9). §6.4 solo l'avviso (T5). §8 (T3, global constraint). §10 Rust (T1-T8), JS (T9). §4.4/§4.7
  e §7 sono del piano 2b.
- **Placeholder.** Nessun "TBD"/"simile al task N"; ogni step ha comando e atteso.
- **Coerenza dei nomi.** `ShellSessionState::{new, session_id, cwd, set_cwd, exec, resolve_exec,
  abort_turn, abort_all}` (T4) usati in T8; `ShellSessionToolClient::for_turn` (T4) in T8;
  `route_shell_turn`/`ShellTurn`/`output_window_title`/`NO_UI_ACK` (T5) in T8; `classify_shell_input`
  /`read_web_search_enabled`/`RESET_MESSAGE`/`AI_SYNTAX_ERROR` (T6) in T8/T10; `PluginHost::find`,
  `probe`, `run_ping`, `UI_PING_TIMEOUT` (T7) in T8; `Registry::{shared, set_ui_sink, clear_ui_sink_if,
  ui_sink, register_shell, unregister_shell, register_ui_ping, resolve_ui_ping}` (T2) in T8; `HelloInfo`
  (T8) in T8; `ShellConfirmer::new` (T3) in T8; `ExecReply` (T4) in T8.
- **Conteggi test** (attesi, da riverificare): protocol +5, orchestrator +39 unit (T2 4, T3 4, T4 6,
  T5 5, T6 7, T7 6, T8 7) + 5 integrazione, JS +5.
