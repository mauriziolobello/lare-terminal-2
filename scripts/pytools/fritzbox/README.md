# fritzbox — stato del router FRITZ!Box via fritzconnection (TR-064)

Script one-shot (non server MCP persistente): invocato da `crates/mcp-nmap` via shell-out,
stampa una riga JSON ed esce. Stesso pattern di `local_network_info`/`traceroute` in `network_info.rs`.

## Creare il virtual environment

```powershell
cd scripts/pytools/fritzbox
python -m venv venv
venv\Scripts\python.exe -m pip install -r requirements.txt
```

Il venv **non** va committato (`.gitignore` esiste già).

## Configurazione

`<config_dir>/fritzbox.json` (vedi `Test Run/Configuration/fritzbox.example.json`):
```json
{
  "host": "192.168.178.1",
  "user": "il-tuo-utente-fritzbox",
  "password": "la-tua-password-fritzbox"
}
```

Il router deve avere "Zugriff für Anwendungen zulassen" abilitato
(Heimnetz → Netzwerk → Netzwerkeinstellungen).