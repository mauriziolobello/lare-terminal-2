# Documentazione — italiano

**Tutta** la documentazione di Lare Terminal 2.0 vive sotto `Docs/i18n/<lingua>/`, una cartella
per lingua con la stessa struttura di file. L'italiano (`ita`) è la versione di riferimento; le
traduzioni in altre lingue (es. `eng/`) possono essere prodotte da strumenti diversi. Motivo:
il giorno in cui il repo diventasse pubblico, chi è interessato deve poter leggere tutta la
storia del progetto — decisioni, spec, piani, spike — nella propria lingua.

Struttura sotto ogni lingua:

- `NN-argomento.md` — documentazione narrativa: architettura, guide d'uso e di deploy, stato del
  sistema, handoff fra sessioni. Numero progressivo a due cifre come nella v1
  (`02-architecture.md`, `08-persistent-shell.md`, …).
- `superpowers/specs/AAAA-MM-GG-argomento-design.md` — spec di design approvate: la fonte di
  verità per il "perché" delle scelte.
- `superpowers/plans/AAAA-MM-GG-argomento-plan.md` — piani di implementazione derivati dalle spec.
- `spikes/AAAA-MM-GG-argomento.md` — esiti degli spike tecnici (il codice usa-e-getta sta in
  `spikes/` nella root del repo, fuori da `Docs/`).

Restano accanto al codice, non tradotti: `crates/<crate>/CHANGELOG.md` e `IMPLEMENTATION.md`.
