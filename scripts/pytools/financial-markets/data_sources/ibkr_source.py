"""IbkrDataSource -- seconda implementazione di DataSource, via TWS/IB
Gateway (stesso protocollo socket, la "TWS API" classica -- ib_async parla
a entrambi identico, cambia solo la porta). Wrapper sottile su ib_async
2.1.0: NON pesantemente unit-testato riga per riga con mock (mockare il
comportamento event-driven interno della libreria costa piu' di quanto
renda) -- verificato principalmente con smoke test dal vivo (Task 13),
stesso principio di YFinanceDataSource. ECCEZIONE deliberata: un piccolo
fake sostituisce IB() per gli invarianti che non possono dipendere solo
dallo smoke test -- readonly=True/credenziali passate a connect() (vedi
test_ibkr_source.py::test_connect_always_passes_readonly_true_...), la
guardia su contract.conId per contratti non risolvibili (vedi
_fetch_bars_or_raise_async/_require_qualified), e la guardia analoga sugli
strike di una option chain (vedi fetch_options/_fetch_options_or_raise_async) --
la cui assenza in tutti e tre i casi sarebbe una regressione di
correttezza/sicurezza silenziosa, non solo funzionale.

readonly=True SEMPRE, hardcoded -- vedi Global Constraints del piano. Non e'
configurabile: questo progetto e' sola lettura per costruzione, non per
convenzione.

Nota empirica (fix post-review, verificato dal vivo con IB Gateway): per
un ticker/indice non risolvibile, ib_async 2.1.0 NON solleva un'eccezione
da qualifyContracts()/reqHistoricalData() -- lascia contract.conId a 0 e
ritorna una lista vuota, entrambi silenziosamente. Un semplice
try/except attorno alla chiamata NON intercetta questo caso: serve la
guardia esplicita su conId (vedi _require_qualified). Stesso principio,
conseguenza piu' grave, per gli Option di una option chain (Task 6): un
Option non qualificato passato a reqTickers solleva un ValueError NON
gestito da ib_async stesso (Contract.__hash__ richiede un conId) -- non un
fallimento silenzioso ma un crash, quindi il filtro pre-reqTickers in
fetch_options e' obbligatorio, non solo difensivo.

Task 14 -- loop asyncio dedicato (fix "This event loop is already
running"): l'API sincrona di ib_async (IB.connect(), IB.qualifyContracts(),
...) e' letteralmente un wrapper su `self._run(self.xxxAsync(...))`
(verificato per introspezione sul sorgente installato) -- possiede/esegue
il proprio loop asyncio internamente. Chiamata da dentro il processo
FastMCP del server MCP, che serve gia' le richieste sul proprio loop
async, i due collidono (causa confermata: GitHub PrefectHQ/fastmcp#1751,
stesso stack). Il fix: OGNI metodo pubblico di IbkrDataSource sottomette
UNA sola coroutine end-to-end (connect -> lavoro -> disconnect, tutta
`await`ata con le varianti *Async di ib_async) al loop dedicato in
ibkr_loop.py, via run_on_ibkr_loop() -- mai una chiamata sincrona di
ib_async ne' un await diretto nel loop del chiamante. Vincolo strutturale
(non aggirabile): un oggetto IB() e' legato al loop su cui e' stato
creato/connesso, quindi non si puo' creare/connettere `ib` su una
sottomissione e poi richiamare metodi su di lui da un'altra."""

import logging

from ib_async import IB, Index, Stock

from . import DataSource, Fundamentals, OhlcBar, OptionContract, ScreeningSnapshot
from .ibkr_loop import run_on_ibkr_loop

# Trovato nella verifica dal vivo del Task 13 (2026-08-14, IB Gateway acceso):
# ib_async logga a livello INFO, via il logger padre "ib_async" (ib.py/
# client.py ne prendono uno figlio ciascuno, tutti propagano qui sopra --
# verificato per introspezione sul sorgente installato), l'INTERO sync
# dell'account ad ogni connect() -- incluso quello "leggero" di
# test_connection() -- posizioni reali, portfolio, esecuzioni, commissioni,
# numero di conto in chiaro. Se in futuro l'orchestrator/servizio redirige
# stdout/stderr su file (backlog "Windows installer + servizi"), quei dati
# finirebbero scritti su disco a ogni click di "Test connessione" o ogni
# `/markets`. WARNING (non ERROR): i messaggi "Warning NNNN" di IB (stato
# dei data farm, es. "Connessione con il Server dei dati di mercato è OK")
# e i veri errori di connessione restano visibili per il debug -- solo lo
# spam INFO (connect/position/portfolio/execDetails) viene tagliato.
logging.getLogger("ib_async").setLevel(logging.WARNING)

# durationStr/barSizeSetting di reqHistoricalData per ciascun timeframe di
# TIMEFRAMES ('M','W','D','H4','H1') -- valori validi confermati dalla
# documentazione IB (barSizeSetting: '1 month'/'1 week'/'1 day'/'4 hours'/
# '1 hour'). durationStr sceglie quanto storico chiedere per timeframe --
# stessa filosofia di _TIMEFRAME_TO_YFINANCE in yfinance_source.py.
_TIMEFRAME_TO_BAR_SIZE = {
    "M": ("1 month", "5 Y"),
    "W": ("1 week", "2 Y"),
    "D": ("1 day", "6 M"),
    "H4": ("4 hours", "60 D"),
    "H1": ("1 hour", "60 D"),
}

# Traduzione ticker stile Yahoo (usati dai chiamanti esistenti, es.
# report.py) -> contratto Index IB (symbol, exchange). Named deliverable:
# un indice non qui dentro fallisce leggibile (vedi fetch_index_series),
# mai un risultato vuoto silenzioso.
_INDEX_TICKER_MAP = {
    "^GSPC": ("SPX", "CBOE"),
    "^DJI": ("INDU", "CME"),
    "^IXIC": ("COMP", "NASDAQ"),
}


class IbkrConnectionError(Exception):
    """Sollevata quando la connessione a TWS/Gateway fallisce -- messaggio
    leggibile per l'utente, mai uno stack trace grezzo di ib_async."""


def _require_qualified(contract, label: str) -> None:
    """Guardia esplicita post-qualifyContracts -- vedi nota empirica in
    cima al modulo: ib_async 2.1.0 non solleva per un simbolo non
    risolvibile, lascia silenziosamente `contract.conId` a 0. Estratta come
    funzione a se' stante (usata sia da _fetch_bars_or_raise_async sia da
    _fetch_options_or_raise_async) per non duplicare il messaggio d'errore
    condiviso. Funzione PURA (nessun I/O) -- resta sincrona anche dopo il
    Task 14, non ha bisogno di essere una coroutine."""
    if not contract.conId:
        raise IbkrConnectionError(
            f"Impossibile risolvere il contratto per {label!r} su TWS/Gateway — "
            f"simbolo sconosciuto o non disponibile per l'exchange configurato."
        )


def _ticker_to_option_contract(t, expiry: str) -> OptionContract:
    """Converte un Ticker di ib_async (risultato di reqTickers) nel formato
    comune OptionContract. Funzione pura (nessuna chiamata IB) -- estratta
    a parte per essere testabile in isolamento senza un fake di IB, stessa
    filosofia delle funzioni _normalize_* in yfinance_source.py.

    bid/ask/last: IB usa -1 per "nessuna quotazione disponibile" (osservato
    dal vivo, spike Task 6, specialmente fuori dagli orari di mercato) e
    last puo' essere NaN -- in entrambi i casi diventano None, mai il
    valore grezzo. iv: letto da modelGreeks (i "computed greeks" di IB),
    None se IB non li ha ancora calcolati per questo tick (comune con dati
    event-driven asincroni, vedi nota nel brief) o se il valore e' 0."""
    return {
        "strike": float(t.contract.strike),
        "expiry": expiry,
        "iv": float(t.modelGreeks.impliedVol) if t.modelGreeks and t.modelGreeks.impliedVol else None,
        "option_type": "call" if t.contract.right == "C" else "put",
        "bid": float(t.bid) if t.bid and t.bid > 0 else None,
        "ask": float(t.ask) if t.ask and t.ask > 0 else None,
        "last_price": float(t.last) if t.last and t.last > 0 else None,
    }


class IbkrDataSource(DataSource):
    def __init__(self, port: int, client_id: int, host: str = "127.0.0.1", timeout: float = 8.0):
        self.host = host
        self.port = port
        self.client_id = client_id
        self.timeout = timeout

    async def _connect_async(self) -> IB:
        """Coroutine di connessione condivisa -- ogni metodo pubblico
        end-to-end la awaita UNA volta, come primo passo della propria
        singola coroutine sottomessa al loop dedicato (mai un round-trip
        separato: vedi nota Task 14 in cima al modulo)."""
        ib = IB()
        try:
            await ib.connectAsync(self.host, self.port, clientId=self.client_id, timeout=self.timeout, readonly=True)
        except Exception as e:
            raise IbkrConnectionError(
                f"Impossibile connettersi a TWS/Gateway su {self.host}:{self.port} — "
                f"verifica che sia avviato e che l'API sia abilitata (File → Global "
                f"Configuration → API → Settings → \"Enable ActiveX and Socket Clients\"). "
                f"Dettaglio: {e}"
            ) from e
        return ib

    async def _fetch_bars_or_raise_async(self, ib: IB, contract, label: str, duration: str, bar_size: str) -> list[OhlcBar]:
        """Esegue qualifyContractsAsync + reqHistoricalDataAsync e converte
        il risultato nel formato comune list[OhlcBar] -- condiviso da
        _fetch_ohlc_async/_fetch_index_series_async (stesso identico corpo,
        cambia solo il tipo di contratto costruito dal chiamante).

        Due meccanismi di errore distinti, verificati dal vivo con IB
        Gateway (non solo per costruzione):

        1. Guardia esplicita su `contract.conId` dopo qualifyContractsAsync.
           Per un simbolo sconosciuto a IB, ib_async NON solleva
           un'eccezione: lascia silenziosamente conId a 0 (contratto non
           risolto) e reqHistoricalDataAsync ritorna poi una lista vuota,
           anche lei senza sollevare -- un fallimento silenzioso puro, mai
           visibile a un try/except. Verificato empiricamente: AAPL/SPX
           qualificano a un conId non-zero, un ticker/indice inventato
           resta a 0 in entrambi i casi (Stock e Index).
        2. try/except attorno all'intera chiamata per gli errori che
           *invece* ib_async solleva davvero (violazioni di pacing,
           durationStr/barSizeSetting non validi, timeout di rete).

        In entrambi i casi il risultato per il chiamante è lo stesso:
        IbkrConnectionError leggibile che nomina `label` (ticker o indice
        richiesto), mai uno stack trace grezzo e mai un risultato vuoto
        silenzioso."""
        try:
            await ib.qualifyContractsAsync(contract)
            _require_qualified(contract, label)
            bars = await ib.reqHistoricalDataAsync(
                contract, endDateTime="", durationStr=duration, barSizeSetting=bar_size,
                whatToShow="TRADES", useRTH=True,
            )
            return [
                {
                    "date": str(bar.date), "open": float(bar.open), "high": float(bar.high),
                    "low": float(bar.low), "close": float(bar.close), "volume": float(bar.volume),
                }
                for bar in bars
            ]
        except IbkrConnectionError:
            raise
        except Exception as e:
            raise IbkrConnectionError(
                f"Impossibile recuperare i dati storici per {label!r} da TWS/Gateway — "
                f"verifica che il simbolo sia valido e risolvibile da IB (contratto "
                f"sconosciuto o storico non disponibile). Dettaglio: {e}"
            ) from e

    async def _fetch_ohlc_async(self, ticker: str, timeframe: str) -> list[OhlcBar]:
        """Coroutine end-to-end (connect -> lavoro -> disconnect) sottomessa
        UNA sola volta al loop dedicato da fetch_ohlc(). `timeframe' e' gia'
        stato validato dal chiamante sincrono (vedi fetch_ohlc) -- qui si
        assume valido."""
        bar_size, duration = _TIMEFRAME_TO_BAR_SIZE[timeframe]
        ib = await self._connect_async()
        try:
            contract = Stock(ticker, "SMART", "USD")
            return await self._fetch_bars_or_raise_async(ib, contract, ticker, duration, bar_size)
        finally:
            ib.disconnect()

    def fetch_ohlc(self, ticker: str, timeframe: str) -> list[OhlcBar]:
        # Validazione pura PRIMA di run_on_ibkr_loop: un timeframe
        # sconosciuto non deve costare una submission al loop dedicato.
        if timeframe not in _TIMEFRAME_TO_BAR_SIZE:
            raise ValueError(f"timeframe sconosciuto: {timeframe!r}")
        return run_on_ibkr_loop(lambda: self._fetch_ohlc_async(ticker, timeframe))

    async def _fetch_index_series_async(self, index_ticker: str, timeframe: str) -> list[OhlcBar]:
        """Identica a _fetch_ohlc_async, cambia solo il tipo di contratto
        (Index invece di Stock) -- index_ticker/timeframe gia' validati dal
        chiamante sincrono (vedi fetch_index_series)."""
        symbol, exchange = _INDEX_TICKER_MAP[index_ticker]
        bar_size, duration = _TIMEFRAME_TO_BAR_SIZE[timeframe]
        ib = await self._connect_async()
        try:
            contract = Index(symbol, exchange, "USD")
            return await self._fetch_bars_or_raise_async(ib, contract, index_ticker, duration, bar_size)
        finally:
            ib.disconnect()

    def fetch_index_series(self, index_ticker: str, timeframe: str) -> list[OhlcBar]:
        if index_ticker not in _INDEX_TICKER_MAP:
            raise ValueError(
                f"indice non mappato per IB: {index_ticker!r} — aggiungi una entry a "
                f"_INDEX_TICKER_MAP in ibkr_source.py"
            )
        if timeframe not in _TIMEFRAME_TO_BAR_SIZE:
            raise ValueError(f"timeframe sconosciuto: {timeframe!r}")
        return run_on_ibkr_loop(lambda: self._fetch_index_series_async(index_ticker, timeframe))

    # fetch_fundamentals/fetch_screening_snapshot: Task 5 CHIUSO in via
    # definitiva (2026-08-14, vedi task-5-final-brief.md) -- errore 10358
    # "dati fondamentali non consentiti" (task-5-report.md) e' un entitlement
    # gap (nessuna sottoscrizione Reuters Fundamentals sull'account), non un
    # bug di codice. Decisione utente: "riduci scope, resta gratis" invece di
    # sottoscrivere -- questi campi non saranno MAI serviti da questa fonte,
    # per nessun ticker. Ritorno STATICO immediato, zero I/O: nessuna
    # chiamata a IB()/ib_async, quindi nessuna dipendenza da connect_async,
    # run_on_ibkr_loop o dalla raggiungibilita' di TWS/Gateway -- il motivo
    # dell'assenza non e' "non risponde", e' "non lo forniamo". Vedi
    # unavailable_fields() sotto per l'insieme completo dei campi assenti,
    # usato da report.py/screeners per mostrare un avviso VISIBILE invece di
    # nascondere silenziosamente la lacuna.
    def fetch_fundamentals(self, ticker: str) -> Fundamentals:
        return {
            "ticker": ticker,
            "name": "N/D",
            "sector": "N/D",
            "industry": "N/D",
            "market_cap": None,
            "pe_ratio": None,
            "current_price": None,
            "revenue_by_period": [],
            "earnings_by_period": [],
        }

    def fetch_options(self, ticker: str) -> list[OptionContract]:
        return run_on_ibkr_loop(lambda: self._fetch_options_async(ticker))

    async def _fetch_options_async(self, ticker: str) -> list[OptionContract]:
        """Coroutine end-to-end (connect -> lavoro -> disconnect) sottomessa
        UNA sola volta al loop dedicato da fetch_options()."""
        ib = await self._connect_async()
        try:
            return await self._fetch_options_or_raise_async(ib, ticker)
        finally:
            ib.disconnect()

    async def _fetch_options_or_raise_async(self, ib: IB, ticker: str) -> list[OptionContract]:
        """Solo la scadenza piu' vicina -- stessa scelta di
        YFinanceDataSource (yfinance_source.py, refinement 2026-07-23).

        Guardia critica (verificata dal vivo, spike Task 6): reqSecDefOptParams
        ritorna l'unione degli strike su TUTTE le scadenze del titolo, non
        solo quella piu' vicina selezionata qui sotto -- su AAPL, 86/254
        combinazioni strike/right non si sono qualificate per la scadenza
        piu' vicina (qualifyContracts lascia silenziosamente conId=0, senza
        sollevare -- stessa "guardia esplicita" gia' nota per fetch_ohlc,
        vedi nota empirica in cima al modulo). Passare un contratto non
        qualificato a reqTickers solleva un ValueError NON gestito piu' in
        basso in ib_async (Contract.__hash__ richiede un conId) -- i non
        risolti vanno filtrati PRIMA di reqTickers, mai dopo."""
        from ib_async import Option

        try:
            underlying = Stock(ticker, "SMART", "USD")
            await ib.qualifyContractsAsync(underlying)
            _require_qualified(underlying, ticker)

            chains = await ib.reqSecDefOptParamsAsync(underlying.symbol, "", underlying.secType, underlying.conId)
            chain = next((c for c in chains if c.exchange == "SMART"), chains[0] if chains else None)
            if chain is None or not chain.expirations:
                return []

            # IB esprime le scadenze in 'YYYYMMDD' (formato nativo di
            # reqSecDefOptParams) -- serve cosi' per costruire i contratti
            # Option (lastTradeDateOrContractMonth), ma YFinanceDataSource
            # (yfinance_source.py) e i consumer esistenti (report.py,
            # option_chain.py) si aspettano 'YYYY-MM-DD' nello stesso campo
            # OptionContract["expiry"] -- convertito solo per il risultato
            # ritornato al chiamante, mai per le chiamate a ib_async.
            expiry_raw = sorted(chain.expirations)[0]
            expiry_iso = f"{expiry_raw[:4]}-{expiry_raw[4:6]}-{expiry_raw[6:]}"
            strikes = sorted(chain.strikes)
            contracts_to_price = [
                Option(ticker, expiry_raw, strike, right, "SMART")
                for strike in strikes
                for right in ("C", "P")
            ]
            await ib.qualifyContractsAsync(*contracts_to_price)
            resolved = [c for c in contracts_to_price if c.conId]
            if not resolved:
                return []

            tickers = await ib.reqTickersAsync(*resolved)
            return [_ticker_to_option_contract(t, expiry_iso) for t in tickers]
        except IbkrConnectionError:
            raise
        except Exception as e:
            raise IbkrConnectionError(
                f"Impossibile recuperare la option chain per {ticker!r} da TWS/Gateway — "
                f"verifica che il simbolo sia valido e risolvibile da IB. Dettaglio: {e}"
            ) from e

    def fetch_screening_snapshot(self, ticker: str) -> ScreeningSnapshot:
        # Stesso principio di fetch_fundamentals sopra: ritorno statico,
        # zero I/O. Anche i 3 campi in teoria ottenibili da un percorso IB
        # diverso (reqContractDetails/reqMktData, nessuna sottoscrizione
        # fundamentals richiesta -- market_cap/current_price/
        # fifty_two_week_high) restano None qui: scelta di scope
        # DELIBERATA, non un oversight -- vedi task-5-final-brief.md
        # ("Scope deliberatamente ristretto"). L'utente ha scelto "riduci
        # scope", non "massimizza copertura gratuita", e quel percorso
        # userebbe codice ib_async mai verificato dal vivo in questa
        # sessione (IB Gateway non raggiungibile).
        return {
            "sector": None,
            "market_cap": None,
            "ps_ratio": None,
            "revenue_growth": None,
            "fifty_two_week_high": None,
            "current_price": None,
            "target_mean_price": None,
            "peg_ratio": None,
            "pe_ratio": None,
            "debt_to_equity": None,
            "dividend_yield": None,
            "payout_ratio": None,
            "target_high_price": None,
            "target_low_price": None,
            "industry": None,
        }

    def unavailable_fields(self) -> set[str]:
        """Insieme FINALE, non provvisorio -- vedi task-5-final-brief.md per
        la decisione di scope (Reuters Fundamentals non sottoscritto sul
        conto, entitlement gap confermato -- task-5-report.md -- + i 3 campi
        market_cap/current_price/fifty_two_week_high, in teoria ottenibili
        da un percorso IB diverso -- reqContractDetails/reqMktData --
        deliberatamente NON implementati qui: fuori scope per scelta
        dell'utente, non un limite tecnico). Usato da report.py/
        screeners/__init__.py per mostrare un avviso visibile invece di far
        apparire "N/D"/None come se fossero un dato mancante per QUESTO
        ticker (com'è per YFinance) anziché per l'intera fonte."""
        return {
            "name", "sector", "industry", "market_cap", "current_price",
            "fifty_two_week_high", "pe_ratio", "ps_ratio", "debt_to_equity",
            "dividend_yield", "payout_ratio", "revenue_growth", "peg_ratio",
            "revenue_by_period", "earnings_by_period", "target_mean_price",
            "target_high_price", "target_low_price",
        }

    async def _test_connection_async(self) -> tuple[bool, str]:
        """Verifica di raggiungibilità a TWS/IB Gateway — leggera, non un
        fetch completo. Connette, verifica isConnected(), disconnette --
        isConnected() resta sincrona: legge solo uno stato locale gia'
        noto al client, non fa I/O di rete (verificato per introspezione
        sul sorgente installato, Task 14)."""
        try:
            ib = await self._connect_async()
        except IbkrConnectionError as e:
            return False, str(e)
        try:
            connected = ib.isConnected()
        finally:
            ib.disconnect()
        if connected:
            return True, f"Connesso a {self.host}:{self.port} (clientId={self.client_id})."
        return False, f"Connessione a {self.host}:{self.port} non confermata."

    def test_connection(self) -> tuple[bool, str]:
        # Timeout esplicito piu' basso del default 60s (Fix F, review-fix-
        # wave 2026-08-14): il client MCP SCOPED usato dal bottone "Test
        # connessione" (ws.rs, test_market_data_source_now()) passa
        # call_timeout_secs=30 -- un tokio::time::timeout che UCCIDE il
        # processo Python se non risponde in tempo. Con il default 60s, una
        # connessione lenta farebbe scattare PRIMA il kill lato Rust (con
        # un messaggio generico) che il timeout Python qui (con un
        # messaggio piu' specifico) -- 20s resta comodamente sotto il
        # tetto Rust di 30s, e ben sopra self.timeout=8.0 (il timeout di
        # connessione IB stesso). Gli altri metodi pubblici (fetch_ohlc/
        # fetch_index_series/fetch_options) girano nel canale /markets, che
        # usa call_timeout_secs=300 -- restano al default 60s, NON toccati.
        return run_on_ibkr_loop(self._test_connection_async, timeout=20.0)
