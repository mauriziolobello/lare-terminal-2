# Lare Terminal 2.0

[English](./README.md) | **Italiano**

Un terminale PowerShell con un'AI agentica integrata nella riga di comando — non un chatbot a
fianco, non un overlay: un vero terminale (prompt reale, storico, profili, `cd` che persiste) in
cui le righe che iniziano con `/` sono comandi speciali, e `/ai "..."` fa agire l'AI **nella
stessa sessione di shell**, con lo stesso `cwd`, dietro un gate di conferma esplicito su ogni
comando che propone di eseguire.

Continua la storia di [Lare Terminal](https://github.com/mauriziolobello/lare-terminal), la sua
prima incarnazione (un overlay trasparente richiamato a tasto) — quel repository resta pubblico
come riferimento storico completo.

## Cosa fa

- **Shell PowerShell vera** (host custom del motore PowerShell, PSReadLine, profili) in una
  finestra terminale propria — oppure come profilo di Windows Terminal.
- **`/ai "richiesta"`** — l'AI propone ed esegue comandi nella tua sessione, ognuno dietro una
  conferma `[Y/n]`; la risposta finale si apre in una finestra Markdown salvabile.
- **Comandi `/…`** — `/help`, `/config`, `/library` (archivio documenti), `/find` (ricerca file
  dal vivo), `/open`, `/web`, `/show`, e altri.
- **Canali esterni a tool fissi** — `/netsec` (diagnostica di rete su `nmap`), `/markets` (analisi
  mercati finanziari, tool Python).
- **AI Chat** — stanza condivisa fra più macchine Lare nella stessa LAN, con partecipazione delle AI.
- **Telegram** — comandi da remoto, con pairing e secondo fattore TOTP.
- **Plugin** — eseguibili sidecar con finestra propria (`/calc`, `/ping`, …).
- **Nove lingue** d'interfaccia e di risposta AI: italiano, inglese, spagnolo, tedesco, francese,
  olandese, danese, russo, polacco.

## Requisiti

Solo **Windows 10/11 x64**.

Per compilare:

- [Rust](https://rustup.rs/) stabile, toolchain MSVC (con i Build Tools di Visual Studio)
- [.NET 10 SDK](https://dotnet.microsoft.com/)
- Node.js — solo per i test JS del frontend e per lo script di sviluppo `shell-client.mjs`

Per eseguire:

- **WebView2 Runtime** (già presente su Windows 10/11 aggiornati)
- **PowerShell 7.6+** (`pwsh`) installato — la shell carica PSReadLine dai suoi moduli
- **.NET 10 Runtime** x64
- Una **chiave API Anthropic** (o di un altro provider supportato) — senza, l'AI risponde con uno
  stub di prova
- Opzionali: `nmap` per `/netsec`; Python 3 per `/markets` e lo stato del router FRITZ!Box

## Avvio rapido

```powershell
git clone https://github.com/mauriziolobello/lare-terminal-2.git
cd lare-terminal-2

.\build.ps1 -IncludePlugins            # compila Rust + host C# (la prima volta è lenta)
.\deploy_test_run.ps1 -IncludePlugins  # popola la cartella eseguibile "Test Run\"
```

Poi la chiave dell'AI — una delle due:

```powershell
# a) variabile d'ambiente (vale dai terminali aperti DOPO il setx)
setx ANTHROPIC_API_KEY "sk-ant-..."

# b) oppure file di configurazione (permette anche altri provider)
copy "Test Run\Configuration\llms.example.json" "Test Run\Configuration\llms.json"
#    e sostituisci i segnaposto <your-...-api-key> con le tue chiavi
```

E si avvia:

```powershell
cd "Test Run"
.\ui.exe
```

Un solo comando: si apre la finestra del terminale e l'orchestratore parte da solo in background.
Prova `/help`, poi `/ai "elenca i 3 file più grandi qui"`.

## Configurazione

Tutta la configurazione vive in `Test Run\Configuration\` (o nella cartella passata con
`--config-dir`). I file con segreti o dati personali **non sono nel repository**: per ciascuno c'è
un **template `*.example.json`** da copiare togliendo `.example` dal nome e sostituendo i
segnaposto `<your-...>`.

| Template | Serve per | Necessario? |
|---|---|---|
| `llms.example.json` | chiavi API e scelta del provider/modello AI | solo se non usi `ANTHROPIC_API_KEY` o vuoi un altro provider |
| `telegramsettings.example.json` | token del bot Telegram | solo per attivare Telegram |
| `network.example.json` | AI Chat in LAN (nome, porta, partecipazione dell'AI) | no — generato al primo avvio, disattivo |
| `fritzbox.example.json` | credenziali del router FRITZ!Box per `/netsec` | solo per `fritzbox_status` |
| `search-paths.example.json` | cartelle indicizzate da `/find` | no — generato al primo avvio |
| `search-content.example.json` | estensioni considerate testo/binario da `/find` | no — generato al primo avvio |
| `market_data.example.json` | fonte dati per `/markets` | no — si imposta da `/config` |
| `config.example.json` | preferenze UI (lingua, trasparenza, ricerca web) | no — si imposta da `/config` |

Dettagli su ogni file (chi lo crea, quando, cosa è segreto):
[`Test Run/Configuration/README.md`](./Test%20Run/Configuration/README.md).

## Cosa leggere, in ordine

1. [`00-apertura.md`](./Docs/i18n/ita/00-apertura.md) — cos'è, perché esiste, mappa dei documenti.
2. [`BUILD.md`](./Docs/i18n/ita/BUILD.md) → [`DEPLOY.md`](./Docs/i18n/ita/DEPLOY.md) →
   [`RUN.md`](./Docs/i18n/ita/RUN.md) — compilare, preparare la cartella eseguibile, avviare
   (con i gotcha di Windows).
3. [`KNOWN-ISSUES.md`](./Docs/i18n/ita/KNOWN-ISSUES.md) — limiti noti prima di sorprendersi.
4. Per capire come è fatto: [`01-architettura.md`](./Docs/i18n/ita/01-architettura.md),
   [`02-decisions.md`](./Docs/i18n/ita/02-decisions.md) (log delle decisioni),
   [`03-stato-e-implementazione.md`](./Docs/i18n/ita/03-stato-e-implementazione.md), poi i
   documenti per sottosistema (plugin, tool Python, canali, lingue).

## Stack

Rust (orchestratore, protocollo, server di tool, interfaccia Tauri), C# (host custom del motore
PowerShell), JavaScript vanilla (frontend delle finestre), Python (tool di dominio via MCP).
Dettagli e perché delle scelte in [`01-architettura.md`](./Docs/i18n/ita/01-architettura.md).

## Licenza

[MIT](./LICENSE).
