# Lare — Befehle

Gib `/…`-Befehle in der Befehlszeile von Lare Terminal ein (deiner PowerShell-Sitzung).
Das Ergebnis jedes Slash-Befehls erscheint in einem Fenster; im Terminal bleibt eine Bestätigungszeile stehen.

## KI
- `/ai "Anfrage"` oder `/ "Anfrage"` — die KI antwortet und führt Befehle **in deiner Shell** aus
  (jeder vorgeschlagene Befehl fragt vor der Ausführung mit `[Y/n]` um Bestätigung). Anführungszeichen sind Pflicht.

## Befehle
- `/help` — dieses Fenster.
- `/ping` — prüft die Schichten von Lare (lare-shell, Orchestrator, plugin-ping, ui.exe).
- `/config` — Konfiguration (Aussehen, Websuche, KI, Märkte).
- `/library` — Archiv gespeicherter Dokumente (wieder öffenbar).
- `/aichat` — AI Chat (Kommunikation zwischen vernetzten Lare-Maschinen, mit KI-Teilnahme).
- `/open <target>` — öffnet eine URL, einen Ordner oder eine Datei mit der Standard-App. Keine
  Anführungszeichen um das Ziel (anders als oben bei `/ai`): gib es so ein, wie es ist, auch mit
  Leerzeichen.
- `/web <query>` — sucht die Abfrage im Standardbrowser.
- `/show <markdown>` — öffnet ein Fenster mit dem angegebenen Markdown.
- `/find [<query>] [in:"<Ausdruck>"] [folder:from-here]` — Live-Dateisuche, eigenes Fenster.
- `/reset` — startet die Shell-Sitzung neu.
- `/nowin <Anfrage>` — die KI antwortet als Text im Terminal, kein Markdown-Fenster.
- `/calc` — Taschenrechner (Plugin).

## Externe Tools (eigenes Fenster)
- `/markets` — Tools für Finanzmärkte (Ticker-Suche, Aktienreport, Symbolliste, Screener).
- `/netsec` — Tools für Netzwerkscans und -diagnose (Router-Status, Quick Scan, OS-/Versionserkennung, Host Discovery, Schwachstellenscan).
- `/pyping` — Testkanal für die Python-Tool-Infrastruktur (Nachrichten-Echo).

## Alles andere
- Jede Zeile, die nicht mit `/` beginnt, ist PowerShell, wie immer.
- Ein unbekannter Slash-Befehl wird stillschweigend ignoriert.
