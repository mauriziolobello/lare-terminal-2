"""Screener 'goldman-sachs' — report stile equity research, 10 titoli, quota
minima 30% Technology/Communication Services, bonus punteggio per prezzo
sotto 80-100$, nessuna esclusione (top assoluto, può ripetersi run dopo
run). Prompt originale copiato dall'utente, non scritto da lui — vedi
Docs/superpowers/specs/2026-08-11-screener-goldman-sachs-design.md.

INDIPENDENTE da consumer_usage.py per design esplicito del progetto ("ogni
screener userà logiche diverse, altrimenti non avrebbe senso avere screener
diversi") — nessun import da quel modulo, i piccoli helper di formattazione/
tabella sono duplicati qui, non condivisi. Come consumer_usage.py, tutto
PURO tranne `run()` in fondo (I/O: cache, fetch) e `enrich_selection()`
(fetch live sui vincitori, mai in cache — vedi spec §5)."""

import math

import number_format
import peers

PRICE_BONUS_MAX = 8.0     # punti, tetto del bonus — nudge, non fattore dominante
PRICE_BONUS_FULL = 80.0   # $ — prezzo a cui il bonus è pieno
PRICE_BONUS_ZERO = 100.0  # $ — prezzo a cui il bonus si azzera


def price_bonus(price: float | None) -> float:
    """Bonus di punteggio per prezzo/azione accessibile (spec §2.2: preferenza
    morbida, MAI un'esclusione). Pieno sotto/a PRICE_BONUS_FULL, lineare fino
    a zero a PRICE_BONUS_ZERO, zero (mai negativo) sopra — un titolo caro non
    è penalizzato, semplicemente non riceve il bonus."""
    if price is None:
        return 0.0
    if price <= PRICE_BONUS_FULL:
        return PRICE_BONUS_MAX
    if price >= PRICE_BONUS_ZERO:
        return 0.0
    span = PRICE_BONUS_ZERO - PRICE_BONUS_FULL
    return PRICE_BONUS_MAX * (PRICE_BONUS_ZERO - price) / span


# (nome metrica, higher_is_better) — solo 4 (a differenza delle 5 di
# consumer-usage): dividend_yield/payout/target_high/target_low restano
# SOLO display (spec §4) — su un orizzonte di 6 mesi il dividendo è quasi
# irrilevante come segnale di merito, ma va comunque mostrato come dato.
METRICS = (("pe_ratio", False), ("revenue_growth", True),
           ("debt_to_equity", False), ("target_upside", True))

MIN_METRICS = 2
TOP_MIN, TOP_MAX = 5, 50


def clamp_top(top: int) -> int:
    return max(TOP_MIN, min(TOP_MAX, top))


def build_rows(cache: dict[str, dict], names: dict[str, str]) -> list[dict]:
    """Nessun filtro di settore (a differenza di consumer-usage): ogni
    entry con un fetch riuscito (sector non null) qualifica per il
    ranking, qualunque settore."""
    rows = []
    for ticker, entry in cache.items():
        sector = entry.get("sector")
        if sector is None:
            continue
        price = entry.get("current_price")
        target = entry.get("target_mean_price")
        target_upside = ((target - price) / price) if (target is not None and price) else None
        rows.append({
            "ticker": ticker,
            "name": names.get(ticker, ticker),
            "sector": sector,
            "current_price": price,
            "dividend_yield": entry.get("dividend_yield"),
            "payout_ratio": entry.get("payout_ratio"),
            "target_mean_price": target,
            "target_high_price": entry.get("target_high_price"),
            "target_low_price": entry.get("target_low_price"),
            "metrics": {
                "pe_ratio": entry.get("pe_ratio"),
                "revenue_growth": entry.get("revenue_growth"),
                "debt_to_equity": entry.get("debt_to_equity"),
                "target_upside": target_upside,
            },
        })
    return rows


def score_rows(rows: list[dict]) -> list[dict]:
    """Score = media dei percentili disponibili (>= MIN_METRICS su 4) + bonus
    prezzo (price_bonus, spec §4) — stessa matematica a percentili di
    consumer-usage, duplicata qui per indipendenza (non importata)."""
    percentiles: dict[str, dict[str, float]] = {t: {} for t in (r["ticker"] for r in rows)}
    for metric, higher_is_better in METRICS:
        present = [(r["ticker"], r["metrics"][metric]) for r in rows
                   if r["metrics"][metric] is not None]
        n = len(present)
        for ticker, value in present:
            if n == 1:
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
        base = sum(available.values()) / len(available)
        score = round(base + price_bonus(row["current_price"]), 1)
        scored.append({**row, "score": score})
    scored.sort(key=lambda r: (-r["score"], r["name"]))
    return scored


FLOOR_SECTORS = ("Technology", "Communication Services")
FLOOR_FRAC = 0.3


def select(rows: list[dict], top_n: int) -> list[dict]:
    """Selezione deterministica (spec §4/§2.3: top assoluto, può ripetersi —
    a differenza di consumer-usage nessuna esclusione 30gg, nessun RNG qui).
    Riserva fino a FLOOR_FRAC dei posti a FLOOR_SECTORS (pavimento, MAI
    riempito con nomi peggiori se il pool tech è insufficiente), poi
    riempie il resto per punteggio puro su TUTTO il pool residuo — nessun
    tetto: altro tech può comunque entrare se merita."""
    pool = sorted(rows, key=lambda r: (-r["score"], r["name"]))
    n = min(top_n, len(pool))
    if n == 0:
        return []
    floor = math.ceil(FLOOR_FRAC * n)
    tech = [r for r in pool if r["sector"] in FLOOR_SECTORS][:floor]
    tech_tickers = {r["ticker"] for r in tech}
    rest = [r for r in pool if r["ticker"] not in tech_tickers][:n - len(tech)]
    selected = tech + rest
    selected.sort(key=lambda r: (-r["score"], r["name"]))
    return selected


def revenue_trend_label(revenue_by_period: list[tuple[str, float]]) -> str:
    """Descrive l'andamento ricavi dai punti disponibili (tipicamente ~4
    anni fiscali via yfinance, non garantiti 5 — spec §3/§12) confrontando i
    delta fra periodi consecutivi. Meno di 2 punti: nessun trend leggibile."""
    if len(revenue_by_period) < 2:
        return "n/d"
    deltas = [b - a for (_, a), (_, b) in zip(revenue_by_period, revenue_by_period[1:])]
    if all(d > 0 for d in deltas):
        return "crescita costante"
    if all(d < 0 for d in deltas):
        return "in calo"
    return "misto"


def enrich_selection(source, selection: list[dict]) -> None:
    """Arricchisce SOLO i vincitori finali (spec §5) — mai il pool candidato
    intero: due dati costosi (P/E medio di settore, trend ricavi) calcolati
    con fetch LIVE, mai in cache persistente (N piccolo, freschezza preferita
    a un dato stantio). Muta ogni row in place aggiungendo 'sector_avg_pe' e
    'revenue_trend'. Memoizza a livello di singolo ticker peer (non di
    settore): un peer ripetuto fra due vincitori dello stesso settore non
    rifetcha, corretto anche se un vincitore È uno dei peer noti del proprio
    settore (peers.peers_for_sector esclude sempre il ticker del vincitore
    stesso, quindi la lista peer può differire leggermente fra vincitori
    dello stesso settore — la memoizzazione per ticker singolo resta
    corretta in ogni caso, a differenza di una memoizzazione per settore)."""
    fetched_pe: dict[str, float | None] = {}

    def peer_pe(ticker: str) -> float | None:
        if ticker not in fetched_pe:
            try:
                fetched_pe[ticker] = source.fetch_fundamentals(ticker).get("pe_ratio")
            except Exception:
                fetched_pe[ticker] = None
        return fetched_pe[ticker]

    for row in selection:
        peer_tickers = peers.peers_for_sector(row["sector"], row["ticker"])
        pe_values = [v for t in peer_tickers if (v := peer_pe(t)) is not None]
        row["sector_avg_pe"] = (sum(pe_values) / len(pe_values)) if pe_values else None
        try:
            fundamentals = source.fetch_fundamentals(row["ticker"])
            row["revenue_trend"] = revenue_trend_label(fundamentals.get("revenue_by_period", []))
        except Exception:
            row["revenue_trend"] = "n/d"


def _fmt(value, decimals=2, percent=False):
    """Formatta un numero con number_format, aggiungendo il simbolo % se
    richiesto. Restituisce 'n/d' per valori None. number_format.format_number
    usa la virgola decimale italiana: 88.5 con 1 decimale diventa "88,5"."""
    if value is None:
        return "n/d"
    if percent:
        return number_format.format_number(value * 100, decimals) + "%"
    return number_format.format_number(value, decimals)


def _sort_header(label: str, sort_type: str) -> str:
    """Genera una cella di intestazione con frecce direzionali di sort (▲/▼)
    per il frontend (data-dir/data-type per click -> riordina tabella)."""
    return (f'<th>{label} '
            f'<span class="sort-arrow" data-dir="asc" data-type="{sort_type}">▲</span>'
            f'<span class="sort-arrow" data-dir="desc" data-type="{sort_type}">▼</span></th>')


def _cell(display: str, sort_value) -> str:
    """Genera una cella <td> con eventualmente un attributo data-sort-value per
    il sort client-side. Se sort_value è None (es. trend testuale), la cella
    non è ordinabile."""
    if sort_value is None:
        return f"<td>{display}</td>"
    return f'<td data-sort-value="{sort_value}">{display}</td>'


_TABLE_HEADER = (
    "<th>Pos</th>"
    + _sort_header("Nome", "text") + _sort_header("Asset", "text")
    + _sort_header("Settore", "text") + _sort_header("Ultimo", "num")
    + _sort_header("Score", "num") + _sort_header("P/E", "num")
    + _sort_header("P/E settore", "num") + _sort_header("Crescita", "num")
    + _sort_header("Trend ricavi", "text") + _sort_header("D/E", "num")
    + _sort_header("Div. yield", "num") + _sort_header("Payout", "num")
    + _sort_header("Target medio", "num") + _sort_header("Target alto", "num")
    + _sort_header("Target basso", "num")
)


def _table(selection: list[dict]) -> str:
    """Costruisce una tabella HTML con i dati dei titoli selezionati. Ogni
    riga ha celle con data-sort-value per l'ordinamento client-side (tranne
    il trend testuale). Usata da render_document()."""
    lines = ["<table>", f"<thead><tr>{_TABLE_HEADER}</tr></thead>", "<tbody>"]
    for pos, row in enumerate(selection, start=1):
        m = row["metrics"]
        cells = (
            f"<td data-pos>{pos}</td>"
            + _cell(row["name"], row["name"])
            + _cell(row["ticker"], row["ticker"])
            + _cell(row["sector"], row["sector"])
            + _cell(_fmt(row.get("current_price")), row.get("current_price"))
            + _cell(number_format.format_number(row["score"], 1), row["score"])
            + _cell(_fmt(m.get("pe_ratio")), m.get("pe_ratio"))
            + _cell(_fmt(row.get("sector_avg_pe")), row.get("sector_avg_pe"))
            + _cell(_fmt(m.get("revenue_growth"), percent=True), m.get("revenue_growth"))
            + _cell(row.get("revenue_trend", "n/d"), None)
            + _cell(_fmt(m.get("debt_to_equity")), m.get("debt_to_equity"))
            + _cell(_fmt(row.get("dividend_yield"), percent=True), row.get("dividend_yield"))
            + _cell(_fmt(row.get("payout_ratio"), percent=True), row.get("payout_ratio"))
            + _cell(_fmt(row.get("target_mean_price")), row.get("target_mean_price"))
            + _cell(_fmt(row.get("target_high_price")), row.get("target_high_price"))
            + _cell(_fmt(row.get("target_low_price")), row.get("target_low_price"))
        )
        lines.append(f"<tr>{cells}</tr>")
    lines.append("</tbody></table>")
    return "\n".join(lines)


_COLUMN_LEGEND = (
    "Colonne: **Score** = media percentili di P/E, crescita ricavi, D/E, "
    "upside sul target + bonus fino a 8 punti per prezzo sotto 80-100$; "
    "**P/E settore** = media dei peer noti (peers.py, n/d se settore non "
    "mappato); **Trend ricavi** = andamento sugli anni fiscali disponibili "
    "(tipicamente ~4, non garantiti 5); **D/E** = debito/patrimonio; "
    "**Payout** = quota utili distribuita come dividendo. n/d = dato non "
    "disponibile."
)


_LEGEND = """## Come leggere questa selezione

Score = media dei percentili di 4 metriche (P/E, crescita ricavi, D/E,
upside sul target medio analisti) DENTRO il pool candidato di oggi, più un
bonus fino a 8 punti per titoli con prezzo/azione sotto 80-100$ (nessuna
esclusione per i titoli più cari). Almeno il 30% dei posti è riservato a
titoli Technology/Communication Services quando il pool lo permette (mai un
tetto: altro tech può comunque entrare per merito). Questa È la classifica
assoluta dei migliori secondo questo punteggio — a differenza dello
screener 'Uso Consumer', NESSUNA esclusione: la stessa selezione può
ripetersi run dopo run se le condizioni non cambiano."""


def _coverage_lines(coverage: dict) -> str:
    """Genera un blocco Markdown con statistiche di copertura (cache, universo,
    qualificati, fetch di questa esecuzione). Usato da render_document() e
    render_summary_for_ai()."""
    fs = coverage["fetch_stats"]
    pct = (100 * coverage["cached"] / coverage["universe"]) if coverage["universe"] else 0
    return "\n".join([
        "## Copertura",
        f"- Titoli in cache fondamentali: {coverage['cached']} su {coverage['universe']} dell'universo ({number_format.format_number(pct, 1)}%)",
        f"- di cui aggiornati in questa esecuzione: {fs['new'] + fs['refreshed']} ({fs['new']} nuovi, {fs['refreshed']} refresh, {fs['failed']} falliti)",
        f"- Titoli qualificati per il ranking: {coverage['qualified']}",
        f"- Entry con dati più vecchi di 7 giorni usati comunque: {coverage['stale_used']} (in attesa di refresh)",
    ])


def render_document(selection: list[dict], coverage: dict, generated_at: str = "") -> str:
    """Genera un documento Markdown completo per la visualizzazione della
    selezione (tabella HTML + legenda + copertura). Usato dal comando /markets
    per aprire il risultato in una finestra Markdown. Se generated_at non è
    vuoto, viene aggiunto al titolo (formato previsto: "DD/MM/YYYY HH:MM")."""
    title = "# Screening equity research — Goldman Sachs"
    if generated_at:
        title += f" — {generated_at}"
    parts = [
        title,
        _coverage_lines(coverage),
        f"## Selezione di oggi ({len(selection)} titoli — top assoluto, può ripetersi da un'esecuzione all'altra)\n\n"
        + (_table(selection) + "\n\n" + _COLUMN_LEGEND if selection
           else "Nessun titolo qualificato oggi (cache giovane)."),
        _LEGEND,
    ]
    return "\n\n".join(parts)


def render_summary_for_ai(selection: list[dict], coverage: dict) -> str:
    """Genera un sommario testuale della selezione per l'AI (per il turno AI
    dove l'AI scrive un giudizio equity research). Ogni riga è un bullet point
    con tutti i dati rilevanti (score, metriche, trend, target, upside).
    Se la selezione è vuota, avvisa l'AI di non scrivere alcun giudizio."""
    parts = [_coverage_lines(coverage)]
    if not selection:
        parts.append("Selezione vuota: nessun titolo qualificato oggi. Non scrivere alcun giudizio.")
        return "\n\n".join(parts)
    lines = ["Selezione di oggi (per il tuo giudizio equity research, titolo per titolo):"]
    for row in selection:
        m = row["metrics"]
        lines.append(
            f"- {row['name']} ({row['ticker']}, {row['sector']}) — score {_fmt(row['score'], decimals=1)}, "
            f"P/E {_fmt(m.get('pe_ratio'))} (settore {_fmt(row.get('sector_avg_pe'))}), "
            f"crescita ricavi {_fmt(m.get('revenue_growth'), percent=True)}, "
            f"trend {row.get('revenue_trend', 'n/d')}, "
            f"D/E {_fmt(m.get('debt_to_equity'))}, "
            f"dividend yield {_fmt(row.get('dividend_yield'), percent=True)}, "
            f"payout {_fmt(row.get('payout_ratio'), percent=True)}, "
            f"target medio {_fmt(row.get('target_mean_price'))} "
            f"(alto {_fmt(row.get('target_high_price'))}, basso {_fmt(row.get('target_low_price'))}), "
            f"upside {_fmt(m.get('target_upside'), percent=True)}")
    parts.append("\n".join(lines))
    return "\n\n".join(parts)


def render_channel_summary(selection: list[dict], coverage: dict) -> str:
    """Genera una sintesi una riga per il canale (output conciso: percentuale
    cache, titoli qualificati, fetch di questa esecuzione, primo titolo con
    score). Usato da run() per inviare un feedback rapido al cursore/Telegram."""
    fs = coverage["fetch_stats"]
    pct = (100 * coverage["cached"] / coverage["universe"]) if coverage["universe"] else 0
    lines = [
        f"Screening Goldman Sachs: {coverage['cached']} titoli in cache ({number_format.format_number(pct, 1)}% universo), {coverage['qualified']} qualificati.",
        f"Fetch di questa esecuzione: {fs['new'] + fs['refreshed']} ({fs['new']} nuovi, {fs['refreshed']} refresh).",
    ]
    if selection:
        first = selection[0]
        lines.append(f"Selezione di {len(selection)} titoli. Primo: {first['name']} ({first['ticker']}) — score {number_format.format_number(first['score'], 1)}.")
    else:
        lines.append("Nessun titolo qualificato oggi.")
    return "\n".join(lines)


def run(source, tickers: list[dict], cache_path, discoveries_path, top: int = 25) -> dict:
    """Orchestrazione I/O per lo screener 'goldman-sachs'. `discoveries_path`
    è accettato (firma imposta dal registry, screeners/__init__.py) ma MAI
    usato — nessuna esclusione 30gg per questo screener (spec §2.3, decisione
    utente: top assoluto, può ripetersi). `cache_path` è CONDIVISO con
    consumer-usage: stesso file fundamentals_cache.json, stesso universo
    indici — entrambi gli screener contribuiscono e beneficiano della stessa
    cache nel tempo (spec §7), nessuna infrastruttura nuova."""
    import fundamentals_cache
    import index_membership
    import random
    from datetime import date, datetime

    top = clamp_top(top)
    today = date.today()
    rng = random.Random()  # solo per select_candidates (campionamento fetch
                            # fuori-indice) — select() qui è deterministico.

    cache = fundamentals_cache.load_cache(cache_path)

    index_tickers = sorted(
        index_membership.SP500_TICKERS
        | index_membership.DOW_JONES_TICKERS
        | index_membership.NASDAQ100_TICKERS)
    all_tickers = [t["ticker"] for t in tickers]

    candidates = fundamentals_cache.select_candidates(
        index_tickers, all_tickers, cache, today, rng)
    fetch_stats = fundamentals_cache.run_fetch(
        cache, candidates, source.fetch_screening_snapshot, today)
    fundamentals_cache.save_cache(cache_path, cache)

    names = {t["ticker"]: t["name"] for t in tickers}
    rows = build_rows(cache, names)
    scored = score_rows(rows)
    selection = select(scored, top)
    enrich_selection(source, selection)

    coverage = {
        "cached": len(cache),
        "universe": len(all_tickers),
        "qualified": len(scored),
        "stale_used": sum(1 for e in cache.values()
                          if not fundamentals_cache.is_fresh(e, today)),
        "fetch_stats": fetch_stats,
    }
    generated_at = datetime.now().strftime("%d/%m/%Y %H:%M")
    return {
        "summary": render_summary_for_ai(selection, coverage),
        "report_markdown": render_document(selection, coverage, generated_at=generated_at),
        "channel_summary": render_channel_summary(selection, coverage),
        "title_suffix": f" — {generated_at}",
    }
