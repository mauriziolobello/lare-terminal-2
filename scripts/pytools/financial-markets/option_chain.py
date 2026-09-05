"""Tabella option chain vera: call e put affiancate per strike, ritagliata
attorno allo strike più vicino al prezzo attuale (ATM) — non l'intera
scadenza, che per un titolo liquido può avere centinaia di strike (refinement
2026-07-23, dopo che la prima versione mostrava tutto senza limiti)."""

import number_format

# Ordine colonne confermato dall'utente: IV/Bid/Ask/Ultimo per il lato call,
# poi lo strike, poi IV/Bid/Ask/Ultimo per il lato put — stesso sotto-ordine
# per entrambi i lati, non mirror-ato.
_SUBCOLUMNS = ("iv", "bid", "ask", "last_price")
_SUBCOLUMN_LABELS = {"iv": "IV", "bid": "Bid", "ask": "Ask", "last_price": "Ultimo"}

# Sfondo della riga ATM — un blu leggermente meno scuro dello sfondo scuro
# di base dell'app (`charts.py::_DARK_BG`, "#141824") — richiesto in coppia
# col grassetto, che da solo non risaltava abbastanza nella finestra scura.
_ATM_ROW_BG = "#1f3a5c"


def build_rows(options: list[dict], current_price: float | None, window: int = 10) -> list[dict]:
    """Accoppia call/put per strike, poi ritaglia a `window` strike sopra e
    `window` sotto quello più vicino a `current_price` (ATM) — `None` per il
    lato mancante quando uno strike ha solo call o solo put quotati.
    `current_price=None` (dato non disponibile) ritorna tutto, ordinato,
    nessuno strike marcato ATM. Ogni riga porta `is_atm` (bool): per
    costruzione (ritaglio simmetrico) è la riga centrale della tabella —
    `render_table` la evidenzia in grassetto."""
    by_strike: dict[float, dict] = {}
    for opt in options:
        row = by_strike.setdefault(opt["strike"], {"strike": opt["strike"], "call": None, "put": None})
        row[opt["option_type"]] = opt
    strikes = sorted(by_strike)
    if not strikes:
        return []
    if current_price is None:
        selected = strikes
        atm_strike = None
    else:
        atm_idx = min(range(len(strikes)), key=lambda i: abs(strikes[i] - current_price))
        atm_strike = strikes[atm_idx]
        lo = max(0, atm_idx - window)
        hi = min(len(strikes), atm_idx + window + 1)
        selected = strikes[lo:hi]
    rows = []
    for s in selected:
        row = by_strike[s]
        row["is_atm"] = s == atm_strike
        rows.append(row)
    return rows


def _format_side(contract: dict | None) -> list[str]:
    if contract is None:
        return ["n/d", "n/d", "n/d", "n/d"]
    iv = contract.get("iv")
    iv_str = number_format.format_number(iv * 100, 2) + "%" if iv is not None else "n/d"
    return [
        iv_str,
        number_format.format_number(contract.get("bid"), 2),
        number_format.format_number(contract.get("ask"), 2),
        number_format.format_number(contract.get("last_price"), 2),
    ]


def render_table(rows: list[dict]) -> str:
    """HTML (non Markdown a pipe: una tabella GFM non può avere una cella
    che si espande su più colonne) — intestazione a due righe: la prima ha
    "CALL" su 4 colonne, una cella vuota in corrispondenza di "Strike",
    "PUT" sulle ultime 4; la seconda porta le sole etichette (IV/Bid/Ask/
    Ultimo/Strike, senza ripetere "Call"/"Put" — il raggruppamento lo dice
    già la riga sopra). La finestra Markdown la rende via marked.js +
    DOMPurify (`window.js`): entrambi passano/preservano HTML `<table>`
    semplice, incluso `colspan`/`style` (verificato dal vivo). La riga ATM
    (per costruzione quella centrale — vedi `build_rows`) è in grassetto
    (`<strong>`, non `**...**`: dentro un blocco HTML grezzo il Markdown
    non viene ri-analizzato) E ha uno sfondo leggermente più chiaro
    (`_ATM_ROW_BG`) — il grassetto da solo non risaltava abbastanza."""
    if not rows:
        return "Nessuno strike disponibile per questa scadenza."
    labels = [_SUBCOLUMN_LABELS[c] for c in _SUBCOLUMNS]
    label_cells = "".join(f"<th>{label}</th>" for label in labels)
    lines = [
        "<table>",
        "<thead>",
        '<tr><th colspan="4">CALL</th><th></th><th colspan="4">PUT</th></tr>',
        f"<tr>{label_cells}<th>Strike</th>{label_cells}</tr>",
        "</thead>",
        "<tbody>",
    ]
    for row in rows:
        cells = [*_format_side(row["call"]), number_format.format_number(row["strike"], 2), *_format_side(row["put"])]
        if row.get("is_atm"):
            cells_html = "".join(f"<td><strong>{cell}</strong></td>" for cell in cells)
            lines.append(f'<tr style="background-color:{_ATM_ROW_BG}">{cells_html}</tr>')
        else:
            cells_html = "".join(f"<td>{cell}</td>" for cell in cells)
            lines.append(f"<tr>{cells_html}</tr>")
    lines.append("</tbody>")
    lines.append("</table>")
    return "\n".join(lines)
