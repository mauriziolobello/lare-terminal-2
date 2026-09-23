# Lare — Opdrachten

Typ `/…`-opdrachten in de opdrachtregel van Lare Terminal (je PowerShell-sessie).
Het resultaat van elke slash-opdracht verschijnt in een venster; in de terminal blijft een bevestigingsregel staan.

## AI
- `/ai "verzoek"` of `/ "verzoek"` — de AI antwoordt en voert opdrachten uit **in je shell**
  (elke voorgestelde opdracht vraagt vóór uitvoering om bevestiging met `[Y/n]`). Aanhalingstekens zijn verplicht.

## Opdrachten
- `/help` — dit venster.
- `/ping` — controleert de lagen van Lare (lare-shell, orchestrator, plugin-ping, ui.exe).
- `/config` — configuratie (uiterlijk, web zoeken, AI, markten).
- `/library` — archief van opgeslagen documenten (opnieuw te openen).
- `/aichat` — AI Chat (communicatie tussen Lare-machines in hetzelfde netwerk, met AI-deelname).
- `/open <target>` — opent een URL, map of bestand met de standaardapp. Geen
  aanhalingstekens om het doel (anders dan hierboven bij `/ai`): typ het zoals het is, ook met
  spaties.
- `/web <query>` — zoekt de query op in de standaardbrowser.
- `/show <markdown>` — opent een venster met de opgegeven Markdown.
- `/find [<query>] [in:"<uitdrukking>"] [folder:from-here]` — live bestandszoekfunctie, eigen venster.
- `/reset` — herstart de shell-sessie.
- `/nowin <verzoek>` — de AI antwoordt als tekst in de terminal, geen Markdown-venster.
- `/calc` — rekenmachine (plugin).

## Externe tools (eigen venster)
- `/markets` — tools voor financiële markten (tickerzoekfunctie, aandelenrapport, symbolenlijst, screener).
- `/netsec` — tools voor netwerkscans en -diagnose (routerstatus, quick scan, OS-/versiedetectie, host discovery, kwetsbaarheidsscan).
- `/pyping` — testkanaal voor de Python-tool-infrastructuur (bericht-echo).

## Al het overige
- Elke regel die niet met `/` begint, is PowerShell, zoals altijd.
- Een onbekende slash-opdracht wordt stilzwijgend genegeerd.
