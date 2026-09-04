<#
.SYNOPSIS
    Installa un profilo "frammento" di Windows Terminal che punta all'eseguibile
    compilato di lare-shell-spike.

.DESCRIZIONE
    Windows Terminal supporta i "fragment extensions": file JSON depositati sotto
    %LOCALAPPDATA%\Microsoft\Windows Terminal\Fragments\<Nome>\<file>.json che
    aggiungono automaticamente profili al menu "+" di Windows Terminal, senza
    dover editare a mano settings.json. Questo script:
      1. cerca l'eseguibile compilato più recente (dotnet build/publish possono
         mettere l'output in cartelle diverse a seconda di RID/configurazione,
         quindi cerchiamo con Get-ChildItem invece di assumere un path fisso);
      2. scrive il file JSON del frammento con un profilo "Lare Terminal (spike)"
         che lancia quell'eseguibile.

    NOTA: questo script NON viene eseguito automaticamente da Claude/dall'agente
    che ha generato lo spike: va lanciato manualmente dall'utente quando vuole
    provare il profilo in Windows Terminal.
#>

[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

$repoRoot = $PSScriptRoot

# Cerchiamo l'eseguibile più recente sotto bin\ (qualunque sia RID/configurazione):
# in un progetto senza RuntimeIdentifier esplicito, `dotnet build` produce
# bin\<Config>\net10.0\lare-shell-spike.exe; con `dotnet publish -r win-x64` invece
# il percorso include anche \win-x64\. Piuttosto che assumere un path fisso, li
# cerchiamo tutti e prendiamo il più recente per data di modifica.
$candidates = Get-ChildItem -Path (Join-Path $repoRoot 'bin') -Recurse -Filter 'lare-shell-spike.exe' -ErrorAction SilentlyContinue |
    Sort-Object LastWriteTime -Descending

if (-not $candidates -or $candidates.Count -eq 0) {
    Write-Error "Nessun eseguibile 'lare-shell-spike.exe' trovato sotto '$repoRoot\bin'. Esegui prima 'dotnet build -c Release' (o 'dotnet publish')."
}

$exePath = $candidates[0].FullName
Write-Host "Eseguibile trovato: $exePath"

$fragmentDir = Join-Path $env:LOCALAPPDATA 'Microsoft\Windows Terminal\Fragments\Lare'
$fragmentFile = Join-Path $fragmentDir 'lare-terminal-spike.json'

if (-not (Test-Path $fragmentDir)) {
    New-Item -ItemType Directory -Path $fragmentDir -Force | Out-Null
}

# Struttura minima di un fragment: un array "profiles" con gli stessi campi che
# useresti in settings.json per un profilo normale. "commandline" deve essere un
# path assoluto (o un comando risolvibile) all'eseguibile.
$fragment = [ordered]@{
    profiles = @(
        [ordered]@{
            name             = 'Lare Terminal (spike)'
            commandline      = $exePath
            startingDirectory = '%USERPROFILE%'
            icon             = 'ms-appx:///ProfileIcons/{574e775e-4f2a-5b96-ac1e-a2962a402336}.png'
        }
    )
}

$json = $fragment | ConvertTo-Json -Depth 5
Set-Content -Path $fragmentFile -Value $json -Encoding utf8

Write-Host "Frammento scritto in: $fragmentFile"
Write-Host "Apri Windows Terminal e controlla il menu a tendina dei profili: dovresti vedere 'Lare Terminal (spike)'."
Write-Host "Se Windows Terminal era già aperto, potrebbe servire riavviarlo per far rilevare il nuovo frammento."
