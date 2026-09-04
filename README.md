# Lare Terminal 2.0

Riscrittura di [Lare Terminal](https://github.com/mauriziolobello/lare-terminal) con un modello di
esecuzione diverso: il front-end non è più un overlay Tauri standalone richiamato via hotkey, ma
un **command layer integrato nella shell reale dell'utente** (PowerShell su Windows; bash/zsh su
Linux/macOS in seguito). Le righe che iniziano con `/` vengono intercettate dalla shell e inoltrate
al daemon Lare; tutto il resto resta alla shell.

Obiettivo dichiarato: **mantenere il più possibile** di quanto costruito nella v1 (orchestratore,
loop tool-use AI multi-LLM, gate di conferma, plugin sidecar, tool Python MCP, Library, Telegram,
AI Chat), cambiando solo il punto d'ingresso e ciò che vi è accoppiato.

## Stato

**Brainstorming in corso** (dal 2026-09-04). Nessun codice ancora. Spec di design in
`Docs/i18n/ita/superpowers/specs/`; da lì il piano di implementazione. Tutta la documentazione
sta sotto `Docs/i18n/<lingua>/` (italiano come riferimento).

## Punti di partenza

- **v1** — `C:\Users\Maurizio\Documents\Progetti\Lare Terminal`, leggere per primo
  `Docs/STATO-ATTUALE.md` (Parte II §18-20: contratti tecnici verificati sul codice e mappa di
  accoppiamento al modello di esecuzione attuale, con le domande aperte da cui parte il 2.0).
- **Bozza 2.0** — Google Doc "Lare Terminal 2.0" (proposta di un'altra AI, scritta senza conoscere
  la v1: buona come inquadramento — adapter sottile → daemon, CLI-first, namespace gerarchici — non
  come spec).
- **Layout di deploy v1** — `C:\Lare Terminal` (binari + `Local\`/`Roaming\` + `plugins\` +
  `pytools\` + `startup.json`). Il 2.0 lo replica in `Test Run\` dentro questo repo, con
  un'unica cartella `Configuration\` al posto di `Local\`+`Roaming\`.

## Decisioni già prese (2026-09-04)

- I comandi OS proposti dall'AI eseguono **nella shell reale dell'utente** (Lare è ospite, non
  proprietario della shell) fin dalla prima versione. La shell posseduta dal `mcp-server` v1 resta
  per i canali senza shell utente (Telegram, AI Chat).
- **Sterzata (sera del 2026-09-04):** il lato shell non è un hook PSReadLine + client CLI, ma una
  **host PowerShell custom in C#** (`lare-shell`, come `pwsh.exe` è una host del motore
  PowerShell): possiede il REPL, legge con PSReadLine, intercetta le righe `/…`, esegue il resto
  in-process, disegna barra di stato e segnalini nel terminale, e si presenta come profilo
  "Lare Terminal" in Windows Terminal. Il core Rust (orchestratore, protocollo, plugin, pytools)
  non cambia. In corso uno **spike** usa-e-getta in `spikes/lare-shell-host/`; lo spec in
  `Docs/i18n/ita/superpowers/specs/` verrà riscritto nelle sezioni shell (§2, §4, §6) dopo
  l'esito.
- L'overlay F2 della v1 **non sopravvive**: `ui.exe` resta solo host di finestre Tauri
  (Markdown, `/config`, `/library`, plugin). Ogni output dei comandi slash, `/ai` incluso, va in
  una finestra Markdown; nel terminale restano prompt di conferma, conferme brevi ed errori.
- Comportamento **non difforme dalla v1** per le superfici che restano: `/markets`, `/calc`,
  `/library` aprono le stesse finestre Tauri. AI Chat, Library, `/config`, `/help` sono finestre
  **uniche per macchina**, condivise da tutte le sessioni Lare aperte.
- MVP: plugin `/ping` (round-trip di salute attraverso tutti gli strati) e `/calc`; slash
  `/help`, `/config`, `/open`, `/web`, `/library`, `/ai "testo"` (≡ `/ "testo"`).
- Niente variabili d'ambiente dove evitabile: tutto in file JSON sotto `Configuration\`; unico
  override ammesso il flag `--config-dir` da riga di comando. Una sola cwd: quella della sessione
  PowerShell.
- Documentazione narrativa in `Docs/i18n/<lingua>/`, italiano (`ita`) come riferimento.
