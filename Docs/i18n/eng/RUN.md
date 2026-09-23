# RUN — Launching Lare Terminal 2.0

How to launch the program **once it is compiled and deployed** ([`BUILD.md`](./BUILD.md) →
[`DEPLOY.md`](./DEPLOY.md)). This document does not cover building or deployment — only which
executable to launch and why.

## The definitive method (recommended)

From `Test Run\` (or the folder where you copied the deployment):

```powershell
.\ui.exe
```

**A single command.** The Lare terminal window opens (xterm.js inside a ConPTY) hosting a real
PowerShell shell; if the orchestrator is not already running, it launches it automatically
(self-healing). No manual background processes to start, no need to open Windows Terminal, no
profile to install: `ui.exe` **is** the application, complete and self-contained. This is **Mode A**
from the specification — the only way designed for everyday use, not one alternative
among equals.

Closing the window terminates everything (the child `lare-shell.exe` process and `ui.exe` itself) —
leaving zero orphaned processes, while the orchestrator remains running in the background as intended
(a shared daemon decoupled from any single window).

The remainder of this page describes alternatives and special cases: if the single command above
meets your needs, nothing else is required.

## Why Mode B also exists

Users preferring a standalone tab inside their **own** Windows Terminal (without `ui.exe`'s HTML
chrome — title bars, status indicators, buttons — using purely the PowerShell prompt with slash
commands still functioning) can use **Mode B**: `lare-shell.exe` launched directly via a dedicated
Windows Terminal profile, rather than within `ui.exe`. Same shell, same engine, same `/…` commands —
differing only in the graphical container. Installed once (see below), it functions like any standard
Windows Terminal tab.

Both modes **are not mutually exclusive**: you can have Mode B installed and still run `ui.exe`
whenever preferred, or vice versa. They do not share processes: they are two distinct ways to launch
the same `lare-shell.exe`.

**Mode A cannot become a Windows Terminal profile** — not by choice, but due to an architectural
boundary: WT hosts *console-subsystem* processes attached to its own managed ConPTY, rendering their
output in its tab. `ui.exe` is a *GUI-subsystem* application in release builds (`windows_subsystem =
"windows"`, `main.rs`) — the terminal visible in Mode A is drawn by xterm.js within its Tauri window;
its ConPTY connection to `lare-shell.exe` is internal plumbing, invisible to the Windows console
subsystem. Directing a WT profile to `ui.exe` would at best result in a blank tab with the actual
window opening separately — not an embedded tab. For convenient Mode A launches, the natural approach
is a shortcut or taskbar icon pointing to `ui.exe`, not a WT profile.
`install-wt-profile.ps1` exists exclusively for Mode B.

## Mode A — Details

```powershell
.\ui.exe
```

What happens, in order:
1. `ui.exe` verifies whether the orchestrator responds on `127.0.0.1:<ws_port>`; if unreachable and
   `autostart.orchestrator` is enabled in `startup.json` (default: yes), it launches it automatically
   and awaits connection for up to 5 seconds (self-healing, spec §6.4).
2. The terminal window opens: inside, `lare-shell.exe` (the custom C# PowerShell host) runs in a
   pseudo-console (ConPTY), presenting a genuine PowerShell prompt — including PSReadLine, history,
   and Tab completion.
3. Slash commands `/…` (`/help`, `/ping`, `/ai "…"`, …) function as in any other Lare shell; results
   render in a dedicated window (Markdown, tabular, etc.), with a confirmation line logged in the
   terminal.
4. If the internal shell crashes or exits, the window displays a "shell terminated" banner with a
   **restart** button — avoiding having to close and reopen `ui.exe`.
5. Closing the window (X, Alt+F4) terminates `lare-shell.exe` and the `ui.exe` process. The
   orchestrator continues running (as a shared daemon surviving window closures).

No flags are required for normal use. `ui.exe --no-terminal` exists for internal use (the C# host in
Mode B and orchestrator autostart pass it when requiring window hosting without an unwanted secondary
terminal) — never needed manually in daily use.

## Mode B — Details

**One-time installation**, from `Test Run\` (or the folder where the deployment was copied):

```powershell
.\install-wt-profile.ps1
```

Writes a JSON fragment to `%LOCALAPPDATA%\Microsoft\Windows Terminal\Fragments\Lare\`, parsed by
Windows Terminal **only upon its own startup**: after installation, **restart Windows Terminal**
(close all windows) — opening a new tab inside an existing window is insufficient.
`.\uninstall-wt-profile.ps1` removes the fragment.

**Everyday usage**: open a new tab using the "Lare Terminal" profile in Windows Terminal (via tab
dropdown or assigned shortcut). Same self-healing behavior as Mode A: `lare-shell.exe` launches
`orchestrator.exe` and `ui.exe` automatically if missing.

`ui.exe`, when launched via host self-healing in this mode, runs with `--no-terminal` (no terminal
window — you are already inside the Windows Terminal tab): Markdown/config/library windows still
appear when triggered, without opening an extra terminal window.

## Development mode (code authors only) — NOT the standard way to run the application

When actively developing source code and testing newly compiled binaries **without** staging
`Test Run\` on each iteration ([`DEPLOY.md`](./DEPLOY.md)), you can execute directly from
`target\debug\`. An explicit `--config-dir` is **mandatory** (otherwise resolution looks for
`target\debug\Configuration\`, which does not exist):

```powershell
# Terminale 1
cargo run -p orchestrator -- --config-dir "Test Run\Configuration" --console-log

# Terminale 2
cargo run -p ui -- --config-dir "Test Run\Configuration"
```

`--console-log` outputs orchestrator logs to this console in addition to the daily file in
`Test Run\Configuration\logs\` (without the flag, logs go to file only — tailored for autostart,
where no user is watching a console).

Similarly for the C# host from source (see [`BUILD.md`](./BUILD.md)):

```powershell
dotnet run --project shell/lare-shell/src/LareShell -- --config-dir "Test Run\Configuration"
```

**Gotcha: terminal with redirected stdin (e.g. Claude Code) → no PSReadLine, no interactive gate.**
`lare-shell` distinguishes "PSReadLine available but unused" (redirected stdin, fallback to
`Console.ReadLine`) from "PSReadLine unavailable" (different banner); with redirected stdin, the
`[Y/n]` gate reads a text line and **fails closed** upon EOF (no real keyboard = no one to prompt).
For authentic testing (PSReadLine, keypress `[Y/n]` prompts, Ctrl+C), use a real interactive console
— Windows Terminal, not the terminal of a tool like Claude Code — see [`TESTING-e2e.md`](./TESTING-e2e.md).

## Advanced case: live orchestrator console logging

Unnecessary for standard usage — both `ui.exe` (Mode A) and `lare-shell.exe` (Mode B) launch the
orchestrator autonomously if missing. However, they launch it **detached and silently**, without
console logging (writing only to the daily log in `Configuration\logs\`). To observe live
orchestrator logs during debugging (or with `autostart.orchestrator` disabled in `startup.json`),
start it manually BEFORE `ui.exe` or Mode B, from `Test Run\`:

```powershell
.\debug_orchestrator.ps1
```

Equivalent to `orchestrator.exe --console-log`. Once responding, launch `ui.exe` or Mode B normally —
they detect the active orchestrator and avoid launching a second instance.

(Startup order is irrelevant — each component waits and retries for the other). For standard usage,
Mode A alone is completely sufficient.
