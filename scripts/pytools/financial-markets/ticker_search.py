"""Ricerca ticker in cache locale — match parziale case-insensitive su nome
o ticker. Nessuna rete: opera solo sul file scritto da refresh_tickers.py.
Vedi Docs/superpowers/specs/2026-07-22-financial-markets-stock-report-design.md §5.
"""

import json
from pathlib import Path


def load_tickers(path: Path) -> list[dict]:
    """Carica la cache locale (formato: lista di {'ticker','name'})."""
    return json.loads(path.read_text(encoding="utf-8"))


def search(query: str, tickers: list[dict], limit: int = 10) -> list[dict]:
    """Match parziale case-insensitive su `name` O `ticker`. Query vuota →
    lista vuota (mai l'intera cache). Risultati troncati a `limit`."""
    normalized = query.strip().lower()
    if not normalized:
        return []
    matches = [
        entry
        for entry in tickers
        if normalized in entry["ticker"].lower() or normalized in entry["name"].lower()
    ]
    return matches[:limit]
