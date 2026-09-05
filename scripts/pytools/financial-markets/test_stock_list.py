"""Test per stock_list.py — join Nome/Asset/Paese/Indice, ordinamento,
rendering tabella, validazione paese. Dati sintetici per i ticker (la
cache reale è caricata da server.py, non qui), ma index_membership è quello
vero — questi test verificano anche che l'integrazione dei due moduli sia
corretta, non solo ciascuno in isolamento."""

import stock_list


def test_build_rows_joins_name_asset_country_and_index():
    tickers = [{"ticker": "AAPL", "name": "Apple Inc."}]
    rows = stock_list.build_rows(tickers, "USA")
    assert rows[0] == {"name": "Apple Inc.", "asset": "AAPL", "country": "USA", "index": "S&P 500, Dow Jones, Nasdaq-100"}


def test_build_rows_sorts_alphabetically_by_name():
    # Ticker e nome in ordine OPPOSTO fra loro: se il codice ordinasse per
    # ticker invece che per nome, questo test fallirebbe (con la vecchia
    # fixture ZZZ/Zeta + AAA/Alpha i due ordini coincidevano e il test
    # sarebbe passato comunque, mascherando un eventuale bug — bug reale
    # trovato in review).
    tickers = [{"ticker": "AAA", "name": "Zeta Corp"}, {"ticker": "ZZZ", "name": "Alpha Corp"}]
    rows = stock_list.build_rows(tickers, "USA")
    assert [r["name"] for r in rows] == ["Alpha Corp", "Zeta Corp"]


def test_build_rows_ticker_not_in_any_index_shows_dash():
    tickers = [{"ticker": "ZZZZNOTREAL", "name": "Nobody Corp"}]
    rows = stock_list.build_rows(tickers, "USA")
    assert rows[0]["index"] == "-"


def test_build_rows_empty_tickers_returns_empty_list():
    assert stock_list.build_rows([], "USA") == []


def test_build_rows_uppercases_country_for_display():
    # country arriva dall'AI/utente e può essere in minuscolo ("usa") — la
    # colonna Paese deve mostrare sempre la spelling maiuscola convenzionale.
    tickers = [{"ticker": "AAPL", "name": "Apple Inc."}]
    rows = stock_list.build_rows(tickers, "usa")
    assert rows[0]["country"] == "USA"


def test_render_table_has_four_columns_in_order():
    rows = stock_list.build_rows([{"ticker": "AAPL", "name": "Apple Inc."}], "USA")
    table = stock_list.render_table(rows)
    header = table.splitlines()[0]
    assert header == "| Nome | Asset | Paese | Indice |"


def test_render_table_body_row_matches_the_built_row():
    rows = stock_list.build_rows([{"ticker": "AAPL", "name": "Apple Inc."}], "USA")
    table = stock_list.render_table(rows)
    body = table.splitlines()[2]
    assert body == "| Apple Inc. | AAPL | USA | S&P 500, Dow Jones, Nasdaq-100 |"


def test_render_table_empty_rows_returns_a_readable_placeholder():
    table = stock_list.render_table([])
    assert "Nessun titolo trovato" in table


def test_is_country_supported_accepts_usa_case_insensitive():
    assert stock_list.is_country_supported("USA")
    assert stock_list.is_country_supported("usa")


def test_is_country_supported_rejects_anything_else():
    assert not stock_list.is_country_supported("Italy")
    assert not stock_list.is_country_supported("")


def test_unsupported_country_message_names_the_requested_country():
    msg = stock_list.unsupported_country_message("Italy")
    assert "Italy" in msg
    assert "USA" in msg
