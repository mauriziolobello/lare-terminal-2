# Briefing per AI esterne — leggimi PRIMA di scrivere una sola riga di codice

Questo documento è il primo file che devi leggere, per intero, prima di iniziare qualunque
compito che ti è stato assegnato su **Lare Terminal 2.0**. Non è facoltativo e non è un
riassunto: contiene le regole vincolanti del progetto ed è pensato per essere **autosufficiente**
— anche se lo stai leggendo incollato in una chat, senza accesso al resto del repository, contiene
già tutto quello che serve per lavorare secondo le regole del progetto. Se il compito che ti è
stato dato in un messaggio separato sembra in conflitto con qualcosa scritto qui, questo documento
vince — segnala il conflitto nel tuo report finale (sezione 9) invece di ignorarlo silenziosamente.

## TL;DR (se riesci a leggere solo questo)

1. Lavora sempre su un branch dedicato, mai su `main`.
2. TDD vero: test che fallisce PRIMA del codice, eseguito davvero, non a memoria.
3. Commenti prodighi, didattici, **in italiano**.
4. `CHANGELOG.md` + `IMPLEMENTATION.md` del crate + `HANDOFF.md` aggiornati nello **stesso commit**
   del codice.
5. Resta dentro lo scope del compito assegnato — niente "già che c'ero".
6. Riporta SOLO risultati che hai davvero verificato tu; se non hai potuto verificare qualcosa,
   dillo esplicitamente.
7. Il tuo lavoro non è definitivo finché un supervisore non lo approva.

Il resto del documento spiega ciascuno di questi punti nel dettaglio e copre i casi che il TL;DR
non può coprire — leggilo comunque per intero se puoi.

## 0. Chi sei, in questo processo

Non sei l'ultima parola su questo codice. Il flusso di lavoro con cui Maurizio (il proprietario
del progetto) sta operando è:

1. Maurizio ti assegna UN compito delimitato (un bug, una feature piccola, un refactoring),
   insieme al materiale di contesto necessario (vedi l'ultima sezione di questo documento, rivolta
   a lui).
2. Tu lo implementi seguendo questo documento, fino in fondo, per quanto le tue capacità (shell,
   git, esecuzione di test) lo permettono.
3. **Un supervisore (un'altra AI, Claude, che opera per conto di Maurizio) rivede il tuo lavoro
   prima che venga considerato definitivo.** Può correggerlo, respingerlo in parte, o chiederne
   la revisione. Il tuo lavoro non è "finito" quando tu dici che lo è: è finito quando il
   supervisore lo approva.

Questo significa concretamente:

- **Non lavori mai su `main`.** Crea sempre un branch dedicato al tuo compito (sezione 6).
- **I tuoi commit sono materiale di lavoro, non la parola definitiva.** Committa spesso, in
  passi piccoli e leggibili — è quello che rende possibile la revisione, non un problema da
  evitare.
- **Onestà sui risultati vale più della velocità.** Se qualcosa non hai potuto verificarlo
  davvero (un test non gira, un comando fallisce, un ambiente ti manca), DEVI dirlo esplicitamente
  nel report finale. Dichiarare "fatto" o "testato" senza averlo davvero verificato è l'errore più
  costoso che puoi fare in questo flusso: il supervisore si fida di quello che scrivi, e se scopre
  dal vivo che un test non passava mentre tu avevi scritto "tutti i test passano", quella bugia
  costa più tempo di qualunque bug.

## 1. Prima cosa da stabilire: che ambiente hai davvero?

Prima di seguire il resto di questo documento, capisci quali di queste tre cose hai a
disposizione, perché cambiano come lavori:

- **Accesso al repository + shell + git** (il caso ideale: puoi leggere file, eseguire comandi,
  fare commit). Segui tutte le sezioni seguenti alla lettera.
- **Accesso al repository ma NON a una shell/git** (puoi leggere e scrivere file, ma non eseguire
  comandi). Fai lo stesso lavoro di lettura/scrittura file descritto sotto; per tutto ciò che
  richiederebbe un comando (build, test, commit), scrivi nel report finale (sezione 9) esattamente
  quali comandi il supervisore deve eseguire per verificare, e non dichiarare mai un test "passato"
  che non hai potuto far girare.
- **Nessun accesso al repository, solo questa chat** (il compito e il contesto ti sono stati
  incollati in un messaggio). In questo caso:
  - Non puoi leggere `CLAUDE.md` o gli altri file del progetto oltre a quello che ti è stato
    incollato — le sezioni 2 e 3 di QUESTO documento contengono già il nucleo di quelle regole,
    proprio per coprire questo caso. Se Maurizio non ti ha incollato altro contesto (codice
    esistente, file da modificare), il tuo output sarà necessariamente più povero — dillo nel
    report finale invece di far finta di avere il quadro completo.
  - Produci il codice per intero (contenuto completo dei file che cambi, non frammenti o diff a
    parole), così chi ha accesso al repository può applicarlo meccanicamente.
  - Scrivi comunque il report finale (sezione 9) come ultimo messaggio, sotto l'intestazione
    `## Report per il supervisore`.

Se hai accesso al repository, leggi anche `CLAUDE.md` alla radice (contiene le stesse regole di
questo documento, in forma più compatta, scritte per un altro assistente — ma valide anche per
te) e i file elencati in sezione 1bis. Se non hai accesso, non è un problema: le sezioni 2 e 3 qui
sotto bastano per lavorare secondo le regole del progetto.

## 1bis. Se hai accesso al repository: cosa leggere, in ordine, prima di scrivere codice

1. **`Docs/i18n/ita/HANDOFF.md`** — lo stato corrente del progetto: cosa è fatto, cosa manca, le
   versioni dei crate. Leggilo per capire dove si inserisce il tuo compito e cosa esiste già che
   potresti riusare invece di reinventare.
2. **`Docs/i18n/ita/06-decisions.md`** — il log delle decisioni architetturali (ADR). Se il tuo
   compito tocca un'area con una decisione già presa lì, quella decisione è vincolante: non la
   ridiscuti autonomamente, la rispetti (se pensi che vada cambiata, scrivilo nel report finale
   come proposta, non come fatto compiuto).
3. **`Docs/i18n/ita/KNOWN-ISSUES.md`** — problemi noti già catalogati. Se il tuo compito è
   toccato da uno di questi, non stupirti e non provare a "risolverlo di striscio" se non è quello
   che ti è stato chiesto: resta nello scope del tuo compito (sezione 8).
4. Se il tuo compito riguarda **compilare, distribuire o avviare** il programma:
   `Docs/i18n/ita/BUILD.md`, `Docs/i18n/ita/DEPLOY.md`, `Docs/i18n/ita/RUN.md` — tre file
   separati, ciascuno con un solo compito (compilazione, popolare la cartella di deploy,
   esecuzione). Non confonderli.
5. Se il tuo compito riguarda **verifica end-to-end**: `Docs/i18n/ita/TESTING-e2e.md`.
6. Se ti è stato indicato uno **spec** o un **piano** specifico (percorsi tipo
   `Docs/i18n/ita/superpowers/specs/AAAA-MM-GG-....md` o
   `Docs/i18n/ita/superpowers/plans/AAAA-MM-GG-....md`): leggilo per intero. È la fonte di verità
   del "perché" per quel pezzo di lavoro — il compito che ti è stato dato ne è un estratto, non un
   sostituto.
7. **Il codice esistente dell'area che tocchi.** Prima di scrivere una riga nuova, leggi il file
   che stai per modificare per intero (non solo la funzione bersaglio) e cerca pattern analoghi
   già usati altrove nel crate — questo progetto ha convenzioni consolidate (vedi sezione 3) e un
   codice nuovo che le ignora viene respinto in revisione, anche se "funziona".
8. **Almeno un test esistente dello stesso crate**, come riferimento di stile per come sono
   scritti i test in questo progetto (struttura, nomi delle funzioni di test in italiano
   descrittivo, uso di `tempfile::tempdir()` per path fittizi — vedi sezione 3).

Se uno di questi file non esiste più o è stato spostato, non bloccarti: cercalo con il tuo
strumento di ricerca file (`find`/`grep`/equivalente) prima di assumere che l'informazione non
esista.

## 2. Cos'è Lare Terminal 2.0 — architettura in breve

Terminale simile a PowerShell con "power command" `/…` e AI integrata. Tre livelli:

```
Canali (host C# "lare-shell" via WebSocket, Telegram in-process)
  → orchestrator (demone Rust: WS + router + AI + plugin host)
    → mcp-server (processo Rust separato, stdio; possiede una shell reale solo per Telegram/AI Chat)
ui.exe (Tauri, Rust + JS) = finestra terminale + host di finestre secondarie (Markdown, /config, /library, plugin, /find)
```

Crate Rust in `crates/`: `protocol`, `startup-config`, `mcp-server`, `mcp-nmap`, `orchestrator`,
`plugin-protocol`, `plugin-*` (plugin sidecar), `ui` (in `crates/ui/src-tauri`, frontend JS puro
in `crates/ui/frontend`). Host C# in `shell/lare-shell/` (soluzione .NET separata, non nel
workspace Cargo). Tool Python in `scripts/pytools/<dominio>/` (un virtualenv per dominio, mai
committato). Piattaforma target di sviluppo: **Windows** — codice specifico di piattaforma è
normale e atteso (vedi la regola `cfg(windows)` in sezione 3).

**Configurazione — regola unica (decisione D6, non derogabile):** ogni binario riceve
`--config-dir <path>` come argomento esplicito, altrimenti usa `<cartella dell'eseguibile>\Configuration\`
come default. **Nessun binario legge variabili d'ambiente `LARE_*`** (né `LOCALAPPDATA`/`APPDATA`).
Se stai per scrivere `std::env::var(...)` o equivalente per leggere qualcosa di specifico di Lare
Terminal, ti stai sbagliando: cerca come `--config-dir` viene già passato e propagato nel crate
che stai toccando, e fai lo stesso.

**Sicurezza — anche questa non derogabile:** il WebSocket dell'orchestratore ascolta SOLO su
`127.0.0.1` (mai `0.0.0.0`), autenticato con un token su file. Ogni comando proposto dall'AI
integrata passa da un gate di conferma esplicito prima di essere eseguito (mai eseguito
silenziosamente). Se il tuo compito tocca uno di questi due punti, la conferma esplicita non è
opzionale né bypassabile "per semplicità" o "per velocizzare i test".

## 3. Convenzioni di codice — non derogabili

Queste regole valgono per QUALUNQUE linguaggio nel repository (Rust, C#, JavaScript, Python),
salvo dove esplicitamente detto il contrario:

- **TDD genuino.** Per ogni comportamento nuovo o ogni bug fix: prima scrivi il test che fallisce
  per il motivo giusto (RED — eseguilo davvero e guarda che fallisca, non assumerlo), poi il
  codice minimo che lo fa passare (GREEN — eseguilo davvero e guarda che passi), poi eventualmente
  rifinisci senza rompere niente (REFACTOR — riesegui i test dopo ogni rifinitura). Un test scritto
  DOPO il codice per "documentarlo" non è TDD e non soddisfa questa regola — se ti trovi a scrivere
  prima il codice, fermati e riparti dal test.
- **Composizione preferita a ereditarietà.** In Rust questo si traduce in trait come confini
  d'astrazione e composizione di struct, non gerarchie profonde. In C# ed eventuale altro codice
  OOP, stesso principio: preferisci interfacce iniettate a classi base con override.
- **SOLID applicato con giudizio, non come dogma decorativo.** Una funzione con una sola
  responsabilità chiara è più importante di un pattern con un nome elegante.
- **Commenti prodighi e didattici, in italiano.** Questo progetto usa i commenti anche per
  insegnare i costrutti del linguaggio a chi legge (l'utente sta imparando Rust partendo da basi
  OOP: quando puoi, lega i costrutti Rust — trait, lifetime, `Arc`, pattern matching — a concetti
  OOP noti). Non è "commenta solo la logica complessa": è un livello di commento più alto del
  default che probabilmente useresti in altri contesti. Stesso livello di commento in C# e JS.
  I commenti sono in ITALIANO, non in inglese, indipendentemente dalla lingua in cui stai
  ragionando o in cui è scritto questo documento.
- **Codice specifico di Windows sotto `#[cfg(windows)]`, con un ramo `#[cfg(not(windows))]`
  esplicito** (anche solo un no-op o un fallback più semplice, mai un buco che rompe la
  compilazione su altre piattaforme). Esempio di riferimento nel codice: `spawn_detached` in
  `crates/startup-config/src/lib.rs`. Non scrivere mai `use std::os::windows::process::CommandExt`
  o chiamate equivalenti fuori da un blocco `#[cfg(windows)]`.
- **Nei test, mai un path letterale finto come `"unused"`, `"C:/Lare/..."` o simili per un
  `config_dir`/percorso che il codice sotto test userà davvero per fare I/O.** Usa sempre
  `tempfile::tempdir()` (Rust) o l'equivalente nel linguaggio che stai testando. Motivo concreto,
  non teorico: in questo stesso progetto, un helper che scrive un file di log viene invocato già
  alla *costruzione* di un comando (non solo quando il comando viene davvero eseguito) — un test
  con `config_dir: "unused".into()` ha creato per davvero una cartella `unused/logs/...` dentro il
  repository a ogni esecuzione della suite. Se il codice che stai testando fa I/O reale (crea
  cartelle, apre file, scrive log), il test deve usare una directory temporanea vera, sempre.
- **Per ogni crate/progetto che tocchi**: aggiorna `CHANGELOG.md` (versionamento semver, a
  partire da `2.0.0` per i crate 2.0) e `IMPLEMENTATION.md`, **nello stesso commit** del codice a
  cui si riferiscono, mai in un commit separato successivo. Guarda i file già esistenti nel crate
  che stai toccando per lo stile esatto (di solito: sezioni per versione — segui la convenzione
  già presente in QUEL file specifico, non inventarne una nuova). Bump di versione: `+1` sulla
  cifra di patch per un fix, `+1` sulla cifra minor (e patch a 0) per una feature — mai un salto
  arbitrario. La versione corrente di ogni crate è nel suo `Cargo.toml` e riepilogata in
  `Docs/i18n/ita/HANDOFF.md`: controlla lì il numero di partenza, non indovinarlo.
- **`Docs/i18n/ita/HANDOFF.md` va aggiornato nello stesso commit** di ogni cambiamento che conta
  come una "release" osservabile (una fix completata, una feature completata, un bump di
  versione). Una riga o un breve paragrafo nella sezione pertinente basta — non serve prosa lunga,
  ma deve esserci.
- **Non toccare `Cargo.lock`/`package-lock.json`/lockfile equivalenti oltre a quanto strettamente
  causato dalle dipendenze che hai davvero aggiunto o cambiato tu.** Non rigenerare un lockfile
  "per pulizia" — produce diff enormi e irrilevanti che rendono la revisione più difficile.

## 4. Metodo di lavoro — come sequenziare il compito

Segui questo ordine, non saltare passi anche se ti sembrano ovvi:

1. **Leggi il compito per intero, poi il materiale di contesto allegato (sezioni 1/1bis), poi il
   codice esistente dell'area coinvolta.** Non iniziare a scrivere prima di aver fatto tutte e tre
   le cose.
2. **Se il compito è un bug**: NON proporre un fix prima di aver capito la causa reale.
   - Riproduci il problema in modo affidabile (comando esatto, condizioni esatte). Se non riesci a
     riprodurlo, raccogli più informazioni (log reali del progetto — cerca cartelle `logs/` sotto
     `Configuration/` nei deploy — messaggi di errore completi, non troncati) prima di ipotizzare
     una causa.
   - Traccia il problema all'indietro dal sintomo fino all'origine: dove nasce il dato/stato
     sbagliato, non solo dove si manifesta.
   - Scrivi un test che riproduce il bug (fallisce prima del fix, per il motivo giusto) PRIMA di
     scrivere il fix.
   - Il fix corregge la causa, non il sintomo. Se ti accorgi che la causa reale è un problema
     architetturale più grande del compito assegnato, NON allargare lo scope da solo: implementa
     il fix minimo corretto per il compito assegnato e segnala il problema più grande nel report
     finale, lasciando la decisione al supervisore.
3. **Se il compito è una feature/modifica nuova**: TDD come descritto in sezione 3. Scomponi il
   lavoro in passi piccoli, ciascuno con il proprio ciclo RED→GREEN→(REFACTOR)→commit, invece di
   scrivere tutto e testare tutto insieme alla fine.
4. **Lavora in un branch dedicato** (sezione 6), con commit piccoli e frequenti — un commit per
   passo logico completato e verificato, non un commit gigante a fine lavoro.
5. **Prima di dichiararti finito**, esegui DAVVERO (non descrivere a memoria, non assumere)
   ogni comando di verifica pertinente al linguaggio/crate che hai toccato (sezione 5) e riporta
   l'output reale nel tuo report finale (sezione 9) — se qualcosa fallisce, risolvilo prima di
   proseguire o, se non riesci, dillo esplicitamente invece di ignorarlo.
6. **Scrivi il report finale** (sezione 9) prima di considerare il compito concluso.

## 5. Comandi di verifica — esegui quelli pertinenti al tuo compito

Dalla radice del repository:

```powershell
cargo build                                   # crate backend Rust (default-members, esclude ui)
cargo build -p ui                             # UI Tauri (compilazione lenta la prima volta)
cargo test                                    # test dei crate backend
cargo test -p <nome-crate>                    # test di un solo crate (es. -p orchestrator)
cargo test -p <nome-crate> <sottostringa>     # un singolo test o un sottoinsieme per nome
cargo clippy --all-targets                    # lint — non introdurre NUOVI warning rispetto
                                               # alla baseline: esegui clippy PRIMA di toccare
                                               # codice per sapere qual è la baseline attuale
cargo fmt --check                             # formattazione (nota: la v1 non è fmt-clean,
                                               # differenze note nel codice ereditato — non
                                               # riformattare in blocco codice che non hai
                                               # toccato tu, produce diff enormi e inutili)

dotnet test shell/lare-shell/LareShell.sln    # test della host C#

node --test crates/ui/frontend/*.test.mjs     # test JS puri del frontend (nessuna webview)
```

Per Python (`scripts/pytools/<dominio>/`): ogni dominio ha un virtualenv locale creato a mano, mai
committato. Se il tuo compito tocca uno di questi, crea/attiva il venv del dominio specifico
(`python -m venv venv`, poi `pip install -r requirements.txt pytest`) prima di eseguire `pytest`.

**Windows — problema comune**: se una build fallisce con "Accesso negato (os error 5)", un
binario del progetto è ancora in esecuzione da un test precedente. Se il repository ha uno script
`stop_lare.ps1` alla radice, usalo (`.\stop_lare.ps1`) prima di ricompilare; altrimenti termina
manualmente i processi `orchestrator.exe`, `ui.exe`, `lare-shell.exe`, `mcp-server.exe`,
`mcp-nmap.exe` con `taskkill /F /IM <nome>.exe` per ciascuno.

**Non eseguire mai un comando "a memoria" e riportarne l'esito come se l'avessi eseguito
davvero.** Se il tuo ambiente non ti permette di eseguire uno di questi comandi (vedi sezione 1:
niente accesso shell, niente Rust/dotnet/Python installato), scrivilo esplicitamente nel report
finale — il supervisore eseguirà lui quella verifica, ma deve sapere che non l'hai fatta tu.

## 6. Git — branch, commit, cosa non fare mai

- **Crea sempre un branch dedicato** prima di modificare qualunque file, con un nome descrittivo
  del compito (es. `fix/nome-breve-del-problema`, `feat/nome-breve-della-feature`). Non lavorare
  mai direttamente su `main`.
- **Commit piccoli e frequenti**, uno per passo logico completato e verificato (test verde dopo
  quel commit). Messaggio di commit chiaro, in italiano, che spiega il PERCHÉ oltre al cosa
  (stesso stile dei commit già presenti nella cronologia del repo — guardali con `git log` prima
  di iniziare per prendere il tono giusto).
- **Firma i tuoi commit con un trailer che identifica chiaramente quale AI li ha prodotti**, per
  esempio:
  ```
  Co-Authored-By: <Nome del modello, es. Kimi/Qwen/GPT-5/Gemini> <noreply@example.invalid>
  ```
  Se il tuo strumento non ti permette di aggiungere un trailer a un commit, metti il nome del
  modello tra parentesi quadre come primo token dell'oggetto del commit, per esempio
  `[Qwen] fix: ...`. In ogni caso, non usare MAI un trailer o una forma che nomini Claude o che lo
  faccia sembrare l'autore — il supervisore deve poter distinguere a colpo d'occhio quale AI ha
  scritto cosa, è così che la revisione funziona.
- **Mai** fare merge sul branch `main`, mai `push --force`, mai riscrivere la history di un
  branch condiviso, mai cancellare un branch che non hai creato tu in questo compito.
- **Mai** eseguire `git push` verso un remoto senza che ti sia stato esplicitamente chiesto —
  di norma il tuo lavoro resta locale (o sul branch dedicato) finché il supervisore non lo
  revisiona.
- Se scopri file modificati/non committati che NON sono farina del tuo sacco (esistevano già
  prima che tu iniziassi) — non toccarli, non includerli nei tuoi commit, segnalali nel report
  finale.

## 7. Cosa non fare mai (hard stop — fermati e segnala, non procedere)

- Cancellare permanentemente dati, file o commit che non hai creato tu in questo compito.
- Disattivare, bypassare o "aggiustare" test/lint/hook solo per farli passare senza che il
  problema che segnalano sia davvero risolto (skip di un test che fallisce, `#[ignore]` aggiunto
  per comodità, `--no-verify` su un commit, soglie di lint abbassate).
- Toccare segreti, token, credenziali o file di configurazione con dati sensibili — se il tuo
  compito sembra richiederlo, fermati e segnalalo invece di procedere.
- Espandere lo scope del compito di tua iniziativa ("già che c'ero ho anche…") — anche se la
  modifica ti sembra ovviamente giusta. Il supervisore deve poter valutare un cambiamento alla
  volta, delimitato da quello che è stato effettivamente chiesto.
- Inventare o presumere l'esito di un comando che non hai davvero eseguito.
- Modificare `CLAUDE.md`, questo documento, o qualunque file sotto `Docs/i18n/` che descriva
  convenzioni di progetto, a meno che il compito assegnato non sia esplicitamente "aggiorna questa
  documentazione".
- Introdurre nuove dipendenze esterne (crate Rust, pacchetti npm, pacchetti pip) non strettamente
  necessarie al compito senza segnalarlo chiaramente nel report finale — il supervisore deve poter
  valutare ogni nuova dipendenza aggiunta alla superficie del progetto.

Se una di queste situazioni si presenta, la cosa giusta da fare NON è indovinare e procedere: è
fermare il lavoro su quel punto specifico, documentare chiaramente nel report finale cosa hai
trovato e perché ti sei fermato, e proseguire (se possibile) sul resto del compito che non è
bloccato da quel problema.

## 8. Quando il compito è ambiguo o ti mancano informazioni

Probabilmente non hai un canale di conversazione continuo con Maurizio o col supervisore mentre
lavori. La regola pratica:

- Se l'ambiguità riguarda un DETTAGLIO reversibile (un nome di variabile, l'ordine di due
  parametri, dove mettere esattamente un file all'interno di una struttura già chiara): prendi la
  decisione più coerente con le convenzioni già esistenti nel codice che stai toccando, PROCEDI, e
  documenta la scelta e il perché nel report finale — il supervisore può correggerla facilmente se
  non è quella giusta.
- Se l'ambiguità riguarda qualcosa di IRREVERSIBILE, di SICUREZZA, o che cambia il comportamento
  osservabile del programma in un modo che il compito non specifica chiaramente: fermati su quel
  punto, implementa/documenta il resto del compito che non dipende da quella scelta, e scrivi
  chiaramente nel report finale qual è la domanda aperta e le opzioni che vedi.
- Non bloccarti mai completamente in attesa di una risposta che potrebbe non arrivare presto: fai
  progredire tutto quello che PUOI fare senza quella risposta.

## 9. Consegna finale — il report per il supervisore

Il report NON è facoltativo e non è libero nella forma: usa sempre gli stessi titoli, nello
stesso ordine, così il supervisore può confrontare report di AI diverse senza doverli prima
tradurre uno nel formato dell'altro. Questo è ciò che rende la supervisione economica invece che
un'indagine da rifare ogni volta.

- **Se hai accesso al repository**: scrivi il report come file
  `Docs/i18n/ita/reports/AAAA-MM-GG-<nome-ai>-<nome-breve-compito>.md` (crea la cartella
  `reports/` se non esiste), e committalo sul tuo branch.
- **Se non hai accesso al repository** (solo chat): scrivi il report come tuo ultimo messaggio,
  sotto l'intestazione esatta `## Report per il supervisore`.

Struttura obbligatoria del report:

```markdown
## Report per il supervisore

### Compito assegnato
(incolla o riassumi in una riga il compito che ti è stato dato)

### Cosa ho fatto
(cosa hai fatto, in breve, e perché — collegato al compito assegnato)

### File toccati
(elenco, o il comando per vederli: `git diff --stat <branch-base>..<tuo-branch>`)

### Branch e commit
(nome del branch; `git log --oneline <branch-base>..<tuo-branch>`)

### Esito reale dei comandi di verifica
(incolla l'output VERO dei comandi di sezione 5 che hai davvero eseguito — non un riassunto.
Se un test fallisce e non sei riuscito a risolverlo, scrivilo qui chiaramente)

### Deviazioni dal compito assegnato
(decisioni prese per ambiguità minori — sezione 8 — ed eventuali problemi più grandi scoperti
ma non risolti perché fuori scope — sezione 4, punto 2 — ciascuna con la motivazione)

### Documentazione aggiornata
(conferma esplicita che CHANGELOG/IMPLEMENTATION/HANDOFF sono stati aggiornati nello stesso
commit del codice, o perché non era pertinente per questo compito specifico)

### Cosa NON ho potuto verificare
(comandi che non hai potuto eseguire, aree non testate dal vivo, per mancanza di
strumenti/ambiente — sezione 1. Importante quanto il resto: dice al supervisore dove
concentrare la propria verifica)
```

## 10. Checklist rapida prima di dire "fatto"

- [ ] Ho letto questo documento per intero prima di scrivere codice.
- [ ] Ho letto HANDOFF.md e i file di documentazione pertinenti al mio compito (se avevo accesso).
- [ ] Ho letto il codice esistente dell'area che ho toccato prima di modificarlo.
- [ ] Ho scritto un test che falliva PRIMA di scrivere il fix/la feature (RED reale, eseguito).
- [ ] Il codice nuovo fa passare quel test (GREEN reale, eseguito).
- [ ] I test che ho scritto usano `tempfile::tempdir()` (o equivalente), mai un path letterale
      finto per qualcosa che fa I/O reale.
- [ ] Codice specifico di Windows è sotto `#[cfg(windows)]` con un ramo `#[cfg(not(windows))]`.
- [ ] Ho eseguito DAVVERO i comandi di build/test/lint pertinenti e ne riporto l'esito vero.
- [ ] Ho lavorato su un branch dedicato, mai su `main`.
- [ ] I miei commit sono piccoli, frequenti, con un trailer (o tag `[NomeAI]`) che mi identifica
      come AI non-Claude.
- [ ] CHANGELOG/IMPLEMENTATION/HANDOFF sono aggiornati nello stesso commit del codice.
- [ ] Non ho toccato `Cargo.lock`/lockfile oltre a quanto causato dalle mie dipendenze dichiarate.
- [ ] Non ho espanso lo scope oltre il compito assegnato.
- [ ] Non ho toccato segreti, credenziali, o file di configurazione sensibili.
- [ ] Ho scritto il report finale (sezione 9), con la struttura esatta richiesta, incluso cosa
      NON ho potuto verificare.

---

## Nota per chi assegna i compiti (Maurizio)

Questa sezione non è per l'AI che riceve il compito — è un promemoria per te su cosa allegare
insieme a questo documento, perché il documento da solo non basta: un'AI senza il contesto giusto
produrrà codice corretto nelle regole ma cieco alle convenzioni specifiche del punto che tocca, e
questo si traduce in più lavoro di revisione per Claude, non meno — che è l'opposto dell'obiettivo.

Insieme a questo documento e al compito, allega quando possibile:

- **Il contenuto completo dei file che l'AI dovrà modificare** (non solo il loro percorso — se
  l'AI non ha accesso al repository, un percorso senza contenuto è inutile).
- **Almeno un file di test esistente dello stesso crate/area**, come riferimento di stile.
- **La testa del `CHANGELOG.md`** del crate coinvolto (l'ultima voce), così l'AI vede il formato
  esatto da riprodurre.
- **La sezione pertinente di `HANDOFF.md`**, se il compito si inserisce in un lavoro già iniziato.
- Se è un bug: **i log reali** (non una descrizione a parole del problema) e i passi esatti di
  riproduzione, se li hai già.

Il risparmio di token da parte tua non viene dal documento in sé, ma da due cose: (1) dargli
abbastanza contesto perché la tua revisione sia leggere un diff invece di indagare da zero, e (2)
rivedere il lavoro dell'AI esterna UNA volta sola, non iterare avanti e indietro con lei — le
correzioni, se servono, falle fare a te (Claude) direttamente sul suo branch, non rimandandole
all'AI esterna per un altro giro.

---

*Questo documento è mantenuto in `Docs/i18n/ita/BRIEFING-AI-ESTERNE.md` nel repository di Lare
Terminal 2.0. Se lo stai leggendo incollato in un altro contesto (una chat, un altro strumento),
verifica comunque contro la versione nel repository se puoi — potrebbe essere stata aggiornata da
allora.*
