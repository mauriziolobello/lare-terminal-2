# Changelog — plugin-lc

All notable changes to this crate will be documented here.
Format: [Keep a Changelog](https://keepachangelog.com/en/1.0.0/). Semver from `0.1.0`.

## 2.0.0 — 2026-09-05 — fork da v1 0.4.5

Copia del crate dalla v1 (`mauriziolobello/lare-terminal`) nel repo 2.0. Nessuna modifica
funzionale in questa voce; le modifiche del piano 1 seguono nelle voci successive.

## [0.4.5] — 2026-07-14 — Cascadia Mono Light (peso sottile, standard app-wide)

### Changed
- `.lc-root`'s `font-family` da `'Cascadia Mono', ...` a `'Cascadia Mono Light', 'Cascadia
  Mono', 'Consolas', 'Courier New', monospace` — stesso cambio applicato a tutte le finestre
  dell'app (vedi `crates/ui/CHANGELOG.md` 0.45.3). Windows installa ogni peso Cascadia come
  famiglia separata (non un font variabile), serve il nome esatto `'Cascadia Mono Light'`.
- Test `font_family_matches_app_stack` aggiornato per il nuovo stack.

## [0.4.4] — 2026-07-14 — font/colore testo allineati allo standard app-wide

### Changed
- `.lc-root`'s `font-family` da `'Consolas', 'Courier New', monospace` a `'Cascadia Mono',
  'Consolas', 'Courier New', monospace` — nuovo standard unico app-wide (vedi `crates/ui/CHANGELOG.md`
  0.45.2), scelto dall'utente dopo un confronto dal vivo con `/config`.
- `.lc-root`'s `color` da `#e0e0e0` (grigio puro) a `rgba(220, 235, 255, 0.92)` — lo stesso
  `--text` condiviso da ogni altra finestra dell'app.
- Dimensione (`font-size: 13px`) già allineata, nessuna modifica necessaria.
- 2 test aggiornati/aggiunti (`font_family_matches_app_stack` riscritto per il nuovo stack,
  `lc_root_text_color_matches_app_text_token` nuovo).

## [0.4.3] — 2026-07-14 — bordi/hover/selezione/warning/dialog derivano da --window-alpha

### Changed
- Ultimo giro di correzione dal vivo sull'alpha configurabile: `/lc` era rimasto l'unico
  posto dell'app con sfondi/bordi PIENAMENTE OPACHI (hex senza alpha) su hover riga,
  riga selezionata, bordi pannello, dialog di conferma, box di avviso, pulsanti funzione/
  annulla, tabella diff — tutti "rompevano" l'effetto vetro contro i pannelli ormai
  translucidi. Convertiti tutti a `rgba(R, G, B, var(--window-alpha, 0.87))`, stessa
  tinta di prima. Restano deliberatamente opachi: lo scrim dietro i dialog
  (`.mc-overlay`, garantisce il dimming) e il campo di input testuale
  (`.mc-mkdir-input`, resta leggibile a ogni valore di alpha).
- 9 nuovi test (`css_block`-based, RED-first) coprono ogni selettore convertito, più un
  test di blocco che verifica che scrim e input restino opachi.

## [0.4.2] — 2026-07-13 — fix: header del pannello attivo restava opaco

### Fixed
- `.lc-panel--active .lc-panel-header` (l'header col path corrente, quando il pannello ha il
  focus) aveva uno sfondo opaco `#0f3460`, mai toccato né in 0.4.0 né in 0.4.1 — solo lo stato
  NON attivo di `.lc-panel-header` derivava da `--window-alpha`. L'utente l'ha notato dal vivo
  (screenshot: pannello sinistro attivo di Lare Commander, alpha "del tutto assente" sulla barra
  col path `C:\`). Ora anche questa regola usa `rgba(15, 52, 96, var(--window-alpha, 0.87))` —
  stessa tinta di accento blu, ma con lo stesso alpha di ogni altro sfondo pannello.

## [0.4.1] — 2026-07-13 — rimosso lo scarto relativo: alpha uniforme come tutte le altre finestre

### Changed
- Correzione post-live-test: l'utente ha verificato dal vivo che gli scarti relativi
  introdotti in 0.4.0 (-0.32 `.lc-panels`, -0.09 `.lc-panel`, -0.02 `.lc-panel-header`/
  `.lc-funcbar`) producevano un alpha diverso tra i pannelli di `/lc` e diverso dal
  valore configurato in `/config` — non l'obiettivo dichiarato dall'utente ("stesso
  valore su ogni finestra, senza eccezioni"). Rimossa la formula
  `clamp(0, calc(var(--window-alpha, 0.87) ± offset), 1)` da tutti e 4 i selettori,
  sostituita con la forma piatta `rgba(R, G, B, var(--window-alpha, 0.87))` — stesso
  alpha di `.lc-root` e di ogni altra finestra dell'app. Il colore RGB (tinta storica)
  di ciascun selettore resta invariato.

## [0.4.0] — 2026-07-13 — alpha configurabile da /config (deriva da --window-alpha)

### Changed
- Gli sfondi dei pannelli (`.lc-root`, `.lc-panels`, `.lc-panel`, `.lc-panel-header`,
  `.lc-funcbar`) derivano ora l'alpha da `--window-alpha` (ereditata dall'host via
  `plugin-window.js`), invece di un valore fisso 0.87. Configurabile da `/config`.
  Gli scarti relativi storici (-0.32/-0.09/-0.02) sono preservati come costanti CSS.

## [0.3.5] — 2026-07-13 — allineamento alpha trasparenza con tutte le altre finestre

L'utente ha notato che la trasparenza di `/lc` (e degli altri plugin) non era la stessa della
finestra principale, e che `/config` non aveva alpha percepibile. Indagine: l'app aveva 4 valori
alpha diversi sparsi tra le finestre (0.82 cursore, 0.86 config/aichat, 0.87 window/window-search,
0.92 shell plugin+catalogo), nessuno realmente "lo standard". Scelto **0.87** come riferimento
unico (decisione utente), allineate TUTTE le finestre dell'app in un solo giro — non solo `/lc`.

### Fixed

- `.lc-root`'s sfondo esterno: `rgba(26, 26, 46, 0.92)` → `0.87` — allineato allo standard
  dell'app, non più solo alla shell di `plugin-window.html` (che a sua volta era sbagliata).
  Gli alpha degli strati INTERNI (`.lc-panels` 0.55, `.lc-panel` 0.78, header/funcbar 0.85) restano
  invariati per design (round 4): sono scelte di leggibilità distinte dall'alpha "di finestra".

### Note (fuori da questo crate, stesso commit)

Stesso alpha 0.87 applicato anche a: `index.html` (cursore, era 0.82), `config.html`+`config.css`
(era 0.86 sul body ma **0.98** — quasi opaco — sul box del dialog, la vera causa del "niente
alpha" percepito su `/config`), `library.html` (era 0.77), `aichat-window.html` (era 0.86),
`plugin-window.html` (shell condivisa di tutti i plugin, era 0.92), `plugin-catalog.css`'s
`--lare-bg` (default ereditato da `/calc`/`/ping`/`/counter` quando non lo sovrascrivono, era
0.92). `window.html`/`window-search.html` erano già a 0.87, nessuna modifica.

## [0.3.4] — 2026-07-12 — fix da uso dal vivo (round 5): scroll dentro il pannello (`min-height: 0`) + tasti Home/End

Quinto giro sui bug osservati dall'uso reale del supervisore. Root cause già diagnosticata
empiricamente (gotcha CSS flexbox arcinoto), implementata come specificata. Una fix CSS + una
piccola feature tastiera.

### Fixed

- **La lista directory allungava il pannello invece di scrollare al suo interno.** Round 4 aveva
  già dato a `.lc-root { height: 100% }` (resize verticale ok, confermato dal vivo), ma lo scroll
  DENTRO ogni pannello ancora non partiva: con molte voci `.lc-rows` cresceva, allungando
  `.lc-panel` e `.lc-panels`, e spingeva la funcbar (F5…/Del/Escape) giù/fuori vista; la rotellina
  del mouse non aveva nulla di limitato su cui agire. **Root cause: il default `min-height: auto`
  dei flex item.** Un flex item ha `min-height` = `auto` di default (= "alto almeno quanto il mio
  contenuto"), che vince silenziosamente sia su `flex: 1` (non riesce a rimpicciolire l'item sotto
  il contenuto) sia su `overflow-y: auto` (cresce invece di clippare). Aggiunto `min-height: 0` a
  TUTTI e tre gli anelli della catena flex-column — `.lc-panels`, `.lc-panel`, `.lc-rows` — così
  ogni item può rimpicciolire fino allo spazio assegnato dal layout e la scrollbar/rotellina di
  `.lc-rows` si attiva finalmente. (Un fix parziale su un solo anello non basterebbe: il vincolo
  tornerebbe a propagarsi verso il basso.) **Stessa CLASSE** del fix di round 4 (dimensionamento
  box-model in flex) ma **meccanismo diverso**: round 4 era un'altezza percentuale mancante a monte;
  questo è il `min-height: auto` dei flex item a valle. `.lc-root` NON toccato (il suo `height: 100%`
  è corretto e confermato). **Da confermare visivamente dal supervisore** (scrollbar dentro `.lc-rows`,
  funcbar sempre visibile, rotellina funzionante): il subagente non vede un browser reale — stessa
  situazione della fix di resize di round 4.

### Added

- **Tasti Home/End** — saltano rispettivamente alla PRIMA (indice 0, tipicamente "..") e all'ULTIMA
  voce della directory corrente. Rispecchiano esattamente il pattern esistente di ArrowUp/Down e
  PageUp/Down: due keytrap invisibili in più (`data-key="Home"/"End"`, `data-evt="key:Home"/"key:End"`)
  in `render_keytraps`, due nuovi metodi `PanelState::cursor_to_start`/`cursor_to_end` (salto assoluto,
  non relativo) e i due match arm `key:Home`/`key:End` in `handle_ui_event`. `cursor_to_end` usa
  `saturating_sub(1)` → nessun panic su lista vuota. `move_cursor` (usato da frecce/PageUp/Down)
  invariato.

### Tests (+8: 96 → 104 Rust)

- `render`: `lc_panels_css_has_min_height_zero`, `lc_panel_css_has_min_height_zero`,
  `lc_rows_css_has_min_height_zero` (estrazione mirata del blocco `{ ... }` di ogni regola — non una
  sottostringa globale — via helper `css_block`; il selettore si passa con la graffa aperta così
  `.lc-panel {` non collide con `.lc-panels`/`.lc-panel--active`/`.lc-panel-header`). Esteso
  `keytraps_present_once_each` per coprire Home/End.
- `state`: `cursor_to_start_jumps_to_first_entry`, `cursor_to_end_jumps_to_last_entry`,
  `cursor_to_end_on_empty_list_does_not_panic` (guardia su 0 voci), `home_key_moves_cursor_to_first_entry`,
  `end_key_moves_cursor_to_last_entry` (integrazione via `handle_ui_event`, mirror dello stile ArrowDown/PageUp).

## [0.3.3] — 2026-07-12 — fix da uso dal vivo (round 4): resize/scrollbar, focus MkDir iniziale + comando Elimina

Quarto giro sui bug osservati dall'uso reale del supervisore (root cause già diagnosticate
empiricamente: ispezione diretta del bundle DOMPurify vendored e del CSS di `#plugin-root`).
Due fix + una feature nuova (eliminazione, file o cartella VUOTA — nessuna ricorsione).

### Fixed

- **Resize verticale + scrollbar al livello sbagliato.** Entrambi avevano la stessa root cause:
  `#plugin-root` (la "chrome" host in cui l'HTML del plugin è iniettato — fuori dal controllo di
  questo crate) è esso stesso uno scroll container (`flex: 1; overflow-y: auto; padding: 14px 16px`),
  mentre `.lc-root` aveva `min-height: 520px` ma **nessun `height: 100%`** → si dimensionava sulla
  propria altezza di CONTENUTO invece di stirarsi a riempire la finestra. Conseguenze: (a) il resize
  verticale della finestra cambiava l'altezza di `#plugin-root` ma `.lc-root` non la seguiva → i
  pannelli non rispondevano; (b) con molte righe `.lc-root` cresceva OLTRE `#plugin-root`, e poiché
  quest'ultimo ha il proprio `overflow-y: auto` la scrollbar compariva **attorno all'intero plugin**
  (header + pannelli + funcbar) invece che dentro il singolo `.lc-rows`. Aggiunto `height: 100%` a
  `.lc-root`: ora la catena flex-column (`.lc-panels` → `.lc-panel` → `.lc-rows`, invariata da round 3)
  ha un'altezza reale e limitata da distribuire e clippare → resize propagato e scroll dentro il pannello.
  (`.lc-panels`/`.lc-panel`/`.lc-rows` NON toccati: erano già corretti, mancava solo l'altezza a monte.)
- **Input MkDir senza focus iniziale.** Round 3 aveva aggiunto la PRESERVAZIONE del focus fra un render
  e l'altro, ma nulla dava il focus la PRIMA volta che il dialog appariva → l'utente doveva cliccare il
  campo prima di digitare. Aggiunto l'attributo booleano HTML `autofocus` all'input MkDir. Poiché questo
  build di DOMPurify NON ha `autofocus` nell'allowlist di default (verificato per ispezione diretta di
  `vendor/purify.min.js`: la stringa "autofocus" non compare), va aggiunto a `ADD_ATTR` in
  `plugin-runtime.mjs` (stesso meccanismo già usato per `data-evt`/`data-key`) o verrebbe rimosso in
  silenzio. I due meccanismi coesistono: `autofocus` al primo render, la logica di round 3 (ri-focus via
  `data-evt`) dai render successivi (ogni tasto ne scatena uno).

### Added

- **Comando Elimina (tasto `Delete`).** Nuova voce: elimina un file, oppure una directory **solo se
  vuota** (nessuna ricorsione — una directory non vuota viene rifiutata con un errore esplicito, mai
  cancellata: eliminare un intero albero è troppo distruttivo per un singolo tasto). Rispecchia
  esattamente il pattern F5 (Copia)/F6 (Sposta): dialog di conferma prima di agire, stessa guardia
  anti-`..`, errori d'esecuzione via `Dialog::Error`. Tasto `Delete` (distinto da Backspace) + pulsante
  visibile "Del Elimina" nella barra funzioni.
  - `ops::delete_item(path)` — file → `remove_file`; directory vuota → `remove_dir`; directory non vuota
    → `Err` (`Error::other`, messaggio "la cartella non è vuota…").
  - `state::Dialog::ConfirmDelete { path, is_dir }` (un solo path, niente destinazione); arm `"key:Delete"`
    in `handle_ui_event` (guardia `..`) e arm `dialog:confirm:yes` in `handle_dialog_event` (elimina +
    refresh di ENTRAMBI i pannelli, o `Dialog::Error` su fallimento).
  - `render`: dialog di conferma ("Elimina il file/la cartella <code>path</code>?", path HTML-escaped) +
    6° pulsante funcbar (`data-key="Delete" data-evt="key:Delete"`).

### Note di scope

- **Copia**: confermata funzionante in un round precedente, NON toccata (ciò che sembrava rotto era il
  sintomo del resize/scrollbar sopra — i pulsanti del dialog di fatto irraggiungibili in overflow).
- Fuori scope: eliminazione ricorsiva di directory (rifiutata esplicitamente dal supervisore); qualsiasi
  modifica a F5/F6/F7/F8/Escape o a `copy_item`/`move_item`/`mkdir`. L'unica modifica all'host runtime
  condiviso è la riga `ADD_ATTR` in `plugin-runtime.mjs` (+ il suo test).
- Il fix di `.lc-root { height: 100% }` è CSS e va **confermato visivamente dal supervisore** (il
  subagente non vede un browser reale); il ragionamento sul layout flex è nel commento inline.

### Tests (+13: 12 Rust → 96 Rust totali · +1 node → 18 node su plugin-runtime.test.mjs)

- `ops`: `delete_item_removes_file`, `delete_item_removes_empty_directory`,
  `delete_item_refuses_non_empty_directory` (RED-first: non-cancellazione + messaggio).
- `state`: `delete_key_on_file_opens_confirm_delete`, `delete_key_on_dotdot_shows_error_not_confirm`,
  `confirm_delete_removes_file_and_refreshes_both_panels`,
  `confirm_delete_non_empty_dir_shows_error_and_keeps_dir` (regressione end-to-end via
  `handle_ui_event`/`handle_dialog_event`, non `ops` in isolamento).
- `render`: `lc_root_css_fills_height_of_real_container`, `render_mkdir_input_has_autofocus`,
  `render_confirm_delete_dialog_shows_title_and_escaped_path`,
  `render_confirm_delete_dialog_says_folder_when_dir`, `funcbar_has_delete_button`.
- `plugin-runtime.test.mjs`: `sanitizeAndRender: keeps autofocus on the element (via ADD_ATTR)` — fake
  DOMPurify che modella l'allowlist (strip di `autofocus`/`data-evt` salvo ADD_ATTR).

## [0.3.2] — 2026-07-12 — fix da uso dal vivo (round 3): input MkDir, scroll, resize, ESC chiudi, unità disco, trasparenza

Terzo giro di fix su bug osservati dall'uso reale del supervisore (diagnosi empirica
process-level, che ha provato F5→Copy e F7→MkDir funzionanti a livello `state.rs`: i bug
erano nel layer host/browser e nel wiring markup, non nella logica). Parte delle fix è
nell'host runtime condiviso (`crates/ui/frontend/plugin-window.js`), parte in questo crate.

### Fixed (in questo crate: `render.rs` / `state.rs` / `fs.rs`)

- **ESC ora chiude la finestra.** A livello top (nessun dialog aperto) `key:Escape` era
  `state.dialog = None`, un no-op osservabile (dialog già `None`); il pulsante è etichettato
  "ESC Chiudi" ma la chiusura non era mai stata implementata. Ora restituisce
  `PluginToHost::CloseWindow { window_id }`. Escape CON un dialog aperto continua a chiudere
  solo il dialog (comportamento invariato, blindato da test).
- **Scroll con la rotellina del mouse.** `.lc-rows` passa da `overflow: hidden` a
  `overflow-y: auto` e `render_panel` renderizza ORA TUTTE le voci (prima solo una finestra
  di 20 righe decisa lato server → non c'era nulla da scrollare). Rimosso l'indicatore
  percentuale (privo di senso senza finestratura), sostituito da un contatore "N elementi".
  La riga selezionata porta `aria-selected="true"` → l'host la porta in vista via
  `scrollIntoView` quando la selezione si sposta da tastiera oltre l'area visibile.
- **Resize verticale libero.** `.lc-root` porta `data-no-autofit`: marker di sola
  presentazione letto da `autoFitHeight` (host) per NON auto-adattare l'altezza della
  finestra ad ogni render — prima ogni update la ri-schiacciava sull'altezza "naturale",
  vanificando il resize verticale manuale (la finestra "seguiva l'orizzontale ma non il
  verticale").
- **Navigazione fra unità disco (Windows).** Alla radice di un'unità ("C:\") compare ora
  ".." (prima `Path::parent()` per "C:\" è `None` → nessuna via d'uscita). `fs.rs` guadagna
  `drive_list_sentinel()` (path sentinella = `PathBuf::new()`, vista "elenco unità"),
  `is_drive_root()` (cfg-windows; `false` altrove) e un `list_drives()` interno che sonda
  A:..Z:. `parent_of("C:\")` → sentinella; `list_dir(sentinella)` → elenco unità (senza "..").
  Entrare in un'unità dall'elenco riusa `enter_selected()` invariato.

### Removed / Refactor

- **`PanelState.scroll`** rimosso: esisteva solo per la finestratura server-side, ora morta.
  `move_cursor` perde la logica di aggiustamento scroll (mantiene invariato lo spostamento
  del cursore ±1/±10); `enter_selected`/Backspace non azzerano più `scroll`.

### Trasparenza (alpha)

- Gli sfondi dei contenitori "sempre visibili" passano da esadecimale PIENAMENTE opaco a
  `rgba(...)` con alpha, così Lare Commander è translucido come ogni altra finestra di Lare
  Terminal: `.lc-root` `#1a1a2e` → `rgba(26,26,46,0.92)` (alpha come la shell), `.lc-panels`
  `#111` → `rgba(17,17,17,0.55)`, `.lc-panel` `#1a1a2e` → `rgba(26,26,46,0.78)`,
  `.lc-panel-header`/`.lc-funcbar` `#16213e` → `rgba(22,33,62,0.85)`. Dialog e colori di
  selezione/accento restano opachi per leggibilità (scelte alpha rivedibili dal supervisore).

### Verificato (nessuna modifica)

- **Font**: unico stack esplicito è `.lc-root` = `'Consolas', 'Courier New', monospace`;
  l'unico altro `font-family` è `.mc-mkdir-input { font-family: inherit }`. Cascade pulito.
- **Header pannello con path sentinella**: `.display()` della sentinella è stringa vuota →
  header vuoto per la vista "elenco unità". Accettabile, nessun special-case (nota: possibile
  micro-miglioria UX futura, es. etichetta "Unità disco").

### Note di scope

- Le fix all'host runtime (listener `input`, preservazione focus/cursore su re-render,
  opt-out `data-no-autofit`, `scrollIntoView` su `aria-selected`) vivono in
  `crates/ui/frontend/plugin-window.js` (DOM-driven, non unit-testato con `node:test` per
  convenzione del progetto: nessun `plugin-window.test.mjs` esiste; verifica strutturale +
  build). Nessuna modifica a protocollo/manifest/orchestrator. `plugin.json` NON toccato.
- "Copia non funziona" NON è stato modificato: già provato funzionante a livello Rust dal
  supervisore (copia reale su disco riuscita). Probabilmente era il dialog tagliato da una
  finestra non ridimensionabile — risolto indirettamente dalla fix di resize verticale.

### Tests (+11, ora 84)

- `state`: `escape_at_top_level_closes_window`, `escape_with_dialog_open_does_not_close_window`,
  `drive_list_round_trip_from_drive_root`.
- `fs`: `list_dir_at_drive_root_shows_dotdot`, `is_drive_root_detects_only_drive_roots`,
  `list_dir_on_sentinel_lists_drives_without_dotdot`, `parent_of_drive_root_and_sentinel_round_trip`.
- `render`: `render_shows_all_entries_not_capped_at_visible_rows`,
  `render_selected_row_has_aria_selected`, `render_root_has_no_autofit_marker`,
  `lc_root_background_is_translucent`.

## [0.3.1] — 2026-07-12 — fix UX da uso dal vivo: tastiera, doppio-click, layout, font

Lare Commander spediva con 68/68 unit test verdi ma era di fatto INUSABILE dal vivo: nessuna
navigazione da tastiera (frecce/PageUp/Down/Tab/Enter/Backspace), doppio-click inerte, finestra
troppo stretta col layout non responsivo, font diverso dal resto dell'app. Root cause: la logica
in `state.rs` era corretta e testata, ma l'HTML generato da `render.rs` NON offriva alcun modo di
attivarla da tastiera/doppio-click (wiring markup↔host incompleto). Fix solo di rendering/wiring —
nessuna modifica alle guardie di sicurezza/data-loss della 0.3.0.

### Fixed (wiring markup ↔ host runtime)
- **Tastiera**: i pulsanti F5–F8/ESC portano ora ANCHE `data-key` (oltre a `data-evt`), così il tasto fisico li attiva (prima solo il mouse). Aggiunti 7 "keytrap" invisibili (`.lc-keytrap { display: none }`) per i tasti senza pulsante visibile — ArrowDown/Up, PageDown/Up, Tab, Enter, Backspace — ciascuno con `data-key`/`data-evt` (stringhe copiate 1:1 dai match di `state::handle_ui_event`). La finestratura server-side (scroll) diventa così raggiungibile.
- **Doppio-click**: ogni riga (inclusa "..") porta `data-dblevt="key:Enter"` → doppio-click = seleziona + Enter (riusa la logica già testata di `state.rs`, zero nuovo stato).
- **Layout**: `.lc-root` passa da `width: 900px` fisso a `width: 100%` (+ `min-width: 640px` difensivo) → riempie la finestra reale data dall'host.
- **Font**: `font-family` allineato allo stack dell'app: `'Consolas', 'Courier New', monospace`.

### Added
- `plugin.json`: campo `window: { width: 960.0, height: 620.0 }` — la finestra si apre alla dimensione adatta a due pannelli (invece del default 480×360).

### Tests (+5, ora 73)
- `funcbar_buttons_have_both_data_evt_and_data_key`, `keytraps_present_once_each` (regressione che avrebbe colto il bug originale), `rows_carry_double_click_enter`, `lc_root_css_fills_width_not_fixed_900`, `font_family_matches_app_stack`.

## [0.3.0] — 2026-07-12

Pass di correzioni critiche + hardening (bug di sicurezza/data-loss trovati da una
review avversariale). Nota: il `Cargo.toml` era rimasto a `0.1.0` (valore di
scaffold mai aggiornato) mentre `plugin.json`/CHANGELOG erano già a `0.2.0`; qui
la versione del crate è allineata e portata a `0.3.0`.

### Fixed (data-loss critici)

- **F5/F6 sulla voce ".." corrompeva/cancellava la directory padre.** La voce ".."
  (indice 0, cursore che si resetta a 0 ad ogni navigazione → banalmente raggiungibile)
  produceva `src = <panel>/..` (senza `file_name`) e `dst` che collassava sulla dir
  dell'altro pannello; F6 (Sposta) eseguiva `move_item(parent, other)` → il fallback
  cross-device faceva `copy_item` + **`remove_dir_all(parent)`**, cancellando l'intero
  albero del genitore. **Difesa in profondità su due livelli:**
  - `state.rs` (F5/F6): la voce ".." è rifiutata (mostra `Dialog::Error`), niente dialog.
  - `ops.rs` (`copy_item`/`move_item`): rifiutano con `InvalidInput` ogni `src` senza
    `file_name()` (invariante difensivo generale, non specifico di "..").
- **Copy-into-itself.** `copy_item`/`move_item` rifiutano ora `dst` discendente di `src`
  (`dst.starts_with(src)` → `InvalidInput`), sia sul percorso `rename` veloce sia sul
  fallback ricorsivo — prima copiare una cartella dentro una sua sottocartella ricorreva
  finché il path non esplodeva.

### Fixed (path traversal)

- **Dialog F7 (mkdir) — nome non validato.** Un nome come `C:\Windows\System32\pwned`,
  `..\..\..\Windows\System32` o un path UNC sfuggiva dalla directory del pannello.
  Nuova `is_safe_component_name` (rifiuta vuoto, `.`, `..`, e ogni nome con `/`, `\`, `:`)
  chiamata prima di `ops::mkdir`; su rifiuto mostra `Dialog::Error`. Spazi e unicode
  restano accettati.

### Changed (persistenza ridisegnata)

- I path dei pannelli si salvano ora in **`storage_dir/config.json`** (la dir privata del
  plugin fornita dall'host nell'`Init`), non più in `plugin.json` — che è il manifest
  statico letto dalla discovery e mostrato nella tab "Plugins" della Library (violazione
  della separazione manifest statico / stato runtime, `Docs/18-plugin-system.md`).
- **Salvataggio ad ogni navigazione** (Enter/Backspace), non più solo al `Deinit` — che
  l'host invia **solo** allo shutdown dell'orchestratore (la chiusura della finestra `/lc`
  è `PluginWindowClosed`, gestita come `forget_window` senza `Deinit`): la feature era di
  fatto **morta** entro la sessione. Ora la persistenza attraversa sia chiudi/riapri sia il
  riavvio dell'orchestratore.
- **Rimosso il reset-on-`Init`** (`reset_to_home`): controproducente con la persistenza
  cross-restart (cancellava lo stato salvato prima che venisse riletto).
- `try_save` semplificato: `config.json` è di proprietà esclusiva del plugin → niente più
  merge dei campi, serializza solo `left_path`/`right_path`, e fa `create_dir_all(storage_dir)`
  (l'host non garantisce l'esistenza della dir).
- `plugin.json` ripulito: rimossi i campi `left_path`/`right_path`, forma manifest pulita
  (come `plugin-calc`).
- `LcState.plugin_dir` → `LcState.storage_dir`.

### Added (UX / robustezza)

- **Avviso di sovrascrittura**: `Dialog::ConfirmCopy`/`ConfirmMove` hanno un campo
  `overwrite: bool` (= `dst.exists()`); il render mostra una riga di avviso ambra distinta
  ("la destinazione esiste già e verrà sovrascritta") quando `true`.
- **Escape HTML dell'header del pannello**: il path dell'header passa ora da `html_escape`
  (coerente con il resto del file; un path con `&` prima corrompeva il markup).

### Fixed (lint)

- `fs.rs`: `sort_by` → `sort_by_key` (2 istanze). `config.rs`: `io::Error::new(Other, e)` →
  `io::Error::other(e)`. `cargo clippy -p plugin-lc --all-targets` pulito.

### Deploy

- `deploy_binary_only.ps1`: aggiunto `"lc"` a `$PluginIds` (prima il plugin era saltato
  silenziosamente da `-IncludePlugins`).

### Tests

- Nuovi test TDD (RED reale prima del codice): scenario esatto ".."-su-F5/F6 via
  `handle_ui_event`, guardie `copy_item`/`move_item` (src senza nome, dst dentro src),
  `is_safe_component_name` + flusso F7 malevolo, salvataggio-su-navigazione con lettura di
  `config.json`, config `config.json`-only (roundtrip / missing→home / path-inesistente→home /
  crea storage_dir), avviso di sovrascrittura nel render, escape dell'header. 67 test verdi.

---

## [0.2.0] — 2026-07-12

### Added (config persistente)

- **`src/config.rs`** — nuovo modulo `LcConfig` per la gestione dei path persistenti.
  - `LcConfig::load(&plugin_dir)`: legge `left_path`/`right_path` da `plugin.json` accanto
    all'eseguibile; se il file manca o i path non esistono sul filesystem → entrambi a home.
  - `LcConfig::save(&self, &plugin_dir)`: scrive i due path in `plugin.json` preservando tutti
    gli altri campi (merge JSON senza sovrascrivere `name`, `id`, `version`, …).
  - `LcConfig::reset_to_home(&plugin_dir)`: resetta i path a `%USERPROFILE%` (o `$HOME` su
    sistemi non-Windows). Chiamato all'`Init`, cioè ogni volta che l'orchestratore riparte.
  - `plugin_dir` rilevato in `main.rs` con `std::env::current_exe().parent()`.

- **Politica di persistenza:** ogni sessione (avvio orchestratore) parte dalla home; durante
  la sessione, i path vengono salvati alla chiusura del pannello (`Deinit`) e riletti alla
  riapertura (`Activate`) — navigazione persistente all'interno della stessa sessione.

- **`plugin.json`** — aggiunti i campi `left_path` e `right_path` (inizialmente stringa vuota;
  risolti a home al primo caricamento).

- **Tests config (5):** `load_returns_home_when_missing`, `load_returns_home_when_paths_not_exist`,
  `save_and_reload_roundtrips`, `save_preserves_other_fields`, `reset_to_home_writes_home_paths`.

### Added (confronto cartelle potenziato — `FolderCompare`)

- **`ops::FolderCompareResult`** — struttura con 5 liste: `only_left`, `only_right`,
  `newer_left` (presenti in entrambi, più recenti a sinistra), `newer_right`, `identical`.
- **`ops::folder_compare(left, right) -> io::Result<FolderCompareResult>`** — legge i
  metadati reali (`std::fs::metadata`) per confrontare `modified()`. Per le directory
  il confronto è per nome soltanto (marcate come `identical`). Non ricorsivo.
- **`Dialog::FolderCompare { left_path, right_path, result }`** — sostituisce
  `Dialog::CompareResult`. Contiene i path per il titolo del dialog.
- **Render FolderCompare** — overlay con 4 colonne: `Solo sinistra`, `Differenze data`
  (⬅ più recente a sinistra, ➡ a destra), `Solo destra`, `Identici`. Conteggi per colonna.

- **Tests ops (3 nuovi):** `folder_compare_finds_only_sides`, `folder_compare_detects_newer_left`,
  `folder_compare_detects_newer_right`.

### Added (diff file di testo — `FileDiff`)

- **`ops::DiffLine`** — enum `Same { line }` / `Left { line }` / `Right { line }`.
- **`ops::FileDiffResult`** — contiene le righe diff interlacciate, contatori `left_only` /
  `right_only` / `same`, flag `truncated` (true se file troncato a 1000 righe per lato).
- **`ops::diff_files(left, right) -> io::Result<FileDiffResult>`**:
  - Limite: 512 KB per file; rifiuta file binari (byte NUL).
  - Algoritmo **LCS (Longest Common Subsequence)** su righe: tabella DP `(m+1)×(n+1)` in
    `u16`, complessità O(m×n) con m,n ≤ 1000. Backtracking per ricostruire Same/Left/Right.
- **`Dialog::FileDiff { diff }`** — aperto da F8 quando entrambi i pannelli hanno un **file**
  selezionato (non directory).
- **Logica F8 aggiornata:** se il cursore sinistro punta a un file E il cursore destro punta
  a un file → `FileDiff`; altrimenti → `FolderCompare`.
- **Render FileDiff** — overlay con tabella side-by-side a due colonne:
  - `Same`: sfondo neutro, riga mostrata in entrambe le colonne.
  - `Left`: sfondo `#3d1c1c` (rosso scuro), riga nella colonna sinistra, destra vuota.
  - `Right`: sfondo `#1c3d1c` (verde scuro), sinistra vuota, riga nella colonna destra.
- **Tests ops (5 nuovi):** `diff_identical_files_no_diffs`, `diff_added_line_right`,
  `diff_removed_line_left`, `diff_binary_returns_error`, `diff_large_returns_error`.

### Changed

- **`LcState`**: aggiunto campo `plugin_dir: PathBuf` — usato da `handle()` nel `Deinit` per
  salvare i path correnti. `LcState::new()` imposta `plugin_dir: PathBuf::new()` (default per
  test); `main.rs` lo sovrascrive con il valore reale.
- **`Dialog::CompareResult`** → **`Dialog::FolderCompare`** (breaking change interno;
  nessun impatto sul protocollo host).
- **`handle()` Deinit**: ora chiama `LcConfig::save(&state.plugin_dir)` prima di restituire
  `vec![]`.
- **Tests state aggiornati (2):** `f8_opens_compare_result` → `f8_on_dirs_opens_folder_compare`
  e aggiunto `f8_on_two_files_opens_file_diff`.
- **Tests render aggiornati (1):** `render_compare_dialog_shows_lists` → usa `Dialog::FolderCompare`.

---

## [0.1.0] — 2026-07-11

### Added (v0.1 — Lare Commander, interfaccia doppio pannello)

Implementazione iniziale del plugin Lare Commander. Attivato con `/lc`.

**Moduli creati:**

- **`src/fs.rs`** — `Entry { name, is_dir, size }`, `list_dir(path)` (ordinamento: `..` primo,
  poi dir case-insensitive, poi file case-insensitive), `parent_of(path)`.
- **`src/ops.rs`** — `CompareResult`, `compare(left, right)` (puro, su listing); `copy_item`,
  `copy_dir_all`, `move_item`, `mkdir`.
- **`src/state.rs`** — `Side`, `PanelState`, `Dialog` (ConfirmCopy, ConfirmMove, MkdirInput,
  CompareResult, Error), `LcState`; `handle()` + `handle_ui_event()` + `handle_dialog_event()`.
- **`src/render.rs`** — `render_window()` con CSS inline `lc-*`: pannelli, righe, barra funzioni,
  dialog modali; `html_escape`, `format_size`.
- **`src/main.rs`** — Loop stdin sincrono; gestione Init/Activate/UiEvent/Deinit; `home_dir()`.
- **`Cargo.toml`** — `[[bin]] name="lc"`, deps: `plugin-protocol` + `serde_json`.
- **`plugin.json`** — `id="lc"`, `name="Lare Commander"`, `triggers.command="/lc"`.

**Funzionalità UI:**

| Tasto | Azione |
|---|---|
| ↑ / ↓ | Muovi cursore |
| PgUp / PgDn | Muovi cursore ±10 |
| Enter | Entra in directory |
| Backspace | Sali di un livello |
| Tab | Cambia pannello attivo |
| F5 | Copia (dialog conferma) |
| F6 | Sposta (dialog conferma) |
| F7 | Crea directory (dialog input) |
| F8 | Confronta listing dei due pannelli |
| ESC | Chiudi dialog |
| Click riga | Seleziona + attiva pannello |
| Click header | Attiva pannello |

**Test suite iniziale: 36 test verdi** (fs: 7, ops: 8, state: 9, render: 12).
