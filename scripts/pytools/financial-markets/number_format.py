"""Formattazione numerica in stile it-IT (punto per le migliaia, virgola
per i decimali) — usata da report.py per rendere leggibili market cap,
ricavi/utili e altri numeri grossi che altrimenti apparirebbero come
`4786462654464` senza alcun separatore."""

_SCALES = ((1e12, "T"), (1e9, "B"), (1e6, "M"), (1e3, "K"))

# f"{n:,.Nf}" produce la convenzione en-US (virgola=migliaia, punto=decimali).
# str.translate scambia i due separatori in UN solo passaggio, guardando
# sempre i caratteri della stringa ORIGINALE: a differenza di due
# .replace() sequenziali, non rischia di ri-sostituire un carattere già
# scritto dal replace precedente.
_SEP_SWAP = str.maketrans({",": ".", ".": ","})


def format_number(n: float | int | None, decimals: int = 0) -> str:
    """`1234567` (decimals=0) → `"1.234.567"`; `1234.5` (decimals=2) →
    `"1.234,50"`. `None` → `"n/d"` (dato non disponibile, mai un errore)."""
    if n is None:
        return "n/d"
    return f"{n:,.{decimals}f}".translate(_SEP_SWAP)


def format_abbreviated(n: float | int | None) -> str:
    """`4786462654464` → `"4,79T"` (scala dinamica K/M/B/T, mai un'unica
    scala fissa — un valore in miliardi non deve mai essere abbreviato
    come se fosse in migliaia). `None` → `"n/d"`."""
    if n is None:
        return "n/d"
    for scale, suffix in _SCALES:
        if abs(n) >= scale:
            return format_number(n / scale, 2) + suffix
    return format_number(n, 0)
