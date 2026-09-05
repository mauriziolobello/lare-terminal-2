"""Test per technical_cache.py -- staleness 1gg, priorita' zoccolo, budget,
fetch fallito, round-trip. Mirror STRUTTURALE di test_fundamentals_cache.py,
entry shape diversa ({"fetched_at","bars"}), nessuna rete: fetch iniettato,
date iniettate, RNG seedato."""

import random
from datetime import date

import technical_cache

TODAY = date(2026, 8, 26)


def _entry(fetched_on: str, bars=None):
    return {"fetched_at": fetched_on, "bars": bars if bars is not None else []}


def test_load_cache_missing_file_returns_empty_dict(tmp_path):
    assert technical_cache.load_cache(tmp_path / "nope.json") == {}


def test_load_cache_corrupted_json_degrades_to_empty_dict_not_a_crash(tmp_path):
    # Un file cache troncato/corrotto (crash a meta' scrittura, disco pieno,
    # ecc.) deve degradare a "riparti da zero", mai far crashare ogni run
    # successivo con una JSONDecodeError non gestita.
    path = tmp_path / "cache.json"
    path.write_text('{"AAPL": {"fetched_at": "2026-08-2', encoding="utf-8")  # JSON troncato
    assert technical_cache.load_cache(path) == {}


def test_save_then_load_round_trips(tmp_path):
    path = tmp_path / "cache.json"
    bar = {"date": "2026-08-25", "open": 1.0, "high": 1.5, "low": 0.5, "close": 1.2, "volume": 100.0}
    cache = {"AAPL": _entry("2026-08-26", bars=[bar])}
    technical_cache.save_cache(path, cache)
    assert technical_cache.load_cache(path) == cache


def test_save_cache_is_atomic_leaves_no_temp_file_behind(tmp_path):
    # save_cache scrive su un file temporaneo nella STESSA cartella e poi
    # os.replace() lo sostituisce al path reale -- a scrittura conclusa non
    # deve restare nessun file temporaneo residuo accanto al file finale.
    path = tmp_path / "cache.json"
    technical_cache.save_cache(path, {"AAPL": _entry("2026-08-26")})
    leftovers = [p for p in tmp_path.iterdir() if p != path]
    assert leftovers == []


def test_save_cache_never_corrupts_existing_file_if_replace_fails(tmp_path, monkeypatch):
    # Simula un crash DOPO che il file temporaneo e' stato scritto ma PRIMA
    # che os.replace() lo sostituisca al file reale -- il file originale
    # (l'ultimo salvataggio riuscito) deve restare intatto e leggibile, mai
    # un JSON a meta' scritto (spec: scrittura atomica, non diretta).
    path = tmp_path / "cache.json"
    original = {"AAPL": _entry("2026-08-01")}
    technical_cache.save_cache(path, original)

    def boom(*args, **kwargs):
        raise OSError("crash simulato durante la sostituzione atomica")

    monkeypatch.setattr(technical_cache.os, "replace", boom)
    try:
        technical_cache.save_cache(path, {"AAPL": _entry("2026-08-26")})
    except OSError:
        pass
    assert technical_cache.load_cache(path) == original


def test_is_fresh_only_same_day():
    # Freschezza 1 giorno di borsa: fetchato OGGI -> fresco; fetchato ieri -> scaduto.
    assert technical_cache.is_fresh(_entry("2026-08-26"), TODAY)
    assert not technical_cache.is_fresh(_entry("2026-08-25"), TODAY)


def test_select_candidates_prioritizes_missing_index_members():
    cache = {"AAPL": _entry("2026-08-26")}  # fresco -> non candidato
    rng = random.Random(42)
    out = technical_cache.select_candidates(
        index_tickers=["AAPL", "MSFT", "KO"],
        all_tickers=["AAPL", "MSFT", "KO", "XXX1", "XXX2"],
        cache=cache, today=TODAY, rng=rng, budget=2)
    assert set(out) == {"MSFT", "KO"}


def test_select_candidates_never_exceeds_budget():
    rng = random.Random(42)
    out = technical_cache.select_candidates(
        index_tickers=[f"I{n}" for n in range(100)],
        all_tickers=[f"I{n}" for n in range(100)] + [f"X{n}" for n in range(100)],
        cache={}, today=TODAY, rng=rng, budget=60)
    assert len(out) == 60


def test_run_fetch_counts_new_refreshed_and_failed():
    cache = {"OLD": _entry("2026-08-01", bars=[{"date": "x", "open": 1, "high": 1, "low": 1, "close": 1, "volume": 1}])}

    def fetch_fn(ticker):
        if ticker == "BAD":
            raise ValueError("rete giu'")
        return [{"date": "2026-08-26", "open": 1.0, "high": 1.0, "low": 1.0, "close": 1.0, "volume": 1.0}]

    stats = technical_cache.run_fetch(cache, ["NEW", "OLD", "BAD"], fetch_fn, TODAY)
    assert stats == {"new": 1, "refreshed": 1, "failed": 1}
    assert cache["NEW"]["fetched_at"] == "2026-08-26"
    assert cache["OLD"]["fetched_at"] == "2026-08-26"


def test_run_fetch_failed_entry_has_empty_bars_not_an_exception():
    cache = {}

    def fetch_fn(ticker):
        raise ValueError("rete giu'")

    technical_cache.run_fetch(cache, ["BAD"], fetch_fn, TODAY)
    assert cache["BAD"]["bars"] == []
    assert cache["BAD"]["fetched_at"] == "2026-08-26"
    assert technical_cache.is_fresh(cache["BAD"], TODAY)


def test_run_fetch_preserves_existing_bars_on_transient_failure():
    good_bar = {"date": "2026-08-01", "open": 1.0, "high": 1.5, "low": 0.5, "close": 1.2, "volume": 100.0}
    cache = {"OLD": _entry("2026-08-25", bars=[good_bar])}  # scaduto ma con storia valida

    def fetch_fn(ticker):
        raise ValueError("rete giu' -- transitorio")

    stats = technical_cache.run_fetch(cache, ["OLD"], fetch_fn, TODAY)
    assert stats == {"new": 0, "refreshed": 0, "failed": 1}
    # Le barre valide NON vengono cancellate da un fallimento transitorio.
    assert cache["OLD"]["bars"] == [good_bar]
    assert cache["OLD"]["fetched_at"] == "2026-08-26"


def test_select_candidates_fills_remaining_budget_with_random_sample():
    rng = random.Random(42)
    out = technical_cache.select_candidates(
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
    out1 = technical_cache.select_candidates(rng=random.Random(7), **args)
    out2 = technical_cache.select_candidates(rng=random.Random(7), **args)
    assert out1 == out2
