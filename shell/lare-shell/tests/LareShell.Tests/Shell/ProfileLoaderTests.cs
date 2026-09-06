using System.Management.Automation;
using LareShell.Config;
using LareShell.Host;
using LareShell.Shell;
using LareShell.Tests.Host;
using Xunit;

namespace LareShell.Tests.Shell;

public class ProfilePathsTests
{
    [Fact]
    public void Compute_da_i_cinque_percorsi_con_i_nomi_di_pwsh_e_della_host()
    {
        ProfileLoader.ProfilePaths p = ProfileLoader.Compute(@"C:\Users\u\Documents", @"C:\Program Files\PowerShell\7", @"C:\Lare\shell");
        Assert.Equal(@"C:\Program Files\PowerShell\7\profile.ps1", p.AllUsersAllHosts);
        Assert.Equal(@"C:\Program Files\PowerShell\7\LareShell_profile.ps1", p.AllUsersCurrentHost);
        Assert.Equal(@"C:\Users\u\Documents\PowerShell\profile.ps1", p.CurrentUserAllHosts);
        Assert.Equal(@"C:\Users\u\Documents\PowerShell\LareShell_profile.ps1", p.CurrentUserCurrentHost);
        Assert.Equal(@"C:\Users\u\Documents\PowerShell\Microsoft.PowerShell_profile.ps1", p.PwshCurrentHost);
    }

    [Fact]
    public void Senza_pwsh_i_profili_AllUsers_puntano_alla_cartella_dell_app()
    {
        ProfileLoader.ProfilePaths p = ProfileLoader.Compute(@"C:\Users\u\Documents", null, @"C:\Lare\shell");
        Assert.Equal(@"C:\Lare\shell\profile.ps1", p.AllUsersAllHosts);
    }

    [Fact]
    public void LoadOrder_e_profile_poi_quello_di_pwsh_poi_quello_della_host()
    {
        // spec §4.4: profile.ps1, Microsoft.PowerShell_profile.ps1 (raccomandazione: alias e
        // oh-my-posh dell'utente appaiono in Lare come in pwsh), LareShell_profile.ps1.
        ProfileLoader.ProfilePaths p = ProfileLoader.Compute(@"C:\d", @"C:\p", @"C:\a");
        Assert.Equal(new[] { p.CurrentUserAllHosts, p.PwshCurrentHost, p.CurrentUserCurrentHost }, ProfileLoader.LoadOrder(p));
    }
}

[Collection("runspace")]
public class ProfileLoaderRunspaceTests
{
    [Fact]
    public void Load_dot_sourcia_i_profili_esistenti_in_ordine_e_continua_dopo_un_errore()
    {
        string docs = Path.Combine(Path.GetTempPath(), "lare-prof-" + Guid.NewGuid().ToString("N"));
        string dir = Path.Combine(docs, "PowerShell");
        Directory.CreateDirectory(dir);
        try
        {
            File.WriteAllText(Path.Combine(dir, "profile.ps1"), "function global:lare_t1 { 1 }\r\n$global:lare_order = 'a'\r\n");
            File.WriteAllText(Path.Combine(dir, "Microsoft.PowerShell_profile.ps1"), "$global:lare_order += 'b'\r\nGet-Item 'C:\\__lare_non_esiste__'\r\nfunction global:lare_t2 { 2 }\r\n");
            File.WriteAllText(Path.Combine(dir, "LareShell_profile.ps1"), "$global:lare_order += 'c'\r\n");

            var host = new LareHost(new FakeConsoleModes());
            using RunspaceSession s = RunspaceSession.Open(host, HostLog.Null);
            ProfileLoader.ProfilePaths p = ProfileLoader.Compute(docs, s.PwshDir, AppContext.BaseDirectory);

            host.HostUI.Recorder.Begin();
            IReadOnlyList<string> loaded = ProfileLoader.Load(s.Runspace, host, p, File.Exists);
            string printed = host.HostUI.Recorder.End();

            Assert.Equal(3, loaded.Count);
            Assert.True(RunspaceSession.FunctionExists(s.Runspace, "lare_t1"));
            Assert.True(RunspaceSession.FunctionExists(s.Runspace, "lare_t2"), "l'errore non terminante non deve fermare il profilo");
            Assert.Equal("abc", s.Runspace.SessionStateProxy.GetVariable("lare_order"));
            Assert.Contains("__lare_non_esiste__", printed);   // l'errore è stato mostrato, non inghiottito
        }
        finally
        {
            Directory.Delete(docs, recursive: true);
        }
    }

    [Fact]
    public void Load_salta_i_profili_assenti_senza_errori()
    {
        var host = new LareHost(new FakeConsoleModes());
        using RunspaceSession s = RunspaceSession.Open(host, HostLog.Null);
        ProfileLoader.ProfilePaths p = ProfileLoader.Compute(@"C:\__nessuna_cartella__", null, AppContext.BaseDirectory);
        Assert.Empty(ProfileLoader.Load(s.Runspace, host, p, File.Exists));
    }

    [Fact]
    public void SetDollarProfile_espone_la_stringa_e_le_quattro_NoteProperty_di_pwsh()
    {
        using RunspaceSession s = RunspaceSession.Open(new LareHost(new FakeConsoleModes()), HostLog.Null);
        ProfileLoader.ProfilePaths p = ProfileLoader.Compute(@"C:\d", @"C:\p", @"C:\a");
        ProfileLoader.SetDollarProfile(s.Runspace, p);

        using var ps = PowerShell.Create();
        ps.Runspace = s.Runspace;
        ps.AddScript("\"$PROFILE|$($PROFILE.AllUsersAllHosts)|$($PROFILE.AllUsersCurrentHost)|$($PROFILE.CurrentUserAllHosts)|$($PROFILE.CurrentUserCurrentHost)\"");
        string got = ps.Invoke().Single().BaseObject.ToString()!;
        Assert.Equal(@"C:\d\PowerShell\LareShell_profile.ps1|C:\p\profile.ps1|C:\p\LareShell_profile.ps1|C:\d\PowerShell\profile.ps1|C:\d\PowerShell\LareShell_profile.ps1", got);
    }
}
