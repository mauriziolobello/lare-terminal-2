# Lare Terminal 2.0

**English** | [Italiano](./README.it.md)

A PowerShell terminal with an agentic AI built into the command line — not a chatbot on the side,
not an overlay: a real terminal (real prompt, history, profiles, a `cd` that persists) where lines
starting with `/` are special commands, and `/ai "..."` makes the AI act **in the same shell
session**, with the same `cwd`, behind an explicit confirmation gate on every command it proposes
to run.

It continues the story of [Lare Terminal](https://github.com/mauriziolobello/lare-terminal), its
first incarnation (a transparent overlay summoned by hotkey) — that repository stays public as the
complete historical reference.

## What it does

- **A real PowerShell shell** (custom host of the PowerShell engine, PSReadLine, profiles) in its
  own terminal window — or as a Windows Terminal profile.
- **`/ai "request"`** — the AI proposes and runs commands in your session, each behind a `[Y/n]`
  confirmation; the final answer opens in a Markdown window you can save.
- **`/…` commands** — `/help`, `/config`, `/library` (document archive), `/find` (live file
  search), `/open`, `/web`, `/show`, and more.
- **External channels with fixed tools** — `/netsec` (network diagnostics on top of `nmap`),
  `/markets` (financial market analysis, Python tools).
- **AI Chat** — a room shared by several Lare machines on the same LAN, with the AIs taking part.
- **Telegram** — remote commands, with pairing and a TOTP second factor.
- **Plugins** — sidecar executables with their own window (`/calc`, `/ping`, …).
- **Nine languages** for the interface and AI answers: Italian, English, Spanish, German, French,
  Dutch, Danish, Russian, Polish.

## Requirements

**Windows 10/11 x64** only.

To build:

- [Rust](https://rustup.rs/) stable, MSVC toolchain (with the Visual Studio Build Tools)
- [.NET 10 SDK](https://dotnet.microsoft.com/)
- Node.js — only for the frontend JS tests and the `shell-client.mjs` dev script

To run:

- **WebView2 Runtime** (already present on up-to-date Windows 10/11)
- **PowerShell 7.6+** (`pwsh`) installed — the shell loads PSReadLine from its modules
- **.NET 10 Runtime** x64
- An **Anthropic API key** (or one for another supported provider) — without it, the AI answers
  with a test stub
- Optional: `nmap` for `/netsec`; Python 3 for `/markets` and the FRITZ!Box router status

## Quick start

```powershell
git clone https://github.com/mauriziolobello/lare-terminal-2.git
cd lare-terminal-2

.\build.ps1 -IncludePlugins            # builds Rust + the C# host (slow the first time)
.\deploy_test_run.ps1 -IncludePlugins  # populates the runnable folder "Test Run\"
```

Then the AI key — either of the two:

```powershell
# a) environment variable (applies to terminals opened AFTER setx)
setx ANTHROPIC_API_KEY "sk-ant-..."

# b) or the configuration file (also allows other providers)
copy "Test Run\Configuration\llms.example.json" "Test Run\Configuration\llms.json"
#    and replace the <your-...-api-key> placeholders with your keys
```

And launch it:

```powershell
cd "Test Run"
.\ui.exe
```

One command: the terminal window opens and the orchestrator starts on its own in the background.
Try `/help`, then `/ai "list the 3 largest files here"`. The interface language is chosen in
`/config` (Italian by default).

## Configuration

All configuration lives in `Test Run\Configuration\` (or in the folder passed with
`--config-dir`). Files holding secrets or personal data are **not in the repository**: for each
one there is a **`*.example.json` template** to copy, dropping `.example` from the name and
replacing the `<your-...>` placeholders.

| Template | Used for | Needed? |
|---|---|---|
| `llms.example.json` | API keys and choice of AI provider/model | only if you don't use `ANTHROPIC_API_KEY` or want another provider |
| `telegramsettings.example.json` | Telegram bot token | only to enable Telegram |
| `network.example.json` | AI Chat on the LAN (name, port, AI participation) | no — generated on first run, disabled |
| `fritzbox.example.json` | FRITZ!Box router credentials for `/netsec` | only for `fritzbox_status` |
| `search-paths.example.json` | folders indexed by `/find` | no — generated on first run |
| `search-content.example.json` | extensions treated as text/binary by `/find` | no — generated on first run |
| `market_data.example.json` | data source for `/markets` | no — set from `/config` |
| `config.example.json` | UI preferences (language, transparency, web search) | no — set from `/config` |

Details on every file (who creates it, when, what is secret):
[`Test Run/Configuration/README.md`](./Test%20Run/Configuration/README.md) (Italian).

## What to read, in order

1. [`00-opening.md`](./Docs/i18n/eng/00-opening.md) — what it is, why it exists, map of the docs.
2. [`BUILD.md`](./Docs/i18n/eng/BUILD.md) → [`DEPLOY.md`](./Docs/i18n/eng/DEPLOY.md) →
   [`RUN.md`](./Docs/i18n/eng/RUN.md) — build, prepare the runnable folder, launch (with the
   Windows gotchas).
3. [`KNOWN-ISSUES.md`](./Docs/i18n/eng/KNOWN-ISSUES.md) — known limits before they surprise you.
4. To understand how it's built: [`01-architecture.md`](./Docs/i18n/eng/01-architecture.md),
   [`02-decisions.md`](./Docs/i18n/eng/02-decisions.md) (decision log),
   [`03-status-and-implementation.md`](./Docs/i18n/eng/03-status-and-implementation.md), then the
   per-subsystem documents (plugins, Python tools, channels, languages).

The Italian documentation in [`Docs/i18n/ita/`](./Docs/i18n/ita/) is the reference version; the
English one is kept in step with it.

## Stack

Rust (orchestrator, protocol, tool servers, Tauri interface), C# (custom host of the PowerShell
engine), vanilla JavaScript (window frontends), Python (domain tools over MCP). Details and the
reasons behind the choices in [`01-architecture.md`](./Docs/i18n/eng/01-architecture.md).

## License

[MIT](./LICENSE).
