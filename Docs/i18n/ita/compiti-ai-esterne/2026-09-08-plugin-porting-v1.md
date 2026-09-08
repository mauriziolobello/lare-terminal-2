# Compito per AI esterna — completare il porting dei plugin da v1 (`counter`, `crypto`, `lc`)

> **Prima cosa**: leggi per intero `Docs/i18n/ita/BRIEFING-AI-ESTERNE.md` alla radice del
> repository, poi questo file per intero, PRIMA di scrivere codice.

## Contesto — cosa significa davvero "non ancora trasferiti" qui

Il piano 1 di questo progetto (2.0) ha copiato per intero i sorgenti di **tutti e cinque** i
plugin di v1 nel workspace 2.0: `plugin-ping`, `plugin-counter`, `plugin-calc`, `plugin-lc`,
`plugin-crypto` (vedi `Docs/i18n/ita/HANDOFF.md`, sezione "Versioni correnti" — tutti e cinque
elencati come `2.0.0 (da v1 ...)`). Tutti e cinque **compilano già** oggi
(`cargo build -p plugin-counter -p plugin-crypto -p plugin-lc` — verificato prima di scrivere
questo compito) e sono già membri di `default-members` nel `Cargo.toml` del workspace: un plain
`cargo test` dalla radice esegue già i loro test unitari.

**Il gap reale, verificato leggendo il codice prima di scrivere questo compito, non è "il codice
non esiste"**:

| Plugin | Compila | Test unitari propri | e2e in `crates/orchestrator/tests/` | In `deploy_test_run.ps1 -IncludePlugins` |
|---|---|---|---|---|
| `ping` | sì | sì | sì (`plugin_e2e.rs`) | sì |
| `calc` | sì | sì | sì (`plugin_calc_e2e.rs`) | sì |
| `counter` | sì | sì | **sì** (`plugin_window_e2e.rs`, round-trip Activate→UiEvent→UpdateWindow) | **NO** |
| `lc` | sì | sì | **sì** (`plugin_lc_window_e2e.rs`) | **NO** |
| `crypto` | sì | sì | **NO — nessun file la tocca** | **NO** |

Riga `e2e` per `counter`/`lc` verificata VERDE davvero prima di scrivere questo compito (non solo
letta): `cargo test -p orchestrator --test plugin_window_e2e --test plugin_lc_window_e2e --
--ignored` → 3 test, tutti `ok`. Questo è il tuo baseline: prima di toccare qualunque file,
esegui lo stesso comando e conferma che vedi lo stesso risultato — se non lo vedi, il problema è
precedente al tuo lavoro e va segnalato subito nel report, non assunto come "l'ho rotto io".

Quindi il lavoro vero è più piccolo e più preciso di "portare tre plugin da zero":

1. **`crypto`** ha davvero un buco — nessun test end-to-end nell'orchestrator, mai eseguito
   attraverso la catena reale discovery→spawn→protocollo. È l'unico dei tre dove serve scrivere
   qualcosa di nuovo di sostanza.
2. **`counter`** e **`lc`** hanno già una e2e che li esercita per intero (in una dir temporanea
   costruita a mano dal test, NON tramite `deploy_test_run.ps1`) — non sono mai stati verificati
   attraverso il percorso di deploy REALE che userebbe un utente vero. Il gap qui è la
   riga mancante in `deploy_test_run.ps1` più una verifica dal vivo, non nuovo codice.

## Scope

**Tocca**:
- `deploy_test_run.ps1` (radice del repo) — aggiungi `counter`, `crypto`, `lc` all'elenco dei
  plugin copiati con `-IncludePlugins` (oggi righe ~44-49, `foreach ($id in "ping", "calc")`).
- `build.ps1` (radice del repo) — **stesso identico gap**: il suo blocco `-IncludePlugins` (oggi
  righe ~45-49) compila esplicitamente solo `-p plugin-ping -p plugin-calc`. Anche se un plain
  `cargo build`/`cargo test` (senza `-p`) compila già tutti e 5 i plugin (sono in
  `default-members` del `Cargo.toml` del workspace — verificato), questo flag è pensato come
  l'elenco autorevole di "quali plugin fanno parte del deploy": va tenuto coerente con
  `deploy_test_run.ps1`. Aggiungi `-p plugin-counter -p plugin-crypto -p plugin-lc` alla riga
  `cargo build`, e aggiorna il messaggio `Write-Host` successivo che elenca i nomi.
- `crates/orchestrator/tests/plugin_crypto_e2e.rs` — nuovo file, e2e reale per `plugin-crypto`
  (dettagli sotto).
- Documentazione (vedi sezione dedicata più sotto).

**NON toccare** il codice sorgente di `plugin-ping`/`plugin-calc` (già fatto, fuori scope) né
`crates/plugin-counter/src/`, `crates/plugin-lc/src/` (già funzionanti, verificato dalla loro
e2e esistente — se durante la verifica dal vivo scopri che uno dei due NON funziona davvero
attraverso `deploy_test_run.ps1` nonostante la sua e2e passi, quello è un finding reale da
indagare — vedi la sezione "Se trovi un bug reale" più sotto, non da ignorare né da aggirare).

## Parte 1 — `deploy_test_run.ps1`

Riga (numero indicativo, verifica leggendo il file):

```powershell
foreach ($id in "ping", "calc") {
```

diventa:

```powershell
foreach ($id in "ping", "calc", "counter", "crypto", "lc") {
```

Il resto del blocco (copia `$id.exe` + verifica che `plugins\$id\plugin.json` sia già committato
— lo è per tutti e 5, verificato) resta identico, è già generico per `$id`. Verificato anche che
`Test Run\plugins\` oggi contiene SOLO `ping`/`calc` (le cartelle di `counter`/`crypto`/`lc` non
esistono ancora lì) — è normale, le crea questo stesso script quando gira con l'elenco corretto,
non è un problema da risolvere a mano prima.

Nota non bloccante, per non farti perdere tempo se te ne accorgi durante la Parte 1 o 3: i
`plugin.json` di `crypto` (`"version": "1.0.1"`) e `lc` (`"version": "0.4.5"`) hanno un numero di
versione diverso dal `Cargo.toml` del proprio crate (`2.0.0` per entrambi) — è un disallineamento
preesistente, non introdotto da questo compito. Non "sistemarlo": fuori scope, a meno che tu
scopra che quel numero è usato a runtime da qualche parte (verificato di no, con un grep veloce
prima di scrivere questo compito, ma se trovi il contrario segnalalo nel report invece di
correggerlo silenziosamente).

## Parte 2 — e2e reale per `plugin-crypto`

Nuovo file `crates/orchestrator/tests/plugin_crypto_e2e.rs`, stesso pattern strutturale già usato
nel crate — **leggi per intero, prima di scrivere, questi due file esistenti come riferimento**:

- `crates/orchestrator/tests/plugin_e2e.rs` (83 righe, il più semplice — round trip
  Init→Ready→Deinit, individua il binario compilato in `target/debug/`, costruisce una
  `plugins/<id>/` in una tempdir, usa `discover`+`PluginHost::start`+`spawn_plugin` reali).
- `crates/orchestrator/tests/plugin_window_e2e.rs` (già testa `counter`: round trip completo
  `activate` → `ShowWindow` → `route_ui_event` → `UpdateWindow` — è la struttura più vicina a
  cosa serve per `crypto`, che ha bisogno di simulare un input utente, non solo Init/Ready).
- `crates/orchestrator/tests/plugin_calc_e2e.rs` (204 righe — un altro plugin con vero
  comportamento computazionale dietro l'interfaccia, non solo un contatore: utile per vedere come
  si struttura un test e2e che verifica un RISULTATO, non solo che una finestra si apra).

**Cosa deve verificare, come minimo (bar minima, allineata alle altre e2e del crate)**:
`Init` → `Ready{name:"crypto", protocol_version:1}` → `Activate` → `ShowWindow` (finestra
principale, contiene l'interfaccia del cifrario di default).

**Cosa dovrebbe verificare, se ci arrivi (bar completa — un vero round-trip funzionale, non solo
che la finestra si apra)**: un ciclo reale di cifratura. Per capire gli `element_id`/la forma
esatta degli eventi UI che `plugin-crypto` si aspetta, **leggi
`crates/plugin-crypto/src/state.rs`** (380 righe — contiene `pub(crate) fn handle(...)`, il
dispatch puro di `HostToPlugin`, PIÙ i suoi stessi test unitari in fondo al file, che sono la
fonte di verità più affidabile su quali `UiEvent{element_id, value}` producono quali risposte —
più affidabile di questo documento, che non li elenca uno per uno per non rischiare di darti
un'informazione sbagliata su un file che non ho letto riga per riga). Il cifrario più semplice da
usare per un test è **Cesare** (`crates/plugin-crypto/src/ciphers/caesar.rs`, 105 righe — leggilo
per un input/output noto da usare come asserzione, es. testo in chiaro + chiave → testo cifrato
atteso).

Se vuoi capire il PERCHÉ di certe scelte di design di questo plugin (non necessario per scrivere
il test, ma disponibile): `crates/plugin-crypto/src/state.rs` cita
`Docs/superpowers/specs/2026-07-19-crypto-plugin-design.md` — quel file esiste SOLO nel repo v1
(sola lettura): `C:\Users\Maurizio\Documents\Progetti\Lare Terminal\Docs\superpowers\specs\2026-07-19-crypto-plugin-design.md`
(percorso assoluto sulla macchina di Maurizio — se il tuo ambiente non ha accesso a quel percorso,
salta questo riferimento, non è necessario per il compito).

**Un dettaglio del design di `crypto` utile per capire il codice, NON un rischio**: `state.rs`
genera un `window_id` per una "dialog parametri" separata dalla finestra principale, con un
offset (`DIALOG_WINDOW_ID_OFFSET = 1_000_000`) per non collidere con gli id sequenziali che
l'host assegna a `Activate`. Questo richiede che l'host registri il `window_id` per OGNI
`ShowWindow` osservato, non solo per quello di `Activate` — **verificato in
`crates/orchestrator/src/plugins/host.rs` (righe ~373-388, commento "2026-07-19") che questo fix
è già presente nella 2.0**: il pump fa `windows.lock().await.insert(*window_id, pid.clone())` per
ogni `ShowWindow` che vede, con un blind-insert idempotente. Quindi la dialog parametri di
`crypto` dovrebbe funzionare correttamente da subito — se durante la Parte 3 si comporta in modo
imprevisto, è un bug genuino da indagare (vedi sezione sotto), non un problema noto già previsto.

## Parte 3 — verifica dal vivo (l'unica che copre il percorso di deploy REALE)

Nessuna delle e2e esistenti (incluse quelle già verdi per `counter`/`lc`) passa mai da
`deploy_test_run.ps1` — costruiscono la propria cartella `plugins/` a mano in una tempdir. Questo
compito chiude anche quel gap:

```powershell
.\stop_lare.ps1
.\build.ps1 -SkipShell -IncludePlugins
.\deploy_test_run.ps1 -SkipShell -IncludePlugins
```

(dopo il tuo fix della Parte 1, entrambi gli `-IncludePlugins` sopra devono coprire tutti e 5 i
plugin — è anche il modo più semplice per accorgerti se hai dimenticato di aggiornare uno dei due
script: se `deploy_test_run.ps1` stampa un `Warning: plugin ... non compilato ... saltato` per
`counter`/`crypto`/`lc`, vuol dire che `build.ps1` non li ha compilati, verifica quel file)

Atteso: il comando stampa `copiato plugin ping`/`calc`/`counter`/`crypto`/`lc` (non più
`Warning: plugin ... non compilato ... saltato` per questi tre). Verifica con
`Get-ChildItem "Test Run\plugins\"` che tutte e 5 le cartelle esistano con dentro `.exe` +
`plugin.json`.

Poi, avvio reale:

```powershell
.\Test Run\ui.exe
```

Attendi il self-heal, poi usa i comandi `/counter`, `/crypto`, `/lc` (uno alla volta) nella
finestra "Lare Terminal". Atteso per ciascuno: la finestra del plugin si apre, nessun crash
dell'orchestratore, nessuna finestra console spuria (fix di una sessione precedente di questo
progetto — se ne vedi comparire una durante questo test, È un regressione reale su quel fix,
segnalala nel report, non aggirarla). Se vuoi essere rigoroso sulla verifica "nessuna finestra
spuria": enumera le top-level window con UIA SENZA filtrare per PID (lo stesso errore che ha
inficiato una verifica precedente in questo progetto — un filtro sui soli PID di
`ui`/`orchestrator`/`lare-shell` non vedrebbe mai una finestra spuria, che appartiene al PID di
`WindowsTerminal.exe`) — non è un requisito stretto di questo compito, ma se lo fai, fallo bene.

Se non riesci a fare questa parte dal vivo (nessun accesso a un ambiente Windows reale, o
altro limite): scrivilo esplicitamente nel report finale.

## Se trovi un bug reale (non solo "manca il test/il deploy")

Se durante la Parte 2 o la Parte 3 scopri che uno di questi tre plugin NON funziona davvero
(crash, finestra che non si apre, protocollo che si blocca, la dialog parametri di `crypto` rotta)
— **non è "fuori scope", è esattamente il tipo di cosa che questo compito serve a scoprire**.
Segui comunque la disciplina generale di `BRIEFING-AI-ESTERNE.md`: capisci la causa reale prima di
proporre un fix (non un fix a sintomo), un test che riproduce il problema PRIMA del fix, il fix
minimo che lo risolve. Se il fix richiederebbe toccare `crates/orchestrator/src/plugins/` (il
codice host, non il plugin) — è permesso, a differenza del divieto sui sorgenti di
`plugin-counter`/`plugin-lc` sopra: quel divieto vale SOLO se non hai trovato un problema reale
lì, per non introdurre modifiche non necessarie a codice che già funziona.

## Documentazione da aggiornare

- `Docs/i18n/ita/HANDOFF.md` — una voce in "FATTO" che descrive cosa hai completato (deploy dei
  tre plugin + e2e di `crypto`). Se hai aggiunto un fix reale (sezione sopra) al codice di un
  crate, aggiorna anche la riga di quel crate in "Versioni correnti" con il bump di versione
  appropriato (patch per un fix) e il suo `CHANGELOG.md`/`IMPLEMENTATION.md` — SOLO per il/i
  crate che hai davvero modificato, non per tutti e cinque a prescindere (`counter`/`lc` restano
  `2.0.0` se non li hai toccati; `crypto` probabilmente resta `2.0.0` anche lui se il compito si
  ferma alla Parte 2 senza trovare bug — aggiungere SOLO un test e2e in un altro crate
  (`orchestrator`) non è un cambiamento di comportamento osservabile del plugin stesso).
- `crates/orchestrator/CHANGELOG.md` — una voce breve per la nuova e2e di `plugin-crypto`
  (aggiunta di copertura di test, non un fix — non serve necessariamente un bump di versione per
  questo da solo, usa il tuo giudizio secondo la regola generale di `BRIEFING-AI-ESTERNE.md`
  sezione 3 sul versionamento; se non fai un bump, spiega perché nel report).

## Verifica finale (comandi da eseguire davvero)

```powershell
cargo build     # tutti i default-members, incluso ping/calc/counter/lc/crypto
cargo test      # idem, incluso il nuovo plugin_crypto_e2e.rs (di default #[ignore], vedi sotto)
# le QUATTRO e2e dei plugin con finestra, incluse le due preesistenti (non solo la nuova) — un
# "cargo test passa" da solo non dice nulla su di loro, sono tutte #[ignore] di default:
cargo test -p orchestrator --test plugin_window_e2e --test plugin_lc_window_e2e --test plugin_crypto_e2e -- --ignored
cargo clippy -p orchestrator -p plugin-crypto --all-targets
```

Ricorda: le e2e in questo crate sono `#[ignore]` di default (richiedono binari compilati) — un
plain `cargo test` NON le esegue, serve `-- --ignored` esplicito. Verifica che il tuo nuovo test
segua la stessa convenzione (`#[ignore = "..."]` con un messaggio che dice cosa compilare prima,
stesso stile degli altri tre file di riferimento).

## Cosa NON fare (oltre alle regole generali di `BRIEFING-AI-ESTERNE.md`)

- Non toccare `plugin-ping`/`plugin-calc` (già fatti, fuori scope).
- Non toccare `crates/plugin-counter/src/` o `crates/plugin-lc/src/` A MENO CHE tu non abbia
  trovato un bug reale verificato dal vivo (vedi sezione dedicata) — non "migliorare" codice che
  già funziona senza un motivo concreto scoperto durante questo compito.
- Non bumpare la versione di un crate che non hai davvero modificato nel comportamento.
