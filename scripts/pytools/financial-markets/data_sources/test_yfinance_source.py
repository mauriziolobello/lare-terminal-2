"""Test per le funzioni pure di normalizzazione in yfinance_source.py.
NON testa YFinanceDataSource stesso (wrapper sottile su yfinance, verifica
reale via smoke test manuale, vedi il docstring del modulo) -- solo la
logica di normalizzazione, che è pura e non tocca la rete."""

from data_sources.yfinance_source import _normalize_debt_to_equity, _normalize_dividend_yield


def test_normalize_debt_to_equity_divides_by_100():
    assert _normalize_debt_to_equity(45.2) == 45.2 / 100


def test_normalize_debt_to_equity_none_passthrough():
    assert _normalize_debt_to_equity(None) is None


def test_normalize_dividend_yield_above_one_divided():
    assert _normalize_dividend_yield(3.5) == 0.035  # 3.5 = "3.5%" -> 0.035


def test_normalize_dividend_yield_at_or_below_one_passthrough():
    assert _normalize_dividend_yield(0.8) == 0.8  # già frazionario (sub-1%)
    assert _normalize_dividend_yield(1.0) == 1.0  # boundary: <=1 non diviso


def test_normalize_dividend_yield_none_passthrough():
    assert _normalize_dividend_yield(None) is None
