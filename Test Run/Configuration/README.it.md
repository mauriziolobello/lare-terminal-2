# Test Run/Configuration/

[English](./README.md) | **Italiano**

Questa è la cartella di configurazione (`--config-dir`) usata da **tutti** i
binari quando girano da `Test Run\` (l'orchestratore, `ui.exe`, e a cascata i
processi figli — `mcp-server.exe`, i plugin, gli script Python). Regola unica
(spec 2.0, decisione D6, vedi `crates/startup-config/src/lib.rs`): ogni
binario usa `--config-dir <path>` se passato, altrimenti
`<cartella dell'eseguibile>\Configuration\`. Nessuna variabile d'ambiente
`LARE_*` viene mai letta.

**Importante — i percorsi relativi in `startup.json` (`paths.*`, `log.dir`)
sono risolti rispetto alla RADICE DEL DEPLOY, cioè la cartella che CONTIENE
`Configuration\` (qui: `Test Run\`), non rispetto a questa cartella e non
rispetto alla cwd del processo.** Esempio: `"mcp_server": "mcp-server.exe"`
risolve a `Test Run\mcp-server.exe`, non a
`Test Run\Configuration\mcp-server.exe`.

## File committati (template, nessun segreto)

| File | Cos'è |
|---|---|
| `startup.json` | Template di configurazione — porta WS, path relativi (shell, mcp-server, plugin, pytools, routines), modello AI, autostart, log. Coincide con i default hard-coded (`StartupConfig::default()`): un file assente avrebbe lo stesso effetto, il template esiste per essere il punto dove editare la porta/il modello senza dover ricordare i default. |
| `README.it.md`, `README.md` | Questo file (italiano) e la sua versione inglese. |
| `routines/.gitkeep` | Mantiene tracciata la cartella `routines/` (destinazione di `paths.routines_dir`) prima che l'utente ci salvi la prima routine. |
| `i18n/<lang>.json` | Dizionari dell'interfaccia, una lingua per file (`it`, `en`, `es`, `de`, `fr`, `nl`, `da`, `ru`, `pl`). `it.json` è la base e il fallback. |
| `help/<lang>.md` | Corpo della finestra `/help`, una lingua per file (stesse nove lingue). |
| `*.example.json` | Template dei file di configurazione non committati — vedi sotto. |

## Template `*.example.json` — da copiare per creare la propria configurazione

I file con segreti o dati personali (sotto, "File generati a runtime") non sono nel
repository. Per ciascuno di quelli che un utente può voler scrivere o ritoccare a mano esiste
un template con la stessa struttura: copialo togliendo `.example` dal nome e sostituisci i
segnaposto `<your-...>` con i tuoi valori.

```powershell
copy llms.example.json llms.json    # poi modifica llms.json
```

| Template | File reale | Quando serve |
|---|---|---|
| `llms.example.json` | `llms.json` | Per scegliere provider/modello AI o mettere le chiavi API nel file invece che in `ANTHROPIC_API_KEY`. `active` indica quale voce di `providers` usare; ogni provider rimanda a una chiave di `api_keys` tramite `api_key_ref`. |
| `telegramsettings.example.json` | `telegramsettings.json` | Solo per attivare Telegram: il token del bot ottenuto da `@BotFather`. |
| `fritzbox.example.json` | `fritzbox.json` | Solo per il tool `fritzbox_status` di `/netsec`: host, utente e password del router. |
| `network.example.json` | `network.json` | AI Chat in LAN. Facoltativo: l'orchestrator lo genera al primo avvio (servizio disattivo); si modifica anche da `/config` → AI Chat. |
| `search-paths.example.json` | `search-paths.json` | Cartelle indicizzate da `/find`. Facoltativo: generato al primo avvio con le cartelle standard dell'utente. |
| `search-content.example.json` | `search-content.json` | Estensioni trattate come testo/binario dalla ricerca nel contenuto. Facoltativo: generato al primo avvio. |
| `market_data.example.json` | `market_data.json` | Fonte dati di `/markets`. Facoltativo: si imposta da `/config` → Dati Mercato. |
| `config.example.json` | `config.json` | Preferenze UI (lingua, trasparenza, ricerca web). Facoltativo: si imposta da `/config`. |

`notes.json`, `telegram-state.json`, `token`, `memory-<label>.md` non hanno template: sono
interamente generati e gestiti dal programma.

## File generati a runtime — MAI committare (segreti o dati locali)

Creati dal primo avvio dei binari, uno per macchina/deploy. Nessuno di
questi va in `git add`, anche se `deploy_test_run.ps1` non li tocca mai
direttamente:

| File | Chi lo crea | Segreto? |
|---|---|---|
| `token` | orchestrator, al primo avvio (`token_store::resolve_token`) | **Sì** — token di autenticazione del canale WS (256 bit) |
| `llms.json` | mai auto-generato: va creato a mano se si vuole un provider AI diverso dal default (Claude diretto via `ANTHROPIC_API_KEY`) | **Sì** — contiene `api_keys` |
| `telegramsettings.json` | mai auto-generato: va creato a mano per attivare il canale Telegram | **Sì** — contiene il token del bot |
| `telegram-state.json` | orchestrator, quando `telegramsettings.json` è presente (secret TOTP + chat id appaiata) | **Sì** |
| `config.json` | `ui.exe`, al primo salvataggio delle impostazioni finestra | No (locale, non sensibile) |
| `search-paths.json` | orchestrator, a ogni avvio (`PathsConfig::load_or_generate`) — cartelle indicizzate dalla ricerca | No, ma specifico della macchina (percorsi assoluti locali) |
| `search-content.json` | orchestrator, a ogni avvio (`ContentConfig::load_or_generate`) | No, specifico della macchina |
| `network.json` | orchestrator, a ogni avvio (`load_or_generate_with_migration`) — impostazioni AI Chat (nickname, porta di discovery, autopartecipazione), disattivo di default | No |
| `market_data.json` | `ui.exe`, al primo salvataggio delle impostazioni "fonte dati mercato" (tab `/config`) | No, ma specifico della macchina (es. porta/host di IB Gateway) |
| `aichat.json` | nome legacy di `network.json` (pre-migrazione): letto solo per compatibilità se presente, mai scritto con questo nome nel codice attuale | No |
| `notes.json` | orchestrator, al primo salvataggio di una nota da AI Chat | No |
| `memory-<label>.md` | orchestrator, memoria persistente di AI Chat per etichetta AI | No, ma è contenuto conversazionale locale |

## Cartelle create a runtime — MAI committare il contenuto

| Cartella | Chi la crea |
|---|---|
| `logs/` | orchestrator, al primo avvio (`logging::open_log_file`) — log giornalieri `orchestrator.log.<data>` |
| `library/documents/` | `ui.exe`, al primo avvio (`create_dir_all` in `main.rs`, setup Tauri) |
| `library/find/` | `ui.exe`, al primo avvio (ricerche salvate) |
| `plugin-storage/<id>/` | orchestrator, per ogni plugin scoperto (storage privato del plugin) |

`library/` intera e `logs/` sono escluse da `.gitignore`: non serve (e non va
fatto) creare `.gitkeep` al loro interno — le rispettive app le creano da
sole con `create_dir_all` al primo avvio.

## Plugin e pytools

`plugins/` e `pytools/` NON vivono qui dentro: sono cartelle sorelle di
`Configuration\` (root del deploy, `Test Run\`), coerentemente con la regola
dei path relativi spiegata sopra (`paths.plugins_dir: "plugins"`,
`paths.pytools_dir: "pytools"`).
