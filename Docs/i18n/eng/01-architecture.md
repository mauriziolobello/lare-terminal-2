# Architecture

This document explains **how Lare Terminal is built** and **why** — its current form is the
culmination of a series of reasoned decisions, not a project designed all at once from the start. The
complete log, decision by decision, is in [`02-decisions.md`](./02-decisions.md); here the same
story is told as a single thread.

## The three tiers

The architecture rests on a principle established in the very first version of the project and never
questioned since: **MCP standardizes tools, not AI**. "An AI that can use any capability and can be
swapped out" encompasses two distinct requirements, and each needs its own mechanism — conflating
them leads to an ambiguous design. Hence three distinct tiers:

```
Channels  →  Orchestrator  →  Tool server (MCP)
```

- **Channels** are the entry points through which a command or request enters the system: the local
  PowerShell shell, a Telegram bot, a dedicated AI chat. Each speaks the same protocol to the
  orchestrator.
- **The orchestrator** is the "brain": it maintains the conversation loop with the AI, decides which
  command to execute (behind confirmation, never with silent autonomy), and routes output to the
  appropriate channel or window. The AI itself sits behind a **swappable adapter** — changing
  provider (or model) does not require modifying the rest of the system.
  Currently, the project defaults its AI models to **Anthropic's Claude**: not due to any technical
  limitation of the adapter (which remains generic), but because the project originated and
  developed as a direct collaboration with that family of models.
- **The tool server** exposes reusable capabilities — executing a command in a session, opening a
  native application, searching files, querying an external tool — through the Model Context
  Protocol, allowing diverse tools to be added without touching the orchestrator.

## The core change in version two: being the shell, not its guest

The first version of Lare Terminal was an overlay: a transparent window, summoned via hotkey,
overlaid on the screen like a cursor. It worked, but remained outside the real shell — whoever wanted
PowerShell "for real" had to leave it.

The question that opened the second version: **what if Lare were the shell, not its guest?** The
chosen answer is neither a hook on someone else's shell nor a lightweight client speaking to it from
outside, but a **custom host for the PowerShell engine**, written in C# — exactly the role that
`pwsh.exe` plays for that same engine. It owns its own REPL, reads input with PSReadLine (the same
library that gives pwsh advanced line editing and history), intercepts `/…` lines before they reach
the runspace, and executes everything else exactly as any pwsh would — same `cwd`, same profiles,
same modules.

This host lives inside a **Tauri window** with a real terminal emulator (xterm.js) hosting it via
ConPTY (Windows native pseudo-terminal) — not a graphical widget pretending to be a terminal, but a
real terminal housing an actual shell process. Status bars and badges live in HTML *around* the
terminal area, never overlaid on its content.

The first version's overlay did not survive this change: it was removed, not left dormant in the
codebase.

## The protocol: channels and roles

A channel connects to the orchestrator via WebSocket (on `127.0.0.1` only, authenticated with a local
token) and declares a **role**: `shell` for a terminal session, `ui` for the process owning secondary
windows (a single instance per machine). This distinction exists because a shell session lacks, by
itself, a place to render a table or a lengthy explanation — that task belongs to the process with
role `ui`, to which the orchestrator routes every "open/update window" message, while confirmation
text remains in the terminal from which the command originated.

## How a response is generated

When a line begins with `/ai "…"` (or the shorthand `/ "…"`), the host sends the command to the
orchestrator, which immediately opens an output window with a placeholder and initiates the
conversation turn with the AI. If the AI proposes executing a command, that command **never runs
autonomously**: the host displays a confirmation prompt in the terminal, and only explicit consent
executes it — within the same shell session, with the same `cwd` that the session had at that
moment. The result returns to the AI, which may continue the turn (more commands, more confirmation
requests) until reaching the final response, which replaces the placeholder in the output window.

This confirmation gate is the sole authorization point for any command proposed by the AI, and is
**non-negotiable**: it applies to the local channel just as it does to Telegram (where, being an
untrusted remote channel, a second factor of authentication is also required before even reaching the
gate).

## Windows as a surface for rich responses

Not every response is a line of text. A lengthy explanation, a table, formatted code: all of this goes
into a dedicated **Markdown window**, sanitized before rendering (no raw HTML ever passes from
generated content). Some windows are **unique per machine** — the archive of saved documents,
configuration, AI Chat independent of the terminal — because it makes sense for them to remain a single
reference point even with multiple terminal sessions open simultaneously; others, such as the output
of a single command, live and die with that command.

## Configuration: one single rule, always

Every binary in the project resolves its configuration directory in exactly one way: the explicit
`--config-dir` argument, or — if absent — the `Configuration\` directory next to its own
executable. **No project-specific environment variables** enter into this resolution: a child binary
always receives the path as an argument from whatever launched it. The reasoning is not abstract: in
the project's first version, three independent reading points for the same environment variable had
quietly drifted apart, each convinced it was reading the same value. A single resolution path
eliminates the entire category of bugs, not just a single instance.

## Technology choices, and why not the alternatives

- **Rust for the entire backend, Tauri for the UI** — not out of language ideology, but due to a
  concrete constraint: the project aims to run equivalently across multiple operating systems, and a
  single cross-platform codebase guarantees that parity by design, rather than maintaining it
  manually across separate native implementations per platform. Tauri selects a compiled native core
  with a minimal footprint, unlike alternatives based on an embedded full browser engine.
- **The shell host in C#**, not in Rust: it directly uses `Microsoft.PowerShell.SDK`, the very same
  package upon which `pwsh.exe` is built — replicating that behavior (profiles, execution policy,
  PSReadLine) starting from a low-level binding would have been an immense and fragile effort for
  zero benefit.
- **A voice engine independent of the webview** (planned, not yet implemented): native voice APIs
  across operating systems are distinct engines, and thus would not guarantee identical cross-platform
  behavior — following the same rationale behind the Rust/Tauri choice.

## Security, in brief

- The local channel listens exclusively on `127.0.0.1`, never on an external network interface,
  authenticated with a locally generated token.
- Every command proposed by the AI must pass through the explicit confirmation gate described
  above — never running silently.
- A remote channel (Telegram) requires a second authentication factor before even reaching the gate.
- Markdown content displayed in windows is always sanitized prior to rendering.

## Further reading

- [`02-decisions.md`](./02-decisions.md) — every decision summarized here, along with discarded
  options and the full rationale behind each.
- [`03-status-and-implementation.md`](./03-status-and-implementation.md) — what part of this
  architecture is actually implemented and in active use today, area by area.
