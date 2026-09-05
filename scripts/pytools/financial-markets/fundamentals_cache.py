"""Cache persistente dei fondamentali per screen_stocks — load/save JSON,
staleness 7 giorni, selezione candidati (zoccolo indici prioritario +
campione casuale fuori-indice), budget di fetch per esecuzione. Il fetch
vero è una funzione INIETTATA (in produzione
YFinanceDataSource.fetch_screening_snapshot): questo modulo non tocca mai
la rete, ed è interamente testabile con fake. Vedi Docs/superpowers/specs/
2026-07-26-financial-markets-screen-stocks-design.md §5."""

import json
import random
from datetime import date, timedelta
from pathlib import Path

MAX_AGE_DAYS = 7
FETCH_BUDGET = 60

# Le chiavi dati di un entry (oltre a fetched_at) — usate per costruire
# l'entry "fallito ma visitato" con tutti i campi null.
_SNAPSHOT_KEYS = ("sector", "market_cap", "ps_ratio", "revenue_growth",
                  "fifty_two_week_high", "current_price", "target_mean_price",
                  "peg_ratio", "pe_ratio", "debt_to_equity", "dividend_yield",
                  "payout_ratio", "target_high_price", "target_low_price",
                  "industry")


def load_cache(path: Path) -> dict[str, dict]:
    if not path.exists():
        return {}
    return json.loads(path.read_text(encoding="utf-8"))


def save_cache(path: Path, cache: dict[str, dict]) -> None:
    path.write_text(json.dumps(cache, ensure_ascii=False), encoding="utf-8")


def is_fresh(entry: dict, today: date, max_age_days: int = MAX_AGE_DAYS) -> bool:
    """A `max_age_days` compiuti l'entry è scaduto (stessa semantica di
    refresh_tickers.is_stale: 7 giorni esatti → rinfresca)."""
    fetched = date.fromisoformat(entry["fetched_at"])
    return today - fetched < timedelta(days=max_age_days)


def select_candidates(index_tickers: list[str], all_tickers: list[str],
                      cache: dict, today: date, rng: random.Random,
                      budget: int = FETCH_BUDGET) -> list[str]:
    """Chi fetchare in QUESTA esecuzione: prima lo zoccolo indici mancante o
    scaduto (nell'ordine della lista, deterministico), poi — fino a
    esaurimento del budget — un campione casuale dei ticker fuori-indice
    mancanti o scaduti. Il campione cambia a ogni run (rng non seedato in
    produzione): è l'esplorazione progressiva dell'universo."""
    def needs_fetch(ticker: str) -> bool:
        entry = cache.get(ticker)
        return entry is None or not is_fresh(entry, today)

    candidates = [t for t in index_tickers if needs_fetch(t)][:budget]

    remaining = budget - len(candidates)
    if remaining > 0:
        index_set = set(index_tickers)
        outside = [t for t in all_tickers if t not in index_set and needs_fetch(t)]
        sample_size = min(remaining, len(outside))
        candidates.extend(rng.sample(outside, sample_size))
    return candidates


def run_fetch(cache: dict, candidates: list[str], fetch_fn, today: date) -> dict:
    """Esegue il fetch per ogni candidato, aggiornando `cache` in place.
    QUALUNQUE eccezione da fetch_fn = "fetch fallito": l'entry viene scritto
    con campi null e fetched_at odierno — conta come visitato (non riprovato
    a ogni run finché non scade), mai un'eccezione che uccida l'esecuzione."""
    stats = {"new": 0, "refreshed": 0, "failed": 0}
    for ticker in candidates:
        existed = ticker in cache
        try:
            snapshot = dict(fetch_fn(ticker))
        except Exception:
            snapshot = {key: None for key in _SNAPSHOT_KEYS}
            stats["failed"] += 1
        else:
            stats["refreshed" if existed else "new"] += 1
        snapshot["fetched_at"] = today.isoformat()
        cache[ticker] = snapshot
    return stats
