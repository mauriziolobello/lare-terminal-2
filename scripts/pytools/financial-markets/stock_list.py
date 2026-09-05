"""Elenco tabellare titoli per il tool list_stocks — join locale fra la
cache ticker già in memoria (server.py::_tickers) e le liste statiche di
appartenenza indice (index_membership.py). Nessuna chiamata di rete. Vedi
Docs/superpowers/specs/2026-07-24-financial-markets-list-stocks-design.md.
"""

import index_membership

SUPPORTED_COUNTRIES = ("USA",)


def is_country_supported(country: str) -> bool:
    return country.upper() in SUPPORTED_COUNTRIES


def unsupported_country_message(country: str) -> str:
    return f"Al momento è supportato solo il mercato USA (richiesto: {country})."


def build_rows(tickers: list[dict], country: str) -> list[dict]:
    """`tickers` = il contenuto già caricato di tickers_us.json (lista di
    {'ticker','name'}). Ordina per Nome, alfabetico."""
    # country arriva dall'AI/utente e può essere in qualunque casing
    # ("usa", "Usa", ...) — normalizzato in maiuscolo per la colonna Paese.
    country = country.upper()
    rows = [
        {
            "name": t["name"],
            "asset": t["ticker"],
            "country": country,
            "index": index_membership.indices_for(t["ticker"]),
        }
        for t in tickers
    ]
    return sorted(rows, key=lambda r: r["name"])


def render_table(rows: list[dict]) -> str:
    """Markdown a pipe: | Nome | Asset | Paese | Indice |. Righe vuote → un
    placeholder leggibile, stesso principio di option_chain.render_table."""
    if not rows:
        return "Nessun titolo trovato."
    lines = ["| Nome | Asset | Paese | Indice |", "|---|---|---|---|"]
    for row in rows:
        lines.append(f"| {row['name']} | {row['asset']} | {row['country']} | {row['index']} |")
    return "\n".join(lines)
