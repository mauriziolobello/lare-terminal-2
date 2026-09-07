using LareShell.Config;
using LareShell.Host;
using LareShell.Protocol;
using LareShell.Shell;

namespace LareShell;

/// <summary>
/// Il ciclo read-eval-print-loop (come Repl.cs dello spike, senza barre VT — D13): prompt
/// dell'utente → riga (PSReadLine) → se inizia con '/' va all'orchestratore (SlashTurn), altrimenti
/// al runspace (Executor.RunInteractive). Tutto su QUESTO thread (§4.4). Ctrl+C: ferma la
/// pipeline in corso e cancella il turno slash in corso (token), come pwsh — non esce dalla shell.
/// Riconnessione on demand (ruling 2): prima di ogni /… si verifica la connessione e, se manca,
/// la si ristabilisce con l'autostart (Launcher.EnsureConnected, 5 s).
/// </summary>
internal sealed class Repl
{
    private readonly RunspaceSession _session;
    private readonly Executor _executor;
    private readonly OrchestratorClient _client;
    private readonly Launcher _launcher;
    private readonly HostLog _log;

    // Token del turno slash in corso: l'handler di Ctrl+C (altro thread) lo cancella.
    private volatile CancellationTokenSource? _turnCts;

    public Repl(RunspaceSession session, Executor executor, OrchestratorClient client, Launcher launcher, HostLog log)
    {
        _session = session;
        _executor = executor;
        _client = client;
        _launcher = launcher;
        _log = log;
    }

    public int Run()
    {
        // VT processing incondizionata: l'OSC 9001 va interpretata, non stampata come testo.
        ConsoleModes.TryEnableVirtualTerminalProcessing();

        // Lezione dello spike: con stdin rediretto PSConsoleHostReadLine non dà mai EOF → Console.ReadLine.
        bool usePsReadLine = !Console.IsInputRedirected && _session.PsReadLineAvailable;

        ConsoleCancelEventHandler onCancel = (_, e) =>
        {
            e.Cancel = true;                 // non terminare il processo
            _executor.StopCurrent();         // ferma la pipeline (utente o ExecInShell)
            _turnCts?.Cancel();              // sveglia SlashTurn in attesa → CancelCommand
        };
        Console.CancelKeyPress += onCancel;

        try
        {
            Banner(usePsReadLine);
            LoadProfiles();
            ConnectAtStartup();

            while (!_session.Host.ShouldExit)
            {
                Console.Write(_session.EvaluatePrompt());
                string? line = _session.ReadLine(usePsReadLine);
                if (line is null)
                {
                    Console.WriteLine();
                    Console.WriteLine("[LARE] EOF su stdin, uscita.");
                    break;
                }

                if (SlashLine.IsSlash(line))
                {
                    RunSlash(SlashLine.Normalize(line));
                    continue;
                }

                if (string.IsNullOrWhiteSpace(line))
                {
                    continue;
                }

                _executor.RunInteractive(line);
            }
        }
        finally
        {
            Console.CancelKeyPress -= onCancel;
        }

        return _session.Host.ExitCode;
    }

    private void Banner(bool usePsReadLine)
    {
        Console.WriteLine("Lare Terminal " + HostInfo.Version + " — sessione " + _client.SessionId
                          + (usePsReadLine ? string.Empty : "  (PSReadLine non disponibile: editing di riga base)"));
    }

    private void LoadProfiles()
    {
        string docs = Environment.GetFolderPath(Environment.SpecialFolder.MyDocuments);
        ProfileLoader.ProfilePaths paths = ProfileLoader.Compute(docs, _session.PwshDir, AppContext.BaseDirectory);
        ProfileLoader.SetDollarProfile(_session.Runspace, paths);
        IReadOnlyList<string> loaded = ProfileLoader.Load(_session.Runspace, _session.Host, paths, File.Exists);
        _log.Info("profili caricati: " + (loaded.Count == 0 ? "nessuno" : string.Join(", ", loaded)));
    }

    private void ConnectAtStartup()
    {
        if (EnsureConnected())
        {
            Console.WriteLine("orchestratore: connesso");
            _launcher.EnsureUi();
        }
        else
        {
            Console.WriteLine("orchestratore: NON connesso — la shell funziona, i comandi /… no (ritento al prossimo /…)");
        }
    }

    /// <summary>true se connessi (già o dopo autostart+retry).</summary>
    private bool EnsureConnected()
    {
        if (_client.IsConnected)
        {
            return true;
        }

        return _launcher.EnsureConnected(
            () => _client.Connect(_session.CurrentDirectory, TimeSpan.FromSeconds(3)),
            Launcher.ConnectWindow,
            Launcher.RetryInterval);
    }

    private void RunSlash(string input)
    {
        // Segnale all'emulatore (§4.7), prima di qualunque altra cosa: costa nulla e non dipende dal WS.
        Console.Out.Write(Osc.Intercept(input));
        Console.Out.Flush();

        if (!EnsureConnected())
        {
            Console.WriteLine("orchestratore non raggiungibile: comando ignorato");
            return;
        }

        _launcher.EnsureUi();   // self-heal (ruling 8): la finestra di output vive in ui.exe

        using var cts = new CancellationTokenSource();
        _turnCts = cts;
        try
        {
            var turn = new SlashTurn(_client, new ConsoleGate(), _executor, Console.Out, _log);
            TurnResult result = turn.Run(input, _session.CurrentDirectory, cts.Token);
            if (result == TurnResult.Disconnected)
            {
                _log.Warn("turno interrotto per disconnessione; riconnessione al prossimo /…");
            }
        }
        finally
        {
            _turnCts = null;
        }
    }
}
