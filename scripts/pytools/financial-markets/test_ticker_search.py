"""Test per ticker_search.py — fixture locale, nessuna rete."""

import json

import ticker_search


FIXTURE = [
    {"ticker": "AAPL", "name": "Apple Inc."},
    {"ticker": "NVO", "name": "Novo Nordisk A/S"},
    {"ticker": "MSFT", "name": "Microsoft Corporation"},
]


def test_search_matches_partial_company_name_case_insensitive():
    assert ticker_search.search("novo", FIXTURE) == [{"ticker": "NVO", "name": "Novo Nordisk A/S"}]


def test_search_matches_partial_ticker():
    assert ticker_search.search("aap", FIXTURE) == [{"ticker": "AAPL", "name": "Apple Inc."}]


def test_search_empty_query_returns_empty_list():
    assert ticker_search.search("", FIXTURE) == []
    assert ticker_search.search("   ", FIXTURE) == []


def test_search_no_match_returns_empty_list():
    assert ticker_search.search("zzz-nomatch", FIXTURE) == []


def test_search_respects_limit():
    many = [{"ticker": f"T{i}", "name": f"Company {i}"} for i in range(20)]
    assert len(ticker_search.search("company", many, limit=5)) == 5


def test_load_tickers_reads_json_file(tmp_path):
    path = tmp_path / "tickers_us.json"
    path.write_text(json.dumps(FIXTURE), encoding="utf-8")
    assert ticker_search.load_tickers(path) == FIXTURE
