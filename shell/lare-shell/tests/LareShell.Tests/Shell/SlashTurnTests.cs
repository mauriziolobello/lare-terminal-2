using System.Text.Json;
using System.Text.Json.Nodes;
using LareShell.Config;
using LareShell.Protocol;
using LareShell.Shell;
using LareShell.Tests.Protocol;
using Xunit;

// xUnit1031 ("non usare operazioni bloccanti nei metodi di test") è spento per TUTTO il file, di
// proposito: qui il blocco È l'oggetto del test. SlashTurn.Run gira sul thread che lo chiama (il
// REPL, spec §4.4) e consuma il canale con ponti bloccanti; se i test diventassero `async Task`
// non verificherebbero più quel modello (nota esplicita del brief: non convertirli). Il deadlock
// che la regola previene qui non può accadere: il lato server è pilotato da Task.Run (thread del
// pool, senza SynchronizationContext) e OrchestratorClient usa ConfigureAwait(false) ovunque,
// quindi nessuna continuazione deve tornare sul thread bloccato.
#pragma warning disable xUnit1031

namespace LareShell.Tests.Shell;

internal sealed class FakeGate : IGate
{
    public Queue<GateAnswer> Answers { get; } = new();
    public List<string> Asked { get; } = new();
    public List<GateAnswer> Given { get; } = new();

    public GateAnswer Ask(string commands, Func<bool> shouldAbandon)
    {
        Asked.Add(commands);
        // Senza risposta pronta simula l'utente che non preme nulla: aspetta che il turno finisca.
        var deadline = DateTime.UtcNow.AddSeconds(5);
        while (Answers.Count == 0)
        {
            if (shouldAbandon())
            {
                Given.Add(GateAnswer.Abandoned);
                return GateAnswer.Abandoned;
            }

            if (DateTime.UtcNow > deadline) throw new TimeoutException("il gate finto non ha ricevuto né risposta né abbandono");
            Thread.Sleep(10);
        }

        GateAnswer a = Answers.Dequeue();
        Given.Add(a);
        return a;
    }
}

internal sealed class FakeExecutor : IExecutor
{
    public List<(string Command, bool Capture, int ThreadId)> Calls { get; } = new();
    public ExecOutcome Outcome { get; set; } = new(0, "fake-output", @"C:\x", Stopped: false);

    /// <summary>Se valorizzata, Run la lancia invece di tornare Outcome: simula un runspace rotto.</summary>
    public Exception? Throws { get; set; }

    public ExecOutcome Run(string command, bool capture)
    {
        Calls.Add((command, capture, Environment.CurrentManagedThreadId));
        return Throws is null ? Outcome : throw Throws;
    }

    public void StopCurrent() { }
}

public class SlashTurnTests
{
    private static CancellationToken Ct() => new CancellationTokenSource(TimeSpan.FromSeconds(10)).Token;

    private static string J(object o) => JsonSerializer.Serialize(o);

    /// <summary>Server finto + client vero già connessi (handshake fatto).</summary>
    private static (FakeOrchestrator Server, OrchestratorClient Client) Connected()
    {
        FakeOrchestrator server = FakeOrchestrator.Start();
        var client = new OrchestratorClient(server.Uri, () => "tok", "s1", "2.0.0", HostLog.Null);
        CancellationToken ct = Ct();
        // L'accept gira su un thread del pool (dove SynchronizationContext.Current è null), come il
        // resto del lato server nei test: il thread del test si blocca subito dopo dentro Connect e
        // non deve dipendere da come xUnit inoltra le continuazioni del proprio contesto.
        Task<JsonObject> accepted = Task.Run(() => server.AcceptAsync(ct));
        string? reason = client.Connect(@"C:\w", TimeSpan.FromSeconds(10));
        accepted.GetAwaiter().GetResult();
        Assert.Null(reason);
        return (server, client);
    }

    [Fact]
    public void Turno_completo_gate_exec_chunk_done()
    {
        // spec §4.2 + §10: Hello→Command→ToolConfirmRequest→ToolConfirmResponse→ExecInShell→ExecResult→Done
        (FakeOrchestrator server, OrchestratorClient client) = Connected();
        using (client)
        {
            var gate = new FakeGate();
            gate.Answers.Enqueue(GateAnswer.Accept);
            var executor = new FakeExecutor();
            var output = new StringWriter();
            var turn = new SlashTurn(client, gate, executor, output, HostLog.Null) { NewId = () => "turno-1" };
            CancellationToken ct = Ct();

            Task<(JsonObject Cmd, JsonObject Confirm, JsonObject Result)> serverSide = Task.Run(async () =>
            {
                JsonObject cmd = await server.ReceiveAsync(ct);
                await server.SendAsync(J(new { type = "tool_confirm_request", id = "g1", commands = "Get-Date" }), ct);
                JsonObject confirm = await server.ReceiveAsync(ct);
                await server.SendAsync(J(new { type = "exec_in_shell", turn_id = "turno-1", exec_id = "e1", command = "Get-Date", capture = true }), ct);
                JsonObject result = await server.ReceiveAsync(ct);
                await server.SendAsync(J(new { type = "chunk", id = "turno-1", content = "→ finestra \"x\" aperta" }), ct);
                await server.SendAsync(J(new { type = "done", id = "turno-1", exit_code = (int?)null }), ct);
                return (cmd, confirm, result);
            });

            int replThread = Environment.CurrentManagedThreadId;
            TurnResult r = turn.Run("/ai \"x\"", @"C:\w", CancellationToken.None);
            (JsonObject cmd, JsonObject confirm, JsonObject result) = serverSide.GetAwaiter().GetResult();

            Assert.Equal(TurnResult.Completed, r);
            Assert.Equal("command", (string?)cmd["type"]);
            Assert.Equal("turno-1", (string?)cmd["id"]);
            Assert.Equal("/ai \"x\"", (string?)cmd["input"]);
            Assert.Equal(@"C:\w", (string?)cmd["cwd"]);
            Assert.Equal("tool_confirm_response", (string?)confirm["type"]);
            Assert.Equal("g1", (string?)confirm["id"]);
            Assert.True((bool?)confirm["accept"]);
            Assert.Equal(new[] { "Get-Date" }, gate.Asked);
            // exec_result: eco del turn_id (contratto d), exec_id, esito del finto executor
            Assert.Equal("exec_result", (string?)result["type"]);
            Assert.Equal("turno-1", (string?)result["turn_id"]);
            Assert.Equal("e1", (string?)result["exec_id"]);
            Assert.Equal(0, (int?)result["exit_code"]);
            Assert.Equal("fake-output", (string?)result["output"]);
            Assert.Equal(@"C:\x", (string?)result["cwd"]);
            // marshaling (spec §10): l'ExecInShell arrivato dal thread del socket è eseguito dal thread che ha chiamato Run
            (string command, bool capture, int threadId) = Assert.Single(executor.Calls);
            Assert.Equal(("Get-Date", true, replThread), (command, capture, threadId));
            Assert.Contains("→ finestra \"x\" aperta", output.ToString());
            server.DisposeAsync().AsTask().GetAwaiter().GetResult();
        }
    }

    [Fact]
    public void Rifiuto_al_gate_manda_accept_false()
    {
        (FakeOrchestrator server, OrchestratorClient client) = Connected();
        using (client)
        {
            var gate = new FakeGate();
            gate.Answers.Enqueue(GateAnswer.Reject);
            var executor = new FakeExecutor();
            var turn = new SlashTurn(client, gate, executor, TextWriter.Null, HostLog.Null) { NewId = () => "t" };
            CancellationToken ct = Ct();

            Task<JsonObject> serverSide = Task.Run(async () =>
            {
                await server.ReceiveAsync(ct);
                await server.SendAsync(J(new { type = "tool_confirm_request", id = "g1", commands = "Remove-Item x" }), ct);
                JsonObject confirm = await server.ReceiveAsync(ct);
                await server.SendAsync(J(new { type = "done", id = "t", exit_code = (int?)null }), ct);
                return confirm;
            });

            Assert.Equal(TurnResult.Completed, turn.Run("/ai \"x\"", @"C:\w", CancellationToken.None));
            Assert.False((bool?)serverSide.GetAwaiter().GetResult()["accept"]);
            // Sanity check, non la tesi del test: qui il server non manda alcun exec_in_shell, quindi
            // questa riga non dimostra che un rifiuto blocchi l'esecuzione (lo decide l'orchestratore).
            Assert.Empty(executor.Calls);
            server.DisposeAsync().AsTask().GetAwaiter().GetResult();
        }
    }

    [Fact]
    public void Ctrl_C_in_attesa_manda_cancel_command_e_torna_Cancelled()
    {
        // spec §4.4: "Durante l'attesa del turno: la host manda CancelCommand{id}, stampa una riga, torna al prompt"
        (FakeOrchestrator server, OrchestratorClient client) = Connected();
        using (client)
        {
            var output = new StringWriter();
            var turn = new SlashTurn(client, new FakeGate(), new FakeExecutor(), output, HostLog.Null) { NewId = () => "t" };
            CancellationToken ct = Ct();
            using var ctrlC = new CancellationTokenSource();

            Task<JsonObject> serverSide = Task.Run(async () =>
            {
                await server.ReceiveAsync(ct);          // command: l'orchestratore "ci pensa"…
                return await server.ReceiveAsync(ct);   // …e riceve il cancel
            });
            Task.Run(async () => { await Task.Delay(200); ctrlC.Cancel(); });

            Assert.Equal(TurnResult.Cancelled, turn.Run("/ai \"x\"", @"C:\w", ctrlC.Token));
            JsonObject cancel = serverSide.GetAwaiter().GetResult();
            Assert.Equal("cancel_command", (string?)cancel["type"]);
            Assert.Equal("t", (string?)cancel["id"]);
            Assert.Contains("annullato", output.ToString());
            server.DisposeAsync().AsTask().GetAwaiter().GetResult();
        }
    }

    [Fact]
    public void Error_chiude_il_turno_con_Failed_e_stampa_il_messaggio()
    {
        (FakeOrchestrator server, OrchestratorClient client) = Connected();
        using (client)
        {
            var output = new StringWriter();
            var turn = new SlashTurn(client, new FakeGate(), new FakeExecutor(), output, HostLog.Null) { NewId = () => "t" };
            CancellationToken ct = Ct();
            Task serverSide = Task.Run(async () =>
            {
                await server.ReceiveAsync(ct);
                await server.SendAsync(J(new { type = "error", id = "t", code = "routing_error", message = "sintassi: /ai \"testo\"" }), ct);
            });

            Assert.Equal(TurnResult.Failed, turn.Run("/ai x", @"C:\w", CancellationToken.None));
            serverSide.GetAwaiter().GetResult();
            Assert.Contains("sintassi: /ai \"testo\"", output.ToString());
            server.DisposeAsync().AsTask().GetAwaiter().GetResult();
        }
    }

    [Fact]
    public void Messaggi_di_altri_turni_vengono_scartati()
    {
        // contratto (c): un Done di un turno già chiuso/diverso non chiude quello corrente
        (FakeOrchestrator server, OrchestratorClient client) = Connected();
        using (client)
        {
            var executor = new FakeExecutor();
            var output = new StringWriter();
            var turn = new SlashTurn(client, new FakeGate(), executor, output, HostLog.Null) { NewId = () => "t" };
            CancellationToken ct = Ct();
            Task serverSide = Task.Run(async () =>
            {
                await server.ReceiveAsync(ct);
                await server.SendAsync(J(new { type = "done", id = "vecchio", exit_code = (int?)null }), ct);
                // Il chunk DOPO il done estraneo è la vera guardia: se un done qualsiasi chiudesse il
                // turno, questa riga non verrebbe mai stampata (e nemmeno l'exec_in_shell letto).
                await server.SendAsync(J(new { type = "chunk", id = "t", content = "ancora-vivo" }), ct);
                await server.SendAsync(J(new { type = "exec_in_shell", turn_id = "altro", exec_id = "e9", command = "Remove-Item x", capture = true }), ct);
                await server.SendAsync(J(new { type = "heartbeat", id = "t" }), ct);
                await server.SendAsync(J(new { type = "done", id = "t", exit_code = (int?)null }), ct);
            });

            Assert.Equal(TurnResult.Completed, turn.Run("/ping", @"C:\w", CancellationToken.None));
            serverSide.GetAwaiter().GetResult();
            Assert.Contains("ancora-vivo", output.ToString());   // il done di "vecchio" NON ha chiuso il turno
            Assert.Empty(executor.Calls);   // §8: l'ExecInShell di un altro turno NON viene eseguito
            server.DisposeAsync().AsTask().GetAwaiter().GetResult();
        }
    }

    [Fact]
    public void Exec_fermato_da_Ctrl_C_cancella_il_turno_senza_ExecResult()
    {
        (FakeOrchestrator server, OrchestratorClient client) = Connected();
        using (client)
        {
            var gate = new FakeGate();
            gate.Answers.Enqueue(GateAnswer.Accept);
            var executor = new FakeExecutor { Outcome = new ExecOutcome(130, "", @"C:\w", Stopped: true) };
            var turn = new SlashTurn(client, gate, executor, TextWriter.Null, HostLog.Null) { NewId = () => "t" };
            CancellationToken ct = Ct();
            Task<JsonObject> serverSide = Task.Run(async () =>
            {
                await server.ReceiveAsync(ct);
                await server.SendAsync(J(new { type = "tool_confirm_request", id = "g1", commands = "Start-Sleep 99" }), ct);
                await server.ReceiveAsync(ct);   // accept
                await server.SendAsync(J(new { type = "exec_in_shell", turn_id = "t", exec_id = "e1", command = "Start-Sleep 99", capture = true }), ct);
                return await server.ReceiveAsync(ct);   // deve essere cancel_command, NON exec_result
            });

            Assert.Equal(TurnResult.Cancelled, turn.Run("/ai \"x\"", @"C:\w", CancellationToken.None));
            Assert.Equal("cancel_command", (string?)serverSide.GetAwaiter().GetResult()["type"]);
            server.DisposeAsync().AsTask().GetAwaiter().GetResult();
        }
    }

    [Fact]
    public void Eccezione_dell_executor_cancella_il_turno()
    {
        // Se il runspace lancia, il turno NON deve restare appeso lato orchestratore ad aspettare
        // un ExecResult che non arriverà mai: la host chiude con un CancelCommand e lo dice a video.
        (FakeOrchestrator server, OrchestratorClient client) = Connected();
        using (client)
        {
            var gate = new FakeGate();
            gate.Answers.Enqueue(GateAnswer.Accept);
            var executor = new FakeExecutor { Throws = new InvalidOperationException("runspace rotto") };
            var output = new StringWriter();
            var turn = new SlashTurn(client, gate, executor, output, HostLog.Null) { NewId = () => "t" };
            CancellationToken ct = Ct();
            Task<JsonObject> serverSide = Task.Run(async () =>
            {
                await server.ReceiveAsync(ct);
                await server.SendAsync(J(new { type = "tool_confirm_request", id = "g1", commands = "Get-Date" }), ct);
                await server.ReceiveAsync(ct);   // accept
                await server.SendAsync(J(new { type = "exec_in_shell", turn_id = "t", exec_id = "e1", command = "Get-Date", capture = true }), ct);
                return await server.ReceiveAsync(ct);   // deve essere cancel_command, NON exec_result
            });

            Assert.Equal(TurnResult.Cancelled, turn.Run("/ai \"x\"", @"C:\w", CancellationToken.None));
            JsonObject cancel = serverSide.GetAwaiter().GetResult();
            Assert.Equal("cancel_command", (string?)cancel["type"]);
            Assert.Equal("t", (string?)cancel["id"]);
            Assert.Contains("runspace rotto", output.ToString());
            server.DisposeAsync().AsTask().GetAwaiter().GetResult();
        }
    }

    [Fact]
    public void Caduta_della_connessione_a_meta_turno_torna_Disconnected()
    {
        (FakeOrchestrator server, OrchestratorClient client) = Connected();
        using (client)
        {
            var output = new StringWriter();
            var turn = new SlashTurn(client, new FakeGate(), new FakeExecutor(), output, HostLog.Null) { NewId = () => "t" };
            CancellationToken ct = Ct();
            Task serverSide = Task.Run(async () =>
            {
                await server.ReceiveAsync(ct);
                await server.CloseAsync();
            });

            Assert.Equal(TurnResult.Disconnected, turn.Run("/ping", @"C:\w", CancellationToken.None));
            serverSide.GetAwaiter().GetResult();
            Assert.Contains("orchestratore", output.ToString());
            server.DisposeAsync().AsTask().GetAwaiter().GetResult();
        }
    }

    [Fact]
    public void Il_gate_viene_abbandonato_se_in_coda_c_e_gia_l_Error_del_turno()
    {
        // Timeout 180 s lato orchestratore (spec §4.3): la richiesta scade e arriva Done/Error mentre
        // l'utente non ha ancora risposto → il prompt si chiude da solo, senza restare appeso.
        (FakeOrchestrator server, OrchestratorClient client) = Connected();
        using (client)
        {
            var gate = new FakeGate();   // nessuna risposta in coda: aspetta shouldAbandon
            var turn = new SlashTurn(client, gate, new FakeExecutor(), TextWriter.Null, HostLog.Null) { NewId = () => "t" };
            CancellationToken ct = Ct();
            Task serverSide = Task.Run(async () =>
            {
                await server.ReceiveAsync(ct);
                await server.SendAsync(J(new { type = "tool_confirm_request", id = "g1", commands = "x" }), ct);
                await server.SendAsync(J(new { type = "error", id = "t", code = "ai_error", message = "conferma scaduta" }), ct);
            });

            Assert.Equal(TurnResult.Failed, turn.Run("/ai \"x\"", @"C:\w", CancellationToken.None));
            serverSide.GetAwaiter().GetResult();
            Assert.Equal(new[] { GateAnswer.Abandoned }, gate.Given);
            server.DisposeAsync().AsTask().GetAwaiter().GetResult();
        }
    }

    [Fact]
    public void Il_gate_viene_abbandonato_se_in_coda_ci_sono_ack_e_Done_del_turno()
    {
        // F3 (revisione finale piano 2b): per un turno gateizzato l'orchestratore (surface.rs,
        // route_shell_turn) manda alla shell UN SOLO Chunk di ack ("→ finestra aperta") seguito
        // subito dal Done — mai un altro Chunk di testo nel mezzo (il testo dell'AI finisce nella
        // finestra ui, non nella shell). Quindi trovare quell'ack in TESTA alla coda (TryPeek)
        // basta per sapere che il turno sta per chiudersi: il gate deve abbandonare SUBITO,
        // invece di aspettare un tasto dell'utente fino al Done stesso.
        (FakeOrchestrator server, OrchestratorClient client) = Connected();
        using (client)
        {
            var gate = new FakeGate();   // nessuna risposta in coda: aspetta shouldAbandon
            var output = new StringWriter();
            var turn = new SlashTurn(client, gate, new FakeExecutor(), output, HostLog.Null) { NewId = () => "t" };
            CancellationToken ct = Ct();
            Task serverSide = Task.Run(async () =>
            {
                await server.ReceiveAsync(ct);
                await server.SendAsync(J(new { type = "tool_confirm_request", id = "g1", commands = "x" }), ct);
                await server.SendAsync(J(new { type = "chunk", id = "t", content = "→ finestra aperta" }), ct);
                await server.SendAsync(J(new { type = "done", id = "t", exit_code = (int?)null }), ct);
            });

            Assert.Equal(TurnResult.Completed, turn.Run("/ai \"x\"", @"C:\w", CancellationToken.None));
            serverSide.GetAwaiter().GetResult();
            Assert.Equal(new[] { GateAnswer.Abandoned }, gate.Given);
            Assert.Contains("→ finestra aperta", output.ToString());
            server.DisposeAsync().AsTask().GetAwaiter().GetResult();
        }
    }

    [Fact]
    public void Senza_connessione_Run_torna_Disconnected_senza_bloccare()
    {
        // NB (scostamento minimo dal brief): FakeOrchestrator è IAsyncDisposable, non IDisposable,
        // quindi qui niente `using` — si chiude in coda come negli altri test di questo file.
        FakeOrchestrator server = FakeOrchestrator.Start();
        using var client = new OrchestratorClient(server.Uri, () => "tok", "s1", "2.0.0", HostLog.Null);   // mai connesso
        var output = new StringWriter();
        var turn = new SlashTurn(client, new FakeGate(), new FakeExecutor(), output, HostLog.Null);
        Assert.Equal(TurnResult.Disconnected, turn.Run("/ping", @"C:\w", CancellationToken.None));
        Assert.Contains("non raggiungibile", output.ToString());
        server.DisposeAsync().AsTask().GetAwaiter().GetResult();
    }
}
