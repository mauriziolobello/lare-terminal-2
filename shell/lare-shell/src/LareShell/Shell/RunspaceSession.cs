using System.Collections.ObjectModel;
using System.Management.Automation;
using System.Management.Automation.Runspaces;
using LareShell.Config;
using LareShell.Host;

namespace LareShell.Shell;

/// <summary>
/// La runspace ospitata (il "motore" PowerShell dentro il nostro processo) con tutto ciò che il
/// REPL le chiede: PSReadLine per leggere le righe (come pwsh.exe: invoca la funzione
/// PSConsoleHostReadLine), la funzione <c>prompt</c> dell'utente, la cwd del runspace.
/// Possiede la runspace: <c>Dispose</c> la chiude. Va aperta SUL thread del REPL
/// (Runspace.DefaultRunspace è per thread).
/// </summary>
internal sealed class RunspaceSession : IDisposable
{
    private RunspaceSession(LareHost host, Runspace runspace, bool psReadLineAvailable, string? pwshDir)
    {
        Host = host;
        Runspace = runspace;
        PsReadLineAvailable = psReadLineAvailable;
        PwshDir = pwshDir;
    }

    public LareHost Host { get; }

    public Runspace Runspace { get; }

    public bool PsReadLineAvailable { get; }

    /// <summary>Cartella di pwsh trovata da PwshLocator (null = non trovata: niente PSReadLine).</summary>
    public string? PwshDir { get; }

    /// <summary>cwd DEL RUNSPACE (spec §4.6): è quella che vede Set-Location, non
    /// Directory.GetCurrentDirectory() del processo (le due possono divergere).</summary>
    public string CurrentDirectory => Runspace.SessionStateProxy.Path.CurrentLocation.ProviderPath;

    public static RunspaceSession Open(LareHost host, HostLog log)
    {
        // 1. PSReadLine vive nei moduli di pwsh, non nel NuGet: anteponiamo la sua cartella al
        //    PSModulePath del PROCESSO prima di creare la runspace (il motore calcola il proprio
        //    PSModulePath all'apertura, partendo da quello del processo).
        string? pwshDir = PwshLocator.FindInstallDir();
        if (pwshDir is not null)
        {
            PrependModulePath(Path.Combine(pwshDir, "Modules"));
        }
        else
        {
            log.Warn("pwsh non trovato (PATH/registro/Program Files): PSReadLine non disponibile");
        }

        // 2. Stato iniziale come ConsoleHost: moduli di default + PSReadLine importato all'apertura.
        //    NESSUN iss.ExecutionPolicy: la policy LocalMachine viene da powershell.config.json
        //    accanto all'exe (Global Constraints, ADR-019).
        InitialSessionState iss = InitialSessionState.CreateDefault();
        iss.ImportPSModule(new[] { "PSReadLine" });

        Runspace runspace = RunspaceFactory.CreateRunspace(host, iss);
        runspace.Open();
        // Molte API "ambient" del motore assumono una runspace di default per il thread corrente.
        Runspace.DefaultRunspace = runspace;

        bool psReadLine = FunctionExists(runspace, "PSConsoleHostReadLine");
        log.Info("runspace aperta; pwsh=" + (pwshDir ?? "?") + " PSReadLine=" + psReadLine);
        return new RunspaceSession(host, runspace, psReadLine, pwshDir);
    }

    /// <summary>Antepone <paramref name="modulesDir"/> al PSModulePath del processo, una sola volta.</summary>
    internal static void PrependModulePath(string modulesDir)
    {
        string wanted = modulesDir.TrimEnd('\\');
        string current = Environment.GetEnvironmentVariable("PSModulePath") ?? string.Empty;
        bool present = current.Split(';', StringSplitOptions.RemoveEmptyEntries)
            .Any(p => p.TrimEnd('\\').Equals(wanted, StringComparison.OrdinalIgnoreCase));
        if (!present)
        {
            Environment.SetEnvironmentVariable("PSModulePath", current.Length == 0 ? wanted : wanted + ";" + current);
        }
    }

    public static bool FunctionExists(Runspace runspace, string name)
    {
        try
        {
            using var ps = PowerShell.Create();
            ps.Runspace = runspace;
            ps.AddCommand("Get-Command").AddParameter("Name", name).AddParameter("ErrorAction", "SilentlyContinue");
            return ps.Invoke().Count > 0;
        }
        catch
        {
            return false;
        }
    }

    /// <summary>Valuta la funzione <c>prompt</c> (default del motore o ridefinita dal profilo),
    /// come ConsoleHost.EvaluatePrompt; fallback "PS &lt;cwd&gt;&gt; ".</summary>
    public string EvaluatePrompt()
    {
        try
        {
            using var ps = PowerShell.Create();
            ps.Runspace = Runspace;
            Collection<PSObject> result = ps.AddCommand("prompt").Invoke();
            if (result.Count > 0 && result[0].BaseObject is string text && text.Length > 0)
            {
                return text;
            }
        }
        catch
        {
            // prompt rotto dal profilo: fallback sotto.
        }

        return "PS " + CurrentDirectory + "> ";
    }

    /// <summary>Legge una riga: PSReadLine (PSConsoleHostReadLine) se disponibile e richiesto,
    /// altrimenti Console.ReadLine. <c>null</c> = EOF. Lezione dello spike: con stdin rediretto
    /// PSConsoleHostReadLine non dà mai EOF (loop infinito) → il chiamante passa
    /// <paramref name="usePsReadLine"/> = false in quel caso.</summary>
    public string? ReadLine(bool usePsReadLine)
    {
        if (usePsReadLine && PsReadLineAvailable)
        {
            try
            {
                using var ps = PowerShell.Create();
                ps.Runspace = Runspace;
                Collection<PSObject> result = ps.AddCommand("PSConsoleHostReadLine").Invoke();
                // 0 risultati = Ctrl+C durante l'editing: riga vuota, non EOF.
                return result.Count == 1 ? result[0].BaseObject as string ?? string.Empty : string.Empty;
            }
            catch (Exception ex)
            {
                Host.UI.WriteWarningLine("PSConsoleHostReadLine ha fallito (" + ex.GetType().Name + "): fallback a Console.ReadLine per questa riga");
            }
        }

        return Console.ReadLine();
    }

    public void Dispose()
    {
        Runspace.DefaultRunspace = null;
        Runspace.Dispose();
    }
}
