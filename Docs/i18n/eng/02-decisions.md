# Decisions (ADR)

Architectural decision log. For each entry: **context**, **options**, **decision**, **why** (trade-offs, not just the verdict), **consequences**. This document captures the *reasoning* — the easiest thing to lose. See also [`01-architecture.md`](./01-architecture.md), the same story told as a single thread rather than decision by decision.

> **Note on continuity with v1.** ADR-001..014 are copied intact from the decision log of the
> [previous version of the project](https://github.com/mauriziolobello/lare-terminal), including
> their internal references to other documents (`07-ux-and-config.md`, `08-persistent-shell.md`,
> `09-open-app.md`, `10-custom-windows.md`, `ENVIRONMENT.md`, a spec dated 2026-07-15): those files
> live ONLY in the previous repository, not under this repo — accessible from there. Numbering
> continues from ADR-015 with decisions specific to 2.0.

---

## ADR-001 — Role of MCP: where the "AI brain" lives

**Context.** "MCP server in the backend to allow any AI to interface" is ambiguous. MCP has a directionality: an AI host (client) invokes tools exposed by a server.

**Options.**
- A) Backend = pure tool server; the AI is an external host.
- B) Backend = brain with a pluggable AI adapter; uses MCP internally.

**Decision.** A **3-tier** architecture taking the best of both: MCP server (reusable tools) + orchestrator with AI adapter (swappable AI) + channels (UI/Telegram/voice).

**Why.** The key misunderstanding: **MCP standardizes tools, not AI**. "Any AI" encompasses two distinct desires requiring two mechanisms:
- "capabilities reusable across diverse AIs / other tools" → **MCP server**
- "ability to change the brain (Claude→GPT→local)" → **AI adapter**
Furthermore, a pure MCP server cannot handle streaming conversation toward a cursor: an orchestrator is needed *in front*.

**Consequences.** "Step 1 = communication interface" consists of **two contracts**: UI↔Orchestrator and Orchestrator↔MCP Server.

---

## ADR-002 — Stack: Rust/Tauri vs Electron vs native (C/Swift)

**Context.** Need a transparent overlay + hotkey + animated cursor, with Win/macOS parity and attention to performance.

**Options.** TypeScript+Electron · C#/.NET+Avalonia · Rust+Tauri · C(Win)+Swift(macOS).

**Decision.** **Rust + Tauri** (UI), backend entirely in Rust.

**Why.**
- Clarified the "interpreted = slow" misconception: Electron JIT-compiles (compute is fine). Electron's problem is the **footprint** (Chromium ~150MB, high RAM), not speed.
- **No language choice makes AI responses faster** — perceived latency is network/model + "keep app warm" strategy. The language impacts footprint, cold-start, and overlay snappiness.
- C(Win)+Swift(macOS) would be the most "native" route, but **betrays parity**: two codebases, continuous divergence. A single cross-platform codebase guarantees parity by design.
- Tauri: native compiled core, minimal footprint, fancy cursor still in HTML/CSS.

**Trade-off accepted.** Learning curve of Rust (the user has never used it; proceeds with supervision). In Rust, some AI parts are written by hand (no official Anthropic SDK) — but it is just HTTPS+JSON.

---

## ADR-003 — Voice: Whisper, neither Web Speech nor native OS APIs

**Context.** Voice is a future phase, but the UI choice must not corner us later.

**False dilemma identified.** Appeared to be "guaranteed voice (Electron/Web Speech) **or** lightweight footprint (Tauri)". This is false.

**Why it dissolves.**
- Web Speech API pairs poorly: on macOS WKWebView it is limited/absent; in Electron it has historical quirks.
- Native OS voice APIs (Windows Speech vs Apple Speech) are **different engines** → no parity.
- **Whisper** (whisper.cpp) is a **single engine**, identical across Win/macOS, offline, more accurate, **independent of the webview**.

**Decision.** Voice will be a **Whisper sidecar**. Consequence: Tauri **does not** corner us on voice → Electron's sole advantage (consistent Web Speech) evaporates, leaving only its weight. Indirect confirmation of ADR-002.

---

## ADR-004 — Topology: orchestrator as a separate daemon (not fused into the UI)

**Context.** In Tauri, the orchestrator could live inside the core (2 processes), or be an autonomous daemon (3 processes).

**Decision.** **Separate daemon.**

**Why.** Telegram and voice are communication tools **independent of the UI**: if the UI closes, they must continue working. Bonus: the orchestrator contract becomes explicit and testable in isolation (`wscat`/`curl`/scripts).

**Cost accepted (~half to one extra day).** Concentrated in: autostart/lifecycle (the only piece without cross-platform parity — Run key vs LaunchAgent), security handshake, discovery/single instance. **Start minimal** (background process launched from terminal) and harden later (Phase 5).

---

## ADR-005 — Default AI: Claude Opus 4.8 via HTTPS Messages API

**Context.** Which model to choose and how to invoke it from Rust.

**Decision.** Default **`claude-opus-4-8`** (Opus 4.8), via **Messages API over HTTPS** using `reqwest` + SSE streaming; `thinking: {type:"adaptive"}`. Behind the `AIAdapter` interface (swappable).

**Why.** Opus 4.8 is the recommended default for capability. No official Anthropic SDK exists for Rust → direct HTTP calls (simple, JSON only; identical across platforms by design). The adapter retains swappability (GPT/local = another trait implementation) without touching the rest.

---

## ADR-006 — Conventions: TDD, SOLID, CHANGELOG/IMPLEMENTATION, private repo, Sonnet agent

**Decisions (user requirements).**
- **Non-negotiable TDD.** Ironclad rule: *no production code without a prior failing test*. RED → GREEN → REFACTOR cycle, with mandatory verification that the test fails for the right reason before implementing. Applies to bugfixes as well.
- **Non-negotiable SOLID** (exceptions only for immediate necessity, to be cleaned up). In Rust: SRP=focused modules/crates; OCP/DIP/ISP=`trait` as boundaries; + composition (consistent with "composition over inheritance").
- **`CHANGELOG.md` for each subproject** with `major.minor.update` semver.
- **`IMPLEMENTATION.md` for each subproject** with current implementation details.
- **Private GitHub repository** from day one.
- **Workflow:** code is built by a **Sonnet agent** (`lare-builder`, `claude-sonnet-4-6`); supervision (tasks, reviews, TDD/SOLID/contract adherence, tests, docs).

**Why.** Quality and traceability from day 1; clean separation between construction (Sonnet) and review (supervisor). Didactic bonus of TDD: the test written first serves as the **readable specification** of behavior — reading it before implementation accelerates learning Rust syntax.

---

## ADR-007 — Channel security: command execution and 2FA on Telegram — **implemented (Telegram channel, v0.11.0)**

**Context.** An automated security review rightly flagged that `run_os_command` executes arbitrary OS commands (CRITICAL) and that unvalidated `cwd` permitted an NTLM leak via UNC paths on Windows (HIGH).

**Decisions.**
- **Command execution = intended feature.** Executing OS commands is the purpose of the product, not a bug. Mitigations: allowlist + confirmation for dangerous commands (TODO Phase 2), WS localhost only (`127.0.0.1`) + token.
- **`cwd` hardening (done, Phase 1):** `validate_cwd` rejects UNC/device/verbatim paths (before touching the filesystem), non-absolute, non-existent → blocks the NTLM vector. (mcp-server v0.1.1)
- **Telegram = untrusted remote input → requires 2FA.** The Telegram channel features a **2FA TOTP recognition procedure** (RFC 6238, Google Authenticator) + **one-time code pairing**, **independent** of simply having access to the chatbot. Implemented in v0.11.0.

**Gate implemented (v0.11.0).** Before each OS command or `/open`/`/web` slash command, the Telegram channel:
1. Verifies an active TOTP session (`/login` requests the TOTP code every 30 minutes).
2. If the command requires confirmation (`needs_confirmation`): sends inline buttons (`OK` / `Annulla`) and awaits callback — the command executes **only if the user clicks OK** within 2 minutes.
3. Rate-limiting: 5 consecutive failures (pairing or login) → 1-minute lockout.

**In-loop gate on AI tools (v0.12.0).** Extension closing the residual gap: an NL command inducing the AI to use `run_in_session`/`open_target` is now **not** executed directly. `AiAdapter::respond` accepts `confirmer: Option<&dyn ToolConfirmer>`; on Telegram it is `Some(TelegramConfirmer)` → each tool proposed by the AI asks `[Esegui]/[Annulla]` (opaque ID, session re-verified before and on tap). In local UI it remains `None` (autonomous, unchanged).

**Why.** The truly dangerous vector is not the local machine (WS localhost+token) but the **remote channel**: anyone discovering the bot could dispatch commands. 2FA + gate close that vector at the root; the in-loop gate (v0.12.0) closes it even when it is the AI — not the user — choosing to execute a command.

**Consequences.** Complete 2FA gate for OS + `/open`/`/web` via Telegram **and** for each `run_in_session`/`open_target` proposed by the AI within the NL loop (v0.12.0). Non-gated slash commands (`/show`, `/reset`, etc.) do not require explicit confirmation — TOTP login already acts as session boundary. Local UI remains autonomous (no confirmer).

**Per-tool extension (2026-07-15).** The gate was per-channel (`Some`/`None` decided the entire channel). `ToolConfirmer::should_gate(tool_name)` (default: gate everything except `show_markdown`, preserving Telegram) shifts granularity to individual tools: a `LocalUiConfirmer` gates ONLY an explicit `SENSITIVE_TOOLS` list (empty at introduction), leaving `run_in_session`/`open_target` autonomous locally as always. Rationale: future external tools (dedicated MCP servers, e.g. nmap) may have sensitive effects (network-wide, not just local machine) warranting confirmation locally as well, without degrading fluid daily usage of the local UI. Design: a spec dated 2026-07-15 (see continuity note at top of document).

---

## ADR-008 — Configurable activation hotkey

**Context.** The summon hotkey (default `F2`) must not be hardcoded.

**Decision.** The hotkey is an **early configuration parameter**: any function key or key combination (`Ctrl+Alt+T`, `Opt+T`, …). Details and implications in `07-ux-and-config.md` (previous version of the project).

**Why.** User preferences and conflicts with other apps/OS. `tauri-plugin-global-shortcut` already supports arbitrary accelerators, so the cost is expose+persist+re-register, not implementing from scratch. **Consequence:** design a config layer from the start (do not hardcode `F2`).

*Continuity note (2.0): the hotkey and the overlay to which it applied did not survive the model shift described in ADR-016 — this decision remains as part of v1 history.*

---

## ADR-009 — Linux as an official target (tri-platform project)

**Context.** Beyond Windows (initial) and macOS, evaluate Linux as a target.

**Decision.** Linux is an **official target**: a **tri-platform** project (Windows/macOS/Linux).

**Why.** Negligible cost now: the stack is already cross-platform, and the `#[cfg(unix)]` branch of `mcp-server` covers Linux alongside macOS. Deciding now prevents the codebase from maturing under the assumption of only two platforms.

**Known caveat (open risk).** On **Wayland**, the security model restricts global hotkeys and focus-stealing (an app cannot seize them at will); on **X11**, it is permissive like Windows. → A **dedicated Wayland hotkey/focus spike** will be needed (potentially via portal/compositor) once a Linux machine is available, analogous to the Windows spike. System equivalents in `ENVIRONMENT.md` (webkit2gtk, XDG/systemd autostart) — previous version of the project.

**Consequences.** Documents made tri-platform; Phase 6 expanded to macOS **and** Linux with the Wayland spike.

---

## ADR-010 — Slash commands ("meta" input from the cursor)

**Context.** Need "meta" functions (configuration and future utilities) accessible from the same cursor, without menus.

**Decision.** Input prefixed with **`/`** = **slash command**, a **third category of input** alongside OS and NL. First command: **`/config`** (opens a dialog). See `07-ux-and-config.md` (previous version of the project) for the dialog and parameters.

**Handling (Phase 1).** Slash commands are **handled by the UI** (`/config` opens a local configuration window, which persists settings to the config file). In the future, certain slash commands may route to the backend; the `/` prefix remains the discriminator. The orchestrator `router` may gain a `Route::Slash`, but it is currently unnecessary (UI intercepts prior to sending).

**Why.** Familiar pattern (like AI slash commands), discoverable, and does not pollute OS/NL routing: `/` is a clean discriminator like `$` for the OS.

---

## ADR-011 — Persistent shell session (replaces one-shot execution)

**Context.** Execution was one-shot (`cmd /C …` fresh for each command) → `cd`/env do not persist. The user wants "the cursor to BE a terminal session".

**Decision.** Replace `run_os_command` with a **persistent shell** in `mcp-server`: `run_in_session`. Default **PowerShell** (Windows) / **sh** (Unix). Approach **A**: piped stdin/stdout + **markers** for clear boundaries and exit codes (NOT a PTY → no colors/interactive programs for now). Full spec: `08-persistent-shell.md` (previous version of the project).

**Why.** It reflects the model of a real terminal (persistent state) and satisfies the confirmed requirement. A full PTY/emulator (vim, colors) is a future upgrade, unnecessary now (YAGNI). The **WS protocol remains unchanged** (Chunk/Done) — changes occur only inside `mcp-server`, thanks to capability isolation in the MCP server.

**Consequences.** `cwd` is no longer a per-command parameter (it is session state) → the anti-UNC parameter guard does not apply to this path. `/reset` slash command to restart the session. First surface in a series ("response surfaces": open-native-app and custom-windows follow).

---

## ADR-012 — Surface 2: `/open` (open native app) + UI-local vs backend slash

**Context.** Second response surface: opening the right native app (folder→Explorer, URL→browser, file→app). Having the AI choose autonomously is Phase 2.

**Decisions.**
- **Capability:** MCP tool `open_target` in `mcp-server`, via `opener` crate (cross-platform, no shell string). Spec: `09-open-app.md` (previous version of the project).
- **Trigger:** explicit command **`/open <target>`** (no heuristics → no ambiguity with `cd`/commands until AI is integrated).
- **Slash routing (ADR-010 refinement):** slash commands split into **UI-local** (`/config` → dialog) and **backend** (all others → forwarded to orchestrator, `Slash` route, dispatched to `/open`/`/reset`/unknown). WS protocol unchanged.

**Why.** Immediate utility without the AI; the same capability will be used by the AI in Phase 2. Bonus: closes the `/reset` TODO (Slice B) — becomes a functioning backend slash alongside `/open`.

---

## ADR-013 — Surface 3: custom windows (Markdown)

**Context.** Third surface: a rich window for displaying formatted responses (explanations, code, tables) — the substance for the AI's learning intent.

**Decisions.**
- **Capability = UI** (Tauri owns windows), not `mcp-server`. Therefore **the protocol extends** (for the first time): `ServerMsg::OpenWindow { title, kind, content }`, `WindowKind::Markdown`. Additive. Spec: `10-custom-windows.md` (previous version of the project).
- **Flow:** orchestrator emits `OpenWindow` → UI creates a new `WebviewWindow` and renders Markdown.
- **Trigger:** backend slash **`/show <markdown>`** (explicit now; AI emits the same message in Phase 2).
- **Security:** Markdown→HTML **sanitized** (pinned, local vendored `marked`+`DOMPurify`; no CDN; no raw innerHTML). Alternatively, a minimal DOM-based renderer.

**Why.** Building the *rich output channel* before the brain (AI) that will populate it. Additive contract extension → zero breaking changes.

---

## ADR-014 — AI connectivity: direct provider AND proxy (OpenRouter) — [PLANNED, not implemented]

**Context.** Extension of ADR-005 (pluggable AI adapter). In addition to connecting **directly** to a provider (e.g. Anthropic), the project must support **proxies/aggregators** such as **OpenRouter** (an OpenAI-compatible API routing to many models).

**Decision (to implement in Phase 2).** `AIAdapter` will feature (at least) two families of implementations:
- **Direct:** Anthropic Messages API (default `claude-opus-4-8`), already planned in ADR-005.
- **Via proxy:** OpenRouter / OpenAI-compatible endpoint (configurable base URL + API key, user-selected model).
Selection and credentials via configuration (see `07-ux-and-config.md`, previous version of the project; secrets kept outside repo).

**Non-negotiable constraint — MCP compatibility.** Any chosen AI/proxy must support **tool/function calling**, as the orchestrator (MCP client) drives `mcp-server` tools via the AI's tool-use loop. A model/proxy lacking function calling is **unacceptable** (would break tool access). The `AIAdapter` abstraction must therefore expose tool-use uniformly, mapping MCP tools to the provider/proxy format.

**Why.** Flexibility and cost efficiency (OpenRouter offers access to numerous models via a single integration) without compromising architecture: MCP remains the capability bus, the adapter normalizes tool-use. **Recorded only for now**: no code until Phase 2 is addressed.

---

## ADR-015 — Shell side: custom PowerShell engine host in C# (2026-09-04)

Context: 2.0 wants `/…` commands directly on the command line of a PowerShell session and AI commands executed within the user's shell. Decision: `lare-shell` is a custom host for the PowerShell engine (like `pwsh.exe`), not a hook on an external shell. Consequences: C# component; pwsh-flavored Linux/macOS.

**Verified via a disposable spike** (2026-09-04/05, ~1700 C# lines, code not promoted as base for final implementation): hosted runspace with its own `PSHost`/`PSHostUserInterface`/`PSHostRawUserInterface`, PSReadLine imported from the same path as an installed pwsh 7.6.5, line reading via `PSConsoleHostReadLine` (same mechanism as `ConsoleHost`), `/…` lines intercepted and not executed, every other line executed with `AddScript | Out-Default`.

Interactive test outcomes across two rounds (Windows Terminal 1.24, pwsh 7.6.5, Python 3.14.7): prompt/colors/history/Tab completion matching pwsh, `/…` commands intercepted correctly, real commands and persistent `cd`, Ctrl+C on a long-running command — **confirmed in the first round**. An interactive Python REPL (`python`, native command) **failed** in the first round (infinite traceback loop) and was resolved in the second. Fixed status bars drawn directly in the terminal (DECSTBM scroll region) required a second round to work without visual overlaps.

Technical, non-obvious lessons verified empirically in the spike:

- **ANSI escape sequences must be constructed from explicit character codes** (`(char)0x1B`), never from a string literal `"\x1b…"` in C#: `\x` in a C# string consumes up to 4 subsequent hexadecimal digits, so `"\x1b7"` produces not ESC followed by "7", but a single wrong Unicode character (U+01B7) — the cause of **all** rendering glitches observed in round one.
- **An intermediate cmdlet placed between a native command and `Out-Default` deprives it of a real console**: an interactive native program (the Python REPL in test 3b) stops functioning if anything sits in the pipeline between it and the final output — e.g. a `Tee-Object` used to count passing objects redirects native process stdout to a pipe, and a program expecting a real console (like interactive Python) fails when querying screen dimensions. With pure `line | Out-Default`, as `ConsoleHost` does, the child inherits the real console.
- **`NotifyBeginApplication`/`NotifyEndApplication` are not optional**: they save and restore console modes around external program execution — without them, the host interface remains in an inconsistent state after an interactive external program terminates.
- **A hosted runspace does not inherently inherit the execution policy of a real pwsh**: in the spike, it was forced in-process (`RemoteSigned`) purely to load PSReadLine — a choice explicitly noted as "to be reconsidered in the product, not blindly copied" (subsequently resolved in ADR-019).
- **PSReadLine with redirected input enters a loop**: it must be explicitly bypassed when `Console.IsInputRedirected`, just as `ConsoleHost` does.

**Discovered cost, not a flaw of the decision**: fixed status bars function, but **erase the scrollback buffer** — a high price for a real terminal. This prompted the question resolved in ADR-016: housing that same host inside its own application window (Tauri + xterm.js emulator + ConPTY), with bars and indicators in HTML outside the terminal area, and scrollback handled by the emulator rather than the host.

## ADR-016 — Lare Terminal is a Tauri window with xterm.js + ConPTY (2026-09-05)

Decision: the host runs inside a Tauri window with an xterm.js emulator; status bars and badges in HTML. The v1 F2 overlay is retired; `ui.exe` remains a window host with a hidden host page replacing the cursor.

**Verified with a second disposable spike** (2026-09-05, built upon first spike code, adding a `--no-bars` flag to disable in-terminal bars and introducing an OSC 9001 signaling channel): Tauri 2.11.5 vanilla JS, window with three-row CSS grid (top indicator bar, terminal area, bottom quick-command bar), vendored xterm.js 6.0.0 + addon-fit (Campbell theme, Cascadia Mono font), `portable-pty` 0.9.0 for ConPTY integration on Windows (Tauri commands `pty_spawn`/`pty_write`/`pty_resize`, chunks in base64 to avoid corrupting multi-byte UTF-8 split across JSON boundaries), a direct host→emulator channel (custom OSC 9001 sequence, intercepted by `registerOscHandler`) to illuminate an "AI working" indicator without passing through the orchestrator protocol.

Interactive test outcomes: **confirmed across the board**, including the core driver for the change — scrollback is fully restored and mouse-wheel accessible (eliminating the previous spike's cost), resizing smoothly adapts terminal and bars (minor flicker addressed via debounced fit or WebGL renderer, not a blocker), bottom quick-buttons write commands into the shell, OSC 9001 badge lights up reliably (ConPTY passes custom sequences through — a fallback channel via window title proved unnecessary), and closing the window leaves no orphaned `lare-*` processes.

Technical facts observed, relevant beyond the spike:

- **ConPTY does not signal end-of-output (EOF) when the child process exits**: the only reliable indicator is explicitly waiting for process termination (`Child::wait()`) on a dedicated thread — the final product's *exit watcher* originated from this observation.
- **Tauri's `generate_context!` embeds `frontendDist` at compile time**: modifying only frontend files without touching Rust code may not trigger crate recompilation — requires `cargo clean -p <crate>` or a `build.rs` with `rerun-if-changed` on the frontend directory. An operational gotcha kept in mind throughout subsequent development.
- Custom OSC sequences traverse ConPTY (at least on Windows 11/WT 1.24): a useful direct host→emulator channel in production, complementing the WebSocket to the orchestrator.

**Consequences.** Final form of the shell side confirmed: Tauri window with integrated terminal (xterm.js ↔ ConPTY ↔ `lare-shell.exe` ↔ WS ↔ orchestrator), alongside pre-existing Markdown/config/library/plugin windows. `lare-shell.exe` also remains usable standalone within a Windows Terminal profile (without bars, or with transient bars) for users preferring that mode — one engine, two renderers.

## ADR-017 — Configuration: one directory, no environment variables (2026-09-04)

Decision: `--config-dir` or `<exe>\Configuration\`; paths relative to deploy root; children receive `--config-dir` as argument. Rationale: in v1, three independent readers of the same environment variable drifted quietly out of sync (`llms_config.rs:119-125` v1).

## ADR-018 — Shell channel: connection registry, surface router, output upon `Done` (2026-09-05)

**Context.** With the C# host (ADR-015), the orchestrator receives commands from shell sessions lacking a rendering surface: output goes to `ui.exe` windows (D14), AI-proposed commands run in the client's shell (model B), and `ui.exe` is single-instance per machine.

**Decision.** (1) Each connection declares a `role` (`ui`|`shell`, default `ui`); an in-process registry (`connections.rs`) tracks the `ui` sink (channel-less `ui` connection, last-one-wins) and shell sessions. (2) For a turn originating from a shell, `ServerMsg::surface()` determines the destination of each message (`Origin` = the shell, `Ui` = the sink); `Chunk`s are buffered and delivered once upon `Done`/`Error` (`OutputWindowContent`), while the shell receives a single confirmation line. (3) `run_in_session` on the shell channel is an `ExecInShell`→`ExecResult` round-trip over the same connection, ALWAYS gated (`ShellConfirmer`); cwd is tracked per session. (4) Unknown slash command from shell → silent `Done` + log; `/ai "x"` ≡ `/ "x"`.

**Consequences.** `ui`/Telegram connections remain unchanged (zero v1 regressions). Without `ui.exe` connected, a shell turn still completes (warning line replaces confirmation); autostart belongs to Plan 3. In-window streaming remains outside MVP (§12). `/find` and `/nowin` from the shell are dropped in this version — declared debt in [`03-status-and-implementation.md`](./03-status-and-implementation.md).

## ADR-019 — Host `lare-shell`: file-based policy, pwsh profiles, capture via host UI, single thread for runspace and console (2026-09-06)

**Context.** The spike forced execution policy in-process, did not load pwsh profiles, did not communicate via WS, and did not capture output. The production product must behave "like pwsh" (§4.4) and speak the shell channel protocol of Plan 2a while satisfying contracts (a)–(d).

**Decision.** (1) LocalMachine execution policy originates from a `powershell.config.json` placed alongside the executable (`$PSHOME` for a host = its directory) — **however, this only applies with an explicit RID**: without `<RuntimeIdentifier>`, RID-specific SDK assets (including `System.Management.Automation.dll`) end up in `runtimes\win\lib\net10.0\`, and `$PSHOME` becomes THAT directory rather than the executable directory — meaning the file next to the executable would never be read (discovered in Task 4: 7 out of 11 tests failed until added). Both `.csproj` files therefore declare `<RuntimeIdentifier>win-x64</RuntimeIdentifier>` + `<SelfContained>false</SelfContained>`, matching deploy publish settings (`-r win-x64`, framework-dependent): `Set-ExecutionPolicy` alters policy just like in pwsh. (2) PSReadLine is sourced from pwsh modules, prepended to the process `PSModulePath` (pwsh 7.6+ required). (3) CurrentUser profiles loaded in order `profile.ps1`, `Microsoft.PowerShell_profile.ps1`, `LareShell_profile.ps1`; `$PROFILE` matches pwsh; AllUsers profiles not loaded. (4) Output capture (`capture:true`) by recording `Write*` calls on `PSHostUserInterface`, with `ForEach-Object { $_ }` between script and `Out-Default` so native processes pass through the pipe; `capture:false` = pure pipeline. (5) A single thread (REPL) owns runspace and console; socket enqueues into a `Channel`; no `async` within REPL. (6) On-demand reconnection with autostart (5 s); child processes run with `UseShellExecute=true` (no inherited console) and **hidden windows for BOTH** children, `orchestrator.exe` and `ui.exe` — not just the first as originally planned: discovered during e2e testing that `ui.exe` in debug builds is a console app that, with Windows Terminal as default terminal, spawned a WT tab and stole focus from the shell (fix `a8d148f`). (7) `exit_code` = `$?` captured not via direct string appending, but by wrapping the AI command in `try { <cmd> } finally { $global:__lare_ok = $? }`: direct append fails if the command concludes with a top-level `return` (`return` terminates script execution before reaching the appended line); a `. { }`/`& { }` block around the command loses it differently (`$?` always resets to true at the boundary of an invoked/dot-sourced block regardless of internal outcome); only `try/finally` captures true `$?` without introducing a new invocation scope (variables and functions declared by the command persist in the session, as if typed at the prompt) — verified empirically in Task 4. `$LASTEXITCODE` is cleared prior to command execution.

**Consequences.** The host feels like a "real" pwsh to the user (PSReadLine, profile, prompt) plus `/…` commands; tests run against a mock WS server using `TcpListener` (never `HttpListener`); declared debts in [`03-status-and-implementation.md`](./03-status-and-implementation.md).

## ADR-020 — Terminal window: `--no-terminal` as explicit role for `ui.exe` (2026-09-07)

**Context.** Plan 3 introduces a terminal window that `ui.exe` opens by default on launch. However, `ui.exe` is also launched by callers already owning a shell (C# host self-healing in mode B, orchestrator autostart to open an output window) — for them, a second terminal window would be a conspicuous bug rather than a feature.

**Decision.** An explicit flag, `--no-terminal`, distinguishes the two roles rather than a heuristic (e.g. "is a lare-shell.exe already running?", which is fragile and prone to race conditions). Callers launching `ui.exe` solely for the host window role always pass it; its absence indicates mode A.

**Consequences.** `Launcher.EnsureUi()` (C#, `lare-shell` 2.0.1) and orchestrator autostart (Task 6) both pass the flag. A user manually launching `ui.exe` without the flag always gets the terminal window — an intentional default behavior (mode A is the standard interactive user experience).
