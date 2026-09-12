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