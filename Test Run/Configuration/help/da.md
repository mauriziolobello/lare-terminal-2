# Lare — Kommandoer

Skriv `/…`-kommandoer på kommandolinjen i Lare Terminal (din PowerShell-session).
Resultatet af hver slash-kommando vises i et vindue; en bekræftelseslinje bliver stående i terminalen.

## AI
- `/ai "forespørgsel"` eller `/ "forespørgsel"` — AI'en svarer og udfører kommandoer **i din shell**
  (hver foreslået kommando beder om `[Y/n]`-bekræftelse, før den kører). Anførselstegn er obligatoriske.

## Kommandoer
- `/help` — dette vindue.
- `/ping` — kontrollerer Lare-lagene (lare-shell, orchestrator, plugin-ping, ui.exe).
- `/config` — konfiguration (udseende, websøgning, AI, markeder).
- `/library` — arkiv med gemte dokumenter (kan genåbnes).
- `/aichat` — AI Chat (kommunikation mellem Lare-maskiner i netværk, med AI-deltagelse).
- `/open <target>` — åbner en URL, en mappe eller en fil med standardappen. Ingen
  anførselstegn omkring målet (i modsætning til `/ai` ovenfor): skriv det, som det er, også med
  mellemrum.
- `/web <query>` — søger forespørgslen i standardbrowseren.
- `/show <markdown>` — åbner et vindue med den angivne Markdown.
- `/find [<query>] [in:"<udtryk>"] [folder:from-here]` — live-filsøgning, eget vindue.
- `/reset` — genstarter shell-sessionen.
- `/nowin <forespørgsel>` — AI'en svarer som tekst i terminalen, intet Markdown-vindue.
- `/calc` — lommeregner (plugin).

## Eksterne værktøjer (eget vindue)
- `/markets` — værktøjer til finansielle markeder (ticker-søgning, aktierapport, symbolliste, screener).
- `/netsec` — værktøjer til netværksscanning og -diagnostik (routerstatus, quick scan, OS-/versionsdetektering, host discovery, sårbarhedsscanning).
- `/pyping` — testkanal for Python-værktøjsinfrastrukturen (besked-ekko).

## Alt andet
- Enhver linje, der ikke starter med `/`, er PowerShell, som altid.
- En ukendt slash-kommando ignoreres stiltiende.
