"""FakeDataSource — dati fissi e deterministici per i test di fundamentals.py/
charts.py/report.py. Mai usato in produzione (server.py usa sempre
YFinanceDataSource)."""

from . import DataSource, Fundamentals, OhlcBar, OptionContract, ScreeningSnapshot


def _bar(date: str, price: float) -> OhlcBar:
    """Barra OHLC fittizia: stesso prezzo su O/H/L/C, volume fisso — basta a
    esercitare il rendering dei grafici senza inventare candele realistiche
    per ogni test."""
    return {"date": date, "open": price, "high": price + 1, "low": price - 1, "close": price, "volume": 1_000_000}


def _synthetic_daily_series(days: int = 300) -> list[OhlcBar]:
    """Serie sintetica di `days` barre giornaliere, trend rialzista con
    oscillazione -- basta a rendere calcolabili SMA200/RSI/MACD/Bollinger/
    volume ratio nei test di citadel.py (screener 'citadel', Docs/
    superpowers/specs/2026-08-26-screener-citadel-design.md) senza
    inventare centinaia di righe a mano. Date sequenziali di calendario
    (non un vero calendario di borsa -- irrilevante per la matematica
    degli indicatori, serve solo l'ordine cronologico)."""
    import math
    from datetime import date, timedelta

    bars: list[OhlcBar] = []
    start = date(2024, 1, 1)
    for i in range(days):
        price = 100.0 + i * 0.25 + 5.0 * math.sin(i / 8.0)
        day = start + timedelta(days=i)
        volume = 1_000_000 + int(200_000 * math.sin(i / 5.0))
        bars.append({
            "date": day.isoformat(),
            "open": price - 0.5,
            "high": price + 1.0,
            "low": price - 1.0,
            "close": price,
            "volume": float(volume),
        })
    return bars


class FakeDataSource(DataSource):
    def __init__(self):
        self.fundamentals_by_ticker: dict[str, Fundamentals] = {
            "FAKE": {
                "ticker": "FAKE",
                "name": "Fake Corp",
                "sector": "Technology",
                "industry": "Software",
                "market_cap": 1_000_000_000.0,
                "pe_ratio": 20.0,
                "current_price": 110.0,
                "revenue_by_period": [("2024-Q4", 100.0), ("2025-Q4", 120.0)],
                "earnings_by_period": [("2024-Q4", 10.0), ("2025-Q4", 15.0)],
            },
            # Peer fittizi per il settore "Technology" — peers.peers_for_sector
            # mappa questo settore sui ticker reali AAPL/MSFT/GOOGL/NVDA (vedi
            # peers.py). report.py chiama la mappa REALE (non uno stub), quindi
            # questa fixture deve saper rispondere per quei 4 ticker così come
            # farebbe YFinanceDataSource in produzione. Valori arbitrari, solo
            # per esercitare peer_comparison_table end-to-end.
            "AAPL": {
                "ticker": "AAPL",
                "name": "Fake Apple",
                "sector": "Technology",
                "industry": "Consumer Electronics",
                "market_cap": 2_500_000_000.0,
                "pe_ratio": 28.0,
                "current_price": 190.0,
                "revenue_by_period": [],
                "earnings_by_period": [],
            },
            "MSFT": {
                "ticker": "MSFT",
                "name": "Fake Microsoft",
                "sector": "Technology",
                "industry": "Software",
                "market_cap": 2_200_000_000.0,
                "pe_ratio": 32.0,
                "current_price": 420.0,
                "revenue_by_period": [],
                "earnings_by_period": [],
            },
            "GOOGL": {
                "ticker": "GOOGL",
                "name": "Fake Alphabet",
                "sector": "Technology",
                "industry": "Internet Content & Information",
                "market_cap": 1_800_000_000.0,
                "pe_ratio": 24.0,
                "current_price": 165.0,
                "revenue_by_period": [],
                "earnings_by_period": [],
            },
            "NVDA": {
                "ticker": "NVDA",
                "name": "Fake Nvidia",
                "sector": "Technology",
                "industry": "Semiconductors",
                "market_cap": 3_000_000_000.0,
                "pe_ratio": 45.0,
                "current_price": 130.0,
                "revenue_by_period": [],
                "earnings_by_period": [],
            },
        }
        self.ohlc_by_ticker_timeframe: dict[tuple[str, str], list[OhlcBar]] = {
            ("FAKE", "M"): [_bar("2025-01", 100.0), _bar("2025-02", 110.0)],
            ("FAKE", "W"): [_bar("2025-07-01", 105.0), _bar("2025-07-08", 108.0)],
            ("FAKE", "D"): _synthetic_daily_series(),
            ("FAKE", "H4"): [_bar("2025-07-21 12:00", 110.5), _bar("2025-07-21 16:00", 111.0)],
            ("FAKE", "H1"): [_bar("2025-07-21 15:00", 110.8), _bar("2025-07-21 16:00", 111.0)],
        }
        self.options_by_ticker: dict[str, list[OptionContract]] = {
            "FAKE": [
                {"strike": 100.0, "expiry": "2025-08-15", "iv": 0.35, "option_type": "call", "bid": 11.0, "ask": 11.5, "last_price": 11.2},
                {"strike": 100.0, "expiry": "2025-08-15", "iv": 0.40, "option_type": "put", "bid": 1.0, "ask": 1.3, "last_price": 1.1},
                {"strike": 110.0, "expiry": "2025-08-15", "iv": 0.33, "option_type": "call", "bid": 3.0, "ask": 3.3, "last_price": 3.1},
                {"strike": 110.0, "expiry": "2025-08-15", "iv": 0.38, "option_type": "put", "bid": 3.0, "ask": 3.4, "last_price": 3.2},
            ]
        }
        self.index_series: dict[str, list[OhlcBar]] = {"D": [_bar("2025-07-20", 5000.0), _bar("2025-07-21", 5010.0)]}
        self.screening_by_ticker: dict[str, ScreeningSnapshot] = {
            "FAKE": {
                "sector": "Technology",
                "market_cap": 1_000_000_000.0,
                "ps_ratio": 2.5,
                "revenue_growth": 0.15,
                "fifty_two_week_high": 140.0,
                "current_price": 110.0,
                "target_mean_price": 130.0,
                "peg_ratio": 1.2,
                "pe_ratio": 22.0,
                "debt_to_equity": 0.6,
                "dividend_yield": 0.012,
                "payout_ratio": 0.25,
                "target_high_price": 150.0,
                "target_low_price": 100.0,
                # Em dash deliberato: esercita la normalizzazione dash di
                # industry_qualifies() (jensen_huang.py) end-to-end.
                "industry": "Software—Infrastructure",
            }
        }

    def fetch_fundamentals(self, ticker: str) -> Fundamentals:
        return self.fundamentals_by_ticker[ticker]

    def fetch_ohlc(self, ticker: str, timeframe: str) -> list[OhlcBar]:
        return self.ohlc_by_ticker_timeframe[(ticker, timeframe)]

    def fetch_options(self, ticker: str) -> list[OptionContract]:
        return self.options_by_ticker[ticker]

    def fetch_index_series(self, index_ticker: str, timeframe: str) -> list[OhlcBar]:
        return self.index_series[timeframe]

    def fetch_screening_snapshot(self, ticker: str) -> ScreeningSnapshot:
        return self.screening_by_ticker[ticker]
