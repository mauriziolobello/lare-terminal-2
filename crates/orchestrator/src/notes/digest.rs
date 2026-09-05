//! Digest (impronte senza testo) e logica di riconciliazione per il "Blocco
//! note". Design: Docs/superpowers/specs/2026-07-29-library-notes-design.md §5.

use crate::notes::Note;
use serde::{Deserialize, Serialize};

/// Impronta di una nota: basta a decidere se due copie sono allineate, senza
/// mai portare testo (design §5.2, punto 2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteDigest {
    pub deleted: bool,
    pub title_touched: (String, u64),
    /// Ordinato per nome macchina (confronto deterministico).
    pub segment_keys: Vec<(String, u64)>,
}

pub fn digest_of(note: &Note) -> NoteDigest {
    let mut segment_keys: Vec<(String, u64)> =
        note.segments.iter().map(|s| (s.machine.clone(), s.seq)).collect();
    segment_keys.sort();
    NoteDigest { deleted: note.deleted, title_touched: note.title_touched.clone(), segment_keys }
}

/// Renderizza il corpo per la UI: un solo segmento CON TESTO → testo
/// semplice; 2+ → ognuno preceduto da `— <machine> —`, ordinati per nome
/// macchina (già garantito da `merge_note`, ma riordiniamo qui per non
/// dipendere da chi costruisce il `Note` passato).
///
/// I segmenti con `text` vuoto (una macchina che ha svuotato deliberatamente
/// il proprio segmento via `NoteEditRequested { text: "" }`) sono esclusi dal
/// conteggio e dal rendering: contano come "non ha contribuito", non come
/// "ha contribuito con niente" — altrimenti una nota a 2 macchine dove una
/// svuota il proprio testo mostrerebbe un'intestazione "— machine —" penzolante
/// senza nulla sotto. Il `Segment` resta comunque nei dati (v. `NoteEditRequested`
/// in `aichat/service.rs`): serve a preservare il `seq` per il merge CRDT.
pub fn render_body(note: &Note) -> String {
    let mut segments: Vec<_> = note.segments.iter().filter(|s| !s.text.is_empty()).collect();
    segments.sort_by(|a, b| a.machine.cmp(&b.machine));
    if segments.len() <= 1 {
        return segments.first().map(|s| s.text.clone()).unwrap_or_default();
    }
    segments
        .iter()
        .map(|s| format!("— {} —\n{}", s.machine, s.text))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Confronta il proprio archivio (`mine`) con i digest ricevuti dall'altro
/// lato del link (`their_digests`) e decide cosa serve scambiare:
/// - `want`: id di note che l'altro lato ha (secondo il suo digest) e che a noi
///   mancano o abbiamo più vecchie — dobbiamo chiederne il contenuto pieno.
/// - `push`: note NOSTRE che l'altro lato non conosce affatto, o conosce con un
///   digest più vecchio del nostro — gliele mandiamo per intero.
///
/// Nota nota da entrambi i lati con LO STESSO digest → non scambiata (già allineata).
pub fn reconcile(mine: &[Note], their_digests: &[(String, NoteDigest)]) -> (Vec<String>, Vec<Note>) {
    use std::collections::HashMap;

    let their_map: HashMap<&String, &NoteDigest> =
        their_digests.iter().map(|(id, d)| (id, d)).collect();
    let mine_map: HashMap<&String, &Note> = mine.iter().map(|n| (&n.id, n)).collect();

    let mut want = Vec::new();
    for (id, their_digest) in &their_map {
        match mine_map.get(id) {
            None => want.push((*id).clone()), // nota sconosciuta a noi
            Some(my_note) => {
                let my_digest = digest_of(my_note);
                if &my_digest != *their_digest {
                    want.push((*id).clone()); // digest diverso: potremmo essere indietro
                }
            }
        }
    }

    let mut push = Vec::new();
    for note in mine {
        match their_map.get(&note.id) {
            None => push.push(note.clone()), // loro non la conoscono affatto
            Some(their_digest) => {
                if &digest_of(note) != *their_digest {
                    push.push(note.clone()); // potremmo avere qualcosa che a loro manca
                }
            }
        }
    }

    (want, push)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::Segment;

    fn seg(machine: &str, seq: u64, text: &str) -> Segment {
        Segment { machine: machine.into(), seq, text: text.into(), edited_at_ms: 0 }
    }

    fn note(id: &str, segments: Vec<Segment>) -> Note {
        Note {
            id: id.into(),
            title: "t".into(),
            title_touched: ("skimble".into(), 0),
            segments,
            created_by: "skimble".into(),
            created_at_ms: 0,
            deleted: false,
        }
    }

    #[test]
    fn render_body_single_segment_has_no_header() {
        let n = note("n1", vec![seg("skimble", 1, "solo testo")]);
        assert_eq!(render_body(&n), "solo testo");
    }

    #[test]
    fn render_body_multiple_segments_has_headers_in_machine_order() {
        let n = note("n1", vec![seg("rumpleteazer", 1, "B"), seg("skimble", 1, "A")]);
        assert_eq!(render_body(&n), "— rumpleteazer —\nB\n\n— skimble —\nA");
    }

    #[test]
    fn render_body_empty_segment_excluded_no_dangling_header() {
        // `NoteEditRequested` con testo vuoto (svuotare deliberatamente il
        // proprio segmento) NON rimuove il Segment dai dati — resta con
        // `text: ""` per preservare il `seq` ai fini del merge CRDT. Ma un
        // segmento vuoto non deve MAI produrre un'intestazione "— machine —"
        // penzolante senza nulla sotto: conta come "non ha contribuito",
        // esattamente come se non avesse mai scritto nulla.
        let n = note("n1", vec![seg("rumpleteazer", 1, "B"), seg("skimble", 2, "")]);
        assert_eq!(render_body(&n), "B");
    }

    #[test]
    fn render_body_all_segments_empty_is_empty_string() {
        let n = note("n1", vec![seg("rumpleteazer", 1, ""), seg("skimble", 1, "")]);
        assert_eq!(render_body(&n), "");
    }

    #[test]
    fn reconcile_wants_note_unknown_locally() {
        let their = vec![("n1".to_string(), digest_of(&note("n1", vec![seg("skimble", 1, "x")])))];
        let (want, push) = reconcile(&[], &their);
        assert_eq!(want, vec!["n1".to_string()]);
        assert!(push.is_empty());
    }

    #[test]
    fn reconcile_pushes_note_unknown_to_peer() {
        let mine = vec![note("n1", vec![seg("skimble", 1, "x")])];
        let (want, push) = reconcile(&mine, &[]);
        assert!(want.is_empty());
        assert_eq!(push.len(), 1);
        assert_eq!(push[0].id, "n1");
    }

    #[test]
    fn reconcile_covers_stale_tombstone_case() {
        // Il gemello del caso descritto dall'utente: la nostra copia è già
        // cancellata (tombstone), il digest del peer mostra ancora viva —
        // deve finire in `push` così il peer la marchi cancellata anche lui.
        let mut mine_note = note("n1", vec![seg("skimble", 1, "x")]);
        mine_note.deleted = true;
        let their_digest = digest_of(&note("n1", vec![seg("skimble", 1, "x")])); // deleted: false
        let (want, push) = reconcile(&[mine_note.clone()], &[("n1".to_string(), their_digest)]);
        assert!(want.contains(&"n1".to_string()), "vogliamo comunque il loro stato per confronto");
        assert_eq!(push, vec![mine_note]);
    }

    #[test]
    fn reconcile_skips_notes_with_identical_digest() {
        let n = note("n1", vec![seg("skimble", 1, "x")]);
        let their = vec![("n1".to_string(), digest_of(&n))];
        let (want, push) = reconcile(&[n], &their);
        assert!(want.is_empty());
        assert!(push.is_empty());
    }
}
