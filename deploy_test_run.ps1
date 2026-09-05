<#
.SYNOPSIS  Copia i binari compilati in "Test Run\" (layout di deploy dentro il repo).
.DESCRIPTION
  target\<BuildConfig>\ -> Test Run\ : orchestrator.exe, mcp-server.exe, mcp-nmap.exe, ui.exe;
  scripts\pytools\ -> Test Run\pytools\ (mai venv/cache);
  con -IncludePlugins: target\<BuildConfig>\<id>.exe -> Test Run\plugins\<id>\<id>.exe
  (i plugin.json sono già committati). Non tocca Test Run\Configuration.
#>
param(
    [ValidateSet("debug", "release")] [string]$BuildConfig = "debug",
    [switch]$IncludePlugins
)
$ErrorActionPreference = "Stop"
$Repo = $PSScriptRoot
$Target = Join-Path $Repo "target\$BuildConfig"
$Dest = Join-Path $Repo "Test Run"
if (-not (Test-Path $Target)) { throw "Manca ${Target}: compila prima (cargo build; cargo build -p ui)." }
foreach ($exe in "orchestrator.exe", "mcp-server.exe", "mcp-nmap.exe", "ui.exe") {
    $src = Join-Path $Target $exe
    if (-not (Test-Path $src)) { throw "Manca $src" }
    Copy-Item $src (Join-Path $Dest $exe) -Force
    Write-Host "copiato $exe"
}
robocopy (Join-Path $Repo "scripts\pytools") (Join-Path $Dest "pytools") /E /XD venv __pycache__ .pytest_cache /XF tickers_us.json fundamentals_cache.json technical_cache.json discoveries.json /NFL /NDL /NJH /NJS | Out-Null
# robocopy usa i bit 0-7 di $LASTEXITCODE per segnalare cosa ha copiato, NON errori:
# 0 = niente da copiare, 1 = copiati file, 2 = file/dir extra nella destinazione,
# 4 = mismatch (dir vs file con lo stesso nome) — tutti "successo", si sommano a
# bitmask (es. 3 = 1+2). Da 8 in su sono veri fallimenti (es. 16 = errore fatale,
# accesso negato) — l'unica soglia corretta per "robocopy è fallita" è >= 8, MAI
# "diverso da 0" (che qui farebbe fallire lo script quasi sempre, anche a copia riuscita).
if ($LASTEXITCODE -ge 8) { throw "robocopy fallita (exit $LASTEXITCODE) copiando pytools\" }
Write-Host "copiato pytools\ (senza venv)"
if ($IncludePlugins) {
    foreach ($id in "ping", "calc") {
        $src = Join-Path $Target "$id.exe"
        if (Test-Path $src) { Copy-Item $src (Join-Path $Dest "plugins\$id\$id.exe") -Force; Write-Host "copiato plugin $id" }
        else { Write-Warning "plugin $id non compilato ($src): saltato" }
    }
}
Write-Host "Test Run pronta: $Dest"
# Azzera $LASTEXITCODE di fine script: senza questo, un robocopy con exit 1-7
# (successo, vedi sopra) lascia comunque un $LASTEXITCODE diverso da zero nella
# sessione chiamante — chi lancia questo script e controlla l'exit code per
# sapere se è andato tutto bene (es. un altro script, o un CI) leggerebbe un
# falso fallimento anche quando "Test Run pronta" è stato stampato sopra.
$global:LASTEXITCODE = 0
