# Lare — Comandi

Scrivi i comandi `/…` nella riga di comando di Lare Terminal (la tua sessione PowerShell).
L'esito di ogni comando slash compare in una finestra; nel terminale resta una riga di conferma.

## AI
- `/ai "richiesta"` oppure `/ "richiesta"` — l'AI risponde ed esegue comandi **nella tua shell**
  (ogni comando proposto chiede conferma `[Y/n]` prima di partire). Le virgolette sono obbligatorie.

## Comandi
- `/help` — questa finestra.
- `/ping` — verifica gli strati di Lare (lare-shell, orchestratore, plugin-ping, ui.exe).
- `/config` — configurazione (aspetto, ricerca web, AI, mercati).
- `/library` — archivio dei documenti salvati (riapribili).
- `/aichat` — AI Chat (comunicazione fra macchine Lare in rete, con partecipazione dell'AI).
- `/open <target>` — apri un URL, una cartella o un file con l'app di default.
- `/web <query>` — cerca la query nel browser di default.
- `/show <markdown>` — apri una finestra con il Markdown indicato.
- `/find [<query>] [in:"<frase>"] [folder:from-here]` — cerca file dal vivo, finestra dedicata.
- `/reset` — riavvia la sessione shell.
- `/nowin <richiesta>` — l'AI risponde come testo nel terminale, niente finestra Markdown.
- `/calc` — calcolatrice (plugin).

## Strumenti esterni (finestra dedicata)
- `/markets` — strumenti sui mercati finanziari (ricerca ticker, report azionario, elenco titoli, screener).
- `/nmap` — strumenti di scansione di rete (quick scan, rilevamento OS/versioni, host discovery, ricerca vulnerabilità).
- `/pyping` — canale di prova per l'infrastruttura dei tool Python (eco di un messaggio).

## Tutto il resto
- Qualunque riga che non inizia con `/` è PowerShell, come sempre.
- Uno slash sconosciuto viene ignorato in silenzio.
