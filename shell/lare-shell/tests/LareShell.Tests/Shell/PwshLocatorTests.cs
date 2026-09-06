using LareShell.Shell;
using Xunit;

namespace LareShell.Tests.Shell;

public class PwshLocatorTests
{
    [Fact]
    public void Trova_la_prima_cartella_del_PATH_che_contiene_pwsh_exe()
    {
        string path = @"C:\Windows;C:\Program Files\PowerShell\7;C:\altro\pwsh";
        string? dir = PwshLocator.FindInstallDir(path, f => f.Equals(@"C:\Program Files\PowerShell\7\pwsh.exe", StringComparison.OrdinalIgnoreCase));
        Assert.Equal(@"C:\Program Files\PowerShell\7", dir);
    }

    [Fact]
    public void Ignora_voci_vuote_e_virgolette()
    {
        string path = @";""C:\pw"";";
        string? dir = PwshLocator.FindInstallDir(path, f => f.Equals(@"C:\pw\pwsh.exe", StringComparison.OrdinalIgnoreCase));
        Assert.Equal(@"C:\pw", dir);
    }

    [Fact]
    public void Null_se_nessuna_cartella_ha_pwsh()
    {
        Assert.Null(PwshLocator.FindInstallDir(@"C:\a;C:\b", _ => false));
        Assert.Null(PwshLocator.FindInstallDir(null, _ => true));
    }

    [Fact]
    public void Su_questa_macchina_pwsh_7_viene_trovato()
    {
        // Prerequisito dichiarato (spec §7): pwsh 7.6+ installato. Se fallisce qui, fallisce tutto il resto.
        string? dir = PwshLocator.FindInstallDir();
        Assert.NotNull(dir);
        Assert.True(File.Exists(Path.Combine(dir!, "pwsh.exe")));
        Assert.True(Directory.Exists(Path.Combine(dir!, "Modules", "PSReadLine")), "manca Modules\\PSReadLine in " + dir);
    }
}
