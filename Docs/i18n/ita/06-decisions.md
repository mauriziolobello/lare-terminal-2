# 06 — Decisioni (ADR)

Log delle decisioni architetturali. Per ognuna: **contesto**, **opzioni**, **decisione**, **perché** (trade-off, non solo il verdetto), **conseguenze**. Questo è il documento che cattura il *ragionamento* — la cosa più facile da perdere.

> **Nota sulla continuità con la v1.** ADR-001..014 sono copiate intatte dal log della v1
> (`C:\Users\Maurizio\Documents\Progetti\Lare Terminal\Docs\06-decisions.md`), compresi i loro
> riferimenti interni ad altri documenti v1 (`07-ux-and-config.md`, `08-persistent-shell.md`,
> `09-open-app.md`, `10-custom-windows.md`, `Docs/superpowers/specs/2026-07-15-…`): quei file
> vivono SOLO nel repo v1 (sola lettura, `..\Lare Terminal\Docs\`), non sotto questo repo. La
> numerazione continua da ADR-015 con le decisioni proprie della 2.0.

---

## ADR-001 — Ruolo dell'MCP: dove vive il "cervello AI"

**Contesto.** "Server MCP nel backend per permettere a qualunque AI di interfacciarsi" è ambiguo. MCP ha una direzionalità: un host AI (client) chiama i tool esposti da un server.

**Opzioni.**
- A) Backend = server di tool puro; l'AI è un host esterno.
- B) Backend = cervello con AI adapter pluggable; usa MCP internamente.

**Decisione.** Architettura a **3 strati** che prende il meglio di entrambe: server MCP (tool riusabili) + orchestratore con AI adapter (AI sostituibile) + canali (UI/Telegram/voce).

**Perché.** Il malinteso chiave: **MCP standardizza i tool, non l'AI**. "Qualunque AI" contiene due desideri distinti che richiedono due meccanismi:
- "le capacità riusabili da AI diverse / altri tool" → **server MCP**
- "poter cambiare il cervello (Claude→GPT→locale)" → **AI adapter**
Inoltre un server MCP puro non può gestire una conversazione in streaming verso un cursore: serve comunque un orchestratore *davanti*.

**Conseguenze.** Lo "step 1 = interfaccia di comunicazione" sono **due contratti**: UI↔Orchestratore e Orchestratore↔Server MCP.

---

## ADR-002 — Stack: Rust/Tauri vs Electron vs nativo (C/Swift)

**Contesto.** Serve overlay trasparente + hotkey + cursore animato, con gemellaggio Win/macOS e attenzione alla performance.

**Opzioni.** TypeScript+Electron · C#/.NET+Avalonia · Rust+Tauri · C(Win)+Swift(macOS).

**Decisione.** **Rust + Tauri** (UI), backend interamente in Rust.

**Perché.**
- Chiarito il malinteso "interpretato = lento": Electron JIT-compila (calcolo ok). Il problema di Electron è il **footprint** (Chromium ~150MB, RAM alta), non la velocità.
- **Nessuna scelta di linguaggio rende le risposte AI più veloci** — la latenza percepita è rete/modello + strategia "tieni l'app calda". Il linguaggio incide su footprint, cold-start, snappiness dell'overlay.
- C(Win)+Swift(macOS) sarebbe la via più "nativa" ma **tradisce il gemellaggio**: due codebase, divergenza continua. Una codebase cross-platform garantisce il gemellaggio per costruzione.
- Tauri: core compilato nativo, footprint minimo, cursore fancy comunque in HTML/CSS.

**Trade-off accettato.** Curva di apprendimento di Rust (l'utente non l'ha mai usato; procede con supervisione). In Rust alcuni pezzi AI si scrivono a mano (niente SDK Anthropic ufficiale) — ma è solo HTTPS+JSON.

---

## ADR-003 — Voce: Whisper, non Web Speech né API native OS

**Contesto.** La voce è una fase futura, ma la scelta della UI non deve incastrarci dopo.

**Falso dilemma individuato.** Sembrava "voce garantita (Electron/Web Speech) **oppure** leggerezza (Tauri)". È falso.

**Perché si scioglie.**
- La Web Speech API gemella male: su macOS WKWebView è limitata/assente; in Electron ha quirk storici.
- Le API vocali native dell'OS (Windows Speech vs Apple Speech) sono **motori diversi** → non gemellano.
- **Whisper** (whisper.cpp) è un **motore unico**, identico su Win/macOS, offline, più accurato, **indipendente dalla webview**.

**Decisione.** La voce sarà un **sidecar Whisper**. Conseguenza: Tauri **non** incastra sulla voce → l'unico vantaggio di Electron (Web Speech coerente) evapora, e resta solo il suo peso. Conferma indiretta di ADR-002.

---

## ADR-004 — Topologia: orchestratore come daemon separato (non fuso nella UI)

**Contesto.** In Tauri l'orchestratore potrebbe vivere dentro il core (2 processi). Oppure essere un daemon autonomo (3 processi).

**Decisione.** **Daemon separato.**

**Perché.** Telegram e voce sono strumenti di comunicazione **indipendenti dalla UI**: se la UI si chiude, devono continuare a funzionare. Bonus: il contratto dell'orchestratore diventa esplicito e testabile in isolamento (`wscat`/`curl`/script).

**Costo accettato (~mezza–una giornata in più).** Concentrato in: autostart/ciclo di vita (unico pezzo che non gemella — Run key vs LaunchAgent), handshake di sicurezza, discovery/istanza singola. **Si parte minimali** (processo in background lanciato da terminale) e si indurisce dopo (Fase 5).

---

## ADR-005 — Default AI: Claude Opus 4.8 via Messages API HTTPS

**Contesto.** Quale modello e come chiamarlo da Rust.

**Decisione.** Default **`claude-opus-4-8`** (Opus 4.8), via **Messages API su HTTPS** con `reqwest` + streaming SSE; `thinking: {type:"adaptive"}`. Dietro l'interfaccia `AIAdapter` (sostituibile).

**Perché.** Opus 4.8 è il default raccomandato per capacità. Non esiste un SDK Anthropic ufficiale per Rust → chiamata HTTP diretta (semplice, solo JSON; gemella per costruzione). L'adapter mantiene la sostituibilità (GPT/locale = altra impl del trait), senza toccare il resto.

---

## ADR-006 — Convenzioni: TDD, SOLID, CHANGELOG/IMPLEMENTATION, repo privato, agente Sonnet

**Decisioni (richieste utente).**
- **TDD non derogabile.** Regola di ferro: *nessun codice di produzione senza prima un test che fallisce*. Ciclo RED → GREEN → REFACTOR, con verifica obbligatoria che il test fallisca per il motivo giusto prima di implementare. Vale anche per i bugfix.
- **SOLID non derogabile** (deroghe solo per necessità immediate, da sanare). In Rust: SRP=moduli/crate focalizzati; OCP/DIP/ISP=`trait` come confini; + composizione (coerente con "composizione, non ereditarietà").
- **`CHANGELOG.md` per ogni sottoprogetto** con semver `major.minor.update`.
- **`IMPLEMENTATION.md` per ogni sottoprogetto** con i dettagli implementativi correnti.
- **Repository GitHub privato** fin da subito.
- **Workflow:** il codice lo costruisce un **agente Sonnet** (`lare-builder`, `claude-sonnet-4-6`); supervisione (task, review, aderenza TDD/SOLID/contratti, test, doc).

**Perché.** Qualità e tracciabilità fin dal giorno 1; separazione netta tra costruzione (Sonnet) e revisione (supervisore). Bonus didattico del TDD: il test scritto per primo è la **specifica leggibile** del comportamento — leggerlo prima dell'implementazione accelera l'apprendimento della sintassi Rust.

---

## ADR-007 — Sicurezza dei canali: esecuzione comandi e 2FA su Telegram — **implementato (canale Telegram, v0.11.0)**

**Contesto.** Una revisione di sicurezza automatica ha segnalato (giustamente) che `run_os_command` esegue comandi OS arbitrari (CRITICAL) e che `cwd` non validato consentiva un leak NTLM via path UNC su Windows (HIGH).

**Decisioni.**
- **Esecuzione comandi = funzionalità voluta.** Eseguire comandi OS è lo scopo del prodotto, non un difetto. Mitigazioni: allowlist + conferma per comandi pericolosi (TODO Fase 2), WS solo `127.0.0.1` + token.
- **`cwd` hardening (fatto, Fase 1):** `validate_cwd` rifiuta path UNC/device/verbatim (prima di toccare il filesystem), non assoluti, inesistenti → blocca il vettore NTLM. (mcp-server v0.1.1)
- **Telegram = input remoto non fidato → richiede 2FA.** Il canale Telegram ha una **procedura di riconoscimento 2FA TOTP** (RFC 6238, Google Authenticator) + **pairing monouso** via codice, **indipendente** dal semplice avere accesso al chatbot. Implementato in v0.11.0.

**Gate implementato (v0.11.0).** Prima di ogni comando OS o slash `/open`/`/web`, il canale Telegram:
1. Verifica la sessione TOTP attiva (`/login` richiede il codice TOTP ogni 30 minuti).
2. Se il comando richiede conferma (`needs_confirmation`): invia bottoni inline (`OK` / `Annulla`) e attende la callback — il comando viene eseguito **solo se l'utente clicca OK** entro 2 minuti.
3. Rate-limit: 5 fallimenti consecutivi (pairing o login) → lockout 1 minuto.

**Gate in-loop sui tool dell'AI (v0.12.0).** Estensione che chiude il buco residuo: un comando NL che induce l'AI a usare `run_in_session`/`open_target` ora **non** è più eseguito direttamente. `AiAdapter::respond` accetta `confirmer: Option<&dyn ToolConfirmer>`; su Telegram è `Some(TelegramConfirmer)` → ogni tool proposto dall'AI chiede `[Esegui]/[Annulla]` (id opaco, sessione ri-verificata prima e al tap). In UI locale resta `None` (autonomo, invariato).

**Perché.** Il vettore davvero pericoloso non è la macchina locale (WS localhost+token) ma il **canale remoto**: chiunque conosca il bot potrebbe inviare comandi. Il 2FA + gate chiudono quel vettore alla radice; il gate in-loop (v0.12.0) lo chiude anche quando è l'AI — non l'utente — a decidere di eseguire un comando.

**Conseguenze.** Gate 2FA completo per OS + `/open`/`/web` via Telegram **e** per ogni `run_in_session`/`open_target` proposto dall'AI dentro il loop NL (v0.12.0). Gli slash non-gated (`/show`, `/reset`, ecc.) non richiedono conferma esplicita — il login TOTP è già la barriera di sessione. La UI locale resta autonoma (nessun confirmer).

**Estensione per-tool (2026-07-15).** Il gate era per-canale (`Some`/`None` decide
tutto il canale). `ToolConfirmer::should_gate(tool_name)` (default: gate su tutto
tranne `show_markdown`, preserva Telegram) sposta la granularità al singolo tool:
un `LocalUiConfirmer` gateizza SOLO un elenco esplicito `SENSITIVE_TOOLS` (vuoto
all'introduzione), lasciando `run_in_session`/`open_target` autonomi in locale
come sempre. Motivazione: i futuri tool esterni (server MCP dedicati, es. nmap)
possono avere effetti sensibili (rete, non solo la macchina locale) che
giustificano una conferma anche in locale, senza rendere l'uso quotidiano della
UI locale meno fluido. Design:
`Docs/superpowers/specs/2026-07-15-local-tool-confirm-gate-design.md`.

---

## ADR-008 — Hotkey di attivazione configurabile

**Contesto.** L'hotkey di richiamo (default `F2`) non deve essere fissa.

**Decisione.** L'hotkey è un **parametro di configurazione fra i primi**: qualunque tasto funzione o combinazione (`Ctrl+Alt+T`, `Opt+T`, …). Dettagli e implicazioni in `07-ux-and-config.md`.

**Perché.** Preferenze utente e conflitti con altre app/OS. `tauri-plugin-global-shortcut` supporta già accelerator arbitrari, quindi il costo è esporre+persistere+ri-registrare, non implementare da zero. **Conseguenza:** prevedere uno strato di config fin da subito (non hard-codare `F2`).

---

## ADR-009 — Linux come target ufficiale (progetto tri-platform)

**Contesto.** Oltre a Windows (iniziale) e macOS, valutare Linux come target.

**Decisione.** Linux è **target ufficiale**: progetto **tri-platform** (Windows/macOS/Linux).

**Perché.** Costo quasi nullo ora: lo stack è già cross-platform e il ramo `#[cfg(unix)]` di `mcp-server` copre Linux oltre a macOS. Decidere adesso evita che il codice maturi assumendo solo due piattaforme.

**Caveat noto (rischio aperto).** Su **Wayland** il modello di sicurezza restringe hotkey globali e focus-steal (un'app non può rubarli a piacimento); su **X11** è permissivo come Windows. → Servirà uno **spike dedicato hotkey/focus su Wayland** (eventuale via portal/compositor) quando avremo una macchina Linux, analogo a quello fatto su Windows. Equivalenti di sistema in `ENVIRONMENT.md` (webkit2gtk, autostart XDG/systemd).

**Conseguenze.** Documenti resi tri-platform; Fase 6 estesa a macOS **e** Linux con lo spike Wayland.

---

## ADR-010 — Comandi slash (input "meta" dal cursore)

**Contesto.** Servono funzioni "meta" (configurazione e, in futuro, altro) accessibili dallo stesso cursore, senza menu.

**Decisione.** Input con prefisso **`/`** = **comando slash**, una **terza categoria di input** accanto a OS e NL. Primo comando: **`/config`** (apre una dialog). Vedi `07-ux-and-config.md` per la dialog e i parametri.

**Gestione (Fase 1).** I comandi slash sono **gestiti dalla UI** (`/config` apre una finestra locale di configurazione, che persiste i settaggi nel file di config). In futuro alcuni comandi slash potranno essere instradati al backend; il prefisso `/` resta il discriminatore. Il `router` dell'orchestratore potrà guadagnare una `Route::Slash`, ma per ora non è necessario (la UI intercetta prima dell'invio).

**Perché.** Pattern familiare (come le slash dell'AI), discoverable, e non sporca il routing OS/NL: `/` è un discriminatore netto come `$` per l'OS.

---

## ADR-011 — Sessione shell persistente (sostituisce l'esecuzione one-shot)

**Contesto.** L'esecuzione era one-shot (`cmd /C …` fresca a ogni comando) → `cd`/env non persistono. L'utente vuole che "il cursore SIA una sessione di terminale".

**Decisione.** Sostituire `run_os_command` con una **shell persistente** nel `mcp-server`: `run_in_session`. Default **PowerShell** (Windows) / **sh** (Unix). Approccio **A**: stdin/stdout in pipe + **marker** per confini netti ed exit code (NON un PTY → niente colori/programmi interattivi per ora). Spec completa: `08-persistent-shell.md`.

**Perché.** È il modello di un vero terminale (stato persistente) e copre l'esigenza confermata. Il PTY/emulatore completo (vim, colori) è un upgrade futuro, non necessario ora (YAGNI). Il **protocollo WS resta invariato** (Chunk/Done) — cambia solo dentro `mcp-server`, grazie all'isolamento delle capacità nel server MCP.

**Conseguenze.** `cwd` non è più un parametro per-comando (è stato di sessione) → il guard anti-UNC del parametro non si applica a questo percorso. Slash `/reset` per riavviare la sessione. Prima superficie di una serie ("superfici di risposta": apri-app-nativa e finestre-custom seguiranno).

---

## ADR-012 — Superficie 2: `/open` (apri app nativa) + slash UI-local vs backend

**Contesto.** Seconda superficie di risposta: aprire l'app nativa giusta (cartella→Explorer, URL→browser, file→app). L'AI che sceglie da sola è Fase 2.

**Decisioni.**
- **Capacità:** tool MCP `open_target` nel `mcp-server`, via crate `opener` (cross-platform, no shell-string). Spec: `09-open-app.md`.
- **Trigger:** comando esplicito **`/open <target>`** (no euristica → niente ambiguità con `cd`/comandi finché non c'è l'AI).
- **Routing slash (refinement ADR-010):** gli slash si dividono in **UI-local** (`/config` → dialog) e **backend** (tutti gli altri → inoltrati all'orchestratore, route `Slash`, dispatch `/open`/`/reset`/sconosciuto). Protocollo WS invariato.

**Perché.** Valore immediato senza l'AI; la stessa capacità sarà usata dall'AI in Fase 2. Bonus: chiude il TODO di `/reset` (Slice B) — diventa un backend-slash funzionante insieme a `/open`.

---

## ADR-013 — Superficie 3: finestre custom (Markdown)

**Contesto.** Terza superficie: una finestra ricca dove versare risposte formattate (spiegazioni, codice, tabelle) — la "carne" per l'intento apprendimento dell'AI.

**Decisioni.**
- **Capacità = UI** (Tauri possiede le finestre), non `mcp-server`. Quindi **il protocollo si estende** (prima volta): `ServerMsg::OpenWindow { title, kind, content }`, `WindowKind::Markdown`. Additivo. Spec: `10-custom-windows.md`.
- **Flusso:** orchestratore emette `OpenWindow` → la UI crea una nuova `WebviewWindow` e renderizza il Markdown.
- **Trigger:** backend slash **`/show <markdown>`** (esplicito ora; l'AI emette lo stesso messaggio in Fase 2).
- **Sicurezza:** Markdown→HTML **sanificato** (vendor `marked`+`DOMPurify` pinnati, locali; no CDN; no innerHTML grezzo). In alternativa renderer minimale DOM-based.

**Perché.** Costruire il *canale di output ricco* prima del cervello (AI) che lo riempirà. Estensione del contratto additiva → nessuna rottura.

---

## ADR-014 — Connettività AI: provider diretto E proxy (OpenRouter) — [PIANIFICATO, non implementato]

**Contesto.** Estensione di ADR-005 (AI adapter pluggable). Oltre a collegarsi **direttamente** a un provider (es. Anthropic), il progetto deve poter usare **proxy/aggregatori** come **OpenRouter** (API OpenAI-compatibile che instrada verso molti modelli).

**Decisione (da implementare in Fase 2).** L'`AIAdapter` avrà (almeno) due famiglie di implementazioni:
- **Diretta:** Anthropic Messages API (default `claude-opus-4-8`), già previsto da ADR-005.
- **Via proxy:** OpenRouter / endpoint OpenAI-compatibile (base URL + API key configurabili, modello scelto dall'utente).
Selezione e credenziali via configurazione (vedi `07-ux-and-config.md`; segreti fuori dal repo).

**Vincolo non negoziabile — compatibilità MCP.** Qualunque AI/proxy scelto deve supportare il **tool/function calling**, perché è l'orchestratore (client MCP) a guidare i tool del `mcp-server` tramite il loop di tool-use dell'AI. Un modello/proxy senza function calling **non** è ammissibile (romperebbe l'accesso ai tool). L'astrazione `AIAdapter` deve quindi esporre il tool-use in modo uniforme, mappando i tool MCP sul formato del provider/proxy.

**Perché.** Flessibilità e costo (OpenRouter dà accesso a molti modelli con una sola integrazione) senza sacrificare l'architettura: l'MCP resta il bus delle capacità, l'adapter normalizza il tool-use. **Per ora solo registrato**: nessun codice finché non si affronta la Fase 2.

---

## ADR-015 — Lato shell: host custom del motore PowerShell in C# (2026-09-04)

Contesto: la 2.0 vuole i comandi `/…` nella riga di comando di una sessione PowerShell e i comandi
dell'AI eseguiti nella shell dell'utente. Decisione: `lare-shell` è una host custom del motore
PowerShell (come `pwsh.exe`), non un hook su una shell altrui. Provata dallo spike
`spikes/2026-09-05-lare-shell-host.md`. Conseguenze: componente C#; Linux/macOS pwsh-flavored.

## ADR-016 — Lare Terminal è una finestra Tauri con xterm.js + ConPTY (2026-09-05)

Decisione: la host gira dentro una finestra Tauri con emulatore xterm.js; barre e segnalini in HTML.
Provata dallo spike `spikes/2026-09-05-lare-terminal-window.md`. L'overlay F2 della v1 muore;
`ui.exe` resta host di finestre con una pagina host nascosta al posto del cursore.

## ADR-017 — Configurazione: una cartella, nessuna variabile d'ambiente (2026-09-04)

Decisione: `--config-dir` o `<exe>\Configuration\`; percorsi relativi alla radice del deploy;
i figli ricevono `--config-dir` per argomento. Motivo: nella v1 tre lettori indipendenti della
stessa env var divergevano in silenzio (`llms_config.rs:119-125` v1).

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
