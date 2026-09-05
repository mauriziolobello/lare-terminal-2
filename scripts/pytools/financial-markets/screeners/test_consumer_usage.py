"""Test per screening.py — direzioni metriche, percentili, rinormalizzazione
su dato mancante, soglia 2 metriche, selezione vincolata (esclusione,
cap settore, 70/30 con seed), rendering. Tutto sintetico, nessun I/O."""

import random

from screeners import consumer_usage as screening


def _entry(sector="Technology", ps=2.0, growth=0.10, high=100.0, price=80.0,
           target=95.0, peg=1.5, fetched="2026-07-27"):
    return {"sector": sector, "market_cap": 1.0, "ps_ratio": ps,
            "revenue_growth": growth, "fifty_two_week_high": high,
            "current_price": price, "target_mean_price": target,
            "peg_ratio": peg, "fetched_at": fetched}


# ── build_rows ─────────────────────────────────────────────────────────────

def test_build_rows_keeps_only_consumer_sectors():
    cache = {"AAA": _entry(sector="Technology"),
             "BBB": _entry(sector="Energy"),
             "CCC": _entry(sector="Consumer Defensive")}
    rows = screening.build_rows(cache, {"AAA": "A", "BBB": "B", "CCC": "C"})
    assert {r["ticker"] for r in rows} == {"AAA", "CCC"}


def test_build_rows_derives_drawdown_and_target_upside():
    cache = {"AAA": _entry(high=100.0, price=80.0, target=100.0)}
    rows = screening.build_rows(cache, {"AAA": "A"})
    m = rows[0]["metrics"]
    assert abs(m["drawdown"] - 0.20) < 1e-9          # 1 - 80/100
    assert abs(m["target_upside"] - 0.25) < 1e-9     # (100-80)/80


def test_build_rows_derived_metrics_none_when_inputs_missing_or_zero():
    entry = _entry()
    entry["fifty_two_week_high"] = None
    entry["current_price"] = 0.0   # divisione per zero → upside None
    cache = {"AAA": entry}
    rows = screening.build_rows(cache, {"AAA": "A"})
    assert rows[0]["metrics"]["drawdown"] is None
    assert rows[0]["metrics"]["target_upside"] is None


def test_build_rows_name_falls_back_to_ticker():
    cache = {"AAA": _entry()}
    rows = screening.build_rows(cache, {})
    assert rows[0]["name"] == "AAA"


def test_build_rows_skips_null_entries_from_failed_fetches():
    cache = {"BAD": {k: None for k in ("sector", "market_cap", "ps_ratio",
             "revenue_growth", "fifty_two_week_high", "current_price",
             "target_mean_price", "peg_ratio")} | {"fetched_at": "2026-07-27"}}
    assert screening.build_rows(cache, {}) == []


# ── score_rows ─────────────────────────────────────────────────────────────

def _row(ticker, sector="Technology", **metrics):
    base = {"ps_ratio": None, "drawdown": None, "revenue_growth": None,
            "target_upside": None, "peg_ratio": None}
    base.update(metrics)
    return {"ticker": ticker, "name": ticker, "sector": sector, "metrics": base}


def test_score_rows_directions_low_ps_beats_high_ps():
    rows = [_row("LOW", ps_ratio=1.0, drawdown=0.1),
            _row("HIGH", ps_ratio=10.0, drawdown=0.1)]
    scored = screening.score_rows(rows)
    low = next(r for r in scored if r["ticker"] == "LOW")
    high = next(r for r in scored if r["ticker"] == "HIGH")
    assert low["score"] > high["score"]


def test_score_rows_directions_high_drawdown_growth_upside_beat_low():
    rows = [_row("UP", drawdown=0.5, revenue_growth=0.3, target_upside=0.4),
            _row("DOWN", drawdown=0.1, revenue_growth=0.0, target_upside=0.0)]
    scored = screening.score_rows(rows)
    up = next(r for r in scored if r["ticker"] == "UP")
    down = next(r for r in scored if r["ticker"] == "DOWN")
    assert up["score"] > down["score"]


def test_score_rows_low_peg_beats_high_peg():
    rows = [_row("CHEAP", peg_ratio=0.8, drawdown=0.1),
            _row("DEAR", peg_ratio=3.0, drawdown=0.1)]
    scored = screening.score_rows(rows)
    cheap = next(r for r in scored if r["ticker"] == "CHEAP")
    dear = next(r for r in scored if r["ticker"] == "DEAR")
    assert cheap["score"] > dear["score"]


def test_score_rows_missing_metric_renormalizes_not_zero():
    # B ha solo 2 metriche, entrambe le migliori del gruppo → score alto,
    # MAI penalizzato dalla mancanza delle altre tre.
    rows = [_row("A", ps_ratio=5.0, drawdown=0.1, revenue_growth=0.1,
                 target_upside=0.1, peg_ratio=2.0),
            _row("B", ps_ratio=1.0, drawdown=0.6)]
    scored = screening.score_rows(rows)
    b = next(r for r in scored if r["ticker"] == "B")
    assert b["score"] == 100.0


def test_score_rows_excludes_rows_with_fewer_than_two_metrics():
    rows = [_row("OK", ps_ratio=1.0, drawdown=0.2),
            _row("THIN", ps_ratio=1.0)]
    scored = screening.score_rows(rows)
    assert {r["ticker"] for r in scored} == {"OK"}


def test_score_rows_sorted_desc_ties_broken_by_name():
    rows = [_row("ZZZ", ps_ratio=1.0, drawdown=0.2),
            _row("AAA", ps_ratio=1.0, drawdown=0.2)]
    scored = screening.score_rows(rows)
    assert [r["ticker"] for r in scored] == ["AAA", "ZZZ"]
    assert scored[0]["score"] == scored[1]["score"]


# ── select ─────────────────────────────────────────────────────────────────

def _scored(ticker, score, sector="Technology"):
    return {"ticker": ticker, "name": ticker, "sector": sector,
            "metrics": {}, "score": score}


def test_select_excludes_recently_shown_tickers():
    rows = [_scored("A", 90), _scored("B", 80), _scored("C", 70)]
    out = screening.select(rows, top_n=2, excluded={"A"}, rng=random.Random(1))
    assert {r["ticker"] for r in out} <= {"B", "C"}
    assert all(r["ticker"] != "A" for r in out)


def test_select_shorter_when_pool_is_small_never_pads():
    rows = [_scored("A", 90)]
    out = screening.select(rows, top_n=25, excluded=set(), rng=random.Random(1))
    assert len(out) == 1


def test_select_sector_cap_40_percent():
    # 10 posti, cap = ceil(0.4*10) = 4: mai più di 4 dello stesso settore.
    rows = ([_scored(f"T{i}", 100 - i, "Technology") for i in range(8)]
            + [_scored(f"C{i}", 50 - i, "Consumer Defensive") for i in range(8)])
    out = screening.select(rows, top_n=10, excluded=set(), rng=random.Random(1))
    tech = [r for r in out if r["sector"] == "Technology"]
    assert len(tech) <= 4


def test_select_marks_exploratory_rows():
    # Settori distribuiti su tutti e 4 i consumer (5 titoli l'uno): il cap
    # 40% (4 su 10 per settore) non interferisce — questo test misura SOLO
    # la ripartizione 70/30, non il cap (coperto dal test precedente).
    sectors = ("Technology", "Consumer Defensive", "Consumer Cyclical", "Communication Services")
    rows = [_scored(f"S{i}", 100 - i, sectors[i % 4]) for i in range(20)]
    out = screening.select(rows, top_n=10, excluded=set(), rng=random.Random(1))
    exploratory = [r for r in out if r["exploratory"]]
    merit = [r for r in out if not r["exploratory"]]
    # 10 posti: 3 esplorativi (round(0.3*10)), 7 di merito.
    assert len(exploratory) == 3
    assert len(merit) == 7


def test_select_is_reproducible_with_same_seed():
    sectors = ("Technology", "Consumer Defensive", "Consumer Cyclical", "Communication Services")
    rows = [_scored(f"S{i}", 100 - i, sectors[i % 4]) for i in range(30)]
    out1 = screening.select(rows, top_n=10, excluded=set(), rng=random.Random(9))
    out2 = screening.select(rows, top_n=10, excluded=set(), rng=random.Random(9))
    assert [r["ticker"] for r in out1] == [r["ticker"] for r in out2]


# ── clamp_top / rendering ──────────────────────────────────────────────────

def test_clamp_top_bounds():
    assert screening.clamp_top(1) == 5
    assert screening.clamp_top(25) == 25
    assert screening.clamp_top(99) == 50


def test_build_rows_carries_current_price_for_the_ultimo_column():
    # Refinement post-verifica dal vivo (2026-07-27): la tabella di
    # selezione mostra anche l'ultimo prezzo — build_rows deve propagarlo
    # nella riga, non solo consumarlo dentro drawdown/upside.
    cache = {"AAA": _entry(price=123.45)}
    rows = screening.build_rows(cache, {"AAA": "A"})
    assert rows[0]["current_price"] == 123.45


def test_render_document_table_has_ultimo_column_and_column_legend():
    sel = [dict(_scored("AAA", 90.0), exploratory=False, current_price=123.45,
                metrics={"ps_ratio": 1.5, "drawdown": 0.25, "revenue_growth": 0.15,
                         "target_upside": 0.10, "peg_ratio": None})]
    coverage = {"cached": 178, "universe": 10400, "qualified": 61,
                "excluded_recent": 12, "stale_used": 5,
                "fetch_stats": {"new": 38, "refreshed": 22, "failed": 0}}
    doc = screening.render_document(sel, first_appearance_tickers=set(), coverage=coverage)
    # Header HTML: Settore precede Ultimo precede Score (ordine colonne).
    assert doc.index("<th>Settore ") < doc.index("<th>Ultimo ") < doc.index("<th>Score ")
    # Il prezzo compare formattato it-IT nella riga.
    assert "123,45" in doc
    # Legenda delle colonne SUBITO dopo la tabella, PRIMA della sezione
    # "Come leggere" (che spiega la filosofia, non le colonne).
    legend_pos = doc.index("Colonne:")
    table_pos = doc.index("<table>")
    philosophy_pos = doc.index("## Come leggere questa selezione")
    assert table_pos < legend_pos < philosophy_pos
    for label in ("Ultimo", "Score", "P/S", "Drawdown", "Crescita", "Upside", "PEG"):
        assert label in doc[legend_pos:philosophy_pos], f"colonna {label} non spiegata nella legenda"


def test_render_document_title_carries_the_generated_at_timestamp():
    # Richiesta esplicita post-verifica dal vivo: data e ora nel titolo del
    # documento — il timestamp è un PARAMETRO (screening.py resta puro,
    # l'orologio vive in server.py come date.today()/rng).
    sel = [dict(_scored("AAA", 90.0), exploratory=False, current_price=100.0,
                metrics={"ps_ratio": 1.5, "drawdown": 0.25, "revenue_growth": 0.15,
                         "target_upside": 0.10, "peg_ratio": 1.0})]
    coverage = {"cached": 1, "universe": 10, "qualified": 1,
                "excluded_recent": 0, "stale_used": 0,
                "fetch_stats": {"new": 1, "refreshed": 0, "failed": 0}}
    doc = screening.render_document(sel, first_appearance_tickers=set(),
                                    coverage=coverage, generated_at="27/07/2026 15:42")
    assert doc.splitlines()[0] == "# Screening potenziale — mercato USA — 27/07/2026 15:42"


def test_render_document_missing_current_price_shows_nd_in_ultimo():
    sel = [dict(_scored("AAA", 90.0), exploratory=False,
                metrics={"ps_ratio": 1.5, "drawdown": 0.25, "revenue_growth": 0.15,
                         "target_upside": 0.10, "peg_ratio": 1.0})]  # niente current_price
    coverage = {"cached": 1, "universe": 10, "qualified": 1,
                "excluded_recent": 0, "stale_used": 0,
                "fetch_stats": {"new": 1, "refreshed": 0, "failed": 0}}
    doc = screening.render_document(sel, first_appearance_tickers=set(), coverage=coverage)
    assert "n/d" in doc


def test_render_document_has_expected_sections_star_and_exploratory_note():
    sel = [dict(_scored("AAA", 90.0), exploratory=False,
                metrics={"ps_ratio": 1.2345, "drawdown": 0.25,
                         "revenue_growth": 0.15, "target_upside": 0.10,
                         "peg_ratio": None}),
           dict(_scored("BBB", 70.0), exploratory=True,
                metrics={"ps_ratio": None, "drawdown": 0.30,
                         "revenue_growth": 0.05, "target_upside": 0.20,
                         "peg_ratio": 2.0})]
    coverage = {"cached": 178, "universe": 10400, "qualified": 61,
                "excluded_recent": 12, "stale_used": 5,
                "fetch_stats": {"new": 38, "refreshed": 22, "failed": 0}}
    doc = screening.render_document(sel, first_appearance_tickers={"AAA"}, coverage=coverage)
    assert "# Screening potenziale — mercato USA" in doc
    assert "## Copertura" in doc
    assert "## Selezione di oggi" in doc
    assert "## Come leggere questa selezione" in doc
    assert "★" in doc                       # prima apparizione di AAA
    assert "(esplorativa)" in doc           # BBB entrata per quota esplorativa
    assert "n/d" in doc                     # metrica mancante mai cella vuota
    assert "178" in doc and "61" in doc


def test_render_channel_summary_is_at_most_five_lines():
    sel = [dict(_scored("AAA", 90.0), exploratory=False, metrics={})]
    coverage = {"cached": 178, "universe": 10400, "qualified": 61,
                "excluded_recent": 12, "stale_used": 5,
                "fetch_stats": {"new": 38, "refreshed": 22, "failed": 0}}
    out = screening.render_channel_summary(sel, coverage)
    assert out.count("\n") <= 4
    assert "AAA" in out


def test_render_channel_summary_empty_selection_is_still_meaningful():
    coverage = {"cached": 5, "universe": 10400, "qualified": 0,
                "excluded_recent": 0, "stale_used": 0,
                "fetch_stats": {"new": 5, "refreshed": 0, "failed": 0}}
    out = screening.render_channel_summary([], coverage)
    assert "0" in out or "nessun" in out.lower()


def test_render_document_empty_selection_renders_fallback_not_crash():
    coverage = {"cached": 5, "universe": 10400, "qualified": 0,
                "excluded_recent": 0, "stale_used": 0,
                "fetch_stats": {"new": 5, "refreshed": 0, "failed": 0}}
    doc = screening.render_document([], first_appearance_tickers=set(), coverage=coverage)
    assert "# Screening potenziale — mercato USA" in doc
    assert "## Copertura" in doc
    assert "Nessun titolo qualificato oggi" in doc
    assert "## Come leggere questa selezione" in doc


# ── render_summary_for_ai ────────────────────────────────────────────────────

def test_render_summary_for_ai_includes_coverage_and_per_row_metrics():
    sel = [dict(_scored("AAA", 90.0), exploratory=False,
                metrics={"ps_ratio": 1.5, "drawdown": 0.25, "revenue_growth": 0.15,
                         "target_upside": 0.10, "peg_ratio": None})]
    coverage = {"cached": 178, "universe": 10400, "qualified": 61,
                "excluded_recent": 12, "stale_used": 5,
                "fetch_stats": {"new": 38, "refreshed": 22, "failed": 0}}
    out = screening.render_summary_for_ai(sel, coverage)
    assert "## Copertura" in out          # spec §8: l'AI riceve anche la copertura
    assert "178" in out
    assert "AAA" in out and "90" in out
    assert "n/d" in out                    # PEG mancante mai cella vuota


def test_render_summary_for_ai_empty_selection_tells_the_ai_not_to_judge():
    coverage = {"cached": 5, "universe": 10400, "qualified": 0,
                "excluded_recent": 0, "stale_used": 0,
                "fetch_stats": {"new": 5, "refreshed": 0, "failed": 0}}
    out = screening.render_summary_for_ai([], coverage)
    assert "## Copertura" in out
    assert "Non scrivere alcun giudizio" in out


# ── ordinamento cliccabile: markup HTML della tabella ───────────────────────

def test_table_header_has_two_sort_arrows_per_sortable_column_none_for_pos():
    sel = [dict(_scored("AAA", 90.0), exploratory=False, current_price=100.0,
                metrics={"ps_ratio": 1.0, "drawdown": 0.1, "revenue_growth": 0.1,
                         "target_upside": 0.1, "peg_ratio": 1.0})]
    coverage = {"cached": 1, "universe": 10, "qualified": 1,
                "excluded_recent": 0, "stale_used": 0,
                "fetch_stats": {"new": 1, "refreshed": 0, "failed": 0}}
    doc = screening.render_document(sel, first_appearance_tickers=set(), coverage=coverage)
    assert doc.count('class="sort-arrow"') == 20  # 10 colonne sortabili * 2 frecce
    assert "<th>Pos</th>" in doc


def test_table_row_carries_raw_sort_value_for_numeric_columns():
    sel = [dict(_scored("AAA", 87.3), exploratory=False, current_price=234.56,
                metrics={"ps_ratio": 28.4, "drawdown": 0.1523, "revenue_growth": 0.081,
                         "target_upside": 0.22, "peg_ratio": None})]
    coverage = {"cached": 1, "universe": 10, "qualified": 1,
                "excluded_recent": 0, "stale_used": 0,
                "fetch_stats": {"new": 1, "refreshed": 0, "failed": 0}}
    doc = screening.render_document(sel, first_appearance_tickers=set(), coverage=coverage)
    assert 'data-sort-value="234.56"' in doc   # Ultimo
    assert 'data-sort-value="87.3"' in doc     # Score
    assert 'data-sort-value="28.4"' in doc     # P/S
    assert 'data-sort-value="0.1523"' in doc   # Drawdown (frazione grezza, non %)


def test_table_row_omits_sort_value_when_metric_is_missing():
    sel = [dict(_scored("AAA", 90.0), exploratory=False, current_price=100.0,
                metrics={"ps_ratio": 1.0, "drawdown": 0.1, "revenue_growth": 0.1,
                         "target_upside": 0.1, "peg_ratio": None})]
    coverage = {"cached": 1, "universe": 10, "qualified": 1,
                "excluded_recent": 0, "stale_used": 0,
                "fetch_stats": {"new": 1, "refreshed": 0, "failed": 0}}
    doc = screening.render_document(sel, first_appearance_tickers=set(), coverage=coverage)
    # PEG è l'ultima colonna: n/d, nessun data-sort-value su quella cella.
    assert "<td>n/d</td>" in doc


def test_table_row_pos_cell_carries_data_pos_marker():
    sel = [dict(_scored("AAA", 90.0), exploratory=False, current_price=100.0,
                metrics={"ps_ratio": 1.0, "drawdown": 0.1, "revenue_growth": 0.1,
                         "target_upside": 0.1, "peg_ratio": 1.0})]
    coverage = {"cached": 1, "universe": 10, "qualified": 1,
                "excluded_recent": 0, "stale_used": 0,
                "fetch_stats": {"new": 1, "refreshed": 0, "failed": 0}}
    doc = screening.render_document(sel, first_appearance_tickers=set(), coverage=coverage)
    assert "<td data-pos>1</td>" in doc


def test_table_name_sort_value_excludes_star_and_exploratory_note():
    sel = [dict(_scored("AAA", 90.0), exploratory=True, current_price=100.0,
                metrics={"ps_ratio": 1.0, "drawdown": 0.1, "revenue_growth": 0.1,
                         "target_upside": 0.1, "peg_ratio": 1.0})]
    coverage = {"cached": 1, "universe": 10, "qualified": 1,
                "excluded_recent": 0, "stale_used": 0,
                "fetch_stats": {"new": 1, "refreshed": 0, "failed": 0}}
    doc = screening.render_document(sel, first_appearance_tickers={"AAA"}, coverage=coverage)
    assert 'data-sort-value="AAA"' in doc           # nome pulito nell'attributo
    assert ">AAA ★ (esplorativa)<" in doc            # ★ e nota SOLO nel testo visibile
