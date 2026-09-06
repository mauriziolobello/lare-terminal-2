using System.Text.Json.Nodes;
using LareShell.Config;
using LareShell.Protocol;
using Xunit;

namespace LareShell.Tests.Protocol;

public class OrchestratorClientTests
{
    private static CancellationToken Timeout() => new CancellationTokenSource(TimeSpan.FromSeconds(10)).Token;

    private static OrchestratorClient NewClient(FakeOrchestrator server, string token = "tok") =>
        new(server.Uri, () => token, "sess1", "2.0.0", HostLog.Null);

    [Fact]
    public async Task Connect_manda_hello_con_role_shell_e_riesce_su_server_info()
    {
        CancellationToken ct = Timeout();
        await using FakeOrchestrator server = FakeOrchestrator.Start(expectedToken: "tok");
        using OrchestratorClient client = NewClient(server);

        Task<JsonObject> accepted = server.AcceptAsync(ct);
        string? reason = await client.ConnectAsync(@"C:\cwd", ct);
        JsonObject hello = await accepted;

        Assert.Null(reason);
        Assert.True(client.IsConnected);
        Assert.Equal("hello", (string?)hello["type"]);
        Assert.Equal("shell", (string?)hello["role"]);
        Assert.Equal("sess1", (string?)hello["session_id"]);
        Assert.Equal(@"C:\cwd", (string?)hello["cwd"]);
        Assert.Equal("2.0.0", (string?)hello["version"]);
    }

    [Fact]
    public async Task Connect_fallisce_con_motivo_se_il_token_e_rifiutato()
    {
        CancellationToken ct = Timeout();
        await using FakeOrchestrator server = FakeOrchestrator.Start(expectedToken: "buono");
        using OrchestratorClient client = NewClient(server, token: "sbagliato");

        Task<JsonObject> accepted = server.AcceptAsync(ct);
        string? reason = await client.ConnectAsync(@"C:\", ct);
        await accepted;

        Assert.NotNull(reason);
        Assert.False(client.IsConnected);
    }

    [Fact]
    public async Task Connect_fallisce_con_motivo_se_nessuno_ascolta()
    {
        // Porta presa e subito rilasciata: quasi certamente nessuno ci ascolta.
        await using FakeOrchestrator probe = FakeOrchestrator.Start();
        Uri uri = probe.Uri;
        await probe.DisposeAsync();

        using var client = new OrchestratorClient(uri, () => "tok", "s", "2.0.0", HostLog.Null);
        string? reason = await client.ConnectAsync(@"C:\", Timeout());
        Assert.NotNull(reason);
        Assert.False(client.IsConnected);
    }

    [Fact]
    public async Task Connect_fallisce_se_il_token_non_e_disponibile()
    {
        await using FakeOrchestrator server = FakeOrchestrator.Start();
        using var client = new OrchestratorClient(server.Uri, () => null, "s", "2.0.0", HostLog.Null);
        string? reason = await client.ConnectAsync(@"C:\", Timeout());
        Assert.Contains("token", reason, StringComparison.OrdinalIgnoreCase);
    }

    [Fact]
    public async Task Incoming_consegna_i_messaggi_parsati_in_ordine()
    {
        CancellationToken ct = Timeout();
        await using FakeOrchestrator server = FakeOrchestrator.Start();
        using OrchestratorClient client = NewClient(server);
        Task<JsonObject> accepted = server.AcceptAsync(ct);
        Assert.Null(await client.ConnectAsync(@"C:\", ct));
        await accepted;

        await server.SendAsync("{\"type\":\"chunk\",\"id\":\"t\",\"content\":\"a\"}", ct);
        await server.SendAsync("{\"type\":\"exec_in_shell\",\"turn_id\":\"t\",\"exec_id\":\"e\",\"command\":\"dir\",\"capture\":true}", ct);
        await server.SendAsync("{\"type\":\"done\",\"id\":\"t\",\"exit_code\":null}", ct);

        Assert.IsType<Chunk>(await client.Incoming.ReadAsync(ct));
        Assert.IsType<ExecInShell>(await client.Incoming.ReadAsync(ct));
        Assert.IsType<Done>(await client.Incoming.ReadAsync(ct));
    }

    [Fact]
    public async Task Un_messaggio_spezzato_in_due_frame_viene_riassemblato()
    {
        CancellationToken ct = Timeout();
        await using FakeOrchestrator server = FakeOrchestrator.Start();
        using OrchestratorClient client = NewClient(server);
        Task<JsonObject> accepted = server.AcceptAsync(ct);
        Assert.Null(await client.ConnectAsync(@"C:\", ct));
        await accepted;

        string content = new('x', 50_000);
        await server.SendFragmentedAsync("{\"type\":\"chunk\",\"id\":\"t\",\"content\":\"" + content + "\"}", splitAt: 20_000, ct);

        Chunk c = Assert.IsType<Chunk>(await client.Incoming.ReadAsync(ct));
        Assert.Equal(content, c.Content);
    }

    [Fact]
    public async Task Send_arriva_al_server()
    {
        CancellationToken ct = Timeout();
        await using FakeOrchestrator server = FakeOrchestrator.Start();
        using OrchestratorClient client = NewClient(server);
        Task<JsonObject> accepted = server.AcceptAsync(ct);
        Assert.Null(await client.ConnectAsync(@"C:\", ct));
        await accepted;

        Assert.True(await client.SendAsync(Wire.Command("id1", "/ping", @"C:\"), ct));
        JsonObject got = await server.ReceiveAsync(ct);
        Assert.Equal("command", (string?)got["type"]);
        Assert.Equal("/ping", (string?)got["input"]);
    }

    [Fact]
    public async Task Send_senza_connessione_ritorna_false_senza_lanciare()
    {
        await using FakeOrchestrator server = FakeOrchestrator.Start();
        using OrchestratorClient client = NewClient(server);
        Assert.False(await client.SendAsync(Wire.Ping(1), Timeout()));
    }

    [Fact]
    public async Task Chiusura_dal_server_accoda_Disconnected_e_IsConnected_diventa_false()
    {
        CancellationToken ct = Timeout();
        await using FakeOrchestrator server = FakeOrchestrator.Start();
        using OrchestratorClient client = NewClient(server);
        Task<JsonObject> accepted = server.AcceptAsync(ct);
        Assert.Null(await client.ConnectAsync(@"C:\", ct));
        await accepted;

        await server.CloseAsync();

        Disconnected d = Assert.IsType<Disconnected>(await client.Incoming.ReadAsync(ct));
        Assert.False(string.IsNullOrEmpty(d.Reason));
        Assert.False(client.IsConnected);
    }

    [Fact]
    public async Task Tipo_sconosciuto_e_JSON_rotto_non_fermano_il_loop()
    {
        CancellationToken ct = Timeout();
        await using FakeOrchestrator server = FakeOrchestrator.Start();
        using OrchestratorClient client = NewClient(server);
        Task<JsonObject> accepted = server.AcceptAsync(ct);
        Assert.Null(await client.ConnectAsync(@"C:\", ct));
        await accepted;

        await server.SendAsync("{\"type\":\"cwd\",\"path\":\"C:\\\\\"}", ct);   // messaggio v1 per la ui
        await server.SendAsync("questo non è json", ct);                      // scartato con log
        await server.SendAsync("{\"type\":\"heartbeat\",\"id\":\"t\"}", ct);

        Assert.Equal("cwd", Assert.IsType<Unknown>(await client.Incoming.ReadAsync(ct)).Type);
        Assert.IsType<Heartbeat>(await client.Incoming.ReadAsync(ct));
    }

    [Fact]
    public async Task Riconnessione_dopo_una_caduta_rifa_hello_con_la_stessa_sessione()
    {
        CancellationToken ct = Timeout();
        await using FakeOrchestrator server = FakeOrchestrator.Start();
        using OrchestratorClient client = NewClient(server);

        Task<JsonObject> first = server.AcceptAsync(ct);
        Assert.Null(await client.ConnectAsync(@"C:\", ct));
        await first;
        await server.CloseAsync();
        Assert.IsType<Disconnected>(await client.Incoming.ReadAsync(ct));

        Task<JsonObject> second = server.AcceptAsync(ct);
        Assert.Null(await client.ConnectAsync(@"C:\altro", ct));
        JsonObject hello2 = await second;
        Assert.Equal("sess1", (string?)hello2["session_id"]);
        Assert.Equal(@"C:\altro", (string?)hello2["cwd"]);
        Assert.True(client.IsConnected);
    }
}
