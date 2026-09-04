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
`Docs/superpowers/specs/` quando approvata; da lì il piano di implementazione.

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
  proprietario della shell) fin dalla prima versione. Meccanismo: *exit-and-resume* fra wrapper
  shell e client CLI Rust; nuovo `ToolClient` lato orchestratore. La shell posseduta dal
  `mcp-server` v1 resta per i canali senza shell utente (Telegram, AI Chat).
- Comportamento **non difforme dalla v1** per le superfici che restano: `/markets`, `/calc`,
  `/library` aprono le stesse finestre Tauri.
- MVP: plugin `/ping` (round-trip di salute attraverso tutti gli strati) e `/calc`; slash
  `/help`, `/config`, `/open`, `/web`, `/library`.
- Niente variabili d'ambiente dove evitabile: tutto in file JSON sotto `Configuration\`; unico
  override ammesso il flag `--config-dir` da riga di comando.
