using System.Management.Automation;
using LareShell.Host;
using Xunit;

namespace LareShell.Tests.Host;

/// <summary>Console mode finte: due "handle" (out/in) con una mode ciascuno.</summary>
internal sealed class FakeConsoleModes : IConsoleModes
{
    public uint OutMode;
    public uint InMode;
    public bool Available = true;
    public int SetCalls;

    // Fa fallire SOLO le letture (TryGetMode), non le scritture (TrySetMode): simula il caso
    // "sapevamo la mode iniziale al costruttore, ma quando è partita l'app nativa GetConsoleMode
    // è fallita" — TrySetMode continua a funzionare a prescindere da questo flag, esattamente
    // come farebbe SetConsoleMode su un handle valido anche se una GetConsoleMode precedente
    // fosse fallita per un motivo transitorio.
    public bool GetFailsNow;

    private static readonly IntPtr OutHandle = new(11);
    private static readonly IntPtr InHandle = new(10);

    public IntPtr GetHandle(int std) => std == ConsoleModes.StdOutputHandle ? OutHandle : InHandle;

    public bool TryGetMode(IntPtr handle, out uint mode)
    {
        // La mode "vera" viene comunque riportata in out anche quando la lettura "fallisce"
        // (GetFailsNow): rispecchia Win32 GetConsoleMode, che quando fallisce lascia lpMode
        // indefinito, ma qui usiamo il valore corrente per rendere il test deterministico e
        // verificare che LareHost ignori il valore quando il bool di ritorno è false.
        mode = handle == OutHandle ? OutMode : InMode;
        return Available && !GetFailsNow;
    }

    public bool TrySetMode(IntPtr handle, uint mode)
    {
        if (!Available) return false;
        SetCalls++;
        if (handle == OutHandle) OutMode = mode; else InMode = mode;
        return true;
    }
}

public class LareHostTests
{
    [Fact]
    public void Identita_della_host()
    {
        var host = new LareHost(new FakeConsoleModes());
        Assert.Equal("LareShell", host.Name);
        Assert.Equal(new Version(2, 0, 0), host.Version);
        Assert.Same(host.UI, host.HostUI);
    }

    [Fact]
    public void SetShouldExit_segnala_senza_uscire_dal_processo()
    {
        var host = new LareHost(new FakeConsoleModes());
        Assert.False(host.ShouldExit);
        host.SetShouldExit(7);
        Assert.True(host.ShouldExit);
        Assert.Equal(7, host.ExitCode);
    }

    [Fact]
    public void NotifyBegin_ripristina_le_mode_iniziali_e_NotifyEnd_quelle_salvate_con_VT_acceso()
    {
        // spec §4.4: "NotifyBeginApplication/NotifyEndApplication salvano/ripristinano le
        // modalità console (output e input) come ConsoleHost.cs:1227-1270"
        var modes = new FakeConsoleModes { OutMode = 0x0003, InMode = 0x01F7 };   // stato all'avvio
        var host = new LareHost(modes);                                          // le cattura qui

        modes.OutMode = 0x0007;   // PSReadLine/VT hanno cambiato le mode nel frattempo
        modes.InMode = 0x01E0;

        host.NotifyBeginApplication();
        Assert.Equal(0x0003u, modes.OutMode);   // l'app nativa vede la console "come all'avvio"
        Assert.Equal(0x01F7u, modes.InMode);

        host.NotifyEndApplication();
        Assert.Equal(0x01E0u, modes.InMode);                                                             // input: come prima dell'app
        Assert.Equal(0x0007u | ConsoleModes.EnableVirtualTerminalProcessing | ConsoleModes.EnableProcessedOutput, modes.OutMode);
    }

    [Fact]
    public void Notify_annidati_ripristinano_solo_quando_l_ultima_app_finisce()
    {
        var modes = new FakeConsoleModes { OutMode = 0x0003, InMode = 0x01F7 };
        var host = new LareHost(modes);
        modes.OutMode = 0x0007;
        modes.InMode = 0x01E0;

        host.NotifyBeginApplication();   // contatore 1 → mode iniziali
        host.NotifyBeginApplication();   // contatore 2 → invariate
        int callsAfterBegin = modes.SetCalls;
        host.NotifyEndApplication();     // contatore 1 → NON ripristina
        Assert.Equal(callsAfterBegin, modes.SetCalls);
        Assert.Equal(0x01F7u, modes.InMode);
        host.NotifyEndApplication();     // contatore 0 → ripristina
        Assert.Equal(0x01E0u, modes.InMode);
    }

    [Fact]
    public void Senza_console_reale_le_notifiche_sono_no_op()
    {
        var modes = new FakeConsoleModes { Available = false };
        var host = new LareHost(modes);
        host.NotifyBeginApplication();
        host.NotifyEndApplication();
        Assert.Equal(0, modes.SetCalls);
    }

    [Fact]
    public void NotifyEnd_con_mode_non_leggibili_all_inizio_dell_app_ripristina_le_mode_iniziali_con_VT()
    {
        // Copre il ramo "_savedModesValid == false" di NotifyEndApplication: se in
        // NotifyBeginApplication la lettura delle mode "correnti" fallisce (qui simulato con
        // GetFailsNow), non abbiamo nulla di affidabile da ripristinare in NotifyEndApplication,
        // quindi cadiamo sul fallback "meglio le mode iniziali che quelle lasciate dall'app".
        var modes = new FakeConsoleModes { OutMode = 0x0003, InMode = 0x01F7 };   // stato all'avvio, leggibile
        var host = new LareHost(modes);                                          // le cattura qui (lettura OK)

        modes.OutMode = 0x0007;   // PSReadLine/VT hanno cambiato le mode nel frattempo
        modes.InMode = 0x01E0;
        modes.GetFailsNow = true;   // ora GetConsoleMode "fallisce" (handle diventato non valido, ecc.)

        host.NotifyBeginApplication();
        // _savedModesValid è false (TryGetMode ha restituito false per entrambi gli handle):
        // NotifyBeginApplication ha comunque impostato le mode iniziali (TrySetMode non dipende
        // da GetFailsNow), quindi l'app nativa le vede già correttamente.
        Assert.Equal(0x0003u, modes.OutMode);
        Assert.Equal(0x01F7u, modes.InMode);

        modes.GetFailsNow = false;   // le letture tornano disponibili (TrySetMode funzionava comunque)
        host.NotifyEndApplication();

        // Ramo fallback: NotifyEndApplication non aveva mode "salvate" valide, quindi ripristina
        // le mode iniziali (non quelle lasciate dall'app nativa) e sull'output riaccende VT.
        Assert.Equal(0x01F7u, modes.InMode);
        Assert.Equal(0x0003u | ConsoleModes.EnableVirtualTerminalProcessing | ConsoleModes.EnableProcessedOutput, modes.OutMode);
    }

    [Fact]
    public void I_Write_della_UI_finiscono_nel_registratore_quando_attivo()
    {
        var host = new LareHost(new FakeConsoleModes());
        LareHostUI ui = host.HostUI;
        ui.Write("fuori");                  // prima di Begin: non registrato
        ui.Recorder.Begin();
        ui.Write("a");
        ui.WriteLine("b");
        ui.WriteErrorLine("err");
        ui.WriteWarningLine("w");
        ui.Write(ConsoleColor.Red, ConsoleColor.Black, "c");
        string got = ui.Recorder.End();
        Assert.DoesNotContain("fuori", got);
        Assert.Contains("a", got);
        Assert.Contains("b" + Environment.NewLine, got);
        Assert.Contains("err", got);
        Assert.Contains("WARNING: w", got);
        Assert.Contains("c", got);
    }

    [Fact]
    public void WriteProgress_non_viene_registrato()
    {
        // WriteProgress passa da WriteColored (non da Write): le barre di Write-Progress
        // sarebbero solo rumore nell'output catturato per l'AI (spec §4.5), quindi il
        // registratore deve restare vuoto anche se attivo.
        var host = new LareHost(new FakeConsoleModes());
        LareHostUI ui = host.HostUI;
        ui.Recorder.Begin();
        ui.WriteProgress(1, new ProgressRecord(1, "attività", "stato") { PercentComplete = 50 });
        Assert.Equal(string.Empty, ui.Recorder.End());
    }
}
