"""Mappa statica settore→peer (curata a mano) — yfinance non garantisce una
vera lista di peer per settore, meglio una lista piccola e controllata che un
automatismo fragile. Vedi Docs/superpowers/specs/2026-07-22-financial-markets-
stock-report-design.md §3.
"""

# Settore (come riportato da yfinance in Fundamentals['sector']) → 3-5 ticker
# peer noti. Lista aperta: aggiungere una voce qui non richiede modifiche a
# nessun altro file.
SECTOR_PEERS: dict[str, list[str]] = {
    "Technology": ["AAPL", "MSFT", "GOOGL", "NVDA"],
    "Healthcare": ["JNJ", "PFE", "NVO", "UNH"],
    "Consumer Cyclical": ["AMZN", "TSLA", "HD", "MCD"],
    "Financial Services": ["JPM", "BAC", "GS", "V"],
    "Energy": ["XOM", "CVX", "COP"],
    "Industrials": ["CAT", "BA", "GE"],
    "Communication Services": ["GOOGL", "META", "NFLX", "DIS"],
}


def peers_for_sector(sector: str, exclude_ticker: str, limit: int = 4) -> list[str]:
    """Ritorna fino a `limit` peer del settore, mai il ticker stesso. Settore
    non mappato → lista vuota (mai un automatismo di fallback fragile)."""
    candidates = SECTOR_PEERS.get(sector, [])
    return [t for t in candidates if t != exclude_ticker][:limit]
