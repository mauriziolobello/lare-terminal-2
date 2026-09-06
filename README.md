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

**Piano 1 ("fondamenta") completato** (2026-09-05): workspace con tutti i crate della v1 copiati e
versionati `2.0.x`, regola unica di configurazione (`--config-dir`/`startup.json`, nessuna
variabile d'ambiente `LARE_*`), `ui.exe` ridotto a puro host di finestre (overlay F2 rimosso), e
`Test Run\` come layout di deploy verificato dal vivo dentro il repo.
**Piano 2a ("protocollo shell") completato** (2026-09-06): il canale shell (`protocol`/
`orchestrator`/`ui` 2.1.0, ADR-018) è pronto lato server — registro delle connessioni, gate di
conferma, router di superficie con output in finestra Markdown, built-in `/ping` — verificabile
oggi con il client di sviluppo `scripts/dev/shell-client.mjs` (nessuna dipendenza da `lare-shell`).
Dettagli, versioni correnti e debiti noti in `Docs/i18n/ita/HANDOFF.md`; checklist e2e in
`Docs/i18n/ita/TESTING-e2e.md`.
**`shell\lare-shell.exe` (la host C# vera) non esiste ancora** — prossimo passo il **piano 2b**
(quella host, che parla il protocollo già pronto), seguito dal piano 3 (finestra terminale
Tauri). Tutta la documentazione sta sotto `Docs/i18n/<lingua>/` (italiano come riferimento).

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

## Decisioni già prese (2026-09-04/05)

- **Lare È la shell, non un suo ospite** (ADR-015): `lare-shell` è una **host custom del motore
  PowerShell in C#** (come `pwsh.exe` è una host di quel motore) — possiede il REPL, legge con
  PSReadLine, intercetta le righe `/…`, esegue il resto in-process. La shell posseduta dal
  `mcp-server` v1 resta solo per i canali senza shell utente (Telegram, AI Chat).
- **Lare Terminal è una finestra Tauri** con emulatore xterm.js che ospita `lare-shell` via
  ConPTY; barre e segnalini in HTML fuori dall'area terminale (ADR-016). Entrambe le decisioni
  confermate da due spike usa-e-getta, **conclusi con successo** — esiti in
  `Docs/i18n/ita/spikes/`; lo spec (`Docs/i18n/ita/superpowers/specs/`) è stato riscritto sulla
  forma finale.
- L'overlay F2 della v1 **non è sopravvissuto** (rimosso nel piano 1): `ui.exe` resta host di
  finestre Tauri (Markdown, `/config`, `/library`, plugin). Ogni output dei comandi slash, `/ai`
  incluso, va in una finestra Markdown; nel terminale restano prompt di conferma, conferme brevi
  ed errori.
- Comportamento **non difforme dalla v1** per le superfici che restano: `/markets`, `/calc`,
  `/library` aprono le stesse finestre Tauri. AI Chat, Library, `/config`, `/help` sono finestre
  **uniche per macchina**, condivise da tutte le sessioni Lare aperte.
- MVP: plugin `/ping` (round-trip di salute attraverso tutti gli strati) e `/calc`; slash
  `/help`, `/config`, `/open`, `/web`, `/library`, `/ai "testo"` (≡ `/ "testo"`).
- Niente variabili d'ambiente dove evitabile: tutto in file JSON sotto `Configuration\`; unico
  override ammesso il flag `--config-dir` da riga di comando. Una sola cwd: quella della sessione
  PowerShell.
- Documentazione narrativa in `Docs/i18n/<lingua>/`, italiano (`ita`) come riferimento.
