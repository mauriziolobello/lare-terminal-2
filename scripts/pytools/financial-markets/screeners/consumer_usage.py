"""Screener 'consumer-usage' (ex screening.py, migrato in
screeners/2026-08-11 — Docs/superpowers/specs/2026-08-11-markets-screener-
registry-design.md): filtro settori consumer, derivazione metriche, score a
percentili, selezione vincolata (esclusione 30gg, cap settore 40%, quota
esplorativa 70/30), rendering documento/riassunti — tutto PURO: nessun I/O,
nessuna rete, RNG e insieme di esclusione iniettati (vedi
Docs/superpowers/specs/2026-07-26-financial-markets-screen-stocks-design.md
§6-§8 per il design originale di questa parte). L'unica eccezione è `run()`
in fondo al file: orchestrazione I/O (cache, fetch, discoveries), isolata
lì apposta, chiamata da screeners.dispatch()."""

import math
import random

import number_format

CONSUMER_SECTORS = ("Consumer Defensive", "Consumer Cyclical",
                    "Communication Services", "Technology")

# (nome metrica, higher_is_better) — direzioni dalla spec §7.1.
METRICS = (("ps_ratio", False), ("drawdown", True), ("revenue_growth", True),
           ("target_upside", True), ("peg_ratio", False))

MIN_METRICS = 2
SECTOR_CAP_FRAC = 0.4
EXPLORE_FRAC = 0.3
TOP_MIN, TOP_MAX = 5, 50

def clamp_top(top: int) -> int:
    return max(TOP_MIN, min(TOP_MAX, top))


def build_rows(cache: dict[str, dict], names: dict[str, str]) -> list[dict]:
    """Filtra i settori consumer e deriva le metriche. Entry con settore
    null (fetch fallito) o fuori dai settori consumer: scartato qui."""
    rows = []
    for ticker, entry in cache.items():
        if entry.get("sector") not in CONSUMER_SECTORS:
            continue
        high = entry.get("fifty_two_week_high")
        price = entry.get("current_price")
        target = entry.get("target_mean_price")
        drawdown = (1 - price / high) if (high and price and high > 0) else None
        target_upside = ((target - price) / price) if (target is not None and price) else None
        rows.append({
            "ticker": ticker,
            "name": names.get(ticker, ticker),
            "sector": entry["sector"],
            # Non è una metrica di score: serve solo alla colonna "Ultimo"
            # della tabella (refinement post-verifica dal vivo 2026-07-27).
            "current_price": price,
            "metrics": {
                "ps_ratio": entry.get("ps_ratio"),
                "drawdown": drawdown,
                "revenue_growth": entry.get("revenue_growth"),
                "target_upside": target_upside,
                "peg_ratio": entry.get("peg_ratio"),
            },
        })
    return rows


def score_rows(rows: list[dict]) -> list[dict]:
    """Score 0-100 = media dei percentili disponibili (direzione orientata:
    percentile alto = favorevole). Sotto MIN_METRICS metriche: escluso —
    uno score su una metrica sola non è comparabile. Percentile = quota di
    valori PEGGIORI battuti (0-100); con un solo valore presente → 50
    (neutro, nessun confronto possibile)."""
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
        score = round(sum(available.values()) / len(available), 1)
        scored.append({**row, "score": score})
    scored.sort(key=lambda r: (-r["score"], r["name"]))
    return scored


def select(rows: list[dict], top_n: int, excluded: set[str],
           rng: random.Random) -> list[dict]:
    """Selezione vincolata (spec §7.3): (1) esclusione finestra 30gg, (2)
    cap settore 40% dei posti, (3) ~70% merito + ~30% estrazione
    probabilistica pesata per score. Pool insufficiente → selezione più
    corta, mai soglie abbassate."""
    pool = [r for r in rows if r["ticker"] not in excluded]
    n = min(top_n, len(pool))
    if n == 0:
        return []
    cap = math.ceil(SECTOR_CAP_FRAC * n)
    explore_slots = round(EXPLORE_FRAC * n)
    merit_slots = n - explore_slots

    selected: list[dict] = []
    sector_counts: dict[str, int] = {}
    remaining: list[dict] = []
    for row in pool:  # già ordinati per (score desc, nome)
        if len(selected) < merit_slots and sector_counts.get(row["sector"], 0) < cap:
            selected.append({**row, "exploratory": False})
            sector_counts[row["sector"]] = sector_counts.get(row["sector"], 0) + 1
        else:
            remaining.append(row)

    while len(selected) < n:
        eligible = [r for r in remaining if sector_counts.get(r["sector"], 0) < cap]
        if not eligible:
            break
        weights = [max(r["score"], 1.0) for r in eligible]
        pick = rng.choices(eligible, weights=weights, k=1)[0]
        remaining.remove(pick)
        selected.append({**pick, "exploratory": True})
        sector_counts[pick["sector"]] = sector_counts.get(pick["sector"], 0) + 1

    selected.sort(key=lambda r: (-r["score"], r["name"]))
    return selected


def _fmt(value, decimals=2, percent=False):
    if value is None:
        return "n/d"
    if percent:
        return number_format.format_number(value * 100, decimals) + "%"
    return number_format.format_number(value, decimals)


def _sort_header(label: str, sort_type: str) -> str:
    """Header <th> con le due azioni di ordinamento (▲ crescente, ▼
    decrescente): window.js le legge via data-dir/data-type. Nessun
    mapping nome-colonna da mantenere sincrono — l'indice colonna si
    deriva da th.cellIndex al click (vedi table-sort.mjs, Task 2)."""
    return (f'<th>{label} '
            f'<span class="sort-arrow" data-dir="asc" data-type="{sort_type}">▲</span>'
            f'<span class="sort-arrow" data-dir="desc" data-type="{sort_type}">▼</span></th>')


def _cell(display: str, sort_value) -> str:
    """<td> con data-sort-value quando il dato è disponibile (sort_value
    non None) — assente per un dato n/d, cosicché il JS lo tratti come
    sempre-ultimo in qualunque direzione di ordinamento."""
    if sort_value is None:
        return f"<td>{display}</td>"
    return f'<td data-sort-value="{sort_value}">{display}</td>'


_TABLE_HEADER = (
    "<th>Pos</th>"
    + _sort_header("Nome", "text") + _sort_header("Asset", "text")
    + _sort_header("Settore", "text") + _sort_header("Ultimo", "num")
    + _sort_header("Score", "num") + _sort_header("P/S", "num")
    + _sort_header("Drawdown", "num") + _sort_header("Crescita", "num")
    + _sort_header("Upside", "num") + _sort_header("PEG", "num")
)


def _table(selection: list[dict], first_appearance_tickers: set[str]) -> str:
    lines = ["<table>", f"<thead><tr>{_TABLE_HEADER}</tr></thead>", "<tbody>"]
    for pos, row in enumerate(selection, start=1):
        m = row["metrics"]
        star = " ★" if row["ticker"] in first_appearance_tickers else ""
        note = " (esplorativa)" if row.get("exploratory") else ""
        cells = (
            f"<td data-pos>{pos}</td>"
            + _cell(f"{row['name']}{star}{note}", row["name"])
            + _cell(row["ticker"], row["ticker"])
            + _cell(row["sector"], row["sector"])
            + _cell(_fmt(row.get("current_price")), row.get("current_price"))
            + _cell(number_format.format_number(row["score"], 1), row["score"])
            + _cell(_fmt(m.get("ps_ratio")), m.get("ps_ratio"))
            + _cell(_fmt(m.get("drawdown"), percent=True), m.get("drawdown"))
            + _cell(_fmt(m.get("revenue_growth"), percent=True), m.get("revenue_growth"))
            + _cell(_fmt(m.get("target_upside"), percent=True), m.get("target_upside"))
            + _cell(_fmt(m.get("peg_ratio")), m.get("peg_ratio"))
        )
        lines.append(f"<tr>{cells}</tr>")
    lines.append("</tbody></table>")
    return "\n".join(lines)


# Legenda compatta delle colonne — SUBITO sotto la tabella (richiesta
# esplicita dopo la verifica dal vivo): cosa significa ogni colonna, non la
# filosofia della selezione (quella resta in _LEGEND più sotto).
_COLUMN_LEGEND = (
    "Colonne: **Ultimo** = ultimo prezzo (USD); **Score** = punteggio relativo "
    "0-100 (vedi sotto); **P/S** = rapporto prezzo/ricavi; **Drawdown** = "
    "distanza % dal massimo delle ultime 52 settimane; **Crescita** = crescita "
    "dei ricavi anno su anno; **Upside** = distanza % dal target medio degli "
    "analisti; **PEG** = P/E diviso la crescita attesa. n/d = dato non "
    "disponibile."
)


_LEGEND = """## Come leggere questa selezione

Lo score (0-100) è la media dei percentili di 5 metriche DENTRO l'insieme
filtrato di oggi — è un confronto relativo fra queste aziende, non un
giudizio assoluto. Direzioni: P/S basso (ricavi alti ignorati dal prezzo),
drawdown dal massimo 52 settimane alto (momento di ribasso), crescita
ricavi alta, upside sul target degli analisti alto, PEG basso (prezzo
economico rispetto alla crescita). Una metrica mancante viene esclusa
dalla media, mai contata come zero.

Questa NON è la classifica assoluta dei migliori: i titoli mostrati negli
ultimi 30 giorni sono esclusi (ogni esecuzione mostra facce nuove), al
massimo il 40% dei posti va allo stesso settore, e circa il 30% dei posti
è assegnato per estrazione probabilistica pesata fra i qualificati (righe
marcate "(esplorativa)") invece che per puro punteggio — per far emergere
anche i nomi meno ovvi. ★ = prima apparizione assoluta in una selezione."""


def _coverage_lines(coverage: dict) -> str:
    """Blocco '## Copertura' — condiviso da render_document (documento
    Markdown) e render_summary_for_ai (spec §8: l'AI riceve la stessa
    sezione per contestualizzare quanto è giovane/completa la cache)."""
    fs = coverage["fetch_stats"]
    pct = (100 * coverage["cached"] / coverage["universe"]) if coverage["universe"] else 0
    return "\n".join([
        "## Copertura",
        f"- Titoli in cache fondamentali: {coverage['cached']} su {coverage['universe']} dell'universo ({number_format.format_number(pct, 1)}%)",
        f"- di cui aggiornati in questa esecuzione: {fs['new'] + fs['refreshed']} ({fs['new']} nuovi, {fs['refreshed']} refresh, {fs['failed']} falliti)",
        f"- Dopo filtro settori consumer: {coverage['qualified']} titoli qualificati",
        f"- Esclusi perché già mostrati negli ultimi 30 giorni: {coverage['excluded_recent']}",
        f"- Entry con dati più vecchi di 7 giorni usati comunque: {coverage['stale_used']} (in attesa di refresh)",
    ])


def render_document(selection: list[dict], first_appearance_tickers: set[str],
                    coverage: dict, generated_at: str = "") -> str:
    """`generated_at`: data/ora già formattata (es. "27/07/2026 15:42") —
    parametro, mai un orologio letto qui (modulo puro): la fornisce
    server.py. Stringa vuota → titolo senza timestamp (retrocompatibile)."""
    title = "# Screening potenziale — mercato USA"
    if generated_at:
        title += f" — {generated_at}"
    parts = [
        title,
        _coverage_lines(coverage),
        f"## Selezione di oggi ({len(selection)} titoli — ★ = prima apparizione assoluta)\n\n"
        + (_table(selection, first_appearance_tickers) + "\n\n" + _COLUMN_LEGEND if selection
           else "Nessun titolo qualificato oggi (cache giovane o tutti i qualificati mostrati di recente)."),
        _LEGEND,
    ]
    return "\n\n".join(parts)


def render_summary_for_ai(selection: list[dict], coverage: dict) -> str:
    """Cosa riceve l'AI per scrivere il giudizio uso-di-massa: la sezione
    Copertura (contesto: quanto è giovane/completa la cache) + la selezione
    con le metriche — spec §8. Niente graduatoria completa oltre la
    selezione: non serve al giudizio e gonfia il contesto."""
    parts = [_coverage_lines(coverage)]
    if not selection:
        parts.append("Selezione vuota: nessun titolo qualificato oggi. Non scrivere alcun giudizio.")
        return "\n\n".join(parts)
    lines = ["Selezione di oggi (per il tuo giudizio uso-di-massa, titolo per titolo):"]
    for row in selection:
        m = row["metrics"]
        lines.append(
            f"- {row['name']} ({row['ticker']}, {row['sector']}) — score {row['score']}, "
            f"P/S {_fmt(m.get('ps_ratio'))}, drawdown {_fmt(m.get('drawdown'), percent=True)}, "
            f"crescita {_fmt(m.get('revenue_growth'), percent=True)}, "
            f"upside {_fmt(m.get('target_upside'), percent=True)}, PEG {_fmt(m.get('peg_ratio'))}")
    parts.append("\n".join(lines))
    return "\n\n".join(parts)


def render_channel_summary(selection: list[dict], coverage: dict) -> str:
    fs = coverage["fetch_stats"]
    pct = (100 * coverage["cached"] / coverage["universe"]) if coverage["universe"] else 0
    lines = [
        f"Screening: {coverage['cached']} titoli in cache ({number_format.format_number(pct, 1)}% universo), {coverage['qualified']} dopo filtro consumer.",
        f"Fetch di questa esecuzione: {fs['new'] + fs['refreshed']} ({fs['new']} nuovi, {fs['refreshed']} refresh).",
    ]
    if selection:
        first = selection[0]
        lines.append(f"Selezione di {len(selection)} titoli in finestra. Primo: {first['name']} ({first['ticker']}) — score {number_format.format_number(first['score'], 1)}.")
    else:
        lines.append("Nessun titolo qualificato oggi (cache giovane o novità esaurite).")
    return "\n".join(lines)


def run(source, tickers: list[dict], cache_path, discoveries_path, top: int = 25) -> dict:
    """Orchestrazione I/O (cache, fetch, discoveries) + selezione/rendering
    per lo screener 'consumer-usage'. Non PURA (a differenza del resto di
    questo modulo, ex screening.py) — isolata qui in fondo al file perché
    richiede fetch di rete/disco che il resto del modulo volutamente non fa.
    Chiamata da screeners.dispatch() via il registry (screeners/__init__.py).
    `source`/`tickers`/`cache_path`/`discoveries_path` sono iniettati (mai
    letti da uno stato globale) — testabile con FakeDataSource, vedi
    screeners/test_registry.py."""
    import fundamentals_cache
    import discoveries
    import index_membership
    from datetime import date, datetime

    top = clamp_top(top)
    today = date.today()
    rng = random.Random()

    cache = fundamentals_cache.load_cache(cache_path)
    ledger = discoveries.load_ledger(discoveries_path)

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
    already_shown = discoveries.excluded(ledger, today)
    selection = select(scored, top, already_shown, rng)

    first_appearances = {r["ticker"] for r in selection
                         if discoveries.is_first_appearance(ledger, r["ticker"])}
    discoveries.record(ledger, [r["ticker"] for r in selection], today)
    discoveries.save_ledger(discoveries_path, ledger)

    coverage = {
        "cached": len(cache),
        "universe": len(all_tickers),
        "qualified": len(scored),
        "excluded_recent": sum(1 for r in scored if r["ticker"] in already_shown),
        "stale_used": sum(1 for e in cache.values()
                          if not fundamentals_cache.is_fresh(e, today)),
        "fetch_stats": fetch_stats,
    }
    generated_at = datetime.now().strftime("%d/%m/%Y %H:%M")
    return {
        "summary": render_summary_for_ai(selection, coverage),
        "report_markdown": render_document(
            selection, first_appearances, coverage, generated_at=generated_at),
        "channel_summary": render_channel_summary(selection, coverage),
        "title_suffix": f" — {generated_at}",
    }
