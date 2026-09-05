# scripts/pytools/ — tool esterni Python (MCP)

Vedi `Docs/superpowers/specs/2026-07-21-pytools-infrastructure-design.md` per il design completo
(scritto quando questa cartella si chiamava ancora `pytools/` in radice — rinominata
`scripts/pytools/` il 2026-07-24, la convenzione di risoluzione sotto è invariata).

Ogni sottocartella è un **ambito/dominio**: uno o più script/server MCP che condividono UN SOLO
virtual environment. Non condividere un venv tra ambiti diversi (es. `python-ping` e
`financial-markets` restano separati) — ma più script dello STESSO ambito condividono lo stesso
venv (niente duplicazione di dipendenze pesanti come pandas/numpy).

## Creare un nuovo ambito (o il venv di uno esistente)

```powershell
cd scripts/pytools/<nome-ambito>
python -m venv venv
venv\Scripts\Activate.ps1
pip install -r requirements.txt
```

Il venv (`venv/`) non va mai committato — è in `.gitignore`. Lo script/server MCP (`.py`,
`requirements.txt`) sì.

## Convenzione di risoluzione (lato Rust — `PythonMcpToolClient::resolve`)

Una formula sola, simmetrica: `<root>` = la RADICE `scripts/pytools/` (mai la cartella di un
ambito specifico), presa da `LARE_PYTOOLS_DIR` se la variabile è impostata, altrimenti da
`pytools/` sibling dell'eseguibile orchestrator corrente (nome invariato: è la convenzione della
cartella copiata accanto al binario in un DEPLOY, indipendente da dove vive il codice sorgente nel
repo — vedi `deploy_binary_only.ps1`). `domain_id` viene sempre unito sopra `<root>`, in entrambi
i casi:

- interprete: `<root>/<domain_id>/venv/Scripts/python.exe` (Windows) o
  `<root>/<domain_id>/venv/bin/python3` (Unix)
- script: `<root>/<domain_id>/<script_relpath>`

Punta quindi la variabile alla cartella `scripts/pytools/` stessa (es.
`LARE_PYTOOLS_DIR=...\scripts\pytools`), MAI alla cartella di un singolo ambito
(`...\scripts\pytools\python-ping` sarebbe sbagliato) — è la stessa variabile che ogni ambito
futuro condivide, `domain_id` distingue quale sottocartella usare.

**Nel workflow di sviluppo normale (`cargo run -p orchestrator`) il ramo di default non risolve
mai**: nulla copia `scripts/pytools/` accanto al binario compilato (niente `build.rs`, nessuna
risorsa Tauri che lo faccia). `LARE_PYTOOLS_DIR` va quindi impostata SEMPRE in dev, puntando alla
cartella `scripts/pytools/` del repository:

```powershell
# Da eseguire dalla radice del repository (dove sta anche questa cartella scripts/pytools/)
$env:LARE_PYTOOLS_DIR="$PWD\scripts\pytools"
```

## Ambiti esistenti

- `python-ping/` — tool di prova per validare l'infrastruttura (nessuna dipendenza di dominio).
- `financial-markets/` — report azionario USA (`/markets`): fondamentale, grafici, option chain,
  comparative (dati fattuali) + narrativa/ipotesi di investimento (scritte dall'AI). Vedi
  `Docs/superpowers/specs/2026-07-22-financial-markets-stock-report-design.md`.
