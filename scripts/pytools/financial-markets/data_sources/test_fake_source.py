"""Test per data_sources/fake_source.py — verifica solo che FakeDataSource
implementi l'intero contratto DataSource (ABC) e ritorni i dati fissi
attesi. yfinance_source.py NON ha test qui: wrapper sottile su libreria
esterna, verificato via smoke test manuale (Task 12)."""

from data_sources.fake_source import FakeDataSource


def test_fake_data_source_is_instantiable_implementing_the_full_contract():
    # Se un metodo abstract non fosse implementato, Python solleva TypeError
    # all'istanziazione — questo È il RED prima dello Step 3.
    source = FakeDataSource()
    assert source.fetch_fundamentals("FAKE")["ticker"] == "FAKE"
    assert len(source.fetch_ohlc("FAKE", "D")) > 0
    assert len(source.fetch_options("FAKE")) > 0
    assert len(source.fetch_index_series("^GSPC", "D")) > 0


def test_fake_data_source_has_bars_for_all_five_timeframes():
    source = FakeDataSource()
    for timeframe in ("M", "W", "D", "H4", "H1"):
        assert len(source.fetch_ohlc("FAKE", timeframe)) > 0, f"manca il timeframe {timeframe}"


def test_fake_data_source_returns_a_screening_snapshot_with_all_expected_keys():
    source = FakeDataSource()
    snap = source.fetch_screening_snapshot("FAKE")
    assert set(snap.keys()) == {
        "sector", "market_cap", "ps_ratio", "revenue_growth",
        "fifty_two_week_high", "current_price", "target_mean_price", "peg_ratio",
        "pe_ratio", "debt_to_equity", "dividend_yield", "payout_ratio",
        "target_high_price", "target_low_price", "industry",
    }
    assert snap["sector"] == "Technology"
    assert snap["industry"] == "Software—Infrastructure"
    assert snap["current_price"] == 110.0


def test_fake_data_source_screening_snapshot_unknown_ticker_raises_key_error():
    # Il chiamante (fundamentals_cache.run_fetch) tratta QUALUNQUE eccezione
    # come "fetch fallito, entry null" — KeyError è il comportamento naturale
    # del fake, non serve inventare un errore custom.
    source = FakeDataSource()
    try:
        source.fetch_screening_snapshot("ZZZZNOTREAL")
        assert False, "atteso KeyError"
    except KeyError:
        pass
