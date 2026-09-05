"""Grafici candlestick 5 timeframe → PNG → base64 embedded. Usa mplfinance,
che si aspetta un DataFrame indicizzato per data con colonne
Open/High/Low/Close/Volume — qui costruito dai bar di data_sources."""

import base64
import io

# Il server MCP gira in un subprocess headless (nessun display, nessuna
# GUI): matplotlib NON deve selezionare un backend interattivo (es. TkAgg,
# che su Windows è la scelta automatica di default quando Tk è disponibile).
# Un backend interattivo può fallire in modo intermittente con
# _tkinter.TclError quando si renderizzano molti grafici in sequenza in un
# contesto non interattivo — osservato dal vivo eseguendo l'intera suite di
# test (che renderizza molti più grafici di quanti ne renderizzi in
# isolamento tramite report.py).
#
# `matplotlib.use("Agg")` DEVE essere chiamato prima che `mplfinance` (che
# importa `matplotlib.pyplot` al suo interno) o `matplotlib.pyplot` vengano
# importati ovunque nel processo: una volta che pyplot ha già scelto un
# backend, `use()` successivo non ha più effetto in modo affidabile. Per
# questo l'import di matplotlib e la use() qui sopra precedono l'import di
# mplfinance in questo file — è deliberatamente fuori ordine alfabetico.
import matplotlib

matplotlib.use("Agg")

import mplfinance as mpf
import pandas as pd

from data_sources import OhlcBar, TIMEFRAMES

# Palette allineata al tema scuro dell'app (crates/ui/frontend/config.css:
# sfondo finestra rgba(20,24,36,...), testo #e8eefc) — refinement 2026-07-23,
# prima i grafici usavano lo sfondo bianco di default di mplfinance, stonato
# nella finestra scura del report.
_DARK_BG = "#141824"
_DARK_FG = "#e8eefc"
_DARK_GRID = "#2a3040"
_UP_COLOR = "#26a69a"
_DOWN_COLOR = "#ef5350"


def _dark_style():
    # Costruito a ogni chiamata (non una costante a livello di modulo):
    # `mpf.make_mpf_style` non è documentata come thread-safe/idempotente da
    # richiamare su un oggetto condiviso fra plot diversi, e il costo è
    # trascurabile rispetto al rendering stesso.
    marketcolors = mpf.make_marketcolors(
        up=_UP_COLOR,
        down=_DOWN_COLOR,
        edge={"up": _UP_COLOR, "down": _DOWN_COLOR},
        wick={"up": _UP_COLOR, "down": _DOWN_COLOR},
        volume={"up": _UP_COLOR, "down": _DOWN_COLOR},
    )
    return mpf.make_mpf_style(
        base_mpf_style="nightclouds",
        marketcolors=marketcolors,
        facecolor=_DARK_BG,
        figcolor=_DARK_BG,
        gridcolor=_DARK_GRID,
        gridstyle=":",
        rc={
            "axes.labelcolor": _DARK_FG,
            "axes.titlecolor": _DARK_FG,
            "xtick.color": _DARK_FG,
            "ytick.color": _DARK_FG,
            "text.color": _DARK_FG,
        },
    )


def bars_to_dataframe(bars: list[OhlcBar]) -> pd.DataFrame:
    df = pd.DataFrame(bars)
    # I bar reali (yfinance) arrivano già in ora locale del mercato USA
    # (America/New_York, offset -05:00 EST o -04:00 EDT secondo il periodo
    # dell'anno) — verificato dal vivo. Una finestra di 60gg per H4/H1 può
    # attraversare il cambio ora legale: pandas rifiuta di costruire un
    # DatetimeIndex con offset misti senza normalizzare esplicitamente
    # (errore "Mixed timezones detected", osservato dal vivo su AAPL).
    # `utc=True` risolve l'ambiguità di parsing; il successivo
    # `tz_convert` riporta i timestamp all'ora locale del mercato (senza,
    # i grafici H1/H4 mostrerebbero l'ora UTC invece di quella di
    # apertura/chiusura reale — un errore silenzioso ma visibile
    # nell'etichetta dell'asse). Per barre D/W/M (già mezzanotte
    # America/New_York) il giro andata-ritorno per UTC è un no-op: stesso
    # orario, stessa data.
    df["date"] = pd.to_datetime(df["date"], utc=True).dt.tz_convert("America/New_York")
    df = df.set_index("date")
    return df.rename(columns={"open": "Open", "high": "High", "low": "Low", "close": "Close", "volume": "Volume"})


def candlestick_png_base64(bars: list[OhlcBar], title: str) -> str | None:
    """Ritorna una data URI `data:image/png;base64,...` pronta per un tag
    Markdown `![...](...)`. Nessun file temporaneo: mplfinance scrive su un
    buffer in memoria.

    Sentinel: se `bars` è vuota (es. nessun dato intraday disponibile per
    quel timeframe) ritorna `None` invece di tentare il plot — un DataFrame
    vuoto non ha la colonna "date" e farebbe fallire `bars_to_dataframe` con
    `KeyError`, che altrimenti si propagherebbe fino a far crashare l'intero
    report (vedi design spec §4: dati mancanti → sezione omessa, mai
    un'eccezione). Il chiamante (`all_timeframe_charts`, e a cascata
    `report.py::_charts_section`) deve riconoscere `None` e renderizzare una
    nota "non disponibile" al posto dell'immagine."""
    if not bars:
        return None
    df = bars_to_dataframe(bars)
    buffer = io.BytesIO()
    # `facecolor` in `savefig` copre ANCHE il margine attorno agli assi (lo
    # stile da solo colora l'area del plot, non l'intera figura salvata) —
    # senza, il PNG avrebbe un bordo bianco visibile nella finestra scura.
    mpf.plot(df, type="candle", title=title, style=_dark_style(), savefig=dict(fname=buffer, format="png", facecolor=_DARK_BG))
    buffer.seek(0)
    encoded = base64.b64encode(buffer.read()).decode("ascii")
    return f"data:image/png;base64,{encoded}"


def all_timeframe_charts(fetch_ohlc, ticker: str) -> dict[str, str | None]:
    """`fetch_ohlc` è `DataSource.fetch_ohlc` (o un adattatore equivalente) —
    un grafico per ciascuno dei 5 timeframe. Un valore `None` per un
    timeframe significa "nessun dato disponibile" (propagato da
    `candlestick_png_base64`), non un errore."""
    return {tf: candlestick_png_base64(fetch_ohlc(ticker, tf), f"{ticker} — {tf}") for tf in TIMEFRAMES}
