<#
.SYNOPSIS  Rimuove il profilo "Lare Terminal" da Windows Terminal (il fragment scritto da install-wt-profile.ps1).
#>
$ErrorActionPreference = "Stop"
$fragmentPath = Join-Path $env:LOCALAPPDATA "Microsoft\Windows Terminal\Fragments\Lare\lare-terminal.json"
if (Test-Path $fragmentPath) {
    Remove-Item $fragmentPath -Force
    Write-Host "Profilo 'Lare Terminal' rimosso ($fragmentPath). Riavvia Windows Terminal."
} else {
    Write-Host "Nessun profilo da rimuovere ($fragmentPath non esiste)."
}
