using LareShell.Config;
using LareShell.Host;
using LareShell.Shell;
using LareShell.Tests.Host;
using Xunit;

namespace LareShell.Tests.Shell;

[Collection("runspace")]
public class ExecutorTests : IDisposable
{
    private readonly LareHost _host = new(new FakeConsoleModes());
    private readonly RunspaceSession _session;
    private readonly Executor _executor;

    public ExecutorTests()
    {
        _session = RunspaceSession.Open(_host, HostLog.Null);
        _executor = new Executor(_session);
    }

    public void Dispose() => _session.Dispose();

    [Fact]
    public void Cmdlet_con_capture_true_torna_output_formattato_exit_0_e_cwd()
    {
        ExecOutcome o = _executor.Run("Get-Date -Format yyyy", capture: true);
        Assert.Equal(0, o.ExitCode);
        Assert.Contains(DateTime.Now.Year.ToString(), o.Output);
        Assert.Equal(_session.CurrentDirectory, o.Cwd);
        Assert.False(o.Stopped);
    }

    [Fact]
    public void Programma_nativo_con_capture_true_viene_catturato_con_il_suo_exit_code()
    {
        // spec §4.5: con un cmdlet in mezzo (ForEach-Object) il nativo NON eredita la console:
        // il suo stdout passa dalla pipe → Out-Default → LareHostUI.Write → Recorder.
        ExecOutcome o = _executor.Run("cmd /c \"echo ciao-nativo & exit 3\"", capture: true);
        Assert.Contains("ciao-nativo", o.Output);
        Assert.Equal(3, o.ExitCode);
    }

    [Fact]
    public void Errore_non_terminante_da_exit_1_e_il_testo_dell_errore_in_output()
    {
        // spec §9: "Pipeline in errore in ExecInShell → testo dell'errore nell'output, exit_code 1"
        ExecOutcome o = _executor.Run("Get-Item 'C:\\__lare_non_esiste__'", capture: true);
        Assert.Equal(1, o.ExitCode);
        Assert.Contains("__lare_non_esiste__", o.Output);
    }

    [Fact]
    public void Errore_di_sintassi_da_exit_1_con_il_messaggio_in_output()
    {
        ExecOutcome o = _executor.Run("if (", capture: true);
        Assert.Equal(1, o.ExitCode);
        Assert.False(string.IsNullOrWhiteSpace(o.Output));
    }

    [Fact]
    public void Dopo_un_comando_fallito_uno_riuscito_torna_a_exit_0()
    {
        _executor.Run("cmd /c exit 5", capture: true);
        ExecOutcome o = _executor.Run("Get-Date", capture: true);
        Assert.Equal(0, o.ExitCode);
    }

    [Fact]
    public void Set_Location_persiste_e_la_cwd_torna_aggiornata()
    {
        // spec §4.6 (D17): "cd persiste" fra un ExecInShell e il successivo — stesso runspace.
        string temp = Path.GetTempPath().TrimEnd('\\');
        ExecOutcome o = _executor.Run("Set-Location '" + temp + "'", capture: true);
        Assert.Equal(temp, o.Cwd.TrimEnd('\\'), ignoreCase: true);
        Assert.Equal(temp, _executor.Run("Get-Location", capture: true).Cwd.TrimEnd('\\'), ignoreCase: true);
    }

    [Fact]
    public void Capture_false_da_output_vuoto_ma_exit_code_e_cwd()
    {
        ExecOutcome o = _executor.Run("cmd /c exit 4", capture: false);
        Assert.Equal(string.Empty, o.Output);
        Assert.Equal(4, o.ExitCode);
        Assert.Equal(_session.CurrentDirectory, o.Cwd);
    }

    [Fact]
    public void StopCurrent_ferma_la_pipeline_e_segna_Stopped()
    {
        // Ctrl+C durante un ExecInShell (spec §4.4): la pipeline viene fermata, niente ExecResult parziale.
        var stopper = new Thread(() =>
        {
            Thread.Sleep(400);
            _executor.StopCurrent();
        });
        stopper.Start();
        ExecOutcome o = _executor.Run("Start-Sleep -Seconds 20", capture: true);
        stopper.Join();
        Assert.True(o.Stopped);
    }

    [Fact]
    public void RunInteractive_esegue_senza_accendere_il_registratore()
    {
        // Comando digitato dall'utente: niente cattura (l'output va solo a schermo) e niente
        // lettura di $? (ruling 3: il prompt dell'utente deve vedere il $? vero).
        bool stopped = _executor.RunInteractive("$global:lare_interactive = 41 + 1");
        Assert.False(stopped);
        Assert.False(_host.HostUI.Recorder.IsRecording);
        Assert.Equal(42, _session.Runspace.SessionStateProxy.GetVariable("lare_interactive"));
    }

    [Fact]
    public void RunInteractive_mostra_gli_errori_e_non_lancia()
    {
        Assert.False(_executor.RunInteractive("if ("));                       // sintassi
        Assert.False(_executor.RunInteractive("Get-Item 'C:\\__lare_no__'")); // non terminante
    }
}
