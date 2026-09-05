//! Modello dati e merge CRDT delle note del "Blocco note" (Library).
//! Design: Docs/superpowers/specs/2026-07-29-library-notes-design.md §3-4.

use serde::{Deserialize, Serialize};

pub mod digest;
pub mod store;

/// Il contributo di UNA macchina al corpo di una nota. Al massimo un segmento
/// per macchina: una nuova modifica dalla stessa macchina SOSTITUISCE il
/// proprio segmento (identificato dalla chiave `machine`), non ne aggiunge uno.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
    pub machine: String,
    /// Contatore LOCALE a questa macchina (non condiviso con le altre):
    /// incrementa di 1 ad ogni modifica di QUESTA macchina. Usato per decidere
    /// quale dei due segmenti con la stessa chiave `machine` è più recente.
    pub seq: u64,
    pub text: String,
    /// Solo display — MAI usato per decidere un merge.
    pub edited_at_ms: u64,
}

/// Una nota: titolo (last-write-wins) + corpo (unione di segmenti per macchina).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub id: String,
    pub title: String,
    /// (macchina, orologio di parete in ms) dell'ultima modifica al titolo —
    /// usato SOLO per il tie-break "chi vince" fra due titoli concorrenti.
    pub title_touched: (String, u64),
    pub segments: Vec<Segment>,
    pub created_by: String,
    pub created_at_ms: u64,
    /// Tombstone: una volta `true`, `merge_note` non lo rimette MAI a `false`.
    pub deleted: bool,
}

/// Unisce due copie della STESSA nota (stesso `id`) in un risultato che
/// converge indipendentemente dall'ordine/molteplicità delle chiamate:
/// idempotente, commutativa, associativa (v. design §4 — proprietà verificate
/// esplicitamente nei test sotto, non solo casi singoli).
pub fn merge_note(a: &Note, b: &Note) -> Note {
    use std::collections::HashMap;

    let mut by_machine: HashMap<String, Segment> = HashMap::new();
    for s in a.segments.iter().chain(b.segments.iter()) {
        by_machine
            .entry(s.machine.clone())
            .and_modify(|cur| {
                if s.seq > cur.seq {
                    *cur = s.clone();
                }
            })
            .or_insert_with(|| s.clone());
    }
    let mut segments: Vec<Segment> = by_machine.into_values().collect();
    segments.sort_by(|x, y| x.machine.cmp(&y.machine)); // ordine deterministico

    // Titolo: LWW sull'orologio di parete, tie-break sul nome macchina.
    let (title, title_touched) = if b.title_touched.1 > a.title_touched.1
        || (b.title_touched.1 == a.title_touched.1 && b.title_touched.0 > a.title_touched.0)
    {
        (b.title.clone(), b.title_touched.clone())
    } else {
        (a.title.clone(), a.title_touched.clone())
    };

    Note {
        id: a.id.clone(),
        title,
        title_touched,
        segments,
        created_by: a.created_by.clone(),
        created_at_ms: a.created_at_ms,
        deleted: a.deleted || b.deleted, // tombstone vince sempre
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(machine: &str, seq: u64, text: &str) -> Segment {
        Segment { machine: machine.into(), seq, text: text.into(), edited_at_ms: seq * 1000 }
    }

    fn note(id: &str, segments: Vec<Segment>, title_from: (&str, u64)) -> Note {
        Note {
            id: id.into(),
            title: "titolo".into(),
            title_touched: (title_from.0.into(), title_from.1),
            segments,
            created_by: "skimble".into(),
            created_at_ms: 0,
            deleted: false,
        }
    }

    #[test]
    fn merge_is_idempotent() {
        let n = note("n1", vec![seg("skimble", 1, "ciao")], ("skimble", 100));
        assert_eq!(merge_note(&n, &n), n);
    }

    #[test]
    fn merge_is_commutative() {
        let a = note("n1", vec![seg("skimble", 1, "A")], ("skimble", 100));
        let b = note("n1", vec![seg("rumpleteazer", 1, "B")], ("rumpleteazer", 50));
        assert_eq!(merge_note(&a, &b), merge_note(&b, &a));
    }

    #[test]
    fn merge_is_associative() {
        let a = note("n1", vec![seg("skimble", 1, "A")], ("skimble", 10));
        let b = note("n1", vec![seg("rumpleteazer", 1, "B")], ("rumpleteazer", 20));
        let c = note("n1", vec![seg("mungojerrie", 1, "C")], ("mungojerrie", 30));
        assert_eq!(merge_note(&merge_note(&a, &b), &c), merge_note(&a, &merge_note(&b, &c)));
    }

    #[test]
    fn merge_takes_higher_seq_segment_from_same_machine() {
        let a = note("n1", vec![seg("skimble", 1, "vecchio")], ("skimble", 10));
        let b = note("n1", vec![seg("skimble", 2, "nuovo")], ("skimble", 10));
        let m = merge_note(&a, &b);
        assert_eq!(m.segments, vec![seg("skimble", 2, "nuovo")]);
    }

    /// Ramo di tie-break MAI esercitato prima (gap di copertura chiuso nella
    /// review finale di branch, insieme al FIX 1 su `next_note_id`): due segmenti
    /// con la STESSA `machine` e lo STESSO `seq`.
    ///
    /// È la precondizione che il doc-comment di `merge_note` assume implicitamente:
    /// `(machine, seq)` identifica un'unica modifica, quindi due segmenti con la
    /// stessa chiave sono la STESSA modifica ritrasmessa — stesso testo. Il ramo
    /// `if s.seq > cur.seq` NON scatta (nessuno dei due è maggiore) e vince
    /// deterministicamente il primo incontrato: comportamento corretto proprio
    /// perché i due testi coincidono.
    ///
    /// Questa precondizione è ciò che il FIX 1 rende davvero vera: PRIMA del fix,
    /// dopo un riavvio, due note DIVERSE potevano nascere con lo stesso id e con
    /// `seq = 1` sulla stessa macchina — cioè stessa chiave `(machine, seq)` con
    /// testo diverso, un caso in cui il merge NON è commutativo. Seedando
    /// `next_note_id` dallo store, quello stato non è più raggiungibile.
    ///
    /// Test di CARATTERIZZAZIONE, non ciclo TDD: `merge_note` non è stato
    /// modificato, quindi è verde fin da subito (nessun RED possibile).
    #[test]
    fn merge_same_machine_same_seq_same_text_is_lossless_and_order_independent() {
        let a = note("n1", vec![seg("skimble", 3, "identico")], ("skimble", 10));
        let b = note("n1", vec![seg("skimble", 3, "identico")], ("skimble", 10));

        let ab = merge_note(&a, &b);
        let ba = merge_note(&b, &a);

        assert_eq!(ab.segments.len(), 1, "stessa chiave (machine, seq): UN solo segmento, nessuna duplicazione");
        assert_eq!(ab.segments[0], seg("skimble", 3, "identico"), "nessuna perdita di testo");
        assert_eq!(ab, ba, "commutativo anche sul ramo di parità di seq");
        assert_eq!(merge_note(&ab, &b), ab, "idempotente: ri-fondere non cambia nulla");
    }

    #[test]
    fn merge_unions_segments_from_different_machines_without_duplication() {
        let a = note("n1", vec![seg("skimble", 1, "A")], ("skimble", 10));
        let b = note("n1", vec![seg("rumpleteazer", 1, "B")], ("skimble", 10));
        let m = merge_note(&a, &b);
        assert_eq!(m.segments.len(), 2, "nessuna duplicazione, un segmento per macchina");
        // Ri-fondere lo stesso stato non deve MAI far crescere il numero di segmenti
        // (regressione diretta del bug "contatore singolo + concatenazione", design §4).
        let m2 = merge_note(&m, &b);
        assert_eq!(m2.segments.len(), 2);
    }

    #[test]
    fn merge_title_last_write_wins_by_wallclock() {
        let a = note("n1", vec![], ("skimble", 100));
        let mut b = note("n1", vec![], ("rumpleteazer", 200));
        b.title = "titolo nuovo".into();
        let m = merge_note(&a, &b);
        assert_eq!(m.title, "titolo nuovo");
        assert_eq!(m.title_touched, ("rumpleteazer".to_string(), 200));
    }

    #[test]
    fn merge_tombstone_never_resets_to_false() {
        let mut a = note("n1", vec![], ("skimble", 10));
        a.deleted = true;
        let b = note("n1", vec![seg("rumpleteazer", 5, "nuovo testo")], ("rumpleteazer", 20));
        let m = merge_note(&a, &b);
        assert!(m.deleted, "cancellato vince sempre, indipendentemente da chi arriva dopo");
    }
}
