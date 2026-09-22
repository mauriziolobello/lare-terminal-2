# Architettura

Questo documento spiega **come è fatto** Lare Terminal e **perché** — la forma attuale è il punto
d'arrivo di una serie di decisioni motivate, non un progetto disegnato tutto insieme all'inizio. Il
log completo, decisione per decisione, è in [`02-decisions.md`](./02-decisions.md); qui la stessa
storia è raccontata in un unico filo.

## I tre strati

L'architettura si regge su un principio stabilito fin dalla prima versione del progetto e mai
rimesso in discussione: **MCP standardizza i tool, non l'AI**. "Un'AI che possa usare qualunque
capacità e possa essere sostituita" contiene due esigenze diverse, e ognuna ha bisogno del proprio
meccanismo — confonderle porta a un design ambiguo. Da qui tre strati distinti:

```
Canali  →  Orchestratore  →  Server di tool (MCP)
```

- **I canali** sono i punti da cui un comando o una richiesta entrano nel sistema: la shell PowerShell
  locale, un bot Telegram, una chat AI dedicata. Ognuno parla lo stesso protocollo verso
  l'orchestratore.
- **L'orchestratore** è il "cervello": tiene il loop di conversazione con l'AI, decide quale
  comando eseguire (dietro conferma, mai in autonomia silenziosa), instrada l'output verso il
  canale o la finestra giusta. L'AI stessa è dietro un **adapter sostituibile** — cambiare
  provider (o modello) non richiede toccare il resto del sistema.
  Attualmente il progetto sceglie di puntare i propri modelli AI su **Claude di Anthropic** in via
  predefinita: non per un vincolo tecnico dell'adapter (che resta generico), ma perché nasce e si
  sviluppa come un caso di collaborazione diretta con quella famiglia di modelli.
- **Il server di tool** espone le capacità riusabili — eseguire un comando in una sessione,
  aprire un'applicazione nativa, cercare file, interrogare uno strumento esterno — attraverso il
  Model Context Protocol, così che tool diversi possano essere aggiunti senza toccare
  l'orchestratore.

## Il cambiamento centrale della seconda versione: essere la shell, non un suo ospite

La prima versione di Lare Terminal era un overlay: una finestra trasparente, richiamata a tasto,
che si sovrapponeva allo schermo come un cursore. Funzionava, ma restava fuori dalla shell vera —
chi voleva PowerShell "per davvero" doveva uscirne.

La domanda che ha aperto la seconda versione: **e se Lare fosse la shell, non un suo ospite?** La
risposta scelta non è un hook su una shell altrui né un client leggero che le parla da fuori, ma una
**host custom del motore PowerShell**, scritta in C# — esattamente il ruolo che `pwsh.exe` gioca
per quello stesso motore. Possiede il proprio REPL, legge l'input con PSReadLine (la stessa
libreria che dà a pwsh editing avanzato e storico), intercetta le righe `/…` prima che raggiungano
il runspace, esegue tutto il resto esattamente come farebbe un pwsh qualunque — stesso `cwd`,
stessi profili, stessi moduli.

Questa host vive dentro una **finestra Tauri** con un emulatore di terminale reale (xterm.js) che
la ospita via ConPTY (pseudo-terminale nativo di Windows) — non un widget grafico che finge un
terminale, ma un vero terminale con dentro un vero processo shell. Barre di stato e segnalini
vivono in HTML *intorno* all'area del terminale, mai sovrapposti al suo contenuto.

L'overlay della prima versione non è sopravvissuto a questo cambiamento: è stato rimosso, non
lasciato dormiente nel codice.

## Il protocollo: canali e ruoli

Un canale si connette all'orchestratore via WebSocket (solo su `127.0.0.1`, autenticato con un
token locale) e dichiara un **ruolo**: `shell` per una sessione di terminale, `ui` per il processo
che possiede le finestre secondarie (una sola istanza per macchina). Questa distinzione esiste
perché una sessione shell non ha, da sé, un posto dove disegnare una tabella o una spiegazione
lunga — quel compito spetta al processo con ruolo `ui`, verso cui l'orchestratore instrada ogni
messaggio "apri/aggiorna una finestra", mentre il testo di conferma resta nel terminale da cui è
partito il comando.

## Come nasce una risposta

Quando una riga inizia per `/ai "…"` (o la forma abbreviata `/ "…"`), l'host manda il comando
all'orchestratore, che apre subito una finestra di output con un segnaposto e avvia il ciclo di
conversazione con l'AI. Se l'AI propone di eseguire un comando, quel comando **non parte mai in
autonomia**: l'host mostra un prompt di conferma nel terminale, e solo un consenso esplicito lo fa
eseguire — nella stessa sessione shell, con lo stesso `cwd` che la sessione aveva in quel momento.
Il risultato torna all'AI, che può proseguire il turno (altri comandi, altre richieste di
conferma) fino alla risposta finale, che sostituisce il segnaposto nella finestra di output.

Questo gate di conferma è l'unico punto di autorizzazione di un comando proposto dall'AI, ed è
**non negoziabile**: vale per il canale locale così come per Telegram (dove, essendo un canale
remoto non fidato, si aggiunge anche un secondo fattore di autenticazione prima ancora di arrivare
al gate).

## Le finestre come superficie di risposta ricca

Non ogni risposta è una riga di testo. Una spiegazione lunga, una tabella, del codice formattato:
tutto questo va in una **finestra Markdown** dedicata, sanificata prima di essere renderizzata
(nessun HTML grezzo arriva mai dal contenuto generato). Alcune finestre sono **uniche per
macchina** — l'archivio dei documenti salvati, la configurazione, la chat AI indipendente dal
terminale — perché ha senso che restino un solo punto di riferimento anche con più sessioni di
terminale aperte contemporaneamente; altre, come l'output di un singolo comando, vivono e muoiono
con quel comando.

## Configurazione: una sola regola, sempre

Ogni binario del progetto risolve la propria cartella di configurazione in un solo modo:
l'argomento esplicito `--config-dir`, oppure — se assente — la cartella `Configuration\` accanto
al proprio eseguibile. **Nessuna variabile d'ambiente specifica del progetto** entra in questa
risoluzione: un binario figlio riceve sempre il percorso per argomento da chi lo avvia. La
motivazione non è astratta: nella prima versione del progetto, tre punti di lettura indipendenti
della stessa variabile d'ambiente erano arrivati a divergere in silenzio, ognuno convinto di
leggere lo stesso valore. Una sola via di risoluzione elimina la categoria di bug, non solo
l'istanza.

## Scelte tecnologiche, e perché non le alternative

- **Rust per tutto il backend, Tauri per l'interfaccia** — non per una convinzione ideologica sul
  linguaggio, ma per un vincolo concreto: il progetto punta a girare in modo equivalente su più
  sistemi operativi, e una codebase unica cross-platform garantisce quell'equivalenza per
  costruzione, invece di manutenerla a mano su implementazioni native separate per ogni piattaforma.
  Tauri sceglie un core nativo compilato con un footprint ridotto, a differenza di alternative
  basate su un motore browser completo incorporato.
- **La host della shell in C#**, non in Rust: usa direttamente `Microsoft.PowerShell.SDK`, lo
  stesso pacchetto su cui è costruito `pwsh.exe` — replicare quel comportamento (profili, policy di
  esecuzione, PSReadLine) partendo da un binding a basso livello sarebbe stato un lavoro enorme e
  fragile per un beneficio nullo.
- **Un motore vocale indipendente dalla webview** (pianificato, non ancora implementato): le API
  vocali native di ogni sistema operativo sono motori diversi tra loro, quindi non garantirebbero
  lo stesso comportamento multipiattaforma — la stessa logica dietro la scelta di Rust/Tauri.

## Sicurezza, in breve

- Il canale locale ascolta solo su `127.0.0.1`, mai su un'interfaccia di rete esterna, autenticato
  con un token generato localmente.
- Ogni comando che l'AI propone di eseguire passa dal gate di conferma esplicito descritto sopra —
  mai un'esecuzione silenziosa.
- Un canale remoto (Telegram) richiede un secondo fattore di autenticazione prima di poter persino
  arrivare al gate.
- Il contenuto Markdown mostrato nelle finestre è sempre sanificato prima del rendering.

## Per approfondire

- [`02-decisions.md`](./02-decisions.md) — ogni decisione qui riassunta, con le opzioni scartate e
  il ragionamento completo dietro ciascuna.
- [`03-stato-e-implementazione.md`](./03-stato-e-implementazione.md) — cosa di questa architettura
  è oggi realmente implementato e in uso, area per area.
