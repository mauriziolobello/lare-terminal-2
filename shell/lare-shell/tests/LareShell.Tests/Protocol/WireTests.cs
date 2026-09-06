using System.Text.Json.Nodes;
using LareShell.Protocol;
using Xunit;

namespace LareShell.Tests.Protocol;

public class WireParseTests
{
    [Fact]
    public void Parse_server_info()
    {
        ServerMessage m = Wire.Parse("{\"type\":\"server_info\",\"version\":\"2.1.0\",\"ai_provider\":\"anthropic\",\"capabilities\":[\"a\"]}");
        ServerInfo info = Assert.IsType<ServerInfo>(m);
        Assert.Equal("2.1.0", info.Version);
        Assert.Equal("anthropic", info.AiProvider);
    }

    [Fact]
    public void Parse_chunk_done_error()
    {
        Chunk c = Assert.IsType<Chunk>(Wire.Parse("{\"type\":\"chunk\",\"id\":\"t1\",\"content\":\"ciao\"}"));
        Assert.Equal(("t1", "ciao"), (c.Id, c.Content));

        Done d = Assert.IsType<Done>(Wire.Parse("{\"type\":\"done\",\"id\":\"t1\",\"exit_code\":null}"));
        Assert.Equal("t1", d.Id);
        Assert.Null(d.ExitCode);

        Done d2 = Assert.IsType<Done>(Wire.Parse("{\"type\":\"done\",\"id\":\"t1\",\"exit_code\":3}"));
        Assert.Equal(3, d2.ExitCode);

        TurnError e = Assert.IsType<TurnError>(Wire.Parse("{\"type\":\"error\",\"id\":\"t1\",\"code\":\"routing_error\",\"message\":\"boom\"}"));
        Assert.Equal(("t1", "routing_error", "boom"), (e.Id, e.Code, e.Message));
    }

    [Fact]
    public void Parse_tool_confirm_request_ed_exec_in_shell()
    {
        ToolConfirmRequest r = Assert.IsType<ToolConfirmRequest>(Wire.Parse("{\"type\":\"tool_confirm_request\",\"id\":\"g1\",\"commands\":\"Get-Date\"}"));
        Assert.Equal(("g1", "Get-Date"), (r.Id, r.Commands));

        ExecInShell x = Assert.IsType<ExecInShell>(Wire.Parse("{\"type\":\"exec_in_shell\",\"turn_id\":\"t1\",\"exec_id\":\"e1\",\"command\":\"dir\",\"capture\":false}"));
        Assert.Equal(("t1", "e1", "dir", false), (x.TurnId, x.ExecId, x.Command, x.Capture));
    }

    [Fact]
    public void Parse_heartbeat_e_pong()
    {
        Assert.Equal("t1", Assert.IsType<Heartbeat>(Wire.Parse("{\"type\":\"heartbeat\",\"id\":\"t1\"}")).Id);
        Assert.Equal(42L, Assert.IsType<Pong>(Wire.Parse("{\"type\":\"pong\",\"ts\":42}")).Ts);
    }

    [Fact]
    public void Parse_tipo_sconosciuto_da_Unknown_con_il_nome_del_tipo()
    {
        // Un messaggio v1 che la shell non gestisce (es. cwd, open_window) non deve far cadere
        // il loop di ricezione: viene consegnato come Unknown e il consumatore lo ignora.
        Unknown u = Assert.IsType<Unknown>(Wire.Parse("{\"type\":\"open_window\",\"kind\":\"markdown\"}"));
        Assert.Equal("open_window", u.Type);
    }

    [Fact]
    public void Parse_ignora_campi_extra()
    {
        Chunk c = Assert.IsType<Chunk>(Wire.Parse("{\"type\":\"chunk\",\"id\":\"t1\",\"content\":\"x\",\"extra\":1}"));
        Assert.Equal("x", c.Content);
    }

    [Theory]
    [InlineData("non json")]
    [InlineData("{\"id\":\"t1\"}")]                       // manca type
    [InlineData("{\"type\":\"chunk\",\"id\":\"t1\"}")]     // manca content
    [InlineData("[1,2]")]                                  // non un oggetto
    [InlineData("{\"type\":\"done\",\"id\":\"t\",\"exit_code\":\"x\"}")]  // exit_code malformato
    [InlineData("{\"type\":\"pong\"}")]                   // manca ts
    public void Parse_invalido_lancia_WireException(string json)
    {
        Assert.Throws<WireException>(() => Wire.Parse(json));
    }
}

public class WireSerializeTests
{
    private static JsonObject Obj(string json) => (JsonObject)JsonNode.Parse(json)!;

    [Fact]
    public void Hello_ha_role_shell_session_cwd_e_version()
    {
        JsonObject o = Obj(Wire.Hello("tok", "s1", @"C:\x", "2.0.0"));
        Assert.Equal("hello", (string?)o["type"]);
        Assert.Equal("tok", (string?)o["token"]);
        Assert.Equal("shell", (string?)o["role"]);
        Assert.Equal("s1", (string?)o["session_id"]);
        Assert.Equal(@"C:\x", (string?)o["cwd"]);
        Assert.Equal("2.0.0", (string?)o["version"]);
        // `channel` assente o null: la connessione shell non chiede un canale esterno.
        Assert.True(o["channel"] is null);
    }

    [Fact]
    public void Command_ha_i_campi_obbligatori_v1_con_i_valori_fissi()
    {
        JsonObject o = Obj(Wire.Command("id1", "/ai \"x\"", @"C:\x"));
        Assert.Equal("command", (string?)o["type"]);
        Assert.Equal("id1", (string?)o["id"]);
        Assert.Equal("/ai \"x\"", (string?)o["input"]);
        // input_mode e command_type NON hanno default lato Rust: vanno sempre inviati.
        Assert.Equal("keyboard", (string?)o["input_mode"]);
        Assert.Equal("auto", (string?)o["command_type"]);
        Assert.Equal(@"C:\x", (string?)o["cwd"]);
        Assert.False((bool?)o["web_search"]);
    }

    [Fact]
    public void ToolConfirmResponse_ExecResult_CancelCommand_Ping()
    {
        JsonObject a = Obj(Wire.ToolConfirmResponse("g1", true));
        Assert.Equal("tool_confirm_response", (string?)a["type"]);
        Assert.Equal("g1", (string?)a["id"]);
        Assert.True((bool?)a["accept"]);

        JsonObject r = Obj(Wire.ExecResult("t1", "e1", 2, "out", @"C:\y"));
        Assert.Equal("exec_result", (string?)r["type"]);
        Assert.Equal("t1", (string?)r["turn_id"]);
        Assert.Equal("e1", (string?)r["exec_id"]);
        Assert.Equal(2, (int?)r["exit_code"]);
        Assert.Equal("out", (string?)r["output"]);
        Assert.Equal(@"C:\y", (string?)r["cwd"]);

        JsonObject c = Obj(Wire.CancelCommand("id1"));
        Assert.Equal("cancel_command", (string?)c["type"]);
        Assert.Equal("id1", (string?)c["id"]);

        JsonObject p = Obj(Wire.Ping(7));
        Assert.Equal("ping", (string?)p["type"]);
        Assert.Equal(7L, (long?)p["ts"]);
    }

    [Fact]
    public void Serializzazione_preserva_unicode_e_ritorni_a_capo()
    {
        JsonObject r = Obj(Wire.ExecResult("t", "e", 0, "è\r\nà", @"C:\"));
        Assert.Equal("è\r\nà", (string?)r["output"]);
    }
}
