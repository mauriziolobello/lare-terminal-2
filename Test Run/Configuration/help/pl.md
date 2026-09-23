# Lare — Polecenia

Wpisuj polecenia `/…` w wierszu poleceń Lare Terminal (w twojej sesji PowerShell).
Wynik każdego polecenia slash pojawia się w oknie; w terminalu pozostaje linia potwierdzenia.

## AI
- `/ai "zapytanie"` lub `/ "zapytanie"` — AI odpowiada i wykonuje polecenia **w twoim shellu**
  (każde proponowane polecenie prosi o potwierdzenie `[Y/n]` przed uruchomieniem). Cudzysłowy są wymagane.

## Polecenia
- `/help` — to okno.
- `/ping` — sprawdza warstwy Lare (lare-shell, orchestrator, plugin-ping, ui.exe).
- `/config` — konfiguracja (wygląd, wyszukiwanie w sieci, AI, rynki).
- `/library` — archiwum zapisanych dokumentów (można je ponownie otworzyć).
- `/aichat` — AI Chat (komunikacja między połączonymi w sieć maszynami Lare, z udziałem AI).
- `/open <target>` — otwiera adres URL, folder lub plik domyślną aplikacją. Bez cudzysłowów
  wokół celu (inaczej niż `/ai` powyżej): wpisz go tak, jak jest, nawet ze spacjami.
- `/web <query>` — szuka zapytania w domyślnej przeglądarce.
- `/show <markdown>` — otwiera okno z podanym Markdown.
- `/find [<query>] [in:"<fraza>"] [folder:from-here]` — wyszukiwanie plików na żywo, dedykowane okno.
- `/reset` — restartuje sesję shell.
- `/nowin <zapytanie>` — AI odpowiada jako tekst w terminalu, bez okna Markdown.
- `/calc` — kalkulator (plugin).

## Narzędzia zewnętrzne (osobne okno)
- `/markets` — narzędzia rynków finansowych (wyszukiwanie tickerów, raport spółki, lista symboli, screener).
- `/netsec` — narzędzia do skanowania i diagnostyki sieci (stan routera, szybki skan, wykrywanie systemu operacyjnego/wersji, wykrywanie hostów, skanowanie podatności).
- `/pyping` — kanał testowy dla infrastruktury narzędzi Python (echo wiadomości).

## Wszystko inne
- Każda linia, która nie zaczyna się od `/`, jest PowerShell, jak zawsze.
- Nieznane polecenie slash jest po cichu ignorowane.
