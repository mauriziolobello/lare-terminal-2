# Documentazione — italiano

Questa cartella contiene la documentazione **narrativa** di Lare Terminal 2.0 in italiano:
architettura, guide d'uso e di deploy, stato del sistema, handoff fra sessioni. Le altre lingue
vivono in cartelle sorelle sotto `Docs/i18n/` (es. `eng/`), con la stessa struttura di file; le
traduzioni possono essere prodotte da strumenti diversi, la versione italiana è quella di
riferimento.

Cosa **non** sta qui:
- `Docs/superpowers/specs/` e `Docs/superpowers/plans/` — spec di design e piani di
  implementazione: artefatti di processo, fonte di verità per il "perché" delle scelte, scritti in
  italiano ma non tradotti.
- `Docs/spikes/` — esiti degli spike tecnici (codice usa-e-getta in `spikes/`).
- `crates/<crate>/CHANGELOG.md` e `IMPLEMENTATION.md` — documentazione per crate, accanto al codice.

Convenzione nomi: `NN-argomento.md` con numero progressivo a due cifre, come nella v1
(`02-architecture.md`, `08-persistent-shell.md`, …).
