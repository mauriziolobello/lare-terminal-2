//! Persistenza delle note in `notes.json` — stesso pattern di `aichat/config.rs`
//! (JSON pretty, `load_or_generate`/`save`), in `<config_dir>` (2.0, D6 —
//! `main.rs` passa `config_dir.join("notes.json")`, nessuna variabile
//! d'ambiente).
//! Store di proprietà dell'orchestrator (design §8): sopravvive a UI chiusa e
//! a un riavvio dell'orchestrator, necessario per rispondere a un digest anche
//! senza che l'utente abbia mai aperto la Library.

use crate::notes::Note;
use std::path::{Path, PathBuf};

/// `Clone` (Task 7, fix di review): `perform()` deve poter clonare uno snapshot
/// di `self.notes` per passarlo a un `tokio::spawn` che fa il vero I/O in
/// background (`Effect::SaveNotes`) — senza `Clone`, l'unica alternativa
/// sarebbe scrivere il file SINCRONAMENTE dentro `perform`, bloccando l'intero
/// loop dell'attore (keepalive/elezione/relay) per la durata dello scrivi-su-
/// disco. `PathBuf`/`Vec<Note>` sono entrambi già `Clone`, quindi il derive è
/// automatico e non aggiunge alcun comportamento nuovo a questo tipo.
#[derive(Clone)]
pub struct NotesStore {
    path: PathBuf,
    notes: Vec<Note>,
}

impl NotesStore {
    /// Carica da `path`; se assente o corrotto, parte da archivio vuoto
    /// (stesso comportamento tollerante di `aichat::config::load_or_generate`).
    pub fn load_or_generate(path: &Path) -> Self {
        let notes = std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Vec<Note>>(&bytes).ok())
            .unwrap_or_default();
        Self { path: path.to_path_buf(), notes }
    }

    /// Store puramente in-memoria, nessun file coinvolto — usato dai test
    /// (`AiChatService::new_for_test`, Task 7) per evitare qualunque I/O reale
    /// nei test unitari dell'attore.
    pub fn empty_in_memory() -> Self {
        Self { path: PathBuf::new(), notes: Vec::new() }
    }

    /// No-op silenzioso se `path` è vuoto (store in-memory, v. `empty_in_memory`).
    pub fn save(&self) -> std::io::Result<()> {
        if self.path.as_os_str().is_empty() {
            return Ok(());
        }
        let json = serde_json::to_vec_pretty(&self.notes).map_err(std::io::Error::other)?;
        std::fs::write(&self.path, json)
    }

    pub fn all(&self) -> &[Note] {
        &self.notes
    }

    pub fn get(&self, id: &str) -> Option<&Note> {
        self.notes.iter().find(|n| n.id == id)
    }

    /// Inserisce `incoming` se l'id è nuovo; altrimenti fonde con la copia
    /// esistente via `merge_note` e SOSTITUISCE l'entry. Ritorna il `Note`
    /// risultante (per costruire l'effetto `ToUi(NoteUpserted)`/broadcast).
    pub fn upsert_merged(&mut self, incoming: Note) -> Note {
        if let Some(pos) = self.notes.iter().position(|n| n.id == incoming.id) {
            let merged = crate::notes::merge_note(&self.notes[pos], &incoming);
            self.notes[pos] = merged.clone();
            merged
        } else {
            self.notes.push(incoming.clone());
            incoming
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn note(id: &str) -> Note {
        Note {
            id: id.into(),
            title: "t".into(),
            title_touched: ("skimble".into(), 0),
            segments: vec![],
            created_by: "skimble".into(),
            created_at_ms: 0,
            deleted: false,
        }
    }

    #[test]
    fn load_or_generate_empty_when_file_absent() {
        let dir = TempDir::new().unwrap();
        let store = NotesStore::load_or_generate(&dir.path().join("notes.json"));
        assert!(store.all().is_empty());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("notes.json");
        let mut store = NotesStore::load_or_generate(&path);
        store.upsert_merged(note("n1"));
        store.save().unwrap();

        let reloaded = NotesStore::load_or_generate(&path);
        assert_eq!(reloaded.all().len(), 1);
        assert_eq!(reloaded.get("n1").unwrap().id, "n1");
    }

    #[test]
    fn upsert_merged_inserts_new_id() {
        let dir = TempDir::new().unwrap();
        let mut store = NotesStore::load_or_generate(&dir.path().join("notes.json"));
        store.upsert_merged(note("n1"));
        assert_eq!(store.all().len(), 1);
    }

    #[test]
    fn upsert_merged_merges_existing_id_instead_of_duplicating() {
        let dir = TempDir::new().unwrap();
        let mut store = NotesStore::load_or_generate(&dir.path().join("notes.json"));
        store.upsert_merged(note("n1"));
        let mut deleted_version = note("n1");
        deleted_version.deleted = true;
        store.upsert_merged(deleted_version);

        assert_eq!(store.all().len(), 1, "stesso id → merge, non un secondo elemento");
        assert!(store.get("n1").unwrap().deleted);
    }

    #[test]
    fn empty_in_memory_save_is_a_silent_noop() {
        let mut store = NotesStore::empty_in_memory();
        store.upsert_merged(note("n1"));
        assert!(store.save().is_ok(), "nessun file, nessun errore: no-op silenzioso");
    }
}
