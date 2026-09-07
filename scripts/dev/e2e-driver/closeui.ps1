# Chiude (WindowPattern.Close) tutte le finestre visibili di ui.exe tranne la pagina host
# "Lare Terminal", e stampa i nomi: serve a vedere il terminale sotto e a verificare quali
# finestre il turno ha aperto.
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$p = (Get-Process ui -ErrorAction SilentlyContinue).Id
if (-not $p) { "ui non in esecuzione"; return }
$root = [System.Windows.Automation.AutomationElement]::RootElement
$cond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ProcessIdProperty, $p)
$ws = $root.FindAll([System.Windows.Automation.TreeScope]::Children, $cond)
$names = @()
foreach ($w in $ws) {
    if ($w.Current.Name -and $w.Current.Name -ne 'Lare Terminal') {
        $names += $w.Current.Name
        try { $w.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).Close() } catch { }
    }
}
"finestre ui chiuse: " + ($names -join ' | ')
