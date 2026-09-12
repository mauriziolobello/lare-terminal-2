# Compito per AI esterna — Canale `/nmap` → `/netsec` + primo strumento oltre nmap: stato FRITZ!Box (fritzconnection)

> **Prima cosa**: leggi per intero `Docs/i18n/ita/BRIEFING-AI-ESTERNE.md` alla radice del
> repository, poi questo file per intero, PRIMA di scrivere codice. **Crea la tua worktree
> dedicata come primissima cosa** (`git worktree add .worktrees/netsec-fritzbox -b
> feat/netsec-fritzbox`), come impone BRIEFING-AI-ESTERNE.md §1/§6.

## Contesto — cosa esiste oggi

Il canale `/nmap` è un **canale tool esterno** (architettura in
`crates/orchestrator/src/external_channel.rs`, registro statico
`EXTERNAL_TOOL_CHANNELS`): l'utente digita `/nmap` in una finestra terminale, si apre una
finestra dedicata dove un'AI ha accesso a un set FISSO di tool (mai una shell generica),
elencati esattamente nel campo `system_prompt_override` del canale. Il sidecar dietro il canale è
`crates/mcp-nmap` (crate/binario `mcp-nmap`, protocollo MCP via `rmcp`, stdio), spawnato lazy
dall'orchestratore (`crates/orchestrator/src/nmap_tool_client.rs`, `NmapToolClient`) che gli passa
`--config-dir <path>` (già oggi, anche se `mcp-nmap::main()` non lo interpreta ancora — Parte B lo
corregge). Oggi il canale espone ESATTAMENTE 7 tool: 5 scan nmap (`nmap_quick_scan`,
`nmap_os_detect`, `nmap_version_scan`, `nmap_host_discovery`, `nmap_vuln_scan`) + 2 info diagnostici
built-in del SO (`local_network_info`, `traceroute`, in `crates/mcp-nmap/src/network_info.rs`).

Maurizio vuole far evolvere questo canale verso uno strumento più ampio di analisi/diagnostica di
rete ("quasi pentesting"). Primo passo concreto: un tool che legge lo stato del proprio router
domestico (FRITZ!Box, protocollo TR-064) tramite la libreria Python `fritzconnection` — risponde
direttamente alla domanda con cui è nato questo compito ("ci sono tentativi di accesso dall'esterno
sulla rete di casa?"), anche se — vedi §5 — la risposta che può dare è più limitata di quanto quella
domanda lasci intendere, e il testo del tool/prompt deve dirlo onestamente.

Il compito ha due parti indipendenti, **in quest'ordine, ciascuna con il proprio commit** (Maurizio
lo ha chiesto esplicitamente: prima rinomina il canale, poi aggiungi lo strumento nuovo).

---

## Parte A — Rinomina del canale: `/nmap` → `/netsec`

Il nome tecnico interno (crate `mcp-nmap`, binario `mcp-nmap.exe`, file `nmap_tool_client.rs`,
struct `NmapToolClient`, funzione `format_nmap_invocation`, nomi dei 5 tool `nmap_*`, costante
`NMAP_SYSTEM_PROMPT`) **NON cambia** — sono dettagli interni, cambiarli aumenterebbe il diff senza
alcun beneficio. Cambia SOLO ciò che l'utente/l'AI vedono: lo slash da digitare, l'id del canale nel
registro, il titolo finestra, il testo del system prompt, e ogni posto (help, test) che verifica
queste stringhe user-facing.

### A.1 — `crates/orchestrator/src/external_channel.rs`

Riga ~240-248, l'entry `EXTERNAL_TOOL_CHANNELS[0]`:

```rust
        id: "netsec",              // era "nmap"
        slash_trigger: "/netsec",  // era "/nmap"
        window_title: "Lare — netsec",  // era "Lare — nmap"
```

(gli altri campi dell'entry — `tool_client`, `format_invocation: Some(format_nmap_invocation)`,
`system_prompt_override: Some(NMAP_SYSTEM_PROMPT)` — restano invariati, i nomi Rust non cambiano).

`NMAP_SYSTEM_PROMPT` (riga ~62): il testo cambia (nuovo nome canale +, se hai già completato anche
la Parte B nello stesso branch, l'ottavo tool — vedi §B.4; se stai lavorando Parte A da sola,
aggiorna solo il nome del canale, lasciando "Hai ESATTAMENTE sette strumenti"). Sostituisci
"Sei l'assistente del canale nmap di Lare Terminal" con "Sei l'assistente del canale netsec di Lare
Terminal".

Test da aggiornare (riga ~577): `assert_eq!(EXTERNAL_TOOL_CHANNELS[0].slash_trigger, "/nmap");` →
`"/netsec"`.

### A.2 — `crates/ui/frontend/external-channels.js`

Mirror lato frontend dello stesso registro (35 righe, leggilo per intero). Trova l'entry con
`slash: "/nmap"` e aggiorna `id`, `slash`, `title` con gli stessi 3 nuovi valori di A.1 (stessa
corrispondenza campo-per-campo del resto del file — guarda come sono scritte le altre entry per lo
stile esatto).

### A.3 — `crates/orchestrator/src/shell_slash.rs`

Riga 44, `USER_FACING_CHANNEL_TRIGGERS`:

```rust
const USER_FACING_CHANNEL_TRIGGERS: &[&str] = &["/netsec", "/pyping", "/markets"];
```

Test riga ~219-220 (`classify_shell_input`) e riga ~283 (`channel_table_exposes_the_three_user_facing_channels`,
la tupla `("nmap".to_string(), "nmap".to_string())` diventa `("netsec".to_string(), "netsec".to_string())`
— **attenzione**: quella funzione ordina il vettore alfabeticamente prima di confrontare
(`t.sort()`), quindi controlla dove "netsec" ricade nell'ordine rispetto a "financial-markets" e
"python-ping" e sistema l'ordine del `vec![...]` atteso di conseguenza, non limitarti a sostituire
la stringa in-place.

### A.4 — `crates/orchestrator/src/core.rs`

Riga ~1546, la lista che verifica il contenuto reale del file di help imbustato:

```rust
for cmd in ["/aichat", "/markets", "/netsec", "/pyping"] {
```

e il commento subito sopra (riga ~1542-1543) che nomina `/nmap` va aggiornato a `/netsec`.

Riga ~589, dentro l'helper di test `run()`: la fixture di help scritta ad-hoc dal test contiene
`- /nmap\n` in una stringa — aggiornala a `- /netsec\n` per coerenza (nessun test verifica questo
valore specifico, ma lasciare "nmap" qui dopo il rename sarebbe fuorviante per chi legge il test in
futuro).

### A.5 — `crates/orchestrator/src/ws.rs`

SOLO commenti — nessun test qui dipende dal nome reale del canale. Aggiorna il testo dei commenti
alle righe ~87, ~820-821, ~954, ~960, ~1028 che nominano `/nmap` come canale (es. "per un canale
esterno come `/nmap`" → "come `/netsec`"). Le righe ~637/~662 nominano "i cinque tool del canale
nmap" — quello resta corretto invariato (sono i nomi dei tool `nmap_*`, che non cambiano).

**NON toccare** i test alle righe ~1086/1096/1098: usano `"nmap"` come channel-id letterale
arbitrario per testare un meccanismo generico (`connection_owns_plugin_sink`), indipendente dal
registro reale — cambiarli non è necessario e non li fa fallire se lasciati.

### A.6 — `Test Run/Configuration/help/{it,en,es}.md`

Ovunque compaia `/nmap` (voce nell'elenco comandi, eventuale riga di spiegazione), sostituisci con
`/netsec`, in tutte e 3 le lingue.

### A.7 — Cosa NON toccare in Parte A

- `crates/orchestrator/src/tool_client.rs`: ogni occorrenza di "nmap" lì è un comando shell
  d'ESEMPIO in un commento/messaggio generico ("timeout... comandi lenti (nmap, ping -t...)"),
  niente a che fare col canale — non cambiare nulla in questo file.
- `crates/orchestrator/src/router.rs`: la fixture nmap lì è già stata verificata come
  self-contenuta (non dipende dal registro reale) — non toccarla.
- Ogni nome Rust interno elencato in cima a questa Parte A (crate/bin `mcp-nmap`, `NmapToolClient`,
  `nmap_tool_client.rs`, `format_nmap_invocation`, `NMAP_SYSTEM_PROMPT`, i 5 nomi tool `nmap_*`).

### A.8 — Versioni e verifica

Bump patch: `crates/orchestrator/Cargo.toml` (2.2.6 → 2.2.7), `crates/ui/src-tauri/Cargo.toml`
(2.3.3 → 2.3.4). `CHANGELOG.md`/`IMPLEMENTATION.md` di entrambi i crate, sezione nuova (non
retroattiva su una già committata). `Docs/i18n/ita/HANDOFF.md` aggiornato nello stesso commit
(hook `commit-msg` te lo impone comunque).

```powershell
cargo test -p orchestrator
node --test crates/ui/frontend/external-channels.test.mjs  # se esiste; altrimenti l'intera suite js
cargo clippy --all-targets
cargo fmt --check
```

## FINE PARTE A — commit e stop

Commit descrittivo (`git commit -F <file>`, mai `-m` inline con backtick), poi fermati: aspetta
conferma/prossimo via libera prima di iniziare la Parte B (anche se lavori nella stessa worktree/
branch, sono due commit distinti).

---

## Parte B — Nuovo tool `fritzbox_status` (libreria Python `fritzconnection`)

### B.1 — Perché questo pattern e non il pattern MCP persistente

`scripts/pytools/{financial-markets,python-ping}` sono server MCP Python **persistenti**
(`PythonMcpToolClient`, handshake MCP, restano vivi tra una chiamata e l'altra). Per una singola
lettura periodica dello stato del router questo è sproporzionato: nessuno stato da mantenere tra
chiamate, nessun bisogno di un secondo protocollo MCP innestato in un sidecar che ne parla già uno.
Il pattern giusto è quello già usato da `crates/mcp-nmap/src/network_info.rs` per
`local_network_info`/`traceroute`: **uno shell-out one-shot**, cattura output, ritorna JSON. Qui il
processo shellato è uno script Python invece di un comando nativo Win32, quindi niente decodifica
OEM (quella è specifica di `ipconfig`/`arp`/ecc.) — lo script Python emette UTF-8 direttamente.

### B.2 — Config: `<config_dir>/fritzbox.json`

Nuovo file di configurazione, stesso "cassetto" degli altri file sensibili in `Configuration/`
(`llms.json`, `token`). Schema:

```json
{
  "host": "192.168.178.1",
  "user": "il-tuo-utente-fritzbox",
  "password": "la-tua-password-fritzbox"
}
```

Crea **due** file:
- `Test Run/Configuration/fritzbox.example.json` — committato, con i valori placeholder esatti
  sopra (nessun segreto reale).
- Aggiungi a `.gitignore` (vicino alla riga esistente `Test Run/Configuration/llms.json`):
  `Test Run/Configuration/fritzbox.json`

**Mai** scrivere una password reale in un file che finisce nel commit — se generi/testi con
credenziali vere durante lo sviluppo, verificale SEMPRE assenti da `git status`/`git diff` prima di
committare (BRIEFING-AI-ESTERNE.md §7 lo richiede in generale; qui è particolarmente concreto).

### B.3 — Script Python: `scripts/pytools/fritzbox/fritzbox_status.py`

Nuovo dominio pytools (mirror di `scripts/pytools/python-ping/`: una cartella con script +
`requirements.txt`, venv creato a mano, mai committato — vedi `scripts/pytools/README.md`, sezione
"Creare un nuovo ambito"). Crea anche `scripts/pytools/fritzbox/requirements.txt`:

```
fritzconnection>=1.13
```

Lo script, invocato come `python -X utf8 fritzbox_status.py --config-dir <path>`, legge
`<config_dir>/fritzbox.json`, si connette al router, raccoglie 3 informazioni (log eventi, IP
pubblico, elenco host LAN), e stampa **su stdout, e SOLO quello**, un'unica riga JSON
`{"output": "<testo>", "is_error": <bool>}` — mai un traceback su stdout, mai un `exit` diverso da 0
(gli errori sono dati, non fallimenti di processo — stesso principio dei tool nmap esistenti). Log
diagnostici (se servono durante lo sviluppo) vanno su stderr, mai stdout.

```python
#!/usr/bin/env python3
"""fritzbox_status — legge lo stato del router FRITZ!Box via TR-064 (fritzconnection).

Invocazione: python -X utf8 fritzbox_status.py --config-dir <path>
Legge <config_dir>/fritzbox.json ({"host", "user", "password"}).
Stampa su stdout UNA riga JSON {"output": str, "is_error": bool} e ritorna sempre
exit code 0 — un router irraggiungibile o credenziali sbagliate sono un ESITO
(is_error: true, messaggio leggibile), non un crash del processo: il lato Rust
(crates/mcp-nmap/src/fritzbox.rs) inoltra `output` all'AI così com'è.

Nota onestà (importante, vedi anche NMAP_SYSTEM_PROMPT lato Rust): questo tool
NON rileva "tentativi di accesso dall'esterno" in senso di intrusion detection.
Il registro eventi del FRITZ!Box (GetDeviceLog) tipicamente riporta login falliti,
tentativi VPN, riconnessioni WAN — non pacchetti bloccati dal firewall verso porte
chiuse (quelli il FRITZ!Box normalmente non li logga affatto). Riporta quello che
il log contiene, non lo si interpreta né si esagera cosa significa.
"""
import json
import sys
from pathlib import Path


def parse_config_dir() -> Path:
    """Estrae --config-dir dagli argv, stesso flag/convenzione di tutto Lare (D6:
    nessuna variabile d'ambiente). Nessun fallback a cwd: chi lancia questo script
    è sempre crates/mcp-nmap/src/fritzbox.rs, che passa sempre il flag esplicito."""
    args = sys.argv[1:]
    for i, a in enumerate(args):
        if a == "--config-dir" and i + 1 < len(args):
            return Path(args[i + 1])
        if a.startswith("--config-dir="):
            return Path(a.split("=", 1)[1])
    raise ValueError("--config-dir mancante")


def emit(output: str, is_error: bool) -> None:
    print(json.dumps({"output": output, "is_error": is_error}), flush=True)


def main() -> None:
    try:
        config_dir = parse_config_dir()
    except ValueError as e:
        emit(f"argomenti non validi: {e}", True)
        return

    config_path = config_dir / "fritzbox.json"
    if not config_path.exists():
        emit(
            f"file di configurazione mancante: {config_path} — crea questo file "
            "partendo da fritzbox.example.json (host/user/password del router).",
            True,
        )
        return

    try:
        cfg = json.loads(config_path.read_text(encoding="utf-8"))
        host = cfg["host"]
        user = cfg["user"]
        password = cfg["password"]
    except (json.JSONDecodeError, KeyError) as e:
        emit(f"fritzbox.json non valido o incompleto: {e}", True)
        return

    try:
        from fritzconnection import FritzConnection
        from fritzconnection.lib.fritzstatus import FritzStatus
        from fritzconnection.lib.fritzhosts import FritzHosts
    except ImportError as e:
        emit(f"libreria fritzconnection non installata nel venv: {e}", True)
        return

    try:
        fc = FritzConnection(address=host, user=user, password=password, timeout=10)
    except Exception as e:  # fritzconnection non ha una singola eccezione dedicata
        # per "auth/rete fallita" nella versione qui pinnata — cattura ampia
        # deliberata, il messaggio va comunque al modello come dato, non come
        # crash: preferibile a un try/except più stretto che rischia di non
        # coprire un caso reale mai visto in test.
        emit(f"connessione al router fallita ({host}): {e}", True)
        return

    sections = []

    # 1) Log eventi del router — sezione DeviceInfo:1, azione GetDeviceLog.
    #    Errori QUI non abortiscono lo script: le altre due sezioni restano utili.
    try:
        log = fc.call_action("DeviceInfo1", "GetDeviceLog")
        raw = log.get("NewDeviceLog", "")
        # Ultime 50 righe: il log può essere lungo, il modello non ha bisogno
        # dell'intera storia per rispondere a "eventi recenti?".
        lines = raw.strip().splitlines()[-50:]
        sections.append("## Registro eventi router (ultime righe)\n" + "\n".join(lines))
    except Exception as e:
        sections.append(f"## Registro eventi router\n_non disponibile: {e}_")

    # 2) IP pubblico / stato connessione WAN.
    try:
        status = FritzStatus(fc=fc)
        sections.append(
            "## Stato connessione WAN\n"
            f"IP esterno: {status.external_ip}\n"
            f"Connesso: {status.is_connected}\n"
            f"Uptime connessione: {status.connection_uptime}s"
        )
    except Exception as e:
        sections.append(f"## Stato connessione WAN\n_non disponibile: {e}_")

    # 3) Host sulla rete locale (utile per scoprire dispositivi non riconosciuti).
    try:
        hosts = FritzHosts(fc=fc)
        rows = []
        for h in hosts.get_hosts_info():
            rows.append(
                f"- {h.get('name', '?')} — {h.get('ip', '?')} "
                f"({h.get('mac', '?')}) attivo={h.get('status', '?')}"
            )
        sections.append("## Dispositivi noti sulla rete locale\n" + "\n".join(rows))
    except Exception as e:
        sections.append(f"## Dispositivi noti sulla rete locale\n_non disponibile: {e}_")

    output = "\n\n".join(sections)
    # is_error solo se OGNI sezione è fallita (nessuna informazione utile prodotta)
    # — stesso principio di local_network_info in network_info.rs.
    all_failed = all("_non disponibile:" in s for s in sections)
    emit(output, all_failed)


if __name__ == "__main__":
    main()
```

Crea anche `scripts/pytools/fritzbox/README.md` breve (mirror dello stile delle altre cartelle
pytools se ne esiste uno locale, altrimenti 4-5 righe: cosa fa, come creare il venv, dove va
`fritzbox.json`).

### B.4 — Rust: `crates/mcp-nmap/src/main.rs` legge `--config-dir`

Oggi `main()` non interpreta NESSUN argomento CLI (verificato leggendo il file per intero), anche
se lo riceve già da `NmapToolClient::resolve` — nessuno lo usa. Aggiungi il parsing, riusando
l'helper condiviso di `startup-config` (**mai** parsing ad-hoc, D6):

```rust
// in Cargo.toml di mcp-nmap, aggiungi:
// startup-config = { path = "../startup-config" }

// in main.rs, dentro fn main() (o equivalente, prima di avviare il server rmcp):
let args: Vec<String> = std::env::args().collect();
let exe_dir = std::env::current_exe()
    .ok()
    .and_then(|p| p.parent().map(std::path::Path::to_path_buf))
    .unwrap_or_else(|| std::path::PathBuf::from("."));
let config_dir = startup_config::resolve_config_dir(
    startup_config::parse_config_dir(&args),
    &exe_dir,
);
let (startup_cfg, _warning) = startup_config::StartupConfig::load(&config_dir);
let pytools_root = startup_config::StartupConfig::resolve_path(&config_dir, &startup_cfg.paths.pytools_dir);
```

`config_dir` e `pytools_root` vanno passati al costruttore di `NmapServer` (qualunque esso sia
oggi — leggi come viene istanziato in `main()` e aggiungi questi due campi allo struct esistente,
stesso stile con cui gli altri campi già ci sono, es. quelli usati da `scan.rs`).

### B.5 — Rust: nuovo modulo `crates/mcp-nmap/src/fritzbox.rs`

**Niente trait/seam di mocking per il sotto-processo** — segui esattamente il precedente diretto di
`network_info.rs::run_and_capture`, che documenta perché (un solo chiamante, mockare aggiungerebbe
un'astrazione senza reale beneficio, YAGNI). La logica di INTERPRETAZIONE dell'output invece va
fattorizzata in una funzione pura, testabile senza spawnare nulla (stesso principio con cui
`decode_oem` in `network_info.rs` è testato da solo, senza dover mockare `GetConsoleOutputCP`):

```rust
//! # fritzbox — stato del router FRITZ!Box via script Python (fritzconnection)
//!
//! Stesso principio isolativo di `network_info.rs`: un tool fisso, un solo
//! shell-out one-shot, nessuna shell arbitraria. A differenza dei comandi nativi
//! Win32 di `network_info.rs`, qui il sotto-processo è uno script Python
//! (`scripts/pytools/fritzbox/fritzbox_status.py`) che emette UTF-8 direttamente
//! (invocato con `-X utf8`) — nessuna decodifica OEM necessaria.

use crate::network_info::NetworkInfoOutcome;
use std::path::Path;

#[derive(serde::Deserialize)]
struct FritzScriptJson {
    output: String,
    is_error: bool,
}

/// Interpreta l'esito grezzo del sotto-processo Python. Pura, senza I/O —
/// testabile passando stringhe dirette, senza spawnare nulla (mirror di
/// `network_info::decode_oem`, testato allo stesso modo).
pub(crate) fn parse_script_output(stdout: &str, stderr: &str, exit_success: bool) -> NetworkInfoOutcome {
    if !exit_success {
        return NetworkInfoOutcome {
            output: format!("script fritzbox terminato con errore.\nstderr:\n{stderr}"),
            is_error: true,
        };
    }
    match serde_json::from_str::<FritzScriptJson>(stdout.trim()) {
        Ok(v) => NetworkInfoOutcome { output: v.output, is_error: v.is_error },
        Err(e) => NetworkInfoOutcome {
            output: format!(
                "output dello script fritzbox non interpretabile ({e}); output grezzo:\n{stdout}"
            ),
            is_error: true,
        },
    }
}

/// Shell-out one-shot: `<python_path> -X utf8 <script_path> --config-dir <config_dir>`.
/// Verifica prima che python/script esistano (messaggio leggibile, stesso stile di
/// `PythonMcpToolClient::resolve` — vedi crates/orchestrator/src/python_mcp_tool_client.rs)
/// invece di lasciare che lo spawn fallisca con un errore OS opaco.
pub async fn fritzbox_status(python_path: &Path, script_path: &Path, config_dir: &Path) -> NetworkInfoOutcome {
    if !python_path.exists() {
        return NetworkInfoOutcome {
            output: format!(
                "venv Python non trovato per \"fritzbox\": {} — crea il virtual environment \
                 (vedi scripts/pytools/fritzbox/README.md)",
                python_path.display()
            ),
            is_error: true,
        };
    }
    if !script_path.exists() {
        return NetworkInfoOutcome {
            output: format!("script fritzbox non trovato: {}", script_path.display()),
            is_error: true,
        };
    }

    let mut cmd = tokio::process::Command::new(python_path);
    cmd.arg("-X").arg("utf8").arg(script_path).arg("--config-dir").arg(config_dir);
    cmd.stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW — stesso motivo di NmapToolClient
    }

    // Timeout dedicato: un router irraggiungibile non deve far attendere il
    // timeout esterno da 900s del canale (NMAP_CALL_TIMEOUT_SECS) — 30s bastano
    // ampiamente per una chiamata TR-064 locale.
    let spawn_and_wait = async {
        let child = cmd.output();
        child.await
    };
    match tokio::time::timeout(std::time::Duration::from_secs(30), spawn_and_wait).await {
        Ok(Ok(out)) => parse_script_output(
            &String::from_utf8_lossy(&out.stdout),
            &String::from_utf8_lossy(&out.stderr),
            out.status.success(),
        ),
        Ok(Err(e)) => NetworkInfoOutcome {
            output: format!("esecuzione dello script fritzbox fallita: {e}"),
            is_error: true,
        },
        Err(_) => NetworkInfoOutcome {
            output: "timeout (30s) in attesa dello script fritzbox — router irraggiungibile?".to_string(),
            is_error: true,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_script_output_passes_through_valid_json() {
        let out = parse_script_output(r#"{"output":"tutto ok","is_error":false}"#, "", true);
        assert_eq!(out.output, "tutto ok");
        assert!(!out.is_error);
    }

    #[test]
    fn parse_script_output_reports_error_flag_from_script() {
        let out = parse_script_output(r#"{"output":"router irraggiungibile","is_error":true}"#, "", true);
        assert!(out.is_error);
        assert_eq!(out.output, "router irraggiungibile");
    }

    #[test]
    fn parse_script_output_handles_garbage_stdout() {
        let out = parse_script_output("questo non e' JSON", "", true);
        assert!(out.is_error);
        assert!(out.output.contains("non interpretabile"));
        assert!(out.output.contains("questo non e' JSON"));
    }

    #[test]
    fn parse_script_output_surfaces_nonzero_exit() {
        let out = parse_script_output("", "Traceback...", false);
        assert!(out.is_error);
        assert!(out.output.contains("Traceback"));
    }

    #[tokio::test]
    async fn fritzbox_status_errs_readably_when_venv_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let out = fritzbox_status(
            &tmp.path().join("venv/Scripts/python.exe"),
            &tmp.path().join("fritzbox_status.py"),
            tmp.path(),
        )
        .await;
        assert!(out.is_error);
        assert!(out.output.contains("venv"));
    }

    #[tokio::test]
    async fn fritzbox_status_errs_readably_when_script_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let fake_python = tmp.path().join("python.exe");
        std::fs::write(&fake_python, b"").unwrap();
        let out = fritzbox_status(&fake_python, &tmp.path().join("missing.py"), tmp.path()).await;
        assert!(out.is_error);
        assert!(out.output.contains("non trovato"));
    }
}
```

Dichiara il modulo in `main.rs` (`mod fritzbox;`).

### B.6 — Rust: ottavo tool `#[tool]` in `NmapServer` (`main.rs`)

Segui esattamente lo stile dei metodi `local_network_info`/`traceroute` esistenti (leggili prima).
Nome tool: `fritzbox_status`, nessun parametro (mirror di `local_network_info`). Descrizione onesta
(vedi §B.1/§B.7 — non promettere "rilevamento intrusioni"):

```rust
#[tool(description = "Legge lo stato del router FRITZ!Box di casa: registro eventi recenti \
    (login falliti, tentativi VPN — NON traffico WAN bloccato dal firewall), IP pubblico attuale, \
    ed elenco dei dispositivi collegati alla rete locale. Richiede che l'utente abbia configurato \
    Configuration/fritzbox.json (vedi fritzbox.example.json). Nessun parametro.")]
async fn fritzbox_status(&self) -> ... {
    let outcome = crate::fritzbox::fritzbox_status(&self.<python_path_field>, &self.<script_path_field>, &self.<config_dir_field>).await;
    // stessa forma di ritorno JSON di local_network_info/traceroute — leggi
    // come quei due metodi convertono NetworkInfoOutcome nel tipo di ritorno
    // rmcp e fai lo stesso, non inventare una forma diversa.
}
```

I 3 campi (`python_path`, `script_path`, `config_dir`) derivano da B.4:
`python_path = pytools_root.join("fritzbox").join("venv").join(if cfg!(windows) {"Scripts/python.exe"} else {"bin/python3"})`,
`script_path = pytools_root.join("fritzbox").join("fritzbox_status.py")`, `config_dir` è quello già
risolto in B.4.

### B.7 — Orchestrator: esporre l'ottavo tool nel canale

`crates/orchestrator/src/nmap_tool_client.rs`:
- `tool_defs()` (riga ~467+): aggiungi un ottavo `ToolDef`:
  ```rust
  ToolDef {
      name: "fritzbox_status".to_string(),
      description: "Stato del router FRITZ!Box di casa: registro eventi, IP pubblico, dispositivi LAN.".to_string(),
      input_schema: serde_json::json!({ "type": "object", "properties": {} }),
  },
  ```
- `dispatch()` (riga ~535+): aggiungi `"fritzbox_status" => self.call_info_tool("fritzbox_status", None).await,`
  (stesso schema di `local_network_info`, nessun target).
- Test `tool_defs_exposes_exactly_the_seven_nmap_channel_tools` (riga ~654): rinomina in
  `tool_defs_exposes_exactly_the_eight_nmap_channel_tools`, `assert_eq!(defs.len(), 8, ...)`,
  aggiungi `assert!(defs.iter().any(|d| d.name == "fritzbox_status"));`.

`crates/orchestrator/src/external_channel.rs`, `NMAP_SYSTEM_PROMPT`: aggiungi l'ottavo tool alla
lista descritta all'AI, cambia "ESATTAMENTE sette strumenti" in "ESATTAMENTE otto strumenti", e
aggiungi — testuale, non parafrasato — questa frase subito dopo la descrizione di
`fritzbox_status` nel prompt: "Questo tool NON rileva intrusioni o pacchetti bloccati dal
firewall: riporta solo ciò che il registro eventi del router contiene (tipicamente login falliti e
tentativi VPN) — non equiparare l'assenza di eventi nel log a 'nessun tentativo di accesso
dall'esterno'."

### B.8 — Versioni e verifica

`crates/mcp-nmap/Cargo.toml`: 2.0.1 → 2.1.0 (funzionalità nuova, non solo fix). `orchestrator`:
2.2.7 → 2.3.0 (segue Parte A). `CHANGELOG.md`/`IMPLEMENTATION.md` di `mcp-nmap` e `orchestrator`,
`Docs/i18n/ita/HANDOFF.md` nello stesso commit.

```powershell
cargo test -p mcp-nmap
cargo test -p orchestrator
cargo clippy --all-targets
cargo fmt --check
```

Non è possibile un vero test end-to-end senza un FRITZ!Box reale raggiungibile e un venv con
`fritzconnection` installato — non è richiesto in questo compito. Se vuoi verificare dal vivo
(opzionale): il router deve avere "Zugriff für Anwendungen zulassen" (Consenti accesso alle
applicazioni) abilitato in Heimnetz → Netzwerk → Netzwerkeinstellungen; **non inserire mai una
password reale in un file che committi** (vedi §B.2).

## FINE PARTE B — commit e stop

Stesso formato di commit di Parte A, poi fermati per la revisione del supervisore.

---

## Cosa NON fare (entrambe le parti)

- Non introdurre variabili d'ambiente (D6: solo `--config-dir`).
- Non far dipendere `mcp-nmap` da `orchestrator` (dipendenza in un solo verso: orchestrator →
  crate condivisi come `startup-config`; mai il contrario).
- Non aggiungere un secondo protocollo MCP dentro lo script Python: è un one-shot, stampa un JSON e
  finisce, esattamente come `local_network_info`/`traceroute`.
- Non promettere nel prompt/tool description capacità di intrusion detection che il tool non ha
  (vedi §B.1/§B.7).
- Non introdurre un trait/seam di mocking per il sotto-processo Python (§B.5 spiega perché, YAGNI —
  mirror della scelta già fatta in `network_info.rs`).
- Non toccare `tool_client.rs` o `router.rs` (§A.7).
- Non scrivere credenziali reali in nessun file che finisce nel commit.
