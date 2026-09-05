"""Risolve la cartella dati locale di Lare -- mirror MINIMO (2 livelli, non 3)
del resolver Rust `startup_config::resolve`/`default_local_dir`: env var
LARE_LOCAL_DIR se impostata (ereditata dal processo padre orchestrator,
niente nuova plumbing di spawn), altrimenti %LOCALAPPDATA%\\dev.lare.terminal\\.

NON legge startup.json -- fuori scope in questa fase (vedi CLAUDE.md, caveat
fase 1/2: orchestrator+ui possono gia' divergere se local_dir e' impostata
SOLO in startup.json; questo resolver Python e' un terzo punto di lettura,
stesso caveat, usa la env var se vuoi un local_dir non-default)."""

import os
from pathlib import Path


def resolve() -> Path:
    """Resolve the local data directory for Lare.

    Returns:
        Path to the local data directory. Priority:
        1. LARE_LOCAL_DIR env var (if set and non-empty)
        2. %LOCALAPPDATA%\\dev.lare.terminal\\ (fallback)
        3. .lare-data (final fallback if LOCALAPPDATA also missing)
    """
    env_value = os.environ.get("LARE_LOCAL_DIR", "").strip()
    if env_value:
        return Path(env_value)
    local_appdata = os.environ.get("LOCALAPPDATA", "").strip()
    if local_appdata:
        return Path(local_appdata) / "dev.lare.terminal"
    return Path(".lare-data")
