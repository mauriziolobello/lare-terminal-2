//! render.rs — Genera l'HTML completo della finestra Lare Commander.
//!
//! `render_window` è la funzione pura centrale: riceve lo stato del plugin e
//! restituisce una stringa HTML. Ogni volta che lo stato cambia, il plugin
//! invia l'HTML completo all'host che sostituisce il contenuto della finestra.
//!
//! Layout della finestra:
//! ```
//! ┌──────────────────────────────────────────┐
//! │  /path/sinistra   │   /path/destra       │  ← header pannelli
//! │ ┌────────────────┬────────────────────┐  │
//! │ │ ..             │ ..                 │  │  ← riga ".."
//! │ │ 📁 cartella/   │ 📁 cartella/       │  │
//! │ │ 📄 file.txt    │ ▶ file.txt         │  │  ← riga attiva (highlight)
//! │ └────────────────┴────────────────────┘  │
//! │  [F5 Copia] [F6 Sposta] [F7 MkDir] ...   │  ← barra comandi
//! └──────────────────────────────────────────┘
//! ```
//!
//! Se `state.dialog` è `Some(...)`, sopra ai pannelli appare un pannello modale.

use crate::state::{LcState, PanelState, Side, Dialog};
use crate::ops::{DiffLine, FolderCompareResult};

/// Genera l'HTML completo della finestra.
pub fn render_window(state: &LcState) -> String {
    let panel_content = render_panels(state);
    let dialog_html   = render_dialog(state);
    let funcbar       = render_funcbar();
    // Keytrap invisibili: globali alla finestra, renderizzati una sola volta.
    let keytraps      = render_keytraps();

    // `data-no-autofit`: marker di sola presentazione letto dall'host runtime
    // (`autoFitHeight` in plugin-window.js) per NON auto-adattare l'altezza della
    // finestra ad ogni render — così l'utente può ridimensionarla verticalmente a
    // piacere (un file manager deve essere liberamente ridimensionabile).
    format!(
        r#"<div class="lare-window lc-root" data-no-autofit>
  <style>{INLINE_CSS}</style>
  {keytraps}
  {dialog_html}
  <div class="lc-panels">
    {panel_content}
  </div>
  <div class="lc-funcbar">
    {funcbar}
  </div>
</div>"#
    )
}

/// Genera i due pannelli affiancati.
fn render_panels(state: &LcState) -> String {
    let left_html  = render_panel(&state.left,  Side::Left,  state.active);
    let right_html = render_panel(&state.right, Side::Right, state.active);
    format!("{left_html}\n{right_html}")
}

/// Genera il pannello singolo (header + lista di entry).
fn render_panel(panel: &PanelState, side: Side, active: Side) -> String {
    let side_str = match side { Side::Left => "left", Side::Right => "right" };
    let is_active   = side == active;
    let panel_class = if is_active { "lc-panel lc-panel--active" } else { "lc-panel" };

    // HTML-escape del path: coerente con ogni altra stringa derivata dal
    // filesystem in questo file (un path con '&' o metacaratteri HTML altrimenti
    // corromperebbe il markup / iniezione nella webview su path non-Windows).
    let path_str = html_escape(&panel.path.display().to_string());
    let header = format!(
        r#"<div class="lc-panel-header" data-evt="panel:{side_str}:focus">{path_str}</div>"#
    );

    let error_html = if let Some(ref err) = panel.error {
        format!(r#"<div class="lc-error">{}</div>"#, html_escape(err))
    } else {
        String::new()
    };

    // Renderizza TUTTE le voci del pannello: lo scroll è ora nativo del browser
    // (`.lc-rows { overflow-y: auto }`), non più una finestra di VISIBLE_ROWS righe
    // decisa lato server. Questo è ciò che dà lo scroll con la rotellina del mouse.
    let rows: String = panel.entries.iter().enumerate().map(|(abs_idx, entry)| {
        let is_selected = is_active && abs_idx == panel.cursor;
        let row_class  = if is_selected { "lc-row lc-row--selected" } else { "lc-row" };
        // `aria-selected="true"` SOLO sulla riga selezionata del pannello attivo:
        // è l'aggancio (convenzione ARIA generica) che l'host usa per portarla in
        // vista con scrollIntoView quando la selezione si sposta via tastiera oltre
        // l'area visibile.
        let aria = if is_selected { r#" aria-selected="true""# } else { "" };
        let icon       = if entry.is_dir { "📁 " } else { "📄 " };
        let label      = html_escape(&entry.label());
        let size_str   = if entry.is_dir || entry.name == ".." {
            "     ".to_string()
        } else {
            format_size(entry.size)
        };
        // data-evt   → click singolo: seleziona la riga (panel:side:row:idx).
        // data-dblevt → doppio click: equivale a premere Enter sulla riga (key:Enter).
        //   Presente su OGNI riga, inclusa "..": `state::enter_selected()` no-op sui
        //   file e naviga in su su ".." — comportamento voluto. Riusa la logica già
        //   testata di state.rs: nessun nuovo stato qui.
        format!(
            r#"<div class="{row_class}"{aria} data-evt="panel:{side_str}:row:{abs_idx}" data-dblevt="key:Enter">{icon}<span class="lc-name">{label}</span><span class="lc-size">{size_str}</span></div>"#
        )
    }).collect();

    // Contatore delle voci reali (esclude "..") in fondo al pannello: rimpiazza il
    // vecchio indicatore percentuale, privo di senso ora che non c'è più
    // finestratura server-side. Informazione utile a colpo d'occhio.
    let real_count = panel.entries.iter().filter(|e| e.name != "..").count();
    let count_indicator = format!(r#"<div class="lc-scroll">{real_count} elementi</div>"#);

    format!(
        r#"<div class="{panel_class}">
  {header}
  {error_html}
  <div class="lc-rows">{rows}</div>
  {count_indicator}
</div>"#
    )
}

/// Genera la barra funzioni in basso.
///
/// Ogni pulsante porta DUE attributi:
/// - `data-evt` → l'evento plugin emesso al click del mouse (via il delegated
///   click listener dell'host).
/// - `data-key` → il `KeyboardEvent.key` del tasto fisico corrispondente, così
///   premere F5..F8/ESC dalla tastiera attiva lo stesso evento (via `eventFromKey`).
///   Il valore di `data-key` si ricava togliendo il prefisso "key:" dall'evento
///   (es. "key:F5" → "F5", "key:Escape" → "Escape"), che è esattamente il nome
///   del tasto restituito dal browser.
fn render_funcbar() -> String {
    let buttons = [
        ("key:F5",     "F5 Copia"),
        ("key:F6",     "F6 Sposta"),
        ("key:F7",     "F7 MkDir"),
        ("key:F8",     "F8 Confronta"),
        // Elimina: `data-key="Delete"` = KeyboardEvent.key del tasto fisico Canc.
        // Etichetta nello stile "tasto + verbo italiano" degli altri pulsanti.
        ("key:Delete", "Del Elimina"),
        ("key:Escape", "ESC Chiudi"),
    ];
    buttons.iter().map(|(evt, label)| {
        // "key:F5" → "F5": il KeyboardEvent.key del tasto fisico.
        let key = evt.strip_prefix("key:").unwrap_or(evt);
        format!(r#"<button class="lare-button lc-fn-btn" data-key="{key}" data-evt="{evt}">{label}</button>"#)
    }).collect::<Vec<_>>().join("\n    ")
}

/// Genera i "keytrap": elementi INVISIBILI (`.lc-keytrap { display: none }`) il cui
/// unico scopo è offrire all'host un elemento con `data-key`/`data-evt` per i tasti
/// che NON hanno un pulsante visibile sullo schermo (frecce, PageUp/Down, Tab, Enter,
/// Backspace). Senza di essi l'host — che mappa un keydown solo cercando un elemento
/// `[data-key]` nel DOM — non avrebbe nulla da attivare, e la navigazione da tastiera
/// (l'unica prevista per Lare Commander) sarebbe irraggiungibile.
///
/// Le stringhe `data-evt` (`key:ArrowDown`, …) sono copiate 1:1 dai match arm di
/// `state::handle_ui_event`; `data-key` è il corrispondente `KeyboardEvent.key`.
///
/// Sono globali alla finestra (non per-pannello): renderizzati una sola volta.
fn render_keytraps() -> String {
    // (KeyboardEvent.key, element_id gestito da handle_ui_event)
    let keys = [
        ("ArrowDown", "key:ArrowDown"),
        ("ArrowUp",   "key:ArrowUp"),
        ("PageDown",  "key:PageDown"),
        ("PageUp",    "key:PageUp"),
        ("Tab",       "key:Tab"),
        ("Enter",     "key:Enter"),
        ("Backspace", "key:Backspace"),
        // Home/End (round 5): saltano a inizio/fine lista. `KeyboardEvent.key` dei
        // tasti fisici Home/End è letteralmente "Home"/"End" (standard, senza ambiguità);
        // i `data-evt` combaciano 1:1 con i match arm `key:Home`/`key:End` di state.rs.
        ("Home",      "key:Home"),
        ("End",       "key:End"),
    ];
    keys.iter().map(|(key, evt)| {
        format!(r#"<div class="lc-keytrap" data-key="{key}" data-evt="{evt}"></div>"#)
    }).collect::<Vec<_>>().join("\n  ")
}

// ─── Dialog ───────────────────────────────────────────────────────────────────

/// Genera il pannello modale del dialog (se presente).
fn render_dialog(state: &LcState) -> String {
    match &state.dialog {
        None => String::new(),

        Some(Dialog::ConfirmCopy { src, dst, overwrite }) => {
            let src_s = html_escape(&src.display().to_string());
            let dst_s = html_escape(&dst.display().to_string());
            let warn  = overwrite_warning(*overwrite);
            render_confirm_dialog("Copia",
                &format!("Copia <code>{src_s}</code> in <code>{dst_s}</code>?{warn}"))
        }

        Some(Dialog::ConfirmMove { src, dst, overwrite }) => {
            let src_s = html_escape(&src.display().to_string());
            let dst_s = html_escape(&dst.display().to_string());
            let warn  = overwrite_warning(*overwrite);
            render_confirm_dialog("Sposta",
                &format!("Sposta <code>{src_s}</code> in <code>{dst_s}</code>?{warn}"))
        }

        Some(Dialog::ConfirmDelete { path, is_dir }) => {
            // HTML-escape del path: convenzione di sicurezza già usata da
            // ConfirmCopy/ConfirmMove (un path con metacaratteri corromperebbe
            // altrimenti il markup / iniezione nella webview).
            let path_s = html_escape(&path.display().to_string());
            let what   = if *is_dir { "la cartella" } else { "il file" };
            render_confirm_dialog("Elimina",
                &format!("Eliminare {what} <code>{path_s}</code>?"))
        }

        Some(Dialog::MkdirInput { name }) => {
            let name_esc = html_escape(name);
            format!(
                r#"<div class="lc-dialog">
  <div class="lc-dialog-title lare-label">Nuova Directory</div>
  <div class="lc-dialog-body">
    <!-- `autofocus`: attributo booleano HTML standard (forma bare, senza `="..."`).
         Dà il focus al campo la PRIMA volta che il dialog appare — round 3 aveva
         aggiunto solo la PRESERVAZIONE del focus tra un render e l'altro, ma nulla
         dava il focus iniziale. DOMPurify (host) lo lascia passare perché è in
         ADD_ATTR (vedi plugin-runtime.mjs). Dai render successivi in poi (ad ogni
         tasto), la logica di round 3 in plugin-window.js ri-focussa il campo via
         `data-evt`: i due meccanismi coesistono senza conflitto. -->
    <input class="lare-input mc-mkdir-input" type="text"
           value="{name_esc}" autofocus data-evt="dialog:mkdir:input" placeholder="Nome directory..." />
  </div>
  <div class="lc-dialog-actions">
    <button class="lare-button" data-evt="dialog:confirm:yes">Crea</button>
    <button class="lare-button mc-btn-cancel" data-evt="dialog:confirm:no">Annulla</button>
  </div>
</div>
<div class="mc-overlay"></div>"#
            )
        }

        Some(Dialog::FolderCompare { left_path, right_path, result }) => {
            render_folder_compare(left_path, right_path, result)
        }

        Some(Dialog::FileDiff { diff }) => {
            render_file_diff(diff)
        }

        Some(Dialog::Error { msg }) => {
            let msg_esc = html_escape(msg);
            format!(
                r#"<div class="lc-dialog lc-dialog--error">
  <div class="lc-dialog-title lare-label">Errore</div>
  <div class="lc-dialog-body">{msg_esc}</div>
  <div class="lc-dialog-actions">
    <button class="lare-button" data-evt="dialog:confirm:yes">OK</button>
  </div>
</div>
<div class="mc-overlay"></div>"#
            )
        }
    }
}

/// Genera il dialog di confronto cartelle con 4 colonne.
fn render_folder_compare(
    left_path:  &std::path::Path,
    right_path: &std::path::Path,
    result:     &FolderCompareResult,
) -> String {
    let left_label  = html_escape(&left_path.display().to_string());
    let right_label = html_escape(&right_path.display().to_string());

    let render_list = |items: &[String], empty_msg: &str| -> String {
        if items.is_empty() {
            format!("<em>{}</em>", empty_msg)
        } else {
            items.iter()
                .map(|s| format!("<li>{}</li>", html_escape(s)))
                .collect::<Vec<_>>().join("")
        }
    };

    let only_left_html  = render_list(&result.only_left,  "nessuno");
    let only_right_html = render_list(&result.only_right, "nessuno");
    let identical_html  = render_list(&result.identical,  "nessuno");

    // Colonna "differenze di data": newer_left (⬅) + newer_right (➡)
    let date_diff_html: String = {
        let mut items = Vec::new();
        for n in &result.newer_left  { items.push(format!("<li>⬅ {}</li>", html_escape(n))); }
        for n in &result.newer_right { items.push(format!("<li>➡ {}</li>", html_escape(n))); }
        if items.is_empty() { "<em>nessuna</em>".to_string() }
        else { items.join("") }
    };

    format!(
        r#"<div class="lc-dialog lc-dialog--wide lc-dialog--fullwidth">
  <div class="lc-dialog-title lare-label">Confronto Cartelle</div>
  <div class="lc-dialog-subtitle">{left_label}  ↔  {right_label}</div>
  <div class="lc-dialog-body mc-compare">
    <div class="mc-compare-col">
      <div class="mc-compare-header">Solo sinistra ({n_ol})</div>
      <ul class="mc-compare-list">{only_left_html}</ul>
    </div>
    <div class="mc-compare-col">
      <div class="mc-compare-header">Differenze data</div>
      <ul class="mc-compare-list">{date_diff_html}</ul>
    </div>
    <div class="mc-compare-col">
      <div class="mc-compare-header">Solo destra ({n_or})</div>
      <ul class="mc-compare-list">{only_right_html}</ul>
    </div>
    <div class="mc-compare-col">
      <div class="mc-compare-header">Identici ({n_id})</div>
      <ul class="mc-compare-list">{identical_html}</ul>
    </div>
  </div>
  <div class="lc-dialog-actions">
    <button class="lare-button" data-evt="dialog:confirm:yes">Chiudi</button>
  </div>
</div>
<div class="mc-overlay"></div>"#,
        n_ol = result.only_left.len(),
        n_or = result.only_right.len(),
        n_id = result.identical.len(),
    )
}

/// Genera il dialog di diff file di testo (side-by-side).
fn render_file_diff(diff: &crate::ops::FileDiffResult) -> String {
    let left_name  = html_escape(&diff.left_name);
    let right_name = html_escape(&diff.right_name);

    let truncated_note = if diff.truncated {
        r#"<div class="lc-diff-note">⚠ File troncato a 1000 righe per lato.</div>"#
    } else { "" };

    let rows: String = diff.lines.iter().map(|dl| {
        match dl {
            DiffLine::Same { line } => {
                let l = html_escape(line);
                format!(
                    r#"<tr class="lc-diff-same"><td class="lc-diff-cell">{l}</td><td class="lc-diff-cell">{l}</td></tr>"#
                )
            }
            DiffLine::Left { line } => {
                let l = html_escape(line);
                format!(
                    r#"<tr class="lc-diff-left"><td class="lc-diff-cell">{l}</td><td class="lc-diff-cell"></td></tr>"#
                )
            }
            DiffLine::Right { line } => {
                let r = html_escape(line);
                format!(
                    r#"<tr class="lc-diff-right"><td class="lc-diff-cell"></td><td class="lc-diff-cell">{r}</td></tr>"#
                )
            }
        }
    }).collect();

    format!(
        r#"<div class="lc-dialog lc-dialog--wide lc-dialog--diff">
  <div class="lc-dialog-title lare-label">Diff file di testo</div>
  <div class="lc-diff-stats">
    ➖ {left_only} righe solo a sinistra &nbsp;|&nbsp;
    ➕ {right_only} righe solo a destra &nbsp;|&nbsp;
    ═ {same} righe identiche
  </div>
  {truncated_note}
  <div class="lc-dialog-body lc-diff-body">
    <table class="lc-diff-table">
      <thead>
        <tr>
          <th class="lc-diff-th">{left_name}</th>
          <th class="lc-diff-th">{right_name}</th>
        </tr>
      </thead>
      <tbody>{rows}</tbody>
    </table>
  </div>
  <div class="lc-dialog-actions">
    <button class="lare-button" data-evt="dialog:confirm:yes">Chiudi</button>
  </div>
</div>
<div class="mc-overlay"></div>"#,
        left_only  = diff.left_only,
        right_only = diff.right_only,
        same       = diff.same,
    )
}

/// Riga di avviso mostrata nei dialog di copia/spostamento quando la
/// destinazione esiste già (verrà sovrascritta senza ulteriore conferma).
/// Stringa vuota se non c'è rischio di sovrascrittura.
fn overwrite_warning(overwrite: bool) -> String {
    if overwrite {
        r#"<div class="lc-warning">⚠ Attenzione: la destinazione esiste già e verrà sovrascritta.</div>"#.to_string()
    } else {
        String::new()
    }
}

/// Helper per costruire un dialog di conferma (Sì/No) generico.
fn render_confirm_dialog(title: &str, body: &str) -> String {
    format!(
        r#"<div class="lc-dialog">
  <div class="lc-dialog-title lare-label">{title}</div>
  <div class="lc-dialog-body">{body}</div>
  <div class="lc-dialog-actions">
    <button class="lare-button" data-evt="dialog:confirm:yes">Sì</button>
    <button class="lare-button mc-btn-cancel" data-evt="dialog:confirm:no">No</button>
  </div>
</div>
<div class="mc-overlay"></div>"#
    )
}

/// Formatta una dimensione in byte in una stringa leggibile.
fn format_size(bytes: u64) -> String {
    if bytes < 1_024 {
        format!("{bytes:>6}B")
    } else if bytes < 1_048_576 {
        format!("{:>5.1}K", bytes as f64 / 1_024.0)
    } else if bytes < 1_073_741_824 {
        format!("{:>5.1}M", bytes as f64 / 1_048_576.0)
    } else {
        format!("{:>5.1}G", bytes as f64 / 1_073_741_824.0)
    }
}

/// Escape dei caratteri HTML speciali.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
     .replace('<', "&lt;")
     .replace('>', "&gt;")
     .replace('"', "&quot;")
}

/// CSS inline specifico del plugin.
const INLINE_CSS: &str = r#"
/* Keytrap: elementi invisibili che espongono solo data-key/data-evt all'host
   per i tasti senza pulsante visibile (frecce, PageUp/Down, Tab, Enter, Backspace).
   `display: none` via CLASSE (non style inline): l'allowlist DOMPurify dell'host
   tiene esplicitamente solo `data-evt`/`data-key`, non `style` — un `style` inline
   verrebbe rimosso, una classe è sempre conservata. */
.lc-keytrap { display: none; }

/* ── Layout root ──────────────────────────────────────────────── */
.lc-root {
  display: flex;
  flex-direction: column;
  /* width: 100% → riempie la finestra reale data dall'host (960×620 di default,
     ma anche una finestra ridimensionata dall'utente). Prima era 900px fisso,
     quindi il contenitore ignorava la dimensione della finestra. */
  width: 100%;
  /* height: 100% → riempie l'ALTEZZA reale di `#plugin-root`, il contenitore
     host (in plugin-window.html) che è `flex: 1` + `overflow-y: auto`. Senza
     questa riga `.lc-root` si dimensiona sulla propria altezza di CONTENUTO
     naturale invece di stirarsi a riempire la finestra: due conseguenze osservate
     dal vivo (round 4). (a) Ridimensionando la finestra in verticale, l'altezza
     disponibile di `#plugin-root` cambia ma `.lc-root` non la segue → i pannelli
     non rispondono al resize verticale. (b) Con molte righe, `.lc-root` cresce
     OLTRE `#plugin-root`, e poiché `#plugin-root` ha il proprio `overflow-y: auto`
     è LÌ che compare la scrollbar — attorno all'intero plugin (header, pannelli,
     funcbar) invece che dentro ogni singolo `.lc-rows`. Dando a `.lc-root`
     un'altezza reale e limitata, la catena flex-column (`.lc-panels flex:1` →
     `.lc-panel flex:1 overflow:hidden` → `.lc-rows overflow-y:auto`) ha finalmente
     un'altezza da distribuire e clippare: lo scroll torna dentro il pannello. */
  height: 100%;
  /* min-width difensivo: se la finestra viene resa molto stretta, i due pannelli
     restano leggibili (senza floor collasserebbero sotto una soglia usabile). */
  min-width: 640px;
  min-height: 520px;
  /* Stack font allineato al resto dell'app (0.4.5: Cascadia Mono LIGHT —
     Windows installa ogni peso come famiglia separata, non un font
     variabile, quindi serve il nome esatto 'Cascadia Mono Light' per
     ottenere il peso sottile, non font-weight numerico su 'Cascadia Mono'
     Regular. Fallback a Cascadia Mono Regular poi Consolas se assenti). */
  font-family: 'Cascadia Mono Light', 'Cascadia Mono', 'Consolas', 'Courier New', monospace;
  font-size: 13px;
  /* Sfondo translucido come ogni altra finestra di Lare Terminal. L'alpha non è
     più un valore fisso: deriva da `--window-alpha`, la custom property che
     `plugin-window.js` imposta su `document.documentElement` PRIMA che questo
     HTML venga iniettato in `#plugin-root` (Task 7) — essendo `.lc-root` un
     discendente del documento, la eredita senza bisogno di propagazione propria.
     `--window-alpha` a sua volta viene da `/config` (slider alpha, Task 1-6):
     cambiandolo lì, la trasparenza di `/lc` cambia insieme a tutte le altre
     finestre. Il fallback `0.87` in `var(--window-alpha, 0.87)` copre il caso in
     cui la property non sia (ancora) impostata, e resta lo stesso valore storico
     di riferimento (round 6, vedi CHANGELOG 0.3.5). Prima era l'esadecimale
     PIENAMENTE opaco #1a1a2e (round 4, vanificava la trasparenza), poi 0.92
     (round 4, allineato SOLO alla shell di plugin-window.html), poi 0.87 fisso
     (round 6). Scelte alpha (giudizio rivedibile dal supervisore): i contenitori
     "sempre visibili" (root/panels/panel/header/funcbar) sono translucidi.
     Bordi, hover, selezione, warning/errore e dialog derivano ANCH'ESSI da
     --window-alpha da 0.4.3 (correzione dal vivo: restavano gli unici blocchi
     pienamente opachi in tutta l'app, "rompevano" l'effetto vetro). Restano
     opachi solo lo scrim dietro i dialog e i campi di input testuale. */
  background: rgba(26, 26, 46, var(--window-alpha, 0.87));
  /* Colore testo allineato allo stesso --text usato da ogni altra finestra
     dell'app (era #e0e0e0, grigio puro senza tinta blu — 0.4.4). */
  color: rgba(220, 235, 255, 0.92);
  border: 1px solid rgba(68, 68, 68, var(--window-alpha, 0.87));
  border-radius: 4px;
  overflow: hidden;
}

/* ── Pannelli ──────────────────────────────────────────────────── */
.lc-panels {
  display: flex;
  flex: 1;
  /* min-height: 0 — vedi la nota estesa su `.lc-rows`. Ogni anello della catena
     flex-column deve poter rimpicciolire sotto il proprio contenuto, altrimenti il
     `min-height: auto` di default si propaga e blocca il clipping più in basso. */
  min-height: 0;
  gap: 2px;
  padding: 4px;
  /* Gutter/contenitore dei due pannelli: stesso alpha di --window-alpha di ogni
     altro pannello — niente più scarto relativo (rimosso: l'utente vuole lo
     STESSO valore configurato in /config su ogni finestra, senza eccezioni). */
  background: rgba(17, 17, 17, var(--window-alpha, 0.87));
}

.lc-panel {
  flex: 1;
  /* min-height: 0 — anello centrale della catena (vedi nota su `.lc-rows`). */
  min-height: 0;
  display: flex;
  flex-direction: column;
  /* Superficie del pannello: stesso alpha di --window-alpha di ogni altro
     pannello — niente più scarto relativo. */
  background: rgba(26, 26, 46, var(--window-alpha, 0.87));
  border: 1px solid rgba(51, 51, 51, var(--window-alpha, 0.87));
  border-radius: 2px;
  overflow: hidden;
}

.lc-panel--active {
  border-color: rgba(0, 180, 216, var(--window-alpha, 0.87));
}

.lc-panel-header {
  padding: 3px 8px;
  /* Header del pannello: stesso alpha di --window-alpha di ogni altro
     pannello — niente più scarto relativo. */
  background: rgba(22, 33, 62, var(--window-alpha, 0.87));
  color: #90e0ef;
  font-weight: bold;
  border-bottom: 1px solid rgba(51, 51, 51, var(--window-alpha, 0.87));
  cursor: pointer;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.lc-panel--active .lc-panel-header {
  /* Stessa tinta di accento blu di prima, ma ora con lo stesso alpha di ogni
     altro pannello — l'header del pannello attivo non deve diventare opaco
     solo perché ha il focus. */
  background: rgba(15, 52, 96, var(--window-alpha, 0.87));
  color: #caf0f8;
}

/* ── Righe ─────────────────────────────────────────────────────── */
/* `overflow-y: auto`: la lista scorre nativamente (rotellina del mouse /
   scrollbar). Prima era `overflow: hidden` e il render mostrava solo una
   finestra di 20 righe decisa lato server → non c'era nulla da scrollare.

   `min-height: 0` (round 5) — QUESTA è la riga load-bearing per lo scroll.
   Gotcha classico dei flexbox: un flex item ha `min-height` di DEFAULT = `auto`,
   che significa "alto almeno quanto la mia altezza di contenuto naturale". Questo
   default vince silenziosamente sia su `flex: 1` (che quindi NON riesce a
   rimpicciolire l'item sotto il suo contenuto) sia su `overflow-y: auto` (che
   quindi NON clippa: l'item cresce invece di scrollare). Risultato osservato dal
   vivo: la lista allunga `.lc-rows` → allunga `.lc-panel` → sfora `.lc-panels`
   → spinge la funcbar giù/fuori vista, e la rotellina non ha nulla di limitato su
   cui agire. `min-height: 0` sovrascrive quel default: l'item può finalmente
   rimpicciolire fino allo spazio che la catena flex gli assegna, e a quel punto
   `overflow-y: auto` (e la rotellina nativa) si attivano davvero. Va messo su
   TUTTA la catena `.lc-panels` → `.lc-panel` → `.lc-rows`: se un solo anello resta
   `auto`, il vincolo torna a propagarsi verso il basso e il clipping non parte.

   NB: è la STESSA classe di problema del fix di round 4 (`.lc-root { height: 100% }`)
   — entrambi sono bug di dimensionamento del box model in un layout flex — ma un
   MECCANISMO DIVERSO: round 4 era un'altezza percentuale mancante a monte (il
   contenitore non aveva un'altezza reale da distribuire); questo è il `min-height:
   auto` di default dei flex item, che impedisce il rimpicciolimento a valle. Sono
   due cause distinte, non due facce della stessa. */
.lc-rows {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
}

.lc-row {
  display: flex;
  align-items: center;
  padding: 1px 6px;
  cursor: pointer;
  white-space: nowrap;
}

.lc-row:hover {
  /* Sfondo prima opaco — stessa tinta, ora deriva da --window-alpha come
     ogni altro sfondo pannello (correzione dal vivo, 0.4.3). */
  background: rgba(22, 33, 62, var(--window-alpha, 0.87));
}

.lc-row--selected {
  /* Sfondo prima opaco — stessa tinta, ora deriva da --window-alpha. */
  background: rgba(15, 52, 96, var(--window-alpha, 0.87));
  color: #caf0f8;
  font-weight: bold;
}

.lc-name {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
}

.lc-size {
  color: #888;
  font-size: 11px;
  margin-left: 8px;
  min-width: 52px;
  text-align: right;
}

.lc-scroll {
  text-align: right;
  padding: 2px 6px;
  color: #555;
  font-size: 11px;
  border-top: 1px solid rgba(51, 51, 51, var(--window-alpha, 0.87));
}

.lc-error {
  color: #ff6b6b;
  padding: 4px 8px;
  font-size: 12px;
}

/* ── Barra funzioni ────────────────────────────────────────────── */
.lc-funcbar {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  padding: 4px 8px;
  /* Barra funzioni: stesso alpha di --window-alpha di ogni altro pannello —
     niente più scarto relativo, stesso trattamento dell'header sopra. */
  background: rgba(22, 33, 62, var(--window-alpha, 0.87));
  border-top: 1px solid rgba(51, 51, 51, var(--window-alpha, 0.87));
}

.lc-fn-btn {
  font-size: 12px;
  padding: 3px 10px;
  /* Pulsante funzione: sfondo/bordo prima opachi, ora derivati da
     --window-alpha come ogni altro elemento pannello (correzione dal vivo,
     0.4.3). */
  background: rgba(15, 52, 96, var(--window-alpha, 0.87));
  color: #caf0f8;
  border: 1px solid rgba(0, 180, 216, var(--window-alpha, 0.87));
  border-radius: 3px;
  cursor: pointer;
}

.lc-fn-btn:hover {
  background: rgba(0, 180, 216, var(--window-alpha, 0.87));
  color: #111;
}

/* ── Dialog modale ──────────────────────────────────────────────── */
.mc-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0,0,0,0.5);
  z-index: 10;
}

.lc-dialog {
  position: fixed;
  top: 50%;
  left: 50%;
  transform: translate(-50%, -50%);
  z-index: 20;
  /* Sfondo/bordo del dialog: prima opachi per leggibilità (scelta rivista dal
     vivo, 0.4.3) — ora come `.move-dialog-box`/`.config-dialog-box`,
     derivano da --window-alpha. Lo scrim dietro (`.mc-overlay`) resta opaco:
     è lui a garantire il contrasto, non il box del dialog. */
  background: rgba(22, 33, 62, var(--window-alpha, 0.87));
  border: 1px solid rgba(0, 180, 216, var(--window-alpha, 0.87));
  border-radius: 6px;
  padding: 16px 20px;
  min-width: 340px;
  max-width: 640px;
}

.lc-dialog--wide {
  min-width: 620px;
}

.lc-dialog--fullwidth {
  max-width: 860px;
}

.lc-dialog--error {
  border-color: rgba(255, 107, 107, var(--window-alpha, 0.87));
}

.lc-dialog--diff {
  max-width: 860px;
  max-height: 80vh;
  display: flex;
  flex-direction: column;
}

.lc-dialog-title {
  font-size: 14px;
  font-weight: bold;
  margin-bottom: 6px;
  color: #90e0ef;
}

.lc-dialog-subtitle {
  font-size: 11px;
  color: #777;
  margin-bottom: 10px;
  word-break: break-all;
}

.lc-dialog-body {
  margin-bottom: 14px;
  line-height: 1.5;
}

.lc-dialog-actions {
  display: flex;
  gap: 8px;
  justify-content: flex-end;
}

/* Avviso di sovrascrittura: giallo/ambra, visivamente distinto dal testo di conferma.
   Sfondo/bordo prima opachi, ora derivati da --window-alpha (correzione dal
   vivo, 0.4.3) — stessa tinta, il testo resta pienamente leggibile perché il
   colore giallo (#ffd166) non è mai stato coinvolto nell'alpha. */
.lc-warning {
  margin-top: 10px;
  padding: 6px 10px;
  background: rgba(58, 46, 0, var(--window-alpha, 0.87));
  border: 1px solid rgba(255, 183, 3, var(--window-alpha, 0.87));
  border-radius: 3px;
  color: #ffd166;
  font-weight: bold;
  font-size: 12px;
}

.mc-btn-cancel {
  background: rgba(51, 51, 51, var(--window-alpha, 0.87));
  border-color: rgba(102, 102, 102, var(--window-alpha, 0.87));
  color: #bbb;
}

.mc-btn-cancel:hover {
  background: rgba(85, 85, 85, var(--window-alpha, 0.87));
}

.mc-mkdir-input {
  width: 100%;
  padding: 4px 8px;
  background: #1a1a2e;
  border: 1px solid #00b4d8;
  color: #e0e0e0;
  border-radius: 3px;
  font-family: inherit;
  font-size: 13px;
}

/* ── Confronto cartelle ─────────────────────────────────────────── */
.mc-compare {
  display: flex;
  gap: 12px;
}

.mc-compare-col {
  flex: 1;
}

.mc-compare-header {
  font-weight: bold;
  color: #90e0ef;
  margin-bottom: 6px;
  font-size: 12px;
}

.mc-compare-list {
  margin: 0;
  padding: 0 0 0 16px;
  font-size: 12px;
  max-height: 220px;
  overflow-y: auto;
  color: #ccc;
}

/* ── Diff file di testo ─────────────────────────────────────────── */
.lc-diff-stats {
  font-size: 12px;
  color: #aaa;
  margin-bottom: 8px;
}

.lc-diff-note {
  font-size: 11px;
  color: #f0a;
  margin-bottom: 6px;
}

.lc-diff-body {
  overflow-y: auto;
  max-height: 55vh;
}

.lc-diff-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
  table-layout: fixed;
}

.lc-diff-th {
  /* Header sticky della tabella diff: sfondo prima opaco, ora derivato da
     --window-alpha (correzione dal vivo, 0.4.3). */
  background: rgba(15, 52, 96, var(--window-alpha, 0.87));
  color: #90e0ef;
  padding: 3px 8px;
  text-align: left;
  font-weight: bold;
  position: sticky;
  top: 0;
  z-index: 1;
}

.lc-diff-cell {
  padding: 1px 8px;
  white-space: pre;
  overflow: hidden;
  text-overflow: ellipsis;
  border-bottom: 1px solid rgba(30, 30, 58, var(--window-alpha, 0.87));
  width: 50%;
}

/* Righe della tabella diff (stato per riga: identiche/solo-sinistra/solo-destra) —
   sfondi prima opachi, ora derivati da --window-alpha (correzione dal vivo, 0.4.3). */
.lc-diff-same td {
  background: rgba(26, 26, 46, var(--window-alpha, 0.87));
  color: #ccc;
}

.lc-diff-left td {
  background: rgba(61, 28, 28, var(--window-alpha, 0.87));
  color: #f8a0a0;
}

.lc-diff-right td {
  background: rgba(28, 61, 28, var(--window-alpha, 0.87));
  color: #a0f8a0;
}
"#;

// ─── Test ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{LcState, Dialog};
    use crate::ops::{FolderCompareResult, FileDiffResult, DiffLine};

    fn make_state() -> LcState {
        let tmp = std::env::temp_dir();
        LcState::new(tmp.clone(), tmp, 99)
    }

    #[test]
    fn render_contains_both_paths() {
        let state = make_state();
        let html  = render_window(&state);
        let path_str = state.left.path.display().to_string();
        assert!(html.contains(&path_str), "HTML deve contenere il path: {path_str}");
    }

    #[test]
    fn render_contains_funcbar_buttons() {
        let state = make_state();
        let html  = render_window(&state);
        assert!(html.contains("F5 Copia"));
        assert!(html.contains("F6 Sposta"));
        assert!(html.contains("F7 MkDir"));
        assert!(html.contains("F8 Confronta"));
    }

    #[test]
    fn render_active_panel_has_active_class() {
        let state = make_state();
        let html  = render_window(&state);
        assert!(html.contains("lc-panel--active"));
    }

    #[test]
    fn render_no_dialog_by_default() {
        let state = make_state();
        let html  = render_window(&state);
        assert!(!html.contains("class=\"lc-dialog\""),
            "senza dialog non ci deve essere un elemento lc-dialog");
    }

    #[test]
    fn render_error_dialog() {
        let mut state = make_state();
        state.dialog = Some(Dialog::Error { msg: "Errore di test".to_string() });
        let html = render_window(&state);
        assert!(html.contains("lc-dialog"));
        assert!(html.contains("Errore di test"));
    }

    #[test]
    fn render_folder_compare_shows_columns() {
        use std::path::PathBuf;
        let mut state = make_state();
        state.dialog = Some(Dialog::FolderCompare {
            left_path:  PathBuf::from("/sinistra"),
            right_path: PathBuf::from("/destra"),
            result: FolderCompareResult {
                only_left:   vec!["a.txt".to_string()],
                only_right:  vec!["b.txt".to_string()],
                newer_left:  vec!["c.txt".to_string()],
                newer_right: vec![],
                identical:   vec!["d.txt".to_string()],
            },
        });
        let html = render_window(&state);
        assert!(html.contains("a.txt"),  "only_left deve comparire");
        assert!(html.contains("b.txt"),  "only_right deve comparire");
        assert!(html.contains("⬅ c.txt"), "newer_left deve comparire con icona ⬅");
        assert!(html.contains("d.txt"),  "identical deve comparire");
        assert!(html.contains("Solo sinistra"));
        assert!(html.contains("Differenze data"));
        assert!(html.contains("Solo destra"));
        assert!(html.contains("Identici"));
    }

    #[test]
    fn render_file_diff_shows_filenames() {
        let mut state = make_state();
        state.dialog = Some(Dialog::FileDiff {
            diff: FileDiffResult {
                left_name:  "left.txt".to_string(),
                right_name: "right.txt".to_string(),
                lines: vec![
                    DiffLine::Same  { line: "comune".to_string() },
                    DiffLine::Left  { line: "solo-sx".to_string() },
                    DiffLine::Right { line: "solo-dx".to_string() },
                ],
                left_only:  1,
                right_only: 1,
                same:       1,
                truncated:  false,
            },
        });
        let html = render_window(&state);
        assert!(html.contains("left.txt"),  "nome file sinistro deve comparire");
        assert!(html.contains("right.txt"), "nome file destro deve comparire");
        assert!(html.contains("lc-diff-left"),  "riga Left deve avere classe lc-diff-left");
        assert!(html.contains("lc-diff-right"), "riga Right deve avere classe lc-diff-right");
        assert!(html.contains("lc-diff-same"),  "riga Same deve avere classe lc-diff-same");
        assert!(html.contains("solo-sx"),  "contenuto riga Left deve comparire");
        assert!(html.contains("solo-dx"),  "contenuto riga Right deve comparire");
    }

    #[test]
    fn render_mkdir_dialog() {
        let mut state = make_state();
        state.dialog = Some(Dialog::MkdirInput { name: "nuova_dir".to_string() });
        let html = render_window(&state);
        assert!(html.contains("Nuova Directory"));
        assert!(html.contains("nuova_dir"));
    }

    #[test]
    fn render_mkdir_input_has_autofocus() {
        // L'input MkDir deve portare l'attributo booleano `autofocus`: è ciò che
        // gli dà il focus la PRIMA volta che il dialog appare (la preservazione
        // del focus di round 3 agisce solo dai render successivi in poi). Verifica
        // che `autofocus` sia proprio sull'input MkDir (legato a `data-evt`), non
        // altrove nel markup.
        let mut state = make_state();
        state.dialog = Some(Dialog::MkdirInput { name: String::new() });
        let html = render_window(&state);
        assert!(html.contains("autofocus data-evt=\"dialog:mkdir:input\""),
            "l'input MkDir deve avere `autofocus` per il focus iniziale");
    }

    // ── Part C (round 4): dialog di conferma eliminazione + pulsante Del ──────

    #[test]
    fn render_confirm_delete_dialog_shows_title_and_escaped_path() {
        use std::path::PathBuf;
        let mut state = make_state();
        // Path con '&' per verificare l'escape HTML (come per ConfirmCopy/Move).
        state.dialog = Some(Dialog::ConfirmDelete {
            path: PathBuf::from("/a&b/file.txt"),
            is_dir: false,
        });
        let html = render_window(&state);
        assert!(html.contains("Elimina"), "il dialog deve avere il titolo 'Elimina'");
        assert!(html.contains("a&amp;b"), "il path nel dialog deve essere HTML-escaped");
        assert!(html.contains("il file"), "per un file il testo deve dire 'il file'");
    }

    #[test]
    fn render_confirm_delete_dialog_says_folder_when_dir() {
        use std::path::PathBuf;
        let mut state = make_state();
        state.dialog = Some(Dialog::ConfirmDelete {
            path: PathBuf::from("/tmp/cartella"),
            is_dir: true,
        });
        let html = render_window(&state);
        assert!(html.contains("la cartella"),
            "per una directory il testo deve dire 'la cartella'");
    }

    #[test]
    fn funcbar_has_delete_button() {
        // La barra funzioni deve esporre un pulsante Elimina attivabile sia col
        // mouse (data-evt) sia col tasto fisico Delete (data-key). Le stringhe
        // devono combaciare 1:1 con l'arm "key:Delete" di state::handle_ui_event.
        let state = make_state();
        let html  = render_window(&state);
        assert!(html.contains("data-key=\"Delete\""),
            "manca il pulsante con data-key=\"Delete\" nella funcbar");
        assert!(html.contains("data-evt=\"key:Delete\""),
            "manca il pulsante con data-evt=\"key:Delete\" nella funcbar");
    }

    // ── Fix 4: avviso di sovrascrittura su ConfirmCopy/ConfirmMove ────────────

    #[test]
    fn render_confirm_copy_shows_overwrite_warning_only_when_true() {
        use std::path::PathBuf;
        // overwrite = true → l'avviso deve comparire.
        let mut state = make_state();
        state.dialog = Some(Dialog::ConfirmCopy {
            src: PathBuf::from("/a/x.txt"),
            dst: PathBuf::from("/b/x.txt"),
            overwrite: true,
        });
        let html = render_window(&state);
        assert!(html.contains("sovrascritta"),
            "con overwrite=true deve comparire l'avviso di sovrascrittura");

        // overwrite = false → nessun avviso.
        let mut state2 = make_state();
        state2.dialog = Some(Dialog::ConfirmCopy {
            src: PathBuf::from("/a/x.txt"),
            dst: PathBuf::from("/b/x.txt"),
            overwrite: false,
        });
        let html2 = render_window(&state2);
        assert!(!html2.contains("sovrascritta"),
            "con overwrite=false NON deve comparire l'avviso");
    }

    #[test]
    fn render_confirm_move_shows_overwrite_warning() {
        use std::path::PathBuf;
        let mut state = make_state();
        state.dialog = Some(Dialog::ConfirmMove {
            src: PathBuf::from("/a/x.txt"),
            dst: PathBuf::from("/b/x.txt"),
            overwrite: true,
        });
        let html = render_window(&state);
        assert!(html.contains("sovrascritta"),
            "ConfirmMove con overwrite=true deve mostrare l'avviso");
    }

    #[test]
    fn html_escape_escapes_special_chars() {
        assert_eq!(html_escape("<b>&\""), "&lt;b&gt;&amp;&quot;");
    }

    // ── Fix 5: escape del path nell'header del pannello ───────────────────────

    #[test]
    fn render_panel_header_path_is_html_escaped() {
        use std::path::PathBuf;
        // Un path con '&' deve comparire escaped nell'header, come ogni altra
        // stringa derivata dal filesystem in questo file.
        let state = LcState::new(PathBuf::from("/tmp/a&b"), PathBuf::from("/tmp/other"), 3);
        let html  = render_window(&state);
        assert!(html.contains("a&amp;b"),
            "il path dell'header del pannello deve essere HTML-escaped");
    }

    #[test]
    fn format_size_formats_correctly() {
        assert_eq!(format_size(0).trim(),         "0B");
        assert_eq!(format_size(512).trim(),       "512B");
        assert_eq!(format_size(1024).trim(),      "1.0K");
        assert_eq!(format_size(1_500_000).trim(), "1.4M");
    }

    #[test]
    fn render_selected_row_has_selected_class() {
        let base = std::env::temp_dir().join("lc_render_row_test");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("test.txt"), b"").unwrap();
        let mut state = LcState::new(base.clone(), base.clone(), 1);
        state.left.cursor = 1;
        let html = render_window(&state);
        assert!(html.contains("lc-row--selected"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn render_data_evt_for_rows() {
        let base = std::env::temp_dir().join("lc_render_evt_test");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("alpha.txt"), b"").unwrap();
        let state = LcState::new(base.clone(), base.clone(), 2);
        let html  = render_window(&state);
        assert!(html.contains("data-evt=\"panel:left:row:0\""));
        let _ = std::fs::remove_dir_all(&base);
    }

    // ── Slice UX: wiring tastiera / doppio-click / layout ─────────────────────
    //
    // Questi test sono la REGRESSIONE che avrebbe colto il bug originale: la logica
    // in state.rs era corretta e testata, ma l'HTML generato non offriva ALCUN modo
    // di attivare le frecce/PageUp/PageDown/Tab/Enter/Backspace da tastiera, né il
    // doppio-click. I test qui verificano il MARKUP prodotto, non solo la logica.

    #[test]
    fn funcbar_buttons_have_both_data_evt_and_data_key() {
        // Ogni pulsante della barra funzioni deve essere attivabile sia col mouse
        // (data-evt) sia premendo il tasto fisico (data-key = KeyboardEvent.key).
        // Prima della fix i pulsanti avevano solo data-evt → F5..F8/ESC da tastiera
        // non funzionavano.
        let state = make_state();
        let html  = render_window(&state);
        for (evt, key) in [
            ("key:F5", "F5"),
            ("key:F6", "F6"),
            ("key:F7", "F7"),
            ("key:F8", "F8"),
            ("key:Escape", "Escape"),
        ] {
            assert!(html.contains(&format!("data-evt=\"{evt}\"")),
                "manca data-evt=\"{evt}\" nella funcbar");
            assert!(html.contains(&format!("data-key=\"{key}\"")),
                "manca data-key=\"{key}\" (il tasto fisico deve attivare il pulsante)");
        }
    }

    #[test]
    fn keytraps_present_once_each() {
        // I 7 "keytrap" invisibili wire-ano i tasti SENZA pulsante visibile. Devono
        // comparire ESATTAMENTE una volta ciascuno, con la coppia data-key/data-evt
        // che `state::handle_ui_event` sa gestire (stringhe copiate 1:1 dal match).
        let state = make_state();
        let html  = render_window(&state);
        for (key, evt) in [
            ("ArrowDown", "key:ArrowDown"),
            ("ArrowUp",   "key:ArrowUp"),
            ("PageDown",  "key:PageDown"),
            ("PageUp",    "key:PageUp"),
            ("Tab",       "key:Tab"),
            ("Enter",     "key:Enter"),
            ("Backspace", "key:Backspace"),
            // Round 5 Part B: Home/End (salto a inizio/fine lista). `KeyboardEvent.key`
            // dei tasti fisici Home/End è letteralmente "Home"/"End".
            ("Home",      "key:Home"),
            ("End",       "key:End"),
        ] {
            let needle = format!("data-key=\"{key}\" data-evt=\"{evt}\"");
            let count  = html.matches(&needle).count();
            assert_eq!(count, 1,
                "il keytrap {key} deve comparire esattamente una volta (trovato {count})");
        }
    }

    #[test]
    fn rows_carry_double_click_enter() {
        // Ogni riga (inclusa "..") deve avere data-dblevt="key:Enter": il doppio
        // click = selezione (click nativo) + Enter (riusa la logica di state.rs).
        let base = std::env::temp_dir().join("lc_render_dblevt_test");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("beta.txt"), b"").unwrap();
        // Almeno 2 entry per pannello: ".." (riga 0) + beta.txt.
        let state = LcState::new(base.clone(), base.clone(), 5);
        let html  = render_window(&state);
        let count = html.matches("data-dblevt=\"key:Enter\"").count();
        assert!(count >= 2,
            "ogni riga (inclusa \"..\") deve avere data-dblevt (trovate {count} occorrenze)");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn lc_root_css_fills_width_not_fixed_900() {
        // `.lc-root` deve riempire la finestra (width: 100%) invece del vecchio
        // valore fisso 900px, così il layout risponde alla dimensione reale.
        let state = make_state();
        let html  = render_window(&state);
        // Estrazione mirata del blocco `.lc-root { ... }`: evita falsi positivi da
        // altre regole che usano `width: 100%` (es. .mc-mkdir-input).
        let start = html.find(".lc-root {").expect("blocco .lc-root presente");
        let rest  = &html[start..];
        let end   = rest.find('}').expect("chiusura blocco .lc-root");
        let block = &rest[..end];
        assert!(block.contains("width: 100%"),
            ".lc-root deve riempire la larghezza (width: 100%)");
        assert!(!block.contains("width: 900px"),
            ".lc-root non deve avere una larghezza fissa 900px");
    }

    #[test]
    fn lc_root_css_fills_height_of_real_container() {
        // `.lc-root` deve avere `height: 100%` per riempire l'altezza reale di
        // `#plugin-root` (il contenitore host, che è esso stesso `flex:1` +
        // `overflow-y:auto`). Senza, `.lc-root` si dimensiona sul PROPRIO
        // contenuto: il resize verticale della finestra non si propaga ai
        // pannelli e la scrollbar compare attorno all'intero plugin (livello
        // sbagliato) invece che dentro ogni `.lc-rows`. Vedi Part A round 4.
        let state = make_state();
        let html  = render_window(&state);
        // Estrazione mirata del blocco `.lc-root { ... }` (come nel test width):
        // evita falsi positivi da altre regole che usano `height`.
        let start = html.find(".lc-root {").expect("blocco .lc-root presente");
        let rest  = &html[start..];
        let end   = rest.find('}').expect("chiusura blocco .lc-root");
        let block = &rest[..end];
        assert!(block.contains("height: 100%"),
            ".lc-root deve riempire l'altezza del contenitore reale (height: 100%)");
    }

    // ── Round 5 Part A: min-height:0 sulla catena flex (scroll dentro il pannello) ──
    //
    // Regressione del bug osservato dal vivo: la lista directory ALLUNGA il pannello
    // invece di essere clippata e scrollata al suo interno → la funcbar viene spinta
    // giù/fuori vista. Root cause: ogni flex item ha `min-height: auto` di default
    // (= "alto almeno quanto il mio contenuto"), che silenziosamente vince su
    // `flex: 1` (impedisce di rimpicciolire sotto il contenuto) e su `overflow-y: auto`
    // (cresce invece di clippare). Serve `min-height: 0` su TUTTA la catena
    // `.lc-panels` → `.lc-panel` → `.lc-rows`, non su un solo anello.

    /// Helper di test: estrae il corpo `{ ... }` della regola CSS il cui prefisso è
    /// `selector_with_brace` (da passare CON la graffa aperta, es. `".lc-rows {"`).
    /// Passare la graffa evita collisioni fra selettori con prefisso comune: `".lc-panel {"`
    /// NON combacia con `".lc-panels {"`, `".lc-panel--active {"` né `".lc-panel-header {"`.
    /// Rispecchia l'estrazione mirata dei test `.lc-root`: cercare `min-height: 0` come
    /// sottostringa globale del CSS darebbe solo un falso senso di precisione (potrebbe
    /// combaciare altrove), quindi si isola il blocco della singola regola e si asserisce lì.
    fn css_block<'a>(html: &'a str, selector_with_brace: &str) -> &'a str {
        let start = html
            .find(selector_with_brace)
            .unwrap_or_else(|| panic!("il blocco {selector_with_brace} deve essere presente"));
        let rest = &html[start..];
        let end = rest.find('}').expect("chiusura del blocco CSS ('}')");
        &rest[..end]
    }

    #[test]
    fn lc_panels_css_has_min_height_zero() {
        let state = make_state();
        let html = render_window(&state);
        let block = css_block(&html, ".lc-panels {");
        assert!(
            block.contains("min-height: 0"),
            ".lc-panels deve avere min-height: 0 (anello a monte della catena flex)"
        );
    }

    #[test]
    fn lc_panel_css_has_min_height_zero() {
        let state = make_state();
        let html = render_window(&state);
        // ".lc-panel {" (con lo spazio prima della graffa) isola ESATTAMENTE la regola
        // `.lc-panel`, senza collidere con `.lc-panels`/`.lc-panel--active`/`.lc-panel-header`.
        let block = css_block(&html, ".lc-panel {");
        assert!(
            block.contains("min-height: 0"),
            ".lc-panel deve avere min-height: 0 (anello centrale della catena flex)"
        );
    }

    #[test]
    fn lc_rows_css_has_min_height_zero() {
        let state = make_state();
        let html = render_window(&state);
        let block = css_block(&html, ".lc-rows {");
        assert!(
            block.contains("min-height: 0"),
            ".lc-rows deve avere min-height: 0 (l'anello che clippa e scrolla il contenuto)"
        );
    }

    // ── Part E: trasparenza alpha (come ogni altra finestra di Lare Terminal) ──

    #[test]
    fn lc_root_background_is_translucent() {
        let state = make_state();
        let html  = render_window(&state);
        // Estrazione mirata del blocco `.lc-root { ... }` (come nel test width).
        let start = html.find(".lc-root {").expect("blocco .lc-root presente");
        let rest  = &html[start..];
        let end   = rest.find('}').expect("chiusura blocco .lc-root");
        let block = &rest[..end];
        assert!(block.contains("rgba("),
            ".lc-root deve avere uno sfondo translucido (rgba con alpha)");
        assert!(!block.contains("background: #1a1a2e;"),
            ".lc-root non deve piu' avere lo sfondo opaco #1a1a2e");
    }

    // ── Task 8: alpha configurabile — deriva da --window-alpha (ereditata dall'host) ──
    //
    // Prima di questo task gli alpha erano valori `rgba(...)` letterali (0.87/0.55/0.78/0.85),
    // scelti manualmente per allinearsi allo standard "0.87" dell'app (vedi 0.3.5 sopra). Ma
    // erano comunque FISSI: cambiando l'alpha da `/config` (Task 1-7, custom property
    // `--window-alpha` su `document.documentElement`), `/lc` non lo seguiva. Questi 5 test
    // verificano che ogni sfondo derivi ora dalla custom property ereditata, con lo scarto
    // relativo storico preservato come costante nella `calc()`.

    #[test]
    fn lc_root_background_uses_window_alpha() {
        let state = make_state();
        let html  = render_window(&state);
        let block = css_block(&html, ".lc-root {");
        assert!(
            block.contains("var(--window-alpha"),
            ".lc-root deve derivare l'alpha da --window-alpha"
        );
    }

    #[test]
    fn lc_panels_background_uses_window_alpha_flat() {
        // Nessuno scarto relativo: l'utente vuole lo STESSO alpha configurato
        // in /config su ogni pannello, senza eccezioni (correzione post-live-test).
        let state = make_state();
        let html  = render_window(&state);
        let block = css_block(&html, ".lc-panels {");
        assert!(
            block.contains("var(--window-alpha, 0.87)") && !block.contains("calc("),
            ".lc-panels deve usare var(--window-alpha, 0.87) senza scarto/calc"
        );
    }

    #[test]
    fn lc_panel_background_uses_window_alpha_flat() {
        let state = make_state();
        let html  = render_window(&state);
        let block = css_block(&html, ".lc-panel {");
        assert!(
            block.contains("var(--window-alpha, 0.87)") && !block.contains("calc("),
            ".lc-panel deve usare var(--window-alpha, 0.87) senza scarto/calc"
        );
    }

    #[test]
    fn lc_panel_header_background_uses_window_alpha_flat() {
        let state = make_state();
        let html  = render_window(&state);
        let block = css_block(&html, ".lc-panel-header {");
        assert!(
            block.contains("var(--window-alpha, 0.87)") && !block.contains("calc("),
            ".lc-panel-header deve usare var(--window-alpha, 0.87) senza scarto/calc"
        );
    }

    #[test]
    fn lc_funcbar_background_uses_window_alpha_flat() {
        let state = make_state();
        let html  = render_window(&state);
        let block = css_block(&html, ".lc-funcbar {");
        assert!(
            block.contains("var(--window-alpha, 0.87)") && !block.contains("calc("),
            ".lc-funcbar deve usare var(--window-alpha, 0.87) senza scarto/calc"
        );
    }

    #[test]
    fn lc_panel_active_header_background_uses_window_alpha() {
        // Regressione: l'header del pannello ATTIVO (`.lc-panel--active .lc-panel-header`)
        // aveva uno sfondo opaco `#0f3460`, mai toccato dal lavoro sull'alpha configurabile
        // — l'utente l'ha notato dal vivo (screenshot: pannello sinistro attivo, alpha
        // "del tutto assente" sull'header col path corrente). Deve derivare da
        // --window-alpha come ogni altro sfondo pannello, senza opaco hardcoded.
        let state = make_state();
        let html  = render_window(&state);
        let block = css_block(&html, ".lc-panel--active .lc-panel-header {");
        assert!(
            block.contains("var(--window-alpha, 0.87)"),
            ".lc-panel--active .lc-panel-header deve derivare l'alpha da --window-alpha"
        );
        assert!(
            !block.contains("#0f3460"),
            ".lc-panel--active .lc-panel-header non deve piu' avere lo sfondo opaco #0f3460"
        );
    }

    // ── 0.4.3: bordi/hover/selezione/warning/dialog — tutto il resto del
    // plugin era rimasto pienamente opaco (hex senza alpha), l'unico posto
    // dell'app dove l'effetto vetro si "rompeva" del tutto. L'utente l'ha
    // segnalato dal vivo su hover/selezione delle righe. Questi test coprono
    // ogni sfondo/bordo rimasto opaco, tranne lo scrim (`.mc-overlay`) e il
    // campo di input (`.mc-mkdir-input`), esclusi per decisione esplicita.

    #[test]
    fn lc_row_hover_and_selected_use_window_alpha() {
        let state = make_state();
        let html  = render_window(&state);
        let hover = css_block(&html, ".lc-row:hover {");
        assert!(hover.contains("var(--window-alpha, 0.87)") && !hover.contains("#16213e"),
            ".lc-row:hover deve derivare l'alpha da --window-alpha, non piu' opaco");
        let selected = css_block(&html, ".lc-row--selected {");
        assert!(selected.contains("var(--window-alpha, 0.87)") && !selected.contains("#0f3460"),
            ".lc-row--selected deve derivare l'alpha da --window-alpha, non piu' opaco");
    }

    #[test]
    fn lc_dialog_background_and_border_use_window_alpha() {
        let state = make_state();
        let html  = render_window(&state);
        let block = css_block(&html, ".lc-dialog {");
        assert!(block.contains("var(--window-alpha, 0.87)") && !block.contains("#16213e") && !block.contains("#00b4d8"),
            ".lc-dialog deve derivare l'alpha da --window-alpha, non piu' opaco");
    }

    #[test]
    fn lc_dialog_error_border_uses_window_alpha() {
        let state = make_state();
        let html  = render_window(&state);
        let block = css_block(&html, ".lc-dialog--error {");
        assert!(block.contains("var(--window-alpha, 0.87)") && !block.contains("#ff6b6b"),
            ".lc-dialog--error deve derivare l'alpha da --window-alpha, non piu' opaco");
    }

    #[test]
    fn lc_warning_background_and_border_use_window_alpha() {
        let state = make_state();
        let html  = render_window(&state);
        let block = css_block(&html, ".lc-warning {");
        assert!(block.contains("var(--window-alpha, 0.87)") && !block.contains("#3a2e00") && !block.contains("#ffb703"),
            ".lc-warning deve derivare l'alpha da --window-alpha, non piu' opaco");
        assert!(block.contains("#ffd166"),
            ".lc-warning deve mantenere il testo giallo pienamente leggibile (non coinvolto nell'alpha)");
    }

    #[test]
    fn lc_fn_btn_uses_window_alpha() {
        let state = make_state();
        let html  = render_window(&state);
        let block = css_block(&html, ".lc-fn-btn {");
        assert!(block.contains("var(--window-alpha, 0.87)") && !block.contains("#0f3460") && !block.contains("#00b4d8"),
            ".lc-fn-btn deve derivare l'alpha da --window-alpha, non piu' opaco");
        let hover = css_block(&html, ".lc-fn-btn:hover {");
        assert!(hover.contains("var(--window-alpha, 0.87)") && !hover.contains("#00b4d8"),
            ".lc-fn-btn:hover deve derivare l'alpha da --window-alpha, non piu' opaco");
    }

    #[test]
    fn lc_cancel_btn_uses_window_alpha() {
        let state = make_state();
        let html  = render_window(&state);
        let block = css_block(&html, ".mc-btn-cancel {");
        assert!(block.contains("var(--window-alpha, 0.87)") && !block.contains("#333") && !block.contains("#666"),
            ".mc-btn-cancel deve derivare l'alpha da --window-alpha, non piu' opaco");
        let hover = css_block(&html, ".mc-btn-cancel:hover {");
        assert!(hover.contains("var(--window-alpha, 0.87)") && !hover.contains("#555"),
            ".mc-btn-cancel:hover deve derivare l'alpha da --window-alpha, non piu' opaco");
    }

    #[test]
    fn lc_diff_table_uses_window_alpha() {
        let state = make_state();
        let html  = render_window(&state);
        let th = css_block(&html, ".lc-diff-th {");
        assert!(th.contains("var(--window-alpha, 0.87)") && !th.contains("#0f3460"),
            ".lc-diff-th deve derivare l'alpha da --window-alpha, non piu' opaco");
        let cell = css_block(&html, ".lc-diff-cell {");
        assert!(cell.contains("var(--window-alpha, 0.87)") && !cell.contains("#1e1e3a"),
            ".lc-diff-cell deve derivare l'alpha da --window-alpha, non piu' opaco");
        let same = css_block(&html, ".lc-diff-same td {");
        assert!(same.contains("var(--window-alpha, 0.87)") && !same.contains("#1a1a2e"),
            ".lc-diff-same td deve derivare l'alpha da --window-alpha, non piu' opaco");
        let left = css_block(&html, ".lc-diff-left td {");
        assert!(left.contains("var(--window-alpha, 0.87)") && !left.contains("#3d1c1c"),
            ".lc-diff-left td deve derivare l'alpha da --window-alpha, non piu' opaco");
        let right = css_block(&html, ".lc-diff-right td {");
        assert!(right.contains("var(--window-alpha, 0.87)") && !right.contains("#1c3d1c"),
            ".lc-diff-right td deve derivare l'alpha da --window-alpha, non piu' opaco");
    }

    #[test]
    fn lc_panel_borders_use_window_alpha() {
        let state = make_state();
        let html  = render_window(&state);
        let root = css_block(&html, ".lc-root {");
        assert!(root.contains("border: 1px solid rgba(") && root.contains("var(--window-alpha, 0.87)"),
            ".lc-root deve avere il bordo derivato da --window-alpha, non piu' #444 opaco");
        let panel = css_block(&html, ".lc-panel {");
        assert!(panel.contains("border: 1px solid rgba(") && panel.contains("var(--window-alpha, 0.87)"),
            ".lc-panel deve avere il bordo derivato da --window-alpha, non piu' #333 opaco");
        let active = css_block(&html, ".lc-panel--active {");
        assert!(active.contains("var(--window-alpha, 0.87)") && !active.contains("#00b4d8"),
            ".lc-panel--active deve avere il border-color derivato da --window-alpha, non piu' opaco");
        let header = css_block(&html, ".lc-panel-header {");
        assert!(header.contains("border-bottom: 1px solid rgba(") && header.contains("var(--window-alpha, 0.87)"),
            ".lc-panel-header deve avere il border-bottom derivato da --window-alpha, non piu' #333 opaco");
    }

    #[test]
    fn lc_scrim_and_input_field_stay_fully_opaque() {
        // Decisione esplicita (2026-07-14): lo scrim dietro i dialog e il
        // campo di input testuale restano OPACHI — il primo per garantire il
        // dimming dietro il dialog, il secondo per restare leggibile a ogni
        // valore di alpha. Test di blocco: se qualcuno li convertisse per
        // errore in un giro futuro, questo fallisce.
        let state = make_state();
        let html  = render_window(&state);
        let overlay = css_block(&html, ".mc-overlay {");
        assert!(!overlay.contains("var(--window-alpha"),
            ".mc-overlay (scrim) deve restare opaco, non derivare da --window-alpha");
        let input = css_block(&html, ".mc-mkdir-input {");
        assert!(!input.contains("var(--window-alpha"),
            ".mc-mkdir-input (campo di testo) deve restare opaco, non derivare da --window-alpha");
    }

    // ── Part D: scroll nativo — nessun troncamento server-side ────────────────

    /// Regressione (headline Part D): con più di VISIBLE_ROWS (20) voci, TUTTE le
    /// righe devono essere renderizzate (lo scroll lo gestisce nativamente il
    /// browser). Prima la lista era troncata a una finestra di 20 righe, quindi la
    /// riga con indice 24 (25ª voce) NON compariva → niente da scrollare.
    #[test]
    fn render_shows_all_entries_not_capped_at_visible_rows() {
        let base = std::env::temp_dir().join("lc_render_all_rows_test");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        // 24 file + ".." = 25 voci (indici 0..24).
        for i in 0..24 {
            std::fs::write(base.join(format!("file{i:02}.txt")), b"").unwrap();
        }
        let state = LcState::new(base.clone(), base.clone(), 1);
        assert_eq!(state.left.entries.len(), 25, "precondizione: 25 voci (.. + 24 file)");
        let html = render_window(&state);
        assert!(html.contains("data-evt=\"panel:left:row:24\""),
            "la 25ª riga (indice 24) deve essere renderizzata: niente cap a VISIBLE_ROWS");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// La riga selezionata porta `aria-selected="true"` (aggancio per lo
    /// scrollIntoView generico dell'host, Part A4); le altre no.
    #[test]
    fn render_selected_row_has_aria_selected() {
        let base = std::env::temp_dir().join("lc_render_aria_test");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("uno.txt"), b"").unwrap();
        std::fs::write(base.join("due.txt"), b"").unwrap();
        let mut state = LcState::new(base.clone(), base.clone(), 1);
        state.left.cursor = 1; // seleziona una riga specifica nel pannello attivo
        let html = render_window(&state);
        // Esattamente una riga selezionata nel pannello attivo (left).
        assert_eq!(html.matches("aria-selected=\"true\"").count(), 1,
            "una sola riga deve portare aria-selected=\"true\"");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// `.lc-root` porta `data-no-autofit`: marker (di sola presentazione) letto
    /// dall'host runtime per disattivare l'auto-fit dell'altezza su questa
    /// finestra, così il resize verticale manuale dell'utente è rispettato.
    #[test]
    fn render_root_has_no_autofit_marker() {
        let state = make_state();
        let html  = render_window(&state);
        assert!(html.contains("data-no-autofit"),
            ".lc-root deve portare data-no-autofit (opt-out auto-fit altezza)");
    }

    #[test]
    fn font_family_matches_app_stack() {
        // Il font deve allinearsi al nuovo standard unico app-wide (0.4.5):
        // 'Cascadia Mono Light', 'Cascadia Mono', 'Consolas', 'Courier New', monospace.
        let state = make_state();
        let html  = render_window(&state);
        assert!(
            html.contains("'Cascadia Mono Light', 'Cascadia Mono', 'Consolas', 'Courier New', monospace"),
            "il font deve usare il nuovo stack standard dell'app (Cascadia Mono Light primo)"
        );
        assert!(!html.contains("'Courier New', Courier, monospace"),
            "il vecchio stack font non deve piu' comparire");
    }

    #[test]
    fn lc_root_text_color_matches_app_text_token() {
        // Regressione: .lc-root usava un grigio puro (#e0e0e0) senza la tinta
        // blu del token --text condiviso da ogni altra finestra dell'app.
        // L'utente l'ha notato dal vivo confrontando /config con Lare Commander.
        let state = make_state();
        let html  = render_window(&state);
        let block = css_block(&html, ".lc-root {");
        assert!(block.contains("color: rgba(220, 235, 255, 0.92)"),
            ".lc-root deve usare lo stesso colore testo delle altre finestre");
    }
}
