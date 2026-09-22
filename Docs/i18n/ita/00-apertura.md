# Lare Terminal 2.0

## Cos'è

Lare Terminal è un terminale PowerShell con un'AI agentica integrata nella riga di comando.

Non è un chatbot in una finestra a fianco del terminale, e non è un overlay che si sovrappone allo
schermo per poi sparire: è un vero terminale — un vero motore PowerShell, prompt incluso, storico
dei comandi, profili, moduli, `cd` che persiste come sempre — dentro il quale alcune righe hanno un
significato speciale. Una riga che inizia con `/` è un **power command**: può essere un'istruzione
diretta al programma (`/config`, `/library`, `/help`, `/calc`), oppure una richiesta in linguaggio
naturale rivolta a un'AI (`/ai "trova i file più grandi in questa cartella e spiegami perché
occupano così tanto"`), che allora agisce **nella stessa sessione di shell**: i comandi che decide
di eseguire girano nel tuo runspace, con il tuo `cwd`, i tuoi moduli, la tua cronologia — non in un
sandbox separato che poi ti riporta un risultato.

Ogni altra riga — quelle che non iniziano con `/` — è ignorata dal livello Lare e passa dritta al
motore PowerShell sottostante, esattamente come se Lare non ci fosse.

## Perché esiste

Il progetto è alla sua seconda incarnazione. La prima (Lare Terminal, la cui storia completa resta
nel repository che la precede — vedi sotto) era un **overlay**: una finestra trasparente richiamata
con un tasto, sovrapposta allo schermo, un cursore attraverso cui impartire comandi e conversare con
un'AI. Funzionava, ma restava un *ospite* del sistema — non la shell stessa. Chi voleva usare
davvero PowerShell doveva uscire dall'overlay ed entrare in un terminale vero; l'AI e la shell reale
vivevano in due mondi separati.

Lare Terminal 2.0 nasce da una domanda semplice: **e se Lare fosse la shell, invece di affacciarsi
su di essa?** Non un hook che intercetta un'altra shell, non un client che le parla da fuori — una
**host custom del motore PowerShell**, nello stesso modo in cui `pwsh.exe` è una host di quel
motore. Il resto dell'architettura (l'orchestratore che parla con l'AI, il server MCP con i tool,
i canali come Telegram) resta quasi tutto quello che era già stato costruito nella prima versione;
cambia il punto d'ingresso, e ciò che vi è accoppiato.

## Per chi

Per chi passa la giornata in un terminale PowerShell e vuole un'AI che non sia un'applicazione a
parte da cui copiare e incollare comandi, ma un partecipante alla stessa sessione — che vede lo
stesso `cwd`, può eseguire comandi reali (dietro conferma esplicita, mai in silenzio), e può
rispondere in finestre dedicate quando la risposta è più ricca di una riga di testo (una tabella,
una spiegazione lunga, del codice).

È anche, dichiaratamente, un progetto costruito in **collaborazione diretta fra una persona e delle
AI di sviluppo** (Claude di Anthropic in supervisione, più AI esterne per l'implementazione dei
compiti più delimitati) — non solo nel prodotto finale, ma nel *modo* in cui è stato costruito: TDD
reale, decisioni architetturali motivate e non solo verbalizzate, un log di ogni scelta importante
(`02-decisions.md`). Chi è curioso di come si costruisce software con un'AI come collaboratore
abituale, non solo come autocompletamento, trova qui un caso concreto e documentato.

## Rapporto con la versione precedente

Questo repository (`lare-terminal-2`) continua la storia di
[Lare Terminal](https://github.com/mauriziolobello/lare-terminal), il progetto precedente, stesso
proprietario. La prima versione resta pubblica per intero, come riferimento storico: architettura
precedente, decisioni originarie (ADR-001..014, riprese anche qui), il percorso che ha portato fin
qui. Non è materiale "superato e da ignorare" — è la prima metà della stessa storia, e mostra il
ragionamento con cui il progetto si è evoluto.

## Stato del progetto

In sviluppo attivo. L'architettura a tre strati (canali → orchestratore → server di tool) è
stabile; la shell PowerShell custom, la finestra terminale, il loop AI con gate di conferma, il
sistema di plugin e i canali (Telegram, AI Chat, integrazioni esterne) sono implementati e in uso
quotidiano. Il quadro dettagliato — cosa c'è oggi, cosa manca, i debiti noti — è in
[`03-stato-e-implementazione.md`](./03-stato-e-implementazione.md).

## Come muoversi in questa documentazione

- [`01-architettura.md`](./01-architettura.md) — i tre strati, le decisioni chiave e perché sono
  state prese così.
- [`02-decisions.md`](./02-decisions.md) — il log delle decisioni architetturali (ADR), a partire
  dalla prima versione.
- [`03-stato-e-implementazione.md`](./03-stato-e-implementazione.md) — cosa esiste oggi, per area.
- [`04-plugin-system.md`](./04-plugin-system.md) — come funzionano i plugin ed estendere il
  programma con uno nuovo.
- [`05-pytools.md`](./05-pytools.md) — tool Python invocabili dall'AI (analisi finanziaria,
  networking, altro).
- [`06-canali.md`](./06-canali.md) — Telegram, AI Chat, i canali di integrazione esterna.
- [`07-i18n.md`](./07-i18n.md) — come funziona il supporto multilingua dell'interfaccia.
- [`BUILD.md`](./BUILD.md), [`DEPLOY.md`](./DEPLOY.md), [`RUN.md`](./RUN.md) — compilare,
  distribuire, avviare.
- [`KNOWN-ISSUES.md`](./KNOWN-ISSUES.md) — problemi noti.
- [`TESTING-e2e.md`](./TESTING-e2e.md) — checklist di verifica end-to-end.
