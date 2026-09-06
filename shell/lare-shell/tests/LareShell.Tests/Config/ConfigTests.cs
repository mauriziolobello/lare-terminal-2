using LareShell.Config;
using Xunit;

namespace LareShell.Tests.Config;

public class CliArgsTests
{
    [Fact]
    public void Parse_legge_config_dir_session_e_selftest()
    {
        CliArgs a = CliArgs.Parse(new[] { "--config-dir", @"D:\cfg", "--session", "abc", "--selftest" });
        Assert.Equal(@"D:\cfg", a.ConfigDir);
        Assert.Equal("abc", a.SessionId);
        Assert.True(a.SelfTest);
    }

    [Fact]
    public void Parse_senza_argomenti_da_tutti_null_e_false()
    {
        CliArgs a = CliArgs.Parse(Array.Empty<string>());
        Assert.Null(a.ConfigDir);
        Assert.Null(a.SessionId);
        Assert.False(a.SelfTest);
    }

    [Fact]
    public void Parse_ignora_config_dir_senza_valore_in_coda()
    {
        CliArgs a = CliArgs.Parse(new[] { "--config-dir" });
        Assert.Null(a.ConfigDir);
    }
}

public class ConfigDirTests
{
    [Fact]
    public void Resolve_senza_flag_usa_la_cartella_Configuration_accanto_alla_cartella_dell_exe()
    {
        // spec §6.1: lare-shell.exe vive in <deploy>\shell\ → <deploy>\Configuration\
        string dir = ConfigDir.Resolve(null, @"C:\Lare\shell\");
        Assert.Equal(@"C:\Lare\Configuration", dir);
    }

    [Fact]
    public void Resolve_con_flag_usa_il_valore_dato()
    {
        Assert.Equal(@"D:\cfg", ConfigDir.Resolve(@"D:\cfg", @"C:\Lare\shell\"));
    }

    [Fact]
    public void DeployRoot_e_il_padre_della_cartella_di_configurazione()
    {
        Assert.Equal(@"C:\Lare", ConfigDir.DeployRoot(@"C:\Lare\Configuration"));
    }
}

public class StartupConfigTests
{
    [Fact]
    public void Parse_null_da_i_default()
    {
        StartupConfig c = StartupConfig.Parse(null);
        Assert.Equal(7331, c.WsPort);
        Assert.True(c.AutostartOrchestrator);
        Assert.True(c.AutostartUi);
        Assert.Empty(c.Warnings);
    }

    [Fact]
    public void Parse_legge_ws_port_e_autostart()
    {
        StartupConfig c = StartupConfig.Parse("{ \"ws_port\": 8000, \"autostart\": { \"orchestrator\": false, \"ui\": false } }");
        Assert.Equal(8000, c.WsPort);
        Assert.False(c.AutostartOrchestrator);
        Assert.False(c.AutostartUi);
    }

    [Fact]
    public void Parse_malformato_da_i_default_con_un_avviso_mai_eccezione()
    {
        // spec §9: "startup.json malformato → log + default, mai panic"
        StartupConfig c = StartupConfig.Parse("{ ws_port: ");
        Assert.Equal(7331, c.WsPort);
        Assert.Single(c.Warnings);
        Assert.Contains("startup.json", c.Warnings[0]);
    }

    [Fact]
    public void Load_legge_lo_startup_json_committato_in_Test_Run()
    {
        // Lo stesso file che legge l'orchestratore (crate startup-config): se lo schema cambia
        // di là, questo test se ne accorge di qua.
        string configDir = Path.Combine(TestPaths.RepoRoot(), "Test Run", "Configuration");
        StartupConfig c = StartupConfig.Load(configDir);
        Assert.Equal(7331, c.WsPort);
        Assert.True(c.AutostartOrchestrator);
        Assert.True(c.AutostartUi);
        Assert.Empty(c.Warnings);
    }

    [Fact]
    public void Load_senza_file_da_i_default()
    {
        string dir = Path.Combine(Path.GetTempPath(), "lare-cfg-" + Guid.NewGuid().ToString("N"));
        StartupConfig c = StartupConfig.Load(dir);
        Assert.Equal(7331, c.WsPort);
        Assert.Empty(c.Warnings);
    }
}

public class TokenFileTests
{
    [Fact]
    public void Read_ritorna_il_token_trimmato_o_null_se_manca()
    {
        string dir = Path.Combine(Path.GetTempPath(), "lare-tok-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(dir);
        try
        {
            Assert.Null(TokenFile.Read(dir));
            File.WriteAllText(Path.Combine(dir, "token"), "abc123\r\n");
            Assert.Equal("abc123", TokenFile.Read(dir));
            File.WriteAllText(Path.Combine(dir, "token"), "   ");
            Assert.Null(TokenFile.Read(dir));
        }
        finally
        {
            Directory.Delete(dir, recursive: true);
        }
    }
}

public class HostLogTests
{
    [Fact]
    public void Open_crea_logs_e_scrive_righe_con_livello()
    {
        string dir = Path.Combine(Path.GetTempPath(), "lare-log-" + Guid.NewGuid().ToString("N"));
        try
        {
            HostLog log = HostLog.Open(dir);
            log.Info("ciao");
            log.Warn("attenzione");
            string text = File.ReadAllText(Path.Combine(dir, "logs", "lare-shell.log"));
            Assert.Contains("INFO ciao", text);
            Assert.Contains("WARN attenzione", text);
        }
        finally
        {
            if (Directory.Exists(dir)) Directory.Delete(dir, recursive: true);
        }
    }

    [Fact]
    public void Open_su_percorso_non_scrivibile_non_lancia_e_le_scritture_sono_no_op()
    {
        // Un percorso impossibile (carattere non valido): il log deve degradare in silenzio,
        // mai far cadere la shell per un problema di log (spec §9: la shell resta usabile).
        HostLog log = HostLog.Open("Z:\\<>|\0impossibile");
        Assert.Same(HostLog.Null, log);
        log.Info("niente");
    }
}
