"""Interfaccia comune per le fonti dati finanziari — yfinance è la prima
implementazione, non l'unica per sempre (richiesta esplicita in brainstorming:
l'architettura non deve assumere un'unica fonte). Aggiungere una fonte futura
è un nuovo file in questo package che implementa `DataSource`, senza toccare
fundamentals.py/charts.py/report.py (dipendono solo da questa interfaccia).
"""

from abc import ABC, abstractmethod
from typing import TypedDict

TIMEFRAMES = ("M", "W", "D", "H4", "H1")


class Fundamentals(TypedDict):
    ticker: str
    name: str
    sector: str
    industry: str
    market_cap: float | None
    pe_ratio: float | None
    # Refinement 2026-07-23: serve sia per la riga "Prezzo attuale" del
    # Fondamentale sia per centrare l'option chain sullo strike ATM
    # (report.py/option_chain.py).
    current_price: float | None
    # Ogni tupla è (periodo, valore) — es. ("2025-Q4", 123.4e9). Ordine
    # cronologico, più recente per ultimo.
    revenue_by_period: list[tuple[str, float]]
    earnings_by_period: list[tuple[str, float]]


class OhlcBar(TypedDict):
    date: str
    open: float
    high: float
    low: float
    close: float
    volume: float


class OptionContract(TypedDict):
    strike: float
    expiry: str
    iv: float | None
    option_type: str  # "call" | "put"
    # Refinement 2026-07-23 — servono per la tabella call/put affiancate
    # (option_chain.py): prima non c'erano, l'unica colonna oltre a
    # strike/scadenza era l'IV.
    bid: float | None
    ask: float | None
    last_price: float | None


class ScreeningSnapshot(TypedDict):
    """Snapshot compatto per lo screening (`screen_stocks`) — una sola
    chiamata `.info` yfinance copre tutti i campi. Tutti nullable: un campo
    mancante nel provider diventa None, mai un'eccezione (la gestione
    del dato mancante è a valle, in screening.py)."""
    sector: str | None
    market_cap: float | None
    ps_ratio: float | None
    revenue_growth: float | None
    fifty_two_week_high: float | None
    current_price: float | None
    target_mean_price: float | None
    peg_ratio: float | None
    # Estensione 2026-08-11 (screener 'goldman-sachs') — additiva, consumer-
    # usage ignora questi campi (legge solo le chiavi che gli servono).
    pe_ratio: float | None
    debt_to_equity: float | None
    dividend_yield: float | None
    payout_ratio: float | None
    target_high_price: float | None
    target_low_price: float | None
    # Estensione 2026-08-17 (screener 'jensen-huang') — additiva, gli altri
    # screener ignorano questo campo. Granularità più fine di `sector`
    # ("Semiconductors" è una industry dentro il sector "Technology").
    industry: str | None


class DataSource(ABC):
    """Contratto che ogni fonte dati deve implementare. `fundamentals.py`/
    `charts.py`/`report.py` dipendono SOLO da questa interfaccia, mai da
    yfinance direttamente."""

    @abstractmethod
    def fetch_fundamentals(self, ticker: str) -> Fundamentals: ...

    @abstractmethod
    def fetch_ohlc(self, ticker: str, timeframe: str) -> list[OhlcBar]:
        """`timeframe` è uno di TIMEFRAMES ('M','W','D','H4','H1')."""
        ...

    @abstractmethod
    def fetch_options(self, ticker: str) -> list[OptionContract]: ...

    @abstractmethod
    def fetch_index_series(self, index_ticker: str, timeframe: str) -> list[OhlcBar]:
        """Serie storica di un indice (es. '^GSPC' per S&P 500), stesso
        formato di fetch_ohlc — per il confronto titolo-vs-indice."""
        ...

    @abstractmethod
    def fetch_screening_snapshot(self, ticker: str) -> ScreeningSnapshot:
        """Fondamentali compatti per lo screening — vedi Docs/superpowers/
        specs/2026-07-26-financial-markets-screen-stocks-design.md §5."""
        ...

    def test_connection(self) -> tuple[bool, str]:
        """Verifica di raggiungibilità, leggera — non un fetch completo.
        Default: YFinance non richiede una connessione persistente, sempre ok
        (una vera verifica di rete qui duplicherebbe fetch_fundamentals senza
        motivo — yfinance non ha un concetto di "sessione" da testare)."""
        return True, "YFinance non richiede connessione — pronto."

    def unavailable_fields(self) -> set[str]:
        """Campi di Fundamentals/ScreeningSnapshot che questa fonte non può
        STRUTTURALMENTE fornire (mai per un singolo ticker — quello resta
        None silenzioso come sempre, questo è a livello di fonte intera).
        Metodo CONCRETO (non astratto): default vuoto, ereditato gratis da
        ogni fonte che non ha lacune strutturali note (YFinance,
        FakeDataSource) — solo IbkrDataSource lo sovrascrive (vedi
        ibkr_source.py). Il chiamante (report.py/screeners/__init__.py) usa
        questo insieme per mostrare un avviso VISIBILE all'utente, non per
        calcolarci sopra — decisione utente 2026-08-14, vedi
        task-5-final-brief.md."""
        return set()


# Etichette leggibili in italiano per ogni campo di Fundamentals/
# ScreeningSnapshot che una fonte può dichiarare assente via
# unavailable_fields() — usate solo da format_unavailable_warning() sotto,
# mai per la logica di fetch. Un campo senza etichetta qui ricade sul nome
# tecnico grezzo (vedi .get(f, f) sotto), non un errore.
_FIELD_LABELS = {
    "name": "nome societario",
    "sector": "settore",
    "industry": "industria",
    "market_cap": "capitalizzazione di mercato",
    "current_price": "prezzo attuale",
    "fifty_two_week_high": "massimo 52 settimane",
    "pe_ratio": "P/E",
    "ps_ratio": "P/S",
    "debt_to_equity": "debito/equity",
    "dividend_yield": "dividend yield",
    "payout_ratio": "payout ratio",
    "revenue_growth": "crescita ricavi",
    "peg_ratio": "PEG ratio",
    "revenue_by_period": "ricavi storici",
    "earnings_by_period": "utili storici",
    "target_mean_price": "prezzo obiettivo medio (analisti)",
    "target_high_price": "prezzo obiettivo massimo (analisti)",
    "target_low_price": "prezzo obiettivo minimo (analisti)",
}


def format_unavailable_warning(fields: set[str]) -> str | None:
    """Riga Markdown di avviso per i campi non disponibili dalla fonte
    attiva, o None se l'insieme è vuoto (caso comune: YFinance/
    FakeDataSource). `sorted()` rende l'ordine deterministico — un set non
    garantisce un ordine di iterazione stabile fra run diverse."""
    if not fields:
        return None
    labels = sorted(_FIELD_LABELS.get(f, f) for f in fields)
    return f"> ⚠️ Fonte dati: campi non disponibili — {', '.join(labels)}."
