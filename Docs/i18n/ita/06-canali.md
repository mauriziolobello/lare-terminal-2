# Canali

Questo documento descrive i canali attraverso cui un comando o una richiesta rivolta all'AI possono
entrare in Lare Terminal, oltre alla shell locale già raccontata in
[`01-architettura.md`](./01-architettura.md) e [`03-stato-e-implementazione.md`](./03-stato-e-implementazione.md).
Nel modello a tre strati (canali → orchestratore → server di tool) ogni canale è un punto d'ingresso
diverso che parla lo stesso protocollo verso lo stesso orchestratore: cambia solo *da dove* arriva
la richiesta, non il principio con cui viene gestita una volta arrivata — in particolare, il gate di
conferma esplicito su ogni comando proposto dall'AI non è mai negoziabile, su nessun canale.

Oggi, oltre alla shell, esistono:

- **AI Chat** — una finestra di chat dedicata, che può mettere in comunicazione più macchine che
  eseguono Lare Terminal sulla stessa rete locale.
- **Telegram** — un bot che permette di impartire comandi da remoto.
- **I canali esterni** — integrazioni verso strumenti specifici (diagnostica di rete, analisi di
  mercati finanziari), ciascuna con un insieme fisso e predefinito di capacità.

## La shell locale, in breve

La shell locale è il canale predefinito ed è già descritta per intero in
[`01-architettura.md`](./01-architettura.md) (il protocollo, il gate di conferma, come nasce una
risposta) e in [`03-stato-e-implementazione.md`](./03-stato-e-implementazione.md) (host della shell,
finestra terminale, finestre di output). Qui basta un richiamo: gira solo su `127.0.0.1`, autenticata
con un token locale, e per questo è l'unico canale che l'AI può usare in autonomia relativa — dietro
al gate di conferma esplicito, ma senza un secondo fattore di autenticazione, perché il canale stesso
non è raggiungibile da fuori la macchina.

## AI Chat

`/aichat` apre una finestra dedicata (`aichat-window`), indipendente dal terminale — una delle
finestre con interfaccia tradotta in più lingue. È **singleton per macchina**: se la finestra è già
aperta, `/aichat` non ne apre una seconda ma porta il focus su quella esistente (stesso pattern di
`/config` e `/library`). Un'icona a busta segnala attività non vista mentre la finestra è chiusa.
L'apertura è sempre esplicita — nessuna auto-apertura all'avvio dell'applicazione.

### Non una chat con la sola AI locale: una stanza fra macchine

A differenza di quanto il nome potrebbe suggerire, AI Chat non è (solo) una finestra di
conversazione con l'AI della propria macchina: è un **canale di rete fra più installazioni di Lare
Terminal sulla stessa rete locale**. Il meccanismo:

- **Scoperta dei peer via UDP broadcast.** Ogni macchina con il servizio attivo annuncia
  periodicamente la propria presenza in broadcast sulla subnet; chi non si annuncia più per un
  tempo limite viene considerato uscito.
- **Elezione deterministica e "sticky" di un hub.** Fra i peer noti, vince l'IP più basso — ma chi
  è già hub lo resta finché è presente, senza prelazione da un IP più basso arrivato dopo (il
  cambio di hub in una topologia a stella ha un costo, quindi si minimizza quanto accade).
- **Relay a stella.** L'hub eletto inoltra i messaggi a tutti i peer collegati.
- **La rete trasporta solo testo e presenza, mai l'AI stessa.** Ogni macchina usa la propria AI
  locale (proprio provider, propria chiave): non esiste un'AI "centrale" della stanza.

Il servizio è **disattivato per default** e si configura dalla scheda "AI Chat" di `/config`
(nickname della macchina, se la propria AI partecipa quando invocata, se partecipa anche di
propria iniziativa).

### Chi parla: umani e AI, per invito

I partecipanti sono umani — uno per macchina collegata, ciascuno dalla propria finestra — e,
opzionalmente, l'AI di ciascuna macchina. Un'AI remota (di un'altra macchina) entra nella
conversazione attiva solo dopo un **passaggio di ammissione/consenso**: chi è già presente deve
accettare l'ingresso di un nuovo partecipante AI, non è automatico. Un'invocazione esplicita usa una
sintassi ad arroba: `@<etichetta>-ai` per una macchina specifica, `@all` per tutte; l'invocazione
della propria macchina, invece, agisce sempre — l'umano ha scritto nella propria finestra, il
consenso è implicito.

Oltre a rispondere quando invocata, l'AI può **auto-partecipare**: giudicare da sola, messaggio per
messaggio, se ha qualcosa di genuinamente utile da aggiungere, e in tal caso intervenire di propria
iniziativa; se non ha nulla da dire, risponde con un marcatore interno (mai mostrato) e resta in
silenzio. Un contatore limita i turni AI consecutivi, perché l'auto-partecipazione — a differenza
dell'invocazione esplicita — non è "sicura per costruzione" contro un botta-e-risposta fra AI di
macchine diverse.

Ogni AI partecipante può anche tenere una piccola **memoria persistente per macchina**, che scrive
o aggiorna di propria iniziativa (mai per estrazione automatica da un messaggio umano) tramite un
marcatore nella propria risposta, e che ritrova nelle conversazioni successive. Un documento salvato
nell'archivio (Library) può essere condiviso nella chat con il pulsante dedicato, verso una o più
macchine presenti.

### System prompt indipendente, e cosa NON può fare

AI Chat ha un proprio system prompt (due varianti — una per la risposta a un'invocazione esplicita,
una per l'auto-partecipazione), indipendente dal prompt generico usato da un turno `/ai` nel
terminale. Il prompt dice esplicitamente all'AI chi è (nome, provider, macchina), che è uno fra più
partecipanti (umani e AI, anche con nome simile al proprio — da non impersonare), e soprattutto:
**non può eseguire comandi né aprire finestre — risponde solo con testo semplice.** Non c'è un gate
di conferma da attraversare in AI Chat semplicemente perché non ci sono tool da proporre: la
superficie stessa è più piccola di quella di un turno `/ai`.

### Cosa distingue AI Chat da un turno `/ai`

Un turno `/ai` nel terminale è una richiesta singola, dentro una sessione di shell: l'AI può
proporre comandi (dietro conferma) che girano in quella sessione, con quel `cwd`, e la risposta
finale sostituisce un segnaposto in una finestra di output legata a quel turno. AI Chat è invece
una **conversazione persistente e condivisa**, potenzialmente fra macchine diverse, mai legata a un
singolo comando: non esegue nulla, non apre finestre di risultato, e il suo unico prodotto è testo
in una stanza a cui altri (umani o AI) possono rispondere in qualunque momento.

## Telegram

Il canale Telegram permette di impartire comandi da remoto tramite un bot dedicato. È l'unico
canale che riceve **input non fidato da remoto**: chiunque conosca il bot potrebbe scrivergli,
mentre il canale locale è già protetto dal fatto di girare solo su `127.0.0.1`. Da questa differenza
discende l'intero modello di sicurezza del canale (ADR-007).

### Attivazione e setup

Il canale è opzionale e si attiva creando `<cartella-di-configurazione>/telegramsettings.json` con
il token di un bot ottenuto tramite `@BotFather`; se il file manca, il canale semplicemente non
parte. Al primo avvio (nessun secret TOTP ancora presente), l'orchestratore stampa **una sola volta**
su stderr un QR a caratteri ANSI leggibile con Google Authenticator, più l'URI `otpauth://` in chiaro
come alternativa testuale — né il QR né l'URI vengono mai loggati o mostrati di nuovo. Lo stato TOTP
(secret + chat appaiata) resta persistito localmente.

### Il secondo fattore: pairing e login

Prima ancora di poter raggiungere il gate di conferma dei comandi, il canale richiede due passaggi
indipendenti:

1. **Pairing monouso** — `/pair <codice>` (codice mostrato solo al primo avvio, valido 10 minuti)
   associa una singola chat Telegram al bot. Fatto una volta, non va ripetuto: agli avvii successivi
   il codice di pairing non viene più stampato, e un tap da una chat diversa da quella appaiata è
   ignorato.
2. **Login TOTP** — `/login <codice-a-6-cifre>` apre una sessione di 30 minuti, tenuta solo in
   memoria (mai persistita): dopo ogni riavvio dell'orchestratore va rifatto.

Cinque tentativi falliti consecutivi (sia in pairing sia in login) causano un lockout di un minuto.

### Uso pratico e gate di conferma

Una volta autenticati, il canale accetta sia comandi diretti sia richieste in linguaggio naturale
verso l'AI, oltre a poche slash dedicate (`/open`, `/reset`). Qui il gate di conferma è **più
severo che in locale**: ogni comando OS, ogni `/open`/`/web`, e ogni singolo tool che l'AI propone
durante un turno (eccetto la sola scrittura di una finestra Markdown, che su questo canale non
esiste comunque) passa da bottoni inline `[Esegui]`/`[Annulla]` su Telegram, con un timeout di due
minuti — mentre in locale, di norma, l'AI esegue in autonomia dietro il solo prompt del terminale.
I comandi impartiti da Telegram non girano in una sandbox isolata per il canale remoto: passano
dallo stesso processo di tool condiviso (singolo per l'intero workspace) usato anche dalle
connessioni locali — stesso spazio di lavoro, non un ambiente separato per macchina remota.

Non esiste ancora un pulsante di annullamento per un comando già in corso su questo canale, né un
interruttore per la ricerca web (sempre disattivata su Telegram); le risposte, non avendo una
finestra dedicata dove stare, vengono spezzate in più messaggi quando superano la lunghezza massima
di un messaggio Telegram. Il canale non ha un proprio system prompt indipendente: usa lo stesso
prompt generico di un turno `/ai` nel terminale (meno, ovviamente, la possibilità di aprire
finestre), e risponde di default in italiano indipendentemente dalla lingua configurata per
l'interfaccia locale — vedi [`07-i18n.md`](./07-i18n.md) per il dettaglio di come si propaga (o non
si propaga) la lingua canale per canale.

## Canali esterni

I canali esterni sono integrazioni verso strumenti specifici. Il principio architetturale che li
governa è lo stesso per tutti, ed è deliberatamente rigido: **mai una shell arbitraria.** Ogni
canale esterno espone all'AI un **registro fisso e predefinito di tool** — mai un modo di eseguire
comandi generici — e ogni nuova capacità richiede scrivere un nuovo tool apposta, non allargare un
tool esistente perché "faccia anche quello".

Tecnicamente, un canale esterno è una connessione **isolata su tre assi**, tutti derivati dallo
stesso oggetto (`ToolClient`) che il canale fornisce:

- **Definizioni** — l'AI del canale vede *solo* i tool di quel canale, mai i tool generici
  (`run_in_session`, `open_target`, `show_markdown`) usati dal resto del sistema.
- **Dispaccio** — un nome di tool fuori dal menu del canale (anche se il modello lo invocasse per
  errore, magari confondendolo con un tool generico) viene rifiutato come sconosciuto, non eseguito.
- **Concorrenza** — una conferma in sospeso su un canale esterno non blocca il terminale, e
  viceversa: sono code indipendenti.

Ogni canale esterno ha anche il proprio system prompt indipendente, che elenca all'AI esattamente i
tool disponibili — così il modello non tenta di invocare qualcosa che, semplicemente, non ha.

Il registro tecnico contiene oggi cinque voci; solo due sono capacità rivolte all'utente con un
proprio slash dedicato e proprio scopo di prodotto — le altre tre sono infrastruttura o supporto ad
altre funzionalità, non nuovi "canali" da imparare a usare:

| Slash | id nel registro | Cosa è |
|---|---|---|
| `/netsec` | `netsec` | Diagnostica di rete — capacità di prodotto |
| `/markets` | `financial-markets` | Analisi mercati finanziari — capacità di prodotto |
| `/pyping` | `python-ping` | Canale di prova, valida l'infrastruttura dei tool Python — non una capacità |
| (nessuno slash) | `library-expand` | Usato dal pulsante "Espandi" su un documento già aperto in Library |
| (nessuno slash) | `config-market-data-test` | Usato dal pulsante "Test connessione" nella scheda Dati Mercato di `/config` |

### `/netsec` — diagnostica di rete

Il canale (rinominato da `/nmap` a `/netsec` in un aggiornamento recente — i nomi tecnici interni,
crate `mcp-nmap`, binario `mcp-nmap.exe`, i singoli tool `nmap_*`, non sono cambiati, solo ciò che
utente e AI vedono) è sostenuto da un crate Rust dedicato che avvolge il binario `nmap` reale più
alcuni comandi diagnostici nativi del sistema operativo. Espone oggi **otto tool fissi**:

- **Cinque scansioni nmap** — `nmap_quick_scan` (TCP connect rapido, nessun privilegio), `nmap_os_detect`
  (rilevamento del sistema operativo, richiede privilegi elevati via UAC di Windows e consenso
  esplicito), `nmap_version_scan` (`-sV`, versioni dei servizi in ascolto), `nmap_host_discovery`
  (`-sn`, quali host di una rete sono attivi, nessuna scansione porte), `nmap_vuln_scan` (verifica di
  vulnerabilità note tramite gli script NSE della sola categoria integrata `vuln` — mai script NSE
  arbitrari o personalizzati, per scelta esplicita, così l'AI non lascia intendere all'utente di poter
  eseguire NSE a piacere).
- **Due diagnostici locali built-in** — `local_network_info` (IP, subnet, gateway, ARP, routing
  della macchina locale — usato dall'AI per determinare un target quando l'utente non ne specifica
  uno) e `traceroute` (percorso di rete verso un host).
- **Un tool sullo stato del router domestico** — `fritzbox_status`, che legge (via la libreria
  Python `fritzconnection`, invocata come script Python one-shot, non un server persistente) il
  registro eventi recenti del router FRITZ!Box configurato, l'IP pubblico corrente e i dispositivi
  collegati alla rete locale. Richiede un file di configurazione dedicato
  (`Configuration/fritzbox.json`, mai committato) con host/utente/password del router — i dettagli
  sull'infrastruttura Python condivisa che questo tool usa sono in
  [`05-pytools.md`](./05-pytools.md).

Il system prompt del canale include una nota di onestà tecnica esplicita su questo ultimo tool: il
registro eventi di un router domestico **non è un sistema di rilevamento intrusioni**. Riporta
tipicamente login falliti e tentativi VPN, non il traffico bloccato dal firewall verso porte chiuse
(che un FRITZ!Box normalmente non registra affatto) — l'assenza di eventi nel log non va letta come
"nessun tentativo di accesso dall'esterno".

I cinque tool di scansione restano soggetti a conferma esplicita anche quando il canale gira in
locale — un'eccezione dichiarata alla regola per cui la UI locale è di norma autonoma sui propri
tool, perché uno scan ha effetti che escono dalla macchina stessa. Il banner di conferma mostra
target e tipo di scansione prima dell'esecuzione; per `nmap_os_detect`, che richiede elevazione, è
anche l'unico punto in cui l'utente vede il target, perché la finestra UAC di Windows non lo mostra.
I tre tool diagnostici (`local_network_info`, `traceroute`, `fritzbox_status`) non sono invece
soggetti a questa conferma aggiuntiva.

L'evoluzione di questo canale verso strumenti più ampi (scansione di rete più approfondita, verifica
TLS, cattura pacchetti) è un'idea discussa ma non ancora progettata.

### `/markets` — analisi mercati finanziari

Canale tool basato su un server MCP Python persistente (non uno script one-shot come
`fritzbox_status`: resta vivo fra una chiamata e l'altra, con un timeout di 300 secondi per singola
chiamata — necessario perché alcuni screening arricchiscono la selezione con decine di richieste
aggiuntive di dati fondamentali). Il sidecar Python viene avviato al primo uso del canale, come ogni
altro canale esterno.

Espone cinque tool: `search_ticker` (risolve un nome societario nel ticker USA corrispondente),
`stock_report` (report fattuale completo — fondamentale, grafici su cinque orizzonti temporali,
option chain, comparative), `list_stocks` (elenco tabellare dei titoli conosciuti, oggi filtrabile
solo per "USA"), `list_screeners` (elenca gli screening disponibili e apre una finestra di
selezione) e `run_screener` (esegue uno screening specifico). La fonte dei dati di mercato
(`yfinance` oggi) si configura dalla scheda "Dati Mercato" di `/config`, che offre anche un test di
connessione dedicato.

Per `stock_report` e `run_screener`, la finestra Markdown con il documento completo si apre in modo
**deterministico** — decisa dal sistema, non dall'AI — e viene completata a fine turno fondendo in
coda la parte scritta dall'AI: per `stock_report`, una "Narrativa di trend" e un'"Ipotesi di
investimento" (con punteggi separati su breve e medio termine); per `run_screener`, una sezione di
giudizio specifica dello screening eseguito. In entrambi i casi il prompt del canale chiude sempre
con un avviso esplicito: si tratta di un'analisi di fattori, non di una raccomandazione operativa.
Gli screening disponibili oggi applicano strategie diverse fra loro (adozione da parte dei
consumatori, analisi in stile equity-research su titoli a grande capitalizzazione, infrastruttura
per l'intelligenza artificiale, analisi tecnica quantitativa a doppio regime).

## Per approfondire

- [`00-apertura.md`](./00-apertura.md) — cos'è il progetto e perché esiste.
- [`01-architettura.md`](./01-architettura.md) — il modello a tre strati (canali → orchestratore →
  server di tool) e il gate di conferma, di cui ogni canale qui descritto è un'istanza.
- [`03-stato-e-implementazione.md`](./03-stato-e-implementazione.md) — cosa di questi canali è oggi
  realmente implementato e in uso.
- [`05-pytools.md`](./05-pytools.md) — gli script Python condivisi da alcuni tool dei canali esterni
  (`fritzbox_status`, il dominio `financial-markets`).

## Limiti dichiarati

- **Telegram**: nessun pulsante di annullamento per un comando già avviato; nessuna ricerca web;
  sessione di login solo in memoria, richiesta ad ogni riavvio dell'orchestratore; risponde sempre in
  italiano indipendentemente dalla lingua configurata per l'interfaccia locale.
- **AI Chat**: i partecipanti AI rispondono solo con testo semplice, mai comandi né finestre; i due
  system prompt del canale sono fissi in italiano, non seguono la lingua configurata
  dell'interfaccia; oggi esiste un solo provider AI attivo per l'intera applicazione, quindi ogni
  macchina partecipa alla stanza con una sola "voce" AI.
- **`/markets`**: solo titoli USA oggi (il filtro per paese esiste, ma "USA" è l'unico valore
  supportato).
- **`/netsec`**: l'evoluzione verso capacità più ampie di analisi di rete è un'idea discussa, non
  ancora progettata; `fritzbox_status` è specifico ai router FRITZ!Box (protocollo TR-064).
