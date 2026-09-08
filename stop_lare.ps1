<#
.SYNOPSIS  Termina i processi di Lare Terminal 2.0 ancora in esecuzione (igiene pre-build).
.DESCRIPTION
  Cerca orchestrator.exe, ui.exe, lare-shell.exe, mcp-server.exe, mcp-nmap.exe, e i plugin
  ping.exe/calc.exe, e li termina — pensato da lanciare prima di una build (`build.ps1`), per
  evitare l'"Accesso negato (os error 5)" di un binario ancora aperto.

  Due modalità, per percorso dell'eseguibile (MAI solo per nome — vedi sotto):

  -TestRun (default): SOLO i processi il cui eseguibile sta dentro "Test Run\" di questo repo —
    il ciclo di sviluppo normale, sicuro per costruzione (qualunque altro "calc"/"ping" nel
    sistema ha un percorso diverso e non viene mai toccato).

  -Release: i processi con questi nomi ovunque SUL FILESYSTEM TRANNE dentro "Test Run\" di
    questo repo — una copia deployata altrove (debug o release non conta: qui "release" indica
    "esecuzione reale, lontana dalla Test Run di sviluppo", non il profilo di compilazione).
    Per ping.exe/calc.exe (nomi che coincidono con la Calcolatrice e l'utility di rete di
    Windows) il confronto qui è SEMPRE sul percorso completo, mai sul nome nudo: un "calc.exe"
    che non sta in una cartella "plugins\calc\" non è il nostro plugin e non viene toccato.
#>
param(
    [switch]$TestRun,
    [switch]$Release
)
$ErrorActionPreference = "Stop"

if ($TestRun -and $Release) { throw "-TestRun e -Release si escludono a vicenda: scegline una (default: -TestRun)." }
if (-not $Release) { $TestRun = $true }  # default

$Repo = $PSScriptRoot
$Names = @("orchestrator", "ui", "lare-shell", "mcp-server", "mcp-nmap", "ping", "calc")
# Nomi che coincidono con binari non-Lare comuni (Calcolatrice, l'utility "ping" di rete):
# per questi il confronto è sempre sul percorso completo, mai sul solo nome processo.
$AmbiguousNames = @("ping", "calc")

# `.Path` su un `Process` può lanciare per un processo che l'utente corrente non può ispezionare
# (elevato, di un altro utente): lo trattiamo come "percorso sconosciuto", mai come errore fatale
# dell'intero script — stesso principio difensivo di `IProcessStarter.IsRunning` (Launcher.cs).
function Get-SafePath($proc) {
    try { return $proc.Path } catch { return $null }
}

$candidates = Get-Process -Name $Names -ErrorAction SilentlyContinue
if (-not $candidates) {
    Write-Host "nessun processo Lare in esecuzione."
    exit 0
}

if ($TestRun) {
    $testRunDir = Join-Path $Repo "Test Run"
    if (-not (Test-Path $testRunDir)) {
        Write-Host "Test Run\ non esiste (mai deployata): nessun processo da terminare."
        exit 0
    }
    $root = (Resolve-Path $testRunDir).Path
    $matched = $candidates | Where-Object {
        $p = Get-SafePath $_
        $p -and $p.StartsWith($root, [System.StringComparison]::OrdinalIgnoreCase)
    }
    Write-Host "modalita: -TestRun (solo processi dentro $root)"
} else {
    $testRunDir = Join-Path $Repo "Test Run"
    $testRunRoot = if (Test-Path $testRunDir) { (Resolve-Path $testRunDir).Path } else { $null }
    $matched = $candidates | Where-Object {
        $p = Get-SafePath $_
        if (-not $p) { return $false }
        if ($testRunRoot -and $p.StartsWith($testRunRoot, [System.StringComparison]::OrdinalIgnoreCase)) { return $false }
        if ($AmbiguousNames -contains $_.ProcessName) {
            return $p -match '\\plugins\\(ping|calc)\\(ping|calc)\.exe$'
        }
        return $true
    }
    Write-Host "modalita: -Release (tutto il filesystem tranne Test Run\)"
}

if (-not $matched) {
    Write-Host "nessun processo corrispondente trovato."
    exit 0
}

foreach ($proc in $matched) {
    $p = Get-SafePath $proc
    Write-Host "termino $($proc.ProcessName).exe (pid $($proc.Id)) — $p"
    try {
        Stop-Process -Id $proc.Id -Force -ErrorAction Stop
    } catch {
        Write-Warning "impossibile terminare $($proc.ProcessName).exe (pid $($proc.Id)): $_"
    }
}
Write-Host "fatto: $($matched.Count) processo/i terminato/i (tentativo)."
