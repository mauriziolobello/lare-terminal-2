"""Cartella di configurazione di Lare (2.0): `--config-dir <path>` passato
dall'orchestratore come argomento (dopo il percorso dello script), altrimenti
`<radice del deploy>/Configuration`, dove la radice del deploy è la cartella
che contiene `pytools/` (questo file vive in `pytools/<dominio>/`).

Nessuna variabile d'ambiente viene letta (decisione D6 dello spec 2.0): nella
v1 questo modulo rispecchiava la env var `LARE_LOCAL_DIR` ereditata dal padre,
un terzo lettore indipendente che poteva divergere dagli altri due.
"""

from __future__ import annotations

import sys
from pathlib import Path

FLAG = "--config-dir"


def resolve(argv: list[str] | None = None) -> Path:
    args = sys.argv if argv is None else argv
    it = iter(args)
    for a in it:
        if a == FLAG:
            value = next(it, "")
            if value.strip():
                return Path(value)
        elif a.startswith(FLAG + "="):
            value = a[len(FLAG) + 1:]
            if value.strip():
                return Path(value)
    return Path(__file__).resolve().parents[2] / "Configuration"
