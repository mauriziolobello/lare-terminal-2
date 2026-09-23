# Channels

This document describes the channels through which a command or AI request can enter Lare Terminal,
beyond the local shell already discussed in [`01-architecture.md`](./01-architecture.md) and
[`03-status-and-implementation.md`](./03-status-and-implementation.md). In the three-tier model
(channels → orchestrator → tool servers), every channel represents a different entry point speaking
the same protocol to the same orchestrator: only *where* the request originates changes, not the
core principles governing its handling — in particular, the explicit confirmation gate for every
command proposed by the AI is non-negotiable across all channels.

Today, beyond the shell, there exist:

- **AI Chat** — a dedicated chat window capable of interconnecting multiple machines running Lare
  Terminal on the same local network.
- **Telegram** — a bot allowing commands to be issued remotely.
- **External channels** — integrations with specific tools (network diagnostics, financial market
  analysis), each featuring a fixed, predefined set of capabilities.

## The local shell, in brief

The local shell is the default channel and is fully detailed in
[`01-architecture.md`](./01-architecture.md) (the protocol, the confirmation gate, how responses are
generated) and [`03-status-and-implementation.md`](./03-status-and-implementation.md) (shell host,
terminal window, output windows). Briefly recalled here: it runs exclusively on `127.0.0.1`,
authenticated with a local token, making it the only channel the AI can interact with in relative
autonomy — behind the explicit confirmation gate, but without a second authentication factor,
because the channel itself cannot be reached from outside the machine.

## AI Chat

`/aichat` opens a dedicated window (`aichat-window`), independent of the terminal — one of the
windows localized across multiple languages. It is a **machine singleton**: if already open,
`/aichat` does not spawn a second instance, but brings focus to the existing one (matching the
pattern of `/config` and `/library`). An envelope icon signals unread activity while the window is
closed. Window opening is always explicit — never auto-opened on application launch.

### Not merely a chat with local AI: a cross-machine room

Contrary to what the name might suggest, AI Chat is not (merely) a chat interface with the local
machine's AI: it is a **network channel bridging multiple Lare Terminal installations on the same
local area network**. The mechanism:

- **Peer discovery via UDP broadcast.** Every machine with the service enabled periodically announces
  its presence via broadcast on the subnet; peers failing to announce within a threshold are marked
  as departed.
- **Deterministic and "sticky" hub election.** Among discovered peers, the lowest IP address wins —
  yet an active hub retains its status as long as it remains present, without preemption by a lower
  IP appearing later (hub migration in a star topology incurs network cost, so churn is minimized).
- **Star relay.** The elected hub relays messages to all connected peers.
- **The network transports only text and presence, never the AI model itself.** Each machine uses
  its own local AI (own provider, own API key): there is no "central" room AI.

The service is **disabled by default** and configured via the "AI Chat" tab of `/config` (machine
nickname, whether the local AI participates when addressed, whether it participates on its own
initiative).

### Who speaks: humans and AIs, by invitation

Participants are humans — one per connected machine, each typing in their own window — and,
optionally, the AI of each machine. A remote AI (belonging to another machine) joins active
conversation only following an **admission/consent handshake**: whoever is already present must
accept the entry of a new AI participant; it is not automatic. Explicit invocation uses an @-syntax:
`@<label>-ai` for a specific machine, `@all` for all machines; addressing the local machine's
AI always engages directly — the human is typing in their own window, making consent implicit.

Beyond answering when invoked, the AI can **auto-participate**: judging independently, message by
message, whether it has something genuinely useful to contribute, and intervening on its own
initiative if so; if it has nothing to contribute, it responds with an internal token (never
rendered) and remains silent. A counter caps consecutive AI turns because auto-participation — unlike
explicit invocation — is not structurally immune to runaway feedback loops between AIs on different
machines.

Each participating AI can also maintain a small **persistent per-machine memory**, which it writes or
updates on its own initiative (never via automated extraction from human messages) via a marker in
its response, retrieving it across subsequent sessions. A document saved in the Library can be shared
into the chat with a dedicated button, sent to one or more present machines.

### Independent system prompt, and what it CANNOT do

AI Chat uses its own system prompt (two variants — one for responding to explicit invocation, one for
auto-participation), completely independent of the general prompt used by `/ai` turns in the
terminal. The prompt explicitly informs the AI of its identity (name, provider, machine), notes that
it is one of several participants (human and AI, potentially with similar names — strictly avoiding
impersonation), and critically: **it cannot execute commands or open windows — it responds solely in
plain text.** There is no confirmation gate to navigate in AI Chat simply because there are no tools
to propose: the surface area is strictly smaller than that of a terminal `/ai` turn.

### What distinguishes AI Chat from an `/ai` turn

A terminal `/ai` turn is a single discrete request within a shell session: the AI can propose commands
(behind confirmation) running in that session, with that `cwd`, and the final answer replaces a
placeholder inside a linked output window. AI Chat, by contrast, is a **persistent, shared
conversation**, potentially across multiple machines, decoupled from any single shell command: it
executes nothing, opens no result windows, and its sole product is text in a room where others (human
or AI) can respond at any time.

## Telegram

The Telegram channel allows issuing commands remotely via a dedicated bot. It is the only channel
receiving **untrusted remote input**: anyone knowing the bot handle could message it, whereas the
local channel is already shielded by running exclusively on `127.0.0.1`. The channel's entire
security architecture derives from this distinction (ADR-007).

### Activation and setup

The channel is optional and enabled by creating `<config-dir>/telegramsettings.json` containing a bot
token obtained via `@BotFather`; if the file is missing, the channel simply does not start. On the
very first launch (no TOTP secret yet provisioned), the orchestrator prints an ANSI QR code to
stderr **only once**, readable with Google Authenticator, along with the raw `otpauth://` URI as a
textual alternative — neither the QR nor the URI is ever logged or displayed again. TOTP state
(secret + paired chat ID) persists locally.

### The second factor: pairing and login

Before even reaching the command confirmation gate, the channel mandates two independent stages:

1. **One-time pairing** — `/pair <code>` (code shown only on first launch, valid for 10 minutes) binds
   a single Telegram chat to the bot. Once complete, it never needs repeating: on subsequent launches
   the pairing code is not printed, and input from any other chat is ignored.
2. **TOTP login** — `/login <6-digit-code>` establishes a 30-minute session held only in memory
   (never persisted): it must be re-entered after every orchestrator restart.

Five consecutive failed attempts (in either pairing or login) trigger a one-minute lockout.

### Practical use and confirmation gate

Once authenticated, the channel accepts both direct commands and natural language requests to the
AI, as well as a few dedicated slash commands (`/open`, `/reset`). Here the confirmation gate is
**stricter than locally**: every OS command, every `/open`/`/web`, and every single tool proposed by
the AI during a turn (except writing to a Markdown window, which does not exist on this channel
anyway) requires inline `[Esegui]`/`[Annulla]` buttons on Telegram (Italian button labels —
hardcoded, not localized), governed by a two-minute timeout
— whereas locally, the AI normally executes autonomously behind the terminal prompt alone. Commands
issued from Telegram do not run in an isolated remote sandbox: they traverse the same shared tool
process (single-instance across the workspace) used by local connections — sharing the same
workspace, not a segregated environment per remote client.

There is currently no cancel button for commands already in progress on this channel, nor a toggle
for web search (permanently disabled on Telegram); responses, lacking a dedicated window, are split
across multiple messages whenever they exceed Telegram's maximum message length. The channel lacks an
independent system prompt: it shares the generic prompt of a terminal `/ai` turn (minus window
capabilities), defaulting to Italian regardless of the language configured for the local UI — see
[`07-i18n.md`](./07-i18n.md) for details on how language propagates (or does not propagate) per
channel.

## External channels

External channels are integrations with specific tools. The architectural principle governing them
is universal and intentionally rigid: **never an arbitrary shell.** Every external channel exposes a
**fixed, predefined tool registry** to the AI — never generic command execution — and adding a new
capability requires writing an explicit new tool, rather than broadening an existing tool to "handle
that as well."

Technically, an external channel is a connection **isolated along three axes**, all derived from the
same object (`ToolClient`) provided by the channel:

- **Definitions** — the channel's AI sees *only* that channel's tools, never the generic tools
  (`run_in_session`, `open_target`, `show_markdown`) used by the rest of the system.
- **Dispatch** — a tool name outside the channel's declared menu (even if the model calls it by
  mistake, e.g. confusing it with a generic tool) is rejected as unknown rather than executed.
- **Concurrency** — a pending confirmation on an external channel does not block the terminal, and
  vice versa: queues remain entirely independent.

Each external channel also maintains an independent system prompt detailing its available tools —
ensuring the model does not attempt to invoke tools it simply does not possess.

The technical registry contains five entries today; only two are user-facing product capabilities
with their own dedicated slash triggers and distinct product scope — the other three represent
infrastructure or feature support, rather than standalone "channels":

| Slash | Registry ID | What it is |
|---|---|---|
| `/netsec` | `netsec` | Network diagnostics — product capability |
| `/markets` | `financial-markets` | Financial market analysis — product capability |
| `/pyping` | `python-ping` | Test channel, validates Python tooling infrastructure — not a capability |
| (no slash) | `library-expand` | Used by the "Expand" button on a document already open in Library |
| (no slash) | `config-market-data-test` | Used by the "Test Connection" button in the Market Data tab of `/config` |

### `/netsec` — network diagnostics

The channel (renamed from `/nmap` to `/netsec` in a recent update — internal technical identifiers,
the `mcp-nmap` crate, the `mcp-nmap.exe` binary, and individual `nmap_*` tools remain unchanged,
affecting only user- and AI-facing naming) is backed by a dedicated Rust crate wrapping the actual
`nmap` binary alongside native operating system diagnostic commands. It exposes **eight fixed
tools**:

- **Five nmap scans** — `nmap_quick_scan` (rapid TCP connect, unprivileged), `nmap_os_detect` (OS
  detection, requiring elevated privileges via Windows UAC and explicit consent), `nmap_version_scan`
  (`-sV`, listening service versions), `nmap_host_discovery` (`-sn`, live network host discovery, no
  port scan), `nmap_vuln_scan` (known vulnerability checks using NSE scripts exclusively from the
  built-in `vuln` category — never arbitrary or custom NSE scripts, by deliberate design, preventing
  the AI from suggesting arbitrary NSE execution to users).
- **Two built-in local diagnostic tools** — `local_network_info` (local machine IP, subnet, gateway,
  ARP, routing table — used by the AI to resolve targets when unspecified by the user) and
  `traceroute` (network path tracing to a host).
- **One home router status tool** — `fritzbox_status`, which reads (via the `fritzconnection` Python
  library, executed as a one-shot Python script rather than a persistent server) the recent event
  log, current public IP, and active LAN devices of a configured FRITZ!Box router. Requires a
  dedicated configuration file (`Configuration/fritzbox.json`, never committed) containing router
  host/username/password — details on the shared Python infrastructure powering this tool are in
  [`05-pytools.md`](./05-pytools.md).

The channel's system prompt includes an explicit note of technical honesty regarding this last tool:
a home router's event log **is not an intrusion detection system**. It typically records failed
logins and VPN connection attempts, not firewall-dropped traffic destined for closed ports (which a
FRITZ!Box normally does not log at all) — an empty event log should never be interpreted as "zero
external probe attempts."

The five scanning tools remain subject to explicit confirmation even when run locally — a declared
exception to the rule that the local UI normally operates autonomously over its tools, as scanning produces
network-visible effects outside the machine itself. The confirmation banner displays target and scan
type prior to execution; for `nmap_os_detect`, which requires elevation, this is also the only point
where the user can review the target, as the Windows UAC dialog does not display it. The three
diagnostic tools (`local_network_info`, `traceroute`, `fritzbox_status`) are exempt from this extra
confirmation.

Expanding this channel toward broader tooling (deeper network scanning, TLS validation, packet
capture) represents a discussed idea, but has not yet been designed.

### `/markets` — financial market analysis

A tool channel backed by a persistent Python MCP server (unlike the one-shot script in
`fritzbox_status`: it remains alive between calls, configured with a 300-second per-call timeout —
necessary because certain screeners enrich candidates with dozens of supplemental fundamental queries).
The Python sidecar starts upon first channel invocation, like any other external channel.

It exposes five tools: `search_ticker` (resolves corporate names to matching US tickers),
`stock_report` (comprehensive factual report — fundamentals, charts across five time horizons, option
chains, peer comparisons), `list_stocks` (tabular list of known tickers, currently filterable only by
"USA"), `list_screeners` (enumerates available screeners and opens a selection dialog), and
`run_screener` (executes a specific screener). The market data provider (`yfinance` currently) is
configured via the "Market Data" tab of `/config`, which also provides a dedicated connection test.

For `stock_report` and `run_screener`, the Markdown window containing the complete document opens
**deterministically** — triggered by system logic rather than the AI — and is populated at turn
completion by appending the AI-authored synthesis: for `stock_report`, a "Trend Narrative" and
"Investment Hypothesis" (with separate short- and medium-term scores); for `run_screener`,
an evaluative summary specific to the completed screen. In both cases, the channel prompt strictly
concludes with an explicit disclaimer: output represents factor analysis, not investment advice.
Currently available screeners implement distinct strategies (consumer adoption, equity-research-style
large-cap analysis, AI infrastructure, dual-regime quantitative technicals).

## Further reading

- [`00-opening.md`](./00-opening.md) — what the project is and why it exists.
- [`01-architecture.md`](./01-architecture.md) — the three-tier model (channels → orchestrator → tool
  servers) and confirmation gate instantiated by each channel described here.
- [`03-status-and-implementation.md`](./03-status-and-implementation.md) — what part of these
  channels is actually implemented and in active use today.
- [`05-pytools.md`](./05-pytools.md) — Python scripts shared by certain external channel tools
  (`fritzbox_status`, the `financial-markets` domain).

## Declared limitations

- **Telegram**: no cancellation button for running commands; no web search capability; login session
  persists in-memory only, requiring re-authentication after orchestrator restarts; answers
  permanently in Italian regardless of local interface language configuration.
- **AI Chat**: AI participants respond solely in plain text, never issuing commands or opening
  windows; the channel's two system prompts are fixed in Italian, unaffected by interface language
  settings; only a single AI provider is active across the application today, meaning each machine
  participates with a single AI "voice."
- **`/markets`**: US stocks only today (country filtering exists in the interface, but "USA" is the
  sole supported value).
- **`/netsec`**: evolution toward broader network analysis capabilities is discussed but not yet
  designed; `fritzbox_status` is specific to FRITZ!Box routers (TR-064 protocol).
