"""Test per discoveries.py — finestra di esclusione 30gg, prime apparizioni,
round-trip. Date sempre iniettate, mai date.today() nei test."""

from datetime import date
from pathlib import Path

import discoveries

TODAY = date(2026, 7, 27)


def test_load_ledger_missing_file_returns_empty(tmp_path):
    assert discoveries.load_ledger(tmp_path / "nope.json") == {}


def test_save_then_load_round_trips(tmp_path):
    path = tmp_path / "discoveries.json"
    ledger = {"AAPL": "2026-07-27"}
    discoveries.save_ledger(path, ledger)
    assert discoveries.load_ledger(path) == ledger


def test_excluded_inside_the_30_day_window():
    ledger = {"AAPL": "2026-07-01", "KO": "2026-06-01"}
    # AAPL: 26 giorni fa → escluso. KO: 56 giorni fa → riammesso.
    assert discoveries.excluded(ledger, TODAY) == {"AAPL"}


def test_excluded_boundary_exactly_30_days_is_readmitted():
    ledger = {"AAPL": "2026-06-27"}  # esattamente 30 giorni
    assert discoveries.excluded(ledger, TODAY) == set()


def test_is_first_appearance_true_only_when_never_in_ledger():
    ledger = {"KO": "2026-01-01"}  # anche se fuori finestra, NON è una prima
    assert discoveries.is_first_appearance(ledger, "AAPL")
    assert not discoveries.is_first_appearance(ledger, "KO")


def test_record_stamps_today_and_overwrites_old_dates():
    ledger = {"KO": "2026-01-01"}
    discoveries.record(ledger, ["AAPL", "KO"], TODAY)
    assert ledger == {"AAPL": "2026-07-27", "KO": "2026-07-27"}
