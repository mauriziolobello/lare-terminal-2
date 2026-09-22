# Strumenti Python (pytools)

Alcuni canali esterni di Lare Terminal si appoggiano, per una parte del proprio lavoro, a script
Python indipendenti dal resto del sistema — non moduli Rust, non plugin, un dominio a parte con le
proprie dipendenze e il proprio ambiente di esecuzione. Questo documento spiega cosa sono, come
sono organizzati e come si aggiunge un nuovo ambito. Per il quadro generale del progetto vedi
[`00-apertura.md`](./00-apertura.md); per i tre strati (canali → orchestratore → server di tool) in
cui i pytool restano incastrati, [`01-architettura.md`](./01-architettura.md).

## Cosa sono, e perché esistono in un progetto Rust/C#

Il resto del backend è Rust, la shell è C#, l'interfaccia è Tauri — pytools è un'eccezione
dichiarata a questa omogeneità. Esiste perché alcune capacità hanno, in Python, un ecosistema
maturo che non avrebbe senso reimplementare da zero: l'analisi di dati finanziari (`yfinance`,
`mplfinance`, `pandas`, un client per Interactive Brokers) per il dominio `financial-markets`, una
libreria dedicata al protocollo TR-064 dei router FRITZ!Box (`fritzconnection`) per `fritzbox`.
Riscrivere queste librerie in Rust per rispettare l'omogeneità del linguaggio sarebbe un lavoro
enorme per un beneficio nullo — la stessa logica, applicata altrove nel progetto, dietro la scelta
di tenere la host della shell in C# invece di ripartire da un binding a basso livello a
`Microsoft.PowerShell.SDK` (vedi [`01-architettura.md`](./01-architettura.md), "Scelte
tecnologiche").

Un pytool non è un modo per aggirare l'architettura a tre strati: resta sempre dietro un server di
tool, in un modo o nell'altro (vedi sotto) — mai un canale che parla direttamente all'AI o esegue
codice arbitrario. L'AI continua a vedere un insieme fisso di strumenti con nome, descrizione e
schema dei parametri; che l'implementazione dietro quello strumento sia Rust o un sottoprocesso
Python è un dettaglio che il livello sopra non vede.

## Due modi di collegare Python al resto del sistema

Esistono oggi due pattern diversi, scelti caso per caso in base a cosa quel dominio deve davvero
fare. Non ci sono regole scritte per la scelta, ma il criterio che emerge dai casi esistenti è
chiaro: uno stato da mantenere tra una chiamata e l'altra (o più tool sotto lo stesso ombrello) va
verso il primo pattern; una singola query senza stato, che si aggancia bene a un canale Rust già
esistente, verso il secondo.

**Server MCP persistente.** L'orchestratore stesso spawna il processo Python come server MCP e gli
parla direttamente in stdio, con lo stesso protocollo (via la libreria `rmcp`) con cui parla a
qualsiasi altro server di tool. Il tipo Rust che lo fa è `PythonMcpToolClient`
(`crates/orchestrator/src/python_mcp_tool_client.rs`), esplicitamente descritto nel codice come
"mirror" del client usato per i server MCP nativi in Rust — stesso pattern lazy-spawn-and-reuse: il
processo nasce alla prima richiesta sul canale, resta vivo e viene riusato per le chiamate
successive nella stessa connessione (un client per connessione WebSocket), e viene terminato
esplicitamente lato Rust quando non serve più. È il pattern di `python-ping` e `financial-markets`:
entrambi hanno un proprio `server.py` che costruisce un oggetto FastMCP, dichiara uno o più tool con
`@mcp.tool()`, e resta in ascolto su stdio con `mcp.run(transport="stdio")`.

**Script one-shot.** Qui non è l'orchestratore a parlare con Python: è un server di tool Rust già
esistente che, per UNO dei tool che espone, spawna lo script Python come sottoprocesso, aspetta che
stampi una singola riga JSON `{"output": "...", "is_error": bool}` su stdout, e lo lascia
terminare — nessun processo che resta vivo, nessun handshake MCP verso lo script stesso. È il
pattern di `fritzbox`: il canale `/netsec` (`crates/mcp-nmap`) spawna `fritzbox_status.py` con
`python -X utf8 fritzbox_status.py --config-dir <dir>` (il flag `-X utf8` evita ogni problema di
codifica OEM su Windows, a differenza dei comandi nativi Win32 usati dagli altri tool dello stesso
canale), attende con un timeout dedicato (30 secondi, indipendente dal timeout generale del
canale), e interpreta la riga JSON risultante. Un esito negativo — router irraggiungibile,
credenziali sbagliate — è sempre un **esito** riportato all'AI come dato (`is_error: true`, un
messaggio leggibile), mai un crash del processo: lo script Python esce sempre con codice 0 quando
riesce a produrre una risposta, qualunque essa sia.

## Gli ambiti di oggi

Un "ambito" (o dominio) è una sottocartella di `scripts/pytools/` con il proprio virtual
environment — non coincide necessariamente con "canale": `python-ping` e `financial-markets` sono
ciascuno sia un ambito pytools sia un canale a sé stante (con proprio slash trigger e prompt di
sistema); `fritzbox` è un ambito pytools che serve un solo tool dentro un canale Rust preesistente
(`/netsec`), non un canale a parte.

| Ambito | Pattern | Usato da | Cosa fa |
|---|---|---|---|
| `python-ping` | persistente | canale `/pyping` | Dominio di prova: un solo tool, `pyping`, che restituisce un'eco deterministica. Isola i bug di meccanismo (venv, spawn, handshake MCP) da quelli di dominio, prima che arrivi un tool Python reale — nessuna dipendenza oltre `mcp`. |
| `financial-markets` | persistente | canale `/markets` | Report azionario USA: dati fondamentali, grafici su cinque orizzonti temporali, option chain, comparative con i titoli affini, un elenco tabellare filtrabile per paese, uno screener con registry di più varianti (`list_screeners`/`run_screener`). Le fonti dati (yfinance, Interactive Brokers, una fonte fittizia per i test) sono intercambiabili dietro un'astrazione comune. |
| `fritzbox` | one-shot | tool `fritzbox_status` del canale `/netsec` | Diagnostica del router domestico via TR-064: ultime righe del registro eventi, IP pubblico e stato della connessione WAN, dispositivi noti sulla rete locale. Dichiarato apertamente nel codice: non è rilevamento intrusioni — il registro del FRITZ!Box riporta login falliti e tentativi VPN, non i pacchetti bloccati dal firewall; lo strumento riferisce quello che il log contiene, senza interpretarlo oltre. |

## Convenzioni pratiche

- **Un virtual environment per ambito, mai condiviso tra ambiti diversi.** Più script dello
  *stesso* ambito condividono lo stesso venv (`financial-markets` ne ha diversi, tutti sotto un
  unico `venv/`) — la ragione è evitare di duplicare dipendenze pesanti come `pandas` per ogni
  singolo script.
- **Il venv non va mai committato.** `.gitignore` lo esclude a livello globale (`**/venv/`,
  `**/__pycache__/`), non con una regola specifica di pytools. È committato invece tutto ciò che
  compone l'ambito: gli script `.py`, `requirements.txt`, un `README.md` di dominio quando esiste.
  File generati a runtime accanto a `server.py` (cache come `tickers_us.json`,
  `fundamentals_cache.json`, `discoveries.json` per `financial-markets`) non fanno parte del
  sorgente.
- **Il provisioning del venv è manuale per scelta**, non un'omissione: nessun meccanismo crea o
  aggiorna un venv in automatico. Crearlo o ricrearlo è sempre:
  ```powershell
  cd scripts/pytools/<nome-ambito>
  python -m venv venv
  venv\Scripts\Activate.ps1
  pip install -r requirements.txt
  ```
- **Un venv mancante non fa crashare nulla**: sia il pattern persistente (già in fase di
  risoluzione del canale) sia quello one-shot verificano l'esistenza dell'interprete prima di
  spawnare, e restituiscono un messaggio leggibile che indica di creare il virtual environment,
  invece di un errore di sistema opaco.

## Configurazione: `--config-dir`, come ovunque

Vale qui la stessa regola unica descritta in [`01-architettura.md`](./01-architettura.md)
("Configurazione: una sola regola, sempre"): nessuna variabile d'ambiente specifica del progetto.
Ogni server o script Python riceve dal proprio chiamante — l'orchestratore per il pattern
persistente, il crate Rust proprietario del canale per il pattern one-shot — l'argomento esplicito
`--config-dir <dir>` allo spawn, sempre, indipendentemente dal fatto che quel dominio ne abbia
davvero bisogno: `python-ping` lo riceve e lo ignora (FastMCP non tocca `sys.argv`, quindi
l'argomento in più non causa mai un errore "unrecognized arguments"). Un dominio che legge
configurazione lo fa con un parsing esplicito degli argv (`config_dir.resolve()` in
`financial-markets`, una funzione equivalente in `fritzbox_status.py`) — mai con
`os.environ.get(...)`.

La prima versione del progetto leggeva questa cartella dalla variabile d'ambiente
`LARE_LOCAL_DIR` (ereditata dal processo padre, con fallback su `%LOCALAPPDATA%`): un terzo punto
di lettura indipendente da quelli Rust/UI, che poteva divergere in silenzio dagli altri due. La 2.0
elimina questa variabile per lo stesso motivo per cui elimina ogni `LARE_*` nel resto del sistema.

File di configurazione per-dominio tipici: `market_data.json` per `financial-markets`,
`fritzbox.json` per `fritzbox` (un modello si trova in `fritzbox.example.json`, dentro la cartella
`Configuration`). Un dettaglio non uniforme fra i due, onestamente riportato: se il flag manca del
tutto — caso limite, capita solo lanciando uno script a mano durante lo sviluppo, mai nell'uso
normale tramite Rust — `financial-markets` ricade su un percorso di default (`<radice del
deploy>/Configuration`), mentre `fritzbox_status.py` solleva un errore invece di indovinare un
percorso.

## Dove vive la radice pytools

La formula di risoluzione è unica e condivisa da entrambi i pattern (`PythonMcpToolClient::resolve`
per il persistente, la stessa logica in `crates/mcp-nmap/src/main.rs` per il one-shot): la radice
(`<root>`) è `startup.json.paths.pytools_dir` (default `"pytools"`); se è un percorso assoluto
viene usato così com'è, altrimenti è risolto rispetto alla **radice del deploy** — la cartella che
contiene `Configuration/`, cioè il genitore della cartella passata con `--config-dir`. Il
`domain_id` si unisce sempre sopra questa radice:

- interprete: `<root>/<domain_id>/venv/Scripts/python.exe` (Windows) o
  `<root>/<domain_id>/venv/bin/python3` (Unix)
- script: `<root>/<domain_id>/<script_relpath>`

Anche questo percorso, in v1, veniva letto da una variabile d'ambiente dedicata
(`LARE_PYTOOLS_DIR`) — eliminata nella 2.0 insieme a tutte le altre.

Nel workflow di sviluppo normale, l'orchestratore si avvia con un `--config-dir` esplicito che
punta a `Test Run\Configuration`: la radice del deploy è quindi `Test Run\`, e il default
`pytools_dir: "pytools"` risolve a `Test Run\pytools\` — **non** a `scripts/pytools/` del
repository. Due modi legittimi per far combaciare le cose:

1. Copiare `scripts/pytools/` dentro `Test Run/pytools/` (lo script di deploy lo fa già,
   escludendo esplicitamente `venv/`, `__pycache__/`, `.pytest_cache/` e i file di cache generati)
   e creare il venv di ogni ambito direttamente dentro `Test Run\pytools\<ambito>\` — la copia non
   porta mai con sé un venv, va sempre creato lì.
2. Puntare `paths.pytools_dir`, in `Test Run\Configuration\startup.json`, a un percorso assoluto
   verso il sorgente nel repository, per lavorare direttamente lì senza copiare nulla:
   ```json
   { "paths": { "pytools_dir": "C:/.../Lare Terminal 2.0/scripts/pytools" } }
   ```

## Aggiungere un nuovo ambito, passo per passo

**Pattern persistente** (il caso più comune per un dominio con più tool, o con uno stato da
mantenere vivo tra una chiamata e l'altra):

1. Creare `scripts/pytools/<nome-ambito>/`, con `requirements.txt` (almeno `mcp<2.0.0`) e un
   `server.py` che costruisce un `FastMCP`, dichiara uno o più `@mcp.tool()`, e chiama
   `mcp.run(transport="stdio")` alla fine.
2. Creare il venv e installare le dipendenze (vedi sopra).
3. Registrare l'ambito come voce in `EXTERNAL_TOOL_CHANNELS`
   (`crates/orchestrator/src/external_channel.rs`): un `id`, uno `slash_trigger`, un
   `window_title`, un prompt di sistema dedicato, e una `tool_client` factory che chiama
   `PythonMcpToolClient::resolve(config_dir, startup, "<id>", "server.py", vec![...])` con un
   `PythonToolSpec` per ogni tool da esporre. Questa voce è l'unico modo per rendere l'ambito
   raggiungibile come **canale** con proprio slash trigger; un caller Rust può anche costruire un
   `PythonMcpToolClient` ad hoc per un singolo scopo interno senza passare dal registro (è il caso
   del tool diagnostico `test_market_data_source` di `financial-markets`, richiamato direttamente
   da un pulsante "Test connessione" nella finestra di configurazione, non da un canale a sé) — ma
   in entrambi i casi serve sempre del codice Rust esplicito che nomini il tool: nessuna scoperta
   automatica.

Un punto facile da perdere: **un tool è dichiarato due volte, con ruoli diversi**. In Python, il
decoratore `@mcp.tool()` e il suo docstring sono l'implementazione — cosa il tool fa davvero. In
Rust, il campo `def: ToolDef { name, description, input_schema }` dentro ogni `PythonToolSpec` è
ciò che l'AI **vede** — nome, descrizione e schema dei parametri sono scritti a mano lato Rust, e
solo i tool elencati nel `vec![...]` passato a `resolve()` per un dato canale sono raggiungibili da
quel canale. `financial-markets/server.py` dichiara sei `@mcp.tool()`; il canale `/markets` ne
registra cinque — il sesto, `test_market_data_source`, non è fra questi (vedi sopra). Rust resta
quindi autoritativo su quale sottoinsieme di capacità è visibile e con quale descrizione,
indipendentemente da cosa dichiara il codice Python.

Se un tool deve aprire una finestra Markdown con il risultato (invece di restare testo semplice in
chat), `server.py` può seguire un contratto JSON opt-in — `{"summary", "report_markdown",
"channel_summary"[, "title", "title_suffix"]}` — che il lato Rust interpreta per costruire la
finestra; un tool che ritorna una stringa semplice (`pyping`) resta testo in chat, senza che questo
sia un errore.

**Pattern one-shot** (per una singola capacità senza stato, che si aggancia a un canale Rust già
esistente):

1. Creare `scripts/pytools/<nome-ambito>/` con lo script `.py` e `requirements.txt`.
2. Lo script accetta `--config-dir` (anche se non lo usa, per uniformità con il resto dei pytool),
   fa il proprio lavoro, e stampa **sempre** una riga JSON `{"output": "...", "is_error": bool}` su
   stdout prima di uscire con codice 0 — un esito negativo va riportato come dato leggibile, non
   come eccezione non gestita.
3. Nel crate Rust che possiede già il canale di destinazione (es. `mcp-nmap` per `/netsec`),
   aggiungere la logica di spawn: risolvere interprete e script con la stessa formula descritta
   sopra, costruire il sottoprocesso con un timeout dedicato, interpretare la riga JSON risultante,
   ed esporre l'esito come un tool ordinario di quel canale.

## Limiti noti, dichiarati apertamente

- Nessuna scoperta automatica: creare la cartella e il venv giusti non basta a rendere un ambito
  raggiungibile — serve sempre una modifica esplicita lato Rust (una voce di registro per un
  canale persistente, del codice di spawn per il pattern one-shot, o un client costruito ad hoc
  come nel caso di `test_market_data_source`).
- Il comportamento in assenza del flag `--config-dir` non è identico ambito per ambito (vedi
  sopra) — una differenza che conta solo per chi lancia uno script Python a mano, mai nell'uso
  normale.
- Oggi esistono solo tre ambiti; il criterio di scelta fra i due pattern è quello osservabile nei
  casi esistenti, non una regola scritta a priori.

## Per approfondire

- [`00-apertura.md`](./00-apertura.md) — cos'è Lare Terminal e perché esiste.
- [`01-architettura.md`](./01-architettura.md) — i tre strati e la regola di configurazione che
  vale anche qui.
- [`02-stato-e-implementazione.md`](./02-stato-e-implementazione.md) — quadro complessivo di cosa
  è implementato oggi, pytools incluso.
- [`05-canali.md`](./05-canali.md) — i canali che usano questi tool (`/netsec`, `/markets`).
