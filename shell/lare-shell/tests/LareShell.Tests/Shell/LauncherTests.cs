using LareShell.Config;
using LareShell.Shell;
using Xunit;

namespace LareShell.Tests.Shell;

internal sealed class FakeStarter : IProcessStarter
{
    public HashSet<string> Existing { get; } = new(StringComparer.OrdinalIgnoreCase);
    public HashSet<string> Running { get; } = new(StringComparer.OrdinalIgnoreCase);
    public List<(string Exe, IReadOnlyList<string> Args, bool Hidden)> Started { get; } = new();

    public bool Exists(string exePath) => Existing.Contains(exePath);

    public bool IsRunning(string exePath) => Running.Contains(exePath);

    public void Start(string exePath, IReadOnlyList<string> args, bool hideWindow) => Started.Add((exePath, args, hideWindow));
}

public class LauncherTests
{
    private const string Root = @"C:\Lare";
    private const string Cfg = @"C:\Lare\Configuration";
    private static readonly TimeSpan Window = TimeSpan.FromMilliseconds(300);
    private static readonly TimeSpan Retry = TimeSpan.FromMilliseconds(20);

    private static (Launcher L, FakeStarter S, StringWriter Out) Make(bool autoOrch = true, bool autoUi = true)
    {
        var starter = new FakeStarter();
        starter.Existing.Add(@"C:\Lare\orchestrator.exe");
        starter.Existing.Add(@"C:\Lare\ui.exe");
        var output = new StringWriter();
        var cfg = new StartupConfig { AutostartOrchestrator = autoOrch, AutostartUi = autoUi };
        return (new Launcher(Root, Cfg, cfg, starter, HostLog.Null, output), starter, output);
    }

    /// <summary>Sequenza di esiti di connessione: null = connesso.</summary>
    private static Func<string?> Sequence(params string?[] outcomes)
    {
        int i = 0;
        return () => i < outcomes.Length ? outcomes[i++] : outcomes[^1];
    }

    [Fact]
    public void Percorsi_degli_eseguibili_nella_radice_del_deploy()
    {
        (Launcher l, _, _) = Make();
        Assert.Equal(@"C:\Lare\orchestrator.exe", l.OrchestratorExe);
        Assert.Equal(@"C:\Lare\ui.exe", l.UiExe);
    }

    [Fact]
    public void Se_la_connessione_riesce_subito_non_avvia_nulla()
    {
        (Launcher l, FakeStarter s, _) = Make();
        Assert.True(l.EnsureConnected(Sequence((string?)null), Window, Retry));
        Assert.Empty(s.Started);
    }

    [Fact]
    public void Se_fallisce_avvia_l_orchestratore_con_config_dir_e_ritenta_finche_riesce()
    {
        // spec §6.4: "non raggiungono il WS → se autostart.orchestrator, avviano orchestrator.exe e ritentano per 5 s"
        (Launcher l, FakeStarter s, StringWriter o) = Make();
        Assert.True(l.EnsureConnected(Sequence("rifiutata", "rifiutata", null), Window, Retry));
        (string exe, IReadOnlyList<string> args, bool hidden) = Assert.Single(s.Started);
        Assert.Equal(@"C:\Lare\orchestrator.exe", exe);
        Assert.Equal(new[] { "--config-dir", Cfg }, args);   // D6: i figli ricevono --config-dir per argomento
        Assert.True(hidden);                                  // app console: nessuna finestra
        Assert.Contains("avvio", o.ToString(), StringComparison.OrdinalIgnoreCase);
    }

    [Fact]
    public void Con_autostart_disattivo_non_avvia_e_torna_false()
    {
        (Launcher l, FakeStarter s, StringWriter o) = Make(autoOrch: false);
        Assert.False(l.EnsureConnected(Sequence("rifiutata"), Window, Retry));
        Assert.Empty(s.Started);
        Assert.Contains("autostart", o.ToString(), StringComparison.OrdinalIgnoreCase);
    }

    [Fact]
    public void Se_l_eseguibile_manca_non_avvia_e_lo_dice()
    {
        (Launcher l, FakeStarter s, StringWriter o) = Make();
        s.Existing.Remove(@"C:\Lare\orchestrator.exe");
        Assert.False(l.EnsureConnected(Sequence("rifiutata"), Window, Retry));
        Assert.Empty(s.Started);
        Assert.Contains(@"C:\Lare\orchestrator.exe", o.ToString());
    }

    [Fact]
    public void Se_non_si_connette_entro_la_finestra_torna_false_dopo_un_solo_avvio()
    {
        (Launcher l, FakeStarter s, StringWriter o) = Make();
        var sw = System.Diagnostics.Stopwatch.StartNew();
        Assert.False(l.EnsureConnected(Sequence("rifiutata"), Window, Retry));
        Assert.Single(s.Started);
        Assert.True(sw.Elapsed >= Window, "ha smesso di ritentare troppo presto: " + sw.Elapsed);
        Assert.Contains("non raggiungibile", o.ToString());
    }

    [Fact]
    public void EnsureUi_avvia_ui_exe_solo_se_non_gira_gia()
    {
        (Launcher l, FakeStarter s, _) = Make();
        Assert.True(l.EnsureUi());
        (string exe, IReadOnlyList<string> args, bool hidden) = Assert.Single(s.Started);
        Assert.Equal(@"C:\Lare\ui.exe", exe);
        Assert.Equal(new[] { "--config-dir", Cfg }, args);
        Assert.False(hidden);   // app GUI: finestra normale

        s.Running.Add(@"C:\Lare\ui.exe");
        Assert.False(l.EnsureUi());
        Assert.Single(s.Started);
    }

    [Fact]
    public void EnsureUi_rispetta_autostart_ui_e_l_assenza_dell_exe()
    {
        (Launcher l1, FakeStarter s1, _) = Make(autoUi: false);
        Assert.False(l1.EnsureUi());
        Assert.Empty(s1.Started);

        (Launcher l2, FakeStarter s2, _) = Make();
        s2.Existing.Remove(@"C:\Lare\ui.exe");
        Assert.False(l2.EnsureUi());
        Assert.Empty(s2.Started);
    }
}
