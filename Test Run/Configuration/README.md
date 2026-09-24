# Test Run/Configuration/

**English** | [Italiano](./README.it.md)

This is the configuration folder (`--config-dir`) used by **all** binaries when they run from
`Test Run\` (the orchestrator, `ui.exe`, and in turn their child processes — `mcp-server.exe`,
the plugins, the Python scripts). One single rule (2.0 spec, decision D6, see
`crates/startup-config/src/lib.rs`): every binary uses `--config-dir <path>` if passed, otherwise
`<executable folder>\Configuration\`. No `LARE_*` environment variable is ever read.

**Important — relative paths in `startup.json` (`paths.*`, `log.dir`) are resolved against the
DEPLOY ROOT, i.e. the folder that CONTAINS `Configuration\` (here: `Test Run\`), not against this
folder and not against the process cwd.** Example: `"mcp_server": "mcp-server.exe"` resolves to
`Test Run\mcp-server.exe`, not to `Test Run\Configuration\mcp-server.exe`.

## Committed files (templates, no secrets)

| File | What it is |
|---|---|
| `startup.json` | Configuration template — WS port, relative paths (shell, mcp-server, plugins, pytools, routines), AI model, autostart, logging. Matches the hard-coded defaults (`StartupConfig::default()`): a missing file would have the same effect; the template exists as the place to edit the port/model without having to remember the defaults. |
| `README.md`, `README.it.md` | This file (English) and its Italian version. |
| `routines/.gitkeep` | Keeps the `routines/` folder (target of `paths.routines_dir`) tracked before the user saves the first routine into it. |
| `i18n/<lang>.json` | Interface dictionaries, one language per file (`it`, `en`, `es`, `de`, `fr`, `nl`, `da`, `ru`, `pl`). `it.json` is the base and the fallback. |
| `help/<lang>.md` | Body of the `/help` window, one language per file (same nine languages). |
| `*.example.json` | Templates of the uncommitted configuration files — see below. |

## `*.example.json` templates — copy them to create your own configuration

Files holding secrets or personal data (below, "Files generated at runtime") are not in the
repository. For each one a user may want to write or tweak by hand there is a template with the
same structure: copy it dropping `.example` from the name and replace the `<your-...>`
placeholders with your own values.

```powershell
copy llms.example.json llms.json    # then edit llms.json
```

| Template | Real file | When you need it |
|---|---|---|
| `llms.example.json` | `llms.json` | To choose the AI provider/model, or to keep the API keys in the file instead of in `ANTHROPIC_API_KEY`. `active` names which `providers` entry to use; each provider points to a key in `api_keys` through `api_key_ref`. |
| `telegramsettings.example.json` | `telegramsettings.json` | Only to enable Telegram: the bot token obtained from `@BotFather`. |
| `fritzbox.example.json` | `fritzbox.json` | Only for the `fritzbox_status` tool of `/netsec`: router host, user and password. |
| `network.example.json` | `network.json` | AI Chat on the LAN. Optional: the orchestrator generates it on first run (service disabled); it can also be edited from `/config` → AI Chat. |
| `search-paths.example.json` | `search-paths.json` | Folders indexed by `/find`. Optional: generated on first run with the user's standard folders. |
| `search-content.example.json` | `search-content.json` | Extensions treated as text/binary by the content search. Optional: generated on first run. |
| `market_data.example.json` | `market_data.json` | Data source for `/markets`. Optional: set from `/config` → Market Data. |
| `config.example.json` | `config.json` | UI preferences (language, transparency, web search). Optional: set from `/config`. |

`notes.json`, `telegram-state.json`, `token`, `memory-<label>.md` have no template: they are
entirely generated and managed by the program.

## Files generated at runtime — NEVER commit (secrets or local data)

Created on the binaries' first run, one per machine/deploy. None of these goes into `git add`,
even though `deploy_test_run.ps1` never touches them directly:

| File | Created by | Secret? |
|---|---|---|
| `token` | orchestrator, on first run (`token_store::resolve_token`) | **Yes** — authentication token of the WS channel (256 bit) |
| `llms.json` | never auto-generated: create it by hand if you want an AI provider other than the default (direct Claude via `ANTHROPIC_API_KEY`) | **Yes** — contains `api_keys` |
| `telegramsettings.json` | never auto-generated: create it by hand to enable the Telegram channel | **Yes** — contains the bot token |
| `telegram-state.json` | orchestrator, when `telegramsettings.json` is present (TOTP secret + paired chat id) | **Yes** |
| `config.json` | `ui.exe`, on the first save of the window settings | No (local, not sensitive) |
| `search-paths.json` | orchestrator, on every start (`PathsConfig::load_or_generate`) — folders indexed by the search | No, but machine-specific (local absolute paths) |
| `search-content.json` | orchestrator, on every start (`ContentConfig::load_or_generate`) | No, machine-specific |
| `network.json` | orchestrator, on every start (`load_or_generate_with_migration`) — AI Chat settings (nickname, discovery port, auto-participation), disabled by default | No |
| `market_data.json` | `ui.exe`, on the first save of the "market data source" settings (`/config` tab) | No, but machine-specific (e.g. IB Gateway port/host) |
| `aichat.json` | legacy name of `network.json` (pre-migration): read only for compatibility if present, never written under this name by the current code | No |
| `notes.json` | orchestrator, on the first save of a note from AI Chat | No |
| `memory-<label>.md` | orchestrator, persistent AI Chat memory per AI label | No, but it is local conversational content |

## Folders created at runtime — NEVER commit their contents

| Folder | Created by |
|---|---|
| `logs/` | orchestrator, on first run (`logging::open_log_file`) — daily logs `orchestrator.log.<date>` |
| `library/documents/` | `ui.exe`, on first run (`create_dir_all` in `main.rs`, Tauri setup) |
| `library/find/` | `ui.exe`, on first run (saved searches) |
| `plugin-storage/<id>/` | orchestrator, for each discovered plugin (the plugin's private storage) |

The whole of `library/` and `logs/` are excluded by `.gitignore`: there is no need (and it must
not be done) to create a `.gitkeep` inside them — the respective apps create them on their own
with `create_dir_all` on first run.

## Plugins and pytools

`plugins/` and `pytools/` do NOT live in here: they are sibling folders of `Configuration\` (the
deploy root, `Test Run\`), consistently with the relative-path rule explained above
(`paths.plugins_dir: "plugins"`, `paths.pytools_dir: "pytools"`).
