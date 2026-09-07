# Changelog — lare-shell

All notable changes to this component are documented here.
Format: [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), versioning: [SemVer](https://semver.org/).

---

## 2.0.0 — 2026-09-06 (piano 2b)

Prima versione della host custom del motore PowerShell (ADR-015), verificabile in modalità B
(profilo Windows Terminal "Lare Terminal").

- Configurazione: `--config-dir` o `<exe>\..\Configuration\`, `startup.json` (ws_port, autostart), `token`,
  log su file `logs\lare-shell.log` (D6: nessuna variabile d'ambiente).
- Protocollo 2.1 (canale shell): `hello{role:"shell"}`, `command`, `tool_confirm_response`, `exec_result`,
  `cancel_command`; ricezione su thread proprio → `Channel`, consumo sul thread del REPL.
- Runspace ospitata: PSReadLine dai moduli di pwsh (anteposti al PSModulePath del processo),
  execution policy LocalMachine da `powershell.config.json` accanto all'exe (ADR-019), profili
  `profile.ps1` → `Microsoft.PowerShell_profile.ps1` → `LareShell_profile.ps1`, `$PROFILE` con 4 NoteProperty.
- Turno slash: gate `[Y/n]` a lettura tasto, `ExecInShell` con `capture` true (cmdlet di passaggio,
  output catturato dai `Write*` della host, cap 200 KB testa+coda) / false (console attaccata),
  `exit_code` da `$?`/`$LASTEXITCODE`, cwd per sessione, Ctrl+C = stop + cancel.
- Autostart di `orchestrator.exe` (retry 5 s) e `ui.exe` (§6.4), processi senza console ereditata.
- OSC 9001 `intercept` (ESC da `(char)0x1B`, test di scansione dei sorgenti).
- `--selftest`; xUnit: 115 test, con server WS finto in-process (`FakeOrchestrator`).
