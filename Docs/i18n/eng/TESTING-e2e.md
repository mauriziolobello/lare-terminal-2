# TESTING-e2e — End-to-End Checklist

Live verification (non-automatable: real processes, real windows) confirming that the deployment
actually works — rather than merely verifying that unit tests pass. Parts 1–4 reproduce what was
verified live upon concluding **Plan 1** ("foundations": staging in `Test Run\`, launching
orchestrator and `ui.exe`); Part 5 adds the shell channel from **Plan 2a** (development client,
without the real host); Part 6 adds the actual C# host from **Plan 2b** in Mode B (Windows Terminal);
Part 7 adds the terminal window from **Plan 3** in Mode A (`ui.exe`, xterm.js + ConPTY) — **executed
live partially**, see the note at the head of that section. Complete the **Outcome** column by
executing steps in order, on a clean machine where possible (no stale `Test Run\Configuration\token`
or `logs\` from prior runs, to evaluate the "first run" scenario).

**Prerequisites**: builds completed (`cargo build`, `cargo build -p ui`, `cargo build -p plugin-ping
-p plugin-calc`) and `.\deploy_test_run.ps1 -IncludePlugins` executed — see [`BUILD.md`](./BUILD.md)
and [`DEPLOY.md`](./DEPLOY.md).

## Part 1 — Orchestrator from `Test Run\`

| # | Step | Expected | Outcome |
|---|---|---|---|
| 1 | From `Test Run\`: `.\init_orchestrator.ps1` (equivalent to `orchestrator.exe --console-log`) | Starts without panicking; console displays `config dir: ...\Test Run\Configuration` | |
| 2 | Same execution, next line | Displays `Lare Terminal orchestrator v2.0.x starting (...)` alongside related crate versions | |
| 3 | If `Test Run\Configuration\token` did NOT exist before step 1 | Displays `[token] primo avvio: token creato in ...\Test Run\Configuration\token`; file exists afterwards, containing 64 hex characters | |
| 4 | Same execution | Displays `plugin host: 2 plugin/i scoperti in ...\Test Run\plugins` (requires deployment with `-IncludePlugins`, otherwise 0) | |
| 5 | Same execution | Displays `Lare Terminal orchestrator listening on ws://127.0.0.1:<ws_port>` (default 7331, from `Configuration\startup.json`) | |
| 6 | After a few seconds, inspect filesystem | `Test Run\Configuration\logs\orchestrator.log.<today's-date>` exists | |
| 7 | No prior step produced a Rust panic or stack trace | — | |

## Part 2 — `ui.exe` with active orchestrator

| # | Step | Expected | Outcome |
|---|---|---|---|
| 8 | With step 1's orchestrator still running, from `Test Run\`: `.\ui.exe --open library` (note: `init_tauri.ps1` does not pass `--open` — launch `ui.exe` directly for this check) | Console displays `[ui] config dir: ...\Test Run\Configuration`, then `[ui] Loaded config: ...`, then `[ui] Lare Terminal v2.0.x started.`; no crash | |
| 9 | Same launch | The **Library** window opens (not the legacy overlay/cursor window: that no longer exists — see ADR-016) | |
| 10 | Press F2 while a Lare window holds focus | Nothing happens (the global F2 hotkey and overlay were removed in Plan 1, ADR-016 — zero registered `global-shortcut` plugins) | |
| 11 | Repeat step 8 passing `--open config` instead of `--open library` | The **`/config`** window opens (Web search + Transparency tabs, without legacy hotkey/position/cursor fields removed between v1 and 2.0) | |
| 12 | Terminate everything with a single command: `Get-Process \| Where-Object { $_.Path -like "*Test Run*" } \| Stop-Process -Force` (catches `orchestrator`, `ui`, AND plugin `ping.exe` — avoiding system `ping`, which `taskkill /IM ping.exe` would touch) | Re-running the same `Get-Process` immediately afterwards yields no results (zero orphaned processes) | |

## Part 3 — `ui.exe` WITHOUT orchestrator

Updated to incorporate two fixes following Plan 1: self-healing (Plan 3, spec §6.4 — `ui.exe` NOW
checks orchestrator reachability upon launch and starts it autonomously if needed) and windowless logging
(`ui.exe` has no visible stdout/stderr in ANY build — unconditional `windows_subsystem = "windows"` —
logging instead to `Configuration\logs\ui.log.<date>`).

| # | Step | Expected | Outcome |
|---|---|---|---|
| 13 | Enumerate ALL existing top-level windows via UIA (`[System.Windows.Automation.AutomationElement]::RootElement.FindAll(TreeScope.Children, TrueCondition)`, **without filtering by PID** — filtering only by `ui`/`orchestrator`/`lare-shell` PID would NEVER catch a spurious window/tab, as that window belongs to the `WindowsTerminal.exe` PID, not one of the three Lare processes) as baseline ("before"). Ensure orchestrator is NOT running, then from `Test Run\`: `.\ui.exe`. Wait ~4s, then re-enumerate ALL top-level windows ("after") and compare with "before" (`Compare-Object`) | Self-heal starts it autonomously (no crash): the `orchestrator.exe` process appears in `Get-Process` within seconds. The ONLY new entries in the "before"/"after" diff are `Name = "Lare Terminal"` (`Class = "Tauri Window"`) and an element with `Class = "Tao Thread Event Target"` (`IsOffscreen = False` but no visible content — internal Tao/wry framework detail, not a user window) — **no new entries with `ProcName = "WindowsTerminal"` or `Name` resembling `...\ui.exe`/`...\plugins\...\exe`** (the bug symptom prior to this fix) | |
| 13bis | (optional, covers `mcp-server.exe`) In a session with active orchestrator, trigger an `/ai` turn requiring an `mcp-server` tool (e.g. `/ai "list 3 largest files here"`, `BUILD.md`) — incurs a real API call. Re-enumerate top-level windows | `mcp-server.exe` appears in `Get-Process`, `Configuration\logs\mcp-server.log` is created — no new entries in window diff | |
| 14 | Open `Test Run\Configuration\logs\ui.log.<today's-date>` | Contains startup lines: `[ui] config dir: ...\Test Run\Configuration` and `[ui] Lare Terminal v2.2.x started.` — replaces stdout verification, which no longer exists (`ui.exe` never owns a console in any build) | |
| 15 | Close `ui.exe` | Process terminates without leaving remnants (`Get-Process` as in step 12) | |

> Note: prior to these two fixes, "no crash" was the only verifiable condition via terminal in this
> scenario, as an explicit application log like "orchestrator unreachable" did not exist. It exists
> now: self-healing logs the attempt (`tracing::warn!`, via `startup_config::logging`, a module
> shared with the orchestrator) to `ui.log.<date>` — inspectable even in release builds, where
> previously `ui.exe` lacked stderr.

## Part 4 — Portable Deployment

| # | Step | Expected | Outcome |
|---|---|---|---|
| 16 | Copy the entire `Test Run\` directory (with binaries already staged) to another location, e.g. `C:\LareTest\` | Copy succeeds (no absolute paths embedded within committed files) | |
| 17 | From `C:\LareTest\`: `.\init_orchestrator.ps1` | Launches exactly as in step 1, but `config dir:` and all derived paths (plugins, logs) point to the NEW location (`C:\LareTest\Configuration`, `C:\LareTest\plugins`, ...) — zero modifications required in `startup.json` (paths relative to deploy root, not the original location) | |
| 18 | From `C:\LareTest\`: `.\init_tauri.ps1` (without `--open`, if `orchestrator` is active) | Starts without crashing; without `--open` and without `lare-shell` (not yet built in this plan) no window opens automatically — expected behavior for this plan, not a defect | |
| 19 | Clean up: terminate processes started from the copy | Zero orphaned processes | |

## Part 5 — Shell Channel with Development Client

Validates the Plan 2a shell channel ([`02-decisions.md`](./02-decisions.md) ADR-018) without
`lare-shell` (Plan 2b C# host): `scripts/dev/shell-client.mjs` mimics the host — see
[`BUILD.md`](./BUILD.md) §"Development utility: speaking the shell channel without a real console" for
usage and the Git Bash/MSYS gotcha (run from **PowerShell**). Prerequisites: orchestrator and
`ui.exe` launched from `Test Run\` (Parts 1/2); running without `ANTHROPIC_API_KEY` is supported — the
orchestrator falls back to `StubAdapter`, sufficient for validating message routing (though not
response quality).

| # | Command | Expected in terminal (dev client) | Expected on `ui.exe` |
|---|---|---|---|
| 20 | `node scripts/dev/shell-client.mjs -- '/ping'` | `→ finestra "Lare — /ping" aperta` + `[done exit_code=0]` | Window opens: **"Lare — /ping"** containing the per-tier table (`lare-shell`/`orchestrator`/`plugin-ping`/`ui.exe`) |
| 21 | `node scripts/dev/shell-client.mjs -- '/nonesiste'` | Prints only `[done exit_code=0]` (no confirmation line: slash command silently discarded, orchestrator logs `discard slash`) | No window opens |
| 22 | `node scripts/dev/shell-client.mjs -- '/ai ciao'` | `[error routing_error] sintassi: /ai "testo" (virgolette obbligatorie)` | No window opens |
| 23 | `node scripts/dev/shell-client.mjs -- '/reset'` | `non applicabile: la sessione è la tua` + `[done exit_code=0]` | No window opens |
| 24 | `node scripts/dev/shell-client.mjs -- '/config'` | Prints only `[done exit_code=0]` (no confirmation line for `OpenUiLocal`) | Opens (or focuses) **"Lare — Configurazione"** window |
| 25 | `node scripts/dev/shell-client.mjs -- '/help'` | `→ finestra aperta` (spec §3.2 exception: `/help` does NOT additionally open the output window, generic confirmation line) | Opens (or focuses) singleton window **"Lare — Comandi"** |
| 26 | `node scripts/dev/shell-client.mjs -- '/open .'` | `→ finestra "Lare — /open" aperta` | Opens **"Lare — /open"** window displaying command outcome |
| 27 | `node scripts/dev/shell-client.mjs -- '/ai "ciao, chi sei?"'` | `[Y/n]` only if `StubAdapter` proposes a tool (otherwise direct to) `→ finestra "ciao, chi sei?" aperta` + `[done]` (`exit_code` **null**, AI execution path) | Opens **"ciao, chi sei?"** window (title taken from quoted text) containing response content |

## Part 6 — Mode B in Windows Terminal (`lare-shell` host)

Live verification of **Plan 2b** (ADR-015/ADR-019): the real C# host opened as the "Lare Terminal"
Windows Terminal profile (`install-wt-profile.ps1`), with `orchestrator.exe`/`ui.exe` **not** running
prior to opening the tab (`Get-Process orchestrator, ui -ErrorAction SilentlyContinue | Stop-Process`).
Conducted by controller on 2026-09-07 (no interactive terminal on implementer side; SendKeys driver
+ screenshots) — complete outcomes recorded in ledger
(`.superpowers/sdd/2026-09-06-piano-2b-host-lare-shell/progress.md`, section "E2E dal vivo (controller)"
[live E2E (controller)]).
Transcribed faithfully here; where the ledger did not explicitly annotate a step, the Outcome column
reads "Not verified" rather than assuming success.

**Machine constraints (first three passes)**: no `ANTHROPIC_API_KEY` or `Configuration\llms.json` →
`StubAdapter` (zero tool calls, zero real gates): items 5–9 were covered solely by automated test
suites (`SlashTurnTests`, `ExecutorTests`, `ws_integration.rs`).
**Fourth pass (2026-09-07, real AI)**: `llms.json` sourced from v1 deployment (`C:\Lare
Terminal\Local\llms.json` → `Test Run\Configuration\llms.json`, gitignored; active provider
`claude-direct`), verifying items 5–9 live in a new Windows Terminal window — outcomes in table.

**Two passes**: the first discovered an issue (§"Defects discovered" below), resolved via commit
`a8d148f` (2 files + new tests, 115/115); the second, following republication of `Test Run\`,
re-verified areas affected by the fix (autostart, reconnection).

| # | Check | Outcome |
|---|---|---|
| 1 | Tab launch banner; "orchestratore: NON connesso … avvio … orchestrator.exe" → "orchestratore avviato e connesso"; `ui.exe` appears (autostart §6.4) | OK — autostart orchestrator (~1 s) and `ui.exe`. First pass: defect, `ui.exe` (console app in debug builds) opened a Windows Terminal TAB stealing focus (see defect 1 below). Second pass, post-fix: OK, autostart without spurious WT tab |
| 2 | Prompt/profile matching pwsh (PSReadLine, aliases, oh-my-posh if installed); `Get-Date`, `dir`, `cd ..` function | OK banner/PSReadLine/`Get-Date`; OK `cd ..` persists in prompt (D17). `dir` was not explicitly logged as an isolated step in ledger — not distinctly verified |
| 3 | `/ping` → window "Lare — /ping" with tier rows + terminal confirmation line | OK (window + confirmation line); re-verified in second pass as part of reconnection verification (item 10) |
| 4 | `/help`, `/config`, `/library` → correct windows; `/nonesiste` → silent; `/ai x` without quotes → syntax error | OK `/help` → "Lare — Comandi"; OK `/config`; OK `/library` → "Lare — Archivio" (second pass); OK `/nonesiste` silent (discard log); OK `/ai x` → syntax error line |
| 5 | `/ai "list 3 largest files in this directory"` → `[Y/n]` → Enter → command executes IN terminal → Markdown window with result | **OK (fourth pass, real AI)**: three consecutive gates (two `cerca routine`, then `Get-ChildItem -File \| Sort-Object Length -Descending \| Select-Object -First 3 …`), `y` to each, table printed to terminal, window "list 3 largest files…" displays result, terminal displays `→ finestra … aperta`. **Anomaly observed once**: first two gates appeared pre-accepted (printed `y` with no keystroke sent) and third `y` leaked into next prompt — pending/duplicate keystrokes in console buffer; fix: gate flushes buffer prior to prompt and logs discarded keystrokes (`gate: scartato tasto pendente …`); subsequent passes exhibited zero pending or duplicated keys (log `gate: tasto Key=Y`) |
| 6 | `/ai "go to Documents folder"` → subsequent prompt reflects `Documents` (cwd persists, D17) | **OK (real AI)**: gate `cd ~\Documents; pwd` → `y` → `Path` printed → prompt `PS C:\Users\<user>\Documents>` |
| 7 | `/ai "delete all temporary files"` → `n` at gate → zero commands execute, turn finishes | **OK (real AI)**: gate `cerca routine` + `Get-ChildItem -Force \| Where-Object …` → `n` (log `gate: tasto Key=N`) → zero execution, turn concludes, window opens with AI response |
| 8 | Ctrl+C while awaiting AI turn, and Ctrl+C during lengthy AI-initiated `ExecInShell` → "annullato (Ctrl+C)"/"comando interrotto (Ctrl+C): turno annullato" | **OK (real AI, new WT window)**: (a) `/ai "count slowly to one million…"` + Ctrl+C after 2 s → `annullato (Ctrl+C)`, prompt; logs `Ctrl+C ricevuto (turno in corso: True)` / `turno … annullato`, and subsequent `Chunk`/`Done` for that turn are discarded as stale (contract c); (b) `/ai "run Start-Sleep -Seconds 60"` → `y` → Ctrl+C → `[LARE] comando interrotto (Ctrl+C).` + `comando interrotto (Ctrl+C): turno annullato`, prompt. Ctrl+C on a TYPED command (`Start-Sleep 30`, third pass): `[LARE] comando interrotto (Ctrl+C).` and prompt. In first two passes (host in `conhost`) line was missing: root cause was the **driver** (`SendKeys ^c` and `GenerateConsoleCtrlEvent` did not deliver Ctrl+C, even to a control `pwsh`) — not the host |
| 9 | `/ai "open interactive python"` → Python REPL usable, `exit()` returns to prompt | **OK (real AI)**: gate `python (interattivo)` → `y` → Python 3.14 REPL in terminal (`>>>`), input typed and executed, `exit` → line `→ finestra … aperta` and prompt |
| 10 | Terminate `ui.exe` → `/help` restarts it (self-heal, ruling 8); kill orchestrator (`Stop-Process -Name orchestrator`) → `/ping` → "orchestratore non raggiungibile … avvio …" → reconnected and window opens | Partially verified. OK: "killed orchestrator → `/ping` restarts and reconnects (same session)", `/ping` window opens (second pass). Manual termination of `ui.exe` followed by `/help` self-heal was not logged as an isolated step — not explicitly verified (autostart of `ui.exe` upon tab launch is verified, item 1) |
| 11 | `exit` → tab closes; `Get-Process lare-shell` → nothing; `Get-Process orchestrator, ui` → STILL running (detached processes) | OK, verified **twice**: first pass, "orchestrator and ui survive (even when host ran inside a closed WT tab)"; second pass, "orchestrator and ui alive" post-`exit`. The `Get-Process lare-shell` check was not transcribed in ledger verbatim — survival of the two detached processes is confirmed across both passes |
| 12 | Copy `Test Run\` to `%TEMP%\LareCopia\`, run `LareCopia\shell\lare-shell.exe --selftest` → `[OK]` against `LareCopia`'s Configuration (relative paths, §6.3) | **Not verified**: this scenario (copying to another folder, relative paths) was not logged in today's live e2e ledger. `Test Run\shell\lare-shell.exe --selftest` on the original deployment (not copied) was executed with `[OK]` on every check (Task 8, re-verified by controller) |

### Defects discovered during e2e (first pass) and resolutions

1. **`ui.exe` stole focus from the shell.** In debug builds, `ui.exe` is a console application
   (Tauri retains the console for logs); started with `WindowStyle.Normal` and Windows Terminal as
   default terminal, that console spawned as a NEW WT TAB. Fix: `Launcher.EnsureUi()` also spawns `ui.exe`
   with `hideWindow: true` (verified: actual windows created by `ui.exe` — Markdown, `/config`,
   `/library` — remain visible; only the debug console remains hidden). See ADR-019 item 6 (revised).
2. **WT profile fragment**: the symptom "failed to launch `Terminal`" stemmed from the SendKeys
   driver, not quoting; the final revision mandated **quoted** `commandline` arguments (without
   quotes, paths with spaces cause `CreateProcess` to search prefixes like `…\Progetti\Lare.exe`),
   fix wave `22dcb09`.
3. **Fragment requires Windows Terminal restart.** The user's WT window was already open when the
   fragment was installed → the "Lare Terminal" profile was not yet loaded in that window → the
   **first pass** was conducted in a **conhost** window (not WT), with `ui.exe` launched manually
   (`-WindowStyle Hidden`) rather than via profile. The **second pass**, post-fix and following
   re-staging of `Test Run\`, re-verified autostart and reconnection.

E2E fix wave: commit `a8d148f` (2 files + new tests, 115/115).

**Incident note during e2e.** During the first pass, operated via SendKeys and UI Automation, the
controller terminated an existing "PowerShell" tab with `exit` inside the user's Windows Terminal
window (`wt -w new` had opened a second WT window within the same process, and enumeration targeted
the first): almost certainly a user tab unrelated to this e2e — irrecoverable, noted for full
transparency.

## Part 7 — Mode A (Plan 3)

> **Live execution: partially completed.** This part covers the checklist from spec §10 for **Mode
> A** (`ui.exe` with terminal window, xterm.js + ConPTY). Task 7 of this plan (documentation) did not
> execute it live — the author lacked an interactive GUI environment to launch `ui.exe` and observe
> windows. The controller executed it live AFTER plan completion (using the same keyboard-driven
> methodology from Part 6 — `scripts/dev/e2e-driver/`, SendKeys + screenshots + UI Automation, README
> with takeaways), including the window-close fix emerging from this pass (commit `07c584c`), as well
> as identifying and fixing a second defect live (restart button hidden behind xterm.js viewport,
> `z-index`, commit `80cd46a`). **Steps 1/2/5/6/7/9/10, Mode B/autostart Task 6 checks, and the
> restart button were executed live by the controller post-plan; remaining to run live are `/calc`,
> `/config`/`/library`/bar buttons, duplicate `/aichat`, launch without autostart, and window resize.**
> Following the principle of prior Parts: record REAL observed outcomes, never assume them.

Prerequisites: builds completed (`cargo build`, `cargo build -p ui`, `cargo build -p plugin-calc`)
and `.\deploy_test_run.ps1 -IncludePlugins` executed (also publishes `lare-shell` to `Test Run\shell\`
and `calc.exe` to `Test Run\plugins\calc\`, without which step 3 cannot respond) — see
[`RUN.md`](./RUN.md) §"Mode A".

| # | Step | Expected | Outcome |
|---|---|---|---|
| 1 | From `Test Run\`: `.\ui.exe` (WITHOUT `--no-terminal`) | Terminal window opens (xterm.js in ConPTY) hosting `lare-shell.exe` — pwsh prompt visible, PSReadLine banner | **OK.** Launched standalone `ui.exe` (no active orchestrator): self-heal spawned `orchestrator.exe` AND `lare-shell.exe` (pty child via ConPTY); banner "Lare Terminal 2.0.0 — sessione \<id\>" (8-character hex ID confirmed, e.g. `1fe87d14`), "orchestratore: connesso", real PowerShell prompt |
| 2 | Type `/ping` into the in-window shell | Window "Lare — /ping" displays per-tier table; confirmation line logged in terminal | **OK** (typed directly, not via button) — confirmed via terminal acknowledgement line ("→ finestra "Lare - /ping" aperta"); window contents not inspected visually |
| 3 | Type `/calc` (requires `-IncludePlugins` deployment, see above) | Plugin `calc` responds as deployed in `Test Run\plugins\calc\` | To be executed |
| 4 | `/config`, `/library`, `/help` — typed once in shell, clicked once from the 4 `.slash-btn` buttons above terminal (`terminal.html`: `/help`, `/library`, `/aichat`, `/config` — NOT `/calc`/`/ping`, which lack dedicated buttons) | Matching windows open in both cases (typed and clicked), identical outcomes | To be executed — typed `/help` (not via button) verified separately: OK, opens Markdown window "Lare — Comandi" with real content; `/config`/`/library` and all `.slash-btn` buttons were not tested |
| 5 | `/ai "list 3 largest files in this directory"` | Terminal `[Y/n]` gate → `y` → command executes in terminal → Markdown window with result | **OK**, with real AI (`llms.json` present, Anthropic provider) — tested with `/ai "count how many .ps1 files are in this folder"`: two consecutive `[Y/n]` gates (routine search, then PowerShell command), both accepted, result "0", Markdown window opens with request title and terminal acknowledgement |
| 6 | Following step 5 command, observe OSC 9001 badge in terminal window | Badge illuminates (`intercept` event received via `registerOscHandler(9001, …)`) | **OK.** Verified that "last: /command" badge updates correctly for `/help`, `/ping`, and `/ai "…"` with entire quoted text |
| 7 | During lengthy `/ai "…"` turn (non-instantaneous reply) | ActivityIndicator badge illuminates for duration of turn and turns off on `Done`/`Error` | **OK.** Confirmed visually (screenshot) that indicator illuminates during `/ai` turn (from gate onwards) and turns off post-`Done` — end-to-end chain orchestrator→host.js→Tauri event→terminal.js operates correctly |
| 8 | Type `/aichat` twice in succession | A single AI Chat window opens (singleton), not two | To be executed |
| 9 | Close terminal window | Child `lare-shell.exe` process terminates with it — zero orphaned processes (Task Manager) | **OK, but ONLY AFTER window close fix (commit `07c584c`)** — prior to fix, `lare-shell.exe` and `ui.exe` remained alive post-close (real bug, uncovered by this exact e2e step). Post-fix: verified via `Get-Process` that `lare-shell.exe` and `ui.exe` terminate cleanly; `orchestrator.exe` remains running (expected, independent daemon) |
| 10 | Kill `orchestrator.exe` BEFORE launching `ui.exe`, then launch `ui.exe` (Mode A) | Self-heal (Task 3): `ui.exe` spawns `orchestrator.exe` autonomously; terminal window functions normally (zero user-visible errors) | **OK.** Step 1 (launching standalone `ui.exe`) demonstrates this as well |
| 11 | Launch `ui.exe` without orchestrator AND with `autostart.orchestrator` disabled in `startup.json` | "non raggiungibile" message in terminal; shell remains usable for local commands (`dir`, `cd`, …) — only `/…` commands fail | To be executed |

**Additional live verifications conducted without a dedicated table row** (spec §10 did not define
separate rows — recorded here rather than forcing into existing rows):

- **Mode B (`ui.exe --no-terminal`)**: OK. `ui.exe --config-dir … --no-terminal` triggers orchestrator
  self-healing but NO terminal window (0 "Lare Terminal" windows via UIA) and NO `lare-shell.exe`.
- **Orchestrator autostart of `ui.exe` (Task 6)**: OK. Started ONLY `orchestrator.exe` (no `ui.exe`),
  then issued a `/help` command via `node scripts/dev/shell-client.mjs` (simulating a shell session
  without `ui.exe`): orchestrator autonomously started `ui.exe --no-terminal` (confirmed: `ui` process
  appears, zero terminal windows, zero `lare-shell.exe`), window "Lare — Comandi" opened, ack line
  "→ finestra aperta" printed (rather than the `NO_UI_ACK` fallback).
- **Restart button after external `lare-shell.exe` crash**: OK. Also verified the defect discovered
  in the banner during this pass (button painted under opaque xterm.js viewport, unclickable — fixed
  via `z-index: 1` on `#restart-banner`, commit `80cd46a`): post-fix, "shell terminated (exit code
  …)" banner displays over the terminal (confirmed via screenshot); clicking "restart" revives
  `lare-shell.exe` (new PID, same session), `term.reset()` clears screen, prompt operational.

**Explicitly unverified live rows/steps** (retained as "To be executed", avoiding assumed outcomes):
row 3 (`/calc`); row 4 (`/config`/`/library`, neither typed nor clicked via buttons; bottom bar
buttons in general — never clicked, typed directly); row 8 (`/aichat` twice → single window); row 11
(launching `ui.exe` without orchestrator AND with autostart disabled); window resizing (no re-fit
verification).
