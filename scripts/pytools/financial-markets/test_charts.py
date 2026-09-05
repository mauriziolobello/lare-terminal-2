"""Test per charts.py — usa FakeDataSource, verifica la forma della data URI
(non il contenuto esatto dei pixel: non è quello che deve restare stabile,
TRANNE il tema scuro dello sfondo — quello è un requisito esplicito, verificato
sotto leggendo un pixel d'angolo)."""

import base64
import datetime
import io

import matplotlib
from data_sources.fake_source import FakeDataSource
import charts


def test_charts_module_forces_the_agg_backend():
    # charts.py deve forzare il backend "Agg" (headless, nessuna GUI) in
    # codice — non affidarsi a una variabile d'ambiente (MPLBACKEND) che il
    # chiamante potrebbe scordare. L'MCP server gira in un subprocess senza
    # display: se matplotlib seleziona un backend interattivo (es. TkAgg su
    # Windows), il rendering ripetuto può fallire in modo intermittente con
    # _tkinter.TclError.
    assert matplotlib.get_backend().lower() == "agg"


def test_candlestick_png_base64_returns_a_valid_data_uri():
    fake = FakeDataSource()
    bars = fake.fetch_ohlc("FAKE", "D")
    result = charts.candlestick_png_base64(bars, "FAKE — D")
    assert result.startswith("data:image/png;base64,")
    payload = result.removeprefix("data:image/png;base64,")
    decoded = base64.b64decode(payload)
    assert decoded[:8] == b"\x89PNG\r\n\x1a\n"  # magic number PNG


def test_all_timeframe_charts_returns_one_entry_per_timeframe():
    fake = FakeDataSource()
    result = charts.all_timeframe_charts(fake.fetch_ohlc, "FAKE")
    assert set(result.keys()) == {"M", "W", "D", "H4", "H1"}
    for uri in result.values():
        assert uri.startswith("data:image/png;base64,")


def test_candlestick_png_base64_returns_none_for_empty_bars():
    # Un timeframe senza dati (es. un ticker senza intraday H1 disponibile)
    # non deve far esplodere l'intero report con una KeyError su "date" —
    # deve degradare a un sentinel che il chiamante (report.py) sa
    # riconoscere e rendere come sezione "non disponibile". None è il
    # sentinel scelto: nessuna immagine da mostrare per questo timeframe.
    assert charts.candlestick_png_base64([], "EMPTY") is None


def test_all_timeframe_charts_propagates_none_for_empty_timeframe():
    fake = FakeDataSource()

    def fetch_ohlc_with_empty_h1(ticker, tf):
        if tf == "H1":
            return []
        return fake.fetch_ohlc(ticker, tf)

    result = charts.all_timeframe_charts(fetch_ohlc_with_empty_h1, "FAKE")
    assert result["H1"] is None
    for tf in ("M", "W", "D", "H4"):
        assert result[tf].startswith("data:image/png;base64,")


def test_candlestick_png_base64_uses_a_dark_background_not_mplfinance_default_white():
    # Refinement 2026-07-23: prima il grafico usava lo sfondo bianco di
    # default di mplfinance, stonato nella finestra scura del report — un
    # angolo dell'immagine (lontano dalle candele, certamente sfondo puro)
    # deve essere scuro, non bianco.
    from PIL import Image

    fake = FakeDataSource()
    bars = fake.fetch_ohlc("FAKE", "D")
    result = charts.candlestick_png_base64(bars, "FAKE — D")
    payload = result.removeprefix("data:image/png;base64,")
    img = Image.open(io.BytesIO(base64.b64decode(payload))).convert("RGB")
    corner = img.getpixel((0, 0))
    brightness = sum(corner) / 3
    assert brightness < 100, f"angolo dell'immagine troppo chiaro per uno sfondo scuro: {corner}"


def test_bars_to_dataframe_converts_real_offsets_to_market_local_hour():
    # Riproduce esattamente la forma dei bar reali di yfinance (offset UTC
    # espliciti, non stringhe naive come il resto dei fixture di questo
    # file): senza il tz_convert("America/New_York") in bars_to_dataframe,
    # questa barra delle 9:30 EDT (apertura mercato) verrebbe mostrata
    # sull'asse del grafico come 13:30 UTC — un errore silenzioso ma
    # visibile, trovato dal vivo interrogando AAPL. Nessuna rete: la
    # stringa è scritta a mano nello stesso formato di
    # yfinance_source.py::_rows_to_bars (str() di un Timestamp tz-aware).
    bars = [{"date": "2025-07-16 09:30:00-04:00", "open": 100.0, "high": 101.0, "low": 99.0, "close": 100.5, "volume": 1000}]
    df = charts.bars_to_dataframe(bars)
    assert df.index[0].hour == 9, f"atteso ora locale di mercato 9 (apertura), trovato {df.index[0].hour}"
    assert str(df.index.tz) == "America/New_York"


def test_bars_to_dataframe_preserves_calendar_date_across_dst_boundary():
    # Guardia contro la regressione opposta: il giro andata-ritorno per UTC
    # non deve spostare la DATA di una barra a mezzanotte (D/W/M) — America/
    # New_York è sempre a ovest di UTC, quindi una mezzanotte locale cade
    # nelle prime ore dello STESSO giorno UTC, mai nel giorno precedente.
    # Il cambio ora legale 2025 è il 9 marzo alle 2:00 locali: il 7 marzo è
    # ancora EST (-05:00), il 10 marzo è già EDT (-04:00) — entrambe date
    # scelte con margine dal momento esatto del cambio, per evitare offset
    # inventati che non corrispondono a un istante reale (il 9 marzo stesso
    # a mezzanotte sarebbe ancora EST, non EDT: usarlo qui produrrebbe un
    # falso positivo/negativo dovuto a un dato di partenza sbagliato, non al
    # codice sotto test).
    bars = [
        {"date": "2025-03-07 00:00:00-05:00", "open": 1, "high": 1, "low": 1, "close": 1, "volume": 1},  # EST, prima del cambio
        {"date": "2025-03-10 00:00:00-04:00", "open": 1, "high": 1, "low": 1, "close": 1, "volume": 1},  # EDT, dopo il cambio
    ]
    df = charts.bars_to_dataframe(bars)
    assert list(df.index.date) == [datetime.date(2025, 3, 7), datetime.date(2025, 3, 10)]
