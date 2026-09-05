"""Test per option_chain.py — accoppiamento call/put per strike, ritaglio
attorno all'ATM, rendering della tabella. Dati sintetici, mai da
FakeDataSource (una lista di 20+ strike andrebbe a gonfiare fake_source.py
senza motivo — questo modulo si testa da solo)."""

import option_chain


def _contract(strike, option_type, iv=0.30, bid=1.0, ask=1.1, last_price=1.05):
    return {"strike": strike, "expiry": "2025-08-15", "iv": iv, "option_type": option_type, "bid": bid, "ask": ask, "last_price": last_price}


def test_build_rows_pairs_call_and_put_at_the_same_strike():
    options = [_contract(100.0, "call"), _contract(100.0, "put")]
    rows = option_chain.build_rows(options, current_price=100.0)
    assert len(rows) == 1
    assert rows[0]["strike"] == 100.0
    assert rows[0]["call"]["option_type"] == "call"
    assert rows[0]["put"]["option_type"] == "put"


def test_build_rows_handles_a_strike_missing_one_side():
    # Capita nella option chain reale: uno strike molto fuori mercato può
    # avere solo call o solo put quotati.
    options = [_contract(100.0, "call")]
    rows = option_chain.build_rows(options, current_price=100.0)
    assert rows[0]["call"] is not None
    assert rows[0]["put"] is None


def test_build_rows_limits_to_window_strikes_above_and_below_atm():
    # 30 strike (90..119, passo 1), prezzo attuale 100 → atteso SOLO lo
    # strike ATM ± 10 (21 strike totali: 90..110), non l'intera lista.
    strikes = list(range(90, 120))
    options = []
    for s in strikes:
        options.append(_contract(float(s), "call"))
        options.append(_contract(float(s), "put"))
    rows = option_chain.build_rows(options, current_price=100.0, window=10)
    assert len(rows) == 21, f"atteso 21 righe (100 ± 10), trovate {len(rows)}"
    assert [r["strike"] for r in rows] == [float(s) for s in range(90, 111)]


def test_build_rows_centers_on_the_strike_closest_to_current_price_not_a_round_number():
    # Il prezzo attuale (103.4) non coincide con nessuno strike esatto —
    # l'ATM deve essere lo strike PIÙ VICINO (103 o 105 secondo la lista),
    # non un arrotondamento a priori che potrebbe non esistere nella lista.
    strikes = [95.0, 100.0, 103.0, 105.0, 110.0]
    options = [c for s in strikes for c in (_contract(s, "call"), _contract(s, "put"))]
    rows = option_chain.build_rows(options, current_price=103.4, window=1)
    assert [r["strike"] for r in rows] == [100.0, 103.0, 105.0]


def test_build_rows_without_current_price_returns_everything_sorted():
    options = [_contract(110.0, "call"), _contract(90.0, "call"), _contract(100.0, "call")]
    rows = option_chain.build_rows(options, current_price=None)
    assert [r["strike"] for r in rows] == [90.0, 100.0, 110.0]


def test_build_rows_empty_options_returns_empty_list():
    assert option_chain.build_rows([], current_price=100.0) == []


def test_build_rows_marks_the_strike_closest_to_current_price_as_atm():
    strikes = [95.0, 100.0, 103.0, 105.0, 110.0]
    options = [c for s in strikes for c in (_contract(s, "call"), _contract(s, "put"))]
    rows = option_chain.build_rows(options, current_price=103.4, window=10)
    atm_strikes = [r["strike"] for r in rows if r["is_atm"]]
    assert atm_strikes == [103.0], f"atteso solo 103.0 marcato ATM: {atm_strikes}"


def test_build_rows_without_current_price_marks_no_strike_as_atm():
    options = [_contract(110.0, "call"), _contract(90.0, "call"), _contract(100.0, "call")]
    rows = option_chain.build_rows(options, current_price=None)
    assert not any(r["is_atm"] for r in rows)


def test_render_table_has_a_two_row_header_call_and_put_span_four_columns_each():
    options = [_contract(100.0, "call", iv=0.35, bid=11.0, ask=11.5, last_price=11.2),
               _contract(100.0, "put", iv=0.40, bid=1.0, ask=1.3, last_price=1.1)]
    rows = option_chain.build_rows(options, current_price=100.0)
    table = option_chain.render_table(rows)
    assert '<th colspan="4">CALL</th>' in table
    assert '<th colspan="4">PUT</th>' in table
    # La riga d'intestazione CALL/PUT ha una cella vuota in corrispondenza
    # della colonna Strike della riga sotto.
    header_row_1 = table.splitlines()[2]
    assert header_row_1 == '<tr><th colspan="4">CALL</th><th></th><th colspan="4">PUT</th></tr>'
    # La seconda riga porta le sole etichette, MAI "Call "/"Put " davanti
    # (il raggruppamento lo dice già la riga sopra — richiesta esplicita).
    header_row_2 = table.splitlines()[3]
    assert "Call" not in header_row_2 and "Put" not in header_row_2
    for label in ("IV", "Bid", "Ask", "Ultimo", "Strike"):
        assert f"<th>{label}</th>" in header_row_2


def test_render_table_bolds_and_shades_the_atm_row_only():
    options = [
        _contract(95.0, "call"), _contract(95.0, "put"),
        _contract(100.0, "call"), _contract(100.0, "put"),
        _contract(105.0, "call"), _contract(105.0, "put"),
    ]
    rows = option_chain.build_rows(options, current_price=100.0, window=1)
    table = option_chain.render_table(rows)
    # Righe di corpo: iniziano con "<tr" e portano almeno una "<td" — sia la
    # forma semplice ("<tr><td...") sia quella con lo sfondo ATM
    # ("<tr style=...><td...") vanno intercettate, mai le righe di header
    # (che portano solo "<th").
    body_rows = [line for line in table.splitlines() if line.startswith("<tr") and "<td" in line]
    assert len(body_rows) == 3
    bolded = [r for r in body_rows if "<strong>" in r]
    assert len(bolded) == 1, f"atteso ESATTAMENTE una riga in grassetto (l'ATM): {body_rows}"
    assert "100" in bolded[0], f"la riga in grassetto deve essere quella dello strike ATM (100): {bolded[0]}"
    assert "background-color" in bolded[0], f"la riga ATM deve anche avere uno sfondo diverso: {bolded[0]}"
    shaded = [r for r in body_rows if "background-color" in r]
    assert len(shaded) == 1, f"atteso lo sfondo SOLO sulla riga ATM, non sulle altre: {body_rows}"


def test_render_table_uses_n_d_for_a_missing_side():
    rows = option_chain.build_rows([_contract(100.0, "call")], current_price=100.0)
    table = option_chain.render_table(rows)
    assert "n/d" in table


def test_render_table_empty_rows_returns_a_readable_placeholder():
    table = option_chain.render_table([])
    assert "<table" not in table
    assert len(table) > 0
