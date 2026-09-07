using System.Diagnostics;
using LareShell.Config;

namespace LareShell.Shell;

/// <summary>Confine d'astrazione sui processi: il Launcher decide COSA avviare, questo COME.</summary>
internal interface IProcessStarter
{
    bool Exists(string exePath);

    /// <summary>true se un processo con quel percorso esatto di eseguibile è già vivo.</summary>
    bool IsRunning(string exePath);

    void Start(string exePath, IReadOnlyList<string> args, bool hideWindow);
}

/// <summary>
/// Avvio "staccato" (spec §6.4): <c>UseShellExecute = true</c> → il figlio NON eredita gli handle
/// della nostra console (stdio compresi) e vive in una console propria: un Ctrl+C nella shell non
/// lo abbatte, e i suoi log non sporcano il terminale. <c>WindowStyle = Hidden</c> nasconde quella
/// console per ENTRAMBI i figli (scoperto all'e2e del piano 2b, non la formulazione originale del
/// piano — vedi <c>Launcher.EnsureUi</c>): <c>orchestrator.exe</c> è un'app console e la finestra
/// nascosta è la sua UNICA finestra; <c>ui.exe</c> in build debug è ANCH'esso un'app console (Tauri
/// tiene la console per i log), quindi ha anche lui una console da nascondere — ma le sue finestre
/// vere (Tauri/xterm) sono create e mostrate esplicitamente dall'app stessa e restano visibili a
/// prescindere da questo flag. È l'equivalente pratico in .NET di DETACHED_PROCESS.
/// </summary>
internal sealed class ProcessStarter : IProcessStarter
{
    public bool Exists(string exePath) => File.Exists(exePath);

    public bool IsRunning(string exePath)
    {
        string name = Path.GetFileNameWithoutExtension(exePath);
        foreach (Process p in Process.GetProcessesByName(name))
        {
            try
            {
                // MainModule lancia per i processi di altri utenti/elevati: quelli non sono "il nostro" ui.exe.
                if (string.Equals(p.MainModule?.FileName, exePath, StringComparison.OrdinalIgnoreCase))
                {
                    return true;
                }
            }
            catch (Exception ex) when (ex is System.ComponentModel.Win32Exception or InvalidOperationException)
            {
                // ignorato: vedi sopra
            }
            finally
            {
                p.Dispose();
            }
        }

        return false;
    }

    public void Start(string exePath, IReadOnlyList<string> args, bool hideWindow)
    {
        var psi = new ProcessStartInfo(exePath)
        {
            UseShellExecute = true,
            WorkingDirectory = Path.GetDirectoryName(exePath) ?? string.Empty,
            WindowStyle = hideWindow ? ProcessWindowStyle.Hidden : ProcessWindowStyle.Normal,
        };
        foreach (string a in args)
        {
            psi.ArgumentList.Add(a);
        }

        Process.Start(psi)?.Dispose();
    }
}

/// <summary>
/// Self-heal all'avvio della host in modalità B (spec §6.4): se il WS non risponde e
/// <c>autostart.orchestrator</c> è attivo, avvia <c>&lt;radice deploy&gt;\orchestrator.exe --config-dir …</c>
/// e ritenta per 5 s; poi, se <c>autostart.ui</c>, avvia <c>ui.exe</c> quando non c'è già un
/// processo con quel percorso. Gara all'avvio (§2.3): due host che partono insieme avviano due
/// orchestratori; il bind della porta decide (il secondo esce), entrambe le host si connettono
/// entro la finestra di retry. Stampa sul <c>console</c> passato (mai su Console direttamente: testabile).
/// </summary>
internal sealed class Launcher
{
    public static readonly TimeSpan ConnectWindow = TimeSpan.FromSeconds(5);
    public static readonly TimeSpan RetryInterval = TimeSpan.FromMilliseconds(250);

    private readonly string _configDir;
    private readonly StartupConfig _cfg;
    private readonly IProcessStarter _starter;
    private readonly HostLog _log;
    private readonly TextWriter _console;

    public Launcher(string deployRoot, string configDir, StartupConfig cfg, IProcessStarter starter, HostLog log, TextWriter console)
    {
        OrchestratorExe = Path.Combine(deployRoot, "orchestrator.exe");
        UiExe = Path.Combine(deployRoot, "ui.exe");
        _configDir = configDir;
        _cfg = cfg;
        _starter = starter;
        _log = log;
        _console = console;
    }

    public string OrchestratorExe { get; }

    public string UiExe { get; }

    /// <param name="connect">Un tentativo di connessione: null = riuscito, altrimenti il motivo.</param>
    public bool EnsureConnected(Func<string?> connect, TimeSpan window, TimeSpan retry)
    {
        string? reason = connect();
        if (reason is null)
        {
            return true;
        }

        if (!_cfg.AutostartOrchestrator)
        {
            _console.WriteLine("orchestratore non raggiungibile (" + reason + "); autostart disattivo in startup.json");
            return false;
        }

        if (!_starter.Exists(OrchestratorExe))
        {
            _console.WriteLine("orchestratore non raggiungibile (" + reason + ") e " + OrchestratorExe + " non esiste");
            return false;
        }

        _console.WriteLine("orchestratore non raggiungibile (" + reason + "): avvio " + OrchestratorExe);
        _log.Info("autostart orchestratore: " + OrchestratorExe);
        _starter.Start(OrchestratorExe, new[] { "--config-dir", _configDir }, hideWindow: true);

        var deadline = DateTime.UtcNow + window;
        while (DateTime.UtcNow < deadline)
        {
            Thread.Sleep(retry);
            reason = connect();
            if (reason is null)
            {
                _console.WriteLine("orchestratore avviato e connesso");
                return true;
            }
        }

        _console.WriteLine("orchestratore non raggiungibile dopo " + window.TotalSeconds + " s (" + reason + "): i comandi /… non funzioneranno finché non risponde");
        _log.Warn("autostart orchestratore fallito: " + reason);
        return false;
    }

    /// <summary>Avvia ui.exe se manca e l'autostart è attivo. true = avviata adesso.</summary>
    public bool EnsureUi()
    {
        if (!_cfg.AutostartUi || !_starter.Exists(UiExe) || _starter.IsRunning(UiExe))
        {
            return false;
        }

        _log.Info("autostart ui: " + UiExe);
        // hideWindow anche per ui.exe (scoperto all'e2e del piano 2b): in build debug ui.exe è
        // un'app CONSOLE (Tauri tiene la console per i log) e, con Windows Terminal impostato come
        // terminale predefinito, una console nuova si apre come SCHEDA di WT e ruba il fuoco alla
        // shell. WindowStyle.Hidden nasconde solo quella console: le finestre di ui.exe sono
        // create e mostrate esplicitamente dall'app, e restano visibili (verificato dal vivo).
        _starter.Start(UiExe, new[] { "--config-dir", _configDir }, hideWindow: true);
        return true;
    }
}
