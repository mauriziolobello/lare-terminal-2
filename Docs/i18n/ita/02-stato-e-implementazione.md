# Stato e implementazione

Se [`01-architettura.md`](./01-architettura.md) spiega come è fatto il sistema e perché, questo
documento fotografa **cosa esiste davvero, oggi**: componente per componente, cosa è completo,
cosa è debito noto dichiarato apertamente. È il documento da aggiornare a ogni release — riflette
lo stato del codice, non l'intenzione originaria.

## Componenti e versioni

| Componente | Cosa fa |
|---|---|
| `protocol` | Contratto dei messaggi WebSocket condivisi da tutti i componenti |
| `orchestrator` | Hub centrale: registro connessioni, router dei comandi, loop AI, gate di conferma |
| `lare-shell` (.NET/C#) | Host custom del motore PowerShell — non un crate Cargo |
| `ui` (Tauri) | Applicazione desktop: finestra terminale + ogni finestra secondaria |
| `mcp-server` | Server di tool MCP generico (shell persistente, apertura app, ricerca file) |
| `mcp-nmap` | Server di tool del canale `/netsec` (diagnostica di rete) |
| `startup-config` | Risoluzione condivisa di `--config-dir`/`startup.json` |
| `plugin-protocol` + `plugin-ping`/`plugin-calc`/`plugin-counter`/`plugin-lc`/`plugin-crypto` | Sistema di plugin sidecar — dettagli in [`03-plugin-system.md`](./03-plugin-system.md) |

Ogni componente tiene il proprio `CHANGELOG.md` (semver) e `IMPLEMENTATION.md` (dettaglio tecnico
corrente) accanto al codice — la fonte più aggiornata, per chi vuole scendere oltre questo
documento.

## Due modalità di esecuzione

- **Modalità A (predefinita)** — l'utente avvia direttamente l'applicazione; si apre una finestra
  terminale (xterm.js su pseudo-terminale nativo) con la host della shell come processo figlio.
- **Modalità B** — un profilo dedicato di Windows Terminal lancia la host della shell da sola,
  senza finestra grafica propria; le finestre di supporto (output, configurazione…) vengono
  avviate separatamente quando servono.

Le due modalità condividono un meccanismo di auto-riparazione reciproca: se l'orchestratore non è
raggiungibile, chi lo richiede lo avvia e ritenta per qualche secondo; se manca una finestra
pronta a ricevere un output, l'orchestratore la avvia a sua volta.

## Host della shell (C#)

Componente .NET dedicato, con la propria suite di test (xUnit). Possiede il runspace PowerShell,
carica PSReadLine dai moduli reali di pwsh, rispetta i profili utente (`$PROFILE`) nello stesso
ordine di un pwsh vero, gestisce cattura dell'output/exit code/cwd persistente, il gate `[Y/n]`
di conferma, la riconnessione on-demand verso l'orchestratore (nessun task di riconnessione in
background: ogni comando verifica e, se serve, ristabilisce la connessione).

**Limiti noti, dichiarati apertamente**: i profili `AllUsers` non sono caricati; il log della host
non ha rotazione; la variabile di stato `$?` nel prompt riflette solo l'ultimo comando digitato a
mano, mai l'esito di un turno AI appena concluso; un `exit` eseguito dall'AI dentro un comando
termina il processo come farebbe un `exit` digitato dall'utente. Il modulo che gestisce l'input
interattivo non ha una suite di test automatica (richiede una console reale) — è coperto solo da
verifica manuale end-to-end.

## Finestra terminale

Terminale reale (xterm.js sopra uno pseudo-terminale nativo, non un widget che lo simula), con
indicatori di attività ("AI al lavoro") alimentati da un canale di comunicazione diretto fra la
host e l'emulatore, indipendente dal protocollo verso l'orchestratore. Un flag dedicato consente
di avviare l'applicazione senza aprire questa finestra, quando serve solo per ospitare le finestre
secondarie.

## Finestre di output e risposte AI (`show_markdown`)

Il tool con cui l'AI produce risposte formattate — spiegazioni lunghe, tabelle, codice — dentro una
finestra dedicata. Punti implementativi rilevanti:

- **Una finestra per turno, aggiornata in place**: chiamate ripetute dello stesso tool nello
  stesso turno (tipico quando il modello affina una risposta con più ricerche) aggiornano la
  stessa finestra invece di aprirne di nuove.
- **Gate di chiusura**: se si prova a chiudere una finestra mentre il turno che la alimenta è
  ancora attivo, un overlay chiede conferma; confermare annulla davvero il turno sottostante, non
  lo lascia proseguire in background.
- **Badge di avanzamento** ("ricerca in corso…" / "completato") e orario dell'ultimo aggiornamento.
- **Pulizia del contenuto**: i marcatori di citazione grezzi che il modello a volte produce in
  testo libero (non come campo strutturato) vengono rimossi prima del rendering.
- **Salvataggio nell'archivio**: il bottone "Salva" resta sempre attivo — il primo click crea il
  documento, i click successivi sovrascrivono lo stesso file invece di duplicarlo; un lampeggio
  transitorio conferma l'azione senza lasciare uno stato stabile che possa sembrare "lavoro
  concluso" mentre la finestra continua ad aggiornarsi.
- **Copia rapida**: due azioni copiano il contenuto della finestra negli appunti, come testo
  semplice o come markdown sorgente.
- **Non incluso**: lo streaming del contenuto token per token — il testo arriva sempre intero al
  termine del turno, mai progressivamente.

## AI Chat e Telegram

Due canali aggiuntivi oltre alla shell locale. **AI Chat** è una finestra dedicata, indipendente
dal terminale, con un proprio prompt di sistema. **Telegram** permette di impartire comandi da
remoto tramite un bot; essendo un canale che riceve input non fidato da remoto, richiede un secondo
fattore di autenticazione (TOTP) prima di poter anche solo raggiungere il gate di conferma dei
comandi. Dettagli in [`05-canali.md`](./05-canali.md).

## Sistema di plugin

Processi separati (sidecar) con un protocollo proprio, ciascuno con la propria finestra Tauri:
una calcolatrice con modalità programmatore (basi numeriche, operazioni bit a bit), un plugin di
cifratura, e alcuni plugin più semplici usati anche come riferimento per chi vuole scriverne uno
nuovo. Dettagli e guida in [`03-plugin-system.md`](./03-plugin-system.md).

## Ricerca web nei turni AI

Un turno può usare la ricerca web, abilitata di default o per singola richiesta a seconda della
configurazione. È il meccanismo che alimenta le risposte più ricche mostrate da `show_markdown`,
incluso l'affinamento progressivo descritto sopra.

## Archivio (Library)

I documenti prodotti dall'AI e salvati restano consultabili in un archivio dedicato, con una
finestra propria per sfogliarli.

## Lingue dell'interfaccia

Italiano, inglese, spagnolo — sia per l'interfaccia sia per le risposte dell'AI nei turni. Dettagli
tecnici in [`06-i18n.md`](./06-i18n.md).

## Canali esterni

Integrazioni verso strumenti specifici, ciascuna con un set fisso e predefinito di capacità (mai
accesso generico al sistema): diagnostica di rete (inclusa la lettura dello stato del proprio
router domestico) e un canale per l'analisi di mercati finanziari. Dettagli in
[`05-canali.md`](./05-canali.md).

## Strumenti Python (pytools)

Alcuni canali esterni si appoggiano a script Python indipendenti, uno per dominio, ciascuno col
proprio ambiente virtuale. Dettagli in [`04-pytools.md`](./04-pytools.md).

## Sicurezza e gate di conferma

Nessun comando proposto dall'AI viene eseguito senza una conferma esplicita — vale per il canale
locale come per ogni altro canale. Un solo turno AI alla volta per connessione: una seconda
richiesta resta in coda invece di sovrapporsi. Test end-to-end del gate con un modello AI reale
(non uno stub) esistono ma restano esclusi dall'esecuzione automatica di routine, perché
richiedono una chiave API valida.

## Configurazione

Nessuna variabile d'ambiente specifica del progetto: ogni processo riceve il proprio percorso di
configurazione per argomento esplicito. Un registro dei provider AI disponibili permette di
selezionare quale usare; oggi è attivo un solo provider alla volta per l'intera applicazione — non
esiste ancora un modo per indirizzare una singola richiesta a un provider diverso da quello attivo.

## Build e distribuzione

Il layout di distribuzione vive dentro lo stesso repository (non un pacchetto separato), così da
poter essere verificato e copiato altrove senza modifiche. Dettagli operativi in
[`BUILD.md`](./BUILD.md), [`DEPLOY.md`](./DEPLOY.md), [`RUN.md`](./RUN.md).

## Verifica

Diverse migliaia di test automatici coprono il workspace Rust, la libreria JavaScript del
frontend e la host C# (xUnit) — tutti verdi all'ultima verifica. Una checklist di verifica end-to-end
manuale copre gli scenari che richiedono un'interazione reale con l'interfaccia grafica; il grosso
degli scenari principali è confermato, restano alcune voci minori non ancora ripassate dal vivo
(dettagli in [`TESTING-e2e.md`](./TESTING-e2e.md)).

## Debito noto e limiti dichiarati

Onestà tecnica: questo è ciò che **non** ci si deve aspettare funzioni oggi, o che è un limite
consapevole piuttosto che un bug non ancora notato.

- La finestra di output/markdown lato frontend non ha copertura di test a livello DOM — più di un
  bug in quest'area è stato trovato solo con verifica manuale.
- Non esiste ancora un modo per indirizzare una singola richiesta AI a un provider diverso da
  quello attivo, né gruppi di provider, né conversazioni fra più AI.
- Lo streaming del contenuto token per token nella finestra di output è deliberatamente fuori
  scope, non un'omissione.
- Un possibile scenario di riavvio dell'orchestratore può far partire una finestra applicativa
  duplicata invece di riusare quella esistente — bug noto, non ancora corretto, solo documentato.
- La directory di lavoro della sessione shell non è oggi inclusa nel contesto che l'AI riceve
  all'inizio di un turno.
- L'evoluzione dei canali di diagnostica di rete verso strumenti più ampi (scansione di rete più
  approfondita, verifica TLS, cattura pacchetti) è un'idea discussa ma non progettata.
- Un meccanismo per far interagire l'AI con applicazioni esterne tramite input simulato da
  tastiera esiste solo come script di test, non come funzionalità del prodotto.

Per il quadro completo e aggiornato, inclusi i debiti minori non elencati qui, i file
`IMPLEMENTATION.md` di ciascun componente restano la fonte più dettagliata.
