# Lare Terminal 2.0

## What it is

Lare Terminal is a PowerShell terminal with agentic AI integrated directly into the command line.

It is not a chatbot in a window alongside the terminal, and it is not an overlay that pops up over
the screen only to disappear: it is a real terminal — a real PowerShell engine, prompt included,
command history, profiles, modules, `cd` that persists as usual — within which certain lines have a
special meaning. A line starting with `/` is a **power command**: it can be a direct instruction to
the program (`/config`, `/library`, `/help`, `/calc`), or a natural language request addressed to an
AI (`/ai "find the largest files in this directory and explain why they take up so much space"`),
which then acts **in the same shell session**: the commands it decides to execute run in your
runspace, with your `cwd`, your modules, your history — not in a separate sandbox that merely
reports back a result.

Every other line — those not starting with `/` — is ignored by the Lare layer and passed straight
through to the underlying PowerShell engine, exactly as if Lare were not there.

## Why it exists

The project is in its second incarnation. The first (Lare Terminal, whose full history remains in
the preceding repository — see below) was an **overlay**: a transparent window summoned via a
hotkey, overlaid on the screen, a cursor through which to issue commands and converse with an AI. It
worked, but remained a *guest* of the system — not the shell itself. Whoever wanted to actually use
PowerShell had to leave the overlay and enter a real terminal; the AI and the real shell lived in
two separate worlds.

Lare Terminal 2.0 was born from a simple question: **what if Lare were the shell, instead of merely
looking into it?** Not a hook intercepting another shell, not a client talking to it from outside — a
**custom host for the PowerShell engine**, in the same way `pwsh.exe` is a host for that engine. The
rest of the architecture (the orchestrator communicating with the AI, the MCP server with the tools,
channels like Telegram) retains almost everything built in the first version; what changes is the
entry point, and what is coupled to it.

## Who it is for

For those who spend their day in a PowerShell terminal and want an AI that is not a separate
application from which to copy and paste commands, but a participant in the same session — seeing
the same `cwd`, capable of executing real commands (behind explicit confirmation, never silently),
and able to respond in dedicated windows when the answer is richer than a single line of text (a
table, an extensive explanation, code).

It is also, avowedly, a project built in **direct collaboration between a person and development
AIs** (Anthropic's Claude supervising, plus external AIs implementing more scoped tasks) — not only
in the final product, but in the *way* it was built: genuine TDD, architectural decisions reasoned
through rather than merely stated, a log of every important choice (`02-decisions.md`). Anyone
curious about how software is built with an AI as a regular collaborator, rather than just code
autocompletion, will find a concrete, documented case study here.

## Relationship with the previous version

This repository (`lare-terminal-2`) continues the story of
[Lare Terminal](https://github.com/mauriziolobello/lare-terminal), the previous project by the same
owner. The first version remains fully public as historical reference: previous architecture,
original decisions (ADR-001..014, also referenced here), the path that led here. It is not
"obsolete material to ignore" — it is the first half of the same story, showing the reasoning
through which the project evolved.

## Project status

Under active development. The three-tier architecture (channels → orchestrator → tool server) is
stable; the custom PowerShell shell, the terminal window, the AI loop with confirmation gate, the
plugin system, and the channels (Telegram, AI Chat, external integrations) are implemented and in
daily use. A detailed overview — what is present today, what is missing, and known technical debt —
can be found in [`03-status-and-implementation.md`](./03-status-and-implementation.md).

## Navigating this documentation

- [`01-architecture.md`](./01-architecture.md) — the three tiers, key decisions, and why they were
  made that way.
- [`02-decisions.md`](./02-decisions.md) — the architectural decision log (ADRs), starting from the
  first version.
- [`03-status-and-implementation.md`](./03-status-and-implementation.md) — what exists today, area
  by area.
- [`04-plugin-system.md`](./04-plugin-system.md) — how plugins work and how to extend the
  program with a new one.
- [`05-pytools.md`](./05-pytools.md) — Python tools invokable by the AI (financial analysis,
  networking, etc.).
- [`06-channels.md`](./06-channels.md) — Telegram, AI Chat, external integration channels.
- [`07-i18n.md`](./07-i18n.md) — how interface multilingual support works.
- [`BUILD.md`](./BUILD.md), [`DEPLOY.md`](./DEPLOY.md), [`RUN.md`](./RUN.md) — building, deploying,
  running.
- [`KNOWN-ISSUES.md`](./KNOWN-ISSUES.md) — known issues.
- [`TESTING-e2e.md`](./TESTING-e2e.md) — end-to-end verification checklist.
