using System.Management.Automation;
using System.Management.Automation.Host;
using System.Management.Automation.Runspaces;

namespace LareShell.Shell;

/// <summary>
/// I profili (spec §4.4). In una host custom $PROFILE NON viene popolato dal motore (lo fa
/// ConsoleHost stesso, HostUtilities.GetDollarProfile è internal): calcoliamo i percorsi con la
/// stessa convenzione di pwsh (Documents\PowerShell, nome file da $Host.Name) e li esponiamo come
/// pwsh (stringa = CurrentUserCurrentHost + 4 NoteProperty). Carichiamo, in ordine:
/// profile.ps1 · Microsoft.PowerShell_profile.ps1 (quello di pwsh: alias, oh-my-posh, moduli
/// dell'utente appaiono in Lare come in pwsh) · LareShell_profile.ps1. I profili AllUsers (nel
/// $PSHOME di pwsh) NON vengono caricati — debito dichiarato in HANDOFF.
/// </summary>
internal static class ProfileLoader
{
    internal sealed record ProfilePaths(
        string AllUsersAllHosts,
        string AllUsersCurrentHost,
        string CurrentUserAllHosts,
        string CurrentUserCurrentHost,
        string PwshCurrentHost);

    public static ProfilePaths Compute(string documentsDir, string? pwshDir, string appDir)
    {
        string userDir = Path.Combine(documentsDir, "PowerShell");
        string allUsersDir = pwshDir ?? appDir;
        return new ProfilePaths(
            AllUsersAllHosts: Path.Combine(allUsersDir, "profile.ps1"),
            AllUsersCurrentHost: Path.Combine(allUsersDir, HostInfo.Name + "_profile.ps1"),
            CurrentUserAllHosts: Path.Combine(userDir, "profile.ps1"),
            CurrentUserCurrentHost: Path.Combine(userDir, HostInfo.Name + "_profile.ps1"),
            PwshCurrentHost: Path.Combine(userDir, "Microsoft.PowerShell_profile.ps1"));
    }

    public static IReadOnlyList<string> LoadOrder(ProfilePaths p) =>
        new[] { p.CurrentUserAllHosts, p.PwshCurrentHost, p.CurrentUserCurrentHost };

    /// <summary>$PROFILE come in pwsh: la stringa è CurrentUserCurrentHost, con 4 NoteProperty.</summary>
    public static void SetDollarProfile(Runspace runspace, ProfilePaths p)
    {
        PSObject profile = PSObject.AsPSObject(p.CurrentUserCurrentHost);
        profile.Properties.Add(new PSNoteProperty("AllUsersAllHosts", p.AllUsersAllHosts));
        profile.Properties.Add(new PSNoteProperty("AllUsersCurrentHost", p.AllUsersCurrentHost));
        profile.Properties.Add(new PSNoteProperty("CurrentUserAllHosts", p.CurrentUserAllHosts));
        profile.Properties.Add(new PSNoteProperty("CurrentUserCurrentHost", p.CurrentUserCurrentHost));
        runspace.SessionStateProxy.SetVariable("PROFILE", profile);
    }

    /// <summary>Dot-source dei profili esistenti nell'ordine di <see cref="LoadOrder"/>. Un errore
    /// in un profilo viene mostrato (Out-Default, come ConsoleHost.RunProfile) e si prosegue col
    /// successivo. Ritorna i percorsi effettivamente caricati.</summary>
    public static IReadOnlyList<string> Load(Runspace runspace, PSHost host, ProfilePaths p, Func<string, bool> exists)
    {
        var loaded = new List<string>();
        foreach (string path in LoadOrder(p))
        {
            if (!exists(path))
            {
                continue;
            }

            try
            {
                using var ps = PowerShell.Create();
                ps.Runspace = runspace;
                // ". '<path>'" = dot-sourcing: esegue nello scope corrente (le funzioni definite
                // restano); l'apice è raddoppiato per i percorsi con apostrofi. Error→Output +
                // Out-Default: gli errori non terminanti si vedono in rosso e non fermano il profilo.
                ps.AddScript(". '" + path.Replace("'", "''") + "'", useLocalScope: false);
                ps.Commands.Commands[0].MergeMyResults(PipelineResultTypes.Error, PipelineResultTypes.Output);
                ps.AddCommand("Out-Default");
                ps.Invoke();
                loaded.Add(path);
            }
            catch (RuntimeException ex)
            {
                host.UI.WriteErrorLine("Errore nel profilo " + path + ": " + ex.Message);
            }
        }

        return loaded;
    }
}
