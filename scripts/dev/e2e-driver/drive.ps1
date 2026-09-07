# Driver e2e per la scheda "Lare Terminal" di Windows Terminal: attiva la finestra dal titolo,
# invia tasti (SendKeys) e cattura screenshot della finestra in primo piano.
param(
    [ValidateSet("start", "type", "shot", "keys")] [string]$Cmd,
    [string]$Text = "",
    [string]$Out = "",
    [string]$Title = "LareE2E",
    # Eseguibile da lanciare nella nuova scheda WT: default la host pubblicata in Test Run\shell\.
    [string]$Exe = (Join-Path (Split-Path (Split-Path (Split-Path $PSScriptRoot -Parent) -Parent) -Parent) "Test Run\shell\lare-shell.exe")
)
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName Microsoft.VisualBasic
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class Win {
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
"@

Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class Fg {
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
  public static string Title(){ var sb=new StringBuilder(256); GetWindowText(GetForegroundWindow(), sb, 256); return sb.ToString(); }
}
"@

# Porta in primo piano la finestra il cui titolo inizia con $Title e VERIFICA che ci sia riuscita:
# AppActivate da solo fallisce in silenzio quando un'altra app (es. la finestra di ui.exe) ha il
# fuoco, e i tasti finirebbero li'. Tre tentativi: AppActivate, poi UIA SetFocus, poi il trucco
# del tasto ALT (sblocca SetForegroundWindow) + SetForegroundWindow sull'hwnd trovato via UIA.
function Activate {
    for ($i = 0; $i -lt 3; $i++) {
        try { [Microsoft.VisualBasic.Interaction]::AppActivate($Title) } catch { }
        Start-Sleep -Milliseconds 300
        if ([Fg]::Title().StartsWith($Title)) { return }
        $root = [System.Windows.Automation.AutomationElement]::RootElement
        $cond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, $Title)
        $w = $root.FindFirst([System.Windows.Automation.TreeScope]::Children, $cond)
        if ($w) {
            try { $w.SetFocus() } catch { }
            Start-Sleep -Milliseconds 300
            if ([Fg]::Title().StartsWith($Title)) { return }
            [Fg]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero); [Fg]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero)
            [Fg]::SetForegroundWindow([IntPtr]$w.Current.NativeWindowHandle) | Out-Null
            Start-Sleep -Milliseconds 300
            if ([Fg]::Title().StartsWith($Title)) { return }
        }
    }
    throw "impossibile portare in primo piano '$Title' (foreground: '$([Fg]::Title())')"
}

switch ($Cmd) {
    "start" {
        # Nuova finestra WT con il profilo "Lare Terminal" e titolo di scheda fisso (per AppActivate).
        # Una sola stringa: con l'array Start-Process NON quota gli argomenti con spazi
        # (wt riceveva `-p Lare` e provava ad avviare `Terminal`).
        # Il fragment del profilo viene letto da WT solo al suo avvio: la finestra WT dell'utente
        # era gia' aperta, quindi `-p "Lare Terminal"` cade sul profilo di default. Avviamo
        # direttamente l'exe (stessa cosa che fa il profilo: host nuda in una scheda WT).
        if (-not (Test-Path $Exe)) { throw "manca $Exe (esegui deploy_test_run.ps1)" }
        Start-Process wt.exe -ArgumentList "-w new --title $Title --suppressApplicationTitle -d `"$env:USERPROFILE`" `"$Exe`""
        Start-Sleep -Seconds 2
        Activate
        "started"
    }
    "type" {
        Activate
        [System.Windows.Forms.SendKeys]::SendWait($Text)
        [System.Windows.Forms.SendKeys]::SendWait("{ENTER}")
        "typed"
    }
    "keys" {
        Activate
        [System.Windows.Forms.SendKeys]::SendWait($Text)
        "sent"
    }
    "shot" {
        Activate
        $h = [Win]::GetForegroundWindow()
        $r = New-Object Win+RECT
        [Win]::GetWindowRect($h, [ref]$r) | Out-Null
        $w = $r.R - $r.L; $hh = $r.B - $r.T
        $bmp = New-Object System.Drawing.Bitmap $w, $hh
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        $g.CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size)
        $bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
        $g.Dispose(); $bmp.Dispose()
        "shot $w x $hh -> $Out"
    }
}
