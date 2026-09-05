# Spike 2 — finestra Tauri + xterm.js + ConPTY che ospita `lare-shell`

**Data:** 2026-09-05 (notte). **Codice:** `spikes/lare-terminal-window/` + modifiche minime a
`spikes/lare-shell-host/` (flag `--no-bars`, emissione OSC 9001). Usa-e-getta.
**Domanda:** la host PowerShell custom dello spike 1 può girare dentro una **finestra Tauri**
(estetica v1) con un emulatore di terminale nella webview, così che barre, segnalini e comandi
cliccabili siano HTML fuori dall'area terminale e lo scrollback sia quello dell'emulatore?
**Risposta: sì, su tutta la linea** (parole dell'utente: "ok su tutta la linea! E la finestra
graficamente si presenta molto meglio!").

## Cosa è stato costruito

- **Tauri 2.11.5** (vanilla JS, `frontendDist` statico, nessun bundler — come il crate `ui` v1),
  finestra 1000×650 con griglia CSS a tre righe: barra indicatori in alto (nome, shell in uso,
  orologio, `ultimo: /comando`, pallino), area terminale, barra in basso con pulsanti `/help`
  `/library` `/aichat` `/config`.
- **xterm.js 6.0.0** + addon-fit 0.11.0 vendored in `frontend/vendor/`, tema Campbell (quello di
  pwsh in Windows Terminal), font Cascadia Mono.
- **`portable-pty` 0.9.0** (ConPTY su Windows): comandi Tauri `pty_spawn`/`pty_write`/`pty_resize`
  /`shell_info`; thread lettore che emette i chunk in base64 (UTF-8 multi-byte spezzato fra chunk
  non si corrompe nel JSON); `term.write(Uint8Array)` lato JS; `onData` → `pty_write`;
  `ResizeObserver` → fit → `pty_resize`.
- Figlio: `lare-shell-spike.exe --no-bars` (fallback `pwsh.exe` se assente), cwd = home utente.
- Segnalino: la host emette `ESC ] 9001 ; lare ; intercept ; <riga> ESC \` a ogni intercettazione;
  xterm.js lo cattura con `registerOscHandler(9001, …)` e accende il pallino 2 s. Canale di riserva
  via titolo finestra, previsto perché ConPTY può scartare OSC sconosciute — **non è servito**.

## Esito dei test interattivi (utente, 2026-09-05)

| # | Verifica | Esito |
|---|---|---|
| 1 | Dentro la finestra sembra pwsh: prompt, colori, history, Tab; `python` REPL e `exit()`; Ctrl+C su `Start-Sleep` | **sì** |
| 2 | Scrollback dopo `dir C:\Windows`: tutte le righe recuperabili con la rotellina | **sì, pulito** — il costo n.6 dello spike 1 sparisce |
| 3 | Resize: terminale si riadatta, barre al loro posto | **sì**; lieve *flickering* mentre si allarga (il testo si riadatta al nuovo numero di colonne) — da attenuare con debounce del fit / renderer WebGL, non un blocco |
| 4 | Pulsanti in basso scrivono il comando nella shell | **sì** |
| 5 | Segnalino in alto dopo un `/comando` (pallino verde 2 s, `ultimo: /help`) via OSC 9001 | **sì** — ConPTY ha lasciato passare l'OSC custom |
| 6 | Chiusura con la X: nessun processo `lare-*` residuo | **sì** (verificato dall'utente in Task Manager) |

## Fatti tecnici emersi

1. **ConPTY non segnala EOF in lettura quando il figlio esce**: l'unico segnale affidabile è
   `Child::wait()` in un thread dedicato (l'*exit watcher*). Da tenere nel prodotto.
2. **`generate_context!` di Tauri incorpora `frontendDist` a compile time**: modifiche al solo
   frontend non fanno ricompilare il crate; serve `cargo clean -p <crate>` (o un `build.rs` che
   dichiari `rerun-if-changed` sulla cartella frontend). Gotcha operativo per il piano.
3. **Le OSC custom attraversano ConPTY** (almeno con Windows 11 / WT 1.24): canale utile per
   segnali host→emulatore anche in produzione, in aggiunta al WS dell'orchestratore.
4. **Nessun processo orfano** con la chiusura normale della finestra: la pty muore col padre e il
   figlio con lei.

## Conclusione per il design

Forma finale del lato shell di Lare Terminal 2.0 confermata:

```
ui.exe (Tauri) — finestra "Lare Terminal": xterm.js ↔ ConPTY ↔ lare-shell.exe (host C#) ↔ WS ↔ orchestrator
                 + finestre Markdown/config/library/plugin (v1)
```

La stessa `lare-shell.exe` resta usabile nuda in un profilo Windows Terminal (senza barre, o con
barre transitorie), per chi vuole la scheda "quasi pwsh". Un motore, due renderer. Lo spec
(`superpowers/specs/2026-09-04-lare-terminal-2-design.md`) va riscritto nelle sezioni shell su
questa base — una volta sola.
