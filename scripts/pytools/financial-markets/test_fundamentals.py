"""Test per fundamentals.py — usa FakeDataSource, nessuna rete."""

from data_sources.fake_source import FakeDataSource
import fundamentals


def test_multi_period_table_merges_revenue_and_earnings_by_period():
    fake = FakeDataSource()
    fx = fake.fetch_fundamentals("FAKE")
    table = fundamentals.multi_period_table(fx)
    assert table == [
        {"period": "2024-Q4", "revenue": 100.0, "earnings": 10.0},
        {"period": "2025-Q4", "revenue": 120.0, "earnings": 15.0},
    ]


def test_peer_comparison_table_includes_ticker_and_all_peers():
    fake = FakeDataSource()
    fake.fundamentals_by_ticker["PEER1"] = {**fake.fundamentals_by_ticker["FAKE"], "ticker": "PEER1", "market_cap": 500.0, "pe_ratio": 15.0}
    fx = fake.fetch_fundamentals("FAKE")
    table = fundamentals.peer_comparison_table(fake, "FAKE", fx, ["PEER1"])
    assert [row["ticker"] for row in table] == ["FAKE", "PEER1"]


def test_index_comparison_returns_pct_change_for_both():
    fake = FakeDataSource()
    ticker_ohlc = fake.fetch_ohlc("FAKE", "D")
    result = fundamentals.index_comparison(fake, ticker_ohlc, timeframe="D")
    assert result["index_ticker"] == "^GSPC"
    assert result["index_name"] == "S&P 500"
    assert result["ticker_change_pct"] is not None
    assert result["index_change_pct"] is not None


def test_pct_change_none_when_fewer_than_two_bars():
    assert fundamentals._pct_change([{"close": 100.0}]) is None
