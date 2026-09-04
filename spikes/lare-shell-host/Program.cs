using System.Collections.ObjectModel;
using System.Management.Automation;
using System.Management.Automation.Runspaces;

namespace LareShellSpike;

/// <summary>
/// Entry point del processo. Due modalità:
///  - `--selftest`: esegue una serie di controlli non interattivi (utile in CI o
///    per verificare rapidamente, senza una console vera, che l'host funzioni) e
///    ritorna un exit code (0 = tutto ok, 1 = almeno un controllo fallito).
///  - default: avvia il REPL interattivo (Repl.RunInteractive).
/// </summary>
internal static class Program
{
    private static int Main(string[] args)
    {
        // Forziamo UTF-8 in output: senza questo, su console/pipe Windows con
        // codepage non-UTF8 le lettere accentate italiane dei nostri messaggi
        // (è, à, ecc.) vengono mostrate come caratteri corrotti.
        try { Console.OutputEncoding = System.Text.Encoding.UTF8; } catch { /* stdout rediretto verso un file: ignorabile */ }

        if (args.Contains("--selftest"))
        {
            return SelfTest.Run();
        }

        return Repl.RunInteractive();
    }
}

/// <summary>
/// Controlli automatici, pensati per essere eseguiti senza un terminale interattivo
/// reale (niente StatusBar, niente lettura da tastiera): creano una runspace ospitata
/// dallo stesso LareHost usato dal REPL e verificano che i pezzi fondamentali
/// funzionino, stampando una riga [OK]/[FAIL] per ciascun controllo.
/// </summary>
internal static class SelfTest
{
    public static int Run()
    {
        bool allOk = true;

        Console.WriteLine("=== lare-shell-spike --selftest ===");

        // --- Setup: stessa identica costruzione della runspace usata dal REPL ---
        InitialSessionState iss = InitialSessionState.CreateDefault();
        iss.ImportPSModule(new[] { "PSReadLine" });
        // Vedi commento analogo in Repl.cs: senza questo, il caricamento di
        // PSReadLine.psm1 fallisce con "running scripts is disabled".
        iss.ExecutionPolicy = Microsoft.PowerShell.ExecutionPolicy.RemoteSigned;

        var host = new LareHost();
        Runspace? runspace = null;
        try
        {
            runspace = RunspaceFactory.CreateRunspace(host, iss);
            runspace.Open();
            allOk &= Check("Apertura runspace ospitata (LareHost + PSReadLine importato)", true);
        }
        catch (Exception ex)
        {
            allOk &= Check("Apertura runspace ospitata (LareHost + PSReadLine importato)", false, ex.Message);
            Console.WriteLine();
            Console.WriteLine(allOk ? "TUTTI I CONTROLLI SONO PASSATI." : "ALCUNI CONTROLLI SONO FALLITI.");
            return allOk ? 0 : 1;
        }

        try
        {
            // --- Check 1: la funzione PSConsoleHostReadLine esiste dopo l'import ---
            bool hasReadLineFunction = Repl.FunctionExists(runspace, "PSConsoleHostReadLine");
            allOk &= Check(
                "Funzione PSConsoleHostReadLine presente dopo l'import di PSReadLine",
                hasReadLineFunction,
                hasReadLineFunction ? "YES" : "NO");

            // Fatto utile per il report: da dove è stato caricato PSReadLine (e con
            // quale versione). Il PSModulePath di una runspace ospitata NON è
            // necessariamente lo stesso di pwsh.exe: è importante sapere quale copia
            // del modulo è stata effettivamente trovata.
            PrintModuleInfo(runspace);

            // --- Check 2: Get-ChildItem $env:TEMP | Select-Object -First 3 -------
            int itemCount = -1;
            try
            {
                using var ps = PowerShell.Create();
                ps.Runspace = runspace;
                string tempPath = Environment.GetEnvironmentVariable("TEMP") ?? Path.GetTempPath();
                ps.AddCommand("Get-ChildItem").AddParameter("Path", tempPath).AddParameter("ErrorAction", "SilentlyContinue");
                ps.AddCommand("Select-Object").AddParameter("First", 3);
                Collection<PSObject> results = ps.Invoke();
                itemCount = results.Count;
            }
            catch (Exception ex)
            {
                Console.WriteLine("  eccezione durante Get-ChildItem: " + ex.Message);
            }

            allOk &= Check(
                "Get-ChildItem $env:TEMP | Select-Object -First 3 (cattura oggetti)",
                itemCount >= 0,
                "oggetti catturati: " + itemCount);

            // --- Check 3: intercettazione righe che iniziano con '/' --------------
            const string sampleSlashLine = "/help";
            bool intercepted = Repl.TryIntercept(sampleSlashLine, out string interceptMessage);
            allOk &= Check(
                "Intercettazione riga '" + sampleSlashLine + "' tramite la stessa funzione usata dal REPL",
                intercepted,
                interceptMessage);

            // Controllo di non-regressione simmetrico: una riga che NON inizia con
            // '/' non deve essere intercettata.
            bool notIntercepted = !Repl.TryIntercept("Get-Date", out _);
            allOk &= Check("Riga 'Get-Date' (senza '/') NON intercettata", notIntercepted);
        }
        finally
        {
            runspace.Close();
            runspace.Dispose();
        }

        Console.WriteLine();
        Console.WriteLine(allOk ? "TUTTI I CONTROLLI SONO PASSATI." : "ALCUNI CONTROLLI SONO FALLITI.");
        return allOk ? 0 : 1;
    }

    private static void PrintModuleInfo(Runspace runspace)
    {
        try
        {
            using var ps = PowerShell.Create();
            ps.Runspace = runspace;
            ps.AddCommand("Get-Module").AddParameter("Name", "PSReadLine");
            ps.AddCommand("Select-Object").AddParameter("Property", new[] { "Name", "Version", "Path" });
            Collection<PSObject> results = ps.Invoke();
            if (results.Count == 0)
            {
                Console.WriteLine("  Get-Module PSReadLine: nessun modulo caricato (inatteso se il check precedente e' YES).");
                return;
            }

            foreach (PSObject obj in results)
            {
                string name = obj.Properties["Name"]?.Value?.ToString() ?? "?";
                string version = obj.Properties["Version"]?.Value?.ToString() ?? "?";
                string path = obj.Properties["Path"]?.Value?.ToString() ?? "?";
                Console.WriteLine("  Get-Module PSReadLine -> Name=" + name + " Version=" + version + " Path=" + path);
            }
        }
        catch (Exception ex)
        {
            Console.WriteLine("  Get-Module PSReadLine: eccezione " + ex.Message);
        }
    }

    private static bool Check(string description, bool ok, string? detail = null)
    {
        string tag = ok ? "[OK]" : "[FAIL]";
        string suffix = detail is null ? string.Empty : " (" + detail + ")";
        Console.WriteLine(tag + " " + description + suffix);
        return ok;
    }
}
