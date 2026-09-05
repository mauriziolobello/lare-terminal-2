"""Test per fundamentals_cache.py — staleness, priorità zoccolo, budget,
fetch fallito, round-trip. Nessuna rete: fetch iniettato, date iniettate,
RNG seedato."""

import json
import random
from datetime import date
from pathlib import Path

import fundamentals_cache

TODAY = date(2026, 7, 27)


def _entry(fetched_on: str, sector="Technology"):
    return {"sector": sector, "market_cap": 1.0, "ps_ratio": 1.0,
            "revenue_growth": 0.1, "fifty_two_week_high": 10.0,
            "current_price": 9.0, "target_mean_price": 11.0,
            "peg_ratio": 1.0, "fetched_at": fetched_on}


def test_load_cache_missing_file_returns_empty_dict(tmp_path):
    assert fundamentals_cache.load_cache(tmp_path / "nope.json") == {}


def test_save_then_load_round_trips(tmp_path):
    path = tmp_path / "cache.json"
    cache = {"AAPL": _entry("2026-07-27")}
    fundamentals_cache.save_cache(path, cache)
    assert fundamentals_cache.load_cache(path) == cache


def test_is_fresh_within_seven_days():
    assert fundamentals_cache.is_fresh(_entry("2026-07-21"), TODAY)      # 6 giorni
    assert not fundamentals_cache.is_fresh(_entry("2026-07-19"), TODAY)  # 8 giorni


def test_is_fresh_exactly_seven_days_is_stale():
    # Confine esplicito: "staleness 7 giorni" = a 7 giorni compiuti va
    # rinfrescato (stessa semantica di refresh_tickers.is_stale).
    assert not fundamentals_cache.is_fresh(_entry("2026-07-20"), TODAY)


def test_select_candidates_prioritizes_missing_index_members():
    cache = {"AAPL": _entry("2026-07-27")}  # fresco → non candidato
    rng = random.Random(42)
    out = fundamentals_cache.select_candidates(
        index_tickers=["AAPL", "MSFT", "KO"],
        all_tickers=["AAPL", "MSFT", "KO", "XXX1", "XXX2"],
        cache=cache, today=TODAY, rng=rng, budget=2)
    # Budget 2: i due membri indice mancanti vengono PRIMA di qualunque
    # campione fuori-indice.
    assert set(out) == {"MSFT", "KO"}


def test_select_candidates_fills_remaining_budget_with_random_sample():
    rng = random.Random(42)
    out = fundamentals_cache.select_candidates(
        index_tickers=["AAPL"],
        all_tickers=["AAPL", "X1", "X2", "X3", "X4", "X5"],
        cache={}, today=TODAY, rng=rng, budget=3)
    assert out[0] == "AAPL"
    assert len(out) == 3
    assert set(out[1:]) <= {"X1", "X2", "X3", "X4", "X5"}


def test_select_candidates_is_deterministic_with_seeded_rng():
    args = dict(index_tickers=["AAPL"],
                all_tickers=["AAPL", "X1", "X2", "X3", "X4", "X5"],
                cache={}, today=TODAY, budget=3)
    out1 = fundamentals_cache.select_candidates(rng=random.Random(7), **args)
    out2 = fundamentals_cache.select_candidates(rng=random.Random(7), **args)
    assert out1 == out2


def test_select_candidates_includes_stale_index_members():
    cache = {"AAPL": _entry("2026-01-01")}  # molto scaduto
    rng = random.Random(42)
    out = fundamentals_cache.select_candidates(
        index_tickers=["AAPL"], all_tickers=["AAPL"],
        cache=cache, today=TODAY, rng=rng, budget=5)
    assert out == ["AAPL"]


def test_select_candidates_never_exceeds_budget():
    rng = random.Random(42)
    out = fundamentals_cache.select_candidates(
        index_tickers=[f"I{n}" for n in range(100)],
        all_tickers=[f"I{n}" for n in range(100)] + [f"X{n}" for n in range(100)],
        cache={}, today=TODAY, rng=rng, budget=60)
    assert len(out) == 60


def test_run_fetch_counts_new_refreshed_and_failed():
    cache = {"OLD": _entry("2026-01-01")}

    def fetch_fn(ticker):
        if ticker == "BAD":
            raise ValueError("rete giù")
        return {"sector": "Technology", "market_cap": 1.0, "ps_ratio": 1.0,
                "revenue_growth": 0.1, "fifty_two_week_high": 10.0,
                "current_price": 9.0, "target_mean_price": 11.0, "peg_ratio": 1.0}

    stats = fundamentals_cache.run_fetch(cache, ["NEW", "OLD", "BAD"], fetch_fn, TODAY)
    assert stats == {"new": 1, "refreshed": 1, "failed": 1}
    assert cache["NEW"]["fetched_at"] == "2026-07-27"
    assert cache["OLD"]["fetched_at"] == "2026-07-27"
    # Il fetch fallito è comunque "visitato": entry con campi null e data —
    # non verrà riprovato a ogni run finché non scade.
    assert cache["BAD"]["sector"] is None
    assert cache["BAD"]["fetched_at"] == "2026-07-27"
    assert fundamentals_cache.is_fresh(cache["BAD"], TODAY)


def test_run_fetch_failed_entry_has_null_values_for_all_snapshot_keys_including_new_ones():
    cache = {}

    def fetch_fn(ticker):
        raise ValueError("rete giù")

    fundamentals_cache.run_fetch(cache, ["BAD"], fetch_fn, TODAY)
    for key in ("pe_ratio", "debt_to_equity", "dividend_yield",
                "payout_ratio", "target_high_price", "target_low_price",
                "industry"):
        assert cache["BAD"][key] is None, f"manca {key} nell'entry fallita"
