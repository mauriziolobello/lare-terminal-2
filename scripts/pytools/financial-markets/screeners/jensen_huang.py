"""Screener 'jensen-huang' — reverse-engineering dei pick di Jensen Huang
(CEO Nvidia): fornitori infrastruttura AI (filtro industry) in ipercrescita,
valutazione compressa RELATIVA alla crescita (PSG), sconto dal massimo 52
settimane, consenso analisti; bonus +10 per i ticker della lista curata
NVIDIA_LINKED. Nessuna esclusione recente (top assoluto, può ripetersi run
dopo run), NESSUN enrichment live post-selezione. Vedi
Docs/superpowers/specs/2026-08-17-screener-jensen-huang-design.md.

INDIPENDENTE da consumer_usage.py e goldman_sachs.py per design esplicito
del progetto ("ogni screener userà logiche diverse") — nessun import da quei
moduli, gli helper di formattazione/tabella e la matematica a percentili
sono duplicati qui, non condivisi. Tutto PURO tranne `run()` in fondo
(I/O: cache, fetch)."""

import number_format

# Lista CURATA A MANO dei ticker con legame Nvidia documentato (investimenti
# Nvidia, citazioni nei keynote di Jensen — fonte: ricerca 2026-08-16, thread
# X "The Assembly" + notizie 2025/2026). Ultimo aggiornamento: 2026-08-17.
# Candidati futuri da verificare su 13F: ARM, RXRX. Si aggiorna editando
# questa costante — nessun fetch automatico per design (decisione utente:
# opzione 2, spec §1/§4).
NVIDIA_LINKED = frozenset({"NBIS", "APLD", "CRWV", "IREN", "NOW", "TSM", "MU"})

NVIDIA_BONUS = 10.0  # punti su scala 0-100 — spinta moderata, decisa dall'utente

# Industry "fornitori infrastruttura AI" (spec §2) — confronto in forma
# NORMALIZZATA (minuscole, dash unificati): yfinance non garantisce il
# separatore ("Software—Application" em dash vs "Software - Application"),
# rischio aperto spec §9 da verificare nello smoke test.
_AI_SUPPLIER_INDUSTRIES = frozenset({
    "semiconductors",
    "semiconductor equipment & materials",
    "software-infrastructure",
    "software-application",
    "information technology services",
    "internet content & information",
})


def _normalize_industry(industry: str) -> str:
    """Minuscole + em/en dash → trattino + spazi attorno al trattino rimossi:
    'Software—Application', 'Software - Application' e 'software-application'
    collassano tutte sulla stessa chiave."""
    lowered = industry.lower().replace("—", "-").replace("–", "-")
    while " -" in lowered or "- " in lowered:
        lowered = lowered.replace(" -", "-").replace("- ", "-")
    return lowered.strip()


def industry_qualifies(industry: str | None) -> bool:
    """True se l'industry è un fornitore di infrastruttura AI (spec §2).
    None (fetch fallito, fonte senza industry, entry di cache pre-estensione
    2026-08-17) → False: mai indovinare."""
    if industry is None:
        return False
    return _normalize_industry(industry) in _AI_SUPPLIER_INDUSTRIES


def nvidia_bonus(ticker: str) -> float:
    """Bonus additivo per legame Nvidia documentato (spec §4). Mai un filtro,
    mai un'esclusione: un ticker fuori lista semplicemente non lo riceve."""
    return NVIDIA_BONUS if ticker in NVIDIA_LINKED else 0.0


def build_rows(cache: dict[str, dict], names: dict[str, str]) -> list[dict]:
    """Filtro sull'INDUSTRY (granularità più fine del sector usato da
    consumer-usage): solo fornitori di infrastruttura AI (spec §2). Le entry
    senza industry (fetch fallito, fonte che non lo fornisce, cache scritta
    prima dell'estensione 2026-08-17) sono scartate — il pool cresce run
    dopo run man mano che il refresh a 7 giorni rinnova la cache."""
    rows = []
    for ticker, entry in cache.items():
        industry = entry.get("industry")
        if not industry_qualifies(industry):
            continue
        price = entry.get("current_price")
        ps_ratio = entry.get("ps_ratio")
        growth = entry.get("revenue_growth")
        high = entry.get("fifty_two_week_high")
        target = entry.get("target_mean_price")
        # PSG = P/S diviso crescita ricavi — il "PEG delle società senza
        # utili" (spec §3): definito solo con crescita POSITIVA, altrimenti
        # None (mai un valore pessimo inventato).
        psg = (ps_ratio / growth) if (ps_ratio is not None and growth is not None and growth > 0) else None
        # Pullback = sconto dal massimo 52 settimane; clamp a >= 0 (un
        # titolo sopra il massimo intraday non deve produrre uno sconto
        # negativo che ne ALZEREBBE il percentile "higher is better").
        pullback = max(0.0, (high - price) / high) if (high is not None and high > 0 and price is not None) else None
        target_upside = ((target - price) / price) if (target is not None and price) else None
        rows.append({
            "ticker": ticker,
            "name": names.get(ticker, ticker),
            "industry": industry,
            "nvidia_linked": ticker in NVIDIA_LINKED,
            "current_price": price,
            "ps_ratio": ps_ratio,
            "target_mean_price": target,
            "metrics": {
                "revenue_growth": growth,
                "psg": psg,
                "pullback": pullback,
                "target_upside": target_upside,
            },
        })
    return rows


# (nome metrica, higher_is_better) — set proprio di questo screener (spec §3):
# ipercrescita, valutazione compressa RELATIVA alla crescita (PSG), sconto dal
# massimo, consenso analisti. Capex intensity esclusa per design (richiederebbe
# un fetch cashflow, romperebbe il vincolo "una chiamata .info" — spec §3).
METRICS = (("revenue_growth", True), ("psg", False),
           ("pullback", True), ("target_upside", True))

MIN_METRICS = 2
TOP_MIN, TOP_MAX = 5, 50


def clamp_top(top: int) -> int:
    return max(TOP_MIN, min(TOP_MAX, top))


def score_rows(rows: list[dict]) -> list[dict]:
    """Score = media dei percentili disponibili (>= MIN_METRICS su 4) + bonus
    Nvidia (nvidia_bonus, spec §4) — stessa matematica a percentili degli
    altri screener, duplicata qui per indipendenza (non importata)."""
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
        score = round(base + nvidia_bonus(row["ticker"]), 1)
        scored.append({**row, "score": score})
    scored.sort(key=lambda r: (-r["score"], r["name"]))
    return scored


def select(rows: list[dict], top_n: int) -> list[dict]:
    """Selezione deterministica: top-N puro per punteggio (spec §5) — nessuna
    quota di settore (a differenza di goldman-sachs), nessuna esclusione 30gg
    (top assoluto, può ripetersi), nessun RNG."""
    pool = sorted(rows, key=lambda r: (-r["score"], r["name"]))
    return pool[:max(0, top_n)]


def _fmt(value, decimals=2, percent=False):
    """Formattazione comune per le celle numeriche: None -> 'n/d' (mai un
    errore né uno zero inventato); percent=True moltiplica per 100 e aggiunge
    '%' (le metriche come revenue_growth/pullback/target_upside sono frazioni,
    es. 0.15 -> '15,00%')."""
    if value is None:
        return "n/d"
    if percent:
        return number_format.format_number(value * 100, decimals) + "%"
    return number_format.format_number(value, decimals)


def _sort_header(label: str, sort_type: str) -> str:
    """Intestazione <th> con le due frecce di ordinamento (asc/desc) che il
    frontend della tabella (JS) legge da data-dir/data-type — stesso pattern
    duplicato negli altri screener (indipendenza voluta, non un import)."""
    return (f'<th>{label} '
            f'<span class="sort-arrow" data-dir="asc" data-type="{sort_type}">▲</span>'
            f'<span class="sort-arrow" data-dir="desc" data-type="{sort_type}">▼</span></th>')


def _cell(display: str, sort_value) -> str:
    """<td> con l'attributo data-sort-value (il valore GREZZO usato dal JS
    per ordinare, distinto dal testo formattato mostrato all'utente).
    sort_value None -> nessun attributo (colonna non ordinabile su quel dato,
    es. n/d): il frontend degrada a ordinamento testuale su quella cella."""
    if sort_value is None:
        return f"<td>{display}</td>"
    return f'<td data-sort-value="{sort_value}">{display}</td>'


# Ordine colonne fisso della tabella (spec): Pos, Nome, Asset, Industria,
# Nvidia, Ultimo, Score, Crescita, P/S, PSG, Pullback 52wk, Target medio,
# Upside. "Pos" non è ordinabile (posizione di ranking, non un dato del
# titolo) quindi non passa da _sort_header.
_TABLE_HEADER = (
    "<th>Pos</th>"
    + _sort_header("Nome", "text") + _sort_header("Asset", "text")
    + _sort_header("Industria", "text") + _sort_header("Nvidia", "num")
    + _sort_header("Ultimo", "num") + _sort_header("Score", "num")
    + _sort_header("Crescita", "num") + _sort_header("P/S", "num")
    + _sort_header("PSG", "num") + _sort_header("Pullback 52wk", "num")
    + _sort_header("Target medio", "num") + _sort_header("Upside", "num")
)


def _table(selection: list[dict]) -> str:
    """Corpo HTML della tabella di selezione — una riga per titolo, nell'ordine
    già deciso da select() (score decrescente). "Pos" è la posizione 1-based
    nel ranking (non un dato di riga, ricalcolata qui con enumerate)."""
    lines = ["<table>", f"<thead><tr>{_TABLE_HEADER}</tr></thead>", "<tbody>"]
    for pos, row in enumerate(selection, start=1):
        m = row["metrics"]
        nvidia = "✓" if row["nvidia_linked"] else "—"
        cells = (
            f"<td data-pos>{pos}</td>"
            + _cell(row["name"], row["name"])
            + _cell(row["ticker"], row["ticker"])
            + _cell(row["industry"], row["industry"])
            + _cell(nvidia, 1 if row["nvidia_linked"] else 0)
            + _cell(_fmt(row.get("current_price")), row.get("current_price"))
            + _cell(number_format.format_number(row["score"], 1), row["score"])
            + _cell(_fmt(m.get("revenue_growth"), percent=True), m.get("revenue_growth"))
            + _cell(_fmt(row.get("ps_ratio")), row.get("ps_ratio"))
            + _cell(_fmt(m.get("psg")), m.get("psg"))
            + _cell(_fmt(m.get("pullback"), percent=True), m.get("pullback"))
            + _cell(_fmt(row.get("target_mean_price")), row.get("target_mean_price"))
            + _cell(_fmt(m.get("target_upside"), percent=True), m.get("target_upside"))
        )
        lines.append(f"<tr>{cells}</tr>")
    lines.append("</tbody></table>")
    return "\n".join(lines)


_COLUMN_LEGEND = (
    "Colonne: **Score** = media percentili di crescita ricavi, PSG, pullback "
    "dal massimo 52 settimane e upside sul target medio analisti, + bonus 10 "
    "punti per legame Nvidia documentato; **Nvidia** = ✓ se il ticker è nella "
    "lista curata NVIDIA_LINKED (aggiornata a mano, ultima revisione "
    "2026-08-17); **PSG** = P/S diviso crescita ricavi (il 'PEG delle società "
    "senza utili' — più basso è, più la valutazione è compressa rispetto alla "
    "crescita; n/d se la crescita non è positiva); **Pullback 52wk** = sconto "
    "dal massimo 52 settimane. n/d = dato non disponibile."
)


_LEGEND = """## Come leggere questa selezione

Idea dello screener: reverse-engineering dei pick di Jensen Huang (CEO
Nvidia) — le aziende in cui Nvidia investe o che Jensen cita nei keynote
tendono a essere fornitori di infrastruttura AI (foundry, memoria HBM,
neocloud, software agentic) in ipercrescita con valutazione compressa
RELATIVA alla crescita. Universo: solo industry fornitori-AI
(semiconduttori, semiconductor equipment, software infrastructure/
application, IT services, internet content). Score = media dei percentili
di 4 metriche DENTRO il pool di oggi + bonus 10 punti per i ticker della
lista curata NVIDIA_LINKED (mai un filtro: un titolo fuori lista può
comunque vincere per metriche). Nessuna esclusione: la stessa selezione può
ripetersi run dopo run se le condizioni non cambiano. Nota: le entry di
cache più vecchie dell'estensione 'industry' (2026-08-17) non qualificano
finché non vengono rinfrescate — il pool cresce nelle prime esecuzioni."""


def _coverage_lines(coverage: dict) -> str:
    """Sezione '## Copertura' — quanta parte dell'universo è in cache, quanto
    aggiornato in QUESTA esecuzione, quanti titoli hanno superato il filtro
    industry + soglia metriche minime, quante entry stale sono state usate
    comunque (in attesa del prossimo refresh a 7 giorni)."""
    fs = coverage["fetch_stats"]
    pct = (100 * coverage["cached"] / coverage["universe"]) if coverage["universe"] else 0
    return "\n".join([
        "## Copertura",
        f"- Titoli in cache fondamentali: {coverage['cached']} su {coverage['universe']} dell'universo ({number_format.format_number(pct, 1)}%)",
        f"- di cui aggiornati in questa esecuzione: {fs['new'] + fs['refreshed']} ({fs['new']} nuovi, {fs['refreshed']} refresh, {fs['failed']} falliti)",
        f"- Titoli qualificati per il ranking (industry AI-supplier, >= 2 metriche): {coverage['qualified']}",
        f"- Entry con dati più vecchi di 7 giorni usati comunque: {coverage['stale_used']} (in attesa di refresh)",
    ])


def render_document(selection: list[dict], coverage: dict, generated_at: str = "") -> str:
    """Documento Markdown completo (per la finestra/Library): titolo +
    copertura + tabella di selezione (o messaggio "nessun qualificato") +
    legenda colonne + legenda concettuale dello screener."""
    title = "# Screening ecosistema AI — Jensen Huang"
    if generated_at:
        title += f" — {generated_at}"
    parts = [
        title,
        _coverage_lines(coverage),
        f"## Selezione di oggi ({len(selection)} titoli — top assoluto, può ripetersi da un'esecuzione all'altra)\n\n"
        + (_table(selection) + "\n\n" + _COLUMN_LEGEND if selection
           else "Nessun titolo qualificato oggi (cache giovane o entry senza industry — vedi legenda)."),
        _LEGEND,
    ]
    return "\n\n".join(parts)


def render_summary_for_ai(selection: list[dict], coverage: dict) -> str:
    """Versione testuale (non tabellare) pensata per il contesto dell'AI —
    una riga per titolo con tutte le metriche, così l'AI può scrivere il
    proprio giudizio (spec: judgment_instructions in screeners/__init__.py)
    senza dover interpretare HTML."""
    parts = [_coverage_lines(coverage)]
    if not selection:
        parts.append("Selezione vuota: nessun titolo qualificato oggi. Non scrivere alcun giudizio.")
        return "\n\n".join(parts)
    lines = ["Selezione di oggi (per il tuo giudizio sull'ecosistema AI, titolo per titolo):"]
    for row in selection:
        m = row["metrics"]
        nvidia = "legame Nvidia: SÌ (lista curata)" if row["nvidia_linked"] else "legame Nvidia: non in lista"
        lines.append(
            f"- {row['name']} ({row['ticker']}, {row['industry']}) — score {row['score']}, "
            f"{nvidia}, "
            f"crescita ricavi {_fmt(m.get('revenue_growth'), percent=True)}, "
            f"P/S {_fmt(row.get('ps_ratio'))}, PSG {_fmt(m.get('psg'))}, "
            f"pullback 52wk {_fmt(m.get('pullback'), percent=True)}, "
            f"target medio {_fmt(row.get('target_mean_price'))}, "
            f"upside {_fmt(m.get('target_upside'), percent=True)}")
    parts.append("\n".join(lines))
    return "\n\n".join(parts)


def render_channel_summary(selection: list[dict], coverage: dict) -> str:
    """Riassunto breve per canali a spazio limitato (es. Telegram): copertura
    in 2 righe + il primo titolo della selezione (se presente)."""
    fs = coverage["fetch_stats"]
    pct = (100 * coverage["cached"] / coverage["universe"]) if coverage["universe"] else 0
    lines = [
        f"Screening Jensen Huang: {coverage['cached']} titoli in cache ({number_format.format_number(pct, 1)}% universo), {coverage['qualified']} qualificati.",
        f"Fetch di questa esecuzione: {fs['new'] + fs['refreshed']} ({fs['new']} nuovi, {fs['refreshed']} refresh).",
    ]
    if selection:
        first = selection[0]
        lines.append(f"Selezione di {len(selection)} titoli. Primo: {first['name']} ({first['ticker']}) — score {number_format.format_number(first['score'], 1)}.")
    else:
        lines.append("Nessun titolo qualificato oggi.")
    return "\n".join(lines)


def run(source, tickers: list[dict], cache_path, discoveries_path, top: int = 25) -> dict:
    """Orchestrazione I/O per lo screener 'jensen-huang'. `discoveries_path`
    è accettato (firma imposta dal registry, screeners/__init__.py) ma MAI
    usato — nessuna esclusione 30gg (spec §5: top assoluto, può ripetersi).
    `cache_path` è CONDIVISO con gli altri screener: stesso file
    fundamentals_cache.json, stesso universo indici (spec §7). NESSUN
    enrichment live post-selezione (spec §5) — a differenza di
    goldman-sachs, il documento si chiude coi dati della cache."""
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
