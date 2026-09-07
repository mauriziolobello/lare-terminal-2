using LareShell.Config;
using LareShell.Protocol;

namespace LareShell.Shell;

internal enum TurnResult
{
    Completed,
    Failed,
    Cancelled,
    Disconnected,
}

/// <summary>
/// Il ciclo di UN turno slash (spec §4.2), eseguito interamente sul thread del REPL: manda il
/// Command, poi consuma il canale dei messaggi in arrivo finché il turno non finisce. Il thread
/// del socket ha già accodato tutto in <c>OrchestratorClient.Incoming</c>; qui si consuma e si
/// reagisce — gate, esecuzione nel runspace, stampa — sempre da questo thread (§4.4).
/// Contratti (crates/protocol/IMPLEMENTATION.md): (a) un turno alla volta — garantito perché Run
/// blocca il REPL; (b) id = GUID; (c) fine al primo Done/Error; (d) turn_id echeggiato.
/// </summary>
internal sealed class SlashTurn
{
    private readonly OrchestratorClient _client;
    private readonly IGate _gate;
    private readonly IExecutor _executor;
    private readonly TextWriter _output;
    private readonly HostLog _log;

    public SlashTurn(OrchestratorClient client, IGate gate, IExecutor executor, TextWriter output, HostLog log)
    {
        _client = client;
        _gate = gate;
        _executor = executor;
        _output = output;
        _log = log;
    }

    /// <summary>Generatore dell'id del turno (contratto b). Sostituibile nei test.</summary>
    public Func<string> NewId { get; init; } = () => Guid.NewGuid().ToString("N");

    public TurnResult Run(string input, string cwd, CancellationToken ctrlC)
    {
        DiscardStale();

        string id = NewId();
        if (!_client.IsConnected || !_client.Send(Wire.Command(id, input, cwd)))
        {
            _output.WriteLine("orchestratore non raggiungibile: comando ignorato");
            return TurnResult.Disconnected;
        }

        _log.Info("turno " + id + " avviato: " + input);

        while (true)
        {
            ServerMessage msg;
            try
            {
                // Ponte bloccante sul canale: il thread del REPL dorme finché il socket non accoda
                // qualcosa o l'utente preme Ctrl+C (token cancellato dal Repl).
                msg = _client.Incoming.ReadAsync(ctrlC).AsTask().GetAwaiter().GetResult();
            }
            catch (OperationCanceledException)
            {
                return Cancel(id, "annullato (Ctrl+C)");
            }

            switch (msg)
            {
                case Chunk c when c.Id == id:
                    _output.WriteLine(c.Content);
                    break;

                case Done d when d.Id == id:
                    _log.Info("turno " + id + " completato");
                    return TurnResult.Completed;

                case TurnError e when e.Id == id:
                    _output.WriteLine("errore: " + e.Message);
                    _log.Warn("turno " + id + " in errore (" + e.Code + "): " + e.Message);
                    return TurnResult.Failed;

                case ToolConfirmRequest req:
                    // Attribuita al turno corrente (ruling 5: l'id della richiesta è opaco, ma un
                    // solo turno alla volta è in corso — contratto a).
                    switch (_gate.Ask(req.Commands, () => TerminalPending(id)))
                    {
                        case GateAnswer.Accept:
                            _client.Send(Wire.ToolConfirmResponse(req.Id, accept: true));
                            break;
                        case GateAnswer.Reject:
                            _client.Send(Wire.ToolConfirmResponse(req.Id, accept: false));
                            break;
                        case GateAnswer.Cancel:
                            return Cancel(id, "annullato (Ctrl+C)");
                        case GateAnswer.Abandoned:
                            break;   // il Done/Error in coda chiuderà il turno al prossimo giro
                    }

                    break;

                case ExecInShell ex when ex.TurnId == id:
                    ExecOutcome outcome;
                    try
                    {
                        outcome = _executor.Run(ex.Command, ex.Capture);
                    }
                    catch (Exception e)
                    {
                        // Il runspace può lanciare (host chiuso, pipeline in uno stato illegale, un
                        // bug nostro): se l'eccezione salisse, il REPL uscirebbe dal turno senza
                        // dire niente all'orchestratore, che resterebbe ad aspettare l'ExecResult
                        // fino al proprio timeout — turno appeso da entrambe le parti. Chiudiamo
                        // noi con un CancelCommand, come per il Ctrl+C durante l'esecuzione.
                        _log.Warn("esecuzione fallita nel turno " + id + ": " + e);
                        return Cancel(id, "errore nell'esecuzione: " + e.Message);
                    }

                    if (outcome.Stopped)
                    {
                        // §4.4: Ctrl+C durante l'exec = stop della pipeline E cancel del turno, mai un ExecResult parziale.
                        return Cancel(id, "comando interrotto (Ctrl+C): turno annullato");
                    }

                    // (d) eco del turn_id ricevuto, mai il nostro.
                    _client.Send(Wire.ExecResult(ex.TurnId, ex.ExecId, outcome.ExitCode, outcome.Output, outcome.Cwd));
                    break;

                case ExecInShell other:
                    // §8: si esegue SOLO ciò che appartiene al turno gateizzato in corso.
                    _log.Warn("ExecInShell per un altro turno (" + other.TurnId + " ≠ " + id + "): NON eseguito");
                    break;

                case Disconnected dc:
                    _output.WriteLine("connessione all'orchestratore persa (" + dc.Reason + "): turno interrotto");
                    return TurnResult.Disconnected;

                case Heartbeat or Pong or ServerInfo:
                    break;

                default:
                    _log.Debug("messaggio scartato durante il turno " + id + ": " + msg);
                    break;
            }
        }
    }

    private TurnResult Cancel(string id, string line)
    {
        _client.Send(Wire.CancelCommand(id));
        _output.WriteLine(line);
        _log.Info("turno " + id + " annullato");
        return TurnResult.Cancelled;
    }

    /// <summary>true se in TESTA alla coda c'è già la fine del turno (Done/Error con questo id) o
    /// la caduta della connessione: il gate smette di aspettare l'utente. Limite noto: TryPeek vede
    /// solo il primo messaggio — se prima del Done c'è un Chunk (l'ack di conferma), il prompt resta
    /// finché l'utente non preme un tasto; poi il turno si chiude normalmente.</summary>
    private bool TerminalPending(string id) =>
        _client.Incoming.TryPeek(out ServerMessage? next) &&
        (next is Done d && d.Id == id || next is TurnError e && e.Id == id || next is Disconnected);

    /// <summary>Scarta ciò che è rimasto in coda da turni già chiusi (contratto c, ruling 5).
    /// Un Disconnected non va "perso": IsConnected lo rende comunque visibile al chiamante.</summary>
    private void DiscardStale()
    {
        while (_client.Incoming.TryRead(out ServerMessage? stale))
        {
            _log.Debug("scartato fuori turno: " + stale);
        }
    }
}
