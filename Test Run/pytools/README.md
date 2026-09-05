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

> Nota (2.0): questa sezione descriveva, in v1, una variabile d'ambiente `LARE_PYTOOLS_DIR`. Il
> resolver Rust è stato aggiornato (Task 4/5, D6: nessuna variabile d'ambiente) — il testo sotto
> riflette il meccanismo attuale; vedi `crates/startup-config/src/lib.rs`
> (`StartupConfig::resolve_path`, `Paths::pytools_dir`) e
> `crates/orchestrator/src/python_mcp_tool_client.rs` (`PythonMcpToolClient::resolve`).

Una formula sola, simmetrica: `<root>` = la RADICE `scripts/pytools/` (mai la cartella di un
ambito specifico), presa da `startup.json.paths.pytools_dir` (default `"pytools"`) se il valore è
un percorso assoluto, altrimenti risolta rispetto alla RADICE DEL DEPLOY (la cartella che contiene
`Configuration/`, cioè il genitore della cartella di configurazione passata via `--config-dir` —
vedi la sezione successiva). `domain_id` viene sempre unito sopra `<root>`:

- interprete: `<root>/<domain_id>/venv/Scripts/python.exe` (Windows) o
  `<root>/<domain_id>/venv/bin/python3` (Unix)
- script: `<root>/<domain_id>/<script_relpath>`

**Nel workflow di sviluppo normale** l'orchestratore si lancia con un `--config-dir` esplicito
(vedi `CLAUDE.md`, sezione avvio in sviluppo):

```powershell
cargo run -p orchestrator -- --config-dir "Test Run\Configuration" --console-log
```

La radice del deploy è quindi il genitore di quella cartella (`Test Run\`), e il default
`pytools_dir: "pytools"` risolve a `Test Run\pytools\` (con un venv per dominio dentro, es.
`Test Run\pytools\financial-markets\venv\`), NON a `scripts/pytools/` del repository
direttamente. Per puntare invece al sorgente in `scripts/pytools/` senza copiare nulla, imposta
`paths.pytools_dir` in `Test Run\Configuration\startup.json` a un percorso assoluto:

```json
{ "paths": { "pytools_dir": "C:/.../Lare Terminal 2.0/scripts/pytools" } }
```

## Cartella di configurazione: `--config-dir`

Ogni server MCP Python (`server.py` di un dominio) riceve dall'orchestratore, allo spawn, un
argomento a riga di comando: `--config-dir <dir>` (2° argomento dopo il percorso dello script —
vedi `PythonMcpToolClient::ensure_connected`). È la cartella di configurazione di Lare (2.0, D6:
"nessuna variabile d'ambiente") — dove vivono file come `market_data.json` — e va letta con
`config_dir.resolve()` (`financial-markets/config_dir.py`), MAI con una `os.environ.get(...)`:
questo è l'UNICO modo in cui il processo Python conosce quella cartella.

`config_dir.resolve(argv=None)` (default `argv=sys.argv`) capisce sia `--config-dir <dir>` (due
argomenti) sia `--config-dir=<dir>` (uno solo, con `=`). Se il flag non è presente — es. lanciando
lo script a mano durante lo sviluppo, senza passare per l'orchestratore — il default è
`<radice del deploy>/Configuration`, dove la radice del deploy è la cartella che contiene
`pytools/` (ogni `server.py` di dominio vive in `pytools/<dominio>/`, due livelli sotto la radice).

Un server MCP che non ha bisogno di leggere configurazione (`python-ping`) accetta comunque il
flag per uniformità — l'orchestratore lo passa a OGNI server Python, che lo usi o no — ma non lo
elabora: FastMCP non tocca `sys.argv`, quindi l'argomento extra non causa mai un errore
"unrecognized arguments".

Nella v1 questa cartella veniva letta dalla variabile d'ambiente `LARE_LOCAL_DIR` (ereditata dal
processo padre orchestrator) con fallback su `%LOCALAPPDATA%` — un terzo punto di lettura
indipendente da quelli Rust/UI, che poteva divergere dagli altri due. La 2.0 elimina ogni
variabile d'ambiente `LARE_*`: `--config-dir` è l'unico canale, passato esplicitamente ad ogni
spawn.

## Ambiti esistenti

- `python-ping/` — tool di prova per validare l'infrastruttura (nessuna dipendenza di dominio).
- `financial-markets/` — report azionario USA (`/markets`): fondamentale, grafici, option chain,
  comparative (dati fattuali) + narrativa/ipotesi di investimento (scritte dall'AI). Vedi
  `Docs/superpowers/specs/2026-07-22-financial-markets-stock-report-design.md`.
