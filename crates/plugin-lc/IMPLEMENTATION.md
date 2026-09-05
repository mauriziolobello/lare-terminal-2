# IMPLEMENTATION — plugin-lc

**Version:** 0.3.5 (round 6: alpha `.lc-root` allineato a 0.87 — lo standard di TUTTA l'app, non più
solo della shell `plugin-window.html` — stesso giro ha allineato anche index.html/window.html/
window-search.html/library.html/config.html+config.css/aichat-window.html/plugin-catalog.css, vedi
`Docs/HANDOFF.md`. Su base 0.3.4 round 5 `min-height: 0` scroll-dentro-il-pannello + Home/End,
0.3.3 `.lc-root { height: 100% }` + focus MkDir + Elimina, 0.3.2 ESC/scroll/resize/unità disco,
0.3.1 tastiera/doppio-click/layout, 0.3.0 data-loss/traversal/persistenza)
**Binary:** `lc` (discovered as `plugins/lc/lc.exe` on Windows)
**Role:** File manager a doppio pannello in stile Midnight Commander. Attivato via `/lc`. Pannelli navigabili indipendentemente; operazioni F5 Copia, F6 Sposta, F7 MkDir, F8 Confronta/Diff, Del Elimina.

---

## Files

| File | Purpose |
|---|---|
| `Cargo.toml` | `[[bin]] name="lc"`, deps: `plugin-protocol` + `serde_json` |
| `plugin.json` | Manifest statico pulito: `id="lc"`, `version`, `triggers.command="/lc"` (nessun campo di stato runtime) |
| `src/config.rs` | Lettura/scrittura path persistenti in `storage_dir/config.json` |
| `src/fs.rs` | Lettura directory, ordinamento (dir prima, case-insensitive), `parent_of` |
| `src/ops.rs` | Operazioni file (copy, move, mkdir, delete) + guardie difensive + compare listing + folder_compare + diff_files |
| `src/state.rs` | `LcState`, `PanelState`, `Dialog`, `Side`; handler puro `handle()`; guardie `..`/traversal; `save_config` |
| `src/render.rs` | Genera HTML completo della finestra (pannelli + dialog + CSS inline `lc-*`) |
| `src/main.rs` | Loop stdin JSON (Contratto P); cattura `storage_dir` dall'`Init`; gestisce Init/Activate/Deinit |

---

## Architecture

Il crate è organizzato in moduli puri (testabili in isolamento) + il binario:

```
config.rs  →  LcConfig::load(&storage_dir)          // legge left_path/right_path da storage_dir/config.json
               LcConfig::save(&self, &storage_dir)  // scrive SOLO i 2 path + create_dir_all
fs.rs      →  list_dir(path) -> Result<Vec<Entry>>  // listing ordinato (dir prima)
               parent_of(path) -> PathBuf
ops.rs     →  compare(left, right) -> CompareResult           // puro, su Vec<Entry>
               folder_compare(left, right) -> FolderCompareResult // I/O, con mod time
               diff_files(left, right) -> FileDiffResult       // LCS diff su righe
               copy_item / move_item / mkdir                   // I/O
state.rs   →  LcState + handle(msg) -> Vec<PluginToHost>       // puro (tranne ops I/O)
render.rs  →  render_window(&LcState) -> String                // puro
main.rs    →  loop stdin: Init → Ready; Activate → ShowWindow; UiEvent → UpdateWindow
```

---

## config.rs

Legge e scrive `left_path`/`right_path` nel file **`config.json`** dentro la directory di
storage privata del plugin (`storage_dir`), fornita dall'host nel messaggio `Init` come
`storage_root.join(&manifest.id)` — es. `%LOCALAPPDATA%\dev.lare.terminal\plugin-storage\lc\`.
Questo file è di **proprietà esclusiva del plugin** (nulla nell'orchestrator lo legge o scrive),
quindi contiene solo i due campi e non serve alcun merge. `save` fa `create_dir_all(storage_dir)`
perché l'host **non** garantisce che la dir esista già.

> ⚠️ Perché NON `plugin.json`: quel file è il **manifest statico** che la discovery
> (`orchestrator/src/plugins/discovery.rs`) parsa e che la tab "Plugins" della Library
> mostra all'utente. Scriverci lo stato runtime violava la separazione manifest/stato
> (`Docs/18-plugin-system.md`).

**Politica di persistenza (attraversa i riavvii):**

| Evento | Azione |
|---|---|
| Avvio orchestratore (`Init`) | Cattura `storage_dir`. **Nessun reset**: lo stato salvato sopravvive al riavvio |
| Apertura `/lc` (`Activate`) | `LcConfig::load(&storage_dir)` — ripristina i path salvati |
| Navigazione (Enter/Backspace) | `state::save_config` — salva **subito** i path correnti |
| Chiusura orchestratore (`Deinit`) | `LcConfig::save` — rete di sicurezza (l'host invia `Deinit` solo allo shutdown, non alla chiusura finestra) |

**Perché save-on-navigate e non save-on-close:** l'host **non** invia `Deinit` alla chiusura
della finestra `/lc` (quella è `ClientMsg::PluginWindowClosed` → `forget_window`, senza `Deinit`
verso il processo plugin). Non esiste quindi un segnale di "finestra chiusa" che raggiunga il
plugin; salvare ad ogni cambio di path è il sostituto funzionale e garantisce che l'ultimo path
visitato sia sempre quello ripristinato.

**Resilienza:** Se lo storage non è raggiungibile, tutte le operazioni falliscono
silenziosamente. Il plugin funziona sempre con i valori di default (home).

---

## fs.rs

### `Entry`

```rust
pub struct Entry {
    pub name:   String,
    pub is_dir: bool,
    pub size:   u64,    // 0 per le directory
}
impl Entry {
    pub fn label(&self) -> String  // aggiunge "/" alle directory
}
```

### `list_dir(path: &Path) -> io::Result<Vec<Entry>>`

0. **Se `path` è la sentinella "elenco unità"** (`drive_list_sentinel()`) → ritorna
   `list_drives()` (nessun `read_dir`, nessun `..`: è la cima assoluta).
1. Legge la directory con `std::fs::read_dir`
2. Separa directory e file
3. Ordina entrambe le liste case-insensitive (`.to_lowercase()`)
4. Prepende `..` se **esiste un livello superiore**: `path.parent().is_some()` **oppure**
   `is_drive_root(path)` (la radice di un'unità ha `parent() == None` ma un livello superiore
   concettuale — l'elenco unità).
5. Restituisce: `[.., dirs..., files...]`

### `parent_of(path: &Path) -> PathBuf`

- `is_drive_root(path)` (es. "C:\") → `drive_list_sentinel()` ("su" = elenco unità).
- `path == drive_list_sentinel()` → invariato (cima assoluta).
- altrimenti: il genitore, o `path` stesso se è già la root.

### Navigazione fra unità disco (Windows) — `drive_list_sentinel` / `is_drive_root` / `list_drives`

- **`drive_list_sentinel() -> PathBuf`** = `PathBuf::new()` (stringa vuota): path sentinella
  che rappresenta la vista "elenco unità disco", non un percorso reale (non collide mai con
  un path valido).
- **`is_drive_root(&Path) -> bool`** (`#[cfg(windows)]`): `true` per "X:\", "X:/", "X:"
  (X = lettera ASCII); `false` per sottodir, UNC, path relativi, vuoto. Non-Windows: sempre
  `false` (nessun concetto di unità multiple → comportamento invariato lì).
- **`list_drives() -> Vec<Entry>`** (interno, `#[cfg(windows)]`): sonda A:..Z: con
  `Path::new("X:\").exists()`, ritorna una `Entry { name: "X:\", is_dir: true }` per ognuna
  esistente. Non-Windows: vuoto. Nessuna dipendenza aggiuntiva.
- **Round-trip**: "C:\" → ".." → `parent_of` → sentinella → `list_dir` → elenco unità →
  selezione "C:\" → `enter_selected` fa `PathBuf::new().join("C:\") == "C:\"` → radice unità.
  `enter_selected` è **invariato**: gestisce la sentinella senza casi speciali.

---

## ops.rs

### `CompareResult` / `compare` (puro, su listing in memoria)

Confronto per nome tra due `Vec<Entry>`. Restituisce `only_left`, `only_right`, `common`.
Usato internamente e nei test di unità.

### `FolderCompareResult` / `folder_compare` (I/O, con date)

```rust
pub struct FolderCompareResult {
    pub only_left:   Vec<String>,  // presenti solo a sinistra
    pub only_right:  Vec<String>,  // presenti solo a destra
    pub newer_left:  Vec<String>,  // in entrambi, ma più recente a sinistra
    pub newer_right: Vec<String>,  // in entrambi, ma più recente a destra
    pub identical:   Vec<String>,  // stessa data/ora (o entrambi dir)
}
```

`folder_compare(left: &Path, right: &Path) -> io::Result<FolderCompareResult>`
→ legge `std::fs::metadata` per confrontare `modified()`.
→ Per le directory: sempre `identical` (non confronta la data).

### `DiffLine` / `FileDiffResult` / `diff_files`

```rust
pub enum DiffLine {
    Same  { line: String },
    Left  { line: String },   // solo nel file sinistro
    Right { line: String },   // solo nel file destro
}

pub struct FileDiffResult {
    pub left_name:  String,
    pub right_name: String,
    pub lines:      Vec<DiffLine>,
    pub left_only:  usize,
    pub right_only: usize,
    pub same:       usize,
    pub truncated:  bool,      // true se troncato a MAX_DIFF_LINES
}
```

`diff_files(left: &Path, right: &Path) -> io::Result<FileDiffResult>`

- Limite: 512 KB per file, max 1000 righe per lato per l'LCS
- Rifiuta file binari (byte NUL)
- Algoritmo: **LCS (Longest Common Subsequence)** su righe
  - Tabella DP `(m+1)×(n+1)` in `u16` (max LCS = 1000 < 65535)
  - Complessità: O(m×n) tempo e spazio, con m,n ≤ 1000 → max ~8 MB
  - Backtracking per ricostruire la sequenza Same/Left/Right

### `copy_item` / `move_item` + guardie difensive

`validate_src_dst(src, dst)` è chiamata all'inizio di **entrambe** (in `move_item`
esplicitamente, perché il percorso veloce `rename` non passa da `copy_item`):

1. `src.file_name().is_none()` → `Err(InvalidInput)`. Un path che termina in `..` (o una
   radice) non ha nome: operarci significherebbe agire sull'intera directory padre — è il
   footgun che cancellava dati con F5/F6 su "..".
2. `dst.starts_with(src)` → `Err(InvalidInput)`. Impedisce di copiare/spostare una cartella
   dentro se stessa (ricorsione infinita). `starts_with` confronta per **componenti**, quindi
   `/a/dir2` NON è "dentro" `/a/dir`.

`move_item` mantiene il fallback cross-device (`rename` → in caso di errore `copy_item` +
`remove_dir_all`/`remove_file`).

### `delete_item` (round 4)

`delete_item(path: &Path) -> io::Result<()>`

- **file** → `std::fs::remove_file`.
- **directory vuota** → `std::fs::remove_dir`.
- **directory NON vuota** → `Err(io::Error::other("la cartella non è vuota — eliminazione ricorsiva
  non supportata"))`. La non-vuotezza è rilevata con `read_dir(path)?.next().is_some()` (legge solo la
  prima voce, non l'intero listing) **prima** di `remove_dir`, per dare un messaggio chiaro in italiano
  invece dell'errore OS grezzo. **Nessuna ricorsione** (`remove_dir`, non `remove_dir_all`): eliminare un
  intero albero da un singolo tasto è troppo distruttivo (scelta esplicita del supervisore). `Error::other`
  è la forma idiomatica per `ErrorKind::Other` (allineata a `config.rs` dopo la pulizia clippy di round 3).

---

## state.rs

### Tipi principali

```rust
pub enum Side { Left, Right }

pub struct PanelState {
    pub path:    PathBuf,       // può essere la sentinella "elenco unità" (vuota)
    pub entries: Vec<Entry>,
    pub cursor:  usize,
    pub error:   Option<String>,
}
// `scroll` RIMOSSO in 0.3.2: esisteva solo per la finestratura server-side (ora la
// lista è renderizzata per intero e lo scroll è nativo del browser). `move_cursor`
// sposta solo il cursore; l'host porta in vista la selezione via `scrollIntoView`.

pub enum Dialog {
    ConfirmCopy   { src: PathBuf, dst: PathBuf, overwrite: bool }, // overwrite = dst.exists()
    ConfirmMove   { src: PathBuf, dst: PathBuf, overwrite: bool },
    ConfirmDelete { path: PathBuf, is_dir: bool },                 // un solo path, niente dst
    MkdirInput    { name: String },
    FolderCompare { left_path: PathBuf, right_path: PathBuf, result: ops::FolderCompareResult },
    FileDiff      { diff: ops::FileDiffResult },
    Error         { msg: String },
}

pub struct LcState {
    pub left:        PanelState,
    pub right:       PanelState,
    pub active:      Side,
    pub dialog:      Option<Dialog>,
    pub window_id:   u64,
    pub storage_dir: PathBuf,   // per config save/load (config.json)
}
```

### Helper privati (state.rs)

- `selected_is_dotdot(state) -> bool` — guardia F5/F6/Delete: la voce ".." non è copiabile/spostabile/eliminabile.
- `save_config(state)` — salva `left`/`right` in `storage_dir/config.json` (fail-silent).
- `is_safe_component_name(name) -> bool` — guardia F7: rifiuta vuoto/`.`/`..`/`/`/`\`/`:`
  (blocca path traversal e path assoluti; accetta spazi e unicode).

### `handle(state, msg) -> Vec<PluginToHost>`

| Messaggio | Azione |
|---|---|
| `UiEvent` | Delega a `handle_ui_event` |
| `Deinit` | Salva config (`LcConfig::save`) + `vec![]` |
| `Activate` / `Init` | Gestiti in `main.rs` (Activate) o ignorati (Init) |

### Mappatura eventi UI

| `element_id` | Azione |
|---|---|
| `key:ArrowUp/Down` | Muovi cursore ±1 (`move_cursor`, relativo) |
| `key:PageUp/Down` | Muovi cursore ±10 (`move_cursor`, relativo) |
| `key:Home` | Salta alla prima voce (`cursor_to_start`, assoluto) |
| `key:End` | Salta all'ultima voce (`cursor_to_end`, assoluto; `saturating_sub` su lista vuota) |
| `key:Enter` | Entra in directory; **su successo `save_config`** |
| `key:Backspace` | Sali di un livello; **su successo `save_config`** |
| `key:Tab` | Cambia pannello attivo |
| `key:F5` | Se selezione = ".." → `Dialog::Error`; altrimenti `ConfirmCopy` (`overwrite = dst.exists()`) |
| `key:F6` | Se selezione = ".." → `Dialog::Error`; altrimenti `ConfirmMove` (`overwrite = dst.exists()`) |
| `key:F7` | Dialog MkdirInput |
| `key:F8` | se entrambi file → FileDiff; altrimenti → FolderCompare |
| `key:Delete` | Se selezione = ".." → `Dialog::Error`; altrimenti `ConfirmDelete { path, is_dir }` |
| `key:Escape` (con dialog aperto) | Chiudi il dialog (in `handle_dialog_event`) |
| `key:Escape` (top level, nessun dialog) | `PluginToHost::CloseWindow { window_id }` (chiude la finestra) |
| `panel:{side}:row:{n}` | Seleziona riga n, attiva pannello |
| `panel:{side}:focus` | Attiva pannello |
| `dialog:confirm:yes` (mkdir) | Valida con `is_safe_component_name`; se invalido → `Dialog::Error`; altrimenti `ops::mkdir` |
| `dialog:confirm:yes` (delete) | `ops::delete_item`; su `Ok` refresh di entrambi i pannelli, su `Err` → `Dialog::Error` |
| `dialog:confirm:yes/no` | Esegui / annulla operazione |
| `dialog:mkdir:input` | Aggiorna nome in MkdirInput |

---

## render.rs

`render_window(&LcState) -> String` genera l'intero HTML della finestra.
Il CSS è **inline** (costante `INLINE_CSS`) con classi `lc-*`.

### Layout principale

```
.lare-window.lc-root [data-no-autofit]       (width:100%, height:100%, min-width:640px, min-height:520px, font Consolas, bg rgba translucido)
  <style>INLINE_CSS</style>
  .lc-keytrap × 9 (display:none)             (data-key/data-evt: Arrow*/Page*/Tab/Enter/Backspace/Home/End)
  [dialog overlay se dialog.is_some()]
  .lc-panels
    .lc-panel[.lc-panel--active]   × 2
      .lc-panel-header (path, data-evt=panel:side:focus)   (path vuoto per la vista "elenco unità")
      .lc-rows  (overflow-y:auto → scroll nativo)
        .lc-row[.lc-row--selected][aria-selected] × TUTTE le voci (niente cap server-side)
          data-evt=panel:side:row:n  +  data-dblevt=key:Enter   (doppio-click = Enter)
          la riga selezionata del pannello attivo porta aria-selected="true" (aggancio scrollIntoView host)
          📁/📄 + .lc-name + .lc-size
      .lc-scroll ("N elementi": conteggio voci reali, esclude "..")
  .lc-funcbar
    button × 6 (F5 Copia · F6 Sposta · F7 MkDir · F8 Confronta · Del Elimina · ESC Chiudi)  (ciascuno data-key + data-evt)
```

**`min-height: 0` sulla catena flex (0.3.4).** Anche con `.lc-root { height: 100% }` (round 4) lo scroll
DENTRO il pannello non partiva: la lista allungava `.lc-rows` → `.lc-panel` → `.lc-panels`, spingendo la
funcbar fuori vista. Root cause: ogni flex item ha `min-height: auto` di default (= "alto almeno quanto il
contenuto"), che vince silenziosamente su `flex: 1` (non rimpicciolisce sotto il contenuto) e su
`overflow-y: auto` (cresce invece di clippare). Aggiunto `min-height: 0` a **tutti e tre** gli anelli
`.lc-panels` → `.lc-panel` → `.lc-rows`: un fix parziale su un solo anello non basta (il vincolo torna a
propagarsi verso il basso). Stessa CLASSE del fix di round 4 (dimensionamento box-model in flex) ma
meccanismo DIVERSO (default `min-height: auto` dei flex item a valle, non un'altezza percentuale mancante a
monte). `.lc-root` invariato. Da confermare visivamente dal supervisore (no browser lato agente).

**Tasti Home/End (0.3.4).** `render_keytraps` emette ora **9** keytrap (i 7 precedenti + `Home`/`End`,
`KeyboardEvent.key` = "Home"/"End"). `handle_ui_event` ha i match arm `key:Home`/`key:End` che delegano a
`PanelState::cursor_to_start` (cursore → 0) e `cursor_to_end` (cursore → `entries.len().saturating_sub(1)`,
niente panic su lista vuota). Salto ASSOLUTO, distinto dal `move_cursor` relativo di frecce/PageUp/Down
(invariato).

**`.lc-root { height: 100% }` (0.3.3).** `#plugin-root` (chrome host, `plugin-window.html`) è esso stesso
uno scroll container (`flex:1; overflow-y:auto; padding:14px 16px`). Senza `height: 100%`, `.lc-root` si
dimensiona sul PROPRIO contenuto: (a) il resize verticale della finestra non si propaga ai pannelli, (b) con
molte righe `.lc-root` sfora `#plugin-root` e la scrollbar compare attorno all'INTERO plugin (livello host)
invece che dentro `.lc-rows`. Con un'altezza reale e limitata, la catena flex-column `.lc-panels flex:1` →
`.lc-panel flex:1 overflow:hidden` → `.lc-rows flex:1 overflow-y:auto` (invariata da 0.3.2) distribuisce e
clippa correttamente → scroll dentro il pannello e resize propagato. (Da confermare visivamente: no browser lato agente.)

**`autofocus` sull'input MkDir (0.3.3).** L'`<input>` del dialog MkdirInput porta l'attributo booleano
`autofocus` → focus al PRIMO render del dialog (round 3 aveva solo la preservazione fra render successivi).
Sopravvive alla sanitizzazione perché `plugin-runtime.mjs` lo aggiunge a `ADD_ATTR` (non è nell'allowlist
di default di questo build DOMPurify, verificato per ispezione del bundle vendored).

**Scroll nativo (0.3.2).** `render_panel` renderizza TUTTE le `panel.entries` (niente più
slice `[scroll..scroll+VISIBLE_ROWS]`); `.lc-rows` ha `overflow-y: auto`. La riga selezionata
porta `aria-selected="true"` → l'host (`plugin-window.js`, `render()`) chiama `scrollIntoView`
per portarla in vista quando la selezione da tastiera esce dall'area visibile.

**`data-no-autofit` (0.3.2).** `.lc-root` porta il marker `data-no-autofit` (sopravvive alla
sanitizzazione DOMPurify: `ALLOW_DATA_ATTR` default `true`), letto da `autoFitHeight` nell'host
per NON auto-adattare l'altezza della finestra ad ogni render → l'utente può ridimensionarla
verticalmente a piacere.

**Trasparenza alpha (aggiornato in 0.4.1 — vedi sotto).** I contenitori "sempre visibili" usano
`rgba(...)` con alpha invece dell'esadecimale opaco: `.lc-root`/`.lc-panels`/`.lc-panel`/
`.lc-panel-header`/`.lc-funcbar` derivano TUTTI lo stesso `var(--window-alpha, 0.87)` (nessuno
strato interno ha più un alpha proprio — vedi "Alpha configurabile" e "v0.4.1" sotto per la
cronologia di come si è arrivati qui). Dialog e colori di selezione/accento (`#0f3460`) restano
opachi.

**Allineamento alpha app-wide (round 6, 0.3.5).** L'utente ha notato che `/lc` (e gli altri plugin)
non avevano la stessa trasparenza della finestra principale, e che `/config` non aveva alpha
percepibile. Indagine: l'app aveva 4 valori sparsi (0.82 cursore, 0.86 config/aichat, 0.87
window/window-search, 0.92 shell-plugin+catalogo) — nessuno era davvero "lo standard". Scelto
**0.87** come riferimento unico (decisione utente), allineate TUTTE le finestre nello stesso
commit: `index.html` (0.82→0.87), `config.html`+`config.css` (0.86 sul body ma **0.98** — quasi
opaco — sul box del dialog, la vera causa del "niente alpha" percepito su `/config`), `library.html`
(0.77→0.87), `aichat-window.html` (0.86→0.87), `plugin-window.html` (shell condivisa di tutti i
plugin, 0.92→0.87), `plugin-catalog.css`'s `--lare-bg` (default ereditato da `/calc`/`/ping`/
`/counter`, 0.92→0.87). `window.html`/`window-search.html` erano già a 0.87.

**Alpha configurabile da `--window-alpha` (Task 8, 0.4.0).** Il round 6 (0.3.5, sopra) aveva
allineato tutti i valori `rgba(...)` a un unico letterale `0.87`, ma erano ancora FISSI: cambiando
l'alpha da `/config` (slider aggiunto in un lavoro parallelo su `ui`, Task 1-7 dello stesso piano),
`/lc` non lo seguiva perché il suo CSS è una stringa inline generata server-side, indipendente dal
resto dell'app. `plugin-window.js` (Task 7, già mergiato) imposta la custom property
`--window-alpha` su `document.documentElement` PRIMA di iniettare l'HTML del plugin dentro
`#plugin-root`: essendo `.lc-root` un discendente del documento, la eredita senza bisogno di
propagazione propria (CSS custom properties attraversano i confini del DOM per ereditarietà
normale, non serve un meccanismo dedicato). I 5 `background:` di `INLINE_CSS` sono stati riscritti
per leggere quella property invece del letterale:
- `.lc-root`/`.lc-panels`/`.lc-panel`/`.lc-panel-header`/`.lc-funcbar`: tutti e cinque
  `rgba(R, G, B, var(--window-alpha, 0.87))` — stesso identico alpha ovunque, il fallback `0.87`
  copre il caso (raro: solo se l'host non l'avesse ancora impostata) in cui la property non sia
  presente.

**v0.4.1 — rimosso lo scarto relativo.** 0.4.0 dava a `.lc-panels`/`.lc-panel`/`.lc-panel-header`/
`.lc-funcbar` un alpha DIVERSO dal valore configurato in `/config`, tramite
`clamp(0, calc(var(--window-alpha, 0.87) - OFFSET), 1)` con OFFSET = -0.32/-0.09/-0.02/-0.02 (lo
scarto storico rispetto al root, preservato dal round 4). L'utente ha testato dal vivo e ha
rifiutato esplicitamente questo comportamento: con lo slider a un valore come 0.82, i pannelli di
`/lc` mostravano un alpha diverso l'uno dall'altro e diverso da 0.82 — l'obiettivo dichiarato è
"lo stesso valore su ogni finestra, senza eccezioni". Rimossa la formula da tutti e 4 i selettori
che la usavano, sostituita con la stessa forma piatta già in uso da `.lc-root`. Il colore RGB
(tinta storica) di ciascun selettore resta invariato — solo l'alpha ora è uniforme.

**v0.4.2 — fix: header del pannello attivo restava opaco.** Né 0.4.0 né 0.4.1 avevano toccato
`.lc-panel--active .lc-panel-header` — la regola che sovrascrive lo sfondo dell'header SOLO
quando il pannello ha il focus (`background: #0f3460`, esadecimale pienamente opaco, invariato
fin dalla prima versione del plugin). L'utente l'ha notato dal vivo con uno screenshot: pannello
sinistro attivo, la barra col path corrente (`C:\`) mostrava "alpha del tutto assente" mentre il
resto del pannello era già corretto. Root cause: questa regola non era mai stata inclusa
nell'inventario dei 4+1 selettori "sfondo pannello" toccati da 0.4.0/0.4.1 — è la STESSA classe di
sfondo di `.lc-panel-header` (path corrente), solo in uno stato (`:active`-panel) diverso, ma
viveva in una regola CSS separata mai riletta insieme alle altre. Fix: stessa tinta di accento
(`#0f3460` → `rgba(15, 52, 96, ...)`, conversione esadecimale→decimale) ma con
`var(--window-alpha, 0.87))` al posto dell'alpha implicito 1.0 dell'esadecimale.

Nuovo test (`lc_panel_active_header_background_uses_window_alpha`) verificato RED-first: la prima
stesura del commento esplicativo citava letteralmente la stringa `#0f3460` DENTRO il blocco
`{ ... }` della regola (per spiegare "stessa tinta di prima") — il test la trovava e falliva,
perché `css_block` include tutto il testo tra `{` e la prima `}`, commenti compresi. Lezione
riutilizzabile: quando si scrive un test che asserisce l'ASSENZA di una stringa in un blocco CSS,
i commenti dentro quel blocco contano come contenuto — non citare il valore vecchio letteralmente
nel commento che lo sostituisce.

**v0.4.3 — bordi/hover/selezione/warning/dialog derivano da --window-alpha.** Dopo il fix
dell'header attivo (0.4.2), l'utente ha guardato di nuovo l'interfaccia e l'ha trovata "troppo
diversa" — hover ed evidenziazione della selezione restavano sfondi PIENAMENTE OPACHI (hex senza
alpha), mentre ogni altro pannello dell'app era ormai translucido. Ha chiesto di estendere
`--window-alpha` "a tutti gli elementi grafici", lasciando la scelta del valore all'utente via
`/config`. Prima di eseguire, censimento completo di TUTTI i colori hardcoded non-testo nell'app
(10 file, ~150 regole) via agente di ricerca dedicato — ha rivelato che il resto dell'app (index/
window/library/config/aichat/plugin-window.html + config.css + plugin-catalog.css) non aveva
NESSUN colore di sfondo/bordo pienamente opaco: ogni hover/focus/accento lì era già un `rgba()`
con un proprio alpha scelto deliberatamente (es. hover al 20% per un tocco di colore sottile, non
per "quanto è trasparente la finestra"). Il problema dei blocchi opachi era **concentrato quasi
interamente in `plugin-lc`** — coerente con la lezione già in memoria: questo plugin fu costruito
da una sessione isolata con una convenzione di colore diversa dal resto del progetto (esadecimali
pieni invece di `rgba()` translucidi fin dall'inizio).

Chiesta conferma scope all'utente (3 opzioni: solo hover/selezione; sfondi+hover/selezione/
warning/errore/bordi esclusi scrim e campi di input; letteralmente tutto tranne il testo) — scelta
l'opzione intermedia (raccomandata): ogni sfondo/bordo/box-shadow "superficie pannello" converte,
RESTANO opachi solo lo scrim dietro i dialog (`.mc-overlay` — se diventasse translucido a un
valore di alpha basso il dialog galleggerebbe senza un dimming visibile dietro) e i campi di input
testuale (`.mc-mkdir-input` — deve restare leggibile a ogni valore di alpha).

Convertiti in `plugin-lc/src/render.rs`: bordi di `.lc-root`/`.lc-panel`/`.lc-panel--active`/
`.lc-panel-header`/`.lc-scroll`/`.lc-funcbar` (erano `#444`/`#333`/`#00b4d8`), hover e selezione
delle righe (`.lc-row:hover`/`.lc-row--selected`, erano `#16213e`/`#0f3460` — il caso
originariamente segnalato dall'utente), il dialog di conferma (`.lc-dialog`, `.lc-dialog--error`),
il box di avviso sovrascrittura (`.lc-warning` — solo sfondo/bordo, il testo giallo `#ffd166`
resta invariato per non compromettere la leggibilità dell'avviso), i pulsanti funzione/annulla
(`.lc-fn-btn`, `.mc-btn-cancel` con i rispettivi stati `:hover`), e l'intera tabella diff
(`.lc-diff-th`, `.lc-diff-cell`, `.lc-diff-same/left/right td`). 9 nuovi test RED-first (stesso
pattern `css_block` + assert doppio "contiene var(--window-alpha)" e "non contiene più l'hex
vecchio"), più un test di blocco (`lc_scrim_and_input_field_stay_fully_opaque`) che fissa
esplicitamente l'esclusione di scrim e campo di input, per evitare che un giro futuro li converta
per errore assieme al resto.

**v0.4.4 — font/colore testo allineati allo standard app-wide.** Ultimo giro: l'utente ha inviato
uno screenshot che confronta `/config` con Lare Commander, osservando che i font sembravano "due
applicazioni diverse". Indagine (in `crates/ui/IMPLEMENTATION.md` 0.45.2 il dettaglio completo):
l'intera app aveva due convenzioni di font mai riconciliate — Consolas monospace (cursore/Library/
Markdown/Search/`plugin-lc`) contro Segoe UI sans-serif (`/config`/AI Chat/shell dei plugin).
`.lc-root` usava già Consolas (scelta deliberata del round 2 della saga plugin-lc, per allinearsi
a `library.html`) — quindi NON era l'outlier sul font-FAMILY, lo era invece `/config` (Segoe UI).
Ma `.lc-root`'s `color: #e0e0e0` (grigio puro) ERA un vero outlier: ogni altra finestra dell'app
condivide lo stesso `--text: rgba(220, 235, 255, 0.92)` (tinta blu). L'utente ha scelto **Cascadia
Mono** (monospace più sottile di Consolas) come nuovo standard unico per TUTTA l'app — `.lc-root`
aggiornato di conseguenza (era comunque già sullo stack giusto, solo il primo font della lista
cambia) e il colore testo allineato al token condiviso. Dimensione (`13px`) già corretta, nessuna
modifica. 2 test: `font_family_matches_app_stack` riscritto per il nuovo stack (era già presente
da prima, verificava lo stack Consolas — RED naturale non necessario dato che il test esisteva
già e ha semplicemente iniziato a fallire quando lo stack è cambiato, poi riscritto per il nuovo
valore atteso), `lc_root_text_color_matches_app_text_token` nuovo (RED-first).

**v0.4.5 — Cascadia Mono Light (peso sottile, standard app-wide).** L'utente ha notato dal vivo
che il titolo delle finestre sembrava più "sottile" del contenuto — vero in apparenza (illusione
ottica maiuscolo+letter-spacing) ma non nel CSS (nessun `font-weight` diverso da nessuna parte).
Verificato via PowerShell che Windows installa ogni peso Cascadia come famiglia SEPARATA (non un
font variabile) — `'Cascadia Mono Light'` esiste per nome, `font-weight: 300` su `'Cascadia
Mono'` non avrebbe avuto effetto. `.lc-root`'s stack cambiato a `'Cascadia Mono Light', 'Cascadia
Mono', 'Consolas', 'Courier New', monospace'` (dettaglio completo in `crates/ui/IMPLEMENTATION.md`
0.45.3, stesso cambio su tutte le finestre). Test `font_family_matches_app_stack` aggiornato per
il nuovo stack.

**Wiring tastiera / doppio-click (v0.3.1).** L'host runtime mappa un keydown fisico solo cercando
un elemento `[data-key]` nel DOM; per questo `render_funcbar()` aggiunge `data-key` ai pulsanti
F5–F8/ESC e `render_keytraps()` emette 9 `div.lc-keytrap` invisibili (via CLASSE, non `style`
inline — l'allowlist DOMPurify tiene solo `data-evt`/`data-key`) per i tasti senza pulsante
visibile (Arrow*/Page*/Tab/Enter/Backspace + Home/End da 0.3.4). Le stringhe `data-key`/`data-evt` combaciano 1:1 con i match arm di `handle_ui_event`.
Ogni `.lc-row` porta `data-dblevt="key:Enter"`: il doppio-click riusa la logica di `enter_selected`.

### Dialog overlay

| Tipo | Contenuto |
|---|---|
| `ConfirmCopy/Move` | Titolo + src→dst + **avviso ambra `.lc-warning` se `overwrite`** + [Sì][No] |
| `ConfirmDelete` | Titolo "Elimina" + "Eliminare il file/la cartella `<code>`path`</code>`?" (path escaped) + [Sì][No] |
| `MkdirInput` | Input testo (`autofocus`) + [Crea][Annulla] |
| `FolderCompare` | 4 colonne: solo-sin, ⬅/➡ differenze, solo-des, identici |
| `FileDiff` | Tabella side-by-side: Same (neutro) / Left (rosso scuro) / Right (verde scuro) |
| `Error` | Messaggio + [OK] |

---

## main.rs

```
Init:     cattura storage_dir dal messaggio (nessun reset) → send Ready
Activate: LcConfig::load(&storage_dir) → costruisce LcState (storage_dir) → send ShowWindow
UiEvent:  state::handle(state, msg)    → send UpdateWindow (se non vuoto)
Deinit:   state::handle(state, msg)    → salva config (rete di sicurezza, dentro handle)
```

Il loop è **sincrono** (BufRead line-by-line), single-threaded.

---

## Limitazioni note (deferred)

Valutate dalla review come non bloccanti; documentate qui per non perderle:

- **`list_dir` / `folder_compare` non paginati.** Directory con un numero enorme di voci
  vengono lette e renderizzate tutte in un colpo — limite noto di una UI da file-manager,
  non un bug.
- **Handler `Deinit` non fa `break` del loop stdin.** Differisce stilisticamente da
  `plugin-calc`, ma è innocuo: l'host chiude la pipe, il plugin raggiunge EOF ed esce pulito
  (anche con `kill_on_drop(true)` lato host).
- **Symlink/junction in `copy_dir_all`.** Non gestiti in modo speciale (caso limite,
  specifico delle junction Windows) — non è un problema per il caso comune.
- **Tabella diff LCS in `u16`.** Sicura dato il bound esistente `MAX_DIFF_LINES = 1000`
  (< 65535); da rivedere solo se quel limite salisse molto.
- **Guardia copy-into-self case-sensitive.** `dst.starts_with(src)` confronta i componenti in
  modo case-sensitive, mentre i filesystem Windows sono case-insensitive: un discendente con
  case diverso (`src=C:\Foo`, `dst=C:\foo\...`) può aggirare la guardia. È il predicato prescritto
  dalla review; NON tocca la guardia data-loss critica (`file_name()==None` per "..", indipendente
  dal case) e un eventuale bypass darebbe al più una copia parziale con errore, mai una
  cancellazione. Da irrobustire (confronto case-insensitive su Windows) se emergesse dal vivo.

---

## Test coverage

**120 test Rust verdi** (0.4.0: +5 su 104; 0.4.1: stesso conteggio, 4 test rinominati/riscritti;
0.4.2: +1, `lc_panel_active_header_background_uses_window_alpha`; 0.4.3: +9; 0.4.4: +1 nuovo,
1 riscritto senza cambiare il conteggio) **+ 18 node** su `plugin-runtime.test.mjs`. Aggiunte
0.4.0, riscritte in 0.4.1, estese in 0.4.2/0.4.3/0.4.4 (RED-first su ciascuna: prima il test
asseriva lo scarto/l'assenza dell'opaco/il font vecchio, poi si è verificato che fallisse contro
il CSS precedente, poi corretto il CSS):

| Modulo | Test (0.4.0 → 0.4.1 → +1 in 0.4.2 → +9 in 0.4.3 → +1 in 0.4.4) |
|---|---|
| `render` | `lc_root_background_uses_window_alpha` (invariato) — `lc_panels_background_uses_window_alpha_with_offset` → `lc_panels_background_uses_window_alpha_flat`, idem per `lc_panel`/`lc_panel_header`/`lc_funcbar` (tutti via helper `css_block`, ora asseriscono `var(--window-alpha, 0.87)` presente E `calc(` assente nel blocco della singola regola — niente più scarto); `lc_panel_active_header_background_uses_window_alpha` (0.4.2 — regressione sull'header del pannello attivo, mai coperto prima); 0.4.3: `lc_row_hover_and_selected_use_window_alpha`, `lc_dialog_background_and_border_use_window_alpha`, `lc_dialog_error_border_uses_window_alpha`, `lc_warning_background_and_border_use_window_alpha`, `lc_fn_btn_uses_window_alpha`, `lc_cancel_btn_uses_window_alpha`, `lc_diff_table_uses_window_alpha`, `lc_panel_borders_use_window_alpha`, `lc_scrim_and_input_field_stay_fully_opaque` (test di blocco: verifica che scrim e campo di input RESTINO opachi) |

Aggiunte 0.3.4 (RED-first):

| Modulo | Test (novità 0.3.4) |
|---|---|
| `render` | `lc_panels_css_has_min_height_zero`, `lc_panel_css_has_min_height_zero`, `lc_rows_css_has_min_height_zero` (estrazione mirata del blocco `{ ... }` di ogni regola via helper `css_block`, selettore con graffa aperta); `keytraps_present_once_each` esteso a Home/End |
| `state` | `cursor_to_start_jumps_to_first_entry`, `cursor_to_end_jumps_to_last_entry`, `cursor_to_end_on_empty_list_does_not_panic` (guardia 0 voci), `home_key_moves_cursor_to_first_entry`, `end_key_moves_cursor_to_last_entry` (integrazione via `handle_ui_event`) |

Aggiunte 0.3.3 (RED-first):

| Modulo | Test (novità 0.3.3) |
|---|---|
| `ops` | `delete_item_removes_file`, `delete_item_removes_empty_directory`, `delete_item_refuses_non_empty_directory` |
| `state` | `delete_key_on_file_opens_confirm_delete`, `delete_key_on_dotdot_shows_error_not_confirm`, `confirm_delete_removes_file_and_refreshes_both_panels`, `confirm_delete_non_empty_dir_shows_error_and_keeps_dir` (end-to-end) |
| `render` | `lc_root_css_fills_height_of_real_container`, `render_mkdir_input_has_autofocus`, `render_confirm_delete_dialog_shows_title_and_escaped_path`, `render_confirm_delete_dialog_says_folder_when_dir`, `funcbar_has_delete_button` |
| `plugin-runtime.test.mjs` (host) | `sanitizeAndRender: keeps autofocus on the element (via ADD_ATTR)` — fake DOMPurify che modella l'allowlist |

Aggiunte 0.3.2 (RED-first):

| Modulo | Test (novità 0.3.2) |
|---|---|
| `state` | `escape_at_top_level_closes_window`, `escape_with_dialog_open_does_not_close_window`, `drive_list_round_trip_from_drive_root` |
| `fs` | `list_dir_at_drive_root_shows_dotdot`, `is_drive_root_detects_only_drive_roots`, `list_dir_on_sentinel_lists_drives_without_dotdot`, `parent_of_drive_root_and_sentinel_round_trip` |
| `render` | `render_shows_all_entries_not_capped_at_visible_rows`, `render_selected_row_has_aria_selected`, `render_root_has_no_autofit_marker`, `lc_root_background_is_translucent` |

Tabella storica (aggiunte 0.3.0), RED-first:

| Modulo | Test (novità 0.3.0 in **grassetto**) |
|---|---|
| `config` | load_returns_home_when_file_missing, load_returns_home_when_saved_paths_dont_exist, save_and_reload_roundtrips, **save_creates_storage_dir_if_missing**, **save_writes_only_left_and_right** |
| `fs` | list_dir_returns_entries, dirs_come_before_files, dirs_sorted_case_insensitive, files_sorted_case_insensitive, dotdot_is_first_entry, entry_label_adds_slash_for_dirs, parent_of_returns_parent |
| `ops` | compare_* (5), folder_compare_* (3), copy_file_works, copy_dir_works, move_file_works, mkdir_creates_directory, diff_* (5), **copy_item_rejects_src_without_filename**, **move_item_rejects_src_without_filename**, **copy_item_rejects_dst_inside_src**, **move_item_rejects_dst_inside_src**, **copy_item_rejects_folder_into_own_subfolder**, **move_item_rejects_folder_into_own_subfolder** |
| `state` | initial_active_panel_is_left, tab_switches_active_panel, arrow_down_moves_cursor, arrow_up_does_not_go_negative, enter_on_dir_navigates_into_it, f7_opens_mkdir_dialog, escape_closes_dialog, f8_on_two_files_opens_file_diff, f8_on_dirs_opens_folder_compare, panel_row_click_sets_cursor_and_active_side, **f6_on_dotdot_does_not_destroy_parent_directory**, **f5_on_dotdot_shows_error_not_confirm**, **enter_saves_current_paths_to_storage_dir**, **backspace_saves_current_paths_to_storage_dir**, **f7_mkdir_rejects_traversal_name**, **is_safe_component_name_accepts_ordinary_names**, **is_safe_component_name_rejects_traversal_and_separators** |
| `render` | render_contains_both_paths, render_contains_funcbar_buttons, render_active_panel_has_active_class, render_no_dialog_by_default, render_error_dialog, render_folder_compare_shows_columns, render_file_diff_shows_filenames, render_mkdir_dialog, render_selected_row_has_selected_class, render_data_evt_for_rows, html_escape_escapes_special_chars, format_size_formats_correctly, **render_confirm_copy_shows_overwrite_warning_only_when_true**, **render_confirm_move_shows_overwrite_warning**, **render_panel_header_path_is_html_escaped** |
