<#
.SYNOPSIS  Installa il profilo "Lare Terminal" in Windows Terminal (modalità B, spec §2.3).
.DESCRIPTION
  Scrive un "fragment" JSON in %LOCALAPPDATA%\Microsoft\Windows Terminal\Fragments\Lare\
  (è la cartella in cui Windows Terminal cerca i profili aggiunti da app terze: leggere
  LOCALAPPDATA qui è integrazione con WT, non configurazione Lare — spec §6.1). Il profilo avvia
  shell\lare-shell.exe nudo: la host risolve la Configuration come <exe>\..\Configuration\ e, se
  servono, avvia orchestratore e ui.exe da sola (§6.4). Riavvia Windows Terminal dopo l'installazione.
#>
param(
    [string]$ShellExe = (Join-Path $PSScriptRoot "shell\lare-shell.exe")
)
$ErrorActionPreference = "Stop"
if (-not (Test-Path $ShellExe)) {
    throw "Manca $ShellExe - esegui prima deploy_test_run.ps1 (fa il dotnet publish in Test Run\shell\)."
}
$fragmentDir = Join-Path $env:LOCALAPPDATA "Microsoft\Windows Terminal\Fragments\Lare"
New-Item -ItemType Directory -Force $fragmentDir | Out-Null
$fragmentPath = Join-Path $fragmentDir "lare-terminal.json"
$fragment = [ordered]@{
    profiles = @(
        [ordered]@{
            name              = "Lare Terminal"
            # Virgolettato: Windows Terminal passa la commandline a CreateProcess senza
            # lpApplicationName, che per un percorso senza virgolette e con spazi ("Test Run")
            # prova a risolvere ogni prefisso troncato allo spazio (…\Progetti\Lare.exe,
            # …\Lare Terminal.exe, …) — un eseguibile piazzato lì da un attaccante partirebbe
            # con il token dell'utente. Le virgolette tolgono l'ambiguità (il fallimento e2e
            # osservato in precedenza era dovuto al driver di test, non alle virgolette).
            commandline       = '"' + $ShellExe + '"'
            startingDirectory = "%USERPROFILE%"
        }
    )
}
$fragment | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8NoBOM $fragmentPath
Write-Host "Profilo 'Lare Terminal' installato in $fragmentPath"
Write-Host "Riavvia Windows Terminal e aprilo dal menu a tendina delle schede."
