<#
.SYNOPSIS
    Rimuove il frammento di profilo Windows Terminal installato da install-wt-profile.ps1.

.DESCRIZIONE
    Elimina semplicemente il file JSON scritto sotto
    %LOCALAPPDATA%\Microsoft\Windows Terminal\Fragments\Lare\lare-terminal-spike.json
    (e la cartella "Lare" se rimane vuota). Non tocca nient'altro: nessuna modifica
    a settings.json, nessun'altra cartella del progetto.
#>

[CmdletBinding()]
param()

$fragmentDir = Join-Path $env:LOCALAPPDATA 'Microsoft\Windows Terminal\Fragments\Lare'
$fragmentFile = Join-Path $fragmentDir 'lare-terminal-spike.json'

if (Test-Path $fragmentFile) {
    Remove-Item -Path $fragmentFile -Force
    Write-Host "Rimosso: $fragmentFile"
} else {
    Write-Host "Nessun frammento trovato in: $fragmentFile (niente da rimuovere)."
}

if (Test-Path $fragmentDir) {
    $remaining = Get-ChildItem -Path $fragmentDir -Force -ErrorAction SilentlyContinue
    if (-not $remaining -or $remaining.Count -eq 0) {
        Remove-Item -Path $fragmentDir -Force
        Write-Host "Cartella vuota rimossa: $fragmentDir"
    }
}

Write-Host "Il profilo 'Lare Terminal (spike)' dovrebbe sparire da Windows Terminal al prossimo riavvio (o refresh automatico dei frammenti)."
