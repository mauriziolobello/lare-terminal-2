# Status and Implementation

If [`01-architecture.md`](./01-architecture.md) explains how the system is designed and why, this
document snapshots **what actually exists today**: component by component, what is complete, and
what is openly declared as technical debt. This is the document to update with every release — it
reflects the reality of the code, not the original intent.

## Components and versions

| Component | What it does |
|---|---|
| `protocol` | Contract for WebSocket messages shared across all components |
| `orchestrator` | Central hub: connection registry, command router, AI loop, confirmation gate |
| `lare-shell` (.NET/C#) | Custom host for the PowerShell engine — not a Cargo crate |
| `ui` (Tauri) | Desktop application: terminal window + all secondary windows |
| `mcp-server` | Generic MCP tool server (persistent shell, app launching, file search) |
| `mcp-nmap` | Tool server for the `/netsec` channel (network diagnostics) |
| `startup-config` | Shared resolution for `--config-dir`/`startup.json` |
| `plugin-protocol` + `plugin-ping`/`plugin-calc`/`plugin-counter`/`plugin-lc`/`plugin-crypto` | Sidecar plugin system — details in [`04-plugin-system.md`](./04-plugin-system.md) |

Each component maintains its own `CHANGELOG.md` (semver) and `IMPLEMENTATION.md` (current technical
details) alongside its code — the most up-to-date source for those looking deeper than this document.

## Two execution modes

- **Mode A (default)** — the user launches the application directly; a terminal window opens
  (xterm.js on top of a native pseudo-terminal) with the shell host running as a child process.
- **Mode B** — a dedicated Windows Terminal profile launches the shell host standalone, without its
  own graphical window; supporting windows (output, configuration, etc.) are spawned separately
  when needed.

Both modes share a mutual self-healing mechanism: if the orchestrator is unreachable, whichever
component needs it starts it up and retries for a few seconds; if a window ready to receive output
is missing, the orchestrator spawns it in turn.

## Shell host (C#)

A dedicated .NET component with its own test suite (xUnit). It owns the PowerShell runspace, loads
PSReadLine from actual pwsh modules, respects user profiles (`$PROFILE`) in the exact order of a real
pwsh, handles output capture/exit code/persistent cwd, the `[Y/n]` confirmation gate, and
on-demand reconnection to the orchestrator (no background reconnection tasks: each command checks
and, if necessary, re-establishes the connection).

**Known limitations, openly declared**: `AllUsers` profiles are not loaded; host logging lacks log
rotation; the `$?` status variable in the prompt reflects only the last manually typed command, never
the outcome of an AI turn that just concluded; an `exit` executed by the AI within a command
terminates the process just as an `exit` typed by the user would. The module handling interactive
input lacks an automated test suite (requires a real console) — it is covered only by manual
end-to-end verification.

## Terminal window

A real terminal (xterm.js on top of a native pseudo-terminal, not a widget simulating one), with
activity indicators ("AI working") driven by a direct communication channel between host and
emulator, independent of the orchestrator protocol. A dedicated flag allows launching the
application without opening this window, when it is needed only to host secondary windows.

## Output windows and AI responses (`show_markdown`)

The tool through which the AI generates formatted responses — extensive explanations, tables, code —
within a dedicated window. Relevant implementation highlights:

- **One window per turn, updated in place**: repeated invocations of the same tool within the same
  turn (typical when the model refines an answer with multiple searches) update the same window
  rather than opening new ones.
- **Close confirmation gate**: attempting to close a window while the turn driving it is still
  active triggers a confirmation overlay; confirming genuinely cancels the underlying turn rather
  than letting it continue in the background.
- **Progress badges** ("searching…" / "completed") and timestamp of the latest update.
- **Content sanitization**: raw citation markers that the model occasionally outputs in free text
  (rather than as structured fields) are stripped prior to rendering.
- **Saving to the library**: the "Save" button remains permanently active — the first click creates
  the document, subsequent clicks overwrite the same file instead of duplicating it; a transient
  flash confirms the action without leaving a persistent state that might look like "work finished"
  while the window continues to update.
- **Quick copy**: two actions copy window contents to the clipboard, either as plain text or as
  source Markdown.
- **Not included**: token-by-token streaming — text is always delivered in full upon turn
  completion, never incrementally.

## AI Chat and Telegram

Two additional channels beyond the local shell. **AI Chat** is a dedicated window, independent of the
terminal, with its own system prompt. **Telegram** allows issuing commands remotely via a bot; as a
channel receiving untrusted remote input, it requires a second factor of authentication (TOTP) before
even reaching the command confirmation gate. Details in [`06-channels.md`](./06-channels.md).

## Plugin system

Separate processes (sidecars) with their own protocol, each with its own Tauri window: a calculator
with programmer mode (numeric bases, bitwise operations), an encryption plugin, and simpler plugins
used also as references for building new ones. Details and guide in
[`04-plugin-system.md`](./04-plugin-system.md).

## Web search in AI turns

A turn can use web search, enabled by default or on a per-request basis depending on configuration.
This mechanism powers the richer responses displayed by `show_markdown`, including the progressive
refinement described above.

## Library

Documents generated by the AI and saved remain browsable in a dedicated archive, with its own window
for browsing.

## Interface languages

Italian, English, Spanish — for both the user interface and AI conversational responses. Technical
details in [`07-i18n.md`](./07-i18n.md).

## External channels

Integrations with specific tools, each featuring a fixed, predefined set of capabilities (never
generic system access): network diagnostics (including reading the state of one's home router) and a
channel for financial market analysis. Details in [`06-channels.md`](./06-channels.md).

## Python tools (pytools)

Certain external channels rely on independent Python scripts, one per domain, each with its own
virtual environment. Details in [`05-pytools.md`](./05-pytools.md).

## Security and confirmation gate

No command proposed by the AI is executed without explicit confirmation — this holds for the local
channel as for any other channel. Only one AI turn at a time per connection: a second request is
queued rather than overlapping. End-to-end tests of the gate using a real AI model (not a stub) exist
but remain excluded from routine automated test runs because they require a valid API key.

## Configuration

No project-specific environment variables: each process receives its configuration directory via an
explicit argument. A registry of available AI providers enables selecting which one to use;
currently only one provider is active at a time for the entire application — there is not yet a way
to route an individual request to a provider different from the active one.

## Build and deployment

The deployment layout lives inside the repository itself (not in a separate package), allowing it to
be verified and copied elsewhere without modifications. Operational details in
[`BUILD.md`](./BUILD.md), [`DEPLOY.md`](./DEPLOY.md), [`RUN.md`](./RUN.md).

## Verification

Several thousand automated tests cover the Rust workspace, frontend JavaScript library, and C# host
(xUnit) — all passing as of the latest verification. A manual end-to-end verification checklist
covers scenarios requiring real graphical interface interaction; most major scenarios are
confirmed, with a few minor items not yet re-verified live (details in
[`TESTING-e2e.md`](./TESTING-e2e.md)).

## Known technical debt and declared limitations

Technical honesty: this is what should **not** be expected to work today, or what represents an
intentional boundary rather than an overlooked bug.

- The frontend output/markdown window lacks DOM-level test coverage — more than one bug in this area
  was discovered only through manual verification.
- There is not yet a way to route an individual AI request to a provider different from the active
  one, nor provider groups, nor multi-AI conversations.
- Token-by-token streaming in the output window is intentionally out of scope, not an oversight.
- A possible orchestrator restart scenario can launch a duplicate application window rather than
  reusing the existing one — a known bug, not yet fixed, only documented.
- The shell session working directory is not currently included in the context the AI receives at the
  start of a turn.
- Evolution of network diagnostic channels toward broader tooling (deeper network scanning, TLS
  inspection, packet capture) is a discussed idea but has not been designed.
- A mechanism allowing the AI to interact with external applications via simulated keyboard input
  exists only as a test script, not as a product feature.

For the full, up-to-date picture, including minor technical debt not listed here, each component's
`IMPLEMENTATION.md` remains the most detailed source.
