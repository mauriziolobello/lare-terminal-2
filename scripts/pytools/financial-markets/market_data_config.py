"""Legge market_data.json e costruisce la DataSource attiva. Stesso file
scritto da ui (Tauri, market_data_settings.rs) -- letto qui direttamente,
nessuna nuova plumbing lato orchestrator (vedi local_dir.py). Default SEMPRE
YFinance su file assente/malformato/kind sconosciuto -- nessuna regressione
per chi non ha mai toccato /config."""

import json
from pathlib import Path

from data_sources.yfinance_source import YFinanceDataSource

# Costante Lare per clientId -- stessa usata come default quando
# market_data.json non specifica client_id per una fonte IB (vedi
# Global Constraints del piano). Import locale in _build_ibkr per evitare
# un import di ib_async quando la fonte attiva e' yfinance (nessuna
# dipendenza a runtime da una libreria non necessaria).
DEFAULT_IBKR_CLIENT_ID = 731


def build_data_source(market_data_json_path: Path):
    """Build the active DataSource from market_data.json configuration.

    Reads market_data.json at the given path, extracts the 'active' source
    and its entry from 'sources' list, and instantiates the corresponding
    DataSource class.

    Defaults to YFinanceDataSource on:
    - File not found
    - JSON parsing error
    - JSON is valid but not a dict (e.g., [], null, "string")
    - Active source entry not found in sources list
    - Active source kind not recognized

    Args:
        market_data_json_path: Path to market_data.json file

    Returns:
        Instantiated DataSource (YFinanceDataSource or IbkrDataSource)
    """
    try:
        content = market_data_json_path.read_text()
        config = json.loads(content)
    except (OSError, json.JSONDecodeError):
        return YFinanceDataSource()

    # Garantisci che config è un dict (valido JSON ma non un dict = malformato)
    if not isinstance(config, dict):
        return YFinanceDataSource()

    active = config.get("active", "yfinance")
    sources = config.get("sources", [])

    # Garantisci che sources è una lista (se qualcuno ha scritto un JSON malformato)
    if not isinstance(sources, list):
        return YFinanceDataSource()

    # Filtra elementi non-dict PRIMA di chiamare .get("kind") su di essi --
    # JSON valido come "sources": ["yfinance"] (stringa invece di oggetto)
    # farebbe sollevare AttributeError non catturato (s.get su una str),
    # a livello di IMPORT di server.py (questa funzione è chiamata a livello
    # modulo) -- l'intero processo Python morirebbe, ogni tool del canale
    # smetterebbe di funzionare, non solo IBKR.
    sources = [s for s in sources if isinstance(s, dict)]

    entry = next((s for s in sources if s.get("kind") == active), None)
    if entry is None:
        return YFinanceDataSource()

    if active == "yfinance":
        return YFinanceDataSource()
    if active in ("tws", "ib_gateway"):
        from data_sources.ibkr_source import IbkrDataSource
        port = entry.get("port") or (7496 if active == "tws" else 4001)
        client_id = entry.get("client_id") or DEFAULT_IBKR_CLIENT_ID
        return IbkrDataSource(port=port, client_id=client_id)

    return YFinanceDataSource()
