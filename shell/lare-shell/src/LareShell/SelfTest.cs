using System.Management.Automation;
using LareShell.Config;
using LareShell.Host;
using LareShell.Shell;

namespace LareShell;

/// <summary>
/// <c>--selftest</c>: controlli non interattivi (nessuna tastiera, nessun orchestratore) con
/// exit code 0/1, per verificare un deploy (Test Run\shell\lare-shell.exe --selftest) o in CI.
/// Stampa una riga [OK]/[FAIL] per controllo, come lo spike.
/// </summary>
internal static class SelfTest
{
    public static int Run(string configDir, StartupConfig cfg)
    {
        bool allOk = true;
        Console.WriteLine("=== lare-shell " + HostInfo.Version + " --selftest ===");
        allOk &= Check("Cartella di configurazione: " + configDir, Directory.Exists(configDir));
        allOk &= Check("startup.json letto (ws_port " + cfg.WsPort + ")", cfg.Warnings.Count == 0, string.Join("; ", cfg.Warnings));
        allOk &= Check("powershell.config.json accanto all'exe", File.Exists(Path.Combine(AppContext.BaseDirectory, "powershell.config.json")));

        string? pwsh = PwshLocator.FindInstallDir();
        allOk &= Check("pwsh trovato", pwsh is not null, pwsh);

        try
        {
            using RunspaceSession s = RunspaceSession.Open(new LareHost(), HostLog.Null);
            allOk &= Check("Runspace aperta", true);
            allOk &= Check("PSReadLine (PSConsoleHostReadLine)", s.PsReadLineAvailable);
            using var ps = PowerShell.Create();
            ps.Runspace = s.Runspace;
            string policy = ps.AddScript("(Get-ExecutionPolicy -Scope LocalMachine).ToString()").Invoke().Single().BaseObject.ToString()!;
            allOk &= Check("Execution policy LocalMachine = RemoteSigned", policy == "RemoteSigned", policy);
        }
        catch (Exception ex)
        {
            allOk &= Check("Runspace aperta", false, ex.Message);
        }

        Console.WriteLine();
        Console.WriteLine(allOk ? "TUTTI I CONTROLLI SONO PASSATI." : "ALCUNI CONTROLLI SONO FALLITI.");
        return allOk ? 0 : 1;
    }

    private static bool Check(string description, bool ok, string? detail = null)
    {
        Console.WriteLine((ok ? "[OK]   " : "[FAIL] ") + description + (string.IsNullOrEmpty(detail) ? string.Empty : " (" + detail + ")"));
        return ok;
    }
}
