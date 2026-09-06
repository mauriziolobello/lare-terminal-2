using System.Management.Automation;
using LareShell.Config;
using LareShell.Host;
using LareShell.Shell;
using LareShell.Tests.Host;
using Xunit;

namespace LareShell.Tests.Shell;

[Collection("runspace")]
public class RunspaceSessionTests
{
    [Fact]
    public void Open_carica_PSReadLine_dai_moduli_di_pwsh()
    {
        using RunspaceSession s = RunspaceSession.Open(new LareHost(new FakeConsoleModes()), HostLog.Null);
        Assert.True(s.PsReadLineAvailable, "PSConsoleHostReadLine assente: PSReadLine non caricato");
        Assert.NotNull(s.PwshDir);

        using var ps = PowerShell.Create();
        ps.Runspace = s.Runspace;
        ps.AddScript("(Get-Module PSReadLine).Path");
        string path = ps.Invoke().Single().BaseObject.ToString()!;
        Assert.StartsWith(s.PwshDir!, path, StringComparison.OrdinalIgnoreCase);
    }

    [Fact]
    public void La_execution_policy_LocalMachine_viene_dal_powershell_config_json_accanto_all_exe()
    {
        // spec §4.4/§13: "risolverla come ConsoleHost, non forzarla". Il file è copiato
        // nell'output (CopyToOutputDirectory) e PowerShell lo legge da $PSHOME.
        Assert.True(File.Exists(Path.Combine(AppContext.BaseDirectory, "powershell.config.json")));
        using RunspaceSession s = RunspaceSession.Open(new LareHost(new FakeConsoleModes()), HostLog.Null);
        using var ps = PowerShell.Create();
        ps.Runspace = s.Runspace;
        ps.AddScript("(Get-ExecutionPolicy -Scope LocalMachine).ToString()");
        Assert.Equal("RemoteSigned", ps.Invoke().Single().BaseObject.ToString());
    }

    [Fact]
    public void CurrentDirectory_segue_Set_Location_del_runspace_non_la_cwd_del_processo()
    {
        using RunspaceSession s = RunspaceSession.Open(new LareHost(new FakeConsoleModes()), HostLog.Null);
        string temp = Path.GetTempPath().TrimEnd('\\');
        using var ps = PowerShell.Create();
        ps.Runspace = s.Runspace;
        ps.AddScript("Set-Location '" + temp + "'").Invoke();
        Assert.Equal(temp, s.CurrentDirectory.TrimEnd('\\'), ignoreCase: true);
    }

    [Fact]
    public void EvaluatePrompt_usa_la_funzione_prompt_dell_utente_con_fallback()
    {
        using RunspaceSession s = RunspaceSession.Open(new LareHost(new FakeConsoleModes()), HostLog.Null);
        Assert.False(string.IsNullOrWhiteSpace(s.EvaluatePrompt()));   // default: "PS C:\…> "
        using var ps = PowerShell.Create();
        ps.Runspace = s.Runspace;
        ps.AddScript("function global:prompt { 'lare> ' }").Invoke();
        Assert.Equal("lare> ", s.EvaluatePrompt());
    }

    [Fact]
    public void PrependModulePath_antepone_una_sola_volta()
    {
        string original = Environment.GetEnvironmentVariable("PSModulePath") ?? string.Empty;
        try
        {
            RunspaceSession.PrependModulePath(@"C:\finta\Modules");
            RunspaceSession.PrependModulePath(@"C:\finta\Modules\");
            string now = Environment.GetEnvironmentVariable("PSModulePath")!;
            Assert.StartsWith(@"C:\finta\Modules;", now);
            Assert.Equal(1, now.Split(';').Count(p => p.TrimEnd('\\').Equals(@"C:\finta\Modules", StringComparison.OrdinalIgnoreCase)));
        }
        finally
        {
            Environment.SetEnvironmentVariable("PSModulePath", original);
        }
    }
}
