"""Test per screeners/jensen_huang.py — screener 'Jensen Huang' (fornitori
ecosistema AI in ipercrescita, filtro industry, bonus lista NVIDIA_LINKED,
top assoluto ripetibile). Modulo INDIPENDENTE da consumer_usage.py e
goldman_sachs.py per design (Docs/superpowers/specs/
2026-08-17-screener-jensen-huang-design.md §3) — nessun import incrociato,
nessuna fixture condivisa con gli altri test screener."""

from screeners import jensen_huang as jh
from data_sources.fake_source import FakeDataSource


def test_industry_qualifies_exact_labels():
    assert jh.industry_qualifies("Semiconductors")
    assert jh.industry_qualifies("Semiconductor Equipment & Materials")
    assert jh.industry_qualifies("Information Technology Services")
    assert jh.industry_qualifies("Internet Content & Information")


def test_industry_qualifies_normalizes_dash_variants_and_case():
    # yfinance non garantisce il separatore (em dash vs " - ") — spec §2/§9.
    assert jh.industry_qualifies("Software—Infrastructure")
    assert jh.industry_qualifies("Software - Infrastructure")
    assert jh.industry_qualifies("Software—Application")
    assert jh.industry_qualifies("software - application")


def test_industry_qualifies_rejects_non_ai_suppliers_and_none():
    assert not jh.industry_qualifies("Banks - Regional")
    assert not jh.industry_qualifies("Consumer Electronics")
    assert not jh.industry_qualifies(None)


def test_nvidia_bonus_for_linked_and_unlinked_tickers():
    assert jh.nvidia_bonus("CRWV") == 10.0
    assert jh.nvidia_bonus("TSM") == 10.0
    assert jh.nvidia_bonus("XOM") == 0.0


def _cache():
    return {
        "CRWV": {"industry": "Software—Infrastructure", "current_price": 100.0,
                 "ps_ratio": 10.0, "revenue_growth": 2.0,
                 "fifty_two_week_high": 150.0, "target_mean_price": 140.0},
        "SLOW": {"industry": "Semiconductors", "current_price": 90.0,
                 "ps_ratio": 6.0, "revenue_growth": 0.05,
                 "fifty_two_week_high": 100.0, "target_mean_price": 95.0},
        "BANK": {"industry": "Banks - Regional", "current_price": 50.0,
                 "ps_ratio": 2.0, "revenue_growth": 0.10,
                 "fifty_two_week_high": 60.0, "target_mean_price": 55.0},
        "OLDCACHE": {"current_price": 10.0, "ps_ratio": 1.0,
                     "revenue_growth": 0.5, "fifty_two_week_high": 20.0,
                     "target_mean_price": 15.0},  # nessuna chiave industry
        "SHRINK": {"industry": "Semiconductors", "current_price": 40.0,
                   "ps_ratio": 3.0, "revenue_growth": -0.10,
                   "fifty_two_week_high": 80.0, "target_mean_price": 42.0},
    }


def _names():
    return {"CRWV": "CoreWeave", "SLOW": "Slow Semi", "BANK": "Some Bank",
            "SHRINK": "Shrinking Semi"}


def test_build_rows_keeps_only_ai_supplier_industries():
    rows = jh.build_rows(_cache(), _names())
    tickers = {r["ticker"] for r in rows}
    # BANK fuori (industry non AI), OLDCACHE fuori (entry di cache
    # pre-estensione: nessuna chiave industry -> None -> scartata, spec §2).
    assert tickers == {"CRWV", "SLOW", "SHRINK"}


def test_build_rows_computes_psg_pullback_and_target_upside():
    rows = jh.build_rows(_cache(), _names())
    crwv = next(r for r in rows if r["ticker"] == "CRWV")
    assert crwv["metrics"]["psg"] == 10.0 / 2.0
    assert crwv["metrics"]["pullback"] == (150.0 - 100.0) / 150.0
    assert crwv["metrics"]["target_upside"] == (140.0 - 100.0) / 100.0


def test_build_rows_psg_is_none_when_growth_not_positive():
    rows = jh.build_rows(_cache(), _names())
    shrink = next(r for r in rows if r["ticker"] == "SHRINK")
    # Crescita negativa: PSG non definito (None), MAI un valore pessimo
    # inventato — il titolo resta rankabile sulle altre metriche (spec §9).
    assert shrink["metrics"]["psg"] is None
    assert shrink["metrics"]["revenue_growth"] == -0.10


def test_build_rows_flags_nvidia_linked_tickers():
    rows = jh.build_rows(_cache(), _names())
    assert next(r for r in rows if r["ticker"] == "CRWV")["nvidia_linked"] is True
    assert next(r for r in rows if r["ticker"] == "SLOW")["nvidia_linked"] is False


def test_build_rows_pullback_clamped_at_zero_when_price_above_high():
    cache = {"HOT": {"industry": "Semiconductors", "current_price": 110.0,
                     "ps_ratio": 5.0, "revenue_growth": 0.5,
                     "fifty_two_week_high": 100.0, "target_mean_price": 120.0}}
    rows = jh.build_rows(cache, {})
    assert rows[0]["metrics"]["pullback"] == 0.0


def test_score_rows_all_metrics_better_plus_nvidia_bonus():
    cache = {
        # CRWV batte XSEM su TUTTE e 4 le metriche (crescita più alta, PSG
        # più basso, pullback più alto, upside più alto) -> percentile puro
        # 100; è anche in NVIDIA_LINKED -> +10 = 110.0. XSEM: 0 + 0 = 0.0.
        "CRWV": {"industry": "Software—Infrastructure", "current_price": 100.0,
                 "ps_ratio": 10.0, "revenue_growth": 2.0,
                 "fifty_two_week_high": 150.0, "target_mean_price": 140.0},
        "XSEM": {"industry": "Semiconductors", "current_price": 95.0,
                 "ps_ratio": 8.0, "revenue_growth": 0.10,
                 "fifty_two_week_high": 100.0, "target_mean_price": 97.0},
    }
    scored = jh.score_rows(jh.build_rows(cache, {}))
    crwv = next(r for r in scored if r["ticker"] == "CRWV")
    xsem = next(r for r in scored if r["ticker"] == "XSEM")
    assert crwv["score"] == 110.0
    assert xsem["score"] == 0.0


def test_score_rows_excludes_rows_with_fewer_than_two_metrics():
    cache = {
        "ONLY1": {"industry": "Semiconductors", "current_price": None,
                  "ps_ratio": None, "revenue_growth": 0.5,
                  "fifty_two_week_high": None, "target_mean_price": None},
        "FULL": {"industry": "Semiconductors", "current_price": 50.0,
                 "ps_ratio": 5.0, "revenue_growth": 0.3,
                 "fifty_two_week_high": 80.0, "target_mean_price": 60.0},
        "FULL2": {"industry": "Semiconductors", "current_price": 60.0,
                  "ps_ratio": 6.0, "revenue_growth": 0.2,
                  "fifty_two_week_high": 70.0, "target_mean_price": 65.0},
    }
    scored = jh.score_rows(jh.build_rows(cache, {}))
    assert {r["ticker"] for r in scored} == {"FULL", "FULL2"}  # ONLY1: 1 metrica < MIN_METRICS


def test_score_rows_sorted_descending_by_score():
    cache = {
        "CRWV": {"industry": "Software—Infrastructure", "current_price": 100.0,
                 "ps_ratio": 10.0, "revenue_growth": 2.0,
                 "fifty_two_week_high": 150.0, "target_mean_price": 140.0},
        "XSEM": {"industry": "Semiconductors", "current_price": 95.0,
                 "ps_ratio": 8.0, "revenue_growth": 0.10,
                 "fifty_two_week_high": 100.0, "target_mean_price": 97.0},
    }
    scored = jh.score_rows(jh.build_rows(cache, {}))
    assert scored[0]["ticker"] == "CRWV"


def _scored_row(ticker, score):
    return {"ticker": ticker, "name": ticker, "score": score}


def test_select_returns_top_n_deterministic():
    rows = [_scored_row(f"T{i}", 100 - i) for i in range(10)]
    selected = jh.select(rows, 5)
    assert [r["ticker"] for r in selected] == ["T0", "T1", "T2", "T3", "T4"]
    assert jh.select(rows, 5) == jh.select(rows, 5)  # nessun RNG, no esclusione


def test_select_shorter_than_top_n_when_pool_too_small_and_empty_ok():
    assert len(jh.select([_scored_row("A", 10)], 10)) == 1
    assert jh.select([], 10) == []


def test_clamp_top_bounds():
    assert jh.clamp_top(1) == 5
    assert jh.clamp_top(25) == 25
    assert jh.clamp_top(99) == 50


def _selection_row():
    return {
        "ticker": "CRWV", "name": "CoreWeave", "industry": "Software—Infrastructure",
        "nvidia_linked": True, "current_price": 100.0, "ps_ratio": 10.0,
        "target_mean_price": 140.0, "score": 92.5,
        "metrics": {"revenue_growth": 2.0, "psg": 5.0,
                    "pullback": 0.333, "target_upside": 0.4},
    }


def _coverage():
    return {"cached": 40, "universe": 500, "qualified": 12, "stale_used": 3,
            "fetch_stats": {"new": 10, "refreshed": 5, "failed": 1}}


def test_render_document_contains_ticker_score_and_nvidia_badge():
    doc = jh.render_document([_selection_row()], _coverage(), generated_at="17/08/2026 10:00")
    assert "CRWV" in doc
    # number_format usa la virgola decimale italiana: 92.5 -> "92,5".
    assert "92,5" in doc
    assert "Jensen Huang" in doc
    assert "17/08/2026 10:00" in doc
    assert "✓" in doc  # badge Nvidia per CRWV


def test_render_document_empty_selection_says_no_qualified():
    doc = jh.render_document([], _coverage())
    assert "Nessun titolo qualificato" in doc


def test_render_summary_for_ai_contains_metrics_and_nvidia_flag():
    summary = jh.render_summary_for_ai([_selection_row()], _coverage())
    assert "CoreWeave" in summary
    assert "legame Nvidia" in summary
    # _fmt: 2 decimali, virgola italiana -> PSG 5.0 rende "5,00".
    assert "5,00" in summary


def test_render_summary_for_ai_empty_selection_says_no_judgment():
    summary = jh.render_summary_for_ai([], _coverage())
    assert "Non scrivere alcun giudizio" in summary


def test_render_channel_summary_mentions_first_ticker():
    summary = jh.render_channel_summary([_selection_row()], _coverage())
    assert "CoreWeave" in summary
    assert "CRWV" in summary


def test_run_returns_full_contract_shape(tmp_path):
    from datetime import date
    import json
    # Pre-seed la cache con un entry FAKE già fresco: questo test verifica il
    # WIRING di run() (candidates->fetch->build_rows->score_rows->select->
    # render_*), non l'algoritmo di select_candidates (già testato a parte in
    # test_fundamentals_cache.py) -- con cache vuota, il budget di fetch (60)
    # viene sempre esaurito dall'universo indici reale prima di raggiungere un
    # ticker fuori indice come FAKE, quindi il pre-seed evita di dipendere da
    # quell'aritmetica (caratteristica nota e out-of-scope di
    # fundamentals_cache.select_candidates, condivisa con goldman-sachs/
    # consumer-usage). A differenza del pre-seed di goldman-sachs, QUESTO
    # entry deve avere "industry" valorizzato: build_rows() di jensen_huang
    # filtra su industry_qualifies(), un entry senza quella chiave (o None)
    # verrebbe scartato prima ancora di arrivare allo score.
    cache_path = tmp_path / "cache.json"
    fake_entry = {
        "sector": "Technology", "market_cap": 1_000_000_000.0, "ps_ratio": 2.5,
        "revenue_growth": 0.15, "fifty_two_week_high": 140.0, "current_price": 110.0,
        "target_mean_price": 130.0, "peg_ratio": 1.2, "pe_ratio": 22.0,
        "debt_to_equity": 0.6, "dividend_yield": 0.012, "payout_ratio": 0.25,
        "target_high_price": 150.0, "target_low_price": 100.0,
        "industry": "Software—Infrastructure",  # qualifica per industry_qualifies()
        "fetched_at": date.today().isoformat(),
    }
    cache_path.write_text(json.dumps({"FAKE": fake_entry}), encoding="utf-8")

    source = FakeDataSource()
    tickers = [{"ticker": "FAKE", "name": "Fake Corp"}]
    out = jh.run(source, tickers, cache_path, tmp_path / "discoveries.json", top=5)
    assert "summary" in out
    assert "report_markdown" in out
    assert "channel_summary" in out
    assert "title_suffix" in out
    # FAKE ha industry "Software—Infrastructure" (qualifica) e 4 metriche
    # complete -> deve comparire nel report.
    assert "Fake Corp" in out["report_markdown"]


def test_run_ignores_discoveries_path_never_written(tmp_path):
    source = FakeDataSource()
    tickers = [{"ticker": "FAKE", "name": "Fake Corp"}]
    discoveries_path = tmp_path / "discoveries.json"
    jh.run(source, tickers, tmp_path / "cache.json", discoveries_path, top=5)
    assert not discoveries_path.exists()  # nessuna esclusione 30gg per questo screener
