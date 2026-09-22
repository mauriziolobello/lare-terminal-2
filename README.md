# Lare Terminal 2.0

Un terminale PowerShell con un'AI agentica integrata nella riga di comando — non un chatbot a
fianco, non un overlay: un vero terminale (prompt reale, storico, profili, `cd` che persiste) in
cui le righe che iniziano con `/` sono comandi speciali, e `/ai "..."` fa agire l'AI **nella
stessa sessione di shell**, con lo stesso `cwd`, dietro un gate di conferma esplicito su ogni
comando che propone di eseguire.

Continua la storia di [Lare Terminal](https://github.com/mauriziolobello/lare-terminal), la sua
prima incarnazione (un overlay trasparente richiamato a tasto) — quel repository resta pubblico
come riferimento storico completo.

## Documentazione

Tutta sotto [`Docs/i18n/ita/`](./Docs/i18n/ita/), a partire da
[`00-apertura.md`](./Docs/i18n/ita/00-apertura.md) — cos'è, perché esiste, come muoversi nel resto
dei documenti (architettura, decisioni, stato attuale, sistema di plugin, tool Python, canali di
comunicazione, supporto multilingua).

## Stack

Rust (orchestratore, protocollo, server di tool, interfaccia Tauri), C# (host custom del motore
PowerShell), JavaScript vanilla (frontend delle finestre). Dettagli e perché delle scelte in
[`01-architettura.md`](./Docs/i18n/ita/01-architettura.md).

## Sviluppo

```powershell
cargo build              # crate backend
cargo build -p ui        # interfaccia Tauri
cargo test                # test del workspace
```

Guide operative complete: [`BUILD.md`](./Docs/i18n/ita/BUILD.md),
[`DEPLOY.md`](./Docs/i18n/ita/DEPLOY.md), [`RUN.md`](./Docs/i18n/ita/RUN.md).
