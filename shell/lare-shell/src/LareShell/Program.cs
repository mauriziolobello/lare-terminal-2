using LareShell.Config;
using LareShell.Host;
using LareShell.Protocol;
using LareShell.Shell;

namespace LareShell;

/// <summary>
/// Entry point: composizione degli oggetti (config → log → runspace → client → launcher → REPL).
/// Nessuna logica qui: solo "chi dipende da chi", così ogni pezzo resta testabile da solo.
/// </summary>
internal static class Program
{
    private static int Main(string[] args)
    {
        try { Console.OutputEncoding = System.Text.Encoding.UTF8; } catch { /* stdout rediretto */ }

        CliArgs cli = CliArgs.Parse(args);
        string configDir = ConfigDir.Resolve(cli.ConfigDir, AppContext.BaseDirectory);
        try { Directory.CreateDirectory(configDir); } catch { /* §9: "creata al primo avvio" — se non si può, i passi dopo lo diranno */ }

        HostLog log = HostLog.Open(configDir);
        StartupConfig cfg = StartupConfig.Load(configDir);
        foreach (string w in cfg.Warnings)
        {
            Console.WriteLine("[LARE] " + w);
            log.Warn(w);
        }

        if (cli.SelfTest)
        {
            return SelfTest.Run(configDir, cfg);
        }

        // Id di sessione: da ui.exe (--session, modalità A) o generato (modalità B). Sempre
        // presente: senza, i log dell'orchestratore non correlano la connessione.
        string sessionId = string.IsNullOrWhiteSpace(cli.SessionId) ? Guid.NewGuid().ToString("N")[..8] : cli.SessionId;
        log.Info("avvio lare-shell " + HostInfo.Version + " sessione " + sessionId + " config " + configDir);

        try
        {
            var host = new LareHost();
            using RunspaceSession session = RunspaceSession.Open(host, log);
            var executor = new Executor(session);
            using var client = new OrchestratorClient(
                new Uri("ws://127.0.0.1:" + cfg.WsPort + "/"),
                () => TokenFile.Read(configDir),
                sessionId,
                HostInfo.Version,
                log);
            var launcher = new Launcher(ConfigDir.DeployRoot(configDir), configDir, cfg, new ProcessStarter(), log, Console.Out);

            return new Repl(session, executor, client, launcher, log).Run();
        }
        catch (Exception ex)
        {
            // Ultima rete: un errore non previsto all'avvio (es. runspace che non si apre) deve
            // lasciare una riga leggibile, non uno stack trace in una scheda che si chiude.
            Console.Error.WriteLine("[LARE] errore fatale: " + ex.GetType().Name + ": " + ex.Message);
            log.Warn("errore fatale: " + ex);
            return 1;
        }
    }
}
