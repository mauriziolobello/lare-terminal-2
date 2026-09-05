"""Test per screeners/citadel.py -- screener 'Citadel' (analisi tecnica
quant, doppio regime momentum/mean-reversion, tetto 2/settore). Modulo
INDIPENDENTE da consumer_usage.py/goldman_sachs.py/jensen_huang.py per
design (Docs/superpowers/specs/2026-08-26-screener-citadel-design.md) --
nessun import incrociato, nessuna fixture condivisa con gli altri test
screener."""

from screeners import citadel as cd
import json
from data_sources.fake_source import FakeDataSource


def test_sma_returns_none_when_not_enough_values():
    assert cd.sma([1.0, 2.0, 3.0], 5) is None


def test_sma_averages_last_n_values():
    assert cd.sma([10.0, 20.0, 30.0, 40.0], 2) == 35.0


def test_rsi_returns_none_when_not_enough_closes():
    assert cd.rsi([1.0] * 10, period=14) is None


def test_rsi_all_gains_is_100():
    closes = [100.0 + i for i in range(15)]  # 14 salite consecutive
    assert cd.rsi(closes, period=14) == 100.0


def test_rsi_all_losses_is_zero():
    closes = [100.0 - i for i in range(15)]  # 14 discese consecutive
    assert cd.rsi(closes, period=14) == 0.0


def test_rsi_mixed_is_between_bounds():
    closes = [100.0, 102.0, 101.0, 103.0, 100.0, 104.0, 102.0,
              105.0, 103.0, 106.0, 104.0, 107.0, 105.0, 108.0, 106.0]
    value = cd.rsi(closes, period=14)
    assert 0.0 < value < 100.0


def test_trend_stack_all_bullish_is_three():
    assert cd.trend_stack(price=110.0, sma50=105.0, sma100=100.0, sma200=95.0) == 3


def test_trend_stack_all_bearish_is_zero():
    assert cd.trend_stack(price=90.0, sma50=95.0, sma100=100.0, sma200=105.0) == 0


def test_trend_stack_missing_sma_never_raises():
    assert cd.trend_stack(price=110.0, sma50=None, sma100=None, sma200=None) == 0
    assert cd.trend_stack(price=110.0, sma50=105.0, sma100=None, sma200=None) == 1


def _uptrend_closes(n=60):
    # Flat for min(30, n) bars, then strong uptrend -- shows momentum divergence
    flat = min(30, n)
    return [100.0] * flat + [100.0 + (i - flat) * 2.0 for i in range(flat, n)]


def _downtrend_closes(n=60):
    # Flat for min(30, n) bars, then strong downtrend -- shows momentum divergence
    flat = min(30, n)
    return [100.0] * flat + [100.0 - (i - flat) * 2.0 for i in range(flat, n)]


def test_macd_returns_none_when_not_enough_closes():
    assert cd.macd([1.0] * 20) is None


def test_macd_uptrend_has_positive_histogram():
    result = cd.macd(_uptrend_closes())
    assert result is not None
    assert result["histogram"] > 0


def test_macd_downtrend_has_negative_histogram():
    result = cd.macd(_downtrend_closes())
    assert result is not None
    assert result["histogram"] < 0


def test_macd_histogram_n_bars_ago_returns_none_when_not_enough_history():
    assert cd.macd_histogram_n_bars_ago(_uptrend_closes(n=30), n=10) is None


def test_macd_histogram_n_bars_ago_matches_macd_on_truncated_series():
    closes = _uptrend_closes(n=60)
    expected = cd.macd(closes[:-10])["histogram"]
    assert cd.macd_histogram_n_bars_ago(closes, n=10) == expected


def test_bollinger_returns_none_when_not_enough_closes():
    assert cd.bollinger([1.0] * 5, period=20) is None


def test_bollinger_flat_price_has_zero_width_bands():
    bands = cd.bollinger([100.0] * 20, period=20)
    assert bands == {"lower": 100.0, "mid": 100.0, "upper": 100.0}


def test_percent_b_none_on_zero_width_bands():
    # Prezzo piatto -> stddev 0 -> bande larghezza 0 -> %B deve essere
    # None, MAI una divisione per zero (spec §9 fix).
    assert cd.percent_b([100.0] * 20, period=20) is None


def test_percent_b_above_one_when_price_breaks_out_above_upper_band():
    # Ultima chiusura molto sopra le 19 precedenti (piatte): il prezzo
    # sfonda la banda alta -- %B > 1 (verificato: ~1.59), non un errore,
    # %B non e' clampato a [0,1] per design (segnala il breakout).
    closes = [100.0] * 19 + [110.0]
    value = cd.percent_b(closes, period=20)
    assert value is not None
    assert value > 1.0


def test_volume_ratio_returns_none_when_not_enough_volumes():
    assert cd.volume_ratio([100.0] * 5, period=20) is None


def test_volume_ratio_above_average_is_greater_than_one():
    volumes = [1_000_000.0] * 19 + [3_000_000.0]
    assert cd.volume_ratio(volumes, period=20) > 1.0


def test_percent_b_none_on_realistic_flat_price_with_cents():
    # 7.77 e' un valore verificato: sotto la vecchia guardia width==0
    # (float esatto), bollinger([7.77]*20) produce un width non-zero per
    # rumore di arrotondamento (~1e-15), quindi percent_b tornava un
    # valore fabbricato (0.75) invece di None. La nuova guardia rileva la
    # flatness sull'input (min==max) e cattura correttamente questo caso.
    assert cd.percent_b([7.77] * 20, period=20) is None


def _bar(date_str, high, low):
    return {"date": date_str, "open": (high + low) / 2, "high": high,
            "low": low, "close": (high + low) / 2, "volume": 1_000_000.0}


def test_six_month_high_low_empty_bars_is_none():
    assert cd.six_month_high_low([]) is None


def test_six_month_high_low_uses_full_window():
    bars = [_bar(f"2026-01-{d:02d}", high=100.0 + d, low=90.0 - d) for d in range(1, 11)]
    result = cd.six_month_high_low(bars)
    assert result == {"high": 110.0, "low": 80.0}


def test_swing_high_low_uses_lookback_window_only():
    old_bar = _bar("2020-01-01", high=500.0, low=1.0)  # fuori dal lookback
    recent = [_bar(f"2026-01-{d:02d}", high=100.0 + d, low=90.0 - d) for d in range(1, 11)]
    result = cd.swing_high_low([old_bar] + recent, lookback=10)
    assert result == {"high": 110.0, "low": 80.0}


def test_fibonacci_levels_returns_five_standard_levels():
    levels = cd.fibonacci_levels(swing_low=100.0, swing_high=200.0)
    assert levels["50.0%"] == 150.0
    assert levels["23.6%"] == 200.0 - 0.236 * 100.0
    assert levels["78.6%"] == 200.0 - 0.786 * 100.0
    assert set(levels) == {"23.6%", "38.2%", "50.0%", "61.8%", "78.6%"}


def _daily_bars_across_weeks(weeks: int, base_price: float = 100.0, step: float = 5.0):
    """7 barre giornaliere per settimana, chiusura crescente di `step` a
    ogni settimana -- basta a rendere il trend settimanale deterministico
    senza inventare un vero calendario di borsa."""
    from datetime import date, timedelta
    bars = []
    start = date(2026, 1, 5)  # lunedi'
    for w in range(weeks):
        price = base_price + w * step
        for d in range(7):
            day = start + timedelta(weeks=w, days=d)
            bars.append({"date": day.isoformat(), "open": price, "high": price + 1,
                        "low": price - 1, "close": price, "volume": 1_000_000.0})
    return bars


def test_weekly_trend_none_when_not_enough_weeks():
    bars = _daily_bars_across_weeks(weeks=3)
    assert cd.weekly_trend(bars, sma_period=10) is None


def test_weekly_trend_up_when_rising_across_weeks():
    bars = _daily_bars_across_weeks(weeks=15, step=5.0)
    assert cd.weekly_trend(bars, sma_period=10) == "up"


def test_weekly_trend_down_when_falling_across_weeks():
    bars = _daily_bars_across_weeks(weeks=15, step=-5.0)
    assert cd.weekly_trend(bars, sma_period=10) == "down"


def _daily_bars_across_months(months: int, base_price: float = 100.0, step: float = 10.0):
    from datetime import date
    bars = []
    for m in range(months):
        year = 2025 + (m // 12)
        month = (m % 12) + 1
        price = base_price + m * step
        for d in (5, 15, 25):
            bars.append({"date": date(year, month, d).isoformat(), "open": price,
                        "high": price + 1, "low": price - 1, "close": price,
                        "volume": 1_000_000.0})
    return bars


def test_monthly_trend_up_when_rising_across_months():
    bars = _daily_bars_across_months(months=8, step=10.0)
    assert cd.monthly_trend(bars, sma_period=6) == "up"


def test_monthly_trend_down_when_falling_across_months():
    bars = _daily_bars_across_months(months=8, step=-10.0)
    assert cd.monthly_trend(bars, sma_period=6) == "down"


def test_group_last_close_keeps_the_last_bar_of_each_group_not_the_first():
    from datetime import date, timedelta
    start = date(2026, 1, 5)  # lunedi' (stessa ancora ISO delle altre fixture di questo task)
    bars = [{"date": (start + timedelta(days=d)).isoformat(), "open": 0, "high": 0, "low": 0,
            "close": 100.0 + d, "volume": 0.0} for d in range(7)]  # una sola settimana ISO, chiusure 100..106
    closes = cd._group_last_close(bars, lambda d: d.isocalendar()[:2])
    assert closes == [106.0]  # l'ULTIMA barra della settimana (day=6, close=106), non la prima (100)


def _uptrend_bars(n=60, start_price=100.0, step=1.0):
    from datetime import date, timedelta
    bars = []
    start = date(2024, 1, 1)
    for i in range(n):
        price = start_price + i * step
        day = start + timedelta(days=i)
        bars.append({"date": day.isoformat(), "open": price - 0.5, "high": price + 1.0,
                    "low": price - 1.0, "close": price, "volume": 1_000_000.0})
    return bars


def _downtrend_bars(n=60, start_price=200.0, step=1.0):
    from datetime import date, timedelta
    bars = []
    start = date(2024, 1, 1)
    for i in range(n):
        price = start_price - i * step
        day = start + timedelta(days=i)
        bars.append({"date": day.isoformat(), "open": price + 0.5, "high": price + 1.0,
                    "low": price - 1.0, "close": price, "volume": 1_000_000.0})
    return bars


def test_build_rows_classifies_uptrend_as_momentum():
    tech_cache = {"UP": {"fetched_at": "2026-08-26", "bars": _uptrend_bars()}}
    rows = cd.build_rows(tech_cache, {"UP": "Semiconductors"}, {"UP": "Uptrend Co"})
    assert rows[0]["regime"] == "momentum"
    assert "trend_stack" in rows[0]["metrics"]
    assert "pullback" not in rows[0]["metrics"]


def test_build_rows_classifies_downtrend_as_reversion():
    tech_cache = {"DOWN": {"fetched_at": "2026-08-26", "bars": _downtrend_bars()}}
    rows = cd.build_rows(tech_cache, {"DOWN": "Semiconductors"}, {"DOWN": "Downtrend Co"})
    assert rows[0]["regime"] == "reversion"
    assert "pullback" in rows[0]["metrics"]
    assert "trend_stack" not in rows[0]["metrics"]


def test_build_rows_excludes_ticker_with_too_few_bars():
    tech_cache = {"SHORT": {"fetched_at": "2026-08-26", "bars": _uptrend_bars(n=10)}}
    rows = cd.build_rows(tech_cache, {"SHORT": "Semiconductors"}, {"SHORT": "Short Co"})
    assert rows == []


def test_build_rows_excludes_ticker_with_unknown_industry():
    tech_cache = {"UP": {"fetched_at": "2026-08-26", "bars": _uptrend_bars()}}
    rows = cd.build_rows(tech_cache, {}, {"UP": "Uptrend Co"})  # nessuna industry nota
    assert rows == []


def test_build_rows_falls_back_to_ticker_when_name_unknown():
    tech_cache = {"UP": {"fetched_at": "2026-08-26", "bars": _uptrend_bars()}}
    rows = cd.build_rows(tech_cache, {"UP": "Semiconductors"}, {})
    assert rows[0]["name"] == "UP"


def test_build_rows_returns_all_expected_top_level_fields():
    # Blocca la shape della riga: Task 8 (score_rows) e Task 10 (rendering)
    # dipendono dai nomi esatti di questi campi -- un rename qui silente
    # romperebbe entrambi senza toccare i test locali di build_rows.
    tech_cache = {"UP": {"fetched_at": "2026-08-26", "bars": _uptrend_bars()}}
    rows = cd.build_rows(tech_cache, {"UP": "Semiconductors"}, {"UP": "Uptrend Co"})
    assert set(rows[0]) == {
        "ticker", "name", "industry", "regime", "current_price", "rsi",
        "macd_histogram", "percent_b", "volume_ratio", "trend_stack",
        "six_month_high", "six_month_low", "swing_high", "swing_low",
        "fibonacci", "weekly_trend", "monthly_trend", "metrics",
    }


def _oscillating_downtrend_bars(n=60, start_price=200.0):
    """Trend discendente ma NON lineare (alterna -3/+1 invece di -1
    costante) e volume variabile (non costante) -- a differenza di
    _downtrend_bars(), qui RSI non e' esattamente 0/100, volume_ratio non
    e' esattamente 1.0 e l'istogramma MACD non e' simmetrico. Serve a far
    fallire un bug di segno/operando scambiato nel wiring di build_rows()
    che su una serie lineare/degenere passerebbe inosservato (vedi review
    Task 7)."""
    from datetime import date, timedelta
    bars = []
    start = date(2024, 1, 1)
    price = start_price
    for i in range(n):
        price += -3.0 if i % 2 == 0 else 1.0
        day = start + timedelta(days=i)
        volume = 1_000_000.0 + (i % 7) * 50_000.0
        bars.append({"date": day.isoformat(), "open": price + 0.5, "high": price + 1.0,
                    "low": price - 1.0, "close": price, "volume": volume})
    return bars


def _oscillating_uptrend_bars(n=60, start_price=100.0):
    """Speculare a _oscillating_downtrend_bars(): trend ascendente non
    lineare (alterna +3/-1), volume variabile -- stessa motivazione."""
    from datetime import date, timedelta
    bars = []
    start = date(2024, 1, 1)
    price = start_price
    for i in range(n):
        price += 3.0 if i % 2 == 0 else -1.0
        day = start + timedelta(days=i)
        volume = 1_000_000.0 + (i % 7) * 50_000.0
        bars.append({"date": day.isoformat(), "open": price - 0.5, "high": price + 1.0,
                    "low": price - 1.0, "close": price, "volume": volume})
    return bars


def test_build_rows_reversion_metrics_match_direct_function_calls():
    # Pin dei VALORI (non solo delle chiavi) delle 5 metriche reversion,
    # confrontando contro chiamate dirette alle funzioni delle Task 2-6
    # gia' testate singolarmente -- cattura un segno/operando scambiato nel
    # wiring di build_rows() senza dover derivare a mano nuovi numeri
    # magici (review Task 7: RSI/volume_ratio/macd_improving erano tutti
    # degeneri sulle fixture lineari originali e non discriminavano bug di
    # segno).
    bars = _oscillating_downtrend_bars()
    tech_cache = {"DOWN2": {"fetched_at": "2026-08-26", "bars": bars}}
    rows = cd.build_rows(tech_cache, {"DOWN2": "Semiconductors"}, {})
    row = rows[0]
    assert row["regime"] == "reversion"  # conferma che la fixture classifica come atteso

    closes = [b["close"] for b in bars]
    volumes = [b["volume"] for b in bars]
    expected_rsi = cd.rsi(closes)
    expected_pb = cd.percent_b(closes)
    expected_vr = cd.volume_ratio(volumes)
    six_month = cd.six_month_high_low(bars)
    expected_pullback = (six_month["high"] - closes[-1]) / six_month["high"]
    hist_now = cd.macd(closes)["histogram"]
    hist_ago = cd.macd_histogram_n_bars_ago(closes, 10)
    expected_improving = hist_now - hist_ago

    # La fixture non e' degenere: RSI ne' 0 ne' 100 (altrimenti -RSI == RSI
    # per via di -0.0 == 0.0 e il test non distinguerebbe una negazione
    # dimenticata).
    assert expected_rsi not in (0.0, 100.0)
    assert row["metrics"]["rsi_inverted"] == -expected_rsi
    assert row["metrics"]["percent_b_inverted"] == -expected_pb
    assert row["metrics"]["volume_ratio"] == expected_vr
    assert row["metrics"]["pullback"] == expected_pullback
    assert row["metrics"]["macd_improving"] == expected_improving


def test_build_rows_momentum_metrics_match_direct_function_calls():
    # Stessa idea del test precedente, per il regime momentum.
    bars = _oscillating_uptrend_bars()
    tech_cache = {"UP2": {"fetched_at": "2026-08-26", "bars": bars}}
    rows = cd.build_rows(tech_cache, {"UP2": "Semiconductors"}, {})
    row = rows[0]
    assert row["regime"] == "momentum"

    closes = [b["close"] for b in bars]
    volumes = [b["volume"] for b in bars]
    price = closes[-1]
    s50, s100, s200 = cd.sma(closes, 50), cd.sma(closes, 100), cd.sma(closes, 200)
    expected_trend_stack = float(cd.trend_stack(price, s50, s100, s200))
    expected_macd_hist = cd.macd(closes)["histogram"]
    expected_pb = cd.percent_b(closes)
    expected_vr = cd.volume_ratio(volumes)
    expected_rsi = cd.rsi(closes)
    expected_rsi_health = -abs(expected_rsi - 60)

    assert row["metrics"]["trend_stack"] == expected_trend_stack
    assert row["metrics"]["macd_histogram"] == expected_macd_hist
    assert row["metrics"]["percent_b"] == expected_pb
    assert row["metrics"]["volume_ratio"] == expected_vr
    assert row["metrics"]["rsi_health"] == expected_rsi_health


def _momentum_row(ticker, trend_stack_val, macd_hist, pb, vol_ratio, rsi_health):
    return {"ticker": ticker, "name": ticker, "regime": "momentum",
            "metrics": {"trend_stack": trend_stack_val, "macd_histogram": macd_hist,
                       "percent_b": pb, "volume_ratio": vol_ratio, "rsi_health": rsi_health}}


def _reversion_row(ticker, pullback, rsi_inv, pb_inv, vol_ratio, macd_improving):
    return {"ticker": ticker, "name": ticker, "regime": "reversion",
            "metrics": {"pullback": pullback, "rsi_inverted": rsi_inv,
                       "percent_b_inverted": pb_inv, "volume_ratio": vol_ratio,
                       "macd_improving": macd_improving}}


def test_score_rows_ranks_within_own_regime_group():
    # 12 titoli momentum (>= MIN_GROUP_FOR_PERCENTILE) cosi' i percentili
    # sono pieni, non il fallback neutro 50.0.
    rows = [_momentum_row(f"M{i}", trend_stack_val=float(i % 4), macd_hist=float(i),
                          pb=float(i) / 12, vol_ratio=1.0 + i / 12, rsi_health=-float(i))
            for i in range(12)]
    scored = cd.score_rows(rows)
    assert len(scored) == 12
    assert scored[0]["score"] >= scored[-1]["score"]


def test_score_rows_small_group_gets_neutral_fifty():
    # Gruppo reversion piccolo (3 < MIN_GROUP_FOR_PERCENTILE=10): ogni
    # metrica vale 50.0 neutro per tutti, mai un percentile pieno che
    # gonfierebbe il migliore dei 3 (spec §4, trovato in review).
    rows = [_reversion_row(f"R{i}", pullback=float(i), rsi_inv=float(i),
                           pb_inv=float(i), vol_ratio=1.0, macd_improving=float(i))
            for i in range(3)]
    scored = cd.score_rows(rows)
    assert all(r["score"] == 50.0 for r in scored)


def test_score_rows_excludes_rows_with_fewer_than_three_metrics():
    rows = [
        _momentum_row("FULL", trend_stack_val=3.0, macd_hist=1.0, pb=0.8, vol_ratio=1.2, rsi_health=-2.0),
        {"ticker": "SPARSE", "name": "SPARSE", "regime": "momentum",
         "metrics": {"trend_stack": None, "macd_histogram": None, "percent_b": 0.5,
                    "volume_ratio": None, "rsi_health": -5.0}},
    ]
    scored = cd.score_rows(rows)
    assert {r["ticker"] for r in scored} == {"FULL"}


def test_score_rows_merges_both_regime_pools_sorted_by_score():
    momentum = [_momentum_row(f"M{i}", trend_stack_val=3.0, macd_hist=1.0, pb=0.9,
                              vol_ratio=1.5, rsi_health=-1.0) for i in range(10)]
    reversion = [_reversion_row(f"R{i}", pullback=0.5, rsi_inv=-20.0, pb_inv=-0.1,
                                vol_ratio=1.1, macd_improving=0.2) for i in range(10)]
    scored = cd.score_rows(momentum + reversion)
    tickers = {r["ticker"] for r in scored}
    assert tickers == {f"M{i}" for i in range(10)} | {f"R{i}" for i in range(10)}


def test_score_rows_never_leaks_percentiles_across_regimes_on_shared_metric_name():
    # volume_ratio e' un nome di metrica CONDIVISO da MOMENTUM_METRICS e
    # REVERSION_METRICS -- il rischio concreto e' calcolare il percentile
    # contro il pool misto invece che dentro il proprio gruppo. Qui il
    # gruppo momentum ha valori piccoli (max 2.0), il gruppo reversion ha
    # valori enormi (>=1000): se i percentili si mescolassero, il miglior
    # titolo momentum (volume_ratio piu' alto nel SUO gruppo) risulterebbe
    # "battuto" dai valori reversion e non prenderebbe piu' 100.0 per
    # quella metrica -- lo score complessivo scenderebbe sotto il massimo
    # teorico per quel gruppo.
    # M9 ha TUTTI i valori al massimo del gruppo momentum
    momentum_rows = [_momentum_row(f"M{i}", trend_stack_val=float(i),
                                   macd_hist=float(i),
                                   pb=float(i) / 10,
                                   vol_ratio=1.0 + float(i) / 10,
                                   rsi_health=float(i) - 10)
                     for i in range(10)]
    # Valori enormi su vol_ratio, MAI devono influenzare il gruppo momentum
    reversion_rows = [_reversion_row(f"R{i}", pullback=0.5, rsi_inv=-20.0, pb_inv=-0.1,
                                     vol_ratio=1000.0 + float(i), macd_improving=0.2)
                      for i in range(10)]
    scored = cd.score_rows(momentum_rows + reversion_rows)
    m9 = next(r for r in scored if r["ticker"] == "M9")
    # M9 batte tutti gli altri 9 nel SUO gruppo momentum su tutte le 5 metriche
    # -> 100.0 su ogni metrica -> score = 100.0
    # I valori enormi di reversion NON devono influenzare il risultato.
    assert m9["score"] == 100.0


def test_normalize_industry_handles_dash_variants_and_case():
    assert cd._normalize_industry("Software—Application") == "software-application"
    assert cd._normalize_industry("Software - Application") == "software-application"


def test_clamp_top_bounds():
    assert cd.clamp_top(1) == 4
    assert cd.clamp_top(10) == 10
    assert cd.clamp_top(99) == 30


def _scored_row(ticker, score, industry):
    return {"ticker": ticker, "name": ticker, "score": score, "industry": industry}


def test_select_caps_at_two_per_bucket_not_a_quota():
    # 5 titoli Automobiles (bucket singolo, NON AI-supplier -- a differenza
    # di Semiconductors, che avrebbe due bucket candidati e confonderebbe
    # questo test) coi punteggi migliori, 1 solo Farmaceutica: il tetto
    # prende solo i primi 2 Automobiles, poi PHARMA1 (bucket diverso, sotto
    # il proprio tetto) entra PRIMA del backfill; l'ultimo posto arriva dal
    # backfill (spec §5: tetto, non obbligo di copertura).
    rows = ([_scored_row(f"AUTO{i}", score=100 - i, industry="Automobiles") for i in range(5)]
            + [_scored_row("PHARMA1", score=50.0, industry="Drug Manufacturers")])
    selected = cd.select(rows, top_n=4)
    assert [r["ticker"] for r in selected] == ["AUTO0", "AUTO1", "PHARMA1", "AUTO2"]
    assert selected[3]["backfilled"] is True
    assert not selected[0].get("backfilled")


def test_select_dual_candidacy_fills_slots_without_backfill():
    # 2 Semiconductors (candidati sia per 'AI' sia per 'semiconductors' --
    # finiscono in bucket DIVERSI, uno ciascuno, mai in conflitto) + 2
    # Drug Manufacturers: 4 bucket-slot totali bastano per top_n=4 SENZA
    # backfill.
    rows = ([_scored_row(f"SEMI{i}", score=100 - i, industry="Semiconductors") for i in range(2)]
            + [_scored_row(f"PHARMA{i}", score=90 - i, industry="Drug Manufacturers") for i in range(2)])
    selected = cd.select(rows, top_n=4)
    assert len(selected) == 4
    assert not any(r.get("backfilled") for r in selected)


def test_select_ai_bucket_wins_tie_when_two_candidates_both_empty():
    # Semiconductors e' in _AI_SUPPLIER_INDUSTRIES -> candidato sia per
    # 'AI' sia per 'semiconductors'; a parita' (entrambi vuoti) vince AI.
    rows = [_scored_row("NVDA", score=100.0, industry="Semiconductors")]
    selected = cd.select(rows, top_n=1)
    assert selected[0]["bucket"] == "AI"


def test_select_never_exceeds_top_n():
    rows = [_scored_row(f"T{i}", score=100 - i, industry=f"Industry{i}") for i in range(20)]
    selected = cd.select(rows, top_n=10)
    assert len(selected) == 10


def _selection_row():
    return {
        "ticker": "CRWV", "name": "CoreWeave", "bucket": "AI", "regime": "momentum",
        "current_price": 100.0, "score": 88.5, "rsi": 62.0, "macd_histogram": 1.5,
        "percent_b": 0.8, "volume_ratio": 1.3, "weekly_trend": "up", "monthly_trend": "up",
        "six_month_high": 120.0, "six_month_low": 60.0,
        "swing_high": 110.0, "swing_low": 90.0,
        "fibonacci": {"23.6%": 105.28, "38.2%": 102.36, "50.0%": 100.0,
                     "61.8%": 97.64, "78.6%": 94.28},
    }


def _coverage():
    return {"cached": 40, "universe": 440, "qualified": 12, "stale_used": 3,
            "fetch_stats": {"new": 10, "refreshed": 5, "failed": 1}}


def test_render_document_contains_ticker_score_and_regime():
    doc = cd.render_document([_selection_row()], _coverage(), generated_at="26/08/2026 10:00")
    assert "CRWV" in doc
    assert "Momentum" in doc
    assert "26/08/2026 10:00" in doc
    assert "Citadel" in doc


def test_render_document_row_has_correct_column_order():
    """Estrae ogni data-sort-value nell'ordine in cui appare nell'HTML --
    un fingerprint POSIZIONALE dell'intera riga, non solo presenza di valori.
    Uno scambio di due celle QUALUNQUE nell'ordine delle colonne (Nome/Asset/
    Settore/Regime/Ultimo/Score/RSI/MACD hist/%B/Volume ratio/Trend) cambia
    questa sequenza. Cattura regressioni che test senza posizione non vedono."""
    doc = cd.render_document([_selection_row()], _coverage(), generated_at="26/08/2026 10:00")
    import re
    values_in_order = re.findall(r'data-sort-value="([^"]*)"', doc)
    # La fixture ha una sola riga -> primi 11 valori sono le nostre colonne
    # (Pos non ha data-sort-value)
    row_values = values_in_order[:11]
    assert row_values == [
        "CoreWeave", "CRWV", "AI", "Momentum", "100.0", "88.5",
        "62.0", "1.5", "0.8", "1.3", "up / up",
    ], f"Column order mismatch. Got: {row_values}"


def test_render_document_empty_selection_says_no_qualified():
    doc = cd.render_document([], _coverage())
    assert "Nessun titolo qualificato" in doc


def test_render_summary_for_ai_contains_regime_and_metrics():
    summary = cd.render_summary_for_ai([_selection_row()], _coverage())
    # Nota: questo test verifica solo PRESENZA, non POSIZIONE -- per
    # discriminare column-order bugs, vedi
    # test_render_document_row_has_correct_column_order (HTML fingerprint).
    assert "CoreWeave" in summary
    assert "CRWV" in summary
    assert "Momentum" in summary
    assert "livelli Fibonacci" in summary


def test_render_summary_for_ai_empty_selection_says_no_judgment():
    summary = cd.render_summary_for_ai([], _coverage())
    assert "Non scrivere alcun giudizio" in summary


def test_render_channel_summary_mentions_first_ticker():
    summary = cd.render_channel_summary([_selection_row()], _coverage())
    assert "CoreWeave" in summary
    assert "CRWV" in summary


def _seed_fundamentals_cache(path, ticker, industry):
    """citadel.run() legge cache_path (fundamentals_cache.json) in SOLA
    LETTURA per l'industry (spec §7) -- a differenza dei fratelli, non lo
    scrive mai. Nei test serve pre-seminarlo a mano, altrimenti un
    tmp_path fresco non contiene alcuna industry e la selezione resta
    sempre vuota."""
    path.write_text(json.dumps({ticker: {"industry": industry, "fetched_at": "2026-08-01",
                                         "sector": None, "market_cap": None, "ps_ratio": None,
                                         "revenue_growth": None, "fifty_two_week_high": None,
                                         "current_price": None, "target_mean_price": None,
                                         "peg_ratio": None, "pe_ratio": None, "debt_to_equity": None,
                                         "dividend_yield": None, "payout_ratio": None,
                                         "target_high_price": None, "target_low_price": None}}),
                    encoding="utf-8")


def test_run_returns_full_contract_shape(tmp_path):
    from datetime import date

    # Pre-seed ANCHE il technical_cache (sibling derivato) con un entry
    # FAKE gia' fresco (fetched_at = oggi): con cache vuota, il budget di
    # fetch (60) viene sempre esaurito dai 440 ticker reali dell'universo
    # indici PRIMA di raggiungere un ticker fuori-indice come FAKE (stesso
    # limite noto e out-of-scope di technical_cache.select_candidates,
    # verificato leggendo il pattern gia' usato da
    # screeners/test_jensen_huang.py::test_run_returns_full_contract_shape
    # per fundamentals_cache) -- senza questo pre-seed FAKE non finirebbe
    # mai in tech_cache e il test fallirebbe silenziosamente su una
    # selezione vuota, non su un bug reale del wiring che si vuole testare.
    source = FakeDataSource()
    tickers = [{"ticker": "FAKE", "name": "Fake Corp"}]
    cache_path = tmp_path / "cache.json"
    _seed_fundamentals_cache(cache_path, "FAKE", "Software—Infrastructure")
    technical_cache_path = tmp_path / "technical_cache.json"
    technical_cache_path.write_text(json.dumps({
        "FAKE": {"fetched_at": date.today().isoformat(), "bars": source.fetch_ohlc("FAKE", "D")}
    }), encoding="utf-8")

    out = cd.run(source, tickers, cache_path, tmp_path / "discoveries.json", top=5)
    assert "summary" in out
    assert "report_markdown" in out
    assert "channel_summary" in out
    assert "title_suffix" in out
    # FAKE ha industry AI-supplier e serie in rialzo (regime momentum) --
    # deve comparire nel report.
    assert "Fake Corp" in out["report_markdown"]
    assert "Momentum" in out["report_markdown"]


def test_run_ignores_discoveries_path_never_written(tmp_path):
    # Non serve pre-seminare technical_cache.json qui (a differenza di
    # test_run_returns_full_contract_shape sopra): questo test non legge il
    # contenuto del report, solo che discoveries_path resti intatto -- vero
    # indipendentemente dal fatto che FAKE venga o meno fetchato.
    source = FakeDataSource()
    tickers = [{"ticker": "FAKE", "name": "Fake Corp"}]
    cache_path = tmp_path / "cache.json"
    _seed_fundamentals_cache(cache_path, "FAKE", "Software—Infrastructure")
    discoveries_path = tmp_path / "discoveries.json"
    cd.run(source, tickers, cache_path, discoveries_path, top=5)
    assert not discoveries_path.exists()  # nessuna esclusione 30gg per questo screener


def test_run_writes_own_technical_cache_sibling_file_never_touches_fundamentals(tmp_path):
    source = FakeDataSource()
    tickers = [{"ticker": "FAKE", "name": "Fake Corp"}]
    cache_path = tmp_path / "cache.json"
    _seed_fundamentals_cache(cache_path, "FAKE", "Software—Infrastructure")
    before = cache_path.read_text(encoding="utf-8")
    cd.run(source, tickers, cache_path, tmp_path / "discoveries.json", top=5)
    # fundamentals_cache.json (cache_path) NON viene mai scritto da citadel.
    assert cache_path.read_text(encoding="utf-8") == before
    # il proprio cache OHLC vive in un file sibling derivato.
    assert (tmp_path / "technical_cache.json").exists()


def test_run_empty_fundamentals_cache_gives_empty_selection_not_a_crash(tmp_path):
    # Nessuna industry nota per nessun ticker (cache_path assente) --
    # selezione vuota, mai un'eccezione (stesso principio degli altri
    # screener con una fonte degradata).
    source = FakeDataSource()
    tickers = [{"ticker": "FAKE", "name": "Fake Corp"}]
    out = cd.run(source, tickers, tmp_path / "cache.json", tmp_path / "discoveries.json", top=5)
    assert "report_markdown" in out
    assert "Nessun titolo qualificato" in out["report_markdown"]
