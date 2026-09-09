# Lare — Commands

Type `/…` commands in the Lare Terminal command line (your PowerShell session).
The result of each slash command appears in a window; a confirmation line remains in the terminal.

## AI
- `/ai "prompt"` or `/ "prompt"` — AI answers and executes commands **in your shell**
  (each proposed command asks for `[Y/n]` confirmation before running). Quotes are required.

## Commands
- `/help` — this window.
- `/ping` — check Lare layers (lare-shell, orchestrator, plugin-ping, ui.exe).
- `/config` — configuration (appearance, web search, AI, markets).
- `/library` — archive of saved documents (reopenable).
- `/aichat` — AI Chat (communication between networked Lare machines, with AI participation).
- `/open <target>` — open a URL, folder, or file with the default app. No quotes around
  the target (unlike `/ai` above): type it as-is, even with spaces.
- `/web <query>` — search the query in the default browser.
- `/show <markdown>` — open a window with the given Markdown.
- `/find [<query>] [in:"<phrase>"] [folder:from-here]` — live file search, dedicated window.
- `/reset` — restart the shell session.
- `/nowin <prompt>` — AI answers as text in the terminal, no Markdown window.
- `/calc` — calculator (plugin).

## External tools (dedicated window)
- `/markets` — financial market tools (ticker search, stock report, symbol list, screener).
- `/nmap` — network scanning tools (quick scan, OS/version detection, host discovery, vulnerability scan).
- `/pyping` — test channel for the Python tool infrastructure (message echo).

## Everything else
- Any line that does not start with `/` is PowerShell, as always.
- An unknown slash command is silently ignored.
