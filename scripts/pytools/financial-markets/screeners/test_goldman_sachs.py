"""Test per screeners/goldman_sachs.py — screener 'Goldman Sachs' (equity
research, quota minima 30% tech, bonus prezzo, top assoluto ripetibile).
Modulo INDIPENDENTE da consumer_usage.py per design (Docs/superpowers/specs/
2026-08-11-screener-goldman-sachs-design.md §4) — nessun import incrociato,
nessuna fixture condivisa con test_consumer_usage.py."""

from data_sources.fake_source import FakeDataSource
from screeners import goldman_sachs as gs


def test_price_bonus_full_at_or_below_80():
    assert gs.price_bonus(80.0) == 8.0
    assert gs.price_bonus(50.0) == 8.0
    assert gs.price_bonus(0.0) == 8.0


def test_price_bonus_zero_at_or_above_100():
    assert gs.price_bonus(100.0) == 0.0
    assert gs.price_bonus(250.0) == 0.0


def test_price_bonus_linear_between_80_and_100():
    assert gs.price_bonus(90.0) == 4.0  # metà strada = metà bonus


def test_price_bonus_none_price_is_zero():
    assert gs.price_bonus(None) == 0.0


def _cache():
    return {
        "AAA": {"sector": "Technology", "current_price": 50.0, "pe_ratio": 15.0,
                "revenue_growth": 0.20, "debt_to_equity": 0.3, "target_mean_price": 60.0,
                "dividend_yield": None, "payout_ratio": None,
                "target_high_price": 70.0, "target_low_price": 45.0},
        "BBB": {"sector": "Energy", "current_price": 200.0, "pe_ratio": 30.0,
                "revenue_growth": 0.05, "debt_to_equity": 0.9, "target_mean_price": 190.0,
                "dividend_yield": 0.03, "payout_ratio": 0.4,
                "target_high_price": 220.0, "target_low_price": 170.0},
        "NOSECTOR": {"sector": None, "current_price": 10.0, "pe_ratio": 5.0,
                     "revenue_growth": 0.1, "debt_to_equity": 0.1, "target_mean_price": 12.0,
                     "dividend_yield": None, "payout_ratio": None,
                     "target_high_price": None, "target_low_price": None},
    }


def _names():
    return {"AAA": "Alpha Corp", "BBB": "Beta Energy"}


def test_build_rows_skips_entries_with_null_sector():
    rows = gs.build_rows(_cache(), _names())
    tickers = {r["ticker"] for r in rows}
    assert tickers == {"AAA", "BBB"}  # NOSECTOR fuori: fetch senza settore


def test_build_rows_computes_target_upside():
    rows = gs.build_rows(_cache(), _names())
    aaa = next(r for r in rows if r["ticker"] == "AAA")
    assert aaa["metrics"]["target_upside"] == (60.0 - 50.0) / 50.0


def test_build_rows_no_sector_filter_any_sector_qualifies():
    rows = gs.build_rows(_cache(), _names())
    sectors = {r["sector"] for r in rows}
    assert sectors == {"Technology", "Energy"}  # nessun filtro consumer-style


def test_score_rows_lower_pe_and_debt_scores_higher_before_price_bonus():
    rows = gs.build_rows(_cache(), _names())
    scored = gs.score_rows(rows)
    aaa = next(r for r in scored if r["ticker"] == "AAA")
    bbb = next(r for r in scored if r["ticker"] == "BBB")
    # AAA: P/E più basso, crescita più alta, D/E più basso, upside più alto
    # di BBB su tutte e 4 le metriche -> percentile puro 100 + bonus prezzo
    # pieno (50$ <= 80$) = 108.0; BBB: percentile puro 0 + bonus 0 (200$) = 0.0.
    assert aaa["score"] == 108.0
    assert bbb["score"] == 0.0


def test_score_rows_sorted_descending_by_score():
    rows = gs.build_rows(_cache(), _names())
    scored = gs.score_rows(rows)
    assert scored[0]["ticker"] == "AAA"


def test_score_rows_excludes_rows_with_fewer_than_two_metrics():
    rows = [
        {"ticker": "ONE", "name": "One Corp", "sector": "Technology", "current_price": 50.0,
         "metrics": {"pe_ratio": 10.0, "revenue_growth": None, "debt_to_equity": None, "target_upside": None}},
        {"ticker": "TWO", "name": "Two Corp", "sector": "Technology", "current_price": 50.0,
         "metrics": {"pe_ratio": 10.0, "revenue_growth": 0.1, "debt_to_equity": None, "target_upside": None}},
    ]
    scored = gs.score_rows(rows)
    tickers = {r["ticker"] for r in scored}
    assert tickers == {"TWO"}  # ONE ha solo 1 metrica presente, sotto MIN_METRICS=2


def test_score_rows_single_candidate_gets_neutral_percentile_not_zero():
    # Unico ticker nella lista: per ogni metrica presente n=1 -> percentile
    # neutro 50.0 (mai 0), non "penalizzato" per mancanza di confronto.
    rows = [
        {"ticker": "SOLO", "name": "Solo Corp", "sector": "Technology", "current_price": 90.0,
         "metrics": {"pe_ratio": 10.0, "revenue_growth": 0.1, "debt_to_equity": None, "target_upside": None}},
    ]
    scored = gs.score_rows(rows)
    # 2 metriche presenti, entrambe n=1 -> percentile 50.0 ciascuna, media 50.0,
    # + price_bonus(90.0) = 4.0 (lineare fra 80 e 100) -> 54.0
    assert scored[0]["score"] == 54.0


def test_score_rows_ties_broken_by_name():
    rows = [
        {"ticker": "ZZZ", "name": "Zeta Corp", "sector": "Technology", "current_price": 50.0,
         "metrics": {"pe_ratio": 10.0, "revenue_growth": 0.1, "debt_to_equity": 0.2, "target_upside": 0.1}},
        {"ticker": "AAA", "name": "Alpha Corp", "sector": "Technology", "current_price": 50.0,
         "metrics": {"pe_ratio": 10.0, "revenue_growth": 0.1, "debt_to_equity": 0.2, "target_upside": 0.1}},
    ]
    scored = gs.score_rows(rows)
    assert scored[0]["score"] == scored[1]["score"]  # dati identici -> stesso punteggio
    assert scored[0]["name"] == "Alpha Corp"  # parità di punteggio -> ordine alfabetico


def _scored_row(ticker, sector, score):
    return {"ticker": ticker, "name": ticker, "sector": sector, "score": score}


def test_select_reserves_floor_seats_for_tech_when_available():
    # 10 posti, floor = ceil(0.3*10) = 3. 5 tech (punteggio alto ma non i
    # primi 3 assoluti), 5 non-tech con punteggio più alto: il floor deve
    # comunque garantire >= 3 tech anche se non sono i migliori assoluti.
    rows = (
        [_scored_row(f"T{i}", "Technology", 50 - i) for i in range(5)]
        + [_scored_row(f"N{i}", "Energy", 90 - i) for i in range(5)]
    )
    selected = gs.select(rows, 10)
    tech_count = sum(1 for r in selected if r["sector"] == "Technology")
    assert tech_count >= 3
    assert len(selected) == 10


def test_select_floor_shorter_when_tech_pool_insufficient():
    # Solo 1 titolo tech disponibile in tutto il pool: il floor (3) non può
    # essere raggiunto, mai riempito con nomi peggiori per compensare.
    rows = (
        [_scored_row("T0", "Technology", 80)]
        + [_scored_row(f"N{i}", "Energy", 70 - i) for i in range(9)]
    )
    selected = gs.select(rows, 10)
    tech_count = sum(1 for r in selected if r["sector"] == "Technology")
    assert tech_count == 1
    assert len(selected) == 10


def test_select_no_cap_more_tech_can_fill_remaining_seats_by_score():
    # 8 titoli tech con punteggio altissimo, 2 non-tech bassi: nessun tetto,
    # tutti gli 8 tech possono entrare se meritano per punteggio.
    rows = (
        [_scored_row(f"T{i}", "Technology", 100 - i) for i in range(8)]
        + [_scored_row(f"N{i}", "Energy", 10 - i) for i in range(2)]
    )
    selected = gs.select(rows, 10)
    tech_count = sum(1 for r in selected if r["sector"] == "Technology")
    assert tech_count == 8


def test_select_deterministic_same_input_same_output():
    rows = [_scored_row(f"T{i}", "Technology", 100 - i) for i in range(10)]
    assert gs.select(rows, 5) == gs.select(rows, 5)  # nessun RNG, no esclusione


def test_select_shorter_than_top_n_when_pool_too_small():
    rows = [_scored_row("A", "Technology", 10)]
    selected = gs.select(rows, 10)
    assert len(selected) == 1


def test_select_empty_pool_returns_empty():
    assert gs.select([], 10) == []


def test_select_floor_displaces_higher_scoring_non_tech_when_capacity_scarce():
    # 3 tech a punteggio basso, 7 non-tech a punteggio alto, ma top_n=5 < 10
    # righe totali: qui il floor DEVE spostare non-tech più meritevoli per
    # far posto al tech, a differenza dei test esistenti (top_n == len(rows),
    # dove ogni riga viene comunque selezionata indipendentemente dal floor).
    rows = (
        [_scored_row(f"T{i}", "Technology", 10 - i) for i in range(3)]      # T0=10,T1=9,T2=8
        + [_scored_row(f"N{i}", "Energy", 100 - i) for i in range(7)]       # N0=100..N6=94
    )
    selected = gs.select(rows, 5)
    tech_count = sum(1 for r in selected if r["sector"] == "Technology")
    tickers = {r["ticker"] for r in selected}
    assert len(selected) == 5
    # floor = ceil(0.3*5) = 2 -> T0,T1 riservati (i 2 tech migliori, non T2)
    assert tech_count == 2
    assert "T0" in tickers and "T1" in tickers
    assert "T2" not in tickers  # 3° tech non entra: il floor è 2, non un tetto minimo diverso
    # senza floor i primi 5 per punteggio sarebbero N0..N4 (tutti non-tech) --
    # con il floor, N3 e N4 vengono spostati fuori per fare posto a T0/T1.
    assert "N3" not in tickers
    assert "N4" not in tickers
    assert {"N0", "N1", "N2"} <= tickers


def test_revenue_trend_label_constant_growth():
    points = [("2022", 100.0), ("2023", 110.0), ("2024", 130.0)]
    assert gs.revenue_trend_label(points) == "crescita costante"


def test_revenue_trend_label_decline():
    points = [("2022", 130.0), ("2023", 110.0), ("2024", 100.0)]
    assert gs.revenue_trend_label(points) == "in calo"


def test_revenue_trend_label_mixed():
    points = [("2022", 100.0), ("2023", 130.0), ("2024", 110.0)]
    assert gs.revenue_trend_label(points) == "misto"


def test_revenue_trend_label_insufficient_points_is_nd():
    assert gs.revenue_trend_label([]) == "n/d"
    assert gs.revenue_trend_label([("2024", 100.0)]) == "n/d"


def test_enrich_selection_adds_sector_avg_pe_from_known_peers():
    # FakeDataSource ha AAPL/MSFT/GOOGL/NVDA come peer "Technology" con
    # pe_ratio 28/32/24/45 -> media 32.25 (data_sources/fake_source.py).
    selection = [{"ticker": "FAKE", "sector": "Technology", "name": "Fake Corp"}]
    gs.enrich_selection(FakeDataSource(), selection)
    assert selection[0]["sector_avg_pe"] == 32.25


def test_enrich_selection_unmapped_sector_is_none():
    selection = [{"ticker": "FAKE", "sector": "Utilities", "name": "Fake Corp"}]
    gs.enrich_selection(FakeDataSource(), selection)
    assert selection[0]["sector_avg_pe"] is None  # Utilities non in peers.SECTOR_PEERS


def test_enrich_selection_adds_revenue_trend_from_fundamentals():
    # FakeDataSource["FAKE"].revenue_by_period = [("2024-Q4",100),("2025-Q4",120)]
    selection = [{"ticker": "FAKE", "sector": "Technology", "name": "Fake Corp"}]
    gs.enrich_selection(FakeDataSource(), selection)
    assert selection[0]["revenue_trend"] == "crescita costante"


def test_enrich_selection_memoizes_peer_fetches_across_same_sector_winners():
    source = FakeDataSource()
    calls = []
    original = source.fetch_fundamentals
    source.fetch_fundamentals = lambda t: (calls.append(t), original(t))[1]
    selection = [
        {"ticker": "FAKE", "sector": "Technology", "name": "Fake Corp"},
        {"ticker": "OTHER", "sector": "Technology", "name": "Other Corp"},
    ]
    source.fundamentals_by_ticker["OTHER"] = source.fundamentals_by_ticker["FAKE"]
    gs.enrich_selection(source, selection)
    # Peer AAPL/MSFT/GOOGL/NVDA fetchati una volta sola nonostante 2 vincitori
    # nello stesso settore (più le 2 fetch dirette per il trend ricavi).
    peer_calls = [c for c in calls if c in ("AAPL", "MSFT", "GOOGL", "NVDA")]
    assert len(peer_calls) == len(set(peer_calls))


def test_enrich_selection_differs_correctly_when_winner_ticker_is_a_known_peer():
    # AAPL è sia un vincitore SIA uno dei peer noti hardcoded per "Technology"
    # (peers.py). peers_for_sector esclude sempre il ticker del vincitore
    # stesso, quindi la lista peer di AAPL (esclude AAPL) e quella di FAKE
    # (non esclude nulla) DEVONO differire -- questo è lo scenario esatto
    # che richiede la memoizzazione per singolo ticker peer, non per settore
    # (spec design §5): una cache a livello di settore darebbe a entrambi lo
    # stesso valore medio, sbagliato per almeno uno dei due.
    source = FakeDataSource()
    calls = []
    original = source.fetch_fundamentals
    source.fetch_fundamentals = lambda t: (calls.append(t), original(t))[1]
    selection = [
        {"ticker": "AAPL", "sector": "Technology", "name": "Fake Apple"},
        {"ticker": "FAKE", "sector": "Technology", "name": "Fake Corp"},
    ]
    gs.enrich_selection(source, selection)
    aapl_row = next(r for r in selection if r["ticker"] == "AAPL")
    fake_row = next(r for r in selection if r["ticker"] == "FAKE")
    # AAPL come vincitore: peer = MSFT,GOOGL,NVDA (esclude se stesso) -> media (32+24+45)/3
    assert aapl_row["sector_avg_pe"] == (32.0 + 24.0 + 45.0) / 3
    # FAKE come vincitore: peer = AAPL,MSFT,GOOGL,NVDA (FAKE non è nella lista) -> media (28+32+24+45)/4
    assert fake_row["sector_avg_pe"] == (28.0 + 32.0 + 24.0 + 45.0) / 4
    assert aapl_row["sector_avg_pe"] != fake_row["sector_avg_pe"]
    # I peer CONDIVISI (MSFT,GOOGL,NVDA) vengono fetchati una sola volta
    # nonostante servano a entrambi i vincitori -- memoizzazione per ticker,
    # non per settore (altrimenti i due valori sopra sarebbero uguali).
    # AAPL può essere fetchato due volte: una come peer per FAKE, una come
    # ticker stesso per il trend ricavi di AAPL — non è un fallimento di memo.
    shared_peer_calls = [c for c in calls if c in ("MSFT", "GOOGL", "NVDA")]
    assert len(shared_peer_calls) == len(set(shared_peer_calls))


class _IbkrLikeFundamentalsSource(FakeDataSource):
    """Imita ESATTAMENTE lo shape ritornato da
    IbkrDataSource.fetch_fundamentals (Reuters Fundamentals non
    sottoscritto -- vedi ibkr_source.py/task-5-final-brief.md): pe_ratio
    None per ogni ticker (peer inclusi), revenue_by_period sempre vuoto.
    Verifica diretta dei call site goldman_sachs.py:172/182 (accesso
    `.get(...)`, non subscript diretto) -- dispatch() da solo NON basta a
    esercitare questo path: sector=None fa scartare ogni entry PRIMA di
    arrivare a enrich_selection (vedi test_registry.py)."""

    def fetch_fundamentals(self, ticker):
        return {
            "ticker": ticker, "name": "N/D", "sector": "N/D", "industry": "N/D",
            "market_cap": None, "pe_ratio": None, "current_price": None,
            "revenue_by_period": [], "earnings_by_period": [],
        }


def test_enrich_selection_survives_ibkr_like_source_with_all_fundamentals_nd_or_none():
    selection = [{"ticker": "FAKE", "sector": "Technology", "name": "Fake Corp"}]
    gs.enrich_selection(_IbkrLikeFundamentalsSource(), selection)
    # Nessuna eccezione (verificato per il solo fatto di arrivare qui) +
    # comportamento CORRETTO, non solo "silenziosamente inghiottito" dal
    # try/except: pe_ratio assente per ogni peer -> media non calcolabile.
    assert selection[0]["sector_avg_pe"] is None
    # revenue_by_period sempre vuoto -> nessun trend leggibile.
    assert selection[0]["revenue_trend"] == "n/d"


def _selection_row():
    return {
        "ticker": "AAA", "name": "Alpha Corp", "sector": "Technology",
        "current_price": 50.0, "score": 88.5,
        "metrics": {"pe_ratio": 15.0, "revenue_growth": 0.2,
                    "debt_to_equity": 0.3, "target_upside": 0.2},
        "dividend_yield": 0.01, "payout_ratio": 0.2,
        "target_mean_price": 60.0, "target_high_price": 70.0, "target_low_price": 45.0,
        "sector_avg_pe": 25.0, "revenue_trend": "crescita costante",
    }


def _coverage():
    return {"cached": 40, "universe": 500, "qualified": 12, "stale_used": 3,
            "fetch_stats": {"new": 10, "refreshed": 5, "failed": 1}}


def test_render_document_contains_ticker_and_score():
    doc = gs.render_document([_selection_row()], _coverage(), generated_at="11/08/2026 10:00")
    assert "AAA" in doc
    # number_format.format_number usa la virgola decimale italiana (scambia
    # "." <-> ","): 88.5 con 1 decimale rende "88,5", non "88.5".
    assert "88,5" in doc
    assert "Goldman Sachs" in doc
    assert "11/08/2026 10:00" in doc


def test_render_document_empty_selection_says_no_qualified():
    doc = gs.render_document([], _coverage())
    assert "Nessun titolo qualificato" in doc


def test_render_summary_for_ai_contains_key_metrics_for_judgment():
    summary = gs.render_summary_for_ai([_selection_row()], _coverage())
    assert "Alpha Corp" in summary
    assert "crescita costante" in summary
    # _fmt usa number_format.format_number (2 decimali, virgola italiana):
    # sector_avg_pe=25.0 -> "25,00", non "25.0".
    assert "25,00" in summary  # sector_avg_pe nel testo
    # number_format usa la virgola decimale italiana: score 88.5 -> "88,5",
    # non "88.5" -- coerente con _table/render_channel_summary che usano lo
    # stesso formato per lo stesso campo.
    assert "88,5" in summary


def test_render_summary_for_ai_empty_selection_says_no_judgment():
    summary = gs.render_summary_for_ai([], _coverage())
    assert "Non scrivere alcun giudizio" in summary


def test_render_channel_summary_mentions_first_ticker():
    summary = gs.render_channel_summary([_selection_row()], _coverage())
    assert "Alpha Corp" in summary
    assert "AAA" in summary


def test_run_returns_full_contract_shape(tmp_path):
    from datetime import date
    import json
    # Pre-seed la cache con un entry FAKE già fresco: questo test verifica
    # il WIRING di run() (candidates->fetch->build_rows->score_rows->select->
    # enrich_selection->render_*), non l'algoritmo di select_candidates
    # (già testato a parte in test_fundamentals_cache.py) -- con cache
    # vuota, il budget di fetch (60) viene sempre esaurito dall'universo
    # indici reale (440 ticker, da index_membership) prima di raggiungere
    # un ticker fuori indice come FAKE, quindi il pre-seed evita di dipendere
    # da quell'aritmetica (caratteristica nota e out-of-scope di
    # fundamentals_cache.select_candidates, condivisa con consumer-usage).
    cache_path = tmp_path / "cache.json"
    fake_entry = {
        "sector": "Technology", "market_cap": 1_000_000_000.0, "ps_ratio": 2.5,
        "revenue_growth": 0.15, "fifty_two_week_high": 140.0, "current_price": 110.0,
        "target_mean_price": 130.0, "peg_ratio": 1.2, "pe_ratio": 22.0,
        "debt_to_equity": 0.6, "dividend_yield": 0.012, "payout_ratio": 0.25,
        "target_high_price": 150.0, "target_low_price": 100.0,
        "fetched_at": date.today().isoformat(),
    }
    cache_path.write_text(json.dumps({"FAKE": fake_entry}), encoding="utf-8")

    source = FakeDataSource()
    tickers = [{"ticker": "FAKE", "name": "Fake Corp"}]
    out = gs.run(source, tickers, cache_path, tmp_path / "discoveries.json", top=5)
    assert "summary" in out
    assert "report_markdown" in out
    assert "channel_summary" in out
    assert "title_suffix" in out
    assert "Fake Corp" in out["report_markdown"]


def test_run_ignores_discoveries_path_never_written(tmp_path):
    source = FakeDataSource()
    tickers = [{"ticker": "FAKE", "name": "Fake Corp"}]
    discoveries_path = tmp_path / "discoveries.json"
    gs.run(source, tickers, tmp_path / "cache.json", discoveries_path, top=5)
    assert not discoveries_path.exists()  # nessuna esclusione 30gg per questo screener
