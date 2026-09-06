using Microsoft.Win32;

namespace LareShell.Shell;

/// <summary>
/// Trova la cartella di installazione di pwsh (es. C:\Program Files\PowerShell\7). Serve per
/// PSReadLine: il NuGet Microsoft.PowerShell.SDK NON lo include (spec §7) e su questa macchina
/// <c>&lt;pwsh&gt;\Modules</c> non è nel PSModulePath di macchina/utente — è pwsh.exe stesso ad
/// aggiungere il proprio <c>$PSHOME\Modules</c> al PSModulePath del suo processo. Una host che
/// gira nuda in Windows Terminal non lo eredita: lo aggiungiamo noi (RunspaceSession.Open).
/// Ordine: PATH → registro (HKLM\SOFTWARE\Microsoft\PowerShellCore\InstalledVersions\*\InstallLocation)
/// → C:\Program Files\PowerShell\7. Leggere PATH/registro NON è configurazione Lare (D6).
/// </summary>
internal static class PwshLocator
{
    /// <summary>Parte pura, testabile: scandisce un PATH dato con un predicato di esistenza.</summary>
    public static string? FindInstallDir(string? pathEnv, Func<string, bool> fileExists)
    {
        if (string.IsNullOrEmpty(pathEnv))
        {
            return null;
        }

        foreach (string raw in pathEnv.Split(';', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries))
        {
            string dir = raw.Trim('"');
            if (dir.Length == 0)
            {
                continue;
            }

            if (fileExists(Path.Combine(dir, "pwsh.exe")))
            {
                return dir;
            }
        }

        return null;
    }

    public static string? FindInstallDir()
    {
        string? fromPath = FindInstallDir(Environment.GetEnvironmentVariable("PATH"), File.Exists);
        if (fromPath is not null)
        {
            return fromPath;
        }

        if (OperatingSystem.IsWindows())
        {
            try
            {
                using RegistryKey? versions = Registry.LocalMachine.OpenSubKey(@"SOFTWARE\Microsoft\PowerShellCore\InstalledVersions");
                foreach (string sub in versions?.GetSubKeyNames() ?? Array.Empty<string>())
                {
                    using RegistryKey? key = versions!.OpenSubKey(sub);
                    if (key?.GetValue("InstallLocation") is string loc && File.Exists(Path.Combine(loc, "pwsh.exe")))
                    {
                        return loc.TrimEnd('\\');
                    }
                }
            }
            catch (Exception ex) when (ex is System.Security.SecurityException or IOException or UnauthorizedAccessException)
            {
                // Registro non leggibile: si passa al fallback.
            }
        }

        string fallback = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles), "PowerShell", "7");
        return File.Exists(Path.Combine(fallback, "pwsh.exe")) ? fallback : null;
    }
}
