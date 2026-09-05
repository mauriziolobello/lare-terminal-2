"""Refresh della cache locale ticker USA — sorgente ufficiale SEC.gov
(company_tickers.json), niente scraping non ufficiale. Vedi
Docs/superpowers/specs/2026-07-22-financial-markets-stock-report-design.md §6.

Uso:
    python refresh_tickers.py                    # refresh verso tickers_us.json
    python refresh_tickers.py --dest altro.json   # refresh verso un altro path
"""

import argparse
import json
import time
import urllib.request
from pathlib import Path

SEC_URL = "https://www.sec.gov/files/company_tickers.json"
# SEC richiede un User-Agent identificativo per le richieste automatizzate
# (non un browser finto) — senza, le richieste vengono rifiutate/rallentate.
USER_AGENT = "Lare Terminal financial-markets tool (contatto: mauriziolobello@gmail.com)"
STALE_AFTER_SECONDS = 7 * 24 * 60 * 60  # 7 giorni, deciso in brainstorming


def fetch_raw(url: str = SEC_URL) -> dict:
    """Scarica il JSON grezzo da SEC.gov. Nessun retry: un fallimento di rete
    deve propagare un errore leggibile, non restare silenzioso."""
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.load(response)


def normalize(raw: dict) -> list[dict]:
    """Trasforma il formato SEC ({'0': {...}, '1': {...}}) in una lista piatta
    [{'ticker': ..., 'name': ...}, ...] — quello che ticker_search.py consuma."""
    return [{"ticker": entry["ticker"], "name": entry["title"]} for entry in raw.values()]


def refresh(dest: Path, url: str = SEC_URL) -> int:
    """Scarica, normalizza, scrive `dest`. Ritorna il numero di ticker scritti."""
    entries = normalize(fetch_raw(url))
    dest.parent.mkdir(parents=True, exist_ok=True)
    dest.write_text(json.dumps(entries, ensure_ascii=False, indent=2), encoding="utf-8")
    return len(entries)


def is_stale(dest: Path, max_age_seconds: int = STALE_AFTER_SECONDS) -> bool:
    """True se `dest` non esiste, oppure è più vecchio di `max_age_seconds`."""
    if not dest.exists():
        return True
    age = time.time() - dest.stat().st_mtime
    return age > max_age_seconds


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dest", default="tickers_us.json", help="Percorso di destinazione della cache")
    args = parser.parse_args()
    count = refresh(Path(args.dest))
    print(f"Scritti {count} ticker in {args.dest}")
