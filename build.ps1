<#
.SYNOPSIS  Compila Lare Terminal 2.0 (Rust + host C#) — pronta per deploy_test_run.ps1.
.DESCRIPTION
  cargo build [--release] (default-members: orchestrator, mcp-server, mcp-nmap, startup-config,
  protocol) + cargo build [--release] -p ui (salta con -SkipUi, lento la prima volta) +
  cargo build [--release] -p plugin-ping -p plugin-calc (solo con -IncludePlugins) +
  dotnet build di shell\lare-shell\LareShell.sln (salta con -SkipShell). La host C# compila
  sempre in Debug (dotnet build): non ha una build "release" utile da sorgente — solo il publish
  di deploy_test_run.ps1 lo è (BUILD.md). Non popola Test Run\: dopo questo script, esegui
  .\deploy_test_run.ps1 (stesso -BuildConfig) per quello.
#>
param(
    [ValidateSet("debug", "release")] [string]$BuildConfig = "debug",
    [switch]$SkipUi,
    [switch]$IncludePlugins,
    [switch]$SkipShell,
    # cargo clean -p ui prima di compilare: serve solo se una modifica al SOLO frontend
    # (crates/ui/frontend/) non sembra avere effetto — gotcha di generate_context! (BUILD.md).
    [switch]$CleanUi
)
$ErrorActionPreference = "Stop"
$Repo = $PSScriptRoot
$CargoFlags = @()
if ($BuildConfig -eq "release") { $CargoFlags += "--release" }

Write-Host "== cargo build $($CargoFlags -join ' ') (default-members) =="
& cargo build @CargoFlags
if ($LASTEXITCODE -ne 0) { throw "cargo build fallita (exit $LASTEXITCODE)" }
Write-Host "compilato: orchestrator, mcp-server, mcp-nmap, startup-config, protocol"

if ($CleanUi) {
    Write-Host "== cargo clean -p ui =="
    & cargo clean -p ui
}

if (-not $SkipUi) {
    Write-Host "== cargo build $($CargoFlags -join ' ') -p ui =="
    & cargo build @CargoFlags -p ui
    if ($LASTEXITCODE -ne 0) { throw "cargo build -p ui fallita (exit $LASTEXITCODE)" }
    Write-Host "compilato: ui"
} else {
    Write-Host "saltato: ui (-SkipUi)"
}

if ($IncludePlugins) {
    Write-Host "== cargo build $($CargoFlags -join ' ') -p plugin-ping -p plugin-calc =="
    & cargo build @CargoFlags -p plugin-ping -p plugin-calc
    if ($LASTEXITCODE -ne 0) { throw "cargo build dei plugin fallita (exit $LASTEXITCODE)" }
    Write-Host "compilato: plugin-ping, plugin-calc"
}

if (-not $SkipShell) {
    $sln = Join-Path $Repo "shell\lare-shell\LareShell.sln"
    Write-Host "== dotnet build $sln =="
    & dotnet build $sln --nologo -v quiet
    if ($LASTEXITCODE -ne 0) { throw "dotnet build della host fallita (exit $LASTEXITCODE)" }
    Write-Host "compilata: lare-shell (host C#)"
} else {
    Write-Host "saltato: lare-shell (-SkipShell)"
}

Write-Host "Build pronta (config: $BuildConfig)."
Write-Host "Prossimo passo: .\deploy_test_run.ps1 -BuildConfig $BuildConfig$(if ($IncludePlugins) { ' -IncludePlugins' })"
# Vedi deploy_test_run.ps1 per lo stesso accorgimento: azzera l'exit code di fine script così
# chi lo chiama (un altro script, una CI) non legga un falso fallimento residuo.
$global:LASTEXITCODE = 0
