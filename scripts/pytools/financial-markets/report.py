"""Assembla le sezioni fattuali 1-4 del report in un unico documento
Markdown, più un riassunto numerico brevissimo per il pannello del canale.
Le sezioni 5-6 (narrativa/ipotesi investimento) NON sono qui: le scrive
l'AI leggendo il riassunto testuale, e vengono fuse nel documento lato Rust
a fine turno (vedi FINANCIAL_MARKETS_SYSTEM_PROMPT e
`ChannelReport::defer_to_turn_end`).

`build_report` produce `(full_markdown, channel_summary)`: il primo (con i
grafici embedded) per la finestra Markdown/Library; il secondo — poche
righe, tutti numeri già presenti nelle sezioni del primo — per il pannello
del canale, mostrato SUBITO, prima che l'AI scriva la sua analisi.
`strip_chart_images` deriva dal primo la variante per l'AI (stessi dati
fattuali, senza le immagini — l'AI non "vede" un data URI base64 in modo
utile, e ogni immagine pesa decine di KB di contesto)."""

import re

from data_sources import DataSource, TIMEFRAMES, format_unavailable_warning
import charts
import fundamentals
import number_format
import option_chain
import peers

# Un tag Markdown `![alt](data:image/png;base64,...)` — usato solo per
# derivare il riassunto testuale per l'AI, mai per il documento completo.
_CHART_IMAGE_PATTERN = re.compile(r"!\[[^\]]*\]\(data:image/png;base64,[^)]*\)")


def build_report(source: DataSource, ticker: str) -> tuple[str, str]:
    try:
        fx = source.fetch_fundamentals(ticker)
        ohlc_by_tf = {tf: source.fetch_ohlc(ticker, tf) for tf in TIMEFRAMES}
        options = source.fetch_options(ticker)
        peer_tickers = peers.peers_for_sector(fx["sector"], exclude_ticker=ticker)
        # Calcolato UNA volta: serve sia alla sezione Comparative sia al
        # riassunto di canale — ripeterlo raddoppierebbe una fetch di rete
        # reale (fetch_index_series) per ogni stock_report.
        idx = fundamentals.index_comparison(source, ohlc_by_tf["D"])
    except Exception as e:
        # A differenza di YFinanceDataSource (storicamente mai testata contro
        # fallimenti di rete qui), IbkrDataSource solleva DELIBERATAMENTE
        # IbkrConnectionError/ValueError quando TWS/Gateway è spento o un
        # simbolo non risolve (Task 4/6, comportamento voluto — un
        # fallimento silenzioso sarebbe peggio). Import lazy per evitare di
        # tirare in ib_async quando la fonte attiva è YFinance (stesso
        # pattern usato in market_data_config.py::build_data_source). Solo
        # questi due tipi vengono degradati — un'eccezione NON legata a IBKR
        # (bug genuino altrove) deve continuare a propagare, non essere
        # inghiottita silenziosamente. Se ib_async NON è installato in
        # questo ambiente (fonte attiva YFinance, mai stato necessario
        # importarlo prima), l'import stesso solleva ImportError -- senza
        # questo try/except quell'ImportError MASCHEREREBBE l'eccezione
        # originale `e` (qualunque essa sia) invece di propagarla, una
        # regressione rispetto al comportamento pre-fix (trovato in
        # review, 2026-08-14).
        try:
            from data_sources.ibkr_source import IbkrConnectionError
        except ImportError:
            raise e
        if not isinstance(e, (IbkrConnectionError, ValueError)):
            raise
        warning = f"⚠️ Impossibile generare il report: {e}"
        full_markdown = "\n\n".join([f"# {ticker}", warning])
        return full_markdown, warning

    sections = [
        f"# {fx['name']} ({ticker})",
        _fundamentals_section(fx),
        _charts_section(ohlc_by_tf, ticker),
        _options_section(options, fx.get("current_price")),
        _comparative_section(source, ticker, fx, peer_tickers, idx),
    ]
    # Avviso VISIBILE (mai calcolato sopra, solo mostrato) quando la fonte
    # dichiara campi strutturalmente assenti (es. IbkrDataSource senza
    # sottoscrizione Reuters Fundamentals) — subito dopo il titolo, prima
    # di qualunque sezione fattuale. Nessun avviso per YFinance/
    # FakeDataSource (unavailable_fields() torna set() di default).
    warning = format_unavailable_warning(source.unavailable_fields())
    if warning:
        sections.insert(1, warning)
    full_markdown = "\n\n".join(sections)
    return full_markdown, build_channel_summary(fx, idx)


def build_channel_summary(fx: dict, idx: dict) -> str:
    """Riassunto numerico ≤5 righe per il pannello del canale — mostrato
    SUBITO dopo il dispatch, deterministico (mai dall'AI). Ogni numero qui
    è già presente, a vario titolo, nelle sezioni Fondamentale/Comparative
    del documento completo — nessun dato "nuovo" che il documento non
    abbia."""
    ticker_pct = f"{idx['ticker_change_pct']:.2f}%" if idx["ticker_change_pct"] is not None else "n/d"
    index_pct = f"{idx['index_change_pct']:.2f}%" if idx["index_change_pct"] is not None else "n/d"
    return "\n".join([
        f"{fx['name']} ({fx['ticker']})",
        f"Prezzo attuale: {number_format.format_number(fx.get('current_price'), 2)}",
        f"Market cap: {number_format.format_abbreviated(fx['market_cap'])}",
        f"P/E: {number_format.format_number(fx['pe_ratio'], 2)}",
        f"Variazione (D) {fx['ticker']}: {ticker_pct} — {idx['index_ticker']} ({idx['index_name']}): {index_pct}",
    ])


def strip_chart_images(markdown: str) -> str:
    """Sostituisce ogni grafico embedded con una nota testuale — stesso
    contenuto fattuale, senza le immagini. Idempotente su un testo che non
    ne contiene già (no-op)."""
    return _CHART_IMAGE_PATTERN.sub("[grafico candlestick — visibile nella finestra con il documento completo]", markdown)


def _fundamentals_section(fx: dict) -> str:
    market_cap = fx["market_cap"]
    return "\n".join([
        "## Fondamentale",
        f"- Settore: {fx['sector']} / {fx['industry']}",
        f"- Prezzo attuale: {number_format.format_number(fx.get('current_price'), 2)}",
        f"- Market cap: {number_format.format_number(market_cap)} ({number_format.format_abbreviated(market_cap)})",
        f"- P/E: {number_format.format_number(fx['pe_ratio'], 2)}",
    ])


def _charts_section(ohlc_by_tf: dict, ticker: str) -> str:
    images = charts.all_timeframe_charts(lambda t, tf: ohlc_by_tf[tf], ticker)
    lines = ["## Grafici"]
    for tf in TIMEFRAMES:
        lines.append(f"### {tf}")
        if images[tf] is None:
            # Nessun dato per questo timeframe (es. niente intraday H1
            # disponibile per il ticker) — nota "non disponibile" invece di
            # un tag immagine rotto, stessa filosofia di _options_section.
            lines.append("Non disponibile per questo timeframe.")
        else:
            lines.append(f"![{ticker} {tf}]({images[tf]})")
    return "\n".join(lines)


def _options_section(options: list[dict], current_price: float | None) -> str:
    if not options:
        return "## Option chain\n\nNon disponibile per questo ticker."
    # Solo gli strike attorno al prezzo attuale (refinement 2026-07-23: prima
    # mostrava OGNI strike delle prime 3 scadenze, illeggibile) — vedi
    # option_chain.py per i dettagli di accoppiamento/ritaglio.
    rows = option_chain.build_rows(options, current_price)
    expiry_note = f"Scadenza: {options[0]['expiry']}." if options else ""
    return "\n".join([
        "## Option chain",
        "",
        f"{expiry_note} Solo gli strike più vicini al prezzo attuale (fino a 10 sopra e 10 sotto).",
        "",
        option_chain.render_table(rows),
    ])


def _comparative_section(source: DataSource, ticker: str, fx: dict, peer_tickers: list[str], idx: dict) -> str:
    lines = [
        "## Comparative",
        "",
        "### Trend multi-periodo (stesso titolo)",
        "",
        "Ricavi e utili netti riportati per periodo fiscale, dal più vecchio al più recente — mostra se il business sta crescendo o rallentando nel tempo:",
    ]
    for row in fundamentals.multi_period_table(fx):
        revenue = number_format.format_abbreviated(row["revenue"])
        earnings = number_format.format_abbreviated(row["earnings"])
        lines.append(f"- {row['period']}: ricavi {revenue}, utili {earnings}")

    lines.append("")
    lines.append("### Vs peer/settore")
    lines.append("")
    if peer_tickers:
        lines.append(
            "Confronto con i principali titoli dello stesso settore su due multipli: "
            "la market cap (dimensione dell'azienda sul mercato) e il P/E (rapporto "
            "prezzo/utili — quante volte gli utili annui il mercato è disposto a pagare "
            "per il titolo: un valore più alto del settore indica maggiori aspettative "
            "di crescita, o una valutazione più cara):"
        )
        for row in fundamentals.peer_comparison_table(source, ticker, fx, peer_tickers):
            market_cap = number_format.format_abbreviated(row["market_cap"])
            pe_ratio = number_format.format_number(row["pe_ratio"], 2)
            lines.append(f"- {row['ticker']}: market cap {market_cap}, P/E {pe_ratio}")
    else:
        lines.append("Nessun peer mappato per questo settore.")

    ticker_pct = f"{idx['ticker_change_pct']:.2f}%" if idx["ticker_change_pct"] is not None else "n/d"
    index_pct = f"{idx['index_change_pct']:.2f}%" if idx["index_change_pct"] is not None else "n/d"
    lines.append("")
    lines.append(f"### Vs indice ({idx['index_ticker']} — {idx['index_name']})")
    lines.append("")
    lines.append(
        f"Variazione percentuale sul periodo disponibile (timeframe D) per il titolo e "
        f"per l'indice di mercato di riferimento — un valore più alto del titolo indica "
        f"una performance relativa migliore nello stesso periodo:"
    )
    lines.append(f"- {ticker} ({fx['name']}): {ticker_pct}")
    lines.append(f"- {idx['index_ticker']} ({idx['index_name']}): {index_pct}")
    return "\n".join(lines)
