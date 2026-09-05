"""Cache persistente delle serie storiche OHLC per lo screener 'citadel' --
load/save JSON, staleness 1 giorno di borsa, selezione candidati (stesso
zoccolo indici + campione fuori-indice), budget di fetch per esecuzione.

Mirror STRUTTURALE di fundamentals_cache.py, MODULO SEPARATO -- non un
riuso: fundamentals_cache.run_fetch scrive i fallimenti in forma flat
(_SNAPSHOT_KEYS), incompatibile con la forma qui ({"fetched_at",
"bars": [...]}) -- verificato leggendo il file (spec
Docs/superpowers/specs/2026-08-26-screener-citadel-design.md §2/§7).

Il fetch vero e' una funzione INIETTATA (in produzione
lambda t: source.fetch_ohlc(t, "D")): questo modulo non tocca mai la rete,
interamente testabile con fake."""

import json
import os
import random
from datetime import date, timedelta
from pathlib import Path

MAX_AGE_DAYS = 1
FETCH_BUDGET = 60


def load_cache(path: Path) -> dict[str, dict]:
    """Legge il JSON della cache -- un file assente e' "cache vuota", mai un
    errore (bootstrap normale). Un file PRESENTE ma corrotto (crash a meta'
    scrittura, disco pieno, ecc.) degrada allo stesso modo: "riparti da
    zero" invece di far crashare ogni run successivo con una
    JSONDecodeError non gestita -- con save_cache ora atomica (sotto)
    questo caso dovrebbe essere raro, ma la difesa costa poco e vale la
    pena tenerla comunque."""
    if not path.exists():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return {}


def save_cache(path: Path, cache: dict[str, dict]) -> None:
    """Scrittura ATOMICA: prima su un file temporaneo nella STESSA cartella
    (stesso filesystem -- richiesto perche' os.replace() sia atomico), poi
    os.replace() lo sostituisce al path reale in un colpo solo. Con ~440
    ticker x ~126 barre questo file e' di alcuni MB e viene riscritto ad
    ogni run: una scrittura diretta interrotta a meta' (crash, kill,
    disco pieno) lascerebbe un JSON invalido senza alcun modo di
    recuperare. Con questo schema, o il file vecchio resta intatto (crash
    prima di os.replace()), o il nuovo e' completo (crash dopo) -- mai una
    via di mezzo corrotta."""
    temp_path = path.parent / (path.name + ".tmp")
    temp_path.write_text(json.dumps(cache, ensure_ascii=False), encoding="utf-8")
    os.replace(temp_path, path)


def is_fresh(entry: dict, today: date, max_age_days: int = MAX_AGE_DAYS) -> bool:
    """A `max_age_days` compiuti l'entry e' scaduto -- con MAX_AGE_DAYS=1,
    fresco solo se fetchato OGGI stesso (a differenza dei 7 giorni dei
    fondamentali: il prezzo cambia ogni giorno di borsa)."""
    fetched = date.fromisoformat(entry["fetched_at"])
    return today - fetched < timedelta(days=max_age_days)


def select_candidates(index_tickers: list[str], all_tickers: list[str],
                      cache: dict, today: date, rng: random.Random,
                      budget: int = FETCH_BUDGET) -> list[str]:
    """Identica logica di fundamentals_cache.select_candidates (zoccolo
    indici prioritario, poi campione fuori-indice fino a budget) --
    duplicata perche' e' un modulo separato, non per una differenza di
    comportamento."""
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
    QUALUNQUE eccezione da fetch_fn = "fetch fallito": l'entry viene
    scritta con fetched_at odierno -- conta come visitato (non riprovato a
    ogni run finche' non scade) -- ma le barre gia' in cache vengono
    PRESERVATE (mai azzerate da un fallimento transitorio: a differenza
    di fundamentals_cache.py, qui il dato e' storia CUMULATIVA, non uno
    snapshot puntuale -- un blip di rete non deve cancellare mesi di
    prezzi). Un ticker mai visto prima (nessuna entry preesistente) non ha
    nulla da preservare -> bars=[]."""
    stats = {"new": 0, "refreshed": 0, "failed": 0}
    for ticker in candidates:
        existed = ticker in cache
        try:
            bars = list(fetch_fn(ticker))
        except Exception:
            bars = cache[ticker]["bars"] if existed else []
            stats["failed"] += 1
        else:
            stats["refreshed" if existed else "new"] += 1
        cache[ticker] = {"fetched_at": today.isoformat(), "bars": bars}
    return stats
