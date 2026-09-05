"""Screener 'citadel' -- analisi tecnica quant-style: doppio regime deciso
dal trend recente di ciascun titolo (momentum se prezzo >= SMA50, altrimenti
mean-reversion), tetto (non quota) di 2 titoli per settore, pattern grafici
ed entry/stop/target lasciati al giudizio narrativo dell'AI. Vedi
Docs/superpowers/specs/2026-08-26-screener-citadel-design.md.

INDIPENDENTE da consumer_usage.py/goldman_sachs.py/jensen_huang.py per
design esplicito del progetto -- nessun import da quei moduli. Due costanti
(_normalize_industry, _AI_SUPPLIER_INDUSTRIES) sono duplicate da
jensen_huang.py con un commento che punta all'originale (Task 9) -- non
importate. Tutto PURO tranne `run()` in fondo (I/O: cache, fetch)."""

import number_format
from datetime import date as _date


def sma(values: list[float], period: int) -> float | None:
    """None se non ci sono abbastanza valori -- mai un valore inventato."""
    if len(values) < period:
        return None
    return sum(values[-period:]) / period


def _ema_series(values: list[float], period: int) -> list[float | None]:
    """EMA allineata indice per indice: None finche' non ci sono almeno
    `period` valori (seed = SMA dei primi `period`), poi ricorsione
    standard (k = 2/(period+1)). Usata sia da rsi() indirettamente (no) sia
    da macd() (Task 3) per costruire le serie EMA12/EMA26/EMA9."""
    if len(values) < period:
        return [None] * len(values)
    k = 2 / (period + 1)
    result: list[float | None] = [None] * (period - 1)
    seed = sum(values[:period]) / period
    result.append(seed)
    prev = seed
    for v in values[period:]:
        prev = v * k + prev * (1 - k)
        result.append(prev)
    return result


def rsi(closes: list[float], period: int = 14) -> float | None:
    """Media semplice di guadagni/perdite sugli ultimi `period` cambi --
    variante NON-Wilder (nessuna libreria TA disponibile nel progetto,
    approssimazione documentata, spec §9 rischio 1). None se non ci sono
    almeno period+1 chiusure."""
    if len(closes) < period + 1:
        return None
    window = closes[-(period + 1):]
    changes = [window[i] - window[i - 1] for i in range(1, len(window))]
    gains = [c for c in changes if c > 0]
    losses = [-c for c in changes if c < 0]
    avg_gain = sum(gains) / period
    avg_loss = sum(losses) / period
    if avg_loss == 0:
        return 100.0
    if avg_gain == 0:
        return 0.0
    rs = avg_gain / avg_loss
    return 100 - (100 / (1 + rs))


def trend_stack(price: float, sma50: float | None, sma100: float | None,
                sma200: float | None) -> int:
    """Conta quante delle 3 condizioni rialziste valgono (0-3): prezzo>SMA50,
    SMA50>SMA100, SMA100>SMA200. Una SMA mancante fa fallire silenziosamente
    quella singola condizione -- mai un'eccezione, mai un valore inventato."""
    count = 0
    if sma50 is not None and price > sma50:
        count += 1
    if sma50 is not None and sma100 is not None and sma50 > sma100:
        count += 1
    if sma100 is not None and sma200 is not None and sma100 > sma200:
        count += 1
    return count


def macd(closes: list[float], fast: int = 12, slow: int = 26,
         signal: int = 9) -> dict | None:
    """MACD standard: EMA(fast) - EMA(slow) = linea; EMA(signal) della
    linea = segnale; istogramma = linea - segnale. None se non ci sono
    abbastanza chiusure per costruire almeno `signal` punti di linea
    valida (serve EMA(slow) piena, poi EMA(signal) sulla linea)."""
    ema_fast = _ema_series(closes, fast)
    ema_slow = _ema_series(closes, slow)
    line_series = [f - s if f is not None and s is not None else None
                   for f, s in zip(ema_fast, ema_slow)]
    valid_line = [v for v in line_series if v is not None]
    if len(valid_line) < signal:
        return None
    signal_series = _ema_series(valid_line, signal)
    return {
        "line": valid_line[-1],
        "signal": signal_series[-1],
        "histogram": valid_line[-1] - signal_series[-1],
    }


def macd_histogram_n_bars_ago(closes: list[float], n: int, fast: int = 12,
                              slow: int = 26, signal: int = 9) -> float | None:
    """Istogramma MACD calcolato usando solo le barre fino a `n` barre fa --
    per il segnale 'miglioramento' del regime mean-reversion (spec §4:
    hist(oggi) - hist(N barre fa) > 0 = svolta precoce)."""
    if n <= 0 or len(closes) <= n:
        return None
    past = macd(closes[:-n], fast, slow, signal)
    return past["histogram"] if past is not None else None


def bollinger(closes: list[float], period: int = 20,
             num_std: float = 2.0) -> dict | None:
    """Bande di Bollinger standard: SMA(period) +/- num_std * stddev(period).
    None se non ci sono abbastanza chiusure."""
    if len(closes) < period:
        return None
    window = closes[-period:]
    mid = sum(window) / period
    variance = sum((c - mid) ** 2 for c in window) / period
    std = variance ** 0.5
    return {"lower": mid - num_std * std, "mid": mid, "upper": mid + num_std * std}


def percent_b(closes: list[float], period: int = 20,
             num_std: float = 2.0) -> float | None:
    """Posizione del prezzo nel canale: 0 = banda bassa, 1 = banda alta.
    None se le bande hanno larghezza zero (prezzo piatto). La flatness si
    rileva sulla finestra di INPUT (min==max), non sul width derivato: un
    controllo `width == 0` sul float calcolato non e' affidabile su prezzi
    reali con centesimi perche' l'arrotondamento della somma nella varianza
    crea rumore (~1e-13 o zero a caso) -- alcuni prezzi sfuggono, altri no.
    Rilevare min==max sull'input e' preciso e senza scale arbitrarie."""
    bands = bollinger(closes, period, num_std)
    if bands is None:
        return None
    window = closes[-period:]
    if min(window) == max(window):
        return None
    width = bands["upper"] - bands["lower"]
    return (closes[-1] - bands["lower"]) / width


def volume_ratio(volumes: list[float], period: int = 20) -> float | None:
    """Volume dell'ultimo giorno diviso la media degli ultimi `period`
    giorni (ultimo giorno incluso nella media -- lieve bias di
    autoinclusione, accettato/documentato, spec §9 rischio 2)."""
    if len(volumes) < period:
        return None
    window = volumes[-period:]
    avg = sum(window) / period
    if avg == 0:
        return None
    return volumes[-1] / avg


def six_month_high_low(bars: list[dict]) -> dict | None:
    """Massimo/minimo sulla finestra EFFETTIVAMENTE restituita da
    fetch_ohlc(ticker, "D") dalla fonte dati attiva -- oggi ~126 barre
    (~6 mesi di borsa) sia da yfinance_source.py sia da ibkr_source.py
    (verificato in design review, 2026-08-27). NON un vero anno di borsa:
    la funzione si chiamava `fifty_two_week_high_low` per un design
    originale che assumeva una cache a ~300 barre, mai vera in produzione
    -- rinominata per riflettere onestamente cosa misura davvero. Se ci
    sono meno barre disponibili, usa tutte quelle che ci sono (mai None
    per 'poche barre', solo per bars vuoto). Nome e soglia (126) andrebbero
    rivisti se una fonte dati cambiasse la finestra restituita da
    fetch_ohlc."""
    if not bars:
        return None
    window = bars[-126:] if len(bars) >= 126 else bars
    return {"high": max(b["high"] for b in window), "low": min(b["low"] for b in window)}


def swing_high_low(bars: list[dict], lookback: int = 63) -> dict | None:
    """Massimo/minimo sulla finestra di lookback (default ~3 mesi di borsa)
    -- base per i livelli di Fibonacci (spec §3). Volutamente PIU' CORTA
    della finestra ~6 mesi di six_month_high_low: con la cache reale che
    non supera mai ~126 barre, un default qui uguale (126) renderebbe le
    due finestre identiche e i livelli di Fibonacci ridondanti col range a
    6 mesi -- 63 barre le mantiene un dato distinto."""
    if not bars:
        return None
    window = bars[-lookback:] if len(bars) >= lookback else bars
    return {"high": max(b["high"] for b in window), "low": min(b["low"] for b in window)}


def fibonacci_levels(swing_low: float, swing_high: float) -> dict[str, float]:
    """5 livelli standard di ritracciamento fra lo swing low e lo swing
    high -- dato oggettivo per il giudizio dell'AI (Task 10/spec §6), MAI
    usato nello score (spec §3)."""
    span = swing_high - swing_low
    return {
        "23.6%": swing_high - 0.236 * span,
        "38.2%": swing_high - 0.382 * span,
        "50.0%": swing_high - 0.5 * span,
        "61.8%": swing_high - 0.618 * span,
        "78.6%": swing_high - 0.786 * span,
    }


def _parse_date(raw: str) -> "_date":
    return _date.fromisoformat(raw[:10])


def _group_last_close(bars: list[dict], key_fn) -> list[float]:
    """Chiusura dell'ULTIMA barra di ogni gruppo (settimana ISO o mese),
    nell'ordine di apparizione -- resampling LOCALE (nessuna chiamata di
    rete aggiuntiva, spec §3): la stessa serie daily gia' in cache basta
    per il campo narrativo multi-timeframe."""
    groups: dict = {}
    order: list = []
    for bar in bars:
        key = key_fn(_parse_date(bar["date"]))
        if key not in groups:
            order.append(key)
        groups[key] = bar["close"]
    return [groups[k] for k in order]


def weekly_trend(bars: list[dict], sma_period: int = 10) -> str | None:
    """Chiusura settimanale vs SMA(sma_period) delle chiusure settimanali."""
    closes = _group_last_close(bars, lambda d: d.isocalendar()[:2])
    avg = sma(closes, sma_period)
    if avg is None:
        return None
    if closes[-1] > avg:
        return "up"
    if closes[-1] < avg:
        return "down"
    return "flat"


def monthly_trend(bars: list[dict], sma_period: int = 6) -> str | None:
    """Chiusura mensile vs SMA(sma_period) delle chiusure mensili."""
    closes = _group_last_close(bars, lambda d: (d.year, d.month))
    avg = sma(closes, sma_period)
    if avg is None:
        return None
    if closes[-1] > avg:
        return "up"
    if closes[-1] < avg:
        return "down"
    return "flat"


MIN_BARS_FOR_REGIME = 50


def _classify_regime(price: float, sma50: float | None) -> str | None:
    """None se SMA50 non calcolabile (< 50 barre) -- il chiamante esclude
    la riga, mai un regime indovinato."""
    if sma50 is None:
        return None
    return "momentum" if price >= sma50 else "reversion"


def build_rows(technical_cache: dict, industry_by_ticker: dict[str, str | None],
               names: dict[str, str]) -> list[dict]:
    """Una riga per ticker con OHLC sufficiente (>= MIN_BARS_FOR_REGIME
    barre) e industry nota (letta da fundamentals_cache.json, popolato da
    un altro screener -- None esclude, mai indovinato, spec §7). Calcola
    tutti gli indicatori e classifica il regime; popola SOLO le 5 metriche
    del proprio regime in `metrics` -- l'altro gruppo di metriche resta
    assente dalla riga (mai un valore inventato per il regime non
    applicabile, spec §3/§4)."""
    rows = []
    for ticker, entry in technical_cache.items():
        bars = entry.get("bars") or []
        if len(bars) < MIN_BARS_FOR_REGIME:
            continue
        industry = industry_by_ticker.get(ticker)
        if industry is None:
            continue
        closes = [b["close"] for b in bars]
        volumes = [b["volume"] for b in bars]
        price = closes[-1]
        s50, s100, s200 = sma(closes, 50), sma(closes, 100), sma(closes, 200)
        regime = _classify_regime(price, s50)
        if regime is None:
            continue
        macd_now = macd(closes)
        pb = percent_b(closes)
        vr = volume_ratio(volumes)
        rsi_now = rsi(closes)
        six_month = six_month_high_low(bars)
        swing = swing_high_low(bars)
        fib = fibonacci_levels(swing["low"], swing["high"]) if swing else None
        macd_histogram = macd_now["histogram"] if macd_now else None
        row = {
            "ticker": ticker,
            "name": names.get(ticker, ticker),
            "industry": industry,
            "regime": regime,
            "current_price": price,
            "rsi": rsi_now,
            "macd_histogram": macd_histogram,
            "percent_b": pb,
            "volume_ratio": vr,
            # Con la finestra reale di ~126 barre, sma(closes, 200) e' SEMPRE
            # None (servirebbero >=200 barre) -- la terza condizione di
            # trend_stack (SMA100 > SMA200) non scatta mai in pratica, lo
            # score di trend stack tocca al massimo 2/3. Atteso, non un bug:
            # da rivedere solo se la finestra dati crescesse a 200+ barre.
            "trend_stack": trend_stack(price, s50, s100, s200),
            "six_month_high": six_month["high"] if six_month else None,
            "six_month_low": six_month["low"] if six_month else None,
            "swing_high": swing["high"] if swing else None,
            "swing_low": swing["low"] if swing else None,
            "fibonacci": fib,
            "weekly_trend": weekly_trend(bars),
            "monthly_trend": monthly_trend(bars),
        }
        if regime == "momentum":
            row["metrics"] = {
                "trend_stack": float(row["trend_stack"]),
                "macd_histogram": macd_histogram,
                "percent_b": pb,
                "volume_ratio": vr,
                "rsi_health": (-abs(rsi_now - 60) if rsi_now is not None else None),
            }
        else:
            high = row["six_month_high"]
            # Sconto (%) dal massimo ~6 mesi (six_month_high_low, NON un
            # vero massimo a 52 settimane -- vedi docstring) -- piu' e'
            # profondo, piu' alto il punteggio nel regime mean-reversion.
            pullback = ((high - price) / high) if high else None
            macd_improving = None
            hist_10_ago = macd_histogram_n_bars_ago(closes, 10)
            if macd_histogram is not None and hist_10_ago is not None:
                macd_improving = macd_histogram - hist_10_ago
            row["metrics"] = {
                "pullback": pullback,
                "rsi_inverted": (-rsi_now if rsi_now is not None else None),
                "percent_b_inverted": (-pb if pb is not None else None),
                "volume_ratio": vr,
                "macd_improving": macd_improving,
            }
        rows.append(row)
    return rows


# (nome metrica, higher_is_better) -- due set separati per regime (spec §4).
MOMENTUM_METRICS = (("trend_stack", True), ("macd_histogram", True),
                    ("percent_b", True), ("volume_ratio", True),
                    ("rsi_health", True))
REVERSION_METRICS = (("pullback", True), ("rsi_inverted", True),
                     ("percent_b_inverted", True), ("volume_ratio", True),
                     ("macd_improving", True))

MIN_METRICS = 3
# Guardia gruppo piccolo (spec §4, trovato in review post-approvazione):
# sotto questa soglia una metrica vale 50.0 neutro per tutti invece di un
# percentile pieno 0-100, che gonfierebbe artificialmente il migliore di
# un gruppo piccolo.
MIN_GROUP_FOR_PERCENTILE = 10


def _percentile_score_group(rows: list[dict], metrics: tuple) -> list[dict]:
    """Percentili SOLO dentro questo gruppo (mai contro l'altro regime)."""
    percentiles: dict[str, dict[str, float]] = {r["ticker"]: {} for r in rows}
    for metric, higher_is_better in metrics:
        present = [(r["ticker"], r["metrics"][metric]) for r in rows
                   if r["metrics"].get(metric) is not None]
        n = len(present)
        if n == 0:
            continue
        for ticker, value in present:
            if n < MIN_GROUP_FOR_PERCENTILE:
                percentiles[ticker][metric] = 50.0
                continue
            if higher_is_better:
                beaten = sum(1 for _, other in present if other < value)
            else:
                beaten = sum(1 for _, other in present if other > value)
            percentiles[ticker][metric] = 100.0 * beaten / (n - 1)

    scored = []
    for row in rows:
        available = percentiles[row["ticker"]]
        if len(available) < MIN_METRICS:
            continue
        score = round(sum(available.values()) / len(available), 1)
        scored.append({**row, "score": score})
    return scored


def score_rows(rows: list[dict]) -> list[dict]:
    """Due gruppi separati per regime, percentili SOLO dentro il proprio
    gruppo, poi UNIONE in un pool ordinato per score (spec §4) -- mai
    percentili calcolati sul pool intero (mescolerebbe scale diverse)."""
    momentum_rows = [r for r in rows if r["regime"] == "momentum"]
    reversion_rows = [r for r in rows if r["regime"] == "reversion"]
    scored = (_percentile_score_group(momentum_rows, MOMENTUM_METRICS)
              + _percentile_score_group(reversion_rows, REVERSION_METRICS))
    scored.sort(key=lambda r: (-r["score"], r["name"]))
    return scored


# Duplicato da jensen_huang.py (industry_qualifies/_AI_SUPPLIER_INDUSTRIES)
# -- indipendenza fra screener per design (nessun import incrociato). Se
# aggiorni questa lista, aggiorna ANCHE l'originale in jensen_huang.py
# (spec §7): le due copie non si tengono in sync da sole.
_AI_SUPPLIER_INDUSTRIES = frozenset({
    "semiconductors",
    "semiconductor equipment & materials",
    "software-infrastructure",
    "software-application",
    "information technology services",
    "internet content & information",
})


def _normalize_industry(industry: str) -> str:
    """Minuscole + em/en dash -> trattino + spazi attorno al trattino
    rimossi (duplicato da jensen_huang.py, vedi commento sopra)."""
    lowered = industry.lower().replace("—", "-").replace("–", "-")
    while " -" in lowered or "- " in lowered:
        lowered = lowered.replace(" -", "-").replace("- ", "-")
    return lowered.strip()


def _is_ai_supplier(industry: str) -> bool:
    return _normalize_industry(industry) in _AI_SUPPLIER_INDUSTRIES


def _candidate_buckets(row: dict) -> list[str]:
    """Bucket candidati per una riga: la propria industry normalizzata,
    piu' 'AI' se l'industry e' un fornitore AI (spec §5) -- un titolo puo'
    contare per entrambi, select() decide quale in base ai posti liberi."""
    industry_bucket = _normalize_industry(row["industry"])
    if _is_ai_supplier(row["industry"]):
        return ["AI", industry_bucket]
    return [industry_bucket]


TOP_MIN, TOP_MAX = 4, 30


def clamp_top(top: int) -> int:
    return max(TOP_MIN, min(TOP_MAX, top))


def select(rows: list[dict], top_n: int = 10) -> list[dict]:
    """Tetto <=2/bucket scorrendo il pool gia' ordinato per score (spec §5)
    -- NON una quota: se il tetto impedisce di arrivare a top_n, backfill
    dal resto del pool ignorando il tetto. A parita' di posti liberi tra
    due bucket candidati, vince 'AI' (tie-break deterministico)."""
    bucket_counts: dict[str, int] = {}
    selected = []
    remaining = []
    for row in rows:
        candidates = _candidate_buckets(row)
        target = min(candidates, key=lambda b: (bucket_counts.get(b, 0), 0 if b == "AI" else 1))
        if bucket_counts.get(target, 0) < 2:
            selected.append({**row, "bucket": target})
            bucket_counts[target] = bucket_counts.get(target, 0) + 1
        else:
            remaining.append(row)
        if len(selected) == top_n:
            return selected
    for row in remaining:
        if len(selected) == top_n:
            break
        selected.append({**row, "bucket": _candidate_buckets(row)[0], "backfilled": True})
    return selected


def _fmt(value, decimals=2, percent=False):
    """Formatta un numero con il separatore locale e decimali controllati.
    Restituisce 'n/d' per valori None. Se percent=True, moltiplica per 100
    e aggiunge il simbolo %."""
    if value is None:
        return "n/d"
    if percent:
        return number_format.format_number(value * 100, decimals) + "%"
    return number_format.format_number(value, decimals)


def _sort_header(label: str, sort_type: str) -> str:
    """Cella intestazione con frecce ascendente/discendente per il sorting
    JavaScript lato client -- data-dir indica la direzione, data-type il
    tipo di valore (text/num per ordinamento corretto)."""
    return (f'<th>{label} '
            f'<span class="sort-arrow" data-dir="asc" data-type="{sort_type}">▲</span>'
            f'<span class="sort-arrow" data-dir="desc" data-type="{sort_type}">▼</span></th>')


def _cell(display: str, sort_value) -> str:
    """Cella <td> con valore di sort opzionale -- data-sort-value tiene il
    numero/testo vero per il sorting JavaScript, <content> e' la
    visualizzazione (eventualmente formattata)."""
    if sort_value is None:
        return f"<td>{display}</td>"
    return f'<td data-sort-value="{sort_value}">{display}</td>'


_TABLE_HEADER = (
    "<th>Pos</th>"
    + _sort_header("Nome", "text") + _sort_header("Asset", "text")
    + _sort_header("Settore", "text") + _sort_header("Regime", "text")
    + _sort_header("Ultimo", "num") + _sort_header("Score", "num")
    + _sort_header("RSI", "num") + _sort_header("MACD hist", "num")
    + _sort_header("%B", "num") + _sort_header("Volume ratio", "num")
    + _sort_header("Trend sett./mens.", "text")
)


def _table(selection: list[dict]) -> str:
    """Tabella HTML con una riga per titolo selezionato -- colonne: posizione,
    nome, ticker, settore, regime, ultimo prezzo, score, 5 metriche, trend
    multi-timeframe. Ogni cella di dato contiene data-sort-value per
    permettere ordinamento client-side."""
    lines = ["<table>", f"<thead><tr>{_TABLE_HEADER}</tr></thead>", "<tbody>"]
    for pos, row in enumerate(selection, start=1):
        regime_label = "Momentum" if row["regime"] == "momentum" else "Reversion"
        trend_label = f"{row.get('weekly_trend') or 'n/d'} / {row.get('monthly_trend') or 'n/d'}"
        cells = (
            f"<td data-pos>{pos}</td>"
            + _cell(row["name"], row["name"])
            + _cell(row["ticker"], row["ticker"])
            + _cell(row["bucket"], row["bucket"])
            + _cell(regime_label, regime_label)
            + _cell(_fmt(row.get("current_price")), row.get("current_price"))
            + _cell(number_format.format_number(row["score"], 1), row["score"])
            + _cell(_fmt(row.get("rsi")), row.get("rsi"))
            + _cell(_fmt(row.get("macd_histogram")), row.get("macd_histogram"))
            + _cell(_fmt(row.get("percent_b")), row.get("percent_b"))
            + _cell(_fmt(row.get("volume_ratio")), row.get("volume_ratio"))
            + _cell(trend_label, trend_label)
        )
        lines.append(f"<tr>{cells}</tr>")
    lines.append("</tbody></table>")
    return "\n".join(lines)


_COLUMN_LEGEND = (
    "Colonne: **Score** = media percentili delle 5 metriche del proprio "
    "regime (Momentum: trend stack/MACD/%B/volume/RSI-salute; Reversion: "
    "pullback/RSI-invertito/%B-invertito/volume/MACD-in-miglioramento), "
    "calcolate DENTRO il proprio gruppo, mai contro l'altro regime; "
    "**Regime** = Momentum (prezzo >= SMA50, segue la forza) o Reversion "
    "(prezzo < SMA50, compra il ribasso); **Settore** = industry yfinance "
    "grezza o 'AI' (fornitori infrastruttura AI, lista curata); max 2 "
    "titoli per settore salvo backfill se il tetto non basta a "
    "raggiungere la selezione richiesta. n/d = dato non disponibile."
)


_LEGEND = """## Come leggere questa selezione

Idea dello screener: analisi tecnica quant-style, doppio regime deciso dal
trend recente di ciascun titolo -- chi e' sopra la propria media mobile a
50 giorni viene giudicato sulla FORZA (momentum: trend stack, MACD,
posizione nelle bande di Bollinger, volume, RSI in zona sana), chi e'
sotto viene giudicato sullo SCONTO (mean-reversion: profondita' del
pullback dal massimo ~6 mesi, ipervenduto RSI/%B, volume di
capitolazione, MACD in miglioramento). I due punteggi sono percentili
calcolati dentro il proprio gruppo, poi uniti in un solo pool ordinato.
Diversificazione: max 2 titoli per settore (industry yfinance grezza, o
'AI' per i fornitori di infrastruttura AI) -- un LIMITE superiore, non un
obbligo: se i punteggi migliori si concentrano in pochi settori, alcuni
bucket possono restare scoperti. Nessuna esclusione: la stessa selezione
puo' ripetersi run dopo run. Il giudizio qualitativo (pattern grafici,
zona di ingresso, stop-loss, target) e' scritto dall'AI, non da questo
modulo -- vedi la sezione finale."""


def _coverage_lines(coverage: dict) -> str:
    """Righe di testo Markdown sulla copertura dell'universo: quanti titoli
    in cache, percentuale, quanti aggiornati oggi, quanti qualificati,
    quanti con dati vecchi pero' usati comunque."""
    fs = coverage["fetch_stats"]
    pct = (100 * coverage["cached"] / coverage["universe"]) if coverage["universe"] else 0
    return "\n".join([
        "## Copertura",
        f"- Titoli con serie storica in cache: {coverage['cached']} su {coverage['universe']} dell'universo ({number_format.format_number(pct, 1)}%)",
        f"- di cui aggiornati in questa esecuzione: {fs['new'] + fs['refreshed']} ({fs['new']} nuovi, {fs['refreshed']} refresh, {fs['failed']} falliti)",
        f"- Titoli qualificati per il ranking (>= 50 barre, industry nota, >= 3/5 metriche): {coverage['qualified']}",
        f"- Entry con dati piu' vecchi di 1 giorno di borsa usati comunque: {coverage['stale_used']} (in attesa di refresh)",
    ])


def render_document(selection: list[dict], coverage: dict, generated_at: str = "") -> str:
    """Documento Markdown completo della selezione: titolo, copertura,
    tabella (o messaggio 'nessun titolo'), legenda di lettura. Consumato
    da run() e pubblicato in finestre UI/Telegram."""
    title = "# Screening tecnico quant — Citadel"
    if generated_at:
        title += f" — {generated_at}"
    parts = [
        title,
        _coverage_lines(coverage),
        f"## Selezione di oggi ({len(selection)} titoli — max 2 per settore, può ripetersi da un'esecuzione all'altra)\n\n"
        + (_table(selection) + "\n\n" + _COLUMN_LEGEND if selection
           else "Nessun titolo qualificato oggi (cache giovane o industry non ancora nota — vedi legenda)."),
        _LEGEND,
    ]
    return "\n\n".join(parts)


def render_summary_for_ai(selection: list[dict], coverage: dict) -> str:
    """Riassunto della selezione per il prompt narrativo dell'AI (Task 11):
    dati di copertura, poi una riga per titolo con tutti gli indicatori
    tecnici leggibili dall'AI (in forma verbale, non tabellare). Se
    selection è vuota, dice all'AI di non scrivere giudizi."""
    parts = [_coverage_lines(coverage)]
    if not selection:
        parts.append("Selezione vuota: nessun titolo qualificato oggi. Non scrivere alcun giudizio.")
        return "\n\n".join(parts)
    lines = ["Selezione di oggi (per il tuo giudizio tecnico, titolo per titolo):"]
    for row in selection:
        regime_label = "Momentum (segue la forza)" if row["regime"] == "momentum" else "Mean-reversion (compra il ribasso)"
        fib = row.get("fibonacci") or {}
        fib_txt = ", ".join(f"{k} {number_format.format_number(v, 2)}" for k, v in fib.items())
        lines.append(
            f"- {row['name']} ({row['ticker']}, settore {row['bucket']}) — score {row['score']}, "
            f"regime: {regime_label}, prezzo {_fmt(row.get('current_price'))}, "
            f"RSI {_fmt(row.get('rsi'))}, MACD hist {_fmt(row.get('macd_histogram'))}, "
            f"%B {_fmt(row.get('percent_b'))}, volume ratio {_fmt(row.get('volume_ratio'))}, "
            f"trend settimanale {row.get('weekly_trend') or 'n/d'}, "
            f"trend mensile {row.get('monthly_trend') or 'n/d'}, "
            f"massimo/minimo ~6 mesi {_fmt(row.get('six_month_high'))}/{_fmt(row.get('six_month_low'))}, "
            f"swing ~3 mesi {_fmt(row.get('swing_low'))}-{_fmt(row.get('swing_high'))}, "
            f"livelli Fibonacci: {fib_txt or 'n/d'}")
    parts.append("\n".join(lines))
    return "\n\n".join(parts)


def render_channel_summary(selection: list[dict], coverage: dict) -> str:
    """Una riga per Telegram/UI cursore con uno snapshot della selezione:
    numero di titoli in cache, percentuale universo, numero qualificati,
    fetch di oggi, e il primo titolo con i dati base."""
    fs = coverage["fetch_stats"]
    pct = (100 * coverage["cached"] / coverage["universe"]) if coverage["universe"] else 0
    lines = [
        f"Screening Citadel: {coverage['cached']} titoli con serie storica ({number_format.format_number(pct, 1)}% universo), {coverage['qualified']} qualificati.",
        f"Fetch di questa esecuzione: {fs['new'] + fs['refreshed']} ({fs['new']} nuovi, {fs['refreshed']} refresh).",
    ]
    if selection:
        first = selection[0]
        lines.append(f"Selezione di {len(selection)} titoli. Primo: {first['name']} ({first['ticker']}) — score {number_format.format_number(first['score'], 1)}, regime {first['regime']}.")
    else:
        lines.append("Nessun titolo qualificato oggi.")
    return "\n".join(lines)


def run(source, tickers: list[dict], cache_path, discoveries_path, top: int = 10) -> dict:
    """Orchestrazione I/O per lo screener 'citadel'. `discoveries_path' e'
    accettato (firma imposta dal registry, screeners/__init__.py) ma MAI
    usato -- nessuna esclusione 30gg (spec §5). `cache_path' e' SEMPRE il
    path di fundamentals_cache.json (stesso ricevuto da ogni screener via
    server.py) -- letto QUI in SOLA LETTURA solo per il campo 'industry'
    (spec §7): citadel non fa fondamentali, non lo scrive mai. La propria
    cache OHLC vive in un file SIBLING derivato (mai passato da fuori)."""
    import fundamentals_cache
    import index_membership
    import random
    import technical_cache
    from datetime import date, datetime

    top = clamp_top(top)
    today = date.today()
    rng = random.Random()

    technical_cache_path = cache_path.parent / "technical_cache.json"
    tech_cache = technical_cache.load_cache(technical_cache_path)

    index_tickers = sorted(
        index_membership.SP500_TICKERS
        | index_membership.DOW_JONES_TICKERS
        | index_membership.NASDAQ100_TICKERS)
    all_tickers = [t["ticker"] for t in tickers]

    candidates = technical_cache.select_candidates(
        index_tickers, all_tickers, tech_cache, today, rng)
    fetch_stats = technical_cache.run_fetch(
        tech_cache, candidates, lambda t: source.fetch_ohlc(t, "D"), today)
    technical_cache.save_cache(technical_cache_path, tech_cache)

    fundamentals = fundamentals_cache.load_cache(cache_path)
    industry_by_ticker = {t: e.get("industry") for t, e in fundamentals.items()}
    names = {t["ticker"]: t["name"] for t in tickers}

    rows = build_rows(tech_cache, industry_by_ticker, names)
    scored = score_rows(rows)
    selection = select(scored, top)

    coverage = {
        "cached": len(tech_cache),
        "universe": len(all_tickers),
        "qualified": len(scored),
        "stale_used": sum(1 for e in tech_cache.values()
                          if not technical_cache.is_fresh(e, today)),
        "fetch_stats": fetch_stats,
    }
    generated_at = datetime.now().strftime("%d/%m/%Y %H:%M")
    return {
        "summary": render_summary_for_ai(selection, coverage),
        "report_markdown": render_document(selection, coverage, generated_at=generated_at),
        "channel_summary": render_channel_summary(selection, coverage),
        "title_suffix": f" — {generated_at}",
    }
