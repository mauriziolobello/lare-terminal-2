"""Test per number_format.py — formattazione it-IT e abbreviazione dinamica."""

import number_format


def test_format_number_adds_thousands_separators():
    assert number_format.format_number(4786462654464) == "4.786.462.654.464"


def test_format_number_uses_comma_for_decimals():
    assert number_format.format_number(1234.5, 2) == "1.234,50"


def test_format_number_none_is_n_d():
    assert number_format.format_number(None) == "n/d"


def test_format_number_small_value_no_separator_needed():
    assert number_format.format_number(42) == "42"


def test_format_abbreviated_trillion_scale():
    # Il valore reale che ha innescato il bug segnalato dall'utente: la market
    # cap intera (4786462654464 ≈ 4,79 * 10^12) va abbreviata in TRILIONI
    # ("T"), non in miliardi ("B") — la scala deve essere scelta dinamicamente
    # in base alla grandezza del numero, non fissata a priori.
    assert number_format.format_abbreviated(4786462654464) == "4,79T"


def test_format_abbreviated_billion_scale():
    assert number_format.format_abbreviated(2_500_000_000) == "2,50B"


def test_format_abbreviated_million_scale():
    assert number_format.format_abbreviated(15_000_000) == "15,00M"


def test_format_abbreviated_thousand_scale():
    assert number_format.format_abbreviated(4_200) == "4,20K"


def test_format_abbreviated_below_thousand_is_plain_number():
    assert number_format.format_abbreviated(42) == "42"


def test_format_abbreviated_none_is_n_d():
    assert number_format.format_abbreviated(None) == "n/d"


def test_format_abbreviated_negative_value_keeps_sign():
    assert number_format.format_abbreviated(-2_500_000_000) == "-2,50B"
