"""Test per index_membership.py — appartenenza indice, singola/multipla/nessuna."""

import index_membership


def test_indices_for_ticker_in_a_single_index():
    # LCID (Lucid Motors) è SOLO in Nasdaq-100 nelle nostre liste curate
    # (non in DOW_JONES_TICKERS né in SP500_TICKERS) — un vero caso a un
    # solo indice, a differenza dei grandi nomi come AAPL/KO che nella
    # realtà (e nelle nostre liste) sono quasi sempre in più indici insieme.
    assert index_membership.indices_for("LCID") == "Nasdaq-100"


def test_indices_for_ticker_in_multiple_indices():
    # AAPL è sia S&P 500 sia Nasdaq-100 (e Dow Jones) in questa lista curata.
    result = index_membership.indices_for("AAPL")
    assert "S&P 500" in result
    assert "Nasdaq-100" in result
    assert "Dow Jones" in result
    assert result.count(",") == 2, f"attese 3 appartenenze separate da virgola: {result!r}"


def test_indices_for_ticker_in_no_index():
    assert index_membership.indices_for("ZZZZNOTAREALTICKER") == "-"


def test_indices_for_is_case_insensitive():
    assert index_membership.indices_for("aapl") == index_membership.indices_for("AAPL")


def test_sp500_set_contains_well_known_large_caps():
    # Spot-check di accuratezza (nomi notissimi, difficile che siano assenti
    # da qualunque lista S&P 500 plausibile) — non sostituisce una verifica
    # completa, ma cattura un errore grossolano nella lista curata.
    for ticker in ("AAPL", "MSFT", "AMZN", "GOOGL", "JPM", "XOM", "JNJ", "PG", "KO", "WMT"):
        assert ticker in index_membership.SP500_TICKERS, f"{ticker} atteso nell'S&P 500"


def test_dow_jones_set_has_exactly_30_members():
    assert len(index_membership.DOW_JONES_TICKERS) == 30


def test_indices_for_uses_the_real_sec_hyphenated_class_share_spelling():
    # La cache reale (tickers_us.json, da SEC) spella le azioni multi-classe
    # con un TRATTINO, mai un punto (es. "BRK-B", non "BRK.B") — un bug reale
    # trovato in review: la prima versione di SP500_TICKERS usava "BRK.B",
    # che non fa mai match con la vera chiave di join.
    assert index_membership.indices_for("BRK-B") == "S&P 500"
    assert index_membership.indices_for("BRK.B") == "-", "il punto non è mai la spelling reale della cache SEC"
