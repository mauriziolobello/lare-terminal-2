"""Metriche multi-periodo e tabelle comparative: (a) trend dello stesso
titolo su più periodi, (b) titolo-vs-peer/settore, (c) titolo-vs-indice di
mercato. Dipende solo da data_sources.DataSource, mai da yfinance
direttamente."""

from data_sources import DataSource

INDEX_TICKER = "^GSPC"
# Non tutti conoscono il significato di "^GSPC" — il nome leggibile viaggia
# insieme al ticker in `index_comparison` (refinement 2026-07-23), non solo
# in questo commento.
INDEX_NAME = "S&P 500"


def multi_period_table(fundamentals: dict) -> list[dict]:
    """(a) Trend dello stesso titolo: unisce revenue/earnings per periodo in
    righe {'period', 'revenue', 'earnings'}."""
    revenue = dict(fundamentals["revenue_by_period"])
    earnings = dict(fundamentals["earnings_by_period"])
    periods = sorted(set(revenue) | set(earnings))
    return [{"period": p, "revenue": revenue.get(p), "earnings": earnings.get(p)} for p in periods]


def peer_comparison_table(source: DataSource, ticker: str, fundamentals: dict, peer_tickers: list[str]) -> list[dict]:
    """(b) Titolo vs peer/settore: multipli (P/E, market cap) affiancati."""
    rows = [{"ticker": ticker, "market_cap": fundamentals["market_cap"], "pe_ratio": fundamentals["pe_ratio"]}]
    for peer_ticker in peer_tickers:
        peer_fundamentals = source.fetch_fundamentals(peer_ticker)
        rows.append({"ticker": peer_ticker, "market_cap": peer_fundamentals["market_cap"], "pe_ratio": peer_fundamentals["pe_ratio"]})
    return rows


def index_comparison(source: DataSource, ticker_ohlc: list[dict], timeframe: str = "D") -> dict:
    """(c) Titolo vs indice di mercato: variazione % sul periodo disponibile,
    per entrambi, sullo stesso timeframe."""
    index_bars = source.fetch_index_series(INDEX_TICKER, timeframe)
    return {
        "ticker_change_pct": _pct_change(ticker_ohlc),
        "index_change_pct": _pct_change(index_bars),
        "index_ticker": INDEX_TICKER,
        "index_name": INDEX_NAME,
    }


def _pct_change(bars: list[dict]) -> float | None:
    if len(bars) < 2:
        return None
    first, last = bars[0]["close"], bars[-1]["close"]
    if first == 0:
        return None
    return (last - first) / first * 100
