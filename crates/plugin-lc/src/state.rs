//! state.rs — Stato del plugin Lare Commander.
//!
//! `LcState` è l'intero stato mutabile del plugin. `handle` è la funzione
//! centrale: riceve un messaggio dall'host e restituisce le risposte.
//!
//! Struttura:
//!   - `LcState`    — stato globale (due pannelli + pannello attivo + dialog + storage_dir)
//!   - `PanelState` — stato di un singolo pannello (path, listing, cursore)
//!   - `Side`       — quale pannello è attivo (Left / Right)
//!   - `Dialog`     — dialog modale (conferma, input, compare, diff, errore)
//!   - `handle`     — dispatcher principale

use std::path::PathBuf;
use plugin_protocol::{HostToPlugin, PluginToHost};
use crate::fs::{self, Entry};
use crate::render::render_window;
use crate::ops;
use crate::config::LcConfig;

// ─── Tipi di stato ────────────────────────────────────────────────────────────

/// Quale pannello ha il "focus".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side { Left, Right }

impl Side {
    pub fn other(self) -> Self {
        match self { Side::Left => Side::Right, Side::Right => Side::Left }
    }
}

/// Stato di un singolo pannello.
#[derive(Debug, Clone)]
pub struct PanelState {
    pub path:    PathBuf,
    pub entries: Vec<Entry>,
    pub cursor:  usize,
    pub error:   Option<String>,
}

impl PanelState {
    pub fn new(path: PathBuf) -> Self {
        let (entries, error) = match fs::list_dir(&path) {
            Ok(e)    => (e, None),
            Err(err) => (Vec::new(), Some(err.to_string())),
        };
        PanelState { path, entries, cursor: 0, error }
    }

    pub fn refresh(&mut self) {
        match fs::list_dir(&self.path) {
            Ok(e) => {
                self.entries = e;
                self.cursor  = self.cursor.min(self.entries.len().saturating_sub(1));
                self.error   = None;
            }
            Err(err) => {
                self.entries.clear();
                self.error = Some(err.to_string());
            }
        }
    }

    /// Entry attualmente selezionata.
    pub fn selected(&self) -> Option<&Entry> {
        self.entries.get(self.cursor)
    }

    /// Path completo dell'entry selezionata.
    pub fn selected_path(&self) -> Option<PathBuf> {
        self.selected().map(|e| self.path.join(&e.name))
    }

    pub fn move_cursor(&mut self, delta: i32) {
        // Sposta il cursore di `delta`, tenendolo nei limiti [0, len-1].
        // Nessuna gestione di "scroll": la lista è resa per intero e lo scorrimento
        // è nativo del browser (l'host porta in vista la selezione via scrollIntoView).
        let len = self.entries.len() as i32;
        if len == 0 { return; }
        let new = (self.cursor as i32 + delta).clamp(0, len - 1);
        self.cursor = new as usize;
    }

    /// Sposta il cursore sulla PRIMA entry (indice 0 — tipicamente "..").
    pub fn cursor_to_start(&mut self) {
        self.cursor = 0;
    }

    /// Sposta il cursore sull'ULTIMA entry della directory corrente.
    /// `saturating_sub(1)` evita l'underflow su `usize` quando la lista è vuota
    /// (0 voci → cursore a 0, nessun panic).
    pub fn cursor_to_end(&mut self) {
        self.cursor = self.entries.len().saturating_sub(1);
    }

    pub fn enter_selected(&mut self) -> bool {
        if let Some(entry) = self.selected() {
            if !entry.is_dir { return false; }
            let new_path = if entry.name == ".." {
                fs::parent_of(&self.path)
            } else {
                self.path.join(&entry.name)
            };
            match fs::list_dir(&new_path) {
                Ok(entries) => {
                    self.path    = new_path;
                    self.entries = entries;
                    self.cursor  = 0;
                    self.error   = None;
                    true
                }
                Err(err) => { self.error = Some(err.to_string()); false }
            }
        } else {
            false
        }
    }
}

// ─── Dialog ───────────────────────────────────────────────────────────────────

/// Dialog modale aperto nella finestra (se presente).
#[derive(Debug, Clone)]
pub enum Dialog {
    /// Conferma di un'operazione di copia.
    /// `overwrite` = `true` se `dst` esiste già (verrà sovrascritto silenziosamente
    /// da `std::fs::copy`): la UI mostra un avviso distinto.
    ConfirmCopy { src: PathBuf, dst: PathBuf, overwrite: bool },
    /// Conferma di un'operazione di spostamento. `overwrite` come sopra.
    ConfirmMove { src: PathBuf, dst: PathBuf, overwrite: bool },
    /// Conferma di un'eliminazione. A differenza di copy/move non c'è una
    /// destinazione: si elimina un solo `path`. `is_dir` distingue file da
    /// cartella nel testo del dialog (e nulla più: la logica di rifiuto delle
    /// cartelle non vuote vive in `ops::delete_item`, non qui).
    ConfirmDelete { path: PathBuf, is_dir: bool },
    /// Input per il nome della nuova directory.
    MkdirInput { name: String },
    /// Confronto cartelle con presenza/assenza e date.
    FolderCompare {
        left_path:  PathBuf,
        right_path: PathBuf,
        result: ops::FolderCompareResult,
    },
    /// Diff di testo tra due file selezionati.
    FileDiff { diff: ops::FileDiffResult },
    /// Messaggio di errore generico.
    Error { msg: String },
}

// ─── LcState ──────────────────────────────────────────────────────────────────

/// Stato globale del plugin.
#[derive(Debug, Clone)]
pub struct LcState {
    pub left:       PanelState,
    pub right:      PanelState,
    pub active:     Side,
    pub dialog:     Option<Dialog>,
    /// Usato dal main per identificare la finestra corrente verso l'host.
    #[allow(dead_code)]
    pub window_id:  u64,
    /// Directory di storage privata del plugin (fornita dall'host nell'`Init`);
    /// usata per leggere/scrivere `config.json` con i path dei due pannelli.
    pub storage_dir: PathBuf,
}

impl LcState {
    /// Costruisce lo stato iniziale.
    /// `storage_dir` è lasciato vuoto (PathBuf::new()) per i test;
    /// `main.rs` lo sovrascrive con il path reale ricevuto nell'`Init`.
    pub fn new(left_path: PathBuf, right_path: PathBuf, window_id: u64) -> Self {
        LcState {
            left:        PanelState::new(left_path),
            right:       PanelState::new(right_path),
            active:      Side::Left,
            dialog:      None,
            window_id,
            storage_dir: PathBuf::new(),
        }
    }

    pub fn active_panel(&self) -> &PanelState {
        match self.active { Side::Left => &self.left, Side::Right => &self.right }
    }

    pub fn active_panel_mut(&mut self) -> &mut PanelState {
        match self.active { Side::Left => &mut self.left, Side::Right => &mut self.right }
    }

    pub fn inactive_panel(&self) -> &PanelState {
        match self.active { Side::Left => &self.right, Side::Right => &self.left }
    }
}

// ─── Handler principale ───────────────────────────────────────────────────────

/// Gestisce un messaggio `HostToPlugin` e restituisce le risposte.
pub fn handle(state: &mut LcState, msg: HostToPlugin) -> Vec<PluginToHost> {
    match msg {
        HostToPlugin::UiEvent { window_id, element_id, value } => {
            handle_ui_event(state, window_id, &element_id, value.as_deref())
        }

        HostToPlugin::Deinit {} => {
            // Salva i path correnti nel file di configurazione. Rete di sicurezza
            // per il caso di shutdown pulito dell'orchestratore; la persistenza
            // vera avviene già ad ogni navigazione (vedi `save_config`).
            LcConfig {
                left_path:  state.left.path.clone(),
                right_path: state.right.path.clone(),
            }.save(&state.storage_dir);
            vec![]
        }

        // Activate e Init sono gestiti in main.rs.
        HostToPlugin::Activate { .. } | HostToPlugin::Init { .. } => vec![],
    }
}

// ─── handle_ui_event ──────────────────────────────────────────────────────────

fn handle_ui_event(
    state:      &mut LcState,
    window_id:  u64,
    element_id: &str,
    value:      Option<&str>,
) -> Vec<PluginToHost> {
    if state.dialog.is_some() {
        return handle_dialog_event(state, window_id, element_id, value);
    }

    match element_id {
        "key:ArrowDown"  => { state.active_panel_mut().move_cursor(1);  }
        "key:ArrowUp"    => { state.active_panel_mut().move_cursor(-1); }
        "key:PageDown"   => { state.active_panel_mut().move_cursor(10); }
        "key:PageUp"     => { state.active_panel_mut().move_cursor(-10); }
        // Home/End (round 5): salto assoluto a inizio/fine lista (non relativo come
        // le frecce/PageUp/Down). Delegano ai metodi dedicati di `PanelState`.
        "key:Home"       => { state.active_panel_mut().cursor_to_start(); }
        "key:End"        => { state.active_panel_mut().cursor_to_end();   }

        "key:Enter"      => {
            // Su navigazione riuscita, persisti subito i path (save-on-navigate).
            if state.active_panel_mut().enter_selected() {
                save_config(state);
            }
        }

        "key:Backspace"  => {
            let panel  = state.active_panel_mut();
            let parent = fs::parent_of(&panel.path.clone());
            let changed = match fs::list_dir(&parent) {
                Ok(entries) => {
                    panel.path    = parent;
                    panel.entries = entries;
                    panel.cursor  = 0;
                    panel.error   = None;
                    true
                }
                Err(e) => { panel.error = Some(e.to_string()); false }
            };
            if changed {
                save_config(state);
            }
        }

        "key:Tab" => { state.active = state.active.other(); }

        "key:F5" => {
            // Guardia (data-loss): la voce ".." rappresenta la directory padre,
            // non un file copiabile. `src` diventerebbe `<panel>/..` (senza
            // file_name) e `dst` collasserebbe sull'altra dir → copia ricorsiva
            // indesiderata dell'intero genitore. Rifiuta prima di aprire il dialog.
            if selected_is_dotdot(state) {
                state.dialog = Some(Dialog::Error {
                    msg: "La voce '..' non può essere copiata.".to_string(),
                });
            } else if let Some(src) = state.active_panel().selected_path() {
                let dst = state.inactive_panel().path.join(src.file_name().unwrap_or_default());
                let overwrite = dst.exists();
                state.dialog = Some(Dialog::ConfirmCopy { src, dst, overwrite });
            }
        }

        "key:F6" => {
            // Stessa guardia di F5, ma qui il rischio è massimo: `move_item` sul
            // genitore ne rimuoverebbe l'intero albero (remove_dir_all).
            if selected_is_dotdot(state) {
                state.dialog = Some(Dialog::Error {
                    msg: "La voce '..' non può essere spostata.".to_string(),
                });
            } else if let Some(src) = state.active_panel().selected_path() {
                let dst = state.inactive_panel().path.join(src.file_name().unwrap_or_default());
                let overwrite = dst.exists();
                state.dialog = Some(Dialog::ConfirmMove { src, dst, overwrite });
            }
        }

        "key:F7" => {
            state.dialog = Some(Dialog::MkdirInput { name: String::new() });
        }

        // F8 — diff file se entrambi i cursori puntano a file,
        //       altrimenti confronto cartelle.
        "key:F8" => {
            let left_is_file  = state.left.selected().map(|e| !e.is_dir).unwrap_or(false);
            let right_is_file = state.right.selected().map(|e| !e.is_dir).unwrap_or(false);

            if left_is_file && right_is_file {
                // Entrambi file: diff di testo.
                let lp = state.left.selected_path().unwrap();
                let rp = state.right.selected_path().unwrap();
                match ops::diff_files(&lp, &rp) {
                    Ok(diff) => state.dialog = Some(Dialog::FileDiff { diff }),
                    Err(e)   => state.dialog = Some(Dialog::Error { msg: e.to_string() }),
                }
            } else {
                // Almeno uno è directory: confronto cartelle.
                let lp = state.left.path.clone();
                let rp = state.right.path.clone();
                match ops::folder_compare(&lp, &rp) {
                    Ok(result) => state.dialog = Some(Dialog::FolderCompare {
                        left_path:  lp,
                        right_path: rp,
                        result,
                    }),
                    Err(e) => state.dialog = Some(Dialog::Error { msg: e.to_string() }),
                }
            }
        }

        // Delete — Elimina la voce selezionata (file, o directory VUOTA).
        // Stessa guardia data-loss di F5/F6: la voce ".." è la directory padre,
        // non un elemento eliminabile (rifiuta con un errore, niente conferma).
        // La distruttività reale (rifiuto delle cartelle non vuote) è in
        // `ops::delete_item`, invocata solo alla conferma; qui apriamo il dialog.
        "key:Delete" => {
            if selected_is_dotdot(state) {
                state.dialog = Some(Dialog::Error {
                    msg: "La voce '..' non può essere eliminata.".to_string(),
                });
            } else if let Some(path) = state.active_panel().selected_path() {
                // `is_dir` serve solo al testo del dialog ("il file"/"la cartella").
                let is_dir = state.active_panel().selected().map(|e| e.is_dir).unwrap_or(false);
                state.dialog = Some(Dialog::ConfirmDelete { path, is_dir });
            }
        }

        // A questo punto `state.dialog` è sempre `None` (se fosse `Some`, la
        // funzione avrebbe già delegato a `handle_dialog_event` in cima). Il
        // pulsante della barra è etichettato "ESC Chiudi": qui Escape chiede
        // all'host di CHIUDERE la finestra. Prima era `state.dialog = None`, un
        // no-op osservabile (dialog già None) → nessuna chiusura mai implementata.
        "key:Escape" => {
            return vec![PluginToHost::CloseWindow { window_id }];
        }

        id if id.starts_with("panel:left:row:") => {
            if let Ok(n) = id["panel:left:row:".len()..].parse::<usize>() {
                state.active = Side::Left;
                state.left.cursor = n.min(state.left.entries.len().saturating_sub(1));
            }
        }
        id if id.starts_with("panel:right:row:") => {
            if let Ok(n) = id["panel:right:row:".len()..].parse::<usize>() {
                state.active = Side::Right;
                state.right.cursor = n.min(state.right.entries.len().saturating_sub(1));
            }
        }
        "panel:left:focus"  => { state.active = Side::Left;  }
        "panel:right:focus" => { state.active = Side::Right; }

        _ => { return vec![]; }
    }

    update_window(state, window_id)
}

// ─── handle_dialog_event ──────────────────────────────────────────────────────

fn handle_dialog_event(
    state:      &mut LcState,
    window_id:  u64,
    element_id: &str,
    value:      Option<&str>,
) -> Vec<PluginToHost> {
    let dialog = match state.dialog.take() {
        Some(d) => d,
        None    => return vec![],
    };

    match (&dialog, element_id) {
        // ─ ConfirmCopy ─
        (Dialog::ConfirmCopy { src, dst, .. }, "dialog:confirm:yes") => {
            let (src, dst) = (src.clone(), dst.clone());
            match ops::copy_item(&src, &dst) {
                Ok(()) => { state.left.refresh(); state.right.refresh(); }
                Err(e) => {
                    state.dialog = Some(Dialog::Error { msg: e.to_string() });
                    return update_window(state, window_id);
                }
            }
        }
        (Dialog::ConfirmCopy { .. }, _) => {}  // no / escape → chiudi

        // ─ ConfirmMove ─
        (Dialog::ConfirmMove { src, dst, .. }, "dialog:confirm:yes") => {
            let (src, dst) = (src.clone(), dst.clone());
            match ops::move_item(&src, &dst) {
                Ok(()) => { state.left.refresh(); state.right.refresh(); }
                Err(e) => {
                    state.dialog = Some(Dialog::Error { msg: e.to_string() });
                    return update_window(state, window_id);
                }
            }
        }
        (Dialog::ConfirmMove { .. }, _) => {}

        // ─ ConfirmDelete ─
        (Dialog::ConfirmDelete { path, .. }, "dialog:confirm:yes") => {
            let path = path.clone();
            match ops::delete_item(&path) {
                // Aggiorna ENTRAMBI i pannelli: se puntano alla stessa dir (o a dir
                // che si sovrappongono) l'eliminazione deve sparire da entrambe le
                // liste — stesso trattamento di ConfirmCopy/ConfirmMove.
                Ok(()) => { state.left.refresh(); state.right.refresh(); }
                Err(e) => {
                    state.dialog = Some(Dialog::Error { msg: e.to_string() });
                    return update_window(state, window_id);
                }
            }
        }
        (Dialog::ConfirmDelete { .. }, _) => {}  // no / escape → chiudi

        // ─ MkdirInput: aggiornamento testo ─
        (Dialog::MkdirInput { .. }, "dialog:mkdir:input") => {
            let name = value.unwrap_or("").to_string();
            state.dialog = Some(Dialog::MkdirInput { name });
            return update_window(state, window_id);
        }
        // ─ MkdirInput: conferma ─
        (Dialog::MkdirInput { name }, "dialog:confirm:yes") => {
            let name = name.clone();
            // Guardia (path traversal): il nome digitato deve essere un singolo
            // componente. Senza questo check, `path.join("..\\..\\evil")` o un
            // path assoluto ("C:\\...") sfuggirebbe dalla directory del pannello.
            if !is_safe_component_name(&name) {
                state.dialog = Some(Dialog::Error {
                    msg: "Nome cartella non valido.".to_string(),
                });
                return update_window(state, window_id);
            }
            let new_dir = state.active_panel().path.join(&name);
            match ops::mkdir(&new_dir) {
                Ok(()) => { state.active_panel_mut().refresh(); }
                Err(e) => {
                    state.dialog = Some(Dialog::Error { msg: e.to_string() });
                    return update_window(state, window_id);
                }
            }
        }
        (Dialog::MkdirInput { .. }, _) => {}

        // ─ Dialog di sola lettura: qualsiasi azione li chiude ─
        (Dialog::FolderCompare { .. }, _)
        | (Dialog::FileDiff    { .. }, _)
        | (Dialog::Error       { .. }, _) => {}
    }

    update_window(state, window_id)
}

fn update_window(state: &LcState, window_id: u64) -> Vec<PluginToHost> {
    vec![PluginToHost::UpdateWindow { window_id, html: render_window(state) }]
}

/// `true` se la voce selezionata nel pannello attivo è la voce speciale "..".
/// Estratto in helper perché usato da F5 e F6 (guardia anti data-loss).
fn selected_is_dotdot(state: &LcState) -> bool {
    state.active_panel()
        .selected()
        .map(|e| e.name == "..")
        .unwrap_or(false)
}

/// Salva i path correnti dei due pannelli in `storage_dir/config.json`.
/// Chiamato ad ogni navigazione riuscita (Enter/Backspace) → la persistenza
/// attraversa i riavvii senza dipendere da alcun segnale di chiusura verso il
/// processo plugin (che non esiste). Fail-silent se lo storage è ostile.
fn save_config(state: &LcState) {
    LcConfig {
        left_path:  state.left.path.clone(),
        right_path: state.right.path.clone(),
    }.save(&state.storage_dir);
}

/// `true` se `name` è un nome di componente di path sicuro per `path.join(name)`,
/// cioè può risolvere SOLO in un unico componente sotto la directory corrente.
/// Blocca il path traversal nel dialog F7 (mkdir):
/// - vuoto, "." e ".." → non sono nomi di nuove cartelle;
/// - '/' o '\\' (separatori) → impediscono sotto-path e risalite (`..\..\`);
/// - ':' → impedisce path assoluti / drive-letter (`C:\...`) e stream ADS Windows.
///
/// Un nome privo di questi tre caratteri, per costruzione, non può che risolvere
/// in un singolo componente — quindi accettiamo tranquillamente spazi e unicode.
fn is_safe_component_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains(':')
}

// ─── Test ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_state_tag(tag: &str) -> LcState {
        let base      = std::env::temp_dir().join(format!("lc_state_test_{tag}"));
        let left_dir  = base.join("left");
        let right_dir = base.join("right");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&left_dir).unwrap();
        fs::create_dir_all(&right_dir).unwrap();
        LcState::new(left_dir, right_dir, 42)
    }

    fn cleanup(state: &LcState) {
        if let Some(parent) = state.left.path.parent() {
            let _ = fs::remove_dir_all(parent);
        }
    }

    #[test]
    fn initial_active_panel_is_left() {
        let state = make_state_tag("initial");
        assert_eq!(state.active, Side::Left);
        cleanup(&state);
    }

    #[test]
    fn tab_switches_active_panel() {
        let mut state = make_state_tag("tab");
        assert_eq!(state.active, Side::Left);
        let msgs = handle_ui_event(&mut state, 42, "key:Tab", None);
        assert_eq!(state.active, Side::Right);
        assert!(msgs.iter().any(|m| matches!(m, PluginToHost::UpdateWindow { .. })));
        cleanup(&state);
    }

    #[test]
    fn arrow_down_moves_cursor() {
        let base = std::env::temp_dir().join("lc_arrow_test");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        fs::write(base.join("aaa.txt"), b"").unwrap();
        fs::write(base.join("bbb.txt"), b"").unwrap();
        let mut state = LcState::new(base.clone(), base.clone(), 1);
        assert_eq!(state.left.cursor, 0);
        handle_ui_event(&mut state, 1, "key:ArrowDown", None);
        assert_eq!(state.left.cursor, 1, "il cursore deve scendere di 1");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn arrow_up_does_not_go_negative() {
        let base = std::env::temp_dir().join("lc_up_test");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        fs::write(base.join("x.txt"), b"").unwrap();
        let mut state = LcState::new(base.clone(), base.clone(), 2);
        assert_eq!(state.left.cursor, 0);
        handle_ui_event(&mut state, 2, "key:ArrowUp", None);
        assert_eq!(state.left.cursor, 0, "il cursore non deve scendere sotto 0");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn enter_on_dir_navigates_into_it() {
        let base    = std::env::temp_dir().join("lc_enter_test");
        let sub_dir = base.join("subdir");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&sub_dir).unwrap();
        let mut state = LcState::new(base.clone(), base.clone(), 3);
        // ".." è la prima entry, "subdir" è la seconda (cursor=1 dopo la dir)
        // Le entry sono ordinate: ".." poi "subdir/"
        state.left.cursor = 1; // punta a "subdir"
        let navigated = state.left.enter_selected();
        assert!(navigated, "deve navigare in subdir");
        assert_eq!(state.left.path, sub_dir);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn f7_opens_mkdir_dialog() {
        let mut state = make_state_tag("f7");
        assert!(state.dialog.is_none());
        handle_ui_event(&mut state, 42, "key:F7", None);
        assert!(matches!(state.dialog, Some(Dialog::MkdirInput { .. })),
            "F7 deve aprire il dialog MkdirInput");
        cleanup(&state);
    }

    #[test]
    fn escape_closes_dialog() {
        let mut state = make_state_tag("escape");
        state.dialog = Some(Dialog::Error { msg: "test".to_string() });
        handle_ui_event(&mut state, 42, "key:Escape", None);
        assert!(state.dialog.is_none(), "Escape deve chiudere il dialog");
        cleanup(&state);
    }

    /// Con un dialog aperto, Escape deve SOLO chiudere il dialog e restituire un
    /// `UpdateWindow` — NON deve chiudere l'intera finestra. Questo test blinda la
    /// distinzione fra i due comportamenti di Escape (dialog-dismiss vs window-close)
    /// dopo la fix di `escape_at_top_level_closes_window`.
    #[test]
    fn escape_with_dialog_open_does_not_close_window() {
        let mut state = make_state_tag("escape_dialog_no_close");
        state.dialog = Some(Dialog::Error { msg: "test".to_string() });
        let msgs = handle_ui_event(&mut state, 42, "key:Escape", None);
        assert!(state.dialog.is_none(), "Escape deve chiudere il dialog");
        // Nessun CloseWindow: la finestra resta aperta, si emette solo UpdateWindow.
        assert!(
            !msgs.iter().any(|m| matches!(m, PluginToHost::CloseWindow { .. })),
            "Escape con dialog aperto NON deve chiudere la finestra"
        );
        assert!(
            msgs.iter().any(|m| matches!(m, PluginToHost::UpdateWindow { .. })),
            "Escape con dialog aperto deve ri-renderizzare la finestra (UpdateWindow)"
        );
        cleanup(&state);
    }

    /// A livello top (nessun dialog aperto) Escape deve CHIUDERE la finestra:
    /// il pulsante della barra è etichettato "ESC Chiudi". Prima della fix era un
    /// no-op (`state.dialog = None` con dialog già `None`) → UpdateWindow inutile.
    #[test]
    fn escape_at_top_level_closes_window() {
        let mut state = make_state_tag("escape_top");
        assert!(state.dialog.is_none(), "precondizione: nessun dialog aperto");
        let msgs = handle_ui_event(&mut state, 42, "key:Escape", None);
        assert_eq!(
            msgs,
            vec![PluginToHost::CloseWindow { window_id: 42 }],
            "ESC a livello top deve restituire esattamente CloseWindow, non UpdateWindow"
        );
        cleanup(&state);
    }

    #[test]
    fn f8_on_dirs_opens_folder_compare() {
        // Entrambi i pannelli hanno solo ".." come entry → cursor su dir → FolderCompare
        let mut state = make_state_tag("f8_dirs");
        // cursor=0 → ".." → is_dir=true → FolderCompare
        handle_ui_event(&mut state, 42, "key:F8", None);
        assert!(matches!(state.dialog, Some(Dialog::FolderCompare { .. })),
            "F8 su directory deve aprire FolderCompare");
        cleanup(&state);
    }

    #[test]
    fn f8_on_two_files_opens_file_diff() {
        let base = std::env::temp_dir().join("lc_f8_files");
        let _ = fs::remove_dir_all(&base);
        let left_dir  = base.join("left");
        let right_dir = base.join("right");
        fs::create_dir_all(&left_dir).unwrap();
        fs::create_dir_all(&right_dir).unwrap();
        fs::write(left_dir.join("a.txt"),  b"line1\nline2\n").unwrap();
        fs::write(right_dir.join("b.txt"), b"line1\nline3\n").unwrap();

        let mut state = LcState::new(left_dir, right_dir, 99);
        // Cursore su index 1 (il file, non "..") in entrambi i pannelli
        state.left.cursor  = 1;
        state.right.cursor = 1;

        handle_ui_event(&mut state, 99, "key:F8", None);
        assert!(matches!(state.dialog, Some(Dialog::FileDiff { .. })),
            "F8 su due file deve aprire FileDiff");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn panel_row_click_sets_cursor_and_active_side() {
        let base = std::env::temp_dir().join("lc_click_test");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        fs::write(base.join("a.txt"), b"").unwrap();
        fs::write(base.join("b.txt"), b"").unwrap();
        let mut state = LcState::new(base.clone(), base.clone(), 5);
        handle_ui_event(&mut state, 5, "panel:right:row:2", None);
        assert_eq!(state.active, Side::Right);
        assert_eq!(state.right.cursor, 2);
        let _ = fs::remove_dir_all(&base);
    }

    // ── Fix 1 (data-loss critica): F5/F6 sulla voce ".." ──────────────────────

    /// Regressione: premere F6 (Sposta) sulla voce ".." NON deve toccare la
    /// directory genitore. Prima della fix, `src` diventava `<panel>/sub/..`
    /// (senza `file_name`) e `dst` collassava sulla dir dell'altro pannello →
    /// `move_item` finiva per `remove_dir_all` sull'intera directory padre.
    #[test]
    fn f6_on_dotdot_does_not_destroy_parent_directory() {
        let base       = std::env::temp_dir().join("lc_dotdot_f6_test");
        let _ = fs::remove_dir_all(&base);
        let left_base  = base.join("left");
        let right_base = base.join("right");
        let sub        = left_base.join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::create_dir_all(&right_base).unwrap();
        // Sentinella nella directory padre: deve sopravvivere all'operazione.
        fs::write(left_base.join("sentinel.txt"), b"prezioso").unwrap();

        let mut state = LcState::new(left_base.clone(), right_base, 7);
        // Entries di left_base: ["..", "sub/", "sentinel.txt"] → "sub" è a index 1.
        handle_ui_event(&mut state, 7, "key:ArrowDown", None); // cursor 0 → 1 ("sub")
        handle_ui_event(&mut state, 7, "key:Enter", None);     // entra in "sub"
        assert_eq!(state.left.path, sub, "il pannello deve essere dentro 'sub'");
        // Ora cursor=0 punta a ".." dentro "sub".
        assert_eq!(state.left.selected().map(|e| e.name.as_str()), Some(".."),
            "la voce selezionata deve essere '..'");

        // Premi F6 e conferma.
        handle_ui_event(&mut state, 7, "key:F6", None);
        handle_ui_event(&mut state, 7, "dialog:confirm:yes", None);

        // La directory padre e la sua sentinella devono essere intatte.
        assert!(left_base.exists(), "la directory padre NON deve essere cancellata");
        assert!(left_base.join("sentinel.txt").exists(),
            "il file sentinella nella directory padre deve sopravvivere");

        let _ = fs::remove_dir_all(&base);
    }

    // ── Part C (round 4): comando Elimina (tasto Delete) ──────────────────────

    /// Premere Delete su un file normale apre `Dialog::ConfirmDelete` con il path
    /// corretto e `is_dir = false` (mirror di F5/F6 → ConfirmCopy/ConfirmMove).
    #[test]
    fn delete_key_on_file_opens_confirm_delete() {
        let base = std::env::temp_dir().join("lc_del_key_file");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        fs::write(base.join("doc.txt"), b"x").unwrap();
        let mut state = LcState::new(base.clone(), base.clone(), 30);
        // Entries: ["..", "doc.txt"] → cursor=1 punta al file.
        state.left.cursor = 1;
        handle_ui_event(&mut state, 30, "key:Delete", None);
        match &state.dialog {
            Some(Dialog::ConfirmDelete { path, is_dir }) => {
                assert_eq!(path, &base.join("doc.txt"), "il path da eliminare deve essere il file selezionato");
                assert!(!is_dir, "un file deve avere is_dir = false");
            }
            other => panic!("Delete su un file deve aprire ConfirmDelete, trovato: {other:?}"),
        }
        let _ = fs::remove_dir_all(&base);
    }

    /// Premere Delete sulla voce ".." NON deve costruire un ConfirmDelete: deve
    /// mostrare un `Dialog::Error` (stessa guardia data-loss di F5/F6 su "..").
    #[test]
    fn delete_key_on_dotdot_shows_error_not_confirm() {
        let base       = std::env::temp_dir().join("lc_del_key_dotdot");
        let _ = fs::remove_dir_all(&base);
        let left_base  = base.join("left");
        let right_base = base.join("right");
        let sub        = left_base.join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::create_dir_all(&right_base).unwrap();

        let mut state = LcState::new(left_base, right_base, 31);
        handle_ui_event(&mut state, 31, "key:ArrowDown", None); // cursor → "sub"
        handle_ui_event(&mut state, 31, "key:Enter", None);     // dentro "sub", cursor su ".."
        assert_eq!(state.left.selected().map(|e| e.name.as_str()), Some(".."),
            "precondizione: selezionato '..'");
        handle_ui_event(&mut state, 31, "key:Delete", None);
        assert!(matches!(state.dialog, Some(Dialog::Error { .. })),
            "Delete su '..' deve aprire un Dialog::Error, non un ConfirmDelete");
        let _ = fs::remove_dir_all(&base);
    }

    /// Confermare l'eliminazione di un file lo rimuove DAVVERO dal disco e
    /// aggiorna entrambi i pannelli (mirror di ConfirmCopy/ConfirmMove).
    #[test]
    fn confirm_delete_removes_file_and_refreshes_both_panels() {
        let base = std::env::temp_dir().join("lc_del_confirm_file");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        fs::write(base.join("gone.txt"), b"x").unwrap();
        // Entrambi i pannelli sulla stessa dir: la refresh di entrambi è
        // osservabile (l'eliminazione sparisce da tutte e due le liste).
        let mut state = LcState::new(base.clone(), base.clone(), 32);
        state.left.cursor = 1; // "gone.txt"
        handle_ui_event(&mut state, 32, "key:Delete", None);
        assert!(matches!(state.dialog, Some(Dialog::ConfirmDelete { .. })),
            "precondizione: dialog di conferma aperto");
        handle_ui_event(&mut state, 32, "dialog:confirm:yes", None);

        assert!(!base.join("gone.txt").exists(), "il file deve essere stato eliminato dal disco");
        assert!(state.dialog.is_none(), "dopo la conferma il dialog deve chiudersi");
        // Entrambi i pannelli aggiornati: "gone.txt" non compare più in nessuno.
        assert!(!state.left.entries.iter().any(|e| e.name == "gone.txt"),
            "il pannello sinistro deve essere stato aggiornato");
        assert!(!state.right.entries.iter().any(|e| e.name == "gone.txt"),
            "il pannello destro deve essere stato aggiornato");
        let _ = fs::remove_dir_all(&base);
    }

    /// Regressione end-to-end (headline Part C): confermare l'eliminazione di una
    /// directory NON vuota deve mostrare `Dialog::Error` (messaggio che cita la
    /// non-vuotezza) e lasciare la directory e il suo contenuto INTATTI su disco.
    /// Guidato attraverso il flusso reale `handle_ui_event`/`handle_dialog_event`,
    /// non testando `ops::delete_item` in isolamento (verifica anche il wiring).
    #[test]
    fn confirm_delete_non_empty_dir_shows_error_and_keeps_dir() {
        let base = std::env::temp_dir().join("lc_del_confirm_nonempty");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let d = base.join("piena");
        fs::create_dir(&d).unwrap();
        let inner = d.join("dentro.txt");
        fs::write(&inner, b"prezioso").unwrap();

        // Entries di base: ["..", "piena/"] → cursor=1 punta alla cartella.
        let mut state = LcState::new(base.clone(), base.clone(), 33);
        state.left.cursor = 1;
        handle_ui_event(&mut state, 33, "key:Delete", None);
        match &state.dialog {
            Some(Dialog::ConfirmDelete { is_dir, .. }) => assert!(*is_dir, "deve essere marcata come directory"),
            other => panic!("Delete su una cartella deve aprire ConfirmDelete, trovato: {other:?}"),
        }
        handle_ui_event(&mut state, 33, "dialog:confirm:yes", None);

        // La cartella e il contenuto devono sopravvivere; deve comparire un errore.
        assert!(d.exists(),     "la directory non vuota NON deve essere eliminata");
        assert!(inner.exists(), "il contenuto della directory deve sopravvivere");
        match &state.dialog {
            Some(Dialog::Error { msg }) => assert!(msg.to_lowercase().contains("vuota"),
                "il messaggio d'errore deve spiegare che la cartella non è vuota (trovato: {msg})"),
            other => panic!("l'eliminazione di una cartella non vuota deve mostrare Dialog::Error, trovato: {other:?}"),
        }
        let _ = fs::remove_dir_all(&base);
    }

    // ── Fix 2 (persistenza): salvataggio ad ogni navigazione ──────────────────

    /// Regressione (feature morta): navigare in una sottodirectory (Enter) deve
    /// salvare SUBITO i path in `storage_dir/config.json` — non solo al `Deinit`
    /// (che l'host invia solo allo shutdown, mai alla chiusura della finestra).
    #[test]
    fn enter_saves_current_paths_to_storage_dir() {
        let base    = std::env::temp_dir().join("lc_save_on_nav_test");
        let _ = fs::remove_dir_all(&base);
        let sub     = base.join("sub");
        let storage = base.join("storage");
        fs::create_dir_all(&sub).unwrap();
        fs::create_dir_all(&storage).unwrap();

        let mut state = LcState::new(base.clone(), base.clone(), 11);
        state.storage_dir = storage.clone();

        // Entries di base: ["..", "storage/", "sub/"] → ArrowDown due volte per "sub".
        handle_ui_event(&mut state, 11, "key:ArrowDown", None);
        handle_ui_event(&mut state, 11, "key:ArrowDown", None);
        handle_ui_event(&mut state, 11, "key:Enter", None);
        assert_eq!(state.left.path, sub, "precondizione: siamo entrati in 'sub'");

        let content = fs::read_to_string(storage.join("config.json"))
            .expect("config.json deve essere stato scritto alla navigazione");
        let v: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(v["left_path"].as_str(), Some(sub.to_string_lossy().as_ref()),
            "il config salvato deve riflettere la directory in cui abbiamo navigato");

        let _ = fs::remove_dir_all(&base);
    }

    /// Anche la risalita (Backspace) deve salvare i path correnti.
    #[test]
    fn backspace_saves_current_paths_to_storage_dir() {
        let base    = std::env::temp_dir().join("lc_save_on_back_test");
        let _ = fs::remove_dir_all(&base);
        let sub     = base.join("sub");
        let storage = base.join("storage");
        fs::create_dir_all(&sub).unwrap();
        fs::create_dir_all(&storage).unwrap();

        // Parti DENTRO sub, poi risali con Backspace.
        let mut state = LcState::new(sub.clone(), base.clone(), 12);
        state.storage_dir = storage.clone();

        handle_ui_event(&mut state, 12, "key:Backspace", None);
        assert_eq!(state.left.path, base, "precondizione: risaliti a base");

        let content = fs::read_to_string(storage.join("config.json"))
            .expect("config.json deve essere stato scritto alla risalita");
        let v: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(v["left_path"].as_str(), Some(base.to_string_lossy().as_ref()));

        let _ = fs::remove_dir_all(&base);
    }

    /// Anche F5 (Copia) sulla voce ".." non deve avviare una copia ricorsiva del
    /// genitore: deve mostrare un dialog di errore e non un ConfirmCopy.
    #[test]
    fn f5_on_dotdot_shows_error_not_confirm() {
        let base       = std::env::temp_dir().join("lc_dotdot_f5_test");
        let _ = fs::remove_dir_all(&base);
        let left_base  = base.join("left");
        let right_base = base.join("right");
        let sub        = left_base.join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::create_dir_all(&right_base).unwrap();

        let mut state = LcState::new(left_base, right_base, 8);
        handle_ui_event(&mut state, 8, "key:ArrowDown", None);
        handle_ui_event(&mut state, 8, "key:Enter", None); // dentro "sub", cursor su ".."
        handle_ui_event(&mut state, 8, "key:F5", None);
        assert!(matches!(state.dialog, Some(Dialog::Error { .. })),
            "F5 su '..' deve aprire un Dialog::Error, non un ConfirmCopy");

        let _ = fs::remove_dir_all(&base);
    }

    // ── Fix 3 (path traversal): validazione nome F7 mkdir ─────────────────────

    /// Un nome con separatori/".." nel dialog F7 non deve creare directory FUORI
    /// dal pannello corrente: deve essere rifiutato con un `Dialog::Error`.
    #[test]
    fn f7_mkdir_rejects_traversal_name() {
        let base = std::env::temp_dir().join("lc_mkdir_traversal_test");
        let _ = fs::remove_dir_all(&base);
        let panel = base.join("a").join("b").join("c");
        fs::create_dir_all(&panel).unwrap();

        let mut state = LcState::new(panel.clone(), base.clone(), 21);

        // F7 → input malevolo → conferma.
        handle_ui_event(&mut state, 21, "key:F7", None);
        handle_ui_event(&mut state, 21, "dialog:mkdir:input", Some("..\\..\\evil"));
        handle_ui_event(&mut state, 21, "dialog:confirm:yes", None);

        // La directory di traversal (c/../../evil = base/a/evil) NON deve esistere.
        let escaped = base.join("a").join("evil");
        assert!(!escaped.exists(),
            "il nome di traversal non deve creare directory fuori dal pannello");
        assert!(matches!(state.dialog, Some(Dialog::Error { .. })),
            "un nome non valido deve mostrare un Dialog::Error");

        let _ = fs::remove_dir_all(&base);
    }

    // ── Part C (integrazione): round-trip radice unità ↔ elenco unità ─────────

    /// Round-trip completo: "C:\" → ".." → elenco unità (path sentinella) →
    /// selezione "C:\" → di nuovo alla radice dell'unità. Verifica che
    /// `enter_selected()` (invariato) gestisca correttamente la sentinella in
    /// entrambe le direzioni, incluso `PathBuf::new().join("C:\\") == "C:\\"`.
    /// Gate `#[cfg(windows)]`: usa "C:\" che esiste sempre in questo ambiente
    /// Windows-primary (evita di dipendere da altre unità del PC di test).
    #[cfg(windows)]
    #[test]
    fn drive_list_round_trip_from_drive_root() {
        let mut panel = PanelState::new(PathBuf::from("C:\\"));
        // Alla radice dell'unità la prima voce è ".." (Part C) → cursore lì.
        assert_eq!(panel.selected().map(|e| e.name.as_str()), Some(".."),
            "precondizione: la voce selezionata alla radice è '..'");

        // "C:\" → ".." → elenco unità (sentinella).
        // NB: qui `crate::fs` esplicito perché il modulo di test importa
        // `use std::fs;` (per le operazioni su file), che oscura `crate::fs`.
        assert!(panel.enter_selected(), "'..' alla radice deve navigare all'elenco unità");
        assert_eq!(panel.path, crate::fs::drive_list_sentinel(),
            "dopo '..' il pannello mostra l'elenco unità (path sentinella)");
        assert!(panel.entries.iter().any(|e| e.name == "C:\\"),
            "l'elenco unità deve contenere l'unità corrente (C:\\)");

        // Elenco unità → seleziona "C:\" → torna alla radice dell'unità.
        let idx = panel.entries.iter().position(|e| e.name == "C:\\").unwrap();
        panel.cursor = idx;
        assert!(panel.enter_selected(), "selezionare un'unità deve entrarci");
        assert_eq!(panel.path, PathBuf::from("C:\\"),
            "selezionare un'unità dall'elenco riporta alla sua radice");
    }

    // ── Round 5 Part B: tasti Home/End (salto a inizio/fine lista) ────────────

    /// `cursor_to_start` porta il cursore a 0 partendo da metà lista.
    #[test]
    fn cursor_to_start_jumps_to_first_entry() {
        let base = std::env::temp_dir().join("lc_home_unit");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        fs::write(base.join("a.txt"), b"").unwrap();
        fs::write(base.join("b.txt"), b"").unwrap();
        fs::write(base.join("c.txt"), b"").unwrap();
        let mut panel = PanelState::new(base.clone());
        // Entries ordinate: ["..", "a.txt", "b.txt", "c.txt"] → parti a metà.
        panel.cursor = 2;
        panel.cursor_to_start();
        assert_eq!(panel.cursor, 0, "cursor_to_start deve portare il cursore alla prima voce (0)");
        let _ = fs::remove_dir_all(&base);
    }

    /// `cursor_to_end` porta il cursore sull'ultima entry (`entries.len() - 1`).
    #[test]
    fn cursor_to_end_jumps_to_last_entry() {
        let base = std::env::temp_dir().join("lc_end_unit");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        fs::write(base.join("a.txt"), b"").unwrap();
        fs::write(base.join("b.txt"), b"").unwrap();
        fs::write(base.join("c.txt"), b"").unwrap();
        let mut panel = PanelState::new(base.clone());
        // 4 voci → ultimo indice = 3.
        panel.cursor = 1;
        panel.cursor_to_end();
        assert_eq!(panel.cursor, panel.entries.len() - 1,
            "cursor_to_end deve portare il cursore sull'ultima voce");
        assert_eq!(panel.cursor, 3, "con 4 voci l'ultimo indice è 3");
        let _ = fs::remove_dir_all(&base);
    }

    /// Guardia: `cursor_to_end` su una lista VUOTA (0 entries) non deve panicare.
    /// `saturating_sub(1)` su `0usize` dà `0` (nessun underflow). Questo test PROVA
    /// la guardia invece di darla per scontata.
    #[test]
    fn cursor_to_end_on_empty_list_does_not_panic() {
        let mut panel = PanelState::new(PathBuf::from("/percorso/inesistente/lc_xyz_vuoto"));
        panel.entries.clear(); // difensivo: forza esplicitamente 0 voci
        panel.cursor = 0;
        panel.cursor_to_end();
        assert_eq!(panel.cursor, 0, "su lista vuota il cursore resta 0 senza panicare");
    }

    /// Integrazione: `key:Home` porta il cursore alla prima voce e restituisce un
    /// `UpdateWindow` (mirror dello stile dei test ArrowDown/PageUp).
    #[test]
    fn home_key_moves_cursor_to_first_entry() {
        let base = std::env::temp_dir().join("lc_home_evt");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        fs::write(base.join("a.txt"), b"").unwrap();
        fs::write(base.join("b.txt"), b"").unwrap();
        fs::write(base.join("c.txt"), b"").unwrap();
        let mut state = LcState::new(base.clone(), base.clone(), 40);
        state.left.cursor = 2; // parti a metà lista
        let msgs = handle_ui_event(&mut state, 40, "key:Home", None);
        assert_eq!(state.left.cursor, 0, "Home deve portare il cursore alla prima voce");
        assert!(msgs.iter().any(|m| matches!(m, PluginToHost::UpdateWindow { .. })),
            "Home deve restituire un UpdateWindow");
        let _ = fs::remove_dir_all(&base);
    }

    /// Integrazione: `key:End` porta il cursore all'ultima voce e restituisce un
    /// `UpdateWindow`.
    #[test]
    fn end_key_moves_cursor_to_last_entry() {
        let base = std::env::temp_dir().join("lc_end_evt");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        fs::write(base.join("a.txt"), b"").unwrap();
        fs::write(base.join("b.txt"), b"").unwrap();
        fs::write(base.join("c.txt"), b"").unwrap();
        let mut state = LcState::new(base.clone(), base.clone(), 41);
        assert_eq!(state.left.cursor, 0, "precondizione: cursore all'inizio");
        let last = state.left.entries.len() - 1;
        let msgs = handle_ui_event(&mut state, 41, "key:End", None);
        assert_eq!(state.left.cursor, last, "End deve portare il cursore all'ultima voce");
        assert!(msgs.iter().any(|m| matches!(m, PluginToHost::UpdateWindow { .. })),
            "End deve restituire un UpdateWindow");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn is_safe_component_name_accepts_ordinary_names() {
        assert!(is_safe_component_name("documenti"));
        assert!(is_safe_component_name("con spazio"));        // spazio ok
        assert!(is_safe_component_name(" leading-space"));    // spazio iniziale ok
        assert!(is_safe_component_name("café_pianì"));        // unicode ok
        assert!(is_safe_component_name("file.tar.gz"));
    }

    #[test]
    fn is_safe_component_name_rejects_traversal_and_separators() {
        assert!(!is_safe_component_name(""),  "vuoto");
        assert!(!is_safe_component_name("."),  "punto singolo");
        assert!(!is_safe_component_name(".."), "doppio punto");
        assert!(!is_safe_component_name("a/b"),  "slash");
        assert!(!is_safe_component_name("a\\b"), "backslash");
        assert!(!is_safe_component_name("C:"),   "drive-letter");
        // I pattern di escape confermati empiricamente dalla review:
        assert!(!is_safe_component_name("C:\\Windows\\System32\\pwned"));
        assert!(!is_safe_component_name("..\\..\\..\\Windows\\System32"));
        assert!(!is_safe_component_name("\\\\server\\share\\evil")); // UNC
    }
}
