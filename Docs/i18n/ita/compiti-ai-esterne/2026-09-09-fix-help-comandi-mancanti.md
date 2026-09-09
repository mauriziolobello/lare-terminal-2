# Compito per AI esterna — `/help` non elenca `/find`, `/reset`, `/nowin`

> **Prima cosa**: leggi per intero `Docs/i18n/ita/BRIEFING-AI-ESTERNE.md` alla radice del
> repository, poi questo file per intero, PRIMA di scrivere codice.

## Cosa è successo

Maurizio ha confrontato dal vivo `/help` (2.0) con l'equivalente v1 e ha trovato che 3 comandi
REALMENTE esistenti in 2.0 non sono documentati: `/find`, `/reset`, `/nowin`. Verificato leggendo
il codice (non assunto) prima di scrivere questo compito — vedi sotto.

## Scope — SOLO contenuto, zero codice

Dalla migrazione fatta in un compito precedente, `/help` carica il proprio corpo da file esterni,
non da costanti Rust — questo compito tocca SOLO quei 3 file di testo, nessun `.rs`:

- `Test Run/Configuration/help/it.md`
- `Test Run/Configuration/help/en.md`
- `Test Run/Configuration/help/es.md`

## I 3 comandi mancanti — comportamento verificato nel codice

**`/find [<query>] [in:"<frase>"] [folder:from-here]`** — ricerca file dal vivo, finestra
dedicata. Verificato in `crates/orchestrator/src/search/query.rs`: `<query>` cerca sul nome file
(parole libere, tutte devono comparire; `*`/`?` per glob; `re:<pattern>` per regex);
`in:"<frase>"` cerca nel contenuto (frase esatta, case-insensitive, combinabile col nome);
`folder:from-here` restringe alla cartella corrente e sottocartelle (esclude standard/cloud/unità
esterne), combinabile con nome e `in:`.

**`/reset`** — riavvia la sessione shell. Verificato in `crates/orchestrator/src/core.rs`
(branch `"reset"`): produce il messaggio `sessione riavviata` nel terminale.

**`/nowin <richiesta>`** — l'AI risponde come testo nel terminale, niente finestra Markdown.
Verificato in `crates/orchestrator/src/core.rs` (`strip_nowin_prefix`): se usato senza richiesta
(`/nowin` da solo) produce il messaggio `usa /nowin <richiesta>`.

## Dove inserirli

Nella sezione `## Comandi` (it) / `## Commands` (en) / `## Comandos` (es) di ciascun file, insieme
agli altri comandi core già elencati — non nella sezione "Strumenti esterni" (quella è per
`/markets`/`/nmap`/`/pyping`, canali con processo esterno, `/find`/`/reset`/`/nowin` non lo sono).
Mantieni lo stesso stile delle righe esistenti (un bullet `- \`/comando <arg>\` — descrizione.`),
stesso ordine logico del resto della sezione (non importa la posizione esatta, basta che siano
vicino agli altri comandi "core" tipo `/open`/`/web`/`/show`).

**Non aggiungere nessun altro comando** oltre a questi 3 — in particolare non aggiungere i plugin
(`/calc`, `/counter`, `/crypto`, `/lc`, `/ping`): verificato che né v1 né la versione attuale di
`it.md` li elencano tutti (solo `/calc` compare, gli altri no) — è una scelta di design esistente,
non un'omissione da correggere in questo compito. Se hai un dubbio su questo, fermati e segnalalo
nel report invece di aggiungerli di tua iniziativa.

## Traduzione

`it.md` è la fonte di verità per il contenuto esatto (testo verificato sopra). Traduci in inglese
naturale per `en.md` e in spagnolo naturale per `es.md`, stesso registro conciso delle righe
esistenti nei rispettivi file — non un traduttore automatico letterale. Per `es.md`, se hai dubbi
sulla resa esatta di un termine tecnico (es. "sessione riavviata"), usa il tuo giudizio da
madrelingua.

## Documentazione

- `Docs/i18n/ita/HANDOFF.md`: una riga in FATTO — sono file dati, non serve bump di versione di
  nessun crate (stessa regola già applicata a `i18n/es.json`: un file di traduzione da solo non
  bumpa nulla).
- Non serve toccare `CHANGELOG.md`/`IMPLEMENTATION.md` di `orchestrator` — nessun codice Rust
  cambia.

## Verifica finale

Non c'è codice da compilare. Verifica a mano che:
- I 3 comandi compaiono in tutti e 3 i file, con la sintassi esatta verificata sopra.
- Nessun altro comando è stato aggiunto o rimosso.
- Lo stile (bullet, backtick, trattino em-dash "—") è coerente con le righe esistenti in ciascun
  file.

Se il tuo ambiente ha `git`: `git diff` deve mostrare SOLO questi 3 file toccati.

## Cosa NON fare

- Non toccare nessun file `.rs` — questo è un compito di solo contenuto.
- Non aggiungere comandi plugin (vedi sopra).
- Non toccare `it.json`/`en.json`/`es.json` sotto `Configuration/i18n/` — sono un sistema diverso
  (stringhe della finestra Tauri), non c'entrano con `/help`.
- Non tradurre "Lare Terminal" — compare nella riga introduttiva di ciascun file ("Scrivi i
  comandi... nella riga di comando di Lare Terminal", verificato) e resta letterale in tutte e 3
  le lingue, come già è oggi.
