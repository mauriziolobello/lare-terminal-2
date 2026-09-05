import json

import market_data_config
from data_sources.yfinance_source import YFinanceDataSource


def test_build_data_source_defaults_to_yfinance_when_file_absent(tmp_path):
    source = market_data_config.build_data_source(tmp_path / "market_data.json")
    assert isinstance(source, YFinanceDataSource)


def test_build_data_source_defaults_to_yfinance_when_file_malformed(tmp_path):
    path = tmp_path / "market_data.json"
    path.write_text("not json")
    source = market_data_config.build_data_source(path)
    assert isinstance(source, YFinanceDataSource)


def test_build_data_source_reads_yfinance_explicitly(tmp_path):
    path = tmp_path / "market_data.json"
    path.write_text(json.dumps({"active": "yfinance", "sources": [{"kind": "yfinance"}]}))
    source = market_data_config.build_data_source(path)
    assert isinstance(source, YFinanceDataSource)


def test_build_data_source_builds_ibkr_source_for_tws(tmp_path):
    path = tmp_path / "market_data.json"
    path.write_text(json.dumps({
        "active": "tws",
        "sources": [
            {"kind": "yfinance"},
            {"kind": "tws", "port": 7496, "client_id": 731},
        ],
    }))
    source = market_data_config.build_data_source(path)
    # Task 4 introduce IbkrDataSource -- import qui sotto per evitare un
    # ciclo di import prima che esista (questo test fallisce a RED per
    # ImportError finché IbkrDataSource non esiste, atteso).
    from data_sources.ibkr_source import IbkrDataSource
    assert isinstance(source, IbkrDataSource)
    assert source.port == 7496
    assert source.client_id == 731


def test_build_data_source_falls_back_to_yfinance_for_unknown_active(tmp_path):
    path = tmp_path / "market_data.json"
    path.write_text(json.dumps({"active": "bogus", "sources": [{"kind": "yfinance"}]}))
    source = market_data_config.build_data_source(path)
    assert isinstance(source, YFinanceDataSource)


def test_build_data_source_defaults_to_yfinance_when_json_is_array(tmp_path):
    """Valido JSON ma non un dict — fallback a YFinance."""
    path = tmp_path / "market_data.json"
    path.write_text(json.dumps([]))
    source = market_data_config.build_data_source(path)
    assert isinstance(source, YFinanceDataSource)


def test_build_data_source_defaults_to_yfinance_when_json_is_null(tmp_path):
    """Valido JSON ma non un dict — fallback a YFinance."""
    path = tmp_path / "market_data.json"
    path.write_text(json.dumps(None))
    source = market_data_config.build_data_source(path)
    assert isinstance(source, YFinanceDataSource)


def test_build_data_source_defaults_to_yfinance_when_sources_entries_are_not_dicts(tmp_path):
    """Fix C (review-fix-wave, 2026-08-14): 'sources' e' una lista valida ma
    contiene elementi che non sono dict (es. una stringa nuda, JSON valido
    scritto a mano). Prima del fix, `s.get("kind")` su una str non-dict
    solleva AttributeError NON catturato a livello di IMPORT di server.py
    (build_data_source e' chiamato a livello modulo) -- l'intero processo
    Python muore, ogni tool del canale smette di funzionare. Fallback atteso:
    stessa degradazione a YFinance gia' usata per gli altri casi malformati
    sopra, mai un'eccezione."""
    path = tmp_path / "market_data.json"
    path.write_text(json.dumps({"active": "yfinance", "sources": ["yfinance"]}))
    source = market_data_config.build_data_source(path)
    assert isinstance(source, YFinanceDataSource)
