"""Test per refresh_tickers.py — nessuna chiamata di rete reale: fetch_raw è
mockato, si testano solo normalize()/refresh()/is_stale() in isolamento."""

import json
import os
import time
from unittest.mock import patch

import refresh_tickers


FIXTURE_RAW = {
    "0": {"cik_str": 320193, "ticker": "AAPL", "title": "Apple Inc."},
    "1": {"cik_str": 1024305, "ticker": "NVO", "title": "Novo Nordisk A/S"},
}


def test_normalize_flattens_sec_format_to_ticker_name_pairs():
    result = refresh_tickers.normalize(FIXTURE_RAW)
    assert result == [
        {"ticker": "AAPL", "name": "Apple Inc."},
        {"ticker": "NVO", "name": "Novo Nordisk A/S"},
    ]


def test_refresh_writes_normalized_json_and_returns_count(tmp_path):
    dest = tmp_path / "tickers_us.json"
    with patch.object(refresh_tickers, "fetch_raw", return_value=FIXTURE_RAW):
        count = refresh_tickers.refresh(dest)
    assert count == 2
    written = json.loads(dest.read_text(encoding="utf-8"))
    assert written == [
        {"ticker": "AAPL", "name": "Apple Inc."},
        {"ticker": "NVO", "name": "Novo Nordisk A/S"},
    ]


def test_is_stale_true_when_file_missing(tmp_path):
    assert refresh_tickers.is_stale(tmp_path / "missing.json") is True


def test_is_stale_false_when_file_fresh(tmp_path):
    dest = tmp_path / "tickers_us.json"
    dest.write_text("[]", encoding="utf-8")
    assert refresh_tickers.is_stale(dest, max_age_seconds=3600) is False


def test_is_stale_true_when_file_older_than_max_age(tmp_path):
    dest = tmp_path / "tickers_us.json"
    dest.write_text("[]", encoding="utf-8")
    old_time = time.time() - 999999
    os.utime(dest, (old_time, old_time))
    assert refresh_tickers.is_stale(dest, max_age_seconds=3600) is True
