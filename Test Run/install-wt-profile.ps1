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
            # Senza virgolette anche se il percorso contiene spazi ("Test Run"): Windows Terminal
            # risolve da solo un eseguibile con spazi nel percorso (verificato dallo spike e all'e2e).
            commandline       = $ShellExe
            startingDirectory = "%USERPROFILE%"
        }
    )
}
$fragment | ConvertTo-Json -Depth 5 | Set-Content -Encoding UTF8 $fragmentPath
Write-Host "Profilo 'Lare Terminal' installato in $fragmentPath"
Write-Host "Riavvia Windows Terminal e aprilo dal menu a tendina delle schede."
