# e2e-driver — guidare Lare Terminal "da tastiera" senza un umano

Metodo usato dal controller AI per l'e2e dal vivo del piano 2b (2026-09-07): la host `lare-shell`
gira in una **nuova finestra di Windows Terminal**, l'AI le manda tasti, cattura la finestra come
immagine e la "legge", e usa UI Automation per vedere/chiudere le finestre di `ui.exe`. Nessuna
libreria: solo PowerShell 7, `System.Windows.Forms.SendKeys`, `System.Drawing` e
`UIAutomationClient`.

**Da conservare** (richiesta di Maurizio, 2026-09-07): questa metodologia diventerà un plugin o un
metodo interno di interazione dell'AI dentro Lare Terminal — vedi HANDOFF "DA FARE / Idee".

## Script

| Script | Cosa fa |
|---|---|
| `drive.ps1 -Cmd start [-Exe <path>]` | apre una **nuova** finestra WT (`wt -w new`) con titolo fisso `LareE2E` (`--suppressApplicationTitle`) che lancia `lare-shell.exe` (default: `Test Run\shell\lare-shell.exe` del repo) |
| `drive.ps1 -Cmd type -Text "…"` | porta la finestra in primo piano (verificato) e digita il testo + Invio |
| `drive.ps1 -Cmd keys -Text "…"` | come `type` ma senza Invio (es. `y`, `n`, `^c` = Ctrl+C) |
| `drive.ps1 -Cmd shot -Out file.png` | screenshot della finestra in primo piano (poi l'AI legge il PNG) |
| `closeui.ps1` | chiude tutte le finestre di `ui.exe` tranne la pagina host e ne stampa i nomi (le finestre di `ui.exe` sono sempre in primo piano e coprono il terminale) |

Esempio di giro completo (da Git Bash servono `MSYS_NO_PATHCONV=1` e le virgolette come sotto):

```bash
export MSYS_NO_PATHCONV=1
P="pwsh -NoProfile -File scripts/dev/e2e-driver/drive.ps1"
$P -Cmd start && sleep 14
$P -Cmd type -Text '/ai "vai nella cartella Documents"' && sleep 25
$P -Cmd keys -Text "y" && sleep 15
pwsh -NoProfile -File scripts/dev/e2e-driver/closeui.ps1
$P -Cmd shot -Out /tmp/e2e.png        # poi Read del PNG
grep "gate:" "Test Run/Configuration/logs/lare-shell.log" | tail
```

## Lezioni (costate ore: non ripeterle)

- **Verifica SEMPRE la finestra in primo piano prima di mandare tasti.** `AppActivate` fallisce in
  silenzio quando un'altra app ha il fuoco (tipicamente una finestra di `ui.exe`, che è
  always-on-top): i tasti finiscono lì. `drive.ps1` ritenta con UIA `SetFocus` + trucco del tasto
  ALT + `SetForegroundWindow` e lancia se non ci riesce.
- **Mai `Ctrl+Shift+W` o `exit` verso Windows Terminal senza aver verificato la finestra.** `wt -w
  new` apre una seconda finestra nello stesso processo `WindowsTerminal.exe`; enumerando "la prima"
  finestra WT si legge quella dell'utente. Una scheda PowerShell di Maurizio è stata chiusa per
  errore così.
- **Il fragment del profilo WT è letto solo all'avvio di WT**: con la finestra dell'utente già
  aperta, `-p "Lare Terminal"` cade sul profilo di default. Per l'e2e si lancia l'exe direttamente.
- **`conhost` + Ctrl+C sintetico non funziona**: né `SendKeys "^c"` né `GenerateConsoleCtrlEvent`
  consegnano un Ctrl+C a un processo lanciato con `Start-Process conhost.exe` (nemmeno a un `pwsh`
  di controllo). In una finestra WT (ConPTY) `SendKeys "^c"` funziona.
- **`SendKeys` interpreta `( ) { } + ^ % ~`**: `print(2+2)` arriva come `print2"`. Escapare con le
  graffe (`{(}`) o evitare quei caratteri nei comandi digitati.
- **Da Git Bash `/help` diventa `C:/Program Files/Git/help`**: `MSYS_NO_PATHCONV=1`.
- Uno screenshot piccolo (es. 216×139) = la finestra in primo piano non era quella giusta.
- Le finestre di `ui.exe` (`/help`, `/ping`, output dell'AI) coprono il terminale: `closeui.ps1`
  prima dello screenshot, e i loro nomi sono la prova che il turno le ha aperte.
- Tenere `Start-Process` con **una sola stringa** di argomenti: con l'array non quota gli
  argomenti con spazi (`-p Lare Terminal` → WT prova ad avviare `Terminal`).
