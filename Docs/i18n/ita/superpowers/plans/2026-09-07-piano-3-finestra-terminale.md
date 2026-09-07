# Piano 3 — finestra terminale (`ui.exe` + xterm.js + ConPTY, modalità A) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** dare a `ui.exe` una vera finestra terminale (xterm.js nella webview, ConPTY via
`portable-pty`, `lare-shell.exe` come processo figlio) — la modalità A dello spec, l'ultimo pezzo
mancante di Lare Terminal 2.0 prima del MVP. Chiude anche i debiti aperti da piano 2b: self-heal
dell'orchestratore lato Rust (oggi esiste solo in C#), il consumo di `ActivityIndicator` (emesso
da piano 2a, mai letto da nessuno) e l'autostart di `ui.exe` da parte dell'orchestratore (spec
§6.4, mai implementato).

**Architecture:** una nuova finestra Tauri (`label: "terminal"`) ospita xterm.js; un nuovo modulo
Rust `pty.rs` fa da ponte fra i comandi Tauri (`pty_spawn`/`pty_write`/`pty_resize`) e
`portable-pty`, che lancia `lare-shell.exe --config-dir <dir> --session <id>` in una
pseudo-console; un secondo modulo `launcher.rs` (mirror di `Launcher.cs`) avvia l'orchestratore se
non risponde, PRIMA che la finestra terminale nasca. Lato orchestratore, un nuovo passo in
`start_turn` (`shell_turn.rs`) avvia `ui.exe --no-terminal` (nuovo flag: apre solo la host page
nascosta, mai la finestra terminale) quando serve una finestra e nessuna connessione `ui` esiste.
Il codice dello spike 2 (`spikes/lare-terminal-window/`, già verificato dal vivo — vedi il suo
doc di sintesi) è la base concreta per pty.rs/terminal.js: adattato, non reinventato.

**Tech Stack:** Rust (`tauri` 2, `portable-pty` 0.9, `base64` 0.22, `rand` 0.8 — quest'ultimo già
nel workspace), vanilla JS + `xterm.js` 6.0.0 + `addon-fit` 0.11.0 (vendored), C# (.NET, xUnit) per
la piccola modifica a `lare-shell`, `node:test` per la logica JS pura.

**Spec:** `Docs/i18n/ita/superpowers/specs/2026-09-04-lare-terminal-2-design.md` (§2.3 modalità
A/B, §5 finestra terminale, §6.4 avvio e self-heal, §9 gestione errori, §10 test, §12 fuori MVP).
Spike di riferimento (throwaway, NON toccare): `spikes/lare-terminal-window/` +
`Docs/i18n/ita/spikes/2026-09-05-lare-terminal-window.md`.

## Global Constraints

- **`--no-terminal`** (nuovo, sostituisce nessun flag esistente): fa sì che `ui.exe` apra SOLO la
  host page nascosta (comportamento di oggi), mai la finestra terminale. Lo passano SEMPRE chi
  avvia `ui.exe` per un ruolo "solo host": `lare-shell.exe` (`Launcher.EnsureUi`, C#, Task 3) e
  l'autostart dell'orchestratore (Task 6). Senza il flag, la finestra terminale nasce all'avvio
  (modalità A, uso interattivo diretto).
- **`--open config|library`** (flag di sviluppo del piano 1) è rimosso in questo piano (Task 0):
  la finestra terminale rende superfluo aprire una finestra "a mano" senza passare dalla shell.
- **Versioni vendored**: `xterm.js` **6.0.0**, `addon-fit` **0.11.0** (stesse dello spike,
  verificate dal vivo — non aggiornare senza motivo).
- **Dipendenze Rust nuove** (`crates/ui/src-tauri/Cargo.toml`): `portable-pty = "0.9"`,
  `base64 = "0.22"`, `rand = "0.8"` (già nel workspace, usato da `orchestrator`).
- **Formato evento `pty-out`**: payload = **stringa base64** di un chunk di byte grezzi (mai UTF-8
  diretto — un chunk può tagliare un carattere multi-byte a metà). **`pty-exit`**: payload = **i64**
  (exit code, `-1` se non determinabile).
- **Debounce del resize**: **50 ms** (valore confermato dallo spike 2, non altri).
- **Id di sessione**: 8 esadecimali minuscoli, **stesso formato** già in uso lato orchestratore
  (`format!("{:08x}", rand::random::<u32>())`, `crates/orchestrator/src/ws.rs:278`) — non un nuovo
  formato.
- **Timeout self-heal**: finestra di connessione **5 s**, retry **250 ms** (valori già usati da
  `Launcher.cs` lato C#, riusati identici lato Rust — spec §6.4 non li specifica diversamente).
  Attesa autostart `ui.exe` lato orchestratore: **10 s** (spec §9, tabella errori).
- **Processo staccato**: su Windows, `DETACHED_PROCESS (0x8) | CREATE_NEW_PROCESS_GROUP (0x200)`
  via `CommandExt::creation_flags` — **un solo posto** che lo sa fare (`startup_config::spawn_detached`,
  Task 3), riusato da `ui.exe` (self-heal orchestratore) e dall'orchestratore (autostart `ui.exe`).
- **TDD reale**: RED prima del codice, in tutti e tre i linguaggi coinvolti (Rust `cargo test`, C#
  `dotnet test`, JS `node:test`). Commenti didattici e prodighi, in italiano. Composizione (trait +
  fake) preferita all'ereditarietà — vedi `PtyOutputSink` (Task 2) e il parametro `spawn` di
  `ensure_ui_sink` (Task 6): stesso schema di `IProcessStarter` in `Launcher.cs`.
- **Riavvia il processo `orchestrator` dopo ogni build del backend** durante la verifica manuale
  (gotcha noto del progetto — `taskkill //F //IM orchestrator.exe //IM ui.exe //IM lare-shell.exe`).
- **Fuori scope di questo piano** (per la stessa ragione di §12 dello spec): streaming
  token-per-token della risposta AI (HANDOFF lo elenca sotto "piano 3" ma lo spec lo fila §12 —
  questo piano lo **esclude esplicitamente**: tocca il buffering in `surface.rs` e il re-render
  della finestra Markdown, è un piano a sé); tab/multi-finestra terminale (spec: "una finestra, una
  sessione" nell'MVP); tray icon/servizio Windows/autorun al login.

---

## Task 0: Scaffold — vendoring, dipendenze, rimozione `--open`

Prepara il terreno: le librerie JS che serviranno da Task 4 in poi, le dipendenze Rust, e la
rimozione pulita del flag di sviluppo `--open` (piano 1) ora che la finestra terminale lo rende
inutile. Nessun comportamento nuovo in questo task — solo terreno pulito.

**Files:**
- Create: `crates/ui/frontend/vendor/xterm.js` (copia da `spikes/lare-terminal-window/frontend/vendor/xterm.js`)
- Create: `crates/ui/frontend/vendor/xterm.css` (copia da `spikes/lare-terminal-window/frontend/vendor/xterm.css`)
- Create: `crates/ui/frontend/vendor/addon-fit.js` (copia da `spikes/lare-terminal-window/frontend/vendor/addon-fit.js`)
- Modify: `crates/ui/src-tauri/Cargo.toml` (aggiunge `portable-pty`, `base64`, `rand`)
- Create: `crates/ui/src-tauri/build.rs`
- Modify: `crates/ui/src-tauri/src/main.rs` (rimuove `DevOpenRequest`, `dev_open_request`, il
  parsing di `--open`, la voce in `generate_handler!`)
- Modify: `crates/ui/frontend/host.js` (rimuove il blocco `dev_open_request`)
- Modify: `CLAUDE.md`, `Docs/i18n/ita/DEPLOY.md`, `Docs/i18n/ita/RUN-LOCAL.md` (tolgono le note
  "flag di sviluppo `--open`, sparisce nel piano 3")
- Modify: `crates/ui/CHANGELOG.md` (voce `[Unreleased]`)

**Interfaces:**
- Produces: `crates/ui/frontend/vendor/{xterm.js,xterm.css,addon-fit.js}` pronti per essere
  caricati da `terminal.html` (Task 4) con `<script src="vendor/xterm.js">` /
  `<link rel="stylesheet" href="vendor/xterm.css">` — stessi nomi di file dello spike.

- [ ] **Step 1: copia i tre file vendored**

```powershell
Copy-Item "spikes\lare-terminal-window\frontend\vendor\xterm.js" "crates\ui\frontend\vendor\xterm.js"
Copy-Item "spikes\lare-terminal-window\frontend\vendor\xterm.css" "crates\ui\frontend\vendor\xterm.css"
Copy-Item "spikes\lare-terminal-window\frontend\vendor\addon-fit.js" "crates\ui\frontend\vendor\addon-fit.js"
```

Verifica: `Get-ChildItem crates\ui\frontend\vendor\` deve elencare 5 file (i 3 nuovi + i 2
esistenti `marked.min.js`/`purify.min.js`).

- [ ] **Step 2: aggiungi le dipendenze Rust**

In `crates/ui/src-tauri/Cargo.toml`, sotto `[dependencies]` (dopo `startup-config = { path =
"../../startup-config" }`), aggiungi:

```toml
portable-pty = "0.9"
base64 = "0.22"
rand = "0.8"
```

- [ ] **Step 3: `build.rs` per il gotcha `generate_context!`**

Lo spike 2 ha scoperto che `generate_context!` incorpora `frontendDist` a compile time: una
modifica al solo frontend non fa ricompilare il crate. Crea `crates/ui/src-tauri/build.rs`:

```rust
// build.rs — dice a Cargo di ricompilare `ui` quando cambia SOLO il frontend
// (../frontend). Senza questo, `generate_context!` in main.rs incorpora
// `frontendDist` a compile time e una modifica a terminal.js/host.js/*.mjs
// non fa ripartire la build: bisognava fare `cargo clean -p ui` a mano
// (gotcha scoperto durante lo spike 2, Docs/i18n/ita/spikes/2026-09-05-lare-terminal-window.md).
fn main() {
    tauri_build::build();
    println!("cargo:rerun-if-changed=../frontend");
}
```

Aggiungi `tauri-build = "2"` sotto `[build-dependencies]` in `crates/ui/src-tauri/Cargo.toml` (crea
la sezione se non esiste — è lo stesso helper che genera gli schema delle `capabilities/`, già
presente in `crates/ui/src-tauri/gen/schemas/`, quindi il crate `tauri-build` è già installato nel
lockfile: verifica con `cargo tree -p ui -i tauri-build` dopo lo Step 4 sotto).

- [ ] **Step 4: verifica che il workspace compili con le nuove dipendenze**

```powershell
cargo check -p ui
```

Expected: nessun errore. Se `tauri-build` non risolve, verifica la versione esatta con
`cargo tree -p ui` e allinea `[build-dependencies]` a quella già nel lockfile.

- [ ] **Step 5: rimuovi `DevOpenRequest` e il parsing `--open` da `main.rs`**

Rimuovi lo struct e il comando (righe 201-211 di `crates/ui/src-tauri/src/main.rs`):

```rust
pub struct DevOpenRequest(pub Option<String>);

#[tauri::command]
fn dev_open_request(state: tauri::State<DevOpenRequest>) -> Option<String> {
    state.0.clone()
}
```

e il relativo doc-comment sopra. Rimuovi il blocco di parsing in `main()` (righe 1294-1309):

```rust
    // ── Flag di sviluppo TEMPORANEO `--open config|library` (Task 7, piano 1
    //    → rimosso nel piano 3): ...
    let args: Vec<String> = std::env::args().collect();
    let open = args
        .iter()
        .position(|a| a == "--open")
        .and_then(|i| args.get(i + 1))
        .filter(|v| *v == "config" || *v == "library")
        .cloned();
```

Rimuovi `.manage(DevOpenRequest(open))` e la voce `dev_open_request,` in `generate_handler![...]`.

- [ ] **Step 6: rimuovi il blocco `--open` da `host.js`**

In `crates/ui/frontend/host.js`, righe 851-858, rimuovi:

```js
  // Flag di sviluppo TEMPORANEO (piano 1 → rimosso nel piano 3, vedi
  // main.rs::DevOpenRequest): ...
  const open = await invokeCmd("dev_open_request");
  if (open === "config") invokeCmd("open_config_window");
  if (open === "library") invokeCmd("open_library_window");
```

- [ ] **Step 7: aggiorna le tre note doc che citano `--open` come "temporaneo fino al piano 3"**

In `CLAUDE.md` riga 51, rimuovi la riga `# Flag di sviluppo (finché non c'è la modalità A, piano
3): ui.exe --open config|library`.

In `Docs/i18n/ita/DEPLOY.md`, sostituisci il blockquote (righe 4-7) rimuovendo la frase "fino ad
allora il flag di sviluppo `--open` resta utile per aprire una finestra senza passare dalla shell —
vedi `RUN-LOCAL.md`." con "La modalità A (`ui.exe` che lancia `lare-shell.exe` dentro una ConPTY) è
il piano 3, completato — vedi `RUN-LOCAL.md` sezione \"Modalità A\"."

In `Docs/i18n/ita/RUN-LOCAL.md`, rimuovi l'intera sezione "### Flag di sviluppo `--open`
(temporaneo, sparisce nel piano 3)" (righe 64-77) — la sostituisce la sezione "Modalità A" che
Task 7 aggiunge.

- [ ] **Step 8: verifica e commit**

```powershell
cargo build -p ui
cargo clippy -p ui --all-targets
```

Expected: build pulita, nessun nuovo warning clippy rispetto alla baseline.

```powershell
git add crates/ui/frontend/vendor/xterm.js crates/ui/frontend/vendor/xterm.css crates/ui/frontend/vendor/addon-fit.js crates/ui/src-tauri/Cargo.toml crates/ui/src-tauri/build.rs crates/ui/src-tauri/src/main.rs crates/ui/frontend/host.js CLAUDE.md "Docs/i18n/ita/DEPLOY.md" "Docs/i18n/ita/RUN-LOCAL.md" crates/ui/CHANGELOG.md
git commit -m "chore(ui): scaffold piano 3 — vendor xterm.js/addon-fit, dipendenze pty, rimosso --open"
```

---

## Task 1: Moduli JS puri (base64, OSC 9001, segnalini, debounce)

Logica pura estratta PRIMA di scrivere `terminal.js` (Task 4), per essere testabile con
`node:test` senza DOM/xterm.js — stesso principio di `output-buffer.mjs`/`host-dispatch.mjs`.

**Files:**
- Create: `crates/ui/frontend/base64.mjs`
- Create: `crates/ui/frontend/base64.test.mjs`
- Create: `crates/ui/frontend/osc-lare.mjs`
- Create: `crates/ui/frontend/osc-lare.test.mjs`
- Create: `crates/ui/frontend/indicators.mjs`
- Create: `crates/ui/frontend/indicators.test.mjs`
- Create: `crates/ui/frontend/fit-debounce.mjs`
- Create: `crates/ui/frontend/fit-debounce.test.mjs`

**Interfaces:**
- Produces (usati da `terminal.js`, Task 4): `base64ToUint8Array(b64: string): Uint8Array`;
  `parseLareOsc(data: string): {kind: "intercept", line: string} | null`;
  `createIndicatorState(): {lastCommand: string|null, aiBusy: boolean}`,
  `onIntercept(state, line): state`, `onActivity(state, sessionId, ownSessionId, on): state`;
  `createDebouncer(delayMs, setTimeoutFn?, clearTimeoutFn?): (fn: () => void) => void`.

- [ ] **Step 1: RED — `base64.test.mjs`**

```js
import { test } from "node:test";
import assert from "node:assert/strict";
import { base64ToUint8Array } from "./base64.mjs";

test("decodifica base64 in byte grezzi", () => {
  const encoded = Buffer.from("hello").toString("base64");
  const bytes = base64ToUint8Array(encoded);
  assert.deepEqual(Array.from(bytes), [104, 101, 108, 108, 111]);
});

test("stringa vuota decodifica in array vuoto", () => {
  assert.equal(base64ToUint8Array("").length, 0);
});
```

Run: `node --test crates/ui/frontend/base64.test.mjs` — Expected: FAIL (`base64.mjs` non esiste).

- [ ] **Step 2: GREEN — `base64.mjs`**

```js
// Decodifica pura di un chunk `pty-out` (stringa base64 → byte grezzi).
// `atob` è globale sia nel webview (Chromium) sia in Node 16+: nessuna
// dipendenza da `Buffer` (che non esiste nel browser), stesso codice nei
// due ambienti — per questo la funzione si può testare con node:test.
export function base64ToUint8Array(b64) {
  const binary = atob(b64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}
```

Run: `node --test crates/ui/frontend/base64.test.mjs` — Expected: PASS.

- [ ] **Step 3: RED — `osc-lare.test.mjs`**

```js
import { test } from "node:test";
import assert from "node:assert/strict";
import { parseLareOsc } from "./osc-lare.mjs";

test("riconosce un payload lare;intercept;<riga>", () => {
  assert.deepEqual(parseLareOsc("lare;intercept;/help"), { kind: "intercept", line: "/help" });
});

test("la riga può contenere punti e virgola: solo i primi due separano", () => {
  assert.deepEqual(parseLareOsc("lare;intercept;/ai \"a;b;c\""), {
    kind: "intercept",
    line: '/ai "a;b;c"',
  });
});

test("payload senza il prefisso lare;intercept; è null", () => {
  assert.equal(parseLareOsc("altro;9001;x"), null);
  assert.equal(parseLareOsc("lare;altro-evento;x"), null);
  assert.equal(parseLareOsc(""), null);
});
```

Run: `node --test crates/ui/frontend/osc-lare.test.mjs` — Expected: FAIL.

- [ ] **Step 4: GREEN — `osc-lare.mjs`**

```js
// Parsing puro del payload OSC 9001 emesso da `Osc.cs` (host lare-shell,
// spec §4.7): `lare;intercept;<riga intercettata>`. Estratto dall'handler
// inline di `registerOscHandler` (spike 2) per essere testabile senza
// xterm.js — terminal.js (Task 4) lo userà dentro quell'handler.
export function parseLareOsc(data) {
  const parts = data.split(";");
  if (parts[0] !== "lare" || parts[1] !== "intercept") return null;
  return { kind: "intercept", line: parts.slice(2).join(";") };
}
```

Run: `node --test crates/ui/frontend/osc-lare.test.mjs` — Expected: PASS.

- [ ] **Step 5: RED — `indicators.test.mjs`**

```js
import { test } from "node:test";
import assert from "node:assert/strict";
import { createIndicatorState, onIntercept, onActivity } from "./indicators.mjs";

test("stato iniziale: nessun comando, AI non al lavoro", () => {
  assert.deepEqual(createIndicatorState(), { lastCommand: null, aiBusy: false });
});

test("onIntercept aggiorna lastCommand, lascia aiBusy invariato", () => {
  const s = onIntercept({ lastCommand: null, aiBusy: true }, "/help");
  assert.deepEqual(s, { lastCommand: "/help", aiBusy: true });
});

test("onActivity con la propria sessione aggiorna aiBusy", () => {
  const s = onActivity({ lastCommand: null, aiBusy: false }, "abc123", "abc123", true);
  assert.equal(s.aiBusy, true);
});

test("onActivity di un'altra sessione è ignorato", () => {
  const before = { lastCommand: null, aiBusy: false };
  const after = onActivity(before, "altra-sessione", "abc123", true);
  assert.deepEqual(after, before);
});
```

Run: `node --test crates/ui/frontend/indicators.test.mjs` — Expected: FAIL.

- [ ] **Step 6: GREEN — `indicators.mjs`**

```js
// Stato puro della barra segnalini della finestra terminale (piano 3):
// ultimo comando intercettato (OSC 9001) e "AI al lavoro" (ActivityIndicator,
// Task 5). Nessun DOM qui — terminal.js legge questo stato e lo riflette
// nell'HTML; qui solo le transizioni, testabili da sole.
export function createIndicatorState() {
  return { lastCommand: null, aiBusy: false };
}

export function onIntercept(state, line) {
  return { ...state, lastCommand: line };
}

export function onActivity(state, sessionId, ownSessionId, on) {
  // Un ActivityIndicator per una sessione diversa dalla nostra finestra va
  // ignorato: l'MVP ha una sola finestra terminale per processo, ma il
  // filtro costa nulla e previene un bug quando ne arriverà una seconda.
  if (sessionId !== ownSessionId) return state;
  return { ...state, aiBusy: on };
}
```

Run: `node --test crates/ui/frontend/indicators.test.mjs` — Expected: PASS.

- [ ] **Step 7: RED — `fit-debounce.test.mjs`**

```js
import { test } from "node:test";
import assert from "node:assert/strict";
import { createDebouncer } from "./fit-debounce.mjs";

test("richiama fn una sola volta se invocato più volte entro il delay", () => {
  const scheduled = [];
  const fakeSetTimeout = (fn, ms) => {
    scheduled.push({ fn, ms, cancelled: false });
    return scheduled.length - 1;
  };
  const fakeClearTimeout = (handle) => {
    scheduled[handle].cancelled = true;
  };
  const debounced = createDebouncer(50, fakeSetTimeout, fakeClearTimeout);

  let calls = 0;
  debounced(() => calls++);
  debounced(() => calls++); // deve cancellare la schedulazione precedente

  const alive = scheduled.filter((s) => !s.cancelled);
  assert.equal(alive.length, 1, "una sola schedulazione viva");
  assert.equal(alive[0].ms, 50);
  alive[0].fn();
  assert.equal(calls, 1);
});
```

Run: `node --test crates/ui/frontend/fit-debounce.test.mjs` — Expected: FAIL.

- [ ] **Step 8: GREEN — `fit-debounce.mjs`**

```js
// Debounce puro (50 ms, confermato dallo spike 2) per il fit del terminale
// sul resize della finestra. `setTimeoutFn`/`clearTimeoutFn` sono un seam:
// nel prodotto sono i globali `setTimeout`/`clearTimeout`, nei test un fake
// che non aspetta tempo reale.
export function createDebouncer(delayMs, setTimeoutFn = setTimeout, clearTimeoutFn = clearTimeout) {
  let handle = null;
  return function debounced(fn) {
    if (handle !== null) clearTimeoutFn(handle);
    handle = setTimeoutFn(fn, delayMs);
  };
}
```

Run: `node --test crates/ui/frontend/fit-debounce.test.mjs` — Expected: PASS.

- [ ] **Step 9: tutti i test JS del crate insieme + commit**

```powershell
node --test crates/ui/frontend/*.test.mjs
```

Expected: PASS (inclusi i test JS pre-esistenti — nessuna regressione).

```powershell
git add crates/ui/frontend/base64.mjs crates/ui/frontend/base64.test.mjs crates/ui/frontend/osc-lare.mjs crates/ui/frontend/osc-lare.test.mjs crates/ui/frontend/indicators.mjs crates/ui/frontend/indicators.test.mjs crates/ui/frontend/fit-debounce.mjs crates/ui/frontend/fit-debounce.test.mjs
git commit -m "feat(ui): moduli JS puri per la finestra terminale (base64, OSC 9001, segnalini, debounce)"
```

---

## Task 2: `pty.rs` — plumbing ConPTY testabile

Il cuore Rust della finestra terminale: apre la pseudo-console, ci lancia un processo, inoltra
output/uscita a un `PtyOutputSink` iniettato (mai `AppHandle` Tauri direttamente in questo modulo —
è il confine che lo rende testabile con `cargo test -p ui`, spec §10: "pty plumbing con una shell
finta"). Corregge anche il bug mai risolto nello spike 2 (throwaway, quindi mai importava lì): lo
stato non si liberava mai alla fine del processo, rendendo impossibile un secondo `pty_spawn` (il
bottone "riavvia" di Task 4 dipende dal fix).

**Files:**
- Create: `crates/ui/src-tauri/src/pty.rs`
- Modify: `crates/ui/src-tauri/src/lib.rs` (aggiunge `pub mod pty;`)
- Modify: `crates/ui/src-tauri/src/config_dir.rs` (aggiunge `ConfigDirState::shell_exe()`)
- Modify: `crates/ui/src-tauri/src/main.rs` (comandi Tauri `pty_spawn`/`pty_write`/`pty_resize`,
  `.manage(PtyState(...))`, voci in `generate_handler!`)

**Interfaces:**
- Produces: `pty::PtyOutputSink` (trait: `on_output(&self, base64_chunk: &str)`,
  `on_exit(&self, code: i64)`), `pty::SharedPtyState` (= `Arc<Mutex<Option<PtySession>>>`),
  `pty::shared_state() -> SharedPtyState`, `pty::spawn<S: PtyOutputSink>(state, sink: Arc<S>, exe:
  &str, args: &[String], cwd: Option<&Path>, cols: u16, rows: u16) -> Result<(), String>`,
  `pty::write(state, data: &[u8]) -> Result<(), String>`, `pty::resize(state, cols, rows) ->
  Result<(), String>`. `ConfigDirState::shell_exe() -> PathBuf`.
- Consumes (da Task 0): dipendenze `portable-pty`/`base64` già in `Cargo.toml`.

- [ ] **Step 1: RED — test in `pty.rs` con una shell finta**

Scrivi `crates/ui/src-tauri/src/pty.rs` con SOLO questo blocco (il resto arriva allo Step 3):

```rust
// crates/ui/src-tauri/src/pty.rs
#[cfg(test)]
mod tests {
    #[test]
    fn placeholder_red() {
        panic!("pty::spawn non esiste ancora");
    }
}
```

Aggiungi `pub mod pty;` a `crates/ui/src-tauri/src/lib.rs` (dopo `pub mod library_watch;`).

Run: `cargo test -p ui pty::` — Expected: FAIL (panic del placeholder).

- [ ] **Step 2: scrivi il vero test (ancora RED — il modulo non c'è)**

Sostituisci il blocco `#[cfg(test)]` con:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
    use std::sync::Mutex as StdMutex;
    use std::time::Duration;

    #[derive(Default)]
    struct FakeSink {
        outputs: StdMutex<Vec<String>>,
        exit_code: AtomicI64,
        exited: AtomicBool,
    }
    impl PtyOutputSink for FakeSink {
        fn on_output(&self, chunk: &str) {
            self.outputs.lock().unwrap().push(chunk.to_string());
        }
        fn on_exit(&self, code: i64) {
            self.exit_code.store(code, Ordering::SeqCst);
            self.exited.store(true, Ordering::SeqCst);
        }
    }

    fn wait_until<F: Fn() -> bool>(cond: F) {
        for _ in 0..100 {
            if cond() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("timeout in attesa della condizione");
    }

    #[test]
    fn spawn_scrive_e_riceve_output_di_una_shell_finta() {
        let state = shared_state();
        let sink = std::sync::Arc::new(FakeSink::default());
        // "cmd /c echo hello": shell finta disponibile su ogni Windows —
        // niente dipendenza da lare-shell.exe (spec §10).
        spawn(
            &state,
            sink.clone(),
            "cmd",
            &["/c".into(), "echo hello".into()],
            None,
            80,
            24,
        )
        .unwrap();

        wait_until(|| sink.exited.load(Ordering::SeqCst));
        assert_eq!(sink.exit_code.load(Ordering::SeqCst), 0);

        let decoded: String = sink
            .outputs
            .lock()
            .unwrap()
            .iter()
            .map(|b64| {
                use base64::Engine as _;
                String::from_utf8(base64::engine::general_purpose::STANDARD.decode(b64).unwrap())
                    .unwrap()
            })
            .collect();
        assert!(decoded.contains("hello"), "output ricevuto: {decoded:?}");

        // Lo stato è tornato libero: una nuova spawn deve poter ripartire
        // (bottone "riavvia" di terminal.js, Task 4) — non il bug "sessione
        // già attiva" mai corretto nello spike 2 (throwaway).
        assert!(state.lock().unwrap().is_none());
    }

    #[test]
    fn spawn_rifiuta_una_seconda_sessione_mentre_la_prima_e_attiva() {
        let state = shared_state();
        let sink = std::sync::Arc::new(FakeSink::default());
        spawn(&state, sink.clone(), "cmd", &["/c".into(), "pause".into()], None, 80, 24).unwrap();
        let err = spawn(&state, sink, "cmd", &["/c".into(), "echo x".into()], None, 80, 24)
            .unwrap_err();
        assert!(err.contains("già attiva"));
        // pulizia: un invio termina "pause", non lasciamo un cmd.exe orfano.
        let _ = write(&state, b"\r\n");
    }
}
```

Run: `cargo test -p ui pty::` — Expected: FAIL (`spawn`/`shared_state`/`write`/`PtyOutputSink` non
esistono).

- [ ] **Step 3: GREEN — implementazione**

Sopra il blocco `#[cfg(test)]`, in `crates/ui/src-tauri/src/pty.rs`:

```rust
//! Wrapper puro attorno a `portable-pty` (ConPTY su Windows, spec §5): apre
//! una pseudo-console, ci lancia un processo, e inoltra output/uscita a un
//! `PtyOutputSink` iniettato — mai `AppHandle` direttamente qui, così questo
//! modulo si testa con `cargo test -p ui` senza un runtime Tauri (spec §10:
//! "pty plumbing con una shell finta").
//!
//! Confine OOP: `PtyOutputSink` è l'interfaccia (trait) fra "cosa è successo
//! nella pty" e "come lo sa il resto del mondo" — nel prodotto la implementa
//! un `AppHandle` Tauri (emette gli eventi `pty-out`/`pty-exit`, vedi
//! `main.rs`), nei test un fake che registra le chiamate in un `Vec`. Stesso
//! schema di `IProcessStarter` nella host C# (composizione, non eredità).
//!
//! Basato sul codice verificato dal vivo dello spike 2
//! (`spikes/lare-terminal-window/src-tauri/src/main.rs`), adattato per
//! separare la pura logica pty (qui) dai comandi Tauri (`main.rs`).

use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

/// Confine testabile: nel prodotto lo implementa `AppHandle` (emette eventi
/// Tauri), nei test un fake che registra le chiamate.
pub trait PtyOutputSink: Send + Sync + 'static {
    /// Chunk di output pty, già codificato base64 — vedi `spawn_reader_thread`
    /// sul perché base64 e non una `String` UTF-8 diretta.
    fn on_output(&self, base64_chunk: &str);
    /// Il processo figlio è terminato, con questo exit code (-1 se non determinabile).
    fn on_exit(&self, code: i64);
}

/// Sessione pty attiva: master (per il resize) + writer (per scrivere
/// input). NIENTE campo `child` qui — vedi `spawn_exit_watcher`:
/// `Child::wait()` richiede possesso esclusivo e vive nel suo thread
/// dedicato, non nello stato condiviso.
pub struct PtySession {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
}

/// Stato condiviso: `None` finché nessuna sessione è mai partita; tornato a
/// `None` dall'exit watcher quando il processo termina, così una nuova
/// `spawn` (bottone "riavvia") trova lo stato libero invece di rifiutarsi
/// con "sessione già attiva" (bug mai corretto nello spike 2, throwaway).
pub type SharedPtyState = Arc<Mutex<Option<PtySession>>>;

pub fn shared_state() -> SharedPtyState {
    Arc::new(Mutex::new(None))
}

/// Apre la pseudo-console e ci lancia `exe` con `args`, in `cwd` (default: la
/// cwd del processo). Errore se una sessione è già attiva ("una finestra,
/// una sessione", spec §5) o se lo spawn fallisce.
pub fn spawn<S: PtyOutputSink>(
    state: &SharedPtyState,
    sink: Arc<S>,
    exe: &str,
    args: &[String],
    cwd: Option<&Path>,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    if guard.is_some() {
        return Err("una sessione pty è già attiva".to_string());
    }

    // ConPTY rifiuta dimensioni 0: clampiamo invece di propagare un errore
    // criptico se il frontend invoca questo comando prima che #terminal
    // abbia una dimensione reale.
    let cols = cols.max(2);
    let rows = rows.max(1);

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("openpty fallita: {e}"))?;

    let mut cmd = CommandBuilder::new(exe);
    for a in args {
        cmd.arg(a);
    }
    if let Some(dir) = cwd {
        cmd.cwd(dir);
    }

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("spawn_command fallita ({exe}): {e}"))?;

    // Il lato slave non serve più al padre una volta ereditato dal figlio;
    // tenerlo in vita impedisce a certi backend di segnalare mai un EOF di
    // lettura (spike 2).
    drop(pair.slave);

    let writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("take_writer fallita: {e}"))?;
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("try_clone_reader fallita: {e}"))?;

    spawn_reader_thread(sink.clone(), reader);
    spawn_exit_watcher(sink, child, state.clone());

    *guard = Some(PtySession {
        master: pair.master,
        writer,
    });
    Ok(())
}

/// Scrive `data` nella pty attiva (errore se nessuna sessione è viva).
pub fn write(state: &SharedPtyState, data: &[u8]) -> Result<(), String> {
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    let session = guard.as_mut().ok_or("nessuna sessione pty attiva")?;
    session.writer.write_all(data).map_err(|e| e.to_string())?;
    session.writer.flush().map_err(|e| e.to_string())
}

/// Ridimensiona la pty attiva.
pub fn resize(state: &SharedPtyState, cols: u16, rows: u16) -> Result<(), String> {
    let guard = state.lock().map_err(|e| e.to_string())?;
    let session = guard.as_ref().ok_or("nessuna sessione pty attiva")?;
    session
        .master
        .resize(PtySize {
            rows: rows.max(1),
            cols: cols.max(2),
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())
}

fn spawn_reader_thread<S: PtyOutputSink>(sink: Arc<S>, mut reader: Box<dyn Read + Send>) {
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break, // pty chiusa dal lato lettura.
                Ok(n) => sink.on_output(&BASE64.encode(&buf[..n])),
                Err(_) => break,
            }
        }
    });
}

/// Possiede il processo figlio e blocca su `wait()` finché non termina:
/// l'UNICO segnale affidabile di fine-processo su Windows/ConPTY (che non
/// segnala mai un EOF di lettura alla chiusura del figlio — spike 2, fatto
/// tecnico #1). Libera lo stato PRIMA di notificare: un `on_exit` che
/// richiama subito `spawn` (bottone "riavvia") deve trovare la sessione già
/// sgombra.
fn spawn_exit_watcher<S: PtyOutputSink>(
    sink: Arc<S>,
    mut child: Box<dyn Child + Send + Sync>,
    state: SharedPtyState,
) {
    std::thread::spawn(move || {
        let code = match child.wait() {
            Ok(status) => status.exit_code() as i64,
            Err(_) => -1,
        };
        if let Ok(mut guard) = state.lock() {
            *guard = None;
        }
        sink.on_exit(code);
    });
}
```

Run: `cargo test -p ui pty::` — Expected: PASS (entrambi i test).

- [ ] **Step 4: `ConfigDirState::shell_exe()`**

In `crates/ui/src-tauri/src/config_dir.rs`, dopo `plugins_dir()`:

```rust
    /// Percorso risolto dell'eseguibile `lare-shell.exe` (Task 2, piano 3):
    /// stesso schema di `plugins_dir()`, `paths.shell` di `startup.json`
    /// risolto contro la radice del deploy.
    pub fn shell_exe(&self) -> PathBuf {
        StartupConfig::resolve_path(&self.config_dir, &self.startup.paths.shell)
    }
```

- [ ] **Step 5: comandi Tauri in `main.rs`**

Aggiungi, vicino a `diagnose_connection` (usa lo stesso stile: `#[tauri::command]`, `State`):

```rust
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

/// Avvia `lare-shell.exe --config-dir <dir> --session <id>` nella pty della
/// finestra terminale. `session_id` arriva dal frontend, che lo ha ottenuto
/// da `get_terminal_session` (Task 4) — un solo id per finestra, generato
/// UNA volta alla creazione della finestra, non ad ogni riavvio della shell.
#[tauri::command]
fn pty_spawn(
    app: tauri::AppHandle,
    state: tauri::State<'_, PtyState>,
    cfg: tauri::State<'_, config_dir::ConfigDirState>,
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
fn pty_write(state: tauri::State<'_, PtyState>, data: String) -> Result<(), String> {
    ui_lib::pty::write(&state.0, data.as_bytes())
}

#[tauri::command]
fn pty_resize(state: tauri::State<'_, PtyState>, cols: u16, rows: u16) -> Result<(), String> {
    ui_lib::pty::resize(&state.0, cols, rows)
}
```

Aggiungi `.manage(PtyState(ui_lib::pty::shared_state()))` nel builder (vicino a
`.manage(cfg_state)`), e `pty_spawn, pty_write, pty_resize,` in `generate_handler![...]`.

- [ ] **Step 6: verifica e commit**

```powershell
cargo test -p ui
cargo clippy -p ui --all-targets
```

Expected: tutti i test passano, nessun nuovo warning.

```powershell
git add crates/ui/src-tauri/src/pty.rs crates/ui/src-tauri/src/lib.rs crates/ui/src-tauri/src/config_dir.rs crates/ui/src-tauri/src/main.rs
git commit -m "feat(ui): pty.rs — plumbing ConPTY testabile, comandi pty_spawn/pty_write/pty_resize"
```

---

## Task 3: `launcher.rs` — self-heal dell'orchestratore lato Rust + flag `--no-terminal`

Oggi il self-heal esiste SOLO in C# (`Launcher.cs`, modalità B). `ui.exe` in modalità A ne ha
bisogno esattamente allo stesso modo: se il WS non risponde, deve provare ad avviare
`orchestrator.exe` prima di aprire la finestra terminale (altrimenti la shell nasce già "orfana").
Aggiunge anche il flag `--no-terminal` (Global Constraints) e lo helper `spawn_detached`
condiviso, e la piccola correzione a `lare-shell` che lo usa.

**Files:**
- Create: `crates/ui/src-tauri/src/launcher.rs`
- Modify: `crates/ui/src-tauri/src/lib.rs` (aggiunge `pub mod launcher;`)
- Modify: `crates/startup-config/src/lib.rs` (aggiunge `pub fn spawn_detached`)
- Modify: `crates/ui/src-tauri/src/main.rs` (chiama `ensure_orchestrator` in `main()`, parsing
  `--no-terminal`, `.manage(NoTerminal(...))`)
- Modify: `shell/lare-shell/src/LareShell/Shell/Launcher.cs` (passa `--no-terminal` a `ui.exe`)
- Modify: `shell/lare-shell/tests/LareShell.Tests/Shell/LauncherTests.cs` (aggiorna l'asserzione)
- Modify: `shell/lare-shell/CHANGELOG.md` (bump a `2.0.1`, voce `Fixed`)
- Modify: `shell/lare-shell/src/LareShell/LareShell.csproj` (`<Version>2.0.1</Version>`)

**Interfaces:**
- Produces: `startup_config::spawn_detached(exe: &Path, args: &[String]) ->
  std::io::Result<std::process::Child>` (riusato anche da Task 6, orchestrator).
  `launcher::ensure_orchestrator(connect: &dyn Fn() -> Option<String>, autostart: bool,
  orchestrator_exe: &Path, config_dir: &Path, window: Duration, retry: Duration) -> bool`.
  `launcher::CONNECT_WINDOW: Duration` (5s), `launcher::RETRY_INTERVAL: Duration` (250ms).

- [ ] **Step 1: RED — `spawn_detached` in `startup-config`**

In `crates/startup-config/src/lib.rs`, aggiungi in fondo al modulo test (`mod tests`):

```rust
    #[test]
    fn spawn_detached_avvia_un_processo_reale() {
        let mut child =
            spawn_detached(Path::new("cmd"), &["/c".into(), "exit".into(), "0".into()]).unwrap();
        let status = child.wait().unwrap();
        assert!(status.success());
    }
```

Run: `cargo test -p startup-config spawn_detached` — Expected: FAIL (funzione non esiste).

- [ ] **Step 2: GREEN — `spawn_detached`**

Aggiungi in `crates/startup-config/src/lib.rs` (fuori dal modulo test, vicino a `deploy_root`):

```rust
/// Avvia `exe` con `args`, staccato dal processo corrente (spec §6.4):
/// nessuna console ereditata, un Ctrl+C nel padre non lo abbatte. Un solo
/// posto che sa COME staccare un processo su Windows — riusato sia da
/// `ui.exe` (self-heal dell'orchestratore, `launcher::ensure_orchestrator`)
/// sia dall'orchestratore stesso (autostart di `ui.exe`, piano 3 Task 6),
/// invece di duplicare la logica nei due crate.
#[cfg(windows)]
pub fn spawn_detached(exe: &Path, args: &[String]) -> std::io::Result<std::process::Child> {
    use std::os::windows::process::CommandExt;
    // DETACHED_PROCESS (0x8) | CREATE_NEW_PROCESS_GROUP (0x200): equivalente
    // Windows di `setsid` — nessuna console ereditata, gruppo di processi
    // proprio (un Ctrl+C nella console del padre non raggiunge il figlio).
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    std::process::Command::new(exe)
        .args(args)
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .spawn()
}

/// Non verificato fuori Windows (il prodotto oggi lo è, ADR-019): spawn
/// semplice, senza distacco — meglio di un errore di compilazione su altre
/// piattaforme di sviluppo.
#[cfg(not(windows))]
pub fn spawn_detached(exe: &Path, args: &[String]) -> std::io::Result<std::process::Child> {
    std::process::Command::new(exe).args(args).spawn()
}
```

Run: `cargo test -p startup-config` — Expected: PASS (tutti i test, inclusi i pre-esistenti).

- [ ] **Step 3: RED — `launcher.rs` (ui crate)**

Crea `crates/ui/src-tauri/src/launcher.rs`:

```rust
// crates/ui/src-tauri/src/launcher.rs
#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::path::Path;
    use std::time::Duration;

    #[test]
    fn ensure_orchestrator_non_avvia_nulla_se_la_connessione_riesce_subito() {
        let calls = RefCell::new(0);
        let connect = || {
            *calls.borrow_mut() += 1;
            None
        };
        let ok = ensure_orchestrator(
            &connect,
            true,
            Path::new("non-esiste.exe"),
            Path::new("."),
            Duration::from_millis(1),
            Duration::from_millis(1),
        );
        assert!(ok);
        assert_eq!(*calls.borrow(), 1); // un solo tentativo, nessun avvio.
    }

    #[test]
    fn ensure_orchestrator_rifiuta_se_autostart_disattivo() {
        let connect = || Some("timeout".to_string());
        let ok = ensure_orchestrator(
            &connect,
            false,
            Path::new("non-esiste.exe"),
            Path::new("."),
            Duration::from_millis(1),
            Duration::from_millis(1),
        );
        assert!(!ok);
    }

    #[test]
    fn ensure_orchestrator_rifiuta_se_leseguibile_non_esiste() {
        let connect = || Some("timeout".to_string());
        let ok = ensure_orchestrator(
            &connect,
            true,
            Path::new("sicuramente-non-esiste.exe"),
            Path::new("."),
            Duration::from_millis(1),
            Duration::from_millis(1),
        );
        assert!(!ok);
    }
}
```

Run: `cargo test -p ui launcher::` — Expected: FAIL (`ensure_orchestrator` non esiste).

- [ ] **Step 4: GREEN — implementazione**

Sopra il blocco `#[cfg(test)]`:

```rust
//! Self-heal all'avvio di `ui.exe` (spec §6.4): se il WS dell'orchestratore
//! non risponde e `autostart.orchestrator` è attivo in `startup.json`,
//! avvialo e ritenta per una finestra di tempo. Mirror Rust di
//! `shell/lare-shell/src/LareShell/Shell/Launcher.cs` (stessa struttura,
//! stessi nomi dove sensato) — un motore, due implementazioni dello stesso
//! ruolo (host C#, qui `ui.exe`), non eredità fra loro.

use std::path::Path;
use std::time::{Duration, Instant};

use startup_config::spawn_detached;

pub const CONNECT_WINDOW: Duration = Duration::from_secs(5);
pub const RETRY_INTERVAL: Duration = Duration::from_millis(250);

/// Se `connect()` fallisce e `autostart` è attivo, avvia `orchestrator_exe
/// --config-dir <config_dir>` (staccato) e ritenta `connect()` ogni `retry`
/// fino a `window`. `connect()`: `None` = riuscito, altrimenti il motivo
/// (stessa forma di `Func<string?>` in `Launcher.cs::EnsureConnected`).
/// Ritorna `true` se, alla fine, `connect()` ha avuto successo.
pub fn ensure_orchestrator(
    connect: &dyn Fn() -> Option<String>,
    autostart: bool,
    orchestrator_exe: &Path,
    config_dir: &Path,
    window: Duration,
    retry: Duration,
) -> bool {
    let Some(reason) = connect() else {
        return true;
    };

    if !autostart {
        eprintln!("[ui] orchestratore non raggiungibile ({reason}); autostart disattivo in startup.json");
        return false;
    }
    if !orchestrator_exe.exists() {
        eprintln!("[ui] orchestratore non raggiungibile ({reason}) e {orchestrator_exe:?} non esiste");
        return false;
    }

    eprintln!("[ui] orchestratore non raggiungibile ({reason}): avvio {orchestrator_exe:?}");
    let args = vec![
        "--config-dir".to_string(),
        config_dir.to_string_lossy().into_owned(),
    ];
    if let Err(e) = spawn_detached(orchestrator_exe, &args) {
        eprintln!("[ui] avvio orchestratore fallito: {e}");
        return false;
    }

    let deadline = Instant::now() + window;
    while Instant::now() < deadline {
        std::thread::sleep(retry);
        if connect().is_none() {
            eprintln!("[ui] orchestratore avviato e connesso");
            return true;
        }
    }

    eprintln!("[ui] orchestratore non raggiungibile dopo {window:?}: i comandi non funzioneranno finché non risponde");
    false
}
```

Run: `cargo test -p ui launcher::` — Expected: PASS.

Aggiungi `pub mod launcher;` a `crates/ui/src-tauri/src/lib.rs`.

- [ ] **Step 5: wiring in `main()`**

In `crates/ui/src-tauri/src/main.rs`, dopo il blocco che stampa `[ui] config dir: ...` (circa riga
1292) e PRIMA di `tauri::Builder::default()`:

```rust
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
    let orchestrator_exe = startup_config::deploy_root(&cfg_state.config_dir).join("orchestrator.exe");
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
```

Aggiungi `.manage(NoTerminal(no_terminal))` nel builder, e lo struct (vicino a `PtyState`):

```rust
/// true se `ui.exe` è stato avviato in ruolo "solo host" (`--no-terminal`,
/// piano 3): non apre la finestra terminale — resta solo la host page
/// nascosta + le finestre aperte on-demand (config, library, ...).
struct NoTerminal(bool);
```

(La finestra terminale vera e propria — che legge questo stato — è Task 4.)

- [ ] **Step 6: `lare-shell` 2.0.1 — `Launcher.EnsureUi` passa `--no-terminal`**

In `shell/lare-shell/src/LareShell/Shell/Launcher.cs`, nel metodo `EnsureUi()`:

```csharp
        _starter.Start(UiExe, new[] { "--config-dir", _configDir }, hideWindow: true);
```

diventa:

```csharp
        // "--no-terminal" (piano 3): la host C# possiede già la propria
        // shell (questo processo); `ui.exe` in modalità B serve SOLO per le
        // finestre (config/library/output), mai per una seconda finestra
        // terminale — altrimenti ogni /comando ne aprirebbe una non voluta.
        _starter.Start(UiExe, new[] { "--config-dir", _configDir, "--no-terminal" }, hideWindow: true);
```

- [ ] **Step 7: RED→GREEN il test C# esistente**

In `shell/lare-shell/tests/LareShell.Tests/Shell/LauncherTests.cs`, riga 110:

```csharp
        Assert.Equal(new[] { "--config-dir", Cfg }, args);
```

Run prima della modifica: `dotnet test shell/lare-shell/LareShell.sln --filter EnsureUi_avvia_ui_exe_solo_se_non_gira_gia`
— Expected: FAIL (l'asserzione vede ancora 2 argomenti, lo Step 6 ne produce 3).

Poi correggi l'asserzione:

```csharp
        Assert.Equal(new[] { "--config-dir", Cfg, "--no-terminal" }, args);
```

Run: stesso comando — Expected: PASS.

- [ ] **Step 8: bump versione `lare-shell` 2.0.1**

In `shell/lare-shell/src/LareShell/LareShell.csproj`, riga 9: `<Version>2.0.0</Version>` →
`<Version>2.0.1</Version>`.

In `shell/lare-shell/CHANGELOG.md`, sotto `## [Unreleased]` (o crea `## [2.0.1]` se
`[Unreleased]` è vuoto dopo l'ultima release):

```markdown
### Fixed
- `Launcher.EnsureUi()` passa `--no-terminal` a `ui.exe` (piano 3): senza, ogni self-heal in
  modalità B avrebbe aperto una seconda finestra terminale non voluta.
```

- [ ] **Step 9: verifica completa e commit**

```powershell
cargo test -p ui -p startup-config
dotnet test shell/lare-shell/LareShell.sln
cargo clippy -p ui -p startup-config --all-targets
```

Expected: tutto verde, nessun nuovo warning.

```powershell
git add crates/ui/src-tauri/src/launcher.rs crates/ui/src-tauri/src/lib.rs crates/ui/src-tauri/src/main.rs crates/startup-config/src/lib.rs shell/lare-shell/src/LareShell/Shell/Launcher.cs shell/lare-shell/tests/LareShell.Tests/Shell/LauncherTests.cs shell/lare-shell/src/LareShell/LareShell.csproj shell/lare-shell/CHANGELOG.md
git commit -m "feat(ui,startup-config,lare-shell): self-heal Rust dell'orchestratore, flag --no-terminal, lare-shell 2.0.1"
```

---

## Task 4: finestra terminale — HTML/CSS/JS + capability + costruzione della finestra

Il pezzo visibile: xterm.js nella webview, cablato a `pty_spawn`/`pty_write`/`pty_resize` (Task 2),
con OSC 9001 (Task 1), segnalini (Task 1), resize debounce (Task 1), pulsanti `/help` `/library`
`/aichat` `/config`, e un banner "riavvia" quando la shell termina (usa il fix di Task 2).

**Files:**
- Create: `crates/ui/frontend/terminal.html`
- Create: `crates/ui/frontend/terminal.css`
- Create: `crates/ui/frontend/terminal.js`
- Create: `crates/ui/src-tauri/capabilities/terminal-window.json`
- Modify: `crates/ui/src-tauri/src/main.rs` (comando `get_terminal_session`, costruzione della
  finestra `terminal` in `setup()`, gated da `NoTerminal`)

**Interfaces:**
- Consumes (Task 1): `base64ToUint8Array`, `parseLareOsc`, `createIndicatorState`/`onIntercept`/
  `onActivity`, `createDebouncer`. (Task 2): comandi Tauri `pty_spawn({sessionId, cols, rows})`,
  `pty_write({data})`, `pty_resize({cols, rows})`, eventi `pty-out`/`pty-exit`.
- Produces: comando Tauri `get_terminal_session() -> String`; evento globale `terminal:activity`
  (ascoltato qui, emesso da Task 5) con payload `{session_id, kind, on}`.

- [ ] **Step 1: `terminal.html`**

```html
<!doctype html>
<html lang="it">
<head>
  <meta charset="utf-8" />
  <title>Lare Terminal</title>
  <link rel="stylesheet" href="vendor/xterm.css" />
  <link rel="stylesheet" href="terminal.css" />
</head>
<body>
  <div id="bar-top">
    <span id="session-label"></span>
    <span id="last-command">ultimo: —</span>
    <span id="activity-dot" class="dot"></span>
    <span id="clock"></span>
  </div>
  <div id="terminal">
    <div id="restart-banner" hidden>
      <span id="restart-message"></span>
      <button id="restart-btn">riavvia</button>
    </div>
  </div>
  <div id="bar-bottom">
    <button class="slash-btn" data-cmd="/help">/help</button>
    <button class="slash-btn" data-cmd="/library">/library</button>
    <button class="slash-btn" data-cmd="/aichat">/aichat</button>
    <button class="slash-btn" data-cmd="/config">/config</button>
  </div>

  <script src="vendor/xterm.js"></script>
  <script src="vendor/addon-fit.js"></script>
  <script type="module" src="terminal.js"></script>
</body>
</html>
```

- [ ] **Step 2: `terminal.css`**

```css
:root {
  color-scheme: dark;
  --bg: #0c0c0c;
  --fg: #cccccc;
  --bar-bg: #1a1a1a;
  --dot-off: #444444;
  --dot-on: #13a10e;
}
html, body {
  margin: 0;
  height: 100%;
  background: var(--bg);
  color: var(--fg);
  font-family: "Cascadia Mono", Consolas, monospace;
}
body {
  display: grid;
  grid-template-rows: auto 1fr auto;
  height: 100vh;
}
#bar-top, #bar-bottom {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 4px 10px;
  background: var(--bar-bg);
  font-size: 12px;
}
#bar-top { justify-content: space-between; }
#terminal { position: relative; overflow: hidden; }
.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--dot-off);
  transition: background 0.2s;
}
.dot.on { background: var(--dot-on); }
.slash-btn {
  background: #252525;
  color: var(--fg);
  border: 1px solid #3a3a3a;
  border-radius: 4px;
  padding: 2px 8px;
  cursor: pointer;
  font-family: inherit;
  font-size: 12px;
}
.slash-btn:hover { background: #333333; }
#restart-banner {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  background: rgba(0, 0, 0, 0.85);
}
#restart-banner[hidden] { display: none; }
#restart-btn {
  background: #252525;
  color: var(--fg);
  border: 1px solid #3a3a3a;
  border-radius: 4px;
  padding: 6px 16px;
  cursor: pointer;
  font-family: inherit;
}
```

- [ ] **Step 3: `terminal.js`**

```js
// Cablaggio della finestra terminale (piano 3): xterm.js ↔ pty (Rust,
// portable-pty/ConPTY) ↔ lare-shell.exe. Adattato dallo spike 2
// (spikes/lare-terminal-window/frontend/app.js) alle convenzioni del
// prodotto: session id dal backend (get_terminal_session), OSC/base64/
// debounce/segnalini nei moduli puri testati da soli (Task 1),
// ActivityIndicator dal WS orchestratore via host.js (Task 5).

import { base64ToUint8Array } from "./base64.mjs";
import { parseLareOsc } from "./osc-lare.mjs";
import { createIndicatorState, onIntercept, onActivity } from "./indicators.mjs";
import { createDebouncer } from "./fit-debounce.mjs";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

// Palette Campbell (Windows Terminal, profilo PowerShell) — stessa dello spike 2.
const campbellTheme = {
  background: "#0C0C0C", foreground: "#CCCCCC", cursor: "#FFFFFF",
  black: "#0C0C0C", red: "#C50F1F", green: "#13A10E", yellow: "#C19C00",
  blue: "#0037DA", magenta: "#881798", cyan: "#3A96DD", white: "#CCCCCC",
  brightBlack: "#767676", brightRed: "#E74856", brightGreen: "#16C60C",
  brightYellow: "#F9F1A5", brightBlue: "#3B78FF", brightMagenta: "#B4009E",
  brightCyan: "#61D6D6", brightWhite: "#F2F2F2",
};

const term = new Terminal({
  cursorBlink: true,
  scrollback: 5000,
  theme: campbellTheme,
  fontFamily: "Cascadia Mono, Consolas, monospace",
  fontSize: 14,
  allowProposedApi: true,
});
const fitAddon = new FitAddon.FitAddon();
term.loadAddon(fitAddon);
term.open(document.getElementById("terminal"));
fitAddon.fit();
term.focus();

let indicatorState = createIndicatorState();
const lastCommandEl = document.getElementById("last-command");
const dotEl = document.getElementById("activity-dot");
let dotOffTimer = null;

function renderIndicators() {
  lastCommandEl.textContent = "ultimo: " + (indicatorState.lastCommand ?? "—");
  dotEl.classList.toggle("on", indicatorState.aiBusy);
}

// OSC 9001 (canale diretto host→emulatore, spec §4.7): accende il pallino
// per 2s indipendentemente da ActivityIndicator, che segue l'intero turno
// /ai (Task 5), non il singolo comando intercettato.
term.parser.registerOscHandler(9001, (data) => {
  const parsed = parseLareOsc(data);
  if (parsed) {
    indicatorState = onIntercept(indicatorState, parsed.line);
    renderIndicators();
    clearTimeout(dotOffTimer);
    dotOffTimer = setTimeout(() => dotEl.classList.remove("on"), 2000);
  }
  return true;
});

let ownSessionId = null;

// ActivityIndicator (Task 5): segue l'intero turno /ai, non solo l'OSC.
listen("terminal:activity", (event) => {
  if (!ownSessionId) return;
  indicatorState = onActivity(indicatorState, event.payload.session_id, ownSessionId, event.payload.on);
  renderIndicators();
});

const clockEl = document.getElementById("clock");
function tickClock() {
  const now = new Date();
  const pad = (n) => String(n).padStart(2, "0");
  clockEl.textContent = `${pad(now.getHours())}:${pad(now.getMinutes())}:${pad(now.getSeconds())}`;
}
tickClock();
setInterval(tickClock, 1000);

// Pulsanti /help /library /aichat /config: scrivono direttamente nella pty
// (come se l'utente li avesse digitati) — nessun comando Tauri dedicato.
document.querySelectorAll(".slash-btn").forEach((btn) => {
  btn.addEventListener("click", () => {
    invoke("pty_write", { data: btn.dataset.cmd + "\r" }).catch((err) => console.error("pty_write:", err));
    term.focus();
  });
});

const restartBanner = document.getElementById("restart-banner");
const restartMessage = document.getElementById("restart-message");
const restartBtn = document.getElementById("restart-btn");

async function spawnShell() {
  restartBanner.hidden = true;
  try {
    await invoke("pty_spawn", { sessionId: ownSessionId, cols: term.cols, rows: term.rows });
  } catch (err) {
    term.write("\r\n\x1b[31m[lare] pty_spawn fallita: " + err + "\x1b[0m\r\n");
  }
}

restartBtn.addEventListener("click", () => {
  term.reset();
  spawnShell();
});

async function main() {
  ownSessionId = await invoke("get_terminal_session");
  document.getElementById("session-label").textContent = "sessione " + ownSessionId;

  await listen("pty-out", (event) => term.write(base64ToUint8Array(event.payload)));
  await listen("pty-exit", (event) => {
    term.write("\r\n\x1b[31m[lare] shell terminata (exit code: " + event.payload + ")\x1b[0m\r\n");
    restartMessage.textContent = "shell terminata (exit code " + event.payload + ")";
    restartBanner.hidden = false;
  });

  await spawnShell();

  term.onData((data) => {
    invoke("pty_write", { data }).catch((err) => console.error("pty_write:", err));
  });

  const debounced = createDebouncer(50);
  const resizeObserver = new ResizeObserver(() => {
    debounced(() => {
      fitAddon.fit();
      invoke("pty_resize", { cols: term.cols, rows: term.rows }).catch((err) => console.error("pty_resize:", err));
    });
  });
  resizeObserver.observe(document.getElementById("terminal"));
}

main();
```

- [ ] **Step 4: capability `terminal-window.json`**

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "terminal-window",
  "description": "Capability per la finestra terminale di Lare Terminal (piano 3, label: 'terminal'). core:default per l'IPC Tauri (pty_spawn/pty_write/pty_resize/get_terminal_session/close_self) e l'event API (ascolto pty-out/pty-exit/terminal:activity). Nessuna titlebar chromeless qui (decorations:true, finestra reale con la sua chrome OS, non un popup) quindi core:window:allow-start-dragging NON è incluso. Global-shortcut e clipboard NON inclusi.",
  "platforms": ["linux", "macOS", "windows"],
  "windows": ["terminal"],
  "permissions": [
    "core:default"
  ]
}
```

- [ ] **Step 5: comando `get_terminal_session` + costruzione della finestra**

In `crates/ui/src-tauri/src/main.rs`, vicino a `NoTerminal`:

```rust
/// Id di sessione (8 esadecimali, stesso formato di `ws.rs::hello.session_id`
/// lato orchestratore) della finestra terminale di questo processo — un solo
/// campo perché l'MVP ha una sola finestra terminale per processo ("una
/// finestra, una sessione", spec §5).
struct TerminalSession(String);

#[tauri::command]
fn get_terminal_session(state: tauri::State<'_, TerminalSession>) -> String {
    state.0.clone()
}
```

Aggiungi `get_terminal_session,` a `generate_handler![...]`.

Nel blocco `.setup(|app| { ... Ok(()) })`, PRIMA di `Ok(())` finale (dopo il blocco del thread
"q per uscire"):

```rust
            // ── Finestra terminale (piano 3, modalità A) ────────────────────
            // Costruita qui (non dichiarata staticamente in tauri.conf.json
            // come "main") perché è condizionale: chi avvia `ui.exe` in
            // ruolo "solo host" (`--no-terminal`, Task 3) non la vuole.
            let no_terminal = app.state::<NoTerminal>().0;
            if !no_terminal {
                let session_id = format!("{:08x}", rand::random::<u32>());
                app.manage(TerminalSession(session_id.clone()));
                tauri::WebviewWindowBuilder::new(
                    app,
                    "terminal",
                    tauri::WebviewUrl::App("terminal.html".into()),
                )
                .title("Lare Terminal")
                .inner_size(1000.0, 650.0)
                .decorations(true)
                .transparent(false)
                .resizable(true)
                .build()
                .map_err(|e| format!("finestra terminale: {e}"))?;
                println!("[ui] finestra terminale aperta (sessione {session_id})");
            }
```

- [ ] **Step 6: verifica manuale — smoke test locale**

```powershell
cargo build -p ui
cargo run -p ui -- --config-dir "Test Run\Configuration"
```

Expected (verifica dal vivo, non automatizzabile in questo step): si apre una finestra "Lare
Terminal" 1000×650 con dentro un prompt (pwsh o `lare-shell.exe`, a seconda che quest'ultimo esista
in `Test Run\shell\`); digitare `/help` scrive nel terminale; ridimensionare la finestra non fa
scomparire il testo; chiudere la finestra termina il processo `lare-shell.exe`/`pwsh.exe` figlio
(verificabile in Task Manager). Se `lare-shell.exe` non è pubblicato in `Test Run\`, esegui prima
`.\deploy_test_run.ps1`.

- [ ] **Step 7: `cargo check`/clippy + commit**

```powershell
cargo check -p ui
cargo clippy -p ui --all-targets
node --test crates/ui/frontend/*.test.mjs
```

Expected: tutto pulito.

```powershell
git add crates/ui/frontend/terminal.html crates/ui/frontend/terminal.css crates/ui/frontend/terminal.js crates/ui/src-tauri/capabilities/terminal-window.json crates/ui/src-tauri/src/main.rs
git commit -m "feat(ui): finestra terminale — xterm.js + ConPTY, OSC 9001, segnalini, bottone riavvia"
```

---

## Task 5: consumo di `ActivityIndicator` (host.js relay → `terminal:activity`)

`ActivityIndicator` è emesso da `surface.rs` dal piano 2a ma non ha mai avuto un consumatore — la
finestra terminale (Task 4) lo ascolta già come evento Tauri `terminal:activity`; questo task lo fa
arrivare fin lì, da `host.js` (che riceve il messaggio WS grezzo).

**Files:**
- Modify: `crates/ui/frontend/host-dispatch.mjs` (`activity_indicator` → `"relay"`, non più
  "ignored")
- Modify: `crates/ui/frontend/host-dispatch.test.mjs` (nuovo test)
- Modify: `crates/ui/frontend/host.js` (nuovo `case`, nuovo helper `emitToTerminal`)

**Interfaces:**
- Consumes: `msg = {type: "activity_indicator", session_id, kind, on}` (wire, da
  `protocol::ServerMsg::ActivityIndicator`).
- Produces: evento Tauri globale `terminal:activity` con payload `{session_id, kind, on}` —
  consumato da `terminal.js` (Task 4, già scritto e in ascolto).

- [ ] **Step 1: RED — `host-dispatch.test.mjs`**

Aggiungi a `crates/ui/frontend/host-dispatch.test.mjs`:

```js
test("activity_indicator è relay (piano 3, Task 5)", () => {
  assert.equal(classifyServerMsg({ type: "activity_indicator" }), "relay");
});
```

Run: `node --test crates/ui/frontend/host-dispatch.test.mjs` — Expected: FAIL (oggi ritorna
`"ignored"`, il tipo non è in nessuno dei due Set).

- [ ] **Step 2: GREEN — `host-dispatch.mjs`**

In `crates/ui/frontend/host-dispatch.mjs`, aggiungi `"activity_indicator"` al `Set` `RELAY`
(qualunque posizione, es. subito dopo la riga `"ai_chat_message",`):

```js
const RELAY = new Set([
  "ai_chat_message",
  "activity_indicator",
  // ... resto invariato
```

Run: `node --test crates/ui/frontend/host-dispatch.test.mjs` — Expected: PASS.

- [ ] **Step 3: relay in `host.js`**

In `crates/ui/frontend/host.js`, aggiungi un `case` nello switch di `handleServerMsg` (vicino agli
altri case "Task N" recenti, es. dopo il case `"notes_snapshot"`):

```js
    // ── Segnalino di stato per la finestra terminale (piano 3, Task 5) ─────
    case "activity_indicator":
      emitToTerminal("terminal:activity", {
        session_id: msg.session_id,
        kind: msg.kind,
        on: msg.on,
      });
      break;
```

Aggiungi l'helper (vicino a `emitToLibrary`/`emitToSearch`, stesso schema):

```js
function emitToTerminal(event, payload) {
  if (tauriEvent?.emit) {
    tauriEvent
      .emit(event, payload)
      .catch((e) => console.error(`[host] emit ${event} error:`, e));
  }
}
```

- [ ] **Step 4: verifica e commit**

```powershell
node --test crates/ui/frontend/*.test.mjs
```

Expected: tutti i test JS passano (inclusi i pre-esistenti — nessuna regressione sul `default` case
dello switch).

```powershell
git add crates/ui/frontend/host-dispatch.mjs crates/ui/frontend/host-dispatch.test.mjs crates/ui/frontend/host.js
git commit -m "feat(ui): consumo di ActivityIndicator — relay verso la finestra terminale"
```

---

## Task 6: autostart di `ui.exe` dall'orchestratore

Ultimo pezzo dello spec §6.4 mai implementato: quando un turno shell apre una finestra e nessuna
connessione `ui` esiste, l'orchestratore avvia `ui.exe --no-terminal` e attende fino a 10s che si
connetta, PRIMA di aprire il turno (spec §9, tabella errori). Se `ui.exe` non esiste, l'autostart è
disattivo, o i 10s scadono senza connessione, il turno prosegue come oggi (`NO_UI_ACK`, invariato —
vedi la nota di reconciliazione in Task 7 sulla dicitura esatta di §9).

**Files:**
- Modify: `crates/orchestrator/src/runtime_config.rs` (aggiunge `RuntimeConfig::ui_exe()`)
- Modify: `crates/orchestrator/src/shell_turn.rs` (nuova funzione `ensure_ui_sink` + wiring in
  `start_turn`)

**Interfaces:**
- Produces: `RuntimeConfig::ui_exe(&self) -> PathBuf`. `ensure_ui_sink(registry: &SharedRegistry,
  autostart: bool, exists: impl Fn() -> bool, spawn: impl FnOnce(), window: Duration, retry:
  Duration) -> Option<UnboundedSender<ServerMsg>>` (privato al modulo, `pub(crate)` se serve al
  test in un altro file — qui resta locale a `shell_turn.rs`).
- Consumes (Task 3): `startup_config::spawn_detached`, `startup_config::deploy_root`.

- [ ] **Step 1: `RuntimeConfig::ui_exe()`**

In `crates/orchestrator/src/runtime_config.rs`, dopo `mcp_nmap_exe()`:

```rust
    /// Percorso di `ui.exe` (piano 3, autostart §6.4): non è in `paths.*`
    /// (a differenza di `mcp_server`/`mcp_nmap`) — vive sempre alla radice
    /// del deploy, come `orchestrator.exe`, mai spostabile via `startup.json`.
    pub fn ui_exe(&self) -> PathBuf {
        startup_config::deploy_root(&self.config_dir).join("ui.exe")
    }
```

- [ ] **Step 2: RED — test di `ensure_ui_sink` in `shell_turn.rs`**

Aggiungi al modulo test di `crates/orchestrator/src/shell_turn.rs` (o crealo se non esiste —
verifica con `grep -n "mod tests" crates/orchestrator/src/shell_turn.rs`):

```rust
    #[tokio::test]
    async fn ensure_ui_sink_ritorna_subito_se_gia_registrato() {
        let registry = crate::connections::Registry::shared();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<protocol::ServerMsg>();
        registry.lock().await.set_ui_sink(tx);
        let mut spawned = false;
        let sink = ensure_ui_sink(
            &registry,
            true,
            || true,
            || spawned = true,
            std::time::Duration::from_millis(50),
            std::time::Duration::from_millis(10),
        )
        .await;
        assert!(sink.is_some());
        assert!(!spawned);
    }

    #[tokio::test]
    async fn ensure_ui_sink_non_avvia_nulla_se_autostart_disattivo() {
        let registry = crate::connections::Registry::shared();
        let mut spawned = false;
        let sink = ensure_ui_sink(
            &registry,
            false,
            || true,
            || spawned = true,
            std::time::Duration::from_millis(50),
            std::time::Duration::from_millis(10),
        )
        .await;
        assert!(sink.is_none());
        assert!(!spawned);
    }

    #[tokio::test]
    async fn ensure_ui_sink_non_avvia_nulla_se_leseguibile_non_esiste() {
        let registry = crate::connections::Registry::shared();
        let mut spawned = false;
        let sink = ensure_ui_sink(
            &registry,
            true,
            || false,
            || spawned = true,
            std::time::Duration::from_millis(50),
            std::time::Duration::from_millis(10),
        )
        .await;
        assert!(sink.is_none());
        assert!(!spawned);
    }

    #[tokio::test]
    async fn ensure_ui_sink_avvia_e_trova_il_sink_comparso_durante_lattesa() {
        let registry = crate::connections::Registry::shared();
        let mut spawned = false;
        let registry_clone = registry.clone();
        // Simula `ui.exe` che si connette 30ms dopo l'avvio (la finestra di
        // attesa del test è 200ms, retry 10ms — 30ms sta comodamente dentro).
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<protocol::ServerMsg>();
            registry_clone.lock().await.set_ui_sink(tx);
        });
        let sink = ensure_ui_sink(
            &registry,
            true,
            || true,
            || spawned = true,
            std::time::Duration::from_millis(200),
            std::time::Duration::from_millis(10),
        )
        .await;
        assert!(sink.is_some());
        assert!(spawned);
    }
```

Se `shell_turn.rs` non ha ancora un `#[cfg(test)] mod tests { use super::*; ... }`, crealo in fondo
al file con quell'intestazione prima di incollare i test sopra.

Run: `cargo test -p orchestrator ensure_ui_sink` — Expected: FAIL (`ensure_ui_sink` non esiste).

- [ ] **Step 3: GREEN — `ensure_ui_sink`**

Aggiungi in `crates/orchestrator/src/shell_turn.rs`, fuori dal modulo test (vicino a `start_turn`):

```rust
/// Finestra di attesa per l'autostart di `ui.exe` (spec §9: "attesa 10 s").
const UI_AUTOSTART_WINDOW: Duration = Duration::from_secs(10);
const UI_AUTOSTART_RETRY: Duration = Duration::from_millis(250);

/// Se non c'è un sink `ui` registrato e `autostart` è attivo, avvia `ui.exe`
/// (staccato) e attende fino a `window` che una connessione `ui` compaia nel
/// registro, ripetendo il controllo ogni `retry`. `exists`/`spawn` sono
/// l'unico punto che tocca filesystem/processi: nei test sono fake che non
/// avviano nulla di reale — stesso schema di `launcher::ensure_orchestrator`
/// lato `ui.exe` (composizione, non un mock del registro).
async fn ensure_ui_sink(
    registry: &SharedRegistry,
    autostart: bool,
    exists: impl Fn() -> bool,
    spawn: impl FnOnce(),
    window: Duration,
    retry: Duration,
) -> Option<UnboundedSender<ServerMsg>> {
    if let Some(sink) = registry.lock().await.ui_sink() {
        return Some(sink);
    }
    if !autostart || !exists() {
        return None;
    }
    spawn();
    let deadline = Instant::now() + window;
    while Instant::now() < deadline {
        tokio::time::sleep(retry).await;
        if let Some(sink) = registry.lock().await.ui_sink() {
            return Some(sink);
        }
    }
    None
}
```

Run: `cargo test -p orchestrator ensure_ui_sink` — Expected: PASS (tutti e 4 i test).

- [ ] **Step 4: wiring in `start_turn`**

Sostituisci in `crates/orchestrator/src/shell_turn.rs`:

```rust
    let ui = deps.registry.lock().await.ui_sink();
    tokio::spawn(route_shell_turn(turn_rx, deps.out_tx.clone(), ui, turn));
```

con:

```rust
    // Autostart di ui.exe (spec §6.4/§9) se nessuna connessione ui esiste
    // ancora: senza, questo turno aprirebbe una finestra che nessuno può
    // vedere finché ui.exe non parte da solo (self-heal lato ui.exe, Task 3
    // — ma quello parte SOLO se un utente avvia ui.exe direttamente).
    let ui_exe = deps.rt.ui_exe();
    let config_dir = deps.rt.config_dir.clone();
    let ui = ensure_ui_sink(
        &deps.registry,
        deps.rt.startup.autostart.ui,
        || ui_exe.exists(),
        || {
            let args = vec![
                "--config-dir".to_string(),
                config_dir.to_string_lossy().into_owned(),
                "--no-terminal".to_string(),
            ];
            if let Err(e) = startup_config::spawn_detached(&ui_exe, &args) {
                tracing::warn!("autostart ui.exe fallito: {e}");
            }
        },
        UI_AUTOSTART_WINDOW,
        UI_AUTOSTART_RETRY,
    )
    .await;
    tokio::spawn(route_shell_turn(turn_rx, deps.out_tx.clone(), ui, turn));
```

Aggiungi `use std::time::{Duration, Instant};` se non già importato (verifica: il file importa già
`std::time::{Duration, Instant}` alla riga 19 — nessuna modifica agli import necessaria).

- [ ] **Step 5: verifica che i test esistenti non rallentino**

```powershell
cargo test -p orchestrator
```

Expected: tutti verdi, **senza** un'attesa di 10s reali in nessun test — verificalo cronometrando:
`Measure-Command { cargo test -p orchestrator shell_turn:: }` deve restare nell'ordine dei
millisecondi/pochi secondi (i test esistenti che passano per `start_turn` non hanno un `ui.exe`
reale su disco nella loro `config_dir` di test, quindi `exists()` è `false` e `ensure_ui_sink`
ritorna `None` immediatamente, senza attendere).

- [ ] **Step 6: `cargo clippy` + commit**

```powershell
cargo clippy -p orchestrator --all-targets
```

Expected: nessun nuovo warning.

```powershell
git add crates/orchestrator/src/runtime_config.rs crates/orchestrator/src/shell_turn.rs
git commit -m "feat(orchestrator): autostart di ui.exe quando manca il sink (spec §6.4/§9)"
```

---

## Task 7: documentazione e rilascio

Chiude il piano: e2e manuale in modalità A, sezione "Modalità A" in RUN-LOCAL, ADR-020, bump
versioni, riconciliazione della dicitura di §9 con il comportamento reale (Task 6), aggiornamento
di HANDOFF/CHANGELOG/IMPLEMENTATION.

**Files:**
- Modify: `Docs/i18n/ita/TESTING-e2e.md` (nuova "Parte 7 — modalità A")
- Modify: `Docs/i18n/ita/RUN-LOCAL.md` (nuova sezione "Modalità A")
- Create: `Docs/i18n/ita/06-decisions.md` (nuovo ADR-020 — o l'aggiunge se il file è un log
  cumulativo, verifica il formato degli ADR precedenti prima di scrivere)
- Modify: `Docs/i18n/ita/superpowers/specs/2026-09-04-lare-terminal-2-design.md` (§9 riga
  "Nessuna connessione ui...", §2.3/§6.4 nota sul flag `--no-terminal`)
- Modify: `crates/ui/CHANGELOG.md`, `crates/ui/IMPLEMENTATION.md` (bump `2.1.0` → `2.2.0`,
  `crates/ui/src-tauri/Cargo.toml` versione, `crates/ui/src-tauri/tauri.conf.json` versione)
- Modify: `crates/orchestrator/CHANGELOG.md`, `crates/orchestrator/IMPLEMENTATION.md` (bump
  `2.1.0` → `2.2.0`, `crates/orchestrator/Cargo.toml` versione)
- Modify: `Docs/i18n/ita/HANDOFF.md` (stato piano 3 completato, rimuove/aggiorna le voci "DA FARE"
  toccate da questo piano)

**Interfaces:**
- Nessuna — solo documentazione, nessun codice.

- [ ] **Step 1: riconcilia §9 dello spec con il comportamento reale (Task 6)**

In `Docs/i18n/ita/superpowers/specs/2026-09-04-lare-terminal-2-design.md`, tabella §9, la riga:

```markdown
| Nessuna connessione `ui` per un messaggio "finestra" | Autostart `ui.exe`, attesa 10 s, poi `Error` al mittente ("finestra non disponibile") |
```

diventa (il "poi" è l'unica parte che cambia — l'attesa 10s e l'autostart sono implementati esatti
come descritto):

```markdown
| Nessuna connessione `ui` per un messaggio "finestra" | Autostart `ui.exe --no-terminal`, attesa fino a 10 s per il sink; se non compare, il turno prosegue con `NO_UI_ACK` come già oggi (nessun `Error` dedicato — comportamento consolidato piano 2a/2b, non modificato dal piano 3) |
```

- [ ] **Step 2: nota su `--no-terminal` in §2.3/§6.4**

In §6.4, dopo il punto sui "Processi staccati", aggiungi:

```markdown
- **`--no-terminal`** (piano 3): chi avvia `ui.exe` per il solo ruolo host (self-heal della host
  C# in modalità B, autostart dell'orchestratore) passa sempre questo flag — senza, ogni self-heal
  aprirebbe anche una finestra terminale non voluta. Assente = modalità A (finestra terminale
  all'avvio, uso interattivo diretto).
```

- [ ] **Step 3: sezione "Modalità A" in RUN-LOCAL.md**

Aggiungi in `Docs/i18n/ita/RUN-LOCAL.md`, dove prima c'era la sezione "Flag di sviluppo `--open`"
(rimossa in Task 0), una sezione con lo stesso stile della sezione "Host C#" esistente:

```markdown
### Modalità A (finestra terminale)

`ui.exe`, avviato SENZA `--no-terminal`, apre la finestra terminale all'avvio: xterm.js dentro
ConPTY, `lare-shell.exe` come processo figlio (risolto da `startup.json` → `paths.shell`).

**Avvio da sorgente:**

​```powershell
cargo run -p ui -- --config-dir "Test Run\Configuration"
​```

Se `Test Run\shell\lare-shell.exe` non esiste, esegui prima `.\deploy_test_run.ps1` (pubblica anche
la host C#). Chiudere la finestra termina il processo `lare-shell.exe` figlio — nessun processo
residuo (verificabile in Task Manager).

**Modalità B** (`ui.exe --no-terminal`, avviato dalla host C# o dall'orchestratore): apre solo la
host page nascosta, mai la finestra terminale — usata quando la shell "vera" è già
`lare-shell.exe` in una scheda Windows Terminal (profilo "Lare Terminal", vedi sezione "Host C#"
sopra).
```

- [ ] **Step 4: "Parte 7" in TESTING-e2e.md**

Aggiungi in `Docs/i18n/ita/TESTING-e2e.md`, seguendo lo stile delle Parti precedenti (tabella
passo/esito), una "Parte 7 — modalità A (piano 3)" che copre la checklist di spec §10: apertura
`ui.exe` → finestra terminale con la shell dentro; `/ping`; `/calc`; `/config`/`/library`/`/help`
(pulsanti E digitati); `/ai "…"` → gate `[Y/n]` → esegue → finestra Markdown col risultato;
segnalino OSC 9001 si accende dopo un comando; segnalino ActivityIndicator si accende durante un
turno `/ai` lungo; `/aichat` due volte → una sola finestra; chiusura finestra → nessun processo
residuo; kill di `orchestrator.exe` prima dell'avvio → self-heal lo riavvia (Task 3); avvio di
`ui.exe` senza orchestratore E senza autostart → messaggio "non raggiungibile", shell resta
utilizzabile. Esegui questa parte dal vivo (non è automatizzabile) e registra gli esiti reali,
non presunti — stesso principio delle Parti precedenti.

- [ ] **Step 5: ADR-020**

Aggiungi in `Docs/i18n/ita/06-decisions.md` (verifica il formato esatto delle voci precedenti —
ADR-019 in poi — prima di scrivere, per restare coerente):

```markdown
## ADR-020: finestra terminale — `--no-terminal` come ruolo esplicito di `ui.exe`

**Contesto**: piano 3 introduce una finestra terminale che `ui.exe` apre di default all'avvio.
Ma `ui.exe` viene avviato anche da chi ha già una shell (self-heal della host C# in modalità B,
autostart dell'orchestratore per aprire una finestra di output) — per loro una seconda finestra
terminale sarebbe un bug visibile, non una funzionalità.

**Decisione**: un flag esplicito, `--no-terminal`, distingue i due ruoli invece di un'euristica
(es. "c'è già un lare-shell.exe in esecuzione?", fragile e con finestre di gara). Chi avvia
`ui.exe` per il solo ruolo host lo passa sempre; la sua assenza è la modalità A.

**Conseguenze**: `Launcher.EnsureUi()` (C#, `lare-shell` 2.0.1) e l'autostart dell'orchestratore
(Task 6) lo passano entrambi. Un utente che avvia `ui.exe` a mano senza il flag ottiene sempre la
finestra terminale — comportamento di default intenzionale (modalità A è l'uso interattivo
"normale" del prodotto).
```

- [ ] **Step 6: bump versioni**

`crates/ui/src-tauri/Cargo.toml` riga `version`: `"2.1.0"` → `"2.2.0"`.
`crates/ui/src-tauri/tauri.conf.json` riga `"version"`: `"2.1.0"` → `"2.2.0"`.
`crates/orchestrator/Cargo.toml` riga `version`: `"2.1.0"` → `"2.2.0"`.

In `crates/ui/CHANGELOG.md` e `crates/orchestrator/CHANGELOG.md`, sposta `[Unreleased]` a
`## [2.2.0] - 2026-09-07` (o la data reale di merge) con le voci accumulate nei task precedenti
(finestra terminale, self-heal Rust, autostart `ui.exe`, `--no-terminal`, ecc. — riassunte, non
ripetute parola per parola dai singoli commit).

Aggiorna `crates/ui/IMPLEMENTATION.md` e `crates/orchestrator/IMPLEMENTATION.md` con una sezione
breve su `pty.rs`/`launcher.rs`/`ensure_ui_sink` (dove vivono, cosa fanno, perché sono testabili
senza rete/Tauri/processi reali — stesso livello di dettaglio delle sezioni esistenti).

- [ ] **Step 7: HANDOFF.md**

In `Docs/i18n/ita/HANDOFF.md`: sposta la descrizione "piano 3" da "DA FARE" a "FATTO" (con
versioni/commit), rimuovi la nota "prerequisito piano 3" ormai risolta (già chiusa da `bb4b3d1`
prima di questo piano), e — se presente — la voce sullo streaming token-per-token sotto "piano 3"
va corretta: questo piano lo esclude esplicitamente (Global Constraints sopra); se HANDOFF lo
elencava come parte di piano 3, riformulala come una voce "DA FARE" a sé, non più legata a questo
piano.

- [ ] **Step 8: verifica finale completa + commit**

```powershell
cargo build
cargo test
node --test crates/ui/frontend/*.test.mjs
cargo clippy --all-targets
cargo fmt --check
dotnet test shell/lare-shell/LareShell.sln
```

Expected: tutto verde, `cargo fmt --check` pulito (o esegui `cargo fmt` se emerge un diff).

```powershell
git add -A
git commit -m "docs(piano 3): e2e modalità A, ADR-020, versioni 2.2.0/2.2.0/2.0.1, HANDOFF aggiornato"
```

---

## Self-Review

**1. Copertura spec** — §2.3 (modalità A/B, ConPTY): Task 2/4. §5 (finestra terminale, xterm.js,
singleton, chiusura → nessun processo residuo): Task 2/4. §6.4 (self-heal, autostart, processi
staccati): Task 3/6. §9 (tabella errori — riga "finestra non disponibile"): Task 6, riconciliata in
Task 7 Step 1. §10 (test C#/Rust/JS per piano 3, e2e modalità A): Task 1/2/3/6 per gli
automatici, Task 7 Step 4 per l'e2e manuale. §12 (fuori MVP: streaming, multi-finestra, tray):
escluso esplicitamente nei Global Constraints — nessun task lo tocca.

**2. Scan placeholder** — nessun "TBD"/"da implementare dopo"/codice non mostrato: ogni step ha
codice reale (verificato contro i file esistenti dall'inventario Explore) o un comando shell
eseguibile con il suo output atteso.

**3. Coerenza dei tipi** — `PtyOutputSink` (Task 2) è lo stesso trait usato da `AppHandleSink`
(Task 2, `main.rs`) e dal `FakeSink` di test; `SharedPtyState` è lo stesso tipo passato da
`pty_spawn`/`pty_write`/`pty_resize` (Task 2) e da `PtyState` (Task 2, `main.rs`); `ensure_ui_sink`
(Task 6) e `ensure_orchestrator` (Task 3) condividono lo stesso schema di firma (`connect`/`exists`
+ `spawn` iniettati) pur vivendo in crate diversi — deliberato, non un'incoerenza: sono due
implementazioni dello stesso pattern, non la stessa funzione. `--no-terminal` è letto in Task 3
(`main.rs`, ui) e generato in Task 3 (`Launcher.cs`, lare-shell) e Task 6 (`shell_turn.rs`,
orchestrator) — stessa stringa letterale nei tre punti, verificata riga per riga sopra.
