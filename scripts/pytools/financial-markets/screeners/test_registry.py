"""Test per il registry/dispatch di screeners/__init__.py — usa
FakeDataSource (niente rete) e tmp_path (niente file reali). Copre SOLO la
logica NUOVA (routing per id, injection di title/judgment_instructions) —
la selezione/rendering di consumer-usage è già coperta da
test_consumer_usage.py (migrato da test_screening.py). NON importa
server.py: side-effect pesanti all'import (refresh ticker,
YFinanceDataSource), stessa scelta di test_consumer_usage.py."""

import screeners
from data_sources.fake_source import FakeDataSource


def _tickers():
    return [{"ticker": "FAKE", "name": "Fake Corp"}]


def test_dispatch_invalid_screener_id_returns_error_with_valid_ids(tmp_path):
    out = screeners.dispatch("bogus-id", FakeDataSource(), _tickers(),
                             tmp_path / "cache.json", tmp_path / "discoveries.json")
    assert "error" in out
    assert "consumer-usage" in out["error"]


def test_dispatch_consumer_usage_injects_title_and_judgment_instructions(tmp_path):
    out = screeners.dispatch("consumer-usage", FakeDataSource(), _tickers(),
                             tmp_path / "cache.json", tmp_path / "discoveries.json", top=25)
    assert out["title"] == "Uso Consumer"
    assert "Istruzioni per il giudizio finale:" in out["summary"]
    assert "Giudizio uso di massa" in out["summary"]
    assert "report_markdown" in out
    assert "channel_summary" in out
    assert "title_suffix" in out  # timestamp, aggiunto da consumer_usage.run


def test_list_items_returns_id_title_description_for_every_screener():
    items = screeners.list_items()
    assert len(items) >= 1
    assert {"id", "title", "description"} <= set(items[0].keys())
    ids = [i["id"] for i in items]
    assert "consumer-usage" in ids


def test_registry_dict_keys_match_entry_id():
    for key, entry in screeners.SCREENERS.items():
        assert key == entry.id


def test_dispatch_goldman_sachs_injects_title_and_judgment_instructions(tmp_path):
    out = screeners.dispatch("goldman-sachs", FakeDataSource(), _tickers(),
                             tmp_path / "cache.json", tmp_path / "discoveries.json", top=5)
    assert out["title"] == "Goldman Sachs"
    assert "Istruzioni per il giudizio finale:" in out["summary"]
    assert "Giudizio equity research" in out["summary"]
    assert "report_markdown" in out
    assert "channel_summary" in out


def test_list_items_includes_all_four_screeners():
    ids = [i["id"] for i in screeners.list_items()]
    assert "consumer-usage" in ids
    assert "goldman-sachs" in ids
    assert "jensen-huang" in ids
    assert "citadel" in ids


def test_dispatch_jensen_huang_injects_title_and_judgment_instructions(tmp_path):
    out = screeners.dispatch("jensen-huang", FakeDataSource(), _tickers(),
                             tmp_path / "cache.json", tmp_path / "discoveries.json", top=5)
    assert out["title"] == "Jensen Huang"
    assert "Istruzioni per il giudizio finale:" in out["summary"]
    assert "Giudizio ecosistema AI" in out["summary"]
    assert "report_markdown" in out
    assert "channel_summary" in out


def test_dispatch_citadel_injects_title_and_judgment_instructions(tmp_path):
    cache_path = tmp_path / "cache.json"
    cache_path.write_text('{"FAKE": {"industry": "Software—Infrastructure", "fetched_at": "2026-08-01"}}', encoding="utf-8")
    out = screeners.dispatch("citadel", FakeDataSource(), _tickers(),
                             cache_path, tmp_path / "discoveries.json", top=5)
    assert out["title"] == "Citadel"
    assert "Istruzioni per il giudizio finale:" in out["summary"]
    assert "Giudizio tecnico" in out["summary"]
    assert "report_markdown" in out
    assert "channel_summary" in out


def test_dispatch_citadel_survives_missing_fundamentals_cache(tmp_path):
    # cache_path non esiste -- nessuna industry nota per nessun ticker,
    # selezione vuota, mai un'eccezione (stesso principio degli altri tre).
    out = screeners.dispatch("citadel", FakeDataSource(), _tickers(),
                             tmp_path / "cache.json", tmp_path / "discoveries.json")
    assert "report_markdown" in out
    assert "Nessun titolo qualificato" in out["report_markdown"]


# --- avviso campi non disponibili (Task 5/7/8 finale, 2026-08-14) ---------
# Mirror di test_report.py::test_build_report_shows_visible_warning_...


def test_dispatch_shows_visible_warning_when_source_reports_unavailable_fields(tmp_path):
    fake = FakeDataSource()
    fake.unavailable_fields = lambda: {"peg_ratio"}
    out = screeners.dispatch("consumer-usage", fake, _tickers(),
                             tmp_path / "cache.json", tmp_path / "discoveries.json")
    assert "PEG ratio" in out["report_markdown"]


def test_dispatch_omits_warning_when_source_has_no_unavailable_fields(tmp_path):
    out = screeners.dispatch("consumer-usage", FakeDataSource(), _tickers(),
                             tmp_path / "cache.json", tmp_path / "discoveries.json")
    assert "⚠️ Fonte dati" not in out["report_markdown"]


# Fix E (review-fix-wave, 2026-08-14): l'avviso deve arrivare ANCHE a
# result["summary"] (letto dall'AI per il proprio giudizio finale, vedi
# docstring di run_screener in server.py), non solo a
# result["report_markdown"] (mostrato all'utente). Prima del fix,
# result["summary"] veniva costruito PRIMA del blocco warning e non lo
# includeva mai -- mirror del test sopra, ma su summary.
def test_dispatch_shows_visible_warning_in_summary_too_when_source_reports_unavailable_fields(tmp_path):
    fake = FakeDataSource()
    fake.unavailable_fields = lambda: {"peg_ratio"}
    out = screeners.dispatch("consumer-usage", fake, _tickers(),
                             tmp_path / "cache.json", tmp_path / "discoveries.json")
    assert "PEG ratio" in out["summary"]


class _IbkrLikeSnapshotSource(FakeDataSource):
    """Imita ESATTAMENTE lo shape ritornato da
    IbkrDataSource.fetch_screening_snapshot (Reuters Fundamentals non
    sottoscritto -- vedi ibkr_source.py/task-5-final-brief.md): sector
    None (entry scartata da build_rows sia in consumer_usage.py sia in
    goldman_sachs.py), tutti gli altri campi None. Eredita il resto da
    FakeDataSource -- non serve per questi due test (dispatch() non
    tocca fetch_ohlc/fetch_options qui)."""

    def fetch_screening_snapshot(self, ticker):
        return {
            "sector": None, "market_cap": None, "ps_ratio": None,
            "revenue_growth": None, "fifty_two_week_high": None,
            "current_price": None, "target_mean_price": None,
            "peg_ratio": None, "pe_ratio": None, "debt_to_equity": None,
            "dividend_yield": None, "payout_ratio": None,
            "target_high_price": None, "target_low_price": None,
            "industry": None,
        }

    def unavailable_fields(self):
        return {
            "sector", "market_cap", "current_price", "fifty_two_week_high",
            "pe_ratio", "ps_ratio", "debt_to_equity", "dividend_yield",
            "payout_ratio", "revenue_growth", "peg_ratio",
            "target_mean_price", "target_high_price", "target_low_price",
            "industry",
        }


def test_dispatch_consumer_usage_survives_ibkr_like_snapshot_with_all_fields_none(tmp_path):
    # Verifica aggiuntiva Step 4: nessuno dei due screener solleva con una
    # fonte IBKR-like (tutti i campi screening_snapshot None) — sector=None
    # fa scartare ogni entry in build_rows, quindi selezione vuota, mai
    # un'eccezione.
    out = screeners.dispatch("consumer-usage", _IbkrLikeSnapshotSource(), _tickers(),
                             tmp_path / "cache.json", tmp_path / "discoveries.json")
    assert "report_markdown" in out
    assert "⚠️ Fonte dati" in out["report_markdown"]


def test_dispatch_goldman_sachs_survives_ibkr_like_snapshot_with_all_fields_none(tmp_path):
    out = screeners.dispatch("goldman-sachs", _IbkrLikeSnapshotSource(), _tickers(),
                             tmp_path / "cache.json", tmp_path / "discoveries.json")
    assert "report_markdown" in out
    assert "⚠️ Fonte dati" in out["report_markdown"]


def test_dispatch_jensen_huang_survives_ibkr_like_snapshot_with_all_fields_none(tmp_path):
    # industry=None fa scartare ogni entry in jensen_huang.build_rows —
    # selezione vuota, mai un'eccezione (stesso principio degli altri due).
    out = screeners.dispatch("jensen-huang", _IbkrLikeSnapshotSource(), _tickers(),
                             tmp_path / "cache.json", tmp_path / "discoveries.json")
    assert "report_markdown" in out
    assert "⚠️ Fonte dati" in out["report_markdown"]
