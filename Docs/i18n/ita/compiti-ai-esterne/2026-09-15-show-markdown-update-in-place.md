# Compito per AI esterna — `show_markdown` in-place, gate di chiusura con conferma, badge di progresso

> **Prima cosa**: leggi per intero `Docs/i18n/ita/BRIEFING-AI-ESTERNE.md` alla radice del
> repository, poi questo file per intero, PRIMA di scrivere codice. **Crea la tua worktree
> dedicata come primissima cosa** (`git worktree add .worktrees/md-update-in-place -b
> feat/md-update-in-place`), come impone BRIEFING-AI-ESTERNE.md §1/§6.

## Contesto — il bug, vissuto due volte dal vivo

`show_markdown` (`crates/orchestrator/src/agent.rs`) è uno dei 5 tool storici che l'AI ha in ogni
turno `/ai "…"` con `allow_windows: true`. Oggi (`crates/orchestrator/src/ai_adapter.rs`, branch
`if name == "show_markdown"`) OGNI chiamata apre una finestra Markdown NUOVA
(`ServerMsg::OpenWindow`, mai un aggiornamento). Trovato la prima volta l'8/9 (commit `6e0dc85`,
mitigato SOLO con un'istruzione nel prompt del tool: "chiamalo una sola volta, a fine turno") e
rivissuto dal vivo il 12/9 da Maurizio: un turno `/ai` con ricerca web ha aperto la STESSA
risposta (quasi identica) in 4 finestre separate nell'arco di alcuni minuti, mentre il modello
continuava a raffinarla — lui chiudeva ogni finestra e gliene si riapriva un'altra subito dopo, "al
buio" (nessun segno che la ricerca continuasse oltre il cursore sospeso nel terminale).

Tre pezzi, in quest'ordine — ognuno testabile e commit-abile da solo, ma B dipende dai messaggi
introdotti in A e C dipende dagli eventi introdotti in B:

- **Parte A**: `show_markdown` aggiorna la STESSA finestra invece di aprirne una nuova (il bug
  vero e proprio). Include un fix a un meccanismo esistente che, così com'è oggi, impedirebbe
  proprio questo (vedi §A.1).
- **Parte B**: se l'utente prova a chiudere la finestra MENTRE il turno è ancora attivo, un
  modale chiede conferma; se conferma, il turno viene DAVVERO annullato (non continua in
  background) e la finestra si chiude senza più riaprirsi.
- **Parte C**: badge "ricerca in corso…"/"completato" + orario ultimo aggiornamento, così
  l'utente non è mai "al buio" sullo stato del turno.

Decisioni prese con Maurizio, non rimesse in discussione: (1) la seconda chiamata di
`show_markdown` non è un errore da sopprimere — è comportamento voluto, vedere la risposta
raffinarsi; (2) l'UNICO modo di chiudere durante un turno attivo passa dal modale — non esiste un
caso "si chiude e si riapre da sola" nel design finale; (3) "sì" nel modale annulla anche il turno
AI sottostante (`CancelCommand`, già esistente), non lo lascia proseguire silenzioso; (4) il
"progresso" visibile è onesto sui limiti reali: `web_search`/`web_fetch` sono tool **server-side**
(Anthropic), l'orchestratore non vede i singoli passi della ricerca — niente log "sto cercando
X…", solo un indicatore attivo/concluso + orario ultimo contenuto.

## Parte A — update-in-place

### A.1 — Prerequisito: `output-buffer.mjs` è oggi "one-shot", va reso persistente

**Leggi `crates/ui/frontend/output-buffer.mjs` per intero prima di toccarlo.** Il buffer
`createOutputBuffers()` esiste per un problema reale (la finestra di output è una webview
separata che si apre in modo asincrono; se il contenuto arriva prima che si sia iscritta va
bufferizzato, non perso) MA è scritto per un caso **one-shot**: sia `content()` sia `subscribe()`,
quando consegnano il contenuto, cancellano l'entry dalla mappa (`map.delete(windowId)`). Una
finestra che riceve un SECONDO `output_window_content` per lo stesso `window_id` (esattamente il
nostro caso: seconda chiamata di `show_markdown` nello stesso turno) troverebbe l'entry sparita,
la ributterizzerebbe come se la finestra non si fosse mai iscritta, e quel contenuto non
arriverebbe MAI alla finestra (nessuno richiama `subscribe()` una seconda volta — window.js lo fa
una volta sola al bootstrap). **Senza questo fix, la Parte A sembra funzionare al primo
show_markdown e si blocca silenziosamente dal secondo in poi** — verificalo tu stesso con un
test RED prima di procedere.

Fix: rendi la sottoscrizione persistente invece che one-shot. In `content(windowId, markdown)`,
quando `b.subscribed` è vero, NON cancellare più l'entry (`map.delete`) — lasciarla con
`subscribed: true`, così ogni `content()` successivo la trova ancora sottoscritta ed emette
subito. In `subscribe(windowId)`, quando c'è già contenuto bufferizzato, dopo averlo restituito
imposta l'entry a `{ markdown: null, subscribed: true }` invece di cancellarla. `close(windowId)`
resta l'UNICO modo in cui un'entry sparisce (già chiamato da `window.js` alla chiusura — verifica
che lo faccia anche per QUESTA finestra, non solo per l'output-turno standard).

Aggiorna il doc-comment del modulo (non è più "one-shot", ora "persistente fino alla chiusura
esplicita della finestra"). Aggiungi un test in `output-buffer.test.mjs`:

```js
test("content persiste dopo la sottoscrizione: più aggiornamenti, tutti emessi", () => {
  const b = createOutputBuffers();
  b.open("w1");
  assert.strictEqual(b.subscribe("w1"), null); // niente ancora bufferizzato
  assert.deepStrictEqual(b.content("w1", "primo"), { emit: true });
  assert.deepStrictEqual(b.content("w1", "secondo"), { emit: true }); // PRIMA di questo fix: { emit: false }
  assert.deepStrictEqual(b.content("w1", "terzo"), { emit: true });
});
```

Esegui l'intera suite esistente di `output-buffer.test.mjs` dopo il fix: deve restare tutta verde
(il cambiamento non altera il comportamento one-shot per chi non manda un secondo `content()`,
che è esattamente il caso dell'output-turno standard).

### A.2 — Nuovo messaggio protocollo: `MarkdownWindowTurnEnded`

`crates/protocol/src/lib.rs`, dentro `enum ServerMsg`, vicino a `OutputWindowContent`:

```rust
/// Segnala che il turno "proprietario" della finestra `window_id` (una
/// finestra `show_markdown`, vedi `ai_adapter.rs`) è concluso (successo,
/// errore o cancellazione) — la finestra lo usa per sapere se può chiudersi
/// liberamente o deve ancora mostrare il gate di conferma (Parte B) e il
/// badge "ricerca in corso" (Parte C). Mandato SOLO se quel turno ha
/// aperto una finestra `show_markdown` — un turno che non l'ha mai
/// chiamato non genera questo messaggio (nessuna finestra ad cui riferirsi).
MarkdownWindowTurnEnded { window_id: String },
```

Aggiungila al branch `Ui` di `ServerMsg::surface()` (match esaustivo, il compilatore ti obbliga
ad aggiungerla — stessa riga di `OutputWindowContent`/`OpenUiLocal`). Aggiorna i due test
esaustivi esistenti (cerca `activity_indicator` nei test: uno verifica il wire round-trip di
OGNI variante, l'altro verifica che ogni variante "verso ui" sia in quella lista — aggiungi
`MarkdownWindowTurnEnded` a entrambi, wire name atteso `"markdown_window_turn_ended"`, snake_case
automatico di serde). Bump `crates/protocol/Cargo.toml`: `2.1.1` → `2.2.0` (variante additiva,
minor).

### A.3 — `ai_adapter.rs`: update-in-place + guard di fine turno

Leggi `crates/orchestrator/src/ai_adapter.rs::respond()` per intero prima di modificarlo — ha
**7 punti di uscita** (`return` espliciti dopo `emit!(done())`/`emit!(fail())`/cancellazione, più
la fine naturale della funzione). Duplicare l'emissione di `MarkdownWindowTurnEnded` in ognuno è
fragile (un futuro ottavo punto di uscita lo dimenticherebbe silenziosamente). Usa invece un
guard RAII — a QUALUNQUE uscita della funzione (return esplicito o implicito, anche un panic),
`Drop` lo manda se necessario:

```rust
/// RAII: a qualunque uscita di `respond()`, se è stata aperta una finestra
/// `show_markdown` in questo turno, notifica alla UI che il turno è concluso
/// — una sola emissione garantita indipendentemente da QUALE dei ~7 punti di
/// uscita di `respond()` sia stato preso, senza doverli duplicare a mano.
struct MarkdownWindowEndGuard<'a> {
    tx: &'a UnboundedSender<ServerMsg>,
    window_id: Option<String>,
}
impl<'a> Drop for MarkdownWindowEndGuard<'a> {
    fn drop(&mut self) {
        if let Some(window_id) = self.window_id.take() {
            let _ = self.tx.send(ServerMsg::MarkdownWindowTurnEnded { window_id });
        }
    }
}
```

Dichiaralo subito prima del `for _ in 0..agent::MAX_ITERATIONS` esistente:

```rust
let mut md_window_guard = MarkdownWindowEndGuard { tx: &tx, window_id: None };
```

Nel branch `if name == "show_markdown"` (oggi ~8 righe: calcola `(title, content)`, poi un solo
`emit!(ServerMsg::OpenWindow{...})`), sostituisci con:

```rust
if name == "show_markdown" {
    if opts.allow_windows {
        let (title, content) = agent::markdown_window(&input);
        // window_id stabile per l'intero turno: `{id}-md`. CONTRATTO
        // esplicito col frontend (window.js, Parte B): un window_id che
        // finisce per "-md" appartiene a un turno, il turno stesso è
        // tutto ciò che precede quel suffisso. Non cambiare questo schema
        // senza aggiornare `window.js` di conseguenza.
        let window_id = format!("{id}-md");
        if md_window_guard.window_id.is_none() {
            // Prima chiamata nel turno: apre la finestra (stesso schema
            // dell'output-turno standard: Open poi subito Content — il fix
            // A.1 garantisce che nessun content vada perso per via della
            // finestra non ancora sottoscritta).
            emit!(ServerMsg::OpenOutputWindow { window_id: window_id.clone(), title: title.clone() });
            emit!(ServerMsg::OutputWindowContent { window_id: window_id.clone(), markdown: content });
            emit!(chunk(format!("\u{1F4C4} finestra aperta: {title}")));
            md_window_guard.window_id = Some(window_id);
        } else {
            // Chiamata successiva nello STESSO turno: aggiorna la finestra
            // già aperta, non aprirne una nuova (il fix vero e proprio).
            emit!(ServerMsg::OutputWindowContent { window_id, markdown: content });
            emit!(chunk("\u{1F4C4} finestra aggiornata".to_string()));
        }
    }
    ...
```

(il resto del branch, dopo — se esiste altro codice per `show_markdown` oltre l'apertura finestra
— resta invariato; leggi il branch reale prima di scrivere, questo è uno scheletro delle righe
che cambiano davvero).

**Non toccare** gli altri due produttori di `ServerMsg::OpenWindow` in questo stesso file (il
report differito `flush_pending_report` e il report immediato quando `defer_to_turn_end == false`,
entrambi per tool custom come `stock_report`) — restano `OpenWindow` verbatim, comportamento
invariato: sono documenti fattuali di UN tool diverso, aprono sempre esattamente una finestra a
turno per costruzione, non hanno il bug. Non toccare nemmeno gli `OpenWindow` di `/help`/`/show`
in `core.rs` (turni con `output_window == false`, mai passano da questo branch).

Test da aggiornare/aggiungere in `ai_adapter.rs` (usa `FakeChatBackend::sequence` come i test
esistenti per `show_markdown` — leggili prima, sono lo stesso identico schema):

- `show_markdown_first_call_opens_output_window_not_open_window`: una sequenza con UNA sola
  tool_use `show_markdown` seguita da testo finale → verifica che i messaggi contengano
  `OpenOutputWindow{window_id: "c1-md", ..}` + `OutputWindowContent{window_id: "c1-md", ..}`,
  **mai** `ServerMsg::OpenWindow`.
- `show_markdown_second_call_same_turn_updates_not_reopens`: una sequenza con DUE tool_use
  `show_markdown` (contenuti diversi, es. "bozza" poi "finale") prima del testo finale → verifica
  esattamente UN `OpenOutputWindow` (non due) e DUE `OutputWindowContent` per lo stesso
  `window_id`, nell'ordine.
- `show_markdown_turn_end_emits_markdown_window_turn_ended`: dopo un turno che ha chiamato
  `show_markdown` almeno una volta e si conclude con successo → l'ULTIMO messaggio (o comunque
  presente dopo `Done`) è `MarkdownWindowTurnEnded{window_id: "c1-md"}`.
- `no_show_markdown_no_turn_ended_signal`: un turno che non chiama mai `show_markdown` → nessun
  `MarkdownWindowTurnEnded` fra i messaggi (il guard non deve emettere nulla se `window_id` è
  rimasto `None`).
- `show_markdown_cancelled_turn_still_emits_turn_ended`: mirror del test esistente di
  cancellazione (`CancellationToken`, `Done{exit_code: Some(130)}`) ma con `show_markdown` chiamato
  PRIMA della cancellazione → `MarkdownWindowTurnEnded` presente comunque (il guard scatta anche
  sul `return` del ramo cancellato).

### A.4 — `agent.rs`: capovolgi la description e il test che la pinna

Riga ~144, `ToolDef` di `show_markdown`. La description oggi dice "chiamalo una sola volta per
turno... ogni chiamata apre una finestra nuova" — con l'update-in-place questa istruzione è
**sbagliata**: va invertita, dicendo esplicitamente che chiamate successive sono benvenute e
aggiornano la stessa finestra:

```rust
description: "Mostra contenuto Markdown ricco (spiegazioni lunghe, codice, tabelle) in una finestra dedicata. Usalo quando la risposta e' formattata o lunga invece di scriverla come testo semplice. Puoi chiamarlo più volte nello stesso turno via via che affini la risposta (es. dopo ricerche web successive): ogni chiamata AGGIORNA la stessa finestra con la versione più recente, non ne apre una seconda.".to_string(),
```

Rinomina e inverti il test `show_markdown_description_instructs_at_most_once_per_turn` (righe
~594-612) in `show_markdown_description_allows_multiple_calls_per_turn_to_update`: verifica che
la description NON contenga più "una sola volta"/"al massimo una volta"/"una volta sola", e che
contenga invece qualcosa come "più volte" e "aggiorna"/"aggiorn". Riscrivi anche il commento di
regressione sopra il test (righe ~587-593): non è più "la description deve dire di chiamarlo una
volta sola" ma "questa feature (Docs/i18n/ita/compiti-ai-esterne/2026-09-15-show-markdown-update-
in-place.md) è il fix vero della regressione trovata l'8/9 — l'istruzione 'una sola volta' era
una mitigazione temporanea, ora sostituita da un meccanismo che rende sicure le chiamate multiple".

### A.5 — Verifica

```powershell
cargo test -p protocol -p orchestrator
node --test crates/ui/frontend/output-buffer.test.mjs
cargo clippy --all-targets ; cargo fmt --check
```

## FINE PARTE A — commit e stop

## Parte B — Gate di chiusura con conferma + cancellazione cross-connection

### B.1 — Perché `CancelCommand` da una finestra secondaria non funziona oggi

`ClientMsg::CancelCommand{id}` esiste già (`ws.rs`, gestito riga ~394) e fa esattamente quello
che serve: cancella il `CancellationToken` del comando + `ShellSession::abort_turn`. **Ma** oggi
il token vive in una mappa LOCALE alla connessione che ha aperto il comando (`commands:
HashMap<String, CancellationToken>`, dentro `handle_connection` — riga ~336/649). Un turno `/ai`
viene aperto sulla connessione **shell** (lare-shell); la finestra Markdown gira dentro `ui.exe`,
che parla sulla connessione **ui**, separata. Un `CancelCommand{id}` mandato dalla connessione
`ui` cercherebbe l'id nella PROPRIA mappa `commands` (vuota per un turno shell) → no-op silenzioso,
il turno continuerebbe indisturbato. Serve un registro CONDIVISO fra connessioni.

`crates/orchestrator/src/connections.rs`: leggilo per intero (breve, ~150 righe). `Registry` è
già lo state condiviso `Arc<Mutex<Registry>>` per esattamente questo tipo di problema (sink `ui`
raggiungibile da un turno shell). Aggiungi:

```rust
use tokio_util::sync::CancellationToken;

pub struct Registry {
    // ... campi esistenti invariati ...
    command_tokens: HashMap<String, CancellationToken>,
}
// (aggiungi command_tokens: HashMap::new() al Default derive — se Registry
// deriva #[derive(Default)] come oggi, HashMap::default() è già vuoto,
// nessuna modifica al costruttore serve).

impl Registry {
    /// Registra il token di cancellazione del comando `id`, raggiungibile
    /// da QUALUNQUE connessione (non solo quella che ha aperto il comando).
    pub fn register_command_token(&mut self, id: &str, token: CancellationToken) {
        self.command_tokens.insert(id.to_string(), token);
    }

    /// Rimuove e ritorna il token per `id`, se presente. Usato sia per
    /// cancellare (poi `.cancel()` sul risultato) sia per la pulizia a fine
    /// turno (risultato scartato) — un comando concluso non deve restare
    /// nella mappa indefinitamente (leak).
    pub fn take_command_token(&mut self, id: &str) -> Option<CancellationToken> {
        self.command_tokens.remove(id)
    }
}
```

`crates/orchestrator/src/ws.rs`:
- Riga ~649-650 (`let cancel = CancellationToken::new(); commands.insert(id.clone(), cancel.clone());`):
  registra ANCHE nel registry condiviso: `registry.lock().await.register_command_token(&id, cancel.clone());`
  (verifica il nome esatto della variabile che tiene il registry in questa funzione — è lo stesso
  `SharedRegistry` già passato a `handle_connection`, usato altrove in questo file per il sink `ui`).
- Dentro il `tokio::spawn` che esegue `handle_command` (dopo l'`.await` che lo conclude, prima
  della fine del blocco async), pulisci: `let _ = registry_clone.lock().await.take_command_token(&id);`
  (serve clonare `registry`/l'`Arc` per il task spawnato, come già fai per `hist`/`out`/ecc. in
  quello stesso blocco — stesso schema `Arc::clone`).
- Branch `ClientMsg::CancelCommand{id}` (riga ~394-405): DOPO il tentativo sulla mappa locale
  (`commands.remove(&id)`), se non ha trovato nulla lì, prova il registro condiviso:
  ```rust
  ClientMsg::CancelCommand { id } => {
      let cancelled_locally = if let Some(cmd_token) = commands.remove(&id) {
          cmd_token.cancel();
          true
      } else {
          false
      };
      if !cancelled_locally {
          // Comando aperto su un'ALTRA connessione (es. turno shell,
          // cancellato dalla connessione ui — vedi Registry::register_command_token).
          if let Some(cmd_token) = registry.lock().await.take_command_token(&id) {
              cmd_token.cancel();
          }
      }
      if let Some(s) = &shell {
          s.abort_turn(&id).await;
      }
  }
  ```
  (`shell.abort_turn` resta invariato — `None` sulla connessione `ui`, quindi questo pezzo è
  no-op lì, corretto: l'`abort_turn` serve solo per sbloccare un `run_in_session` pendente sulla
  STESSA connessione shell, non fa parte del problema cross-connection).

Test nuovi in `ws.rs`/`connections.rs` (o `ws_integration.rs` se è lì che vivono i test end-to-end
di WS reali — verifica dove stanno i test analoghi esistenti per `CancelCommand` e mettili
accanto):
- `cancel_command_from_ui_connection_cancels_a_shell_originated_token`: una connessione shell
  apre un comando (registra un token), una connessione UI SEPARATA manda `CancelCommand` con lo
  stesso id → il token risulta cancellato (`token.is_cancelled()`).
- `cancel_command_cleans_up_registry_after_turn_completes`: dopo che un comando finisce
  normalmente (senza cancellazione), il suo token non è più recuperabile da
  `take_command_token` (rimosso, niente leak).

### B.2 — `host.js`: relay del nuovo evento

Leggi come `open_output_window`/`output_window_content` sono gestiti (righe ~216-233) per lo
stile esatto. Aggiungi, vicino:

```js
case "markdown_window_turn_ended":
  emitToPlugin("markdown:turn-ended", { window_id: msg.window_id });
  break;
```

### B.3 — `window.js`: il gate

Leggi `crates/ui/frontend/window.js` per intero prima di toccarlo — in particolare `myOutputId`
(riga 44, `null` per una finestra Markdown normale, valorizzato per una finestra `output-<id>`) e
`closeWindow()`/i suoi due chiamanti (bottone ×, Esc — righe 72-89).

**Riconoscere una finestra "di turno con show_markdown"**: il suo `myOutputId` (già derivato dalla
label via `outputWindowIdFromLabel`, `ui-local.mjs`) termina per `-md` — è il CONTRATTO esplicito
con `ai_adapter.rs` (§A.3: `window_id = format!("{id}-md")`). Deriva l'id del turno togliendo
quel suffisso: `const turnId = myOutputId?.endsWith("-md") ? myOutputId.slice(0, -3) : null;`. Una
finestra con `turnId !== null` è quella coinvolta dal gate; ogni altra finestra (Markdown normale,
output-turno standard senza `-md`) non cambia comportamento.

Stato locale (solo se `turnId !== null`): `let turnActive = true;` (una finestra così esiste solo
perché un turno l'ha aperta — il turno è per forza attivo al momento dell'apertura). Sottoscrivi
l'evento del §B.2:

```js
if (turnId && tauriEvent?.listen) {
  tauriEvent.listen("markdown:turn-ended", (event) => {
    if (event.payload?.window_id === myOutputId) turnActive = false;
  });
}
```

**Il gate vero e proprio.** Sostituisci `closeWindow()` (righe 72-77) con una funzione
`requestClose()` che decide se chiudere subito o mostrare il modale:

```js
async function requestClose() {
  if (turnId && turnActive) {
    showCloseConfirmModal();
    return;
  }
  await closeWindow(); // la vecchia closeWindow(), invariata, rinominata via
}
```

`closeBtnEl`/l'handler Esc (righe 81, 84-89) chiamano `requestClose()` invece di `closeWindow()`
direttamente.

**Intercetta anche la chiusura nativa** (Alt+F4, chiusura da barra — oggi NESSUN listener la
intercetta, bypasserebbe il gate): Tauri v2, `getCurrentWindow().onCloseRequested(event => {...})`.
Verifica l'import esatto già usato in questo file per `getCurrentWindow` (riga 44) e usa la stessa
istanza:

```js
const currentWindow = window.__TAURI__?.window?.getCurrentWindow?.();
if (turnId && currentWindow?.onCloseRequested) {
  currentWindow.onCloseRequested(async (event) => {
    if (turnActive) {
      event.preventDefault();
      showCloseConfirmModal();
    }
    // turnActive === false: nessun preventDefault, la chiusura nativa procede.
  });
}
```

**Il modale** (`showCloseConfirmModal()`): overlay HTML in-finestra (mai un dialog nativo del SO —
lo stile di ogni finestra Lare è chromeless/transparent, un dialog OS stonerebbe). Markup minimo
in `window.html` (nascosto di default, `hidden` o `display:none`), due bottoni:

```html
<div id="close-confirm-modal" class="modal-overlay" hidden>
  <div class="modal-box">
    <p data-i18n="closeConfirmMessage">La ricerca non è ancora finita. Chiudere comunque?</p>
    <button id="close-confirm-yes">Sì</button>
    <button id="close-confirm-no">No</button>
  </div>
</div>
```

```js
function showCloseConfirmModal() {
  document.getElementById("close-confirm-modal").hidden = false;
}
document.getElementById("close-confirm-yes").addEventListener("click", async () => {
  document.getElementById("close-confirm-modal").hidden = true;
  turnActive = false; // PRIMA di chiudere: altrimenti onCloseRequested rientra nel gate.
  if (tauriEvent?.emit) {
    try { await tauriEvent.emit("markdown:cancel-turn", { turn_id: turnId }); } catch (_) {}
  }
  await closeWindow();
});
document.getElementById("close-confirm-no").addEventListener("click", () => {
  document.getElementById("close-confirm-modal").hidden = true;
});
```

CSS minimo per `.modal-overlay`/`.modal-box` in `window.html` o il CSS del progetto per queste
finestre (cerca dove sta lo stile di `#titlebar`/`.close-btn` in questo stesso file/cartella e
mettiti nello stesso posto, stessa convenzione di naming).

### B.4 — Finestra principale: traduce l'evento in `CancelCommand`

Cerca dove `window-search.js`'s evento `search:cancel` viene tradotto in `ClientMsg::CancelSearch`
sul WS (probabilmente `terminal.js`, la finestra che possiede la connessione — leggi quel punto
per lo stile esatto) e aggiungi il gemello:

```js
tauriEvent.listen("markdown:cancel-turn", (event) => {
  const turnId = event.payload?.turn_id;
  if (turnId && client) client.sendCancelCommand(turnId); // o l'equivalente già esistente per CancelCommand
});
```

Verifica se un metodo per mandare `CancelCommand` esiste già sul client WS (cercalo — probabilmente
già usato altrove, es. per un pulsante "stop" nel terminale) prima di scriverne uno nuovo.

### B.5 — Verifica

```powershell
cargo test -p orchestrator
node --test crates/ui/frontend/*.test.mjs
cargo clippy --all-targets ; cargo fmt --check
```

Verifica manuale (non automatizzabile, ma fai una checklist esplicita nel report): apri un
`/ai "domanda che richiede ricerca web"`, mentre la finestra Markdown è aperta prova a chiuderla
→ appare il modale; "No" la lascia aperta e il turno prosegue (verifica che un secondo
`show_markdown` aggiorni comunque la finestra); "Sì" chiude la finestra E il terminale principale
riceve il `Done` di un turno cancellato (non aspetta il completamento naturale). Prova anche
Alt+F4 sulla finestra mentre il turno è attivo: stesso gate.

## FINE PARTE B — commit e stop

## Parte C — Badge di progresso

Solo frontend, nessuna modifica Rust. Nella stessa finestra (`window.js`/`window.html`, scoping
identico a Parte B: solo se `turnId !== null`).

- Un badge fisso (es. vicino al titolo) con due stati: `"🔍 ricerca in corso…"` mentre
  `turnActive === true`, `"✓ completato"` quando arriva `markdown:turn-ended` per questa finestra
  (stesso listener del §B.3 — aggiorna anche il badge, non solo `turnActive`).
- Una riga "aggiornato alle HH:MM:SS" che si aggiorna ad ogni `output:content` ricevuto per
  `myOutputId` (il listener esiste già per popolare il contenuto — aggiungi lì l'aggiornamento
  dell'orario, `new Date().toLocaleTimeString()` o equivalente, nessuna nuova sottoscrizione).

Nessun test automatico sensato per puro DOM/CSS in questo progetto (verifica se `window.test.mjs`
o simile esiste e già testa markup — se sì, segui lo stesso schema; altrimenti verifica manuale,
documentata nel report con uno screenshot o descrizione precisa di cosa hai visto).

## FINE PARTE C — commit e stop

## Cosa NON fare (tutte le parti)

- Non toccare i produttori di `OpenWindow` per `/help`, `/show`, e i "report" di tool custom
  (`stock_report` ecc.) — restano `OpenWindow` verbatim, non hanno il bug, non serve toccarli.
- Non introdurre un dialog nativo del SO per la conferma di chiusura — overlay HTML in-finestra,
  coerente con lo stile chromeless di ogni finestra Lare.
- Non derivare l'id del turno da una finestra Markdown "normale" (senza `-md`) — il gate si
  applica SOLO alle finestre aperte da `show_markdown` dentro un turno, non a `/help`/`/show`/
  finestre plugin.
- Non promettere un log passo-passo della ricerca web ("sto cercando X…") — quel dato non esiste
  lato orchestratore (tool server-side, opachi). Il badge dice solo "in corso"/"completato".
- Non lasciare `command_tokens` (Registry) crescere senza pulizia — ogni turno che si conclude
  (con o senza cancellazione) deve rimuovere la propria entry.
