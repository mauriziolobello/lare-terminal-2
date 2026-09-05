"""Test per report.py — usa FakeDataSource, verifica che tutte le sezioni
fattuali siano presenti nel Markdown risultante, e che il riassunto di
canale sia sempre entro 5 righe con numeri tracciabili al documento."""

from data_sources.fake_source import FakeDataSource
import report


def test_build_report_contains_all_four_factual_sections():
    fake = FakeDataSource()
    markdown, _summary = report.build_report(fake, "FAKE")
    assert "## Fondamentale" in markdown
    assert "## Grafici" in markdown
    assert "## Option chain" in markdown
    assert "## Comparative" in markdown


def test_build_report_embeds_a_chart_per_timeframe():
    fake = FakeDataSource()
    markdown, _summary = report.build_report(fake, "FAKE")
    for tf in ("M", "W", "D", "H4", "H1"):
        assert f"### {tf}" in markdown
    assert markdown.count("data:image/png;base64,") == 5


def test_build_report_omits_options_gracefully_when_none_available():
    fake = FakeDataSource()
    fake.options_by_ticker["FAKE"] = []
    markdown, _summary = report.build_report(fake, "FAKE")
    assert "Non disponibile per questo ticker" in markdown


def test_build_report_renders_a_two_sided_option_chain_table():
    fake = FakeDataSource()
    markdown, _summary = report.build_report(fake, "FAKE")
    assert '<th colspan="4">CALL</th>' in markdown
    assert '<th colspan="4">PUT</th>' in markdown


def test_build_report_formats_market_cap_with_thousands_separator_and_abbreviation():
    # Bug segnalato dall'utente: "Market cap: 4786462654464" illeggibile —
    # ora deve avere separatori delle migliaia E una forma abbreviata.
    fake = FakeDataSource()
    fake.fundamentals_by_ticker["FAKE"]["market_cap"] = 4786462654464.0
    markdown, _summary = report.build_report(fake, "FAKE")
    assert "4.786.462.654.464" in markdown
    assert "4,79T" in markdown


def test_build_report_omits_chart_gracefully_when_timeframe_has_no_data():
    # Un ticker con fondamentali e daily ma senza intraday H1 disponibile
    # (caso reale yfinance) non deve far crashare l'intero report con una
    # KeyError su "date" propagata da charts.bars_to_dataframe — prima del
    # fix questa chiamata solleva KeyError; con il fix la sezione grafico
    # per H1 diventa una nota "non disponibile", il resto del report resta
    # intatto.
    fake = FakeDataSource()
    fake.ohlc_by_ticker_timeframe[("FAKE", "H1")] = []
    markdown, _summary = report.build_report(fake, "FAKE")
    assert "### H1" in markdown
    assert "Non disponibile per questo timeframe" in markdown
    # Gli altri 4 timeframe restano grafici embedded regolari.
    assert markdown.count("data:image/png;base64,") == 4


def test_build_report_channel_summary_is_at_most_five_lines():
    fake = FakeDataSource()
    _markdown, summary = report.build_report(fake, "FAKE")
    assert summary.count("\n") <= 4, f"atteso massimo 5 righe: {summary!r}"


def test_build_report_channel_summary_numbers_are_traceable_to_the_document():
    # I numeri del riassunto di canale devono comparire ANCHE nel documento
    # completo — mai un dato "nuovo" che solo il pannello mostra.
    fake = FakeDataSource()
    markdown, summary = report.build_report(fake, "FAKE")
    assert "FAKE" in summary
    assert "20,00" in summary  # P/E, stesso formato del Fondamentale
    assert "20,00" in markdown


def test_strip_chart_images_removes_all_embedded_pngs():
    # Il riassunto per l'AI (server.py::stock_report) non deve contenere le
    # immagini base64 (megabyte di testo che l'AI non "vede" comunque in
    # modo utile) — solo il documento completo (per la finestra/Library) le
    # mantiene. Vedi Docs/superpowers/specs/2026-07-22-financial-markets-
    # stock-report-design.md e la correzione dal vivo del 2026-07-23.
    fake = FakeDataSource()
    full_markdown, _summary = report.build_report(fake, "FAKE")
    stripped = report.strip_chart_images(full_markdown)
    assert "data:image/png;base64," not in stripped


def test_strip_chart_images_keeps_all_other_content_intact():
    fake = FakeDataSource()
    full_markdown, _summary = report.build_report(fake, "FAKE")
    stripped = report.strip_chart_images(full_markdown)
    for heading in ("## Fondamentale", "## Grafici", "## Option chain", "## Comparative", "### M", "### H1"):
        assert heading in stripped, f"sezione persa dopo lo strip: {heading}"


def test_strip_chart_images_is_a_no_op_when_there_are_no_images():
    text = "## Option chain\n\nNon disponibile per questo ticker."
    assert report.strip_chart_images(text) == text


# --- avviso campi non disponibili (Task 5/7/8 finale, 2026-08-14) ---------


def test_build_report_shows_visible_warning_when_source_reports_unavailable_fields():
    # Monkeypatch diretto sull'istanza (come da brief): FakeDataSource resta
    # invariata nel file, solo questo test override il metodo.
    fake = FakeDataSource()
    fake.unavailable_fields = lambda: {"peg_ratio"}
    markdown, _summary = report.build_report(fake, "FAKE")
    assert "PEG ratio" in markdown


def test_build_report_omits_warning_when_source_has_no_unavailable_fields():
    # Comportamento YFinance/FakeDataSource INVARIATO: nessun override,
    # nessun avviso.
    fake = FakeDataSource()
    markdown, _summary = report.build_report(fake, "FAKE")
    assert "⚠️ Fonte dati" not in markdown


class _IbkrLikeFundamentalsSource(FakeDataSource):
    """Imita ESATTAMENTE lo shape ritornato da
    IbkrDataSource.fetch_fundamentals (Reuters Fundamentals non
    sottoscritto -- vedi ibkr_source.py): tutti i campi testuali "N/D",
    tutti i numerici None, liste storiche vuote. Eredita il resto da
    FakeDataSource (ohlc/options/index_series) per esercitare
    build_report() end-to-end senza duplicare tutte le fixture."""

    def fetch_fundamentals(self, ticker):
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


def test_build_report_survives_a_source_with_all_fundamentals_nd_or_none():
    # Prova diretta che report.py (accesso diretto fx["chiave"], MAI
    # .get(...)) sopravvive a una fonte con fundamentals interamente
    # assenti -- nessun KeyError/TypeError, e "N/D" compare nel documento
    # (titolo e/o riga Settore).
    fake = _IbkrLikeFundamentalsSource()
    markdown, _summary = report.build_report(fake, "FAKE")
    assert "N/D" in markdown


# --- degrado su fallimento runtime IBKR (Fix D, review-fix-wave 2026-08-14) -


class _IbkrLikeFailingSource(FakeDataSource):
    """Mirror del comportamento REALE di IbkrDataSource: solleva
    IbkrConnectionError a runtime (es. TWS/Gateway spento a metà di uno
    stock_report) invece di ritornare dati. Usa la classe VERA (non un
    doppio locale) perché la produzione fa isinstance(e, IbkrConnectionError)
    -- un mock non correlato non verrebbe intercettato."""

    def fetch_ohlc(self, ticker, timeframe):
        from data_sources.ibkr_source import IbkrConnectionError
        raise IbkrConnectionError("Impossibile connettersi a TWS/Gateway su 127.0.0.1:4001")


def test_build_report_degrades_gracefully_instead_of_raising_on_ibkr_connection_error():
    # A differenza di YFinanceDataSource (mai testata contro fallimenti di
    # rete qui), IbkrDataSource solleva DELIBERATAMENTE quando TWS/Gateway è
    # spento -- build_report non deve MAI propagare l'eccezione grezza a
    # server.py/FastMCP, deve degradare a un documento con solo l'avviso.
    fake = _IbkrLikeFailingSource()
    markdown, summary = report.build_report(fake, "FAKE")
    assert "⚠️ Impossibile generare il report" in markdown
    assert "TWS/Gateway" in markdown
    assert "⚠️ Impossibile generare il report" in summary


def test_build_report_still_raises_on_unrelated_exceptions():
    # Non regressione: un'eccezione NON legata a IBKR (bug genuino altrove)
    # non deve essere silenziosamente inghiottita -- solo IbkrConnectionError/
    # ValueError vengono degradati, tutto il resto propaga come prima.
    class _BrokenSource(FakeDataSource):
        def fetch_ohlc(self, ticker, timeframe):
            raise RuntimeError("bug genuino, non IBKR")

    import pytest
    with pytest.raises(RuntimeError, match="bug genuino"):
        report.build_report(_BrokenSource(), "FAKE")


def test_build_report_still_raises_original_exception_when_ibkr_source_is_not_importable(monkeypatch):
    # Segnalato dall'advisor in review (2026-08-14): l'import lazy di
    # IbkrConnectionError dentro l'except di build_report gira SEMPRE, anche
    # per eccezioni non-IBKR -- se ib_async non e' installato in questo
    # ambiente (es. la fonte attiva e' YFinance e ib_async non e' mai stato
    # richiesto), quell'import solleva ImportError e MASCHERA l'eccezione
    # originale invece di propagarla. `sys.modules[nome] = None` e' il modo
    # standard per simulare "modulo non importabile" senza disinstallare
    # davvero ib_async dal venv di test.
    import sys
    monkeypatch.setitem(sys.modules, "data_sources.ibkr_source", None)

    class _BrokenSource(FakeDataSource):
        def fetch_ohlc(self, ticker, timeframe):
            raise RuntimeError("bug non IBKR, ma ib_async non importabile qui")

    import pytest
    with pytest.raises(RuntimeError, match="bug non IBKR"):
        report.build_report(_BrokenSource(), "FAKE")
