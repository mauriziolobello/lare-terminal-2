"""Test per DataSource.unavailable_fields() (metodo concreto, default sulla
ABC) e format_unavailable_warning() -- Task 5/7/8 finale, decisione utente
2026-08-14 (vedi task-5-final-brief.md): la fonte dichiara quali campi non
puo' STRUTTURALMENTE fornire, cosi' che report.py/screeners possano
mostrare un avviso VISIBILE invece di nascondere la lacuna in silenzio.

FakeDataSource NON viene toccata da questo task (nessuna modifica al file)
-- eredita il default set() gratis dalla ABC, verificato qui sotto."""

from data_sources import format_unavailable_warning
from data_sources.fake_source import FakeDataSource
from data_sources.ibkr_source import IbkrDataSource


def test_fake_data_source_unavailable_fields_defaults_to_empty_set():
    # Nessuna modifica a fake_source.py: il default e' ereditato dalla ABC.
    assert FakeDataSource().unavailable_fields() == set()


def test_format_unavailable_warning_returns_none_for_empty_set():
    # Caso comune (YFinance/FakeDataSource): nessun avviso da mostrare.
    assert format_unavailable_warning(set()) is None


def test_format_unavailable_warning_contains_readable_italian_label():
    warning = format_unavailable_warning({"peg_ratio"})
    assert warning is not None
    assert "PEG ratio" in warning
    assert warning.startswith("> ⚠️")


def test_format_unavailable_warning_sorts_multiple_labels_deterministically():
    # L'ordine non deve dipendere dall'ordine di iterazione di un set
    # (non garantito fra run) -- l'avviso deve essere stabile.
    first = format_unavailable_warning({"name", "sector", "pe_ratio"})
    second = format_unavailable_warning({"pe_ratio", "name", "sector"})
    assert first == second


def test_ibkr_data_source_unavailable_fields_is_not_empty_and_contains_known_gaps():
    # Invarianti minimi (Reuters Fundamentals non sottoscritto + scope
    # deliberatamente ridotto, vedi task-5-final-brief.md) -- non l'intero
    # insieme letterale, per non rendere il test fragile a ogni piccola
    # modifica dell'insieme finale.
    source = IbkrDataSource(port=4001, client_id=731)
    fields = source.unavailable_fields()
    assert fields
    assert "name" in fields
    assert "target_mean_price" in fields


def test_format_unavailable_warning_composes_with_the_real_ibkr_field_set():
    # Composizione REALE (non solo un set sintetico a 1 elemento, vedi test
    # sopra): il vero insieme di IbkrDataSource passato al vero formattatore
    # -- garantisce che OGNI campo dichiarato assente da IbkrDataSource abbia
    # un'etichetta leggibile in _FIELD_LABELS. Senza questo test, un campo
    # aggiunto a IbkrDataSource.unavailable_fields() senza la corrispondente
    # etichetta farebbe trapelare silenziosamente un identificatore
    # snake_case grezzo (via .get(f, f)) in un avviso italiano per l'utente
    # finale -- nessuna eccezione lo segnalerebbe, solo questo test.
    from data_sources import _FIELD_LABELS

    fields = IbkrDataSource(port=4001, client_id=731).unavailable_fields()

    missing_labels = fields - set(_FIELD_LABELS)
    assert not missing_labels, f"campi senza etichetta leggibile in _FIELD_LABELS: {missing_labels}"

    warning = format_unavailable_warning(fields)
    assert warning is not None
    assert warning.startswith("> ⚠️ Fonte dati: campi non disponibili — ")
