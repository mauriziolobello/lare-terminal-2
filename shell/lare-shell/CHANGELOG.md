# Changelog — lare-shell

All notable changes to this component are documented here.
Format: [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), versioning: [SemVer](https://semver.org/).

---

## [2.0.2] — 2026-09-17

Segnalato da Maurizio: il banner del REPL ("Lare Terminal 2.0.0 — sessione ...") mostrava ancora
"2.0.0" nonostante il progetto fosse già a 2.0.1 da tempo (bump `--no-terminal`, piano 3 Task 3).

### Fixed

- `HostInfo.Version` era una `const string` hardcoded a "2.0.0", scritta a mano e mai più
  aggiornata insieme al `.csproj` — un secondo posto, oltre a `<Version>` in `LareShell.csproj`,
  da tenere sincronizzato manualmente. Ora è letta a runtime da `Assembly.GetName().Version`
  (generato da MSBuild direttamente dal `<Version>` del `.csproj`), eliminando la duplicazione:
  un solo posto vero, il `.csproj`, esattamente come il commento della classe già dichiarava
  d'intenzione. Usata `AssemblyName.Version` (non `AssemblyInformationalVersionAttribute`, la
  prima scelta tentata): quest'ultima, in un repo git, include automaticamente un suffisso
  `+<commit-sha>` che `System.Version` (usato da `LareHost.Version` per `PSHost.Version`) non sa
  parsare — confermato da un test RED che ha sollevato `FormatException` prima di correggere
  l'approccio.
- Test aggiornato (`LareHostTests.Identita_della_host`): confrontava `host.Version` con un secondo
  numero letterale hardcoded (`new Version(2, 0, 0)`), che avrebbe riprodotto lo stesso bug alla
  prossima release — ora confronta con `HostInfo.Version` stesso.

118/118 test xUnit verdi.

## [2.0.1] — 2026-09-07

Fix wave della revisione finale del piano 2b, più il fix `--no-terminal` del piano 3 Task 3 (quest'
ultimo motiva il bump di versione — i fix del piano 2b, di per sé, non lo avrebbero richiesto).

### Fixed

- `ConsoleGate`: il buffer di input della console viene svuotato prima del prompt `[Y/n]` (il gate risponde solo a un tasto premuto DOPO il prompt) e i tasti scartati/letti sono loggati — all'e2e con AI reale due gate risultavano accettati da tasti pendenti.
- `install-wt-profile.ps1`: `commandline` del profilo Windows Terminal ora virgolettata — senza
  virgolette, WT passa la stringa a `CreateProcess` senza `lpApplicationName`, che con un percorso
  con spazi ("Test Run") prova a risolvere ogni prefisso troncato allo spazio.
- Ctrl+C reso osservabile: l'handler logga sempre la propria attivazione
  (`Repl.cs`); `Console.TreatControlCAsInput` viene riportato a `false` dopo ogni `ReadLine`,
  invece di fidarsi che PSReadLine lo faccia da sé.
- `SlashTurn.TerminalPending`: un `Chunk` di ack in testa alla coda (con l'id del turno) conta
  ora come terminale quanto `Done`/`Error` — per un turno gateizzato l'orchestratore manda
  esattamente un ack seguito dal `Done`, quindi il gate abbandona subito invece di aspettare un
  tasto dell'utente.
- `Executor.Run`: `Recorder.End()` gira ora in un `finally` attorno a `Invoke`, così il
  registratore si chiude anche se `Invoke` lanciasse un'eccezione inattesa; il segnaposto
  `$global:__lare_ok` viene rimosso su TUTTI i rami dell'esito (anche `Stopped`/errore di
  sintassi), non solo su quello che passa da `ReadExitCode`.
- `Osc.Intercept`: payload troncato a 4096 caratteri (una riga patologica non deve produrre una
  OSC chilometrica); `Repl.RunSlash` non scrive più la sequenza quando `stdout` è rediretto (file
  o pipe), per non spedire byte grezzi a un consumatore che non è un emulatore.
- `deploy_test_run.ps1`: `Test Run\shell\` viene ripulita prima del `dotnet publish` (che non
  pulisce da sé), per non lasciare DLL orfane dopo un bump di pacchetto NuGet.
- Doc-comment di `Launcher.ProcessStarter` allineato al comportamento reale: sia
  `orchestrator.exe` sia `ui.exe` partono con console nascosta (non solo il primo, come diceva
  ancora il commento).
- `Launcher.EnsureUi()` passa `--no-terminal` a `ui.exe` (piano 3): senza, ogni self-heal in
  modalità B avrebbe aperto una seconda finestra terminale non voluta.

### Added

- Test: `ExecutorTests.RunInteractive_fermato_da_StopCurrent_ritorna_true`,
  `SlashTurnTests.Il_gate_viene_abbandonato_se_in_coda_ci_sono_ack_e_Done_del_turno`,
  `OscTests.Intercept_tronca_payload_oltre_4096_caratteri`.

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
