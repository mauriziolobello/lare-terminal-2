"""Test per peers.py — nessuna rete, solo lookup sulla mappa statica."""

import peers


def test_peers_for_known_sector_excludes_the_ticker_itself():
    result = peers.peers_for_sector("Technology", exclude_ticker="AAPL")
    assert "AAPL" not in result
    assert "MSFT" in result


def test_peers_for_unknown_sector_returns_empty_list():
    assert peers.peers_for_sector("Settore Inesistente", exclude_ticker="X") == []


def test_peers_respects_limit():
    result = peers.peers_for_sector("Technology", exclude_ticker="ZZZ", limit=2)
    assert len(result) == 2
