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
            // e.Cancel = true PRIMA di tutto, fuori dal try: anche se StopCurrent/Cancel
            // lanciano, il processo non deve terminare (comportamento di default di Ctrl+C
            // senza questo flag). Un'eccezione NON gestita qui sfuggirebbe dall'handler di
            // Console.CancelKeyPress: il runtime la considera non gestita e TERMINA il
            // processo — l'esatto opposto di "Ctrl+C non esce mai dalla shell" (§4.4). Per
            // questo il corpo è avvolto in un try/catch che logga e assorbe tutto.
            e.Cancel = true;
            try
            {
                // Prima riga: prova nel log che l'handler è davvero scattato, a prescindere
                // da cosa succede dopo — utile per diagnosticare un Ctrl+C che sembra "no-op"
                // (revisione finale piano 2b).
                _log.Info("Ctrl+C ricevuto (turno in corso: " + (_turnCts is not null) + ")");
                _executor.StopCurrent();     // ferma la pipeline (utente o ExecInShell)
                _turnCts?.Cancel();           // sveglia SlashTurn in attesa → CancelCommand
            }
            catch (Exception ex)
            {
                _log.Warn("handler Ctrl+C: " + ex.Message);
            }
        };
        Console.CancelKeyPress += onCancel;

        try
        {
            Banner(usePsReadLine, _session.PsReadLineAvailable);
            LoadProfiles();
            ConnectAtStartup();

            while (!_session.Host.ShouldExit)
            {
                Console.Write(_session.EvaluatePrompt());
                string? line = _session.ReadLine(usePsReadLine);

                // PSReadLine mette la console in TreatControlCAsInput durante l'editing della
                // riga (Ctrl+C diventa un carattere, non un segnale) e dovrebbe ripristinarlo
                // da sola all'uscita — ma non ci affidiamo a quel "dovrebbe": se ReadLine esce
                // per qualunque via (anche un percorso interno non previsto) senza ripristinare
                // il flag, un Ctrl+C durante l'esecuzione del comando successivo non
                // raggiungerebbe più CancelKeyPress e la §4.4 (Ctrl+C ferma la pipeline) si
                // romperebbe silenziosamente. Lo forziamo qui, ad ogni giro del loop.
                try
                {
                    Console.TreatControlCAsInput = false;
                }
                catch
                {
                    // Nessuna console reale (stdin/stdout rediretti in test): nessun flag da
                    // ripristinare, nulla da fare.
                }

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

    /// <summary>
    /// Tre casi distinti, non due: PSReadLine può essere disponibile ma non usato (stdin
    /// rediretto — non è un problema, solo un fatto), oppure proprio non disponibile (editing
    /// di riga più povero: un fatto diverso, che vale la pena segnalare in modo diverso).
    /// </summary>
    private void Banner(bool usePsReadLine, bool psReadLineAvailable)
    {
        string suffix = usePsReadLine
            ? string.Empty
            : psReadLineAvailable
                ? "  (PSReadLine non in uso: stdin rediretto)"
                : "  (PSReadLine non disponibile: editing di riga base)";
        Console.WriteLine("Lare Terminal " + HostInfo.Version + " — sessione " + _client.SessionId + suffix);
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
        // Segnale all'emulatore (§4.7), prima di qualunque altra cosa: costa nulla e non dipende
        // dal WS. Solo se lo stdout è la console vera (F5c, revisione finale piano 2b): con
        // stdout rediretto su file o su una pipe, la sequenza OSC finirebbe scritta come byte
        // grezzi nel file/nel processo a valle invece che interpretata da un emulatore — rumore,
        // non un segnale.
        if (!Console.IsOutputRedirected)
        {
            Console.Out.Write(Osc.Intercept(input));
            Console.Out.Flush();
        }

        if (!EnsureConnected())
        {
            Console.WriteLine("orchestratore non raggiungibile: comando ignorato");
            return;
        }

        _launcher.EnsureUi();   // self-heal (ruling 8): la finestra di output vive in ui.exe

        // NIENTE "using" qui: un CTS senza timer né registrazioni non possiede risorse native
        // da rilasciare, quindi ometterne il Dispose non perde nulla di reale — e questo evita
        // la race con l'handler di Ctrl+C (altro thread): se il turno finisce ed entra nel
        // finally proprio mentre l'handler legge _turnCts, con "using" l'handler potrebbe
        // chiamare Cancel() su un CTS già disposto (ObjectDisposedException). Senza "using",
        // Cancel() su un CTS già "consumato" ma non disposto è un semplice no-op innocuo.
        var cts = new CancellationTokenSource();
        _turnCts = cts;
        try
        {
            var turn = new SlashTurn(_client, new ConsoleGate(_log), _executor, Console.Out, _log);
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
