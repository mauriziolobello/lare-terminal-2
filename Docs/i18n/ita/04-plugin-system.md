# Sistema di plugin

Questo documento spiega il sistema di plugin di Lare Terminal: cos'è, come un plugin vive dentro
il programma, e come si scrive un plugin nuovo. Per una descrizione del progetto nel suo
complesso vedi [`00-apertura.md`](./00-apertura.md); per il quadro d'insieme dell'architettura,
[`01-architettura.md`](./01-architettura.md); per cosa esiste oggi, plugin per plugin,
[`03-stato-e-implementazione.md`](./03-stato-e-implementazione.md).

## Cos'è un plugin

Un plugin è un **processo separato** (un eseguibile a sé, non una libreria caricata dentro
l'orchestratore) che parla con l'orchestratore attraverso un protocollo proprio, JSON riga per
riga su stdin/stdout — internamente ci si riferisce a questo protocollo come "Contract P",
definito per intero nel crate `plugin-protocol`. Quando un plugin ha qualcosa da mostrare, apre
una **finestra Tauri dedicata**, distinta dal terminale e da ogni altra finestra
dell'applicazione.

Questo è deliberatamente il design più semplice che potesse funzionare: nessun sistema di plugin
dinamici caricati in-process (niente `dlopen`/DLL condivise, niente WASM), nessuna API interna
esposta a runtime. Un plugin è a tutti gli effetti un secondo programma, avviato e sorvegliato
dall'orchestratore, che comunica per messaggi.

**Perché un processo separato e non una libreria in-process.** La ragione è l'isolamento dei
guasti. Un plugin che va in panico, si blocca, o smette di rispondere non porta giù
l'orchestratore né gli altri plugin — resta un processo a sé, e la sua interruzione si osserva
come tale, non si propaga: l'handshake iniziale (`Init` → `Ready`) ha un timeout di 5 secondi,
oltre il quale l'orchestratore rinuncia e considera il plugin non disponibile; un invio fallito
verso un plugin già attivo (pipe rotta, processo terminato) rimuove il canale di scrittura verso
di lui, così che il prossimo comando che lo richiede lo faccia ripartire da zero invece di
continuare a bussare a un processo morto; alla chiusura dell'orchestratore, ogni processo plugin
viene comunque terminato (`kill_on_drop`), non lasciato orfano. Nessuna di queste garanzie
sarebbe possibile, allo stesso costo, con codice caricato nello stesso processo
dell'orchestratore.

Va detto con altrettanta chiarezza cosa questo isolamento **non è**: non è una sandbox di
sicurezza. Un plugin gira come processo nativo con gli stessi permessi dell'utente che ha avviato
il programma — non c'è un confine che gli impedisca di leggere file o fare rete, se il suo codice
lo facesse. L'unica cosa effettivamente contenuta è l'HTML che un plugin produce per la propria
finestra: passa attraverso DOMPurify lato host prima di essere renderizzato (stessa cautela
descritta in [`01-architettura.md`](./01-architettura.md) per le finestre Markdown dell'AI),
quindi uno script o un attributo pericoloso nell'HTML di un plugin non attraversa quel confine.
L'unico plugin di questo repository non pensato per un uso reale lo dichiara esplicitamente nel
proprio codice: **`crypto` implementa RSA "da manuale", senza padding OAEP e senza aritmetica a
tempo costante — uno strumento didattico, non adatto a cifrare qualcosa che conti davvero.**

## Sul filesystem: `plugins/<id>/`

Ogni plugin vive nella propria sottocartella dentro `plugins/`, con due soli file obbligatori:

```
plugins/
  <id>/
    plugin.json      -- manifest statico
    <id>(.exe)        -- binario del plugin (.exe su Windows, nessuna estensione altrove)
```

La **discovery**, eseguita dall'orchestratore a ogni avvio, scandisce questa cartella: legge
`plugin.json`, e se il manifest è valido cerca — per convenzione, senza bisogno di dichiararlo
altrove — un binario con lo stesso nome dell'`id` nella stessa cartella. Se manca il manifest, è
malformato, o il binario non esiste, quella sottocartella viene saltata con un avviso su stderr:
mai un errore fatale, un plugin rotto non impedisce agli altri di partire.

Il manifest è minimale:

```json
{
  "name": "Calcolatrice",
  "id": "calc",
  "version": "1.0.0",
  "protocol_version": 1,
  "triggers": { "command": "/calc" },
  "window": { "width": 960.0, "height": 620.0 }
}
```

| Campo | Obbligatorio | Significato |
|---|---|---|
| `name` | sì | Nome visualizzato (titolo di default della finestra). |
| `id` | sì | Identificatore stabile: nome della cartella, nome del binario, chiave di instradamento. |
| `version` | sì | Versione del *plugin* come prodotto — indipendente dalla versione del crate Rust che lo implementa. |
| `protocol_version` | sì | Versione del protocollo che il plugin parla, per una futura negoziazione. |
| `triggers.command` | no | Slash command che attiva il plugin (es. `/calc`). Assente di default. |
| `triggers.interval` | no | Cadenza di un timer (es. `"5m"`) — dichiarabile oggi, non ancora eseguito (vedi più sotto). |
| `window` | no | Dimensione iniziale della finestra. Assente → l'host usa un default generico (480×360). |

Campi non riconosciuti nel JSON sono ignorati silenziosamente (estensibilità in avanti); campi
mancanti fra quelli opzionali assumono un default innocuo — un manifest senza `triggers` equivale
a `triggers: {}`, uno senza `window` equivale a "nessuna preferenza di dimensione".

**L'`id` è vincolato per sicurezza**, non solo per convenzione: deve essere composto solo da
lettere/cifre ASCII, `_` e `-`. Il motivo è che il percorso del binario si costruisce componendo
la cartella dei plugin con l'`id` letto dal manifest, e due comportamenti della risoluzione dei
percorsi lo rendono pericoloso se non validato: un `id` con `..` risale fuori dalla cartella dei
plugin (normale risoluzione di un percorso relativo), mentre un `id` assoluto o con lettera di
drive (es. `C:/Windows/calc`) fa sì che unire i due percorsi **scarti il prefisso** e ritorni solo
il secondo — è così che si comporta `Path::join` in Rust, e non solo lì. In entrambi i casi il
"binario del plugin" trovato potrebbe essere un eseguibile arbitrario del sistema. La discovery
scarta l'intero plugin, prima ancora di cercare il binario, se l'`id` contiene un carattere fuori
da quell'insieme.

## Ciclo di vita

Un plugin che dichiara `triggers.interval` — o nessun trigger — viene avviato **eager**, subito
all'avvio dell'orchestratore. Un plugin che dichiara **solo** `triggers.command` viene avviato
**lazy**, al primo utilizzo: risparmia un processo per ogni plugin mai invocato in quella
sessione.

| `triggers` | Politica |
|---|---|
| `interval` presente | Eager — deve essere già vivo per ricevere un futuro `OnTimer` |
| solo `command` | Lazy — parte al primo slash command che lo richiede |
| nessuno dei due | Eager (caso degenere, per semplicità) |

Che sia eager o lazy, l'avvio segue sempre la stessa sequenza:

1. L'orchestratore avvia il processo (`Command::new(bin_path)`, con lo stesso argomento
   `--config-dir` che riceve ogni binario del progetto — vedi la "sola regola" di configurazione
   in [`01-architettura.md`](./01-architettura.md). Nessun plugin oggi legge quell'argomento, ma
   la convenzione vale comunque, senza eccezioni per i plugin, in vista del giorno in cui uno ne
   avrà bisogno).
2. Gli invia `Init { protocol_version, config, storage_dir }` — il primo messaggio, sempre.
3. Attende **`Ready`** come primissima risposta, entro **5 secondi**. Qualunque altro esito — un
   messaggio diverso, l'EOF, il timeout — e il plugin non entra in servizio: l'orchestratore lo
   segnala su stderr e prosegue con gli altri.
4. Da qui in poi il plugin è "attivo": l'orchestratore tiene un canale di scrittura verso di lui
   e un task dedicato ne legge l'output in loop, traducendo ogni messaggio verso la UI.

Quando arriva lo slash command dichiarato — una di quelle righe che iniziano per `/`, i *power
command* introdotti in [`00-apertura.md`](./00-apertura.md), riconosciuta con un confronto esatto
sulla stringa dopo trim, non un prefisso — l'orchestratore invia `Activate { window_id, args }`:
`window_id` è un contatore progressivo mai riusato, `args` porta oggi sempre `null` (un comando
slash non passa ancora parametri al plugin). Il plugin risponde con `ShowWindow`, e da quel
momento ogni interazione dell'utente nella finestra (click, digitazione) arriva come `UiEvent`, a
cui il plugin risponde con `UpdateWindow` — un rimpiazzo completo dell'HTML della finestra, non
una patch parziale (vedi sotto il perché).

Due dettagli contano per chi scrive un plugin:

- **Chiudere la finestra non ferma il plugin.** L'utente che preme la ✕ della finestra fa sì che
  l'host dimentichi quella finestra (il suo `window_id` non viene più instradato), ma il processo
  del plugin resta vivo con tutto il suo stato — riaprirlo (stesso slash command) può mostrare
  uno stato che il plugin ha ricordato dall'ultima volta, se lo ha voluto (`lc` lo fa
  esplicitamente, salvando i percorsi dei due pannelli nel proprio storage a ogni navigazione).
- **`Deinit` arriva solo allo spegnimento dell'orchestratore**, mai alla chiusura di una singola
  finestra. È l'unico segnale di shutdown "pulito" che un plugin riceve; un plugin che vuole
  salvare qualcosa in modo affidabile non può contare solo su quel momento, e conviene che salvi
  a ogni cambiamento di stato rilevante — di nuovo l'approccio di `lc`, che non si fida di
  ricevere sempre un `Deinit` per ogni singola sessione d'uso.
- Un plugin **può aprire più di una finestra di propria iniziativa**, non solo quella
  dell'`Activate`, inventando i propri `window_id` (tipicamente con un offset che non collide con
  quelli assegnati dall'host). `crypto` lo fa per la dialog dei parametri di un cifrario: una
  vera seconda finestra Tauri, indipendente dalla principale e spostabile a parte.

## Il protocollo

Il protocollo — due enum JSON "taggati" condivisi dal crate `plugin-protocol` — è deliberatamente
piccolo e additivo: ogni nuovo tipo di messaggio si aggiunge in coda, senza rompere i plugin
esistenti che non lo conoscono ancora. Ogni messaggio è una riga JSON con un campo `"type"` che
ne indica la variante.

**Host → plugin:**

| Messaggio | Quando | Forma |
|---|---|---|
| `Init` | Sempre il primo, dopo lo spawn | `{"type":"Init","protocol_version":1,"config":null,"storage_dir":"…/Configuration/plugin-storage/calc"}` |
| `Activate` | Slash command riconosciuto | `{"type":"Activate","window_id":7,"args":null}` |
| `UiEvent` | Click o input nella finestra | `{"type":"UiEvent","window_id":7,"element_id":"inc","value":null}` |
| `Deinit` | Shutdown dell'orchestratore | `{"type":"Deinit"}` |

**Plugin → host:**

| Messaggio | Quando | Forma |
|---|---|---|
| `Ready` | Risposta obbligatoria a `Init` | `{"type":"Ready","name":"calc","protocol_version":1}` |
| `ShowWindow` | In risposta ad `Activate` (o di iniziativa) | `{"type":"ShowWindow","window_id":7,"title":"Calcolatrice","html":"<div>…</div>"}` |
| `UpdateWindow` | In risposta a un `UiEvent` | `{"type":"UpdateWindow","window_id":7,"html":"<div>…</div>"}` |
| `CloseWindow` | Quando il plugin non ha più nulla da mostrare | `{"type":"CloseWindow","window_id":7}` |
| `Log` | Diagnostica libera | `{"type":"Log","level":"info","msg":"…"}` (ricevuto ma non ancora esposto in UI) |

`UiEvent.element_id` corrisponde all'attributo `data-evt` dell'elemento HTML cliccato (vedi la
sezione successiva); `value` è `Some(...)` per un `<input>`/`<textarea>`/`<select>` (il valore
corrente del campo), `None` per un pulsante. `ShowWindow`/`UpdateWindow` portano sempre l'**intero**
HTML della finestra — "full-HTML replace", non un albero di patch — perché è la strategia più
semplice da implementare correttamente in un plugin, e la più prevedibile da leggere: chi scrive
`render_window(&state) -> String` non deve mai ragionare su cosa è cambiato rispetto al render
precedente, solo su cosa deve esserci ora.

## La finestra: cosa può contenere un plugin

Una finestra plugin non è una webview qualunque: è un contenitore condiviso con regole precise,
pensate perché un plugin non debba reinventarle.

- **CSS condiviso.** `plugin-catalog.css`, iniettato automaticamente in ogni finestra plugin,
  definisce classi comuni (`lare-window`, `lare-label`, `lare-button`, `lare-input`…) coerenti
  col tema scuro e con la trasparenza configurabile dell'intera applicazione (`--window-alpha`,
  la stessa variabile che governa ogni altra finestra). Usarle è una scelta, non un obbligo — ma
  il costo di ignorarle è reale: `lc` è nato con una propria convenzione di colori CSS
  (esadecimali pienamente opachi, invece dei `rgba()` translucidi usati ovunque nel resto del
  programma), e ha dovuto essere riallineato in un giro di lavoro successivo, regola per regola.
- **Eventi via attributi `data-*`, non JavaScript del plugin.** Un plugin non porta codice
  JavaScript proprio: interagisce con l'utente dichiarando `data-evt="qualcosa"` sugli elementi
  HTML che genera. Il runtime host cattura click (`data-evt`), doppio click (`data-dblevt`, utile
  per "seleziona ed entra" in un elenco) e tasti fisici mappati su un pulsante (`data-key`, letto
  a ogni `keydown`), e li traduce tutti nello stesso `UiEvent` verso il plugin. Un `<input>`,
  `<textarea>` o `<select>` con `data-evt` è inoltre "value-bearing": ogni tasto premuto al suo
  interno invia subito un `UiEvent` con il valore corrente — è così che un plugin può reagire
  alla digitazione live, non solo a una submit esplicita.
- **Sanificazione a due livelli.** L'HTML che un plugin produce passa da DOMPurify lato host
  prima del render (niente `<script>`, niente handler inline) — ma un plugin prudente sanifica
  anche lato proprio, a livello di stringa, qualunque testo dell'utente prima di iniettarlo nel
  proprio HTML (tutti i plugin di questo repository lo fanno, con un piccolo escape manuale di
  `&`/`<`/`>`/`"`). Sono due livelli indipendenti: nessuno dei due sostituisce l'altro.
- **Dimensione e ridimensionamento.** Per default la finestra si adatta automaticamente
  all'altezza del proprio contenuto dopo ogni render (utile per un plugin che cresce o si
  restringe, come la calcolatrice quando passa da una riga a una frazione 2D). Un plugin che
  preferisce un'altezza scelta liberamente dall'utente — un file manager con pannelli scorrevoli,
  per esempio — marca il proprio elemento radice con l'attributo `data-no-autofit` per uscire da
  quel comportamento.
- **Il focus e il valore di un campo attivo sopravvivono a un `UpdateWindow`**, anche se
  "full-HTML replace" ricrea ogni elemento del DOM da zero: l'host cattura cosa l'utente sta
  scrivendo un istante prima di sostituire l'HTML, e lo ripristina sul nuovo elemento
  corrispondente. Senza questa cautela, digitare in un campo di testo di un plugin sarebbe quasi
  impossibile (un carattere alla volta che appare e sparisce) — un problema reale osservato
  durante un test dal vivo di `crypto` e risolto una volta sola, a livello di piattaforma, non
  plugin per plugin.

## I plugin di oggi

Cinque plugin vivono oggi in questo repository. Non è un elenco chiuso — è la base a cui guardare
per capire cosa un plugin può fare, dal più semplice al più ricco.

- **`ping`** — il plugin di riferimento minimo: risponde `Ready` all'`Init` e nient'altro, non
  apre mai una finestra. Esiste per validare la catena discovery → spawn → handshake, ed è anche
  la sonda usata dal comando diagnostico `/ping` (built-in dell'orchestratore, distinto dal
  plugin: misura il tempo `Init → Ready` di un'istanza usa-e-getta e lo riporta insieme allo
  stato degli altri strati del programma).
- **`counter`** — il più semplice plugin **con** finestra: un contatore e un pulsante "+1". Serve
  da riferimento didattico per il ciclo `Activate → ShowWindow`, `UiEvent → UpdateWindow`.
- **`calc`** — una calcolatrice completa: aritmetica, funzioni scientifiche (trigonometria,
  logaritmi, potenze, radici, fattoriale), e una **modalità programmatore** con basi numeriche
  (decimale/esadecimale/ottale/binaria), larghezza di bit configurabile e operatori bit a bit. È
  il plugin più ricco funzionalmente, buon esempio di quanto stato interno e quanta logica un
  plugin possa avere pur restando un processo sidecar ordinario.
- **`lc`** ("Lare Commander") — un file manager a doppio pannello in stile Midnight Commander:
  copia, sposta, crea cartelle, elimina, confronta cartelle e fa il diff fra due file. Usa la
  propria `storage_dir` per ricordare, fra un riavvio e l'altro, l'ultimo percorso aperto in
  ciascun pannello — l'esempio più concreto di persistenza per-plugin nel repository.
- **`crypto`** — cifrari classici (Cesare, Vigenère) e RSA, come strumento di studio: come già
  detto sopra, la sua implementazione RSA è volutamente priva delle protezioni che servirebbero
  per un uso reale.

## Scrivere un plugin nuovo

Il protocollo in sé non impone un linguaggio: è JSON riga per riga su stdin/stdout, leggibile e
scrivibile da qualunque runtime capace di I/O standard. In pratica, oggi, **ogni plugin del
repository è scritto in Rust** e condivide il crate `plugin-protocol` per i tipi dei messaggi —
è il percorso verificato, con test, esempi funzionanti e un posto naturale nel workspace Cargo.
Un plugin in un altro linguaggio è possibile per costruzione (la discovery si limita a cercare un
eseguibile nativo chiamato `<id>` o `<id>.exe` nella cartella del plugin — non verifica come è
stato prodotto), ma è una strada non ancora percorsa in questo progetto: darla per scontata senza
averla mai messa alla prova sarebbe ottimismo, non un fatto verificato.

Il resto di questa sezione usa **`ping`** come riferimento — il plugin più piccolo che esiste,
l'intero contenuto utile sta in poche righe.

### 1. Il crate

```
crates/plugin-<id>/
  Cargo.toml         -- [[bin]] name = "<id>"; dipendenza da plugin-protocol (+ serde_json)
  plugin.json          -- manifest (vedi sopra)
  src/main.rs           -- loop I/O + logica
  IMPLEMENTATION.md     -- (convenzione del repository) dettaglio tecnico del plugin
```

Il nuovo crate va aggiunto sia a `members` sia a `default-members` nel `Cargo.toml` di workspace,
alla radice del repository — altrimenti `cargo build`/`cargo test` senza `-p` non lo toccano.

### 2. La logica: una funzione pura, non un loop pieno di I/O

Ogni plugin di questo repository separa la logica (testabile, senza I/O) dal loop che legge
stdin e scrive stdout. Per `ping`, l'intera logica è:

```rust
fn handle(msg: HostToPlugin) -> Vec<PluginToHost> {
    match msg {
        HostToPlugin::Init { .. } => {
            vec![PluginToHost::Ready { name: "ping".into(), protocol_version: 1 }]
        }
        HostToPlugin::Deinit {} => Vec::new(),
        HostToPlugin::Activate { .. } | HostToPlugin::UiEvent { .. } => Vec::new(),
    }
}
```

`handle` prende un messaggio in ingresso e ritorna zero o più risposte — nessuno stato, nessun
I/O, testabile con un semplice `assert_eq!`. Un plugin con stato (`counter`, `calc`, …) passa
invece `&mut StatoDelPlugin` come primo argomento, ma la forma resta la stessa: input puro,
output puro.

Il loop attorno a `handle` è sempre la stessa manciata di righe: legge una riga da stdin, la
deserializza in `HostToPlugin`, chiama `handle`, scrive ogni risposta come una riga JSON su
stdout con `flush` esplicito, ed esce dal ciclo — terminando il processo — dopo aver processato
`Deinit`. Non serve un runtime asincrono: un plugin stdio è single-task per natura, un loop
bloccante su `stdin.lock().lines()` basta.

### 3. Il manifest

```json
{ "name": "<Nome visualizzato>", "id": "<id>", "version": "1.0.0", "protocol_version": 1, "triggers": {} }
```

`triggers: {}` (o assente) rende il plugin eager — utile in sviluppo, per vederlo partire subito
senza bisogno di uno slash command. Un plugin pensato per essere invocato su richiesta dichiara
invece `triggers.command`.

### 4. Compilare, distribuire, registrare

```powershell
cargo build -p <id>                       # target\debug\<id>.exe
```

Perché l'orchestratore lo trovi, il binario compilato e il manifest devono finire insieme sotto
`plugins/<id>/` nella cartella di deploy (`Test Run\` durante lo sviluppo — vedi
[`BUILD.md`](./BUILD.md) e [`DEPLOY.md`](./DEPLOY.md)). Lo script `deploy_test_run.ps1
-IncludePlugins` automatizza questa copia, ma tiene un elenco esplicito di id noti (`ping`,
`calc`, `counter`, `crypto`, `lc`) — un plugin nuovo va aggiunto a quell'elenco, o copiato a mano,
finché lo script non viene esteso.

Non c'è ricaricamento a caldo: **l'orchestratore va riavviato** dopo aver aggiunto o ricompilato
un plugin, esattamente come per ogni altra modifica al backend.

### 5. Verificare che sia stato trovato

La tab "Plugins" della finestra Library elenca ogni cartella sotto `plugins/` che contiene un
`plugin.json` leggibile — anche uno malformato, mostrato "com'è" invece che nascosto, proprio
perché è un pannello diagnostico: se il plugin non compare lì, il problema è a monte
dell'esecuzione (cartella sbagliata, JSON non valido). Se compare lì ma non parte quando
invocato, il problema è nell'handshake o nel binario stesso.

### 6. Testare

- **Unit test su `handle`** (o sull'equivalente con stato): lo stile di tutti i plugin esistenti,
  nessun processo reale coinvolto — `cargo test -p <id>`.
- **Un giro manuale via stdin**, senza passare dall'orchestratore:
  ```powershell
  echo '{"type":"Init","protocol_version":1,"config":null,"storage_dir":"C:/tmp"}' | .\target\debug\<id>.exe
  ```
  ci si aspetta una riga `{"type":"Ready",...}` su stdout.
- **Un test end-to-end reale**, con la discovery e l'host veri (`orchestrator/tests/plugin_e2e.rs`
  ne è l'esempio per `ping`): compila il binario, allestisce una `plugins/<id>/` temporanea, lo
  fa scoprire e avviare da `PluginHost`, verifica `Init → Ready → Deinit`. Escluso dall'esecuzione
  di routine (richiede il binario già compilato) — si lancia esplicitamente con
  `cargo test -p orchestrator --test plugin_e2e -- --ignored`.

## Debito noto e limiti dichiarati

Onestà tecnica, per non promettere più di quanto il codice faccia oggi:

- **`triggers.interval` è dichiarabile ma non eseguito.** Il manifest accetta un intervallo, la
  spawn policy lo tratta correttamente (eager), ma non esiste ancora un ciclo `OnTimer` che lo
  faccia scattare — è un tipo di messaggio previsto nel protocollo, non ancora implementato.
- **`Init.config` è sempre `null`.** Il canale per passare configurazione utente a un plugin
  esiste nel messaggio, ma nessun percorso reale la popola ancora.
- **`Activate.args` è sempre `null`.** Uno slash command non porta ancora parametri al plugin che
  attiva — solo il fatto che è stato invocato.
- **`Log` arriva all'host ma non arriva da nessuna parte visibile.** Il messaggio è ricevuto e
  scartato: non c'è ancora un pannello o un file dedicato a mostrarlo.
- **`storage_dir` non viene creata dall'host.** Un plugin che vuole scriverci deve fare
  `create_dir_all` da sé, sempre — l'host garantisce solo il percorso, non che esista.
- **Nessuna negoziazione reale di `protocol_version`.** Il campo è nel manifest e nei messaggi,
  pronto per l'uso, ma oggi l'unica versione esistente è `1`, e l'host non rifiuta ancora un
  plugin che ne dichiarasse una diversa.

## Per approfondire

- [`03-stato-e-implementazione.md`](./03-stato-e-implementazione.md) — cosa di questo sistema è
  oggi realmente in uso, insieme al resto del programma.
- `crates/plugin-protocol/IMPLEMENTATION.md` e i file `IMPLEMENTATION.md` di ciascun
  `crates/plugin-<id>/` — il dettaglio tecnico corrente, aggiornato a ogni cambiamento del
  singolo plugin.
