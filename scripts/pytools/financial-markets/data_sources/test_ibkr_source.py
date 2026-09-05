import asyncio
import logging
import re

import pytest

import data_sources.ibkr_source as ibkr_source
from data_sources.ibkr_source import IbkrDataSource, IbkrConnectionError, _TIMEFRAME_TO_BAR_SIZE, _INDEX_TICKER_MAP


def test_importing_module_silences_ib_async_info_logging():
    # Trovato nella verifica dal vivo del Task 13 (2026-08-14): senza
    # questo, ogni connect() -- anche quello "leggero" di test_connection()
    # -- fa stampare a ib_async l'intero sync account (posizioni, portfolio,
    # esecuzioni, numero di conto) a livello INFO. WARNING+ resta visibile
    # per il debug (stato dei data farm, veri errori di connessione).
    assert logging.getLogger("ib_async").level == logging.WARNING


def test_constructor_stores_port_and_client_id():
    source = IbkrDataSource(port=4001, client_id=731)
    assert source.port == 4001
    assert source.client_id == 731
    assert source.host == "127.0.0.1"


def test_timeframe_to_bar_size_covers_all_timeframes():
    from data_sources import TIMEFRAMES
    for tf in TIMEFRAMES:
        assert tf in _TIMEFRAME_TO_BAR_SIZE, f"timeframe {tf} non mappato"


def test_index_ticker_map_covers_common_yahoo_style_tickers():
    # Traduzione ^GSPC ecc. (stile Yahoo, usato dai chiamanti esistenti,
    # es. report.py::index_comparison) -> contratto Index IB. Named
    # deliverable esplicito (vedi consiglio advisor in brainstorming): un
    # indice non mappato deve fallire in modo leggibile, non silenziosamente.
    assert "^GSPC" in _INDEX_TICKER_MAP
    assert "^DJI" in _INDEX_TICKER_MAP
    assert "^IXIC" in _INDEX_TICKER_MAP


def test_fetch_ohlc_raises_readable_error_on_unknown_timeframe():
    # Validazione pura, nel metodo PUBBLICO sincrono, PRIMA di
    # run_on_ibkr_loop (Task 14) -- un ValueError per un timeframe
    # sbagliato non deve costare una submission al loop dedicato, quindi
    # questo test chiama ancora il metodo pubblico direttamente (nessun
    # thread viene avviato: la ValueError viene sollevata prima).
    source = IbkrDataSource(port=4001, client_id=731)
    with pytest.raises(ValueError, match="timeframe"):
        source.fetch_ohlc("AAPL", "BOGUS")


def test_fetch_index_series_raises_readable_error_on_unmapped_index():
    source = IbkrDataSource(port=4001, client_id=731)
    with pytest.raises(ValueError, match="non mappato"):
        source.fetch_index_series("^BOGUS", "D")


class _RecordingFakeIB:
    """Fake sostitutivo di ib_async.IB -- registra gli argomenti passati a
    connectAsync() senza aprire alcuna connessione di rete reale. Il
    monkeypatch sostituisce SOLO il costruttore `IB` nel namespace del
    modulo ibkr_source, non il comportamento event-driven interno della
    libreria -- resta nella convenzione del progetto "niente mock pesante
    di ib_async" (vedi docstring in cima a ibkr_source.py).

    Task 14: espone connectAsync() (non piu' connect()) -- il nuovo codice
    di ibkr_source.py chiama solo le varianti *Async, mai le sincrone."""

    def __init__(self):
        self.connect_kwargs = None

    async def connectAsync(self, host, port, clientId, timeout, readonly):
        self.connect_kwargs = {
            "host": host, "port": port, "clientId": clientId,
            "timeout": timeout, "readonly": readonly,
        }

    def disconnect(self):
        pass


def test_connect_always_passes_readonly_true_with_correct_host_port_client_id(monkeypatch):
    # Invariante piu' critico del progetto: readonly=True e le credenziali
    # di connessione corrette sono cio' che rende sicura la connessione a
    # un vero conto di trading live. Prima di questo test, nessuno dei 5
    # test strutturali esistenti si accorgerebbe se readonly diventasse
    # False o se host/port venissero scambiati.
    #
    # Task 14: _connect_async() e' una coroutine -- eseguita qui con
    # asyncio.run() diretto, MAI tramite run_on_ibkr_loop/il thread
    # dedicato (quello lo copre test_ibkr_loop.py; qui l'unico interesse e'
    # l'invariante readonly/credenziali, non la meccanica del loop).
    monkeypatch.setattr(ibkr_source, "IB", _RecordingFakeIB)
    source = IbkrDataSource(port=4001, client_id=731)

    ib = asyncio.run(source._connect_async())

    assert ib.connect_kwargs == {
        "host": "127.0.0.1", "port": 4001, "clientId": 731,
        "timeout": 8.0, "readonly": True,
    }


class _FakeIBQualifyContractsFails:
    """Simula un ticker/indice non risolvibile: connectAsync() riesce,
    qualifyContractsAsync() solleva un'eccezione grezza di ib_async (come
    accadrebbe per un simbolo inesistente)."""

    async def connectAsync(self, *args, **kwargs):
        pass

    def disconnect(self):
        pass

    async def qualifyContractsAsync(self, contract):
        raise Exception("No security definition has been found for the request")

    async def reqHistoricalDataAsync(self, *args, **kwargs):
        raise AssertionError("reqHistoricalDataAsync non deve essere chiamato se qualifyContractsAsync fallisce")


class _FakeIBHistoricalDataFails:
    """Simula un contratto qualificato ma senza dati storici disponibili
    (es. HMDS senza dati): reqHistoricalDataAsync() solleva un'eccezione
    grezza di ib_async."""

    async def connectAsync(self, *args, **kwargs):
        pass

    def disconnect(self):
        pass

    async def qualifyContractsAsync(self, contract):
        pass

    async def reqHistoricalDataAsync(self, *args, **kwargs):
        raise Exception("HMDS query returned no data")


def test_fetch_ohlc_wraps_qualify_contracts_failure_in_readable_error_naming_ticker(monkeypatch):
    # Task 14: esercita direttamente la coroutine _fetch_ohlc_async (via
    # asyncio.run), non il metodo pubblico sincrono -- eviterebbe di
    # avviare il thread dedicato reale per un test che verifica solo il
    # wrapping dell'errore, non la meccanica del loop (vedi
    # test_ibkr_loop.py per quella).
    monkeypatch.setattr(ibkr_source, "IB", _FakeIBQualifyContractsFails)
    source = IbkrDataSource(port=4001, client_id=731)
    with pytest.raises(IbkrConnectionError, match=re.escape("ZZZINVALIDTICKER123")):
        asyncio.run(source._fetch_ohlc_async("ZZZINVALIDTICKER123", "D"))


def test_fetch_ohlc_wraps_historical_data_failure_in_readable_error_naming_ticker(monkeypatch):
    monkeypatch.setattr(ibkr_source, "IB", _FakeIBHistoricalDataFails)
    source = IbkrDataSource(port=4001, client_id=731)
    with pytest.raises(IbkrConnectionError, match=re.escape("AAPL")):
        asyncio.run(source._fetch_ohlc_async("AAPL", "D"))


def test_fetch_ohlc_public_method_routes_through_run_on_ibkr_loop_and_returns_bars(monkeypatch):
    # Unico test che esercita la wiring "lambda" del metodo pubblico
    # (fetch_ohlc -> run_on_ibkr_loop(lambda: self._fetch_ohlc_async(...)))
    # end-to-end, incluso l'avvio reale del thread dedicato -- tutti gli
    # altri test su fetch_ohlc esercitano _fetch_ohlc_async DIRETTAMENTE
    # (piu' veloci, niente thread, vedi Step 4 del brief). Qui invece si
    # verifica che il metodo pubblico sincrono instradi correttamente i
    # propri argomenti nella lambda passata a run_on_ibkr_loop -- un bug
    # plausibile (argomenti scambiati, closure che cattura la variabile
    # sbagliata, lambda dimenticata) non sarebbe visibile chiamando solo
    # _fetch_ohlc_async. Non serve ripeterlo per fetch_index_series/
    # fetch_options: stessa identica forma di wiring, stesso rischio
    # coperto una volta sola qui.
    class _FakeIBWithOneBar:
        async def connectAsync(self, *args, **kwargs):
            pass

        def disconnect(self):
            pass

        async def qualifyContractsAsync(self, contract):
            contract.conId = 265598

        async def reqHistoricalDataAsync(self, *args, **kwargs):
            return [
                type("Bar", (), {
                    "date": "2026-08-14", "open": 1.0, "high": 2.0,
                    "low": 0.5, "close": 1.5, "volume": 100.0,
                })()
            ]

    monkeypatch.setattr(ibkr_source, "IB", _FakeIBWithOneBar)
    source = IbkrDataSource(port=4001, client_id=731)

    result = source.fetch_ohlc("AAPL", "D")

    assert result == [{
        "date": "2026-08-14", "open": 1.0, "high": 2.0,
        "low": 0.5, "close": 1.5, "volume": 100.0,
    }]


def test_fetch_index_series_wraps_qualify_contracts_failure_in_readable_error_naming_index(monkeypatch):
    monkeypatch.setattr(ibkr_source, "IB", _FakeIBQualifyContractsFails)
    source = IbkrDataSource(port=4001, client_id=731)
    with pytest.raises(IbkrConnectionError, match=re.escape("^GSPC")):
        asyncio.run(source._fetch_index_series_async("^GSPC", "D"))


class _FakeIBUnresolvableContractNoException:
    """Riproduce il comportamento REALE di ib_async 2.1.0 per un ticker non
    risolvibile -- verificato dal vivo con IB Gateway (Task 4, fix
    post-review): qualifyContractsAsync() NON solleva alcuna eccezione,
    lascia semplicemente contract.conId a 0 (il contratto passato non
    viene mutato con un conId valido) -- comportamento confermato identico
    fra qualifyContracts/qualifyContractsAsync (Task 14: la sincrona e'
    letteralmente `self._run(self.qualifyContractsAsync(...))` nel sorgente
    di ib_async, verificato per introspezione). reqHistoricalDataAsync()
    ritorna poi una BarDataList vuota, anch'essa senza sollevare. Il
    fallimento e' quindi SILENZIOSO (nessuna eccezione in nessuno dei due
    passi) -- diverso dall'assunzione originale dello scaffold del brief
    (_FakeIBQualifyContractsFails sopra, che simula un'eccezione mai
    osservata dal vivo per questo caso). Una guardia esplicita su conId
    dopo qualifyContractsAsync, non un try/except, e' cio' che deve
    intercettare questo path."""

    async def connectAsync(self, *args, **kwargs):
        pass

    def disconnect(self):
        pass

    async def qualifyContractsAsync(self, contract):
        pass  # conId del contratto resta 0 (default) -- nessuna eccezione

    async def reqHistoricalDataAsync(self, *args, **kwargs):
        return []  # comportamento osservato dal vivo per un contratto non qualificato


def test_fetch_ohlc_raises_readable_error_when_contract_stays_unqualified_without_exception(monkeypatch):
    monkeypatch.setattr(ibkr_source, "IB", _FakeIBUnresolvableContractNoException)
    source = IbkrDataSource(port=4001, client_id=731)
    with pytest.raises(IbkrConnectionError, match=re.escape("ZZZINVALIDTICKER123")):
        asyncio.run(source._fetch_ohlc_async("ZZZINVALIDTICKER123", "D"))


# --- fetch_options (Task 6) ---------------------------------------------


def test_fetch_options_raises_readable_error_on_connection_failure(monkeypatch):
    source = IbkrDataSource(port=4001, client_id=731)

    async def fail_connect(self):
        raise IbkrConnectionError("simulato")

    monkeypatch.setattr(IbkrDataSource, "_connect_async", fail_connect)
    with pytest.raises(IbkrConnectionError):
        asyncio.run(source._fetch_options_async("AAPL"))


class _StubOptionChain:
    """Riproduce la forma minima di ib_async.OptionChain che il codice
    legge (exchange/expirations/strikes) -- campi confermati dallo spike
    dal vivo del Task 6 (reqSecDefOptParams su AAPL)."""

    def __init__(self, exchange, expirations, strikes):
        self.exchange = exchange
        self.expirations = expirations
        self.strikes = strikes


class _FakeIBOptionChain:
    """Fake configurabile per fetch_options -- NON e' un mock riga-per-riga
    di ib_async (vedi docstring in cima al modulo): copre solo l'unico
    invariante di sicurezza/correttezza scoperto empiricamente in questo
    task (vedi test_fetch_options_filters_unresolved_strikes_before_pricing)
    piu' pochi rami strutturali (chain assente, nessuna scadenza, scelta
    exchange, scadenza piu' vicina). Il contenuto reale dei tick (bid/ask/
    iv) e' verificato allo smoke test dal vivo, non qui.

    `resolved_strikes=None` (default) qualifica tutti gli Option passati;
    un set esplicito riproduce il comportamento dal vivo scoperto nello
    spike: reqSecDefOptParams ritorna l'unione degli strike su TUTTE le
    scadenze del titolo, quindi molti Option(strike, scadenza_piu'_vicina)
    non si qualificano per la scadenza effettivamente scelta.

    Task 14: tutti i metodi diventano async (*Async) -- il corpo logico
    resta identico, cambia solo `def` -> `async def`."""

    def __init__(self, chains, resolved_strikes=None, resolve_underlying=True):
        self.chains = chains
        self.resolved_strikes = resolved_strikes
        self.resolve_underlying = resolve_underlying
        self.req_tickers_calls: list[tuple] = []

    async def connectAsync(self, *args, **kwargs):
        pass

    def disconnect(self):
        pass

    async def qualifyContractsAsync(self, *contracts):
        for c in contracts:
            if c.secType == "STK":
                if self.resolve_underlying:
                    c.conId = 265598
            else:  # Option
                if self.resolved_strikes is None or c.strike in self.resolved_strikes:
                    # conId fittizio ma univoco e non-zero, sufficiente per
                    # superare la guardia `if c.conId`.
                    c.conId = 900000 + int(c.strike * 10) + (0 if c.right == "C" else 1)

    async def reqSecDefOptParamsAsync(self, symbol, exchange, secType, conId):
        # La guardia sul sottostante deve impedire di arrivare qui con un
        # conId non risolto -- vedi test_fetch_options_raises_readable_
        # error_when_underlying_unresolvable.
        assert conId, "reqSecDefOptParamsAsync non deve essere chiamato senza un conId valido"
        return self.chains

    async def reqTickersAsync(self, *contracts):
        self.req_tickers_calls.append(contracts)
        # Ticker "vuoto" di default (stessa forma osservata dal vivo in
        # pre-market: bid/ask=-1, last=nan, modelGreeks=None) -- il mapping
        # dei campi e' testato a parte, in isolamento, su _ticker_to_option_contract.
        return [
            type("StubTicker", (), {
                "contract": c, "bid": -1, "ask": -1, "last": float("nan"), "modelGreeks": None,
            })()
            for c in contracts
        ]


def test_fetch_options_raises_readable_error_when_underlying_unresolvable(monkeypatch):
    fake = _FakeIBOptionChain(chains=[], resolve_underlying=False)
    monkeypatch.setattr(ibkr_source, "IB", lambda: fake)
    source = IbkrDataSource(port=4001, client_id=731)
    with pytest.raises(IbkrConnectionError, match=re.escape("Impossibile risolvere il contratto per 'ZZZINVALIDTICKER123'")):
        asyncio.run(source._fetch_options_async("ZZZINVALIDTICKER123"))


def test_fetch_options_returns_empty_list_when_no_option_chains(monkeypatch):
    fake = _FakeIBOptionChain(chains=[])
    monkeypatch.setattr(ibkr_source, "IB", lambda: fake)
    source = IbkrDataSource(port=4001, client_id=731)
    assert asyncio.run(source._fetch_options_async("AAPL")) == []


def test_fetch_options_returns_empty_list_when_chain_has_no_expirations(monkeypatch):
    chain = _StubOptionChain(exchange="SMART", expirations=[], strikes=[100.0])
    fake = _FakeIBOptionChain(chains=[chain])
    monkeypatch.setattr(ibkr_source, "IB", lambda: fake)
    source = IbkrDataSource(port=4001, client_id=731)
    assert asyncio.run(source._fetch_options_async("AAPL")) == []


def test_fetch_options_prefers_smart_exchange_chain_when_multiple_present(monkeypatch):
    other_chain = _StubOptionChain(exchange="CBOE", expirations=["20260814"], strikes=[999.0])
    smart_chain = _StubOptionChain(exchange="SMART", expirations=["20260814"], strikes=[100.0])
    fake = _FakeIBOptionChain(chains=[other_chain, smart_chain])
    monkeypatch.setattr(ibkr_source, "IB", lambda: fake)
    source = IbkrDataSource(port=4001, client_id=731)

    result = asyncio.run(source._fetch_options_async("AAPL"))

    assert {opt["strike"] for opt in result} == {100.0}


def test_fetch_options_uses_nearest_expiry_only(monkeypatch):
    # Scadenze deliberatamente fuori ordine -- il codice deve ordinarle e
    # prendere solo la piu' vicina, stessa scelta di YFinanceDataSource
    # (yfinance_source.py, refinement 2026-07-23).
    chain = _StubOptionChain(exchange="SMART", expirations=["20261219", "20260814", "20260901"], strikes=[100.0])
    fake = _FakeIBOptionChain(chains=[chain])
    monkeypatch.setattr(ibkr_source, "IB", lambda: fake)
    source = IbkrDataSource(port=4001, client_id=731)

    result = asyncio.run(source._fetch_options_async("AAPL"))

    # Formato ISO (YYYY-MM-DD), non il formato grezzo IB (YYYYMMDD) -- vedi
    # test_fetch_options_expiry_matches_yfinance_iso_format sotto.
    assert result and all(opt["expiry"] == "2026-08-14" for opt in result)


def test_fetch_options_expiry_matches_yfinance_iso_format(monkeypatch):
    # IB esprime le scadenze in 'YYYYMMDD' (formato nativo di
    # reqSecDefOptParams, confermato dallo spike dal vivo del Task 6), ma
    # YFinanceDataSource.fetch_options (yfinance_source.py) e i consumer
    # esistenti (report.py, test_option_chain.py fixture) usano 'YYYY-MM-DD'
    # -- lo stesso campo OptionContract["expiry"] deve avere lo stesso
    # formato indipendentemente dalla fonte, o il report mostra date
    # incoerenti a seconda della fonte dati attiva.
    chain = _StubOptionChain(exchange="SMART", expirations=["20260814"], strikes=[100.0])
    fake = _FakeIBOptionChain(chains=[chain])
    monkeypatch.setattr(ibkr_source, "IB", lambda: fake)
    source = IbkrDataSource(port=4001, client_id=731)

    result = asyncio.run(source._fetch_options_async("AAPL"))

    assert result and all(opt["expiry"] == "2026-08-14" for opt in result)


def test_fetch_options_filters_unresolved_strikes_before_pricing(monkeypatch):
    # Scoperta empirica dello spike dal vivo (Task 6): reqSecDefOptParams
    # ritorna l'unione degli strike su TUTTE le scadenze del titolo, non
    # solo quella piu' vicina selezionata da fetch_options -- su AAPL,
    # 86/254 combinazioni strike/right non si sono qualificate per la
    # scadenza piu' vicina (conId rimasto 0, NESSUNA eccezione sollevata,
    # stessa "guardia esplicita" gia' nota per fetch_ohlc). Passare un
    # contratto non qualificato a reqTickers solleva un ValueError non
    # gestito piu' in basso in ib_async (Contract.__hash__ richiede un
    # conId) -- verificato dal vivo. Il codice DEVE filtrare i non
    # risolti PRIMA di chiamare reqTickersAsync, mai dopo.
    chain = _StubOptionChain(exchange="SMART", expirations=["20260814"], strikes=[90.0, 100.0, 110.0])
    fake = _FakeIBOptionChain(chains=[chain], resolved_strikes={100.0})
    monkeypatch.setattr(ibkr_source, "IB", lambda: fake)
    source = IbkrDataSource(port=4001, client_id=731)

    result = asyncio.run(source._fetch_options_async("AAPL"))

    assert len(fake.req_tickers_calls) == 1
    priced_strikes = {c.strike for c in fake.req_tickers_calls[0]}
    assert priced_strikes == {100.0}
    assert {opt["strike"] for opt in result} == {100.0}
    assert len(result) == 2  # call + put per l'unico strike risolto


def test_fetch_options_returns_empty_list_when_no_strike_resolves(monkeypatch):
    chain = _StubOptionChain(exchange="SMART", expirations=["20260814"], strikes=[90.0, 110.0])
    fake = _FakeIBOptionChain(chains=[chain], resolved_strikes=set())
    monkeypatch.setattr(ibkr_source, "IB", lambda: fake)
    source = IbkrDataSource(port=4001, client_id=731)

    assert asyncio.run(source._fetch_options_async("AAPL")) == []
    assert fake.req_tickers_calls == []  # mai chiamato con una lista vuota


# --- fetch_fundamentals / fetch_screening_snapshot (Task 5 finale) ------
#
# Decisione utente 2026-08-14 (task-5-final-brief.md): niente sottoscrizione
# Reuters Fundamentals, "riduci scope, resta gratis" -- questi due metodi
# ritornano dati STATICI, mai una chiamata di rete/ib_async. Il motivo
# dell'assenza e' un entitlement gap (permesso mancante sull'account), non
# un problema di connettivita' -- non dipende da IB Gateway essendo
# raggiungibile o meno, quindi zero I/O per costruzione, sempre.


def test_fetch_fundamentals_returns_every_expected_key_without_any_network_call(monkeypatch):
    # "IB() non chiamato" e' l'invariante piu' importante di questo test:
    # sostituendo IB con qualcosa che esplode SE istanziato, un'eventuale
    # regressione che tentasse comunque una connessione fallirebbe qui, non
    # in produzione contro un vero Gateway.
    def _boom(*args, **kwargs):
        raise AssertionError("IB() non deve essere istanziato da fetch_fundamentals")
    monkeypatch.setattr(ibkr_source, "IB", _boom)
    source = IbkrDataSource(port=4001, client_id=731)

    result = source.fetch_fundamentals("AAPL")

    assert set(result.keys()) == {
        "ticker", "name", "sector", "industry", "market_cap", "pe_ratio",
        "current_price", "revenue_by_period", "earnings_by_period",
    }
    assert result["ticker"] == "AAPL"
    assert result["name"] == "N/D"
    assert result["sector"] == "N/D"
    assert result["industry"] == "N/D"
    assert result["market_cap"] is None
    assert result["pe_ratio"] is None
    assert result["current_price"] is None
    assert result["revenue_by_period"] == []
    assert result["earnings_by_period"] == []


def test_fetch_screening_snapshot_returns_every_expected_key_without_any_network_call(monkeypatch):
    def _boom(*args, **kwargs):
        raise AssertionError("IB() non deve essere istanziato da fetch_screening_snapshot")
    monkeypatch.setattr(ibkr_source, "IB", _boom)
    source = IbkrDataSource(port=4001, client_id=731)

    result = source.fetch_screening_snapshot("AAPL")

    assert set(result.keys()) == {
        "sector", "market_cap", "ps_ratio", "revenue_growth",
        "fifty_two_week_high", "current_price", "target_mean_price",
        "peg_ratio", "pe_ratio", "debt_to_equity", "dividend_yield",
        "payout_ratio", "target_high_price", "target_low_price",
        "industry",
    }
    assert all(v is None for v in result.values())


# --- _ticker_to_option_contract (funzione pura, nessun fake necessario) ---


def test_ticker_to_option_contract_maps_call_and_put():
    from types import SimpleNamespace

    from data_sources.ibkr_source import _ticker_to_option_contract

    call_ticker = SimpleNamespace(
        contract=SimpleNamespace(strike=100.0, right="C"),
        bid=1.5, ask=1.6, last=1.55, modelGreeks=SimpleNamespace(impliedVol=0.25),
    )
    put_ticker = SimpleNamespace(
        contract=SimpleNamespace(strike=100.0, right="P"),
        bid=2.0, ask=2.1, last=2.05, modelGreeks=None,
    )

    call = _ticker_to_option_contract(call_ticker, "20260814")
    put = _ticker_to_option_contract(put_ticker, "20260814")

    assert call == {
        "strike": 100.0, "expiry": "20260814", "iv": 0.25, "option_type": "call",
        "bid": 1.5, "ask": 1.6, "last_price": 1.55,
    }
    assert put["option_type"] == "put"
    assert put["iv"] is None


def test_ticker_to_option_contract_treats_non_positive_bid_ask_last_as_none():
    from types import SimpleNamespace

    from data_sources.ibkr_source import _ticker_to_option_contract

    # Forma osservata dal vivo in pre-market (spike Task 6): bid/ask=-1
    # ("nessuna quotazione"), last=nan -- nessuno dei tre e' un prezzo
    # valido e deve diventare None, mai -1/-1/nan passati al chiamante.
    ticker = SimpleNamespace(
        contract=SimpleNamespace(strike=100.0, right="C"),
        bid=-1, ask=-1, last=float("nan"), modelGreeks=None,
    )

    result = _ticker_to_option_contract(ticker, "20260814")

    assert result["bid"] is None
    assert result["ask"] is None
    assert result["last_price"] is None
    assert result["iv"] is None


def test_ticker_to_option_contract_treats_zero_implied_vol_as_none():
    from types import SimpleNamespace

    from data_sources.ibkr_source import _ticker_to_option_contract

    ticker = SimpleNamespace(
        contract=SimpleNamespace(strike=100.0, right="P"),
        bid=1.0, ask=1.1, last=1.05, modelGreeks=SimpleNamespace(impliedVol=0.0),
    )

    result = _ticker_to_option_contract(ticker, "20260814")

    assert result["iv"] is None


# --- test_connection: timeout esplicito piu' basso del default (Fix F, ------
# review-fix-wave 2026-08-14) ------------------------------------------------


def test_test_connection_passes_a_lower_timeout_than_the_60s_default(monkeypatch):
    # Il client MCP SCOPED usato dal bottone "Test connessione" (ws.rs,
    # test_market_data_source_now()) passa call_timeout_secs=30 --
    # tokio::time::timeout che UCCIDE il processo Python se non risponde in
    # tempo. run_on_ibkr_loop ha un default di 60s: se una connessione e'
    # lenta abbastanza da avvicinarsi a 30s, il lato Rust ucciderebbe il
    # processo PRIMA che il timeout Python (messaggio piu' specifico) possa
    # mai scattare. test_connection deve passare un timeout esplicito
    # comodamente sotto il tetto Rust (20s), NON il default a 60s -- spy su
    # run_on_ibkr_loop (mai un vero I/O), mirror dell'approccio monkeypatch
    # gia' usato sopra per IB.
    captured = {}

    def _fake_run_on_ibkr_loop(coro_factory, timeout=60.0):
        captured["timeout"] = timeout
        return (True, "ok")

    monkeypatch.setattr(ibkr_source, "run_on_ibkr_loop", _fake_run_on_ibkr_loop)
    source = IbkrDataSource(port=4001, client_id=731)

    source.test_connection()

    assert captured["timeout"] == 20.0, "test_connection deve passare timeout=20.0 esplicito, non il default 60s"
