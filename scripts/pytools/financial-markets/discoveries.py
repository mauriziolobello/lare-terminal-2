"""Ledger delle scoperte per screen_stocks — memoria di chi è già apparso
in una selezione, per la finestra di esclusione 30gg (ogni run mostra facce
nuove) e per marcare le prime apparizioni assolute (★). Vedi
Docs/superpowers/specs/2026-07-26-financial-markets-screen-stocks-design.md
§7.3-7.4. La data "oggi" è SEMPRE un parametro: questo modulo non chiama
mai date.today() — determinismo nei test."""

import json
from datetime import date, timedelta
from pathlib import Path

EXCLUSION_WINDOW_DAYS = 30


def load_ledger(path: Path) -> dict[str, str]:
    if not path.exists():
        return {}
    return json.loads(path.read_text(encoding="utf-8"))


def save_ledger(path: Path, ledger: dict[str, str]) -> None:
    path.write_text(json.dumps(ledger, ensure_ascii=False), encoding="utf-8")


def excluded(ledger: dict[str, str], today: date,
             window_days: int = EXCLUSION_WINDOW_DAYS) -> set[str]:
    """Ticker apparsi da MENO di `window_days` giorni: esclusi dalla
    selezione odierna. A finestra compiuta (>= 30 giorni) si è riammessi."""
    return {
        ticker for ticker, shown in ledger.items()
        if today - date.fromisoformat(shown) < timedelta(days=window_days)
    }


def is_first_appearance(ledger: dict[str, str], ticker: str) -> bool:
    return ticker not in ledger


def record(ledger: dict[str, str], tickers: list[str], today: date) -> None:
    for ticker in tickers:
        ledger[ticker] = today.isoformat()
