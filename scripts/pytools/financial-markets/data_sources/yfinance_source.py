"""YFinanceDataSource — prima implementazione di DataSource. Wrapper sottile
su `yfinance`: NON testato in unit (nessun valore nel mockare una libreria
esterna riga per riga) — verifica reale tramite smoke test manuale
(stock_report("AAPL") dal vivo, Task 12)."""

import pandas as pd
import yfinance as yf

from . import DataSource, Fundamentals, OhlcBar, OptionContract, ScreeningSnapshot

# yfinance non ha un intervallo nativo "4h": lo ricaviamo resamplando i dati
# orari (1h) via pandas. Mappa timeframe→(interval yfinance, period, resample).
_TIMEFRAME_TO_YFINANCE = {
    "M": ("1mo", "5y", None),
    "W": ("1wk", "2y", None),
    "D": ("1d", "6mo", None),
    "H4": ("1h", "60d", "4h"),
    "H1": ("1h", "60d", None),
}


def _rows_to_bars(df: pd.DataFrame) -> list[OhlcBar]:
    return [
        {
            "date": str(index),
            "open": float(row["Open"]),
            "high": float(row["High"]),
            "low": float(row["Low"]),
            "close": float(row["Close"]),
            "volume": float(row["Volume"]),
        }
        for index, row in df.iterrows()
    ]


def _fetch_history(ticker: str, timeframe: str) -> list[OhlcBar]:
    interval, period, resample = _TIMEFRAME_TO_YFINANCE[timeframe]
    df = yf.Ticker(ticker).history(period=period, interval=interval)
    if resample:
        df = df.resample(resample).agg({
            "Open": "first", "High": "max", "Low": "min", "Close": "last", "Volume": "sum",
        }).dropna()
    return _rows_to_bars(df)


def _normalize_debt_to_equity(raw: float | None) -> float | None:
    """yfinance espone debtToEquity in punti percentuali (es. 45.2 = 45.2%)
    -- normalizzato a rapporto (0.452) per essere letto come "debito/
    patrimonio" nel senso italiano standard. Pura: nessuna chiamata di
    rete, testabile senza mockare yfinance."""
    return (raw / 100) if raw is not None else None


def _normalize_dividend_yield(raw: float | None) -> float | None:
    """dividendYield: formato non verificato empiricamente su questa
    installazione di yfinance (spec Docs/superpowers/specs/2026-08-11-
    screener-goldman-sachs-design.md §3/§12) -- normalizzazione difensiva:
    valori > 1 trattati come già-percentuale (es. 3.5 = 3.5% -> /100),
    valori <= 1 assunti già frazionari. Campo SOLO display (mai nello
    score) -- un errore di scala qui non altera il ranking, ma la
    logica stessa è pura e va comunque verificata con un test."""
    if raw is None:
        return None
    return raw / 100 if raw > 1 else raw


class YFinanceDataSource(DataSource):
    def fetch_fundamentals(self, ticker: str) -> Fundamentals:
        handle = yf.Ticker(ticker)
        info = handle.info
        financials = handle.financials
        revenue_by_period: list[tuple[str, float]] = []
        earnings_by_period: list[tuple[str, float]] = []
        if financials is not None and not financials.empty:
            if "Total Revenue" in financials.index:
                revenue_by_period = [(str(col.date()), float(val)) for col, val in financials.loc["Total Revenue"].items()][::-1]
            if "Net Income" in financials.index:
                earnings_by_period = [(str(col.date()), float(val)) for col, val in financials.loc["Net Income"].items()][::-1]
        return {
            "ticker": ticker,
            "name": info.get("longName", ticker),
            "sector": info.get("sector", "n/d"),
            "industry": info.get("industry", "n/d"),
            "market_cap": info.get("marketCap"),
            "pe_ratio": info.get("trailingPE"),
            # "currentPrice" manca per alcuni tipi di strumento (es. alcuni
            # ETF) — "regularMarketPrice" è il fallback che yfinance stesso
            # usa più diffusamente.
            "current_price": info.get("currentPrice") or info.get("regularMarketPrice"),
            "revenue_by_period": revenue_by_period,
            "earnings_by_period": earnings_by_period,
        }

    def fetch_ohlc(self, ticker: str, timeframe: str) -> list[OhlcBar]:
        return _fetch_history(ticker, timeframe)

    def fetch_options(self, ticker: str) -> list[OptionContract]:
        # Refinement 2026-07-23: SOLO la scadenza più vicina (non le prime 3
        # come prima) — una vera option chain si legge per singola
        # scadenza; limitare a un solo expiry è ciò che rende sensato
        # centrare la tabella sullo strike ATM in option_chain.py (più
        # scadenze mescolate insieme avrebbero più righe per lo stesso
        # strike, senza un ordinamento naturale).
        handle = yf.Ticker(ticker)
        expiries = handle.options
        if not expiries:
            return []
        expiry = expiries[0]
        chain = handle.option_chain(expiry)
        contracts: list[OptionContract] = []
        for option_type, df in (("call", chain.calls), ("put", chain.puts)):
            for _, row in df.iterrows():
                contracts.append({
                    "strike": float(row["strike"]),
                    "expiry": expiry,
                    "iv": float(row.get("impliedVolatility") or 0) or None,
                    "option_type": option_type,
                    "bid": float(row.get("bid") or 0) or None,
                    "ask": float(row.get("ask") or 0) or None,
                    "last_price": float(row.get("lastPrice") or 0) or None,
                })
        return contracts

    def fetch_index_series(self, index_ticker: str, timeframe: str) -> list[OhlcBar]:
        return _fetch_history(index_ticker, timeframe)

    def fetch_screening_snapshot(self, ticker: str) -> ScreeningSnapshot:
        info = yf.Ticker(ticker).info
        debt_to_equity = _normalize_debt_to_equity(info.get("debtToEquity"))
        dividend_yield = _normalize_dividend_yield(info.get("dividendYield"))
        return {
            "sector": info.get("sector"),
            "market_cap": info.get("marketCap"),
            "ps_ratio": info.get("priceToSalesTrailing12Months"),
            "revenue_growth": info.get("revenueGrowth"),
            "fifty_two_week_high": info.get("fiftyTwoWeekHigh"),
            "current_price": info.get("currentPrice") or info.get("regularMarketPrice"),
            "target_mean_price": info.get("targetMeanPrice"),
            "peg_ratio": info.get("trailingPegRatio"),
            "pe_ratio": info.get("trailingPE"),
            "debt_to_equity": debt_to_equity,
            "dividend_yield": dividend_yield,
            "payout_ratio": info.get("payoutRatio"),
            "target_high_price": info.get("targetHighPrice"),
            "target_low_price": info.get("targetLowPrice"),
            "industry": info.get("industry"),
        }
